//! The plan view's chart: a pure layout of one Workflow's Jobs as GitHub-Actions-style cards, left to right; [`super::boxes`]
//! puts each Workflow's chart in its box.
//!
//! In: Jobs. Out: a `Buffer` of the whole chart (no padding: the view insets the window it blits into) plus each pill's
//! rectangle. Implied needs are dropped (transitive reduction); Jobs with the same needs share one card, stacked; a card's
//! column is one past the deepest column it needs; each card has one entry and one exit at its middle row, joined by one
//! vertical bus per gap between columns, lines that skip columns riding a row free of cards.
//!
//! Hand-written drawing is only the line glyphs (`Mask`): ratatui 0.30's `Cell::merge_symbol` (ratatui-core 0.1.2
//! `buffer/cell.rs`) merges straight edges into junctions but turns a rounded corner (`╭╮╰╯`) plain when a third line
//! joins, so `Mask` keeps the corner. The window copy is [`copy_cells`]. Positions are lengths laid end to end
//! ([`starts`]), not ratatui's `Layout`, whose constraint solver takes seconds for a few hundred cards; pills are
//! `Line`s aligned left and right.

use std::collections::{HashMap, HashSet};

use ratatui::{
    buffer::Buffer,
    layout::{Margin, Position, Rect},
    style::Style,
    text::Line,
    widgets::Widget,
};

use crate::buffer::copy_cells;
use crate::cells;
use crate::palette::{self, Meaning};
use crate::pill;
use cpd_core::config::Tier;
use crate::read::{JobNode, JobStatus};
use cpd_core::ids::JobKey;

/// A Job's status as the chart draws it. A `cancelled` Job is a colour 8 pill, in its place; a `running` Job whose Attempt is
/// `merging` is `Merging` (the cockpit's `in_review`); a `failed` Job is always red (hs-d586): `Failed` with the static close
/// circle, `FailedWorked` (the red moon loader) while the Plan's Lead works.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Live {
    Draft,
    Queued,
    Running,
    Merging,
    Merged,
    Failed,
    FailedWorked,
    Cancelled,
}

impl Live {
    /// The icon and meaning the palette gives this status; a running Job's icon is the spinner's `frame`.
    pub fn icon(self, frame: usize) -> (&'static str, Meaning) {
        match self {
            Live::Draft => palette::job_status(JobStatus::Draft, false),
            Live::Queued => palette::job_status(JobStatus::Queued, false),
            Live::Running => (palette::SPINNER[frame % palette::SPINNER.len()], palette::job_status(JobStatus::Running, false).1),
            Live::Merging => palette::job_status(JobStatus::Running, true),
            Live::Merged => palette::job_status(JobStatus::Merged, false),
            Live::Failed => palette::job_status(JobStatus::Failed, false),
            Live::FailedWorked => (palette::SPINNER[frame % palette::SPINNER.len()], palette::job_status(JobStatus::Failed, false).1),
            Live::Cancelled => palette::job_status(JobStatus::Cancelled, false),
        }
    }

    /// The pill's colours (hs-d534): the status colour's bright pair as the background, text 0 (the palette's [`palette::job_pill`]).
    pub fn pill(self) -> Style {
        match self {
            Live::Draft => palette::job_pill(JobStatus::Draft, false, false),
            Live::Queued => palette::job_pill(JobStatus::Queued, false, false),
            Live::Running => palette::job_pill(JobStatus::Running, false, false),
            Live::Merging => palette::job_pill(JobStatus::Running, true, false),
            Live::Merged => palette::job_pill(JobStatus::Merged, false, false),
            Live::Failed | Live::FailedWorked => palette::job_pill(JobStatus::Failed, false, true),
            Live::Cancelled => palette::job_pill(JobStatus::Cancelled, false, false),
        }
    }

    /// `worked_on`: the failed Job is being worked on (its pill turns).
    pub fn of(job: &JobNode, worked_on: bool) -> Live {
        match job.status {
            JobStatus::Draft => Live::Draft,
            JobStatus::Queued => Live::Queued,
            JobStatus::Running if job.active_attempt().is_some_and(|a| a.status.is_landing()) => Live::Merging,
            JobStatus::Running => Live::Running,
            JobStatus::Merged => Live::Merged,
            JobStatus::Failed if worked_on => Live::FailedWorked,
            JobStatus::Failed => Live::Failed,
            JobStatus::Cancelled => Live::Cancelled,
        }
    }
}

