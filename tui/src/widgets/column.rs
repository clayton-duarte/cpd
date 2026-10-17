//! The Dispatch column's drawing (commands.md §13.2), a port of the reference's `draw_dispatch` and `pr::render`
//! (`tui/src/main.rs`, `tui/src/pr.rs`): the machine, the pull requests of the Plans in the cockpit (one line per PR), git and Jira (the last two
//! still future), under one blank row; each section but the first starts with a header. Pure: [`draw`] only writes to the frame.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::palette::{self, Meaning};
use crate::read::{Place, PlanCard, PrState, PrRow, ReviewDecision, Widgets};

/// Rows of the column besides the PR rows: the blank top row, the machine (two), the three headers each with a blank row
/// above and below, and the two `(future)` rows of git and Jira.
const FIXED_ROWS: u16 = 1 + 2 + 3 * 3 + 2;

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

/// The lines of one Plan: its label, then one line per PR with a pull request, `#n state checks review title` (the title
/// cut first to fit `width`); a Plan without a pull request takes one line.
fn plan_lines(card: &PlanCard, width: usize) -> Vec<Line<'static>> {
    if card.prs.is_empty() {
        return vec![Line::from(vec![
            " ".into(),
            bold(truncate(&card.label, width.saturating_sub(8))),
            dim("  no PR"),
        ])];
    }
    let head = Line::from(vec![" ".into(), bold(truncate(&card.label, width.saturating_sub(1)))]);
    std::iter::once(head).chain(card.prs.iter().map(|pr| pr_line(pr, width))).collect()
}

/// One pull request: `  #n state checks review  title`, or `  #n loading…` before its first poll.
fn pr_line(pr: &PrRow, width: usize) -> Line<'static> {
    let PrRow { number, state, polled, .. } = pr;
    let num = format!("  #{number}");
    let Some(polled) = polled else { return Line::from(vec![dim(num), dim(" loading…")]) };
    let state_text = match state {
        PrState::Open => "open",
        PrState::Closed => "closed",
        PrState::Merged => "merged",
    };
    let review = match polled.decision {
        ReviewDecision::Approved => "approved",
        ReviewDecision::ChangesRequested => "changes requested",
        ReviewDecision::None => "no review",
    };
    let (icon, checks) = palette::checks(polled.checks);
    let used = num.chars().count() + 1 + state_text.chars().count() + 3 + review.chars().count();
    let mut spans = vec![
        num.into(),
        " ".into(),
        Span::styled(state_text, palette::fg(palette::pr_state(*state))),
        " ".into(),
        Span::styled(icon, palette::fg(checks)),
        " ".into(),
        review.into(),
    ];
    let room = width.saturating_sub(used + 2);
    if room >= 4 {
        spans.push(dim(format!("  {}", truncate(&polled.title, room))));
    }
    Line::from(spans)
}

/// A section header, one line: `──`, then ` <title> ` as a grey reversed block (not bold), then `─` to the right edge, all in
/// the Nothing colour. A title wider than `width` is cut; the line never wraps.
fn section_header(title: &str, width: usize) -> Line<'static> {
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

/// Draws the column in `area`: a blank row, the machine, then the PRs, git and Jira, each under its header.
pub fn draw(f: &mut Frame, area: Rect, w: &Widgets, machine: &Machine) {
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
    let mut text = vec![];
    // The Plans in the cockpit: Plans with a pull request first, then those without only while the pane has room for the
    // rest.
    let cockpit: Vec<&PlanCard> = w.plans.iter().filter(|c| matches!(c.lead_place, Place::Chat | Place::Parked)).collect();
    let room = usize::from(area.height).saturating_sub(usize::from(FIXED_ROWS));
    let mut used = 0;
    for card in cockpit.iter().filter(|c| !c.prs.is_empty()).chain(cockpit.iter().filter(|c| c.prs.is_empty())) {
        let lines = plan_lines(card, usize::from(area.width));
        if card.prs.is_empty() && used + lines.len() > room {
            continue;
        }
        used += lines.len();
        text.extend(lines);
    }
    if cockpit.is_empty() {
        text.push(Line::from(dim(" no Plans in the cockpit")));
    }
    f.render_widget(Paragraph::new(text), prs);
    f.render_widget(Paragraph::new(dim(" (future)")), git);
    f.render_widget(Paragraph::new(dim(" (future)")), jira);
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use super::*;
    use crate::read::{Engine, Slots};

    fn buffer(width: u16, height: u16) -> Buffer {
        let w = Widgets {
            slots: Slots { limit: 3, held: vec![] },
            engine: Engine::Never,
            plans: vec![],
            escalations: vec![],
            events: vec![],
            events_cursor: 0,
        };
        let machine = Machine { name: "homeserver".to_owned(), load: ["0.52", "0.41", "0.30"].map(str::to_owned).to_vec() };
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, f.area(), &w, &machine)).unwrap();
        terminal.backend().buffer().clone()
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
            " no Plans in the coc".to_owned(),
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
}
