//! The tree view's state and keys (the reference's `run_context`, without the
//! terminal): what a key, a click or a focus change does, and how the view is
//! drawn. Pure: an action to run is returned as an [`Effect`], and the caller
//! runs it, then hands the result to [`View::done`].
//!
//! Keys (commands.md §13.2): ↑↓ and j/k move the cursor, Enter opens the cursor's Session, `n` starts a new Plan,
//! `x` closes a Session's pane (both ask in a dialog popup, as the menu's `…`, Close and Archive do; the view has no
//! inline prompt), `?` opens the legend. No other key does anything. A right click opens the menu
//! ([`super::menu`]), which then takes every key and click.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::widgets::{Paragraph, Wrap};
use tui_tree_widget::TreeState;

use super::actions::{Action, Deletable, Failure, Free, IN_CHAT_SLOT, Subject};
use super::cursor::{self, Input};
use super::folds::{Folds, FoldsFile};
use super::menu::{self, Item, Kind, Menu};
use super::ask::{Ask, Question};
use super::rows::{self, Hits, Id, Row};
use cpd_core::home::CpdHome;
use cpd_core::ids::SessionId;
use cpd_core::ports::Ports;
use cpd_core::session::State;
use serde_json::Value;
use crate::palette;
use crate::read::{ContextNode, Dot, SessionNode, Tree};

/// The engine has been down this long, on every snapshot, when the tree speaks: past the supervisor's 15 s wait and its
/// retries (contract §6.6, hs-d363).
const DOWN_FOR: Duration = Duration::from_secs(60);
/// What the Managers hear of a downtime.
const DOWN_TOLD: &str = "The cpd engine has been down for 60 s and its supervisor has not restarted it; see run/watch.log, start it with cpd watch --ensure.";

/// The longest name of a Session (commands.md §1.4).
/// Rows a wheel notch scrolls the tree by.
const WHEEL_ROWS: usize = 3;
const NAME_MAX: usize = 80;
const FORK_SUFFIX: &str = " (fork)";

/// The popup of Fork… and New session…: the name of a fork of `source`, starting with `<label> (fork)` (the label cut
/// so that the name is not too long).
fn fork_of(title: &'static str, source: &SessionNode, label: &str) -> Question {
    let label: String = label.chars().take(NAME_MAX - FORK_SUFFIX.len()).collect();
    Question::input(title, Ask::Fork { source: source.id.clone() }, format!("{label}{FORK_SUFFIX}"))
}

/// The popup of Rename… on `row`, starting with its current label.
fn rename_of(row: &Row<'_>) -> Option<Question> {
    let ask = match *row {
        Row::Context(c) => Ask::RenameContext(c.id.clone()),
        Row::Plan(_, p) => Ask::RenamePlan(p.head.id.clone()),
        Row::Fork(_, _, s) | Row::Manager(_, s) => Ask::RenameSession(s.id.clone()),
        Row::Toggle | Row::NewPlan(_) | Row::Archive => return None,
    };
    Some(Question::input("Rename", ask, row.label()))
}

/// A refusal the User must see: the `(title, text)` of its `text` dialog.
type Refusal = (&'static str, &'static str);

/// What `x` and the menu's Close do on `row`: the close, or the refusal (the Session is in the chat slot);
/// nothing on a row with no Session.
fn close_of(row: Row<'_>) -> Option<Result<Action, Refusal>> {
    let session = row.session()?;
    if session.is_active() {
        return Some(Err(("Close Session", IN_CHAT_SLOT)));
    }
    Some(Ok(Action::Close(session.id.clone())))
}

/// The effect of `x` and the menu's Close: the command at once, or the refusal in a `text` dialog.
fn close_effect(closed: Option<Result<Action, Refusal>>) -> Effect {
    match closed {
        Some(Ok(action)) => Effect::Run(action),
        Some(Err((title, text))) => Effect::text(title, text),
        None => Effect::None,
    }
}

/// The object an archive command of the row names: the Context, the Plan, or the fork; none for a toggle row.
fn subject_of(row: &Row<'_>) -> Option<Subject> {
    match *row {
        Row::Context(c) => Some(Subject::Context(c.id.clone())),
        Row::Plan(_, p) => Some(Subject::Plan(p.head.id.clone())),
        Row::Fork(_, _, s) | Row::Manager(_, s) => Some(Subject::Session(s.id.clone())),
        Row::Toggle | Row::NewPlan(_) | Row::Archive => None,
    }
}

/// What a delete of `row` is about: its Plan or Context; none for a fork or a toggle row, or a row that `plan delete` and
/// `context delete` would not take yet (they refuse the rest with `archive_blocked`).
fn deletable_of(row: &Row<'_>) -> Option<Deletable> {
    match *row {
        Row::Context(c) if row.deletable() => Some(Deletable::Context(c.id.clone())),
        Row::Plan(_, p) if row.deletable() => Some(Deletable::Plan(p.head.id.clone())),
        _ => None,
    }
}

/// What the caller does after an input.
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Run this command, then call [`View::finished`] with the result.
    Run(Action),
    /// Open this dialog popup, then run the command its answer makes, if any ([`Question::ask`]); a failure is logged.
    Ask(Question),
    /// Open the legend popup (`?`); a failure is logged.
    Legend,
    /// Open a `text` dialog popup ([`crate::dialog::show_error`]): a failure or refusal the User must see (commands.md
    /// §17 "No notes"). `title` says what failed, `text` why (`<code>: <message>`).
    Text { title: String, text: String },
}

impl Effect {
    fn text(title: impl Into<String>, text: impl Into<String>) -> Effect {
        Effect::Text { title: title.into(), text: text.into() }
    }
}

pub struct View {
    pub tree: Tree,
    pub state: TreeState<Id>,
    known: HashSet<Vec<Id>>,
    /// The file the folds are kept in (hs-d260); none in a view that keeps them for its own life only.
    folds: Option<FoldsFile>,
    /// Keyboard focus of this pane, from the terminal's focus events; the cursor shows only while focused.
    focused: bool,
    /// The cursor stays hidden until the first arrow key.
    last_input: Input,
    /// The right-click menu, while it is open.
    menu: Option<Menu>,
    /// The inner area of the last frame (the pane less a blank column on each side, below the blank top row): the
    /// menu is clamped inside it and hit-tested against it, and its `x` is the column of the tree's first cell.
    pane: Rect,
    /// The tree's scroll: the wheel and the cursor move it.
    tree_scroll: rows::Scroll,
    /// Where the last frame drew the tree and its cue rows (the gap below them is not it): the wheel there scrolls it.
    tree_area: Rect,
    /// The last failed snapshot's error (one row), until a snapshot succeeds: a lasting state of the view, drawn as its
    /// footer (commands.md §17 "No notes", rule (b)).
    pub error: Option<String>,
    /// `$CPD_HOME/policy`: the tree whose known owners (`<owner>/owner.toml`) New Context… chooses the owner from (none when the home is unresolved).
    pub(crate) policy_dir: Option<PathBuf>,
    /// The engine's downtime now: since when `locks/watch.lock` has been read free on every snapshot, and whether the
    /// Managers were told of it (view state only).
    down: Option<Down>,
    /// Whether the Context and Plan rows draw their cost (pro-d189, pro-d196): off at every start, flipped by `$`, never stored.
    costs: bool,
}

/// One downtime of the engine (see [`View::down`]).
struct Down {
    since: Instant,
    spoke: bool,
}

/// The refusal of New Context… when the instance has no owner policy.
const NO_OWNER_POLICY: &str = "No owner policy: add $CPD_HOME/policy/<owner>/owner.toml";

impl View {
    pub fn new(tree: Tree, focused: bool) -> View {
        let mut view = View {
            tree: Tree { contexts: Vec::new(), events_cursor: 0, inbox: Vec::new(), engine_running: true },
            state: TreeState::default(),
            known: HashSet::new(),
            folds: None,
            focused,
            last_input: Input::Click,
            costs: false,
            menu: None,
            pane: Rect::default(),
            tree_scroll: rows::Scroll::default(),
            tree_area: Rect::default(),
            error: None,
            policy_dir: CpdHome::from_env().ok().map(|h| cpd_core::policy::policy_dir(&h)),
            down: None,
        };
        view.refresh(tree);
        view
    }

    /// Names every dot the tree draws as a spinner: a Plan's Sessions and a Context row's Manager dot, so the pane is
    /// redrawn with the clock. (Archived or asleep rows never turn; one scrolled or folded out of sight counts, the
    /// cost being a redraw.)
    pub fn animated(&self) -> bool {
        let turns = |dot: Dot| matches!(dot, Dot::Working(_) | Dot::Fixing);
        let mut sessions = self.tree.contexts.iter().flat_map(|c| &c.plans).flat_map(|p| &p.sessions);
        sessions.any(|s| turns(s.dot) && !s.is_archived())
            || self.tree.contexts.iter().any(|c| turns(c.dot) && !c.is_archived() && !c.manager_asleep)
    }

    /// A new snapshot: keep the cursor on its Session.
    pub fn refresh(&mut self, tree: Tree) {
        let before = rows::selected_index(&self.state, &self.tree);
        self.tree = tree;
        rows::open_new(&mut self.state, &mut self.known, &self.tree);
        rows::reselect(&mut self.state, &self.tree, before);
    }

    /// Starts from the folds in `file` and keeps them there from now on (commands.md §13.2, hs-d260). A file that cannot
    /// be read or is not valid leaves the defaults and is the log line `fold state reset: <reason>` (returned; the view
    /// draws nothing); the next write replaces it.
    pub fn keep_folds(&mut self, mut file: FoldsFile) -> Vec<String> {
        let mut lines = Vec::new();
        match file.load() {
            Ok(stored) => stored.merge(&mut self.state, &mut self.known),
            Err(reason) => lines.push(format!("fold state reset: {reason}")),
        }
        // the file is as it was until a fold changes
        file.assume(Folds::of(&self.state, &self.tree));
        self.folds = Some(file);
        lines
    }

    /// Writes the folds when they have changed (at most once a second); a failure is the log line returned, and is retried.
    #[cfg(test)]
    pub fn save_folds(&mut self) -> Option<String> {
        self.save_folds_with(&Ports::real())
    }

    /// [`View::save_folds`] with the second counted on the clock of `ports`.
    pub fn save_folds_with(&mut self, ports: &Ports) -> Option<String> {
        let file = self.folds.as_mut()?;
        file.save_with(ports, &Folds::of(&self.state, &self.tree)).err()
    }

    fn cursor_shown(&self) -> bool {
        cursor::shown(self.focused, self.last_input, !self.state.selected().is_empty())
    }

    pub fn focus(&mut self, gained: bool) {
        self.focused = gained;
        if !gained {
            self.last_input = Input::Click;
        }
    }