/// What the layout needs to know of a Job.
#[derive(Clone, Debug)]
pub struct FlowJob {
    pub key: String,
    pub status: Live,
    /// None for a stub draft: the pill shows `—`.
    pub tier: Option<Tier>,
    pub needs: Vec<String>,
    /// How many Attempts the Job has had; the pill shows `#<n>` before the tier from the second on.
    pub attempt: usize,
}

pub struct Pill {
    pub key: String,
    pub rect: Rect,
}

/// Jobs (one Workflow's) as the chart draws them, in creation order: a cancelled Job is drawn muted, in its place, with nothing after it; the needs on it, and its own, are not drawn (they count for nothing). `worked_on`: the Plan's failed Jobs being worked on.
pub fn flow_jobs<'a>(jobs: impl IntoIterator<Item = &'a JobNode>, worked_on: &[JobKey]) -> Vec<FlowJob> {
    let all: Vec<&JobNode> = jobs.into_iter().collect();
    let cancelled = |key: &JobKey| all.iter().any(|j| &j.key == key && j.status == JobStatus::Cancelled);
    all.iter()
        .map(|j| FlowJob {
            key: j.key.to_string(),
            status: Live::of(j, worked_on.contains(&j.key)),
            tier: j.shown_tier,
            needs: if j.status == JobStatus::Cancelled {
                Vec::new()
            } else {
                j.needs.iter().filter(|n| all.iter().any(|l| &l.key == *n) && !cancelled(n)).map(|n| n.to_string()).collect()
            },
            attempt: j.attempts.len(),
        })
        .collect()
}

pub struct Chart {
    pub buf: Buffer,
    /// In the order of the (visible) Jobs.
    pub pills: Vec<Pill>,
    /// Pill indices of each column, top to bottom.
    pub columns: Vec<Vec<usize>>,
}

/// Columns between a column's right bracket and the next column's left bracket: line, bus, line.
const GAP: u16 = 3;

fn truncate(s: &str, w: usize) -> String {
    cells::truncate(s, w)
}

/// The left half of a pill (`<icon> <key>`, the key cut to `key_w` columns) and the right half (`[│#<n>]│<tier>`, `#<n>` from the second Attempt).
fn pill_parts(j: &FlowJob, key_w: usize, frame: usize) -> (String, String) {
    let a = if j.attempt > 1 { format!("│#{}", j.attempt) } else { String::new() };
    (format!("{} {}", j.status.icon(frame).0, truncate(&j.key, key_w)), format!("{}│{}", a, j.tier.map_or_else(|| "—".to_owned(), |t| t.to_string())))
}

/// The needs of each Job (as indices) that no other need already implies: a need reachable through another need is dropped.
fn reduced_needs(jobs: &[FlowJob]) -> Vec<Vec<usize>> {
    let idx: HashMap<&str, usize> = jobs.iter().enumerate().map(|(i, j)| (j.key.as_str(), i)).collect();
    let needs: Vec<Vec<usize>> = jobs.iter().map(|j| j.needs.iter().filter_map(|n| idx.get(n.as_str()).copied()).collect()).collect();
    // everything a Job reaches through its needs (a visited set, so a cycle ends)
    let reach: Vec<HashSet<usize>> = (0..jobs.len())
        .map(|i| {
            let (mut seen, mut todo) = (HashSet::new(), needs[i].clone());
            while let Some(p) = todo.pop() {
                if seen.insert(p) {
                    todo.extend(&needs[p]);
                }
            }
            seen
        })
        .collect();
    needs.iter().map(|ns| ns.iter().copied().filter(|p| !ns.iter().any(|q| q != p && reach[*q].contains(p))).collect()).collect()
}

/// Jobs that need the same Jobs, stacked; `needs` are the cards they need.
struct Card {
    jobs: Vec<usize>,
    needs: Vec<usize>,
}

/// Cards in the Plan's order (a card sits where its first Job does). Jobs with no needs are a card each.
fn cards_of(reduced: &[Vec<usize>]) -> Vec<Card> {
    let mut cards: Vec<Card> = Vec::new();
    let mut by_needs: HashMap<Vec<usize>, usize> = HashMap::new();
    let mut card_of = vec![0; reduced.len()];
    for (i, ns) in reduced.iter().enumerate() {
        let mut key = ns.clone();
        key.sort();
        let c = match by_needs.get(&key) {
            Some(&c) if !key.is_empty() => c,
            _ => {
                cards.push(Card { jobs: Vec::new(), needs: Vec::new() });
                by_needs.insert(key, cards.len() - 1);
                cards.len() - 1
            }
        };
        cards[c].jobs.push(i);
        card_of[i] = c;
    }
    for c in &mut cards {
        let mut needs: Vec<usize> = reduced[c.jobs[0]].iter().map(|&p| card_of[p]).collect();
        needs.sort();
        needs.dedup();
        c.needs = needs;
    }
    cards
}

