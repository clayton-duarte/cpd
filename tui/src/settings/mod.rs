//! The `settings` view (commands.md §13.2), in a tmux popup opened by the StatusBar's settings button
//! (`cpd tui action settings`): the Agent Tiers section. It reads and writes only through cpd commands.

use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind};

#[cfg(test)]
mod experimental_tests;
mod model;
#[cfg(test)]
mod model_tests;
mod view;
#[cfg(test)]
mod view_tests;

use super::dialog::scroll::Scroll;
use super::view_ports::{Ctx, Input, RunFn, Tty, run_view_with};
use super::{Capture, RunCommand, ViewError, popup, run_view_on};
use cpd_core::error::CpdError;
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::session::PROGRAM;
use model::{Effect, Model, Reads, Step};
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use serde_json::Value;
use std::ffi::OsString;
use std::io::Write;

/// The popup's inner size, columns and rows.
const WIDTH: u16 = 66;
const HEIGHT: u16 = 26;
/// The popup's share of the client, in percent, at most.
const SHARE: u16 = 90;
/// The tmux border around the popup's frame.
const POPUP_BORDER: u16 = 2;

/// The tmux arguments that open the settings in a popup: 66 x 26, at most [`SHARE`] % of the client's size `(columns, rows)`.
pub fn popup_args(client: (u16, u16)) -> Vec<String> {
    let cap = |n: u16| (u32::from(n) * u32::from(SHARE) / 100) as u16;
    let size = (
        (WIDTH + POPUP_BORDER).min(cap(client.0)),
        (HEIGHT + POPUP_BORDER).min(cap(client.1)),
    );
    popup::args(popup::Popup {
        title: Some(" settings "),
        border: false,
        client: None,
        dash: false,
        size,
        command: &format!("{PROGRAM} tui view settings"),
    })
}

/// `cpd tui view settings`: the dialog, its commands run through `run_command`.
pub(crate) fn run(ports: &Ports, run_command: RunCommand) -> Result<Output, CpdError> {
    let with_program = |mut argv: Vec<OsString>| {
        argv.insert(0, OsString::from(PROGRAM));
        run_command(argv)
    };
    run_view_on(
        ports,
        "settings",
        Capture::Mouse,
        || open(&with_program),
        |ctx, model| show(ctx, &with_program, model),
    )
}

/// [`run`] on a terminal, its facts and its input that the caller gives, and the commands it runs through `run_command`
/// (a test's `TestBackend`, scripted input and a fake [`RunFn`]); the commands are the words after `cpd`.
pub fn run_on<B: Backend, W: Write>(
    ports: &Ports,
    run_command: RunFn,
    tty: &mut dyn Tty,
    out: &mut W,
    terminal: Terminal<B>,
    input: &mut dyn Input,
) -> Result<Output, CpdError> {
    run_view_with(
        ports,
        tty,
        out,
        terminal,
        input,
        "settings",
        Capture::Mouse,
        || open(run_command),
        |ctx, model| show(ctx, run_command, model),
    )
}

/// `cpd <words>` through `run_command`: its JSON.
fn read(run_command: RunFn, words: &[&str]) -> Result<Value, CpdError> {
    match run_command(words.iter().map(OsString::from).collect())? {
        Output::Success(fields) => Ok(Value::Object(fields)),
        Output::Owned => Err(CpdError::new(
            cpd_core::error::ErrorCode::Internal,
            format!("cpd {} printed no JSON value.", words.join(" ")),
        )),
    }
}

/// The three reads, before the first draw: a failure is the command's error, with the screen untouched.
fn open(run_command: RunFn) -> Result<Model, CpdError> {
    let config = read(run_command, &["config"])?;
    let tiers = read(run_command, &["tiers"])?;
    let models = read(run_command, &["models", "show"])?;
    Ok(Model::new(Reads::parse(&config, &tiers, &models)?)
        .with_experimental(model::experimental(&config)?)
        .with_concurrency(model::concurrency(&config)?)
        .at_menu())
}

