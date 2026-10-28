//! The Dispatch column's drawing (commands.md §13.2), a port of the reference's `draw_dispatch` and `pr::render`
//! (`tui/src/main.rs`, `tui/src/pr.rs`): the machine (hostname, CPU %, one % per GPU, memory %) and the open pull
//! requests (two lines each), under one blank row; each section but the first starts with a header. Pure: [`draw`] only writes to the frame.

use cpd_core::ids::SessionId;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::host::Usage;
use crate::cells;
use crate::dialog::scroll::{self, Cue};
use crate::link::{self, Link, Links};
use crate::palette::{self, Meaning};
use crate::box_widget::SectionTitle;
use super::inbox::{self, Shape, inbox_share};
use cpd_core::inbox::model::Item;
use time::OffsetDateTime;
use crate::read::{Dot, OpenPr, PrFacts, PrState, ReviewDecision, pr_look};

/// The glyph in the last column of a clean PR's second line: md-source_merge, U+F062D (Material Design Nerd Font).
const MERGE: &str = "\u{F062D}";

/// What a click on an owned PR reaches: its two lines, except the `󱆭` cell (the terminal opens that link).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// The PR's two rows, the column's full width.
    pub area: Rect,
    /// The `󱆭` cell, inside `area`.
    pub arrow: Position,
    /// The merge icon's cell, inside `area`, when the PR shows one, or the spinner's while its merge runs: no click on
    /// it reaches the Lead.
    pub icon: Option<Position>,
    /// The Lead of the owner: `cpd session open <lead>`.
    pub lead: SessionId,
}

impl Hit {
    pub fn contains(&self, at: Position) -> bool {
        self.area.contains(at) && at != self.arrow && Some(at) != self.icon
    }
}

/// A merge icon on the screen: a click on `area` (one cell) starts the merge of `pull`, `<owner>/<repo>#<n>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeIcon {
    pub area: Rect,
    pub pull: String,
}

/// An entry of a PR row's right-click menu (commands.md §13.2, hs-d296).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrItem {
    /// The merge, at once: only on a row that shows the merge icon.
    Merge,
    /// A merge of this pull request is running (hs-d495): the `Merge` entry, shown with the spinner and not selectable.
    Merging,
    /// `tmux set-buffer -w -- <url>`.
    CopyLink,
    /// `session open` of the owner's Lead, then the focus on the chat: only on a PR a Plan owns.
    GoToLead,
}

impl crate::menu::Entry for PrItem {
    fn label(self) -> &'static str {
        match self {
            PrItem::Merge | PrItem::Merging => "Merge",
            PrItem::CopyLink => "Copy Link",
            PrItem::GoToLead => "Go to Lead",
        }
    }

    fn busy(self) -> bool {
        self == PrItem::Merging
    }
}

/// A PR row on the screen, whatever its owner: a right click on its two lines opens its menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrRow {
    /// The PR's two rows, the column's full width.
    pub area: Rect,
    /// `<owner>/<repo>#<n>`.
    pub pull: String,
    pub url: String,
    /// The row shows the merge icon.
    pub clean: bool,
    /// A merge of the pull request is running (hs-d495): the menu's Merge entry is [`PrItem::Merging`].
    pub merging: bool,
    pub lead: Option<SessionId>,
}

impl PrRow {
    /// The menu's entries, in order: Merge (a clean row; busy while its merge runs), Copy Link, Go to Lead (an owned one).
    pub fn items(&self) -> Vec<PrItem> {
        let merge = if self.merging { PrItem::Merging } else { PrItem::Merge };
        [self.clean.then_some(merge), Some(PrItem::CopyLink), self.lead.as_ref().map(|_| PrItem::GoToLead)]
            .into_iter()
            .flatten()
            .collect()
    }
}

/// Nerd Font `nf-oct-cpu` and `nf-md-memory`: one cell each.
const CPU_ICON: &str = "\u{F4BC}";
const GPU_ICON: &str = "\u{E266}";
const MEM_ICON: &str = "\u{EFC5}";

/// The machine the column names: its hostname and its CPU % and memory %.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Machine {
    pub name: String,
    pub usage: Usage,
}

impl Machine {
    /// ` <cpu icon> N%  <gpu icon> A% B%  <mem icon> M%`, one % per GPU and no GPU part when there is none; a value not known (the
    /// CPU until a second sample, or one that cannot be read) is `–`.
    fn usage_line(&self) -> String {
        let known = |v: Option<u8>| v.map_or("–".to_owned(), |v| v.to_string());
        let gpus = &self.usage.gpus;
        let gpu = if gpus.is_empty() { String::new() } else { format!("{GPU_ICON} {}  ", gpus.iter().map(|g| format!("{}%", known(*g))).collect::<Vec<_>>().join(" ")) };
        format!(" {CPU_ICON} {}%  {gpu}{MEM_ICON} {}%", known(self.usage.cpu), known(self.usage.mem))
    }
}

fn bold(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), palette::read_only_text().add_modifier(Modifier::BOLD))
}

pub(crate) fn muted(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), palette::muted_text())
}

/// `text` cut to `max` columns, ending in `…` when cut.
fn truncate(text: &str, max: usize) -> String {
    cells::truncate(text, max)
}

