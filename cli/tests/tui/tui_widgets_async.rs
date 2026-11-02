//! The Inbox menu's picks run off the `widgets` view's thread (commands.md §13.2, "Inbox actions run off the view's thread",
//! pro-d350): a pick closes the menu and the view draws the next frame without it BEFORE the command runs to its end; the
//! command runs on the view's `Background`, while the view goes on drawing and reading input; when it ends the view reads
//! the Inbox again at once and a failure is logged (`failed: <title>: <why>`, `run/tui/widgets.log`).
//!
//! Each test is in-process on the driven pump (`Tree::run`, `widgets::run_on`: its `Background` runs a job on a real thread,
//! which a scripted pump cannot show: it runs jobs inline and so cannot see the order). One call of the command is held on
//! a `Gate` that the test opens; the loop is handed to the test while the command waits (`Tree::eager`). There is no timer,
//! no sleep and no timeout here: the gate is the only wait.

use crate::common;

use std::ffi::OsString;

use common::fx::Fx;
use common::inbox::{W, condition, open, plan_row, seed, session_row, view_fx};
use common::tree::{Gate, GateKey, Tree, engine, stamp};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::runner::{Call, Reply};
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_tui::widgets;
use serde_json::{Value, json};

const HEIGHT: u16 = 30;
const URL: &str = "https://github.com/acme/shop/pull/4";
/// What the gated call answers: the code and the message of the cpd error the failure line carries.
const CODE: &str = "item_closed";
const MESSAGE: &str = "Item is closed.";
/// The labels of the Inbox's menu entries (the tests' "no menu" check); the next Job adds `Post`, `Merge` and the like.
const ENTRIES: [&str; 7] = [
    "Dismiss",
    "Go to Lead",
    "Copy Link",
    "Snooze 15 min",
    "Snooze 1 h",
    "Snooze until tomorrow 09:00",
    "Approve",
];
/// The row under test, and a second row (a `dialog` item, whose menu is the four choices) to right click while the command waits.
const ROW: &str = "shop#4 ready to merge";
const OTHER: &str = "f-blocked label";

// ---------- the fixture ----------

/// The Contexts `alpha` and `beta`, the Plan `ship` (Lead `l-ship`) and `items` in the Inbox. The next Job reuses it with
/// its own `items` (a `Post` row, an unowned row, a row with a url).
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
    let plan = plan_row(
        "ship",
        "alpha",
        "dispatch",
        "l-ship",
        "2026-01-02T00:00:10.000Z",
    );
    common::sql(&mut store, &(plan + &sessions.concat()));
    reconcile(&mut store, &items, &stamp(&fx, -330)).unwrap();
    fx
}

/// The `pr-ready` row of `acme/shop#4` with `status` as the label's tail, owned (`l-ship`, `ship`) or not.
fn pr_row(status: &str, owned: bool) -> Condition {
    Condition {
        session: owned.then(|| SessionId::parse("l-ship").unwrap()),
        plan: owned.then(|| PlanId::parse("ship").unwrap()),
        url: Some(URL.to_owned()),
        ..condition(
            Kind::PrReady,
            Urgency::Yellow,
            "pr:acme/shop#4",
            &format!("shop#4 {status}"),
        )
    }
}

/// The second row: a `dialog` item.
fn other() -> Condition {
    condition(
        Kind::Dialog,
        Urgency::Red,
        "session:f-blocked",
        "f-blocked label",
    )
}

