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
