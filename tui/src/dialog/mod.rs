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
#[cfg(test)]
pub(crate) use view::layout_sample;
#[cfg(test)]
mod scroll_width_tests;
#[cfg(test)]
mod input_tests;
#[cfg(test)]
mod select_tests;
#[cfg(test)]
mod form_tests;
#[cfg(test)]
mod pane;
#[cfg(test)]
mod pane_tests;
#[cfg(test)]
mod checklist_tests;
#[cfg(test)]
mod view_width_tests;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ratatui::backend::Backend;
use ratatui::crossterm::event::Event;
use serde::{Deserialize, Serialize};

use super::cockpit::{client_size, on_client};
use super::popup;
use super::viewlog::ViewLog;
use super::view_ports::Ctx;
use super::{Capture, run_view_on};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::Tmux;
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::session::PROGRAM;
use crate::box_widget::spacing;
use validate::Validator;
pub use view::{Dialog, Mode};
use view::{CANCEL, Field, REFUSAL_ROWS, Row, wrap};

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
    /// A labelled text box that grows with its text
    Input(Input),
    /// Scrollable read-only text
    Text(Text),
    /// Entries to tick, with `[Approve]` and `[Later]`; the outcome is `{approved, ticked, unticked}`
    Checklist(Checklist),
    /// A radio list of options, with `[OK]` and `[Cancel]`; the outcome is `{confirmed, value}`
    Select(Select),
    /// A labelled text box and a labelled radio list, with `[OK]` and `[Cancel]`; the outcome is `{confirmed, value, choice}`
    Form(Form),
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

#[derive(clap::Args, Debug)]
struct Checklist {
    #[command(flatten)]
    common: Common,
    /// A JSON file: an array of `{id, text, detail}`
    #[arg(long, value_name = "path")]
    items: PathBuf,
}

#[derive(clap::Args, Debug)]
struct Select {
    #[command(flatten)]
    common: Common,
    /// The question above the options
    #[arg(long)]
    prompt: String,
    /// An option, repeated, in the order shown
    #[arg(long = "option", required = true)]
    options: Vec<String>,
}

#[derive(clap::Args, Debug)]
struct Form {
    #[command(flatten)]
    common: Common,
    /// The label of the text box
    #[arg(long)]
    input_label: String,
    /// The label of the radio list
    #[arg(long)]
    select_label: String,
    /// An option, repeated, in the order shown
    #[arg(long = "option", required = true)]
    options: Vec<String>,
}

/// The options of a `select` on the command line: at least one, each non-empty, one line, no control character, at most
/// 70 characters, all distinct; otherwise `args_invalid`.
fn check_options(options: &[String]) -> Result<(), CpdError> {
    let bad = |why: String| CpdError::new(ErrorCode::ArgsInvalid, format!("Cannot use the select options: {why}."));
    if options.is_empty() {
        return Err(bad("there is none".into()));
    }
    for (n, o) in options.iter().enumerate() {
        if o.is_empty() || o.chars().any(char::is_control) || o.chars().count() > 70 {
            return Err(bad(format!("{o:?} is empty, longer than 70 characters or has a control character")));
        }
        if options[..n].contains(o) {
            return Err(bad(format!("{o:?} is repeated")));
        }
    }
    Ok(())
}

/// How a dialog closed: the result file's content. `value` is the typed text of an accepted `input`, and only then.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    confirmed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<String>,
    /// The option chosen in a `form`, and only then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    choice: Option<String>,
}

impl Outcome {
    pub fn confirmed() -> Outcome {
        Outcome { confirmed: true, value: None, choice: None }
    }

    pub fn cancelled() -> Outcome {
        Outcome { confirmed: false, value: None, choice: None }
    }

    pub fn value(value: String) -> Outcome {
        Outcome { confirmed: true, value: Some(value), choice: None }
    }

