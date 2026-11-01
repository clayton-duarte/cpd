//! The input box (contract §6.1.1): a labelled rounded box every text input is drawn in, a `BoxWidget<Full>` whose body is
//! the wrapped text.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::box_widget::{BoxWidget, Walls};
use crate::cells;
use crate::{palette, pill};

/// The columns the box takes besides its text: wall and padding on each side.
const SIDES: usize = 4;

/// The text width of a box `width` cells wide (at least 1).
pub(crate) fn text_width(width: usize) -> usize {
    width.saturating_sub(SIDES).max(1)
}

/// The number of text rows for `len` characters: `len / tw + 1`, `tw = width - 4` (1 for `width <= 4`).
#[cfg(test)]
pub(crate) fn text_rows(len: usize, width: usize) -> usize {
    len / text_width(width) + 1
}

/// The wrapped rows of `text` at `width` display columns: each row a `(from, to)` range of char indices. A
/// wide character that does not fit in the row's remaining columns starts the next row whole.
fn rows(text: &[char], width: usize) -> Vec<(usize, usize)> {
    let width = width.max(1);
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut used = 0;
    for (i, c) in text.iter().enumerate() {
        let w = cells::char_width(*c);
        if used + w > width && i > start {
            ranges.push((start, i));
            start = i;
            used = 0;
        }
        used += w;
    }
    ranges.push((start, text.len()));
    ranges
}

/// Where the cursor at char index `cursor` sits among rows wrapped at `width` columns: `(row, column)` in display
/// columns. The cursor keeps its index; its column is the display width of the text before it.
pub(crate) fn cursor_at(text: &[char], cursor: usize, width: usize) -> (usize, usize) {
    let tw = width.max(1);
    let before: usize = text[..cursor.min(text.len())]
        .iter()
        .map(|&c| cells::char_width(c))
        .sum();
    (before / tw, before % tw)
}

/// The rows a box shows for `text` wrapped at `width` columns with the cursor at `cursor`: the wrapped rows, or
/// one more when the cursor sits in a row past the last one.
pub(crate) fn wrapped_rows(text: &[char], cursor: usize, width: usize) -> usize {
    let (row, _) = cursor_at(text, cursor, width);
    rows(text, width).len().max(row + 1)
}

/// The body of the box: the text rows from `first_row` on, the cursor a block.
struct Text<'a> {
    text: &'a [char],
    cursor: usize,
    first_row: usize,
}

impl Widget for Text<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let tw = usize::from(area.width).max(1);
        let ranges = rows(self.text, tw);
        let (cursor_row, _) = cursor_at(self.text, self.cursor, tw);
        let shown = |from: usize, to: usize| {
            self.text
                .get(from.min(self.text.len())..to.min(self.text.len()))
                .unwrap_or_default()
                .iter()
                .collect::<String>()
        };
        for n in 0..area.height {
            let k = self.first_row + usize::from(n);
            let (from, to) = ranges
                .get(k)
                .copied()
                .unwrap_or((self.text.len(), self.text.len()));
            let mut spans = Vec::new();
            if k == cursor_row {
                let at = self.cursor.clamp(from, to);
                spans.push(Span::styled(shown(from, at), palette::clickable_text()));
                spans.push(Span::styled(
                    self.text.get(at).map_or(" ".to_owned(), char::to_string),
                    palette::selected(),
                ));
                spans.push(Span::styled(
                    shown((at + 1).min(to), to),
                    palette::clickable_text(),
                ));
            } else {
                spans.push(Span::styled(shown(from, to), palette::clickable_text()));
            }
            Paragraph::new(Line::from(spans))
                .render(Rect::new(area.x, area.y + n, area.width, 1), buf);
        }
    }
}

/// The box for `text` in a wall rect `width` wide: the label as a pill on the top border (cut to the room, none for an empty
/// one), the text rows from `first_row` on.
pub(crate) fn boxed<'a>(
    label: &str,
    text: &'a [char],
    cursor: usize,
    focused: bool,
    first_row: usize,
    width: usize,
) -> impl Widget + 'a {
    let border = palette::input_border(focused);
    let room = width.saturating_sub(5);
    let label = cells::take(label, room);
    let widget = BoxWidget::new(
        Walls::Full,
        border,
        Text {
            text,
            cursor,
            first_row,
        },
    );
    if label.is_empty() || width < 6 {
        return widget;
    }
    widget.title(Line::from(pill::pill(
        label.to_owned(),
        palette::label_pill(focused),
    )))
}
