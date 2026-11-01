//! The merge (contract §7.4): driver step A8 against repos of the fake git route, with
//! no per-repo lock (hs-d243), and the recovery R of an interrupted merge. All in-process: `cpd_core::merge::merge`
//! and `recover_attempt` run on the fake ports of [`Kit`] (the git route of the `FakeRunner`, whose `Universe` holds
//! the repos and the bare `origin`s), against a Plan written straight into the database.
//!
//! No Gate runs at the merge (R19): there is no `gate_failed` or `gate_error`
//! outcome (Escalation rs-merge-a1-q1).
//!
//! What real git did by itself and the route does not (a `pre-receive` hook that declines, a `post-rewrite` hook, a
//! directory that cannot be written) is a scripted reply of the `FakeRunner`: the test says what git answers.

use crate::common;

use std::collections::BTreeMap;
use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::fx::Fx as Kit;
use cpd_core::config::{Catalog, Config, TierModel};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::fakes::git::{Oid, parse};
use cpd_core::git::worktree::prepare_stacked_lead_worktree;
use cpd_core::git::{Setup, create_attempt_worktree, create_lead_worktree, pr_lead_worktree_path, prepare_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Reply};
use cpd_core::ids::{AttemptId, PlanId, PrKey};
use cpd_core::merge::{self, Attempt, Ctx, End, Failure, Merge, take_free_driver_lock};
use cpd_core::ports::Ports;
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
/// Lead Session and Plan (phase `dispatch`), and a config. It derefs to the [`Kit`] (`env`, `git`, `scoped`, `fakes`, ...).
struct Fx {
    kit: Kit,
    home: CpdHome,
    config: Config,
    catalog: Catalog,
    repo: PathBuf,
    lead: PathBuf,
}

impl Deref for Fx {
    type Target = Kit;

    fn deref(&self) -> &Kit {
        &self.kit
    }
}

/// A PR of Plan `pay` as the tests drive it: its key, repo, Lead Branch and
/// Lead worktree, and the bare `origin` of its repo when it has one.
#[derive(Clone)]
struct Pr {
    key: String,
    repo: PathBuf,
    branch: String,
    lead: PathBuf,
    origin: Option<PathBuf>,
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
    let origin = if with_origin { add_origin(&fx, &fx.repo) } else { fx.env.root().join("origins").join("pay.git") };
    let setup = Setup { timeout: Duration::from_secs(30), log: fx.env.root().join("lead.log") };
    let created = fx.scoped(|| create_lead_worktree(&fx.repo, &plan(), "main", &setup)).unwrap();
    assert!(created.push_failed.is_none());
    fx.lead = created.path;
    fx.store()
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, description, created_txn, created_at, started_at)
                       VALUES ('pay', 'repo', '{repo}', 'repo', 'main', 'dispatch', 'cpd/pay', '{lead}', 'Charge the card', 'A description', cpd_txn(), 't', 't');",
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
fn add_origin(fx: &Fx, repo: &Path) -> PathBuf {
    let name = repo.file_name().unwrap().to_string_lossy().into_owned();
    let origin = fx.env.root().join("origins").join(format!("{name}.git"));
    fx.git(fx.env.root(), &["init", "-q", "--bare", "-b", "main", origin.to_str().unwrap()]);
    fx.github_origin(repo);
    fx.local_origin(repo, &origin);
    fx.git(repo, &["push", "-q", "origin", "main"]);
    origin
}

