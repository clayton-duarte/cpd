//! The Worker (contract §3.4 A3, §6.4, §6.5; commands.md §16.1): the facts the
//! Worker Spec is rendered from, the Worker's pi launch in the hidden work
//! session, and its session file.

use std::path::PathBuf;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use crate::attempt::ledger::Attempt as Target;
use crate::config::{Catalog, Config, Tier};
use crate::error::{CpdError, ErrorCode};
use crate::clock::Clock;
#[cfg(test)]
use crate::clock::SystemClock;
use crate::home::CpdHome;
use crate::host::git_guard;
use crate::host::layout::attempt_window;
use crate::host::pi::{self, PiDirs, WorkerLaunch, extensions_to_pass, worker_candidates};
use crate::host::{PaneId, Tmux, WindowId};
use crate::ids::{AttemptId, EscalationId, PlanId, SessionId};
use crate::plan::jobs::Job;
use crate::ports::Ports;
use crate::attempt::ledger::{Cause, Stored};
use crate::spec::{self, Answer, Evidence, Input, JobKind, Previous};

/// How the merge words a conflict's detail (`crate::merge`, §7.4 step 5):
/// the file list follows it, comma-separated, ending with a full stop.
const CONFLICT_PREFIX: &str = "The rebase onto the Lead Branch conflicted in: ";

/// The files of a `merge_conflict` detail, as the merge wrote them.
fn conflict_files(detail: &str) -> Vec<String> {
    let Some(list) = detail.strip_prefix(CONFLICT_PREFIX) else { return Vec::new() };
    let list = list.strip_suffix('.').unwrap_or(list);
    list.split(", ").filter(|f| !f.is_empty()).map(str::to_owned).collect()
}

fn bad(what: String) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("The Ledger holds an invalid value: {what}."))
}

/// The ended earlier Attempts of `target`'s Job, oldest first, each with what
/// explains its end: for `merge_conflict`, the files from its Event.
fn previous(conn: &Connection, target: &Target) -> Result<Vec<Previous>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT id, number, tier, cause FROM attempts
          WHERE plan = ?1 AND job = ?2 AND status = 'ended' AND number < ?3 ORDER BY number",
    )?;
    let rows = stmt
        .query_map(params![target.plan.as_str(), target.job.as_str(), target.id.number().get()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(id, number, tier, cause)| {
            let cause = spec::parse_cause(&cause)?;
            let evidence = match cause {
                Stored::Failed(Cause::MergeConflict) => Evidence::Files(conflict_files(&end_detail(conn, target, &id)?)),
                Stored::Failed(Cause::GateFailed | Cause::GateError) => {
                    Evidence::Gate(crate::merge::parse_gate_detail(&end_detail(conn, target, &id)?))
                }
                Stored::Failed(Cause::ProviderError) => Evidence::Message(end_detail(conn, target, &id)?),
                _ => Evidence::None,
            };
            Ok(Previous {
                number,
                tier: Tier::parse(&tier).map_err(|_| bad(format!("tier {tier:?} of Attempt {id}")))?,
                cause,
                evidence,
            })
        })
        .collect()
}

/// The `detail` of the end Event of Attempt `id` (`attempt-retry`'s `feedback` or `job-failed`'s `detail`); empty when it has none.
fn end_detail(conn: &Connection, target: &Target, id: &str) -> Result<String, CpdError> {
    let data: Option<String> = conn
        .query_row(
            "SELECT data FROM events WHERE plan = ?1 AND attempt = ?2
                AND kind IN ('attempt-retry', 'job-failed') ORDER BY id DESC LIMIT 1",
            params![target.plan.as_str(), id],
            |r| r.get(0),
        )
        .optional()?;
    let data: Value = data.map_or(Value::Null, |d| serde_json::from_str(&d).unwrap_or(Value::Null));
    Ok(data.get("feedback").or_else(|| data.get("detail")).and_then(Value::as_str).unwrap_or("").to_owned())
}