/// A card's column: one past the deepest card it needs, 0 with no needs. A need that closes a cycle is ignored.
fn column_of(c: usize, cards: &[Card], memo: &mut [Option<usize>], open: &mut HashSet<usize>) -> usize {
    if let Some(l) = memo[c] {
        return l;
    }
    open.insert(c);
    let mut l = 0;
    for &p in &cards[c].needs {
        if !open.contains(&p) {
            l = l.max(column_of(p, cards, memo, open) + 1);
        }
    }
    open.remove(&c);
    memo[c] = Some(l);
    l
}

// ---------- line masks (the Workflow boxes' arrows use them too) ----------
const U: u8 = 1;
const D: u8 = 2;
const L: u8 = 4;
const R: u8 = 8;

pub(super) fn glyph(m: u8) -> &'static str {
    match m {
        3 | 1 | 2 => "│",
        12 | 4 | 8 => "─",
        10 => "╭",
        6 => "╮",
        9 => "╰",
        5 => "╯",
        11 => "├",
        7 => "┤",
        14 => "┬",
        13 => "┴",
        15 => "┼",
        _ => " ",
    }
}

/// Which of the four directions a line leaves each cell in; lines that meet in a cell merge into one junction glyph.
pub(super) struct Mask {
    w: u16,
    cells: Vec<u8>,
}

impl Mask {
    pub(super) fn new(w: u16, h: u16) -> Self {
        Mask { w, cells: vec![0; w as usize * h as usize] }
    }
    fn add(&mut self, x: u16, y: u16, d: u8) {
        let w = self.w as usize;
        self.cells[y as usize * w + x as usize] |= d;
    }
    pub(super) fn at(&self, x: u16, y: u16) -> u8 {
        self.cells[y as usize * self.w as usize + x as usize]
    }
    pub(super) fn h(&mut self, y: u16, x1: u16, x2: u16) {
        let (a, b) = (x1.min(x2), x1.max(x2));
        for x in a..=b {
            if x > a {
                self.add(x, y, L)
            }
            if x < b {
                self.add(x, y, R)
            }
        }
    }
    pub(super) fn v(&mut self, x: u16, y1: u16, y2: u16) {
        let (a, b) = (y1.min(y2), y1.max(y2));
        for y in a..=b {
            if y > a {
                self.add(x, y, U)
            }
            if y < b {
                self.add(x, y, D)
            }
        }
    }
}

/// A straight line of the chart: horizontal on row `y` or vertical in column `x`, ends included.
enum Seg {
    H { y: u16, x1: u16, x2: u16 },
    V { x: u16, y1: u16, y2: u16 },
}

/// The nearest row to `near` that no card takes in columns `cols` (rows below every card are always free).
pub(super) fn free_row(near: u16, cols: std::ops::Range<usize>, busy: &[HashSet<u16>]) -> u16 {
    (0..).flat_map(|d| [near.checked_sub(d), Some(near + d)]).flatten().find(|r| cols.clone().all(|l| !busy[l].contains(r))).unwrap()
}

/// The chart of `all`, a running Job's icon at spinner frame `frame` (every frame is one cell wide: the layout is the same).
/// Where each of `lengths` starts, laid end to end from `start` with `gap` between: what ratatui's `Layout` gives for
/// `Length` constraints that fill their area exactly. Not `Layout` itself: its constraint solver took 4.6 s for a column of
/// 300 cards in a debug build, the plan view's first draw (cli/tests/tui_idle.rs).
fn starts(start: u16, lengths: impl IntoIterator<Item = u16>, gap: u16) -> Vec<u16> {
    lengths
        .into_iter()
        .scan(start, |at, len| {
            let here = *at;
            *at = at.saturating_add(len).saturating_add(gap);
            Some(here)
        })
        .collect()
}

