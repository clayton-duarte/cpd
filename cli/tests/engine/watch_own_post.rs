//! Engine step E11 confirms the User's own Slack post of a pull request (decisions of 2026-11-05 EST; contract §3.7):
//! for a Workflow whose pull request is open, that has NO `pr_posts` row and whose owner policy enables `pr.post`, the
//! EARLIEST message of the team channel authored by `inbox.slack_user_id` (the helper's `author_id`) that carries the
//! PR's github url is its post. E11 then writes `pr_posts` (channel, ts, link) with that message's ts. Messages by
//! others and PRs that already have a row change nothing.
//!
//! In-process, on virtual time, like `watch_team.rs`: the helper is the `node` route of the `FakeRunner` (never a real
//! Slack or node); the engine runs until the poll's cache is written and a few ticks have passed.

use crate::common;

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use common::fx::Fx;
use cpd_core::clock::{Clock, ManualClock};
use cpd_core::host::runner::{Call, Handler, Reply};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

const CHANNEL: &str = "C1";
const ME: &str = "U1";
const NOBODY_REVIEWED: &str = r#"{"data":{"viewer":{"login":"clayton-duarte"}}}"#;

/// One virtual tick (`watch.tick_seconds`).
const TICK: Duration = Duration::from_secs(1);

/// The `node` route: the Slack helper, answering the one fixed JSON.
struct Helper(String);

impl Handler for Helper {
    fn handle(&self, _call: &Call) -> Reply {
        Reply::exit(0, &format!("{}\n", self.0), "")
    }
}

/// A post of the helper's JSON: `author_id` is the Slack id of its author.
fn post(author_id: &str, number: u64, ts_raw: &str) -> Value {
    json!({"repo": "acme/pay", "number": number, "ts": ts_raw.parse::<f64>().unwrap(), "ts_raw": ts_raw,
           "author": "Someone", "author_id": author_id, "primary": true, "reactions": [], "claim": null,
           "eyes_me": false, "thread_tags_me": false})
}

struct Own {
    fx: Fx,
}

/// Plan `pay` with open pull requests 812 and 813 of `acme/pay` (Workflows `one` and `two`), the owner policy enabling
/// `pr.post`, and the helper answering `posts`. Workflow `two` already has a `pr_posts` row.
fn own(posts: Vec<Value>) -> Own {
    let fx = Fx::with_ports();
    let adapter = fx.env.root().join("adapter");
    fs::create_dir_all(&adapter).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"m1\"\npi_adapter = \"{}\"\n[watch]\ntick_seconds = 1\npr_poll_seconds = 3600\nteam_review_seconds = 1800\n[inbox]\nteam_channel = \"{CHANNEL}\"\nslack_user_id = \"{ME}\"\n[tmux]\nsocket = \"{}\"\n",
        adapter.display(),
        fx.env.tmux_socket()
    ));
    fx.env.owner_policy("acme", "[pr.post]\nenabled = true\n");
    fx.env.quiet_models(fx.fakes.clock.utc());
    fx.engine_routes();
    fx.install_pi();
    fx.fake_gh(Reply::ok(NOBODY_REVIEWED));
    let answer = json!({"ok": true, "channel": CHANNEL, "read": posts.len(), "posts": posts});
    let _ = (*fx.fakes.runner)
        .clone()
        .route("node", Arc::new(Helper(answer.to_string())));
    let now = fx.stamp_ago(0);
    let mut sql = format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('pay-lead', 'lead', 'ctx', 'pay', 'Lead pay', '/', 'O3', 'stopped', '{now}', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay', 'done', 'pay-lead', 't', 't', '{now}');"
    );
    for (key, number) in [("one", 812), ("two", 813)] {
        sql.push_str(&format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
               VALUES ('acme/pay', {number}, 'https://github.com/acme/pay/pull/{number}', 'PR {key}', 0, 'open', 'none', 'none', 't', NULL, 't');
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at, merged_at)
               VALUES ('pay', '{key}', '/nowhere', 'main', 'open', 'cpd/pay/{key}', '/nowhere/{key}', {number}, 'https://github.com/acme/pay/pull/{number}', 't',
                       (SELECT id FROM pull_requests WHERE repo = 'acme/pay' AND number = {number}), cpd_txn(), 't', 't', NULL);"
        ));
    }
    sql.push_str(
        "INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at)
           VALUES ('pay', 'two', 'C1', '1700000000.000001', 'https://acme.slack.com/archives/C1/p1700000000000001', 't');",
    );
    fx.env.sql(&sql);
    Own { fx }
}

