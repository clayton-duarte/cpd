//! Fresh Lead admission, using disposable stores and in-process external-service fakes.
use std::sync::{Mutex, MutexGuard};

use serde_json::json;

use super::{NewArgs, new};
use crate::config::WorkerRunner;
use crate::env::{Env, with_fixture_environment};
use crate::error::ErrorCode;
use crate::home::CpdHome;
use crate::ids::{ContextId, PlanId, SessionId};
use crate::inbox::{
    cmd::{DismissArgs, claim_in},
    model::{Condition, Kind, Urgency, reconcile},
    team::{GhPr, GhState, Lookup, Review},
};
use crate::ports::{Fakes, Ports};

static ENV_LOCK: Mutex<()> = Mutex::new(());
struct World {
    _lock: MutexGuard<'static, ()>,
    saved_session: Option<std::ffi::OsString>,
    saved_attempt: Option<std::ffi::OsString>,
    dir: tempfile::TempDir,
    home: CpdHome,
    fakes: Fakes,
}
impl Drop for World {
    fn drop(&mut self) {
        // SAFETY: these tests serialize their environment changes and restore both variables.
        unsafe {
            match &self.saved_session {
                Some(v) => std::env::set_var("CPD_SESSION", v),
                None => std::env::remove_var("CPD_SESSION"),
            }
            match &self.saved_attempt {
                Some(v) => std::env::set_var("CPD_ATTEMPT", v),
                None => std::env::remove_var("CPD_ATTEMPT"),
            }
        }
    }
}
impl World {
    fn new(config: &str) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
        std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
        std::fs::write(
            home.config(),
            format!("version = 1\nmachine = \"m1\"\nlead_tier = \"S4\"\n{config}\n"),
        )
        .unwrap();
        let fakes = Fakes::new();
        let saved_session = std::env::var_os("CPD_SESSION");
        let saved_attempt = std::env::var_os("CPD_ATTEMPT");
        // SAFETY: serialized and restored by Drop.
        unsafe {
            std::env::remove_var("CPD_SESSION");
            std::env::remove_var("CPD_ATTEMPT");
        }
        let mut env = Env::open_home_with(&fakes.ports, home.clone()).unwrap();
        env.store.write("fixture", |tx| {
            tx.execute_batch("INSERT INTO contexts (id,title,owner,created_at,created_on) VALUES ('ctx','Ctx','acme','t','m1'),('codereview','codereview','clayton-duarte','t','m1');
                INSERT INTO sessions (id,role,context,name,cwd,tier,state,state_at,created_at) VALUES ('mgr','manager','ctx','Manager','/','S4','idle','t','t'),('reviewmgr','manager','codereview','Review Manager','/','S4','idle','t','t');")?;
            Ok(())
        }).unwrap();
        Self {
            _lock: lock,
            saved_session,
            saved_attempt,
            dir,
            home,
            fakes,
        }
    }
    fn env(&self) -> Env {
        Env::open_home_with(&self.fakes.ports, self.home.clone()).unwrap()
    }
    fn call(&self, args: NewArgs) -> Result<crate::output::Output, crate::error::CpdError> {
        with_fixture_environment(&self.home, self.dir.path(), || {
            Ports::scope(&self.fakes.ports, || new::new(args))
        })
    }
    fn query(&self, sql: &str) -> i64 {
        self.env()
            .store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }
    fn runner(&self, plan: &str) -> String {
        self.env().store.read(|c| Ok(c.query_row("SELECT s.runner FROM plans p JOIN sessions s ON s.id=p.lead_session WHERE p.id=?1", [plan], |r| r.get(0))?)).unwrap()
    }
    fn no_launch(&self) {
        assert!(self.fakes.runner.calls().is_empty());
        assert!(self.fakes.spawn.launches().is_empty());
    }
}
fn args(id: &str, runner: Option<WorkerRunner>, shaping: bool) -> NewArgs {
    NewArgs {
        id: PlanId::parse(id).unwrap(),
        context: ContextId::parse("ctx").unwrap(),
        title: "Test plan".parse().unwrap(),
        jira: None,
        brief_file: None,
        tier: None,
        by_user: true,
        runner,
        shaping,
    }
}
fn assert_pair(w: &World, plan: &str, phase: &str) {
    assert_eq!(w.runner(plan), "durable");
    assert_eq!(w.query(&format!("SELECT count(*) FROM plans p JOIN sessions s ON p.lead_session=s.id AND s.plan=p.id WHERE p.id='{plan}' AND p.phase='{phase}' AND s.role='lead' AND s.tmux_pane IS NULL")), 1);
}
#[test]
fn fresh_lead_defaults_to_durable() {
    for config in ["", "lead_runner = \"pi\""] {
        let w = World::new(config);
        w.call(args("ordinary", None, false))
            .expect("ordinary creation");
        assert_pair(&w, "ordinary", "grounding");
        // SAFETY: serialized and restored by World.
        unsafe { std::env::set_var("CPD_SESSION", "mgr") };
        w.call(args("shaping", None, true))
            .expect("manager shaping creation");
        assert_pair(&w, "shaping", "shaping");
        w.no_launch();
    }
}
#[test]
fn pi_lead_refused_before_writes() {
    let w = World::new("");
    let events = w.query("SELECT count(*) FROM events");
    let err = w
        .call(args("ordinary", Some(WorkerRunner::Pi), false))
        .expect_err("explicit pi must be refused");
    assert_eq!(err.code(), ErrorCode::Usage);
    assert!(err.message().to_lowercase().contains("durable"));
    assert_eq!(w.query("SELECT count(*) FROM plans"), 0);
    assert_eq!(
        w.query("SELECT count(*) FROM sessions WHERE role='lead'"),
        0
    );
    assert_eq!(w.query("SELECT count(*) FROM context_records"), 0);
    assert_eq!(w.query("SELECT count(*) FROM events"), events);
    assert!(
        !w.home
            .lead_message(&PlanId::parse("ordinary").unwrap())
            .exists()
    );
    w.no_launch();
    let mut env = w.env();
    let id = PlanId::parse("direct").unwrap();
    let context = ContextId::parse("ctx").unwrap();
    let session = SessionId::new_v4();
    let creation = new::Creation {
        id: &id,
        context: &context,
        title: "direct",
        jira: None,
        name: "ctx:direct",
        cwd: "/",
        tier: "S4",
        runner: WorkerRunner::Pi,
        by_user: true,
        session: &session,
    };
    let error = env
        .store
        .write("direct", |tx| new::insert(tx, &creation))
        .expect_err("shared insert must guard pi");
    assert_eq!(error.code(), ErrorCode::Usage);
    assert_eq!(w.query("SELECT count(*) FROM plans"), 0);
    assert_eq!(
        w.query("SELECT count(*) FROM sessions WHERE role='lead'"),
        0
    );
    assert_eq!(w.query("SELECT count(*) FROM events"), events);
}
fn seed_inbox(w: &World) {
    let mut env = w.env();
    let subject = "pr:acme/pay#5";
    let oid = "5555555555555555555555555555555555555555";
    let condition = Condition {
        kind: Kind::ReviewRequested,
        urgency: Urgency::Blue,
        subject: subject.into(),
        label: "Review pay".into(),
        url: Some("https://github.com/acme/pay/pull/5".into()),
        context: None,
        plan: None,
        job: None,
        session: None,
        record: None,
        event: None,
        opened_at: None,
        head_oid: Some(oid.into()),
        reviewed_oid: None,
    };
    reconcile(&mut env.store, &[condition], "2026-01-01T00:00:00.000Z").unwrap();
    crate::inbox::team::seed_cache(
        &mut env.store,
        "C02JU6C2Z1D",
        &[
            json!({"repo":"acme/pay","number":5,"ts":1767225600.0,"ts_raw":"1767225600.000100","author":"Ryan","primary":true,"reactions":[],"claim":null,"eyes_me":null,"thread_tags_me":false}),
        ],
    );
    let pr = GhPr {
        title: "Add retries".into(),
        draft: false,
        state: GhState::Open,
        author: "ryan".into(),
        decision: Some("CHANGES_REQUESTED".into()),
        reviews: vec![Review {
            author: "clayton-duarte".into(),
            state: "CHANGES_REQUESTED".into(),
            commit_oid: Some("1111111111111111111111111111111111111111".into()),
        }],
        head_oid: Some(oid.into()),
        checks_state: Some("SUCCESS".into()),
    };
    let lookup = Lookup {
        me: "clayton-duarte".into(),
        workflows: [(("acme/pay".into(), 5), Some(pr))].into(),
    };
    crate::inbox::swr_fixture::lookup(&mut env.store, &lookup).unwrap();
}
#[test]
fn inbox_claim_refuses_pi_before_claim() {
    // The public claim has no runner flag: even a Pi-configured default must resolve durable.
    // Its shared insert must independently reject a Pi Creation (tested below).
    let w = World::new("lead_runner = \"pi\"");
    seed_inbox(&w);
    let out = Ports::scope(&w.fakes.ports, || {
        claim_in(&w.fakes.ports, w.env(), DismissArgs { id: 1 })
    });
    out.expect("a configured Pi default must not cause a Pi Lead");
    assert_pair(&w, "r-pay-5", "grounding");
    assert_eq!(
        w.query("SELECT count(*) FROM inbox_items WHERE id=1 AND claimed_at IS NOT NULL"),
        1
    );
    w.no_launch();
    let mut env = w.env();
    let id = PlanId::parse("guard").unwrap();
    let context = ContextId::parse("codereview").unwrap();
    let session = SessionId::new_v4();
    let creation = new::Creation {
        id: &id,
        context: &context,
        title: "guard",
        jira: None,
        name: "guard",
        cwd: "/",
        tier: "S4",
        runner: WorkerRunner::Pi,
        by_user: false,
        session: &session,
    };
    let events = w.query("SELECT count(*) FROM events");
    let err = env
        .store
        .write("direct claim insert", |tx| new::insert(tx, &creation))
        .expect_err("pi insertion rejected");
    assert_eq!(err.code(), ErrorCode::Usage);
    assert_eq!(w.query("SELECT count(*) FROM events"), events);
    assert_eq!(w.query("SELECT count(*) FROM plans WHERE id='guard'"), 0);
}
#[test]
fn existing_plan_and_pairing_are_preserved() {
    let w = World::new("lead_runner = \"pi\"");
    w.call(args("paired", None, false)).expect("create pair");
    assert_pair(&w, "paired", "grounding");
    let events = w.query("SELECT count(*) FROM events");
    w.call(args("paired", None, false))
        .expect("idempotent reuse");
    assert_pair(&w, "paired", "grounding");
    assert_eq!(w.query("SELECT count(*) FROM plans WHERE id='paired'"), 1);
    assert_eq!(
        w.query("SELECT count(*) FROM sessions WHERE role='lead' AND plan='paired'"),
        1
    );
    assert_eq!(w.query("SELECT count(*) FROM events"), events);
    w.no_launch();
}
