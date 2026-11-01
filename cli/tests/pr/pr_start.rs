//! Starting a PR (contract §3.8, §7.2; commands.md §4.8, §9.7): `plan lock`
//! starts the PRs without needs, all or nothing; engine E7 starts a `waiting`
//! PR once its needs are merged; `pr start` retries a failed start; R35's
//! remote rules apply to each PR's Lead Branch. In-process on `run_with` over the fake ports: the git route with
//! repos and bare `origin`s, the engine on `watch::run_with` over the `ManualClock`, and a Plan written straight
//! into the database.

use crate::common;

use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use common::fx::{Fx as Kit, exit_code};
use common::{Run, empty_comments};
use common::refused;
use cpd_core::clock::Clock;
use cpd_core::host::runner::Reply;
use cpd_core::ports::Ports;
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

/// The Plan `pay`'s env: the [`Kit`] (`env`, `git`, `calls`, `watch_ticks`, ...) it derefs to.
struct Fx {
    kit: Kit,
    /// Numbers the second clones of [`Fx::land_on_origin`].
    clones: std::cell::Cell<usize>,
}

impl Deref for Fx {
    type Target = Kit;

    fn deref(&self) -> &Kit {
        &self.kit
    }
}

/// The clock of eight `cpd` commands on threads of one process: the store's setup lock is held by one of them while the others
/// wait for it, and a wait on the `ManualClock` passes at once (the 5 s `store_busy_ms` in an instant), so a nap here only
/// yields the processor, and the time stands still until the holder is done.
struct Yielding(Arc<dyn Clock>);

impl Clock for Yielding {
    fn now(&self) -> std::time::Instant {
        self.0.now()
    }

    fn sleep(&self, _: std::time::Duration) {
        std::thread::yield_now();
    }

    fn utc(&self) -> time::OffsetDateTime {
        self.0.utc()
    }
}

/// A repo with its bare `origin`.
struct Repo {
    path: PathBuf,
    origin: PathBuf,
}

/// The poller's search answering no pull requests.
const EMPTY_SEARCH: &str = r#"{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}"#;

/// What a fake `cpd watch --ensure` prints when the engine started (`plan lock` runs it).
const WATCH_OK: &str = r#"{"ok": true, "started": true, "pane": "%9"}"#;

/// The env with its config, a `cpd` whose `watch --ensure` succeeds, Context `ctx` and Plan `pay` in phase `plan`.
fn fx() -> Fx {
    let kit = Kit::with_ports();
    kit.env.write_config(&format!(
        "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n[watch]\ntick_seconds = 1\npr_poll_seconds = 3600\n[tmux]\nsocket = \"{}\"\n",
        kit.env.tmux_socket()
    ));
    kit.env.quiet_models(cpd_core::clock::Clock::utc(&*kit.fakes.clock));
    let fx = Fx { kit, clones: std::cell::Cell::new(0) };
    fx.env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pay', 'ctx', 'Pay the bills', 'plan', 'lead1', 't');",
    );
    fx
}

impl Fx {
    fn events(&self, kind: &str) -> Vec<Value> {
        self.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
    }

    /// A repo `name` on `main` with a bare `origin` holding `main`.
    fn repo(&self, name: &str) -> Repo {
        let path = self.keyed_repo(name).canonicalize().unwrap();
        let origin = self.env.root().join("origins").join(format!("{name}.git"));
        self.git(self.env.root(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
        self.local_origin(&path, &origin);
        self.git(&path, &["push", "-q", "origin", "main"]);
        Repo { path, origin }
    }

    /// `pr add <key> --repo <repo> [--needs <needs>]`.
    fn add(&self, key: &str, repo: &Path, needs: &[&str]) {
        let needs = needs.join(",");
        let mut args = vec!["pr", "add", key, "--repo", repo.to_str().unwrap(), "--title", "Change the PR", "--shape", "config", "--plan", "pay"];
        if !needs.is_empty() {
            args.extend(["--needs", &needs]);
        } else {
            args.push("--no-needs");
        }
        self.cpd(&args).ok();
        self.cancel_stub(key, "pay");
    }

    /// Cancels the stub draft `<pr>-config` of the shape: these tests add the Jobs they need, as a Lead cancels a stub it does not need.
    fn cancel_stub(&self, pr: &str, plan: &str) {
        self.cpd(&["job", "cancel", &format!("{pr}-config"), "--reason", "a stub of the template", "--plan", plan]).ok();
    }

    /// A draft Job of PR `pr`.
    fn job(&self, key: &str, pr: &str) {
        self.env.sql(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('pay', '{pr}', '{key}', 'Job {key}', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'draft', 0, cpd_txn(), 't');"
        ));
    }

    /// Reviews and approves the Plan as it is.
    fn approve(&self) {
        self.env.outline_for_review("pay");
        let review = self.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"]).ok();
        let id = review["review"]["id"].as_str().unwrap().to_owned();
        self.cpd(&["plan", "decide", &id, "--approve", "--plan", "pay"]).ok();
    }

    fn lock(&self) -> Run {
        // `plan lock` starts the engine: `cpd watch --ensure` answers it has.
        self.fake("cpd", Reply::ok(WATCH_OK));
        self.cpd(&["plan", "lock", "--plan", "pay"])
    }

    /// The Plan locked, as `plan lock` leaves it, without starting any PR.
    fn dispatch(&self) {
        self.env.sql("UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';");
    }

    /// PR `key` started (`dispatch`), as `fx.start` leaves it without a worktree.
    fn dispatch_sql(key: &str) -> String {
        format!("UPDATE prs SET status = 'dispatch', lead_worktree = '/x/{key}', started_at = 't' WHERE plan = 'pay' AND key = '{key}';")
    }

    /// A `dispatch` PR `key` `open`, with its pull request.
    fn opened_sql(key: &str) -> String {
        format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
               VALUES ('acme/{key}', 1, 'https://github.com/acme/{key}/pull/1', 'T', 0, 'open', 'none', 'none', 't', 't');
             UPDATE prs SET status = 'open', pr_number = 1, pr_url = 'https://github.com/acme/{key}/pull/1', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/{key}') WHERE plan = 'pay' AND key = '{key}';"
        )
    }

    /// An `open` PR `key` merged.
    fn merge_sql(key: &str) -> String {
        format!(
            "UPDATE prs SET status = 'merged', merged_at = 't' WHERE plan = 'pay' AND key = '{key}';
             UPDATE pull_requests SET state = 'merged', merged_at = 't' WHERE repo = 'acme/{key}';"
        )
    }

    /// PR `key` `open`, one allowed step at a time.
    fn open(&self, key: &str) {
        self.env.sql(&(Self::dispatch_sql(key) + &Self::opened_sql(key)));
    }

    /// PR `key` `merged`, in one write: the engine never sees it `open`.
    fn merged(&self, key: &str) {
        self.env.sql(&(Self::dispatch_sql(key) + &Self::opened_sql(key) + &Self::merge_sql(key)));
    }

    /// Plan `pay` in `dispatch` with `a1` merged and `a2` (needing it) waiting, both in `repo`.
    fn a2_ready(&self, repo: &Path) {
        self.add("a1", repo, &[]);
        self.add("a2", repo, &["a1"]);
        self.dispatch();
        self.merged("a1");
    }

    fn start(&self, key: &str) -> Run {
        self.cpd(&["pr", "start", key, "--plan", "pay"])
    }

    /// `pr add <key> --repo <repo> <extra…> --plan pay`, its stub Job cancelled.
    fn add_with(&self, key: &str, repo: &Path, extra: &[&str]) {
        let mut args = vec!["pr", "add", key, "--repo", repo.to_str().unwrap(), "--title", "Change the PR", "--shape", "config"];
        args.extend(extra);
        args.extend(["--plan", "pay"]);
        self.cpd(&args).ok();
        self.cancel_stub(key, "pay");
    }

