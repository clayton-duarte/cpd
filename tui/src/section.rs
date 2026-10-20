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

    // ---- section look v3 (commands.md §17, "Section titles"; hs-d349): the pill title, red until `title` draws it ----

    /// U+E0B6: the pill's left cap.
    const CAP_LEFT: &str = "\u{E0B6}";
    /// U+E0B4: the pill's right cap.
    const CAP_RIGHT: &str = "\u{E0B4}";

    fn cells(line: &Line<'static>) -> usize {
        text(line.clone()).chars().count()
    }

    fn muted(span: &Span<'_>) -> bool {
        span.style.fg == palette::fg(Meaning::Nothing).fg
    }

    #[test]
    fn the_pill_title_is_a_corner_a_line_the_capped_text_a_line_to_the_edge_and_the_count() {
        let got = text(title("Inbox", Some(35), 20));
        assert_eq!(got, format!("╭─{CAP_LEFT}Inbox{CAP_RIGHT}{}35", "─".repeat(9)));
        assert_eq!(got.chars().count(), 20, "full width, no margin");
        assert_eq!(text(title("PRs", Some(3), 24)), format!("╭─{CAP_LEFT}PRs{CAP_RIGHT}{}3", "─".repeat(24 - 2 - 5 - 1)));
        assert_eq!(text(title("Inbox", Some(0), 14)), format!("╭─{CAP_LEFT}Inbox{CAP_RIGHT}─0"));
    }

    #[test]
    fn the_pill_title_without_a_count_fills_to_the_right_edge() {
        let got = text(title("git", None, 18));
        assert_eq!(got, format!("╭─{CAP_LEFT}git{CAP_RIGHT}{}", "─".repeat(18 - 2 - 5)));
        assert_eq!(got.chars().count(), 18);
        assert_eq!(text(title("Jira", None, 12)), format!("╭─{CAP_LEFT}Jira{CAP_RIGHT}{}", "─".repeat(12 - 2 - 6)));
    }

    #[test]
    fn the_pills_text_is_inverted_its_lines_are_muted_and_the_pill_is_not() {
        let l = title("Inbox", Some(35), 20);
        let find = |what: &str| l.spans.iter().find(|s| s.content.contains(what)).unwrap_or_else(|| panic!("no span with {what:?}: {:?}", l.spans));
        let pill = find("Inbox");
        assert!(pill.style.add_modifier.contains(Modifier::REVERSED), "the title text is reverse video: {pill:?}");
        assert!(!muted(pill), "{pill:?}");
        for cap in [CAP_LEFT, CAP_RIGHT] {
            assert!(!muted(find(cap)), "the cap {cap:?} is part of the pill, not muted");
        }
        let lines: Vec<_> = l.spans.iter().filter(|s| !s.content.is_empty() && s.content.chars().all(|c| c == '╭' || c == '─')).collect();
        assert!(lines.iter().any(|s| s.content.contains('╭')) && lines.iter().any(|s| s.content.contains('─')), "the corner and the lines: {:?}", l.spans);
        assert!(lines.iter().all(|s| muted(s)), "the corner and every line are muted: {lines:?}");
        assert_eq!(cells(&l), 20);
    }
}
