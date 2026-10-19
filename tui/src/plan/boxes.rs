//! The plan view's PR boxes (commands.md §13.2, R34 A15, hs-d115): one bordered box per PR that is not `cancelled`,
//! holding the chart of that PR's Jobs ([`flow::layout`]) with its title on the top border; the boxes top to bottom by
//! the PRs' needs, one row of boxes per level, each need an arrow from the needed box's bottom border to the needing
//! box's top border.
//!
//! In: the PRs and the Jobs. Out: [`Boxes`], one [`Chart`] of the whole canvas of boxes, so the scrolling and the clicks
//! work on it as on one chart (the pills' rectangles are on the whole buffer), and the row of boxes of each of its
//! columns, so the selection moves ←→ within a row of boxes and ↑↓ across rows ([`Boxes::step`]). The arrows are
//! routed as the chart's lines are, turned a quarter: one horizontal run per gap between rows of boxes, a need that
//! skips rows riding a column free of boxes.

use std::collections::HashSet;

use ratatui::{
    buffer::Buffer,
    layout::{Margin, Rect},
    style::Stylize,
    text::{Line, Span},
    widgets::{Block, Widget},
};

use super::flow::{self, Chart, Dir, Mask, Pill, free_row, glyph};
use crate::link::link;
use crate::palette::{self, Meaning};
use crate::read::{JobNode, PrNode, PrState, Pull};

/// Rows between two rows of boxes: the run of lines, then the arrowheads.
const GAP: u16 = 2;
/// What the box of a PR with no Jobs (none that is not cancelled) says.
const NO_JOBS: &str = " no Jobs ";
/// The head of an arrow, just above the needing box's top border.
const ARROW: &str = "▼";
/// The needed box's bottom border where an arrow leaves it.
const EXIT: &str = "┬";

/// The canvas of boxes: the chart of all of them, and the row of boxes (the level, [`levels`]) of each of its columns.
pub struct Boxes {
    pub chart: Chart,
    /// Indexed by the chart's columns.
    pub row_of: Vec<usize>,
}

/// The level of each PR (indices into `prs`): 0 with no needs, else one more than the highest level among its needs.
/// A need that is not one of `prs`, or that closes a cycle, is ignored.
pub fn levels(prs: &[PrNode]) -> Vec<usize> {
    fn level(i: usize, needs: &[Vec<usize>], memo: &mut [Option<usize>], open: &mut HashSet<usize>) -> usize {
        if let Some(l) = memo[i] {
            return l;
        }
        open.insert(i);
        let mut l = 0;
        for &p in &needs[i] {
            if !open.contains(&p) {
                l = l.max(level(p, needs, memo, open) + 1);
            }
        }
        open.remove(&i);
        memo[i] = Some(l);
        l
    }
    let needs: Vec<Vec<usize>> =
        prs.iter().map(|p| p.needs.iter().filter_map(|n| prs.iter().position(|q| &q.key == n)).collect()).collect();
    let mut memo = vec![None; prs.len()];
    (0..prs.len()).map(|i| level(i, &needs, &mut memo, &mut HashSet::new())).collect()
}

/// A PR state's word.
fn state_text(state: PrState) -> &'static str {
    match state {
        PrState::Open => "open",
        PrState::Merged => "merged",
        PrState::Closed => "closed",
    }
}

/// What follows `#<number> <state>` in a title once the PR has a pull request: the `↗` its link covers.
const PULL_LINK: &str = " ↗";

/// The word and colour of a pull request's state in a title: `conflict` (failed) for an open one that conflicts, else
/// its state in the palette's PR state colour. The title and its link's width both use it.
fn state_word(pull: &Pull) -> (&'static str, Meaning) {
    match pull.state {
        PrState::Open if pull.conflicting => palette::PR_CONFLICT,
        state => (state_text(state), palette::pr_state(state)),
    }
}