    /// An accepted `form`: the typed text and the option chosen.
    pub fn form(value: String, choice: String) -> Outcome {
        Outcome { confirmed: true, value: Some(value), choice: Some(choice) }
    }
}

impl Kind {
    fn parts(self) -> Result<(Common, Dialog), CpdError> {
        Ok(match self {
            Kind::Checklist(a) => {
                let bad = |why: String| {
                    CpdError::new(ErrorCode::ArgsInvalid, format!("Cannot read the checklist items {}: {why}.", a.items.display()))
                        .with("path", a.items.display().to_string())
                };
                let json = std::fs::read_to_string(&a.items).map_err(|e| bad(e.to_string()))?;
                let rows: Vec<Row> = serde_json::from_str(&json).map_err(|e| bad(e.to_string()))?;
                let ticked = vec![false; rows.len()];
                let dialog = Dialog::new(a.common.title.clone(), String::new(), Mode::Checklist { rows, ticked, cursor: 0 });
                (a.common, dialog)
            }
            Kind::Confirm(a) => {
                let dialog = Dialog::new(a.common.title.clone(), a.text, Mode::Confirm { yes: a.yes_label });
                (a.common, dialog)
            }
            Kind::Input(a) => {
                let mode = Mode::Input { validator: a.validate, field: Field::new(&a.initial), refused: None };
                let dialog = Dialog::new(a.common.title.clone(), a.prompt, mode);
                (a.common, dialog)
            }
            Kind::Select(a) => {
                check_options(&a.options)?;
                let dialog = Dialog::new(a.common.title.clone(), a.prompt, Mode::Select { options: a.options, cursor: 0 });
                (a.common, dialog)
            }
            Kind::Form(a) => {
                // skeleton (Job dlg-tests): the options are not checked yet
                let dialog = Dialog::form(a.common.title.clone(), a.input_label, a.select_label, a.options);
                (a.common, dialog)
            }
            Kind::Text(a) => {
                let dialog = Dialog::new(a.common.title.clone(), a.body, Mode::Text);
                (a.common, dialog)
            }
        })
    }
}

pub(super) fn run(ports: &Ports, args: Args) -> Result<Output, CpdError> {
    // The arguments are parsed at the edge, before the terminal is asked: a bad items file is refused on any stdout.
    let parts = args.kind.parts()?;
    let mut ending: Option<(Common, Dialog, Outcome)> = None;
    run_view_on(
        ports,
        "dialog",
        Capture::Mouse,
        move || Ok(parts),
        |ctx, (common, mut dialog)| {
            let outcome = show_with(ctx, &mut dialog)?;
            ending = Some((common, dialog, outcome));
            Ok(())
        },
    )?;
    // The terminal is restored; the result file is written from what the body kept.
    let (common, dialog, outcome) = ending.expect("the body ran");
    write_result(&common.out, &dialog, &outcome)?;
    Ok(Output::Owned)
}

/// Writes how `dialog` closed to `path`: `{approved, ticked, unticked}` for a checklist, the [`Outcome`] for the others.
fn write_result(path: &Path, dialog: &Dialog, outcome: &Outcome) -> Result<(), CpdError> {
    match dialog.ticks() {
        Some((ticked, unticked)) => {
            let json = serde_json::json!({"approved": outcome.confirmed, "ticked": ticked, "unticked": unticked});
            write_json(path, &json.to_string())
        }
        None => write_outcome(path, outcome),
    }
}

/// Draws and takes input until the dialog closes: input and frames are the [`Ctx`]'s.
pub fn show_with<B: Backend>(ctx: &mut Ctx<B>, dialog: &mut Dialog) -> std::io::Result<Outcome> {
    loop {
        ctx.draw(|f| dialog.draw(f))?;
        if let Some(outcome) = dispatch(dialog, ctx.input.read()?) {
            return Ok(outcome);
        }
    }
}

