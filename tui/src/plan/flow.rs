//! The plan view's chart: a pure layout of a Plan's Jobs as GitHub-Actions-style cards, left to right.
//!
//! In: Jobs. Out: a `Buffer` of the whole chart (no padding: the view insets the window it blits into) plus each pill's
//! rectangle. Implied needs are dropped (transitive reduction); Jobs with the same needs share one card, stacked; a card's
//! column is one past the deepest column it needs; each card has one entry and one exit at its middle row, joined by one
//! vertical bus per gap between columns, lines that skip columns riding a row free of cards.
//!
//! Hand-written drawing is only the line glyphs (`Mask`) and the window copy (`Chart::blit`): ratatui has no widget for
//! box-drawing edges that merge into junctions, nor for copying a window of a `Buffer`. Positions come from ratatui's
//! `Layout`; pills are `Line`s aligned left and right.

use std::collections::{HashMap, HashSet};

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::Widget,
};

use crate::palette::{self, Meaning};
use cpd_core::config::Tier;
use crate::read::{AttemptStatus, Flow, JobNode, JobStatus};

/// A Job's status as the chart draws it. A cancelled Job is not drawn, so it has none; a `running` Job whose Attempt is
/// `merging` is `Merging` (the cockpit's `in_review`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Live {
    Draft,
    Queued,
    Running,
    Merging,
    Merged,
    Failed,
}

impl Live {
    /// The icon and meaning the palette gives this status.
    pub fn icon(self) -> (&'static str, Meaning) {
        match self {
            Live::Draft => palette::job_status(JobStatus::Draft, false),
            Live::Queued => palette::job_status(JobStatus::Queued, false),
            Live::Running => palette::job_status(JobStatus::Running, false),
            Live::Merging => palette::job_status(JobStatus::Running, true),
            Live::Merged => palette::job_status(JobStatus::Merged, false),
            Live::Failed => palette::job_status(JobStatus::Failed, false),
        }
    }

    /// `None` for a cancelled Job.
    pub fn of(job: &JobNode) -> Option<Live> {
        Some(match job.status {
            JobStatus::Draft => Live::Draft,
            JobStatus::Queued => Live::Queued,
            JobStatus::Running if job.active_attempt().is_some_and(|a| a.status == AttemptStatus::Merging) => Live::Merging,
            JobStatus::Running => Live::Running,
            JobStatus::Merged => Live::Merged,
            JobStatus::Failed => Live::Failed,
            JobStatus::Cancelled => return None,
        })
    }
}

/// What the layout needs to know of a Job.
#[derive(Clone, Debug)]
pub struct FlowJob {
    pub key: String,
    pub status: Live,
    pub tier: Tier,
    pub needs: Vec<String>,
    /// How many Attempts the Job has had; the pill shows `aN` from the second on.
    pub attempt: usize,
}

pub struct Pill {
    pub key: String,
    pub rect: Rect,
    pub column: usize,
}

