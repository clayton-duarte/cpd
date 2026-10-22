//! `pr open`, `pr poll` and `pr rebase` (commands.md §9, contract §7.6): the
//! built binary against temporary repos with a bare `origin`, a fake `gh`, and
//! a Plan written straight into the database. `pr open` and `pr rebase` take no
//! lock (hs-d243): their Gate runs in a gate worktree of its own.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use common::{CPD, Run, SLEEP_SECONDS, TestEnv, wait_until};
use cpd_core::git::{Setup, create_attempt_worktree, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::store::Store;
use nix::fcntl::{Flock, FlockArg};
use cpd_tui::read::{self, Checks, PrState, ReviewDecision};
use rusqlite::params;
use serde_json::{Value, json};

const URL: &str = "https://github.com/acme/pay/pull/812";

/// The fake `gh`: each call is answered from a file in `gh/`, or, when the directory
/// `gh/<name of the working directory>/` has that file, from it (a PR's Lead worktree
/// answers for its PR); `gh/fail`, when present, makes every call fail with its text on stderr.
const GH: &str = r#"
dir="$(dirname "$0")/../gh"
here="$dir/$(basename "$PWD")"
file() { if [ -e "$here/$1" ]; then echo "$here/$1"; else echo "$dir/$1"; fi; }
if [ -e "$dir/fail" ]; then cat "$dir/fail" >&2; exit 1; fi
case "$1 $2" in
  "pr list") cat "$(file list.json)" ;;
  "pr create") cat "$(file create.out)" ;;
  "api graphql")
    case "$4" in
      *"search(type: ISSUE"*) cat "$(file search.json)" ;;
      *"p0: repository"*) cat "$(file lookup.json)"; if [ -e "$dir/lookup.exit" ]; then exit "$(cat "$dir/lookup.exit")"; fi ;;
      *) echo "unexpected graphql query: $4" >&2; exit 9 ;;
    esac ;;
  *) echo "unexpected gh call: $*" >&2; exit 9 ;;
esac
"#;

const WEB_URL: &str = "https://github.com/acme/web/pull/813";

const TITLE: &str = "Add the payment webhook";

/// The stored title of PR `repo`, and of PR `web` (hs-d285): neither is the Plan's title, `Pay the bills`.
const REPO_TITLE: &str = "Charge the card at checkout";
const WEB_PR_TITLE: &str = "Show the receipt";

const EMPTY_SEARCH: &str = r#"{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}"#;

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
    /// What the lock pushed to `cpd/pay` on origin (R35, §7.2): the base.
    lock_pushed: String,
    /// The Lead Branch of PR `repo`: `cpd/pay`, or `cpd/pay/repo` where a second PR of the Plan exists (`fx_stacked`).
    branch: String,
}

fn fx(phase: &str) -> Fx {
    fx_with(phase, "")
}

fn fx_with(phase: &str, config_extra: &str) -> Fx {
    fx_full(phase, config_extra, "merged")
}

/// `job` is the status of the Plan's one Job, `merged` or `cancelled`.
fn fx_full(phase: &str, config_extra: &str, job: &str) -> Fx {
    fx_branch(phase, config_extra, job, "cpd/pay")
}

/// A Plan whose PR `repo` has the Lead Branch `cpd/pay/repo`: git cannot hold `cpd/pay` and `cpd/pay/web` both.
fn fx_stacked(phase: &str) -> Fx {
    fx_branch(phase, "", "merged", "cpd/pay/repo")
}

fn fx_branch(phase: &str, config_extra: &str, job: &str, branch: &str) -> Fx {
    let env = TestEnv::new();
    env.owner_policy("acme", "[pr]\ngate = \"local\"\n");
    let repo = env.keyed_repo("pay").canonicalize().unwrap();
    let origin = env.root().join("repos/origin.git");
    env.git(env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
    env.local_origin(&repo, &origin);
    env.git(&repo, &["push", "-q", "origin", "main"]);
    let setup = Setup { timeout: Duration::from_secs(30), log: env.root().join("lead.log") };
    let (lead, lock_pushed) = if branch == "cpd/pay" {
        let created = create_lead_worktree(&repo, &plan(), "main", &setup).unwrap();
        (created.path, created.base_sha)
    } else {
        let path = repo.parent().unwrap().join("pay.pay.repo");
        let prepared = cpd_core::git::worktree::prepare_lead_worktree(&repo, &plan(), &path, branch, "main", &setup).map_err(|f| f.error).unwrap();
        prepared.push().unwrap();
        (prepared.path, prepared.base_sha)
    };
    env.git(&lead, &["config", "user.name", "cpd test"]);
    env.git(&lead, &["config", "user.email", "test@cpd.invalid"]);
    fs::write(lead.join("api.txt"), "api\n").unwrap();
    env.git(&lead, &["add", "-A"]);
    env.git(&lead, &["commit", "-q", "-m", "api"]);
    let merged_sha = env.git(&lead, &["rev-parse", "HEAD"]);

    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(
        home.config(),
        format!("version = 1\nmachine = \"t\"\n[timeouts]\n{config_extra}\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket()),
    )
    .unwrap();
    let gh = env.root().join("gh");
    fs::create_dir_all(&gh).unwrap();
    for (name, text) in [("list.json", "[]"), ("create.out", URL), ("search.json", EMPTY_SEARCH), ("lookup.json", r#"{"data":{}}"#)] {
        fs::write(gh.join(name), format!("{text}\n")).unwrap();
    }
    if phase == "dispatch" {
        // `pr open` reads `createdAt` of the pull request it just opened (store-W5).
        fs::write(gh.join("lookup.json"), format!("{}\n", lookup_answer(&[Some(pay("OPEN", None, None))]))).unwrap();
    }
    env.fake_gh(GH);

    let fx = Fx { env, home, repo, origin, lead, merged_sha, lock_pushed, branch: branch.to_owned() };
    if phase == "deploy" {
        fx.git(&fx.lead, &["push", "-q", "-u", "origin", branch]);
    }
    let lead = format!("'{}'", fx.lead.display());
    let (locked, status, columns, values) = match phase {
        "plan" => ("NULL", "waiting", "", String::new()),
        "dispatch" => ("'t'", "dispatch", "", String::new()),
        "deploy" => ("'t'", "open", ", pr_number, pr_url, pr_opened_at, pull_request", format!(", 812, '{URL}', 'opened', (SELECT id FROM pull_requests)")),
        "done" => ("'t'", "merged", ", pr_number, pr_url, pr_opened_at, pull_request, merged_at", format!(", 812, '{URL}', 'opened', (SELECT id FROM pull_requests), 'finished'")),
        other => panic!("no phase {other}"),
    };
    let (worktree, done) = match phase {
        "plan" => ("NULL", "NULL"),
        "done" => (&lead[..], "'finished'"),
        _ => (&lead[..], "NULL"),
    };
    // What `pr open` writes: the pull request's row, with the Plan's title, no review, no checks.
    let pull_request = match phase {
        "deploy" => format!("INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                         VALUES ('acme/pay', 812, '{URL}', 'Pay the bills', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');"),
        "done" => format!("INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                           VALUES ('acme/pay', 812, '{URL}', 'Pay the bills', 0, 'merged', 'none', 'none', 'opened', 'finished', 'finished');"),
        _ => String::new(),
    };
    let sql = format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay the bills', '{phase}', 'lead1', 't', {locked}, {done});
         {pull_request}
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at{columns})
           VALUES ('pay', 'repo', '{repo}', 'main', '{status}', '{branch}', {worktree}, '{REPO_TITLE}', 'config', cpd_txn(), 't', {locked}{values});",
        repo = fx.repo.display(),
        branch = branch,
    );
    fx.store()
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            tx.execute(
                "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, merged_sha, cancel_reason, cancel_requested_at, created_txn, created_at)
                 VALUES ('pay', 'repo', 'api', 'The | API', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', ?4, 0, ?1,
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
        self.env.cpd(&["pr", command, "--plan", "pay"])
    }

    /// Sets what the fake `gh` answers: `list.json`, `create.out`, `search.json`, `lookup.json`, `lookup.exit`, `fail`.
    fn gh(&self, file: &str, text: &str) {
        fs::write(self.env.root().join("gh").join(file), format!("{text}\n")).unwrap();
    }

    /// Sets `test_concurrency` (a top-level key, so before the tables) in `config.toml`.
    fn test_concurrency(&self, n: u32) {
        let config = fs::read_to_string(self.home.config()).unwrap();
        fs::write(self.home.config(), config.replacen("machine = \"t\"\n", &format!("machine = \"t\"\ntest_concurrency = {n}\n"), 1)).unwrap();
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
        self.git(&self.repo, &["rev-parse", &format!("refs/heads/{}", self.branch)])
    }

    /// `(phase, pr_number, pr_url, pr_opened_at, pull_request, done_at)` of the Plan.
    fn plan_row(&self) -> (String, Option<i64>, Option<String>, Option<String>, Option<i64>, Option<String>) {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT p.phase, r.pr_number, r.pr_url, r.pr_opened_at, r.pull_request, p.done_at
                       FROM plans p JOIN prs r ON r.plan = p.id WHERE p.id = 'pay'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
                )?)
            })
            .unwrap()
    }

    /// The row of `pull_requests` the Plan's PR points to, as `(title, review_decision, checks, state)`.
    fn pr_data(&self) -> (String, String, String, String) {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT q.title, q.review_decision, q.checks, q.state
                       FROM prs r JOIN pull_requests q ON q.id = r.pull_request WHERE r.plan = 'pay'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?)
            })
            .unwrap()
    }

    /// The `pull_request` of the Plan's PR, as `plan show` prints it.
    fn shown_pull_request(&self) -> Value {
        self.env.cpd(&["plan", "show", "--plan", "pay"]).ok()["prs"][0]["pull_request"].clone()
    }

    /// How many write transactions have run: the rows of `txns`.
    fn txns(&self) -> i64 {
        self.store().read(|c| Ok(c.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?)).unwrap()
    }

    /// `(text, data)` of the Events of Plan `pay` of `kind`, in order.
    fn events(&self, kind: &str) -> Vec<(String, Value)> {
        self.plan_events("pay", kind)
    }

    /// `(text, data)` of the Events of Plan `plan` of `kind`, in order.
    fn plan_events(&self, plan: &str, kind: &str) -> Vec<(String, Value)> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare("SELECT text, data FROM events WHERE plan = ?1 AND kind = ?2 ORDER BY id")?;
                let rows = stmt
                    .query_map([plan, kind], |r| Ok((r.get::<_, String>(0)?, serde_json::from_str(&r.get::<_, String>(1)?).unwrap())))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    fn gh_calls(&self) -> Vec<Vec<String>> {
        self.env.calls("gh")
    }

    /// The `pre-merge` hook's output: the log without the Gate's own `gate: ...` lines at its head (the slot's, §7.6).
    fn gate_log(&self) -> String {
        let log = self.pr_log("pre-merge");
        let mut rest = log.as_str();
        while let Some(line) = rest.lines().next().filter(|l| l.starts_with("gate: ")) {
            rest = &rest[line.len() + 1..];
        }
        rest.to_owned()
    }

    /// `run/pay/logs/pr-repo.<hook>.log`.
    fn pr_log(&self, hook: &str) -> String {
        fs::read_to_string(self.home.plan_dir(&plan()).join(format!("logs/pr-repo.{hook}.log"))).unwrap_or_default()
    }

    /// No gate worktree is left: none on disk next to the repo, none known to git.
    fn assert_no_gate_worktree(&self) {
        let left: Vec<String> = fs::read_dir(self.repo.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".gate-"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
        let listed = self.git(&self.repo, &["worktree", "list", "--porcelain"]);
        assert!(!listed.contains(".gate-"), "{listed}");
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

/// What the Plan's second PR is for the two-PR tests.
enum Web {
    /// Started: Lead worktree, a Merged Job, pull request not opened.
    Dispatch,
    /// Opened as pull request 813.
    Open,
    /// Not started: it needs `repo`, and its Job is `queued`.
    Waiting,
}

impl Fx {
    /// A second PR `web` of Plan `pay` in its own repo with a bare `origin`: Lead Branch
    /// `cpd/pay/web`, Lead worktree `web.pay.web`, Job `ui`; the fake `gh` answers for it
    /// (pull request 813) when run in that worktree. Returns the Lead worktree.
    fn add_web(&self, state: Web) -> PathBuf {
        let repo = self.env.keyed_repo("web").canonicalize().unwrap();
        let origin = self.env.root().join("repos/web-origin.git");
        self.git(self.env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
        self.env.local_origin(&repo, &origin);
        self.git(&repo, &["push", "-q", "origin", "main"]);
        let lead = repo.parent().unwrap().join("web.pay.web");
        let mut sha = String::new();
        if !matches!(state, Web::Waiting) {
            self.git(&repo, &["worktree", "add", "-q", "-b", "cpd/pay/web", lead.to_str().unwrap(), "main"]);
            self.git(&lead, &["config", "user.name", "cpd test"]);
            self.git(&lead, &["config", "user.email", "test@cpd.invalid"]);
            fs::write(lead.join("ui.txt"), "ui\n").unwrap();
            self.git(&lead, &["add", "-A"]);
            self.git(&lead, &["commit", "-q", "-m", "ui"]);
            sha = self.git(&lead, &["rev-parse", "HEAD"]);
            self.git(&lead, &["push", "-q", "-u", "origin", "cpd/pay/web"]);
        }
        let answers = self.env.root().join("gh/web.pay.web");
        fs::create_dir_all(&answers).unwrap();
        fs::write(answers.join("create.out"), format!("{WEB_URL}\n")).unwrap();
        fs::write(answers.join("lookup.json"), format!("{}\n", lookup_answer(&[Some(node("acme/web", 813, "OPEN", None, None))]))).unwrap();
        let (status, worktree, started, open_columns, open_values, job, needs) = match state {
            Web::Dispatch => ("dispatch", format!("'{}'", lead.display()), "'t'", "", String::new(), "merged", ""),
            Web::Open => (
                "open",
                format!("'{}'", lead.display()),
                "'t'",
                ", pr_number, pr_url, pr_opened_at, pull_request",
                format!(", 813, '{WEB_URL}', 'opened', (SELECT id FROM pull_requests WHERE number = 813)"),
                "merged",
                "",
            ),
            Web::Waiting => ("waiting", "NULL".to_owned(), "NULL", "", String::new(), "queued", "INSERT INTO pr_needs (plan, pr, needs) VALUES ('pay', 'web', 'repo');"),
        };
        self.store()
            .write("fixture", |tx| {
                tx.execute_batch(&format!(
                    "{row}
                     INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at{open_columns})
                       VALUES ('pay', 'web', '{repo}', 'main', '{status}', 'cpd/pay/web', {worktree}, '{WEB_PR_TITLE}', 'config', cpd_txn(), 't', {started}{open_values});
                     {needs}",
                    repo = repo.display(),
                    row = if matches!(state, Web::Open) {
                        format!(
                            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                             VALUES ('acme/web', 813, '{WEB_URL}', 'Pay the bills (web)', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');"
                        )
                    } else {
                        String::new()
                    },
                ))?;
                tx.execute(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, merged_sha, created_txn, created_at)
                     VALUES ('pay', 'web', 'ui', 'The UI', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', ?3, 0, ?1,
                             iif(?3 = 'merged', ?2, NULL), ?4, 't')",
                    params!["f".repeat(64), sha, job, tx.id()],
                )?;
                Ok(())
            })
            .unwrap();
        lead
    }

    fn pr_cmd(&self, command: &str, key: &str) -> Run {
        self.env.cpd(&["pr", command, "--pr", key, "--plan", "pay"])
    }

    fn phase(&self) -> String {
        self.plan_row().0
    }

    fn pr_status(&self, key: &str) -> String {
        self.store().read(|c| Ok(c.query_row("SELECT status FROM prs WHERE plan = 'pay' AND key = ?1", [key], |r| r.get(0))?)).unwrap()
    }
}

fn code(run: &Run) -> (String, i32) {
    (run.error()["code"].as_str().unwrap().to_owned(), run.exit)
}

/// A `pre-merge` paused mid-run: it prints the gated commit and its files, creates
/// `<root>/gate-started`, waits for `<root>/gate-go` (at most 60 s), then prints
/// them again.
fn paused_gate(root: &Path) -> String {
    let (started, go) = (root.join("gate-started"), root.join("gate-go"));
    format!(
        "pre-merge = \"git rev-parse HEAD; ls; touch {}; i=0; while [ ! -e {} ] && [ $i -lt 600 ]; do sleep 0.1; i=$((i+1)); done; git rev-parse HEAD; ls\"\n",
        started.display(),
        go.display()
    )
}

/// Waits until the [`paused_gate`] has started.
fn await_gate(root: &Path) {
    common::wait_until_for("the Gate to start", Duration::from_secs(120), || root.join("gate-started").exists());
}

/// Lets the [`paused_gate`] finish.
fn release_gate(root: &Path) {
    fs::write(root.join("gate-go"), "").unwrap();
}

impl Fx {
    /// `cpd pr <command> --plan pay`, started in the background.
    fn spawn_pr(&self, command: &str) -> Child {
        Child(Some(self.env.command(CPD).args(["pr", command, "--plan", "pay"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()))
    }
}

// ------------------------------------------------------------------ pr open

#[test]
fn open_gates_pushes_creates_the_pr_and_records_it() {
    let fx = fx("dispatch");
    fx.hooks(
        "pre-start = \"echo set up in $(basename $PWD)\"\npre-merge = \"echo gate ran on {{ branch }} in $(basename $PWD) at $(git rev-parse HEAD)\"\n",
    );
    let tip = fx.lead_sha();

    let out = fx.pr("open").ok();

    let opened_at = out["pr"]["pr"]["opened_at"].as_str().unwrap().to_owned();
    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "phase": "deploy", "pr": {"key": "repo", "status": "open", "pr": {"number": 812, "url": URL, "opened_at": opened_at}},
               "sha": tip, "pushed": true, "remote": tip})
    );
    // The Gate ran in a gate worktree detached at the tip, `pre-start` first, with `branch` the
    // Lead Branch, and wrote its logs; the gate worktree is gone.
    let gate = "pay.pay.repo.gate-";
    assert!(fx.pr_log("pre-start").starts_with(&format!("set up in {gate}")), "{}", fx.pr_log("pre-start"));
    let log = fx.gate_log();
    assert!(log.starts_with(&format!("gate ran on cpd/pay in {gate}")) && log.trim_end().ends_with(&format!(" at {tip}")), "{log}");
    fx.assert_no_gate_worktree();
    // Pushed, by rule P: no upstream written to `.git/config` (G1).
    assert_eq!(fx.remote_branch("cpd/pay"), Some(tip));
    let config = fs::read_to_string(fx.repo.join(".git/config")).unwrap();
    assert!(!config.contains("[branch "), "{config}");
    // gh: look for an existing PR, then create it with the Jobs table as its body.
    let calls = fx.gh_calls();
    assert_eq!(calls[0], ["pr", "list", "--head", "cpd/pay", "--state", "all", "--json", "number,url,state"]);
    assert_eq!(calls[1][..8], ["pr", "create", "--base", "main", "--head", "cpd/pay", "--title", REPO_TITLE]);
    let body = &calls[1][9];
    assert_eq!(calls[1][8], "--body");
    assert!(body.contains("| api | The \\| API | merged | `"), "{body}");
    assert!(body.contains(&fx.merged_sha), "{body}");
    // Rows and the Event, in one transaction.
    let row = fx.plan_row();
    assert_eq!((row.0.as_str(), row.1, row.2.as_deref(), row.3.as_deref()), ("deploy", Some(812), Some(URL), Some(opened_at.as_str())));
    // The pull request's row (D6), from the URL `gh pr create` printed, and the PR points to it.
    assert!(row.4.is_some(), "prs.pull_request is set");
    assert_eq!(
        fx.shown_pull_request(),
        json!({"repo": "acme/pay", "number": 812, "url": URL, "title": REPO_TITLE, "draft": false, "state": "open",
               "review_decision": "none", "checks": "none", "mergeable": "unknown", "merge_state": "unknown",
               "created_at": "opened", "merged_at": null})
    );
    let opened = fx.events("pr-opened");
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].1, json!({"pr": "repo", "number": 812, "url": URL}));
    let events = fx.events("plan-phase");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1, json!({"from": "dispatch", "to": "deploy", "prs": [{"key": "repo", "number": 812, "url": URL}]}));
    assert!(events[0].0.contains("cpd pr poll --plan pay"), "{}", events[0].0);
}

#[test]
fn open_titles_the_pull_request_and_the_body_with_the_stored_title() {
    let fx = fx("dispatch");
    fx.pr("open").ok();

    let create = &fx.gh_calls()[1];
    assert_eq!(create[1], "create");
    assert_eq!(create[7], REPO_TITLE, "not the Plan's title");
    assert!(create[9].starts_with(&format!("# {REPO_TITLE}\n")), "{}", create[9]);
}

#[test]
fn open_refuses_a_pr_without_a_title_before_the_gate_and_any_gh_call() {
    let fx = fx("dispatch");
    fx.hooks("pre-merge = \"echo gate ran\"\n");
    fx.store().write("fixture", |tx| Ok(tx.execute("UPDATE prs SET title = NULL", [])?)).unwrap();
    let txns = fx.txns();

    let run = fx.pr("open");

    assert_eq!(code(&run), ("pr_title_required".into(), 3));
    let error = run.error();
    assert_eq!(error["details"], json!({"plan": "pay", "pr": "repo"}));
    assert!(error["message"].as_str().unwrap().contains("cpd pr edit repo --title \"<text>\""), "{error}");
    assert!(fx.gh_calls().is_empty(), "no gh call");
    assert!(fs::read_to_string(fx.home.plan_dir(&plan()).join("logs/pr-repo.pre-merge.log")).is_err(), "the Gate did not run");
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lock_pushed.clone()), "nothing pushed");
    assert_eq!((fx.plan_row().0, fx.txns()), ("dispatch".to_owned(), txns));
}

