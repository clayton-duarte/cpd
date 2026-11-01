//! The section titles as the User reads them back from a pane (hs-d475, hs-d478, hs-d480): one place for every area's tests.
//! They mirror `tui/src/section.rs` (`title` and the Inbox's header), whose own pieces are private to `cpd-tui`; the rows read
//! back are trimmed, so the blank last column is not in them.

/// The pill's caps, U+E0B6 and U+E0B4.
pub const CAP_LEFT: &str = "\u{E0B6}";
pub const CAP_RIGHT: &str = "\u{E0B4}";

/// The Inbox's header on a `width`-column pane: `───`, the title as a pill, `─` up to the count's pill and one `─`.
pub fn flush_title(width: usize, title: &str, count: usize) -> String {
    let count = format!("{CAP_LEFT}{count}{CAP_RIGHT}");
    let fill = "─".repeat(width - 3 - (title.chars().count() + 2) - count.chars().count() - 1);
    format!("───{CAP_LEFT}{title}{CAP_RIGHT}{fill}{count}─")
}