    fn pr_status(&self, key: &str) -> String {
        self.env.query(&format!("SELECT status FROM prs WHERE plan = 'pay' AND key = '{key}'"))
    }

    /// Where PR `key`'s Lead worktree goes in `repo`.
    fn lead_path(&self, repo: &Path, key: &str) -> PathBuf {
        let name = repo.file_name().unwrap().to_str().unwrap();
        repo.parent().unwrap().join(format!("{name}.pay.{key}"))
    }

    fn log(&self, key: &str) -> String {
        self.env.cpd_home().join(format!("run/pay/logs/lead-{key}.pre-start.log")).display().to_string()
    }

    /// Sets `pre-start = <command>` in the `[hooks]` of `repo`'s policy file.
    fn pre_start(&self, repo: &Repo, command: &str) {
        let name = repo.path.file_name().unwrap().to_str().unwrap();
        self.env.repo_policy("acme", name, &format!("[hooks]\npre-start = \"{command}\"\n"));
    }

    /// Lands a commit on the remote `main` through a second clone, so `repo` has not fetched it.
    fn land_on_origin(&self, repo: &Repo, message: &str) -> String {
        let other = self.other_clone();
        self.git(self.env.root(), &["clone", "-q", repo.origin.to_str().unwrap(), other.to_str().unwrap()]);
        self.git(&other, &["commit", "-q", "--allow-empty", "-m", message]);
        self.git(&other, &["push", "-q", "origin", "main"]);
        self.git(&other, &["rev-parse", "HEAD"])
    }

    /// Lands a commit writing `path` on the remote `main` through a second clone, so `repo` has not fetched it.
    fn land_file_on_origin(&self, repo: &Repo, path: &str, content: &str) -> String {
        let other = self.other_clone();
        self.git(self.env.root(), &["clone", "-q", repo.origin.to_str().unwrap(), other.to_str().unwrap()]);
        fs::write(other.join(path), content).unwrap();
        self.git(&other, &["add", "-A"]);
        self.git(&other, &["commit", "-q", "-m", &format!("main writes {path}")]);
        self.git(&other, &["push", "-q", "origin", "main"]);
        self.git(&other, &["rev-parse", "HEAD"])
    }

    /// `branch` is on the remote at one commit past the old `main` that writes `path`; then `main` moves on, writing
    /// `path` differently (so the branch conflicts with it) or another file (so it only lacks it). Returns the branch's tip.
    fn remote_branch_then_main_moves(&self, repo: &Repo, branch: &str, conflicting: bool) -> String {
        fs::write(repo.path.join("clash.txt"), "the branch's\n").unwrap();
        self.git(&repo.path, &["add", "-A"]);
        self.git(&repo.path, &["commit", "-q", "-m", "the branch"]);
        let tip = head(self, &repo.path, "HEAD");
        self.git(&repo.path, &["push", "-q", "origin", &format!("HEAD:refs/heads/{branch}")]);
        self.git(&repo.path, &["reset", "-q", "--hard", "HEAD~1"]);
        let (path, content) = if conflicting { ("clash.txt", "main's\n") } else { ("moved.txt", "main moved\n") };
        self.land_file_on_origin(repo, path, content);
        tip
    }

    /// A second clone's directory, a new one at each call.
    fn other_clone(&self) -> PathBuf {
        self.clones.set(self.clones.get() + 1);
        self.env.root().join(format!("other-clone-{}", self.clones.get()))
    }

