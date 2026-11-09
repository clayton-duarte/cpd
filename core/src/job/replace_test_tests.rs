//! Contract tests for replacing a locked test Job (contract §3.2, air-d144).
//! The service seam below is deliberately unimplemented; the fixtures exercise
//! the real store and fake every external listing, repository and runner action.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::replace_test::{self, Boundary, Request};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::PlanId;
use crate::plan::jobs::{self, Job};
use crate::ports::Fakes;

const T: &str = "2026-01-01T00:00:00.000Z";
const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    Acknowledged,
    Failed,
    TimedOut,
    NaturalEnd,
    RunnerStillLive,
    MergeInProgress,
    CommitFailed,
}

/// Fake selector listing and repository refs: the map records the actual owner
/// of each test, not merely whether its name exists. No git or test process runs.
struct Boundaries {
    owners: BTreeMap<String, String>,
    refs: BTreeMap<String, String>,
    stop: Stop,
    stops: usize,
    fence_checks: usize,
    fenced: bool,
    ancestry: BTreeMap<String, Vec<String>>,
}

impl Boundary for Boundaries {
    fn owner(
        &mut self,
        _env: &mut Env,
        _job: &Job,
        _request: &Request,
        selector: &str,
    ) -> Result<String, CpdError> {
        self.owners
            .get(selector)
            .cloned()
            .ok_or_else(|| CpdError::new(ErrorCode::Usage, "selector not found"))
    }
    fn stop(&mut self, env: &mut Env, plan: &PlanId, request: &Request) -> Result<(), CpdError> {
        self.fence_checks += 1;
        self.fenced = env
            .store
            .read(|c| Ok(replace_test::check_fence(c, plan, &request.source).is_err()))?;
        self.stops += 1;
        if !matches!(self.stop, Stop::Acknowledged | Stop::NaturalEnd) {
            return Err(CpdError::new(ErrorCode::CasLost, "teardown failed"));
        }
        env.store.write("fake stop", |tx| {
            tx.execute("UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'aborted', ended_at = (SELECT at FROM txns WHERE id = cpd_txn()) WHERE plan = ?1 AND job = ?2 AND status = 'running'", rusqlite::params![plan.as_str(), request.source])?;
            Ok(())
        })?;
        Ok(())
    }
    fn quiescent(&mut self, _env: &mut Env, _plan: &PlanId, _source: &str) -> Result<(), CpdError> {
        if matches!(
            self.stop,
            Stop::RunnerStillLive | Stop::TimedOut | Stop::Failed | Stop::MergeInProgress
        ) {
            return Err(CpdError::new(ErrorCode::CasLost, "runner not quiescent"));
        }
        Ok(())
    }
    fn start(
        &mut self,
        _env: &mut Env,
        _plan: &PlanId,
        request: &Request,
    ) -> Result<String, CpdError> {
        let sha = self.refs.get("cpd/pay--old-tests-a1").unwrap().clone();
        self.refs
            .insert(format!("{}:start", request.new_key), sha.clone());
        Ok(sha)
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    env: Env,
    io: Boundaries,
    request: Request,
    old_selector: String,
    old_files: Vec<String>,
    commit: String,
}

impl Fixture {
    fn new(
        old_selector: &str,
        selector: &str,
        old_files: &[&str],
        file: &str,
        commit: &str,
    ) -> Self {
        let dir = tempfile::tempdir_in(".").unwrap();
        let home = CpdHome::resolve(Some(dir.path().join("home").into()), None).unwrap();
        std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
        std::fs::write(home.config(), "version = 1\nmachine = \"m1\"\n").unwrap();
        let fakes = Fakes::new();
        let mut env = Env::open_home_with(&fakes.ports, home).unwrap();
        let old_tests = json!([old_selector]).to_string();
        let old_files_json = json!(old_files).to_string();
        env.store.write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', '{T}', '{T}');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead-pay', '{T}', '{T}');
                 INSERT INTO workflows (plan, key, repo, base, status, shape, lead_branch, lead_worktree, created_txn, created_at, started_at)
                   VALUES ('pay', 'api', '/fake/repo', 'main', 'dispatch', 'tdd', 'cpd/pay/api', '/fake/lead', cpd_txn(), '{T}', '{T}');
                 INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'api', 'docs', 'Docs', 'S0', 'g', 'c', '[\"docs/\"]', '[\"a\"]', '[]', 'docs', 'queued', 0, '{FP}', cpd_txn(), '{T}');
                 INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, fingerprint, tests, test_files, resources, created_txn, created_at)
                   VALUES ('pay', 'api', 'old-tests', 'Red tests', 'S0', 'g', 'c', '[\"pi/test/\"]', '[\"a\"]', '[]', 'test', 'running', 0, '{FP}', ?1, ?2, '[\"cargo\"]', cpd_txn(), '{T}');
                 INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'api', 'impl', 'Implement', 'S0', 'g', 'c', '[\"engine/src/\"]', '[\"a\"]', '[]', 'impl', 'queued', 0, '{FP}', cpd_txn(), '{T}');"
            ).replace("?1", &format!("'{}'", old_tests.replace('\'', "''"))).replace("?2", &format!("'{}'", old_files_json.replace('\'', "''"))))?;
            tx.execute_batch(&format!(
                "INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'old-tests', 'docs'), ('pay', 'impl', 'old-tests');
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, driver_pid, base_sha, started_at)
                   VALUES ('pay', 'old-tests-a1', 'old-tests', 1, 'S0', 'ladder', 'running', 'cpd/pay--old-tests-a1', '/fake/attempt', 42, 'base', '{T}');"
            ))?;
            Ok(())
        }).unwrap();
        Self {
            _dir: dir,
            env,
            io: Boundaries {
                owners: BTreeMap::from([(selector.into(), file.into())]),
                refs: BTreeMap::from([("cpd/pay--old-tests-a1".into(), commit.into())]),
                stop: Stop::Acknowledged,
                stops: 0,
                fence_checks: 0,
                fenced: false,
                ancestry: BTreeMap::new(),
            },
            request: Request {
                source: "old-tests".into(),
                new_key: "new-tests".into(),
                tests: vec![selector.into()],
                test_files: vec![file.into()],
                reason: "correct the locked selector".into(),
                requester: "lead-pay".into(),
            },
            old_selector: old_selector.into(),
            old_files: old_files.iter().map(|s| (*s).into()).collect(),
            commit: commit.into(),
        }
    }

    fn repetition() -> Self {
        Self::new(
            "durable_worker_repetition_is_detected",
            "pi/test/durable-worker/run.test.ts::durable_worker_repetition_is_detected",
            &["engine/src/"],
            "pi/test/durable-worker/run.test.ts",
            "7db2966d5",
        )
    }

    fn webui() -> Self {
        let mut f = Self::new(
            "webui_human_input_restores_tripped_guard",
            "pi/test/webui-guard.test.ts::webui_human_input_restores_tripped_guard",
            &["pi/test/"],
            "pi/test/webui-guard.test.ts",
            "85129dad145facd53c9757c36673edf484389344",
        );
        f.request.test_files = vec!["pi/test/".into()];
        f
    }

    fn run(&mut self) -> Result<(), String> {
        replace_test::replace(
            &mut self.env,
            &mut self.io,
            &PlanId::parse("pay").unwrap(),
            &self.request,
        )
        .map_err(|e| e.to_string())
    }

    fn jobs(&mut self) -> Vec<Job> {
        self.env
            .store
            .read(|c| jobs::load(c, &PlanId::parse("pay").unwrap()))
            .unwrap()
    }

    fn row(&mut self, query: &str) -> String {
        self.env
            .store
            .read(|c| Ok(c.query_row(query, [], |r| r.get(0))?))
            .unwrap()
    }

    fn snapshot(&mut self) -> Value {
        self.env.store.read(|c| {
            let mut out = serde_json::Map::new();
            for (name, query) in [
                ("jobs", "SELECT json_group_array(json_object('key', key, 'status', status, 'tests', tests, 'files', test_files, 'fingerprint', fingerprint)) FROM (SELECT * FROM jobs ORDER BY key)"),
                ("edges", "SELECT json_group_array(json_array(job, needs)) FROM (SELECT * FROM dependencies ORDER BY job, needs)"),
                ("attempts", "SELECT json_group_array(json_object('id', id, 'status', status, 'branch', branch, 'base', base_sha, 'submitted', submitted_sha)) FROM (SELECT * FROM attempts ORDER BY id)"),
                ("history", "SELECT json_group_array(json_array(id, txn, tbl, pk, op, before, after)) FROM (SELECT * FROM history ORDER BY id)"),
                ("events", "SELECT json_group_array(json_array(id, txn, kind, job, text, data)) FROM (SELECT * FROM events ORDER BY id)"),
            ] {
                let text: String = c.query_row(query, [], |r| r.get(0))?;
                out.insert(name.into(), serde_json::from_str(&text).unwrap());
            }
            Ok(Value::Object(out))
        }).unwrap()
    }

    fn assert_replaced(&mut self) {
        let jobs = self.jobs();
        let old = jobs.iter().find(|j| j.key == "old-tests").unwrap();
        let new = jobs.iter().find(|j| j.key == "new-tests").unwrap();
        let implementation = jobs.iter().find(|j| j.key == "impl").unwrap();
        assert_eq!(old.status, "cancelled");
        assert!(old.cancel_reason.as_deref().unwrap().contains("new-tests"));
        assert_eq!(old.tests, Some(json!([self.old_selector])));
        assert_eq!(old.test_files, Some(json!(self.old_files)));
        assert_eq!(new.status, "queued");
        assert_eq!(new.kind, "test");
        assert_eq!(new.workflow, old.workflow);
        assert_eq!(new.title, old.title);
        assert_eq!(new.tier, old.tier);
        assert_eq!(new.resources, old.resources);
        assert_eq!(new.needs, vec!["docs"]);
        assert_eq!(new.tests, Some(json!(self.request.tests)));
        assert_eq!(new.test_files, Some(json!(self.request.test_files)));
        assert_eq!(
            new.fingerprint.as_deref(),
            Some(jobs::locked_fingerprint(new).as_str())
        );
        assert_eq!(implementation.status, "queued");
        assert_eq!(implementation.needs, vec!["new-tests"]);
        assert_eq!(
            self.row("SELECT branch FROM attempts WHERE id = 'old-tests-a1'"),
            "cpd/pay--old-tests-a1"
        );
        assert_eq!(
            self.row("SELECT status FROM attempts WHERE id = 'old-tests-a1'"),
            "ended"
        );
        assert_eq!(
            self.io.refs.get("cpd/pay--old-tests-a1"),
            Some(&self.commit)
        );
        let start = self
            .io
            .refs
            .get("new-tests:start")
            .expect("replacement starting commit");
        assert!(
            start == &self.commit
                || self
                    .io
                    .ancestry
                    .get(start)
                    .is_some_and(|ancestors| ancestors.contains(&self.commit)),
            "replacement starts from the original red commit, without recreating it"
        );
        let audit = self.snapshot();
        for field in ["history", "events"] {
            let text = audit[field].to_string();
            assert!(
                text.contains("old-tests")
                    && text.contains("new-tests")
                    && text.contains(&self.request.tests[0]),
                "{field} must record the mapping and correction"
            );
        }
    }
}

