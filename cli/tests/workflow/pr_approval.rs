//! Approval-gated agent replies, posts and reposts, and the internal `pr approve` (decision pro-d451). An agent is the Lead
//! with `$CPD_SESSION` set; the cockpit runs the same command with it empty. Everything external (`gh`, the Slack helper
//! `node`) is a scripted fake: no real GitHub, Slack, git or tmux.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use cpd_core::clock::Clock;
use cpd_core::host::runner::{Call, Reply};
use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use super::pr_view::{Pay, fx};
use crate::common::Run;

const LEAD: &str = "lead1";
const URL: &str = "https://github.com/acme/pay/pull/812";
const FIRST: &str = "2026-01-01T00:00:00.000Z";
const THREAD_ANSWER: &str = r#"{"data": {"addPullRequestReviewThreadReply": {"comment": {"id": "c1", "url": "https://github.com/acme/repo/pull/7#discussion_r1"}}}}"#;
const COMMENT_ANSWER: &str = r#"{"data": {"addComment": {"commentEdge": {"node": {"id": "c2", "url": "https://github.com/acme/repo/pull/7#issuecomment-2"}}}}}"#;
const PR_ID_ANSWER: &str = r#"{"data": {"repository": {"pullRequest": {"id": "PR_node"}}}}"#;
const SLACK_OK: &str = r#"{"ok":true,"channel":"C0POLICY1","ts":"1793040197.490209","link":"https://acme.slack.com/archives/C0POLICY1/p1793040197490209"}"#;

fn item(id: &str, kind: &str, bot: bool) -> Value {
    json!({"id": id, "kind": kind, "last": id, "author": if bot { "review-bot" } else { "reviewer" }, "bot": bot,
           "path": null, "outdated": false, "url": format!("{URL}#{id}"), "excerpt": "This retry never backs off."})
}

/// A bot thread, a human thread, a human comment, a human review and a bot comment.
fn list() -> Value {
    json!([
        item("PRRT_B", "thread", true),
        item("PRRT_H", "thread", false),
        item("IC_H", "comment", false),
        item("PRR_H", "review", false),
        item("IC_B", "comment", true)
    ])
}

/// What a test changes of the world.
struct Setup {
    reply: &'static str,
    require_approval: bool,
    gh_fails: bool,
    seed_post: bool,
    /// No feedback at all: a Pull Request that is ready to post.
    ready: bool,
}

fn setup(reply: &'static str) -> Setup {
    Setup {
        reply,
        require_approval: true,
        gh_fails: false,
        seed_post: false,
        ready: false,
    }
}

