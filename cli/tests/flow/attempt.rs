//! `attempt submit`, `attempt read`, `attempt show` and `attempt list`
//! (commands.md §6.1–§6.3). `attempt submit` without a hook runs in this process on the fake git ([`Git`]: repos of the
//! `Universe`, no git process); the quick-check and kind-check tests still run the built binary against a temporary
//! `$CPD_HOME` and temporary git repos (they run `sh` hooks). `attempt read`, `attempt show` and `attempt list` call no git and no hook,
//! so their tests run the command tree in this process (`Fx`, [`Rows`]) against rows alone. The Plan, its Jobs,
//! Attempts and Sessions are fixtures written straight into the database; the commands under test are the only ones
//! that write the rest.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::fx::Fx as InProcess;
use common::{Run, TestEnv, refused};
use cpd_core::git::{Setup, create_attempt_worktree, create_lead_worktree, pr_lead_worktree_path, prepare_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId, PrKey};
use cpd_core::ports::Ports;
use cpd_core::store::Store;
use rusqlite::params;
use serde_json::{Value, json};

const PLAN: &str = "pay";

// ---------- fixtures ----------

/// A repo with a Lead worktree for Plan `pay`, a database with its Context, Lead
/// Session, Plan (phase `dispatch`) and Job `api` (`running`) with Attempt `api-a1`
/// (`running`) in its own worktree, cut from the Lead Branch.
struct Fx {
    env: TestEnv,
    home: CpdHome,
    repo: PathBuf,
    lead: PathBuf,
    /// The worktree of `api-a1`.
    work: PathBuf,
}

/// `files` are committed on `main` before the Lead worktree is cut from it.
fn fixture(files: &[(&str, &str)]) -> Fx {
    fixture_kind(files, "work", None)
}

/// [`fixture`] whose repo policy file is `file` (its `[hooks]`).
fn hooked(file: &str) -> Fx {
    let fx = fixture(&[]);
    fx.env.repo_policy("acme", "pay", file);
    fx
}

/// [`fixture`] with Job `api` of `kind` and, for a `test` Job, its `(tests, test_files)` as JSON arrays.
fn fixture_kind(files: &[(&str, &str)], kind: &str, tests: Option<(&str, &str)>) -> Fx {
    fixture_needing(files, kind, tests, None)
}

/// [`fixture_kind`] whose Job needs the merged `test` Job `t` naming `needs` (a JSON array), see [`Fx::attempt_needing`].
fn fixture_needing(files: &[(&str, &str)], kind: &str, tests: Option<(&str, &str)>, needs: Option<&str>) -> Fx {
    let env = TestEnv::new();
    let repo = env.git_repo("pay").canonicalize().unwrap();
    // The worktrees are created in this process, whose environment is not the TestEnv's.
    for (key, value) in [("user.name", "cpd test"), ("user.email", "test@cpd.invalid"), ("commit.gpgsign", "false")] {
        env.git(&repo, &["config", key, value]);
    }
    if !files.is_empty() {
        for (path, content) in files {
            let full = repo.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, content).unwrap();
        }
        env.git(&repo, &["add", "-A"]);
        env.git(&repo, &["commit", "-q", "-m", "setup"]);
    }
    let setup = Setup { timeout: Duration::from_secs(30), log: env.root().join("setup.log") };
    let lead = create_lead_worktree(&repo, &plan(), "main", &setup).unwrap().path;
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    // The PR is of the repo `acme/pay` (its `name`): its policy file is required, empty unless a test sets hooks.
    env.repo_policy("acme", "pay", "");
    let mut fx = Fx { env, home, repo, lead, work: PathBuf::new() };
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{}', 'pay', 'main', 'dispatch', 'cpd/pay', '{}', cpd_txn(), 't', 't');",
        fx.repo.display(),
        fx.lead.display()
    ));
    fx.work = fx.attempt_needing(&Pr { key: "repo".into(), repo: fx.repo.clone(), lead: fx.lead.clone() }, "api", "running", kind, tests, needs);
    fx
}

/// A started PR of Plan `pay`: its key, repo and Lead worktree.
struct Pr {
    key: String,
    repo: PathBuf,
    lead: PathBuf,
}

