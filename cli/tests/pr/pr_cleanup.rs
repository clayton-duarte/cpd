//! The clean-up `pr poll` runs when it sees a PR merged (commands.md §9.2,
//! contract §7.2; hs-d138, hs-d142): `cpd pr poll` in-process on `run_with` over the fake ports (the git route with a
//! bare `origin`, a `gh` handler, the `FakeSpawn` for the Gate's pid), and Plans written straight into the database.

use crate::common;

use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

use common::empty_comments;
use common::fx::Fx as Kit;
use cpd_core::git::{Setup, create_attempt_worktree, gate_worktree_path, prepare_lead_worktree, pr_lead_worktree_path};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::host::spawn::{Launch, Pid, Signal, Spawn};
use cpd_core::ids::{AttemptId, PlanId, PrKey};
use cpd_core::store::Store;
use rusqlite::params;
use serde_json::{Value, json};

/// What the fake `gh` prints: the search, the lookup and `gh pr view` (the pull request's `headRefOid`, contract §7.2 "The
/// recorded head"); without a view, `gh pr view` fails.
struct Answers {
    search: String,
    lookup: String,
    view: Option<String>,
}

/// The in-process `gh` of the clean-up tests; any call but those three exits 9.
pub(crate) struct Gh(Mutex<Answers>);

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let answers = self.0.lock().unwrap();
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["api", "graphql", "-f", query, ..] if query.contains("search(type: ISSUE") => Reply::ok(&answers.search),
            ["api", "graphql", "-f", query, ..] if query.contains("reviewThreads") => Reply::ok(&empty_comments(query)),
            ["api", "graphql", "-f", query, ..] if query.contains("p0: repository") => Reply::ok(&answers.lookup),
            ["api", "graphql", "-f", query, ..] => Reply::exit(9, "", &format!("unexpected graphql query: {query}\n")),
            ["pr", "view", ..] => match &answers.view {
                Some(view) => Reply::ok(view),
                None => Reply::exit(1, "", "no such file: view.json\n"),
            },
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

/// A pull request of `acme/pay` as the search and the lookup print it.
pub(crate) fn node(number: i64, state: &str) -> String {
    let merged = if state == "MERGED" { r#""2026-10-14T10:00:00Z""# } else { "null" };
    format!(
        r#"{{"repository":{{"nameWithOwner":"acme/pay"}},"number":{number},"url":"https://github.com/acme/pay/pull/{number}","title":"PR {number}",
        "isDraft":false,"state":"{state}","createdAt":"opened","updatedAt":"U1","mergedAt":{merged},"reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","viewerDidAuthor":true,"viewerLatestReviewRequest":null,
        "commits":{{"nodes":[{{"commit":{{"statusCheckRollup":null}}}}]}}}}"#
    )
}

pub(crate) fn search_answer(nodes: &[String]) -> String {
    format!(
        r#"{{"data":{{"search":{{"issueCount":{},"nodes":[{}]}},"reviewRequests":{{"issueCount":0,"nodes":[]}},"rateLimit":{{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}}}}"#,
        nodes.len(),
        nodes.join(",")
    )
}

/// One PR of the Plan: its Lead worktree and one failed Attempt's worktree.
pub(crate) struct Pr {
    pub(crate) key: &'static str,
    pub(crate) number: i64,
    pub(crate) lead: PathBuf,
    pub(crate) lead_branch: String,
    pub(crate) attempt: PathBuf,
    pub(crate) attempt_branch: String,
}

/// Plan `pay` in phase `deploy`, in one repo with a bare `origin`, with one PR per key; every PR is
/// open, its Lead Branch is pushed and its Lead worktree is clean. It derefs to the [`Kit`] (`env`, `git`, `calls`, ...).
pub(crate) struct Fx {
    kit: Kit,
    pub(crate) home: CpdHome,
    pub(crate) repo: PathBuf,
    pub(crate) prs: Vec<Pr>,
    gh: Arc<Gh>,
}

