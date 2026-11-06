//! The Slack writes through the outbox (docs/architecture/swr-slack.md §4, §5): a reaction, a post and a repost are
//! `swr_mutations` of the `slack` system, the overlay shows them at once, and `execute` runs the helper through the Runner.
//! The helpers (`node`) and `gh` are scripted on `FakeRunner`; time is the `ManualClock`; no Slack, no process, no sleep.
//! Tests of this module run one per process (nextest): the variables point at the test's own temp dir.

use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::Parser;
use serde_json::{Value, json};

use super::slack::{self, Slack};
use crate::config::Config;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::host::runner::{Call, Handler, Reply};
use crate::output::Output;
use crate::ports::{Fakes, Ports};
use crate::store::Store;
use crate::swr::outbox::{self, Retry};
use crate::swr::overlay;
use crate::swr::source::{Cx, ExecOutcome, Source};

const CHANNEL: &str = "C0TEAM001";
const KEY: &str = "slack:channel:C0TEAM001";
const TS: &str = "1760951522.011300";
const REFUSED: &str = r#"{"ok":false,"error":{"code":"slack_refused","message":"Slack refused the reaction.","details":{"reason":"invalid_name"}}}"#;
const FAILED: &str = r#"{"ok":false,"error":{"code":"slack_failed","message":"Slack did not answer.","details":{}}}"#;
const SENT: &str =
    r#"{"ok":true,"channel":"C0POLICY1","ts":"1700000000.000100","link":"https://slack.test/p1"}"#;

/// A scripted helper: `(exit, stdout)` for every call, recording the calls.
struct Helper {
    reply: (i32, &'static str),
    calls: Arc<Mutex<Vec<Call>>>,
}

impl Handler for Helper {
    fn handle(&self, call: &Call) -> Reply {
        self.calls.lock().unwrap().push(call.clone());
        Reply::exit(self.reply.0, &format!("{}\n", self.reply.1), "")
    }
}

/// `gh` for `pr post`: a repo that allows squash and no auto-merge.
struct Gh;

impl Handler for Gh {
    fn handle(&self, _call: &Call) -> Reply {
        Reply::ok(
            r#"{"data":{"repository":{"autoMergeAllowed":false,"squashMergeAllowed":true,"mergeCommitAllowed":false,"rebaseMergeAllowed":false,"pullRequest":{"id":"PR_1","autoMergeRequest":null}}}}"#,
        )
    }
}

/// pi staged under `root` (a `bin/pi` on PATH and its install tree) and the adapter directory; returns the adapter path.
fn stage_pi(root: &std::path::Path) -> std::path::PathBuf {
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
    // SAFETY: one test per process (nextest); the variable points at this test's own temp dir.
    unsafe {
        let old = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&old));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    }
    adapter
}

struct Fx {
    _tmp: tempfile::TempDir,
    home: CpdHome,
    config: Config,
    store: Store,
    fakes: Fakes,
    calls: Arc<Mutex<Vec<Call>>>,
}

