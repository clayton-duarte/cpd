//! The drawing and the input of the three kinds (commands.md §13.2, "The dialog kit"), in the `merge` popup's style:
//! the title as the cockpit's section header, the content under it, a blank row, and the buttons on the last row.
//! A [`Dialog`] is pure state: [`Dialog::draw`] fits it to the area it is given, and the key and mouse handlers answer
//! with the [`Outcome`] that closes it, if any.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Alignment, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::Outcome;
use super::scroll::Scroll;
use super::validate::Validator;
use crate::palette;
use crate::section;

/// The label of the second button of `confirm` and `input`.
pub(crate) const CANCEL: &str = "Cancel";
/// The label of the first button of `input`, and of `text`'s only one.
const OK: &str = "OK";
const CLOSE: &str = "Close";
/// Rows an `input` reserves under its field for a refusal.
pub(crate) const REFUSAL_ROWS: usize = 2;
/// The column of the first button, relative to the area, and the gap between buttons (the merge popup's).
const BUTTON_COLUMN: u16 = 2;
const BUTTON_GAP: u16 = 3;

/// What kind of dialog this is, with the state that kind has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// A question, with `[<yes>]` and `[Cancel]`.
    Confirm { yes: String },
    /// A line of text: the prompt above it, `[OK]` and `[Cancel]`. `refused` is why the last accept was refused.
    Input { validator: Validator, field: Field, refused: Option<String> },
    /// Read-only text, with `[Close]`.
    Text,
}

/// The one line of an `input`, with its cursor (a character index).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Field {
    chars: Vec<char>,
    cursor: usize,
}

impl Field {
    /// A field holding `initial`, the cursor after it.
    pub(crate) fn new(initial: &str) -> Field {
        let chars: Vec<char> = initial.chars().filter(|c| !c.is_control()).collect();
        Field { cursor: chars.len(), chars }
    }

    pub(crate) fn text(&self) -> String {
        self.chars.iter().collect()
    }

    /// Whether `key` edited or moved in the field; the cursor stays inside the text.
    fn key(&mut self, key: KeyEvent) -> Option<Edit> {
        let edit = match key.code {
            KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) && !c.is_control() => {
                self.chars.insert(self.cursor, c);
                self.cursor += 1;
                Edit::Text
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.chars.remove(self.cursor);
                Edit::Text
            }
            KeyCode::Delete if self.cursor < self.chars.len() => {
                self.chars.remove(self.cursor);
                Edit::Text
            }
            KeyCode::Left => {
                self.cursor = self.cursor.saturating_sub(1);
                Edit::Move
            }
            KeyCode::Right => {
                self.cursor = (self.cursor + 1).min(self.chars.len());
                Edit::Move
            }
            KeyCode::Home => {
                self.cursor = 0;
                Edit::Move
            }
            KeyCode::End => {
                self.cursor = self.chars.len();
                Edit::Move
            }
            _ => return None,
        };
        Some(edit)
    }

    /// The row of the field in `width` columns: the text around the cursor, the cursor reversed (a space past the end).
    fn line(&self, width: usize) -> Line<'static> {
        let width = width.max(1);
        let start = (self.cursor + 1).saturating_sub(width);
        let shown: Vec<char> = self.chars.iter().copied().skip(start).take(width).collect();
        let at = self.cursor - start;
        let part = |from: usize, to: usize| shown[from.min(shown.len())..to.min(shown.len())].iter().collect::<String>();
        let cursor = shown.get(at).map_or(" ".to_owned(), char::to_string);
        Line::from(vec![
            Span::raw(format!(" {}", part(0, at))),
            Span::styled(cursor, Style::new().add_modifier(Modifier::REVERSED)),
            Span::raw(part(at + 1, shown.len())),
        ])
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Edit {
    Text,
    Move,
}

/// Where the parts of a dialog are in an area.
struct Layout {
    header: Rect,
    body: Rect,
    buttons: Rect,
}

fn layout(area: Rect) -> Layout {
    let row = |y: u16| Rect::new(area.x, area.y.saturating_add(y), area.width, 1).intersection(area);
    Layout {
        header: row(1),
        body: Rect::new(area.x, area.y.saturating_add(3), area.width, area.height.saturating_sub(5)).intersection(area),
        buttons: if area.height >= 2 { row(area.height - 1) } else { Rect::default() },
    }
}

