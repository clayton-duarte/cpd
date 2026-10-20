//! The right-click menu both the tree and the Deploy column open (commands.md §13.2): a pure model (the entries, the
//! selection, ↑↓, Enter, Esc, the hit test) and its drawing, a bordered box over the view at the click, clamped inside
//! the pane. The view opens it on a right click and gives it every key and click while it is open; there is no key
//! that opens it. What an entry does is the view's: the menu only says which one was picked.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// An entry of a menu: what the box shows. The view's own type says what it does.
pub trait Entry: Copy {
    fn label(self) -> &'static str;
}

/// What a key or a click did to the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome<E> {
    /// The menu stays open.
    Stay,
    /// The menu is dismissed; nothing happens.
    Close,
    /// The menu is dismissed and this entry was picked.
    Pick(E),
}

/// Where a click fell, relative to the drawn menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Item(usize),
    /// On the box, but on no entry: its border.
    Border,
    Outside,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu<E> {
    at: Position,
    items: Vec<E>,
    selected: usize,
}

impl<E: Entry> Menu<E> {
    /// The menu of `items` (never empty), opened by a click at `at`.
    pub fn new(items: Vec<E>, at: Position) -> Menu<E> {
        assert!(!items.is_empty(), "a menu has an entry");
        Menu { at, items, selected: 0 }
    }

