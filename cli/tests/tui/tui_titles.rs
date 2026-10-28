//! The section titles as the User reads them back from a pane (hs-d475, hs-d478, hs-d480): one place for every area's tests.
//! They mirror `tui/src/section.rs` (`title` and the Inbox's header), whose own pieces are private to `cpd-tui`; the rows read
//! back are trimmed, so the blank last column is not in them.

/// The pill's caps, U+E0B6 and U+E0B4.
pub const CAP_LEFT: &str = "\u{E0B6}";
pub const CAP_RIGHT: &str = "\u{E0B4}";

/// A title's row on a `width`-column pane: a blank column, `lead` (two cells), the title as a pill, `─` up to the count's pill
/// (when given) and the blank last column.
fn row(lead: &str, width: usize, title: &str, count: Option<usize>) -> String {
    let count = count.map(|n| n.to_string());
    let used = 1 + 2 + 2 + title.chars().count() + 1 + count.as_ref().map_or(0, |c| 2 + c.chars().count());
    let fill = "─".repeat(width - used);
    let count = count.map(|c| format!("{CAP_LEFT}{c}{CAP_RIGHT}")).unwrap_or_default();
    format!(" {lead}{CAP_LEFT}{title}{CAP_RIGHT}{fill}{count}")
}

/// The non-collapsible title (` ──` + pill) on a `width`-column pane (hs-d478).
pub fn title(width: usize, title: &str, count: Option<usize>) -> String {
    row("──", width, title, count)
}

/// The Inbox's header on a `width`-column pane: `───`, the title as a pill, `─` up to the count's pill and one `─`.
pub fn flush_title(width: usize, title: &str, count: usize) -> String {
    let count = format!("{CAP_LEFT}{count}{CAP_RIGHT}");
    let fill = "─".repeat(width - 3 - (title.chars().count() + 2) - count.chars().count() - 1);
    format!("───{CAP_LEFT}{title}{CAP_RIGHT}{fill}{count}─")
}
