//! The dialog kit (commands.md §13.2, "The dialog kit"; hs-d295, hs-d297): every cockpit confirmation and input is a
//! tmux popup running `cpd tui view dialog <confirm|input|text> …`, built on this module. The popup holds no caller
//! logic: it draws, takes the keys and clicks, and on close writes its [`Outcome`] to the file `--out` named. The
//! caller opens the popup with `display-popup -E`, which returns when the popup closes, reads the outcome and runs
//! its own commands.
//!
//! - [`confirm`], [`input`] and [`text`]: the openers a caller in another module uses;
//! - [`scroll`]: the scrolling (↑↓, j/k, PgUp/PgDn, the wheel), which the logs and legend popups reuse;
//! - [`validate`]: the closed set of `input` checks, passed by name;
//! - [`view`]: the drawing and the input of the kinds, in the `merge` popup's style.

pub(crate) mod scroll;
pub(crate) mod validate;
mod view;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event};
use serde::{Deserialize, Serialize};

use super::widgets::newest_client;
use super::{not_a_terminal, on_a_terminal};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::Tmux;
use cpd_core::output::Output;
use cpd_core::session::PROGRAM;
use validate::Validator;
use view::{CANCEL, Dialog, Field, Mode, REFUSAL_ROWS, wrap};

/// `cpd tui view dialog <confirm|input|text> …`.
#[derive(clap::Args, Debug)]
pub struct Args {
    #[command(subcommand)]
    kind: Kind,
}

#[derive(clap::Subcommand, Debug)]
enum Kind {
    /// A question with two buttons
    Confirm(Confirm),
    /// A one-line text field
    Input(Input),
    /// Scrollable read-only text
    Text(Text),
}

/// What every kind takes.
#[derive(clap::Args, Debug)]
struct Common {
    /// The title, drawn as the section header
    #[arg(long)]
    title: String,
    /// The file the outcome is written to (a small JSON value) when the dialog closes
    #[arg(long, value_name = "path")]
    out: PathBuf,
}

#[derive(clap::Args, Debug)]
struct Confirm {
    #[command(flatten)]
    common: Common,
    /// The question
    #[arg(long)]
    text: String,
    /// The label of the confirming button
    #[arg(long, default_value = "Confirm")]
    yes_label: String,
}

#[derive(clap::Args, Debug)]
struct Input {
    #[command(flatten)]
    common: Common,
    /// The question above the field
    #[arg(long)]
    prompt: String,
    /// The field's starting value
    #[arg(long, default_value = "")]
    initial: String,
    /// The check an accepted value passes; a refused value shows its reason and the dialog stays
    #[arg(long, value_enum)]
    validate: Validator,
}

#[derive(clap::Args, Debug)]
struct Text {
    #[command(flatten)]
    common: Common,
    /// The text
    #[arg(long)]
    body: String,
}

/// How a dialog closed: the result file's content. `value` is the typed text of an accepted `input`, and only then.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Outcome {
    confirmed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<String>,
}

impl Outcome {
    pub(crate) fn confirmed() -> Outcome {
        Outcome { confirmed: true, value: None }
    }

    pub(crate) fn cancelled() -> Outcome {
        Outcome { confirmed: false, value: None }
    }

    pub(crate) fn value(value: String) -> Outcome {
        Outcome { confirmed: true, value: Some(value) }
    }
}

impl Kind {
    fn parts(self) -> (Common, Dialog) {
        match self {
            Kind::Confirm(a) => {
                let dialog = Dialog::new(a.common.title.clone(), a.text, Mode::Confirm { yes: a.yes_label });
                (a.common, dialog)
            }
            Kind::Input(a) => {
                let mode = Mode::Input { validator: a.validate, field: Field::new(&a.initial), refused: None };
                let dialog = Dialog::new(a.common.title.clone(), a.prompt, mode);
                (a.common, dialog)
            }
            Kind::Text(a) => {
                let dialog = Dialog::new(a.common.title.clone(), a.body, Mode::Text);
                (a.common, dialog)
            }
        }
    }
}