fn plan() -> PlanId {
    PlanId::parse(PLAN).unwrap()
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    /// PR `key` of Plan `pay` in a new repo `<key>`, started: Lead Branch `cpd/pay/<key>` and
    /// its Lead worktree. `files` are committed on the Lead Branch.
    fn add_pr(&self, key: &str, files: &[(&str, &str)]) -> Pr {
        let repo = self.env.git_repo(key).canonicalize().unwrap();
        for (name, value) in [("user.name", "cpd test"), ("user.email", "test@cpd.invalid"), ("commit.gpgsign", "false")] {
            self.env.git(&repo, &["config", name, value]);
        }
        let branch = format!("cpd/pay/{key}");
        let lead = pr_lead_worktree_path(&repo, &plan(), &PrKey::parse(key).unwrap(), &branch).unwrap();
        let setup = Setup { timeout: Duration::from_secs(30), log: self.env.root().join("setup.log") };
        prepare_lead_worktree(&repo, &plan(), &lead, &branch, "main", &setup).map_err(|f| f.error).unwrap();
        for (path, content) in files {
            let full = lead.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, content).unwrap();
        }
        self.env.git(&lead, &["add", "-A"]);
        self.env.git(&lead, &["commit", "-q", "-m", "setup", "--allow-empty"]);
        self.env.repo_policy("acme", key, "");
        self.env.sql(&format!(
            "INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
               VALUES ('pay', '{key}', '{}', '{key}', 'main', 'dispatch', '{branch}', '{}', cpd_txn(), 't', 't');",
            repo.display(),
            lead.display()
        ));
        Pr { key: key.into(), repo, lead }
    }

    /// A Job of `pr` with its Attempt `<key>-a1` in `status`: its worktree cut from the Lead Branch HEAD. Returns the worktree.
    fn attempt_in(&self, pr: &Pr, key: &str, status: &str) -> PathBuf {
        self.attempt_kind(pr, key, status, "work", None)
    }

    /// [`Fx::attempt_in`] for a Job of `kind` (`tests`: a `test` Job's `(tests, test_files)`, JSON arrays).
    fn attempt_kind(&self, pr: &Pr, key: &str, status: &str, kind: &str, tests: Option<(&str, &str)>) -> PathBuf {
        self.attempt_needing(pr, key, status, kind, tests, None)
    }

    /// [`Fx::attempt_kind`] for a Job that needs the merged `test` Job `t` (its `tests` as a JSON array; `test_files` is `["tests/"]`): `t` and
    /// the Dependency are written in the Job's own transaction, since a Dependency of a Locked Job is refused later (`dependencies_locked`).
    fn attempt_needing(&self, pr: &Pr, key: &str, status: &str, kind: &str, tests: Option<(&str, &str)>, needs: Option<&str>) -> PathBuf {
        let attempt = AttemptId::parse(&format!("{key}-a1")).unwrap();
        let setup = Setup { timeout: Duration::from_secs(30), log: self.env.root().join("attempt.log") };
        let made = create_attempt_worktree(&pr.repo, &pr.lead, &plan(), &attempt, &setup).unwrap();
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, created_txn, created_at, kind, tests, test_files, review)
                     VALUES ('pay', ?4, ?1, 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, ?2, ?3, 't', ?5, ?6, ?7, ?8)",
                    // The pseudo-kind `review` is a `work` Job with the review flag.
                    params![key, "f".repeat(64), tx.id(), pr.key, if kind == "review" { "work" } else { kind }, tests.map(|t| t.0), tests.map(|t| t.1), kind == "review"],
                )?;
                if let Some(names) = needs {
                    tx.execute(
                        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                                           created_txn, created_at, kind, tests, test_files, merged_sha)
                           VALUES ('pay', ?1, 't', 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'merged', 0, printf('%064d', 1),
                                   ?2, 't', 'test', ?3, '[\"tests/\"]', 'c0ffee')",
                        params![pr.key, tx.id(), names],
                    )?;
                    tx.execute("INSERT INTO dependencies (plan, job, needs) VALUES ('pay', ?1, 't')", params![key])?;
                }
                tx.execute(
                    "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
                     VALUES ('pay', ?1, ?2, 1, 'S2', 'ladder', ?6, ?3, ?4, ?5, 't')",
                    params![attempt.to_string(), key, made.branch, made.path.to_str().unwrap(), made.base_sha, status],
                )?;
                Ok(())
            })
            .unwrap();
        made.path
    }

    /// Commits `path` = `content` in the Attempt worktree.
    fn commit(&self, path: &str, content: &str) -> String {
        let full = self.work.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, content).unwrap();
        self.env.git(&self.work, &["add", "-A"]);
        self.env.git(&self.work, &["commit", "-q", "-m", &format!("edit {path}")]);
        self.env.git(&self.work, &["rev-parse", "HEAD"])
    }

    fn submit(&self) -> Run {
        self.submit_in(&self.work)
    }

    fn submit_in(&self, cwd: &Path) -> Run {
        self.env.cpd_in(cwd, &["attempt", "submit", "api-a1", "--plan", PLAN])
    }

    fn cpd(&self, args: &[&str]) -> Run {
        let mut all = vec!["attempt"];
        all.extend(args);
        all.extend(["--plan", PLAN]);
        self.env.cpd(&all)
    }

    fn status(&self) -> String {
        self.env.query("SELECT status FROM attempts WHERE id = 'api-a1'")
    }

    fn events(&self, kind: &str) -> Vec<Value> {
        self.env.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
    }
}

// ---------- fixtures without git: the rows alone, for the commands that read the store ----------

/// The `base_sha` of the Attempts of [`Rows`]: no repo exists to cut one from.
const BASE: &str = "0123456789abcdef0123456789abcdef01234567";

/// [`fixture`] without a repo, a worktree or a git process: the same rows (Context, Lead Session, Plan, PR `repo`,
/// Job `api` and its Attempt `api-a1`, both `running`) written by SQL, in an [`InProcess`] env, whose commands run in
/// this process. The paths the rows name (`repo`, `lead`, the Attempts' worktrees) do not exist.
struct Rows {
    fx: InProcess,
    home: CpdHome,
    /// The worktree of `api-a1`.
    work: PathBuf,
}

fn rows() -> Rows {
    let fx = InProcess::new();
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    // The PR is of the repo `acme/pay` (its `name`): its policy file is required.
    fx.env.repo_policy("acme", "pay", "");
    let repos = fx.env.root().join("repos");
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'pay', 'main', 'dispatch', 'cpd/pay', '{lead}', cpd_txn(), 't', 't');",
        repo = repos.join("pay").display(),
        lead = repos.join("pay.pay").display()
    ));
    let mut rows = Rows { fx, home, work: PathBuf::new() };
    rows.work = rows.attempt("api", "running");
    rows
}

impl Rows {
    /// Job `key` (`running`) with Attempt `<key>-a1` in `status`, without a worktree. Returns the worktree's path.
    fn attempt(&self, key: &str, status: &str) -> PathBuf {
        let work = self.fx.env.root().join("repos").join(format!("pay.pay--{key}-a1"));
        self.fx.env.sql(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                               status, appended, fingerprint, created_txn, created_at, kind, review)
             VALUES ('pay', 'repo', '{key}', 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{f}', cpd_txn(), 't', 'work', 0);
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
             VALUES ('pay', '{key}-a1', '{key}', 1, 'S2', 'ladder', '{status}', 'cpd/pay--{key}-a1', '{work}', '{BASE}', 't');",
            f = "f".repeat(64),
            work = work.display()
        ));
        work
    }

    /// `cpd attempt <args> --plan pay`, in this process.
    fn cpd(&self, args: &[&str]) -> Run {
        let mut all = vec!["attempt"];
        all.extend(args);
        all.extend(["--plan", PLAN]);
        self.fx.cpd(&all)
    }

    /// `cpd <args>` as given, in this process.
    fn raw(&self, args: &[&str]) -> Run {
        self.fx.cpd(args)
    }

    /// Registers a Worker Session for `api-a1` whose file is `file` (`None`: not known yet).
    fn session(&self, file: Option<&Path>) {
        let file = file.map_or("NULL".to_owned(), |f| format!("'{}'", f.display()));
        self.fx.env.sql(&format!(
            "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, state, state_at, created_at)
               VALUES ('w1', 'worker', 'ctx', 'pay', 'api-a1', 'api-a1', {file}, '/', 'S2', 'working', 't', 't');
             UPDATE attempts SET worker_session = 'w1' WHERE id = 'api-a1';"
        ));
    }

    /// Writes a pi session file with `content`.
    fn session_file(&self, content: &str) -> PathBuf {
        let path = self.fx.env.root().join("session.jsonl");
        fs::write(&path, content).unwrap();
        path
    }
}

