//! Red tests of the Job lines of the Plan view (docs/commands.md §13.2, rules R1 and R5 of Plan plan-lines): every connector
//! cell touches what it joins. [`violations`] and [`reaches`] are the checker the box tests reuse.

use std::collections::{HashSet, VecDeque};

use ratatui::{buffer::Buffer, layout::Rect};

use super::flow::{self, Chart, Live, Pill};
use crate::read::{JobNode, JobStatus};
use crate::test_text::rows;
use cpd_core::config::Tier;
use cpd_core::ids::{JobKey, PrKey};

const U: u8 = 1;
const D: u8 = 2;
const L: u8 = 4;
const R: u8 = 8;
const CAP_L: &str = "\u{e0b6}";
const CAP_R: &str = "\u{e0b4}";

/// The arms of a line glyph; 0 for any other symbol.
fn arms(symbol: &str) -> u8 {
    match symbol {
        "─" => L | R,
        "│" => U | D,
        "╭" => D | R,
        "╮" => D | L,
        "╰" => U | R,
        "╯" => U | L,
        "├" => U | D | R,
        "┤" => U | D | L,
        "┬" => L | R | D,
        "┴" => L | R | U,
        "┼" => U | D | L | R,
        _ => 0,
    }
}

fn contains(r: Rect, x: i32, y: i32) -> bool {
    x >= r.x as i32 && y >= r.y as i32 && x < r.right() as i32 && y < r.bottom() as i32
}

fn on_border(boxes: &[Rect], x: u16, y: u16) -> bool {
    boxes.iter().any(|b| {
        contains(*b, x as i32, y as i32) && (x == b.x || x == b.right() - 1 || y == b.y || y == b.bottom() - 1)
    })
}

/// The neighbour of `(x, y)` in the direction of arm `d`, and the arm it must hold to answer.
fn step(x: u16, y: u16, d: u8) -> (i32, i32, u8) {
    let (x, y) = (x as i32, y as i32);
    match d {
        U => (x, y - 1, D),
        D => (x, y + 1, U),
        L => (x - 1, y, R),
        _ => (x + 1, y, L),
    }
}

/// Every cell of `window` whose line glyph has an arm that leads nowhere, and why. A cell inside a pill is not a line; a cell
/// on a box border is checked only when it is a `┬` (arm down) or a `┴` (arm up).
pub(super) fn violations(buf: &Buffer, pills: &[Rect], boxes: &[Rect], window: Rect) -> Vec<String> {
    let mut found = Vec::new();
    for y in window.y..window.bottom() {
        for x in window.x..window.right() {
            if pills.iter().any(|p| contains(*p, x as i32, y as i32)) {
                continue;
            }
            let symbol = buf[(x, y)].symbol();
            let a = arms(symbol);
            if a == 0 {
                continue;
            }
            let wanted: Vec<u8> = if on_border(boxes, x, y) {
                match symbol {
                    "┬" => vec![D],
                    "┴" => vec![U],
                    _ => continue,
                }
            } else {
                [U, D, L, R].into_iter().filter(|d| a & d != 0).collect()
            };
            for d in wanted {
                let (nx, ny, back) = step(x, y, d);
                if !contains(window, nx, ny) {
                    continue;
                }
                let joined = arms(buf[(nx as u16, ny as u16)].symbol()) & back != 0;
                let in_pill = d & (L | R) != 0 && pills.iter().any(|p| contains(*p, nx, ny));
                if !joined && !in_pill {
                    found.push(format!("({x},{y}) {symbol:?}: arm {} leads to ({nx},{ny}), which does not touch it", name(d)));
                }
            }
        }
    }
    found
}

fn name(d: u8) -> &'static str {
    match d {
        U => "up",
        D => "down",
        L => "left",
        _ => "right",
    }
}

/// Whether a path of matched arms leads from the cell right after pill `from` to the cell left of pill `to`, through other
/// pills left to right (entered at the left cap, left at the right cap).
pub(super) fn reaches(buf: &Buffer, pills: &[Pill], from: &str, to: &str) -> bool {
    let rect = |key: &str| pills.iter().find(|p| p.key == key).unwrap_or_else(|| panic!("no pill {key}")).rect;
    let (from, to) = (rect(from), rect(to));
    let (start, goal) = ((from.right(), from.y), (to.x.wrapping_sub(1), to.y));
    let inside = |x: u16, y: u16| x < buf.area.width && y < buf.area.height;
    if !inside(start.0, start.1) || !inside(goal.0, goal.1) {
        return false;
    }
    let mut seen = HashSet::from([start]);
    let mut todo = VecDeque::from([start]);
    while let Some((x, y)) = todo.pop_front() {
        if (x, y) == goal {
            return true;
        }
        let a = arms(buf[(x, y)].symbol());
        for d in [U, D, L, R].into_iter().filter(|d| a & d != 0) {
            let (nx, ny, back) = step(x, y, d);
            if nx < 0 || ny < 0 || !inside(nx as u16, ny as u16) {
                continue;
            }
            let next = (nx as u16, ny as u16);
            if arms(buf[next].symbol()) & back != 0 && seen.insert(next) {
                todo.push_back(next);
            } else if d == R {
                // a pill entered at its left cap is left at its right cap
                if let Some(p) = pills.iter().find(|p| p.rect.x == next.0 && p.rect.y == next.1) {
                    let out = (p.rect.right(), p.rect.y);
                    if inside(out.0, out.1) && seen.insert(out) {
                        todo.push_back(out);
                    }
                }
            }
        }
    }
    false
}

