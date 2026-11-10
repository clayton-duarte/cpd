//! Publishing an adopted PR (contract §7.4, §7.6, §7.7; commands.md §4.12, §9.1, §9.9): the `open` Job's merge, `pr open`, `pr push`
//! and `plan cancel` of a PR whose pull request is not cpd's own. Its head branch is `feat/x` on a bare `origin`, its Lead Branch the
//! local `cpd/pay/web`, and the pull request is #7. The fake `gh` records its calls. Written by Job adopt-tests-pub.

use crate::common;

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use common::{Run, TestEnv};
use cpd_core::config::{Catalog, Config, TierModel};
use cpd_core::git::{Setup, create_attempt_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::merge::{self, Ctx};
use cpd_core::store::Store;
use rusqlite::params;
use serde_json::json;

const URL: &str = "https://github.com/acme/pay/pull/7";
const LIST_HEAD: [&str; 8] = [
    "pr",
    "list",
    "--head",
    "feat/x",
    "--state",
    "all",
    "--json",
    "number,url,state,createdAt",
];

/// The fake `gh`: the lookup of `feat/x` finds pull request 7, every other head has none, `pr create` would make 812, and the poll's lookup
/// answers pull request 7.
fn gh(env: &TestEnv) {
    let lookup = format!(
        r#"{{"data":{{"p0":{{"pullRequest":{{"repository":{{"nameWithOwner":"acme/pay","isArchived":false}},"number":7,"url":"{URL}","title":"Polish the review submit",
        "isDraft":false,"state":"OPEN","createdAt":"opened","updatedAt":"U1","reviewThreads":{{"nodes":[]}},"mergedAt":null,"reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN",
        "viewerDidAuthor":true,"viewerLatestReviewRequest":null,"commits":{{"nodes":[{{"commit":{{"statusCheckRollup":null}}}}]}}}}}}}}}}"#
    );
    fs::write(env.root().join("lookup.json"), lookup).unwrap();
    env.fake_gh(&format!(
        "case \"$1 $2\" in\n\
         \"pr list\") case \"$*\" in *\"--head feat/x \"*) echo '[{{\"number\":7,\"url\":\"{URL}\",\"state\":\"OPEN\",\"createdAt\":\"2099-01-01T00:00:00Z\"}}]';; *) echo '[]';; esac;;\n\
         \"pr create\") echo 'https://github.com/acme/pay/pull/812';;\n\
         \"api graphql\") cat '{}/lookup.json';;\n\
         *) echo \"unexpected gh $*\" >&2; exit 9;;\nesac",
        env.root().display()
    ));
}

/// A repo with a bare `origin` whose `feat/x` is the pull request's head (commit `B`), and PR `web` of Plan `pay` adopting it: its Lead
/// Branch `cpd/pay/web` is cut from `origin/feat/x`, with Job `api`'s commit on it when the PR is `dispatch`.
struct Fx {
    env: TestEnv,
    home: CpdHome,
    repo: PathBuf,
    origin: PathBuf,
    lead: PathBuf,
}

