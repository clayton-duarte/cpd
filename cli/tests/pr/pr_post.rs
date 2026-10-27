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
struct Setup {
    /// The owner's `[pr.post]` table body after `enabled = true`; `None` leaves the table out.
    post: Option<String>,
    machine_channel: bool,
    status: &'static str,
}

impl Setup {
    fn baseline() -> Setup {
        Setup { post: Some("channel = \"C0POLICY1\"\n".to_owned()), machine_channel: true, status: "open" }
    }

    fn with_post(post: &str) -> Setup {
        Setup { post: Some(post.to_owned()), ..Setup::baseline() }
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
fn post(setup: Setup) -> Pay {
    post_with_node(setup, Reply::ok(&format!("{OK}\n")))
}

/// [`post`] with the helper answering `node` (the first scripted rule is the one that answers, so it is set once, here).
fn post_with_node(setup: Setup, node: Reply) -> Pay {
    let pay = fx(setup.status);
    let env = &pay.fx.env;
    let adapter = env.root().join("adapter");
    fs::create_dir_all(&adapter).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    let inbox = if setup.machine_channel { "[inbox]\nteam_channel = \"C0MACHINE1\"\nslack_user_id = \"U0USER001\"\n" } else { "" };
    env.write_config(&format!(
        "version = 1\nmachine = \"t\"\npi_adapter = \"{}\"\n{inbox}[tmux]\nsocket = \"{}\"\n",
        adapter.display(),
        env.tmux_socket()
    ));
    pay.fx.install_pi();
    let owner = setup.post.map(|body| format!("[pr.post]\nenabled = true\n{body}")).unwrap_or_default();
    env.repo_policy("acme", "owner", &owner);
    env.repo_policy("acme", "pay", "");
    let sql = format!(
        "UPDATE contexts SET owner = 'acme' WHERE id = 'ctx';
         UPDATE prs SET name = 'pay' WHERE plan = 'pay' AND key = 'repo';
         UPDATE pull_requests SET mergeable = 'mergeable', review_decision = 'none', checks = 'passed', authored = 1, feedback = '[]'
           WHERE number = 812;"
    );
    pay.store().write("fixture", |tx| tx.execute_batch(&sql).map_err(Into::into)).unwrap();
    pay.fx.fake_node(node);
    pay
}

impl Pay {
    fn set(&self, assignments: &str) {
        let sql = format!("UPDATE pull_requests SET {assignments} WHERE number = 812;");
        self.store().write("fixture", |tx| tx.execute_batch(&sql).map_err(Into::into)).unwrap();
    }

    fn post_cmd(&self) -> crate::common::Run {
        self.fx.cpd(&["pr", "post", "--pr", "repo", "--plan", "pay"])
    }

    fn posts(&self) -> Vec<Value> {
        self.store()
            .read(|c| {
                let mut statement = c.prepare(
                    "SELECT plan, pr, channel, ts, link, posted_at, reaction, reacted_at FROM pr_posts ORDER BY plan, pr",
                )?;
                let rows = statement.query_map([], |r| {
                    Ok(json!({"plan": r.get::<_, String>(0)?, "pr": r.get::<_, String>(1)?, "channel": r.get::<_, String>(2)?,
                              "ts": r.get::<_, String>(3)?, "link": r.get::<_, String>(4)?, "posted_at": r.get::<_, String>(5)?,
                              "reaction": r.get::<_, Option<String>>(6)?, "reacted_at": r.get::<_, Option<String>>(7)?}))
                })?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .unwrap()
    }

    fn events(&self) -> i64 {
        self.store().read(|c| Ok(c.query_row("SELECT count(*) FROM events", [], |r| r.get(0))?)).unwrap()
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
    let pay = post(Setup::with_post("channel = \"C0POLICY1\"\ntemplate = \":git-pr-open: {{ title }}:\\n{{ url }}\"\n"));
    pay.set("title = 'Pay the bills', url = 'https://github.com/acme/pay/pull/812'");
    let events = pay.events();

    let out = pay.post_cmd().ok();

    let message = ":git-pr-open: Pay the bills:\nhttps://github.com/acme/pay/pull/812";
    let calls = pay.fx.calls("node");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].stdin.as_deref(), Some(message.as_bytes()));
    let posts = pay.posts();
    assert_eq!(posts.len(), 1, "{posts:?}");
    let row = &posts[0];
    assert_eq!(
        (&row["plan"], &row["pr"], &row["channel"], &row["ts"], &row["link"], &row["reaction"], &row["reacted_at"]),
        (
            &json!("pay"),
            &json!("repo"),
            &json!("C0POLICY1"),
            &json!("1793040197.490209"),
            &json!("https://acme.slack.com/archives/C0POLICY1/p1793040197490209"),
            &Value::Null,
            &Value::Null
        )
    );
    let posted_at = row["posted_at"].as_str().unwrap();
    assert!(!posted_at.is_empty());
    let mut out = out;
    out.as_object_mut().unwrap().remove("auto_merge"); // its own tests: pr_post_auto_merge
    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "pr": "repo", "number": 812, "url": URL,
               "post": {"channel": "C0POLICY1", "ts": "1793040197.490209",
                        "link": "https://acme.slack.com/archives/C0POLICY1/p1793040197490209",
                        "posted_at": posted_at, "reaction": null, "reacted_at": null},
               "message": message})
    );
    assert_eq!(pay.events(), events, "a data row: no Event");
}

