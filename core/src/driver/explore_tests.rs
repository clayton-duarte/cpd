//! Red tests of the driver side of an explore Attempt (Plan `explore-roles`, R7; contract §3.4 "An explore Job's Attempt",
//! §8.3 "Policy", §7.2): A2 (no worktree, the explore directory, the stale artefact), A3 (the Worker's environment and
//! Spec), the policy of its Session, and the clean-up that leaves it alone. In process on the fakes of `crate::ports`: no
//! git, no tmux, no process. The fixture is `attempt::explore_tests::Rig`.

use std::path::{Path, PathBuf};

use super::explore::prepare;
use super::runner;
use super::worker::render_spec;
use crate::attempt::explore_tests::{Rig, add_job, artefact, catalog, plan, run_sql, start_running, submit};
use crate::attempt::ledger::{self, Attempt as Target};
use crate::config::Config;
use crate::env::Env;
use crate::home::{CpdHome, Hook};
use crate::host::runner::Reply;
use crate::ids::{AttemptId, OwnerLogin, SessionId};
use crate::policy::PolicyKey;
use crate::ports::Ports;

const WORKER: &str = "9c1a2b3c-1111-4222-8333-444455556666";

fn explore_dir(home: &CpdHome) -> PathBuf {
    home.run_dir().join("pay").join("explore")
}

fn target(env: &mut Env, id: &AttemptId) -> Target {
    env.store.read(|conn| ledger::load(conn, &plan(), id)).unwrap()
}

#[test]
fn attempt_of_an_explore_job_has_no_worktree_and_runs_in_the_explore_directory() {
    let mut rig = Rig::new();
    rig.add_job("look", "scout", "H2");
    let id = rig.start("look");
    let dir = explore_dir(&rig.env.home);

    // The row the starter wrote: a branch no git branch has, the explore directory, no base.
    assert_eq!(rig.one::<String>("SELECT branch FROM attempts WHERE id = 'look-a1'"), "cpd/pay--look-a1");
    assert_eq!(rig.one::<String>("SELECT worktree FROM attempts WHERE id = 'look-a1'"), dir.display().to_string());
    assert_eq!(rig.one::<Option<String>>("SELECT base_sha FROM attempts WHERE id = 'look-a1'"), None);

    // A2: the directory is created if missing, and it is the Worker's cwd; no git and no pre-start run.
    let _ = std::fs::remove_dir_all(&dir);
    let attempt = target(&mut rig.env, &id);
    let home = rig.env.home.clone();
    let before = rig.fakes.runner.calls().len();
    let cwd = Ports::scope(&rig.fakes.ports, || prepare(&rig.fakes.ports, &home, &attempt));
    assert_eq!(cwd.as_ref().ok(), Some(&dir), "the cwd is the explore directory: {cwd:?}");
    assert!(dir.is_dir(), "the explore directory is created");
    let calls: Vec<String> = rig.fakes.runner.calls()[before..].iter().map(|c| format!("{} {:?}", c.program, c.args)).collect();
    assert!(calls.is_empty(), "no worktree, no branch, no pre-start: {calls:?}");
    assert!(!home.hook_log(&plan(), &id, Hook::PreStart).exists(), "pre-start left no log");
    assert_eq!(rig.one::<Option<String>>("SELECT base_sha FROM attempts WHERE id = 'look-a1'"), None, "the Attempt has no base_sha");
}

/// The launch env of the `durable` runner of `target`, over an adapter directory that holds the runner file.
fn worker_env(rig: &Rig, target: &Target) -> Vec<(String, String)> {
    let adapter = rig.tmp.path().join("adapter");
    std::fs::create_dir_all(adapter.join("durable-worker")).unwrap();
    std::fs::write(adapter.join("durable-worker").join("main.ts"), "").unwrap();
    std::fs::write(adapter.join("package.json"), "{}").unwrap();
    let text = format!("version = 1\nmachine = \"t\"\npi_adapter = \"{}\"\n", adapter.display());
    let config = Config::parse(&text, Path::new("config.toml")).unwrap().0;
    let session = SessionId::parse(WORKER).unwrap();
    let launch = runner::launch(&rig.env.home, &config, &catalog(), target, &session).unwrap();
    assert_eq!(launch.cwd.as_deref(), Some(target.worktree.as_path()));
    launch.env_set
}