/// The box's title: ` <key> · <repo name> → <base> · <pull request> `, the key bold; `#<number> <state> ↗` once the PR has
/// a pull request, the state in the palette's PR state colour (`conflict`, failed, for an open one that conflicts), else the PR's status, dim.
pub fn title(pr: &PrNode) -> Line<'static> {
    let mut spans: Vec<Span<'static>> =
        vec![" ".into(), pr.key.to_string().bold(), format!(" · {} → {} · ", pr.repo_name, pr.base).into()];
    match &pr.pull {
        Some(pull) => {
            spans.push(format!("#{} ", pull.number).into());
            let (word, meaning) = state_word(pull);
            spans.push(Span::styled(word, palette::fg(meaning)));
            spans.push(PULL_LINK.into());
        }
        None => spans.push(pr.status.as_str().dim()),
    }
    spans.push(" ".into());
    Line::from(spans)
}

/// The whole canvas of boxes: one per PR of `prs` (the PRs that are not `cancelled`), each holding the chart of its
/// `jobs`. The pills are in the order of `prs`, then of each PR's Jobs. A running Job's icon is the spinner's `frame`.
pub fn layout(prs: &[PrNode], jobs: &[JobNode], frame: usize) -> Boxes {
    let charts: Vec<Chart> =
        prs.iter().map(|p| flow::layout(&flow::flow_jobs(jobs.iter().filter(|j| j.pr == p.key)), frame)).collect();
    let titles: Vec<Line> = prs.iter().map(title).collect();
    // each box: its border around the chart, or around ` no Jobs `, and at least as wide as its title
    let size: Vec<(u16, u16)> = charts
        .iter()
        .zip(&titles)
        .map(|(c, t)| {
            let empty = c.pills.is_empty();
            let inner_w = c.buf.area.width.max(t.width() as u16).max(if empty { NO_JOBS.chars().count() as u16 } else { 0 });
            let inner_h = if empty { 1 } else { c.buf.area.height };
            (inner_w + 2, inner_h + 2)
        })
        .collect();

    // y: each row of boxes as tall as its tallest box, `GAP` between; x: a row's boxes side by side by key, one apart
    let level = levels(prs);
    let nrows = level.iter().max().map_or(0, |m| m + 1);
    let mut rows: Vec<Vec<usize>> = vec![Vec::new(); nrows];
    for (i, &l) in level.iter().enumerate() {
        rows[l].push(i);
    }
    for r in &mut rows {
        r.sort_by(|a, b| prs[*a].key.cmp(&prs[*b].key));
    }
    let row_h: Vec<u16> = rows.iter().map(|r| r.iter().map(|&i| size[i].1).max().unwrap_or(0)).collect();
    let ys: Vec<u16> = row_h.iter().scan(0, |y, h| Some(std::mem::replace(y, *y + h + GAP))).collect();
    let mut at = vec![(0u16, 0u16); prs.len()];
    for (l, r) in rows.iter().enumerate() {
        let mut x = 0;
        for &i in r {
            at[i] = (x, ys[l]);
            x += size[i].0 + 1;
        }
    }
    // a box's arrows leave and enter at the middle column of its border
    let mid = |i: usize| at[i].0 + (size[i].0 - 1) / 2;
    let bottom = |i: usize| at[i].1 + size[i].1 - 1; // its bottom border's row
    let run = |l: usize| ys[l] + row_h[l]; // the row of the run below row of boxes `l`

    // the arrows: needs that do not point down (a cycle) are not drawn
    let edges: Vec<(usize, usize)> = (0..prs.len())
        .flat_map(|c| prs[c].needs.iter().filter_map(|n| prs.iter().position(|q| &q.key == n)).map(move |p| (p, c)))
        .filter(|&(p, c)| level[p] < level[c])
        .collect();
    // a line that skips rows keeps a column clear of their boxes and of the columns beside them
    let busy: Vec<HashSet<u16>> =
        rows.iter().map(|r| r.iter().flat_map(|&i| at[i].0.saturating_sub(1)..=at[i].0 + size[i].0).collect()).collect();
    let mut hs: Vec<(u16, u16, u16)> = Vec::new(); // (y, x1, x2)
    let mut vs: Vec<(u16, u16, u16)> = Vec::new(); // (x, y1, y2)
    for &(p, c) in &edges {
        let (lp, lc) = (level[p], level[c]);
        vs.push((mid(p), bottom(p), run(lp)));
        let mut x = mid(p);
        if lc > lp + 1 {
            let free = free_row(mid(p), lp + 1..lc, &busy);
            hs.push((run(lp), x, free));
            vs.push((free, run(lp), run(lc - 1)));
            x = free;
        }
        hs.push((run(lc - 1), x, mid(c)));
        vs.push((mid(c), run(lc - 1), ys[lc] - 1));
    }
    let boxes_w = (0..prs.len()).map(|i| at[i].0 + size[i].0).max().unwrap_or(0);
    let width = vs.iter().map(|v| v.0 + 1).fold(boxes_w, u16::max);
    let height = if nrows == 0 { 0 } else { ys[nrows - 1] + row_h[nrows - 1] };
    let mut mask = Mask::new(width, height);
    for &(y, x1, x2) in &hs {
        mask.h(y, x1, x2);
    }
    for &(x, y1, y2) in &vs {
        mask.v(x, y1, y2);
    }

    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    let lines = palette::fg(Meaning::Nothing);
    for (i, chart) in charts.iter().enumerate() {
        let rect = Rect::new(at[i].0, at[i].1, size[i].0, size[i].1);
        Block::bordered().border_style(lines).title(titles[i].clone()).render(rect, &mut buf);
        if let Some(pull) = &prs[i].pull {
            // the title starts after the corner and ends with one space; the `↗` is the cell before it, and the link
            // runs back from there over `#<number> <state> ↗`
            let len = format!("#{} {}{PULL_LINK}", pull.number, state_word(pull).0).chars().count() as u16;
            link(&mut buf, Rect::new(rect.x + titles[i].width() as u16 - len, rect.y, len, 1), &pull.url);
        }
        let inner = rect.inner(Margin { horizontal: 1, vertical: 1 });
        if chart.pills.is_empty() {
            Line::from(NO_JOBS.dim()).render(inner, &mut buf);
        }
        for y in 0..chart.buf.area.height {
            for x in 0..chart.buf.area.width {
                buf[(inner.x + x, inner.y + y)] = chart.buf[(x, y)].clone();
            }
        }
    }
    // the lines lie between the boxes, never on them (a line's end on a border is replaced by the exit below)
    for y in 0..height {
        for x in 0..width {
            if mask.at(x, y) != 0 {
                buf[(x, y)].set_symbol(glyph(mask.at(x, y))).set_style(lines);
            }
        }
    }
    for &(p, c) in &edges {
        buf[(mid(p), bottom(p))].set_symbol(EXIT).set_style(lines);
        buf[(mid(c), at[c].1 - 1)].set_symbol(ARROW).set_style(lines);
    }

    // the pills on the whole buffer; the columns row of boxes by row, box by box, then by each box's own chart columns
    let mut base = Vec::with_capacity(prs.len());
    let mut pills: Vec<Pill> = Vec::new();
    for (i, chart) in charts.iter().enumerate() {
        base.push(pills.len());
        pills.extend(chart.pills.iter().map(|p| Pill {
            key: p.key.clone(),
            rect: Rect { x: p.rect.x + at[i].0 + 1, y: p.rect.y + at[i].1 + 1, ..p.rect },
            column: 0, // set below
        }));
    }
    let mut columns: Vec<Vec<usize>> = Vec::new();
    let mut row_of: Vec<usize> = Vec::new();
    for (l, r) in rows.iter().enumerate() {
        for &i in r {
            for column in &charts[i].columns {
                let column: Vec<usize> = column.iter().map(|&p| base[i] + p).collect();
                for &p in &column {
                    pills[p].column = columns.len();
                }
                columns.push(column);
                row_of.push(l);
            }
        }
    }
    Boxes { chart: Chart { buf, pills, columns }, row_of }
}