#[test]
fn calls_the_helper_with_the_pi_entry_and_the_channel() {
    let pay = post(Setup::baseline());

    pay.post_cmd().ok();

    let calls = pay.fx.calls("node");
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    let root = pay.fx.env.root().canonicalize().unwrap();
    let script = root.join("adapter/lib/slack-post.ts");
    let entry = root.join(format!(
        "install/releases/{}/node_modules/@earendil-works/pi-coding-agent/dist/index.js",
        crate::common::healthy::PI_RELEASE
    ));
    let argv: Vec<&str> = call.args.iter().map(String::as_str).collect();
    assert_eq!(argv, [script.to_str().unwrap(), "--coding-agent", entry.to_str().unwrap(), "--channel", "C0POLICY1"]);
    let env = |key: &str| call.envs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    assert_eq!(env("PI_CODING_AGENT_DIR"), Some(Some(root.display().to_string())));
    for removed in ["CPD_PLAN", "CPD_SESSION", "CPD_ATTEMPT"] {
        assert_eq!(env(removed), Some(None), "{removed} is removed");
    }
    assert_eq!(call.cwd, Some(pay.fx.env.cpd_home().canonicalize().unwrap()));
    assert_eq!(call.deadline, std::time::Duration::from_secs(60));
}

#[test]
fn falls_back_to_the_config_team_channel_and_the_default_template() {
    let pay = post_with_node(
        Setup { post: Some(String::new()), ..Setup::baseline() },
        Reply::ok(&format!("{}\n", OK.replace("C0POLICY1", "C0MACHINE1"))),
    );
    pay.set("title = 'Pay the bills', url = 'https://github.com/acme/pay/pull/812'");

    let out = pay.post_cmd().ok();

    let message = "Pay the bills\nhttps://github.com/acme/pay/pull/812";
    let calls = pay.fx.calls("node");
    assert_eq!(calls[0].stdin.as_deref(), Some(message.as_bytes()));
    assert_eq!(calls[0].args.last().map(String::as_str), Some("C0MACHINE1"));
    assert_eq!(out["message"], message);
    assert_eq!(out["post"]["channel"], "C0MACHINE1");
    assert_eq!(pay.posts()[0]["channel"], "C0MACHINE1");
}

#[test]
fn refuses_when_no_channel_is_configured() {
    let pay = post(Setup { post: Some(String::new()), machine_channel: false, ..Setup::baseline() });

    let error = pay.refuses("post_unconfigured", 3);

    assert_eq!(error["details"]["keys"], json!(["pr.post.channel", "inbox.team_channel"]));
}

#[test]
fn a_second_post_of_the_same_pr_is_refused() {
    let pay = post(Setup::baseline());
    let row = json!({"plan": "pay", "pr": "repo", "channel": "C0FIRST001", "ts": "1.2", "link": "https://acme.slack.com/archives/C0FIRST001/p12",
                     "posted_at": "earlier", "reaction": null, "reacted_at": null});
    pay.store()
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO pr_posts (plan, pr, channel, ts, link, posted_at, reaction, reacted_at)
                 VALUES ('pay', 'repo', 'C0FIRST001', '1.2', 'https://acme.slack.com/archives/C0FIRST001/p12', 'earlier', NULL, NULL);",
            )
            .map_err(Into::into)
        })
        .unwrap();

    let error = refused(&pay.post_cmd(), "already_posted", 3);

    assert_eq!(error["details"], json!({"pr": "repo", "channel": "C0FIRST001", "ts": "1.2",
                                        "link": "https://acme.slack.com/archives/C0FIRST001/p12", "posted_at": "earlier"}));
    assert_eq!(pay.node_calls(), 0);
    assert_eq!(pay.posts(), vec![row], "the row is unchanged");
}

