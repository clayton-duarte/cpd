//! The `worker` view (commands.md §13.2, §17): an Attempt's Worker, read-only,
//! beside the chat; ported from the spike's `run_worker` (branch `cpd/cpd-ui`,
//! `tui/src/main.rs`) onto the read model.
//!
//! `cpd tui view worker <attempt> --plan <id>` (the Attempt form) or `cpd tui view worker --job <key> --plan <id>` (the
//! Job form: the Job's latest Attempt, re-resolved at each snapshot, or a placeholder while it has none) finds the Worker's pi session
//! file through the Attempt's Session (`read::worker`, what `attempt show`
//! returns) and renders it ([`render`]): what the Worker was told, said and
//! did. The file is not in the database. Every [`CHECK`] the view looks at
//! the file's size and modification time and re-reads it when either changed,
//! and, when an Event came (followed in process, [`read::Follow`]), takes a new snapshot of the
//! Attempt and its Session, which gives the state at the foot and the file once the driver has registered it. The view
//! writes nothing and starts no process.
//!
//! The header, pinned above the body, says why the Job is `cancelled`, `failed` or waiting on other Jobs ([`header`]).
//!
//! Keys: Up/`k`, Down/`j`, PageUp, PageDown and the mouse wheel scroll; End or
//! `G` follows the end again (the view starts there); `q` or Esc closes its pane.

pub(crate) mod render;
mod view;

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use ratatui::backend::Backend;
use ratatui::crossterm::event::Event;
use ratatui::text::Line;

use super::read::{self, JobStatus, Target, Worker};
use super::runner::kept;
use super::view_ports::Ctx;
use super::{Capture, run_view_on};
use cpd_core::args::PlanArg;
use cpd_core::config::Config;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::{PaneId, Tmux};
use cpd_core::ids::{AttemptId, JobKey, PlanId};
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::store::Store;
use view::View;

/// How often the view looks at the session file and the store (§17).
pub(crate) const CHECK: Duration = Duration::from_millis(500);
/// How long the loop waits for a key before it looks at the clock again.
pub(crate) const TICK: Duration = Duration::from_millis(100);

/// `cpd tui view worker <attempt> --plan <id>` or `cpd tui view worker --job <key> --plan <id>`.
#[derive(clap::Args, Debug)]
#[command(group(clap::ArgGroup::new("target").required(true).multiple(false).args(["attempt", "job"])))]
pub struct Args {
    /// The Attempt whose Worker to show
    #[arg(value_name = "attempt")]
    attempt: Option<AttemptId>,
    /// The Job whose latest Attempt's Worker to show
    #[arg(long, value_name = "key")]
    job: Option<JobKey>,
    #[command(flatten)]
    plan: PlanArg,
}

