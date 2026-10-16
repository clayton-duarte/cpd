//! The merge (contract §7.4): driver step A8 against temporary repos, the
//! per-repo lock, and the recovery R of an interrupted merge.
//!
//! No Gate runs at the merge (R19): there is no `gate_failed` or `gate_error`
//! outcome (Escalation rs-merge-a1-q1).

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::TestEnv;
use cpd_core::config::{Catalog, Config, TierModel};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::git::{Setup, create_attempt_worktree, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::merge::{self, Attempt, Ctx, End, Failure, Merge, RepoLock, take_free_driver_lock};
use cpd_core::store::{Store, WriteTxn};
use rusqlite::params;
use serde_json::{Value, json};

const PLAN: &str = "pay";

fn plan() -> PlanId {
    PlanId::parse(PLAN).unwrap()
}

fn catalog() -> Catalog {
    let model = |m: &str| TierModel { model: m.to_owned(), efforts: vec![0, 1, 2, 3, 4, 5] };
    Catalog { pi_version: "1.2.0".to_owned(), o: model("p/opus"), s: model("p/sonnet"), h: model("p/haiku") }
}

fn metrics(_: &Attempt) -> Option<Value> {
    Some(json!({"turns": 7}))
}

/// A repo with a Lead worktree for Plan `pay`, a database with its Context,
/// Lead Session and Plan (phase `dispatch`), and a config.
struct Fx {
    env: TestEnv,
    home: CpdHome,
    config: Config,
    catalog: Catalog,
    repo: PathBuf,
    lead: PathBuf,
}

fn fx() -> Fx {
    fx_with("")
}

fn fx_with(config_extra: &str) -> Fx {
    build(config_extra, false).0
}

/// [`fx`] for a repo with a bare `origin` (and the path of that bare repo),
/// which already holds `main` and, from the lock, `cpd/pay` (R35).
fn fx_origin() -> (Fx, PathBuf) {
    build("", true)
}

fn build(config_extra: &str, with_origin: bool) -> (Fx, PathBuf) {
    let env = TestEnv::new();
    let repo = env.git_repo("pay").canonicalize().unwrap();
    let origin = env.root().join("origins").join("pay.git");
    if with_origin {
        fs::create_dir_all(&origin).unwrap();
        env.git(&origin, &["init", "-q", "--bare", "-b", "main"]);
        env.git(&repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
        env.git(&repo, &["push", "-q", "origin", "main"]);
    }
    // The merge runs git in this process, whose environment is not the
    // TestEnv's: the repo's own config gives rebase an identity.
    for (key, value) in [("user.name", "cpd test"), ("user.email", "test@cpd.invalid"), ("commit.gpgsign", "false")] {
        env.git(&repo, &["config", key, value]);
    }
    let setup = Setup { timeout: Duration::from_secs(30), log: env.root().join("lead.log") };
    let created = create_lead_worktree(&repo, &plan(), "main", &setup).unwrap();
    assert!(created.push_failed.is_none());
    let lead = created.path;
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(
        home.config(),
        format!("version = 1\nmachine = \"t\"\n{config_extra}\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket()),
    )
    .unwrap();
    let config = Config::load(&home).unwrap();
    let fx = Fx { env, home, config, catalog: catalog(), repo, lead };
    fx.store()
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 't');
                     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                       VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
                     INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session, created_at, locked_at)
                       VALUES ('pay', 'ctx', 'Pay', '{repo}', 'main', 'cpd/pay', '{lead}', 'dispatch', 'lead1', 't', 't');",
                    repo = fx.repo.display(),
                    lead = fx.lead.display()
                ),
            )
        })
        .unwrap();
    (fx, origin)
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    fn ctx<'a>(&'a self, source: &'a str) -> Ctx<'a> {
        Ctx { home: &self.home, config: &self.config, catalog: &self.catalog, source, metrics: &metrics }
    }

    fn git(&self, dir: &Path, args: &[&str]) -> String {
        self.env.git(dir, args)
    }

    fn lead_sha(&self) -> String {
        self.git(&self.repo, &["rev-parse", "refs/heads/cpd/pay"])
    }

    /// Commits `path` = `content` in `dir`'s checkout.
    fn commit(&self, dir: &Path, path: &str, content: &str) -> String {
        let full = dir.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, content).unwrap();
        self.git(dir, &["add", "-A"]);
        self.git(dir, &["commit", "-q", "-m", &format!("edit {path}")]);
        self.git(dir, &["rev-parse", "HEAD"])
    }

    /// Another Job's merge: a commit on the Lead Branch, through the Lead worktree.
    fn advance_lead(&self, path: &str, content: &str) -> String {
        self.commit(&self.lead, path, content)
    }

    /// Job `key` (`running`, Tier S2) with Attempt `<key>-a1` in `merging`:
    /// its worktree cut from the Lead Branch HEAD, with one commit writing
    /// `path` = `content`, submitted.
    fn submitted(&self, key: &str, path: &str, content: &str) -> (AttemptId, PathBuf) {
        let attempt = AttemptId::parse(&format!("{key}-a1")).unwrap();
        let setup = Setup { timeout: Duration::from_secs(30), log: self.env.root().join("attempt.log") };
        let made = create_attempt_worktree(&self.repo, &self.lead, &plan(), &attempt, &setup).unwrap();
        let sha = self.commit(&made.path, path, content);
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO jobs (plan, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, created_txn, created_at)
                     VALUES ('pay', ?1, 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, ?2, ?3, 't')",
                    params![key, "f".repeat(64), tx.id()],
                )?;
                tx.execute(
                    "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha,
                                           submitted_sha, submitted_at, started_at)
                     VALUES ('pay', ?1, ?2, 1, 'S2', 'ladder', 'merging', ?3, ?4, ?5, ?6, 't', 't')",
                    params![
                        attempt.to_string(),
                        key,
                        made.branch,
                        made.path.to_str().unwrap(),
                        made.base_sha,
                        sha
                    ],
                )?;
                Ok(())
            })
            .unwrap();
        (attempt, made.path)
    }

    fn request_cancel(&self, key: &str) {
        self.store()
            .write("job cancel", |tx| {
                tx.execute(
                    "UPDATE jobs SET cancel_reason = 'not needed', cancel_requested_at = 't' WHERE plan = 'pay' AND key = ?1",
                    [key],
                )?;
                Ok(())
            })
            .unwrap();
    }

    fn merge(&self, attempt: &AttemptId) -> Merge {
        merge::merge(&mut self.store(), &self.ctx("attempt run"), &plan(), attempt).unwrap()
    }

    /// R as E2 runs it: the per-repo lock without waiting, then R.
    fn recover(&self) -> Vec<merge::Recovered> {
        let lock = RepoLock::try_take(&self.home, &self.repo).unwrap().expect("the per-repo lock is free");
        merge::recover(&mut self.store(), &self.ctx("watch"), &lock).unwrap()
    }

    /// `(status, outcome, cause, merged_sha, metrics)` of an Attempt.
    fn attempt_row(&self, attempt: &AttemptId) -> (String, Option<String>, Option<String>, Option<String>, Option<String>) {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT status, outcome, cause, merged_sha, metrics FROM attempts WHERE plan = 'pay' AND id = ?1",
                    [attempt.to_string()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )?)
            })
            .unwrap()
    }

    /// `(status, merged_sha, failed_cause)` of a Job.
    fn job_row(&self, key: &str) -> (String, Option<String>, Option<String>) {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT status, merged_sha, failed_cause FROM jobs WHERE plan = 'pay' AND key = ?1",
                    [key],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?)
            })
            .unwrap()
    }

    /// `(kind, data)` of every Event about `attempt`, in order.
    fn events(&self, attempt: &AttemptId) -> Vec<(String, Value)> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare("SELECT kind, data FROM events WHERE plan = 'pay' AND attempt = ?1 ORDER BY id")?;
                let rows = stmt
                    .query_map([attempt.to_string()], |r| {
                        Ok((r.get::<_, String>(0)?, serde_json::from_str(&r.get::<_, String>(1)?).unwrap()))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    fn assert_failed(&self, attempt: &AttemptId, key: &str, cause: &str) -> Value {
        let (status, outcome, row_cause, merged, metrics) = self.attempt_row(attempt);
        assert_eq!((status.as_str(), outcome.as_deref(), row_cause.as_deref(), merged), ("ended", Some("failed"), Some(cause), None));
        assert_eq!(metrics.as_deref(), Some(r#"{"turns":7}"#));
        assert_eq!(self.job_row(key), ("failed".to_owned(), None, Some(cause.to_owned())));
        let events = self.events(attempt);
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(events[0].0, "job-failed");
        assert_eq!(events[0].1["cause"], cause);
        assert_eq!(events[0].1["tier"], "S2");
        assert_eq!(events[0].1["attempts"], 1);
        events[0].1.clone()
    }

    fn status(&self, dir: &Path) -> String {
        self.git(dir, &["status", "--porcelain"])
    }

    fn branch_exists(&self, branch: &str) -> bool {
        self.env
            .command("git")
            .args(["-C", self.repo.to_str().unwrap(), "rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")])
            .status()
            .unwrap()
            .success()
    }
}

/// What the driver leaves when it dies right after step 8: the Attempt
/// rebased onto the Lead Branch and the ref fast-forwarded to it, but the
/// Lead worktree not synced (step 9) and the Ledger not written (step 10).
fn die_after_step_8(fx: &Fx, worktree: &Path) -> String {
    let old = fx.lead_sha();
    fx.git(worktree, &["rebase", "-q", &old]);
    let tip = fx.git(worktree, &["rev-parse", "HEAD"]);
    fx.git(&fx.repo, &["update-ref", "refs/heads/cpd/pay", &tip, &old]);
    tip
}

// ------------------------------------------------------------ outcomes of A8

#[test]
fn merged_rebases_fast_forwards_syncs_the_lead_worktree_and_records_it() {
    let fx = fx();
    let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
    let other = fx.advance_lead("other.txt", "other\n");

    let result = fx.merge(&attempt);

    let new = fx.git(&worktree, &["rev-parse", "HEAD"]);
    assert_eq!(result.end, End::Merged { sha: new.clone() });
    assert!(result.recovered.is_empty());
    // Rebased onto the newer Lead Branch, then fast-forwarded.
    assert_eq!(fx.git(&worktree, &["rev-parse", "HEAD~1"]), other);
    assert_eq!(fx.lead_sha(), new);
    let reflog = fx.git(&fx.repo, &["reflog", "-1", "--format=%gs", "refs/heads/cpd/pay"]);
    assert_eq!(reflog, "cpd: merge pay/api-a1");
    // Step 9: the Lead worktree holds the new commit, clean.
    assert_eq!(fs::read_to_string(fx.lead.join("api.txt")).unwrap(), "api\n");
    assert_eq!(fx.status(&fx.lead), "");
    // Step 10.
    assert_eq!(
        fx.attempt_row(&attempt),
        ("ended".into(), Some("merged".into()), Some("merged".into()), Some(new.clone()), Some(r#"{"turns":7}"#.into()))
    );
    assert_eq!(fx.job_row("api"), ("merged".into(), Some(new.clone()), None));
    assert_eq!(fx.events(&attempt), [("job-merged".to_owned(), json!({"sha": new, "tier": "S2"}))]);
    let command: String = fx
        .store()
        .read(|c| Ok(c.query_row("SELECT t.command FROM events e JOIN txns t ON t.id = e.txn ORDER BY e.id DESC LIMIT 1", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(command, "attempt run");
}

#[test]
fn merge_conflict_aborts_the_rebase_and_goes_up_the_ladder() {
    let fx = fx();
    let (attempt, worktree) = fx.submitted("api", "README.md", "mine\n");
    let before = fx.git(&worktree, &["rev-parse", "HEAD"]);
    let old = fx.advance_lead("README.md", "theirs\n");

    let result = fx.merge(&attempt);

    let End::Failed { cause: Failure::MergeConflict, detail } = &result.end else { panic!("{result:?}") };
    assert!(detail.contains("README.md"), "{detail}");
    // The rebase was aborted; nothing moved.
    assert_eq!(fx.git(&worktree, &["rev-parse", "HEAD"]), before);
    assert_eq!(fx.status(&worktree), "");
    let rebasing = fx.git(&worktree, &["rev-parse", "--git-path", "rebase-merge"]);
    assert!(!worktree.join(rebasing).exists(), "a rebase is still in progress");
    assert_eq!(fx.lead_sha(), old);
    // A ladder cause: the Job is queued again at the next Tier.
    let (status, outcome, cause, merged, _) = fx.attempt_row(&attempt);
    assert_eq!((status.as_str(), outcome.as_deref(), cause.as_deref(), merged), ("ended", Some("failed"), Some("merge_conflict"), None));
    assert_eq!(fx.job_row("api"), ("queued".into(), None, None));
    let events = fx.events(&attempt);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "attempt-retry");
    assert_eq!(events[0].1["outcome"], "failed");
    assert_eq!(events[0].1["cause"], "merge_conflict");
    assert_eq!(events[0].1["next_tier"], "S3");
    assert!(events[0].1["feedback"].as_str().unwrap().contains("README.md"));
}

#[test]
fn merge_conflict_with_the_cap_reached_fails_the_job() {
    let fx = fx_with("retry_cap = 0");
    let (attempt, _) = fx.submitted("api", "README.md", "mine\n");
    fx.advance_lead("README.md", "theirs\n");

    let result = fx.merge(&attempt);

    assert!(matches!(result.end, End::Failed { cause: Failure::MergeConflict, .. }), "{result:?}");
    let data = fx.assert_failed(&attempt, "api", "merge_conflict");
    assert!(data["detail"].as_str().unwrap().contains("README.md"));
}

#[test]
fn dirty_target_is_checked_before_any_work_in_the_attempt_worktree() {
    let fx = fx();
    let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
    let before = fx.git(&worktree, &["rev-parse", "HEAD"]);
    fx.advance_lead("other.txt", "other\n");
    let old = fx.lead_sha();
    fs::write(fx.lead.join("stray.txt"), "untracked\n").unwrap();

    let result = fx.merge(&attempt);

    assert!(matches!(result.end, End::Failed { cause: Failure::DirtyTarget, .. }), "{result:?}");
    // Not rebased: the check came first.
    assert_eq!(fx.git(&worktree, &["rev-parse", "HEAD"]), before);
    assert_eq!(fx.lead_sha(), old);
    let data = fx.assert_failed(&attempt, "api", "dirty_target");
    assert!(data["detail"].as_str().unwrap().contains("stray.txt"));
}

#[test]
fn tampered_when_the_diff_touches_a_protected_path() {
    let fx = fx();
    let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
    fx.commit(&worktree, ".config/wt.toml", "pre-merge = \"true\"\n");
    let old = fx.lead_sha();

    let result = fx.merge(&attempt);

    assert!(matches!(result.end, End::Failed { cause: Failure::Tampered, .. }), "{result:?}");
    assert_eq!(fx.lead_sha(), old);
    let data = fx.assert_failed(&attempt, "api", "tampered");
    assert!(data["detail"].as_str().unwrap().contains(".config/wt.toml"));
}

#[test]
fn tampered_also_by_the_repos_own_protected_list_on_the_lead_branch() {
    let fx = fx();
    fx.advance_lead(".config/cpd.toml", "protected = [\"verify.sh\"]\n");
    let (attempt, _) = fx.submitted("api", "verify.sh", "exit 0\n");

    let result = fx.merge(&attempt);

    assert!(matches!(result.end, End::Failed { cause: Failure::Tampered, .. }), "{result:?}");
    fx.assert_failed(&attempt, "api", "tampered");
}

#[test]
fn cas_lost_when_the_lead_branch_keeps_moving_outside_cpd() {
    let fx = fx();
    let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
    fx.advance_lead("other.txt", "other\n");
    // Someone outside cpd moves the Lead Branch after every rebase, to a
    // commit with the same tree, so the Lead worktree stays clean.
    let hook = fx.repo.join(".git/hooks/post-rewrite");
    fs::write(
        &hook,
        "#!/bin/sh\nold=$(git rev-parse refs/heads/cpd/pay)\n\
         new=$(git commit-tree -p \"$old\" -m outsider \"$old^{tree}\")\n\
         git update-ref refs/heads/cpd/pay \"$new\" \"$old\"\n\
         echo x >> \"$(git rev-parse --git-common-dir)/outsider-moves\"\n",
    )
    .unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();

    let result = fx.merge(&attempt);

    assert!(matches!(result.end, End::Failed { cause: Failure::CasLost, .. }), "{result:?}");
    // Back to step 2 at most 3 times: 4 fast-forwards tried, each lost.
    let moves = fs::read_to_string(fx.repo.join(".git/outsider-moves")).unwrap();
    assert_eq!(moves.lines().count(), 1 + merge::CAS_RETRIES as usize);
    assert_ne!(fx.lead_sha(), fx.git(&worktree, &["rev-parse", "HEAD"]));
    fx.assert_failed(&attempt, "api", "cas_lost");
}

#[test]
fn a_cancel_request_ends_it_cancelled_before_the_merge() {
    let fx = fx();
    let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
    let before = fx.git(&worktree, &["rev-parse", "HEAD"]);
    let old = fx.lead_sha();
    fx.request_cancel("api");

    let result = fx.merge(&attempt);

    assert_eq!(result.end, End::Cancelled);
    assert_eq!(fx.lead_sha(), old);
    assert_eq!(fx.git(&worktree, &["rev-parse", "HEAD"]), before);
    let (status, outcome, cause, _, _) = fx.attempt_row(&attempt);
    assert_eq!((status.as_str(), outcome.as_deref(), cause.as_deref()), ("ended", Some("cancelled"), Some("cancel_requested")));
    assert_eq!(fx.job_row("api").0, "cancelled");
    assert_eq!(fx.events(&attempt), [("job-cancelled".to_owned(), json!({"reason": "not needed"}))]);
}

#[test]
fn step_9_failure_keeps_the_merge_and_reports_a_stale_lead_worktree() {
    let fx = fx();
    let (attempt, _) = fx.submitted("api", "api.txt", "api\n");
    // Clean for step 3, but step 9 cannot write the new file.
    let writable = fs::metadata(&fx.lead).unwrap().permissions();
    fs::set_permissions(&fx.lead, fs::Permissions::from_mode(0o555)).unwrap();

    let result = fx.merge(&attempt);
    fs::set_permissions(&fx.lead, writable).unwrap();

    let End::Merged { sha } = &result.end else { panic!("{result:?}") };
    assert_eq!(&fx.lead_sha(), sha);
    assert_eq!(fx.job_row("api").0, "merged");
    let events = fx.events(&attempt);
    assert_eq!(events.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["job-merged", "error"]);
    assert_eq!(events[1].1["code"], "lead_worktree_stale");
    assert_eq!(events[1].1["source"], "attempt run");
    assert!(events[1].1["details"]["fix"].as_str().unwrap().contains("read-tree -m -u"));
}

#[test]
fn merging_an_attempt_that_is_not_merging_is_refused() {
    let fx = fx();
    let (attempt, _) = fx.submitted("api", "api.txt", "api\n");
    fx.merge(&attempt);
    let err = merge::merge(&mut fx.store(), &fx.ctx("attempt run"), &plan(), &attempt).unwrap_err();
    assert_eq!(err.code(), ErrorCode::IllegalTransition);
}

// ------------------------------------------------------------ the push (R35)

#[test]
fn a_merge_pushes_the_lead_branch_to_origin() {
    let (fx, origin) = fx_origin();
    let (attempt, _) = fx.submitted("api", "api.txt", "api\n");

    let result = fx.merge(&attempt);

    let End::Merged { sha } = &result.end else { panic!("{result:?}") };
    assert_eq!(&fx.lead_sha(), sha);
    assert_eq!(fx.git(&origin, &["rev-parse", "refs/heads/cpd/pay"]), *sha);
    assert_eq!(fx.events(&attempt).iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["job-merged"]);
}

#[test]
fn a_rejected_push_leaves_the_job_merged_with_one_push_failed_error() {
    let (fx, origin) = fx_origin();
    let before = fx.git(&origin, &["rev-parse", "refs/heads/cpd/pay"]);
    let hook = origin.join("hooks").join("pre-receive");
    fs::write(&hook, "#!/bin/sh\necho nope >&2\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let (attempt, _) = fx.submitted("api", "api.txt", "api\n");

    let result = fx.merge(&attempt);

    let End::Merged { sha } = &result.end else { panic!("{result:?}") };
    assert_eq!(&fx.lead_sha(), sha);
    assert_eq!(fx.job_row("api"), ("merged".into(), Some(sha.clone()), None));
    assert_eq!(fx.git(&origin, &["rev-parse", "refs/heads/cpd/pay"]), before);
    let events = fx.events(&attempt);
    assert_eq!(events.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["job-merged", "error"]);
    assert_eq!(events[1].1["code"], "push_failed");
    assert_eq!(events[1].1["source"], "attempt run");
    assert!(events[1].1["details"]["stderr"].as_str().unwrap().contains("nope"));
}

#[test]
fn a_repo_without_origin_does_not_push_and_appends_no_error() {
    let fx = fx();
    let (attempt, _) = fx.submitted("api", "api.txt", "api\n");
    fx.merge(&attempt);
    assert_eq!(fx.events(&attempt).iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["job-merged"]);
}

#[test]
fn r_pushes_after_it_records_a_merge() {
    let (fx, origin) = fx_origin();
    let (_, worktree) = fx.submitted("api", "api.txt", "api\n");
    let tip = die_after_step_8(&fx, &worktree);
    assert_ne!(fx.git(&origin, &["rev-parse", "refs/heads/cpd/pay"]), tip);

    fx.recover();

    assert_eq!(fx.git(&origin, &["rev-parse", "refs/heads/cpd/pay"]), tip);
}

// ------------------------------------------------------------ the per-repo lock

#[test]
fn the_per_repo_lock_is_named_after_the_git_common_dir_and_excludes_others() {
    let fx = fx();
    let common = fx.repo.join(".git").canonicalize().unwrap();
    let path = RepoLock::path(&fx.home, &fx.repo).unwrap();
    assert_eq!(path, fx.home.repo_lock(&common));
    // The same lock from a linked worktree of the repo.
    assert_eq!(RepoLock::path(&fx.home, &fx.lead).unwrap(), path);

    let held = RepoLock::wait(&fx.home, &fx.repo).unwrap();
    assert_eq!(held.file(), path);
    assert!(RepoLock::try_take(&fx.home, &fx.repo).unwrap().is_none());
    let err = RepoLock::wait_for(&fx.home, &fx.repo, Duration::from_millis(300)).unwrap_err();
    assert_eq!(err.code(), ErrorCode::LockTimeout);
    drop(held);
    assert!(RepoLock::try_take(&fx.home, &fx.repo).unwrap().is_some());
}

#[test]
fn the_merge_waits_for_the_per_repo_lock_and_holds_it_until_step_10() {
    let fx = fx();
    let (attempt, _) = fx.submitted("api", "api.txt", "api\n");
    let held = RepoLock::wait(&fx.home, &fx.repo).unwrap();

    std::thread::scope(|s| {
        let merging = s.spawn(|| fx.merge(&attempt));
        std::thread::sleep(Duration::from_millis(500));
        // Still waiting: nothing happened.
        assert_eq!(fx.attempt_row(&attempt).0, "merging");
        assert!(!merging.is_finished());
        drop(held);
        let result = merging.join().unwrap();
        assert!(matches!(result.end, End::Merged { .. }), "{result:?}");
    });
    // Released once step 10 committed.
    assert!(RepoLock::try_take(&fx.home, &fx.repo).unwrap().is_some());
}

// ------------------------------------------------------------ R

#[test]
fn r_after_step_8_ends_merged_at_the_tip_and_finishes_the_stale_lead_worktree() {
    let fx = fx();
    let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
    fx.advance_lead("other.txt", "other\n");
    let tip = die_after_step_8(&fx, &worktree);
    // The Lead worktree is stale: index and files are the old tree, HEAD is new.
    assert!(!fx.lead.join("api.txt").exists());
    assert_ne!(fx.status(&fx.lead), "");
    fx.request_cancel("api");

    let recovered = fx.recover();

    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].attempt, attempt);
    assert_eq!(recovered[0].end, Some(End::Merged { sha: tip.clone() }));
    // Step 9 finished.
    assert_eq!(fx.status(&fx.lead), "");
    assert_eq!(fs::read_to_string(fx.lead.join("api.txt")).unwrap(), "api\n");
    assert_eq!(fx.lead_sha(), tip);
    // Step 10, with the cancel request reported as too late.
    assert_eq!(
        fx.attempt_row(&attempt),
        ("ended".into(), Some("merged".into()), Some("merged".into()), Some(tip.clone()), Some(r#"{"turns":7}"#.into()))
    );
    assert_eq!(fx.job_row("api"), ("merged".into(), Some(tip.clone()), None));
    assert_eq!(
        fx.events(&attempt),
        [("job-merged".to_owned(), json!({"sha": tip, "tier": "S2", "cancel_too_late": true}))]
    );
    // The rest of A9: the worktree and branch are gone.
    assert!(!worktree.exists());
    assert!(!fx.branch_exists("cpd/pay--api-a1"));
    // Idempotent: nothing left to recover.
    assert!(fx.recover().is_empty());
}

#[test]
fn r_before_step_8_ends_crashed_through_the_ladder() {
    let fx = fx();
    let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
    let old = fx.lead_sha();

    let recovered = fx.recover();

    assert_eq!(recovered.len(), 1);
    assert!(matches!(recovered[0].end, Some(End::Failed { cause: Failure::Crashed, .. })), "{recovered:?}");
    assert_eq!(fx.lead_sha(), old);
    let (status, outcome, cause, merged, _) = fx.attempt_row(&attempt);
    assert_eq!((status.as_str(), outcome.as_deref(), cause.as_deref(), merged), ("ended", Some("failed"), Some("crashed"), None));
    assert_eq!(fx.job_row("api"), ("queued".into(), None, None));
    let events = fx.events(&attempt);
    assert_eq!(events.len(), 1);
    assert_eq!((events[0].0.as_str(), &events[0].1["cause"], &events[0].1["next_tier"]), ("attempt-retry", &json!("crashed"), &json!("S3")));
    // Kept for debugging.
    assert!(worktree.exists());
}

#[test]
fn r_before_step_8_with_a_cancel_request_ends_cancelled() {
    let fx = fx();
    let (attempt, _) = fx.submitted("api", "api.txt", "api\n");
    fx.request_cancel("api");

    let recovered = fx.recover();

    assert_eq!(recovered[0].end, Some(End::Cancelled));
    assert_eq!(fx.job_row("api").0, "cancelled");
    assert_eq!(fx.events(&attempt)[0].0, "job-cancelled");
}

#[test]
fn r_on_a_git_failure_changes_nothing_and_reports_merge_check_failed_once() {
    let fx = fx();
    let (attempt, _) = fx.submitted("api", "api.txt", "api\n");
    // The Attempt branch is gone: R1's rev-parse fails, which proves nothing.
    fx.git(&fx.repo, &["update-ref", "-d", "refs/heads/cpd/pay--api-a1"]);

    for _ in 0..2 {
        let recovered = fx.recover();
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0].end, None);
    }

    assert_eq!(fx.attempt_row(&attempt).0, "merging");
    assert_eq!(fx.job_row("api").0, "running");
    let events = fx.events(&attempt);
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].0, "error");
    assert_eq!(events[0].1["code"], "merge_check_failed");
    assert_eq!(events[0].1["source"], "watch");
}

#[test]
fn r_leaves_an_attempt_whose_driver_is_alive() {
    let fx = fx();
    let (attempt, _) = fx.submitted("api", "api.txt", "api\n");
    let _driver = take_free_driver_lock(&fx.home, &plan(), &attempt).unwrap().expect("free");

    assert!(fx.recover().is_empty());
    assert_eq!(fx.attempt_row(&attempt).0, "merging");
    // While the lock is held, a fresh probe sees it held.
    assert!(take_free_driver_lock(&fx.home, &plan(), &attempt).unwrap().is_none());
}

#[test]
fn the_merge_runs_r_first_so_a_dead_drivers_stale_lead_worktree_is_no_dirty_target() {
    let fx = fx();
    let (dead, dead_worktree) = fx.submitted("api", "api.txt", "api\n");
    let (live, live_worktree) = fx.submitted("ui", "ui.txt", "ui\n");
    let dead_tip = die_after_step_8(&fx, &dead_worktree);

    let result = fx.merge(&live);

    assert_eq!(result.recovered.len(), 1);
    assert_eq!(result.recovered[0].attempt, dead);
    assert_eq!(result.recovered[0].end, Some(End::Merged { sha: dead_tip.clone() }));
    let new = fx.git(&live_worktree, &["rev-parse", "HEAD"]);
    assert_eq!(result.end, End::Merged { sha: new.clone() });
    assert_eq!(fx.git(&live_worktree, &["rev-parse", "HEAD~1"]), dead_tip);
    assert_eq!(fx.lead_sha(), new);
    assert_eq!(fx.status(&fx.lead), "");
    assert!(fx.lead.join("api.txt").exists() && fx.lead.join("ui.txt").exists());
    assert_eq!(fx.job_row("api").0, "merged");
    assert_eq!(fx.job_row("ui").0, "merged");
}
