//! The in-pane section title (commands.md §17, "Section titles"; hs-d298, hs-d308): the one helper every view draws
//! its section titles with. A heavy `━` line, full width with no horizontal margin: the title left, an optional count
//! right, for example `━━ PRs ━━━━━━━━ 3 ━━`. A title joined to a fold block (the Inbox's) starts with `┯`:
//! `┯━━ Inbox ━━━━━━ 32 ━━`.
//!
//! The spacing is the caller's layout, in rows: one empty row above and one below, and none below a joined title.
//! The section's body keeps one space of margin on each side.
//!
//! Pure: it only builds a line. The rules are muted (`palette::fg(Meaning::Nothing)`), the title is bold and the count
//! is not.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::palette::{self, Meaning};

/// The heavy rule.
const RULE: &str = "━";
/// The start of a title.
const LEAD: &str = "━━";
/// The start of a title joined to the block under it: the `┯` sits over the block's `│`.
const JOINED: &str = "┯━━";
/// The rule after the count.
const TAIL: &str = "━━";

/// A title: `━━ <title> ━━━…` with, when `count` is given and fits, ` <count> ━━` at the right edge. A title wider than
/// `width` is cut and the count is left out when it does not fit after at least one `━`; the line never wraps.
pub fn title(title: &str, count: Option<usize>, width: usize) -> Line<'static> {
    line(LEAD, title, count, width)
}

/// The same, joined to the block under it with a `┯`: `┯━━ <title> ━━━…`.
pub fn joined(title: &str, count: Option<usize>, width: usize) -> Line<'static> {
    line(JOINED, title, count, width)
}

fn line(lead: &str, title: &str, count: Option<usize>, width: usize) -> Line<'static> {
    let muted = palette::fg(Meaning::Nothing);
    let lead: String = lead.chars().take(width).collect();
    let left = width - lead.chars().count();
    let block: String = format!(" {title} ").chars().take(left).collect();
    let left = left - block.chars().count();
    let count = count.map(|n| format!(" {n} ")).filter(|c| 1 + c.chars().count() + TAIL.chars().count() <= left);
    let (count, tail) = match &count {
        Some(c) => (c.clone(), TAIL),
        None => (String::new(), ""),
    };
    let fill = left - count.chars().count() - tail.chars().count();
    Line::from(vec![
        Span::styled(lead, muted),
        Span::styled(block, Style::new().add_modifier(Modifier::BOLD)),
        Span::styled(RULE.repeat(fill), muted),
        Span::raw(count),
        Span::styled(tail, muted),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: Line<'static>) -> String {
        line.spans.iter().map(|s| s.content.to_string()).collect()
    }

    #[test]
    fn a_title_without_a_count_fills_to_the_edge() {
        assert_eq!(text(title("PRs", None, 20)), "━━ PRs ━━━━━━━━━━━━━");
        assert_eq!(text(title("PRs", None, 12)), "━━ PRs ━━━━━");
    }

    #[test]
    fn a_title_with_a_count_ends_in_the_count_and_two_rules() {
        assert_eq!(text(title("PRs", Some(3), 20)), "━━ PRs ━━━━━━━━ 3 ━━");
        assert_eq!(text(title("PRs", Some(12), 14)), "━━ PRs ━ 12 ━━");
    }

    #[test]
    fn a_joined_title_starts_with_the_join() {
        assert_eq!(text(joined("Inbox", Some(32), 22)), "┯━━ Inbox ━━━━━━ 32 ━━");
        assert_eq!(text(joined("Inbox", Some(32), 30)), "┯━━ Inbox ━━━━━━━━━━━━━━ 32 ━━");
        assert_eq!(text(joined("Inbox", Some(0), 16)), "┯━━ Inbox ━ 0 ━━");
        assert_eq!(text(joined("Inbox", None, 16)), "┯━━ Inbox ━━━━━━");
    }

    #[test]
    fn a_narrow_width_drops_the_count_then_cuts_the_title() {
        assert_eq!(text(title("PRs", Some(3), 11)), "━━ PRs ━━━━");
        assert_eq!(text(title("Jira", None, 5)), "━━ Ji");
        assert_eq!(text(title("Jira", None, 1)), "━");
        assert_eq!(text(title("Jira", Some(1), 0)), "");
    }

    #[test]
    fn the_rules_are_muted_the_title_is_bold_and_the_count_is_plain() {
        let l = title("PRs", Some(3), 20);
        assert_eq!(l.spans[0].style.fg, palette::fg(Meaning::Nothing).fg);
        assert!(l.spans[1].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(l.spans[3].style, Style::new());
        assert_eq!(l.spans[4].style.fg, palette::fg(Meaning::Nothing).fg);
    }
}
