//! The tree view's state and keys (the reference's `run_context`, without the
//! terminal): what a key, a click or a focus change does, and how the view is
//! drawn. Pure: an action to run is returned as an [`Effect`], and the caller
//! runs it, then hands the result to [`View::done`].
//!
//! Keys (commands.md §13.2): ↑↓ and j/k move the cursor, Enter opens the cursor's Session, `n` starts a new Plan,
//! `x` closes a Session's pane, `?` opens the legend. After the last Session the cursor goes on into the Inbox
//! section's item rows (and back up); there Enter jumps to the item's cause, `s` opens the snooze choices and `d`
//! dismisses. No other key does anything. A right click opens the menu
//! ([`super::menu`]), which then takes every key and click.

use std::collections::HashSet;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::widgets::{Paragraph, Wrap};
use time::OffsetDateTime;
use tui_tree_widget::TreeState;

use super::actions::{Action, Free, IN_CHAT_SLOT, Subject, Until};
use super::cursor::{self, Input};
use super::inbox::{self, Hit, Jump, Shape};
use super::menu::{self, Item, Kind, Menu};
use super::prompt::{Ask, Outcome, Prompt, Target};
use super::rows::{self, Id, Row};
use cpd_core::inbox::model::Kind as InboxKind;
use cpd_core::session::State;
use crate::palette;
use crate::read::{ContextNode, SessionNode, Tree};

/// The longest name of a Session (commands.md §1.4).
const NAME_MAX: usize = 80;
const FORK_SUFFIX: &str = " (fork)";

/// The prompt of Fork… and New session…: the name of a fork of `source`, starting with `<label> (fork)` (the label cut
/// so that the name is not too long).
fn fork_of(source: &SessionNode, label: &str) -> Prompt {
    let label: String = label.chars().take(NAME_MAX - FORK_SUFFIX.len()).collect();
    Prompt::typed(Ask::Fork { source: source.id.clone() }, format!("{label}{FORK_SUFFIX}"))
}

/// The prompt of Rename… on `row`, starting with its current label.
fn rename_of(row: &Row<'_>) -> Option<Prompt> {
    let ask = match *row {
        Row::Context(c) => Ask::RenameContext(c.id.clone()),
        Row::Plan(_, p) => Ask::RenamePlan(p.head.id.clone()),
        Row::Fork(_, _, s) => Ask::RenameSession(s.id.clone()),
        Row::More => return None,
    };
    Some(Prompt::typed(ask, row.label()))
}

/// What `x` and the menu's Close do on `row`: the confirmation, or the note that refuses it (the Session is in the
/// chat slot); nothing on a row with no Session.
fn close_of(row: Row<'_>) -> Option<Result<Prompt, &'static str>> {
    let session = row.session()?;
    if session.is_active() {
        return Some(Err(IN_CHAT_SLOT));
    }
    let target = Target { id: session.id.clone(), label: row.label(), working: session.state == State::Working };
    Some(Ok(Prompt::close(target)))
}

/// The object an archive command of the row names: the Context, the Plan, or the fork; none for a `…` row.
fn subject_of(row: &Row<'_>) -> Option<Subject> {
    match *row {
        Row::Context(c) => Some(Subject::Context(c.id.clone())),
        Row::Plan(_, p) => Some(Subject::Plan(p.head.id.clone())),
        Row::Fork(_, _, s) => Some(Subject::Session(s.id.clone())),
        Row::More => None,
    }
}

/// What the caller does after an input.
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Run this command, then call [`View::done`] with the result.
    Run(Action),
    /// Open the legend popup (`?`); a failure is the note.
    Legend,
    /// Open the logs popup at this Event (the bottom with none): a click on an `engine-error` or `engine-down` item.
    Logs(Option<i64>),
}

pub struct View {
    pub tree: Tree,
    pub state: TreeState<Id>,
    known: HashSet<Vec<Id>>,
    /// Keyboard focus of this pane, from the terminal's focus events; the cursor shows only while focused.
    focused: bool,
    /// The cursor stays hidden until the first arrow key.
    last_input: Input,
    prompt: Option<Prompt>,
    /// The right-click menu, while it is open; never together with a prompt.
    menu: Option<Menu>,
    /// The inner area of the last frame (the pane less a blank column on each side, below the blank top row): the
    /// menu is clamped inside it and hit-tested against it, and its `x` is the column of the tree's first cell.
    pane: Rect,
    /// The Inbox section's rows (header, items, `+<n> more`) are all shown, not only the first three. View state only.
    inbox_expanded: bool,
    /// Where the last frame drew the Inbox section (empty when it drew none): clicks there are the section's.
    inbox_area: Rect,
    /// The item row of the Inbox section the keyboard cursor is on (an index of the snapshot's Inbox, a shown one):
    /// the tree's cursor then rests on its last Session. Hidden like the tree's cursor ([`cursor::shown`]).
    inbox_cursor: Option<usize>,
    /// The one-line result or refusal of the last action, until the next `n`, `x` or click.
    pub note: String,
}

impl View {
    pub fn new(tree: Tree, focused: bool) -> View {
        let mut view = View {
            tree: Tree { contexts: Vec::new(), events_cursor: 0, inbox: Vec::new(), engine_running: true },
            state: TreeState::default(),
            known: HashSet::new(),
            focused,
            last_input: Input::Click,
            prompt: None,
            menu: None,
            pane: Rect::default(),
            inbox_expanded: false,
            inbox_area: Rect::default(),
            inbox_cursor: None,
            note: String::new(),
        };
        view.refresh(tree);
        view
    }

    /// A new snapshot: keep the cursor on its Session, or on its Inbox item (the item now at the same position when
    /// it is gone).
    pub fn refresh(&mut self, tree: Tree) {
        let before = rows::selected_index(&self.state, &self.tree);
        let item = self.inbox_cursor.and_then(|i| self.tree.inbox.get(i)).map(|item| item.id);
        self.tree = tree;
        rows::open_new(&mut self.state, &mut self.known, &self.tree);
        rows::reselect(&mut self.state, &self.tree, before);
        if let Some(i) = self.inbox_cursor {
            let moved = item.and_then(|id| self.tree.inbox.iter().position(|it| it.id == id));
            self.inbox_cursor = Some(moved.unwrap_or(i));
            self.clamp_inbox_cursor();
        }
    }

    /// The Inbox cursor stays on a shown item row; with none, it goes.
    fn clamp_inbox_cursor(&mut self) {
        let shown = Shape::new(self.tree.inbox.len(), self.inbox_expanded).shown();
        self.inbox_cursor = self.inbox_cursor.and_then(|i| shown.checked_sub(1).map(|last| i.min(last)));
    }

    fn cursor_shown(&self) -> bool {
        cursor::shown(self.focused, self.last_input, !self.state.selected().is_empty() || self.inbox_cursor.is_some())
    }

    /// The Inbox item under the keyboard cursor, while it shows.
    fn cursor_item(&self) -> Option<&cpd_core::inbox::model::Item> {
        self.inbox_cursor.filter(|_| self.cursor_shown()).and_then(|i| self.tree.inbox.get(i))
    }

    pub fn focus(&mut self, gained: bool) {
        self.focused = gained;
        if !gained {
            self.last_input = Input::Click;
        }
    }

    /// The result of an [`Effect::Run`] or [`Effect::Legend`].
    pub fn done(&mut self, result: Result<String, String>) {
        self.note = result.unwrap_or_else(|e| e);
    }

