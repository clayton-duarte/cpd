//! The one inline prompt of the tree (the spike's `prompt.rs`), shared by its
//! text-entry actions: the id, title and repo of a new Plan (`n`), the name of
//! a fork (`f`), a Session's new name (`r`). Pure: the view feeds it keys and
//! acts on its [`Outcome`].

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::args::Name;
use crate::ids::{ContextId, PlanId, SessionId};

/// The Session an action is about, as the prompt words it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub id: SessionId,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Id,
    Title,
    Repo,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// `n`: asks for the id, the title and (when the Context has no Plan to take it from) the repo of a new
    /// Plan in `context`. `refused` is why the last Enter was not taken.
    NewPlan { context: ContextId, repo: Option<String>, step: Step, id: Option<PlanId>, title: String, input: String, refused: Option<String> },
    /// `f`: asks for the name of the fork of `source`; empty keeps the command's default.
    Fork { source: Target, input: String, refused: Option<String> },
    /// `r`: asks for the new name of `target`, starting from the current one.
    Rename { target: Target, input: String, refused: Option<String> },
}

/// What the view does after a key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The prompt stays open.
    Open,
    /// The prompt is dismissed; nothing happens.
    Cancel,
    /// `plan new <id> --context <context> --repo <repo> --title <title>`.
    NewPlan { context: ContextId, id: PlanId, title: String, repo: String },
    /// `session fork <source> [--name <name>]`.
    Fork { source: SessionId, name: Option<Name> },
    /// `session rename <name> --session <session>`.
    Rename { session: SessionId, name: Name },
}

/// A key typed into `input`: true when it was text or Backspace.
fn edit(input: &mut String, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Backspace => {
            input.pop();
            true
        }
        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            input.push(c);
            true
        }
        _ => false,
    }
}

impl Prompt {
    /// `repo`: the repo of the Context's Plans (the newest), when it has one.
    pub fn new_plan(context: ContextId, repo: Option<String>) -> Self {
        Prompt::NewPlan { context, repo, step: Step::Id, id: None, title: String::new(), input: String::new(), refused: None }
    }

    pub fn fork(source: Target) -> Self {
        Prompt::Fork { source, input: String::new(), refused: None }
    }

    pub fn rename(target: Target, current: &str) -> Self {
        Prompt::Rename { target, input: current.to_owned(), refused: None }
    }

    pub fn key(&mut self, key: KeyEvent) -> Outcome {
        if key.code == KeyCode::Esc {
            return Outcome::Cancel;
        }
        match self {
            Prompt::NewPlan { context, repo, step, id, title, input, refused } => {
                if edit(input, key) || key.code != KeyCode::Enter {
                    return Outcome::Open;
                }
                let text = input.trim().to_owned();
                if text.is_empty() {
                    *refused = Some("A value is required.".into());
                    return Outcome::Open;
                }
                *refused = None;
                match step {
                    Step::Id => match PlanId::parse(&text) {
                        Ok(plan) => {
                            *id = Some(plan);
                            *step = Step::Title;
                            input.clear();
                        }
                        Err(e) => *refused = Some(e.message().to_owned()),
                    },
                    Step::Title => {
                        *title = text;
                        input.clear();
                        if let Some(repo) = repo.clone() {
                            return Outcome::NewPlan { context: context.clone(), id: id.clone().expect("the id step ran"), title: title.clone(), repo };
                        }
                        *step = Step::Repo;
                    }
                    Step::Repo => {
                        return Outcome::NewPlan { context: context.clone(), id: id.clone().expect("the id step ran"), title: title.clone(), repo: text };
                    }
                }
                Outcome::Open
            }
            Prompt::Fork { source, input, refused } => {
                if edit(input, key) || key.code != KeyCode::Enter {
                    return Outcome::Open;
                }
                match input.trim() {
                    "" => Outcome::Fork { source: source.id.clone(), name: None },
                    name => match name.parse::<Name>() {
                        Ok(name) => Outcome::Fork { source: source.id.clone(), name: Some(name) },
                        Err(e) => {
                            *refused = Some(e.message().to_owned());
                            Outcome::Open
                        }
                    },
                }
            }
            Prompt::Rename { target, input, refused } => {
                if edit(input, key) || key.code != KeyCode::Enter {
                    return Outcome::Open;
                }
                match input.trim().parse::<Name>() {
                    Ok(name) => Outcome::Rename { session: target.id.clone(), name },
                    Err(e) => {
                        *refused = Some(e.message().to_owned());
                        Outcome::Open
                    }
                }
            }
        }
    }

    /// The prompt's one line of text.
    pub fn text(&self) -> String {
        let refusal = |r: &Option<String>| r.as_ref().map(|r| format!("{r} ")).unwrap_or_default();
        match self {
            Prompt::NewPlan { context, step, input, refused, .. } => {
                let (what, enter) = match step {
                    Step::Id => ("id", "Enter: next"),
                    Step::Title => ("title", "Enter: next"),
                    Step::Repo => ("repo", "Enter starts"),
                };
                let enter = if *step == Step::Title && !self.asks_repo() { "Enter starts" } else { enter };
                format!("{}New Plan in {context}, {what}: {input}█ ({enter}, Esc cancels)", refusal(refused))
            }
            Prompt::Fork { source, input, refused } => {
                format!("{}Fork {}, name: {input}█ (empty: default; Enter forks, Esc cancels)", refusal(refused), source.label)
            }
            Prompt::Rename { target, input, refused } => {
                format!("{}Rename {} to: {input}█ (Enter renames, Esc cancels)", refusal(refused), target.label)
            }
        }
    }

    /// Whether a new Plan's last step is the repo.
    fn asks_repo(&self) -> bool {
        matches!(self, Prompt::NewPlan { repo: None, .. })
    }
}
