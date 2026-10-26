//! The cockpit's one OSC 8 helper (commands.md §13.2): [`link`] makes cells a
//! view has already drawn a hyperlink, without changing the visible text or
//! any cell's column. Plan inbox's cockpit PR reuses it.
//!
//! # Technique
//!
//! ratatui counts a symbol's width from its characters, so a symbol that
//! carries `ESC ] 8 ; ; <url> BEL` would be taken for a much wider cell and
//! the backend's diff would misplace what follows (ratatui issue #902). The
//! link is therefore written as pieces of at most 2 cells. Each piece is one
//! cell whose symbol is `ESC]8;;<url>BEL` + the piece's text + `ESC]8;;BEL`,
//! and whose [`CellDiffOption::ForcedWidth`] (ratatui 0.30) is the piece's
//! real width. The diff then skips the piece's other cells, and the next
//! cell is drawn at its own column. Verified against the crossterm backend's
//! bytes by the tests below. The cells of a piece beyond the first keep their
//! symbols; the diff never writes them.
//!
//! A piece never joins two cells of different style, because only the first
//! cell's style is written.

use ratatui::buffer::{Buffer, CellDiffOption, CellWidth};
use ratatui::layout::{Position, Rect};
use ratatui::text::Line;
use ratatui::widgets::Widget;

use cpd_core::error::CpdError;
use cpd_core::host::runner::Runner;

/// The link cue, drawn where a view links to a page (hs-d191, hs-d235): `nf-md-share_circle` (U+F11AD).
pub const CUE: &str = "\u{F11AD}";

