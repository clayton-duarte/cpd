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
use ratatui::crossterm::event::{self, Event, KeyEventKind, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, Widget};

use cpd_core::error::CpdError;
use cpd_core::output::Output;
use cpd_core::plan::Phase;
use cpd_core::session::PROGRAM;
use crate::dialog::scroll::Scroll;
use crate::palette::{self, Meaning};
use crate::read::{Dot, JobCounts, JobStatus, PrState};
use crate::section;
use crate::{Capture, run_view};

/// The rounded caps of a Plan pill (powerline, U+E0B6 and U+E0B4), as the Plan view draws them (`plan/flow.rs`, which is
/// private to its module).
const CAP_L: &str = "\u{e0b6}";
const CAP_R: &str = "\u{e0b4}";

/// A sample pill with its rounded caps: the caps in the pill's colour on the default background, as the Plan view draws
/// a Job pill (`plan/flow.rs`).
fn capped(body: String, pill: Style) -> Line<'static> {
    let cap = Style::new().fg(pill.bg.expect("a pill has a colour"));
    Line::from(vec![Span::styled(CAP_L, cap), Span::styled(body, pill), Span::styled(CAP_R, cap)])
}

const COLUMN_SPACING: u16 = 2;
/// A tmux popup draws a border of one cell on each side.
const POPUP_BORDER: u16 = 2;
const DOTS_TITLE: &str = "Session dots";
/// The rows above the body: the title, drawn as a section header (hs-d124, hs-d297).
/// The title's rows: an empty row above it; the body's first row is the empty row below (hs-d308).
const HEADER_ROWS: u16 = 2;
const INTRO: &str = "Legend: what the cockpit's colours and icons mean   (any key closes)";
/// The popup's command: the legend view, run like every pane of the cockpit (by `PATH`).
const COMMAND: [&str; 4] = [PROGRAM, "tui", "view", "legend"];

struct Section {
    title: &'static str,
    header: Option<[&'static str; 3]>,
    rows: Vec<Vec<Line<'static>>>,
}

impl Section {
    /// Width of each column: the widest cell, the header included.
    fn column_widths(&self) -> Vec<u16> {
        let columns = self.rows.iter().map(Vec::len).max().unwrap_or(0);
        (0..columns)
            .map(|c| {
                let cells = self.rows.iter().filter_map(|r| r.get(c)).map(Line::width);
                let head = self.header.map_or(0, |h| h.get(c).map_or(0, |t| t.chars().count()));
                cells.chain(std::iter::once(head)).max().unwrap_or(0) as u16
            })
            .collect()
    }

    /// The table's width, or the title's when that is wider.
    fn width(&self) -> u16 {
        let w = self.column_widths();
        (w.iter().sum::<u16>() + COLUMN_SPACING * (w.len() as u16).saturating_sub(1))
            .max(self.title.chars().count() as u16)
    }

    /// The title row, the header row if any, and the rows.
    fn height(&self) -> u16 {
        1 + u16::from(self.header.is_some()) + self.rows.len() as u16
    }

    fn draw(&self, buf: &mut Buffer, area: Rect) {
        let [title, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        Paragraph::new(Span::styled(self.title, Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)))
            .render(title, buf);
        let mut table = Table::new(
            self.rows.iter().map(|r| Row::new(r.iter().cloned().map(Cell::from))),
            self.column_widths().into_iter().map(Constraint::Length),
        )
        .column_spacing(COLUMN_SPACING);
        if let Some(h) = self.header {
            table = table
                .header(Row::new(h.iter().map(|t| Cell::from(*t))).style(Style::new().add_modifier(Modifier::DIM)));
        }
        table.render(body, buf);
    }
}

fn plain(text: &str) -> Line<'static> {
    Line::from(text.to_string())
}

/// The Context row's precedence line, with the real glyphs (the working one as its first frame).
fn precedence() -> String {
    format!(
        "Context row: its most urgent Session: red > yellow > magenta > white > {} > {} > {} > {}",
        palette::SPINNER[0],
        palette::WAITING,
        palette::IDLE,
        palette::STOPPED
    )
}