#[test]
fn refuses_a_pr_that_is_not_open() {
    let pay = post(Setup { status: "waiting", ..Setup::baseline() });

    let error = pay.refuses("pr_not_open", 3);

    assert_eq!(error["details"], json!({"pr": "repo", "status": "waiting"}));
}

#[test]
fn refuses_an_owner_that_has_not_enabled_posting() {
    let pay = post(Setup { post: None, ..Setup::baseline() });

    let error = pay.refuses("post_disabled", 3);

    assert_eq!(error["details"], json!({"pr": "repo", "key": "pr.post.enabled"}));
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

    assert_eq!(error["details"]["reasons"], json!(["draft", "conflicting", "changes_requested", "checks_failed", "bot_comments"]));
    assert_eq!((&error["details"]["pr"], &error["details"]["number"], &error["details"]["url"]), (&json!("repo"), &json!(812), &json!(URL)));
}

#[test]
fn checks_that_are_pending_or_absent_are_not_green() {
    for checks in ["pending", "none"] {
        let pay = post(Setup::baseline());
        pay.set(&format!("checks = '{checks}'"));

        let error = pay.refuses("pr_not_ready", 3);

        assert_eq!(error["details"]["reasons"], json!(["checks_pending"]), "{checks}");
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
        let merged_at = if state == "merged" { ", merged_at = '2026-01-01T00:00:00.000Z'" } else { "" };
        pay.set(&format!("state = '{state}'{merged_at}"));

        let error = pay.refuses("pr_not_ready", 3);

        assert_eq!(error["details"]["reasons"], json!(["not_open"]), "{state}");
    }
}

#[test]
fn open_human_comments_and_the_feedback_checks_policy_do_not_change_readiness() {
    let pay = post(Setup::baseline());
    pay.set(&format!("feedback = '{}'", json!([human_item()])));

    pay.post_cmd().ok();

    assert_eq!(pay.node_calls(), 1);
    assert_eq!(pay.posts().len(), 1);
    drop(pay);

    let relaxed = post(Setup::baseline());
    relaxed.fx.env.repo_policy("acme", "owner", "[pr.post]\nenabled = true\nchannel = \"C0POLICY1\"\n[pr.feedback]\nchecks = \"none\"\n");
    relaxed.set("checks = 'failed'");
    let error = relaxed.refuses("pr_not_ready", 3);
    assert_eq!(error["details"]["reasons"], json!(["checks_failed"]));
}

#[test]
fn slack_not_signed_in_records_nothing() {
    let pay = post_with_node(Setup::baseline(), Reply::exit(3, &format!("{AUTH}\n"), ""));

    let error = refused(&pay.post_cmd(), "slack_auth", 3);

    assert_eq!(error["message"], "Slack needs a sign-in. Run: pi mcp login slack");
    assert!(pay.posts().is_empty());
}

#[test]
fn a_helper_that_prints_nothing_is_spawn_failed() {
    let pay = post_with_node(Setup::baseline(), Reply::exit(1, "", ""));

    refused(&pay.post_cmd(), "spawn_failed", 1);

    assert!(pay.posts().is_empty());
}

#[test]
fn the_message_is_sent_verbatim_without_escaping() {
    let pay = post(Setup::baseline());
    pay.set("title = 'Fix a & b <c> \"d\"'");

    let out = pay.post_cmd().ok();

    let message = format!("Fix a & b <c> \"d\"\n{URL}");
    assert_eq!(pay.fx.calls("node")[0].stdin.as_deref(), Some(message.as_bytes()));
    assert_eq!(out["message"], message);
}

#[test]
fn the_post_records_the_reaction_the_owner_chose() {
    let pay = post(Setup::with_post("channel = \"C0POLICY1\"\nmerged_reaction = \"git-merge\"\n"));

    let out = pay.post_cmd().ok();

    assert_eq!(out["post"]["reaction"], "git-merge");
    assert_eq!(out["post"]["reacted_at"], Value::Null);
    let posts = pay.posts();
    assert_eq!((&posts[0]["reaction"], &posts[0]["reacted_at"]), (&json!("git-merge"), &Value::Null));
    drop(pay);

    let plain = post(Setup::baseline());
    plain.post_cmd().ok();
    assert_eq!(plain.posts()[0]["reaction"], Value::Null);
}
