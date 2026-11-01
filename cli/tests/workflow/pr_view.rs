//! `pr view` (commands.md §9.8): a PR's checks, reviews and comments, read-only.

use crate::common;

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use common::fx::Fx;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::store::Store;
use serde_json::json;

const URL: &str = "https://github.com/acme/pay/pull/812";

/// The fake `gh`, in-process: `fail`, when set, makes every call fail; else `pr view` and `api` answer with their texts.
#[derive(Default)]
pub(super) struct Gh {
    fail: Mutex<Option<String>>,
    /// The second page of inline comments, when set.
    inline2: Mutex<Option<String>>,
    /// How GitHub answers the auto-merge calls of `pr post` (commands.md §9.8b).
    pub(super) auto: Mutex<AutoMerge>,
}

/// GitHub's side of squash auto-merge: the read query `autoMergeAllowed ... autoMergeRequest` and the mutation
/// `enablePullRequestAutoMerge`.
pub(super) struct AutoMerge {
    pub(super) allowed: bool,
    /// The pull request already has an auto-merge request.
    pub(super) already: bool,
    /// GitHub's error message for the mutation, when it refuses.
    pub(super) refuse: Option<String>,
    /// The repo's allowed merge methods, read beside `autoMergeAllowed`: squash, merge commit, rebase.
    pub(super) squash: bool,
    pub(super) merge: bool,
    pub(super) rebase: bool,
    /// Only the mutation fails (gh exits 1), the read succeeds.
    pub(super) fail_mutation: bool,
}

impl Default for AutoMerge {
    fn default() -> Self {
        AutoMerge {
            allowed: true,
            already: false,
            refuse: None,
            squash: true,
            merge: false,
            rebase: false,
            fail_mutation: false,
        }
    }
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        if let Some(fail) = &*self.fail.lock().unwrap() {
            return Reply::exit(1, "", fail);
        }
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        if args[..2] == ["api", "graphql"] {
            let auto = self.auto.lock().unwrap();
            let text = args.join(" ");
            if text.contains("enablePullRequestAutoMerge") {
                if auto.fail_mutation {
                    return Reply::exit(1, "", "gh: HTTP 502\n");
                }
                return match &auto.refuse {
                    Some(message) => Reply::exit(
                        1,
                        &format!(
                            "{}\n",
                            json!({"data": {"enablePullRequestAutoMerge": null}, "errors": [{"message": message}]})
                        ),
                        &format!("gh: {message}\n"),
                    ),
                    None => Reply::ok(&format!(
                        "{}\n",
                        json!({"data": {"enablePullRequestAutoMerge": {"pullRequest": {"autoMergeRequest": {"enabledAt": "2026-10-27T18:00:00Z"}}}}})
                    )),
                };
            }
            if text.contains("autoMergeAllowed") {
                let request = if auto.already {
                    json!({"enabledAt": "2026-10-27T17:00:00Z"})
                } else {
                    serde_json::Value::Null
                };
                return Reply::ok(&format!(
                    "{}\n",
                    json!({"data": {"repository": {"autoMergeAllowed": auto.allowed, "squashMergeAllowed": auto.squash, "mergeCommitAllowed": auto.merge, "rebaseMergeAllowed": auto.rebase, "pullRequest": {"id": "PR_812", "autoMergeRequest": request}}}})
                ));
            }
        }
        match args[..2] {
            ["pr", "view"] => Reply::ok(VIEW),
            ["api", endpoint]
                if endpoint.starts_with("repos/{owner}/{repo}/pulls/812/comments") =>
            {
                // `gh api --paginate` prints every page, one JSON array after the other.
                let second = self
                    .inline2
                    .lock()
                    .unwrap()
                    .clone()
                    .filter(|_| args.contains(&"--paginate"));
                Reply::ok(&format!("{INLINE}{}", second.unwrap_or_default()))
            }
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

const VIEW: &str = r#"{
  "state": "OPEN", "mergedAt": null, "title": "Pay the bills", "url": "https://github.com/acme/pay/pull/812",
  "reviewDecision": "CHANGES_REQUESTED",
  "statusCheckRollup": [
    {"__typename": "CheckRun", "name": "ci/test", "conclusion": "FAILURE", "status": "COMPLETED", "detailsUrl": "https://github.com/acme/pay/actions/runs/1"},
    {"__typename": "StatusContext", "context": "ci/lint", "state": "SUCCESS", "targetUrl": "https://lint.example/1"},
    {"__typename": "StatusContext", "context": "ci/old", "state": "SUCCESS"}
  ],
  "reviews": [{"id": "PRR_1", "author": {"login": "reviewer"}, "state": "CHANGES_REQUESTED", "body": "Please add a test."}],
  "comments": [{"id": "IC_1", "author": {"login": "reviewer"}, "body": "Thanks for the fix."}]
}"#;

const INLINE: &str = r#"[{"id": 1893, "user": {"login": "reviewer"}, "path": "src/payout.rs", "body": "This retry never backs off."}]"#;

pub(super) struct Pay {
    pub(super) fx: Fx,
    home: CpdHome,
    pub(super) gh: Arc<Gh>,
}

/// Plan `pay` with one PR `repo` in `status`; its Lead worktree is a plain directory (the fake `gh` ignores it).
pub(super) fn fx(status: &str) -> Pay {
    let fx = Fx::with_ports();
    let env = &fx.env;
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(
        home.config(),
        format!(
            "version = 1\nmachine = \"t\"\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.owner_policy("acme", "[pr]\ngate = \"local\"\n");
    let gh = Arc::new(Gh::default());
    let _ = (*fx.fakes.runner).clone().route("gh", gh.clone());
    let lead: PathBuf = env.root().join("lead");
    fs::create_dir_all(&lead).unwrap();
    let (columns, values) = match status {
        "open" => (
            ", pr_number, pr_url, pr_opened_at, pull_request",
            format!(", 812, '{URL}', 'opened', (SELECT id FROM pull_requests)"),
        ),
        _ => ("", String::new()),
    };
    let (worktree, started) = if status == "waiting" {
        ("NULL", "NULL")
    } else {
        ("'LEAD'", "'t'")
    };
    let sql = format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay the bills', '{phase}', 'lead1', 't', {locked});
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
           VALUES ('acme/pay', 812, '{URL}', 'Pay the bills', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at{columns})
           VALUES ('pay', 'repo', '/nowhere', 'main', '{status}', 'cpd/pay', {worktree}, cpd_txn(), 't', {started}{values});",
        phase = if status == "open" { "deploy" } else { "plan" },
        locked = if status == "open" { "'t'" } else { "NULL" },
    )
    .replace("LEAD", &lead.display().to_string());
    let pay = Pay { fx, home, gh };
    pay.store()
        .write("fixture", |tx| tx.execute_batch(&sql).map_err(Into::into))
        .unwrap();
    pay
}

impl Pay {
    pub(super) fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    fn count(&self, table: &str) -> i64 {
        self.store()
            .read(|c| Ok(c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?))
            .unwrap()
    }

