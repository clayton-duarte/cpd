//! Red tests of `job add`, `job edit --spec`, `job cancel` and `job retry` for the explore kinds `scout` and `research`
//! (Plan `explore-roles`, R2-R6; contract §2.2 "Job kinds", commands.md §5). Every command runs in process on a temp
//! `$CPD_HOME` with a fixed Catalog: no pi, no git, no process.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde_json::{Value, json};

use super::add::add_in;
use super::cancel::{Shape, cancel_in};
use super::edit::edit_in;
use super::retry::retry_in;
use super::{AddArgs, EditArgs, RetryArgs};
use crate::args::{PlanArg, Text, WorkflowArg};
use crate::config::{Catalog, Tier, TierModel};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::ids::{JobKey, PlanId, WorkflowKey};
use crate::output::Output;
use crate::plan::jobs;
use crate::plan::locked_fingerprint;

const T: &str = "2026-01-01T00:00:00.000Z";

struct Fixture {
    dir: tempfile::TempDir,
    env: Env,
    /// `CPD_ATTEMPT` is cleared for the fixture's life (a Worker pane cannot edit a Locked Job) and restored on drop.
    saved_attempt: Option<std::ffi::OsString>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // SAFETY: nextest gives each test a process of its own.
        unsafe {
            match &self.saved_attempt {
                Some(v) => std::env::set_var("CPD_ATTEMPT", v),
                None => std::env::remove_var("CPD_ATTEMPT"),
            }
        }
    }
}

/// Plan `pay` in `phase` with `prs` Workflows (`api`, `web`), in a temp `$CPD_HOME` whose Catalog has every effort 0-5 of every model.
/// `repo` is the Workflows' repo path.
fn fixture_at(phase: &str, workflows: &[&str], repo: Option<&std::path::Path>) -> Fixture {
    fixture_with(phase, workflows, repo, false)
}

/// [`fixture_at`], or, with `repo_is_temp_dir`, with the Workflows' repo the temp dir that holds `$CPD_HOME` (known once the dir exists,
/// and fixed at insert: the `prs_locked` trigger refuses a later change of `repo`).
fn fixture_with(
    phase: &str,
    workflows: &[&str],
    repo: Option<&std::path::Path>,
    repo_is_temp_dir: bool,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let dir_path = dir.path().to_owned();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"m1\"\n").unwrap();
    let owner = home.config().parent().unwrap().join("policy").join("acme");
    std::fs::create_dir_all(&owner).unwrap();
    std::fs::write(owner.join("owner.toml"), "").unwrap();
    let mut env = Env::open_home(home).unwrap();
    let model = |m: &str| TierModel {
        model: m.to_owned(),
        efforts: vec![0, 1, 2, 3, 4, 5],
    };
    env.catalog = Some(Catalog {
        pi_version: "1.0.0".to_owned(),
        o: model("o/m"),
        s: model("s/m"),
        h: model("h/m"),
        g: TierModel {
            model: "g/m".to_owned(),
            efforts: vec![],
        },
    });
    let locked = if matches!(phase, "grounding" | "plan") {
        "NULL".to_owned()
    } else {
        format!("'{T}'")
    };
    let done = if phase == "done" {
        format!("'{T}'")
    } else {
        "NULL".to_owned()
    };
    let repo = if repo_is_temp_dir {
        dir_path.display().to_string()
    } else {
        repo.map_or("/r".to_owned(), |p| p.display().to_string())
    };
    let waiting = matches!(phase, "grounding" | "plan");
    let status = if waiting { "waiting" } else { "dispatch" };
    let (worktree, started) = if waiting {
        ("NULL".to_owned(), "NULL".to_owned())
    } else {
        ("'/lw'".to_owned(), format!("'{T}'"))
    };
    let mut sql = format!(
        "INSERT INTO contexts (id, title, created_at, created_on, owner) VALUES ('ctx', 'Ctx', '{T}', 'm1', 'acme');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', '{T}', '{T}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'P', '{phase}', 'l-pay', '{T}', {locked}, {done});"
    );
    for workflow in workflows {
        sql.push_str(&format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
               VALUES ('pay', '{workflow}', '{repo}', 'main', '{status}', 'cpd/pay/{workflow}', {worktree}, 1, '{T}', {started});"
        ));
    }
    // SAFETY: nextest gives each test a process of its own; the fixture restores the variable on drop.
    let saved_attempt = std::env::var_os("CPD_ATTEMPT");
    unsafe { std::env::remove_var("CPD_ATTEMPT") };
    let mut f = Fixture {
        dir,
        env,
        saved_attempt,
    };
    f.sql(&sql);
    f
}