#[test]
fn worker_env_of_an_explore_attempt_has_cpd_job_kind() {
    let mut rig = Rig::new();
    rig.add_job("look", "scout", "H2");
    rig.add_job("dig", "research", "S1");
    let (scout, research) = (rig.start("look"), rig.start("dig"));
    // A Worker of another kind: a `work` Job of a PR.
    rig.sql(
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at)
           VALUES ('pay', 'api', '/r', 'main', 'dispatch', 'cpd/pay/api', '/lw', 't', cpd_txn(), 't');
         INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
           VALUES ('pay', 'api', 'w', 'W', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, 'ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff', cpd_txn(), 't');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
           VALUES ('pay', 'w-a1', 'w', 1, 'S2', 'ladder', 'running', 'cpd/pay--w-a1', '/w', 't');",
    );
    let work = AttemptId::parse("w-a1").unwrap();

    let kind = |env: &[(String, String)]| env.iter().find(|(k, _)| k == "CPD_JOB_KIND").map(|(_, v)| v.clone());
    let t = target(&mut rig.env, &scout);
    assert_eq!(kind(&worker_env(&rig, &t)).as_deref(), Some("scout"));
    let t = target(&mut rig.env, &research);
    assert_eq!(kind(&worker_env(&rig, &t)).as_deref(), Some("research"));
    let t = target(&mut rig.env, &work);
    let env = worker_env(&rig, &t);
    assert_eq!(kind(&env), None, "a Worker of any other kind has no CPD_JOB_KIND");
    assert!(env.iter().any(|(k, v)| k == "CPD_ATTEMPT" && v == "w-a1"), "it is still a Worker's environment");
}

#[test]
fn a_new_attempt_removes_the_stale_artefact() {
    let mut rig = Rig::new();
    rig.add_job("look", "scout", "H2");
    let first = rig.start("look");
    let stale = rig.write_artefact("look", "# An earlier Attempt's findings\n");
    let other = rig.write_artefact("other", "# Another Job's findings\n");
    // Attempt 1 failed; attempt 2 is the retry the starter wrote.
    let dir = explore_dir(&rig.env.home);
    rig.sql(&format!(
        "UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = 't' WHERE plan = 'pay' AND id = '{first}';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
           VALUES ('pay', 'look-a2', 'look', 2, 'H3', 'ladder', 'starting', 'cpd/pay--look-a2', '{}', 't');",
        dir.display()
    ));
    let second = AttemptId::parse("look-a2").unwrap();
    let attempt = target(&mut rig.env, &second);
    let home = rig.env.home.clone();

    for pass in 1..=2 {
        let result = Ports::scope(&rig.fakes.ports, || prepare(&rig.fakes.ports, &home, &attempt));
        assert!(result.is_ok(), "pass {pass}: {result:?}");
        assert!(!stale.exists(), "pass {pass}: the stale artefact of the Job is deleted");
        assert!(other.exists(), "pass {pass}: another Job's artefact stays");
        assert_eq!(artefact(&home, "look"), stale);
    }
}

#[test]
fn worker_spec_of_an_explore_job_has_a_deliverable_section() {
    let mut rig = Rig::new();
    rig.add_job("look", "scout", "H2");
    let id = rig.start("look");
    let attempt = target(&mut rig.env, &id);
    let home = rig.env.home.clone();
    let job = rig
        .env
        .store
        .read(|conn| Ok(crate::plan::jobs::load(conn, &plan())?.into_iter().find(|j| j.key == "look").unwrap()))
        .unwrap();

    let path = rig.env.store.read(|conn| render_spec(conn, &home, &attempt, &job));
    assert!(path.is_ok(), "the Worker Spec of an explore Job renders: {path:?}");
    let text = std::fs::read_to_string(path.unwrap()).unwrap();

    let heading = text.lines().find(|l| l.starts_with('#') && l.contains("Deliverable"));
    assert!(heading.is_some(), "a Deliverable section:\n{text}");
    let section = &text[text.find(heading.unwrap()).unwrap()..];
    let artefact = artefact(&home, "look");
    assert!(section.contains(&artefact.display().to_string()), "the artefact path {}:\n{section}", artefact.display());
    assert!(section.contains("cpd attempt submit look-a1 --summary-file "), "the submit command:\n{section}");
    assert!(section.contains("1500"), "the 1500-character cap:\n{section}");
    assert!(section.contains("cpd context search"), "the read-only context search:\n{section}");
    assert!(section.contains("cpd context show"), "the read-only context show:\n{section}");
}

