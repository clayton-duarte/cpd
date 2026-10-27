//! Two clicks at most (pro-d111, pro-d114; commands.md §13.2 "The dialog kit", the tree's keys and right-click menu):
//! Close, Archive and the `x` key run at once with no popup, Delete keeps its `confirm`, and New Context… is ONE `form`
//! dialog. The tree is driven through its keys and menu; the dialogs are a stub that records what was opened: no tmux.

use std::cell::RefCell;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::actions::{Action, Deletable, Free, IN_CHAT_SLOT, Subject};
use super::ask::Dialogs;
use super::fixture::{WHEN, sample};
use super::view::{Effect, View};
use crate::dialog::validate::Validator;
use cpd_core::error::CpdError;
use cpd_core::ids::PlanId;
use cpd_core::session::State;
use serde_json::Value;

const NO_POLICY: &str = "No owner policy: add $CPD_HOME/policy/<owner>/owner.toml";

/// The dialogs, as stubs: `opened` is every one opened, in order. A `confirm` answers `yes`, a `form` answers `form`.
struct Stub {
    opened: RefCell<Vec<String>>,
    yes: bool,
    form: Option<(&'static str, &'static str)>,
}

impl Stub {
    fn new(yes: bool, form: Option<(&'static str, &'static str)>) -> Stub {
        Stub { opened: RefCell::default(), yes, form }
    }

    fn opened(&self) -> Vec<String> {
        self.opened.borrow().clone()
    }
}

impl Dialogs for Stub {
    fn input(&self, title: &str, prompt: &str, _: &str, _: Validator) -> Result<Option<String>, CpdError> {
        self.opened.borrow_mut().push(format!("input {title} | {prompt}"));
        Ok(self.form.map(|(title, _)| title.to_owned()))
    }

    fn confirm(&self, title: &str, text: &str, yes_label: &str) -> Result<bool, CpdError> {
        self.opened.borrow_mut().push(format!("confirm {title} | {text} | {yes_label}"));
        Ok(self.yes)
    }

    fn select(&self, title: &str, prompt: &str, options: &[String]) -> Result<Option<String>, CpdError> {
        self.opened.borrow_mut().push(format!("select {title} | {prompt} | {}", options.join(", ")));
        Ok(self.form.map(|_| options[0].clone()))
    }

    fn form(&self, title: &str, input_label: &str, select_label: &str, options: &[String]) -> Result<Option<(String, String)>, CpdError> {
        self.opened.borrow_mut().push(format!("form {title} | {input_label:?} | {select_label:?} | {}", options.join(", ")));
        Ok(self.form.map(|(t, o)| (t.to_owned(), o.to_owned())))
    }

    fn preview(&self, _: &Deletable) -> Result<Value, String> {
        Ok(serde_json::json!({"jobs": 47, "attempts": 121, "sessions": 3, "worktrees": 11, "disk_freed_mb": 342, "plans": [{}, {}]}))
    }
}

/// What the caller of the view does with `effect`: the words of the command it runs (`None`: nothing runs), after the
/// dialogs the effect opens, which the stub records. A refusal of the popup's answer is the `Err`.
fn resolve(effect: Effect, dialogs: &Stub) -> Result<Option<Vec<String>>, String> {
    match effect {
        Effect::None => Ok(None),
        Effect::Run(action) => Ok(Some(action.argv())),
        Effect::Ask(question) => question.ask(dialogs).map(|action| action.map(|a| a.argv())),
        other => panic!("a command or a popup, got {other:?}"),
    }
}

fn words(w: &[&str]) -> Option<Vec<String>> {
    Some(w.iter().map(|s| (*s).to_owned()).collect())
}

fn press(view: &mut View, code: KeyCode) -> Effect {
    view.key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn drawn(view: &mut View) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(44, 24)).unwrap();
    terminal.draw(|f| view.draw(f, 0)).unwrap();
    let buf = terminal.backend().buffer();
    (0..24).map(|y| (0..44).map(|x| buf[(x, y)].symbol()).collect()).collect()
}

/// A view of `tree` with the cursor on the row `path`, as after an arrow key.
fn on(tree: crate::read::Tree, path: &[&str]) -> View {
    let mut view = View::new(tree, true);
    press(&mut view, KeyCode::Down);
    view.state.select(path.iter().map(|p| (*p).to_owned()).collect());
    view
}

/// The right-click menu of the row drawn at (`column`, `row`), the item `label` picked with the arrow keys and Enter.
fn pick(view: &mut View, column: u16, row: u16, label: &str) -> Effect {
    drawn(view);
    let click = |button| MouseEvent { kind: MouseEventKind::Down(button), column, row, modifiers: KeyModifiers::NONE };
    assert_eq!(view.mouse(click(MouseButton::Right)), Effect::None);
    let rows = drawn(view);
    let first = usize::from(row) + 1; // the first item is the row below the click
    let at = (first..rows.len())
        .find(|y| rows[*y].contains(&format!("│ {label}")))
        .unwrap_or_else(|| panic!("no menu item {label:?} at ({column}, {row}): {rows:#?}"));
    for _ in first..at {
        press(view, KeyCode::Down);
    }
    press(view, KeyCode::Enter)
}