impl Boxes {
    /// The same canvas with `margin` around it ([`Chart::with_margin`]).
    pub fn with_margin(self, margin: Margin) -> Boxes {
        Boxes { chart: self.chart.with_margin(margin), ..self }
    }

    /// The neighbour of pill `from`: as [`Chart::step`] within a row of boxes (←→ cross into the neighbouring box), and
    /// ↑↓ at the top or bottom of a box's column cross to the nearest pill (by column, then by row) of the nearest row
    /// of boxes above or below that has one. Stays put at an edge.
    pub fn step(&self, from: usize, dir: Dir) -> usize {
        let pills = &self.chart.pills;
        let row = |i: usize| self.row_of[pills[i].column];
        let to = self.chart.step(from, dir);
        if to != from && row(to) == row(from) {
            return to;
        }
        let rows = (0..pills.len()).map(row);
        let target = match dir {
            Dir::Up => rows.filter(|&r| r < row(from)).max(),
            Dir::Down => rows.filter(|&r| r > row(from)).min(),
            Dir::Left | Dir::Right => None,
        };
        let Some(target) = target else { return from };
        // twice the centre column of a pill, so it is whole
        let centre = |i: usize| 2 * pills[i].rect.x + pills[i].rect.width;
        (0..pills.len())
            .filter(|&i| row(i) == target)
            .min_by_key(|&i| (centre(i).abs_diff(centre(from)), pills[i].rect.y.abs_diff(pills[from].rect.y)))
            .unwrap_or(from)
    }
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::CellWidth;
    use ratatui::style::Modifier;

