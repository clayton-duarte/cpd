//! The Dispatch column's drawing (commands.md §13.2), a port of the reference's `draw_dispatch` and `pr::render`
//! (`tui/src/main.rs`, `tui/src/pr.rs`): the machine, the open pull requests (two lines each), git and Jira (the last two
//! still future), under one blank row; each section but the first starts with a header. Pure: [`draw`] only writes to the frame.

use cpd_core::ids::SessionId;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use crate::link;
use crate::palette::{self, Meaning};
use crate::read::{OpenPr, PrState, ReviewDecision, Widgets};

/// The glyph at the right end of a PR's first line: a link to the pull request.
const ARROW: &str = "↗";

/// What a click on an owned PR reaches: its two lines, except the `↗` cell (the terminal opens that link).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// The PR's two rows, the column's full width.
    pub area: Rect,
    /// The `↗` cell, inside `area`.
    pub arrow: Position,
    /// The Lead of the owner: `cpd session open <lead>`.
    pub lead: SessionId,
}

impl Hit {
    pub fn contains(&self, at: Position) -> bool {
        self.area.contains(at) && at != self.arrow
    }
}

/// The machine the column names: its hostname and the first three fields of its load average.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Machine {
    pub name: String,
    pub load: Vec<String>,
}

fn bold(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().add_modifier(Modifier::BOLD))
}

fn dim(text: impl Into<String>) -> Span<'static> {
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

/// One open pull request, two lines (commands.md §13.2): ` owner/repo#n` with `↗` in the last column, then
/// `  <open|draft> <checks> <review>  <title>`, the title cut first to fit `width`; the name is cut so it never reaches the `↗`.
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
    let room = width.saturating_sub(used + 2);
    if room >= 4 {
        let title = format!("  {}", truncate(&pr.title, room));
        spans.push(if pr.active { Span::raw(title) } else { dim(title) });
    }
    let first = Line::from(vec![bold(name), gap.into(), ARROW.into()]);
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

/// Which pull requests the PRs section draws, and the cues around them (commands.md §13.2, "Scrolling the PRs section").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Window {
    /// The first pull request drawn, after clamping.
    first: usize,
    /// How many are drawn.
    shown: usize,
    /// A ` ▲ <first> more` row opens the section.
    cue_above: bool,
    /// How many are hidden below: a ` ▼ <n> more` row ends the section when not 0.
    below: usize,
}

/// The largest offset for `count` pull requests in a section of `height` rows: the last one reaches the bottom (under the
/// `▲` cue, which takes a row). 0 when every one fits.
fn max_scroll(count: usize, height: usize) -> usize {
    if count * 2 <= height {
        return 0;
    }
    count.saturating_sub((height.saturating_sub(1)) / 2).min(count.saturating_sub(1))
}

/// The window at offset `scroll` (clamped to [`max_scroll`]). A cue takes one row of the section, and a pull request is whole.
fn window(count: usize, height: usize, scroll: usize) -> Window {
    let first = scroll.min(max_scroll(count, height));
    let cue_above = first > 0 && height > 0;
    let room = height - usize::from(cue_above);
    let left = count - first;
    if left * 2 <= room {
        return Window { first, shown: left, cue_above, below: 0 };
    }
    let shown = room.saturating_sub(1) / 2;
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
}