    /// The engine with E5 polling every second of virtual time and `cpd pr poll` run for real, in this process, over a `gh`
    /// that finds no pull request by search and knows `acme/a1#1`, merged on GitHub once `merged` is set.
    fn engine_polling(&self, merged: &Arc<AtomicBool>) {
        let text = fs::read_to_string(self.env.cpd_home().join("config.toml")).unwrap();
        fs::write(self.env.cpd_home().join("config.toml"), text.replace("pr_poll_seconds = 3600", "pr_poll_seconds = 1")).unwrap();
        self.engine_routes_polling();
        let merged = merged.clone();
        self.route_fn("gh", move |call| {
            if call.args.iter().any(|arg| arg.contains("search(type: ISSUE")) {
                return Reply::ok(EMPTY_SEARCH);
            }
            if let Some(query) = call.args.iter().find(|arg| arg.contains("reviewThreads")) {
                return Reply::ok(&empty_comments(query));
            }
            // E5 polls whatever the phase (§3.7): its search lists nothing, so it looks up a1's pull request.
            let (state, at) = if merged.load(Ordering::SeqCst) { ("MERGED", r#""2026-10-14T10:00:00Z""#) } else { ("OPEN", "null") };
            let node = format!(
                r#"{{"repository":{{"nameWithOwner":"acme/a1"}},"number":1,"url":"https://github.com/acme/a1/pull/1","title":"T","isDraft":false,"state":"{state}","createdAt":"t","updatedAt":"U1","mergedAt":{at},"reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","viewerDidAuthor":true,"viewerLatestReviewRequest":null,"commits":{{"nodes":[]}}}}"#
            );
            Reply::ok(&format!(r#"{{"data":{{"p0":{{"pullRequest":{node}}}}}}}"#))
        });
    }

    /// Runs the engine, on virtual time, until `done` says so, asked between two naps of the state in the database.
    fn engine_until(&self, done: impl Fn(&Look) -> bool + Send + 'static) {
        let db = self.env.cpd_home().join("cpd.db");
        // E7 starts a PR through the ambient ports of the thread: the fake ones, scoped.
        self.scoped(|| self.watch_steps(move |_| {
            let conn = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("open the store");
            conn.busy_timeout(std::time::Duration::from_secs(5)).expect("set the busy timeout");
            done(&Look(conn))
        }))
        .expect("the engine ends cleanly");
    }

    /// Runs the engine until `n` more polls have searched.
    fn engine_polls(&self, n: usize) {
        let runner = self.fakes.runner.clone();
        let searches = move || runner.calls().iter().filter(|call| call.program.ends_with("gh") && call.args.iter().any(|a| a.contains("search(type"))).count();
        let to = searches() + n;
        self.scoped(|| self.watch_steps(move |_| searches() >= to)).expect("the engine ends cleanly");
    }

    /// Runs the engine for `n` ticks.
    fn engine_ticks(&self, n: usize) {
        self.scoped(|| self.watch_ticks(n)).expect("the engine ends cleanly");
    }
}

/// What a step of [`Fx::engine_until`] reads of the store.
struct Look(Connection);

impl Look {
    fn status(&self, key: &str) -> String {
        self.0.query_row("SELECT status FROM prs WHERE plan = 'pay' AND key = ?1", [key], |r| r.get(0)).expect("the PR's status")
    }

    /// Whether the start of PR `key` failed (E7's `pr_start_failed` marks it).
    fn start_failed(&self, key: &str) -> bool {
        self.0.query_row("SELECT start_failed_at IS NOT NULL FROM prs WHERE plan = 'pay' AND key = ?1", [key], |r| r.get(0)).expect("the PR's mark")
    }
}

fn head(fx: &Fx, repo: &Path, rev: &str) -> String {
    fx.git(repo, &["rev-parse", rev])
}

fn remote_branch(fx: &Fx, repo: &Path, branch: &str) -> String {
    fx.git(repo, &["ls-remote", "--heads", "origin", branch])
}

/// The `branch.*` entries of `repo`'s `.git/config` (upstreams), one per line.
fn branch_config(fx: &Fx, repo: &Path) -> String {
    // Exit 1 and no output when nothing matches: no `Fx::git`, which panics on it.
    String::from_utf8(fx.git_output(repo, &["config", "--get-regexp", r"^branch\."]).stdout).unwrap()
}

// ---------- plan lock ----------

#[test]
fn lock_starts_only_the_prs_without_needs() {
    let fx = fx();
    let (api, web) = (fx.repo("api"), fx.repo("web"));
    fx.add("api", &api.path, &[]);
    fx.add("web", &web.path, &["api"]);
    fx.job("schema", "api");
    fx.job("ui", "web");
    fx.approve();
    let base = head(&fx, &api.path, "HEAD");

    let out = fx.lock().ok();

    let worktree = fx.lead_path(&api.path, "api");
    assert_eq!(worktree.file_name().unwrap().to_str().unwrap(), "api.pay.api");
    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "phase": "dispatch", "jobs": ["open-api", "open-web", "schema", "ui"],
               "prs": [{"key": "api", "status": "dispatch", "needs": [], "base_pr": null, "lead_branch": "pay/api",
                        "lead_worktree": worktree.display().to_string(), "base_sha": base, "reused": false, "pushed": true},
                       {"key": "web", "status": "waiting", "needs": ["api"], "base_pr": null, "lead_branch": "pay/web",
                        "lead_worktree": null, "base_sha": null, "reused": null, "pushed": null}],
               "watch": {"started": true, "pane": "%9"}})
    );
    // The started PR: branch, worktree, remote and row.
    assert_eq!(fx.git(&worktree, &["symbolic-ref", "--short", "HEAD"]), "pay/api");
    assert_eq!(remote_branch(&fx, &api.path, "pay/api"), format!("{base}\trefs/heads/pay/api"));
    assert_eq!(branch_config(&fx, &api.path), "", "no upstream (hs-d243 G1)");
    assert_eq!(fx.pr_status("api"), "dispatch");
    assert_eq!(fx.env.query::<String>("SELECT lead_worktree FROM prs WHERE key = 'api'"), worktree.display().to_string());
    // The waiting one: nothing created anywhere.
    assert_eq!(fx.pr_status("web"), "waiting");
    assert!(!fx.lead_path(&web.path, "web").exists());
    assert_eq!(fx.git(&web.path, &["branch", "--list", "pay/*"]), "");
    assert_eq!(remote_branch(&fx, &web.path, "pay/web"), "");
    // Every Job is queued at once.
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM jobs WHERE status = 'queued' AND fingerprint IS NOT NULL"), 4);
    assert_eq!(fx.env.query::<String>("SELECT phase FROM plans WHERE id = 'pay'"), "dispatch");

    let phase = fx.events("plan-phase");
    assert_eq!(phase.last().unwrap()["data"], json!({"from": "plan", "to": "dispatch", "jobs": ["open-api", "open-web", "schema", "ui"], "prs": ["api"]}));
    let started = fx.events("pr-started");
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!(
        started[0]["data"],
        json!({"pr": "api", "repo": api.path.display().to_string(), "base": "main", "lead_branch": "pay/api",
               "lead_worktree": worktree.display().to_string(), "base_sha": base, "reused": false, "pushed": true, "by": "lock"})
    );
    assert_eq!(started[0]["plan"], "pay");
    assert!(fx.events("error").is_empty());

    // Again: `already`, the PRs as they are, nothing started.
    let again = fx.lock().ok();
    assert_eq!(again["already"], true);
    assert_eq!(again["prs"][0]["status"], "dispatch");
    assert_eq!(again["prs"][1]["status"], "waiting");
    assert_eq!(again["prs"][0]["base_pr"], Value::Null);
    assert_eq!(again["prs"][1]["base_pr"], Value::Null);
    assert_eq!(fx.events("pr-started").len(), 1);
}

#[test]
fn lock_leaves_a_stacked_pr_waiting_and_starts_only_its_base_pr() {
    let fx = fx();
    let repo = fx.repo("pay");
    fx.add("a1", &repo.path, &[]);
    fx.add_with("b1", &repo.path, &["--base", "a1"]);
    fx.job("schema", "a1");
    fx.job("ui", "b1");
    fx.approve();
    let base = head(&fx, &repo.path, "HEAD");

    let out = fx.lock().ok();

    let worktree = fx.lead_path(&repo.path, "a1");
    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "phase": "dispatch", "jobs": ["open-a1", "open-b1", "schema", "ui"],
               "prs": [{"key": "a1", "status": "dispatch", "needs": [], "base_pr": null, "lead_branch": "pay/a1",
                        "lead_worktree": worktree.display().to_string(), "base_sha": base, "reused": false, "pushed": true},
                       {"key": "b1", "status": "waiting", "needs": [], "base_pr": "pay/a1", "lead_branch": "pay/b1",
                        "lead_worktree": null, "base_sha": null, "reused": null, "pushed": null}],
               "watch": {"started": true, "pane": "%9"}})
    );
    assert_eq!(fx.git(&worktree, &["symbolic-ref", "--short", "HEAD"]), "pay/a1");
    assert_eq!(remote_branch(&fx, &repo.path, "pay/a1"), format!("{base}\trefs/heads/pay/a1"));
    assert_eq!(fx.pr_status("a1"), "dispatch");
    assert_eq!(fx.pr_status("b1"), "waiting");
    assert!(!fx.lead_path(&repo.path, "b1").exists());
    assert_eq!(fx.git(&repo.path, &["for-each-ref", "--format=%(refname:short)", "refs/heads/pay/"]), "pay/a1");
    assert_eq!(remote_branch(&fx, &repo.path, "pay/b1"), "");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM jobs WHERE status = 'queued' AND fingerprint IS NOT NULL"), 4);
    assert_eq!(fx.env.query::<String>("SELECT phase FROM plans WHERE id = 'pay'"), "dispatch");

    let phase = fx.events("plan-phase");
    assert_eq!(phase.last().unwrap()["data"]["prs"], json!(["a1"]));
    let started = fx.events("pr-started");
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!(started[0]["data"]["pr"], "a1");
    assert_eq!(started[0]["data"]["by"], "lock");
    assert!(fx.events("error").is_empty());

    // Again: `a1` is started, not open, so `b1` still waits on it.
    let again = fx.lock().ok();
    assert_eq!(again["already"], true);
    assert_eq!(again["prs"][0]["base_pr"], Value::Null);
    assert_eq!(again["prs"][1]["base_pr"], "pay/a1");
    assert_eq!(fx.events("pr-started").len(), 1);
}

#[test]
fn lock_starts_no_pr_down_a_stack_and_still_starts_an_independent_pr() {
    let fx = fx();
    let (api, web) = (fx.repo("api"), fx.repo("web"));
    fx.add("a1", &api.path, &[]);
    fx.add_with("b1", &api.path, &["--base", "a1"]);
    fx.add_with("c1", &api.path, &["--base", "b1"]);
    fx.add("w1", &web.path, &[]);
    for (job, pr) in [("ja", "a1"), ("jb", "b1"), ("jc", "c1"), ("jw", "w1")] {
        fx.job(job, pr);
    }
    fx.approve();

    let out = fx.lock().ok();

    let rows = out["prs"].as_array().unwrap();
    let row = |key: &str| rows.iter().find(|p| p["key"] == key).unwrap_or_else(|| panic!("no row {key}: {out}"));
    assert_eq!(row("a1")["status"], "dispatch");
    assert_eq!(row("w1")["status"], "dispatch");
    assert_eq!(row("b1")["status"], "waiting");
    assert_eq!(row("c1")["status"], "waiting");
    assert_eq!(row("b1")["base_pr"], "pay/a1");
    assert_eq!(row("c1")["base_pr"], "pay/b1");
    for key in ["b1", "c1"] {
        assert_eq!(fx.pr_status(key), "waiting");
        assert!(!fx.lead_path(&api.path, key).exists(), "{key}'s worktree");
        assert_eq!(remote_branch(&fx, &api.path, &format!("pay/{key}")), "", "{key} on origin");
    }
    assert_eq!(fx.git(&api.path, &["for-each-ref", "--format=%(refname:short)", "refs/heads/pay/"]), "pay/a1");
    assert_eq!(fx.git(&web.path, &["for-each-ref", "--format=%(refname:short)", "refs/heads/pay/"]), "pay/w1");
    let mut started: Vec<String> = fx.events("pr-started").iter().map(|e| e["data"]["pr"].as_str().unwrap().to_owned()).collect();
    started.sort();
    assert_eq!(started, ["a1", "w1"]);
}

