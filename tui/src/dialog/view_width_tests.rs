//! Red tests for audit-shell F1 in the dialog: wide lines wrap and a wide title is cut, by display columns
//! rather than `char`s.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::text::{Line, Span};

use super::view::{Dialog, Mode};

/// Ten `計` (U+8A08): ten `char`s, twenty display columns.
fn wide(n: usize) -> String {
    "計".repeat(n)
}

fn render(d: &mut Dialog, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| d.draw(f)).unwrap();
    terminal.backend().buffer().clone()
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

#[test]
fn a_dialog_with_wide_text_wraps_its_lines_and_cuts_its_title_by_columns() {
    // A 14-column frame: text width 10. Ten `計` are twenty columns, so two rows of five; the title is far wider
    // than the frame and must be cut by columns, leaving the top row exactly as wide as the box.
    let width = 14;
    let content = wide(10);
    let mut d = Dialog::new(wide(20), content.clone(), Mode::Text);
    let buf = render(&mut d, width, 10);

    let inside = usize::from(width) - 4;
    let mut drawn = String::new();
    for y in 2..buf.area.height.saturating_sub(3) {
        let row = read_columns(&buf, 2, y, inside);
        assert!(
            Line::from(row.as_str()).width() <= inside,
            "body row {y} is wider than {inside} columns: {row:?}"
        );
        drawn.push_str(row.trim_end());
    }
    assert_eq!(
        drawn, content,
        "the wide line wraps inside the dialog's width instead of being cut"
    );

    let top = read_columns(&buf, 0, 0, usize::from(width));
    assert_eq!(
        Line::from(top.as_str()).width(),
        usize::from(width),
        "the frame's top row is as wide as the box: {top:?}"
    );
    assert_eq!(
        top.chars().last(),
        Some('╮'),
        "the top right corner is not overwritten by a title too wide: {top:?}"
    );
}