/// A home with pi staged and the helper `node` answering `reply` to every call.
fn fx(reply: (i32, &'static str)) -> Fx {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let adapter = stage_pi(&root);
    let home = CpdHome::resolve(Some(root.join("cpd").into_os_string()), None).unwrap();
    std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    std::fs::write(
        home.config(),
        format!(
            "version = 1\nmachine = \"t\"\npi_adapter = \"{}\"\n[inbox]\nteam_channel = \"{CHANNEL}\"\nslack_user_id = \"U0USER001\"\n[tmux]\nsocket = \"s\"\n",
            adapter.display()
        ),
    )
    .unwrap();
    // SAFETY: one test per process (nextest).
    unsafe { std::env::set_var("CPD_HOME", home.root()) };
    let fakes = Fakes::new();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let _ = (*fakes.runner).clone().route(
        "node",
        Arc::new(Helper {
            reply,
            calls: calls.clone(),
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
    fn now(&self) -> time::OffsetDateTime {
        self.fakes.ports.clock.utc()
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

    /// The message (`TS`) of the channel as the last read stored it: one `eyes` count.
    fn seed_message(&mut self) {
        self.sql(&format!(
            "INSERT INTO swr_slack_posts (channel, ts, repo, number, primary_pr, author_is_me, reactions_json, eyes_me, fetched_at)
               VALUES ('{CHANNEL}', '{TS}', 'acme/pay', 812, 1, 0, '{{\"eyes\":1}}', 0, 't')"
        ));
    }

    /// The `slack` mutations: `(op_kind, target_key, vars_json, status)`.
    fn mutations(&mut self) -> Vec<(String, String, String, String)> {
        self.store
            .read(|c| {
                let mut stmt = c.prepare(
                    "SELECT op_kind, target_key, vars_json, status FROM swr_mutations WHERE system = 'slack' ORDER BY id",
                )?;
                let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap()
    }

    /// `execute` of the oldest due `slack` mutation, as E24 claims it.
    fn execute(&mut self) -> ExecOutcome {
        let now = self.now();
        let m = outbox::claim_next(&mut self.store, now, "slack")
            .unwrap()
            .expect("a due slack mutation");
        let mut cx = Cx {
            store: &mut self.store,
            ports: &self.fakes.ports,
            home: &self.home,
            config: &self.config,
        };
        Slack.execute(&mut cx, &m)
    }
}

fn vars(channel: &str, ts: &str, emoji: &str) -> String {
    json!({"channel": channel, "emoji": emoji, "ts": ts}).to_string()
}

#[test]
fn slack_reaction_enqueues_a_deduped_mutation_per_channel_ts_emoji() {
    let mut fx = fx((0, "{}"));
    let now = fx.now();

    let first = slack::react(&mut fx.store, now, CHANNEL, TS, "eyes").unwrap();
    let again = slack::react(&mut fx.store, now, CHANNEL, TS, "eyes").unwrap();
    let other = slack::react(&mut fx.store, now, CHANNEL, TS, "github-check-mark").unwrap();

    assert!(first.queued, "the write is queued, not sent");
    assert_eq!(
        again.mutation_id, first.mutation_id,
        "a second click on (channel, ts, emoji) returns the active mutation"
    );
    assert_ne!(
        other.mutation_id, first.mutation_id,
        "another emoji is its own"
    );
    assert_eq!(
        fx.mutations(),
        vec![
            (
                "reaction".into(),
                KEY.into(),
                vars(CHANNEL, TS, "eyes"),
                "pending".into()
            ),
            (
                "reaction".into(),
                KEY.into(),
                vars(CHANNEL, TS, "github-check-mark"),
                "pending".into()
            ),
        ],
        "one row per (channel, ts, emoji) on the channel query, nothing sent yet"
    );
    assert!(
        fx.calls.lock().unwrap().is_empty(),
        "enqueue runs no helper"
    );
}

#[test]
fn slack_reaction_overlay_updates_the_post_row_at_once() {
    let mut fx = fx((0, "{}"));
    fx.seed_message();
    let now = fx.now();

    slack::react(&mut fx.store, now, CHANNEL, TS, "eyes").unwrap();
    slack::react(&mut fx.store, now, CHANNEL, TS, "github-check-mark").unwrap();

    let view = slack::overlaid(&mut fx.store, CHANNEL, TS)
        .unwrap()
        .expect("the message row");
    assert_eq!(
        view.reactions,
        [("eyes".to_owned(), 2), ("github-check-mark".to_owned(), 1)].into(),
        "each active reaction adds one to its emoji"
    );
    assert_eq!(
        view.eyes_me,
        Some(true),
        "the User's own :eyes: is the claim"
    );
    assert_eq!(
        overlay::active(&mut fx.store, "slack", KEY).unwrap().len(),
        2,
        "the overlay reads the active mutations of the channel query"
    );
    assert_eq!(
        fx.count("SELECT eyes_me FROM swr_slack_posts"),
        0,
        "the cached row is untouched until the next read"
    );
}

#[test]
fn slack_transient_reaction_failure_retries_after_600_seconds() {
    let mut fx = fx((3, FAILED));
    let now = fx.now();
    slack::react(&mut fx.store, now, CHANNEL, TS, "eyes").unwrap();

    let outcome = fx.execute();

    assert_eq!(
        outcome,
        ExecOutcome::Retry {
            after: Duration::from_secs(600)
        },
        "§4.3: any code that is not a refusal retries in 600 s"
    );
    let calls = fx.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1, "one helper run");
    let args = &calls[0].args;
    let pair = |name: &str, value: &str| args.windows(2).any(|w| w[0] == name && w[1] == value);
    assert!(
        args.iter().any(|a| a.ends_with("slack-react.ts"))
            && pair("--channel", CHANNEL)
            && pair("--ts", TS)
            && pair("--emoji", "eyes"),
        "{args:?}"
    );
}

#[test]
fn slack_refused_reaction_executes_as_failed_with_the_slack_error_name() {
    let mut fx = fx((3, REFUSED));
    let now = fx.now();
    slack::react(&mut fx.store, now, CHANNEL, TS, "nope").unwrap();

    let outcome = fx.execute();

    assert_eq!(
        outcome,
        ExecOutcome::Failed {
            error: "invalid_name".into()
        },
        "§4.3: a refusal is Failed with Slack's error name"
    );
    outbox::fail(&mut fx.store, 1, now, "invalid_name", Retry::Never).unwrap();
    assert_eq!(
        fx.count(
            "SELECT count(*) FROM events WHERE kind = 'error' AND data LIKE '%slack_refused%'"
        ),
        1,
        "one `error` Event slack_refused, raised by the refusal"
    );
    assert_eq!(
        slack::overlaid(&mut fx.store, CHANNEL, TS).unwrap(),
        None,
        "a failed reaction contributes nothing to the overlay"
    );
}

/// `pr <argv>` through the real command on `ports`.
#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: crate::pr::Command,
}

fn pr(ports: &Ports, argv: &[&str]) -> Result<Output, CpdError> {
    let cli = Cli::try_parse_from(std::iter::once("pr").chain(argv.iter().copied())).unwrap();
    Ports::scope(ports, || crate::pr::run(ports, cli.command))
}

struct Post {
    rig: crate::janitor_tests::Rig,
    calls: Arc<Mutex<Vec<Call>>>,
}

/// A ready, authored pull request `acme/pay#801` of Workflow `api`, posting enabled to `C0POLICY1`, the helper answering `SENT`.
fn post_fixture() -> Post {
    post_fixture_with((0, SENT))
}

/// [`post_fixture`] with the helper answering `reply` to every call.
fn post_fixture_with(reply: (i32, &'static str)) -> Post {
    let mut rig = crate::janitor_tests::Rig::new();
    rig.workflow("api", "open");
    let root = rig.home.root().to_path_buf();
    let adapter = stage_pi(&root);
    std::fs::write(
        rig.home.config(),
        format!(
            "version = 1\nmachine = \"t\"\npi_adapter = \"{}\"\n[inbox]\nteam_channel = \"C0MACHINE1\"\nslack_user_id = \"U0USER001\"\n[tmux]\nsocket = \"s\"\n",
            adapter.display()
        ),
    )
    .unwrap();
    let policy = root.join("policy/acme");
    std::fs::create_dir_all(&policy).unwrap();
    std::fs::write(
        policy.join("owner.toml"),
        "[pr.post]\nenabled = true\nchannel = \"C0POLICY1\"\nmerged_reaction = \"merge\"\n",
    )
    .unwrap();
    std::fs::write(policy.join("pay.toml"), "").unwrap();
    rig.sql(
        "UPDATE pull_requests SET mergeable = 'mergeable', review_decision = 'none', checks = 'passed', authored = 1, feedback = '[]', merge_state = 'clean'
           WHERE number = 801;",
    );
    // SAFETY: one test per process (nextest).
    unsafe { std::env::set_var("CPD_HOME", &root) };
    let calls = Arc::new(Mutex::new(Vec::new()));
    let _ = (*rig.fakes.runner)
        .clone()
        .route(
            "node",
            Arc::new(Helper {
                reply,
                calls: calls.clone(),
            }),
        )
        .route("gh", Arc::new(Gh));
    Post { rig, calls }
}

impl Post {
    fn store(&self) -> Store {
        Store::open(&self.rig.home, 5000).unwrap()
    }

    fn count(&self, sql: &str) -> i64 {
        self.store()
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    /// The `post` and `repost` mutations: `(op_kind, target_key, vars, status)`.
    fn mutations(&self) -> Vec<(String, String, Value, String)> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare(
                    "SELECT op_kind, target_key, vars_json, status FROM swr_mutations WHERE op_kind IN ('post', 'repost') ORDER BY id",
                )?;
                let rows = stmt.query_map([], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?, r.get(3)?))
                })?;
                Ok(rows
                    .collect::<rusqlite::Result<Vec<_>>>()?
                    .into_iter()
                    .map(|(o, t, v, s)| (o, t, serde_json::from_str(&v).unwrap(), s))
                    .collect())
            })
            .unwrap()
    }

    fn sent(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    /// `execute` of the oldest due `slack` mutation, as E24 claims it.
    fn execute(&self) -> ExecOutcome {
        let ports = &self.rig.fakes.ports;
        let now = ports.clock.utc();
        let home = &self.rig.home;
        let config = Config::load(home).unwrap();
        let mut store = self.store();
        let m = outbox::claim_next(&mut store, now, "slack")
            .unwrap()
            .expect("a due slack mutation");
        let mut cx = Cx {
            store: &mut store,
            ports,
            home,
            config: &config,
        };
        Slack.execute(&mut cx, &m)
    }
}

#[test]
fn slack_post_is_an_outbox_op_and_the_overlay_marks_the_pr_posting() {
    let fx = post_fixture();
    let ports = fx.rig.fakes.ports.clone();

    let out = pr(&ports, &["post", "--plan", "pay", "--workflow", "api"]).expect("queues the post");

    let Output::Success(fields) = out else {
        panic!("a success")
    };
    assert_eq!(fields.get("queued"), Some(&json!(true)), "{fields:?}");
    let id = fields
        .get("mutation_id")
        .and_then(Value::as_i64)
        .expect("the mutation id");
    assert_eq!(fx.sent(), 0, "pr post sends nothing: E24 does");
    assert_eq!(
        fx.count("SELECT count(*) FROM pr_posts"),
        0,
        "the pr_posts row does not exist until the send answers"
    );
    let mutations = fx.mutations();
    assert_eq!(mutations.len(), 1, "{mutations:?}");
    let (op, target, vars, status) = &mutations[0];
    assert_eq!(
        (op.as_str(), target.as_str(), status.as_str()),
        ("post", "slack:channel:C0POLICY1", "pending")
    );
    assert_eq!(vars["channel"], "C0POLICY1");
    assert_eq!(
        (
            vars["plan"].as_str(),
            vars["workflow"].as_str(),
            vars["number"].as_i64()
        ),
        (Some("pay"), Some("api"), Some(801))
    );
    assert!(
        vars["text"].as_str().is_some_and(|t| !t.is_empty()),
        "the text is rendered at enqueue: {vars}"
    );
    assert!(
        slack::posting(&mut fx.store(), "pay", "api").unwrap(),
        "the overlay marks the Workflow's pull request as posting"
    );
    // The one guard: a second `pr post` while the first is active is already_posted (§5.1).
    let again = pr(&ports, &["post", "--plan", "pay", "--workflow", "api"]).unwrap_err();
    assert_eq!(again.code().as_str(), "already_posted");
    assert_eq!(again.details()["mutation_id"], id);
    assert_eq!(fx.mutations().len(), 1);
}

#[test]
fn slack_repost_goes_through_the_outbox() {
    let fx = post_fixture();
    fx.rig.sql(
        "INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, last_posted_at, auto_merge)
           VALUES ('pay', 'api', 'C0POLICY1', '1700000000.000100', 'https://slack.test/p1', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z', 'none')",
    );
    let ports = fx.rig.fakes.ports.clone();

    let first = pr(&ports, &["repost", "--plan", "pay", "--workflow", "api"]).expect("queues");
    let second = pr(&ports, &["repost", "--plan", "pay", "--workflow", "api"]).expect("queues");

    let (Output::Success(a), Output::Success(b)) = (first, second) else {
        panic!("successes")
    };
    assert_eq!(
        (a["queued"].clone(), b["queued"].clone()),
        (json!(true), json!(true))
    );
    assert_ne!(
        a["mutation_id"], b["mutation_id"],
        "each repost is one reminder: no coalescing"
    );
    assert_eq!(fx.sent(), 0, "pr repost sends nothing: E24 does");
    let mutations = fx.mutations();
    assert_eq!(mutations.len(), 2, "{mutations:?}");
    for (op, target, vars, status) in &mutations {
        assert_eq!(
            (op.as_str(), target.as_str(), status.as_str()),
            ("repost", "slack:channel:C0POLICY1", "pending")
        );
        assert_eq!(vars["channel"], "C0POLICY1");
        assert_eq!(vars["thread_ts"], "1700000000.000100");
        assert_eq!(vars["text"], "Bibbidi-bobbidi-bump!");
    }
    assert_eq!(
        fx.count("SELECT count(*) FROM pr_posts WHERE last_posted_at = '2026-01-01T00:00:00.000Z'"),
        1,
        "last_posted_at moves when the send completes, not at enqueue"
    );
}

#[test]
fn slack_failed_post_drops_the_posting_overlay() {
    let fx = post_fixture();
    let ports = fx.rig.fakes.ports.clone();
    pr(&ports, &["post", "--plan", "pay", "--workflow", "api"]).expect("queues the post");
    assert!(slack::posting(&mut fx.store(), "pay", "api").unwrap());
    let now = ports.clock.utc();
    let mut store = fx.store();
    let m = outbox::claim_next(&mut store, now, "slack")
        .unwrap()
        .expect("the queued post");

    outbox::fail(&mut store, m.id, now, "slack_auth", Retry::Never).unwrap();

    assert!(
        !slack::posting(&mut store, "pay", "api").unwrap(),
        "a failed post contributes nothing to the overlay (swr.md §9)"
    );
    assert_eq!(
        fx.count("SELECT count(*) FROM pr_posts"),
        0,
        "and no row was invented"
    );
}

// ---- inbox claim ----

const SUBJECT: &str = "pr:acme/pay#5";
const CLAIM_CHANNEL: &str = "C02JU6C2Z1D";
const TS_RAW: &str = "1767225600.000100";

#[test]
fn slack_claim_eyes_reaction_goes_through_the_outbox() {
    use crate::inbox::cmd::{DismissArgs, claim_in};
    use crate::inbox::model::{Condition, Kind, Urgency, reconcile};

    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    std::fs::write(
        home.config(),
        "version = 1\nmachine = \"m1\"\nlead_tier = \"S4\"\nlead_runner = \"pi\"\n[tmux]\nsocket = \"t\"\n",
    )
    .unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let condition = Condition {
        kind: Kind::ReviewRequested,
        urgency: Urgency::Blue,
        subject: SUBJECT.into(),
        label: "acme/pay#5 review requested".into(),
        url: Some("https://github.com/acme/pay/pull/5".into()),
        context: None,
        plan: None,
        job: None,
        session: None,
        record: None,
        event: None,
        opened_at: None,
        head_oid: Some("5555555555555555555555555555555555555555".into()),
        reviewed_oid: Some("1111111111111111111111111111111111111111".into()),
    };
    reconcile(&mut store, &[condition], "2026-01-01T00:00:00.000Z").unwrap();
    let post = json!({"repo": "acme/pay", "number": 5, "ts": 1767225600.0, "ts_raw": TS_RAW,
        "author": "Ryan Case", "primary": true, "reactions": [], "claim": null, "eyes_me": null, "thread_tags_me": false});
    crate::inbox::team::seed_cache(&mut store, CLAIM_CHANNEL, &[post]);
    store
        .write("fixture", |tx| {
            tx.execute_batch("PRAGMA defer_foreign_keys = ON;")?;
            Ok(tx.execute_batch(
                "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('codereview', 'codereview', 'clayton-duarte', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, state, state_at, created_at)
                   VALUES ('mgr', 'manager', 'codereview', NULL, NULL, 'codereview-manager', '/s/mgr.jsonl', '/repo', 'O4', 'idle', 't', 't');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, runner, asleep_at)
                   VALUES ('oldlead', 'lead', 'codereview', 'r-pay-5', 'Old', '/', 'S4', 'stopped', 't', 't', 'pi', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, archived_at, done_at, pr_subject, changes_requested_at)
                   VALUES ('r-pay-5', 'codereview', 'Review pay#5', 'grounding', 'oldlead', 't', NULL, NULL, NULL, 'pr:acme/pay#5', 't');",
            )?)
        })
        .unwrap();
    let graphql = json!({"data": {"viewer": {"login": "clayton-duarte"}, "p0": {"pullRequest": {
        "title": "Add retries", "isDraft": false, "state": "OPEN", "reviewDecision": "CHANGES_REQUESTED",
        "headRefOid": "5555555555555555555555555555555555555555",
        "commits": {"nodes": [{"commit": {"statusCheckRollup": {"state": "SUCCESS"}}}]}, "author": {"login": "ryan"},
        "latestReviews": {"nodes": [{"state": "CHANGES_REQUESTED", "author": {"login": "clayton-duarte"},
                                      "commit": {"oid": "1111111111111111111111111111111111111111"}}]}}}}})
    .to_string();
    let runner =
        crate::host::runner::FakeRunner::with_tmux().on("gh", |_| true, Reply::ok(&graphql));
    let ports = Ports {
        clock: Arc::new(crate::clock::ManualClock::new()),
        runner: Arc::new(runner.clone()),
        spawn: Arc::new(crate::host::spawn::FakeSpawn::new()),
        ..Ports::real()
    };
    // SAFETY: one test per process (nextest): the fake tmux's server key and the home are this test's.
    unsafe {
        std::env::set_var("TMUX_TMPDIR", dir.path());
        std::env::set_var("HOME", dir.path());
        std::env::set_var("CPD_HOME", home.root());
        std::env::remove_var("CPD_SESSION");
        std::env::remove_var("PI_SESSION_ID");
    }
    crate::host::Tmux::new("t")
        .with_runner(Arc::new(runner.clone()))
        .run(&["new-session", "-d", "-s", "cpd", "sleep", "600"].map(str::to_owned))
        .unwrap();
    let env = crate::env::Env::open_home_with(&ports, home.clone()).unwrap();

    claim_in(&ports, env, DismissArgs { id: 1 }).expect("the claim succeeds");

    let rows: Vec<(String, String, String, String)> = store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT op_kind, target_key, vars_json, status FROM swr_mutations WHERE system = 'slack' ORDER BY id",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap();
    assert_eq!(
        rows,
        vec![(
            "reaction".into(),
            format!("slack:channel:{CLAIM_CHANNEL}"),
            vars(CLAIM_CHANNEL, TS_RAW, "eyes"),
            "pending".into()
        )],
        "the claim's :eyes: is one reaction mutation (§5.2)"
    );
    let legacy: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM slack_reactions", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(
        legacy, 0,
        "nothing is written to the retired slack_reactions queue"
    );
}

const OK_REACT: &str =
    r#"{"ok":true,"channel":"C0TEAM001","ts":"1760951522.011300","emoji":"eyes","already":false}"#;
const ALREADY_REACTED: &str =
    r#"{"ok":true,"channel":"C0TEAM001","ts":"1760951522.011300","emoji":"eyes","already":true}"#;

/// The owner's policy text naming `emoji` as the merge reaction (none when `None`).
fn owner_policy(emoji: Option<&str>) -> String {
    let reaction = emoji.map_or(String::new(), |e| format!("merged_reaction = \"{e}\"\n"));
    format!("[pr.post]\nenabled = true\nchannel = \"C0POLICY1\"\n{reaction}")
}

impl Fx {
    /// Plan `pay` with Workflow `repo` (Context owner `acme`, its open pull request), its post on `CHANNEL`/`TS` with the
    /// merge reaction `git-merge`, and the owner's policy file `policy/acme/owner.toml` with `policy` as its text.
    fn merge_fixture(&mut self, policy: &str) {
        self.sql(&format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, merge_state, authored)
               VALUES ('acme/pay', 812, 'https://github.com/acme/pay/pull/812', 'Pay', 0, 'open', 'none', 'none', 't', 't', 'clean', 1);
             INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
               VALUES ('pay', 'ctx', 'Pay', 'deploy', 'lead1', '1999-12-31T00:00:00.000Z', '2000-01-01T00:00:00.000Z');
             INSERT INTO workflows (plan, key, repo, base, name, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                                    pr_number, pr_url, pr_opened_at, pull_request)
               VALUES ('pay', 'repo', '/r', 'main', 'repo', 'open', 'cpd/pay/repo', '/w', cpd_txn(), 't', 't',
                       812, 'https://github.com/acme/pay/pull/812', 't', (SELECT id FROM pull_requests WHERE number = 812));
             INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, reaction, reacted_at)
               VALUES ('pay', 'repo', '{CHANNEL}', '{TS}', 'https://acme.slack.com/archives/{CHANNEL}/p1', 't', 'git-merge', NULL);"
        ));
        let dir = crate::policy::policy_dir(&self.home).join("acme");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("owner.toml"), policy).unwrap();
    }

    /// The merge reaction of Workflow `repo`, enqueued the way the merge does (`react_merge_in`).
    fn enqueue_merge(&mut self) {
        self.store
            .write("merge", |tx| {
                slack::react_merge_in(tx, "pay", "repo").map_err(Into::into)
            })
            .unwrap();
    }

    /// `due` of the Slack Source as E24 calls it.
    fn due(&mut self) -> Vec<String> {
        let now = self.now();
        let mut cx = Cx {
            store: &mut self.store,
            ports: &self.fakes.ports,
            home: &self.home,
            config: &self.config,
        };
        Slack.due(&mut cx, now).unwrap()
    }
}

