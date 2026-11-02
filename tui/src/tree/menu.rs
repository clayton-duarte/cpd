//! The tree's right-click menu (commands.md §13.2): the items of a row kind and what the menu is about (the row's
//! path), on the shared box ([`crate::menu`]: the selection, ↑↓, Enter, Esc, the hit test and the
//! drawing). The view opens it on a right click and gives it every key and click while it is open; there is no key
//! that opens it. What an item does is the view's: the menu only says which one was picked.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyEvent, MouseEvent};
use ratatui::layout::{Position, Rect};

use super::rows::{Id, Row};
use crate::menu::{self, Entry};

pub use crate::menu::Outcome;

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
    Delete,
    CopyId,
}

impl Entry for Item {
    fn label(self) -> &'static str {
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
            Item::Delete => "Delete",
            Item::CopyId => "Copy id",
        }
    }
}

/// What a right click was on, which decides the items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Context,
    /// A Permanent Context's row (never renamed or archived).
    Permanent,
    Plan,
    /// A side Session of a Plan.
    Fork,
    /// A row of an archived object, or inside one.
    Archived,
    /// No row: the empty area of the tree.
    Empty,
}

impl Kind {
    /// The kind of the row clicked (`None`: the empty area); a toggle row has no menu.
    pub fn of(row: Option<Row<'_>>) -> Option<Kind> {
        match row {
            None => Some(Kind::Empty),
            Some(Row::Toggle)
            | Some(Row::NewPlan(_))
            | Some(Row::Archive)
            | Some(Row::StandingHeader) => None,
            Some(row) if row.archived() => Some(Kind::Archived),
            Some(Row::Manager(..)) => Some(Kind::Fork),
            Some(Row::Context(c)) if c.id.permanent().is_some() => Some(Kind::Permanent),
            Some(Row::Context(_)) => Some(Kind::Context),
            Some(Row::Plan(..)) => Some(Kind::Plan),
            Some(Row::Fork(..)) => Some(Kind::Fork),
        }
    }

    /// The items, in order (commands.md §13.2).
    pub fn items(self) -> &'static [Item] {
        match self {
            Kind::Context => &[Item::NewPlan, Item::Rename, Item::CopyId, Item::Archive],
            Kind::Permanent => &[Item::NewPlan, Item::CopyId],
            Kind::Plan => &[Item::NewSession, Item::Rename, Item::CopyId, Item::Archive],
            Kind::Fork => &[
                Item::Open,
                Item::Rename,
                Item::Fork,
                Item::CopyId,
                Item::Close,
                Item::Archive,
            ],
            Kind::Archived => &[Item::Rename, Item::CopyId, Item::Unarchive],
            Kind::Empty => &[Item::NewContext],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    /// The tree path of the row clicked; empty for the empty area.
    pub path: Vec<Id>,
    menu: menu::Menu<Item>,
}

impl Menu {
    /// The menu of a row of `kind`; `deletable` adds the Delete item last (an archived or asleep Plan or Context row).
    pub fn new(kind: Kind, path: Vec<Id>, at: Position, deletable: bool) -> Menu {
        let mut items = kind.items().to_vec();
        if deletable {
            items.push(Item::Delete);
        }
        Menu {
            path,
            menu: menu::Menu::new(items, at),
        }
    }

    #[cfg(test)]
    pub fn selected(&self) -> usize {
        self.menu.selected()
    }

    pub fn key(&mut self, key: KeyEvent) -> Outcome<Item> {
        self.menu.key(key)
    }

    pub fn mouse(&mut self, pane: Rect, m: MouseEvent) -> Outcome<Item> {
        self.menu.mouse(pane, m)
    }

    #[cfg(test)]
    pub fn rect(&self, pane: Rect) -> Rect {
        self.menu.rect(pane)
    }

    pub fn draw(&self, f: &mut Frame, pane: Rect) {
        self.menu.draw(f, pane);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::{KeyCode, KeyModifiers};

    use super::super::fixture::{archived, sample};
    use super::super::rows;
    use super::*;

    fn labels(kind: Kind) -> Vec<&'static str> {
        kind.items().iter().map(|i| i.label()).collect()
    }

    fn path(parts: &[&str]) -> Vec<Id> {
        parts.iter().map(|p| (*p).to_owned()).collect()
    }