fn node(key: &str, status: JobStatus, tier: Option<&str>, needs: &[&str]) -> JobNode {
    JobNode {
        kind: "work".into(),
        key: JobKey::parse(key).unwrap(),
        pr: Some(PrKey::parse("p").unwrap()),
        seq: 0,
        title: String::new(),
        status,
        tier: tier.map(|t| Tier::parse(t).unwrap()),
        needs: needs.iter().map(|n| JobKey::parse(n).unwrap()).collect(),
        appended: false,
        retry_tier: None,
        shown_tier: tier.map(|t| Tier::parse(t).unwrap()),
        failed_cause: None,
        cancel_reason: None,
        merged_sha: None,
        waiting_on: Vec::new(),
        attempts: Vec::new(),
    }
}

fn chart_of(nodes: &[JobNode]) -> Chart {
    flow::layout(&flow::flow_jobs(nodes, &[]), 0)
}

/// A pill as drawn: caps, icon, key, `pad` blanks, the bar and the tier.
fn pill(live: Live, key: &str, pad: usize, tier: &str) -> String {
    format!("{CAP_L}{} {key}{}│{tier}{CAP_R}", live.icon(0).0, " ".repeat(pad))
}

fn rep(s: &str, n: usize) -> String {
    s.repeat(n)
}

fn check(c: &Chart) -> Vec<String> {
    let rects: Vec<Rect> = c.pills.iter().map(|p| p.rect).collect();
    violations(&c.buf, &rects, &[], c.buf.area)
}

fn assert_chart(c: &Chart, want: &[String]) {
    let got = rows(&c.buf);
    assert_eq!(got, want, "\ngot:\n{}\nwant:\n{}", got.join("\n"), want.join("\n"));
    assert!(check(c).is_empty(), "a connector does not touch what it joins: {:#?}\n{}", check(c), got.join("\n"));
}

#[test]
fn the_checker_flags_a_gap_before_a_line_and_a_line_that_ends_in_the_air() {
    let mut buf = Buffer::empty(Rect::new(0, 0, 12, 4));
    buf.set_string(0, 0, "xxxx", ratatui::style::Style::default());
    buf.set_string(6, 0, "───", ratatui::style::Style::default()); // a gap at x=4..5 before the line
    buf.set_string(3, 2, "│", ratatui::style::Style::default()); // a vertical with nothing above or below
    let pills = [Rect::new(0, 0, 4, 1)];
    let found = violations(&buf, &pills, &[], buf.area);
    assert!(found.iter().any(|f| f.starts_with("(6,0)") && f.contains("left")), "{found:#?}");
    assert!(found.iter().any(|f| f.starts_with("(3,2)") && f.contains("up")), "{found:#?}");
    assert!(found.iter().any(|f| f.starts_with("(3,2)") && f.contains("down")), "{found:#?}");
    // a right end in the air is flagged inside the window, and not outside it
    assert!(found.iter().any(|f| f.starts_with("(8,0)") && f.contains("right")), "{found:#?}");
    assert!(violations(&buf, &pills, &[], Rect::new(0, 0, 9, 1)).iter().all(|f| !f.starts_with("(8,0)")));
}

#[test]
fn the_checker_accepts_a_line_that_touches_pills_and_a_box_junction() {
    let mut buf = Buffer::empty(Rect::new(0, 0, 12, 5));
    let plain = ratatui::style::Style::default();
    buf.set_string(0, 0, "xxxx─╮", plain);
    buf.set_string(5, 1, "│", plain);
    buf.set_string(2, 2, "╭──┴──╮", plain);
    buf.set_string(2, 3, "│     │", plain);
    buf.set_string(2, 4, "╰─────╯", plain);
    let pills = [Rect::new(0, 0, 4, 1)];
    let boxes = [Rect::new(2, 2, 7, 3)];
    assert_eq!(violations(&buf, &pills, &boxes, buf.area), Vec::<String>::new());
    // the same junction with no line above it is flagged, and its border `─` never is
    buf.set_string(5, 1, " ", plain);
    let found = violations(&buf, &pills, &boxes, buf.area);
    assert!(found.iter().any(|f| f.starts_with("(5,2)") && f.contains("up")), "{found:#?}");
    assert!(found.iter().all(|f| !f.starts_with("(3,2)") && !f.starts_with("(4,2)")), "{found:#?}");
}

