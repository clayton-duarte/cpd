//! The kit's scrolling (commands.md §13.2, "The dialog kit"): an offset into `total` rows seen through a window of
//! `page` rows. ↑↓, j/k, PgUp/PgDn, End/G and the mouse wheel move it, and it never leaves `0..=max_offset`. It draws
//! nothing: the view reads [`Scroll::offset`] and [`Scroll::page`]. The `text` and `confirm` kinds use it, and so do
//! the logs and the legend popups.

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind,
};
use ratatui::text::{Line, Span};

use crate::cells;

/// Rows a wheel notch moves (the Worker and Plan views' notch).
pub(crate) const WHEEL_ROWS: usize = 3;

/// Which way a scroll cue points: `▲ <n> more` (rows hidden above) or `▼ <n> more` (rows hidden below).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cue {
    Above,
    Below,
}

/// The one scroll indicator of cpd (commands.md §17, "Scroll cues"; hs-d348): `▲ <n> more` / `▼ <n> more`, muted (colour 8),
/// **right-aligned** in a row of `width` cells (blank to its left, so it never cuts a line drawn there). Every region
/// that scrolls draws its cues with this; a cue wider than the row is cut at its right edge.
pub(crate) fn cue(which: Cue, n: usize, width: usize) -> Line<'static> {
    let arrow = match which {
        Cue::Above => "▲",
        Cue::Below => "▼",
    };
    let text: String = cells::take(&format!("{arrow} {n} more"), width).to_string();
    let pad = " ".repeat(width - cells::width(&text));
    Line::from(vec![
        Span::raw(pad),
        Span::styled(text, crate::palette::muted_text()),
    ])
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Scroll {
    offset: usize,
    page: usize,
    total: usize,
}

impl Scroll {
    /// The window now shows `page` of `total` rows (the content or the area changed); the offset is clamped.
    pub(crate) fn resize(&mut self, total: usize, page: usize) {
        self.total = total;
        self.page = page;
        self.offset = self.offset.min(self.max_offset());
    }

    /// The first row shown.
    pub(crate) fn offset(&self) -> usize {
        self.offset
    }

    pub(crate) fn page(&self) -> usize {
        self.page
    }

    pub(crate) fn total(&self) -> usize {
        self.total
    }

    /// The largest offset: the last row reaches the bottom of the window. 0 when everything fits.
    pub(crate) fn max_offset(&self) -> usize {
        self.total.saturating_sub(self.page)
    }

    /// Whether some row is out of the window.
    pub(crate) fn scrollable(&self) -> bool {
        self.total > self.page
    }

    /// Moves the least that shows rows `first..=last` (`first` wins when they do not all fit).
    pub(crate) fn show(&mut self, first: usize, last: usize) {
        if last >= self.offset + self.page {
            self.offset = (last + 1).saturating_sub(self.page);
        }
        if first < self.offset {
            self.offset = first;
        }
        self.offset = self.offset.min(self.max_offset());
    }

    /// Moves by `delta` rows, clamped.
    pub(crate) fn by(&mut self, delta: isize) {
        let moved = if delta < 0 {
            self.offset.saturating_sub(delta.unsigned_abs())
        } else {
            self.offset.saturating_add(delta.unsigned_abs())
        };
        self.offset = moved.min(self.max_offset());
    }