/// Plan `pay`'s open Workflow `repo` (named `pay`), `[pr.feedback] reply` as `s.reply`, `[pr.post] require_approval` as set,
/// the feedback list stored, `gh` and the Slack helper (`node`) scripted.
fn world(s: &Setup) -> Pay {
    let pay = fx("open");
    let env = &pay.fx.env;
    let adapter = env.root().join("adapter");
    fs::create_dir_all(&adapter).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    env.write_config(&format!(
        "version = 1\nmachine = \"t\"\npi_adapter = \"{}\"\n[inbox]\nteam_channel = \"C0MACHINE1\"\nslack_user_id = \"U0USER001\"\n[tmux]\nsocket = \"{}\"\n",
        adapter.display(),
        env.tmux_socket()
    ));
    pay.fx.install_pi();
    env.owner_policy(
        "acme",
        &format!(
            "[pr]\ngate = \"local\"\n[pr.feedback]\nreply = \"{}\"\n[pr.post]\nenabled = true\nchannel = \"C0POLICY1\"\nrequire_approval = {}\n",
            s.reply, s.require_approval
        ),
    );
    env.repo_policy("acme", "pay", "");
    let has = |text: &'static str| move |c: &Call| c.args.join("\n").contains(text);
    let runner = (*pay.fx.fakes.runner).clone();
    // A post world keeps `pr_view`'s own `gh` (its auto-merge read): the reply rules would answer that query too.
    let _ = if s.ready {
        runner
    } else if s.gh_fails {
        runner.on(
            "gh",
            |_| true,
            Reply::exit(1, "", "HTTP 502: bad gateway\n"),
        )
    } else {
        runner
            .on(
                "gh",
                has("addPullRequestReviewThreadReply"),
                Reply::ok(THREAD_ANSWER),
            )
            .on("gh", has("addComment"), Reply::ok(COMMENT_ANSWER))
            .on("gh", has("pullRequest"), Reply::ok(PR_ID_ANSWER))
    };
    let mut sql = format!(
        "UPDATE contexts SET owner = 'acme' WHERE id = 'ctx';
         UPDATE workflows SET name = 'pay' WHERE plan = 'pay' AND key = 'repo';
         UPDATE pull_requests SET mergeable = 'mergeable', review_decision = 'none', checks = 'passed', authored = 1, feedback = '{}'
           WHERE number = 812;",
        if s.ready { json!([]) } else { list() }
    );
    if s.seed_post {
        sql.push_str(&format!(
            "INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, last_posted_at, reaction, reacted_at)
             VALUES ('pay', 'repo', 'C0FIRST001', '1.2', 'https://acme.slack.com/archives/C0FIRST001/p12', '{FIRST}', '{FIRST}', NULL, NULL);"
        ));
    }
    pay.store()
        .write("fixture", |tx| tx.execute_batch(&sql).map_err(Into::into))
        .unwrap();
    pay.fx.fake_node(Reply::ok(&format!("{SLACK_OK}\n")));
    pay
}

fn as_agent(pay: &Pay, session: &str, args: &[&str]) -> Run {
    pay.fx
        .cpd_with(pay.fx.env.root(), args, &[("CPD_SESSION", session)])
}

/// The cockpit: the same command with `$CPD_SESSION` empty.
fn as_cockpit(pay: &Pay, args: &[&str]) -> Run {
    as_agent(pay, "", args)
}

fn reply_args<'a>(to: &'a str) -> Vec<&'a str> {
    vec![
        "pr",
        "reply",
        "--workflow",
        "repo",
        "--plan",
        "pay",
        "--to",
        to,
        "--body",
        "Fixed.",
    ]
}

fn reply(pay: &Pay, session: &str, to: &str) -> Run {
    as_agent(pay, session, &reply_args(to))
}

fn approve_args<'a>(kind: &'a str, to: Option<&'a str>) -> Vec<&'a str> {
    let mut args = vec![
        "pr",
        "approve",
        "--kind",
        kind,
        "--workflow",
        "repo",
        "--plan",
        "pay",
    ];
    if let Some(to) = to {
        args.extend(["--to", to]);
    }
    args
}

fn approve(pay: &Pay, session: &str, kind: &str, to: Option<&str>) -> Value {
    as_agent(pay, session, &approve_args(kind, to)).ok()
}

fn approval_dir(pay: &Pay) -> PathBuf {
    pay.fx.env.cpd_home().join("run/pr/approve")
}

fn approval_files(pay: &Pay) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(approval_dir(pay)) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries.map(|e| e.unwrap().path()).collect();
    files.sort();
    files
}

