//! The input box (contract §6.1.1): a labelled rounded box every text input is drawn in, a `BoxWidget<Full>` whose body is
//! the wrapped text.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::box_widget::{BoxWidget, Walls};
use crate::{palette, pill};

/// The columns the box takes besides its text: wall and padding on each side.
const SIDES: usize = 4;

/// The text width of a box `width` cells wide (at least 1).
pub(crate) fn text_width(width: usize) -> usize {
    width.saturating_sub(SIDES).max(1)
}

/// The number of text rows for `len` characters: `len / tw + 1`, `tw = width - 4` (1 for `width <= 4`).
pub(crate) fn text_rows(len: usize, width: usize) -> usize {
    len / text_width(width) + 1
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
        let shown = |from: usize, to: usize| self.text.get(from.min(self.text.len())..to.min(self.text.len())).unwrap_or_default().iter().collect::<String>();
        for n in 0..area.height {
            let k = self.first_row + usize::from(n);
            let (from, to) = (k * tw, (k + 1) * tw);
            let mut spans = Vec::new();
            if self.cursor / tw == k {
                let at = self.cursor % tw;
                spans.push(Span::styled(shown(from, from + at), palette::clickable_text()));
                spans.push(Span::styled(self.text.get(from + at).map_or(" ".to_owned(), char::to_string), palette::selected()));
                spans.push(Span::styled(shown(from + at + 1, to), palette::clickable_text()));
            } else {
                spans.push(Span::styled(shown(from, to), palette::clickable_text()));
            }
            Paragraph::new(Line::from(spans)).render(Rect::new(area.x, area.y + n, area.width, 1), buf);
        }
    }
}

/// The box for `text` in a wall rect `width` wide: the label as a pill on the top border (cut to the room, none for an empty
/// one), the text rows from `first_row` on.
pub(crate) fn boxed<'a>(label: &str, text: &'a [char], cursor: usize, focused: bool, first_row: usize, width: usize) -> impl Widget + 'a {
    let border = palette::input_border(focused);
    let room = width.saturating_sub(5);
    let label: String = label.chars().take(room).collect();
    let widget = BoxWidget::new(Walls::Full, border, Text { text, cursor, first_row });
    if label.is_empty() || width < 6 {
        return widget;
    }
    widget.title(Line::from(pill::pill(label, palette::label_pill(focused))))
}