/// One open pull request, two lines (commands.md §13.2): ` owner/repo#n` with `󱆭` in the last column, then
/// `  <open|draft> <checks> <review>  <title>`, the title cut first to fit `width`; the name is cut so it never reaches the `󱆭`.
/// A `clean` one ends the second line in the merge icon, in the last column; the title is cut so it never reaches it.
/// A conflicting one adds ` conflict` after the review. A pull request a Plan owns is clickable, so its colourless text is 15 and
/// its coloured marks the bright pairs, and it draws `conflict` and failed checks in `Meaning::Waiting` (its Lead fixes them,
/// hs-d302); one no Plan owns is read-only: text 7, base colours, red `conflict` and failed checks (hs-d534). An `open` word is
/// yellow only when the PR is `clean`, ready to merge. An `active` one (a PR of the chat slot's Plan) is the blue block, bold,
/// across both lines, with no colour of its own.
/// One whose Lead is working shows the Lead's dot, `palette::SPINNER[frame]` (the tree's), after the state word: `  open 󰝦 …`.
/// Its colour is [`Dot::working`]: the red loader when `plan_failed`, else the bright pair of the owning Plan's phase
/// (`plan_phase`; `None`: cyan, hs-d622).
pub(crate) fn pr_lines(pr: &OpenPr, width: usize, frame: usize) -> [Line<'static>; 2] {
    let name = truncate(&format!(" {}#{}", pr.repo, pr.number), width.saturating_sub(2));
    let free = width.saturating_sub(cells::width(&name) + 1);
    let jira = pr.jira.as_deref().filter(|key| free > 2 + cells::width(key)).map(|key| muted(format!("  {key}")));
    let gap = " ".repeat(free.saturating_sub(jira.as_ref().map_or(0, |j| j.width())));
    let review = match pr.review_decision {
        ReviewDecision::Approved => "approved",
        ReviewDecision::ChangesRequested => "changes requested",
        ReviewDecision::None => "no review",
    };
    let owned = pr.lead.is_some();
    let (icon, checks) = palette::checks_of(pr.checks, owned);
    let look = pr_look(&PrFacts {
        state: PrState::Open,
        draft: pr.draft,
        clean: pr.clean,
        conflicting: pr.conflicting,
        owned,
        update: pr.update,
        asked: false,
    });
    let (state_text, state_meaning) = look.state;
    // the colour of a mark that has a meaning, and the colourless text: bright and 15 when the PR is owned (clickable)
    let mark = |m| if owned { palette::fg_clickable(m) } else { palette::fg(m) };
    let text = if owned { palette::clickable_text() } else { palette::read_only_text() };
    let dot = pr.lead_working.then(|| palette::dot_span(Dot::working(pr.plan_failed, pr.plan_phase), frame, false));
    let used = 2 + cells::width(state_text) + if dot.is_some() { 2 } else { 0 } + 3 + cells::width(review) + look.conflict.map_or(0, |(word, _)| 1 + cells::width(word));
    let mut spans = vec![
        "  ".into(),
        Span::styled(state_text, mark(state_meaning)),
    ];
    if let Some(dot) = dot {
        spans.extend([" ".into(), dot]);
    }
    spans.extend([
        " ".into(),
        Span::styled(icon, mark(checks)),
        " ".into(),
        Span::styled(review, text),
    ]);
    if let Some((word, meaning)) = look.conflict {
        spans.push(Span::styled(format!(" {word}"), mark(meaning)));
    }
    let reserved = if pr.clean { 2 } else { 0 };
    let room = width.saturating_sub(used + 2 + reserved);
    let mut shown = used;
    if room >= 4 {
        let title = format!("  {}", truncate(&pr.title, room));
        shown += cells::width(&title);
        spans.push(muted(title));
    }
    if pr.clean {
        spans.push(" ".repeat(width.saturating_sub(shown + 1)).into());
        // While its merge runs the cell is the spinner (hs-d495), turning with the shared clock.
        spans.push(if pr.merging {
            Span::styled(palette::SPINNER[frame % palette::SPINNER.len()], palette::fg_clickable(Meaning::Review))
        } else {
            Span::styled(MERGE, palette::clickable_text())
        });
    }
    let mut first = vec![Span::styled(name, text.add_modifier(Modifier::BOLD))];
    first.extend(jira);
    first.extend([gap.into(), Span::styled(link::CUE, palette::clickable_text())]);
    let first = Line::from(first);
    let second = Line::from(spans);
    [first, second].map(|line| if pr.active { block(line, width) } else { line })
}

/// `line` as the blue block, bold, `width` cells wide: every span takes the block's colours, so no mark keeps a colour of its own
/// (rule (3)), and a span of spaces fills the rest of the row.
fn block(mut line: Line<'static>, width: usize) -> Line<'static> {
    let style = palette::selected().add_modifier(Modifier::BOLD);
    line.style = style;
    line.spans.iter_mut().for_each(|s| s.style = s.style.patch(style));
    line.spans.push(Span::styled(" ".repeat(width.saturating_sub(line.width())), style));
    line
}

/// Which items a scrolling list draws, and the cues around them (commands.md §13.2, "Scrolling the PRs section"; the
/// tree's Inbox section scrolls with the same code, one row an item).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Window {
    /// The first item drawn, after clamping.
    pub first: usize,
    /// How many are drawn.
    pub shown: usize,
    /// A `▲ <first> more` row (right-aligned, `scroll::cue`) opens the list.
    pub cue_above: bool,
    /// How many are hidden below: a `▼ <n> more` row ends the list when not 0.
    pub below: usize,
}

/// The largest offset for `count` items of `item_rows` rows each in a list of `height` rows: the last one reaches the
/// bottom (under the `▲` cue, which takes a row). 0 when every one fits.
pub(crate) fn max_scroll(count: usize, height: usize, item_rows: usize) -> usize {
    if count * item_rows <= height {
        return 0;
    }
    count.saturating_sub((height.saturating_sub(1)) / item_rows).min(count.saturating_sub(1))
}

/// The window at offset `scroll` (clamped to [`max_scroll`]). A cue takes one row of the list, and an item is whole.
pub(crate) fn window(count: usize, height: usize, item_rows: usize, scroll: usize) -> Window {
    let first = scroll.min(max_scroll(count, height, item_rows));
    let cue_above = first > 0 && height > 0;
    let room = height - usize::from(cue_above);
    let left = count - first;
    if left * item_rows <= room {
        return Window { first, shown: left, cue_above, below: 0 };
    }
    let shown = room.saturating_sub(1) / item_rows;
    Window { first, shown, cue_above, below: left - shown }
}

/// What [`draw`] reports to the view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drawn {
    /// Where a click reaches an owned PR: the pull requests on screen.
    pub hits: Vec<Hit>,
    /// The PRs section, where a wheel event scrolls.
    pub prs: Rect,
    /// The largest scroll offset (in pull requests) of the section as drawn.
    pub max_scroll: usize,
    /// Where a click reaches a merge icon: the pull requests on screen that show one.
    pub icons: Vec<MergeIcon>,
    /// Where a right click reaches a pull request: every one on screen, owned or not.
    pub rows: Vec<PrRow>,
    /// Every link drawn (the PR rows' cues): a click on one opens its url.
    pub links: Links,
    /// The Inbox section, pinned to the bottom (its first row is the empty one): the wheel there scrolls it, a click on its
    /// toggle row folds or unfolds it.
    pub inbox: Rect,
    /// The Inbox section as drawn: capped by the column's split and scrolled ([`Shape::hit`], [`Shape::wheeled`]).
    pub inbox_shape: Shape,
}