pub fn layout(all: &[FlowJob], frame: usize) -> Chart {
    let jobs = all;
    let cards = cards_of(&reduced_needs(&jobs));
    let col_of: Vec<usize> = {
        let mut memo = vec![None; cards.len()];
        (0..cards.len()).map(|c| column_of(c, &cards, &mut memo, &mut HashSet::new())).collect()
    };
    let ncols = col_of.iter().max().map_or(0, |m| m + 1);
    let mut cols: Vec<Vec<usize>> = vec![Vec::new(); ncols];
    for c in 0..cards.len() {
        cols[col_of[c]].push(c);
    }

    // x: the left bracket column, then each column's pills, `GAP` + both bracket columns between columns
    let parts: Vec<(String, String)> = jobs.iter().map(|j| pill_parts(j, usize::MAX, frame)).collect();
    let pill_w = |i: usize| (Line::from(parts[i].0.as_str()).width() + Line::from(parts[i].1.as_str()).width()) as u16;
    // a column's live pills are as wide as the widest of them; a cancelled Job's is as wide as itself, so it never widens the
    // others
    let live_w: Vec<u16> = cols
        .iter()
        .map(|cs| cs.iter().flat_map(|&c| &cards[c].jobs).filter(|&&i| jobs[i].status != Live::Cancelled).map(|&i| pill_w(i) + pill::CAPS_COLUMNS).max().unwrap_or(0))
        .collect();
    let rect_w = |i: usize, l: usize| if jobs[i].status == Live::Cancelled { pill_w(i) + pill::CAPS_COLUMNS } else { live_w[l] };
    let col_w: Vec<u16> = cols.iter().enumerate().map(|(l, cs)| cs.iter().flat_map(|&c| &cards[c].jobs).map(|&i| rect_w(i, l)).max().unwrap_or(0)).collect();
    let spacing = GAP + 2;
    let inner_w = col_w.iter().sum::<u16>() + spacing * ncols.saturating_sub(1) as u16;
    let xs: Vec<u16> = starts(0, col_w.iter().copied(), spacing);
    // the chart pads nothing: its container does (`BoxWidget`). Only the right bracket of a card of several Jobs in the last
    // column lies past the pills, one column on
    let bracketed = ncols > 0 && cols[ncols - 1].iter().any(|&c| cards[c].jobs.len() > 1);
    let width = if ncols == 0 { 0 } else { inner_w + u16::from(bracketed) };

    // y: each column's cards stacked (one empty row between cards), centred in the tallest column, ordered by the mean
    // row of the cards they need (ties: the Plan's order)
    let height_of = |c: usize| cards[c].jobs.len() as u16;
    let hmax = cols.iter().map(|cs| cs.iter().map(|&c| height_of(c)).sum::<u16>() + cs.len().saturating_sub(1) as u16).max().unwrap_or(0);
    let (mut top, mut mid) = (vec![0u16; cards.len()], vec![0u16; cards.len()]);
    for l in 0..ncols {
        let barycentre = |c: &usize| match &cards[*c].needs[..] {
            [] => 0.0,
            ns => ns.iter().map(|&p| mid[p] as f64).sum::<f64>() / ns.len() as f64,
        };
        let mut ordered = cols[l].clone();
        ordered.sort_by(|a, b| barycentre(a).total_cmp(&barycentre(b)));
        cols[l] = ordered;
        let stack = cols[l].iter().map(|&c| height_of(c)).sum::<u16>() + cols[l].len() as u16 - 1;
        // centred; the spare row of an odd difference goes below (ratatui's own centring would put it above, and the
        // accepted render has it below)
        let ys = starts((hmax - stack) / 2, cols[l].iter().map(|&c| height_of(c)), 1);
        for (&c, y) in cols[l].iter().zip(ys) {
            (top[c], mid[c]) = (y, y + (height_of(c) - 1) / 2);
        }
    }

    // lines: one bus per gap; a line that skips columns rides a free row from its own bus to the bus before its target.
    // Needs that do not point left (a cycle) are not drawn.
    let edges: Vec<(usize, usize)> = (0..cards.len()).flat_map(|c| cards[c].needs.iter().map(move |&p| (p, c))).filter(|&(p, c)| col_of[p] < col_of[c]).collect();
    let busy: Vec<HashSet<u16>> = cols.iter().map(|cs| cs.iter().flat_map(|&c| top[c]..top[c] + height_of(c)).collect()).collect();
    let right = |c: usize| xs[col_of[c]] + cards[c].jobs.iter().map(|&i| rect_w(i, col_of[c])).max().unwrap_or(0); // the right bracket's column: the cell after the card's own widest pill
    let bus = |l: usize| xs[l] + col_w[l] + 2;
    let mut segs: Vec<Seg> = Vec::new();
    let mut bus_rows: Vec<Vec<u16>> = vec![Vec::new(); ncols];
    for &(p, c) in &edges {
        let (lp, lc) = (col_of[p], col_of[c]);
        segs.push(Seg::H { y: mid[p], x1: right(p), x2: bus(lp) });
        bus_rows[lp].push(mid[p]);
        if lc > lp + 1 {
            let y = free_row(mid[p], lp + 1..lc, &busy);
            segs.push(Seg::H { y, x1: bus(lp), x2: bus(lc - 1) });
            bus_rows[lp].push(y);
            bus_rows[lc - 1].push(y);
        }
        bus_rows[lc - 1].push(mid[c]);
        segs.push(Seg::H { y: mid[c], x1: bus(lc - 1), x2: xs[lc] - 1 });
    }
    for (l, rows) in bus_rows.iter().enumerate() {
        if let (Some(&a), Some(&b)) = (rows.iter().min(), rows.iter().max()) {
            segs.push(Seg::V { x: bus(l), y1: a, y2: b });
        }
    }
    let height = segs.iter().map(|s| match s { Seg::H { y, .. } => *y + 1, Seg::V { y2, .. } => *y2 + 1 }).fold(hmax, u16::max);
    let mut mask = Mask::new(width, height);
    for s in &segs {
        match *s {
            Seg::H { y, x1, x2 } => mask.h(y, x1, x2),
            Seg::V { x, y1, y2 } => mask.v(x, y1, y2),
        }
    }
    // entry and exit: a card of several Jobs is held by a bracket on each side, joined at the middle row; a single
    // Job's are plain line ends
    for c in 0..cards.len() {
        let (first, last) = (top[c], top[c] + height_of(c) - 1);
        // column 0 has no entry (no needs), so its `lx` is never drawn at
        let (lx, rx) = (xs[col_of[c]].saturating_sub(1), right(c));
        if first == last {
            if edges.iter().any(|e| e.1 == c) {
                mask.add(lx, first, R);
            }
            if edges.iter().any(|e| e.0 == c) {
                mask.add(rx, first, L);
            }
            continue;
        }
        for y in first..=last {
            let vertical = (if y > first { U } else { 0 }) | (if y < last { D } else { 0 });
            let (open_l, open_r) = if y == first || y == last { (R, L) } else { (0, 0) };
            mask.add(lx, y, vertical | open_l);
            mask.add(rx, y, vertical | open_r);
        }
    }

    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    let lines = palette::muted_text();
    for y in 0..height {
        for x in 0..width {
            if mask.at(x, y) != 0 {
                buf[(x, y)].set_symbol(glyph(mask.at(x, y))).set_style(lines);
            }
        }
    }
    let mut pills: Vec<Option<Pill>> = (0..jobs.len()).map(|_| None).collect();
    let mut columns: Vec<Vec<usize>> = vec![Vec::new(); ncols];
    for (l, cs) in cols.iter().enumerate() {
        for &c in cs {
            for (k, &i) in cards[c].jobs.iter().enumerate() {
                let rect = Rect::new(xs[l], top[c] + k as u16, rect_w(i, l), 1);
                // a cap on each side in the status colour, on the default background; between them the icon and key to
                // the left, the attempt label and tier to the right, whatever the column's width. A cancelled Job's is colour 8
                let (left, right) = (parts[i].0.as_str(), parts[i].1.as_str());
                let gap = body_of(rect).width as usize - Line::from(left).width() - Line::from(right).width();
                Line::from(pill::pill(format!("{left}{}{right}", " ".repeat(gap)), jobs[i].status.pill())).render(rect, &mut buf);
                pills[i] = Some(Pill { key: jobs[i].key.clone(), rect });
                columns[l].push(i);
            }
        }
    }
    Chart { buf, pills: pills.into_iter().flatten().collect(), columns }
}

