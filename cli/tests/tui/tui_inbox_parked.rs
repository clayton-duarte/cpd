//! The `drafts-parked` Inbox item in the cockpit (commands.md §13.2 "Clicks on an Inbox item", contract §2.2 `drafts-parked`;
//! hs-d731, hs-d733): its row is magenta like the other magenta kinds, with its label, its dot counts, and a click on it
//! jumps to the Plan's Lead (as `plan-review` with no url). The view runs in this process on a script (`common/inbox.rs`), with
//! the fake tmux server and the fake `cpd` of the `FakeRunner`; the label and the colour are the only TUI change.

use crate::common;

use std::cell::RefCell;
use std::ffi::OsString;

use common::fx::Fx;
use common::inbox::{
    World, frame_with, gone, open, plan_row, rows, seed, session_row, show, show_with, view_fx,
};
use common::sql;
use common::tree::{engine, stamp};
use cpd_core::error::CpdError;
use cpd_core::host::runner::Reply;
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::output::Output;
use cpd_tui::palette;
use cpd_tui::pump::Signal;
use serde_json::json;

const CLIENT: &str = "/dev/pts/9";
const HEIGHT: u16 = 30;
const LABEL: &str = "ship: 2 parked drafts";

thread_local! {
    /// The words (without the program) of every command the view ran through `run_command`, in order.
    static RAN: RefCell<Vec<Vec<String>>> = const { RefCell::new(Vec::new()) };
}

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

/// Every command of `cpd` the view ran, without the program: what went through `run_command`, then through the runner.
fn cpd_calls(fx: &Fx) -> Vec<Vec<String>> {
    let mut all = RAN.with(|ran| ran.borrow().clone());
    all.extend(fx.calls("cpd").into_iter().map(|call| call.args));
    all
}

/// What E8 opens for the Plan `ship`: magenta, linked to the Plan and its Lead `l-ship`.
fn parked() -> Condition {
    Condition {
        kind: Kind::DraftsParked,
        urgency: Urgency::Magenta,
        subject: "drafts-parked:ship".to_owned(),
        label: LABEL.to_owned(),
        url: None,
        context: Some(cpd_core::ids::ContextId::parse("alpha").unwrap()),
        plan: Some(PlanId::parse("ship").unwrap()),
        job: None,
        session: Some(SessionId::parse("l-ship").unwrap()),
        record: None,
        event: None,
        opened_at: None,
        head_oid: None,
        reviewed_oid: None,
    }
}

/// The Plan `ship` (Lead `l-ship`, no pane) and the item open for 330 s; the cockpit with a client and the fake `cpd`.
fn rig() -> Fx {
    let fx = view_fx();
    seed(&fx, &[], 330);
    let mut store = open(&fx.env);
    sql(
        &mut store,
        &(plan_row(
            "ship",
            "alpha",
            "dispatch",
            "l-ship",
            "2026-01-02T00:00:10.000Z",
        ) + &session_row(
            "l-ship",
            "lead",
            "alpha",
            "ship",
            "2026-01-02T00:00:10.000Z",
        )),
    );
    reconcile(&mut store, &[parked()], &stamp(&fx, -330)).unwrap();
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
    fx
}

fn first_rows(fx: &Fx) -> Vec<String> {
    let frames = show(fx, HEIGHT, vec![Signal::Commit, gone()], &World::new());
    let at = frame_with(&frames, 0, "the item's row", |r| {
        r.iter().any(|row| row.contains(LABEL))
    });
    rows(&frames, at)
}

#[test]
fn draws_magenta_with_its_label() {
    let fx = rig();
    let _engine = engine(&fx);
    let screen = first_rows(&fx);
    let row = screen.iter().find(|r| r.contains(LABEL)).expect("the row");
    assert!(
        row.contains(palette::TIER_MAGENTA),
        "the magenta tier glyph on the row: {row:?}"
    );
    assert!(row.contains("5m"), "its age: {row:?}");
    let magenta = palette::tier_span(Urgency::Magenta);
    assert_eq!(magenta.content, palette::TIER_MAGENTA);
    assert_eq!(
        palette::item_span(Kind::DraftsParked, Urgency::Magenta).content,
        palette::item_span(Kind::PlanReview, Urgency::Magenta).content,
        "the glyph of the other magenta kinds"
    );

    // Its dot counts: the Lead the item is linked to shows magenta.
    let tree = cpd_tui::read::tree(&mut open(&fx.env)).unwrap();
    let lead = tree
        .contexts
        .iter()
        .flat_map(|c| &c.plans)
        .flat_map(|p| &p.sessions)
        .find(|n| n.id.as_str() == "l-ship")
        .expect("the Lead");
    assert_eq!(
        lead.dot,
        cpd_tui::read::Dot::Magenta,
        "the item fills the Lead's dot"
    );
}

#[test]
fn click_jumps_to_the_lead() {
    let fx = rig();
    let _engine = engine(&fx);
    let screen = first_rows(&fx);
    let row = screen
        .iter()
        .position(|r| r.contains(LABEL))
        .expect("the row");
    fx.fake("cpd", Reply::ok(r#"{"ok":true}"#));
    RAN.with(|ran| ran.borrow_mut().clear());
    let signals = vec![
        Signal::Commit,
        common::inbox::click(1, u16::try_from(row).unwrap()),
        gone(),
    ];
    show_with(&fx, HEIGHT, signals, &World::new(), recording);
    assert_eq!(
        cpd_calls(&fx),
        [vec![
            "session".to_owned(),
            "open".to_owned(),
            "l-ship".to_owned()
        ]],
        "the Lead of the Plan opens, as for a plan-review with no url"
    );
}
