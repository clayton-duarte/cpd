//! Which `pr-ready` items fill the Plan Lead's dot: only Yellow and Red.

use super::dot::urgency;
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::inbox::model::{Item, Kind, Urgency};

fn item(kind: Kind, urgency: Urgency, session: Option<&str>, plan: Option<&str>) -> Item {
    Item {
        id: 1,
        kind,
        urgency,
        subject: "s".into(),
        label: "l".into(),
        url: None,
        context: None,
        plan: plan.map(|p| PlanId::parse(p).unwrap()),
        job: None,
        session: session.map(|s| SessionId::parse(s).unwrap()),
        record: None,
        event: None,
        opened_at: "2026-01-01T00:00:00.000Z".into(),
        closed_at: None,
        snoozed_until: None,
        dismissed_at: None,
        claimed_at: None,
    }
}

fn id(s: &str) -> SessionId {
    SessionId::parse(s).unwrap()
}

fn plan(s: &str) -> PlanId {
    PlanId::parse(s).unwrap()
}

#[test]
fn a_pr_ready_item_fills_its_leads_dot_only_when_it_is_yellow_or_red() {
    let pp = plan("pp");
    let other = plan("qq");
    let cases = [
        (Urgency::Yellow, Some(Urgency::Yellow)),
        (Urgency::Red, Some(Urgency::Red)),
        (Urgency::Cyan, None),
        (Urgency::White, None),
    ];
    for (u, lead) in cases {
        let items = [item(Kind::PrReady, u, None, Some("pp"))];
        assert_eq!(urgency(&items, &id("s-lead"), Some(&pp)), lead, "lead of pp, {u:?}");
        assert_eq!(urgency(&items, &id("s-lead"), None), None, "fork, {u:?}");
        assert_eq!(urgency(&items, &id("s-lead"), Some(&other)), None, "lead of another Plan, {u:?}");
    }
}

#[test]
fn a_cyan_or_white_pr_ready_item_leaves_a_lower_item_the_dot() {
    let pp = plan("pp");
    let s = id("s-lead");
    let workflow = |u| item(Kind::PrReady, u, None, Some("pp"));
    let unread = item(Kind::Unread, Urgency::White, Some("s-lead"), None);
    let cyan_dialog = item(Kind::Dialog, Urgency::Cyan, Some("s-lead"), None);
    assert_eq!(urgency(&[workflow(Urgency::Cyan), unread.clone()], &s, Some(&pp)), Some(Urgency::White));
    assert_eq!(urgency(&[workflow(Urgency::White), cyan_dialog], &s, Some(&pp)), Some(Urgency::Cyan));
    assert_eq!(urgency(&[workflow(Urgency::Cyan)], &s, Some(&pp)), None);
    assert_eq!(urgency(&[workflow(Urgency::Yellow), unread], &s, Some(&pp)), Some(Urgency::Yellow));
}

#[test]
fn an_empty_list_and_the_other_plan_kinds_keep_their_dot() {
    let pp = plan("pp");
    let s = id("s-lead");
    assert_eq!(urgency(&[], &s, Some(&pp)), None);
    let review = [item(Kind::PlanReview, Urgency::Magenta, None, Some("pp"))];
    assert_eq!(urgency(&review, &s, Some(&pp)), Some(Urgency::Magenta));
    let stuck = [item(Kind::JobStuck, Urgency::Red, None, Some("pp"))];
    assert_eq!(urgency(&stuck, &s, Some(&pp)), Some(Urgency::Red));
}
