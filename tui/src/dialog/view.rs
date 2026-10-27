//! The drawing and the input of the four kinds (commands.md §13.2, "The dialog kit"; hs-d578): the popup is one rounded
//! frame with the title as a pill on its top border, a blank row, the body inside one column of padding, a blank row, the
//! pill buttons and the closing border.
//! A [`Dialog`] is pure state: [`Dialog::draw`] fits it to the area it is given, and the key and mouse handlers answer
//! with the [`Outcome`] that closes it, if any.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use super::Outcome;
use super::scroll::Scroll;
use super::validate::Validator;
use crate::box_widget::{BoxWidget, Walls, geometry, spacing};
use crate::cells;
use crate::input_box;
use crate::palette;
use crate::pill;

/// The label of the second button of `confirm` and `input` (the checklist's is [`LATER`]).
pub(crate) const CANCEL: &str = "Cancel";
/// The label of the first button of `input`, and of `text`'s only one.
const OK: &str = "OK";
const CLOSE: &str = "Close";
/// Rows an `input` reserves under its field for a refusal.
pub(crate) const REFUSAL_ROWS: usize = 2;

/// What kind of dialog this is, with the state that kind has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    /// A question, with the `<yes>` and `Cancel` buttons.
    Confirm { yes: String },
    /// A line of text: the prompt above it, the `OK` and `Cancel` buttons. `refused` is why the last accept was refused.
    Input { validator: Validator, field: Field, refused: Option<String> },
    /// Read-only text, with the `Close` button.
    Text,
    /// Entries `[ ] <id> — <detail>` with the record's full text under each, the User ticks, with the `Approve` and `Later` buttons
    /// (hs-d549, hs-d551, hs-d569 CD2, CD3). `cursor` is an entry.
    Checklist { rows: Vec<Row>, ticked: Vec<bool>, cursor: usize },
    /// A radio list: the prompt, then one row per option, with the `OK` and `Cancel` buttons. `cursor` is an option.
    Select { options: Vec<String>, cursor: usize },
    /// A text box labelled `input_label`, then a radio list labelled `select_label`, with the `OK` and `Cancel` buttons
    /// (pro-d114). `list` is whether the radio list has the focus (else the box); `refused` is why the last accept was refused.
    Form { input_label: String, select_label: String, options: Vec<String>, cursor: usize, field: Field, list: bool, refused: Option<String> },
}

/// One row of a `checklist`: the `--items` file is a JSON array of these.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub(crate) id: String,
    pub(crate) text: String,
    pub(crate) detail: String,
}

impl Row {
    /// The header line as drawn, with the box of `ticked`, with no control character.
    pub(crate) fn header(&self, ticked: bool) -> String {
        let line = format!("[{}] {} — {}", if ticked { 'x' } else { ' ' }, self.id, self.detail);
        line.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
    }
}

/// The label of the second button of `checklist`: nothing is decided, every entry stays pending.
pub(crate) const LATER: &str = "Later";
/// The columns the text of an entry is indented by.
pub(crate) const TEXT_INDENT: usize = 4;

/// One drawn line of a checklist: the entry it belongs to (none for the empty row between two), and whether it is the header.
#[derive(Clone, Debug, PartialEq, Eq)]
struct EntryLine {
    entry: Option<usize>,
    header: bool,
    text: String,
}

/// The label of the confirming button of `checklist`.
pub(crate) const APPROVE: &str = "Approve";

