//! The Dispatch column's drawing (commands.md §13.2, §17): the machine-wide
//! state of the read model's [`Widgets`] in one pane, top to bottom: the engine
//! and the slots in use, the PRs and their feedback, the open Escalations, the
//! latest Events, and a one-row hint. Sections are separated by one dim rule,
//! as in the spike's Dispatch pane. Pure: [`draw`] only writes to the frame.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::plan::Phase;
use crate::tui::palette::{self, Meaning};
use crate::tui::read::{Engine, MachineEscalation, PlanCard, PrState, Slots, Widgets};

/// The rows a separator takes.
const SEPARATOR: u16 = 1;
/// The rows the Events section keeps at least (its title and two Events) before a Plan without a PR is listed.
const EVENTS_MIN: u16 = 3;
pub const HINT: &str = " ? legend   q quit";

fn bold(text: &str) -> Span<'static> {
    Span::styled(text.to_owned(), Style::new().add_modifier(Modifier::BOLD))
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

/// `3s`, `4m`, `2h`: how long ago `at` (RFC 3339) was. `?` when it does not parse (the store writes
/// RFC 3339 only, so this does not happen on a Ledger cpd wrote).
fn age(at: &str, now: OffsetDateTime) -> String {
    let Ok(then) = OffsetDateTime::parse(at, &Rfc3339) else { return "?".into() };
    match (now - then).whole_seconds().max(0) {
        s @ 0..=59 => format!("{s}s"),
        s @ 60..=3599 => format!("{}m", s / 60),
        s => format!("{}h", s / 3600),
    }
}

/// `HH:MM:SS` (UTC) of an RFC 3339 time, as the Events carry it.
fn clock(at: &str) -> &str {
    at.get(11..19).unwrap_or(at)
}

fn plural(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

/// A line of `prefix` spans, then `text` cut to the width that is left.
fn fitted(mut spans: Vec<Span<'static>>, text: &str, style: Style, width: u16) -> Line<'static> {
    let used: usize = spans.iter().map(Span::width).sum();
    spans.push(Span::styled(truncate(text, usize::from(width).saturating_sub(used)), style));
    Line::from(spans)
}

fn engine_lines(engine: &Engine) -> Vec<Line<'static>> {
    let (icon, meaning, text) = match engine {
        Engine::Never => ("○", Meaning::Nothing, "not started yet".to_owned()),
        Engine::Running { pid, since } => ("●", Meaning::InProgress, format!("running  pid {pid}  since {}", clock(since))),
        Engine::Stopped { at } => ("○", Meaning::Nothing, format!("stopped  {}", clock(at))),
        Engine::Crashed { pid, since } => ("✗", Meaning::Failed, format!("crashed  pid {pid} is gone  since {}", clock(since))),
    };
    vec![
        Line::from(bold(" Engine")),
        Line::from(vec![" ".into(), Span::styled(icon, palette::fg(meaning)), " ".into(), Span::styled(text, palette::fg(meaning))]),
    ]
}

fn slot_lines(slots: &Slots, now: OffsetDateTime, width: u16) -> Vec<Line<'static>> {
    let load = match (slots.used(), slots.limit as usize) {
        (0, _) => Meaning::Nothing,
        (used, limit) if used >= limit => Meaning::Review,
        _ => Meaning::InProgress,
    };
    let mut lines = vec![Line::from(vec![bold(" Slots "), Span::styled(format!("{}/{}", slots.used(), slots.limit), palette::fg(load))])];
    if slots.held.is_empty() {
        lines.push(Line::from(dim(" none in use")));
    }
    for held in &slots.held {
        let status = held.status.as_str();
        let tail = format!(" {} {status} {}", held.tier, age(&held.started_at, now));
        let room = usize::from(width).saturating_sub(tail.chars().count() + 1);
        lines.push(Line::from(vec![
            " ".into(),
            truncate(&format!("{}/{}", held.plan, held.attempt), room).into(),
            format!(" {} ", held.tier).into(),
            Span::styled(status.to_owned(), palette::fg(palette::attempt_status(held.status))),
            dim(format!(" {}", age(&held.started_at, now))),
        ]));
    }
    lines
}

