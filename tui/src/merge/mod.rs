//! The `merge` view (commands.md §13.2; hs-d273, hs-d473): the confirmation of the Deploy column's merge icon, in a
//! tmux popup. It shows the pull request (`owner/repo#n` and its title, from the store) and the method (`squash`), with
//! `[Merge]` and `[Cancel]`. Enter or `y`, or a click on `[Merge]`, starts `cpd tui action merge <owner>/<repo>#<n>
//! --client <name>` with tmux `run-shell -b` and exits, so the popup closes at once; Esc, `n`, `q` or a click on
//! `[Cancel]` closes it with nothing done. It shows nothing after the confirm and writes nothing itself.
//!
//! - [`popup_args`] and [`open`]: the popup, on the newest client, which the merge icon, the menu's Merge and the
//!   Inbox `pr-ready` item run;
//! - [`action`]: the merge the confirm starts, in the background: `pr merge`, then `pr poll`; a failed merge opens
//!   the cockpit's one failure dialog ([`dialog::show_error_on`]), a failed poll is a line of the log.

pub mod marker;

use std::ffi::OsString;
use std::fmt;

use ratatui::Frame;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use ratatui::style::Modifier;
use crate::palette;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use rusqlite::OptionalExtension;

use super::viewlog::ViewLog;
use super::{RunCommand, dialog, not_a_terminal, on_a_terminal};
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
/// The view the merge's log is named after (`$CPD_HOME/run/tui/merge.log`, hs-d364): the action is the `merge`
/// view's.
const LOG: &str = "merge";

/// `<owner>/<repo>#<n>`, checked only as far as the view needs it (a `/` in the repository, a positive number);
/// `pr merge` is the authority on the rest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pull {
    repo: String,
    number: i64,
}

impl Pull {
    /// The pull request of an Inbox item's subject, `pr:<owner>/<repo>#<n>`; none for any other subject.
    pub fn from_subject(subject: &str) -> Option<Pull> {
        Pull::parse(subject.strip_prefix("pr:")?).ok()
    }

    pub(crate) fn parse(text: &str) -> Result<Pull, String> {
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

/// `cpd tui view merge <owner>/<repo>#<n> --client <name>`.
#[derive(clap::Args, Debug)]
pub struct Args {
    /// The pull request to merge
    #[arg(value_name = "owner/repo#n", value_parser = Pull::parse)]
    pull: Pull,
    /// The tmux client the popup opened on (`#{client_name}`), where the merge's failure dialog opens
    #[arg(long)]
    client: String,
}

/// What the User's key or click asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Merge,
    Cancel,
}

/// The action of a key press, if any.
fn on_key(key: KeyEvent) -> Option<Action> {
    if key.kind != KeyEventKind::Press || key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
        return None;
    }
    match key.code {
        KeyCode::Enter | KeyCode::Char('y') => Some(Action::Merge),
        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => Some(Action::Cancel),
        _ => None,
    }
}

/// The two buttons' rects in `area`: `[Merge]`, `[Cancel]`.
fn buttons(area: Rect) -> [Rect; 2] {
    let at = |column: u16, text: &str| {
        Rect::new(area.x + column, area.y + BUTTON_ROW, text.len() as u16, 1).intersection(area)
    };
    [at(MERGE_COLUMN, MERGE), at(CANCEL_COLUMN, CANCEL)]
}

