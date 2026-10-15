//! `pr open`, `pr poll` and `pr rebase` (commands.md §9, contract §7.6): the
//! built binary against temporary repos with a bare `origin`, a fake `gh`, and
//! a Plan written straight into the database.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{Run, TestEnv};
use cpd2::git::{Setup, create_attempt_worktree, create_lead_worktree};
use cpd2::home::CpdHome;
use cpd2::ids::{AttemptId, PlanId};
use cpd2::merge::RepoLock;
use cpd2::store::Store;
use rusqlite::params;
use serde_json::{Value, json};

const URL: &str = "https://github.com/acme/pay/pull/812";

/// The fake `gh`: each call is answered from a file in `gh/`; `gh/fail`, when
/// present, makes every call fail with its text on stderr.
const GH: &str = r#"
dir="$(dirname "$0")/../gh"
if [ -e "$dir/fail" ]; then cat "$dir/fail" >&2; exit 1; fi
case "$1 $2" in
  "pr list") cat "$dir/list.json" ;;
  "pr create") cat "$dir/create.out" ;;
  "pr view") cat "$dir/view.json" ;;
  "api repos/{owner}/{repo}/pulls/812/comments") cat "$dir/inline.json" ;;
  *) echo "unexpected gh call: $*" >&2; exit 9 ;;
esac
"#;

const TITLE: &str = "Add the payment webhook";

const OPEN_VIEW: &str = r#"{"state":"OPEN","mergedAt":null,"title":"Add the payment webhook","reviewDecision":"","statusCheckRollup":[],"reviews":[],"comments":[]}"#;

fn plan() -> PlanId {
    PlanId::parse("pay").unwrap()
}

/// A repo with a bare `origin`, the Lead worktree of Plan `pay` with one
/// Merged Job's commit on it, a database and a config.
struct Fx {
    env: TestEnv,
    home: CpdHome,
    repo: PathBuf,
    origin: PathBuf,
    lead: PathBuf,
    merged_sha: String,
}

fn fx(phase: &str) -> Fx {
    fx_with(phase, "")
}

fn fx_with(phase: &str, config_extra: &str) -> Fx {
    fx_full(phase, config_extra, "merged")
}