/// Draws the column in `area`: a blank row, the machine, the PRs under their header and the Inbox section pinned last
/// (commands.md §13.2, pro-d55). The PRs and the Inbox share the rows below the machine as the tree and the Inbox did
/// ([`inbox_share`], hs-d307), the PRs in the tree's place. The PRs section
/// starts at the pull request `scroll` (clamped), with a dim `▲`/`▼` cue row where some are hidden. The Inbox draws `inbox`, folded or
/// `expanded`, from the item `inbox_scroll`, its ages taken at `now`, the spinner in the glyph cell of a `pr-ready` item whose pull
/// request is in `merging`. It draws no note
/// (commands.md §17 "No notes"); the snapshot's error is [`draw_error`]'s. `frame` is the spinner's frame (`palette::frame`) for the rows whose
/// Lead is working. Returns where a click reaches an owned PR or a merge icon.
#[allow(clippy::too_many_arguments)]
pub fn draw(
    f: &mut Frame,
    area: Rect,
    open: &[OpenPr],
    machine: &Machine,
    scroll: usize,
    frame: usize,
    inbox: &[Item],
    (expanded, inbox_scroll): (bool, usize),
    now: OffsetDateTime,
    merging: &std::collections::BTreeSet<String>,
) -> Drawn {
    // the PRs and the Inbox share the rows under the blank row and the machine; the Inbox leaves the PRs one row
    let column = area.height.saturating_sub(3);
    let prs_rows = usize::from(SectionTitle::ROWS) + (2 * open.len()).max(1);
    let shape = Shape::new(inbox.len(), expanded).capped(inbox_share(prs_rows, inbox.len(), expanded, column)).scrolled(inbox_scroll);
    let inbox_height = shape.height().min(column.saturating_sub(1));
    let [_top, home, prs_section, inbox_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(2), Constraint::Min(0), Constraint::Length(inbox_height)]).areas(area);
    let [prs_header, prs] = Layout::vertical([Constraint::Length(SectionTitle::ROWS), Constraint::Min(1)]).areas(prs_section);
    // the section's title carries the padding itself, so the section is drawn across the column
    let mut links = inbox::draw(f, inbox_area, inbox, shape, None, now, &|item| inbox::merges(merging, item).then_some(frame));
    f.render_widget(SectionTitle::new("PRs", Some(open.len())), prs_header);
    f.render_widget(Paragraph::new(vec![Line::from(bold(format!(" {}", machine.name))), Line::styled(machine.usage_line(), palette::read_only_text())]), home);
    let height = usize::from(prs.height);
    let win = window(open.len(), height, 2, scroll);
    // The list sits under the `▲` row, if any; whole pull requests only.
    let list = Rect::new(prs.x, prs.y + u16::from(win.cue_above), prs.width, prs.height - u16::from(win.cue_above));
    let shown: Vec<(u16, &OpenPr)> = (list.y..).step_by(2).zip(open.iter().skip(win.first).take(win.shown)).collect();
    let mut text = vec![];
    let mut hits = vec![];
    let mut icons = vec![];
    let mut rows = vec![];
    // The `󱆭` is the last column of the section's first line of each PR, the merge icon that of its second.
    let arrow_x = prs.right().checked_sub(1);
    for (y, pr) in &shown {
        text.extend(pr_lines(pr, usize::from(prs.width), frame));
        rows.push(PrRow {
            area: Rect::new(prs.x, *y, prs.width, 2),
            pull: format!("{}#{}", pr.repo, pr.number),
            url: pr.url.clone(),
            clean: pr.clean,
            merging: pr.clean && pr.merging,
            lead: pr.lead.clone(),
        });
        let icon = arrow_x.filter(|_| pr.clean).map(|x| Position::new(x, y + 1));
        // The spinner is no merge icon: a click on it starts no merge.
        if let Some(at) = icon.filter(|_| !pr.merging) {
            icons.push(MergeIcon { area: Rect::new(at.x, at.y, 1, 1), pull: format!("{}#{}", pr.repo, pr.number) });
        }
        if let (Some(lead), Some(x)) = (&pr.lead, arrow_x) {
            hits.push(Hit { area: Rect::new(prs.x, *y, prs.width, 2), arrow: Position::new(x, *y), icon, lead: lead.clone() });
        }
    }
    if open.is_empty() {
        text.push(Line::from(muted(" no open PRs")));
    }
    f.render_widget(Paragraph::new(text), list);
    if win.cue_above {
        let cue = scroll::cue(Cue::Above, win.first, usize::from(prs.width));
        f.render_widget(Paragraph::new(cue), Rect::new(prs.x, prs.y, prs.width, 1));
    }
    if win.below > 0 && prs.height > 0 {
        let cue = scroll::cue(Cue::Below, win.below, usize::from(prs.width));
        f.render_widget(Paragraph::new(cue), Rect::new(prs.x, prs.bottom() - 1, prs.width, 1));
    }
    for (y, pr) in &shown {
        if let Some(x) = arrow_x {
            let buf = f.buffer_mut();
            let block = buf[(x, *y)].style();
            links.draw(Link::new(&pr.url), Rect::new(x, *y, 1, 1), buf);
            // The active block keeps its colours on every cell (rule (3)): the cue's own colour is for the other rows.
            if pr.active {
                buf[(x, *y)].set_style(block);
            }
        }
    }
    Drawn { hits, prs, max_scroll: max_scroll(open.len(), height, 2), icons, rows, links, inbox: inbox_area, inbox_shape: shape }
}


/// The snapshot's `error`, a lasting state of the column (commands.md §17 "No notes", rule (b)): it replaces the last row
/// of `area`, in `palette::error()`, over what [`draw`] drew.
pub fn draw_error(f: &mut Frame, area: Rect, error: &str) {
    let last = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1.min(area.height));
    f.render_widget(Clear, last);
    f.render_widget(Paragraph::new(Span::styled(error.to_owned(), palette::error())), last);
}

/// The layout test's sample (`layout_tests`): the column drawn as the view draws it, with no pull request; the `PRs` title is
/// its rows 3 to 5, which are the sample (the machine's rows above keep their own indent, they are not a section title's).
#[cfg(test)]
pub(crate) fn layout_sample(width: u16, height: u16, case: crate::layout_tests::SectionCase) -> crate::layout_tests::Sample {
    use crate::layout_tests::{Kind, Placed, SectionCase, Sample};
    use ratatui::buffer::Buffer;
    let machine = Machine { name: "homeserver".to_owned(), usage: Usage { cpu: Some(23), gpus: vec![], mem: Some(41) } };
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| drop(draw(f, f.area(), &[], &machine, 0, 0, &[], (false, 0), OffsetDateTime::UNIX_EPOCH, &Default::default()))).unwrap();
    let pane = Rect::new(0, 0, width, 3);
    let mut buf = Buffer::empty(pane);
    for (y, x) in (0..3).flat_map(|y| (0..width).map(move |x| (y, x))) {
        buf[(x, y)] = terminal.backend().buffer()[(x, y + 3)].clone();
    }
    let boxes = match case {
        SectionCase::Title => vec![Placed { name: "PRs title", kind: Kind::Rule, rect: pane, exempt: Vec::new() }],
        SectionCase::Rows => Vec::new(),
    };
    Sample { buf, container: pane, boxes }
}

