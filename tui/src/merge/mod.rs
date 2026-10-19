//! The `merge` view (commands.md §13.2; hs-d273): the confirmation of the Deploy column's merge icon, in a tmux
//! popup. It shows the pull request (`owner/repo#n` and its title, from the store) and the method (`squash`), with
//! `[Merge]` and `[Cancel]`. Enter or `y`, or a click on `[Merge]`, runs `cpd pr merge <owner>/<repo>#<n>` in this
//! process (§9.10); Esc, `n`, `q` or a click on `[Cancel]` closes it with nothing done. After a merge it shows the
//! result, or the command's error message in `palette::note()`, until any key closes it. It writes nothing itself.
//!
//! - [`popup_args`] and [`open`]: the popup, which the merge icon runs.

use std::ffi::OsString;
use std::fmt;

use ratatui::Frame;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use rusqlite::OptionalExtension;

use super::widgets::newest_client;
use super::{RunCommand, not_a_terminal, on_a_terminal};
use crate::palette;
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;
use cpd_core::output::Output;
use cpd_core::session::PROGRAM;
use cpd_core::store::Store;

/// The merge method the dialog names (the one `pr merge` uses, §9.10).
const METHOD: &str = "squash";
/// The popup's size `(columns, rows)`, border included: the dialog's five rows and 58 columns inside it.
const POPUP: (u16, u16) = (60, 7);
const MERGE: &str = "[Merge]";
const CANCEL: &str = "[Cancel]";
/// The dialog's button row, and the columns of its buttons, relative to the view's area.
const BUTTON_ROW: u16 = 4;
const MERGE_COLUMN: u16 = 2;
const CANCEL_COLUMN: u16 = MERGE_COLUMN + MERGE.len() as u16 + 3;

/// `<owner>/<repo>#<n>`, checked only as far as the view needs it (a `/` in the repository, a positive number);
/// `pr merge` is the authority on the rest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pull {
    repo: String,
    number: i64,
}

impl Pull {
    fn parse(text: &str) -> Result<Pull, String> {
        let bad = || format!("{text:?} is not <owner>/<repo>#<n>, with n a positive integer.");
        let (repo, number) = text.split_once('#').ok_or_else(bad)?;
        let number = number.parse::<i64>().ok().filter(|n| *n > 0).ok_or_else(bad)?;
        if !repo.contains('/') {
            return Err(bad());
        }
        Ok(Pull { repo: repo.to_owned(), number })
    }
}

impl fmt::Display for Pull {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.repo, self.number)
    }
}

/// `cpd tui view merge <owner>/<repo>#<n>`.
#[derive(clap::Args, Debug)]
pub struct Args {
    /// The pull request to merge
    #[arg(value_name = "owner/repo#n", value_parser = Pull::parse)]
    pull: Pull,
}

/// What the User's key or click asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Merge,
    Cancel,
    /// Any key, once the result shows.
    Close,
}

/// Where the dialog is.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Phase {
    Ask,
    Merging,
    Merged,
    Failed(String),
}

/// The action of a key press in `phase`, if any.
fn on_key(phase: &Phase, key: KeyEvent) -> Option<Action> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    match phase {
        Phase::Ask if key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => None,
        Phase::Ask => match key.code {
            KeyCode::Enter | KeyCode::Char('y') => Some(Action::Merge),
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => Some(Action::Cancel),
            _ => None,
        },
        Phase::Merged | Phase::Failed(_) => Some(Action::Close),
        Phase::Merging => None,
    }
}

/// The two buttons' rects in `area`: `[Merge]`, `[Cancel]`.
fn buttons(area: Rect) -> [Rect; 2] {
    let at = |column: u16, text: &str| {
        Rect::new(area.x + column, area.y + BUTTON_ROW, text.len() as u16, 1).intersection(area)
    };
    [at(MERGE_COLUMN, MERGE), at(CANCEL_COLUMN, CANCEL)]
}