/// What the PRs section lists: Plans with a PR, then (when `with_idle`) the Plans in Dispatch without one.
fn pr_lines(plans: &[PlanCard], with_idle: bool, width: u16) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(bold(" PRs"))];
    let label = |card: &PlanCard| format!(" {}", card.head.title);
    for card in plans.iter().filter(|c| c.head.pr.is_some()) {
        let pr = card.head.pr.as_ref().expect("filtered on a PR");
        lines.push(fitted(vec![], &label(card), Style::new().add_modifier(Modifier::BOLD), width));
        let (icon, checks) = palette::checks(pr.checks_summary());
        lines.push(Line::from(vec![
            format!("  #{} ", pr.number).into(),
            Span::styled(
                match pr.state {
                    PrState::Open => "open",
                    PrState::Closed => "closed",
                    PrState::Merged => "merged",
                },
                palette::fg(palette::pr_state(pr.state)),
            ),
            " ".into(),
            Span::styled(icon, palette::fg(checks)),
            format!(" {}  {}", plural(pr.reviews, "review"), plural(pr.comments, "comment")).into(),
        ]));
        if let Some(feedback) = card.feedback.as_ref().filter(|f| !f.failing_checks.is_empty()) {
            lines.push(fitted(
                vec!["  ".into(), Span::styled("✗ ", palette::fg(Meaning::Failed))],
                &feedback.failing_checks.join(", "),
                palette::fg(Meaning::Failed),
                width,
            ));
        }
    }
    if with_idle {
        for card in plans.iter().filter(|c| c.head.pr.is_none() && c.head.phase == Phase::Dispatch) {
            let room = usize::from(width).saturating_sub(" no PR".len() + 2);
            lines.push(Line::from(vec![bold(&truncate(&label(card), room)), dim("  no PR")]));
        }
    }
    if lines.len() == 1 {
        lines.push(Line::from(dim(" none")));
    }
    lines
}

fn escalation_lines(open: &[MachineEscalation], width: u16) -> Vec<Line<'static>> {
    let count = if open.is_empty() { Span::styled("0", palette::fg(Meaning::Nothing)) } else { Span::styled(open.len().to_string(), palette::fg(Meaning::Review)) };
    let mut lines = vec![Line::from(vec![bold(" Escalations "), count])];
    for e in open {
        let who = format!("{}/{}", e.plan, e.job);
        let prefix = vec![" ".into(), Span::styled("● ", palette::fg(Meaning::Review)), who.into(), "  ".into()];
        lines.push(fitted(prefix, &e.escalation.question, Style::new().add_modifier(Modifier::DIM), width));
    }
    if open.is_empty() {
        lines.push(Line::from(dim(" none open")));
    }
    lines
}

/// The latest Events that fit `height` rows under the title, newest last.
fn event_lines(w: &Widgets, height: u16, width: u16) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(bold(" Events"))];
    let rows = usize::from(height).saturating_sub(1);
    if w.events.is_empty() {
        lines.push(Line::from(dim(" none yet")));
    }
    for e in &w.events[w.events.len().saturating_sub(rows)..] {
        let text = if e.text.is_empty() { &e.kind } else { &e.text };
        let style = if e.kind == "error" { palette::fg(Meaning::Failed) } else { Style::new() };
        lines.push(fitted(vec![" ".into(), dim(clock(&e.at)), " ".into()], text, style, width));
    }
    lines
}

fn separator(f: &mut Frame, area: Rect) {
    // `Borders::TOP` of a one-row block is the line itself.
    f.render_widget(Block::new().borders(Borders::TOP).border_style(palette::fg(Meaning::Nothing)), area);
}

/// Draws the column in `area`; `now` is the clock of the ages. `note` (a failure the view reports) takes the
/// hint's row.
pub fn draw(f: &mut Frame, area: Rect, w: &Widgets, now: OffsetDateTime, note: Option<&str>) {
    let width = area.width;
    let mut machine = engine_lines(&w.engine);
    machine.extend(slot_lines(&w.slots, now, width));
    let escalations = escalation_lines(&w.escalations, width);
    // The Plans in Dispatch without a PR are listed only while there is room.
    let fixed = machine.len() + escalations.len() + 4 * usize::from(SEPARATOR) + 1;
    let with_pr = pr_lines(&w.plans, false, width);
    let with_idle = pr_lines(&w.plans, true, width);
    let room = usize::from(area.height).saturating_sub(fixed + usize::from(EVENTS_MIN));
    let prs = if with_idle.len() <= room.max(with_pr.len()) { with_idle } else { with_pr };

    let [machine_area, sep1, prs_area, sep2, esc_area, sep3, events_area, hint] = Layout::vertical([
        Constraint::Length(machine.len() as u16),
        Constraint::Length(SEPARATOR),
        Constraint::Length(prs.len() as u16),
        Constraint::Length(SEPARATOR),
        Constraint::Length(escalations.len() as u16),
        Constraint::Length(SEPARATOR),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    for sep in [sep1, sep2, sep3] {
        separator(f, sep);
    }
    f.render_widget(Paragraph::new(machine), machine_area);
    f.render_widget(Paragraph::new(prs), prs_area);
    f.render_widget(Paragraph::new(escalations), esc_area);
    f.render_widget(Paragraph::new(event_lines(w, events_area.height, width)), events_area);
    let foot = match note {
        Some(note) => Line::from(Span::styled(format!(" {}", truncate(note, usize::from(width).saturating_sub(1))), palette::fg(Meaning::Failed))),
        None => Line::from(dim(HINT)),
    };
    f.render_widget(Paragraph::new(foot), hint);
}
