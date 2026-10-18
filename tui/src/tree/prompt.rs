//! The one inline prompt of the tree (the reference's `prompt.rs`), shared by
//! the session actions: a text input for `n` (the name of the new Plan), a text input for the menu's `…` items
//! (rename, fork, new Plan, new Context) and a y/N confirmation for `x` (close a Session; the menu's Archive has
//! none, hs-d90). Pure: the view feeds it keys and acts on its [`Outcome`].
//!
//! The texts are the reference's, verbatim (commands.md §13.2). The one
//! difference: the name is the new Plan's id (§2.1), so a name that is no Plan
//! id is refused in the prompt, as an empty one is.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::actions::Action;
use super::slug::{self, CONTEXT_MAX, PLAN_MAX};
use cpd_core::ids::{ContextId, PlanId, SessionId};

/// The Session an action is about, as the prompt words it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub id: SessionId,
    pub label: String,
    /// The Session's state is `working`.
    pub working: bool,
}

/// What a text prompt of the menu asks for, with what its command needs besides the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    RenameContext(ContextId),
    RenamePlan(PlanId),
    RenameSession(SessionId),
    /// `Fork…` and `New session…`: the name of a Session forked from `source`.
    Fork { source: SessionId },
    /// The title of a new Context; its id is derived against the ids `taken`.
    NewContext { taken: Vec<String> },
    /// The title of a new Plan in `context` (R34 A12: no repo; the Plan's PRs name theirs).
    NewPlan { context: ContextId, taken: Vec<String> },
}

impl Ask {
    fn label(&self) -> &'static str {
        match self {
            Ask::RenameContext(_) | Ask::RenamePlan(_) | Ask::NewContext { .. } | Ask::NewPlan { .. } => "Title: ",
            Ask::RenameSession(_) | Ask::Fork { .. } => "Name: ",
        }
    }

    fn required(&self) -> &'static str {
        match self {
            Ask::RenameContext(_) | Ask::RenamePlan(_) | Ask::NewContext { .. } | Ask::NewPlan { .. } => "A title is required.",
            Ask::RenameSession(_) | Ask::Fork { .. } => NAME_REQUIRED,
        }
    }
}

/// The refusal of a title no id can be derived from.
const NO_ID: &str = "A title needs at least two letters or digits, the first a letter, to make an id.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// The menu's `…` items: `ask`, typed into `input` (prefilled for a rename). `refused` is why the last Enter was
    /// not taken.
    Text { ask: Ask, input: String, refused: Option<String> },
    /// `n`: asks for the name of a new Plan in `context` (that of `target`'s Plan). `refused` is why the
    /// last Enter was not taken.
    Name { target: Target, context: ContextId, input: String, refused: Option<String> },
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
    /// `plan new <id> --context <context> --title <id>`.
    NewPlan { context: ContextId, id: PlanId },
    /// `session close <id>`.
    Close { id: SessionId },
    /// The command a text prompt of the menu asked for.
    Run(Action),
}

/// The refusal of an empty name (the reference's).
const NAME_REQUIRED: &str = "A name is required.";

impl Prompt {
    pub fn name(target: Target, context: ContextId) -> Self {
        Prompt::Name { target, context, input: String::new(), refused: None }
    }

    /// A text prompt of the menu, starting with `input`.
    pub fn typed(ask: Ask, input: String) -> Self {
        Prompt::Text { ask, input, refused: None }
    }

    pub fn close(target: Target) -> Self {
        Prompt::Close { target }
    }