    /// The tree never runs `cpd watch --ensure` by itself: the supervisor restarts a stopped engine (contract §6.6,
    /// hs-d328, hs-d363). It only speaks: when `locks/watch.lock` has been read free on every snapshot for [`DOWN_FOR`]
    /// (`self.tree.engine_running` is false at each call since `now`), it tells the most recently active live Manager once
    /// per downtime (`run` with [`Action::Tell`], `managers` lists them). It draws nothing (commands.md §17 "No notes": the
    /// Inbox's `engine-down` item and the Managers' message say it); it returns the lines for the view's log
    /// (`run/tui/tree.log`), a failure to list or tell among them; never writes to the terminal.
    pub fn watch_engine(
        &mut self,
        now: Instant,
        run: &mut dyn FnMut(&Action) -> Result<Value, Failure>,
        managers: &mut dyn FnMut() -> Result<Vec<SessionId>, String>,
    ) -> Vec<String> {
        if self.tree.engine_running {
            self.down = None;
            return Vec::new();
        }
        let down = self.down.get_or_insert(Down { since: now, spoke: false });
        if down.spoke || now.saturating_duration_since(down.since) < DOWN_FOR {
            return Vec::new();
        }
        down.spoke = true;
        let mut lines = vec![format!("engine down {} s+: telling the Manager", DOWN_FOR.as_secs())];
        match managers() {
            Err(why) => lines.push(format!("listing the Managers failed: {why}")),
            Ok(ids) if ids.is_empty() => lines.push("no live Manager to tell".to_owned()),
            Ok(ids) => {
                for manager in ids {
                    match run(&Action::Tell { manager: manager.clone(), text: DOWN_TOLD.to_owned() }) {
                        Ok(_) => lines.push(format!("told {manager}")),
                        Err(why) => lines.push(format!("telling {manager} failed: {}: {}", why.code(), why.message())),
                    }
                }
            }
        }
        lines
    }

    /// What the view does with the `result` of `action` (commands.md §17 "No notes"): a success says nothing; a failure
    /// is a `text` dialog, [`Effect::Text`]: the command is what failed, `<code>: <message>` is why.
    pub fn finished(&mut self, action: &Action, result: Result<String, String>) -> Effect {
        match result {
            Ok(_) => Effect::None,
            Err(why) => Effect::text(action.argv().join(" "), why),
        }
    }

    /// The cursor's row, while the cursor shows: Enter, `n` and `x` act on it.
    fn cursor_row(&self) -> Option<Row<'_>> {
        rows::row(&self.tree, self.state.selected()).filter(|r| self.cursor_shown() && !r.archived())
    }

    pub fn key(&mut self, key: KeyEvent) -> Effect {
        if key.kind != KeyEventKind::Press {
            return Effect::None;
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
                if let Some((row, plan)) = self.cursor_row().and_then(|r| r.session().and(r.plan()).map(|p| (r, p))) {
                    let ask = Ask::NamedPlan { context: plan.head.context.clone(), label: row.label() };
                    return Effect::Ask(Question::input("New Plan", ask, String::new()));
                }
            }
            KeyCode::Char('x') => {
                return close_effect(self.cursor_row().and_then(close_of));
            }
            KeyCode::Char('?') => return Effect::Legend,
            // like `?`, it needs no cursor and leaves the cursor's visibility alone
            KeyCode::Char('$') => self.costs = !self.costs,
            // the first arrow after a click only shows the cursor again where it was (the first Session when there is none)
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Up | KeyCode::Char('k') => {
                let delta = if matches!(key.code, KeyCode::Down | KeyCode::Char('j')) { 1 } else { -1 };
                self.arrow(delta);
                self.last_input = Input::Arrow;
            }
            KeyCode::Enter => {
                if let Some(session) = self.cursor_row().and_then(|r| r.session()) {
                    return Effect::Run(Action::Open(session.id.clone()));
                }
            }
            _ => {}
        }
        Effect::None
    }

    /// An arrow key (`delta` 1 down, -1 up) while the cursor shows: it moves through the Sessions; when it does not show, the
    /// key only shows it again where it was (on the first Session when it has none).
    fn arrow(&mut self, delta: isize) {
        if self.cursor_shown() {
            rows::move_cursor(&mut self.state, &self.tree, delta);
        } else if self.state.selected().is_empty() {
            rows::move_cursor(&mut self.state, &self.tree, 1);
        }
    }

    pub fn mouse(&mut self, m: MouseEvent) -> Effect {
        let at = Position::new(m.column, m.row);
        if let Some(menu) = self.menu.as_mut() {
            // an open menu takes every click: a left click picks, any button outside closes
            return match menu.mouse(self.pane, m) {
                menu::Outcome::Pick(item) => self.pick(item),
                menu::Outcome::Close => {
                    self.menu = None;
                    Effect::None
                }
                menu::Outcome::Stay => Effect::None,
            };
        }
        match m.kind {
            // the wheel over the tree scrolls its rows, three rows a notch, as the Plan view; elsewhere it does nothing
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp if self.tree_area.contains(at) => {
                self.tree_scroll.wheel(m.kind == MouseEventKind::ScrollDown, WHEEL_ROWS);
                Effect::None
            }
            MouseEventKind::Down(MouseButton::Left) => self.click(m),
            MouseEventKind::Down(MouseButton::Right) => {
                self.open_menu(at);
                Effect::None
            }
            _ => Effect::None,
        }
    }

    /// A right click at `at`: the menu of the row there, of the empty area, or none (a toggle row).
    fn open_menu(&mut self, at: Position) {
        let path = self.row_at(at);
        let row = path.as_deref().and_then(|p| rows::row(&self.tree, p));
        let deletable = row.as_ref().is_some_and(Row::deletable);
        self.menu = Kind::of(row).map(|kind| Menu::new(kind, path.unwrap_or_default(), at, deletable));
    }

    /// The item picked in the menu, which is closed: the command to run, a popup to ask first, or the refusal.
    fn pick(&mut self, item: Item) -> Effect {
        let Some(menu) = self.menu.take() else { return Effect::None };
        if item == Item::NewContext {
            let owners: Vec<String> =
                self.policy_dir.as_deref().map(cpd_core::policy::known_owners).unwrap_or_default().iter().map(|o| o.as_str().to_owned()).collect();
            if owners.is_empty() {
                return Effect::text("New Context", NO_OWNER_POLICY);
            }
            let taken = self.tree.contexts.iter().map(|c| c.id.to_string()).collect();
            return Effect::Ask(Question::NewContext { taken, owners });
        }
        // the row may be gone, with a snapshot taken while the menu was open
        let Some(row) = rows::row(&self.tree, &menu.path) else { return Effect::None };
        match item {
            Item::Open => row.session().map_or(Effect::None, |s| Effect::Run(Action::Open(s.id.clone()))),
            Item::Close => close_effect(close_of(row)),
            Item::Archive => {
                let Some(subject) = subject_of(&row) else { return Effect::None };
                match self.free_for(&row, &menu.path) {
                    Some(free) => Effect::Run(Action::Archive { subject, free }),
                    None => Effect::None,
                }
            }
            Item::Unarchive => subject_of(&row).map_or(Effect::None, |subject| Effect::Run(Action::Unarchive(subject))),
            Item::Delete => match deletable_of(&row) {
                Some(what) => Effect::Ask(Question::delete(what, &row.label())),
                None => Effect::None,
            },
            Item::Rename => self.ask(rename_of(&row).map(Ok)),
            Item::Fork | Item::NewSession => {
                let title = if item == Item::Fork { "Fork" } else { "New session" };
                self.ask(row.session().map(|source| Ok(fork_of(title, source, &row.label()))))
            }
            Item::NewPlan => {
                let asked = match row {
                    Row::Context(c) => Some(Ok(self.new_plan_in(c))),
                    _ => None,
                };
                self.ask(asked)
            }
            Item::CopyId => row.shown_id().map_or(Effect::None, |id| Effect::Run(Action::CopyId(id))),
            Item::NewContext => Effect::None,
        }
    }

    /// How the archive of `row` (drawn at `path`) frees the chat slot (F3, §13.2): `Some(None)` when none of the
    /// Sessions it archives holds it. Else the next live Session in tree order (the rows as drawn, the archived ones
    /// and the toggle rows skipped, and those the archive closes), else the previous one, else a park. `None` when the
    /// row is not drawn (a snapshot took it away while the menu was open).
    fn free_for(&self, row: &Row<'_>, path: &[Id]) -> Option<Option<Free>> {
        let archiving: Vec<&SessionNode> = match *row {
            Row::Context(c) => c.plans.iter().flat_map(|p| &p.sessions).collect(),
            Row::Plan(_, p) => p.sessions.iter().collect(),
            Row::Fork(_, _, s) | Row::Manager(_, s) => vec![s],
            Row::Toggle | Row::NewPlan(_) | Row::Archive => return Some(None),
        };
        let manager_in_chat = matches!(*row, Row::Context(c) if c.manager_place == Some(crate::read::Place::Chat));
        if !manager_in_chat && !archiving.iter().any(|s| s.is_active()) {
            return Some(None);
        }
        let drawn = rows::items(&self.tree, self.state.opened(), 0, 0);
        let anchor = drawn.iter().position(|f| f.path == path)?;
        let live: Vec<(usize, &SessionNode)> = drawn
            .iter()
            .enumerate()
            .filter_map(|(i, f)| {
                let row = rows::row(&self.tree, &f.path).filter(|r| !r.archived())?;
                let session = row.session().filter(|s| !archiving.iter().any(|a| a.id == s.id))?;
                Some((i, session))
            })
            .collect();
        let next = live.iter().find(|(i, _)| *i > anchor).or_else(|| live.iter().rev().find(|(i, _)| *i < anchor));
        Some(Some(next.map_or(Free::Park, |(_, s)| Free::Open(s.id.clone()))))
    }

    /// The popup of New Plan… in `context` (R34 A12: no repo).
    fn new_plan_in(&self, context: &ContextNode) -> Question {
        let taken = self.tree.contexts.iter().flat_map(|c| &c.plans).map(|p| p.head.id.to_string()).collect();
        Question::input("New Plan", Ask::NewPlan { context: context.id.clone(), taken }, String::new())
    }

    /// The popup to open, or the dialog that says why not.
    fn ask(&mut self, asked: Option<Result<Question, Refusal>>) -> Effect {
        match asked {
            Some(Ok(question)) => Effect::Ask(question),
            Some(Err((title, text))) => Effect::text(title, text),
            None => Effect::None,
        }
    }

    /// A left click.
    fn click(&mut self, m: MouseEvent) -> Effect {
        self.last_input = Input::Click;
        let at = Position::new(m.column, m.row);
        let Some(path) = self.row_at(at) else { return Effect::None };
        // only a closing row (`╰󰬦 expand`, `╰󰬬 collapse`) shows or hides a Context's archived rows; the title row keeps its own action:
        // a live Context's row opens its Manager (B3-A6), a live Session row opens it and an archived one shows its
        // history
        if rows::click_toggle(&mut self.state, &self.tree, &path) {
            return Effect::None;
        }
        // a live empty Context's `╰󰬪 new` row opens the same New Plan popup the context menu's New Plan… does (hs-d688)
        if let Some(Row::NewPlan(c)) = rows::row(&self.tree, &path) {
            return Effect::Ask(self.new_plan_in(c));
        }
        if let Some(Row::Context(c)) = rows::row(&self.tree, &path) {
            let open = Action::OpenManager(c.id.clone());
            return Effect::Run(if c.is_archived() { Action::Resume { unarchive: Subject::Context(c.id.clone()), then: Box::new(open) } } else { open });
        }
        match rows::row(&self.tree, &path) {
            Some(row) => match (row.session(), row.resume()) {
                (Some(_), Some((unarchive, open))) => Effect::Run(Action::Resume { unarchive, then: Box::new(open) }),
                (Some(s), None) => Effect::Run(Action::Open(s.id.clone())),
                (None, _) => Effect::None,
            },
            None => Effect::None,
        }
    }

    /// The row drawn at `at`, none outside the rows the last frame drew.
    fn row_at(&self, at: Position) -> Option<Vec<Id>> {
        let Hits { rows, paths } = self.tree_scroll.hits();
        let path = rows.contains(at).then(|| paths.get(usize::from(at.y - rows.y)))??;
        // an empty row between two Contexts is no row
        rows::row(&self.tree, path)?;
        Some(path.clone())
    }

    /// The text of the footer: the error of the last snapshot, if any (a lasting state, commands.md §17 "No notes").
    fn footer(&self) -> String {
        self.error.clone().unwrap_or_default()
    }

    /// Draw the ▲ line (the blank row), the tree, the ▼ line and (when the snapshot cannot be read) its error in the rows at the bottom: the
    /// reference's layout. One blank column is left on each side of all of it. `frame` is the animation frame of the working dots (`palette::frame`).
    pub fn draw(&mut self, f: &mut Frame, frame: usize) {
        let inner = f.area().inner(Margin::new(1, 0));
        let footer = self.footer();
        let width = inner.width.max(1) as usize;
        let wrapped = rows::width(&footer).div_ceil(width);
        let footer_height =
            if footer.is_empty() { 0 } else { (wrapped + usize::from(wrapped > 1)).max(1).min(inner.height as usize) as u16 };
        let [body, footer_area] = Layout::vertical([Constraint::Min(0), Constraint::Length(footer_height)]).areas(inner);
        if footer_height > 0 {
            f.render_widget(Paragraph::new(footer).style(palette::error()).wrap(Wrap { trim: false }), footer_area);
        }
        // the tree scrolls to keep the cursor's Session in view
        let cursor = self.cursor_shown();
        rows::draw(f, body, &self.tree, &mut self.state, cursor, frame, self.costs, &mut self.tree_scroll);
        let tree_rows = u16::try_from(rows::row_count(&self.state, &self.tree)).unwrap_or(u16::MAX);
        let list = rows::list_area(body);
        self.tree_area = Rect { height: list.height.min(tree_rows), ..list };
        self.pane = Rect { y: inner.y + inner.height.min(1), height: inner.height.saturating_sub(1), ..inner };
        if let Some(menu) = &self.menu {
            menu.draw(f, self.pane);
        }
    }
}

