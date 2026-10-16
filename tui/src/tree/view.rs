//! The tree view's state and keys (the reference's `run_context`, without the
//! terminal): what a key, a click or a focus change does, and how the view is
//! drawn. Pure: an action to run is returned as an [`Effect`], and the caller
//! runs it, then hands the result to [`View::done`].
//!
//! Keys (commands.md §13.2): ↑↓ and j/k move the cursor, Enter opens the cursor's Session, `n` starts a new Plan,
//! `x` closes a Session's pane, `?` opens the legend. No other key does anything. A right click opens the menu
//! ([`super::menu`]), which then takes every key and click.

use std::collections::HashSet;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::widgets::{Paragraph, Wrap};
use tui_tree_widget::TreeState;

use super::actions::{Action, IN_CHAT_SLOT, Subject};
use super::cursor::{self, Input};
use super::menu::{self, Item, Kind, Menu};
use super::prompt::{Outcome, Prompt, Target};
use super::rows::{self, Id, Row};
use cpd_core::session::State;
use crate::palette;
use crate::read::Tree;

/// The note of an item that needs typed text, which this view does not take yet.
const NOT_YET: &str = "not yet";

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
    /// The area of the last frame: the menu is clamped inside it and hit-tested against it.
    pane: Rect,
    /// The one-line result or refusal of the last action, until the next `n`, `x` or click.
    pub note: String,
}

impl View {
    pub fn new(tree: Tree, focused: bool) -> View {
        let mut view = View {
            tree: Tree { contexts: Vec::new(), events_cursor: 0 },
            state: TreeState::default(),
            known: HashSet::new(),
            focused,
            last_input: Input::Click,
            prompt: None,
            menu: None,
            pane: Rect::default(),
            note: String::new(),
        };
        view.refresh(tree);
        view
    }

