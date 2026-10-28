//! The clicks and the right-click menu of the Inbox section in the `widgets` view (commands.md §13.2, "Clicks on an Inbox
//! item" and the right-click paragraph; pro-d55): a click on an item row jumps to its cause, a right click opens the menu of
//! four entries. The view runs in this process on a script (`common/inbox.rs`): the `ManualClock`, a scripted pump, the fake
//! tmux server (one client, the cockpit `cpd` with the view and the chat slot) and the fake `cpd` of the `FakeRunner`.
//!
//! What a click runs through `run_command` (the in-process command tree) is recorded and answered by this file's own
//! `recording` instead of running; what the view runs through the runner is the fake `cpd`'s. [`cpd_calls`] reads both, so a test
//! pins the argv of each command, not which of the two carried it.

use crate::common;

use std::cell::RefCell;
use std::ffi::OsString;
use std::sync::{Arc, Mutex, PoisonError};

use common::fx::Fx;
use common::inbox::{World, condition, four, frame_with, gone, open, plan_row, rows, seed, session_row, show, show_with, view_fx};
use common::screen::{Screen, right_click};
use common::tree::{engine, stamp};
use common::sql;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::runner::Reply;
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::output::Output;
use cpd_tui::pump::Signal;
use ratatui::crossterm::event::KeyCode;
use serde_json::{Value, json};

/// The client the fake server's `list-clients` names.
const CLIENT: &str = "/dev/pts/9";

/// The height of the pane: the Inbox is pinned to its bottom rows.
const HEIGHT: u16 = 30;

// ---------- what a click runs ----------