fn fx(status: &str) -> Fx {
    let env = TestEnv::new();
    env.owner_policy("acme", "[pr]\ngate = \"local\"\n");
    let repo = env.keyed_repo("pay").canonicalize().unwrap();
    let origin = env.root().join("repos/origin.git");
    env.git(
        env.root(),
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    );
    env.local_origin(&repo, &origin);
    env.git(
        &repo,
        &["push", "-q", "origin", "main", "main:refs/heads/feat/x"],
    );
    env.git(&repo, &["fetch", "-q", "origin", "feat/x"]);
    for (key, value) in [
        ("user.name", "cpd test"),
        ("user.email", "test@cpd.invalid"),
        ("commit.gpgsign", "false"),
    ] {
        env.git(&repo, &["config", key, value]);
    }
    let lead = repo.parent().unwrap().join("pay.pay.web");
    env.git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "cpd/pay/web",
            lead.to_str().unwrap(),
            "origin/feat/x",
        ],
    );
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(
        home.config(),
        format!(
            "version = 1\nmachine = \"t\"\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    gh(&env);

    let fx = Fx {
        env,
        home,
        repo,
        origin,
        lead,
    };
    let merged = if status == "dispatch" {
        fx.commit("api.txt")
    } else {
        String::new()
    };
    let (columns, values, pull_request) = match status {
        "open" => (
            ", pr_number, pr_url, pr_opened_at, pull_request",
            format!(", 7, '{URL}', 't', (SELECT id FROM pull_requests)"),
            format!(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                 VALUES ('acme/pay', 7, '{URL}', 'Polish the review submit', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');"
            ),
        ),
        _ => ("", String::new(), String::new()),
    };
    let phase = if status == "open" {
        "deploy"
    } else {
        "dispatch"
    };
    let sql = format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay the bills', '{phase}', 'lead1', 't', 't');
         {pull_request}
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, head_ref, created_txn, created_at, started_at{columns})
           VALUES ('pay', 'web', '{repo}', 'main', '{status}', 'cpd/pay/web', '{lead}', 'A local label', 'config', 'feat/x', cpd_txn(), 't', 't'{values});",
        repo = fx.repo.display(),
        lead = fx.lead.display(),
    );
    fx.store()
        .write("fixture", |tx| Ok(tx.execute_batch(&sql)?))
        .unwrap();
    if status == "dispatch" {
        // The PR's work is done: `pr open` wants every Job but the `open` and `update` ones merged.
        fx.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, merged_sha, created_txn, created_at)
                     VALUES ('pay', 'web', 'api', 'The API', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'merged', 0, ?1, ?2, ?3, 't')",
                    params!["f".repeat(64), merged, tx.id()],
                )?;
                Ok(())
            })
            .unwrap();
    }
    fx
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    fn git(&self, dir: &std::path::Path, args: &[&str]) -> String {
        self.env.git(dir, args)
    }

    /// Commits `name` on the Lead Branch; returns the new tip.
    fn commit(&self, name: &str) -> String {
        fs::write(self.lead.join(name), name).unwrap();
        self.git(&self.lead, &["add", "-A"]);
        self.git(
            &self.lead,
            &[
                "-c",
                "user.name=cpd test",
                "-c",
                "user.email=test@cpd.invalid",
                "commit",
                "-q",
                "-m",
                name,
            ],
        );
        self.lead_sha()
    }

    fn lead_sha(&self) -> String {
        self.git(&self.repo, &["rev-parse", "refs/heads/cpd/pay/web"])
    }

    /// What `origin` holds at `branch`, if anything.
    fn remote(&self, branch: &str) -> Option<String> {
        let out = self.git(
            &self.origin,
            &[
                "for-each-ref",
                "--format=%(objectname)",
                &format!("refs/heads/{branch}"),
            ],
        );
        (!out.is_empty()).then_some(out)
    }

    /// Every branch of `origin`.
    fn remote_branches(&self) -> Vec<String> {
        self.git(
            &self.origin,
            &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
        )
        .lines()
        .map(str::to_owned)
        .collect()
    }

    /// Moves `feat/x` on `origin` by a commit the Lead Branch lacks, through a second clone.
    fn move_remote(&self) -> String {
        let clone = self.env.root().join("clone");
        self.git(
            self.env.root(),
            &[
                "clone",
                "-q",
                self.origin.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );
        self.git(&clone, &["checkout", "-q", "feat/x"]);
        fs::write(clone.join("remote.txt"), "remote\n").unwrap();
        self.git(&clone, &["add", "-A"]);
        self.git(
            &clone,
            &[
                "-c",
                "user.name=someone",
                "-c",
                "user.email=someone@cpd.invalid",
                "commit",
                "-q",
                "-m",
                "remote",
            ],
        );
        self.git(&clone, &["push", "-q", "origin", "feat/x"]);
        self.remote("feat/x").unwrap()
    }

    fn pr(&self, command: &str) -> Run {
        self.env
            .cpd(&["pr", command, "--workflow", "web", "--plan", "pay"])
    }

    /// `(status, pr_number, pr_url)` of PR `web`.
    fn row(&self) -> (String, Option<i64>, Option<String>) {
        self.store()
            .read(|c| Ok(c.query_row("SELECT status, pr_number, pr_url FROM workflows WHERE plan = 'pay' AND key = 'web'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?))
            .unwrap()
    }

    /// The `gh pr ...` calls so far.
    fn pr_calls(&self) -> Vec<Vec<String>> {
        self.env
            .calls("gh")
            .into_iter()
            .filter(|c| c[0] == "pr")
            .collect()
    }

    /// What publishing the Lead tip as the adopted pull request's must have done: pushed to `feat/x` and nowhere else, found the pull
    /// request by its head and created none, recorded it.
    fn assert_published(&self, tip: &str) {
        assert_eq!(
            self.remote("feat/x").as_deref(),
            Some(tip),
            "origin/feat/x is the Lead tip"
        );
        assert_eq!(
            self.remote_branches(),
            ["feat/x", "main"],
            "no cpd/* branch on origin"
        );
        let calls = self.pr_calls();
        assert!(
            calls.iter().any(|c| c == &LIST_HEAD),
            "gh looked the pull request up by its head: {calls:?}"
        );
        assert!(
            calls.iter().all(|c| c[1] != "create"),
            "no gh pr create: {calls:?}"
        );
        assert_eq!(
            self.row(),
            ("open".to_owned(), Some(7), Some(URL.to_owned()))
        );
    }
}

// ---------- the `open` Job's merge, in a child process: it runs `gh`, which must be the fake one ----------

/// The Attempt `open-web-a1` of the PR's `open` Job `open-web` in `merging`, as `attempt submit` leaves it: its worktree cut from the
/// Lead Branch, nothing committed (the Worker's base did not move).
fn submitted_open_job(fx: &Fx) -> AttemptId {
    let attempt = AttemptId::parse("open-web-a1").unwrap();
    let setup = Setup {
        timeout: Duration::from_secs(30),
        log: fx.env.root().join("attempt.log"),
    };
    let made = create_attempt_worktree(
        &fx.repo,
        &fx.lead,
        &PlanId::parse("pay").unwrap(),
        &attempt,
        &setup,
    )
    .unwrap();
    let sha = fx.git(&made.path, &["rev-parse", "HEAD"]);
    fx.store()
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, kind, fingerprint, created_txn, created_at)
                 VALUES ('pay', 'web', 'open-web', 'Open the pull request', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'running', 1, 'open', ?1, ?2, 't')",
                params!["f".repeat(64), tx.id()],
            )?;
            tx.execute(
                "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, submitted_sha, submitted_at,
                                       started_at, driver_pid)
                 VALUES ('pay', 'open-web-a1', 'open-web', 1, 'S2', 'ladder', 'merging', ?1, ?2, ?3, ?4, 't', 't', 4242)",
                params![made.branch, made.path.to_str().unwrap(), made.base_sha, sha],
            )?;
            Ok(())
        })
        .unwrap();
    attempt
}

