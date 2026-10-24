//! New Context…: the `Another owner…` row, the login input and the confirm popup of an unknown owner
//! (docs/interface.md, docs/commands.md). Every dialog is a stub: no tmux, no sleep.

use std::cell::RefCell;

use super::actions::{Action, Deletable};
use super::ask::{check_login, Ask, Dialogs, Question, ANOTHER_OWNER, USE_FALLBACK};
use crate::dialog::validate::Validator;
use cpd_core::error::CpdError;
use cpd_core::ids::ContextId;
use serde_json::Value;

/// What the stub answers: the texts of the `input` popups in order (`None` cancels), the row the select picks, and
/// whether the confirm says yes. `log` is every popup opened.
struct Stub {
    inputs: RefCell<Vec<Option<&'static str>>>,
    pick: Option<&'static str>,
    yes: bool,
    log: RefCell<Vec<String>>,
}

impl Stub {
    fn new(inputs: &[Option<&'static str>], pick: Option<&'static str>, yes: bool) -> Stub {
        Stub { inputs: RefCell::new(inputs.iter().rev().copied().collect()), pick, yes, log: RefCell::default() }
    }
    fn log(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
    fn confirms(&self) -> Vec<String> {
        self.log().into_iter().filter(|l| l.starts_with("confirm")).collect()
    }
}

impl Dialogs for Stub {
    fn input(&self, title: &str, prompt: &str, _: &str, _: Validator) -> Result<Option<String>, CpdError> {
        self.log.borrow_mut().push(format!("input | {title} | {prompt}"));
        Ok(self.inputs.borrow_mut().pop().expect("an input popup the test did not expect").map(str::to_owned))
    }
    fn confirm(&self, title: &str, text: &str, yes_label: &str) -> Result<bool, CpdError> {
        self.log.borrow_mut().push(format!("confirm | {title} | {text} | {yes_label}"));
        Ok(self.yes)
    }
    fn select(&self, title: &str, prompt: &str, options: &[String]) -> Result<Option<String>, CpdError> {
        self.log.borrow_mut().push(format!("select | {title} | {prompt} | {}", options.join(", ")));
        Ok(self.pick.map(str::to_owned))
    }
    fn preview(&self, _: &Deletable) -> Result<Value, String> {
        unreachable!("no preview in New Context")
    }
}

fn question(owners: &[&str]) -> Question {
    let owners = owners.iter().map(|o| o.to_string()).collect();
    Question::input("New Context", Ask::NewContext { taken: vec![], owners }, String::new())
}

fn action(owner: &str, confirm_fallback: bool) -> Action {
    Action::NewContext {
        id: ContextId::parse("building-cpd").unwrap(),
        title: "Building cpd!".into(),
        owner: owner.into(),
        confirm_fallback,
    }
}

const KNOWN: [&str; 3] = ["clayton-duarte", "missionlane", "missionlane-scratch"];

fn argv(a: &Action) -> Vec<String> {
    a.argv()
}

#[test]
fn t1_the_select_lists_the_known_owners_then_another_owner_last() {
    let stub = Stub::new(&[Some("Building cpd!")], None, false);
    assert_eq!(question(&KNOWN).ask(&stub), Ok(None));
    let options = format!("{}, {ANOTHER_OWNER}", KNOWN.join(", "));
    assert_eq!(stub.log()[1], format!("select | New Context | Owner: | {options}"));
}

#[test]
fn t1_picking_a_known_owner_runs_context_new_without_confirm_fallback() {
    let stub = Stub::new(&[Some("Building cpd!")], Some("missionlane"), false);
    let got = question(&KNOWN).ask(&stub);
    assert!(stub.log()[1].ends_with(&format!(", {ANOTHER_OWNER}")), "the row is offered: {:?}", stub.log());
    assert_eq!(got, Ok(Some(action("missionlane", false))));
    assert!(!argv(&got.unwrap().unwrap()).contains(&"--confirm-fallback".to_owned()));
}

#[test]
fn t2_picking_another_owner_opens_an_input_for_the_login() {
    let stub = Stub::new(&[Some("Building cpd!"), Some("missionlane")], Some(ANOTHER_OWNER), false);
    question(&KNOWN).ask(&stub).unwrap();
    let log = stub.log();
    assert_eq!(log.len() >= 3 && log[2].starts_with("input | "), true, "an input follows the select: {log:?}");
}

#[test]
fn t2_cancelling_the_login_input_runs_nothing() {
    let stub = Stub::new(&[Some("Building cpd!"), None], Some(ANOTHER_OWNER), true);
    assert_eq!(question(&KNOWN).ask(&stub), Ok(None));
    assert!(stub.confirms().is_empty(), "{:?}", stub.log());
}

#[test]
fn t2_the_login_check_refuses_an_empty_string_and_what_is_no_github_login() {
    for bad in ["", "a b", "-x", &"a".repeat(40)] {
        assert!(check_login(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn t2_the_login_check_accepts_a_login_and_lowercases_it() {
    assert_eq!(check_login("Acme-Co"), Ok("acme-co".to_owned()));
}

#[test]
fn t2_a_typed_login_is_lowercased_in_the_action() {
    let stub = Stub::new(&[Some("Building cpd!"), Some("Missionlane")], Some(ANOTHER_OWNER), false);
    assert_eq!(question(&KNOWN).ask(&stub), Ok(Some(action("missionlane", false))));
}

#[test]
fn t3_a_typed_known_login_opens_no_confirm_popup() {
    let stub = Stub::new(&[Some("Building cpd!"), Some("missionlane-scratch")], Some(ANOTHER_OWNER), true);
    assert_eq!(question(&KNOWN).ask(&stub), Ok(Some(action("missionlane-scratch", false))));
    assert!(stub.confirms().is_empty(), "{:?}", stub.log());
}

#[test]
fn t4_a_typed_unknown_login_opens_one_confirm_naming_it_and_the_fallback_owner() {
    let stub = Stub::new(&[Some("Building cpd!"), Some("acme-co")], Some(ANOTHER_OWNER), false);
    question(&KNOWN).ask(&stub).unwrap();
    let confirms = stub.confirms();
    assert_eq!(confirms.len(), 1, "{:?}", stub.log());
    assert!(confirms[0].contains("acme-co") && confirms[0].contains("clayton-duarte"), "{confirms:?}");
    assert!(confirms[0].ends_with(&format!("| {USE_FALLBACK}")), "{confirms:?}");
    assert_eq!(USE_FALLBACK, "Use clayton-duarte's policy");
}

#[test]
fn t4_yes_runs_context_new_with_confirm_fallback_last_in_the_argv() {
    let stub = Stub::new(&[Some("Building cpd!"), Some("acme-co")], Some(ANOTHER_OWNER), true);
    let got = question(&KNOWN).ask(&stub);
    assert_eq!(got, Ok(Some(action("acme-co", true))));
    assert_eq!(
        argv(&got.unwrap().unwrap()),
        ["context", "new", "building-cpd", "--title=Building cpd!", "--owner=acme-co", "--confirm-fallback"]
    );
}

#[test]
fn t4_no_runs_nothing() {
    let stub = Stub::new(&[Some("Building cpd!"), Some("acme-co")], Some(ANOTHER_OWNER), false);
    assert_eq!(question(&KNOWN).ask(&stub), Ok(None));
    assert_eq!(stub.confirms().len(), 1, "{:?}", stub.log());
}

#[test]
fn t5_the_title_is_asked_before_the_owner() {
    let stub = Stub::new(&[Some("Building cpd!")], None, false);
    question(&KNOWN).ask(&stub).unwrap();
    let log = stub.log();
    assert!(log[0].starts_with("input | New Context | Title:") && log[1].starts_with("select"), "{log:?}");
    assert!(log[1].ends_with(ANOTHER_OWNER), "{log:?}");
}

#[test]
fn t5_cancelling_the_owner_step_after_the_title_runs_nothing() {
    let stub = Stub::new(&[Some("Building cpd!")], None, true);
    assert_eq!(question(&KNOWN).ask(&stub), Ok(None));
    assert_eq!(stub.log().len(), 2, "{:?}", stub.log());
    assert!(stub.log()[1].ends_with(ANOTHER_OWNER), "the owner step is the select with the extra row: {:?}", stub.log());
}