#[test]
fn open_keeps_the_row_the_poll_stored_first() {
    let fx = fx("dispatch");
    // The poll saw the pull request (it is the User's, and open) before `pr open` recorded it.
    fx.store()
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                 VALUES ('acme/pay', 812, '{URL}', 'As the poll wrote it', 1, 'open', 'approved', 'pending', 'github time', NULL, 'poll time')"
            ))?;
            Ok(())
        })
        .unwrap();

    fx.pr("open").ok();

    assert_eq!(
        fx.shown_pull_request(),
        json!({"repo": "acme/pay", "number": 812, "url": URL, "title": "As the poll wrote it", "draft": true, "state": "open",
               "review_decision": "approved", "checks": "pending", "mergeable": "unknown", "merge_state": "unknown",
               "created_at": "github time", "merged_at": null})
    );
    let rows: i64 = fx.store().read(|c| Ok(c.query_row("SELECT count(*) FROM pull_requests", [], |r| r.get(0))?)).unwrap();
    assert_eq!(rows, 1);
}

/// `pay` as GitHub reports it (with the title `pr open` gave it), created at `created`.
fn pay_created(state: &str, created: &str) -> String {
    pay(state, None, None).replace("Pay the bills", REPO_TITLE).replace(r#""createdAt":"opened""#, &format!(r#""createdAt":"{created}""#))
}

#[test]
fn open_stores_the_created_at_gh_reports_and_later_polls_keep_it() {
    let fx = fx("dispatch");
    fx.lookup(&[Some(pay_created("OPEN", "2026-05-01T10:00:00Z"))]);

    fx.pr("open").ok();

    let created = || fx.shown_pull_request()["created_at"].clone();
    assert_eq!(created(), json!("2026-05-01T10:00:00Z"), "GitHub's time, not cpd's");
    let writes = || -> i64 {
        fx.store().read(|c| Ok(c.query_row("SELECT count(*) FROM history WHERE tbl = 'pull_requests'", [], |r| r.get(0))?)).unwrap()
    };
    assert_eq!(writes(), 1, "one insert");

    // The poll reads the same pull request: nothing to write.
    fx.search(&[pay_created("OPEN", "2026-05-01T10:00:00Z")]);
    fx.poll();
    assert_eq!((created(), writes()), (json!("2026-05-01T10:00:00Z"), 1));

    // Whatever a later poll reads, `created_at` is written once.
    fx.search(&[pay_created("OPEN", "2026-05-01T10:00:01Z").replace(r#""isDraft":false"#, r#""isDraft":true"#)]);
    fx.poll();
    assert_eq!(created(), json!("2026-05-01T10:00:00Z"));
    assert_eq!(fx.shown_pull_request()["draft"], json!(true), "the other values still follow the poll");
}

#[test]
fn open_fails_loud_when_gh_cannot_find_the_pull_request_it_created() {
    let fx = fx("dispatch");
    fx.lookup(&[None]);

    let run = fx.pr("open");

    assert_eq!(code(&run).0, "gh_failed");
    assert_eq!(fx.plan_row().0, "dispatch");
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
    fx.gh("list.json", r#"[{"number": 812, "url": "https://github.com/acme/pay/pull/812", "state": "OPEN"}]"#);

    let out = fx.pr("open").ok();

    assert_eq!(out["pr"]["pr"]["number"], 812);
    assert!(fx.gh_calls().iter().all(|c| c[1] != "create"), "{:?}", fx.gh_calls());
    assert_eq!(fx.plan_row().0, "deploy");
}

/// A fixture of phase `dispatch` whose `pre-merge` hook leaves a marker file when it runs; and that marker.
fn fx_with_marked_hook() -> (Fx, PathBuf) {
    let fx = fx("dispatch");
    let marker = fx.env.root().join("hook-ran");
    fx.hooks(&format!("pre-merge = \"touch {}\"\n", marker.display()));
    (fx, marker)
}

const MERGED_LIST: &str = r#"[{"number": 812, "url": "https://github.com/acme/pay/pull/812", "state": "MERGED"}]"#;

#[test]
fn open_adopts_a_merged_pull_request_without_the_gate_and_the_plan_is_done() {
    let (fx, marker) = fx_with_marked_hook();
    fx.gh("list.json", MERGED_LIST);
    fx.lookup(&[Some(pay("MERGED", None, None))]);
    let remote_before = fx.remote_branch("cpd/pay");

    let out = fx.pr("open").ok();

    let opened_at = out["pr"]["pr"]["opened_at"].as_str().unwrap().to_owned();
    assert_eq!(
        out,
        json!({"ok": true, "adopted": "merged", "plan": "pay", "phase": "done",
               "pr": {"key": "repo", "status": "merged", "pr": {"number": 812, "url": URL, "opened_at": opened_at}}})
    );
    // No Gate, no push, no pull request created.
    assert!(!marker.exists(), "the pre-merge hook ran");
    assert_eq!(fx.gate_log(), "");
    assert_eq!(fx.remote_branch("cpd/pay"), remote_before);
    assert!(fx.gh_calls().iter().all(|c| c[1] != "create"), "{:?}", fx.gh_calls());
    // The PR and the Plan, as `pr poll` leaves them.
    let row = fx.plan_row();
    assert_eq!((row.0.as_str(), row.1, row.2.as_deref()), ("done", Some(812), Some(URL)));
    assert!(row.4.is_some() && row.5.is_some(), "{row:?}");
    assert_eq!(fx.pr_status("repo"), "merged");
    assert_eq!(fx.pr_data(), ("Pay the bills".into(), "none".into(), "none".into(), "merged".into()));
    assert_eq!(fx.shown_pull_request()["merged_at"], "2026-10-14T10:00:00Z");
    let merged = fx.events("pr-merged");
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].1, json!({"pr": "repo", "number": 812, "url": URL, "merged_at": "2026-10-14T10:00:00Z"}));
    assert!(fx.events("pr-opened").is_empty());
    assert_eq!(fx.events("plan-phase")[0].1["to"], "deploy");
    let done = fx.events("plan-done");
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].1["prs"][0]["key"], "repo");

    // Again: already, nothing more.
    let txns = fx.txns();
    let again = fx.pr("open").ok();
    assert_eq!((again["already"].as_bool(), again["phase"].as_str()), (Some(true), Some("done")));
    assert_eq!((fx.txns(), fx.events("plan-done").len()), (txns, 1));
}

#[test]
fn open_adopting_a_merged_pull_request_keeps_the_plan_in_dispatch_while_another_pr_is_not_open() {
    let fx = fx("dispatch");
    fx.add_web(Web::Dispatch);
    fx.gh("list.json", MERGED_LIST);
    fx.lookup(&[Some(pay("MERGED", None, None))]);

    let out = fx.pr_cmd("open", "repo").ok();

    assert_eq!((out["adopted"].as_str(), out["phase"].as_str(), out["pr"]["status"].as_str()), (Some("merged"), Some("dispatch"), Some("merged")));
    assert_eq!((fx.phase().as_str(), fx.pr_status("web").as_str()), ("dispatch", "dispatch"));
    assert_eq!(fx.events("pr-merged").len(), 1);
    assert!(fx.events("plan-phase").is_empty() && fx.events("plan-done").is_empty());
}

#[test]
fn open_adopting_a_merged_pull_request_updates_the_row_the_poll_stored_open() {
    let fx = fx("dispatch");
    fx.store()
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                 VALUES ('acme/pay', 812, '{URL}', 'As the poll wrote it', 0, 'open', 'approved', 'passed', 'github time', NULL, 'poll time')"
            ))?;
            Ok(())
        })
        .unwrap();
    fx.gh("list.json", MERGED_LIST);
    fx.lookup(&[Some(pay("MERGED", Some("APPROVED"), Some("SUCCESS")))]);

    fx.pr("open").ok();

    assert_eq!(fx.pr_data().3, "merged");
    assert_eq!(fx.pr_status("repo"), "merged");
    let rows: i64 = fx.store().read(|c| Ok(c.query_row("SELECT count(*) FROM pull_requests", [], |r| r.get(0))?)).unwrap();
    assert_eq!(rows, 1);
}

#[test]
fn open_refuses_a_closed_pull_request_naming_it_and_changes_nothing() {
    let (fx, marker) = fx_with_marked_hook();
    // The newest (813) is closed, whatever the order `gh` lists them in; the older one is open.
    fx.gh(
        "list.json",
        r#"[{"number": 812, "url": "https://github.com/acme/pay/pull/812", "state": "OPEN"},
            {"number": 813, "url": "https://github.com/acme/pay/pull/813", "state": "CLOSED"}]"#,
    );
    let remote_before = fx.remote_branch("cpd/pay");
    let txns = fx.txns();

    let run = fx.pr("open");

    assert_eq!(code(&run), ("pr_closed".into(), 3));
    let error = run.error();
    assert_eq!((error["details"]["number"].clone(), error["details"]["url"].clone()), (json!(813), json!("https://github.com/acme/pay/pull/813")));
    assert!(error["message"].as_str().unwrap().contains("https://github.com/acme/pay/pull/813"), "{error}");
    assert!(!marker.exists(), "the pre-merge hook ran");
    assert_eq!(fx.remote_branch("cpd/pay"), remote_before);
    assert_eq!((fx.txns(), fx.plan_row().0.as_str(), fx.pr_status("repo").as_str()), (txns, "dispatch", "dispatch"));
    assert!(fx.gh_calls().iter().all(|c| c[1] == "list"), "{:?}", fx.gh_calls());
}

#[test]
fn open_with_an_open_pull_request_of_the_branch_still_gates_pushes_and_adopts_it() {
    let (fx, marker) = fx_with_marked_hook();
    fx.gh("list.json", r#"[{"number": 812, "url": "https://github.com/acme/pay/pull/812", "state": "OPEN"}]"#);

    let out = fx.pr("open").ok();

    assert!(marker.exists(), "the Gate runs for an open pull request");
    assert!(out.get("adopted").is_none(), "{out}");
    assert_eq!((out["phase"].as_str(), out["pr"]["status"].as_str()), (Some("deploy"), Some("open")));
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lead_sha()));
    assert_eq!(fx.events("pr-opened").len(), 1);
    assert!(fx.events("pr-merged").is_empty());
}

#[test]
fn open_adopts_an_open_pull_request_of_a_lead_branch_that_is_behind_its_base() {
    let (fx, marker) = fx_with_marked_hook();
    fx.gh("list.json", r#"[{"number": 812, "url": "https://github.com/acme/pay/pull/812", "state": "OPEN"}]"#);
    fx.advance_base("base.txt", "base\n");

    let out = fx.pr("open").ok();

    assert!(marker.exists(), "the Gate ran");
    assert_eq!((out["phase"].as_str(), out["pr"]["status"].as_str()), (Some("deploy"), Some("open")));
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lead_sha()));
}

#[test]
fn open_refuses_to_adopt_an_open_pull_request_of_a_lead_branch_that_conflicts_with_its_base() {
    let (fx, marker) = fx_with_marked_hook();
    fx.gh("list.json", r#"[{"number": 812, "url": "https://github.com/acme/pay/pull/812", "state": "OPEN"}]"#);
    let base = conflicting(&fx);

    let run = fx.pr("open");

    assert_eq!(code(&run), ("base_conflict".into(), 3));
    assert_eq!(run.error()["details"]["base_sha"], base.as_str());
    assert_eq!(run.error()["details"]["files"], json!(["api.txt"]));
    assert!(!marker.exists(), "no Gate");
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lock_pushed.clone()), "nothing was pushed");
    assert_eq!(fx.plan_row().0, "dispatch");
}

#[test]
fn open_again_is_already_and_touches_nothing() {
    let fx = fx("dispatch");
    let first = fx.pr("open").ok();
    let calls = fx.gh_calls().len();
    let before = fx.plan_row();

    let again = fx.pr("open").ok();

    assert_eq!(again, json!({"ok": true, "plan": "pay", "phase": "deploy", "pr": first["pr"], "already": true}));
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
    assert_eq!(out["pr"]["pr"]["number"], 812);
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
                "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, created_txn, created_at)
                 VALUES ('pay', 'repo', 'ui', 'UI', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 0, ?1, ?2, 't')",
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

impl Fx {
    /// The PR's `open` Job `open-repo` in `status`, as engine E4 adds it (contract §3.7 E4: appended, no Dependencies).
    fn open_job(&self, status: &str) {
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, kind, fingerprint, merged_sha, failed_cause, cancel_reason, cancel_requested_at, created_txn, created_at)
                     VALUES ('pay', 'repo', 'open-repo', 'Open the pull request', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', ?1, 1, 'open', ?2,
                             iif(?1 = 'merged', 'abc', NULL), iif(?1 = 'failed', 'crashed', NULL),
                             iif(?1 = 'cancelled', 'start over', NULL), iif(?1 = 'cancelled', 't', NULL), ?3, 't')",
                    params![status, "f".repeat(64), tx.id()],
                )?;
                Ok(())
            })
            .unwrap();
    }
}

/// `pr open` is the repair (contract §7.6): while the PR's `open` Job is `queued` or `running` that Job is about to do it, and two openers
/// would race. Refused `open_job_live` (exit 3, `details.job`), with nothing run, written or pushed.
#[test]
fn open_is_refused_open_job_live_while_the_open_job_is_queued_or_running() {
    for status in ["queued", "running"] {
        let fx = fx("dispatch");
        fx.open_job(status);
        let txns = fx.txns();

        let run = fx.pr("open");

        assert_eq!(code(&run), ("open_job_live".into(), 3), "{status}: {run:?}");
        assert_eq!(run.error()["details"]["job"], "open-repo", "{status}");
        assert!(fx.gh_calls().is_empty(), "{status}");
        assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lock_pushed.clone()), "{status}: nothing was pushed");
        assert_eq!((fx.plan_row().0, fx.txns()), ("dispatch".to_owned(), txns), "{status}");
    }
}

/// A PR whose `open` Job is `failed` (the Lead decides between `job retry` and this), `cancelled` or `merged` on a PR still `dispatch`
/// (a merge that died before it opened the pull request) is opened by hand as before; `jobs_not_done` counts no `open` Job.
#[test]
fn open_by_hand_goes_on_when_the_open_job_failed_was_cancelled_or_merged_and_counts_it_for_nothing() {
    for status in ["failed", "cancelled", "merged"] {
        let fx = fx("dispatch");
        fx.open_job(status);

        let out = fx.pr("open").ok();

        assert_eq!(out["pr"]["status"], "open", "{status}: {out}");
        assert_eq!(fx.plan_row().0, "deploy", "{status}");
        assert_eq!(fx.gh_calls()[1][..2], ["pr", "create"], "{status}");
    }
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
fn open_gates_the_commit_and_leaves_the_lead_worktrees_local_changes_alone() {
    let fx = fx("dispatch");
    fx.hooks("pre-merge = \"ls; cat api.txt\"\n");
    fs::write(fx.lead.join("scratch.txt"), "x").unwrap();
    fs::write(fx.lead.join("api.txt"), "edited\n").unwrap();
    let tip = fx.lead_sha();

    let out = fx.pr("open").ok();

    // Gated and pushed: the commit, not the Lead worktree's files (no `dirty_target`, hs-d243).
    assert_eq!((out["sha"].as_str(), out["pushed"].as_bool()), (Some(tip.as_str()), Some(true)));
    let log = fx.gate_log();
    assert!(!log.contains("scratch.txt") && log.contains("api\n") && !log.contains("edited"), "{log}");
    assert_eq!(fx.remote_branch("cpd/pay"), Some(tip));
    // The local changes are untouched.
    assert_eq!(fx.git(&fx.lead, &["status", "--porcelain"]), " M api.txt\n?? scratch.txt");
}

#[test]
fn open_after_another_open_opened_the_pr_during_its_gate_is_already_and_pushes_nothing() {
    let fx = fx("dispatch");
    fx.hooks(&paused_gate(fx.env.root()));

    let child = fx.spawn_pr("open");
    await_gate(fx.env.root());
    // Another `pr open` commits meanwhile.
    fx.store()
        .write("pr open", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                   VALUES ('acme/pay', 812, '{URL}', 'Pay the bills', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');
                 UPDATE prs SET status = 'open', pr_number = 812, pr_url = '{URL}', pr_opened_at = 'opened',
                        pull_request = (SELECT id FROM pull_requests) WHERE plan = 'pay';"
            ))?;
            Ok(())
        })
        .unwrap();
    release_gate(fx.env.root());
    let out = child.finish();

    let out: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!((out["ok"].as_bool(), out["already"].as_bool()), (Some(true), Some(true)), "{out}");
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lock_pushed.clone()), "pr open pushed nothing");
    // Only step 0's lookup (#91) ran, before the Gate: no `gh pr create`.
    let calls: Vec<String> = fx.gh_calls().iter().map(|c| c[1].clone()).collect();
    assert_eq!(calls, ["list"]);
    fx.assert_no_gate_worktree();
}

#[test]
fn open_adopts_the_pull_request_a_concurrent_open_created_when_its_create_fails() {
    let fx = fx("dispatch");
    // `gh pr create` fails because another `pr open` of the PR created it: the lookup now finds it.
    let raced = format!(r#"[{{"number": 812, "url": "{URL}", "state": "OPEN"}}]"#);
    fs::write(fx.env.root().join("gh/raced.json"), raced).unwrap();
    fx.env.fake_gh(&GH.replace(
        r#""pr create") cat "$(file create.out)" ;;"#,
        r#""pr create") cp "$dir/raced.json" "$dir/list.json"; echo "a pull request for branch cpd/pay already exists" >&2; exit 1 ;;"#,
    ));

    let out = fx.pr("open").ok();

    assert_eq!(out["pr"]["pr"]["number"], 812);
    let calls: Vec<String> = fx.gh_calls().iter().map(|c| c[1].clone()).collect();
    assert_eq!(calls, ["list", "create", "list", "graphql"]);
    assert_eq!(fx.plan_row().0, "deploy");
}

#[test]
fn open_reports_a_create_that_fails_with_no_pull_request_to_adopt() {
    let fx = fx("dispatch");
    fx.env.fake_gh(&GH.replace(r#""pr create") cat "$(file create.out)" ;;"#, r#""pr create") echo "HTTP 422" >&2; exit 1 ;;"#));

    let run = fx.pr("open");

    assert_eq!(code(&run), ("gh_failed".into(), 1));
    assert!(run.error()["details"]["stderr"].as_str().unwrap().contains("HTTP 422"));
    assert_eq!(fx.plan_row().0, "dispatch");
}

#[test]
fn open_with_a_failing_pre_start_is_gate_error_and_runs_no_gate() {
    let fx = fx("dispatch");
    fx.hooks("pre-start = \"echo no deps; exit 2\"\npre-merge = \"echo gate ran\"\n");

    let run = fx.pr("open");

    assert_eq!(code(&run), ("gate_error".into(), 1));
    let details = &run.error()["details"];
    assert_eq!((details["hook"].as_str(), details["exit"].as_i64()), (Some("pre-start"), Some(2)), "{details}");
    assert!(details["tail"].as_str().unwrap().contains("no deps"), "{details}");
    assert!(details["log"].as_str().unwrap().ends_with("logs/pr-repo.pre-start.log"), "{details}");
    assert_eq!(fx.gate_log(), "", "pre-merge did not run");
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lock_pushed.clone()), "pr open pushed nothing");
    assert_eq!(fx.plan_row().0, "dispatch");
    fx.assert_no_gate_worktree();
}

#[test]
fn a_gate_worktree_left_by_a_killed_run_is_swept_and_a_live_ones_is_kept() {
    let fx = fx("dispatch");
    let tip = fx.lead_sha();
    // A dead pid (a child that has exited) and a live one (this test's).
    let mut dead = std::process::Command::new("true").spawn().unwrap();
    let dead_pid = dead.id();
    dead.wait().unwrap();
    let path = |pid: u32| fx.repo.parent().unwrap().join(format!("pay.pay.repo.gate-{pid}"));
    for pid in [dead_pid, std::process::id()] {
        let reason = format!("cpd gate {pid}");
        fx.git(&fx.repo, &["worktree", "add", "-q", "--detach", "--lock", "--reason", &reason, path(pid).to_str().unwrap(), &tip]);
    }

    fx.pr("open").ok();

    assert!(!path(dead_pid).exists());
    assert!(path(std::process::id()).exists());
    let listed = fx.git(&fx.repo, &["worktree", "list", "--porcelain"]);
    assert!(!listed.contains(&format!(".gate-{dead_pid}")) && listed.contains(&format!(".gate-{}", std::process::id())), "{listed}");
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
    assert_eq!(details["log"], fx.home.plan_dir(&plan()).join("logs/pr-repo.pre-merge.log").to_str().unwrap());
    assert!(fx.gate_log().contains("the suite is red"));
    // Nothing pushed, no PR, no row: the phase stays dispatch.
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lock_pushed.clone()), "pr open pushed nothing");
    // Only the lookup before the Gate: nothing created.
    assert!(fx.gh_calls().iter().all(|c| c[1] == "list"), "{:?}", fx.gh_calls());
    let row = fx.plan_row();
    assert_eq!((row.0.as_str(), row.1), ("dispatch", None));
    assert!(fx.events("plan-phase").is_empty());
}

/// A fixture of `phase` under policy `pr.gate = "ci"` (cpd's default), whose failing `pre-merge` leaves a marker if it runs.
fn ci_fx_with_failing_hook(phase: &str) -> (Fx, PathBuf) {
    let fx = fx(phase);
    fx.env.owner_policy("acme", "[pr]\ngate = \"ci\"\n");
    let marker = fx.env.root().join("hook-ran");
    fx.hooks(&format!("pre-merge = \"touch {}; exit 3\"\n", marker.display()));
    (fx, marker)
}

#[test]
fn open_with_gate_ci_runs_no_gate_and_pushes() {
    let (fx, marker) = ci_fx_with_failing_hook("dispatch");

    let out = fx.pr("open").ok();

    assert_eq!(out["phase"], "deploy");
    assert!(!marker.exists(), "the pre-merge hook ran under pr.gate ci");
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lead_sha()), "the Lead Branch was pushed");
}

