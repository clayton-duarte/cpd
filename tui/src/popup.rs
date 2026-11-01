//! The one builder of tmux `display-popup` arguments (audit-plan F2, audit-shell F6; hs-d599): every popup
//! (`dialog`, `merge`, the legend, the Logs view) assembles its argument vector through [`args`], so the shape
//! lives in one place. The module also owns the one [`sh_quote`] for values spliced into a popup's shell command
//! and the one [`newest_client`] parse of `list-clients`' lines.

/// `arg` as one word for the popup's shell: in single quotes.
pub(crate) fn sh_quote(arg: &str) -> String {
    format!("'{}'", arg.replace('\'', r"'\''"))
}

/// What a `display-popup` call differs by: the tmux border title (`-T`) or none, the tmux border (`-B`, which the
/// dialogs and merge turn off to draw their own frame), the client to open on (`-c`) when the caller places it
/// here (merge; the cockpit splices `-c` at index 1 for the views it aims, `cockpit::on_client`), a `--` before
/// the command (merge), and the size `(columns, rows)`.
pub(crate) struct Popup<'a> {
    pub(crate) title: Option<&'a str>,
    pub(crate) border: bool,
    pub(crate) client: Option<&'a str>,
    pub(crate) dash: bool,
    pub(crate) size: (u16, u16),
    pub(crate) command: &'a str,
}

/// The tmux arguments that open `popup`'s command in a `display-popup`.
pub(crate) fn args(popup: Popup<'_>) -> Vec<String> {
    let mut args = vec!["display-popup".to_owned(), "-E".to_owned()];
    if popup.border {
        args.push("-B".to_owned());
    }
    if let Some(client) = popup.client {
        args.extend(["-c".to_owned(), client.to_owned()]);
    }
    if let Some(title) = popup.title {
        args.extend(["-T".to_owned(), title.to_owned()]);
    }
    args.extend([
        "-w".to_owned(),
        popup.size.0.to_string(),
        "-h".to_owned(),
        popup.size.1.to_string(),
    ]);
    if popup.dash {
        args.push("--".to_owned());
    }
    args.push(popup.command.to_owned());
    args
}

/// The newest attached client, with its name when the listing names one and its size `(columns, rows)`, from
/// `list-clients`' `#{client_activity} #{client_width} #{client_height} [#{client_name}]` lines. A caller that
/// wants only the size (the legend, the dialogs) drops the name; one that wants the name (merge) requires it. A
/// line whose activity, width or height does not parse is skipped.
pub(crate) fn newest_client(listing: &str) -> Option<(Option<String>, (u16, u16))> {
    listing
        .lines()
        .filter_map(|line| {
            let mut words = line.splitn(4, ' ');
            let activity: u64 = words.next()?.parse().ok()?;
            let size = (words.next()?.parse().ok()?, words.next()?.parse().ok()?);
            Some((
                activity,
                words.next().filter(|n| !n.is_empty()).map(str::to_owned),
                size,
            ))
        })
        .max_by_key(|(activity, _, _)| *activity)
        .map(|(_, name, size)| (name, size))
}