// ---------- fixtures over the fake git: worktrees without a git process ----------

/// [`Fx`] in this process: the repo, its Lead worktree and the Attempts' worktrees are repos of the fake git (`Universe`),
/// their files real files in the env's temp dir; the same rows. No git, hook or other process runs.
struct Git {
    fx: InProcess,
    home: CpdHome,
    repo: PathBuf,
    lead: PathBuf,
    /// The worktree of `api-a1`.
    work: PathBuf,
}

/// `files` are committed on `main` before the Lead worktree is cut from it.
fn git_fixture(files: &[(&str, &str)]) -> Git {
    git_fixture_kind(files, "work")
}

/// [`git_fixture`] with Job `api` of `kind` (`work` or `review`).
fn git_fixture_kind(files: &[(&str, &str)], kind: &str) -> Git {
    let fx = InProcess::with_ports();
    let repo = fx.git_repo("pay").canonicalize().unwrap();
    for (path, content) in files {
        fx.commit_file(&repo, path, content, "setup");
    }
    let lead = in_fakes(&fx, || create_lead_worktree(&repo, &plan(), "main", &setup(&fx, "setup.log"))).unwrap().path;
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    // The PR is of the repo `acme/pay` (its `name`): its policy file is required, empty unless a test sets `[protected]`.
    fx.env.repo_policy("acme", "pay", "");
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{}', 'pay', 'main', 'dispatch', 'cpd/pay', '{}', cpd_txn(), 't', 't');",
        repo.display(),
        lead.display()
    ));
    let mut git = Git { fx, home, repo, lead, work: PathBuf::new() };
    git.work = git.attempt_in(&Pr { key: "repo".into(), repo: git.repo.clone(), lead: git.lead.clone() }, "api", "running", kind);
    git
}

/// `f` with the fake ports of `fx` as the ambient ones: the core's git functions (`create_lead_worktree`, ...) run on them.
fn in_fakes<R>(fx: &InProcess, f: impl FnOnce() -> R) -> R {
    Ports::scope(&fx.fakes.ports, f)
}

fn setup(fx: &InProcess, log: &str) -> Setup {
    Setup { timeout: Duration::from_secs(30), log: fx.env.root().join(log) }
}

impl Git {
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    /// PR `key` of Plan `pay` in a new repo `<key>`, started: Lead Branch `cpd/pay/<key>` and its Lead worktree.
    fn add_pr(&self, key: &str) -> Pr {
        let repo = self.fx.git_repo(key).canonicalize().unwrap();
        let branch = format!("cpd/pay/{key}");
        let lead = pr_lead_worktree_path(&repo, &plan(), &PrKey::parse(key).unwrap(), &branch).unwrap();
        in_fakes(&self.fx, || prepare_lead_worktree(&repo, &plan(), &lead, &branch, "main", &setup(&self.fx, "setup.log"))).map_err(|f| f.error).unwrap();
        self.fx.env.repo_policy("acme", key, "");
        self.fx.env.sql(&format!(
            "INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
               VALUES ('pay', '{key}', '{}', '{key}', 'main', 'dispatch', '{branch}', '{}', cpd_txn(), 't', 't');",
            repo.display(),
            lead.display()
        ));
        Pr { key: key.into(), repo, lead }
    }

    /// A Job of `pr` and of `kind` (`work` or `review`) with its Attempt `<key>-a1` in `status`: its worktree cut from the
    /// Lead Branch HEAD. Returns the worktree.
    fn attempt_in(&self, pr: &Pr, key: &str, status: &str, kind: &str) -> PathBuf {
        let attempt = AttemptId::parse(&format!("{key}-a1")).unwrap();
        let made = in_fakes(&self.fx, || create_attempt_worktree(&pr.repo, &pr.lead, &plan(), &attempt, &setup(&self.fx, "attempt.log"))).unwrap();
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, created_txn, created_at, kind, review)
                     VALUES ('pay', ?4, ?1, 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, ?2, ?3, 't', 'work', ?5)",
                    // The pseudo-kind `review` is a `work` Job with the review flag.
                    params![key, "f".repeat(64), tx.id(), pr.key, kind == "review"],
                )?;
                tx.execute(
                    "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
                     VALUES ('pay', ?1, ?2, 1, 'S2', 'ladder', ?6, ?3, ?4, ?5, 't')",
                    params![attempt.to_string(), key, made.branch, made.path.to_str().unwrap(), made.base_sha, status],
                )?;
                Ok(())
            })
            .unwrap();
        made.path
    }

    /// [`Git::attempt_in`] for a `work` Job of the first PR.
    fn attempt_as(&self, key: &str, status: &str) -> PathBuf {
        self.attempt_in(&Pr { key: "repo".into(), repo: self.repo.clone(), lead: self.lead.clone() }, key, status, "work")
    }

    /// Commits `path` = `content` in `dir`, a worktree.
    fn commit_in(&self, dir: &Path, path: &str, content: &str) -> String {
        self.fx.commit_file(dir, path, content, &format!("edit {path}"));
        self.fx.git(dir, &["rev-parse", "HEAD"])
    }

    /// Commits `path` = `content` in the Attempt worktree.
    fn commit(&self, path: &str, content: &str) -> String {
        self.commit_in(&self.work, path, content)
    }

    fn submit(&self) -> Run {
        self.submit_in(&self.work)
    }

    fn submit_in(&self, cwd: &Path) -> Run {
        self.fx.cpd_in(cwd, &["attempt", "submit", "api-a1", "--plan", PLAN])
    }

    fn status(&self) -> String {
        self.fx.env.query("SELECT status FROM attempts WHERE id = 'api-a1'")
    }

    fn txns(&self) -> i64 {
        self.fx.env.query("SELECT count(*) FROM txns")
    }

    fn events(&self, kind: &str) -> Vec<Value> {
        self.fx.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
    }

    /// The `attempt-status` Events' `data`, oldest first.
    fn statuses(&self) -> Vec<Value> {
        self.events("attempt-status").iter().map(|e| e["data"].clone()).collect()
    }

    /// The `txn` of each `attempt-status` Event, oldest first.
    fn status_txns(&self) -> Vec<i64> {
        let text: String = self.fx.env.query("SELECT group_concat(txn) FROM (SELECT txn FROM events WHERE kind = 'attempt-status' ORDER BY rowid)");
        text.split(',').map(|t| t.parse().unwrap()).collect()
    }
}

