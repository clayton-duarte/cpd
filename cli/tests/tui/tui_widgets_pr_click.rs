//! The clicks and the glyph colour of an authored pull request's Inbox row (commands.md §13.2, "Clicks on an Inbox item";
//! pro-d130, pro-d131): the label opens the pull request, the rest of the row opens its Lead, the glyph takes the colour of the
//! row's urgency. The rig is the one of `tui_widgets_inbox_actions.rs` (Plan `ship`, Lead `l-ship`, the view's `run_command`
//! recorded, the fake `cpd`, tmux and `open`).

use crate::common;

use std::cell::RefCell;
use std::ffi::OsString;

use common::fx::Fx;
use common::inbox::{
    World, condition, frame_with, gone, open, plan_row, seed, session_row, show, show_with, view_fx,
};
use common::screen::Screen;
use common::sql;
use common::tree::{engine, stamp};
use cpd_core::error::CpdError;
use cpd_core::host::runner::Reply;
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::output::Output;
use cpd_tui::palette::{self, Meaning};
use cpd_tui::pump::Signal;
use serde_json::json;

/// The client the fake server's `list-clients` names.
const CLIENT: &str = "/dev/pts/9";
const HEIGHT: u16 = 30;
const URL: &str = "https://github.com/acme/shop/pull/4";
/// The glyph of a pull request row.
const GLYPH: &str = "\u{F12F4}";

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

/// The panes the view gave the focus (`select-pane -t <pane>`), in order.
fn focused(fx: &Fx) -> Vec<String> {
    let sent = fx.calls("tmux").into_iter().map(|c| c.args);
    sent.filter_map(|args| {
        args.iter()
            .position(|a| a == "select-pane")
            .map(|at| args[at + 2].clone())
    })
    .collect()
}

/// The argv of every popup the fake server opened.
fn popups(fx: &Fx) -> Vec<Vec<String>> {
    fx.tmux_probe().state().popups()
}

// ---------- the fixtures ----------