/// What `event` does to `dialog`: the outcome that closes it, if any.
fn dispatch(dialog: &mut Dialog, event: Event) -> Option<Outcome> {
    match event {
        Event::Key(key) => dialog.press_key(key),
        Event::Mouse(mouse) => dialog.press_mouse(mouse),
        _ => None,
    }
}

fn write_outcome(path: &Path, outcome: &Outcome) -> Result<(), CpdError> {
    write_json(path, &serde_json::to_string(outcome).expect("an outcome is JSON"))
}

fn write_json(path: &Path, json: &str) -> Result<(), CpdError> {
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

/// The shell command the popup runs: `cpd tui view dialog <kind> --title=… --out=… --<field>=…`. Each is one word in
/// the `--name=value` form, so a value that starts with `-` is a value.
fn view_command(kind: &str, title: &str, out: &Path, fields: &[(&str, &str)]) -> String {
    let mut command = format!("{PROGRAM} tui view dialog {kind} --title={} --out={}", popup::sh_quote(title), popup::sh_quote(&out.display().to_string()));
    for (name, value) in fields {
        command.push_str(&format!(" --{name}={}", popup::sh_quote(value)));
    }
    command
}

/// The tmux arguments that open `command` in a popup of `size` `(columns, rows)`, with no tmux border (`-B`): the view draws its own frame.
fn popup_args(size: (u16, u16), command: &str) -> Vec<String> {
    popup::args(popup::Popup { title: None, border: true, client: None, dash: false, size, command })
}

/// The narrowest and the widest inside of a dialog; a longer line wraps.
const MIN_INNER: usize = 40;
const MAX_INNER: usize = 76;

/// The popup's size `(columns, rows)` for content `natural` columns wide whose body is `body_rows(width)` rows at `width`
/// text columns. The popup is exactly the dialog's frame: `inside + 2` columns, at most the client's width less one
/// column of margin on each side, and `body + 5` rows (the top border, a blank row, the body, a blank row, the buttons and
/// the bottom border), at most the client's height.
fn popup_size(client: (u16, u16), title: &str, natural: usize, body_rows: impl Fn(usize) -> usize) -> (u16, u16) {
    let inner = (natural + 2).max(title.chars().count() + 6).clamp(MIN_INNER, MAX_INNER);
    let rows = body_rows(inner - 2) + 5;
    let to_u16 = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
    (to_u16(inner + 2).min(client.0.saturating_sub(2 * spacing::MARGIN_X)).max(1), to_u16(rows).min(client.1))
}

/// The popup's size `(columns, rows)` for an `input`: wide enough for the label and the initial text, tall enough for the
/// longest value the validator accepts (commands.md §13.2).
fn input_popup_size(client: (u16, u16), title: &str, prompt: &str, initial: &str, validator: Validator) -> (u16, u16) {
    let initial = initial.chars().count();
    let natural = view::label(prompt).chars().count().max(initial) + 5;
    popup_size(client, title, natural, |text_width| {
        let tw = text_width - 4 * spacing::PAD_X as usize;
        validator.max_len().max(initial) / tw + 1 + 2 + REFUSAL_ROWS
    })
}

/// The width of the longest line of `text`.
fn widest(text: &str) -> usize {
    text.lines().map(|l| l.chars().count()).max().unwrap_or(0)
}

/// The client a popup opens on: the most recently active one, or the one named (`#{client_name}`), for a caller that
/// knows its client (a `cpd tui action`, commands.md §13.2).
#[derive(Clone, Copy, Debug)]
enum On<'a> {
    Newest,
    Client(&'a str),
}

/// Opens the popup `kind` with `fields` on the client `on` and returns how it closed. `size` is the popup's size for
/// that client's.
fn ask(
    tmux: &Tmux,
    on: On<'_>,
    title: &str,
    kind: &str,
    fields: &[(&str, &str)],
    size: impl FnOnce((u16, u16), &str) -> (u16, u16),
) -> Result<Outcome, CpdError> {
    ask_with(tmux, on, title, kind, fields, size, read_outcome)
}

/// [`ask`] for a kind whose result file `read` parses.
fn ask_with<T>(
    tmux: &Tmux,
    on: On<'_>,
    title: &str,
    kind: &str,
    fields: &[(&str, &str)],
    size: impl FnOnce((u16, u16), &str) -> (u16, u16),
    read: impl FnOnce(&Path) -> Result<T, CpdError>,
) -> Result<T, CpdError> {
    let client = match on {
        On::Newest => {
            let clients =
                tmux.run(&["list-clients", "-F", "#{client_activity} #{client_width} #{client_height}"].map(str::to_owned))?;
            popup::newest_client(&clients).map(|(_, size)| size).ok_or_else(|| {
                CpdError::new(ErrorCode::TmuxFailed, "No tmux client is attached to the cockpit.").with("socket", tmux.socket())
            })?
        }
        On::Client(name) => client_size(tmux, name)?,
    };
    let out = temp_path();
    let _ = std::fs::remove_file(&out);
    let command = view_command(kind, title, &out, fields);
    let popup = popup_args(size(client, title), &command);
    let popup = match on {
        On::Newest => popup,
        On::Client(name) => on_client(popup, name),
    };
    let ran = tmux.run(&popup);
    let read = read(&out);
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
    Ok(ask(tmux, On::Newest, title, "confirm", &fields, size)?.confirmed)
}

/// A one-line field in a popup, starting with `initial` under the question `prompt`; Enter accepts a value that
/// passes `validator`, a refused one shows why and the popup stays. `None` when the User cancelled.
#[allow(dead_code)] // the tree's New…, Rename…, Fork… and `n` (Job kit-tree)
pub(crate) fn input(tmux: &Tmux, title: &str, prompt: &str, initial: &str, validator: Validator) -> Result<Option<String>, CpdError> {
    let fields = [("prompt", prompt), ("initial", initial), ("validate", validator.as_arg())];
    let size = |client, title: &str| input_popup_size(client, title, prompt, initial, validator);
    let outcome = ask(tmux, On::Newest, title, "input", &fields, size)?;
    match (outcome.confirmed, outcome.value) {
        (false, _) => Ok(None),
        (true, Some(value)) => Ok(Some(value)),
        (true, None) => Err(CpdError::new(ErrorCode::Internal, "The input dialog confirmed with no value.")),
    }
}

/// A radio list of `options` in a popup, under the question `prompt`. The option chosen, `None` when the User cancelled.
#[allow(dead_code)] // the tree's New Context… (Job tui-nc)
pub(crate) fn select(tmux: &Tmux, title: &str, prompt: &str, options: &[String]) -> Result<Option<String>, CpdError> {
    let mut fields = vec![("prompt", prompt)];
    fields.extend(options.iter().map(|o| ("option", o.as_str())));
    let natural = widest(prompt).max(options.iter().map(|o| 4 + o.chars().count()).max().unwrap_or(0));
    let size = |client, title: &str| popup_size(client, title, natural, |width| wrap(prompt, width).len() + options.len());
    let outcome = ask(tmux, On::Newest, title, "select", &fields, size)?;
    match (outcome.confirmed, outcome.value) {
        (false, _) => Ok(None),
        (true, Some(value)) => Ok(Some(value)),
        (true, None) => Err(CpdError::new(ErrorCode::Internal, "The select dialog confirmed with no value.")),
    }
}

/// What a `checklist` closed with: whether it was approved, and the ids ticked and not.
#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Checked {
    pub(crate) approved: bool,
    pub(crate) ticked: Vec<String>,
    pub(crate) unticked: Vec<String>,
}

