//! `cpd tui view history` (commands.md §13.2, §17): the frame drawn over a prepared store and a fixture pi session
//! file (on a `TestBackend`, in this process), the refusals before drawing (`run_with`, in this process, `Fx`) and the
//! keys: the view loop (`cpd_tui::show_history_with`) in this process on a `SharedBackend`, the scripted input
//! (`common::screen::Script`) and the `ManualClock`. Rendering and scrolling are unit-tested in `tui/src/history/` and
//! `tui/src/worker/`.

use crate::common;

use std::path::{Path, PathBuf};

use common::TestEnv;
use common::fx::Fx;
use common::screen::{Screen, Script, SharedBackend, Step};
use cpd_tui::view_ports::Ctx;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serde_json::json;

// ---------- the prepared store ----------

fn env() -> TestEnv {
    let env = TestEnv::new();
    configure(&env);
    env
}

fn configure(env: &TestEnv) {
    std::fs::write(
        env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"m1\"\n",
    )
    .unwrap();
}

/// Context `alpha` with the Lead Session `l1`, named `old lead`, with `file` (or none registered), archived or not.
fn populate(env: &TestEnv, file: Option<&Path>, archived: bool) {
    let file = file.map_or("NULL".to_owned(), |f| format!("'{}'", f.display()));
    let archived = if archived {
        "'2026-01-04T00:00:00.000Z'"
    } else {
        "NULL"
    };
    env.sql(
        &format!(
            "INSERT INTO contexts (id, title, jira, created_at, created_on)
             VALUES ('alpha', 'Title', NULL, '2026-01-01T00:00:00.000Z', 'm1');
             INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at,
                                   created_at, archived_at, forked_from)
             VALUES ('l1', 'lead', 'alpha', NULL, NULL, 'old lead', {file}, '/repo', 'O3', NULL, 'stopped', NULL,
                     '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z', {archived}, 'l1');",
        ),
    );
}

fn entry(role: &str, part: serde_json::Value) -> String {
    json!({"type": "message", "timestamp": "2026-10-15T16:33:44.329Z", "message": {"role": role, "content": [part]}}).to_string() + "\n"
}

fn session_file(env: &TestEnv) -> PathBuf {
    let path = env.root().join("lead.jsonl");
    let text = [
        json!({"type": "session", "id": "l1"}).to_string() + "\n",
        entry("user", json!({"type": "text", "text": "Plan the API."})),
        entry(
            "assistant",
            json!({"type": "text", "text": "Here is the plan."}),
        ),
        entry(
            "assistant",
            json!({"type": "toolCall", "name": "bash", "arguments": {"command": "cpd plan show"}}),
        ),
        entry("toolResult", json!({"type": "text", "text": "ok"})),
    ]
    .concat();
    std::fs::write(&path, text).unwrap();
    path
}

// ---------- tests ----------

/// The view's first frame in a `width` × `height` pane over the file of the Session `l1` of `fx`, as the rows the pane shows.
fn drawn(fx: &Fx, width: u16, height: u16) -> Vec<String> {
    let (name, archived, file): (String, bool, String) = fx.env.read(|c| {
        c.query_row(
            "SELECT name, archived_at IS NOT NULL, file FROM sessions WHERE id = 'l1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
    });
    let text = std::fs::read_to_string(file).unwrap();
    let zone = cpd_core::clock::Clock::zone(&*fx.fakes.ports.clock);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|f| cpd_tui::draw_history(&name, archived, &text, &zone, f))
        .unwrap();
    let mut rows = Screen::from_buffer(terminal.backend().buffer()).rows();
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    rows
}

#[test]
fn the_session_file_of_an_archived_lead_is_rendered_read_only() {
    let fx = Fx::new();
    configure(&fx.env);
    let file = session_file(&fx.env);
    populate(&fx.env, Some(&file), true);
    assert_eq!(
        drawn(&fx, 40, 12),
        [
            "",
            " old lead (archived, read-only)",
            " 12:33:44 you",
            "       Plan the API.",
            " 12:33:44 agent",
            "       Here is the plan.",
            " 12:33:44 → bash cpd plan show",
            "         󰘌 ok",
        ]
    );
}