fn mode(path: &std::path::Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// Changes fields of the one approval file (kept private, as it was).
fn rewrite_approval(pay: &Pay, changes: Value) {
    let files = approval_files(pay);
    assert_eq!(files.len(), 1, "{files:?}");
    let mut held: Value = serde_json::from_str(&fs::read_to_string(&files[0]).unwrap()).unwrap();
    for (key, value) in changes.as_object().unwrap() {
        held[key] = value.clone();
    }
    fs::write(&files[0], held.to_string()).unwrap();
    fs::set_permissions(&files[0], fs::Permissions::from_mode(0o600)).unwrap();
}

fn mutations(pay: &Pay) -> Vec<String> {
    pay.fx
        .calls("gh")
        .into_iter()
        .map(|c| c.args.join("\n"))
        .filter(|a| a.contains("addPullRequestReviewThreadReply") || a.contains("addComment"))
        .collect()
}

fn feedback(pay: &Pay) -> Option<String> {
    pay.store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT feedback FROM pull_requests WHERE number = 812",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

fn count(pay: &Pay, table: &str) -> i64 {
    pay.store()
        .read(|c| Ok(c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?))
        .unwrap()
}

/// `approval_required` (exit 3) naming the Workflow, the kind and (a reply) the item.
fn assert_approval_required(run: &Run, kind: &str, to: Option<&str>) {
    let error = run.error();
    assert_eq!(
        (error["code"].as_str().unwrap(), run.exit),
        ("approval_required", 3),
        "{run:?}"
    );
    assert_eq!(error["details"]["workflow"], "repo", "{run:?}");
    assert_eq!(error["details"]["kind"], kind, "{run:?}");
    if let Some(to) = to {
        assert_eq!(error["details"]["to"], to, "{run:?}");
    }
}

/// An agent's reply to `to` is refused and leaves everything as it was.
fn assert_reply_refused(pay: &Pay, session: &str, to: &str) {
    let calls = pay.fx.calls("gh").len();
    let stored = feedback(pay);

    let run = reply(pay, session, to);

    assert_approval_required(&run, "reply", Some(to));
    assert_eq!(pay.fx.calls("gh").len(), calls, "no GitHub call for {to}");
    assert!(mutations(pay).is_empty(), "nothing is posted for {to}");
    assert_eq!(feedback(pay), stored, "the stored list is unchanged");
}

fn assert_replied(run: &Run, to: &str) {
    let out = run.ok();
    assert_eq!(out["replied"]["to"], to, "{out}");
}

#[test]
fn bots_only_lets_an_agent_reply_to_a_bot_thread() {
    let pay = world(&setup("bots_only"));

    assert_replied(&reply(&pay, LEAD, "PRRT_B"), "PRRT_B");

    let posted = mutations(&pay);
    assert_eq!(posted.len(), 1, "{posted:?}");
    assert!(
        posted[0].contains("addPullRequestReviewThreadReply"),
        "{}",
        posted[0]
    );
    assert!(posted[0].contains("PRRT_B"), "{}", posted[0]);
    // The same agent gets no such freedom on a human thread.
    assert_reply_refused(&pay, LEAD, "PRRT_H");
}

#[test]
fn bots_only_refuses_an_agent_reply_to_a_human_thread_comment_or_review() {
    let pay = world(&setup("bots_only"));

    for to in ["PRRT_H", "IC_H", "PRR_H"] {
        assert_reply_refused(&pay, LEAD, to);
    }
}

#[test]
fn bots_only_refuses_an_agent_reply_to_a_bot_comment() {
    let pay = world(&setup("bots_only"));

    assert_reply_refused(&pay, LEAD, "IC_B");
}

#[test]
fn all_lets_an_agent_reply_to_a_human_item() {
    let pay = world(&setup("all"));

    for to in ["PRRT_H", "IC_H", "PRR_H", "IC_B"] {
        assert_replied(&reply(&pay, LEAD, to), to);
    }

    assert_eq!(mutations(&pay).len(), 4);
    drop(pay);
    let strict = world(&setup("bots_only"));
    // Only the policy differs: under `bots_only` the same reply is refused.
    assert_reply_refused(&strict, LEAD, "PRRT_H");
}

#[test]
fn a_reply_without_cpd_session_is_never_refused() {
    let pay = world(&setup("bots_only"));
    assert_reply_refused(&pay, LEAD, "PRRT_H");

    for to in ["PRRT_H", "IC_H", "PRR_H", "IC_B"] {
        let run = as_cockpit(&pay, &reply_args(to));
        assert_replied(&run, to);
    }

    assert_eq!(mutations(&pay).len(), 4);
}

#[test]
fn an_approval_lets_the_agent_reply_to_that_item_once() {
    let pay = world(&setup("bots_only"));
    approve(&pay, LEAD, "reply", Some("PRRT_H"));
    assert_eq!(approval_files(&pay).len(), 1);

    assert_replied(&reply(&pay, LEAD, "PRRT_H"), "PRRT_H");

    let posted = mutations(&pay);
    assert_eq!(posted.len(), 1, "{posted:?}");
    assert!(posted[0].contains("PRRT_H"), "{}", posted[0]);
    assert!(
        approval_files(&pay).is_empty(),
        "the approval is used up by the reply"
    );
    assert_reply_refused(&pay, LEAD, "PRRT_H");
}

#[test]
fn an_approval_for_another_item_or_workflow_or_session_does_not_apply() {
    // Another item.
    let pay = world(&setup("bots_only"));
    approve(&pay, LEAD, "reply", Some("IC_H"));
    assert_reply_refused(&pay, LEAD, "PRRT_H");
    assert_eq!(approval_files(&pay).len(), 1, "the approval is untouched");

    // Another Workflow.
    drop(pay);
    let pay = world(&setup("bots_only"));
    approve(&pay, LEAD, "reply", Some("PRRT_H"));
    rewrite_approval(&pay, json!({"workflow": "other"}));
    assert_reply_refused(&pay, LEAD, "PRRT_H");

    // Another Session.
    drop(pay);
    let pay = world(&setup("bots_only"));
    approve(&pay, "lead2", "reply", Some("PRRT_H"));
    assert_reply_refused(&pay, LEAD, "PRRT_H");

    // Another kind: a post approval is no reply approval.
    drop(pay);
    let pay = world(&setup("bots_only"));
    approve(&pay, LEAD, "post", None);
    assert_reply_refused(&pay, LEAD, "PRRT_H");
}

#[test]
fn an_expired_approval_is_refused() {
    let pay = world(&setup("bots_only"));
    approve(&pay, LEAD, "reply", Some("PRRT_H"));
    let at = pay.fx.fakes.clock.utc() - Duration::minutes(11);
    rewrite_approval(&pay, json!({"at": at.format(&Rfc3339).unwrap()}));

    assert_reply_refused(&pay, LEAD, "PRRT_H");
}

#[test]
fn a_failed_reply_keeps_the_approval() {
    let pay = world(&Setup {
        gh_fails: true,
        ..setup("bots_only")
    });
    approve(&pay, LEAD, "reply", Some("PRRT_H"));

    let run = reply(&pay, LEAD, "PRRT_H");

    assert_eq!(
        (run.error()["code"].as_str().unwrap(), run.exit),
        ("gh_failed", 1),
        "{run:?}"
    );
    assert_eq!(approval_files(&pay).len(), 1, "a failed reply keeps it");
}

#[test]
fn require_approval_refuses_an_agent_post_and_repost() {
    let pay = world(&Setup {
        ready: true,
        ..setup("all")
    });
    let queued = count(&pay, "swr_mutations");

    let run = as_agent(
        &pay,
        LEAD,
        &["pr", "post", "--workflow", "repo", "--plan", "pay"],
    );

    assert_approval_required(&run, "post", None);
    assert_eq!(count(&pay, "swr_mutations"), queued, "nothing is queued");
    assert_eq!(count(&pay, "pr_posts"), 0);

    drop(pay);
    let pay = world(&Setup {
        seed_post: true,
        ready: true,
        ..setup("all")
    });
    let queued = count(&pay, "swr_mutations");

    let run = as_agent(
        &pay,
        LEAD,
        &["pr", "repost", "--workflow", "repo", "--plan", "pay"],
    );

    assert_approval_required(&run, "post", None);
    assert_eq!(count(&pay, "swr_mutations"), queued, "nothing is queued");
}

#[test]
fn an_approved_post_goes_through_once_and_a_second_is_refused() {
    let pay = world(&Setup {
        ready: true,
        ..setup("all")
    });
    approve(&pay, LEAD, "post", None);
    let post = ["pr", "post", "--workflow", "repo", "--plan", "pay"];

    let out = as_agent(&pay, LEAD, &post).ok();

    assert_eq!(out["queued"], true, "{out}");
    assert!(approval_files(&pay).is_empty(), "the post used it up");

    // One approval covers one `pr repost` as well, and only one.
    drop(pay);
    let pay = world(&Setup {
        seed_post: true,
        ready: true,
        ..setup("all")
    });
    approve(&pay, LEAD, "post", None);
    let repost = ["pr", "repost", "--workflow", "repo", "--plan", "pay"];
    let queued = count(&pay, "swr_mutations");

    let first = as_agent(&pay, LEAD, &repost).ok();

    assert_eq!(first["queued"], true, "{first}");
    assert_eq!(count(&pay, "swr_mutations"), queued + 1);
    assert!(approval_files(&pay).is_empty());
    let second = as_agent(&pay, LEAD, &repost);
    assert_approval_required(&second, "post", None);
    assert_eq!(count(&pay, "swr_mutations"), queued + 1, "no second send");
}

#[test]
fn the_cockpit_post_and_repost_need_no_approval() {
    let pay = world(&Setup {
        ready: true,
        ..setup("all")
    });
    let post = ["pr", "post", "--workflow", "repo", "--plan", "pay"];
    assert_approval_required(&as_agent(&pay, LEAD, &post), "post", None);

    let out = as_cockpit(&pay, &post).ok();

    assert_eq!(out["queued"], true, "{out}");

    drop(pay);
    let pay = world(&Setup {
        seed_post: true,
        ready: true,
        ..setup("all")
    });
    let repost = ["pr", "repost", "--workflow", "repo", "--plan", "pay"];
    assert_approval_required(&as_agent(&pay, LEAD, &repost), "post", None);

    let out = as_cockpit(&pay, &repost).ok();

    assert_eq!(out["queued"], true, "{out}");
}

#[test]
fn pr_approve_writes_a_private_file_and_prints_its_expiry() {
    let pay = world(&setup("bots_only"));

    let out = approve(&pay, LEAD, "reply", Some("PRRT_H"));

    let dir = approval_dir(&pay);
    assert_eq!(mode(&dir), 0o700, "{dir:?}");
    let files = approval_files(&pay);
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0].extension().and_then(|e| e.to_str()), Some("json"));
    assert_eq!(mode(&files[0]), 0o600);
    let held: Value = serde_json::from_str(&fs::read_to_string(&files[0]).unwrap()).unwrap();
    let at = OffsetDateTime::parse(held["at"].as_str().unwrap(), &Rfc3339).unwrap();
    assert_eq!(
        held,
        json!({"session": LEAD, "kind": "reply", "plan": "pay", "workflow": "repo", "to": "PRRT_H", "at": held["at"]})
    );
    assert_eq!(
        out["approved"]["expires_at"]
            .as_str()
            .map(|e| OffsetDateTime::parse(e, &Rfc3339).unwrap()),
        Some(at + Duration::minutes(10)),
        "{out}"
    );
    assert_eq!(out["approved"]["kind"], "reply");
    assert_eq!(out["approved"]["workflow"], "repo");
    assert_eq!(out["approved"]["to"], "PRRT_H");
    assert!(pay.fx.calls("gh").is_empty(), "approve calls no gh");

    // A post approval holds no item.
    let post = approve(&pay, LEAD, "post", None);
    assert_eq!(post["approved"]["kind"], "post");
    assert_eq!(post["approved"]["to"], Value::Null, "{post}");
    assert_eq!(approval_files(&pay).len(), 2);
}

#[test]
fn pr_approve_refuses_a_missing_session_and_a_wrong_to_flag() {
    let pay = world(&setup("bots_only"));
    approve(&pay, LEAD, "reply", Some("PRRT_H"));
    let usage = |run: Run| {
        assert_eq!(
            (run.error()["code"].as_str().unwrap(), run.exit),
            ("usage", 2),
            "{run:?}"
        );
    };

    usage(as_cockpit(&pay, &approve_args("reply", Some("PRRT_H"))));
    usage(as_agent(&pay, LEAD, &approve_args("reply", None)));
    usage(as_agent(&pay, LEAD, &approve_args("post", Some("PRRT_H"))));

    assert_eq!(
        approval_files(&pay).len(),
        1,
        "only the valid one is written"
    );
}
