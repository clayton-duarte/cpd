//! The plan view's PR boxes (commands.md §13.2, R34 A15): one bordered box per PR that is not `cancelled`, holding the
//! chart of that PR's Jobs ([`flow::layout`]) with its title on the top border; the boxes left to right by the PRs'
//! needs, each need an arrow from the needed box's right border to the needing box's left border.
//!
//! In: the PRs and the Jobs. Out: one [`Chart`] of the whole row of boxes, so the selection, the scrolling and the clicks
//! work on it as on one chart: the pills' rectangles are on the whole buffer, and its columns are the boxes' chart
//! columns, box column by box column (boxes stacked in one box column share them), so ←→ cross into the neighbouring
//! box. The arrows are routed as the chart's lines are: one vertical bus per gap between box columns, a need that skips
//! box columns riding a row free of boxes.

use std::collections::HashSet;

use ratatui::{
    buffer::Buffer,
    layout::{Margin, Rect},
    style::Stylize,
    text::{Line, Span},
    widgets::{Block, Widget},
};

use super::flow::{self, Chart, Mask, Pill, free_row, glyph};
use crate::palette::{self, Meaning};
use crate::read::{JobNode, PrNode, PrState};

/// Columns between two box columns: two of line, the bus, one of line and the arrowhead.
const GAP: u16 = 5;
/// What the box of a PR with no Jobs (none that is not cancelled) says.
const NO_JOBS: &str = " no Jobs ";
/// The head of an arrow, just left of the needing box's left border.
const ARROW: &str = "▶";
/// The needed box's right border where an arrow leaves it.
const EXIT: &str = "├";

/// The column of each PR (indices into `prs`): 0 with no needs, else one more than the highest column among its needs.
/// A need that is not one of `prs`, or that closes a cycle, is ignored.
pub fn columns(prs: &[PrNode]) -> Vec<usize> {
    fn column(i: usize, needs: &[Vec<usize>], memo: &mut [Option<usize>], open: &mut HashSet<usize>) -> usize {
        if let Some(c) = memo[i] {
            return c;
        }
        open.insert(i);
        let mut c = 0;
        for &p in &needs[i] {
            if !open.contains(&p) {
                c = c.max(column(p, needs, memo, open) + 1);
            }
        }
        open.remove(&i);
        memo[i] = Some(c);
        c
    }
    let needs: Vec<Vec<usize>> =
        prs.iter().map(|p| p.needs.iter().filter_map(|n| prs.iter().position(|q| &q.key == n)).collect()).collect();
    let mut memo = vec![None; prs.len()];
    (0..prs.len()).map(|i| column(i, &needs, &mut memo, &mut HashSet::new())).collect()
}

/// A PR state's word.
fn state_text(state: PrState) -> &'static str {
    match state {
        PrState::Open => "open",
        PrState::Merged => "merged",
        PrState::Closed => "closed",
    }
}

/// The box's title: ` <key> · <repo name> → <base> · <pull request> `, the key bold; `#<number> <state>` once the PR has a
/// pull request, the state in the palette's PR state colour, else the PR's status, dim.
pub fn title(pr: &PrNode) -> Line<'static> {
    let mut spans: Vec<Span<'static>> =
        vec![" ".into(), pr.key.to_string().bold(), format!(" · {} → {} · ", pr.repo_name, pr.base).into()];
    match pr.pull {
        Some((number, state)) => {
            spans.push(format!("#{number} ").into());
            spans.push(Span::styled(state_text(state), palette::fg(palette::pr_state(state))));
        }
        None => spans.push(pr.status.as_str().dim()),
    }
    spans.push(" ".into());
    Line::from(spans)
}

