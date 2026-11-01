//! Red tests for audit-shell F1 in the input box: wide text is wrapped, and the cursor placed, by display
//! columns rather than `char`s.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::input_box::{boxed, text_width};
use crate::palette;

/// Ten `計` (U+8A08): ten `char`s, twenty display columns.
fn wide(n: usize) -> String {
    "計".repeat(n)
}

/// The columns from `x` of `width`: a wide symbol is pushed once and its continuation cell skipped, so the
/// returned string is the row as drawn, not one symbol per buffer cell.
fn read_columns(buf: &Buffer, x: u16, y: u16, width: usize) -> String {
    let mut row = String::new();
    let mut cx = x;
    while usize::from(cx - x) < width {
        let symbol = buf[(cx, y)].symbol();
        row.push_str(symbol);
        cx += u16::try_from(Span::raw(symbol).width().max(1)).unwrap();
    }
    row
}

/// The text rows of a box `w` wide, from the body of the frame (`wall`, `pad`, text, `pad`, `wall`).
fn body_rows(buf: &Buffer, w: usize) -> Vec<String> {
    let width = text_width(w);
    (1..buf.area.height.saturating_sub(1))
        .map(|y| read_columns(buf, 2, y, width))
        .collect()
}

fn draw(text: &str, cursor: usize, w: usize, h: u16) -> Buffer {
    let chars: Vec<char> = text.chars().collect();
    let area = Rect::new(0, 0, u16::try_from(w).unwrap(), h);
    let mut buf = Buffer::empty(area);
    boxed("Title", &chars, cursor, true, 0, w).render(area, &mut buf);
    buf
}

#[test]
fn the_input_box_wraps_wide_text_by_columns_and_puts_the_cursor_in_its_column() {
    // Seven `計` (14 columns) fit a text width of 14; the other three and the `a` take the second row. The cursor is
    // after the tenth `計`: twenty display columns before it, so row 1, text column 6 — not char column 10.
    let text = format!("{}a", wide(10));
    let width = 18;
    let tw = text_width(width);
    assert_eq!(tw, 14);
    let buf = draw(&text, 10, width, 4);

    let rows = body_rows(&buf, width);
    let mut drawn = String::new();
    for (n, row) in rows.iter().enumerate() {
        assert!(
            Line::from(row.as_str()).width() <= tw,
            "text row {n} is wider than {tw} columns: {row:?}"
        );
        drawn.push_str(row.trim_end());
    }
    assert_eq!(
        drawn, text,
        "every character is drawn, wrapped at the text width: {rows:?}"
    );

    let selected: Vec<(u16, u16, String)> = (1..buf.area.height.saturating_sub(1))
        .flat_map(|y| (2..u16::try_from(width - 2).unwrap()).map(move |x| (x, y)))
        .filter(|&(x, y)| palette::check::is_selected(&buf[(x, y)]))
        .map(|(x, y)| (x, y, buf[(x, y)].symbol().to_owned()))
        .collect();
    assert_eq!(
        selected,
        [(2 + 6, 2, "a".to_owned())],
        "the cursor is in the column the ten `計` before it occupy"
    );
}