/// `job` is the status of the Plan's one Job, `merged` or `cancelled`.
fn fx_full(phase: &str, config_extra: &str, job: &str) -> Fx {
    let env = TestEnv::new();
    let repo = env.git_repo("pay").canonicalize().unwrap();
    let origin = env.root().join("repos/origin.git");
    env.git(env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
    env.git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
    env.git(&repo, &["push", "-q", "origin", "main"]);
    let setup = Setup { timeout: Duration::from_secs(30), log: env.root().join("lead.log") };
    let lead = create_lead_worktree(&repo, &plan(), "main", &setup).unwrap().path;
    env.git(&lead, &["config", "user.name", "cpd2 test"]);
    env.git(&lead, &["config", "user.email", "test@cpd2.invalid"]);
    fs::write(lead.join("api.txt"), "api\n").unwrap();
    env.git(&lead, &["add", "-A"]);
    env.git(&lead, &["commit", "-q", "-m", "api"]);
    let merged_sha = env.git(&lead, &["rev-parse", "HEAD"]);

    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(
        home.config(),
        format!("version = 1\nmachine = \"t\"\n[timeouts]\nlock_wait_seconds = 1\n{config_extra}\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket()),
    )
    .unwrap();
    let gh = env.root().join("gh");
    fs::create_dir_all(&gh).unwrap();
    for (name, text) in [("list.json", "[]"), ("create.out", URL), ("view.json", OPEN_VIEW), ("inline.json", "[]")] {
        fs::write(gh.join(name), format!("{text}\n")).unwrap();
    }
    env.fake_gh(GH);

    let fx = Fx { env, home, repo, origin, lead, merged_sha };
    if phase == "pr" {
        fx.git(&fx.lead, &["push", "-q", "-u", "origin", "cpd/pay"]);
    }
    let (worktree, columns, values) = match phase {
        "plan" => ("NULL".to_owned(), "", ""),
        "dispatch" => (format!("'{}'", fx.lead.display()), ", locked_at", ", 't'"),
        "pr" => (
            format!("'{}'", fx.lead.display()),
            ", locked_at, pr_number, pr_url, pr_opened_at",
            &format!(", 't', 812, '{URL}', 'opened'")[..],
        ),
        "done" => (
            format!("'{}'", fx.lead.display()),
            ", locked_at, pr_number, pr_url, pr_opened_at, done_at",
            &format!(", 't', 812, '{URL}', 'opened', 'finished'")[..],
        ),
        other => panic!("no phase {other}"),
    };
    let sql = format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session, created_at{columns})
           VALUES ('pay', 'ctx', 'Pay the bills', '{repo}', 'main', 'cpd/pay', {worktree}, '{phase}', 'lead1', 't'{values});",
        repo = fx.repo.display(),
    );
    fx.store()
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            tx.execute(
                "INSERT INTO jobs (plan, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, merged_sha, cancel_reason, cancel_requested_at, created_txn, created_at)
                 VALUES ('pay', 'api', 'The | API', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', ?4, 0, ?1,
                         iif(?4 = 'merged', ?2, NULL), iif(?4 = 'cancelled', 'not needed', NULL), iif(?4 = 'cancelled', 't', NULL), ?3, 't')",
                params!["f".repeat(64), fx.merged_sha, tx.id(), job],
            )?;
            Ok(())
        })
        .unwrap();
    fx
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        self.env.git(dir, args)
    }

    fn pr(&self, command: &str) -> Run {
        self.env.cpd2(&["pr", command, "--plan", "pay"])
    }

    /// Sets what the fake `gh` answers: `list.json`, `create.out`, `view.json`, `inline.json`, `fail`.
    fn gh(&self, file: &str, text: &str) {
        fs::write(self.env.root().join("gh").join(file), format!("{text}\n")).unwrap();
    }

    /// Commits the Lead worktree's `.config/wt.toml`.
    fn hooks(&self, toml: &str) {
        fs::create_dir_all(self.lead.join(".config")).unwrap();
        fs::write(self.lead.join(".config/wt.toml"), toml).unwrap();
        self.git(&self.lead, &["add", "-A"]);
        self.git(&self.lead, &["commit", "-q", "-m", "hooks"]);
    }

    fn remote_branch(&self, branch: &str) -> Option<String> {
        let out = self.git(&self.origin, &["for-each-ref", "--format=%(objectname)", &format!("refs/heads/{branch}")]);
        (!out.is_empty()).then_some(out)
    }

    fn lead_sha(&self) -> String {
        self.git(&self.repo, &["rev-parse", "refs/heads/cpd/pay"])
    }

    /// `(phase, pr_number, pr_url, pr_opened_at, pr_seen, pr_polled_at, done_at)` of the Plan.
    fn plan_row(&self) -> (String, Option<i64>, Option<String>, Option<String>, Value, Option<String>, Option<String>) {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT phase, pr_number, pr_url, pr_opened_at, pr_seen, pr_polled_at, done_at FROM plans WHERE id = 'pay'",
                    [],
                    |r| {
                        let seen: String = r.get(4)?;
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, serde_json::from_str(&seen).unwrap(), r.get(5)?, r.get(6)?))
                    },
                )?)
            })
            .unwrap()
    }

    /// `(pr_title, pr_review_decision, pr_checks)` of the Plan; `None` while all three are null.
    fn pr_data(&self) -> Option<(String, String, String)> {
        let (title, decision, checks): (Option<String>, Option<String>, Option<String>) = self
            .store()
            .read(|c| {
                Ok(c.query_row("SELECT pr_title, pr_review_decision, pr_checks FROM plans WHERE id = 'pay'", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?)
            })
            .unwrap();
        match (title, decision, checks) {
            (None, None, None) => None,
            (Some(t), Some(d), Some(c)) => Some((t, d, c)),
            partial => panic!("the three columns are set together, or not at all: {partial:?}"),
        }
    }

    /// `(kind, data)` of the Plan's Events of `kind`, in order.
    fn events(&self, kind: &str) -> Vec<(String, Value)> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare("SELECT text, data FROM events WHERE plan = 'pay' AND kind = ?1 ORDER BY id")?;
                let rows = stmt
                    .query_map([kind], |r| Ok((r.get::<_, String>(0)?, serde_json::from_str(&r.get::<_, String>(1)?).unwrap())))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    fn gh_calls(&self) -> Vec<Vec<String>> {
        self.env.calls("gh")
    }

    fn gate_log(&self) -> String {
        fs::read_to_string(self.home.plan_dir(&plan()).join("logs/pr.pre-merge.log")).unwrap_or_default()
    }

    /// Moves `main` on `origin` (through the main checkout) with a commit writing `path`.
    fn advance_base(&self, path: &str, content: &str) -> String {
        fs::write(self.repo.join(path), content).unwrap();
        self.git(&self.repo, &["add", "-A"]);
        self.git(&self.repo, &["commit", "-q", "-m", &format!("base {path}")]);
        self.git(&self.repo, &["push", "-q", "origin", "main"]);
        self.git(&self.repo, &["rev-parse", "HEAD"])
    }
}

fn code(run: &Run) -> (String, i32) {
    (run.error()["code"].as_str().unwrap().to_owned(), run.exit)
}

// ------------------------------------------------------------------ pr open

#[test]
fn open_gates_pushes_creates_the_pr_and_records_it() {
    let fx = fx("dispatch");
    fx.hooks("pre-merge = \"echo gate ran on $(git rev-parse --abbrev-ref HEAD)\"\n");

    let out = fx.pr("open").ok();

    let opened_at = out["pr"]["opened_at"].as_str().unwrap().to_owned();
    assert_eq!(out, json!({"ok": true, "plan": "pay", "phase": "pr", "pr": {"number": 812, "url": URL, "opened_at": opened_at}}));
    // The Gate ran in the Lead worktree on the Lead Branch, and wrote its log.
    assert!(fx.gate_log().contains("gate ran on cpd/pay"), "{}", fx.gate_log());
    // Pushed.
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lead_sha()));
    assert_eq!(fx.git(&fx.lead, &["rev-parse", "--abbrev-ref", "cpd/pay@{upstream}"]), "origin/cpd/pay");
    // gh: look for an existing PR, then create it with the Jobs table as its body.
    let calls = fx.gh_calls();
    assert_eq!(calls[0], ["pr", "list", "--head", "cpd/pay", "--state", "open", "--json", "number,url"]);
    assert_eq!(calls[1][..8], ["pr", "create", "--base", "main", "--head", "cpd/pay", "--title", "Pay the bills"]);
    let body = &calls[1][9];
    assert_eq!(calls[1][8], "--body");
    assert!(body.contains("| api | The \\| API | merged | `"), "{body}");
    assert!(body.contains(&fx.merged_sha), "{body}");
    // Rows and the Event, in one transaction.
    let row = fx.plan_row();
    assert_eq!((row.0.as_str(), row.1, row.2.as_deref(), row.3.as_deref()), ("pr", Some(812), Some(URL), Some(opened_at.as_str())));
    let events = fx.events("plan-phase");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1, json!({"from": "dispatch", "to": "pr", "pr": {"number": 812, "url": URL}}));
    assert!(events[0].0.contains("cpd2 pr poll --plan pay"), "{}", events[0].0);
}