#[test]
fn a_session_that_is_not_archived_says_read_only_only() {
    let fx = Fx::new();
    configure(&fx.env);
    let file = session_file(&fx.env);
    populate(&fx.env, Some(&file), false);
    let screen = drawn(&fx, 40, 12);
    assert_eq!(screen[0], "", "the pad's first row is blank");
    assert_eq!(screen[1], " old lead (read-only)");
    assert!(screen.iter().any(|r| r.contains("󰘌 ok")), "{screen:?}");
}

#[test]
fn the_keys_scroll_and_q_leaves_for_the_placeholder_text() {
    let fx = Fx::new();
    configure(&fx.env);
    let file = session_file(&fx.env);
    populate(&fx.env, Some(&file), true);
    let text = std::fs::read_to_string(&file).unwrap();
    let screen = SharedBackend::new(40, 4);
    let steps = vec![
        Step::look(|s| assert!(s.contains("󰘌 ok"), "the end: {:?}", s.rows())),
        Step::ch('g'), // not a key
        Step::ch('k'),
        Step::ch('k'),
        Step::ch('k'),
        Step::ch('k'),
        Step::ch('k'),
        Step::ch('k'),
        Step::look(|s| {
            let rows = s.rows();
            assert!(
                rows.get(2).is_some_and(|r| r.contains("you")),
                "the top: {rows:?}"
            );
            // the uniform 1-cell pad: row 0 and row 3 blank, the title in row 1, the body in row 2
            assert_eq!(
                rows,
                ["", " old lead (archived, read-only)", " 12:33:44 you", ""]
            );
        }),
        Step::ch('G'),
        Step::look(|s| assert!(s.contains("󰘌 ok"), "the end again: {:?}", s.rows())),
        // q ends the loop, which `run` follows with the chat slot's placeholder
        Step::ch('q'),
    ];
    let mut input = Script::on_screen(fx.fakes.clock.clone(), &screen, steps);
    let mut ctx = Ctx::new(&fx.fakes.ports, screen.terminal(), &mut input);
    cpd_tui::show_history_with(&mut ctx, "old lead", true, &text).unwrap();
    drop(ctx);
    assert_eq!(input.remaining(), 0, "the loop ended on q, the last step");
}

#[test]
fn an_unknown_session_or_a_missing_file_is_refused_before_drawing() {
    // `load` refuses before `run_view` looks at the terminal, so no terminal is reached.
    let fx = Fx::with_ports();
    let env = &fx.env;
    configure(env);
    populate(env, None, true);
    let run = fx.cpd(&["tui", "view", "history", "nope"]);
    assert_eq!(
        (run.exit, run.error()["code"].clone()),
        (4, json!("session_not_found")),
        "{run:?}"
    );
    let run = fx.cpd(&["tui", "view", "history", "l1"]);
    assert_eq!(
        run.error()["code"],
        "file_unreadable",
        "a Session with no file: {run:?}"
    );
    env.sql(&format!(
        "UPDATE sessions SET file = '{}' WHERE id = 'l1'",
        env.root().join("gone.jsonl").display()
    ));
    let run = fx.cpd(&["tui", "view", "history", "l1"]);
    assert_eq!(
        run.error()["code"],
        "file_unreadable",
        "a file that is not there: {run:?}"
    );
    let run = fx.cpd(&["tui", "view", "history"]);
    assert_eq!(
        (run.exit, run.error()["code"].clone()),
        (2, json!("usage")),
        "{run:?}"
    );
}

#[test]
fn the_view_needs_a_terminal() {
    let fx = Fx::with_ports();
    configure(&fx.env);
    let file = session_file(&fx.env);
    populate(&fx.env, Some(&file), true);
    let run = fx.cpd_without_a_terminal(&["tui", "view", "history", "l1"]);
    assert_eq!(run.exit, 3, "{run:?}");
    let error = run.error();
    assert_eq!(error["code"], "not_a_terminal");
    assert_eq!(error["details"]["command"], "tui view history");
}

/// The one smoke of the built binary for the refusal (until `tooling/smoke.rs` has it): the real `isatty` of piped stdio.
#[test]
fn the_built_binary_needs_a_terminal() {
    let env = env();
    let file = session_file(&env);
    populate(&env, Some(&file), true);
    let run = env.cpd(&["tui", "view", "history", "l1"]);
    assert_eq!(run.exit, 3, "{run:?}");
    let error = run.error();
    assert_eq!(error["code"], "not_a_terminal");
    assert_eq!(error["details"]["command"], "tui view history");
}