    use super::*;
    use crate::plan::flow::{Dir, Selection};
    use crate::read::{JobStatus, PrStatus};
    use cpd_core::config::Tier;
    use cpd_core::ids::{JobKey, PrKey};

    fn pr(key: &str, needs: &[&str]) -> PrNode {
        PrNode {
            key: PrKey::parse(key).unwrap(),
            repo_name: format!("{key}-repo"),
            base: "main".into(),
            status: PrStatus::Dispatch,
            needs: needs.iter().map(|n| PrKey::parse(n).unwrap()).collect(),
            pull: None,
        }
    }

    fn pull(number: i64, state: PrState) -> Pull {
        Pull { number, state, url: format!("https://github.com/acme/repo/pull/{number}"), conflicting: false }
    }

    fn job(pr: &str, key: &str, needs: &[&str]) -> JobNode {
        JobNode {
            key: JobKey::parse(key).unwrap(),
            pr: PrKey::parse(pr).unwrap(),
            seq: 0,
            title: String::new(),
            status: JobStatus::Queued,
            tier: Tier::parse("S1").unwrap(),
            needs: needs.iter().map(|n| JobKey::parse(n).unwrap()).collect(),
            appended: false,
            retry_tier: None,
            failed_cause: None,
            cancel_reason: None,
            merged_sha: None,
            attempts: Vec::new(),
        }
    }