/// How a command is answered.
#[derive(Clone)]
enum Says {
    Ok(Value),
    Refused(ErrorCode, &'static str),
}

thread_local! {
    /// The words (without the program) of every command the view ran through `run_command`, in order.
    static RAN: RefCell<Vec<Vec<String>>> = const { RefCell::new(Vec::new()) };
    /// What `run_command` answers: the first entry whose words start the command's.
    static SAYS: RefCell<Vec<(Vec<String>, Says)>> = const { RefCell::new(Vec::new()) };
}

/// `run_command` of the view under test: records the words and answers from [`says`], `{}` for a command nobody scripted.
fn recording(argv: Vec<OsString>) -> Result<Output, CpdError> {
    let mut words: Vec<String> = argv.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    if words.first().is_some_and(|w| w == "cpd") {
        words.remove(0);
    }
    RAN.with(|ran| ran.borrow_mut().push(words.clone()));
    let said = SAYS.with(|says| says.borrow().iter().find(|(prefix, _)| words.starts_with(prefix)).map(|(_, said)| said.clone()));
    match said {
        None => Ok(Output::success(json!({}))),
        Some(Says::Ok(fields)) => Ok(Output::success(fields)),
        Some(Says::Refused(code, message)) => Err(CpdError::new(code, message)),
    }
}

/// The commands starting with `prefix` answer `said`, both through `run_command` and through the runner's fake `cpd`.
fn says(fx: &Fx, prefix: &[&str], said: Says) {
    let words: Vec<String> = prefix.iter().map(|w| (*w).to_owned()).collect();
    SAYS.with(|all| all.borrow_mut().push((words.clone(), said.clone())));
    let reply = match said {
        Says::Ok(fields) => {
            let mut reply = fields.as_object().expect("the fields of a reply").clone();
            reply.insert("ok".to_owned(), json!(true));
            Reply::ok(&Value::Object(reply).to_string())
        }
        Says::Refused(code, message) => Reply::exit(3, &json!({"ok": false, "error": {"code": code.as_str(), "message": message}}).to_string(), ""),
    };
    let _ = (*fx.fakes.runner).clone().on("cpd", move |call| call.args.starts_with(&words), reply);
}

/// Every command of `cpd` the view ran, without the program, in order: first what went through `run_command`, then what went
/// through the runner.
fn cpd_calls(fx: &Fx) -> Vec<Vec<String>> {
    let mut all = RAN.with(|ran| ran.borrow().clone());
    all.extend(fx.calls("cpd").into_iter().map(|call| call.args));
    all
}

fn words(w: &[&str]) -> Vec<String> {
    w.iter().map(|s| (*s).to_owned()).collect()
}

/// The urls the view put in tmux's buffer (`set-buffer -w -- <url>`), in order.
fn copies(fx: &Fx) -> Vec<String> {
    let sent = fx.calls("tmux").into_iter().map(|c| c.args).filter(|args| args.iter().any(|a| a == "set-buffer"));
    sent.map(|args| {
        assert_eq!(args[args.len() - 3..], ["-w", "--", args.last().unwrap().as_str()], "{args:?}");
        args.last().unwrap().clone()
    })
    .collect()
}

/// The panes the view gave the focus (`select-pane -t <pane>`), in order.
fn focused(fx: &Fx) -> Vec<String> {
    let sent = fx.calls("tmux").into_iter().map(|c| c.args);
    sent.filter_map(|args| args.iter().position(|a| a == "select-pane").map(|at| args[at + 2].clone())).collect()
}

/// The argv (`display-popup` first) of every popup the fake server opened.
fn popups(fx: &Fx) -> Vec<Vec<String>> {
    fx.tmux_probe().state().popups()
}

/// The popups' commands, the last word of each.
fn commands(fx: &Fx) -> Vec<String> {
    popups(fx).iter().map(|p| p.last().expect("a popup has a command").clone()).collect()
}

// ---------- the fixtures ----------

/// The Contexts `alpha` and `beta`, the Plan `ship` (Lead `l-ship`), its fork `f-blocked` and the Manager `m-alpha`: no pane,
/// so none is in the chat slot. Then `items`, opened 330 s ago (`5m`), and the engine's lock is the caller's.
fn rig(items: Vec<Condition>) -> Fx {
    let fx = view_fx();
    seed(&fx, &[], 330);
    let mut store = open(&fx.env);
    let sessions = [
        session_row("l-ship", "lead", "alpha", "ship", "2026-01-02T00:00:10.000Z"),
        session_row("f-blocked", "lead", "alpha", "ship", "2026-01-02T00:00:02.000Z"),
        session_row("m-alpha", "manager", "alpha", "", "2026-01-02T00:00:07.000Z"),
    ];
    sql(&mut store, &(plan_row("ship", "alpha", "dispatch", "l-ship", "2026-01-02T00:00:10.000Z") + &sessions.concat()));
    reconcile(&mut store, &items, &stamp(&fx, -330)).unwrap();
    fx
}

/// `condition`, linked to `session` and the Plan `ship`, with `url`.
fn linked(kind: Kind, urgency: Urgency, subject: &str, label: &str, session: Option<&str>, url: Option<&str>) -> Condition {
    Condition {
        session: session.map(|s| SessionId::parse(s).unwrap()),
        plan: Some(PlanId::parse("ship").unwrap()),
        url: url.map(str::to_owned),
        ..condition(kind, urgency, subject, label)
    }
}

/// The cockpit (session `cpd`, window `main`) in the fake server of `fx`: the view and the chat placeholder beside it, and a
/// client; returns the chat pane.
fn cockpit(fx: &Fx) -> String {
    let ok = |args: &[&str]| {
        let out = fx.tmux(args);
        assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let view = ok(&["new-session", "-d", "-s", "cpd", "-n", "main", "-x", "120", "-y", "40", "-P", "-F", "#{pane_id}", "cpd tui view widgets"]);
    ok(&["set", "-p", "-t", &view, "@cpd_view", "widgets"]);
    let chat = ok(&["split-window", "-h", "-d", "-t", &view, "-P", "-F", "#{pane_id}", "cpd tui view chat"]);
    ok(&["set", "-p", "-t", &chat, "@cpd_view", "chat"]);
    fx.tmux_route.fake().add_client(CLIENT, 200, 60);
    chat
}

/// The rows of the first frame the view draws over the store of `fx`, with the Inbox section on it: where the clicks go.
fn first_rows(fx: &Fx) -> Vec<String> {
    let frames = show(fx, HEIGHT, vec![Signal::Commit, gone()], &World::new());
    let at = frame_with(&frames, 0, "the Inbox section", |r| r.iter().any(|row| row.contains("Inbox")));
    rows(&frames, at)
}

/// The row of the item `label` on `screen`, after the title.
fn row_of(screen: &[String], label: &str) -> u16 {
    let title = screen.iter().position(|r| r.contains("Inbox")).expect("the title");
    let at = screen.iter().skip(title).position(|r| r.contains(label)).unwrap_or_else(|| panic!("{label} on\n{}", screen.join("\n")));
    u16::try_from(title + at).unwrap()
}

/// A left click at the item `label`'s row, at column 10.
fn click_on(screen: &[String], label: &str) -> Signal {
    common::inbox::click(10, row_of(screen, label))
}

/// A left click on the tier glyph of the item `label`'s row (column 1): the jump.
fn click_glyph_of(screen: &[String], label: &str) -> Signal {
    common::inbox::click(1, row_of(screen, label))
}

/// A left click on the link cue of the item `label`'s row.
fn click_cue_of(screen: &[String], label: &str) -> Signal {
    let row = &screen[usize::from(row_of(screen, label))];
    let at = row.chars().position(|c| c.to_string() == cpd_tui::link::CUE).expect("the cue on the row");
    common::inbox::click(u16::try_from(at).unwrap(), row_of(screen, label))
}

/// Runs the view on `signals` (after a first commit) with the fake `cpd` answering `{"ok":true}` to what nobody scripted.
fn run(fx: &Fx, signals: Vec<Signal>) -> Vec<Screen> {
    RAN.with(|ran| ran.borrow_mut().clear());
    fx.fake("cpd", Reply::ok(r#"{"ok":true}"#));
    let mut all = vec![Signal::Commit];
    all.extend(signals);
    all.push(gone());
    show_with(fx, HEIGHT, all, &World::new(), recording)
}

/// The labels of the menu box on `screen`: the `│ <label> │` rows between the `╮` and the `╯` rows (the segment before the last `│`; the
/// Inbox's item rows have no `│` of their own).
fn menu_labels(screen: &[String]) -> Vec<String> {
    let top = screen.iter().position(|r| r.contains('╮'));
    let Some(top) = top else { return vec![] };
    screen[top + 1..].iter().take_while(|r| !r.contains('╯')).filter_map(|r| r.rsplit('│').nth(1)).map(|l| l.trim().to_owned()).collect()
}

fn the_menu_is_closed(screen: &[String]) -> bool {
    screen.iter().all(|r| !r.contains('╮') && !r.contains('╯'))
}

/// The signals of a right click on `label`, `down` presses of Down, and Enter.
fn pick(screen: &[String], label: &str, down: usize) -> Vec<Signal> {
    let mut signals = vec![Signal::Input(right_click(10, row_of(screen, label)))];
    signals.extend((0..down).map(|_| common::inbox::press(KeyCode::Down)));
    signals.push(common::inbox::press(KeyCode::Enter));
    signals
}

// ---------- the clicks ----------

#[test]
fn a_click_on_a_dialog_or_unread_item_opens_its_session_in_the_chat_slot() {
    let fx = rig(vec![
        linked(Kind::Dialog, Urgency::Red, "session:f-blocked", "f-blocked label", Some("f-blocked"), None),
        linked(Kind::Unread, Urgency::White, "session:m-alpha", "m-alpha label", Some("m-alpha"), None),
    ]);
    let _engine = engine(&fx);
    let chat = cockpit(&fx);
    let screen = first_rows(&fx);
    run(&fx, vec![click_on(&screen, "f-blocked label"), click_on(&screen, "m-alpha label")]);
    assert_eq!(cpd_calls(&fx), [words(&["session", "open", "f-blocked"]), words(&["session", "open", "m-alpha"])]);
    assert_eq!(focused(&fx), [chat.clone(), chat], "the focus goes to the chat slot after each open");
    assert!(popups(&fx).is_empty(), "{:?}", popups(&fx));
}

#[test]
fn a_click_on_an_archived_sessions_item_runs_session_history() {
    let fx = rig(vec![linked(Kind::Dialog, Urgency::Red, "session:f-blocked", "f-blocked label", Some("f-blocked"), None)]);
    let _engine = engine(&fx);
    sql(&mut open(&fx.env), "UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'f-blocked';");
    cockpit(&fx);
    let screen = first_rows(&fx);
    run(&fx, vec![click_on(&screen, "f-blocked label")]);
    assert_eq!(cpd_calls(&fx), [words(&["session", "history", "f-blocked"])], "an archived Session is shown, not opened");
}

const REVIEW: &str = "http://10.0.0.5:20000/";

fn plan_review() -> Condition {
    linked(Kind::PlanReview, Urgency::Magenta, "review:ship/r1", "ship plan review", None, Some(REVIEW))
}

#[test]
fn a_click_on_a_plan_review_item_opens_its_lead_and_copies_the_review_page() {
    let fx = rig(vec![plan_review()]);
    let _engine = engine(&fx);
    let chat = cockpit(&fx);
    let screen = first_rows(&fx);
    run(&fx, vec![click_glyph_of(&screen, "ship plan review")]);
    assert_eq!(cpd_calls(&fx), [words(&["session", "open", "l-ship"])], "the Lead of the item's Plan is opened");
    assert_eq!(focused(&fx), [chat]);
    assert_eq!(copies(&fx), [REVIEW]);
    assert!(popups(&fx).is_empty(), "{:?}", popups(&fx));
}

#[test]
fn a_plan_review_item_whose_lead_cannot_open_copies_nothing_and_shows_a_text_dialog() {
    let fx = rig(vec![plan_review()]);
    let _engine = engine(&fx);
    cockpit(&fx);
    says(&fx, &["session", "open"], Says::Refused(ErrorCode::NoCockpit, "No cockpit."));
    let screen = first_rows(&fx);
    run(&fx, vec![click_glyph_of(&screen, "ship plan review")]);
    assert_eq!(cpd_calls(&fx), [words(&["session", "open", "l-ship"])]);
    assert_eq!(copies(&fx), Vec::<String>::new(), "the page is not copied when its Lead did not open");
    let commands = commands(&fx);
    let [dialog] = &commands[..] else { panic!("{commands:?}") };
    assert!(dialog.starts_with("cpd tui view dialog text --title="), "a text dialog: {dialog}");
    assert!(dialog.ends_with("--body='no_cockpit: No cockpit.'"), "the body is why: {dialog}");
}

#[test]
fn a_click_on_a_loop_guard_item_resumes_its_session_in_the_background() {
    let fx = rig(vec![linked(Kind::LoopGuard, Urgency::Red, "session:l-ship", "l-ship loop guard", Some("l-ship"), None)]);
    let _engine = engine(&fx);
    cockpit(&fx);
    let screen = first_rows(&fx);
    run(&fx, vec![click_on(&screen, "l-ship loop guard")]);
    assert_eq!(cpd_calls(&fx), [words(&["session", "resume", "l-ship"])]);
    assert_eq!(focused(&fx), Vec::<String>::new(), "it is resumed in the background: the focus stays");
    assert!(popups(&fx).is_empty(), "{:?}", popups(&fx));
}

#[test]
fn a_click_on_a_review_requested_item_copies_its_url() {
    let url = "https://x.io/review";
    let fx = rig(vec![Condition {
        plan: None,
        context: None,
        ..linked(Kind::ReviewRequested, Urgency::Yellow, "pr:acme/web#5", "acme/web#5 review requested", None, Some(url))
    }]);
    let _engine = engine(&fx);
    cockpit(&fx);
    let screen = first_rows(&fx);
    run(&fx, vec![click_glyph_of(&screen, "acme/web#5 review requested")]);
    assert_eq!(copies(&fx), [url]);
    assert_eq!(cpd_calls(&fx), Vec::<Vec<String>>::new(), "a copy runs no command");
    assert!(popups(&fx).is_empty(), "{:?}", popups(&fx));
}

#[test]
fn a_plain_click_on_a_pr_ready_item_does_nothing() {
    let fx = rig(vec![Condition {
        plan: None,
        ..linked(Kind::PrReady, Urgency::White, "pr:acme/api#3", "acme/api#3 ready", None, Some("https://x.io/ready"))
    }]);
    let _engine = engine(&fx);
    cockpit(&fx);
    let screen = first_rows(&fx);
    run(&fx, vec![click_glyph_of(&screen, "acme/api#3 ready")]);
    assert_eq!(popups(&fx), Vec::<Vec<String>>::new(), "no popup (pro-d111)");
    let shells: Vec<_> = fx.calls("tmux").into_iter().filter(|c| c.args.iter().any(|a| a == "run-shell")).collect();
    assert!(shells.is_empty(), "no merge starts: {shells:?}");
    assert_eq!(cpd_calls(&fx), Vec::<Vec<String>>::new());
}

#[test]
fn a_click_on_the_engine_down_item_runs_watch_ensure() {
    // no engine lock and no Manager: `engine-down` is the only item, never stored
    let fx = view_fx();
    seed(&fx, &[], 330);
    cockpit(&fx);
    says(&fx, &["watch", "--ensure"], Says::Ok(json!({"started": true})));
    let screen = first_rows(&fx);
    run(&fx, vec![click_on(&screen, "engine is down")]);
    assert_eq!(cpd_calls(&fx), [words(&["watch", "--ensure"])]);
    assert!(popups(&fx).is_empty(), "{:?}", popups(&fx));
}

/// The rows of the checklists the fake popups saw.
struct Checklists {
    seen: Arc<Mutex<Vec<Value>>>,
}

/// The popups of the fake tmux answer `answers` in order (`None`: Esc, no outcome file; a popup past the end is Esc too) and
/// record the rows of the `--items` file of a checklist.
fn checklists(fx: &Fx, answers: &[Option<&str>]) -> Checklists {
    let answers = Arc::new(Mutex::new(answers.iter().map(|a| a.map(str::to_owned)).collect::<Vec<_>>()));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (given, saw) = (answers, seen.clone());
    fx.tmux_route.fake().on_popup(move |argv| {
        let command = argv.last().expect("a popup has a command");
        if let Some(at) = command.find(" --items='") {
            let path = &command[at + " --items='".len()..];
            let path = &path[..path.find('\'').expect("the closing quote")];
            let rows = std::fs::read_to_string(path).expect("the items file");
            saw.lock().unwrap_or_else(PoisonError::into_inner).push(serde_json::from_str(&rows).expect("JSON rows"));
        }
        let mut given = given.lock().unwrap_or_else(PoisonError::into_inner);
        if given.is_empty() { None } else { given.remove(0) }
    });
    Checklists { seen }
}

fn curation(kind: Kind, label: &str, subject: &str) -> Condition {
    Condition { plan: None, ..linked(kind, Urgency::Cyan, subject, label, None, None) }
}

#[test]
fn a_click_on_a_curate_retire_item_opens_its_checklist_and_approve_runs_the_retire() {
    let fx = rig(vec![curation(Kind::CurateRetire, "alpha: retire records", "curate:alpha:retire")]);
    let _engine = engine(&fx);
    cockpit(&fx);
    let asked = checklists(&fx, &[Some(r#"{"approved":true,"ticked":["r-1"],"unticked":["r-2"]}"#)]);
    says(
        &fx,
        &["context", "curation"],
        Says::Ok(json!({
            "retire": [
                {"record": "r-1", "text": "Old note", "reason": "duplicate", "target": "r-0"},
                {"record": "r-2", "text": "Other note", "reason": "stale", "target": "r-9"},
            ],
            "retire_total": 2,
            "docs": [],
            "docs_total": 0,
        })),
    );
    let screen = first_rows(&fx);
    run(&fx, vec![click_on(&screen, "alpha: retire records")]);
    assert_eq!(
        cpd_calls(&fx),
        [
            words(&["context", "curation", "--context=alpha", "--limit=10"]),
            words(&["context", "retire", "--context=alpha", "--approve=r-1", "--reject=r-2"]),
        ]
    );
    let commands = commands(&fx);
    let [checklist] = &commands[..] else { panic!("{commands:?}") };
    assert!(checklist.starts_with("cpd tui view dialog checklist --title='Retire records of alpha' --out="), "{checklist}");
    let seen = asked.seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert_eq!(
        seen,
        [json!([
            {"id": "r-1", "text": "Old note", "detail": "duplicate r-0"},
            {"id": "r-2", "text": "Other note", "detail": "stale r-9"},
        ])]
    );
}

#[test]
fn a_click_on_a_curate_docs_item_and_later_runs_nothing() {
    let fx = rig(vec![curation(Kind::CurateDocs, "alpha: documentation candidates", "curate:alpha:docs")]);
    let _engine = engine(&fx);
    cockpit(&fx);
    // the first checklist is answered [Later] (closed unapproved), the second with Esc (a cancelled dialog writes the same
    // not-approved outcome; Lead answer deploy-act-impl-a1-q3)
    let later = Some(r#"{"approved":false,"ticked":[],"unticked":["r-3"]}"#);
    let asked = checklists(&fx, &[later, later]);
    says(
        &fx,
        &["context", "curation"],
        Says::Ok(json!({"retire": [], "retire_total": 0, "docs": [{"record": "r-3", "text": "Use sqlite", "paths": ["a.md", "b.md"]}], "docs_total": 1})),
    );
    let screen = first_rows(&fx);
    let click = || click_on(&screen, "alpha: documentation candidates");
    run(&fx, vec![click(), click()]);
    let curation = words(&["context", "curation", "--context=alpha", "--limit=10"]);
    assert_eq!(cpd_calls(&fx), [curation.clone(), curation], "each click asks for the curation, and Later and Esc run no `context docs`");
    let commands = commands(&fx);
    assert_eq!(commands.len(), 2, "one checklist per click: {commands:?}");
    assert!(commands.iter().all(|c| c.starts_with("cpd tui view dialog checklist --title='Documentation candidates of alpha' --out=")), "{commands:?}");
    let seen = asked.seen.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let rows = json!([{"id": "r-3", "text": "Use sqlite", "detail": "a.md, b.md"}]);
    assert_eq!(seen, [rows.clone(), rows]);
}

// ---------- the right-click menu ----------

const ENTRIES: [&str; 4] = ["Snooze 15 min", "Snooze 1 h", "Snooze until tomorrow 09:00", "Dismiss"];

#[test]
fn a_right_click_on_an_item_opens_its_four_entries_and_each_runs_its_command() {
    // the ids are the order of `four()`: f-blocked 1, l-rel 2, m-alpha 3, m-beta 4 (drawn third)
    let fx = rig(four());
    let _engine = engine(&fx);
    cockpit(&fx);
    let screen = first_rows(&fx);
    let mut signals = Vec::new();
    signals.extend(pick(&screen, "l-rel label", 0));
    signals.extend(pick(&screen, "l-rel label", 1));
    signals.extend(pick(&screen, "l-rel label", 2));
    signals.extend(pick(&screen, "m-beta label", 3));
    let frames = run(&fx, signals);
    let open = frame_with(&frames, 0, "the item's menu", |r| menu_labels(r) == ENTRIES);
    frame_with(&frames, open, "the menu closed", the_menu_is_closed);
    assert_eq!(
        cpd_calls(&fx),
        [
            words(&["inbox", "snooze", "2", "15m"]),
            words(&["inbox", "snooze", "2", "1h"]),
            words(&["inbox", "snooze", "2", "tomorrow"]),
            words(&["inbox", "dismiss", "4"]),
        ]
    );
    assert!(popups(&fx).is_empty(), "{:?}", popups(&fx));
}

#[test]
fn a_refused_snooze_shows_the_failure_dialog_and_esc_closes_the_menu() {
    let fx = rig(four());
    let _engine = engine(&fx);
    cockpit(&fx);
    says(&fx, &["inbox", "snooze"], Says::Refused(ErrorCode::ItemClosed, "Item 2 is closed."));
    let screen = first_rows(&fx);
    let mut signals = pick(&screen, "l-rel label", 0);
    signals.extend([Signal::Input(right_click(10, row_of(&screen, "l-rel label"))), common::inbox::press(KeyCode::Esc)]);
    let frames = run(&fx, signals);
    let first = frame_with(&frames, 0, "the menu", |r| menu_labels(r) == ENTRIES);
    let closed = frame_with(&frames, first, "the menu closed by Snooze 15 min", the_menu_is_closed);
    let again = frame_with(&frames, closed, "the menu again", |r| menu_labels(r) == ENTRIES);
    frame_with(&frames, again, "the menu closed by Esc", the_menu_is_closed);
    assert_eq!(cpd_calls(&fx), [words(&["inbox", "snooze", "2", "15m"])], "Esc runs nothing");
    let commands = commands(&fx);
    let [dialog] = &commands[..] else { panic!("{commands:?}") };
    assert!(dialog.starts_with("cpd tui view dialog text --title="), "a text dialog: {dialog}");
    assert!(dialog.ends_with("--body='item_closed: Item 2 is closed.'"), "the body is why: {dialog}");
}

#[test]
fn a_click_on_the_label_or_the_cue_of_a_link_kind_item_runs_open_with_its_url_and_no_jump() {
    let items = [
        (plan_review(), "ship plan review", REVIEW),
        (
            Condition {
                plan: None,
                context: None,
                ..linked(Kind::ReviewRequested, Urgency::Yellow, "pr:acme/web#5", "acme/web#5 review requested", None, Some("https://x.io/review"))
            },
            "acme/web#5 review requested",
            "https://x.io/review",
        ),
        (
            Condition { plan: None, ..linked(Kind::PrReady, Urgency::White, "pr:acme/api#3", "acme/api#3 ready", None, Some("https://x.io/ready")) },
            "acme/api#3 ready",
            "https://x.io/ready",
        ),
    ];
    for (item, label, url) in items {
        let fx = rig(vec![item]);
        let _engine = engine(&fx);
        cockpit(&fx);
        fx.fake("open", Reply::ok(""));
        let screen = first_rows(&fx);
        for click in [click_on(&screen, label), click_cue_of(&screen, label)] {
            let before = fx.calls("open").len();
            run(&fx, vec![click]);
            let opened: Vec<_> = fx.calls("open").into_iter().skip(before).map(|c| c.args).collect();
            assert_eq!(opened, [[url]], "one `open` with the url of {label}");
        }
        assert_eq!(cpd_calls(&fx), Vec::<Vec<String>>::new(), "no jump for {label}");
        assert_eq!(copies(&fx), Vec::<String>::new(), "no copy for {label}");
        assert!(popups(&fx).is_empty(), "{:?}", popups(&fx));
    }
}

fn review_requested() -> Condition {
    Condition {
        plan: None,
        context: None,
        ..linked(Kind::ReviewRequested, Urgency::Yellow, "pr:acme/web#5", "acme/web#5 review requested", None, Some("https://x.io/review"))
    }
}

const CLAIM_ENTRIES: [&str; 5] = ["Claim", "Snooze 15 min", "Snooze 1 h", "Snooze until tomorrow 09:00", "Dismiss"];

#[test]
fn a_right_click_on_a_review_requested_item_opens_claim_first_and_no_other_item_has_claim() {
    let fx = rig(vec![review_requested()]);
    let _engine = engine(&fx);
    cockpit(&fx);
    let screen = first_rows(&fx);
    let frames = run(&fx, pick(&screen, "acme/web#5 review requested", 0).into_iter().take(1).collect());
    frame_with(&frames, 0, "the review-requested item's menu", |r| menu_labels(r) == CLAIM_ENTRIES);
    drop(_engine);
    drop(fx);

    let fx = rig(four());
    let _engine = engine(&fx);
    cockpit(&fx);
    let screen = first_rows(&fx);
    let frames = run(&fx, pick(&screen, "l-rel label", 0).into_iter().take(1).collect());
    frame_with(&frames, 0, "the menu of another kind without Claim", |r| menu_labels(r) == ENTRIES);
}

#[test]
fn choosing_claim_runs_inbox_claim_with_the_items_id() {
    let fx = rig(vec![review_requested()]);
    let _engine = engine(&fx);
    cockpit(&fx);
    let screen = first_rows(&fx);
    let frames = run(&fx, pick(&screen, "acme/web#5 review requested", 0));
    let open = frame_with(&frames, 0, "the item's menu", |r| menu_labels(r) == CLAIM_ENTRIES);
    frame_with(&frames, open, "the menu closed", the_menu_is_closed);
    assert_eq!(cpd_calls(&fx), [words(&["inbox", "claim", "1"])]);
    assert!(popups(&fx).is_empty(), "{:?}", popups(&fx));
}

#[test]
fn a_refused_claim_shows_the_failure_dialog_and_esc_closes_the_menu() {
    let fx = rig(vec![review_requested()]);
    let _engine = engine(&fx);
    cockpit(&fx);
    says(&fx, &["inbox", "claim"], Says::Refused(ErrorCode::ItemClosed, "Item 1 is closed."));
    let screen = first_rows(&fx);
    let label = "acme/web#5 review requested";
    let mut signals = pick(&screen, label, 0);
    signals.extend([Signal::Input(right_click(10, row_of(&screen, label))), common::inbox::press(KeyCode::Esc)]);
    let frames = run(&fx, signals);
    let first = frame_with(&frames, 0, "the menu", |r| menu_labels(r) == CLAIM_ENTRIES);
    let closed = frame_with(&frames, first, "the menu closed by Claim", the_menu_is_closed);
    let again = frame_with(&frames, closed, "the menu again", |r| menu_labels(r) == CLAIM_ENTRIES);
    frame_with(&frames, again, "the menu closed by Esc", the_menu_is_closed);
    assert_eq!(cpd_calls(&fx), [words(&["inbox", "claim", "1"])], "Esc runs nothing");
    let commands = commands(&fx);
    let [dialog] = &commands[..] else { panic!("{commands:?}") };
    assert!(dialog.starts_with("cpd tui view dialog text --title="), "a text dialog: {dialog}");
    assert!(dialog.ends_with("--body='item_closed: Item 1 is closed.'"), "the body is why: {dialog}");
}

fn deploy() -> Condition {
    linked(Kind::Deploy, Urgency::Yellow, "deploy:acme/pay#123", "acme/pay deploy production: Release 1.2", None, Some("https://github.com/acme/pay/actions/runs/123"))
}

const APPROVE_ENTRIES: [&str; 5] = ["Approve", "Snooze 15 min", "Snooze 1 h", "Snooze until tomorrow 09:00", "Dismiss"];
const DEPLOY_LABEL: &str = "acme/pay deploy production: Release 1.2";

#[test]
fn a_right_click_on_a_deploy_item_opens_approve_first_and_no_other_item_has_approve() {
    let fx = rig(vec![deploy()]);
    let _engine = engine(&fx);
    cockpit(&fx);
    let screen = first_rows(&fx);
    let frames = run(&fx, pick(&screen, DEPLOY_LABEL, 0).into_iter().take(1).collect());
    frame_with(&frames, 0, "the deploy item's menu", |r| menu_labels(r) == APPROVE_ENTRIES);
    drop(_engine);
    drop(fx);

    let fx = rig(four());
    let _engine = engine(&fx);
    cockpit(&fx);
    let screen = first_rows(&fx);
    let frames = run(&fx, pick(&screen, "l-rel label", 0).into_iter().take(1).collect());
    frame_with(&frames, 0, "the menu of another kind without Approve", |r| menu_labels(r) == ENTRIES);
}

#[test]
fn choosing_approve_runs_inbox_approve_with_the_items_id() {
    let fx = rig(vec![deploy()]);
    let _engine = engine(&fx);
    cockpit(&fx);
    let screen = first_rows(&fx);
    let frames = run(&fx, pick(&screen, DEPLOY_LABEL, 0));
    let open = frame_with(&frames, 0, "the item's menu", |r| menu_labels(r) == APPROVE_ENTRIES);
    frame_with(&frames, open, "the menu closed", the_menu_is_closed);
    assert_eq!(cpd_calls(&fx), [words(&["inbox", "approve", "1"])]);
    assert!(popups(&fx).is_empty(), "{:?}", popups(&fx));
}

#[test]
fn a_refused_approve_shows_the_failure_dialog_and_esc_closes_the_menu() {
    let fx = rig(vec![deploy()]);
    let _engine = engine(&fx);
    cockpit(&fx);
    says(&fx, &["inbox", "approve"], Says::Refused(ErrorCode::GhFailed, "gh failed."));
    let screen = first_rows(&fx);
    let mut signals = pick(&screen, DEPLOY_LABEL, 0);
    signals.extend([Signal::Input(right_click(10, row_of(&screen, DEPLOY_LABEL))), common::inbox::press(KeyCode::Esc)]);
    let frames = run(&fx, signals);
    let first = frame_with(&frames, 0, "the menu", |r| menu_labels(r) == APPROVE_ENTRIES);
    let closed = frame_with(&frames, first, "the menu closed by Approve", the_menu_is_closed);
    let again = frame_with(&frames, closed, "the menu again", |r| menu_labels(r) == APPROVE_ENTRIES);
    frame_with(&frames, again, "the menu closed by Esc", the_menu_is_closed);
    assert_eq!(cpd_calls(&fx), [words(&["inbox", "approve", "1"])], "Esc runs nothing");
    let commands = commands(&fx);
    let [dialog] = &commands[..] else { panic!("{commands:?}") };
    assert!(dialog.starts_with("cpd tui view dialog text --title='inbox approve 1' "), "titled with the command words: {dialog}");
    assert!(dialog.ends_with("--body='gh_failed: gh failed.'"), "the body is why: {dialog}");
}
