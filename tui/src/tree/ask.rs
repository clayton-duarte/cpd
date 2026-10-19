//! What the tree asks in a dialog popup (the dialog kit, `crate::dialog`; hs-d295, hs-d297) before it runs a command:
//! an `input` for `n` (the new Plan's id) and for the menu's `…` items (rename, fork, new Plan, new Context), and a
//! `confirm` for `x` and the menu's Close and Archive. The tree has no inline prompt row. Pure but for the dialogs
//! themselves, which a [`Dialogs`] opens: the view returns a [`Question`], the caller [asks](Question::ask) it and runs
//! the [`Action`] the answer makes, if any.
//!
//! The questions and the texts are the former inline prompt's, verbatim (commands.md §13.2). The name of `n` is the
//! new Plan's id (§2.1), so the popup refuses one that is no Plan id; a title no id can be derived from is refused
//! after the popup, as the note.

use super::actions::Action;
use super::slug::{self, CONTEXT_MAX, PLAN_MAX};
use crate::dialog::validate::Validator;
use cpd_core::error::CpdError;
use cpd_core::ids::{ContextId, PlanId, SessionId};

/// What an `input` popup of the tree asks for, with what its command needs besides the text.
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
    /// `n`: the id of a new Plan in `context`, forked from the Session `label`.
    NamedPlan { context: ContextId, label: String },
}

/// The refusal of a title no id can be derived from.
const NO_ID: &str = "A title needs at least two letters or digits, the first a letter, to make an id.";

impl Ask {
    /// The question above the field: the former prompt's label.
    fn prompt(&self) -> String {
        match self {
            Ask::RenameContext(_) | Ask::RenamePlan(_) | Ask::NewContext { .. } | Ask::NewPlan { .. } => "Title:".into(),
            Ask::RenameSession(_) | Ask::Fork { .. } => "Name:".into(),
            Ask::NamedPlan { label, .. } => format!("Fork {label} into a new Plan, name:"),
        }
    }

    fn validator(&self) -> Validator {
        match self {
            Ask::RenameContext(_) | Ask::RenamePlan(_) | Ask::NewContext { .. } | Ask::NewPlan { .. } => Validator::Title,
            Ask::RenameSession(_) | Ask::Fork { .. } => Validator::Name,
            Ask::NamedPlan { .. } => Validator::PlanId,
        }
    }

    /// The command for the `text` the popup accepted; or why it is refused after all.
    fn answer(&self, text: String) -> Result<Action, String> {
        let refuse = |e: CpdError| e.message().to_owned();
        Ok(match self {
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
            Ask::NamedPlan { context, .. } => {
                let id = PlanId::parse(&text).map_err(refuse)?;
                let title = id.to_string();
                Action::NewPlan { context: context.clone(), id, title }
            }
        })
    }
}

/// A popup the tree opens before it runs a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Question {
    /// An `input` popup titled `title`, starting with `initial`; its text goes to `ask`'s command.
    Input { title: &'static str, ask: Ask, initial: String },
    /// A `confirm` popup, `[<yes_label>] [Cancel]`; `then` runs when confirmed.
    Confirm { title: &'static str, text: String, yes_label: &'static str, then: Action },
}

/// The popups of the dialog kit, as the tree opens them: `crate::dialog::{input, confirm}` for the cockpit, a stub in a
/// test.
pub trait Dialogs {
    /// The accepted text of an `input` popup; `None` when cancelled.
    fn input(&self, title: &str, prompt: &str, initial: &str, validator: Validator) -> Result<Option<String>, CpdError>;
    /// Whether a `confirm` popup was confirmed.
    fn confirm(&self, title: &str, text: &str, yes_label: &str) -> Result<bool, CpdError>;
}

impl Question {
    /// `Close <label>? pi stops; the session file stays`, preceded by `<label> is working.` (the former prompt's words).
    pub fn close(id: SessionId, label: &str, working: bool) -> Question {
        let warning = if working { format!("{label} is working. ") } else { String::new() };
        Question::Confirm {
            title: "Close",
            text: format!("{warning}Close {label}? pi stops; the session file stays"),
            yes_label: "Close",
            then: Action::Close(id),
        }
    }

    /// `Archive <label>?` (hs-d297); `then` is the archive, with the step that frees the chat slot when it needs one.
    pub fn archive(label: &str, then: Action) -> Question {
        Question::Confirm { title: "Archive", text: format!("Archive {label}?"), yes_label: "Archive", then }
    }

    pub fn input(title: &'static str, ask: Ask, initial: String) -> Question {
        Question::Input { title, ask, initial }
    }

