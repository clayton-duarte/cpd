//! A Dialog Inbox item of a review Plan carries the pull request's `url` (pro-d412, A4): its menu offers Open Link and
//! Copy Link, and the primary click stays Go to Lead.

use super::inbox::{Entry, Jump, click, item};
use crate::menu::Entry as _;
use crate::tree::actions::Action;
use crate::tree::fixture::sample;
use cpd_core::ids::SessionId;
use cpd_core::inbox::model::{Item, Kind, Urgency};

const OPENED: &str = "2026-01-01T00:00:00.000Z";
const URL: &str = "https://github.com/acme/api-server/pull/912";

fn dialog(url: Option<&str>) -> Item {
    let mut it = item(1, Kind::Dialog, Urgency::Red, "review asks", OPENED);
    it.session = Some(SessionId::parse("m-alpha").unwrap());
    it.url = url.map(str::to_owned);
    it
}

fn labels(item: &Item) -> Vec<&'static str> {
    Entry::entries(item, false)
        .into_iter()
        .map(|e| e.label())
        .collect()
}

#[test]
fn a_dialog_item_with_a_url_offers_open_link_and_copy_link() {
    let got = labels(&dialog(Some(URL)));
    for want in ["Open Link", "Copy Link"] {
        assert!(got.contains(&want), "{want} is in the menu: {got:?}");
    }
}

#[test]
fn a_dialog_item_primary_click_still_goes_to_the_lead() {
    let it = dialog(Some(URL));
    assert_eq!(
        click(&sample(), &it),
        Some(Jump::Run(Action::Open(
            SessionId::parse("m-alpha").unwrap()
        ))),
        "the click opens the Lead"
    );
    // and the links are added to the menu, they do not take the click
    assert!(labels(&it).contains(&"Open Link"), "{:?}", labels(&it));
}

#[test]
fn a_dialog_item_without_a_url_has_no_link_actions() {
    let got = labels(&dialog(None));
    assert!(
        !got.contains(&"Open Link") && !got.contains(&"Copy Link"),
        "{got:?}"
    );
    // the same item with a url differs by exactly the two link entries
    let with: Vec<_> = labels(&dialog(Some(URL)))
        .into_iter()
        .filter(|l| !["Open Link", "Copy Link"].contains(l))
        .collect();
    assert_eq!(
        with, got,
        "a url adds Open Link and Copy Link, nothing else"
    );
    assert_eq!(
        labels(&dialog(Some(URL))).len(),
        got.len() + 2,
        "a url adds the two link entries"
    );
}