/// Draws the column in `area`: a blank row, the machine, then the PRs, git and Jira, each under its header. The PRs section
/// starts at the pull request `scroll` (clamped), with a dim `▲`/`▼` cue row where some are hidden. A non-empty `note`
/// replaces the column's last row, in `palette::note()`.
pub fn draw(f: &mut Frame, area: Rect, w: &Widgets, machine: &Machine, note: &str, scroll: usize) -> Drawn {
    let [_top, home, _above_prs, prs_header, _below_prs, prs, _above_git, git_header, _below_git, git, _above_jira, jira_header, _below_jira, jira] =
        Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
    .areas(area);
    render_header(f, prs_header, "PRs");
    render_header(f, git_header, "git");
    render_header(f, jira_header, "Jira");
    f.render_widget(
        Paragraph::new(vec![Line::from(bold(format!(" {}", machine.name))), Line::from(format!(" load {}", machine.load.join(" ")))]),
        home,
    );
    let height = usize::from(prs.height);
    let win = window(w.prs.len(), height, scroll);
    // The list sits under the `▲` row, if any; whole pull requests only.
    let list = Rect::new(prs.x, prs.y + u16::from(win.cue_above), prs.width, prs.height - u16::from(win.cue_above));
    let shown: Vec<(u16, &OpenPr)> = (list.y..).step_by(2).zip(w.prs.iter().skip(win.first).take(win.shown)).collect();
    let mut text = vec![];
    let mut hits = vec![];
    // The `↗` is the last column of the section's first line of each PR.
    let arrow_x = prs.right().checked_sub(1);
    for (y, pr) in &shown {
        text.extend(pr_lines(pr, usize::from(prs.width)));
        if let (Some(lead), Some(x)) = (&pr.lead, arrow_x) {
            hits.push(Hit { area: Rect::new(prs.x, *y, prs.width, 2), arrow: Position::new(x, *y), lead: lead.clone() });
        }
    }
    if w.prs.is_empty() {
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
    f.render_widget(Paragraph::new(dim(" (future)")), git);
    f.render_widget(Paragraph::new(dim(" (future)")), jira);
    if !note.is_empty() {
        let last = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1.min(area.height));
        f.render_widget(Clear, last);
        f.render_widget(Paragraph::new(Span::styled(note.to_owned(), palette::note())), last);
    }
    Drawn { hits, prs, max_scroll: max_scroll(w.prs.len(), height) }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use super::*;
    use crate::read::{Engine, Slots};
    use cpd_core::ids::SessionId;

    fn widgets(prs: Vec<OpenPr>) -> Widgets {
        Widgets {
            slots: Slots { limit: 3, held: vec![] },
            engine: Engine::Never,
            plans: vec![],
            prs,
            escalations: vec![],
            events: vec![],
            events_cursor: 0,
        }
    }

    fn drawn(width: u16, height: u16, w: &Widgets, note: &str) -> (Buffer, Vec<Hit>) {
        let (buf, d) = scrolled(width, height, w, note, 0);
        (buf, d.hits)
    }

    fn scrolled(width: u16, height: u16, w: &Widgets, note: &str, scroll: usize) -> (Buffer, Drawn) {
        let machine = Machine { name: "homeserver".to_owned(), load: ["0.52", "0.41", "0.30"].map(str::to_owned).to_vec() };
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut out = None;
        terminal.draw(|f| out = Some(draw(f, f.area(), w, &machine, note, scroll))).unwrap();
        (terminal.backend().buffer().clone(), out.unwrap())
    }

    fn buffer(width: u16, height: u16) -> Buffer {
        drawn(width, height, &widgets(vec![]), "").0
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
        let text = |width| section_header("Jira", width).spans.iter().map(|s| s.content.to_string()).collect::<String>();
        assert_eq!(text(5), "── Ji");
        assert_eq!(text(1), "─");
        assert_eq!(text(0), "");
    }

    #[test]
    fn the_column_has_a_blank_row_an_untitled_machine_and_three_headers() {
        let buf = buffer(20, 15);
        let rule = |title: &str| format!("── {title} {}", "─".repeat(20 - 4 - title.chars().count()));
        let got: Vec<String> = (0..15).map(|y| row(&buf, y)).collect();
        let expected = [
            "".to_owned(),
            " homeserver".to_owned(),
            " load 0.52 0.41 0.30".to_owned(),
            "".to_owned(),
            rule("PRs"),
            "".to_owned(),
            " no open PRs".to_owned(),
            "".to_owned(),
            rule("git"),
            "".to_owned(),
            " (future)".to_owned(),
            "".to_owned(),
            rule("Jira"),
            "".to_owned(),
            " (future)".to_owned(),
        ];
        assert_eq!(got, expected, "{got:#?}");
        assert!(buf[(1, 1)].modifier.contains(Modifier::BOLD), "the hostname is bold");
        assert!(buf[(1, 10)].modifier.contains(Modifier::DIM), "(future) is dim");
    }

    #[test]
    fn a_pr_ends_its_first_line_in_a_linked_arrow_and_the_name_never_reaches_it() {
        let (buf, _) = drawn(24, 20, &widgets(vec![pr(7, Some("l-a"))]), "");
        // the arrow's cell carries the link: its symbol is wrapped in the OSC 8 escapes
        let link = "\x1b]8;;https://github.com/acme/repo/pull/7\x07↗\x1b]8;;\x07";
        assert_eq!(row(&buf, 6), format!(" acme/repo#7{}{link}", " ".repeat(11)));
        // the name is cut before the arrow's column and its gap
        let (buf, _) = drawn(9, 20, &widgets(vec![pr(7, Some("l-a"))]), "");
        assert_eq!(row(&buf, 6).replace(link, "↗"), " acme/… ↗");
    }

    #[test]
    fn an_unowned_pr_is_dim_on_both_lines_and_has_no_hit_area() {
        let (buf, hits) = drawn(30, 20, &widgets(vec![pr(7, None), pr(8, Some("l-a"))]), "");
        for (y, dim) in [(6, true), (7, true), (8, false), (9, false)] {
            assert_eq!(buf[(1, y)].modifier.contains(Modifier::DIM), dim, "row {y}");
            assert_eq!(buf[(2, y)].modifier.contains(Modifier::DIM), dim, "row {y}");
        }
        assert!(buf[(29, 6)].modifier.contains(Modifier::DIM) && buf[(29, 6)].symbol().contains('↗'), "the arrow stays");
        let [hit] = &hits[..] else { panic!("{hits:?}") };
        assert_eq!(hit.lead.as_str(), "l-a");
        assert_eq!((hit.area, hit.arrow), (Rect::new(0, 8, 30, 2), Position::new(29, 8)));
    }

    #[test]
    fn a_hit_area_is_the_two_rows_except_the_arrow_cell() {
        let (_, hits) = drawn(30, 20, &widgets(vec![pr(7, Some("l-a"))]), "");
        let hit = &hits[0];
        for (x, y, inside) in [(0, 6, true), (28, 6, true), (29, 6, false), (29, 7, true), (0, 7, true), (0, 8, false), (0, 5, false)] {
            assert_eq!(hit.contains(Position::new(x, y)), inside, "({x}, {y})");
        }
    }

    #[test]
    fn a_note_replaces_the_last_row_in_the_note_style() {
        let (buf, _) = drawn(30, 15, &widgets(vec![]), "no_cockpit: nothing here");
        assert_eq!(row(&buf, 14), "no_cockpit: nothing here");
        assert_eq!(buf[(0, 14)].fg, palette::note().fg.unwrap());
        assert_eq!(row(&buf, 10), " (future)", "the rest is as drawn");
    }

    fn second_line(pr: &OpenPr, width: u16) -> (String, Buffer) {
        let (buf, _) = drawn(width, 20, &widgets(vec![pr.clone()]), "");
        (row(&buf, 7), buf)
    }

    #[test]
    fn a_conflicting_pr_shows_conflict_in_red_after_its_review_and_others_do_not() {
        let mut p = pr(7, Some("l-a"));
        assert_eq!(second_line(&p, 40).0, "  open ✓ no review  A title");
        p.conflicting = true;
        let (line, buf) = second_line(&p, 40);
        assert_eq!(line, "  open ✓ no review conflict  A title");
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
        assert_eq!(second_line(&p, 28).0, "  open ✓ no review conflict");
        assert_eq!(second_line(&p, 32).0, "  open ✓ no review conflict");
        assert_eq!(second_line(&p, 33).0, "  open ✓ no review conflict  A t…");
    }

    /// `count` owned pull requests, numbers 1.., each with its own Lead `l-<n>`.
    fn many(count: i64) -> Widgets {
        widgets((1..=count).map(|n| pr(n, Some(&format!("l-{n}")))).collect())
    }

    /// The PRs section's rows: 14 rows of the rest of the column, and `lines` for the section.
    fn section(buf: &Buffer) -> Vec<String> {
        (6..buf.area.height - 8).map(|y| row(buf, y)).collect()
    }

    /// A column whose PRs section is `lines` rows tall (the rest of the column takes 14).
    fn height(lines: u16) -> u16 {
        14 + lines
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