/// The repo `pay`, a database with Context, Lead Session and Plan `pay`
/// (phase `dispatch`) but no PR yet, and a config. `lead` is not set.
fn plan_without_prs(config_extra: &str) -> Fx {
    let kit = Kit::with_ports();
    let repo = kit.git_repo("pay").canonicalize().unwrap();
    let home = CpdHome::resolve(Some(kit.env.cpd_home().into()), None).unwrap();
    fs::write(
        home.config(),
        format!("version = 1\nmachine = \"t\"\n{config_extra}\n[tmux]\nsocket = \"{}\"\n", kit.env.tmux_socket()),
    )
    .unwrap();
    let config = Config::load(&home).unwrap();
    let fx = Fx { kit, home, config, catalog: catalog(), repo, lead: PathBuf::new() };
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

/// Whether `call` is a `git -C <dir> ...` of `dir`.
fn in_dir(call: &Call, dir: &Path) -> bool {
    call.args.windows(2).any(|w| w[0] == "-C" && Path::new(&w[1]) == dir)
}

/// Whether `call` is `git <.. sub ..>`.
fn is(call: &Call, sub: &str) -> bool {
    call.args.iter().any(|a| a == sub)
}

/// What `git push --porcelain` prints and exits with when the remote's `pre-receive` hook declines (and prints `nope`).
fn declined_push() -> Reply {
    Reply::exit(1, "To origin\n!\trefs/heads/x:refs/heads/cpd/pay\t[remote rejected] (pre-receive hook declined)\nDone\n", "remote: nope\n")
}

/// What GitHub's ref-lock race prints (contract §7.4 rule P): a `!` line with no `(non-fast-forward)` reason.
fn lock_race_push() -> Reply {
    Reply::exit(
        1,
        "To origin\n!\trefs/heads/x:refs/heads/cpd/pay\t[remote rejected] (cannot lock ref 'refs/heads/cpd/pay': is at 1111111111111111111111111111111111111111 but expected 2222222222222222222222222222222222222222)\nDone\n",
        "error: failed to push some refs to 'origin'\n",
    )
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    fn ctx<'a>(&'a self, source: &'a str) -> Ctx<'a> {
        Ctx { home: &self.home, config: &self.config, catalog: &self.catalog, source, metrics: &metrics }
    }

    /// `git <args>` answers `reply` wherever `matches` says so: a rule of the `FakeRunner`, which wins over the git route.
    fn script(&self, matches: impl Fn(&Call) -> bool + Send + Sync + 'static, reply: Reply) {
        let _ = (*self.fakes.runner).clone().on("git", matches, reply);
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

    /// Records the Attempt worktree's HEAD as the `submitted_sha`: for a test that
    /// builds its Attempt branch after the `submitted` fixture, as a submit would have.
    fn resubmit(&self, attempt: &AttemptId, worktree: &Path) {
        let tip = self.git(worktree, &["rev-parse", "HEAD"]);
        self.store()
            .write("fixture", |tx| {
                tx.execute("UPDATE attempts SET submitted_sha = ?2 WHERE plan = 'pay' AND id = ?1", params![attempt.to_string(), tip])?;
                Ok(())
            })
            .unwrap();
    }

    /// Another Job's merge: a commit on the Lead Branch, through the Lead worktree.
    fn advance_lead(&self, path: &str, content: &str) -> String {
        self.commit(&self.lead, path, content)
    }

    /// The PR `repo` that [`fx`] makes: Lead Branch `cpd/pay` in `self.repo`.
    fn first_pr(&self) -> Pr {
        Pr { key: "repo".into(), repo: self.repo.clone(), branch: "cpd/pay".into(), lead: self.lead.clone(), origin: None }
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
        let made = self.scoped(|| create_attempt_worktree(&pr.repo, &pr.lead, &plan(), &attempt, &setup)).unwrap();
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

    /// Adds PR `key` of Plan `pay` on `repo`, started as `pr start` does: Lead Branch
    /// `cpd/pay/<key>` and its Lead worktree, pushed when the repo has an `origin`.
    fn add_pr(&self, key: &str, repo: &Path, origin: Option<PathBuf>) -> Pr {
        let branch = format!("cpd/pay/{key}");
        let path = self.scoped(|| pr_lead_worktree_path(repo, &plan(), &PrKey::parse(key).unwrap(), &branch)).unwrap();
        let setup = Setup { timeout: Duration::from_secs(30), log: self.env.root().join("lead.log") };
        let prepared = self.scoped(|| prepare_lead_worktree(repo, &plan(), &path, &branch, "main", &setup)).map_err(|f| f.error).unwrap();
        self.scoped(|| prepared.push()).unwrap();
        // The PR's policy is its repo's policy file, named as the PR.
        self.env.repo_policy("acme", key, "");
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, description, created_txn, created_at, started_at)
                     VALUES ('pay', ?1, ?2, ?1, 'main', 'dispatch', ?3, ?4, 'A title', 'A description', ?5, 't', 't')",
                    params![key, repo.to_str().unwrap(), branch, path.to_str().unwrap(), tx.id()],
                )?;
                Ok(())
            })
            .unwrap();
        Pr { key: key.into(), repo: repo.to_owned(), branch, lead: path, origin }
    }

    /// Adds PR `key` of Plan `pay`, **stacked** on `base_pr` and started as `pr start` does: its Lead Branch
    /// `cpd/pay/<key>` is cut from the head of `base_pr`'s Lead Branch, which is its `base` (contract §7.2).
    fn add_stacked_pr(&self, key: &str, base_pr: &Pr) -> Pr {
        let repo = &base_pr.repo;
        let branch = format!("cpd/pay/{key}");
        let path = self.scoped(|| pr_lead_worktree_path(repo, &plan(), &PrKey::parse(key).unwrap(), &branch)).unwrap();
        let setup = Setup { timeout: Duration::from_secs(30), log: self.env.root().join("lead.log") };
        let prepared = self.scoped(|| prepare_stacked_lead_worktree(repo, &plan(), &path, &branch, &base_pr.branch, &setup)).map_err(|f| f.error).unwrap();
        self.scoped(|| prepared.push()).unwrap();
        // The PR's policy is its repo's policy file, named as the PR.
        self.env.repo_policy("acme", key, "");
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, description, created_txn, created_at, started_at)
                     VALUES ('pay', ?1, ?2, ?1, ?3, 'dispatch', ?4, ?5, 'A title', 'A description', ?6, 't', 't')",
                    params![key, repo.to_str().unwrap(), base_pr.branch, branch, path.to_str().unwrap(), tx.id()],
                )?;
                Ok(())
            })
            .unwrap();
        Pr { key: key.into(), repo: repo.clone(), branch, lead: path, origin: base_pr.origin.clone() }
    }

    /// The tip of `pr`'s Lead Branch, read in its repo.
    fn tip(&self, pr: &Pr) -> String {
        self.git(&pr.repo, &["rev-parse", &format!("refs/heads/{}", pr.branch)])
    }

    /// Another Job's merge into `pr`: a commit through its Lead worktree.
    fn advance(&self, pr: &Pr, path: &str, content: &str) -> String {
        self.commit(&pr.lead, path, content)
    }

    /// The tip of `branch` on `pr`'s `origin`.
    fn remote_tip(&self, pr: &Pr) -> String {
        self.git(pr.origin.as_ref().expect("the PR's repo has an origin"), &["rev-parse", &format!("refs/heads/{}", pr.branch)])
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
                self.scoped(|| merge::recover_attempt(&mut self.store(), &self.ctx("watch"), &p, &a)).unwrap()
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
        self.try_merge(attempt).unwrap()
    }

    /// [`Fx::merge`] that does not unwrap: the merge of an Attempt that is not `merging` is refused.
    fn try_merge(&self, attempt: &AttemptId) -> Result<Merge, CpdError> {
        self.scoped(|| merge::merge(&mut self.store(), &self.ctx("attempt run"), &plan(), attempt))
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

    fn status(&self, dir: &Path) -> String {
        self.git(dir, &["status", "--porcelain"])
    }

    /// The message of the newest reflog entry of `branch` in `repo` (`git reflog -1 --format=%gs`): the route has no `reflog`.
    fn reflog_message(&self, repo: &Path, branch: &str) -> String {
        let u = self.universe.lock().unwrap();
        let id = u.repo_of(repo).expect("a repo of the Universe");
        u.reflog(id, &format!("refs/heads/{branch}")).first().expect("the branch has a reflog").message.clone()
    }

    /// The parents of commit `sha` (`git rev-list --parents -n 1`, `<sha>^2`): the route has no `^N`.
    fn parents(&self, sha: &str) -> Vec<String> {
        let u = self.universe.lock().unwrap();
        u.commit_data(&Oid::new(sha)).expect("a commit of the Universe").parents.iter().map(|p| p.as_str().to_owned()).collect()
    }

    /// The paths where the index of worktree `wt` differs from the tree of `rev` (`git diff --cached --name-only <rev>`).
    fn staged_against(&self, wt: &Path, rev: &str) -> Vec<String> {
        let u = self.universe.lock().unwrap();
        let tree = parse(&u, wt, &format!("{rev}^{{tree}}")).unwrap();
        let committed: BTreeMap<String, Oid> = u.tree_entries(&tree).unwrap().into_iter().collect();
        let staged: BTreeMap<String, Oid> = u.index(wt).into_iter().filter(|(_, _, stage)| *stage == 0).map(|(p, o, _)| (p, o)).collect();
        committed.keys().chain(staged.keys()).filter(|p| committed.get(*p) != staged.get(*p)).cloned().collect::<std::collections::BTreeSet<_>>().into_iter().collect()
    }

    /// What `git merge --no-commit` and `git commit` make of a resolved conflict (the route keeps no `MERGE_HEAD`): the worktree's
    /// index (what the test staged) as a commit of two parents, HEAD and `other`, on the worktree's branch. Returns it.
    fn merge_commit(&self, worktree: &Path, other: &str, message: &str) -> String {
        let (head, branch, tree) = (
            self.git(worktree, &["rev-parse", "HEAD"]),
            self.git(worktree, &["symbolic-ref", "--short", "HEAD"]),
            self.git(worktree, &["write-tree"]),
        );
        let commit = {
            let mut u = self.universe.lock().unwrap();
            let id = u.repo_of(worktree).expect("a repo of the Universe");
            u.commit(id, &Oid::new(&tree), &[Oid::new(&head), Oid::new(other)], message).as_str().to_owned()
        };
        self.git(worktree, &["update-ref", &format!("refs/heads/{branch}"), &commit, &head]);
        commit
    }

    fn branch_exists(&self, branch: &str) -> bool {
        self.git_output(&self.repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).status.success()
    }
}

