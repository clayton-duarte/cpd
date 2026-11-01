//! The `@cpd_workers` step (contract §6.1; commands.md §12; User, hs-d149,
//! hs-d198): the right side of the StatusBar, which tmux's status line cannot
//! compute: the plain text `workers Q󰞘W󰞘N/M` and its short form `Q󰞘W󰞘N/M`
//! (hs-d749, hs-d750): no style, drawn in the pill's one reversed text colour on its colour-8 block.
//! Every tick, `N` is the tenant Jobs of `workers` (the starter's count,
//! contract §3.4 "Tenancy"), `M` is `concurrency`, `W` the Ready Jobs (E3's rule) and
//! `Q` the `queued` Jobs; the options are set in one `tmux set` call only
//! when one of them differs from what was last set (the first tick always
//! sets them, and so does the first tick after a tmux failure or a missing
//! server: a new server starts without them). No row, no Event, except an `error` when tmux
//! fails while a server runs and no tmux restart is pending (a pending restart
//! takes the server away under the step: that failure is expected and not
//! reported): with no server there is no status line to feed, and the step
//! waits.

use super::engine::Engine;
use super::report::{About, Step};
use super::restart;
use super::schedule::READY;
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::host::tmux::RUNNER_OPTION;
use cpd_core::incident::is_on;
use rusqlite::OptionalExtension;

/// The options the status line draws: the long form and the short form.
const LONG: &str = "@cpd_workers";
const SHORT: &str = "@cpd_workers_short";

/// The global option carrying the incident pill's text while incident mode is on (contract §6.1).
const INCIDENT: &str = "@cpd_incident";

/// The tenant Jobs of `workers` (§3.4 "Tenancy"): the occupancy the starter counts, a `failed` Job included.
const HELD: &str = "SELECT count(*) FROM job_tenants WHERE pool = 'workers'";

/// Every `queued` Job, the waiting ones included.
const QUEUED: &str = "SELECT count(*) FROM jobs WHERE status = 'queued'";

/// The glyph between the three numbers (User, hs-d749): a private-use Nerd Font glyph, one cell wide like the buttons' glyphs.
const SEPARATOR: char = '\u{f0798}';

/// The three numbers of the StatusBar.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(super) struct Counts {
    /// Tenant Jobs of `workers`.
    pub(super) running: u32,
    /// Config `concurrency`.
    pub(super) limit: u32,
    /// Ready Jobs.
    pub(super) waiting: u32,
    /// `queued` Jobs.
    pub(super) queued: u32,
}

impl Counts {
    /// All four in one read.
    fn read(conn: &rusqlite::Connection, limit: u32) -> Result<Counts, CpdError> {
        let count = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, u32>(0));
        Ok(Counts {
            running: count(HELD)?,
            limit,
            waiting: count(&format!("SELECT count(*) FROM ({READY})"))?,
            queued: count(QUEUED)?,
        })
    }

    /// `workers Q󰞘W󰞘N/M`: the label and the short form, plain.
    pub(super) fn long(self) -> String {
        format!("workers {}", self.short())
    }

    /// `Q󰞘W󰞘N/M`: the three numbers and `/M`, plain, no label.
    pub(super) fn short(self) -> String {
        format!(
            "{}{SEPARATOR}{}{SEPARATOR}{}/{}",
            self.queued, self.waiting, self.running, self.limit
        )
    }
}

pub(super) fn run(engine: &mut Engine) {
    let limit = engine.config.concurrency;
    let now = engine.ports.clock.utc();
    let read = engine
        .store
        .read(|conn| Ok((Counts::read(conn, limit)?, incident(conn, now)?)));
    let (counts, incident) = match read {
        Ok(read) => read,
        Err(e) => return engine.fail(Step::Workers, About::Machine, &e),
    };
    let (long, short) = (counts.long(), counts.short());
    // pro-d199: read afresh every tick; a config that does not parse sets nothing (no stale fallback).
    let runner = match Config::load(&engine.home) {
        Ok(fresh) => fresh.worker_runner,
        Err(e) => return engine.fail(Step::Workers, About::Machine, &e),
    };
    // What was last set: the two workers texts and the incident value, one string (no value holds a newline).
    let incident_state = match &incident {
        Some(elapsed) => format!("on:{elapsed}"),
        None => "off".to_string(),
    };
    let state = format!("{long}\n{short}\n{incident_state}\n{}", runner.as_str());
    if engine.workers.as_deref() == Some(state.as_str()) {
        return;
    }
    // No server, no status line: nothing is set or remembered, and it is set once a server appears.
    match engine.tmux.server_running() {
        Ok(true) => {}
        Ok(false) => {
            engine.workers = None;
            return engine.clear(Step::Workers, &About::Machine);
        }
        Err(e) => return failed(engine, &e),
    }
    // One call, so a status line never draws a text that belongs to another.
    let mut args: Vec<String> = ["set", "-g", LONG, &long, ";", "set", "-g", SHORT, &short]
        .map(str::to_owned)
        .to_vec();
    args.extend([";", "set", "-g", RUNNER_OPTION, runner.as_str()].map(str::to_owned));
    args.push(";".to_string());
    match &incident {
        Some(elapsed) => args.extend(["set", "-g", INCIDENT, elapsed].map(str::to_owned)),
        None => args.extend(["set", "-gu", INCIDENT].map(str::to_owned)),
    }
    let set: Result<String, CpdError> = engine.tmux.run(&args);
    match set {
        Ok(_) => {
            engine.workers = Some(state);
            engine.clear(Step::Workers, &About::Machine);
        }
        Err(e) => failed(engine, &e),
    }
}