impl Deref for Fx {
    type Target = Kit;

    fn deref(&self) -> &Kit {
        &self.kit
    }
}

fn plan() -> PlanId {
    PlanId::parse("pay").unwrap()
}

pub(crate) fn fx(keys: &[&'static str]) -> Fx {
    let kit = Kit::with_ports();
    // The PRs are of the repo `acme/pay` (their `name`): its policy file is required, empty unless a test sets hooks.
    let repo = kit.keyed_repo("pay").canonicalize().unwrap();
    let origin = kit.env.root().join("repos/origin.git");
    kit.git(kit.env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
    kit.local_origin(&repo, &origin);
    kit.git(&repo, &["push", "-q", "origin", "main"]);

    kit.env.write_config(&format!("version = 1\nmachine = \"t\"\n[tmux]\nsocket = \"{}\"\n", kit.env.tmux_socket()));
    let home = CpdHome::resolve(Some(kit.env.cpd_home().into()), None).unwrap();
    let gh = Arc::new(Gh(Mutex::new(Answers { search: format!("{}\n", search_answer(&[])), lookup: "{\"data\":{}}\n".to_owned(), view: None })));
    let _ = (*kit.fakes.runner).clone().route("gh", gh.clone());

    let setup = Setup { timeout: Duration::from_secs(30), log: kit.env.root().join("setup.log") };
    let mut prs = Vec::new();
    for (i, key) in keys.iter().enumerate() {
        let lead_branch = format!("cpd/pay/{key}");
        let lead = pr_lead_worktree_path(&repo, &plan(), &PrKey::parse(key).unwrap(), &lead_branch).unwrap();
        let prepared = kit.scoped(|| prepare_lead_worktree(&repo, &plan(), &lead, &lead_branch, "main", &setup)).map_err(|f| f.error).unwrap();
        kit.git(&lead, &["config", "user.name", "cpd test"]);
        kit.git(&lead, &["config", "user.email", "test@cpd.invalid"]);
        kit.commit_file(&lead, &format!("{key}.txt"), "x\n", key);
        kit.git(&lead, &["push", "-q", "-u", "origin", &lead_branch]);
        let attempt = AttemptId::parse(&format!("{key}-a1")).unwrap();
        let made = kit.scoped(|| create_attempt_worktree(&repo, &lead, &plan(), &attempt, &setup)).unwrap();
        prs.push(Pr {
            key,
            number: 812 + i as i64,
            lead,
            lead_branch: prepared.branch,
            attempt: made.path,
            attempt_branch: made.branch,
        });
    }
    let mut sql = String::from(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay the bills', 'deploy', 'lead1', 't', 't');",
    );
    for pr in &prs {
        sql += &format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
               VALUES ('acme/pay', {n}, 'https://github.com/acme/pay/pull/{n}', 'PR {n}', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');
             INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                              pr_number, pr_url, pr_opened_at, pull_request)
               VALUES ('pay', '{key}', '{repo}', 'pay', 'main', 'open', '{branch}', '{lead}', cpd_txn(), 't', 't',
                       {n}, 'https://github.com/acme/pay/pull/{n}', 'opened', (SELECT id FROM pull_requests WHERE number = {n}));",
            n = pr.number,
            key = pr.key,
            repo = repo.display(),
            branch = pr.lead_branch,
            lead = pr.lead.display(),
        );
    }
    let fx = Fx { kit, home, repo, prs, gh };
    fx.store()
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            for pr in &fx.prs {
                tx.execute(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, cancel_reason, cancel_requested_at, created_txn, created_at)
                     VALUES ('pay', ?1, ?1, 'B', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'cancelled', 0, ?2, 'no', 't', ?3, 't')",
                    params![pr.key, "f".repeat(64), tx.id()],
                )?;
                tx.execute(
                    "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, base_sha,
                                           started_at, ended_at)
                     VALUES ('pay', ?1, ?2, 1, 'S2', 'ladder', 'ended', 'failed', 'crashed', ?3, ?4, 'sha', 't', 't')",
                    params![format!("{}-a1", pr.key), pr.key, pr.attempt_branch, pr.attempt.to_str().unwrap()],
                )?;
            }
            Ok(())
        })
        .unwrap();
    fx
}

