//! Starting a PR (contract §3.8, §7.2; commands.md §4.8, §9.7): `plan lock`
//! starts the PRs without needs, all or nothing; engine E7 starts a `waiting`
//! PR once its needs are merged; `pr start` retries a failed start; R35's
//! remote rules apply to each PR's Lead Branch. The built binary against a
//! temporary `$CPD_HOME`, repos with bare `origin`s and a Plan written straight
//! into the database.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};

use common::{CPD, Run, TestEnv, WATCH_OK, refused, wait_until, write_executable};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use serde_json::{Value, json};


struct Fx {
    env: TestEnv,
}

/// A repo with its bare `origin`.
struct Repo {
    path: PathBuf,
    origin: PathBuf,
}

/// The poller's search answering no pull requests.
const EMPTY_SEARCH: &str = r#"{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}"#;

/// The env with its config (a one-second tick), a fake `cpd` whose `watch --ensure`
/// succeeds, Context `ctx` and Plan `pay` in phase `plan`.
fn fx() -> Fx {
    let env = TestEnv::new();
    let config = format!(
        "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n[watch]\ntick_seconds = 1\npr_poll_seconds = 1\n[tmux]\nsocket = \"{}\"\n",
        env.tmux_socket()
    );
    fs::write(env.cpd_home().join("config.toml"), config).unwrap();
    env.fake("cpd", &format!("if [ \"$1 $2\" = 'watch --ensure' ]; then {WATCH_OK}; fi\nexec '{CPD}' \"$@\""));
    // E5 polls on every engine: a `gh` that finds no pull requests, so that no test reaches the real one.
    env.fake_gh(&format!("echo '{EMPTY_SEARCH}'"));
    let fx = Fx { env };
    fx.env.sql(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pay', 'ctx', 'Pay the bills', 'plan', 'lead1', 't');",
    );
    fx
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap(), 5000).unwrap()
    }

    fn query<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        self.store().read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
    }

    fn events(&self, kind: &str) -> Vec<Value> {
        self.env.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
    }

    /// A repo `name` on `main` with a bare `origin` holding `main`.
    fn repo(&self, name: &str) -> Repo {
        let path = self.env.git_repo(name).canonicalize().unwrap();
        let origin = self.env.root().join("origins").join(format!("{name}.git"));
        fs::create_dir_all(&origin).unwrap();
        self.env.git(&origin, &["init", "-q", "--bare", "-b", "main"]);
        self.env.git(&path, &["remote", "add", "origin", origin.to_str().unwrap()]);
        self.env.git(&path, &["push", "-q", "origin", "main"]);
        Repo { path, origin }
    }

    /// `pr add <key> --repo <repo> [--needs <needs>]`.
    fn add(&self, key: &str, repo: &Path, needs: &[&str]) {
        let needs = needs.join(",");
        let mut args = vec!["pr", "add", key, "--repo", repo.to_str().unwrap(), "--plan", "pay"];
        if !needs.is_empty() {
            args.extend(["--needs", &needs]);
        }
        self.env.cpd(&args).ok();
    }

    /// A draft Job of PR `pr`.
    fn job(&self, key: &str, pr: &str) {
        self.env.sql(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('pay', '{pr}', '{key}', 'Job {key}', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, cpd_txn(), 't');"
        ));
    }

    /// Reviews and approves the Plan as it is.
    fn approve(&self) {
        let review = self.env.cpd(&["plan", "review", "--plan", "pay"]).ok();
        let id = review["review"]["id"].as_str().unwrap().to_owned();
        self.env.cpd(&["plan", "decide", &id, "--approve", "--plan", "pay"]).ok();
    }

    fn lock(&self) -> Run {
        self.env.cpd(&["plan", "lock", "--plan", "pay"])
    }

    /// The Plan locked, as `plan lock` leaves it, without starting any PR.
    fn dispatch(&self) {
        self.env.sql("UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';");
    }

    /// PR `key` `open`, one allowed step at a time.
    fn open(&self, key: &str) {
        self.env.sql(&format!(
            "UPDATE prs SET status = 'dispatch', lead_worktree = '/x/{key}', started_at = 't' WHERE plan = 'pay' AND key = '{key}';
             INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
               VALUES ('acme/{key}', 1, 'https://github.com/acme/{key}/pull/1', 'T', 0, 'open', 'none', 'none', 't', 't');
             UPDATE prs SET status = 'open', pr_number = 1, pr_url = 'https://github.com/acme/{key}/pull/1', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/{key}') WHERE plan = 'pay' AND key = '{key}';"
        ));
    }

    /// An `open` PR `key` merged.
    fn merge(&self, key: &str) {
        self.env.sql(&format!(
            "UPDATE prs SET status = 'merged', merged_at = 't' WHERE plan = 'pay' AND key = '{key}';
             UPDATE pull_requests SET state = 'merged', merged_at = 't' WHERE repo = 'acme/{key}';"
        ));
    }

    fn merged(&self, key: &str) {
        self.open(key);
        self.merge(key);
    }

    /// Plan `pay` in `dispatch` with `a1` merged and `a2` (needing it) waiting, both in `repo`.
    fn a2_ready(&self, repo: &Path) {
        self.add("a1", repo, &[]);
        self.add("a2", repo, &["a1"]);
        self.dispatch();
        self.merged("a1");
    }

    fn start(&self, key: &str) -> Run {
        self.env.cpd(&["pr", "start", key, "--plan", "pay"])
    }

    fn pr_status(&self, key: &str) -> String {
        self.query(&format!("SELECT status FROM prs WHERE plan = 'pay' AND key = '{key}'"))
    }

    /// Where PR `key`'s Lead worktree goes in `repo`.
    fn lead_path(&self, repo: &Path, key: &str) -> PathBuf {
        let name = repo.file_name().unwrap().to_str().unwrap();
        repo.parent().unwrap().join(format!("{name}.pay.{key}"))
    }

    fn log(&self, key: &str) -> String {
        self.env.cpd_home().join(format!("run/pay/logs/lead-{key}.pre-start.log")).display().to_string()
    }

    /// Commits `.config/wt.toml` with `pre-start = <command>` to `repo`'s `main` and pushes it.
    fn pre_start(&self, repo: &Repo, command: &str) {
        fs::create_dir_all(repo.path.join(".config")).unwrap();
        fs::write(repo.path.join(".config/wt.toml"), format!("pre-start = \"{command}\"\n")).unwrap();
        self.env.git(&repo.path, &["add", "-A"]);
        self.env.git(&repo.path, &["commit", "-q", "-m", "pre-start"]);
        self.env.git(&repo.path, &["push", "-q", "origin", "main"]);
    }

    /// Lands a commit on the remote `main` through a second clone, so `repo` has not fetched it.
    fn land_on_origin(&self, repo: &Repo, message: &str) -> String {
        let other = self.env.root().join("other-clone");
        let _ = fs::remove_dir_all(&other);
        self.env.git(self.env.root(), &["clone", "-q", repo.origin.to_str().unwrap(), other.to_str().unwrap()]);
        self.env.git(&other, &["commit", "-q", "--allow-empty", "-m", message]);
        self.env.git(&other, &["push", "-q", "origin", "main"]);
        self.env.git(&other, &["rev-parse", "HEAD"])
    }

    /// A running `cpd watch`, killed when dropped.
    fn engine(&self) -> Engine {
        let child = self.env.command(CPD).arg("watch").stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let engine = Engine { child: Some(child) };
        wait_until("watch-started", || !self.events("watch-started").is_empty());
        engine
    }
}