    /// A key press: ↑ or k one row up, ↓ or j one down, PgUp and PgDn a page, End or G to the last page (tui-G1).
    /// Whether the key was one of these.
    pub(crate) fn key(&mut self, key: KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press
            || key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return false;
        }
        let page = isize::try_from(self.page.max(1)).unwrap_or(isize::MAX);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.by(1),
            KeyCode::PageUp => self.by(-page),
            KeyCode::PageDown => self.by(page),
            KeyCode::End | KeyCode::Char('G') => self.by(isize::MAX),
            _ => return false,
        }
        true
    }

    /// The wheel: [`WHEEL_ROWS`] rows a notch. Whether the event was a wheel notch.
    pub(crate) fn mouse(&mut self, mouse: MouseEvent) -> bool {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.by(-(WHEEL_ROWS as isize)),
            MouseEventKind::ScrollDown => self.by(WHEEL_ROWS as isize),
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::MouseButton;

    use super::*;

    fn new(total: usize, page: usize) -> Scroll {
        let mut s = Scroll::default();
        s.resize(total, page);
        s
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn wheel(kind: MouseEventKind) -> MouseEvent {
        MouseEvent {
            kind,
            column: 3,
            row: 3,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn the_offset_is_clamped_to_zero_and_the_last_page() {
        let mut s = new(20, 8);
        assert_eq!((s.offset(), s.max_offset(), s.scrollable()), (0, 12, true));
        s.by(-5);
        assert_eq!(s.offset(), 0);
        s.by(100);
        assert_eq!(s.offset(), 12);
        s.by(-1);
        assert_eq!(s.offset(), 11);
    }

    #[test]
    fn content_that_fits_does_not_scroll() {
        let mut s = new(5, 8);
        assert_eq!((s.max_offset(), s.scrollable()), (0, false));
        s.by(3);
        assert_eq!(s.offset(), 0);
        assert!(new(0, 0).max_offset() == 0);
    }

    #[test]
    fn arrows_and_j_k_move_one_row_and_pages_move_a_page() {
        let mut s = new(40, 10);
        for code in [KeyCode::Down, KeyCode::Char('j')] {
            assert!(s.key(press(code)), "{code:?}");
        }
        assert_eq!(s.offset(), 2);
        for code in [KeyCode::Up, KeyCode::Char('k')] {
            assert!(s.key(press(code)), "{code:?}");
        }
        assert_eq!(s.offset(), 0);
        assert!(s.key(press(KeyCode::PageDown)));
        assert_eq!(s.offset(), 10);
        assert!(s.key(press(KeyCode::PageDown)));
        assert!(s.key(press(KeyCode::PageDown)));
        assert!(s.key(press(KeyCode::PageDown)));
        assert_eq!(s.offset(), 30);
        assert!(s.key(press(KeyCode::PageUp)));
        assert_eq!(s.offset(), 20);
    }

    #[test]
    fn end_and_g_go_to_the_last_page() {
        for code in [KeyCode::End, KeyCode::Char('G')] {
            let mut s = new(40, 10);
            assert!(s.key(press(code)), "{code:?}");
            assert_eq!(s.offset(), 30, "{code:?}");
        }
    }

    #[test]
    fn other_keys_releases_and_modified_keys_are_not_scrolling() {
        let mut s = new(40, 10);
        for code in [
            KeyCode::Enter,
            KeyCode::Char('y'),
            KeyCode::Esc,
            KeyCode::Char('J'),
            KeyCode::Home,
        ] {
            assert!(!s.key(press(code)), "{code:?}");
        }
        assert!(!s.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL)));
        let mut release = press(KeyCode::Down);
        release.kind = KeyEventKind::Release;
        assert!(!s.key(release));
        assert_eq!(s.offset(), 0);
    }

    #[test]
    fn the_wheel_moves_three_rows_a_notch() {
        let mut s = new(40, 10);
        assert!(s.mouse(wheel(MouseEventKind::ScrollDown)));
        assert_eq!(s.offset(), WHEEL_ROWS);
        assert!(s.mouse(wheel(MouseEventKind::ScrollUp)));
        assert!(s.mouse(wheel(MouseEventKind::ScrollUp)));
        assert_eq!(s.offset(), 0);
        assert!(!s.mouse(wheel(MouseEventKind::Down(MouseButton::Left))));
        assert!(!s.mouse(wheel(MouseEventKind::Moved)));
    }

    #[test]
    fn a_resize_clamps_the_offset() {
        let mut s = new(40, 10);
        s.by(100);
        assert_eq!(s.offset(), 30);
        s.resize(40, 25);
        assert_eq!(s.offset(), 15);
        s.resize(10, 25);
        assert_eq!(s.offset(), 0);
        assert_eq!((s.total(), s.page()), (10, 25));
    }

    // ---- the shared cue (commands.md §17, "Scroll cues"; hs-d348): red until `cue` draws it ----

    fn flat(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn a_cue_is_right_aligned_in_its_row_with_the_count() {
        assert_eq!(flat(&cue(Cue::Above, 3, 20)), format!("{:>20}", "▲ 3 more"));
        assert_eq!(
            flat(&cue(Cue::Below, 33, 30)),
            format!("{:>30}", "▼ 33 more")
        );
        assert_eq!(
            flat(&cue(Cue::Below, 1, 8)),
            "▼ 1 more",
            "a row exactly as wide as the cue"
        );
    }

    #[test]
    fn a_cue_is_as_wide_as_its_row_so_a_line_drawn_on_the_left_is_never_cut() {
        for width in [12, 13, 40] {
            let row = flat(&cue(Cue::Above, 7, width));
            assert_eq!(row.chars().count(), width, "{row:?}");
            assert!(row.ends_with("▲ 7 more"), "{row:?}");
            assert!(row.starts_with(' '), "blank on the left: {row:?}");
        }
    }

    #[test]
    fn a_cue_is_muted() {
        let line = cue(Cue::Below, 2, 20);
        let cue_span = line
            .spans
            .iter()
            .find(|s| s.content.contains("more"))
            .expect("the cue's text");
        assert_eq!(cue_span.style, crate::palette::muted_text(), "{cue_span:?}");
    }
}
