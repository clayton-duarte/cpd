//! Red tests (bug-guard-tests-a1) for the `bug-breach` Event kind (contract §5.2; air-d105,
//! air-d106; Plan `protocol-enforce`, Workflow `bug-guard`): engine step E25 appends it when an
//! open bug has no mitigation past the 15-minute bound, or when a new open bug repeats an open
//! bug's normalized symptom. The adapter ignores the kind, so `PROTOCOL` does not change.
//!
//! Red until `bug-guard-impl` registers the kind (the variant is `Kind::BugProtocolBreach`).

use super::{Kind, Set};

/// §5.2's `bug-breach` row: `context?` and `plan?`, no `job`/`attempt`/`session`, and `data`
/// `{bug, reason, symptom, bound}`. Red until the kind is registered.
#[test]
fn bug_protocol_breach_kind_is_registered() {
    let kind: Kind = "bug-breach"
        .parse()
        .expect("`bug-breach` is an Event kind of contract §5.2");
    assert_eq!(kind.as_str(), "bug-breach");
    assert!(Kind::ALL.contains(&kind), "the kind is one of Kind::ALL");
    let spec = kind.spec();
    assert_eq!(
        (
            spec.context,
            spec.plan,
            spec.job,
            spec.attempt,
            spec.session
        ),
        (Set::Maybe, Set::Maybe, Set::No, Set::No, Set::No)
    );
    assert_eq!(spec.required, ["bug", "reason", "symptom", "bound"]);
    assert!(spec.optional.is_empty(), "{:?}", spec.optional);
}
