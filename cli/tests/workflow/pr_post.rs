//! Its `gh` answers the three merge-method flags through the shared `Gh` fake in pr_view.rs (default: squash only).
//! `pr post` (commands.md §9.8b, §21.8): posts a PR's stored pull request to Slack through the helper (`node`, a scripted
//! fake here) and records it in `pr_posts`. Its GitHub calls (auto-merge, §9.8b) are tested in `pr_post_auto_merge`; a refusal makes none.

use std::fs;

use cpd_core::host::runner::Reply;
use serde_json::{Value, json};

use super::pr_view::{Pay, fx};
use crate::common::refused;

const URL: &str = "https://github.com/acme/pay/pull/812";
const OK: &str = r#"{"ok":true,"channel":"C0POLICY1","ts":"1793040197.490209","link":"https://acme.slack.com/archives/C0POLICY1/p1793040197490209"}"#;
const AUTH: &str = r#"{"ok":false,"error":{"code":"slack_auth","message":"Slack needs a sign-in. Run: pi mcp login slack","details":{}}}"#;

/// What a test changes of the baseline.
pub(super) struct Setup {
    /// The owner's `[pr.post]` table body after `enabled = true`; `None` leaves the table out.
    post: Option<String>,
    machine_channel: bool,
    status: &'static str,
}

impl Setup {
    pub(super) fn baseline() -> Setup {
        Setup {
            post: Some("channel = \"C0POLICY1\"\n".to_owned()),
            machine_channel: true,
            status: "open",
        }
    }

    fn with_post(post: &str) -> Setup {
        Setup {
            post: Some(post.to_owned()),
            ..Setup::baseline()
        }
    }
}

fn bot_item() -> Value {
    json!({"id": "IC_9", "kind": "comment", "last": "IC_9", "author": "review-bot", "bot": true, "path": null, "outdated": false,
           "url": format!("{URL}#IC_9"), "excerpt": "Possible null dereference."})
}

fn human_item() -> Value {
    json!({"id": "PRRT_1", "kind": "thread", "last": "PRRT_1", "author": "reviewer", "bot": false, "path": "src/payout.rs", "outdated": false,
           "url": format!("{URL}#discussion_r9"), "excerpt": "This retry never backs off."})
}

/// The baseline of the Plan's one PR (its Context owned by `acme`, its repo named `pay`, the owner's policy as `setup`
/// says, the stored row ready to post) and the helper answering [`OK`].
pub(super) fn post(setup: Setup) -> Pay {
    post_with_node(setup, Reply::ok(&format!("{OK}\n")))
}

/// [`post`] with the helper answering `node` (the first scripted rule is the one that answers, so it is set once, here).
fn post_with_node(setup: Setup, node: Reply) -> Pay {
    let pay = fx(setup.status);
    let env = &pay.fx.env;
    let adapter = env.root().join("adapter");
    fs::create_dir_all(&adapter).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    let inbox = if setup.machine_channel {
        "[inbox]\nteam_channel = \"C0MACHINE1\"\nslack_user_id = \"U0USER001\"\n"
    } else {
        ""
    };
    env.write_config(&format!(
        "version = 1\nmachine = \"t\"\npi_adapter = \"{}\"\n{inbox}[tmux]\nsocket = \"{}\"\n",
        adapter.display(),
        env.tmux_socket()
    ));
    pay.fx.install_pi();
    let owner = setup
        .post
        .map(|body| format!("[pr.post]\nenabled = true\n{body}"))
        .unwrap_or_default();
    env.repo_policy("acme", "owner", &owner);
    env.repo_policy("acme", "pay", "");
    let sql = "UPDATE contexts SET owner = 'acme' WHERE id = 'ctx';
         UPDATE workflows SET name = 'pay' WHERE plan = 'pay' AND key = 'repo';
         UPDATE pull_requests SET mergeable = 'mergeable', review_decision = 'none', checks = 'passed', authored = 1, feedback = '[]'
           WHERE number = 812;"
        .to_string();
    pay.store()
        .write("fixture", |tx| tx.execute_batch(&sql).map_err(Into::into))
        .unwrap();
    pay.fx.fake_node(node);
    pay
}

