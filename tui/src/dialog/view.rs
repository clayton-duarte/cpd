//! The drawing and the input of the three kinds (commands.md §13.2, "The dialog kit"), in the `merge` popup's style:
//! the title as the cockpit's section header, the content under it, a blank row, and the buttons on the last row.
//! A [`Dialog`] is pure state: [`Dialog::draw`] fits it to the area it is given, and the key and mouse handlers answer
//! with the [`Outcome`] that closes it, if any.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Alignment, Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::Outcome;
use super::scroll::Scroll;
use super::validate::Validator;
use crate::input_box;
use crate::palette;
use crate::section;

/// The label of the second button of `confirm` and `input` (the checklist's is [`LATER`]).
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
    /// Entries `[ ] <id> — <detail>` with the record's full text under each, the User ticks, with `[Approve]` and `[Later]`
    /// (hs-d549, hs-d551, hs-d569 CD2, CD3). `cursor` is an entry.
    Checklist { rows: Vec<Row>, ticked: Vec<bool>, cursor: usize },
}

/// One row of a `checklist`: the `--items` file is a JSON array of these.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Row {
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

/// The label of an input's box: its prompt without trailing whitespace and one trailing `:`.
pub(super) fn label(prompt: &str) -> String {
    let t = prompt.trim_end();
    t.strip_suffix(':').unwrap_or(t).trim_end().to_owned()
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
            Mode::Checklist { .. } => vec![format!("[{APPROVE}]"), format!("[{LATER}]")],
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

    /// Keeps the header line of the cursor entry of a `checklist` inside the scrolled window.
    fn follow_cursor(&mut self) {
        let Mode::Checklist { cursor, .. } = &self.mode else { return };
        let width = usize::from(layout(self.area).body.width).saturating_sub(2);
        let cursor = self.entry_lines(width).iter().position(|l| l.header && l.entry == Some(*cursor)).unwrap_or(0);
        let (offset, page) = (self.scroll.offset(), self.scroll.page().max(1));
        if cursor < offset {
            self.scroll.by(-((offset - cursor) as isize));
        } else if cursor >= offset + page {
            self.scroll.by((cursor + 1 - offset - page) as isize);
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
        if let Mode::Checklist { .. } = &self.mode {
            let width = usize::from(body.width).saturating_sub(2);
            return self.entry_lines(width).into_iter().map(|l| l.text).collect();
        }
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
                let (chars, cursor) = field.parts();
                let box_width = usize::from(area.width);
                let avail = usize::from(parts.body.height).saturating_sub(2 + REFUSAL_ROWS).max(1);
                let tw = box_width.saturating_sub(6).max(1);
                let start = (cursor / tw + 1).saturating_sub(avail);
                let drawn = input_box::lines(&label(&self.text), chars, cursor, true, box_width);
                let last = drawn.len() - 1;
                let shown = 1 + avail.min(drawn.len() - 2);
                let window: Vec<Line> = std::iter::once(drawn[0].clone())
                    .chain(drawn[1..last].iter().skip(start).take(avail).cloned())
                    .chain(std::iter::once(drawn[last].clone()))
                    .collect();
                for (n, line) in window.into_iter().enumerate() {
                    f.render_widget(Paragraph::new(line), row(n));
                }
                if let Some(why) = refused {
                    for (n, text) in wrap(why, width).iter().take(REFUSAL_ROWS).enumerate() {
                        let line = Line::from(Span::styled(format!(" {}", cut(text, width)), palette::note()));
                        f.render_widget(Paragraph::new(line), row(shown + 1 + n));
                    }
                }
            }
            Mode::Checklist { cursor, .. } => {
                let first = self.scroll.offset();
                for (n, line) in self.entry_lines(width).iter().skip(first).take(self.scroll.page()).enumerate() {
                    let style = match (line.header, line.entry == Some(*cursor)) {
                        (true, true) => palette::selected(),
                        (true, false) => palette::clickable_text(),
                        (false, _) => palette::read_only_text(),
                    };
                    f.render_widget(Paragraph::new(format!(" {}", line.text)).style(style), row(n));
                }
            }
            _ => {
                let first = self.scroll.offset();
                for (n, text) in rows.iter().skip(first).take(self.scroll.page()).enumerate() {
                    f.render_widget(Paragraph::new(format!(" {text}")).style(palette::read_only_text()), row(n));
                }
            }
        }
        for (n, (label, rect)) in self.labels().iter().zip(self.buttons(area)).enumerate() {
            let style = if n == 0 { palette::selected() } else { palette::clickable_text() };
            f.render_widget(Paragraph::new(label.as_str()).style(style), rect);
        }
        if self.scroll.scrollable() {
            let last = (self.scroll.offset() + self.scroll.page()).min(self.scroll.total());
            let position = format!("{}-{}/{} ", self.scroll.offset() + 1, last, self.scroll.total());
            let taken = self.buttons(area).last().map_or(0, |r| r.x + r.width - area.x);
            if usize::from(taken) + position.chars().count() < usize::from(area.width) {
                let muted = Paragraph::new(position).alignment(Alignment::Right).style(palette::muted_text());
                f.render_widget(muted, parts.buttons);
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
        let body = layout(self.area).body;
        if matches!(self.mode, Mode::Checklist { .. }) && body.contains(at) {
            let n = self.scroll.offset() + usize::from(at.y - body.y);
            let width = usize::from(body.width).saturating_sub(2);
            let entry = self.entry_lines(width).get(n).and_then(|l| l.entry);
            if let (Some(entry), Mode::Checklist { ticked, cursor, .. }) = (entry, &mut self.mode) {
                ticked[entry] = !ticked[entry];
                *cursor = entry;
            }
            return None;
        }
        match self.buttons(self.area).iter().position(|b| b.contains(at))? {
            0 => self.confirm(),
            _ => Some(Outcome::cancelled()),
        }
    }
}

#[cfg(test)]
#[path = "view_tests.rs"]
mod tests;
