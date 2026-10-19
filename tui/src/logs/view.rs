//! The Event log's state and drawing (commands.md §13.2): one line per Event, newest last. Pure: the state takes
//! Events and keys, and [`draw`] only writes to the frame.

use ratatui::Frame;
use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use cpd_core::events::Event;
use crate::palette::{self, Meaning};
use crate::scroll::Scroll;

/// The Events and where the view is (a [`Scroll`] over one row per Event: it follows the bottom until scrolled up).
#[derive(Clone, Debug, Default)]
pub struct Log {
    events: Vec<Event>,
    scroll: Scroll,
    /// The error of the last failed read (one row), shown in the last row until a read succeeds.
    pub error: Option<String>,
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
        self.scroll.follows()
    }

    /// Appends Events (newest last). A view that was scrolled up stays on the same lines; one at the bottom follows.
    pub fn push(&mut self, events: Vec<Event>) {
        self.events.extend(events);
        self.scroll.fit(self.events.len(), self.scroll.height());
    }

    /// The view is `height` rows tall, its last row for the error while there is one.
    pub fn fit(&mut self, height: u16) {
        let height = usize::from(height.saturating_sub(u16::from(self.error.is_some())));
        self.scroll.fit(self.events.len(), height);
        if let (true, Some(id)) = (std::mem::take(&mut self.reveal), self.selected) {
            if let Some(i) = self.events.iter().position(|e| e.id == id) {
                // the line goes to the middle of the view, which is full when it can be: `end` is one past the last
                // line shown
                let end = (i + 1 + height.saturating_sub(1) / 2).max(height);
                self.scroll.to(end.saturating_sub(height));
            }
        }
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll.by(-(lines.min(isize::MAX as usize) as isize));
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll.by(lines.min(isize::MAX as usize) as isize);
    }

    /// A key of the view: the scrolling keys of [`Scroll::key`].
    pub fn key(&mut self, code: KeyCode) {
        self.scroll.key(code);
    }

    /// A page: the rows of the view.
    pub fn page(&self) -> usize {
        self.scroll.page()
    }

    /// The Events on screen, oldest first.
    pub fn visible(&self) -> &[Event] {
        let top = self.scroll.top();
        &self.events[top..(top + self.scroll.height()).min(self.events.len())]
    }
}

fn dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().add_modifier(Modifier::DIM))
}

/// `HH:MM:SS` (UTC) of an RFC 3339 time, as the Events carry it.
fn clock(at: &str) -> &str {
    at.get(11..19).unwrap_or(at)
}

/// The Event's line, as the Dispatch column showed it: its time of day (dim), then its `text`, or its kind when
/// the `text` is empty; an `error` Event in the failed colour. Cut to `width` columns with `…`.
fn line(e: &Event, width: usize) -> Line<'static> {
    let text = if e.text.is_empty() { &e.kind } else { &e.text };
    let style = if e.kind == "error" { palette::fg(Meaning::Failed) } else { Style::new() };
    let room = width.saturating_sub(" 00:00:00 ".len());
    let text = if text.chars().count() <= room {
        text.clone()
    } else {
        text.chars().take(room.saturating_sub(1)).chain(['…']).collect()
    };
    Line::from(vec![" ".into(), dim(clock(&e.at)), " ".into(), Span::styled(text, style)])
}

/// Draws the visible Events in `area`, and the error in its last row. Call [`Log::fit`] with its height first.
pub fn draw(f: &mut Frame, area: Rect, log: &Log) {
    let area = match &log.error {
        Some(error) => {
            let [events, foot] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
            f.render_widget(Paragraph::new(Span::styled(format!(" {error}"), palette::note())), foot);
            events
        }
        None => area,
    };
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
    use super::*;

    fn event(n: usize) -> Event {
        Event {
            id: n as i64,
            at: "2026-01-01T00:00:00.000Z".into(),
            kind: "note".into(),
            context: None,
            plan: None,
            job: None,
            attempt: None,
            session: None,
            text: format!("event {n}"),
            data: serde_json::Value::Null,
        }
    }

    /// tui-G1: the log's keys are the Worker and history views' (`Scroll::key`), `End` and `G` included.
    #[test]
    fn the_log_scrolls_with_the_keys_of_the_other_views_and_end_follows_the_bottom() {
        let mut log = Log::new((0..10).map(event).collect());
        log.fit(4);
        log.key(KeyCode::Char('k'));
        assert!(!log.follows());
        assert_eq!(log.visible()[3].id, 8);
        log.key(KeyCode::End);
        assert!(log.follows(), "End follows the bottom again");
        assert_eq!(log.visible()[3].id, 9);
        log.key(KeyCode::PageUp);
        log.key(KeyCode::Char('G'));
        assert!(log.follows(), "so does G");
    }
}
