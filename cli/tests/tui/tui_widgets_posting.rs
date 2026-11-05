//! A picked Post or Merge on a `pr-ready` Inbox row hides that row at once, client side only (the database is not
//! written), while its command runs; a refused command brings the row back with the error dialog.
//!
//! Built like `tui_widgets_async`: in-process on the driven pump, the command held on a mocked `Gate` (a call of `cpd`
//! on the fake runner; Merge is a held `tmux run-shell` call). Never a real gh, Slack or tmux.

use crate::common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::fx::Fx;
use common::inbox::{W, condition, open, plan_row, seed, session_row, view_fx};
use common::sql;
use common::tree::{Gate, GateKey, Tree, engine, stamp};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::deadline::Failed;
use cpd_core::host::runner::{Call, Handler, Reply, Runner};
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_tui::widgets;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, ExitStatus};

const HEIGHT: u16 = 30;
const URL: &str = "https://github.com/acme/shop/pull/4";
const CODE: &str = "item_closed";
const MESSAGE: &str = "Item is closed.";
const ROW: &str = "shop#4 ready for user review";
const MERGE_ROW: &str = "shop#4 ready to merge";
const OTHER: &str = "f-blocked label";

fn rig(status: &str) -> Fx {
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
    let pr = Condition {
        session: Some(SessionId::parse("l-ship").unwrap()),
        plan: Some(PlanId::parse("ship").unwrap()),
        url: Some(URL.to_owned()),
        ..condition(
            Kind::PrReady,
            Urgency::Yellow,
            "pr:acme/shop#4",
            &format!("shop#4 {status}"),
        )
    };
    let other = condition(
        Kind::Dialog,
        Urgency::Red,
        "session:f-blocked",
        "f-blocked label",
    );
    reconcile(&mut store, &[pr, other], &stamp(&fx, -330)).unwrap();
    sql(
        &mut store,
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at, merge_state)
         VALUES ('acme/shop', 4, 'https://github.com/acme/shop/pull/4', 'A title', 0, 'open', 'none', 'passed',
                 '2026-01-03T00:00:00.000Z', NULL, '2026-02-02T00:00:00.000Z', 'clean');",
    );
    sql(
        &mut store,
        "UPDATE workflows SET status = 'open', pr_number = 4, pr_url = 'https://github.com/acme/shop/pull/4', pr_opened_at = '2026-01-03T00:00:00.000Z',
         pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/shop' AND number = 4) WHERE plan = 'ship';",
    );
    fx
}

/// Every table that an Inbox pick could write, as one comparable text.
fn database(fx: &Fx) -> String {
    open(&fx.env)
        .read(|c| {
            let mut out = Vec::new();
            let mut stmt = c.prepare(
                "SELECT id, kind, urgency, subject, label, closed_at, snoozed_until, dismissed_at, claimed_at FROM inbox_items ORDER BY id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(format!(
                    "{:?}",
                    (0..9).map(|i| r.get::<_, Option<String>>(i).or_else(|_| r.get::<_, i64>(i).map(|n| Some(n.to_string()))).unwrap()).collect::<Vec<_>>()
                ))
            })?;
            for row in rows {
                out.push(row?);
            }
            Ok(out.join("\n"))
        })
        .unwrap()
}

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
    Err(CpdError::new(
        ErrorCode::ItemClosed,
        body["error"]["message"].as_str().expect("a message"),
    ))
}

/// A `tmux` call whose argv has `verb`, held on a gate (the Merge goes through `run-shell`).
#[derive(Clone)]
struct Hold {
    gate: Arc<Gate>,
    verb: &'static str,
    made: Arc<AtomicUsize>,
}

struct Holding {
    inner: Arc<dyn Runner>,
    hold: Hold,
}

impl Holding {
    fn held(&self, call: &Call) -> Option<std::process::Output> {
        let tmux = std::path::Path::new(&call.program).file_name() == Some("tmux".as_ref());
        if !(tmux && call.args.iter().any(|a| a == self.hold.verb)) {
            return None;
        }
        self.hold.made.fetch_add(1, Ordering::SeqCst);
        let _ = self.hold.gate.handle(call);
        Some(std::process::Output {
            status: ExitStatus::from_raw(1 << 8),
            stdout: vec![],
            stderr: b"denied".to_vec(),
        })
    }
}

fn call_of(cmd: &Command, tool: &str, deadline: std::time::Duration) -> Call {
    Call {
        tool: tool.to_owned(),
        program: cmd.get_program().to_string_lossy().into_owned(),
        args: cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect(),
        envs: vec![],
        cwd: None,
        deadline,
        stdin: None,
        stderr_to: None,
    }
}

