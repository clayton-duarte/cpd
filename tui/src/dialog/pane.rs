//! The harness of the pane tests (`pane_tests.rs`, `checklist_tests.rs`): `cpd tui view dialog …` run in-process on a
//! ratatui `TestBackend` of the popup's size (the popup is opened with `-B`, so the view gets a pane of the popup's
//! whole size). The arguments are parsed as `run` parses them ([`Kind::parts`]), each event goes through [`dispatch`]
//! as `show` sends it, and a closed dialog's result is written by [`write_result`] to the `--out` file, as `run` does.
//! Columns and rows of a click are as the terminal reports them to the view: 0-based.

use std::path::PathBuf;

use clap::Parser;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use serde_json::Value;

use super::{Args, Dialog, Outcome, dispatch, write_result};

#[derive(Parser)]
struct Cli {
    #[command(flatten)]
    dialog: Args,
}

/// A dialog being shown in a pane of its own.
pub(super) struct Pane {
    dialog: Dialog,
    terminal: Terminal<TestBackend>,
    out: PathBuf,
    closed: Option<Outcome>,
    /// Holds the `--out` file and the `--items` file; removed with the pane.
    _dir: tempfile::TempDir,
}

impl Pane {
    /// `cpd tui view dialog <kind> --title=<title> --out=<out> <fields>` in a pane of `width` by `height`.
    pub(super) fn start(kind: &str, title: &str, fields: &[&str], width: u16, height: u16) -> Pane {
        Pane::open(
            tempfile::tempdir().unwrap(),
            kind,
            title,
            fields,
            width,
            height,
        )
    }

    /// A `checklist` over `rows` (`{id, text, detail}`), written to the `--items` file.
    pub(super) fn checklist(title: &str, rows: &Value, width: u16, height: u16) -> Pane {
        let dir = tempfile::tempdir().unwrap();
        let items = dir.path().join("items.json");
        std::fs::write(&items, rows.to_string()).unwrap();
        Pane::open(
            dir,
            "checklist",
            title,
            &[&format!("--items={}", items.display())],
            width,
            height,
        )
    }

    fn open(
        dir: tempfile::TempDir,
        kind: &str,
        title: &str,
        fields: &[&str],
        width: u16,
        height: u16,
    ) -> Pane {
        let out = dir.path().join(format!("{kind}-out.json"));
        let mut argv = vec![
            "cpd".to_owned(),
            kind.to_owned(),
            format!("--title={title}"),
            format!("--out={}", out.display()),
        ];
        argv.extend(fields.iter().map(|f| (*f).to_owned()));
        let cli = Cli::try_parse_from(&argv).unwrap_or_else(|e| panic!("{argv:?}: {e}"));
        let (_, dialog) = cli.dialog.kind.parts().unwrap();
        let terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        Pane {
            dialog,
            terminal,
            out,
            closed: None,
            _dir: dir,
        }
    }

    /// The screen: the view draws, then each row as a terminal shows it, its trailing blanks cut.
    pub(super) fn screen(&mut self) -> Vec<String> {
        let dialog = &mut self.dialog;
        self.terminal.draw(|f| dialog.draw(f)).unwrap();
        let buf = self.terminal.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect()
    }

    /// Whether the view has closed (it takes no more input then).
    pub(super) fn closed(&self) -> bool {
        self.closed.is_some()
    }

    /// The file the view wrote when it closed.
    pub(super) fn out(&self) -> &std::path::Path {
        &self.out
    }

    /// The outcome file's JSON; the view must have closed.
    pub(super) fn outcome(&self) -> Value {
        assert!(self.closed(), "the view is still open");
        serde_json::from_str(
            &std::fs::read_to_string(&self.out).expect("the view wrote its outcome"),
        )
        .unwrap()
    }

    /// One event, after a draw (`show` draws before it reads); a closing one writes the result.
    fn send(&mut self, event: Event) {
        self.screen();
        if self.closed() {
            return;
        }
        if let Some(outcome) = dispatch(&mut self.dialog, event) {
            write_result(&self.out, &self.dialog, &outcome).unwrap();
            self.closed = Some(outcome);
        }
    }

