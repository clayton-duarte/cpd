//! The legend: what every colour and icon of the cockpit means. `cpd-tui legend` draws it (the Context pane's button
//! and `?` open it in a tmux popup). Everything it shows, icons and colours, comes from `palette.rs` and from the
//! views' own functions (the Plan pill's text, the Context counters), so it cannot disagree with them.
//! Each section is a ratatui `Table` (aligned columns); the sections are stacked with a `Layout`.

use std::path::Path;

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, Paragraph, Row, Table},
};

use crate::{
    actions::quote,
    context::{self, Workflow},
    flow::{self, FlowJob},
    palette::{self, Meaning},
};

const COLUMN_SPACING: u16 = 2;
/// A tmux popup draws a border of one cell on each side.
const POPUP_BORDER: u16 = 2;
const INTRO: &str = "Legend: what the cockpit's colours and icons mean   (any key closes)";

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
        (w.iter().sum::<u16>() + COLUMN_SPACING * (w.len() as u16).saturating_sub(1)).max(self.title.chars().count() as u16)
    }

    /// The title row, the header row if any, and the rows.
    fn height(&self) -> u16 {
        1 + u16::from(self.header.is_some()) + self.rows.len() as u16
    }

    fn draw(&self, f: &mut Frame, area: Rect) {
        let [title, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        f.render_widget(Paragraph::new(Span::styled(self.title, Style::new().add_modifier(Modifier::BOLD | Modifier::UNDERLINED))), title);
        let mut table = Table::new(self.rows.iter().map(|r| Row::new(r.iter().cloned().map(Cell::from))), self.column_widths().into_iter().map(Constraint::Length)).column_spacing(COLUMN_SPACING);
        if let Some(h) = self.header {
            table = table.header(Row::new(h.iter().map(|t| Cell::from(*t))).style(Style::new().add_modifier(Modifier::DIM)));
        }
        f.render_widget(table, body);
    }
}

fn plain(text: &str) -> Line<'static> {
    Line::from(text.to_string())
}

