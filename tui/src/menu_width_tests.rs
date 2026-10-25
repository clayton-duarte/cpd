//! Red tests for audit-tree F1 in the right-click menu: a busy entry is padded in display columns, not `char`s.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::{Position, Rect};

use crate::menu::{Entry, Menu};

/// Ten `計` (U+8A08): ten `char`s, twenty display columns.
const WIDE: &str = "計計計計計計計計計計";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Maybe(&'static str, bool);

impl Entry for Maybe {
    fn label(self) -> &'static str {
        self.0
    }

    fn busy(self) -> bool {
        self.1
    }
}

#[test]
fn a_busy_menu_entry_is_padded_by_columns() {
    let menu = Menu::new(vec![Maybe(WIDE, true), Maybe("Copy Link", false)], Position::new(1, 1));
    let mut terminal = Terminal::new(TestBackend::new(30, 8)).unwrap();
    terminal.draw(|f| menu.draw_at(f, f.area(), 3)).unwrap();
    let buf = terminal.backend().buffer();
    let rect = menu.rect(Rect::new(0, 0, 30, 8));
    // A full box's body: one wall and one padding column, then the entries; its last column holds the spinner.
    let (x, y) = (rect.x + 2, rect.y + 1);
    let inner = rect.width - 4;
    let row: String = (x..x + inner).map(|cx| buf[(cx, y)].symbol()).collect();
    assert_eq!(row.chars().filter(|c| *c == '計').count(), 10, "the whole wide label fits: {row:?}");
    let spinner = crate::palette::SPINNER[3];
    assert_eq!(buf[(x + inner - 1, y)].symbol(), spinner, "the spinner is in the body's last column, not one past it: {row:?}");
    assert_eq!(row.chars().count(), usize::from(inner), "the busy entry fills the body by columns: {row:?}");
}