#[test]
fn rebase_with_gate_ci_runs_no_gate_and_pushes() {
    let (fx, marker) = ci_fx_with_failing_hook("deploy");
    fx.git(&fx.lead, &["push", "-q", "origin", "cpd/pay"]);
    fx.advance_base("base.txt", "base\n");

    let out = fx.pr("rebase").ok();

    assert_eq!(out["rebased"], true);
    assert!(!marker.exists(), "the pre-merge hook ran under pr.gate ci");
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lead_sha()), "the rebased branch was pushed");
}

/// The Lead Branch of `fx` is behind its base, with no conflict: `main` moved on `origin` after the Lead Branch was cut,
/// in a file the Lead Branch does not touch.
fn behind(fx: &Fx) -> String {
    fx.advance_base("base.txt", "base\n")
}

/// The Lead Branch of `fx` conflicts with its base: `main` moved on `origin` with a different `api.txt`, which the Lead
/// Branch added too.
fn conflicting(fx: &Fx) -> String {
    fx.advance_base("api.txt", "other api\n")
}

/// The refusal `base_conflict` (hs-d490): its `details`, nothing pushed, no `gh pr create`, nothing written.
fn assert_base_conflict(fx: &Fx, run: &Run, base_sha: &str) {
    assert_eq!(code(run), ("base_conflict".into(), 3));
    let details = &run.error()["details"];
    assert_eq!(
        *details,
        json!({"pr": "repo", "base": "main", "base_sha": base_sha, "sha": fx.lead_sha(), "files": ["api.txt"],
               "next": [format!("git -C {} merge --no-edit origin/main", fx.lead.display()), "cpd pr open --pr repo"]})
    );
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lock_pushed.clone()), "pr open pushed nothing");
    assert!(fx.gh_calls().iter().all(|c| c[1] == "list"), "{:?}", fx.gh_calls());
    assert_eq!(fx.plan_row().0, "dispatch");
    assert!(fx.events("pr-opened").is_empty());
}

#[test]
fn open_opens_a_lead_branch_behind_its_base_as_it_is_with_no_merge() {
    let fx = fx("dispatch");
    let marker = fx.env.root().join("hook-ran");
    fx.hooks(&format!("pre-merge = \"touch {}\"\n", marker.display()));
    let base = behind(&fx);
    let head = fx.lead_sha();

    let out = fx.pr("open").ok();

    assert_eq!(out["phase"], "deploy");
    assert!(marker.exists(), "the Gate ran on the head as it is");
    assert_eq!(fx.lead_sha(), head, "no merge of the base into the Lead Branch");
    assert_eq!(fx.remote_branch("cpd/pay"), Some(head.clone()), "the head as it is was pushed");
    assert_ne!(fx.git(&fx.repo, &["rev-list", "--count", &format!("{head}..{base}")]), "0", "the base is still ahead");
    assert!(fx.gh_calls().iter().any(|c| c[..2] == ["pr", "create"]), "{:?}", fx.gh_calls());
    assert_eq!(fx.events("pr-opened").len(), 1);
}

#[test]
fn open_refuses_a_lead_branch_that_conflicts_with_its_base_then_opens_after_the_merge() {
    let fx = fx("dispatch");
    let marker = fx.env.root().join("hook-ran");
    fx.hooks(&format!("pre-merge = \"touch {}\"\n", marker.display()));
    let base = conflicting(&fx);

    let run = fx.pr("open");

    assert_base_conflict(&fx, &run, &base);
    assert!(!marker.exists(), "the Gate ran before the conflict check refused");

    // The Lead runs `next`: the merge in the Lead worktree (resolved, here with the Lead's side), then `pr open` again.
    fx.git(&fx.lead, &["merge", "-q", "--no-edit", "-X", "ours", "origin/main"]);
    let out = fx.pr("open").ok();

    assert_eq!(out["phase"], "deploy");
    assert!(marker.exists(), "the Gate ran on the merged head");
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lead_sha()));
    assert_eq!(fx.git(&fx.repo, &["merge-base", "--is-ancestor", &base, &fx.lead_sha()]), "");
}

#[test]
fn open_under_gate_ci_opens_a_lead_branch_behind_its_base() {
    let (fx, marker) = ci_fx_with_failing_hook("dispatch");
    behind(&fx);
    let head = fx.lead_sha();

    let out = fx.pr("open").ok();

    assert_eq!(out["phase"], "deploy");
    assert!(!marker.exists(), "the pre-merge hook ran under pr.gate ci");
    assert_eq!((fx.lead_sha(), fx.remote_branch("cpd/pay")), (head.clone(), Some(head)));
}

#[test]
fn open_under_gate_ci_applies_the_same_conflict_check_and_runs_no_gate() {
    let (fx, marker) = ci_fx_with_failing_hook("dispatch");
    let base = conflicting(&fx);

    let run = fx.pr("open");

    assert_base_conflict(&fx, &run, &base);
    assert!(!marker.exists());
    fx.git(&fx.lead, &["merge", "-q", "--no-edit", "-X", "ours", "origin/main"]);
    assert_eq!(fx.pr("open").ok()["phase"], "deploy");
    assert!(!marker.exists(), "the pre-merge hook ran under pr.gate ci");
}

#[test]
fn open_does_not_fetch_the_base_again_after_the_gate_so_a_base_that_moves_then_refuses_nothing() {
    let fx = fx("dispatch");
    // The base moves, in a conflicting way, while the Gate runs: B was read once, before it (hs-d326).
    let main = fx.repo.clone();
    fx.hooks(&format!(
        "pre-merge = \"cd {main} && echo late > api.txt && git add -A && git commit -q -m late && git push -q origin main\"\n",
        main = main.display()
    ));

    let out = fx.pr("open").ok();

    assert_eq!(out["phase"], "deploy");
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lead_sha()));
}

#[test]
fn open_never_creates_a_draft_pull_request() {
    let fx = fx("dispatch");

    fx.pr("open").ok();

    let calls = fx.gh_calls();
    let create = calls.iter().find(|c| c[..2] == ["pr", "create"]).expect("gh pr create ran");
    assert!(!create.iter().any(|a| a.contains("draft")), "{create:?}");
    assert!(calls.iter().all(|c| !c.iter().any(|a| a.contains("draft"))), "{calls:?}");
}

#[test]
fn open_with_gate_local_runs_the_gate_that_the_ci_policy_skips() {
    let fx = fx("dispatch");
    let marker = fx.env.root().join("hook-ran");
    fx.hooks(&format!("pre-merge = \"touch {}; exit 3\"\n", marker.display()));

    assert_eq!(code(&fx.pr("open")), ("gate_failed".into(), 3));
    assert!(marker.exists());
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
    assert!(details["log"].as_str().unwrap().ends_with("logs/pr-repo.pre-merge.log"));
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lock_pushed.clone()), "pr open pushed nothing");
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

    assert_eq!(fx.plan_row().0, "deploy");
}

#[test]
fn open_without_a_selected_plan_is_plan_required() {
    let fx = fx("dispatch");
    let run = fx.env.cpd(&["pr", "open"]);
    assert_eq!(code(&run), ("plan_required".into(), 2));
}

#[test]
fn open_an_unknown_plan_is_plan_not_found() {
    let fx = fx("dispatch");
    let run = fx.env.cpd(&["pr", "open", "--plan", "nope"]);
    assert_eq!(code(&run), ("plan_not_found".into(), 4));
}

// -------------------------------------------------------------- R before the Gate

const LEVELS: &str = r#"{"O":["off","low","medium","high","xhigh","max"],"S":["off","low","medium"],"H":["off","low","medium"],"G":["off"]}"#;

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
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('oth', 'ctx', 'Other', 'dispatch', 'lead2', 't', 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                   VALUES ('oth', 'repo', '{repo}', 'main', 'dispatch', 'cpd/oth', '{lead}', cpd_txn(), 't', 't');",
                repo = fx.repo.display(),
                lead = lead.display()
            ))?;
            tx.execute(
                "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, created_txn, created_at)
                 VALUES ('oth', 'repo', 'x', 'X', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, ?1, ?2, 't')",
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
fn open_push_and_rebase_run_no_r_for_another_prs_orphaned_merge() {
    // Nobody but E2 and guard U runs R (hs-d243), and guard U only for its own PR's
    // Attempts: this pi has no install, so asking it for a catalog would fail.
    for command in ["open", "push", "rebase"] {
        let fx = fx(if command == "open" { "dispatch" } else { "deploy" });
        fx.env.fake_pi("exit 1");
        orphaned_merge(&fx);

        fx.pr(command).ok();

        let status: String = fx.store().read(|c| Ok(c.query_row("SELECT status FROM attempts WHERE id = 'x-a1'", [], |r| r.get(0))?)).unwrap();
        assert_eq!(status, "merging", "{command}");
    }
}

/// Job `x` of PR `repo` with Attempt `x-a1` in `merging` whose merge's compare-and-swap
/// happened (§7.4 step 8) but not its step 10: its commit (`x.txt`) is on `cpd/pay`, the
/// Lead worktree is stale, and the Ledger knows nothing of it. Returns its worktree and tip.
fn unrecorded_merge(fx: &Fx) -> (PathBuf, String) {
    let attempt = AttemptId::parse("x-a1").unwrap();
    let setup = Setup { timeout: Duration::from_secs(30), log: fx.env.root().join("x.log") };
    let made = create_attempt_worktree(&fx.repo, &fx.lead, &plan(), &attempt, &setup).unwrap();
    fs::write(made.path.join("x.txt"), "x\n").unwrap();
    fx.git(&made.path, &["add", "-A"]);
    fx.git(&made.path, &["commit", "-q", "-m", "x"]);
    let tip = fx.git(&made.path, &["rev-parse", "HEAD"]);
    fx.store()
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, created_txn, created_at)
                 VALUES ('pay', 'repo', 'x', 'X', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, ?1, ?2, 't')",
                params!["f".repeat(64), tx.id()],
            )?;
            tx.execute(
                "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha,
                                       submitted_sha, submitted_at, started_at)
                 VALUES ('pay', 'x-a1', 'x', 1, 'S2', 'ladder', 'merging', ?1, ?2, ?3, ?4, 't', 't')",
                params![made.branch, made.path.to_str().unwrap(), made.base_sha, tip],
            )?;
            Ok(())
        })
        .unwrap();
    fx.git(&fx.repo, &["update-ref", "refs/heads/cpd/pay", &tip, &made.base_sha]);
    (made.path, tip)
}

/// `(status, outcome, merged_sha)` of Attempt `x-a1` of Plan `pay`, and its Job's status.
fn attempt_x(fx: &Fx) -> ((String, Option<String>, Option<String>), String) {
    fx.store()
        .read(|c| {
            let attempt = c.query_row("SELECT status, outcome, merged_sha FROM attempts WHERE plan = 'pay' AND id = 'x-a1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?;
            let job = c.query_row("SELECT status FROM jobs WHERE plan = 'pay' AND key = 'x'", [], |r| r.get(0))?;
            Ok((attempt, job))
        })
        .unwrap()
}

#[test]
fn rebase_runs_r_first_for_a_merge_of_its_pr_whose_driver_died() {
    let fx = fx("deploy");
    install_pi(&fx.env);
    let (worktree, tip) = unrecorded_merge(&fx);
    let base = fx.advance_base("base.txt", "base\n");

    let out = fx.pr("rebase").ok();

    // Guard U: R ended the Attempt `merged` before the rewrite, not `crashed`.
    assert_eq!(attempt_x(&fx), (("ended".to_owned(), Some("merged".to_owned()), Some(tip.clone())), "merged".to_owned()));
    let source: String = fx
        .store()
        .read(|c| Ok(c.query_row("SELECT t.command FROM events e JOIN txns t ON t.id = e.txn WHERE e.kind = 'job-merged'", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(source, "pr rebase");
    assert!(!worktree.exists(), "R's rest of A9 removed the Attempt's worktree");
    // Then the rebase of S, which holds the merge, onto the base; pushed despite R's own push.
    let new = fx.lead_sha();
    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "repo", "rebased": true, "old": tip, "new": new, "pushed": true}));
    assert!(fx.git(&fx.repo, &["merge-base", "--is-ancestor", &base, &new]).is_empty());
    assert_eq!(fx.remote_branch("cpd/pay"), Some(new));
    // Rule S: the Lead worktree holds both, clean.
    assert_eq!(fs::read_to_string(fx.lead.join("x.txt")).unwrap(), "x\n");
    assert_eq!(fs::read_to_string(fx.lead.join("base.txt")).unwrap(), "base\n");
    assert_eq!(fx.git(&fx.lead, &["status", "--porcelain"]), "");
    assert!(fx.events("error").is_empty(), "{:?}", fx.events("error"));
}

#[test]
fn rebase_waits_for_a_live_driver_to_record_its_merge() {
    let fx = fx("deploy");
    fx.env.fake_pi("exit 1");
    let (_, tip) = unrecorded_merge(&fx);
    fx.advance_base("base.txt", "base\n");
    let attempt = AttemptId::parse("x-a1").unwrap();
    let driver = cpd_core::merge::take_free_driver_lock(&fx.home, &plan(), &attempt).unwrap().expect("the driver lock is free");

    let child = fx.spawn_pr("rebase");
    // Guard U waits: the Lead Branch is not rewritten while the merge is unrecorded.
    // time-claim: the rebase is still waiting 1.5 s on, the Lead Branch unrewritten; no per-poll signal from the waiting rebase.
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(fx.lead_sha(), tip);
    assert_eq!(attempt_x(&fx).0.0, "merging");
    // The driver's step 10.
    fx.store()
        .write("attempt run", |tx| {
            tx.execute_batch(&format!(
                "UPDATE attempts SET status = 'ended', outcome = 'merged', cause = 'merged', merged_sha = '{tip}', ended_at = 't'
                  WHERE plan = 'pay' AND id = 'x-a1';
                 UPDATE jobs SET status = 'merged', merged_sha = '{tip}' WHERE plan = 'pay' AND key = 'x';"
            ))?;
            Ok(())
        })
        .unwrap();
    let out = child.finish();
    drop(driver);

    let out: Value = serde_json::from_slice(&out.stdout).unwrap();
    let new = fx.lead_sha();
    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "repo", "rebased": true, "old": tip, "new": new, "pushed": true}));
    assert_eq!(fx.remote_branch("cpd/pay"), Some(new));
    assert!(fx.events("job-merged").is_empty(), "R did not run: the live driver recorded it");
}

// ------------------------------------------------------------------ pr poll

/// A pull request as the search and the lookup print it. Its title and creation time are what the
/// fixtures stored for 812 and 813, so that only the other fields can differ.
fn node(repo: &str, number: i64, state: &str, review: Option<&str>, rollup: Option<&str>) -> String {
    let title = if number == 813 { "Pay the bills (web)" } else { "Pay the bills" };
    let merged = if state == "MERGED" { r#""2026-10-14T10:00:00Z""# } else { "null" };
    let review = review.map_or("null".to_owned(), |r| format!("\"{r}\""));
    let rollup = rollup.map_or("null".to_owned(), |s| format!(r#"{{"state":"{s}"}}"#));
    format!(
        r#"{{"repository":{{"nameWithOwner":"{repo}"}},"number":{number},"url":"https://github.com/{repo}/pull/{number}","title":"{title}",
        "isDraft":false,"state":"{state}","createdAt":"opened","mergedAt":{merged},"reviewDecision":{review},
        "mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN",
        "commits":{{"nodes":[{{"commit":{{"statusCheckRollup":{rollup}}}}}]}}}}"#
    )
}

/// `node`, as GitHub reports its `mergeable` and `mergeStateStatus`.
fn merge_state(node: String, mergeable: &str, merge_state: &str) -> String {
    node.replace(
        r#""mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN""#,
        &format!(r#""mergeable":"{mergeable}","mergeStateStatus":"{merge_state}""#),
    )
}

/// 812, the Plan's pull request.
fn pay(state: &str, review: Option<&str>, rollup: Option<&str>) -> String {
    node("acme/pay", 812, state, review, rollup)
}

const RESET: &str = "2026-10-20T11:00:00Z";

/// The search's answer: `nodes` of the User's own pull requests, `requested` of those that ask for their
/// review, and a rate limit with `remaining` requests.
fn search_answer_with(nodes: &[String], requested: &[String], remaining: u32) -> String {
    format!(
        r#"{{"data":{{"search":{{"issueCount":{},"nodes":[{}]}},"reviewRequests":{{"issueCount":{},"nodes":[{}]}},"rateLimit":{{"cost":1,"remaining":{remaining},"resetAt":"{RESET}"}}}}}}"#,
        nodes.len(),
        nodes.join(","),
        requested.len(),
        requested.join(",")
    )
}

/// The search's answer with no review request.
fn search_answer(nodes: &[String], remaining: u32) -> String {
    search_answer_with(nodes, &[], remaining)
}

/// The lookup's answer, one alias per entry; `None` is an alias GitHub cannot find.
fn lookup_answer(nodes: &[Option<String>]) -> String {
    let data: Vec<String> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| format!(r#""p{i}":{{"pullRequest":{}}}"#, n.as_deref().unwrap_or("null")))
        .collect();
    let errors: Vec<String> = nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.is_none())
        .map(|(i, _)| format!(r#"{{"type":"NOT_FOUND","path":["p{i}","pullRequest"],"message":"Could not resolve to a PullRequest"}}"#))
        .collect();
    match errors.is_empty() {
        true => format!(r#"{{"data":{{{}}}}}"#, data.join(",")),
        false => format!(r#"{{"data":{{{}}},"errors":[{}]}}"#, data.join(","), errors.join(",")),
    }
}

/// `pr-feedback`'s data for PR `repo`.
fn feedback(state: &str, checks: &str, decision: &str, reasons: &[&str]) -> Value {
    feedback_merge(state, checks, decision, "unknown", "unknown", reasons)
}

/// `pr-feedback`'s data for PR `repo`, with the row's `mergeable` and `merge_state`; `conflict` follows `reasons`.
fn feedback_merge(state: &str, checks: &str, decision: &str, mergeable: &str, merge_state: &str, reasons: &[&str]) -> Value {
    json!({"pr": "repo", "number": 812, "url": URL, "state": state, "checks": checks, "review_decision": decision,
           "mergeable": mergeable, "merge_state": merge_state, "conflict": reasons.contains(&"conflict"), "reasons": reasons})
}

impl Fx {
    /// What the fake `gh`'s search answers.
    fn search(&self, nodes: &[String]) {
        self.gh("search.json", &search_answer(nodes, 4987));
    }

    /// What the fake `gh`'s search answers: `authored` in the `author:@me` search, `requested` in the
    /// `review-requested:@me` one.
    fn search_both(&self, authored: &[String], requested: &[String]) {
        self.gh("search.json", &search_answer_with(authored, requested, 4987));
    }

    /// `(repo, number, authored, review_requested)` of every row of `pull_requests`, in order.
    fn memberships(&self) -> Vec<(String, i64, bool, bool)> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare("SELECT repo, number, authored, review_requested FROM pull_requests ORDER BY id")?;
                Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<_>>()?)
            })
            .unwrap()
    }

    /// What the fake `gh`'s lookup answers.
    fn lookup(&self, nodes: &[Option<String>]) {
        self.gh("lookup.json", &lookup_answer(nodes));
    }

    /// `pr poll --plan pay`.
    fn poll(&self) -> Value {
        self.pr("poll").ok()
    }

    /// `pr poll`, as engine E5 runs it.
    fn poll_all(&self) -> Value {
        self.env.cpd(&["pr", "poll"]).ok()
    }

    /// How many Events there are, of any kind.
    fn event_count(&self) -> i64 {
        self.store().read(|c| Ok(c.query_row("SELECT count(*) FROM events", [], |r| r.get(0))?)).unwrap()
    }

    /// `(repo, number)` of every row of `pull_requests`, in order.
    fn pull_requests(&self) -> Vec<(String, i64)> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare("SELECT repo, number FROM pull_requests ORDER BY id")?;
                Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?)
            })
            .unwrap()
    }
}

fn is_search(call: &[String]) -> bool {
    call[..3] == ["api", "graphql", "-f"] && call[3].starts_with("query=query { search(type: ISSUE, query: \"is:pr is:open author:@me archived:false")
}

fn is_lookup(call: &[String]) -> bool {
    call[..3] == ["api", "graphql", "-f"] && call[3].starts_with("query=query { p0: repository(owner: ")
}

#[test]
fn the_first_poll_inserts_every_pull_request_and_appends_nothing_for_unowned_ones() {
    let fx = fx("dispatch");
    fx.search(&[node("acme/pay", 901, "OPEN", Some("APPROVED"), Some("SUCCESS")), node("acme/web", 902, "OPEN", None, None)]);
    let events = fx.event_count();

    let out = fx.poll_all();

    assert_eq!(
        out,
        json!({"ok": true, "polled": 2,
               "changed": [
                   {"repo": "acme/pay", "number": 901, "state": "open", "draft": false, "checks": "passed", "review_decision": "approved", "mergeable": "unknown", "merge_state": "unknown"},
                   {"repo": "acme/web", "number": 902, "state": "open", "draft": false, "checks": "none", "review_decision": "none", "mergeable": "unknown", "merge_state": "unknown"}],
               "base_changed": [], "removed": 0, "feedback": [], "update_jobs": [], "merged": [], "done": [],
               "rate_limit": {"cost": 1, "remaining": 4987, "reset_at": RESET}})
    );
    assert_eq!(fx.pull_requests(), [("acme/pay".to_owned(), 901), ("acme/web".to_owned(), 902)]);
    assert_eq!(fx.event_count(), events, "no PR owns them: no Event");
    // One search call, from `$CPD_HOME`; no lookup, since no PR is open.
    let calls = fx.gh_calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(is_search(&calls[0]), "{calls:?}");
    assert!(calls[0][3].ends_with("rateLimit { cost remaining resetAt } }"), "{}", calls[0][3]);
}