const CHILD_ATTEMPT: &str = "CPD_ADOPT_PUB_ATTEMPT";

/// Driver step A8 for the Attempt named by `CPD_ADOPT_PUB_ATTEMPT`, run by [`merge_in_child`] with the env's `PATH` (the fake `gh` first)
/// and `CPD_HOME`: the test process's own environment is not changed (`set_var` races the other tests). Prints `END <debug of the end>`.
#[test]
#[ignore = "run by merge_in_child, in a child process"]
fn child_merges_the_attempt() {
    let home =
        CpdHome::resolve(Some(std::env::var_os("CPD_HOME").expect("CPD_HOME")), None).unwrap();
    let attempt = AttemptId::parse(&std::env::var(CHILD_ATTEMPT).expect(CHILD_ATTEMPT)).unwrap();
    let config = Config::load(&home).unwrap();
    let model = |m: &str| TierModel {
        model: m.to_owned(),
        efforts: vec![0, 1, 2, 3, 4, 5],
    };
    let catalog = Catalog {
        j: cpd_core::config::TierModel {
            model: String::new(),
            efforts: Vec::new(),
        },
        pi_version: "1.2.0".to_owned(),
        o: model("p/opus"),
        s: model("p/sonnet"),
        h: model("p/haiku"),
        g: TierModel {
            model: "p/gemini".to_owned(),
            efforts: vec![],
        },
    };
    let ctx = Ctx {
        home: &home,
        config: &config,
        catalog: &catalog,
        source: "attempt run",
        metrics: &|_| None,
        classifier: cpd_core::merge::no_verdict_classifier(),
    };
    let done = merge::merge(
        &mut Store::open(&home, 5000).unwrap(),
        &ctx,
        &PlanId::parse("pay").unwrap(),
        &attempt,
    )
    .unwrap();
    println!("END {:?}", done.end);
}

/// Step A8 for `attempt`, run by this test binary again as a child (it ends with the child, no sleep): the `Debug` of its end.
fn merge_in_child(fx: &Fx, attempt: &AttemptId) -> String {
    let exe = std::env::current_exe().unwrap();
    let out = fx
        .env
        .command(exe)
        .args([
            "workflow_adopt_pub::child_merges_the_attempt",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads",
            "1",
        ])
        .env(CHILD_ATTEMPT, attempt.to_string())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "the child merge failed: {stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
        .split_once("END ")
        .unwrap_or_else(|| panic!("no END: {stdout}"))
        .1
        .lines()
        .next()
        .unwrap()
        .to_owned()
}

