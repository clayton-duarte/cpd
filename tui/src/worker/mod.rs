//! The `worker` view (commands.md §13.2, §17): an Attempt's Worker, read-only,
//! beside the chat; ported from the spike's `run_worker` (branch `cpd/cpd-ui`,
//! `tui/src/main.rs`) onto the read model.
//!
//! `cpd tui view worker <attempt> --plan <id>` finds the Worker's pi session
//! file through the Attempt's Session (`read::worker`, what `attempt show`
//! returns) and renders it ([`render`]): what the Worker was told, said and
//! did. The file is not in the database. Every [`CHECK`] the view looks at
//! the file's size and modification time and re-reads it when either changed,
//! and, when an Event came (followed in process, [`read::Follow`]), takes a new snapshot of the
//! Attempt and its Session, which gives the state at the foot and the file once the driver has registered it. The view
//! writes nothing and starts no process.
//!
//! Keys: Up/`k`, Down/`j`, PageUp, PageDown and the mouse wheel scroll; End or
//! `G` follows the end again (the view starts there); `q` or Esc closes its pane.

pub(crate) mod render;
mod view;

use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event};
use ratatui::style::Style;
use ratatui::text::Line;

use super::read::{self, AttemptStatus, Outcome, Worker};
use super::runner::kept;
use super::{Capture, run_view};
use cpd_core::args::PlanArg;
use cpd_core::config::Config;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::{PaneId, Tmux};
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::output::Output;
use cpd_core::store::Store;
use view::View;

/// How often the view looks at the session file and the store (§17).
const CHECK: Duration = Duration::from_millis(500);
/// How long the loop waits for a key before it looks at the clock again.
const TICK: Duration = Duration::from_millis(100);

/// `cpd tui view worker <attempt> --plan <id>`.
#[derive(clap::Args, Debug)]
pub struct Args {
    /// The Attempt whose Worker to show
    #[arg(value_name = "attempt")]
    attempt: AttemptId,
    #[command(flatten)]
    plan: PlanArg,
}

pub(super) fn run(args: Args) -> Result<Output, CpdError> {
    let plan = args.plan.require()?;
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    // `plan_not_found` and `attempt_not_found` refuse before anything is drawn.
    let snapshot = read::worker(&mut store, plan, &args.attempt)?;
    let follow = read::Follow::new(&mut store, &[], snapshot.events_cursor)?;
    let output = run_view(
        "worker",
        Capture::Mouse,
        || Ok(snapshot),
        |terminal, snapshot| Ok(event_loop(terminal, &mut store, follow, plan, &args.attempt, snapshot)?),
    )?;
    close_pane(&config)?;
    Ok(output)
}

/// `q` closes the view's pane: the cockpit's `pane-died` hook starts a view again whenever its program exits
/// (contract §6.1), so exiting would be a restart. Outside tmux there is no pane to close.
fn close_pane(config: &Config) -> Result<(), CpdError> {
    let Some(pane) = std::env::var("TMUX_PANE").ok().and_then(|p| PaneId::parse(&p)) else { return Ok(()) };
    Tmux::new(config.tmux.socket.clone()).kill_pane(&pane)
}

/// What identifies the version of the session file last rendered.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    store: &mut Store,
    mut follow: read::Follow,
    plan: &PlanId,
    attempt: &AttemptId,
    first: Worker,
) -> std::io::Result<()> {
    let mut view = View::new();
    let mut shown: Option<Stamp> = None;
    let mut snapshot = Some(first);
    let mut last_check = Instant::now();
    refresh(&mut view, &mut shown, snapshot.as_ref());
    let mut redraw = true; // the pane is written to only when something changed: the Worker, its file, input, a resize
    loop {
        if last_check.elapsed() >= CHECK {
            let before = (view.status.clone(), view.note.clone(), shown.clone());
            last_check = Instant::now();
            // a failed poll or read keeps the snapshot, the error is the note, the next check tries again (runner::kept)
            let mut error = None;
            let retry = !view.note.is_empty();
            if let Some(events) = kept(follow.poll(store), &mut error) {
                if retry || !events.is_empty() {
                    if let Some(worker) = kept(read::worker(store, plan, attempt), &mut error) {
                        snapshot = Some(worker);
                    }
                }
            }
            view.note = error.unwrap_or_default();
            refresh(&mut view, &mut shown, snapshot.as_ref());
            redraw |= before != (view.status.clone(), view.note.clone(), shown.clone());
        }
        if redraw {
            terminal.draw(|f| view.draw(f))?;
            redraw = false;
        }
        if !event::poll(TICK)? {
            continue;
        }
        redraw = true;
        match event::read()? {
            Event::Key(key) => {
                if view.key(key) {
                    return Ok(());
                }
            }
            Event::Mouse(m) => view.mouse(m),
            _ => {}
        }
    }
}