/// Makes the cells of `area` (one row, `area.y`) an OSC 8 link to `url`, in pieces of at most 2 cells.
///
/// Call it after drawing the text. Cells outside the buffer are skipped. A `url` that is empty, or holds a control
/// character or a non-ASCII byte, is drawn without a link.
pub fn link(buf: &mut Buffer, area: Rect, url: &str) {
    if url.is_empty() || !url.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        return;
    }
    let area = area.intersection(buf.area);
    if area.height == 0 {
        return;
    }
    let (y, end) = (area.y, area.right());
    let mut x = area.x;
    while x < end {
        let first = buf[(x, y)].clone();
        let mut width = first.cell_width().max(1);
        let mut text = first.symbol().to_string();
        if width < 2 && x + width < end {
            let next = &buf[(x + width, y)];
            if next.cell_width() == 1 && next.style() == first.style() {
                text.push_str(next.symbol());
                width += 1;
            }
        }
        let width = width.min(end - x);
        let cell = &mut buf[(x, y)];
        cell.set_symbol(&format!("\x1b]8;;{url}\x07{text}\x1b]8;;\x07"));
        cell.set_diff_option(CellDiffOption::ForcedWidth(
            width.try_into().expect("a piece is at least 1 cell wide"),
        ));
        x += width;
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io::{self, Write};
    use std::rc::Rc;

    use ratatui::backend::CrosstermBackend;
    use ratatui::style::{Modifier, Style};
    use ratatui::text::Span;
    use ratatui::{Terminal, TerminalOptions, Viewport};

    use super::*;

    const URL: &str = "https://example.com/pull/7";
    const OPEN: &str = "\x1b]8;;https://example.com/pull/7\x07";
    const CLOSE: &str = "\x1b]8;;\x07";

    /// A writer the test keeps a handle on, since the backend's own `writer()` is unstable.
    #[derive(Clone, Default)]
    struct Sink(Rc<RefCell<Vec<u8>>>);

    impl Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct Term {
        terminal: Terminal<CrosstermBackend<Sink>>,
        sink: Sink,
    }

    fn terminal(width: u16) -> Term {
        let sink = Sink::default();
        let terminal = Terminal::with_options(
            CrosstermBackend::new(sink.clone()),
            TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, width, 1)) },
        )
        .unwrap();
        Term { terminal, sink }
    }

    /// Draws `text`, links `link_area`; returns the bytes this draw wrote.
    fn draw(terminal: &mut Term, text: &str, link_area: Rect, url: &str) -> Vec<u8> {
        let before = terminal.sink.0.borrow().len();
        terminal
            .terminal
            .draw(|frame| {
                let buf = frame.buffer_mut();
                buf.set_string(0, 0, text, Style::default());
                link(buf, link_area, url);
            })
            .unwrap();
        terminal.sink.0.borrow()[before..].to_vec()
    }

    /// The text the cue tests draw: the cue in the third cell.
    fn cued() -> String {
        format!("ab{CUE}cd")
    }

    /// A minimal terminal: replays `bytes` into one row of `width` columns.
    /// Handles CUP (`ESC[r;cH`), ignores other CSI and all OSC sequences; a
    /// printed symbol advances the cursor by its `unicode-width`, as the tests' symbols have.
    fn replay(bytes: &[u8], width: usize) -> String {
        let text = std::str::from_utf8(bytes).unwrap();
        let mut row = vec![" ".to_string(); width];
        let mut col = 0;
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            match c {
                '\x1b' => match chars.next() {
                    Some('[') => {
                        let mut params = String::new();
                        let last = loop {
                            let c = chars.next().unwrap();
                            if c.is_ascii_alphabetic() {
                                break c;
                            }
                            params.push(c);
                        };
                        if last == 'H' {
                            col = params.split(';').nth(1).unwrap().parse::<usize>().unwrap() - 1;
                        }
                    }
                    Some(']') => while chars.next() != Some('\x07') {},
                    other => panic!("unexpected escape {other:?}"),
                },
                c => {
                    row[col] = c.to_string();
                    let cells = Span::raw(c.to_string()).width();
                    col += cells;
                    if col <= width && cells == 2 {
                        row[col - 1] = String::new(); // the wide symbol's second column
                    }
                }
            }
        }
        row.concat()
    }

    fn position(haystack: &[u8], needle: &str) -> usize {
        haystack
            .windows(needle.len())
            .position(|w| w == needle.as_bytes())
            .unwrap_or_else(|| panic!("{needle:?} not in {:?}", String::from_utf8_lossy(haystack)))
    }

    fn count(haystack: &[u8], needle: &str) -> usize {
        haystack.windows(needle.len()).filter(|w| *w == needle.as_bytes()).count()
    }

    #[test]
    fn the_cue_is_one_cell() {
        assert_eq!(Span::raw(CUE).width(), 1);
    }

    #[test]
    fn one_cell_link_wraps_the_arrow_and_moves_nothing() {
        let mut terminal = terminal(5);
        let bytes = draw(&mut terminal, &cued(), Rect::new(2, 0, 1, 1), URL);

        let link = position(&bytes, &format!("{OPEN}{CUE}{CLOSE}"));
        assert!(position(&bytes, "ab") < link);
        assert!(link < position(&bytes, "c"), "c is written after the link closes");
        assert!(position(&bytes, "c") < position(&bytes, "d"));
        assert_eq!(count(&bytes, "\x1b]8;;"), 2, "one open, one close");
        assert_eq!(replay(&bytes, 5), cued());
    }

    #[test]
    fn five_cell_link_is_three_pieces_and_moves_nothing() {
        let mut terminal = terminal(7);
        let bytes = draw(&mut terminal, "x12345y", Rect::new(1, 0, 5, 1), URL);

        let mut at = position(&bytes, "x");
        for piece in ["12", "34", "5"] {
            let found = position(&bytes[at..], &format!("{OPEN}{piece}{CLOSE}"));
            at += found + 1;
        }
        assert_eq!(count(&bytes, OPEN), 3);
        assert_eq!(count(&bytes, CLOSE), 3);
        assert!(at < bytes.len() && position(&bytes[at..], "y") > 0);
        assert_eq!(replay(&bytes, 7), "x12345y");
    }

    #[test]
    fn a_wide_symbol_is_one_piece_and_the_next_cell_keeps_its_column() {
        let mut terminal = terminal(5);
        let bytes = draw(&mut terminal, "a世bc", Rect::new(1, 0, 2, 1), URL);

        assert_eq!(count(&bytes, OPEN), 1);
        position(&bytes, &format!("{OPEN}世{CLOSE}"));
        assert_eq!(replay(&bytes, 5), "a世bc");
    }

    #[test]
    fn a_piece_never_joins_cells_of_different_style() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 2, 1));
        buf.set_string(0, 0, "a", Style::default());
        buf.set_string(1, 0, "b", Style::default().add_modifier(Modifier::BOLD));
        let area = buf.area;
        link(&mut buf, area, URL);

        assert_eq!(buf[(0, 0)].symbol(), format!("{OPEN}a{CLOSE}"));
        assert_eq!(buf[(1, 0)].symbol(), format!("{OPEN}b{CLOSE}"));
    }

    #[test]
    fn redraw_without_change_writes_nothing_for_the_link() {
        let mut terminal = terminal(5);
        draw(&mut terminal, &cued(), Rect::new(2, 0, 1, 1), URL);
        let again = draw(&mut terminal, &cued(), Rect::new(2, 0, 1, 1), URL);
        let text = String::from_utf8(again).unwrap();
        assert!(!text.contains("]8;;"), "{text:?}");
        assert!(!text.contains(CUE) && !text.contains('c') && !text.contains('d'), "{text:?}");

        // A longer link, too.
        let mut terminal = self::terminal(7);
        draw(&mut terminal, "x12345y", Rect::new(1, 0, 5, 1), URL);
        let again = draw(&mut terminal, "x12345y", Rect::new(1, 0, 5, 1), URL);
        let text = String::from_utf8(again).unwrap();
        assert!(!text.contains("]8;;") && !text.contains('1') && !text.contains('y'), "{text:?}");
    }

    #[test]
    fn a_url_with_a_control_character_or_non_ascii_byte_is_not_linked() {
        for url in ["https://x/\x1b[31m", "https://x/\x07", "https://x/\n", "https://x/é", "https://x/\x7f", ""] {
            let mut terminal = terminal(5);
            let bytes = draw(&mut terminal, &cued(), Rect::new(2, 0, 1, 1), url);
            let text = String::from_utf8(bytes.clone()).unwrap();
            assert!(!text.contains("]8;;"), "{url:?}: {text:?}");
            assert_eq!(replay(&bytes, 5), cued(), "{url:?}");
        }
    }

    #[test]
    fn cells_outside_the_buffer_are_skipped() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        buf.set_string(0, 0, "abc", Style::default());
        link(&mut buf, Rect::new(2, 0, 4, 1), URL);
        link(&mut buf, Rect::new(5, 0, 2, 1), URL);
        link(&mut buf, Rect::new(0, 3, 2, 1), URL);

        assert_eq!(buf[(0, 0)].symbol(), "a");
        assert_eq!(buf[(1, 0)].symbol(), "b");
        assert_eq!(buf[(2, 0)].symbol(), format!("{OPEN}c{CLOSE}"));
    }
}

