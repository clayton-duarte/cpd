//! The optimistic hide of a picked Post or Merge: the pure filter and the failure path of the `posting` set.

use super::inbox::{item, shown};
use super::{Posting, posted};
use cpd_core::inbox::model::{Item, Kind, Urgency};

const OPENED: &str = "2026-01-02T23:55:00.000Z";

fn pr(id: i64, urgency: Urgency, pull: &str) -> Item {
    Item {
        subject: format!("pr:{pull}"),
        ..item(id, Kind::PrReady, urgency, &format!("{pull} label"), OPENED)
    }
}

#[test]
fn shown_drops_the_row_of_a_pull_request_being_posted() {
    let posted_row = pr(1, Urgency::Yellow, "acme/shop#4");
    let other = pr(2, Urgency::Yellow, "acme/shop#5");
    let cyan = pr(3, Urgency::Cyan, "acme/api#9");
    let dialog = item(4, Kind::Dialog, Urgency::Red, "blocked", OPENED);
    let items = vec![posted_row, other.clone(), cyan.clone(), dialog.clone()];
    let posting: Posting = ["acme/shop#4".to_owned()].into();
    assert_eq!(shown(&items, &posting), vec![other, cyan, dialog]);
}

#[test]
fn a_failed_post_takes_its_row_out_of_posting_so_it_comes_back() {
    let mut posting: Posting = ["acme/shop#4".to_owned(), "acme/shop#5".to_owned()].into();
    posted(&mut posting, "acme/shop#4", true);
    assert_eq!(posting, Posting::from(["acme/shop#5".to_owned()]));
}