fn fixture(phase: &str) -> Fixture {
    fixture_at(phase, &["api"], None)
}

impl Fixture {
    fn sql(&mut self, sql: &str) {
        self.env
            .store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
    }

    /// A Spec file with `value` as its content.
    fn spec_file(&self, name: &str, value: &Value) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, value.to_string()).unwrap();
        path
    }

    fn explore_spec(&self, name: &str) -> PathBuf {
        self.spec_file(name, &explore_spec())
    }

    /// An existing queued Job of `kind` (explore: no Workflow, derived touches) with a true fingerprint, in its own transaction.
    fn job(&mut self, key: &str, kind: &str, status: &str, tier: &str, needs: &[&str]) {
        let explore = matches!(kind, "scout" | "research");
        let (workflow, touches) = if explore {
            ("NULL".to_owned(), format!("[\"run/pay/explore/{key}.md\"]"))
        } else {
            ("'api'".to_owned(), "[\"a\"]".to_owned())
        };
        // The true fingerprint of the Job, computed from a twin row that the rolled-back transaction takes away again.
        let mut fingerprint = String::new();
        let _ = self.env.store.write("fingerprint", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, created_txn, created_at)
                   VALUES ('pay', {workflow}, 'twin', 'J', '{tier}', 'g', 'c', '{touches}', '[\"b\"]', '[]', '{kind}', 'draft', 1, cpd_txn(), '{T}');"
            ))?;
            for need in needs {
                tx.execute_batch(&format!("INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'twin', '{need}');"))?;
            }
            let all = jobs::load(tx, &PlanId::parse("pay").unwrap())?;
            fingerprint = locked_fingerprint(all.iter().find(|j| j.key == "twin").unwrap());
            Err::<(), _>(CpdError::new(ErrorCode::Internal, "roll back the twin"))
        });
        // Without needs: inserted `queued`, then moved to `status`. With needs: inserted as a draft, so that its needs can be
        // written, then Locked as `plan lock` does.
        let cause = if status == "failed" {
            "'crashed'"
        } else {
            "NULL"
        };
        let first = if needs.is_empty() { "queued" } else { "draft" };
        let first_fingerprint = if needs.is_empty() {
            format!("'{fingerprint}'")
        } else {
            "NULL".to_owned()
        };
        self.sql(&format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, fingerprint, created_txn, created_at)
               VALUES ('pay', {workflow}, '{key}', 'J', '{tier}', 'g', 'c', '{touches}', '[\"b\"]', '[]', '{kind}', '{first}', 1, {first_fingerprint}, cpd_txn(), '{T}');"
        ));
        for need in needs {
            self.sql(&format!(
                "INSERT INTO dependencies (plan, job, needs) VALUES ('pay', '{key}', '{need}');"
            ));
        }
        self.sql(&format!("UPDATE jobs SET status = '{status}', fingerprint = '{fingerprint}', failed_cause = {cause} WHERE plan = 'pay' AND key = '{key}';"));
    }

    /// An ended failed Attempt of Job `key`.
    fn attempt(&mut self, key: &str, status: &str) {
        let (end, outcome, cause) = if status == "ended" {
            (format!("'{T}'"), "'failed'", "'crashed'")
        } else {
            ("NULL".to_owned(), "NULL", "NULL")
        };
        self.sql(&format!(
            "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, driver_pid, started_at, ended_at)
               VALUES ('pay', '{key}-a1', '{key}', 1, 'H2', 'ladder', '{status}', {outcome}, {cause}, 'cpd/pay--{key}-a1', '/w', 4242, '{T}', {end});"
        ));
    }

    // The test helper's eight arguments mirror the command's inputs verbatim.
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        key: &str,
        kind: &str,
        tier: &str,
        spec: &Path,
        workflow: Option<&str>,
        draft: bool,
        needs: &[&str],
    ) -> Result<Output, CpdError> {
        let args = AddArgs {
            key: JobKey::parse(key).unwrap(),
            title: Text::from_str("T").unwrap(),
            tier: Some(Tier::parse(tier).unwrap()),
            spec: Some(spec.to_path_buf()),
            needs: needs.iter().map(|n| JobKey::parse(n).unwrap()).collect(),
            draft,
            urgent: false,
            kind: Some(kind.to_owned()),
            workflow: WorkflowArg {
                workflow: workflow.map(|p| WorkflowKey::parse(p).unwrap()),
            },
            plan: PlanArg {
                plan: Some(PlanId::parse("pay").unwrap()),
            },
        };
        add_in(&mut self.env, args)
    }

    /// The one-field read of a Job's column.
    fn column(&mut self, key: &str, column: &str) -> Option<String> {
        self.env
            .store
            .read(|c| {
                Ok(c.query_row(
                    &format!("SELECT {column} FROM jobs WHERE plan = 'pay' AND key = ?1"),
                    [key],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }

    fn count(&mut self, sql: &str) -> i64 {
        self.env
            .store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    fn jobs(&mut self) -> i64 {
        self.count("SELECT count(*) FROM jobs")
    }

    fn events(&mut self, kind: &str) -> i64 {
        self.count(&format!(
            "SELECT count(*) FROM events WHERE kind = '{kind}'"
        ))
    }
}

/// The four-field Spec of an explore Job.
fn explore_spec() -> Value {
    json!({"goal": "Where is the retry ladder written?", "context": "Look in the engine.", "acceptance": ["names the file"], "out_of_scope": []})
}

/// The Spec of a `work` Job.
fn work_spec() -> Value {
    json!({"goal": "g", "context": "c", "touches": ["a"], "acceptance": ["b"], "out_of_scope": []})
}

fn fields(out: Result<Output, CpdError>) -> Value {
    match out.unwrap_or_else(|e| panic!("expected a success, got {e}")) {
        Output::Success(f) => Value::Object(f),
        Output::Owned => panic!("a success"),
    }
}

fn refused(out: Result<Output, CpdError>, code: ErrorCode) -> CpdError {
    match out {
        Ok(_) => panic!("expected {code}, got a success"),
        Err(e) => {
            assert_eq!(e.code(), code, "{e}");
            e
        }
    }
}

fn tier(t: &str) -> Tier {
    Tier::parse(t).unwrap()
}

#[test]
fn explore_add_scout_in_grounding_is_queued_with_derived_touches() {
    let mut f = fixture("grounding");
    let spec = f.explore_spec("s.json");
    let out = fields(f.add("s1", "scout", "H2", &spec, None, false, &[]));
    assert_eq!(out["job"]["status"], "queued");
    assert_eq!(out["job"]["pr"], Value::Null);
    assert_eq!(f.column("s1", "status").as_deref(), Some("queued"));
    assert_eq!(f.column("s1", "workflow"), None);
    assert!(
        f.column("s1", "fingerprint")
            .is_some_and(|fp| fp.len() == 64),
        "a Locked-Job fingerprint"
    );
    let touches: Value = serde_json::from_str(&f.column("s1", "touches").unwrap()).unwrap();
    assert_eq!(touches, json!(["run/pay/explore/s1.md"]));
    assert_eq!(f.events("job-added"), 1);

    let work = f.spec_file("w.json", &work_spec());
    let e = refused(
        f.add("w1", "work", "S2", &work, Some("api"), false, &[]),
        ErrorCode::IllegalPhase,
    );
    assert_eq!(e.exit().code(), 3);
    assert_eq!(f.jobs(), 1);
}

#[test]
fn explore_add_is_queued_in_plan_dispatch_and_deploy_phases() {
    for phase in ["plan", "dispatch", "deploy"] {
        let mut f = fixture(phase);
        let spec = f.explore_spec("s.json");
        let out = fields(f.add("s1", "scout", "H2", &spec, None, false, &[]));
        assert_eq!(out["job"]["status"], "queued", "phase {phase}");
        assert_eq!(
            out["job"]["appended"], true,
            "phase {phase}: appended as a Job added after the lock"
        );
        assert_eq!(
            f.column("s1", "status").as_deref(),
            Some("queued"),
            "phase {phase}"
        );
        assert!(f.column("s1", "fingerprint").is_some(), "phase {phase}");
        let research = f.explore_spec("r.json");
        let out = fields(f.add("r1", "research", "S2", &research, None, false, &[]));
        assert_eq!(out["job"]["status"], "queued", "phase {phase}");
    }
}

#[test]
fn explore_add_needs_no_pr_flag_when_the_plan_has_two_prs() {
    for phase in ["plan", "dispatch"] {
        let mut f = fixture_at(phase, &["api", "web"], None);
        let spec = f.explore_spec("s.json");
        let out = f.add("s1", "scout", "H2", &spec, None, false, &[]);
        if let Err(e) = &out {
            assert_ne!(
                e.code(),
                ErrorCode::WorkflowRequired,
                "an explore Job belongs to the Plan, not to a Workflow"
            );
        }
        assert_eq!(fields(out)["job"]["pr"], Value::Null, "phase {phase}");
        assert_eq!(f.column("s1", "workflow"), None);
    }
}

#[test]
fn explore_add_refuses_the_pr_flag() {
    for kind in ["scout", "research"] {
        let mut f = fixture_at("dispatch", &["api", "web"], None);
        let spec = f.explore_spec("s.json");
        let tier = if kind == "scout" { "H2" } else { "S2" };
        let e = refused(
            f.add("s1", kind, tier, &spec, Some("api"), false, &[]),
            ErrorCode::ExploreWorkflowForbidden,
        );
        assert_eq!(e.exit().code(), 3);
        assert_eq!(f.jobs(), 0, "nothing written");
        assert_eq!(f.events("job-added"), 0);
    }
}

#[test]
fn explore_add_refuses_draft() {
    for phase in ["plan", "dispatch"] {
        let mut f = fixture(phase);
        let spec = f.explore_spec("s.json");
        let e = refused(
            f.add("s1", "scout", "H2", &spec, None, true, &[]),
            ErrorCode::ExploreNoDraft,
        );
        assert_eq!(e.exit().code(), 3, "phase {phase}");
        assert_eq!(f.jobs(), 0, "nothing written");
    }
}

/// The Plan's `(phase, done_at)`.
fn plan_state(f: &mut Fixture) -> (String, Option<String>) {
    f.env
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT phase, done_at FROM plans WHERE id = 'pay'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap()
}

/// The `plan-phase` Events as `(from, to)`.
fn plan_phases(f: &mut Fixture) -> Vec<(String, String)> {
    f.env
        .store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT json_extract(data, '$.from'), json_extract(data, '$.to') FROM events WHERE kind = 'plan-phase' ORDER BY rowid",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap()
}

/// The Plan's `locked_at`.
fn locked_at(f: &mut Fixture) -> Option<String> {
    f.env
        .store
        .read(|c| {
            Ok(
                c.query_row("SELECT locked_at FROM plans WHERE id = 'pay'", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap()
}

/// Plan `pay` re-planned: phase `plan` with `locked_at` set (pro-d433, assumption A1).
/// It is reached the way a Lead reaches it: an explore add (`first`) on a dispatch Plan.
fn replanned() -> Fixture {
    let mut f = fixture("dispatch");
    let spec = f.explore_spec("first.json");
    fields(f.add("first", "scout", "H2", &spec, None, false, &[]));
    f
}

/// Kinds of the Events in order, of the kinds `plan-phase` and `job-added`.
fn phase_and_added(f: &mut Fixture) -> Vec<String> {
    f.env
        .store
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT kind FROM events WHERE kind IN ('plan-phase', 'job-added') ORDER BY id",
            )?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            Ok(rows.collect::<Result<Vec<String>, _>>()?)
        })
        .unwrap()
}

#[test]
fn explore_add_moves_a_dispatch_deploy_or_done_plan_back_to_phase_plan() {
    // pro-d429: a Plan past planning goes back to phase plan; pro-d433: locked_at is kept.
    for phase in ["dispatch", "deploy", "done"] {
        for (key, kind, tier, file) in [
            ("s1", "scout", "H2", "s.json"),
            ("r1", "research", "S2", "r.json"),
        ] {
            let mut f = fixture(phase);
            let locked = locked_at(&mut f);
            assert!(locked.is_some(), "{phase}");
            let spec = f.explore_spec(file);
            let out = fields(f.add(key, kind, tier, &spec, None, false, &[]));
            assert_eq!(out["job"]["status"], "queued", "{phase} {kind}");
            assert_eq!(f.column(key, "status").as_deref(), Some("queued"));
            assert_eq!(
                plan_state(&mut f),
                ("plan".to_owned(), None),
                "{phase} {kind}: back in plan, done_at NULL"
            );
            assert_eq!(locked_at(&mut f), locked, "{phase} {kind}: locked_at kept");
        }
    }
}

#[test]
fn explore_add_appends_one_plan_phase_event_before_job_added() {
    for phase in ["dispatch", "deploy", "done"] {
        let mut f = fixture(phase);
        let spec = f.explore_spec("s.json");
        fields(f.add("s1", "scout", "H2", &spec, None, false, &[]));
        assert_eq!(
            plan_phases(&mut f),
            vec![(phase.to_owned(), "plan".to_owned())],
            "{phase}"
        );
        assert_eq!(
            phase_and_added(&mut f),
            ["plan-phase", "job-added"],
            "{phase}"
        );
        // A rerun (already) appends no second plan-phase.
        let again = fields(f.add("s1", "scout", "H2", &spec, None, false, &[]));
        assert_eq!(again["already"], true, "{phase}");
        assert_eq!(plan_phases(&mut f).len(), 1, "{phase}");
        // A refused add writes nothing more.
        refused(
            f.add("s2", "scout", "S2", &spec, None, false, &[]),
            ErrorCode::TierNotAllowed,
        );
        assert_eq!(plan_phases(&mut f).len(), 1, "{phase}");
        assert_eq!(plan_state(&mut f).0, "plan", "{phase}");
    }
    // A refused add on a Plan not yet moved appends none and leaves the phase.
    for phase in ["dispatch", "deploy", "done"] {
        let mut f = fixture(phase);
        let spec = f.explore_spec("s.json");
        refused(
            f.add("s1", "scout", "S2", &spec, None, false, &[]),
            ErrorCode::TierNotAllowed,
        );
        assert!(plan_phases(&mut f).is_empty(), "{phase}");
        assert_eq!(plan_state(&mut f).0, phase);
    }
}

#[test]
fn explore_add_in_grounding_and_plan_leaves_the_phase() {
    for phase in ["grounding", "plan"] {
        let mut f = fixture(phase);
        let spec = f.explore_spec("s.json");
        fields(f.add("s1", "scout", "H2", &spec, None, false, &[]));
        assert_eq!(plan_state(&mut f), (phase.to_owned(), None), "{phase}");
        assert_eq!(locked_at(&mut f), None, "{phase}");
        assert!(plan_phases(&mut f).is_empty(), "{phase}");
        assert_eq!(f.events("job-added"), 1, "{phase}");
    }
    // A Plan that an earlier add moved back to phase plan is not moved again, by the same or a later add.
    let mut f = fixture("dispatch");
    let spec = f.explore_spec("s.json");
    fields(f.add("s1", "scout", "H2", &spec, None, false, &[]));
    fields(f.add("s2", "scout", "H2", &spec, None, false, &[]));
    assert_eq!(plan_state(&mut f), ("plan".to_owned(), None));
    assert_eq!(plan_phases(&mut f).len(), 1);
}

#[test]
fn explore_add_on_a_replanned_plan_changes_nothing_but_the_job() {
    let mut f = replanned();
    assert_eq!(plan_state(&mut f), ("plan".to_owned(), None));
    let locked = locked_at(&mut f);
    assert!(locked.is_some());
    let moved = plan_phases(&mut f);
    assert_eq!(moved, vec![("dispatch".to_owned(), "plan".to_owned())]);
    let spec = f.explore_spec("s.json");
    fields(f.add("s1", "scout", "H2", &spec, None, false, &[]));
    assert_eq!(plan_state(&mut f), ("plan".to_owned(), None));
    assert_eq!(locked_at(&mut f), locked);
    assert_eq!(
        plan_phases(&mut f),
        moved,
        "the second add appends no plan-phase"
    );
    assert_eq!(f.column("s1", "status").as_deref(), Some("queued"));
    assert_eq!(f.events("job-added"), 2);
}

#[test]
fn explore_add_refused_in_a_done_plan_leaves_it_done() {
    let mut f = fixture("done");
    let before = plan_state(&mut f);
    assert_eq!(before.0, "done");
    let spec = f.explore_spec("s.json");
    refused(
        f.add("s1", "scout", "S2", &spec, None, false, &[]),
        ErrorCode::TierNotAllowed,
    );
    refused(
        f.add("s2", "scout", "H2", &spec, None, true, &[]),
        ErrorCode::ExploreNoDraft,
    );
    assert_eq!(plan_state(&mut f), before, "still done, done_at unchanged");
    assert!(plan_phases(&mut f).is_empty());
    assert_eq!(f.jobs(), 0);
    // The refusals above are of the arguments, not of the phase: the same Plan accepts a valid add.
    fields(f.add("s3", "scout", "H2", &spec, None, false, &[]));
    assert_eq!(plan_state(&mut f).0, "plan");
}

#[test]
fn explore_add_rerun_in_a_reopened_plan_adds_no_second_plan_phase() {
    let mut f = fixture("done");
    let spec = f.explore_spec("s.json");
    let first = fields(f.add("s1", "scout", "H2", &spec, None, false, &[]));
    assert_ne!(first["already"], true);
    let second = fields(f.add("s1", "scout", "H2", &spec, None, false, &[]));
    assert_eq!(second["already"], true);
    assert_eq!(
        plan_phases(&mut f),
        vec![("done".to_owned(), "plan".to_owned())]
    );
    assert_eq!(f.jobs(), 1);
}

/// `tier_not_allowed` (exit 2) whose details name the kind and its range.
fn assert_out_of_range(e: CpdError, kind: &str, range: &str) {
    assert_eq!(e.code(), ErrorCode::TierNotAllowed, "{e}");
    assert_eq!(e.exit().code(), 2);
    let details = Value::Object(e.details().clone()).to_string();
    assert!(
        details.contains(kind) && details.contains(range),
        "details name {kind} and {range}: {details}"
    );
}

#[test]
fn explore_spec_has_exactly_four_fields() {
    let mut f = fixture("dispatch");
    for (field, value) in [
        ("touches", json!(["x"])),
        ("tests", json!(["t"])),
        ("test_files", json!(["f"])),
        ("resources", json!(["r"])),
    ] {
        let mut spec = explore_spec();
        spec[field] = value;
        let path = f.spec_file("s.json", &spec);
        let e = refused(
            f.add("s1", "scout", "H2", &path, None, false, &[]),
            ErrorCode::SpecInvalid,
        );
        assert_eq!(e.exit().code(), 2);
        assert_eq!(
            e.details()["path"],
            format!("$.{field}"),
            "the JSON path of the forbidden field"
        );
    }
    for field in ["goal", "context", "acceptance", "out_of_scope"] {
        let mut spec = explore_spec();
        spec.as_object_mut().unwrap().remove(field);
        let path = f.spec_file("s.json", &spec);
        let e = refused(
            f.add("s1", "research", "S2", &path, None, false, &[]),
            ErrorCode::SpecInvalid,
        );
        assert_eq!(e.details()["path"], format!("$.{field}"), "a missing field");
    }
    assert_eq!(f.jobs(), 0);
}

#[test]
fn explore_spec_context_may_name_machine_paths() {
    let mut f = fixture("dispatch");
    let mut spec = explore_spec();
    spec["context"] = json!(
        "Read /Users/someone/projects/other/src and ~/notes/ideas.md, and /opt/tools/README."
    );
    spec["acceptance"] = json!(["cites a line of /Users/someone/projects/other/src/main.rs"]);
    let path = f.spec_file("s.json", &spec);
    fields(f.add("s1", "scout", "H2", &path, None, false, &[]));
    fields(f.add("r1", "research", "S2", &path, None, false, &[]));
    assert_eq!(f.jobs(), 2);
}

#[test]
fn explore_spec_skips_the_touches_tree_check() {
    // The Workflows' repo is a directory that is no git tree: a tree check would read it and fail.
    let repo = tempfile::tempdir().unwrap();
    let mut f = fixture_at("dispatch", &["api"], Some(repo.path()));
    let spec = f.explore_spec("s.json");
    let out = f.add("s1", "scout", "H2", &spec, None, false, &[]);
    if let Err(e) = &out {
        assert_ne!(e.code(), ErrorCode::TouchesMissing);
    }
    fields(out);
    let touches: Value = serde_json::from_str(&f.column("s1", "touches").unwrap()).unwrap();
    assert_eq!(
        touches,
        json!(["run/pay/explore/s1.md"]),
        "the artefact, in no tree"
    );
}

#[test]
fn explore_needs_only_explore_jobs() {
    let mut f = fixture("dispatch");
    f.job("w1", "work", "queued", "S2", &[]);
    let spec = f.explore_spec("s.json");
    fields(f.add("a", "scout", "H2", &spec, None, false, &[]));
    fields(f.add("b", "scout", "H2", &spec, None, false, &["a"]));
    fields(f.add("c", "research", "S2", &spec, None, false, &["a", "b"]));

    let e = refused(
        f.add("d", "scout", "H2", &spec, None, false, &["w1"]),
        ErrorCode::ExploreNeedsMixed,
    );
    assert_eq!(e.exit().code(), 3);
    assert!(
        Value::Object(e.details().clone())
            .to_string()
            .contains("w1"),
        "details.jobs lists the Jobs on the other side"
    );
    let work = f.spec_file("w.json", &work_spec());
    refused(
        f.add("w2", "work", "S2", &work, Some("api"), false, &["a"]),
        ErrorCode::ExploreNeedsMixed,
    );
    assert_eq!(f.jobs(), 4, "w1, a, b, c: the refused ones wrote nothing");

    refused(
        f.add("loop", "scout", "H2", &spec, None, false, &["loop"]),
        ErrorCode::DependencyCycle,
    );
    assert_eq!(f.jobs(), 4);
}

#[test]
fn work_spec_may_cite_an_explore_artefact_path() {
    // The Workflows' repo is the directory that holds `$CPD_HOME`, so the artefact path lies inside a primary checkout by name.
    let mut f = fixture_with("dispatch", &["api"], None, true);
    let root = f.dir.path().display().to_string();
    let artefact = format!("{}/run/pay/explore/s1.md", f.env.home.root().display());
    let mut spec = work_spec();
    spec["context"] = json!(format!(
        "Read $CPD_HOME/run/pay/explore/s1.md first, or {artefact}."
    ));
    let path = f.spec_file("w.json", &spec);
    // Past the Spec checks the add may meet the (absent) git tree of the repo: only the Spec's verdict is pinned.
    if let Err(e) = f.add("w1", "work", "S2", &path, Some("api"), false, &[]) {
        assert_ne!(
            e.code(),
            ErrorCode::SpecInvalid,
            "the artefact of an explore Job is no primary checkout: {e}"
        );
    }

    // A primary-checkout path of the Plan's Workflow is still refused.
    let mut spec = work_spec();
    spec["context"] = json!(format!("Read {root}/src/lib.rs."));
    let path = f.spec_file("w2.json", &spec);
    let e = refused(
        f.add("w2", "work", "S2", &path, Some("api"), false, &[]),
        ErrorCode::SpecInvalid,
    );
    assert_eq!(e.details()["path"], "$.context");
}

#[test]
fn explore_spec_edit_while_queued_without_attempt() {
    let mut f = fixture("dispatch");
    f.job("e1", "scout", "queued", "H2", &[]);
    let before = f.column("e1", "fingerprint").unwrap();
    let mut spec = explore_spec();
    spec["goal"] = json!("A sharper question?");
    let path = f.spec_file("new.json", &spec);
    let edit = |spec: Option<PathBuf>, title: Option<&str>, tier: Option<&str>| EditArgs {
        key: JobKey::parse("e1").unwrap(),
        title: title.map(|t| Text::from_str(t).unwrap()),
        tier: tier.map(|t| Tier::parse(t).unwrap()),
        spec,
        needs: None,
        no_needs: false,
        workflow: None,
        kind: None,
        plan: PlanArg {
            plan: Some(PlanId::parse("pay").unwrap()),
        },
    };
    let out = fields(edit_in(&mut f.env, edit(Some(path), None, None)));
    assert_eq!(out["job"]["status"], "queued");
    assert_eq!(
        f.column("e1", "goal").as_deref(),
        Some("A sharper question?")
    );
    let after = f.column("e1", "fingerprint").unwrap();
    assert_ne!(after, before, "the fingerprint is recomputed");
    let plan = PlanId::parse("pay").unwrap();
    let job = f
        .env
        .store
        .read(|c| jobs::load(c, &plan))
        .unwrap()
        .into_iter()
        .find(|j| j.key == "e1")
        .unwrap();
    assert_eq!(after, locked_fingerprint(&job));
    let touches: Value = serde_json::from_str(&f.column("e1", "touches").unwrap()).unwrap();
    assert_eq!(
        touches,
        json!(["run/pay/explore/e1.md"]),
        "touches stays the derived artefact"
    );

    refused(
        edit_in(&mut f.env, edit(None, Some("New title"), None)),
        ErrorCode::JobLocked,
    );
    refused(
        edit_in(&mut f.env, edit(None, None, Some("H3"))),
        ErrorCode::JobLocked,
    );
    assert_eq!(f.column("e1", "title").as_deref(), Some("J"));
    assert_eq!(f.column("e1", "tier").as_deref(), Some("H2"));
}

#[test]
fn explore_cancel_ends_a_queued_job_and_flags_a_running_one() {
    let mut f = fixture("dispatch");
    let spec = f.explore_spec("s.json");
    fields(f.add("q", "scout", "H2", &spec, None, false, &[]));
    fields(f.add("dep", "research", "S2", &spec, None, false, &["q"]));
    fields(f.add("run", "scout", "H2", &spec, None, false, &[]));
    f.sql("UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'run';");
    f.attempt("run", "running");
    let plan = PlanId::parse("pay").unwrap();
    let cancel = |f: &mut Fixture, key: &str, reason: &str| {
        cancel_in(
            &mut f.env,
            &plan,
            JobKey::parse(key).unwrap(),
            reason,
            Shape::Keep,
        )
    };

    let out = fields(cancel(&mut f, "q", "not needed"));
    assert_eq!(out["status"], "cancelled");
    assert_eq!(f.column("q", "status").as_deref(), Some("cancelled"));
    assert_eq!(
        f.column("q", "cancel_reason").as_deref(),
        Some("not needed")
    );
    assert_eq!(
        f.column("dep", "status").as_deref(),
        Some("failed"),
        "an explore Job that needs it fails as any dependent does"
    );

    let out = fields(cancel(&mut f, "run", "stop"));
    assert_eq!(out["cancel_requested"], true);
    assert_eq!(f.column("run", "status").as_deref(), Some("running"));
    assert_eq!(f.column("run", "cancel_reason").as_deref(), Some("stop"));
    assert!(f.column("run", "cancel_requested_at").is_some());
    assert_eq!(f.events("job-cancel-requested"), 1);
}

#[test]
fn explore_retry_refuses_a_tier_outside_the_range() {
    let mut f = fixture("dispatch");
    f.job("s1", "scout", "failed", "H2", &[]);
    f.attempt("s1", "ended");
    let retry = |f: &mut Fixture, t: &str| {
        retry_in(
            &mut f.env,
            RetryArgs {
                key: JobKey::parse("s1").unwrap(),
                tier: tier(t),
                plan: PlanArg {
                    plan: Some(PlanId::parse("pay").unwrap()),
                },
            },
        )
    };
    assert_out_of_range(
        retry(&mut f, "S1").expect_err("S1 is outside the scout range (letter H)"),
        "scout",
        "S1",
    );
    assert_eq!(f.column("s1", "status").as_deref(), Some("failed"));
    let out = fields(retry(&mut f, "H3"));
    assert_eq!(out["status"], "queued");
    assert_eq!(f.column("s1", "status").as_deref(), Some("queued"));
    assert_eq!(f.column("s1", "retry_tier").as_deref(), Some("H3"));
}