/// The sections, built only from the palette and the read model's counters.
fn sections() -> Vec<Section> {
    let meanings = Section {
        title: "Meanings → colour (one source for every view)",
        header: None,
        rows: Meaning::ALL
            .iter()
            .map(|m| vec![Line::from(Span::styled("██", palette::fg(*m))), plain(m.label())])
            .collect(),
    };
    let mut jobs: Vec<Vec<Line<'static>>> = palette::JOB_LEGEND
        .iter()
        .map(|(name, status, merging)| {
            let (icon, meaning) = palette::job_status(*status, *merging);
            vec![
                plain(name),
                capped(format!(" {icon} key │S1 "), palette::pill(meaning)),
                Line::from(Span::styled("3", palette::count_style(*status, *merging, 3))),
            ]
        })
        .collect();
    // a failed Job is waiting (its Lead handles it) until `job-stuck` opens (audit Q3): the stuck one is red, after `failed`
    let (stuck_icon, stuck) = palette::JOB_STUCK;
    let failed = palette::JOB_LEGEND.iter().position(|(_, status, _)| *status == JobStatus::Failed).expect("the legend lists failed");
    jobs.insert(
        failed + 1,
        vec![
            plain("failed, stuck"),
            capped(format!(" {stuck_icon} key │S1 "), palette::pill(stuck)),
            Line::from(Span::styled("3", palette::fg(stuck))),
        ],
    );
    // The counters' order is `JobCounts::counters`'s: queued, running, merging, failed, merged.
    let sample = JobCounts { queued: 1, running: 1, merging: 2, failed: 0, merged: 3, ..JobCounts::default() };
    let slots = [0, 1, 2, 4, 3].map(|i| palette::JOB_LEGEND[i]);
    let mut counters = Vec::new();
    for (i, ((_, status, merging), n)) in slots.iter().zip(sample.counters(Phase::Dispatch)).enumerate() {
        if i > 0 {
            counters.push(palette::separator());
        }
        counters.push(Span::styled(n.to_string(), palette::count_style(*status, *merging, n)));
    }
    let order = slots.map(|(name, _, _)| name).join("⋅");
    jobs.push(vec![
        plain("counters row"),
        Line::from(counters),
        Line::from(order).style(Style::new().add_modifier(Modifier::DIM)),
    ]);
    let jobs = Section { title: "Jobs: Plan pills and Context counters", header: Some(["status", "Plan pill", "counter"]), rows: jobs };
    let dot = |d: Dot, frame: usize| Line::from(palette::dot_span(d, frame, false));
    let frames = Line::from((0..palette::SPINNER.len()).map(|f| palette::dot_span(Dot::Working, f, false)).collect::<Vec<_>>());
    let dots = Section {
        title: DOTS_TITLE,
        header: None,
        rows: vec![
            vec![plain(palette::NEEDS_YOU), plain("the Session needs you (colour below)")],
            vec![frames, plain("the Lead is working (one frame every 100 ms)")],
            vec![dot(Dot::Waiting, 0), plain("waiting: an agent waits for another agent (a Worker for its Lead), not for you")],
            vec![dot(Dot::Idle, 0), plain("idle, nothing needed")],
            vec![dot(Dot::Stopped, 0), plain("pi stopped")],
            vec![dot(Dot::Red, 0), plain("red: a dialog in dispatch or deploy, a stuck Job, an engine error, your own failing or changed pull request")],
            vec![dot(Dot::Yellow, 0), plain("yellow: ready to merge, review requested")],
            vec![dot(Dot::Magenta, 0), plain("magenta: blocked while grounding, planning or done, or the Plan's review is pending")],
            vec![dot(Dot::White, 0), plain("white: idle, not seen since it last went idle")],
            vec![plain(""), plain(&precedence())],
        ],
    };
    let sample = |(name, meaning): (&str, Meaning)| Line::from(Span::styled(format!("#2 {name}"), palette::fg(meaning)));
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
    let prs = Section { title: "PRs (Dispatch)", header: None, rows: pr_rows };
    let checks = Section {
        title: "PR checks",
        header: None,
        rows: vec![
            palette::CHECKS_LEGEND
                .iter()
                .map(|(name, c)| {
                    let (icon, meaning) = palette::checks(*c);
                    Line::from(vec![Span::styled(icon, palette::fg(meaning)), format!(" {name}").into()])
                })
                .collect(),
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
    vec![meanings, jobs, dots, prs, pulse, checks]
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
/// needs its own width: `╭─`, the pill (the title and its two caps) and one `─`.
pub fn size() -> (u16, u16) {
    let sections = sections();
    let width = sections.iter().map(Section::width).max().unwrap_or(0) + 2;
    (width.max(INTRO.chars().count() as u16 + 5), HEADER_ROWS + body_height(&sections))
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
/// and, when the body is taller than the rest of `area`, a dim `first-last/total` closes the title's line.
fn draw_scrolled(f: &mut Frame, area: Rect, scroll: &mut Scroll) {
    let [head, window] = Layout::vertical([Constraint::Length(HEADER_ROWS), Constraint::Min(0)]).areas(area);
    let title = Rect::new(head.x, head.y + 1, head.width, 1).intersection(head);
    f.render_widget(Paragraph::new(section::title(INTRO, None, usize::from(title.width))), title);
    let buf = body(&sections(), window.width);
    scroll.resize(usize::from(buf.area.height), usize::from(window.height));
    for y in 0..window.height.min(buf.area.height.saturating_sub(scroll.offset() as u16)) {
        for x in 0..window.width {
            f.buffer_mut()[(window.x + x, window.y + y)] = buf[(x, y + scroll.offset() as u16)].clone();
        }
    }
    if scroll.scrollable() {
        let last = (scroll.offset() + scroll.page()).min(scroll.total());
        let position = format!(" {}-{}/{} ", scroll.offset() + 1, last, scroll.total());
        let width = position.chars().count() as u16;
        if width < title.width {
            let at = Rect::new(title.x + title.width - width, title.y, width, 1);
            f.render_widget(Paragraph::new(position).style(palette::fg(Meaning::Nothing)), at);
        }
    }
}

/// The tmux arguments that open the legend in a popup: sized to the legend
/// (plus the border), at most the client's size `(columns, rows)`.
pub fn popup_args(client: (u16, u16)) -> Vec<String> {
    let (w, h) = size();
    [
        "display-popup".to_owned(),
        "-E".to_owned(),
        "-T".to_owned(),
        " legend ".to_owned(),
        "-w".to_owned(),
        (w + POPUP_BORDER).min(client.0).to_string(),
        "-h".to_owned(),
        (h + POPUP_BORDER).min(client.1).to_string(),
        COMMAND.join(" "),
    ]
    .to_vec()
}

/// `cpd tui view legend`: draws the legend until a key that does not scroll, or a click.
pub(crate) fn run() -> Result<Output, CpdError> {
    run_view("legend", Capture::Mouse, || Ok(()), |terminal, ()| Ok(show(terminal)?))
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

fn show(terminal: &mut ratatui::DefaultTerminal) -> std::io::Result<()> {
    let mut scroll = Scroll::default();
    loop {
        terminal.draw(|f| draw_scrolled(f, f.area(), &mut scroll))?;
        if closes(&event::read()?, &mut scroll) {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::Kind;

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn the_section_titles_are_exact() {
        let titles: Vec<&str> = sections().iter().map(|s| s.title).collect();
        assert_eq!(
            titles,
            [
                "Meanings → colour (one source for every view)",
                "Jobs: Plan pills and Context counters",
                "Session dots",
                "PRs (Dispatch)",
                "Pulse",
                "PR checks",
            ]
        );
    }

    #[test]
    fn the_pulse_section_lists_the_kind_colours_in_precedence_order_the_grey_zero_and_quiet() {
        let pulse = &sections()[4];
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
                palette::fg(Meaning::Nothing).fg,
                palette::fg(Meaning::Review).fg
            ]
        );
    }

    #[test]
    fn session_dots_lists_every_glyph_and_each_circle_in_its_palette_colour() {
        let dots = &sections()[2];
        assert_eq!(dots.title, "Session dots");
        let shown: String = dots.rows.iter().flatten().map(text).collect();
        let precedence = format!(
            "red > yellow > magenta > white > {} > {} > {} > {}",
            palette::SPINNER[0],
            palette::WAITING,
            palette::IDLE,
            palette::STOPPED
        );
        for glyph in [palette::NEEDS_YOU, &palette::SPINNER.concat(), palette::WAITING, palette::IDLE, palette::STOPPED, &precedence] {
            assert!(shown.contains(glyph), "{glyph}: {shown}");
        }
        for (row, dot) in dots.rows[5..9].iter().zip([Dot::Red, Dot::Yellow, Dot::Magenta, Dot::White]) {
            assert_eq!(row[0].spans, [palette::dot_span(dot, 0, false)], "{dot:?}");
        }
        assert!(!shown.contains("phase"), "{shown}");
        let frames: Vec<_> = dots.rows[1][0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(frames, palette::SPINNER);
    }

    #[test]
    fn the_dots_say_what_needs_you_and_what_only_waits_for_an_agent() {
        let dots = &sections()[2];
        let says: Vec<String> = dots.rows.iter().map(|r| text(&r[1])).collect();
        assert_eq!(
            says[2],
            "waiting: an agent waits for another agent (a Worker for its Lead), not for you",
            "the waiting dot, between working and idle"
        );
        assert_eq!(dots.rows[2][0].spans, [palette::dot_span(Dot::Waiting, 0, false)]);
        assert_eq!(
            says[5],
            "red: a dialog in dispatch or deploy, a stuck Job, an engine error, your own failing or changed pull request"
        );
        assert_eq!(says[6], "yellow: ready to merge, review requested");
        let shown = says.join("\n");
        assert!(!shown.contains("Escalation") && !shown.contains("or pr"), "an Escalation is no item, the phase is deploy: {shown}");
    }

    #[test]
    fn the_pr_states_list_conflict_in_the_failed_colour_after_the_others() {
        let prs = &sections()[3];
        let names: Vec<String> = prs.rows.iter().map(|r| text(&r[0])).collect();
        assert_eq!(names, ["#2 open", "#2 merged", "#2 closed", "#2 closed", "#2 conflict"]);
        let conflict = &prs.rows[4][0].spans[0];
        assert_eq!(conflict.style, palette::fg(palette::Meaning::Failed));
    }

    #[test]
    fn a_closed_pr_is_waiting_while_its_lead_handles_it_and_red_once_it_asks_you() {
        let prs = &sections()[3];
        let (handled, asked) = (&prs.rows[2], &prs.rows[3]);
        assert_eq!(handled[0].spans[0].style, palette::fg(Meaning::Waiting));
        assert_eq!(asked[0].spans[0].style, palette::fg(Meaning::Failed));
        assert_eq!((text(&handled[1]), text(&asked[1])), ("its Lead handles it".to_owned(), "its Lead asks you".to_owned()));
    }

    #[test]
    fn a_failed_job_is_waiting_and_a_stuck_one_is_red_in_the_pill_and_the_counter() {
        let jobs = &sections()[1];
        let (failed, stuck) = (&jobs.rows[4], &jobs.rows[5]);
        assert_eq!((text(&failed[0]), text(&stuck[0])), ("failed".to_owned(), "failed, stuck".to_owned()));
        // the pill's body, after its left cap
        let bg = |row: &Vec<Line>| row[1].spans[1].style.bg;
        assert_eq!((bg(failed), bg(stuck)), (palette::pill(Meaning::Waiting).bg, palette::pill(Meaning::Failed).bg));
        let fg = |row: &Vec<Line>| row[2].spans[0].style.fg;
        assert_eq!((fg(failed), fg(stuck)), (palette::fg(Meaning::Waiting).fg, palette::fg(Meaning::Failed).fg));
    }

    #[test]
    fn the_job_statuses_listed_are_those_the_chart_draws_and_not_cancelled() {
        let jobs = &sections()[1];
        let names: Vec<String> = jobs.rows.iter().map(|r| text(&r[0])).collect();
        assert_eq!(names, ["queued", "running", "merging", "merged", "failed", "failed, stuck", "draft", "counters row"]);
        let shown: String =
            sections().iter().flat_map(|s| s.rows.iter().flatten().map(text)).chain(sections().iter().map(|s| s.title.to_owned())).collect();
        assert!(!shown.contains("cancelled") && !shown.contains(palette::CANCELLED), "{shown}");
    }

    #[test]
    fn the_popup_is_sized_to_the_legend_and_capped_at_the_client() {
        let (w, h) = size();
        let args = popup_args((215, 62));
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

    fn row(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_owned()
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
        assert!(row(&buf, 1).starts_with("╭─\u{E0B6}Legend: what") && row(&buf, 1).contains("(any key closes)\u{E0B4}─"));
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
        assert_eq!((buf[(0, 1)].symbol(), buf[(1, 1)].symbol(), buf[(w - 1, 1)].symbol()), (" ", "╭", " "), "{}", row(&buf, 1));
        assert_eq!((buf[(2, 1)].symbol(), buf[(3, 1)].symbol(), buf[(4, 1)].symbol()), ("─", "\u{E0B6}", "L"), "{}", row(&buf, 1));
        let text: String = (4..).map(|x| buf[(x, 1)].symbol()).take_while(|s| *s != "\u{E0B4}").collect();
        section::check::grey_pill(&buf, 3, 1, &text);
    }
}
