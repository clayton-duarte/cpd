//! The right-click menu of an authored pull request's Inbox row and what its entries run (commands.md §13.2, "The Inbox's
//! right-click menu", pro-d130, pro-d131): `Merge` or `Post` by the label's status, `Go to Lead`, `Copy Link`, then the four
//! choices. The rig is the one of `tui_widgets_inbox_actions.rs` (Plan `ship`, Lead `l-ship`, the view's `run_command` recorded,
//! the fake `cpd`, tmux and `open`). The status is the label's tail, never the `pull_requests` row.

use crate::common;

use std::cell::RefCell;
use std::ffi::OsString;

use common::fx::Fx;
use common::inbox::{World, condition, frame_with, gone, open, plan_row, rows, seed, session_row, show, show_with, view_fx};
use common::screen::{Screen, right_click};
use common::sql;
use common::tree::{engine, stamp};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::runner::Reply;
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::output::Output;
use cpd_tui::palette;
use cpd_tui::pump::Signal;
use ratatui::crossterm::event::KeyCode;
use serde_json::json;

/// The client the fake server's `list-clients` names.
const CLIENT: &str = "/dev/pts/9";
const HEIGHT: u16 = 30;
const URL: &str = "https://github.com/acme/shop/pull/4";
/// The key the fixture's `plan_row` gives the Plan's `prs` row.
const PR_KEY: &str = "repo";
const FOUR: [&str; 4] = ["Snooze 15 min", "Snooze 1 h", "Snooze until tomorrow 09:00", "Dismiss"];

