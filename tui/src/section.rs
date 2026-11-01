//! The in-pane section title (commands.md §17, "Section titles"): the helpers every view draws its section titles with, as a
//! **grey pill** on a line. Two forms remain:
//!
//! - [`title`]: inside the panel's padding of one column on each side, ` ──<pill>───…<count> `; the left cap sits three columns
//!   from the edge.
//! - [`flush_title`]: the Inbox's, with no padding cell, from column 0 to the last: `───<pill>───…<count>─`.
//!
//! The title is as the caller gives it, in Title Case, with no padding spaces inside the pill. It is never clickable (the helper
//! draws a line, it is no hit target). The spacing is the caller's layout, in rows.
//!
//! Pure: it only builds a line. The pill is a read-only block (hs-d534): background 7, text 0, its caps colour 7; the lines are
//! muted (colour 8).

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::cells;
use crate::palette;
use crate::pill;

/// The start of a title's row: two lines.
const PLAIN_LEAD: &str = "──";
/// The line to the right edge.
const RULE: &str = "─";
/// The padding: one blank column on each side.
const PAD: &str = " ";

/// The section title (hs-d478): a title `width` cells wide whose left cap sits three columns from the edge,
/// ` ──<pill>───…` (the padding, a muted `──`, the pill), for every in-pane title but the Inbox's (logs, legend, dialog titles).
pub fn title(title: &str, count: Option<usize>, width: usize) -> Line<'static> {
    row(PLAIN_LEAD, palette::muted_text(), title, count, palette::pill_static(), width)
}

/// The flush title: `───`, the title pill, a `─` fill, the count pill and one `─`, `width` cells, no padding. Cut and dropped as
/// [`title`]'s are: the count first, then the title's text, then the pill; width 0 is the empty line.
pub fn flush_title(title: &str, count: Option<usize>, width: usize) -> Line<'static> {
    if width == 0 {
        return Line::default();
    }
    let mut line = row_padded("───", 0, palette::muted_text(), title, count, palette::pill_static(), width - 1);
    line.spans.push(Span::styled(RULE, palette::muted_text()));
    line
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

    // ---- the title's layout (hs-d478, hs-d480): a corner, a line, the capped text, a line to the edge and the count pill, inside the padding ----

    fn cells(line: &Line<'static>) -> usize {
        text(line.clone()).chars().count()
    }

    fn muted(span: &Span<'_>) -> bool {
        span.style.fg == palette::muted_text().fg
    }



    #[test]
    fn the_non_collapsible_title_is_the_padding_two_lines_the_pill_and_a_line_to_the_edge() {
        let got = text(title("Logs", None, 20));
        assert_eq!(got, format!(" ──{OPEN_CAP}Logs{CLOSE_CAP}{} ", "─".repeat(20 - 2 - 2 - 6)));
        assert_eq!(got.chars().count(), 20);
        let got = text(title("Workflows", Some(3), 26));
        assert_eq!(got, format!(" ──{OPEN_CAP}Workflows{CLOSE_CAP}{}{} ", "─".repeat(26 - 1 - 2 - 5 - 3 - 1), count_pill("3")));
        assert_eq!(got.chars().count(), 26);
    }

    #[test]
    fn every_title_form_has_its_left_cap_at_column_3_and_the_non_collapsible_has_no_corner() {
        for (line, lead) in [(title("Logs", None, 22), " ──")] {
            let got = text(line.clone());
            assert!(got.starts_with(lead), "{got:?}");
            assert_eq!(got.chars().nth(3), Some('\u{E0B6}'), "the left cap sits three columns from the edge: {got:?}");
        }
        assert!(!text(title("Logs", None, 22)).contains('╭'), "a non-collapsible title has no corner");
    }

    #[test]
    fn the_pills_text_and_caps_are_the_static_pills_and_the_lines_are_muted() {
        for l in [title("Inbox", Some(35), 22)] {
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
        assert_eq!(text(title("Workflows", Some(3), 9)), format!(" ──{OPEN_CAP}Workflows{CLOSE_CAP} "), "the non-collapsible title drops its count the same way");
    }

    // ---- section look v4 (commands.md §17, "Section titles"; hs-d475 to hs-d480) ----

    /// `line` drawn alone in a row `width` cells wide.
    fn buffer_of(line: Line<'static>, width: u16) -> ratatui::buffer::Buffer {
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 1)).unwrap();
        terminal.draw(|f| f.render_widget(ratatui::widgets::Paragraph::new(line), f.area())).unwrap();
        terminal.backend().buffer().clone()
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
        for (line, name) in [(title("Inbox", Some(35), 22), "title")] {
            let buf = buffer_of(line, 22);
            check::grey_pill(&buf, 17, 0, "35"); // caps at columns 17 and 20, the padding at 21
            assert_eq!(buf[(21, 0)].symbol(), " ", "the right padding, {name}");
            assert_eq!(buf[(16, 0)].symbol(), "─", "a muted line before the count pill, {name}");
        }
        let buf = buffer_of(title("Workflows", Some(3), 26), 26);
        check::grey_pill(&buf, 22, 0, "3");
    }

    #[test]
    fn a_title_row_starts_at_column_1_and_ends_one_column_before_the_edge() {
        let got = text(title("Git", None, 18));
        assert_eq!(got, format!(" ──{OPEN_CAP}Git{CLOSE_CAP}{} ", "─".repeat(9)));
        assert_eq!(got.chars().count(), 18);
    }

    #[test]
    fn a_title_of_several_words_is_drawn_as_it_is_given_with_no_padding_spaces() {
        let got = text(title("Section Or Panel Title", None, 40));
        assert_eq!(got, format!(" ──{OPEN_CAP}Section Or Panel Title{CLOSE_CAP}{} ", "─".repeat(12)));
        assert!(!got.contains(&format!("{OPEN_CAP} ")) && !got.contains(&format!(" {CLOSE_CAP}")), "{got:?}");
    }
}
