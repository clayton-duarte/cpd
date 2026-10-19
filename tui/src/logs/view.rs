//! The Event log's state and drawing (commands.md §13.2): one line per Event, newest last. Pure: the state takes
//! Events and keys, and [`draw`] only writes to the frame.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use cpd_core::events::Event;
use crate::palette::{self, Meaning};

/// The Events and where the view is: `up` lines above the bottom (0: it shows the bottom, so it follows).
#[derive(Clone, Debug, Default)]
pub struct Log {
    events: Vec<Event>,
    up: usize,
    /// The rows the last [`Log::fit`] gave.
    height: usize,
    /// The Event the view was opened at (`--event`): its line is reversed. Kept after the User scrolls.
    selected: Option<i64>,
    /// The selected Event is to be scrolled into view by the next [`Log::fit`], which knows the height.
    reveal: bool,
}

impl Log {
    pub fn new(events: Vec<Event>) -> Log {
        Log { events, ..Log::default() }
    }

    /// The view opened at the Event `id`: its line is selected and, at the first [`Log::fit`], scrolled to the middle of
    /// the view, which then does not follow until the User scrolls down to the bottom. An `id` that is not among
    /// `events` selects nothing and the view opens at the bottom.
    pub fn at(events: Vec<Event>, id: i64) -> Log {
        let found = events.iter().any(|e| e.id == id);
        Log { events, selected: found.then_some(id), reveal: found, ..Log::default() }
    }

    /// Whether the view shows the bottom, and so follows new Events.
    pub fn follows(&self) -> bool {
        self.up == 0
    }

    /// Appends Events (newest last). A view that was scrolled up stays on the same lines; one at the bottom follows.
    pub fn push(&mut self, events: Vec<Event>) {
        if self.up > 0 {
            self.up += events.len();
        }
        self.events.extend(events);
        self.clamp();
    }

    /// The view is `height` rows tall.
    pub fn fit(&mut self, height: u16) {
        self.height = usize::from(height);
        if let (true, Some(id)) = (std::mem::take(&mut self.reveal), self.selected) {
            if let Some(i) = self.events.iter().position(|e| e.id == id) {
                // the line goes to the middle of the view, which is full when it can be: `end` is one past the last
                // line shown
                let end = (i + 1 + self.height.saturating_sub(1) / 2).max(self.height);
                self.up = self.events.len().saturating_sub(end);
            }
        }
        self.clamp();
    }

    fn clamp(&mut self) {
        self.up = self.up.min(self.events.len().saturating_sub(self.height));
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.up = self.up.saturating_add(lines);
        self.clamp();
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.up = self.up.saturating_sub(lines);
    }

    /// A page: the rows of the view.
    pub fn page(&self) -> usize {
        self.height.max(1)
    }

    /// The Events on screen, oldest first.
    pub fn visible(&self) -> &[Event] {
        let end = self.events.len() - self.up.min(self.events.len());
        &self.events[end.saturating_sub(self.height)..end]
    }
}

fn dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().add_modifier(Modifier::DIM))
}

/// `HH:MM:SS` (UTC) of an RFC 3339 time, as the Events carry it.
fn clock(at: &str) -> &str {
    at.get(11..19).unwrap_or(at)
}

/// The `error` codes of a failure the engine hit for a Lead, whose source is still `watch`: the Lead handles them
/// (contract §2.2: not Inbox items).
const LEAD_CODES: [&str; 2] = ["pr_start_failed", "push_failed"];

/// Whether an `error` Event is addressed to a Lead, who handles it (audit Q5): it names a Plan and its source is not the
/// engine's own `watch` (`pr poll`'s `cleanup_failed`, the drivers' errors), or its code is one of [`LEAD_CODES`]. The
/// engine's own step errors (`source` `watch`, contract §5.2) and those that name no Plan are the User's.
fn addressed_to_lead(e: &Event) -> bool {
    let field = |name: &str| e.data.get(name).and_then(|v| v.as_str());
    e.plan.is_some() && (field("source") != Some("watch") || field("code").is_some_and(|c| LEAD_CODES.contains(&c)))
}