impl Pay {
    pub(super) fn set(&self, assignments: &str) {
        let sql = format!("UPDATE pull_requests SET {assignments} WHERE number = 812;");
        self.store()
            .write("fixture", |tx| tx.execute_batch(&sql).map_err(Into::into))
            .unwrap();
    }

    pub(super) fn post_cmd(&self) -> crate::common::Run {
        self.fx
            .cpd(&["pr", "post", "--workflow", "repo", "--plan", "pay"])
    }

    fn posts(&self) -> Vec<Value> {
        self.store()
            .read(|c| {
                let mut statement = c.prepare(
                    "SELECT plan, workflow, channel, ts, link, posted_at, reaction, reacted_at FROM pr_posts ORDER BY plan, workflow",
                )?;
                let rows = statement.query_map([], |r| {
                    Ok(json!({"plan": r.get::<_, String>(0)?, "workflow": r.get::<_, String>(1)?, "channel": r.get::<_, String>(2)?,
                              "ts": r.get::<_, String>(3)?, "link": r.get::<_, String>(4)?, "posted_at": r.get::<_, String>(5)?,
                              "reaction": r.get::<_, Option<String>>(6)?, "reacted_at": r.get::<_, Option<String>>(7)?}))
                })?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .unwrap()
    }

    fn events(&self) -> i64 {
        self.store()
            .read(|c| Ok(c.query_row("SELECT count(*) FROM events", [], |r| r.get(0))?))
            .unwrap()
    }

    fn node_calls(&self) -> usize {
        self.fx.calls("node").len()
    }

    /// The refusal `code` (exit `exit`), with nothing sent and nothing written.
    fn refuses(&self, code: &str, exit: i32) -> Value {
        let events = self.events();
        let error = refused(&self.post_cmd(), code, exit);
        assert_eq!(self.node_calls(), 0, "nothing is sent");
        assert!(self.posts().is_empty(), "nothing is written");
        assert_eq!(self.events(), events);
        assert!(self.fx.calls("gh").is_empty(), "no GitHub call");
        error
    }
}

#[test]
fn posts_the_rendered_message_and_records_it() {
    let pay = post(Setup::with_post(
        "channel = \"C0POLICY1\"\ntemplate = \":git-pr-open: {{ title }}:\\n{{ url }}\"\n",
    ));
    pay.set("title = 'Pay the bills', url = 'https://github.com/acme/pay/pull/812'");
    let events = pay.events();

    let out = pay.post_cmd().ok();

    // Queued, not sent (swr-slack.md 5): the message is rendered now, the row and the send come from E24.
    let message = ":git-pr-open: Pay the bills:\nhttps://github.com/acme/pay/pull/812";
    assert_eq!(out["queued"], true, "{out}");
    assert_eq!(out["message"], message);
    assert_eq!(pay.node_calls(), 0, "nothing is sent yet");
    assert!(
        pay.posts().is_empty(),
        "no pr_posts row until the send answers"
    );
    assert_eq!(pay.events(), events, "a data row: no Event");
}

#[test]
fn calls_the_helper_with_the_pi_entry_and_the_channel() {
    let pay = post(Setup::baseline());

    let out = pay.post_cmd().ok();

    // The helper's argv and env are the send's (swr-slack.md 5): core swr::source::slack_write_tests pins them.
    assert_eq!(out["queued"], true, "{out}");
    assert_eq!(out["channel"], "C0POLICY1");
    assert_eq!(pay.node_calls(), 0, "nothing is sent yet");
}

#[test]
fn falls_back_to_the_config_team_channel_and_the_default_template() {
    let pay = post_with_node(
        Setup {
            post: Some(String::new()),
            ..Setup::baseline()
        },
        Reply::ok(&format!("{}\n", OK.replace("C0POLICY1", "C0MACHINE1"))),
    );
    pay.set("title = 'Pay the bills', url = 'https://github.com/acme/pay/pull/812'");

    let out = pay.post_cmd().ok();

    // The channel and message are fixed at enqueue; the helper's channel is the send's (core test).
    assert_eq!(out["queued"], true, "{out}");
    assert_eq!(
        out["message"],
        "Pay the bills\nhttps://github.com/acme/pay/pull/812"
    );
    assert_eq!(out["channel"], "C0MACHINE1");
    assert_eq!(pay.node_calls(), 0, "nothing is sent yet");
    assert!(pay.posts().is_empty());
}

#[test]
fn refuses_when_no_channel_is_configured() {
    let pay = post(Setup {
        post: Some(String::new()),
        machine_channel: false,
        ..Setup::baseline()
    });

    let error = pay.refuses("post_unconfigured", 3);

    assert_eq!(
        error["details"]["keys"],
        json!(["pr.post.channel", "inbox.team_channel"])
    );
}

#[test]
fn a_second_post_of_the_same_pr_is_refused() {
    let pay = post(Setup::baseline());
    let row = json!({"plan": "pay", "workflow": "repo", "channel": "C0FIRST001", "ts": "1.2", "link": "https://acme.slack.com/archives/C0FIRST001/p12",
                     "posted_at": "earlier", "reaction": null, "reacted_at": null});
    pay.store()
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, reaction, reacted_at)
                 VALUES ('pay', 'repo', 'C0FIRST001', '1.2', 'https://acme.slack.com/archives/C0FIRST001/p12', 'earlier', NULL, NULL);",
            )
            .map_err(Into::into)
        })
        .unwrap();

    let error = refused(&pay.post_cmd(), "already_posted", 3);

    assert_eq!(
        error["details"],
        json!({"pr": "repo", "channel": "C0FIRST001", "ts": "1.2",
                                        "link": "https://acme.slack.com/archives/C0FIRST001/p12", "posted_at": "earlier"})
    );
    assert_eq!(pay.node_calls(), 0);
    assert_eq!(pay.posts(), vec![row], "the row is unchanged");
}