struct Engine {
    child: Option<Child>,
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn head(fx: &Fx, repo: &Path, rev: &str) -> String {
    fx.env.git(repo, &["rev-parse", rev])
}

fn remote_branch(fx: &Fx, repo: &Path, branch: &str) -> String {
    fx.env.git(repo, &["ls-remote", "--heads", "origin", branch])
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
        json!({"ok": true, "plan": "pay", "phase": "dispatch", "jobs": ["schema", "ui"],
               "prs": [{"key": "api", "status": "dispatch", "needs": [], "lead_branch": "cpd/pay/api",
                        "lead_worktree": worktree.display().to_string(), "base_sha": base, "reused": false, "pushed": true},
                       {"key": "web", "status": "waiting", "needs": ["api"], "lead_branch": "cpd/pay/web",
                        "lead_worktree": null, "base_sha": null, "reused": null, "pushed": null}],
               "watch": {"started": true, "pane": "%9"}})
    );
    // The started PR: branch, worktree, remote and row.
    assert_eq!(fx.env.git(&worktree, &["symbolic-ref", "--short", "HEAD"]), "cpd/pay/api");
    assert_eq!(remote_branch(&fx, &api.path, "cpd/pay/api"), format!("{base}\trefs/heads/cpd/pay/api"));
    assert_eq!(fx.env.git(&worktree, &["rev-parse", "--abbrev-ref", "@{upstream}"]), "origin/cpd/pay/api");
    assert_eq!(fx.pr_status("api"), "dispatch");
    assert_eq!(fx.query::<String>("SELECT lead_worktree FROM prs WHERE key = 'api'"), worktree.display().to_string());
    // The waiting one: nothing created anywhere.
    assert_eq!(fx.pr_status("web"), "waiting");
    assert!(!fx.lead_path(&web.path, "web").exists());
    assert_eq!(fx.env.git(&web.path, &["branch", "--list", "cpd/pay/*"]), "");
    assert_eq!(remote_branch(&fx, &web.path, "cpd/pay/web"), "");
    // Every Job is queued at once.
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM jobs WHERE status = 'queued' AND fingerprint IS NOT NULL"), 2);
    assert_eq!(fx.query::<String>("SELECT phase FROM plans WHERE id = 'pay'"), "dispatch");

