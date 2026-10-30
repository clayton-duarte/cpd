//! The legend: what every colour and icon of the cockpit means, ported from the
//! spike's `legend.rs`. `cpd tui view legend` draws it; the widgets view's `?`
//! opens it in a tmux popup, since the right column is narrower than the
//! legend (commands.md §13.2). Everything it shows, icons and colours, comes
//! from the palette and from the read model's own ordering of the counters, so
//! it cannot disagree with them. Each section is a ratatui `Table` (aligned
//! columns); the sections are stacked with a `Layout`. The title is a section header (the dialog kit's style); a legend
//! taller than its popup scrolls with the kit's scrolling (↑↓, j/k, PgUp/PgDn, the wheel), and any other key closes it.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, KeyEventKind, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, Widget};

use cpd_core::error::CpdError;
use cpd_core::inbox::model::Urgency;
use cpd_core::output::Output;
use cpd_core::session::PROGRAM;
use crate::buffer::copy_cells;
use crate::cells;
use crate::dialog::scroll::Scroll;
use crate::palette::{self, Meaning};
use crate::pill::pill;
use crate::popup;
use crate::read::{Dot, Work, JobCounts, JobStatus, PrState};
use crate::box_widget::SectionTitle;
use crate::view_ports::Ctx;
use crate::{Capture, run_view_on};
use cpd_core::ports::Ports;
use ratatui::backend::Backend;

const COLUMN_SPACING: u16 = 2;
/// A tmux popup draws a border of one cell on each side.
const POPUP_BORDER: u16 = 2;
const DOTS_TITLE: &str = "Session Dots";
/// The rows above the body: the title, drawn as a section header (hs-d124, hs-d297).
/// The title's rows: an empty row above it; the body's first row is the empty row below (hs-d308).
const HEADER_ROWS: u16 = 2;
const INTRO: &str = "Legend: What The Cockpit's Colours And Icons Mean (Any Key Closes)";
/// The popup's command: the legend view, run like every pane of the cockpit (by `PATH`).
const COMMAND: [&str; 4] = [PROGRAM, "tui", "view", "legend"];

pub(crate) struct Section {
    pub(crate) title: &'static str,
    pub(crate) header: Option<[&'static str; 3]>,
    pub(crate) rows: Vec<Vec<Line<'static>>>,
}

impl Section {
    /// Width of each column: the widest cell, the header included.
    fn column_widths(&self) -> Vec<u16> {
        let columns = self.rows.iter().map(Vec::len).max().unwrap_or(0);
        (0..columns)
            .map(|c| {
                let cells = self.rows.iter().filter_map(|r| r.get(c)).map(Line::width);
                let head = self.header.map_or(0, |h| h.get(c).map_or(0, |t| cells::width(t)));
                cells.chain(std::iter::once(head)).max().unwrap_or(0) as u16
            })
            .collect()
    }

    /// The table's width, or the title's when that is wider.
    fn width(&self) -> u16 {
        let w = self.column_widths();
        (w.iter().sum::<u16>() + COLUMN_SPACING * (w.len() as u16).saturating_sub(1))
            .max(cells::width(self.title) as u16)
    }

    /// The title row, the header row if any, and the rows.
    fn height(&self) -> u16 {
        1 + u16::from(self.header.is_some()) + self.rows.len() as u16
    }

    fn draw(&self, buf: &mut Buffer, area: Rect) {
        let [title, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        Paragraph::new(Span::styled(self.title, palette::read_only_text().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)))
            .render(title, buf);
        let mut table = Table::new(
            self.rows.iter().map(|r| Row::new(r.iter().cloned().map(Cell::from))),
            self.column_widths().into_iter().map(Constraint::Length),
        )
        .column_spacing(COLUMN_SPACING);
        if let Some(h) = self.header {
            table = table
                .header(Row::new(h.iter().map(|t| Cell::from(*t))).style(palette::muted_text()));
        }
        table.render(body, buf);
    }
}

/// Every other word of the legend is read-only text, colour 7.
fn plain(text: &str) -> Line<'static> {
    Line::styled(text.to_string(), palette::read_only_text())
}

/// The Context row's precedence line, with the real glyphs (the working one as its first frame).
fn precedence() -> String {
    format!(
        "Context row: its most urgent Session: red > yellow > magenta > cyan > white > {} > {} > {} > {}",
        palette::SPINNER[0],
        palette::WAITING,
        palette::IDLE,
        palette::STOPPED
    )
}

