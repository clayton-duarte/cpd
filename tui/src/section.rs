//! The in-pane section title (commands.md §17, "Section titles"; hs-d298, hs-d308, hs-d475, hs-d476): the one helper every
//! view draws its section titles with, as a **grey pill** on a line, inside the panel's padding of one column on each
//! side: a blank cell, `╭─`, the title as a pill (its text inverted, between the powerline semicircle caps `` and
//! ``), a muted `─` line up to the optional count and a blank cell: ` ╭─Inbox────…────35 `. The title is as the
//! caller gives it, in Title Case, with no padding spaces inside the pill. It is never clickable (the helper draws a line,
//! it is no hit target).
//!
//! The spacing is the caller's layout, in rows: one empty row above and one below a title outside a collapsible widget
//! (`crate::fold` has its own margins); the body has no vertical spacing.
//!
//! Pure: it only builds a line. The pill is grey: the corner, the lines and the caps are muted
//! (`palette::fg(Meaning::Nothing)`), the pill's text is the muted block with the background colour on it (reverse video).

use ratatui::style::Modifier;
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
/// The padding: one blank column on each side.
const PAD: &str = " ";

/// A title `width` cells wide, its first and last cell the padding: ` ╭─<pill>───…` with, when `count` is given and
/// fits after at least one `─`, the count against the padding on the right. A title wider than the room is cut (a pill
/// narrower than 3 cells is left out); the line never wraps.
pub fn title(title: &str, count: Option<usize>, width: usize) -> Line<'static> {
    let muted = palette::fg(Meaning::Nothing);
    let pad = usize::from(width >= 2);
    let width = width - 2 * pad;
    let lead: String = LEAD.chars().take(width).collect();
    let left = width - lead.chars().count();
    let text: String = title.chars().take(left.saturating_sub(2)).collect();
    let pill = left >= 3 && !text.is_empty();
    let left = if pill { left - text.chars().count() - 2 } else { left };
    let count = count.map(|n| n.to_string()).filter(|c| pill && 1 + c.chars().count() <= left).unwrap_or_default();
    let fill = left - count.chars().count();
    let mut spans = vec![Span::raw(PAD.repeat(pad)), Span::styled(lead, muted)];
    if pill {
        spans.push(Span::styled(CAP_LEFT, muted));
        spans.push(Span::styled(text, muted.add_modifier(Modifier::REVERSED)));
        spans.push(Span::styled(CAP_RIGHT, muted));
    }
    spans.push(Span::styled(RULE.repeat(fill), muted));
    spans.push(Span::raw(count));
    spans.push(Span::raw(PAD.repeat(pad)));
    Line::from(spans)
}

/// What the tests of every view check of a section title's pill (hs-d475): one place, not a copy per view.
#[cfg(test)]
pub(crate) mod check {
    use ratatui::buffer::Buffer;
    use ratatui::style::{Color, Modifier};

    use super::{CAP_LEFT, CAP_RIGHT};
    use crate::palette::{self, Meaning};

