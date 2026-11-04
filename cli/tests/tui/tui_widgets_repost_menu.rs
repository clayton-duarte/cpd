//! The RePost entry of an authored pull request's Inbox row menu (commands.md §13.2, pro-d386, pro-d388): shown only on a
//! `review overdue` row, in place of Post, and it runs `cpd pr repost --workflow <key> --plan <plan>` at once. The rig is the one of
//! `tui_widgets_pr_menu.rs` (Plan `ship`, Lead `l-ship`, the view's `run_command` recorded, the fake `cpd`, tmux and `open`).

use crate::common;

use std::cell::RefCell;
use std::ffi::OsString;

use common::fx::Fx;
use common::inbox::{
    World, condition, frame_with, gone, open, plan_row, rows, seed, session_row, show, show_with,
    view_fx,
};
use common::screen::{Screen, right_click};
use common::sql;
use common::tree::{engine, stamp};
use cpd_core::error::CpdError;
use cpd_core::host::runner::Reply;
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::output::Output;
use cpd_tui::pump::Signal;
use ratatui::crossterm::event::KeyCode;
use serde_json::json;

const CLIENT: &str = "/dev/pts/9";
const HEIGHT: u16 = 30;
const URL: &str = "https://github.com/acme/shop/pull/4";
const PR_KEY: &str = "repo";
const FOUR: [&str; 4] = [
    "Snooze 15 min",
    "Snooze 1 h",
    "Snooze until tomorrow 09:00",
    "Dismiss",
];

thread_local! {
    /// The words (without the program) of every command the view ran through `run_command`, in order.
    static RAN: RefCell<Vec<Vec<String>>> = const { RefCell::new(Vec::new()) };
}