/// The sections, built only from the palette and the read model's counters.
pub(crate) fn sections() -> Vec<Section> {
    let meanings = Section {
        title: "Meanings → Colour (One Source For Every View)",
        header: None,
        rows: Meaning::ALL
            .iter()
            .map(|m| vec![Line::from(Span::styled("██", palette::fg(*m))), plain(m.label())])
            .collect(),
    };
    let mut jobs: Vec<Vec<Line<'static>>> = palette::JOB_LEGEND
        .iter()
        .map(|(name, status, merging)| {
            let (icon, _meaning) = palette::job_status(*status, *merging);
            vec![
                plain(name),
                Line::from(pill(format!("{icon} key│S1"), palette::job_pill(*status, *merging, false))),
                Line::from(Span::styled("3", palette::count_style(*status, *merging, 3))),
            ]
        })
        .collect();
    // the failed pill turns through the 28 moon phases, red, while one of its Jobs is being worked on (pro-d88)
    let failed = palette::JOB_LEGEND.iter().position(|(_, status, _)| *status == JobStatus::Failed).expect("the legend lists failed");
    jobs.insert(
        failed + 1,
        vec![
            plain("red loader: a failed Job being worked on"),
            Line::from(palette::SPINNER.iter().map(|m| Span::styled(*m, palette::fg_clickable(palette::Meaning::Failed))).collect::<Vec<_>>()),
        ],
    );
    // The counters' order is `JobCounts::counters`'s: planned, active, failed, done (the legend draws all four roles).
    let sample = JobCounts { draft: 1, running: 1, failed: 1, merged: 1, ..JobCounts::default() };
    let mut counters = Vec::new();
    for (i, n) in sample.counters().into_iter().enumerate() {
        if i > 0 {
            counters.push(palette::separator());
        }
        let (status, merging) = sample.counter_role(i);
        counters.push(Span::styled(n.to_string(), palette::count_style(status, merging, n)));
    }
    let order = ["planned", "active", "failed", "done"].join("⋅");
    jobs.push(vec![
        plain("counters row"),
        Line::from(counters),
        Line::from(order).style(palette::muted_text()),
    ]);
    jobs.push(vec![
        plain("cost"),
        Line::from(cpd_core::metrics::format_cost(0.12)).style(palette::muted_text()),
        Line::from("$0.012 ⋅ $1.23k ⋅ omitted when unknown").style(palette::muted_text()),
    ]);
    let jobs = Section { title: "Jobs: Plan Pills And Context Counters", header: Some(["status", "Plan pill", "counter"]), rows: jobs };
    let dot = |d: Dot, frame: usize| Line::from(palette::dot_span(d, frame, false));
    let loader = |d: Dot| Line::from((0..palette::SPINNER.len()).map(|f| palette::dot_span(d, f, false)).collect::<Vec<_>>());
    // the working loader once per phase (hs-d622): the cyan row is dispatch, which a Session with no Plan shares
    let frames = loader(Dot::Working(None));
    let fixing = loader(Dot::Fixing);
    let dots = Section {
        title: DOTS_TITLE,
        header: None,
        rows: vec![
            vec![plain(palette::NEEDS_YOU), plain("the Session needs you (colour below)")],
            vec![frames, plain("the Lead is working, dispatch (a Session with no Plan too) (one frame every 50 ms)")],
            vec![fixing, plain("a Plan's failed Job is being worked on (one frame every 50 ms)")],
            vec![dot(Dot::Waiting, 0), plain("waiting: an agent waits for another agent (a Worker for its Lead), not for you")],
            vec![dot(Dot::Idle, 0), plain("idle, nothing needed")],
            vec![dot(Dot::Stopped, 0), plain("pi stopped")],
            vec![dot(Dot::Red, 0), plain("red: a dialog in dispatch or deploy, a stuck Job, an engine error, your own broken pull request that nobody is fixing")],
            vec![dot(Dot::Yellow, 0), plain("yellow: ready to post, ready to merge, review requested")],
            vec![dot(Dot::Magenta, 0), plain("magenta: blocked while grounding, planning or done, or the Plan's review is pending")],
            vec![dot(Dot::Cyan, 0), plain("cyan: a cpd notice that blocks nothing: curation")],
            vec![dot(Dot::White, 0), plain("white: idle, not seen since it last went idle")],
            vec![loader(Dot::Working(Some(Work::Planning))), plain("the Lead is working, grounding (one frame every 50 ms)")],
            vec![loader(Dot::Working(Some(Work::Planning))), plain("the Lead is working, plan (one frame every 50 ms)")],
            vec![loader(Dot::Working(Some(Work::Review))), plain("the Lead is working, deploy (one frame every 50 ms)")],
            vec![loader(Dot::Working(Some(Work::Planning))), plain("the Lead is working, done (one frame every 50 ms)")],
            vec![plain(""), plain(&precedence())],
        ],
    };
    let sample = |(name, meaning): (&str, Meaning)| Line::from(Span::styled(format!("#2 {name}"), palette::fg_clickable(meaning)));
    let mut pr_rows: Vec<Vec<Line<'static>>> = palette::PR_LEGEND
        .iter()
        .map(|(name, state)| {
            let row = vec![sample((name, palette::pr_state(*state)))];
            // a closed pull request is waiting while its Lead handles it, red once it asks you (audit Q4)
            if *state == PrState::Closed { [row, vec![plain("its Lead handles it")]].concat() } else { row }
        })
        .collect();
    pr_rows.push(vec![sample(palette::PR_CLOSED_ASKED), plain("its Lead asks you")]);
    pr_rows.push(vec![sample(palette::PR_CONFLICT)]);
    pr_rows.push(vec![sample(palette::PR_UPDATING_RUNNING), plain("its update Job is fixing the conflict")]);
    pr_rows.push(vec![sample(palette::PR_UPDATING_QUEUED), plain("its update Job is queued")]);
    let prs = Section { title: "PRs (Dispatch)", header: None, rows: pr_rows };
    let checks = Section {
        title: "PR Checks",
        header: None,
        rows: vec![
            palette::CHECKS_LEGEND
                .iter()
                .map(|(name, c)| {
                    let (icon, meaning) = palette::checks(*c);
                    Line::from(vec![Span::styled(icon, palette::fg_clickable(meaning)), Span::styled(format!(" {name}"), palette::read_only_text())])
                })
                .collect(),
        ],
    };
    let tier = |urgency, meaning: &'static str| vec![Line::from(palette::tier_span(urgency)), plain(meaning)];
    let inbox = Section {
        title: "Inbox",
        header: None,
        rows: vec![
            tier(Urgency::Red, "a blocker only you can clear"),
            tier(Urgency::Yellow, "pull-request work"),
            tier(Urgency::Magenta, "a decision before the work runs"),
            tier(Urgency::Blue, "blue, external support: a review asked of you"),
            tier(Urgency::Cyan, "cyan, a cpd notice: curation, nothing blocks; also a change in the models a provider offers"),
            vec![Line::from(Span::styled(palette::ITEM_DOCS, palette::fg_clickable(Meaning::Notice))), plain("a documentation candidate")],
            vec![Line::from(Span::styled(palette::ITEM_RETIRE, palette::fg_clickable(Meaning::Notice))), plain("a record to retire")],
            tier(Urgency::White, "an unread reply"),
            vec![Line::from(Span::styled(palette::ITEM_DEPLOY, palette::fg_clickable(Meaning::Review))), plain("a deploy waits for your approval")],
            vec![Line::from(Span::styled(palette::ITEM_PR_READY, palette::fg_clickable(Meaning::Review))), plain("a pull request of yours, coloured by its state")],
            vec![Line::from(Span::styled(palette::ITEM_REVIEW_REQUESTED, palette::fg_clickable(Meaning::External))), plain("a review asked of you")],
        ],
    };
    let pulse = Section {
        title: "Pulse",
        header: None,
        // three entries a row: the popup is at most 80 % of the client tall (fewer an entry would not fit)
        rows: crate::plan::pulse::legend()
            .chunks(3)
            .map(|pair| pair.iter().flat_map(|(sample, meaning)| [Line::from(sample.clone()), plain(meaning)]).collect())
            .collect(),
    };
    vec![meanings, jobs, dots, prs, inbox, pulse, checks]
}

