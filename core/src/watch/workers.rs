//! The `@cpd_workers` step (contract §6.1; commands.md §12; User, hs-d149,
//! hs-d198): the styled `workers N/M · W waiting · Q queued` text of the
//! StatusBar, which tmux's status line cannot compute, its short form
//! `N/M·W·Q` and the long form's width. Every tick, `N` is the Attempts not
//! `ended` (the slots of the `widgets` view), `M` is `concurrency`, `W` the
//! Ready Jobs (E3's rule) and `Q` the `queued` Jobs; the three options are set
//! in one `tmux set` call only when one of them differs from what was last set
//! (the first tick always sets them). No row, no Event, except an `error` when
//! tmux fails while a server runs: with no server there is no status line to
//! feed, and the step waits.

use super::engine::Engine;
use super::report::{About, Step};
use super::schedule::READY;
use crate::error::CpdError;

/// The options the status line draws: the long form, the short form, the long form's width in cells.
const LONG: &str = "@cpd_workers";
const SHORT: &str = "@cpd_workers_short";
const WIDTH: &str = "@cpd_workers_width";

/// The Attempts holding a slot (§3.4): the same query as the `widgets` view's slots.
const HELD: &str = "SELECT count(*) FROM attempts WHERE status <> 'ended'";

/// Every `queued` Job, the waiting ones included.
const QUEUED: &str = "SELECT count(*) FROM jobs WHERE status = 'queued'";

/// The three numbers of the StatusBar.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct Counts {
    /// Attempts not `ended`.
    running: u32,
    /// Config `concurrency`.
    limit: u32,
    /// Ready Jobs.
    waiting: u32,
    /// `queued` Jobs.
    queued: u32,
}

impl Counts {
    /// All four in one read.
    fn read(conn: &rusqlite::Connection, limit: u32) -> Result<Counts, CpdError> {
        let count = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, u32>(0));
        Ok(Counts { running: count(HELD)?, limit, waiting: count(&format!("SELECT count(*) FROM ({READY})"))?, queued: count(QUEUED)? })
    }

    /// `N` green above 0, else colour 8.
    fn running_colour(self) -> &'static str {
        if self.running > 0 { "green" } else { "colour8" }
    }

    /// `W` dim at 0, else the palette's Review colour (`Meaning::Review`, yellow); the style that follows it is dim again.
    fn waiting_styled(self) -> String {
        if self.waiting > 0 { format!("#[nodim,fg=yellow]{}#[default,dim]", self.waiting) } else { self.waiting.to_string() }
    }

    /// `workers N/M · W waiting · Q queued`, the words, `/M`, `·` and `Q` dim.
    fn long(self) -> String {
        format!(
            "#[dim]workers #[nodim,fg={}]{}#[default,dim]/{} · {} waiting · {} queued#[default]",
            self.running_colour(),
            self.running,
            self.limit,
            self.waiting_styled(),
            self.queued
        )
    }

    /// `N/M·W·Q`, coloured as the long form.
    fn short(self) -> String {
        format!("#[nodim,fg={}]{}#[default,dim]/{}·{}·{}#[default]", self.running_colour(), self.running, self.limit, self.waiting_styled(), self.queued)
    }

    /// The long form's visible width in cells (every character is one cell).
    fn width(self) -> usize {
        format!("workers {}/{} · {} waiting · {} queued", self.running, self.limit, self.waiting, self.queued).chars().count()
    }
}

