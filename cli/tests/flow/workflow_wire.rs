//! The wire of the Workflow rename (Plan `glossary-rename`, Context hs-d805; rename map row
//! `ren-red-wire`; research `docs/research/glossary-rename.md` §3.3 and §3.5): the closed Event kind set
//! (contract §5.2) accepts the `workflow-*` kinds and no longer the object `pr-*` ones, the `data` keys
//! the adapter reads become `workflow`/`workflows`, the object refusal codes are `workflow_*`, and
//! `PROTOCOL` is 24 (22 at the rename, 23 with the Lead-less `shaping` phase of Plan ctx-explore, 24 with `session-message`'s optional `data.origin`, Plan ui-origin).
//!
//! The closed [`Kind`] set and its [`Spec`] `data` keys are cpd's side of the adapter protocol; the pi
//! adapter mirrors them in `pi/lib/event.ts` (`pi/test/event.test.ts`, the `ren-red-pi` Job).

use std::collections::HashSet;
use std::str::FromStr;

use cpd_core::error::ErrorCode;
use cpd_core::events::{Kind, PROTOCOL};

/// The six object kinds become `workflow-*` (research §3.5, map R6/R7).
const WORKFLOW_KINDS: [&str; 6] = [
    "workflow-added",
    "workflow-edited",
    "workflow-described",
    "workflow-cancelled",
    "workflow-started",
    "workflow-base-changed",
];

/// The object kinds they replace. The closed set holds no old name: readers read the new kinds only.
const RETIRED_KINDS: [&str; 6] = [
    "pr-added",
    "pr-edited",
    "pr-described",
    "pr-cancelled",
    "pr-started",
    "pr-base-changed",
];

/// The pull-request kinds keep their names (map R6/R7).
const PULL_REQUEST_KINDS: [&str; 3] = ["pr-opened", "pr-merged", "pr-feedback"];

/// The `data` keys that held the Workflow key or the Workflow list; none survives (research §3.5).
const RETIRED_DATA_KEYS: [&str; 5] = ["pr", "prs", "from_pr", "to_pr", "until_pr"];

/// Every `data` key of a kind, required and optional.
fn data_keys(kind: Kind) -> Vec<&'static str> {
    let spec = kind.spec();
    spec.required
        .iter()
        .chain(spec.optional.iter())
        .copied()
        .collect()
}

/// The kind the wire accepts, or a panic naming what it refused.
fn parsed(name: &str) -> Kind {
    Kind::from_str(name).unwrap_or_else(|e| panic!("the wire accepts {name:?}: {e:?}"))
}

#[test]
fn kinds_codes_and_protocol() {
    // Kinds: the closed set holds the `workflow-*` kinds, not the object `pr-*` ones, and keeps the
    // three pull-request kinds.
    for name in WORKFLOW_KINDS {
        assert_eq!(
            parsed(name).as_str(),
            name,
            "the wire accepts and prints {name}"
        );
    }
    for name in RETIRED_KINDS {
        assert!(
            Kind::from_str(name).is_err(),
            "{name} is not a §5.2 kind any more"
        );
    }
    for name in PULL_REQUEST_KINDS {
        assert_eq!(
            parsed(name).as_str(),
            name,
            "the pull-request kind {name} keeps its name"
        );
    }

    // Data keys: no retired key is left anywhere, and the Workflow key is `workflow`/`workflows`.
    for kind in Kind::ALL {
        for key in data_keys(*kind) {
            assert!(
                !RETIRED_DATA_KEYS.contains(&key),
                "{} still carries the retired data key {key:?}",
                kind.as_str()
            );
        }
    }
    for name in WORKFLOW_KINDS {
        assert!(
            data_keys(parsed(name)).contains(&"workflow"),
            "{name} carries the `workflow` data key"
        );
    }
    for name in [
        "pr-opened",
        "pr-merged",
        "pr-feedback",
        "job-added",
        "job-merged",
        "all-merged",
    ] {
        assert!(
            data_keys(parsed(name)).contains(&"workflow"),
            "{name} carries the `workflow` data key"
        );
    }
    for (kind, key) in [
        ("workflow-base-changed", "from_workflow"),
        ("workflow-base-changed", "to_workflow"),
        ("plan-done", "workflows"),
        ("plan-phase", "workflows"),
        ("incident-started", "until_workflow"),
        ("incident-plans", "until_workflow"),
    ] {
        assert!(
            data_keys(parsed(kind)).contains(&key),
            "{kind} carries the `{key}` data key"
        );
    }

    // Error codes (map R13): the object codes are `workflow_*`; the pull-request codes stay.
    let codes: HashSet<&str> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
    for code in [
        "workflow_required",
        "invalid_workflow_id",
        "workflow_exists",
        "workflow_in_use",
        "workflow_empty",
        "workflow_locked",
        "workflow_not_ready",
        "workflow_ended",
        "workflow_title_required",
        "workflow_start_failed",
        "workflow_adopted",
        "workflow_not_open",
        "workflow_not_found",
        "base_workflow_unknown",
        "base_workflow_unavailable",
        "base_workflow_other_repo",
        "workflow_other_plan",
        "explore_workflow_forbidden",
        "needs_other_workflow",
        "until_workflow_not_incident",
    ] {
        assert!(
            codes.contains(code),
            "the object refusal code {code} exists"
        );
    }
    for code in [
        "pr_required",
        "invalid_pr_id",
        "pr_exists",
        "pr_in_use",
        "pr_empty",
        "pr_locked",
        "pr_ended",
        "pr_title_required",
        "pr_start_failed",
        "pr_adopted",
        "pr_not_open",
        "pr_not_found",
        "base_pr_unknown",
        "base_pr_unavailable",
        "base_pr_other_repo",
        "pr_other_plan",
        "explore_pr_forbidden",
        "needs_other_pr",
        "until_pr_not_incident",
    ] {
        assert!(
            !codes.contains(code),
            "the object refusal code {code} is renamed"
        );
    }
    for code in ["pr_closed", "pr_not_closed", "pr_not_ready"] {
        assert!(
            codes.contains(code),
            "the pull-request refusal code {code} stays"
        );
    }

    // The protocol number: 19 before the rename (`core/src/events/kind.rs:38`, `pi/lib/event.ts:9`), 21
    // with the wire (contract §5.2, research §3.5), 23 with the Lead-less `shaping` phase (Plan ctx-explore), 24 with
    // `session-message`'s optional `data.origin` (Plan ui-origin).
    assert_eq!(PROTOCOL, 24);
}