#[test]
fn policy_of_an_explore_worker_resolves_from_the_contexts_owner() {
    let mut rig = Rig::new();
    rig.add_job("look", "research", "S1");
    let id = rig.start("look");
    rig.sql(&format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
           VALUES ('{WORKER}', 'worker', 'ctx', 'pay', '{id}', 'pay look a1', '/', 'S1', 'working', 't', 't');"
    ));
    let worker = SessionId::parse(WORKER).unwrap();
    let owner = PolicyKey::Owner(OwnerLogin::parse("acme").unwrap());

    let found = rig.env.store.read(|conn| crate::policy::show::key_of_session(conn, &worker));
    assert_eq!(found.as_ref().ok().map(|(_, key)| key), Some(&owner), "the Context owner's policy, no PR and no error: {found:?}");
    assert_eq!(found.as_ref().ok().map(|(role, _)| *role), Some("worker"));
    // As a Lead's does.
    let lead_key = rig.env.store.read(|conn| {
        let id: String = conn.query_row("SELECT id FROM sessions WHERE role = 'lead'", [], |r| r.get(0))?;
        Ok(id)
    });
    assert!(lead_key.is_ok());
}

#[test]
fn clean_up_leaves_explore_attempts_and_their_directory_alone() {
    use crate::janitor_tests::Rig as Janitor;

    let mut rig = Janitor::new();
    rig.workflow("api", "open");
    let mut env = Env::open_home_with(&rig.fakes.ports, rig.home.clone()).unwrap();
    add_job(&mut env, "look", "scout", "H2");
    let id = start_running(&mut env, &rig.fakes, "look");
    let artefact = artefact(&env.home, "look");
    std::fs::create_dir_all(artefact.parent().unwrap()).unwrap();
    std::fs::write(&artefact, "# Findings\n").unwrap();
    let summary = rig.tmp.path().join("summary.md");
    std::fs::write(&summary, "Found it.").unwrap();
    // The explore Attempt of a merged explore Job, ended the way `attempt submit` ends it.
    let ended = submit(&mut env, &rig.fakes, &id, Some(&summary));
    assert!(ended.is_ok(), "the explore Attempt ends merged: {ended:?}");
    assert_eq!(env.store.read(|c| Ok(c.query_row("SELECT status FROM jobs WHERE key = 'look'", [], |r| r.get::<_, String>(0))?)).unwrap(), "merged");

    let writes = |env: &mut Env| -> (i64, i64) {
        env.store.read(|c| Ok((c.query_row("SELECT max(id) FROM history", [], |r| r.get(0))?, c.query_row("SELECT max(id) FROM txns", [], |r| r.get(0))?))).unwrap()
    };
    let before = writes(&mut env);
    let calls = rig.fakes.runner.calls().len();
    let dir = explore_dir(&env.home);

    let _ = (*rig.fakes.runner).clone().on("gh", |c| c.args.starts_with(&["pr".to_owned(), "list".to_owned()]), Reply::ok("[]"));
    let _ = rig.pass();
    let args = crate::prune::Args { apply: true, drop: Vec::new(), repos: vec![rig.repo.dir().to_path_buf()] };
    let _ = crate::prune::run(&rig.fakes.ports, &rig.home, &args);

    assert!(artefact.is_file(), "the artefact stays");
    assert!(dir.is_dir(), "the explore directory stays");
    assert_eq!(writes(&mut env), before, "no row is touched");
    let removals: Vec<String> = rig.fakes.runner.calls()[calls..]
        .iter()
        .filter(|c| c.program.ends_with("git"))
        .map(|c| c.args.join(" "))
        .filter(|a| a.contains("remove") || a.contains("-D") || a.contains("prune"))
        .collect();
    assert!(removals.is_empty(), "no worktree or branch is removed: {removals:?}");
}