/// The foot's text: the Attempt, its status and its Session's state, then `failed: <cause>` once the Attempt ended failed
/// (the same cause as its Job's `failed_cause`), so that the `job-stuck` click shows why the Job is stuck.
fn foot(worker: &Worker) -> String {
    let session = worker.session.as_ref().map_or_else(
        || "no Session yet".to_owned(),
        |s| match &s.state_label {
            Some(label) => format!("{} {label}", s.state.as_str()),
            None => s.state.as_str().to_owned(),
        },
    );
    let mut foot = format!("{} {} · {session}", worker.attempt.id, worker.attempt.status.as_str());
    if let (AttemptStatus::Ended, Some(Outcome::Failed), Some(cause)) =
        (worker.attempt.status, worker.attempt.outcome, worker.attempt.cause)
    {
        foot.push_str(&format!(" · failed: {}", cause.as_str()));
    }
    foot
}

/// Takes the foot's status from `snapshot` and re-reads the session file when it is not the one `shown`.
fn refresh(view: &mut View, shown: &mut Option<Stamp>, snapshot: Option<&Worker>) {
    let Some(worker) = snapshot else { return };
    let session = worker.session.as_ref();
    view.status = foot(worker);
    let Some(path) = session.and_then(|s| s.file.as_deref()) else {
        *shown = None;
        view.set_lines(vec![placeholder("No session file yet: the Worker has not started its Session.")]);
        return;
    };
    match std::fs::metadata(path) {
        Ok(meta) => {
            let stamp = Stamp { path: path.into(), len: meta.len(), modified: meta.modified().ok() };
            if shown.as_ref() == Some(&stamp) {
                return;
            }
            match std::fs::read_to_string(path) {
                Ok(text) => {
                    view.set_lines(render::render(&text));
                    *shown = Some(stamp);
                }
                Err(e) => view.note = format!("Cannot read {path}: {e}"),
            }
        }
        Err(e) => {
            *shown = None;
            view.set_lines(vec![placeholder(&format!("No session file at {path}: {e}"))]);
        }
    }
}

fn placeholder(text: &str) -> Line<'static> {
    Line::styled(format!(" {text}"), Style::new().dim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::{AttemptDetail, Cause, JobStatus, Origin};
    use cpd_core::config::Tier;

    fn worker(status: AttemptStatus, outcome: Option<Outcome>, cause: Option<Cause>) -> Worker {
        Worker {
            plan: PlanId::parse("ship").unwrap(),
            attempt: AttemptDetail {
                id: AttemptId::parse("api-a1").unwrap(),
                tier: Tier::parse("S3").unwrap(),
                origin: Origin::Ladder,
                status,
                outcome,
                cause,
                branch: "cpd/ship--api-a1".into(),
                worktree: "/w".into(),
                worker_session: None,
                spec_path: None,
                submitted_sha: None,
                submitted_at: None,
                check_tail: None,
                merged_sha: None,
                nudged: false,
                stop_request: None,
                metrics: None,
                started_at: "2026-01-03T00:01:00.000Z".into(),
                ended_at: None,
            },
            job_title: "Job api".into(),
            job_status: JobStatus::Failed,
            session: None,
            escalations: Vec::new(),
            events_cursor: 1,
        }
    }

    #[test]
    fn the_foot_of_an_attempt_that_ended_failed_says_why() {
        let failed = worker(AttemptStatus::Ended, Some(Outcome::Failed), Some(Cause::Crashed));
        assert_eq!(foot(&failed), "api-a1 ended · no Session yet · failed: crashed");
        let merge = worker(AttemptStatus::Ended, Some(Outcome::Failed), Some(Cause::MergeConflict));
        assert_eq!(foot(&merge), "api-a1 ended · no Session yet · failed: merge_conflict");
    }

    #[test]
    fn the_foot_of_any_other_attempt_says_no_failure() {
        assert_eq!(foot(&worker(AttemptStatus::Running, None, None)), "api-a1 running · no Session yet");
        assert_eq!(
            foot(&worker(AttemptStatus::Ended, Some(Outcome::Merged), Some(Cause::Merged))),
            "api-a1 ended · no Session yet"
        );
        assert_eq!(
            foot(&worker(AttemptStatus::Ended, Some(Outcome::Cancelled), Some(Cause::CancelRequested))),
            "api-a1 ended · no Session yet"
        );
    }
}