/// The argv of the one helper run, as a pair test.
fn helper_args(fx: &Fx) -> Vec<String> {
    let calls = fx.calls.lock().unwrap();
    assert_eq!(calls.len(), 1, "one helper run");
    calls[0].args.clone()
}

#[test]
fn slack_queued_reaction_is_sent_and_done() {
    let mut fx = fx((0, OK_REACT));
    let now = fx.now();
    slack::react(&mut fx.store, now, CHANNEL, TS, "eyes").unwrap();

    assert_eq!(fx.execute(), ExecOutcome::Done, "the helper placed it");
    let args = helper_args(&fx);
    let pair = |name: &str, value: &str| args.windows(2).any(|w| w[0] == name && w[1] == value);
    assert!(
        args.iter().any(|a| a.ends_with("slack-react.ts"))
            && pair("--channel", CHANNEL)
            && pair("--ts", TS)
            && pair("--emoji", "eyes"),
        "{args:?}"
    );
}

#[test]
fn slack_already_reacted_answer_counts_as_done() {
    let mut fx = fx((0, ALREADY_REACTED));
    let now = fx.now();
    slack::react(&mut fx.store, now, CHANNEL, TS, "eyes").unwrap();

    assert_eq!(
        fx.execute(),
        ExecOutcome::Done,
        "a reaction that is already there is done"
    );
}