/// The action of a mouse event over a dialog in `area`: a left click on a button.
fn on_mouse(area: Rect, mouse: MouseEvent) -> Option<Action> {
    if mouse.kind != MouseEventKind::Down(MouseButton::Left) {
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
fn draw(f: &mut Frame, area: Rect, pull: &Pull, title: Option<&str>) {
    let width = usize::from(area.width).saturating_sub(2);
    fn row(f: &mut Frame, area: Rect, n: u16, line: Line<'static>) {
        f.render_widget(Paragraph::new(line), Rect::new(area.x, area.y + n, area.width, 1).intersection(area));
    }
    row(f, area, 0, Line::from(Span::styled(format!(" {}", cut(&pull.to_string(), width)), palette::read_only_text().add_modifier(Modifier::BOLD))));
    if let Some(title) = title {
        row(f, area, 1, Line::styled(format!(" {}", cut(title, width)), palette::read_only_text()));
    }
    row(f, area, 2, Line::styled(format!(" method: {METHOD}"), palette::read_only_text()));
    let [merge, cancel] = buttons(area);
    f.render_widget(Paragraph::new(MERGE).style(palette::selected()), merge);
    f.render_widget(Paragraph::new(CANCEL).style(palette::clickable_text()), cancel);
}

/// The popup's size, border included, at most the client's `(columns, rows)`.
fn popup_size(client: (u16, u16)) -> (u16, u16) {
    (POPUP.0.min(client.0), POPUP.1.min(client.1))
}

/// `arg` as one word for the popup's shell: in single quotes.
fn sh_quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', r"'\''"))
}

/// The tmux arguments that open the dialog for `arg` (`<owner>/<repo>#<n>`) in a popup on the client `name`, at most
/// the client's size `(columns, rows)`. The view gets `--client <name>`, for the merge it starts. (tmux does not
/// expand formats in `display-popup`'s shell command, so the `#` of `arg` stays as it is.)
pub fn popup_args(name: &str, client: (u16, u16), arg: &str) -> Vec<String> {
    let (w, h) = popup_size(client);
    [
        "display-popup".to_owned(),
        "-E".to_owned(),
        "-c".to_owned(),
        name.to_owned(),
        "-T".to_owned(),
        " merge ".to_owned(),
        "-w".to_owned(),
        w.to_string(),
        "-h".to_owned(),
        h.to_string(),
        "--".to_owned(),
        format!("{PROGRAM} tui view merge {} --client {}", sh_quote(arg), sh_quote(name)),
    ]
    .to_vec()
}

/// The name and the size `(columns, rows)` of the most recently active client in `list-clients`'
/// `#{client_activity} #{client_width} #{client_height} #{client_name}` lines.
fn newest_named_client(listing: &str) -> Option<(String, (u16, u16))> {
    listing
        .lines()
        .filter_map(|line| {
            let mut words = line.splitn(4, ' ');
            let activity: u64 = words.next()?.parse().ok()?;
            let size = (words.next()?.parse().ok()?, words.next()?.parse().ok()?);
            Some((activity, words.next().filter(|n| !n.is_empty())?.to_owned(), size))
        })
        .max_by_key(|(activity, _, _)| *activity)
        .map(|(_, name, size)| (name, size))
}

/// The merge icon's click, the menu's Merge and an Inbox `pr-ready` item's: the dialog for `arg` in a tmux popup over
/// the cockpit, on the most recently active client.
pub(crate) fn open(tmux: &Tmux, arg: &str) -> Result<(), CpdError> {
    let format = "#{client_activity} #{client_width} #{client_height} #{client_name}";
    let clients = tmux.run(&["list-clients", "-F", format].map(str::to_owned))?;
    let (name, size) = newest_named_client(&clients).ok_or_else(|| {
        CpdError::new(ErrorCode::TmuxFailed, "No tmux client is attached to the cockpit.").with("socket", tmux.socket())
    })?;
    tmux.run(&popup_args(&name, size, arg)).map(drop)
}

/// The tmux arguments the confirm runs: `run-shell -b 'cpd tui action merge <ref> --client <name>'`. The command is
/// quoted for the shell, then every `#` doubled for `run-shell`'s format expansion, so it reaches `cpd` unchanged;
/// stderr is dropped, as the StatusBar's binding drops it (`run-shell -b` shows any output in view mode).
fn start_args(pull: &Pull, client: &str) -> Vec<String> {
    let command = format!("{PROGRAM} tui action merge {} --client {} 2>/dev/null", sh_quote(&pull.to_string()), sh_quote(client));
    ["run-shell".to_owned(), "-b".to_owned(), command.replace('#', "##")].to_vec()
}

/// The stored title of `pull`, if the store has it.
fn stored_title(store: &mut Store, pull: &Pull) -> Result<Option<String>, CpdError> {
    store.read(|conn| {
        Ok(conn
            .query_row("SELECT title FROM pull_requests WHERE repo = ?1 AND number = ?2", (&pull.repo, pull.number), |r| r.get(0))
            .optional()?)
    })
}

pub(super) fn run(args: Args) -> Result<Output, CpdError> {
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
    let result = mouse.and_then(|()| show(&mut terminal, &args.pull, title.as_deref()));
    let _ = ratatui::crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    let action = result.map_err(|e| {
        CpdError::new(ErrorCode::NotATerminal, format!("The terminal of cpd tui view merge failed: {e}."))
            .with("command", "tui view merge")
    })?;
    if action == Action::Merge {
        // The merge runs in the background, owned by the tmux server: this process ends now, and the popup closes.
        Tmux::new(config.tmux.socket).run(&start_args(&args.pull, &args.client))?;
    }
    Ok(Output::Owned)
}

/// Draws and takes input until the User confirms or cancels.
fn show(terminal: &mut ratatui::DefaultTerminal, pull: &Pull, title: Option<&str>) -> std::io::Result<Action> {
    loop {
        let mut area = Rect::default();
        terminal.draw(|f| {
            area = f.area();
            draw(f, area, pull, title);
        })?;
        let action = match event::read()? {
            Event::Key(key) => on_key(key),
            Event::Mouse(mouse) => on_mouse(area, mouse),
            _ => None,
        };
        if let Some(action) = action {
            return Ok(action);
        }
    }
}

/// `cpd tui action merge <owner>/<repo>#<n> --client <name>` (commands.md §13.2, the `merge` action; hs-d473,
/// hs-d312): runs `cpd pr merge` in process and, once it succeeds, `cpd pr poll`, the full refresh the periodic poll
/// runs, so the Deploy column shows the merge at once. A success says nothing. A failed poll is a line of the log
/// only: the pull request did merge. A failed merge runs no poll; it is a line of the log, written before any tmux
/// call so that it is always there, then the cockpit's one failure dialog on `client`. The `Err` is what the dialog
/// could not show: the caller shows it with `display-message -c <client>`.
///
/// The merge marker ([`marker`], hs-d495) is written first, before `pr merge`, and removed when the request ends:
/// after the poll on a success, before the dialog on a failure, on any other exit. While a live marker of `pull`
/// exists, the action does nothing but log `merge already running`.
pub(crate) fn action(home: &CpdHome, tmux: &Tmux, pull: &Pull, client: &str, run_command: RunCommand) -> Result<(), CpdError> {
    let log = ViewLog::of(home, LOG);
    if marker::live(home).contains(&pull.to_string()) {
        // A merge of it runs: its marker is not this action's to touch, and the User is told nothing.
        let _ = log.append(&format!("merge already running {pull}"));
        return Ok(());
    }
    marker::write(home, pull)?;
    let marked = Marked { home, pull };
    let cpd = |words: &[&str]| run_command(std::iter::once(PROGRAM).chain(words.iter().copied()).map(OsString::from).collect());
    let failed = match cpd(&["pr", "merge", &pull.to_string()]) {
        Ok(_) => {
            if let Err(e) = cpd(&["pr", "poll"]) {
                // Nothing else to show it with: the next periodic poll refreshes the column.
                let _ = log.append(&format!("pr poll after merge {pull}: {}: {}", e.code(), e.message()));
            }
            return Ok(());
        }
        Err(e) => e,
    };
    // The request has ended: the spinner goes before the dialog opens.
    drop(marked);
    let mut text = format!("merge {pull}: {}: {}", failed.code(), failed.message());
    if let Err(unwritten) = log.append(&text) {
        text = format!("{text}\n{unwritten}");
    }
    dialog::show_error_on(tmux, client, &format!("merge {pull}"), &text)
        .map_err(|unshown| CpdError::new(unshown.code(), text).with("dialog", unshown.message()))
}

/// The marker of a merge this process runs, removed when it is dropped: no exit, a panic included, leaves it behind.
struct Marked<'a> {
    home: &'a CpdHome,
    pull: &'a Pull,
}