impl Fx {
    pub(crate) fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    /// What `gh`'s search and lookup answer, as the text of their stdout.
    pub(crate) fn answer(&self, search: &str, lookup: &str) {
        let mut answers = self.gh.0.lock().unwrap();
        answers.search = format!("{search}\n");
        answers.lookup = format!("{lookup}\n");
    }

    /// The pull requests GitHub reports: `open` ones in the search, `merged` ones in the lookup.
    fn github(&self, open: &[&str], merged: &[&str]) {
        let number = |key: &str| self.prs.iter().find(|p| p.key == key).unwrap().number;
        let nodes: Vec<String> = open.iter().map(|k| node(number(k), "OPEN")).collect();
        let aliases: Vec<String> =
            merged.iter().enumerate().map(|(i, k)| format!(r#""p{i}":{{"pullRequest":{}}}"#, node(number(k), "MERGED"))).collect();
        self.answer(&search_answer(&nodes), &format!(r#"{{"data":{{{}}}}}"#, aliases.join(",")));
    }

    pub(crate) fn poll(&self) -> Value {
        self.cpd(&["pr", "poll"]).ok()
    }

    pub(crate) fn pr(&self, key: &str) -> &Pr {
        self.prs.iter().find(|p| p.key == key).unwrap()
    }

    pub(crate) fn branch_exists(&self, branch: &str) -> bool {
        !self.git(&self.repo, &["branch", "--list", branch]).is_empty()
    }

    /// Whether the PR's worktrees and local branches are all gone: `(attempt, lead)`.
    pub(crate) fn gone(&self, key: &str) -> (bool, bool) {
        let pr = self.pr(key);
        (
            !pr.attempt.exists() && !self.branch_exists(&pr.attempt_branch),
            !pr.lead.exists() && !self.branch_exists(&pr.lead_branch),
        )
    }

    /// `UPDATE attempts SET <set>` for one Attempt of the Plan.
    fn set_attempt(&self, id: &str, set: &str) {
        self.store().write("fixture", |tx| Ok(tx.execute(&format!("UPDATE attempts SET {set} WHERE plan = 'pay' AND id = ?1"), [id])?)).unwrap();
    }

    /// The `data` of the Plan's `error` Events, in order.
    pub(crate) fn errors(&self) -> Vec<Value> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare("SELECT data FROM events WHERE plan = 'pay' AND kind = 'error' ORDER BY id")?;
                let rows = stmt
                    .query_map([], |r| Ok(serde_json::from_str(&r.get::<_, String>(0)?).unwrap()))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    /// A process the `FakeSpawn` started: running, with the pid it gave.
    fn started(&self) -> u32 {
        self.fakes.spawn.spawn(&Launch { program: "cpd".to_owned(), ..Launch::default() }).unwrap().0
    }

    /// The pid of a process the `FakeSpawn` started and ended.
    fn ended_pid(&self) -> u32 {
        let pid = self.started();
        self.fakes.spawn.exit(Pid(pid), 0);
        pid
    }
}

#[test]
fn a_merged_pr_loses_its_worktrees_and_branches_and_another_pr_keeps_everything() {
    let fx = fx(&["api", "web"]);
    fx.github(&["web"], &["api"]);

    let out = fx.poll();

    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "api"}]));
    assert_eq!(fx.gone("api"), (true, true));
    assert_eq!(fx.gone("web"), (false, false));
    assert!(fx.pr("web").attempt.exists() && fx.pr("web").lead.exists());
    assert!(fx.branch_exists(&fx.pr("web").attempt_branch) && fx.branch_exists(&fx.pr("web").lead_branch));
    assert_eq!(fx.errors(), Vec::<Value>::new());
    let remote = fx.git(&fx.repo, &["ls-remote", "--heads", "origin", "cpd/pay/api"]);
    assert!(!remote.is_empty(), "the remote branch is never touched");
}