#[test]
fn a_review_request_is_stored_as_not_authored_and_deleted_when_it_leaves_the_search() {
    let fx = fx("dispatch");
    fx.search_both(&[node("acme/pay", 901, "OPEN", None, None)], &[node("acme/web", 902, "OPEN", Some("APPROVED"), Some("FAILURE"))]);
    let events = fx.event_count();

    let out = fx.poll_all();

    assert_eq!((out["polled"].clone(), out["changed"].as_array().unwrap().len(), out["removed"].clone()), (json!(2), 2, json!(0)));
    assert_eq!(fx.memberships(), [("acme/pay".to_owned(), 901, true, false), ("acme/web".to_owned(), 902, false, true)]);
    assert_eq!(fx.event_count(), events, "no PR owns either: no Event, not even for a review request with failed checks");
    let calls = fx.gh_calls();
    assert_eq!(calls.len(), 1, "both searches in one gh call: {calls:?}");
    assert!(is_search(&calls[0]) && calls[0][3].contains("review-requested:@me is:pr is:open archived:false"), "{calls:?}");

    // The same answer: nothing is written.
    let txns = fx.txns();
    assert_eq!(fx.poll_all()["changed"], json!([]));
    assert_eq!(fx.txns(), txns);

    // It leaves the review-request search: its row is deleted, the authored one stays.
    fx.search_both(&[node("acme/pay", 901, "OPEN", None, None)], &[]);
    let out = fx.poll_all();
    assert_eq!((out["removed"].clone(), out["changed"].clone()), (json!(1), json!([])));
    assert_eq!(fx.memberships(), [("acme/pay".to_owned(), 901, true, false)]);
    assert_eq!(fx.gh_calls().len(), 3, "one call per poll, three polls");
}

#[test]
fn a_pull_request_in_both_searches_is_authored_and_review_requested_and_each_flag_clears_when_it_leaves_its_search() {
    let fx = fx("dispatch");
    let mine = node("acme/pay", 901, "OPEN", None, None);
    fx.search_both(&[mine.clone()], &[mine.clone()]);
    let out = fx.poll_all();
    assert_eq!((out["polled"].clone(), out["changed"].as_array().unwrap().len()), (json!(1), 1));
    assert_eq!(fx.memberships(), [("acme/pay".to_owned(), 901, true, true)]);

    // Left the author search only: `authored` clears, the row is kept and reported as changed.
    fx.search_both(&[], &[mine.clone()]);
    let out = fx.poll_all();
    assert_eq!((out["changed"].as_array().unwrap().len(), out["removed"].clone()), (1, json!(0)));
    assert_eq!(fx.memberships(), [("acme/pay".to_owned(), 901, false, true)]);

    // Back in the author search only: `review_requested` clears.
    fx.search_both(&[mine.clone()], &[]);
    fx.poll_all();
    assert_eq!(fx.memberships(), [("acme/pay".to_owned(), 901, true, false)]);

    // In neither: no PR points to it, so it is deleted.
    fx.search_both(&[], &[]);
    assert_eq!(fx.poll_all()["removed"], 1);
    assert!(fx.memberships().is_empty());
}

#[test]
fn a_pr_opened_by_cpd_is_authored_and_a_row_a_pr_points_to_stays_while_in_no_search() {
    let fx = fx("deploy");
    assert_eq!(fx.memberships(), [("acme/pay".to_owned(), 812, true, false)]);
    fx.search_both(&[], &[]);
    fx.lookup(&[Some(pay("OPEN", None, None))]);
    fx.poll();
    assert_eq!(fx.memberships(), [("acme/pay".to_owned(), 812, true, false)], "a lookup answering open changes nothing");
}