    let phase = fx.events("plan-phase");
    assert_eq!(phase.last().unwrap()["data"], json!({"from": "plan", "to": "dispatch", "jobs": ["schema", "ui"], "prs": ["api"]}));
    let started = fx.events("pr-started");
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!(
        started[0]["data"],
        json!({"pr": "api", "repo": api.path.display().to_string(), "base": "main", "lead_branch": "cpd/pay/api",
               "lead_worktree": worktree.display().to_string(), "base_sha": base, "reused": false, "pushed": true, "by": "lock"})
    );
    assert_eq!(started[0]["plan"], "pay");
    assert!(fx.events("error").is_empty());

    // Again: `already`, the PRs as they are, nothing started.
    let again = fx.lock().ok();
    assert_eq!(again["already"], true);
    assert_eq!(again["prs"][0]["status"], "dispatch");
    assert_eq!(again["prs"][1]["status"], "waiting");
    assert_eq!(fx.events("pr-started").len(), 1);
}

#[test]
fn lock_is_all_or_nothing_when_a_pr_fails_to_start() {
    let fx = fx();
    let (api, web) = (fx.repo("api"), fx.repo("web"));
    fx.pre_start(&web, "echo setting up; exit 3");
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
        assert_eq!(fx.env.git(&repo.path, &["branch", "--list", "cpd/pay/*"]), "", "{key}'s branch");
        assert_eq!(remote_branch(&fx, &repo.path, &format!("cpd/pay/{key}")), "", "{key} on origin");
    }
    assert_eq!(fx.query::<String>("SELECT phase FROM plans WHERE id = 'pay'"), "plan");
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM prs WHERE status <> 'waiting'"), 0);
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM jobs WHERE status = 'draft'"), 2);
    assert!(fx.events("pr-started").is_empty());
    assert!(fx.env.calls("cpd").iter().all(|c| c[0] != "watch"));
}

#[test]
fn lock_refuses_a_pr_without_a_draft_job() {
    let fx = fx();
    let (api, web) = (fx.repo("api"), fx.repo("web"));
    fx.add("api", &api.path, &[]);
    fx.add("web", &web.path, &[]);
    fx.job("ui", "web");
    fx.approve();

    let error = refused(&fx.lock(), "pr_empty", 3);

    assert_eq!(error["details"]["prs"], json!(["api"]));
    assert_eq!(fx.query::<String>("SELECT phase FROM plans WHERE id = 'pay'"), "plan");
    assert!(!fx.lead_path(&web.path, "web").exists());
}