#[test]
fn lock_is_all_or_nothing_when_a_pr_fails_to_start() {
    let fx = fx();
    let (api, web) = (fx.repo("api"), fx.repo("web"));
    fx.pre_start(&web, "echo setting up; exit 3");
    // What `sh` does with that is the shell's: it prints the line and exits 3.
    fx.fake("sh", Reply::exit(3, "setting up\n", ""));
    fx.add("api", &api.path, &[]);
    fx.add("web", &web.path, &[]);
    fx.job("schema", "api");
    fx.job("ui", "web");
    fx.approve();

    let error = refused(&fx.lock(), "setup_failed", 1);

    assert_eq!((&error["details"]["pr"], &error["details"]["step"]), (&json!("web"), &json!("pre-start")));
    assert_eq!(error["details"]["log"], fx.log("web"));
    assert!(error["details"]["tail"].as_str().unwrap().contains("setting up"), "{error}");
    // The PR that passed is gone again, and nothing was pushed.
    for (repo, key) in [(&api, "api"), (&web, "web")] {
        assert!(!fx.lead_path(&repo.path, key).exists(), "{key}'s worktree");
        assert_eq!(fx.git(&repo.path, &["branch", "--list", "pay/*"]), "", "{key}'s branch");
        assert_eq!(remote_branch(&fx, &repo.path, &format!("pay/{key}")), "", "{key} on origin");
    }
    assert_eq!(fx.env.query::<String>("SELECT phase FROM plans WHERE id = 'pay'"), "plan");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM prs WHERE status <> 'waiting'"), 0);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM jobs WHERE status = 'draft'"), 4);
    assert!(fx.events("pr-started").is_empty());
    assert!(fx.calls("cpd").iter().all(|c| c.args[0] != "watch"));
}

#[test]
fn lock_refuses_a_pr_without_a_draft_job() {
    let fx = fx();
    let (api, web) = (fx.repo("api"), fx.repo("web"));
    fx.add("api", &api.path, &[]);
    fx.add("web", &web.path, &[]);
    // As a Lead cancels a stub it does not need: the opener stub too, so `api` has no draft Job left.
    fx.cpd(&["job", "cancel", "open-api", "--reason", "a stub of the template", "--plan", "pay"]).ok();
    fx.job("ui", "web");
    fx.approve();

    let error = refused(&fx.lock(), "pr_empty", 3);

    assert_eq!(error["details"]["prs"], json!(["api"]));
    assert_eq!(fx.env.query::<String>("SELECT phase FROM plans WHERE id = 'pay'"), "plan");
    assert!(!fx.lead_path(&web.path, "web").exists());
}

/// Plans `api` and `web` locked: `web`'s Lead Branch is on the remote already.
fn lock_fx_with_remote_web_branch(conflicting: bool) -> (Fx, Repo, Repo, String) {
    let fx = fx();
    let (api, web) = (fx.repo("api"), fx.repo("web"));
    fx.add("api", &api.path, &[]);
    fx.add("web", &web.path, &[]);
    fx.job("schema", "api");
    fx.job("ui", "web");
    fx.approve();
    let tip = fx.remote_branch_then_main_moves(&web, "pay/web", conflicting);
    (fx, api, web, tip)
}

#[test]
fn lock_reuses_a_remote_lead_branch_that_lacks_a_base_that_merely_moved_ahead() {
    let (fx, _api, web, tip) = lock_fx_with_remote_web_branch(false);

    let out = fx.lock().ok();

    let started = out["prs"].as_array().unwrap().iter().find(|p| p["key"] == "web").unwrap();
    assert_eq!(started["reused"], true, "{out}");
    assert_eq!(fx.git(&fx.lead_path(&web.path, "web"), &["rev-parse", "HEAD"]), tip);
    assert_eq!(remote_branch(&fx, &web.path, "pay/web"), format!("{tip}\trefs/heads/pay/web"), "not reset, not pushed over");
    assert_eq!(fx.env.query::<String>("SELECT phase FROM plans WHERE id = 'pay'"), "dispatch");
}

#[test]
fn lock_refuses_a_remote_lead_branch_of_a_pr_that_conflicts_with_its_base() {
    let (fx, api, web, tip) = lock_fx_with_remote_web_branch(true);

    let error = refused(&fx.lock(), "branch_exists", 3);

    assert_eq!(error["details"]["pr"], "web");
    assert_eq!(error["details"]["branch"], "pay/web");
    assert_eq!(error["details"]["files"], json!(["clash.txt"]));
    assert_eq!(remote_branch(&fx, &web.path, "pay/web"), format!("{tip}\trefs/heads/pay/web"));
    // All or nothing: api, started first, is not left behind or pushed.
    assert!(!fx.lead_path(&api.path, "api").exists());
    assert_eq!(remote_branch(&fx, &api.path, "pay/api"), "");
    assert_eq!(fx.env.query::<String>("SELECT phase FROM plans WHERE id = 'pay'"), "plan");
}

// ---------- pr start ----------

#[test]
fn pr_start_refusals_change_nothing() {
    let fx = fx();
    let repo = fx.repo("pay");
    fx.add("a1", &repo.path, &[]);
    fx.add("a2", &repo.path, &["a1"]);
    fx.add("c", &repo.path, &[]);
    fx.cpd(&["pr", "cancel", "c", "--plan", "pay"]).ok();

    let error = refused(&fx.start("a1"), "illegal_phase", 3);
    assert_eq!((&error["details"]["phase"], &error["details"]["needs"]), (&json!("plan"), &json!("dispatch")));
    fx.dispatch();
    refused(&fx.start("zz"), "pr_not_found", 4);
    refused(&fx.start("c"), "illegal_transition", 3);
    let error = refused(&fx.start("a2"), "pr_not_ready", 3);
    assert_eq!(error["details"]["needs"], json!(["a1"]));
    assert_eq!(fx.cpd(&["pr", "start", "a2"]).error()["code"], "plan_required");

    assert_eq!(fx.pr_status("a2"), "waiting");
    assert_eq!(fx.git(&repo.path, &["branch", "--list", "pay/*"]), "");
    assert!(!fx.lead_path(&repo.path, "a2").exists());
    assert!(fx.events("pr-started").is_empty());

    // A PR that is already started or merged: `already`, nothing written.
    fx.merged("a1");
    let out = fx.start("a1").ok();
    assert_eq!((&out["already"], &out["pr"]["key"], &out["pr"]["status"]), (&json!(true), &json!("a1"), &json!("merged")));
    assert!(fx.events("pr-started").is_empty());
}

#[test]
fn pr_start_creates_pushes_and_records_the_start() {
    let fx = fx();
    let repo = fx.repo("pay");
    fx.a2_ready(&repo.path);
    let tip = fx.land_on_origin(&repo, "what a1 merged");

    let out = fx.start("a2").ok();

    let worktree = fx.lead_path(&repo.path, "a2");
    assert_eq!(
        out,
        json!({"ok": true, "pr": {"key": "a2", "status": "dispatch", "lead_branch": "pay/a2",
                                  "lead_worktree": worktree.display().to_string(), "base_sha": tip, "reused": false, "pushed": true}})
    );
    assert_eq!(fx.git(&worktree, &["rev-parse", "HEAD"]), tip, "cut from a fresh fetch of origin/main");
    assert_eq!(remote_branch(&fx, &repo.path, "pay/a2"), format!("{tip}\trefs/heads/pay/a2"));
    assert_eq!(branch_config(&fx, &repo.path), "", "no upstream (hs-d243 G1)");
    assert_eq!(fx.pr_status("a2"), "dispatch");
    assert!(fx.env.query::<Option<String>>("SELECT start_failed_at FROM prs WHERE key = 'a2'").is_none());
    let started = fx.events("pr-started");
    assert_eq!((started.len(), &started[0]["data"]["by"], &started[0]["data"]["pr"]), (1, &json!("pr start"), &json!("a2")));
    assert!(fx.events("error").is_empty());

    // Idempotent: the PR is started, so it is `already`.
    let again = fx.start("a2").ok();
    assert_eq!((&again["already"], &again["pr"]["status"]), (&json!(true), &json!("dispatch")));
    assert_eq!(fx.events("pr-started").len(), 1);
}

