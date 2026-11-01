//! Red tests of the need lines between the PR boxes (docs/commands.md §13.2, rules R2 to R5 of Plan plan-lines): a need line
//! ends in a `┴` on the needing box's top border, a box keeps a dash after its title, and every connector cell touches what it
//! joins, in the canvas and in the drawn view, scrolled or not. The checker is [`violations`] and [`reaches`] of `lines_tests`.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::flow::{Chart, Live, Pill};
use super::lines_tests::{reaches, violations};
use super::{Ui, boxes, sample_centre, sample_job, sample_pr};
use crate::read::{JobNode, JobStatus, NeedsUser, PrNode, PrState, PrStatus, Pull};
use crate::test_text::rows;
use cpd_core::config::Tier;
use cpd_core::ids::{JobKey, PrKey};

const CAP_L: &str = "\u{e0b6}";
const CAP_R: &str = "\u{e0b4}";
const TO: &str = "\u{F19B0}";
const CUE: &str = "󱆭";

fn rep(s: &str, n: usize) -> String {
    s.repeat(n)
}

fn sp(n: usize) -> String {
    " ".repeat(n)
}

/// A PR `key` of repo `repo`, base `main`, `dispatch`, needing `needs`.
fn pr(key: &str, repo: &str, needs: &[&str]) -> PrNode {
    PrNode { repo_name: repo.into(), needs: needs.iter().map(|n| PrKey::parse(n).unwrap()).collect(), ..sample_pr(key) }
}

fn job(pr: &str, key: &str, status: JobStatus, tier: Option<&str>, needs: &[&str]) -> JobNode {
    JobNode {
        kind: "work".into(),
        status,
        tier: tier.map(|t| Tier::parse(t).unwrap()),
        shown_tier: tier.map(|t| Tier::parse(t).unwrap()),
        needs: needs.iter().map(|n| JobKey::parse(n).unwrap()).collect(),
        ..sample_job(pr, key)
    }
}

/// A box's title as the docs draw it: ` <key> · <repo> 󱦰 <base> · <status> `, or the pull request and the link cue.
fn title(p: &PrNode) -> String {
    let last = match &p.pull {
        Some(pull) => format!("#{} open {CUE}", pull.number),
        None => p.status.as_str().to_owned(),
    };
    format!(" {} · {} {TO} {} · {last} ", p.key, p.repo_name, p.base)
}

/// A pill as drawn: caps, icon, key, the bar and the tier.
fn pill(live: Live, key: &str, tier: &str) -> String {
    format!("{CAP_L}{} {key}│{tier}{CAP_R}", live.icon(0).0)
}

/// The rectangle of each PR's box, read off the canvas: the title starts two cells after the box's `╭`, the box's `╮` is the
/// first one after the title on that row and its `╰` the first one down the box's left column. (Chart lines inside a box use
/// `╭` and `╮` too, so the corners are found from the titles, not by scanning for them.) In the order of `prs`.
fn box_rects(buf: &Buffer, prs: &[PrNode]) -> Vec<Rect> {
    prs.iter()
        .map(|p| {
            let t: Vec<String> = title(p).chars().map(String::from).collect();
            let at = |x: u16, y: u16| buf[(x, y)].symbol();
            for y in 0..buf.area.height {
                for x in 2..buf.area.width.saturating_sub(t.len() as u16) {
                    if t.iter().enumerate().all(|(i, c)| at(x + i as u16, y) == c) {
                        let left = x - 2;
                        let right = (x..buf.area.width).find(|&x2| at(x2, y) == "╮").expect("a box has a `╮`");
                        let bottom = (y + 1..buf.area.height).find(|&y2| at(left, y2) == "╰").expect("a box has a `╰`");
                        return Rect::new(left, y, right - left + 1, bottom - y + 1);
                    }
                }
            }
            panic!("no title {:?} on the canvas:\n{}", title(p), rows(buf).join("\n"));
        })
        .collect()
}