/// A cockpit link: the cue `CUE`, or `<text> CUE`, the whole output the click target (pro-d60).
pub struct Link<'a> {
    #[allow(dead_code)]
    url: &'a str,
    #[allow(dead_code)]
    text: Option<Line<'a>>,
}

impl<'a> Link<'a> {
    /// The cue only.
    pub fn new(url: &'a str) -> Link<'a> {
        Link { url, text: None }
    }
    /// `<text> CUE`, the text underlined.
    pub fn text(self, text: impl Into<Line<'a>>) -> Link<'a> {
        Link { text: Some(text.into()), ..self }
    }
    /// Cells of the whole output: 1, or the text's width + 2.
    pub fn width(&self) -> u16 {
        todo!("Link::width")
    }
}

impl Widget for Link<'_> {
    fn render(self, _area: Rect, _buf: &mut Buffer) {
        todo!("Link::render")
    }
}

/// The (rect, url) of every link drawn.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Links {
    drawn: Vec<(Rect, String)>,
}

impl Links {
    /// Renders `link` into `area` and records the rect it drew with the url.
    pub fn draw(&mut self, _link: Link<'_>, _area: Rect, _buf: &mut Buffer) {
        let _ = &self.drawn;
        todo!("Links::draw")
    }
    /// The url of the link drawn on that cell.
    pub fn at(&self, _at: Position) -> Option<&str> {
        todo!("Links::at")
    }
}

/// Opens `url` in the default browser (macOS `open`) through `runner`.
pub fn open_url(_runner: &dyn Runner, _url: &str) -> Result<(), CpdError> {
    todo!("open_url")
}
