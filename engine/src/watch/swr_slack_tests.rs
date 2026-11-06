//! E24 running the real `slack` Source (docs/architecture/swr-slack.md §3.5, §6): the E11 confirmation of the User's own
//! posts, which queues an `auto_merge` mutation (§6.2) and fixes D1 to D3 (§6.1), and the garbage collection of posts
//! the channel no longer holds. The helper is a scripted `node` route on the `FakeRunner`; the clock is the `ManualClock`
//! (2026-01-01T00:00:00Z); no Slack, no process, no sleep. One test per process (nextest): the variables are this test's.

use std::collections::VecDeque;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{Value, json};

use super::swr;
use super::testkit::Fx;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::ports::Fakes;

const CHANNEL: &str = "C0TEAM001";
const ME: &str = "U0USER001";
/// 2026-01-01T00:00:00Z, the `ManualClock`'s start, as Slack's epoch seconds.
const EPOCH: i64 = 1_767_225_600;

/// The helper: answers its replies in order, the last one staying.
struct Helper(Mutex<VecDeque<String>>);

impl Handler for Helper {
    fn handle(&self, _call: &Call) -> Reply {
        let mut replies = self.0.lock().unwrap();
        let stdout = if replies.len() > 1 {
            replies.pop_front().unwrap()
        } else {
            replies.front().cloned().expect("a scripted reply")
        };
        Reply::exit(0, &format!("{stdout}\n"), "")
    }
}

/// A post of the helper's answer: `author_id`'s message `ts_raw` linking `acme/pay#number`.
fn post(author_id: &str, ts_raw: &str, number: i64) -> Value {
    json!({
        "repo": "acme/pay", "number": number, "primary": true,
        "ts": ts_raw.parse::<f64>().unwrap(), "ts_raw": ts_raw,
        "author": "Someone", "author_id": author_id, "author_is_me": author_id == ME,
        "reactions": {}, "reply_count": null, "latest_reply": null, "claim": null,
        "eyes_me": null, "thread_tags_me": null
    })
}

fn answer(posts: &[Value]) -> String {
    json!({"ok": true, "channel": CHANNEL, "read": 70, "posts": posts}).to_string()
}

/// An engine whose Workflow `repo` of Plan `pay` is open on the open pull request `acme/pay#812` (created
/// `created_at`) and posting is enabled by the owner's policy (`enabled`) with merge reaction `merged`; E24 runs the registry
/// and the helper answers `replies`.
fn fx(replies: Vec<String>, created_at: &str, enabled: bool) -> (Fx, Fakes, tempfile::TempDir) {
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
    let mut fx = Fx::with_plan(&format!(
        "pi_adapter = \"{}\"\n[inbox]\nteam_channel = \"{CHANNEL}\"\nslack_user_id = \"{ME}\"\n",
        adapter.display()
    ));
    let policy = fx.engine.home.root().join("policy/acme");
    std::fs::create_dir_all(&policy).unwrap();
    std::fs::write(
        policy.join("owner.toml"),
        format!(
            "[pr.post]\nenabled = {enabled}\nchannel = \"C0POLICY1\"\nmerged_reaction = \"merge\"\n"
        ),
    )
    .unwrap();
    std::fs::write(policy.join("pay.toml"), "").unwrap();
    fx.fixture(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, authored)
           VALUES ('acme/pay', 812, 'https://github.com/acme/pay/pull/812', 'T', 0, 'open', 'none', 'none', '{created_at}', 't', 1);
         UPDATE workflows SET status = 'open', pr_number = 812, pr_url = 'https://github.com/acme/pay/pull/812', pr_opened_at = 't',
                pull_request = (SELECT id FROM pull_requests WHERE number = 812)
          WHERE plan = 'pay' AND key = 'repo';"
    ));
    // SAFETY: one test per process (nextest); the variables point at this test's own temp dir.
    unsafe {
        std::env::set_var("CPD_HOME", fx.engine.home.root());
        let old = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&old));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    }
    let fakes = Fakes::new();
    fx.engine.ports = fakes.ports.clone();
    let _ = (*fakes.runner)
        .clone()
        .route("node", Arc::new(Helper(Mutex::new(replies.into()))));
    (fx, fakes, tmp)
}

fn tick(fx: &mut Fx) {
    swr::run(&mut fx.engine);
}