/// The action of a mouse event in `phase` over a dialog in `area`: a left click on a button.
fn on_mouse(phase: &Phase, area: Rect, mouse: MouseEvent) -> Option<Action> {
    if *phase != Phase::Ask || mouse.kind != MouseEventKind::Down(MouseButton::Left) {
        return None;
    }
    let at = Position::new(mouse.column, mouse.row);
    let [merge, cancel] = buttons(area);
    if merge.contains(at) {
        Some(Action::Merge)
    } else if cancel.contains(at) {
        Some(Action::Cancel)
    } else {
        None
    }
}

/// `text` cut to `width` columns, with `…` at the cut.
fn cut(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    text.chars().take(width.saturating_sub(1)).chain(std::iter::once('…')).collect()
}

/// Draws the dialog in `area`. `title` is the stored title, if the store has the pull request.
fn draw(f: &mut Frame, area: Rect, pull: &Pull, title: Option<&str>, phase: &Phase) {
    let width = usize::from(area.width).saturating_sub(2);
    fn row(f: &mut Frame, area: Rect, n: u16, line: Line<'static>) {
        f.render_widget(Paragraph::new(line), Rect::new(area.x, area.y + n, area.width, 1).intersection(area));
    }
    row(f, area, 0, Line::from(Span::styled(format!(" {}", cut(&pull.to_string(), width)), Style::new().add_modifier(Modifier::BOLD))));
    if let Some(title) = title {
        row(f, area, 1, Line::from(format!(" {}", cut(title, width))));
    }
    match phase {
        Phase::Ask => {
            row(f, area, 2, Line::from(format!(" method: {METHOD}")));
            let [merge, cancel] = buttons(area);
            f.render_widget(Paragraph::new(MERGE).style(Style::new().add_modifier(Modifier::REVERSED)), merge);
            f.render_widget(Paragraph::new(CANCEL), cancel);
        }
        Phase::Merging => row(f, area, 2, Line::from(" merging…")),
        Phase::Merged => {
            row(f, area, 2, Line::from(format!(" Merged {pull} ({METHOD}).")));
            row(f, area, 4, Line::from(" any key closes").style(Style::new().add_modifier(Modifier::DIM)));
        }
        Phase::Failed(message) => {
            let body = Rect::new(area.x, area.y + 2, area.width, area.height.saturating_sub(3)).intersection(area);
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(format!(" {message}"), palette::note()))).wrap(Wrap { trim: false }),
                body,
            );
            row(f, area, 4, Line::from(" any key closes").style(Style::new().add_modifier(Modifier::DIM)));
        }
    }
}

/// The popup's size, border included, at most the client's `(columns, rows)`.
fn popup_size(client: (u16, u16)) -> (u16, u16) {
    (POPUP.0.min(client.0), POPUP.1.min(client.1))
}

/// `arg` as one word for the popup's shell: in single quotes.
fn sh_quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', r"'\''"))
}

/// The tmux arguments that open the dialog for `arg` (`<owner>/<repo>#<n>`) in a popup, at most the client's size
/// `(columns, rows)`.
pub fn popup_args(client: (u16, u16), arg: &str) -> Vec<String> {
    let (w, h) = popup_size(client);
    [
        "display-popup".to_owned(),
        "-E".to_owned(),
        "-T".to_owned(),
        " merge ".to_owned(),
        "-w".to_owned(),
        w.to_string(),
        "-h".to_owned(),
        h.to_string(),
        format!("{PROGRAM} tui view merge {}", sh_quote(arg)),
    ]
    .to_vec()
}

/// The merge icon's click: the dialog for `arg` in a tmux popup over the cockpit.
pub(crate) fn open(tmux: &Tmux, arg: &str) -> Result<(), CpdError> {
    let clients = tmux.run(&["list-clients", "-F", "#{client_activity} #{client_width} #{client_height}"].map(str::to_owned))?;
    let client = newest_client(&clients).ok_or_else(|| {
        CpdError::new(ErrorCode::TmuxFailed, "No tmux client is attached to the cockpit.").with("socket", tmux.socket())
    })?;
    tmux.run(&popup_args(client, arg)).map(drop)
}

/// The stored title of `pull`, if the store has it.
fn stored_title(store: &mut Store, pull: &Pull) -> Result<Option<String>, CpdError> {
    store.read(|conn| {
        Ok(conn
            .query_row("SELECT title FROM pull_requests WHERE repo = ?1 AND number = ?2", (&pull.repo, pull.number), |r| r.get(0))
            .optional()?)
    })
}