/// The words of the command a Save step runs.
fn words(step: &Step) -> Vec<String> {
    match step {
        Step::PresetSet(refs) => ["preset", "set"]
            .into_iter()
            .map(str::to_owned)
            .chain(
                ["--O", "--S", "--H", "--G"]
                    .into_iter()
                    .zip(refs)
                    .flat_map(|(flag, r)| [flag.to_owned(), r.clone()]),
            )
            .collect(),
        Step::TiersSet(t) => ["config", "tiers", "set"]
            .into_iter()
            .map(str::to_owned)
            .chain(
                [
                    ("--auxiliar", t.auxiliar),
                    ("--worker", t.worker),
                    ("--lead", t.lead),
                    ("--manager", t.manager),
                ]
                .into_iter()
                .flat_map(|(flag, tier)| [flag.to_owned(), tier.to_string()]),
            )
            .collect(),
        Step::Concurrency(n) => ["config", "concurrency", "set"]
            .into_iter()
            .map(str::to_owned)
            .chain([n.to_string()])
            .collect(),
        Step::Experimental(feature, on) => ["config", "experimental", feature.key()]
            .into_iter()
            .map(str::to_owned)
            .chain([if *on { "on" } else { "off" }.to_owned()])
            .collect(),
        Step::Retier => ["session", "retier", "--all"]
            .into_iter()
            .map(str::to_owned)
            .collect(),
    }
}

fn run_words(run_command: RunFn, words: &[String]) -> Result<Output, CpdError> {
    run_command(words.iter().map(OsString::from).collect())
}

/// The steps of Save, in order: a step that succeeded is the baseline; the first refusal stops and shows. Whether all ran.
fn save(run_command: RunFn, model: &mut Model) -> bool {
    for step in model.steps() {
        match run_words(run_command, &words(&step)) {
            Ok(_) => model.step_done(&step),
            Err(e) => {
                let e = if step == Step::Retier {
                    CpdError::new(
                        e.code(),
                        format!(
                            "{} The settings are written; the sessions were not re-tiered.",
                            e.message()
                        ),
                    )
                } else {
                    e
                };
                model.step_failed(&step, &e);
                return false;
            }
        }
    }
    true
}

/// Runs what `effect` asks; whether the view closes.
fn apply(run_command: RunFn, model: &mut Model, effect: Effect) -> bool {
    match effect {
        Effect::None => false,
        Effect::Close => true,
        Effect::Save => save(run_command, model),
        Effect::Reread(refs) => {
            let mut argv = vec!["tiers".to_owned()];
            for (letter, model_ref) in refs {
                argv.push("--model".to_owned());
                argv.push(format!("{letter}={model_ref}"));
            }
            let read = run_words(run_command, &argv).and_then(|out| match out {
                Output::Success(f) => model::catalog(&Value::Object(f)),
                Output::Owned => Err(CpdError::new(
                    cpd_core::error::ErrorCode::Internal,
                    "cpd tiers printed no JSON value.",
                )),
            });
            match read {
                Ok(catalog) => model.reread_done(catalog),
                Err(e) => model.reread_failed(&e),
            }
            false
        }
    }
}

/// What a click does.
fn click(model: &mut Model, hit: view::Hit) -> Effect {
    match hit {
        view::Hit::Value(row) => {
            if model.rows().iter().any(|r| r.row == row && r.open) {
                model.key(KeyCode::Esc.into());
            } else {
                model.open(row);
            }
            Effect::None
        }
        view::Hit::Flip(feature) => {
            model.flip(feature);
            Effect::None
        }
        view::Hit::Entry(i) => {
            let open = model.rows().into_iter().find(|r| r.open);
            let entry = model.list().and_then(|l| l.entries.get(i).cloned());
            match (open, entry) {
                (Some(open), Some(entry)) => {
                    match model.offered(open.row).iter().position(|e| *e == entry) {
                        Some(at) => model.pick(open.row, at),
                        None => Effect::None,
                    }
                }
                _ => Effect::None,
            }
        }
        view::Hit::Menu(i) => {
            model.enter(i);
            Effect::None
        }
        view::Hit::Concurrency => {
            model.edit_concurrency();
            Effect::None
        }
        view::Hit::Load => model.load(),
        view::Hit::Save => model.save(),
        view::Hit::Cancel => Effect::Close,
    }
}

/// Draws and handles input until the view closes: Esc, `q` and Cancel close without writing; Save closes when its steps ran.
fn show<B: Backend>(
    ctx: &mut Ctx<B>,
    run_command: RunFn,
    mut model: Model,
) -> Result<(), ViewError> {
    let mut scroll = Scroll::default();
    let mut follow = true;
    let mut area = Rect::ZERO;
    loop {
        ctx.draw(|f| {
            area = f.area();
            view::draw(f, &model, &mut scroll, follow);
        })?;
        follow = false;
        let effect = match ctx.input.read()? {
            Event::Key(k) if k.kind == KeyEventKind::Press => {
                follow = true;
                model.key(k)
            }
            Event::Mouse(m) if scroll.mouse(m) => Effect::None,
            Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                follow = true;
                view::hit(&model, &scroll, area, m).map_or(Effect::None, |h| click(&mut model, h))
            }
            _ => Effect::None,
        };
        if apply(run_command, &mut model, effect) {
            return Ok(());
        }
    }
}