#[test]
fn the_open_job_adopts_the_pull_request_of_the_head_and_creates_none() {
    let fx = fx("dispatch");
    let tip = fx.lead_sha();
    assert_ne!(fx.remote("feat/x").as_deref(), Some(tip.as_str()));
    let attempt = submitted_open_job(&fx);

    let end = merge_in_child(&fx, &attempt);

    assert!(end.starts_with("Merged"), "{end}");
    fx.assert_published(&tip);
}

#[test]
fn pr_open_of_an_adopted_pr_queues_its_open_job_and_publishes_nothing() {
    let fx = fx("dispatch");
    let tip = fx.lead_sha();

    let out = fx.pr("open").ok();

    // `pr open` queues the PR's opener (contract §7.6); its merge publishes, as the test above shows.
    assert_eq!(
        (out["status"].as_str(), out["job"].as_str()),
        (Some("queued"), Some("open-web")),
        "{out}"
    );
    assert_ne!(
        fx.remote("feat/x").as_deref(),
        Some(tip.as_str()),
        "nothing is pushed by the command itself"
    );
}

#[test]
fn pr_push_pushes_to_the_head_branch() {
    let fx = fx("open");
    let tip = fx.commit("more.txt");
    assert_ne!(fx.remote("feat/x").as_deref(), Some(tip.as_str()));

    let out = fx.pr("push").ok();

    assert_eq!(
        (out["pushed"].as_bool(), out["sha"].as_str()),
        (Some(true), Some(tip.as_str())),
        "{out}"
    );
    assert_eq!(fx.remote("feat/x").as_deref(), Some(tip.as_str()));
    assert_eq!(
        fx.remote_branches(),
        ["feat/x", "main"],
        "no cpd/* branch on origin"
    );

    // A head that moved and conflicts is refused, and nothing is forced.
    let moved = fx.move_remote();
    // The commit conflicts with the moved head's, so the reconcile cannot merge it.
    let mine = fx.commit("remote.txt");
    let run = fx.pr("push");
    assert_eq!(
        (run.error()["code"].as_str(), run.exit),
        (Some("push_rejected"), 3),
        "{run:?}"
    );
    assert_eq!(
        fx.remote("feat/x").as_deref(),
        Some(moved.as_str()),
        "the remote is untouched"
    );
    assert_eq!(fx.lead_sha(), mine);
    assert_eq!(fx.remote_branches(), ["feat/x", "main"]);
}

// ---------- plan cancel ----------

/// Plan `pay` with two `open` PRs, each with its pull request open: `web`, adopted (`acme/pay#7`), and `api`, cpd's own (`acme/api#11`).
fn cancel_fx() -> TestEnv {
    let env = TestEnv::new();
    env.fake_tmux();
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(
        home.config(),
        format!(
            "version = 1\nmachine = \"t\"\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.fake_gh("case \"$1 $2\" in\n\"pr view\") echo '{\"state\":\"OPEN\"}';;\nesac\nexit 0");
    let pr = |key: &str, repo: &str, number: i64, branch: &str, head_ref: &str| {
        format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
               VALUES ('acme/{repo}', {number}, 'https://github.com/acme/{repo}/pull/{number}', 'T', 0, 'open', 'none', 'none', 't', NULL, 't');
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, head_ref, created_txn, created_at, started_at,
                              pr_number, pr_url, pr_opened_at, pull_request)
               VALUES ('pay', '{key}', '/repos/{repo}', 'main', 'open', '{branch}', '/wt/{key}', 'T', 'config', {head_ref}, cpd_txn(), 't', 't',
                       {number}, 'https://github.com/acme/{repo}/pull/{number}', 't', (SELECT id FROM pull_requests WHERE repo = 'acme/{repo}'));"
        )
    };
    env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'deploy', 'lead1', 't', 't');
         {}{}",
        pr("web", "pay", 7, "cpd/pay/web", "'feat/x'"),
        pr("api", "api", 11, "cpd/pay/api", "NULL"),
    ));
    env
}

#[test]
fn plan_cancel_leaves_an_adopted_pull_request_open() {
    let env = cancel_fx();

    let out = env.cpd(&["plan", "cancel", "--plan", "pay"]).ok();

    let closes: Vec<Vec<String>> = env
        .calls("gh")
        .into_iter()
        .filter(|c| c[..2] == ["pr", "close"])
        .collect();
    assert_eq!(
        closes,
        [["pr", "close", "11", "--repo", "acme/api"]],
        "gh pr close for the cpd PR only: {out}"
    );
    assert_eq!(
        out["closed_prs"],
        json!([{"pr": "api", "number": 11}]),
        "{out}"
    );
    assert_eq!(
        out["left_open"],
        json!([{"pr": "web", "number": 7}]),
        "{out}"
    );
}