    /// Keys by their tmux names (`Enter`, `Escape`, `Space`, `BSpace`, `Up`, `Down`, `NPage`, `PPage`) or one character.
    pub(super) fn keys(&mut self, keys: &[&str]) {
        for key in keys {
            let code = match *key {
                "Enter" => KeyCode::Enter,
                "Escape" => KeyCode::Esc,
                "Space" => KeyCode::Char(' '),
                "BSpace" => KeyCode::Backspace,
                "Up" => KeyCode::Up,
                "Down" => KeyCode::Down,
                "NPage" => KeyCode::PageDown,
                "PPage" => KeyCode::PageUp,
                one if one.chars().count() == 1 => KeyCode::Char(one.chars().next().unwrap()),
                other => panic!("no key named {other:?}"),
            };
            self.send(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
        }
    }

    /// `text` typed, a key per character.
    pub(super) fn literal(&mut self, text: &str) {
        for c in text.chars() {
            self.send(Event::Key(KeyEvent::new(
                KeyCode::Char(c),
                KeyModifiers::NONE,
            )));
        }
    }

    /// A left button press at 0-based `(column, row)`.
    pub(super) fn click(&mut self, column: u16, row: u16) {
        self.mouse(MouseEventKind::Down(MouseButton::Left), column, row);
    }

    /// One wheel notch down at 0-based `(column, row)`.
    pub(super) fn wheel_down(&mut self, column: u16, row: u16) {
        self.mouse(MouseEventKind::ScrollDown, column, row);
    }

    /// One wheel notch up at 0-based `(column, row)`.
    pub(super) fn wheel_up(&mut self, column: u16, row: u16) {
        self.mouse(MouseEventKind::ScrollUp, column, row);
    }

    fn mouse(&mut self, kind: MouseEventKind, column: u16, row: u16) {
        self.send(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
    }
}

/// A pill: its label between the spec's caps, no padding.
pub(super) fn pill(label: &str) -> String {
    let caps = &cpd_core::pill::spec().caps;
    format!("{}{label}{}", caps.left, caps.right)
}

/// The top border of a popup `w` wide: `╭─`, the title pill, `─` up to `╮`.
pub(super) fn top(w: usize, title: &str) -> String {
    format!(
        "╭─{}{}╮",
        pill(title),
        "─".repeat(w - 5 - title.chars().count())
    )
}

/// The bottom border of a popup `w` wide.
pub(super) fn bottom(w: usize) -> String {
    format!("╰{}╯", "─".repeat(w - 2))
}

/// A row of the inside of a popup `w` wide: wall, padding, `content` (blank to the width `w - 4`), padding, wall.
pub(super) fn row(w: usize, content: &str) -> String {
    format!(
        "│ {content}{} │",
        " ".repeat(w - 4 - content.chars().count())
    )
}

/// The buttons row of a popup `w` wide: the pill `accept` flush left and, when given, the pill `other` flush right.
pub(super) fn buttons(w: usize, accept: &str, other: Option<&str>) -> String {
    let left = pill(accept);
    match other {
        Some(other) => {
            let right = pill(other);
            let gap = w - 4 - left.chars().count() - right.chars().count();
            row(w, &format!("{left}{}{right}", " ".repeat(gap)))
        }
        None => row(w, &left),
    }
}

/// The 0-based column of the left cap of the pill `label` in `line`.
pub(super) fn column_of(line: &str, label: &str) -> usize {
    let at = line
        .find(&pill(label))
        .unwrap_or_else(|| panic!("{label:?} in {line:?}"));
    line[..at].chars().count()
}

/// Whether a row of `rows` holds the pill `label`.
pub(super) fn has_pill(rows: &[String], label: &str) -> bool {
    rows.iter().any(|l| l.contains(&pill(label)))
}