/// The checker's findings over `window` of `buf`.
fn check(buf: &Buffer, pills: &[Pill], boxes: &[Rect], window: Rect) -> Vec<String> {
    let rects: Vec<Rect> = pills.iter().map(|p| p.rect).collect();
    violations(buf, &rects, boxes, window)
}

/// The column a need line to box `r` lands on (R3): the box's middle column, or the first column after the title.
fn landing(r: Rect, p: &PrNode) -> u16 {
    (r.x + (r.width - 1) / 2).max(r.x + 2 + title(p).chars().count() as u16)
}

/// Whether a line leads from box `from`'s `┬` to box `to`'s `┴`: [`reaches`] on a copy of the canvas where the boxes are
/// blank but for those two junctions (a path along a box's own border is no line), with a one-cell pill to stand for each.
fn joins(buf: &Buffer, boxes: &[Rect], from: usize, to: usize, to_pr: &PrNode) -> bool {
    let mut clean = buf.clone();
    for r in boxes {
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                let keep = (y == r.bottom() - 1 && buf[(x, y)].symbol() == "┬") || (y == r.y && buf[(x, y)].symbol() == "┴");
                if !keep {
                    clean[(x, y)].set_symbol(" ");
                }
            }
        }
    }
    let (f, t) = (boxes[from], boxes[to]);
    let start = f.x + (f.width - 1) / 2;
    let goal = landing(t, to_pr);
    let stand = |key: &str, x: u16, y: u16| Pill { key: key.into(), rect: Rect::new(x, y, 1, 1) };
    let pills = [stand("from", start - 1, f.bottom() - 1), stand("to", goal + 1, t.y)];
    reaches(&clean, &pills, "from", "to")
}

fn canvas_of(prs: &[PrNode], jobs: &[JobNode]) -> Chart {
    boxes::layout(prs, jobs, &NeedsUser::default(), 0).chart
}

fn shown(buf: &Buffer) -> String {
    rows(buf).join("\n")
}

fn assert_canvas(c: &Chart, prs: &[PrNode], want: &[String]) {
    let got = rows(&c.buf);
    assert_eq!(got, want, "\ngot:\n{}\nwant:\n{}", got.join("\n"), want.join("\n"));
    let found = check(&c.buf, &c.pills, &box_rects(&c.buf, prs), c.buf.area);
    assert!(found.is_empty(), "a connector does not touch what it joins: {found:#?}\n{}", got.join("\n"));
}

fn screenshot_1() -> (Vec<PrNode>, Vec<JobNode>) {
    let prs = vec![pr("api", "api-repo", &[]), pr("web", "web-repo", &["api"])];
    let jobs = vec![
        job("api", "api-job-long-name-for-wide-chart", JobStatus::Queued, Some("S1"), &[]),
        job("web", "web-job", JobStatus::Queued, Some("S1"), &[]),
    ];
    (prs, jobs)
}

fn screenshot_2() -> (Vec<PrNode>, Vec<JobNode>) {
    let prs = vec![
        PrNode { status: PrStatus::Merged, ..pr("dotfiles-prev", "dotfiles", &[]) },
        pr("leakflake", "dotfiles", &["dotfiles-prev"]),
    ];
    let jobs = vec![
        job("dotfiles-prev", "prev-job", JobStatus::Merged, Some("S1"), &[]),
        job("leakflake", "leakflake-config", JobStatus::Cancelled, None, &[]),
        job("leakflake", "leakflake-fix", JobStatus::Merged, Some("S2"), &[]),
        job("leakflake", "open-leakflake", JobStatus::Queued, Some("S1"), &["leakflake-config", "leakflake-fix"]),
    ];
    (prs, jobs)
}

fn skipped_row() -> (Vec<PrNode>, Vec<JobNode>) {
    let prs = vec![pr("a", "a-repo", &[]), pr("b", "b-repo", &["a"]), pr("c", "c-repo", &["b", "a"])];
    let jobs = ["a", "b", "c"].map(|k| job(k, &format!("j-{k}"), JobStatus::Queued, Some("S1"), &[])).to_vec();
    (prs, jobs)
}

