//! The Worker (contract §3.4 A3, §6.4, §6.5; commands.md §16.1): the facts the
//! Worker Spec is rendered from, the Worker's pi launch in the hidden work
//! session, and its session file.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;

use super::ledger::{self, Target};
use crate::config::{Catalog, Config, Tier};
use crate::error::{CpdError, ErrorCode};
use crate::git;
use crate::home::CpdHome;
use crate::host::layout::attempt_window;
use crate::host::pi::{self, PiDirs, WorkerLaunch, extensions_to_pass, worker_candidates};
use crate::host::{PaneId, Tmux, WindowId};
use crate::ids::{AttemptId, EscalationId, PlanId, SessionId};
use crate::metrics;
use crate::plan::jobs::Job;
use crate::session::run::{quiet_since, turn_finished};
use crate::spec::{self, Answer, Cause, Input, Previous};
use crate::store::Store;

/// How often a review Job's driver looks at the Lead while it waits for it to be idle (§6.7 step 1).
const LEAD_POLL: Duration = Duration::from_secs(2);

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
    let protected = git::protected_list(&target.worktree, &target.lead_branch)?;
    let (touches, acceptance, out_of_scope) = (
        strings(&job.touches, "touches")?,
        strings(&job.acceptance, "acceptance")?,
        strings(&job.out_of_scope, "out_of_scope")?,
    );
    let path = spec::write(
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
    )?;
    if job.review {
        let base: String = conn.query_row(
            "SELECT r.base FROM jobs j JOIN prs r ON r.plan = j.plan AND r.key = j.pr WHERE j.plan = ?1 AND j.key = ?2",
            params![target.plan.as_str(), job.key],
            |r| r.get(0),
        )?;
        let findings = crate::job::findings_path(home, &target.plan, &job.key);
        let base_text = std::fs::read_to_string(&path).map_err(|e| unreadable(&path, &e))?;
        let text = review_spec(&base_text, target, &job.title, &base, &findings, home)?;
        std::fs::write(&path, text).map_err(|e| unreadable(&path, &e))?;
    }
    Ok(path)
}

fn unreadable(path: &Path, e: &std::io::Error) -> CpdError {
    CpdError::new(ErrorCode::StoreFailed, format!("Cannot use {}: {e}.", path.display())).with("path", path.display().to_string())
}

/// A review Job's Spec (commands.md §16.1, B2): the Worker Spec `base_text` with
/// its *Who you are*, *Rules* and *Finish* sections replaced. The Job,
/// Previous Attempts and Answers sections are the Worker's own. The template
/// belongs to the Worker Spec (`crate::spec`), so the replacement is made here, in code.
fn review_spec(base_text: &str, target: &Target, title: &str, base: &str, findings: &Path, home: &CpdHome) -> Result<String, CpdError> {
    let broken = |what: &str| {
        CpdError::new(ErrorCode::StoreFailed, format!("Cannot render a review Job's Spec: the Worker Spec has no {what} section."))
            .with("attempt", target.id.to_string())
    };
    let heading = base_text.find('\n').ok_or_else(|| broken("title"))?;
    let rules = base_text.find("\n## Rules\n").ok_or_else(|| broken("Rules"))?;
    let job = base_text.find("\n## Job\n").ok_or_else(|| broken("Job"))?;
    let finish = base_text.find("\n## Finish\n").ok_or_else(|| broken("Finish"))?;
    if !(heading < rules && rules < job && job < finish) {
        return Err(broken("ordered"));
    }
    let (plan, attempt, number) = (&target.plan, &target.id, target.id.number());
    let (key, lead, worktree) = (target.job.as_str(), &target.lead_branch, target.worktree.display());
    let specs = home.plan_dir(plan).join("specs").display().to_string();
    let findings = findings.display();
    let who = format!(
        "{title_line}\n\n## Who you are\n\n\
         You are a CPD Worker on Attempt {number} of Job `{key}` ({title}) in Plan `{plan}`, a **review Job**: a fork of the Plan's Lead session, so you know the intention.\n\
         Your attempt id is `{attempt}`. Your Tier is `{tier}`.\n\
         Your worktree is `{worktree}`. Your current directory is that worktree.\n\n\
         ## Rules\n\n\
         - Review, do not build: **commit nothing** and do not edit the repo. The worktree must stay clean.\n\
         - Read the diff of the Lead Branch against the base: `git diff {base}...{lead}`.\n\
         - Read the Plan's Specs: `cpd plan show --plan {plan}`, and the rendered Specs in `{specs}`.\n\
         - Write your findings, in Markdown, to `{findings}` (create its directory if it is missing). The file must not be empty. The Lead reads it and turns the findings into fix Jobs.\n\
         - Never ask the User anything. You have no conversation with them.\n\
         - If continuing would require guessing, ask the Lead and wait for the answer. The command blocks until the Lead answers:\n\n\
         \x20     cpd ask {attempt} \"<your question>\"\n\n\
         \x20 Ask one real blocker at a time. Do not ask what this Spec or the code already answers.\n\
         - Do not stop until you have submitted.\n",
        title_line = &base_text[..heading],
        tier = target.tier,
    );
    let fin = format!(
        "\n## Finish\n\n\
         When `{findings}` holds your findings, run exactly:\n\n\
         \x20   cpd attempt submit {attempt}\n\n\
         If it succeeds, stop at once and make no more changes. If it refuses, the error code says what to do:\n\n\
         - `wrong_worktree`: you ran it outside your worktree. Run it from the worktree.\n\
         - `dirty_tree`: the worktree has changes. Undo them (`git checkout -- .`, delete stray files), then submit again.\n\
         - `review_commits`: you committed. Undo it (`git reset --hard` to the Attempt's base commit), then submit again.\n\
         - `review_missing`: the findings file does not exist or is empty. Write it, then submit again.\n\
         - `attempt_not_running`: the Attempt was stopped. Stop.\n\n\
         Do not stop until `cpd attempt submit` has succeeded.\n"
    );
    Ok(format!("{who}{}{fin}", &base_text[job..finish]))
}

