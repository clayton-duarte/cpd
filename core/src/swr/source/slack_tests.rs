//! The `slack` Source (docs/architecture/swr-slack.md §3, §3.5): `fetch` runs the helper through the Runner with the
//! stdin of §3.2, stores its `posts` in `swr_slack_posts` and never writes `run/team-review.json`. The helper is a
//! scripted `node` route on `FakeRunner`; time is the `ManualClock`; no Slack, no process, no sleep.
//! Tests of this module run one per process (nextest): the variables point at the test's own temp dir.

use std::collections::VecDeque;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use time::OffsetDateTime;

use super::slack::Slack;
use crate::config::Config;
use crate::home::CpdHome;
use crate::host::runner::{Call, Handler, Reply};
use crate::ports::Fakes;
use crate::store::Store;
use crate::swr::cache;
use crate::swr::source::{Cx, Source};

const CHANNEL: &str = "C0TEAM001";
const KEY: &str = "slack:channel:C0TEAM001";
const ME: &str = "U0USER001";

/// The helper: answers its replies in order (the last one stays) and records the stdin of every call.
struct Helper {
    replies: Mutex<VecDeque<(i32, String)>>,
    stdins: Arc<Mutex<Vec<Call>>>,
}

impl Handler for Helper {
    fn handle(&self, call: &Call) -> Reply {
        self.stdins.lock().unwrap().push(call.clone());
        let mut replies = self.replies.lock().unwrap();
        let (code, stdout) = if replies.len() > 1 {
            replies.pop_front().unwrap()
        } else {
            replies.front().cloned().expect("a scripted reply")
        };
        Reply::exit(code, &format!("{stdout}\n"), "")
    }
}

struct Fx {
    _tmp: tempfile::TempDir,
    home: CpdHome,
    config: Config,
    store: Store,
    fakes: Fakes,
    calls: Arc<Mutex<Vec<Call>>>,
}

/// One post of the helper's answer (§3.3): `ts_raw` of Ryan's (`U0RYAN001`) message linking `repo#number`.
fn post(ts_raw: &str, number: i64, primary: bool) -> Value {
    json!({
        "repo": "acme/pay", "number": number, "primary": primary,
        "ts": ts_raw.parse::<f64>().unwrap(), "ts_raw": ts_raw,
        "author": "Ryan Case", "author_id": "U0RYAN001", "author_is_me": false,
        "reactions": {"eyes": 2}, "reply_count": 3, "latest_reply": "1760953000.000100",
        "claim": {"kind": "claimed", "reason": "claimed with :eyes: in Slack"},
        "eyes_me": false, "thread_tags_me": false
    })
}

fn answer(posts: &[Value]) -> (i32, String) {
    (
        0,
        json!({"ok": true, "channel": CHANNEL, "read": posts.len() + 70, "posts": posts})
            .to_string(),
    )
}

/// A home with pi staged, the channel and the User configured, and the helper answering `replies`.
fn fx(replies: Vec<(i32, String)>) -> Fx {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("pi"), "").unwrap();
    std::fs::set_permissions(bin.join("pi"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::create_dir_all(root.join("install")).unwrap();
    std::fs::write(root.join("install/current-version"), "1.2.0\n").unwrap();
    for package in ["pi-coding-agent", "pi-ai"] {
        let dist = root
            .join("install/releases/1.2.0/node_modules/@earendil-works")
            .join(package)
            .join("dist");
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join("index.js"), "").unwrap();
    }
    let adapter = root.join("adapter");
    std::fs::create_dir_all(&adapter).unwrap();
    std::fs::write(adapter.join("package.json"), "{}").unwrap();
    let home = CpdHome::resolve(Some(root.join("cpd").into_os_string()), None).unwrap();
    std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    std::fs::write(
        home.config(),
        format!(
            "version = 1\nmachine = \"t\"\npi_adapter = \"{}\"\n[inbox]\nteam_channel = \"{CHANNEL}\"\nslack_user_id = \"{ME}\"\n[tmux]\nsocket = \"s\"\n",
            adapter.display()
        ),
    )
    .unwrap();
    // SAFETY: one test per process (nextest); the variables point at this test's own temp dir.
    unsafe {
        std::env::set_var("CPD_HOME", home.root());
        let old = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&old));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    }
    let fakes = Fakes::new();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let _ = (*fakes.runner).clone().route(
        "node",
        Arc::new(Helper {
            replies: Mutex::new(replies.into()),
            stdins: calls.clone(),
        }),
    );
    let config = Config::load(&home).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    Fx {
        _tmp: tmp,
        home,
        config,
        store,
        fakes,
        calls,
    }
}

