//! Job selection persistence through the production writer and reader.
use crate::env::Env;
use crate::home::CpdHome;
use crate::ids::PlanId;
use crate::job::{
    AddArgs,
    add::{add_in, add_with_selection_in},
};
use crate::plan::jobs::{self, JobSelection};
use crate::policy::RoleSelection;
use clap::Parser;

#[derive(Parser)]
struct Args {
    #[command(flatten)]
    add: AddArgs,
}

fn fixture() -> (tempfile::TempDir, Env) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"m1\"\n").unwrap();
    let fakes = crate::ports::Fakes::new();
    let mut env = Env::open_home_with(&fakes.ports, home).unwrap();
    env.store.write("fixture", |tx| {
        tx.execute_batch("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
            INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at) VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
            INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pay', 'ctx', 'P', 'plan', 'l-pay', 't');
            INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at) VALUES ('pay', 'api', '/r', 'main', 'waiting', 'pay/api', cpd_txn(), 't');")?;
        Ok(())
    }).unwrap();
    (dir, env)
}

fn args(key: &str, spec: &std::path::Path) -> AddArgs {
    Args::parse_from([
        "test",
        key,
        "--title",
        "Title",
        "--plan",
        "pay",
        "--workflow",
        "api",
        "--draft",
        "--waiting-on",
        "lead",
        "--spec",
        spec.to_str().unwrap(),
    ])
    .add
}

fn load(env: &mut Env) -> Vec<jobs::Job> {
    env.store
        .read(|conn| jobs::load(conn, &PlanId::parse("pay").unwrap()))
        .unwrap()
}

#[test]
fn job_direct_selection_roundtrip() {
    let (_dir, mut env) = fixture();
    let policy = env.home.root().join("policy/acme");
    std::fs::create_dir_all(&policy).unwrap();
    std::fs::write(policy.join("owner.toml"), "").unwrap();
    env.store
        .write("fixture", |tx| {
            tx.execute("UPDATE contexts SET owner = 'acme' WHERE id = 'ctx'", [])?;
            Ok(())
        })
        .unwrap();
    let model = |name: &str| crate::config::TierModel {
        model: name.to_owned(),
        efforts: vec![1, 2, 3],
    };
    env.catalog = Some(crate::config::Catalog {
        j: model("j/m"),
        pi_version: "1.0.0".into(),
        o: model("o/m"),
        s: model("s/m"),
        h: model("h/m"),
        g: model("g/m"),
    });
    let legacy_spec = _dir.path().join("legacy.json");
    std::fs::write(&legacy_spec, r#"{"goal":"g","context":"c","touches":["src/a.rs"],"acceptance":["works"],"out_of_scope":[]}"#).unwrap();
    let legacy = Args::parse_from([
        "test",
        "legacy",
        "--title",
        "Title",
        "--plan",
        "pay",
        "--workflow",
        "api",
        "--draft",
        "--waiting-on",
        "lead",
        "--tier",
        "H3",
        "--spec",
        legacy_spec.to_str().unwrap(),
    ])
    .add;
    add_in(&mut env, legacy).expect("legacy writer accepts tier and valid spec");
    assert_eq!(
        load(&mut env)[0].selection(),
        JobSelection::LegacyTier("H3".into())
    );
    let selection = JobSelection::Direct(RoleSelection {
        model: "openai/example".into(),
        effort: 2,
    });
    let spec = _dir.path().join("direct-spec.json");
    std::fs::write(&spec, r#"{"goal":"g","context":"c","touches":["src/a.rs"],"acceptance":["works"],"out_of_scope":[]}"#).unwrap();
    add_with_selection_in(
        &mut env,
        args("direct", &spec),
        Some(match selection.clone() {
            JobSelection::Direct(direct) => direct,
            _ => unreachable!(),
        }),
    )
    .expect("direct writer accepts a complete selection");
    let jobs = load(&mut env);
    assert_eq!(jobs.len(), 2);
    assert_eq!(
        jobs.iter().find(|j| j.key == "direct").unwrap().selection(),
        selection
    );
    let stored: (Option<String>, Option<String>, Option<i64>) = env
        .store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT tier, selected_model, selected_effort FROM jobs WHERE key = 'direct'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!(stored, (None, Some("openai/example".into()), Some(2)));
}
