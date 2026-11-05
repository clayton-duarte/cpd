//! The post ledger of `pr post` (decision extending pro-d408): the `pr_posts` row is written BEFORE the Slack send and
//! dropped only when the send is known not to have happened. In process on the janitor's `Rig`: the Slack helper (`node`) and
//! `gh` are scripted on the runner, never real.

use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use time::macros::datetime;

use super::post::{args, post};
use crate::error::CpdError;
use crate::host::runner::{Call, Handler, Reply};
use crate::ids::{PlanId, WorkflowKey};
use crate::inbox::rules::conditions_with;
use crate::janitor_tests::Rig;
use crate::ports::Ports;
use crate::store::Store;

const UNPARSEABLE: &str = r#"{"ok":false,"error":{"code":"slack_unparseable","message":"Slack answered something unreadable.","details":{}}}"#;
const FAILED: &str = r#"{"ok":false,"error":{"code":"slack_failed","message":"Slack refused before sending.","details":{}}}"#;
const READY: &str = "ready for user review";

/// The Slack helper: answers `reply` (exit 3) and records whether the Workflow's `pr_posts` row existed at the send.
struct Helper {
    reply: &'static str,
    row_during_send: Arc<Mutex<Vec<bool>>>,
    home: crate::home::CpdHome,
}

impl Handler for Helper {
    fn handle(&self, _call: &Call) -> Reply {
        let mut store = Store::open(&self.home, 5000).unwrap();
        let row = rows(&mut store) > 0;
        self.row_during_send.lock().unwrap().push(row);
        Reply::exit(3, &format!("{}\n", self.reply), "")
    }
}

/// Every repo allows squash only and no auto-merge: the one `gh` read of `pr post`.
struct Gh;

impl Handler for Gh {
    fn handle(&self, _call: &Call) -> Reply {
        Reply::ok(
            r#"{"data":{"repository":{"autoMergeAllowed":false,"squashMergeAllowed":true,"mergeCommitAllowed":false,"rebaseMergeAllowed":false,"pullRequest":{"id":"PR_1","autoMergeRequest":null}}}}"#,
        )
    }
}

fn rows(store: &mut Store) -> i64 {
    store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM pr_posts", [], |r| r.get(0))?))
        .unwrap()
}

struct Fx {
    rig: Rig,
    sends: Arc<Mutex<Vec<bool>>>,
}

/// A ready, authored pull request `acme/pay#801` of Workflow `api`, posting enabled, the helper answering `reply`.
fn fixture(reply: &'static str) -> Fx {
    let mut rig = Rig::new();
    rig.workflow("api", "open");
    let root = rig.home.root().to_path_buf();
    // pi: a `bin/pi` on PATH and its install tree; the adapter directory the config names.
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
        "[pr.post]\nenabled = true\nchannel = \"C0POLICY1\"\n",
    )
    .unwrap();
    std::fs::write(policy.join("pay.toml"), "").unwrap();
    rig.sql(
        "UPDATE pull_requests SET mergeable = 'mergeable', review_decision = 'none', checks = 'passed', authored = 1, feedback = '[]', merge_state = 'clean'
           WHERE number = 801;",
    );
    // SAFETY: tests of this module run one per process (nextest); the variables point at this test's own temp dir.
    unsafe {
        std::env::set_var("CPD_HOME", &root);
        let old = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&old));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    }
    let sends = Arc::new(Mutex::new(Vec::new()));
    let runner = (*rig.fakes.runner).clone();
    let _ = runner
        .route(
            "node",
            Arc::new(Helper {
                reply,
                row_during_send: sends.clone(),
                home: rig.home.clone(),
            }),
        )
        .route("gh", Arc::new(Gh));
    Fx { rig, sends }
}

impl Fx {
    fn post(&self) -> Result<crate::output::Output, CpdError> {
        let ports = &self.rig.fakes.ports;
        Ports::scope(ports, || {
            post(
                ports,
                args(
                    PlanId::parse("pay").unwrap(),
                    WorkflowKey::parse("api").unwrap(),
                ),
            )
        })
    }

    fn rows(&self) -> i64 {
        rows(&mut Store::open(&self.rig.home, 5000).unwrap())
    }

    fn node_calls(&self) -> usize {
        self.rig
            .fakes
            .runner
            .calls()
            .iter()
            .filter(|c| c.tool == "node")
            .count()
    }

    /// Row 1 of the PR's Inbox item.
    fn status(&self) -> String {
        let mut store = Store::open(&self.rig.home, 5000).unwrap();
        let all = conditions_with(
            &mut store,
            &self.rig.home,
            datetime!(2026-01-02 00:00 UTC),
            20,
            1,
            None,
        )
        .unwrap();
        let item = all
            .into_iter()
            .find(|c| c.subject == "pr:acme/pay#801")
            .expect("the PR has an Inbox row");
        item.label.split('\n').next().unwrap().to_owned()
    }
}

#[test]
fn a_post_whose_send_outcome_is_unknown_keeps_its_pr_posts_row() {
    let fx = fixture(UNPARSEABLE);
    let error = fx.post().unwrap_err();
    assert_eq!(error.code().as_str(), "slack_unparseable", "{error:?}");
    assert_eq!(fx.node_calls(), 1, "the message was sent once");
    assert_eq!(fx.rows(), 1, "the unknown outcome keeps the row");
}

#[test]
fn a_post_whose_send_failed_before_sending_drops_its_pr_posts_row() {
    let fx = fixture(FAILED);
    let error = fx.post().unwrap_err();
    assert_eq!(error.code().as_str(), "slack_failed", "{error:?}");
    assert_eq!(
        *fx.sends.lock().unwrap(),
        [true],
        "the row is written before the send"
    );
    assert_eq!(fx.rows(), 0, "a send known not to have happened drops it");
}

#[test]
fn a_second_post_after_an_unknown_outcome_is_refused_and_sends_nothing() {
    let fx = fixture(UNPARSEABLE);
    let _ = fx.post().unwrap_err();
    let error = fx.post().unwrap_err();
    assert_eq!(error.code().as_str(), "already_posted", "{error:?}");
    assert_eq!(fx.node_calls(), 1, "the mock send was called once in total");
}

#[test]
fn a_post_whose_row_was_written_before_the_send_shows_no_ready_to_post_row() {
    let fx = fixture(UNPARSEABLE);
    assert!(fx.status().ends_with(READY), "before: {}", fx.status());
    let _ = fx.post().unwrap_err();
    let after = fx.status();
    assert!(!after.ends_with(READY), "after an unknown send: {after}");
}