    /// A new snapshot: keep the cursor on its Session.
    pub fn refresh(&mut self, tree: Tree) {
        let before = rows::selected_index(&self.state, &self.tree);
        self.tree = tree;
        rows::open_new(&mut self.state, &mut self.known, &self.tree);
        rows::reselect(&mut self.state, &self.tree, before);
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

    /// The result of an [`Effect::Run`] or [`Effect::Legend`].
    pub fn done(&mut self, result: Result<String, String>) {
        self.note = result.unwrap_or_else(|e| e);
    }

    /// The cursor's row, while the cursor shows: Enter, `n` and `x` act on it.
    fn cursor_row(&self) -> Option<Row<'_>> {
        rows::row(&self.tree, self.state.selected()).filter(|r| self.cursor_shown() && !r.archived())
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
                Outcome::NewPlan { context, id, repo } => {
                    self.prompt = None;
                    Effect::Run(Action::NewPlan { context, id, repo })
                }
                Outcome::Close { id } => {
                    self.prompt = None;
                    Effect::Run(Action::Close(id))
                }
                Outcome::Archive { subject } => {
                    self.prompt = None;
                    Effect::Run(Action::Archive(subject))
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
                    self.prompt = Some(Prompt::name(target, plan.head.context.clone(), plan.head.repo.clone()));
                }
            }
            KeyCode::Char('x') => {
                self.note.clear();
                let asked = self.cursor_row().and_then(close_of);
                self.ask(asked);
            }
            KeyCode::Char('?') => return Effect::Legend,
            // the first arrow after a click only shows the cursor again where it was (the first Session when there is none)
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Up | KeyCode::Char('k') => {
                let delta = if matches!(key.code, KeyCode::Down | KeyCode::Char('j')) { 1 } else { -1 };
                if self.cursor_shown() {
                    rows::move_cursor(&mut self.state, &self.tree, delta);
                } else if self.state.selected().is_empty() {
                    rows::move_cursor(&mut self.state, &self.tree, 1);
                }
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

    pub fn mouse(&mut self, m: MouseEvent) -> Effect {
        if self.prompt.is_some() {
            return Effect::None;
        }
        let at = Position::new(m.column, m.row);
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
        if item.needs_text() {
            self.note = NOT_YET.into();
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
                let prompt = subject_of(&row).map(|subject| Prompt::archive(subject, row.label()));
                self.ask(prompt.map(Ok));
                Effect::None
            }
            Item::Unarchive => subject_of(&row).map_or(Effect::None, |subject| Effect::Run(Action::Unarchive(subject))),
            Item::NewPlan | Item::Rename | Item::NewSession | Item::Fork | Item::NewContext => Effect::None,
        }
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
        let Some(path) = self.row_at(Position::new(m.column, m.row)) else { return Effect::None };
        // a `…` row expands; the dotted line of an expanded group collapses it
        if rows::click_group(&mut self.state, &path, m.column) {
            return Effect::None;
        }
        // a Context row only collapses or expands; a live Session row opens it; an archived row does nothing
        if rows::click_context(&mut self.state, &self.tree, &path) {
            return Effect::None;
        }
        match rows::row(&self.tree, &path).filter(|r| !r.archived()).and_then(|r| r.session()) {
            Some(s) => Effect::Run(Action::Open(s.id.clone())),
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

    /// Draw the tree, and (when there is something to say) the footer in the rows at the bottom: the
    /// reference's layout, the prompt bold.
    pub fn draw(&mut self, f: &mut Frame) {
        let area = f.area();
        let footer = self.footer();
        let width = area.width.max(1) as usize;
        let wrapped = footer.chars().count().div_ceil(width);
        let footer_height =
            if footer.is_empty() { 0 } else { (wrapped + usize::from(wrapped > 1)).max(1).min(area.height as usize) as u16 };
        let [list, footer_area] = Layout::vertical([Constraint::Min(0), Constraint::Length(footer_height)]).areas(area);
        if footer_height > 0 {
            let style = if self.prompt.is_some() { palette::note().bold() } else { palette::note() };
            f.render_widget(Paragraph::new(footer).style(style).wrap(Wrap { trim: false }), footer_area);
        }
        let cursor = self.cursor_shown();
        rows::draw(f, list, &self.tree, &mut self.state, cursor);
        self.pane = area;
        if let Some(menu) = &self.menu {
            menu.draw(f, area);
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
    use cpd_core::ids::{ContextId, PlanId, SessionId};

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
        terminal.draw(|f| view.draw(f)).unwrap();
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
        assert_eq!(view.state.selected(), ["alpha", "ship", "f-idle"]);
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
        assert_eq!(press(&mut on(&["alpha", "ship", "f-blocked"]), KeyCode::Enter), Effect::Run(Action::Open(sid("f-blocked"))));
    }

    #[test]
    fn n_asks_for_a_name_and_starts_a_plan_in_the_context_and_repo_of_the_session() {
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
                repo: "/repo/sketch".into(),
            })
        );
        assert!(view.prompt.is_none());
        let mut view = on(&["alpha", "ship", "f-idle"]);
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
        view.state.select(vec!["alpha".into(), "ship".into(), "f-idle".into()]);
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
        assert_eq!(rows[8], "○ Gamma");
        assert_eq!(rows[11], "Fork Sketch into a new Plan, name: █ (Enter");
        assert_eq!(rows[12], "starts, Esc cancels)");
        assert_eq!(rows[13], "", "two wrapped rows take three");
        let mut terminal = Terminal::new(TestBackend::new(44, 14)).unwrap();
        terminal.draw(|f| view.draw(f)).unwrap();
        let cell = &terminal.backend().buffer()[(0, 11)];
        assert_eq!((cell.fg, cell.modifier.contains(Modifier::BOLD)), (palette::ui(palette::Ui::Note), true));
        press(&mut view, KeyCode::Esc);
        let rows = drawn(&mut view, 44, 14);
        assert!(rows[9..].iter().all(|r| r.is_empty()), "{rows:?}");
        view.done(Err("closed".into()));
        let rows = drawn(&mut view, 44, 14);
        assert_eq!(rows[13], "closed", "a note is one row at the bottom");
    }

