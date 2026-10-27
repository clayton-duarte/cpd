//! What the tree asks in a dialog popup (the dialog kit, `crate::dialog`; hs-d295, hs-d297) before it runs a command:
//! an `input` for `n` (the new Plan's id) and for the menu's `…` items (rename, fork, new Plan, new Context), and a
//! `confirm` for `x` and the menu's Close and Archive. The tree has no inline prompt row. Pure but for the dialogs
//! themselves, which a [`Dialogs`] opens: the view returns a [`Question`], the caller [asks](Question::ask) it and runs
//! the [`Action`] the answer makes, if any.
//!
//! The questions and the texts are the former inline prompt's, verbatim (commands.md §13.2). The name of `n` is the
//! new Plan's id (§2.1), so the popup refuses one that is no Plan id; a title no id can be derived from is refused
//! after the popup, in a `text` dialog (commands.md §17 "No notes").

use super::actions::{Action, Deletable};
use serde_json::Value;
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
    /// The title of a new Context; its id is derived against the ids `taken`. `owners` are the owner policies of the
    /// instance (at least one): the owner is always asked next, in a `select` popup of them.
    NewContext { taken: Vec<String>, owners: Vec<String> },
    /// The title of a new Plan in `context` (R34 A12: no repo; the Plan's PRs name theirs).
    NewPlan { context: ContextId, taken: Vec<String> },
    /// `n`: the id of a new Plan in `context`, forked from the Session `label`.
    NamedPlan { context: ContextId, label: String },
}

/// The last row of the owner select: the owner is typed in an input, not among the known ones.
pub(crate) const ANOTHER_OWNER: &str = "Another owner…";

/// The label of the confirm popup's yes button for an unknown owner.
pub(crate) const USE_FALLBACK: &str = "Use clayton-duarte's policy";