thread_local! {
    /// The words (without the program) of every command the view ran through `run_command`, in order.
    static RAN: RefCell<Vec<Vec<String>>> = const { RefCell::new(Vec::new()) };
    /// What `run_command` answers to a command starting with the words: a refusal `(code, message)`.
    static REFUSES: RefCell<Vec<(Vec<String>, ErrorCode, &'static str)>> = const { RefCell::new(Vec::new()) };
}

/// `run_command` of the view under test: records the words, answers `{}` or the scripted refusal.
fn recording(argv: Vec<OsString>) -> Result<Output, CpdError> {
    let mut words: Vec<String> = argv.iter().map(|a| a.to_string_lossy().into_owned()).collect();
    if words.first().is_some_and(|w| w == "cpd") {
        words.remove(0);
    }
    RAN.with(|ran| ran.borrow_mut().push(words.clone()));
    let refused = REFUSES.with(|r| r.borrow().iter().find(|(prefix, ..)| words.starts_with(prefix)).map(|(_, code, message)| (*code, *message)));
    match refused {
        None => Ok(Output::success(json!({}))),
        Some((code, message)) => Err(CpdError::new(code, message)),
    }
}

fn refuse(prefix: &[&str], code: ErrorCode, message: &'static str) {
    REFUSES.with(|r| r.borrow_mut().push((prefix.iter().map(|w| (*w).to_owned()).collect(), code, message)));
}

/// Every command of `cpd` the view ran, without the program, in order: through `run_command`, then through the runner.
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

/// The Contexts `alpha` and `beta`, the Plan `ship` (Lead `l-ship`), its fork `f-blocked`: no pane in the chat slot; then `items`.
fn rig(items: Vec<Condition>) -> Fx {
    let fx = view_fx();
    seed(&fx, &[], 330);
    let mut store = open(&fx.env);
    let sessions = [
        session_row("l-ship", "lead", "alpha", "ship", "2026-01-02T00:00:10.000Z"),
        session_row("f-blocked", "lead", "alpha", "ship", "2026-01-02T00:00:02.000Z"),
    ];
    sql(&mut store, &(plan_row("ship", "alpha", "dispatch", "l-ship", "2026-01-02T00:00:10.000Z") + &sessions.concat()));
    reconcile(&mut store, &items, &stamp(&fx, -330)).unwrap();
    fx
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

/// The signals of a right click on `label`, `down` presses of Down, and Enter.
fn pick(screen: &[String], label: &str, down: usize) -> Vec<Signal> {
    let mut signals = vec![Signal::Input(right_click(10, row_of(screen, label)))];
    signals.extend((0..down).map(|_| common::inbox::press(KeyCode::Down)));
    signals.push(common::inbox::press(KeyCode::Enter));
    signals
}


// ---------- the rows ----------

/// `condition`, linked to `session` and the Plan `ship`, with `url`.
fn linked(kind: Kind, urgency: Urgency, subject: &str, label: &str, session: Option<&str>, url: Option<&str>) -> Condition {
    Condition {
        session: session.map(|s| SessionId::parse(s).unwrap()),
        plan: Some(PlanId::parse("ship").unwrap()),
        url: url.map(str::to_owned),
        ..condition(kind, urgency, subject, label)
    }
}

/// The pull request row of `number` with `status` as the label's tail: owned by Plan `ship` and its Lead, with `url`.
fn pr_row(number: u32, status: &str, session: Option<&str>, url: Option<&str>) -> Condition {
    Condition {
        session: session.map(|s| SessionId::parse(s).unwrap()),
        plan: session.map(|_| PlanId::parse("ship").unwrap()),
        url: url.map(str::to_owned),
        ..condition(Kind::PrReady, Urgency::Yellow, &format!("pr:acme/shop#{number}"), &format!("shop#{number} {status}"))
    }
}

/// The row of `status` for `acme/shop#4`, owned, with its url.
fn owned(status: &str) -> Condition {
    pr_row(4, status, Some("l-ship"), Some(URL))
}

/// The `pull_requests` row `acme/shop#number`, with `merge_state`, and (for #4) the Plan `ship`'s `prs` row pointing at it.
fn pull_request(fx: &Fx, number: u32, merge_state: &str) {
    let mut store = open(&fx.env);
    sql(
        &mut store,
        &format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at, merge_state)
             VALUES ('acme/shop', {number}, 'https://github.com/acme/shop/pull/{number}', 'A title', 0, 'open', 'none', 'passed',
                     '2026-01-03T00:00:00.000Z', NULL, '2026-02-02T00:00:00.000Z', '{merge_state}');"
        ),
    );
    if number == 4 {
        sql(&mut store, "UPDATE prs SET status = 'open', pr_number = 4, pr_url = 'https://github.com/acme/shop/pull/4', pr_opened_at = '2026-01-03T00:00:00.000Z',
                        pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/shop' AND number = 4) WHERE plan = 'ship';");
    }
}

/// The menu's labels with the busy spinner of an entry dropped: what the right click on `label` opens.
fn menu_of(fx: &Fx, label: &str) -> Vec<String> {
    let screen = first_rows(fx);
    let frames = run(fx, vec![Signal::Input(right_click(10, row_of(&screen, label)))]);
    let at = frame_with(&frames, 0, "a menu", |r| r.iter().any(|row| row.contains('╮')));
    menu_labels(&rows(&frames, at))
}

/// A rig with the pull request #4 clean, and `items`.
fn shop(items: Vec<Condition>) -> Fx {
    let fx = rig(items);
    pull_request(&fx, 4, "clean");
    fx
}

fn with(first: &[&str]) -> Vec<String> {
    first.iter().copied().chain(FOUR).map(str::to_owned).collect()
}

// ---------- the entries ----------

#[test]
fn the_menu_of_a_ready_to_merge_row_is_merge_go_to_lead_copy_link_then_the_four_choices() {
    let fx = shop(vec![owned("ready to merge")]);
    let _engine = engine(&fx);
    cockpit(&fx);
    assert_eq!(menu_of(&fx, "shop#4 ready to merge"), with(&["Merge", "Go to Lead", "Copy Link"]));
}

#[test]
fn the_menu_of_a_ready_for_review_row_starts_with_post_then_go_to_lead_and_copy_link() {
    let fx = shop(vec![owned("ready for user review")]);
    let _engine = engine(&fx);
    cockpit(&fx);
    assert_eq!(menu_of(&fx, "shop#4 ready for user review"), with(&["Post", "Go to Lead", "Copy Link"]));
}

#[test]
fn an_unowned_row_has_no_go_to_lead_and_a_row_without_a_url_has_no_copy_link() {
    let fx = shop(vec![pr_row(4, "ready to merge", None, Some(URL))]);
    let _engine = engine(&fx);
    cockpit(&fx);
    assert_eq!(menu_of(&fx, "shop#4 ready to merge"), with(&["Merge", "Copy Link"]));
    drop(_engine);
    drop(fx);

    let fx = shop(vec![pr_row(4, "failing", Some("l-ship"), None)]);
    let _engine = engine(&fx);
    cockpit(&fx);
    assert_eq!(menu_of(&fx, "shop#4 failing"), with(&["Go to Lead"]));
}

#[test]
fn a_broken_waiting_or_draft_row_offers_neither_merge_nor_post() {
    for (number, status) in [(4, "failing"), (5, "checks running"), (6, "draft"), (7, "waiting")] {
        let fx = shop(vec![pr_row(number, status, Some("l-ship"), Some(URL))]);
        let _engine = engine(&fx);
        cockpit(&fx);
        assert_eq!(menu_of(&fx, &format!("shop#{number} {status}")), with(&["Go to Lead", "Copy Link"]), "{status}");
    }
    let fx = shop(vec![linked(Kind::Dialog, Urgency::Red, "session:f-blocked", "f-blocked label", Some("f-blocked"), None)]);
    let _engine = engine(&fx);
    cockpit(&fx);
    assert_eq!(menu_of(&fx, "f-blocked label"), FOUR, "another kind keeps its old menu");
}

#[test]
fn merge_and_post_come_from_the_label_status_not_from_the_pull_request_row() {
    let fx = rig(vec![owned("ready to merge")]);
    pull_request(&fx, 4, "blocked");
    let _engine = engine(&fx);
    cockpit(&fx);
    assert_eq!(menu_of(&fx, "shop#4 ready to merge"), with(&["Merge", "Go to Lead", "Copy Link"]), "blocked, labelled ready to merge");
    drop(_engine);
    drop(fx);

    let fx = shop(vec![owned("ready for user review")]);
    let _engine = engine(&fx);
    cockpit(&fx);
    assert_eq!(menu_of(&fx, "shop#4 ready for user review"), with(&["Post", "Go to Lead", "Copy Link"]), "clean, labelled ready for user review");
}

// ---------- what the entries run ----------

#[test]
fn picking_merge_runs_the_merge_action_at_once_with_no_confirm_popup() {
    let fx = shop(vec![owned("ready to merge")]);
    fx.fake("sh", Reply::ok(""));
    let _engine = engine(&fx);
    cockpit(&fx);
    let screen = first_rows(&fx);
    run(&fx, pick(&screen, "shop#4 ready to merge", 0));
    let shells: Vec<String> = fx
        .calls("tmux")
        .into_iter()
        .map(|c| c.args)
        .filter(|args| args.iter().any(|a| a == "run-shell"))
        .map(|args| args.last().unwrap().clone())
        .collect();
    assert_eq!(shells, [format!("cpd tui action merge 'acme/shop##4' --client '{CLIENT}' 2>/dev/null")]);
    assert!(popups(&fx).is_empty(), "no confirm popup: {:?}", popups(&fx));
}

#[test]
fn picking_post_runs_cpd_pr_post_with_the_pr_key_and_plan_and_opens_no_popup() {
    let fx = shop(vec![owned("ready for user review")]);
    let _engine = engine(&fx);
    cockpit(&fx);
    let screen = first_rows(&fx);
    run(&fx, pick(&screen, "shop#4 ready for user review", 0));
    assert_eq!(cpd_calls(&fx), [words(&["pr", "post", "--pr", PR_KEY, "--plan", "ship"])]);
    assert!(popups(&fx).is_empty(), "{:?}", popups(&fx));
}

#[test]
fn a_refused_post_opens_the_error_dialog_and_a_post_with_no_matching_pr_row_says_so() {
    let fx = shop(vec![owned("ready for user review")]);
    let _engine = engine(&fx);
    cockpit(&fx);
    refuse(&["pr", "post"], ErrorCode::WorkflowNotReady, "The pull request is not ready.");
    let screen = first_rows(&fx);
    run(&fx, pick(&screen, "shop#4 ready for user review", 0));
    let opened = commands(&fx);
    let [dialog] = &opened[..] else { panic!("{opened:?}") };
    assert!(dialog.starts_with("cpd tui view dialog text --title="), "a text dialog: {dialog}");
    assert!(dialog.contains("Post Pull Request"), "the title: {dialog}");
    assert!(dialog.ends_with("--body='pr_not_ready: The pull request is not ready.'"), "the body is why: {dialog}");
    drop(_engine);
    drop(fx);

    let fx = rig(vec![owned("ready for user review")]);
    let _engine = engine(&fx);
    cockpit(&fx);
    let screen = first_rows(&fx);
    run(&fx, pick(&screen, "shop#4 ready for user review", 0));
    assert_eq!(cpd_calls(&fx), Vec::<Vec<String>>::new(), "no `pr post` without a matching PR row");
    let opened = commands(&fx);
    let [dialog] = &opened[..] else { panic!("{opened:?}") };
    assert!(dialog.starts_with("cpd tui view dialog text --title="), "a failure dialog: {dialog}");
    assert!(dialog.to_lowercase().contains("no pr of plan ship points at acme/shop#4"), "{dialog}");
}

#[test]
fn a_live_merge_marker_shows_the_merging_entry_busy() {
    let fx = shop(vec![owned("ready to merge")]);
    let _engine = engine(&fx);
    cockpit(&fx);
    crate::tui_merge::put_marker_at(&fx.env.cpd_home(), "acme__shop__4.json", fx.live_pid());
    let screen = first_rows(&fx);
    let glyph = screen.iter().find(|r| r.contains("shop#4 ready to merge")).and_then(|r| r.chars().nth(1)).map(String::from);
    assert!(glyph.as_ref().is_some_and(|g| palette::SPINNER.contains(&g.as_str())), "a spinner frame in the glyph cell: {screen:?}");
    let frames = run(&fx, vec![Signal::Input(right_click(10, row_of(&screen, "shop#4 ready to merge")))]);
    let at = frame_with(&frames, 0, "a menu", |r| r.iter().any(|row| row.contains('╮')));
    let labels = menu_labels(&rows(&frames, at));
    assert_eq!(labels.len(), 7, "{labels:?}");
    let busy = labels[0].strip_prefix("Merge").unwrap_or_else(|| panic!("{labels:?}"));
    assert!(busy.is_empty() || palette::SPINNER.iter().any(|s| busy.trim() == *s), "Merge drawn busy: {labels:?}");
    assert_eq!(labels[1..], with(&["Go to Lead", "Copy Link"]), "{labels:?}");
    // Enter and a click on the busy entry pick nothing
    let picked = run(&fx, pick(&screen, "shop#4 ready to merge", 0));
    let _ = picked;
    let shells = fx.calls("tmux").into_iter().filter(|c| c.args.iter().any(|a| a == "run-shell")).count();
    assert_eq!((shells, popups(&fx).len(), cpd_calls(&fx).len()), (0, 0, 0), "the busy entry picks nothing");
}

#[test]
fn go_to_lead_and_copy_link_run_their_commands() {
    let fx = shop(vec![owned("ready to merge")]);
    let _engine = engine(&fx);
    let chat = cockpit(&fx);
    let screen = first_rows(&fx);
    let mut signals = pick(&screen, "shop#4 ready to merge", 1);
    signals.extend(pick(&screen, "shop#4 ready to merge", 2));
    run(&fx, signals);
    assert_eq!(cpd_calls(&fx), [words(&["session", "open", "l-ship"])]);
    assert_eq!(focused(&fx), [chat]);
    assert_eq!(copies(&fx), [URL]);
}
