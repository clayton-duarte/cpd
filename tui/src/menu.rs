//! The right-click menu both the tree and the Dispatch column open (commands.md §13.2): a pure model (the entries, the
//! selection, ↑↓, Enter, Esc, the hit test) and its drawing, a bordered box over the view at the click, clamped inside
//! the pane. The view opens it on a right click and gives it every key and click while it is open; there is no key
//! that opens it. What an entry does is the view's: the menu only says which one was picked.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use crate::palette::{self, Meaning};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::box_widget::{BoxWidget, Walls};

/// The columns a busy entry's spinner takes beside its label: a space and the glyph.
const SPINNER_WIDTH: usize = 2;

/// An entry of a menu: what the box shows. The view's own type says what it does.
pub trait Entry: Copy {
    fn label(self) -> &'static str;

    /// A busy entry (hs-d495) is shown with the spinner and cannot be selected: no key and no click picks it.
    fn busy(self) -> bool {
        false
    }
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
            KeyCode::Enter => self.pick(self.selected),
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

    /// Picks entry `i`: a busy entry is not selectable, the menu stays as it is.
    fn pick(&self, i: usize) -> Outcome<E> {
        if self.items[i].busy() { Outcome::Stay } else { Outcome::Pick(self.items[i]) }
    }

    /// The box inside `pane`: one border, one blank and the label per entry, at the click, moved left and up as far as
    /// it takes to fit (and cut, when the pane is smaller than the box).
    pub fn rect(&self, pane: Rect) -> Rect {
        let widest = self.items.iter().map(|i| Span::raw(i.label()).width() + if i.busy() { SPINNER_WIDTH } else { 0 }).max().unwrap_or(0) as u16;
        Rect::new(self.at.x, self.at.y, widest + 4, self.items.len() as u16 + 2).clamp(pane)
    }

    pub fn hit(&self, pane: Rect, at: Position) -> Hit {
        let rect = self.rect(pane);
        if !rect.contains(at) {
            return Hit::Outside;
        }
        let inner = Block::bordered().inner(rect);
        if !inner.contains(at) {
            return Hit::Border;
        }
        match usize::from(at.y.saturating_sub(inner.y)) {
            i if i < self.items.len() => Hit::Item(i),
            _ => Hit::Border,
        }
    }

