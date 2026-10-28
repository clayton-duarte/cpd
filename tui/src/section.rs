//! The in-pane section title (commands.md §17, "Section titles"; hs-d298, hs-d308, hs-d475, hs-d476): the one helper every
//! view draws its section titles with, as a **grey pill** on a line, inside the panel's padding of one column on each
//! side: a blank cell, `╭─`, the title as a pill (its text inverted, between the powerline semicircle caps `` and
//! ``), a muted `─` line up to the optional count (a grey pill too, hs-d480) and a blank cell: ` ╭─Inbox────…────35 `; a non-collapsible title starts ` ──` instead of ` ╭─` (hs-d478). The title is as the
//! caller gives it, in Title Case, with no padding spaces inside the pill. It is never clickable (the helper draws a line,
//! it is no hit target).
//!
//! The spacing is the caller's layout, in rows: one empty row above and one below a title outside a collapsible widget
//! (`crate::fold` has its own margins); the body has no vertical spacing.
//!
//! Pure: it only builds a line. The pill is a read-only block (hs-d534): background 7, text 0, its caps colour 7; the corner and
//! the lines are muted (colour 8).

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::cells;
use crate::palette;
use crate::pill;

/// The start of a collapsible title's row: the corner and one line.
const LEAD: &str = "╭─";
/// The start of a non-collapsible title's row: two lines, no corner.
const PLAIN_LEAD: &str = "──";
/// The line to the right edge.
const RULE: &str = "─";
/// The padding: one blank column on each side.
const PAD: &str = " ";

/// The **non-collapsible** section title (hs-d478): a title `width` cells wide whose left cap sits three columns from the edge,
/// ` ──<pill>───…` (the padding, a muted `──`, the pill), for every in-pane title that is no collapsible widget's (logs, legend,
/// the Deploy column's sections, dialog titles).
pub fn title(title: &str, count: Option<usize>, width: usize) -> Line<'static> {
    row(PLAIN_LEAD, palette::muted_text(), title, count, palette::pill_static(), width)
}

/// The **collapsible** section title (hs-d478): a title `width` cells wide, its first and last cell the padding: ` ╭─<pill>───…`
/// with, when `count` is given and fits after at least one `─`, the count against the padding on the right. For a title that heads a
/// collapsible widget (the Inbox's); its left cap sits three columns from the edge, as [`title`]'s does. A title wider than the room
/// is cut (a pill narrower than 3 cells is left out); the line never wraps. The count is a grey pill too (hs-d480).
pub fn collapsible_title(title: &str, count: Option<usize>, width: usize) -> Line<'static> {
    row(LEAD, palette::muted_text(), title, count, palette::pill_static(), width)
}

/// The title of a box drawn with `Walls::Rail` (`crate::box_widget`): what the widget does not draw, the line after the corner
/// and the title's pill to the last cell, no corner and no padding (the widget owns both): `─<pill>───…<count>`, `width` cells.
/// `collapsible_title` is the Plan's other callers'.
pub fn rail_title(title: &str, count: Option<usize>, width: usize) -> Line<'static> {
    row_padded(RULE, 0, palette::muted_text(), title, count, palette::pill_static(), width)
}

/// The flush title: `───`, the title pill, a `─` fill, the count pill and one `─`, `width` cells, no padding.
pub fn flush_title(title: &str, count: Option<usize>, width: usize) -> Line<'static> {
    let _ = (title, count, width);
    todo!("flush title: the implementation Job")
}

/// The row the forms share: `lead` (two cells) and `muted`, the style of the lead and the lines, and `pill_style` are what differ.
fn row(lead: &str, muted: Style, title: &str, count: Option<usize>, pill_style: Style, width: usize) -> Line<'static> {
    row_padded(lead, usize::from(width >= 2), muted, title, count, pill_style, width)
}