/// Entries `[ ] <id> — <detail>` (`rows` are `(id, text, detail)`), the full text wrapped under each, to tick in a popup,
/// `[Approve]` and `[Later]`.
pub(crate) fn checklist(tmux: &Tmux, title: &str, rows: &[(String, String, String)]) -> Result<Checked, CpdError> {
    let items: Vec<serde_json::Value> =
        rows.iter().map(|(id, text, detail)| serde_json::json!({"id": id, "text": text, "detail": detail})).collect();
    let file = temp_path().with_extension("items.json");
    std::fs::write(&file, serde_json::Value::Array(items).to_string()).map_err(|e| {
        CpdError::new(ErrorCode::Internal, format!("Cannot write the checklist items to {}: {e}.", file.display()))
    })?;
    let natural = rows
        .iter()
        .map(|(id, text, detail)| format!("[ ] {id} — {detail}").chars().count().max(widest(text) + view::TEXT_INDENT))
        .max()
        .unwrap_or(0);
    let body_rows = |width: usize| {
        let entries: usize = rows.iter().map(|(_, text, _)| 1 + wrap(text, width.saturating_sub(view::TEXT_INDENT)).len()).sum();
        (entries + rows.len().saturating_sub(1)).max(1)
    };
    let size = |client, title: &str| popup_size(client, title, natural, body_rows);
    let path = file.display().to_string();
    let asked = ask_with(tmux, On::Newest, title, "checklist", &[("items", &path)], size, |out| {
        let shown = out.display().to_string();
        let bad = |why: String| CpdError::new(ErrorCode::Internal, format!("The dialog left no usable result at {shown}: {why}."));
        let read = std::fs::read_to_string(out);
        let _ = std::fs::remove_file(out);
        serde_json::from_str(&read.map_err(|e| bad(e.to_string()))?).map_err(|e| bad(e.to_string()))
    });
    let _ = std::fs::remove_file(&file);
    asked
}