#[test]
fn a_need_line_that_turns_ends_in_a_junction_on_the_needing_boxes_top_border() {
    let (prs, jobs) = screenshot_1();
    let c = canvas_of(&prs, &jobs);
    let q = Live::Queued;
    // api's box is 43 cells (its pill 39 and the walls and padding), web's 38 (its title 34 plus 4): the line leaves api's `┬`
    // at column 21, runs to column 36, the first `─` after web's title, and drops onto web's top border
    let want = [
        format!("╭─{}{}╮", title(&prs[0]), rep("─", 6)),
        format!("│ {} │", pill(q, "api-job-long-name-for-wide-chart", "S1")),
        format!("╰{}┬{}╯", rep("─", 20), rep("─", 20)),
        format!("{}╰{}╮", sp(21), rep("─", 14)),
        format!("{}│", sp(36)),
        format!("╭─{}┴╮", title(&prs[1])),
        format!("│ {}{}│", pill(q, "web-job", "S1"), sp(21)),
        format!("╰{}╯", rep("─", 36)),
    ];
    assert_canvas(&c, &prs, &want);
    let boxes = box_rects(&c.buf, &prs);
    assert!(joins(&c.buf, &boxes, 0, 1, &prs[1]), "no line from api's `┬` to web's `┴`\n{}", shown(&c.buf));
}

#[test]
fn a_need_line_lands_after_the_title_when_the_title_covers_the_middle_column() {
    let (prs, jobs) = screenshot_2();
    let c = canvas_of(&prs, &jobs);
    let (m, q, k) = (Live::Merged, Live::Queued, Live::Cancelled);
    // prev's box is 46 cells (its title 42 plus 4), leakflake's 52 (its chart 48 plus 4): leakflake's title (40) covers its
    // middle column 25, so the `┴` is at column 42; fix's line starts at fix's own right cap (column 20 of the chart)
    let want = [
        format!("╭─{}─╮", title(&prs[0])),
        format!("│ {}{}│", pill(m, "prev-job", "S1"), sp(28)),
        format!("╰{}┬{}╯", rep("─", 21), rep("─", 22)),
        format!("{}╰{}╮", sp(22), rep("─", 19)),
        format!("{}│", sp(42)),
        format!("╭─{}┴{}╮", title(&prs[1]), rep("─", 8)),
        format!("│ {}{}│", pill(k, "leakflake-config", "—"), sp(27)),
        format!("│ {}╭──{} │", sp(24), pill(q, "open-leakflake", "S1")),
        format!("│ {}{}╯{}│", pill(m, "leakflake-fix", "S2"), rep("─", 4), sp(24)),
        format!("╰{}╯", rep("─", 50)),
    ];
    assert_canvas(&c, &prs, &want);
    let boxes = box_rects(&c.buf, &prs);
    assert!(joins(&c.buf, &boxes, 0, 1, &prs[1]), "no line from prev's `┬` to leakflake's `┴`\n{}", shown(&c.buf));
}

/// Strips the links (`ESC ] 8 ; ; … BEL`) the title's pull request carries in its cells.
fn plain(buf: &Buffer, y: u16) -> String {
    let mut text = String::new();
    let mut skipping = false;
    for x in 0..buf.area.width {
        for ch in buf[(x, y)].symbol().chars() {
            match (skipping, ch) {
                (_, '\u{1b}') => skipping = true,
                (true, '\u{7}') => skipping = false,
                (false, ch) => text.push(ch),
                _ => {}
            }
        }
    }
    text.trim_end().to_owned()
}

