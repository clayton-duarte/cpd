//! The Inbox counter's tier (contract §6.1.1, hs-d534): the text `@cpd_inbox_tier` holds, which the status line turns into
//! the pill's colour.

use super::Inbox;
use cpd_core::inbox::model::Urgency::{self, Magenta, Red, White, Yellow};

/// The counter over open items of these urgencies, as `Inbox::read` builds it: the count and the most urgent.
fn inbox(items: &[Urgency]) -> Inbox {
    Inbox { count: items.len(), urgency: items.iter().copied().max() }
}

/// Red over yellow over magenta over white; `none` at zero.
#[test]
fn the_inbox_tier_names_the_most_urgent_item() {
    assert_eq!(inbox(&[]).tier(), "none");
    assert_eq!(inbox(&[White]).tier(), "white");
    assert_eq!(inbox(&[White, Magenta]).tier(), "magenta");
    assert_eq!(inbox(&[Magenta, White, Yellow, White]).tier(), "yellow");
    assert_eq!(inbox(&[White, Yellow, Red, Magenta]).tier(), "red");
    assert_eq!(inbox(&[Red]).tier(), "red");
    assert_eq!(inbox(&[Yellow, Magenta]).tier(), "yellow");
}
