//! `pr repost` (commands.md §9.8b2, pro-d388): one reminder in the Slack thread of a Workflow's post, through the helper (`node`, a
//! scripted fake here, never a real Slack); on success `last_posted_at` moves and nothing else of the `pr_posts` row does.

use std::fs;

use cpd_core::host::runner::Reply;
use serde_json::json;

use super::pr_view::{Pay, fx};
use crate::common::refused;

const FIRST: &str = "2026-01-01T00:00:00.000Z";
const AUTH: &str = r#"{"ok":false,"error":{"code":"slack_auth","message":"Slack needs a sign-in. Run: pi mcp login slack","details":{}}}"#;

const OK: &str = r#"{"ok":true,"channel":"C0FIRST001","ts":"1793040197.490209","link":"https://acme.slack.com/archives/C0FIRST001/p1793040197490209"}"#;

/// Plan `pay`'s open Workflow `repo` with the Slack helper (`node`) answering `node`, and its first post stored.
fn world(node: Reply) -> Pay {
    let pay = bare(node);
    pay.seed_post();
    pay
}

/// [`world`] before its first post is stored.
fn bare(node: Reply) -> Pay {
    let pay = fx("open");
    let env = &pay.fx.env;
    let adapter = env.root().join("adapter");
    fs::create_dir_all(&adapter).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    env.write_config(&format!(
        "version = 1\nmachine = \"t\"\npi_adapter = \"{}\"\n[tmux]\nsocket = \"{}\"\n",
        adapter.display(),
        env.tmux_socket()
    ));
    pay.fx.install_pi();
    pay.fx.fake_node(node);
    pay
}

impl Pay {
    fn repost_cmd(&self) -> crate::common::Run {
        self.fx
            .cpd(&["pr", "repost", "--workflow", "repo", "--plan", "pay"])
    }

    /// The Workflow's first post, long ago: `posted_at` and `last_posted_at` both `FIRST`.
    fn seed_post(&self) {
        self.store()
            .write("fixture", |tx| {
                tx.execute_batch(&format!(
                    "INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, last_posted_at, reaction, reacted_at)
                     VALUES ('pay', 'repo', 'C0FIRST001', '1.2', 'https://acme.slack.com/archives/C0FIRST001/p12', '{FIRST}', '{FIRST}', NULL, NULL);"
                ))
                .map_err(Into::into)
            })
            .unwrap();
    }

    /// `(posted_at, last_posted_at)` of the Workflow's post.
    fn times(&self) -> Option<(String, Option<String>)> {
        self.store()
            .read(|c| {
                let mut st = c.prepare("SELECT posted_at, last_posted_at FROM pr_posts WHERE plan = 'pay' AND workflow = 'repo'")?;
                let mut rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
                Ok(rows.next().transpose()?)
            })
            .unwrap()
    }

    fn event_count(&self) -> i64 {
        self.store()
            .read(|c| Ok(c.query_row("SELECT count(*) FROM events", [], |r| r.get(0))?))
            .unwrap()
    }
}

#[test]
fn repost_sends_the_thread_broadcast_and_moves_last_posted_at() {
    let pay = world(Reply::ok(&format!("{OK}\n")));
    let events = pay.event_count();

    let out = pay.repost_cmd().ok();

    // Queued (swr-slack.md 5): the broadcast and last_posted_at move when the send completes, in core
    // swr::source::slack_write_tests::slack_repost_sends_the_broadcast_and_moves_last_posted_at_each_time.
    assert_eq!(out["queued"], true, "{out}");
    assert_eq!(out["post"], json!({"channel": "C0FIRST001", "ts": "1.2"}));
    assert_eq!(pay.fx.calls("node").len(), 0, "nothing is sent yet");
    let (posted_at, last) = pay.times().unwrap();
    assert_eq!(posted_at, FIRST, "the first post never changes");
    assert_eq!(
        last.as_deref(),
        Some(FIRST),
        "last_posted_at waits for the send"
    );
    assert_eq!(pay.event_count(), events, "a data row: no Event");
}

#[test]
fn repost_twice_moves_last_posted_at_each_time() {
    let pay = world(Reply::ok(&format!("{OK}\n")));

    let first = pay.repost_cmd().ok();
    let second = pay.repost_cmd().ok();

    // Each repost is one reminder: two queued mutations, none coalesced (core
    // swr::source::slack_write_tests::slack_repost_goes_through_the_outbox).
    assert_eq!(first["queued"], true, "{first}");
    assert_eq!(second["queued"], true, "{second}");
    assert_ne!(first["mutation_id"], second["mutation_id"], "no coalescing");
    assert_eq!(pay.fx.calls("node").len(), 0, "nothing is sent yet");
    assert_eq!(pay.times().unwrap().0, FIRST);
}

#[test]
fn repost_refuses_a_workflow_without_a_post() {
    let pay = bare(Reply::ok(&format!("{OK}\n")));

    let error = refused(&pay.repost_cmd(), "post_not_found", 3);

    assert_eq!(error["details"]["workflow"], "repo");
    assert!(pay.fx.calls("node").is_empty(), "nothing is sent");
    assert_eq!(pay.times(), None, "nothing is written");
}

#[test]
fn a_failed_send_leaves_last_posted_at_unchanged_and_reports_the_helper_code() {
    let pay = world(Reply::exit(3, &format!("{AUTH}\n"), ""));

    // A sign-in refusal is the send's: core swr::source::slack_write_tests::slack_repost_refused_for_a_sign_in_leaves_last_posted_at.
    let out = pay.repost_cmd().ok();

    assert_eq!(out["queued"], true, "{out}");
    assert_eq!(pay.fx.calls("node").len(), 0, "nothing is sent yet");
    assert_eq!(
        pay.times(),
        Some((FIRST.to_owned(), Some(FIRST.to_owned())))
    );
}