#[test]
fn slack_merge_reaction_places_the_emoji_the_policy_names_at_execute() {
    let mut fx = fx((0, OK_REACT));
    fx.merge_fixture(&owner_policy(Some("rocket")));
    fx.enqueue_merge();

    assert_eq!(fx.execute(), ExecOutcome::Done);
    let args = helper_args(&fx);
    assert!(
        args.windows(2)
            .any(|w| w[0] == "--emoji" && w[1] == "rocket"),
        "the emoji is the policy's now, not the queued git-merge: {args:?}"
    );
}

#[test]
fn slack_merge_reaction_whose_policy_names_no_emoji_is_failed_policy_empty() {
    let mut fx = fx((0, OK_REACT));
    fx.merge_fixture(&owner_policy(None));
    fx.enqueue_merge();

    assert_eq!(
        fx.execute(),
        ExecOutcome::Failed {
            error: "policy_empty".into()
        },
        "a policy that names no emoji parks the reaction"
    );
    assert!(
        fx.calls.lock().unwrap().is_empty(),
        "no helper run for an empty policy"
    );
    assert_eq!(
        fx.count("SELECT count(*) FROM events WHERE kind = 'error' AND data LIKE '%policy_empty%'"),
        1,
        "one error Event naming the policy"
    );
}

