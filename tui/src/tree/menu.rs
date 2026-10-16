//! The tree's right-click menu (commands.md §13.2): a pure model (the items of a row kind, the selection, ↑↓,
//! Enter, Esc, the hit test) and its drawing, a bordered box over the tree at the click, clamped inside the
//! pane. The view opens it on a right click and gives it every key and click while it is open; there is no key
//! that opens it. What an item does is the view's: the menu only says which one was picked.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Position, Rect};
use ratatui::style::Modifier;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use super::rows::{Id, Row};

/// One item of a menu. The labels are the contract's, exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    NewPlan,
    Rename,
    Archive,
    NewSession,
    Open,
    Fork,
    Close,
    NewContext,
    Unarchive,
}

impl Item {
    pub fn label(self) -> &'static str {
        match self {
            Item::NewPlan => "New Plan…",
            Item::Rename => "Rename…",
            Item::Archive => "Archive",
            Item::NewSession => "New session…",
            Item::Open => "Open",
            Item::Fork => "Fork…",
            Item::Close => "Close",
            Item::NewContext => "New Context…",
            Item::Unarchive => "Unarchive",
        }
    }

    /// The item asks for typed text first (the `…` ones).
    pub fn needs_text(self) -> bool {
        matches!(self, Item::NewPlan | Item::Rename | Item::NewSession | Item::Fork | Item::NewContext)
    }
}

/// What a right click was on, which decides the items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Context,
    Plan,
    /// A side Session of a Plan.
    Fork,
    /// A row of an archived object, or inside one.
    Archived,
    /// No row: the empty area of the tree.
    Empty,
}

impl Kind {
    /// The kind of the row clicked (`None`: the empty area); a `…` row has no menu.
    pub fn of(row: Option<Row<'_>>) -> Option<Kind> {
        match row {
            None => Some(Kind::Empty),
            Some(Row::More) => None,
            Some(row) if row.archived() => Some(Kind::Archived),
            Some(Row::Context(_)) => Some(Kind::Context),
            Some(Row::Plan(..)) => Some(Kind::Plan),
            Some(Row::Fork(..)) => Some(Kind::Fork),
        }
    }

    /// The items, in order (commands.md §13.2).
    pub fn items(self) -> &'static [Item] {
        match self {
            Kind::Context => &[Item::NewPlan, Item::Rename, Item::Archive],
            Kind::Plan => &[Item::NewSession, Item::Rename, Item::Archive],
            Kind::Fork => &[Item::Open, Item::Rename, Item::Fork, Item::Close, Item::Archive],
            Kind::Archived => &[Item::Unarchive],
            Kind::Empty => &[Item::NewContext],
        }
    }
}

/// What a key or a click did to the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The menu stays open.
    Stay,
    /// The menu is dismissed; nothing happens.
    Close,
    /// The menu is dismissed and this item was picked.
    Pick(Item),
}

/// Where a click fell, relative to the drawn menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Item(usize),
    /// On the box, but on no item: its border.
    Border,
    Outside,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    /// The tree path of the row clicked; empty for the empty area.
    pub path: Vec<Id>,
    at: Position,
    items: &'static [Item],
    selected: usize,
}

impl Menu {
    pub fn new(kind: Kind, path: Vec<Id>, at: Position) -> Menu {
        Menu { path, at, items: kind.items(), selected: 0 }
    }