impl Runner for Holding {
    fn output(
        &self,
        cmd: &mut Command,
        tool: &str,
        deadline: std::time::Duration,
    ) -> Result<std::process::Output, Failed> {
        match self.held(&call_of(cmd, tool, deadline)) {
            Some(out) => Ok(out),
            None => self.inner.output(cmd, tool, deadline),
        }
    }

    fn output_unbounded(
        &self,
        cmd: &mut Command,
        tool: &str,
    ) -> std::io::Result<std::process::Output> {
        match self.held(&call_of(cmd, tool, std::time::Duration::MAX)) {
            Some(out) => Ok(out),
            None => self.inner.output_unbounded(cmd, tool),
        }
    }

    fn run(&self, call: &Call) -> Result<std::process::Output, Failed> {
        match self.held(call) {
            Some(out) => Ok(out),
            None => self.inner.run(call),
        }
    }
}

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

fn log(fx: &Fx) -> String {
    std::fs::read_to_string(fx.env.cpd_home().join("run/tui/widgets.log")).unwrap_or_default()
}

/// Starts the widgets view (with `hold` on a `tmux` verb when given), picks `entry` on `row`, returns the tree.
fn pick(fx: &Fx, hold: Option<Hold>, row: &str, entry: &str) -> Tree {
    let mut tree = Tree::run(
        fx,
        "widgets",
        u16::try_from(W).unwrap(),
        HEIGHT,
        Box::new(move |ports, tty, out, terminal, input| match &hold {
            None => widgets::run_on(ports, through_the_gate, tty, out, terminal, input),
            Some(hold) => {
                let ports = Ports {
                    runner: Arc::new(Holding {
                        inner: ports.runner.clone(),
                        hold: hold.clone(),
                    }),
                    ..ports.clone()
                };
                widgets::run_on(&ports, through_the_gate, tty, out, terminal, input)
            }
        }),
    );
    tree.until_shows(row);
    tree.eager(true);
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
    tree
}

fn cockpit(fx: &Fx) {
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
    fx.tmux_route.fake().add_client("/dev/pts/9", 200, 60);
}

fn no_row(tree: &Tree, row: &str) {
    let screen = tree.screen();
    assert!(
        !screen.iter().any(|r| r.contains(row)),
        "the row of the picked pull request is hidden while its command runs:\n{}",
        screen.join("\n")
    );
    assert!(
        screen.iter().any(|r| r.contains(OTHER)),
        "the other rows stay:\n{}",
        screen.join("\n")
    );
}

#[test]
fn a_picked_post_hides_its_row_while_its_command_runs() {
    let fx = rig("ready for user review");
    let _engine = engine(&fx);
    let key = gate_cpd(&fx);
    let before = database(&fx);
    let tree = pick(&fx, None, ROW, "Post");
    no_row(&tree, ROW);
    assert_eq!(database(&fx), before, "the pick writes nothing");
    key.open();
    drop(tree);
}

#[test]
fn a_picked_merge_hides_its_row_while_its_command_runs() {
    let fx = rig("ready to merge");
    cockpit(&fx);
    let _engine = engine(&fx);
    let (gate, key) = Gate::new(Reply::exit(1, "", "denied"));
    let hold = Hold {
        gate,
        verb: "run-shell",
        made: Arc::new(AtomicUsize::new(0)),
    };
    let before = database(&fx);
    let tree = pick(&fx, Some(hold), MERGE_ROW, "Merge");
    no_row(&tree, MERGE_ROW);
    assert_eq!(database(&fx), before, "the pick writes nothing");
    key.open();
    drop(tree);
}

#[test]
fn a_refused_post_brings_its_row_back_with_the_error_dialog() {
    let fx = rig("ready for user review");
    let _engine = engine(&fx);
    let key = gate_cpd(&fx);
    let before = database(&fx);
    let mut tree = pick(&fx, None, ROW, "Post");
    no_row(&tree, ROW);
    key.open();
    let expect = format!("failed: Post Pull Request: {CODE}: {MESSAGE}");
    tree.until_that(&expect, |_| log(&fx).contains(&expect));
    tree.until_shows(ROW);
    assert_eq!(
        database(&fx),
        before,
        "the refusal leaves the database as it was"
    );
    // the error dialog is the tmux popup of `dialog::text`, asked of the fake tmux
    assert!(
        common::tree::tmux_verbs(&fx)
            .iter()
            .any(|v| v.contains("popup")),
        "the error dialog is open: {:?}",
        common::tree::tmux_verbs(&fx)
    );
}
