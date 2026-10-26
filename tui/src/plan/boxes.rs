//! The plan view's PR boxes (commands.md §13.2, R34 A15, hs-d115): one bordered box per PR (a `cancelled` one muted, below
//! the others, hs-d518),
//! holding the chart of that PR's Jobs ([`flow::layout`]) with its title on the top border; the boxes top to bottom by
//! the PRs' needs (the PRs not merged below the merged ones, [`levels`]), one row of boxes per level, each need a plain box-drawing line from the needed box's bottom border to the needing
//! box's top border (no arrowhead; hs-d638).
//!
//! In: the PRs and the Jobs. Out: [`Boxes`], one [`Chart`] of the whole canvas of boxes, so the scrolling and the clicks
//! work on it as on one chart (the pills' rectangles are on the whole buffer), and the row of boxes of each of its
//! columns, so the selection moves ←→ within a row of boxes and ↑↓ across rows ([`Boxes::step`]). The arrows are
//! routed as the chart's lines are, turned a quarter: one horizontal run per gap between rows of boxes, a need that
//! skips rows riding a column free of boxes.

use std::collections::HashSet;

use ratatui::{
    buffer::Buffer,
    layout::{Margin, Position, Rect},
    style::Modifier,
    text::{Line, Span},
    widgets::Widget,
};

use cpd_core::plan::Phase;

use super::flow::{self, Chart, Dir, Mask, Pill, free_row, glyph};
use crate::buffer::copy_cells;
use crate::box_widget::{BoxWidget, Walls, geometry, spacing};
use crate::link::{CUE, Link, Links};
use crate::palette::{self, Meaning};
use crate::read::{Dot, JobNode, JobStatus, NeedsUser, PrFacts, PrNode, PrStatus, Pull, pr_look};

/// Rows between two rows of boxes: the run of lines, then the row that reaches the needing box's top border.
const GAP: u16 = 2;
/// What the box of a PR with no Jobs says (the widget's padding is the space around it).
const NO_JOBS: &str = "no Jobs";

/// The body of a PR box: the chart's buffer blitted at the body's corner, or [`NO_JOBS`] when the PR has none.
struct ChartBody<'a> {
    chart: &'a Chart,
}

impl Widget for ChartBody<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.chart.pills.is_empty() {
            Line::from(Span::styled(NO_JOBS, palette::muted_text())).render(area, buf);
        }
        copy_cells(&self.chart.buf, Rect::new(0, 0, area.width, area.height), buf, Position::new(area.x, area.y));
    }
}

/// Between a box title's repo name and its base: `nf-md-arrow_right_thin` (U+F19B0).
const TO: &str = "\u{F19B0}";
/// The needed box's bottom border where a line leaves it.
const EXIT: &str = "┬";

/// The canvas of boxes: the chart of all of them, and the row of boxes (the level, [`levels`]) of each of its columns.
pub struct Boxes {
    pub chart: Chart,
    /// Indexed by the chart's columns.
    pub row_of: Vec<usize>,
    /// The links drawn in the box titles (pro-d60), on the canvas as [`layout`] drew it: `links_origin` is where that
    /// canvas's corner now is (the margin [`Boxes::with_margin`] added), so a cell of the chart is looked up less it.
    pub links: Links,
    pub links_origin: (u16, u16),
}

/// The level of each PR (indices into `prs`). A merged PR's is 0 with no needs, else one more than the highest level
/// among its needs. A PR that is not merged is at max(1 + the highest level among its needs, M + 1), M the highest level
/// of any merged PR (no floor when none is merged): every box that is not merged is below every merged box, so the work
/// in progress sits together (User, 2026-10-18). A need that is not one of `prs`, or that closes a cycle, is ignored.
/// The `cancelled` PRs are all one level below the last of the others (the first when there is none); their needs count for nothing.
pub fn levels(prs: &[PrNode]) -> Vec<usize> {
    fn level(i: usize, floor: usize, needs: &[Vec<usize>], memo: &mut [Option<usize>], open: &mut HashSet<usize>) -> usize {
        if let Some(l) = memo[i] {
            return l;
        }
        open.insert(i);
        let mut l = floor;
        for &p in &needs[i] {
            if !open.contains(&p) {
                l = l.max(level(p, floor, needs, memo, open) + 1);
            }
        }
        open.remove(&i);
        memo[i] = Some(l);
        l
    }
    let needs: Vec<Vec<usize>> = (0..prs.len()).map(|i| live_needs(prs, i)).collect();
    // by the needs alone
    let mut memo = vec![None; prs.len()];
    let base: Vec<usize> = (0..prs.len()).map(|i| level(i, 0, &needs, &mut memo, &mut HashSet::new())).collect();
    let merged = |i: usize| prs[i].status == PrStatus::Merged;
    let by_needs = match (0..prs.len()).filter(|&i| merged(i)).map(|i| base[i]).max() {
        None => base,
        Some(m) => {
            // the merged keep their level; the others are floored below them
            let mut memo: Vec<Option<usize>> = (0..prs.len()).map(|i| merged(i).then_some(base[i])).collect();
            (0..prs.len()).map(|i| level(i, m + 1, &needs, &mut memo, &mut HashSet::new())).collect()
        }
    };
    let below = (0..prs.len()).filter(|&i| prs[i].status != PrStatus::Cancelled).map(|i| by_needs[i] + 1).max().unwrap_or(0);
    (0..prs.len()).map(|i| if prs[i].status == PrStatus::Cancelled { below } else { by_needs[i] }).collect()
}