#[test]
fn lock_refuses_a_remote_lead_branch_of_a_pr_that_lacks_the_base() {
    let fx = fx();
    let (api, web) = (fx.repo("api"), fx.repo("web"));
    fx.add("api", &api.path, &[]);
    fx.add("web", &web.path, &[]);
    fx.job("schema", "api");
    fx.job("ui", "web");
    fx.approve();
    // web's Lead Branch is on the remote at the old main; then main moves on.
    fx.env.git(&web.path, &["push", "-q", "origin", "main:refs/heads/cpd/pay/web"]);
    let old = head(&fx, &web.path, "HEAD");
    fx.land_on_origin(&web, "main moves on");

    let error = refused(&fx.lock(), "branch_exists", 3);

    assert_eq!(error["details"]["pr"], "web");
    assert_eq!(error["details"]["branch"], "cpd/pay/web");
    assert_eq!(remote_branch(&fx, &web.path, "cpd/pay/web"), format!("{old}\trefs/heads/cpd/pay/web"));
    // All or nothing: api, started first, is not left behind or pushed.
    assert!(!fx.lead_path(&api.path, "api").exists());
    assert_eq!(remote_branch(&fx, &api.path, "cpd/pay/api"), "");
    assert_eq!(fx.query::<String>("SELECT phase FROM plans WHERE id = 'pay'"), "plan");
}

// ---------- pr start ----------

#[test]
fn pr_start_refusals_change_nothing() {
    let fx = fx();
    let repo = fx.repo("pay");
    fx.add("a1", &repo.path, &[]);
    fx.add("a2", &repo.path, &["a1"]);
    fx.add("c", &repo.path, &[]);
    fx.env.cpd(&["pr", "cancel", "c", "--plan", "pay"]).ok();

    let error = refused(&fx.start("a1"), "illegal_phase", 3);
    assert_eq!((&error["details"]["phase"], &error["details"]["needs"]), (&json!("plan"), &json!("dispatch")));
    fx.dispatch();
    refused(&fx.start("zz"), "pr_not_found", 4);
    refused(&fx.start("c"), "illegal_transition", 3);
    let error = refused(&fx.start("a2"), "pr_not_ready", 3);
    assert_eq!(error["details"]["needs"], json!(["a1"]));
    assert_eq!(fx.env.cpd(&["pr", "start", "a2"]).error()["code"], "plan_required");

    assert_eq!(fx.pr_status("a2"), "waiting");
    assert_eq!(fx.env.git(&repo.path, &["branch", "--list", "cpd/pay/*"]), "");
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
        json!({"ok": true, "pr": {"key": "a2", "status": "dispatch", "lead_branch": "cpd/pay/a2",
                                  "lead_worktree": worktree.display().to_string(), "base_sha": tip, "reused": false, "pushed": true}})
    );
    assert_eq!(fx.env.git(&worktree, &["rev-parse", "HEAD"]), tip, "cut from a fresh fetch of origin/main");
    assert_eq!(remote_branch(&fx, &repo.path, "cpd/pay/a2"), format!("{tip}\trefs/heads/cpd/pay/a2"));
    assert_eq!(fx.env.git(&worktree, &["rev-parse", "--abbrev-ref", "@{upstream}"]), "origin/cpd/pay/a2");
    assert_eq!(fx.pr_status("a2"), "dispatch");
    assert!(fx.query::<Option<String>>("SELECT start_failed_at FROM prs WHERE key = 'a2'").is_none());
    let started = fx.events("pr-started");
    assert_eq!((started.len(), &started[0]["data"]["by"], &started[0]["data"]["pr"]), (1, &json!("pr start"), &json!("a2")));
    assert!(fx.events("error").is_empty());

    // Idempotent: the PR is started, so it is `already`.
    let again = fx.start("a2").ok();
    assert_eq!((&again["already"], &again["pr"]["status"]), (&json!(true), &json!("dispatch")));
    assert_eq!(fx.events("pr-started").len(), 1);
}

#[test]
fn pr_start_reuses_a_remote_lead_branch_that_contains_the_base() {
    let fx = fx();
    let repo = fx.repo("pay");
    fx.a2_ready(&repo.path);
    // A commit only the remote has, on top of the base, on the PR's branch.
    fx.env.git(&repo.path, &["commit", "-q", "--allow-empty", "-m", "only on the remote"]);
    let tip = head(&fx, &repo.path, "HEAD");
    fx.env.git(&repo.path, &["push", "-q", "origin", "HEAD:refs/heads/cpd/pay/a2"]);
    fx.env.git(&repo.path, &["reset", "-q", "--hard", "HEAD~1"]);

    let out = fx.start("a2").ok();

    let worktree = fx.lead_path(&repo.path, "a2");
    assert_eq!((&out["pr"]["base_sha"], &out["pr"]["reused"], &out["pr"]["pushed"]), (&json!(tip), &json!(true), &json!(false)));
    assert_eq!(fx.env.git(&worktree, &["rev-parse", "HEAD"]), tip);
    assert_eq!(remote_branch(&fx, &repo.path, "cpd/pay/a2"), format!("{tip}\trefs/heads/cpd/pay/a2"));
    assert_eq!(fx.events("pr-started")[0]["data"]["reused"], true);
}