#[test]
fn open_with_no_hooks_passes_the_gate() {
    let fx = fx("dispatch");
    fx.pr("open").ok();
    assert_eq!(fx.gate_log(), "");
}

#[test]
fn open_adopts_a_pr_an_earlier_run_created() {
    let fx = fx("dispatch");
    fx.gh("list.json", r#"[{"number": 812, "url": "https://github.com/acme/pay/pull/812"}]"#);

    let out = fx.pr("open").ok();

    assert_eq!(out["pr"]["number"], 812);
    assert!(fx.gh_calls().iter().all(|c| c[1] != "create"), "{:?}", fx.gh_calls());
    assert_eq!(fx.plan_row().0, "pr");
}

#[test]
fn open_again_is_already_and_touches_nothing() {
    let fx = fx("dispatch");
    let first = fx.pr("open").ok();
    let calls = fx.gh_calls().len();
    let before = fx.plan_row();

    let again = fx.pr("open").ok();

    assert_eq!(again, json!({"ok": true, "plan": "pay", "phase": "pr", "pr": first["pr"], "already": true}));
    assert_eq!(fx.gh_calls().len(), calls);
    assert_eq!(fx.plan_row(), before);
    assert_eq!(fx.events("plan-phase").len(), 1);
}

#[test]
fn open_on_a_done_plan_is_already() {
    let fx = fx("done");
    let out = fx.pr("open").ok();
    assert_eq!(out["phase"], "done");
    assert_eq!(out["already"], true);
    assert_eq!(out["pr"]["number"], 812);
}

#[test]
fn open_refuses_a_phase_before_dispatch() {
    let fx = fx("plan");
    let run = fx.pr("open");
    assert_eq!(code(&run), ("illegal_phase".into(), 3));
    assert_eq!(run.error()["details"]["phase"], "plan");
}

#[test]
fn open_refuses_while_a_job_is_not_merged_or_cancelled() {
    let fx = fx("dispatch");
    fx.store()
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO jobs (plan, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, created_txn, created_at)
                 VALUES ('pay', 'ui', 'UI', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 0, ?1, ?2, 't')",
                params!["f".repeat(64), tx.id()],
            )?;
            Ok(())
        })
        .unwrap();

    let run = fx.pr("open");

    assert_eq!(code(&run), ("jobs_not_done".into(), 3));
    assert_eq!(run.error()["details"]["jobs"], json!([{"key": "ui", "status": "queued"}]));
    assert!(fx.gh_calls().is_empty());
    assert_eq!(fx.plan_row().0, "dispatch");
}

#[test]
fn open_refuses_when_no_job_is_merged() {
    let fx = fx_full("dispatch", "", "cancelled");
    let run = fx.pr("open");
    assert_eq!(code(&run), ("jobs_not_done".into(), 3));
    assert_eq!(run.error()["details"]["jobs"], json!([]));
    assert_eq!(fx.plan_row().0, "dispatch");
}

#[test]
fn open_times_out_waiting_for_the_per_repo_lock() {
    let fx = fx("dispatch");
    let _held = RepoLock::wait(&fx.home, &fx.repo).unwrap();

    let run = fx.pr("open");

    assert_eq!(code(&run), ("lock_timeout".into(), 5));
    assert_eq!(fx.remote_branch("cpd/pay"), None);
    assert_eq!(fx.plan_row().0, "dispatch");
}

#[test]
fn open_refuses_a_dirty_lead_worktree() {
    let fx = fx("dispatch");
    fs::write(fx.lead.join("scratch.txt"), "x").unwrap();

    let run = fx.pr("open");

    assert_eq!(code(&run), ("dirty_target".into(), 3));
    assert_eq!(run.error()["details"]["files"], json!(["?? scratch.txt"]));
    assert_eq!(fx.remote_branch("cpd/pay"), None);
}

#[test]
fn open_refuses_a_failed_gate_and_pushes_nothing() {
    let fx = fx("dispatch");
    fx.hooks("pre-merge = \"echo the suite is red; exit 3\"\n");

    let run = fx.pr("open");

    assert_eq!(code(&run), ("gate_failed".into(), 3));
    let details = &run.error()["details"];
    assert_eq!(details["exit"], 3);
    assert!(details["tail"].as_str().unwrap().contains("the suite is red"), "{details}");
    assert_eq!(details["log"], fx.home.plan_dir(&plan()).join("logs/pr.pre-merge.log").to_str().unwrap());
    assert!(fx.gate_log().contains("the suite is red"));
    // Nothing pushed, no PR, no row: the phase stays dispatch.
    assert_eq!(fx.remote_branch("cpd/pay"), None);
    assert!(fx.gh_calls().is_empty());
    let row = fx.plan_row();
    assert_eq!((row.0.as_str(), row.1), ("dispatch", None));
    assert!(fx.events("plan-phase").is_empty());
}

#[test]
fn open_reports_a_gate_that_could_not_be_judged_as_gate_error() {
    let fx = fx("dispatch");
    // An undefined template variable: `hook_invalid`, so nothing ran.
    fx.hooks("pre-merge = \"echo {{ no_such_variable }}\"\n");

    let run = fx.pr("open");

    assert_eq!(code(&run), ("gate_error".into(), 1));
    let details = &run.error()["details"];
    assert!(details["reason"].as_str().unwrap().contains("hook_invalid"), "{details}");
    assert_eq!(details["tail"], "");
    assert!(details["log"].as_str().unwrap().ends_with("logs/pr.pre-merge.log"));
    assert_eq!(fx.remote_branch("cpd/pay"), None);
    assert_eq!(fx.plan_row().0, "dispatch");
}