pub(super) fn run(args: Args, run_command: RunCommand) -> Result<Output, CpdError> {
    if !on_a_terminal() {
        return Err(not_a_terminal("tui view merge"));
    }
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let title = stored_title(&mut store, &args.pull)?;
    // From here on this process owns the terminal (commands.md §1.2).
    let mut terminal = ratatui::init();
    let mouse = ratatui::crossterm::execute!(std::io::stdout(), EnableMouseCapture);
    let result = mouse.and_then(|()| show(&mut terminal, &args.pull, title.as_deref(), run_command));
    let _ = ratatui::crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    match result {
        Ok(()) => Ok(Output::Owned),
        Err(e) => Err(CpdError::new(ErrorCode::NotATerminal, format!("The terminal of cpd tui view merge failed: {e}."))
            .with("command", "tui view merge")),
    }
}

/// Draws and takes input until the dialog closes.
fn show(terminal: &mut ratatui::DefaultTerminal, pull: &Pull, title: Option<&str>, run_command: RunCommand) -> std::io::Result<()> {
    let mut phase = Phase::Ask;
    loop {
        let mut area = Rect::default();
        terminal.draw(|f| {
            area = f.area();
            draw(f, area, pull, title, &phase);
        })?;
        let action = match event::read()? {
            Event::Key(key) => on_key(&phase, key),
            Event::Mouse(mouse) => on_mouse(&phase, area, mouse),
            _ => None,
        };
        match action {
            Some(Action::Cancel | Action::Close) => return Ok(()),
            Some(Action::Merge) => {
                phase = Phase::Merging;
                terminal.draw(|f| draw(f, f.area(), pull, title, &phase))?;
                let argv: Vec<OsString> = [PROGRAM, "pr", "merge", &pull.to_string()].map(OsString::from).into();
                phase = match run_command(argv) {
                    Ok(_) => Phase::Merged,
                    Err(e) => Phase::Failed(e.message().to_owned()),
                };
            }
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use super::*;

    fn pull() -> Pull {
        Pull::parse("o/r#7").unwrap()
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn click(column: u16, row: u16) -> MouseEvent {
        MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column, row, modifiers: KeyModifiers::NONE }
    }

    fn render(phase: &Phase, title: Option<&str>, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, f.area(), &pull(), title, phase)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn rows(buf: &Buffer) -> Vec<String> {
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_owned())
            .collect()
    }

    #[test]
    fn the_argument_is_owner_repo_hash_number() {
        assert_eq!(pull().to_string(), "o/r#7");
        for bad in ["", "o/r", "o/r#", "o/r#x", "o/r#0", "o/r#-1", "r#7", "o#7"] {
            assert!(Pull::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_dialog_shows_the_pull_request_its_title_the_method_and_both_buttons() {
        let buf = render(&Phase::Ask, Some("Add the thing"), 58, 5);
        assert_eq!(rows(&buf), [" o/r#7", " Add the thing", " method: squash", "", "  [Merge]   [Cancel]"]);
    }

    #[test]
    fn a_missing_title_shows_no_title_and_a_long_one_is_cut() {
        assert_eq!(rows(&render(&Phase::Ask, None, 58, 5))[1], "");
        assert_eq!(rows(&render(&Phase::Ask, Some("a very long title indeed"), 12, 5))[1], " a very lo…");
    }

    #[test]
    fn the_result_and_the_error_show_until_a_key() {
        assert_eq!(rows(&render(&Phase::Merged, Some("T"), 58, 5)), [" o/r#7", " T", " Merged o/r#7 (squash).", "", " any key closes"]);
        let buf = render(&Phase::Failed("o/r#7 is OPEN and BLOCKED.".into()), Some("T"), 58, 5);
        assert_eq!(rows(&buf)[2], " o/r#7 is OPEN and BLOCKED.");
        assert_eq!(buf[(2, 2)].fg, palette::note().fg.unwrap());
        assert_eq!(rows(&buf)[4], " any key closes");
    }

    #[test]
    fn a_dialog_in_a_tiny_area_draws_without_panicking() {
        for (w, h) in [(0, 0), (1, 1), (10, 2), (58, 3)] {
            render(&Phase::Ask, Some("T"), w, h);
            render(&Phase::Failed("x".repeat(200)), Some("T"), w, h);
        }
    }

    #[test]
    fn enter_or_y_merges_and_esc_n_or_q_cancels() {
        for code in [KeyCode::Enter, KeyCode::Char('y')] {
            assert_eq!(on_key(&Phase::Ask, press(code)), Some(Action::Merge), "{code:?}");
        }
        for code in [KeyCode::Esc, KeyCode::Char('n'), KeyCode::Char('q')] {
            assert_eq!(on_key(&Phase::Ask, press(code)), Some(Action::Cancel), "{code:?}");
        }
        for code in [KeyCode::Char('Y'), KeyCode::Char(' '), KeyCode::Tab, KeyCode::Char('j')] {
            assert_eq!(on_key(&Phase::Ask, press(code)), None, "{code:?}");
        }
        assert_eq!(on_key(&Phase::Ask, KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL)), None);
    }

    #[test]
    fn after_a_merge_any_key_closes_and_while_merging_none_does() {
        for phase in [Phase::Merged, Phase::Failed("x".into())] {
            for code in [KeyCode::Enter, KeyCode::Char('y'), KeyCode::Char('z'), KeyCode::Esc] {
                assert_eq!(on_key(&phase, press(code)), Some(Action::Close), "{phase:?} {code:?}");
            }
        }
        assert_eq!(on_key(&Phase::Merging, press(KeyCode::Esc)), None);
        let mut release = press(KeyCode::Enter);
        release.kind = KeyEventKind::Release;
        assert_eq!(on_key(&Phase::Ask, release), None);
    }

    #[test]
    fn a_left_click_on_a_button_acts_and_nothing_else_does() {
        let area = Rect::new(0, 0, 58, 5);
        let [merge, cancel] = buttons(area);
        assert_eq!((merge, cancel), (Rect::new(2, 4, 7, 1), Rect::new(12, 4, 8, 1)));
        assert_eq!(on_mouse(&Phase::Ask, area, click(2, 4)), Some(Action::Merge));
        assert_eq!(on_mouse(&Phase::Ask, area, click(8, 4)), Some(Action::Merge));
        assert_eq!(on_mouse(&Phase::Ask, area, click(12, 4)), Some(Action::Cancel));
        assert_eq!(on_mouse(&Phase::Ask, area, click(19, 4)), Some(Action::Cancel));
        for (column, row) in [(1, 4), (9, 4), (11, 4), (20, 4), (2, 3), (2, 0)] {
            assert_eq!(on_mouse(&Phase::Ask, area, click(column, row)), None, "{column},{row}");
        }
        let right = MouseEvent { kind: MouseEventKind::Down(MouseButton::Right), ..click(2, 4) };
        assert_eq!(on_mouse(&Phase::Ask, area, right), None);
        assert_eq!(on_mouse(&Phase::Merged, area, click(2, 4)), None);
        // the buttons are where they are drawn
        let buf = render(&Phase::Ask, None, 58, 5);
        assert_eq!(buf[(merge.x, merge.y)].symbol(), "[");
        assert_eq!(buf[(cancel.x + cancel.width - 1, cancel.y)].symbol(), "]");
    }

    #[test]
    fn the_popup_is_60_by_7_at_most_the_clients_size_and_runs_the_view() {
        let command = "cpd tui view merge 'o/r#7'";
        assert_eq!(
            popup_args((200, 60), "o/r#7"),
            ["display-popup", "-E", "-T", " merge ", "-w", "60", "-h", "7", command]
        );
        let small = popup_args((40, 5), "o/r#7");
        assert_eq!((small[5].as_str(), small[7].as_str()), ("40", "5"));
    }

    #[test]
    fn the_argument_is_quoted_for_the_popups_shell() {
        assert_eq!(sh_quote("o/r#7"), "'o/r#7'");
        assert_eq!(sh_quote("a'b"), r"'a'\''b'");
    }
}