#[test]
fn a_poll_reply_without_the_review_request_search_is_gh_failed_and_writes_nothing() {
    let fx = fx("deploy");
    let (txns, events) = (fx.txns(), fx.event_count());
    fx.gh(
        "search.json",
        &search_answer(&[pay("OPEN", None, Some("FAILURE"))], 4987).replace(r#""reviewRequests":{"issueCount":0,"nodes":[]},"#, ""),
    );

    let run = fx.pr("poll");

    assert_eq!(code(&run), ("gh_failed".into(), 1));
    assert!(run.error()["message"].as_str().unwrap().contains("reviewRequests"), "{}", run.error());
    assert_eq!((fx.txns(), fx.event_count()), (txns, events));
}

#[test]
fn a_second_identical_poll_writes_nothing() {
    let fx = fx("deploy");
    fx.search(&[pay("OPEN", None, Some("FAILURE")), node("acme/web", 902, "OPEN", None, None)]);
    let first = fx.poll();
    assert_eq!(first["changed"].as_array().unwrap().len(), 2);

    let (txns, events) = (fx.txns(), fx.event_count());
    let again = fx.poll();

    assert_eq!(
        again,
        json!({"ok": true, "polled": 2, "changed": [], "base_changed": [], "removed": 0, "feedback": [], "update_jobs": [], "merged": [], "done": [],
               "rate_limit": {"cost": 1, "remaining": 4987, "reset_at": RESET}, "prs": again["prs"]})
    );
    assert_eq!((fx.txns(), fx.event_count()), (txns, events), "no write transaction, no Event");
    assert_eq!(fx.gh_calls().iter().filter(|c| is_search(c)).count(), 2);
    assert!(fx.gh_calls().iter().all(|c| !is_lookup(c)), "812 is in the search: no lookup");
}

// ------------------------------------------------------------------ pr poll: the base follow (hs-d325, hs-d326)

/// `node`, with the base branch GitHub reports for the pull request (`baseRefName`).
fn based(node: String, base: &str) -> String {
    assert_eq!(node.matches(r#""mergedAt":"#).count(), 1);
    node.replace(r#""mergedAt":"#, &format!(r#""baseRefName":"{base}","mergedAt":"#))
}

/// 814, the pull request of PR `up`.
fn up(state: &str) -> String {
    node("acme/pay", 814, state, None, None)
}

impl Fx {
    /// Adds PR `up` of Plan `pay`, `open` with its pull request 814, stacked on PR `repo` (its base is `cpd/pay`).
    fn stack_up(&self) {
        let sql = format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
               VALUES ('acme/pay', 814, 'https://github.com/acme/pay/pull/814', 'Pay the bills', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                              pr_number, pr_url, pr_opened_at, pull_request)
               VALUES ('pay', 'up', '{}', 'cpd/pay', 'open', 'cpd/pay/up', '/nonexistent/up', cpd_txn(), 't', 't',
                       814, 'https://github.com/acme/pay/pull/814', 'opened', (SELECT id FROM pull_requests WHERE number = 814));",
            self.repo.display()
        );
        self.store().write("fixture", |tx| Ok(tx.execute_batch(&sql)?)).unwrap();
    }

    /// `(status, base)` of PR `key` of Plan `pay`.
    fn base_of(&self, key: &str) -> (String, String) {
        self.store()
            .read(|c| Ok(c.query_row("SELECT status, base FROM prs WHERE plan = 'pay' AND key = ?1", [key], |r| Ok((r.get(0)?, r.get(1)?)))?))
            .unwrap()
    }

    /// `(draft, state)` of the pull request `number`, as the poll stored it.
    fn stored_pull_request(&self, number: i64) -> (bool, String) {
        self.store()
            .read(|c| Ok(c.query_row("SELECT draft, state FROM pull_requests WHERE number = ?1", [number], |r| Ok((r.get(0)?, r.get(1)?)))?))
            .unwrap()
    }
}

/// Plan `pay`: PR `repo` (812, base `main`) and PR `up` (814) stacked on it, both open, as a poll stored them.
fn stacked() -> Fx {
    let fx = fx("deploy");
    fx.stack_up();
    fx.search(&[based(pay("OPEN", None, None), "main"), based(up("OPEN"), "cpd/pay")]);
    let out = fx.poll();
    assert_eq!(out["base_changed"], json!([]), "GitHub agrees with cpd: nothing to follow");
    assert_eq!(fx.base_of("up"), ("open".to_owned(), "cpd/pay".to_owned()));
    fx
}

#[test]
fn poll_follows_github_repointing_a_stacked_pr_when_its_base_pr_merges_and_keeps_it_ready() {
    let fx = stacked();
    let events = fx.event_count();
    // The base PR merged: GitHub deleted its branch and repointed 814 to `main`. 812 left the open search.
    fx.search(&[based(up("OPEN"), "main")]);
    fx.lookup(&[Some(pay("MERGED", None, None))]);

    let out = fx.poll();

    assert_eq!(out["base_changed"], json!([{"plan": "pay", "pr": "up", "from": "cpd/pay", "to": "main"}]));
    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!(out["feedback"], json!([]));
    assert_eq!(out["done"], json!([]), "up is still open");
    // The PR's base is `main` and nothing else about it moved: still open, its pull request still ready.
    assert_eq!(fx.base_of("up"), ("open".to_owned(), "main".to_owned()));
    assert_eq!(fx.stored_pull_request(814), (false, "open".to_owned()));
    assert_eq!(fx.plan_row().0, "deploy");
    // One informational Event for the Plan, once; no `pr-feedback`.
    let told = fx.events("pr-base-changed");
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(
        told[0].0,
        "PR up of Plan pay now targets main (was cpd/pay): its base PR pay/repo merged and GitHub repointed the pull request. \
         Its tree is unchanged: no new Gate is needed and it stays ready. Read it with cpd plan show --plan pay."
    );
    assert_eq!(
        told[0].1,
        json!({"pr": "up", "from": "cpd/pay", "to": "main", "from_pr": "pay/repo", "to_pr": null, "by": "poll",
               "number": 814, "url": "https://github.com/acme/pay/pull/814"})
    );
    assert!(fx.events("pr-feedback").is_empty());
    assert_eq!(fx.event_count() - events, 2, "pr-merged and pr-base-changed, nothing else");
    // cpd changes no base on GitHub, runs no Gate and opens nothing: it only asked GitHub (graphql).
    assert!(fx.gh_calls().iter().all(|c| c[0] == "api"), "{:?}", fx.gh_calls());
    assert!(!fx.home.plan_dir(&plan()).join("logs/pr-up.pre-merge.log").exists(), "no Gate ran for up");
    assert_eq!(fx.gate_log(), "", "no Gate ran for repo");

    // A second poll changes nothing.
    let (txns, events) = (fx.txns(), fx.event_count());
    let again = fx.poll();
    assert_eq!(again["base_changed"], json!([]));
    assert_eq!(again["changed"], json!([]));
    assert_eq!((fx.txns(), fx.event_count()), (txns, events), "no write transaction, no Event");
    assert_eq!(fx.base_of("up"), ("open".to_owned(), "main".to_owned()));
}

#[test]
fn poll_follows_a_base_retargeted_by_hand_on_any_open_pr() {
    let fx = stacked();
    fx.search(&[based(pay("OPEN", None, None), "develop"), based(up("OPEN"), "cpd/pay")]);

    let out = fx.poll();

    assert_eq!(out["base_changed"], json!([{"plan": "pay", "pr": "repo", "from": "main", "to": "develop"}]));
    assert_eq!(fx.base_of("repo"), ("open".to_owned(), "develop".to_owned()));
    assert_eq!(fx.base_of("up"), ("open".to_owned(), "cpd/pay".to_owned()), "814 still targets 812's branch, as GitHub says");
    let told = fx.events("pr-base-changed");
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(
        told[0].0,
        "PR repo of Plan pay now targets develop (was main): the base changed on GitHub. \
         Its tree is unchanged: no new Gate is needed and it stays ready. Read it with cpd plan show --plan pay."
    );
    assert_eq!(told[0].1["from_pr"], Value::Null);
    assert_eq!(told[0].1["to_pr"], Value::Null);

    let events = fx.event_count();
    assert_eq!(fx.poll()["base_changed"], json!([]));
    assert_eq!(fx.event_count(), events);
}

#[test]
fn poll_changes_nothing_for_a_pr_stacked_above_a_base_pr_closed_unmerged() {
    let fx = stacked();
    // GitHub deletes the branch of a merged pull request only: 812 is closed, 814 still targets `cpd/pay`.
    fx.search(&[based(up("OPEN"), "cpd/pay")]);
    fx.lookup(&[Some(pay("CLOSED", None, None))]);

    let out = fx.poll();

    assert_eq!(out["base_changed"], json!([]));
    assert_eq!(out["feedback"], json!([{"plan": "pay", "pr": "repo"}]), "the closed pull request is the poll's usual pr-feedback");
    assert_eq!(fx.base_of("up"), ("open".to_owned(), "cpd/pay".to_owned()));
    assert_eq!(fx.stored_pull_request(814), (false, "open".to_owned()));
    assert!(fx.events("pr-base-changed").is_empty());

    // The same answers again: still nothing, and the base PR is not cancelled out from under the stacked PR.
    let (txns, events) = (fx.txns(), fx.event_count());
    assert_eq!(fx.poll()["base_changed"], json!([]));
    assert_eq!((fx.txns(), fx.event_count()), (txns, events));
    let run = fx.env.cpd(&["pr", "cancel", "repo", "--plan", "pay"]);
    let error = common::refused(&run, "pr_in_use", 3);
    assert_eq!(error["details"]["prs"], json!(["up"]), "{error}");
    assert_eq!(fx.base_of("repo").0, "open");
    assert_eq!(fx.base_of("up"), ("open".to_owned(), "cpd/pay".to_owned()));
}

/// What PR `web` of Plan `pay` is told when its base PR `pay/repo` merged before `web` had a pull request (hs-d301): `by` is who
/// unstacked it, `number` and `url` are the poll's, both null.
fn web_unstacked(from: &str, by: &str) -> (String, Value) {
    let data = match by {
        "poll" => json!({"pr": "web", "from": from, "to": "main", "from_pr": "pay/repo", "to_pr": null, "by": "poll", "number": null, "url": null}),
        _ => json!({"pr": "web", "from": from, "to": "main", "from_pr": "pay/repo", "to_pr": null, "by": by}),
    };
    (
        format!(
            "PR web of Plan pay now targets main (was {from}): its base PR pay/repo merged before its pull request existed. \
             Its tree is unchanged: no new Gate is needed and it stays ready. Read it with cpd plan show --plan pay."
        ),
        data,
    )
}

#[test]
fn poll_unstacks_a_dispatch_pr_in_the_transaction_that_records_its_base_prs_merge() {
    let fx = fx_stacked("deploy");
    stacked_pr(&fx);
    assert_eq!(fx.web_base(), fx.branch);
    // pay/repo merged (a pull request the search no longer lists); web has no pull request.
    fx.search(&[]);
    fx.lookup(&[Some(pay("MERGED", None, None))]);

    let out = fx.poll();

    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!(fx.web_base(), "main");
    assert_eq!(fx.plan_events("pay", "pr-base-changed"), vec![web_unstacked(&fx.branch, "poll")], "once");
    assert_eq!(fx.plan_events("pay", "pr-feedback"), vec![]);
    assert_eq!(fx.web_status(), "dispatch", "nothing else about web changed: no Gate, no start, no pull request");
    assert!(fx.gh_calls().iter().all(|c| c[0] == "api"), "{:?}", fx.gh_calls());

    // A second poll changes nothing.
    let (txns, events) = (fx.txns(), fx.event_count());
    fx.poll();
    assert_eq!((fx.txns(), fx.event_count()), (txns, events));
}

#[test]
fn poll_does_not_tell_the_plan_of_failed_checks_when_the_policy_says_none() {
    let fx = fx("deploy");
    fx.env.owner_policy("acme", "[pr.feedback]\nchecks = \"none\"\n");
    fx.search(&[pay("OPEN", None, Some("FAILURE"))]);

    let out = fx.poll();

    assert_eq!(out["feedback"], json!([]));
    assert_eq!(fx.pr_data().2, "failed", "the row still records the checks");
    assert!(fx.events("pr-feedback").is_empty());
}

#[test]
fn poll_tells_the_plan_once_that_the_checks_turned_failed() {
    let fx = fx("deploy");
    fx.search(&[pay("OPEN", None, Some("FAILURE"))]);

    let out = fx.poll();

    assert_eq!(out["feedback"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!(out["changed"], json!([{"repo": "acme/pay", "number": 812, "state": "open", "draft": false, "checks": "failed", "review_decision": "none", "mergeable": "unknown", "merge_state": "unknown"}]));
    assert_eq!(fx.pr_data(), ("Pay the bills".into(), "none".into(), "failed".into(), "open".into()));
    let events = fx.events("pr-feedback");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1, feedback("open", "failed", "none", &["checks_failed"]));
    assert_eq!(events[0].0, "PR repo (#812) of Plan pay needs attention: checks failed. Read the details with cpd pr view --plan pay --pr repo.");
    assert_eq!(fx.plan_row().0, "deploy");

    // Still failing: nothing new. `ERROR` is `failed` too.
    fx.search(&[pay("OPEN", None, Some("ERROR"))]);
    assert_eq!(fx.poll()["feedback"], json!([]));
    // Pending, then failing again: from anything else, so told again.
    fx.search(&[pay("OPEN", None, Some("PENDING"))]);
    assert_eq!(fx.poll()["feedback"], json!([]));
    fx.search(&[pay("OPEN", None, Some("FAILURE"))]);
    assert_eq!(fx.poll()["feedback"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!(fx.events("pr-feedback").len(), 2);
}

impl Fx {
    /// `(key, status)` of every `update` Job of Plan `pay`, by key.
    fn update_jobs(&self) -> Vec<(String, String)> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare("SELECT key, status FROM jobs WHERE plan = 'pay' AND kind = 'update' ORDER BY key")?;
                Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap()
    }

    /// GitHub says pull request 812 is `mergeable` / `merge_state`; one poll of Plan `pay`.
    fn polled(&self, mergeable: &str, merge_state_: &str) -> Value {
        self.search(&[merge_state(pay("OPEN", None, None), mergeable, merge_state_)]);
        self.poll()
    }
}

/// The poll queues the PR's `update` Job in the transaction that stores `conflicting` (contract §7.6 step 6, §3.7 E5): `up-repo`, Tier
/// `open_tier`, `queued`, `appended`, no Dependencies, and `job-added` with `kind` `update`, `by` `poll`, `reason` `conflict`. The conflict
/// is no `pr-feedback`, and the PR stays `open`. The same conflict seen again writes nothing.
#[test]
fn poll_queues_one_update_job_when_the_pull_request_becomes_conflicting() {
    let fx = fx("deploy");
    fx.polled("MERGEABLE", "CLEAN");
    assert_eq!(fx.update_jobs(), Vec::<(String, String)>::new());

    let out = fx.polled("CONFLICTING", "DIRTY");

    assert_eq!(fx.update_jobs(), [("up-repo".to_owned(), "queued".to_owned())]);
    assert_eq!(out["feedback"], json!([]), "a conflict is no pr-feedback reason");
    assert!(fx.events("pr-feedback").is_empty());
    let row: (String, String, i64, bool) = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT title, tier, appended, fingerprint IS NOT NULL FROM jobs WHERE plan = 'pay' AND key = 'up-repo'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?)
        })
        .unwrap();
    assert_eq!(row, (format!("Bring the pull request up to date: {REPO_TITLE}"), "S2".into(), 1, true));
    let dependencies: i64 = fx.store().read(|c| Ok(c.query_row("SELECT count(*) FROM dependencies WHERE job = 'up-repo'", [], |r| r.get(0))?)).unwrap();
    assert_eq!(dependencies, 0);
    let added = fx.events("job-added");
    assert_eq!(added.len(), 1, "{added:?}");
    let data = &added[0].1;
    assert_eq!((&data["kind"], &data["by"], &data["reason"], &data["pr"]), (&json!("update"), &json!("poll"), &json!("conflict"), &json!("repo")));
    assert_eq!(fx.pr_status("repo"), "open");

    let txns = fx.txns();
    fx.polled("CONFLICTING", "DIRTY");
    assert_eq!((fx.txns(), fx.update_jobs().len()), (txns, 1), "the same conflict again writes nothing");
}

/// A base that merely moved queues nothing (hs-d497: no `behind` detection): `merge_state` `behind` is stored as GitHub gives it and is
/// no cause, whatever `mergeable` says. The conflict that follows does queue the Job.
#[test]
fn poll_queues_no_update_job_for_a_base_that_merely_moved_and_one_for_the_conflict_that_follows() {
    let fx = fx("deploy");
    fx.polled("MERGEABLE", "CLEAN");

    for (mergeable, state) in [("MERGEABLE", "BEHIND"), ("UNKNOWN", "BEHIND"), ("MERGEABLE", "BLOCKED"), ("MERGEABLE", "UNSTABLE")] {
        fx.polled(mergeable, state);
        assert_eq!(fx.update_jobs(), Vec::<(String, String)>::new(), "{mergeable}/{state}");
        assert_eq!(fx.shown_pull_request()["merge_state"], state.to_lowercase(), "{mergeable}/{state}: stored as GitHub says");
    }
    assert!(fx.events("job-added").is_empty());

    fx.polled("CONFLICTING", "DIRTY");

    assert_eq!(fx.update_jobs(), [("up-repo".to_owned(), "queued".to_owned())]);
}

/// "Once per conflict": no second `update` Job while one is `queued` or `running`, however often the conflict is seen again, and a conflict
/// that comes after the Job ended (`merged`) is a new cause, a new Job, `up-repo-2`.
#[test]
fn poll_queues_no_second_update_job_while_one_is_live_and_a_new_one_for_a_conflict_after_it_ended() {
    let fx = fx("deploy");
    fx.polled("CONFLICTING", "DIRTY");
    assert_eq!(fx.update_jobs(), [("up-repo".to_owned(), "queued".to_owned())]);

    // GitHub recomputes after the base moved (`unknown`) and the conflict is seen as a transition again: the Job is still queued.
    fx.polled("UNKNOWN", "UNKNOWN");
    fx.polled("CONFLICTING", "DIRTY");
    assert_eq!(fx.update_jobs(), [("up-repo".to_owned(), "queued".to_owned())]);

    // Still no second one while it runs.
    fx.store().write("fixture", |tx| Ok(tx.execute_batch("UPDATE jobs SET status = 'running' WHERE key = 'up-repo';")?)).unwrap();
    fx.polled("UNKNOWN", "UNKNOWN");
    fx.polled("CONFLICTING", "DIRTY");
    assert_eq!(fx.update_jobs(), [("up-repo".to_owned(), "running".to_owned())]);

    // It merged: the next conflict is a new transition and a new Job.
    fx.store().write("fixture", |tx| Ok(tx.execute_batch("UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'up-repo';")?)).unwrap();
    fx.polled("UNKNOWN", "UNKNOWN");
    fx.polled("CONFLICTING", "DIRTY");
    assert_eq!(fx.update_jobs(), [("up-repo".to_owned(), "merged".to_owned()), ("up-repo-2".to_owned(), "queued".to_owned())]);
}

#[test]
fn a_conflicting_pull_request_with_failed_checks_is_told_only_the_checks() {
    let fx = fx("deploy");
    fx.search(&[merge_state(pay("OPEN", None, Some("FAILURE")), "CONFLICTING", "DIRTY")]);

    fx.poll();

    let events = fx.events("pr-feedback");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1, feedback_merge("open", "failed", "none", "conflicting", "dirty", &["checks_failed"]));
    assert!(events[0].0.contains("needs attention: checks failed. Read"), "{}", events[0].0);
}

#[test]
fn a_conflicting_pull_request_no_plan_owns_gets_no_event() {
    let fx = fx("deploy");
    fx.search(&[pay("OPEN", None, None), merge_state(node("acme/web", 902, "OPEN", None, None), "CONFLICTING", "DIRTY")]);
    let events = fx.event_count();

    let out = fx.poll();

    assert_eq!(out["feedback"], json!([]));
    assert_eq!(out["changed"][0]["mergeable"], "conflicting");
    assert_eq!(fx.event_count(), events);
    let stored: (String, String) = fx
        .store()
        .read(|c| Ok(c.query_row("SELECT mergeable, merge_state FROM pull_requests WHERE number = 902", [], |r| Ok((r.get(0)?, r.get(1)?)))?))
        .unwrap();
    assert_eq!(stored, ("conflicting".to_owned(), "dirty".to_owned()));
}

#[test]
fn a_value_outside_the_mergeable_sets_fails_the_poll_and_stores_nothing() {
    let fx = fx("deploy");
    for bad in [merge_state(pay("OPEN", None, None), "MAYBE", "DIRTY"), merge_state(pay("OPEN", None, None), "CONFLICTING", "QUEUED")] {
        fx.search(&[bad]);
        let run = fx.pr("poll");
        assert_eq!(code(&run), ("gh_failed".into(), 1));
    }
    assert!(fx.events("pr-feedback").is_empty());
    assert_eq!(fx.shown_pull_request()["mergeable"], "unknown");
}

#[test]
fn poll_tells_the_plan_once_that_changes_were_requested() {
    let fx = fx("deploy");
    fx.search(&[pay("OPEN", Some("CHANGES_REQUESTED"), None)]);

    assert_eq!(fx.poll()["feedback"], json!([{"plan": "pay", "pr": "repo"}]));
    let events = fx.events("pr-feedback");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1, feedback("open", "none", "changes_requested", &["changes_requested"]));
    assert!(events[0].0.contains("needs attention: changes requested. Read"), "{}", events[0].0);

    let txns = fx.txns();
    assert_eq!(fx.poll()["feedback"], json!([]));
    assert_eq!((fx.txns(), fx.events("pr-feedback").len()), (txns, 1));

    // Approved, which tells nobody, then changes requested again.
    fx.search(&[pay("OPEN", Some("APPROVED"), None)]);
    assert_eq!(fx.poll()["feedback"], json!([]));
    fx.search(&[pay("OPEN", Some("CHANGES_REQUESTED"), None)]);
    assert_eq!(fx.poll()["feedback"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!(fx.events("pr-feedback").len(), 2);
}

#[test]
fn improvements_are_stored_and_tell_nobody() {
    let fx = fx("deploy");
    fx.search(&[pay("OPEN", None, Some("FAILURE"))]);
    fx.poll();
    assert_eq!(fx.pr_data(), ("Pay the bills".into(), "none".into(), "failed".into(), "open".into()));

    fx.search(&[pay("OPEN", Some("APPROVED"), Some("SUCCESS")).replace("Pay the bills", TITLE)]);
    let out = fx.poll();

    assert_eq!(out["feedback"], json!([]));
    assert_eq!(fx.pr_data(), (TITLE.into(), "approved".into(), "passed".into(), "open".into()));
    assert_eq!(fx.events("pr-feedback").len(), 1, "only the first poll's checks turning failed");
}

#[test]
fn poll_maps_gh_values_to_the_stored_ones() {
    let fx = fx("deploy");
    let cases = [
        // (reviewDecision, rollup state, stored decision, stored checks)
        (Some("APPROVED"), None, "approved", "none"),
        (Some("CHANGES_REQUESTED"), Some("SUCCESS"), "changes_requested", "passed"),
        (Some("REVIEW_REQUIRED"), Some("PENDING"), "none", "pending"),
        (Some(""), Some("EXPECTED"), "none", "pending"),
        (None, Some("FAILURE"), "none", "failed"),
        (Some("APPROVED"), Some("ERROR"), "approved", "failed"),
    ];
    for (review, rollup, want_review, want_checks) in cases {
        fx.search(&[pay("OPEN", review, rollup)]);
        fx.poll();
        assert_eq!(fx.pr_data(), ("Pay the bills".into(), want_review.into(), want_checks.into(), "open".into()), "{review:?} {rollup:?}");
    }
}

#[test]
fn a_draft_is_stored_and_printed() {
    let fx = fx("deploy");
    fx.search(&[pay("OPEN", None, None).replace(r#""isDraft":false"#, r#""isDraft":true"#)]);
    let out = fx.poll();
    assert_eq!(out["changed"][0]["draft"], true);
    assert_eq!(out["prs"][0]["pull_request"]["draft"], true);
}

#[test]
fn an_unowned_pull_request_that_leaves_the_search_is_deleted() {
    let fx = fx("dispatch");
    fx.search(&[node("acme/pay", 901, "OPEN", None, None), node("acme/web", 902, "OPEN", None, None)]);
    fx.poll_all();
    assert_eq!(fx.pull_requests().len(), 2);

    fx.search(&[node("acme/web", 902, "OPEN", None, None)]);
    let out = fx.poll_all();

    assert_eq!((out["removed"].clone(), out["polled"].clone(), out["changed"].clone()), (json!(1), json!(1), json!([])));
    assert_eq!(fx.pull_requests(), [("acme/web".to_owned(), 902)]);

    fx.search(&[]);
    assert_eq!(fx.poll_all()["removed"], 1);
    assert!(fx.pull_requests().is_empty());
}

#[test]
fn an_owned_row_is_never_deleted_and_a_lookup_answering_open_changes_nothing() {
    let fx = fx("deploy");
    fx.search(&[]);
    fx.lookup(&[Some(pay("OPEN", None, None))]);
    let (txns, events) = (fx.txns(), fx.event_count());

    let out = fx.poll();

    assert_eq!((out["polled"].clone(), out["changed"].clone(), out["removed"].clone()), (json!(0), json!([]), json!(0)));
    assert_eq!((fx.txns(), fx.event_count()), (txns, events));
    assert_eq!(fx.pull_requests(), [("acme/pay".to_owned(), 812)]);
    let calls = fx.gh_calls();
    assert!(is_search(&calls[0]) && is_lookup(&calls[1]) && calls.len() == 2, "{calls:?}");
    assert!(calls[1][3].contains(r#"p0: repository(owner: "acme", name: "pay") { pullRequest(number: 812) { repository { nameWithOwner } number url"#), "{}", calls[1][3]);
}

#[test]
fn an_owned_pull_request_that_left_the_search_and_is_merged_takes_the_merged_path() {
    let fx = fx("deploy");
    let (worktree, branch) = kept_attempt(&fx);
    let started = fx.env.tmux(&["new-session", "-d", "-s", "cpd-work", "-n", "pay--b-a1", &format!("sleep {SLEEP_SECONDS}")]);
    assert!(started.status.success(), "{started:?}");
    assert!(fx.env.tmux(&["new-window", "-d", "-t", "cpd-work", "-n", "other-plan", &format!("sleep {SLEEP_SECONDS}")]).status.success());
    fx.search(&[]);
    fx.lookup(&[Some(pay("MERGED", Some("APPROVED"), Some("SUCCESS")).replace("Pay the bills", TITLE))]);

    let out = fx.poll();

    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!((out["done"].clone(), out["feedback"].clone(), out["polled"].clone()), (json!(["pay"]), json!([]), json!(0)));
    assert_eq!(out["changed"], json!([{"repo": "acme/pay", "number": 812, "state": "merged", "draft": false, "checks": "passed", "review_decision": "approved", "mergeable": "unknown", "merge_state": "unknown"}]));
    assert_eq!(out["prs"], json!([{"key": "repo", "status": "merged", "pull_request": fx.shown_pull_request()}]));
    assert_eq!(fx.shown_pull_request()["merged_at"], "2026-10-14T10:00:00Z");
    assert_eq!(fx.pr_data(), (TITLE.into(), "approved".into(), "passed".into(), "merged".into()));
    assert_eq!((fx.plan_row().0, fx.pr_status("repo")), ("done".to_owned(), "merged".to_owned()));
    assert!(fx.plan_row().5.is_some());
    let done = fx.events("plan-done");
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].1, json!({"prs": [{"key": "repo", "number": 812, "url": URL, "merged_at": "2026-10-14T10:00:00Z"}]}));
    let merged = fx.events("pr-merged");
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].1, json!({"pr": "repo", "number": 812, "url": URL, "merged_at": "2026-10-14T10:00:00Z"}));
    assert!(fx.events("pr-feedback").is_empty());
    // The Attempt's worktree and branch and the Lead's (pushed, clean) are gone, the Attempt's window is killed.
    assert!(!worktree.exists());
    assert_eq!(fx.git(&fx.repo, &["branch", "--list", &branch]), "");
    assert!(!fx.lead.exists());
    assert_eq!(fx.git(&fx.repo, &["branch", "--list", "cpd/pay"]), "");
    let windows = window_names(&fx);
    assert!(!windows.contains("pay--b-a1") && windows.contains("cpd-work:other-plan"), "{windows}");

    // Done: a rerun polls again and finds nothing new.
    let txns = fx.txns();
    let again = fx.poll();
    assert_eq!((again["merged"].clone(), again["done"].clone(), again["changed"].clone()), (json!([]), json!([]), json!([])));
    assert_eq!((fx.txns(), fx.events("plan-done").len()), (txns, 1));
}

#[test]
fn poll_with_a_done_plan_reruns_its_clean_up() {
    let fx = fx("done");
    let (worktree, branch) = kept_attempt(&fx);
    fx.search(&[]);

    let out = fx.poll();

    assert_eq!(out["done"], json!([]));
    assert_eq!(out["prs"][0]["status"], "merged");
    assert!(!worktree.exists());
    assert_eq!(fx.git(&fx.repo, &["branch", "--list", &branch]), "");
    assert!(fx.lead.exists());
}

#[test]
fn an_owned_pull_request_that_left_the_search_and_is_closed_is_told_to_the_plan_once() {
    let fx = fx("deploy");
    fx.search(&[]);
    fx.lookup(&[Some(pay("CLOSED", Some("CHANGES_REQUESTED"), Some("FAILURE")))]);

    let out = fx.poll();

    assert_eq!((out["feedback"].clone(), out["merged"].clone(), out["done"].clone()), (json!([{"plan": "pay", "pr": "repo"}]), json!([]), json!([])));
    let events = fx.events("pr-feedback");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].1, feedback("closed", "failed", "changes_requested", &["checks_failed", "changes_requested", "closed"]));
    assert!(events[0].0.contains("needs attention: checks failed, changes requested, closed without a merge. Read"), "{}", events[0].0);
    assert_eq!(fx.pr_data().3, "closed");
    assert_eq!((fx.plan_row().0, fx.pr_status("repo")), ("deploy".to_owned(), "open".to_owned()), "the PR stays open in cpd");

    // Polled again: still closed, told once; the row is already right, so it is not looked up again (commands-W4).
    let txns = fx.txns();
    assert_eq!(fx.poll()["feedback"], json!([]));
    assert_eq!((fx.txns(), fx.events("pr-feedback").len()), (txns, 1));
    assert_eq!(fx.gh_calls().iter().filter(|c| is_lookup(c)).count(), 1);

    // Reopened: it is in the search again, and nobody is told. Closed again: told again.
    fx.search(&[pay("OPEN", None, None)]);
    fx.poll();
    assert_eq!(fx.pr_data().3, "open");
    assert_eq!(fx.events("pr-feedback").len(), 1);
    fx.search(&[]);
    fx.lookup(&[Some(pay("CLOSED", None, None))]);
    fx.poll();
    assert_eq!(fx.events("pr-feedback").len(), 2);
}

#[test]
fn a_pull_request_closed_without_a_merge_is_not_looked_up_again() {
    let fx = fx("deploy");
    fx.search(&[]);
    fx.lookup(&[Some(pay("CLOSED", None, None))]);
    fx.poll();
    assert_eq!(fx.pr_data().3, "closed");
    let lookups = || fx.gh_calls().iter().filter(|c| is_lookup(c)).count();
    assert_eq!(lookups(), 1);

    for _ in 0..3 {
        fx.poll();
    }

    assert_eq!(lookups(), 1, "three more ticks looked it up again");
    assert_eq!((fx.plan_row().0, fx.pr_status("repo")), ("deploy".to_owned(), "open".to_owned()));
}

#[test]
fn a_lookup_with_a_not_found_alias_and_exit_1_still_applies_the_others() {
    let fx = fx("deploy");
    fx.add_web(Web::Open);
    fx.search(&[]);
    // `gh` exits 1 when one alias is NOT_FOUND although `data` holds the others.
    fx.lookup(&[Some(pay("MERGED", None, None)), None]);
    fx.gh("lookup.exit", "1");

    let out = fx.poll();

    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!(out["done"], json!([]));
    assert_eq!((fx.pr_status("repo"), fx.pr_status("web")), ("merged".to_owned(), "open".to_owned()));
    assert_eq!(fx.phase(), "deploy");
    let calls = fx.gh_calls();
    assert!(calls[1][3].contains(r#"p0: repository(owner: "acme", name: "pay")"#) && calls[1][3].contains(r#"p1: repository(owner: "acme", name: "web")"#), "{}", calls[1][3]);
    assert_eq!(out["prs"][1]["pull_request"]["state"], "open");
}

#[test]
fn with_a_plan_the_output_adds_its_prs_and_their_rows() {
    let fx = fx("dispatch");
    fx.add_web(Web::Waiting);
    fx.pr_cmd("open", "repo").ok();
    fx.search(&[pay("OPEN", None, Some("PENDING"))]);

    let without = fx.poll_all();
    assert!(without.get("prs").is_none(), "{without}");
    let out = fx.poll();

    assert_eq!(out["polled"], 1);
    assert_eq!(out["prs"].as_array().unwrap().len(), 2);
    assert_eq!(out["prs"][0], json!({"key": "repo", "status": "open", "pull_request": fx.shown_pull_request()}));
    assert_eq!(out["prs"][0]["pull_request"]["checks"], "pending");
    assert_eq!(out["prs"][1], json!({"key": "web", "status": "waiting", "pull_request": null}));
}

#[test]
fn poll_has_no_phase_precondition_and_knows_its_plan() {
    let fx = fx("plan");
    fx.search(&[]);
    let out = fx.poll();
    assert_eq!(out["prs"], json!([{"key": "repo", "status": "waiting", "pull_request": null}]));

    let run = fx.env.cpd(&["pr", "poll", "--plan", "nope"]);
    assert_eq!(code(&run), ("plan_not_found".into(), 4));
    assert_eq!(fx.gh_calls().iter().filter(|c| is_search(c)).count(), 1, "an unknown Plan polls nothing");
}

#[test]
fn poll_has_no_pr_flag() {
    let fx = fx("deploy");
    let run = fx.pr_cmd("poll", "repo");
    assert_eq!(code(&run), ("usage".into(), 2));
    assert!(fx.gh_calls().is_empty());
}

/// A child process, killed when dropped: a test that panics leaves none behind.
struct Child(Option<std::process::Child>);

impl Child {
    fn pid(&self) -> u32 {
        self.0.as_ref().unwrap().id()
    }

    fn finish(mut self) -> std::process::Output {
        self.0.take().unwrap().wait_with_output().unwrap()
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Whether process `pid` has `path` open.
fn has_open(pid: u32, path: &Path) -> bool {
    let Ok(fds) = fs::read_dir(format!("/proc/{pid}/fd")) else { return false };
    fds.filter_map(|fd| fs::read_link(fd.ok()?.path()).ok()).any(|target| target == path)
}

#[test]
fn a_second_poll_waits_for_the_lock() {
    let fx = fx("deploy");
    fx.search(&[pay("OPEN", None, None)]);
    let path = fx.home.pr_poll_lock();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).unwrap();
    let path = path.canonicalize().unwrap();
    let held = Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap();

    let child = Child(Some(fx.env.command(CPD).args(["pr", "poll"]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap()));
    let pid = child.pid();
    // The poll has the lock file open and waits for it: it has made no gh call.
    common::wait_until(&format!("the poll to open {}", path.display()), || has_open(pid, &path));
    assert!(fx.gh_calls().is_empty(), "{:?}", fx.gh_calls());

    drop(held);
    let out = child.finish();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    assert_eq!(serde_json::from_slice::<Value>(&out.stdout).unwrap()["polled"], 1);
    assert_eq!(fx.gh_calls().len(), 1);
}

#[test]
fn poll_reports_a_failing_gh_and_changes_nothing() {
    let fx = fx("deploy");
    let (txns, events) = (fx.txns(), fx.event_count());
    fx.gh("fail", "rate limited");

    let run = fx.pr("poll");

    assert_eq!(code(&run), ("gh_failed".into(), 1));
    let details = &run.error()["details"];
    assert!(details["stderr"].as_str().unwrap().contains("rate limited"), "{details}");
    assert_eq!(details["exit"], 1);
    assert_eq!(details["argv"].as_array().unwrap()[..3], ["gh", "api", "graphql"]);
    assert_eq!((fx.txns(), fx.event_count()), (txns, events));
}

// ---------- `pr poll`: a `gh` HTTP 5xx or timeout is a transient `gh_failed` (contract §7.6 step 4, hs-d354) ----------

/// `pr poll` against a `gh` that runs `script` (with `vars` set); the `gh_failed` it exits 1 with, its `details`.
/// Nothing is written.
fn poll_failure(fx: &Fx, script: &str, vars: &[(&str, &str)]) -> Value {
    let (txns, events) = (fx.txns(), fx.event_count());
    fx.env.fake_gh(script);
    let run = fx.env.cpd_with(fx.env.root(), &["pr", "poll", "--plan", "pay"], vars);
    assert_eq!(code(&run), ("gh_failed".into(), 1), "{script}");
    assert_eq!((fx.txns(), fx.event_count()), (txns, events), "{script}");
    run.error()["details"].clone()
}

/// A `gh` that answers the search with `search.json` and runs `lookup` for the lookup of the PR the search did not list.
fn lookup_script(lookup: &str) -> String {
    format!(r#"case "$4" in *"search(type"*) cat "$(dirname "$0")/../gh/search.json" ;; *) {lookup} ;; esac"#)
}

const NGINX: &str = "<!DOCTYPE html><html><head><title>502 Bad Gateway</title></head><body><center><h1>502 Bad Gateway</h1></center><hr><center>nginx</center></body></html>";

#[test]
fn poll_says_an_http_5xx_of_the_search_is_transient() {
    let fx = fx("deploy");
    for (script, reason) in [
        ("echo 'gh: HTTP 502: Bad Gateway (https://api.github.com/graphql)' >&2; exit 1".to_owned(), "HTTP 502"),
        // nginx's page with the status in the message: the same case.
        (format!("echo '{NGINX}'; echo 'gh: HTTP 502: Bad Gateway (https://api.github.com/graphql)' >&2; exit 1"), "HTTP 502"),
        ("echo 'gh: Service Unavailable (HTTP 503)' >&2; exit 1".to_owned(), "HTTP 503"),
        // The status in stdout when stderr is empty.
        ("echo 'gh: HTTP 504: Gateway Timeout'; exit 1".to_owned(), "HTTP 504"),
    ] {
        let details = poll_failure(&fx, &script, &[]);
        assert_eq!((&details["transient"], &details["call"], &details["reason"], &details["exit"]), (&json!(true), &json!("search"), &json!(reason), &json!(1)), "{script}: {details}");
        assert_eq!(details["argv"].as_array().unwrap()[..3], ["gh", "api", "graphql"], "{details}");
    }
}

#[test]
fn poll_says_a_gh_that_hangs_past_its_deadline_is_transient_with_reason_timeout() {
    let fx = fx("deploy");
    // The 15 s kill of each call is shortened (contract §1.2, `CPD_TEST_DEADLINE_GH`).
    let vars = [("CPD_TEST_DEADLINE_GH", "0.3")];
    let details = poll_failure(&fx, &format!("exec sleep {SLEEP_SECONDS}"), &vars);
    assert_eq!((&details["transient"], &details["call"], &details["reason"]), (&json!(true), &json!("search"), &json!("timeout")), "{details}");
    // The search answers, the lookup of the PR it did not list hangs.
    let details = poll_failure(&fx, &lookup_script(&format!("exec sleep {SLEEP_SECONDS}")), &vars);
    assert_eq!((&details["transient"], &details["call"], &details["reason"]), (&json!(true), &json!("lookup"), &json!("timeout")), "{details}");
}

#[test]
fn poll_says_an_http_5xx_of_the_lookup_is_transient_and_other_lookup_failures_are_not() {
    let fx = fx("deploy");
    let details = poll_failure(&fx, &lookup_script("echo 'gh: HTTP 502: Bad Gateway' >&2; exit 1"), &[]);
    assert_eq!((&details["transient"], &details["call"], &details["reason"]), (&json!(true), &json!("lookup"), &json!("HTTP 502")), "{details}");
    for script in ["echo 'gh: HTTP 401: Bad credentials' >&2; exit 1", r#"echo '{"errors":[{"message":"Bad credentials"}]}'; exit 1"#] {
        let details = poll_failure(&fx, &lookup_script(script), &[]);
        assert_eq!((&details["transient"], &details["call"], &details["reason"]), (&json!(false), &json!("lookup"), &Value::Null), "{script}: {details}");
    }
}

/// `gh`'s own failure to parse a response that ended early (hs-d354, E5 at 2026-10-20 16:43:13Z): exit 1, this on stderr, nothing on stdout.
const GH_CUT: &str = "echo 'unexpected end of JSON input' >&2; exit 1";

#[test]
fn poll_says_a_cut_response_of_the_search_is_transient() {
    let fx = fx("deploy");
    for script in [
        GH_CUT,
        // A body that stopped mid-way holds no complete JSON.
        r#"echo '{"data":{"search":{"issueCount":1,"nodes":[{"repository":'; echo 'unexpected end of JSON input' >&2; exit 1"#,
    ] {
        let details = poll_failure(&fx, script, &[]);
        assert_eq!(
            (&details["transient"], &details["call"], &details["reason"], &details["exit"]),
            (&json!(true), &json!("search"), &json!("unexpected end of JSON input"), &json!(1)),
            "{script}: {details}"
        );
        assert!(details["stderr"].as_str().unwrap().contains("unexpected end of JSON input"), "{details}");
    }
}

#[test]
fn poll_says_a_cut_response_of_the_lookup_is_transient() {
    let fx = fx("deploy");
    let details = poll_failure(&fx, &lookup_script(GH_CUT), &[]);
    assert_eq!(
        (&details["transient"], &details["call"], &details["reason"]),
        (&json!(true), &json!("lookup"), &json!("unexpected end of JSON input")),
        "{details}"
    );
}

#[test]
fn poll_says_other_json_failures_of_gh_are_not_the_cut_response() {
    let fx = fx("deploy");
    for script in [
        // A body that arrived whole and is wrong: other `gh` parse messages.
        "echo \"invalid character '<' looking for beginning of value\" >&2; exit 1",
        "echo 'json: cannot unmarshal string into Go value of type map[string]interface {}' >&2; exit 1",
        // `gh` exit 0 with an empty body is the poll's own check, never transient, whatever stderr says.
        "echo 'unexpected end of JSON input' >&2; exit 0",
    ] {
        let details = poll_failure(&fx, script, &[]);
        assert_eq!((&details["transient"], &details["call"], &details["reason"]), (&json!(false), &json!("search"), &Value::Null), "{script}: {details}");
    }
}

#[test]
fn poll_says_every_other_gh_failure_is_not_transient() {
    let fx = fx("deploy");
    for script in [
        "echo 'gh: HTTP 401: Bad credentials' >&2; exit 1".to_owned(),
        "echo 'gh: HTTP 403: Resource not accessible' >&2; exit 1".to_owned(),
        "echo 'gh: HTTP 404: Not Found' >&2; exit 1".to_owned(),
        "echo 'gh: HTTP 429: Too Many Requests' >&2; exit 1".to_owned(),
        // An HTML page with no 5xx status in the message is not transient, whatever the exit.
        format!("echo '{NGINX}'; echo 'gh: exit status 1' >&2; exit 1"),
        format!("echo '{NGINX}'"),
        // A 2xx answer with a bad or empty body.
        "exit 0".to_owned(),
        "echo 'not json'".to_owned(),
        // GraphQL's own `errors`, a RATE_LIMITED among them.
        r#"echo '{"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded"}]}'; echo 'gh: API rate limit exceeded' >&2; exit 1"#.to_owned(),
        // `HTTP 502` in a body that is a 2xx answer is not a status.
        r#"echo '{"data":null,"errors":[{"message":"upstream said HTTP 502"}]}'"#.to_owned(),
    ] {
        let details = poll_failure(&fx, &script, &[]);
        assert_eq!((&details["transient"], &details["call"], &details["reason"]), (&json!(false), &json!("search"), &Value::Null), "{script}: {details}");
        assert!(details["argv"].is_array() && details.get("exit").is_some() && details.get("stderr").is_some(), "{details}");
    }
}

#[test]
fn poll_refuses_a_gh_answer_that_is_not_an_answer_and_writes_nothing() {
    let fx = fx("deploy");
    let (txns, events) = (fx.txns(), fx.event_count());
    let failing = pay("OPEN", None, Some("FAILURE"));
    for (name, body) in [
        ("empty", String::new()),
        ("not JSON", "<html>Unicorn!</html>".to_owned()),
        // The nodes are there, but an `errors` makes the search no answer: never "no PRs", never partial.
        ("errors", search_answer(&[failing], 4987).replace("}}}", r#"}},"errors":[{"message":"boom"}]}"#)),
    ] {
        fx.gh("search.json", &body);
        let run = fx.pr("poll");
        assert_eq!(code(&run), ("gh_failed".into(), 1), "{name}");
    }
    // The search answers; the lookup has no `data`: the whole poll fails and the search's changes are not written.
    fx.search(&[node("acme/web", 902, "OPEN", None, None)]);
    fx.gh("lookup.json", r#"{"errors":[{"message":"Bad credentials"}]}"#);
    assert_eq!(code(&fx.pr("poll")), ("gh_failed".into(), 1));
    assert_eq!((fx.txns(), fx.event_count(), fx.pull_requests().len()), (txns, events, 1));
}

/// The Dispatch column's read of the Plan: label, number, state, checks, decision, title.
fn dispatch_card(fx: &Fx) -> (String, Option<(i64, PrState, Option<(String, ReviewDecision, Checks)>)>) {
    let widgets = read::widgets(&mut fx.store(), 4, 10).unwrap();
    assert_eq!(widgets.plans.len(), 1);
    let card = widgets.plans.into_iter().next().unwrap();
    let pr = card.prs.into_iter().next().map(|pr| (pr.number, pr.state, pr.polled.map(|p| (p.title, p.decision, p.checks))));
    (card.label, pr)
}

#[test]
fn dispatch_rows_carry_label_number_state_checks_decision_and_title() {
    let fx = fx("deploy");
    // Before the first poll the row is what `pr open` wrote: the Plan's title, no review, no checks.
    assert_eq!(
        dispatch_card(&fx),
        ("Lead".into(), Some((812, PrState::Open, Some(("Pay the bills".into(), ReviewDecision::None, Checks::None)))))
    );

    fx.search(&[pay("OPEN", Some("CHANGES_REQUESTED"), Some("FAILURE")).replace("Pay the bills", TITLE)]);
    fx.poll();
    assert_eq!(
        dispatch_card(&fx),
        ("Lead".into(), Some((812, PrState::Open, Some((TITLE.into(), ReviewDecision::ChangesRequested, Checks::Failed)))))
    );

    fx.search(&[]);
    fx.lookup(&[Some(pay("MERGED", Some("APPROVED"), Some("SUCCESS")).replace("Pay the bills", TITLE))]);
    fx.poll();
    assert_eq!(
        dispatch_card(&fx),
        ("Lead".into(), Some((812, PrState::Merged, Some((TITLE.into(), ReviewDecision::Approved, Checks::Passed)))))
    );
}

#[test]
fn a_plan_without_a_pr_has_a_label_and_no_pr_row() {
    let fx = fx("dispatch");
    assert_eq!(dispatch_card(&fx), ("Lead".into(), None));
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
                "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, cancel_reason, cancel_requested_at, created_txn, created_at)
                 VALUES ('pay', 'repo', 'b', 'B', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'cancelled', 0, ?1, 'no', 't', ?2, 't')",
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

// ---------------------------------------------------------------- pr rebase

#[test]
fn rebase_onto_a_moved_base_gates_and_force_pushes_with_a_lease() {
    let fx = fx("deploy");
    fx.hooks("pre-merge = \"git rev-parse HEAD\"\n");
    let old = fx.lead_sha();
    let base = fx.advance_base("base.txt", "base\n");

    let out = fx.pr("rebase").ok();

    let new = fx.lead_sha();
    assert_ne!(new, old);
    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "repo", "rebased": true, "old": old, "new": new, "pushed": true}));
    assert!(fx.git(&fx.repo, &["merge-base", "--is-ancestor", &base, &new]).is_empty());
    // The Lead worktree is on the rebased branch and clean.
    assert_eq!(fs::read_to_string(fx.lead.join("base.txt")).unwrap(), "base\n");
    assert_eq!(fx.git(&fx.lead, &["status", "--porcelain"]), "");
    // The Gate ran after the rebase; the push followed it.
    assert_eq!(fx.gate_log().trim(), new);
    assert_eq!(fx.remote_branch("cpd/pay"), Some(new));
    // No Ledger row, no Event.
    assert!(fx.events("plan-phase").is_empty());
    assert_eq!(fx.plan_row().0, "deploy");
}

#[test]
fn rebase_with_the_base_unmoved_only_gates_and_pushes() {
    let fx = fx("deploy");
    // A Job Merged in phase pr: on the Lead Branch, not yet on the PR.
    fs::write(fx.lead.join("more.txt"), "more\n").unwrap();
    fx.git(&fx.lead, &["add", "-A"]);
    fx.git(&fx.lead, &["commit", "-q", "-m", "more"]);
    let tip = fx.lead_sha();
    assert_ne!(fx.remote_branch("cpd/pay"), Some(tip.clone()));

    let out = fx.pr("rebase").ok();

    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "repo", "rebased": false, "old": tip, "new": tip, "pushed": true}));
    assert_eq!(fx.remote_branch("cpd/pay"), Some(tip));
    // Idempotent: again, the same.
    assert_eq!(fx.pr("rebase").ok()["rebased"], false);
}

impl Fx {
    /// The Lead Branch commits `README.md` as `lead` and pushes; the base then commits it as `base`.
    /// Returns the Lead Branch's tip.
    fn conflicting(&self, lead: &[&str]) -> String {
        for text in lead {
            fs::write(self.lead.join("README.md"), format!("{text}\n")).unwrap();
            self.git(&self.lead, &["add", "-A"]);
            self.git(&self.lead, &["commit", "-q", "-m", &format!("lead {text}")]);
        }
        self.git(&self.lead, &["push", "-q", "origin", "cpd/pay"]);
        self.advance_base("README.md", "base\n");
        self.lead_sha()
    }

    /// `cpd pr rebase --plan pay <flag>`.
    fn rebase(&self, flag: &str) -> Run {
        self.env.cpd(&["pr", "rebase", "--plan", "pay", flag])
    }

    fn rebasing(&self) -> bool {
        ["rebase-merge", "rebase-apply"].iter().any(|name| {
            let path = self.git(&self.lead, &["rev-parse", "--path-format=absolute", "--git-path", name]);
            Path::new(&path).exists()
        })
    }

    /// Resolves `README.md` in the Lead worktree and stages it, as the Lead does.
    fn resolve(&self, text: &str) {
        fs::write(self.lead.join("README.md"), format!("{text}\n")).unwrap();
        self.git(&self.lead, &["add", "README.md"]);
    }
}

#[test]
fn rebase_stops_at_a_conflict_in_the_lead_worktree_and_pushes_nothing() {
    let fx = fx("deploy");
    fx.hooks("pre-merge = \"git rev-parse HEAD\"\n");
    let before = fx.conflicting(&["lead"]);

    let run = fx.pr("rebase");

    assert_eq!(code(&run), ("rebase_conflict".into(), 3));
    let details = &run.error()["details"];
    assert_eq!(details["files"], json!(["README.md"]));
    assert_eq!(details["worktree"], fx.lead.to_str().unwrap());
    assert_eq!(details["next"], "cpd pr rebase --pr repo --plan pay --continue");
    assert_eq!(details["abort"], "cpd pr rebase --pr repo --plan pay --abort");
    // The rebase waits in the Lead worktree, with markers; the ref and the remote are as they were.
    assert!(fx.rebasing());
    assert!(fs::read_to_string(fx.lead.join("README.md")).unwrap().contains("<<<<<<<"));
    assert_eq!(fx.lead_sha(), before);
    assert_eq!(fx.remote_branch("cpd/pay"), Some(before));
    // No Gate, no `gh`, no gate worktree, no Event.
    assert_eq!(fx.gate_log(), "");
    assert!(fx.gh_calls().is_empty());
    fx.assert_no_gate_worktree();
    assert!(fx.events("error").is_empty());

    // A second plain run does not start another one.
    let again = fx.pr("rebase");
    assert_eq!(code(&again), ("rebase_in_progress".into(), 3));
    assert_eq!(again.error()["details"]["next"], details["next"]);
    assert_eq!(again.error()["details"]["abort"], details["abort"]);
    assert!(fx.rebasing());
}

#[test]
fn rebase_continue_gates_and_pushes_the_resolved_rebase_with_the_lease() {
    let fx = fx("deploy");
    fx.hooks("pre-merge = \"git rev-parse HEAD\"\n");
    let before = fx.conflicting(&["lead"]);
    assert_eq!(code(&fx.pr("rebase")).0, "rebase_conflict");
    fx.resolve("resolved");

    let out = fx.rebase("--continue").ok();

    let new = fx.lead_sha();
    assert_ne!(new, before);
    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "repo", "rebased": true, "old": before, "new": new, "pushed": true}));
    assert!(!fx.rebasing());
    assert_eq!(fx.git(&fx.lead, &["status", "--porcelain"]), "");
    assert_eq!(fs::read_to_string(fx.lead.join("README.md")).unwrap(), "resolved\n");
    assert_eq!(fx.git(&fx.lead, &["rev-parse", "HEAD"]), new);
    // The Gate ran on the rebased commit, then the push.
    assert_eq!(fx.gate_log().trim(), new);
    assert_eq!(fx.remote_branch("cpd/pay"), Some(new));
    fx.assert_no_gate_worktree();
    // No Event, and nothing left to continue.
    assert!(fx.events("error").is_empty());
    assert_eq!(code(&fx.rebase("--continue")), ("no_rebase_in_progress".into(), 3));
}

#[test]
fn rebase_continue_stops_again_at_a_second_conflicting_commit() {
    let fx = fx("deploy");
    let before = fx.conflicting(&["lead 1", "lead 2"]);
    assert_eq!(code(&fx.pr("rebase")).0, "rebase_conflict");
    fx.resolve("resolved 1");

    let run = fx.rebase("--continue");

    assert_eq!(code(&run), ("rebase_conflict".into(), 3));
    assert_eq!(run.error()["details"]["files"], json!(["README.md"]));
    assert_eq!(run.error()["details"]["next"], "cpd pr rebase --pr repo --plan pay --continue");
    assert!(fx.rebasing());
    assert_eq!(fx.lead_sha(), before);
    assert_eq!(fx.remote_branch("cpd/pay"), Some(before.clone()));

    fx.resolve("resolved 2");
    let out = fx.rebase("--continue").ok();
    assert_eq!((out["rebased"].as_bool(), out["old"].as_str()), (Some(true), Some(before.as_str())));
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lead_sha()));
    assert_eq!(fs::read_to_string(fx.lead.join("README.md")).unwrap(), "resolved 2\n");
    assert!(!fx.rebasing());
}