#[test]
fn a_box_keeps_a_dash_after_its_title() {
    let open = PrNode {
        pull: Some(Pull {
            number: 12,
            state: PrState::Open,
            url: "https://github.com/acme/repo/pull/12".into(),
            conflicting: false,
            clean: false,
            update: None,
            draft: false,
        }),
        ..pr("long-pr-title-key", "long-pr-title-key-repo", &[])
    };
    for p in [pr("long-pr-title-key", "long-pr-title-key-repo", &[]), open] {
        let t = title(&p);
        let c = canvas_of(std::slice::from_ref(&p), &[]);
        let width = t.chars().count() + 4; // `╭`, `─`, the title, at least one `─`, `╮`
        assert_eq!(usize::from(c.buf.area.width), width, "the box is title + 4 cells wide\n{}", shown(&c.buf));
        if p.pull.is_none() {
            assert_eq!(plain(&c.buf, 0), format!("╭─{t}─╮"), "\n{}", shown(&c.buf));
        } else {
            // the link's cells hold pieces, so only the cells the link does not touch are compared
            let w = c.buf.area.width;
            let cell = |x: u16| c.buf[(x, 0)].symbol().to_string();
            assert_eq!([cell(0), cell(1)], ["╭", "─"], "\n{}", shown(&c.buf));
            assert_eq!([cell(w - 3), cell(w - 2), cell(w - 1)], [" ", "─", "╮"], "\n{}", shown(&c.buf));
        }
    }
}

#[test]
fn a_need_line_that_skips_a_row_of_boxes_touches_both_boxes() {
    let (prs, jobs) = skipped_row();
    let c = canvas_of(&prs, &jobs);
    let q = Live::Queued;
    // every box is 34 cells (its title 30 plus 4), its `┬` at column 16 and every landing at column 32, the first `─` after the
    // title; the line a -> c rides the free column 35, past b's right wall
    let t = title(&prs[0]).chars().count();
    assert_eq!(t, 30);
    let body = |k: &str, wall: &str| format!("│ {}{}│{wall}", pill(q, k, "S1"), sp(21));
    let want = [
        format!("╭─{}─╮", title(&prs[0])),
        body("j-a", ""),
        format!("╰{}┬{}╯", rep("─", 15), rep("─", 16)),
        format!("{}╰{}┬──╮", sp(16), rep("─", 15)),
        format!("{}│  │", sp(32)),
        format!("╭─{}┴╮ │", title(&prs[1])),
        body("j-b", " │"),
        format!("╰{}┬{}╯ │", rep("─", 15), rep("─", 16)),
        format!("{}╰{}┬──╯", sp(16), rep("─", 15)),
        format!("{}│", sp(32)),
        format!("╭─{}┴╮", title(&prs[2])),
        body("j-c", ""),
        format!("╰{}╯", rep("─", 32)),
    ];
    assert_canvas(&c, &prs, &want);
    let boxes = box_rects(&c.buf, &prs);
    for (from, to) in [(0, 1), (1, 2), (0, 2)] {
        assert!(joins(&c.buf, &boxes, from, to, &prs[to]), "no line from box {from} to box {to}\n{}", shown(&c.buf));
    }
}

#[test]
fn every_line_of_a_sweep_of_small_plans_touches_what_it_joins() {
    let key_sets = [["a", "b", "c", "d"], ["k", "key-two", "k3", "key-number-four-long"]];
    let pairs: Vec<(usize, usize)> = (1..4).flat_map(|k| (0..k).map(move |i| (i, k))).collect();
    let mut cases = 0;
    for keys in key_sets {
        for needs in 0..64u32 {
            let prs: Vec<PrNode> = (0..4)
                .map(|k| {
                    let n: Vec<&str> = pairs.iter().enumerate().filter(|(b, (_, kk))| *kk == k && needs >> b & 1 == 1).map(|(_, (i, _))| keys[*i]).collect();
                    pr(keys[k], &format!("{}-repo", keys[k]), &n)
                })
                .collect();
            let jobs: Vec<JobNode> = (0..4)
                .flat_map(|k| {
                    [
                        job(keys[k], &format!("live-{k}"), JobStatus::Queued, Some("S1"), &[]),
                        job(keys[k], &format!("cancelled-job-number-{k}"), JobStatus::Cancelled, None, &[]),
                    ]
                })
                .collect();
            let c = canvas_of(&prs, &jobs);
            cases += 1;
            let case = format!("keys {keys:?}, needs {needs:06b}\n{}", shown(&c.buf));
            let boxes = box_rects(&c.buf, &prs);
            let found = check(&c.buf, &c.pills, &boxes, c.buf.area);
            assert!(found.is_empty(), "{case}\n{found:#?}");
            for (k, p) in prs.iter().enumerate() {
                for need in &p.needs {
                    let i = prs.iter().position(|q| &q.key == need).unwrap();
                    let at = landing(boxes[k], p);
                    assert!(at < boxes[k].right() - 1, "{case}\nthe landing of box {k} is on its corner");
                    assert_eq!(c.buf[(at, boxes[k].y)].symbol(), "┴", "{case}\nno `┴` on box {k}'s top border at column {at}");
                    assert!(joins(&c.buf, &boxes, i, k, p), "{case}\nno line from box {i} to box {k}");
                }
            }
        }
    }
    assert_eq!(cases, 128);
}