/// [`row`] with its padding, `pad` blank cells on each side, given.
fn row_padded(lead: &str, pad: usize, muted: Style, title: &str, count: Option<usize>, pill_style: Style, width: usize) -> Line<'static> {
    let width = width - 2 * pad;
    let lead: String = cells::take(lead, width).to_string();
    let left = width - cells::width(&lead);
    let text: String = cells::take(title, left.saturating_sub(2)).to_string();
    let pill = left >= 3 && !text.is_empty();
    let left = if pill { left - cells::width(&text) - 2 } else { left };
    let count = count.map(|n| n.to_string()).filter(|c| pill && 1 + cells::width(c) + 2 <= left).unwrap_or_default();
    let fill = left - if count.is_empty() { 0 } else { cells::width(&count) + 2 };
    let mut spans = vec![Span::raw(PAD.repeat(pad)), Span::styled(lead, muted)];
    if pill {
        spans.extend(pill::pill(text, pill_style));
    }
    spans.push(Span::styled(RULE.repeat(fill), muted));
    if !count.is_empty() {
        spans.extend(pill::pill(count, pill_style));
    }
    spans.push(Span::raw(PAD.repeat(pad)));
    Line::from(spans)
}

/// What the tests of every view check of a section title's pill (hs-d475): one place, not a copy per view.
/// The caps the tests spell (A8): a deliberate spec change fails them on purpose.
#[cfg(test)]
const OPEN_CAP: &str = "\u{E0B6}";
#[cfg(test)]
const CLOSE_CAP: &str = "\u{E0B4}";

#[cfg(test)]
pub(crate) mod check {
    use ratatui::buffer::Buffer;
    use ratatui::buffer::Cell;

    use super::{OPEN_CAP, CLOSE_CAP};
    use crate::palette;