#[test]
fn rebase_continue_refuses_while_conflict_markers_remain() {
    let fx = fx("deploy");
    let before = fx.conflicting(&["lead"]);
    assert_eq!(code(&fx.pr("rebase")).0, "rebase_conflict");

    // Unmerged.
    let run = fx.rebase("--continue");
    assert_eq!(code(&run), ("rebase_conflict".into(), 3));
    assert_eq!(run.error()["details"]["files"], json!(["README.md"]));
    assert_eq!(run.error()["details"]["next"], "cpd pr rebase --pr repo --plan pay --continue");

    // Staged with the markers in it.
    fx.git(&fx.lead, &["add", "README.md"]);
    let run = fx.rebase("--continue");
    assert_eq!(code(&run), ("rebase_conflict".into(), 3));
    assert_eq!(run.error()["details"]["files"], json!(["README.md"]));

    assert!(fx.rebasing());
    assert_eq!(fx.lead_sha(), before);
    assert_eq!(fx.remote_branch("cpd/pay"), Some(before));
    assert_eq!(fx.gate_log(), "");
}

#[test]
fn rebase_continue_and_abort_refuse_when_no_rebase_is_in_progress() {
    let fx = fx("deploy");
    let before = fx.lead_sha();

    for flag in ["--continue", "--abort"] {
        let run = fx.rebase(flag);
        assert_eq!(code(&run), ("no_rebase_in_progress".into(), 3), "{flag}");
    }
    assert_eq!(fx.lead_sha(), before);
    assert!(fx.gh_calls().is_empty());
    assert_eq!(fx.gate_log(), "");
}

#[test]
fn rebase_continue_and_abort_together_are_a_usage_error() {
    let fx = fx("deploy");
    let run = fx.env.cpd(&["pr", "rebase", "--plan", "pay", "--continue", "--abort"]);
    assert_eq!(code(&run), ("usage".into(), 2));
}

#[test]
fn rebase_abort_restores_the_branch() {
    let fx = fx("deploy");
    fx.hooks("pre-merge = \"git rev-parse HEAD\"\n");
    let before = fx.conflicting(&["lead"]);
    assert_eq!(code(&fx.pr("rebase")).0, "rebase_conflict");

    let out = fx.rebase("--abort").ok();

    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "repo", "aborted": true, "tip": before}));
    assert!(!fx.rebasing());
    assert_eq!(fx.lead_sha(), before);
    assert_eq!(fx.git(&fx.lead, &["rev-parse", "HEAD"]), before);
    assert_eq!(fx.git(&fx.lead, &["symbolic-ref", "--short", "HEAD"]), "cpd/pay");
    assert_eq!(fx.git(&fx.lead, &["status", "--porcelain"]), "");
    assert_eq!(fs::read_to_string(fx.lead.join("README.md")).unwrap(), "lead\n");
    assert_eq!(fx.remote_branch("cpd/pay"), Some(before));
    assert_eq!(fx.gate_log(), "");
    assert!(fx.events("error").is_empty());
    // Idempotent: no.
    assert_eq!(code(&fx.rebase("--abort")), ("no_rebase_in_progress".into(), 3));
}

#[test]
fn rebase_continue_does_not_overwrite_a_push_it_has_not_seen() {
    let fx = fx("deploy");
    fx.conflicting(&["lead"]);
    assert_eq!(code(&fx.pr("rebase")).0, "rebase_conflict");
    // Someone else pushes to the PR's branch during the stop.
    let clone = fx.env.root().join("repos/clone");
    fx.git(fx.env.root(), &["clone", "-q", fx.origin.to_str().unwrap(), clone.to_str().unwrap()]);
    fx.git(&clone, &["checkout", "-q", "cpd/pay"]);
    fs::write(clone.join("theirs.txt"), "theirs\n").unwrap();
    fx.git(&clone, &["add", "-A"]);
    fx.git(&clone, &["commit", "-q", "-m", "theirs"]);
    fx.git(&clone, &["push", "-q", "origin", "cpd/pay"]);
    let theirs = fx.remote_branch("cpd/pay");
    fx.resolve("resolved");

    let run = fx.rebase("--continue");

    assert_eq!(code(&run), ("git_failed".into(), 1));
    assert_eq!(fx.remote_branch("cpd/pay"), theirs);
    fx.assert_no_gate_worktree();
}

#[test]
fn rebase_continue_refuses_when_the_lead_branch_moved_during_the_stop() {
    let fx = fx("deploy");
    let before = fx.conflicting(&["lead"]);
    assert_eq!(code(&fx.pr("rebase")).0, "rebase_conflict");
    // A merge lands during the stop: the ref moves under the rebase.
    let moved = fx.git(&fx.repo, &["rev-parse", &format!("{before}^")]);
    fx.git(&fx.repo, &["update-ref", "refs/heads/cpd/pay", &moved]);
    fx.resolve("resolved");

    let run = fx.rebase("--continue");

    assert_eq!(code(&run), ("cas_lost".into(), 3));
    assert_eq!(run.error()["details"]["reason"], "lead_branch_moved");
    assert_eq!(run.error()["details"]["tip"], moved);
    assert!(fx.rebasing());
    assert_eq!(fx.remote_branch("cpd/pay"), Some(before));
}

#[test]
fn rebase_with_tracked_local_changes_that_a_conflict_needs_refuses_and_changes_nothing() {
    let fx = fx("deploy");
    let before = fx.conflicting(&["lead"]);
    fs::write(fx.lead.join("api.txt"), "edited\n").unwrap();

    let run = fx.pr("rebase");

    assert_eq!(code(&run), ("lead_worktree_dirty".into(), 3));
    assert_eq!(run.error()["details"]["files"], json!(["api.txt"]));
    assert!(!fx.rebasing());
    assert_eq!(fx.lead_sha(), before);
    assert_eq!(fs::read_to_string(fx.lead.join("api.txt")).unwrap(), "edited\n");
}

#[test]
fn rebase_refuses_a_pr_that_is_not_open() {
    for (phase, status) in [("dispatch", "dispatch"), ("done", "merged")] {
        let fx = fx(phase);
        let run = fx.pr("rebase");
        assert_eq!(code(&run), ("illegal_transition".into(), 3), "{phase}");
        assert_eq!(run.error()["details"]["from"], status);
        assert_eq!(run.error()["details"]["pr"], "repo");
    }
}

#[test]
fn rebase_refuses_a_plan_that_is_not_locked() {
    let fx = fx("plan");
    let run = fx.pr("rebase");
    assert_eq!(code(&run), ("illegal_phase".into(), 3));
    assert_eq!(run.error()["details"]["phase"], "plan");
}

