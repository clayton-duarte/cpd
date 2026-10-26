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
struct Gh {
    fail: Mutex<Option<String>>,
    /// The second page of inline comments, when set.
    inline2: Mutex<Option<String>>,
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        if let Some(fail) = &*self.fail.lock().unwrap() {
            return Reply::exit(1, "", fail);
        }
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..2] {
            ["pr", "view"] => Reply::ok(VIEW),
            ["api", endpoint] if endpoint.starts_with("repos/{owner}/{repo}/pulls/812/comments") => {
                // `gh api --paginate` prints every page, one JSON array after the other.
                let second = self.inline2.lock().unwrap().clone().filter(|_| args.contains(&"--paginate"));
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
    gh: Arc<Gh>,
}

/// Plan `pay` with one PR `repo` in `status`; its Lead worktree is a plain directory (the fake `gh` ignores it).
pub(super) fn fx(status: &str) -> Pay {
    let fx = Fx::with_ports();
    let env = &fx.env;
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), format!("version = 1\nmachine = \"t\"\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket())).unwrap();
    let gh = Arc::new(Gh::default());
    let _ = (*fx.fakes.runner).clone().route("gh", gh.clone());
    let lead: PathBuf = env.root().join("lead");
    fs::create_dir_all(&lead).unwrap();
    let (columns, values) = match status {
        "open" => (", pr_number, pr_url, pr_opened_at, pull_request", format!(", 812, '{URL}', 'opened', (SELECT id FROM pull_requests)")),
        _ => ("", String::new()),
    };
    let (worktree, started) = if status == "waiting" { ("NULL", "NULL") } else { ("'LEAD'", "'t'") };
    let sql = format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay the bills', '{phase}', 'lead1', 't', {locked});
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
           VALUES ('acme/pay', 812, '{URL}', 'Pay the bills', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at{columns})
           VALUES ('pay', 'repo', '/nowhere', 'main', '{status}', 'cpd/pay', {worktree}, cpd_txn(), 't', {started}{values});",
        phase = if status == "open" { "deploy" } else { "plan" },
        locked = if status == "open" { "'t'" } else { "NULL" },
    )
    .replace("LEAD", &lead.display().to_string());
    let pay = Pay { fx, home, gh };
    pay.store().write("fixture", |tx| tx.execute_batch(&sql).map_err(Into::into)).unwrap();
    pay
}

impl Pay {
    pub(super) fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    fn count(&self, table: &str) -> i64 {
        self.store().read(|c| Ok(c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?)).unwrap()
    }

    fn gh_inline2(&self, text: &str) {
        *self.gh.inline2.lock().unwrap() = Some(format!("{text}\n"));
    }

    fn gh_fails(&self, stderr: &str) {
        *self.gh.fail.lock().unwrap() = Some(format!("{stderr}\n"));
    }

    fn gh_calls(&self) -> Vec<Vec<String>> {
        self.fx.calls("gh").into_iter().map(|call| call.args).collect()
    }
}

#[test]
fn view_prints_the_checks_reviews_comments_and_inline_comments() {
    let fx = fx("open");

    let out = fx.fx.cpd(&["pr", "view", "--pr", "repo", "--plan", "pay"]).ok();

    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay",
            "pr": {"key": "repo", "number": 812, "url": URL, "title": "Pay the bills", "state": "open",
                   "review_decision": "changes_requested", "checks": "failed"},
            "checks": [
                {"name": "ci/test", "conclusion": "FAILURE", "url": "https://github.com/acme/pay/actions/runs/1"},
                {"name": "ci/lint", "conclusion": "SUCCESS", "url": "https://lint.example/1"},
                {"name": "ci/old", "conclusion": "SUCCESS"}],
            "reviews": [{"id": "PRR_1", "author": "reviewer", "state": "CHANGES_REQUESTED", "body": "Please add a test."}],
            "comments": [{"id": "IC_1", "author": "reviewer", "body": "Thanks for the fix."}],
            "inline_comments": [{"id": "1893", "author": "reviewer", "path": "src/payout.rs", "body": "This retry never backs off."}]})
    );
    let gh = fx.gh_calls();
    assert_eq!(gh[0][..3], ["pr", "view", "812"]);
    assert_eq!(gh[0][4], "state,mergedAt,statusCheckRollup,reviews,comments,title,reviewDecision,url");
    assert_eq!(gh[1], ["api", "repos/{owner}/{repo}/pulls/812/comments?per_page=100", "--paginate"]);
}

#[test]
fn view_reads_every_page_of_inline_comments() {
    let fx = fx("open");
    fx.gh_inline2(r#"[{"id": 1894, "user": {"login": "other"}, "path": "src/lib.rs", "body": "Second page."}]"#);

    let out = fx.fx.cpd(&["pr", "view", "--pr", "repo", "--plan", "pay"]).ok();

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

    assert_eq!((run.error()["code"].as_str().unwrap(), run.exit), ("illegal_transition", 3));
    assert!(fx.gh_calls().is_empty());
}

#[test]
fn view_reports_a_failing_gh_as_gh_failed() {
    let fx = fx("open");
    fx.gh_fails("HTTP 401: bad credentials");
    let txns = fx.count("txns");

    let run = fx.fx.cpd(&["pr", "view", "--plan", "pay"]);

    assert_eq!((run.error()["code"].as_str().unwrap(), run.exit), ("gh_failed", 1));
    let details = &run.error()["details"];
    assert_eq!(details["argv"].as_array().unwrap()[..3], [json!("gh"), json!("pr"), json!("view")]);
    assert!(details["stderr"].as_str().unwrap().contains("bad credentials"), "{details}");
    assert_eq!(fx.count("txns"), txns);
}
