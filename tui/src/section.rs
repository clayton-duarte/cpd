//! The in-pane section title (commands.md §17, "Section titles"; hs-d298, hs-d308, hs-d349): the one helper every view
//! draws its section titles with, as a **pill** on a line, full width with no horizontal margin: `╭─`, the title as a
//! pill (the text inverted, with powerline semicircle caps `` and ``), a muted `─` line to the right edge, ending with
//! the optional count: `╭─Inbox────…────35`.
//!
//! The spacing is the caller's layout, in rows: one empty row above, none below. The section's body keeps one space of
//! margin on each side.
//!
//! Pure: it only builds a line. The corner and the lines are muted (`palette::fg(Meaning::Nothing)`), the pill is not.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::palette::{self, Meaning};

/// The start of the row: the corner and one line.
const LEAD: &str = "╭─";
/// The line to the right edge.
const RULE: &str = "─";
/// U+E0B6: the pill's left cap.
const CAP_LEFT: &str = "\u{E0B6}";
/// U+E0B4: the pill's right cap.
const CAP_RIGHT: &str = "\u{E0B4}";

/// A title: `╭─<pill>───…` with, when `count` is given and fits after at least one `─`, the count at the right edge. A
/// title wider than `width` is cut (a pill narrower than 3 cells is left out); the line never wraps.
pub fn title(title: &str, count: Option<usize>, width: usize) -> Line<'static> {
    let muted = palette::fg(Meaning::Nothing);
    let lead: String = LEAD.chars().take(width).collect();
    let left = width - lead.chars().count();
    let text: String = title.chars().take(left.saturating_sub(2)).collect();
    let pill = left >= 3 && !text.is_empty();
    let left = if pill { left - text.chars().count() - 2 } else { left };
    let count = count.map(|n| n.to_string()).filter(|c| pill && 1 + c.chars().count() <= left).unwrap_or_default();
    let fill = left - count.chars().count();
    let mut spans = vec![Span::styled(lead, muted)];
    if pill {
        spans.push(Span::raw(CAP_LEFT));
        spans.push(Span::styled(text, Style::new().add_modifier(Modifier::REVERSED)));
        spans.push(Span::raw(CAP_RIGHT));
    }
    spans.push(Span::styled(RULE.repeat(fill), muted));
    spans.push(Span::raw(count));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: Line<'static>) -> String {
        line.spans.iter().map(|s| s.content.to_string()).collect()
    }

    // ---- section look v3 (commands.md §17, "Section titles"; hs-d349): the pill title, red until `title` draws it ----

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
        assert_eq!(text(title("Inbox", Some(0), 14)), format!("╭─{CAP_LEFT}Inbox{CAP_RIGHT}{}0", "─".repeat(4)));
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

    #[test]
    fn a_narrow_width_drops_the_count_then_cuts_the_title_then_the_pill() {
        assert_eq!(text(title("PRs", Some(3), 7)), format!("╭─{CAP_LEFT}PRs{CAP_RIGHT}"), "no room for a line and the count");
        assert_eq!(text(title("Jira", None, 6)), format!("╭─{CAP_LEFT}Ji{CAP_RIGHT}"));
        assert_eq!(text(title("Jira", None, 3)), "╭──");
        assert_eq!(text(title("Jira", None, 1)), "╭");
        assert_eq!(text(title("Jira", Some(1), 0)), "");
    }
}
