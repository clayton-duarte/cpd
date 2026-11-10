//! Tests (bug-guard-tests-a1, amended by breach-log-impl-a1; contract §5.2; air-d105, air-d106,
//! D14, air-d119; Plan `protocol-enforce`): engine step E25 appends a `protocol-breach` Event
//! (`protocol` `bug report`, `source` `E25`) when an open bug has no mitigation past the 15-minute
//! bound, or when a new open bug repeats an open bug's normalized symptom. The adapter ignores the
//! kind; `PROTOCOL` rises for the guards' Ledger leg, not for the kind.

use super::{Kind, Set};

/// §5.2's `protocol-breach` row: `context?`, `plan?` and `session?`, no `job`/`attempt`, `data`
/// `{protocol, source}` plus the guard's or E25's own fields.
#[test]
fn bug_protocol_breach_kind_is_registered() {
    let kind: Kind = "protocol-breach"
        .parse()
        .expect("`protocol-breach` is an Event kind of contract §5.2");
    assert_eq!(kind.as_str(), "protocol-breach");
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
        (Set::Maybe, Set::Maybe, Set::No, Set::No, Set::Maybe)
    );
    assert_eq!(spec.required, ["protocol", "source"]);
    assert_eq!(
        spec.optional,
        [
            "role", "model", "tier", "tool", "command", "bug", "reason", "symptom", "bound"
        ]
    );
}
