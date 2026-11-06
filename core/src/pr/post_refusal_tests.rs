//! The post ledger of `pr post`: the `pr_posts` row is written only AFTER an OK send; any error writes nothing and is retryable. In process on the janitor's `Rig`: the Slack helper (`node`) and
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
/// An OK send whose empty `ts` the `pr_posts` CHECK refuses: the row write fails after the send.
const SENT_UNSTORABLE: &str =
    r#"{"ok":true,"channel":"C0POLICY1","ts":"","link":"https://slack.test/p0"}"#;
const FOUND: &str = r#"{"ok":true,"found":true,"ts":"1700000000.000100"}"#;

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
        Reply::exit(
            if reply == SENT || reply == SENT_UNSTORABLE {
                0
            } else {
                3
            },
            &format!("{reply}\n"),
            "",
        )
    }
}

/// `gh` answers an empty object: `pr post` reads the cached repo settings, so no test depends on it.
struct Gh;

impl Handler for Gh {
    fn handle(&self, _call: &Call) -> Reply {
        Reply::ok("{}")
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
           WHERE number = 801;
         INSERT INTO swr_repos (repo, default_branch, is_archived, auto_merge_allowed, squash_merge_allowed, merge_commit_allowed,
                                rebase_merge_allowed, delete_branch_on_merge, fetched_at)
           VALUES ('acme/pay', 'main', 0, 0, 1, 0, 0, 1, 't');",
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

    fn ts(&self) -> String {
        Store::open(&self.rig.home, 5000)
            .unwrap()
            .read(|c| {
                Ok(
                    c.query_row("SELECT ts FROM pr_posts WHERE workflow = 'api'", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap()
    }
}

/// Observed through the mocked send: no row when the send is called; after an OK send the row holds the returned ts.
#[test]
fn a_post_writes_its_pr_posts_row_only_after_the_send_resolves() {
    let fx = fixture(&[SENT], FOUND);
    fx.post().expect("posts");
    assert_eq!(*fx.sends.lock().unwrap(), [false], "no row at the send");
    assert_eq!(fx.rows(), 1, "the row exists after");
    assert_eq!(fx.ts(), "1700000000.000100", "the row holds the sent ts");
}

#[test]
fn a_send_failure_writes_no_pr_posts_row_and_a_second_post_sends() {
    let fx = fixture(&[FAILED, SENT], FOUND);
    let error = fx.post().unwrap_err();
    assert_eq!(error.code().as_str(), "slack_failed", "{error:?}");
    assert_eq!(fx.rows(), 0, "a failed post wrote nothing");
    fx.post().expect("the retry posts");
    assert_eq!(fx.sends.lock().unwrap().len(), 2, "the retry sent once");
    assert_eq!(fx.rows(), 1);
}

/// The lookup helper answers "found" so that any lookup inside `pr post` would write a row.
#[test]
fn an_unparseable_send_response_writes_no_pr_posts_row_and_is_retryable() {
    let fx = fixture(&[UNPARSEABLE, SENT], FOUND);
    let error = fx.post().unwrap_err();
    assert_eq!(error.code().as_str(), "slack_unparseable", "{error:?}");
    assert_eq!(fx.rows(), 0, "an unreadable answer wrote nothing");
    fx.post().expect("the retry posts");
    assert_eq!(fx.sends.lock().unwrap().len(), 2, "the retry sent again");
    assert_eq!(fx.rows(), 1);
}

#[test]
fn a_store_failure_after_the_send_writes_no_pr_posts_row_and_is_retryable() {
    let fx = fixture(&[SENT_UNSTORABLE, SENT], FOUND);
    let error = fx.post().unwrap_err();
    assert_eq!(error.code().as_str(), "store_failed", "{error:?}");
    assert_eq!(fx.rows(), 0, "a failed row write left no row");
    fx.post().expect("the retry posts");
    assert_eq!(fx.sends.lock().unwrap().len(), 2, "the retry sent again");
    assert_eq!(fx.rows(), 1);
}