/// The vertical constraints of the body: each section after a gap.
fn rows(sections: &[Section]) -> Vec<Constraint> {
    sections.iter().flat_map(|s| [Constraint::Length(1), Constraint::Length(s.height())]).collect()
}

/// The rows of the body, under the title's row.
fn body_height(sections: &[Section]) -> u16 {
    rows(sections).iter().map(|c| if let Constraint::Length(n) = c { *n } else { 0 }).sum()
}

/// The size the legend needs: columns, rows (without a popup's border). The title is a section header, which
/// needs its own width: the padding, `╭─`, the pill (the title and its two caps), one `─` and the padding.
pub fn size() -> (u16, u16) {
    let sections = sections();
    let width = sections.iter().map(Section::width).max().unwrap_or(0) + 2;
    (width.max(INTRO.chars().count() as u16 + 7), HEADER_ROWS + body_height(&sections))
}

/// The whole body drawn off screen, `width` columns wide: the popup shows a window of it.
fn body(sections: &[Section], width: u16) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, body_height(sections)));
    let areas = Layout::vertical(rows(sections)).split(buf.area.inner(Margin::new(1, 0)));
    for (section, area) in sections.iter().zip(areas.iter().skip(1).step_by(2)) {
        section.draw(&mut buf, *area);
    }
    buf
}

