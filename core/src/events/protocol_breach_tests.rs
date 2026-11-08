//! Red tests (breach-log-tests-a1) for the `protocol-breach` Event kind (contract §5.2; D14,
//! air-d119; Plan `protocol-enforce`): one kind for every protocol's breach, `data.protocol`
//! naming it, replacing the bug-only `bug-breach` kind the Workflow `bug-guard` first named.
//! The engine step E25 appends it for the bug report protocol and the pi guards' Ledger leg
//! reports it through `cpd session report` (commands.md §10.18); the adapter ignores the kind, so
//! `PROTOCOL` rises to 25 for the guards' `session report` flags, not for the kind.
//!
//! Red until `breach-log-impl` registers the kind and its `protocol` field (the variant is
//! `Kind::ProtocolBreach`).

use super::{Kind, Set};

/// §5.2's `protocol-breach` row: `context?`, `plan?` and (a guard's Event) `session?`, no
/// `job`/`attempt`, and `data.protocol` naming the breached protocol. Red until the kind is
/// registered with its new fields.
#[test]
fn the_protocol_field_is_recorded() {
    let kind: Kind = "protocol-breach"
        .parse()
        .expect("`protocol-breach` is an Event kind of contract §5.2");
    assert_eq!(kind.as_str(), "protocol-breach");
    assert!(Kind::ALL.contains(&kind), "the kind is one of Kind::ALL");
    let spec = kind.spec();
    assert!(
        spec.required.contains(&"protocol"),
        "the kind records `data.protocol`, the breached protocol's name (contract §5.2): {:?}",
        spec.required
    );
    assert_eq!(
        (
            spec.context,
            spec.plan,
            spec.job,
            spec.attempt,
            spec.session
        ),
        (Set::Maybe, Set::Maybe, Set::No, Set::No, Set::Maybe),
        "a guard's Event names the refused Session; E25's Event names no Session"
    );
}
