//! `attempt submit`, `attempt read`, `attempt show` and `attempt list` (commands.md §6.1–§6.3). Every test runs in this
//! process, on the fakes of `Fx::with_ports`. `attempt submit` runs on the fake git ([`Git`]: repos of the `Universe`, no git
//! process), and the hooks and `[test-commands]` of a repo, which `cpd` runs with `sh -c`, are answered by a route of the
//! `FakeRunner` ([`Git::sh`]): the tests pin the command `cpd` renders and what it makes of the answer, not what a shell does.
//! `attempt read`, `attempt show` and `attempt list` call no git and no hook, so their tests run against rows alone
//! ([`Rows`]). The Plan, its Jobs, Attempts and Sessions are fixtures written straight into the database; the commands under
//! test are the only ones that write the rest.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::fx::Fx as InProcess;
use common::{Run, refused};
use cpd_core::config::{Catalog, Config, TierModel};
use cpd_core::git::{
    Setup, create_attempt_worktree, create_lead_worktree, pr_lead_worktree_path,
    prepare_lead_worktree,
};
use cpd_core::git::{
    Setup, create_attempt_worktree, create_lead_worktree, prepare_lead_worktree,
    workflow_lead_worktree_path,
};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::ids::{AttemptId, PlanId, WorkflowKey};
use cpd_core::merge::{self, End, Failure};
use cpd_core::ports::Ports;
use cpd_core::store::Store;
use rusqlite::params;
use serde_json::{Value, json};

const PLAN: &str = "pay";

/// A started PR of Plan `pay`: its key, repo and Lead worktree.
struct Workflow {
    key: String,
    repo: PathBuf,
    lead: PathBuf,
}

fn plan() -> PlanId {
    PlanId::parse(PLAN).unwrap()
}

// ---------- fixtures without git: the rows alone, for the commands that read the store ----------

/// The `base_sha` of the Attempts of [`Rows`]: no repo exists to cut one from.
const BASE: &str = "0123456789abcdef0123456789abcdef01234567";

/// A repo, a worktree and a git process left out: the rows of [`Git`] (Context, Lead Session, Plan, PR `repo`,
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
         INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'pay', 'main', 'dispatch', 'cpd/pay', '{lead}', cpd_txn(), 't', 't');",
        repo = repos.join("pay").display(),
        lead = repos.join("pay.pay").display()
    ));
    let mut rows = Rows {
        fx,
        home,
        work: PathBuf::new(),
    };
    rows.work = rows.attempt("api", "running");
    rows
}

impl Rows {
    /// Job `key` (`running`) with Attempt `<key>-a1` in `status`, without a worktree. Returns the worktree's path.
    fn attempt(&self, key: &str, status: &str) -> PathBuf {
        let work = self
            .fx
            .env
            .root()
            .join("repos")
            .join(format!("pay.pay--{key}-a1"));
        self.fx.env.sql(&format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                               status, appended, fingerprint, created_txn, created_at, kind)
             VALUES ('pay', 'repo', '{key}', 'T', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'running', 0, '{f}', cpd_txn(), 't', 'work');
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
    /// What the route of `sh` answers with.
    answer: Arc<Mutex<Answer>>,
}

/// `files` are committed on `main` before the Lead worktree is cut from it.
fn git_fixture(files: &[(&str, &str)]) -> Git {
    git_fixture_kind(files, "work")
}

/// [`git_fixture`] with Job `api` of `kind` (`work`, `test` or `impl`).
fn git_fixture_kind(files: &[(&str, &str)], kind: &str) -> Git {
    git_fixture_needing(files, kind, None, None)
}

/// [`git_fixture_kind`] for a `test` Job's `(tests, test_files)` (JSON arrays) and, for an `impl` Job, the `tests` (a JSON array)
/// of the merged `test` Job `t` it needs, see [`Git::attempt_needing`].
fn git_fixture_needing(
    files: &[(&str, &str)],
    kind: &str,
    tests: Option<(&str, &str)>,
    needs: Option<&str>,
) -> Git {
    let fx = InProcess::with_ports();
    let repo = fx.git_repo("pay").canonicalize().unwrap();
    for (path, content) in files {
        fx.commit_file(&repo, path, content, "setup");
    }
    let lead = in_fakes(&fx, || {
        create_lead_worktree(&repo, &plan(), "main", &setup(&fx, "setup.log"))
    })
    .unwrap()
    .path;
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
         INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{}', 'pay', 'main', 'dispatch', 'cpd/pay', '{}', cpd_txn(), 't', 't');",
        repo.display(),
        lead.display()
    ));
    let answer: Answer =
        Arc::new(|command, _| panic!("a command no test scripted (Git::sh): {command}"));
    let answer = Arc::new(Mutex::new(answer));
    let _ = (*fx.fakes.runner)
        .clone()
        .route("sh", Arc::new(Sh(answer.clone())));
    let mut git = Git {
        fx,
        home,
        repo,
        lead,
        work: PathBuf::new(),
        answer,
    };
    git.work = git.attempt_needing(
        &Workflow {
            key: "repo".into(),
            repo: git.repo.clone(),
            lead: git.lead.clone(),
        },
        "api",
        "running",
        kind,
        tests,
        needs,
    );
    git
}

/// `f` with the fake ports of `fx` as the ambient ones: the core's git functions (`create_lead_worktree`, ...) run on them.
fn in_fakes<R>(fx: &InProcess, f: impl FnOnce() -> R) -> R {
    Ports::scope(&fx.fakes.ports, f)
}

/// [`git_fixture`] whose repo policy file is `file` (its `[hooks]`): the commands of its hooks are answered by [`Git::sh`].
fn hooked(file: &str) -> Git {
    let fx = git_fixture(&[]);
    fx.fx.env.repo_policy("acme", "pay", file);
    fx
}

/// How `sh -c <command>` is answered, from the command and the directory it runs in.
type Answer = Arc<dyn Fn(&str, &Path) -> Reply + Send + Sync>;

/// The route of `sh` on the `FakeRunner`: it answers with the [`Answer`] of the moment (the last [`Git::sh`]), and a command is
/// unscripted (the test fails) until a test gives one.
struct Sh(Arc<Mutex<Answer>>);

impl Handler for Sh {
    fn handle(&self, call: &Call) -> Reply {
        let [flag, command] = call.args.as_slice() else {
            panic!("sh was not called as `sh -c <command>`: {:?}", call.args)
        };
        assert_eq!(flag, "-c", "{:?}", call.args);
        let answer = self.0.lock().unwrap().clone();
        answer(
            command,
            call.cwd.as_deref().expect("a hook runs in a directory"),
        )
    }
}

fn setup(fx: &InProcess, log: &str) -> Setup {
    Setup {
        timeout: Duration::from_secs(30),
        log: fx.env.root().join(log),
    }
}

impl Git {
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    /// PR `key` of Plan `pay` in a new repo `<key>`, started: Lead Branch `cpd/pay/<key>` and its Lead worktree.
    fn add_pr(&self, key: &str) -> Workflow {
        let repo = self.fx.git_repo(key).canonicalize().unwrap();
        let branch = format!("cpd/pay/{key}");
        let lead =
            workflow_lead_worktree_path(&repo, &plan(), &WorkflowKey::parse(key).unwrap(), &branch)
                .unwrap();
        in_fakes(&self.fx, || {
            prepare_lead_worktree(
                &repo,
                &plan(),
                &lead,
                &branch,
                "main",
                &setup(&self.fx, "setup.log"),
            )
        })
        .map_err(|f| f.error)
        .unwrap();
        self.fx.env.repo_policy("acme", key, "");
        self.fx.env.sql(&format!(
            "INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
               VALUES ('pay', '{key}', '{}', '{key}', 'main', 'dispatch', '{branch}', '{}', cpd_txn(), 't', 't');",
            repo.display(),
            lead.display()
        ));
        Workflow {
            key: key.into(),
            repo,
            lead,
        }
    }

    /// A Job of `pr` and of `kind` (`work`, `test` or `impl`) with its Attempt `<key>-a1` in `status`: its worktree cut from the
    /// Lead Branch HEAD. Returns the worktree.
    fn attempt_in(&self, pr: &Workflow, key: &str, status: &str, kind: &str) -> PathBuf {
        self.attempt_needing(pr, key, status, kind, None, None)
    }