/// A pill's rectangle without its two caps.
fn body_of(rect: Rect) -> Rect {
    Rect { x: rect.x + 1, width: rect.width.saturating_sub(2), ..rect }
}

impl Chart {
    /// The same chart on a larger canvas: `margin` blank columns left and right and rows above and below it. The buffer and
    /// the pills' rectangles are on the canvas, so scrolling and the box lines share its coordinates.
    pub fn with_margin(self, margin: Margin) -> Chart {
        let (dx, dy) = (margin.horizontal, margin.vertical);
        let area = self.buf.area;
        let mut buf = Buffer::empty(Rect::new(0, 0, area.width + 2 * dx, area.height + 2 * dy));
        copy_cells(&self.buf, area, &mut buf, Position::new(dx, dy));
        let pills = self.pills.into_iter().map(|p| Pill { rect: Rect { x: p.rect.x + dx, y: p.rect.y + dy, ..p.rect }, ..p }).collect();
        Chart { buf, pills, columns: self.columns }
    }

    /// Copy the window at (`ox`, `oy`) into `dst` at `area`. Offsets are clamped to the chart; the clamped offsets are
    /// returned. (Hand-written: ratatui has no widget that copies a window of a `Buffer`.)
    pub fn blit(&self, dst: &mut Buffer, area: Rect, (ox, oy): (u16, u16)) -> (u16, u16) {
        let ox = ox.min(self.buf.area.width.saturating_sub(area.width));
        let oy = oy.min(self.buf.area.height.saturating_sub(area.height));
        copy_cells(&self.buf, Rect::new(ox, oy, area.width, area.height), dst, Position::new(area.x, area.y));
        (ox, oy)
    }
}