/// The `answered` Escalations of earlier Attempts of `target`'s Job, oldest first.
fn answers(conn: &Connection, target: &Target) -> Result<Vec<Answer>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT e.id, e.question, e.answer FROM escalations e
           JOIN attempts a ON a.plan = e.plan AND a.id = e.attempt
          WHERE e.plan = ?1 AND a.job = ?2 AND a.number < ?3 AND e.status = 'answered'
          ORDER BY a.number, e.number",
    )?;
    let rows = stmt
        .query_map(params![target.plan.as_str(), target.job.as_str(), target.id.number().get()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(|(id, question, answer)| {
            Ok(Answer { id: EscalationId::parse(&id).map_err(|_| bad(format!("escalation id {id:?}")))?, question, answer })
        })
        .collect()
}

fn strings(value: &Value, what: &str) -> Result<Vec<String>, CpdError> {
    value
        .as_array()
        .and_then(|a| a.iter().map(|v| v.as_str().map(str::to_owned)).collect::<Option<Vec<_>>>())
        .ok_or_else(|| bad(format!("{what} {value}")))
}

/// A3: renders the Worker Spec to `run/<plan>/specs/<attempt>.md` and returns its path.
pub fn render_spec(conn: &Connection, home: &CpdHome, target: &Target, job: &Job) -> Result<PathBuf, CpdError> {
    let previous = previous(conn, target)?;
    let answers = answers(conn, target)?;
    // An explore Job has no PR, no quick check and no protected path of a policy: its Worker works in no repository.
    let explore = crate::plan::jobs::is_explore(&job.kind);
    let (quick_check, protected) = if explore {
        (Vec::new(), Vec::new())
    } else {
        let policy = crate::pr::policy_of_job(conn, home, &target.plan, &job.key)?;
        (spec::quick_check(&policy.hooks.pre_commit), policy.protected.paths.clone())
    };
    let artefact = super::explore::artefact_in(&target.worktree, &job.key);
    let job_spec = job.require_spec()?;
    let (touches, acceptance, out_of_scope) = (
        strings(&job_spec.touches, "touches")?,
        strings(&job_spec.acceptance, "acceptance")?,
        strings(&job_spec.out_of_scope, "out_of_scope")?,
    );
    // The kind's facts are read as stored now: an approval reaches an Attempt that is running (§7.7).
    let all = crate::plan::jobs::load(conn, &target.plan)?;
    let guard = crate::job::guard::of(&all, job);
    let (own_tests, own_files) = crate::job::guard::own(job);
    let resources = crate::job::guard::resources(job);
    let kind = match job.kind.as_str() {
        "work" => JobKind::Work,
        "docs" => JobKind::Docs,
        "test" => JobKind::Test { tests: &own_tests, test_files: &own_files },
        "impl" => JobKind::Impl { test_jobs: &guard.jobs, tests: &guard.tests, guarded: &guard.files, approved: &guard.approved },
        "verify" => JobKind::Verify,
        "open" => JobKind::Open,
        "update" => JobKind::Update,
        "scout" | "research" => JobKind::Explore { kind: &job.kind, artefact: &artefact },
        other => return Err(bad(format!("kind {other:?} of Job {}", job.key))),
    };
    let path = spec::write(
        home,
        &Input {
            kind,
            plan: &target.plan,
            attempt: &target.id,
            title: &job.title,
            tier: target.tier,
            worktree: &target.worktree,
            goal: &job_spec.goal,
            context: &job_spec.context,
            touches: &touches,
            acceptance: &acceptance,
            out_of_scope: &out_of_scope,
            resources: &resources,
            quick_check: &quick_check,
            protected: &protected,
            previous: &previous,
            answers: &answers,
        },
    )?;
    Ok(path)
}

/// The Worker's pane, once pi runs in it.
#[derive(Clone, Debug)]
pub struct Pane {
    pub pane: PaneId,
    pub window: WindowId,
}

/// A3: starts pi as Worker `session` in window `<plan>--<attempt>` of the work
/// session (creating the session when it is missing), cwd the Attempt
/// worktree, and marks the pane with `@cpd_session` (§6.4, §6.5).
pub fn start(
    home: &CpdHome,
    config: &Config,
    catalog: &Catalog,
    tmux: &Tmux,
    dirs: &PiDirs,
    target: &Target,
    session: &SessionId,
) -> Result<Pane, CpdError> {
    let model = catalog.model_arg(target.tier)?;
    let adapter = config.pi_adapter_dir()?;
    let extensions = extensions_to_pass(&worker_candidates(&adapter), &target.worktree, dirs, &mut std::io::stderr());
    git_guard::install(home)?;
    let mut launch = pi::worker(&WorkerLaunch {
        home,
        session,
        plan: &target.plan,
        attempt: &target.id,
        model: &model,
        extensions: &extensions,
    });
    super::runner::explore_env(&mut launch.env, target);
    let spawn = launch.spawn(&target.worktree);
    let work = config.tmux.work_session.as_str();
    let name = attempt_window(&target.plan, &target.id);
    let (window, pane) = match tmux.create_session(work, &name, &spawn)? {
        Some(ids) => ids,
        None => tmux.new_window(work, &name, &spawn)?,
    };
    tmux.set_pane_session(&pane, session)?;
    Ok(Pane { pane, window })
}

/// A9 (§3.4): `kill-window` is tried this many times, `KILL_PAUSE` apart.
const KILL_TRIES: u32 = 3;
const KILL_PAUSE: Duration = Duration::from_secs(1);

/// Runs `kill` up to `tries` times, `pause` apart; a failure after which
/// `gone` says the window is already gone counts as success. The last error
/// is returned when every try failed.
#[cfg(test)]
fn retry_kill(
    tries: u32,
    pause: Duration,
    kill: impl FnMut() -> Result<(), CpdError>,
    gone: impl FnMut() -> bool,
) -> Result<(), CpdError> {
    retry_kill_with(&SystemClock, tries, pause, kill, gone)
}

/// [`retry_kill`] whose `pause` is a nap on `clock`, never after the last try.
pub(super) fn retry_kill_with(
    clock: &dyn Clock,
    tries: u32,
    pause: Duration,
    mut kill: impl FnMut() -> Result<(), CpdError>,
    mut gone: impl FnMut() -> bool,
) -> Result<(), CpdError> {
    let mut tried = 0;
    loop {
        tried += 1;
        match kill() {
            Ok(()) => return Ok(()),
            Err(_) if gone() => return Ok(()),
            Err(e) if tried >= tries => return Err(e),
            Err(_) => clock.sleep(pause),
        }
    }
}

/// A9: kills the Attempt's window in the work session, if any is left. A
/// failed `kill-window` is retried (see [`KILL_TRIES`]); a window already
/// gone counts as killed.
pub fn kill_window(config: &Config, tmux: &Tmux, target: &Target) -> Result<(), CpdError> {
    kill_attempt_window(config, tmux, &target.plan, &target.id)
}

/// [`kill_window`] for Attempt `id` of `plan`: the one A9 kill, shared by the
/// driver, E2 and R.
pub fn kill_attempt_window(config: &Config, tmux: &Tmux, plan: &PlanId, id: &AttemptId) -> Result<(), CpdError> {
    kill_attempt_window_with(&Ports::current(), config, tmux, plan, id)
}

/// [`kill_attempt_window`] on `ports`: the pause between two tries of `kill-window` naps on `ports.clock`.
pub fn kill_attempt_window_with(
    ports: &Ports,
    config: &Config,
    tmux: &Tmux,
    plan: &PlanId,
    id: &AttemptId,
) -> Result<(), CpdError> {
    let name = attempt_window(plan, id);
    let windows: Vec<WindowId> = tmux
        .list_panes()?
        .into_iter()
        .filter(|p| p.session_name == config.tmux.work_session && p.window_name == name)
        .map(|p| p.window)
        .collect();
    for window in windows {
        let gone = || tmux.list_panes().is_ok_and(|panes| panes.iter().all(|p| p.window != window));
        retry_kill_with(&*ports.clock, KILL_TRIES, KILL_PAUSE, || tmux.kill_window(&window), gone)?;
    }
    Ok(())
}

/// Whether a pi `errorMessage` is a provider error (decision pro-d102): exactly "Connection error." or "Request timed
/// out.", or a leading HTTP 5xx status (a 3-digit code 500-599 followed by a space or the end of the message).
pub fn is_provider_error(error_message: &str) -> bool {
    if matches!(error_message, "Connection error." | "Request timed out.") {
        return true;
    }
    let bytes = error_message.as_bytes();
    bytes.len() >= 3
        && bytes[0] == b'5'
        && bytes[1].is_ascii_digit()
        && bytes[2].is_ascii_digit()
        && bytes.get(3).is_none_or(|b| *b == b' ')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failure() -> CpdError {
        CpdError::new(ErrorCode::TmuxFailed, "kill failed")
    }

    #[test]
    fn a_kill_that_fails_three_times_reports_the_error() {
        let mut calls = 0;
        let result = retry_kill(3, Duration::ZERO, || { calls += 1; Err(failure()) }, || false);
        assert_eq!(calls, 3);
        assert_eq!(result.unwrap_err().code(), ErrorCode::TmuxFailed);
    }

    #[test]
    fn a_kill_that_fails_twice_then_works_is_ok() {
        let mut calls = 0;
        let result = retry_kill(3, Duration::ZERO, || { calls += 1; if calls < 3 { Err(failure()) } else { Ok(()) } }, || false);
        assert_eq!(calls, 3);
        assert!(result.is_ok());
    }

    #[test]
    fn a_window_already_gone_is_success_without_a_retry() {
        let mut calls = 0;
        let result = retry_kill(3, Duration::ZERO, || { calls += 1; Err(failure()) }, || true);
        assert_eq!(calls, 1);
        assert!(result.is_ok());
    }

    #[test]
    fn conflict_files_come_from_the_merges_detail() {
        assert_eq!(conflict_files("The rebase onto the Lead Branch conflicted in: a.txt, src/b.rs."), ["a.txt", "src/b.rs"]);
        assert!(conflict_files("Something else.").is_empty());
    }
}