#[test]
fn open_reports_a_failing_gh_as_gh_failed() {
    let fx = fx("dispatch");
    fx.gh("fail", "HTTP 401: bad credentials");

    let run = fx.pr("open");

    assert_eq!(code(&run), ("gh_failed".into(), 1));
    let details = &run.error()["details"];
    assert_eq!(details["argv"].as_array().unwrap()[..3], [json!("gh"), json!("pr"), json!("list")]);
    assert_eq!(details["exit"], 1);
    assert!(details["stderr"].as_str().unwrap().contains("bad credentials"), "{details}");
    assert_eq!(fx.plan_row().0, "dispatch");
}

#[test]
fn open_rerun_after_gh_failed_finishes() {
    let fx = fx("dispatch");
    fx.gh("fail", "network down");
    assert_eq!(code(&fx.pr("open")).0, "gh_failed");
    fs::remove_file(fx.env.root().join("gh/fail")).unwrap();

    fx.pr("open").ok();

    assert_eq!(fx.plan_row().0, "pr");
}

#[test]
fn open_without_a_selected_plan_is_plan_required() {
    let fx = fx("dispatch");
    let run = fx.env.cpd2(&["pr", "open"]);
    assert_eq!(code(&run), ("plan_required".into(), 2));
}

#[test]
fn open_an_unknown_plan_is_plan_not_found() {
    let fx = fx("dispatch");
    let run = fx.env.cpd2(&["pr", "open", "--plan", "nope"]);
    assert_eq!(code(&run), ("plan_not_found".into(), 4));
}

// -------------------------------------------------------------- R before the Gate

const LEVELS: &str = r#"{"O":["off","low","medium","high","xhigh","max"],"S":["off","low","medium"],"H":["off","low","medium"]}"#;

/// A pi install whose catalog module a fake `node` answers: R reads pi's catalog.
fn install_pi(env: &TestEnv) {
    env.fake_pi("exit 0");
    fs::create_dir_all(env.root().join("install")).unwrap();
    fs::write(env.root().join("install/current-version"), "1.2.0\n").unwrap();
    let scope = env.root().join("install/releases/1.2.0/node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    env.fake("node", &format!("echo '{LEVELS}'"));
}

/// Plan `oth` on the same repo whose driver died right after its merge's
/// compare-and-swap (§7.4 step 8): the Attempt is `merging`, its commit is on
/// `cpd/oth`, and the Ledger knows nothing of it.
fn orphaned_merge(fx: &Fx) -> (PathBuf, PathBuf) {
    let oth = PlanId::parse("oth").unwrap();
    let setup = Setup { timeout: Duration::from_secs(30), log: fx.env.root().join("oth.log") };
    let lead = create_lead_worktree(&fx.repo, &oth, "main", &setup).unwrap().path;
    let attempt = AttemptId::parse("x-a1").unwrap();
    let made = create_attempt_worktree(&fx.repo, &lead, &oth, &attempt, &setup).unwrap();
    fs::write(made.path.join("x.txt"), "x\n").unwrap();
    fx.git(&made.path, &["add", "-A"]);
    fx.git(&made.path, &["commit", "-q", "-m", "x"]);
    let tip = fx.git(&made.path, &["rev-parse", "HEAD"]);
    fx.store()
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead2', 'lead', 'ctx', 'oth', 'Lead', '/', 'O3', 'idle', 't', 't');
                 INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session, created_at, locked_at)
                   VALUES ('oth', 'ctx', 'Other', '{repo}', 'main', 'cpd/oth', '{lead}', 'dispatch', 'lead2', 't', 't');",
                repo = fx.repo.display(),
                lead = lead.display()
            ))?;
            tx.execute(
                "INSERT INTO jobs (plan, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, created_txn, created_at)
                 VALUES ('oth', 'x', 'X', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, ?1, ?2, 't')",
                params!["f".repeat(64), tx.id()],
            )?;
            tx.execute(
                "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha,
                                       submitted_sha, submitted_at, started_at)
                 VALUES ('oth', 'x-a1', 'x', 1, 'S2', 'ladder', 'merging', ?1, ?2, ?3, ?4, 't', 't')",
                params![made.branch, made.path.to_str().unwrap(), made.base_sha, tip],
            )?;
            Ok(())
        })
        .unwrap();
    fx.git(&fx.repo, &["update-ref", "refs/heads/cpd/oth", &tip, &made.base_sha]);
    (lead, made.path)
}