/// What the driver leaves when it dies right after step 8: the Attempt
/// rebased onto the Lead Branch and the ref fast-forwarded to it, but the
/// Lead worktree not synced (step 9) and the Ledger not written (step 10).
fn die_after_step_8(fx: &Fx, worktree: &Path) -> String {
    let old = fx.lead_sha();
    fx.git(worktree, &["rebase", &old]);
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
    // Rebased onto the newer Lead Branch, then fast-forwarded.
    assert_eq!(fx.git(&worktree, &["rev-parse", "HEAD~1"]), other);
    assert_eq!(fx.lead_sha(), new);
    assert_eq!(fx.reflog_message(&fx.repo, "cpd/pay"), "cpd: merge pay/api-a1");
    // Step 9: the Lead worktree holds the new commit, clean.
    assert_eq!(fs::read_to_string(fx.lead.join("api.txt")).unwrap(), "api\n");
    assert_eq!(fx.status(&fx.lead), "");
    // Step 10.
    assert_eq!(
        fx.attempt_row(&attempt),
        ("ended".into(), Some("merged".into()), Some("merged".into()), Some(new.clone()), Some(r#"{"turns":7}"#.into()))
    );
    assert_eq!(fx.job_row("api"), ("merged".into(), Some(new.clone()), None));
    assert_eq!(fx.events(&attempt), [("job-merged".to_owned(), json!({"sha": new, "tier": "S2", "kind": "work", "turn": false}))]);
    let command: String = fx
        .store()
        .read(|c| Ok(c.query_row("SELECT t.command FROM events e JOIN txns t ON t.id = e.txn ORDER BY e.id DESC LIMIT 1", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(command, "attempt run");
}

#[test]
fn an_attempt_that_already_contains_the_lead_tip_lands_as_is_with_its_merge_commit() {
    let fx = fx();
    let (attempt, worktree) = fx.submitted("api", "README.md", "mine\n");
    // The Lead Branch changes the same file on the other side.
    let old = fx.advance_lead("README.md", "theirs\n");
    // The Job merges the Lead tip and resolves the conflict.
    let merge = fx.git_output(&worktree, &["merge", "--no-ff", "-m", "merge the base", &old]);
    assert!(!merge.status.success(), "the fixture needs a conflict");
    fs::write(worktree.join("README.md"), "resolved\n").unwrap();
    fx.git(&worktree, &["add", "-A"]);
    let tip = fx.merge_commit(&worktree, &old, "merge the base");
    fx.resubmit(&attempt, &worktree);

    let result = fx.merge(&attempt);

    assert_eq!(result.end, End::Merged { sha: tip.clone() });
    assert_eq!(fx.lead_sha(), tip);
    assert_eq!(fx.git(&worktree, &["rev-parse", "HEAD"]), tip);
    // The merge commit is kept, with both parents.
    assert_eq!(fx.parents(&tip).len(), 2);
    assert_eq!(fx.parents(&tip)[1], old);
    assert_eq!(fs::read_to_string(fx.lead.join("README.md")).unwrap(), "resolved\n");
    assert_eq!(fx.job_row("api"), ("merged".into(), Some(tip), None));
}

#[test]
fn merge_conflict_aborts_the_rebase_and_retries_at_the_same_tier() {
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
    let rebasing = fx.git(&worktree, &["rev-parse", "--path-format=absolute", "--git-path", "rebase-merge"]);
    assert!(!worktree.join(rebasing).exists(), "a rebase is still in progress");
    assert_eq!(fx.lead_sha(), old);
    // Retried at the same Tier: the Job is queued again.
    let (status, outcome, cause, merged, _) = fx.attempt_row(&attempt);
    assert_eq!((status.as_str(), outcome.as_deref(), cause.as_deref(), merged), ("ended", Some("failed"), Some("merge_conflict"), None));
    assert_eq!(fx.job_row("api"), ("queued".into(), None, None));
    let events = fx.events(&attempt);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "attempt-retry");
    assert_eq!(events[0].1["outcome"], "failed");
    assert_eq!(events[0].1["cause"], "merge_conflict");
    assert_eq!(events[0].1["next_tier"], "S2");
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

/// Step 3 is removed (hs-d243): an untracked file in the Lead worktree is no
/// `dirty_target`; the merge lands, rule S syncs around the file and keeps it.
#[test]
fn an_untracked_file_in_the_lead_worktree_does_not_stop_the_merge() {
    let fx = fx();
    let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
    let other = fx.advance_lead("other.txt", "other\n");
    fs::write(fx.lead.join("stray.txt"), "untracked\n").unwrap();

    let result = fx.merge(&attempt);

    let new = fx.git(&worktree, &["rev-parse", "HEAD"]);
    assert_eq!(result.end, End::Merged { sha: new.clone() });
    assert_eq!(fx.git(&worktree, &["rev-parse", "HEAD~1"]), other);
    assert_eq!(fx.lead_sha(), new);
    assert_eq!(fs::read_to_string(fx.lead.join("api.txt")).unwrap(), "api\n");
    assert_eq!(fs::read_to_string(fx.lead.join("stray.txt")).unwrap(), "untracked\n");
    assert_eq!(fx.status(&fx.lead), "?? stray.txt");
    assert_eq!(fx.events(&attempt).iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["job-merged"]);
}

/// Rule S after the fast-forward never overwrites a Lead's staged edit: the
/// merge stands, the edit is kept, and `lead_worktree_stale` is reported.
#[test]
fn a_staged_edit_in_the_lead_worktree_is_kept_and_reported_stale() {
    let fx = fx();
    let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
    fs::write(fx.lead.join("README.md"), "the Lead's edit\n").unwrap();
    fx.git(&fx.lead, &["add", "README.md"]);

    let result = fx.merge(&attempt);

    let new = fx.git(&worktree, &["rev-parse", "HEAD"]);
    assert_eq!(result.end, End::Merged { sha: new.clone() });
    assert_eq!(fx.lead_sha(), new);
    assert_eq!(fx.job_row("api"), ("merged".into(), Some(new), None));
    assert_eq!(fs::read_to_string(fx.lead.join("README.md")).unwrap(), "the Lead's edit\n");
    assert_eq!(fx.staged_against(&fx.lead, "HEAD~1"), ["README.md"]);
    assert!(!fx.lead.join("api.txt").exists(), "rule S stopped: nothing was written");
    let events = fx.events(&attempt);
    assert_eq!(events.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["job-merged", "error"]);
    assert_eq!(events[1].1["code"], "lead_worktree_stale");
    assert_eq!(events[1].1["source"], "attempt run");
}

#[test]
fn tampered_when_the_diff_touches_a_protected_path() {
    let fx = fx();
    let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
    fx.commit(&worktree, ".cpd/x", "x\n");
    fx.resubmit(&attempt, &worktree);
    let old = fx.lead_sha();

    let result = fx.merge(&attempt);

    assert!(matches!(result.end, End::Failed { cause: Failure::Tampered, .. }), "{result:?}");
    assert_eq!(fx.lead_sha(), old);
    let data = fx.assert_failed(&attempt, "api", "tampered");
    assert!(data["detail"].as_str().unwrap().contains(".cpd/x"));
}

#[test]
fn an_impl_jobs_diff_that_changes_a_guarded_red_test_ends_tampered_unless_the_lead_approved_the_path() {
    // Contract §7.5, §7.7: the files of the `test` Jobs an `impl` Job needs are guarded at the merge as at the submit.
    for (approved, tampered) in [(None, true), (Some("[\"tests/\"]"), false)] {
        let fx = fx();
        fx.advance_lead("tests/t.rs", "// red\n");
        let (attempt, _) = fx.submitted_needing(&fx.first_pr(), "api", "impl", Some(("tests/t.rs", "// edited to pass\n")), Some("[\"tests/\"]"));
        if let Some(paths) = approved {
            fx.store().write("fixture", |tx| exec(tx, &format!("UPDATE jobs SET test_approved = '{paths}' WHERE key = 'api'"))).unwrap();
        }
        let old = fx.lead_sha();

        let result = fx.merge(&attempt);

        if tampered {
            assert!(matches!(result.end, End::Failed { cause: Failure::Tampered, .. }), "{result:?}");
            assert_eq!(fx.lead_sha(), old);
            let data = fx.assert_failed(&attempt, "api", "tampered");
            assert!(data["detail"].as_str().unwrap().contains("tests/t.rs"), "{data}");
        } else {
            assert!(matches!(result.end, End::Merged { .. }), "an approved path is merged: {result:?}");
        }
    }
}

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
fn tampered_also_by_the_protected_paths_of_the_prs_policy() {
    let fx = fx();
    fx.env.repo_policy("acme", "repo", "[protected]\npaths = [\"verify.sh\"]\n");
    let (attempt, _) = fx.submitted("api", "verify.sh", "exit 0\n");

    let result = fx.merge(&attempt);

    assert!(matches!(result.end, End::Failed { cause: Failure::Tampered, .. }), "{result:?}");
    fx.assert_failed(&attempt, "api", "tampered");
}

/// A commit on `main` writing `path` = `content` (`None`: removing it), pushed to `origin`.
fn base_edits(fx: &Fx, path: &str, content: Option<&str>) {
    match content {
        Some(content) => {
            fx.commit(&fx.repo, path, content);
        }
        None => {
            fx.git(&fx.repo, &["rm", "-q", path]);
            fx.git(&fx.repo, &["commit", "-q", "-m", &format!("remove {path}")]);
        }
    }
    fx.git(&fx.repo, &["push", "-q", "origin", "main"]);
}

/// The Attempt's worktree merges `origin/main` (the Lead tip is its ancestor, so no rebase).
fn merge_the_base(fx: &Fx, attempt: &AttemptId, worktree: &Path) -> String {
    fx.git(worktree, &["merge", "-q", "--no-ff", "-m", "merge the base", "origin/main"]);
    fx.resubmit(attempt, worktree);
    fx.git(worktree, &["rev-parse", "HEAD"])
}

#[test]
fn an_attempt_that_merges_a_base_with_changed_protected_files_is_merged() {
    for path in [".cpd/x"] {
        let (fx, _) = fx_origin();
        let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
        base_edits(&fx, path, Some("protected = [\"verify.sh\"]\n"));
        let tip = merge_the_base(&fx, &attempt, &worktree);

        let result = fx.merge(&attempt);

        assert_eq!(result.end, End::Merged { sha: tip.clone() }, "{path}");
        assert_eq!(fx.lead_sha(), tip, "{path}");
        assert_eq!(fx.job_row("api"), ("merged".into(), Some(tip), None), "{path}");
    }
}

#[test]
fn an_attempt_that_changes_a_protected_file_to_anything_but_the_bases_is_tampered() {
    for path in [".cpd/x"] {
        let (fx, _) = fx_origin();
        let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
        base_edits(&fx, path, Some("protected = [\"verify.sh\"]\n"));
        merge_the_base(&fx, &attempt, &worktree);
        fx.commit(&worktree, path, "protected = [\"mine\"]\n");
        fx.resubmit(&attempt, &worktree);
        let old = fx.lead_sha();

        let result = fx.merge(&attempt);

        assert!(matches!(result.end, End::Failed { cause: Failure::Tampered, .. }), "{path}: {result:?}");
        assert_eq!(fx.lead_sha(), old, "{path}");
        fx.assert_failed(&attempt, "api", "tampered");
    }
}

#[test]
fn a_protected_file_deleted_on_the_base_and_in_the_attempt_is_merged() {
    for path in [".cpd/x"] {
        let (fx, _) = fx_origin();
        base_edits(&fx, path, Some("protected = []\n"));
        fx.git(&fx.lead, &["merge", "-q", "--ff-only", "origin/main"]);
        let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
        base_edits(&fx, path, None);
        let tip = merge_the_base(&fx, &attempt, &worktree);
        assert!(!worktree.join(path).exists());

        let result = fx.merge(&attempt);

        assert_eq!(result.end, End::Merged { sha: tip }, "{path}");
    }
}

/// PR `base` (Lead Branch `cpd/pay/base`) with a bare `origin`, a PR `web` stacked on it (Lead Branch `cpd/pay/web`),
/// and a Job of `web` whose Attempt has one commit.
fn stacked_job() -> (Fx, Pr, Pr, AttemptId, PathBuf) {
    let fx = plan_without_prs("");
    let origin = add_origin(&fx, &fx.repo);
    let base = fx.add_pr("base", &fx.repo, Some(origin));
    let web = fx.add_stacked_pr("web", &base);
    let (attempt, worktree) = fx.submitted_on(&web, "ui", "ui.txt", "ui\n");
    (fx, base, web, attempt, worktree)
}

/// The Attempt's worktree merges `reference` (the Lead tip is its ancestor, so no rebase) and is submitted again.
fn merge_ref(fx: &Fx, attempt: &AttemptId, worktree: &Path, reference: &str) {
    fx.git(worktree, &["merge", "-q", "--no-ff", "-m", "merge the base", reference]);
    fx.resubmit(attempt, worktree);
}

/// `pr`'s Lead Branch moves on (another Job's merge, a commit writing `path`) and is pushed.
fn advance_and_push(fx: &Fx, pr: &Pr, path: &str, content: &str) {
    fx.advance(pr, path, content);
    fx.git(&pr.lead, &["push", "-q", "origin", &pr.branch]);
}

#[test]
fn a_stacked_prs_job_that_merges_the_base_prs_protected_file_changes_is_merged() {
    for path in [".cpd/x"] {
        let (fx, base, web, attempt, worktree) = stacked_job();
        // The base PR's Lead Branch moves on, a protected file with it, and is pushed.
        advance_and_push(&fx, &base, path, "protected = [\"verify.sh\"]\n");
        merge_ref(&fx, &attempt, &worktree, "origin/cpd/pay/base");
        let tip = fx.git(&worktree, &["rev-parse", "HEAD"]);

        let result = fx.merge(&attempt);

        assert_eq!(result.end, End::Merged { sha: tip.clone() }, "{path}");
        assert_eq!(fx.tip(&web), tip, "{path}");
    }
}

#[test]
fn a_stacked_prs_job_that_merges_the_root_base_brings_its_protected_file_changes_without_tampering() {
    for path in [".cpd/x"] {
        let (fx, _, web, attempt, worktree) = stacked_job();
        // `main`, the branch the stack lands on, changes a protected file the base PR's branch does not have yet.
        base_edits(&fx, path, Some("protected = [\"verify.sh\"]\n"));
        merge_ref(&fx, &attempt, &worktree, "origin/main");
        let tip = fx.git(&worktree, &["rev-parse", "HEAD"]);

        let result = fx.merge(&attempt);

        assert_eq!(result.end, End::Merged { sha: tip.clone() }, "{path}");
        assert_eq!(fx.tip(&web), tip, "{path}");
    }
}

#[test]
fn a_stacked_prs_job_that_changes_a_protected_file_to_anything_else_is_tampered() {
    for path in [".cpd/x"] {
        let (fx, base, web, attempt, worktree) = stacked_job();
        base_edits(&fx, path, Some("protected = [\"verify.sh\"]\n"));
        advance_and_push(&fx, &base, path, "protected = [\"base-pr\"]\n");
        merge_ref(&fx, &attempt, &worktree, "origin/main");
        fx.commit(&worktree, path, "protected = [\"mine\"]\n");
        fx.resubmit(&attempt, &worktree);
        let old = fx.tip(&web);

        let result = fx.merge(&attempt);

        assert!(matches!(result.end, End::Failed { cause: Failure::Tampered, .. }), "{path}: {result:?}");
        assert_eq!(fx.tip(&web), old, "{path}");
    }
}

#[test]
fn a_job_of_a_pr_that_is_not_stacked_gets_no_extra_base_for_a_protected_file_only_another_branch_has() {
    // The root base is the stacked PR's extra: a PR that is not stacked accepts the Lead tip's and `origin/main`'s only.
    let fx = plan_without_prs("");
    let origin = add_origin(&fx, &fx.repo);
    let one = fx.add_pr("one", &fx.repo, Some(origin.clone()));
    let other = fx.add_pr("other", &fx.repo, Some(origin));
    let (attempt, worktree) = fx.submitted_on(&one, "api", "api.txt", "api\n");
    advance_and_push(&fx, &other, ".cpd/x", "other\n");
    merge_ref(&fx, &attempt, &worktree, "origin/cpd/pay/other");

    let result = fx.merge(&attempt);

    assert!(matches!(result.end, End::Failed { cause: Failure::Tampered, .. }), "{result:?}");
}

#[test]
fn cas_lost_when_the_lead_branch_keeps_moving_outside_cpd() {
    let fx = fx();
    let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
    fx.advance_lead("other.txt", "other\n");
    // Someone outside cpd moves the Lead Branch under every fast-forward, to a commit with the same tree (so the Lead
    // worktree stays clean): the `update-ref` of the merge answers that it cannot lock the ref, and the ref is not where the
    // merge saw it. Scripted: the rule moves the ref (in the `Universe`, as the outsider's own git would) and counts the moves.
    let moves = Arc::new(AtomicUsize::new(0));
    let (universe, repo, counted) = (fx.universe.clone(), fx.repo.clone(), moves.clone());
    fx.script(
        move |call| {
            if !is(call, "update-ref") || !call.args.iter().any(|a| a == "refs/heads/cpd/pay") {
                return false;
            }
            let mut u = universe.lock().unwrap();
            let id = u.repo_of(&repo).expect("a repo of the Universe");
            let old = u.ref_oid(id, "refs/heads/cpd/pay").expect("the Lead Branch");
            let tree = u.commit_data(&old).expect("a commit").tree;
            let outsider = u.commit(id, &tree, &[old], "outsider");
            u.set_ref(id, "refs/heads/cpd/pay", &outsider, "outsider");
            counted.fetch_add(1, Ordering::SeqCst);
            true
        },
        Reply::exit(128, "", "fatal: cannot lock ref 'refs/heads/cpd/pay': is at another commit than expected\n"),
    );

    let result = fx.merge(&attempt);

    assert!(matches!(result.end, End::Failed { cause: Failure::CasLost, .. }), "{result:?}");
    // Back to step 2 at most 3 times: 4 fast-forwards tried, each lost.
    assert_eq!(moves.load(Ordering::SeqCst), 1 + merge::CAS_RETRIES as usize);
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
    // Step 9 (rule S) cannot write the new file: git's `read-tree -m -u` fails on the directory.
    fx.script(|call| is(call, "read-tree"), Reply::exit(128, "", "error: unable to create file api.txt: Permission denied\nfatal: Could not reset index file to revision\n"));

    let result = fx.merge(&attempt);

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
    let err = fx.try_merge(&attempt).unwrap_err();
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
    // The `pre-receive` hook of origin declines the push (the hook is the remote's, not ours: scripted).
    fx.script(|call| is(call, "push") && is(call, "--porcelain"), declined_push());
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

impl Fx {
    /// `data` of every `error` Event of the Plan, in order.
    fn plan_errors(&self) -> Vec<Value> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare("SELECT data FROM events WHERE plan = 'pay' AND kind = 'error' ORDER BY id")?;
                let rows = stmt
                    .query_map([], |r| Ok(serde_json::from_str(&r.get::<_, String>(0)?).unwrap()))?
                    .collect::<rusqlite::Result<Vec<Value>>>()?;
                Ok(rows)
            })
            .unwrap()
    }
}

fn codes(errors: &[Value]) -> Vec<&str> {
    errors.iter().map(|e| e["code"].as_str().unwrap()).collect()
}

#[test]
fn a_driver_error_repeated_for_the_plan_inside_the_window_is_appended_once() {
    let (fx, _origin) = fx_origin();
    fx.script(|call| is(call, "push") && is(call, "--porcelain"), declined_push());
    let (api, _) = fx.submitted("api", "api.txt", "api\n");
    fx.merge(&api);
    let (web, _) = fx.submitted("web", "web.txt", "web\n");
    fx.merge(&web);

    assert_eq!(fx.job_row("api").0, "merged");
    assert_eq!(fx.job_row("web").0, "merged");
    let errors = fx.plan_errors();
    assert_eq!(codes(&errors), ["push_failed"], "{errors:?}");

    let (docs, _) = fx.submitted("docs", "docs.txt", "docs\n");
    fx.script(|call| is(call, "read-tree"), Reply::exit(128, "", "error: unable to create file docs.txt: Permission denied\nfatal: Could not reset index file to revision\n"));
    fx.merge(&docs);

    let errors = fx.plan_errors();
    assert_eq!(codes(&errors), ["push_failed", "lead_worktree_stale"], "{errors:?}");
}

#[test]
fn a_driver_error_repeated_for_the_plan_after_the_window_is_appended_again() {
    let (fx, _origin) = fx_origin();
    fx.script(|call| is(call, "push") && is(call, "--porcelain"), declined_push());
    let (api, _) = fx.submitted("api", "api.txt", "api\n");
    fx.merge(&api);
    let (web, _) = fx.submitted("web", "web.txt", "web\n");
    fx.merge(&web);
    let errors = fx.plan_errors();
    assert_eq!(codes(&errors), ["push_failed"], "{errors:?}");

    fx.store()
        .write("fixture", |tx| {
            let guard: String = tx.query_row("SELECT sql FROM sqlite_schema WHERE name = 'events_no_update'", [], |r| r.get(0))?;
            tx.execute_batch(&format!(
                "DROP TRIGGER events_no_update;
                 UPDATE events SET at = '2026-01-01T00:00:00.000Z'
                  WHERE id = (SELECT min(id) FROM events WHERE plan = 'pay' AND kind = 'error');
                 {guard};"
            ))?;
            Ok(())
        })
        .unwrap();
    let (docs, _) = fx.submitted("docs", "docs.txt", "docs\n");
    fx.merge(&docs);

    let errors = fx.plan_errors();
    assert_eq!(codes(&errors), ["push_failed", "push_failed"], "{errors:?}");
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

/// Rule P: a push rejected because origin already holds a descendant of `new`
/// (a later merge pushed first) is a success, with no `push_failed`.
#[test]
fn a_push_that_origin_already_contains_is_no_push_failed() {
    let (fx, origin) = fx_origin();
    let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
    // The Attempt already contains the Lead tip, so step 5 does not rebase: `new` is its HEAD.
    let new = fx.git(&worktree, &["rev-parse", "HEAD"]);
    // A descendant of `new` reaches origin first, then leaves the worktree.
    let later = fx.commit(&worktree, "later.txt", "later\n");
    fx.git(&worktree, &["push", "-q", "origin", &format!("{later}:refs/heads/cpd/pay")]);
    fx.git(&worktree, &["reset", "-q", "--hard", &new]);

    let result = fx.merge(&attempt);

    assert_eq!(result.end, End::Merged { sha: new.clone() });
    assert_eq!(fx.lead_sha(), new);
    assert_eq!(fx.git(&origin, &["rev-parse", "refs/heads/cpd/pay"]), later);
    assert_eq!(fx.events(&attempt).iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["job-merged"]);
}

#[test]
fn a_remote_rejected_lock_race_the_origin_already_contains_is_no_push_failed() {
    let (fx, origin) = fx_origin();
    let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
    let new = fx.git(&worktree, &["rev-parse", "HEAD"]);
    let later = fx.commit(&worktree, "later.txt", "later\n");
    fx.git(&worktree, &["push", "-q", "origin", &format!("{later}:refs/heads/cpd/pay")]);
    fx.git(&worktree, &["reset", "-q", "--hard", &new]);
    fx.script(|call| is(call, "push") && is(call, "--porcelain"), lock_race_push());

    let result = fx.merge(&attempt);

    assert_eq!(result.end, End::Merged { sha: new.clone() });
    assert_eq!(fx.git(&origin, &["rev-parse", "refs/heads/cpd/pay"]), later);
    assert_eq!(fx.events(&attempt).iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["job-merged"]);
}

#[test]
fn a_remote_rejected_push_the_origin_does_not_contain_puts_git_s_reason_in_the_push_failed_message() {
    let (fx, _origin) = fx_origin();
    fx.script(|call| is(call, "push") && is(call, "--porcelain"), lock_race_push());
    let (attempt, _) = fx.submitted("api", "api.txt", "api\n");

    fx.merge(&attempt);

    let events = fx.events(&attempt);
    assert_eq!(events.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["job-merged", "error"]);
    assert_eq!(events[1].1["code"], "push_failed");
    assert!(events[1].1["message"].as_str().unwrap().contains("cannot lock ref"), "{:?}", events[1].1);
    assert!(events[1].1["details"]["stderr"].as_str().unwrap().contains("failed to push some refs"), "{:?}", events[1].1);
}

// ------------------------------------------------- an `open` or `update` Job's merge (contract §7.4; hs-d340)

/// Step 5 never rebases for these kinds: the Gate ran on the submitted HEAD, and a rebase would drop its merge commit and replay the base's
/// commits. The Lead tip must already be an ancestor of HEAD, else the Attempt ends `failed` / `merge_conflict` with the detail
/// `lead_branch_moved` (retried at the same Tier: the next Attempt starts from the newer tip). Nothing moves: no rebase, no fast-forward, no push.
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
    assert_eq!(fx.job_row("up-repo"), ("queued".into(), None, None), "retried at the same Tier: the next Attempt starts from the newer tip");
}

/// An `update` Job whose Worker merged the base (the Lead tip is its ancestor) lands as it is, `new` = HEAD, and its push is the point:
/// origin holds the resolution when the Job is `merged`.
#[test]
fn an_update_jobs_merge_pushes_what_it_landed() {
    let (fx, origin) = fx_origin();
    let (attempt, worktree) = fx.submitted_as(&fx.first_pr(), "up-repo", "update", Some(("resolved.txt", "resolved\n")));

    let result = fx.merge(&attempt);

    let new = fx.git(&worktree, &["rev-parse", "HEAD"]);
    assert_eq!(result.end, End::Merged { sha: new.clone() });
    assert_eq!(fx.git(&origin, &["rev-parse", "refs/heads/cpd/pay"]), new);
    assert_eq!(fx.job_row("up-repo"), ("merged".into(), Some(new), None));
    let kinds: Vec<String> = fx.events(&attempt).into_iter().map(|e| e.0).collect();
    assert_eq!(kinds, ["job-merged"]);
    // `job-merged` carries the Job's kind (§5.2); an `update` Job writes no PR row and no `pr-opened`.
    let merged = &fx.events(&attempt)[0].1;
    assert_eq!(merged["kind"], "update", "{merged}");
}

/// The push is part of the Job (§7.4 step 8): one that fails and is not rule P's success ends the Attempt `failed` / `crashed` with the
/// detail `push_failed`, after the fast-forward (the next Attempt finds the Lead Branch already holding the merge), where an ordinary Job
/// stays Merged with a `push_failed` error.
#[test]
fn a_failed_push_of_an_update_job_ends_the_attempt_crashed_with_push_failed_after_the_fast_forward() {
    let (fx, origin) = fx_origin();
    let before = fx.git(&origin, &["rev-parse", "refs/heads/cpd/pay"]);
    fx.script(|call| is(call, "push") && is(call, "--porcelain"), declined_push());
    let (attempt, worktree) = fx.submitted_as(&fx.first_pr(), "up-repo", "update", Some(("resolved.txt", "resolved\n")));

    let result = fx.merge(&attempt);

    let End::Failed { cause: Failure::Crashed, detail } = &result.end else { panic!("{result:?}") };
    assert!(detail.contains("push_failed"), "{detail}");
    assert_eq!(fx.lead_sha(), fx.git(&worktree, &["rev-parse", "HEAD"]), "the fast-forward stands");
    assert_eq!(fx.git(&origin, &["rev-parse", "refs/heads/cpd/pay"]), before);
    let (status, outcome, cause, merged, _) = fx.attempt_row(&attempt);
    assert_eq!((status.as_str(), outcome.as_deref(), cause.as_deref(), merged), ("ended", Some("failed"), Some("crashed"), None));
    assert_eq!(fx.job_row("up-repo"), ("queued".into(), None, None), "crashed is retried at the same Tier");
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
        [("job-merged".to_owned(), json!({"sha": tip, "tier": "S2", "kind": "work", "turn": false, "cancel_too_late": true}))]
    );
    // The rest of A9: the worktree and branch are gone.
    assert!(!worktree.exists());
    assert!(!fx.branch_exists("cpd/pay--api-a1"));
    // Idempotent: nothing left to recover.
    assert!(fx.recover().is_empty());
}

#[test]
fn r_before_step_8_ends_crashed_and_retries_at_the_same_tier() {
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
    assert_eq!((events[0].0.as_str(), &events[0].1["cause"], &events[0].1["next_tier"]), ("attempt-retry", &json!("crashed"), &json!("S2")));
    // Kept for debugging.
    assert!(worktree.exists());
}

/// G13: R removes the driver lock file of an Attempt it ended, and nothing recreates it.
#[test]
fn r_removes_the_driver_lock_of_the_attempt_it_ended() {
    for merged in [true, false] {
        let fx = fx();
        let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
        if merged {
            die_after_step_8(&fx, &worktree);
        }
        let lock = fx.home.attempt_lock(&plan(), &attempt);

        let recovered = fx.recover();

        assert_eq!(recovered.len(), 1, "merged: {merged}");
        assert_eq!(fx.attempt_row(&attempt).0, "ended");
        assert!(!lock.exists(), "merged: {merged}: {}", lock.display());
        assert!(fx.recover().is_empty());
        assert!(!lock.exists(), "a second R recreated {}", lock.display());
    }
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
    fx.script(|call| is(call, "rev-parse") && is(call, "refs/heads/cpd/pay--api-a1"), Reply::exit(128, "", "fatal: Needed a single revision\n"));

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

/// The merge runs no R first (hs-d243): a dead driver's stale Lead worktree is
/// finished by the next merge's rule S, and its Attempt is left to R (E2).
#[test]
fn rule_s_finishes_a_dead_drivers_stale_lead_worktree_and_the_merge_leaves_its_attempt_to_r() {
    let fx = fx();
    let (dead, dead_worktree) = fx.submitted("api", "api.txt", "api\n");
    let (live, live_worktree) = fx.submitted("ui", "ui.txt", "ui\n");
    let dead_tip = die_after_step_8(&fx, &dead_worktree);
    assert_ne!(fx.status(&fx.lead), "", "the fixture needs a stale Lead worktree");

    let result = fx.merge(&live);

    let new = fx.git(&live_worktree, &["rev-parse", "HEAD"]);
    assert_eq!(result.end, End::Merged { sha: new.clone() });
    assert_eq!(fx.git(&live_worktree, &["rev-parse", "HEAD~1"]), dead_tip);
    assert_eq!(fx.lead_sha(), new);
    assert_eq!(fx.status(&fx.lead), "");
    assert!(fx.lead.join("api.txt").exists() && fx.lead.join("ui.txt").exists());
    assert_eq!(fx.events(&live).iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["job-merged"]);
    assert_eq!(fx.job_row("ui").0, "merged");
    // Not the merge's: the dead driver's Attempt is still `merging` until R.
    assert_eq!(fx.attempt_row(&dead).0, "merging");
    assert_eq!(fx.job_row("api").0, "running");
    let recovered = fx.recover();
    assert_eq!(recovered.len(), 1);
    assert_eq!((&recovered[0].attempt, &recovered[0].end), (&dead, &Some(End::Merged { sha: dead_tip })));
    assert_eq!(fx.job_row("api").0, "merged");
}

/// Two merges of one PR at once, with no lock: the compare-and-swap lets one
/// in and sends the other back to step 2; both land, and rule S leaves the
/// Lead worktree at the tip, clean.
#[test]
fn two_merges_of_one_pr_at_once_both_land_and_the_lead_worktree_ends_at_the_tip() {
    let (fx, origin) = fx_origin();
    let (api, _) = fx.submitted("api", "api.txt", "api\n");
    let (ui, _) = fx.submitted("ui", "ui.txt", "ui\n");

    // Two `cpd attempt run` of this process: each thread scopes the fake ports itself, and the route's mutex is git's ref lock.
    let ports = fx.fakes.ports.clone();
    let merge_on_thread = |attempt: &AttemptId| {
        Ports::scope(&ports, || {
            let ctx = Ctx { home: &fx.home, config: &fx.config, catalog: &fx.catalog, source: "attempt run", metrics: &metrics };
            merge::merge(&mut Store::open(&fx.home, 5000).unwrap(), &ctx, &plan(), attempt).unwrap()
        })
    };
    let (a, b) = std::thread::scope(|s| {
        let a = s.spawn(|| merge_on_thread(&api));
        let b = s.spawn(|| merge_on_thread(&ui));
        (a.join().unwrap(), b.join().unwrap())
    });

    let (a, b) = (merged_sha(&a), merged_sha(&b));
    let tip = fx.lead_sha();
    assert!(tip == a || tip == b, "{tip} is neither {a} nor {b}");
    for sha in [&a, &b] {
        fx.git(&fx.repo, &["merge-base", "--is-ancestor", sha, &tip]);
    }
    assert_eq!(fx.git(&origin, &["rev-parse", "refs/heads/cpd/pay"]), tip);
    assert_eq!(fx.status(&fx.lead), "");
    assert_eq!(fx.git(&fx.lead, &["rev-parse", "HEAD"]), tip);
    assert!(fx.lead.join("api.txt").exists() && fx.lead.join("ui.txt").exists());
    for attempt in [&api, &ui] {
        assert_eq!(fx.events(attempt).iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["job-merged"]);
    }
}

// ------------------------------------------------------------ PRs across repos (R34)

/// Plan `pay` with two PRs, `schema` and `web`, both started: in two repos
/// (`pay` and `web`), or both in `pay` (then on branches `cpd/pay/schema` and
/// `cpd/pay/web`). With `with_origin` each repo has a bare `origin`.
fn two_prs(same_repo: bool, with_origin: bool) -> (Fx, Pr, Pr) {
    let fx = plan_without_prs("");
    let origin = |repo: &Path| with_origin.then(|| add_origin(&fx, repo));
    let schema_origin = origin(&fx.repo);
    let schema = fx.add_pr("schema", &fx.repo, schema_origin.clone());
    let web = if same_repo {
        fx.add_pr("web", &fx.repo, schema_origin)
    } else {
        let repo = fx.git_repo("web").canonicalize().unwrap();
        let web_origin = origin(&repo);
        fx.add_pr("web", &repo, web_origin)
    };
    (fx, schema, web)
}

/// [`die_after_step_8`] for a Job of `pr`.
fn die_after_step_8_on(fx: &Fx, pr: &Pr, worktree: &Path) -> String {
    let old = fx.tip(pr);
    fx.git(worktree, &["rebase", &old]);
    let tip = fx.git(worktree, &["rev-parse", "HEAD"]);
    fx.git(&pr.repo, &["update-ref", &format!("refs/heads/{}", pr.branch), &tip, &old]);
    tip
}

fn merged_sha(result: &Merge) -> String {
    let End::Merged { sha } = &result.end else { panic!("{result:?}") };
    sha.clone()
}

#[test]
fn in_two_repos_each_merge_lands_on_its_own_prs_branch_only() {
    let (fx, schema, web) = two_prs(false, false);
    let (schema_old, web_old) = (fx.tip(&schema), fx.tip(&web));
    let (api, api_worktree) = fx.submitted_on(&schema, "api", "api.txt", "api\n");
    let (ui, ui_worktree) = fx.submitted_on(&web, "ui", "ui.txt", "ui\n");
    // The Attempt worktrees are siblings of their own PR's repo, cut from its Lead Branch.
    assert_eq!(api_worktree.parent(), schema.repo.parent());
    assert_eq!(fx.git(&api_worktree, &["rev-parse", "HEAD~1"]), schema_old);
    assert_eq!(fx.git(&ui_worktree, &["rev-parse", "HEAD~1"]), web_old);

    let on_schema = merged_sha(&fx.merge(&api));
    assert_eq!(fx.tip(&schema), on_schema);
    assert_eq!(fx.tip(&web), web_old, "the other PR's branch did not move");
    let on_web = merged_sha(&fx.merge(&ui));

    assert_eq!(fx.tip(&schema), on_schema);
    assert_eq!(fx.tip(&web), on_web);
    assert_eq!(fx.git(&schema.repo, &["rev-parse", &format!("{on_schema}~1")]), schema_old);
    assert_eq!(fx.git(&web.repo, &["rev-parse", &format!("{on_web}~1")]), web_old);
    // Each repo holds its own PR's branch and not the other's.
    assert!(!fx.git(&web.repo, &["branch", "--list", "cpd/pay/schema"]).contains("schema"));
    assert!(!fx.git(&schema.repo, &["branch", "--list", "cpd/pay/web"]).contains("web"));
    // Step 9 synced each Lead worktree; step 10 recorded both.
    assert_eq!(fs::read_to_string(schema.lead.join("api.txt")).unwrap(), "api\n");
    assert_eq!(fs::read_to_string(web.lead.join("ui.txt")).unwrap(), "ui\n");
    assert!(!schema.lead.join("ui.txt").exists() && !web.lead.join("api.txt").exists());
    assert_eq!((fx.status(&schema.lead), fx.status(&web.lead)), (String::new(), String::new()));
    assert_eq!(fx.job_row("api"), ("merged".into(), Some(on_schema.clone()), None));
    assert_eq!(fx.job_row("ui"), ("merged".into(), Some(on_web.clone()), None));
    assert_eq!(fx.reflog_message(&web.repo, "cpd/pay/web"), "cpd: merge pay/ui-a1");
}

#[test]
fn in_one_repo_each_merge_lands_on_its_own_prs_branch_only() {
    let (fx, schema, web) = two_prs(true, false);
    assert_eq!(schema.repo, web.repo);
    let web_old = fx.tip(&web);
    let (api, _) = fx.submitted_on(&schema, "api", "api.txt", "api\n");

    let on_schema = merged_sha(&fx.merge(&api));
    assert_eq!(fx.tip(&schema), on_schema);
    assert_eq!(fx.tip(&web), web_old);
    assert!(!web.lead.join("api.txt").exists());
    let (ui, _) = fx.submitted_on(&web, "ui", "ui.txt", "ui\n");
    let on_web = merged_sha(&fx.merge(&ui));

    assert_eq!(fx.tip(&schema), on_schema);
    assert_eq!(fx.tip(&web), on_web);
    // The two branches share the base, not each other's commits.
    assert_eq!(fx.git(&web.repo, &["merge-base", &on_schema, &on_web]), fx.git(&web.repo, &["rev-parse", &format!("{on_web}~1")]));
    assert!(schema.lead.join("api.txt").exists() && !schema.lead.join("ui.txt").exists());
    assert!(web.lead.join("ui.txt").exists() && !web.lead.join("api.txt").exists());
    assert_eq!((fx.status(&schema.lead), fx.status(&web.lead)), (String::new(), String::new()));
    assert_eq!(fx.job_row("api").0, "merged");
    assert_eq!(fx.job_row("ui").0, "merged");
}

#[test]
fn the_cas_and_the_dirty_check_are_those_of_the_prs_own_branch_and_worktree() {
    let (fx, schema, web) = two_prs(true, false);
    let (api, _) = fx.submitted_on(&schema, "api", "api.txt", "api\n");
    // Another Job's merge moved the web PR's branch, and its Lead worktree is dirty: not schema's concern.
    fx.advance(&web, "other.txt", "other\n");
    fs::write(web.lead.join("stray.txt"), "untracked\n").unwrap();
    // And one that moved the schema PR's branch: the rebase picks it up.
    let moved = fx.advance(&schema, "moved.txt", "moved\n");

    let worktree = fx.store().read(|c| Ok(c.query_row("SELECT worktree FROM attempts WHERE id = 'api-a1'", [], |r| r.get::<_, String>(0))?)).unwrap();
    let sha = merged_sha(&fx.merge(&api));

    assert_eq!(fx.git(Path::new(&worktree), &["rev-parse", "HEAD~1"]), moved);
    assert_eq!(fx.tip(&schema), sha);
}

#[test]
fn the_tamper_list_is_that_of_the_prs_own_policy() {
    let (fx, schema, web) = two_prs(true, false);
    // Only the schema PR's policy protects verify.sh.
    fx.env.repo_policy("acme", "schema", "[protected]\npaths = [\"verify.sh\"]\n");
    let (api, _) = fx.submitted_on(&schema, "api", "verify.sh", "exit 0\n");

    let result = fx.merge(&api);
    assert!(matches!(result.end, End::Failed { cause: Failure::Tampered, .. }), "{result:?}");
    fx.assert_failed(&api, "api", "tampered");

    let (ui, _) = fx.submitted_on(&web, "ui", "verify.sh", "exit 0\n");
    let on_web = merged_sha(&fx.merge(&ui));
    assert_eq!(fx.tip(&web), on_web);
    assert!(web.lead.join("verify.sh").exists());
}

#[test]
fn each_merge_pushes_its_own_prs_branch_to_its_own_origin() {
    let (fx, schema, web) = two_prs(false, true);
    let (schema_remote, web_remote) = (fx.remote_tip(&schema), fx.remote_tip(&web));
    let (api, _) = fx.submitted_on(&schema, "api", "api.txt", "api\n");
    let (ui, _) = fx.submitted_on(&web, "ui", "ui.txt", "ui\n");

    let on_schema = merged_sha(&fx.merge(&api));
    assert_eq!(fx.remote_tip(&schema), on_schema);
    assert_eq!(fx.remote_tip(&web), web_remote, "the other repo's origin was not touched");
    let on_web = merged_sha(&fx.merge(&ui));

    assert_eq!(fx.remote_tip(&web), on_web);
    assert_eq!(fx.remote_tip(&schema), on_schema);
    assert_ne!(schema_remote, on_schema);
    for attempt in [&api, &ui] {
        assert_eq!(fx.events(attempt).iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["job-merged"]);
    }
}

#[test]
fn in_one_repo_a_merge_pushes_its_prs_branch_and_not_the_others() {
    let (fx, schema, web) = two_prs(true, true);
    let web_remote = fx.remote_tip(&web);
    let (api, _) = fx.submitted_on(&schema, "api", "api.txt", "api\n");

    let on_schema = merged_sha(&fx.merge(&api));

    assert_eq!(fx.remote_tip(&schema), on_schema);
    assert_eq!(fx.remote_tip(&web), web_remote);
    assert_eq!(fx.events(&api).iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["job-merged"]);
}

#[test]
fn a_rejected_push_of_one_prs_branch_is_that_attempts_push_failed_only() {
    let (fx, schema, web) = two_prs(false, true);
    // Only the origin of the schema PR's repo declines.
    let schema_lead = schema.lead.clone();
    fx.script(move |call| is(call, "push") && is(call, "--porcelain") && in_dir(call, &schema_lead), declined_push());
    let (api, _) = fx.submitted_on(&schema, "api", "api.txt", "api\n");
    let (ui, _) = fx.submitted_on(&web, "ui", "ui.txt", "ui\n");

    let on_schema = merged_sha(&fx.merge(&api));
    let on_web = merged_sha(&fx.merge(&ui));

    assert_eq!(fx.job_row("api"), ("merged".into(), Some(on_schema), None));
    assert_eq!(fx.remote_tip(&web), on_web);
    let events = fx.events(&api);
    assert_eq!(events.iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["job-merged", "error"]);
    assert_eq!(events[1].1["code"], "push_failed");
    assert_eq!(fx.events(&ui).iter().map(|e| e.0.as_str()).collect::<Vec<_>>(), ["job-merged"]);
}

#[test]
fn r_recovers_the_orphans_of_its_repo_per_pr_and_pushes_each_prs_branch() {
    let (fx, schema, web) = two_prs(true, true);
    let (_, api_worktree) = fx.submitted_on(&schema, "api", "api.txt", "api\n");
    let (_, ui_worktree) = fx.submitted_on(&web, "ui", "ui.txt", "ui\n");
    let on_schema = die_after_step_8_on(&fx, &schema, &api_worktree);
    let on_web = die_after_step_8_on(&fx, &web, &ui_worktree);
    assert_ne!(fx.remote_tip(&schema), on_schema);

    let recovered = fx.recover_in(&web.repo);

    let ends: Vec<_> = recovered.iter().map(|r| (r.attempt.to_string(), r.end.clone())).collect();
    assert_eq!(
        ends,
        [
            ("api-a1".to_owned(), Some(End::Merged { sha: on_schema.clone() })),
            ("ui-a1".to_owned(), Some(End::Merged { sha: on_web.clone() })),
        ]
    );
    assert_eq!((fx.remote_tip(&schema), fx.remote_tip(&web)), (on_schema.clone(), on_web.clone()));
    // Step 9 finished in each PR's own Lead worktree.
    assert_eq!(fs::read_to_string(schema.lead.join("api.txt")).unwrap(), "api\n");
    assert_eq!(fs::read_to_string(web.lead.join("ui.txt")).unwrap(), "ui\n");
    assert_eq!((fx.status(&schema.lead), fx.status(&web.lead)), (String::new(), String::new()));
    assert_eq!(fx.job_row("api"), ("merged".into(), Some(on_schema), None));
    assert_eq!(fx.job_row("ui"), ("merged".into(), Some(on_web), None));
    assert!(!api_worktree.exists() && !ui_worktree.exists());
}

#[test]
fn r_of_one_repo_leaves_the_orphans_of_another_repo() {
    let (fx, schema, web) = two_prs(false, false);
    let (api, api_worktree) = fx.submitted_on(&schema, "api", "api.txt", "api\n");
    let (ui, ui_worktree) = fx.submitted_on(&web, "ui", "ui.txt", "ui\n");
    die_after_step_8_on(&fx, &schema, &api_worktree);
    let on_web = die_after_step_8_on(&fx, &web, &ui_worktree);

    let recovered = fx.recover_in(&web.repo);

    assert_eq!(recovered.len(), 1);
    assert_eq!((&recovered[0].attempt, &recovered[0].end), (&ui, &Some(End::Merged { sha: on_web })));
    assert_eq!(fx.attempt_row(&api).0, "merging");
    assert_eq!(fx.job_row("api").0, "running");
}

#[test]
fn a_merge_leaves_another_prs_orphan_in_its_repo_to_r() {
    let (fx, schema, web) = two_prs(true, false);
    let (dead, dead_worktree) = fx.submitted_on(&schema, "api", "api.txt", "api\n");
    let (live, _) = fx.submitted_on(&web, "ui", "ui.txt", "ui\n");
    let dead_tip = die_after_step_8_on(&fx, &schema, &dead_worktree);

    let result = fx.merge(&live);

    assert_eq!(fx.tip(&web), merged_sha(&result));
    assert_eq!(fx.tip(&schema), dead_tip);
    assert_eq!(fx.attempt_row(&dead).0, "merging");
    assert_eq!(fx.job_row("api").0, "running");
}


// ------------------------------------------------------------ a base's commits keep their SHAs (§7.4 step 5, hs-d747)

/// The Attempt of `api.txt`, `main` gaining `base.txt` (pushed), the Lead Branch gaining `lead.txt`, then `bring` run in
/// the Attempt worktree to bring `origin/main` in, and the Attempt submitted again. Returns the Attempt, and `main`'s SHA.
fn attempt_with_the_base(bring: &[&str]) -> (Fx, AttemptId, String) {
    let (fx, _) = fx_origin();
    let (attempt, worktree) = fx.submitted("api", "api.txt", "api\n");
    base_edits(&fx, "base.txt", Some("base\n"));
    let main = fx.git(&fx.repo, &["rev-parse", "origin/main"]);
    fx.advance_lead("lead.txt", "lead\n");
    fx.git(&worktree, bring);
    fx.resubmit(&attempt, &worktree);
    (fx, attempt, main)
}

fn assert_lands_with_the_base(fx: &Fx, attempt: &AttemptId, main: &str) {
    let result = fx.merge(attempt);

    let End::Merged { sha } = &result.end else { panic!("{result:?}") };
    assert_eq!(&fx.lead_sha(), sha);
    assert_eq!(fx.git(&fx.repo, &["rev-list", "--count", main, "--not", sha]), "0", "the base's commit lost its SHA");
    for (file, content) in [("api.txt", "api\n"), ("lead.txt", "lead\n"), ("base.txt", "base\n")] {
        assert_eq!(fs::read_to_string(fx.lead.join(file)).unwrap(), content, "{file}");
    }
}

#[test]
fn an_attempt_that_rebased_onto_the_base_lands_the_bases_commits_under_their_own_shas() {
    let (fx, attempt, main) = attempt_with_the_base(&["rebase", "origin/main"]);
    assert_lands_with_the_base(&fx, &attempt, &main);
}

#[test]
fn an_attempt_that_merged_the_base_lands_the_bases_commits_under_their_own_shas() {
    let (fx, attempt, main) = attempt_with_the_base(&["merge", "-q", "--no-ff", "-m", "merge the base", "origin/main"]);
    assert_lands_with_the_base(&fx, &attempt, &main);
}

#[test]
fn a_stacked_prs_attempt_that_brought_in_the_root_base_lands_the_bases_commits_under_their_own_shas() {
    let (fx, _, web, attempt, worktree) = stacked_job();
    base_edits(&fx, "base.txt", Some("base\n"));
    let main = fx.git(&fx.repo, &["rev-parse", "origin/main"]);
    advance_and_push(&fx, &web, "lead.txt", "lead\n");
    merge_ref(&fx, &attempt, &worktree, "origin/main");

    let result = fx.merge(&attempt);

    let End::Merged { sha } = &result.end else { panic!("{result:?}") };
    assert_eq!(&fx.tip(&web), sha);
    assert_eq!(fx.git(&fx.repo, &["rev-list", "--count", &main, "--not", sha]), "0", "the base's commit lost its SHA");
}

#[test]
fn a_conflict_with_the_lead_tip_when_the_attempt_holds_base_commits_is_a_merge_conflict_with_the_files() {
    let (fx, _) = fx_origin();
    let (attempt, worktree) = fx.submitted("api", "shared.txt", "mine\n");
    base_edits(&fx, "base.txt", Some("base\n"));
    let old = fx.advance_lead("shared.txt", "theirs\n");
    fx.git(&worktree, &["rebase", "origin/main"]);
    fx.resubmit(&attempt, &worktree);
    let before = fx.git(&worktree, &["rev-parse", "HEAD"]);

    let result = fx.merge(&attempt);

    let End::Failed { cause: Failure::MergeConflict, detail } = &result.end else { panic!("{result:?}") };
    assert!(detail.contains("shared.txt"), "{detail}");
    assert_eq!(fx.git(&worktree, &["rev-parse", "HEAD"]), before);
    assert_eq!(fx.status(&worktree), "");
    assert_eq!(fx.lead_sha(), old);
    assert_eq!(fx.job_row("api"), ("queued".into(), None, None));
    let events = fx.events(&attempt);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, "attempt-retry");
    assert_eq!(events[0].1["cause"], "merge_conflict");
    assert_eq!(events[0].1["next_tier"], "S2");
    assert!(events[0].1["feedback"].as_str().unwrap().contains("shared.txt"));
}
