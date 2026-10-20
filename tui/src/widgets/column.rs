//! The Dispatch column's drawing (commands.md §13.2), a port of the reference's `draw_dispatch` and `pr::render`
//! (`tui/src/main.rs`, `tui/src/pr.rs`): the machine (hostname, CPU % and memory %) and the open pull requests (two lines
//! each), under one blank row; each section but the first starts with a header. Pure: [`draw`] only writes to the frame.

use cpd_core::ids::SessionId;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::host::Usage;
use crate::link;
use crate::palette::{self, Meaning};
use crate::read::{OpenPr, PrState, ReviewDecision};

/// The glyph in the last column of a clean PR's second line: md-source_merge, U+F062D (Material Design Nerd Font).
const MERGE: &str = "\u{F062D}";

/// What a click on an owned PR reaches: its two lines, except the `󰏌` cell (the terminal opens that link).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// The PR's two rows, the column's full width.
    pub area: Rect,
    /// The `󰏌` cell, inside `area`.
    pub arrow: Position,
    /// The merge icon's cell, inside `area`, when the PR shows one.
    pub icon: Option<Position>,
    /// The Lead of the owner: `cpd session open <lead>`.
    pub lead: SessionId,
}

impl Hit {
    pub fn contains(&self, at: Position) -> bool {
        self.area.contains(at) && at != self.arrow && Some(at) != self.icon
    }
}

/// A merge icon on the screen: a click on `area` (one cell) opens the merge popup for `pull`, `<owner>/<repo>#<n>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeIcon {
    pub area: Rect,
    pub pull: String,
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
    Span::styled(text.into(), Style::new().add_modifier(Modifier::BOLD))
}

pub(crate) fn dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().add_modifier(Modifier::DIM))
}

/// `text` cut to `max` characters, ending in `…` when cut.
fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();
    if max > 0 {
        cut.push('…');
    }
    cut
}

/// One open pull request, two lines (commands.md §13.2): ` owner/repo#n` with `󰏌` in the last column, then
/// `  <open|draft> <checks> <review>  <title>`, the title cut first to fit `width`; the name is cut so it never reaches the `󰏌`.
/// A `clean` one ends the second line in the merge icon, in the last column; the title is cut so it never reaches it.
/// A conflicting one adds ` conflict` in red after the review (kept red under the active and dim styles).
/// An `active` one (a PR of the chat slot's Plan) is reversed and bold, as the active tab is; one no Plan owns is dim.
fn pr_lines(pr: &OpenPr, width: usize) -> [Line<'static>; 2] {
    let name = truncate(&format!(" {}#{}", pr.repo, pr.number), width.saturating_sub(2));
    let gap = " ".repeat(width.saturating_sub(name.chars().count() + 1));
    let state_text = if pr.draft { "draft" } else { "open" };
    let review = match pr.review_decision {
        ReviewDecision::Approved => "approved",
        ReviewDecision::ChangesRequested => "changes requested",
        ReviewDecision::None => "no review",
    };
    let (icon, checks) = palette::checks(pr.checks);
    let conflict = " conflict";
    let used = 2 + state_text.chars().count() + 3 + review.chars().count() + if pr.conflicting { conflict.chars().count() } else { 0 };
    let mut spans = vec![
        "  ".into(),
        Span::styled(state_text, palette::fg(palette::pr_state(PrState::Open))),
        " ".into(),
        Span::styled(icon, palette::fg(checks)),
        " ".into(),
        review.into(),
    ];
    if pr.conflicting {
        spans.push(Span::styled(conflict, palette::fg(Meaning::Failed)));
    }
    let reserved = if pr.clean { 2 } else { 0 };
    let room = width.saturating_sub(used + 2 + reserved);
    let mut shown = used;
    if room >= 4 {
        let title = format!("  {}", truncate(&pr.title, room));
        shown += title.chars().count();
        spans.push(if pr.active { Span::raw(title) } else { dim(title) });
    }
    if pr.clean {
        spans.push(" ".repeat(width.saturating_sub(shown + 1)).into());
        spans.push(MERGE.into());
    }
    let first = Line::from(vec![bold(name), gap.into(), link::CUE.into()]);
    let second = Line::from(spans);
    let style = if pr.active {
        Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else if pr.lead.is_none() {
        Style::new().add_modifier(Modifier::DIM)
    } else {
        Style::new()
    };
    [first.patch_style(style), second.patch_style(style)]
}

/// A section header, one line: `──`, then ` <title> ` as a grey reversed block (not bold), then `─` to the right edge, all in
/// the Nothing colour. A title wider than `width` is cut; the line never wraps.
pub(crate) fn section_header(title: &str, width: usize) -> Line<'static> {
    let grey = palette::fg(Meaning::Nothing);
    let lead = width.min(2);
    let block: String = format!(" {title} ").chars().take(width - lead).collect();
    let rest = width - lead - block.chars().count();
    Line::from(vec![
        Span::styled("─".repeat(lead), grey),
        Span::styled(block, grey.add_modifier(Modifier::REVERSED)),
        Span::styled("─".repeat(rest), grey),
    ])
}

