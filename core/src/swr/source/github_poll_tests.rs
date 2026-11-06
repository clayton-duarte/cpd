//! The `auto_merge` executor asks E5 for an immediate poll (`run/pr-poll-now`) after a successful enable, and after nothing else
//! (swr-github.md §9; decision pro-d394). `gh` is a recording `FakeRunner` route; the run dir is the rig's; no sleep.

use std::sync::Arc;

use serde_json::json;

use super::github::GithubSource;
use super::{Cx, ExecOutcome, Source};
use crate::clock::Clock;
use crate::config::Config;
use crate::host::runner::{Call, Handler, Reply};
use crate::janitor_tests::Rig;
use crate::store::Store;
use crate::swr::outbox::{self, Coalesce, Op};

const KEY: &str = "github:pr:acme/pay#801";

/// GitHub's side: the read says auto-merge is `allowed`, the mutation is refused with `refuse`, or `gh` fails when `fail`.
struct Gh {
    allowed: bool,
    refuse: bool,
    fail: bool,
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let text = call.args.join(" ");
        if self.fail {
            return Reply::exit(1, "", "gh: HTTP 502\n");
        }
        if text.contains("enablePullRequestAutoMerge") {
            return if self.refuse {
                Reply::ok(&json!({"data": null, "errors": [{"message": "refused"}]}).to_string())
            } else {
                Reply::ok(&json!({"data": {"enablePullRequestAutoMerge": {"pullRequest": {"autoMergeRequest": {"enabledAt": "t"}}}}}).to_string())
            };
        }
        Reply::ok(&json!({"data": {"repository": {"autoMergeAllowed": self.allowed, "squashMergeAllowed": true,
            "mergeCommitAllowed": false, "rebaseMergeAllowed": false, "pullRequest": {"id": "PR_1", "autoMergeRequest": null}}}}).to_string())
    }
}

/// Executes one queued `auto_merge` against `gh`; whether `run/pr-poll-now` exists after it, and the outcome.
fn run(gh: Gh) -> (bool, ExecOutcome) {
    let rig = Rig::new();
    let _ = (*rig.fakes.runner).clone().route("gh", Arc::new(gh));
    let now = rig.fakes.clock.utc();
    let mut store = Store::open(&rig.home, 5000).unwrap();
    outbox::enqueue(
        &mut store,
        now,
        Op {
            system: "github".into(),
            target_key: KEY.into(),
            op_kind: "auto_merge".into(),
            vars: json!({"repo": "acme/pay", "number": 801}),
            patch: None,
            context: None,
        },
        Coalesce::LatestWins,
    )
    .unwrap();
    let m = outbox::claim_next(&mut store, now, "github")
        .unwrap()
        .unwrap();
    let config = Config::load(&rig.home).unwrap();
    let outcome = {
        let mut cx = Cx {
            store: &mut store,
            ports: &rig.fakes.ports,
            home: &rig.home,
            config: &config,
        };
        GithubSource::default().execute(&mut cx, &m)
    };
    (
        rig.home.run_dir().join(crate::pr::POLL_NOW).exists(),
        outcome,
    )
}

#[test]
fn gh_auto_merge_requests_a_poll_after_a_successful_enable() {
    let (polled, outcome) = run(Gh {
        allowed: true,
        refuse: false,
        fail: false,
    });
    assert_eq!(outcome, ExecOutcome::Done);
    assert!(polled, "an enabled auto-merge asks for a poll at once");
}

#[test]
fn gh_auto_merge_requests_no_poll_after_a_failed_enable() {
    for (gh, name) in [
        (
            Gh {
                allowed: true,
                refuse: false,
                fail: true,
            },
            "gh failure",
        ),
        (
            Gh {
                allowed: true,
                refuse: true,
                fail: false,
            },
            "refused",
        ),
        (
            Gh {
                allowed: false,
                refuse: false,
                fail: false,
            },
            "repo lacks auto-merge",
        ),
    ] {
        let (polled, _) = run(gh);
        assert!(!polled, "{name}: nothing changed on GitHub");
    }
}