#[test]
fn eight_pr_starts_in_one_repo_at_once_all_succeed_and_write_no_upstream() {
    let fx = fx();
    let repo = fx.repo("pay");
    let keys: Vec<String> = (1..=8).map(|i| format!("p{i}")).collect();
    for key in &keys {
        fx.add(key, &repo.path, &[]);
    }
    fx.dispatch();

    // Eight `cpd pr start` at once on the same ports: what they share is the git route's own lock, not a process.
    let runs: Vec<Run> = std::thread::scope(|s| {
        let started: Vec<_> = keys
            .iter()
            .map(|key| {
                let ports = Ports { clock: Arc::new(Yielding(fx.fakes.ports.clock.clone())), runner: fx.fakes.ports.runner.clone(), spawn: fx.fakes.ports.spawn.clone() };
                s.spawn(move || {
                    let argv = ["cpd", "pr", "start", key, "--plan", "pay"].map(std::ffi::OsString::from);
                    let mut stdout = Vec::new();
                    let code = cpd::run_with(&ports, argv, &mut stdout);
                    Run { exit: exit_code(code), stdout: String::from_utf8(stdout).unwrap(), stderr: String::new() }
                })
            })
            .collect();
        started.into_iter().map(|t| t.join().unwrap()).collect()
    });

    for (key, run) in keys.iter().zip(&runs) {
        let out = run.ok();
        assert_eq!((&out["pr"]["status"], &out["pr"]["pushed"]), (&json!("dispatch"), &json!(true)), "{key}");
        let branch = format!("pay/{key}");
        assert_eq!(fx.git(&fx.lead_path(&repo.path, key), &["symbolic-ref", "--short", "HEAD"]), branch);
        assert_ne!(remote_branch(&fx, &repo.path, &branch), "", "{key} pushed");
    }
    assert_eq!(branch_config(&fx, &repo.path), "", "no branch.* entry in .git/config");
    assert_eq!(fx.events("pr-started").len(), 8);
    assert!(fx.events("error").is_empty());
}

#[test]
fn pr_start_reuses_a_remote_lead_branch_that_contains_the_base() {
    let fx = fx();
    let repo = fx.repo("pay");
    fx.a2_ready(&repo.path);
    // A commit only the remote has, on top of the base, on the PR's branch.
    fx.git(&repo.path, &["commit", "-q", "--allow-empty", "-m", "only on the remote"]);
    let tip = head(&fx, &repo.path, "HEAD");
    fx.git(&repo.path, &["push", "-q", "origin", "HEAD:refs/heads/pay/a2"]);
    fx.git(&repo.path, &["reset", "-q", "--hard", "HEAD~1"]);

    let out = fx.start("a2").ok();

    let worktree = fx.lead_path(&repo.path, "a2");
    assert_eq!((&out["pr"]["base_sha"], &out["pr"]["reused"], &out["pr"]["pushed"]), (&json!(tip), &json!(true), &json!(false)));
    assert_eq!(fx.git(&worktree, &["rev-parse", "HEAD"]), tip);
    assert_eq!(remote_branch(&fx, &repo.path, "pay/a2"), format!("{tip}\trefs/heads/pay/a2"));
    assert_eq!(fx.events("pr-started")[0]["data"]["reused"], true);
}

#[test]
fn pr_start_reuses_a_remote_lead_branch_that_lacks_a_base_that_merely_moved_ahead() {
    let fx = fx();
    let repo = fx.repo("pay");
    fx.a2_ready(&repo.path);
    let tip = fx.remote_branch_then_main_moves(&repo, "pay/a2", false);

    let out = fx.start("a2").ok();

    assert_eq!((&out["pr"]["base_sha"], &out["pr"]["reused"], &out["pr"]["pushed"]), (&json!(tip), &json!(true), &json!(false)));
    assert_eq!(fx.git(&fx.lead_path(&repo.path, "a2"), &["rev-parse", "HEAD"]), tip);
    assert_eq!(remote_branch(&fx, &repo.path, "pay/a2"), format!("{tip}\trefs/heads/pay/a2"));
    assert_eq!(fx.pr_status("a2"), "dispatch");
}

#[test]
fn pr_start_refuses_a_remote_lead_branch_that_conflicts_with_its_base() {
    let fx = fx();
    let repo = fx.repo("pay");
    fx.a2_ready(&repo.path);
    let old = fx.remote_branch_then_main_moves(&repo, "pay/a2", true);

    let error = refused(&fx.start("a2"), "branch_exists", 3);

    assert_eq!((&error["details"]["pr"], &error["details"]["branch"]), (&json!("a2"), &json!("pay/a2")));
    assert_eq!(error["details"]["files"], json!(["clash.txt"]));
    assert_eq!(fx.git(&repo.path, &["branch", "--list", "pay/*"]), "");
    assert!(!fx.lead_path(&repo.path, "a2").exists());
    assert_eq!(remote_branch(&fx, &repo.path, "pay/a2"), format!("{old}\trefs/heads/pay/a2"));
    assert_eq!(fx.pr_status("a2"), "waiting");
    assert!(fx.events("pr-started").is_empty());
}

#[test]
fn a_push_failure_at_pr_start_still_starts_it_and_appends_one_push_failed_error() {
    let fx = fx();
    let repo = fx.repo("pay");
    fx.a2_ready(&repo.path);
    // What the remote's pre-receive hook does is git's: the push is answered as git answers a refusal.
    let _ = (*fx.fakes.runner).clone().on("git", |call| call.args.iter().any(|a| a == "push"), Reply::exit(1, "", "remote: refused by hook\n"));

    let out = fx.start("a2").ok();

    assert_eq!((&out["pr"]["status"], &out["pr"]["pushed"]), (&json!("dispatch"), &json!(false)));
    assert_eq!(fx.pr_status("a2"), "dispatch");
    assert_eq!(remote_branch(&fx, &repo.path, "pay/a2"), "");
    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let data = &errors[0]["data"];
    assert_eq!((&data["code"], &data["source"], &data["details"]["pr"]), (&json!("push_failed"), &json!("pr start"), &json!("a2")));
    assert!(data["details"]["stderr"].as_str().unwrap().contains("refused by hook"));
}

#[test]
fn a_repo_without_origin_starts_a_pr_as_before() {
    let fx = fx();
    let repo = fx.keyed_repo("pay").canonicalize().unwrap();
    fx.a2_ready(&repo);
    fx.drop_unreachable_origin(&repo);
    let base = head(&fx, &repo, "HEAD");

    let out = fx.start("a2").ok();

    assert_eq!((&out["pr"]["base_sha"], &out["pr"]["reused"], &out["pr"]["pushed"]), (&json!(base), &json!(false), &json!(false)));
    assert_eq!(fx.git(&repo, &["remote"]), "");
    assert_eq!(fx.git(&fx.lead_path(&repo, "a2"), &["symbolic-ref", "--short", "HEAD"]), "pay/a2");
    assert!(fx.events("error").is_empty());
}