impl Fx {
    fn now(&self) -> OffsetDateTime {
        self.fakes.ports.clock.utc()
    }

    /// One `fetch` of the channel key at the fake clock's time.
    fn fetch(&mut self) -> Result<(), crate::error::CpdError> {
        let now = self.now();
        let mut cx = Cx {
            store: &mut self.store,
            ports: &self.fakes.ports,
            home: &self.home,
            config: &self.config,
        };
        Slack.fetch(&mut cx, &[KEY.to_owned()], now)
    }

    fn sql(&mut self, sql: &str) {
        self.store
            .write("fixture", |tx| Ok(tx.execute_batch(sql)?))
            .unwrap();
    }

    fn count(&mut self, sql: &str) -> i64 {
        self.store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    /// The stdin of the helper's `n`th call, parsed.
    fn stdin(&self, n: usize) -> Value {
        let call = self.calls.lock().unwrap()[n].clone();
        serde_json::from_slice(&call.stdin.expect("the helper is given a stdin")).unwrap()
    }

    fn args(&self, n: usize) -> Vec<String> {
        self.calls.lock().unwrap()[n].args.clone()
    }

    /// `(ts, repo, number, primary_pr, reply_count, reactions_json, eyes_me, thread_tags_me, inactive_at)` of every row.
    #[allow(clippy::type_complexity)]
    fn rows(
        &mut self,
    ) -> Vec<(
        String,
        i64,
        i64,
        Option<i64>,
        String,
        Option<i64>,
        Option<i64>,
        Option<String>,
    )> {
        self.store
            .read(|c| {
                let mut stmt = c.prepare(
                    "SELECT ts, number, primary_pr, reply_count, reactions_json, eyes_me, thread_tags_me, inactive_at
                       FROM swr_slack_posts ORDER BY ts DESC, number",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?))
                })?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap()
    }
}

#[test]
fn slack_read_stores_posts_in_swr_and_no_team_review_file() {
    // One message links two pull requests: one row each, the first is the primary one (§2).
    let mut fx = fx(vec![answer(&[
        post("1760951522.011300", 812, true),
        post("1760951522.011300", 813, false),
    ])]);

    fx.fetch().unwrap();

    assert_eq!(
        fx.rows(),
        vec![
            (
                "1760951522.011300".into(),
                812,
                1,
                Some(3),
                r#"{"eyes":2}"#.into(),
                Some(0),
                Some(0),
                None
            ),
            (
                "1760951522.011300".into(),
                813,
                0,
                Some(3),
                r#"{"eyes":2}"#.into(),
                Some(0),
                Some(0),
                None
            ),
        ],
        "one row per (message, pull request), the counts as the helper read them"
    );
    let query = cache::get(&mut fx.store, KEY)
        .unwrap()
        .expect("the channel query row");
    assert_eq!(query.status, "success");
    assert!(
        !fx.home.run_dir().join("team-review.json").exists(),
        "the team review file is retired (§9)"
    );
    let args = fx.args(0);
    assert!(args.iter().any(|a| a.ends_with("slack-channel.ts")));
    let pair = |name: &str, value: &str| args.windows(2).any(|w| w[0] == name && w[1] == value);
    assert!(
        pair("--channel", CHANNEL) && pair("--me", ME),
        "argv is today's: {args:?}"
    );
}

#[test]
fn slack_unchanged_channel_opens_no_write_txn() {
    let mut fx = fx(vec![answer(&[post("1760951522.011300", 812, true)])]);
    fx.fetch().unwrap();
    let before = fx.count("SELECT count(*) FROM txns");

    // The next read answers the same posts (only `read` differs: it is not in the hash, §3.5) once the query is stale.
    fx.fakes
        .clock
        .advance(std::time::Duration::from_secs(31 * 60));
    fx.fetch().unwrap();

    assert_eq!(fx.calls.lock().unwrap().len(), 2, "the helper ran again");
    assert_eq!(
        fx.count("SELECT count(*) FROM txns"),
        before,
        "unchanged data opens no write transaction (D5)"
    );
}

#[test]
fn slack_passes_cached_post_state_and_terminal_prs_to_the_helper() {
    let mut fx = fx(vec![answer(&[
        post("1760951522.011300", 812, true),
        post("1760951400.000100", 813, true),
    ])]);
    fx.fetch().unwrap();
    assert_eq!(
        fx.stdin(0),
        json!({"version": 1, "known": [], "terminal": []}),
        "an empty cache: nothing known"
    );
    fx.sql(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
           VALUES ('acme/pay', 813, 'https://github.com/acme/pay/pull/813', 'T', 0, 'merged', 'none', 'none', 't', 't', 't');
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
           VALUES ('acme/pay', 812, 'https://github.com/acme/pay/pull/812', 'T', 0, 'open', 'none', 'none', 't', 't');",
    );

    fx.fakes
        .clock
        .advance(std::time::Duration::from_secs(31 * 60));
    fx.fetch().unwrap();

    let stdin = fx.stdin(1);
    assert_eq!(stdin["version"], 1);
    assert_eq!(
        stdin["terminal"],
        json!(["acme/pay#813"]),
        "merged and closed pull requests the rows name"
    );
    let mut known = stdin["known"].as_array().unwrap().clone();
    known.sort_by_key(|k| k["number"].as_i64());
    assert_eq!(
        known,
        vec![
            json!({"ts": "1760951522.011300", "repo": "acme/pay", "number": 812, "author_is_me": false,
                   "reply_count": 3, "latest_reply": "1760953000.000100", "reactions": {"eyes": 2},
                   "eyes_me": false, "thread_tags_me": false}),
            json!({"ts": "1760951400.000100", "repo": "acme/pay", "number": 813, "author_is_me": false,
                   "reply_count": 3, "latest_reply": "1760953000.000100", "reactions": {"eyes": 2},
                   "eyes_me": false, "thread_tags_me": false}),
        ],
        "the stored state of every active row (§3.2)"
    );
}

/// A post of the User's own message, whose thread and eyes were never read (§3.4 T1, E1).
fn own(ts_raw: &str, number: i64) -> Value {
    let mut p = post(ts_raw, number, true);
    p["author_id"] = json!(ME);
    p["author_is_me"] = json!(true);
    p["eyes_me"] = Value::Null;
    p["thread_tags_me"] = Value::Null;
    p
}

#[test]
fn slack_own_post_skips_thread_and_reaction_reads() {
    let mut fx = fx(vec![answer(&[own("1760951522.011300", 812)])]);
    fx.fetch().unwrap();
    fx.fakes
        .clock
        .advance(std::time::Duration::from_secs(31 * 60));
    fx.fetch().unwrap();

    let known = fx.stdin(1)["known"].clone();
    assert_eq!(
        known[0]["author_is_me"], true,
        "the helper is told the message is the User's, so T1 and E1 apply"
    );
    assert_eq!(
        known[0]["thread_tags_me"],
        Value::Null,
        "an unread value stays unknown, never invented"
    );
    let rows = fx.rows();
    assert_eq!(
        (rows[0].5, rows[0].6),
        (None, None),
        "the echoed values are stored as they came"
    );
}

#[test]
fn slack_terminal_pr_post_skips_thread_and_reaction_reads() {
    let mut fx = fx(vec![answer(&[post("1760951522.011300", 812, true)])]);
    fx.fetch().unwrap();
    fx.sql(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
           VALUES ('acme/pay', 812, 'https://github.com/acme/pay/pull/812', 'T', 0, 'merged', 'none', 'none', 't', 't', 't');",
    );
    fx.fakes
        .clock
        .advance(std::time::Duration::from_secs(31 * 60));
    fx.fetch().unwrap();

    assert_eq!(
        fx.stdin(1)["terminal"],
        json!(["acme/pay#812"]),
        "T2 and E1 need the terminal list"
    );
    let rows = fx.rows();
    assert_eq!(
        (rows[0].5, rows[0].6),
        (Some(0), Some(0)),
        "the known values are kept as echoed"
    );
}

#[test]
fn slack_reactions_read_only_when_eyes_count_changed_or_uncached() {
    let mut moved = post("1760951522.011300", 812, true);
    moved["reactions"] = json!({"eyes": 3});
    moved["eyes_me"] = json!(true);
    let mut fx = fx(vec![
        answer(&[post("1760951522.011300", 812, true)]),
        answer(&[moved]),
    ]);
    fx.fetch().unwrap();
    fx.fakes
        .clock
        .advance(std::time::Duration::from_secs(31 * 60));
    fx.fetch().unwrap();

    assert_eq!(
        fx.stdin(1)["known"][0]["reactions"],
        json!({"eyes": 2}),
        "E3 compares with the known count"
    );
    assert_eq!(fx.stdin(1)["known"][0]["eyes_me"], false);
    let rows = fx.rows();
    assert_eq!(rows[0].4, r#"{"eyes":3}"#, "the new count");
    assert_eq!(
        rows[0].5,
        Some(1),
        "the eyes_me the read returned replaces the known one"
    );
}

#[test]
fn slack_thread_read_only_when_reply_count_or_latest_reply_changed() {
    let mut replied = post("1760951522.011300", 812, true);
    replied["reply_count"] = json!(4);
    replied["latest_reply"] = json!("1760954000.000100");
    replied["thread_tags_me"] = json!(true);
    let mut fx = fx(vec![
        answer(&[post("1760951522.011300", 812, true)]),
        answer(&[replied]),
    ]);
    fx.fetch().unwrap();
    fx.fakes
        .clock
        .advance(std::time::Duration::from_secs(31 * 60));
    fx.fetch().unwrap();

    let known = fx.stdin(1)["known"][0].clone();
    assert_eq!(
        (known["reply_count"].clone(), known["latest_reply"].clone()),
        (json!(3), json!("1760953000.000100")),
        "T4 compares with the known thread"
    );
    let rows = fx.rows();
    assert_eq!(
        (rows[0].3, rows[0].6),
        (Some(4), Some(1)),
        "the new reply count and the thread read's answer"
    );
}

#[test]
fn slack_failed_read_keeps_cached_posts() {
    let failed =
        r#"{"ok":false,"error":{"code":"slack_failed","message":"Slack is down.","details":{}}}"#;
    let mut fx = fx(vec![
        answer(&[post("1760951522.011300", 812, true)]),
        (1, failed.into()),
    ]);
    fx.fetch().unwrap();
    let before = fx.rows();
    assert_eq!(before.len(), 1);

    fx.fakes
        .clock
        .advance(std::time::Duration::from_secs(31 * 60));
    let _ = fx.fetch();

    assert_eq!(
        fx.rows(),
        before,
        "a failed read writes and deletes no row (swr.md §16)"
    );
    let query = cache::get(&mut fx.store, KEY).unwrap().unwrap();
    assert_eq!(
        query.status, "error",
        "the failure is recorded on the query"
    );
    assert!(
        query
            .last_error
            .unwrap_or_default()
            .contains("slack_failed")
    );
}

/// A confirmed post queues its auto-merge once: the `auto_merge` op is the one enqueue site (swr-slack.md §6.2), and a later
/// read that confirms the same post queues nothing more.
#[test]
fn slack_confirmed_post_queues_one_auto_merge_and_a_second_read_queues_none() {
    let mut fx = fx(vec![
        answer(&[own("1760951522.011300", 812)]),
        answer(&[own("1760951522.011300", 812)]),
    ]);
    let dir = crate::policy::policy_dir(&fx.home).join("acme");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("owner.toml"),
        "[pr.post]\nenabled = true\nchannel = \"C0POLICY1\"\nmerged_reaction = \"merge\"\n",
    )
    .unwrap();
    std::fs::write(dir.join("pay.toml"), "").unwrap();
    fx.store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, authored)
                   VALUES ('acme/pay', 812, 'https://github.com/acme/pay/pull/812', 'Pay', 0, 'open', 'none', 'none',
                           '2025-10-01T00:00:00Z', '2025-10-01T00:00:00Z', 1);
                 INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('pay', 'ctx', 'Pay', 'deploy', 'lead1', '1999-12-31T00:00:00.000Z', '2000-01-01T00:00:00.000Z');
                 INSERT INTO workflows (plan, key, repo, base, name, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                                        pr_number, pr_url, pr_opened_at, pull_request)
                   VALUES ('pay', 'repo', '/r', 'main', 'repo', 'open', 'cpd/pay/repo', '/w', cpd_txn(), 't', 't',
                           812, 'https://github.com/acme/pay/pull/812', 't', (SELECT id FROM pull_requests WHERE number = 812));",
            )?;
            Ok(())
        })
        .unwrap();

    fx.fetch().unwrap();
    fx.fetch().unwrap();

    let queued: i64 = fx
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM swr_mutations WHERE op_kind = 'auto_merge'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(queued, 1, "one auto-merge mutation for the confirmed post");
}
