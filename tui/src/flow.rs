//! The Plan view's flowchart: a pure layout of a Workflow's Jobs as one-row pills joined by box-drawing edges.
//!
//! In: Jobs and a width. Out: a `Buffer` of the whole chart plus each pill's rectangle. Levels run top to bottom, two
//! rows per level (the pill row, then one row of edges). The view copies a window of the buffer into the frame (`blit`).
//! The chart is as wide as the pane unless a level cannot fit even with the shortest keys: then it is wider and the
//! view scrolls horizontally.

use std::collections::{HashMap, HashSet};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

/// What the layout needs to know of a Job.
#[derive(Clone, Debug)]
pub struct FlowJob {
    pub key: String,
    pub status: String,
    pub tier: String,
    pub needs: Vec<String>,
    /// How many Attempts the Job has had; the pill shows `aN` from the second on.
    pub attempt: usize,
}

pub struct Pill {
    pub key: String,
    pub rect: Rect,
    pub level: usize,
}

pub struct Chart {
    pub buf: Buffer,
    /// In the order of the (visible) Jobs.
    pub pills: Vec<Pill>,
    /// Pill indices of each level, left to right.
    pub levels: Vec<Vec<usize>>,
    /// Cancelled Jobs left out.
    pub hidden: usize,
}

#[derive(Clone, Copy)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

const GAP: u16 = 2; // between pills of a level
const MARGIN: u16 = 1; // either side of the pills: room for the selection markers
const MIN_KEY: usize = 6; // shortest a key is truncated to, `…` included

pub fn icon(status: &str) -> &'static str {
    match status {
        "closed" | "merged" => "✓",
        "running" => "●",
        "in_review" => "◉",
        "queued" => "○",
        "draft" => "◌",
        "failed" | "blocked" | "escalated" => "✗",
        _ => "?",
    }
}

pub fn status_color(status: &str) -> Color {
    match status {
        "closed" => Color::Green,
        "running" | "in_review" => Color::Yellow,
        "failed" => Color::Red,
        _ => Color::Gray,
    }
}

fn truncate(s: &str, w: usize) -> String {
    if s.chars().count() <= w {
        s.to_string()
    } else {
        s.chars().take(w.saturating_sub(1)).collect::<String>() + "…"
    }
}

/// ` <icon> <key> <tier>[ aN] `, the key cut to `key_w` characters.
fn pill_text(j: &FlowJob, key_w: usize) -> String {
    let a = if j.attempt > 1 { format!(" a{}", j.attempt) } else { String::new() };
    format!(" {} {} {}{} ", icon(&j.status), truncate(&j.key, key_w), j.tier, a)
}

fn text_w(s: &str) -> u16 {
    s.chars().count() as u16
}

/// Levels top to bottom: a Job sits one level below the deepest Job it needs (none: level 0). Each level is ordered by
/// the mean relative position of its parents (barycentre), ties by the Workflow's order.
fn levels(jobs: &[FlowJob]) -> Vec<Vec<usize>> {
    let idx: HashMap<&str, usize> = jobs.iter().enumerate().map(|(i, j)| (j.key.as_str(), i)).collect();
    fn depth(i: usize, jobs: &[FlowJob], idx: &HashMap<&str, usize>, lvl: &mut [Option<usize>], open: &mut HashSet<usize>) -> usize {
        if let Some(d) = lvl[i] {
            return d;
        }
        if !open.insert(i) {
            return 0; // a cycle: cut it here
        }
        let d = jobs[i].needs.iter().filter_map(|n| idx.get(n.as_str())).map(|&p| depth(p, jobs, idx, lvl, open) + 1).max().unwrap_or(0);
        open.remove(&i);
        lvl[i] = Some(d);
        d
    }
    let mut lvl = vec![None; jobs.len()];
    let lvl: Vec<usize> = (0..jobs.len()).map(|i| depth(i, jobs, &idx, &mut lvl, &mut HashSet::new())).collect();
    let mut rows = vec![Vec::new(); lvl.iter().max().map_or(0, |m| m + 1)];
    for (i, &l) in lvl.iter().enumerate() {
        rows[l].push(i);
    }
    let mut pos: HashMap<usize, f64> = HashMap::new();
    for row in rows.iter_mut() {
        let mut keyed: Vec<(f64, usize, usize)> = row
            .iter()
            .enumerate()
            .map(|(k, &i)| {
                let ps: Vec<f64> = jobs[i].needs.iter().filter_map(|n| idx.get(n.as_str())).filter_map(|p| pos.get(p)).copied().collect();
                let b = if ps.is_empty() { 0.0 } else { ps.iter().sum::<f64>() / ps.len() as f64 };
                (b, i, k)
            })
            .collect();
        keyed.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.cmp(&b.2)));
        *row = keyed.iter().map(|k| k.1).collect();
        let len = row.len() as f64;
        for (p, &i) in row.iter().enumerate() {
            pos.insert(i, (p as f64 + 0.5) / len);
        }
    }
    rows
}

