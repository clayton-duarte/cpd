//! The `@cpd_workers` step (contract §6.1; commands.md §12; User, hs-d149,
//! hs-d198): the right side of the StatusBar, which tmux's status line cannot
//! compute: the `workers N/M · W waiting · Q queued` text and its short form `N/M·W·Q`.
//! Every tick, `N` is the Attempts not `ended` (the slots of the
//! `widgets` view), `M` is `concurrency`, `W` the Ready Jobs (E3's rule) and
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
use cpd_core::error::CpdError;

/// The options the status line draws: the long form and the short form.
const LONG: &str = "@cpd_workers";
const SHORT: &str = "@cpd_workers_short";

/// The Attempts holding a slot (§3.4): the same query as the `widgets` view's slots.
const HELD: &str = "SELECT count(*) FROM attempts WHERE status <> 'ended'";

/// Every `queued` Job, the waiting ones included.
const QUEUED: &str = "SELECT count(*) FROM jobs WHERE status = 'queued'";

/// The three numbers of the StatusBar.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(super) struct Counts {
    /// Attempts not `ended`.
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
        Ok(Counts { running: count(HELD)?, limit, waiting: count(&format!("SELECT count(*) FROM ({READY})"))?, queued: count(QUEUED)? })
    }

    /// `workers N/M · W waiting · Q queued`, with no style of its own.
    pub(super) fn long(self) -> String {
        format!("workers {}/{} · {} waiting · {} queued", self.running, self.limit, self.waiting, self.queued)
    }

    /// `N/M·W·Q`, with no style of its own.
    pub(super) fn short(self) -> String {
        format!("{}/{}·{}·{}", self.running, self.limit, self.waiting, self.queued)
    }
}

pub(super) fn run(engine: &mut Engine) {
    let limit = engine.config.concurrency;
    let read = engine.store.read(|conn| Counts::read(conn, limit));
    let counts = match read {
        Ok(read) => read,
        Err(e) => return engine.fail(Step::Workers, About::Machine, &e),
    };
    let (long, short) = (counts.long(), counts.short());
    // What was last set: the two values, one string (no value holds a newline).
    let state = format!("{long}\n{short}");
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
    let args = ["set", "-g", LONG, &long, ";", "set", "-g", SHORT, &short].map(str::to_owned);
    let set: Result<String, CpdError> = engine.tmux.run(&args);
    match set {
        Ok(_) => {
            engine.workers = Some(state);
            engine.clear(Step::Workers, &About::Machine);
        }
        Err(e) => failed(engine, &e),
    }
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
mod tests {
    use super::Counts;
    use rusqlite::Connection;

    fn counts(running: u32, limit: u32, waiting: u32, queued: u32) -> Counts {
        Counts { running, limit, waiting, queued }
    }

    #[test]
    fn the_workers_texts_have_no_style() {
        let c = counts(6, 6, 4, 9);
        assert_eq!(c.long(), "workers 6/6 · 4 waiting · 9 queued");
        assert_eq!(c.short(), "6/6·4·9");
    }

    /// The tables the Ready rule reads, with three Plans' worth of rows.
    fn store() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE attempts (status TEXT);
             CREATE TABLE plans (id TEXT, phase TEXT);
             CREATE TABLE prs (plan TEXT, key TEXT, status TEXT);
             CREATE TABLE jobs (plan TEXT, key TEXT, pr TEXT, status TEXT, seq INTEGER, urgent INTEGER NOT NULL DEFAULT 0, kind TEXT NOT NULL DEFAULT 'work');
             CREATE TABLE dependencies (plan TEXT, job TEXT, needs TEXT);
             INSERT INTO plans (id, phase) VALUES ('pay', 'dispatch');
             INSERT INTO prs (plan, key, status) VALUES ('pay', 'a', 'dispatch'), ('pay', 'w', 'waiting');
             INSERT INTO attempts (status) VALUES ('starting'), ('ended');
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