#[test]
fn a_pr_made_before_r34_keeps_its_branch_and_worktree_name() {
    let fx = fx();
    let repo = fx.repo("pay");
    // As migration 4 leaves it: the Lead Branch is `cpd/<plan>`.
    fx.env.sql(&format!(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
         VALUES ('pay', 'pay', '{}', 'main', 'waiting', 'cpd/pay', cpd_txn(), 't');",
        repo.path.display()
    ));
    fx.dispatch();

    let out = fx.start("pay").ok();

    let worktree = repo.path.parent().unwrap().join("pay.pay");
    assert_eq!((&out["pr"]["lead_branch"], &out["pr"]["lead_worktree"]), (&json!("cpd/pay"), &json!(worktree.display().to_string())));
    assert_eq!(fx.git(&worktree, &["symbolic-ref", "--short", "HEAD"]), "cpd/pay");
}

// ---------- engine E7 ----------

#[test]
fn e7_starts_a_dependent_pr_from_a_fresh_fetch_once_its_prerequisite_is_merged() {
    let fx = fx();
    let repo = fx.repo("pay");
    fx.add("a1", &repo.path, &[]);
    fx.add("a2", &repo.path, &["a1"]);
    fx.add("a3", &repo.path, &["a2"]);
    fx.dispatch();
    fx.open("a1");
    // E5 polls whatever the phase (§3.7): its search lists nothing, so it looks up a1's pull request (acme/a1#1), which GitHub
    // merges once `merged` is set.
    let merged = Arc::new(AtomicBool::new(false));
    fx.engine_polling(&merged);

    // a1 is not merged: nothing starts, however many ticks pass (two polls of the engine).
    fx.engine_polls(2);
    assert_eq!((fx.pr_status("a2"), fx.pr_status("a3")), ("waiting".to_owned(), "waiting".to_owned()));

    // a1 merges on GitHub: what it brought is on the remote, which this repo has not fetched.
    let tip = fx.land_on_origin(&repo, "what a1 merged");
    merged.store(true, Ordering::SeqCst);
    fx.engine_until(|look| look.status("a2") == "dispatch");

    let worktree = fx.lead_path(&repo.path, "a2");
    assert_eq!(fx.git(&worktree, &["rev-parse", "HEAD"]), tip);
    assert_eq!(fx.git(&worktree, &["symbolic-ref", "--short", "HEAD"]), "pay/a2");
    assert_eq!(remote_branch(&fx, &repo.path, "pay/a2"), format!("{tip}\trefs/heads/pay/a2"));
    let started = fx.events("pr-started");
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!(
        started[0]["data"],
        json!({"pr": "a2", "repo": repo.path.display().to_string(), "base": "main", "lead_branch": "pay/a2",
               "lead_worktree": worktree.display().to_string(), "base_sha": tip, "reused": false, "pushed": true, "by": "engine"})
    );
    // E5 saw a1 merged (the R34 merged path) before E7 started a2.
    assert_eq!(fx.events("pr-merged").len(), 1);
    // a3 needs a2, which is started but not merged.
    assert_eq!(fx.pr_status("a3"), "waiting");
    assert!(fx.events("error").is_empty());
}

#[test]
fn a_failed_start_marks_the_pr_and_appends_one_error_until_pr_start_retries() {
    let fx = fx();
    let repo = fx.repo("pay");
    let marker = fx.env.root().join("setup-ok");
    fx.pre_start(&repo, &format!("test -e {}", marker.display()));
    fx.a2_ready(&repo.path);
    fx.engine_routes();
    // The hook is `sh -c test -e <marker>`: it passes once the marker exists.
    let hook = format!("test -e {}", marker.display());
    let seen = marker.clone();
    fx.route_fn("sh", move |call| {
        assert_eq!(call.args, ["-c", hook.as_str()]);
        if seen.exists() { Reply::ok("") } else { Reply::exit(1, "", "") }
    });

    // E7's pr_start_failed: the engine runs until the PR is marked.
    fx.engine_until(|look| look.start_failed("a2"));

    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["plan"], "pay");
    let data = &errors[0]["data"];
    assert_eq!((&data["code"], &data["source"]), (&json!("pr_start_failed"), &json!("watch")));
    assert_eq!((&data["details"]["pr"], &data["details"]["step"], &data["details"]["log"]), (&json!("a2"), &json!("pre-start"), &json!(fx.log("a2"))));
    assert_eq!(fx.pr_status("a2"), "waiting");
    let failed_at: String = fx.env.query("SELECT start_failed_at FROM prs WHERE key = 'a2'");
    assert!(!failed_at.is_empty());
    assert_eq!(fx.git(&repo.path, &["branch", "--list", "pay/*"]), "", "the branch is removed");
    assert!(!fx.lead_path(&repo.path, "a2").exists(), "the worktree is removed");

    // The engine does not try again.
    fx.engine_ticks(3);
    assert_eq!(fx.events("error").len(), 1);
    assert!(fx.events("pr-started").is_empty());

    // `pr start` fails the same way and changes nothing.
    let writes = (fx.env.query::<i64>("SELECT count(*) FROM txns"), fx.env.query::<i64>("SELECT count(*) FROM events"));
    let error = refused(&fx.start("a2"), "setup_failed", 1);
    assert_eq!((&error["details"]["pr"], &error["details"]["step"], &error["details"]["log"]), (&json!("a2"), &json!("pre-start"), &json!(fx.log("a2"))));
    assert_eq!(fx.env.query::<String>("SELECT start_failed_at FROM prs WHERE key = 'a2'"), failed_at);
    assert_eq!((fx.env.query::<i64>("SELECT count(*) FROM txns"), fx.env.query::<i64>("SELECT count(*) FROM events")), writes);

    // The cause is fixed: `pr start` starts it and clears the mark.
    fs::write(&marker, "").unwrap();
    let out = fx.start("a2").ok();
    assert_eq!(out["pr"]["status"], "dispatch");
    assert_eq!(fx.pr_status("a2"), "dispatch");
    assert!(fx.env.query::<Option<String>>("SELECT start_failed_at FROM prs WHERE key = 'a2'").is_none());
    let started = fx.events("pr-started");
    assert_eq!((started.len(), &started[0]["data"]["by"]), (1, &json!("pr start")));
    assert_eq!(fx.events("error").len(), 1);
}

// ---------- stacked PRs (contract §3.7 E7, §3.8 "Stacked PRs", §7.2) ----------

/// Plan `pay` in `dispatch` with `a1` started (its Lead worktree, its Lead Branch pushed), then `b1` of the same Plan, stacked on `pay/a1`; the Lead Branch of `a1` then has a commit that is neither on the remote
/// nor on `main`. Returns that commit.
fn stacked_on_a1(fx: &Fx, repo: &Repo) -> String {
    fx.add("a1", &repo.path, &[]);
    fx.dispatch();
    fx.start("a1").ok();
    let a1 = fx.lead_path(&repo.path, "a1");
    fx.git(&a1, &["commit", "-q", "--allow-empty", "-m", "a1 work"]);
    fx.add_with("b1", &repo.path, &["--base", "pay/a1", "--no-needs"]);
    head(fx, &a1, "HEAD")
}

/// `stacked_on_a1`, then `a1` is `open` (its pull request exists): `b1` may start. Returns a1's commit.
fn stacked_on_open_a1(fx: &Fx, repo: &Repo) -> String {
    let tip = stacked_on_a1(fx, repo);
    fx.env.sql(&Fx::opened_sql("a1"));
    tip
}

#[test]
fn pr_start_refuses_a_stacked_pr_whose_base_pr_is_started_but_not_open() {
    let fx = fx();
    let repo = fx.repo("pay");
    let tip = stacked_on_a1(&fx, &repo);
    assert_eq!(fx.pr_status("a1"), "dispatch");

    let error = refused(&fx.start("b1"), "pr_not_ready", 3);

    assert_eq!((&error["details"]["pr"], &error["details"]["base_pr"], &error["details"]["needs"]), (&json!("b1"), &json!("pay/a1"), &json!([])));
    assert_eq!(fx.pr_status("b1"), "waiting");
    assert!(!fx.lead_path(&repo.path, "b1").exists());
    assert_eq!(fx.git(&repo.path, &["branch", "--list", "pay/b1"]), "");
    assert!(fx.events("pr-started").iter().all(|e| e["data"]["pr"] != "b1"));

    fx.env.sql(&Fx::opened_sql("a1"));
    fx.start("b1").ok();
    assert_eq!(fx.git(&fx.lead_path(&repo.path, "b1"), &["rev-parse", "HEAD"]), tip);
    assert_eq!(fx.pr_status("b1"), "dispatch");
}