/// The check of the typed owner login: the login lowercased, or why it is refused (a GitHub login, 1-39 characters).
pub(crate) fn check_login(text: &str) -> Result<String, String> {
    let ok = !text.is_empty()
        && text.chars().count() <= 39
        && !text.starts_with('-')
        && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if ok { Ok(text.to_ascii_lowercase()) } else { Err(format!("{text:?} is not a GitHub login.")) }
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

    /// The command for the `text` the popup accepted, or a further question; or why it is refused after all.
    fn answer(&self, text: String) -> Result<Step, String> {
        let refuse = |e: CpdError| e.message().to_owned();
        Ok(Step::Run(match self {
            Ask::RenameContext(id) => Action::RenameContext { id: id.clone(), title: text },
            Ask::RenamePlan(id) => Action::RenamePlan { id: id.clone(), title: text },
            Ask::RenameSession(id) => Action::RenameSession { id: id.clone(), name: text },
            Ask::Fork { source } => Action::Fork { source: source.clone(), name: text },
            Ask::NewContext { taken, owners } => {
                let id = slug::derive(&text, CONTEXT_MAX, |id| taken.iter().any(|t| t == id)).ok_or(NO_ID)?;
                let id = ContextId::parse(&id).map_err(refuse)?;
                return Ok(Step::Owner { id, title: text, owners: owners.clone() });
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
        }))
    }
}

/// What the text accepted for an [`Ask`] leads to.
enum Step {
    Run(Action),
    /// The owner of the new Context `id` titled `title`, chosen in a `select` popup of `owners`.
    Owner { id: ContextId, title: String, owners: Vec<String> },
}

/// A popup the tree opens before it runs a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Question {
    /// An `input` popup titled `title`, starting with `initial`; its text goes to `ask`'s command.
    Input { title: &'static str, ask: Ask, initial: String },
    /// A `confirm` popup, `[<yes_label>] [Cancel]`; `then` runs when confirmed.
    Confirm { title: &'static str, text: String, yes_label: &'static str, then: Action },
    /// A `confirm` popup `[Delete] [Cancel]` listing what the delete of `what` (`label` is the row's) removes: the list
    /// is the command's own preview, so it is asked first ([`Dialogs::preview`]); `plan delete <id> --yes` runs when
    /// confirmed.
    Delete { what: Deletable, label: String },
}

/// The popups of the dialog kit, as the tree opens them: `crate::dialog::{input, confirm}` for the cockpit, a stub in a
/// test.
pub trait Dialogs {
    /// The accepted text of an `input` popup; `None` when cancelled.
    fn input(&self, title: &str, prompt: &str, initial: &str, validator: Validator) -> Result<Option<String>, CpdError>;
    /// Whether a `confirm` popup was confirmed.
    fn confirm(&self, title: &str, text: &str, yes_label: &str) -> Result<bool, CpdError>;
    /// The option chosen in a `select` popup; `None` when cancelled.
    fn select(&self, title: &str, prompt: &str, options: &[String]) -> Result<Option<String>, CpdError>;
    /// The `(title, option)` of a `form` popup: a text box labelled `input_label` and a radio list of `options` labelled
    /// `select_label`; `None` when cancelled.
    #[allow(dead_code)] // the tree's New Context… (Job dlg-impl)
    fn form(&self, _title: &str, _input_label: &str, _select_label: &str, _options: &[String]) -> Result<Option<(String, String)>, CpdError> {
        unimplemented!("the form popup (skeleton of Job dlg-tests)")
    }
    /// What the delete of `what` would remove: the reply of the command without `--yes`, or why it is refused.
    fn preview(&self, what: &Deletable) -> Result<Value, String>;
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

    /// `Delete <label>?`, with the counts of the preview.
    pub fn delete(what: Deletable, label: &str) -> Question {
        Question::Delete { what, label: label.to_owned() }
    }

    pub fn input(title: &'static str, ask: Ask, initial: String) -> Question {
        Question::Input { title, ask, initial }
    }

    /// The title of the popup: what the dialog is about, and what a failure after it is said to be about.
    pub fn title(&self) -> &'static str {
        match self {
            Question::Input { title, .. } | Question::Confirm { title, .. } => title,
            Question::Delete { .. } => "Delete",
        }
    }

    /// Opens the popup: the command to run, or `None` when the User cancelled; an error says why the popup failed or
    /// the answer is refused.
    pub fn ask(self, dialogs: &impl Dialogs) -> Result<Option<Action>, String> {
        let failed = |e: CpdError| e.message().to_owned();
        match self {
            Question::Input { title, ask, initial } => {
                let Some(text) = dialogs.input(title, &ask.prompt(), &initial, ask.validator()).map_err(failed)? else { return Ok(None) };
                match ask.answer(text)? {
                    Step::Run(action) => Ok(Some(action)),
                    Step::Owner { id, title: name, owners } => {
                        let mut options = owners.clone();
                        options.push(ANOTHER_OWNER.to_owned());
                        let Some(owner) = dialogs.select(title, "Owner:", &options).map_err(failed)? else { return Ok(None) };
                        if owner != ANOTHER_OWNER {
                            return Ok(Some(Action::NewContext { id, title: name, owner, confirm_fallback: false }));
                        }
                        let Some(typed) = dialogs.input(title, "Owner login:", "", Validator::Name).map_err(failed)? else { return Ok(None) };
                        let owner = check_login(&typed)?;
                        if owners.contains(&owner) {
                            return Ok(Some(Action::NewContext { id, title: name, owner, confirm_fallback: false }));
                        }
                        let text = format!("{owner} has no policy. cpd will use clayton-duarte's policy for it and link {owner}.toml to it.");
                        let yes = dialogs.confirm(title, &text, USE_FALLBACK).map_err(failed)?;
                        Ok(yes.then_some(Action::NewContext { id, title: name, owner, confirm_fallback: true }))
                    }
                }
            }
            Question::Confirm { title, text, yes_label, then } => Ok(dialogs.confirm(title, &text, yes_label).map_err(failed)?.then_some(then)),
            Question::Delete { what, label } => {
                let preview = dialogs.preview(&what)?;
                let text = delete_text(&label, &what, &preview);
                Ok(dialogs.confirm("Delete", &text, "Delete").map_err(failed)?.then_some(Action::Delete { what, yes: true }))
            }
        }
    }
}

