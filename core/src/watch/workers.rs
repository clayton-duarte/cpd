//! The `@cpd_workers` step (contract §6.1; commands.md §12; User, hs-d149,
//! hs-d198): the right side of the StatusBar, which tmux's status line cannot
//! compute: the styled Inbox counter `inbox <n>` (commands.md §13.2), the
//! styled `workers N/M · W waiting · Q queued` text, its short form `N/M·W·Q`
//! and the width of the long right side. Every tick, `<n>` is the open,
//! visible Inbox items (the read of `cpd inbox list`), in the colour of the
//! most urgent of them, `N` is the Attempts not `ended` (the slots of the
//! `widgets` view), `M` is `concurrency`, `W` the Ready Jobs (E3's rule) and
//! `Q` the `queued` Jobs; the four options are set in one `tmux set` call only
//! when one of them differs from what was last set (the first tick always
//! sets them, and so does the first tick after a tmux failure or a missing
//! server: a new server starts without them). No row, no Event, except an `error` when tmux
//! fails while a server runs and no tmux restart is pending (a pending restart
//! takes the server away under the step: that failure is expected and not
//! reported): with no server there is no status line to feed, and the step
//! waits.

use time::OffsetDateTime;

use super::engine::Engine;
use crate::clock::timestamp;
use super::report::{About, Step};
use super::restart;
use super::schedule::READY;
use crate::error::CpdError;
use crate::inbox::model::{self, Filter, State, Urgency};

/// The options the status line draws: the Inbox counter, the long form, the short form, and the width in cells of
/// the right items (the Inbox counter's pill, one space and the workers' pill, each pill with its two caps) in the
/// long form and in the short form.
const INBOX: &str = "@cpd_inbox";
const LONG: &str = "@cpd_workers";
const SHORT: &str = "@cpd_workers_short";
const WIDTH: &str = "@cpd_workers_width";
const SHORT_WIDTH: &str = "@cpd_workers_short_width";

/// The cells of a pill's two caps, which the status line's format draws around each text.
const CAPS: usize = 2;

/// The Attempts holding a slot (§3.4): the same query as the `widgets` view's slots.
const HELD: &str = "SELECT count(*) FROM attempts WHERE status <> 'ended'";

/// Every `queued` Job, the waiting ones included.
const QUEUED: &str = "SELECT count(*) FROM jobs WHERE status = 'queued'";

/// The Inbox counter of the StatusBar.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
struct Inbox {
    /// The open items that are neither snoozed nor dismissed.
    count: usize,
    /// The urgency of the most urgent of them; `None` when `count` is 0.
    urgency: Option<Urgency>,
}

impl Inbox {
    /// The visible items of every Context at `now`, as `cpd inbox list` reads them.
    fn read(conn: &rusqlite::Connection, now: String) -> Result<Inbox, CpdError> {
        let items = model::list(conn, &Filter { context: None, state: State::Visible { now } })?;
        Ok(Inbox { count: items.len(), urgency: items.iter().map(|i| i.urgency).max() })
    }

    /// `inbox <n>` in the colour of the most urgent item; dim when there is none.
    fn styled(self) -> String {
        match self.urgency {
            Some(urgency) => format!("#[fg={}]inbox {}#[default]", colour(urgency), self.count),
            None => format!("#[dim]inbox {}#[default]", self.count),
        }
    }

    /// The visible width in cells.
    fn width(self) -> usize {
        format!("inbox {}", self.count).chars().count()
    }
}

/// The tmux colour of an urgency: the tree's `󰪥` colours (the palette's `dot_span`: red, yellow, magenta, white).
fn colour(urgency: Urgency) -> &'static str {
    match urgency {
        Urgency::Red => "red",
        Urgency::Yellow => "yellow",
        Urgency::Magenta => "magenta",
        Urgency::White => "white",
    }
}

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

    /// The short form's visible width in cells.
    fn short_width(self) -> usize {
        format!("{}/{}·{}·{}", self.running, self.limit, self.waiting, self.queued).chars().count()
    }
}

pub(super) fn run(engine: &mut Engine) {
    let limit = engine.config.concurrency;
    let now = timestamp(OffsetDateTime::now_utc());
    let read = engine.store.read(|conn| Ok((Inbox::read(conn, now)?, Counts::read(conn, limit)?)));
    let (inbox, counts) = match read {
        Ok(read) => read,
        Err(e) => return engine.fail(Step::Workers, About::Machine, &e),
    };
    let (styled, long, short) = (inbox.styled(), counts.long(), counts.short());
    // The status line draws each text in a pill and puts one space between the two pills.
    let right = |workers: usize| (inbox.width() + CAPS + 1 + workers + CAPS).to_string();
    let (width, short_width) = (right(counts.width()), right(counts.short_width()));
    // What was last set: the five values, one string (no value holds a newline).
    let state = format!("{styled}\n{long}\n{short}\n{width}\n{short_width}");
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
    // One call, so a status line never draws a width that belongs to another text.
    let args = ["set", "-g", INBOX, &styled, ";", "set", "-g", LONG, &long, ";", "set", "-g", SHORT, &short, ";", "set", "-g", WIDTH, &width, ";", "set", "-g", SHORT_WIDTH, &short_width]
        .map(str::to_owned);
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
mod tests {
    use super::{Counts, Inbox};
    use crate::inbox::model::Urgency;
    use rusqlite::Connection;

    #[test]
    fn no_inbox_item_is_a_dim_zero() {
        let inbox = Inbox { count: 0, urgency: None };
        assert_eq!(inbox.styled(), "#[dim]inbox 0#[default]");
        assert_eq!(inbox.width(), 7);
    }

    #[test]
    fn the_inbox_counter_is_in_the_colour_of_the_most_urgent_item() {
        for (urgency, styled) in [
            (Urgency::Red, "#[fg=red]inbox 12#[default]"),
            (Urgency::Yellow, "#[fg=yellow]inbox 12#[default]"),
            (Urgency::Magenta, "#[fg=magenta]inbox 12#[default]"),
            (Urgency::White, "#[fg=white]inbox 12#[default]"),
        ] {
            let inbox = Inbox { count: 12, urgency: Some(urgency) };
            assert_eq!(inbox.styled(), styled, "{urgency:?}");
            assert_eq!(inbox.width(), 8);
        }
    }

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

    #[test]
    fn the_short_width_is_the_visible_width_of_the_short_form() {
        assert_eq!(counts(6, 6, 4, 9).short_width(), "6/6·4·9".chars().count());
        assert_eq!(counts(10, 12, 100, 1000).short_width(), 14);
    }

    /// The tables the Ready rule reads, with three Plans' worth of rows.
    fn store() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE attempts (status TEXT);
             CREATE TABLE plans (id TEXT, phase TEXT);
             CREATE TABLE prs (plan TEXT, key TEXT, status TEXT);
             CREATE TABLE jobs (plan TEXT, key TEXT, pr TEXT, status TEXT, seq INTEGER, urgent INTEGER NOT NULL DEFAULT 0);
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