#[test]
fn rebase_keeps_the_lead_worktrees_local_changes_and_reports_it_stale() {
    let fx = fx("deploy");
    fs::write(fx.lead.join("api.txt"), "edited\n").unwrap();
    let base = fx.advance_base("base.txt", "base\n");

    let out = fx.pr("rebase").ok();

    // Rebased and pushed: the local changes refuse nothing (no `dirty_target`, hs-d243).
    let new = fx.lead_sha();
    assert_eq!((out["rebased"].as_bool(), out["new"].as_str()), (Some(true), Some(new.as_str())));
    assert!(fx.git(&fx.repo, &["merge-base", "--is-ancestor", &base, &new]).is_empty());
    assert_eq!(fx.remote_branch("cpd/pay"), Some(new));
    // Rule S stopped rather than overwrite them: the edit is kept, reported once.
    assert_eq!(fs::read_to_string(fx.lead.join("api.txt")).unwrap(), "edited\n");
    assert!(!fx.lead.join("base.txt").exists());
    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let data = &errors[0].1;
    assert_eq!((data["source"].as_str(), data["code"].as_str(), data["details"]["pr"].as_str()), (Some("pr rebase"), Some("lead_worktree_stale"), Some("repo")));
    assert_eq!(data["details"]["worktree"], fx.lead.to_str().unwrap());
}

#[test]
fn rebase_of_a_pr_merged_during_its_gate_pushes_nothing() {
    let fx = fx("deploy");
    fx.hooks(&paused_gate(fx.env.root()));
    fx.git(&fx.lead, &["push", "-q", "origin", "cpd/pay"]);
    let pushed = fx.remote_branch("cpd/pay");
    fx.advance_base("base.txt", "base\n");

    let child = fx.spawn_pr("rebase");
    await_gate(fx.env.root());
    fx.store()
        .write("pr poll", |tx| {
            tx.execute_batch("UPDATE prs SET status = 'merged', merged_at = 't' WHERE plan = 'pay'")?;
            Ok(())
        })
        .unwrap();
    release_gate(fx.env.root());
    let out = child.finish();

    let out: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!((out["error"]["code"].as_str(), out["error"]["details"]["from"].as_str()), (Some("illegal_transition"), Some("merged")), "{out}");
    assert_eq!(fx.remote_branch("cpd/pay"), pushed, "nothing pushed");
    fx.assert_no_gate_worktree();
}

#[test]
fn rebase_succeeds_when_the_remote_already_contains_the_rebased_commit() {
    // What a second `pr rebase` of the PR at once can meet: the other run pushed the same R'
    // (and a merge onto it followed) after this run read its lease (Escalation nl-pr-a1-q2).
    let fx = fx("deploy");
    fx.hooks(&paused_gate(fx.env.root()));
    fx.git(&fx.lead, &["push", "-q", "origin", "cpd/pay"]);
    fx.advance_base("base.txt", "base\n");

    let child = fx.spawn_pr("rebase");
    await_gate(fx.env.root());
    let rebased = fx.lead_sha();
    fs::write(fx.lead.join("m.txt"), "m\n").unwrap();
    fx.git(&fx.lead, &["add", "-A"]);
    fx.git(&fx.lead, &["commit", "-q", "-m", "m"]);
    let merged = fx.lead_sha();
    fx.git(&fx.lead, &["push", "-q", "--force", "origin", "cpd/pay"]);
    release_gate(fx.env.root());
    let out = child.finish();

    let out: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!((out["ok"].as_bool(), out["new"].as_str()), (Some(true), Some(rebased.as_str())), "{out}");
    assert_eq!(fx.remote_branch("cpd/pay"), Some(merged), "nothing rewound");
}

#[test]
fn two_rebases_of_one_pr_at_once_both_succeed_and_the_remote_is_the_local_tip() {
    let fx = fx("deploy");
    fx.hooks("pre-merge = \"sleep 1\"\n");
    fx.git(&fx.lead, &["push", "-q", "origin", "cpd/pay"]);
    let base = fx.advance_base("base.txt", "base\n");

    let (first, second) = (fx.spawn_pr("rebase"), fx.spawn_pr("rebase"));
    let outs = [first.finish(), second.finish()];

    // The compare-and-swap picks one; the other finds the rebase done (or loses every round).
    for out in &outs {
        let json: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert!(json["ok"] == true || json["error"]["code"] == "cas_lost", "{json}");
    }
    let tip = fx.lead_sha();
    assert!(fx.git(&fx.repo, &["merge-base", "--is-ancestor", &base, &tip]).is_empty());
    assert_eq!(fx.git(&fx.repo, &["rev-list", "--count", &format!("{base}..{tip}")]), "2", "rebased once: api and hooks on the base");
    assert_eq!(fx.remote_branch("cpd/pay"), Some(tip));
    fx.assert_no_gate_worktree();
}

#[test]
fn rebase_with_a_failing_gate_keeps_the_rebased_branch_and_pushes_nothing() {
    let fx = fx("deploy");
    fx.hooks("pre-merge = \"echo red; exit 1\"\n");
    fx.git(&fx.lead, &["push", "-q", "origin", "cpd/pay"]);
    let pushed = fx.remote_branch("cpd/pay");
    let old = fx.lead_sha();
    fx.advance_base("base.txt", "base\n");

    let run = fx.pr("rebase");

    assert_eq!(code(&run), ("gate_failed".into(), 3));
    let details = &run.error()["details"];
    assert!(details["tail"].as_str().unwrap().contains("red"));
    assert!(details["log"].as_str().unwrap().ends_with("logs/pr-repo.pre-merge.log"));
    // The rebased Lead Branch stays; the remote does not move.
    assert_ne!(fx.lead_sha(), old);
    assert!(fx.lead.join("base.txt").exists());
    assert_eq!(fx.remote_branch("cpd/pay"), pushed);
}

#[test]
fn rebase_with_a_gate_that_cannot_be_judged_is_gate_error() {
    let fx = fx("deploy");
    fx.hooks("pre-merge = \"echo {{ no_such_variable }}\"\n");
    fx.git(&fx.lead, &["push", "-q", "origin", "cpd/pay"]);
    let pushed = fx.remote_branch("cpd/pay");

    let run = fx.pr("rebase");

    assert_eq!(code(&run), ("gate_error".into(), 1));
    assert_eq!(fx.remote_branch("cpd/pay"), pushed);
}

#[test]
fn rebase_does_not_overwrite_a_push_it_has_not_seen() {
    let fx = fx("deploy");
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

// ------------------------------------------------------------- two PRs (R34)

#[test]
fn open_selects_the_pr_and_the_plan_moves_to_pr_with_the_last_one() {
    let fx = fx("dispatch");
    let web = fx.add_web(Web::Dispatch);
    fx.hooks("pre-merge = \"echo gate ran in $(basename $PWD)\"\n");
    fs::create_dir_all(web.join(".config")).unwrap();
    fs::write(web.join(".config/wt.toml"), "pre-merge = \"echo gate ran in $(basename $PWD)\"\n").unwrap();
    fx.git(&web, &["add", "-A"]);
    fx.git(&web, &["commit", "-q", "-m", "hooks"]);

    // Two PRs: --pr is required.
    let run = fx.pr("open");
    assert_eq!(code(&run), ("pr_required".into(), 2));
    assert!(fx.gh_calls().is_empty());

    // `repo` opens; `web` is still in dispatch, so the Plan is.
    let out = fx.pr_cmd("open", "repo").ok();
    assert_eq!((out["phase"].as_str(), out["pr"]["key"].as_str(), out["pr"]["status"].as_str()), (Some("dispatch"), Some("repo"), Some("open")));
    assert_eq!(out["pr"]["pr"]["number"], 812);
    assert_eq!((fx.phase().as_str(), fx.pr_status("repo").as_str(), fx.pr_status("web").as_str()), ("dispatch", "open", "dispatch"));
    assert!(fx.events("plan-phase").is_empty());
    assert_eq!(fx.events("pr-opened")[0].1, json!({"pr": "repo", "number": 812, "url": URL}));
    // Its own Gate log, in its own gate worktree; its own stored title: no `(<key>)` suffix with two PRs (hs-d285).
    assert!(fx.gate_log().trim().starts_with("gate ran in pay.pay.repo.gate-"), "{}", fx.gate_log());
    let calls = fx.gh_calls();
    assert_eq!(calls[1][..8], ["pr", "create", "--base", "main", "--head", "cpd/pay", "--title", REPO_TITLE].map(String::from));

    // `web` opens: now every PR is open, and the Plan moves on.
    let out = fx.pr_cmd("open", "web").ok();
    assert_eq!((out["phase"].as_str(), out["pr"]["pr"]["number"].as_i64()), (Some("deploy"), Some(813)));
    assert_eq!(out["pr"]["pr"]["url"], WEB_URL);
    assert_eq!(fx.phase(), "deploy");
    let log = fs::read_to_string(fx.home.plan_dir(&plan()).join("logs/pr-web.pre-merge.log")).unwrap();
    assert!(log.starts_with("gate: slot 0 taken after 0 s\ngate ran in web.pay.web.gate-"), "{log}");
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lead_sha()));
    assert_eq!(fx.git(&web.parent().unwrap().join("web"), &["rev-parse", "refs/heads/cpd/pay/web"]), fx.git(&web, &["rev-parse", "HEAD"]));
    let calls = fx.gh_calls();
    let create = calls.iter().filter(|c| c[1] == "create").nth(1).unwrap();
    assert_eq!(create[2..8], ["--base", "main", "--head", "cpd/pay/web", "--title", WEB_PR_TITLE].map(String::from));
    // The body covers the PR's own Jobs only.
    assert!(create[9].contains("| ui | The UI | merged |") && !create[9].contains("| api |"), "{}", create[9]);

    let phases = fx.events("plan-phase");
    assert_eq!(phases.len(), 1);
    assert_eq!(
        phases[0].1,
        json!({"from": "dispatch", "to": "deploy", "prs": [{"key": "repo", "number": 812, "url": URL}, {"key": "web", "number": 813, "url": WEB_URL}]})
    );
    assert_eq!(fx.events("pr-opened").len(), 2);

    // Again: already, nothing more.
    let again = fx.pr_cmd("open", "web").ok();
    assert_eq!((again["already"].as_bool(), again["phase"].as_str()), (Some(true), Some("deploy")));
    assert_eq!(fx.events("pr-opened").len(), 2);
}

#[test]
fn open_refuses_a_pr_that_is_waiting_and_names_only_its_own_jobs() {
    let fx = fx("dispatch");
    fx.add_web(Web::Waiting);

    let run = fx.pr_cmd("open", "web");
    assert_eq!(code(&run), ("illegal_transition".into(), 3));
    assert_eq!(run.error()["details"]["from"], "waiting");

    let run = fx.pr_cmd("open", "nope");
    assert_eq!(code(&run), ("pr_not_found".into(), 4));

    // `web`'s queued Job does not keep `repo` from opening (the Plan stays in dispatch: `web` waits).
    let out = fx.pr_cmd("open", "repo").ok();
    assert_eq!(out["phase"], "dispatch");
    assert_eq!(fx.pr_status("web"), "waiting");
}

#[test]
fn poll_covers_every_open_pr_in_dispatch_and_pr_and_ends_the_plan_with_the_last_merge() {
    let fx = fx("dispatch");
    fx.add_web(Web::Dispatch);
    fx.pr_cmd("open", "repo").ok();
    fx.search(&[pay("OPEN", None, None)]);

    // Phase dispatch: the poll lists the Plan's PRs, the open one with its row.
    let out = fx.poll();
    let listed: Vec<(&str, &str)> = out["prs"].as_array().unwrap().iter().map(|p| (p["key"].as_str().unwrap(), p["status"].as_str().unwrap())).collect();
    assert_eq!(listed, [("repo", "open"), ("web", "dispatch")]);
    assert_eq!(out["prs"][1]["pull_request"], Value::Null);
    assert_eq!(fx.gh_calls().iter().filter(|c| is_search(c)).count(), 1);

    // `repo` is merged while `web` is still in dispatch: the PR merges, the Plan does not move.
    fx.search(&[]);
    fx.lookup(&[Some(pay("MERGED", None, None))]);
    let out = fx.poll();
    assert_eq!((out["merged"].clone(), out["done"].clone()), (json!([{"plan": "pay", "pr": "repo"}]), json!([])));
    assert_eq!((fx.phase().as_str(), fx.pr_status("repo").as_str()), ("dispatch", "merged"));
    assert_eq!(fx.events("pr-merged")[0].1, json!({"pr": "repo", "number": 812, "url": URL, "merged_at": "2026-10-14T10:00:00Z"}));
    assert!(fx.events("plan-done").is_empty());

    // `web` opens: the PRs are open or merged, so the Plan moves to pr. Only `web` is owned and open now.
    assert_eq!(fx.pr_cmd("open", "web").ok()["phase"], "deploy");
    fx.search(&[node("acme/web", 813, "OPEN", None, Some("SUCCESS"))]);
    // The row `pr open` wrote has no checks; the search's passed: that is the change.
    let out = fx.poll();
    assert_eq!((out["changed"][0]["number"].clone(), out["merged"].clone()), (json!(813), json!([])));
    assert_eq!(fx.phase(), "deploy");

    // The last PR merges: the Plan is done, with one plan-done listing both.
    fx.search(&[]);
    fx.lookup(&[Some(node("acme/web", 813, "MERGED", None, None))]);
    let out = fx.poll();
    assert_eq!((out["merged"].clone(), out["done"].clone()), (json!([{"plan": "pay", "pr": "web"}]), json!(["pay"])));
    assert_eq!((fx.phase().as_str(), fx.pr_status("web").as_str()), ("done", "merged"));
    let done = fx.events("plan-done");
    assert_eq!(done.len(), 1);
    let listed: Vec<(&str, i64)> = done[0].1["prs"].as_array().unwrap().iter().map(|p| (p["key"].as_str().unwrap(), p["number"].as_i64().unwrap())).collect();
    assert_eq!(listed, [("repo", 812), ("web", 813)]);
    assert_eq!(fx.events("pr-merged").len(), 2);
    assert!(fx.plan_row().5.is_some());

    // Done: nothing owned is open, so no lookup, and still one plan-done.
    let lookups = fx.gh_calls().iter().filter(|c| is_lookup(c)).count();
    let again = fx.poll();
    assert_eq!((again["merged"].clone(), again["done"].clone()), (json!([]), json!([])));
    assert_eq!(fx.gh_calls().iter().filter(|c| is_lookup(c)).count(), lookups);
    assert_eq!(fx.events("plan-done").len(), 1);
}

#[test]
fn a_reopened_plan_whose_new_pr_merges_is_done_again_with_a_second_plan_done() {
    let fx = fx("deploy");
    fx.search(&[]);
    fx.lookup(&[Some(pay("MERGED", None, None))]);
    assert_eq!(fx.poll()["done"], json!(["pay"]));
    assert_eq!(fx.phase(), "done");
    assert_eq!(fx.events("plan-done").len(), 1);

    // `pr add` moves the legacy branch and reopens the Plan.
    fx.env.cpd(&["pr", "add", "fix", "--repo", fx.repo.to_str().unwrap(), "--title", "Change the PR", "--shape", "config", "--plan", "pay"]).ok();
    assert_eq!(fx.phase(), "dispatch");
    // The engine would start the PR and the Lead open it: written straight, as the other tests do,
    // with the pull request's row `pr open` writes.
    fx.store()
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                   VALUES ('acme/pay', 900, 'https://github.com/acme/pay/pull/900', 'Pay the bills', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');
                 UPDATE prs SET status = 'dispatch', lead_worktree = '{lead}', started_at = 't' WHERE plan = 'pay' AND key = 'fix';
                 UPDATE prs SET status = 'open', pr_number = 900, pr_url = 'https://github.com/acme/pay/pull/900', pr_opened_at = 'opened',
                        pull_request = (SELECT id FROM pull_requests WHERE number = 900)
                  WHERE plan = 'pay' AND key = 'fix';
                 UPDATE plans SET phase = 'deploy' WHERE id = 'pay';",
                lead = fx.lead.display()
            ))?;
            Ok(())
        })
        .unwrap();

    fx.lookup(&[Some(node("acme/pay", 900, "MERGED", None, None))]);
    let out = fx.poll();
    assert_eq!((out["merged"].clone(), out["done"].clone()), (json!([{"plan": "pay", "pr": "fix"}]), json!(["pay"])));
    assert_eq!((fx.phase().as_str(), fx.pr_status("fix").as_str()), ("done", "merged"));
    let done = fx.events("plan-done");
    assert_eq!(done.len(), 2, "plan-done once each time the Plan reaches done");
    let listed: Vec<&str> = done[1].1["prs"].as_array().unwrap().iter().map(|p| p["key"].as_str().unwrap()).collect();
    assert_eq!(listed, ["fix", "repo"]);
}

#[test]
fn a_merged_pr_makes_its_dependent_startable_but_poll_does_not_start_it() {
    let fx = fx("dispatch");
    fx.add_web(Web::Waiting);
    fx.pr_cmd("open", "repo").ok();
    assert_eq!(code(&fx.env.cpd(&["pr", "start", "web", "--plan", "pay"])), ("pr_not_ready".into(), 3));

    fx.search(&[]);
    fx.lookup(&[Some(pay("MERGED", None, None))]);
    let out = fx.poll();

    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!((fx.pr_status("repo").as_str(), fx.pr_status("web").as_str(), fx.phase().as_str()), ("merged", "waiting", "dispatch"));
    assert_eq!(fx.events("pr-merged").len(), 1);
    assert!(fx.events("plan-done").is_empty());
    assert!(fx.events("pr-started").is_empty(), "pr poll never starts a PR");
    // Its need is merged now: it can start.
    let started = fx.env.cpd(&["pr", "start", "web", "--plan", "pay"]).ok();
    assert_eq!(started["pr"]["status"], "dispatch");
    assert_eq!(fx.pr_status("web"), "dispatch");
}

#[test]
fn rebase_touches_only_its_pr() {
    let fx = fx("dispatch");
    let web = fx.add_web(Web::Open);
    fx.pr_cmd("open", "repo").ok();
    assert_eq!(fx.phase(), "deploy");
    let web_repo = web.parent().unwrap().join("web");
    let (repo_before, web_before) = (fx.lead_sha(), fx.git(&web_repo, &["rev-parse", "refs/heads/cpd/pay/web"]));
    let web_remote = |fx: &Fx| fx.git(&fx.env.root().join("repos/web-origin.git"), &["rev-parse", "refs/heads/cpd/pay/web"]);
    let web_remote_before = web_remote(&fx);
    // The base moves in `web` only.
    fs::write(web_repo.join("base.txt"), "base\n").unwrap();
    fx.git(&web_repo, &["add", "-A"]);
    fx.git(&web_repo, &["commit", "-q", "-m", "base"]);
    fx.git(&web_repo, &["push", "-q", "origin", "main"]);

    // Two PRs: --pr is required.
    assert_eq!(code(&fx.pr("rebase")), ("pr_required".into(), 2));

    let out = fx.pr_cmd("rebase", "web").ok();

    let new = fx.git(&web_repo, &["rev-parse", "refs/heads/cpd/pay/web"]);
    assert_eq!(out, json!({"ok": true, "plan": "pay", "pr": "web", "rebased": true, "old": web_before, "new": new, "pushed": true}));
    assert_ne!(new, web_before);
    assert_eq!(web_remote(&fx), new);
    assert_ne!(web_remote(&fx), web_remote_before);
    assert_eq!(fs::read_to_string(web.join("base.txt")).unwrap(), "base\n");
    // `repo`'s Lead Branch, worktree and remote are as they were.
    assert_eq!(fx.lead_sha(), repo_before);
    assert_eq!(fx.remote_branch("cpd/pay"), Some(repo_before));
    assert!(fx.events("plan-phase").len() == 1 && fx.gh_calls().iter().filter(|c| c[1] == "create").count() == 1);
}

/// The `needs` of PR `key` in Plan `pay`, sorted.
fn needs_of(fx: &Fx, key: &str) -> Vec<String> {
    fx.store()
        .read(|c| {
            let mut q = c.prepare("SELECT needs FROM pr_needs WHERE plan = 'pay' AND pr = ?1 ORDER BY needs")?;
            let rows = q.query_map([key], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .unwrap()
}

fn add_fix(fx: &Fx, extra: &[&str]) -> Run {
    let mut args = vec!["pr", "add", "fix", "--repo", fx.repo.to_str().unwrap(), "--title", "Change the PR", "--shape", "config", "--plan", "pay"];
    args.extend(extra);
    fx.env.cpd(&args)
}

#[test]
fn pr_add_on_a_reopened_plan_needs_its_merged_prs_by_default() {
    let f = fx("done");
    let out = add_fix(&f, &[]).ok();
    assert_eq!(out["pr"]["needs"], json!(["repo"]));
    assert_eq!(needs_of(&f, "fix"), ["repo"]);
    assert_eq!(f.events("pr-added")[0].1["needs"], json!(["repo"]));
    // A rerun (the Plan is in dispatch now) is `already`, not `pr_exists`.
    assert_eq!(add_fix(&f, &[]).ok()["already"], true);
}

#[test]
fn pr_add_no_needs_and_needs_override_the_reopen_default() {
    let a = fx("done");
    assert_eq!(add_fix(&a, &["--no-needs"]).ok()["pr"]["needs"], json!([]));
    assert!(needs_of(&a, "fix").is_empty());

    let b = fx("done");
    b.env.cpd(&["pr", "add", "x", "--repo", b.repo.to_str().unwrap(), "--title", "Change the PR", "--shape", "config", "--plan", "pay", "--no-needs"]).ok();
    // The Plan is in dispatch now; `--needs x` names a PR that is not merged.
    assert_eq!(add_fix(&b, &["--needs", "x"]).ok()["pr"]["needs"], json!(["x"]));
    assert_eq!(needs_of(&b, "fix"), ["x"]);

    let c = fx("done");
    assert_eq!(code(&add_fix(&c, &["--needs", "repo", "--no-needs"])), ("usage".into(), 2));
    assert_eq!(c.phase(), "done");
}

#[test]
fn pr_add_on_a_plan_in_dispatch_has_no_needs_by_default() {
    let f = fx("dispatch");
    // Another repo: the fixture's own holds `cpd/pay` for its unmerged PR.
    let other = f.env.keyed_repo("other").canonicalize().unwrap();
    let out = f.env.cpd(&["pr", "add", "fix", "--repo", other.to_str().unwrap(), "--title", "Change the PR", "--shape", "config", "--plan", "pay"]).ok();
    assert_eq!(out["pr"]["needs"], json!([]));
    assert!(needs_of(&f, "fix").is_empty());
}

const GONE_URL: &str = "https://github.com/gone/shop/pull/20";

impl Fx {
    /// A second Plan `bob` in Context `ctx2` of owner `gone`, its PR `shop` open as pull request 20 of `gone/shop`.
    /// The owner file of `gone` is written, then deleted: the one owner this machine has no file for.
    fn add_bob_without_an_owner_file(&self) {
        self.env.owner_policy("gone", "");
        fs::remove_file(self.env.cpd_home().join("policy/owners/gone.toml")).unwrap();
        self.store()
            .write("fixture", |tx| {
                tx.execute_batch(&format!(
                    "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx2', 'Ctx2', 'gone', 't', 't');
                     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                       VALUES ('lead2', 'lead', 'ctx2', 'bob', 'Lead', '/', 'O3', 'idle', 't', 't');
                     INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                       VALUES ('bob', 'ctx2', 'Bob', 'deploy', 'lead2', 't', 't');
                     INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
                       VALUES ('gone/shop', 20, '{GONE_URL}', 'Pay the bills', 0, 'open', 'none', 'none', 'opened', 'opened');
                     INSERT INTO prs (plan, key, repo, base, name, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                                      pr_number, pr_url, pr_opened_at, pull_request)
                       VALUES ('bob', 'shop', '/r', 'main', 'shop', 'open', 'cpd/bob/shop', '/w', cpd_txn(), 't', 't',
                               20, '{GONE_URL}', 'opened', (SELECT id FROM pull_requests WHERE number = 20));"
                ))?;
                Ok(())
            })
            .unwrap();
    }

    /// `(plan, data)` of every `error` Event of source `pr poll` that is not a clean-up failure.
    fn policy_errors(&self) -> Vec<(String, Value)> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare(
                    "SELECT plan, data FROM events WHERE kind = 'error' AND json_extract(data, '$.source') = 'pr poll'
                        AND json_extract(data, '$.code') <> 'cleanup_failed' ORDER BY id",
                )?;
                Ok(stmt
                    .query_map([], |r| Ok((r.get::<_, String>(0)?, serde_json::from_str(&r.get::<_, String>(1)?).unwrap())))?
                    .collect::<rusqlite::Result<_>>()?)
            })
            .unwrap()
    }
}