    /// The pill whose left cap is at (`x`, `y`) of `buf` reads `text` between its two caps, with no padding spaces, and is grey:
    /// the caps are the pill's background colour; the text is the static pill's (hs-d534).
    pub(crate) fn grey_pill(buf: &Buffer, x: u16, y: u16, text: &str) {
        let grey = palette::pill_static().fg.unwrap();
        let reset = Cell::EMPTY.bg; // the terminal's own background, which no colour names (no colour outside the palette)
        let width = u16::try_from(text.chars().count()).unwrap();
        let drawn: String = (x..x + width + 2).map(|x| buf[(x, y)].symbol()).collect();
        assert_eq!(drawn, format!("{OPEN_CAP}{text}{CLOSE_CAP}"), "the pill at ({x}, {y})");
        for cap in [x, x + width + 1] {
            let cell = &buf[(cap, y)];
            assert_eq!((cell.fg, cell.bg), (grey, reset), "the cap at ({cap}, {y}) is the pill's colour: {cell:?}");
        }
        let pill = palette::pill_static();
        for at in x + 1..=x + width {
            let cell = &buf[(at, y)];
            assert_eq!((Some(cell.fg), Some(cell.bg)), (pill.fg, pill.bg), "the text at ({at}, {y}): {cell:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: Line<'static>) -> String {
        line.spans.iter().map(|s| s.content.to_string()).collect()
    }

    /// `n` as a pill: the caps around the digits, no padding spaces (hs-d480).
    fn count_pill(n: &str) -> String {
        format!("{OPEN_CAP}{n}{CLOSE_CAP}")
    }

    // ---- the collapsible title's layout (hs-d478, hs-d480): a corner, a line, the capped text, a line to the edge and the count pill, inside the padding ----

    fn cells(line: &Line<'static>) -> usize {
        text(line.clone()).chars().count()
    }

    fn muted(span: &Span<'_>) -> bool {
        span.style.fg == palette::muted_text().fg
    }

    #[test]
    fn the_collapsible_title_is_a_corner_a_line_the_capped_text_a_line_to_the_edge_and_the_count_pill() {
        let got = text(collapsible_title("Inbox", Some(35), 22));
        assert_eq!(got, format!(" ╭─{OPEN_CAP}Inbox{CLOSE_CAP}{}{} ", "─".repeat(7), count_pill("35")));
        assert_eq!(got.chars().count(), 22, "full width, the padding included");
        assert_eq!(text(collapsible_title("PRs", Some(3), 26)), format!(" ╭─{OPEN_CAP}PRs{CLOSE_CAP}{}{} ", "─".repeat(26 - 1 - 2 - 5 - 3 - 1), count_pill("3")));
        assert_eq!(text(collapsible_title("Inbox", Some(0), 16)), format!(" ╭─{OPEN_CAP}Inbox{CLOSE_CAP}{}{} ", "─".repeat(2), count_pill("0")));
    }

    #[test]
    fn the_collapsible_title_without_a_count_fills_to_the_right_padding() {
        let got = text(collapsible_title("Git", None, 20));
        assert_eq!(got, format!(" ╭─{OPEN_CAP}Git{CLOSE_CAP}{} ", "─".repeat(20 - 2 - 2 - 5)));
        assert_eq!(got.chars().count(), 20);
        assert_eq!(text(collapsible_title("Jira", None, 14)), format!(" ╭─{OPEN_CAP}Jira{CLOSE_CAP}{} ", "─".repeat(14 - 2 - 2 - 6)));
    }

    #[test]
    fn the_non_collapsible_title_is_the_padding_two_lines_the_pill_and_a_line_to_the_edge() {
        let got = text(title("Logs", None, 20));
        assert_eq!(got, format!(" ──{OPEN_CAP}Logs{CLOSE_CAP}{} ", "─".repeat(20 - 2 - 2 - 6)));
        assert_eq!(got.chars().count(), 20);
        let got = text(title("PRs", Some(3), 26));
        assert_eq!(got, format!(" ──{OPEN_CAP}PRs{CLOSE_CAP}{}{} ", "─".repeat(26 - 1 - 2 - 5 - 3 - 1), count_pill("3")));
        assert_eq!(got.chars().count(), 26);
    }

    #[test]
    fn every_title_form_has_its_left_cap_at_column_3_and_the_non_collapsible_has_no_corner() {
        for (line, lead) in [(collapsible_title("Inbox", None, 22), " ╭─"), (title("Logs", None, 22), " ──")] {
            let got = text(line.clone());
            assert!(got.starts_with(lead), "{got:?}");
            assert_eq!(got.chars().nth(3), Some('\u{E0B6}'), "the left cap sits three columns from the edge: {got:?}");
        }
        assert!(!text(title("Logs", None, 22)).contains('╭'), "a non-collapsible title has no corner");
    }

    #[test]
    fn the_pills_text_and_caps_are_the_static_pills_and_the_lines_are_muted() {
        for l in [collapsible_title("Inbox", Some(35), 22), title("Inbox", Some(35), 22)] {
            let find = |what: &str| l.spans.iter().find(|s| s.content.contains(what)).unwrap_or_else(|| panic!("no span with {what:?}: {:?}", l.spans));
            let pill = find("Inbox");
            assert_eq!(pill.style, palette::pill_static(), "the title text is the static pill's");
            for cap in [OPEN_CAP, CLOSE_CAP] {
                let cap = find(cap);
                assert_eq!(cap.style.fg, palette::pill_static().fg, "a cap is the pill's colour: {cap:?}");
            }
            let lines: Vec<_> = l.spans.iter().filter(|s| !s.content.is_empty() && s.content.chars().all(|c| c == '╭' || c == '─')).collect();
            assert!(lines.iter().any(|s| s.content.contains('─')), "the lines: {:?}", l.spans);
            assert!(lines.iter().all(|s| muted(s)), "the corner and every line are muted: {lines:?}");
            assert_eq!(cells(&l), 22);
        }
    }

    #[test]
    fn a_narrow_width_drops_the_count_then_cuts_the_title_then_the_pill() {
        assert_eq!(text(collapsible_title("PRs", Some(3), 9)), format!(" ╭─{OPEN_CAP}PRs{CLOSE_CAP} "), "no room for a line and the count");
        assert_eq!(text(collapsible_title("PRs", Some(3), 11)), format!(" ╭─{OPEN_CAP}PRs{CLOSE_CAP}── "), "a line fits, the count pill does not");
        assert_eq!(text(collapsible_title("Jira", None, 8)), format!(" ╭─{OPEN_CAP}Ji{CLOSE_CAP} "));
        assert_eq!(text(collapsible_title("Jira", None, 5)), " ╭── ");
        assert_eq!(text(collapsible_title("Jira", None, 3)), " ╭ ");
        assert_eq!(text(collapsible_title("Jira", None, 2)), "  ");
        assert_eq!(text(collapsible_title("Jira", None, 1)), "╭");
        assert_eq!(text(collapsible_title("Jira", Some(1), 0)), "");
        assert_eq!(text(title("PRs", Some(3), 9)), format!(" ──{OPEN_CAP}PRs{CLOSE_CAP} "), "the non-collapsible title drops its count the same way");
    }

    // ---- section look v4 (commands.md §17, "Section titles"; hs-d475 to hs-d480): red until `title` and `collapsible_title` draw it ----

    /// `line` drawn alone in a row `width` cells wide.
    fn buffer_of(line: Line<'static>, width: u16) -> ratatui::buffer::Buffer {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 1)).unwrap();
        terminal.draw(|f| f.render_widget(ratatui::widgets::Paragraph::new(line), f.area())).unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn the_collapsible_pill_is_grey_with_black_text_and_caps_in_its_colour() {
        let buf = buffer_of(collapsible_title("Inbox", Some(35), 22), 22);
        let row: String = (0..22).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(row.starts_with(" ╭─"), "{row:?}");
        check::grey_pill(&buf, 3, 0, "Inbox");
    }

    #[test]
    fn the_non_collapsible_pill_is_grey_and_its_left_cap_is_at_column_3() {
        let buf = buffer_of(title("Logs", None, 22), 22);
        let row: String = (0..22).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(row.starts_with(" ──"), "{row:?}");
        for x in [1, 2] {
            assert_eq!((buf[(x, 0)].symbol(), buf[(x, 0)].fg), ("─", palette::muted_text().fg.unwrap()), "the muted `──` at column {x}");
        }
        check::grey_pill(&buf, 3, 0, "Logs");
    }

    #[test]
    fn a_count_is_a_grey_pill_at_the_rows_end_inside_the_right_padding() {
        for (line, name) in [(collapsible_title("Inbox", Some(35), 22), "collapsible"), (title("Inbox", Some(35), 22), "non-collapsible")] {
            let buf = buffer_of(line, 22);
            check::grey_pill(&buf, 17, 0, "35"); // caps at columns 17 and 20, the padding at 21
            assert_eq!(buf[(21, 0)].symbol(), " ", "the right padding, {name}");
            assert_eq!(buf[(16, 0)].symbol(), "─", "a muted line before the count pill, {name}");
        }
        let buf = buffer_of(title("PRs", Some(3), 26), 26);
        check::grey_pill(&buf, 22, 0, "3");
    }

    #[test]
    fn a_title_row_starts_at_column_1_and_ends_one_column_before_the_edge() {
        let got = text(collapsible_title("Inbox", Some(35), 22));
        assert_eq!(got, format!(" ╭─{OPEN_CAP}Inbox{CLOSE_CAP}{}{} ", "─".repeat(7), count_pill("35")));
        assert_eq!(got.chars().count(), 22, "the row is as wide as it is told, its first and last cells blank");
        let got = text(title("Git", None, 18));
        assert_eq!(got, format!(" ──{OPEN_CAP}Git{CLOSE_CAP}{} ", "─".repeat(9)));
        assert_eq!(got.chars().count(), 18);
    }

    #[test]
    fn a_title_of_several_words_is_drawn_as_it_is_given_with_no_padding_spaces() {
        let got = text(title("Section Or Panel Title", None, 40));
        assert_eq!(got, format!(" ──{OPEN_CAP}Section Or Panel Title{CLOSE_CAP}{} ", "─".repeat(12)));
        assert!(!got.contains(&format!("{OPEN_CAP} ")) && !got.contains(&format!(" {CLOSE_CAP}")), "{got:?}");
        let got = text(collapsible_title("Section Or Panel Title", None, 40));
        assert_eq!(got, format!(" ╭─{OPEN_CAP}Section Or Panel Title{CLOSE_CAP}{} ", "─".repeat(12)));
    }
}