/// `sample()` with its Plan `old` asleep: a Plan the menu offers Delete on.
fn deletable() -> crate::read::Tree {
    let mut tree = sample();
    tree.contexts[0].plans[2].sessions[0].asleep_at = Some(WHEN.into());
    tree
}

#[test]
fn close_menu_item_runs_session_close_at_once() {
    let mut view = View::new(sample(), true);
    let stub = Stub::new(false, None);
    // the fork `f-blocked`; a cancelling stub would stop a popup, so only no popup runs the command
    assert_eq!(resolve(pick(&mut view, 8, 5, "Close"), &stub), Ok(words(&["session", "close", "f-blocked"])));
    assert_eq!(stub.opened(), Vec::<String>::new(), "no popup");
    // a working Session is closed at once as well
    let mut tree = sample();
    tree.contexts[0].plans[0].sessions[1].state = State::Working;
    let mut view = View::new(tree, true);
    assert_eq!(resolve(pick(&mut view, 8, 9, "Close"), &stub), Ok(words(&["session", "close", "f-idle"])));
    assert_eq!(stub.opened(), Vec::<String>::new(), "no popup");
    // the Session in the chat slot is still refused, in a `text` dialog
    let mut view = View::new(sample(), true);
    assert_eq!(pick(&mut view, 8, 6, "Close"), Effect::Text { title: "Close Session".into(), text: IN_CHAT_SLOT.into() });
}

#[test]
fn x_key_runs_session_close_at_once() {
    let stub = Stub::new(false, None);
    let mut view = on(sample(), &["alpha", "sketch"]);
    assert_eq!(resolve(press(&mut view, KeyCode::Char('x')), &stub), Ok(words(&["session", "close", "l-sketch"])));
    let mut tree = sample();
    tree.contexts[0].plans[0].sessions[1].state = State::Working;
    let mut view = on(tree, &["alpha", "f-idle"]);
    assert_eq!(resolve(press(&mut view, KeyCode::Char('x')), &stub), Ok(words(&["session", "close", "f-idle"])), "working too");
    assert_eq!(stub.opened(), Vec::<String>::new(), "no popup");
    let mut view = on(sample(), &["alpha", "ship"]);
    assert_eq!(press(&mut view, KeyCode::Char('x')), Effect::Text { title: "Close Session".into(), text: IN_CHAT_SLOT.into() }, "the chat slot is refused");
}

#[test]
fn archive_menu_item_runs_the_archive_at_once() {
    let stub = Stub::new(false, None);
    let mut view = View::new(sample(), true);
    // a Session row, a Plan row and a Context row
    assert_eq!(resolve(pick(&mut view, 8, 5, "Archive"), &stub), Ok(words(&["session", "archive", "f-blocked"])));
    assert_eq!(resolve(pick(&mut view, 8, 7, "Archive"), &stub), Ok(words(&["plan", "archive", "--plan=sketch"])));
    assert_eq!(resolve(pick(&mut view, 5, 1, "Archive"), &stub), Ok(words(&["context", "archive", "beta"])));
    assert_eq!(stub.opened(), Vec::<String>::new(), "no popup");
    // the step that frees the chat slot is unchanged: the Plan whose Lead holds it opens the next live Session first
    let ship = Subject::Plan(PlanId::parse("ship").unwrap());
    let freed = Effect::Run(Action::Archive { subject: ship, free: Some(Free::Open(cpd_core::ids::SessionId::parse("l-sketch").unwrap())) });
    assert_eq!(pick(&mut View::new(sample(), true), 8, 6, "Archive"), freed);
}