    fn text(c: &Chart) -> String {
        (0..c.buf.area.height)
            .map(|y| (0..c.buf.area.width).map(|x| c.buf[(x, y)].symbol()).collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn snap(c: &Chart, want: &str) {
        assert_eq!(text(c), want.strip_prefix('\n').unwrap(), "\n{}", text(c));
    }

    #[test]
    fn a_level_is_one_more_than_the_highest_level_of_its_needs() {
        // web needs api and schema; api needs schema; docs needs nothing; e2e needs web and docs
        let prs = [pr("api", &["schema"]), pr("docs", &[]), pr("e2e", &["docs", "web"]), pr("schema", &[]), pr("web", &["api", "schema"])];
        assert_eq!(levels(&prs), [1, 0, 3, 0, 2]);
        assert_eq!(levels(&[]), Vec::<usize>::new());
        // a cycle is laid out without looping
        assert_eq!(levels(&[pr("a", &["b"]), pr("b", &["a"])]).len(), 2);
    }

    #[test]
    fn boxes_of_one_level_sit_side_by_side_by_key() {
        // given out of key order: the row still puts them by key, one column apart
        let prs = [pr("zed", &[]), pr("alpha", &[])];
        let b = layout(&prs, &[job("zed", "z", &[]), job("alpha", "a", &[])], 0);
        let c = &b.chart;
        let rect = |key: &str| c.pills.iter().find(|p| p.key == key).unwrap().rect;
        assert!(rect("a").x < rect("z").x && rect("a").y == rect("z").y, "alpha left of zed");
        assert_eq!(c.columns, [vec![1], vec![0]], "alpha's column first");
        assert_eq!(b.row_of, [0, 0]);
        snap(c, "
┌ alpha · alpha-repo → main · dispatch ┐ ┌ zed · zed-repo → main · dispatch ┐
│  󰝦 a │S1                             │ │  󰝦 z │S1                         │
└──────────────────────────────────────┘ └──────────────────────────────────┘");
    }

    #[test]
    fn one_pr_is_one_box_and_its_chart_is_the_single_chart_inside() {
        let jobs = [job("api", "a", &[]), job("api", "b", &[]), job("api", "c", &["a", "b"])];
        let c = layout(&[pr("api", &[])], &jobs, 0).chart;
        let single = flow::layout(&flow::flow_jobs(&jobs), 0);
        let rows: Vec<String> = text(&c).lines().map(str::to_owned).collect();
        assert_eq!(rows.len(), single.buf.area.height as usize + 2);
        for (y, want) in text(&single).lines().enumerate() {
            let inside: String = rows[y + 1].chars().skip(1).take(want.chars().count()).collect();
            assert_eq!(inside, want, "row {y}");
        }
        snap(&c, "
┌ api · api-repo → main · dispatch ┐
│  󰝦 a │S1 ──╮                     │
│            ├── 󰝦 c │S1           │
│  󰝦 b │S1 ──╯                     │
└──────────────────────────────────┘");
    }

    #[test]
    fn a_pr_with_no_jobs_is_an_empty_box() {
        let c = layout(&[pr("api", &[])], &[], 0).chart;
        snap(&c, "
┌ api · api-repo → main · dispatch ┐
│ no Jobs                          │
└──────────────────────────────────┘");
        assert!(c.buf[(1, 1)].modifier.contains(Modifier::DIM));
        assert!((c.pills.len(), c.columns.len()) == (0, 0));
        assert_eq!(layout(&[], &[], 0).chart.buf.area.area(), 0);
    }

    #[test]
    fn an_open_pull_request_that_conflicts_reads_conflict_in_the_failed_colour() {
        let failed = palette::fg(Meaning::Failed).fg;
        let mut p = pr("api", &[]);
        p.pull = Some(Pull { conflicting: true, ..pull(12, PrState::Open) });
        let t = title(&p);
        assert_eq!(t.to_string(), " api · api-repo → main · #12 conflict ↗ ");
        assert_eq!(t.spans[4].style.fg, failed);
        // `unknown` and `mergeable` are `conflicting: false`: `open`, not in the failed colour
        p.pull = Some(pull(12, PrState::Open));
        let t = title(&p);
        assert_eq!(t.to_string(), " api · api-repo → main · #12 open ↗ ");
        assert_ne!(t.spans[4].style.fg, failed);
        // merged and closed never read `conflict`
        for (state, word) in [(PrState::Merged, "merged"), (PrState::Closed, "closed")] {
            p.pull = Some(Pull { conflicting: true, ..pull(12, state) });
            let t = title(&p);
            assert_eq!(t.spans[4].content, word);
            assert_eq!(t.spans[4].style.fg, palette::fg(palette::pr_state(state)).fg);
        }
    }

    #[test]
    fn the_title_names_the_pull_request_in_its_colour_else_the_status_dim() {
        let mut open = pr("api", &[]);
        open.pull = Some(pull(12, PrState::Open));
        let t = title(&open);
        assert_eq!(t.to_string(), " api · api-repo → main · #12 open ↗ ");
        assert!(t.spans[1].style.add_modifier.contains(Modifier::BOLD), "the key is bold");
        assert_eq!(t.spans[4].style.fg, palette::fg(palette::pr_state(PrState::Open)).fg);
        for (state, word) in [(PrState::Merged, "merged"), (PrState::Closed, "closed")] {
            open.pull = Some(pull(12, state));
            let t = title(&open);
            assert_eq!(t.spans[4].content, word);
            assert_eq!(t.spans[4].style.fg, palette::fg(palette::pr_state(state)).fg);
        }
        let mut waiting = pr("web", &["api"]);
        waiting.status = PrStatus::Waiting;
        let t = title(&waiting);
        assert_eq!(t.to_string(), " web · web-repo → main · waiting ");
        assert!(t.spans[3].style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn a_pull_request_adds_its_arrow_to_the_title_and_widens_the_box_by_it() {
        let mut api = pr("api", &[]);
        let before = layout(&[api.clone()], &[], 0).chart;
        api.pull = Some(pull(12, PrState::Open));
        let c = layout(&[api], &[], 0).chart;
        let mut plain = Chart { buf: c.buf.clone(), pills: Vec::new(), columns: Vec::new() };
        let mut covered = 0;
        for cell in &mut plain.buf.content {
            if covered > 0 {
                // the second cell of a link piece is drawn by the piece
                cell.set_symbol("");
                covered -= 1;
                continue;
            }
            covered = cell.cell_width().saturating_sub(1);
            let symbol = cell.symbol().replace("\x1b]8;;https://github.com/acme/repo/pull/12\x07", "").replace("\x1b]8;;\x07", "");
            cell.set_symbol(&symbol);
        }
        snap(&plain, "
┌ api · api-repo → main · #12 open ↗ ┐
│ no Jobs                            │
└────────────────────────────────────┘");
        assert_eq!(c.buf.area.width, before.buf.area.width + 2, "`#12 open ↗` is two cells longer than `dispatch`");
        // every cell of `#12 open ↗`, and no other, is under the OSC 8 link to the pull request; a piece's second cell
        // keeps its symbol but is covered by the piece
        let top: Vec<char> = text(&plain).lines().next().unwrap().chars().collect();
        let first = top.iter().position(|&ch| ch == '#').unwrap() as u16;
        let arrow = top.iter().position(|&ch| ch == '↗').unwrap() as u16;
        let mut covered = Vec::new();
        for x in 0..c.buf.area.width {
            let symbol = c.buf[(x, 0)].symbol();
            if symbol.contains("]8;;https://github.com/acme/repo/pull/12\x07") {
                covered.extend(x..x + c.buf[(x, 0)].cell_width().max(1));
            }
        }
        assert_eq!(covered, (first..=arrow).collect::<Vec<_>>(), "`#` through `↗`, not the key nor the separators");
        assert!(before.buf.content.iter().all(|cell| !cell.symbol().contains("]8;;")), "no pull request, no link");
    }

    #[test]
    fn two_prs_in_a_chain_are_two_boxes_one_below_the_other_linked_by_an_arrow() {
        let prs = [pr("api", &[]), pr("web", &["api"])];
        let jobs = [job("api", "a", &[]), job("api", "b", &["a"]), job("web", "c", &[])];
        let c = layout(&prs, &jobs, 0).chart;
        snap(&c, "
┌ api · api-repo → main · dispatch ┐
│  󰝦 a │S1 ───── 󰝦 b │S1           │
└────────────────┬─────────────────┘
                 │
                 ▼
┌ web · web-repo → main · dispatch ┐
│  󰝦 c │S1                         │
└──────────────────────────────────┘");
        // the arrow is in the lines' colour
        let exit = c.buf.content.iter().find(|cell| cell.symbol() == EXIT).unwrap();
        assert_eq!(Some(exit.fg), palette::fg(Meaning::Nothing).fg);
    }

    #[test]
    fn one_pr_feeding_two_shares_one_run_into_both() {
        let prs = [pr("docs", &["schema"]), pr("schema", &[]), pr("web", &["schema"])];
        let jobs = [job("schema", "types", &[]), job("schema", "gql", &["types"]), job("web", "page", &[]), job("docs", "readme", &[])];
        snap(&layout(&prs, &jobs, 0).chart, "
┌ schema · schema-repo → main · dispatch ┐
│  󰝦 types │S1 ───── 󰝦 gql │S1           │
└───────────────────┬────────────────────┘
                  ╭─┴───────────────────────────────────╮
                  ▼                                     ▼
┌ docs · docs-repo → main · dispatch ┐ ┌ web · web-repo → main · dispatch ┐
│  󰝦 readme │S1                      │ │  󰝦 page │S1                      │
└────────────────────────────────────┘ └──────────────────────────────────┘");
    }

    #[test]
    fn two_prs_feeding_one_join_one_run_into_it() {
        let prs = [pr("api", &[]), pr("db", &[]), pr("web", &["api", "db"])];
        let jobs = [job("api", "a", &[]), job("db", "d", &[]), job("web", "w", &[])];
        snap(&layout(&prs, &jobs, 0).chart, "
┌ api · api-repo → main · dispatch ┐ ┌ db · db-repo → main · dispatch ┐
│  󰝦 a │S1                         │ │  󰝦 d │S1                       │
└────────────────┬─────────────────┘ └───────────────┬────────────────┘
                 ├───────────────────────────────────╯
                 ▼
┌ web · web-repo → main · dispatch ┐
│  󰝦 w │S1                         │
└──────────────────────────────────┘");
    }

    #[test]
    fn a_need_that_skips_a_row_rides_a_column_free_of_boxes() {
        let prs = [pr("a", &[]), pr("b", &["a"]), pr("c", &["a", "b"])];
        snap(&layout(&prs, &[], 0).chart, "
┌ a · a-repo → main · dispatch ┐
│ no Jobs                      │
└──────────────┬───────────────┘
               ├─────────────────╮
               ▼                 │
┌ b · b-repo → main · dispatch ┐ │
│ no Jobs                      │ │
└──────────────┬───────────────┘ │
               ├─────────────────╯
               ▼
┌ c · c-repo → main · dispatch ┐
│ no Jobs                      │
└──────────────────────────────┘");
    }

    /// The pill index of Job `key`.
    fn at(c: &Chart, key: &str) -> usize {
        c.pills.iter().position(|p| p.key == key).unwrap()
    }

    #[test]
    fn left_and_right_cross_boxes_within_a_row_of_boxes_only() {
        // api and docs side by side; web below them
        let prs = [pr("api", &[]), pr("docs", &[]), pr("web", &["api"])];
        let jobs = [job("api", "a", &[]), job("api", "b", &["a"]), job("docs", "d", &[]), job("web", "w", &[])];
        let b = layout(&prs, &jobs, 0);
        let at = |key: &str| at(&b.chart, key);
        assert_eq!(b.chart.columns, [vec![at("a")], vec![at("b")], vec![at("d")], vec![at("w")]]);
        assert_eq!(b.row_of, [0, 0, 0, 1]);
        assert_eq!(b.step(at("b"), Dir::Right), at("d"), "into the next box");
        assert_eq!(b.step(at("d"), Dir::Left), at("b"), "back into the previous one");
        assert_eq!(b.step(at("d"), Dir::Right), at("d"), "not into the next row of boxes");
        assert_eq!(b.step(at("w"), Dir::Left), at("w"), "nor into the previous one");
        // a pill is found where it is drawn
        let r = b.chart.pills[at("w")].rect;
        assert_eq!(b.chart.pill_at(r.x, r.y), Some(at("w")));
        assert_eq!(b.chart.buf[(r.x + 1, r.y)].symbol(), crate::palette::QUEUED);
    }

    #[test]
    fn up_and_down_move_within_a_box_then_cross_to_the_nearest_pill_of_the_next_row_of_boxes() {
        // api: a, then b and c stacked; docs: d; web (below both): w, then x; e2e (below web): e
        let prs = [pr("api", &[]), pr("docs", &[]), pr("e2e", &["web"]), pr("web", &["api", "docs"])];
        let jobs = [
            job("api", "a", &[]),
            job("api", "b", &["a"]),
            job("api", "c", &["a"]),
            job("docs", "d", &[]),
            job("web", "w", &[]),
            job("web", "x", &["w"]),
            job("e2e", "e", &[]),
        ];
        let b = layout(&prs, &jobs, 0);
        let at = |key: &str| at(&b.chart, key);
        assert_eq!((b.step(at("b"), Dir::Down), b.step(at("c"), Dir::Up)), (at("c"), at("b")), "within a box's column");
        // down from the bottom of a box: the nearest pill below by column
        assert_eq!(b.step(at("c"), Dir::Down), at("x"));
        assert_eq!(b.step(at("a"), Dir::Down), at("w"));
        assert_eq!(b.step(at("w"), Dir::Down), at("e"));
        // up from the top of a box: the nearest by column, then the lowest of a stack
        assert_eq!(b.step(at("x"), Dir::Up), at("c"));
        assert_eq!(b.step(at("e"), Dir::Up), at("w"));
        // at the top and the bottom rows, it stays
        assert_eq!((b.step(at("d"), Dir::Up), b.step(at("e"), Dir::Down)), (at("d"), at("e")));
    }

    #[test]
    fn the_view_s_arrow_keys_cross_rows_of_boxes_and_scroll_to_the_selection() {
        let prs = [pr("api", &[]), pr("web", &["api"])];
        let b = layout(&prs, &[job("api", "a", &[]), job("web", "w", &[])], 0).with_margin(Margin { horizontal: 2, vertical: 1 });
        let mut s = Selection::default();
        crate::plan::arrow(&mut s, &b, Dir::Down); // the first arrow selects the first pill
        assert_eq!((s.key.as_deref(), s.follow), (Some("a"), true));
        crate::plan::arrow(&mut s, &b, Dir::Down);
        assert_eq!((s.key.as_deref(), s.follow), (Some("w"), true));
        // a window of 4 rows at the top scrolls down to show w's row
        let w = b.chart.pills[at(&b.chart, "w")].rect;
        let (_, oy) = b.chart.scroll_to(at(&b.chart, "w"), (0, 0), (40, 4));
        assert!(oy <= w.y && w.y < oy + 4, "w at row {} in a window from {oy}", w.y);
        assert_eq!(b.chart.pill_at(w.x, w.y), Some(at(&b.chart, "w")), "clicks find it on the canvas");
    }

    #[test]
    fn up_and_down_skip_a_row_of_boxes_with_no_jobs() {
        let prs = [pr("a", &[]), pr("b", &["a"]), pr("c", &["b"])];
        let b = layout(&prs, &[job("a", "x", &[]), job("c", "z", &[])], 0);
        let at = |key: &str| at(&b.chart, key);
        assert_eq!((b.step(at("x"), Dir::Down), b.step(at("z"), Dir::Up)), (at("z"), at("x")));
    }

    /// The pane's body of `w`×`h` cells showing the margined chart of one empty PR at `scroll`, one row per string.
    fn body(w: u16, h: u16, scroll: (u16, u16)) -> Vec<String> {
        let c = layout(&[pr("api", &[])], &[], 0).chart.with_margin(Margin { horizontal: 2, vertical: 1 });
        let mut dst = Buffer::empty(Rect::new(0, 0, w, h));
        c.blit(&mut dst, Rect::new(0, 0, w, h), scroll, None);
        (0..h).map(|y| (0..w).map(|x| dst[(x, y)].symbol()).collect()).collect()
    }

    #[test]
    fn at_scroll_zero_the_first_box_starts_two_columns_and_one_row_inside_the_body() {
        let rows = body(40, 6, (0, 0));
        assert_eq!(rows[0].trim(), "");
        assert_eq!(rows[1], "  ┌ api · api-repo → main · dispatch ┐  ");
    }

    #[test]
    fn scrolled_the_box_is_drawn_up_to_every_edge_of_the_body() {
        // the box is 34 columns by 3 rows; the body is 10×3, scrolled to the box's top-left corner, then to its
        // bottom-right one (the margin is 2 columns and 1 row on every side of it)
        assert_eq!(body(10, 3, (2, 1)), ["┌ api · ap", "│ no Jobs ", "└─────────"]);
        assert_eq!(body(10, 3, (28, 1)), ["dispatch ┐", "         │", "─────────┘"]);
    }
}