fn auto_merges(fx: &mut Fx) -> Vec<(String, String, String)> {
    fx.engine
        .store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT system, target_key, status FROM swr_mutations WHERE op_kind = 'auto_merge' ORDER BY id",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

/// The `pr_posts` rows of Workflow `repo`: `(ts, posted_at, last_posted_at, reaction)`.
fn pr_posts(fx: &mut Fx) -> Vec<(String, String, String, Option<String>)> {
    fx.engine
        .store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT ts, posted_at, last_posted_at, reaction FROM pr_posts WHERE plan = 'pay' AND workflow = 'repo'",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

fn at(epoch: i64) -> String {
    cpd_core::clock::timestamp(time::OffsetDateTime::from_unix_timestamp(epoch).unwrap())
}

#[test]
fn slack_confirmed_own_post_queues_pending_auto_merge_when_policy_enables_it() {
    // The User's message, an hour after the pull request was created; a colleague's earlier one is not the post.
    let ts = format!("{}.000100", EPOCH - 3600);
    let earlier_colleague = format!("{}.000200", EPOCH - 7200);
    let (mut fx, _fakes, _tmp) = fx(
        vec![answer(&[
            post(ME, &ts, 812),
            post("U0RYAN001", &earlier_colleague, 812),
        ])],
        "2025-12-31T23:00:00Z",
        true,
    );

    tick(&mut fx);

    assert_eq!(
        auto_merges(&mut fx),
        vec![(
            "github".into(),
            "github:pr:acme/pay#812".into(),
            "pending".into()
        )],
        "§6.2: the confirmed post queues auto_merge on the pull request, by policy only"
    );
    assert_eq!(
        pr_posts(&mut fx),
        vec![(ts, at(EPOCH - 3600), at(EPOCH - 3600), Some("merge".into()))],
        "D2: posted_at and last_posted_at are the message's time; D3: the reaction is the policy's merged_reaction"
    );
}

#[test]
fn slack_post_older_than_the_pull_request_is_not_confirmed() {
    // D1 (contract §2.2 "PR post" (c)): a message before the pull request's created_at is not its post.
    let ts = format!("{}.000100", EPOCH - 7200);
    let (mut fx, _fakes, _tmp) = fx(
        vec![answer(&[post(ME, &ts, 812)])],
        "2025-12-31T23:00:00Z",
        true,
    );

    tick(&mut fx);

    assert_eq!(
        fx.query::<i64>("SELECT count(*) FROM swr_slack_posts"),
        1,
        "the channel was read and stored"
    );
    assert_eq!(pr_posts(&mut fx), Vec::new());
    assert_eq!(auto_merges(&mut fx), Vec::new());
}

#[test]
fn slack_unsent_post_never_gets_auto_merge() {
    // The User's own message does not exist in the channel (the post is still unsent): a colleague's message of the same
    // pull request, and the User's message of a pull request no Workflow owns, confirm nothing.
    let (mut fx, _fakes, _tmp) = fx(
        vec![answer(&[
            post("U0RYAN001", &format!("{}.000100", EPOCH - 3600), 812),
            post(ME, &format!("{}.000200", EPOCH - 1800), 999),
        ])],
        "2025-12-31T23:00:00Z",
        true,
    );

    tick(&mut fx);

    assert_eq!(
        pr_posts(&mut fx),
        Vec::new(),
        "no row without the User's own message"
    );
    assert_eq!(
        auto_merges(&mut fx),
        Vec::new(),
        "never an auto_merge for an unsent post"
    );
    assert_eq!(
        fx.query::<i64>("SELECT count(*) FROM swr_slack_posts"),
        2,
        "the channel itself was read and stored"
    );
}

#[test]
fn slack_posts_unseen_for_seven_days_are_garbage_collected() {
    let seen = post("U0RYAN001", &format!("{}.000100", EPOCH - 3600), 812);
    let (mut fx, fakes, _tmp) = fx(
        vec![answer(&[seen]), answer(&[])],
        "2025-12-31T23:00:00Z",
        true,
    );
    let rows = |fx: &mut Fx| fx.query::<i64>("SELECT count(*) FROM swr_slack_posts");
    let inactive = |fx: &mut Fx| {
        fx.query::<i64>("SELECT count(*) FROM swr_slack_posts WHERE inactive_at IS NOT NULL")
    };

    tick(&mut fx);
    assert_eq!(
        (rows(&mut fx), inactive(&mut fx)),
        (1, 0),
        "the read stored the post"
    );

    // The channel answer no longer holds it: inactive, not deleted (§3.5).
    fakes.clock.advance(Duration::from_secs(31 * 60));
    tick(&mut fx);
    assert_eq!((rows(&mut fx), inactive(&mut fx)), (1, 1));

    // Eight days later the sweep deletes it: it was unseen for longer than swr.gc_days (7).
    fakes.clock.advance(Duration::from_secs(8 * 86_400));
    tick(&mut fx);
    assert_eq!(rows(&mut fx), 0);
}