#[test]
fn slack_merge_reaction_whose_policy_does_not_resolve_sends_the_queued_emoji() {
    let mut fx = fx((0, OK_REACT));
    fx.merge_fixture("[pr.post\nthis is not toml");
    fx.enqueue_merge();

    assert_eq!(
        fx.execute(),
        ExecOutcome::Done,
        "a policy that does not resolve leaves the queued emoji standing"
    );
    let args = helper_args(&fx);
    assert!(
        args.windows(2)
            .any(|w| w[0] == "--emoji" && w[1] == "git-merge"),
        "{args:?}"
    );
}

#[test]
fn slack_parked_merge_reaction_is_requeued_when_the_policy_names_another_emoji() {
    let mut fx = fx((0, OK_REACT));
    fx.merge_fixture(&owner_policy(Some("rocket")));
    fx.enqueue_merge();
    fx.sql("UPDATE swr_mutations SET status = 'failed', error = 'invalid_name' WHERE op_kind = 'reaction';");

    // `due` also asks for the channel query (never fetched); the parked row is requeued alongside it.
    fx.due();
    let reopened: Vec<String> = fx
        .mutations()
        .into_iter()
        .filter(|(op, _, _, status)| op == "reaction" && status == "pending")
        .map(|(_, _, vars, _)| vars)
        .collect();
    assert_eq!(
        reopened.len(),
        1,
        "one new pending row: {:?}",
        fx.mutations()
    );
    assert!(
        reopened[0].contains("\"emoji\":\"rocket\""),
        "{}",
        reopened[0]
    );

    assert_eq!(fx.execute(), ExecOutcome::Done);
    let args = helper_args(&fx);
    assert!(
        args.windows(2)
            .any(|w| w[0] == "--emoji" && w[1] == "rocket"),
        "{args:?}"
    );
}