#[test]
fn pr_start_refuses_a_remote_lead_branch_that_lacks_the_base() {
    let fx = fx();
    let repo = fx.repo("pay");
    fx.a2_ready(&repo.path);
    fx.env.git(&repo.path, &["push", "-q", "origin", "main:refs/heads/cpd/pay/a2"]);
    let old = head(&fx, &repo.path, "HEAD");
    fx.land_on_origin(&repo, "main moves on");

    let error = refused(&fx.start("a2"), "branch_exists", 3);

    assert_eq!((&error["details"]["pr"], &error["details"]["branch"]), (&json!("a2"), &json!("cpd/pay/a2")));
    assert_eq!(fx.env.git(&repo.path, &["branch", "--list", "cpd/pay/*"]), "");
    assert!(!fx.lead_path(&repo.path, "a2").exists());
    assert_eq!(remote_branch(&fx, &repo.path, "cpd/pay/a2"), format!("{old}\trefs/heads/cpd/pay/a2"));
    assert_eq!(fx.pr_status("a2"), "waiting");
    assert!(fx.events("pr-started").is_empty());
}

#[test]
fn a_push_failure_at_pr_start_still_starts_it_and_appends_one_push_failed_error() {
    let fx = fx();
    let repo = fx.repo("pay");
    fx.a2_ready(&repo.path);
    let hook = repo.origin.join("hooks").join("pre-receive");
    write_executable(&hook, "#!/bin/sh\necho refused by hook >&2\nexit 1\n");

    let out = fx.start("a2").ok();

    assert_eq!((&out["pr"]["status"], &out["pr"]["pushed"]), (&json!("dispatch"), &json!(false)));
    assert_eq!(fx.pr_status("a2"), "dispatch");
    assert_eq!(remote_branch(&fx, &repo.path, "cpd/pay/a2"), "");
    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let data = &errors[0]["data"];
    assert_eq!((&data["code"], &data["source"], &data["details"]["pr"]), (&json!("push_failed"), &json!("pr start"), &json!("a2")));
    assert!(data["details"]["stderr"].as_str().unwrap().contains("refused by hook"));
}

#[test]
fn a_repo_without_origin_starts_a_pr_as_before() {
    let fx = fx();
    let repo = fx.env.git_repo("pay").canonicalize().unwrap();
    fx.a2_ready(&repo);
    let base = head(&fx, &repo, "HEAD");

    let out = fx.start("a2").ok();

    assert_eq!((&out["pr"]["base_sha"], &out["pr"]["reused"], &out["pr"]["pushed"]), (&json!(base), &json!(false), &json!(false)));
    assert_eq!(fx.env.git(&repo, &["remote"]), "");
    assert_eq!(fx.env.git(&fx.lead_path(&repo, "a2"), &["symbolic-ref", "--short", "HEAD"]), "cpd/pay/a2");
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
    assert_eq!(fx.env.git(&worktree, &["symbolic-ref", "--short", "HEAD"]), "cpd/pay");
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
    // merges when the file `merged` exists.
    let merged = fx.env.root().join("merged");
    let node = r#"{"repository":{"nameWithOwner":"acme/a1"},"number":1,"url":"https://github.com/acme/a1/pull/1","title":"T","isDraft":false,"state":"'"$state"'","createdAt":"t","mergedAt":'"$at"',"reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","commits":{"nodes":[]}}"#;
    fx.env.fake_gh(&format!(
        r#"case "$4" in
  *"search(type: ISSUE"*) echo '{EMPTY_SEARCH}' ;;
  *) if [ -e '{merged}' ]; then state=MERGED; at='"2026-10-14T10:00:00Z"'; else state=OPEN; at=null; fi
     echo '{{"data":{{"p0":{{"pullRequest":{node}}}}}}}' ;;
esac"#,
        merged = merged.display()
    ));
    let engine = fx.engine();

    // a1 is not merged: nothing starts, however many ticks pass (two whole ticks of the engine).
    fx.env.wait_whole_ticks(2);
    assert_eq!((fx.pr_status("a2"), fx.pr_status("a3")), ("waiting".to_owned(), "waiting".to_owned()));

    // a1 merges on GitHub: what it brought is on the remote, which this repo has not fetched.
    let tip = fx.land_on_origin(&repo, "what a1 merged");
    fs::write(&merged, "").unwrap();
    wait_until("E7 to start a2", || fx.pr_status("a2") == "dispatch");

    let worktree = fx.lead_path(&repo.path, "a2");
    assert_eq!(fx.env.git(&worktree, &["rev-parse", "HEAD"]), tip);
    assert_eq!(fx.env.git(&worktree, &["symbolic-ref", "--short", "HEAD"]), "cpd/pay/a2");
    assert_eq!(remote_branch(&fx, &repo.path, "cpd/pay/a2"), format!("{tip}\trefs/heads/cpd/pay/a2"));
    let started = fx.events("pr-started");
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!(
        started[0]["data"],
        json!({"pr": "a2", "repo": repo.path.display().to_string(), "base": "main", "lead_branch": "cpd/pay/a2",
               "lead_worktree": worktree.display().to_string(), "base_sha": tip, "reused": false, "pushed": true, "by": "engine"})
    );
    // E5 saw a1 merged (the R34 merged path) before E7 started a2.
    assert_eq!(fx.events("pr-merged").len(), 1);
    // a3 needs a2, which is started but not merged.
    assert_eq!(fx.pr_status("a3"), "waiting");
    assert!(fx.events("error").is_empty());
    drop(engine);
}