    /// Opens the popup: the command to run, or `None` when the User cancelled; an error says why the popup failed or
    /// the answer is refused.
    pub fn ask(self, dialogs: &impl Dialogs) -> Result<Option<Action>, String> {
        let failed = |e: CpdError| e.message().to_owned();
        match self {
            Question::Input { title, ask, initial } => match dialogs.input(title, &ask.prompt(), &initial, ask.validator()).map_err(failed)? {
                Some(text) => ask.answer(text).map(Some),
                None => Ok(None),
            },
            Question::Confirm { title, text, yes_label, then } => Ok(dialogs.confirm(title, &text, yes_label).map_err(failed)?.then_some(then)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// A stub of the popups: what was opened, and the answer it gives.
    struct Stub {
        opened: RefCell<Vec<String>>,
        text: Option<&'static str>,
        yes: bool,
    }

    impl Stub {
        fn typing(text: &'static str) -> Stub {
            Stub { opened: RefCell::default(), text: Some(text), yes: false }
        }

        fn cancelling() -> Stub {
            Stub { opened: RefCell::default(), text: None, yes: false }
        }

        fn confirming(yes: bool) -> Stub {
            Stub { opened: RefCell::default(), text: None, yes }
        }
    }

    impl Dialogs for Stub {
        fn input(&self, title: &str, prompt: &str, initial: &str, validator: Validator) -> Result<Option<String>, CpdError> {
            self.opened.borrow_mut().push(format!("input {title} | {prompt} | {initial} | {}", validator.as_arg()));
            Ok(self.text.map(str::to_owned))
        }

        fn confirm(&self, title: &str, text: &str, yes_label: &str) -> Result<bool, CpdError> {
            self.opened.borrow_mut().push(format!("confirm {title} | {text} | {yes_label}"));
            Ok(self.yes)
        }
    }

    fn sid() -> SessionId {
        SessionId::parse("aaaa1111").unwrap()
    }

    fn cpd() -> ContextId {
        ContextId::parse("cpd").unwrap()
    }

    fn taken(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|i| (*i).to_owned()).collect()
    }

    fn input(ask: Ask, initial: &str) -> Question {
        Question::input("T", ask, initial.into())
    }

    #[test]
    fn an_input_runs_the_command_of_its_ask_with_the_text_the_popup_accepted() {
        let cases = [
            (Ask::RenameContext(cpd()), Action::RenameContext { id: cpd(), title: "New title".into() }),
            (Ask::RenamePlan(PlanId::parse("ship").unwrap()), Action::RenamePlan { id: PlanId::parse("ship").unwrap(), title: "New title".into() }),
            (Ask::RenameSession(sid()), Action::RenameSession { id: sid(), name: "New title".into() }),
            (Ask::Fork { source: sid() }, Action::Fork { source: sid(), name: "New title".into() }),
        ];
        for (ask, action) in cases {
            let stub = Stub::typing("New title");
            assert_eq!(input(ask.clone(), "Old").ask(&stub), Ok(Some(action)), "{ask:?}");
            assert_eq!(stub.opened.borrow().len(), 1);
        }
    }

    #[test]
    fn each_input_asks_the_former_prompts_question_from_its_starting_value_and_checks_with_its_validator() {
        let label = |ask: Ask, title: &'static str, initial: &str| {
            let stub = Stub::cancelling();
            assert_eq!(Question::input(title, ask, initial.into()).ask(&stub), Ok(None));
            stub.opened.into_inner()
        };
        assert_eq!(label(Ask::RenameContext(cpd()), "Rename", "Alpha"), ["input Rename | Title: | Alpha | title"]);
        assert_eq!(label(Ask::RenamePlan(PlanId::parse("ship").unwrap()), "Rename", "Ship"), ["input Rename | Title: | Ship | title"]);
        assert_eq!(label(Ask::NewContext { taken: vec![] }, "New Context", ""), ["input New Context | Title: |  | title"]);
        assert_eq!(label(Ask::NewPlan { context: cpd(), taken: vec![] }, "New Plan", ""), ["input New Plan | Title: |  | title"]);
        assert_eq!(label(Ask::RenameSession(sid()), "Rename", "ship"), ["input Rename | Name: | ship | name"]);
        assert_eq!(label(Ask::Fork { source: sid() }, "Fork", "ship (fork)"), ["input Fork | Name: | ship (fork) | name"]);
        let named = Ask::NamedPlan { context: cpd(), label: "Plan sketch".into() };
        assert_eq!(label(named, "New Plan", ""), ["input New Plan | Fork Plan sketch into a new Plan, name: |  | plan-id"]);
    }

    #[test]
    fn a_cancelled_input_runs_nothing() {
        for ask in [Ask::RenameContext(cpd()), Ask::Fork { source: sid() }, Ask::NewContext { taken: vec![] }, Ask::NamedPlan { context: cpd(), label: "x".into() }] {
            assert_eq!(input(ask, "x").ask(&Stub::cancelling()), Ok(None));
        }
    }

    #[test]
    fn new_context_derives_its_id_from_the_title_against_the_taken_ones() {
        let run = |ids: &[&str]| input(Ask::NewContext { taken: taken(ids) }, "").ask(&Stub::typing("Building cpd!"));
        let new = |id: &str| Ok(Some(Action::NewContext { id: ContextId::parse(id).unwrap(), title: "Building cpd!".into() }));
        assert_eq!(run(&[]), new("building-cpd"));
        assert_eq!(run(&["building-cpd"]), new("building-cpd-2"));
        assert_eq!(run(&["building-cpd", "building-cpd-2"]), new("building-cpd-3"));
    }

    #[test]
    fn new_plan_derives_its_id_from_the_title_and_asks_for_no_repo() {
        let plan = Ask::NewPlan { context: cpd(), taken: taken(&["pay"]) };
        let run = Action::NewPlan { context: cpd(), id: PlanId::parse("pay-2").unwrap(), title: "Pay".into() };
        assert_eq!(input(plan, "").ask(&Stub::typing("Pay")), Ok(Some(run)));
    }

    #[test]
    fn a_title_with_no_id_in_it_is_refused_with_its_reason() {
        for title in ["123", "!"] {
            assert_eq!(input(Ask::NewContext { taken: vec![] }, "").ask(&Stub::typing(title)), Err(NO_ID.to_owned()), "{title}");
            assert_eq!(input(Ask::NewPlan { context: cpd(), taken: vec![] }, "").ask(&Stub::typing(title)), Err(NO_ID.to_owned()), "{title}");
        }
    }

    #[test]
    fn n_starts_a_plan_named_by_its_id_which_is_also_its_title() {
        let ask = Ask::NamedPlan { context: cpd(), label: "cpd-ui".into() };
        let run = Action::NewPlan { context: cpd(), id: PlanId::parse("my-plan").unwrap(), title: "my-plan".into() };
        assert_eq!(input(ask.clone(), "").ask(&Stub::typing("my-plan")), Ok(Some(run)));
        let refusal = PlanId::parse("my plan").unwrap_err().message().to_owned();
        assert_eq!(input(ask, "").ask(&Stub::typing("my plan")), Err(refusal), "the popup refuses it first; this is the second check");
    }

    #[test]
    fn a_confirm_runs_its_action_only_when_confirmed() {
        let close = || Question::close(sid(), "cpd-ui", false);
        assert_eq!(close().ask(&Stub::confirming(true)), Ok(Some(Action::Close(sid()))));
        assert_eq!(close().ask(&Stub::confirming(false)), Ok(None));
        let archive = || Question::archive("Plan ship", Action::Archive { subject: super::super::actions::Subject::Session(sid()), free: None });
        assert!(matches!(archive().ask(&Stub::confirming(true)), Ok(Some(Action::Archive { .. }))));
        assert_eq!(archive().ask(&Stub::confirming(false)), Ok(None));
    }

    #[test]
    fn the_confirmations_are_the_former_prompts_texts_with_their_buttons() {
        let text = |q: Question| {
            let stub = Stub::confirming(false);
            assert_eq!(q.ask(&stub), Ok(None));
            stub.opened.into_inner()
        };
        assert_eq!(text(Question::close(sid(), "cpd-ui", false)), ["confirm Close | Close cpd-ui? pi stops; the session file stays | Close"]);
        assert_eq!(
            text(Question::close(sid(), "cpd-ui", true)),
            ["confirm Close | cpd-ui is working. Close cpd-ui? pi stops; the session file stays | Close"]
        );
        assert_eq!(text(Question::archive("Plan ship", Action::Park)), ["confirm Archive | Archive Plan ship? | Archive"]);
    }

    #[test]
    fn a_failed_popup_is_an_error_never_a_cancel_or_a_confirm() {
        struct Failing;
        impl Dialogs for Failing {
            fn input(&self, _: &str, _: &str, _: &str, _: Validator) -> Result<Option<String>, CpdError> {
                Err(CpdError::new(cpd_core::error::ErrorCode::TmuxFailed, "No tmux client is attached to the cockpit."))
            }
            fn confirm(&self, _: &str, _: &str, _: &str) -> Result<bool, CpdError> {
                Err(CpdError::new(cpd_core::error::ErrorCode::TmuxFailed, "No tmux client is attached to the cockpit."))
            }
        }
        let why = "No tmux client is attached to the cockpit.".to_owned();
        assert_eq!(input(Ask::Fork { source: sid() }, "").ask(&Failing), Err(why.clone()));
        assert_eq!(Question::close(sid(), "x", false).ask(&Failing), Err(why));
    }
}