/// Draws the legend in `area`, from its first row.
pub fn draw(f: &mut Frame, area: Rect) {
    draw_scrolled(f, area, &mut Scroll::default());
}

/// Draws the title as a section header, then the window of the body that `scroll` shows: it is fitted to `area` here
/// and, when the body is taller than the rest of `area`, a muted `first-last/total` closes the title's line.
fn draw_scrolled(f: &mut Frame, area: Rect, scroll: &mut Scroll) {
    let [head, window] = Layout::vertical([Constraint::Length(HEADER_ROWS), Constraint::Min(0)]).areas(area);
    let title = Rect::new(head.x, head.y + 1, head.width, 1).intersection(head);
    f.render_widget(SectionTitle::new(INTRO, None), head);
    let buf = body(&sections(), window.width);
    scroll.resize(usize::from(buf.area.height), usize::from(window.height));
    copy_cells(&buf, Rect::new(0, scroll.offset() as u16, window.width, window.height), f.buffer_mut(), Position::new(window.x, window.y));
    if scroll.scrollable() {
        let last = (scroll.offset() + scroll.page()).min(scroll.total());
        let position = format!(" {}-{}/{} ", scroll.offset() + 1, last, scroll.total());
        let width = position.chars().count() as u16;
        // ends one column before the edge: the padding stays blank
        if width + 1 < title.width {
            let at = Rect::new(title.x + title.width - 1 - width, title.y, width, 1);
            f.render_widget(Paragraph::new(position).style(palette::muted_text()), at);
        }
    }
}

/// The tmux arguments that open the legend in a popup: sized to the legend
/// (plus the border), at most the client's size `(columns, rows)`.
pub fn popup_args(client: (u16, u16)) -> Vec<String> {
    let (w, h) = size();
    let size = ((w + POPUP_BORDER).min(client.0), (h + POPUP_BORDER).min(client.1));
    popup::args(popup::Popup {
        title: Some(" legend "),
        border: false,
        client: None,
        dash: false,
        size,
        command: &COMMAND.join(" "),
    })
}

/// `cpd tui view legend`: draws the legend until a key that does not scroll, or a click.
pub(crate) fn run(ports: &Ports) -> Result<Output, CpdError> {
    run_view_on(ports, "legend", Capture::Mouse, || Ok(()), |ctx, ()| Ok(show_with(ctx)?))
}

/// What `event` does: the kit's scrolling moves `scroll` (↑↓, j/k, PgUp/PgDn, the wheel); whether it closes the legend:
/// any other key press, or a click.
fn closes(event: &Event, scroll: &mut Scroll) -> bool {
    match event {
        Event::Key(k) => !scroll.key(*k) && k.kind == KeyEventKind::Press,
        Event::Mouse(m) => !scroll.mouse(*m) && matches!(m.kind, MouseEventKind::Down(_)),
        _ => false,
    }
}

/// Draws the legend until a key that does not scroll, or a click: input and frames are the [`Ctx`]'s.
pub fn show_with<B: Backend>(ctx: &mut Ctx<B>) -> std::io::Result<()> {
    let mut scroll = Scroll::default();
    loop {
        ctx.draw(|f| draw_scrolled(f, f.area(), &mut scroll))?;
        if closes(&ctx.input.read()?, &mut scroll) {
            return Ok(());
        }
    }
}

