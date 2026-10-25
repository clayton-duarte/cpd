//! The merge (contract §7.4): driver step A8 against temporary repos, with
//! no per-repo lock (hs-d243), and the recovery R of an interrupted merge.
//!
//! No Gate runs at the merge (R19): there is no `gate_failed` or `gate_error`
//! outcome (Escalation rs-merge-a1-q1).

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::TestEnv;
use cpd_core::config::{Catalog, Config, TierModel};
use cpd_core::error::CpdError;
use cpd_core::git::{Setup, create_attempt_worktree, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::merge::{self, Attempt, Ctx, End, Failure, Merge, take_free_driver_lock};
use cpd_core::store::{Store, WriteTxn};
use rusqlite::params;
use serde_json::{Value, json};

const PLAN: &str = "pay";

fn plan() -> PlanId {
    PlanId::parse(PLAN).unwrap()
}

fn catalog() -> Catalog {
    let model = |m: &str| TierModel { model: m.to_owned(), efforts: vec![0, 1, 2, 3, 4, 5] };
    Catalog { pi_version: "1.2.0".to_owned(), o: model("p/opus"), s: model("p/sonnet"), h: model("p/haiku"), g: TierModel { model: "p/gemini".to_owned(), efforts: vec![] } }
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

/// A PR of Plan `pay` as the tests drive it: its key, repo, Lead Branch and
/// Lead worktree, and the bare `origin` of its repo when it has one.
#[derive(Clone)]
struct Pr {
    key: String,
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
    let mut fx = plan_without_prs(config_extra);
    let origin = if with_origin { add_origin(&fx.env, &fx.repo) } else { fx.env.root().join("origins").join("pay.git") };
    let setup = Setup { timeout: Duration::from_secs(30), log: fx.env.root().join("lead.log") };
    let created = create_lead_worktree(&fx.repo, &plan(), "main", &setup).unwrap();
    assert!(created.push_failed.is_none());
    fx.lead = created.path;
    fx.store()
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                       VALUES ('pay', 'repo', '{repo}', 'repo', 'main', 'dispatch', 'cpd/pay', '{lead}', cpd_txn(), 't', 't');",
                    repo = fx.repo.display(),
                    lead = fx.lead.display()
                ),
            )
        })
        .unwrap();
    // The PR's policy is its repo's policy file, named as the PR.
    fx.env.repo_policy("acme", "repo", "");
    (fx, origin)
}

/// A bare `origin` for `repo`, which already holds `main`.
fn add_origin(env: &TestEnv, repo: &Path) -> PathBuf {
    let name = repo.file_name().unwrap().to_string_lossy().into_owned();
    let origin = env.root().join("origins").join(format!("{name}.git"));
    fs::create_dir_all(&origin).unwrap();
    env.git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    env.local_origin(repo, &origin);
    env.git(repo, &["push", "-q", "origin", "main"]);
    origin
}

/// The merge runs git in this process, whose environment is not the
/// TestEnv's: the repo's own config gives rebase an identity.
fn identify(env: &TestEnv, repo: &Path) {
    for (key, value) in [("user.name", "cpd test"), ("user.email", "test@cpd.invalid"), ("commit.gpgsign", "false")] {
        env.git(repo, &["config", key, value]);
    }
}