#[test]
fn delete_still_asks_a_confirm_popup() {
    // cancelled: the preview's counts are asked in a `confirm` `[Delete] [Cancel]`, and nothing runs
    let cancel = Stub::new(false, None);
    let mut view = View::new(deletable(), true);
    assert_eq!(resolve(pick(&mut view, 8, 8, "Delete"), &cancel), Ok(None));
    let opened = cancel.opened();
    assert_eq!(opened.len(), 1, "{opened:?}");
    assert!(opened[0].starts_with("confirm Delete | Delete "), "{opened:?}");
    assert!(opened[0].contains("47 Jobs, 121 Attempts, 3 Sessions, 11 worktrees and ~342 MB of disk go. This cannot be undone."), "{opened:?}");
    assert!(opened[0].ends_with(" | Delete"), "the buttons are [Delete] [Cancel]: {opened:?}");
    // confirmed: the delete runs with `--yes`
    let yes = Stub::new(true, None);
    assert_eq!(resolve(pick(&mut view, 8, 8, "Delete"), &yes), Ok(words(&["plan", "delete", "old", "--yes"])));
    // Delete is the only confirm left in the menu: Close and Archive of the same view open none, a confirming stub or not
    assert_eq!(resolve(pick(&mut view, 8, 5, "Close"), &yes), Ok(words(&["session", "close", "f-blocked"])));
    assert_eq!(resolve(pick(&mut view, 8, 5, "Archive"), &yes), Ok(words(&["session", "archive", "f-blocked"])));
    assert_eq!(yes.opened().len(), 1, "only the delete's confirm: {:?}", yes.opened());
}

/// A view whose New Context… reads the owner policies of a temp dir with `files` (paths under `policy/`).
fn owned(files: &[&str]) -> (View, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    for f in files {
        let path = dir.path().join(f);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }
    let mut view = View::new(sample(), true);
    view.policy_dir = Some(dir.path().to_path_buf());
    (view, dir)
}

/// The owner policies, in the order a directory listing may give them.
const OWNERS: [&str; 3] = ["missionlane/owner.toml", "clayton-duarte/owner.toml", "acme-co/owner.toml"];
const FORM: &str = "form New Context | \"Title: \" | \"Owner:\" | acme-co, clayton-duarte, missionlane";

#[test]
fn new_context_opens_one_form_dialog_with_the_title_and_the_owners() {
    let (mut view, _dir) = owned(&OWNERS);
    let stub = Stub::new(false, None);
    let effect = pick(&mut view, 8, 15, "New Context…");
    assert_eq!(resolve(effect, &stub), Ok(None));
    assert_eq!(stub.opened(), [FORM], "one dialog, a `form` titled New Context, the owners sorted");
}

#[test]
fn new_context_form_accepted_runs_context_new_with_the_chosen_owner() {
    let (mut view, _dir) = owned(&OWNERS);
    let stub = Stub::new(false, Some(("Building cpd!", "missionlane")));
    let run = resolve(pick(&mut view, 8, 15, "New Context…"), &stub);
    assert_eq!(run, Ok(words(&["context", "new", "building-cpd", "--title=Building cpd!", "--owner=missionlane"])), "no --confirm-fallback");
    assert_eq!(stub.opened(), [FORM], "no other dialog");
    // the id is derived against the Contexts of the tree, as before: `alpha` is taken
    let stub = Stub::new(false, Some(("Alpha", "acme-co")));
    let run = resolve(pick(&mut view, 8, 15, "New Context…"), &stub);
    assert_eq!(run, Ok(words(&["context", "new", "alpha-2", "--title=Alpha", "--owner=acme-co"])));
}

#[test]
fn new_context_form_cancelled_runs_nothing() {
    let (mut view, _dir) = owned(&OWNERS);
    let stub = Stub::new(true, None);
    assert_eq!(resolve(pick(&mut view, 8, 15, "New Context…"), &stub), Ok(None));
    assert_eq!(stub.opened(), [FORM]);
}

#[test]
fn new_context_offers_no_another_owner() {
    let (mut view, _dir) = owned(&["clayton-duarte/owner.toml"]);
    let stub = Stub::new(false, Some(("Building cpd!", "clayton-duarte")));
    let run = resolve(pick(&mut view, 8, 15, "New Context…"), &stub);
    assert_eq!(stub.opened(), ["form New Context | \"Title: \" | \"Owner:\" | clayton-duarte"], "the one owner is the only option");
    assert_eq!(run, Ok(words(&["context", "new", "building-cpd", "--title=Building cpd!", "--owner=clayton-duarte"])));
    assert!(!stub.opened().iter().any(|o| o.contains("Another owner") || o.starts_with("input") || o.starts_with("confirm")));
}

#[test]
fn new_context_with_no_owner_policy_shows_the_text_dialog_not_the_form() {
    let (mut view, _dir) = owned(&["notes.md"]);
    assert_eq!(pick(&mut view, 8, 15, "New Context…"), Effect::Text { title: "New Context".into(), text: NO_POLICY.into() }, "no form opens");
    // a title no id can be made of is refused in a `text` dialog, after the form, and nothing runs
    let (mut view, _dir) = owned(&OWNERS);
    let stub = Stub::new(false, Some(("123", "acme-co")));
    let refused = resolve(pick(&mut view, 8, 15, "New Context…"), &stub);
    let why = refused.expect_err("the refusal is said in a text dialog");
    assert!(why.starts_with("A title needs at least two letters or digits"), "{why}");
    assert_eq!(stub.opened(), [FORM], "the form opened once, then nothing");
}