/// The Event's line, as the Dispatch column showed it: its time of day (dim), then its `text`, or its kind when
/// the `text` is empty; an `error` Event in the failed colour, in the waiting colour when it is addressed to a Lead (who
/// handles it). Cut to `width` columns with `…`.
fn line(e: &Event, width: usize) -> Line<'static> {
    let text = if e.text.is_empty() { &e.kind } else { &e.text };
    let style = match (e.kind.as_str(), addressed_to_lead(e)) {
        ("error", true) => palette::fg(Meaning::Waiting),
        ("error", false) => palette::fg(Meaning::Failed),
        _ => Style::new(),
    };
    let room = width.saturating_sub(" 00:00:00 ".len());
    let text = if text.chars().count() <= room {
        text.clone()
    } else {
        text.chars().take(room.saturating_sub(1)).chain(['…']).collect()
    };
    Line::from(vec![" ".into(), dim(clock(&e.at)), " ".into(), Span::styled(text, style)])
}

/// Draws the visible Events in `area`. Call [`Log::fit`] with its height first.
pub fn draw(f: &mut Frame, area: Rect, log: &Log) {
    let lines: Vec<Line<'static>> = if log.events.is_empty() {
        vec![Line::from(dim(" none yet"))]
    } else {
        log.visible().iter().map(|e| line(e, usize::from(area.width))).collect()
    };
    f.render_widget(Paragraph::new(lines), area);
    // the Event the view was opened at is reversed across its whole line
    if let Some(i) = log.visible().iter().position(|e| log.selected == Some(e.id)) {
        let row = Rect::new(area.x, area.y + i as u16, area.width, 1);
        f.buffer_mut().set_style(row.intersection(area), Modifier::REVERSED);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;
    use serde_json::{Value, json};

    use super::*;

    fn event(kind: &str, plan: Option<&str>, data: Value) -> Event {
        Event {
            id: 1,
            at: "2026-01-01T10:20:30.000Z".into(),
            kind: kind.into(),
            context: None,
            plan: plan.map(Into::into),
            job: None,
            attempt: None,
            session: None,
            text: "something happened".into(),
            data,
        }
    }

    fn error(plan: Option<&str>, source: &str, code: &str) -> Event {
        event("error", plan, json!({"source": source, "code": code, "message": "m", "details": {}}))
    }

    /// The colour of the Event's text in its line (the line is ` 10:20:30 something happened`).
    fn colour(e: &Event) -> Option<Color> {
        line(e, 60).spans.last().unwrap().style.fg
    }

    #[test]
    fn an_error_the_engine_reports_for_itself_is_red() {
        let red = Some(palette::color(Meaning::Failed));
        assert_eq!(colour(&error(None, "watch", "tmux_failed")), red, "the machine");
        assert_eq!(colour(&error(Some("pay"), "watch", "store_failed")), red, "a step of the engine, about a Plan");
        assert_eq!(colour(&error(None, "pr poll", "gh_rate_limited")), red, "no Plan to address: the User's");
    }

    #[test]
    fn an_error_addressed_to_a_lead_is_waiting() {
        let waiting = Some(palette::color(Meaning::Waiting));
        assert_eq!(colour(&error(Some("pay"), "attempt run", "git_failed")), waiting, "a driver's error");
        assert_eq!(colour(&error(Some("pay"), "pr poll", "cleanup_failed")), waiting);
        assert_eq!(colour(&error(Some("pay"), "plan lock", "push_failed")), waiting);
        // the engine's own start of a PR (source `watch`) is for the Lead too
        assert_eq!(colour(&error(Some("pay"), "watch", "pr_start_failed")), waiting);
        assert_eq!(colour(&error(Some("pay"), "watch", "push_failed")), waiting);
    }

    #[test]
    fn other_events_have_no_colour() {
        assert_eq!(colour(&event("job-failed", Some("pay"), json!({}))), None);
    }
}