    fn gh_inline2(&self, text: &str) {
        *self.gh.inline2.lock().unwrap() = Some(format!("{text}\n"));
    }

    pub(super) fn gh_fails(&self, stderr: &str) {
        *self.gh.fail.lock().unwrap() = Some(format!("{stderr}\n"));
    }

    fn gh_calls(&self) -> Vec<Vec<String>> {
        self.fx
            .calls("gh")
            .into_iter()
            .map(|call| call.args)
            .collect()
    }
}

#[test]
fn view_prints_the_checks_reviews_comments_and_inline_comments() {
    let fx = fx("open");

    let out = fx.fx.cpd(&["pr", "view", "--workflow", "repo", "--plan", "pay"]).ok();

    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay",
            "pr": {"key": "repo", "number": 812, "url": URL, "title": "Pay the bills", "state": "open",
                   "review_decision": "changes_requested", "checks": "failed", "jira": null},
            "checks": [
                {"name": "ci/test", "conclusion": "FAILURE", "url": "https://github.com/acme/pay/actions/runs/1"},
                {"name": "ci/lint", "conclusion": "SUCCESS", "url": "https://lint.example/1"},
                {"name": "ci/old", "conclusion": "SUCCESS"}],
            "reviews": [{"id": "PRR_1", "author": "reviewer", "state": "CHANGES_REQUESTED", "body": "Please add a test."}],
            "comments": [{"id": "IC_1", "author": "reviewer", "body": "Thanks for the fix."}],
            "inline_comments": [{"id": "1893", "author": "reviewer", "path": "src/payout.rs", "body": "This retry never backs off."}], "feedback": null, "post": null})
    );
    let gh = fx.gh_calls();
    assert_eq!(gh[0][..3], ["pr", "view", "812"]);
    assert_eq!(
        gh[0][4],
        "state,mergedAt,statusCheckRollup,reviews,comments,title,reviewDecision,url"
    );
    assert_eq!(
        gh[1],
        [
            "api",
            "repos/{owner}/{repo}/pulls/812/comments?per_page=100",
            "--paginate"
        ]
    );
}