/// The layout test's sample (`layout_tests`): a right click at `click` in a `width` x `height` pane of an empty tree,
/// drawn by [`View::draw`] as in production; the menu's rect is the one the view hit-tests.
#[cfg(test)]
pub(crate) fn layout_sample(width: u16, height: u16, case: crate::layout_tests::Case) -> crate::layout_tests::Sample {
    use crate::layout_tests::{Case, Kind, Placed, Sample};
    use ratatui::backend::TestBackend;
    let Case::Menu(click) = case else { panic!("the tree samples a menu") };
    let (column, row) = click.at(width, height);
    let mut view = View::new(Tree { contexts: Vec::new(), events_cursor: 0, inbox: Vec::new(), engine_running: true }, true);
    let mut terminal = ratatui::Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| view.draw(f, 0)).unwrap();
    view.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Right), column, row, modifiers: ratatui::crossterm::event::KeyModifiers::NONE });
    terminal.draw(|f| view.draw(f, 0)).unwrap();
    let rect = view.menu.as_ref().expect("the right click opens a menu").rect(view.pane);
    let boxes = vec![Placed { name: "tree menu", kind: Kind::Full, rect, exempt: Vec::new() }];
    Sample { buf: terminal.backend().buffer().clone(), container: Rect::new(0, 0, width, height), boxes }
}

