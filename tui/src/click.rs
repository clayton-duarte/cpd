//! `cpd tui click <reset|legend|logs> --client <name>` (commands.md §13.2): what a click on one of the status line's
//! buttons does. The status line's root binding runs it through `run-shell -b` with the clicking client's name.
//!
//! It never writes to stdout or stderr and always exits 0: a failing `run-shell` would put the chat pane into view
//! mode. A failure is shown on the clicking client with `display-message -c`, and a name that is not a button (the
//! binding also fires for a click on the line's empty middle, whose range is `none` or empty) does nothing.

use clap::ValueEnum;
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;
use cpd_core::output::Output;

use super::widgets::legend;
use super::{layout, logs};

/// The status line's buttons: the names of their `#[range=user|…]` ranges.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum Button {
    /// ⧉: reset-layout
    Reset,
    /// ?: the key legend, in a popup
    Legend,
    /// ▤: the Event log, in a popup
    Logs,
}

#[derive(clap::Args, Debug)]
pub struct ClickArgs {
    /// `reset`, `legend` or `logs`; anything else, or nothing, is a click on no button and does nothing
    button: Option<String>,
    /// The tmux client that clicked (`#{client_name}`)
    #[arg(long)]
    client: String,
}

pub(super) fn run(args: ClickArgs) -> Result<Output, CpdError> {
    let Some(button) = args.button.as_deref().and_then(|b| Button::from_str(b, true).ok()) else {
        return Ok(Output::Owned);
    };
    // Without a config there is no socket to show anything through.
    let Ok(config) = CpdHome::from_env().and_then(|home| Config::load(&home)) else {
        return Ok(Output::Owned);
    };
    let tmux = Tmux::new(config.tmux.socket.clone());
    if let Err(e) = click(&tmux, &config.tmux.cockpit_session, button, &args.client) {
        // Nowhere left to report it if this fails too.
        let _ = tmux.run(&display_message_args(&args.client, e.message()));
    }
    Ok(Output::Owned)
}

fn click(tmux: &Tmux, cockpit: &str, button: Button, client: &str) -> Result<(), CpdError> {
    match button {
        Button::Reset => layout::reset(tmux, cockpit),
        Button::Legend => popup(tmux, client, legend::popup_args),
        Button::Logs => popup(tmux, client, logs::popup_args),
    }
}

/// Opens a popup on `client`, sized by that client's own width and height.
fn popup(tmux: &Tmux, client: &str, args: fn((u16, u16)) -> Vec<String>) -> Result<(), CpdError> {
    let size = tmux.run(&strings(&["display", "-p", "-c", client, "#{client_width} #{client_height}"]))?;
    let size = parse_size(&size).ok_or_else(|| {
        CpdError::new(ErrorCode::TmuxFailed, format!("tmux gave no size for client {client}."))
            .with("client", client)
            .with("output", size.trim())
    })?;
    tmux.run(&on_client(args(size), client)).map(drop)
}

/// `<width> <height>`.
fn parse_size(text: &str) -> Option<(u16, u16)> {
    let mut fields = text.split_whitespace().map(str::parse::<u16>);
    match (fields.next(), fields.next(), fields.next()) {
        (Some(Ok(w)), Some(Ok(h)), None) => Some((w, h)),
        _ => None,
    }
}

/// `display-popup <rest>` as `display-popup -c <client> <rest>`.
fn on_client(mut args: Vec<String>, client: &str) -> Vec<String> {
    args.splice(1..1, ["-c".to_owned(), client.to_owned()]);
    args
}

/// `display-message` expands formats in its message, so a `#` in it is doubled.
fn display_message_args(client: &str, message: &str) -> Vec<String> {
    strings(&["display-message", "-c", client, &message.replace('#', "##")])
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| (*a).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_popup_goes_to_the_client() {
        assert_eq!(
            on_client(legend::popup_args((300, 100)), "/dev/pts/3")[..4],
            ["display-popup", "-c", "/dev/pts/3", "-E"]
        );
    }

    #[test]
    fn the_size_is_two_numbers() {
        assert_eq!(parse_size("215 62\n"), Some((215, 62)));
        assert_eq!(parse_size(""), None);
        assert_eq!(parse_size("215"), None);
        assert_eq!(parse_size("215 62 1"), None);
        assert_eq!(parse_size("a b"), None);
    }

    #[test]
    fn a_hash_in_the_message_is_not_a_format() {
        assert_eq!(display_message_args("c", "x #{y}")[3], "x ##{y}");
    }
}