impl Own {
    fn db(&self) -> PathBuf {
        self.fx.env.cpd_home().join("cpd.db")
    }

    /// The engine runs until the helper's answer is cached and three more ticks passed.
    fn run(&self) {
        let clock: ManualClock = (*self.fx.fakes.clock).clone();
        let start = clock.elapsed();
        let cache = self.fx.env.cpd_home().join("run/team-review.json");
        let mut since = None;
        self.fx
            .watch_steps(move |_| {
                let now = clock.elapsed() - start;
                if since.is_none() && cache.exists() {
                    since = Some(now);
                }
                since.is_some_and(|at| now - at >= TICK * 3)
            })
            .unwrap();
    }

    /// `(workflow, channel, ts, link)` of every `pr_posts` row, by workflow.
    fn rows(&self) -> Vec<(String, String, String, String)> {
        let conn =
            Connection::open_with_flags(self.db(), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let mut stmt = conn
            .prepare("SELECT workflow, channel, ts, link FROM pr_posts ORDER BY workflow")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }
}

const TWO_ROW: (&str, &str, &str, &str) = (
    "two",
    "C1",
    "1700000000.000001",
    "https://acme.slack.com/archives/C1/p1700000000000001",
);

fn only_two_row(rows: &[(String, String, String, String)]) -> bool {
    rows.len() == 1
        && rows[0]
            == (
                TWO_ROW.0.into(),
                TWO_ROW.1.into(),
                TWO_ROW.2.into(),
                TWO_ROW.3.into(),
            )
}

#[test]
fn e11_records_the_users_own_post_of_a_pr_with_no_row() {
    let t = own(vec![post(ME, 812, "1730000000.123456")]);
    t.run();
    let rows = t.rows();
    assert_eq!(
        rows.len(),
        2,
        "one new row besides the existing one: {rows:?}"
    );
    let (workflow, channel, ts, link) = &rows[0];
    assert_eq!(workflow, "one");
    assert_eq!(channel, CHANNEL);
    assert_eq!(ts, "1730000000.123456");
    assert!(!link.is_empty(), "the row has a link");
}

#[test]
fn e11_builds_the_permalink_from_channel_and_ts() {
    let t = own(vec![post(ME, 812, "1730000000.123456")]);
    t.run();
    let rows = t.rows();
    let link = rows
        .iter()
        .find(|r| r.0 == "one")
        .map(|r| r.3.clone())
        .expect("a row for workflow `one`");
    assert!(
        link.starts_with("https://") && link.ends_with("/archives/C1/p1730000000123456"),
        "{link}"
    );
    assert!(link.contains(".slack.com/"), "{link}");
}

#[test]
fn e11_ignores_posts_by_others_and_prs_that_already_have_a_row() {
    let t = own(vec![
        post("U2", 812, "1730000000.123456"),
        post(ME, 813, "1730000001.000001"),
    ]);
    t.run();
    assert!(
        t.fx.env.cpd_home().join("run/team-review.json").exists(),
        "E11 ran"
    );
    let rows = t.rows();
    assert!(only_two_row(&rows), "pr_posts is unchanged: {rows:?}");
}

#[test]
fn e11_takes_the_earliest_own_post_when_there_are_several() {
    let t = own(vec![
        post(ME, 812, "1730000500.000002"),
        post(ME, 812, "1730000100.000001"),
    ]);
    t.run();
    let rows = t.rows();
    let row = rows
        .iter()
        .find(|r| r.0 == "one")
        .unwrap_or_else(|| panic!("a row for workflow `one`: {rows:?}"));
    assert_eq!(row.2, "1730000100.000001");
}
