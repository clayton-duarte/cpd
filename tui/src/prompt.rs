//! The one inline prompt of the Context view, shared by both session actions: a text input for `n` (the name of the new
//! Plan) and a y/N confirmation for `x` (close a session). Pure: the view feeds it keys and acts on its `Outcome`.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// The session an action is about, as the prompt words it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// The session id: the view finds the pane again by it when the prompt is confirmed.
    pub id: String,
    pub label: String,
    /// The session's `@agent_state` is `working`.
    pub working: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// `n`: asks for the name of the new Plan that forks `target`. `refused` is set once an empty name was entered.
    Name { target: Target, input: String, refused: bool },
    /// `x`: asks whether to close `target`'s chat pane.
    Close { target: Target },
}

/// What the view does after a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The prompt stays open.
    Open,
    /// The prompt is dismissed; nothing happens.
    Cancel,
    /// Fork `id` into a new Plan called `name` (trimmed, never empty).
    Fork { id: String, name: String },
    /// Close the chat pane of session `id`.
    Close { id: String },
}

impl Prompt {
    pub fn fork(target: Target) -> Self {
        Prompt::Name { target, input: String::new(), refused: false }
    }

    pub fn close(target: Target) -> Self {
        Prompt::Close { target }
    }

    pub fn key(&mut self, key: KeyEvent) -> Outcome {
        match self {
            Prompt::Name { target, input, refused } => match key.code {
                KeyCode::Esc => Outcome::Cancel,
                KeyCode::Enter => {
                    let name = input.trim();
                    if name.is_empty() {
                        *refused = true;
                        Outcome::Open
                    } else {
                        Outcome::Fork { id: target.id.clone(), name: name.to_string() }
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
            Prompt::Name { target, input, refused } => {
                let refusal = if *refused { "A name is required. " } else { "" };
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
        Target { id: "aaaa1111".into(), label: "cpd-ui".into(), working }
    }

    fn press(p: &mut Prompt, code: KeyCode) -> Outcome {
        p.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn type_str(p: &mut Prompt, s: &str) {
        for c in s.chars() {
            assert_eq!(press(p, KeyCode::Char(c)), Outcome::Open);
        }
    }

    #[test]
    fn enter_confirms_the_typed_name() {
        let mut p = Prompt::fork(target(false));
        type_str(&mut p, "  my plan ");
        assert_eq!(press(&mut p, KeyCode::Enter), Outcome::Fork { id: "aaaa1111".into(), name: "my plan".into() });
    }

    #[test]
    fn backspace_edits_the_name() {
        let mut p = Prompt::fork(target(false));
        type_str(&mut p, "abc");
        press(&mut p, KeyCode::Backspace);
        assert_eq!(press(&mut p, KeyCode::Enter), Outcome::Fork { id: "aaaa1111".into(), name: "ab".into() });
    }

    #[test]
    fn q_and_n_are_text_in_the_name() {
        let mut p = Prompt::fork(target(false));
        type_str(&mut p, "nq");
        assert_eq!(press(&mut p, KeyCode::Enter), Outcome::Fork { id: "aaaa1111".into(), name: "nq".into() });
    }

    #[test]
    fn esc_cancels_the_name() {
        let mut p = Prompt::fork(target(false));
        type_str(&mut p, "abc");
        assert_eq!(press(&mut p, KeyCode::Esc), Outcome::Cancel);
    }

    #[test]
    fn an_empty_name_is_refused_and_the_prompt_stays() {
        let mut p = Prompt::fork(target(false));
        assert_eq!(press(&mut p, KeyCode::Enter), Outcome::Open);
        assert!(p.text().starts_with("A name is required. "));
        type_str(&mut p, "   ");
        assert_eq!(press(&mut p, KeyCode::Enter), Outcome::Open, "blanks are no name");
        type_str(&mut p, "x");
        assert_eq!(press(&mut p, KeyCode::Enter), Outcome::Fork { id: "aaaa1111".into(), name: "x".into() });
    }

    #[test]
    fn control_keys_are_not_typed() {
        let mut p = Prompt::fork(target(false));
        p.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(press(&mut p, KeyCode::Enter), Outcome::Open);
    }

    #[test]
    fn the_name_prompt_names_the_session_and_shows_the_input() {
        let mut p = Prompt::fork(target(false));
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
            assert_eq!(press(&mut Prompt::close(target(false)), KeyCode::Char(c)), Outcome::Close { id: "aaaa1111".into() });
        }
        for code in [KeyCode::Char('n'), KeyCode::Enter, KeyCode::Esc, KeyCode::Char(' '), KeyCode::Down] {
            assert_eq!(press(&mut Prompt::close(target(false)), code), Outcome::Cancel);
        }
    }
}