/// The popup's text: what goes, with the counts of the command's preview (Plans only for a Context's), and that it cannot be undone.
fn delete_text(label: &str, what: &Deletable, preview: &Value) -> String {
    let count = |key: &str| preview[key].as_u64().unwrap_or(0);
    let plans = match what {
        Deletable::Context(_) => format!("{} Plans, ", preview["plans"].as_array().map_or(0, Vec::len)),
        Deletable::Plan(_) => String::new(),
    };
    format!(
        "Delete {label}? {plans}{} Jobs, {} Attempts, {} Sessions, {} worktrees and ~{} MB of disk go. This cannot be undone.",
        count("jobs"),
        count("attempts"),
        count("sessions"),
        count("worktrees"),
        count("disk_freed_mb"),
    )
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

        fn select(&self, title: &str, prompt: &str, options: &[String]) -> Result<Option<String>, CpdError> {
            self.opened.borrow_mut().push(format!("select {title} | {prompt} | {}", options.join(", ")));
            Ok(self.text.map(|_| options[0].clone()))
        }

        fn preview(&self, _: &Deletable) -> Result<Value, String> {
            Ok(serde_json::json!({"jobs": 47, "attempts": 121, "sessions": 3, "worktrees": 11, "disk_freed_mb": 342, "plans": [{}, {}]}))
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
        assert_eq!(label(Ask::NewContext { taken: vec![], owners: vec!["o".into()] }, "New Context", ""), ["input New Context | Title: |  | title"]);
        assert_eq!(label(Ask::NewPlan { context: cpd(), taken: vec![] }, "New Plan", ""), ["input New Plan | Title: |  | title"]);
        assert_eq!(label(Ask::RenameSession(sid()), "Rename", "ship"), ["input Rename | Name: | ship | name"]);
        assert_eq!(label(Ask::Fork { source: sid() }, "Fork", "ship (fork)"), ["input Fork | Name: | ship (fork) | name"]);
        let named = Ask::NamedPlan { context: cpd(), label: "Plan sketch".into() };
        assert_eq!(label(named, "New Plan", ""), ["input New Plan | Fork Plan sketch into a new Plan, name: |  | plan-id"]);
    }

    #[test]
    fn a_cancelled_input_runs_nothing() {
        for ask in [Ask::RenameContext(cpd()), Ask::Fork { source: sid() }, Ask::NewContext { taken: vec![], owners: vec!["o".into()] }, Ask::NamedPlan { context: cpd(), label: "x".into() }] {
            assert_eq!(input(ask, "x").ask(&Stub::cancelling()), Ok(None));
        }
    }

    #[test]
    fn new_context_derives_its_id_from_the_title_against_the_taken_ones() {
        let run = |ids: &[&str]| input(Ask::NewContext { taken: taken(ids), owners: vec!["o".into()] }, "").ask(&Stub::typing("Building cpd!"));
        let new = |id: &str| Ok(Some(Action::NewContext { id: ContextId::parse(id).unwrap(), title: "Building cpd!".into(), owner: "o".into(), confirm_fallback: false }));
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
            assert_eq!(input(Ask::NewContext { taken: vec![], owners: vec!["o".into()] }, "").ask(&Stub::typing(title)), Err(NO_ID.to_owned()), "{title}");
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
    fn a_delete_lists_the_previews_counts_and_runs_the_yes_command_only_when_confirmed() {
        let plan = || Question::delete(Deletable::Plan(PlanId::parse("old").unwrap()), "Plan old");
        let stub = Stub::confirming(true);
        assert_eq!(plan().ask(&stub), Ok(Some(Action::Delete { what: Deletable::Plan(PlanId::parse("old").unwrap()), yes: true })));
        assert_eq!(
            stub.opened.into_inner(),
            ["confirm Delete | Delete Plan old? 47 Jobs, 121 Attempts, 3 Sessions, 11 worktrees and ~342 MB of disk go. This cannot be undone. | Delete"]
        );
        assert_eq!(plan().ask(&Stub::confirming(false)), Ok(None));
        let context = Question::delete(Deletable::Context(cpd()), "alpha");
        let stub = Stub::confirming(false);
        assert_eq!(context.ask(&stub), Ok(None));
        assert!(stub.opened.into_inner()[0].starts_with("confirm Delete | Delete alpha? 2 Plans, 47 Jobs"));
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
            fn select(&self, _: &str, _: &str, _: &[String]) -> Result<Option<String>, CpdError> {
                unreachable!("no select in this test")
            }
            fn preview(&self, _: &Deletable) -> Result<Value, String> {
                Err("refused".to_owned())
            }
        }
        let why = "No tmux client is attached to the cockpit.".to_owned();
        assert_eq!(input(Ask::Fork { source: sid() }, "").ask(&Failing), Err(why.clone()));
        assert_eq!(Question::close(sid(), "x", false).ask(&Failing), Err(why));
    }
}
