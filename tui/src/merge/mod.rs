//! The merge (commands.md §13.2; hs-d273, hs-d473; pro-d111): no confirmation. The Deploy column's merge icon, the PR menu's
//! Merge and the Inbox `pr-ready` item's Merge call [`start`], which runs `cpd tui action merge <owner>/<repo>#<n>
//! --client <name>` with tmux `run-shell -b` at once.
//!
//! - [`start`]: the merge, started on the newest client;
//! - [`action`]: the merge it starts, in the background: `pr merge`, then `pr poll`; a failed merge opens
//!   the cockpit's one failure dialog ([`dialog::show_error_on`]), a failed poll is a line of the log.

pub mod marker;

use std::ffi::OsString;
use std::fmt;

use super::popup;
use super::viewlog::ViewLog;
use super::view_ports::RunFn;
use super::dialog;
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;
use cpd_core::ports::Ports;
use cpd_core::session::PROGRAM;
use cpd_core::store::Store;

/// The view the merge's log is named after (`$CPD_HOME/run/tui/merge.log`, hs-d364).
const LOG: &str = "merge";

/// `<owner>/<repo>#<n>`, checked only as far as the view needs it (a `/` in the repository, a positive number);
/// `pr merge` is the authority on the rest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pull {
    pub(crate) repo: String,
    pub(crate) number: i64,
}

impl Pull {
    /// The pull request of an Inbox item's subject, `pr:<owner>/<repo>#<n>`; none for any other subject.
    pub fn from_subject(subject: &str) -> Option<Pull> {
        Pull::parse(subject.strip_prefix("pr:")?).ok()
    }

