//! Events (contract §5; commands.md §11): appending them ([`append`]),
//! `events list` and `events follow`.
//!
//! Delivery to a Lead's pi session (§5.3) is the pi adapter's, in TypeScript.

mod append;
mod kind;
mod select;
mod typed;

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use serde_json::json;

use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::exe::Exe;
use crate::ids::{ContextId, PlanId, SessionId};
use crate::output::{Output, to_line};
use crate::store::{Store, VERSION};

pub use append::{NewEvent, append};
pub use kind::{Kind, PROTOCOL, Set, Spec};
pub use select::{Event, Selection, highest_id};

/// How many Events one query reads, and `follow`'s batch (§5.4).
const BATCH: i64 = 500;

#[derive(clap::Subcommand, Debug)]
pub enum Command {
    /// List Events (§11.1)
    List(ListArgs),
    /// Follow Events, one per line (the one streaming command) (§11.2)
    Follow(FollowArgs),
}

/// At most one of the three (`usage`); they are filters, so none falls back to
/// `CPD_PLAN` or `CPD_SESSION` (commands.md §11).
#[derive(clap::Args, Debug)]
#[group(multiple = false)]
pub struct Scope {
    /// Events of this Plan
    #[arg(long, value_name = "id")]
    plan: Option<PlanId>,
    /// Events of this Context
    #[arg(long, value_name = "id")]
    context: Option<ContextId>,
    /// Events for this Session (§5.3's rule)
    #[arg(long, value_name = "id")]
    session: Option<SessionId>,
}

#[derive(clap::Args, Debug)]
pub struct ListArgs {
    #[command(flatten)]
    scope: Scope,
    /// Only this kind (repeatable)
    #[arg(long = "kind", value_name = "kind")]
    kinds: Vec<Kind>,
    /// Only Events with a greater id [default: 0]
    #[arg(long, value_name = "id", value_parser = clap::value_parser!(i64).range(0..))]
    since: Option<i64>,
    /// At most this many Events
    #[arg(long, value_name = "n", default_value_t = BATCH, value_parser = clap::value_parser!(i64).range(1..=5000))]
    limit: i64,
}

#[derive(clap::Args, Debug)]
pub struct FollowArgs {
    #[command(flatten)]
    scope: Scope,
    /// Only this kind (repeatable)
    #[arg(long = "kind", value_name = "kind")]
    kinds: Vec<Kind>,
    /// Start after this id [default: the highest id now]; 0 replays everything
    #[arg(long, value_name = "id", value_parser = clap::value_parser!(i64).range(0..))]
    since: Option<i64>,
    /// Exit once everything committed at the start has been printed
    #[arg(long)]
    until_idle: bool,
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::List(args) => list(args),
        Command::Follow(args) => follow(args),
    }
}

fn open() -> Result<(Store, Config), CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    Ok((store, config))
}

/// `events list` (§11.1).
fn list(args: ListArgs) -> Result<Output, CpdError> {
    let (mut store, _) = open()?;
    let since = args.since.unwrap_or(0);
    let Scope { plan, context, session } = args.scope;
    let events = store.read(|conn| {
        Selection::resolve(conn, plan, context, session, args.kinds)?.fetch(conn, since, None, args.limit)
    })?;
    let next = events.last().map_or(since, |e| e.id);
    Ok(Output::success(json!({"events": events, "next": next})))
}

