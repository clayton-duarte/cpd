//! Asleep Sessions in the tree view (commands.md §13.2; contract §2.2 "Asleep"; hs-d515, hs-d520) over a prepared store, drawn
//! on a `TestBackend`: the colours are read from the cells. The click on a row is `tests/tui/tui_tree_asleep.rs`'s.

use ratatui::buffer::Buffer;
use ratatui::style::Color;

use super::prepared_tests::{Env, draw, text_rows};
use crate::palette::numbered::{C7, C8, C15};

const WHEN: &str = "2026-02-01T00:00:00.000Z";

fn fork(id: &str, extra_column: &str, extra_value: &str, created: &str) -> String {
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at{extra_column})
         VALUES ('{id}', 'lead', 'alpha', 'ship', NULL, 'name of {id}', '/sessions/{id}.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                 '2026-01-03T00:00:00.000Z', '{created}'{extra_value});"
    )
}

/// Context `alpha` with Plan `ship` (its Lead `l-ship`, idle) and three forks of it: `f-slp` (asleep, the oldest), `f-live` and
/// `f-arc` (archived), all idle. `asleep_at` is the column the Job's migration adds.
fn populate(env: &mut Env) {
    let mut s = String::from(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('alpha', 'Title of alpha', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('ship', 'alpha', 'Plan ship', 'grounding', 'l-ship', '2026-01-02T00:00:00.000Z');",
    );
    s += &fork("l-ship", "", "", "2026-01-02T00:00:00.000Z");
    s += &fork(
        "f-slp",
        ", asleep_at, forked_from",
        &format!(", '{WHEN}', 'l-ship'"),
        "2026-01-02T00:00:01.000Z",
    );
    s += &fork(
        "f-live",
        ", forked_from",
        ", 'l-ship'",
        "2026-01-02T00:00:02.000Z",
    );
    s += &fork(
        "f-arc",
        ", archived_at, forked_from",
        &format!(", '{WHEN}', 'l-ship'"),
        "2026-01-02T00:00:03.000Z",
    );
    env.sql(&s);
}

/// The index of the row of the fork labelled `ship·<suffix>`.
fn row_of(screen: &[String], suffix: &str) -> u16 {
    screen
        .iter()
        .position(|r| r.contains(&format!("ship·{suffix}")))
        .unwrap_or_else(|| panic!("no row of {suffix}: {screen:?}")) as u16
}

/// The foregrounds of the cells that draw something on row `y`, as runs (a colour escape of the row each time it changes), in order.
fn runs(buf: &Buffer, y: u16) -> Vec<Color> {
    let mut found: Vec<Color> = Vec::new();
    for x in (0..buf.area.width).filter(|&x| buf[(x, y)].symbol() != " ") {
        if found.last() != Some(&buf[(x, y)].fg) {
            found.push(buf[(x, y)].fg);
        }
    }
    found
}

#[test]
fn asleep_row_is_colour_7_among_live_rows() {
    let mut env = Env::new();
    populate(&mut env);
    let buf = draw(&mut env.view(), 60, 20);
    let plain = text_rows(&buf);
    let (asleep, live, archived) = (
        row_of(&plain, "f-sl"),
        row_of(&plain, "f-li"),
        row_of(&plain, "f-ar"),
    );
    assert!(
        live < asleep && asleep < archived,
        "the asleep row keeps its place among the live rows by creation, newest first (hs-d686, pro-d48), the archived one is after them: {plain:?}"
    );

    // Contract §6.1.1: a live idle row's dot and name are clickable text (15); an asleep row's marks are all 7 (inactive text);
    // an archived row's are all 8 (muted); the branch `├─` is a line, 8, on all three.
    // The row is ` ├─<dot> ship·f-xxx`: the branch from column 1, the dot at 3, the name from 5.
    let name_end = |y: u16| 5 + plain[y as usize].trim_start().chars().skip(4).count() as u16;
    let fg = |x: u16, y: u16| buf[(x, y)].fg;
    // the space between the dot and the name draws nothing: no colour of its own
    let all = |y: u16, from: u16, color: Color| {
        (from..name_end(y))
            .filter(|&x| buf[(x, y)].symbol() != " ")
            .all(|x| fg(x, y) == color)
    };
    assert!(all(live, 3, C15), "a live idle row: {:?}", runs(&buf, live));
    assert!(
        all(asleep, 3, C7),
        "an asleep row (one colour 7 run from the dot to the name): {:?}",
        runs(&buf, asleep)
    );
    assert!(
        !runs(&buf, asleep).contains(&C15),
        "an asleep row has no clickable-text mark: {:?}",
        runs(&buf, asleep)
    );
    let archived_runs = runs(&buf, archived);
    assert!(
        archived_runs.contains(&C8)
            && !archived_runs.contains(&C7)
            && !archived_runs.contains(&C15),
        "an archived row: {archived_runs:?}"
    );
    assert_ne!(
        runs(&buf, asleep),
        archived_runs,
        "an asleep row is inactive (7), an archived one muted (8)"
    );
}
