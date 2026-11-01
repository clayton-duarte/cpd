//! The Event log's state and drawing (commands.md §13.2): one line per Event, newest last. Pure: the state takes
//! Events and keys, and [`draw`] only writes to the frame.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyEvent, MouseEvent};
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use cpd_core::events::Event;
use crate::dialog::scroll::Scroll;
use crate::palette::{self, Meaning};
use crate::box_widget::SectionTitle;

/// The rows above the Events: the title, drawn as a section header (hs-d124, hs-d297).
/// The title's rows: an empty row, the title and an empty row (hs-d308).
const HEADER_ROWS: u16 = SectionTitle::ROWS;

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
    /// The error of the last failed read (one row), shown in the last row until a read succeeds.
    pub error: Option<String>,
    /// The zone the rows draw their time of day in: the caller's (the Ports clock's), read once at the edge.
    zone: cpd_core::clock::TimeZone,
}

impl Log {
    pub fn new(events: Vec<Event>, zone: cpd_core::clock::TimeZone) -> Log {
        Log { events, zone, scroll: Scroll::default(), follow: true, selected: None, reveal: false, error: None }
    }

    /// The view opened at the Event `id`: its line is selected and, at the first [`Log::fit`], scrolled to the middle of
    /// the view, which then does not follow until the User scrolls down to the bottom. An `id` that is not among
    /// `events` selects nothing and the view opens at the bottom.
    pub fn at(events: Vec<Event>, id: i64, zone: cpd_core::clock::TimeZone) -> Log {
        let found = events.iter().any(|e| e.id == id);
        Log { selected: found.then_some(id), reveal: found, ..Log::new(events, zone) }
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

    /// The area is `height` rows tall, the title's row included, and its last row for the error while there is one.
    pub fn fit(&mut self, height: u16) {
        let page = usize::from(height.saturating_sub(HEADER_ROWS + u16::from(self.error.is_some())));
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

    /// A key press: ↑ or k, ↓ or j, PgUp, PgDn scroll; End or G goes to the bottom, which follows (the kit's keys).
    /// Whether the key was one of these.
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

fn muted(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), palette::muted_text())
}

/// The local `HH:MM:SS` of an RFC 3339 time, as the Events carry it.
fn clock(at: &str, zone: &cpd_core::clock::TimeZone) -> String {
    match time::OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339) {
        Ok(t) => cpd_core::clock::time_of_day(t, zone),
        Err(_) => at.to_owned(),
    }
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

/// The zone the tests draw in: `ManualClock`'s (EST5EDT rule), never the machine's.
#[cfg(test)]
pub(crate) fn test_zone() -> cpd_core::clock::TimeZone {
    cpd_core::clock::Clock::zone(&cpd_core::clock::ManualClock::new())
}

/// The Event's line, as the Dispatch column showed it: its time of day (muted), then its `text`, or its kind when
/// the `text` is empty; an `error` Event in the failed colour, in the waiting colour when it is addressed to a Lead (who
/// handles it). Cut to `width` columns with `…`.
pub(crate) fn line(e: &Event, width: usize, zone: &cpd_core::clock::TimeZone) -> Line<'static> {
    let text = if e.text.is_empty() { &e.kind } else { &e.text };
    let style = match (e.kind.as_str(), addressed_to_lead(e)) {
        ("error", true) => palette::fg(Meaning::Waiting),
        ("error", false) => palette::fg(Meaning::Failed),
        _ => palette::read_only_text(),
    };
    let room = width.saturating_sub("00:00:00 ".len());
    let text = crate::cells::truncate(text, room);
    Line::from(vec![muted(clock(&e.at, zone)), " ".into(), Span::styled(text, style)])
}

/// Draws the title, then the visible Events, in `area`, and the error in its last row. Call [`Log::fit`] with its
/// height first.
pub fn draw(f: &mut Frame, area: Rect, log: &Log) {
    let [head, area] = Layout::vertical([Constraint::Length(SectionTitle::ROWS), Constraint::Min(0)]).areas(area);
    f.render_widget(SectionTitle::new("Logs", None), head);
    let outer = area;
    let area = area.inner(Margin::new(1, 0)); // the rows keep one blank column on each side
    let area = match &log.error {
        Some(error) => {
            let [events, foot] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
            f.render_widget(Paragraph::new(Span::styled(error.clone(), palette::error())), foot);
            events
        }
        None => area,
    };
    let lines: Vec<Line<'static>> = if log.events.is_empty() {
        vec![Line::from(muted("none yet"))]
    } else {
        log.visible().iter().map(|e| line(e, usize::from(area.width), &log.zone)).collect()
    };
    f.render_widget(Paragraph::new(lines), area);
    // the Event the view was opened at is the blue block across the popup's whole width, the margins included
    if let Some(i) = log.visible().iter().position(|e| log.selected == Some(e.id)) {
        let row = Rect::new(outer.x, area.y + i as u16, outer.width, 1);
        f.buffer_mut().set_style(row.intersection(outer), palette::selected());
    }
}