    #[cfg(test)]
    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn key(&mut self, key: KeyEvent) -> Outcome<E> {
        match key.code {
            KeyCode::Esc => Outcome::Close,
            KeyCode::Enter => Outcome::Pick(self.items[self.selected]),
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(self.items.len() - 1);
                Outcome::Stay
            }
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                Outcome::Stay
            }
            _ => Outcome::Stay,
        }
    }

    /// The box inside `pane`: one border, one blank and the label per entry, at the click, moved left and up as far as
    /// it takes to fit (and cut, when the pane is smaller than the box).
    pub fn rect(&self, pane: Rect) -> Rect {
        let widest = self.items.iter().map(|i| Span::raw(i.label()).width()).max().unwrap_or(0) as u16;
        let width = (widest + 4).min(pane.width);
        let height = (self.items.len() as u16 + 2).min(pane.height);
        let x = self.at.x.min(pane.right().saturating_sub(width)).max(pane.x);
        let y = self.at.y.min(pane.bottom().saturating_sub(height)).max(pane.y);
        Rect::new(x, y, width, height)
    }

    pub fn hit(&self, pane: Rect, at: Position) -> Hit {
        let rect = self.rect(pane);
        if !rect.contains(at) {
            return Hit::Outside;
        }
        let inside = at.x > rect.x && at.x + 1 < rect.right() && at.y > rect.y && at.y + 1 < rect.bottom();
        match usize::from(at.y.saturating_sub(rect.y + 1)) {
            i if inside && i < self.items.len() => Hit::Item(i),
            _ => Hit::Border,
        }
    }

    /// A left click: an entry picks it, the border does nothing, outside closes the menu.
    pub fn click(&mut self, pane: Rect, at: Position) -> Outcome<E> {
        match self.hit(pane, at) {
            Hit::Item(i) => {
                self.selected = i;
                Outcome::Pick(self.items[i])
            }
            Hit::Border => Outcome::Stay,
            Hit::Outside => Outcome::Close,
        }
    }

    /// A mouse event while the menu is open: a button press is a [`click`](Menu::click) (only the left button picks;
    /// any button outside closes); nothing else does anything.
    pub fn mouse(&mut self, pane: Rect, m: MouseEvent) -> Outcome<E> {
        let MouseEventKind::Down(button) = m.kind else { return Outcome::Stay };
        match self.click(pane, Position::new(m.column, m.row)) {
            Outcome::Pick(_) if button != MouseButton::Left => Outcome::Stay,
            outcome => outcome,
        }
    }

    pub fn draw(&self, f: &mut Frame, pane: Rect) {
        let rect = self.rect(pane);
        let lines: Vec<Line> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let line = Line::from(format!(" {:<w$} ", item.label(), w = usize::from(rect.width.saturating_sub(4))));
                if i == self.selected { line.style(Modifier::REVERSED) } else { line }
            })
            .collect();
        f.render_widget(Clear, rect);
        f.render_widget(Paragraph::new(lines).block(Block::new().borders(Borders::ALL)), rect);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyModifiers;

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Word(&'static str);

    impl Entry for Word {
        fn label(self) -> &'static str {
            self.0
        }
    }

    fn words(labels: &[&'static str]) -> Vec<Word> {
        labels.iter().copied().map(Word).collect()
    }

    fn press(menu: &mut Menu<Word>, code: KeyCode) -> Outcome<Word> {
        menu.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
        MouseEvent { kind, column: x, row: y, modifiers: KeyModifiers::NONE }
    }

    const PANE: Rect = Rect::new(0, 0, 44, 12);

    /// Four entries, the widest `New Plan…` (9).
    fn four(x: u16, y: u16) -> Menu<Word> {
        Menu::new(words(&["New Plan…", "Rename…", "Copy id", "Archive"]), Position::new(x, y))
    }

    /// Six entries, the widest `Rename…` (7): the box is x 5..=15, y 2..=9 at (5, 2).
    fn six() -> Menu<Word> {
        Menu::new(words(&["Open", "Rename…", "Fork…", "Copy id", "Close", "Archive"]), Position::new(5, 2))
    }

    #[test]
    fn arrows_move_the_selection_clamped_and_enter_picks() {
        let mut menu = six();
        assert_eq!(menu.selected(), 0);
        assert_eq!(press(&mut menu, KeyCode::Up), Outcome::Stay);
        assert_eq!(menu.selected(), 0);
        assert_eq!(press(&mut menu, KeyCode::Down), Outcome::Stay);
        assert_eq!(press(&mut menu, KeyCode::Down), Outcome::Stay);
        assert_eq!(menu.selected(), 2);
        assert_eq!(press(&mut menu, KeyCode::Enter), Outcome::Pick(Word("Fork…")));
        for _ in 0..9 {
            press(&mut menu, KeyCode::Down);
        }
        assert_eq!(menu.selected(), 5);
        assert_eq!(press(&mut menu, KeyCode::Enter), Outcome::Pick(Word("Archive")));
        assert_eq!(press(&mut menu, KeyCode::Up), Outcome::Stay);
        assert_eq!(press(&mut menu, KeyCode::Enter), Outcome::Pick(Word("Close")));
    }

    #[test]
    fn esc_closes_and_no_other_key_does_anything() {
        let mut menu = four(0, 0);
        assert_eq!(press(&mut menu, KeyCode::Esc), Outcome::Close);
        for key in [KeyCode::Char('j'), KeyCode::Char('n'), KeyCode::Char('x'), KeyCode::Char('?'), KeyCode::Char('q'), KeyCode::Tab] {
            assert_eq!(press(&mut menu, key), Outcome::Stay, "{key:?}");
        }
        assert_eq!(menu.selected(), 0);
    }

    #[test]
    fn the_box_is_at_the_click_and_clamped_inside_the_pane() {
        assert_eq!(four(5, 2).rect(PANE), Rect::new(5, 2, 13, 6));
        assert_eq!(four(40, 2).rect(PANE), Rect::new(31, 2, 13, 6), "moved left");
        assert_eq!(four(5, 10).rect(PANE), Rect::new(5, 6, 13, 6), "moved up");
        assert_eq!(four(43, 11).rect(PANE), Rect::new(31, 6, 13, 6));
        let pane = Rect::new(2, 3, 8, 4);
        assert_eq!(four(0, 0).rect(pane), Rect::new(2, 3, 8, 4), "cut to a pane smaller than the box");
        assert_eq!(four(9, 9).rect(pane), Rect::new(2, 3, 8, 4));
    }

    #[test]
    fn the_hit_test_tells_items_border_and_outside() {
        let menu = six();
        let hit = |x, y| menu.hit(PANE, Position::new(x, y));
        assert_eq!(hit(6, 3), Hit::Item(0));
        assert_eq!(hit(10, 3), Hit::Item(0));
        assert_eq!(hit(9, 8), Hit::Item(5));
        for (x, y) in [(5, 2), (10, 2), (5, 5), (15, 5), (10, 9), (15, 9)] {
            assert_eq!(hit(x, y), Hit::Border, "({x}, {y})");
        }
        for (x, y) in [(4, 3), (16, 3), (6, 1), (6, 10), (0, 0), (43, 11)] {
            assert_eq!(hit(x, y), Hit::Outside, "({x}, {y})");
        }
        assert_eq!(hit(14, 3), Hit::Item(0), "the last column inside");
    }

    #[test]
    fn a_click_picks_an_item_ignores_the_border_and_closes_outside() {
        let mut menu = six();
        assert_eq!(menu.click(PANE, Position::new(7, 7)), Outcome::Pick(Word("Close")));
        assert_eq!(menu.selected(), 4);
        assert_eq!(menu.click(PANE, Position::new(7, 2)), Outcome::Stay);
        assert_eq!(menu.click(PANE, Position::new(30, 10)), Outcome::Close);
    }

    #[test]
    fn only_a_left_press_picks_any_press_outside_closes_and_nothing_else_counts() {
        let mut menu = six();
        let down = |button| MouseEventKind::Down(button);
        assert_eq!(menu.mouse(PANE, mouse(down(MouseButton::Left), 7, 3)), Outcome::Pick(Word("Open")));
        assert_eq!(menu.mouse(PANE, mouse(down(MouseButton::Right), 7, 3)), Outcome::Stay, "a right click on an item");
        assert_eq!(menu.mouse(PANE, mouse(down(MouseButton::Right), 30, 10)), Outcome::Close);
        assert_eq!(menu.mouse(PANE, mouse(down(MouseButton::Left), 30, 10)), Outcome::Close);
        assert_eq!(menu.mouse(PANE, mouse(MouseEventKind::Up(MouseButton::Left), 30, 10)), Outcome::Stay);
        assert_eq!(menu.mouse(PANE, mouse(MouseEventKind::ScrollDown, 30, 10)), Outcome::Stay);
    }

    #[test]
    fn the_menu_is_drawn_as_a_bordered_box_over_what_is_there() {
        let mut terminal = Terminal::new(TestBackend::new(20, 7)).unwrap();
        let menu = four(3, 1);
        terminal
            .draw(|f| {
                f.render_widget(Paragraph::new("xxxxxxxxxxxxxxxxxxxx\n".repeat(7)), f.area());
                menu.draw(f, f.area());
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let rows: Vec<String> = (0..7).map(|y| (0..20).map(|x| buf[(x, y)].symbol()).collect()).collect();
        assert_eq!(
            rows,
            ["xxxxxxxxxxxxxxxxxxxx", "xxx┌───────────┐xxxx", "xxx│ New Plan… │xxxx", "xxx│ Rename…   │xxxx", "xxx│ Copy id   │xxxx", "xxx│ Archive   │xxxx", "xxx└───────────┘xxxx"]
                .map(|r| r.chars().take(20).collect::<String>()),
        );
        assert!(buf[(5, 2)].modifier.contains(Modifier::REVERSED), "the selected item");
        assert!(!buf[(5, 3)].modifier.contains(Modifier::REVERSED));
    }
}