    pub fn parse(text: &str) -> Result<Pull, String> {
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

/// The merge icon's click, the menus' Merge: `cpd tui action merge <arg>` (`<owner>/<repo>#<n>`) started at once, on the most
/// recently active client.
pub(crate) fn start(tmux: &Tmux, arg: &str) -> Result<(), CpdError> {
    let pull = Pull::parse(arg).map_err(|e| CpdError::new(ErrorCode::ArgsInvalid, e))?;
    let format = "#{client_activity} #{client_width} #{client_height} #{client_name}";
    let clients = tmux.run(&["list-clients", "-F", format].map(str::to_owned))?;
    let name = popup::newest_client(&clients).and_then(|(name, _)| name).ok_or_else(|| {
        CpdError::new(ErrorCode::TmuxFailed, "No tmux client is attached to the cockpit.").with("socket", tmux.socket())
    })?;
    tmux.run(&start_args(&pull, &name)).map(drop)
}

/// The tmux arguments the merge runs: `run-shell -b 'cpd tui action merge <ref> --client <name>'`. The command is
/// quoted for the shell, then every `#` doubled for `run-shell`'s format expansion, so it reaches `cpd` unchanged;
/// stderr is dropped, as the StatusBar's binding drops it (`run-shell -b` shows any output in view mode).
fn start_args(pull: &Pull, client: &str) -> Vec<String> {
    let command = format!("{PROGRAM} tui action merge {} --client {} 2>/dev/null", popup::sh_quote(&pull.to_string()), popup::sh_quote(client));
    ["run-shell".to_owned(), "-b".to_owned(), command.replace('#', "##")].to_vec()
}

/// The open pull requests as `<owner>/<repo>#<n>` (`SELECT repo, number FROM pull_requests WHERE state = 'open'`).
fn open_pulls(store: &mut Store) -> Result<std::collections::BTreeSet<String>, CpdError> {
    crate::read::open_pulls(store)
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
#[cfg(test)]
pub(crate) fn action(home: &CpdHome, tmux: &Tmux, pull: &Pull, client: &str, run_command: super::RunCommand) -> Result<(), CpdError> {
    action_with(&Ports::real(), home, tmux, pull, client, &run_command)
}

/// [`action`] on injected ports: the clock stamps the marker and the log, the commands are `run`'s, tmux is `tmux`'s.
pub(crate) fn action_with(ports: &Ports, home: &CpdHome, tmux: &Tmux, pull: &Pull, client: &str, run: RunFn) -> Result<(), CpdError> {
    let log = ViewLog::of(home, LOG);
    let config = Config::load(home)?;
    let mut store = Store::open(home, u64::from(config.timeouts.store_busy_ms))?;
    let open = open_pulls(&mut store)?;
    // The only clean-up of a marker left behind: a merged one whose row is no longer open.
    marker::remove_settled(home, &open);
    if marker::live_with(ports, home, &open).contains(&pull.to_string()) {
        // A merge of it runs: its marker is not this action's to touch, and the User is told nothing.
        let _ = log.append_with(ports, &format!("merge already running {pull}"));
        return Ok(());
    }
    marker::write_on(ports, home, pull)?;
    let mut marked = Marked { home, pull, keep: false };
    let cpd = |words: &[&str]| run(std::iter::once(PROGRAM).chain(words.iter().copied()).map(OsString::from).collect());
    let failed = match cpd(&["pr", "merge", &pull.to_string()]) {
        Ok(_) => {
            // The merge happened: the marker stays, marked merged, until a read stores it (hs-d550).
            marker::write_merged_on(ports, home, pull)?;
            if let Err(e) = cpd(&["pr", "poll"]) {
                // Nothing else to show it with: the next periodic poll refreshes the column.
                let _ = log.append_with(ports, &format!("pr poll after merge {pull}: {}: {}", e.code(), e.message()));
            }
            match open_pulls(&mut store) {
                Ok(open) => marked.keep = open.contains(&pull.to_string()),
                Err(e) => {
                    marked.keep = true;
                    let _ = log.append_with(ports, &format!("read after merge {pull}: {}: {}", e.code(), e.message()));
                }
            }
            return Ok(());
        }
        Err(e) => e,
    };
    // The request has ended: the spinner goes before the dialog opens.
    drop(marked);
    let mut text = format!("merge {pull}: {}: {}", failed.code(), failed.message());
    if let Err(unwritten) = log.append_with(ports, &text) {
        text = format!("{text}\n{unwritten}");
    }
    dialog::show_error_on(tmux, client, &format!("merge {pull}"), &text)
        .map_err(|unshown| CpdError::new(unshown.code(), text).with("dialog", unshown.message()))
}

/// The marker of a merge this process runs, removed when it is dropped: no exit, a panic included, leaves it behind.
struct Marked<'a> {
    home: &'a CpdHome,
    pull: &'a Pull,
    /// The action decided the marker stays (the merge is not stored yet).
    keep: bool,
}

impl Drop for Marked<'_> {
    fn drop(&mut self) {
        if !self.keep {
            marker::remove(self.home, self.pull);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pull() -> Pull {
        Pull::parse("o/r#7").unwrap()
    }

    #[test]
    fn the_argument_is_owner_repo_hash_number() {
        assert_eq!(pull().to_string(), "o/r#7");
        for bad in ["", "o/r", "o/r#", "o/r#x", "o/r#0", "o/r#-1", "r#7", "o#7"] {
            assert!(Pull::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_argument_is_quoted_for_the_popups_shell() {
        assert_eq!(popup::sh_quote("o/r#7"), "'o/r#7'");
        assert_eq!(popup::sh_quote("a'b"), r"'a'\''b'");
    }

    #[test]
    fn the_merge_starts_the_action_in_the_background_with_every_hash_doubled_for_run_shell() {
        assert_eq!(
            start_args(&pull(), "/dev/pts/9"),
            ["run-shell", "-b", "cpd tui action merge 'o/r##7' --client '/dev/pts/9' 2>/dev/null"]
        );
    }

    #[test]
    fn the_merge_runs_on_the_newest_named_client() {
        let named = |listing: &str| popup::newest_client(listing).and_then(|(name, size)| Some((name?, size)));
        let listing = "100 80 24 /dev/pts/1\n300 215 62 /dev/pts/4\n200 120 30 /dev/pts/2\n";
        assert_eq!(named(listing), Some(("/dev/pts/4".to_owned(), (215, 62))));
        for bad in ["", "junk\n", "100 80 24\n", "100 80 24 \n", "x 80 24 /dev/pts/1\n"] {
            assert_eq!(named(bad), None, "{bad:?}");
        }
    }
}

#[cfg(test)]
#[path = "action_d550.rs"]
mod d550;
