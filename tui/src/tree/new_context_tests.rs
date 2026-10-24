//! New Context…: after the title the owner is always asked, in a `select` popup (the dialog kit).

use std::cell::RefCell;

use super::actions::{Action, Deletable};
use super::ask::{Ask, Dialogs, Question};
use crate::dialog::validate::Validator;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::ids::ContextId;
use serde_json::Value;

/// Logs each popup (kind, title, prompt, options); answers `input` with the title and `select` with `choice`.
struct Recording {
    popups: RefCell<Vec<String>>,
    title: &'static str,
    choice: Result<Option<&'static str>, &'static str>,
}

impl Recording {
    fn new(title: &'static str, choice: Result<Option<&'static str>, &'static str>) -> Recording {
        Recording { popups: RefCell::default(), title, choice }
    }
    fn log(&self) -> Vec<String> {
        self.popups.borrow().clone()
    }
}

impl Dialogs for Recording {
    fn input(&self, title: &str, prompt: &str, _: &str, _: Validator) -> Result<Option<String>, CpdError> {
        self.popups.borrow_mut().push(format!("input | {title} | {prompt}"));
        Ok(Some(self.title.to_owned()))
    }
    fn confirm(&self, _: &str, _: &str, _: &str) -> Result<bool, CpdError> {
        unreachable!("no confirm in New Context")
    }
    fn select(&self, title: &str, prompt: &str, options: &[String]) -> Result<Option<String>, CpdError> {
        self.popups.borrow_mut().push(format!("select | {title} | {prompt} | {}", options.join(", ")));
        match self.choice {
            Ok(c) => Ok(c.map(str::to_owned)),
            Err(m) => Err(CpdError::new(ErrorCode::TmuxFailed, m)),
        }
    }
    fn preview(&self, _: &Deletable) -> Result<Value, String> {
        unreachable!("no preview in New Context")
    }
}

fn question(owners: &[&str]) -> Question {
    let owners = owners.iter().map(|o| o.to_string()).collect();
    Question::input("New Context", Ask::NewContext { taken: vec![], owners }, String::new())
}

fn new_context(id: &str, title: &str, owner: &str) -> Action {
    Action::NewContext { id: ContextId::parse(id).unwrap(), title: title.into(), owner: owner.into(), confirm_fallback: false }
}

#[test]
fn the_owner_is_asked_in_a_select_popup_after_the_title_even_when_there_is_only_one() {
    let dialogs = Recording::new("Building cpd!", Ok(Some("clayton-duarte")));
    let action = question(&["clayton-duarte"]).ask(&dialogs);
    assert_eq!(dialogs.log(), ["input | New Context | Title:", "select | New Context | Owner: | clayton-duarte"]);
    assert_eq!(action, Ok(Some(new_context("building-cpd", "Building cpd!", "clayton-duarte"))));
}

#[test]
fn the_select_lists_every_owner_in_the_order_given_and_context_new_runs_with_the_chosen_one() {
    let dialogs = Recording::new("Building cpd!", Ok(Some("b")));
    let action = question(&["a", "b", "c"]).ask(&dialogs);
    assert_eq!(dialogs.log()[1], "select | New Context | Owner: | a, b, c");
    assert_eq!(action, Ok(Some(new_context("building-cpd", "Building cpd!", "b"))));
}

#[test]
fn cancelling_the_owner_select_runs_nothing() {
    let dialogs = Recording::new("Building cpd!", Ok(None));
    assert_eq!(question(&["a", "b"]).ask(&dialogs), Ok(None));
    assert_eq!(dialogs.log().len(), 2, "{:?}", dialogs.log());
}

#[test]
fn a_title_no_id_can_be_made_of_is_refused_before_the_owner_is_asked() {
    let dialogs = Recording::new("2024", Ok(Some("a")));
    let action = question(&["a", "b"]).ask(&dialogs);
    assert!(matches!(&action, Err(m) if m.starts_with("A title needs at least two letters or digits")), "{action:?}");
    assert_eq!(dialogs.log(), ["input | New Context | Title:"]);    // the select is the owner step of this flow: a usable title does reach it
    let usable = Recording::new("Building cpd!", Ok(Some("a")));
    question(&["a", "b"]).ask(&usable).unwrap();
    assert_eq!(usable.log().len(), 2, "{:?}", usable.log());
}

#[test]
fn a_failing_owner_select_is_refused_with_its_message_and_runs_nothing() {
    let dialogs = Recording::new("Building cpd!", Err("No tmux client is attached to the cockpit."));
    assert_eq!(question(&["a", "b"]).ask(&dialogs), Err("No tmux client is attached to the cockpit.".to_owned()));
}