/// The layout test's sample (`layout_tests`): the tree's Contexts (the fixture's live ones and an archived one) in a
/// `width` x `height` pane, drawn by [`View::draw`] as in production. Placed is one Rail per Context, its rect from the
/// rows the view saved for hit-testing: the blank row above, the Context's rows, the blank row below. The rows whose
/// glyph sits against the rail by recorded design (`├─glyph`: hs-d337, hs-d339, hs-d484) are exempt by name; the fold is
/// not the widget, so it is exempt from B3 (`Rule::Minimum`).
#[cfg(test)]
pub(crate) fn layout_fold_sample(width: u16, height: u16, case: crate::layout_tests::TreeCase) -> crate::layout_tests::Sample {
    use crate::box_widget::spacing;
    use crate::layout_tests::{Exempt, Kind, Placed, Rule, Sample, TreeCase};
    use ratatui::backend::TestBackend;
    assert_eq!(case, TreeCase::ContextFold, "the fold samples the Context fold");
    let mut view = View::new(super::fixture::archived(), true);
    let mut terminal = ratatui::Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| view.draw(f, 0)).unwrap();
    let pane = Rect::new(0, 0, width, height); // what the tree column passed: the margin is the container's to leave
    let hits = view.tree_scroll.hits();
    // the rows of one Context run until the empty row between two Contexts (which is no row)
    let drawn: Vec<bool> = hits.paths.iter().map(|p| rows::row(&view.tree, p).is_some()).collect();
    let mut boxes = Vec::new();
    let mut start = 0;
    while start < drawn.len() {
        if !drawn[start] {
            start += 1;
            continue;
        }
        let end = (start..drawn.len()).find(|&i| !drawn[i]).unwrap_or(drawn.len());
        let top = hits.rows.y + start as u16 - 1; // the blank row above
        let rect = Rect { y: top, height: (end - start) as u16 + 2, ..spacing::contain(pane) };
        let exempt = (start + 1..end - 1)
            .map(|i| Exempt { name: "hs-d337 hs-d339 hs-d484: the branch ├─ sits against the rail", row: hits.rows.y + i as u16, rule: Rule::Padding })
            .chain((start..end).map(|i| Exempt { name: "pro-d186: the tree's rows end at the pane's inner edge", row: hits.rows.y + i as u16, rule: Rule::RightEdge }))
            .chain([Exempt { name: "hs-d484: the fold is not the widget (B3 does not apply)", row: top, rule: Rule::Minimum }])
            .collect();
        boxes.push(Placed { name: "context fold", kind: Kind::Rail, rect, exempt });
        start = end;
    }
    Sample { buf: terminal.backend().buffer().clone(), container: pane, boxes }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyModifiers, MouseEvent};
    use ratatui::style::Modifier;

    use super::super::fixture::{archived, sample};
    use super::*;
    use super::super::ask::Dialogs;
    use crate::palette::{IDLE, NEEDS_YOU, SPINNER, STOPPED};
    use crate::dialog::validate::Validator;
    use cpd_core::ids::{ContextId, PlanId, SessionId};

    const WORKING: &str = SPINNER[0];

    fn press(view: &mut View, code: KeyCode) -> Effect {
        view.key(KeyEvent::new(code, KeyModifiers::NONE))
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

    fn cid(s: &str) -> ContextId {
        ContextId::parse(s).unwrap()
    }

    fn pid(s: &str) -> PlanId {
        PlanId::parse(s).unwrap()
    }

    fn resume(unarchive: Subject, then: Action) -> Action {
        Action::Resume { unarchive, then: Box::new(then) }
    }

    fn drawn(view: &mut View, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| view.draw(f, 0)).unwrap();
        let buf = terminal.backend().buffer();
        (0..height).map(|y| (0..width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_owned()).collect()
    }

    #[test]
    fn the_tree_is_animated_only_while_a_live_working_dot_is_in_it() {
        assert!(View::new(sample(), true).animated(), "the sample's `ship` Lead is working");
        let each = |tree: &mut Tree, f: &dyn Fn(&mut SessionNode)| {
            tree.contexts.iter_mut().flat_map(|c| &mut c.plans).flat_map(|p| &mut p.sessions).for_each(f);
        };
        let mut tree = sample();
        each(&mut tree, &|s| {
            if s.dot == Dot::Working(None) {
                s.archived_at = Some("2026-02-01T00:00:00.000Z".into());
            }
        });
        assert!(!View::new(tree.clone(), true).animated(), "an archived Session's dot never turns");
        each(&mut tree, &|s| s.dot = Dot::Idle);
        assert!(!View::new(tree, true).animated());
    }

    #[test]
    fn nothing_acts_until_an_arrow_key_shows_the_cursor() {
        let mut view = View::new(sample(), true);
        assert_eq!(view.state.selected(), ["beta", "rel"], "the first Session is selected from the start");
        for key in [KeyCode::Enter, KeyCode::Char('x'), KeyCode::Char('n')] {
            assert_eq!(press(&mut view, key), Effect::None);
        }
        press(&mut view, KeyCode::Down);
        assert_eq!(view.state.selected(), ["beta", "rel"], "the first arrow only shows the cursor");
        press(&mut view, KeyCode::Char('j'));
        assert_eq!(view.state.selected(), ["alpha", "f-blocked"]);
        press(&mut view, KeyCode::Char('k'));
        assert_eq!(view.state.selected(), ["beta", "rel"]);
        press(&mut view, KeyCode::Up);
        assert_eq!(view.state.selected(), ["beta", "rel"], "a Context row is never selected");
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

    /// The question a key or an item opens: its popup, as the caller opens it.
    fn asked(effect: Effect) -> Question {
        let Effect::Ask(question) = effect else { panic!("a popup, got {effect:?}") };
        question
    }

    #[test]
    fn n_opens_an_input_popup_for_the_new_plans_id_in_the_context_of_the_session() {
        let mut view = on(&["alpha", "sketch"]);
        let ask = Ask::NamedPlan { context: ContextId::parse("alpha").unwrap(), label: "Sketch".into() };
        assert_eq!(asked(press(&mut view, KeyCode::Char('n'))), Question::input("New Plan", ask, String::new()));
        // the popup's answer is the command: the id is also the title
        let run = Action::NewPlan {
            context: ContextId::parse("alpha").unwrap(),
            id: PlanId::parse("pay-4521").unwrap(),
            title: "pay-4521".into(),
        };
        assert_eq!(answered(asked(press(&mut view, KeyCode::Char('n'))), Some("pay-4521")), Ok(Some(run)));
        assert_eq!(answered(asked(press(&mut view, KeyCode::Char('n'))), None), Ok(None), "a cancel runs nothing");
        let mut view = on(&["alpha", "f-idle"]);
        let ask = Ask::NamedPlan { context: ContextId::parse("alpha").unwrap(), label: "ship fork".into() };
        assert_eq!(asked(press(&mut view, KeyCode::Char('n'))), Question::input("New Plan", ask, String::new()));
    }

    /// The command `question` makes when its popup answers `text` (`None`: cancelled; a confirm confirms with any text, a select picks its first option).
    fn answered(question: Question, text: Option<&'static str>) -> Result<Option<Action>, String> {
        struct Answer(Option<&'static str>);
        impl Dialogs for Answer {
            fn input(&self, _: &str, _: &str, _: &str, _: Validator) -> Result<Option<String>, cpd_core::error::CpdError> {
                Ok(self.0.map(str::to_owned))
            }
            fn confirm(&self, _: &str, _: &str, _: &str) -> Result<bool, cpd_core::error::CpdError> {
                Ok(self.0.is_some())
            }
            fn select(&self, _: &str, _: &str, options: &[String]) -> Result<Option<String>, cpd_core::error::CpdError> {
                Ok(self.0.map(|_| options[0].clone()))
            }
            fn form(&self, _: &str, _: &str, _: &str, _: &[String]) -> Result<Option<(String, String)>, cpd_core::error::CpdError> {
                unreachable!("no form in these tests")
            }
            fn preview(&self, _: &Deletable) -> Result<serde_json::Value, String> {
                Ok(serde_json::json!({}))
            }
        }
        question.ask(&Answer(text))
    }

    #[test]
    fn x_refuses_the_session_in_the_chat_slot() {
        let mut view = on(&["alpha", "ship"]);
        assert_eq!(press(&mut view, KeyCode::Char('x')), Effect::text("Close Session", "switch to another session first"));
    }

    #[test]
    fn x_and_n_on_a_context_row_open_nothing() {
        let mut view = on(&["alpha"]);
        for key in ['x', 'n'] {
            assert_eq!(press(&mut view, KeyCode::Char(key)), Effect::None, "{key}");
        }
    }

    #[test]
    fn the_footer_is_the_snapshot_error_in_the_bottom_rows_and_no_prompt_row_is_drawn() {
        let mut view = on(&["alpha", "sketch"]);
        press(&mut view, KeyCode::Char('n'));
        press(&mut view, KeyCode::Char('x'));
        let rows = drawn(&mut view, 44, 18);
        assert!(rows[13..].iter().all(|r| r.is_empty()), "no row for a prompt: {rows:?}");
        assert!(rows.iter().all(|r| !r.contains("Enter") && !r.contains("█") && !r.contains("(y/N)")), "{rows:?}");
        view.error = Some("closed".into());
        let rows = drawn(&mut view, 44, 18);
        assert!(rows[13..17].iter().all(|r| r.is_empty()), "nothing above the error: {rows:?}");
        assert_eq!(rows[17], " closed", "an error is one row at the bottom");
        let mut terminal = Terminal::new(TestBackend::new(44, 14)).unwrap();
        terminal.draw(|f| view.draw(f, 0)).unwrap();
        let cell = &terminal.backend().buffer()[(1, 13)];
        assert_eq!((cell.fg, cell.modifier.contains(Modifier::BOLD)), (palette::error().fg.unwrap(), false));
    }

    /// tui-W8: the footer's height is counted in display columns, as it wraps.
    #[test]
    fn a_wide_footer_takes_the_rows_its_display_width_needs() {
        let mut view = on(&["alpha", "sketch"]);
        view.error = Some("失敗失敗失敗失敗失敗失敗失敗失".into()); // 15 characters, 30 columns
        let rows = drawn(&mut view, 24, 14); // 22 columns inside the margin: two rows
        // (the buffer cell after a wide character is blank)
        assert_eq!(rows[11].replace(' ', ""), "失敗失敗失敗失敗失敗失");
        assert_eq!(rows[12].replace(' ', ""), "敗失敗失");
    }

    fn click(view: &mut View, column: u16, row: u16) -> Effect {
        view.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE })
    }

    /// The closing row `╰󰬦 expand` of a Context with archived rows, as the tree draws it (after the margin).
    const EXPAND: &str = " ╰\u{F0B26} expand";
    /// The closing row `╰󰬬 collapse`.
    const COLLAPSE: &str = " ╰\u{F0B2C} collapse";
    /// The tree root's archive row, folded and expanded (hs-d698, hs-d707); drawn with the pane's leading margin.
    const ARCHIVE_DOWN: &str = "\u{F0B26} archive";
    const ARCHIVE_UP: &str = "\u{F0B2C} archive";

    #[test]
    fn a_click_folds_a_context_and_opens_a_session_and_hides_the_cursor() {
        let mut view = View::new(archived(), true);
        press(&mut view, KeyCode::Down);
        view.state.select(vec!["alpha".into(), "sketch".into()]);
        drawn(&mut view, 44, 16);
        assert_eq!(click(&mut view, 8, 10), Effect::None, "the `╰󰬬 collapse` row of the Context `alpha`");
        let rows = drawn(&mut view, 44, 16);
        assert_eq!((rows[4].as_str(), rows[8].as_str()), (format!(" ╭{NEEDS_YOU} alpha").as_str(), EXPAND), "the live rows stay: {rows:?}");
        assert!(!rows.iter().any(|r| r.contains(" old") || r.contains("f-bl")), "only the archived rows are hidden: {rows:?}");
        assert_eq!(view.state.selected(), ["alpha", "sketch"], "a click selects nothing");
        assert!(!view.cursor_shown());
        assert_eq!(click(&mut view, 8, 8), Effect::None, "`╰󰬦 expand` unfolds it");
        drawn(&mut view, 44, 16);
        assert_eq!(click(&mut view, 8, 5), Effect::Run(Action::Open(sid("l-ship"))), "the Plan row");
        assert_eq!(click(&mut view, 8, 7), Effect::Run(Action::Open(sid("f-idle"))), "a fork row");
        assert_eq!(click(&mut view, 8, 15), Effect::None, "below the tree");
        assert_eq!(view.state.selected(), ["alpha", "sketch"], "no click moved the cursor");
    }

    #[test]
    fn a_click_on_a_context_title_opens_its_manager_and_only_its_closing_row_folds() {
        let mut view = View::new(archived(), true);
        drawn(&mut view, 44, 16);
        let manager = Effect::Run(Action::OpenManager(ContextId::parse("alpha").unwrap()));
        for column in [1, 3, 5, 42] {
            assert_eq!(click(&mut view, column, 4), manager, "column {column} of the title row");
        }
        assert_eq!(drawn(&mut view, 44, 16)[10], COLLAPSE, "no click on the title folds");
        for column in [1, 3, 8, 42] {
            assert_eq!(click(&mut view, column, 10), Effect::None, "column {column} of the closing row runs no command");
            assert_eq!(drawn(&mut view, 44, 16)[8], EXPAND, "and folds it");
            assert_eq!(click(&mut view, column, 8), Effect::None, "`╰󰬦 expand` runs no command");
            assert_eq!(drawn(&mut view, 44, 16)[10], COLLAPSE, "and unfolds it");
        }
        click(&mut view, 8, 10);
        drawn(&mut view, 44, 16);
        assert_eq!(click(&mut view, 5, 4), manager, "a folded Context opens its Manager too");
        let Effect::Run(action) = manager else { unreachable!() };
        assert_eq!(action.argv(), ["context", "manager", "--context=alpha", "--by-user"]);
    }

    #[test]
    fn an_archived_context_starts_folded_unfolds_at_its_closing_row_and_a_click_on_its_title_unarchives_it_and_opens_its_manager() {
        let mut view = View::new(archived(), true);
        let rows = drawn(&mut view, 44, 30);
        assert_eq!(rows[15].trim(), ARCHIVE_DOWN, "the archive row starts folded: {rows:?}");
        click(&mut view, 3, 15); // the archive row
        let rows = drawn(&mut view, 44, 30);
        assert_eq!((rows[16].as_str(), rows[17].as_str()), (format!(" ╭{STOPPED} zeta").as_str(), EXPAND), "{rows:?}");
        assert_eq!(click(&mut view, 6, 16), Effect::Run(resume(Subject::Context(cid("zeta")), Action::OpenManager(cid("zeta")))), "its title: unarchive, then its Manager");
        assert_eq!(drawn(&mut view, 44, 30), rows, "its title folds and unfolds nothing");
        click(&mut view, 3, 17);
        let rows = drawn(&mut view, 44, 30);
        assert_eq!(rows[16], format!(" ╭{STOPPED} zeta"));
        assert_eq!(rows[19], COLLAPSE, "its closing row unfolds it");
        assert_eq!(click(&mut view, 6, 16), Effect::Run(resume(Subject::Context(cid("zeta")), Action::OpenManager(cid("zeta")))));
        assert_eq!(click(&mut view, 8, 19), Effect::None);
        assert_eq!(drawn(&mut view, 44, 30)[17], EXPAND, "and folds it");
    }

    #[test]
    fn row_0_is_blank_and_the_rows_follow_one_lower_and_clicks_land_on_the_row_under_the_pointer() {
        let mut view = View::new(sample(), true);
        let rows = drawn(&mut view, 44, 16);
        assert_eq!(rows[0], "", "the blank row at the top");
        assert_eq!(rows[4], format!(" ╭{NEEDS_YOU} alpha"));
        assert!(rows[6].starts_with(&format!(" ├─{WORKING} ship")), "{rows:?}");
        assert_eq!(click(&mut view, 8, 0), Effect::None, "the blank row is no row: not the Context row below it, not the first Session");
        assert!(view.state.opened().contains(&vec!["alpha".to_owned()]), "alpha is not folded");
        assert_eq!(click(&mut view, 8, 6), Effect::Run(Action::Open(sid("l-ship"))), "the Plan's row");
        assert_eq!(click(&mut view, 8, 7), Effect::Run(Action::Open(sid("l-sketch"))), "the Session below it");
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
            view.error = Some("an error that is long enough to wrap onto several rows of the pane".into());
            assert_margins_blank(&mut view, width, height, "a wrapped error");
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
        let ship: String = (0..44).map(|x| buf[(x, 6)].symbol()).collect();
        assert!(ship.trim_end().ends_with("2⋅2⋅5") && ship.chars().nth(42) == Some('5'), "{ship:?}");
        let reversed: Vec<u16> = (0..44).filter(|x| crate::palette::check::is_selected(&buf[(*x, 6)])).collect();
        assert_eq!(reversed, (1..43).collect::<Vec<u16>>());
    }

    #[test]
    fn a_click_on_the_first_drawn_cell_of_a_row_acts_on_that_row_and_the_margins_are_no_row() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        assert_eq!(click(&mut view, 1, 6), Effect::Run(Action::Open(sid("l-ship"))), "the Plan row's branch");
        assert_eq!(click(&mut view, 1, 7), Effect::Run(Action::Open(sid("l-sketch"))));
        assert_eq!(click(&mut view, 42, 7), Effect::Run(Action::Open(sid("l-sketch"))), "the last cell of the list");
        for row in [6, 7] {
            assert_eq!(click(&mut view, 0, row), Effect::None, "the left margin of row {row}");
            assert_eq!(click(&mut view, 43, row), Effect::None, "the right margin of row {row}");
        }
        assert_eq!(click(&mut view, 1, 4), Effect::Run(Action::OpenManager(ContextId::parse("alpha").unwrap())), "the title row keeps its action");
        let rows = drawn(&mut view, 44, 16);
        assert_eq!(click(&mut view, 1, 13), Effect::None, "below the last row there is nothing to fold");
        assert_eq!(drawn(&mut view, 44, 16), rows, "and folds nothing");
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
    fn the_archived_rows_are_inside_the_expanded_block_with_one_toggle_row_that_folds_them_and_a_click_on_one_resumes_it() {
        let mut view = View::new(archived(), true);
        let rows = drawn(&mut view, 44, 23);
        assert!(rows[8].starts_with(&format!(" ├─{STOPPED} old")), "{rows:?}");
        assert_eq!(rows[9], format!(" ├─{NEEDS_YOU} ship·f-bl"));
        assert_eq!(rows[10], COLLAPSE, "one closing row for alpha, its archived rows included");
        assert_eq!(rows[11], "", "one empty row between two Contexts");
        assert_eq!(rows[12], format!(" ╭{STOPPED} gamma"));
        assert!(!rows.iter().any(|r| r.contains('\u{F01D8}')), "no `󰇘` row: {rows:?}");
        // a click on an archived row resumes it: the unarchive of its outermost archived object, then the Open
        assert_eq!(click(&mut view, 3, 8), Effect::Run(resume(Subject::Plan(pid("old")), Action::Open(sid("l-old")))));
        assert_eq!(click(&mut view, 3, 9), Effect::Run(resume(Subject::Session(sid("f-blocked")), Action::Open(sid("f-blocked")))));
        assert_eq!(drawn(&mut view, 44, 23), rows, "a click on an archived row changes nothing");
        // the closing row folds the archived rows only: the live ones stay
        assert_eq!(click(&mut view, 3, 10), Effect::None);
        let rows = drawn(&mut view, 44, 23);
        assert_eq!((rows[7].as_str(), rows[8].as_str(), rows[9].as_str(), rows[10].as_str()), (format!(" ├─{IDLE} ship·f-id").as_str(), EXPAND, "", format!(" ╭{STOPPED} gamma").as_str()));
        assert!(!rows.iter().any(|r| r.contains(" old") || r.contains("f-bl")), "{rows:?}");
        // an archived Context is under the archive row: open the archive row, then unfold it at its own closing row
        assert_eq!(rows[13].trim(), ARCHIVE_DOWN, "the archive row: {rows:?}");
        assert_eq!(click(&mut view, 3, 13), Effect::None);
        let rows = drawn(&mut view, 44, 23);
        assert_eq!((rows[14].as_str(), rows[15].as_str()), (format!(" ╭{STOPPED} zeta").as_str(), EXPAND), "{rows:?}");
        assert_eq!(click(&mut view, 3, 15), Effect::None);
        let rows = drawn(&mut view, 44, 23);
        assert_eq!((rows[14].as_str(), rows[17].as_str()), (format!(" ╭{STOPPED} zeta").as_str(), COLLAPSE), "{rows:?}");
    }

    const UP_COLLAPSE: &str = "╰\u{F0B2C} collapse";
    const DOWN_EXPAND: &str = "╰\u{F0B26} expand";

    fn file_in(dir: &tempfile::TempDir) -> FoldsFile {
        FoldsFile::at(&cpd_core::home::CpdHome::resolve(Some(dir.path().into()), None).unwrap())
    }

    /// The first row at or after row `from` that reads `row` (trimmed): the fold widget's `╰󰬬 collapse` / `╰󰬦 expand`
    /// rows are the only ones that fold (rows.rs).
    fn row_of(rows: &[String], from: usize, row: &str) -> u16 {
        (from + rows[from..].iter().position(|r| r.trim() == row).unwrap_or_else(|| panic!("{row:?} in {rows:#?}"))) as u16
    }

    #[test]
    fn a_collapsed_context_and_an_expanded_archived_context_survive_a_restart_of_the_view_and_the_cursor_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let mut view = View::new(archived(), true);
        view.keep_folds(file_in(&dir));
        let rows = drawn(&mut view, 44, 30);
        assert!(rows.iter().any(|r| r.contains(" old")), "alpha starts expanded, its archived rows show: {rows:#?}");
        click(&mut view, 4, row_of(&rows, 1, UP_COLLAPSE)); // folds alpha: its archived rows go, its live rows stay
        let rows = drawn(&mut view, 44, 30);
        assert!(rows.iter().any(|r| r.contains("ship·f-id")) && !rows.iter().any(|r| r.contains(" old")), "{rows:#?}");
        click(&mut view, 4, row_of(&rows, 1, ARCHIVE_DOWN)); // expands the archive row
        let rows = drawn(&mut view, 44, 30);
        let zeta = rows.iter().position(|r| r.contains("zeta")).expect("the archived Context zeta, under the archive row");
        click(&mut view, 4, row_of(&rows, zeta, DOWN_EXPAND)); // expands it
        view.state.select(vec!["alpha".into(), "sketch".into()]);
        view.save_folds();
        let shown = drawn(&mut view, 44, 30);
        assert!(shown.iter().any(|r| r.contains("zp")), "zeta's Plan shows: {shown:#?}");

        let mut again = View::new(archived(), true);
        again.keep_folds(file_in(&dir));
        assert_eq!(drawn(&mut again, 44, 30), shown, "the folds are back");
        assert_ne!(again.state.selected(), ["alpha", "sketch"], "the cursor is not kept");
    }

    #[test]
    fn a_fold_is_written_once_and_nothing_is_written_until_one_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run/tui/tree-folds.json");
        let mut view = View::new(archived(), true);
        view.keep_folds(file_in(&dir));
        let rows = drawn(&mut view, 44, 30);
        view.save_folds();
        assert!(!path.exists(), "a start with no fold writes nothing");
        click(&mut view, 4, row_of(&rows, 1, UP_COLLAPSE));
        view.save_folds();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"collapsed":["alpha"],"expanded":[]}"#);
    }

    #[test]
    fn an_invalid_file_starts_with_the_defaults_logs_it_and_the_next_write_replaces_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run/tui/tree-folds.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{not json").unwrap();
        let mut view = View::new(archived(), true);
        let lines = view.keep_folds(file_in(&dir));
        assert!(lines.iter().any(|l| l.starts_with("fold state reset: ") && l.contains("tree-folds.json")), "{lines:?}");
        assert!(view.state.opened().contains(&vec!["alpha".to_owned()]), "the defaults");
        let rows = drawn(&mut view, 44, 30);
        view.save_folds();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{not json", "nothing is written until a fold changes");
        click(&mut view, 4, row_of(&rows, 1, UP_COLLAPSE));
        view.save_folds();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"collapsed":["alpha"],"expanded":[]}"#);
    }

    #[test]
    fn a_click_below_the_last_row_does_nothing() {
        let mut view = View::new(archived(), true);
        let rows = drawn(&mut view, 44, 23);
        assert_eq!(rows[15].trim(), ARCHIVE_DOWN);
        assert_eq!(rows[16], "", "the row right below the last tree row");
        assert_eq!(click(&mut view, 1, 16), Effect::None, "the row right below the last");
        assert_eq!(click(&mut view, 3, 17), Effect::None);
        assert_eq!(drawn(&mut view, 44, 23), rows);
        let _ = right_click(&mut view, 1, 16);
        assert_eq!(drawn(&mut view, 44, 23).len(), rows.len());
        assert_eq!(drawn(&mut view, 44, 23)[15].trim(), ARCHIVE_DOWN, "only the left button clicks, and no row was hit");
    }

    #[test]
    fn a_click_on_an_archived_row_unarchives_and_opens_it_and_enter_n_and_x_never_act_on_one() {
        let mut view = View::new(archived(), true);
        drawn(&mut view, 44, 30);
        click(&mut view, 1, 15); // the archive row
        drawn(&mut view, 44, 30); // the archive row's rows are drawn before the next click
        click(&mut view, 1, 17); // `╰󰬦 expand` of the archived Context
        let rows = drawn(&mut view, 44, 30);
        assert!(rows[8].starts_with(&format!(" ├─{STOPPED} old")) && rows[16] == format!(" ╭{STOPPED} zeta"), "{rows:?}");
        // the archived Plan (its Lead), the archived fork of a live Plan, the Plan of the archived Context and its
        // fork: each unarchives its outermost archived object, then opens its Session
        let archived_objects = [
            ((8, 8), Subject::Plan(pid("old")), "l-old"),
            ((8, 9), Subject::Session(sid("f-blocked")), "f-blocked"),
            ((8, 17), Subject::Context(cid("zeta")), "l-zp"),
            ((8, 18), Subject::Context(cid("zeta")), "f-zp"),
        ];
        for ((column, row), subject, id) in archived_objects {
            assert_eq!(click(&mut view, column, row), Effect::Run(resume(subject, Action::Open(sid(id)))), "({column}, {row})");
            assert_eq!(drawn(&mut view, 44, 30), rows, "({column}, {row}) changes nothing");
        }
        // the archived Context's title resumes its Manager
        assert_eq!(click(&mut view, 5, 16), Effect::Run(resume(Subject::Context(cid("zeta")), Action::OpenManager(cid("zeta")))));
        // the cursor is put on archived rows by hand, as a bug might: no key acts on it
        press(&mut view, KeyCode::Down);
        for path in [&["alpha", "old"][..], &["alpha", "f-blocked"], &["zeta", "zp"], &["zeta", "f-zp"]] {
            view.state.select(path.iter().map(|p| (*p).to_owned()).collect());
            for key in [KeyCode::Enter, KeyCode::Char('x'), KeyCode::Char('n')] {
                assert_eq!(press(&mut view, key), Effect::None, "{path:?} {key:?}");
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
        let rows = drawn(view, view.pane.width, view.pane.height);
        (rect.y + 1..rect.bottom() - 1)
            .map(|y| {
                let row: Vec<char> = rows[y as usize].chars().collect();
                row[rect.x as usize + 1..(rect.right() - 1) as usize].iter().collect::<String>().trim().to_owned()
            })
            .collect()
    }

    fn menu_at(view: &mut View, column: u16, row: u16) -> Vec<String> {
        view.menu = None;
        // tall enough for every row of `archived_open`: the tree does not scroll, no cue row hides one (hs-d307)
        drawn(view, 44, 30);
        assert_eq!(right_click(view, column, row), Effect::None);
        assert!(view.menu.is_some(), "({column}, {row})");
        menu_items(view)
    }

    #[test]
    fn a_right_click_opens_the_menu_of_the_row_or_the_empty_area() {
        let mut view = View::new(sample(), true);
        assert_eq!(menu_at(&mut view, 5, 1), ["New Plan…", "Rename…", "Copy id", "Archive"], "a Context");
        assert_eq!(menu_at(&mut view, 8, 2), ["New session…", "Rename…", "Copy id", "Archive"], "a Plan");
        assert_eq!(menu_at(&mut view, 8, 9), ["Open", "Rename…", "Fork…", "Copy id", "Close", "Archive"], "a fork");
        assert_eq!(menu_at(&mut view, 8, 17), ["New Context…"], "the empty area");
        assert_eq!(menu_at(&mut view, 8, 13), ["New Context…"], "below the last row");
        let mut view = View::new(View::new(archived(), true).tree, true);
        assert_eq!(menu_at(&mut view, 8, 1), ["New Plan…", "Rename…", "Copy id", "Archive"], "a live row of a tree with archived ones");
    }

    /// A view of `archived()` with the archived Context unfolded (a tall pane, 30 rows): the archived Plan `old` is
    /// at row 8, its fork `f-blocked` at 9, the archived Context `zeta` at 16, its Plan at 17 and fork at 18.
    fn archived_open() -> View {
        let mut view = View::new(archived(), true);
        let rows = drawn(&mut view, 44, 30);
        click(&mut view, 1, row_of(&rows, 1, ARCHIVE_DOWN)); // the archive row
        let rows = drawn(&mut view, 44, 30);
        assert_eq!(rows[16], format!(" ╭{STOPPED} zeta"), "{rows:?}");
        click(&mut view, 1, 17); // `╰󰬦 expand` of the archived Context
        let rows = drawn(&mut view, 44, 30);
        assert!(rows[8].starts_with(&format!(" ├─{STOPPED} old")) && rows[16] == format!(" ╭{STOPPED} zeta"), "{rows:?}");
        view
    }

    #[test]
    fn a_right_click_on_an_archived_row_offers_rename_copy_id_and_unarchive_and_on_a_closing_row_nothing() {
        let mut view = archived_open();
        // the archived Plan, its fork, an archived Context and its Plan and fork
        // (a Plan or a Context row also offers Delete, hs-d527; a fork does not)
        for (column, row, delete) in [(8, 8, true), (8, 9, false), (8, 16, true), (8, 17, true), (8, 18, false)] {
            let items: &[&str] = if delete { &["Rename…", "Copy id", "Unarchive", "Delete"] } else { &["Rename…", "Copy id", "Unarchive"] };
            assert_eq!(menu_at(&mut view, column, row), items, "({column}, {row})");
            press(&mut view, KeyCode::Esc);
        }
        // the closing rows have none, the empty area has one
        drawn(&mut view, 44, 30);
        for (column, row) in [(3, 10), (3, 13), (3, 19), (1, 20)] {
            let _ = right_click(&mut view, column, row);
            assert_eq!(view.menu.is_some(), row == 20, "({column}, {row}): a closing row has none, the empty area has one");
            view.menu = None;
        }
    }

    #[test]
    fn the_menu_is_drawn_at_the_click_and_clamped_inside_the_pane() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 5);
        let rows = drawn(&mut view, 44, 16);
        assert!(rows[5].ends_with(&format!("shi╭─────────╮")), "{rows:?}");
        assert!(rows[6].contains(&format!("shi│ Open    │")), "{rows:?}");
        assert!(rows[7].contains("│ Rename… │"), "drawn over the tree: {rows:?}");
        assert_eq!(rows[12], " ╰\u{f0b2a} new ╰─────────╯", "over the tree's last row");
        let reversed = |view: &mut View, x, y| {
            let mut terminal = Terminal::new(TestBackend::new(44, 16)).unwrap();
            terminal.draw(|f| view.draw(f, 0)).unwrap();
            crate::palette::check::is_selected(&terminal.backend().buffer()[(x, y)])
        };
        assert!(reversed(&mut view, 10, 6), "the first item is selected");
        press(&mut view, KeyCode::Down);
        assert!(reversed(&mut view, 10, 7) && !reversed(&mut view, 10, 6));
        view.menu = None;
        right_click(&mut view, 43, 15);
        let rows = drawn(&mut view, 44, 16);
        let box_row = |left: char, fill: &str, right: char| format!("{}{left}{fill}{right}", " ".repeat(27));
        assert_eq!((rows[13].ends_with(&box_row('╭', &"─".repeat(14), '╮')[27..]), rows[15].clone()), (true, box_row('╰', &"─".repeat(14), '╯')));
        assert!(rows[14].ends_with("│ New Context… │"), "{rows:?}");
    }

    #[test]
    fn enter_on_the_menu_picks_its_selected_item_and_esc_closes_it_running_nothing() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 9); // a fork: f-idle
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::Open(sid("f-idle"))));
        assert!(view.menu.is_none());
        right_click(&mut view, 8, 9);
        assert_eq!(press(&mut view, KeyCode::Esc), Effect::None);
        assert!(view.menu.is_none());
    }

    fn archive_of(subject: Subject) -> Effect {
        Effect::Run(Action::Archive { subject, free: None })
    }

    #[test]
    fn archive_runs_at_once_on_its_menu_click() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 5); // the fork `f-blocked`, which is not in the chat slot
        for _ in 0..5 {
            press(&mut view, KeyCode::Down);
        }
        assert_eq!(press(&mut view, KeyCode::Enter), archive_of(Subject::Session(sid("f-blocked"))));
        assert!(view.menu.is_none());
        // a click picks it as well
        right_click(&mut view, 8, 5);
        let effect = view.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 10, row: 11, modifiers: KeyModifiers::NONE });
        assert_eq!(effect, archive_of(Subject::Session(sid("f-blocked"))));
        // a Plan and a Context name their own objects: `sketch` and `beta` hold no Lead of the chat slot
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 7);
        for _ in 0..3 {
            press(&mut view, KeyCode::Down);
        }
        assert_eq!(press(&mut view, KeyCode::Enter), archive_of(Subject::Plan(PlanId::parse("sketch").unwrap())));
        drawn(&mut view, 44, 16);
        right_click(&mut view, 5, 1);
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
        // rows: alpha, ship (l-ship in the chat slot), sketch, old, f-idle, f-blocked, its toggle, beta, rel
        let ship = Subject::Plan(PlanId::parse("ship").unwrap());
        assert_eq!(archive_at(sample(), 8, 6), freed(ship, Free::Open(sid("l-sketch"))), "a Plan row: the Plan below it");
        let mut tree = sample();
        tree.contexts[0].plans[0].sessions[0].place = crate::read::Place::Parked;
        tree.contexts[0].plans[0].sessions[2].place = crate::read::Place::Chat;
        assert_eq!(
            archive_at(tree.clone(), 8, 5),
            freed(Subject::Session(sid("f-blocked")), Free::Open(sid("l-ship"))),
            "a fork row: the next live Session below it"
        );
        // the Plan whose side fork holds the slot: the fork is archived with it, so it is not the next
        assert_eq!(archive_at(tree, 8, 6), freed(Subject::Plan(PlanId::parse("ship").unwrap()), Free::Open(sid("l-sketch"))));
        // a Context row: the first live Session below it, none of its own
        assert_eq!(
            archive_at(sample(), 5, 4),
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
        assert_eq!(archive_at(tree, 8, 2), freed(Subject::Plan(PlanId::parse("rel").unwrap()), Free::Open(sid("f-blocked"))));
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
        right_click(&mut view, 8, 6);
        for _ in 0..2 {
            press(&mut view, KeyCode::Down);
        }
        let mut tree = sample();
        tree.contexts[0].plans.retain(|p| p.head.id.as_str() != "ship");
        view.refresh(tree);
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
    }

    #[test]
    fn close_in_the_menu_runs_at_once_as_x_does_and_refuses_the_session_in_the_chat_slot() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 9);
        for _ in 0..4 {
            press(&mut view, KeyCode::Down);
        }
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::Close(sid("f-idle"))));
        // the Lead in the chat slot: the same refusal as `x`
        let mut tree = sample();
        tree.contexts[0].plans[0].sessions[1].place = crate::read::Place::Chat;
        let mut view = View::new(tree, true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 9);
        for _ in 0..4 {
            press(&mut view, KeyCode::Down);
        }
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::text("Close Session", IN_CHAT_SLOT));
    }

    #[test]
    fn rename_and_unarchive_on_an_archived_row_run_without_asking_to_confirm() {
        let mut view = archived_open();
        for (row, subject) in [
            (8, Subject::Plan(PlanId::parse("old").unwrap())),
            (9, Subject::Session(sid("f-blocked"))),
            (16, Subject::Context(ContextId::parse("zeta").unwrap())),
        ] {
            right_click(&mut view, 8, row);
            press(&mut view, KeyCode::Down);
            press(&mut view, KeyCode::Down);
            assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::Unarchive(subject)), "row {row}");
            assert!(view.menu.is_none());
        }
        // Rename… starts from the label of the archived row and runs the rename of the object
        right_click(&mut view, 8, 8);
        let rename = Question::input("Rename", Ask::RenamePlan(PlanId::parse("old").unwrap()), "Old".into());
        assert_eq!(asked(press(&mut view, KeyCode::Enter)), rename);
        assert_eq!(
            answered(rename, Some("Older")),
            Ok(Some(Action::RenamePlan { id: PlanId::parse("old").unwrap(), title: "Older".into() }))
        );
        drawn(&mut view, 44, 30);
        right_click(&mut view, 8, 16);
        let rename = Question::input("Rename", Ask::RenameContext(ContextId::parse("zeta").unwrap()), "Zeta".into());
        assert_eq!(asked(press(&mut view, KeyCode::Enter)), rename);
        // and of an archived fork: its name
        drawn(&mut view, 44, 30);
        right_click(&mut view, 8, 9);
        let rename = Question::input("Rename", Ask::RenameSession(sid("f-blocked")), "ship review".into());
        assert_eq!(asked(press(&mut view, KeyCode::Enter)), rename);
    }

    #[test]
    fn copy_id_copies_the_shown_id_of_every_kind_of_row() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        for (column, row, index, id) in [(5, 4, 2, "alpha"), (8, 6, 2, "ship"), (8, 9, 3, "ship·f-id")] {
            right_click(&mut view, column, row);
            for _ in 0..index {
                press(&mut view, KeyCode::Down);
            }
            assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::CopyId(id.into())), "{id}");
            assert!(view.menu.is_none());
        }
        // archived rows: a Plan, a fork of it, an archived Context
        let mut view = archived_open();
        for (row, id) in [(8, "old"), (9, "ship·f-bl"), (16, "zeta")] {
            right_click(&mut view, 8, row);
            press(&mut view, KeyCode::Down);
            assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::CopyId(id.into())), "row {row}");
        }
    }

    /// Right-clicks (`column`, `row`) and picks the item `index` of its menu with Enter.
    fn pick_at(view: &mut View, column: u16, row: u16, index: usize) -> Effect {
        view.menu = None;
        drawn(view, 44, 24);
        right_click(view, column, row);
        for _ in 0..index {
            press(view, KeyCode::Down);
        }
        press(view, KeyCode::Enter)
    }

    /// The popup the item `index` of the menu at (`column`, `row`) opens.
    fn popup_at(view: &mut View, column: u16, row: u16, index: usize) -> Question {
        asked(pick_at(view, column, row, index))
    }

    #[test]
    fn rename_opens_an_input_popup_from_the_current_label_and_runs_the_rename_of_the_row() {
        let mut view = View::new(sample(), true);
        // a Context: its title
        let question = popup_at(&mut view, 5, 4, 1);
        assert!(view.menu.is_none());
        let id = ContextId::parse("alpha").unwrap();
        assert_eq!(question, Question::input("Rename", Ask::RenameContext(id.clone()), "Alpha".into()));
        assert_eq!(answered(question.clone(), Some("Alpha two")), Ok(Some(Action::RenameContext { id, title: "Alpha two".into() })));
        assert_eq!(answered(question, None), Ok(None), "a cancel runs nothing");
        // a Plan: its title
        let id = PlanId::parse("ship").unwrap();
        let question = popup_at(&mut view, 8, 6, 1);
        assert_eq!(question, Question::input("Rename", Ask::RenamePlan(id.clone()), "Ship".into()));
        assert_eq!(answered(question, Some("Release")), Ok(Some(Action::RenamePlan { id, title: "Release".into() })));
        // a fork: its name
        let question = popup_at(&mut view, 8, 9, 1);
        assert_eq!(question, Question::input("Rename", Ask::RenameSession(sid("f-idle")), "ship fork".into()));
        assert_eq!(
            answered(question, Some("ship fork!")),
            Ok(Some(Action::RenameSession { id: sid("f-idle"), name: "ship fork!".into() }))
        );
    }

    #[test]
    fn fork_and_new_session_open_an_input_popup_for_a_name_and_fork_the_right_source() {
        let mut view = View::new(sample(), true);
        // a fork row: Fork… forks that fork
        let question = popup_at(&mut view, 8, 9, 2);
        assert_eq!(question, Question::input("Fork", Ask::Fork { source: sid("f-idle") }, "ship fork (fork)".into()));
        assert_eq!(
            answered(question, Some("ship fork (fork)")),
            Ok(Some(Action::Fork { source: sid("f-idle"), name: "ship fork (fork)".into() }))
        );
        // a Plan row: New session… forks the Plan's Lead, from the Plan's title
        let question = popup_at(&mut view, 8, 6, 0);
        assert_eq!(question, Question::input("New session", Ask::Fork { source: sid("l-ship") }, "Ship (fork)".into()));
        assert_eq!(
            answered(question.clone(), Some("Ship again")),
            Ok(Some(Action::Fork { source: sid("l-ship"), name: "Ship again".into() }))
        );
        assert_eq!(answered(question, None), Ok(None), "a cancel runs nothing");
    }

    #[test]
    fn the_name_of_a_fork_is_cut_to_fit_the_limit() {
        let mut tree = sample();
        tree.contexts[0].plans[1].head.title = "x".repeat(150);
        let mut view = View::new(tree, true);
        let Question::Input { initial, .. } = popup_at(&mut view, 8, 7, 0) else { panic!("an input") };
        assert_eq!(initial.chars().count(), 80);
        assert!(initial.ends_with("x (fork)"));
    }

    /// A view whose New Context… reads the policy tree of a temp home with `files` (paths under `policy/`).
    fn owned(tree: Tree, files: &[&str]) -> (View, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        for f in files {
            let path = dir.path().join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        let mut view = View::new(tree, true);
        view.policy_dir = Some(dir.path().to_path_buf());
        (view, dir)
    }

    #[test]
    fn new_context_with_no_owner_policy_is_refused_in_a_dialog_and_runs_nothing() {
        let (mut view, _dir) = owned(sample(), &["notes.md"]);
        assert_eq!(
            pick_at(&mut view, 8, 15, 0),
            Effect::text("New Context", "No owner policy: add $CPD_HOME/policy/<owner>/owner.toml"),
            "no input popup opens"
        );
    }

    #[test]
    fn new_plan_opens_an_input_popup_for_a_title_derives_the_id_and_asks_no_repo() {
        let mut view = View::new(sample(), true);
        let question = popup_at(&mut view, 5, 4, 0);
        let Question::Input { title, ref ask, ref initial } = question else { panic!("an input") };
        assert_eq!((title, initial.as_str()), ("New Plan", ""));
        assert!(matches!(ask, Ask::NewPlan { context, .. } if context.as_str() == "alpha"), "{ask:?}");
        assert_eq!(
            answered(question.clone(), Some("Pay 4521")),
            Ok(Some(Action::NewPlan {
                context: ContextId::parse("alpha").unwrap(),
                id: PlanId::parse("pay-4521").unwrap(),
                title: "Pay 4521".into(),
            }))
        );
        // an id taken by any Plan of the tree gets the suffix
        assert_eq!(
            answered(question.clone(), Some("Rel")),
            Ok(Some(Action::NewPlan {
                context: ContextId::parse("alpha").unwrap(),
                id: PlanId::parse("rel-2").unwrap(),
                title: "Rel".into(),
            }))
        );
        assert!(answered(question, Some("!!")).unwrap_err().starts_with("A title needs at least two letters"));
    }

    #[test]
    fn new_plan_in_a_context_with_no_plan_asks_only_the_title() {
        let mut view = View::new(sample(), true);
        let question = popup_at(&mut view, 5, 11, 0); // the Context `gamma`
        assert_eq!(
            answered(question, Some("First plan")),
            Ok(Some(Action::NewPlan {
                context: ContextId::parse("gamma").unwrap(),
                id: PlanId::parse("first-plan").unwrap(),
                title: "First plan".into(),
            }))
        );
    }

    #[test]
    fn an_open_menu_takes_every_key_and_click() {
        let mut view = on(&["alpha", "sketch"]);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 20, 4);
        for key in [KeyCode::Char('n'), KeyCode::Char('x'), KeyCode::Char('?'), KeyCode::Char('j'), KeyCode::Char('q')] {
            assert_eq!(press(&mut view, key), Effect::None, "{key:?}");
            assert!(view.menu.is_some(), "{key:?}");
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
        right_click(&mut view, 8, 9);
        let rect = view.menu.as_ref().unwrap().rect(view.pane);
        assert_eq!(click(&mut view, rect.x + 2, rect.y + 1), Effect::Run(Action::Open(sid("f-idle"))));
        assert!(view.menu.is_none());
    }

    #[test]
    fn no_key_opens_the_menu() {
        let mut view = on(&["alpha", "sketch"]);
        drawn(&mut view, 44, 16);
        for key in [KeyCode::Char('n'), KeyCode::Char('x'), KeyCode::Char('m'), KeyCode::Char('o'), KeyCode::Char(' '), KeyCode::Menu, KeyCode::F(10), KeyCode::Char('/')] {
            press(&mut view, key);
            assert!(view.menu.is_none(), "{key:?}");
        }
    }

    #[test]
    fn a_snapshot_that_drops_the_menus_row_makes_its_item_do_nothing() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 9);
        let mut tree = sample();
        tree.contexts[0].plans[0].sessions.retain(|s| s.id.as_str() != "f-idle");
        view.refresh(tree);
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert!(view.menu.is_none());
    }

    // ---------- helpers of the drawn tree ----------

    fn draw_now(view: &mut View, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| view.draw(f, 0)).unwrap();
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

    /// A fake runner of the commands the tree may run (`session send` to a Manager; never `watch --ensure`), and the live
    /// Managers it can list. The clock is the `Instant`s the tests pass.
    struct Fake {
        calls: Vec<Vec<String>>,
        managers: Vec<SessionId>,
    }

    impl Fake {
        fn with(managers: &[&str]) -> Fake {
            Fake { calls: Vec::new(), managers: managers.iter().map(|m| SessionId::parse(m).unwrap()).collect() }
        }

        /// One look at the engine at `now`: `running` is what the snapshot read of `locks/watch.lock`.
        fn tick(&mut self, view: &mut View, now: Instant, running: bool) {
            view.tree.engine_running = running;
            let (calls, managers) = (&mut self.calls, &self.managers);
            view.watch_engine(
                now,
                &mut |action| {
                    calls.push(action.argv());
                    Ok(serde_json::json!({"ok": true}))
                },
                &mut || Ok(managers.clone()),
            );
        }
    }

    const TOLD: &str = "The cpd engine has been down for 60 s and its supervisor has not restarted it; see run/watch.log, start it with cpd watch --ensure.";

    fn secs(t0: Instant, s: u64) -> Instant {
        t0 + Duration::from_secs(s)
    }

    /// The `session send` of `text` to each of `managers`: from null; send always wakes, with `--unblocks`.
    fn sends(managers: &[&str], text: &str) -> Vec<Vec<String>> {
        managers
            .iter()
            .map(|m| ["session", "send", m, "--unblocks", "the engine has been down 60 s", "--", text].map(str::to_owned).to_vec())
            .collect()
    }

    #[test]
    fn a_lock_free_for_half_a_second_and_then_held_runs_nothing_and_shows_nothing() {
        // the supervisor restarts the engine in about 0.5 s: the tree catches that window and must not act on it (hs-d363)
        let (mut view, mut fake, t0) = (View::new(sample(), true), Fake::with(&["m-alpha", "m-beta"]), Instant::now());
        fake.tick(&mut view, t0, false);
        fake.tick(&mut view, t0 + Duration::from_millis(500), true);
        assert!(fake.calls.is_empty(), "no `watch --ensure`, no message: {:?}", fake.calls);
    }

    #[test]
    fn a_lock_free_for_59_seconds_is_nothing() {
        let (mut view, mut fake, t0) = (View::new(sample(), true), Fake::with(&["m-alpha", "m-beta"]), Instant::now());
        for s in 0..=59 {
            fake.tick(&mut view, secs(t0, s), false);
            assert!(fake.calls.is_empty(), "{s} s down: {:?}", fake.calls);
        }
    }

    #[test]
    fn a_lock_free_for_60_seconds_tells_the_live_manager_once_and_nothing_more_at_120() {
        let (mut view, mut fake, t0) = (View::new(sample(), true), Fake::with(&["m-alpha"]), Instant::now());
        for s in 0..60 {
            fake.tick(&mut view, secs(t0, s), false);
        }
        assert!(fake.calls.is_empty(), "59 s: {:?}", fake.calls);
        fake.tick(&mut view, secs(t0, 60), false);
        assert_eq!(fake.calls, sends(&["m-alpha"], TOLD), "one message, no `watch --ensure`");
        for s in 61..=120 {
            fake.tick(&mut view, secs(t0, s), false);
        }
        assert_eq!(fake.calls.len(), 1, "one message per downtime: {:?}", fake.calls);
    }

    #[test]
    fn the_engine_running_again_and_down_again_tells_again() {
        let (mut view, mut fake, t0) = (View::new(sample(), true), Fake::with(&["m-alpha"]), Instant::now());
        for s in 0..=60 {
            fake.tick(&mut view, secs(t0, s), false);
        }
        assert_eq!(fake.calls.len(), 1, "told: {:?}", fake.calls);
        fake.tick(&mut view, secs(t0, 61), true);
        assert_eq!(fake.calls.len(), 1, "nothing is sent for its return: {:?}", fake.calls);
        // down again: the 60 s count starts over, from the first snapshot that reads the lock free
        for s in 62..=121 {
            fake.tick(&mut view, secs(t0, s), false);
        }
        assert_eq!(fake.calls.len(), 1, "59 s into the second downtime: {:?}", fake.calls);
        fake.tick(&mut view, secs(t0, 122), false);
        assert_eq!(fake.calls, sends(&["m-alpha", "m-alpha"], TOLD), "told again");
    }

    #[test]
    fn a_lock_held_once_in_the_minute_starts_the_count_over() {
        let (mut view, mut fake, t0) = (View::new(sample(), true), Fake::with(&["m-alpha"]), Instant::now());
        for s in 0..40 {
            fake.tick(&mut view, secs(t0, s), false);
        }
        fake.tick(&mut view, secs(t0, 40), true);
        for s in 41..=80 {
            fake.tick(&mut view, secs(t0, s), false);
        }
        assert!(fake.calls.is_empty(), "40 s down since the lock was held: {:?}", fake.calls);
        fake.tick(&mut view, secs(t0, 101), false);
        assert_eq!(fake.calls.len(), 1, "60 s since the lock was held: {:?}", fake.calls);
    }

    #[test]
    fn a_downtime_with_no_live_manager_says_nothing_and_a_failure_to_tell_or_list_is_the_log() {
        let t0 = Instant::now();
        let mut view = View::new(sample(), true);
        view.tree.engine_running = false;
        let mut run = |_: &Action| Ok(serde_json::json!({"ok": true}));
        assert!(view.watch_engine(t0, &mut run, &mut || Ok(Vec::new())).is_empty());
        let lines = view.watch_engine(secs(t0, 60), &mut run, &mut || Ok(Vec::new()));
        assert!(lines.iter().any(|l| l.contains("no live Manager")), "{lines:?}");

        let mut view = View::new(sample(), true);
        view.tree.engine_running = false;
        view.watch_engine(t0, &mut run, &mut || Ok(Vec::new()));
        let lines = view.watch_engine(secs(t0, 60), &mut run, &mut || Err("store busy".to_owned()));
        assert!(lines.iter().any(|l| l.contains("store busy")), "{lines:?}");

        let mut view = View::new(sample(), true);
        view.tree.engine_running = false;
        let mut refuse = |_: &Action| Err(Failure::Broken("no tmux".to_owned()));
        view.watch_engine(t0, &mut refuse, &mut || Ok(vec![SessionId::parse("m-alpha").unwrap()]));
        let lines = view.watch_engine(secs(t0, 60), &mut refuse, &mut || Ok(vec![SessionId::parse("m-alpha").unwrap()]));
        assert!(lines.iter().any(|l| l.contains("no tmux")), "{lines:?}");
    }

    // ---------- spacing v2, the grey title and no closing row to fold nothing (hs-d475, hs-d476, hs-d477): red until the tree draws them ----------

    #[test]
    fn a_context_with_nothing_to_fold_ends_with_its_last_rows_corner_branch_and_no_closing_row() {
        let rows = drawn(&mut View::new(sample(), true), 44, 26);
        let starts = |row: usize, text: String| assert!(rows[row].starts_with(&text), "row {row} starts with {text:?}: {rows:#?}");
        assert_eq!(rows[0], "", "the first block's empty row above");
        assert_eq!(rows[1], format!(" ╭{NEEDS_YOU} beta"));
        starts(2, format!(" ╰─{NEEDS_YOU} rel"));
        assert_eq!(rows[3], "", "one empty row between two blocks, not two (the Lead's assumption: adjacent margins collapse)");
        assert_eq!(rows[4], format!(" ╭{NEEDS_YOU} alpha"));
        starts(5, format!(" ├─{NEEDS_YOU} ship·f-bl"));
        starts(6, format!(" ├─{WORKING} ship"));
        starts(7, format!(" ├─{IDLE} sketch"));
        starts(8, format!(" ├─{STOPPED} old"));
        starts(9, format!(" ╰─{IDLE} ship·f-id"));
        assert_eq!(rows[10], "");
        assert_eq!(rows[11], format!(" ╭{STOPPED} gamma"));
        assert_eq!(rows.iter().filter(|r| r.trim() == "╰󰬪 new").count(), 1, "no terminator line but the live Context with no row's new row: {rows:#?}");
    }

    #[test]
    fn a_context_with_archived_rows_keeps_its_closing_row_and_its_rows_keep_the_branch() {
        let mut view = View::new(archived(), true);
        let rows = drawn(&mut view, 44, 26);
        let starts = |row: usize, text: String| assert!(rows[row].starts_with(&text), "row {row} starts with {text:?}: {rows:#?}");
        assert_eq!(rows[0], "");
        assert_eq!(rows[1], format!(" ╭{NEEDS_YOU} beta"));
        starts(2, format!(" ╰─{NEEDS_YOU} rel"));
        assert_eq!(rows[3], "");
        assert_eq!(rows[4], format!(" ╭{NEEDS_YOU} alpha"));
        starts(5, format!(" ├─{WORKING} ship"));
        starts(6, format!(" ├─{IDLE} sketch"));
        starts(7, format!(" ├─{IDLE} ship·f-id"));
        starts(8, format!(" ├─{STOPPED} old"));
        starts(9, format!(" ├─{NEEDS_YOU} ship·f-bl"));
        assert_eq!(rows[10], COLLAPSE, "the closing row of the expanded Context");
        assert_eq!(rows[11], "");
        assert_eq!(rows[12], format!(" ╭{STOPPED} gamma"));
        assert_eq!(rows[13], " ╰󰬪 new", "a live Context with no row at all ends with the clickable new row");
        assert_eq!(rows[14], "");
        assert_eq!(rows[15].trim(), ARCHIVE_DOWN, "the archive row follows the last live Context");
        // opening it shows the archived Context and its own closing row
        click(&mut view, 3, 15);
        let rows = drawn(&mut view, 44, 26);
        assert_eq!(rows[16], format!(" ╭{STOPPED} zeta"));
        assert_eq!(rows[17], EXPAND, "the folded Context with archived rows: `╰󰬦 expand`");
        assert_eq!(rows[18], "");
    }

    #[test]
    fn a_context_with_no_row_at_all_is_its_title_and_the_terminator() {
        let tree = super::super::fixture::tree(vec![super::super::fixture::context("gamma", vec![])]);
        let rows = drawn(&mut View::new(tree, true), 44, 10);
        assert_eq!(rows[..3], ["".to_owned(), format!(" ╭{STOPPED} gamma"), " ╰󰬪 new".to_owned()]);
    }

    // ---------- no notes (commands.md §17 "No notes", hs-d469) ----------
    //
    // The view draws no transient note line: what the User must see is a `text` dialog (`Effect::Text`), the rest is
    // nothing on the screen and a line in `run/tui/tree.log`. Every test below compares frames, so none names how the
    // implementation keeps its state.

    /// The frame of `view` at 43 × 18, as text.
    fn frame(view: &mut View) -> Vec<String> {
        text_rows(&draw_now(view, 43, 18))
    }

    #[test]
    fn a_success_of_any_action_draws_nothing_and_opens_nothing() {
        for (action, said) in [
            (Action::Open(sid("l-sketch")), ""),
            (Action::EnsureWatch, "already running"),
            (Action::CopyId("l-sketch".into()), "copied l-sketch"),
        ] {
            let mut view = on(&["alpha", "sketch"]);
            let before = frame(&mut view);
            assert_eq!(view.finished(&action, Ok(said.into())), Effect::None, "{action:?}");
            assert_eq!(frame(&mut view), before, "{action:?}");
        }
    }

    #[test]
    fn a_failed_action_is_a_text_dialog_with_what_failed_its_code_and_its_message_and_draws_no_note() {
        for (action, why) in [
            (Action::Open(sid("l-sketch")), "no_cockpit: No cockpit."),
            (Action::EnsureWatch, "watch_start_failed: no tmux"),
        ] {
            let mut view = on(&["alpha", "sketch"]);
            let before = frame(&mut view);
            let Effect::Text { title, text } = view.finished(&action, Err(why.into())) else { panic!("{action:?}: a text dialog") };
            assert_eq!(text, why, "`<code>: <message>`");
            assert_eq!(title, action.argv().join(" "), "it says what failed, the command");
            assert_eq!(frame(&mut view), before, "{action:?}: no note behind the dialog");
        }
    }

    #[test]
    fn a_refusal_of_a_key_or_an_item_is_a_text_dialog_not_a_note() {
        // `x` on the Session in the chat slot
        let mut view = on(&["alpha", "ship"]);
        let before = frame(&mut view);
        let Effect::Text { title, text } = press(&mut view, KeyCode::Char('x')) else { panic!("a text dialog") };
        assert_eq!((title.as_str(), text.as_str()), ("Close Session", "switch to another session first"));
        assert_eq!(frame(&mut view), before);
        // New Context… with no owner policy
        let (mut view, _dir) = owned(sample(), &["notes.md"]);
        let before = frame(&mut view);
        let Effect::Text { title, text } = pick_at(&mut view, 8, 17, 0) else { panic!("a text dialog") };
        assert!((title.as_str(), text.contains("No owner policy")) == ("New Context", true), "{title:?} {text:?}");
        assert_eq!(frame(&mut view), before);
    }

    #[test]
    fn an_engine_down_for_60_seconds_tells_the_manager_and_draws_no_note() {
        let (mut view, mut fake, t0) = (View::new(sample(), true), Fake::with(&["m-alpha"]), Instant::now());
        fake.tick(&mut view, t0, false);
        let before = frame(&mut view);
        for s in 1..=60 {
            fake.tick(&mut view, secs(t0, s), false);
        }
        assert_eq!(fake.calls, sends(&["m-alpha"], TOLD), "the Manager is told");
        assert_eq!(frame(&mut view), before, "and the view says nothing: no `engine down 60 s+: Managers told`");
    }

    #[test]
    fn a_failure_to_tell_or_list_the_managers_is_logged_and_draws_no_note() {
        let t0 = Instant::now();
        let mut run = |_: &Action| Ok(serde_json::json!({"ok": true}));
        let mut view = View::new(sample(), true);
        view.tree.engine_running = false;
        let before = frame(&mut view);
        view.watch_engine(t0, &mut run, &mut || Ok(Vec::new()));
        let lines = view.watch_engine(secs(t0, 60), &mut run, &mut || Err("store busy".to_owned()));
        assert!(lines.iter().any(|l| l.contains("store busy")), "logged: {lines:?}");
        assert_eq!(frame(&mut view), before, "listing failed");

        let mut view = View::new(sample(), true);
        view.tree.engine_running = false;
        let mut refuse = |_: &Action| Err(Failure::Broken("no tmux".to_owned()));
        let manager = || -> Result<Vec<SessionId>, String> { Ok(vec![SessionId::parse("m-alpha").unwrap()]) };
        view.watch_engine(t0, &mut refuse, &mut manager.clone());
        let lines = view.watch_engine(secs(t0, 60), &mut refuse, &mut manager.clone());
        assert!(lines.iter().any(|l| l.contains("no tmux")), "logged: {lines:?}");
        assert_eq!(frame(&mut view), before, "telling failed");
    }

    #[test]
    fn a_fold_file_that_is_reset_draws_nothing_and_logs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run/tui/tree-folds.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{not json").unwrap();
        let mut clean = View::new(archived(), true);
        let plain = drawn(&mut clean, 44, 30);
        let mut view = View::new(archived(), true);
        let lines = view.keep_folds(file_in(&dir));
        assert_eq!(drawn(&mut view, 44, 30), plain, "no `fold state reset: …` line");
        assert!(lines.iter().any(|l| l.starts_with("fold state reset: ") && l.contains("tree-folds.json")), "logged: {lines:?}");
    }
}