#[test]
fn refuses_a_pr_that_is_not_open() {
    let pay = post(Setup {
        status: "waiting",
        ..Setup::baseline()
    });

    let error = pay.refuses("workflow_not_open", 3);

    assert_eq!(error["details"], json!({"pr": "repo", "status": "waiting"}));
}

#[test]
fn refuses_an_owner_that_has_not_enabled_posting() {
    let pay = post(Setup {
        post: None,
        ..Setup::baseline()
    });

    let error = pay.refuses("post_disabled", 3);

    assert_eq!(
        error["details"],
        json!({"pr": "repo", "key": "pr.post.enabled"})
    );
}

#[test]
fn refuses_a_pull_request_the_user_did_not_author() {
    let pay = post(Setup::baseline());
    pay.set("authored = 0");

    let error = pay.refuses("not_authored", 3);

    assert_eq!(error["details"], json!({"pr": "repo", "number": 812}));
}

#[test]
fn refuses_a_pull_request_that_is_not_ready_and_says_every_reason() {
    let pay = post(Setup::baseline());
    pay.set(&format!(
        "draft = 1, mergeable = 'conflicting', review_decision = 'changes_requested', checks = 'failed', feedback = '{}'",
        json!([bot_item()])
    ));

    let error = pay.refuses("pr_not_ready", 3);

    assert_eq!(
        error["details"]["reasons"],
        json!([
            "draft",
            "conflicting",
            "changes_requested",
            "checks_failed",
            "bot_comments"
        ])
    );
    assert_eq!(
        (
            &error["details"]["pr"],
            &error["details"]["number"],
            &error["details"]["url"]
        ),
        (&json!("repo"), &json!(812), &json!(URL))
    );
}

#[test]
fn checks_that_are_pending_or_absent_are_not_green() {
    for checks in ["pending", "none"] {
        let pay = post(Setup::baseline());
        pay.set(&format!("checks = '{checks}'"));

        let error = pay.refuses("pr_not_ready", 3);

        assert_eq!(
            error["details"]["reasons"],
            json!(["checks_pending"]),
            "{checks}"
        );
    }
}

#[test]
fn a_pull_request_whose_comments_were_never_read_is_not_ready() {
    let pay = post(Setup::baseline());
    pay.set("feedback = NULL");

    let error = pay.refuses("pr_not_ready", 3);

    assert_eq!(error["details"]["reasons"], json!(["comments_unread"]));
}

