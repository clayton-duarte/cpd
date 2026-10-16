//! The Dispatch column's drawing (commands.md §13.2), a port of the reference's `draw_dispatch` and `pr::render`
//! (`tui/src/main.rs`, `tui/src/pr.rs`): the machine, the pull requests of the Plans in the cockpit (one line per PR), git and Jira (the last two
//! still future), with one dim rule between sections. Pure: [`draw`] only writes to the frame.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::palette::{self, Meaning};
use crate::read::{Place, PlanCard, PrState, PrRow, ReviewDecision, Widgets};

/// The rows a separator takes.
const SEPARATOR: u16 = 1;
/// Rows of the sections other than the PRs: the machine, git and Jira take two each; three separators lie between the four.
const FIXED_ROWS: u16 = 2 + 2 + 2 + 3 * SEPARATOR;

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

fn separator(f: &mut Frame, area: Rect) {
    // `Borders::TOP` of a one-row block is the line itself.
    f.render_widget(Block::new().borders(Borders::TOP).border_style(palette::fg(Meaning::Nothing)), area);
}

/// Draws the column in `area`: the machine, the PRs, git and Jira, with the shared separator between each pair.
pub fn draw(f: &mut Frame, area: Rect, w: &Widgets, machine: &Machine) {
    let [home, sep1, prs, sep2, git, sep3, jira] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(SEPARATOR),
        Constraint::Min(1),
        Constraint::Length(SEPARATOR),
        Constraint::Length(2),
        Constraint::Length(SEPARATOR),
        Constraint::Length(2),
    ])
    .areas(area);
    for sep in [sep1, sep2, sep3] {
        separator(f, sep);
    }
    f.render_widget(
        Paragraph::new(vec![Line::from(bold(format!(" {}", machine.name))), Line::from(format!(" load {}", machine.load.join(" ")))]),
        home,
    );
    let mut text = vec![Line::from(bold(" PRs"))];
    // The Plans in the cockpit: Plans with a pull request first, then those without only while the pane has room for the
    // rest.
    let cockpit: Vec<&PlanCard> = w.plans.iter().filter(|c| matches!(c.lead_place, Place::Chat | Place::Parked)).collect();
    let room = usize::from(area.height).saturating_sub(usize::from(FIXED_ROWS) + text.len());
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
    f.render_widget(Paragraph::new(vec![Line::from(bold(" git")), Line::from(dim(" (future)"))]), git);
    f.render_widget(Paragraph::new(vec![Line::from(bold(" Jira")), Line::from(dim(" (future)"))]), jira);
}
