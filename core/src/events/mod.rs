//! Events (contract §5; commands.md §11): appending them ([`append`]),
//! `events list` and `events follow`.
//!
//! Delivery to a Lead's pi session (§5.3) is the pi adapter's, in TypeScript.

mod append;
#[cfg(test)]
mod bug_tests;
mod kind;
#[cfg(test)]
mod protocol_breach_tests;
mod notify;
pub mod repeat;
mod select;
mod stale;
mod typed;

use std::ffi::OsString;
use std::io::Write;
use std::time::Duration;

use serde_json::json;

use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::host::exe::Exe;
use crate::host::spawn::Launch;
use crate::ids::{ContextId, PlanId, SessionId};
use crate::output::{Output, to_line};

pub use append::{NewEvent, append};
pub use kind::{Kind, PROTOCOL, Set, Spec};
pub use notify::{escalation, job_event};
pub use select::{Event, Selection, highest_id};
pub use typed::{BasePull, Counts};

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
    pub(crate) plan: Option<PlanId>,
    /// Events of this Context
    #[arg(long, value_name = "id")]
    pub(crate) context: Option<ContextId>,
    /// Events for this Session (§5.3's rule)
    #[arg(long, value_name = "id")]
    pub(crate) session: Option<SessionId>,
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
    pub(crate) scope: Scope,
    /// Only this kind (repeatable)
    #[arg(long = "kind", value_name = "kind")]
    pub(crate) kinds: Vec<Kind>,
    /// Start after this id [default: the highest id now]; 0 replays everything
    #[arg(long, value_name = "id", value_parser = clap::value_parser!(i64).range(0..))]
    pub(crate) since: Option<i64>,
    /// Exit once everything committed at the start has been printed
    #[arg(long)]
    pub(crate) until_idle: bool,
}

pub fn run(command: Command) -> Result<Output, CpdError> {
    match command {
        Command::List(args) => list(args),
        Command::Follow(args) => follow(args),
    }
}

/// `events list` (§11.1).
fn list(args: ListArgs) -> Result<Output, CpdError> {
    let Env { mut store, .. } = Env::open()?;
    let since = args.since.unwrap_or(0);
    let Scope {
        plan,
        context,
        session,
    } = args.scope;
    let events = store.read(|conn| {
        Selection::resolve(conn, plan, context, session, args.kinds)?
            .fetch(conn, since, None, args.limit)
    })?;
    let next = events.last().map_or(since, |e| e.id);
    Ok(Output::success(json!({"events": events, "next": next})))
}

/// [`follow`] on the ports of `env` (`env.ports.clock` for the poll's nap, `env.ports.spawn` for the exe it restarts
/// from) and the writer `out` for its lines, not the process's stdout: what a test of the loop calls.
/// `out` failing with `BrokenPipe` ends it as the reader's end does. With no `self_exe` to watch there is no restart.
pub fn follow_in(env: &mut Env, args: FollowArgs, out: &mut dyn Write) -> Result<Output, CpdError> {
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    follow_loop(env, args, out, &|| false, &argv)
}

/// `events follow` (§11.2, §5.4): prints each Event as a line, in id order,
/// until killed, until stdout closes, or (`--until-idle`) until it has printed
/// what was committed when it started.
fn follow(args: FollowArgs) -> Result<Output, CpdError> {
    let mut env = Env::open()?;
    let mut stdout = std::io::stdout().lock();
    let argv: Vec<OsString> = std::env::args_os().skip(1).collect();
    follow_loop(
        &mut env,
        args,
        &mut stdout,
        &|| reader_gone(&std::io::stdout()),
        &argv,
    )
}