/// The whole row of boxes: one per PR of `prs` (the PRs that are not `cancelled`), each holding the chart of its `jobs`.
/// The pills are in the order of `prs`, then of each PR's Jobs.
pub fn layout(prs: &[PrNode], jobs: &[JobNode]) -> Chart {
    let charts: Vec<Chart> =
        prs.iter().map(|p| flow::layout(&flow::flow_jobs(jobs.iter().filter(|j| j.pr == p.key)))).collect();
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

    // x: each box column as wide as its widest box, `GAP` between; y: a column's boxes stacked by key, one row apart
    let col_of = columns(prs);
    let ncols = col_of.iter().max().map_or(0, |m| m + 1);
    let mut stacks: Vec<Vec<usize>> = vec![Vec::new(); ncols];
    for (i, &l) in col_of.iter().enumerate() {
        stacks[l].push(i);
    }
    for s in &mut stacks {
        s.sort_by(|a, b| prs[*a].key.cmp(&prs[*b].key));
    }
    let col_w: Vec<u16> = stacks.iter().map(|s| s.iter().map(|&i| size[i].0).max().unwrap_or(0)).collect();
    let xs: Vec<u16> = col_w.iter().scan(0, |x, w| Some(std::mem::replace(x, *x + w + GAP))).collect();
    let mut at = vec![(0u16, 0u16); prs.len()];
    for (l, s) in stacks.iter().enumerate() {
        let mut y = 0;
        for &i in s {
            at[i] = (xs[l], y);
            y += size[i].1 + 1;
        }
    }
    // a box's arrows leave and enter at the middle row of its inside
    let mid = |i: usize| at[i].1 + 1 + (size[i].1 - 3) / 2;
    let right = |i: usize| at[i].0 + size[i].0; // the first column after its right border
    let bus = |l: usize| xs[l] + col_w[l] + 2;

    // the arrows: needs that do not point left (a cycle) are not drawn
    let edges: Vec<(usize, usize)> = (0..prs.len())
        .flat_map(|c| prs[c].needs.iter().filter_map(|n| prs.iter().position(|q| &q.key == n)).map(move |p| (p, c)))
        .filter(|&(p, c)| col_of[p] < col_of[c])
        .collect();
    let busy: Vec<HashSet<u16>> =
        stacks.iter().map(|s| s.iter().flat_map(|&i| at[i].1..at[i].1 + size[i].1).collect()).collect();
    let mut hs: Vec<(u16, u16, u16)> = Vec::new(); // (y, x1, x2)
    let mut bus_rows: Vec<Vec<u16>> = vec![Vec::new(); ncols];
    for &(p, c) in &edges {
        let (lp, lc) = (col_of[p], col_of[c]);
        hs.push((mid(p), right(p), bus(lp)));
        bus_rows[lp].push(mid(p));
        if lc > lp + 1 {
            let y = free_row(mid(p), lp + 1..lc, &busy);
            hs.push((y, bus(lp), bus(lc - 1)));
            bus_rows[lp].push(y);
            bus_rows[lc - 1].push(y);
        }
        bus_rows[lc - 1].push(mid(c));
        hs.push((mid(c), bus(lc - 1), xs[lc] - 1));
    }
    let width = if ncols == 0 { 0 } else { xs[ncols - 1] + col_w[ncols - 1] };
    let boxes_h = (0..prs.len()).map(|i| at[i].1 + size[i].1).max().unwrap_or(0);
    let height = hs.iter().map(|h| h.0 + 1).fold(boxes_h, u16::max);
    let mut mask = Mask::new(width, height);
    for &(y, x1, x2) in &hs {
        mask.h(y, x1, x2);
    }
    for (l, rows) in bus_rows.iter().enumerate() {
        if let (Some(&a), Some(&b)) = (rows.iter().min(), rows.iter().max()) {
            mask.v(bus(l), a, b);
        }
    }

    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    let lines = palette::fg(Meaning::Nothing);
    for (i, chart) in charts.iter().enumerate() {
        let rect = Rect::new(at[i].0, at[i].1, size[i].0, size[i].1);
        Block::bordered().border_style(lines).title(titles[i].clone()).render(rect, &mut buf);
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
    // the lines lie between the boxes, never on them
    for y in 0..height {
        for x in 0..width {
            if mask.at(x, y) != 0 {
                buf[(x, y)].set_symbol(glyph(mask.at(x, y))).set_style(lines);
            }
        }
    }
    for &(p, c) in &edges {
        buf[(right(p) - 1, mid(p))].set_symbol(EXIT).set_style(lines);
        buf[(xs[col_of[c]] - 1, mid(c))].set_symbol(ARROW).set_style(lines);
    }

    // the pills on the whole buffer; the columns box column by box column, then by each box's own chart columns
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
    for s in &stacks {
        let depth = s.iter().map(|&i| charts[i].columns.len()).max().unwrap_or(0);
        for k in 0..depth {
            let column: Vec<usize> = s
                .iter()
                .flat_map(|&i| {
                    let b = base[i];
                    charts[i].columns.get(k).into_iter().flatten().map(move |&p| b + p)
                })
                .collect();
            for &p in &column {
                pills[p].column = columns.len();
            }
            columns.push(column);
        }
    }
    Chart { buf, pills, columns }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Modifier;

    use super::*;
    use crate::plan::flow::Dir;
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
    fn a_box_column_is_one_more_than_the_highest_column_of_its_needs() {
        // web needs api and schema; api needs schema; docs needs nothing; e2e needs web and docs
        let prs = [pr("api", &["schema"]), pr("docs", &[]), pr("e2e", &["docs", "web"]), pr("schema", &[]), pr("web", &["api", "schema"])];
        assert_eq!(columns(&prs), [1, 0, 3, 0, 2]);
        assert_eq!(columns(&[]), Vec::<usize>::new());
        // a cycle is laid out without looping
        assert_eq!(columns(&[pr("a", &["b"]), pr("b", &["a"])]).len(), 2);
    }

    #[test]
    fn boxes_of_one_column_are_stacked_by_key() {
        // given out of key order: the column still stacks them by key, one row apart
        let prs = [pr("zed", &[]), pr("alpha", &[])];
        let c = layout(&prs, &[job("zed", "z", &[]), job("alpha", "a", &[])]);
        let y = |key: &str| c.pills.iter().find(|p| p.key == key).unwrap().rect.y;
        assert!(y("a") < y("z"), "alpha above zed");
        assert_eq!(c.columns, [vec![1, 0]], "one column, alpha's pill first");
    }

    #[test]
    fn two_prs_are_two_titled_boxes_linked_by_an_arrow() {
        let prs = [pr("api", &[]), pr("web", &["api"])];
        let jobs = [job("api", "a", &[]), job("api", "b", &["a"]), job("web", "c", &[])];
        let c = layout(&prs, &jobs);
        snap(&c, "
┌ api · api-repo → main · dispatch ┐     ┌ web · web-repo → main · dispatch ┐
│  ○ a S1 ───── ○ b S1             ├────▶│  ○ c S1                          │
└──────────────────────────────────┘     └──────────────────────────────────┘");
    }

    #[test]
    fn one_pr_is_one_box_and_its_chart_is_the_single_chart_inside() {
        let jobs = [job("api", "a", &[]), job("api", "b", &[]), job("api", "c", &["a", "b"])];
        let c = layout(&[pr("api", &[])], &jobs);
        let single = flow::layout(&flow::flow_jobs(&jobs));
        let rows: Vec<String> = text(&c).lines().map(str::to_owned).collect();
        assert_eq!(rows.len(), single.buf.area.height as usize + 2);
        for (y, want) in text(&single).lines().enumerate() {
            let inside: String = rows[y + 1].chars().skip(1).take(want.chars().count()).collect();
            assert_eq!(inside, want, "row {y}");
        }
    }

    #[test]
    fn a_pr_with_no_jobs_is_an_empty_box() {
        let c = layout(&[pr("api", &[])], &[]);
        snap(&c, "
┌ api · api-repo → main · dispatch ┐
│ no Jobs                          │
└──────────────────────────────────┘");
        assert!(c.buf[(1, 1)].modifier.contains(Modifier::DIM));
        assert!((c.pills.len(), c.columns.len()) == (0, 0));
        assert_eq!(layout(&[], &[]).buf.area.area(), 0);
    }

    #[test]
    fn the_title_names_the_pull_request_in_its_colour_else_the_status_dim() {
        let mut open = pr("api", &[]);
        open.pull = Some((12, PrState::Open));
        let t = title(&open);
        assert_eq!(t.to_string(), " api · api-repo → main · #12 open ");
        assert!(t.spans[1].style.add_modifier.contains(Modifier::BOLD), "the key is bold");
        assert_eq!(t.spans[4].style.fg, palette::fg(palette::pr_state(PrState::Open)).fg);
        for (state, word) in [(PrState::Merged, "merged"), (PrState::Closed, "closed")] {
            open.pull = Some((12, state));
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
    fn a_need_that_skips_a_column_rides_a_row_free_of_boxes() {
        let prs = [pr("a", &[]), pr("b", &["a"]), pr("c", &["a", "b"])];
        let c = layout(&prs, &[]);
        snap(&c, "
┌ a · a-repo → main · dispatch ┐     ┌ b · b-repo → main · dispatch ┐     ┌ c · c-repo → main · dispatch ┐
│ no Jobs                      ├──┬─▶│ no Jobs                      ├──┬─▶│ no Jobs                      │
└──────────────────────────────┘  │  └──────────────────────────────┘  │  └──────────────────────────────┘
                                  ╰────────────────────────────────────╯");
    }

    #[test]
    fn the_selection_crosses_boxes_left_and_right() {
        let prs = [pr("api", &[]), pr("web", &["api"])];
        let jobs = [job("api", "a", &[]), job("api", "b", &["a"]), job("web", "c", &[]), job("web", "d", &["c"])];
        let c = layout(&prs, &jobs);
        let at = |key: &str| c.pills.iter().position(|p| p.key == key).unwrap();
        assert_eq!(c.columns, [vec![at("a")], vec![at("b")], vec![at("c")], vec![at("d")]]);
        assert_eq!(c.step(at("b"), Dir::Right), at("c"), "into the next box");
        assert_eq!(c.step(at("c"), Dir::Left), at("b"), "back into the previous one");
        assert_eq!(c.step(at("d"), Dir::Right), at("d"));
        // a pill is found where it is drawn
        let r = c.pills[at("c")].rect;
        assert_eq!(c.pill_at(r.x, r.y), Some(at("c")));
        assert_eq!(c.buf[(r.x + 1, r.y)].symbol(), "○");
    }

    #[test]
    fn stacked_boxes_share_their_columns() {
        // api and docs in box column 0, web in column 1: Right from either goes to web
        let prs = [pr("api", &[]), pr("docs", &[]), pr("web", &["api", "docs"])];
        let jobs = [job("api", "a", &[]), job("docs", "d", &[]), job("web", "w", &[])];
        let c = layout(&prs, &jobs);
        let at = |key: &str| c.pills.iter().position(|p| p.key == key).unwrap();
        assert_eq!(c.columns, [vec![at("a"), at("d")], vec![at("w")]]);
        assert_eq!((c.step(at("a"), Dir::Down), c.step(at("d"), Dir::Right)), (at("d"), at("w")));
    }
}
