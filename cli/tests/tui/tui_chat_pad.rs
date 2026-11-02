//! The one status row (contract §6.1, commands.md §13.2): `status on`, the line in `status-format[0]`, and a 20-row
//! client's window is 19 rows, so the panes reach row 19 and row 20 is the `━` line. The history view leaves the last
//! row of its pane empty (the chat slot's padding).

use crate::common;

use std::cell::RefCell;
use std::fs;
use std::rc::Rc;

use clap::Parser;
use cpd_tui::history;
use serde_json::json;

use common::cockpit::tui;
use common::fx::Fx;
use common::screen::{Bytes, Display, FakeTty, Script, Step, ch, then};

/// `tmux <args>`'s stdout on the fake server; panics when it fails.
fn tmux_out(fx: &Fx, args: &[&str]) -> String {
    let out = fx.tmux(args);
    assert!(
        out.status.success(),
        "tmux {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// The options of the one status row, as the cockpit sets them on the fake server. A 20-row client's window is 19 rows
/// and the lowest pane ends on row 19 because of them (`status on`, no second row): the geometry itself is tmux's, not
/// asserted (research mig-tui-a §5 row 6).
#[test]
fn the_status_line_is_one_row_and_the_panes_reach_the_row_above_it() {
    let fx = common::tree::fx();
    tui(&fx, &[]);

    // The options: one status row, the line in row 0, no row 1.
    assert_eq!(
        tmux_out(&fx, &["show", "-v", "-t", "=cpd:", "status"]).trim(),
        "on"
    );
    assert!(tmux_out(&fx, &["show", "-v", "-t", "=cpd:", "status-format[0]"]).contains("\u{2501}"));
    // The fake server refuses to show an option that was never set, so row 1 is read off the session's own options.
    let set = tmux_out(&fx, &["show", "-t", "=cpd:"]);
    assert!(
        !set.lines().any(|l| l.starts_with("status-format[1]")),
        "no status-format row sits between the panes and the line: {set}"
    );
}

/// The history view in a 40 x 12 terminal of the test process (`history::run_on`: scripted input, a `TestBackend`): its rows
/// as drawn, taken at the end of the conversation and again after `G`.
#[test]
fn the_history_view_leaves_the_last_row_of_its_pane_empty() {
    let fx = Fx::with_ports();
    fx.env.write_config("version = 1\nmachine = \"m1\"\n");
    let path = fx.env.root().join("lead.jsonl");
    let mut text = json!({"type": "session", "id": "l1"}).to_string() + "\n";
    for n in 0..40 {
        text += &json!({"type": "message", "timestamp": "2026-10-15T16:33:44.329Z",
            "message": {"role": "user", "content": [{"type": "text", "text": format!("row {n}")}]}})
        .to_string();
        text.push('\n');
    }
    fs::write(&path, text).unwrap();
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on)
         VALUES ('alpha', 'Title', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at,
                               created_at, archived_at, forked_from)
         VALUES ('l1', 'lead', 'alpha', NULL, NULL, 'old lead', '{}', '/repo', 'O3', NULL, 'stopped', NULL,
                 '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z', '2026-01-04T00:00:00.000Z', 'l1');",
        path.display()
    ));
    let display = Display::new(40, 12);
    let shots = Rc::new(RefCell::new(Vec::<Vec<String>>::new()));
    let take = || {
        let (display, shots) = (display.clone(), shots.clone());
        then(move || shots.borrow_mut().push(display.screen().rows()))
    };
    // the first frame is drawn at the first poll; a step that is a snapshot runs between two polls
    let mut input = Script::new(
        fx.fakes.clock.clone(),
        [take(), Step::Event(ch('G')), take(), Step::Event(ch('q'))],
    );
    let args = history_args(&["l1"]);
    let result = history::run_on(
        &fx.fakes.ports,
        args,
        &mut FakeTty::new(true),
        &mut Bytes::default(),
        display.terminal(),
        &mut input,
    );
    result.unwrap();
    assert_eq!(input.remaining(), 0);
    let check = |rows: &[String]| {
        assert_eq!(rows.len(), 12, "{rows:?}");
        assert_eq!(rows[11], "", "row 12 is empty: {rows:?}");
        assert!(
            !rows[10].is_empty(),
            "the content fills rows 1..11: {rows:?}"
        );
    };
    let shots = shots.borrow();
    assert_eq!(shots.len(), 2);
    for rows in shots.iter() {
        assert!(
            rows.iter().any(|r| r.contains("row 39")),
            "the end of the conversation: {rows:?}"
        );
        check(rows);
    }
}

/// The arguments of `cpd tui view history <argv>`, parsed as the command's parser does.
fn history_args(argv: &[&str]) -> history::Args {
    #[derive(Parser)]
    struct Command {
        #[command(flatten)]
        args: history::Args,
    }
    Command::try_parse_from(std::iter::once("history").chain(argv.iter().copied()))
        .unwrap()
        .args
}