#[test]
fn slack_refused_reaction_without_a_plan_raises_an_error_naming_no_policy() {
    let mut fx = fx((3, REFUSED));
    let now = fx.now();
    slack::react(&mut fx.store, now, CHANNEL, TS, "nope").unwrap();

    assert_eq!(
        fx.execute(),
        ExecOutcome::Failed {
            error: "invalid_name".into()
        }
    );
    assert_eq!(
        fx.count("SELECT count(*) FROM events WHERE kind = 'error' AND data LIKE '%slack_refused%' AND data LIKE '%policy%'"),
        0,
        "a reaction with no Plan names no policy file"
    );
}

#[test]
fn slack_refused_merge_reaction_names_its_emoji_and_policy_file() {
    let mut fx = fx((3, REFUSED));
    fx.merge_fixture(&owner_policy(Some("git-merge")));
    fx.enqueue_merge();

    assert_eq!(
        fx.execute(),
        ExecOutcome::Failed {
            error: "invalid_name".into()
        }
    );
    assert_eq!(
        fx.count("SELECT count(*) FROM events WHERE kind = 'error' AND data LIKE '%slack_refused%' AND data LIKE '%git-merge%' AND data LIKE '%owner.toml%'"),
        1,
        "one error Event naming the emoji and the policy file to fix"
    );
}