    pub fn key(&mut self, key: KeyEvent) -> Outcome {
        match self {
            Prompt::Name { context, input, refused, .. } => match key.code {
                KeyCode::Esc => Outcome::Cancel,
                KeyCode::Enter => {
                    let name = input.trim();
                    if name.is_empty() {
                        *refused = Some(NAME_REQUIRED.into());
                        return Outcome::Open;
                    }
                    match PlanId::parse(name) {
                        Ok(id) => Outcome::NewPlan { context: context.clone(), id },
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
            Prompt::Text { ask, input, refused } => match key.code {
                KeyCode::Esc => Outcome::Cancel,
                KeyCode::Enter => {
                    let text = input.trim().to_owned();
                    if text.is_empty() {
                        *refused = Some(ask.required().into());
                        return Outcome::Open;
                    }
                    match answer(ask, text) {
                        Ok(action) => Outcome::Run(action),
                        Err(why) => {
                            *refused = Some(why);
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
            Prompt::Text { ask, input, refused } => {
                let refusal = refused.as_ref().map(|r| format!("{r} ")).unwrap_or_default();
                format!("{refusal}{}{input}█ (Enter confirms, Esc cancels)", ask.label())
            }
            Prompt::Close { target } => {
                let working = if target.working { format!("{} is working. ", target.label) } else { String::new() };
                format!("{working}Close {}? pi stops; the session file stays (y/N)", target.label)
            }
        }
    }
}

/// The command for the `text` typed to `ask`; or why the text is refused.
fn answer(ask: &Ask, text: String) -> Result<Action, String> {
    let refuse = |e: cpd_core::error::CpdError| e.message().to_owned();
    Ok(match ask {
        Ask::RenameContext(id) => Action::RenameContext { id: id.clone(), title: text },
        Ask::RenamePlan(id) => Action::RenamePlan { id: id.clone(), title: text },
        Ask::RenameSession(id) => Action::RenameSession { id: id.clone(), name: text },
        Ask::Fork { source } => Action::Fork { source: source.clone(), name: text },
        Ask::NewContext { taken } => {
            let id = slug::derive(&text, CONTEXT_MAX, |id| taken.iter().any(|t| t == id)).ok_or(NO_ID)?;
            Action::NewContext { id: ContextId::parse(&id).map_err(refuse)?, title: text }
        }
        Ask::NewPlan { context, taken } => {
            let id = slug::derive(&text, PLAN_MAX, |id| taken.iter().any(|t| t == id)).ok_or(NO_ID)?;
            Action::NewPlan { context: context.clone(), id: PlanId::parse(&id).map_err(refuse)?, title: text }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(working: bool) -> Target {
        Target { id: SessionId::parse("aaaa1111").unwrap(), label: "cpd-ui".into(), working }
    }

    fn name() -> Prompt {
        Prompt::name(target(false), ContextId::parse("cpd").unwrap())
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
        Outcome::NewPlan { context: ContextId::parse("cpd").unwrap(), id: PlanId::parse(id).unwrap() }
    }

    fn cpd() -> ContextId {
        ContextId::parse("cpd").unwrap()
    }

    fn typing(ask: Ask, input: &str) -> Prompt {
        Prompt::typed(ask, input.into())
    }

    fn taken(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|i| (*i).to_owned()).collect()
    }

    #[test]
    fn a_text_prompt_runs_the_command_of_its_ask_with_the_trimmed_text() {
        let sid = || SessionId::parse("aaaa1111").unwrap();
        let cases = [
            (Ask::RenameContext(cpd()), Action::RenameContext { id: cpd(), title: "New title".into() }),
            (Ask::RenamePlan(PlanId::parse("ship").unwrap()), Action::RenamePlan { id: PlanId::parse("ship").unwrap(), title: "New title".into() }),
            (Ask::RenameSession(sid()), Action::RenameSession { id: sid(), name: "New title".into() }),
            (Ask::Fork { source: sid() }, Action::Fork { source: sid(), name: "New title".into() }),
        ];
        for (ask, action) in cases {
            let mut p = typing(ask.clone(), "  New title ");
            assert_eq!(press(&mut p, KeyCode::Enter), Outcome::Run(action), "{ask:?}");
        }
    }

    #[test]
    fn new_context_derives_its_id_from_the_title_against_the_taken_ones() {
        let ask = |ids: &[&str]| Ask::NewContext { taken: taken(ids) };
        let new = |id: &str| Outcome::Run(Action::NewContext { id: ContextId::parse(id).unwrap(), title: "Building cpd!".into() });
        assert_eq!(press(&mut typing(ask(&[]), "Building cpd!"), KeyCode::Enter), new("building-cpd"));
        assert_eq!(press(&mut typing(ask(&["building-cpd"]), "Building cpd!"), KeyCode::Enter), new("building-cpd-2"));
        assert_eq!(press(&mut typing(ask(&["building-cpd", "building-cpd-2"]), "Building cpd!"), KeyCode::Enter), new("building-cpd-3"));
    }

    #[test]
    fn new_plan_derives_its_id_from_the_title_and_asks_for_no_repo() {
        let plan = Ask::NewPlan { context: cpd(), taken: taken(&["pay"]) };
        let run = Outcome::Run(Action::NewPlan { context: cpd(), id: PlanId::parse("pay-2").unwrap(), title: "Pay".into() });
        assert_eq!(press(&mut typing(plan, "Pay"), KeyCode::Enter), run);
    }

    #[test]
    fn a_title_with_no_id_in_it_is_refused_and_kept() {
        let mut p = typing(Ask::NewContext { taken: vec![] }, "123");
        assert_eq!(press(&mut p, KeyCode::Enter), Outcome::Open);
        assert_eq!(p.text(), format!("{NO_ID} Title: 123█ (Enter confirms, Esc cancels)"));
        let mut p = typing(Ask::NewPlan { context: cpd(), taken: vec![] }, "!");
        assert_eq!(press(&mut p, KeyCode::Enter), Outcome::Open);
        assert!(p.text().starts_with(NO_ID));
    }

    #[test]
    fn a_text_prompt_is_typed_into_edited_refused_when_empty_and_cancelled_by_esc() {
        let mut p = typing(Ask::RenameContext(cpd()), "ab");
        type_str(&mut p, "cd");
        press(&mut p, KeyCode::Backspace);
        p.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(p.text(), "Title: abc█ (Enter confirms, Esc cancels)");
        assert_eq!(press(&mut p, KeyCode::Esc), Outcome::Cancel);
        let mut p = typing(Ask::Fork { source: SessionId::parse("aaaa1111").unwrap() }, "");
        assert_eq!(press(&mut p, KeyCode::Enter), Outcome::Open);
        assert_eq!(p.text(), "A name is required. Name: █ (Enter confirms, Esc cancels)");
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