/// The loop of [`follow`] and [`follow_in`]: `gone` says whether the reader went away without a write (G1), and `argv`
/// is the command line (without the program name) a restart on a new install repeats with `--since` moved on. A test
/// of the end by `gone` or of the restart calls it; `follow` passes the process's own.
pub fn follow_loop(
    env: &mut Env,
    args: FollowArgs,
    out: &mut dyn Write,
    gone: &dyn Fn() -> bool,
    argv: &[OsString],
) -> Result<Output, CpdError> {
    let Env {
        store,
        config,
        ports,
        ..
    } = env;
    let exe = match ports.spawn.self_exe() {
        Ok(path) => Some(Exe::at(path)?),
        Err(_) => None,
    };
    let Scope {
        plan,
        context,
        session,
    } = args.scope;
    let (selection, mut cursor, until) = store.read(|conn| {
        let selection = Selection::resolve(conn, plan, context, session, args.kinds)?.following();
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
    // `PRAGMA data_version` when the last query ran; none before the first.
    let mut seen: Option<i64> = None;
    // `PRAGMA schema_version` when the store verdict was last computed; none before the first.
    let mut schema_seen: Option<i64> = None;
    loop {
        // A migration changes the schema: recompute the verdict (contract §4.5) when it did, and end
        // with its code when this binary can no longer open the store, so the adapter restarts the
        // follower on the installed binary.
        let (version, schema_version, refusal) = store.read(|conn| {
            let data = conn.pragma_query_value(None, "data_version", |r| r.get::<_, i64>(0))?;
            let schema = conn.pragma_query_value(None, "schema_version", |r| r.get::<_, i64>(0))?;
            let refusal = if schema_seen == Some(schema) {
                None
            } else {
                crate::store::verdict(conn)?.refusal().cloned()
            };
            Ok((data, schema, refusal))
        })?;
        if let Some(refusal) = refusal {
            return Err(refusal);
        }
        schema_seen = Some(schema_version);
        if seen != Some(version) {
            seen = Some(version);
            loop {
                // The fetch and the stale checks share one read transaction: one consistent view (§5.4).
                let (fetched, last, kept) = store.read(|conn| {
                    let batch = selection.fetch(conn, cursor, until, BATCH)?;
                    let fetched = batch.len() as i64;
                    let last = batch.last().map(|e| e.id);
                    let mut kept = Vec::with_capacity(batch.len());
                    for event in batch {
                        if !(selection.drops_stale() && stale::is_stale(conn, &event)?) {
                            kept.push(event);
                        }
                    }
                    Ok((fetched, last, kept))
                })?;
                let Some(last) = last else { break };
                if !kept.is_empty() {
                    let lines: String = kept.iter().map(to_line).collect();
                    if let Err(e) = print(&mut *out, &lines) {
                        return e.map(|()| Output::Owned);
                    }
                }
                // The cursor passes every Event read, printed or dropped; a full batch is read again at once.
                cursor = last;
                if fetched < BATCH {
                    break;
                }
            }
        }
        if until.is_some() {
            return Ok(Output::Owned);
        }
        if gone() {
            return Ok(Output::Owned);
        }
        // A new install: the same follower again, after the last Event printed (hs-d140). Returns only on failure.
        if let Some(exe) = exe.as_ref().filter(|exe| exe.replaced()) {
            let args = restart_args(argv.iter().cloned(), cursor)
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            return Err(exec_again(ports, exe, args));
        }
        ports.clock.sleep(poll);
    }
}

/// Execs `exe` with `args`; returns only on failure, as the error to report.
pub(crate) fn exec_again(ports: &crate::ports::Ports, exe: &Exe, args: Vec<String>) -> CpdError {
    let launch = Launch {
        program: exe.path().display().to_string(),
        args,
        ..Launch::default()
    };
    let e = ports.spawn.exec(&launch);
    CpdError::new(
        ErrorCode::SpawnFailed,
        format!("Cannot restart cpd from {}: {e}.", exe.path().display()),
    )
    .with("path", exe.path().display().to_string())
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

/// Writes `lines` and flushes. `Err(Ok(()))` when the reader is gone (EPIPE: the normal end of a follower);
/// `Err(Err(_))` for any other write error: `internal`, as no I/O error code exists and the database is not at fault.
fn print(out: &mut (impl Write + ?Sized), lines: &str) -> Result<(), Result<(), CpdError>> {
    out.write_all(lines.as_bytes())
        .and_then(|()| out.flush())
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::BrokenPipe => Ok(()),
            _ => Err(CpdError::new(
                ErrorCode::Internal,
                format!("Cannot write to stdout: {e}."),
            )),
        })
}

/// Whether the reader of `stdout` is gone, checked without writing (G1): a follower with no Event to
/// write would otherwise never meet EPIPE. `POLLOUT` is requested because macOS reports
/// `POLLHUP` for a pipe whose reader closed only then (verified on macOS with `select.poll`: no events requested gives
/// nothing). On Linux such a pipe reports `POLLERR` (unverified on Linux). A file or tty reports `POLLOUT` only: not gone.
fn reader_gone(stdout: &impl std::os::fd::AsFd) -> bool {
    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
    let mut fds = [PollFd::new(stdout.as_fd(), PollFlags::POLLOUT)];
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

    /// A writer whose every write fails with `kind`.
    struct Failing(std::io::ErrorKind);

    impl Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(self.0.into())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Err(self.0.into())
        }
    }

    #[test]
    fn a_stdout_write_error_is_internal_not_store_failed() {
        let err = print(&mut Failing(std::io::ErrorKind::StorageFull), "x\n")
            .unwrap_err()
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::Internal, "{err:?}");
    }

    #[test]
    fn a_closed_reader_ends_the_follower_without_an_error() {
        assert!(matches!(
            print(&mut Failing(std::io::ErrorKind::BrokenPipe), "x\n"),
            Err(Ok(()))
        ));
    }

    /// What the follower's `stdout` being a pipe whose reader went away comes to, with no process: a pipe of this
    /// process (nothing is written to it, so no `SIGPIPE`), and a file, which has no reader to lose.
    #[test]
    fn a_pipe_whose_reader_is_gone_is_gone_and_a_file_is_not() {
        let (reader, writer) = nix::unistd::pipe().unwrap();
        assert!(!reader_gone(&writer), "the reader is still there");
        drop(reader);
        assert!(reader_gone(&writer));
        assert!(!reader_gone(&tempfile::tempfile().unwrap()));
    }

    fn restarted(args: &[&str], cursor: i64) -> Vec<String> {
        restart_args(args.iter().map(OsString::from), cursor)
            .into_iter()
            .map(|a| a.into_string().unwrap())
            .collect()
    }

    #[test]
    fn restart_sets_since_and_keeps_everything_else() {
        assert_eq!(
            restarted(&["events", "follow", "--plan", "pay"], 7),
            ["events", "follow", "--plan", "pay", "--since", "7"]
        );
        assert_eq!(
            restarted(
                &[
                    "events",
                    "follow",
                    "--since",
                    "0",
                    "--kind",
                    "error",
                    "--until-idle"
                ],
                9
            ),
            [
                "events",
                "follow",
                "--kind",
                "error",
                "--until-idle",
                "--since",
                "9"
            ]
        );
        assert_eq!(
            restarted(&["events", "follow", "--since=3"], 4),
            ["events", "follow", "--since", "4"]
        );
    }
}