pub(super) fn run(args: Args) -> Result<Output, CpdError> {
    if !on_a_terminal() {
        return Err(not_a_terminal("tui view dialog"));
    }
    let (common, mut dialog) = args.kind.parts();
    // From here on this process owns the terminal (commands.md §1.2).
    let mut terminal = ratatui::init();
    let mouse = ratatui::crossterm::execute!(std::io::stdout(), EnableMouseCapture);
    let result = mouse.and_then(|()| show(&mut terminal, &mut dialog));
    let _ = ratatui::crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    let outcome = result.map_err(|e| {
        CpdError::new(ErrorCode::NotATerminal, format!("The terminal of cpd tui view dialog failed: {e}."))
            .with("command", "tui view dialog")
    })?;
    write_outcome(&common.out, &outcome)?;
    Ok(Output::Owned)
}

/// Draws and takes input until the dialog closes.
fn show(terminal: &mut ratatui::DefaultTerminal, dialog: &mut Dialog) -> std::io::Result<Outcome> {
    loop {
        terminal.draw(|f| dialog.draw(f))?;
        let closed = match event::read()? {
            Event::Key(key) => dialog.press_key(key),
            Event::Mouse(mouse) => dialog.press_mouse(mouse),
            _ => None,
        };
        if let Some(outcome) = closed {
            return Ok(outcome);
        }
    }
}

fn write_outcome(path: &Path, outcome: &Outcome) -> Result<(), CpdError> {
    let json = serde_json::to_string(outcome).expect("an outcome is JSON");
    std::fs::write(path, json).map_err(|e| {
        CpdError::new(ErrorCode::Internal, format!("Cannot write the dialog's result to {}: {e}.", path.display()))
            .with("path", path.display().to_string())
    })
}

/// Reads and deletes the result file the dialog at `path` wrote. A missing or bad file is an error: the popup was
/// closed some other way, or it failed; never a cancel.
fn read_outcome(path: &Path) -> Result<Outcome, CpdError> {
    let shown = path.display().to_string();
    let bad = |why: String| CpdError::new(ErrorCode::Internal, format!("The dialog left no usable result at {shown}: {why}.")).with("path", shown.clone());
    let read = std::fs::read_to_string(path);
    let _ = std::fs::remove_file(path);
    let json = read.map_err(|e| bad(e.to_string()))?;
    let outcome: Outcome = serde_json::from_str(&json).map_err(|e| bad(e.to_string()))?;
    if !outcome.confirmed && outcome.value.is_some() {
        return Err(bad("a cancelled dialog has a value".into()));
    }
    Ok(outcome)
}

/// A new name for a result file: under the system temp dir, with the process id (and a count, for two dialogs in turn).
fn temp_path() -> PathBuf {
    static COUNT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!("cpd-dialog-{}-{}.json", std::process::id(), COUNT.fetch_add(1, Ordering::Relaxed)))
}

/// `arg` as one word for the popup's shell: in single quotes.
fn sh_quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', r"'\''"))
}

/// The shell command the popup runs: `cpd tui view dialog <kind> --title=… --out=… --<field>=…`. Each is one word in
/// the `--name=value` form, so a value that starts with `-` is a value.
fn view_command(kind: &str, title: &str, out: &Path, fields: &[(&str, &str)]) -> String {
    let mut command = format!("{PROGRAM} tui view dialog {kind} --title={} --out={}", sh_quote(title), sh_quote(&out.display().to_string()));
    for (name, value) in fields {
        command.push_str(&format!(" --{name}={}", sh_quote(value)));
    }
    command
}

/// The tmux arguments that open `command` in a popup of `size` `(columns, rows)`, border included.
fn popup_args(title: &str, size: (u16, u16), command: &str) -> Vec<String> {
    [
        "display-popup".to_owned(),
        "-E".to_owned(),
        "-T".to_owned(),
        format!(" {title} "),
        "-w".to_owned(),
        size.0.to_string(),
        "-h".to_owned(),
        size.1.to_string(),
        command.to_owned(),
    ]
    .to_vec()
}

/// The narrowest and the widest inside of a dialog; a longer line wraps.
const MIN_INNER: usize = 40;
const MAX_INNER: usize = 76;

/// The popup's size `(columns, rows)`, border included, for content `natural` columns wide whose body is
/// `body_rows(width)` rows at `width` text columns; at most the client's size. The inside is an empty row, the title, an empty
/// row, the body, a blank row and the buttons.
fn popup_size(client: (u16, u16), title: &str, natural: usize, body_rows: impl Fn(usize) -> usize) -> (u16, u16) {
    let inner = (natural + 2).max(title.chars().count() + 6).clamp(MIN_INNER, MAX_INNER);
    let rows = body_rows(inner - 2) + 5;
    let fit = |wanted: usize, cap: u16| u16::try_from(wanted + 2).unwrap_or(u16::MAX).min(cap);
    (fit(inner, client.0), fit(rows, client.1))
}