/// The Contexts, the Plan `ship` (Lead `l-ship`); then `items`, opened 330 s ago (`5m`).
fn rig(items: Vec<Condition>) -> Fx {
    let fx = view_fx();
    seed(&fx, &[], 330);
    let mut store = open(&fx.env);
    let sessions = [session_row(
        "l-ship",
        "lead",
        "alpha",
        "ship",
        "2026-01-02T00:00:10.000Z",
    )];
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

/// The `pr-ready` row of `number` with `status` as the label's tail, `urgency`, and the Plan `ship`, its Lead and the url when `owned`.
fn pr_row(number: u32, urgency: Urgency, status: &str, owned: bool) -> Condition {
    Condition {
        session: owned.then(|| SessionId::parse("l-ship").unwrap()),
        plan: owned.then(|| PlanId::parse("ship").unwrap()),
        url: Some(format!("https://github.com/acme/shop/pull/{number}")),
        ..condition(
            Kind::PrReady,
            urgency,
            &format!("pr:acme/shop#{number}"),
            &format!("shop#{number} {status}"),
        )
    }
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

/// The first frame the view draws over the store of `fx` with the Inbox section on it.
fn first_frame(fx: &Fx) -> Screen {
    let frames = show(fx, HEIGHT, vec![Signal::Commit, gone()], &World::new());
    let at = frame_with(&frames, 0, "the Inbox section", |r| {
        r.iter().any(|row| row.contains("Inbox"))
    });
    frames[at].clone()
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

/// The first rows of the first frame of `fx`, and the row of `label` on it.
fn locate(fx: &Fx, label: &str) -> (Screen, u16) {
    let frame = first_frame(fx);
    let screen = frame.rows();
    let at = row_of(&screen, label);
    (frame, at)
}

/// The Inbox is pinned to the bottom (one item: the blank row, the header, the ▲ line, the item's one row, the ▼ line): its header is the
/// fourth row from the last.
fn assert_pinned_to_bottom(frame: &Screen) {
    let screen = frame.rows();
    assert_eq!(
        screen.iter().position(|r| r.contains("Inbox")),
        Some(usize::from(HEIGHT) - 4),
        "the Inbox header row on\n{}",
        screen.join("\n")
    );
}

/// A left click on the age cell of row `row`: the last cells of the row, outside the label's link.
fn click_age(screen: &Screen, row: u16) -> Signal {
    let text = screen.shown_row(row);
    let last = text.trim_end().chars().count();
    common::inbox::click(u16::try_from(last - 1).unwrap(), row)
}

/// Runs the view on `signals` (after a first commit) with the fake `cpd` answering `{"ok":true}`.
fn run(fx: &Fx, signals: Vec<Signal>) -> Vec<Screen> {
    RAN.with(|ran| ran.borrow_mut().clear());
    fx.fake("cpd", Reply::ok(r#"{"ok":true}"#));
    let mut all = vec![Signal::Commit];
    all.extend(signals);
    all.push(gone());
    show_with(fx, HEIGHT, all, &World::new(), recording)
}

// ---------- the clicks ----------

#[test]
fn a_click_on_the_row_outside_the_link_opens_the_owning_lead_in_the_chat_slot() {
    let fx = rig(vec![pr_row(4, Urgency::Yellow, "ready to merge", true)]);
    let _engine = engine(&fx);
    let chat = cockpit(&fx);
    let (frame, row) = locate(&fx, "shop#4 ready to merge");
    assert_pinned_to_bottom(&frame);
    run(
        &fx,
        vec![click_age(&frame, row), common::inbox::click(1, row)],
    );
    assert_eq!(
        cpd_calls(&fx),
        [
            words(&["session", "open", "l-ship"]),
            words(&["session", "open", "l-ship"])
        ]
    );
    assert_eq!(
        focused(&fx),
        [chat.clone(), chat],
        "the focus goes to the chat slot after each open"
    );
    assert!(popups(&fx).is_empty(), "{:?}", popups(&fx));
}

#[test]
fn a_click_on_an_unowned_pr_row_outside_the_link_does_nothing() {
    let fx = rig(vec![pr_row(4, Urgency::Yellow, "ready to merge", false)]);
    let _engine = engine(&fx);
    cockpit(&fx);
    let (frame, row) = locate(&fx, "shop#4 ready to merge");
    assert_pinned_to_bottom(&frame);
    run(
        &fx,
        vec![click_age(&frame, row), common::inbox::click(1, row)],
    );
    assert_eq!(cpd_calls(&fx), Vec::<Vec<String>>::new(), "no command");
    assert_eq!(focused(&fx), Vec::<String>::new(), "no select-pane");
    assert!(popups(&fx).is_empty(), "{:?}", popups(&fx));
}

#[test]
fn a_click_on_the_label_of_a_pr_row_opens_its_url_and_runs_no_command() {
    let fx = rig(vec![pr_row(4, Urgency::Yellow, "ready to merge", true)]);
    let _engine = engine(&fx);
    cockpit(&fx);
    fx.fake("open", Reply::ok(""));
    let (_, row) = locate(&fx, "shop#4 ready to merge");
    run(&fx, vec![common::inbox::click(10, row)]);
    let opened: Vec<_> = fx.calls("open").into_iter().map(|c| c.args).collect();
    assert_eq!(opened, [[URL]], "one `open` with the url");
    assert_eq!(cpd_calls(&fx), Vec::<Vec<String>>::new(), "no command");
}

// ---------- the glyph ----------

#[test]
fn the_pr_row_glyph_takes_the_colour_of_its_urgency() {
    let cases = [
        (Urgency::Red, "failing", Meaning::Failed),
        (Urgency::Yellow, "ready to merge", Meaning::Review),
        (Urgency::Cyan, "checks running", Meaning::Notice),
        (Urgency::White, "draft", Meaning::NotStarted),
    ];
    for (n, (urgency, status, meaning)) in (4..).zip(cases) {
        let fx = rig(vec![pr_row(n, urgency, status, true)]);
        let _engine = engine(&fx);
        let label = format!("shop#{n} {status}");
        let (frame, row) = locate(&fx, &label);
        let glyph = frame.cell(1, row);
        assert_eq!(glyph.symbol, GLYPH, "the glyph of {label}");
        assert_eq!(
            glyph.fg,
            palette::color_clickable(meaning),
            "the colour of {label}"
        );
        assert!(
            frame.shown_row(row).contains(&label),
            "the label stays text: {}",
            frame.shown_row(row)
        );
    }
}

#[test]
fn the_merge_spinner_of_a_ready_to_merge_row_stays_yellow() {
    let fx = rig(vec![pr_row(4, Urgency::Yellow, "ready to merge", true)]);
    let _engine = engine(&fx);
    crate::tui_merge::queue_merge_at(&fx.env.cpd_home(), "acme/shop#4");
    let (frame, row) = locate(&fx, "shop#4 ready to merge");
    assert_pinned_to_bottom(&frame);
    let glyph = frame.cell(1, row);
    assert!(
        palette::SPINNER.contains(&glyph.symbol.as_str()),
        "a spinner frame in the glyph cell: {:?}",
        glyph.symbol
    );
    assert_eq!(glyph.fg, palette::color_clickable(Meaning::Review));
}