    /// [`Git::attempt_in`] for a Job of `kind` too of `test` (`tests`: its `(tests, test_files)`, JSON arrays) and `impl`
    /// (`needs`: the merged `test` Job `t`'s `tests` as a JSON array; its `test_files` is `["tests/"]`): `t` and the Dependency are
    /// written in the Job's own transaction, since a Dependency of a Locked Job is refused later (`dependencies_locked`).
    fn attempt_needing(
        &self,
        pr: &Workflow,
        key: &str,
        status: &str,
        kind: &str,
        tests: Option<(&str, &str)>,
        needs: Option<&str>,
    ) -> PathBuf {
        let attempt = AttemptId::parse(&format!("{key}-a1")).unwrap();
        let made = in_fakes(&self.fx, || {
            create_attempt_worktree(
                &pr.repo,
                &pr.lead,
                &plan(),
                &attempt,
                &setup(&self.fx, "attempt.log"),
            )
        })
        .unwrap();
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, created_txn, created_at, kind, tests, test_files)
                     VALUES ('pay', ?4, ?1, 'T', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'running', 0, ?2, ?3, 't', ?5, ?6, ?7)",
                    params![key, "f".repeat(64), tx.id(), pr.key, kind, tests.map(|t| t.0), tests.map(|t| t.1)],
                )?;
                if let Some(names) = needs {
                    tx.execute(
                        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
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

    /// [`Git::attempt_in`] for a `work` Job of the first PR.
    fn attempt_as(&self, key: &str, status: &str) -> PathBuf {
        self.attempt_in(
            &Workflow {
                key: "repo".into(),
                repo: self.repo.clone(),
                lead: self.lead.clone(),
            },
            key,
            status,
            "work",
        )
    }

    /// Commits `path` = `content` in `dir`, a worktree.
    fn commit_in(&self, dir: &Path, path: &str, content: &str) -> String {
        self.fx
            .commit_file(dir, path, content, &format!("edit {path}"));
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
        self.fx
            .cpd_in(cwd, &["attempt", "submit", "api-a1", "--plan", PLAN])
    }

    /// `cpd attempt <args> --plan pay`, in this process.
    fn cpd(&self, args: &[&str]) -> Run {
        let mut all = vec!["attempt"];
        all.extend(args);
        all.extend(["--plan", PLAN]);
        self.fx.cpd(&all)
    }

    fn status(&self) -> String {
        self.fx
            .env
            .query("SELECT status FROM attempts WHERE id = 'api-a1'")
    }

    /// Every command `cpd` runs through `sh -c` (a hook, a `[test-commands]` command) is answered by `answer(command, cwd)`.
    fn sh(&self, answer: impl Fn(&str, &Path) -> Reply + Send + Sync + 'static) {
        *self.answer.lock().unwrap() = Arc::new(answer);
    }

    /// Driver step A8 for Attempt `id` (`merging`), in this process, on the same fakes, store and config the driver uses: the
    /// merge runs the quick check (`gate.merge`, the repo's `pre-commit`, contract §7.4 step 6, hs-d679), which `submit` does not.
    fn merge(&self, id: &str) -> merge::Merge {
        let config = Config::load(&self.home).unwrap();
        let model = |m: &str| TierModel {
            model: m.to_owned(),
            efforts: vec![0, 1, 2, 3, 4, 5],
        };
        let catalog = Catalog {
            pi_version: "1.0.0".to_owned(),
            o: model("p/opus"),
            s: model("p/sonnet"),
            h: model("p/haiku"),
            g: TierModel {
                model: "p/gemini".to_owned(),
                efforts: vec![],
            },
        };
        let ctx = merge::Ctx {
            home: &self.home,
            config: &config,
            catalog: &catalog,
            source: "attempt test",
            metrics: &|_| None,
        };
        in_fakes(&self.fx, || {
            merge::merge(
                &mut self.store(),
                &ctx,
                &plan(),
                &AttemptId::parse(id).unwrap(),
            )
        })
        .unwrap()
    }

    /// The commands `cpd` has run through `sh -c` so far (rendered), in order.
    fn shells(&self) -> Vec<String> {
        self.fx
            .calls("sh")
            .into_iter()
            .map(|call| call.args[1].clone())
            .collect()
    }

    fn txns(&self) -> i64 {
        self.fx.env.query("SELECT count(*) FROM txns")
    }

    fn events(&self, kind: &str) -> Vec<Value> {
        self.fx.cpd(&["events", "list", "--kind", kind]).ok()["events"]
            .as_array()
            .unwrap()
            .clone()
    }

    /// The `attempt-status` Events' `data`, oldest first.
    fn statuses(&self) -> Vec<Value> {
        self.events("attempt-status")
            .iter()
            .map(|e| e["data"].clone())
            .collect()
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
    assert_eq!(
        out,
        json!({"ok": true, "attempt": "api-a1", "submitted": true, "sha": sha, "next": NEXT})
    );

    assert_eq!(fx.status(), "merging");
    let (submitted_sha, submitted_at, tail): (String, String, Option<String>) = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT submitted_sha, submitted_at, check_tail FROM attempts WHERE id = 'api-a1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!(submitted_sha, sha);
    assert!(
        submitted_at.ends_with('Z') && submitted_at.contains('T'),
        "{submitted_at}"
    );
    assert_eq!(
        tail, None,
        "no pre-commit table: nothing ran, check_tail stays null"
    );
    assert_eq!(
        fx.fx
            .env
            .query::<String>("SELECT status FROM jobs WHERE key = 'api'"),
        "running",
        "the Job stays running"
    );
    assert!(
        !fx.home
            .hook_log(
                &plan(),
                &AttemptId::parse("api-a1").unwrap(),
                cpd_core::home::Hook::PreCommit
            )
            .exists()
    );

    let events = fx.events("attempt-status");
    assert_eq!(events.len(), 3, "{events:?}");
    assert_eq!(
        events[0]["data"],
        json!({"from": "running", "to": "running", "check": "start", "what": "checks"})
    );
    assert_eq!(
        events[1]["data"],
        json!({"from": "running", "to": "running", "check": "end", "what": "checks"})
    );
    assert_eq!(
        events[2]["data"],
        json!({"from": "running", "to": "merging"})
    );
    assert_eq!(
        (
            events[2]["plan"].as_str(),
            events[2]["job"].as_str(),
            events[2]["attempt"].as_str()
        ),
        (Some("pay"), Some("api"), Some("api-a1"))
    );
    assert_eq!(
        fx.fx
            .env
            .query::<String>("SELECT command FROM txns ORDER BY id DESC LIMIT 1"),
        "attempt submit"
    );
}

#[test]
fn submit_works_from_a_subdirectory_and_takes_the_plan_from_the_environment() {
    let fx = git_fixture(&[]);
    let sha = fx.commit("src/a.rs", "x\n");
    let run = fx.fx.cpd_with(
        &fx.work.join("src"),
        &["attempt", "submit", "api-a1"],
        &[("CPD_PLAN", PLAN)],
    );
    assert_eq!(run.exit, 0, "{}", run.stdout);
    assert_eq!(
        fx.fx
            .env
            .query::<String>("SELECT submitted_sha FROM attempts WHERE id = 'api-a1'"),
        sha
    );
}

#[test]
fn submit_twice_is_already_and_writes_nothing() {
    let fx = git_fixture(&[]);
    let sha = fx.commit("a.txt", "a\n");
    fx.submit().ok();
    let txns = fx.txns();
    let out = fx.submit().ok();
    assert_eq!(
        out,
        json!({"ok": true, "attempt": "api-a1", "submitted": true, "sha": sha, "next": NEXT, "already": true})
    );
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
    let run = fx
        .fx
        .cpd_in(&web, &["attempt", "submit", "web-a1", "--plan", PLAN]);
    assert_eq!(
        refused(&run, "attempt_not_running", 3)["details"]["status"],
        "starting"
    );
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
    assert_eq!(
        error["details"]["base_sha"],
        fx.fx.git(&fx.work, &["rev-parse", "HEAD"])
    );
    assert_eq!(fx.status(), "running");
}

#[test]
fn submit_refuses_a_diff_that_touches_protected_paths() {
    let fx = git_fixture(&[("verify.sh", "#!/bin/sh\n")]);
    fx.fx.env.repo_policy(
        "acme",
        "pay",
        "[protected]\npaths = [\"verify.sh\", \"scripts/ci/\"]\n",
    );
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
    fx.fx
        .env
        .repo_policy("acme", "web", "[protected]\npaths = [\"verify.sh\"]\n");
    let ui = fx.attempt_in(&web, "ui", "running", "work");
    fx.commit_in(&ui, "verify.sh", "exit 0\n");
    fx.commit_in(&fx.work, "verify.sh", "exit 0\n");

    let error = refused(
        &fx.fx
            .cpd_in(&ui, &["attempt", "submit", "ui-a1", "--plan", PLAN]),
        "gate_tampered",
        3,
    );
    assert_eq!(error["details"]["paths"], json!(["verify.sh"]));
    assert_eq!(
        fx.fx
            .env
            .query::<String>("SELECT status FROM attempts WHERE id = 'ui-a1'"),
        "running"
    );
    // The first PR's repo policy has no such list: the same file is fine there.
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");
}

#[test]
fn submit_runs_the_quick_check_of_the_jobs_pr_with_its_lead_branch_as_target() {
    let fx = git_fixture(&[]);
    let web = fx.add_pr("web");
    fx.fx.env.repo_policy("acme", "web", "[gate]\nmerge = \"local\"\n[hooks.pre-commit]\ncheck = \"echo {{ target }} > ../target-seen.txt; echo checking-web\"\n");
    fx.sh(|_, _| Reply::ok("checking-web\n"));
    let ui = fx.attempt_in(&web, "ui", "running", "work");
    fx.commit_in(&ui, "ui.txt", "ui\n");

    fx.fx
        .cpd_in(&ui, &["attempt", "submit", "ui-a1", "--plan", PLAN])
        .ok();
    // The quick check moved from the submit to the merge (contract §7.3, §7.4 step 6, hs-d679): the submit runs none.
    assert_eq!(
        fx.shells(),
        Vec::<String>::new(),
        "submit runs no quick check"
    );
    fx.merge("ui-a1");

    // The command is rendered with the Lead Branch of the Job's PR as `target` (it ran in the Attempt's worktree).
    let shells = fx.fx.calls("sh");
    assert_eq!(shells.len(), 1, "{shells:?}");
    assert_eq!(
        shells[0].args,
        [
            "-c",
            "echo cpd/pay/web > ../target-seen.txt; echo checking-web"
        ]
    );
    assert_eq!(
        shells[0].cwd.as_deref(),
        Some(ui.canonicalize().unwrap().as_path())
    );
    // The first PR's Attempt has no hook of its own to run.
    fx.commit("a.txt", "a\n");
    fx.submit().ok();
    fx.merge("api-a1");
    assert_eq!(fx.shells().len(), 1, "no second command ran");
}

/// The repo policy's `[gate]` that runs its `pre-commit` (the quick check) at the merge: since hs-d679 the quick check is not
/// the submit's (contract §7.3 R20, §7.4 step 6, commands.md §6.1), so the tests below that keep their historical names pin
/// both halves: `attempt submit` runs no hook, and the merge runs it, in the Attempt's worktree, keeping its output.
const MERGE_GATE: &str = "[gate]\nmerge = \"local\"\n";

/// The `outcome` and `cause` the merge ended the Attempt with, and its detail.
fn failed(merged: &merge::Merge) -> (Failure, &str) {
    match &merged.end {
        End::Failed { cause, detail } => (*cause, detail),
        End::GateFailed { detail, .. } => (Failure::GateFailed, detail),
        other => panic!("the merge did not fail the Attempt: {other:?}"),
    }
}

#[test]
fn submit_runs_the_quick_check_and_keeps_its_tail() {
    let fx = hooked(&format!(
        "{MERGE_GATE}[hooks.pre-commit]\ncheck = \"echo checking; pwd; cat >/dev/null\"\n"
    ));
    // `pwd` prints the directory the command runs in.
    fx.sh(|_, cwd| Reply::ok(&format!("checking\n{}\n", cwd.display())));
    fx.commit("a.txt", "a\n");
    fx.submit().ok();
    // The submit ran nothing: the Attempt is `merging` with no tail and no log.
    assert_eq!(fx.shells(), Vec::<String>::new());
    assert_eq!(fx.status(), "merging");
    assert_eq!(
        fx.fx
            .env
            .query::<Option<String>>("SELECT check_tail FROM attempts WHERE id = 'api-a1'"),
        None
    );
    let log = fx.home.hook_log(
        &plan(),
        &AttemptId::parse("api-a1").unwrap(),
        cpd_core::home::Hook::PreCommit,
    );
    assert!(!log.exists(), "submit wrote no quick check log");

    // The merge runs it, in the worktree, and its output is the log.
    assert_eq!(fx.merge("api-a1").end.outcome(), "merged");
    let ran = fx.fx.calls("sh");
    assert_eq!(ran.len(), 1, "{ran:?}");
    let log = fs::read_to_string(log).unwrap();
    assert!(log.contains("checking"), "{log}");
    assert!(
        log.contains(fx.work.to_str().unwrap()),
        "the hook ran in the worktree: {log}"
    );
}

#[test]
fn submit_refuses_a_failed_quick_check_with_its_tail_and_log() {
    let fx = hooked(&format!(
        "{MERGE_GATE}[hooks.pre-commit]\ncheck = \"echo broken >&2; exit 7\"\n"
    ));
    fx.sh(|_, _| Reply::exit(7, "", "broken\n"));
    fx.commit("a.txt", "a\n");
    // The submit refuses nothing: a failed quick check is the merge's `gate_failed`, with its tail and log in the detail.
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");
    assert_eq!(fx.shells(), Vec::<String>::new());

    let merged = fx.merge("api-a1");
    let (cause, detail) = failed(&merged);
    assert_eq!(cause, Failure::GateFailed, "{detail}");
    assert!(
        detail.contains("exit 7") && detail.contains("broken"),
        "the tail: {detail}"
    );
    let log = fx.home.hook_log(
        &plan(),
        &AttemptId::parse("api-a1").unwrap(),
        cpd_core::home::Hook::PreCommit,
    );
    assert!(log.ends_with("logs/api-a1.pre-commit.log"), "{log:?}");
    assert!(detail.contains(log.to_str().unwrap()), "the log: {detail}");
    assert!(fs::read_to_string(log).unwrap().contains("broken"));
    let events = fx.events("attempt-status");
    let spans: Vec<_> = events
        .iter()
        .map(|e| (e["data"]["check"].as_str(), e["data"]["what"].as_str()))
        .collect();
    assert_eq!(
        &spans[..2],
        [
            (Some("start"), Some("checks")),
            (Some("end"), Some("checks"))
        ],
        "{events:?}"
    );
}

#[test]
fn submit_refuses_a_quick_check_that_could_not_run() {
    {
        let killed = hooked(&format!(
            "{MERGE_GATE}[hooks.pre-commit]\ncheck = \"echo before; kill -9 $$\"\n"
        ));
        // The shell was killed by SIGKILL after it printed `before`.
        killed.sh(|_, _| Reply::signal(9, "before\n", ""));
        killed.commit("a.txt", "a\n");
        killed.submit().ok();
        assert_eq!(killed.status(), "merging", "the submit does not run it");
        let merged = killed.merge("api-a1");
        let (cause, detail) = failed(&merged);
        assert_eq!(cause, Failure::GateError, "{detail}");
        assert!(detail.contains("before"), "{detail}");
    }

    let invalid = hooked(&format!(
        "{MERGE_GATE}[hooks.pre-commit]\ncheck = \"echo {{{{ no_such_variable }}}}\"\n"
    ));
    invalid.commit("a.txt", "a\n");
    invalid.submit().ok();
    let merged = invalid.merge("api-a1");
    let (cause, detail) = failed(&merged);
    assert_eq!(cause, Failure::GateError, "{detail}");
    assert!(detail.contains("hook_invalid"), "{detail}");
    assert_eq!(
        detail.split("tail:").nth(1).map(str::trim),
        Some(""),
        "no tail: {detail}"
    );
    assert_eq!(
        invalid.shells(),
        Vec::<String>::new(),
        "a template that does not render runs nothing"
    );
}

#[test]
fn submit_checks_its_arguments_and_selection() {
    let fx = git_fixture(&[]);
    fx.commit("a.txt", "a\n");
    refused(
        &fx.fx.cpd_in(&fx.work, &["attempt", "submit", "api-a1"]),
        "plan_required",
        2,
    );
    refused(
        &fx.fx
            .cpd_in(&fx.work, &["attempt", "submit", "Bad_Id", "--plan", PLAN]),
        "invalid_attempt_id",
        2,
    );
    refused(
        &fx.fx
            .cpd_in(&fx.work, &["attempt", "submit", "api-a9", "--plan", PLAN]),
        "attempt_not_found",
        4,
    );
    refused(
        &fx.fx
            .cpd_in(&fx.work, &["attempt", "submit", "api-a1", "--plan", "nope"]),
        "plan_not_found",
        4,
    );
    refused(&fx.fx.cpd_in(&fx.work, &["attempt", "submit"]), "usage", 2);
    assert_eq!(fx.status(), "running");
}

// ---------- attempt submit: the checks of the `test` and `impl` kinds (contract §7.7, commands.md §6.1) ----------
//
// The repo's commands run through `sh -c` on the `FakeRunner`: [`test_commands`] answers them, as `sh` would for the
// commands of [`TESTS`], from the files of the Attempt worktree (`BROKEN`, `GREEN`). What `cpd` is tested on is what it
// asks `sh` (the rendered command, its directory, its environment, the order) and what it makes of the answer.

/// A repo's `[test-commands]` (each command is one word of [`test_commands`], which stands for what the repo's would do): `build`
/// fails while `BROKEN` exists, `run` fails unless `GREEN` exists, so a test Job that adds neither is red and one that adds
/// `GREEN` is not. `run` is given the named tests (`run t::one`).
const TESTS: &str = "[test-commands]\n\
    build = \"build\"\n\
    run = \"run {{ tests }}\"\n";

/// `TESTS` with a `list` that exists for the names `t::one` and `t::two` only.
const TESTS_LIST: &str = "[test-commands]\n\
    build = \"build\"\n\
    list = \"list {{ test }}\"\n\
    run = \"run {{ tests }}\"\n";

/// The answer of `sh -c <command>` in the worktree `cwd` to a command of [`TESTS`] or [`TESTS_LIST`].
fn test_commands(command: &str, cwd: &Path) -> Reply {
    match command.split_once(' ').unwrap_or((command, "")) {
        ("build", _) => Reply::exit(i32::from(cwd.join("BROKEN").exists()), "building\n", ""),
        ("run", _) => Reply::exit(i32::from(!cwd.join("GREEN").exists()), "running\n", ""),
        ("list", name) => Reply::exit(i32::from(!["t::one", "t::two"].contains(&name)), "", ""),
        _ => panic!("a command nothing scripted: {command}"),
    }
}

/// Job `api` is a `test` Job of the named test `t::one`, held in `tests/t.rs`.
fn red(toml: &str) -> Git {
    red_naming(toml, r#"["t::one"]"#, r#"["tests/t.rs"]"#)
}

/// [`red`] with the `tests` and `test_files` (JSON arrays) of Job `api`.
fn red_naming(toml: &str, tests: &str, files: &str) -> Git {
    let fx = git_fixture_needing(&[], "test", Some((tests, files)), None);
    fx.fx.env.repo_policy("acme", "pay", toml);
    fx.sh(test_commands);
    fx
}

/// Job `api` is an `impl` Job that needs the merged `test` Job `t` (the tests `t::one` and `t::two`, in `tests/`).
fn green(toml: &str, approved: Option<&str>) -> Git {
    green_naming(toml, approved, r#"["t::one","t::two"]"#)
}

/// [`green`] with the tests `t` names.
fn green_naming(toml: &str, approved: Option<&str>, names: &str) -> Git {
    let fx = git_fixture_needing(&[], "impl", None, Some(names));
    fx.fx.env.repo_policy("acme", "pay", toml);
    fx.sh(test_commands);
    if let Some(paths) = approved {
        fx.fx.env.sql(&format!(
            "UPDATE jobs SET test_approved = '{paths}' WHERE key = 'api'"
        ));
    }
    fx
}

impl Git {
    /// The commands `cpd` has run through `sh -c` so far (rendered), in order, those that start with `word` (`""`: all).
    fn ran(&self, word: &str) -> Vec<String> {
        self.shells()
            .into_iter()
            .filter(|command| command.starts_with(word))
            .collect()
    }

    /// The log `<attempt>.tests-<what>.log` of `api-a1`.
    fn tests_log(&self, what: &str) -> PathBuf {
        self.home
            .root()
            .join(format!("run/pay/logs/api-a1.tests-{what}.log"))
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
    assert_eq!(
        (&details["job"], &details["tests"]),
        (&json!("api"), &json!(["t::one"]))
    );
    assert!(
        details["tail"].as_str().unwrap().contains("building"),
        "{error}"
    );
    assert_eq!(details["log"], fx.tests_log("build").to_str().unwrap());
    assert!(
        fs::read_to_string(fx.tests_log("build"))
            .unwrap()
            .contains("building")
    );
    assert_eq!(
        fx.ran("run"),
        Vec::<String>::new(),
        "`run` runs only after `build` succeeded"
    );
    assert_eq!(fx.status(), "running");
}

#[test]
fn a_test_job_whose_named_tests_pass_is_refused_tests_not_red_and_checked_again_at_every_submit() {
    let fx = red(TESTS);
    fx.commit("tests/t.rs", "// passes\n");
    fx.commit("GREEN", "x\n");
    let error = refused(&fx.submit(), "tests_not_red", 3);
    assert_eq!(
        (&error["details"]["job"], &error["details"]["tests"]),
        (&json!("api"), &json!(["t::one"]))
    );
    assert!(
        error["details"]["tail"]
            .as_str()
            .unwrap()
            .contains("running"),
        "{error}"
    );
    assert_eq!(
        error["details"]["log"],
        fx.tests_log("run").to_str().unwrap()
    );
    assert_eq!(
        fx.ran("run"),
        ["run t::one"],
        "the run was given the named tests"
    );
    assert_eq!(fx.status(), "running");
    // The Worker makes them fail and submits again: every submit runs the checks again.
    fx.fx.git(&fx.work, &["rm", "-q", "GREEN"]);
    fx.fx.git(&fx.work, &["commit", "-q", "-m", "red"]);
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");
    assert_eq!(fx.ran("build").len(), 2, "the checks ran again");
}

#[test]
fn a_test_job_whose_named_tests_fail_passes_and_leaves_both_logs() {
    let fx = red(TESTS);
    let sha = fx.commit("tests/t.rs", "// fails\n");
    let out = fx.submit().ok();
    assert_eq!(
        out,
        json!({"ok": true, "attempt": "api-a1", "submitted": true, "sha": sha, "next": NEXT})
    );
    assert_eq!(fx.status(), "merging");
    assert!(
        fs::read_to_string(fx.tests_log("build"))
            .unwrap()
            .contains("building")
    );
    assert!(
        fs::read_to_string(fx.tests_log("run"))
            .unwrap()
            .contains("running")
    );
    assert_eq!(fx.ran("run"), ["run t::one"]);
}

#[test]
fn the_tests_commands_are_not_given_a_test_slot() {
    // hs-d464: a run of named tests is not a full run; hs-d679 removed the test-slot pool entirely.
    let fx = red(TESTS);
    fx.commit("tests/t.rs", "// fails\n");
    fx.submit().ok();
    let runs: Vec<_> = fx
        .fx
        .calls("sh")
        .into_iter()
        .filter(|call| call.args[1].starts_with("run"))
        .collect();
    assert_eq!(runs.len(), 1);
    let slot = runs[0].envs.iter().find(|(key, _)| key == "CPD_TEST_SLOT");
    assert!(
        matches!(slot, None | Some((_, None))),
        "the run holds no slot: {slot:?}"
    );
}

#[test]
fn a_test_job_must_change_a_path_its_test_files_name_or_is_refused_test_files_missing() {
    let fx = red_naming(TESTS, r#"["t::one"]"#, r#"["tests/t.rs","tests/more/"]"#);
    fx.commit("tests/t.rs", "// red\n");
    let error = refused(&fx.submit(), "test_files_missing", 3);
    assert_eq!(error["details"]["paths"], json!(["tests/more/"]));
    assert_eq!(
        fx.ran(""),
        Vec::<String>::new(),
        "no command ran: the guard guards no file"
    );
    assert!(!fx.tests_log("build").exists());
    // A directory prefix matches a file inside it.
    fx.commit("tests/more/m.rs", "// red\n");
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");
}

/// hs-d716: a linear Attempt (its branch is the Lead Branch, hs-d680) diffs its red tests against the Attempt's
/// `base_sha` (`base...sha`), not the Lead Branch (`lead...sha` is empty when `lead == HEAD`).
#[test]
fn a_linear_test_job_checks_its_red_test_diff_against_the_attempt_base() {
    let fx = git_fixture(&[]);
    // A PR whose Lead Branch is `cpd/<plan>/<key>` (the non-migration-4 form a linear Attempt's branch takes).
    let web = fx.add_pr("web");
    fx.fx.env.repo_policy("acme", "web", TESTS);
    fx.sh(test_commands);
    fx.attempt_needing(
        &web,
        "ui",
        "running",
        "test",
        Some((r#"["t::one"]"#, r#"["tests/t.rs"]"#)),
        None,
    );
    // Seed the linear Attempt: branch == the PR's Lead Branch, worktree == the Lead worktree, base_sha == its tip.
    let base = fx.fx.git(&web.lead, &["rev-parse", "HEAD"]);
    fx.fx.env.sql(&format!(
        "UPDATE attempts SET branch = 'cpd/pay/web', worktree = '{}', base_sha = '{base}' WHERE id = 'ui-a1'",
        web.lead.display()
    ));
    fx.commit_in(&web.lead, "tests/t.rs", "// red\n");
    // The control: the commit changes exactly the file the Job's `test_files` names, and the Lead Branch diff is
    // empty because the linear Attempt's HEAD is the Lead Branch tip.
    assert_eq!(
        fx.fx.git(
            &web.lead,
            &["diff", "--name-only", &format!("{base}...HEAD")]
        ),
        "tests/t.rs"
    );
    assert_eq!(
        fx.fx
            .git(&web.lead, &["diff", "--name-only", "cpd/pay/web...HEAD"]),
        "",
        "linear: HEAD is the Lead Branch"
    );
    // On `main` this is refused `test_files_missing`; the fix diffs `base...sha`.
    let out = fx
        .fx
        .cpd_in(&web.lead, &["attempt", "submit", "ui-a1", "--plan", PLAN])
        .ok();
    assert_eq!(out["submitted"], true, "{out}");
    assert_eq!(
        fx.fx
            .env
            .query::<String>("SELECT status FROM attempts WHERE id = 'ui-a1'"),
        "merging"
    );
}

/// hs-d716: a linear Attempt's protected-path diff also compares against its `base_sha`, so a commit that changes a
/// protected path is refused `gate_tampered` (the Lead Branch diff would be empty).
#[test]
fn a_linear_attempt_that_changes_a_protected_path_is_refused_gate_tampered() {
    let fx = git_fixture(&[]);
    let web = fx.add_pr("web");
    fx.fx
        .env
        .repo_policy("acme", "web", "[protected]\npaths = [\"verify.sh\"]\n");
    fx.commit_in(&web.lead, "verify.sh", "#!/bin/sh\n");
    fx.attempt_needing(&web, "ui", "running", "work", None, None);
    let base = fx.fx.git(&web.lead, &["rev-parse", "HEAD"]);
    fx.fx.env.sql(&format!(
        "UPDATE attempts SET branch = 'cpd/pay/web', worktree = '{}', base_sha = '{base}' WHERE id = 'ui-a1'",
        web.lead.display()
    ));
    fx.commit_in(&web.lead, "verify.sh", "#!/bin/sh\nexit 0\n");
    let error = refused(
        &fx.fx
            .cpd_in(&web.lead, &["attempt", "submit", "ui-a1", "--plan", PLAN]),
        "gate_tampered",
        3,
    );
    assert_eq!(error["details"]["paths"], json!(["verify.sh"]));
    assert_eq!(
        fx.fx
            .env
            .query::<String>("SELECT status FROM attempts WHERE id = 'ui-a1'"),
        "running"
    );
}

fn written(fx: &Git) -> Option<String> {
    fx.fx
        .env
        .query("SELECT actual_files FROM attempts WHERE id = 'api-a1'")
}

#[test]
fn submit_stores_the_written_paths_in_the_update_that_makes_the_attempt_merging() {
    let fx = git_fixture(&[]);
    fx.commit("src/b.rs", "fn b() {}\n");
    fx.commit("a.md", "# a\n");
    fx.submit().ok();
    assert_eq!(written(&fx).as_deref(), Some(r#"["a.md","src/b.rs"]"#));
    let update = "SELECT count(*) FROM history WHERE tbl = 'attempts' AND op = 'update' AND json_extract(after, '$.status') = 'merging' AND json_extract(after, '$.actual_files') IS NOT NULL";
    assert_eq!(
        fx.fx.env.query::<i64>(update),
        1,
        "the update to merging holds the list"
    );
}

#[test]
fn submit_leaves_out_the_files_a_merge_of_the_lead_branch_brought() {
    let fx = git_fixture(&[]);
    fx.commit_in(&fx.lead, "lead.rs", "fn lead() {}\n");
    fx.commit("mine.rs", "fn mine() {}\n");
    fx.fx.git(
        &fx.work,
        &["merge", "-q", "--no-ff", "-m", "merge the lead", "cpd/pay"],
    );
    fx.submit().ok();
    assert_eq!(written(&fx).as_deref(), Some(r#"["mine.rs"]"#));
}

#[test]
fn submit_of_a_linear_attempt_stores_what_its_commits_wrote() {
    let fx = git_fixture(&[]);
    let web = fx.add_pr("web");
    fx.attempt_needing(&web, "ui", "running", "work", None, None);
    let base = fx.fx.git(&web.lead, &["rev-parse", "HEAD"]);
    fx.fx.env.sql(&format!(
        "UPDATE attempts SET branch = 'cpd/pay/web', worktree = '{}', base_sha = '{base}' WHERE id = 'ui-a1'",
        web.lead.display()
    ));
    fx.commit_in(&web.lead, "src/own.rs", "fn own() {}\n");
    fx.fx
        .cpd_in(&web.lead, &["attempt", "submit", "ui-a1", "--plan", PLAN])
        .ok();
    assert_eq!(
        fx.fx
            .env
            .query::<Option<String>>("SELECT actual_files FROM attempts WHERE id = 'ui-a1'")
            .as_deref(),
        Some(r#"["src/own.rs"]"#)
    );
}

#[test]
fn submit_of_an_open_job_with_no_commit_stores_an_empty_list() {
    let fx = git_fixture_kind(&[], "open");
    fx.fx
        .env
        .sql("UPDATE prs SET title = 'Pay' WHERE plan = 'pay' AND key = 'repo'");
    fx.fx.fake_gh(Reply::ok("[]"));
    fx.fx
        .env
        .repo_policy("acme", "pay", "[pr]\ngate = \"local\"\n");
    fx.submit().ok();
    assert_eq!(written(&fx).as_deref(), Some("[]"));
}

#[test]
fn submit_never_refuses_a_job_for_a_file_outside_its_touches() {
    let fx = git_fixture(&[]);
    fx.commit("zzz/other.rs", "fn other() {}\n");
    let out = fx.submit().ok();
    assert_eq!(out["submitted"], true, "{out}");
    assert_eq!(written(&fx).as_deref(), Some(r#"["zzz/other.rs"]"#));
}

#[test]
fn submit_whose_capture_fails_is_git_failed_and_writes_nothing() {
    let fx = git_fixture(&[]);
    fx.commit("src/a.rs", "fn a() {}\n");
    // The capture's `diff` is the one call with no `...` range argument.
    let capture = |call: &cpd_core::host::runner::Call| {
        call.args.iter().any(|a| a == "diff") && !call.args.iter().any(|a| a.contains("..."))
    };
    let _ =
        (*fx.fx.fakes.runner)
            .clone()
            .once("git", capture, Reply::exit(128, "", "fatal: boom\n"));
    refused(&fx.submit(), "git_failed", 1);
    assert_eq!(fx.status(), "running");
    assert_eq!(
        fx.fx
            .env
            .query::<Option<String>>("SELECT submitted_sha FROM attempts WHERE id = 'api-a1'"),
        None
    );
    assert_eq!(written(&fx), None);
}

#[test]
fn a_test_or_impl_job_in_a_repo_with_no_tests_table_is_refused_tests_unconfigured() {
    {
        let fx = red("");
        fx.commit("tests/t.rs", "// red\n");
        let error = refused(&fx.submit(), "tests_unconfigured", 3);
        let message = error["message"].as_str().unwrap();
        assert!(
            message.contains("--shape docs") && message.contains("config"),
            "{message}"
        );
        assert_eq!(fx.status(), "running");
    }

    let fx = green("", None);
    fx.commit("src/lib.rs", "// green\n");
    refused(&fx.submit(), "tests_unconfigured", 3);
}

#[test]
fn a_test_commands_table_with_a_list_is_read() {
    // The table is part of the repo's policy file (contract §7.7, §8.3); a table that is not as documented is `policy_invalid`.
    let fx = red(
        "[protected]\npaths = [\"verify.sh\"]\n[test-commands]\nbuild = \"build\"\nlist = \"list {{ test }}\"\nrun = \"run {{ tests }}\"\n",
    );
    fx.commit("tests/t.rs", "// red\n");
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");
    assert_eq!(
        fx.ran(""),
        ["build", "list t::one", "run t::one"],
        "the table's three commands, `list` with the variable `test`"
    );
}

/// hs-d679: the test-slot pool is gone. A Gate runs with no `run/test-<i>.lock` and no
/// `CPD_TEST_SLOT`: today the Gate runs at submit (a later Job moves it to the merge), and its
/// `pre-merge` hook must not find the slot variable set nor leave a slot lock behind.
#[test]
fn no_gate_takes_a_test_slot_lock_or_sets_cpd_test_slot() {
    let fx = git_fixture_kind(&[], "open");
    let seen = fx.fx.env.root().join("gate-slot.txt");
    // The open Job's Gate needs the PR's stored title and a `gh` that reports no pull request yet.
    fx.fx
        .env
        .sql("UPDATE workflows SET title = 'Pay' WHERE plan = 'pay' AND key = 'repo'");
    fx.fx.fake_gh(Reply::ok("[]"));
    fx.fx.env.repo_policy(
        "acme",
        "pay",
        &format!(
            "[pr]\ngate = \"local\"\n[hooks.pre-merge]\ngate = \"echo [$CPD_TEST_SLOT] > {}\"\n",
            seen.display()
        ),
    );
    fx.submit().ok();
    // If a Gate ran, it saw no slot variable; and it took no slot lock either way.
    if let Ok(slot) = fs::read_to_string(&seen) {
        assert_eq!(slot.trim(), "[]", "the Gate must not set CPD_TEST_SLOT");
    }
    for i in 0..8 {
        let lock = fx.home.root().join("run").join(format!("test-{i}.lock"));
        assert!(!lock.exists(), "the Gate must not take {lock:?}");
    }
}

#[test]
fn list_refuses_a_name_that_matches_no_test_before_run_for_a_test_job() {
    let fx = red_naming(TESTS_LIST, r#"["t::one","t::typo"]"#, r#"["tests/t.rs"]"#);
    fx.commit("tests/t.rs", "// red\n");
    let error = refused(&fx.submit(), "tests_not_found", 3);
    let details = &error["details"];
    assert_eq!(
        (&details["job"], &details["tests"]),
        (&json!("api"), &json!(["t::typo"])),
        "only the name that matches nothing"
    );
    assert_eq!(details["log"], fx.tests_log("list").to_str().unwrap());
    assert_eq!(
        fx.ran("list"),
        ["list t::one", "list t::typo"],
        "`list` runs once per named test, with the variable `test`"
    );
    assert_eq!(fx.ran("run"), Vec::<String>::new(), "before `run`");
    assert_eq!(fx.status(), "running");
}

#[test]
fn list_refuses_a_name_that_matches_no_test_for_an_impl_job_too_and_passes_when_every_name_exists()
{
    {
        let fx = green_naming(TESTS_LIST, None, r#"["t::one","t::missing"]"#);
        fx.commit("src/lib.rs", "// green\n");
        let error = refused(&fx.submit(), "tests_not_found", 3);
        assert_eq!(error["details"]["tests"], json!(["t::missing"]));
        assert_eq!(fx.ran("run"), Vec::<String>::new());
    }

    // Every name exists: `list` does not decide, `run` does (here it fails: a red test Job passes).
    let fx = red(TESTS_LIST);
    fx.commit("tests/t.rs", "// red\n");
    fx.submit().ok();
    assert_eq!(fx.ran("list"), ["list t::one"]);
    assert_eq!(fx.ran("run"), ["run t::one"]);
}

#[test]
fn a_command_that_cannot_run_is_tests_error_and_changes_nothing() {
    // `kill -9 $$` is a reply of the shell killed by SIGKILL; a template that does not render runs nothing.
    type Script = fn(&str, &Path) -> Reply;
    let cases: [(&str, Script, &str); 3] = [
        (
            "[test-commands]\nbuild = \"build\"\nrun = \"run\"\n",
            |_, _| Reply::signal(9, "", ""),
            "build killed by a signal",
        ),
        (
            "[test-commands]\nbuild = \"build\"\nrun = \"run\"\n",
            |command, cwd| {
                if command == "build" {
                    test_commands(command, cwd)
                } else {
                    Reply::signal(9, "", "")
                }
            },
            "run killed by a signal",
        ),
        (
            "[test-commands]\nbuild = \"echo {{ no_such_variable }}\"\nrun = \"run\"\n",
            test_commands,
            "hook_invalid",
        ),
    ];
    for (toml, script, what) in cases {
        let fx = red(toml);
        fx.sh(script);
        fx.commit("tests/t.rs", "// red\n");
        let error = refused(&fx.submit(), "tests_error", 1);
        assert_eq!(error["details"]["job"], "api", "{what}");
        assert_eq!(fx.status(), "running", "{what}");
    }
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
    assert_eq!(
        fx.ran("run"),
        ["run t::one t::two"],
        "derived: the Job names no tests of its own"
    );
    assert!(
        fs::read_to_string(fx.tests_log("run"))
            .unwrap()
            .contains("running")
    );
}

#[test]
fn an_impl_job_whose_tests_still_fail_is_refused_tests_failed() {
    let fx = green(TESTS, None);
    fx.commit("src/lib.rs", "// not enough\n");
    let error = refused(&fx.submit(), "tests_failed", 3);
    assert_eq!(
        (&error["details"]["job"], &error["details"]["tests"]),
        (&json!("api"), &json!(["t::one", "t::two"]))
    );
    assert_eq!(
        error["details"]["log"],
        fx.tests_log("run").to_str().unwrap()
    );
    assert!(
        error["details"]["tail"]
            .as_str()
            .unwrap()
            .contains("running"),
        "{error}"
    );
    assert_eq!(fx.status(), "running");
}

#[test]
fn an_impl_job_whose_build_fails_is_refused_tests_build_failed() {
    let fx = green(TESTS, None);
    fx.commit("src/lib.rs", "// green\n");
    fx.commit("BROKEN", "x\n");
    let error = refused(&fx.submit(), "tests_build_failed", 3);
    assert_eq!(
        error["details"]["log"],
        fx.tests_log("build").to_str().unwrap()
    );
    assert_eq!(fx.ran("run"), Vec::<String>::new());
}

#[test]
fn an_impl_job_may_not_touch_the_files_of_the_test_job_it_needs_unless_the_lead_approved_the_path()
{
    let fx = green(TESTS, None);
    fx.commit("GREEN", "x\n");
    fx.commit("tests/t.rs", "// edited to pass\n");
    fx.commit("tests/new/n.rs", "// a new file in a guarded directory\n");
    let error = refused(&fx.submit(), "tests_tampered", 3);
    let mut paths: Vec<String> = serde_json::from_value(error["details"]["paths"].clone()).unwrap();
    paths.sort();
    assert_eq!(
        (paths, &error["details"]["jobs"]),
        (
            vec!["tests/new/n.rs".to_owned(), "tests/t.rs".to_owned()],
            &json!(["t"])
        )
    );
    assert_eq!(
        fx.ran("run"),
        Vec::<String>::new(),
        "refused before the tests ran"
    );
    assert_eq!(fx.status(), "running");

    // Approving one path is not approving the other; both approved, the Job passes (`test_approved` is read at the check).
    fx.fx
        .env
        .sql("UPDATE jobs SET test_approved = '[\"tests/t.rs\"]' WHERE key = 'api'");
    assert_eq!(
        refused(&fx.submit(), "tests_tampered", 3)["details"]["paths"],
        json!(["tests/new/n.rs"])
    );
    fx.fx
        .env
        .sql("UPDATE jobs SET test_approved = '[\"tests/t.rs\",\"tests/new/\"]' WHERE key = 'api'");
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");
}

#[test]
fn only_an_impl_job_is_guarded_and_only_the_files_of_the_test_jobs_it_needs() {
    // A `test` Job may change its own files, and an `impl` Job anything else.
    {
        let fx = red(TESTS);
        fx.commit("tests/t.rs", "// red\n");
        fx.submit().ok();
    }
    let fx = green(TESTS, None);
    fx.commit("GREEN", "x\n");
    fx.commit("src/tests/lib.rs", "// not under tests/ at the root\n");
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");
}

#[test]
fn the_kind_checks_run_after_the_quick_check_and_a_work_job_runs_none() {
    {
        // The quick check is the merge's now (hs-d679): a repo whose `pre-commit` would fail has it run by no submit, and
        // the kind's checks are the only ones of the span.
        let toml = (
            "[gate]\nmerge = \"local\"\n[hooks.pre-commit]\ncheck = \"exit 7\"\n",
            "[test-commands]\nbuild = \"build\"\nrun = \"false\"\n",
        );
        let fx = git_fixture_needing(
            &[],
            "test",
            Some((r#"["t::one"]"#, r#"["tests/t.rs"]"#)),
            None,
        );
        fx.fx
            .env
            .repo_policy("acme", "pay", &format!("{}{}", toml.0, toml.1));
        // `run = "false"` is red, as a `test` Job's tests must be.
        fx.sh(|command, cwd| {
            if command == "false" {
                Reply::exit(1, "", "")
            } else {
                test_commands(command, cwd)
            }
        });
        fx.commit("tests/t.rs", "// red\n");
        fx.submit().ok();
        assert_eq!(
            fx.ran(""),
            ["build", "false"],
            "the tests ran, the quick check did not"
        );
        assert!(fx.tests_log("build").exists(), "the tests ran");
        assert_eq!(
            fx.statuses(),
            [
                span("start"),
                span("end"),
                json!({"from": "running", "to": "merging"})
            ]
        );
    }

    // A `work` Job in a repo that has `[test-commands]`: no kind check runs, and its checks are one span.
    let fx = git_fixture(&[]);
    fx.fx.env.repo_policy("acme", "pay", TESTS);
    fx.commit("BROKEN", "x\n");
    fx.submit().ok();
    assert_eq!(fx.ran("run"), Vec::<String>::new());
    assert!(!fx.tests_log("build").exists());
    assert_eq!(fx.ran(""), Vec::<String>::new(), "no command ran at all");
    assert_eq!(
        fx.statuses(),
        [
            span("start"),
            span("end"),
            json!({"from": "running", "to": "merging"})
        ]
    );
}

#[test]
fn a_kind_check_is_a_span_of_the_attempt_that_ends_also_when_it_refuses() {
    // Refused: `start` then `end`, and the Attempt stays running.
    let fx = red(TESTS);
    fx.commit("tests/t.rs", "// passes\n");
    fx.commit("GREEN", "x\n");
    refused(&fx.submit(), "tests_not_red", 3);
    assert_eq!(fx.statuses(), [span("start"), span("end")]);
    assert!(
        fx.home.locks_dir().join("check-pay--api-a1.lock").exists(),
        "held by the submit, removed when the Attempt ends"
    );

    // Passed: the span closes before the Attempt becomes `merging`.
    fx.fx.git(&fx.work, &["rm", "-q", "GREEN"]);
    fx.fx.git(&fx.work, &["commit", "-q", "-m", "red"]);
    fx.submit().ok();
    assert_eq!(
        fx.statuses(),
        [
            span("start"),
            span("end"),
            span("start"),
            span("end"),
            json!({"from": "running", "to": "merging"})
        ]
    );
    // The Events carry the Attempt and the Job.
    let events = fx.events("attempt-status");
    assert_eq!(
        (
            events[0]["plan"].as_str(),
            events[0]["job"].as_str(),
            events[0]["attempt"].as_str()
        ),
        (Some("pay"), Some("api"), Some("api-a1"))
    );
}

#[test]
fn a_work_jobs_checks_are_one_span_that_ends_in_the_transaction_that_makes_the_attempt_merging() {
    let fx = git_fixture(&[]);
    fx.commit("a.txt", "x\n");
    fx.submit().ok();
    assert_eq!(
        fx.statuses(),
        [
            span("start"),
            span("end"),
            json!({"from": "running", "to": "merging"})
        ]
    );
    let txns = fx.status_txns();
    assert_eq!(
        txns[1], txns[2],
        "the end and running -> merging share a transaction"
    );
    assert_ne!(txns[0], txns[1]);
}

#[test]
fn a_refused_quick_check_ends_the_checks_span_and_leaves_the_attempt_running() {
    let fx = hooked(&format!(
        "{MERGE_GATE}[hooks.pre-commit]\ncheck = \"exit 7\"\n"
    ));
    fx.sh(|_, _| Reply::exit(7, "", ""));
    fx.commit("a.txt", "x\n");
    // A failing quick check refuses nothing at the submit (hs-d679): the span closes with the Attempt `merging`, and the
    // merge is what the failure ends, the Attempt `failed` / `gate_failed`.
    fx.submit().ok();
    assert_eq!(
        fx.statuses(),
        [
            span("start"),
            span("end"),
            json!({"from": "running", "to": "merging"})
        ]
    );
    assert_eq!(fx.status(), "merging");
    assert_eq!(failed(&fx.merge("api-a1")).0, Failure::GateFailed);
}

#[test]
fn a_test_jobs_quick_check_and_tests_run_in_one_checks_span() {
    let fx = red(TESTS);
    fx.commit("tests/t.rs", "// red\n");
    fx.submit().ok();
    assert_eq!(
        fx.statuses(),
        [
            span("start"),
            span("end"),
            json!({"from": "running", "to": "merging"})
        ]
    );
    let txns = fx.status_txns();
    assert_eq!(txns[1], txns[2]);
}

// ---------- attempt read ----------

const SESSION: &str = concat!(
    r#"{"type":"session","id":"w1"}"#,
    "\n",
    r#"{"type":"message","message":{"role":"user","content":[{"type":"text","text":"Read the spec."}]}}"#,
    "\n",
    r#"{"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","name":"read","arguments":{}}]}}"#,
    "\n",
    r#"{"type":"message","message":{"role":"toolResult","content":[{"type":"text","text":"spec"}]}}"#,
    "\n",
    r#"{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"The schema compiles;\ncommitting."},{"type":"toolCall","name":"bash","arguments":{}}]}}"#,
    "\n",
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
    assert_eq!(
        out["lines"],
        json!(["assistant: committing.", "tool: bash"])
    );
    assert_eq!(
        out["total"], 4,
        "total counts every line, not the kept ones"
    );
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
    fx.fx.env.sql(&format!(
        "UPDATE sessions SET file = '{}' WHERE id = 'w1'",
        gone.display()
    ));
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
            "submitted_sha": null, "submitted_at": null, "check_tail": null, "gate_hook": null, "gate_command": null, "gate_exit": null, "merged_sha": null, "nudged": true,
            "stop_request": null, "metrics": {"turns": 3}, "started_at": "t", "ended_at": null,
            "runner": "pi", "worker_pid": null, "actual_files": null,
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
    let fx = hooked(&format!(
        "{MERGE_GATE}[hooks.pre-commit]\ncheck = \"echo hi\"\n"
    ));
    fx.sh(|_, _| Reply::ok("hi\n"));
    fx.commit("a.txt", "a\n");
    fx.submit().ok();
    // No quick check has run at the submit (hs-d679): no tail, no log to name.
    let out = fx.cpd(&["show", "api-a1"]).ok();
    assert_eq!(out["attempt"]["status"], "merging");
    assert_eq!(out["attempt"]["check_tail"], Value::Null);
    assert_eq!(out["paths"]["check"], Value::Null);
    // The merge runs it: `show` names the log.
    fx.merge("api-a1");
    let out = fx.cpd(&["show", "api-a1"]).ok();
    let check = out["paths"]["check"].as_str().unwrap();
    assert_eq!(fs::read_to_string(check).unwrap(), "hi\n");
    assert!(
        check.ends_with("run/pay/logs/api-a1.pre-commit.log"),
        "{check}"
    );
    assert_eq!(out["paths"]["worker_file"], Value::Null);
    assert_eq!(out["paths"]["spec"], Value::Null);
}

#[test]
fn show_prints_the_captured_actual_files_as_an_array() {
    let fx = rows();
    fx.fx
        .env
        .sql("UPDATE attempts SET actual_files = '[\"a.rs\",\"docs/x.md\"]' WHERE id = 'api-a1'");
    let out = fx.cpd(&["show", "api-a1"]).ok();
    assert_eq!(out["attempt"]["actual_files"], json!(["a.rs", "docs/x.md"]));
    fx.fx
        .env
        .sql("UPDATE attempts SET actual_files = '[]' WHERE id = 'api-a1'");
    let out = fx.cpd(&["show", "api-a1"]).ok();
    assert_eq!(out["attempt"]["actual_files"], json!([]));
}

#[test]
fn show_prints_null_actual_files_for_an_attempt_that_never_captured() {
    let fx = rows();
    fx.fx
        .env
        .sql("UPDATE attempts SET actual_files = NULL WHERE id = 'api-a1'");
    let out = fx.cpd(&["show", "api-a1"]).ok();
    assert!(
        out["attempt"]
            .as_object()
            .unwrap()
            .contains_key("actual_files"),
        "{out}"
    );
    assert_eq!(out["attempt"]["actual_files"], Value::Null);
}

#[test]
fn show_refuses_what_does_not_exist() {
    let fx = rows();
    refused(&fx.cpd(&["show", "api-a2"]), "attempt_not_found", 4);
    refused(
        &fx.raw(&["attempt", "show", "api-a1", "--plan", "nope"]),
        "plan_not_found",
        4,
    );
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
    out["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect()
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
    assert_eq!(
        ids(&fx.cpd(&["list", "--job", "api"]).ok()),
        ["api-a1", "api-a2"]
    );
    assert_eq!(
        ids(&fx.cpd(&["list", "--active"]).ok()),
        ["api-a2", "web-a1"]
    );
    assert_eq!(
        ids(&fx.cpd(&["list", "--active", "--job", "api"]).ok()),
        ["api-a2"]
    );
    assert_eq!(ids(&fx.cpd(&["list", "--job", "web"]).ok()), ["web-a1"]);
}

#[test]
fn list_refuses_what_does_not_exist() {
    let fx = listed();
    refused(&fx.cpd(&["list", "--job", "ghost"]), "job_not_found", 4);
    refused(&fx.cpd(&["list", "--job", "Bad_Key"]), "invalid_job_id", 2);
    refused(
        &fx.raw(&["attempt", "list", "--plan", "nope"]),
        "plan_not_found",
        4,
    );
    refused(&fx.raw(&["attempt", "list"]), "plan_required", 2);
    fx.fx.env.sql(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
         VALUES ('pay', 'repo', 'idle', 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, 1, 't')",
    );
    assert_eq!(
        ids(&fx.cpd(&["list", "--job", "idle"]).ok()),
        Vec::<&str>::new()
    );
}

// ---------- attempt-ended carries the metrics (hs-d798) ----------

impl Git {
    /// [`Git::merge`] with `metrics` as A9's metrics: the driver computes them once (from the
    /// Worker's session file or end report) and the same value ends the Attempt (contract §3.4 A9).
    fn merge_with_metrics(&self, metrics: Value) {
        let config = Config::load(&self.home).unwrap();
        let model = |m: &str| TierModel {
            model: m.to_owned(),
            efforts: vec![0, 1, 2, 3, 4, 5],
        };
        let catalog = Catalog {
            pi_version: "1.0.0".to_owned(),
            o: model("p/opus"),
            s: model("p/sonnet"),
            h: model("p/haiku"),
            g: TierModel {
                model: "p/gemini".to_owned(),
                efforts: vec![],
            },
        };
        let ctx = merge::Ctx {
            home: &self.home,
            config: &config,
            catalog: &catalog,
            source: "attempt test",
            metrics: &|_| Some(metrics.clone()),
        };
        in_fakes(&self.fx, || {
            merge::merge(
                &mut self.store(),
                &ctx,
                &plan(),
                &AttemptId::parse("api-a1").unwrap(),
            )
        })
        .unwrap();
    }
}

#[test]
fn attempt_ended_carries_the_metrics_of_a_pi_attempt() {
    let fx = git_fixture(&[]);
    fx.commit("src/a.rs", "fn a() {}\n");
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");
    let metrics = json!({
        "turns": 2, "tokens_in": 110, "tokens_out": 55, "cache_read": 1000, "cache_write": 207,
        "cost_usd": 0.0168, "duration_s": 20.25, "model": "claude-opus-y", "thinking_level": "high",
        "stop_reason": "stop"
    });
    fx.merge_with_metrics(metrics.clone());
    // Exactly one `attempt-ended` Event for the Attempt, whose `data` is the metrics blob.
    let events = fx.events("attempt-ended");
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["kind"], "attempt-ended");
    assert_eq!(events[0]["data"], metrics);
    // The same transaction that writes `attempts.metrics` appends the Event.
    let event_txn: i64 = fx
        .fx
        .env
        .query("SELECT txn FROM events WHERE kind = 'attempt-ended' AND attempt = 'api-a1'");
    let metrics_txn: i64 = fx
        .fx
        .env
        .query("SELECT txn FROM history WHERE tbl = 'attempts' AND pk LIKE '%api-a1%' AND json_extract(after, '$.metrics') IS NOT NULL ORDER BY txn DESC LIMIT 1");
    assert_eq!(
        event_txn, metrics_txn,
        "the metrics and the Event share a transaction"
    );
}

#[test]
fn attempt_ended_carries_the_metrics_of_a_durable_attempt() {
    let fx = git_fixture(&[]);
    fx.fx
        .env
        .sql("UPDATE attempts SET runner = 'durable' WHERE id = 'api-a1'");
    fx.commit("src/a.rs", "fn a() {}\n");
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");
    let metrics = json!({
        "turns": 5, "tokens_in": 20, "tokens_out": 20, "cache_read": 0, "cache_write": 0,
        "cost_usd": 0.0, "duration_s": 12.5, "model": "p/sonnet", "thinking_level": null,
        "stop_reason": "end_turn"
    });
    fx.merge_with_metrics(metrics.clone());
    let events = fx.events("attempt-ended");
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["kind"], "attempt-ended");
    assert_eq!(events[0]["data"], metrics);
}

#[test]
fn events_follow_carries_the_attempt_ended_payload() {
    let fx = git_fixture(&[]);
    fx.commit("src/a.rs", "fn a() {}\n");
    fx.submit().ok();
    assert_eq!(fx.status(), "merging");
    let metrics = json!({
        "turns": 2, "tokens_in": 110, "tokens_out": 55, "cache_read": 1000, "cache_write": 207,
        "cost_usd": 0.0168, "duration_s": 20.25, "model": "claude-opus-y", "thinking_level": "high",
        "stop_reason": "stop"
    });
    fx.merge_with_metrics(metrics.clone());
    // `cpd events follow --until-idle --kind attempt-ended` replays the metrics payload. It owns its stdout, so it runs
    // through the spawned binary (`env.cpd`), as `session_report.rs` does.
    let run = fx.fx.env.cpd(&[
        "events",
        "follow",
        "--plan",
        PLAN,
        "--until-idle",
        "--since",
        "0",
        "--kind",
        "attempt-ended",
    ]);
    assert_eq!(run.exit, 0, "{run:?}");
    let lines: Vec<Value> = run
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0]["kind"], "attempt-ended");
    assert_eq!(lines[0]["data"], metrics);
}