#[test]
fn a_dirty_lead_worktree_is_kept_with_one_error_event() {
    let fx = fx(&["api"]);
    let api = fx.pr("api");
    fs::write(api.lead.join("untracked.txt"), "wip\n").unwrap();
    fx.github(&[], &["api"]);

    fx.poll();

    assert!(api.lead.exists() && fx.branch_exists(&api.lead_branch));
    assert!(!api.attempt.exists() && !fx.branch_exists(&api.attempt_branch), "the Attempt's worktree goes anyway");
    let errors = fx.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["source"], "pr poll");
    assert_eq!(errors[0]["code"], "cleanup_failed");
    assert_eq!(errors[0]["details"], json!({"plan": "pay", "pr": "api", "path": api.lead.to_str().unwrap(), "reason": "dirty"}));

    fx.poll();

    assert_eq!(fx.errors().len(), 1, "later polls skip it silently");
    assert!(api.lead.exists());

    // Once it is clean, a later poll removes it, still with no Event.
    fs::remove_file(api.lead.join("untracked.txt")).unwrap();
    fx.poll();
    assert_eq!(fx.gone("api"), (true, true));
    assert_eq!(fx.errors().len(), 1);
}

#[test]
fn a_lead_worktree_with_a_commit_not_on_origin_is_kept() {
    let fx = fx(&["api"]);
    let api = fx.pr("api");
    fs::write(api.lead.join("more.txt"), "more\n").unwrap();
    fx.git(&api.lead, &["add", "-A"]);
    fx.git(&api.lead, &["commit", "-q", "-m", "not pushed"]);
    fx.github(&[], &["api"]);

    fx.poll();

    assert!(api.lead.exists() && fx.branch_exists(&api.lead_branch));
    let errors = fx.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["code"], "cleanup_failed");
    assert_eq!(errors[0]["details"]["reason"], "unpushed");
    assert_eq!(errors[0]["details"]["path"], api.lead.to_str().unwrap());
}

impl Fx {
    /// What `gh pr view --json headRefOid` prints: GitHub keeps the pull request's head commit after the merge.
    fn recorded_head(&self, oid: &str) {
        self.gh.0.lock().unwrap().view = Some(format!("{}\n", json!({"headRefOid": oid})));
    }

    /// The head branch is gone from `origin`, as after a squash merge with "Automatically delete head branches" and a
    /// pruning fetch: no `origin/<branch>` either, so the PR's own commits are outside every `origin/*` ref.
    fn delete_remote_branch(&self, key: &str) {
        let branch = self.pr(key).lead_branch.clone();
        self.git(&self.repo, &["push", "-q", "origin", "--delete", &branch]);
        assert_eq!(self.git(&self.repo, &["branch", "-r", "--list", &format!("origin/{branch}")]), "", "origin/{branch} is gone");
        let unpushed = self.git(&self.pr(key).lead, &["rev-list", "HEAD", "--not", "--remotes=origin"]);
        assert!(!unpushed.is_empty(), "the Lead worktree's commits look unpushed now");
    }
}

#[test]
fn a_squash_merged_pr_whose_lead_worktree_holds_the_recorded_head_is_removed() {
    let fx = fx(&["api"]);
    let api = fx.pr("api");
    fx.recorded_head(&fx.git(&api.lead, &["rev-parse", "HEAD"]));
    fx.delete_remote_branch("api");
    fx.github(&[], &["api"]);

    fx.poll();

    assert_eq!(fx.gone("api"), (true, true), "the worktree holds exactly what was merged");
    assert_eq!(fx.errors(), Vec::<Value>::new(), "no cleanup_failed `unpushed`");
}