impl Drop for Marked<'_> {
    fn drop(&mut self) {
        marker::remove(self.home, self.pull);
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

    fn render(title: Option<&str>, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, f.area(), &pull(), title)).unwrap();
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
        let buf = render(Some("Add the thing"), 58, 5);
        assert_eq!(rows(&buf), [" o/r#7", " Add the thing", " method: squash", "", "  [Merge]   [Cancel]"]);
    }

    #[test]
    fn a_missing_title_shows_no_title_and_a_long_one_is_cut() {
        assert_eq!(rows(&render(None, 58, 5))[1], "");
        assert_eq!(rows(&render(Some("a very long title indeed"), 12, 5))[1], " a very lo…");
    }

    #[test]
    fn a_dialog_in_a_tiny_area_draws_without_panicking() {
        for (w, h) in [(0, 0), (1, 1), (10, 2), (58, 3)] {
            render(Some("T"), w, h);
        }
    }

    #[test]
    fn enter_or_y_merges_and_esc_n_or_q_cancels() {
        for code in [KeyCode::Enter, KeyCode::Char('y')] {
            assert_eq!(on_key(press(code)), Some(Action::Merge), "{code:?}");
        }
        for code in [KeyCode::Esc, KeyCode::Char('n'), KeyCode::Char('q')] {
            assert_eq!(on_key(press(code)), Some(Action::Cancel), "{code:?}");
        }
        for code in [KeyCode::Char('Y'), KeyCode::Char(' '), KeyCode::Tab, KeyCode::Char('j')] {
            assert_eq!(on_key(press(code)), None, "{code:?}");
        }
        assert_eq!(on_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL)), None);
    }

    #[test]
    fn a_left_click_on_a_button_acts_and_nothing_else_does() {
        let area = Rect::new(0, 0, 58, 5);
        let [merge, cancel] = buttons(area);
        assert_eq!((merge, cancel), (Rect::new(2, 4, 7, 1), Rect::new(12, 4, 8, 1)));
        assert_eq!(on_mouse(area, click(2, 4)), Some(Action::Merge));
        assert_eq!(on_mouse(area, click(8, 4)), Some(Action::Merge));
        assert_eq!(on_mouse(area, click(12, 4)), Some(Action::Cancel));
        assert_eq!(on_mouse(area, click(19, 4)), Some(Action::Cancel));
        for (column, row) in [(1, 4), (9, 4), (11, 4), (20, 4), (2, 3), (2, 0)] {
            assert_eq!(on_mouse(area, click(column, row)), None, "{column},{row}");
        }
        let right = MouseEvent { kind: MouseEventKind::Down(MouseButton::Right), ..click(2, 4) };
        assert_eq!(on_mouse(area, right), None);
        // the buttons are where they are drawn
        let buf = render(None, 58, 5);
        assert_eq!(buf[(merge.x, merge.y)].symbol(), "[");
        assert_eq!(buf[(cancel.x + cancel.width - 1, cancel.y)].symbol(), "]");
    }

    #[test]
    fn the_argument_is_quoted_for_the_popups_shell() {
        assert_eq!(sh_quote("o/r#7"), "'o/r#7'");
        assert_eq!(sh_quote("a'b"), r"'a'\''b'");
    }

    #[test]
    fn the_confirm_starts_the_action_in_the_background_with_every_hash_doubled_for_run_shell() {
        assert_eq!(
            start_args(&pull(), "/dev/pts/9"),
            ["run-shell", "-b", "cpd tui action merge 'o/r##7' --client '/dev/pts/9' 2>/dev/null"]
        );
    }

    #[test]
    fn the_popup_opens_on_the_newest_named_client() {
        let listing = "100 80 24 /dev/pts/1\n300 215 62 /dev/pts/4\n200 120 30 /dev/pts/2\n";
        assert_eq!(newest_named_client(listing), Some(("/dev/pts/4".to_owned(), (215, 62))));
        for bad in ["", "junk\n", "100 80 24\n", "100 80 24 \n", "x 80 24 /dev/pts/1\n"] {
            assert_eq!(newest_named_client(bad), None, "{bad:?}");
        }
    }
}