#[cfg(test)]
#[path = "flow_width_tests.rs"]
mod flow_width_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_text::rows;

    static CAP_L: &str = "\u{e0b6}";
    static CAP_R: &str = "\u{e0b4}";

    fn job(key: &str, needs: &[&str]) -> FlowJob {
        FlowJob {
            key: key.into(),
            status: Live::Queued,
            tier: Some(Tier::parse("S1").unwrap()),
            needs: needs.iter().map(|s| s.to_string()).collect(),
            attempt: 1,
        }
    }

    fn text(c: &Chart) -> String {
        rows(&c.buf).join("\n")
    }

    /// The chart's text against `want`, written below an opening newline that is not part of it.
    fn snap(c: &Chart, want: &str) {
        assert_eq!(text(c), want.strip_prefix('\n').unwrap());
    }

    #[test]
    fn a_pill_has_the_icon_and_colour_of_its_status() {
        let statuses = [Live::Draft, Live::Queued, Live::Running, Live::Merging, Live::Merged, Live::Failed, Live::FailedWorked, Live::Cancelled];
        let jobs: Vec<FlowJob> = statuses.iter().map(|s| FlowJob { status: *s, ..job(&format!("{s:?}"), &[]) }).collect();
        let c = layout(&jobs, 0);
        for (p, status) in c.pills.iter().zip(statuses) {
            let (icon, _meaning) = status.icon(0);
            assert_eq!(c.buf[(p.rect.x + 1, p.rect.y)].symbol(), icon, "{status:?}");
            // the body keeps the pill's colours
            let cell = &c.buf[(p.rect.x + 1, p.rect.y)];
            assert_eq!((Some(cell.fg), Some(cell.bg)), (status.pill().fg, status.pill().bg), "{status:?}");
            // a rounded cap on each end: the status colour on the default background
            for (x, glyph) in [(p.rect.x, "\u{e0b6}"), (p.rect.right() - 1, "\u{e0b4}")] {
                let cap = &c.buf[(x, p.rect.y)];
                assert_eq!((cap.symbol(), Some(cap.fg), cap.bg), (glyph, status.pill().fg, ratatui::buffer::Cell::default().bg), "{status:?} cap at {x}");
            }
        }
    }

    /// The cells of the pill at `i`, caps included.
    fn cells(c: &Chart, i: usize) -> String {
        let r = c.pills[i].rect;
        (r.x..r.right()).map(|x| c.buf[(x, r.y)].symbol()).collect()
    }

    #[test]
    fn a_pill_is_cap_icon_key_bar_tier_cap_with_no_padding() {
        // hs-d474: no space inside the caps and none before the `│`
        let c = layout(&[job("titles-docs-2", &[])], 0);
        let icon = Live::Queued.icon(0).0;
        assert_eq!(cells(&c, 0), format!("{CAP_L}{icon} titles-docs-2│S1{CAP_R}"));
        assert_eq!(c.pills[0].rect.width as usize, 2 + icon.chars().count() + " titles-docs-2│S1".chars().count());
    }

    #[test]
    fn a_stub_draft_pill_shows_a_dash_for_its_tier_with_no_padding() {
        let c = layout(&[FlowJob { status: Live::Draft, tier: None, ..job("a", &[]) }], 0);
        assert_eq!(cells(&c, 0), format!("{CAP_L}{} a│—{CAP_R}", Live::Draft.icon(0).0));
    }

    #[test]
    fn the_widest_pill_of_a_column_fills_it_and_the_others_pad_between_key_and_bar() {
        let c = layout(&[job("a", &[]), job("long", &[])], 0);
        let icon = Live::Queued.icon(0).0;
        assert_eq!(cells(&c, 0), format!("{CAP_L}{icon} a   │S1{CAP_R}"));
        assert_eq!(cells(&c, 1), format!("{CAP_L}{icon} long│S1{CAP_R}"));
    }

    /// The icon of the first pill of a chart of one Job in `status`, at `frame`.
    fn icon_at(status: Live, frame: usize) -> String {
        let c = layout(&[FlowJob { status, ..job("a", &[]) }], frame);
        c.buf[(c.pills[0].rect.x + 1, c.pills[0].rect.y)].symbol().to_string()
    }

    #[test]
    fn a_running_pill_spins_through_the_28_moon_phases() {
        let icons: Vec<String> = (0..=palette::SPINNER.len()).map(|f| icon_at(Live::Running, f)).collect();
        assert_eq!(icons, [palette::SPINNER.to_vec(), vec![palette::SPINNER[0]]].concat());
    }

    #[test]
    fn a_merging_and_a_queued_pill_do_not_spin() {
        for f in 0..palette::SPINNER.len() {
            assert_eq!(icon_at(Live::Merging, f), palette::MERGING, "frame {f}");
            assert_eq!(icon_at(Live::Queued, f), palette::QUEUED, "frame {f}");
        }
    }

    #[test]
    fn the_layout_is_the_same_at_every_frame() {
        let jobs = [
            FlowJob { status: Live::Running, ..job("a", &[]) },
            FlowJob { status: Live::Running, ..job("b", &["a"]) },
            FlowJob { status: Live::Merging, ..job("c", &["a"]) },
            job("d", &["b", "c"]),
        ];
        let first = layout(&jobs, 0);
        for f in 1..4 {
            let c = layout(&jobs, f);
            assert_eq!(c.buf.area, first.buf.area, "frame {f}");
            assert_eq!(c.columns, first.columns, "frame {f}");
            let rects = |c: &Chart| c.pills.iter().map(|p| (p.key.clone(), p.rect)).collect::<Vec<_>>();
            assert_eq!(rects(&c), rects(&first), "frame {f}");
            // only the two running pills' icons differ
            let differing = |c: &Chart| first.buf.content.iter().zip(&c.buf.content).filter(|(x, y)| x != y).count();
            assert_eq!(differing(&c), 2, "frame {f}");
        }
    }

    #[test]
    fn lines_are_dim_in_the_nothing_colour() {
        let c = layout(&[job("a", &[]), job("b", &["a"])], 0);
        let line = &c.buf[(c.pills[0].rect.right() + 1, 0)];
        assert_eq!((line.symbol(), Some(line.fg)), ("─", palette::fg(Meaning::Nothing).fg));
    }

    #[test]
    fn columns_run_left_to_right() {
        let c = layout(&[job("a", &[]), job("b", &["a"]), job("c", &["b"])], 0);
        snap(&c, "
󰝦 a│S1─────󰝦 b│S1─────󰝦 c│S1");
        assert_eq!(c.columns, vec![vec![0], vec![1], vec![2]]);
    }

    #[test]
    fn an_implied_need_is_not_drawn() {
        // c needs a through b, so its own need of a is dropped: a has one line out, to b
        let c = layout(&[job("a", &[]), job("b", &["a"]), job("c", &["b", "a"])], 0);
        snap(&c, "
󰝦 a│S1─────󰝦 b│S1─────󰝦 c│S1");
        // the same with the implied need listed first
        assert_eq!(text(&layout(&[job("a", &[]), job("b", &["a"]), job("c", &["a", "b"])], 0)), text(&c));
    }

    #[test]
    fn jobs_with_the_same_needs_share_a_card_stacked_with_no_space() {
        let c = layout(&[job("a", &[]), job("b", &["a"]), job("c", &["a"]), job("d", &["a"])], 0);
        snap(&c, "
            ╭󰝦 b│S1╮
󰝦 a│S1────┤󰝦 c│S1│
            ╰󰝦 d│S1╯");
        assert_eq!(c.pills.iter().map(|p| p.rect.y).collect::<Vec<_>>(), [1, 0, 1, 2]);
    }

    #[test]
    fn a_card_has_one_entry_and_one_exit_at_its_middle_row() {
        // three Jobs: both brackets are joined at the middle row, where the line in and the line out meet them
        let c = layout(&[job("a", &[]), job("b", &["a"]), job("c", &["a"]), job("e", &["a"]), job("d", &["b", "c", "e"])], 0);
        snap(&c, "
            ╭󰝦 b│S1╮
󰝦 a│S1────┤󰝦 c│S1├────󰝦 d│S1
            ╰󰝦 e│S1╯");
        // two Jobs: the middle row is the upper one
        let c = layout(&[job("a", &[]), job("b", &["a"]), job("c", &["a"])], 0);
        snap(&c, "
󰝦 a│S1────┬󰝦 b│S1╮
            ╰󰝦 c│S1╯");
    }

    #[test]
    fn single_cards_are_one_row_apart_and_have_plain_line_ends() {
        let c = layout(&[job("a", &[]), job("b", &[]), job("c", &["a"]), job("d", &["b"])], 0);
        snap(&c, "
󰝦 a│S1──┬──󰝦 c│S1
          │
󰝦 b│S1──┴──󰝦 d│S1");
    }

    #[test]
    fn one_bus_per_gap_joins_every_exit_and_every_entry() {
        let c = layout(&[job("a", &[]), job("b", &[]), job("c", &["a"]), job("d", &["b"])], 0);
        let bus_x = c.pills[0].rect.right() + 2;
        // the bus is one column, with a junction for each line in and out
        let glyphs: Vec<&str> = (0..c.buf.area.height).map(|y| c.buf[(bus_x, y)].symbol()).collect();
        assert_eq!(glyphs, ["┬", "│", "┴"]);
        let c = layout(&[job("a", &[]), job("b", &[]), job("c", &["a", "b"])], 0);
        snap(&c, "
󰝦 a│S1──╮
          ├──󰝦 c│S1
󰝦 b│S1──╯");
    }

    #[test]
    fn a_line_that_skips_columns_rides_a_free_row() {
        let c = layout(&[job("a", &[]), job("x", &[]), job("b", &["a"]), job("c", &["b", "x"])], 0);
        snap(&c, "
󰝦 a│S1──╮
          ├──󰝦 b│S1──┬──󰝦 c│S1
󰝦 x│S1──┴────────────╯");
    }

    #[test]
    fn a_column_is_centred_vertically() {
        let c = layout(&[job("a", &[]), job("b", &[]), job("c", &[]), job("d", &["a", "b", "c"])], 0);
        assert_eq!(c.pills[3].rect.y, 2);
        // an odd difference leaves the spare row below
        let c = layout(&[job("a", &[]), job("b", &[]), job("d", &["a", "b"]), job("e", &["a", "b"]), job("f", &["d"])], 0);
        assert_eq!(c.pills.iter().map(|p| p.rect.y).collect::<Vec<_>>(), [0, 2, 0, 1, 1]);
    }

    #[test]
    fn a_column_is_ordered_by_the_mean_row_of_the_cards_it_needs() {
        // d needs the lower root, c the upper one: the order of the Plan (d first) would cross them
        let c = layout(&[job("a", &[]), job("b", &[]), job("d", &["b"]), job("c", &["a"])], 0);
        assert_eq!(c.columns[1], vec![3, 2]);
    }

    #[test]
    fn a_cycle_is_laid_out_without_panicking() {
        let c = layout(&[job("a", &["b"]), job("b", &["a"]), job("c", &["c"])], 0);
        assert_eq!(c.pills.len(), 3);
        assert!(c.columns.iter().all(|col| !col.is_empty()));
    }

    #[test]
    fn an_empty_workflow_is_an_empty_chart() {
        let c = layout(&[], 0);
        assert_eq!((c.pills.len(), c.columns.len(), c.buf.area.area()), (0, 0, 0));
    }

    fn two() -> Chart {
        layout(&[job("a", &[]), job("b", &["a"])], 0)
    }

    #[test]
    fn the_window_is_clamped_to_the_chart_and_copied_as_drawn() {
        let jobs: Vec<FlowJob> = ["a", "b", "c", "d"].iter().map(|k| job(k, &[])).collect();
        let c = layout(&jobs, 0);
        let mut dst = Buffer::empty(Rect::new(0, 0, 30, 3));
        let (ox, oy) = c.blit(&mut dst, Rect::new(2, 1, 20, 1), (0, 100)); // clamped to the chart
        assert_eq!((ox, oy), (0, 6));
        assert_eq!(dst[(4, 1)].symbol(), c.buf[(2, 6)].symbol());
        assert_eq!(dst[(4, 1)].style(), c.buf[(2, 6)].style(), "no pill is marked");
        assert_eq!(dst[(3, 0)].symbol(), " "); // the padding is untouched
    }

    #[test]
    fn a_margin_moves_the_buffer_and_the_pills_together() {
        let plain = two();
        let c = two().with_margin(Margin { horizontal: 2, vertical: 1 });
        assert_eq!((c.buf.area.width, c.buf.area.height), (plain.buf.area.width + 4, plain.buf.area.height + 2));
        for (p, q) in c.pills.iter().zip(&plain.pills) {
            assert_eq!((p.rect.x, p.rect.y), (q.rect.x + 2, q.rect.y + 1));
            assert_eq!(c.buf[(p.rect.x + 2, p.rect.y)].symbol(), plain.buf[(q.rect.x + 2, q.rect.y)].symbol());
        }
    }
}