#[test]
fn e7_holds_a_stacked_pr_until_its_base_pr_is_open_and_then_starts_it_from_the_base_head() {
    let fx = fx();
    let repo = fx.repo("pay");
    let tip = stacked_on_a1(&fx, &repo);
    fx.engine_routes();

    fx.engine_ticks(2);
    assert_eq!(fx.pr_status("b1"), "waiting", "pay/a1 is not open");
    assert!(fx.events("pr-started").iter().all(|e| e["data"]["pr"] != "b1"));

    fx.env.sql(&Fx::opened_sql("a1"));
    fx.engine_until(|look| look.status("b1") == "dispatch");

    assert_eq!(fx.git(&fx.lead_path(&repo.path, "b1"), &["rev-parse", "HEAD"]), tip);
    let started: Vec<Value> = fx.events("pr-started").into_iter().filter(|e| e["data"]["pr"] == "b1").collect();
    assert_eq!((started.len(), &started[0]["data"]["by"]), (1, &json!("engine")));
}

#[test]
fn pr_start_cuts_a_stacked_pr_from_the_base_prs_lead_branch_head() {
    let fx = fx();
    let repo = fx.repo("pay");
    let tip = stacked_on_open_a1(&fx, &repo);
    // `main` moves on, so cutting from `origin/main` would show.
    fx.land_on_origin(&repo, "main moves on");

    let out = fx.start("b1").ok();

    let worktree = fx.lead_path(&repo.path, "b1");
    assert_eq!(
        out,
        json!({"ok": true, "pr": {"key": "b1", "status": "dispatch", "lead_branch": "pay/b1",
                                  "lead_worktree": worktree.display().to_string(), "base_sha": tip, "reused": false, "pushed": true}})
    );
    // The head of a1's local Lead Branch, which origin does not have yet: not a fresh origin/main.
    assert_eq!(fx.git(&worktree, &["rev-parse", "HEAD"]), tip);
    assert_ne!(remote_branch(&fx, &repo.path, "pay/a1"), format!("{tip}\trefs/heads/pay/a1"), "a1's commit is not pushed");
    assert_eq!(remote_branch(&fx, &repo.path, "pay/b1"), format!("{tip}\trefs/heads/pay/b1"));
    assert_eq!(branch_config(&fx, &repo.path), "", "no upstream (hs-d243 G1)");
    assert_eq!(fx.pr_status("b1"), "dispatch");
    let started: Vec<Value> = fx.events("pr-started").into_iter().filter(|e| e["data"]["pr"] == "b1").collect();
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!(
        started[0]["data"],
        json!({"pr": "b1", "repo": repo.path.display().to_string(), "base": "pay/a1", "lead_branch": "pay/b1",
               "lead_worktree": worktree.display().to_string(), "base_sha": tip, "reused": false, "pushed": true, "by": "pr start"})
    );
}

#[test]
fn a_stacked_pr_starts_from_the_local_base_branch_when_the_base_prs_branch_is_not_on_origin() {
    let fx = fx();
    let repo = fx.repo("pay");
    let tip = stacked_on_open_a1(&fx, &repo);
    // a1's push failed at its start, say: origin has no such branch.
    fx.git(&repo.path, &["push", "-q", "origin", "--delete", "pay/a1"]);

    let out = fx.start("b1").ok();

    assert_eq!((&out["pr"]["base_sha"], &out["pr"]["reused"], &out["pr"]["pushed"]), (&json!(tip), &json!(false), &json!(true)));
    assert_eq!(fx.git(&fx.lead_path(&repo.path, "b1"), &["rev-parse", "HEAD"]), tip);
    assert_eq!(remote_branch(&fx, &repo.path, "pay/a1"), "", "nothing pushes the base PR's branch but its own Lead");
}

#[test]
fn a_stacked_pr_starts_from_origin_when_the_base_prs_local_branch_is_gone_and_fails_when_both_are() {
    let fx = fx();
    let repo = fx.repo("pay");
    stacked_on_open_a1(&fx, &repo);
    let a1 = fx.lead_path(&repo.path, "a1");
    let on_origin = remote_branch(&fx, &repo.path, "pay/a1").split('\t').next().unwrap().to_owned();
    fx.git(&repo.path, &["worktree", "remove", "--force", a1.to_str().unwrap()]);
    fx.git(&repo.path, &["branch", "-D", "pay/a1"]);
    // Neither the local branch nor origin's: nothing to start from.
    fx.git(&repo.path, &["push", "-q", "origin", "--delete", "pay/a1"]);

    let error = refused(&fx.start("b1"), "setup_failed", 1);

    assert_eq!((&error["details"]["pr"], &error["details"]["step"], &error["details"]["base"]), (&json!("b1"), &json!("base_pr_branch"), &json!("pay/a1")));
    assert_eq!(fx.pr_status("b1"), "waiting");
    assert!(!fx.lead_path(&repo.path, "b1").exists());
    assert_eq!(fx.git(&repo.path, &["branch", "--list", "pay/*"]), "");

    // origin has it again: that is the start point.
    fx.git(&repo.path, &["push", "-q", "origin", &format!("{on_origin}:refs/heads/pay/a1")]);
    let out = fx.start("b1").ok();
    assert_eq!(out["pr"]["base_sha"], on_origin);
    assert_eq!(fx.git(&fx.lead_path(&repo.path, "b1"), &["rev-parse", "HEAD"]), on_origin);
}

#[test]
fn pr_start_refuses_a_stacked_pr_whose_base_pr_has_not_started() {
    let fx = fx();
    let repo = fx.repo("pay");
    fx.add("a0", &repo.path, &[]);
    fx.add("a1", &repo.path, &["a0"]);
    fx.dispatch();
    fx.add_with("b1", &repo.path, &["--base", "pay/a1", "--no-needs"]);

    let error = refused(&fx.start("b1"), "pr_not_ready", 3);

    assert_eq!((&error["details"]["pr"], &error["details"]["base_pr"], &error["details"]["needs"]), (&json!("b1"), &json!("pay/a1"), &json!([])));
    assert_eq!(fx.pr_status("b1"), "waiting");
    assert!(!fx.lead_path(&repo.path, "b1").exists());
    assert_eq!(fx.git(&repo.path, &["branch", "--list", "pay/*"]), "");
    assert!(fx.events("pr-started").is_empty());
    // a1 starts (a0 merged): b1 may.
    fx.merged("a0");
    fx.start("a1").ok();
    // a1 started but is not open: b1 still waits.
    refused(&fx.start("b1"), "pr_not_ready", 3);
    fx.env.sql(&Fx::opened_sql("a1"));
    assert_eq!(fx.start("b1").ok()["pr"]["status"], "dispatch");
}

#[test]
fn pr_start_refuses_a_pr_whose_need_of_another_plan_is_not_merged() {
    let fx = fx();
    let repo = fx.repo("pay");
    fx.add("a1", &repo.path, &[]);
    fx.dispatch();
    fx.env.sql(&Fx::dispatch_sql("a1"));
    fx.add_with("c1", &repo.path, &["--needs", "a1"]);

    let error = refused(&fx.start("c1"), "pr_not_ready", 3);

    assert_eq!((&error["details"]["needs"], &error["details"]["base_pr"]), (&json!(["a1"]), &Value::Null));
    assert!(error["message"].as_str().unwrap().contains("a1"), "{error}");
    assert_eq!(fx.pr_status("c1"), "waiting");
    fx.env.sql(&(Fx::opened_sql("a1") + &Fx::merge_sql("a1")));
    assert_eq!(fx.start("c1").ok()["pr"]["status"], "dispatch");
}