/// The pane's body window of the drawn view at `offset`, as the canvas's own cells (the drawn cells put back at the offset
/// the view clamped to), with that window; `pane` is the pane's size.
fn drawn(centre: &super::Centre, pane: (u16, u16), offset: (u16, u16)) -> (Buffer, Rect, Buffer) {
    let mut ui = Ui::default();
    ui.scroll = offset;
    let mut terminal = Terminal::new(TestBackend::new(pane.0, pane.1)).unwrap();
    terminal.draw(|f| ui.draw(f, Some(centre))).unwrap();
    let pane_buf = terminal.backend().buffer().clone();
    let canvas = &centre.boxes.chart.buf;
    let (ox, oy) = ui.scroll;
    let window = Rect::new(ox, oy, ui.body.width, ui.body.height).intersection(canvas.area);
    let mut put = Buffer::empty(canvas.area);
    for y in window.y..window.bottom() {
        for x in window.x..window.right() {
            put[(x, y)] = pane_buf[(ui.body.x + x - ox, ui.body.y + y - oy)].clone();
        }
    }
    (put, window, pane_buf)
}

#[test]
fn the_lines_touch_in_every_window_of_the_drawn_view_scrolled_or_not() {
    // the Plan of the view, laid out as `snapshot` does (`sample_centre`, with the canvas margin)
    for (name, (prs, jobs)) in [("screenshot 2", screenshot_2()), ("skipped row", skipped_row())] {
        let centre = sample_centre(prs.clone(), jobs);
        let canvas = &centre.boxes.chart.buf;
        let boxes = box_rects(canvas, &prs);
        for pane in [(40, 12), (70, 20)] {
            for offset in [(0, 0), (0, 3), (5, 0), (7, 4), (u16::MAX, u16::MAX)] {
                let (put, window, pane_buf) = drawn(&centre, pane, offset);
                let found = check(&put, &centre.boxes.chart.pills, &boxes, window);
                assert!(
                    found.is_empty(),
                    "{name}, pane {pane:?}, offset {offset:?} (window {window:?}): {found:#?}\ndrawn:\n{}\ncanvas:\n{}",
                    shown(&pane_buf),
                    shown(canvas)
                );
                if offset == (0, 0) {
                    let edge = |x: u16, y: u16| pane_buf[(x, y)].symbol() != " ";
                    let (w, h) = pane;
                    let mut touched: Vec<(u16, u16)> = (0..h).map(|y| (0, y)).chain((0..w).map(|x| (x, 0))).collect();
                    if canvas.area.width <= w {
                        touched.extend((0..h).map(|y| (w - 1, y)));
                    }
                    if canvas.area.height <= h {
                        touched.extend((0..w).map(|x| (x, h - 1)));
                    }
                    let touched: Vec<_> = touched.into_iter().filter(|&(x, y)| edge(x, y)).collect();
                    assert!(touched.is_empty(), "{name}, pane {pane:?}: the canvas margin leaves {touched:?} on the pane's edge\n{}", shown(&pane_buf));
                }
            }
        }
    }
}
