//! The `plan` view shows cancelled Jobs and PRs (commands.md §13.2, hs-d518): a cancelled Job keeps its place in
//! its PR's chart, muted (the palette's `Meaning::Nothing` grey), with its status `cancelled` and its reason; the
//! needs on it are not drawn; a cancelled PR gets its box, muted, after the live ones, with its Jobs.
//!
//! The view's first frame is drawn on a `TestBackend` over the store of an `Fx` (`cpd_tui::draw_first_frame`), as in
//! `tui_plan`; the store is prepared with SQL, and the Jobs and PRs are cancelled with `job cancel` and `pr cancel`, in
//! this process. The fixture helpers are shared with `tui_plan_waiting`.

use crate::common;

use std::fs;

use common::fx::Fx;
use cpd_tui::palette::{self, CANCELLED, Meaning};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, CellDiffOption};

pub(crate) const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// An `Fx` whose config has `concurrency = 4`.
pub(crate) fn env() -> Fx {
    let fx = Fx::new();
    fx.env.write_models();
    fs::write(
        fx.env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"m1\"\nconcurrency = 4\n",
    )
    .unwrap();
    fx
}

/// The view's first frame on a 300 × 40 pane (the width of the old pane tests) over the store of `fx`.
pub(crate) fn draw(fx: &Fx) -> Buffer {
    draw_sized(fx, 300, 40)
}

pub(crate) fn draw_sized(fx: &Fx, width: u16, height: u16) -> Buffer {
    let mut store = fx.env.store();
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    terminal
        .draw(|f| cpd_tui::draw_first_frame(&mut store, f, now_ms).unwrap())
        .unwrap();
    terminal.backend().buffer().clone()
}

/// The pane's rows as text, each without trailing blanks, as the terminal shows them: a linked piece is one cell whose symbol
/// carries its OSC 8 escapes (`ESC ] 8 ; ; <url> BEL`, which take no column) and holds the text of the 1 or 2 cells it covers
/// (`cpd_tui::link`), so the cells it covers are skipped.
pub(crate) fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height)
        .map(|y| {
            let (mut row, mut x) = (String::new(), 0);
            while x < buf.area.width {
                let cell = &buf[(x, y)];
                row.push_str(&plain(cell.symbol()));
                x += match cell.diff_option {
                    CellDiffOption::ForcedWidth(width) => width.get(),
                    _ => 1,
                };
            }
            row.trim_end().to_owned()
        })
        .collect()
}

/// `symbol` without its OSC 8 escapes.
pub(crate) fn plain(symbol: &str) -> String {
    let mut out = String::new();
    let mut chars = symbol.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            chars.by_ref().find(|c| *c == '\x07');
        } else {
            out.push(c);
        }
    }
    out
}

/// The first cell of `word` in the pane's rows: `None` when no row has it (every glyph of a row is one column wide).
pub(crate) fn cells_of(buf: &Buffer, word: &str) -> Option<Vec<(u16, u16)>> {
    rows(buf).iter().enumerate().find_map(|(y, row)| {
        let at = row.find(word)?;
        let x = row[..at].chars().count() as u16;
        Some(
            (0..word.chars().count() as u16)
                .map(|i| (x + i, y as u16))
                .collect(),
        )
    })
}

