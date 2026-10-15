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
}

impl Log {
    pub fn new(events: Vec<Event>) -> Log {
        Log { events, ..Log::default() }
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

/// Draws the visible Events in `area`. Call [`Log::fit`] with its height first.
pub fn draw(f: &mut Frame, area: Rect, log: &Log) {
    let lines: Vec<Line<'static>> = if log.events.is_empty() {
        vec![Line::from(dim(" none yet"))]
    } else {
        log.visible().iter().map(|e| line(e, usize::from(area.width))).collect()
    };
    f.render_widget(Paragraph::new(lines), area);
}