#[test]
fn open_and_rebase_run_r_for_the_repos_orphaned_merges_first() {
    for command in ["open", "rebase"] {
        let fx = fx(if command == "open" { "dispatch" } else { "pr" });
        install_pi(&fx.env);
        let (oth_lead, attempt_worktree) = orphaned_merge(&fx);

        fx.pr(command).ok();

        let (status, outcome, merged): (String, Option<String>, Option<String>) = fx
            .store()
            .read(|c| {
                Ok(c.query_row("SELECT status, outcome, merged_sha FROM attempts WHERE id = 'x-a1'", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?)
            })
            .unwrap();
        assert_eq!((status.as_str(), outcome.as_deref()), ("ended", Some("merged")), "{command}");
        assert_eq!(merged.unwrap(), fx.git(&fx.repo, &["rev-parse", "cpd/oth"]));
        let job: String = fx.store().read(|c| Ok(c.query_row("SELECT status FROM jobs WHERE plan = 'oth'", [], |r| r.get(0))?)).unwrap();
        assert_eq!(job, "merged");
        // R's rest: the Attempt's worktree is gone, the stale Lead worktree synced.
        assert!(!attempt_worktree.exists());
        assert_eq!(fs::read_to_string(oth_lead.join("x.txt")).unwrap(), "x\n");
        let source: String = fx
            .store()
            .read(|c| Ok(c.query_row("SELECT t.command FROM events e JOIN txns t ON t.id = e.txn WHERE e.kind = 'job-merged'", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(source, format!("pr {command}"));
    }
}

#[test]
fn open_does_not_need_pi_when_no_driver_is_dead() {
    // An Attempt in `merging` whose driver is alive is its driver's to finish: R skips it
    // without asking pi for a catalog (this pi has no install, so asking would fail).
    let fx = fx("dispatch");
    fx.env.fake_pi("exit 1");
    let (_, _) = orphaned_merge(&fx);
    let attempt = AttemptId::parse("x-a1").unwrap();
    let _driver = hold_driver_lock(&fx, &attempt);

    fx.pr("open").ok();

    let status: String = fx.store().read(|c| Ok(c.query_row("SELECT status FROM attempts WHERE id = 'x-a1'", [], |r| r.get(0))?)).unwrap();
    assert_eq!(status, "merging");
}

fn hold_driver_lock(fx: &Fx, attempt: &AttemptId) -> cpd2::merge::DriverLock {
    let oth = PlanId::parse("oth").unwrap();
    cpd2::merge::take_free_driver_lock(&fx.home, &oth, attempt).unwrap().expect("the driver lock is free")
}

// ------------------------------------------------------------------ pr poll

fn feedback_view() -> &'static str {
    r#"{"state":"OPEN","mergedAt":null,
        "title":"Add the payment webhook","reviewDecision":"",
        "statusCheckRollup":[
          {"name":"ci/test","conclusion":"FAILURE"},
          {"name":"lint","conclusion":"SUCCESS"},
          {"context":"legacy","state":"ERROR"}],
        "reviews":[{"id":"PRR_1","author":{"login":"ann"},"state":"CHANGES_REQUESTED","body":"Please fix"}],
        "comments":[{"id":"IC_1","author":{"login":"bob"},"body":"nit"}]}"#
}

#[test]
fn poll_reports_new_feedback_once() {
    let fx = fx("pr");
    fx.gh("view.json", feedback_view());
    fx.gh("inline.json", r#"[{"id": 77, "user": {"login": "cy"}, "path": "src/a.rs", "line": 3, "body": "why?"}]"#);

    let out = fx.pr("poll").ok();

    let changes = json!({
        "checks": [{"name": "ci/test", "conclusion": "FAILURE"}, {"name": "legacy", "conclusion": "ERROR"}],
        "reviews": [{"id": "PRR_1", "author": "ann", "state": "CHANGES_REQUESTED", "body": "Please fix"}],
        "comments": [{"id": "IC_1", "author": "bob", "body": "nit"}, {"id": "77", "author": "cy", "path": "src/a.rs", "body": "why?"}],
    });
    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "state": "OPEN", "merged": false, "title": TITLE, "review_decision": "none", "checks": "failed", "changes": changes})
    );
    // gh: the view, then the inline comments of the same PR.
    let calls = fx.gh_calls();
    assert_eq!(calls[0], ["pr", "view", "812", "--json", "state,mergedAt,statusCheckRollup,reviews,comments,title,reviewDecision"]);
    assert_eq!(calls[1], ["api", "repos/{owner}/{repo}/pulls/812/comments"]);
    // pr_seen, pr_polled_at, and one pr-feedback Event.
    let row = fx.plan_row();
    assert_eq!(
        row.4,
        json!({
            "checks": {"ci/test": "FAILURE", "legacy": "ERROR", "lint": "SUCCESS"},
            "reviewIds": ["PRR_1"],
            "commentIds": ["IC_1", "77"],
        })
    );
    assert!(row.5.is_some());
    assert_eq!(row.0, "pr");
    let events = fx.events("pr-feedback");
    assert_eq!(events.len(), 1);
    let mut data = changes;
    data["state"] = json!("OPEN");
    assert_eq!(events[0].1, data);
    assert!(events[0].0.contains("cpd2 job add"), "{}", events[0].0);

    // A rerun reports nothing new, and appends nothing.
    let again = fx.pr("poll").ok();
    assert_eq!(again["changes"], json!({"checks": [], "reviews": [], "comments": []}));
    assert_eq!(fx.events("pr-feedback").len(), 1);
    assert_eq!(fx.plan_row().4, row.4);

    // Only what is new afterwards.
    fx.gh("inline.json", r#"[{"id": 77, "user": {"login": "cy"}, "path": "src/a.rs", "body": "why?"}, {"id": 78, "user": null, "body": "and?"}]"#);
    let more = fx.pr("poll").ok();
    assert_eq!(more["changes"]["comments"], json!([{"id": "78", "author": null, "body": "and?"}]));
    assert_eq!(more["changes"]["checks"], json!([]));
    assert_eq!(fx.events("pr-feedback").len(), 2);
}

#[test]
fn poll_stores_the_title_the_review_decision_and_the_checks_on_every_poll() {
    let fx = fx("pr");
    assert_eq!(fx.pr_data(), None, "all null until the first poll");
    fx.gh("view.json", feedback_view());
    fx.pr("poll").ok();
    assert_eq!(fx.pr_data(), Some((TITLE.into(), "none".into(), "failed".into())));

    // The PR is renamed, approved and its checks pass: the next poll replaces all three, and reports nothing new.
    fx.gh(
        "view.json",
        r#"{"state":"OPEN","mergedAt":null,"title":"Add the payment webhook v2","reviewDecision":"APPROVED",
            "statusCheckRollup":[{"name":"ci/test","conclusion":"SUCCESS"}],"reviews":[],"comments":[]}"#,
    );
    let out = fx.pr("poll").ok();
    assert_eq!((out["title"].as_str(), out["review_decision"].as_str(), out["checks"].as_str()), (Some("Add the payment webhook v2"), Some("approved"), Some("passed")));
    assert_eq!(fx.pr_data(), Some(("Add the payment webhook v2".into(), "approved".into(), "passed".into())));
    // They belong to the poll: no Event of their own.
    assert_eq!(fx.events("pr-feedback").len(), 1);
}