#[test]
fn a_job_line_starts_at_its_own_pill_when_a_cancelled_pill_widens_its_column() {
    // the screenshot: the cancelled pill is 22 cells, the live one 20; the cancelled Job's needs are not drawn
    let c = chart_of(&[
        node("leakflake-config", JobStatus::Cancelled, None, &[]),
        node("leakflake-fix", JobStatus::Merged, Some("S2"), &[]),
        node("open-leakflake", JobStatus::Queued, Some("S1"), &["leakflake-config", "leakflake-fix"]),
    ]);
    let want = [
        pill(Live::Cancelled, "leakflake-config", 0, "—"),
        format!("{}╭──{}", " ".repeat(24), pill(Live::Queued, "open-leakflake", 0, "S1")),
        format!("{}{}╯", pill(Live::Merged, "leakflake-fix", 0, "S2"), rep("─", 4)),
    ];
    assert_chart(&c, &want);
    assert!(reaches(&c.buf, &c.pills, "leakflake-fix", "open-leakflake"), "\n{}", rows(&c.buf).join("\n"));
}

#[test]
fn a_line_that_skips_a_column_touches_both_pills() {
    // a, x, z are column 0 (z cancelled and the widest: 16 cells; a and xray 11); b is column 1, c column 2
    let c = chart_of(&[
        node("a", JobStatus::Queued, Some("S1"), &[]),
        node("b", JobStatus::Queued, Some("S1"), &["a"]),
        node("c", JobStatus::Queued, Some("S1"), &["b", "xray"]),
        node("xray", JobStatus::Queued, Some("S1"), &[]),
        node("zulu-long", JobStatus::Cancelled, Some("S1"), &[]),
    ]);
    let q = Live::Queued;
    let want = [
        format!("{}{}╮", pill(q, "a", 3, "S1"), rep("─", 7)),
        format!("{}├{}╮", " ".repeat(18), rep("─", 12)),
        format!("{}{}┴──{}──┴──{}", pill(q, "xray", 0, "S1"), rep("─", 7), pill(q, "b", 0, "S1"), pill(q, "c", 0, "S1")),
        String::new(),
        pill(Live::Cancelled, "zulu-long", 0, "S1"),
    ];
    // the first row is `a`'s pill, 7 cells of line and the bus corner; the bus is at column 18
    assert_chart(&c, &want);
    assert!(reaches(&c.buf, &c.pills, "xray", "c"), "\n{}", rows(&c.buf).join("\n"));
    assert!(reaches(&c.buf, &c.pills, "a", "b"), "\n{}", rows(&c.buf).join("\n"));
}

#[test]
fn a_card_of_several_jobs_is_held_by_brackets_that_touch_every_pill() {
    let c = chart_of(&[
        node("a", JobStatus::Queued, Some("S1"), &[]),
        node("b", JobStatus::Queued, Some("S1"), &["a"]),
        node("c", JobStatus::Queued, Some("S1"), &["a"]),
        node("d", JobStatus::Queued, Some("S1"), &["b", "c"]),
        node("e", JobStatus::Queued, Some("S1"), &["b", "c"]),
        node("zulu-long", JobStatus::Cancelled, Some("S1"), &[]),
    ]);
    let q = Live::Queued;
    // the cancelled `zulu-long` (16 cells) widens column 0: `a`'s line starts at its own right cap, 8 cells in
    let want = [
        format!("{}{}┬{}┬{}┬{}╮", pill(q, "a", 0, "S1"), rep("─", 12), pill(q, "b", 0, "S1"), rep("─", 3), pill(q, "d", 0, "S1")),
        format!("{}╰{}╯{}╰{}╯", " ".repeat(20), pill(q, "c", 0, "S1"), " ".repeat(3), pill(q, "e", 0, "S1")),
        pill(Live::Cancelled, "zulu-long", 0, "S1"),
    ];
    assert_chart(&c, &want);
}

#[test]
fn every_job_line_of_a_sweep_of_small_plans_touches_what_it_joins() {
    let key_sets = [["a", "b", "c", "d"], ["k", "key-two", "k3", "key-number-four"]];
    let pairs: Vec<(usize, usize)> = (1..4).flat_map(|k| (0..k).map(move |i| (i, k))).collect();
    let mut layouts = 0;
    for keys in key_sets {
        for needs in 0..64u32 {
            for cancelled in 0..16u32 {
                let nodes: Vec<JobNode> = (0..4)
                    .map(|k| {
                        let status = if cancelled >> k & 1 == 1 { JobStatus::Cancelled } else { JobStatus::Queued };
                        let n: Vec<&str> = pairs.iter().enumerate().filter(|(b, (_, kk))| *kk == k && needs >> b & 1 == 1).map(|(_, (i, _))| keys[*i]).collect();
                        node(keys[k], status, Some("S1"), &n)
                    })
                    .collect();
                let flow = flow::flow_jobs(&nodes, &[]);
                let c = flow::layout(&flow, 0);
                layouts += 1;
                let case = format!("keys {keys:?}, needs {needs:06b}, cancelled {cancelled:04b}");
                let found = check(&c);
                assert!(found.is_empty(), "{case}: {found:#?}\n{}", rows(&c.buf).join("\n"));
                for j in &flow {
                    for n in &j.needs {
                        assert!(reaches(&c.buf, &c.pills, n, &j.key), "{case}: no line from {n} to {}\n{}", j.key, rows(&c.buf).join("\n"));
                    }
                }
            }
        }
    }
    assert_eq!(layouts, 2048);
}