/// The Lead Session a review Job's Worker forks, and its session file.
#[derive(Clone, Debug)]
pub struct Fork {
    pub session: SessionId,
    pub file: PathBuf,
}

/// A3 for a review Job (§3.4, B2): waits until the Plan's Lead is idle, by §6.7
/// step 1 (its session file exists, its state is `idle` or `stopped`, its last
/// message is a finished assistant turn, and the file has been quiet for
/// `fork_quiet_seconds`), checking every 2 seconds without a limit, and returns
/// what to fork. `None` when a cancel request was seen first (the safe point).
/// The Lead is never interrupted.
pub fn wait_for_lead(store: &mut Store, config: &Config, dirs: &PiDirs, target: &Target) -> Result<Option<Fork>, CpdError> {
    let quiet = Duration::from_secs(u64::from(config.timeouts.fork_quiet_seconds));
    loop {
        if store.read(|conn| ledger::cancel_request(conn, &target.plan, &target.job))?.is_some() {
            return Ok(None);
        }
        let (session, state, recorded) = store.read(|conn| lead_session(conn, &target.plan))?;
        let file = recorded.map(PathBuf::from).filter(|p| p.is_file()).or_else(|| pi::session_file(dirs, &session));
        if let Some(file) = file
            && matches!(state.as_str(), "idle" | "stopped")
            && quiet_since(&file, quiet)
            && std::fs::read_to_string(&file).is_ok_and(|text| turn_finished(&text))
        {
            return Ok(Some(Fork { session, file }));
        }
        std::thread::sleep(LEAD_POLL);
    }
}

/// The Plan's Lead Session: its id, state and recorded session file.
fn lead_session(conn: &Connection, plan: &PlanId) -> Result<(SessionId, String, Option<String>), CpdError> {
    let (id, state, file): (String, String, Option<String>) = conn.query_row(
        "SELECT s.id, s.state, s.file FROM plans p JOIN sessions s ON s.id = p.lead_session WHERE p.id = ?1",
        [plan.as_str()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let session = SessionId::parse(&id).map_err(|_| bad(format!("lead session id {id:?} of Plan {plan}")))?;
    Ok((session, state, file))
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
    fork: Option<&Fork>,
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
        fork: fork.map(|f| f.file.as_path()),
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
    kill_attempt_window(config, tmux, &target.plan, &target.id)
}

/// [`kill_window`] for Attempt `id` of `plan`: the one A9 kill, shared by the
/// driver, E2 and R.
pub fn kill_attempt_window(config: &Config, tmux: &Tmux, plan: &PlanId, id: &AttemptId) -> Result<(), CpdError> {
    let name = attempt_window(plan, id);
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