const NEXT: &str = "Stop now. Do not make more changes.";

// ---------- attempt submit ----------

#[test]
fn submit_moves_the_attempt_to_merging_and_tells_the_worker_to_stop() {
    let fx = git_fixture(&[]);
    let sha = fx.commit("src/a.rs", "fn a() {}\n");
    let out = fx.submit().ok();
    assert_eq!(out, json!({"ok": true, "attempt": "api-a1", "submitted": true, "sha": sha, "next": NEXT}));

    assert_eq!(fx.status(), "merging");
    let (submitted_sha, submitted_at, tail): (String, String, Option<String>) = fx
        .store()
        .read(|c| {
            Ok(c.query_row("SELECT submitted_sha, submitted_at, check_tail FROM attempts WHERE id = 'api-a1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?)
        })
        .unwrap();
    assert_eq!(submitted_sha, sha);
    assert!(submitted_at.ends_with('Z') && submitted_at.contains('T'), "{submitted_at}");
    assert_eq!(tail, None, "no pre-commit table: nothing ran, check_tail stays null");
    assert_eq!(fx.fx.env.query::<String>("SELECT status FROM jobs WHERE key = 'api'"), "running", "the Job stays running");
    assert!(!fx.home.hook_log(&plan(), &AttemptId::parse("api-a1").unwrap(), cpd_core::home::Hook::PreCommit).exists());

    let events = fx.events("attempt-status");
    assert_eq!(events.len(), 3, "{events:?}");
    assert_eq!(events[0]["data"], json!({"from": "running", "to": "running", "check": "start", "what": "checks"}));
    assert_eq!(events[1]["data"], json!({"from": "running", "to": "running", "check": "end", "what": "checks"}));
    assert_eq!(events[2]["data"], json!({"from": "running", "to": "merging"}));
    assert_eq!((events[2]["plan"].as_str(), events[2]["job"].as_str(), events[2]["attempt"].as_str()), (Some("pay"), Some("api"), Some("api-a1")));
    assert_eq!(fx.fx.env.query::<String>("SELECT command FROM txns ORDER BY id DESC LIMIT 1"), "attempt submit");
}

#[test]
fn submit_works_from_a_subdirectory_and_takes_the_plan_from_the_environment() {
    let fx = git_fixture(&[]);
    let sha = fx.commit("src/a.rs", "x\n");
    let run = fx.fx.cpd_with(&fx.work.join("src"), &["attempt", "submit", "api-a1"], &[("CPD_PLAN", PLAN)]);
    assert_eq!(run.exit, 0, "{}", run.stdout);
    assert_eq!(fx.fx.env.query::<String>("SELECT submitted_sha FROM attempts WHERE id = 'api-a1'"), sha);
}

#[test]
fn submit_twice_is_already_and_writes_nothing() {
    let fx = git_fixture(&[]);
    let sha = fx.commit("a.txt", "a\n");
    fx.submit().ok();
    let txns = fx.txns();
    let out = fx.submit().ok();
    assert_eq!(out, json!({"ok": true, "attempt": "api-a1", "submitted": true, "sha": sha, "next": NEXT, "already": true}));
    assert_eq!(fx.txns(), txns);
    // Also from outside the worktree: the Attempt's state is checked first.
    assert_eq!(fx.submit_in(fx.fx.env.root()).ok()["already"], true);
    // And once the Attempt has ended with that submission.
    fx.fx.env.sql("UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = 't' WHERE id = 'api-a1'");
    assert_eq!(fx.submit().ok()["already"], true);
}

#[test]
fn submit_refuses_an_attempt_that_is_not_running() {
    // One `Fx` lives at a time in the process (it owns the environment): each case is its own scope.
    {
        let fx = git_fixture(&[]);
        fx.commit("a.txt", "a\n");
        fx.fx.env.sql("UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'timed_out', ended_at = 't' WHERE id = 'api-a1'");
        let error = refused(&fx.submit(), "attempt_not_running", 3);
        assert_eq!(error["details"]["status"], "ended");
        assert_eq!(error["details"]["cause"], "timed_out");
    }

    // Merging, but HEAD is not what was submitted: the HEAD moved after the submit.
    {
        let fx = git_fixture(&[]);
        fx.commit("a.txt", "a\n");
        fx.submit().ok();
        fx.commit("b.txt", "b\n");
        refused(&fx.submit(), "attempt_not_running", 3);
    }

    // Starting: the Worker has not been started yet.
    let fx = git_fixture(&[]);
    let web = fx.attempt_as("web", "starting");
    fx.commit_in(&web, "a.txt", "a\n");
    let run = fx.fx.cpd_in(&web, &["attempt", "submit", "web-a1", "--plan", PLAN]);
    assert_eq!(refused(&run, "attempt_not_running", 3)["details"]["status"], "starting");
}

#[test]
fn submit_refuses_a_caller_outside_the_worktree_or_off_the_branch() {
    let fx = git_fixture(&[]);
    fx.commit("a.txt", "a\n");
    let error = refused(&fx.submit_in(fx.fx.env.root()), "wrong_worktree", 3);
    assert_eq!(error["details"]["branch"], "cpd/pay--api-a1");
    refused(&fx.submit_in(&fx.lead), "wrong_worktree", 3);
    refused(&fx.submit_in(&fx.repo), "wrong_worktree", 3);

    fx.fx.git(&fx.work, &["checkout", "-q", "-b", "other"]);
    refused(&fx.submit(), "wrong_worktree", 3);
    assert_eq!(fx.status(), "running");
}

#[test]
fn submit_refuses_a_dirty_tree_untracked_files_included() {
    let fx = git_fixture(&[]);
    fx.commit("a.txt", "a\n");
    fs::write(fx.work.join("scratch.txt"), "x").unwrap();
    let error = refused(&fx.submit(), "dirty_tree", 3);
    assert_eq!(error["details"]["files"], json!(["?? scratch.txt"]));

    fs::remove_file(fx.work.join("scratch.txt")).unwrap();
    fs::write(fx.work.join("a.txt"), "changed\n").unwrap();
    refused(&fx.submit(), "dirty_tree", 3);
    assert_eq!(fx.status(), "running");
}

#[test]
fn submit_refuses_an_attempt_with_no_commit_on_top_of_the_base() {
    let fx = git_fixture(&[]);
    let error = refused(&fx.submit(), "no_commits", 3);
    assert_eq!(error["details"]["base_sha"], fx.fx.git(&fx.work, &["rev-parse", "HEAD"]));
    assert_eq!(fx.status(), "running");
}

#[test]
fn submit_refuses_a_diff_that_touches_protected_paths() {
    let fx = git_fixture(&[("verify.sh", "#!/bin/sh\n")]);
    fx.fx.env.repo_policy("acme", "pay", "[protected]\npaths = [\"verify.sh\", \"scripts/ci/\"]\n");
    fx.commit("src/fine.rs", "ok\n");
    fx.commit("verify.sh", "#!/bin/sh\nexit 0\n");
    fx.commit("scripts/ci/run.sh", "x\n");
    fx.commit(".cpd/note", "x\n");
    let error = refused(&fx.submit(), "gate_tampered", 3);
    let mut paths: Vec<String> = serde_json::from_value(error["details"]["paths"].clone()).unwrap();
    paths.sort();
    assert_eq!(paths, [".cpd/note", "scripts/ci/run.sh", "verify.sh"]);
    assert_eq!(fx.status(), "running");
}

#[test]
fn submit_takes_the_protected_list_from_the_policy_of_the_jobs_pr() {
    let fx = git_fixture(&[]);
    // Only the web PR's repo policy protects verify.sh.
    let web = fx.add_pr("web");
    fx.fx.env.repo_policy("acme", "web", "[protected]\npaths = [\"verify.sh\"]\n");
    let ui = fx.attempt_in(&web, "ui", "running", "work");
    fx.commit_in(&ui, "verify.sh", "exit 0\n");
    fx.commit_in(&fx.work, "verify.sh", "exit 0\n");

    let error = refused(&fx.fx.cpd_in(&ui, &["attempt", "submit", "ui-a1", "--plan", PLAN]), "gate_tampered", 3);
    assert_eq!(error["details"]["paths"], json!(["verify.sh"]));
    assert_eq!(fx.fx.env.query::<String>("SELECT status FROM attempts WHERE id = 'ui-a1'"), "running");
    // The first PR's repo policy has no such list: the same file is fine there.
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");
}

#[test]
fn submit_runs_the_quick_check_of_the_jobs_pr_with_its_lead_branch_as_target() {
    let fx = fixture(&[]);
    let web = fx.add_pr("web", &[]);
    fx.env.repo_policy("acme", "web", "[hooks.pre-commit]\ncheck = \"echo {{ target }} > ../target-seen.txt; echo checking-web\"\n");
    let ui = fx.attempt_in(&web, "ui", "running");
    fs::write(ui.join("ui.txt"), "ui\n").unwrap();
    fx.env.git(&ui, &["add", "-A"]);
    fx.env.git(&ui, &["commit", "-q", "-m", "ui"]);

    fx.env.cpd_in(&ui, &["attempt", "submit", "ui-a1", "--plan", PLAN]).ok();

    assert_eq!(fs::read_to_string(fx.env.root().join("repos/target-seen.txt")).unwrap(), "cpd/pay/web\n");
    let tail: String = fx.env.query("SELECT check_tail FROM attempts WHERE id = 'ui-a1'");
    assert!(tail.contains("checking-web"), "{tail}");
    // The first PR's Attempt has no hook of its own to run.
    fx.commit("a.txt", "a\n");
    fx.submit().ok();
    assert_eq!(fx.env.query::<Option<String>>("SELECT check_tail FROM attempts WHERE id = 'api-a1'"), None);
}

#[test]
fn submit_runs_the_quick_check_and_keeps_its_tail() {
    let fx = hooked("[hooks.pre-commit]\ncheck = \"echo checking; pwd; cat >/dev/null\"\n");
    fx.commit("a.txt", "a\n");
    fx.submit().ok();
    let tail: String = fx.env.query("SELECT check_tail FROM attempts WHERE id = 'api-a1'");
    assert!(tail.contains("checking"), "{tail}");
    assert!(tail.contains(fx.work.to_str().unwrap()), "the hook ran in the worktree: {tail}");
    let log = fx.home.hook_log(&plan(), &AttemptId::parse("api-a1").unwrap(), cpd_core::home::Hook::PreCommit);
    assert!(fs::read_to_string(log).unwrap().contains("checking"));
}

#[test]
fn submit_refuses_a_failed_quick_check_with_its_tail_and_log() {
    let fx = hooked("[hooks.pre-commit]\ncheck = \"echo broken >&2; exit 7\"\n");
    fx.commit("a.txt", "a\n");
    let error = refused(&fx.submit(), "check_failed", 3);
    assert!(error["details"]["tail"].as_str().unwrap().contains("broken"), "{error}");
    let log = error["details"]["log"].as_str().unwrap();
    assert!(log.ends_with("logs/api-a1.pre-commit.log"), "{log}");
    assert!(fs::read_to_string(log).unwrap().contains("broken"));
    assert_eq!(fx.status(), "running");
    let events = fx.events("attempt-status");
    let spans: Vec<_> = events.iter().map(|e| (e["data"]["check"].as_str(), e["data"]["what"].as_str())).collect();
    assert_eq!(spans, [(Some("start"), Some("checks")), (Some("end"), Some("checks"))], "{events:?}");
}

#[test]
fn submit_checks_its_arguments_and_selection() {
    let fx = git_fixture(&[]);
    fx.commit("a.txt", "a\n");
    refused(&fx.fx.cpd_in(&fx.work, &["attempt", "submit", "api-a1"]), "plan_required", 2);
    refused(&fx.fx.cpd_in(&fx.work, &["attempt", "submit", "Bad_Id", "--plan", PLAN]), "invalid_attempt_id", 2);
    refused(&fx.fx.cpd_in(&fx.work, &["attempt", "submit", "api-a9", "--plan", PLAN]), "attempt_not_found", 4);
    refused(&fx.fx.cpd_in(&fx.work, &["attempt", "submit", "api-a1", "--plan", "nope"]), "plan_not_found", 4);
    refused(&fx.fx.cpd_in(&fx.work, &["attempt", "submit"]), "usage", 2);
    assert_eq!(fx.status(), "running");
}

// ---------- attempt submit: the checks of the `test` and `impl` kinds (contract §7.7, commands.md §6.1) ----------

/// A repo's `[test-commands]`, as fake commands (they run in the Attempt worktree, `sh -c`): `build` fails while `BROKEN` exists,
/// `run` fails unless `GREEN` exists, so a test Job that adds neither is red and one that adds `GREEN` is not. Each leaves a
/// trace beside the worktree: `ran.txt` the named tests it was given, `slot.txt` the slot it was told it holds.
const TESTS: &str = "[test-commands]\n\
    build = \"echo building; test ! -e BROKEN\"\n\
    run = \"echo running; echo {{ tests }} > ../ran.txt; echo slot=[$CPD_TEST_SLOT] > ../slot.txt; test -e GREEN\"\n";

/// `TESTS` with a `list` that exists for the one name `t::one` only; it traces the names it was asked about.
const TESTS_LIST: &str = "[test-commands]\n\
    build = \"true\"\n\
    list = \"echo {{ test }} >> ../listed.txt; test {{ test }} = t::one || test {{ test }} = t::two\"\n\
    run = \"echo {{ tests }} > ../ran.txt; false\"\n";

/// Job `api` is a `test` Job of the named test `t::one`, held in `tests/t.rs`.
fn red(toml: &str) -> Fx {
    let fx = fixture_kind(&[], "test", Some((r#"["t::one"]"#, r#"["tests/t.rs"]"#)));
    fx.env.repo_policy("acme", "pay", toml);
    fx
}

/// Job `api` is an `impl` Job that needs the merged `test` Job `t` (the tests `t::one` and `t::two`, in `tests/`).
fn green(toml: &str, approved: Option<&str>) -> Fx {
    green_naming(toml, approved, r#"["t::one","t::two"]"#)
}

/// [`green`] with the tests `t` names.
fn green_naming(toml: &str, approved: Option<&str>, names: &str) -> Fx {
    let fx = fixture_needing(&[], "impl", None, Some(names));
    fx.env.repo_policy("acme", "pay", toml);
    if let Some(paths) = approved {
        fx.env.sql(&format!("UPDATE jobs SET test_approved = '{paths}' WHERE key = 'api'"));
    }
    fx
}

impl Fx {
    /// A file the fake `[test-commands]` commands left beside the Attempt worktree (`None`: it was never written).
    fn seen(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.work.parent().unwrap().join(name)).ok()
    }

    /// The log `<attempt>.tests-<what>.log` of `api-a1`.
    fn tests_log(&self, what: &str) -> PathBuf {
        self.home.root().join(format!("run/pay/logs/api-a1.tests-{what}.log"))
    }

    /// The `attempt-status` Events' `data`, oldest first.
    fn statuses(&self) -> Vec<Value> {
        self.events("attempt-status").iter().map(|e| e["data"].clone()).collect()
    }
}

fn span(edge: &str) -> Value {
    json!({"from": "running", "to": "running", "check": edge, "what": "checks"})
}

#[test]
fn a_test_job_whose_build_fails_is_refused_tests_build_failed_and_run_never_runs() {
    let fx = red(TESTS);
    fx.commit("tests/t.rs", "// red\n");
    fx.commit("BROKEN", "x\n");
    let error = refused(&fx.submit(), "tests_build_failed", 3);
    let details = &error["details"];
    assert_eq!((&details["job"], &details["tests"]), (&json!("api"), &json!(["t::one"])));
    assert!(details["tail"].as_str().unwrap().contains("building"), "{error}");
    assert_eq!(details["log"], fx.tests_log("build").to_str().unwrap());
    assert!(fs::read_to_string(fx.tests_log("build")).unwrap().contains("building"));
    assert_eq!(fx.seen("ran.txt"), None, "`run` runs only after `build` succeeded");
    assert_eq!(fx.status(), "running");
}

#[test]
fn a_test_job_whose_named_tests_fail_passes_and_leaves_both_logs() {
    let fx = red(TESTS);
    let sha = fx.commit("tests/t.rs", "// fails\n");
    let out = fx.submit().ok();
    assert_eq!(out, json!({"ok": true, "attempt": "api-a1", "submitted": true, "sha": sha, "next": NEXT}));
    assert_eq!(fx.status(), "merging");
    assert!(fs::read_to_string(fx.tests_log("build")).unwrap().contains("building"));
    assert!(fs::read_to_string(fx.tests_log("run")).unwrap().contains("running"));
    assert_eq!(fx.seen("ran.txt").as_deref(), Some("t::one\n"));
}

#[test]
fn the_tests_commands_are_not_given_a_test_slot() {
    // hs-d464: a run of named tests is not a full run; the Gate is what takes a slot (contract §7.7).
    let fx = red(TESTS);
    fx.commit("tests/t.rs", "// fails\n");
    fx.submit().ok();
    assert_eq!(fx.seen("slot.txt").as_deref(), Some("slot=[]\n"));
}

#[test]
fn list_refuses_a_name_that_matches_no_test_before_run_for_a_test_job() {
    let fx = fixture_kind(&[], "test", Some((r#"["t::one","t::typo"]"#, r#"["tests/t.rs"]"#)));
    fx.env.repo_policy("acme", "pay", TESTS_LIST);
    fx.commit("tests/t.rs", "// red\n");
    let error = refused(&fx.submit(), "tests_not_found", 3);
    let details = &error["details"];
    assert_eq!((&details["job"], &details["tests"]), (&json!("api"), &json!(["t::typo"])), "only the name that matches nothing");
    assert_eq!(details["log"], fx.tests_log("list").to_str().unwrap());
    assert_eq!(fx.seen("listed.txt").as_deref(), Some("t::one\nt::typo\n"), "`list` runs once per named test, with the variable `test`");
    assert_eq!(fx.seen("ran.txt"), None, "before `run`");
    assert_eq!(fx.status(), "running");
}

#[test]
fn an_impl_job_is_run_against_the_union_of_the_tests_of_the_test_jobs_it_needs() {
    let fx = green(TESTS, None);
    let sha = fx.commit("src/lib.rs", "// green\n");
    fx.commit("GREEN", "x\n");
    let out = fx.submit().ok();
    assert_eq!(out["submitted"], true, "{out}");
    assert_ne!(out["sha"], sha);
    assert_eq!(fx.status(), "merging");
    assert_eq!(fx.seen("ran.txt").as_deref(), Some("t::one t::two\n"), "derived: the Job names no tests of its own");
    assert!(fs::read_to_string(fx.tests_log("run")).unwrap().contains("running"));
}

#[test]
fn an_impl_job_whose_tests_still_fail_is_refused_tests_failed() {
    let fx = green(TESTS, None);
    fx.commit("src/lib.rs", "// not enough\n");
    let error = refused(&fx.submit(), "tests_failed", 3);
    assert_eq!((&error["details"]["job"], &error["details"]["tests"]), (&json!("api"), &json!(["t::one", "t::two"])));
    assert_eq!(error["details"]["log"], fx.tests_log("run").to_str().unwrap());
    assert!(error["details"]["tail"].as_str().unwrap().contains("running"), "{error}");
    assert_eq!(fx.status(), "running");
}

#[test]
fn an_impl_job_whose_build_fails_is_refused_tests_build_failed() {
    let fx = green(TESTS, None);
    fx.commit("src/lib.rs", "// green\n");
    fx.commit("BROKEN", "x\n");
    let error = refused(&fx.submit(), "tests_build_failed", 3);
    assert_eq!(error["details"]["log"], fx.tests_log("build").to_str().unwrap());
    assert_eq!(fx.seen("ran.txt"), None);
}

#[test]
fn a_work_jobs_checks_are_one_span_that_ends_in_the_transaction_that_makes_the_attempt_merging() {
    let fx = git_fixture(&[]);
    fx.commit("a.txt", "x\n");
    fx.submit().ok();
    assert_eq!(fx.statuses(), [span("start"), span("end"), json!({"from": "running", "to": "merging"})]);
    let txns = fx.status_txns();
    assert_eq!(txns[1], txns[2], "the end and running -> merging share a transaction");
    assert_ne!(txns[0], txns[1]);
}

#[test]
fn a_refused_quick_check_ends_the_checks_span_and_leaves_the_attempt_running() {
    let fx = hooked("[hooks.pre-commit]\ncheck = \"exit 7\"\n");
    fx.commit("a.txt", "x\n");
    refused(&fx.submit(), "check_failed", 3);
    assert_eq!(fx.statuses(), [span("start"), span("end")]);
    assert_eq!(fx.status(), "running");
}

/// Guards the rule that a review Job has no checks and no span (green today).
#[test]
fn a_review_jobs_submit_records_no_check_span() {
    let fx = git_fixture_kind(&[], "review");
    let findings = fx.home.root().join("run/pay/reviews/api.md");
    fs::create_dir_all(findings.parent().unwrap()).unwrap();
    fs::write(&findings, "- one finding\n").unwrap();
    fx.submit().ok();
    assert_eq!(fx.statuses(), [json!({"from": "running", "to": "merging"})]);
}

// ---------- attempt read ----------

const SESSION: &str = concat!(
    r#"{"type":"session","id":"w1"}"#, "\n",
    r#"{"type":"message","message":{"role":"user","content":[{"type":"text","text":"Read the spec."}]}}"#, "\n",
    r#"{"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","name":"read","arguments":{}}]}}"#, "\n",
    r#"{"type":"message","message":{"role":"toolResult","content":[{"type":"text","text":"spec"}]}}"#, "\n",
    r#"{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"The schema compiles;\ncommitting."},{"type":"toolCall","name":"bash","arguments":{}}]}}"#, "\n",
);

#[test]
fn read_prints_assistant_text_and_tool_calls_in_session_order() {
    let fx = rows();
    let file = fx.session_file(SESSION);
    fx.session(Some(&file));
    let out = fx.cpd(&["read", "api-a1"]).ok();
    assert_eq!(
        out,
        json!({"ok": true, "attempt": "api-a1", "session": "w1", "file": file.to_str().unwrap(), "total": 4,
               "lines": ["tool: read", "assistant: The schema compiles;", "assistant: committing.", "tool: bash"]})
    );
    let out = fx.cpd(&["read", "api-a1", "--lines", "2"]).ok();
    assert_eq!(out["lines"], json!(["assistant: committing.", "tool: bash"]));
    assert_eq!(out["total"], 4, "total counts every line, not the kept ones");
    let out = fx.cpd(&["read", "api-a1", "--lines", "100"]).ok();
    assert_eq!(out["lines"].as_array().unwrap().len(), 4);
}

#[test]
fn read_skips_a_partial_last_line_but_not_a_malformed_complete_one() {
    let fx = rows();
    let file = fx.session_file(&format!("{SESSION}{{\"type\":\"message\",\"mess"));
    fx.session(Some(&file));
    assert_eq!(fx.cpd(&["read", "api-a1"]).ok()["total"], 4);

    fs::write(&file, format!("{SESSION}not json\n")).unwrap();
    let error = refused(&fx.cpd(&["read", "api-a1"]), "session_malformed", 1);
    assert_eq!(error["details"]["line"], 6);
    fs::write(&file, format!("{SESSION}{{\"no\":\"type\"}}\n")).unwrap();
    refused(&fx.cpd(&["read", "api-a1"]), "session_malformed", 1);
}

#[test]
fn read_needs_a_registered_session_with_an_existing_file() {
    let fx = rows();
    refused(&fx.cpd(&["read", "api-a1"]), "session_not_found", 4);

    fx.session(None);
    refused(&fx.cpd(&["read", "api-a1"]), "session_not_found", 4);

    let gone = fx.fx.env.root().join("gone.jsonl");
    fx.fx.env.sql(&format!("UPDATE sessions SET file = '{}' WHERE id = 'w1'", gone.display()));
    let error = refused(&fx.cpd(&["read", "api-a1"]), "session_not_found", 4);
    assert_eq!(error["details"]["session"], "w1");

    refused(&fx.cpd(&["read", "api-a9"]), "attempt_not_found", 4);
    refused(&fx.cpd(&["read", "api-a1", "--lines", "0"]), "usage", 2);
    refused(&fx.cpd(&["read", "api-a1", "--lines", "x"]), "usage", 2);
}

// ---------- attempt show ----------

#[test]
fn show_returns_every_field_its_escalations_and_its_paths() {
    let fx = rows();
    let file = fx.session_file(SESSION);
    fx.session(Some(&file));
    fx.fx.env.sql(
        "UPDATE attempts SET spec_path = '/specs/api-a1.md', driver_pid = 4242, nudged = 1, metrics = '{\"turns\":3}' WHERE id = 'api-a1';
         INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
           VALUES ('pay', 'api-a1-q1', 'api-a1', 1, 'Which?', 'open', 99, 't');",
    );
    let out = fx.cpd(&["show", "api-a1"]).ok();
    let attempt = &out["attempt"];
    let base = BASE;
    assert_eq!(
        *attempt,
        json!({
            "plan": "pay", "id": "api-a1", "job": "api", "number": 1, "tier": "S2", "origin": "ladder", "status": "running",
            "outcome": null, "cause": null, "branch": "cpd/pay--api-a1", "worktree": fx.work.to_str().unwrap(),
            "base_sha": base, "driver_pid": 4242, "worker_session": "w1", "spec_path": "/specs/api-a1.md",
            "submitted_sha": null, "submitted_at": null, "check_tail": null, "merged_sha": null, "nudged": true,
            "stop_request": null, "metrics": {"turns": 3}, "started_at": "t", "ended_at": null,
        })
    );
    assert_eq!(
        out["escalations"],
        json!([{"id": "api-a1-q1", "number": 1, "question": "Which?", "status": "open", "answer": null,
                "asker_pid": 99, "asked_at": "t", "answered_at": null, "expired_at": null}])
    );
    let logs = fx.home.root().join("run/pay/logs");
    assert_eq!(
        out["paths"],
        json!({"spec": "/specs/api-a1.md", "check": null, "log": logs.join("attempt-api-a1.log").to_str().unwrap(),
               "worker_file": file.to_str().unwrap()})
    );
    assert!(out["events_cursor"].is_i64(), "{out}");
}

#[test]
fn show_names_the_check_log_once_a_quick_check_has_run() {
    let fx = hooked("[hooks.pre-commit]\ncheck = \"echo hi\"\n");
    fx.commit("a.txt", "a\n");
    fx.submit().ok();
    let out = fx.cpd(&["show", "api-a1"]).ok();
    assert_eq!(out["attempt"]["status"], "merging");
    assert_eq!(out["attempt"]["check_tail"], "hi");
    let check = out["paths"]["check"].as_str().unwrap();
    assert!(check.ends_with("run/pay/logs/api-a1.pre-commit.log"), "{check}");
    assert_eq!(out["paths"]["worker_file"], Value::Null);
    assert_eq!(out["paths"]["spec"], Value::Null);
}

#[test]
fn show_refuses_what_does_not_exist() {
    let fx = rows();
    refused(&fx.cpd(&["show", "api-a2"]), "attempt_not_found", 4);
    refused(&fx.raw(&["attempt", "show", "api-a1", "--plan", "nope"]), "plan_not_found", 4);
    refused(&fx.raw(&["attempt", "show", "api-a1"]), "plan_required", 2);
    refused(&fx.cpd(&["show", "nonsense"]), "invalid_attempt_id", 2);
}

// ---------- attempt list ----------

/// Attempts `api-a1` (ended), `api-a2` (running), `web-a1` (running).
fn listed() -> Rows {
    let fx = rows();
    fx.fx.env.sql(
        "UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = 'u' WHERE id = 'api-a1';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
           VALUES ('pay', 'api-a2', 'api', 2, 'S3', 'retry', 'running', 'cpd/pay--api-a2', '/w2', 't2');",
    );
    fx.attempt("web", "running");
    fx
}

fn ids(out: &Value) -> Vec<&str> {
    out["attempts"].as_array().unwrap().iter().map(|a| a["id"].as_str().unwrap()).collect()
}

#[test]
fn list_returns_the_summary_of_every_attempt_oldest_first() {
    let fx = listed();
    let out = fx.cpd(&["list"]).ok();
    assert_eq!(ids(&out), ["api-a1", "api-a2", "web-a1"]);
    assert_eq!(
        out["attempts"][0],
        json!({"id": "api-a1", "job": "api", "number": 1, "tier": "S2", "origin": "ladder", "status": "ended",
               "outcome": "failed", "cause": "crashed", "started_at": "t", "ended_at": "u"})
    );
    assert_eq!(
        out["attempts"][1],
        json!({"id": "api-a2", "job": "api", "number": 2, "tier": "S3", "origin": "retry", "status": "running",
               "outcome": null, "cause": null, "started_at": "t2", "ended_at": null})
    );
}

#[test]
fn list_filters_by_job_and_by_active() {
    let fx = listed();
    assert_eq!(ids(&fx.cpd(&["list", "--job", "api"]).ok()), ["api-a1", "api-a2"]);
    assert_eq!(ids(&fx.cpd(&["list", "--active"]).ok()), ["api-a2", "web-a1"]);
    assert_eq!(ids(&fx.cpd(&["list", "--active", "--job", "api"]).ok()), ["api-a2"]);
    assert_eq!(ids(&fx.cpd(&["list", "--job", "web"]).ok()), ["web-a1"]);
}

#[test]
fn list_refuses_what_does_not_exist() {
    let fx = listed();
    refused(&fx.cpd(&["list", "--job", "ghost"]), "job_not_found", 4);
    refused(&fx.cpd(&["list", "--job", "Bad_Key"]), "invalid_job_id", 2);
    refused(&fx.raw(&["attempt", "list", "--plan", "nope"]), "plan_not_found", 4);
    refused(&fx.raw(&["attempt", "list"]), "plan_required", 2);
    fx.fx.env.sql(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
         VALUES ('pay', 'repo', 'idle', 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, 1, 't')",
    );
    assert_eq!(ids(&fx.cpd(&["list", "--job", "idle"]).ok()), Vec::<&str>::new());
}