/// The width of the longest line of `text`.
fn widest(text: &str) -> usize {
    text.lines().map(|l| l.chars().count()).max().unwrap_or(0)
}

/// Opens the popup `kind` with `fields` and returns how it closed. `size` is the popup's size for the newest client's.
fn ask(
    tmux: &Tmux,
    title: &str,
    kind: &str,
    fields: &[(&str, &str)],
    size: impl FnOnce((u16, u16), &str) -> (u16, u16),
) -> Result<Outcome, CpdError> {
    let clients = tmux.run(&["list-clients", "-F", "#{client_activity} #{client_width} #{client_height}"].map(str::to_owned))?;
    let client = newest_client(&clients).ok_or_else(|| {
        CpdError::new(ErrorCode::TmuxFailed, "No tmux client is attached to the cockpit.").with("socket", tmux.socket())
    })?;
    let out = temp_path();
    let _ = std::fs::remove_file(&out);
    let command = view_command(kind, title, &out, fields);
    let ran = tmux.run(&popup_args(title, size(client, title), &command));
    let read = read_outcome(&out);
    ran?;
    read
}

/// A question with two buttons, `[<yes_label>]` and `[Cancel]`, in a popup. Whether it was confirmed.
#[allow(dead_code)] // the tree's Close, Archive and `x` (Job kit-tree)
pub(crate) fn confirm(tmux: &Tmux, title: &str, text: &str, yes_label: &str) -> Result<bool, CpdError> {
    let fields = [("text", text), ("yes-label", yes_label)];
    let size = |client, title: &str| {
        let natural = widest(text).max(yes_label.chars().count() + CANCEL.len() + 8);
        popup_size(client, title, natural, |width| wrap(text, width).len())
    };
    Ok(ask(tmux, title, "confirm", &fields, size)?.confirmed)
}

/// A one-line field in a popup, starting with `initial` under the question `prompt`; Enter accepts a value that
/// passes `validator`, a refused one shows why and the popup stays. `None` when the User cancelled.
#[allow(dead_code)] // the tree's New…, Rename…, Fork… and `n` (Job kit-tree)
pub(crate) fn input(tmux: &Tmux, title: &str, prompt: &str, initial: &str, validator: Validator) -> Result<Option<String>, CpdError> {
    let fields = [("prompt", prompt), ("initial", initial), ("validate", validator.as_arg())];
    let size = |client, title: &str| {
        let natural = widest(prompt).max(initial.chars().count() + 1);
        popup_size(client, title, natural, |width| wrap(prompt, width).len() + 1 + REFUSAL_ROWS)
    };
    let outcome = ask(tmux, title, "input", &fields, size)?;
    match (outcome.confirmed, outcome.value) {
        (false, _) => Ok(None),
        (true, Some(value)) => Ok(Some(value)),
        (true, None) => Err(CpdError::new(ErrorCode::Internal, "The input dialog confirmed with no value.")),
    }
}