#[test]
fn poll_maps_gh_review_decisions_and_check_conclusions() {
    let fx = fx("pr");
    let view = |decision: &str, checks: &str| {
        format!(
            r#"{{"state":"OPEN","mergedAt":null,"title":"T","reviewDecision":{decision},"statusCheckRollup":{checks},"reviews":[],"comments":[]}}"#
        )
    };
    let ci = |conclusion: &str| format!(r#"{{"name":"ci","conclusion":"{conclusion}"}}"#);
    let cases = [
        // (reviewDecision, checks, stored decision, stored checks)
        (r#""APPROVED""#, "[]".to_owned(), "approved", "none"),
        (r#""CHANGES_REQUESTED""#, "null".to_owned(), "changes_requested", "none"),
        (r#""REVIEW_REQUIRED""#, format!("[{}]", ci("SUCCESS")), "none", "passed"),
        (r#""""#, format!("[{}]", ci("")), "none", "pending"),
        ("null", format!("[{}]", ci("IN_PROGRESS")), "none", "pending"),
        (r#""APPROVED""#, format!("[{}]", ci("timed_out")), "approved", "failed"),
        // A StatusContext has `state`, not `conclusion`.
        ("null", r#"[{"context":"legacy","state":"PENDING"}]"#.to_owned(), "none", "pending"),
        // Failure wins over pending, pending over passed.
        (
            "null",
            format!(r#"[{},{{"name":"b","conclusion":"QUEUED"}},{{"name":"c","conclusion":"FAILURE"}}]"#, ci("SUCCESS")),
            "none",
            "failed",
        ),
        (
            "null",
            format!(r#"[{},{{"name":"b","conclusion":"QUEUED"}}]"#, ci("SUCCESS")),
            "none",
            "pending",
        ),
        ("null", format!(r#"[{},{{"name":"b","conclusion":"NEUTRAL"}}]"#, ci("SUCCESS")), "none", "passed"),
    ];
    for (decision, checks, want_decision, want_checks) in cases {
        fx.gh("view.json", &view(decision, &checks));
        let out = fx.pr("poll").ok();
        assert_eq!((out["review_decision"].as_str(), out["checks"].as_str()), (Some(want_decision), Some(want_checks)), "{decision} {checks}");
        assert_eq!(fx.pr_data(), Some(("T".into(), want_decision.into(), want_checks.into())), "{decision} {checks}");
    }
}

#[test]
fn poll_refuses_a_view_without_a_title_and_stores_nothing() {
    let fx = fx("pr");
    fx.gh("view.json", r#"{"state":"OPEN","mergedAt":null,"statusCheckRollup":[],"reviews":[],"comments":[]}"#);
    let run = fx.pr("poll");
    assert_eq!(code(&run), ("gh_failed".into(), 1));
    assert_eq!((fx.pr_data(), fx.plan_row().5), (None, None));
}

#[test]
fn poll_with_nothing_new_still_records_the_poll() {
    let fx = fx("pr");

    let out = fx.pr("poll").ok();

    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "state": "OPEN", "merged": false, "title": TITLE, "review_decision": "none", "checks": "none",
               "changes": {"checks": [], "reviews": [], "comments": []}})
    );
    assert!(fx.plan_row().5.is_some());
    assert!(fx.events("pr-feedback").is_empty());
}

#[test]
fn poll_reports_a_closure_without_a_merge_once_until_the_pr_is_reopened() {
    let fx = fx("pr");
    fx.gh("view.json", r#"{"state":"CLOSED","mergedAt":null,"title":"Add the payment webhook","reviewDecision":"","statusCheckRollup":null,"reviews":[],"comments":[]}"#);

    let out = fx.pr("poll").ok();

    assert_eq!(out["state"], "CLOSED");
    assert_eq!(out["merged"], false);
    let events = fx.events("pr-feedback");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1, json!({"checks": [], "reviews": [], "comments": [], "state": "CLOSED"}));
    assert_eq!(fx.plan_row().4["closed"], true);
    assert_eq!(fx.plan_row().0, "pr");

    fx.pr("poll").ok();
    assert_eq!(fx.events("pr-feedback").len(), 1, "a closure is reported once");

    // Reopened: the mark goes, and nothing is reported.
    fx.gh("view.json", OPEN_VIEW);
    fx.pr("poll").ok();
    assert!(fx.plan_row().4.get("closed").is_none(), "{}", fx.plan_row().4);
    assert_eq!(fx.events("pr-feedback").len(), 1);

    // Closed again: reported again.
    fx.gh("view.json", r#"{"state":"CLOSED","mergedAt":null,"title":"Add the payment webhook","reviewDecision":"","statusCheckRollup":[],"reviews":[],"comments":[]}"#);
    fx.pr("poll").ok();
    assert_eq!(fx.events("pr-feedback").len(), 2);
}

/// Attempt `b-a1` of a cancelled Job `b`, ended, with its worktree and branch kept for
/// debugging (§3.4 A9), and a tmux window for it.
fn kept_attempt(fx: &Fx) -> (PathBuf, String) {
    let attempt = AttemptId::parse("b-a1").unwrap();
    let setup = Setup { timeout: Duration::from_secs(30), log: fx.env.root().join("attempt.log") };
    let made = create_attempt_worktree(&fx.repo, &fx.lead, &plan(), &attempt, &setup).unwrap();
    fx.store()
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO jobs (plan, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, cancel_reason, cancel_requested_at, created_txn, created_at)
                 VALUES ('pay', 'b', 'B', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'cancelled', 0, ?1, 'no', 't', ?2, 't')",
                params!["f".repeat(64), tx.id()],
            )?;
            tx.execute(
                "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, base_sha,
                                       started_at, ended_at)
                 VALUES ('pay', 'b-a1', 'b', 1, 'S2', 'ladder', 'ended', 'cancelled', 'cancel_requested', ?1, ?2, ?3, 't', 't')",
                params![made.branch, made.path.to_str().unwrap(), made.base_sha],
            )?;
            Ok(())
        })
        .unwrap();
    (made.path, made.branch)
}

fn window_names(fx: &Fx) -> String {
    let out = fx.env.tmux(&["list-windows", "-a", "-F", "#{session_name}:#{window_name}"]);
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn poll_of_a_merged_pr_ends_the_plan_and_removes_its_attempt_leftovers() {
    let fx = fx("pr");
    let (worktree, branch) = kept_attempt(&fx);
    let started = fx.env.tmux(&["new-session", "-d", "-s", "cpd-work", "-n", "pay--b-a1", "sleep 300"]);
    assert!(started.status.success(), "{started:?}");
    assert!(fx.env.tmux(&["new-window", "-d", "-t", "cpd-work", "-n", "other-plan", "sleep 300"]).status.success());
    fx.gh("view.json", r#"{"state":"MERGED","mergedAt":"2026-10-14T10:00:00Z","title":"Add the payment webhook","reviewDecision":"","statusCheckRollup":[],"reviews":[],"comments":[]}"#);

    let out = fx.pr("poll").ok();

    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "state": "MERGED", "merged": true, "title": TITLE, "review_decision": "none", "checks": "none",
               "changes": {"checks": [], "reviews": [], "comments": []}})
    );
    assert_eq!(fx.pr_data(), Some((TITLE.into(), "none".into(), "none".into())));
    let row = fx.plan_row();
    assert_eq!(row.0, "done");
    assert!(row.6.is_some() && row.5.is_some());
    let done = fx.events("plan-done");
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].1, json!({"pr": {"number": 812, "url": URL}, "merged_at": "2026-10-14T10:00:00Z"}));
    assert!(fx.events("pr-feedback").is_empty());
    // The Attempt's worktree and branch are gone, its window is killed; the Lead's stay.
    assert!(!worktree.exists());
    assert_eq!(fx.git(&fx.repo, &["branch", "--list", &branch]), "");
    assert!(fx.lead.exists());
    assert_eq!(fx.git(&fx.repo, &["branch", "--list", "cpd/pay"]), "+ cpd/pay");
    let windows = window_names(&fx);
    assert!(!windows.contains("pay--b-a1") && windows.contains("cpd-work:other-plan"), "{windows}");

    // Done: a rerun is `already`.
    let again = fx.pr("poll").ok();
    assert_eq!(again, json!({"ok": true, "plan": "pay", "phase": "done", "merged": true, "already": true}));
    assert_eq!(fx.events("plan-done").len(), 1);
}

#[test]
fn poll_refuses_a_plan_that_is_not_in_phase_pr() {
    let fx = fx("dispatch");
    let run = fx.pr("poll");
    assert_eq!(code(&run), ("illegal_phase".into(), 3));
    assert_eq!(run.error()["details"]["phase"], "dispatch");
    assert!(fx.gh_calls().is_empty());
}

#[test]
fn poll_on_a_done_plan_is_already() {
    let fx = fx("done");
    let out = fx.pr("poll").ok();
    assert_eq!(out, json!({"ok": true, "plan": "pay", "phase": "done", "merged": true, "already": true}));
    assert!(fx.gh_calls().is_empty());
}

#[test]
fn poll_reports_a_failing_gh_and_changes_nothing() {
    let fx = fx("pr");
    fx.gh("fail", "rate limited");

    let run = fx.pr("poll");

    assert_eq!(code(&run), ("gh_failed".into(), 1));
    assert!(run.error()["details"]["stderr"].as_str().unwrap().contains("rate limited"));
    assert_eq!(fx.plan_row().5, None);
}

#[test]
fn poll_refuses_gh_output_it_cannot_parse() {
    let fx = fx("pr");
    fx.gh("view.json", r#"{"state": 3}"#);
    let run = fx.pr("poll");
    assert_eq!(code(&run), ("gh_failed".into(), 1));
    assert_eq!(fx.plan_row().5, None);
}

// ---------------------------------------------------------------- pr rebase

#[test]
fn rebase_onto_a_moved_base_gates_and_force_pushes_with_a_lease() {
    let fx = fx("pr");
    fx.hooks("pre-merge = \"git rev-parse HEAD\"\n");
    let old = fx.lead_sha();
    let base = fx.advance_base("base.txt", "base\n");

    let out = fx.pr("rebase").ok();

    let new = fx.lead_sha();
    assert_ne!(new, old);
    assert_eq!(out, json!({"ok": true, "plan": "pay", "rebased": true, "old": old, "new": new, "pushed": true}));
    assert!(fx.git(&fx.repo, &["merge-base", "--is-ancestor", &base, &new]).is_empty());
    // The Lead worktree is on the rebased branch and clean.
    assert_eq!(fs::read_to_string(fx.lead.join("base.txt")).unwrap(), "base\n");
    assert_eq!(fx.git(&fx.lead, &["status", "--porcelain"]), "");
    // The Gate ran after the rebase; the push followed it.
    assert_eq!(fx.gate_log().trim(), new);
    assert_eq!(fx.remote_branch("cpd/pay"), Some(new));
    // No Ledger row, no Event.
    assert!(fx.events("plan-phase").is_empty());
    assert_eq!(fx.plan_row().0, "pr");
}

#[test]
fn rebase_with_the_base_unmoved_only_gates_and_pushes() {
    let fx = fx("pr");
    // A Job Merged in phase pr: on the Lead Branch, not yet on the PR.
    fs::write(fx.lead.join("more.txt"), "more\n").unwrap();
    fx.git(&fx.lead, &["add", "-A"]);
    fx.git(&fx.lead, &["commit", "-q", "-m", "more"]);
    let tip = fx.lead_sha();
    assert_ne!(fx.remote_branch("cpd/pay"), Some(tip.clone()));

    let out = fx.pr("rebase").ok();

    assert_eq!(out, json!({"ok": true, "plan": "pay", "rebased": false, "old": tip, "new": tip, "pushed": true}));
    assert_eq!(fx.remote_branch("cpd/pay"), Some(tip));
    // Idempotent: again, the same.
    assert_eq!(fx.pr("rebase").ok()["rebased"], false);
}

#[test]
fn rebase_refuses_a_conflict_and_changes_nothing() {
    let fx = fx("pr");
    fs::write(fx.lead.join("README.md"), "lead\n").unwrap();
    fx.git(&fx.lead, &["add", "-A"]);
    fx.git(&fx.lead, &["commit", "-q", "-m", "lead readme"]);
    fx.git(&fx.lead, &["push", "-q", "origin", "cpd/pay"]);
    fx.advance_base("README.md", "base\n");
    let before = fx.lead_sha();

    let run = fx.pr("rebase");

    assert_eq!(code(&run), ("rebase_conflict".into(), 3));
    assert_eq!(run.error()["details"]["files"], json!(["README.md"]));
    assert_eq!(fx.lead_sha(), before);
    assert_eq!(fx.git(&fx.lead, &["status", "--porcelain"]), "");
    assert_eq!(fx.remote_branch("cpd/pay"), Some(before));
    let rebasing = fx.git(&fx.lead, &["rev-parse", "--git-path", "rebase-merge"]);
    assert!(!fx.lead.join(&rebasing).exists() && !Path::new(&rebasing).exists());
    assert!(fx.gh_calls().is_empty());
}

#[test]
fn rebase_refuses_a_phase_other_than_pr() {
    for phase in ["dispatch", "done"] {
        let fx = fx(phase);
        let run = fx.pr("rebase");
        assert_eq!(code(&run), ("illegal_phase".into(), 3), "{phase}");
        assert_eq!(run.error()["details"]["phase"], phase);
    }
}

#[test]
fn rebase_times_out_waiting_for_the_per_repo_lock() {
    let fx = fx("pr");
    let _held = RepoLock::wait(&fx.home, &fx.repo).unwrap();
    assert_eq!(code(&fx.pr("rebase")), ("lock_timeout".into(), 5));
}

#[test]
fn rebase_refuses_a_dirty_lead_worktree() {
    let fx = fx("pr");
    fs::write(fx.lead.join("api.txt"), "edited\n").unwrap();
    let run = fx.pr("rebase");
    assert_eq!(code(&run), ("dirty_target".into(), 3));
    assert_eq!(run.error()["details"]["files"], json!(["M api.txt"]));
}

#[test]
fn rebase_with_a_failing_gate_keeps_the_rebased_branch_and_pushes_nothing() {
    let fx = fx("pr");
    fx.hooks("pre-merge = \"echo red; exit 1\"\n");
    fx.git(&fx.lead, &["push", "-q", "origin", "cpd/pay"]);
    let pushed = fx.remote_branch("cpd/pay");
    let old = fx.lead_sha();
    fx.advance_base("base.txt", "base\n");

    let run = fx.pr("rebase");

    assert_eq!(code(&run), ("gate_failed".into(), 3));
    let details = &run.error()["details"];
    assert!(details["tail"].as_str().unwrap().contains("red"));
    assert!(details["log"].as_str().unwrap().ends_with("logs/pr.pre-merge.log"));
    // The rebased Lead Branch stays; the remote does not move.
    assert_ne!(fx.lead_sha(), old);
    assert!(fx.lead.join("base.txt").exists());
    assert_eq!(fx.remote_branch("cpd/pay"), pushed);
}

#[test]
fn rebase_with_a_gate_that_cannot_be_judged_is_gate_error() {
    let fx = fx("pr");
    fx.hooks("pre-merge = \"echo {{ no_such_variable }}\"\n");
    fx.git(&fx.lead, &["push", "-q", "origin", "cpd/pay"]);
    let pushed = fx.remote_branch("cpd/pay");

    let run = fx.pr("rebase");

    assert_eq!(code(&run), ("gate_error".into(), 1));
    assert_eq!(fx.remote_branch("cpd/pay"), pushed);
}

#[test]
fn rebase_does_not_overwrite_a_push_it_has_not_seen() {
    let fx = fx("pr");
    // Someone else pushes to the PR's branch.
    let clone = fx.env.root().join("repos/clone");
    fx.git(fx.env.root(), &["clone", "-q", fx.origin.to_str().unwrap(), clone.to_str().unwrap()]);
    fx.git(&clone, &["checkout", "-q", "cpd/pay"]);
    fs::write(clone.join("theirs.txt"), "theirs\n").unwrap();
    fx.git(&clone, &["add", "-A"]);
    fx.git(&clone, &["commit", "-q", "-m", "theirs"]);
    fx.git(&clone, &["push", "-q", "origin", "cpd/pay"]);
    let theirs = fx.remote_branch("cpd/pay");

    let run = fx.pr("rebase");

    assert_eq!(code(&run), ("git_failed".into(), 1));
    assert_eq!(fx.remote_branch("cpd/pay"), theirs);
}
