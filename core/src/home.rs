//! `$CPD_HOME`: where it is and what is in it (contract §1.4, with
//! commands.md §1.4 = amendment 4 of commands.md §19).
//!
//! Every path cpd 2 writes under `$CPD_HOME` is built here, so the layout has
//! one source of truth. None of these names collide with cpd 1's files
//! (`config.json`, `leads/`, `slots/`, `workflows/`, `locks/merge-*.lock`).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::error::{CpdError, ErrorCode};
use crate::ids::{AttemptId, ContextId, PlanId, SessionId};

/// The repo hooks whose full output is logged per Attempt (contract §7.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hook {
    PreStart,
    PreCommit,
    PreMerge,
}

impl Hook {
    /// The hook's name: its key in the policy `hooks` domain, also used in log file names.
    pub fn as_str(self) -> &'static str {
        match self {
            Hook::PreStart => "pre-start",
            Hook::PreCommit => "pre-commit",
            Hook::PreMerge => "pre-merge",
        }
    }
}

/// Why a driver ended through no fault of its Worker: an install or a store migration (hs-n278). A driver that
/// ends on one of these errors leaves them in `run/<plan>/attempts/<attempt>.infra`, which the engine reads
/// without the store: E2 ends that Attempt as one whose driver never started (hs-d160), at the same Tier and
/// not counted by `retry_cap`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Infra {
    /// The setup lock stayed held (a migration longer than the wait's ceiling).
    LockTimeout,
    /// The store is newer than this binary (a reload, or an install, raced a migration).
    StoreTooNew,
}

impl Infra {
    /// The infrastructure failure `code` is, if it is one.
    pub fn of(code: ErrorCode) -> Option<Infra> {
        match code {
            ErrorCode::LockTimeout => Some(Infra::LockTimeout),
            ErrorCode::StoreTooNew => Some(Infra::StoreTooNew),
            _ => None,
        }
    }

    /// The error code, as the marker holds it.
    pub fn code(self) -> &'static str {
        match self {
            Infra::LockTimeout => ErrorCode::LockTimeout.as_str(),
            Infra::StoreTooNew => ErrorCode::StoreTooNew.as_str(),
        }
    }
}

/// The resolved `$CPD_HOME` directory. Holding one proves it is absolute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CpdHome {
    root: PathBuf,
}

impl CpdHome {
    /// `$CPD_HOME`, else `~/.cpd`, from this process's environment.
    pub fn from_env() -> Result<CpdHome, CpdError> {
        CpdHome::resolve(std::env::var_os("CPD_HOME"), std::env::home_dir())
    }