/// The Jobs of a Plan as the chart draws them, in creation order: cancelled Jobs are left out, with the needs on them.
pub fn flow_jobs(flow: &Flow) -> Vec<FlowJob> {
    let live: Vec<(&JobNode, Live)> = flow.jobs.iter().filter_map(|j| Some((j, Live::of(j)?))).collect();
    live.iter()
        .map(|&(j, status)| FlowJob {
            key: j.key.to_string(),
            status,
            tier: j.tier,
            needs: j.needs.iter().filter(|n| live.iter().any(|(l, _)| &l.key == *n)).map(|n| n.to_string()).collect(),
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

#[derive(Clone, Copy)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Columns between a column's right bracket and the next column's left bracket: line, bus, line.
const GAP: u16 = 3;

fn truncate(s: &str, w: usize) -> String {
    if s.chars().count() <= w {
        s.to_string()
    } else {
        s.chars().take(w.saturating_sub(1)).collect::<String>() + "…"
    }
}

/// The left half of a pill (` <icon> <key> `, the key cut to `key_w` characters) and the right half (`<tier>[ aN] `).
fn pill_parts(j: &FlowJob, key_w: usize) -> (String, String) {
    let a = if j.attempt > 1 { format!(" a{}", j.attempt) } else { String::new() };
    (format!(" {} {} ", j.status.icon().0, truncate(&j.key, key_w)), format!("{}{} ", j.tier, a))
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

// ---------- line masks ----------
const U: u8 = 1;
const D: u8 = 2;
const L: u8 = 4;
const R: u8 = 8;

fn glyph(m: u8) -> &'static str {
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
struct Mask {
    w: u16,
    cells: Vec<u8>,
}

impl Mask {
    fn new(w: u16, h: u16) -> Self {
        Mask { w, cells: vec![0; w as usize * h as usize] }
    }
    fn add(&mut self, x: u16, y: u16, d: u8) {
        let w = self.w as usize;
        self.cells[y as usize * w + x as usize] |= d;
    }
    fn at(&self, x: u16, y: u16) -> u8 {
        self.cells[y as usize * self.w as usize + x as usize]
    }
    fn h(&mut self, y: u16, x1: u16, x2: u16) {
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
    fn v(&mut self, x: u16, y1: u16, y2: u16) {
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
fn free_row(near: u16, cols: std::ops::Range<usize>, busy: &[HashSet<u16>]) -> u16 {
    (0..).flat_map(|d| [near.checked_sub(d), Some(near + d)]).flatten().find(|r| cols.clone().all(|l| !busy[l].contains(r))).unwrap()
}

pub fn layout(all: &[FlowJob]) -> Chart {
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
    let parts: Vec<(String, String)> = jobs.iter().map(|j| pill_parts(j, usize::MAX)).collect();
    let pill_w = |i: usize| (Line::from(parts[i].0.as_str()).width() + Line::from(parts[i].1.as_str()).width()) as u16;
    let col_w: Vec<u16> = cols.iter().map(|cs| cs.iter().flat_map(|&c| &cards[c].jobs).map(|&i| pill_w(i)).max().unwrap_or(0)).collect();
    let spacing = GAP + 2;
    let inner_w = col_w.iter().sum::<u16>() + spacing * ncols.saturating_sub(1) as u16;
    let xs: Vec<u16> = Layout::horizontal(col_w.iter().map(|&w| Constraint::Length(w))).spacing(spacing).split(Rect::new(1, 0, inner_w, 1)).iter().map(|r| r.x).collect();
    let width = if ncols == 0 { 0 } else { inner_w + 2 };

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
        let column = Rect::new(0, (hmax - stack) / 2, 1, stack);
        let rows = Layout::vertical(cols[l].iter().map(|&c| Constraint::Length(height_of(c)))).spacing(1).split(column);
        for (&c, r) in cols[l].iter().zip(rows.iter()) {
            (top[c], mid[c]) = (r.y, r.y + (r.height - 1) / 2);
        }
    }

    // lines: one bus per gap; a line that skips columns rides a free row from its own bus to the bus before its target.
    // Needs that do not point left (a cycle) are not drawn.
    let edges: Vec<(usize, usize)> = (0..cards.len()).flat_map(|c| cards[c].needs.iter().map(move |&p| (p, c))).filter(|&(p, c)| col_of[p] < col_of[c]).collect();
    let busy: Vec<HashSet<u16>> = cols.iter().map(|cs| cs.iter().flat_map(|&c| top[c]..top[c] + height_of(c)).collect()).collect();
    let right = |c: usize| xs[col_of[c]] + col_w[col_of[c]]; // the right bracket's column
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
        let (lx, rx) = (xs[col_of[c]] - 1, right(c));
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
    let lines = palette::fg(Meaning::Nothing);
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
                let rect = Rect::new(xs[l], top[c] + k as u16, col_w[l], 1);
                let style = palette::pill(jobs[i].status.icon().1);
                // the icon and key to the left, the tier (and `aN`) to the right, whatever the column's width
                Line::from(parts[i].0.as_str()).style(style).left_aligned().render(rect, &mut buf);
                Line::from(parts[i].1.as_str()).style(style).right_aligned().render(rect, &mut buf);
                pills[i] = Some(Pill { key: jobs[i].key.clone(), rect, column: l });
                columns[l].push(i);
            }
        }
    }
    Chart { buf, pills: pills.into_iter().flatten().collect(), columns }
}

/// The User's selection in the Plan view: a Job key, held by key so a refresh keeps it. Nothing is selected until the User
/// clicks a pill or presses an arrow key. `follow` asks the next draw to scroll the selection into view (set by keys only:
/// a clicked pill is already visible).
#[derive(Default, Debug, PartialEq)]
pub struct Selection {
    pub key: Option<String>,
    pub follow: bool,
}

impl Selection {
    /// Nothing selected (opening a view, switching Plan).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// A refresh: the pill index of the held key in `chart`. A Job that is gone drops the selection; none is ever created.
    pub fn resolve(&mut self, chart: &Chart) -> Option<usize> {
        let i = self.key.as_ref().and_then(|k| chart.pills.iter().position(|p| &p.key == k));
        if i.is_none() {
            self.reset();
        }
        i
    }

    /// An arrow key: with nothing selected the first pill of the first column, else one step from the selection.
    pub fn arrow(&mut self, chart: &Chart, dir: Dir) {
        let to = match self.resolve(chart) {
            Some(i) => Some(chart.step(i, dir)),
            None => chart.columns.first().and_then(|c| c.first().copied()),
        };
        if let Some(i) = to {
            self.key = Some(chart.pills[i].key.clone());
            self.follow = true;
        }
    }

    /// A click on the pill `key`: it is already in view, so no scrolling follows. Clicking the selected pill clears it.
    pub fn click(&mut self, key: String) {
        if self.key.as_ref() == Some(&key) {
            self.reset();
        } else {
            self.key = Some(key);
            self.follow = false;
        }
    }

    /// A click on empty chart space, or Esc: nothing is selected. Returns the Job that was selected, whose Worker view closes.
    pub fn clear(&mut self) -> Option<String> {
        let was = self.key.take();
        self.reset();
        was
    }

    /// Enter: the key to open, none when nothing is selected.
    pub fn enter(&self) -> Option<String> {
        self.key.clone()
    }
}

impl Chart {
    pub fn pill_at(&self, x: u16, y: u16) -> Option<usize> {
        self.pills.iter().position(|p| x >= p.rect.x && x < p.rect.right() && y == p.rect.y)
    }

    /// The neighbour of pill `from`: up/down the previous/next Job of its column, left/right the nearest Job (by row) of the
    /// previous/next column. Stays put at an edge.
    pub fn step(&self, from: usize, dir: Dir) -> usize {
        let column = self.pills[from].column;
        let col = &self.columns[column];
        let at = col.iter().position(|&i| i == from).unwrap_or(0);
        let nearest = |l: usize| self.columns[l].iter().copied().min_by_key(|&i| self.pills[i].rect.y.abs_diff(self.pills[from].rect.y)).unwrap_or(from);
        match dir {
            Dir::Up => col[at.saturating_sub(1)],
            Dir::Down => col[(at + 1).min(col.len() - 1)],
            Dir::Left if column > 0 => nearest(column - 1),
            Dir::Right if column + 1 < self.columns.len() => nearest(column + 1),
            _ => from,
        }
    }

    /// The smallest scroll offset change that shows pill `i` and its entry and exit in a `w`×`h` window.
    pub fn scroll_to(&self, i: usize, (ox, oy): (u16, u16), (w, h): (u16, u16)) -> (u16, u16) {
        let r = self.pills[i].rect;
        let fit = |o: u16, lo: u16, hi: u16, size: u16| if lo < o { lo } else if hi > o + size { hi.saturating_sub(size) } else { o };
        (fit(ox, r.x.saturating_sub(1), r.right() + 1, w), fit(oy, r.y, r.bottom(), h))
    }

    /// Copy the window at (`ox`, `oy`) into `dst` at `area`, the selected pill bold and reversed. Offsets are clamped to
    /// the chart; the clamped offsets are returned. (Hand-written: ratatui has no widget that copies a window of a `Buffer`.)
    pub fn blit(&self, dst: &mut Buffer, area: Rect, (ox, oy): (u16, u16), selected: Option<usize>) -> (u16, u16) {
        let ox = ox.min(self.buf.area.width.saturating_sub(area.width));
        let oy = oy.min(self.buf.area.height.saturating_sub(area.height));
        for y in 0..area.height.min(self.buf.area.height - oy) {
            for x in 0..area.width.min(self.buf.area.width - ox) {
                dst[(area.x + x, area.y + y)] = self.buf[(x + ox, y + oy)].clone();
            }
        }
        if let Some(r) = selected.map(|i| self.pills[i].rect) {
            let seen = Rect::new(ox, oy, area.width, area.height).intersection(r);
            if !seen.is_empty() {
                let on_dst = Rect::new(area.x + seen.x - ox, area.y + seen.y - oy, seen.width, seen.height);
                dst.set_style(on_dst, Style::new().add_modifier(Modifier::BOLD | Modifier::REVERSED));
            }
        }
        (ox, oy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(key: &str, needs: &[&str]) -> FlowJob {
        FlowJob {
            key: key.into(),
            status: Live::Queued,
            tier: Tier::parse("S1").unwrap(),
            needs: needs.iter().map(|s| s.to_string()).collect(),
            attempt: 1,
        }
    }

    fn text(c: &Chart) -> String {
        (0..c.buf.area.height)
            .map(|y| (0..c.buf.area.width).map(|x| c.buf[(x, y)].symbol()).collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The chart's text against `want`, written below an opening newline that is not part of it.
    fn snap(c: &Chart, want: &str) {
        assert_eq!(text(c), want.strip_prefix('\n').unwrap());
    }

    #[test]
    fn a_pill_has_the_icon_and_colour_of_its_status() {
        let statuses = [Live::Draft, Live::Queued, Live::Running, Live::Merging, Live::Merged, Live::Failed];
        let jobs: Vec<FlowJob> = statuses.iter().map(|s| FlowJob { status: *s, ..job(&format!("{s:?}"), &[]) }).collect();
        let c = layout(&jobs);
        for (p, status) in c.pills.iter().zip(statuses) {
            let (icon, meaning) = status.icon();
            assert_eq!(c.buf[(p.rect.x + 1, p.rect.y)].symbol(), icon, "{status:?}");
            let cell = &c.buf[(p.rect.x, p.rect.y)];
            assert_eq!((Some(cell.fg), Some(cell.bg)), (palette::pill(meaning).fg, palette::pill(meaning).bg), "{status:?}");
        }
    }

    #[test]
    fn lines_are_dim_in_the_nothing_colour() {
        let c = layout(&[job("a", &[]), job("b", &["a"])]);
        let line = &c.buf[(c.pills[0].rect.right() + 1, 0)];
        assert_eq!((line.symbol(), Some(line.fg)), ("─", palette::fg(Meaning::Nothing).fg));
    }

    #[test]
    fn columns_run_left_to_right() {
        let c = layout(&[job("a", &[]), job("b", &["a"]), job("c", &["b"])]);
        snap(&c, "
  ○ a S1 ───── ○ b S1 ───── ○ c S1");
        assert_eq!(c.columns, vec![vec![0], vec![1], vec![2]]);
    }

    #[test]
    fn an_implied_need_is_not_drawn() {
        // c needs a through b, so its own need of a is dropped: a has one line out, to b
        let c = layout(&[job("a", &[]), job("b", &["a"]), job("c", &["b", "a"])]);
        snap(&c, "
  ○ a S1 ───── ○ b S1 ───── ○ c S1");
        // the same with the implied need listed first
        assert_eq!(text(&layout(&[job("a", &[]), job("b", &["a"]), job("c", &["a", "b"])])), text(&c));
    }

    #[test]
    fn jobs_with_the_same_needs_share_a_card_stacked_with_no_space() {
        let c = layout(&[job("a", &[]), job("b", &["a"]), job("c", &["a"]), job("d", &["a"])]);
        snap(&c, "
             ╭ ○ b S1 ╮
  ○ a S1 ────┤ ○ c S1 │
             ╰ ○ d S1 ╯");
        assert_eq!(c.pills.iter().map(|p| p.rect.y).collect::<Vec<_>>(), [1, 0, 1, 2]);
    }

    #[test]
    fn a_card_has_one_entry_and_one_exit_at_its_middle_row() {
        // three Jobs: both brackets are joined at the middle row, where the line in and the line out meet them
        let c = layout(&[job("a", &[]), job("b", &["a"]), job("c", &["a"]), job("e", &["a"]), job("d", &["b", "c", "e"])]);
        snap(&c, "
             ╭ ○ b S1 ╮
  ○ a S1 ────┤ ○ c S1 ├──── ○ d S1
             ╰ ○ e S1 ╯");
        // two Jobs: the middle row is the upper one
        let c = layout(&[job("a", &[]), job("b", &["a"]), job("c", &["a"])]);
        snap(&c, "
  ○ a S1 ────┬ ○ b S1 ╮
             ╰ ○ c S1 ╯");
    }

    #[test]
    fn single_cards_are_one_row_apart_and_have_plain_line_ends() {
        let c = layout(&[job("a", &[]), job("b", &[]), job("c", &["a"]), job("d", &["b"])]);
        snap(&c, "
  ○ a S1 ──┬── ○ c S1
           │
  ○ b S1 ──┴── ○ d S1");
    }

    #[test]
    fn the_tier_is_aligned_right_and_so_is_the_attempt() {
        let mut long = job("a-longer-key", &[]);
        long.attempt = 2;
        let c = layout(&[job("a", &[]), long, job("b", &[])]);
        snap(&c, "
  ○ a               S1

  ○ a-longer-key S1 a2

  ○ b               S1");
    }

    #[test]
    fn one_bus_per_gap_joins_every_exit_and_every_entry() {
        let c = layout(&[job("a", &[]), job("b", &[]), job("c", &["a"]), job("d", &["b"])]);
        let bus_x = c.pills[0].rect.right() + 2;
        // the bus is one column, with a junction for each line in and out
        let glyphs: Vec<&str> = (0..c.buf.area.height).map(|y| c.buf[(bus_x, y)].symbol()).collect();
        assert_eq!(glyphs, ["┬", "│", "┴"]);
        let c = layout(&[job("a", &[]), job("b", &[]), job("c", &["a", "b"])]);
        snap(&c, "
  ○ a S1 ──╮
           ├── ○ c S1
  ○ b S1 ──╯");
    }

    #[test]
    fn a_line_that_skips_columns_rides_a_free_row() {
        let c = layout(&[job("a", &[]), job("x", &[]), job("b", &["a"]), job("c", &["b", "x"])]);
        snap(&c, "
  ○ a S1 ──╮
           ├── ○ b S1 ──┬── ○ c S1
  ○ x S1 ──┴────────────╯");
    }

    #[test]
    fn a_column_is_centred_vertically() {
        let c = layout(&[job("a", &[]), job("b", &[]), job("c", &[]), job("d", &["a", "b", "c"])]);
        assert_eq!(c.pills[3].rect.y, 2);
        // an odd difference leaves the spare row below
        let c = layout(&[job("a", &[]), job("b", &[]), job("d", &["a", "b"]), job("e", &["a", "b"]), job("f", &["d"])]);
        assert_eq!(c.pills.iter().map(|p| p.rect.y).collect::<Vec<_>>(), [0, 2, 0, 1, 1]);
    }

    #[test]
    fn a_column_is_ordered_by_the_mean_row_of_the_cards_it_needs() {
        // d needs the lower root, c the upper one: the order of the Plan (d first) would cross them
        let c = layout(&[job("a", &[]), job("b", &[]), job("d", &["b"]), job("c", &["a"])]);
        assert_eq!(c.columns[1], vec![3, 2]);
    }

    #[test]
    fn a_cycle_is_laid_out_without_panicking() {
        let c = layout(&[job("a", &["b"]), job("b", &["a"]), job("c", &["c"])]);
        assert_eq!(c.pills.len(), 3);
        assert!(c.columns.iter().all(|col| !col.is_empty()));
    }

    #[test]
    fn an_empty_workflow_is_an_empty_chart() {
        let c = layout(&[]);
        assert_eq!((c.pills.len(), c.columns.len(), c.buf.area.area()), (0, 0, 0));
    }

    #[test]
    fn selection_moves_between_columns_and_within_one() {
        // a | b c d | e : b, c, d share a card
        let c = layout(&[job("a", &[]), job("b", &["a"]), job("c", &["a"]), job("d", &["a"]), job("e", &["b", "c", "d"])]);
        let (a, b, cc, d, e) = (0, 1, 2, 3, 4);
        assert_eq!(c.step(a, Dir::Right), cc); // the nearest to a's row
        assert_eq!(c.step(cc, Dir::Right), e);
        assert_eq!(c.step(cc, Dir::Left), a);
        assert_eq!(c.step(a, Dir::Left), a);
        assert_eq!(c.step(e, Dir::Right), e);
        assert_eq!((c.step(cc, Dir::Up), c.step(b, Dir::Up)), (b, b));
        assert_eq!((c.step(cc, Dir::Down), c.step(d, Dir::Down)), (d, d));
        assert_eq!(c.step(a, Dir::Down), a);
    }

    fn two() -> Chart {
        layout(&[job("a", &[]), job("b", &["a"])])
    }

    #[test]
    fn nothing_is_selected_on_open_and_a_refresh_creates_no_selection() {
        let mut s = Selection::default();
        assert_eq!((s.resolve(&two()), s.key.clone()), (None, None));
    }

    #[test]
    fn the_first_arrow_selects_the_first_pill_of_the_first_column_then_moves() {
        let c = two();
        for d in [Dir::Up, Dir::Down, Dir::Left, Dir::Right] {
            let mut s = Selection::default();
            s.arrow(&c, d);
            assert_eq!((s.key.as_deref(), s.follow), (Some("a"), true));
        }
        let mut s = Selection::default();
        s.arrow(&c, Dir::Right);
        s.arrow(&c, Dir::Right);
        assert_eq!(s.key.as_deref(), Some("b"));
        s.arrow(&layout(&[]), Dir::Right); // an empty chart selects nothing
        assert_eq!(s, Selection::default());
    }

    #[test]
    fn a_click_selects_that_pill_without_following() {
        let c = two();
        let mut s = Selection { key: None, follow: true };
        s.click("b".into());
        let at = s.resolve(&c);
        assert_eq!((s.key.as_deref(), s.follow, at), (Some("b"), false, Some(1)));
    }

    #[test]
    fn a_click_on_another_pill_selects_it() {
        let mut s = Selection::default();
        s.click("a".into());
        s.click("b".into());
        assert_eq!(s.key.as_deref(), Some("b"));
    }

    #[test]
    fn a_click_on_the_selected_pill_clears_it() {
        let mut s = Selection::default();
        s.click("a".into());
        s.click("a".into());
        assert_eq!(s, Selection::default());
        assert_eq!(s.enter(), None);
    }

    #[test]
    fn empty_space_and_esc_clear_and_name_the_job_to_close() {
        let mut s = Selection::default();
        s.click("a".into());
        assert_eq!(s.clear().as_deref(), Some("a"));
        assert_eq!((s.clear(), &s), (None, &Selection::default()));
    }

    #[test]
    fn an_arrow_after_a_clear_selects_the_first_pill() {
        let c = two();
        let mut s = Selection::default();
        s.arrow(&c, Dir::Right);
        s.arrow(&c, Dir::Right);
        s.clear();
        s.arrow(&c, Dir::Down);
        assert_eq!((s.key.as_deref(), s.follow), (Some("a"), true));
    }

    #[test]
    fn enter_without_a_selection_opens_nothing() {
        let mut s = Selection::default();
        assert_eq!(s.enter(), None);
        s.click("a".into());
        assert_eq!(s.enter().as_deref(), Some("a"));
    }

    #[test]
    fn a_refresh_keeps_the_selection_by_key() {
        let mut s = Selection::default();
        s.click("b".into());
        let moved = layout(&[job("x", &[]), job("a", &[]), job("b", &["a", "x"])]);
        assert_eq!(s.resolve(&moved), Some(2));
        assert_eq!(s.key.as_deref(), Some("b"));
    }

    #[test]
    fn a_switch_clears_the_selection_even_if_the_new_plan_has_the_same_key() {
        let mut s = Selection::default();
        s.arrow(&two(), Dir::Right);
        s.reset();
        assert_eq!((s.resolve(&two()), s.key), (None, None));
    }

    #[test]
    fn a_selected_job_that_disappears_is_unselected() {
        let mut s = Selection::default();
        s.click("b".into());
        assert_eq!(s.resolve(&layout(&[job("a", &[])])), None);
        assert_eq!(s, Selection::default());
        assert_eq!(s.resolve(&two()), None); // and it does not come back
    }

    #[test]
    fn the_window_scrolls_to_the_selection_on_both_axes_and_marks_it() {
        let jobs: Vec<FlowJob> = ["a", "b", "c", "d"].iter().map(|k| job(k, &[])).collect();
        let c = layout(&jobs);
        // the window shows 3 rows: pill d (row 6) pulls the view down
        assert_eq!(c.scroll_to(3, (0, 0), (20, 3)), (0, 4));
        assert_eq!(c.scroll_to(0, (0, 4), (20, 3)), (0, 0));
        let wide = layout(&[job("a", &[]), job("b", &["a"]), job("c", &["b"])]);
        assert_eq!(wide.scroll_to(2, (0, 0), (12, 1)), (wide.pills[2].rect.right() + 1 - 12, 0));
        let mut dst = Buffer::empty(Rect::new(0, 0, 30, 3));
        let (ox, oy) = c.blit(&mut dst, Rect::new(2, 1, 20, 1), (0, 100), Some(3)); // clamped to the chart
        assert_eq!((ox, oy), (0, 6));
        let cell = &dst[(3, 1)];
        assert!(cell.modifier.contains(Modifier::REVERSED | Modifier::BOLD));
        assert_eq!(dst[(3, 0)].symbol(), " "); // the padding is untouched
    }
}