/// Read-only `body`, scrollable, in a popup on the client `on`, until the User closes it.
fn text_on(tmux: &Tmux, on: On<'_>, title: &str, body: &str) -> Result<(), CpdError> {
    let size = |client, title: &str| popup_size(client, title, widest(body), |width| wrap(body, width).len());
    ask(tmux, on, title, "text", &[("body", body)], size).map(drop)
}

/// Read-only `body`, scrollable, in a popup, until the User closes it.
pub(crate) fn text(tmux: &Tmux, title: &str, body: &str) -> Result<(), CpdError> {
    text_on(tmux, On::Newest, title, body)
}

/// A failure the User must see (commands.md §17 "No notes"): `title` says what failed, `text` why
/// (`<code>: <message>`), in a `text` popup until the User closes it. The one entry point every view and command opens
/// a failure with, so that no view draws a note.
pub(crate) fn show_error(tmux: &Tmux, title: &str, text: &str) -> Result<(), CpdError> {
    self::text(tmux, title, text)
}

/// [`show_error`] on the client named `client` (`display-popup -c`), for a caller that runs with no view of its own
/// and knows its client: a `cpd tui action` (commands.md §13.2, the `merge` action).
pub(crate) fn show_error_on(tmux: &Tmux, client: &str, title: &str, text: &str) -> Result<(), CpdError> {
    text_on(tmux, On::Client(client), title, text)
}

/// A command of a view failed (commands.md §17 "No notes"): a line of the view's `log`, and the error dialog, `what`
/// failed and `err` says why. A log that cannot be written is said in the dialog; a dialog that cannot open is
/// logged: there is nothing else to show it with.
pub(crate) fn report(tmux: &Tmux, log: &ViewLog, what: &str, err: &CpdError) {
    report_why(tmux, log, what, format!("{}: {}", err.code(), err.message()));
}

/// [`report`] of a failure already said in words (`<code>: <message>`).
pub(crate) fn report_why(tmux: &Tmux, log: &ViewLog, what: &str, mut why: String) {
    if let Err(unwritten) = log.append(&format!("failed: {what}: {why}")) {
        why = format!("{why}\n{unwritten}");
    }
    if let Err(unshown) = show_error(tmux, what, &why) {
        let _ = log.append(&format!("failed: dialog: {}", unshown.message()));
    }
}

#[cfg(test)]
mod tests;