    /// `cpd_home` (the value of `CPD_HOME`) when set, else `<home>/.cpd`.
    /// Both must be absolute; an empty `CPD_HOME` is refused, not ignored.
    /// Failure is `usage` (exit 2): the caller fixes its environment.
    pub fn resolve(cpd_home: Option<OsString>, home: Option<PathBuf>) -> Result<CpdHome, CpdError> {
        let root = match cpd_home {
            Some(value) => {
                let path = PathBuf::from(&value);
                if !path.is_absolute() {
                    return Err(CpdError::new(
                        ErrorCode::Usage,
                        format!("CPD_HOME must be an absolute path, got {value:?}."),
                    )
                    .with("variable", "CPD_HOME")
                    .with("value", value.to_string_lossy().into_owned()));
                }
                path
            }
            None => match home {
                Some(home) if home.is_absolute() => home.join(".cpd"),
                _ => {
                    return Err(CpdError::new(
                        ErrorCode::Usage,
                        "CPD_HOME is not set and the home directory is unknown: set CPD_HOME.",
                    )
                    .with("variable", "CPD_HOME"));
                }
            },
        };
        Ok(CpdHome { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `config.toml`: the machine config (contract §8).
    pub fn config(&self) -> PathBuf {
        self.root.join("config.toml")
    }

    /// `cpd.db`: the database (contract §4); `-wal` and `-shm` live beside it.
    pub fn db(&self) -> PathBuf {
        self.root.join("cpd.db")
    }

    /// `contexts/`: the git stores of earlier versions, one folder per Context.
    /// Only migration 0007 reads it (contract §4.5); nothing creates it.
    pub fn old_contexts_dir(&self) -> PathBuf {
        self.root.join("contexts")
    }

    /// `archive/contexts/`: where migration 0007 moves each old store (contract §1.4).
    pub fn archived_contexts_dir(&self) -> PathBuf {
        self.root.join("archive").join("contexts")
    }

    /// `bin/`: the directory first on every pi Session's `PATH` (contract §6.5, hs-d310).
    pub fn bin_dir(&self) -> PathBuf {
        self.root.join("bin")
    }

    /// `bin/git`: the `git` wrapper that refuses `git stash` (contract §1.4, §6.5).
    pub fn git_wrapper(&self) -> PathBuf {
        self.bin_dir().join("git")
    }

    /// `locks/`.
    pub fn locks_dir(&self) -> PathBuf {
        self.root.join("locks")
    }

    /// `locks/watch.lock`: held by the one `cpd watch`.
    pub fn watch_lock(&self) -> PathBuf {
        self.locks_dir().join("watch.lock")
    }

    /// `locks/tmux-restart.lock`: held by the one run of the tmux server's restart (contract §6.1).
    pub fn tmux_restart_lock(&self) -> PathBuf {
        self.locks_dir().join("tmux-restart.lock")
    }

    /// `locks/attempt-<plan>--<attempt>.lock`: the driver lock (contract §3.4).
    pub fn attempt_lock(&self, plan: &PlanId, attempt: &AttemptId) -> PathBuf {
        self.locks_dir()
            .join(format!("attempt-{plan}--{attempt}.lock"))
    }

    /// `locks/check-<plan>--<attempt>.lock`: held by `attempt submit` while its checks run, a span (contract §7.7).
    pub fn check_lock(&self, plan: &PlanId, attempt: &AttemptId) -> PathBuf {
        self.locks_dir()
            .join(format!("check-{plan}--{attempt}.lock"))
    }

    /// `locks/distill-<context>.lock`: held by the Context's one distill run (contract §4.6).
    pub fn distill_lock(&self, context: &ContextId) -> PathBuf {
        self.locks_dir().join(format!("distill-{context}.lock"))
    }

    /// `locks/pr-poll.lock`: held by `pr poll` for the whole poll (contract §7.6).
    pub fn pr_poll_lock(&self) -> PathBuf {
        self.locks_dir().join("pr-poll.lock")
    }

    /// `run/`.
    pub fn run_dir(&self) -> PathBuf {
        self.root.join("run")
    }

    /// `run/quit/<session>`: the marker the User's `/quit` leaves for `session run` (pro-d361).
    pub fn quit_marker(&self, session: &SessionId) -> PathBuf {
        self.run_dir().join("quit").join(session.as_str())
    }

    /// `run/secret/`: the grants of `cpd secret` (contract §8.4, §1.4).
    pub fn secret_dir(&self) -> PathBuf {
        self.run_dir().join("secret")
    }

    /// `run/secret/allow/`: one `<attempt>.json` per Attempt allowed to read secrets.
    pub fn secret_allow_dir(&self) -> PathBuf {
        self.secret_dir().join("allow")
    }

    /// `run/secret/approve/`: one `<uuid>.json` per approval the User gave.
    pub fn secret_approve_dir(&self) -> PathBuf {
        self.secret_dir().join("approve")
    }

    /// `run/watch.log`: the engine's stderr.
    pub fn watch_log(&self) -> PathBuf {
        self.run_dir().join("watch.log")
    }

    /// `run/tmux-restart.pending`: a restart of the tmux server is pending (contract §6.1).
    pub fn tmux_restart_pending(&self) -> PathBuf {
        self.run_dir().join("tmux-restart.pending")
    }

    /// `run/tmux-restart.log`: the output of the detached tmux restart.
    pub fn tmux_restart_log(&self) -> PathBuf {
        self.run_dir().join("tmux-restart.log")
    }

    /// `run/tmux-restart.pid`: the pid of the detached tmux restart the engine started, while it runs.
    pub fn tmux_restart_pid(&self) -> PathBuf {
        self.run_dir().join("tmux-restart.pid")
    }

    /// `run/context-digests/<context>/`: a Context's digest files (contract §1.4, §4.6).
    pub fn context_digest_dir(&self, context: &ContextId) -> PathBuf {
        self.run_dir()
            .join("context-digests")
            .join(context.as_str())
    }

    /// `run/context-digests/<context>/<upto_seq>.jsonl.invalid`: the last invalid output of the distiller (contract §4.6 step 5).
    pub fn context_digest_invalid(&self, context: &ContextId, upto_seq: u64) -> PathBuf {
        self.context_digest_dir(context)
            .join(format!("{upto_seq}.jsonl.invalid"))
    }

    /// `run/context-digests/<context>/distill.pid`: the pid of the distill run the engine started (§3.7 E9), while it runs.
    pub fn distill_pid(&self, context: &ContextId) -> PathBuf {
        self.context_digest_dir(context).join("distill.pid")
    }

    /// `run/<plan>/`.
    pub fn plan_dir(&self, plan: &PlanId) -> PathBuf {
        self.run_dir().join(plan.as_str())
    }

    /// `run/<plan>/plan.md`: the last `plan render`.
    pub fn plan_md(&self, plan: &PlanId) -> PathBuf {
        self.plan_dir(plan).join("plan.md")
    }

    /// `run/<context>/manager-message.md`: a Context's Manager's first message (commands.md §3.10).
    pub fn manager_message(&self, context: &ContextId) -> PathBuf {
        self.run_dir()
            .join(context.as_str())
            .join("manager-message.md")
    }

    /// `run/<plan>/lead-message.md`: the Plan Lead's first message (commands.md §1.4).
    pub fn lead_message(&self, plan: &PlanId) -> PathBuf {
        self.plan_dir(plan).join("lead-message.md")
    }

    /// `run/<plan>/messages/<session>.md`: a fork's first message (commands.md §1.4).
    pub fn session_message(&self, plan: &PlanId, session: &SessionId) -> PathBuf {
        self.plan_dir(plan)
            .join("messages")
            .join(format!("{session}.md"))
    }

    /// `run/<plan>/specs/<attempt>.md`: the rendered Worker Spec.
    pub fn worker_spec(&self, plan: &PlanId, attempt: &AttemptId) -> PathBuf {
        self.plan_dir(plan)
            .join("specs")
            .join(format!("{attempt}.md"))
    }

    /// `run/<plan>/reviews/<attempt>.md`: the review brief.
    pub fn review_brief(&self, plan: &PlanId, attempt: &AttemptId) -> PathBuf {
        self.review_file(plan, attempt, "md")
    }

    /// `run/<plan>/reviews/<attempt>.diff`.
    pub fn review_diff(&self, plan: &PlanId, attempt: &AttemptId) -> PathBuf {
        self.review_file(plan, attempt, "diff")
    }

    /// `run/<plan>/reviews/<attempt>.check.txt`: the quick check at submit.
    pub fn review_check(&self, plan: &PlanId, attempt: &AttemptId) -> PathBuf {
        self.review_file(plan, attempt, "check.txt")
    }

    /// `run/<plan>/reviews/<attempt>.gate.txt`: the Gate at merge.
    pub fn review_gate(&self, plan: &PlanId, attempt: &AttemptId) -> PathBuf {
        self.review_file(plan, attempt, "gate.txt")
    }

    fn review_file(&self, plan: &PlanId, attempt: &AttemptId, extension: &str) -> PathBuf {
        self.plan_dir(plan)
            .join("reviews")
            .join(format!("{attempt}.{extension}"))
    }

    /// `run/<plan>/logs/worker-<attempt>.log`: a `durable` Worker's stdout and stderr, appended (contract §3.4 "The Worker
    /// runner and its seam").
    pub fn worker_log(&self, plan: &PlanId, attempt: &AttemptId) -> PathBuf {
        self.plan_dir(plan)
            .join("logs")
            .join(format!("worker-{attempt}.log"))
    }

    /// `run/<plan>/attempts/<attempt>.report.json`: a `durable` Worker's end report, replaced after every settle (§6.12).
    pub fn worker_report(&self, plan: &PlanId, attempt: &AttemptId) -> PathBuf {
        self.plan_dir(plan)
            .join("attempts")
            .join(format!("{attempt}.report.json"))
    }

    /// `run/<plan>/durable/<attempt>.sqlite`: a `durable` Worker's transcript, appended by its host every turn
    /// (contract §6.12 `storePath`). The driver watches it for the stall rule (§3.4 A4, hs-d798).
    pub fn durable_store(&self, plan: &PlanId, attempt: &AttemptId) -> PathBuf {
        self.plan_dir(plan)
            .join("durable")
            .join(format!("{attempt}.sqlite"))
    }

    /// `run/<plan>/logs/attempt-<attempt>.log`: the Attempt driver's stderr.
    pub fn driver_log(&self, plan: &PlanId, attempt: &AttemptId) -> PathBuf {
        self.plan_dir(plan)
            .join("logs")
            .join(format!("attempt-{attempt}.log"))
    }

    /// `run/<plan>/attempts/<attempt>.infra`: the marker a driver leaves when an install or a store migration
    /// ended it (see [`Infra`]).
    pub fn attempt_infra(&self, plan: &PlanId, attempt: &AttemptId) -> PathBuf {
        self.plan_dir(plan)
            .join("attempts")
            .join(format!("{attempt}.infra"))
    }

    /// Leaves the marker of a driver that ended on `infra` (`message` says why), before it exits and releases
    /// its driver lock. Written whole, then renamed, so a reader never sees half of it.
    pub fn leave_infra(
        &self,
        plan: &PlanId,
        attempt: &AttemptId,
        infra: Infra,
        message: &str,
    ) -> Result<(), CpdError> {
        let path = self.attempt_infra(plan, attempt);
        let failed = |e: std::io::Error| {
            CpdError::new(
                ErrorCode::StoreFailed,
                format!(
                    "Cannot write the infrastructure marker {}: {e}.",
                    path.display()
                ),
            )
        };
        std::fs::create_dir_all(path.parent().expect("a file in attempts/")).map_err(failed)?;
        let tmp = path.with_extension("infra.tmp");
        let body = serde_json::json!({"code": infra.code(), "message": message});
        std::fs::write(&tmp, body.to_string()).map_err(failed)?;
        std::fs::rename(&tmp, &path).map_err(failed)
    }

    /// The marker of `attempt`'s driver: `None` when it left none. A marker that cannot be read is an error, not
    /// a missing one.
    pub fn infra(
        &self,
        plan: &PlanId,
        attempt: &AttemptId,
    ) -> Result<Option<(Infra, String)>, CpdError> {
        let path = self.attempt_infra(plan, attempt);
        let bad = |why: String| {
            CpdError::new(
                ErrorCode::StoreFailed,
                format!(
                    "The infrastructure marker {} is invalid: {why}.",
                    path.display()
                ),
            )
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(bad(e.to_string())),
        };
        let json: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| bad(e.to_string()))?;
        let code = json
            .get("code")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let infra = [Infra::LockTimeout, Infra::StoreTooNew]
            .into_iter()
            .find(|i| i.code() == code)
            .ok_or_else(|| bad(format!("code {code:?}")))?;
        let message = json
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        Ok(Some((infra, message.to_owned())))
    }

    /// Removes the marker of an Attempt that has ended; a missing one is fine.
    pub fn remove_infra(&self, plan: &PlanId, attempt: &AttemptId) -> Result<(), CpdError> {
        let path = self.attempt_infra(plan, attempt);
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(CpdError::new(
                ErrorCode::StoreFailed,
                format!(
                    "Cannot remove the infrastructure marker {}: {e}.",
                    path.display()
                ),
            )),
            _ => Ok(()),
        }
    }

    /// `run/<plan>/logs/<attempt>.<hook>.log`: the full output of a hook run.
    pub fn hook_log(&self, plan: &PlanId, attempt: &AttemptId, hook: Hook) -> PathBuf {
        self.plan_dir(plan)
            .join("logs")
            .join(format!("{attempt}.{}.log", hook.as_str()))
    }

    /// `run/<plan>/logs/<attempt>.tests-<what>.log`: the full output of one of the repo's `[tests]` commands
    /// (`what`: `build`, `list` or `run`, contract §7.7).
    pub fn tests_log(&self, plan: &PlanId, attempt: &AttemptId, what: &str) -> PathBuf {
        self.plan_dir(plan)
            .join("logs")
            .join(format!("{attempt}.tests-{what}.log"))
    }

    /// `run/<plan>/logs/lead.<hook>.log`: the full output of a hook run in the
    /// Lead worktree (`pre-start` at `plan lock`; Escalation `rs-plan-a1-q4`).
    pub fn lead_hook_log(&self, plan: &PlanId, hook: Hook) -> PathBuf {
        self.plan_dir(plan)
            .join("logs")
            .join(format!("lead.{}.log", hook.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> CpdHome {
        CpdHome::resolve(Some("/h/.cpd".into()), None).unwrap()
    }

    fn plan() -> PlanId {
        PlanId::parse("pay-4521").unwrap()
    }

    fn attempt() -> AttemptId {
        AttemptId::parse("api-schema-a2").unwrap()
    }

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn cpd_home_wins_over_home() {
        let h = CpdHome::resolve(Some("/tmp/x".into()), Some("/home/u".into())).unwrap();
        assert_eq!(h.root(), Path::new("/tmp/x"));
    }

    #[test]
    fn defaults_to_dot_cpd_in_home() {
        let h = CpdHome::resolve(None, Some("/home/u".into())).unwrap();
        assert_eq!(h.root(), Path::new("/home/u/.cpd"));
    }

    #[test]
    fn refuses_relative_or_empty_cpd_home() {
        for value in ["", "rel/dir", "./x"] {
            let err = CpdHome::resolve(Some(value.into()), Some("/home/u".into())).unwrap_err();
            assert_eq!(err.code(), ErrorCode::Usage, "{value:?}");
            assert_eq!(err.details()["variable"], "CPD_HOME");
        }
    }

    #[test]
    fn refuses_without_any_home() {
        for home in [None, Some(PathBuf::from("relative"))] {
            let err = CpdHome::resolve(None, home).unwrap_err();
            assert_eq!(err.code(), ErrorCode::Usage);
        }
    }

    #[test]
    fn layout_of_contract_1_4() {
        let h = home();
        assert_eq!(h.config(), p("/h/.cpd/config.toml"));
        assert_eq!(h.db(), p("/h/.cpd/cpd.db"));
        assert_eq!(h.old_contexts_dir(), p("/h/.cpd/contexts"));
        assert_eq!(h.archived_contexts_dir(), p("/h/.cpd/archive/contexts"));
        assert_eq!(h.bin_dir(), p("/h/.cpd/bin"));
        assert_eq!(h.git_wrapper(), p("/h/.cpd/bin/git"));
        assert_eq!(h.watch_lock(), p("/h/.cpd/locks/watch.lock"));
        assert_eq!(
            h.attempt_lock(&plan(), &attempt()),
            p("/h/.cpd/locks/attempt-pay-4521--api-schema-a2.lock")
        );
        assert_eq!(h.pr_poll_lock(), p("/h/.cpd/locks/pr-poll.lock"));
        assert_eq!(
            h.attempt_infra(&plan(), &attempt()),
            p("/h/.cpd/run/pay-4521/attempts/api-schema-a2.infra")
        );
        assert_eq!(h.watch_log(), p("/h/.cpd/run/watch.log"));
        assert_eq!(h.tmux_restart_lock(), p("/h/.cpd/locks/tmux-restart.lock"));
        assert_eq!(
            h.tmux_restart_pending(),
            p("/h/.cpd/run/tmux-restart.pending")
        );
        assert_eq!(h.tmux_restart_log(), p("/h/.cpd/run/tmux-restart.log"));
        assert_eq!(h.tmux_restart_pid(), p("/h/.cpd/run/tmux-restart.pid"));
        assert_eq!(h.plan_md(&plan()), p("/h/.cpd/run/pay-4521/plan.md"));
        assert_eq!(
            h.worker_spec(&plan(), &attempt()),
            p("/h/.cpd/run/pay-4521/specs/api-schema-a2.md")
        );
        assert_eq!(
            h.durable_store(&plan(), &attempt()),
            p("/h/.cpd/run/pay-4521/durable/api-schema-a2.sqlite")
        );
        assert_eq!(
            h.review_brief(&plan(), &attempt()),
            p("/h/.cpd/run/pay-4521/reviews/api-schema-a2.md")
        );
        assert_eq!(
            h.review_diff(&plan(), &attempt()),
            p("/h/.cpd/run/pay-4521/reviews/api-schema-a2.diff")
        );
        assert_eq!(
            h.review_check(&plan(), &attempt()),
            p("/h/.cpd/run/pay-4521/reviews/api-schema-a2.check.txt")
        );
        assert_eq!(
            h.review_gate(&plan(), &attempt()),
            p("/h/.cpd/run/pay-4521/reviews/api-schema-a2.gate.txt")
        );
        assert_eq!(
            h.driver_log(&plan(), &attempt()),
            p("/h/.cpd/run/pay-4521/logs/attempt-api-schema-a2.log")
        );
        for (hook, name) in [
            (Hook::PreStart, "pre-start"),
            (Hook::PreCommit, "pre-commit"),
            (Hook::PreMerge, "pre-merge"),
        ] {
            assert_eq!(
                h.hook_log(&plan(), &attempt(), hook),
                p(&format!(
                    "/h/.cpd/run/pay-4521/logs/api-schema-a2.{name}.log"
                ))
            );
            assert_eq!(
                h.lead_hook_log(&plan(), hook),
                p(&format!("/h/.cpd/run/pay-4521/logs/lead.{name}.log"))
            );
        }
    }

    #[test]
    fn files_added_by_amendment_4() {
        let h = home();
        let session = SessionId::parse("596d007f-6775-4134-b77a-b356cc523842").unwrap();
        assert_eq!(
            h.lead_message(&plan()),
            p("/h/.cpd/run/pay-4521/lead-message.md")
        );
        assert_eq!(
            h.session_message(&plan(), &session),
            p("/h/.cpd/run/pay-4521/messages/596d007f-6775-4134-b77a-b356cc523842.md")
        );
        assert_eq!(
            h.manager_message(&ContextId::parse("pay").unwrap()),
            p("/h/.cpd/run/pay/manager-message.md")
        );
    }

    #[test]
    fn the_context_digest_files_of_contract_4_6() {
        let h = home();
        let ctx = ContextId::parse("building-cpd").unwrap();
        assert_eq!(
            h.context_digest_dir(&ctx),
            p("/h/.cpd/run/context-digests/building-cpd")
        );
        assert_eq!(
            h.context_digest_invalid(&ctx, 1342),
            p("/h/.cpd/run/context-digests/building-cpd/1342.jsonl.invalid")
        );
        assert_eq!(
            h.distill_lock(&ctx),
            p("/h/.cpd/locks/distill-building-cpd.lock")
        );
        assert_eq!(
            h.distill_pid(&ctx),
            p("/h/.cpd/run/context-digests/building-cpd/distill.pid")
        );
    }

    #[test]
    fn no_name_collides_with_cpd_1() {
        let h = home();
        let ours = [
            h.config(),
            h.db(),
            h.old_contexts_dir(),
            h.archived_contexts_dir(),
            h.locks_dir(),
            h.run_dir(),
        ];
        for theirs in ["config.json", "leads", "slots", "workflows"] {
            assert!(!ours.contains(&h.root().join(theirs)), "{theirs}");
        }
    }

    #[test]
    fn only_a_lock_timeout_or_a_store_too_new_is_infrastructure() {
        assert_eq!(Infra::of(ErrorCode::LockTimeout), Some(Infra::LockTimeout));
        assert_eq!(Infra::of(ErrorCode::StoreTooNew), Some(Infra::StoreTooNew));
        for other in [
            ErrorCode::StoreFailed,
            ErrorCode::StoreBusy,
            ErrorCode::IllegalTransition,
        ] {
            assert_eq!(Infra::of(other), None, "{other:?}");
        }
    }

    #[test]
    fn the_infra_marker_round_trips_and_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let h = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        assert_eq!(h.infra(&plan(), &attempt()).unwrap(), None);
        h.leave_infra(&plan(), &attempt(), Infra::StoreTooNew, "newer")
            .unwrap();
        assert_eq!(
            h.infra(&plan(), &attempt()).unwrap(),
            Some((Infra::StoreTooNew, "newer".to_owned()))
        );
        h.remove_infra(&plan(), &attempt()).unwrap();
        h.remove_infra(&plan(), &attempt()).unwrap();
        assert_eq!(h.infra(&plan(), &attempt()).unwrap(), None);
        std::fs::write(
            h.attempt_infra(&plan(), &attempt()),
            r#"{"code":"crashed"}"#,
        )
        .unwrap();
        assert_eq!(
            h.infra(&plan(), &attempt()).unwrap_err().code(),
            ErrorCode::StoreFailed
        );
    }
}
