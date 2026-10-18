//! The legend: what every colour and icon of the cockpit means, ported from the
//! spike's `legend.rs`. `cpd tui view legend` draws it; the widgets view's `?`
//! opens it in a tmux popup, since the right column is narrower than the
//! legend (commands.md §13.2). Everything it shows, icons and colours, comes
//! from the palette and from the read model's own ordering of the counters, so
//! it cannot disagree with them. Each section is a ratatui `Table` (aligned
//! columns); the sections are stacked with a `Layout`.

use ratatui::Frame;
use ratatui::crossterm::event::{self, Event, KeyEventKind, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table};

use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::output::Output;
use cpd_core::plan::Phase;
use cpd_core::session::PROGRAM;
use crate::palette::{self, Meaning};
use crate::read::{Dot, JobCounts};
use crate::{not_a_terminal, on_a_terminal};

const COLUMN_SPACING: u16 = 2;
/// A tmux popup draws a border of one cell on each side.
const POPUP_BORDER: u16 = 2;
const DOTS_TITLE: &str = "Session dots";
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

    fn draw(&self, f: &mut Frame, area: Rect) {
        let [title, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        f.render_widget(
            Paragraph::new(Span::styled(self.title, Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED))),
            title,
        );
        let mut table = Table::new(
            self.rows.iter().map(|r| Row::new(r.iter().cloned().map(Cell::from))),
            self.column_widths().into_iter().map(Constraint::Length),
        )
        .column_spacing(COLUMN_SPACING);
        if let Some(h) = self.header {
            table = table
                .header(Row::new(h.iter().map(|t| Cell::from(*t))).style(Style::new().add_modifier(Modifier::DIM)));
        }
        f.render_widget(table, body);
    }
}

fn plain(text: &str) -> Line<'static> {
    Line::from(text.to_string())
}

/// The Context row's precedence line, with the real glyphs (the working one as its first frame).
fn precedence() -> String {
    format!(
        "Context row: its most urgent Session: red > yellow > magenta > white > {} > {} > {}",
        palette::SPINNER[0],
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
                Line::from(Span::styled(format!(" {icon} key │S1 "), palette::pill(meaning))),
                Line::from(Span::styled("3", palette::count_style(*status, *merging, 3))),
            ]
        })
        .collect();
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
            vec![dot(Dot::Idle, 0), plain("idle, nothing needed")],
            vec![dot(Dot::Stopped, 0), plain("pi stopped")],
            vec![dot(Dot::Red, 0), plain("red: blocked work (a dialog, an Escalation) in dispatch or pr")],
            vec![dot(Dot::Yellow, 0), plain("yellow: the Plan's Lead, its pull request is open with checks passed or failed")],
            vec![dot(Dot::Magenta, 0), plain("magenta: blocked while grounding, planning or done, or the Plan's review is pending")],
            vec![dot(Dot::White, 0), plain("white: idle, not seen since it last went idle")],
            vec![plain(""), plain(&precedence())],
        ],
    };
    let prs = Section {
        title: "PRs (Dispatch)",
        header: None,
        rows: palette::PR_LEGEND
            .iter()
            .map(|(name, state)| vec![Line::from(Span::styled(format!("#2 {name}"), palette::fg(palette::pr_state(*state))))])
            .collect(),
    };
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
    vec![meanings, jobs, dots, prs, checks]
}

/// The vertical constraints: the intro, then each section after a gap.
fn rows(sections: &[Section]) -> Vec<Constraint> {
    [Constraint::Length(1)]
        .into_iter()
        .chain(sections.iter().flat_map(|s| [Constraint::Length(1), Constraint::Length(s.height())]))
        .collect()
}

/// The size the legend needs: columns, rows (without a popup's border).
pub fn size() -> (u16, u16) {
    let sections = sections();
    let width = sections.iter().map(Section::width).max().unwrap_or(0).max(INTRO.chars().count() as u16);
    let height = rows(&sections).iter().map(|c| if let Constraint::Length(n) = c { *n } else { 0 }).sum::<u16>();
    (width + 2, height)
}

pub fn draw(f: &mut Frame, area: Rect) {
    let sections = sections();
    let areas = Layout::vertical(rows(&sections)).split(area.inner(Margin::new(1, 0)));
    f.render_widget(Paragraph::new(Span::styled(INTRO, Style::new().add_modifier(Modifier::BOLD))), areas[0]);
    for (section, area) in sections.iter().zip(areas.iter().skip(2).step_by(2)) {
        section.draw(f, *area);
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

/// `cpd tui view legend`: draws the legend until any key or click.
pub(crate) fn run() -> Result<Output, CpdError> {
    if !on_a_terminal() {
        return Err(not_a_terminal("tui view legend"));
    }
    // From here on this process owns the terminal (commands.md §1.2).
    let mut terminal = ratatui::init();
    let result = show(&mut terminal);
    ratatui::restore();
    match result {
        Ok(()) => Ok(Output::Owned),
        Err(e) => Err(CpdError::new(ErrorCode::NotATerminal, format!("The terminal of cpd tui view legend failed: {e}."))
            .with("command", "tui view legend")),
    }
}

fn show(terminal: &mut ratatui::DefaultTerminal) -> std::io::Result<()> {
    loop {
        terminal.draw(|f| draw(f, f.area()))?;
        match event::read()? {
            Event::Key(k) if k.kind == KeyEventKind::Press => return Ok(()),
            Event::Mouse(m) if matches!(m.kind, MouseEventKind::Down(_)) => return Ok(()),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
                "PR checks",
            ]
        );
    }

    #[test]
    fn session_dots_lists_every_glyph_and_each_circle_in_its_palette_colour() {
        let dots = &sections()[2];
        assert_eq!(dots.title, "Session dots");
        let shown: String = dots.rows.iter().flatten().map(text).collect();
        let precedence = format!("red > yellow > magenta > white > {} > {} > {}", palette::SPINNER[0], palette::IDLE, palette::STOPPED);
        for glyph in [palette::NEEDS_YOU, &palette::SPINNER.concat(), palette::IDLE, palette::STOPPED, &precedence] {
            assert!(shown.contains(glyph), "{glyph}: {shown}");
        }
        for (row, dot) in dots.rows[4..8].iter().zip([Dot::Red, Dot::Yellow, Dot::Magenta, Dot::White]) {
            assert_eq!(row[0].spans, [palette::dot_span(dot, 0, false)], "{dot:?}");
        }
        assert!(!shown.contains("phase"), "{shown}");
        let frames: Vec<_> = dots.rows[1][0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(frames, palette::SPINNER);
    }

    #[test]
    fn the_job_statuses_listed_are_those_the_chart_draws_and_not_cancelled() {
        let jobs = &sections()[1];
        let names: Vec<String> = jobs.rows.iter().map(|r| text(&r[0])).collect();
        assert_eq!(names, ["queued", "running", "merging", "merged", "failed", "draft", "counters row"]);
        let shown: String =
            sections().iter().flat_map(|s| s.rows.iter().flatten().map(text)).chain(sections().iter().map(|s| s.title.to_owned())).collect();
        assert!(!shown.contains("cancelled") && !shown.contains('⊘'), "{shown}");
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
}