#[test]
fn a_pr_whose_owner_file_is_gone_is_reported_and_the_other_plans_pr_still_merges() {
    let fx = fx("deploy");
    fx.add_bob_without_an_owner_file();
    // bob's checks turned failed (it would be told, were its policy known); pay's PR merged, so it is looked up.
    fx.search(&[node("gone/shop", 20, "OPEN", None, Some("FAILURE"))]);
    fx.lookup(&[Some(pay("MERGED", Some("APPROVED"), Some("SUCCESS")))]);

    let out = fx.poll_all();

    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!((out["done"].clone(), out["feedback"].clone()), (json!(["pay"]), json!([])));
    assert_eq!((fx.plan_row().0, fx.pr_status("repo")), ("done".to_owned(), "merged".to_owned()));
    assert_eq!(fx.events("pr-merged").len(), 1);
    // bob's row is written as usual, and bob is told nothing.
    let bob: String = fx.store().read(|c| Ok(c.query_row("SELECT checks FROM pull_requests WHERE number = 20", [], |r| r.get(0))?)).unwrap();
    assert_eq!(bob, "failed");
    let told: i64 = fx.store().read(|c| Ok(c.query_row("SELECT count(*) FROM events WHERE kind = 'pr-feedback'", [], |r| r.get(0))?)).unwrap();
    assert_eq!(told, 0);
    // One `error` Event names the plan, the PR and the code.
    let errors = fx.policy_errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].0, "bob");
    assert_eq!(errors[0].1["code"], "owner_unknown");
    assert_eq!(errors[0].1["details"]["plan"], "bob");
    assert_eq!(errors[0].1["details"]["pr"], "shop");
    assert_eq!(errors[0].1["details"]["owner"], "gone");
    assert!(errors[0].1["message"].as_str().unwrap().contains("PR shop (#20) of Plan bob"), "{}", errors[0].1["message"]);

    // Once per episode: the next polls, still failing the same way, append nothing and write nothing.
    let events = fx.event_count();
    fx.poll_all();
    fx.poll_all();
    assert_eq!((fx.policy_errors().len(), fx.event_count()), (1, events));
}

// ---------- stacked PRs (contract §7.6 `pr open`, §3.8 "Stacked PRs") ----------

/// PR `web` of Plan `pay`, stacked on its PR `repo` (its Lead Branch `fx.branch`, whose
/// Lead worktree is `fx.lead`): started from that branch, one Merged Job's commit on it, its Lead Branch pushed.
struct Stacked {
    lead: PathBuf,
}

fn stacked_pr(fx: &Fx) -> Stacked {
    let ship = PlanId::parse("pay").unwrap();
    let path = fx.repo.parent().unwrap().join("pay.pay.web");
    let setup = Setup { timeout: Duration::from_secs(30), log: fx.env.root().join("web.log") };
    let prepared = cpd_core::git::worktree::prepare_stacked_lead_worktree(&fx.repo, &ship, &path, "cpd/pay/web", fx.branch.as_str(), &setup).map_err(|f| f.error).unwrap();
    assert_eq!(prepared.base_sha, fx.lead_sha(), "cut from the base PR's Lead Branch head");
    prepared.push().unwrap();
    fx.git(&path, &["config", "user.name", "cpd test"]);
    fx.git(&path, &["config", "user.email", "test@cpd.invalid"]);
    fs::write(path.join("web.txt"), "web\n").unwrap();
    fx.git(&path, &["add", "-A"]);
    fx.git(&path, &["commit", "-q", "-m", "web"]);
    let merged = fx.git(&path, &["rev-parse", "HEAD"]);
    fx.store()
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "UPDATE plans SET phase = 'dispatch', done_at = NULL WHERE id = 'pay' AND phase = 'done';
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, title, created_txn, created_at, started_at)
                   VALUES ('pay', 'web', '{repo}', '{base}', 'dispatch', 'cpd/pay/web', '{path}', 'Ship the web', cpd_txn(), 't', 't');",
                repo = fx.repo.display(),
                base = fx.branch,
                path = path.display()
            ))?;
            tx.execute(
                "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, merged_sha, created_txn, created_at)
                 VALUES ('pay', 'web', 'ui', 'The UI', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'merged', 0, ?1, ?2, ?3, 't')",
                params!["f".repeat(64), merged, tx.id()],
            )?;
            Ok(())
        })
        .unwrap();
    Stacked { lead: path }
}

impl Fx {
    fn open_web(&self) -> Run {
        self.env.cpd(&["pr", "open", "--pr", "web", "--plan", "pay"])
    }
}

#[test]
fn open_of_a_stacked_pr_gates_a_head_with_the_base_prs_work_and_targets_its_lead_branch() {
    let fx = fx_stacked("dispatch");
    let web = stacked_pr(&fx);
    // The Gate sees the base PR's work in the gated commit, and its `target` is the base PR's Lead Branch, whose
    // worktree is `target_worktree_path`.
    let marker = fx.env.root().join("gate-saw");
    fs::create_dir_all(web.lead.join(".config")).unwrap();
    fs::write(
        web.lead.join(".config/wt.toml"),
        format!("pre-merge = \"test -f api.txt && test -f web.txt && echo {{{{ target }}}} {{{{ target_worktree_path }}}} > {}\"\n", marker.display()),
    )
    .unwrap();
    fx.git(&web.lead, &["add", "-A"]);
    fx.git(&web.lead, &["commit", "-q", "-m", "hooks"]);

    let out = fx.open_web().ok();

    assert_eq!((&out["phase"], &out["pr"]["status"]), (&json!("dispatch"), &json!("open")));
    assert_eq!(fs::read_to_string(&marker).unwrap().trim(), format!("{} {}", fx.branch, fx.lead.display()));
    // `gh pr create --base <the base PR's Lead Branch>`, never as a draft.
    let calls = fx.gh_calls();
    let create = calls.iter().find(|c| c[..2] == ["pr", "create"]).expect("gh pr create ran");
    let at = |flag: &str| create.iter().position(|a| a == flag).map(|i| create[i + 1].as_str());
    assert_eq!((at("--base"), at("--head")), (Some(fx.branch.as_str()), Some("cpd/pay/web")));
    assert!(!create.iter().any(|a| a.contains("draft")), "{create:?}");
    assert_eq!(fx.remote_branch("cpd/pay/web"), Some(fx.git(&fx.repo, &["rev-parse", "refs/heads/cpd/pay/web"])));
}

#[test]
fn open_of_a_stacked_pr_is_refused_while_its_base_prs_branch_is_not_on_origin() {
    let fx = fx_stacked("dispatch");
    stacked_pr(&fx);
    let pushed = fx.remote_branch("cpd/pay/web");
    fx.git(&fx.repo, &["push", "-q", "origin", "--delete", fx.branch.as_str()]);

    let run = fx.open_web();

    assert_eq!(code(&run), ("base_not_found".into(), 4));
    assert_eq!(run.error()["details"], json!({"pr": "web", "base": fx.branch.as_str(), "base_pr": "pay/repo"}));
    assert!(fx.gh_calls().iter().all(|c| c[1] == "list"), "no pull request was created: {:?}", fx.gh_calls());
    assert_eq!(fx.remote_branch("cpd/pay/web"), pushed, "nothing was pushed");
    // Its Lead pushes the base branch, and the PR opens.
    fx.git(&fx.lead, &["push", "-q", "origin", fx.branch.as_str()]);
    assert_eq!(fx.open_web().ok()["pr"]["status"], "open");
}

/// The base PR's Lead Branch (`cpd/pay`, worktree `fx.lead`) goes on with a commit writing `path`, pushed.
fn base_pr_moves_on(fx: &Fx, path: &str, content: &str) -> String {
    fs::write(fx.lead.join(path), content).unwrap();
    fx.git(&fx.lead, &["add", "-A"]);
    fx.git(&fx.lead, &["commit", "-q", "-m", "more"]);
    fx.git(&fx.lead, &["push", "-q", "origin", fx.branch.as_str()]);
    fx.lead_sha()
}

#[test]
fn open_of_a_stacked_pr_opens_as_it_is_when_its_base_pr_moved_on_without_a_conflict() {
    let fx = fx_stacked("dispatch");
    let web = stacked_pr(&fx);
    base_pr_moves_on(&fx, "more.txt", "more\n");
    let head = fx.git(&web.lead, &["rev-parse", "HEAD"]);

    let out = fx.open_web().ok();

    assert_eq!(out["pr"]["status"], "open");
    assert_eq!(fx.git(&web.lead, &["rev-parse", "HEAD"]), head, "no merge of the base PR's new commits");
    assert_eq!(fx.remote_branch("cpd/pay/web"), Some(head));
}

#[test]
fn open_of_a_stacked_pr_says_to_merge_the_base_prs_new_commits_when_they_conflict() {
    let fx = fx_stacked("dispatch");
    let web = stacked_pr(&fx);
    // The base PR's Lead Branch writes `web.txt` too, differently.
    let base_sha = base_pr_moves_on(&fx, "web.txt", "base pr web\n");
    let pushed = fx.remote_branch("cpd/pay/web");

    let run = fx.open_web();

    assert_eq!(code(&run), ("base_conflict".into(), 3));
    let details = &run.error()["details"];
    assert_eq!((&details["base"], &details["base_sha"], &details["files"]), (&json!(fx.branch.as_str()), &json!(base_sha), &json!(["web.txt"])));
    assert_eq!(
        details["next"],
        json!([format!("git -C {} merge --no-edit origin/{}", web.lead.display(), fx.branch), "cpd pr open --pr web"])
    );
    assert!(fx.gh_calls().iter().all(|c| c[1] == "list"), "{:?}", fx.gh_calls());
    assert_eq!(fx.remote_branch("cpd/pay/web"), pushed, "nothing was pushed");

    // The stacked Lead takes the new commits by merge, as `next` says, and opens.
    fx.git(&web.lead, &["merge", "-q", "--no-edit", "-X", "ours", &format!("origin/{}", fx.branch)]);
    assert_eq!(fx.open_web().ok()["pr"]["status"], "open");
}

impl Fx {
    /// PR `repo` of Plan `pay` (`done` here) is merged and GitHub deleted its branch ("Automatically delete head branches"):
    /// no Lead worktree, no local branch `cpd/pay`, none on origin.
    fn delete_base_branch(&self) {
        self.git(&self.repo, &["worktree", "remove", "--force", self.lead.to_str().unwrap()]);
        self.git(&self.repo, &["branch", "-D", self.branch.as_str()]);
        self.git(&self.repo, &["push", "-q", "origin", "--delete", self.branch.as_str()]);
        assert_eq!(self.remote_branch(self.branch.as_str()), None);
        assert_eq!(self.git(&self.repo, &["branch", "--list", self.branch.as_str()]), "");
    }

    /// The `base` of PR `web` of Plan `pay`.
    fn web_base(&self) -> String {
        self.store().read(|c| Ok(c.query_row("SELECT base FROM prs WHERE plan = 'pay' AND key = 'web'", [], |r| r.get(0))?)).unwrap()
    }

    fn web_status(&self) -> String {
        self.store().read(|c| Ok(c.query_row("SELECT status FROM prs WHERE plan = 'pay' AND key = 'web'", [], |r| r.get(0))?)).unwrap()
    }
}

#[test]
fn open_of_a_pr_stacked_on_a_merged_base_pr_whose_branch_is_gone_opens_on_the_base_prs_base() {
    // pay/repo merged (#164 in the field) and its branch is deleted everywhere; web, stacked on it, never had a pull request.
    let fx = fx_stacked("done");
    stacked_pr(&fx);
    fx.delete_base_branch();
    assert_eq!(fx.web_base(), fx.branch.as_str());
    // `pr open` reads `createdAt` of the pull request it just opened (store-W5).
    fx.gh("lookup.json", &lookup_answer(&[Some(node("acme/web", 813, "OPEN", None, None))]));

    let out = fx.open_web().ok();

    assert_eq!((&out["phase"], &out["pr"]["status"]), (&json!("deploy"), &json!("open")));
    let calls = fx.gh_calls();
    let create = calls.iter().find(|c| c[..2] == ["pr", "create"]).expect("gh pr create ran");
    let at = |flag: &str| create.iter().position(|a| a == flag).map(|i| create[i + 1].as_str());
    assert_eq!((at("--base"), at("--head")), (Some("main"), Some("cpd/pay/web")), "{create:?}");
    assert_eq!(fx.web_base(), "main");
    assert_eq!(fx.plan_events("pay", "pr-base-changed"), vec![web_unstacked(&fx.branch, "pr open")], "one Event, for web's own Plan");
    assert_eq!(fx.remote_branch("cpd/pay/web"), Some(fx.git(&fx.repo, &["rev-parse", "refs/heads/cpd/pay/web"])));
    assert_eq!(fx.remote_branch(fx.branch.as_str()), None, "the deleted branch is not pushed again either");
}

#[test]
fn open_unstacks_a_merged_base_pr_before_every_check_and_keeps_the_unstack_when_a_later_step_refuses() {
    let fx = fx_stacked("done");
    stacked_pr(&fx);
    fx.delete_base_branch();
    // Step 0: the Lead Branch has a closed pull request, which is a refusal after the unstack.
    fx.gh("list.json", r#"[{"number": 813, "url": "https://github.com/acme/pay/pull/813", "state": "CLOSED"}]"#);

    let run = fx.open_web();

    assert_eq!(code(&run), ("pr_closed".into(), 3));
    assert_eq!(fx.web_base(), "main", "the unstack is a fact about the base PR: written although the opening was refused");
    assert_eq!(fx.plan_events("pay", "pr-base-changed"), vec![web_unstacked(&fx.branch, "pr open")]);
    assert_eq!(fx.web_status(), "dispatch");
    assert!(fx.gh_calls().iter().all(|c| c[1] == "list"), "{:?}", fx.gh_calls());

    // Again: already unstacked, nothing more is written.
    let (txns, events) = (fx.txns(), fx.event_count());
    assert_eq!(code(&fx.open_web()), ("pr_closed".into(), 3));
    assert_eq!((fx.txns(), fx.event_count()), (txns, events));
}

// ------------------------------------------------------------ the test slots

/// Holds the test slot file `path` as another Gate would.
fn hold_slot(path: &Path) -> Flock<fs::File> {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = fs::OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).unwrap();
    Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap()
}

/// Whether some process holds the test slot file `path`.
fn slot_is_held(path: &Path) -> bool {
    let file = fs::OpenOptions::new().read(true).write(true).open(path).unwrap();
    Flock::lock(file, FlockArg::LockExclusiveNonblock).is_err()
}

#[test]
fn with_one_slot_a_second_gate_waits_until_the_first_lets_go_and_logs_the_wait() {
    let fx = fx("dispatch");
    fx.test_concurrency(1);
    let ran = fx.env.root().join("pre-start-ran");
    fx.hooks(&format!("pre-start = \"touch {}\"\npre-merge = \"echo gate ran\"\n", ran.display()));
    let held = hold_slot(&fx.home.test_slot(0));

    let child = fx.spawn_pr("open");
    let waiting = "gate: waiting for a slot (1 of 1 busy; test_concurrency 1)\n";
    wait_until("the Gate to log that it waits", || fx.pr_log("pre-merge").contains(waiting));
    assert!(!ran.exists(), "pre-start ran while the only slot was held");
    drop(held);
    let out = child.finish();

    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(ran.exists());
    let log = fx.pr_log("pre-merge");
    let mut lines = log.lines();
    assert_eq!(lines.next(), Some(waiting.trim_end()), "{log}");
    let taken = lines.next().unwrap();
    assert!(taken.starts_with("gate: slot 0 taken after ") && taken.ends_with(" s"), "{log}");
    assert_eq!(lines.collect::<Vec<_>>(), ["gate ran"], "the hook's output follows the lines: {log}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains(waiting) && stderr.contains(taken), "the wait is on stderr too: {stderr}");
    fx.assert_no_gate_worktree();
}

#[test]
fn both_hooks_see_the_slot_the_gate_holds_and_no_other_cpd_variable() {
    let fx = fx("dispatch");
    fx.test_concurrency(1);
    let seen = fx.env.root().join("seen");
    // `cpd` is run with CPD_HOME and more in its environment, and none of it reaches a hook.
    fx.hooks(&format!(
        "pre-start = \"env | grep '^CPD_' | sort > {seen}.start\"\npre-merge = \"env | grep '^CPD_' | sort > {seen}.merge\"\n",
        seen = seen.display()
    ));

    fx.pr("open").ok();

    for hook in ["start", "merge"] {
        let path = fx.env.root().join(format!("seen.{hook}"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "CPD_TEST_SLOT=0\n", "pre-{hook}");
    }
}

#[test]
fn a_gate_that_finds_a_free_slot_logs_it_in_the_log_only() {
    let fx = fx("dispatch");
    fx.hooks("pre-merge = \"echo gate ran\"\n");

    let child = fx.spawn_pr("open");
    let out = child.finish();

    assert!(out.status.success());
    assert_eq!(fx.pr_log("pre-merge"), "gate: slot 0 taken after 0 s\ngate ran\n");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("gate: "), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!slot_is_held(&fx.home.test_slot(0)), "the slot is released when the Gate ends");
}

#[test]
fn a_slot_held_by_a_killed_gate_is_free_at_once() {
    let fx = fx("dispatch");
    fx.test_concurrency(1);
    let root = fx.env.root().to_path_buf();
    fx.hooks(&paused_gate(&root));

    let first = fx.spawn_pr("open");
    await_gate(&root);
    assert!(slot_is_held(&fx.home.test_slot(0)), "the running Gate holds the only slot");
    // SIGKILL: no clean-up runs; its hook (a child) outlives it, and must not keep the slot.
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(first.pid() as i32), nix::sys::signal::Signal::SIGKILL).unwrap();
    first.finish();
    release_gate(&root);

    let out = fx.pr("open").ok();

    assert_eq!(out["pr"]["status"], "open");
    let log = fx.pr_log("pre-merge");
    assert!(log.starts_with("gate: slot 0 taken after 0 s\n") && !log.contains("waiting"), "{log}");
}

#[test]
fn with_two_slots_two_gates_run_together_and_a_third_waits() {
    let fx = fx("dispatch");
    fx.test_concurrency(2);
    let root = fx.env.root().to_path_buf();
    // Each Gate marks itself `in-<pid of its shell>`, waits for `gate-go`, and fails, so no Gate changes the PR.
    let go = root.join("gate-go");
    fx.hooks(&format!(
        "pre-merge = \"touch {root}/in-$$; i=0; while [ ! -e {go} ] && [ $i -lt 600 ]; do sleep 0.1; i=$((i+1)); done; exit 1\"\n",
        root = root.display(),
        go = go.display()
    ));
    let running = || fs::read_dir(&root).unwrap().filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with("in-")).count();

    let (a, b) = (fx.spawn_pr("open"), fx.spawn_pr("open"));
    wait_until("two Gates to run together", || running() == 2);
    let third = fx.spawn_pr("open");
    wait_until("the third Gate to log that it waits", || {
        fx.pr_log("pre-merge").contains("gate: waiting for a slot (2 of 2 busy; test_concurrency 2)\n")
    });
    assert_eq!(running(), 2, "the third Gate runs nothing while both slots are held");
    assert!(slot_is_held(&fx.home.test_slot(0)) && slot_is_held(&fx.home.test_slot(1)));
    release_gate(&root);

    let outs = [a.finish(), b.finish(), third.finish()];

    assert_eq!(running(), 3, "the third Gate ran once a slot was free");
    for out in &outs {
        assert_eq!(out.status.code(), Some(3), "gate_failed: {}", String::from_utf8_lossy(&out.stdout));
    }
    let waited = outs.iter().filter(|o| String::from_utf8_lossy(&o.stderr).contains("gate: waiting for a slot (2 of 2 busy")).count();
    assert_eq!(waited, 1, "only the third Gate waited");
}