#[test]
fn a_closed_or_merged_pull_request_is_not_open() {
    for state in ["closed", "merged"] {
        let pay = post(Setup::baseline());
        let merged_at = if state == "merged" {
            ", merged_at = '2026-01-01T00:00:00.000Z'"
        } else {
            ""
        };
        pay.set(&format!("state = '{state}'{merged_at}"));

        let error = pay.refuses("pr_not_ready", 3);

        assert_eq!(error["details"]["reasons"], json!(["not_open"]), "{state}");
    }
}

#[test]
fn open_human_comments_and_the_feedback_checks_policy_do_not_change_readiness() {
    let pay = post(Setup::baseline());
    pay.set(&format!("feedback = '{}'", json!([human_item()])));

    let out = pay.post_cmd().ok();

    assert_eq!(out["queued"], true, "{out}");
    assert_eq!(pay.node_calls(), 0, "nothing is sent yet");
    drop(pay);

    let relaxed = post(Setup::baseline());
    relaxed.fx.env.repo_policy(
        "acme",
        "owner",
        "[pr.post]\nenabled = true\nchannel = \"C0POLICY1\"\n[pr.feedback]\nchecks = \"none\"\n",
    );
    relaxed.set("checks = 'failed'");
    let error = relaxed.refuses("pr_not_ready", 3);
    assert_eq!(error["details"]["reasons"], json!(["checks_failed"]));
}

#[test]
fn slack_not_signed_in_records_nothing() {
    let pay = post_with_node(Setup::baseline(), Reply::exit(3, &format!("{AUTH}\n"), ""));

    // A sign-in refusal is the send's: core swr::source::slack_write_tests::slack_post_refused_for_a_sign_in_records_nothing.
    let out = pay.post_cmd().ok();

    assert_eq!(out["queued"], true, "{out}");
    assert_eq!(pay.node_calls(), 0, "nothing is sent yet");
    assert!(pay.posts().is_empty());
}

#[test]
fn a_helper_that_prints_nothing_is_spawn_failed() {
    let pay = post_with_node(Setup::baseline(), Reply::exit(1, "", ""));

    // The helper's failure is the send's: core swr::source::slack_write_tests::slack_post_whose_helper_prints_nothing_fails_the_send.
    let out = pay.post_cmd().ok();

    assert_eq!(out["queued"], true, "{out}");
    assert_eq!(pay.node_calls(), 0, "nothing is sent yet");
    assert!(pay.posts().is_empty());
}

#[test]
fn the_message_is_sent_verbatim_without_escaping() {
    let pay = post(Setup::baseline());
    pay.set("title = 'Fix a & b <c> \"d\"'");

    let out = pay.post_cmd().ok();

    // The message is rendered at enqueue, verbatim; the stdin of the send is core's
    // slack_write_tests::slack_post_sends_the_message_verbatim_without_escaping.
    let message = format!("Fix a & b <c> \"d\"\n{URL}");
    assert_eq!(out["message"], message);
    assert_eq!(pay.node_calls(), 0, "nothing is sent yet");
}

#[test]
fn the_post_records_the_reaction_the_owner_chose() {
    let pay = post(Setup::with_post(
        "channel = \"C0POLICY1\"\nmerged_reaction = \"git-merge\"\n",
    ));

    let out = pay.post_cmd().ok();

    // The reaction is recorded in pr_posts at the send: core slack_write_tests::slack_post_sends_the_message_and_records_its_pr_posts_row.
    assert_eq!(out["queued"], true, "{out}");
    assert!(pay.posts().is_empty(), "no row until the send answers");
}

#[test]
fn a_host_post_table_cannot_let_an_owner_that_did_not_enable_posting_post() {
    let pay = post(Setup {
        post: None,
        ..Setup::baseline()
    });
    let dir = pay.fx.env.cpd_home().join("policy");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("host.toml"), "[pr.post]\nenabled = true\n").unwrap();

    pay.refuses("policy_invalid", 1);

    assert_eq!(pay.node_calls(), 0);
    assert!(pay.posts().is_empty());
}