#[test]
fn a_failed_start_marks_the_pr_and_appends_one_error_until_pr_start_retries() {
    let fx = fx();
    let repo = fx.repo("pay");
    let marker = fx.env.root().join("setup-ok");
    fx.pre_start(&repo, &format!("test -e {}", marker.display()));
    fx.a2_ready(&repo.path);
    let engine = fx.engine();

    wait_until("E7's pr_start_failed", || !fx.events("error").is_empty());

    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["plan"], "pay");
    let data = &errors[0]["data"];
    assert_eq!((&data["code"], &data["source"]), (&json!("pr_start_failed"), &json!("watch")));
    assert_eq!((&data["details"]["pr"], &data["details"]["step"], &data["details"]["log"]), (&json!("a2"), &json!("pre-start"), &json!(fx.log("a2"))));
    assert_eq!(fx.pr_status("a2"), "waiting");
    let failed_at: String = fx.query("SELECT start_failed_at FROM prs WHERE key = 'a2'");
    assert!(!failed_at.is_empty());
    assert_eq!(fx.env.git(&repo.path, &["branch", "--list", "cpd/pay/*"]), "", "the branch is removed");
    assert!(!fx.lead_path(&repo.path, "a2").exists(), "the worktree is removed");

    // The engine does not try again.
    fx.env.wait_whole_ticks(3);
    assert_eq!(fx.events("error").len(), 1);
    assert!(fx.events("pr-started").is_empty());

    // `pr start` fails the same way and changes nothing.
    let writes = (fx.query::<i64>("SELECT count(*) FROM txns"), fx.query::<i64>("SELECT count(*) FROM events"));
    let error = refused(&fx.start("a2"), "setup_failed", 1);
    assert_eq!((&error["details"]["pr"], &error["details"]["step"], &error["details"]["log"]), (&json!("a2"), &json!("pre-start"), &json!(fx.log("a2"))));
    assert_eq!(fx.query::<String>("SELECT start_failed_at FROM prs WHERE key = 'a2'"), failed_at);
    assert_eq!((fx.query::<i64>("SELECT count(*) FROM txns"), fx.query::<i64>("SELECT count(*) FROM events")), writes);

    // The cause is fixed: `pr start` starts it and clears the mark.
    fs::write(&marker, "").unwrap();
    let out = fx.start("a2").ok();
    assert_eq!(out["pr"]["status"], "dispatch");
    assert_eq!(fx.pr_status("a2"), "dispatch");
    assert!(fx.query::<Option<String>>("SELECT start_failed_at FROM prs WHERE key = 'a2'").is_none());
    let started = fx.events("pr-started");
    assert_eq!((started.len(), &started[0]["data"]["by"]), (1, &json!("pr start")));
    assert_eq!(fx.events("error").len(), 1);
    drop(engine);
}