fn render_header(f: &mut Frame, area: Rect, title: &str) {
    f.render_widget(Paragraph::new(section_header(title, usize::from(area.width))), area);
}

/// Which items a scrolling list draws, and the cues around them (commands.md §13.2, "Scrolling the PRs section"; the
/// tree's Inbox section scrolls with the same code, one row an item).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Window {
    /// The first item drawn, after clamping.
    pub first: usize,
    /// How many are drawn.
    pub shown: usize,
    /// A ` ▲ <first> more` row opens the list.
    pub cue_above: bool,
    /// How many are hidden below: a ` ▼ <n> more` row ends the list when not 0.
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
}

/// Draws the column in `area`: a blank row, the machine, then the PRs under their header. The PRs section
/// starts at the pull request `scroll` (clamped), with a dim `▲`/`▼` cue row where some are hidden. A non-empty `note`
/// replaces the column's last row, in `palette::note()`. Returns where a click reaches an owned PR or a merge icon.
pub fn draw(f: &mut Frame, area: Rect, open: &[OpenPr], machine: &Machine, note: &str, scroll: usize) -> Drawn {
    let [_top, home, _above_prs, prs_header, _below_prs, prs] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(area);
    render_header(f, prs_header, "PRs");
    f.render_widget(Paragraph::new(vec![Line::from(bold(format!(" {}", machine.name))), Line::from(machine.usage_line())]), home);
    let height = usize::from(prs.height);
    let win = window(open.len(), height, 2, scroll);
    // The list sits under the `▲` row, if any; whole pull requests only.
    let list = Rect::new(prs.x, prs.y + u16::from(win.cue_above), prs.width, prs.height - u16::from(win.cue_above));
    let shown: Vec<(u16, &OpenPr)> = (list.y..).step_by(2).zip(open.iter().skip(win.first).take(win.shown)).collect();
    let mut text = vec![];
    let mut hits = vec![];
    let mut icons = vec![];
    // The `󰏌` is the last column of the section's first line of each PR, the merge icon that of its second.
    let arrow_x = prs.right().checked_sub(1);
    for (y, pr) in &shown {
        text.extend(pr_lines(pr, usize::from(prs.width)));
        let icon = arrow_x.filter(|_| pr.clean).map(|x| Position::new(x, y + 1));
        if let Some(at) = icon {
            icons.push(MergeIcon { area: Rect::new(at.x, at.y, 1, 1), pull: format!("{}#{}", pr.repo, pr.number) });
        }
        if let (Some(lead), Some(x)) = (&pr.lead, arrow_x) {
            hits.push(Hit { area: Rect::new(prs.x, *y, prs.width, 2), arrow: Position::new(x, *y), icon, lead: lead.clone() });
        }
    }
    if open.is_empty() {
        text.push(Line::from(dim(" no open PRs")));
    }
    f.render_widget(Paragraph::new(text), list);
    if win.cue_above {
        f.render_widget(Paragraph::new(dim(format!(" ▲ {} more", win.first))), Rect::new(prs.x, prs.y, prs.width, 1));
    }
    if win.below > 0 && prs.height > 0 {
        f.render_widget(Paragraph::new(dim(format!(" ▼ {} more", win.below))), Rect::new(prs.x, prs.bottom() - 1, prs.width, 1));
    }
    for (y, pr) in &shown {
        if let Some(x) = arrow_x {
            link::link(f.buffer_mut(), Rect::new(x, *y, 1, 1), &pr.url);
        }
    }
    if !note.is_empty() {
        let last = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1.min(area.height));
        f.render_widget(Clear, last);
        f.render_widget(Paragraph::new(Span::styled(note.to_owned(), palette::note())), last);
    }
    Drawn { hits, prs, max_scroll: max_scroll(open.len(), height, 2), icons }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use super::*;
    fn drawn(width: u16, height: u16, open: &[OpenPr], note: &str) -> (Buffer, Vec<Hit>) {
        let (buf, d) = scrolled(width, height, open, note, 0);
        (buf, d.hits)
    }

    fn scrolled(width: u16, height: u16, open: &[OpenPr], note: &str, scroll: usize) -> (Buffer, Drawn) {
        let machine = Machine { name: "homeserver".to_owned(), usage: Usage { cpu: Some(23), gpus: vec![], mem: Some(41) } };
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut out = None;
        terminal.draw(|f| out = Some(draw(f, f.area(), open, &machine, note, scroll))).unwrap();
        (terminal.backend().buffer().clone(), out.unwrap())
    }

    fn icons(width: u16, open: &[OpenPr]) -> Vec<MergeIcon> {
        scrolled(width, 20, open, "", 0).1.icons
    }

    fn buffer(width: u16, height: u16) -> Buffer {
        drawn(width, height, &[], "").0
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
            clean: false,
            active: false,
            lead: lead.map(|l| SessionId::parse(l).unwrap()),
        }
    }

    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_owned()
    }

    #[test]
    fn a_header_is_two_rules_the_title_block_and_rules_to_the_edge() {
        let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
        terminal.draw(|f| render_header(f, f.area(), "PRs")).unwrap();
        let buf = terminal.backend().buffer().clone();
        assert_eq!(row(&buf, 0), format!("── PRs {}", "─".repeat(13)));
        for x in 0..20 {
            let cell = &buf[(x, 0)];
            let title = (2..7).contains(&x);
            assert_eq!(cell.fg, palette::color(Meaning::Nothing), "x={x}");
            assert_eq!(cell.modifier.contains(Modifier::REVERSED), title, "x={x}");
            assert!(!cell.modifier.contains(Modifier::BOLD), "x={x}");
        }
    }

    #[test]
    fn a_header_narrower_than_its_title_cuts_the_title() {
        let text = |width| section_header("Plan", width).spans.iter().map(|s| s.content.to_string()).collect::<String>();
        assert_eq!(text(5), "── Pl");
        assert_eq!(text(1), "─");
        assert_eq!(text(0), "");
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
            format!("── PRs {}", "─".repeat(13)),
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
            assert!(!cell.modifier.contains(Modifier::BOLD | Modifier::DIM) && cell.fg == buf[(0, 0)].fg, "no style on the usage row, x={x}");
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
    fn the_gpu_part_has_one_percent_per_gpu_and_is_absent_with_none() {
        let row = |gpus: Vec<Option<u8>>| Machine { name: "homeserver".to_owned(), usage: Usage { cpu: Some(23), gpus, mem: Some(41) } }.usage_line();
        assert_eq!(row(vec![]), " \u{F4BC} 23%  \u{EFC5} 41%", "no GPU, no GPU part");
        assert_eq!(row(vec![Some(61)]), " \u{F4BC} 23%  \u{E266} 61%  \u{EFC5} 41%");
        assert_eq!(row(vec![Some(61), Some(0)]), " \u{F4BC} 23%  \u{E266} 61% 0%  \u{EFC5} 41%");
        assert_eq!(row(vec![None, Some(5)]), " \u{F4BC} 23%  \u{E266} –% 5%  \u{EFC5} 41%", "an unknown GPU is –%");
    }

    #[test]
    fn a_pr_ends_its_first_line_in_a_linked_arrow_and_the_name_never_reaches_it() {
        let (buf, _) = drawn(24, 20, &[pr(7, Some("l-a"))], "");
        // the arrow's cell carries the link: its symbol is wrapped in the OSC 8 escapes
        let link = format!("\x1b]8;;https://github.com/acme/repo/pull/7\x07{}\x1b]8;;\x07", link::CUE);
        assert_eq!(row(&buf, 6), format!(" acme/repo#7{}{link}", " ".repeat(11)));
        // the name is cut before the arrow's column and its gap
        let (buf, _) = drawn(9, 20, &[pr(7, Some("l-a"))], "");
        assert_eq!(row(&buf, 6).replace(&link, link::CUE), format!(" acme/… {}", link::CUE));
    }

    #[test]
    fn an_unowned_pr_is_dim_on_both_lines_and_has_no_hit_area() {
        let (buf, hits) = drawn(30, 20, &[pr(7, None), pr(8, Some("l-a"))], "");
        for (y, dim) in [(6, true), (7, true), (8, false), (9, false)] {
            assert_eq!(buf[(1, y)].modifier.contains(Modifier::DIM), dim, "row {y}");
            assert_eq!(buf[(2, y)].modifier.contains(Modifier::DIM), dim, "row {y}");
        }
        assert!(buf[(29, 6)].modifier.contains(Modifier::DIM) && buf[(29, 6)].symbol().contains(link::CUE), "the arrow stays");
        let [hit] = &hits[..] else { panic!("{hits:?}") };
        assert_eq!(hit.lead.as_str(), "l-a");
        assert_eq!((hit.area, hit.arrow, hit.icon), (Rect::new(0, 8, 30, 2), Position::new(29, 8), None));
    }

    #[test]
    fn a_hit_area_is_the_two_rows_except_the_arrow_cell() {
        let (_, hits) = drawn(30, 20, &[pr(7, Some("l-a"))], "");
        let hit = &hits[0];
        for (x, y, inside) in [(0, 6, true), (28, 6, true), (29, 6, false), (29, 7, true), (0, 7, true), (0, 8, false), (0, 5, false)] {
            assert_eq!(hit.contains(Position::new(x, y)), inside, "({x}, {y})");
        }
    }

    #[test]
    fn a_note_replaces_the_last_row_in_the_note_style() {
        let (buf, _) = drawn(30, 15, &[], "no_cockpit: nothing here");
        assert_eq!(row(&buf, 14), "no_cockpit: nothing here");
        assert_eq!(buf[(0, 14)].fg, palette::note().fg.unwrap());
        assert_eq!(row(&buf, 6), " no open PRs", "the rest is as drawn");
    }

    fn second_line(pr: &OpenPr, width: u16) -> (String, Buffer) {
        let (buf, _) = drawn(width, 20, &[pr.clone()], "");
        (row(&buf, 7), buf)
    }

    #[test]
    fn a_conflicting_pr_shows_conflict_in_red_after_its_review_and_others_do_not() {
        let mut p = pr(7, Some("l-a"));
        assert_eq!(second_line(&p, 40).0, format!("  open {} no review  A title", palette::CHECKS_PASSED));
        p.conflicting = true;
        let (line, buf) = second_line(&p, 40);
        assert_eq!(line, format!("  open {} no review conflict  A title", palette::CHECKS_PASSED));
        let x = line.find("conflict").map(|b| line[..b].chars().count() as u16).unwrap();
        for dx in 0..8 {
            assert_eq!(buf[(x + dx, 7)].fg, palette::color(Meaning::Failed));
        }
        assert_ne!(buf[(x - 2, 7)].fg, palette::color(Meaning::Failed), "the review is not red");
    }

    #[test]
    fn conflict_keeps_its_red_under_the_active_and_dim_styles() {
        for (active, lead, modifier) in [(true, Some("l-a"), Modifier::REVERSED | Modifier::BOLD), (false, None, Modifier::DIM)] {
            let mut p = pr(7, lead);
            p.conflicting = true;
            p.active = active;
            let (line, buf) = second_line(&p, 40);
            let x = line.find("conflict").map(|b| line[..b].chars().count() as u16).unwrap();
            assert_eq!(buf[(x, 7)].fg, palette::color(Meaning::Failed));
            assert!(buf[(x, 7)].modifier.contains(modifier));
        }
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

    #[test]
    fn a_clean_pr_ends_its_second_line_in_the_merge_icon_and_others_have_none() {
        let mut p = pr(7, Some("l-a"));
        assert_eq!(second_line(&p, 40).0, "  open \u{F05E0} no review  A title");
        p.clean = true;
        let (line, _) = second_line(&p, 40);
        assert_eq!(line, format!("  open \u{F05E0} no review  A title{}{MERGE}", " ".repeat(40 - 27 - 1)));
        assert_eq!(line.chars().count(), 40, "the icon is the last column");
        assert!(!row(&drawn(40, 20, &[p], "").0, 6).contains(MERGE), "never on the first line");
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
        let (_, hits) = drawn(30, 20, &w, "");
        let [first, second] = &hits[..] else { panic!("{hits:?}") };
        assert_eq!(first.icon, Some(Position::new(29, 7)));
        assert_eq!(second.icon, None);
        for (x, y, inside) in [(28, 7, true), (29, 7, false), (29, 6, false), (0, 7, true)] {
            assert_eq!(first.contains(Position::new(x, y)), inside, "({x}, {y})");
        }
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
        let (buf, d) = scrolled(30, height(6), &many(3), "", 0);
        let got = section(&buf);
        assert!(got.iter().all(|r| !r.contains('▲') && !r.contains('▼')), "{got:#?}");
        assert_eq!(names(&got), ["acme/repo#1", "acme/repo#2", "acme/repo#3"]);
        assert_eq!(d.max_scroll, 0);
        // a scroll that has nowhere to go changes nothing
        assert_eq!(scrolled(30, height(6), &many(3), "", 5).0, buf);
    }

    #[test]
    fn a_cue_below_counts_the_hidden_pull_requests_and_replaces_the_last_row_in_dim() {
        // 5 rows: the `▼` row, so two pull requests of the five that do not fit.
        let (buf, d) = scrolled(30, height(5), &many(5), "", 0);
        let got = section(&buf);
        assert_eq!(names(&got), ["acme/repo#1", "acme/repo#2"]);
        assert_eq!(got.last().unwrap(), " ▼ 3 more", "{got:#?}");
        assert!(!got.iter().any(|r| r.contains('▲')));
        let y = 6 + 4;
        assert!((0..9).all(|x| buf[(x, y)].modifier.contains(Modifier::DIM)), "the cue is dim");
        assert_eq!(d.prs, Rect::new(0, 6, 30, 5));
        assert_eq!(d.max_scroll, 3);
    }

    #[test]
    fn a_cue_above_appears_after_a_scroll_and_each_cue_takes_a_row_of_the_list() {
        let (buf, _) = scrolled(30, height(5), &many(5), "", 1);
        let got = section(&buf);
        assert_eq!(got[0], " ▲ 1 more", "{got:#?}");
        // 5 rows less both cues leave 3: one pull request, and a spare row
        assert_eq!(names(&got), ["acme/repo#2"]);
        assert_eq!(got.last().unwrap(), " ▼ 3 more");
        assert!(buf[(1, 6)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn the_offset_is_clamped_so_the_last_pull_request_reaches_the_bottom() {
        // 5 rows, 5 pull requests: at the end, the `▲` row and two pull requests; no `▼`.
        let (buf, d) = scrolled(30, height(5), &many(5), "", 99);
        let got = section(&buf);
        assert_eq!(d.max_scroll, 3);
        assert_eq!(got[0], " ▲ 3 more", "{got:#?}");
        assert_eq!(names(&got), ["acme/repo#4", "acme/repo#5"]);
        assert!(!got.iter().any(|r| r.contains('▼')), "{got:#?}");
        assert_eq!(scrolled(30, height(5), &many(5), "", 3).0, buf);
        // a list that shrank under the offset is drawn at the new end
        let (buf, _) = scrolled(30, height(5), &many(3), "", 3);
        assert_eq!(names(&section(&buf)), ["acme/repo#2", "acme/repo#3"]);
    }

    #[test]
    fn a_hit_area_is_that_of_the_row_drawn_after_a_scroll() {
        let (buf, d) = scrolled(30, height(7), &many(6), "", 2);
        let got = section(&buf);
        assert_eq!(got[0], " ▲ 2 more", "{got:#?}");
        assert_eq!(got.last().unwrap(), " ▼ 2 more", "{got:#?}");
        let leads: Vec<(&str, Rect)> = d.hits.iter().map(|h| (h.lead.as_str(), h.area)).collect();
        assert_eq!(leads, [("l-3", Rect::new(0, 7, 30, 2)), ("l-4", Rect::new(0, 9, 30, 2))]);
        for hit in &d.hits {
            let n = hit.lead.as_str().trim_start_matches("l-");
            assert!(row(&buf, hit.area.y).contains(&format!("acme/repo#{n}")), "{hit:?}");
        }
        assert!(!d.hits.iter().any(|h| h.contains(Position::new(0, 6))), "the cue row is no hit");
    }
}
