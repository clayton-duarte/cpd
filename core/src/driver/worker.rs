//! The Worker (contract §3.4 A3, §6.4, §6.5; commands.md §16.1): the facts the
//! Worker Spec is rendered from, the Worker's pi launch in the hidden work
//! session, and its session file.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use super::ledger::Target;
use crate::config::{Catalog, Config, Tier};
use crate::error::{CpdError, ErrorCode};
use crate::git;
use crate::home::CpdHome;
use crate::host::layout::attempt_window;
use crate::host::pi::{self, PiDirs, WorkerLaunch, extensions_to_pass, worker_candidates};
use crate::host::{PaneId, Tmux, WindowId};
use crate::ids::{EscalationId, SessionId};
use crate::metrics;
use crate::plan::jobs::Job;
use crate::spec::{self, Answer, Cause, Input, Previous};

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
            let files = if cause == "merge_conflict" {
                let data: Option<String> = conn
                    .query_row(
                        "SELECT data FROM events WHERE plan = ?1 AND attempt = ?2
                            AND kind IN ('attempt-retry', 'job-failed') ORDER BY id DESC LIMIT 1",
                        params![target.plan.as_str(), id],
                        |r| r.get(0),
                    )
                    .optional()?;
                let data: Value = data.map_or(Value::Null, |d| serde_json::from_str(&d).unwrap_or(Value::Null));
                let detail = data.get("feedback").or_else(|| data.get("detail")).and_then(Value::as_str).unwrap_or("");
                conflict_files(detail)
            } else {
                Vec::new()
            };
            Ok(Previous {
                number,
                tier: Tier::parse(&tier).map_err(|_| bad(format!("tier {tier:?} of Attempt {id}")))?,
                cause: Cause::parse(&cause, files)?,
            })
        })
        .collect()
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
    let quick_check = spec::quick_check(&target.worktree)?;
    let protected = git::protected_list(&target.worktree, &git::lead_branch(&target.plan))?;
    let (touches, acceptance, out_of_scope) = (
        strings(&job.touches, "touches")?,
        strings(&job.acceptance, "acceptance")?,
        strings(&job.out_of_scope, "out_of_scope")?,
    );
    spec::write(
        home,
        &Input {
            plan: &target.plan,
            attempt: &target.id,
            title: &job.title,
            tier: target.tier,
            worktree: &target.worktree,
            goal: &job.goal,
            context: &job.context,
            touches: &touches,
            acceptance: &acceptance,
            out_of_scope: &out_of_scope,
            quick_check: &quick_check,
            protected: &protected,
            previous: &previous,
            answers: &answers,
        },
    )
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
    let launch = pi::worker(&WorkerLaunch {
        home,
        session,
        plan: &target.plan,
        attempt: &target.id,
        model: &model,
        extensions: &extensions,
    });
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
fn retry_kill(
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
            Err(_) => std::thread::sleep(pause),
        }
    }
}

/// A9: kills the Attempt's window in the work session, if any is left. A
/// failed `kill-window` is retried (see [`KILL_TRIES`]); a window already
/// gone counts as killed.
pub fn kill_window(config: &Config, tmux: &Tmux, target: &Target) -> Result<(), CpdError> {
    let name = attempt_window(&target.plan, &target.id);
    let windows: Vec<WindowId> = tmux
        .list_panes()?
        .into_iter()
        .filter(|p| p.session_name == config.tmux.work_session && p.window_name == name)
        .map(|p| p.window)
        .collect();
    for window in windows {
        let gone = || tmux.list_panes().is_ok_and(|panes| panes.iter().all(|p| p.window != window));
        retry_kill(KILL_TRIES, KILL_PAUSE, || tmux.kill_window(&window), gone)?;
    }
    Ok(())
}

/// A9's `metrics`, parsed from the Worker's session file (cpd 1 §14). None
/// when there is no Worker or no readable file yet; why is logged.
pub fn metrics(dirs: &PiDirs, session: Option<&SessionId>) -> Option<Value> {
    let file = pi::session_file(dirs, session?)?;
    match metrics::parse_file(&file) {
        Ok(m) => Some(m.to_json()),
        Err(e) => {
            eprintln!("cpd attempt run: no metrics from {}: {}", file.display(), e.message());
            None
        }
    }
}

/// The last assistant message's `stopReason` and the number of assistant
/// messages of `file`; None while it cannot be parsed (a line being written).
pub fn last_stop(file: &Path) -> Option<(Option<String>, u64)> {
    metrics::parse_file(file).ok().map(|m| (m.stop_reason, m.turns))
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