// ---------- edge masks ----------
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
        if let Some(c) = self.cells.get_mut(y as usize * w + x as usize) {
            *c |= d;
        }
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

/// Everything `layout` decides before drawing.
struct Plan {
    pills: Vec<Pill>,
    texts: Vec<String>,
    levels: Vec<Vec<usize>>,
    mask: Mask,
}

fn plan(jobs: &[FlowJob], width: u16) -> Plan {
    let idx: HashMap<&str, usize> = jobs.iter().enumerate().map(|(i, j)| (j.key.as_str(), i)).collect();
    let rows = levels(jobs);
    let mut lvl = vec![0; jobs.len()];
    for (l, r) in rows.iter().enumerate() {
        for &i in r {
            lvl[i] = l;
        }
    }
    let parents = |c: usize| jobs[c].needs.iter().filter_map(|n| idx.get(n.as_str()).copied()).collect::<HashSet<_>>();
    // a parent with an edge that skips levels gets a lane right of every pill, one column each
    let mut lane_of: HashMap<usize, u16> = HashMap::new();
    for p in 0..jobs.len() {
        if (0..jobs.len()).any(|c| parents(c).contains(&p) && lvl[c] > lvl[p] + 1) {
            lane_of.insert(p, lane_of.len() as u16);
        }
    }
    let lanes = lane_of.len() as u16;
    // each level's pills, keys cut as little as fits
    let avail = width.saturating_sub(lanes + 2 * MARGIN);
    let longest = jobs.iter().map(|j| j.key.chars().count()).max().unwrap_or(0);
    let level_texts: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            let total = |k: usize| r.iter().map(|&i| text_w(&pill_text(&jobs[i], k))).sum::<u16>() + GAP * (r.len() as u16 - 1);
            let k = (MIN_KEY.min(longest)..=longest).rev().find(|&k| total(k) <= avail).unwrap_or(MIN_KEY.min(longest));
            r.iter().map(|&i| pill_text(&jobs[i], k)).collect()
        })
        .collect();
    let level_w = |l: usize| level_texts[l].iter().map(|t| text_w(t)).sum::<u16>() + GAP * (level_texts[l].len() as u16 - 1);
    let widest = (0..rows.len()).map(level_w).max().unwrap_or(0);
    let base_w = width.saturating_sub(lanes).max(widest + 2 * MARGIN);
    let total_w = base_w + lanes;
    let h = (rows.len() as u16 * 2).saturating_sub(1).max(1);
    let mut pills: Vec<Option<Pill>> = (0..jobs.len()).map(|_| None).collect();
    let mut texts = vec![String::new(); jobs.len()];
    for (l, r) in rows.iter().enumerate() {
        let mut x = MARGIN + (base_w - 2 * MARGIN - level_w(l)) / 2;
        for (k, &i) in r.iter().enumerate() {
            let w = text_w(&level_texts[l][k]);
            pills[i] = Some(Pill { key: jobs[i].key.clone(), rect: Rect::new(x, l as u16 * 2, w, 1), level: l });
            texts[i] = level_texts[l][k].clone();
            x += w + GAP;
        }
    }
    let pills: Vec<Pill> = pills.into_iter().flatten().collect();
    let mut mask = Mask::new(total_w, h);
    let centre = |i: usize| pills[i].rect.x + pills[i].rect.width / 2;
    for c in 0..jobs.len() {
        for p in parents(c) {
            let (s, e) = (pills[p].level as u16 * 2 + 1, pills[c].level as u16 * 2 - 1); // band row under the parent, above the child
            let (px, cx) = (centre(p), centre(c));
            mask.add(px, s, U);
            if lvl[c] == lvl[p] + 1 {
                mask.h(s, px, cx);
                mask.v(cx, s, e);
            } else {
                let lane = base_w + lane_of[&p];
                mask.h(s, px, lane);
                mask.v(lane, s, e);
                mask.h(e, lane, cx);
            }
            mask.add(cx, e, D);
        }
    }
    // the levels of the pill vector (same order as `jobs`)
    let levels = rows;
    Plan { pills, texts, levels, mask }
}