    #[cfg(test)]
    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn key(&mut self, key: KeyEvent) -> Outcome {
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

    /// The box inside `pane`: one border, one blank and the label per item, at the click, moved left and up as far as
    /// it takes to fit (and cut, when the pane is smaller than the box).
    pub fn rect(&self, pane: Rect) -> Rect {
        let widest = self.items.iter().map(|i| i.label().chars().count()).max().unwrap_or(0) as u16;
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

    /// A left click: an item picks it, the border does nothing, outside closes the menu.
    pub fn click(&mut self, pane: Rect, at: Position) -> Outcome {
        match self.hit(pane, at) {
            Hit::Item(i) => {
                self.selected = i;
                Outcome::Pick(self.items[i])
            }
            Hit::Border => Outcome::Stay,
            Hit::Outside => Outcome::Close,
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

    use super::super::fixture::{archived, sample};
    use super::super::rows;
    use super::*;

    fn labels(kind: Kind) -> Vec<&'static str> {
        kind.items().iter().map(|i| i.label()).collect()
    }

    fn path(parts: &[&str]) -> Vec<Id> {
        parts.iter().map(|p| (*p).to_owned()).collect()
    }

    fn press(menu: &mut Menu, code: KeyCode) -> Outcome {
        menu.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    const PANE: Rect = Rect::new(0, 0, 44, 12);

    #[test]
    fn each_kind_has_the_items_of_the_contract() {
        assert_eq!(labels(Kind::Context), ["New Plan…", "Rename…", "Archive"]);
        assert_eq!(labels(Kind::Plan), ["New session…", "Rename…", "Archive"]);
        assert_eq!(labels(Kind::Fork), ["Open", "Rename…", "Fork…", "Close", "Archive"]);
        assert_eq!(labels(Kind::Empty), ["New Context…"]);
        assert_eq!(labels(Kind::Archived), ["Unarchive"]);
    }

    #[test]
    fn the_items_that_need_text_are_the_ones_ending_in_an_ellipsis() {
        for item in [Item::NewPlan, Item::Rename, Item::NewSession, Item::Fork, Item::NewContext, Item::Open, Item::Close, Item::Archive, Item::Unarchive] {
            assert_eq!(item.needs_text(), item.label().ends_with('…'), "{item:?}");
        }
    }

    #[test]
    fn the_kind_of_a_row() {
        let tree = sample();
        let kind = |p: &[&str]| Kind::of(rows::row(&tree, &path(p)));
        assert_eq!(kind(&["alpha"]), Some(Kind::Context));
        assert_eq!(kind(&["alpha", "ship"]), Some(Kind::Plan));
        assert_eq!(kind(&["alpha", "ship", "f-idle"]), Some(Kind::Fork));
        assert_eq!(Kind::of(None), Some(Kind::Empty));
        let tree = archived();
        let kind = |p: &[&str]| Kind::of(rows::row(&tree, &path(p)));
        assert_eq!(kind(&["alpha", "…"]), None, "a `…` row has no menu");
        assert_eq!(kind(&["…"]), None);
        assert_eq!(kind(&["alpha", "…", "old"]), Some(Kind::Archived), "an archived Plan");
        assert_eq!(kind(&["alpha", "…", "fork:f-blocked"]), Some(Kind::Archived), "an archived fork of a live Plan");
        assert_eq!(kind(&["…", "zeta"]), Some(Kind::Archived), "an archived Context");
        assert_eq!(kind(&["…", "zeta", "zp"]), Some(Kind::Archived));
        assert_eq!(kind(&["…", "zeta", "zp", "f-zp"]), Some(Kind::Archived));
        assert_eq!(kind(&["alpha", "ship"]), Some(Kind::Plan), "a live row of a tree with archived ones");
    }

    #[test]
    fn arrows_move_the_selection_clamped_and_enter_picks() {
        let mut menu = Menu::new(Kind::Fork, path(&["a", "b", "c"]), Position::new(3, 3));
        assert_eq!(menu.selected(), 0);
        assert_eq!(press(&mut menu, KeyCode::Up), Outcome::Stay);
        assert_eq!(menu.selected(), 0);
        assert_eq!(press(&mut menu, KeyCode::Down), Outcome::Stay);
        assert_eq!(press(&mut menu, KeyCode::Down), Outcome::Stay);
        assert_eq!(menu.selected(), 2);
        assert_eq!(press(&mut menu, KeyCode::Enter), Outcome::Pick(Item::Fork));
        for _ in 0..9 {
            press(&mut menu, KeyCode::Down);
        }
        assert_eq!(menu.selected(), 4);
        assert_eq!(press(&mut menu, KeyCode::Enter), Outcome::Pick(Item::Archive));
        assert_eq!(press(&mut menu, KeyCode::Up), Outcome::Stay);
        assert_eq!(press(&mut menu, KeyCode::Enter), Outcome::Pick(Item::Close));
    }

    #[test]
    fn esc_closes_and_no_other_key_does_anything() {
        let mut menu = Menu::new(Kind::Context, vec![], Position::new(0, 0));
        assert_eq!(press(&mut menu, KeyCode::Esc), Outcome::Close);
        for key in [KeyCode::Char('j'), KeyCode::Char('n'), KeyCode::Char('x'), KeyCode::Char('?'), KeyCode::Char('q'), KeyCode::Tab] {
            assert_eq!(press(&mut menu, key), Outcome::Stay, "{key:?}");
        }
        assert_eq!(menu.selected(), 0);
    }

    #[test]
    fn the_box_is_at_the_click_and_clamped_inside_the_pane() {
        let menu = |x, y| Menu::new(Kind::Context, vec![], Position::new(x, y)); // 3 items, widest `New Plan…` (9)
        assert_eq!(menu(5, 2).rect(PANE), Rect::new(5, 2, 13, 5));
        assert_eq!(menu(40, 2).rect(PANE), Rect::new(31, 2, 13, 5), "moved left");
        assert_eq!(menu(5, 10).rect(PANE), Rect::new(5, 7, 13, 5), "moved up");
        assert_eq!(menu(43, 11).rect(PANE), Rect::new(31, 7, 13, 5));
        let pane = Rect::new(2, 3, 8, 4);
        assert_eq!(menu(0, 0).rect(pane), Rect::new(2, 3, 8, 4), "cut to a pane smaller than the box");
        assert_eq!(menu(9, 9).rect(pane), Rect::new(2, 3, 8, 4));
    }

    #[test]
    fn the_hit_test_tells_items_border_and_outside() {
        let menu = Menu::new(Kind::Fork, vec![], Position::new(5, 2)); // widest `Rename…` (7): x 5..=15, y 2..=8
        let hit = |x, y| menu.hit(PANE, Position::new(x, y));
        assert_eq!(hit(6, 3), Hit::Item(0));
        assert_eq!(hit(10, 3), Hit::Item(0));
        assert_eq!(hit(9, 7), Hit::Item(4));
        for (x, y) in [(5, 2), (10, 2), (5, 5), (15, 5), (10, 8), (15, 8)] {
            assert_eq!(hit(x, y), Hit::Border, "({x}, {y})");
        }
        for (x, y) in [(4, 3), (16, 3), (6, 1), (6, 9), (0, 0), (43, 11)] {
            assert_eq!(hit(x, y), Hit::Outside, "({x}, {y})");
        }
        assert_eq!(hit(14, 3), Hit::Item(0), "the last column inside");
    }

    #[test]
    fn a_click_picks_an_item_ignores_the_border_and_closes_outside() {
        let mut menu = Menu::new(Kind::Fork, vec![], Position::new(5, 2));
        assert_eq!(menu.click(PANE, Position::new(7, 6)), Outcome::Pick(Item::Close));
        assert_eq!(menu.selected(), 3);
        assert_eq!(menu.click(PANE, Position::new(7, 2)), Outcome::Stay);
        assert_eq!(menu.click(PANE, Position::new(30, 10)), Outcome::Close);
    }

    #[test]
    fn the_menu_is_drawn_as_a_bordered_box_over_what_is_there() {
        let mut terminal = Terminal::new(TestBackend::new(20, 6)).unwrap();
        let menu = Menu::new(Kind::Context, vec![], Position::new(3, 1));
        terminal
            .draw(|f| {
                f.render_widget(Paragraph::new("xxxxxxxxxxxxxxxxxxxx\n".repeat(6)), f.area());
                menu.draw(f, f.area());
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let rows: Vec<String> = (0..6).map(|y| (0..20).map(|x| buf[(x, y)].symbol()).collect()).collect();
        assert_eq!(
            rows,
            ["xxxxxxxxxxxxxxxxxxxx", "xxx┌───────────┐xxxx", "xxx│ New Plan… │xxxx", "xxx│ Rename…   │xxxx", "xxx│ Archive   │xxxx", "xxx└───────────┘xxxx"]
                .map(|r| r.chars().take(20).collect::<String>()),
        );
        assert!(buf[(5, 2)].modifier.contains(Modifier::REVERSED), "the selected item");
        assert!(!buf[(5, 3)].modifier.contains(Modifier::REVERSED));
    }
}
