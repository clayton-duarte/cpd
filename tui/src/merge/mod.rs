//! The merge (commands.md §13.2; hs-d273, hs-d473; pro-d111): no confirmation. The Dispatch column's merge icon, the Workflow menu's
//! Merge and the Inbox `pr-ready` item's Merge call [`start`], which runs `cpd tui action merge <owner>/<repo>#<n>
//! --client <name>` with tmux `run-shell -b` at once.
//!
//! - [`start`]: the merge, started on the newest client;
//! - [`action`]: the merge it starts, in the background: `pr merge`, which queues the merge on the outbox (swr-github.md §9); a
//!   refused one opens the cockpit's one failure dialog ([`dialog::show_error_on`]). The spinner is the queued mutation
//!   ([`in_flight`]), not a file.

use std::ffi::OsString;
use std::fmt;

use super::dialog;
use super::popup;
use super::view_ports::RunFn;
use super::viewlog::ViewLog;
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
        let number = number
            .parse::<i64>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(bad)?;
        if !repo.contains('/') {
            return Err(bad());
        }
        Ok(Pull {
            repo: repo.to_owned(),
            number,
        })
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
    let name = popup::newest_client(&clients)
        .and_then(|(name, _)| name)
        .ok_or_else(|| {
            CpdError::new(
                ErrorCode::TmuxFailed,
                "No tmux client is attached to the cockpit.",
            )
            .with("socket", tmux.socket())
        })?;
    tmux.run(&start_args(&pull, &name)).map(drop)
}

/// The tmux arguments the merge runs: `run-shell -b 'cpd tui action merge <ref> --client <name>'`. The command is
/// quoted for the shell, then every `#` doubled for `run-shell`'s format expansion, so it reaches `cpd` unchanged;
/// stderr is dropped, as the StatusBar's binding drops it (`run-shell -b` shows any output in view mode).
fn start_args(pull: &Pull, client: &str) -> Vec<String> {
    let command = format!(
        "{PROGRAM} tui action merge {} --client {} 2>/dev/null",
        popup::sh_quote(&pull.to_string()),
        popup::sh_quote(client)
    );
    [
        "run-shell".to_owned(),
        "-b".to_owned(),
        command.replace('#', "##"),
    ]
    .to_vec()
}

/// The pull requests whose merge is queued or not yet proven: an active `merge` mutation of the outbox (`pending`, `in_flight`,
/// `revalidating`), as `<owner>/<repo>#<n>`.
pub fn in_flight(store: &mut Store) -> Result<std::collections::BTreeSet<String>, CpdError> {
    store.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT target_key FROM swr_mutations
              WHERE system = 'github' AND op_kind = 'merge' AND status IN ('pending', 'in_flight', 'revalidating')",
        )?;
        let keys = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(keys
            .filter_map(|k| k.ok()?.strip_prefix("github:pr:").map(str::to_owned))
            .collect())
    })
}

/// `cpd tui action merge <owner>/<repo>#<n> --client <name>` (commands.md §13.2, the `merge` action; hs-d473,
/// hs-d312): runs `cpd pr merge` in process, which queues the merge; a success says nothing (the overlay shows the pull request
/// merging at once). A failed merge is a line of the log, written before any tmux call so that it is always there, then the
/// cockpit's one failure dialog on `client`. The `Err` is what the dialog could not show: the caller shows it with
/// `display-message -c <client>`.
/// The merge action on injected ports: the clock stamps the log, the commands are `run`'s, tmux is `tmux`'s.
pub(crate) fn action_with(
    ports: &Ports,
    home: &CpdHome,
    tmux: &Tmux,
    pull: &Pull,
    client: &str,
    run: RunFn,
) -> Result<(), CpdError> {
    let log = ViewLog::of(home, LOG);
    let failed = match run([PROGRAM, "pr", "merge", &pull.to_string()]
        .map(OsString::from)
        .to_vec())
    {
        Ok(_) => return Ok(()),
        Err(e) => e,
    };
    let mut text = format!("merge {pull}: {}: {}", failed.code(), failed.message());
    if let Err(unwritten) = log.append_with(ports, &text) {
        text = format!("{text}\n{unwritten}");
    }
    dialog::show_error_on(tmux, client, &format!("merge {pull}"), &text)
        .map_err(|unshown| CpdError::new(unshown.code(), text).with("dialog", unshown.message()))
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

    /// The pull requests with a queued, running or not-yet-proven `merge` mutation are the ones in flight; a failed or superseded one is not.
    #[test]
    fn the_merge_in_flight_is_the_active_merge_mutation_of_the_outbox() {
        use cpd_core::swr::outbox::{self, Coalesce, Op, Retry};
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        let now = cpd_core::clock::Clock::utc(&cpd_core::clock::ManualClock::new());
        let op = |kind: &str, target: &str| Op {
            system: "github".into(),
            target_key: target.into(),
            op_kind: kind.into(),
            vars: serde_json::json!({"n": target}),
            patch: None,
            context: None,
        };
        outbox::enqueue(
            &mut store,
            now,
            op("merge", "github:pr:o/r#1"),
            Coalesce::LatestWins,
        )
        .unwrap();
        outbox::enqueue(
            &mut store,
            now,
            op("rerun", "github:pr:o/r#2"),
            Coalesce::LatestWins,
        )
        .unwrap();
        let failed = outbox::enqueue(
            &mut store,
            now,
            op("merge", "github:pr:o/r#3"),
            Coalesce::LatestWins,
        )
        .unwrap();
        outbox::fail(&mut store, failed, now, "refused", Retry::Never).unwrap();
        assert_eq!(
            in_flight(&mut store).unwrap(),
            std::collections::BTreeSet::from(["o/r#1".to_owned()])
        );
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
            [
                "run-shell",
                "-b",
                "cpd tui action merge 'o/r##7' --client '/dev/pts/9' 2>/dev/null"
            ]
        );
    }

    #[test]
    fn the_merge_runs_on_the_newest_named_client() {
        let named = |listing: &str| {
            popup::newest_client(listing).and_then(|(name, size)| Some((name?, size)))
        };
        let listing = "100 80 24 /dev/pts/1\n300 215 62 /dev/pts/4\n200 120 30 /dev/pts/2\n";
        assert_eq!(named(listing), Some(("/dev/pts/4".to_owned(), (215, 62))));
        for bad in [
            "",
            "junk\n",
            "100 80 24\n",
            "100 80 24 \n",
            "x 80 24 /dev/pts/1\n",
        ] {
            assert_eq!(named(bad), None, "{bad:?}");
        }
    }
}