pub fn layout(all: &[FlowJob], width: u16) -> Chart {
    let jobs: Vec<FlowJob> = all.iter().filter(|j| j.status != "cancelled").cloned().collect();
    let keys: HashSet<&str> = jobs.iter().map(|j| j.key.as_str()).collect();
    let jobs: Vec<FlowJob> = jobs
        .iter()
        .map(|j| FlowJob { needs: j.needs.iter().filter(|n| keys.contains(n.as_str())).cloned().collect(), ..j.clone() })
        .collect();
    let hidden = all.len() - jobs.len();
    let Plan { pills, texts, levels, mask } = plan(&jobs, width);
    let mut buf = Buffer::empty(Rect::new(0, 0, mask.w.max(width), (mask.cells.len() / mask.w.max(1) as usize).max(1) as u16));
    let dim = Style::new().add_modifier(Modifier::DIM);
    for y in 0..buf.area.height.min((mask.cells.len() / mask.w.max(1) as usize) as u16) {
        for x in 0..mask.w {
            let m = mask.at(x, y);
            if m != 0 {
                buf[(x, y)].set_symbol(glyph(m)).set_style(dim);
            }
        }
    }
    for (p, text) in pills.iter().zip(&texts) {
        let col = jobs.iter().find(|j| j.key == p.key).map_or(Color::Gray, |j| status_color(&j.status));
        buf.set_string(p.rect.x, p.rect.y, text, Style::new().fg(Color::Black).bg(col));
    }
    Chart { buf, pills, levels, hidden }
}

impl Chart {
    pub fn pill_at(&self, x: u16, y: u16) -> Option<usize> {
        self.pills.iter().position(|p| x >= p.rect.x && x < p.rect.right() && y == p.rect.y)
    }

    /// The spatial neighbour of pill `from`: up/down the nearest (by centre) on the level above/below, left/right the
    /// next on the same level. Stays put at an edge.
    pub fn step(&self, from: usize, dir: Dir) -> usize {
        let level = self.pills[from].level;
        let row = &self.levels[level];
        let at = row.iter().position(|&i| i == from).unwrap_or(0);
        let centre = |i: usize| self.pills[i].rect.x as i32 * 2 + self.pills[i].rect.width as i32;
        let nearest = |l: usize| self.levels[l].iter().copied().min_by_key(|&i| (centre(i) - centre(from)).abs());
        match dir {
            Dir::Left => row[at.saturating_sub(1)],
            Dir::Right => row[(at + 1).min(row.len() - 1)],
            Dir::Up if level > 0 => nearest(level - 1).unwrap_or(from),
            Dir::Down if level + 1 < self.levels.len() => nearest(level + 1).unwrap_or(from),
            _ => from,
        }
    }

    /// The smallest scroll offset change that shows pill `i` and its markers in a `w`×`h` window.
    pub fn scroll_to(&self, i: usize, (ox, oy): (u16, u16), (w, h): (u16, u16)) -> (u16, u16) {
        let r = self.pills[i].rect;
        let fit = |o: u16, lo: u16, hi: u16, size: u16| if lo < o { lo } else if hi > o + size { hi.saturating_sub(size) } else { o };
        (fit(ox, r.x.saturating_sub(1), r.right() + 1, w), fit(oy, r.y, r.bottom(), h))
    }

