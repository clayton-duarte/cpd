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
    pay.seed_post();
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

    let calls = pay.fx.calls("node");
    assert_eq!(calls.len(), 1);
    let args: Vec<&str> = calls[0].args.iter().map(String::as_str).collect();
    assert_eq!(
        args[args.len() - 6..],
        [
            "--channel",
            "C0FIRST001",
            "--thread-ts",
            "1.2",
            "--reply-broadcast",
            "true"
        ]
    );
    assert_eq!(
        calls[0].stdin.as_deref(),
        Some(&b"Bibbidi-bobbidi-bump!"[..])
    );
    let (posted_at, last) = pay.times().unwrap();
    let last = last.unwrap();
    assert_eq!(posted_at, FIRST, "the first post never changes");
    assert!(last.as_str() > FIRST, "last_posted_at moved: {last}");
    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "workflow": "repo",
               "post": {"channel": "C0FIRST001", "ts": "1.2"},
               "message": "Bibbidi-bobbidi-bump!", "last_posted_at": last})
    );
    assert_eq!(pay.event_count(), events, "a data row: no Event");
}

#[test]
fn repost_twice_moves_last_posted_at_each_time() {
    let pay = world(Reply::ok(&format!("{OK}\n")));

    let first = pay.repost_cmd().ok();
    let after_first = pay.times().unwrap().1.unwrap();
    let second = pay.repost_cmd().ok();
    let after_second = pay.times().unwrap().1.unwrap();

    assert_eq!(pay.fx.calls("node").len(), 2, "one send per repost");
    assert_eq!(first["last_posted_at"], after_first.as_str());
    assert_eq!(second["last_posted_at"], after_second.as_str());
    assert!(after_first.as_str() > FIRST, "{after_first}");
    assert!(
        after_second > after_first,
        "{after_first} then {after_second}"
    );
    assert_eq!(pay.times().unwrap().0, FIRST);
}

#[test]
fn repost_refuses_a_workflow_without_a_post() {
    let pay = world(Reply::ok(&format!("{OK}\n")));
    pay.store()
        .write("fixture", |tx| {
            tx.execute_batch("DELETE FROM pr_posts;")
                .map_err(Into::into)
        })
        .unwrap();

    let error = refused(&pay.repost_cmd(), "post_not_found", 3);

    assert_eq!(error["details"]["workflow"], "repo");
    assert!(pay.fx.calls("node").is_empty(), "nothing is sent");
    assert_eq!(pay.times(), None, "nothing is written");
}

#[test]
fn a_failed_send_leaves_last_posted_at_unchanged_and_reports_the_helper_code() {
    let pay = world(Reply::ok(&format!("{OK}\n")));
    pay.fx.fake_node(Reply::exit(3, &format!("{AUTH}\n"), ""));

    let error = refused(&pay.repost_cmd(), "slack_auth", 3);

    assert_eq!(
        error["message"],
        "Slack needs a sign-in. Run: pi mcp login slack"
    );
    assert_eq!(
        pay.times(),
        Some((FIRST.to_owned(), Some(FIRST.to_owned())))
    );
}