pub(super) fn run(engine: &mut Engine) {
    let limit = engine.config.concurrency;
    let counts = engine.store.read(|conn| Counts::read(conn, limit));
    let counts = match counts {
        Ok(counts) => counts,
        Err(e) => return engine.fail(Step::Workers, About::Machine, &e),
    };
    let (long, short, width) = (counts.long(), counts.short(), counts.width().to_string());
    // What was last set: the three values, one string (no value holds a newline).
    let state = format!("{long}\n{short}\n{width}");
    if engine.workers.as_deref() == Some(state.as_str()) {
        return;
    }
    // No server, no status line: nothing is set or remembered, and it is set once a server appears.
    match engine.tmux.server_running() {
        Ok(true) => {}
        Ok(false) => return engine.clear(Step::Workers, &About::Machine),
        Err(e) => return engine.fail(Step::Workers, About::Machine, &e),
    }
    // One call, so a status line never draws a width that belongs to another text.
    let args = ["set", "-g", LONG, &long, ";", "set", "-g", SHORT, &short, ";", "set", "-g", WIDTH, &width].map(str::to_owned);
    let set: Result<String, CpdError> = engine.tmux.run(&args);
    match set {
        Ok(_) => {
            engine.workers = Some(state);
            engine.clear(Step::Workers, &About::Machine);
        }
        // Not remembered: the next tick tries again.
        Err(e) => engine.fail(Step::Workers, About::Machine, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::Counts;
    use rusqlite::Connection;

    fn counts(running: u32, limit: u32, waiting: u32, queued: u32) -> Counts {
        Counts { running, limit, waiting, queued }
    }

    #[test]
    fn none_running_none_waiting_is_colour_8_and_dim() {
        let c = counts(0, 3, 0, 0);
        assert_eq!(c.long(), "#[dim]workers #[nodim,fg=colour8]0#[default,dim]/3 · 0 waiting · 0 queued#[default]");
        assert_eq!(c.short(), "#[nodim,fg=colour8]0#[default,dim]/3·0·0#[default]");
    }

    #[test]
    fn some_running_is_green_and_waiting_above_0_is_the_review_colour() {
        let c = counts(6, 6, 4, 9);
        assert_eq!(
            c.long(),
            "#[dim]workers #[nodim,fg=green]6#[default,dim]/6 · #[nodim,fg=yellow]4#[default,dim] waiting · 9 queued#[default]"
        );
        assert_eq!(c.short(), "#[nodim,fg=green]6#[default,dim]/6·#[nodim,fg=yellow]4#[default,dim]·9#[default]");
    }

    #[test]
    fn the_width_is_the_visible_width_of_the_long_form() {
        assert_eq!(counts(6, 6, 4, 9).width(), "workers 6/6 · 4 waiting · 9 queued".chars().count());
        assert_eq!(counts(6, 6, 4, 9).width(), 34);
        assert_eq!(counts(0, 3, 0, 0).width(), 34);
        assert_eq!(counts(10, 12, 100, 1000).width(), 41);
    }

    /// The tables the Ready rule reads, with three Plans' worth of rows.
    fn store() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE attempts (status TEXT);
             CREATE TABLE plans (id TEXT, phase TEXT);
             CREATE TABLE prs (plan TEXT, key TEXT, status TEXT);
             CREATE TABLE jobs (plan TEXT, key TEXT, pr TEXT, status TEXT, seq INTEGER);
             CREATE TABLE dependencies (plan TEXT, job TEXT, needs TEXT);
             INSERT INTO plans VALUES ('pay', 'dispatch');
             INSERT INTO prs VALUES ('pay', 'a', 'dispatch'), ('pay', 'w', 'waiting');
             INSERT INTO attempts VALUES ('starting'), ('ended');
             -- ready: no Dependency
             INSERT INTO jobs VALUES ('pay', 'free', 'a', 'queued', 1);
             -- ready: its only Dependency is merged
             INSERT INTO jobs VALUES ('pay', 'done', 'a', 'merged', 2), ('pay', 'after', 'a', 'queued', 3);
             INSERT INTO dependencies VALUES ('pay', 'after', 'done');
             -- queued, not ready: a Dependency that is not merged
             INSERT INTO jobs VALUES ('pay', 'busy', 'a', 'running', 4), ('pay', 'blocked', 'a', 'queued', 5);
             INSERT INTO dependencies VALUES ('pay', 'blocked', 'busy');
             -- queued, not ready: its PR has not started
             INSERT INTO jobs VALUES ('pay', 'later', 'w', 'queued', 6);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn waiting_counts_only_ready_jobs_and_queued_counts_every_queued_job() {
        assert_eq!(Counts::read(&store(), 3).unwrap(), counts(1, 3, 2, 4));
    }
}
