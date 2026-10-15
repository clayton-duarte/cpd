//! The tree view's state and keys (the reference's `run_context`, without the
//! terminal): what a key, a click or a focus change does, and how the view is
//! drawn. Pure: an action to run is returned as an [`Effect`], and the caller
//! runs it, then hands the result to [`View::done`].
//!
//! Keys (commands.md §13.2): ↑↓ and j/k move the cursor, Enter opens the cursor's Session, `n` starts a new Plan,
//! `x` closes a Session's pane, `?` opens the legend. No other key does anything.

use std::collections::HashSet;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Position};
use ratatui::widgets::{Paragraph, Wrap};
use tui_tree_widget::TreeState;

use super::actions::{Action, IN_CHAT_SLOT};
use super::cursor::{self, Input};
use super::prompt::{Outcome, Prompt, Target};
use super::rows::{self, Id, Row};
use cpd_core::session::State;
use crate::tui::palette;
use crate::tui::read::Tree;

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
        rows::row(&self.tree, self.state.selected()).filter(|_| self.cursor_shown())
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
                if let Some((row, session)) = self.cursor_row().and_then(|r| Some((r, r.session()?))) {
                    if session.is_active() {
                        self.note = IN_CHAT_SLOT.into();
                    } else {
                        let target = Target { id: session.id.clone(), label: row.label(), working: session.state == State::Working };
                        self.prompt = Some(Prompt::close(target));
                    }
                }
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
        if self.prompt.is_some() || m.kind != MouseEventKind::Down(MouseButton::Left) {
            return Effect::None;
        }
        self.note.clear();
        self.last_input = Input::Click;
        let Some(path) = self.state.rendered_at(Position::new(m.column, m.row)).map(<[Id]>::to_vec) else { return Effect::None };
        // a Context row only collapses or expands; a Session row opens it
        if rows::click_context(&mut self.state, &path) {
            return Effect::None;
        }
        match rows::row(&self.tree, &path).and_then(|r| r.session()) {
            Some(s) => Effect::Run(Action::Open(s.id.clone())),
            None => Effect::None,
        }
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
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyModifiers, MouseEvent};
    use ratatui::style::Modifier;

    use super::super::fixture::sample;
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
        assert_eq!(view.footer(), "Fork sketch into a new Plan, name: █ (Enter starts, Esc cancels)");
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert_eq!(view.footer(), "A name is required. Fork sketch into a new Plan, name: █ (Enter starts, Esc cancels)");
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
        assert_eq!(view.footer(), "Close sketch? pi stops; the session file stays (y/N)");
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
        assert_eq!(view.footer(), "Fork sketch into a new Plan, name: qnxjk?█ (Enter starts, Esc cancels)");
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
        assert_eq!(rows[8], "○ gamma");
        assert_eq!(rows[11], "Fork sketch into a new Plan, name: █ (Enter");
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
}