#[test]
fn a_squash_merged_pr_whose_lead_worktree_is_behind_the_recorded_head_is_removed() {
    let fx = fx(&["api"]);
    let api = fx.pr("api");
    // The pull request's head is one commit past the worktree's HEAD, and the object is local.
    fs::write(api.lead.join("more.txt"), "more\n").unwrap();
    fx.git(&api.lead, &["add", "-A"]);
    fx.git(&api.lead, &["commit", "-q", "-m", "more"]);
    fx.recorded_head(&fx.git(&api.lead, &["rev-parse", "HEAD"]));
    fx.git(&api.lead, &["reset", "-q", "--hard", "HEAD~1"]);
    fx.delete_remote_branch("api");
    fx.github(&[], &["api"]);

    fx.poll();

    assert_eq!(fx.gone("api"), (true, true), "HEAD is contained in the recorded head");
    assert_eq!(fx.errors(), Vec::<Value>::new());
}

#[test]
fn a_squash_merged_pr_whose_lead_worktree_holds_a_commit_beyond_the_recorded_head_is_kept_unpushed() {
    let fx = fx(&["api"]);
    let api = fx.pr("api");
    fx.recorded_head(&fx.git(&api.lead, &["rev-parse", "HEAD"]));
    fs::write(api.lead.join("more.txt"), "more\n").unwrap();
    fx.git(&api.lead, &["add", "-A"]);
    fx.git(&api.lead, &["commit", "-q", "-m", "after the merged head"]);
    fx.delete_remote_branch("api");
    fx.github(&[], &["api"]);

    fx.poll();

    assert!(api.lead.exists() && fx.branch_exists(&api.lead_branch), "unpushed work is never removed");
    let errors = fx.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["code"], "cleanup_failed");
    assert_eq!(errors[0]["details"]["reason"], "unpushed");
}

#[test]
fn a_squash_merged_pr_whose_lead_worktree_holds_only_a_rule_s_merge_after_the_recorded_head_is_removed() {
    let fx = fx(&["api"]);
    let api = fx.pr("api");
    fx.recorded_head(&fx.git(&api.lead, &["rev-parse", "HEAD"]));
    // The base moves, and rule S merges it into the Lead worktree after the recorded head: one local merge commit,
    // no work of its own.
    fx.git(&fx.repo, &["-c", "user.name=t", "-c", "user.email=t@cpd.invalid", "commit", "-q", "--allow-empty", "-m", "base moves"]);
    fx.git(&fx.repo, &["push", "-q", "origin", "main"]);
    fx.git(&api.lead, &["merge", "-q", "--no-ff", "-m", "Merge remote-tracking branch 'origin/main'", "origin/main"]);
    // One local commit and it is a merge: the same two counts as `--min-parents=2` (the fake git has no such flag).
    assert_eq!(fx.git(&api.lead, &["rev-list", "--count", "HEAD", "--not", "--remotes=origin"]), "1");
    assert_eq!(fx.git(&api.lead, &["rev-list", "--no-merges", "--count", "HEAD", "--not", "--remotes=origin"]), "0");
    fx.delete_remote_branch("api");
    fx.github(&[], &["api"]);

    fx.poll();

    assert_eq!(fx.gone("api"), (true, true), "a merge of the base is not unpushed work");
    assert_eq!(fx.errors(), Vec::<Value>::new(), "no cleanup_failed `unpushed`");
}

#[test]
fn a_locked_attempt_worktree_survives_until_it_is_unlocked() {
    let fx = fx(&["api"]);
    let api = fx.pr("api");
    fx.universe.lock().unwrap().lock_worktree(&api.attempt, "");
    fx.github(&[], &["api"]);

    fx.poll();

    assert!(api.attempt.exists() && fx.branch_exists(&api.attempt_branch));
    assert!(!api.lead.exists() && !fx.branch_exists(&api.lead_branch), "the failure does not stop the Lead worktree's removal");
    let errors = fx.errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["code"], "cleanup_failed");
    assert_eq!(errors[0]["details"]["path"], api.attempt.to_str().unwrap());
    assert!(errors[0]["details"]["reason"].as_str().unwrap().contains("locked"), "{errors:?}");

    fx.poll();
    assert!(api.attempt.exists(), "still locked");
    assert_eq!(fx.errors().len(), 1, "a retry appends no Event");

    fx.universe.lock().unwrap().unlock_worktree(&api.attempt);
    fx.poll();

    assert_eq!(fx.gone("api"), (true, true));
    assert_eq!(fx.errors().len(), 1);
}