    fn click(view: &mut View, column: u16, row: u16) -> Effect {
        view.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE })
    }

    #[test]
    fn a_click_folds_a_context_and_opens_a_session_and_hides_the_cursor() {
        let mut view = on(&["alpha", "sketch"]);
        drawn(&mut view, 44, 10);
        assert_eq!(click(&mut view, 5, 0), Effect::None, "the Context row `alpha`");
        assert!(!view.state.opened().contains(&vec!["alpha".to_owned()]));
        drawn(&mut view, 44, 10);
        assert_eq!(view.state.selected(), ["alpha", "sketch"], "a click selects nothing");
        assert!(!view.cursor_shown());
        click(&mut view, 5, 0);
        drawn(&mut view, 44, 10);
        assert_eq!(click(&mut view, 8, 1), Effect::Run(Action::Open(sid("l-ship"))), "the Plan row");
        assert_eq!(click(&mut view, 8, 3), Effect::Run(Action::Open(sid("f-blocked"))), "a fork row");
        assert_eq!(click(&mut view, 8, 9), Effect::None, "below the tree");
        assert_eq!(view.state.selected(), ["alpha", "sketch"], "no click moved the cursor");
    }

    #[test]
    fn clicks_are_ignored_while_a_prompt_is_open() {
        let mut view = on(&["alpha", "sketch"]);
        drawn(&mut view, 44, 10);
        press(&mut view, KeyCode::Char('n'));
        assert_eq!(click(&mut view, 8, 1), Effect::None);
        assert_eq!(click(&mut view, 5, 0), Effect::None);
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
    fn a_click_on_a_more_row_expands_it_and_a_click_on_its_dotted_line_collapses_it() {
        let mut view = View::new(archived(), true);
        let rows = drawn(&mut view, 44, 16);
        assert_eq!((rows[4].as_str(), rows[8].as_str()), ("  └ …", "…"));
        assert_eq!(click(&mut view, 3, 4), Effect::None, "the `…` of alpha");
        let rows = drawn(&mut view, 44, 16);
        assert_eq!(rows[4], "  ┊");
        assert!(rows[5].starts_with("  ┊ ┆ ○ Old"), "{rows:?}");
        assert_eq!(rows[6], "  ┊ ┆ ○ ship review (Ship)");
        assert_eq!(rows[10], "…");
        assert_eq!(click(&mut view, 0, 10), Effect::None, "the `…` of the archived Contexts");
        let rows = drawn(&mut view, 44, 16);
        assert_eq!((rows[10].as_str(), rows[11].as_str()), ("┊", "┊ ○ Zeta"));
        // the dotted line of a group collapses it again, in any of its rows
        assert_eq!(click(&mut view, 0, 12), Effect::None);
        let rows = drawn(&mut view, 44, 16);
        assert_eq!((rows[10].as_str(), rows[11].as_str()), ("…", ""));
        assert_eq!(click(&mut view, 2, 5), Effect::None);
        let rows = drawn(&mut view, 44, 16);
        assert_eq!((rows[4].as_str(), rows[5].as_str()), ("  └ …", "● Beta"));
    }

    #[test]
    fn a_click_below_the_last_row_expands_nothing() {
        let mut view = View::new(archived(), true);
        drawn(&mut view, 44, 12);
        assert_eq!(click(&mut view, 0, 11), Effect::None);
        assert_eq!(click(&mut view, 3, 9), Effect::None, "the row right below the last");
        assert!(drawn(&mut view, 44, 12).iter().all(|r| !r.contains('┊')), "the `…` rows are at rows 4 and 8");
        let _ = right_click(&mut view, 0, 8);
        assert!(drawn(&mut view, 44, 12).iter().all(|r| !r.contains('┊')), "only the left button clicks");
    }

    #[test]
    fn a_click_on_an_archived_row_opens_nothing_and_enter_n_and_x_never_act_on_one() {
        let mut view = View::new(archived(), true);
        drawn(&mut view, 44, 16);
        click(&mut view, 3, 4);
        drawn(&mut view, 44, 16);
        click(&mut view, 0, 10);
        let rows = drawn(&mut view, 44, 16);
        assert!(rows[5].starts_with("  ┊ ┆ ○ Old") && rows[11] == "┊ ○ Zeta", "{rows:?}");
        // the Plan, its fork, the stray fork and the archived Plan of the archived Context
        for (column, row) in [(8, 5), (8, 6), (8, 12), (8, 13)] {
            assert_eq!(click(&mut view, column, row), Effect::None, "({column}, {row})");
            assert_eq!(drawn(&mut view, 44, 16), rows, "({column}, {row}) changes nothing");
        }
        // the cursor is put on archived rows by hand, as a bug might: no key acts on it
        press(&mut view, KeyCode::Down);
        for path in [&["alpha", "…", "old"][..], &["alpha", "…", "fork:f-blocked"], &["…", "zeta", "zp"], &["…", "zeta", "zp", "f-zp"], &["alpha", "…"]] {
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
        assert_eq!(view.state.selected(), ["beta", "rel"], "the Session at its place, now");
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
        assert_eq!(menu_at(&mut view, 5, 0), ["New Plan…", "Rename…", "Archive"], "a Context");
        assert_eq!(menu_at(&mut view, 8, 1), ["New session…", "Rename…", "Archive"], "a Plan");
        assert_eq!(menu_at(&mut view, 8, 2), ["Open", "Rename…", "Fork…", "Close", "Archive"], "a fork");
        assert_eq!(menu_at(&mut view, 8, 14), ["New Context…"], "the empty area");
        assert_eq!(menu_at(&mut view, 8, 10), ["New Context…"], "below the last row");
        assert_eq!(view.note, "");
        let mut view = View::new(View::new(archived(), true).tree, true);
        assert_eq!(menu_at(&mut view, 8, 0), ["New Plan…", "Rename…", "Archive"], "a live row of a tree with archived ones");
    }

    #[test]
    fn a_right_click_on_an_archived_row_offers_unarchive_only_and_on_a_more_row_nothing() {
        let mut view = View::new(archived(), true);
        drawn(&mut view, 44, 16);
        click(&mut view, 3, 4);
        drawn(&mut view, 44, 16);
        click(&mut view, 0, 10);
        let rows = drawn(&mut view, 44, 16);
        assert!(rows[5].starts_with("  ┊ ┆ ○ Old") && rows[11] == "┊ ○ Zeta", "{rows:?}");
        // the archived Plan, its fork, an archived Context and its Plan, and an archived fork of a live Plan
        for (column, row) in [(8, 5), (8, 6), (8, 11), (8, 12), (8, 13)] {
            assert_eq!(menu_at(&mut view, column, row), ["Unarchive"], "({column}, {row})");
            press(&mut view, KeyCode::Esc);
        }
        for (column, row) in [(3, 4), (0, 10), (0, 15)] {
            let _ = right_click(&mut view, column, row);
            assert_eq!(view.menu.is_some(), row == 15, "({column}, {row}): `…` rows have none, the empty area has one");
            view.menu = None;
        }
    }

    #[test]
    fn the_menu_is_drawn_at_the_click_and_clamped_inside_the_pane() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 2);
        let rows = drawn(&mut view, 44, 16);
        assert_eq!(rows[2], "  │ ├ ○ ┌─────────┐", "{rows:?}");
        assert_eq!(rows[3], "  │ └ ● │ Open    │");
        assert!(rows[4].starts_with("  ├ ○ Sk│ Rename… │"), "drawn over the tree: {rows:?}");
        assert_eq!(rows[8], "○ Gamma └─────────┘");
        let reversed = |view: &mut View, x, y| {
            let mut terminal = Terminal::new(TestBackend::new(44, 16)).unwrap();
            terminal.draw(|f| view.draw(f)).unwrap();
            terminal.backend().buffer()[(x, y)].modifier.contains(Modifier::REVERSED)
        };
        assert!(reversed(&mut view, 10, 3), "the first item is selected");
        press(&mut view, KeyCode::Down);
        assert!(reversed(&mut view, 10, 4) && !reversed(&mut view, 10, 3));
        view.menu = None;
        right_click(&mut view, 43, 15);
        let rows = drawn(&mut view, 44, 16);
        let box_row = |left: char, fill: &str, right: char| format!("{}{left}{fill}{right}", " ".repeat(28));
        assert_eq!((rows[13].clone(), rows[14].clone(), rows[15].clone()), (box_row('┌', &"─".repeat(14), '┐'), box_row('│', " New Context… ", '│'), box_row('└', &"─".repeat(14), '┘')));
    }

    #[test]
    fn enter_on_the_menu_picks_its_selected_item_and_esc_closes_it_running_nothing() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 2); // a fork: f-idle
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::Open(sid("f-idle"))));
        assert!(view.menu.is_none());
        right_click(&mut view, 8, 2);
        assert_eq!(press(&mut view, KeyCode::Esc), Effect::None);
        assert!(view.menu.is_none() && view.prompt.is_none());
        assert_eq!(view.footer(), "");
    }

    #[test]
    fn archive_asks_first_and_only_y_runs_it() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 2);
        for _ in 0..4 {
            press(&mut view, KeyCode::Down);
        }
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert!(view.menu.is_none());
        assert_eq!(view.footer(), "Archive ship fork? its panes close (y/N)");
        assert_eq!(press(&mut view, KeyCode::Char('y')), Effect::Run(Action::Archive(Subject::Session(sid("f-idle")))));
        assert!(view.prompt.is_none());
        right_click(&mut view, 8, 2);
        view.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 10, row: 7, modifiers: KeyModifiers::NONE });
        assert_eq!(view.footer(), "Archive ship fork? its panes close (y/N)", "a click picks");
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None, "anything but y cancels");
        assert!(view.prompt.is_none());
        // a Plan and a Context name their own objects
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 1);
        press(&mut view, KeyCode::Up);
        for _ in 0..2 {
            press(&mut view, KeyCode::Down);
        }
        press(&mut view, KeyCode::Enter);
        assert_eq!(view.footer(), "Archive Ship? its panes close (y/N)");
        assert_eq!(press(&mut view, KeyCode::Char('y')), Effect::Run(Action::Archive(Subject::Plan(PlanId::parse("ship").unwrap()))));
        drawn(&mut view, 44, 16);
        right_click(&mut view, 5, 0);
        for _ in 0..2 {
            press(&mut view, KeyCode::Down);
        }
        press(&mut view, KeyCode::Enter);
        assert_eq!(view.footer(), "Archive Alpha? its panes close (y/N)");
        assert_eq!(press(&mut view, KeyCode::Char('y')), Effect::Run(Action::Archive(Subject::Context(ContextId::parse("alpha").unwrap()))));
    }

    #[test]
    fn close_in_the_menu_asks_as_x_does_and_refuses_the_session_in_the_chat_slot() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 8, 2);
        for _ in 0..3 {
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
        right_click(&mut view, 8, 2);
        for _ in 0..3 {
            press(&mut view, KeyCode::Down);
        }
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert!(view.prompt.is_none());
        assert_eq!(view.footer(), IN_CHAT_SLOT);
    }

    #[test]
    fn unarchive_runs_without_a_prompt() {
        let mut view = View::new(archived(), true);
        drawn(&mut view, 44, 16);
        click(&mut view, 3, 4);
        drawn(&mut view, 44, 16);
        click(&mut view, 0, 10);
        drawn(&mut view, 44, 16);
        for (row, subject) in [
            (5, Subject::Plan(PlanId::parse("old").unwrap())),
            (6, Subject::Session(sid("f-blocked"))),
            (11, Subject::Context(ContextId::parse("zeta").unwrap())),
        ] {
            right_click(&mut view, 8, row);
            assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::Unarchive(subject)), "row {row}");
            assert!(view.prompt.is_none() && view.menu.is_none());
        }
    }

    #[test]
    fn the_items_that_need_text_say_not_yet_and_run_nothing() {
        let mut view = View::new(sample(), true);
        drawn(&mut view, 44, 16);
        for (column, row, index) in [(5, 0, 0), (5, 0, 1), (8, 1, 0), (8, 1, 1), (8, 2, 1), (8, 2, 2), (8, 14, 0)] {
            right_click(&mut view, column, row);
            for _ in 0..index {
                press(&mut view, KeyCode::Down);
            }
            assert_eq!(press(&mut view, KeyCode::Enter), Effect::None, "({column}, {row}) #{index}");
            assert_eq!(view.footer(), "not yet");
            assert!(view.menu.is_none() && view.prompt.is_none());
            view.note.clear();
        }
    }

    #[test]
    fn an_open_menu_takes_every_key_and_click() {
        let mut view = on(&["alpha", "sketch"]);
        drawn(&mut view, 44, 16);
        right_click(&mut view, 20, 3);
        for key in [KeyCode::Char('n'), KeyCode::Char('x'), KeyCode::Char('?'), KeyCode::Char('j'), KeyCode::Char('q')] {
            assert_eq!(press(&mut view, key), Effect::None, "{key:?}");
            assert!(view.prompt.is_none() && view.menu.is_some(), "{key:?}");
        }
        assert_eq!(view.state.selected(), ["alpha", "sketch"]);
        // a left click outside closes it and does not click what is there (the Context row `alpha` would fold)
        assert_eq!(click(&mut view, 2, 0), Effect::None);
        assert!(view.menu.is_none());
        assert!(view.state.opened().contains(&vec!["alpha".to_owned()]));
        // so does a right click outside, and neither opens another
        right_click(&mut view, 20, 3);
        assert_eq!(right_click(&mut view, 2, 0), Effect::None);
        assert!(view.menu.is_none());
        // a click on the border keeps it; a right click on an item picks nothing
        right_click(&mut view, 20, 3);
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
        right_click(&mut view, 8, 2);
        let rect = view.menu.as_ref().unwrap().rect(view.pane);
        assert_eq!(click(&mut view, rect.x + 2, rect.y + 1), Effect::Run(Action::Open(sid("f-idle"))));
        assert!(view.menu.is_none());
    }

    #[test]
    fn a_prompt_ignores_right_clicks_and_no_key_opens_the_menu() {
        let mut view = on(&["alpha", "sketch"]);
        drawn(&mut view, 44, 16);
        press(&mut view, KeyCode::Char('n'));
        right_click(&mut view, 8, 2);
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
        right_click(&mut view, 8, 2);
        let mut tree = sample();
        tree.contexts[0].plans[0].sessions.retain(|s| s.id.as_str() != "f-idle");
        view.refresh(tree);
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert!(view.menu.is_none());
    }
}