    /// The cursor's row, while the cursor shows: Enter, `n` and `x` act on it.
    fn cursor_row(&self) -> Option<Row<'_>> {
        rows::row(&self.tree, self.state.selected()).filter(|r| self.cursor_shown() && self.inbox_cursor.is_none() && !r.archived())
    }

    pub fn key(&mut self, key: KeyEvent) -> Effect {
        if key.kind != KeyEventKind::Press {
            return Effect::None;
        }
        // An open prompt takes every key; mouse clicks are ignored meanwhile.
        if let Some(prompt) = self.prompt.as_mut() {
            return match prompt.key(key) {
                Outcome::Open => Effect::None,
                Outcome::Cancel => {
                    self.prompt = None;
                    Effect::None
                }
                Outcome::NewPlan { context, id } => {
                    self.prompt = None;
                    let title = id.to_string();
                    Effect::Run(Action::NewPlan { context, id, title })
                }
                Outcome::Run(action) => {
                    self.prompt = None;
                    Effect::Run(action)
                }
                Outcome::Close { id } => {
                    self.prompt = None;
                    Effect::Run(Action::Close(id))
                }
            };
        }
        // An open menu takes every key.
        if let Some(menu) = self.menu.as_mut() {
            return match menu.key(key) {
                menu::Outcome::Stay => Effect::None,
                menu::Outcome::Close => {
                    self.menu = None;
                    Effect::None
                }
                menu::Outcome::Pick(item) => self.pick(item),
            };
        }
        match key.code {
            KeyCode::Char('n') => {
                self.note.clear();
                if let Some((row, session, plan)) = self.cursor_row().and_then(|r| Some((r, r.session()?, r.plan()?))) {
                    let target = Target { id: session.id.clone(), label: row.label(), working: session.state == State::Working };
                    self.prompt = Some(Prompt::name(target, plan.head.context.clone()));
                }
            }
            KeyCode::Char('x') => {
                self.note.clear();
                let asked = self.cursor_row().and_then(close_of);
                self.ask(asked);
            }
            KeyCode::Char('s') => {
                self.note.clear();
                if let Some(item) = self.cursor_item().filter(|i| i.kind != InboxKind::EngineDown) {
                    let at = Position::new(self.inbox_area.x + 2, self.inbox_area.y + 1 + self.inbox_cursor.unwrap_or(0) as u16);
                    self.menu = Some(Menu::for_item(Kind::Snooze, item.id, at));
                }
            }
            KeyCode::Char('d') => {
                self.note.clear();
                if let Some(item) = self.cursor_item().filter(|i| i.kind != InboxKind::EngineDown) {
                    return Effect::Run(Action::Dismiss(item.id));
                }
            }
            KeyCode::Char('?') => return Effect::Legend,
            // the first arrow after a click only shows the cursor again where it was (the first Session when there is none)
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Up | KeyCode::Char('k') => {
                let delta = if matches!(key.code, KeyCode::Down | KeyCode::Char('j')) { 1 } else { -1 };
                self.arrow(delta);
                self.last_input = Input::Arrow;
            }
            KeyCode::Enter => {
                if let Some(i) = self.inbox_cursor.filter(|_| self.cursor_shown()) {
                    return self.jump(i);
                }
                if let Some(session) = self.cursor_row().and_then(|r| r.session()) {
                    return Effect::Run(Action::Open(session.id.clone()));
                }
            }
            _ => {}
        }
        Effect::None
    }

    /// An arrow key (`delta` 1 down, -1 up) while the cursor shows: it moves through the Sessions, on from the last
    /// into the Inbox's shown items and back up from the first to the last Session; when it does not show, the key
    /// only shows it again where it was (on the first Session when it has none).
    fn arrow(&mut self, delta: isize) {
        let shown = Shape::new(self.tree.inbox.len(), self.inbox_expanded).shown();
        if let Some(i) = self.inbox_cursor {
            if self.cursor_shown() {
                self.inbox_cursor = match (delta, i) {
                    (d, 0) if d < 0 => None,
                    (d, i) if d < 0 => Some(i - 1),
                    (_, i) => Some((i + 1).min(shown.saturating_sub(1))),
                };
            }
        } else if self.cursor_shown() {
            let before = rows::selected_index(&self.state, &self.tree);
            rows::move_cursor(&mut self.state, &self.tree, delta);
            // it did not move: the last Session, so down goes into the Inbox
            if delta > 0 && shown > 0 && rows::selected_index(&self.state, &self.tree) == before {
                self.inbox_cursor = Some(0);
            }
        } else if self.state.selected().is_empty() {
            rows::move_cursor(&mut self.state, &self.tree, 1);
            // no Session at all: the Inbox is all there is to reach
            if delta > 0 && shown > 0 && self.state.selected().is_empty() {
                self.inbox_cursor = Some(0);
            }
        }
    }

    pub fn mouse(&mut self, m: MouseEvent) -> Effect {
        if self.prompt.is_some() {
            return Effect::None;
        }
        let at = Position::new(m.column, m.row);
        // a right click on the Inbox section: the menu of an item row; the header and `+<n> more` have none
        if self.menu.is_none() && m.kind == MouseEventKind::Down(MouseButton::Right) && self.inbox_area.contains(at) {
            self.note.clear();
            if let Some(Hit::Item(i)) = Shape::new(self.tree.inbox.len(), self.inbox_expanded).hit(at.y - self.inbox_area.y) {
                let item = &self.tree.inbox[i];
                if item.kind != InboxKind::EngineDown {
                    self.menu = Some(Menu::for_item(Kind::Inbox, item.id, at));
                }
            }
            return Effect::None;
        }
        if let Some(menu) = self.menu.as_mut() {
            // an open menu takes every click: a left click picks, any button outside closes
            let MouseEventKind::Down(button) = m.kind else { return Effect::None };
            return match menu.click(self.pane, at) {
                menu::Outcome::Pick(_) if button != MouseButton::Left => Effect::None,
                menu::Outcome::Pick(item) => self.pick(item),
                menu::Outcome::Close => {
                    self.menu = None;
                    Effect::None
                }
                menu::Outcome::Stay => Effect::None,
            };
        }
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => self.click(m),
            MouseEventKind::Down(MouseButton::Right) => {
                self.open_menu(at);
                Effect::None
            }
            _ => Effect::None,
        }
    }

    /// A right click at `at`: the menu of the row there, of the empty area, or none (a `…` row).
    fn open_menu(&mut self, at: Position) {
        self.note.clear();
        let path = self.row_at(at);
        let row = path.as_deref().and_then(|p| rows::row(&self.tree, p));
        self.menu = Kind::of(row).map(|kind| Menu::new(kind, path.unwrap_or_default(), at));
    }

    /// The item picked in the menu, which is closed: the command to run, a prompt to ask first, or the note.
    fn pick(&mut self, item: Item) -> Effect {
        let Some(menu) = self.menu.take() else { return Effect::None };
        self.note.clear();
        if let Some(id) = menu.inbox {
            return match item {
                Item::Snooze15 => Effect::Run(Action::Snooze { item: id, until: Until::Minutes15 }),
                Item::Snooze1h => Effect::Run(Action::Snooze { item: id, until: Until::Hour1 }),
                Item::SnoozeTomorrow => Effect::Run(Action::Snooze { item: id, until: Until::Tomorrow }),
                Item::Dismiss => Effect::Run(Action::Dismiss(id)),
                _ => Effect::None,
            };
        }
        if item == Item::NewContext {
            let taken = self.tree.contexts.iter().map(|c| c.id.to_string()).collect();
            self.prompt = Some(Prompt::typed(Ask::NewContext { taken }, String::new()));
            return Effect::None;
        }
        // the row may be gone, with a snapshot taken while the menu was open
        let Some(row) = rows::row(&self.tree, &menu.path) else { return Effect::None };
        match item {
            Item::Open => row.session().map_or(Effect::None, |s| Effect::Run(Action::Open(s.id.clone()))),
            Item::Close => {
                self.ask(close_of(row));
                Effect::None
            }
            Item::Archive => {
                let Some(subject) = subject_of(&row) else { return Effect::None };
                match self.free_for(&row, &menu.path) {
                    Some(free) => Effect::Run(Action::Archive { subject, free }),
                    None => Effect::None,
                }
            }
            Item::Unarchive => subject_of(&row).map_or(Effect::None, |subject| Effect::Run(Action::Unarchive(subject))),
            Item::Rename => {
                self.ask(rename_of(&row).map(Ok));
                Effect::None
            }
            Item::Fork | Item::NewSession => {
                let prompt = row.session().map(|source| fork_of(source, &row.label()));
                self.ask(prompt.map(Ok));
                Effect::None
            }
            Item::NewPlan => {
                let prompt = match row {
                    Row::Context(c) => Some(self.new_plan_in(c)),
                    _ => None,
                };
                self.ask(prompt.map(Ok));
                Effect::None
            }
            Item::CopyId => row.shown_id().map_or(Effect::None, |id| Effect::Run(Action::CopyId(id))),
            // an Inbox item's entries are the menu's `inbox` branch above
            Item::NewContext | Item::Snooze15 | Item::Snooze1h | Item::SnoozeTomorrow | Item::Dismiss => Effect::None,
        }
    }

    /// How the archive of `row` (drawn at `path`) frees the chat slot (F3, §13.2): `Some(None)` when none of the
    /// Sessions it archives holds it. Else the next live Session in tree order (the rows as drawn, the archived ones
    /// and the `…` rows skipped, and those the archive closes), else the previous one, else a park. `None` when the
    /// row is not drawn (a snapshot took it away while the menu was open).
    fn free_for(&self, row: &Row<'_>, path: &[Id]) -> Option<Option<Free>> {
        let archiving: Vec<&SessionNode> = match *row {
            Row::Context(c) => c.plans.iter().flat_map(|p| &p.sessions).collect(),
            Row::Plan(_, p) => p.sessions.iter().collect(),
            Row::Fork(_, _, s) => vec![s],
            Row::More => return Some(None),
        };
        if !archiving.iter().any(|s| s.is_active()) {
            return Some(None);
        }
        let items = rows::items(&self.tree, self.state.opened(), 0, 0);
        let drawn = self.state.flatten(&items);
        let anchor = drawn.iter().position(|f| f.identifier == path)?;
        let live: Vec<(usize, &SessionNode)> = drawn
            .iter()
            .enumerate()
            .filter_map(|(i, f)| {
                let row = rows::row(&self.tree, &f.identifier).filter(|r| !r.archived())?;
                let session = row.session().filter(|s| !archiving.iter().any(|a| a.id == s.id))?;
                Some((i, session))
            })
            .collect();
        let next = live.iter().find(|(i, _)| *i > anchor).or_else(|| live.iter().rev().find(|(i, _)| *i < anchor));
        Some(Some(next.map_or(Free::Park, |(_, s)| Free::Open(s.id.clone()))))
    }

    /// The prompt of New Plan… in `context` (R34 A12: no repo).
    fn new_plan_in(&self, context: &ContextNode) -> Prompt {
        let taken = self.tree.contexts.iter().flat_map(|c| &c.plans).map(|p| p.head.id.to_string()).collect();
        Prompt::typed(Ask::NewPlan { context: context.id.clone(), taken }, String::new())
    }

    /// Opens the prompt, or says why not.
    fn ask(&mut self, asked: Option<Result<Prompt, &'static str>>) {
        match asked {
            Some(Ok(prompt)) => self.prompt = Some(prompt),
            Some(Err(note)) => self.note = note.into(),
            None => {}
        }
    }

    /// A left click.
    fn click(&mut self, m: MouseEvent) -> Effect {
        self.note.clear();
        self.last_input = Input::Click;
        let at = Position::new(m.column, m.row);
        if self.inbox_area.contains(at) {
            return self.click_inbox(at.y - self.inbox_area.y);
        }
        let Some(path) = self.row_at(at) else { return Effect::None };
        // a `…` row expands or collapses its archived rows
        if rows::click_more(&mut self.state, &self.tree, &path) {
            return Effect::None;
        }
        // a Context row: its fold glyph folds or unfolds it, the rest of a live one opens its Manager (B3-A6); a live
        // Session row opens it; an archived one shows its history
        if let Some(Row::Context(c)) = rows::row(&self.tree, &path) {
            if m.column == self.pane.x + rows::FOLD_COLUMN {
                rows::click_context(&mut self.state, &self.tree, &path);
                return Effect::None;
            }
            return if c.is_archived() { Effect::None } else { Effect::Run(Action::OpenManager(c.id.clone())) };
        }
        match rows::row(&self.tree, &path) {
            Some(row) => match row.session() {
                Some(s) if row.archived() => Effect::Run(Action::History(s.id.clone())),
                Some(s) => Effect::Run(Action::Open(s.id.clone())),
                None => Effect::None,
            },
            None => Effect::None,
        }
    }

    /// A left click on the Inbox section, `row` rows below its header: `+<n> more` and `− less` toggle the inline
    /// expansion; an item jumps to its cause ([`inbox::jump`]); the header does nothing.
    fn click_inbox(&mut self, row: u16) -> Effect {
        match Shape::new(self.tree.inbox.len(), self.inbox_expanded).hit(row) {
            Some(Hit::Toggle) => {
                self.inbox_expanded = !self.inbox_expanded;
                self.clamp_inbox_cursor();
                Effect::None
            }
            Some(Hit::Item(i)) => self.jump(i),
            Some(Hit::Header) | None => Effect::None,
        }
    }

    /// The jump of the Inbox item `i` ([`inbox::jump`]): a click on its row, or Enter on it.
    fn jump(&self, i: usize) -> Effect {
        match inbox::jump(&self.tree, &self.tree.inbox[i]) {
            Some(Jump::Run(action)) => Effect::Run(action),
            Some(Jump::Logs(event)) => Effect::Logs(event),
            None => Effect::None,
        }
    }

    /// The row drawn at `at`, none below the last row (the tree state answers with the last row there).
    fn row_at(&self, at: Position) -> Option<Vec<Id>> {
        let path = self.state.rendered_at(at)?.to_vec();
        let above = at.y.checked_sub(1).and_then(|y| self.state.rendered_at(Position::new(at.x, y)));
        (above != Some(path.as_slice())).then_some(path)
    }

    /// The text of the footer: the prompt, else the note of the last action.
    fn footer(&self) -> String {
        self.prompt.as_ref().map(Prompt::text).unwrap_or_else(|| self.note.clone())
    }

    /// [`View::draw_at`] now.
    pub fn draw(&mut self, f: &mut Frame, frame: usize) {
        self.draw_at(f, frame, OffsetDateTime::now_utc());
    }

    /// Draw a blank row, the tree below it, the Inbox section pinned under the tree and (when there is something to
    /// say) the footer in the rows at the bottom: the reference's layout, the prompt bold. One blank column is left on
    /// each side of all of it. `frame` is the animation frame of the working dots (`palette::frame`); the Inbox items'
    /// ages are taken at `now`, from their `opened_at`, so they need no read.
    pub fn draw_at(&mut self, f: &mut Frame, frame: usize, now: OffsetDateTime) {
        let inner = f.area().inner(Margin::new(1, 0));
        let footer = self.footer();
        let width = inner.width.max(1) as usize;
        let wrapped = footer.chars().count().div_ceil(width);
        let footer_height =
            if footer.is_empty() { 0 } else { (wrapped + usize::from(wrapped > 1)).max(1).min(inner.height as usize) as u16 };
        // the section leaves the tree at least one row, when there is one to leave
        let room = inner.height.saturating_sub(1 + footer_height).saturating_sub(1);
        let inbox_height = Shape::new(self.tree.inbox.len(), self.inbox_expanded).height().min(room);
        let [_, list, inbox_area, footer_area] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(inbox_height),
            Constraint::Length(footer_height),
        ])
        .areas(inner);
        if inbox_area.height != self.inbox_area.height && !self.state.selected().is_empty() {
            // the tree has more or fewer rows now (the section was expanded or folded, an item came or went): it
            // scrolls to keep the cursor's Session in view
            let selected = self.state.selected().to_vec();
            self.state.select(selected);
        }
        self.inbox_area = inbox_area;
        let inbox_cursor = self.inbox_cursor.filter(|_| self.cursor_shown());
        inbox::draw(f, inbox_area, &self.tree.inbox, self.inbox_expanded, inbox_cursor, now, frame);
        if footer_height > 0 {
            let style = if self.prompt.is_some() { palette::note().bold() } else { palette::note() };
            f.render_widget(Paragraph::new(footer).style(style).wrap(Wrap { trim: false }), footer_area);
        }
        let cursor = self.cursor_shown() && self.inbox_cursor.is_none();
        rows::draw(f, list, &self.tree, &mut self.state, cursor, frame);
        self.pane = Rect { y: inner.y + inner.height.min(1), height: inner.height.saturating_sub(1), ..inner };
        if let Some(menu) = &self.menu {
            menu.draw(f, self.pane);
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyModifiers, MouseEvent};
    use ratatui::style::Modifier;

    use super::super::fixture::{archived, sample};
    use super::*;
    use crate::palette::{IDLE, NEEDS_YOU, SPINNER, STOPPED};
    use cpd_core::ids::{ContextId, PlanId, SessionId};

    const WORKING: &str = SPINNER[0];

    fn press(view: &mut View, code: KeyCode) -> Effect {
        view.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn typed(view: &mut View, text: &str) {
        for c in text.chars() {
            assert_eq!(press(view, KeyCode::Char(c)), Effect::None);
        }
    }

    /// A focused view whose cursor is shown and on `path`.
    fn on(path: &[&str]) -> View {
        let mut view = View::new(sample(), true);
        press(&mut view, KeyCode::Down); // shows the cursor where it was
        view.state.select(path.iter().map(|p| (*p).to_owned()).collect());
        view
    }

    fn sid(s: &str) -> SessionId {
        SessionId::parse(s).unwrap()
    }

    fn drawn(view: &mut View, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| view.draw(f, 0)).unwrap();
        let buf = terminal.backend().buffer();
        (0..height).map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_owned()).collect()
    }

    #[test]
    fn nothing_acts_until_an_arrow_key_shows_the_cursor() {
        let mut view = View::new(sample(), true);
        assert_eq!(view.state.selected(), ["alpha", "ship"], "the first Session is selected from the start");
        for key in [KeyCode::Enter, KeyCode::Char('x'), KeyCode::Char('n')] {
            assert_eq!(press(&mut view, key), Effect::None);
        }
        assert!(view.prompt.is_none());
        press(&mut view, KeyCode::Down);
        assert_eq!(view.state.selected(), ["alpha", "ship"], "the first arrow only shows the cursor");
        press(&mut view, KeyCode::Char('j'));
        assert_eq!(view.state.selected(), ["alpha", "sketch"]);
        press(&mut view, KeyCode::Char('k'));
        assert_eq!(view.state.selected(), ["alpha", "ship"]);
        press(&mut view, KeyCode::Up);
        assert_eq!(view.state.selected(), ["alpha", "ship"], "a Context row is never selected");
    }

    #[test]
    fn a_click_or_losing_focus_hides_the_cursor_until_the_next_arrow() {
        let mut view = on(&["alpha", "sketch"]);
        view.focus(false);
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        view.focus(true);
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None, "focus back is not a key");
        press(&mut view, KeyCode::Down);
        assert_eq!(view.state.selected(), ["alpha", "sketch"], "only shows it again");
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::Open(sid("l-sketch"))));
    }

    #[test]
    fn the_keys_are_the_references_and_no_other() {
        // commands.md §13.2: ↑↓/jk, Enter, `n`, `x`, `?`; `q`, `f`, `r`, `p` and Space do nothing
        for key in [KeyCode::Char('q'), KeyCode::Char('f'), KeyCode::Char('r'), KeyCode::Char('p'), KeyCode::Char(' '), KeyCode::Esc] {
            let mut view = on(&["alpha", "sketch"]);
            assert_eq!(press(&mut view, key), Effect::None, "{key:?}");
            assert!(view.prompt.is_none(), "{key:?}");
            assert!(view.state.opened().contains(&vec!["alpha".to_owned()]), "{key:?} folds nothing");
        }
        assert_eq!(press(&mut on(&["alpha", "sketch"]), KeyCode::Enter), Effect::Run(Action::Open(sid("l-sketch"))));
        assert_eq!(press(&mut View::new(sample(), false), KeyCode::Char('?')), Effect::Legend, "`?` needs no cursor");
    }

    #[test]
    fn enter_opens_the_lead_of_a_plan_row_and_a_fork() {
        assert_eq!(press(&mut on(&["alpha", "ship"]), KeyCode::Enter), Effect::Run(Action::Open(sid("l-ship"))));
        assert_eq!(press(&mut on(&["alpha", "f-blocked"]), KeyCode::Enter), Effect::Run(Action::Open(sid("f-blocked"))));
    }

    #[test]
    fn n_asks_for_a_name_and_starts_a_plan_in_the_context_of_the_session() {
        let mut view = on(&["alpha", "sketch"]);
        assert_eq!(press(&mut view, KeyCode::Char('n')), Effect::None);
        assert_eq!(view.footer(), "Fork Sketch into a new Plan, name: █ (Enter starts, Esc cancels)");
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert_eq!(view.footer(), "A name is required. Fork Sketch into a new Plan, name: █ (Enter starts, Esc cancels)");
        typed(&mut view, "pay-4521");
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Effect::Run(Action::NewPlan {
                context: ContextId::parse("alpha").unwrap(),
                id: PlanId::parse("pay-4521").unwrap(),
                title: "pay-4521".into(),
            })
        );
        assert!(view.prompt.is_none());
        let mut view = on(&["alpha", "f-idle"]);
        press(&mut view, KeyCode::Char('n'));
        assert_eq!(view.footer(), "Fork ship fork into a new Plan, name: █ (Enter starts, Esc cancels)");
    }

    #[test]
    fn x_asks_before_closing_and_only_y_closes() {
        let mut view = on(&["alpha", "sketch"]);
        assert_eq!(press(&mut view, KeyCode::Char('x')), Effect::None);
        assert_eq!(view.footer(), "Close Sketch? pi stops; the session file stays (y/N)");
        assert_eq!(press(&mut view, KeyCode::Char('y')), Effect::Run(Action::Close(sid("l-sketch"))));
        assert!(view.prompt.is_none());
        let mut view = on(&["alpha", "sketch"]);
        press(&mut view, KeyCode::Char('x'));
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None, "anything but y cancels");
        assert!(view.prompt.is_none());
    }

    #[test]
    fn x_warns_first_when_the_session_is_working() {
        let mut tree = sample();
        tree.contexts[0].plans[0].sessions[1].state = State::Working;
        let mut view = View::new(tree, true);
        press(&mut view, KeyCode::Down);
        view.state.select(vec!["alpha".into(), "f-idle".into()]);
        press(&mut view, KeyCode::Char('x'));
        assert_eq!(view.footer(), "ship fork is working. Close ship fork? pi stops; the session file stays (y/N)");
    }

    #[test]
    fn x_refuses_the_session_in_the_chat_slot() {
        let mut view = on(&["alpha", "ship"]);
        assert_eq!(press(&mut view, KeyCode::Char('x')), Effect::None);
        assert!(view.prompt.is_none());
        assert_eq!(view.footer(), "switch to another session first");
    }

    #[test]
    fn a_prompt_takes_every_key_and_esc_cancels_it() {
        let mut view = on(&["alpha", "sketch"]);
        press(&mut view, KeyCode::Char('n'));
        typed(&mut view, "qnxjk?");
        assert_eq!(view.footer(), "Fork Sketch into a new Plan, name: qnxjk?█ (Enter starts, Esc cancels)");
        assert_eq!(view.state.selected(), ["alpha", "sketch"]);
        assert_eq!(press(&mut view, KeyCode::Esc), Effect::None);
        assert!(view.prompt.is_none());
        assert_eq!(view.footer(), "");
    }

    #[test]
    fn the_note_stays_until_the_next_n_x_or_click() {
        let mut view = on(&["alpha", "sketch"]);
        view.done(Err("no_cockpit: No cockpit.".into()));
        press(&mut view, KeyCode::Down);
        assert_eq!(view.note, "no_cockpit: No cockpit.", "an arrow keeps it");
        press(&mut view, KeyCode::Char('x'));
        assert_eq!(view.note, "");
        let mut view = on(&["alpha", "sketch"]);
        view.done(Err("why".into()));
        drawn(&mut view, 44, 10);
        click(&mut view, 40, 9);
        assert_eq!(view.note, "");
    }

    #[test]
    fn the_footer_is_the_bottom_rows_bold_while_a_prompt_is_open() {
        let mut view = on(&["alpha", "sketch"]);
        press(&mut view, KeyCode::Char('n'));
        let rows = drawn(&mut view, 44, 14);
        assert_eq!(rows[9], format!(" ▾ {STOPPED} gamma"));
        assert_eq!(rows[11], " Fork Sketch into a new Plan, name: █");
        assert_eq!(rows[12], " (Enter starts, Esc cancels)");
        assert_eq!(rows[13], "", "two wrapped rows take three");
        let mut terminal = Terminal::new(TestBackend::new(44, 14)).unwrap();
        terminal.draw(|f| view.draw(f, 0)).unwrap();
        let cell = &terminal.backend().buffer()[(1, 11)];
        assert_eq!((cell.fg, cell.modifier.contains(Modifier::BOLD)), (palette::ui(palette::Ui::Note), true));
        press(&mut view, KeyCode::Esc);
        let rows = drawn(&mut view, 44, 14);
        assert!(rows[10..13].iter().all(|r| r.is_empty()) && rows[13].starts_with(" ── Inbox 0 "), "{rows:?}");
        view.done(Err("closed".into()));
        let rows = drawn(&mut view, 44, 14);
        assert!(rows[12].starts_with(" ── Inbox 0 "), "the Inbox section is above the note: {rows:?}");
        assert_eq!(rows[13], " closed", "a note is one row at the bottom");
    }

    fn click(view: &mut View, column: u16, row: u16) -> Effect {
        view.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE })
    }

    #[test]
    fn a_click_folds_a_context_and_opens_a_session_and_hides_the_cursor() {
        let mut view = on(&["alpha", "sketch"]);
        drawn(&mut view, 44, 12);
        assert_eq!(click(&mut view, 1, 1), Effect::None, "the fold glyph of the Context row `alpha`");
        assert!(!view.state.opened().contains(&vec!["alpha".to_owned()]));
        drawn(&mut view, 44, 12);
        assert_eq!(view.state.selected(), ["alpha", "sketch"], "a click selects nothing");
        assert!(!view.cursor_shown());
        click(&mut view, 1, 1);
        drawn(&mut view, 44, 12);
        assert_eq!(click(&mut view, 8, 2), Effect::Run(Action::Open(sid("l-ship"))), "the Plan row");
        assert_eq!(click(&mut view, 8, 6), Effect::Run(Action::Open(sid("f-blocked"))), "a fork row");
        assert_eq!(click(&mut view, 8, 10), Effect::None, "below the tree");
        assert_eq!(view.state.selected(), ["alpha", "sketch"], "no click moved the cursor");
    }

    #[test]
    fn a_click_on_a_context_rows_label_opens_its_manager_and_only_its_fold_glyph_folds() {
        let mut view = on(&["alpha", "sketch"]);
        drawn(&mut view, 44, 12);
        let manager = Effect::Run(Action::OpenManager(ContextId::parse("alpha").unwrap()));
        for column in [2, 5, 42] {
            assert_eq!(click(&mut view, column, 1), manager, "column {column} of the Context row");
        }
        assert!(view.state.opened().contains(&vec!["alpha".to_owned()]), "no label click folds");
        assert_eq!(click(&mut view, 1, 1), Effect::None, "the fold glyph runs no command");
        assert!(!view.state.opened().contains(&vec!["alpha".to_owned()]));
        drawn(&mut view, 44, 12);
        assert_eq!(click(&mut view, 5, 1), manager, "a folded Context opens its Manager too");
        assert_eq!(click(&mut view, 1, 1), Effect::None);
        assert!(view.state.opened().contains(&vec!["alpha".to_owned()]), "the glyph unfolds");
        let Effect::Run(action) = manager else { unreachable!() };
        assert_eq!(action.argv(), ["context", "manager", "--context=alpha"]);
    }

    #[test]
    fn an_archived_context_folds_at_its_glyph_and_opens_no_manager() {
        let mut view = View::new(archived(), true);
        let rows = drawn(&mut view, 44, 16);
        let more = rows.iter().position(|r| r == " …").unwrap() as u16;
        click(&mut view, 1, more); // the `…` row of the archived Contexts
        let rows = drawn(&mut view, 44, 16);
        let y = rows.iter().position(|r| *r == format!(" ▾ {STOPPED} zeta")).unwrap() as u16;
        let zeta = vec!["zeta".to_owned()];
        assert_eq!(click(&mut view, 6, y), Effect::None, "its label: no Manager for an archived Context");
        assert!(view.state.opened().contains(&zeta));
        assert_eq!(click(&mut view, 1, y), Effect::None);
        assert!(!view.state.opened().contains(&zeta), "its glyph folds it");
    }

    #[test]
    fn row_0_is_blank_and_the_rows_follow_one_lower_and_clicks_land_on_the_row_under_the_pointer() {
        let mut view = View::new(sample(), true);
        let rows = drawn(&mut view, 44, 12);
        assert_eq!(rows[0], "", "the blank row at the top");
        assert_eq!(rows[1], format!(" ▾ {NEEDS_YOU} alpha"));
        assert!(rows[2].starts_with(&format!(" ⏵ ├ {WORKING} ship")), "{rows:?}");
        assert_eq!(click(&mut view, 8, 0), Effect::None, "the blank row is no row: not the Context row below it, not the first Session");
        assert!(view.state.opened().contains(&vec!["alpha".to_owned()]), "alpha is not folded");
        assert_eq!(click(&mut view, 8, 2), Effect::Run(Action::Open(sid("l-ship"))), "the first Session's row");
        assert_eq!(click(&mut view, 8, 3), Effect::Run(Action::Open(sid("l-sketch"))), "the Session below it");
        assert_eq!(right_click(&mut view, 8, 0), Effect::None);
        assert_eq!(view.menu.as_ref().map(|m| m.path.clone()), Some(vec![]), "the blank row is the empty area: its menu");
    }

    /// The buffer of a drawn frame.
    fn buffer(view: &mut View, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| view.draw(f, 0)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// Column 0 and the last column are blank, and unstyled, on every row.
    fn assert_margins_blank(view: &mut View, width: u16, height: u16, what: &str) {
        let buf = buffer(view, width, height);
        for y in 0..height {
            for x in [0, width - 1] {
                let cell = &buf[(x, y)];
                assert_eq!((cell.symbol(), cell.modifier), (" ", Modifier::empty()), "{what}: ({x}, {y})");
                assert_eq!(cell.bg, ratatui::buffer::Cell::default().bg, "{what}: ({x}, {y})");
            }
        }
    }

    #[test]
    fn column_0_and_the_last_column_are_blank_on_every_row() {
        for (width, height) in [(44, 16), (30, 16), (12, 8)] {
            let mut view = on(&["alpha", "sketch"]); // the cursor is shown: its row is inverted inside the margins
            assert_margins_blank(&mut view, width, height, "the tree");
            let mut view = View::new(archived(), true);
            assert_margins_blank(&mut view, width, height, "archived rows");
            let mut view = on(&["alpha", "sketch"]);
            view.done(Err("a note that is long enough to wrap onto several rows of the pane".into()));
            assert_margins_blank(&mut view, width, height, "a wrapped note");
            press(&mut view, KeyCode::Char('x'));
            assert_margins_blank(&mut view, width, height, "a prompt");
            let mut view = View::new(sample(), true);
            drawn(&mut view, width, height);
            right_click(&mut view, 5, 1);
            assert_margins_blank(&mut view, width, height, "a menu");
            view.menu = None;
            right_click(&mut view, width - 1, height - 1);
            assert!(view.menu.is_some());
            assert_margins_blank(&mut view, width, height, "a menu at the corner");
        }
        let mut view = View::new(Tree { contexts: Vec::new(), events_cursor: 0, inbox: Vec::new(), engine_running: true }, true);
        assert_margins_blank(&mut view, 30, 5, "no Contexts");
    }

    #[test]
    fn the_counters_end_at_width_minus_2_and_the_cursor_row_is_inverted_between_the_margins() {
        let mut view = on(&["alpha", "ship"]);
        let buf = buffer(&mut view, 44, 12);
        let ship: String = (0..44).map(|x| buf[(x, 2)].symbol()).collect();
        assert!(ship.trim_end().ends_with("2⋅1⋅1⋅0⋅5") && ship.chars().nth(42) == Some('5'), "{ship:?}");
        let reversed: Vec<u16> = (0..44).filter(|x| buf[(*x, 2)].modifier.contains(Modifier::REVERSED)).collect();
        assert_eq!(reversed, (1..43).collect::<Vec<u16>>());
    }

    #[test]
    fn a_click_on_the_first_drawn_cell_of_a_row_acts_on_that_row_and_the_margins_are_no_row() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 12);
        assert_eq!(click(&mut view, 1, 2), Effect::Run(Action::Open(sid("l-ship"))), "the Plan row's `⏵`");
        assert_eq!(click(&mut view, 1, 3), Effect::Run(Action::Open(sid("l-sketch"))));
        assert_eq!(click(&mut view, 42, 3), Effect::Run(Action::Open(sid("l-sketch"))), "the last cell of the list");
        for row in [2, 3] {
            assert_eq!(click(&mut view, 0, row), Effect::None, "the left margin of row {row}");
            assert_eq!(click(&mut view, 43, row), Effect::None, "the right margin of row {row}");
        }
        assert_eq!(click(&mut view, 1, 1), Effect::None, "the Context row folds");
        assert!(!view.state.opened().contains(&vec!["alpha".to_owned()]));
        drawn(&mut view, 44, 12);
        assert_eq!(click(&mut view, 0, 1), Effect::None);
        assert!(!view.state.opened().contains(&vec!["alpha".to_owned()]), "the margin did not unfold it");
        assert_eq!(click(&mut view, 1, 1), Effect::None);
        assert!(view.state.opened().contains(&vec!["alpha".to_owned()]));
    }

    #[test]
    fn the_menu_is_clamped_inside_the_inner_area_and_a_click_on_a_margin_is_outside_it() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 43, 15);
        drawn(&mut view, 44, 16);
        let rect = view.menu.as_ref().unwrap().rect(view.pane);
        assert_eq!((rect.x, rect.y, rect.right(), rect.bottom()), (27, 13, 43, 16));
        right_click(&mut view, 0, 0); // an open menu takes the click: a right click outside closes it
        assert!(view.menu.is_none());
        right_click(&mut view, 0, 0);
        drawn(&mut view, 44, 16);
        let rect = view.menu.as_ref().unwrap().rect(view.pane);
        assert_eq!((rect.x, rect.y), (1, 1), "not on the blank column or row");
    }

    #[test]
    fn clicks_are_ignored_while_a_prompt_is_open() {
        let mut view = on(&["alpha", "sketch"]);
        drawn(&mut view, 44, 10);
        press(&mut view, KeyCode::Char('n'));
        assert_eq!(click(&mut view, 8, 2), Effect::None);
        assert_eq!(click(&mut view, 5, 1), Effect::None);
        assert!(view.state.opened().contains(&vec!["alpha".to_owned()]));
    }

    #[test]
    fn a_refresh_keeps_the_cursor_on_its_row_and_the_folds() {
        let mut view = on(&["alpha", "sketch"]);
        view.state.close(&["beta".to_owned()]);
        let mut tree = sample();
        tree.contexts[0].plans[1].sessions[0].state = State::Working;
        view.refresh(tree);
        assert_eq!(view.state.selected(), ["alpha", "sketch"]);
        assert!(!view.state.opened().contains(&vec!["beta".to_owned()]));
    }

    fn right_click(view: &mut View, column: u16, row: u16) -> Effect {
        view.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Right), column, row, modifiers: KeyModifiers::NONE })
    }

    #[test]
    fn a_click_on_a_more_row_expands_it_and_a_click_on_it_again_collapses_it() {
        let mut view = View::new(archived(), true);
        let rows = drawn(&mut view, 44, 16);
        assert_eq!((rows[5].as_str(), rows[9].as_str()), ("   └ …", " …"));
        assert_eq!(click(&mut view, 3, 5), Effect::None, "the `…` of alpha");
        let rows = drawn(&mut view, 44, 16);
        assert_eq!(rows[5], "   ├ …");
        assert!(rows[6].starts_with(&format!("   ├ {STOPPED} old")), "{rows:?}");
        assert_eq!(rows[7], format!("   └ {NEEDS_YOU} ship·f-bl"));
        assert_eq!(rows[11], " …");
        assert_eq!(click(&mut view, 1, 11), Effect::None, "the `…` of the archived Contexts");
        let rows = drawn(&mut view, 44, 16);
        assert_eq!((rows[11].as_str(), rows[12].as_str()), (" …", format!(" ▾ {STOPPED} zeta").as_str()));
        assert!(rows[13].starts_with(&format!("   ├ {STOPPED} zp")), "{rows:?}");
        // a click on a guide no longer collapses (it is a click on its row: here an archived Plan's history); a
        // click on `…` does
        assert_eq!(click(&mut view, 3, 13), Effect::Run(Action::History(sid("l-zp"))));
        assert_eq!(drawn(&mut view, 44, 16)[12], format!(" ▾ {STOPPED} zeta"));
        assert_eq!(click(&mut view, 1, 11), Effect::None);
        let rows = drawn(&mut view, 44, 16);
        assert_eq!((rows[11].as_str(), rows[12].as_str()), (" …", ""));
        assert_eq!(click(&mut view, 3, 5), Effect::None);
        let rows = drawn(&mut view, 44, 16);
        assert_eq!((rows[5].as_str(), rows[6].as_str()), ("   └ …", format!(" ▾ {NEEDS_YOU} beta").as_str()));
    }

    #[test]
    fn a_click_below_the_last_row_expands_nothing() {
        let mut view = View::new(archived(), true);
        drawn(&mut view, 44, 12);
        assert_eq!(click(&mut view, 1, 11), Effect::None);
        assert_eq!(click(&mut view, 3, 10), Effect::None, "the row right below the last");
        let none_shown = |rows: Vec<String>| rows.iter().all(|r| !r.contains(&format!("{STOPPED} old")) && !r.contains(&format!("{STOPPED} zeta")));
        assert!(none_shown(drawn(&mut view, 44, 12)), "the `…` rows are at rows 5 and 9");
        let _ = right_click(&mut view, 1, 9);
        assert!(none_shown(drawn(&mut view, 44, 12)), "only the left button clicks");
    }

    #[test]
    fn a_click_on_an_archived_session_shows_its_history_and_enter_n_and_x_never_act_on_one() {
        let mut view = View::new(archived(), true);
        drawn(&mut view, 44, 16);
        click(&mut view, 3, 5);
        drawn(&mut view, 44, 16);
        click(&mut view, 1, 11);
        let rows = drawn(&mut view, 44, 16);
        assert!(rows[6].starts_with(&format!("   ├ {STOPPED} old")) && rows[12] == format!(" ▾ {STOPPED} zeta"), "{rows:?}");
        // the archived Plan (its Lead), the archived fork of a live Plan, the Plan of the archived Context and its
        // fork: each shows the history of its Session
        for ((column, row), id) in [((8, 6), "l-old"), ((8, 7), "f-blocked"), ((8, 13), "l-zp"), ((8, 14), "f-zp")] {
            assert_eq!(click(&mut view, column, row), Effect::Run(Action::History(sid(id))), "({column}, {row})");
            assert_eq!(drawn(&mut view, 44, 16), rows, "({column}, {row}) changes nothing");
        }
        // the archived Context folds; a live row still opens
        assert_eq!(click(&mut view, 5, 12), Effect::None);
        // the cursor is put on archived rows by hand, as a bug might: no key acts on it
        press(&mut view, KeyCode::Down);
        for path in [&["alpha", "old"][..], &["alpha", "f-blocked"], &["zeta", "zp"], &["zeta", "f-zp"], &["alpha", "…"]] {
            view.state.select(path.iter().map(|p| (*p).to_owned()).collect());
            for key in [KeyCode::Enter, KeyCode::Char('x'), KeyCode::Char('n')] {
                assert_eq!(press(&mut view, key), Effect::None, "{path:?} {key:?}");
                assert!(view.prompt.is_none(), "{path:?} {key:?}");
            }
        }
    }

    #[test]
    fn a_snapshot_that_archives_the_cursors_session_moves_the_cursor_to_a_live_one() {
        let mut view = on(&["alpha", "sketch"]);
        view.refresh(archived());
        assert_eq!(view.state.selected(), ["alpha", "sketch"], "still there");
        let mut tree = archived();
        tree.contexts[0].plans[1].head.archived_at = Some(super::super::fixture::WHEN.into());
        view.refresh(tree);
        assert_eq!(view.state.selected(), ["alpha", "f-idle"], "the Session at its place, now (the forks follow the Plans, B1)");
    }

    // ----- the right-click menu -----

    /// The labels of the open menu, as drawn: the rows inside its box.
    fn menu_items(view: &mut View) -> Vec<String> {
        let rect = view.menu.as_ref().expect("a menu is open").rect(view.pane);
        let rows = drawn(view, 44, 16);
        (rect.y + 1..rect.bottom() - 1)
            .map(|y| {
                let row: Vec<char> = rows[y as usize].chars().collect();
                row[rect.x as usize + 1..(rect.right() - 1) as usize].iter().collect::<String>().trim().to_owned()
            })
            .collect()
    }

    fn menu_at(view: &mut View, column: u16, row: u16) -> Vec<String> {
        view.menu = None;
        drawn(view, 44, 16);
        assert_eq!(right_click(view, column, row), Effect::None);
        assert!(view.menu.is_some(), "({column}, {row})");
        menu_items(view)
    }

    #[test]
    fn a_right_click_opens_the_menu_of_the_row_or_the_empty_area() {
        let mut view = View::new(sample(), true);
        assert_eq!(menu_at(&mut view, 5, 1), ["New Plan…", "Rename…", "Copy id", "Archive"], "a Context");
        assert_eq!(menu_at(&mut view, 8, 2), ["New session…", "Rename…", "Copy id", "Archive"], "a Plan");
        assert_eq!(menu_at(&mut view, 8, 5), ["Open", "Rename…", "Fork…", "Copy id", "Close", "Archive"], "a fork");
        assert_eq!(menu_at(&mut view, 8, 14), ["New Context…"], "the empty area");
        assert_eq!(menu_at(&mut view, 8, 11), ["New Context…"], "below the last row");
        assert_eq!(view.note, "");
        let mut view = View::new(View::new(archived(), true).tree, true);
        assert_eq!(menu_at(&mut view, 8, 1), ["New Plan…", "Rename…", "Copy id", "Archive"], "a live row of a tree with archived ones");
    }

    #[test]
    fn a_right_click_on_an_archived_row_offers_rename_copy_id_and_unarchive_and_on_a_more_row_nothing() {
        let mut view = View::new(archived(), true);
        drawn(&mut view, 44, 16);
        click(&mut view, 3, 5);
        drawn(&mut view, 44, 16);
        click(&mut view, 1, 11);
        let rows = drawn(&mut view, 44, 16);
        assert!(rows[6].starts_with(&format!("   ├ {STOPPED} old")) && rows[12] == format!(" ▾ {STOPPED} zeta"), "{rows:?}");
        // the archived Plan, its fork, an archived Context and its Plan, and an archived fork of a live Plan
        for (column, row) in [(8, 6), (8, 7), (8, 12), (8, 13), (8, 14)] {
            assert_eq!(menu_at(&mut view, column, row), ["Rename…", "Copy id", "Unarchive"], "({column}, {row})");
            press(&mut view, KeyCode::Esc);
        }
        for (column, row) in [(3, 5), (1, 11), (1, 14)] {
            let _ = right_click(&mut view, column, row);
            assert_eq!(view.menu.is_some(), row == 14, "({column}, {row}): `…` rows have none, the empty area has one");
            view.menu = None;
        }
    }

    #[test]
    fn the_menu_is_drawn_at_the_click_and_clamped_inside_the_pane() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 5);
        let rows = drawn(&mut view, 44, 16);
        assert_eq!(rows[5], format!("   ├ {IDLE} s┌─────────┐"), "{rows:?}");
        assert_eq!(rows[6], format!("   └ {NEEDS_YOU} s│ Open    │"));
        assert!(rows[7].starts_with(&format!(" ▾ {NEEDS_YOU} bet│ Rename… │")), "drawn over the tree: {rows:?}");
        assert_eq!(rows[12], "        └─────────┘");
        let reversed = |view: &mut View, x, y| {
            let mut terminal = Terminal::new(TestBackend::new(44, 16)).unwrap();
            terminal.draw(|f| view.draw(f, 0)).unwrap();
            terminal.backend().buffer()[(x, y)].modifier.contains(Modifier::REVERSED)
        };
        assert!(reversed(&mut view, 10, 6), "the first item is selected");
        press(&mut view, KeyCode::Down);
        assert!(reversed(&mut view, 10, 7) && !reversed(&mut view, 10, 6));
        view.menu = None;
        right_click(&mut view, 43, 15);
        let rows = drawn(&mut view, 44, 16);
        let box_row = |left: char, fill: &str, right: char| format!("{}{left}{fill}{right}", " ".repeat(27));
        assert_eq!((rows[13].clone(), rows[14].clone()), (box_row('┌', &"─".repeat(14), '┐'), box_row('│', " New Context… ", '│')));
        // the menu is drawn over the Inbox header, which shows to its left
        assert!(rows[15].starts_with(" ── Inbox 0 ─") && rows[15].ends_with("└──────────────┘"), "{rows:?}");
    }

    #[test]
    fn enter_on_the_menu_picks_its_selected_item_and_esc_closes_it_running_nothing() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 5); // a fork: f-idle
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::Open(sid("f-idle"))));
        assert!(view.menu.is_none());
        right_click(&mut view, 8, 5);
        assert_eq!(press(&mut view, KeyCode::Esc), Effect::None);
        assert!(view.menu.is_none() && view.prompt.is_none());
        assert_eq!(view.footer(), "");
    }

    fn archive_of(subject: Subject) -> Effect {
        Effect::Run(Action::Archive { subject, free: None })
    }

    #[test]
    fn archive_runs_at_once_with_no_prompt() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 5); // the fork `f-idle`, which is not in the chat slot
        for _ in 0..5 {
            press(&mut view, KeyCode::Down);
        }
        assert_eq!(press(&mut view, KeyCode::Enter), archive_of(Subject::Session(sid("f-idle"))));
        assert!(view.menu.is_none() && view.prompt.is_none());
        assert_eq!(view.footer(), "");
        // a click picks it as well
        right_click(&mut view, 8, 5);
        let effect = view.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 10, row: 11, modifiers: KeyModifiers::NONE });
        assert_eq!(effect, archive_of(Subject::Session(sid("f-idle"))));
        // a Plan and a Context name their own objects: `sketch` and `beta` hold no Lead of the chat slot
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 3);
        for _ in 0..3 {
            press(&mut view, KeyCode::Down);
        }
        assert_eq!(press(&mut view, KeyCode::Enter), archive_of(Subject::Plan(PlanId::parse("sketch").unwrap())));
        drawn(&mut view, 44, 16);
        right_click(&mut view, 5, 7);
        for _ in 0..3 {
            press(&mut view, KeyCode::Down);
        }
        assert_eq!(press(&mut view, KeyCode::Enter), archive_of(Subject::Context(ContextId::parse("beta").unwrap())));
    }

    /// Archive of the row at (`column`, `row`) of `tree`: the menu's last item, picked with Enter.
    fn archive_at(tree: Tree, column: u16, row: u16) -> Effect {
        let mut view = View::new(tree, true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, column, row);
        for _ in 0..5 {
            press(&mut view, KeyCode::Down);
        }
        press(&mut view, KeyCode::Enter)
    }

    fn freed(subject: Subject, free: Free) -> Effect {
        Effect::Run(Action::Archive { subject, free: Some(free) })
    }

    #[test]
    fn archive_of_the_chat_slots_lead_opens_the_next_live_session_in_tree_order_first() {
        // rows: alpha, ship (l-ship in the chat slot), sketch, old, f-idle, f-blocked, beta, rel, old, gamma
        let ship = Subject::Plan(PlanId::parse("ship").unwrap());
        assert_eq!(archive_at(sample(), 8, 2), freed(ship, Free::Open(sid("l-sketch"))), "a Plan row: the Plan below it");
        let mut tree = sample();
        tree.contexts[0].plans[0].sessions[0].place = crate::read::Place::Parked;
        tree.contexts[0].plans[0].sessions[1].place = crate::read::Place::Chat;
        assert_eq!(
            archive_at(tree.clone(), 8, 5),
            freed(Subject::Session(sid("f-idle")), Free::Open(sid("f-blocked"))),
            "a fork row: the fork below it"
        );
        // the Plan whose side fork holds the slot: the fork is archived with it, so it is not the next
        assert_eq!(archive_at(tree, 8, 2), freed(Subject::Plan(PlanId::parse("ship").unwrap()), Free::Open(sid("l-sketch"))));
        // a Context row: the first live Session below it, none of its own
        assert_eq!(
            archive_at(sample(), 5, 1),
            freed(Subject::Context(ContextId::parse("alpha").unwrap()), Free::Open(sid("l-rel")))
        );
    }

    #[test]
    fn archive_of_the_chat_slots_lead_opens_the_previous_session_when_none_follows() {
        // rows: alpha, ship, sketch, f-idle, f-blocked, beta, rel (in the chat slot), gamma
        let mut tree = sample();
        tree.contexts[0].plans[0].sessions[0].place = crate::read::Place::Parked;
        tree.contexts[0].plans.pop(); // `old`
        tree.contexts[1].plans[0].sessions[0].place = crate::read::Place::Chat;
        assert_eq!(archive_at(tree, 8, 7), freed(Subject::Plan(PlanId::parse("rel").unwrap()), Free::Open(sid("f-blocked"))));
    }

    #[test]
    fn archive_of_the_only_live_sessions_lead_in_the_chat_slot_parks_it_first() {
        use super::super::fixture::{context, plan, session, tree};
        let only = |lead: SessionNode, forks: Vec<SessionNode>| {
            tree(vec![context("alpha", vec![plan("ship", "alpha", cpd_core::plan::Phase::Dispatch, [0; 6], lead, forks)])])
        };
        let chat = || session("l-ship", "ship lead", State::Idle, None, crate::read::Place::Chat);
        assert_eq!(
            archive_at(only(chat(), vec![]), 8, 2),
            freed(Subject::Plan(PlanId::parse("ship").unwrap()), Free::Park)
        );
        // an archived Session is no next one
        let mut gone = session("f-old", "old", State::Stopped, None, crate::read::Place::None);
        gone.archived_at = Some(super::super::fixture::WHEN.into());
        assert_eq!(
            archive_at(only(chat(), vec![gone]), 8, 2),
            freed(Subject::Plan(PlanId::parse("ship").unwrap()), Free::Park)
        );
        // a fork in the chat slot with the Plan's Lead beside it: the Lead is the next one
        let fork = session("f-chat", "chat fork", State::Idle, None, crate::read::Place::Chat);
        let lead = session("l-ship", "ship lead", State::Idle, None, crate::read::Place::Parked);
        assert_eq!(archive_at(only(lead, vec![fork]), 8, 3), freed(Subject::Session(sid("f-chat")), Free::Open(sid("l-ship"))));
    }

    #[test]
    fn archive_ignores_the_slot_when_a_snapshot_dropped_the_row() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 2);
        for _ in 0..2 {
            press(&mut view, KeyCode::Down);
        }
        let mut tree = sample();
        tree.contexts[0].plans.retain(|p| p.head.id.as_str() != "ship");
        view.refresh(tree);
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
    }

    #[test]
    fn close_in_the_menu_asks_as_x_does_and_refuses_the_session_in_the_chat_slot() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 5);
        for _ in 0..4 {
            press(&mut view, KeyCode::Down);
        }
        press(&mut view, KeyCode::Enter);
        assert_eq!(view.footer(), "Close ship fork? pi stops; the session file stays (y/N)");
        assert_eq!(press(&mut view, KeyCode::Char('y')), Effect::Run(Action::Close(sid("f-idle"))));
        // the Lead in the chat slot: the same refusal as `x`
        let mut tree = sample();
        tree.contexts[0].plans[0].sessions[1].place = crate::read::Place::Chat;
        let mut view = View::new(tree, true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 5);
        for _ in 0..4 {
            press(&mut view, KeyCode::Down);
        }
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert!(view.prompt.is_none());
        assert_eq!(view.footer(), IN_CHAT_SLOT);
    }

    #[test]
    fn rename_and_unarchive_on_an_archived_row_run_without_asking_to_confirm() {
        let mut view = View::new(archived(), true);
        drawn(&mut view, 44, 16);
        click(&mut view, 3, 5);
        drawn(&mut view, 44, 16);
        click(&mut view, 1, 11);
        drawn(&mut view, 44, 16);
        for (row, subject) in [
            (6, Subject::Plan(PlanId::parse("old").unwrap())),
            (7, Subject::Session(sid("f-blocked"))),
            (12, Subject::Context(ContextId::parse("zeta").unwrap())),
        ] {
            right_click(&mut view, 8, row);
            press(&mut view, KeyCode::Down);
            press(&mut view, KeyCode::Down);
            assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::Unarchive(subject)), "row {row}");
            assert!(view.prompt.is_none() && view.menu.is_none());
        }
        // Rename… starts from the label of the archived row and runs the rename of the object
        right_click(&mut view, 8, 6);
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert_eq!(view.footer(), "Title: Old█ (Enter confirms, Esc cancels)");
        typed(&mut view, "er");
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Effect::Run(Action::RenamePlan { id: PlanId::parse("old").unwrap(), title: "Older".into() })
        );
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 12);
        press(&mut view, KeyCode::Enter);
        assert_eq!(view.footer(), "Title: Zeta█ (Enter confirms, Esc cancels)");
        press(&mut view, KeyCode::Esc);
        // and of an archived fork: its name
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 7);
        press(&mut view, KeyCode::Enter);
        assert_eq!(view.footer(), "Name: ship review█ (Enter confirms, Esc cancels)");
    }

    #[test]
    fn copy_id_copies_the_shown_id_of_every_kind_of_row() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        for (column, row, index, id) in [(5, 1, 2, "alpha"), (8, 2, 2, "ship"), (8, 5, 3, "ship·f-id")] {
            right_click(&mut view, column, row);
            for _ in 0..index {
                press(&mut view, KeyCode::Down);
            }
            assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::CopyId(id.into())), "{id}");
            assert!(view.menu.is_none() && view.prompt.is_none());
        }
        // archived rows: a Plan, a fork of it, an archived Context
        let mut view = View::new(archived(), true);
        drawn(&mut view, 44, 16);
        click(&mut view, 3, 5);
        drawn(&mut view, 44, 16);
        click(&mut view, 1, 11);
        drawn(&mut view, 44, 16);
        for (row, id) in [(6, "old"), (7, "ship·f-bl"), (12, "zeta")] {
            right_click(&mut view, 8, row);
            press(&mut view, KeyCode::Down);
            assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::CopyId(id.into())), "row {row}");
        }
    }

    /// Right-clicks (`column`, `row`) and picks the item `index` of its menu with Enter.
    fn pick_at(view: &mut View, column: u16, row: u16, index: usize) -> Effect {
        view.menu = None;
        drawn(view, 44, 16);
        right_click(view, column, row);
        for _ in 0..index {
            press(view, KeyCode::Down);
        }
        press(view, KeyCode::Enter)
    }

    fn erase(view: &mut View, n: usize) {
        for _ in 0..n {
            press(view, KeyCode::Backspace);
        }
    }

    #[test]
    fn rename_starts_from_the_current_label_and_runs_the_rename_of_the_row() {
        let mut view = View::new(sample(), true);
        // a Context: its title
        assert_eq!(pick_at(&mut view, 5, 1, 1), Effect::None);
        assert!(view.menu.is_none());
        assert_eq!(view.footer(), "Title: Alpha█ (Enter confirms, Esc cancels)");
        typed(&mut view, " two");
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Effect::Run(Action::RenameContext { id: ContextId::parse("alpha").unwrap(), title: "Alpha two".into() })
        );
        assert!(view.prompt.is_none());
        // a Plan: its title
        pick_at(&mut view, 8, 2, 1);
        assert_eq!(view.footer(), "Title: Ship█ (Enter confirms, Esc cancels)");
        erase(&mut view, 4);
        typed(&mut view, "Release");
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Effect::Run(Action::RenamePlan { id: PlanId::parse("ship").unwrap(), title: "Release".into() })
        );
        // a fork: its name
        pick_at(&mut view, 8, 5, 1);
        assert_eq!(view.footer(), "Name: ship fork█ (Enter confirms, Esc cancels)");
        typed(&mut view, "!");
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Effect::Run(Action::RenameSession { id: sid("f-idle"), name: "ship fork!".into() })
        );
    }

    #[test]
    fn rename_refuses_an_empty_text_in_the_prompt_and_esc_cancels() {
        let mut view = View::new(sample(), true);
        pick_at(&mut view, 8, 2, 1);
        erase(&mut view, 4);
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert_eq!(view.footer(), "A title is required. Title: █ (Enter confirms, Esc cancels)");
        typed(&mut view, "   ");
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None, "blanks are no title");
        assert!(view.prompt.is_some());
        assert_eq!(press(&mut view, KeyCode::Esc), Effect::None);
        assert!(view.prompt.is_none());
        pick_at(&mut view, 8, 5, 1);
        erase(&mut view, 20);
        press(&mut view, KeyCode::Enter);
        assert_eq!(view.footer(), "A name is required. Name: █ (Enter confirms, Esc cancels)");
    }

    #[test]
    fn fork_and_new_session_ask_a_name_and_fork_the_right_source() {
        let mut view = View::new(sample(), true);
        // a fork row: Fork… forks that fork
        assert_eq!(pick_at(&mut view, 8, 5, 2), Effect::None);
        assert_eq!(view.footer(), "Name: ship fork (fork)█ (Enter confirms, Esc cancels)");
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Effect::Run(Action::Fork { source: sid("f-idle"), name: "ship fork (fork)".into() })
        );
        // a Plan row: New session… forks the Plan's Lead, from the Plan's title
        assert_eq!(pick_at(&mut view, 8, 2, 0), Effect::None);
        assert_eq!(view.footer(), "Name: Ship (fork)█ (Enter confirms, Esc cancels)");
        erase(&mut view, 7);
        typed(&mut view, " again");
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::Fork { source: sid("l-ship"), name: "Ship again".into() }));
        pick_at(&mut view, 8, 2, 0);
        press(&mut view, KeyCode::Esc);
        assert!(view.prompt.is_none());
    }

    #[test]
    fn the_name_of_a_fork_is_cut_to_fit_the_limit() {
        let mut tree = sample();
        tree.contexts[0].plans[1].head.title = "x".repeat(150);
        let mut view = View::new(tree, true);
        pick_at(&mut view, 8, 3, 0);
        let Some(Prompt::Text { input, .. }) = &view.prompt else { panic!("a prompt") };
        assert_eq!(input.chars().count(), 80);
        assert!(input.ends_with("x (fork)"));
    }

    #[test]
    fn new_context_asks_a_title_and_derives_the_id() {
        let mut view = View::new(sample(), true);
        assert_eq!(pick_at(&mut view, 8, 14, 0), Effect::None);
        assert_eq!(view.footer(), "Title: █ (Enter confirms, Esc cancels)");
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert_eq!(view.footer(), "A title is required. Title: █ (Enter confirms, Esc cancels)");
        typed(&mut view, "Building cpd!");
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Effect::Run(Action::NewContext { id: ContextId::parse("building-cpd").unwrap(), title: "Building cpd!".into() })
        );
        assert!(view.prompt.is_none());
        // a taken id (also of an archived Context) gets `-2`, then `-3`
        let mut view = View::new(archived(), true);
        pick_at(&mut view, 8, 14, 0);
        typed(&mut view, "Alpha");
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Effect::Run(Action::NewContext { id: ContextId::parse("alpha-2").unwrap(), title: "Alpha".into() })
        );
        pick_at(&mut view, 8, 14, 0);
        typed(&mut view, "zeta");
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Effect::Run(Action::NewContext { id: ContextId::parse("zeta-2").unwrap(), title: "zeta".into() })
        );
        // a title that no id is made of is refused in the prompt
        pick_at(&mut view, 8, 14, 0);
        typed(&mut view, "2024");
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert!(view.footer().starts_with("A title needs at least two letters"), "{}", view.footer());
        assert!(view.prompt.is_some());
    }

    #[test]
    fn new_plan_derives_the_id_and_asks_no_repo() {
        let mut view = View::new(sample(), true);
        assert_eq!(pick_at(&mut view, 5, 1, 0), Effect::None);
        assert_eq!(view.footer(), "Title: █ (Enter confirms, Esc cancels)");
        typed(&mut view, "Pay 4521");
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Effect::Run(Action::NewPlan {
                context: ContextId::parse("alpha").unwrap(),
                id: PlanId::parse("pay-4521").unwrap(),
                title: "Pay 4521".into(),
            })
        );
        // an id taken by any Plan of the tree gets the suffix
        pick_at(&mut view, 5, 1, 0);
        typed(&mut view, "Rel");
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Effect::Run(Action::NewPlan {
                context: ContextId::parse("alpha").unwrap(),
                id: PlanId::parse("rel-2").unwrap(),
                title: "Rel".into(),
            })
        );
        pick_at(&mut view, 5, 1, 0);
        typed(&mut view, "!!");
        press(&mut view, KeyCode::Enter);
        assert!(view.footer().starts_with("A title needs at least two letters"));
    }

    #[test]
    fn new_plan_in_a_context_with_no_plan_asks_only_the_title() {
        let mut view = View::new(sample(), true);
        assert_eq!(pick_at(&mut view, 5, 9, 0), Effect::None, "the Context `gamma`");
        typed(&mut view, "First plan");
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Effect::Run(Action::NewPlan {
                context: ContextId::parse("gamma").unwrap(),
                id: PlanId::parse("first-plan").unwrap(),
                title: "First plan".into(),
            })
        );
        assert!(view.prompt.is_none());
    }

    #[test]
    fn a_text_prompt_takes_every_key_and_ignores_clicks() {
        let mut view = View::new(sample(), true);
        pick_at(&mut view, 8, 2, 1);
        typed(&mut view, "qnxjk?");
        assert_eq!(view.footer(), "Title: Shipqnxjk?█ (Enter confirms, Esc cancels)");
        assert_eq!(click(&mut view, 5, 1), Effect::None);
        assert_eq!(right_click(&mut view, 5, 1), Effect::None);
        assert!(view.menu.is_none() && view.prompt.is_some());
    }

    #[test]
    fn an_open_menu_takes_every_key_and_click() {
        let mut view = on(&["alpha", "sketch"]);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 20, 4);
        for key in [KeyCode::Char('n'), KeyCode::Char('x'), KeyCode::Char('?'), KeyCode::Char('j'), KeyCode::Char('q')] {
            assert_eq!(press(&mut view, key), Effect::None, "{key:?}");
            assert!(view.prompt.is_none() && view.menu.is_some(), "{key:?}");
        }
        assert_eq!(view.state.selected(), ["alpha", "sketch"]);
        // a left click outside closes it and does not click what is there (the Context row `alpha` would fold)
        assert_eq!(click(&mut view, 2, 1), Effect::None);
        assert!(view.menu.is_none());
        assert!(view.state.opened().contains(&vec!["alpha".to_owned()]));
        // so does a right click outside, and neither opens another
        right_click(&mut view, 20, 4);
        assert_eq!(right_click(&mut view, 2, 1), Effect::None);
        assert!(view.menu.is_none());
        // a click on the border keeps it; a right click on an item picks nothing
        right_click(&mut view, 20, 4);
        let rect = view.menu.as_ref().unwrap().rect(view.pane);
        click(&mut view, rect.x, rect.y);
        assert!(view.menu.is_some());
        assert_eq!(right_click(&mut view, rect.x + 2, rect.y + 1), Effect::None);
        assert!(view.menu.is_some());
        // the mouse moving or the button coming up changes nothing
        for kind in [MouseEventKind::Up(MouseButton::Left), MouseEventKind::Moved, MouseEventKind::Drag(MouseButton::Left), MouseEventKind::ScrollDown] {
            view.mouse(MouseEvent { kind, column: 0, row: 0, modifiers: KeyModifiers::NONE });
        }
        assert!(view.menu.is_some());
    }

    #[test]
    fn a_click_on_an_item_picks_it() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 5);
        let rect = view.menu.as_ref().unwrap().rect(view.pane);
        assert_eq!(click(&mut view, rect.x + 2, rect.y + 1), Effect::Run(Action::Open(sid("f-idle"))));
        assert!(view.menu.is_none());
    }

    #[test]
    fn a_prompt_ignores_right_clicks_and_no_key_opens_the_menu() {
        let mut view = on(&["alpha", "sketch"]);
        drawn(&mut view, 44, 16);
        press(&mut view, KeyCode::Char('n'));
        right_click(&mut view, 8, 3);
        assert!(view.menu.is_none());
        press(&mut view, KeyCode::Esc);
        for key in [KeyCode::Char('m'), KeyCode::Char('o'), KeyCode::Char(' '), KeyCode::Menu, KeyCode::F(10), KeyCode::Char('/')] {
            press(&mut view, key);
            assert!(view.menu.is_none(), "{key:?}");
        }
    }

    #[test]
    fn a_snapshot_that_drops_the_menus_row_makes_its_item_do_nothing() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 5);
        let mut tree = sample();
        tree.contexts[0].plans[0].sessions.retain(|s| s.id.as_str() != "f-idle");
        view.refresh(tree);
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert!(view.menu.is_none());
    }
    // ---------- the Inbox section (commands.md §13.2) ----------

    use super::super::inbox::item;
    use cpd_core::inbox::model::{Item, Kind as InboxKind, Urgency};

    /// 2026-01-03T00:00:00Z: the clock of the Inbox tests.
    fn now() -> OffsetDateTime {
        OffsetDateTime::parse("2026-01-03T00:00:00.000Z", &time::format_description::well_known::Rfc3339).unwrap()
    }

    /// Five open items, ages `42s`, `5m`, `3h`, `2d` and `10m` at [`now`], in the order of `cpd inbox list` as the
    /// tests have it (the view draws them as it is given them).
    fn five() -> Vec<Item> {
        let mut dialog = item(1, InboxKind::Dialog, Urgency::Red, "f-blocked: Worker question", "2026-01-02T23:59:18.000Z");
        dialog.session = Some(sid("f-blocked"));
        let mut failing = item(2, InboxKind::PrFailing, Urgency::Red, "acme/api#12 checks failed", "2026-01-02T23:55:00.000Z");
        failing.url = Some("https://github.com/acme/api/pull/12".into());
        let mut stuck = item(3, InboxKind::JobStuck, Urgency::Yellow, "ship: Job c is stuck", "2026-01-02T21:00:00.000Z");
        stuck.plan = Some(PlanId::parse("ship").unwrap());
        let mut error = item(4, InboxKind::EngineError, Urgency::Magenta, "engine: store busy", "2026-01-01T00:00:00.000Z");
        error.event = Some(9);
        let mut unread = item(5, InboxKind::Unread, Urgency::White, "ship lead has a reply", "2026-01-02T23:50:00.000Z");
        unread.session = Some(sid("l-ship"));
        vec![dialog, failing, stuck, error, unread]
    }

    fn with_inbox(n: usize) -> View {
        let mut view = View::new(sample(), true);
        view.tree.inbox = five().into_iter().take(n).collect();
        view
    }

    fn draw_now(view: &mut View, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| view.draw_at(f, 0, now())).unwrap();
        terminal.backend().buffer().clone()
    }

    /// The rows as text: a link's cells carry their OSC 8 escapes (`crate::link`), which are not text.
    fn text_rows(buf: &ratatui::buffer::Buffer) -> Vec<String> {
        (0..buf.area.height)
            .map(|y| {
                let (mut row, mut x) = (String::new(), 0);
                while x < buf.area.width {
                    let symbol = buf[(x, y)].symbol();
                    // a piece of a link is one cell holding its text; the backend never writes the cell(s) after it
                    let text = symbol.strip_prefix("\x1b]8;;").and_then(|s| s.split_once('\x07')).map(|(_, t)| t.trim_end_matches("\x1b]8;;\x07"));
                    row += text.unwrap_or(symbol);
                    x += text.map_or(1, |t| t.chars().count() as u16);
                }
                row.trim_end().to_owned()
            })
            .collect()
    }

    /// The header of the section at 43 columns: a blank margin, then the column's header for `n` items.
    fn header(n: usize) -> String {
        let title = format!(" Inbox {n} ");
        format!(" ── Inbox {n} {}", "─".repeat(41 - 2 - title.chars().count()))
    }

    /// An item row at 43 columns: the margin, the filled dot, the label, the age ending at the last column of the inner area.
    fn item_row(label: &str, age: &str) -> String {
        format!(" {NEEDS_YOU} {label}{}{age}", " ".repeat(41 - 2 - label.chars().count() - age.chars().count()))
    }

    #[test]
    fn with_no_item_the_section_is_its_header_pinned_to_the_bottom() {
        let rows = text_rows(&draw_now(&mut with_inbox(0), 43, 16));
        assert_eq!(rows[15], header(0));
        assert_eq!(rows[14], "", "no rows");
        assert_eq!(rows[1], format!(" ▾ {NEEDS_YOU} alpha"), "the tree above is where it was");
    }

    #[test]
    fn three_items_are_three_rows_with_the_age_at_the_right_edge() {
        let rows = text_rows(&draw_now(&mut with_inbox(3), 43, 16));
        assert_eq!(rows[12], header(3));
        assert_eq!(rows[13], item_row("f-blocked: Worker question", "42s"));
        assert_eq!(rows[14], item_row("acme/api#12 checks failed", "5m"));
        assert_eq!(rows[15], item_row("ship: Job c is stuck", "3h"));
        assert_eq!(rows[13].chars().count(), 42, "the age ends in column 41, the last of the inner area");
    }

    #[test]
    fn five_items_show_three_and_plus_two_more_and_a_click_expands_and_folds_inline() {
        let mut view = with_inbox(5);
        let rows = text_rows(&draw_now(&mut view, 43, 16));
        assert_eq!(rows[11], header(5));
        assert_eq!(rows[14], item_row("ship: Job c is stuck", "3h"));
        assert_eq!(rows[15], " +2 more");
        assert_eq!(click(&mut view, 20, 15), Effect::None, "a view-state change: no command");
        let rows = text_rows(&draw_now(&mut view, 43, 16));
        assert_eq!(rows[9], header(5));
        assert_eq!(rows[13], item_row("engine: store busy", "2d"));
        assert_eq!(rows[14], item_row("ship lead has a reply", "10m"));
        assert_eq!(rows[15], " − less");
        assert_eq!(click(&mut view, 3, 15), Effect::None);
        let rows = text_rows(&draw_now(&mut view, 43, 16));
        assert_eq!((rows[11].clone(), rows[15].clone()), (header(5), " +2 more".to_owned()), "folded again");
    }

    #[test]
    fn an_expanded_list_squeezes_the_tree_which_scrolls_to_keep_the_cursor_visible() {
        let mut view = with_inbox(5);
        press(&mut view, KeyCode::Down);
        view.state.select(vec!["beta".into(), "rel".into()]);
        draw_now(&mut view, 43, 14);
        click(&mut view, 20, 13);
        let rows = text_rows(&draw_now(&mut view, 43, 14));
        assert_eq!(rows[7], header(5), "{rows:?}");
        assert!(rows[1..7].iter().any(|r| r.starts_with(&format!("   └ {NEEDS_YOU} rel"))), "the cursor's Session is in the rows left: {rows:?}");
        assert!(!rows[1..7].iter().any(|r| r.contains("alpha")), "the top of the tree scrolled away: {rows:?}");
    }

    #[test]
    fn a_label_too_long_for_its_row_is_cut_with_an_ellipsis_before_the_age() {
        let mut view = with_inbox(1);
        view.tree.inbox[0].label = "f-blocked: a very long question that cannot fit the row".into();
        let rows = text_rows(&draw_now(&mut view, 43, 16));
        let label = "f-blocked: a very long question th…"; // 41 - 2 (dot) - 1 (space) - 3 (age) = 35 columns
        assert_eq!(label.chars().count(), 35);
        assert_eq!(rows[15], format!(" {NEEDS_YOU} {label} 42s"));
        view.tree.inbox[0].label = "x".repeat(33);
        let rows = text_rows(&draw_now(&mut view, 43, 16));
        assert_eq!(rows[15], format!(" {NEEDS_YOU} {}   42s", "x".repeat(33)), "a label that fits is not cut");
    }

    #[test]
    fn the_age_follows_the_clock_with_no_new_snapshot() {
        let mut view = with_inbox(1);
        let draw_at = |view: &mut View, text: &str| {
            let mut terminal = Terminal::new(TestBackend::new(43, 16)).unwrap();
            let at = OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339).unwrap();
            terminal.draw(|f| view.draw_at(f, 0, at)).unwrap();
            text_rows(terminal.backend().buffer())[15].clone()
        };
        let ends = |row: String| row.trim_end().rsplit(' ').next().unwrap().to_owned();
        assert_eq!(ends(draw_at(&mut view, "2026-01-02T23:59:19.000Z")), "1s");
        assert_eq!(ends(draw_at(&mut view, "2026-01-02T23:59:58.000Z")), "40s");
        assert_eq!(ends(draw_at(&mut view, "2026-01-03T00:00:18.000Z")), "1m");
        assert_eq!(ends(draw_at(&mut view, "2026-01-03T00:00:00.000Z")), "42s");
    }

    #[test]
    fn the_dots_have_the_urgency_colours_and_the_ages_the_grey() {
        let buf = draw_now(&mut with_inbox(5), 43, 16);
        let colour = |x, y| buf[(x, y)].fg;
        let dot = |dot: crate::read::Dot| palette::dot_span(dot, 0, false).style.fg.unwrap();
        use crate::read::Dot;
        // rows 12.. are the items: red, red, yellow
        assert_eq!([colour(1, 12), colour(1, 13), colour(1, 14)], [dot(Dot::Red), dot(Dot::Red), dot(Dot::Yellow)]);
        assert_eq!(colour(40, 12), palette::color(palette::Meaning::Nothing), "the age");
        let buf = draw_now(&mut {
            let mut v = with_inbox(5);
            v.inbox_expanded = true;
            v
        }, 43, 16);
        assert_eq!((buf[(1, 13)].fg, buf[(1, 14)].fg), (dot(Dot::Magenta), dot(Dot::White)));
    }

    #[test]
    fn a_click_on_each_kind_runs_its_action() {
        let mut view = with_inbox(5);
        draw_now(&mut view, 43, 16);
        click(&mut view, 20, 15); // expand
        draw_now(&mut view, 43, 16);
        // rows 10.. are the five items: dialog, pr-failing, job-stuck, engine-error, unread
        assert_eq!(click(&mut view, 10, 10), Effect::Run(Action::Open(sid("f-blocked"))), "dialog: session open");
        assert_eq!(click(&mut view, 10, 11), Effect::None, "pr-failing: the label is a link, the view runs nothing");
        assert_eq!(click(&mut view, 10, 12), Effect::Run(Action::Open(sid("l-ship"))), "job-stuck: the Lead of the Plan");
        assert_eq!(click(&mut view, 10, 13), Effect::Logs(Some(9)), "engine-error: the logs at its Event");
        assert_eq!(click(&mut view, 41, 14), Effect::Run(Action::Open(sid("l-ship"))), "unread: anywhere on the row, the age too");
        assert_eq!(click(&mut view, 10, 9), Effect::None, "the header");
        // the section's rows are not the tree's: the Session row above is its own
        assert_eq!(click(&mut view, 8, 2), Effect::Run(Action::Open(sid("l-ship"))));
    }

    #[test]
    fn a_click_on_a_link_kind_or_the_header_changes_nothing_and_a_question_opens_the_manager() {
        let mut view = with_inbox(2);
        draw_now(&mut view, 43, 16);
        let before = view.inbox_expanded;
        assert_eq!(click(&mut view, 10, 14), Effect::Run(Action::Open(sid("f-blocked"))));
        assert_eq!(click(&mut view, 10, 15), Effect::None, "a pr-failing's label is a link");
        assert_eq!(click(&mut view, 10, 13), Effect::None, "the header");
        assert_eq!(view.inbox_expanded, before);
        let mut question = item(6, InboxKind::Question, Urgency::Magenta, "alpha: a question", "2026-01-02T23:00:00.000Z");
        question.context = Some(ContextId::parse("alpha").unwrap());
        view.tree.inbox = vec![question];
        draw_now(&mut view, 43, 16);
        assert_eq!(click(&mut view, 10, 15), Effect::Run(Action::OpenManager(ContextId::parse("alpha").unwrap())));
    }

    #[test]
    fn a_click_on_an_archived_sessions_item_shows_its_history_and_an_engine_down_opens_the_logs_at_the_bottom() {
        let mut view = View::new(archived(), true);
        let mut unread = item(7, InboxKind::Unread, Urgency::White, "ship review has a reply", "2026-01-02T23:00:00.000Z");
        unread.session = Some(sid("f-blocked"));
        let down = item(8, InboxKind::EngineDown, Urgency::Red, "engine is down", "2026-01-02T23:00:00.000Z");
        view.tree.inbox = vec![down, unread];
        draw_now(&mut view, 43, 18);
        assert_eq!(click(&mut view, 10, 16), Effect::Logs(None));
        assert_eq!(click(&mut view, 10, 17), Effect::Run(Action::History(sid("f-blocked"))));
    }

    /// The row of the Inbox item `i`, as the last frame drew it.
    fn inbox_row(view: &View, i: u16) -> u16 {
        view.inbox_area.y + 1 + i
    }

    /// The labels of the open menu, as `menu_items` reads them from the drawn box.
    fn menu_labels(view: &mut View) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(43, 16)).unwrap();
        terminal.draw(|f| view.draw_at(f, 0, now())).unwrap();
        let buf = terminal.backend().buffer();
        let rect = view.menu.as_ref().expect("a menu is open").rect(view.pane);
        (rect.y + 1..rect.bottom() - 1)
            .map(|y| (rect.x + 1..rect.right() - 1).map(|x| buf[(x, y)].symbol()).collect::<String>().trim().to_owned())
            .collect()
    }

    /// Picks the entry `index` of the open menu with Enter.
    fn pick_entry(view: &mut View, index: usize) -> Effect {
        for _ in 0..index {
            press(view, KeyCode::Down);
        }
        press(view, KeyCode::Enter)
    }

    #[test]
    fn a_right_click_on_an_item_row_opens_its_four_entries_and_each_runs_its_command() {
        let mut view = with_inbox(3);
        draw_now(&mut view, 43, 16);
        let row = inbox_row(&view, 1); // the pr-failing, item 2
        view.done(Err("why".into()));
        assert_eq!(right_click(&mut view, 10, row), Effect::None);
        assert_eq!(view.note, "", "a right click clears the note");
        assert_eq!(menu_labels(&mut view), ["Snooze 15 min", "Snooze 1 h", "Snooze until tomorrow 09:00", "Dismiss"]);
        let expected = [
            Action::Snooze { item: 2, until: Until::Minutes15 },
            Action::Snooze { item: 2, until: Until::Hour1 },
            Action::Snooze { item: 2, until: Until::Tomorrow },
            Action::Dismiss(2),
        ];
        for (i, action) in expected.into_iter().enumerate() {
            right_click(&mut view, 10, row);
            assert_eq!(pick_entry(&mut view, i), Effect::Run(action), "entry {i}");
            assert!(view.menu.is_none() && view.prompt.is_none());
        }
        // a click on an entry picks it too, and Esc closes the menu running nothing
        right_click(&mut view, 10, row);
        let rect = view.menu.as_ref().unwrap().rect(view.pane);
        assert_eq!(click(&mut view, rect.x + 2, rect.y + 4), Effect::Run(Action::Dismiss(2)));
        right_click(&mut view, 10, row);
        assert_eq!(press(&mut view, KeyCode::Esc), Effect::None);
        assert!(view.menu.is_none());
    }

    #[test]
    fn the_header_and_the_more_row_have_no_menu_and_engine_down_has_none() {
        let mut view = with_inbox(5);
        draw_now(&mut view, 43, 16);
        let header = view.inbox_area.y;
        assert_eq!(right_click(&mut view, 10, header), Effect::None, "the header");
        assert!(view.menu.is_none());
        let more = inbox_row(&view, 3);
        assert_eq!(right_click(&mut view, 10, more), Effect::None, "`+2 more`");
        assert!(view.menu.is_none());
        let mut down = item(9, InboxKind::EngineDown, Urgency::Red, "engine is down", "2026-01-02T23:00:00.000Z");
        down.id = 9;
        view.tree.inbox.insert(0, down);
        draw_now(&mut view, 43, 16);
        let (first, second) = (inbox_row(&view, 0), inbox_row(&view, 1));
        assert_eq!(right_click(&mut view, 10, first), Effect::None);
        assert!(view.menu.is_none(), "engine-down offers no menu");
        right_click(&mut view, 10, second);
        assert!(view.menu.is_some(), "the next row does");
    }

    /// Down presses until the cursor is in the Inbox, from the first Session.
    fn into_the_inbox(view: &mut View) {
        draw_now(view, 43, 16);
        for _ in 0..20 {
            press(view, KeyCode::Down);
            if view.inbox_cursor.is_some() {
                return;
            }
        }
        panic!("the cursor never reached the Inbox");
    }

    #[test]
    fn the_cursor_goes_from_the_last_session_into_the_item_rows_and_back() {
        let mut view = with_inbox(5);
        draw_now(&mut view, 43, 16);
        press(&mut view, KeyCode::Down); // shows the cursor
        let mut last = None;
        while view.inbox_cursor.is_none() {
            last = view.state.selected().to_vec().into();
            press(&mut view, KeyCode::Down);
        }
        assert_eq!(view.inbox_cursor, Some(0), "down from the last Session enters the first item");
        assert_eq!(Some(view.state.selected().to_vec()), last, "the tree's cursor stays on its last Session");
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Char('j'));
        press(&mut view, KeyCode::Down);
        assert_eq!(view.inbox_cursor, Some(2), "three items shown: it stops on the last of them");
        press(&mut view, KeyCode::Up);
        press(&mut view, KeyCode::Char('k'));
        assert_eq!(view.inbox_cursor, Some(0));
        press(&mut view, KeyCode::Up);
        assert_eq!(view.inbox_cursor, None, "up from the first item is back on the last Session");
        assert_eq!(Some(view.state.selected().to_vec()), last);
        // the toggle row and the header are skipped; expanded, the cursor reaches every item
        let toggle = inbox_row(&view, 3);
        click(&mut view, 20, toggle);
        draw_now(&mut view, 43, 16);
        press(&mut view, KeyCode::Down);
        for _ in 0..9 {
            press(&mut view, KeyCode::Down);
        }
        assert_eq!(view.inbox_cursor, Some(4));
        // folded again, it is on the last shown item
        let toggle = inbox_row(&view, 5);
        click(&mut view, 20, toggle);
        assert_eq!(view.inbox_cursor, Some(2));
    }

    #[test]
    fn the_item_cursor_is_drawn_inverted_across_its_row_only_while_it_shows() {
        let mut view = with_inbox(3);
        into_the_inbox(&mut view); // the dialog: its label is no link (a link's cells are written in pieces)
        let buf = draw_now(&mut view, 43, 16);
        let reversed = |y: u16| (1..42).all(|x| buf[(x, y)].modifier.contains(Modifier::REVERSED));
        assert!(reversed(inbox_row(&view, 0)));
        assert!(!reversed(inbox_row(&view, 1)) && !reversed(inbox_row(&view, 2)));
        let tree_row_reversed = (1..16).any(|y| y < view.inbox_area.y && buf[(5, y)].modifier.contains(Modifier::REVERSED));
        assert!(!tree_row_reversed, "no Session row is inverted while the cursor is in the Inbox");
        // a click hides it; the first arrow shows it again where it was
        let header = view.inbox_area.y;
        click(&mut view, 20, header);
        let buf = draw_now(&mut view, 43, 16);
        // (row 12 is the header, whose title is inverted by design)
        let lit: Vec<_> = (0..16).filter(|&y| y != 12).flat_map(|y| (0..43).map(move |x| (x, y))).filter(|&(x, y)| buf[(x, y)].modifier.contains(Modifier::REVERSED)).collect();
        assert!(lit.is_empty(), "{lit:?}");
        assert_eq!(view.inbox_cursor, Some(0));
        press(&mut view, KeyCode::Down);
        assert_eq!(view.inbox_cursor, Some(0), "it only shows again");
        press(&mut view, KeyCode::Down);
        assert_eq!(view.inbox_cursor, Some(1));
        // focus lost hides it too
        view.focus(false);
        assert_eq!(press(&mut view, KeyCode::Char('d')), Effect::None);
    }

    #[test]
    fn d_dismisses_the_item_under_the_cursor_at_once_and_s_opens_the_snooze_choices() {
        let mut view = with_inbox(3);
        into_the_inbox(&mut view);
        press(&mut view, KeyCode::Down); // the second item: id 2
        assert_eq!(press(&mut view, KeyCode::Char('d')), Effect::Run(Action::Dismiss(2)));
        assert!(view.menu.is_none() && view.prompt.is_none());
        assert_eq!(press(&mut view, KeyCode::Char('s')), Effect::None);
        draw_now(&mut view, 43, 16);
        assert_eq!(menu_labels(&mut view), ["Snooze 15 min", "Snooze 1 h", "Snooze until tomorrow 09:00"]);
        assert_eq!(pick_entry(&mut view, 1), Effect::Run(Action::Snooze { item: 2, until: Until::Hour1 }));
        for (i, until) in [Until::Minutes15, Until::Hour1, Until::Tomorrow].into_iter().enumerate() {
            press(&mut view, KeyCode::Char('s'));
            assert_eq!(pick_entry(&mut view, i), Effect::Run(Action::Snooze { item: 2, until }));
        }
        press(&mut view, KeyCode::Char('s'));
        assert_eq!(press(&mut view, KeyCode::Esc), Effect::None);
        assert!(view.menu.is_none());
        // the menu takes every key, so `d` in it dismisses nothing
        press(&mut view, KeyCode::Char('s'));
        assert_eq!(press(&mut view, KeyCode::Char('d')), Effect::None);
        assert!(view.menu.is_some());
    }

    #[test]
    fn s_and_d_do_nothing_on_a_session_row_and_n_and_x_do_nothing_on_an_item() {
        let mut view = on(&["alpha", "ship"]);
        draw_now(&mut view, 43, 16);
        view.tree.inbox = five();
        draw_now(&mut view, 43, 16);
        for key in ['s', 'd'] {
            assert_eq!(press(&mut view, KeyCode::Char(key)), Effect::None, "{key}");
            assert!(view.menu.is_none() && view.prompt.is_none());
        }
        // before any arrow key none shows, neither on an item
        let mut view = with_inbox(3);
        draw_now(&mut view, 43, 16);
        view.inbox_cursor = Some(0);
        for key in ['s', 'd'] {
            assert_eq!(press(&mut view, KeyCode::Char(key)), Effect::None, "{key}: the cursor is hidden");
        }
        let mut view = with_inbox(3);
        into_the_inbox(&mut view);
        for key in ['n', 'x'] {
            press(&mut view, KeyCode::Char(key));
            assert!(view.prompt.is_none(), "{key}: the cursor is not on a Session");
        }
    }

    #[test]
    fn enter_on_an_item_jumps_as_a_click_does_and_the_refusal_shows_in_the_note() {
        let mut view = with_inbox(5);
        draw_now(&mut view, 43, 16);
        press(&mut view, KeyCode::Down);
        while view.inbox_cursor.is_none() {
            press(&mut view, KeyCode::Down);
        }
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::Open(sid("f-blocked"))), "dialog");
        press(&mut view, KeyCode::Down);
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None, "pr-failing: a link, Enter does nothing");
        press(&mut view, KeyCode::Down);
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::Open(sid("l-ship"))), "job-stuck: the Lead");
        view.tree.inbox[2].kind = InboxKind::EngineError;
        view.tree.inbox[2].event = Some(3);
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::Logs(Some(3)));
        view.done(Err(Action::Dismiss(2).failed("item_closed", "Item 2 is closed.")));
        assert_eq!(view.note, "item_closed: Item 2 is closed.");
        let rows = text_rows(&draw_now(&mut view, 43, 17));
        assert_eq!(rows[16], " item_closed: Item 2 is closed.");
    }

    #[test]
    fn a_snapshot_keeps_the_cursor_on_its_item_or_the_one_at_its_place_and_none_leaves_the_tree() {
        let mut view = with_inbox(5);
        into_the_inbox(&mut view);
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Down);
        assert_eq!(view.inbox_cursor, Some(2));
        let mut tree = sample();
        tree.inbox = five().into_iter().skip(1).collect(); // item 1 left: item 3 is at 1 now
        view.refresh(tree);
        assert_eq!(view.inbox_cursor, Some(1));
        let mut tree = sample();
        tree.inbox = five().into_iter().skip(2).take(1).collect(); // only item 3 is left; it is at 0
        view.refresh(tree);
        assert_eq!(view.inbox_cursor, Some(0));
        let mut tree = sample();
        tree.inbox = Vec::new();
        view.refresh(tree);
        assert_eq!(view.inbox_cursor, None, "no item: the cursor is a Session's again");
        press(&mut view, KeyCode::Up);
        assert!(view.cursor_shown());
    }

    #[test]
    fn with_no_session_the_arrow_reaches_the_items() {
        let mut view = View::new(Tree { contexts: Vec::new(), events_cursor: 0, inbox: five(), engine_running: true }, true);
        draw_now(&mut view, 43, 16);
        press(&mut view, KeyCode::Down);
        assert_eq!(view.inbox_cursor, Some(0));
        assert_eq!(press(&mut view, KeyCode::Char('d')), Effect::Run(Action::Dismiss(1)));
    }

    #[test]
    fn the_section_leaves_the_tree_a_row_in_a_short_pane_and_the_footer_stays_below_it() {
        let mut view = with_inbox(5);
        let rows = text_rows(&draw_now(&mut view, 43, 5));
        assert_eq!(rows[0], "");
        assert!(rows[1].starts_with(&format!(" ⏵ ├ {WORKING} ship")), "one row for the tree, scrolled to the cursor: {rows:?}");
        assert_eq!(rows[2], header(5), "{rows:?}");
        view.done(Err("closed".into()));
        let rows = text_rows(&draw_now(&mut view, 43, 8));
        assert_eq!(rows[7], " closed");
        assert_eq!(rows[2], header(5), "the section is above the note: {rows:?}");
    }
}