#[cfg(test)]
#[path = "column_width_tests.rs"]
mod column_width_tests;

#[cfg(test)]
mod tests {
    use crate::read::PrState;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use crate::palette::Meaning;
    use crate::test_text::row;

    use super::*;
    fn drawn(width: u16, height: u16, open: &[OpenPr]) -> (Buffer, Vec<Hit>) {
        let (buf, d) = scrolled(width, height, open, 0);
        (buf, d.hits)
    }

    fn scrolled(width: u16, height: u16, open: &[OpenPr], scroll: usize) -> (Buffer, Drawn) {
        at_frame(width, height, open, scroll, 0)
    }

    fn at_frame(width: u16, height: u16, open: &[OpenPr], scroll: usize, frame: usize) -> (Buffer, Drawn) {
        let machine = Machine { name: "homeserver".to_owned(), usage: Usage { cpu: Some(23), gpus: vec![], mem: Some(41) } };
        // the Inbox section, with no item, is the pane's last rows: the pane is as tall as they and `height` rows are the PRs'
        let height = height + Shape::new(0, false).height();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut out = None;
        terminal.draw(|f| out = Some(draw(f, f.area(), open, &machine, scroll, frame, &[], (false, 0), time::OffsetDateTime::UNIX_EPOCH, &Default::default()))).unwrap();
        let (full, height) = (terminal.backend().buffer().clone(), height - Shape::new(0, false).height());
        let top = Buffer { area: Rect::new(0, 0, width, height), content: full.content[..usize::from(width) * usize::from(height)].to_vec() };
        (top, out.unwrap())
    }

    fn icons(width: u16, open: &[OpenPr]) -> Vec<MergeIcon> {
        scrolled(width, 20, open, 0).1.icons
    }

    fn buffer(width: u16, height: u16) -> Buffer {
        drawn(width, height, &[]).0
    }

    fn pr(number: i64, lead: Option<&str>) -> OpenPr {
        OpenPr {
            repo: "acme/repo".to_owned(),
            number,
            url: format!("https://github.com/acme/repo/pull/{number}"),
            title: "A title".to_owned(),
            draft: false,
            checks: crate::read::Checks::Passed,
            review_decision: ReviewDecision::None,
            conflicting: false,
            update: None,
            clean: false,
            active: false,
            lead: lead.map(|l| SessionId::parse(l).unwrap()),
            jira: None,
            lead_working: false,
            plan_failed: false,
            plan_phase: None,
            merging: false,
        }
    }

    #[test]
    fn the_prs_header_carries_the_count_of_the_open_pull_requests() {
        let header = |width: u16, count| {
            let mut terminal = Terminal::new(TestBackend::new(width, SectionTitle::ROWS)).unwrap();
            terminal.draw(|f| f.render_widget(SectionTitle::new("PRs", count), f.area())).unwrap();
            row(terminal.backend().buffer(), 1)
        };
        assert_eq!(header(20, Some(3)), format!(" ──\u{E0B6}PRs\u{E0B4}{}\u{E0B6}3\u{E0B4}", "─".repeat(8)));
        assert_eq!(header(30, Some(12)), format!(" ──\u{E0B6}PRs\u{E0B4}{}\u{E0B6}12\u{E0B4}", "─".repeat(30 - 8 - 4 - 1)));
        assert_eq!(header(20, None), format!(" ──\u{E0B6}PRs\u{E0B4}{}", "─".repeat(11)));
        let buf = buffer(20, 15);
        assert_eq!(row(&buf, 4), format!(" ──\u{E0B6}PRs\u{E0B4}{}\u{E0B6}0\u{E0B4}", "─".repeat(8)));
        let two = [pr(1, Some("l-a")), pr(2, None)];
        assert_eq!(row(&drawn(20, 15, &two).0, 4), format!(" ──\u{E0B6}PRs\u{E0B4}{}\u{E0B6}2\u{E0B4}", "─".repeat(8)));
    }

    #[test]
    fn the_column_has_a_blank_row_an_untitled_machine_with_cpu_and_memory_and_the_prs_header_only() {
        let buf = buffer(20, 10);
        let got: Vec<String> = (0..10).map(|y| row(&buf, y)).collect();
        let expected = [
            "".to_owned(),
            " homeserver".to_owned(),
            " \u{F4BC} 23%  \u{EFC5} 41%".to_owned(),
            "".to_owned(),
            format!(" ──\u{E0B6}PRs\u{E0B4}{}\u{E0B6}0\u{E0B4}", "─".repeat(8)),
            "".to_owned(),
            " no open PRs".to_owned(),
            "".to_owned(),
            "".to_owned(),
            "".to_owned(),
        ];
        assert_eq!(got, expected, "{got:#?}");
        assert!(buf[(1, 1)].modifier.contains(Modifier::BOLD), "the hostname is bold");
        for x in 0..20 {
            let cell = &buf[(x, 2)];
            assert!(!cell.modifier.contains(Modifier::BOLD) && (cell.symbol() == " " || cell.fg == palette::read_only_text().fg.unwrap()), "read-only text, no bold, on the usage row, x={x}");
        }
        assert!(!got.iter().any(|r| r.contains("git") || r.contains("Jira") || r.contains("future") || r.contains("load")));
    }

    #[test]
    fn a_value_not_known_is_a_dash() {
        let usage = |cpu, mem| Machine { name: "homeserver".to_owned(), usage: Usage { cpu, gpus: vec![], mem } }.usage_line();
        assert_eq!(usage(None, Some(100)), " \u{F4BC} –%  \u{EFC5} 100%", "the CPU until a second sample");
        assert_eq!(usage(Some(7), None), " \u{F4BC} 7%  \u{EFC5} –%", "a memory that cannot be read");
    }

    #[test]
    fn an_unowned_pr_is_read_only_text_on_both_lines_an_owned_one_clickable_and_only_it_has_a_hit_area() {
        let (buf, hits) = drawn(30, 20, &[pr(7, None), pr(8, Some("l-a"))]);
        for (y, owned) in [(6, false), (7, false), (8, true), (9, true)] {
            let want = if owned { palette::clickable_text() } else { palette::read_only_text() }.fg.unwrap();
            assert_eq!(buf[(2, y)].fg, want, "row {y}");
        }
        assert!(buf[(29, 6)].fg == palette::clickable_text().fg.unwrap() && buf[(29, 6)].symbol().contains(link::CUE), "the arrow stays clickable");
        let [hit] = &hits[..] else { panic!("{hits:?}") };
        assert_eq!(hit.lead.as_str(), "l-a");
        assert_eq!((hit.area, hit.arrow, hit.icon), (Rect::new(0, 8, 30, 2), Position::new(29, 8), None));
    }