/// `events follow` (§11.2, §5.4): prints each Event as a line, in id order,
/// until killed, until stdout closes, or (`--until-idle`) until it has printed
/// what was committed when it started.
fn follow(args: FollowArgs) -> Result<Output, CpdError> {
    let (mut store, config) = open()?;
    let exe = Exe::current()?;
    let Scope { plan, context, session } = args.scope;
    let (selection, mut cursor, until) = store.read(|conn| {
        let selection = Selection::resolve(conn, plan, context, session, args.kinds)?;
        let highest = highest_id(conn)?;
        let cursor = match args.since {
            Some(since) if since > highest => {
                return Err(CpdError::new(
                    ErrorCode::CursorAhead,
                    format!("--since {since} is above the highest Event id, {highest}: the cursor is from another database."),
                )
                .with("since", since)
                .with("highest", highest));
            }
            Some(since) => since,
            None => highest,
        };
        Ok((selection, cursor, highest))
    })?;
    let until = args.until_idle.then_some(until);

    let poll = Duration::from_millis(u64::from(config.events.poll_ms));
    let mut stdout = std::io::stdout().lock();
    // `PRAGMA data_version` when the last query ran; none before the first.
    let mut seen: Option<i64> = None;
    loop {
        let (version, user_version) = store.read(|conn| {
            Ok((
                conn.pragma_query_value(None, "data_version", |r| r.get::<_, i64>(0))?,
                conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))?,
            ))
        })?;
        if user_version > VERSION {
            return Err(CpdError::new(
                ErrorCode::StoreTooNew,
                format!("The database has schema version {user_version}, newer than this cpd's {VERSION}: upgrade cpd."),
            )
            .with("version", user_version)
            .with("supported", VERSION));
        }
        if seen != Some(version) {
            seen = Some(version);
            loop {
                let batch = store.read(|conn| selection.fetch(conn, cursor, until, BATCH))?;
                let Some(last) = batch.last() else { break };
                let next = last.id;
                let lines: String = batch.iter().map(to_line).collect();
                if let Err(e) = stdout.write_all(lines.as_bytes()).and_then(|()| stdout.flush()) {
                    return match e.kind() {
                        std::io::ErrorKind::BrokenPipe => Ok(Output::Owned),
                        _ => Err(CpdError::new(ErrorCode::StoreFailed, format!("Cannot write to stdout: {e}."))),
                    };
                }
                cursor = next;
                if (batch.len() as i64) < BATCH {
                    break;
                }
            }
        }
        if until.is_some() {
            return Ok(Output::Owned);
        }
        if reader_gone(&stdout) {
            return Ok(Output::Owned);
        }
        // A new install: the same follower again, after the last Event printed (hs-d140). Returns only on failure.
        if exe.replaced() {
            return Err(exe.exec(restart_args(std::env::args_os().skip(1), cursor)));
        }
        std::thread::sleep(poll);
    }
}

/// `args` (without the program name) with `--since` set to `cursor`: any `--since <id>` or `--since=<id>`
/// is dropped and `--since <cursor>` put last.
fn restart_args(args: impl Iterator<Item = OsString>, cursor: i64) -> Vec<OsString> {
    let mut out = Vec::new();
    let mut args = args;
    while let Some(arg) = args.next() {
        if arg == "--since" {
            args.next();
        } else if !arg.to_string_lossy().starts_with("--since=") {
            out.push(arg);
        }
    }
    out.push("--since".into());
    out.push(cursor.to_string().into());
    out
}

/// Whether the reader of `stdout` is gone, checked without writing (G1): a follower with no Event to
/// write would otherwise never meet EPIPE. A pipe whose read end closed reports `POLLERR` on Linux; the
/// hangup flag `POLLHUP` is handled too, as macOS reports it (unverified: not run on macOS).
fn reader_gone(stdout: &impl std::os::fd::AsFd) -> bool {
    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
    let mut fds = [PollFd::new(stdout.as_fd(), PollFlags::empty())];
    match poll(&mut fds, PollTimeout::ZERO) {
        Ok(n) if n > 0 => fds[0]
            .revents()
            .is_some_and(|r| r.intersects(PollFlags::POLLHUP | PollFlags::POLLERR)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn restarted(args: &[&str], cursor: i64) -> Vec<String> {
        restart_args(args.iter().map(OsString::from), cursor).into_iter().map(|a| a.into_string().unwrap()).collect()
    }

    #[test]
    fn restart_sets_since_and_keeps_everything_else() {
        assert_eq!(restarted(&["events", "follow", "--plan", "pay"], 7), ["events", "follow", "--plan", "pay", "--since", "7"]);
        assert_eq!(
            restarted(&["events", "follow", "--since", "0", "--kind", "error", "--until-idle"], 9),
            ["events", "follow", "--kind", "error", "--until-idle", "--since", "9"]
        );
        assert_eq!(restarted(&["events", "follow", "--since=3"], 4), ["events", "follow", "--since", "4"]);
    }
}
