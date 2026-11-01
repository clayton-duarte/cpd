//! The blue tier (pro-d58): `Urgency::Blue`, its place in the order and E8's `review-requested` condition.

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::macros::datetime;

use super::model::{Condition, Filter, Item, Kind, State, Urgency, list, reconcile};
use super::rules::{Team, conditions_with};
use super::team::{GhPr, GhState, Lookup};
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::store::Store;

const NOW: OffsetDateTime = datetime!(2026-01-02 00:00 UTC);

fn store() -> (tempfile::TempDir, CpdHome, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (dir, home, store)
}

fn condition(kind: Kind, urgency: Urgency, subject: &str) -> Condition {
    Condition {
        kind,
        urgency,
        subject: subject.to_owned(),
        label: format!("{subject} label"),
        url: None,
        context: None,
        plan: None,
        job: None,
        session: None,
        record: None,
        event: None,
        opened_at: None,
    }
}

fn visible(store: &mut Store) -> Vec<Item> {
    store
        .read(|c| {
            list(
                c,
                &Filter {
                    context: None,
                    state: State::Visible {
                        now: "2026-01-01T00:00:00.000Z".into(),
                    },
                },
            )
        })
        .unwrap()
}

/// Opens `conditions` one by one at `at`, keeping the ones already open.
fn open_one(store: &mut Store, new: Condition, at: &str) {
    let mut open: Vec<Condition> = store
        .read(|c| {
            list(
                c,
                &Filter {
                    context: None,
                    state: State::Open,
                },
            )
        })
        .unwrap()
        .iter()
        .map(|i| condition(i.kind, i.urgency, &i.subject))
        .collect();
    open.push(new);
    reconcile(store, &open, at).unwrap();
}

#[test]
fn urgency_orders_white_cyan_blue_magenta_yellow_red() {
    let order = [
        Urgency::White,
        Urgency::Cyan,
        Urgency::Blue,
        Urgency::Magenta,
        Urgency::Yellow,
        Urgency::Red,
    ];
    for pair in order.windows(2) {
        assert!(pair[0] < pair[1], "{:?} < {:?}", pair[0], pair[1]);
    }
    let mut sorted = order;
    sorted.sort();
    assert_eq!(sorted, order);
    // the last variant of the enum, as the stub is written, is not the highest: the derived order is the declaration order
    assert!(Urgency::Cyan < Urgency::Blue && Urgency::Blue < Urgency::Magenta);
}

#[test]
fn urgency_blue_parses_and_prints_as_blue() {
    assert_eq!(Urgency::parse("blue"), Some(Urgency::Blue));
    assert_eq!(Urgency::Blue.as_str(), "blue");
    for name in ["white", "cyan", "blue", "magenta", "yellow", "red"] {
        assert_eq!(Urgency::parse(name).map(Urgency::as_str), Some(name));
    }
}

#[test]
fn the_list_ranks_blue_between_magenta_and_cyan() {
    let (_dir, _home, mut store) = store();
    for (kind, urgency, subject, at) in [
        (Kind::Unread, Urgency::White, "w", "t1"),
        (Kind::CurateDocs, Urgency::Cyan, "c", "t2"),
        (Kind::ReviewRequested, Urgency::Blue, "pr:acme/pay#1", "t3"),
        (Kind::PlanReview, Urgency::Magenta, "m", "t4"),
        (Kind::PrReady, Urgency::Yellow, "y", "t5"),
        (Kind::Dialog, Urgency::Red, "r", "t6"),
    ] {
        open_one(&mut store, condition(kind, urgency, subject), at);
    }
    let order: Vec<Urgency> = visible(&mut store).into_iter().map(|i| i.urgency).collect();
    assert_eq!(
        order,
        [
            Urgency::Red,
            Urgency::Yellow,
            Urgency::Magenta,
            Urgency::Blue,
            Urgency::Cyan,
            Urgency::White
        ]
    );

    // within the tier, the oldest `opened_at` first
    open_one(
        &mut store,
        condition(Kind::ReviewRequested, Urgency::Blue, "pr:acme/pay#0"),
        "t0",
    );
    let blue: Vec<String> = visible(&mut store)
        .into_iter()
        .filter(|i| i.urgency == Urgency::Blue)
        .map(|i| i.subject)
        .collect();
    assert_eq!(blue, ["pr:acme/pay#0", "pr:acme/pay#1"]);
}

#[test]
fn a_post_that_needs_review_opens_a_blue_item_with_the_same_subject_label_and_url() {
    let (_dir, home, mut store) = store();
    let ts = NOW.unix_timestamp() as f64 - 3600.0;
    let post: Value = json!({"repo": "acme/pay", "number": 812, "ts": ts, "ts_raw": "1.1", "author": "Ryan Case", "primary": true,
                             "reactions": [], "claim": null, "eyes_me": null, "thread_tags_me": false});
    let path = home.run_dir().join("team-review.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let cache = json!({"version": 1, "polled_at": "2026-01-01T00:00:00.000Z", "channel": "C02JU6C2Z1D", "read": 10, "posts": [post]});
    std::fs::write(path, cache.to_string()).unwrap();

    let pr = GhPr {
        title: "Add retries".to_owned(),
        draft: false,
        state: GhState::Open,
        author: "ryan".to_owned(),
        decision: None,
        reviews: Vec::new(),
    };
    let lookup = Lookup {
        me: "clayton-duarte".to_owned(),
        prs: [(("acme/pay".to_owned(), 812), Some(pr))]
            .into_iter()
            .collect(),
    };
    let bots = ["github-actions".to_owned()];
    let mut github = |_: &[(String, u64)]| -> Result<Lookup, CpdError> { Ok(lookup.clone()) };
    let team = Team {
        bot_reviewers: &bots,
        github: &mut github,
    };
    let all = conditions_with(&mut store, &home, NOW, 20, 1, Some(team)).unwrap();
    let reviews: Vec<Condition> = all
        .into_iter()
        .filter(|c| c.kind == Kind::ReviewRequested)
        .collect();
    assert_eq!(reviews.len(), 1, "{reviews:?}");
    let c = &reviews[0];
    assert_eq!(
        (c.urgency, c.subject.as_str(), c.label.as_str()),
        (
            Urgency::Blue,
            "pr:acme/pay#812",
            "review pay#812\nAdd retries"
        )
    );
    assert_eq!(
        c.url.as_deref(),
        Some("https://github.com/acme/pay/pull/812")
    );
}