/// The needs of `prs[i]` that are boxes of `prs` (indices): none of a `cancelled` PR, and none on one.
fn live_needs(prs: &[PrNode], i: usize) -> Vec<usize> {
    if prs[i].status == PrStatus::Cancelled {
        return Vec::new();
    }
    prs[i].needs.iter().filter_map(|n| prs.iter().position(|q| &q.key == n && q.status != PrStatus::Cancelled)).collect()
}

/// The word and colour a title shows: the conflict word when there is one, else the state word (`pr_look`). The title and
/// its link's width both use it.
fn word(pull: &Pull, asked: bool) -> (&'static str, Meaning) {
    let look = pr_look(&PrFacts {
        state: pull.state,
        draft: pull.draft,
        clean: pull.clean,
        conflicting: pull.conflicting,
        owned: true, // every box's pull request is one a Plan owns
        update: pull.update,
        asked,
    });
    look.conflict.unwrap_or(look.state)
}

/// The box's title: ` <key> · <repo name> 󱦰 <base> · <pull request> `, the key bold; `#<number> <state> 󱆭` once the PR has
/// a pull request, the state in the palette's PR state colour (`conflict`, waiting, for an open one that conflicts; `closed`,
/// waiting, red when the Plan's Lead `asked` the User, see [`pr_look`]), else the PR's status, muted. While the Plan's Lead
/// works (`Pull::lead_working`) the Lead's dot, `palette::SPINNER[frame]`, follows the state word: `#12 open 󰝦 󱆭`. The dot's
/// colour is [`Dot::working`]: `plan_failed` makes it the red loader, else `lead_phase` (the Plan's phase; `None`: cyan).
#[cfg(test)]
pub fn title(pr: &PrNode, asked: bool, frame: usize) -> Line<'static> {
    title_of(pr, asked, false, None, frame)
}

/// [`title`] for a Plan that has a failed Job (`plan_failed`) and is in phase `lead_phase`: its working Lead's dot is
/// the red loader when a Job failed, else the bright pair of the phase's colour.
fn title_of(pr: &PrNode, asked: bool, plan_failed: bool, lead_phase: Option<Phase>, frame: usize) -> Line<'static> {
    let read_only = palette::read_only_text();
    let mut spans: Vec<Span<'static>> = vec![
        " ".into(),
        Span::styled(pr.key.to_string(), read_only.add_modifier(Modifier::BOLD)),
        Span::styled(format!(" · {} {TO} {} · ", pr.repo_name, pr.base), read_only),
    ];
    match &pr.pull {
        Some(pull) => {
            // the title is a hit target (the link): clickable text and the bright pairs
            spans.push(Span::styled(format!("#{} ", pull.number), palette::clickable_text()));
            let (word, meaning) = word(pull, asked);
            spans.push(Span::styled(word, palette::fg_clickable(meaning)));
            if pull.lead_working {
                spans.extend([" ".into(), palette::dot_span(Dot::working(plan_failed, lead_phase), frame, false)]);
            }
            spans.push(Span::styled(format!(" {CUE}"), palette::clickable_text()));
        }
        None => spans.push(match (pr.status, &pr.superseded_by) {
            (PrStatus::Cancelled, Some(by)) => format!("cancelled · superseded by {by}").into(),
            (PrStatus::Waiting, _) if !pr.waiting_on.is_empty() => {
                Span::styled(format!("waiting on {}", pr.waiting_on.join(", ")), palette::muted_text())
            }
            (status, _) => Span::styled(status.as_str(), palette::muted_text()),
        }),
    }
    spans.push(" ".into());
    if pr.status == PrStatus::Cancelled {
        // the whole title is muted: the PR is shown, counted nowhere
        spans.iter_mut().for_each(|s| s.style = s.style.patch(palette::muted_text()));
    }
    Line::from(spans)
}