/// The repo `pay`, a database with Context, Lead Session and Plan `pay`
/// (phase `dispatch`) but no PR yet, and a config. `lead` is not set.
fn plan_without_prs(config_extra: &str) -> Fx {
    let env = TestEnv::new();
    let repo = env.git_repo("pay").canonicalize().unwrap();
    identify(&env, &repo);
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(
        home.config(),
        format!("version = 1\nmachine = \"t\"\n{config_extra}\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket()),
    )
    .unwrap();
    let config = Config::load(&home).unwrap();
    let fx = Fx { env, home, config, catalog: catalog(), repo, lead: PathBuf::new() };
    fx.store()
        .write("fixture", |tx| {
            exec(
                tx,
                "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');",
            )
        })
        .unwrap();
    fx
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

    /// The PR `repo` that [`fx`] makes: Lead Branch `cpd/pay` in `self.repo`.
    fn first_pr(&self) -> Pr {
        Pr { key: "repo".into(), repo: self.repo.clone(), lead: self.lead.clone() }
    }

    /// Job `key` (`running`, Tier S2) of the first PR with Attempt `<key>-a1` in `merging`:
    /// its worktree cut from the Lead Branch HEAD, with one commit writing
    /// `path` = `content`, submitted.
    fn submitted(&self, key: &str, path: &str, content: &str) -> (AttemptId, PathBuf) {
        self.submitted_on(&self.first_pr(), key, path, content)
    }

    /// [`Fx::submitted`] for a Job of `pr`.
    fn submitted_on(&self, pr: &Pr, key: &str, path: &str, content: &str) -> (AttemptId, PathBuf) {
        self.submitted_as(pr, key, "work", Some((path, content)))
    }

    /// [`Fx::submitted_on`] for a Job of `kind`, whose Attempt commits `change` (a path and its content) or, when there is none,
    /// nothing: an `open` or `update` Worker whose base did not move submits the Lead tip as it is (contract §7.7).
    fn submitted_as(&self, pr: &Pr, key: &str, kind: &str, change: Option<(&str, &str)>) -> (AttemptId, PathBuf) {
        self.submitted_needing(pr, key, kind, change, None)
    }

    /// [`Fx::submitted_as`] for a Job that needs the merged `test` Job `t`, whose `test_files` are `guarded` (a JSON array): `t` and the
    /// Dependency are written in the Job's own transaction (a Dependency of a Locked Job is refused later, `dependencies_locked`).
    fn submitted_needing(&self, pr: &Pr, key: &str, kind: &str, change: Option<(&str, &str)>, guarded: Option<&str>) -> (AttemptId, PathBuf) {
        let attempt = AttemptId::parse(&format!("{key}-a1")).unwrap();
        let setup = Setup { timeout: Duration::from_secs(30), log: self.env.root().join("attempt.log") };
        let made = create_attempt_worktree(&pr.repo, &pr.lead, &plan(), &attempt, &setup).unwrap();
        let sha = match change {
            Some((path, content)) => self.commit(&made.path, path, content),
            None => self.git(&made.path, &["rev-parse", "HEAD"]),
        };
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, kind, fingerprint, created_txn, created_at)
                     VALUES ('pay', ?4, ?1, 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', ?5, ?6, ?2, ?3, 't')",
                    params![key, "f".repeat(64), tx.id(), pr.key, i64::from(kind != "work"), kind],
                )?;
                if let Some(files) = guarded {
                    tx.execute(
                        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                                           fingerprint, created_txn, created_at, kind, tests, test_files, merged_sha)
                         VALUES ('pay', ?1, 't', 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'merged', 0, printf('%064d', 1),
                                 ?2, 't', 'test', '[\"t::one\"]', ?3, 'c0ffee')",
                        params![pr.key, tx.id(), files],
                    )?;
                    tx.execute("INSERT INTO dependencies (plan, job, needs) VALUES ('pay', ?1, 't')", params![key])?;
                }
                tx.execute(
                    "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha,
                                           submitted_sha, submitted_at, started_at, driver_pid)
                     VALUES ('pay', ?1, ?2, 1, 'S2', 'ladder', 'merging', ?3, ?4, ?5, ?6, 't', 't', 4242)",
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

    /// R as E2 runs it, for each Attempt in `merging` of a PR on `repo`, with no repo lock.
    fn recover_in(&self, repo: &Path) -> Vec<merge::Recovered> {
        let merging: Vec<(String, String)> = self
            .store()
            .read(|c| {
                let mut stmt = c.prepare(
                    "SELECT a.plan, a.id FROM attempts a JOIN jobs j ON j.plan = a.plan AND j.key = a.job
                       JOIN prs r ON r.plan = j.plan AND r.key = j.pr
                      WHERE a.status = 'merging' AND r.repo = ?1 ORDER BY a.plan, a.id",
                )?;
                let rows = stmt.query_map([repo.to_str().unwrap()], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap();
        merging
            .into_iter()
            .filter_map(|(p, a)| {
                let (p, a) = (PlanId::parse(&p).unwrap(), AttemptId::parse(&a).unwrap());
                merge::recover_attempt(&mut self.store(), &self.ctx("watch"), &p, &a).unwrap()
            })
            .collect()
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

    /// R as E2 runs it, for the Attempts of [`Fx::repo`].
    fn recover(&self) -> Vec<merge::Recovered> {
        self.recover_in(&self.repo)
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

}

// ------------------------------------------------------------ outcomes of A8

#[test]
fn a_commit_after_submit_ends_tampered_and_merges_nothing() {
    let fx = fx();
    let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
    let submitted = fx.git(&worktree, &["rev-parse", "HEAD"]);
    let late = fx.commit(&worktree, "late.txt", "late\n");
    let old = fx.lead_sha();

    let result = fx.merge(&attempt);

    assert!(matches!(result.end, End::Failed { cause: Failure::Tampered, .. }), "{result:?}");
    assert_eq!(fx.lead_sha(), old);
    let data = fx.assert_failed(&attempt, "api", "tampered");
    assert_eq!(data["detail"], format!("the Attempt branch moved after submit ({submitted} -> {late})"));
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

// ------------------------------------------------------------ the push (R35)

// ------------------------------------------------- an `open` or `update` Job's merge (contract §7.4; hs-d340)

/// Step 5 never rebases for these kinds: the Gate ran on the submitted HEAD, and a rebase would drop its merge commit and replay the base's
/// commits. The Lead tip must already be an ancestor of HEAD, else the Attempt ends `failed` / `merge_conflict` with the detail
/// `lead_branch_moved` (a ladder cause: the next Attempt starts from the newer tip). Nothing moves: no rebase, no fast-forward, no push.
#[test]
fn an_update_jobs_merge_never_rebases_and_ends_merge_conflict_lead_branch_moved() {
    let (fx, origin) = fx_origin();
    let (attempt, worktree) = fx.submitted_as(&fx.first_pr(), "up-repo", "update", None);
    let before = fx.git(&worktree, &["rev-parse", "HEAD"]);
    // Another Job merged since the Attempt began: an ordinary Job would rebase onto it, cleanly.
    let old = fx.advance_lead("other.txt", "other\n");

    let result = fx.merge(&attempt);

    let End::Failed { cause: Failure::MergeConflict, detail } = &result.end else { panic!("{result:?}") };
    assert!(detail.contains("lead_branch_moved"), "{detail}");
    assert_eq!(fx.git(&worktree, &["rev-parse", "HEAD"]), before, "never rebased");
    assert_eq!(fx.lead_sha(), old, "the Lead Branch did not move");
    assert_ne!(fx.git(&origin, &["rev-parse", "refs/heads/cpd/pay"]), old, "nothing was pushed");
    let (status, outcome, cause, merged, _) = fx.attempt_row(&attempt);
    assert_eq!((status.as_str(), outcome.as_deref(), cause.as_deref(), merged), ("ended", Some("failed"), Some("merge_conflict"), None));
    assert_eq!(fx.job_row("up-repo"), ("queued".into(), None, None), "a ladder cause: the next Attempt starts from the newer tip");
}

// ------------------------------------------------------------ R

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
fn r_leaves_an_attempt_whose_driver_is_alive() {
    let fx = fx();
    let (attempt, _) = fx.submitted("api", "api.txt", "api\n");
    let _driver = take_free_driver_lock(&fx.home, &plan(), &attempt).unwrap().expect("free");

    assert!(fx.recover().is_empty());
    assert_eq!(fx.attempt_row(&attempt).0, "merging");
    // While the lock is held, a fresh probe sees it held.
    assert!(take_free_driver_lock(&fx.home, &plan(), &attempt).unwrap().is_none());
}

// ------------------------------------------------------------ PRs across repos (R34)