const AUTH: &str = r#"{"ok":false,"error":{"code":"slack_auth","message":"Slack needs a sign-in. Run: pi mcp login slack","details":{}}}"#;

/// The send of a queued post (swr-slack.md 5): the message is the one rendered at enqueue, the helper runs with the pi entry and
/// the channel, and the `pr_posts` row is written when the send answers, with the owner's merge reaction.
#[test]
fn slack_post_sends_the_message_and_records_its_pr_posts_row() {
    let fx = post_fixture();
    let ports = fx.rig.fakes.ports.clone();
    pr(&ports, &["post", "--plan", "pay", "--workflow", "api"]).expect("queues the post");
    let text = fx.mutations()[0].2["text"].as_str().unwrap().to_owned();

    assert_eq!(fx.execute(), ExecOutcome::Done);

    let calls = fx.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1, "one helper run");
    assert_eq!(calls[0].stdin.as_deref(), Some(text.as_bytes()));
    let row: (String, String, String, Option<String>, Option<String>) = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT channel, ts, link, reaction, reacted_at FROM pr_posts WHERE plan = 'pay' AND workflow = 'api'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?)
        })
        .unwrap();
    assert_eq!(
        row,
        (
            "C0POLICY1".to_owned(),
            "1700000000.000100".to_owned(),
            "https://slack.test/p1".to_owned(),
            Some("merge".to_owned()),
            None
        ),
        "the reaction is the owner's merged_reaction; reacted_at waits for E24's reaction"
    );
}

#[test]
fn slack_post_runs_the_helper_with_the_pi_entry_and_the_channel() {
    let fx = post_fixture();
    let ports = fx.rig.fakes.ports.clone();
    pr(&ports, &["post", "--plan", "pay", "--workflow", "api"]).expect("queues the post");

    fx.execute();

    let calls = fx.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    let root = fx.rig.home.root().canonicalize().unwrap();
    let script = root.join("adapter/lib/slack-post.ts");
    let entry = root
        .join("install/releases/1.2.0/node_modules/@earendil-works/pi-coding-agent/dist/index.js");
    let argv: Vec<&str> = call.args.iter().map(String::as_str).collect();
    assert_eq!(
        argv,
        [
            script.to_str().unwrap(),
            "--coding-agent",
            entry.to_str().unwrap(),
            "--channel",
            "C0POLICY1"
        ]
    );
    assert_eq!(call.deadline, Duration::from_secs(60));
}

#[test]
fn slack_post_to_the_machine_channel_when_the_owner_names_none() {
    // The helper answers the channel it posted to, which the pr_posts row takes.
    let fx = post_fixture_with((
        0,
        r#"{"ok":true,"channel":"C0MACHINE1","ts":"1700000000.000100","link":"https://slack.test/p1"}"#,
    ));
    let ports = fx.rig.fakes.ports.clone();
    let policy = crate::policy::policy_dir(&fx.rig.home).join("acme");
    std::fs::write(
        policy.join("owner.toml"),
        "[pr.post]\nenabled = true\nmerged_reaction = \"merge\"\n",
    )
    .unwrap();
    pr(&ports, &["post", "--plan", "pay", "--workflow", "api"]).expect("queues the post");

    assert_eq!(fx.execute(), ExecOutcome::Done);

    let calls = fx.calls.lock().unwrap().clone();
    assert_eq!(calls[0].args.last().map(String::as_str), Some("C0MACHINE1"));
    assert_eq!(
        fx.count("SELECT count(*) FROM pr_posts WHERE channel = 'C0MACHINE1'"),
        1,
        "the row names the channel the helper posted to"
    );
}

#[test]
fn slack_post_refused_for_a_sign_in_records_nothing() {
    let fx = post_fixture_with((3, AUTH));
    let ports = fx.rig.fakes.ports.clone();
    pr(&ports, &["post", "--plan", "pay", "--workflow", "api"]).expect("queues the post");

    let ExecOutcome::Failed { error } = fx.execute() else {
        panic!("a sign-in refusal fails the post")
    };
    assert!(error.starts_with("slack_auth"), "{error}");
    assert_eq!(
        fx.count("SELECT count(*) FROM pr_posts"),
        0,
        "nothing recorded"
    );
}

#[test]
fn slack_post_whose_helper_prints_nothing_fails_the_send() {
    let fx = post_fixture_with((1, ""));
    let ports = fx.rig.fakes.ports.clone();
    pr(&ports, &["post", "--plan", "pay", "--workflow", "api"]).expect("queues the post");

    let ExecOutcome::Failed { error } = fx.execute() else {
        panic!("a helper that prints nothing fails the post")
    };
    assert!(error.starts_with("spawn_failed"), "{error}");
    assert_eq!(
        fx.count("SELECT count(*) FROM pr_posts"),
        0,
        "nothing recorded"
    );
}

#[test]
fn slack_post_sends_the_message_verbatim_without_escaping() {
    let fx = post_fixture();
    let ports = fx.rig.fakes.ports.clone();
    fx.rig
        .sql("UPDATE pull_requests SET title = 'Fix a & b <c> \"d\"' WHERE number = 801;");
    pr(&ports, &["post", "--plan", "pay", "--workflow", "api"]).expect("queues the post");
    let text = fx.mutations()[0].2["text"].as_str().unwrap().to_owned();
    assert!(text.starts_with("Fix a & b <c> \"d\"\n"), "{text}");

    fx.execute();

    assert_eq!(
        fx.calls.lock().unwrap()[0].stdin.as_deref(),
        Some(text.as_bytes())
    );
}