    #[test]
    fn a_hit_area_is_the_two_rows_except_the_arrow_cell() {
        let (_, hits) = drawn(30, 20, &[pr(7, Some("l-a"))]);
        let hit = &hits[0];
        for (x, y, inside) in [(0, 6, true), (28, 6, true), (29, 6, false), (29, 7, true), (0, 7, true), (0, 8, false), (0, 5, false)] {
            assert_eq!(hit.contains(Position::new(x, y)), inside, "({x}, {y})");
        }
    }

    #[test]
    fn a_snapshot_error_replaces_the_last_row() {
        let machine = Machine { name: "homeserver".to_owned(), usage: Usage { cpu: Some(23), gpus: vec![], mem: Some(41) } };
        let mut terminal = Terminal::new(TestBackend::new(30, 15)).unwrap();
        terminal
            .draw(|f| {
                draw(f, f.area(), &[], &machine, 0, 0, &[], (false, 0), time::OffsetDateTime::UNIX_EPOCH, &Default::default());
                draw_error(f, f.area(), "no_cockpit: nothing here");
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        assert_eq!(row(&buf, 14), "no_cockpit: nothing here");
        assert_eq!(buf[(0, 14)].fg, palette::error().fg.unwrap());
        assert_eq!(row(&buf, 6), " no open PRs", "the rest is as drawn");
    }

    fn second_line(pr: &OpenPr, width: u16) -> (String, Buffer) {
        let (buf, _) = drawn(width, 20, &[pr.clone()]);
        (row(&buf, 7), buf)
    }

    #[test]
    fn an_owned_conflicting_pr_shows_conflict_in_the_waiting_colour_after_its_review_and_others_do_not() {
        let mut p = pr(7, Some("l-a"));
        assert_eq!(second_line(&p, 40).0, format!("  open {} no review  A title", palette::CHECKS_PASSED));
        p.conflicting = true;
        let (line, buf) = second_line(&p, 40);
        assert_eq!(line, format!("  open {} no review conflict  A title", palette::CHECKS_PASSED));
        let x = line.find("conflict").map(|b| line[..b].chars().count() as u16).unwrap();
        for dx in 0..8 {
            assert_eq!(buf[(x + dx, 7)].fg, palette::color_clickable(Meaning::Waiting), "its Lead fixes it: waiting, not red");
        }
        assert_ne!(buf[(x - 2, 7)].fg, palette::color_clickable(Meaning::Waiting), "the review is not waiting");
    }

    #[test]
    fn an_unowned_conflicting_pr_keeps_conflict_red() {
        let mut p = pr(7, None);
        p.conflicting = true;
        let (line, buf) = second_line(&p, 40);
        let x = line.find("conflict").map(|b| line[..b].chars().count() as u16).unwrap();
        for dx in 0..8 {
            assert_eq!(buf[(x + dx, 7)].fg, palette::color(Meaning::Failed), "no Plan owns it: the User's to fix");
        }
    }

    #[test]
    fn failed_checks_are_waiting_on_an_owned_pr_and_red_on_an_unowned_one() {
        for (lead, meaning) in [(Some("l-a"), Meaning::Waiting), (None, Meaning::Failed)] {
            let mut p = pr(7, lead);
            p.checks = crate::read::Checks::Failed;
            let (line, buf) = second_line(&p, 40);
            let x = line.find(palette::CHECKS_FAILED).map(|b| line[..b].chars().count() as u16).unwrap();
            let colour = if lead.is_some() { palette::color_clickable(meaning) } else { palette::color(meaning) };
            assert_eq!(buf[(x, 7)].fg, colour, "{lead:?}");
        }
    }

    #[test]
    fn conflict_is_cyan_on_an_owned_pr_red_on_an_unowned_one_and_no_colour_in_the_active_block() {
        for (active, lead, colour) in
            [(false, Some("l-a"), palette::color_clickable(Meaning::Waiting)), (false, None, palette::color(Meaning::Failed))]
        {
            let mut p = pr(7, lead);
            p.conflicting = true;
            p.active = active;
            let (line, buf) = second_line(&p, 40);
            let x = line.find("conflict").map(|b| line[..b].chars().count() as u16).unwrap();
            assert_eq!(buf[(x, 7)].fg, colour);
        }
        // the active block: background 4, text 0 and bold on every cell of both lines, whatever the mark was
        let mut p = pr(7, Some("l-a"));
        p.conflicting = true;
        p.active = true;
        let (line, buf) = second_line(&p, 40);
        let x = line.find("conflict").map(|b| line[..b].chars().count() as u16).unwrap();
        assert!((0..40).all(|x| palette::check::is_selected(&buf[(x, 7)]) && palette::check::is_selected(&buf[(x, 6)])));
        assert!(buf[(x, 7)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn a_narrow_column_cuts_the_title_before_conflict() {
        let mut p = pr(7, Some("l-a"));
        p.conflicting = true;
        let open = format!("  open {} no review conflict", palette::CHECKS_PASSED);
        assert_eq!(second_line(&p, 28).0, open);
        assert_eq!(second_line(&p, 32).0, open);
        assert_eq!(second_line(&p, 33).0, format!("{open}  A t…"));
    }

    fn second_at(pr: &OpenPr, width: u16, frame: usize) -> (String, Buffer) {
        let (buf, _) = at_frame(width, 20, &[pr.clone()], 0, frame);
        (row(&buf, 7), buf)
    }

    #[test]
    fn an_owned_pr_whose_lead_works_shows_the_spinner_frame_after_its_state_and_it_turns_with_the_frame() {
        let mut p = pr(7, Some("l-a"));
        p.lead_working = true;
        for frame in 0..palette::SPINNER.len() {
            let (line, buf) = second_at(&p, 40, frame);
            assert_eq!(line, format!("  open {} {} no review  A title", palette::SPINNER[frame], palette::CHECKS_PASSED), "frame {frame}");
            assert_eq!(buf[(2, 7)].fg, palette::color_clickable(palette::pr_state(PrState::Open)), "the state keeps its colour");
        }
        // the frame is the only change between two draws
        assert_ne!(second_at(&p, 40, 1).1, second_at(&p, 40, 2).1);
    }

    #[test]
    fn an_idle_lead_or_an_unowned_pr_shows_no_spinner() {
        let owned = pr(7, Some("l-a"));
        let unowned = pr(8, None);
        for p in [&owned, &unowned] {
            for frame in 0..palette::SPINNER.len() {
                let (line, _) = second_at(p, 40, frame);
                assert_eq!(line, format!("  open {} no review  A title", palette::CHECKS_PASSED), "frame {frame}");
            }
        }
    }

    #[test]
    fn the_spinner_keeps_the_conflict_flag_and_the_title_cut_to_the_width() {
        let mut p = pr(7, Some("l-a"));
        p.lead_working = true;
        p.conflicting = true;
        let open = |frame: usize| format!("  open {} {} no review conflict", palette::SPINNER[frame], palette::CHECKS_PASSED);
        assert_eq!(second_at(&p, 30, 2).0, open(2), "the title is cut first");
        assert_eq!(second_at(&p, 35, 2).0, format!("{}  A t…", open(2)));
    }

    #[test]
    fn a_clean_pr_ends_its_second_line_in_the_merge_icon_and_others_have_none() {
        let mut p = pr(7, Some("l-a"));
        assert_eq!(second_line(&p, 40).0, "  open \u{F05E0} no review  A title");
        p.clean = true;
        let (line, _) = second_line(&p, 40);
        assert_eq!(line, format!("  open \u{F05E0} no review  A title{}{MERGE}", " ".repeat(40 - 27 - 1)));
        assert_eq!(line.chars().count(), 40, "the icon is the last column");
        assert!(!row(&drawn(40, 20, &[p]).0, 6).contains(MERGE), "never on the first line");
    }

    #[test]
    fn the_title_is_cut_so_it_never_reaches_the_merge_icon() {
        let mut p = pr(7, Some("l-a"));
        p.clean = true;
        p.title = "A very long title indeed".to_owned();
        let (line, _) = second_line(&p, 32);
        assert_eq!(line, format!("  open \u{F05E0} no review  A very lo… {MERGE}"));
        assert_eq!(line.chars().count(), 32);
    }

    #[test]
    fn a_merge_icon_has_a_one_cell_area_and_the_row_hit_area_excludes_it() {
        let mut clean = pr(7, Some("l-a"));
        clean.clean = true;
        let mut unowned = pr(8, None);
        unowned.clean = true;
        let blocked = pr(9, Some("l-a"));
        let w = [clean, unowned, blocked];
        assert_eq!(
            icons(30, &w),
            [
                MergeIcon { area: Rect::new(29, 7, 1, 1), pull: "acme/repo#7".to_owned() },
                MergeIcon { area: Rect::new(29, 9, 1, 1), pull: "acme/repo#8".to_owned() },
            ],
            "every clean row, owned or not; none for the other"
        );
        let (_, hits) = drawn(30, 20, &w);
        let [first, second] = &hits[..] else { panic!("{hits:?}") };
        assert_eq!(first.icon, Some(Position::new(29, 7)));
        assert_eq!(second.icon, None);
        for (x, y, inside) in [(28, 7, true), (29, 7, false), (29, 6, false), (0, 7, true)] {
            assert_eq!(first.contains(Position::new(x, y)), inside, "({x}, {y})");
        }
    }

    #[test]
    fn a_row_offers_merge_only_when_clean_and_go_to_lead_only_when_owned() {
        let labels = |clean: bool, lead: Option<&str>| {
            let mut p = pr(7, lead);
            p.clean = clean;
            let (_, d) = scrolled(30, 20, &[p], 0);
            let [row] = &d.rows[..] else { panic!("{:?}", d.rows) };
            row.items().into_iter().map(crate::menu::Entry::label).collect::<Vec<_>>()
        };
        assert_eq!(labels(true, Some("l-a")), ["Merge", "Copy Link", "Go to Lead"]);
        assert_eq!(labels(false, Some("l-a")), ["Copy Link", "Go to Lead"], "not clean");
        assert_eq!(labels(true, None), ["Merge", "Copy Link"], "no owner");
        assert_eq!(labels(false, None), ["Copy Link"], "neither");
    }

    #[test]
    fn every_row_on_screen_is_a_right_click_target_with_its_two_lines_its_pull_and_its_url() {
        let (_, d) = scrolled(30, 20, &[pr(7, Some("l-a")), pr(8, None)], 0);
        let got: Vec<_> = d.rows.iter().map(|r| (r.area, r.pull.as_str(), r.url.as_str())).collect();
        assert_eq!(
            got,
            [
                (Rect::new(0, 6, 30, 2), "acme/repo#7", "https://github.com/acme/repo/pull/7"),
                (Rect::new(0, 8, 30, 2), "acme/repo#8", "https://github.com/acme/repo/pull/8"),
            ]
        );
        let (_, d) = scrolled(30, height(7), &many(6), 2);
        assert_eq!(d.rows.iter().map(|r| r.pull.as_str()).collect::<Vec<_>>(), ["acme/repo#3", "acme/repo#4"], "the rows drawn after a scroll");
    }

    /// `count` owned pull requests, numbers 1.., each with its own Lead `l-<n>`.
    fn many(count: i64) -> Vec<OpenPr> {
        (1..=count).map(|n| pr(n, Some(&format!("l-{n}")))).collect()
    }

    /// The PRs section's rows: below the 6 rows of the rest of the column, to the bottom.
    fn section(buf: &Buffer) -> Vec<String> {
        (6..buf.area.height).map(|y| row(buf, y)).collect()
    }

    /// A column whose PRs section is `lines` rows tall (the rest of the column takes 6).
    fn height(lines: u16) -> u16 {
        6 + lines
    }

    fn names(rows: &[String]) -> Vec<&str> {
        rows.iter().filter_map(|r| r.trim().split(' ').next()).filter(|n| n.contains('#')).collect()
    }

    #[test]
    fn no_cue_is_drawn_when_every_pull_request_fits() {
        let (buf, d) = scrolled(30, height(6), &many(3), 0);
        let got = section(&buf);
        assert!(got.iter().all(|r| !r.contains('▲') && !r.contains('▼')), "{got:#?}");
        assert_eq!(names(&got), ["acme/repo#1", "acme/repo#2", "acme/repo#3"]);
        assert_eq!(d.max_scroll, 0);
        // a scroll that has nowhere to go changes nothing
        assert_eq!(scrolled(30, height(6), &many(3), 5).0, buf);
    }

    #[test]
    fn a_cue_below_counts_the_hidden_pull_requests_and_replaces_the_last_row_in_dim() {
        // 5 rows: the `▼` row, so two pull requests of the five that do not fit.
        let (buf, d) = scrolled(30, height(5), &many(5), 0);
        let got = section(&buf);
        assert_eq!(names(&got), ["acme/repo#1", "acme/repo#2"]);
        assert_eq!(got.last().unwrap(), &format!("{:>30}", "▼ 3 more"), "{got:#?}");
        assert!(!got.iter().any(|r| r.contains('▲')));
        let y = 6 + 4;
        assert!((22..30).all(|x| crate::palette::check::is_muted(&buf[(x, y)])), "the cue is dim");
        assert_eq!(d.prs, Rect::new(0, 6, 30, 5));
        assert_eq!(d.max_scroll, 3);
    }

    #[test]
    fn a_cue_above_appears_after_a_scroll_and_each_cue_takes_a_row_of_the_list() {
        let (buf, _) = scrolled(30, height(5), &many(5), 1);
        let got = section(&buf);
        assert_eq!(got[0], format!("{:>30}", "▲ 1 more"), "{got:#?}");
        // 5 rows less both cues leave 3: one pull request, and a spare row
        assert_eq!(names(&got), ["acme/repo#2"]);
        assert_eq!(got.last().unwrap(), &format!("{:>30}", "▼ 3 more"));
        assert!(crate::palette::check::is_muted(&buf[(29, 6)]));
    }

    #[test]
    fn the_offset_is_clamped_so_the_last_pull_request_reaches_the_bottom() {
        // 5 rows, 5 pull requests: at the end, the `▲` row and two pull requests; no `▼`.
        let (buf, d) = scrolled(30, height(5), &many(5), 99);
        let got = section(&buf);
        assert_eq!(d.max_scroll, 3);
        assert_eq!(got[0], format!("{:>30}", "▲ 3 more"), "{got:#?}");
        assert_eq!(names(&got), ["acme/repo#4", "acme/repo#5"]);
        assert!(!got.iter().any(|r| r.contains('▼')), "{got:#?}");
        assert_eq!(scrolled(30, height(5), &many(5), 3).0, buf);
        // a list that shrank under the offset is drawn at the new end
        let (buf, _) = scrolled(30, height(5), &many(3), 3);
        assert_eq!(names(&section(&buf)), ["acme/repo#2", "acme/repo#3"]);
    }

    #[test]
    fn a_hit_area_is_that_of_the_row_drawn_after_a_scroll() {
        let (buf, d) = scrolled(30, height(7), &many(6), 2);
        let got = section(&buf);
        assert_eq!(got[0], format!("{:>30}", "▲ 2 more"), "{got:#?}");
        assert_eq!(got.last().unwrap(), &format!("{:>30}", "▼ 2 more"), "{got:#?}");
        let leads: Vec<(&str, Rect)> = d.hits.iter().map(|h| (h.lead.as_str(), h.area)).collect();
        assert_eq!(leads, [("l-3", Rect::new(0, 7, 30, 2)), ("l-4", Rect::new(0, 9, 30, 2))]);
        for hit in &d.hits {
            let n = hit.lead.as_str().trim_start_matches("l-");
            assert!(row(&buf, hit.area.y).contains(&format!("acme/repo#{n}")), "{hit:?}");
        }
        assert!(!d.hits.iter().any(|h| h.contains(Position::new(0, 6))), "the cue row is no hit");
    }

    // ---- section look v3 (commands.md §13.2 `widgets`, §17; hs-d348, hs-d349): red until the column draws them ----

    /// The pill's caps, U+E0B6 and U+E0B4.
    const CAPS: (&str, &str) = ("\u{E0B6}", "\u{E0B4}");

    fn all_rows(buf: &Buffer) -> Vec<String> {
        (0..buf.area.height).map(|y| row(buf, y)).collect()
    }

    /// The first section title row of `buf`: a non-collapsible title (` ──`, hs-d478), asserted to be there.
    fn first_title(buf: &Buffer) -> (u16, String) {
        let found = titles(buf);
        assert!(!found.is_empty(), "a pill title row: none starts with ` ──`: {:?}", all_rows(buf));
        found.into_iter().next().unwrap()
    }

    /// The rows that are a section title: they start with the padding and the non-collapsible title's `──` (hs-d478).
    fn titles(buf: &Buffer) -> Vec<(u16, String)> {
        all_rows(buf).into_iter().enumerate().filter(|(_, r)| r.starts_with(" ──")).map(|(y, r)| (y as u16, r)).collect()
    }

    #[test]
    fn the_prs_title_is_the_pill_with_the_count_one_empty_row_above_it() {
        let (buf, _) = scrolled(30, height(5), &many(5), 0);
        let (open, close) = CAPS;
        let (y, title) = first_title(&buf);
        assert_eq!(title, format!(" ──{open}PRs{close}{}{open}5{close}", "─".repeat(30 - 1 - 2 - 5 - 3 - 1)));
        assert_eq!(title.chars().count(), 29, "full width but the padding's last column (the row is trimmed)");
        assert_eq!(row(&buf, y - 1), "", "one empty row above the title");
    }

    #[test]
    fn the_prs_pill_text_is_static_and_its_lines_are_muted() {
        let (buf, _) = scrolled(30, height(5), &many(5), 0);
        let muted = palette::color(palette::Meaning::Nothing);
        let (y, _) = first_title(&buf);
        let at = |x: u16| &buf[(x, y)];
        assert_eq!((at(1).symbol(), at(1).fg), ("─", muted), "the first line (a non-collapsible title has no corner, hs-d478)");
        assert_eq!((at(2).symbol(), at(2).fg), ("─", muted), "its line");
        for x in 4..7 {
            assert_eq!((Some(at(x).fg), Some(at(x).bg)), (palette::pill_static().fg, palette::pill_static().bg), "the title text is the static pill's: column {x} {:?}", at(x));
        }
        assert_eq!((at(11).symbol(), at(11).fg), ("─", muted), "the line to the edge");
    }

    #[test]
    fn the_prs_title_is_the_pill_with_its_count_and_there_is_no_other_title() {
        let (buf, _) = scrolled(30, height(5), &many(2), 0);
        let (open, close) = CAPS;
        let got: Vec<String> = titles(&buf).into_iter().map(|(_, t)| t).collect();
        assert_eq!(got, [format!(" ──{open}PRs{close}{}{open}2{close}", "─".repeat(30 - 1 - 2 - 5 - 3 - 1))]);
    }

    /// The cue rows of the section, top to bottom.
    fn cue_rows(buf: &Buffer) -> Vec<String> {
        all_rows(buf).into_iter().filter(|r| r.contains('▲') || r.contains('▼')).collect()
    }

    #[test]
    fn the_cues_are_right_aligned_at_the_end_of_their_rows() {
        let (buf, _) = scrolled(30, height(7), &many(6), 2);
        assert_eq!(cue_rows(&buf), [format!("{:>30}", "▲ 2 more"), format!("{:>30}", "▼ 2 more")]);
        let (buf, _) = scrolled(30, height(5), &many(5), 0);
        assert_eq!(cue_rows(&buf), [format!("{:>30}", "▼ 3 more")]);
        let (buf, _) = scrolled(30, height(5), &many(5), 99);
        assert_eq!(cue_rows(&buf), [format!("{:>30}", "▲ 3 more")]);
    }

    #[test]
    fn a_right_aligned_cue_is_dim_and_leaves_the_left_of_its_row_blank() {
        let (buf, _) = scrolled(30, height(5), &many(5), 0);
        let y = all_rows(&buf).iter().position(|r| r.contains('▼')).expect("a ▼ cue row") as u16;
        assert!((0..22).all(|x| buf[(x, y)].symbol() == " "), "nothing on the left of the cue");
        assert!((22..30).all(|x| crate::palette::check::is_muted(&buf[(x, y)])), "`▼ 3 more` is dim and ends at the last column");
    }

    // ---- the Deploy column's PRs title v4 (hs-d475, hs-d476): red until the column draws a grey pill inside its padding ----

    #[test]
    fn the_prs_title_is_a_grey_pill_inside_the_columns_one_column_of_padding_on_each_side() {
        let buf = buffer(20, 15);
        let y = 4;
        let cells: String = (0..20).map(|x| buf[(x, y)].symbol()).collect();
        assert_eq!(cells, format!(" ──\u{E0B6}PRs\u{E0B4}{}\u{E0B6}0\u{E0B4} ", "─".repeat(8)), "starts at column 1, ends one column before the edge");
        crate::section::check::grey_pill(&buf, 3, y, "PRs");
        crate::section::check::grey_pill(&buf, 16, y, "0"); // the count is a grey pill at the row's end, inside the right padding (hs-d480)
        let (buf, _) = drawn(30, 15, &[pr(1, Some("l-a")), pr(2, None)]);
        let cells: String = (0..30).map(|x| buf[(x, y)].symbol()).collect();
        assert_eq!(cells, format!(" ──\u{E0B6}PRs\u{E0B4}{}\u{E0B6}2\u{E0B4} ", "─".repeat(30 - 1 - 2 - 5 - 3 - 1)));
    }

    #[test]
    fn the_prs_title_has_one_empty_row_above_and_one_below_and_is_no_click_target() {
        let open = [pr(1, Some("l-a")), pr(2, None)];
        let (buf, d) = scrolled(24, 15, &open, 0);
        assert_eq!((row(&buf, 3).as_str(), row(&buf, 5).as_str()), ("", ""), "an empty row above and one below");
        assert!(row(&buf, 4).contains("PRs"), "the title is on row 4: {}", row(&buf, 4));
        assert!(d.hits.iter().all(|h| h.area.y > 4), "no click on the title row: {:?}", d.hits);
        assert!(d.rows.iter().all(|r| r.area.y > 4), "no menu on the title row: {:?}", d.rows);
        assert!(d.icons.iter().all(|i| i.area.y > 4));
    }

    // ---- a merge in progress (hs-d495): the spinner instead of the merge button, and no click ----

    /// A clean PR, owned or not, whose merge is running.
    fn merging(number: i64, lead: Option<&str>) -> OpenPr {
        OpenPr { clean: true, merging: true, ..pr(number, lead) }
    }

    #[test]
    fn a_merging_prs_merge_cell_is_the_spinner_frame_in_place_of_the_icon_and_turns_with_the_frame() {
        let (clean, running) = (OpenPr { clean: true, ..pr(7, Some("l-a")) }, merging(7, Some("l-a")));
        for frame in 0..palette::SPINNER.len() {
            let (line, _) = second_at(&running, 30, frame);
            let (icon_line, _) = second_at(&clean, 30, frame);
            assert!(!line.contains(MERGE), "frame {frame}: no merge icon while it merges: {line:?}");
            assert_eq!(line, icon_line.replace(MERGE, palette::SPINNER[frame]), "frame {frame}: the spinner is the icon's cell, the rest of the row as before");
            assert_eq!(line.chars().count(), 30, "the last column");
        }
        assert_ne!(second_at(&running, 30, 1).1, second_at(&running, 30, 2).1, "the frame is what changes between two draws");
    }

    #[test]
    fn a_click_on_the_spinner_does_nothing_it_is_no_merge_icon_and_not_the_owners_row_either() {
        let (_, d) = at_frame(30, 20, &[merging(7, Some("l-a")), merging(8, None)], 0, 3);
        assert_eq!(d.icons, [], "no merge icon to click: a running merge does not start again");
        let [hit] = &d.hits[..] else { panic!("{:?}", d.hits) };
        assert!(!hit.contains(Position::new(29, 7)), "nor does the click on the spinner open the Lead");
        assert!(hit.contains(Position::new(28, 7)), "the rest of the owned row still does");
    }

    #[test]
    fn only_the_merging_pull_request_shows_the_spinner_the_others_keep_their_icon_and_its_click() {
        let open = [merging(7, Some("l-a")), OpenPr { clean: true, ..pr(8, Some("l-b")) }, OpenPr { clean: true, ..pr(9, None) }];
        let (buf, d) = at_frame(30, 20, &open, 0, 4);
        assert_eq!(buf[(29, 7)].symbol(), palette::SPINNER[4], "#7 merges");
        assert_eq!((buf[(29, 9)].symbol(), buf[(29, 11)].symbol()), (MERGE, MERGE), "#8 and #9 do not");
        assert_eq!(
            d.icons,
            [
                MergeIcon { area: Rect::new(29, 9, 1, 1), pull: "acme/repo#8".to_owned() },
                MergeIcon { area: Rect::new(29, 11, 1, 1), pull: "acme/repo#9".to_owned() },
            ]
        );
    }

    #[test]
    fn a_row_whose_merge_is_running_offers_merge_busy_and_the_others_offer_it_as_before() {
        let items = |p: OpenPr| {
            let (_, d) = scrolled(30, 20, &[p], 0);
            let [row] = &d.rows[..] else { panic!("{:?}", d.rows) };
            (row.merging, row.items())
        };
        assert_eq!(items(merging(7, Some("l-a"))), (true, vec![PrItem::Merging, PrItem::CopyLink, PrItem::GoToLead]));
        assert_eq!(items(merging(7, None)), (true, vec![PrItem::Merging, PrItem::CopyLink]));
        assert_eq!(items(OpenPr { clean: true, ..pr(7, Some("l-a")) }), (false, vec![PrItem::Merge, PrItem::CopyLink, PrItem::GoToLead]));
        assert_eq!(crate::menu::Entry::label(PrItem::Merging), "Merge");
        assert!(crate::menu::Entry::busy(PrItem::Merging), "shown with the spinner, not selectable");
        assert!(![PrItem::Merge, PrItem::CopyLink, PrItem::GoToLead].into_iter().any(crate::menu::Entry::busy));
    }
}
