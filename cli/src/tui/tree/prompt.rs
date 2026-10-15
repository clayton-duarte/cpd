//! The one inline prompt of the tree (the reference's `prompt.rs`), shared by
//! both session actions: a text input for `n` (the name of the new Plan) and a
//! y/N confirmation for `x` (close a Session). Pure: the view feeds it keys and
//! acts on its [`Outcome`].
//!
//! The texts are the reference's, verbatim (commands.md §13.2). The one
//! difference: the name is the new Plan's id (§2.1), so a name that is no Plan
//! id is refused in the prompt, as an empty one is.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ids::{ContextId, PlanId, SessionId};

/// The Session an action is about, as the prompt words it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub id: SessionId,
    pub label: String,
    /// The Session's state is `working`.
    pub working: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// `n`: asks for the name of a new Plan in `context`, on `repo` (those of `target`'s Plan). `refused` is why the
    /// last Enter was not taken.
    Name { target: Target, context: ContextId, repo: String, input: String, refused: Option<String> },
    /// `x`: asks whether to close `target`'s pane.
    Close { target: Target },
}

/// What the view does after a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The prompt stays open.
    Open,
    /// The prompt is dismissed; nothing happens.
    Cancel,
    /// `plan new <id> --context <context> --repo <repo> --title <id>`.
    NewPlan { context: ContextId, id: PlanId, repo: String },
    /// `session close <id>`.
    Close { id: SessionId },
}

/// The refusal of an empty name (the reference's).
const NAME_REQUIRED: &str = "A name is required.";

impl Prompt {
    pub fn name(target: Target, context: ContextId, repo: String) -> Self {
        Prompt::Name { target, context, repo, input: String::new(), refused: None }
    }

    pub fn close(target: Target) -> Self {
        Prompt::Close { target }
    }

    pub fn key(&mut self, key: KeyEvent) -> Outcome {
        match self {
            Prompt::Name { context, repo, input, refused, .. } => match key.code {
                KeyCode::Esc => Outcome::Cancel,
                KeyCode::Enter => {
                    let name = input.trim();
                    if name.is_empty() {
                        *refused = Some(NAME_REQUIRED.into());
                        return Outcome::Open;
                    }
                    match PlanId::parse(name) {
                        Ok(id) => Outcome::NewPlan { context: context.clone(), id, repo: repo.clone() },
                        Err(e) => {
                            *refused = Some(e.message().to_owned());
                            Outcome::Open
                        }
                    }
                }
                KeyCode::Backspace => {
                    input.pop();
                    Outcome::Open
                }
                KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                    input.push(c);
                    Outcome::Open
                }
                _ => Outcome::Open,
            },
            Prompt::Close { target } => match key.code {
                KeyCode::Char('y' | 'Y') => Outcome::Close { id: target.id.clone() },
                _ => Outcome::Cancel,
            },
        }
    }

    /// The prompt's one line of text.
    pub fn text(&self) -> String {
        match self {
            Prompt::Name { target, input, refused, .. } => {
                let refusal = refused.as_ref().map(|r| format!("{r} ")).unwrap_or_default();
                format!("{refusal}Fork {} into a new Plan, name: {input}█ (Enter starts, Esc cancels)", target.label)
            }
            Prompt::Close { target } => {
                let working = if target.working { format!("{} is working. ", target.label) } else { String::new() };
                format!("{working}Close {}? pi stops; the session file stays (y/N)", target.label)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(working: bool) -> Target {
        Target { id: SessionId::parse("aaaa1111").unwrap(), label: "cpd-ui".into(), working }
    }

    fn name() -> Prompt {
        Prompt::name(target(false), ContextId::parse("cpd").unwrap(), "/repo".into())
    }

    fn press(p: &mut Prompt, code: KeyCode) -> Outcome {
        p.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn type_str(p: &mut Prompt, s: &str) {
        for c in s.chars() {
            assert_eq!(press(p, KeyCode::Char(c)), Outcome::Open);
        }
    }

    fn new_plan(id: &str) -> Outcome {
        Outcome::NewPlan { context: ContextId::parse("cpd").unwrap(), id: PlanId::parse(id).unwrap(), repo: "/repo".into() }
    }

    // The reference's tests (`main:tui/src/prompt.rs`), on Plan ids for names.

    #[test]
    fn enter_confirms_the_typed_name() {
        let mut p = name();
        type_str(&mut p, "  my-plan ");
        assert_eq!(press(&mut p, KeyCode::Enter), new_plan("my-plan"));
    }

    #[test]
    fn backspace_edits_the_name() {
        let mut p = name();
        type_str(&mut p, "abc");
        press(&mut p, KeyCode::Backspace);
        assert_eq!(press(&mut p, KeyCode::Enter), new_plan("ab"));
    }

    #[test]
    fn q_and_n_are_text_in_the_name() {
        let mut p = name();
        type_str(&mut p, "nq");
        assert_eq!(press(&mut p, KeyCode::Enter), new_plan("nq"));
    }

    #[test]
    fn esc_cancels_the_name() {
        let mut p = name();
        type_str(&mut p, "abc");
        assert_eq!(press(&mut p, KeyCode::Esc), Outcome::Cancel);
    }

    #[test]
    fn an_empty_name_is_refused_and_the_prompt_stays() {
        let mut p = name();
        assert_eq!(press(&mut p, KeyCode::Enter), Outcome::Open);
        assert!(p.text().starts_with("A name is required. "));
        type_str(&mut p, "   ");
        assert_eq!(press(&mut p, KeyCode::Enter), Outcome::Open, "blanks are no name");
        type_str(&mut p, "xy");
        assert_eq!(press(&mut p, KeyCode::Enter), new_plan("xy"));
    }

    #[test]
    fn a_name_that_is_no_plan_id_is_refused_and_the_prompt_keeps_it() {
        let mut p = name();
        type_str(&mut p, "my plan");
        assert_eq!(press(&mut p, KeyCode::Enter), Outcome::Open);
        let refusal = PlanId::parse("my plan").unwrap_err().message().to_owned();
        assert_eq!(p.text(), format!("{refusal} Fork cpd-ui into a new Plan, name: my plan█ (Enter starts, Esc cancels)"));
    }

    #[test]
    fn control_keys_are_not_typed() {
        let mut p = name();
        p.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(press(&mut p, KeyCode::Enter), Outcome::Open);
    }

    #[test]
    fn the_name_prompt_names_the_session_and_shows_the_input() {
        let mut p = name();
        type_str(&mut p, "new");
        assert_eq!(p.text(), "Fork cpd-ui into a new Plan, name: new█ (Enter starts, Esc cancels)");
    }

    #[test]
    fn the_close_prompt_text() {
        assert_eq!(Prompt::close(target(false)).text(), "Close cpd-ui? pi stops; the session file stays (y/N)");
    }

    #[test]
    fn the_close_prompt_warns_first_when_the_session_is_working() {
        assert_eq!(Prompt::close(target(true)).text(), "cpd-ui is working. Close cpd-ui? pi stops; the session file stays (y/N)");
    }

    #[test]
    fn only_y_closes() {
        for c in ['y', 'Y'] {
            assert_eq!(press(&mut Prompt::close(target(false)), KeyCode::Char(c)), Outcome::Close { id: target(false).id });
        }
        for code in [KeyCode::Char('n'), KeyCode::Enter, KeyCode::Esc, KeyCode::Char(' '), KeyCode::Down] {
            assert_eq!(press(&mut Prompt::close(target(false)), code), Outcome::Cancel);
        }
    }
}