/// `text` as rows of at most `width` columns: a line break stays one, a longer line is cut at its last space within
/// the width, or at the width when it has none. Control characters other than the line break are blanks.
pub(crate) fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    for line in text.lines() {
        let mut rest: Vec<char> = line.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        while rest.len() > width {
            let cut = (0..=width).rev().find(|&i| rest[i] == ' ' && rest[..i].iter().any(|c| *c != ' '));
            let (take, skip) = cut.map_or((width, 0), |i| (i, 1));
            rows.push(rest[..take].iter().collect::<String>().trim_end().to_owned());
            rest.drain(..take + skip);
        }
        rows.push(rest.into_iter().collect());
    }
    rows
}

/// `text` cut to `width` columns, with `…` at the cut.
fn cut(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    text.chars().take(width.saturating_sub(1)).chain(std::iter::once('…')).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Dialog {
    title: String,
    /// The question of `confirm`, the prompt of `input`, the body of `text`.
    text: String,
    mode: Mode,
    scroll: Scroll,
    /// The area of the last draw: where the clicks land.
    area: Rect,
}

impl Dialog {
    pub(crate) fn new(title: String, text: String, mode: Mode) -> Dialog {
        Dialog { title, text, mode, scroll: Scroll::default(), area: Rect::default() }
    }

    /// The buttons' labels, the confirming one first.
    fn labels(&self) -> Vec<String> {
        match &self.mode {
            Mode::Confirm { yes } => vec![format!("[{yes}]"), format!("[{CANCEL}]")],
            Mode::Input { .. } => vec![format!("[{OK}]"), format!("[{CANCEL}]")],
            Mode::Text => vec![format!("[{CLOSE}]")],
        }
    }

    /// The buttons' rects in `area`, in the order of [`Dialog::labels`].
    fn buttons(&self, area: Rect) -> Vec<Rect> {
        let row = layout(area).buttons;
        let mut column = BUTTON_COLUMN;
        self.labels()
            .iter()
            .map(|label| {
                let width = u16::try_from(label.chars().count()).unwrap_or(u16::MAX);
                let rect = Rect::new(area.x.saturating_add(column), row.y, width, 1).intersection(row);
                column = column.saturating_add(width).saturating_add(BUTTON_GAP);
                rect
            })
            .collect()
    }

    /// The rows of the text at the width of `body`.
    fn rows(&self, body: Rect) -> Vec<String> {
        wrap(&self.text, usize::from(body.width).saturating_sub(2))
    }

    /// Fits the scrolling to `area`.
    fn fit(&mut self, area: Rect) {
        self.area = area;
        let body = layout(area).body;
        match self.mode {
            Mode::Input { .. } => self.scroll = Scroll::default(),
            _ => self.scroll.resize(self.rows(body).len(), usize::from(body.height)),
        }
    }

    pub(crate) fn draw(&mut self, f: &mut Frame) {
        let area = f.area();
        self.fit(area);
        let parts = layout(area);
        f.render_widget(Paragraph::new(section::title(&self.title, None, usize::from(area.width))), parts.header);
        let rows = self.rows(parts.body);
        let row = |n: usize| Rect::new(parts.body.x, parts.body.y.saturating_add(u16::try_from(n).unwrap_or(u16::MAX)), parts.body.width, 1).intersection(parts.body);
        let width = usize::from(area.width).saturating_sub(2);
        match &self.mode {
            Mode::Input { field, refused, .. } => {
                for (n, text) in rows.iter().enumerate() {
                    f.render_widget(Paragraph::new(format!(" {text}")), row(n));
                }
                f.render_widget(Paragraph::new(field.line(width)), row(rows.len()));
                if let Some(why) = refused {
                    for (n, text) in wrap(why, width).iter().take(REFUSAL_ROWS).enumerate() {
                        let line = Line::from(Span::styled(format!(" {}", cut(text, width)), palette::note()));
                        f.render_widget(Paragraph::new(line), row(rows.len() + 1 + n));
                    }
                }
            }
            _ => {
                let first = self.scroll.offset();
                for (n, text) in rows.iter().skip(first).take(self.scroll.page()).enumerate() {
                    f.render_widget(Paragraph::new(format!(" {text}")), row(n));
                }
            }
        }
        for (n, (label, rect)) in self.labels().iter().zip(self.buttons(area)).enumerate() {
            let style = if n == 0 { Style::new().add_modifier(Modifier::REVERSED) } else { Style::new() };
            f.render_widget(Paragraph::new(label.as_str()).style(style), rect);
        }
        if self.scroll.scrollable() {
            let last = (self.scroll.offset() + self.scroll.page()).min(self.scroll.total());
            let position = format!("{}-{}/{} ", self.scroll.offset() + 1, last, self.scroll.total());
            let taken = self.buttons(area).last().map_or(0, |r| r.x + r.width - area.x);
            if usize::from(taken) + position.chars().count() < usize::from(area.width) {
                let dim = Paragraph::new(position).alignment(Alignment::Right).style(Style::new().add_modifier(Modifier::DIM));
                f.render_widget(dim, parts.buttons);
            }
        }
    }

    /// What the dialog's first button does: confirms, or for an `input` accepts the text if it passes.
    fn confirm(&mut self) -> Option<Outcome> {
        match &mut self.mode {
            Mode::Input { validator, field, refused } => match validator.check(&field.text()) {
                Ok(value) => Some(Outcome::value(value)),
                Err(why) => {
                    *refused = Some(why);
                    None
                }
            },
            _ => Some(Outcome::confirmed()),
        }
    }

    /// The outcome of a key press, if it closes the dialog. Scrolling and editing change the state.
    pub(crate) fn press_key(&mut self, key: KeyEvent) -> Option<Outcome> {
        if key.kind != KeyEventKind::Press {
            return None;
        }
        if let Mode::Input { field, refused, .. } = &mut self.mode {
            return match key.code {
                KeyCode::Esc => Some(Outcome::cancelled()),
                KeyCode::Enter => self.confirm(),
                _ => {
                    if field.key(key) == Some(Edit::Text) {
                        *refused = None;
                    }
                    None
                }
            };
        }
        if key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
            return None;
        }
        match key.code {
            KeyCode::Enter | KeyCode::Char('y') => Some(Outcome::confirmed()),
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => Some(Outcome::cancelled()),
            _ => {
                self.scroll.key(key);
                None
            }
        }
    }

    /// The outcome of a mouse event, if it closes the dialog: a left click on a button. The wheel scrolls.
    pub(crate) fn press_mouse(&mut self, mouse: MouseEvent) -> Option<Outcome> {
        if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
            self.scroll.mouse(mouse);
            return None;
        }
        let at = Position::new(mouse.column, mouse.row);
        match self.buttons(self.area).iter().position(|b| b.contains(at))? {
            0 => self.confirm(),
            _ => Some(Outcome::cancelled()),
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn click(column: u16, row: u16) -> MouseEvent {
        MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE }
    }

    fn wheel(kind: MouseEventKind) -> MouseEvent {
        MouseEvent { kind, column: 2, row: 2, modifiers: KeyModifiers::NONE }
    }

    fn confirm() -> Dialog {
        Dialog::new("Archive".into(), "Archive cpd-ui?".into(), Mode::Confirm { yes: "Archive".into() })
    }

    fn input(initial: &str) -> Dialog {
        let mode = Mode::Input { validator: Validator::PlanId, field: Field::new(initial), refused: None };
        Dialog::new("New Plan".into(), "Fork cpd-ui into a new Plan, name:".into(), mode)
    }

    fn text(lines: usize) -> Dialog {
        let body = (1..=lines).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n");
        Dialog::new("Legend".into(), body, Mode::Text)
    }

    fn render(d: &mut Dialog, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| d.draw(f)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn rows(buf: &Buffer) -> Vec<String> {
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_owned())
            .collect()
    }

    fn type_str(d: &mut Dialog, s: &str) {
        for c in s.chars() {
            assert_eq!(d.press_key(press(KeyCode::Char(c))), None);
        }
    }

    #[test]
    fn every_kind_draws_its_title_as_a_section_title_between_two_empty_rows() {
        for mut d in [confirm(), input(""), text(3)] {
            let buf = render(&mut d, 40, 10);
            let mut expected = Terminal::new(TestBackend::new(40, 1)).unwrap();
            expected.draw(|f| f.render_widget(Paragraph::new(section::title(&d.title, None, 40)), f.area())).unwrap();
            for x in 0..40 {
                assert_eq!(buf[(x, 1)], expected.backend().buffer()[(x, 0)], "{} column {x}", d.title);
            }
            let rows = rows(&buf);
            assert_eq!((rows[0].as_str(), rows[2].as_str()), ("", ""), "{}", d.title);
        }
        let rows = rows(&render(&mut confirm(), 20, 8));
        assert_eq!(rows[1], format!(" ╭─{}Archive{}{}", "\u{E0B6}", "\u{E0B4}", "─".repeat(7)));
    }

    #[test]
    fn confirm_draws_the_question_and_both_buttons() {
        let buf = rows(&render(&mut confirm(), 40, 7));
        assert_eq!(buf[1..], [format!(" ╭─\u{E0B6}Archive\u{E0B4}{}", "─".repeat(27)).as_str(), "", " Archive cpd-ui?", "", "", "  [Archive]   [Cancel]"]);
    }

    #[test]
    fn a_long_question_wraps_at_the_width() {
        let mut d = Dialog::new("T".into(), "aaaa bbbb cccc dddd".into(), Mode::Confirm { yes: "Yes".into() });
        let buf = rows(&render(&mut d, 12, 10));
        assert_eq!(buf[3..6], [" aaaa bbbb", " cccc dddd", ""]);
    }

    #[test]
    fn wrap_breaks_at_spaces_cuts_long_words_and_keeps_line_breaks() {
        assert_eq!(wrap("aaa bbb ccc", 7), ["aaa bbb", "ccc"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("a\n\nb\n", 5), ["a", "", "b"]);
        assert_eq!(wrap("", 5), Vec::<String>::new());
        assert_eq!(wrap("tab\there", 20), ["tab here"]);
        assert_eq!(wrap("  ab cd ef", 6), ["  ab", "cd ef"]);
    }

    #[test]
    fn enter_or_y_confirms_and_esc_n_or_q_cancels() {
        for code in [KeyCode::Enter, KeyCode::Char('y')] {
            assert_eq!(confirm().press_key(press(code)), Some(Outcome::confirmed()), "{code:?}");
            assert_eq!(text(3).press_key(press(code)), Some(Outcome::confirmed()), "{code:?}");
        }
        for code in [KeyCode::Esc, KeyCode::Char('n'), KeyCode::Char('q')] {
            assert_eq!(confirm().press_key(press(code)), Some(Outcome::cancelled()), "{code:?}");
        }
        for code in [KeyCode::Char('Y'), KeyCode::Char(' '), KeyCode::Tab] {
            assert_eq!(confirm().press_key(press(code)), None, "{code:?}");
        }
        assert_eq!(confirm().press_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL)), None);
        let mut release = press(KeyCode::Enter);
        release.kind = KeyEventKind::Release;
        assert_eq!(confirm().press_key(release), None);
    }

    #[test]
    fn a_click_on_a_button_acts_and_nothing_else_does() {
        let mut d = confirm();
        render(&mut d, 40, 7);
        let [yes, cancel] = d.buttons(Rect::new(0, 0, 40, 7))[..] else { panic!() };
        assert_eq!((yes, cancel), (Rect::new(2, 6, 9, 1), Rect::new(14, 6, 8, 1)));
        assert_eq!(d.press_mouse(click(2, 6)), Some(Outcome::confirmed()));
        assert_eq!(d.press_mouse(click(10, 6)), Some(Outcome::confirmed()));
        assert_eq!(d.press_mouse(click(14, 6)), Some(Outcome::cancelled()));
        assert_eq!(d.press_mouse(click(21, 6)), Some(Outcome::cancelled()));
        for (column, row) in [(1, 6), (11, 6), (13, 6), (22, 6), (2, 5), (2, 0)] {
            assert_eq!(d.press_mouse(click(column, row)), None, "{column},{row}");
        }
        let right = MouseEvent { kind: MouseEventKind::Down(MouseButton::Right), ..click(2, 6) };
        assert_eq!(d.press_mouse(right), None);
        // the buttons are where they are drawn
        let buf = render(&mut d, 40, 7);
        assert_eq!(buf[(yes.x, yes.y)].symbol(), "[");
        assert_eq!(buf[(cancel.x + cancel.width - 1, cancel.y)].symbol(), "]");
    }

    #[test]
    fn text_has_one_button_and_scrolls_with_keys_and_the_wheel() {
        let mut d = text(20);
        let buf = rows(&render(&mut d, 30, 10));
        // an empty row, the title, an empty row, 5 body rows, blank, buttons
        assert_eq!(buf[3..8], [" line 1", " line 2", " line 3", " line 4", " line 5"]);
        assert!(buf[9].starts_with("  [Close]"), "{:?}", buf[9]);
        assert!(buf[9].ends_with("1-5/20"), "{:?}", buf[9]);
        assert_eq!(d.press_key(press(KeyCode::Down)), None);
        assert_eq!(rows(&render(&mut d, 30, 10))[3], " line 2");
        assert_eq!(d.press_mouse(wheel(MouseEventKind::ScrollDown)), None);
        assert_eq!(rows(&render(&mut d, 30, 10))[3], " line 5");
        d.press_key(press(KeyCode::PageDown));
        assert_eq!(rows(&render(&mut d, 30, 10))[3], " line 10");
        d.press_mouse(wheel(MouseEventKind::ScrollUp));
        d.press_key(press(KeyCode::Char('k')));
        assert_eq!(rows(&render(&mut d, 30, 10))[3], " line 6");
        d.press_key(press(KeyCode::Char('j')));
        d.press_key(press(KeyCode::PageUp));
        assert_eq!(rows(&render(&mut d, 30, 10))[3], " line 2");
        for _ in 0..10 {
            d.press_mouse(wheel(MouseEventKind::ScrollDown));
        }
        let end = rows(&render(&mut d, 30, 10));
        assert_eq!(end[3..8], [" line 16", " line 17", " line 18", " line 19", " line 20"]);
        assert!(end[9].ends_with("16-20/20"), "{:?}", end[9]);
    }

    #[test]
    fn text_that_fits_shows_no_position_and_does_not_scroll() {
        let mut d = text(3);
        let buf = rows(&render(&mut d, 30, 10));
        assert_eq!(buf[9], "  [Close]");
        d.press_mouse(wheel(MouseEventKind::ScrollDown));
        assert_eq!(rows(&render(&mut d, 30, 10))[3], " line 1");
    }

    #[test]
    fn a_confirm_taller_than_its_area_scrolls_too() {
        let mut d = Dialog::new("T".into(), "a\nb\nc\nd\ne".into(), Mode::Confirm { yes: "Yes".into() });
        assert_eq!(rows(&render(&mut d, 30, 8))[3..6], [" a", " b", " c"]);
        d.press_key(press(KeyCode::Down));
        assert_eq!(rows(&render(&mut d, 30, 8))[3..6], [" b", " c", " d"]);
    }

    #[test]
    fn input_draws_the_prompt_the_field_with_its_cursor_and_the_buttons() {
        let mut d = input("abc");
        let buf = render(&mut d, 44, 9);
        assert_eq!(
            rows(&buf)[3..],
            [" Fork cpd-ui into a new Plan, name:", " abc", "", "", "", "  [OK]   [Cancel]"][..]
        );
        // the cursor is the reversed cell after the text
        assert!(buf[(4, 4)].modifier.contains(Modifier::REVERSED));
        assert!(!buf[(3, 4)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn input_types_edits_and_moves_the_cursor() {
        let mut d = input("");
        type_str(&mut d, "yqn");
        d.press_key(press(KeyCode::Left));
        d.press_key(press(KeyCode::Backspace));
        type_str(&mut d, "x");
        d.press_key(press(KeyCode::Home));
        d.press_key(press(KeyCode::Delete));
        type_str(&mut d, "a");
        d.press_key(press(KeyCode::End));
        type_str(&mut d, "z");
        let Mode::Input { field, .. } = &d.mode else { panic!() };
        assert_eq!(field.text(), "axnz");
        // control keys are not text
        assert_eq!(d.press_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)), None);
        let Mode::Input { field, .. } = &d.mode else { panic!() };
        assert_eq!(field.text(), "axnz");
    }

    #[test]
    fn y_n_and_q_are_text_in_an_input_and_do_not_close_it() {
        let mut d = input("");
        type_str(&mut d, "ynq");
        let Mode::Input { field, .. } = &d.mode else { panic!() };
        assert_eq!(field.text(), "ynq");
    }

    #[test]
    fn input_enter_accepts_a_valid_value_trimmed_and_esc_cancels() {
        let mut d = input("");
        type_str(&mut d, "  pay-4521 ");
        assert_eq!(d.press_key(press(KeyCode::Enter)), Some(Outcome::value("pay-4521".into())));
        assert_eq!(input("abc").press_key(press(KeyCode::Esc)), Some(Outcome::cancelled()));
    }

    #[test]
    fn input_refuses_a_bad_value_inside_the_popup_and_the_user_keeps_typing() {
        let mut d = input("");
        type_str(&mut d, "Bad");
        assert_eq!(d.press_key(press(KeyCode::Enter)), None);
        let buf = rows(&render(&mut d, 60, 11));
        assert!(buf[5].starts_with(" Invalid Plan id"), "{buf:?}");
        assert_eq!(buf[4], " Bad");
        // typing clears the refusal, and a fixed value is accepted
        for _ in 0..3 {
            d.press_key(press(KeyCode::Backspace));
        }
        assert!(rows(&render(&mut d, 60, 11))[5].is_empty());
        type_str(&mut d, "good");
        assert_eq!(d.press_key(press(KeyCode::Enter)), Some(Outcome::value("good".into())));
    }

    #[test]
    fn an_empty_input_is_refused() {
        let mut d = input("   ");
        assert_eq!(d.press_key(press(KeyCode::Enter)), None);
        assert!(matches!(&d.mode, Mode::Input { refused: Some(_), .. }));
    }

    #[test]
    fn the_buttons_of_an_input_accept_and_cancel() {
        let mut d = input("ok-id");
        render(&mut d, 44, 9);
        assert_eq!(d.press_mouse(click(3, 8)), Some(Outcome::value("ok-id".into())));
        assert_eq!(d.press_mouse(click(11, 8)), Some(Outcome::cancelled()));
        let mut bad = input("Bad");
        render(&mut bad, 44, 9);
        assert_eq!(bad.press_mouse(click(3, 8)), None);
        assert!(matches!(&bad.mode, Mode::Input { refused: Some(_), .. }));
    }

    #[test]
    fn a_field_longer_than_the_width_shows_the_end_where_the_cursor_is() {
        let mut d = input(&"x".repeat(60));
        type_str(&mut d, "END");
        let buf = rows(&render(&mut d, 20, 10));
        assert_eq!(buf[5], format!(" {}END", "x".repeat(14)));
        d.press_key(press(KeyCode::Home));
        assert_eq!(rows(&render(&mut d, 20, 10))[5], format!(" {}", "x".repeat(18)));
    }

    #[test]
    fn a_dialog_in_a_tiny_area_draws_without_panicking() {
        for (w, h) in [(0, 0), (1, 1), (10, 2), (3, 3), (58, 3), (5, 40)] {
            for mut d in [confirm(), input("abc"), text(20)] {
                render(&mut d, w, h);
                d.press_mouse(click(0, 0));
                d.press_key(press(KeyCode::Down));
            }
            let mut d = input("Bad");
            d.press_key(press(KeyCode::Enter));
            render(&mut d, w, h);
        }
    }

    // ---- the dialog kit's title v4 (hs-d475, hs-d476): red until it is a grey pill inside the padding ----

    #[test]
    fn every_kind_draws_its_title_as_a_grey_pill_from_column_1_to_one_column_before_the_edge() {
        for (mut d, title) in [(confirm(), "Archive"), (input(""), "New Plan"), (text(3), "Legend")] {
            let buf = render(&mut d, 40, 10);
            let width = title.chars().count();
            let row: String = (0..40).map(|x| buf[(x, 1)].symbol()).collect();
            assert_eq!(row, format!(" ╭─\u{E0B6}{title}\u{E0B4}{} ", "─".repeat(40 - 1 - 2 - (width + 2) - 1)), "{title}");
            crate::section::check::grey_pill(&buf, 3, 1, title);
        }
    }

    #[test]
    fn a_click_on_the_title_row_does_nothing() {
        for mut d in [confirm(), input(""), text(3)] {
            render(&mut d, 40, 10);
            for column in [0, 1, 3, 6, 20, 38, 39] {
                assert_eq!(d.press_mouse(click(column, 1)), None, "{} column {column}", d.title);
            }
        }
    }
}