#[cfg(test)]
thread_local! {
    /// How many times [`layout`] ran on this thread (the tests of R2 count them).
    pub(crate) static LAYOUTS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The whole canvas of boxes: one per PR of `prs`, each holding the chart of its
/// `jobs`. The pills are in the order of `prs`, then of each PR's Jobs. A running Job's icon is the spinner's `frame`.
/// `needs_user` says whether the Plan's Lead works (failed pills turn) and asks about a PR.
pub fn layout(prs: &[PrNode], jobs: &[JobNode], needs_user: &NeedsUser, frame: usize) -> Boxes {
    #[cfg(test)]
    LAYOUTS.with(|n| n.set(n.get() + 1));
    let charts: Vec<Chart> = prs
        .iter()
        .map(|p| flow::layout(&flow::flow_jobs(jobs.iter().filter(|j| j.pr == p.key), needs_user.lead_working), frame))
        .collect();
    // the tree's rule: the Plan has a failed Job
    let plan_failed = jobs.iter().any(|j| j.status == JobStatus::Failed);
    let titles: Vec<Line> = prs.iter().map(|p| title_of(p, needs_user.lead_dialog, plan_failed, needs_user.lead_phase, frame)).collect();
    // each box: its walls and padding around the chart, or around `no Jobs`, and at least as wide as its title (the
    // corner, a dash, the title, the corner)
    let size: Vec<(u16, u16)> = charts
        .iter()
        .zip(&titles)
        .map(|(c, t)| {
            let empty = c.pills.is_empty();
            let body_w = c.buf.area.width.max(if empty { NO_JOBS.chars().count() as u16 } else { 0 });
            let inner_h = if empty { 1 } else { c.buf.area.height };
            ((body_w + 2 + 2 * spacing::PAD_X).max(t.width() as u16 + 1 + 2), inner_h + 2)
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
        .flat_map(|c| live_needs(prs, c).into_iter().map(move |p| (p, c)))
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
    let lines = palette::muted_text();
    let mut links = Links::default();
    for (i, chart) in charts.iter().enumerate() {
        let rect = Rect::new(at[i].0, at[i].1, size[i].0, size[i].1);
        BoxWidget::new(Walls::Full, lines, ChartBody { chart }).title(titles[i].clone()).render(rect, &mut buf);
        if let Some(pull) = &prs[i].pull {
            // the title starts after the corner and the dash and ends with one space; the link is the title's tail before its last space:
            // `#<number> <state>`, the Lead's dot when it works (the title's spans after the repo), then the cue
            let spans = &titles[i].spans;
            let link = Link::new(&pull.url).text(Line::from(spans[3..spans.len() - 2].to_vec()));
            let area = Rect::new(rect.x + 1 + titles[i].width() as u16 - link.width(), rect.y, link.width(), 1);
            links.draw(link, area, &mut buf);
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
    for &(p, _) in &edges {
        buf[(mid(p), bottom(p))].set_symbol(EXIT).set_style(lines);
    }

    // the pills on the whole buffer; the columns row of boxes by row, box by box, then by each box's own chart columns
    // a box's body starts where its widget puts it
    let body: Vec<(u16, u16)> = (0..prs.len())
        .map(|i| {
            let inner = geometry(&Walls::Full, Rect::new(at[i].0, at[i].1, size[i].0, size[i].1)).inner;
            (inner.x, inner.y)
        })
        .collect();
    let mut base = Vec::with_capacity(prs.len());
    let mut pills: Vec<Pill> = Vec::new();
    for (i, chart) in charts.iter().enumerate() {
        base.push(pills.len());
        pills.extend(chart.pills.iter().map(|p| Pill {
            key: p.key.clone(),
            rect: Rect { x: p.rect.x + body[i].0, y: p.rect.y + body[i].1, ..p.rect },
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
    Boxes { chart: Chart { buf, pills, columns }, row_of, links, links_origin: (0, 0) }
}

impl Boxes {
    /// The same canvas with `margin` around it ([`Chart::with_margin`]).
    pub fn with_margin(self, margin: Margin) -> Boxes {
        let links_origin = (self.links_origin.0 + margin.horizontal, self.links_origin.1 + margin.vertical);
        Boxes { chart: self.chart.with_margin(margin), links_origin, ..self }
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
    use crate::test_text::rows;
    use crate::read::PrState;
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
            waiting_on: Vec::new(),
            superseded_by: None,
        }
    }

    fn pull(number: i64, state: PrState) -> Pull {
        Pull { number, state, url: format!("https://github.com/acme/repo/pull/{number}"), conflicting: false, clean: false, lead_working: false, update: None, draft: false }
    }

    fn job(pr: &str, key: &str, needs: &[&str]) -> JobNode {
        JobNode {
            key: JobKey::parse(key).unwrap(),
            pr: PrKey::parse(pr).unwrap(),
            seq: 0,
            title: String::new(),
            status: JobStatus::Queued,
            tier: Some(Tier::parse("S1").unwrap()),
            needs: needs.iter().map(|n| JobKey::parse(n).unwrap()).collect(),
            appended: false,
            retry_tier: None,
            failed_cause: None,
            cancel_reason: None,
            merged_sha: None,
            waiting_on: Vec::new(),
            attempts: Vec::new(),
        }
    }

    fn text(c: &Chart) -> String {
        rows(&c.buf).join("\n")
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

    fn merged(key: &str, needs: &[&str]) -> PrNode {
        PrNode { status: PrStatus::Merged, ..pr(key, needs) }
    }

    #[test]
    fn the_prs_not_merged_are_below_every_merged_one_ordered_by_needs() {
        // merged: cpd, drafts, followups (needs cpd); not merged: cleanup (no needs), flake (waiting), status (needs cleanup)
        let prs = [
            merged("cpd", &[]),
            merged("drafts", &[]),
            merged("followups", &["cpd"]),
            pr("cleanup", &[]),
            PrNode { status: PrStatus::Waiting, ..pr("flake", &[]) },
            pr("status", &["cleanup"]),
        ];
        assert_eq!(levels(&prs), [0, 0, 1, 2, 2, 3]);
        // laid out: every pill of a PR not merged is below every pill of a merged one
        let jobs: Vec<JobNode> = prs.iter().map(|p| job(p.key.as_str(), &format!("j-{}", p.key.as_str()), &[])).collect();
        let c = layout(&prs, &jobs, &NeedsUser::default(), 0).chart;
        let y = |key: &str| c.pills.iter().find(|p| p.key == format!("j-{key}")).unwrap().rect.y;
        let last_merged = ["cpd", "drafts", "followups"].iter().map(|k| y(k)).max().unwrap();
        let first_other = ["cleanup", "flake", "status"].iter().map(|k| y(k)).min().unwrap();
        assert!(first_other > last_merged, "{first_other} > {last_merged}");
        assert_eq!(y("cleanup"), y("flake"), "the others sit together, side by side");
        assert!(y("status") > y("cleanup"), "and still by needs");
    }

    #[test]
    fn a_cancelled_pr_is_one_level_below_every_other_and_has_no_needs() {
        let cancelled = |key: &str, needs: &[&str]| PrNode { status: PrStatus::Cancelled, ..pr(key, needs) };
        // `aaa` sorts first and needs `web`; `web` needs the cancelled `old`, which counts for nothing
        let prs = [cancelled("aaa", &["web"]), cancelled("old", &[]), pr("web", &["old"]), pr("z", &["web"])];
        assert_eq!(levels(&prs), [2, 2, 0, 1]);
        assert_eq!(levels(&[cancelled("old", &[])]), [0]);
    }

    #[test]
    fn a_pr_not_merged_that_needs_a_merged_one_sits_below_it_with_an_arrow() {
        let prs = [merged("api", &[]), pr("web", &["api"])];
        assert_eq!(levels(&prs), [0, 1]);
        let b = layout(&prs, &[], &NeedsUser::default(), 0);
        snap(&b.chart, "
╭─ api · api-repo 󱦰 main · merged ╮
│ no Jobs                         │
╰────────────────┬────────────────╯
                 ╰╮
                  │
╭─ web · web-repo 󱦰 main · dispatch ╮
│ no Jobs                           │
╰───────────────────────────────────╯");
    }

    #[test]
    fn a_pr_not_merged_with_no_needs_is_floored_below_the_merged_ones() {
        // the merged chain is two rows deep: the independent open PR goes to the third, not the first
        let prs = [merged("a", &[]), merged("b", &["a"]), pr("c", &[])];
        assert_eq!(levels(&prs), [0, 1, 2]);
        // needs still win over the floor
        let prs = [merged("a", &[]), pr("b", &["a"]), pr("c", &["b"])];
        assert_eq!(levels(&prs), [0, 1, 2]);
        let prs = [merged("a", &[]), pr("x", &[]), pr("y", &["x"]), pr("z", &["y"])];
        assert_eq!(levels(&prs), [0, 1, 2, 3]);
    }

    #[test]
    fn with_no_merged_pr_or_with_all_merged_the_levels_are_the_needs_alone() {
        let names = [("api", &["schema"][..]), ("docs", &[]), ("e2e", &["docs", "web"]), ("schema", &[]), ("web", &["api", "schema"])];
        let none: Vec<PrNode> = names.iter().map(|(k, n)| pr(k, n)).collect();
        let all: Vec<PrNode> = names.iter().map(|(k, n)| merged(k, n)).collect();
        assert_eq!(levels(&none), [1, 0, 3, 0, 2]);
        assert_eq!(levels(&all), [1, 0, 3, 0, 2]);
    }

    #[test]
    fn the_arrow_glyphs_are_one_cell_wide() {
        for glyph in [TO] {
            assert_eq!(Span::raw(glyph).width(), 1, "{glyph:?}");
        }
    }

    #[test]
    fn boxes_of_one_level_sit_side_by_side_by_key() {
        // given out of key order: the row still puts them by key, one column apart
        let prs = [pr("zed", &[]), pr("alpha", &[])];
        let b = layout(&prs, &[job("zed", "z", &[]), job("alpha", "a", &[])], &NeedsUser::default(), 0);
        let c = &b.chart;
        let rect = |key: &str| c.pills.iter().find(|p| p.key == key).unwrap().rect;
        assert!(rect("a").x < rect("z").x && rect("a").y == rect("z").y, "alpha left of zed");
        assert_eq!(c.columns, [vec![1], vec![0]], "alpha's column first");
        assert_eq!(b.row_of, [0, 0]);
        snap(c, "
╭─ alpha · alpha-repo 󱦰 main · dispatch ╮ ╭─ zed · zed-repo 󱦰 main · dispatch ╮
│ 󰝦 a│S1                              │ │ 󰝦 z│S1                          │
╰───────────────────────────────────────╯ ╰───────────────────────────────────╯");
    }

    #[test]
    fn one_pr_is_one_box_and_its_chart_is_the_single_chart_inside() {
        let jobs = [job("api", "a", &[]), job("api", "b", &[]), job("api", "c", &["a", "b"])];
        let c = layout(&[pr("api", &[])], &jobs, &NeedsUser::default(), 0).chart;
        let single = flow::layout(&flow::flow_jobs(&jobs, false), 0);
        let rows: Vec<String> = text(&c).lines().map(str::to_owned).collect();
        assert_eq!(rows.len(), single.buf.area.height as usize + 2);
        for (y, want) in text(&single).lines().enumerate() {
            let inside: String = rows[y + 1].chars().skip(1 + spacing::PAD_X as usize).take(want.chars().count()).collect();
            assert_eq!(inside, want, "row {y}");
        }
        snap(&c, "
╭─ api · api-repo 󱦰 main · dispatch ╮
│ 󰝦 a│S1──╮                       │
│           ├──󰝦 c│S1             │
│ 󰝦 b│S1──╯                       │
╰───────────────────────────────────╯");
    }

    /// The glyphs at the four corners of the buffer of a single box.
    fn corners(c: &Chart) -> [&str; 4] {
        let (w, h) = (c.buf.area.width, c.buf.area.height);
        [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)].map(|(x, y)| c.buf[(x, y)].symbol())
    }

    #[test]
    fn a_box_has_rounded_corners_and_straight_edges() {
        // hs-d474: ╭ ╮ ╰ ╯, not ┌ ┐ └ ┘; the edges stay ─ and │
        for jobs in [vec![], vec![job("api", "a", &[]), job("api", "b", &["a"])]] {
            let c = layout(&[pr("api", &[])], &jobs, &NeedsUser::default(), 0).chart;
            assert_eq!(corners(&c), ["╭", "╮", "╰", "╯"], "{} Jobs", jobs.len());
            let (w, h) = (c.buf.area.width, c.buf.area.height);
            assert_eq!((c.buf[(w / 2, h - 1)].symbol(), c.buf[(0, 1)].symbol(), c.buf[(w - 1, 1)].symbol()), ("─", "│", "│"));
            let shown = text(&c);
            assert!(!shown.contains(['┌', '┐', '└', '┘']), "{shown}");
        }
    }

    #[test]
    fn every_box_of_a_canvas_is_rounded_and_the_links_still_join_with_t_junctions() {
        let prs = [pr("api", &[]), pr("web", &["api"])];
        let c = layout(&prs, &[job("api", "a", &[]), job("web", "c", &[])], &NeedsUser::default(), 0).chart;
        let shown = text(&c);
        assert_eq!([('╭', 2), ('╮', 2), ('╰', 2), ('╯', 2)].map(|(g, _)| shown.matches(g).count()), [2, 2, 2, 2], "two boxes, four corners each: {shown}");
        assert!(!shown.contains(['┌', '┐', '└', '┘']), "{shown}");
        // the needed box's bottom border leaves by a ┬, the line reaches the needing box's top border
        assert_eq!(shown.matches('┬').count(), 1, "{shown}");
        assert!(shown.lines().any(|l| l.starts_with("╰") && l.contains('┬') && l.ends_with('╯')), "{shown}");
    }

    #[test]
    fn a_pr_with_no_jobs_is_an_empty_box() {
        let c = layout(&[pr("api", &[])], &[], &NeedsUser::default(), 0).chart;
        snap(&c, "
╭─ api · api-repo 󱦰 main · dispatch ╮
│ no Jobs                           │
╰───────────────────────────────────╯");
        assert!(crate::palette::check::is_muted(&c.buf[(2, 1)]));
        assert!((c.pills.len(), c.columns.len()) == (0, 0));
        assert_eq!(layout(&[], &[], &NeedsUser::default(), 0).chart.buf.area.area(), 0);
    }

    #[test]
    fn an_open_pull_request_that_conflicts_reads_conflict_in_the_waiting_colour() {
        // every box is a PR a Plan owns: its Lead fixes the conflict (hs-d302), so it is waiting, never red
        let failed = palette::fg_clickable(Meaning::Waiting).fg;
        let mut p = pr("api", &[]);
        p.pull = Some(Pull { conflicting: true, ..pull(12, PrState::Open) });
        let t = title(&p, false, 0);
        assert_eq!(t.to_string(), " api · api-repo 󱦰 main · #12 conflict 󱆭 ");
        assert_eq!(t.spans[4].style.fg, failed);
        assert_ne!(t.spans[4].style.fg, palette::fg_clickable(Meaning::Failed).fg);
        // `unknown` and `mergeable` are `conflicting: false`: `open`, not in the waiting colour
        p.pull = Some(pull(12, PrState::Open));
        let t = title(&p, false, 0);
        assert_eq!(t.to_string(), " api · api-repo 󱦰 main · #12 open 󱆭 ");
        assert_ne!(t.spans[4].style.fg, failed);
        // merged and closed never read `conflict`
        for (state, word) in [(PrState::Merged, "merged"), (PrState::Closed, "closed")] {
            p.pull = Some(Pull { conflicting: true, ..pull(12, state) });
            let t = title(&p, false, 0);
            assert_eq!(t.spans[4].content, word);
            assert_eq!(t.spans[4].style.fg, palette::fg_clickable(palette::pr_state(state)).fg);
        }
    }

    #[test]
    fn a_working_lead_puts_its_dot_after_the_state_word_and_turns_with_the_frame() {
        let mut p = pr("api", &[]);
        p.pull = Some(Pull { lead_working: true, ..pull(12, PrState::Open) });
        for frame in 0..palette::SPINNER.len() {
            let t = title(&p, false, frame);
            assert_eq!(t.to_string(), format!(" api · api-repo 󱦰 main · #12 open {} 󱆭 ", palette::SPINNER[frame]), "frame {frame}");
            assert_eq!(t.spans[4].style.fg, palette::fg_clickable(palette::pr_state(PrState::Open)).fg, "the state keeps its colour");
        }
        // the dot goes with the state, in the waiting colour of `conflict` too
        p.pull = Some(Pull { lead_working: true, conflicting: true, ..pull(12, PrState::Open) });
        let t = title(&p, false, 3);
        assert_eq!(t.to_string(), format!(" api · api-repo 󱦰 main · #12 conflict {} 󱆭 ", palette::SPINNER[3]));
        assert_eq!(t.spans[4].style.fg, palette::fg_clickable(Meaning::Waiting).fg);
    }

    #[test]
    fn an_idle_lead_shows_no_dot_and_a_pr_without_a_pull_request_none() {
        let mut p = pr("api", &[]);
        p.pull = Some(pull(12, PrState::Open));
        for frame in 0..palette::SPINNER.len() {
            let text = title(&p, false, frame).to_string();
            assert_eq!(text, " api · api-repo 󱦰 main · #12 open 󱆭 ");
            assert!(!palette::SPINNER.iter().any(|g| text.contains(g)), "{text}");
        }
        // no pull request yet: nothing to show beside
        let mut waiting = pr("web", &["api"]);
        waiting.status = PrStatus::Waiting;
        assert_eq!(title(&waiting, false, 4).to_string(), " web · web-repo 󱦰 main · waiting ");
    }

    #[test]
    fn the_working_dot_is_drawn_in_the_box_title_inside_the_link_and_widens_the_box_by_its_cell() {
        let mut p = pr("api", &[]);
        p.pull = Some(pull(12, PrState::Open));
        let mut w = p.clone();
        w.pull = Some(Pull { lead_working: true, ..pull(12, PrState::Open) });
        let plain = layout(&[p], &[], &NeedsUser::default(), 5).chart;
        let working = layout(&[w], &[], &NeedsUser::default(), 5).chart;
        assert_eq!(working.buf.area.width, plain.buf.area.width + 2);
        let top: String = (0..working.buf.area.width).map(|x| working.buf[(x, 0)].symbol().to_owned()).collect();
        assert!(top.contains(palette::SPINNER[5]), "{top}");
        // one frame later: the next glyph
        let w2 = layout(&[PrNode { pull: Some(Pull { lead_working: true, ..pull(12, PrState::Open) }), ..pr("api", &[]) }], &[], &NeedsUser::default(), 6).chart;
        let top: String = (0..w2.buf.area.width).map(|x| w2.buf[(x, 0)].symbol().to_owned()).collect();
        assert!(top.contains(palette::SPINNER[6]) && !top.contains(palette::SPINNER[5]), "{top}");
    }

    #[test]
    fn a_closed_pull_request_is_waiting_and_red_only_when_its_lead_asked_the_user() {
        let mut p = pr("api", &[]);
        p.pull = Some(pull(12, PrState::Closed));
        let colour = |asked| title(&p, asked, 0).spans[4].style.fg;
        assert_eq!(colour(false), palette::fg_clickable(Meaning::Waiting).fg, "its Lead handles it");
        assert_eq!(colour(true), palette::fg_clickable(Meaning::Failed).fg, "its Lead has a dialog open: it asks the User");
        // the question concerns the closed pull request only
        for state in [PrState::Open, PrState::Merged] {
            p.pull = Some(pull(12, state));
            assert_eq!(title(&p, true, 0).spans[4].style.fg, title(&p, false, 0).spans[4].style.fg, "{state:?}");
        }
        // through the layout: the title is drawn in the colour
        p.pull = Some(pull(12, PrState::Closed));
        let cell = |needs: &NeedsUser| {
            let c = layout(&[p.clone()], &[], needs, 0).chart;
            let x = (0..c.buf.area.width).find(|&x| c.buf[(x, 0)].symbol().contains("\x07cl")).unwrap();
            c.buf[(x, 0)].fg
        };
        assert_eq!(cell(&NeedsUser::default()), palette::color_clickable(Meaning::Waiting));
        assert_eq!(cell(&NeedsUser { lead_dialog: true, ..NeedsUser::default() }), palette::color_clickable(Meaning::Failed));
    }

    #[test]
    fn the_title_names_the_pull_request_in_its_colour_else_the_status_dim() {
        let mut open = pr("api", &[]);
        open.pull = Some(pull(12, PrState::Open));
        let t = title(&open, false, 0);
        assert_eq!(t.to_string(), " api · api-repo 󱦰 main · #12 open 󱆭 ");
        assert!(t.spans[1].style.add_modifier.contains(Modifier::BOLD), "the key is bold");
        assert_eq!(t.spans[4].style.fg, palette::fg_clickable(palette::pr_state(PrState::Open)).fg);
        for (state, word) in [(PrState::Merged, "merged"), (PrState::Closed, "closed")] {
            open.pull = Some(pull(12, state));
            let t = title(&open, false, 0);
            assert_eq!(t.spans[4].content, word);
            assert_eq!(t.spans[4].style.fg, palette::fg_clickable(palette::pr_state(state)).fg);
        }
        let mut waiting = pr("web", &["api"]);
        waiting.status = PrStatus::Waiting;
        let t = title(&waiting, false, 0);
        assert_eq!(t.to_string(), " web · web-repo 󱦰 main · waiting ");
        assert_eq!(t.spans[3].style, palette::muted_text(), "the status before a pull request exists is muted");
    }

    /// The cells of the title row (y 0) that are the link to the pull request, and the cells of `#` and `󱆭` in the chart.
    fn link_cells(b: &Boxes) -> (Vec<u16>, u16, u16) {
        let c = &b.chart;
        let top: Vec<char> = text(c).lines().next().unwrap().chars().collect();
        let first = top.iter().position(|&ch| ch == '#').unwrap() as u16;
        let arrow = top.iter().position(|&ch| ch.to_string() == CUE).unwrap() as u16;
        let covered = (0..c.buf.area.width).filter(|&x| b.links.at(Position::new(x, 0)).is_some()).collect();
        (covered, first, arrow)
    }

    #[test]
    fn a_conflicting_pull_request_links_exactly_its_number_conflict_and_arrow() {
        let mut api = pr("api", &[]);
        api.pull = Some(Pull { conflicting: true, ..pull(12, PrState::Open) });
        let b = layout(&[api], &[], &NeedsUser::default(), 0);
        snap(&b.chart, "
╭─ api · api-repo 󱦰 main · #12 conflict 󱆭 ╮
│ no Jobs                                 │
╰─────────────────────────────────────────╯");
        let (covered, first, arrow) = link_cells(&b);
        assert_eq!(covered, (first..=arrow).collect::<Vec<_>>(), "`#12 conflict 󱆭` whole, not the key nor the separators");
    }

    #[test]
    fn a_pull_request_adds_its_arrow_to_the_title_and_widens_the_box_by_it() {
        let mut api = pr("api", &[]);
        let before = layout(&[api.clone()], &[], &NeedsUser::default(), 0);
        api.pull = Some(pull(12, PrState::Open));
        let b = layout(&[api], &[], &NeedsUser::default(), 0);
        let c = &b.chart;
        snap(c, "
╭─ api · api-repo 󱦰 main · #12 open 󱆭 ╮
│ no Jobs                             │
╰─────────────────────────────────────╯");
        assert_eq!(c.buf.area.width, before.chart.buf.area.width + 2, "`#12 open 󱆭` is two cells longer than `dispatch`");
        let (covered, first, arrow) = link_cells(&b);
        assert_eq!(covered, (first..=arrow).collect::<Vec<_>>(), "`#` through `󱆭`, not the key nor the separators");
        assert_eq!(before.links, Links::default(), "no pull request, no link");
    }

    #[test]
    fn two_prs_in_a_chain_are_two_boxes_one_below_the_other_linked_by_an_arrow() {
        let prs = [pr("api", &[]), pr("web", &["api"])];
        let jobs = [job("api", "a", &[]), job("api", "b", &["a"]), job("web", "c", &[])];
        let c = layout(&prs, &jobs, &NeedsUser::default(), 0).chart;
        snap(&c, "
╭─ api · api-repo 󱦰 main · dispatch ╮
│ 󰝦 a│S1─────󰝦 b│S1             │
╰─────────────────┬─────────────────╯
                  │
                  │
╭─ web · web-repo 󱦰 main · dispatch ╮
│ 󰝦 c│S1                          │
╰───────────────────────────────────╯");
        // the exit and the line are in the lines' colour
        let exit = c.buf.content.iter().find(|cell| cell.symbol() == EXIT).unwrap();
        assert_eq!(Some(exit.fg), palette::fg(Meaning::Nothing).fg);
    }

    #[test]
    fn one_pr_feeding_two_shares_one_run_into_both() {
        let prs = [pr("docs", &["schema"]), pr("schema", &[]), pr("web", &["schema"])];
        let jobs = [job("schema", "types", &[]), job("schema", "gql", &["types"]), job("web", "page", &[]), job("docs", "readme", &[])];
        snap(&layout(&prs, &jobs, &NeedsUser::default(), 0).chart, "
╭─ schema · schema-repo 󱦰 main · dispatch ╮
│ 󰝦 types│S1─────󰝦 gql│S1             │
╰────────────────────┬────────────────────╯
                   ╭─┴────────────────────────────────────╮
                   │                                      │
╭─ docs · docs-repo 󱦰 main · dispatch ╮ ╭─ web · web-repo 󱦰 main · dispatch ╮
│ 󰝦 readme│S1                       │ │ 󰝦 page│S1                       │
╰─────────────────────────────────────╯ ╰───────────────────────────────────╯");
    }

    #[test]
    fn two_prs_feeding_one_join_one_run_into_it() {
        let prs = [pr("api", &[]), pr("db", &[]), pr("web", &["api", "db"])];
        let jobs = [job("api", "a", &[]), job("db", "d", &[]), job("web", "w", &[])];
        snap(&layout(&prs, &jobs, &NeedsUser::default(), 0).chart, "
╭─ api · api-repo 󱦰 main · dispatch ╮ ╭─ db · db-repo 󱦰 main · dispatch ╮
│ 󰝦 a│S1                          │ │ 󰝦 d│S1                        │
╰─────────────────┬─────────────────╯ ╰────────────────┬────────────────╯
                  ├────────────────────────────────────╯
                  │
╭─ web · web-repo 󱦰 main · dispatch ╮
│ 󰝦 w│S1                          │
╰───────────────────────────────────╯");
    }

    #[test]
    fn a_need_that_skips_a_row_rides_a_column_free_of_boxes() {
        let prs = [pr("a", &[]), pr("b", &["a"]), pr("c", &["a", "b"])];
        snap(&layout(&prs, &[], &NeedsUser::default(), 0).chart, "
╭─ a · a-repo 󱦰 main · dispatch ╮
│ no Jobs                       │
╰───────────────┬───────────────╯
                ├─────────────────╮
                │                 │
╭─ b · b-repo 󱦰 main · dispatch ╮ │
│ no Jobs                       │ │
╰───────────────┬───────────────╯ │
                ├─────────────────╯
                │
╭─ c · c-repo 󱦰 main · dispatch ╮
│ no Jobs                       │
╰───────────────────────────────╯");
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
        let b = layout(&prs, &jobs, &NeedsUser::default(), 0);
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
        assert_eq!((b.chart.buf[(r.x, r.y)].symbol(), b.chart.buf[(r.right() - 1, r.y)].symbol()), ("\u{e0b6}", "\u{e0b4}"), "the caps are on the whole pill's rectangle");
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
        let b = layout(&prs, &jobs, &NeedsUser::default(), 0);
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
        let b = layout(&prs, &[job("api", "a", &[]), job("web", "w", &[])], &NeedsUser::default(), 0).with_margin(Margin { horizontal: 2, vertical: 1 });
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
        let b = layout(&prs, &[job("a", "x", &[]), job("c", "z", &[])], &NeedsUser::default(), 0);
        let at = |key: &str| at(&b.chart, key);
        assert_eq!((b.step(at("x"), Dir::Down), b.step(at("z"), Dir::Up)), (at("z"), at("x")));
    }

    /// The pane's body of `w`×`h` cells showing the margined chart of one empty PR at `scroll`, one row per string.
    fn body(w: u16, h: u16, scroll: (u16, u16)) -> Vec<String> {
        let c = layout(&[pr("api", &[])], &[], &NeedsUser::default(), 0).chart.with_margin(Margin { horizontal: 2, vertical: 1 });
        let mut dst = Buffer::empty(Rect::new(0, 0, w, h));
        c.blit(&mut dst, Rect::new(0, 0, w, h), scroll, None);
        (0..h).map(|y| (0..w).map(|x| dst[(x, y)].symbol()).collect()).collect()
    }

    #[test]
    fn at_scroll_zero_the_first_box_starts_two_columns_and_one_row_inside_the_body() {
        let rows = body(40, 6, (0, 0));
        assert_eq!(rows[0].trim(), "");
        assert_eq!(rows[1], "  ╭─ api · api-repo 󱦰 main · dispatch ╮ ");
    }

    #[test]
    fn scrolled_the_box_is_drawn_up_to_every_edge_of_the_body() {
        // the box is 34 columns by 3 rows; the body is 10×3, scrolled to the box's top-left corner, then to its
        // bottom-right one (the margin is 2 columns and 1 row on every side of it)
        assert_eq!(body(10, 3, (2, 1)), ["╭─ api · a", "│ no Jobs ", "╰─────────"]);
        assert_eq!(body(10, 3, (29, 1)), ["dispatch ╮", "         │", "─────────╯"]);
    }
}