/// The layout test's sample (`layout_tests`): the popup drawn as the view draws it, with three Events; its title is rows 0 to 2.
#[cfg(test)]
pub(crate) fn layout_sample(width: u16, height: u16, case: crate::layout_tests::SectionCase) -> crate::layout_tests::Sample {
    use crate::layout_tests::{Kind, Placed, SectionCase, Sample};
    let events = (0..3)
        .map(|id| Event {
            id: id + 1,
            at: format!("2026-01-03T00:00:{id:02}.000Z"),
            kind: "plan-phase".into(),
            context: None,
            plan: None,
            session: None,
            job: None,
            attempt: None,
            text: format!("event {id}"),
            data: serde_json::json!({}),
        })
        .collect();
    let mut log = Log::new(events, test_zone());
    log.fit(height);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| draw(f, f.area(), &log)).unwrap();
    let pane = Rect::new(0, 0, width, height);
    let boxes = match case {
        SectionCase::Title => vec![Placed { name: "logs title", kind: Kind::Rule, rect: Rect::new(0, 0, width, 3), exempt: Vec::new() }],
        SectionCase::Rows => Vec::new(),
    };
    Sample { buf: terminal.backend().buffer().clone(), container: pane, boxes }
}

#[cfg(test)]
#[path = "width_tests.rs"]
mod width_tests;

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyModifiers, MouseEventKind};

    use super::*;
    use crate::dialog::scroll::WHEEL_ROWS;

    fn events(n: i64) -> Vec<Event> {
        (0..n)
            .map(|id| Event {
                id: id + 1,
                at: format!("2026-01-03T00:00:{id:02}.000Z"),
                kind: "plan-phase".into(),
                context: None,
                plan: None,
                session: None,
                job: None,
                attempt: None,
                text: format!("event {id}"),
                data: serde_json::json!({}),
            })
            .collect()
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn wheel(kind: MouseEventKind) -> MouseEvent {
        MouseEvent { kind, column: 2, row: 2, modifiers: KeyModifiers::NONE }
    }

    /// The texts on screen, `height` rows of the area (the title's included).
    fn shown(log: &mut Log, height: u16) -> Vec<String> {
        log.fit(height);
        log.visible().iter().map(|e| e.text.clone()).collect()
    }

    /// tui-G1: the log's keys are the Worker and history views' (the kit's), `End` and `G` included.
    #[test]
    fn end_and_g_go_to_the_bottom_and_follow_again() {
        let mut log = Log::new(events(10), test_zone());
        log.fit(7);
        assert!(log.key(key(KeyCode::Char('k'))));
        assert!(!log.follows());
        assert!(log.key(key(KeyCode::End)));
        assert!(log.follows(), "End follows the bottom again");
        assert_eq!(log.visible().last().unwrap().id, 10);
        assert!(log.key(key(KeyCode::PageUp)));
        assert!(log.key(key(KeyCode::Char('G'))));
        assert!(log.follows(), "so does G");
    }

    /// tui-W4: the error of a failed read takes the last row; the Events keep the rows above it.
    #[test]
    fn the_error_takes_the_last_row_until_it_is_cleared() {
        let mut log = Log::new(events(10), test_zone());
        log.fit(7);
        assert_eq!(log.page(), 4);
        log.error = Some("no such table: events".into());
        log.fit(7);
        assert_eq!(log.page(), 3);
        assert_eq!(log.visible().last().unwrap().id, 10, "still at the bottom");
        log.error = None;
        log.fit(7);
        assert_eq!(log.page(), 4);
    }

    #[test]
    fn keys_and_the_wheel_scroll_and_scrolling_to_the_bottom_follows_again() {
        let mut log = Log::new(events(20), test_zone());
        // 7 rows: the title with its two empty rows and 4 Events
        assert_eq!(shown(&mut log, 7), ["event 16", "event 17", "event 18", "event 19"]);
        assert!(log.follows());
        assert!(log.key(key(KeyCode::Up)) && !log.follows());
        assert_eq!(shown(&mut log, 7)[0], "event 15");
        assert!(log.key(key(KeyCode::Char('k'))));
        assert!(log.key(key(KeyCode::PageUp)));
        assert_eq!(shown(&mut log, 7)[0], "event 10");
        assert!(log.mouse(wheel(MouseEventKind::ScrollUp)));
        assert_eq!(shown(&mut log, 7)[0], format!("event {}", 10 - WHEEL_ROWS));
        assert!(log.mouse(wheel(MouseEventKind::ScrollDown)) && log.mouse(wheel(MouseEventKind::ScrollDown)));
        assert!(!log.follows(), "{:?}", shown(&mut log, 7));
        assert!(log.key(key(KeyCode::PageDown)) && log.key(key(KeyCode::Char('j'))));
        assert!(log.follows());
        assert_eq!(shown(&mut log, 7)[3], "event 19");
        // keys that do not scroll are left to the view
        for code in [KeyCode::Char('q'), KeyCode::Esc, KeyCode::Enter, KeyCode::Char('J')] {
            assert!(!log.key(key(code)), "{code:?}");
        }
        assert!(!log.mouse(wheel(MouseEventKind::Moved)));
    }

    #[test]
    fn the_view_follows_at_the_bottom_and_stays_put_when_scrolled_up() {
        let mut log = Log::new(events(10), test_zone());
        shown(&mut log, 7);
        log.push(events(12)[10..].to_vec());
        assert_eq!(shown(&mut log, 7), ["event 8", "event 9", "event 10", "event 11"]);
        log.mouse(wheel(MouseEventKind::ScrollUp));
        let before = shown(&mut log, 7);
        log.push(events(15)[12..].to_vec());
        assert_eq!(shown(&mut log, 7), before);
        assert!(!log.follows());
        // the wheel back to the bottom (the 3 Events that came are 3 rows more), and it follows again
        log.mouse(wheel(MouseEventKind::ScrollDown));
        assert!(!log.follows());
        log.mouse(wheel(MouseEventKind::ScrollDown));
        assert!(log.follows());
        log.push(events(16)[15..].to_vec());
        assert_eq!(shown(&mut log, 7)[3], "event 15");
    }

    #[test]
    fn a_taller_area_that_shows_everything_follows_and_cannot_scroll() {
        let mut log = Log::new(events(3), test_zone());
        assert_eq!(shown(&mut log, 12).len(), 3);
        assert!(log.follows());
        log.scroll_up(5);
        assert!(log.follows() && log.visible().len() == 3);
    }

    #[test]
    fn the_title_is_a_section_title_between_two_empty_rows_and_the_events_start_under_it() {
        let mut log = Log::at(events(30), 11, test_zone());
        log.fit(8);
        let mut terminal = Terminal::new(TestBackend::new(24, 8)).unwrap();
        terminal.draw(|f| draw(f, f.area(), &log)).unwrap();
        let got = terminal.backend().buffer();
        let row = |y: u16| (0..24).map(|x| got[(x, y)].symbol()).collect::<String>();
        // an empty row, the title, an empty row, then the Events
        assert_eq!(row(0).trim_end(), "");
        assert_eq!(row(1), format!(" ──\u{E0B6}Logs\u{E0B4}{} ", "─".repeat(14)));
        assert_eq!(row(2).trim_end(), "");
        assert!(row(3).starts_with(" 19:00:08 event 8"), "{}", row(3));
        // the selected Event (id 11, `event 10`) is reversed, in the middle of the five rows
        assert!(row(5).contains("event 10") && crate::palette::check::is_selected(&got[(0, 5)]));
        assert!(!crate::palette::check::is_selected(&got[(0, 4)]));
    }

    // ---- the logs popup's title v4 (hs-d475, hs-d476): red until it is `Logs`, a grey pill inside the padding ----

    #[test]
    fn the_title_is_a_grey_title_case_pill_inside_the_padding_of_one_column_each_side() {
        let mut log = Log::at(events(30), 11, test_zone());
        log.fit(8);
        let mut terminal = Terminal::new(TestBackend::new(24, 8)).unwrap();
        terminal.draw(|f| draw(f, f.area(), &log)).unwrap();
        let got = terminal.backend().buffer();
        let row = |y: u16| (0..24).map(|x| got[(x, y)].symbol()).collect::<String>();
        assert_eq!(row(0).trim_end(), "", "an empty row above");
        assert_eq!(row(1), format!(" ──\u{E0B6}Logs\u{E0B4}{} ", "─".repeat(24 - 1 - 2 - 6 - 1)), "column 1 to column 22");
        assert_eq!(row(2).trim_end(), "", "an empty row below");
        crate::section::check::grey_pill(got, 3, 1, "Logs");
    }
}

/// The colour of an `error` line: red when the engine reported it for itself, the waiting colour when it is a Lead's
/// (hs-d303, audit Q5).
#[cfg(test)]
mod error_colour {
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
        line(e, 60, &<cpd_core::clock::ManualClock as cpd_core::clock::Clock>::zone(&cpd_core::clock::ManualClock::new())).spans.last().unwrap().style.fg
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
        // the engine's own start of a Workflow (source `watch`) is for the Lead too
        assert_eq!(colour(&error(Some("pay"), "watch", "pr_start_failed")), waiting);
        assert_eq!(colour(&error(Some("pay"), "watch", "push_failed")), waiting);
    }

    #[test]
    fn other_events_have_no_colour() {
        assert_eq!(colour(&event("job-failed", Some("pay"), json!({}))), palette::read_only_text().fg, "read-only text, colour 7");
    }
}