#[test]
fn view_reads_every_page_of_inline_comments() {
    let fx = fx("open");
    fx.gh_inline2(r#"[{"id": 1894, "user": {"login": "other"}, "path": "src/lib.rs", "body": "Second page."}]"#);

    let out = fx.fx.cpd(&["pr", "view", "--workflow", "repo", "--plan", "pay"]).ok();

    assert_eq!(
        out["inline_comments"],
        json!([{"id": "1893", "author": "reviewer", "path": "src/payout.rs", "body": "This retry never backs off."},
               {"id": "1894", "author": "other", "path": "src/lib.rs", "body": "Second page."}])
    );
}

#[test]
fn view_without_flags_selects_the_plans_only_pr() {
    let fx = fx("open");
    let out = fx.fx.cpd(&["pr", "view", "--plan", "pay"]).ok();
    assert_eq!(out["pr"]["key"], "repo");
}

#[test]
fn view_writes_nothing_and_appends_no_event() {
    let fx = fx("open");
    let (txns, events) = (fx.count("txns"), fx.count("events"));

    fx.fx.cpd(&["pr", "view", "--plan", "pay"]).ok();

    assert_eq!((fx.count("txns"), fx.count("events")), (txns, events));
}

#[test]
fn view_refuses_a_waiting_pr() {
    let fx = fx("waiting");

    let run = fx.fx.cpd(&["pr", "view", "--plan", "pay"]);

    assert_eq!(
        (run.error()["code"].as_str().unwrap(), run.exit),
        ("illegal_transition", 3)
    );
    assert!(fx.gh_calls().is_empty());
}

#[test]
fn view_reports_a_failing_gh_as_gh_failed() {
    let fx = fx("open");
    fx.gh_fails("HTTP 401: bad credentials");
    let txns = fx.count("txns");

    let run = fx.fx.cpd(&["pr", "view", "--plan", "pay"]);

    assert_eq!(
        (run.error()["code"].as_str().unwrap(), run.exit),
        ("gh_failed", 1)
    );
    let details = &run.error()["details"];
    assert_eq!(
        details["argv"].as_array().unwrap()[..3],
        [json!("gh"), json!("pr"), json!("view")]
    );
    assert!(
        details["stderr"]
            .as_str()
            .unwrap()
            .contains("bad credentials"),
        "{details}"
    );
    assert_eq!(fx.count("txns"), txns);
}

#[test]
fn view_prints_the_post_of_a_posted_pr() {
    let fx = fx("open");
    let link = "https://missionlane.slack.com/archives/C02JU6C2Z1D/p1793040197490209";
    let mut store = fx.store();
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, reaction, reacted_at)
                 VALUES ('pay', 'repo', 'C02JU6C2Z1D', '1793040197.490209', '{link}', 'opened', 'git-merge', NULL)"
            ))
            .map_err(Into::into)
        })
        .unwrap();

    let out = fx.fx.cpd(&["pr", "view", "--workflow", "repo", "--plan", "pay"]).ok();

    assert_eq!(
        out["post"],
        json!({"channel": "C02JU6C2Z1D", "ts": "1793040197.490209", "link": link, "posted_at": "opened", "reaction": "git-merge", "reacted_at": null, "refusal": null})
    );
}

/// `pr view` of an open PR whose head holds `checks`, under the owner policy `body`: the checks summary it prints.
fn summary_of(body: &str, checks: Vec<serde_json::Value>) -> serde_json::Value {
    let fx = fx("open");
    fx.fx.env.owner_policy("acme", body);
    let view = json!({"state": "OPEN", "mergedAt": null, "title": "Pay the bills", "url": URL, "reviewDecision": null, "statusCheckRollup": checks, "reviews": [], "comments": []});
    let _ = (*fx.fx.fakes.runner).clone().on("gh", |c| c.args.starts_with(&["pr".into(), "view".into()]), Reply::ok(&format!("{view}\n")));
    fx.fx.cpd(&["pr", "view", "--workflow", "repo", "--plan", "pay"]).ok()["pr"]["checks"].clone()
}

fn guardian() -> serde_json::Value {
    json!({"__typename": "StatusContext", "context": "GitGuardian Security Checks", "state": "SUCCESS"})
}

#[test]
fn view_reports_pending_checks_when_a_required_check_is_missing() {
    assert_eq!(
        summary_of("[pr]\nrequired_checks = [\"gate\"]\n", vec![guardian()]),
        "pending"
    );
}

#[test]
fn view_with_an_empty_required_list_or_a_passed_required_check_reports_passed() {
    assert_eq!(
        summary_of("[pr]\nrequired_checks = []\n", vec![guardian()]),
        "passed"
    );
    let gate = json!({"__typename": "CheckRun", "name": "gate", "status": "COMPLETED", "conclusion": "SUCCESS"});
    assert_eq!(
        summary_of(
            "[pr]\nrequired_checks = [\"gate\"]\n",
            vec![guardian(), gate]
        ),
        "passed"
    );
}
