//! The tree view's state and keys (the spike's `run_context`, without the
//! terminal): what a key, a click or a focus change does, and how the view is
//! drawn. Pure: an action to run is returned as an [`Effect`], and the caller
//! runs it, then hands the result to [`View::done`].

use std::collections::HashSet;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Position};
use ratatui::style::Style;
use ratatui::widgets::{Paragraph, Wrap};
use tui_tree_widget::TreeState;

use super::actions::Action;
use super::cursor::{self, Input};
use super::palette;
use super::prompt::{Outcome, Prompt, Target};
use super::rows::{self, Id, Row};
use super::separator;
use crate::tui::read::Tree;

/// What the caller does after an input.
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    /// End the view (`q`).
    Quit,
    /// Run this command, then call [`View::done`] with the result.
    Run(Action),
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
    /// The one-line result or refusal of the last action, until the next key.
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

    /// A new snapshot: keep the cursor on its row.
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

    /// The result of an [`Effect::Run`].
    pub fn done(&mut self, result: Result<String, String>) {
        self.note = result.unwrap_or_else(|e| e);
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
                Outcome::NewPlan { context, id, title, repo } => {
                    self.prompt = None;
                    Effect::Run(Action::NewPlan { context, id, title, repo })
                }
                Outcome::Fork { source, name } => {
                    self.prompt = None;
                    Effect::Run(Action::Fork { source, name })
                }
                Outcome::Rename { session, name } => {
                    self.prompt = None;
                    Effect::Run(Action::Rename { session, name })
                }
            };
        }
        match key.code {
            KeyCode::Char('q') => return Effect::Quit,
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Up | KeyCode::Char('k') => {
                let delta = if matches!(key.code, KeyCode::Down | KeyCode::Char('j')) { 1 } else { -1 };
                // the first arrow after a click only shows the cursor again where it was (the first row when there is none)
                if self.cursor_shown() {
                    rows::move_cursor(&mut self.state, &self.tree, delta);
                } else if self.state.selected().is_empty() {
                    rows::move_cursor(&mut self.state, &self.tree, 1);
                }
                self.last_input = Input::Arrow;
                return Effect::None;
            }
            _ => {}
        }
        self.note.clear();
        // the row under the cursor, when it is shown: the action keys act on it
        let shown = self.cursor_shown();
        let Some(row) = rows::row(&self.tree, self.state.selected()).filter(|_| shown) else { return Effect::None };
        let context = row.context();
        let target = |s: &crate::tui::read::SessionNode| Target { id: s.id.clone(), label: rows::session_label(s) };
        match key.code {
            KeyCode::Char('n') => {
                let repo = context.plans.last().map(|p| p.head.repo.clone());
                self.prompt = Some(Prompt::new_plan(context.id.clone(), repo));
            }
            KeyCode::Char('f') => match row.session() {
                Some(s) => self.prompt = Some(Prompt::fork(target(s))),
                None => self.note = "Pick a Plan or a fork to fork.".into(),
            },
            KeyCode::Char('r') => match row.session() {
                Some(s) => self.prompt = Some(Prompt::rename(target(s), &s.name)),
                None => self.note = "Pick a Plan or a fork to rename.".into(),
            },
            KeyCode::Char('p') => match row.session() {
                Some(s) => return Effect::Run(Action::Park(s.id.clone())),
                None => self.note = "Pick a Plan or a fork to park.".into(),
            },
            KeyCode::Char(' ') => {
                let path = self.state.selected().to_vec();
                rows::toggle(&mut self.state, &self.tree, &path);
            }
            KeyCode::Enter => match row {
                Row::Context(_) => {
                    let path = self.state.selected().to_vec();
                    rows::toggle(&mut self.state, &self.tree, &path);
                }
                _ => return self.open(row),
            },
            _ => {}
        }
        Effect::None
    }

    /// Open the Session of `row` in the chat slot.
    fn open(&self, row: Row<'_>) -> Effect {
        match row.session() {
            Some(s) => Effect::Run(Action::Open(s.id.clone())),
            None => Effect::None,
        }
    }

    pub fn mouse(&mut self, m: MouseEvent) -> Effect {
        if self.prompt.is_some() || m.kind != MouseEventKind::Down(MouseButton::Left) {
            return Effect::None;
        }
        self.note.clear();
        self.last_input = Input::Click;
        let Some(path) = self.state.rendered_at(Position::new(m.column, m.row)).map(<[Id]>::to_vec) else { return Effect::None };
        // a Context row only collapses or expands; a Plan or a fork opens its Lead
        match rows::row(&self.tree, &path) {
            Some(Row::Context(_)) => {
                rows::toggle(&mut self.state, &self.tree, &path);
                Effect::None
            }
            Some(row) => self.open(row),
            None => Effect::None,
        }
    }

    /// The text of the footer: the prompt, else the note of the last action.
    fn footer(&self) -> String {
        self.prompt.as_ref().map(Prompt::text).unwrap_or_else(|| self.note.clone())
    }

    /// Draw the tree, then (when there is something to say) a separator and the footer.
    pub fn draw(&mut self, f: &mut Frame) {
        let area = f.area();
        let footer = self.footer();
        let width = area.width.max(1) as usize;
        let lines = footer.chars().count().div_ceil(width);
        let footer_height = if footer.is_empty() { 0 } else { (lines + usize::from(lines > 1)).max(1) as u16 };
        let rule = if footer.is_empty() { 0 } else { separator::HEIGHT };
        let footer_height = footer_height.min(area.height.saturating_sub(rule));
        let [list, rule_area, footer_area] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(rule), Constraint::Length(footer_height)]).areas(area);
        if footer_height > 0 {
            separator::draw(f, rule_area);
            let style = if self.prompt.is_some() { Style::new().bold() } else { Style::new() };
            f.render_widget(Paragraph::new(footer).style(palette::note().patch(style)).wrap(Wrap { trim: false }), footer_area);
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

    use super::super::fixture::sample;
    use super::*;
    use crate::args::Name;
    use crate::ids::{ContextId, PlanId, SessionId};

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
        assert_eq!(view.state.selected(), ["alpha"], "the first row is selected from the start");
        for key in [KeyCode::Enter, KeyCode::Char('p'), KeyCode::Char('f'), KeyCode::Char('r'), KeyCode::Char('n')] {
            assert_eq!(press(&mut view, key), Effect::None);
        }
        assert!(view.prompt.is_none());
        press(&mut view, KeyCode::Down);
        assert_eq!(view.state.selected(), ["alpha"], "the first arrow only shows the cursor");
        press(&mut view, KeyCode::Char('j'));
        assert_eq!(view.state.selected(), ["alpha", "ship"]);
        press(&mut view, KeyCode::Char('k'));
        assert_eq!(view.state.selected(), ["alpha"]);
    }

    #[test]
    fn a_click_or_losing_focus_hides_the_cursor_until_the_next_arrow() {
        let mut view = on(&["alpha", "ship"]);
        view.focus(false);
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        view.focus(true);
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None, "focus back is not a key");
        press(&mut view, KeyCode::Down);
        assert_eq!(view.state.selected(), ["alpha", "ship"], "only shows it again");
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::Open(sid("l-ship"))));
    }

    #[test]
    fn q_ends_the_view_whatever_the_cursor() {
        assert_eq!(press(&mut View::new(sample(), false), KeyCode::Char('q')), Effect::Quit);
    }

    #[test]
    fn enter_opens_the_lead_of_a_plan_row_and_a_fork() {
        assert_eq!(press(&mut on(&["alpha", "ship"]), KeyCode::Enter), Effect::Run(Action::Open(sid("l-ship"))));
        assert_eq!(press(&mut on(&["alpha", "ship", "f-blocked"]), KeyCode::Enter), Effect::Run(Action::Open(sid("f-blocked"))));
    }

    #[test]
    fn enter_on_a_context_folds_it_and_space_folds_a_plan_with_forks() {
        let mut view = on(&["alpha"]);
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert!(!view.state.opened().contains(&vec!["alpha".to_owned()]));
        press(&mut view, KeyCode::Enter);
        assert!(view.state.opened().contains(&vec!["alpha".to_owned()]));
        let mut view = on(&["alpha", "ship"]);
        assert_eq!(press(&mut view, KeyCode::Char(' ')), Effect::None);
        assert!(!view.state.opened().contains(&vec!["alpha".to_owned(), "ship".to_owned()]));
    }

    #[test]
    fn p_parks_the_session_of_the_row() {
        assert_eq!(press(&mut on(&["alpha", "ship"]), KeyCode::Char('p')), Effect::Run(Action::Park(sid("l-ship"))));
        assert_eq!(press(&mut on(&["alpha", "ship", "f-idle"]), KeyCode::Char('p')), Effect::Run(Action::Park(sid("f-idle"))));
        let mut view = on(&["gamma"]);
        assert_eq!(press(&mut view, KeyCode::Char('p')), Effect::None);
        assert_eq!(view.note, "Pick a Plan or a fork to park.");
    }

    #[test]
    fn f_forks_with_a_name_or_the_default() {
        let mut view = on(&["alpha", "ship"]);
        assert_eq!(press(&mut view, KeyCode::Char('f')), Effect::None);
        assert_eq!(view.footer(), "Fork ship lead, name: █ (empty: default; Enter forks, Esc cancels)");
        typed(&mut view, "  my fork ");
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Effect::Run(Action::Fork { source: sid("l-ship"), name: Some("my fork".parse::<Name>().unwrap()) })
        );
        assert!(view.prompt.is_none());
        let mut view = on(&["alpha", "ship", "f-idle"]);
        press(&mut view, KeyCode::Char('f'));
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::Fork { source: sid("f-idle"), name: None }));
    }

    #[test]
    fn a_fork_name_longer_than_80_characters_is_refused_in_the_prompt() {
        let mut view = on(&["alpha", "ship"]);
        press(&mut view, KeyCode::Char('f'));
        typed(&mut view, &"x".repeat(81));
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert!(view.footer().starts_with("A name must be 1 to 80 characters"), "{}", view.footer());
    }

    #[test]
    fn r_renames_starting_from_the_current_name() {
        let mut view = on(&["alpha", "ship"]);
        press(&mut view, KeyCode::Char('r'));
        assert_eq!(view.footer(), "Rename ship lead to: ship lead█ (Enter renames, Esc cancels)");
        for _ in 0..4 {
            press(&mut view, KeyCode::Backspace);
        }
        typed(&mut view, "boss");
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Effect::Run(Action::Rename { session: sid("l-ship"), name: "ship boss".parse().unwrap() })
        );
    }

    #[test]
    fn an_empty_name_is_refused_and_the_prompt_stays() {
        let mut view = on(&["alpha", "ship"]);
        press(&mut view, KeyCode::Char('r'));
        for _ in 0..20 {
            press(&mut view, KeyCode::Backspace);
        }
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert!(view.footer().starts_with("A name must be 1 to 80 characters"), "{}", view.footer());
        typed(&mut view, "x");
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::Run(Action::Rename { session: sid("l-ship"), name: "x".parse().unwrap() }));
    }

    #[test]
    fn n_starts_a_plan_in_the_context_of_the_row_taking_the_repo_of_its_newest_plan() {
        let mut view = on(&["alpha", "sketch"]);
        press(&mut view, KeyCode::Char('n'));
        assert_eq!(view.footer(), "New Plan in alpha, id: █ (Enter: next, Esc cancels)");
        typed(&mut view, "pay-4521");
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert_eq!(view.footer(), "New Plan in alpha, title: █ (Enter starts, Esc cancels)");
        typed(&mut view, "Retry failed payouts");
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Effect::Run(Action::NewPlan {
                context: ContextId::parse("alpha").unwrap(),
                id: PlanId::parse("pay-4521").unwrap(),
                title: "Retry failed payouts".into(),
                repo: "/repo/old".into(),
            })
        );
    }

    #[test]
    fn n_in_a_context_without_plans_asks_for_the_repo() {
        let mut view = on(&["gamma"]);
        press(&mut view, KeyCode::Char('n'));
        typed(&mut view, "first");
        press(&mut view, KeyCode::Enter);
        typed(&mut view, "The first");
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert_eq!(view.footer(), "New Plan in gamma, repo: █ (Enter starts, Esc cancels)");
        typed(&mut view, "/work/repo");
        assert_eq!(
            press(&mut view, KeyCode::Enter),
            Effect::Run(Action::NewPlan {
                context: ContextId::parse("gamma").unwrap(),
                id: PlanId::parse("first").unwrap(),
                title: "The first".into(),
                repo: "/work/repo".into(),
            })
        );
    }

    #[test]
    fn a_bad_plan_id_or_an_empty_value_is_refused_in_the_prompt() {
        let mut view = on(&["alpha"]);
        press(&mut view, KeyCode::Char('n'));
        assert_eq!(press(&mut view, KeyCode::Enter), Effect::None);
        assert!(view.footer().starts_with("A value is required. New Plan in alpha, id:"), "{}", view.footer());
        typed(&mut view, "Bad Id");
        press(&mut view, KeyCode::Enter);
        assert!(view.footer().starts_with("Invalid Plan id \"Bad Id\""), "{}", view.footer());
        assert!(view.footer().contains("id: Bad Id█"), "the prompt stays on the id");
    }

    #[test]
    fn a_prompt_takes_every_key_and_esc_cancels_it() {
        let mut view = on(&["alpha", "ship"]);
        press(&mut view, KeyCode::Char('f'));
        typed(&mut view, "qnpjf");
        assert_eq!(view.footer(), "Fork ship lead, name: qnpjf█ (empty: default; Enter forks, Esc cancels)");
        assert_eq!(view.state.selected(), ["alpha", "ship"]);
        assert_eq!(press(&mut view, KeyCode::Esc), Effect::None);
        assert!(view.prompt.is_none());
        assert_eq!(view.footer(), "");
    }

    #[test]
    fn the_note_of_an_action_stays_until_the_next_key() {
        let mut view = on(&["alpha", "ship"]);
        view.done(Err("no_cockpit: No cockpit.".into()));
        assert_eq!(view.note, "no_cockpit: No cockpit.");
        press(&mut view, KeyCode::Char('x'));
        assert_eq!(view.note, "");
    }

    #[test]
    fn the_footer_is_drawn_under_a_separator_and_the_tree_keeps_the_rest() {
        let mut view = on(&["alpha", "ship"]);
        press(&mut view, KeyCode::Char('f'));
        let rows = drawn(&mut view, 44, 14);
        assert_eq!(rows[8], "○ gamma");
        assert_eq!(rows[10], "─".repeat(44));
        assert_eq!(rows[11], "Fork ship lead, name: █ (empty: default;");
        assert_eq!(rows[12], "Enter forks, Esc cancels)");
        press(&mut view, KeyCode::Esc);
        let rows = drawn(&mut view, 44, 14);
        assert!(rows[9..].iter().all(|r| r.is_empty()), "{rows:?}");
    }

    fn click(view: &mut View, column: u16, row: u16) -> Effect {
        view.mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE })
    }

    #[test]
    fn a_click_folds_a_context_and_opens_a_plan_or_a_fork_and_hides_the_cursor() {
        let mut view = on(&["alpha", "ship"]);
        drawn(&mut view, 44, 10);
        assert_eq!(click(&mut view, 5, 0), Effect::None, "the Context row `alpha`");
        assert!(!view.state.opened().contains(&vec!["alpha".to_owned()]));
        drawn(&mut view, 44, 10);
        assert_eq!(view.state.selected(), ["alpha", "ship"], "a click selects nothing");
        assert!(!view.cursor_shown());
        click(&mut view, 5, 0);
        drawn(&mut view, 44, 10);
        assert_eq!(click(&mut view, 8, 1), Effect::Run(Action::Open(sid("l-ship"))), "the Plan row");
        assert_eq!(click(&mut view, 8, 3), Effect::Run(Action::Open(sid("f-blocked"))), "a fork row");
        assert_eq!(click(&mut view, 8, 9), Effect::None, "below the tree");
    }

    #[test]
    fn clicks_are_ignored_while_a_prompt_is_open() {
        let mut view = on(&["alpha", "ship"]);
        drawn(&mut view, 44, 10);
        press(&mut view, KeyCode::Char('f'));
        assert_eq!(click(&mut view, 8, 1), Effect::None);
    }

    #[test]
    fn a_refresh_keeps_the_cursor_on_its_row_and_the_folds() {
        let mut view = on(&["alpha", "sketch"]);
        view.state.close(&["alpha".to_owned(), "ship".to_owned()]);
        let mut tree = sample();
        tree.contexts[0].plans[1].sessions[0].state = crate::session::State::Working;
        view.refresh(tree);
        assert_eq!(view.state.selected(), ["alpha", "sketch"]);
        assert!(!view.state.opened().contains(&vec!["alpha".to_owned(), "ship".to_owned()]));
    }
}