#[test]
fn an_attempt_that_has_not_ended_defers_the_clean_up_to_a_later_poll_with_no_event() {
    let fx = fx(&["api"]);
    let api = fx.pr("api");
    // A second Job of the PR, whose Attempt still runs.
    fx.store()
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, created_txn, created_at)
                 VALUES ('pay', 'api', 'late', 'L', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 1, ?1, ?2, 't')",
                params!["f".repeat(64), tx.id()],
            )?;
            tx.execute(
                "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
                 VALUES ('pay', 'late-a1', 'late', 1, 'S2', 'ladder', 'running', 'cpd/pay--late-a1', '/nowhere/late-a1', 'sha', 't')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    fx.github(&[], &["api"]);

    let out = fx.poll();

    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "api"}]));
    assert!(api.lead.exists() && fx.branch_exists(&api.lead_branch), "the Lead worktree waits for the Attempt");
    assert!(api.attempt.exists() && fx.branch_exists(&api.attempt_branch), "so does the Attempt's");
    assert_eq!(fx.errors(), Vec::<Value>::new());

    fx.set_attempt("late-a1", "status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = 't'");
    fx.poll();

    assert_eq!(fx.gone("api"), (true, true));
    assert_eq!(fx.errors(), Vec::<Value>::new());
}

#[test]
fn a_held_driver_lock_defers_the_clean_up_to_a_later_poll_with_no_event() {
    // A driver that ended its Attempt may still be in A9, removing its worktree (contract §7.2).
    let fx = fx(&["api"]);
    let api = fx.pr("api");
    fx.github(&[], &["api"]);
    let path = fx.home.attempt_lock(&plan(), &AttemptId::parse("api-a1").unwrap());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let held = fs::File::create(&path).unwrap();
    held.lock().unwrap();

    fx.poll();

    assert!(api.lead.exists() && fx.branch_exists(&api.lead_branch), "the Lead worktree stays under a held driver lock");
    assert!(api.attempt.exists() && fx.branch_exists(&api.attempt_branch));
    assert_eq!(fx.errors(), Vec::<Value>::new());

    fx.poll();
    assert!(api.lead.exists(), "still held");

    drop(held);
    fx.poll();

    assert_eq!(fx.gone("api"), (true, true));
    assert_eq!(fx.errors(), Vec::<Value>::new());
    assert!(path.exists(), "the probe neither creates nor removes the lock file");
}

#[test]
fn a_lead_worktree_a_merge_left_stale_is_brought_up_to_date_and_removed() {
    let fx = fx(&["api"]);
    let api = fx.pr("api");
    // A merge moved the Lead Branch and pushed it, but its rule S did not run.
    let attempt = &api.attempt;
    fs::write(attempt.join("merged.txt"), "merged\n").unwrap();
    fx.git(attempt, &["add", "-A"]);
    fx.git(attempt, &["commit", "-q", "-m", "merged"]);
    let new = fx.git(attempt, &["rev-parse", "HEAD"]);
    let old = fx.git(&fx.repo, &["rev-parse", &api.lead_branch]);
    fx.git(&fx.repo, &["update-ref", &format!("refs/heads/{}", api.lead_branch), &new, &old]);
    fx.git(&fx.repo, &["push", "-q", "origin", &api.lead_branch]);
    assert!(!fx.git(&api.lead, &["status", "--porcelain"]).is_empty(), "the stale tree looks like local changes");
    fx.github(&[], &["api"]);

    fx.poll();

    assert_eq!(fx.gone("api"), (true, true));
    assert_eq!(fx.errors(), Vec::<Value>::new());
}


