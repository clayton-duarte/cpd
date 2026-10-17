//! The Dispatch column's drawing (commands.md §13.2), a port of the reference's `draw_dispatch` and `pr::render`
//! (`tui/src/main.rs`, `tui/src/pr.rs`): the machine, the open pull requests (two lines each), git and Jira (the last two
//! still future), with one dim rule between sections. Pure: [`draw`] only writes to the frame.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::palette::{self, Meaning};
use crate::read::{OpenPr, PrState, ReviewDecision, Widgets};

/// The rows a separator takes.
const SEPARATOR: u16 = 1;

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

/// One open pull request, two lines (commands.md §13.2): ` owner/repo#n`, then `  <open|draft> <checks> <review>  <title>`, the
/// title cut first to fit `width`. An `active` one (a PR of the chat slot's Plan) is reversed and bold, as the active tab is.
fn pr_lines(pr: &OpenPr, width: usize) -> [Line<'static>; 2] {
    let name = truncate(&format!(" {}#{}", pr.repo, pr.number), width);
    let state_text = if pr.draft { "draft" } else { "open" };
    let review = match pr.review_decision {
        ReviewDecision::Approved => "approved",
        ReviewDecision::ChangesRequested => "changes requested",
        ReviewDecision::None => "no review",
    };
    let (icon, checks) = palette::checks(pr.checks);
    let used = 2 + state_text.chars().count() + 3 + review.chars().count();
    let mut spans = vec![
        "  ".into(),
        Span::styled(state_text, palette::fg(palette::pr_state(PrState::Open))),
        " ".into(),
        Span::styled(icon, palette::fg(checks)),
        " ".into(),
        review.into(),
    ];
    let room = width.saturating_sub(used + 2);
    if room >= 4 {
        let title = format!("  {}", truncate(&pr.title, room));
        spans.push(if pr.active { Span::raw(title) } else { dim(title) });
    }
    let (first, second) = (Line::from(bold(name)), Line::from(spans));
    if pr.active {
        let active = Style::new().add_modifier(Modifier::REVERSED | Modifier::BOLD);
        return [first.patch_style(active), second.patch_style(active)];
    }
    [first, second]
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
    // Whole pull requests only: what does not fit the section is dropped from the end.
    let room = usize::from(prs.height).saturating_sub(text.len());
    for pr in w.prs.iter().take(room / 2) {
        text.extend(pr_lines(pr, usize::from(area.width)));
    }
    if w.prs.is_empty() {
        text.push(Line::from(dim(" no open PRs")));
    }
    f.render_widget(Paragraph::new(text), prs);
    f.render_widget(Paragraph::new(vec![Line::from(bold(" git")), Line::from(dim(" (future)"))]), git);
    f.render_widget(Paragraph::new(vec![Line::from(bold(" Jira")), Line::from(dim(" (future)"))]), jira);
}