    fn press(menu: &mut Menu, code: KeyCode) -> Outcome<Item> {
        menu.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn each_kind_has_the_items_of_the_contract() {
        assert_eq!(
            labels(Kind::Context),
            ["New Plan…", "Rename…", "Copy id", "Archive"]
        );
        assert_eq!(
            labels(Kind::Plan),
            ["New session…", "Rename…", "Copy id", "Archive"]
        );
        assert_eq!(
            labels(Kind::Fork),
            ["Open", "Rename…", "Fork…", "Copy id", "Close", "Archive"]
        );
        assert_eq!(labels(Kind::Empty), ["New Context…"]);
        assert_eq!(labels(Kind::Archived), ["Rename…", "Copy id", "Unarchive"]);
    }

    #[test]
    fn the_kind_of_a_row() {
        let tree = sample();
        let kind = |p: &[&str]| Kind::of(rows::row(&tree, &path(p)));
        assert_eq!(kind(&["alpha"]), Some(Kind::Context));
        assert_eq!(kind(&["alpha", "ship"]), Some(Kind::Plan));
        assert_eq!(kind(&["alpha", "f-idle"]), Some(Kind::Fork));
        assert_eq!(Kind::of(None), Some(Kind::Empty));
        let tree = archived();
        let kind = |p: &[&str]| Kind::of(rows::row(&tree, &path(p)));
        assert_eq!(kind(&["╰─ alpha"]), None, "a toggle row has no menu");
        assert_eq!(
            kind(&["alpha", "old"]),
            Some(Kind::Archived),
            "an archived Plan"
        );
        assert_eq!(
            kind(&["alpha", "f-blocked"]),
            Some(Kind::Archived),
            "an archived fork of a live Plan"
        );
        assert_eq!(kind(&["zeta"]), Some(Kind::Archived), "an archived Context");
        assert_eq!(kind(&["zeta", "zp"]), Some(Kind::Archived));
        assert_eq!(kind(&["zeta", "f-zp"]), Some(Kind::Archived));
        assert_eq!(
            kind(&["alpha", "ship"]),
            Some(Kind::Plan),
            "a live row of a tree with archived ones"
        );
    }

    #[test]
    fn delete_is_offered_last_on_an_archived_or_asleep_plan_or_context_row_only() {
        let deletable = |tree: &crate::read::Tree, p: &[&str]| {
            rows::row(tree, &path(p)).is_some_and(|r| r.deletable())
        };
        let tree = sample();
        assert!(
            !deletable(&tree, &["alpha"])
                && !deletable(&tree, &["alpha", "ship"])
                && !deletable(&tree, &["alpha", "f-idle"])
        );
        let tree = archived();
        assert!(
            deletable(&tree, &["alpha", "old"])
                && deletable(&tree, &["zeta"])
                && deletable(&tree, &["zeta", "zp"])
        );
        assert!(
            !deletable(&tree, &["alpha", "f-blocked"])
                && !deletable(&tree, &["zeta", "f-zp"])
                && !deletable(&tree, &["alpha", "ship"])
        );
        // the last item: the selection is clamped, so many Downs reach it
        let last = |kind: Kind, deletable: bool| {
            let mut menu = Menu::new(kind, Vec::new(), Position::new(0, 0), deletable);
            for _ in 0..9 {
                press(&mut menu, KeyCode::Down);
            }
            press(&mut menu, KeyCode::Enter)
        };
        assert_eq!(last(Kind::Archived, true), Outcome::Pick(Item::Delete));
        assert_eq!(last(Kind::Plan, true), Outcome::Pick(Item::Delete));
        assert_eq!(last(Kind::Plan, false), Outcome::Pick(Item::Archive));
    }

    #[test]
    fn arrows_move_the_selection_clamped_and_enter_picks() {
        let mut menu = Menu::new(
            Kind::Fork,
            path(&["a", "b", "c"]),
            Position::new(3, 3),
            false,
        );
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
        assert_eq!(menu.selected(), 5);
        assert_eq!(
            press(&mut menu, KeyCode::Enter),
            Outcome::Pick(Item::Archive)
        );
        assert_eq!(press(&mut menu, KeyCode::Up), Outcome::Stay);
        assert_eq!(press(&mut menu, KeyCode::Enter), Outcome::Pick(Item::Close));
    }
}