/// The id of the Inbox item whose label is `label`.
fn id_of(fx: &Fx, label: &str) -> i64 {
    open(&fx.env)
        .read(|c| {
            Ok(c.query_row(
                "SELECT id FROM inbox_items WHERE label = ?1",
                [label],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

// ---------- the gate ----------

/// Holds every call of `cpd` on a gate that answers a failure (`CODE`, `MESSAGE`, exit 3) once the key opens it. The next Job
/// holds a call of `tmux` (`set-buffer`, `run-shell`) the same way, with a small `Runner` wrapper that waits on a `Gate` for it;
/// the entry then passes the view `Ports { runner: Arc::new(wrapper), ..ports.clone() }`.
fn gate_cpd(fx: &Fx) -> GateKey {
    let (gate, key) = Gate::new(Reply::exit(
        3,
        &json!({"ok": false, "error": {"code": CODE, "message": MESSAGE, "details": {}}})
            .to_string(),
        "",
    ));
    let _ = (*fx.fakes.runner).clone().route("cpd", gate);
    key
}

/// The view's `run_command`: runs the command as a call of `cpd` on the ports of the job (the fake runner, where the gate
/// holds it), turns a failure into its `CpdError` and a success into `{}`. The next Job's Post uses it as it is.
fn through_the_gate(argv: Vec<OsString>) -> Result<Output, CpdError> {
    let mut words: Vec<String> = argv
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if words.first().is_some_and(|w| w == "cpd") {
        words.remove(0);
    }
    let call = Call {
        tool: "cpd".to_owned(),
        program: "cpd".to_owned(),
        args: words,
        envs: vec![],
        cwd: None,
        deadline: std::time::Duration::from_secs(60),
        stdin: None,
        stderr_to: None,
    };
    let out = Ports::current()
        .runner
        .run(&call)
        .map_err(|e| CpdError::new(ErrorCode::Internal, format!("{e:?}")))?;
    if out.status.success() {
        return Ok(Output::success(json!({})));
    }
    let body: Value = serde_json::from_slice(&out.stdout).expect("the gate's cpd error JSON");
    assert_eq!(body["error"]["code"], CODE);
    Err(CpdError::new(
        ErrorCode::ItemClosed,
        body["error"]["message"].as_str().expect("a message"),
    ))
}

// ---------- the steps ----------

/// `widgets.log` of the cpd home of `fx` (the file `dialog::report` appends `failed: ...` to).
fn log(fx: &Fx) -> String {
    std::fs::read_to_string(fx.env.cpd_home().join("run/tui/widgets.log")).unwrap_or_default()
}

/// The labels of the menu box on `screen`: the `│ <label> │` rows between the `╮` and the `╯` rows.
fn menu_labels(screen: &[String]) -> Vec<String> {
    let Some(top) = screen.iter().position(|r| r.contains('╮')) else {
        return vec![];
    };
    screen[top + 1..]
        .iter()
        .take_while(|r| !r.contains('╯'))
        .filter_map(|r| r.rsplit('│').nth(1))
        .map(|l| l.trim().to_owned())
        .collect()
}

/// The row of `label` on `screen`, after the title.
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

/// Whether `screen` has a menu box (a `╮` or a `╯` row) or an entry of a menu.
fn menu_is_open(screen: &[String]) -> bool {
    screen
        .iter()
        .any(|r| r.contains('╮') || r.contains('╯') || ENTRIES.iter().any(|e| r.contains(e)))
}

/// The one step list of every test of the file (the next Job's Post, Copy Link, Merge and clicks call it too, with their
/// `row`, `entry`, `gated` call and `title`): the widgets view on `fx`; a pick of `entry` in the menu of `row` returns while its
/// command waits on the gate `key` holds; the menu is gone and the view goes on (another row's menu opens and closes); then
/// the gate opens, the failure `failed: <title>: ` is logged, the gated call was made once (`gated` says whether a call of
/// `cpd` is the one) and the Inbox was read again.
fn off_the_view_thread(
    fx: &Fx,
    key: GateKey,
    run_command: cpd_tui::RunCommand,
    row: &str,
    entry: &str,
    gated: impl Fn(&Call) -> bool,
    title: &str,
) {
    let mut tree = Tree::run(
        fx,
        "widgets",
        u16::try_from(W).unwrap(),
        HEIGHT,
        Box::new(move |ports, tty, out, terminal, input| {
            widgets::run_on(ports, run_command, tty, out, terminal, input)
        }),
    );
    let key = key; // dropped before `tree`: a view that waits on a command that waits on the gate cannot end
    tree.until_shows(row);
    tree.eager(true);

    // (2) the menu of the row, and the pick of the entry
    tree.right_click(10, row_of(&tree.screen(), row));
    let screen = tree.screen();
    let labels = menu_labels(&screen);
    let down = labels
        .iter()
        .position(|l| l == entry)
        .unwrap_or_else(|| panic!("{entry} in the menu {labels:?} on\n{}", screen.join("\n")));
    for _ in 0..down {
        tree.keys(&["Down"]);
    }
    tree.keys(&["Enter"]); // returns while the command waits on its gate

    // (3) the next frame has no menu, and nothing has failed yet
    let screen = tree.screen();
    assert!(
        !menu_is_open(&screen),
        "the menu is closed before the command ends:\n{}",
        screen.join("\n")
    );
    assert!(
        screen.iter().any(|r| r.contains(row)) && screen.iter().any(|r| r.contains(OTHER)),
        "the Inbox rows are still there:\n{}",
        screen.join("\n")
    );
    assert!(!log(fx).contains("failed:"), "{}", log(fx));

    // (4) the view reads input and draws while the command waits
    tree.right_click(10, row_of(&tree.screen(), OTHER));
    let screen = tree.screen();
    assert!(
        !menu_labels(&screen).is_empty(),
        "the other row's menu is drawn while the command runs:\n{}",
        screen.join("\n")
    );
    tree.keys(&["Escape"]);
    let screen = tree.screen();
    assert!(
        !menu_is_open(&screen),
        "Escape closed it:\n{}",
        screen.join("\n")
    );

    // (5) the command ends: the failure is logged and the Inbox is read again
    let before = tree.snapshots();
    key.open();
    let line = format!("failed: {title}: {CODE}: {MESSAGE}");
    tree.until_that(&line, |_| log(fx).contains(&line));
    let made = fx.calls("cpd").into_iter().filter(&gated).count();
    assert_eq!(made, 1, "the gated call was made once");
    assert!(
        tree.snapshots() > before,
        "the Inbox was read again once the job ended"
    );
}

// ---------- the tests ----------

#[test]
fn a_picked_choice_closes_the_menu_and_the_view_goes_on_while_its_command_runs() {
    let fx = rig(vec![pr_row("ready to merge", false), other()]);
    let _engine = engine(&fx);
    let id = id_of(&fx, ROW);
    let key = gate_cpd(&fx);
    off_the_view_thread(
        &fx,
        key,
        through_the_gate,
        ROW,
        "Dismiss",
        |call| call.args.iter().any(|a| a == "dismiss"),
        &format!("inbox dismiss {id}"),
    );
}

#[test]
fn a_picked_go_to_lead_closes_the_menu_and_the_view_goes_on_while_its_command_runs() {
    let fx = rig(vec![pr_row("ready to merge", true), other()]);
    let _engine = engine(&fx);
    let key = gate_cpd(&fx);
    off_the_view_thread(
        &fx,
        key,
        through_the_gate,
        ROW,
        "Go to Lead",
        |call| {
            call.args
                .starts_with(&["session".to_owned(), "open".to_owned()])
        },
        "session open l-ship",
    );
}