/// The sections, built only from the palette and the views' own text.
fn sections() -> Vec<Section> {
    let meanings = Section {
        title: "Meanings → colour (one source for every view)",
        header: None,
        rows: Meaning::ALL.iter().map(|m| vec![Line::from(Span::styled("██", palette::fg(*m))), plain(m.label())]).collect(),
    };
    let mut jobs: Vec<Vec<Line<'static>>> = palette::JOB_LEGEND
        .iter()
        .map(|(name, status)| {
            let job = FlowJob { key: "key".into(), status: status.to_string(), tier: "S1".into(), needs: Vec::new(), attempt: 1 };
            vec![
                plain(name),
                Line::from(Span::styled(flow::pill_text(&job, 3), palette::pill(palette::job_status(status).1))),
                Line::from(Span::styled("3", palette::count_style(status, 3))),
            ]
        })
        .collect();
    let sample = Workflow { phase: "dispatch".into(), counts: [1, 1, 2, 3, 0] };
    jobs.push(vec![plain("counters row"), context::counters(&sample), Line::from("queued⋅running⋅in review⋅merged⋅failed").style(Style::new().add_modifier(Modifier::DIM))]);
    let jobs = Section { title: "Jobs: Plan pills and Context counters", header: Some(["status", "Plan pill", "counter"]), rows: jobs };
    let dots = Section {
        title: "Context dots: ● needs you, ○ does not; the colour is the Workflow's phase",
        header: None,
        rows: vec![palette::PHASE_LEGEND.iter().map(|(name, phase)| Line::from(vec![palette::dot(true, *phase), " ".into(), palette::dot(false, *phase), format!(" {name}").into()])).collect()],
    };
    let prs = Section {
        title: "PRs (Dispatch)",
        header: None,
        rows: palette::PR_LEGEND.iter().map(|s| vec![Line::from(Span::styled(format!("#2 {s}"), palette::fg(palette::pr_state(s))))]).collect(),
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
    [Constraint::Length(1)].into_iter().chain(sections.iter().flat_map(|s| [Constraint::Length(1), Constraint::Length(s.height())])).collect()
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

/// The tmux arguments that open the legend in a popup: sized to the legend (plus the border), at most the client's size.
pub fn popup_args(exe: &Path, client: (u16, u16)) -> Vec<String> {
    let (w, h) = size();
    let command = format!("{} legend", quote(&exe.display().to_string()));
    ["display-popup", "-E", "-T", " legend ", "-w", &(w + POPUP_BORDER).min(client.0).to_string(), "-h", &(h + POPUP_BORDER).min(client.1).to_string(), &command].map(String::from).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, style::Color};

    fn render() -> Buffer {
        let (w, h) = size();
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, f.area())).unwrap();
        terminal.backend().buffer().clone()
    }

    fn row_text(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
    }

    /// The first cell of the first row that contains `needle` (the legend's glyphs are one column wide).
    fn find(buf: &Buffer, needle: &str) -> (u16, u16) {
        find_after(buf, 0, needle)
    }

    /// Like `find`, from row `from` on (a section's title row).
    fn find_after(buf: &Buffer, from: u16, needle: &str) -> (u16, u16) {
        (from..buf.area.height)
            .find_map(|y| row_text(buf, y).find(needle).map(|b| (row_text(buf, y)[..b].chars().count() as u16, y)))
            .unwrap_or_else(|| panic!("{needle:?} is not in the legend"))
    }

    fn fg(buf: &Buffer, at: (u16, u16)) -> Color {
        buf[at].fg
    }

    #[test]
    fn every_meaning_is_listed_with_its_swatch_in_its_colour() {
        let buf = render();
        for m in Meaning::ALL {
            let (x, y) = find(&buf, m.label());
            assert_eq!(buf[(x - 4, y)].symbol(), "█", "{m:?}");
            assert_eq!(fg(&buf, (x - 4, y)), palette::color(m), "{m:?}");
        }
    }

    #[test]
    fn every_job_status_is_a_pill_and_a_counter_from_the_palette() {
        let buf = render();
        for (name, status) in palette::JOB_LEGEND {
            let (icon, m) = palette::job_status(status);
            let (x, y) = find_after(&buf, find(&buf, "Jobs: Plan").1, name);
            let row = row_text(&buf, y);
            let pill_at = row.chars().position(|c| c.to_string() == icon).expect(name) as u16;
            assert!(pill_at > x, "{name}: the pill follows the name");
            assert_eq!(buf[(pill_at, y)].bg, palette::color(m), "{name}: pill");
            assert_eq!(buf[(pill_at, y)].fg, palette::pill(m).fg.unwrap(), "{name}: pill text");
            assert!(row.contains(" key S1 "), "{name}: {row}");
            let counter = row.chars().collect::<Vec<_>>().iter().rposition(|c| *c == '3').expect(name) as u16;
            assert_eq!(fg(&buf, (counter, y)), palette::color(palette::job_status(status).1), "{name}: counter");
        }
        assert!(row_text(&buf, find(&buf, "counters row").1).contains("1⋅1⋅2⋅3⋅0"));
    }

    #[test]
    fn the_context_dots_are_listed_for_every_phase() {
        let buf = render();
        for (name, phase) in palette::PHASE_LEGEND {
            let (x, y) = find_after(&buf, find(&buf, "Context dots").1, &format!(" {name}"));
            assert_eq!((buf[(x - 3, y)].symbol(), buf[(x - 1, y)].symbol()), ("●", "○"), "{name}");
            let want = palette::color(palette::phase(phase));
            assert_eq!((fg(&buf, (x - 3, y)), fg(&buf, (x - 1, y))), (want, want), "{name}");
        }
    }

    #[test]
    fn the_pr_states_and_checks_are_listed_in_their_colours() {
        let buf = render();
        for state in palette::PR_LEGEND {
            let at = find(&buf, &format!("#2 {state}"));
            assert_eq!(fg(&buf, at), palette::color(palette::pr_state(state)), "{state}");
        }
        for (name, c) in palette::CHECKS_LEGEND {
            let (icon, m) = palette::checks(c);
            let (x, y) = find_after(&buf, find(&buf, "PR checks").1, &format!(" {name}"));
            assert_eq!(buf[(x - 1, y)].symbol(), icon, "{name}");
            assert_eq!(fg(&buf, (x - 1, y)), palette::color(m), "{name}");
        }
    }

    #[test]
    fn the_legend_fits_its_own_size() {
        let buf = render();
        assert!(row_text(&buf, 0).contains("any key closes"));
        let (w, h) = size();
        assert_eq!((buf.area.width, buf.area.height), (w, h));
        assert!(row_text(&buf, h - 1).contains("passed"), "the last section is not cut");
    }

    #[test]
    fn the_popup_is_sized_to_the_legend_and_capped_at_the_client() {
        let (w, h) = size();
        let exe = Path::new("/r/tui/target/release/cpd-tui");
        let args = popup_args(exe, (215, 62));
        assert_eq!(args, ["display-popup", "-E", "-T", " legend ", "-w", &(w + 2).to_string(), "-h", &(h + 2).to_string(), "'/r/tui/target/release/cpd-tui' legend"]);
        let small = popup_args(exe, (40, 20));
        assert_eq!((small[5].as_str(), small[7].as_str()), ("40", "20"));
    }
}