/// The one line of an `input`, with its cursor (a character index).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
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

    /// The characters held and the cursor (a character index).
    pub(crate) fn parts(&self) -> (&[char], usize) {
        (&self.chars, self.cursor)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Edit {
    Text,
    Move,
}

/// Where the parts of a dialog are, inside its frame.
struct Parts {
    /// The body, rows 2 to H-4, inside the padding: content columns 2 to W-3.
    body: Rect,
    /// The inside of the walls, one row of the body: the padding cells included (the refusals use it).
    inside: Rect,
    /// The buttons row, row H-2, inside the padding.
    buttons: Rect,
}

/// The parts of a dialog whose frame's inner rect is `inner`: `[blank, body rows, blank, buttons]`.
fn parts(inner: Rect) -> Parts {
    if inner == Rect::ZERO {
        return Parts { body: Rect::ZERO, inside: Rect::ZERO, buttons: Rect::ZERO };
    }
    let body = Rect::new(inner.x, inner.y + 1, inner.width, inner.height.saturating_sub(3)).intersection(inner);
    Parts {
        body,
        inside: Rect::new(inner.x - spacing::PAD_X, body.y, inner.width + 2 * spacing::PAD_X, body.height),
        buttons: Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
    }
}

/// The parts of a dialog drawn as a popup of `area`.
fn parts_in(area: Rect) -> Parts {
    parts(geometry(&Walls::Full, area).inner)
}

/// A button as a pill: the caps around the label.
fn pill(label: &str, style: Style, rect: Rect, buf: &mut Buffer) {
    let line = Line::from(pill::pill(label.to_owned(), style));
    Paragraph::new(line).render(rect, buf);
}

/// `text` as rows of at most `width` columns: a line break stays one, a longer line is cut at its last space within
/// the width, or at the width when it has none. Control characters other than the line break are blanks.
pub(crate) fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    for line in text.lines() {
        let clean: String = line.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        let mut rest = clean.as_str();
        while cells::width(rest) > width {
            let fits = fitting(rest, width);
            let after = &rest[fits.len()..];
            let split = fits.char_indices().rev().find(|&(i, c)| c == ' ' && fits[..i].chars().any(|c| c != ' '));
            let (head, tail) = if after.starts_with(' ') && fits.chars().any(|c| c != ' ') {
                (fits, &after[1..])
            } else if let Some((i, _)) = split {
                (&fits[..i], &rest[i + 1..])
            } else {
                (fits, after)
            };
            rows.push(head.trim_end().to_owned());
            rest = tail;
        }
        rows.push(rest.to_owned());
    }
    rows
}

/// The longest prefix of `rest` within `width` columns, at least one character, so wrapping always advances.
fn fitting(rest: &str, width: usize) -> &str {
    let fits = cells::take(rest, width);
    if !fits.is_empty() {
        return fits;
    }
    &rest[..rest.chars().next().map_or(0, char::len_utf8)]
}

/// The label of an input's box: its prompt without trailing whitespace and one trailing `:`.
pub(super) fn label(prompt: &str) -> String {
    let t = prompt.trim_end();
    t.strip_suffix(':').unwrap_or(t).trim_end().to_owned()
}