#[test]
fn e7_holds_a_pr_for_a_need_of_another_plan_until_it_is_merged() {
    let fx = fx();
    let repo = fx.repo("pay");
    fx.add("a1", &repo.path, &[]);
    fx.dispatch();
    fx.env.sql(&Fx::dispatch_sql("a1"));
    fx.add_with("c1", &repo.path, &["--needs", "a1"]);
    fx.engine_routes();

    fx.engine_ticks(2);
    assert_eq!(fx.pr_status("c1"), "waiting", "pay/a1 is not merged");
    assert!(fx.events("pr-started").is_empty());

    fx.env.sql(&(Fx::opened_sql("a1") + &Fx::merge_sql("a1")));
    fx.engine_until(|look| look.status("c1") == "dispatch");

    let started = fx.events("pr-started");
    assert_eq!((started.len(), &started[0]["plan"], &started[0]["data"]["pr"], &started[0]["data"]["by"]), (1, &json!("pay"), &json!("c1"), &json!("engine")));
    assert!(fx.events("error").is_empty());
}

#[test]
fn e7_starts_a_stacked_pr_once_its_base_pr_is_open_and_not_before() {
    let fx = fx();
    let repo = fx.repo("pay");
    fx.add("a0", &repo.path, &[]);
    fx.add("a1", &repo.path, &["a0"]);
    fx.dispatch();
    fx.env.sql(&Fx::dispatch_sql("a0"));
    fx.add_with("b1", &repo.path, &["--base", "pay/a1", "--no-needs"]);
    fx.engine_routes();

    // a1 waits for a0, and b1 for a1 to open: nothing starts.
    fx.engine_ticks(2);
    assert_eq!((fx.pr_status("a1"), fx.pr_status("b1")), ("waiting".to_owned(), "waiting".to_owned()));
    assert!(fx.events("pr-started").is_empty());

    // a0 merges: E7 starts a1, but b1 waits: a1 is started, not open.
    let tip = fx.land_on_origin(&repo, "what a0 merged");
    fx.env.sql(&(Fx::opened_sql("a0") + &Fx::merge_sql("a0")));
    fx.engine_until(|look| look.status("a1") == "dispatch");
    fx.engine_ticks(2);
    assert_eq!(fx.pr_status("b1"), "waiting");

    // a1 opens: E7 starts b1 from a1's Lead Branch.
    fx.env.sql(&Fx::opened_sql("a1"));
    fx.engine_until(|look| look.status("b1") == "dispatch");

    let a1 = fx.lead_path(&repo.path, "a1");
    assert_eq!(fx.git(&a1, &["rev-parse", "HEAD"]), tip);
    let b1 = fx.lead_path(&repo.path, "b1");
    assert_eq!(fx.git(&b1, &["rev-parse", "HEAD"]), tip);
    assert_eq!(fx.git(&b1, &["symbolic-ref", "--short", "HEAD"]), "pay/b1");
    let started = fx.events("pr-started");
    let by: Vec<(&Value, &Value)> = started.iter().map(|e| (&e["data"]["pr"], &e["data"]["by"])).collect();
    assert_eq!(by, vec![(&json!("a1"), &json!("engine")), (&json!("b1"), &json!("engine"))], "a1 first");
    assert!(fx.events("error").is_empty());
}

// ---------- a merged base PR, its branch gone (contract §3.8 "Stacked PRs" step 7, hs-d301) ----------

/// `stacked_on_a1`, then `pay/a1` merges and GitHub deletes its branch (no Lead worktree, no local branch, none on origin), and
/// `main` moves on. Returns the new head of `origin/main`: where `b1` now starts from.
fn a1_merged_and_deleted(fx: &Fx, repo: &Repo) -> String {
    stacked_on_a1(fx, repo);
    let a1 = fx.lead_path(&repo.path, "a1");
    fx.git(&repo.path, &["worktree", "remove", "--force", a1.to_str().unwrap()]);
    fx.git(&repo.path, &["branch", "-D", "pay/a1"]);
    fx.git(&repo.path, &["push", "-q", "origin", "--delete", "pay/a1"]);
    assert_eq!((fx.git(&repo.path, &["branch", "--list", "pay/*"]), remote_branch(fx, &repo.path, "pay/a1")), (String::new(), String::new()));
    fx.env.sql(&(Fx::opened_sql("a1") + &Fx::merge_sql("a1")));
    fx.land_on_origin(repo, "what a1 merged")
}

/// `pr-base-changed`'s `text` and `data` for `b1`, unstacked by `by`.
fn b1_unstacked(by: &str) -> (Value, Value) {
    (
        json!("PR b1 of Plan pay now targets main (was pay/a1): its base PR pay/a1 merged before its pull request existed. \
               Its tree is unchanged: no new Gate is needed and it stays ready. Read it with cpd plan show --plan pay."),
        json!({"pr": "b1", "from": "pay/a1", "to": "main", "from_pr": "pay/a1", "to_pr": null, "by": by}),
    )
}

#[test]
fn pr_start_unstacks_a_pr_whose_base_pr_merged_and_starts_it_from_the_new_base() {
    let fx = fx();
    let repo = fx.repo("pay");
    let tip = a1_merged_and_deleted(&fx, &repo);

    let out = fx.start("b1").ok();

    let worktree = fx.lead_path(&repo.path, "b1");
    assert_eq!(
        out,
        json!({"ok": true, "pr": {"key": "b1", "status": "dispatch", "lead_branch": "pay/b1",
                                  "lead_worktree": worktree.display().to_string(), "base_sha": tip, "reused": false, "pushed": true}})
    );
    assert_eq!(fx.git(&worktree, &["rev-parse", "HEAD"]), tip, "cut from origin/main, the merged PR's branch is gone");
    assert_eq!(fx.env.query::<String>("SELECT base FROM prs WHERE plan = 'pay' AND key = 'b1'"), "main");
    let changed: Vec<(Value, Value)> = fx.events("pr-base-changed").iter().map(|e| (e["text"].clone(), e["data"].clone())).collect();
    assert_eq!(changed, vec![b1_unstacked("pr start")]);
    let started: Vec<Value> = fx.events("pr-started").into_iter().filter(|e| e["data"]["pr"] == "b1").collect();
    assert_eq!((started.len(), &started[0]["data"]["base"], &started[0]["data"]["by"]), (1, &json!("main"), &json!("pr start")));
    assert!(fx.events("error").is_empty());
}

#[test]
fn e7_unstacks_a_waiting_pr_whose_base_pr_merged_and_starts_it_from_the_new_base() {
    let fx = fx();
    let repo = fx.repo("pay");
    let tip = a1_merged_and_deleted(&fx, &repo);
    assert_eq!(fx.pr_status("b1"), "waiting");
    fx.engine_routes();

    fx.engine_until(|look| look.status("b1") == "dispatch");

    let worktree = fx.lead_path(&repo.path, "b1");
    assert_eq!(fx.git(&worktree, &["rev-parse", "HEAD"]), tip, "cut from origin/main, the merged PR's branch is gone");
    assert_eq!(fx.env.query::<String>("SELECT base FROM prs WHERE plan = 'pay' AND key = 'b1'"), "main");
    let changed: Vec<(Value, Value)> = fx.events("pr-base-changed").iter().map(|e| (e["text"].clone(), e["data"].clone())).collect();
    assert_eq!(changed, vec![b1_unstacked("engine")], "one Event, whatever the number of ticks");
    let started: Vec<Value> = fx.events("pr-started").into_iter().filter(|e| e["data"]["pr"] == "b1").collect();
    assert_eq!((started.len(), &started[0]["data"]["base"], &started[0]["data"]["by"]), (1, &json!("main"), &json!("engine")));
    assert!(fx.events("error").is_empty(), "{:?}", fx.events("error"));
}