/// `run_command` of the view under test: records the words, answers `{}`.
fn recording(argv: Vec<OsString>) -> Result<Output, CpdError> {
    let mut words: Vec<String> = argv
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if words.first().is_some_and(|w| w == "cpd") {
        words.remove(0);
    }
    RAN.with(|ran| ran.borrow_mut().push(words));
    Ok(Output::success(json!({})))
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

fn popups(fx: &Fx) -> Vec<Vec<String>> {
    fx.tmux_probe().state().popups()
}

// ---------- the fixtures ----------

/// The Contexts `alpha` and `beta`, the Plan `ship` (Lead `l-ship`), its fork `f-blocked`: no pane in the chat slot; then `items`.
fn rig(items: Vec<Condition>) -> Fx {
    let fx = view_fx();
    seed(&fx, &[], 330);
    let mut store = open(&fx.env);
    let sessions = [
        session_row(
            "l-ship",
            "lead",
            "alpha",
            "ship",
            "2026-01-02T00:00:10.000Z",
        ),
        session_row(
            "f-blocked",
            "lead",
            "alpha",
            "ship",
            "2026-01-02T00:00:02.000Z",
        ),
    ];
    sql(
        &mut store,
        &(plan_row(
            "ship",
            "alpha",
            "dispatch",
            "l-ship",
            "2026-01-02T00:00:10.000Z",
        ) + &sessions.concat()),
    );
    reconcile(&mut store, &items, &stamp(&fx, -330)).unwrap();
    fx
}

/// The cockpit (session `cpd`, window `main`) in the fake server of `fx`: the view and the chat placeholder beside it, and a
/// client; returns the chat pane.
fn cockpit(fx: &Fx) -> String {
    let ok = |args: &[&str]| {
        let out = fx.tmux(args);
        assert!(
            out.status.success(),
            "tmux {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let view = ok(&[
        "new-session",
        "-d",
        "-s",
        "cpd",
        "-n",
        "main",
        "-x",
        "120",
        "-y",
        "40",
        "-P",
        "-F",
        "#{pane_id}",
        "cpd tui view widgets",
    ]);
    ok(&["set", "-p", "-t", &view, "@cpd_view", "widgets"]);
    let chat = ok(&[
        "split-window",
        "-h",
        "-d",
        "-t",
        &view,
        "-P",
        "-F",
        "#{pane_id}",
        "cpd tui view chat",
    ]);
    ok(&["set", "-p", "-t", &chat, "@cpd_view", "chat"]);
    fx.tmux_route.fake().add_client(CLIENT, 200, 60);
    chat
}

/// The rows of the first frame the view draws over the store of `fx`, with the Inbox section on it: where the clicks go.
fn first_rows(fx: &Fx) -> Vec<String> {
    let frames = show(fx, HEIGHT, vec![Signal::Commit, gone()], &World::new());
    let at = frame_with(&frames, 0, "the Inbox section", |r| {
        r.iter().any(|row| row.contains("Inbox"))
    });
    rows(&frames, at)
}

/// The row of the item `label` on `screen`, after the title.
fn row_of(screen: &[String], label: &str) -> u16 {
    let title = screen
        .iter()
        .position(|r| r.contains("Inbox"))
        .expect("the title");
    let at = screen
        .iter()
        .skip(title)
        .position(|r| r.contains(label))
        .unwrap_or_else(|| panic!("{label} on\n{}", screen.join("\n")));
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
    screen[top + 1..]
        .iter()
        .take_while(|r| !r.contains('╯'))
        .filter_map(|r| r.rsplit('│').nth(1))
        .map(|l| l.trim().to_owned())
        .collect()
}

/// The signals of a right click on `label`, `down` presses of Down, and Enter.
fn pick(screen: &[String], label: &str, down: usize) -> Vec<Signal> {
    let mut signals = vec![Signal::Input(right_click(10, row_of(screen, label)))];
    signals.extend((0..down).map(|_| common::inbox::press(KeyCode::Down)));
    signals.push(common::inbox::press(KeyCode::Enter));
    signals
}

// ---------- the rows ----------

/// The pull request row of `number` with `status` as the label's tail: owned by Plan `ship` and its Lead, with `url`.
fn pr_row(number: u32, status: &str, session: Option<&str>, url: Option<&str>) -> Condition {
    Condition {
        session: session.map(|s| SessionId::parse(s).unwrap()),
        plan: session.map(|_| PlanId::parse("ship").unwrap()),
        url: url.map(str::to_owned),
        ..condition(
            Kind::PrReady,
            Urgency::Yellow,
            &format!("pr:acme/shop#{number}"),
            &format!("shop#{number} {status}"),
        )
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
        sql(&mut store, "UPDATE workflows SET status = 'open', pr_number = 4, pr_url = 'https://github.com/acme/shop/pull/4', pr_opened_at = '2026-01-03T00:00:00.000Z',
                        pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/shop' AND number = 4) WHERE plan = 'ship';");
    }
}

/// The menu's labels with the busy spinner of an entry dropped: what the right click on `label` opens.
fn menu_of(fx: &Fx, label: &str) -> Vec<String> {
    let screen = first_rows(fx);
    let frames = run(
        fx,
        vec![Signal::Input(right_click(10, row_of(&screen, label)))],
    );
    let at = frame_with(&frames, 0, "a menu", |r| {
        r.iter().any(|row| row.contains('╮'))
    });
    menu_labels(&rows(&frames, at))
}

/// A rig with the pull request #4 clean, and `items`.
fn shop(items: Vec<Condition>) -> Fx {
    let fx = rig(items);
    pull_request(&fx, 4, "clean");
    fx
}

fn with(first: &[&str]) -> Vec<String> {
    first
        .iter()
        .copied()
        .chain(FOUR)
        .map(str::to_owned)
        .collect()
}

fn repost_menu(status: &str) -> Vec<String> {
    let fx = shop(vec![owned(status)]);
    let _engine = engine(&fx);
    cockpit(&fx);
    menu_of(&fx, &format!("shop#4 {status}"))
}

#[test]
fn an_overdue_pr_row_offers_repost_with_the_cpd_pr_repost_argv() {
    let fx = shop(vec![owned("review overdue")]);
    let _engine = engine(&fx);
    cockpit(&fx);
    assert_eq!(
        menu_of(&fx, "shop#4 review overdue"),
        with(&["RePost", "Go to Lead", "Copy Link"]),
        "RePost first, no Post"
    );
    drop(_engine);
    drop(fx);

    let fx = shop(vec![owned("review overdue")]);
    let _engine = engine(&fx);
    cockpit(&fx);
    let screen = first_rows(&fx);
    run(&fx, pick(&screen, "shop#4 review overdue", 0));
    assert_eq!(
        cpd_calls(&fx),
        [words(&[
            "pr",
            "repost",
            "--workflow",
            PR_KEY,
            "--plan",
            "ship"
        ])]
    );
    assert!(popups(&fx).is_empty(), "{:?}", popups(&fx));
}

#[test]
fn a_pr_row_that_is_not_overdue_offers_no_repost() {
    // the control: the menu does name RePost on an overdue row
    assert_eq!(repost_menu("review overdue")[0], "RePost");
    for status in [
        "ready for user review",
        "ready to merge",
        "waiting",
        "draft",
    ] {
        let labels = repost_menu(status);
        assert!(
            !labels.iter().any(|l| l == "RePost"),
            "{status}: {labels:?}"
        );
    }
}