/// A post of Workflow `api` stored long ago (`FIRST`), as the first repost needs.
const REPOST_FIRST: &str = "2026-01-01T00:00:00.000Z";

fn seeded_post() -> Post {
    seeded_post_with((0, SENT))
}

/// [`seeded_post`] with the helper answering `reply`.
fn seeded_post_with(reply: (i32, &'static str)) -> Post {
    let fx = post_fixture_with(reply);
    fx.rig.sql(&format!(
        "INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, last_posted_at, auto_merge)
           VALUES ('pay', 'api', 'C0POLICY1', '1700000000.000100', 'https://slack.test/p1', '{REPOST_FIRST}', '{REPOST_FIRST}', 'none')"
    ));
    fx
}

/// The send of a queued repost (swr-slack.md 5): the broadcast in the thread of the post, and `last_posted_at` moves when
/// the send completes, each time.
#[test]
fn slack_repost_sends_the_broadcast_and_moves_last_posted_at_each_time() {
    let fx = seeded_post();
    let ports = fx.rig.fakes.ports.clone();
    pr(&ports, &["repost", "--plan", "pay", "--workflow", "api"]).expect("queues");
    pr(&ports, &["repost", "--plan", "pay", "--workflow", "api"]).expect("queues");
    let last_at = |fx: &Post| -> String {
        fx.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT last_posted_at FROM pr_posts WHERE plan = 'pay' AND workflow = 'api'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    };
    assert_eq!(last_at(&fx), REPOST_FIRST, "nothing moves at enqueue");

    assert_eq!(fx.execute(), ExecOutcome::Done);
    let calls = fx.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    let args: Vec<&str> = calls[0].args.iter().map(String::as_str).collect();
    assert_eq!(
        args[args.len() - 6..],
        [
            "--channel",
            "C0POLICY1",
            "--thread-ts",
            "1700000000.000100",
            "--reply-broadcast",
            "true"
        ]
    );
    assert_eq!(
        calls[0].stdin.as_deref(),
        Some(&b"Bibbidi-bobbidi-bump!"[..])
    );
    let after_first = last_at(&fx);
    assert!(after_first.as_str() > REPOST_FIRST, "{after_first}");

    // E24 completes a sent mutation before it claims the next one of the same channel.
    let first_id: i64 = fx
        .count("SELECT min(id) FROM swr_mutations WHERE op_kind = 'repost'")
        .into();
    let now = fx.rig.fakes.ports.clock.utc();
    outbox::complete(&mut fx.store(), first_id, now).unwrap();

    assert_eq!(fx.execute(), ExecOutcome::Done);
    let after_second = last_at(&fx);
    assert!(
        after_second > after_first,
        "{after_first} then {after_second}"
    );
}

#[test]
fn slack_repost_refused_for_a_sign_in_leaves_last_posted_at() {
    let fx = seeded_post_with((3, AUTH));
    let ports = fx.rig.fakes.ports.clone();
    pr(&ports, &["repost", "--plan", "pay", "--workflow", "api"]).expect("queues");

    let ExecOutcome::Failed { error } = fx.execute() else {
        panic!("a sign-in refusal fails the repost")
    };
    assert!(error.starts_with("slack_auth"), "{error}");
    assert_eq!(
        fx.count("SELECT count(*) FROM pr_posts WHERE last_posted_at = '2026-01-01T00:00:00.000Z'"),
        1,
        "last_posted_at does not move on a failed send"
    );
}

/// The send of a queued post queues the Jira `in_review` transition in the transaction that writes the `pr_posts` row
/// (swr-slack.md 4: the Jira row is part of the send's completion, not of the enqueue).
#[test]
fn slack_post_send_queues_the_in_review_transition() {
    let fx = post_fixture();
    let ports = fx.rig.fakes.ports.clone();
    pr(&ports, &["post", "--plan", "pay", "--workflow", "api"]).expect("queues the post");
    assert_eq!(
        fx.count("SELECT count(*) FROM jira_transitions"),
        0,
        "nothing is queued at enqueue"
    );

    assert_eq!(fx.execute(), ExecOutcome::Done);

    assert_eq!(
        fx.count("SELECT count(*) FROM jira_transitions WHERE plan = 'pay' AND pr = 'api' AND target = 'in_review'"),
        1,
        "the in_review row is queued with the pr_posts row"
    );
}

/// The send of a queued post runs the Slack helper only: no Jira helper, no Atlassian server.
#[test]
fn slack_post_send_runs_only_the_slack_helper() {
    let fx = post_fixture();
    let ports = fx.rig.fakes.ports.clone();
    pr(&ports, &["post", "--plan", "pay", "--workflow", "api"]).expect("queues the post");

    fx.execute();

    let scripts: Vec<String> = fx
        .calls
        .lock()
        .unwrap()
        .iter()
        .map(|c| c.args[0].rsplit('/').next().unwrap().to_owned())
        .collect();
    assert_eq!(
        scripts,
        ["slack-post.ts"],
        "node sees only the Slack helper"
    );
}