pub(super) fn run(ports: &Ports, args: Args) -> Result<Output, CpdError> {
    let plan = args.plan.require()?;
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let target = match (args.attempt, args.job) {
        (Some(attempt), None) => Target::Attempt(attempt),
        (None, Some(job)) => Target::Job(job),
        _ => unreachable!("clap's group `target` takes exactly one"),
    };
    // `plan_not_found`, `attempt_not_found` and `job_not_found` refuse before anything is drawn.
    let snapshot = read::worker(&mut store, plan, &target)?;
    let follow = read::Follow::new(&mut store, &[], snapshot.events_cursor)?;
    let output = run_view_on(
        ports,
        "worker",
        Capture::Mouse,
        || Ok(snapshot),
        |ctx, snapshot| Ok(event_loop_with(ctx, &mut store, follow, plan, &target, snapshot)?),
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

/// The loop of the view: input, the clock and frames are the [`Ctx`]'s.
pub fn event_loop_with<B: Backend>(
    ctx: &mut Ctx<B>,
    store: &mut Store,
    mut follow: read::Follow,
    plan: &PlanId,
    target: &Target,
    first: Worker,
) -> std::io::Result<()> {
    let mut view = View::new();
    let mut shown: Option<Stamp> = None;
    let mut snapshot = Some(first);
    let mut last_check = ctx.ports.clock.now();
    refresh(&mut view, &mut shown, snapshot.as_ref());
    let mut redraw = true; // the pane is written to only when something changed: the Worker, its file, input, a resize
    loop {
        if ctx.ports.clock.now().saturating_duration_since(last_check) >= CHECK {
            let before = (view.status.clone(), view.header.clone(), view.note.clone(), shown.clone());
            last_check = ctx.ports.clock.now();
            // a failed poll or read keeps the snapshot, the error is the note, the next check tries again (runner::kept)
            let mut error = None;
            let retry = !view.note.is_empty();
            if let Some(events) = kept(follow.poll(store), &mut error) {
                if retry || !events.is_empty() {
                    if let Some(worker) = kept(read::worker(store, plan, target), &mut error) {
                        snapshot = Some(worker);
                    }
                }
            }
            view.note = error.unwrap_or_default();
            refresh(&mut view, &mut shown, snapshot.as_ref());
            redraw |= before != (view.status.clone(), view.header.clone(), view.note.clone(), shown.clone());
        }
        if redraw {
            ctx.draw(|f| view.draw(f))?;
            redraw = false;
        }
        if !ctx.input.poll(TICK)? {
            continue;
        }
        redraw = true;
        match ctx.input.read()? {
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

/// The foot's text: the Attempt, its status and its Session's state; for a Job with no Attempt, the Job's key and its display
/// status (`waiting` for a queued Job that waits for something).
fn foot(worker: &Worker) -> String {
    let Some(attempt) = &worker.attempt else {
        let job = &worker.job;
        let status = if job.status == JobStatus::Queued && !job.waiting_on.is_empty() { "waiting" } else { job.status.as_str() };
        return format!("{} {status}", job.key);
    };
    let session = worker.session.as_ref().map_or_else(
        || "no Session yet".to_owned(),
        |s| match &s.state_label {
            Some(label) => format!("{} {label}", s.state.as_str()),
            None => s.state.as_str().to_owned(),
        },
    );
    format!("{} {} · {session}", attempt.id, attempt.status.as_str())
}

/// Why the Job is in its state, the header's text: `cancelled[: <reason>]`, `failed[: <cause>]`, `waiting on <keys>`; empty
/// for any other state.
fn header(worker: &Worker) -> String {
    let job = &worker.job;
    let with = |word: &str, why: &Option<String>| match why {
        Some(why) => format!("{word}: {why}"),
        None => word.to_owned(),
    };
    match job.status {
        JobStatus::Cancelled => with("cancelled", &job.cancel_reason),
        JobStatus::Failed => with("failed", &job.failed_cause),
        JobStatus::Queued if !job.waiting_on.is_empty() => format!("waiting on {}", job.waiting_on.join(", ")),
        _ => String::new(),
    }
}

/// Takes the foot's status from `snapshot` and re-reads the session file when it is not the one `shown`.
fn refresh(view: &mut View, shown: &mut Option<Stamp>, snapshot: Option<&Worker>) {
    let Some(worker) = snapshot else { return };
    view.status = foot(worker);
    view.header = header(worker);
    if worker.attempt.is_none() {
        *shown = None;
        view.set_lines(vec![placeholder("No Attempt yet: the Job has not started.")]);
        return;
    }
    let Some(path) = worker.session.as_ref().and_then(|s| s.file.as_deref()) else {
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
    Line::styled(format!(" {text}"), crate::palette::muted_text())
}

#[cfg(test)]
mod screen_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::{AttemptDetail, AttemptStatus, Cause, Origin, Outcome, WorkerJob};
    use cpd_core::config::Tier;

    fn job(status: JobStatus) -> WorkerJob {
        WorkerJob {
            key: JobKey::parse("api").unwrap(),
            title: "Job api".into(),
            status,
            cancel_reason: None,
            failed_cause: None,
            waiting_on: Vec::new(),
        }
    }

    fn worker(job: WorkerJob, attempt: Option<AttemptStatus>) -> Worker {
        Worker {
            plan: PlanId::parse("ship").unwrap(),
            job,
            attempt: attempt.map(|status| AttemptDetail {
                id: AttemptId::parse("api-a1").unwrap(),
                tier: Tier::parse("S3").unwrap(),
                origin: Origin::Ladder,
                status,
                outcome: Some(Outcome::Failed),
                cause: Some(Cause::Crashed),
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
            }),
            session: None,
            escalations: Vec::new(),
            events_cursor: 1,
        }
    }

    #[test]
    fn the_foot_of_an_attempt_form_names_the_attempt_and_never_the_cause() {
        let failed = worker(WorkerJob { failed_cause: Some("crashed".into()), ..job(JobStatus::Failed) }, Some(AttemptStatus::Ended));
        assert_eq!(foot(&failed), "api-a1 ended · no Session yet");
        assert_eq!(foot(&worker(job(JobStatus::Running), Some(AttemptStatus::Running))), "api-a1 running · no Session yet");
    }

    #[test]
    fn the_foot_of_a_job_form_is_the_key_and_the_display_status() {
        assert_eq!(foot(&worker(job(JobStatus::Cancelled), None)), "api cancelled");
        assert_eq!(foot(&worker(job(JobStatus::Queued), None)), "api queued");
        let waiting = WorkerJob { waiting_on: vec!["a".into()], ..job(JobStatus::Queued) };
        assert_eq!(foot(&worker(waiting, None)), "api waiting");
    }

    #[test]
    fn the_header_says_why_a_job_is_cancelled_failed_or_waiting() {
        let h = |job| header(&worker(job, None));
        assert_eq!(h(WorkerJob { cancel_reason: Some("gone".into()), ..job(JobStatus::Cancelled) }), "cancelled: gone");
        assert_eq!(h(job(JobStatus::Cancelled)), "cancelled");
        assert_eq!(h(WorkerJob { failed_cause: Some("crashed".into()), ..job(JobStatus::Failed) }), "failed: crashed");
        assert_eq!(h(WorkerJob { waiting_on: vec!["a".into(), "b".into()], ..job(JobStatus::Queued) }), "waiting on a, b");
        assert_eq!(h(job(JobStatus::Queued)), "");
        assert_eq!(h(job(JobStatus::Merged)), "");
    }
}