/// The layout test's sample (`layout_tests`): the legend drawn as the view draws it; its title is rows 0 to 2 (row 2 is the
/// body's first gap).
#[cfg(test)]
pub(crate) fn layout_sample(width: u16, height: u16, case: crate::layout_tests::SectionCase) -> crate::layout_tests::Sample {
    use crate::layout_tests::{Kind, Placed, SectionCase, Sample};
    // the title row is tall enough for the whole body: a scrolled legend writes its position over the title's rule
    let height = if case == SectionCase::Title { height.max(size().1) } else { height };
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| draw(f, f.area())).unwrap();
    let pane = Rect::new(0, 0, width, height);
    let boxes = match case {
        SectionCase::Title => vec![Placed { name: "legend title", kind: Kind::Rule, rect: Rect::new(0, 0, width, 3), exempt: Vec::new() }],
        SectionCase::Rows => Vec::new(),
    };
    Sample { buf: terminal.backend().buffer().clone(), container: pane, boxes }
}

#[cfg(test)]
#[path = "legend_width_tests.rs"]
mod legend_width_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::section;
    use crate::read::Kind;
    use crate::test_text::row;

    static CAP_L: &str = "\u{e0b6}";
    static CAP_R: &str = "\u{e0b4}";

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn the_pulse_section_lists_the_kind_colours_in_precedence_order_the_grey_zero_and_quiet() {
        let pulse = &sections()[5];
        let cells: Vec<String> = pulse.rows.iter().flatten().map(text).collect();
        assert_eq!(
            cells,
            [
                "▂▄▆", "activity per 5 min, first colour wins",
                "▄", "an error, or a Job failed",
                "▄", "a Plan or ask Event",
                "▄", "a PR opened",
                "▄", "an escalation",
                "▄", "a Job or a PR merged",
                "▄", "any other activity",
                "▁", "no activity",
                "quiet 41m", "queued or running work with no activity for 30 min",
            ]
        );
        let colours: Vec<_> = pulse.rows.iter().flatten().step_by(2).map(|l| l.spans[0].style.fg).collect();
        let c = |k| Some(palette::pulse_kind(k));
        assert_eq!(
            colours,
            [
                None,
                c(Kind::Error),
                c(Kind::Plan),
                c(Kind::PrOpened),
                c(Kind::Escalation),
                c(Kind::Merged),
                c(Kind::Other),
                palette::muted_text().fg,
                palette::muted_text().fg
            ]
        );
    }

    #[test]
    fn session_dots_lists_every_glyph_and_each_circle_in_its_palette_colour() {
        let dots = &sections()[2];
        assert_eq!(dots.title, "Session Dots");
        let shown: String = dots.rows.iter().flatten().map(text).collect();
        let precedence = format!(
            "red > yellow > magenta > cyan > white > {} > {} > {} > {}",
            palette::SPINNER[0],
            palette::WAITING,
            palette::IDLE,
            palette::STOPPED
        );
        for glyph in [palette::NEEDS_YOU, &palette::SPINNER.concat(), palette::WAITING, palette::IDLE, palette::STOPPED, &precedence] {
            assert!(shown.contains(glyph), "{glyph}: {shown}");
        }
        for (row, dot) in dots.rows[6..11].iter().zip([Dot::Red, Dot::Yellow, Dot::Magenta, Dot::Cyan, Dot::White]) {
            assert_eq!(row[0].spans, [palette::dot_span(dot, 0, false)], "{dot:?}");
        }
        assert!(!shown.contains("phase"), "{shown}");
        let frames: Vec<_> = dots.rows[1][0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(frames, palette::SPINNER);
    }

    /// hs-d570 BL5, BL7: the cyan dot between magenta and white, and the precedence line with cyan.
    #[test]
    fn the_legends_session_dots_list_the_cyan_dot_and_the_precedence() {
        let dots = &sections()[2];
        assert_eq!(dots.title, "Session Dots");
        let cyan = dots.rows.iter().position(|r| r[0].spans == [palette::dot_span(Dot::Cyan, 0, false)]).expect("a cyan dot row");
        assert_eq!(dots.rows[cyan - 1][0].spans, [palette::dot_span(Dot::Magenta, 0, false)], "after magenta");
        assert_eq!(dots.rows[cyan + 1][0].spans, [palette::dot_span(Dot::White, 0, false)], "before white");
        assert!(text(&dots.rows[cyan][1]).starts_with("cyan:"), "{:?}", text(&dots.rows[cyan][1]));
        let precedence = format!(
            "red > yellow > magenta > cyan > white > {} > {} > {} > {}",
            palette::SPINNER[0],
            palette::WAITING,
            palette::IDLE,
            palette::STOPPED
        );
        let shown: Vec<String> = dots.rows.iter().map(|r| text(&r[1])).collect();
        assert!(shown.iter().any(|s| s.ends_with(&precedence)), "{shown:?}");
    }

    #[test]
    fn the_dots_say_what_needs_you_and_what_only_waits_for_an_agent() {
        let dots = &sections()[2];
        let says: Vec<String> = dots.rows.iter().map(|r| text(&r[1])).collect();
        assert_eq!(
            says[3],
            "waiting: an agent waits for another agent (a Worker for its Lead), not for you",
            "the waiting dot, after the loaders, before idle"
        );
        assert_eq!(dots.rows[3][0].spans, [palette::dot_span(Dot::Waiting, 0, false)]);
        assert_eq!(
            says[6],
            "red: a dialog in dispatch or deploy, a stuck Job, an engine error, your own broken pull request that nobody is fixing"
        );
        assert_eq!(says[7], "yellow: ready to post, ready to merge, review requested");
        let shown = says.join("\n");
        assert!(!shown.contains("Escalation") && !shown.contains("or pr"), "an Escalation is no item, the phase is deploy: {shown}");
    }

    #[test]
    fn the_pr_states_list_conflict_in_the_failed_colour_after_the_others() {
        let prs = &sections()[3];
        let names: Vec<String> = prs.rows.iter().map(|r| text(&r[0])).collect();
        assert_eq!(names, ["#2 open", "#2 merged", "#2 closed", "#2 closed", "#2 conflict", "#2 updating", "#2 updating"]);
        let conflict = &prs.rows[4][0].spans[0];
        assert_eq!(conflict.style, palette::fg_clickable(palette::Meaning::Failed));
    }

    #[test]
    fn a_closed_pr_is_waiting_while_its_lead_handles_it_and_red_once_it_asks_you() {
        let prs = &sections()[3];
        let (handled, asked) = (&prs.rows[2], &prs.rows[3]);
        assert_eq!(handled[0].spans[0].style, palette::fg_clickable(Meaning::Waiting));
        assert_eq!(asked[0].spans[0].style, palette::fg_clickable(Meaning::Failed));
        assert_eq!((text(&handled[1]), text(&asked[1])), ("its Lead handles it".to_owned(), "its Lead asks you".to_owned()));
    }

    #[test]
    fn a_sample_pill_is_cap_icon_key_bar_tier_cap_with_no_padding_like_the_charts() {
        // hs-d474: the legend's sample follows the chart's compact pill
        let jobs = &sections()[1];
        let pills: Vec<_> = jobs.rows.iter().filter(|r| r[1].spans[0].content == CAP_L).collect();
        for row in pills {
            let body = row[1].spans[1].content.as_ref();
            assert_eq!(row[1].spans[2].content, CAP_R);
            assert!(body.ends_with(" key│S1") && !body.starts_with(' ') && !body.contains(" │"), "{}: {body:?}", text(&row[0]));
        }
        let sample = |name: &str| text(&jobs.rows.iter().find(|r| text(&r[0]) == name).unwrap()[1]);
        assert_eq!(sample("queued"), format!("{CAP_L}{} key│S1{CAP_R}", palette::QUEUED));
    }

    #[test]
    fn the_job_statuses_listed_are_those_the_chart_draws_and_not_cancelled() {
        let jobs = &sections()[1];
        let names: Vec<String> = jobs.rows.iter().map(|r| text(&r[0])).collect();
        for name in ["queued", "running", "landing", "merged", "failed", "draft", "counters row"] {
            assert!(names.contains(&name.to_owned()), "{name}: {names:?}");
        }
        let shown: String =
            sections().iter().flat_map(|s| s.rows.iter().flatten().map(text)).chain(sections().iter().map(|s| s.title.to_owned())).collect();
        assert!(!shown.contains("cancelled"), "{shown}");
        // no glyph clause: `palette::CANCELLED` is the failed Job's glyph too (hs-d583), which the legend lists on purpose.
    }

    #[test]
    fn the_popup_is_sized_to_the_legend_and_capped_at_the_client() {
        let (w, h) = size();
        let args = popup_args((215, 80));
        assert_eq!(
            args,
            ["display-popup", "-E", "-T", " legend ", "-w", &(w + 2).to_string(), "-h", &(h + 2).to_string(), "cpd tui view legend"]
        );
        let small = popup_args((40, 20));
        assert_eq!((small[5].as_str(), small[7].as_str()), ("40", "20"));
    }

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent};

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn mouse(kind: MouseEventKind) -> Event {
        Event::Mouse(MouseEvent { kind, column: 4, row: 4, modifiers: KeyModifiers::NONE })
    }

    /// The legend drawn in a `width` x `height` area, with `scroll`.
    fn render(width: u16, height: u16, scroll: &mut Scroll) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw_scrolled(f, f.area(), scroll)).unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn the_title_is_a_section_title_under_an_empty_row_and_over_one() {
        let (w, h) = size();
        let buf = render(w, h, &mut Scroll::default());
        let mut want = Terminal::new(TestBackend::new(w, 1)).unwrap();
        want.draw(|f| f.render_widget(Paragraph::new(section::title(INTRO, None, usize::from(w))), f.area())).unwrap();
        assert!((0..w).all(|x| buf[(x, 1)] == want.backend().buffer()[(x, 0)]), "{}", row(&buf, 1));
        assert_eq!(row(&buf, 0), "");
        assert_eq!(row(&buf, 2), "");
        assert!(row(&buf, 1).starts_with(" ──\u{E0B6}Legend: What") && row(&buf, 1).contains("(Any Key Closes)\u{E0B4}─"));
    }

    #[test]
    fn a_legend_that_fits_does_not_scroll_and_shows_no_position() {
        let (w, h) = size();
        let mut scroll = Scroll::default();
        let buf = render(w, h, &mut scroll);
        assert!(!scroll.scrollable());
        assert!(!row(&buf, 0).contains('/'), "{}", row(&buf, 0));
        assert!(!closes(&key(KeyCode::Down), &mut scroll) && !closes(&mouse(MouseEventKind::ScrollDown), &mut scroll));
        assert_eq!(render(w, h, &mut scroll), buf);
    }

    #[test]
    fn a_legend_taller_than_its_popup_scrolls_with_keys_and_the_wheel() {
        let (w, h) = size();
        let full = render(w, h, &mut Scroll::default());
        let mut scroll = Scroll::default();
        let hr = HEADER_ROWS;
        let page = h - hr - 6;
        let top = render(w, h - 6, &mut scroll);
        assert!(scroll.scrollable() && (scroll.total(), scroll.page()) == (usize::from(h - hr), usize::from(page)));
        assert!(row(&top, 1).ends_with(&format!(" 1-{page}/{}", h - hr)), "{}", row(&top, 1));
        for y in hr..hr + page {
            assert_eq!(row(&top, y), row(&full, y), "unscrolled, row {y}");
        }
        // ↓ and j: one row each
        assert!(!closes(&key(KeyCode::Down), &mut scroll) && !closes(&key(KeyCode::Char('j')), &mut scroll));
        let moved = render(w, h - 6, &mut scroll);
        for y in hr..hr + page {
            assert_eq!(row(&moved, y), row(&full, y + 2), "two rows down, row {y}");
        }
        // the wheel: a notch is three rows; PgDn stops at the last row, which is the legend's
        assert!(!closes(&mouse(MouseEventKind::ScrollUp), &mut scroll));
        assert_eq!(scroll.offset(), 0);
        assert!(!closes(&mouse(MouseEventKind::ScrollDown), &mut scroll));
        assert_eq!(scroll.offset(), 3);
        for _ in 0..3 {
            assert!(!closes(&key(KeyCode::PageDown), &mut scroll));
        }
        let bottom = render(w, h - 6, &mut scroll);
        assert_eq!(scroll.offset(), scroll.max_offset());
        assert_eq!(row(&bottom, hr + page - 1), row(&full, h - 1));
        assert!(row(&bottom, 1).ends_with(&format!(" {}-{}/{}", scroll.max_offset() + 1, h - hr, h - hr)), "{}", row(&bottom, 1));
    }

    #[test]
    fn any_key_that_does_not_scroll_closes_the_legend_and_so_does_a_click() {
        let mut scroll = Scroll::default();
        scroll.resize(50, 10);
        for code in [KeyCode::Char('q'), KeyCode::Esc, KeyCode::Enter, KeyCode::Char(' '), KeyCode::Char('J'), KeyCode::Home] {
            assert!(closes(&key(code), &mut scroll), "{code:?}");
        }
        for code in [KeyCode::Up, KeyCode::Down, KeyCode::Char('j'), KeyCode::Char('k'), KeyCode::PageUp, KeyCode::PageDown] {
            assert!(!closes(&key(code), &mut scroll), "{code:?}");
        }
        let mut release = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert!(!closes(&Event::Key(release), &mut scroll));
        assert!(closes(&mouse(MouseEventKind::Down(MouseButton::Left)), &mut scroll));
        for kind in [MouseEventKind::ScrollUp, MouseEventKind::ScrollDown, MouseEventKind::Moved, MouseEventKind::Up(MouseButton::Left)] {
            assert!(!closes(&mouse(kind), &mut scroll), "{kind:?}");
        }
        assert!(!closes(&Event::FocusGained, &mut scroll));
    }

    // ---- the legend's titles v4 (hs-d475, hs-d476): red until they are Title Case and the popup's title is a grey pill in its padding ----

    #[test]
    fn the_section_titles_are_in_title_case() {
        let titles: Vec<&str> = sections().iter().map(|s| s.title).collect();
        for want in ["Jobs: Plan Pills And Context Counters", "Session Dots", "Pulse"] {
            assert!(titles.contains(&want), "{want:?} is a section title: {titles:?}");
        }
        assert!(!titles.contains(&"Session dots") && !titles.iter().any(|t| t.starts_with("Jobs: Plan pills")), "{titles:?}");
    }

    #[test]
    fn the_popups_title_is_a_grey_pill_inside_the_padding_of_one_column_each_side() {
        let (w, h) = size();
        let buf = render(w, h, &mut Scroll::default());
        assert_eq!((buf[(0, 1)].symbol(), buf[(1, 1)].symbol(), buf[(w - 1, 1)].symbol()), (" ", "─", " "), "{}", row(&buf, 1));
        assert_eq!((buf[(2, 1)].symbol(), buf[(3, 1)].symbol(), buf[(4, 1)].symbol()), ("─", "\u{E0B6}", "L"), "{}", row(&buf, 1));
        let text: String = (4..).map(|x| buf[(x, 1)].symbol()).take_while(|s| *s != "\u{E0B4}").collect();
        section::check::grey_pill(&buf, 3, 1, &text);
    }

    // ---- the Inbox section (hs-d483, hs-d570): the six tiers, red to white, each glyph in its colour with its meaning; red until it exists ----

    #[test]
    fn the_legends_inbox_section_lists_six_tiers_and_the_two_curate_glyphs() {
        let all = sections();
        let at = all.iter().position(|s| s.title == "Inbox").unwrap_or_else(|| panic!("no Inbox section: {:?}", all.iter().map(|s| s.title).collect::<Vec<_>>()));
        let inbox = &all[at];
        let cells: Vec<String> = inbox.rows.iter().flatten().map(text).collect();
        assert_eq!(
            cells,
            [
                "\u{F0362}", "a blocker only you can clear",
                "\u{F069A}", "pull-request work",
                "\u{F173A}", "a decision before the work runs",
                "\u{F0368}", "blue, external support: a review asked of you",
                "\u{F06F1}", "cyan, a cpd notice: curation, nothing blocks; also a change in the models a provider offers",
                "\u{F0653}", "a documentation candidate",
                "\u{F116E}", "a record to retire",
                "\u{F0366}", "an unread reply",
                "\u{F15A9}", "a deploy waits for your approval",
                "\u{F12F4}", "a pull request of yours, coloured by its state",
                "\u{F12F2}", "a review asked of you",
            ]
        );
        let colours: Vec<_> = inbox.rows.iter().map(|r| r[0].spans[0].style.fg).collect();
        let c = |m| Some(palette::color_clickable(m));
        assert_eq!(
            colours,
            [c(Meaning::Failed), c(Meaning::Review), c(Meaning::Planning), c(Meaning::External), c(Meaning::Notice), c(Meaning::Notice), c(Meaning::Notice), c(Meaning::NotStarted), c(Meaning::Review), c(Meaning::Review), c(Meaning::External)],
            "red, yellow, magenta, blue, cyan (three lines: the tier and the two curate kinds), white, then the deploy, pr-ready and review-requested lines"
        );
        let dots = all.iter().position(|s| s.title == "Session Dots" || s.title == "Session dots").unwrap();
        let pulse = all.iter().position(|s| s.title == "Pulse").unwrap();
        assert!(dots < at && at < pulse, "between the Session Dots and the Pulse: {:?}", all.iter().map(|s| s.title).collect::<Vec<_>>());
        let shown: String = cells.concat();
        assert!(palette::SPINNER.iter().all(|frame| !shown.contains(frame)), "the Inbox section draws no state dot: {shown:?}");
    }
}