    /// The pill whose left cap is at (`x`, `y`) of `buf` reads `text` between its two caps, with no padding spaces, and is grey:
    /// the caps are muted; the text is the block inverted (reverse video of the muted colour: the block is muted and the text is the
    /// background colour).
    pub(crate) fn grey_pill(buf: &Buffer, x: u16, y: u16, text: &str) {
        let grey = palette::color(Meaning::Nothing);
        let width = u16::try_from(text.chars().count()).unwrap();
        let drawn: String = (x..x + width + 2).map(|x| buf[(x, y)].symbol()).collect();
        assert_eq!(drawn, format!("{CAP_LEFT}{text}{CAP_RIGHT}"), "the pill at ({x}, {y})");
        for cap in [x, x + width + 1] {
            let cell = &buf[(cap, y)];
            assert_eq!((cell.fg, cell.bg), (grey, Color::Reset), "the cap at ({cap}, {y}) is muted: {cell:?}");
            assert!(!cell.modifier.contains(Modifier::REVERSED), "a cap is not inverted: {cell:?}");
        }
        for at in x + 1..=x + width {
            let cell = &buf[(at, y)];
            assert!(cell.modifier.contains(Modifier::REVERSED), "the text at ({at}, {y}) is inverted: {cell:?}");
            assert_eq!((cell.fg, cell.bg), (grey, Color::Reset), "the block is muted, the text is the background colour: {cell:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: Line<'static>) -> String {
        line.spans.iter().map(|s| s.content.to_string()).collect()
    }

    // ---- the pill title's layout: a corner, a line, the capped text, a line to the edge and the count, inside the padding ----

    fn cells(line: &Line<'static>) -> usize {
        text(line.clone()).chars().count()
    }

    fn muted(span: &Span<'_>) -> bool {
        span.style.fg == palette::fg(Meaning::Nothing).fg
    }

    #[test]
    fn the_pill_title_is_a_corner_a_line_the_capped_text_a_line_to_the_edge_and_the_count() {
        let got = text(title("Inbox", Some(35), 22));
        assert_eq!(got, format!(" ╭─{CAP_LEFT}Inbox{CAP_RIGHT}{}35 ", "─".repeat(9)));
        assert_eq!(got.chars().count(), 22, "full width, the padding included");
        assert_eq!(text(title("PRs", Some(3), 26)), format!(" ╭─{CAP_LEFT}PRs{CAP_RIGHT}{}3 ", "─".repeat(26 - 2 - 2 - 5 - 1)));
        assert_eq!(text(title("Inbox", Some(0), 16)), format!(" ╭─{CAP_LEFT}Inbox{CAP_RIGHT}{}0 ", "─".repeat(4)));
    }

    #[test]
    fn the_pill_title_without_a_count_fills_to_the_right_padding() {
        let got = text(title("Git", None, 20));
        assert_eq!(got, format!(" ╭─{CAP_LEFT}Git{CAP_RIGHT}{} ", "─".repeat(20 - 2 - 2 - 5)));
        assert_eq!(got.chars().count(), 20);
        assert_eq!(text(title("Jira", None, 14)), format!(" ╭─{CAP_LEFT}Jira{CAP_RIGHT}{} ", "─".repeat(14 - 2 - 2 - 6)));
    }

    #[test]
    fn the_pills_text_the_caps_and_the_lines_are_all_muted_and_only_the_text_is_inverted() {
        let l = title("Inbox", Some(35), 22);
        let find = |what: &str| l.spans.iter().find(|s| s.content.contains(what)).unwrap_or_else(|| panic!("no span with {what:?}: {:?}", l.spans));
        let pill = find("Inbox");
        assert!(pill.style.add_modifier.contains(Modifier::REVERSED), "the title text is reverse video: {pill:?}");
        assert!(muted(pill), "{pill:?}");
        for cap in [CAP_LEFT, CAP_RIGHT] {
            let cap = find(cap);
            assert!(muted(cap) && !cap.style.add_modifier.contains(Modifier::REVERSED), "a cap is muted, not inverted: {cap:?}");
        }
        let lines: Vec<_> = l.spans.iter().filter(|s| !s.content.is_empty() && s.content.chars().all(|c| c == '╭' || c == '─')).collect();
        assert!(lines.iter().any(|s| s.content.contains('╭')) && lines.iter().any(|s| s.content.contains('─')), "the corner and the lines: {:?}", l.spans);
        assert!(lines.iter().all(|s| muted(s)), "the corner and every line are muted: {lines:?}");
        assert_eq!(cells(&l), 22);
    }

    #[test]
    fn a_narrow_width_drops_the_count_then_cuts_the_title_then_the_pill() {
        assert_eq!(text(title("PRs", Some(3), 9)), format!(" ╭─{CAP_LEFT}PRs{CAP_RIGHT} "), "no room for a line and the count");
        assert_eq!(text(title("Jira", None, 8)), format!(" ╭─{CAP_LEFT}Ji{CAP_RIGHT} "));
        assert_eq!(text(title("Jira", None, 5)), " ╭── ");
        assert_eq!(text(title("Jira", None, 3)), " ╭ ");
        assert_eq!(text(title("Jira", None, 2)), "  ");
        assert_eq!(text(title("Jira", None, 1)), "╭");
        assert_eq!(text(title("Jira", Some(1), 0)), "");
    }

    // ---- section look v4 (commands.md §17, "Section titles"; hs-d475, hs-d476): red until `title` draws it ----

    /// `line` drawn alone in a row `width` cells wide.
    fn buffer_of(line: Line<'static>, width: u16) -> ratatui::buffer::Buffer {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 1)).unwrap();
        terminal.draw(|f| f.render_widget(ratatui::widgets::Paragraph::new(line), f.area())).unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn the_pill_is_grey_its_block_muted_its_text_the_background_colour_and_its_caps_muted() {
        let buf = buffer_of(title("Inbox", Some(35), 22), 22);
        let row: String = (0..22).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(row.starts_with(" ╭─"), "{row:?}");
        check::grey_pill(&buf, 3, 0, "Inbox");
    }

    #[test]
    fn a_title_row_starts_at_column_1_and_ends_one_column_before_the_edge() {
        let got = text(title("Inbox", Some(35), 22));
        assert_eq!(got, format!(" ╭─{CAP_LEFT}Inbox{CAP_RIGHT}{}35 ", "─".repeat(9)));
        assert_eq!(got.chars().count(), 22, "the row is as wide as it is told, its first and last cells blank");
        let got = text(title("Git", None, 18));
        assert_eq!(got, format!(" ╭─{CAP_LEFT}Git{CAP_RIGHT}{} ", "─".repeat(9)));
        assert_eq!(got.chars().count(), 18);
    }

    #[test]
    fn a_title_of_several_words_is_drawn_as_it_is_given_with_no_padding_spaces() {
        let got = text(title("Section Or Panel Title", None, 40));
        assert_eq!(got, format!(" ╭─{CAP_LEFT}Section Or Panel Title{CAP_RIGHT}{} ", "─".repeat(12)));
        assert!(!got.contains(&format!("{CAP_LEFT} ")) && !got.contains(&format!(" {CAP_RIGHT}")), "{got:?}");
    }
}
