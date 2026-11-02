//! The `settings` view (commands.md §13.2), in a tmux popup opened by the StatusBar's settings button
//! (`cpd tui action settings`). For now a popup with the section title `Agent Tiers`; `q` or Esc closes it.

use ratatui::crossterm::event::{Event, KeyCode, KeyEventKind};

#[allow(dead_code)] // stub: only the tests call it until the implementation lands
mod model;
#[cfg(test)]
mod model_tests;
#[allow(dead_code)] // stub: the implementation fills it in, only the tests call it
mod view;
#[cfg(test)]
mod view_tests;

use super::box_widget::SectionTitle;
use super::view_ports::{Input, RunFn, Tty};
use super::{Capture, RunCommand, popup, run_view_on};
use cpd_core::error::CpdError;
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::session::PROGRAM;
use ratatui::Terminal;
use ratatui::backend::Backend;
use std::io::Write;

/// The section's title.
const TITLE: &str = "Agent Tiers";
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

/// `cpd tui view settings`: draws the title until `q` or Esc.
pub(crate) fn run(ports: &Ports, _run_command: RunCommand) -> Result<Output, CpdError> {
    run_view_on(
        ports,
        "settings",
        Capture::Mouse,
        || Ok(()),
        |ctx, ()| {
            loop {
                ctx.draw(|f| f.render_widget(SectionTitle::new(TITLE, None), f.area()))?;
                if let Event::Key(k) = ctx.input.read()?
                    && k.kind == KeyEventKind::Press
                    && matches!(k.code, KeyCode::Esc | KeyCode::Char('q'))
                {
                    return Ok(());
                }
            }
        },
    )
}

/// [`run`] on a terminal, its facts and its input that the caller gives, and the commands it runs through `run_command`
/// (a test's `TestBackend`, scripted input and a fake [`RunFn`]).
#[allow(dead_code)] // stub: the implementation fills it in, only the tests call it
pub fn run_on<B: Backend, W: Write>(
    _ports: &Ports,
    _run_command: RunFn,
    _tty: &mut dyn Tty,
    _out: &mut W,
    _terminal: Terminal<B>,
    _input: &mut dyn Input,
) -> Result<Output, CpdError> {
    todo!("run_on: the implementation Job")
}