    /// A left click: an entry picks it, the border does nothing, outside closes the menu.
    pub fn click(&mut self, pane: Rect, at: Position) -> Outcome<E> {
        match self.hit(pane, at) {
            Hit::Item(i) if self.items[i].busy() => Outcome::Stay,
            Hit::Item(i) => {
                self.selected = i;
                self.pick(i)
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

    /// [`Menu::draw_at`] at the shared clock's frame now.
    pub fn draw(&self, f: &mut Frame, pane: Rect) {
        self.draw_at(f, pane, crate::palette::frame(std::time::SystemTime::now()));
    }

    /// Draws the menu; a busy entry ([`Entry::busy`]) shows `palette::SPINNER[frame % 28]` (hs-d495).
    pub fn draw_at(&self, f: &mut Frame, pane: Rect, frame: usize) {
        let rect = self.rect(pane);
        let lines: Vec<Line> = self
            .items
            .iter()
            .map(|item| {
                let mut spans = Vec::new();
                if item.busy() {
                    let spinner = crate::palette::SPINNER[frame % crate::palette::SPINNER.len()];
                    spans.push(Span::raw(format!("{} ", item.label())));
                    spans.push(Span::styled(spinner, palette::fg_clickable(Meaning::Review)));
                } else {
                    spans.push(Span::raw(item.label()));
                }
                Line::from(spans).style(palette::clickable_text())
            })
            .collect();
                let selected = self.selected as u16;
        f.render_widget(Clear, rect);
        let body = Paragraph::new(lines);
        f.render_widget(BoxWidget::new(Walls::Full, palette::muted_text(), body).highlight(selected..selected + 1), rect);
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
            ["xxxxxxxxxxxxxxxxxxxx", "xxx╭───────────╮xxxx", "xxx│ New Plan… │xxxx", "xxx│ Rename…   │xxxx", "xxx│ Copy id   │xxxx", "xxx│ Archive   │xxxx", "xxx╰───────────╯xxxx"]
                .map(|r| r.chars().take(20).collect::<String>()),
        );
        assert_eq!((buf[(5, 2)].bg, buf[(5, 2)].fg), (palette::selected().bg.unwrap(), palette::selected().fg.unwrap()), "the selected item: the blue block");
        assert_eq!((buf[(5, 3)].bg, buf[(5, 3)].fg), (ratatui::buffer::Cell::EMPTY.bg, palette::clickable_text().fg.unwrap()), "the others: clickable text");
    }

    // ---- a busy entry (hs-d495): the spinner in place of the button, and not selectable ----

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

    /// `Merge` (busy) and `Copy Link`, at (5, 2): the box is x 5..=17, y 2..=5.
    fn merging() -> Menu<Maybe> {
        Menu::new(vec![Maybe("Merge", true), Maybe("Copy Link", false)], Position::new(5, 2))
    }

    #[test]
    fn a_busy_entry_is_never_picked_by_enter_whatever_the_selection() {
        let mut menu = merging();
        let key = |menu: &mut Menu<Maybe>, code| menu.key(KeyEvent::new(code, KeyModifiers::NONE));
        assert_ne!(key(&mut menu, KeyCode::Enter), Outcome::Pick(Maybe("Merge", true)), "Enter on the busy entry, where the selection starts");
        key(&mut menu, KeyCode::Down);
        assert_eq!(key(&mut menu, KeyCode::Enter), Outcome::Pick(Maybe("Copy Link", false)), "the entry below it is picked");
        key(&mut menu, KeyCode::Up);
        assert_ne!(key(&mut menu, KeyCode::Enter), Outcome::Pick(Maybe("Merge", true)), "Enter on it again");
        assert_ne!(key(&mut menu, KeyCode::Enter), Outcome::Close, "Enter on it does not even close the menu: it does nothing");
    }

    #[test]
    fn a_click_on_a_busy_entry_does_nothing_and_the_rest_of_the_menu_works() {
        let mut menu = merging();
        let left = |x, y| mouse(MouseEventKind::Down(MouseButton::Left), x, y);
        assert_eq!(menu.mouse(PANE, left(7, 3)), Outcome::Stay, "the busy entry: not picked, not closed");
        assert_eq!(menu.click(PANE, Position::new(7, 3)), Outcome::Stay);
        assert_eq!(menu.mouse(PANE, left(7, 4)), Outcome::Pick(Maybe("Copy Link", false)));
        assert_eq!(menu.mouse(PANE, left(30, 10)), Outcome::Close, "outside closes as ever");
    }

    #[test]
    fn a_busy_entry_is_drawn_with_the_spinner_frame_beside_its_label_and_the_others_are_as_before() {
        let menu = merging();
        for frame in 0..crate::palette::SPINNER.len() {
            let mut terminal = Terminal::new(TestBackend::new(30, 8)).unwrap();
            terminal.draw(|f| menu.draw_at(f, f.area(), frame)).unwrap();
            let buf = terminal.backend().buffer();
            let rect = menu.rect(Rect::new(0, 0, 30, 8));
            let line = |y: u16| (rect.x..rect.right()).map(|x| buf[(x, y)].symbol()).collect::<String>();
            let (merge, copy) = (line(rect.y + 1), line(rect.y + 2));
            assert!(merge.starts_with('│') && merge.ends_with('│'), "frame {frame}: the box holds the whole row: {merge:?}");
            assert!(merge.contains("Merge") && merge.contains(crate::palette::SPINNER[frame]), "frame {frame}: {merge:?}");
            assert!(crate::palette::SPINNER.iter().all(|s| !copy.contains(s)), "frame {frame}: only the busy entry shows it: {copy:?}");
            assert!(copy.contains("Copy Link") && copy.starts_with('│') && copy.ends_with('│'), "frame {frame}: {copy:?}");
        }
    }
}