/// `text` cut to `width` columns, with `…` at the cut.
fn cut(text: &str, width: usize) -> String {
    cells::truncate(text, width)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dialog {
    title: String,
    /// The question of `confirm`, the prompt of `input`, the body of `text`.
    text: String,
    mode: Mode,
    scroll: Scroll,
    /// The area of the last draw: where the clicks land.
    area: Rect,
}

impl Dialog {
    pub fn new(title: String, text: String, mode: Mode) -> Dialog {
        Dialog { title, text, mode, scroll: Scroll::default(), area: Rect::default() }
    }

    /// A `form`: a text box labelled `input_label` and a radio list of `options` labelled `select_label`.
    pub(crate) fn form(title: String, input_label: String, select_label: String, options: Vec<String>) -> Dialog {
        let mode = Mode::Form { input_label, select_label, options, cursor: 0, field: Field::new(""), list: false, refused: None };
        Dialog::new(title, String::new(), mode)
    }

    /// The buttons' labels, the confirming one first.
    fn labels(&self) -> Vec<String> {
        match &self.mode {
            Mode::Confirm { yes } => vec![yes.clone(), CANCEL.to_owned()],
            Mode::Input { .. } => vec![OK.to_owned(), CANCEL.to_owned()],
            Mode::Text => vec![CLOSE.to_owned()],
            Mode::Checklist { .. } => vec![APPROVE.to_owned(), LATER.to_owned()],
            Mode::Select { .. } | Mode::Form { .. } => vec![OK.to_owned(), CANCEL.to_owned()],
        }
    }

    /// The ids ticked and the ids not, in order, of a `checklist`.
    pub(crate) fn ticks(&self) -> Option<(Vec<String>, Vec<String>)> {
        let Mode::Checklist { rows, ticked, .. } = &self.mode else { return None };
        let (yes, no): (Vec<_>, Vec<_>) = rows.iter().zip(ticked).partition(|(_, t)| **t);
        let ids = |v: Vec<(&Row, &bool)>| v.into_iter().map(|(r, _)| r.id.clone()).collect();
        Some((ids(yes), ids(no)))
    }

    /// The lines of a `checklist` in `width` text columns: per entry its header, its text wrapped and indented, and an
    /// empty row between two entries.
    fn entry_lines(&self, width: usize) -> Vec<EntryLine> {
        let Mode::Checklist { rows, ticked, .. } = &self.mode else { return Vec::new() };
        let mut lines = Vec::new();
        for (n, (row, t)) in rows.iter().zip(ticked).enumerate() {
            if n > 0 {
                lines.push(EntryLine { entry: None, header: false, text: String::new() });
            }
            lines.push(EntryLine { entry: Some(n), header: true, text: cut(&row.header(*t), width) });
            for text in wrap(&row.text, width.saturating_sub(TEXT_INDENT)) {
                lines.push(EntryLine { entry: Some(n), header: false, text: format!("{}{text}", " ".repeat(TEXT_INDENT)) });
            }
        }
        lines
    }

    /// The prompt's rows of a `select` at `width` columns.
    fn prompt_rows(&self, width: usize) -> Vec<String> {
        wrap(&self.text, width)
    }

    /// The rows of a `select`: the prompt, then `(x) <option>` for the cursor row and `( ) <option>` for the others.
    fn select_rows(&self, width: usize) -> Vec<String> {
        let Mode::Select { options, cursor } = &self.mode else { return Vec::new() };
        let mut rows = self.prompt_rows(width);
        rows.extend(options.iter().enumerate().map(|(n, o)| cut(&format!("({}) {o}", if n == *cursor { 'x' } else { ' ' }), width)));
        rows
    }

    /// Keeps the cursor row of a `checklist` or a `select` inside the scrolled window.
    fn follow_cursor(&mut self) {
        let width = usize::from(parts_in(self.area).body.width);
        let cursor = match &self.mode {
            Mode::Checklist { cursor, .. } => self.entry_lines(width).iter().position(|l| l.header && l.entry == Some(*cursor)).unwrap_or(0),
            Mode::Select { cursor, .. } => self.prompt_rows(width).len() + cursor,
            _ => return,
        };
        let (offset, page) = (self.scroll.offset(), self.scroll.page().max(1));
        if cursor < offset {
            self.scroll.by(-((offset - cursor) as isize));
        } else if cursor >= offset + page {
            self.scroll.by((cursor + 1 - offset - page) as isize);
        }
    }

    /// The buttons' rects in `area`, in the order of [`Dialog::labels`]: the first pill flush left (its left cap at column 2),
    /// the other flush right (its right cap at column W-3). The whole pill, caps included, is the hit target. Only the
    /// buttons drawn whole are returned: when the two do not fit with one blank cell between, the accepting one alone (A13).
    pub(crate) fn buttons(&self, area: Rect) -> Vec<Rect> {
        let row = parts_in(area).buttons;
        let widths: Vec<u16> = self.labels().iter().map(|l| u16::try_from(cells::width(l)).unwrap_or(u16::MAX).saturating_add(pill::CAPS_COLUMNS)).collect();
        let fits = |n: usize| {
            let need = widths[..n].iter().map(|w| u32::from(*w)).sum::<u32>() + n.saturating_sub(1) as u32;
            u32::from(row.width) >= need
        };
        let drawn = if fits(widths.len()) { widths.len() } else { usize::from(fits(1)) };
        widths[..drawn]
            .iter()
            .enumerate()
            .map(|(n, width)| Rect::new(if n == 0 { row.x } else { row.right() - width }, row.y, *width, 1))
            .collect()
    }

    /// The rows of the text at the width of `body`.
    fn rows(&self, body: Rect) -> Vec<String> {
        if let Mode::Checklist { .. } = &self.mode {
            return self.entry_lines(usize::from(body.width)).into_iter().map(|l| l.text).collect();
        }
        if let Mode::Select { .. } = &self.mode {
            return self.select_rows(usize::from(body.width));
        }
        wrap(&self.text, usize::from(body.width))
    }

    /// Fits the scrolling to `area`.
    fn fit(&mut self, area: Rect) {
        self.area = area;
        let body = parts_in(area).body;
        match self.mode {
            Mode::Input { .. } | Mode::Form { .. } => self.scroll = Scroll::default(),
            _ => self.scroll.resize(self.rows(body).len(), usize::from(body.height)),
        }
    }

    pub(crate) fn draw(&mut self, f: &mut Frame) {
        let area = f.area();
        self.draw_in(f, area);
    }

    /// Draws the dialog as a popup of `area`.
    pub(crate) fn draw_in(&mut self, f: &mut Frame, area: Rect) {
        self.fit(area);
        let text: String = cells::take(&self.title, usize::from(area.width).saturating_sub(6)).to_owned();
        let title = Line::from(pill::pill(text.clone(), palette::pill_static()));
        let frame = BoxWidget::new(Walls::Full, palette::muted_text(), Body { dialog: self });
        f.render_widget(if text.is_empty() { frame } else { frame.title(title) }, area);
    }

    /// The body of the frame: the kind's content, then the buttons.
    fn draw_body(&self, inner: Rect, buf: &mut Buffer) {
        let parts = parts(inner);
        let area = self.area;
        let rows = self.rows(parts.body);
        let row_in = |rect: Rect, n: usize| Rect::new(rect.x, rect.y.saturating_add(u16::try_from(n).unwrap_or(u16::MAX)), rect.width, 1).intersection(rect);
        let (row, wide) = (|n: usize| row_in(parts.body, n), |n: usize| row_in(parts.inside, n));
        let content = usize::from(parts.body.width);
        match &self.mode {
            Mode::Input { field, refused, .. } => {
                let (chars, cursor) = field.parts();
                let box_width = usize::from(parts.body.width);
                let avail = usize::from(parts.body.height).saturating_sub(2 + REFUSAL_ROWS).max(1);
                let tw = input_box::text_width(box_width);
                let (cursor_row, _) = input_box::cursor_at(chars, cursor, tw);
                let start = (cursor_row + 1).saturating_sub(avail);
                let shown = avail.min(input_box::wrapped_rows(chars, cursor, tw));
                let height = u16::try_from(shown + 2).unwrap_or(u16::MAX);
                let wall = Rect::new(parts.body.x, parts.body.y, parts.body.width, height).intersection(parts.body);
                input_box::boxed(&label(&self.text), chars, cursor, true, start, box_width).render(wall, buf);
                if let Some(why) = refused {
                    for (n, text) in wrap(why, content).iter().take(REFUSAL_ROWS).enumerate() {
                        let line = Line::from(Span::styled(format!(" {}", cut(text, content)), palette::note()));
                        Paragraph::new(line).render(wide(usize::from(height) + n), buf);
                    }
                }
            }
            Mode::Form { input_label, select_label, options, cursor, field, list, refused } => {
                let (chars, at) = field.parts();
                let box_width = usize::from(parts.body.width);
                let (start, shown) = form_box(chars, at, box_width, parts.body.height, options.len());
                let height = u16::try_from(shown + 2).unwrap_or(u16::MAX);
                let wall = Rect::new(parts.body.x, parts.body.y, parts.body.width, height).intersection(parts.body);
                input_box::boxed(&label(input_label), chars, at, !*list, start, box_width).render(wall, buf);
                if let Some(why) = refused {
                    for (n, text) in wrap(why, content).iter().take(REFUSAL_ROWS).enumerate() {
                        let line = Line::from(Span::styled(format!(" {}", cut(text, content)), palette::note()));
                        Paragraph::new(line).render(wide(usize::from(height) + n), buf);
                    }
                }
                let list_at = usize::from(height) + REFUSAL_ROWS;
                Paragraph::new(cut(select_label, content)).style(palette::read_only_text()).render(row(list_at), buf);
                for (n, o) in options.iter().enumerate() {
                    let style = if n == *cursor { palette::selected() } else { palette::read_only_text() };
                    let text = cut(&format!("({}) {o}", if n == *cursor { 'x' } else { ' ' }), content);
                    Paragraph::new(text).style(style).render(row(list_at + 1 + n), buf);
                }
            }
            Mode::Checklist { cursor, .. } => {
                let first = self.scroll.offset();
                for (n, line) in self.entry_lines(content).iter().skip(first).take(self.scroll.page()).enumerate() {
                    let style = match (line.header, line.entry == Some(*cursor)) {
                        (true, true) => palette::selected(),
                        (true, false) => palette::clickable_text(),
                        (false, _) => palette::read_only_text(),
                    };
                    Paragraph::new(line.text.as_str()).style(style).render(row(n), buf);
                }
            }
            Mode::Select { cursor, .. } => {
                let first = self.scroll.offset();
                let chosen = self.prompt_rows(content).len() + cursor;
                for (n, text) in rows.iter().enumerate().skip(first).take(self.scroll.page()) {
                    let style = if n == chosen { palette::selected() } else { palette::read_only_text() };
                    Paragraph::new(text.as_str()).style(style).render(row(n - first), buf);
                }
            }
            _ => {
                let first = self.scroll.offset();
                for (n, text) in rows.iter().skip(first).take(self.scroll.page()).enumerate() {
                    Paragraph::new(text.as_str()).style(palette::read_only_text()).render(row(n), buf);
                }
            }
        }
        let buttons = self.buttons(area);
        for (n, (label, rect)) in self.labels().iter().zip(&buttons).enumerate() {
            let style = if n == 0 { palette::pill_selected() } else { palette::pill_interactive() };
            pill(label, style, *rect, buf);
        }
        if self.scroll.scrollable() && !buttons.is_empty() {
            let last = (self.scroll.offset() + self.scroll.page()).min(self.scroll.total());
            let position = format!("{}-{}/{}", self.scroll.offset() + 1, last, self.scroll.total());
            // right-aligned: its last character one blank before the right pill's left cap, or at column W-3 with no right pill
            let end = buttons.get(1).map_or(parts.buttons.right(), |r| r.x.saturating_sub(1));
            let left_pill_end = buttons[0].right();
            let len = u16::try_from(cells::width(&position)).unwrap_or(u16::MAX);
            if end >= left_pill_end.saturating_add(len).saturating_add(1) {
                let rect = Rect::new(end - len, parts.buttons.y, len, 1);
                Paragraph::new(position).style(palette::muted_text()).render(rect, buf);
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
            Mode::Select { options, cursor } => Some(Outcome::value(options[*cursor].clone())),
            Mode::Form { options, cursor, field, refused, .. } => match Validator::Title.check(&field.text()) {
                Ok(value) => Some(Outcome::form(value, options[*cursor].clone())),
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
        if let Mode::Form { options, cursor, field, list, refused, .. } = &mut self.mode {
            match (key.code, *list) {
                (KeyCode::Esc, _) => return Some(Outcome::cancelled()),
                (KeyCode::Enter, _) => return self.confirm(),
                (KeyCode::Tab, _) => *list = !*list,
                (KeyCode::Char('q'), true) => return Some(Outcome::cancelled()),
                (KeyCode::Up | KeyCode::Char('k'), true) => *cursor = cursor.saturating_sub(1),
                (KeyCode::Down | KeyCode::Char('j'), true) => *cursor = (*cursor + 1).min(options.len() - 1),
                (_, true) => {}
                (_, false) => {
                    if field.key(key) == Some(Edit::Text) {
                        *refused = None;
                    }
                }
            }
            return None;
        }
        if key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
            return None;
        }
        if let Mode::Checklist { rows, ticked, cursor } = &mut self.mode {
            let last = rows.len().saturating_sub(1);
            match key.code {
                KeyCode::Enter => return Some(Outcome::confirmed()),
                KeyCode::Esc | KeyCode::Char('q') => return Some(Outcome::cancelled()),
                KeyCode::Char(' ') => {
                    if let Some(t) = ticked.get_mut(*cursor) {
                        *t = !*t;
                    }
                }
                KeyCode::Char('a') => {
                    let all = ticked.iter().all(|t| *t);
                    ticked.iter_mut().for_each(|t| *t = !all);
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    *cursor = cursor.saturating_sub(1);
                    self.follow_cursor();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    *cursor = (*cursor + 1).min(last);
                    self.follow_cursor();
                }
                _ => {
                    self.scroll.key(key);
                }
            }
            return None;
        }
        if let Mode::Select { options, cursor } = &mut self.mode {
            match key.code {
                KeyCode::Enter | KeyCode::Char('y') => return Some(Outcome::value(options[*cursor].clone())),
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => return Some(Outcome::cancelled()),
                KeyCode::Up | KeyCode::Char('k') => {
                    *cursor = cursor.saturating_sub(1);
                    self.follow_cursor();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    *cursor = (*cursor + 1).min(options.len() - 1);
                    self.follow_cursor();
                }
                _ => {
                    self.scroll.key(key);
                }
            }
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
        let body = parts_in(self.area).body;
        if matches!(self.mode, Mode::Checklist { .. }) && body.contains(at) {
            let n = self.scroll.offset() + usize::from(at.y - body.y);
            let entry = self.entry_lines(usize::from(body.width)).get(n).and_then(|l| l.entry);
            if let (Some(entry), Mode::Checklist { ticked, cursor, .. }) = (entry, &mut self.mode) {
                ticked[entry] = !ticked[entry];
                *cursor = entry;
            }
            return None;
        }
        if let (Mode::Form { options, cursor, field, list, .. }, true) = (&mut self.mode, body.contains(at)) {
            let (chars, text_at) = field.parts();
            let (_, shown) = form_box(chars, text_at, usize::from(body.width), body.height, options.len());
            let below = usize::from(at.y - body.y).checked_sub(shown + 2 + REFUSAL_ROWS);
            match below {
                None => *list = false,
                Some(n) => {
                    *list = true;
                    if let Some(n) = n.checked_sub(1).filter(|n| *n < options.len()) {
                        *cursor = n;
                    }
                }
            }
            return None;
        }
        if matches!(self.mode, Mode::Select { .. }) && body.contains(at) {
            let n = (self.scroll.offset() + usize::from(at.y - body.y)).checked_sub(self.prompt_rows(usize::from(body.width)).len());
            if let (Some(n), Mode::Select { options, cursor }) = (n, &mut self.mode) {
                if n < options.len() {
                    *cursor = n;
                }
            }
            return None;
        }
        match self.buttons(self.area).iter().position(|b| b.contains(at))? {
            0 => self.confirm(),
            _ => Some(Outcome::cancelled()),
        }
    }
}

/// The box of a `form` in a body `width` x `height` with `options` below it: the first text row shown and how many rows
/// the box shows. The room left by the box, its refusal rows, the list's label and its options is the box's.
fn form_box(chars: &[char], cursor: usize, width: usize, height: u16, options: usize) -> (usize, usize) {
    let avail = usize::from(height).saturating_sub(2 + REFUSAL_ROWS + 1 + options).max(1);
    let tw = input_box::text_width(width);
    let (cursor_row, _) = input_box::cursor_at(chars, cursor, tw);
    (( cursor_row + 1).saturating_sub(avail), avail.min(input_box::wrapped_rows(chars, cursor, tw)))
}

/// The body of the dialog's frame (the one widget of all four kinds).
struct Body<'a> {
    dialog: &'a Dialog,
}

impl Widget for Body<'_> {
    fn render(self, inner: Rect, buf: &mut Buffer) {
        self.dialog.draw_body(inner, buf);
    }
}

/// The layout test's sample (`layout_tests`): `case` drawn by [`Dialog::draw`] as a popup of `width` x `height`, set in a
/// client wider by two margins so the test sees the margin columns. The frame is `Dialog::area` as saved; an input's box
/// is the dialog's body rect, as many rows as its text shows.
#[cfg(test)]
pub(crate) fn layout_sample(width: u16, height: u16, case: crate::layout_tests::Case) -> crate::layout_tests::Sample {
    use crate::layout_tests::{Case, Exempt, Kind, Placed, Rule, Sample};
    use ratatui::buffer::Buffer;
    let body_width = usize::from(width.saturating_sub(4));
    let tw = body_width.saturating_sub(4).max(1);
    let input = |len: usize| {
        let text: String = ('a'..='z').cycle().take(len).collect();
        let mode = Mode::Input { validator: Validator::PlanId, field: Field::new(&text), refused: None };
        (Dialog::new("New Plan".into(), "Name:".into(), mode), len)
    };
    let (mut dialog, input_len) = match case {
        Case::Confirm => (Dialog::new("Archive".into(), "Archive cpd-ui?".into(), Mode::Confirm { yes: "Archive".into() }), None),
        Case::InputEmpty => (input(0).0, Some(0)),
        Case::InputOne => (input(3).0, Some(3)),
        Case::InputWrapped => (input(2 * tw).0, Some(2 * tw)),
        Case::Text => (Dialog::new("Legend".into(), "line 1\nline 2\nline 3".into(), Mode::Text), None),
        Case::Checklist => {
            let row = |id: &str, text: &str, detail: &str| Row { id: id.into(), text: text.into(), detail: detail.into() };
            let rows = vec![row("r-1", "use sqlite as the store", "duplicate r-0"), row("r-2", "wire format", "documented docs/a.md")];
            let mode = Mode::Checklist { rows, ticked: vec![false, false], cursor: 0 };
            (Dialog::new("Retire records of cpd".into(), String::new(), mode), None)
        }
        Case::Menu(_) => panic!("a menu is no dialog"),
    };
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| dialog.draw(f)).unwrap();
    let popup = terminal.backend().buffer().clone();
    let margin = crate::box_widget::spacing::MARGIN_X;
    let container = Rect::new(0, 0, width + 2 * margin, height);
    let mut buf = Buffer::empty(container);
    for y in 0..height {
        for x in 0..width {
            buf[(x + margin, y)] = popup[(x, y)].clone();
        }
    }
    let shift = |r: Rect| Rect::new(r.x + margin, r.y, r.width, r.height);
    let spacing = "dialog blank row (hs-d578)";
    let mut boxes = vec![Placed {
        name: "dialog frame",
        kind: Kind::Full,
        rect: shift(dialog.area),
        exempt: vec![
            Exempt { name: spacing, row: 1, rule: Rule::Vertical },
            Exempt { name: spacing, row: height.saturating_sub(3), rule: Rule::Vertical },
            Exempt { name: "buttons row, empty when no button fits (A13)", row: height.saturating_sub(2), rule: Rule::Vertical },
        ],
    }];
    if let Some(len) = input_len {
        let body = parts_in(dialog.area).body;
        let avail = usize::from(body.height).saturating_sub(2 + REFUSAL_ROWS).max(1);
        let rows = (len / tw + 1).min(avail);
        let rect = Rect::new(body.x, body.y, body.width, u16::try_from(rows + 2).unwrap());
        boxes.push(Placed { name: "input box", kind: Kind::Full, rect: shift(rect), exempt: Vec::new() });
    }
    Sample { buf, container, boxes }
}

#[cfg(test)]
#[path = "view_tests.rs"]
mod tests;
