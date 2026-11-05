//! The post ledger of `pr post`: the `pr_posts` row is written only AFTER the send resolves; an unknown outcome looks the
//! message up in Slack (found: its ts is written; not found: nothing, retryable). In process on the janitor's `Rig`: the Slack helper (`node`) and
//! `gh` are scripted on the runner, never real.

use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use super::post::{args, post};
use crate::error::CpdError;
use crate::host::runner::{Call, Handler, Reply};
use crate::ids::{PlanId, WorkflowKey};
use crate::janitor_tests::Rig;
use crate::ports::Ports;
use crate::store::Store;

const UNPARSEABLE: &str = r#"{"ok":false,"error":{"code":"slack_unparseable","message":"Slack answered something unreadable.","details":{}}}"#;
const FAILED: &str = r#"{"ok":false,"error":{"code":"slack_failed","message":"Slack refused before sending.","details":{}}}"#;
const SENT: &str =
    r#"{"ok":true,"channel":"C0POLICY1","ts":"1700000000.000100","link":"https://slack.test/p1"}"#;
const FOUND: &str = r#"{"ok":true,"found":true,"ts":"1700000000.000100"}"#;
const NOT_FOUND: &str = r#"{"ok":true,"found":false}"#;

/// The Slack helper: answers `reply` (exit 3) and records whether the Workflow's `pr_posts` row existed at the send.
struct Helper {
    sends: Mutex<Vec<&'static str>>,
    lookup: &'static str,
    row_during_send: Arc<Mutex<Vec<bool>>>,
    home: crate::home::CpdHome,
}

impl Handler for Helper {
    fn handle(&self, call: &Call) -> Reply {
        if call.args.iter().any(|a| a.contains("slack-lookup.ts")) {
            return Reply::exit(0, &format!("{}\n", self.lookup), "");
        }
        let mut store = Store::open(&self.home, 5000).unwrap();
        let row = rows(&mut store) > 0;
        self.row_during_send.lock().unwrap().push(row);
        let mut sends = self.sends.lock().unwrap();
        let reply = if sends.len() > 1 {
            sends.remove(0)
        } else {
            sends[0]
        };
        Reply::exit(if reply == SENT { 0 } else { 3 }, &format!("{reply}\n"), "")
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

/// A ready, authored pull request `acme/pay#801` of Workflow `api`, posting enabled, the helper answering `replies` in turn and `lookup` to a lookup.
fn fixture(replies: &[&'static str], lookup: &'static str) -> Fx {
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
                sends: Mutex::new(replies.to_vec()),
                lookup,
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
}

/// Row existence at each send, then a second post: a failing first send then a good one.
#[test]
fn a_post_that_failed_before_sending_writes_no_pr_posts_row_and_can_be_retried() {
    let fx = fixture(&[FAILED, SENT], NOT_FOUND);
    let error = fx.post().unwrap_err();
    assert_eq!(error.code().as_str(), "slack_failed", "{error:?}");
    assert_eq!(*fx.sends.lock().unwrap(), [false], "no row at the send");
    assert_eq!(fx.rows(), 0, "a post that failed wrote nothing");
    fx.post().expect("the retry posts");
    assert_eq!(fx.node_calls(), 2, "the retry sent once more");
    assert_eq!(fx.rows(), 1);
}

#[test]
fn an_unknown_outcome_whose_message_is_found_in_slack_writes_the_found_ts() {
    let fx = fixture(&[UNPARSEABLE], FOUND);
    let _ = fx.post();
    let ts: String = Store::open(&fx.rig.home, 5000)
        .unwrap()
        .read(|c| {
            Ok(
                c.query_row("SELECT ts FROM pr_posts WHERE workflow = 'api'", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap_or_default();
    assert_eq!(ts, "1700000000.000100", "the found message's ts is stored");
    let error = fx.post().unwrap_err();
    assert_eq!(error.code().as_str(), "already_posted", "{error:?}");
}

#[test]
fn an_unknown_outcome_whose_message_is_not_found_writes_nothing_and_can_be_retried() {
    let fx = fixture(&[UNPARSEABLE, SENT], NOT_FOUND);
    let _ = fx.post().unwrap_err();
    assert_eq!(fx.rows(), 0, "an unfound message wrote nothing");
    fx.post().expect("the retry posts");
    assert_eq!(fx.sends.lock().unwrap().len(), 2, "the retry sent again");
}

#[test]
fn a_post_writes_its_row_only_after_the_send_resolves() {
    let fx = fixture(&[SENT], NOT_FOUND);
    fx.post().expect("posts");
    assert_eq!(*fx.sends.lock().unwrap(), [false], "no row at the send");
    assert_eq!(fx.rows(), 1, "the row exists after");
}