/// The `@cpd_incident` text while incident mode is on (contract §6.1, hs-d640): the whole-minute
/// duration from the one `incident` row's `started_at` to now. `None` while off.
fn incident(
    conn: &rusqlite::Connection,
    now: time::OffsetDateTime,
) -> Result<Option<String>, CpdError> {
    if !is_on(conn) {
        return Ok(None);
    }
    let started_at = conn
        .query_row("SELECT started_at FROM incident WHERE id = 1", [], |r| {
            r.get::<_, String>(0)
        })
        .optional()?;
    started_at
        .map(|started_at| elapsed(&started_at, now))
        .transpose()
}

/// The duration from the incident row's `started_at` to `now` (hs-d640, contract §6.1 and 6.1.1),
/// in whole minutes: under 60 minutes `<m>m`, under 24 hours `<h>h` plus ` <m>m` only when the
/// minutes part is non-zero, else `<d>d` plus ` <h>h` only when the hours part is non-zero; a
/// `started_at` in the future reads `0m`. `started_at` is RFC 3339, the format `clock::timestamp`
/// writes; anything else is a loud `StoreFailed`.
fn elapsed(started_at: &str, now: time::OffsetDateTime) -> Result<String, CpdError> {
    let started =
        time::OffsetDateTime::parse(started_at, &time::format_description::well_known::Rfc3339)
            .map_err(|e| {
                CpdError::new(
            ErrorCode::StoreFailed,
            format!("The incident holds a started_at that is not RFC 3339: {started_at:?} ({e})."),
        )
        .with("started_at", started_at)
            })?;
    let minutes = (now - started).whole_minutes().max(0);
    Ok(if minutes < 60 {
        format!("{minutes}m")
    } else if minutes < 1_440 {
        let (hours, rest) = (minutes / 60, minutes % 60);
        if rest == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h {rest}m")
        }
    } else {
        let (days, rest) = (minutes / 1_440, (minutes % 1_440) / 60);
        if rest == 0 {
            format!("{days}d")
        } else {
            format!("{days}d {rest}h")
        }
    })
}

/// A tmux call of the step failed: nothing is remembered (a restarted server starts without the
/// options), and the next tick tries again. While a tmux restart is pending and has not failed, the
/// failure is expected, the restart takes the server away under the step, and is not reported.
fn failed(engine: &mut Engine, error: &CpdError) {
    engine.workers = None;
    if restart::holding(&engine.home).unwrap_or(false) {
        return;
    }
    engine.fail(Step::Workers, About::Machine, error);
}

#[cfg(test)]
mod no_inbox;

#[cfg(test)]
mod runner_tests;

#[cfg(test)]
mod tests {
    use super::Counts;
    use rusqlite::Connection;

    fn counts(running: u32, limit: u32, waiting: u32, queued: u32) -> Counts {
        Counts {
            running,
            limit,
            waiting,
            queued,
        }
    }

    /// The tables the Ready rule reads, with three Plans' worth of rows.
    fn store() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE job_tenants (plan TEXT, job TEXT, pool TEXT);
             CREATE TABLE plans (id TEXT, phase TEXT, archived_at TEXT);
             CREATE TABLE prs (plan TEXT, key TEXT, status TEXT);
             CREATE TABLE jobs (plan TEXT, key TEXT, pr TEXT, status TEXT, seq INTEGER, skipped_n INTEGER, urgent INTEGER NOT NULL DEFAULT 0, kind TEXT NOT NULL DEFAULT 'work');
             CREATE TABLE dependencies (plan TEXT, job TEXT, needs TEXT);
             INSERT INTO plans (id, phase) VALUES ('pay', 'dispatch');
             INSERT INTO prs (plan, key, status) VALUES ('pay', 'a', 'dispatch'), ('pay', 'w', 'waiting');
             INSERT INTO job_tenants (plan, job, pool) VALUES ('pay', 'busy', 'workers'), ('pay', 'busy', 'repo:acme/pay:db');
             -- ready: no Dependency
             INSERT INTO jobs (plan, key, pr, status, seq) VALUES ('pay', 'free', 'a', 'queued', 1);
             -- ready: its only Dependency is merged
             INSERT INTO jobs (plan, key, pr, status, seq) VALUES ('pay', 'done', 'a', 'merged', 2), ('pay', 'after', 'a', 'queued', 3);
             INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'after', 'done');
             -- queued, not ready: a Dependency that is not merged
             INSERT INTO jobs (plan, key, pr, status, seq) VALUES ('pay', 'busy', 'a', 'running', 4), ('pay', 'blocked', 'a', 'queued', 5);
             INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'blocked', 'busy');
             -- queued, not ready: its PR has not started
             INSERT INTO jobs (plan, key, pr, status, seq) VALUES ('pay', 'later', 'w', 'queued', 6);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn waiting_counts_only_ready_jobs_and_queued_counts_every_queued_job() {
        assert_eq!(Counts::read(&store(), 3).unwrap(), counts(1, 3, 2, 4));
    }
}