    /// Copy the window at (`ox`, `oy`) into `dst` at `area`, the selected pill bold between `▸ ◂`. Offsets are clamped
    /// to the chart; the clamped offsets are returned.
    pub fn blit(&self, dst: &mut Buffer, area: Rect, (ox, oy): (u16, u16), selected: Option<usize>) -> (u16, u16) {
        let ox = ox.min(self.buf.area.width.saturating_sub(area.width));
        let oy = oy.min(self.buf.area.height.saturating_sub(area.height));
        for y in 0..area.height.min(self.buf.area.height - oy) {
            for x in 0..area.width.min(self.buf.area.width - ox) {
                dst[(area.x + x, area.y + y)] = self.buf[(x + ox, y + oy)].clone();
            }
        }
        if let Some(r) = selected.map(|i| self.pills[i].rect) {
            let mut put = |x: u16, symbol: Option<&str>| {
                if x >= ox && x < ox + area.width && r.y >= oy && r.y < oy + area.height {
                    let c = &mut dst[(area.x + x - ox, area.y + r.y - oy)];
                    match symbol {
                        Some(s) => {
                            c.set_symbol(s).set_style(Style::new().fg(Color::White).add_modifier(Modifier::BOLD));
                        }
                        None => {
                            c.modifier.insert(Modifier::BOLD);
                        }
                    }
                }
            };
            put(r.x - 1, Some("▸"));
            (r.x..r.right()).for_each(|x| put(x, None));
            put(r.right(), Some("◂"));
        }
        (ox, oy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(key: &str, needs: &[&str]) -> FlowJob {
        FlowJob { key: key.into(), status: "queued".into(), tier: "S1".into(), needs: needs.iter().map(|s| s.to_string()).collect(), attempt: 1 }
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
    fn chain_of_three() {
        let c = layout(&[job("a", &[]), job("b", &["a"]), job("c", &["b"])], 20);
        snap(&c, "
       ○ a S1
          │
       ○ b S1
          │
       ○ c S1");
    }

    #[test]
    fn fan_out() {
        let c = layout(&[job("a", &[]), job("b", &["a"]), job("c", &["a"]), job("d", &["a"])], 30);
        snap(&c, "
            ○ a S1
     ╭─────────┼─────────╮
  ○ b S1    ○ c S1    ○ d S1");
    }

    #[test]
    fn fan_in() {
        let c = layout(&[job("a", &[]), job("b", &[]), job("c", &[]), job("d", &["a", "b", "c"])], 30);
        snap(&c, "
  ○ a S1    ○ b S1    ○ c S1
     ╰─────────┼─────────╯
            ○ d S1");
    }

    #[test]
    fn skip_level_edge_runs_right_of_every_pill() {
        let p = plan(&[job("a", &[]), job("b", &["a"]), job("c", &["b", "a"])], 20);
        for pill in &p.pills {
            for x in pill.rect.x..pill.rect.right() {
                assert_eq!(p.mask.at(x, pill.rect.y), 0, "an edge crosses {}", pill.key);
            }
        }
        let c = layout(&[job("a", &[]), job("b", &["a"]), job("c", &["b", "a"])], 20);
        snap(&c, "
      ○ a S1
         ├─────────╮
      ○ b S1       │
         ├─────────╯
      ○ c S1");
    }

    #[test]
    fn wide_level_truncates_keys_then_scrolls() {
        let jobs: Vec<FlowJob> = ["api-server", "web-client", "docs-generator"].iter().map(|k| job(k, &[])).collect();
        assert_eq!(text(&layout(&jobs, 80)), "           ○ api-server S1    ○ web-client S1    ○ docs-generator S1");
        let c = layout(&jobs, 56);
        assert_eq!(text(&c), "   ○ api-serv… S1    ○ web-clie… S1    ○ docs-gen… S1");
        assert_eq!(c.buf.area.width, 56);
        // at the shortest keys the level is 45 wide: a 40-wide pane scrolls
        let c = layout(&jobs, 40);
        assert_eq!(text(&c), "  ○ api-s… S1    ○ web-c… S1    ○ docs-… S1");
        assert_eq!(c.buf.area.width, 45);
        let (ox, _) = c.scroll_to(2, (0, 0), (40, 1));
        assert_eq!(ox, 5);
        let mut dst = Buffer::empty(Rect::new(0, 0, 40, 1));
        assert_eq!(c.blit(&mut dst, Rect::new(0, 0, 40, 1), (ox, 0), None), (5, 0)); // clamped to the chart
    }

    #[test]
    fn cancelled_jobs_are_hidden() {
        let mut gone = job("gone", &[]);
        gone.status = "cancelled".into();
        let c = layout(&[job("a", &[]), gone, job("b", &["gone", "a"])], 20);
        assert_eq!(c.hidden, 1);
        assert_eq!(c.pills.len(), 2);
        snap(&c, "
       ○ a S1
          │
       ○ b S1");
    }

    #[test]
    fn selection_moves_spatially() {
        let c = layout(&[job("a", &[]), job("b", &["a"]), job("c", &["a"])], 40);
        let (a, b, cc) = (0, 1, 2);
        assert_eq!(c.step(a, Dir::Down), b);
        assert_eq!(c.step(b, Dir::Right), cc);
        assert_eq!(c.step(cc, Dir::Right), cc);
        assert_eq!(c.step(cc, Dir::Up), a);
        assert_eq!(c.step(a, Dir::Up), a);
    }
}