/// Read-only `body`, scrollable, in a popup, until the User closes it.
#[allow(dead_code)] // the callers of Jobs kit-tree and kit-scroll
pub(crate) fn text(tmux: &Tmux, title: &str, body: &str) -> Result<(), CpdError> {
    let size = |client, title: &str| popup_size(client, title, widest(body), |width| wrap(body, width).len());
    ask(tmux, title, "text", &[("body", body)], size).map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_outcome_is_a_small_json_value() {
        let json = |o: &Outcome| serde_json::to_string(o).unwrap();
        assert_eq!(json(&Outcome::confirmed()), r#"{"confirmed":true}"#);
        assert_eq!(json(&Outcome::cancelled()), r#"{"confirmed":false}"#);
        assert_eq!(json(&Outcome::value("a \"b\"".into())), r#"{"confirmed":true,"value":"a \"b\""}"#);
    }

    #[test]
    fn a_result_file_is_read_and_deleted() {
        let dir = tempfile::tempdir().unwrap();
        for outcome in [Outcome::confirmed(), Outcome::cancelled(), Outcome::value("v é".into())] {
            let path = dir.path().join("out.json");
            write_outcome(&path, &outcome).unwrap();
            assert_eq!(read_outcome(&path).unwrap(), outcome);
            assert!(!path.exists());
        }
    }

    #[test]
    fn a_missing_or_bad_result_file_is_an_error_never_a_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.json");
        assert_eq!(read_outcome(&path).unwrap_err().code(), ErrorCode::Internal);
        for bad in ["", "{", "[]", r#"{"confirmed":"yes"}"#, r#"{}"#, r#"{"confirmed":false,"value":"x"}"#, r#"{"confirmed":true,"extra":1}"#] {
            std::fs::write(&path, bad).unwrap();
            let err = read_outcome(&path).unwrap_err();
            assert_eq!(err.code(), ErrorCode::Internal, "{bad:?}");
            assert!(!path.exists(), "{bad:?}: the file is deleted");
        }
    }

    #[test]
    fn a_result_file_that_cannot_be_written_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = write_outcome(&dir.path().join("no/such/dir/out.json"), &Outcome::confirmed()).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Internal);
    }

    #[test]
    fn temporary_files_are_under_the_temp_dir_with_the_process_id_and_differ() {
        let (a, b) = (temp_path(), temp_path());
        assert_ne!(a, b);
        assert!(a.starts_with(std::env::temp_dir()));
        assert!(a.file_name().unwrap().to_str().unwrap().starts_with(&format!("cpd-dialog-{}-", std::process::id())));
    }

    #[test]
    fn the_popup_runs_the_view_with_every_value_quoted_and_named_by_equals() {
        let command = view_command("confirm", "Archive", Path::new("/tmp/o.json"), &[("text", "It's -x?"), ("yes-label", "Archive")]);
        assert_eq!(
            command,
            r"cpd tui view dialog confirm --title='Archive' --out='/tmp/o.json' --text='It'\''s -x?' --yes-label='Archive'"
        );
        assert_eq!(
            popup_args("Archive", (50, 7), &command),
            ["display-popup", "-E", "-T", " Archive ", "-w", "50", "-h", "7", command.as_str()]
        );
    }

    #[test]
    fn the_popup_is_sized_to_its_content_and_capped_at_the_client() {
        let client = (200, 60);
        // short content: the narrowest inside (40), empty + title + empty + 1 row + blank + buttons = 6, and the border
        assert_eq!(popup_size(client, "T", 10, |_| 1), (42, 8));
        // long content: the widest inside (76), wrapped
        assert_eq!(popup_size(client, "T", 300, |w| 300usize.div_ceil(w)), (78, 5 + 5 + 2));
        // a long title widens it
        assert_eq!(popup_size(client, &"t".repeat(50), 10, |_| 1).0, 58);
        // capped
        assert_eq!(popup_size((30, 5), "T", 300, |_| 40), (30, 5));
    }

    #[test]
    fn the_widest_line_is_the_longest_in_characters() {
        let (a, b) = (widest("ab\nabcd\nabc"), widest(""));
        assert_eq!((a, b), (4, 0));
    }

    #[test]
    fn the_command_line_takes_the_kinds_and_their_fields() {
        use clap::Parser;
        #[derive(Parser, Debug)]
        struct Cli {
            #[command(flatten)]
            args: Args,
        }
        let parse = |argv: &[&str]| Cli::try_parse_from(std::iter::once("dialog").chain(argv.iter().copied()));
        assert!(parse(&["confirm", "--title=T", "--out=/o", "--text=Q?"]).is_ok());
        assert!(parse(&["confirm", "--title=T", "--out=/o", "--text=-Q?", "--yes-label=Go"]).is_ok());
        assert!(parse(&["input", "--title=T", "--out=/o", "--prompt=P", "--initial=", "--validate=plan-id"]).is_ok());
        assert!(parse(&["input", "--title=T", "--out=/o", "--prompt=P", "--validate=name"]).is_ok());
        assert!(parse(&["text", "--title=T", "--out=/o", "--body=B"]).is_ok());
        for bad in [
            &["input", "--title=T", "--out=/o", "--prompt=P"][..],
            &["input", "--title=T", "--out=/o", "--prompt=P", "--validate=anything"],
            &["confirm", "--title=T", "--text=Q"],
            &["text", "--out=/o", "--body=B"],
            &["other", "--title=T", "--out=/o"],
            &[],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }
}