/// A fixture Job: `(pr, key, status, kind, needs)`; `status` is `queued`, `merged` or `cancelled` (written `queued`, then cancelled).
pub(crate) type FixtureJob<'a> = (&'a str, &'a str, &'a str, &'a str, &'a [&'a str]);

/// Context `ctx`, Plan `ship` in `dispatch` whose Lead is in the chat slot, the started PRs `prs` (`(key, repo dir)`) and
/// `jobs`; then every `cancelled` Job is cancelled with `job cancel --reason "<key> is not needed"`.
pub(crate) fn plan(fx: &Fx, prs: &[(&str, &str)], jobs: &[FixtureJob]) {
    let mut s = String::from(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('ctx', 'Title', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('ship', 'ctx', 'Plan ship', 'dispatch', 'lead-ship', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');",
    );
    for (key, dir) in prs {
        s += &format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
             VALUES ('ship', '{key}', '/{dir}', 'main', 'dispatch', 'cpd/ship/{key}', '/wt/{key}', cpd_txn(), '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');"
        );
    }
    for (pr, key, status, kind, _) in jobs {
        let merged = if *status == "merged" {
            "'abc123'"
        } else {
            "NULL"
        };
        let status = if *status == "cancelled" {
            "queued"
        } else {
            status
        };
        s += &format!(
            "INSERT INTO jobs (plan, workflow, key, kind, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                               created_txn, merged_sha, created_at)
             VALUES ('ship', '{pr}', '{key}', '{kind}', 'Job {key}', 'S3', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, '{FP}',
                     cpd_txn(), {merged}, '2026-01-02T00:00:00.000Z');"
        );
    }
    for (_, key, _, _, needs) in jobs {
        for need in *needs {
            s += &format!(
                "INSERT INTO dependencies (plan, job, needs) VALUES ('ship', '{key}', '{need}');"
            );
        }
    }
    // the Lead in the chat slot
    s += "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
          VALUES ('lead-ship', 'lead', 'ctx', 'ship', NULL, 'name', '/sessions/lead-ship.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                  '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
          INSERT INTO events (txn, at, kind, context, plan, session, text, data)
          VALUES (cpd_txn(), '2026-01-03T00:00:00.000Z', 'session-moved', 'ctx', 'ship', 'lead-ship', 'fixture',
                  '{\"pane\":\"%1\",\"window\":\"@1\",\"place\":\"chat\"}');";
    fx.env.sql(&s);
    for (_, key, status, _, _) in jobs {
        if *status == "cancelled" {
            fx.cpd(&[
                "job",
                "cancel",
                key,
                "--reason",
                &format!("{key} is not needed"),
                "--plan",
                "ship",
            ])
            .ok();
        }
    }
}

/// The rows of the box titled with `title` (its title row to its bottom border), the box's borders cut off.
fn box_inside(screen: &[String], title: &str) -> Vec<String> {
    let top = screen
        .iter()
        .position(|l| l.trim_start().starts_with("╭─ ") && l.contains(title))
        .unwrap_or_else(|| panic!("no box {title}:\n{}", screen.join("\n")));
    screen[top + 1..]
        .iter()
        .take_while(|l| !l.trim_start().starts_with('╰'))
        .map(|l| {
            l.chars()
                .skip(3)
                .collect::<String>()
                .trim_end()
                .trim_end_matches('│')
                .to_owned()
        })
        .collect()
}

#[test]
fn a_cancelled_job_pill_is_colour_8_and_nothing_follows_it() {
    let fx = env();
    plan(
        &fx,
        &[("api", "api")],
        &[("api", "a", "cancelled", "work", &[])],
    );
    let buf = draw(&fx);
    let screen = rows(&buf);
    let text = screen.join("\n");
    // the whole chart row is the pill, to its closing cap: no status word, no reason
    let row = screen.iter().find(|l| l.contains(" a│")).unwrap();
    let inside: String = row
        .chars()
        .skip(2)
        .collect::<String>()
        .trim_end()
        .trim_end_matches('│')
        .trim_end()
        .to_owned();
    assert_eq!(
        inside,
        format!(" \u{e0b6}{CANCELLED} a│S3\u{e0b4}"),
        "{text}"
    );
    assert!(
        !text.contains("cancelled") && !text.contains("not needed"),
        "{text}"
    );
    // the pill is colour 8, inverted (contract §6.1.1: a cancelled Job's pill): no colour 0 anywhere
    let colours =
        cells_of(&buf, "a│S3").unwrap_or_else(|| panic!("the pill is not on the screen:\n{text}"));
    assert!(
        colours
            .iter()
            .all(|at| buf[*at].fg == palette::color(Meaning::Nothing)),
        "{colours:?}"
    );
}

#[test]
fn cancelled_pr_box_is_drawn_muted_after_live_ones() {
    let fx = env();
    // `aaa` sorts first by key: it is drawn after `web` only because it is cancelled
    plan(
        &fx,
        &[("aaa", "aaa-repo"), ("web", "web-repo")],
        &[
            ("aaa", "o1", "cancelled", "work", &[]),
            ("web", "w", "queued", "work", &[]),
        ],
    );
    fx.cpd(&["workflow", "cancel", "aaa", "--plan", "ship"])
        .ok();
    let buf = draw(&fx);
    let screen = rows(&buf);
    let text = screen.join("\n");
    let row_of = |needle: &str| {
        screen
            .iter()
            .position(|l| l.trim_start().starts_with("╭─ ") && l.contains(needle))
    };
    let (web, aaa) = (
        row_of("web ·").expect("the live box"),
        row_of("aaa ·").unwrap_or_else(|| panic!("the cancelled PR has its box:\n{text}")),
    );
    assert!(
        web < aaa,
        "the cancelled box is below the live one:\n{text}"
    );
    assert!(
        screen[aaa].contains("cancelled"),
        "its title says so: {}",
        screen[aaa]
    );
    assert!(
        box_inside(&screen, "aaa ·")
            .iter()
            .any(|l| l.contains("o1")),
        "with its Jobs:\n{text}"
    );
    let muted = |word: &str| {
        cells_of(&buf, word)
            .unwrap_or_else(|| panic!("{word} is not on the screen"))
            .iter()
            .map(|at| buf[*at].fg == palette::color(Meaning::Nothing))
            .collect::<Vec<_>>()
    };
    assert!(
        muted("cancelled").iter().all(|m| *m),
        "the status is muted: {:?}",
        muted("cancelled")
    );
    let title = muted("aaa ·");
    assert!(
        title.iter().filter(|m| **m).count() >= 3,
        "the cancelled PR's key is muted: {title:?}"
    );
}

#[test]
fn needs_on_a_cancelled_job_are_not_drawn() {
    let fx = env();
    // `b` needs `gone`, which is cancelled: the need counts for nothing
    plan(
        &fx,
        &[("api", "api")],
        &[
            ("api", "gone", "cancelled", "work", &[]),
            ("api", "b", "queued", "work", &["gone"]),
        ],
    );
    let screen = rows(&draw(&fx));
    let text = screen.join("\n");
    assert!(text.contains("gone"), "the cancelled Job is drawn:\n{text}");
    let inside = box_inside(&screen, "api ·").join("\n");
    assert!(
        !inside.contains(['─', '┬', '┴', '├', '┤', '┼', '╭', '╮', '╰', '╯']),
        "no line or link between the pills:\n{text}"
    );
}
