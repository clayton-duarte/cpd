//! The input box (contract §6.1.1): a labelled rounded box every text input is drawn in.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::{palette, section};

/// The columns the box takes besides its text: margin, wall and padding on each side.
const SIDES: usize = 6;

/// The text width of a box `width` cells wide (at least 1).
fn text_width(width: usize) -> usize {
    width.saturating_sub(SIDES).max(1)
}

/// `line` cut to `width` cells.
fn clip(line: Line<'static>, width: usize) -> Line<'static> {
    let mut left = width;
    let mut spans = Vec::new();
    for span in line.spans {
        let n = span.content.chars().count();
        if left == 0 {
            break;
        }
        if n <= left {
            left -= n;
            spans.push(span);
        } else {
            spans.push(Span::styled(span.content.chars().take(left).collect::<String>(), span.style));
            break;
        }
    }
    Line::from(spans)
}

/// The bottom border row `lead` + `─` × (width - 4) + `end`, in the margin.
fn rule(lead: &str, end: &str, border: Style, width: usize) -> Line<'static> {
    let fill = "─".repeat(width.saturating_sub(4));
    clip(Line::from(vec![Span::raw(" "), Span::styled(format!("{lead}{fill}{end}"), border), Span::raw(" ")]), width)
}

/// The box's rows: the top border with the label as a pill, the wrapped text rows, the bottom border; each `width` cells.
pub(crate) fn lines(label: &str, text: &[char], cursor: usize, focused: bool, width: usize) -> Vec<Line<'static>> {
    let border = palette::input_border(focused);
    let tw = text_width(width);
    let mut out = vec![clip(section::box_top(label, focused, width), width)];
    let shown = |range: std::ops::Range<usize>| text.get(range.start.min(text.len())..range.end.min(text.len())).unwrap_or_default().iter().collect::<String>();
    for k in 0..text_rows(text.len(), width) {
        let (from, to) = (k * tw, (k + 1) * tw);
        let mut spans = vec![Span::raw(" "), Span::styled("│", border), Span::raw(" ")];
        let mut cells = 0;
        let mut text_span = |s: String, style: Style, spans: &mut Vec<Span<'static>>| {
            cells += s.chars().count();
            spans.push(Span::styled(s, style));
        };
        if cursor / tw == k {
            let at = cursor % tw;
            let after = from + at + 1;
            text_span(shown(from..from + at), palette::clickable_text(), &mut spans);
            text_span(text.get(from + at).map_or(" ".to_owned(), char::to_string), palette::selected(), &mut spans);
            text_span(shown(after..to), palette::clickable_text(), &mut spans);
        } else {
            text_span(shown(from..to), palette::clickable_text(), &mut spans);
        }
        spans.push(Span::raw(" ".repeat(tw - cells)));
        spans.extend([Span::raw(" "), Span::styled("│", border), Span::raw(" ")]);
        out.push(clip(Line::from(spans), width));
    }
    out.push(rule("╰", "╯", border, width));
    out
}

/// The number of text rows for `len` characters: `len / tw + 1`, `tw = width - 6` (1 for `width <= 6`).
pub(crate) fn text_rows(len: usize, width: usize) -> usize {
    len / text_width(width) + 1
}
