//! The Slack reaction queue's merge-time append (commands.md §21.9; contract §4.4 `slack_reactions`, §7 poll "The reaction
//! queue append"; User decisions pro-d107, pro-d110): the transaction that records a PR's merge queues the post's reaction,
//! and `pr view` reads the post's `reacted_at` from the queue. In-process over a fake `gh`, like `pr_merge.rs`.

use crate::common;

use std::sync::Mutex;

use common::TestEnv;
use common::fx::Fx;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use serde_json::json;

const REF: &str = "acme/api#7";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const MERGED_AT: &str = "2026-10-21T15:01:30Z";
const CHANNEL: &str = "C02JU6C2Z1D";
const TS: &str = "1793040197.490209";

/// A `gh` whose `pr view` says open until `pr merge` ran and merged after; the poll's `api graphql` fails.
struct Gh {
    merged: Mutex<bool>,
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        let view = |state: &str, merge_state: &str, merged_at: Option<&str>| {
            Reply::ok(&format!("{}\n", json!({"state": state, "mergeStateStatus": merge_state, "headRefOid": SHA, "title": "Add", "mergedAt": merged_at})))
        };
        match args[..2] {
            ["pr", "view"] if *self.merged.lock().unwrap() => view("MERGED", "UNKNOWN", Some(MERGED_AT)),
            ["pr", "view"] => view("OPEN", "CLEAN", None),
            ["pr", "merge"] => {
                *self.merged.lock().unwrap() = true;
                Reply::ok("")
            }
            _ => Reply::exit(1, "", ""),
        }
    }
}

/// PR `api` of Plan `pay-1` is open as acme/api#7; its post is as `post` says: `Some(reaction)` posted (the reaction may be
/// none), `None` not posted.
fn fixture(post: Option<Option<&str>>) -> Fx {
    let fx = Fx::with_ports();
    let env = &fx.env;
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    let _ = (*fx.fakes.runner).clone().route("gh", std::sync::Arc::new(Gh { merged: Mutex::new(false) }));
    env.sql(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, merge_state, authored)
           VALUES ('acme/api', 7, 'https://github.com/acme/api/pull/7', 'Add', 0, 'open', 'none', 'none', 't', 't', 'clean', 1);
         INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay-1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay-1', 'ctx', 'Pay', 'deploy', 'lead1', '1999-12-31T00:00:00.000Z', '2000-01-01T00:00:00.000Z');
         INSERT INTO prs (plan, key, repo, base, name, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                          pr_number, pr_url, pr_opened_at, pull_request)
           VALUES ('pay-1', 'api', '/r', 'main', 'api', 'open', 'cpd/pay-1/api', '/w', cpd_txn(), 't', 't',
                   7, 'https://github.com/acme/api/pull/7', 't', (SELECT id FROM pull_requests WHERE number = 7));",
    );
    if let Some(reaction) = post {
        let reaction = reaction.map_or("NULL".to_owned(), |r| format!("'{r}'"));
        env.sql(&format!(
            "INSERT INTO pr_posts (plan, pr, channel, ts, link, posted_at, reaction, reacted_at)
             VALUES ('pay-1', 'api', '{CHANNEL}', '{TS}', 'https://acme.slack.com/archives/{CHANNEL}/p1793040197490209', 't', {reaction}, NULL);"
        ));
    }
    fx
}

/// How many queue rows there are: 0 while the table does not exist.
fn queued(env: &TestEnv) -> i64 {
    let table: i64 = env.query("SELECT count(*) FROM sqlite_master WHERE name = 'slack_reactions'");
    if table == 0 { 0 } else { env.query("SELECT count(*) FROM slack_reactions") }
}

fn merge(fx: &Fx) {
    fx.cpd(&["pr", "merge", REF]).ok();
    assert_eq!(fx.env.query::<String>("SELECT status FROM prs WHERE key = 'api'"), "merged", "the PR's merge is recorded");
}

#[test]
fn a_merge_queues_the_posts_reaction() {
    let fx = fixture(Some(Some("git-merge")));
    merge(&fx);
    let env = &fx.env;
    assert_eq!(queued(env), 1);
    let row = env.query::<String>(
        "SELECT json_object('channel', channel, 'ts', ts, 'emoji', emoji, 'reacted_at', reacted_at, 'queued', queued_at IS NOT NULL) FROM slack_reactions",
    );
    let row: serde_json::Value = serde_json::from_str(&row).unwrap();
    assert_eq!(row, json!({"channel": CHANNEL, "ts": TS, "emoji": "git-merge", "reacted_at": null, "queued": true}));
    assert_eq!(env.query::<Option<String>>("SELECT reacted_at FROM pr_posts"), None, "pr_posts.reacted_at is no longer written");
}

#[test]
fn a_merge_of_a_post_without_a_reaction_queues_nothing() {
    let fx = fixture(Some(None));
    merge(&fx);
    assert_eq!(queued(&fx.env), 0);
}

#[test]
fn a_merge_of_a_pr_without_a_post_queues_nothing() {
    let fx = fixture(None);
    merge(&fx);
    assert_eq!(queued(&fx.env), 0);
}

#[test]
fn merging_twice_queues_one_row() {
    let fx = fixture(Some(Some("git-merge")));
    merge(&fx);
    // The PR is merged on GitHub now: a second `pr merge` reads MERGED, records it again (a no-op) and refuses.
    let _ = fx.cpd(&["pr", "merge", REF]);
    assert_eq!(queued(&fx.env), 1, "one row for one (channel, ts, emoji)");
}

#[test]
fn pr_view_reads_reacted_at_from_the_queue() {
    let pay = super::pr_view::fx("open");
    pay.fx.env.sql(&format!(
        "INSERT INTO pr_posts (plan, pr, channel, ts, link, posted_at, reaction, reacted_at)
         VALUES ('pay', 'repo', '{CHANNEL}', '{TS}', 'https://acme.slack.com/archives/{CHANNEL}/p1793040197490209', 'opened', 'git-merge', 'stale');
         INSERT INTO slack_reactions (channel, ts, emoji, queued_at, reacted_at) VALUES ('{CHANNEL}', '{TS}', 'git-merge', 'q', 'fresh');
         INSERT INTO slack_reactions (channel, ts, emoji, queued_at, reacted_at) VALUES ('{CHANNEL}', '{TS}', 'eyes', 'q', 'other');"
    ));
    let out = pay.fx.cpd(&["pr", "view", "--pr", "repo", "--plan", "pay"]).ok();
    assert_eq!(out["post"]["reacted_at"], "fresh", "the row of (channel, ts, the post's reaction), not pr_posts.reacted_at: {out}");
}
