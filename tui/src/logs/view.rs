//! The Event log's state and drawing (commands.md §13.2): one line per Event, newest last. Pure: the state takes
//! Events and keys, and [`draw`] only writes to the frame.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyEvent, MouseEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use cpd_core::events::Event;
use crate::dialog::scroll::Scroll;
use crate::palette::{self, Meaning};
use crate::widgets::section_header;

/// The rows above the Events: the title, drawn as a section header (hs-d124, hs-d297).
const HEADER_ROWS: u16 = 1;

/// The Events and where the view is. The kit's [`Scroll`] holds the position (an offset from the first Event); `follow`
/// says the view shows the bottom, so a new Event scrolls in. Scrolling decides it again: at the bottom it follows,
/// anywhere else it does not and a new Event leaves the lines on screen as they are.
#[derive(Clone, Debug)]
pub struct Log {
    events: Vec<Event>,
    scroll: Scroll,
    follow: bool,
    /// The Event the view was opened at (`--event`): its line is reversed. Kept after the User scrolls.
    selected: Option<i64>,
    /// The selected Event is to be scrolled into view by the next [`Log::fit`], which knows the height.
    reveal: bool,
}

impl Log {
    pub fn new(events: Vec<Event>) -> Log {
        Log { events, scroll: Scroll::default(), follow: true, selected: None, reveal: false }
    }

    /// The view opened at the Event `id`: its line is selected and, at the first [`Log::fit`], scrolled to the middle of
    /// the view, which then does not follow until the User scrolls down to the bottom. An `id` that is not among
    /// `events` selects nothing and the view opens at the bottom.
    pub fn at(events: Vec<Event>, id: i64) -> Log {
        let found = events.iter().any(|e| e.id == id);
        Log { selected: found.then_some(id), reveal: found, ..Log::new(events) }
    }

    /// Whether the view shows the bottom, and so follows new Events.
    pub fn follows(&self) -> bool {
        self.follow
    }

    fn to_bottom(&mut self) {
        self.scroll.by(isize::try_from(self.scroll.max_offset()).unwrap_or(isize::MAX));
    }

    /// Appends Events (newest last). A view that was scrolled up stays on the same lines; one at the bottom follows.
    pub fn push(&mut self, events: Vec<Event>) {
        self.events.extend(events);
        self.scroll.resize(self.events.len(), self.scroll.page());
        if self.follow {
            self.to_bottom();
        }
    }

    /// The area is `height` rows tall, the title's row included.
    pub fn fit(&mut self, height: u16) {
        let page = usize::from(height.saturating_sub(HEADER_ROWS));
        self.scroll.resize(self.events.len(), page);
        if let (true, Some(id)) = (std::mem::take(&mut self.reveal), self.selected) {
            if let Some(i) = self.events.iter().position(|e| e.id == id) {
                // the line goes to the middle of the view, which is full when it can be: `end` is one past the last
                // line shown
                let end = (i + 1 + page.saturating_sub(1) / 2).max(page);
                let top = isize::try_from(end - page).unwrap_or(isize::MAX);
                self.scroll.by(top - isize::try_from(self.scroll.offset()).unwrap_or(isize::MAX));
                self.follow = false;
            }
        }
        if self.follow {
            self.to_bottom();
        }
        self.settle();
    }

    /// Scrolling decides whether the view follows: it does at the bottom.
    fn settle(&mut self) {
        self.follow = self.scroll.offset() == self.scroll.max_offset();
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll.by(-isize::try_from(lines).unwrap_or(isize::MAX));
        self.settle();
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll.by(isize::try_from(lines).unwrap_or(isize::MAX));
        self.settle();
    }

    /// A key press: ↑ or k, ↓ or j, PgUp, PgDn scroll (the kit's keys). Whether the key was one of these.
    pub fn key(&mut self, key: KeyEvent) -> bool {
        let scrolled = self.scroll.key(key);
        self.settle();
        scrolled
    }

    /// The mouse wheel scrolls (the kit's notch). Whether the event was a wheel notch.
    pub fn mouse(&mut self, mouse: MouseEvent) -> bool {
        let scrolled = self.scroll.mouse(mouse);
        self.settle();
        scrolled
    }

    /// A page: the rows of the view.
    pub fn page(&self) -> usize {
        self.scroll.page().max(1)
    }

    /// The Events on screen, oldest first.
    pub fn visible(&self) -> &[Event] {
        let first = self.scroll.offset().min(self.events.len());
        &self.events[first..(first + self.scroll.page()).min(self.events.len())]
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

/// Draws the title, then the visible Events, in `area`. Call [`Log::fit`] with its height first.
pub fn draw(f: &mut Frame, area: Rect, log: &Log) {
    let [head, area] = Layout::vertical([Constraint::Length(HEADER_ROWS), Constraint::Min(0)]).areas(area);
    f.render_widget(Paragraph::new(section_header("logs", usize::from(head.width))), head);
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