#[test]
fn locked_test_job_replacement_preserves_tdd_dependencies() {
    // Both reported selectors are Rust misses until qualified; their owners
    // differ, so preserving the old ownership is correct only for WebUI.
    for mut f in [Fixture::repetition(), Fixture::webui()] {
        assert!(!f.io.owners.contains_key(&f.old_selector));
        assert!(f.io.owners.contains_key(&f.request.tests[0]));
        f.run()
            .expect("the Lead can replace an attempted locked test Job");
        assert_eq!(f.io.stops, 1);
        f.assert_replaced();
    }

    // No source stop or graph write for bad selection/ownership, a reused key,
    // a Worker request, or a live/terminal downstream Job.
    for case in [
        "bare",
        "wrong-owner",
        "missing",
        "reused",
        "worker",
        "live-dependent",
        "merged-dependent",
        "cancelled-dependent",
        "empty-selectors",
        "not-test",
    ] {
        let mut f = Fixture::repetition();
        match case {
            "bare" => f.request.tests = vec![f.old_selector.clone()],
            "wrong-owner" => f.request.test_files = vec!["engine/src/".into()],
            "missing" => f.request.tests = vec!["pi/test/no.test.ts::missing".into()],
            "reused" => f.request.new_key = "impl".into(),
            "worker" => f.request.requester = "worker".into(),
            "live-dependent" => f.env.store.write("fixture", |tx| {
                tx.execute_batch(&format!("UPDATE jobs SET status = 'running' WHERE key = 'impl';
                  INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, driver_pid, started_at)
                    VALUES ('pay', 'impl-a1', 'impl', 1, 'S0', 'ladder', 'running', 'cpd/pay--impl-a1', '/fake/impl', 43, '{T}');"))?;
                Ok(())
            }).unwrap(),
            "merged-dependent" => f.env.store.write("fixture", |tx| {
                tx.execute("UPDATE jobs SET status = 'running' WHERE key = 'impl'", [])?;
                tx.execute("UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'impl'", [])?;
                Ok(())
            }).unwrap(),
            "cancelled-dependent" => f.env.store.write("fixture", |tx| {
                tx.execute("UPDATE jobs SET status = 'cancelled', cancel_reason = 'done', cancel_requested_at = ?1 WHERE key = 'impl'", [T])?;
                Ok(())
            }).unwrap(),
            "empty-selectors" => f.request.tests.clear(),
            "not-test" => f.request.source = "impl".into(),
            _ => unreachable!(),
        }
        let before = f.snapshot();
        let refs = f.io.refs.clone();
        assert!(f.run().is_err(), "{case} must refuse");
        assert_eq!(
            f.snapshot(),
            before,
            "{case} must not partially change rows, edges, history or events"
        );
        assert_eq!(f.io.refs, refs, "{case} must preserve refs");
        assert_eq!(f.io.stops, 0, "{case} must refuse before stopping");
    }

    // A stop failure is not database-atomic. The intent and fence survive,
    // but no replacement or redirect is visible. The identical request resumes;
    // a different request must not steal the intent.
    for stop in [
        Stop::Failed,
        Stop::TimedOut,
        Stop::RunnerStillLive,
        Stop::MergeInProgress,
        Stop::CommitFailed,
    ] {
        let mut f = Fixture::repetition();
        f.io.stop = stop;
        let before = f.snapshot();
        assert!(f.run().is_err(), "{stop:?} must not redirect");
        assert!(
            f.io.fenced,
            "the durable intent fences source retry/start/submit"
        );
        assert!(
            f.io.fence_checks > 0,
            "the stop seam observes the committed fence before external teardown"
        );
        let jobs = f.jobs();
        assert!(jobs.iter().all(|j| j.key != "new-tests"));
        assert_eq!(
            jobs.iter().find(|j| j.key == "impl").unwrap().needs,
            vec!["old-tests"]
        );
        assert_eq!(f.io.refs.get("cpd/pay--old-tests-a1"), Some(&f.commit));
        let after_failure = f.snapshot();
        assert_eq!(before["edges"], after_failure["edges"]);
        assert_eq!(
            before["jobs"].as_array().unwrap().len(),
            after_failure["jobs"].as_array().unwrap().len()
        );
        assert!(
            after_failure["history"]
                .as_array()
                .unwrap()
                .starts_with(before["history"].as_array().unwrap()),
            "pre-existing history is append-only"
        );
        let original = f.request.clone();
        f.request.new_key = "different-tests".into();
        assert!(
            f.run().is_err(),
            "a different request cannot replace a pending intent"
        );
        assert_eq!(f.snapshot(), after_failure);
        f.request = original;
        f.io.stop = Stop::Acknowledged;
        f.run().expect("same request resumes the durable intent");
        f.assert_replaced();
        let completed = f.snapshot();
        let refs = f.io.refs.clone();
        f.run()
            .expect("repeating a completed replacement is idempotent");
        assert_eq!(f.snapshot(), completed);
        assert_eq!(f.io.refs, refs);
    }

    // Natural completion during stop is safe only once the fake runner has
    // acknowledged teardown, and must not lose the existing commit.
    let mut f = Fixture::webui();
    f.io.stop = Stop::NaturalEnd;
    f.run()
        .expect("terminal natural completion with acknowledged teardown");
    f.assert_replaced();
}
