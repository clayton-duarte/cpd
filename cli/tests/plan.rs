//! Plans (contract §2.3, §3.1; commands.md §4, §15.5 and §19 amendments 6
//! and 8), run as the built binary against a temporary `$CPD_HOME`, temporary
//! git repos and a private tmux server, with a fake `cpd` on `PATH` for the
//! panes (`session run`) and for `watch --ensure`.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{CPD, Run, TestEnv};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::{PlanId, ReviewId};
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

// ---------- fixtures ----------

fn home(env: &TestEnv) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

fn store(env: &TestEnv) -> Store {
    Store::open(&home(env), 5000).unwrap()
}

fn pid(id: &str) -> PlanId {
    PlanId::parse(id).unwrap()
}

/// `config.toml` on this env's private tmux server.
fn write_config(env: &TestEnv) {
    let config = format!("version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket());
    fs::write(env.cpd_home().join("config.toml"), config).unwrap();
}

/// Where the fake `cpd session run <id>` writes its environment.
fn pane_env_file(env: &TestEnv, session: &str) -> PathBuf {
    env.root().join(format!("pane-{session}.env"))
}

/// A fake `cpd` on `PATH`: `session run <id>` records its environment and
/// sleeps (the pane's program); `watch --ensure` runs `watch`; everything
/// else is the built binary.
fn fake_cpd(env: &TestEnv, watch: &str) {
    env.fake(
        "cpd",
        &format!(
            "case \"$1 $2\" in\n\
             \"session run\") env > '{root}/pane-'\"$3\".env; exec sleep 600;;\n\
             \"watch --ensure\") {watch};;\n\
             esac\nexec '{CPD}' \"$@\"",
            root = env.root().display()
        ),
    );
}

const WATCH_OK: &str = "echo '{\"ok\": true, \"started\": true, \"pane\": \"%9\"}'; exit 0";
const WATCH_FAILS: &str =
    "echo '{\"ok\": false, \"error\": {\"code\": \"watch_start_failed\", \"message\": \"No lock.\", \"details\": {}}}'; exit 1";

/// The env with its config, a fake `cpd`, Context `ctx` and the repo `pay`.
fn setup(env: &TestEnv) -> PathBuf {
    write_config(env);
    fake_cpd(env, WATCH_OK);
    env.cpd(&["context", "new", "ctx", "--title", "Payments"]).ok();
    env.git_repo("pay")
}

fn cpd_with(env: &TestEnv, vars: &[(&str, &str)], args: &[&str]) -> Run {
    let mut cmd = env.command(CPD);
    for (k, v) in vars {
        cmd.env(k, v);
    }
    let out = cmd.args(args).output().unwrap();
    Run {
        exit: out.status.code().unwrap(),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn refused(run: &Run, code: &str, exit: i32) -> Value {
    let error = run.error();
    assert_eq!(error["code"], code, "{run:?}");
    assert_eq!(run.exit, exit, "{run:?}");
    error
}

fn query<T: rusqlite::types::FromSql>(env: &TestEnv, sql: &str) -> T {
    store(env).read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
}

fn events(env: &TestEnv, kind: &str) -> Vec<Value> {
    env.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
}

fn new_plan(env: &TestEnv, id: &str, repo: &Path) -> Value {
    env.cpd(&["plan", "new", id, "--context", "ctx", "--repo", repo.to_str().unwrap(), "--title", &format!("Plan {id}")]).ok()
}

fn show(env: &TestEnv, plan: &str) -> Value {
    env.cpd(&["plan", "show", "--plan", plan]).ok()
}

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The environment the fake `session run` of `session` was started with.
fn pane_env(env: &TestEnv, session: &str) -> String {
    let path = pane_env_file(env, session);
    wait_until("the pane's session run", || path.exists() && fs::read_to_string(&path).is_ok_and(|t| t.contains("CPD_SESSION")));
    fs::read_to_string(path).unwrap()
}

/// The panes of the private server: `(pane, window name, @cpd_session)`.
fn panes(env: &TestEnv) -> Vec<(String, String, String)> {
    let out = env.tmux(&["list-panes", "-a", "-F", "#{pane_id}\t#{window_name}\t#{@cpd_session}"]);
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f[0].to_owned(), f[1].to_owned(), f[2].to_owned())
        })
        .collect()
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

/// A draft Job `key` of `plan` needing `needs`, written as `job add` would.
fn add_job(env: &TestEnv, plan: &str, key: &str, needs: &[&str]) {
    store(env)
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, created_txn, created_at)
                     VALUES ('{plan}', (SELECT key FROM prs WHERE plan = '{plan}'), '{key}', 'Job {key}', 'S2', 'Goal of {key}.', 'Context of {key}.', '[\"src/{key}.rs\"]',
                             '[\"cargo test\"]', '[]', 'draft', 0, {id}, 't');",
                    id = tx.id()
                ),
            )?;
            for need in needs {
                exec(tx, &format!("INSERT INTO dependencies (plan, job, needs) VALUES ('{plan}', '{key}', '{need}');"))?;
            }
            Ok(())
        })
        .unwrap();
}

fn edit_goal(env: &TestEnv, plan: &str, key: &str, goal: &str) {
    store(env)
        .write("fixture", |tx| exec(tx, &format!("UPDATE jobs SET goal = '{goal}' WHERE plan = '{plan}' AND key = '{key}';")))
        .unwrap();
}

/// A Plan `id` in phase `plan` with draft Jobs `a` and `b` (needs `a`).
fn planned(env: &TestEnv, repo: &Path, id: &str) {
    new_plan(env, id, repo);
    env.cpd(&["plan", "phase", "plan", "--plan", id]).ok();
    add_job(env, id, "a", &[]);
    add_job(env, id, "b", &["a"]);
}

fn sha256(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

// ---------- plan new ----------

#[test]
fn new_registers_the_plan_and_its_lead_and_starts_it_in_a_parked_window() {
    let env = TestEnv::new();
    let repo = setup(&env);
    env.cpd(&["context", "decide", "One engine per machine.", "--context", "ctx"]).ok();
    let brief = env.root().join("brief.md");
    fs::write(&brief, "Retry the failed payouts.\n").unwrap();

    let vars = [("PLANNOTATOR_PORT", "19432-19439"), ("PLANNOTATOR_REMOTE", "1")];
    let repo_arg = repo.to_str().unwrap();
    let args = ["plan", "new", "pay", "--context", "ctx", "--repo", repo_arg, "--title", "Retry payouts", "--jira", "PAY-1", "--brief-file", brief.to_str().unwrap()];
    let out = cpd_with(&env, &vars, &args).ok();
    let real = repo.canonicalize().unwrap().display().to_string();
    let session = out["plan"]["lead_session"].as_str().unwrap().to_owned();
    assert_eq!(
        out["plan"],
        json!({"id": "pay", "context": "ctx", "title": "Retry payouts", "jira": "PAY-1", "repo": real, "base": "main",
               "lead_branch": "cpd/pay", "phase": "grounding", "lead_session": session})
    );
    assert_eq!((&out["lead"]["session"], &out["lead"]["name"], &out["lead"]["forked_from"]), (&json!(session), &json!("pay"), &Value::Null));
    assert_eq!(out["record"], "m1-p1");
    let (pane, window) = (out["lead"]["pane"].as_str().unwrap(), out["lead"]["window"].as_str().unwrap());

    // The Lead: stopped, in its parked window, with its pane recorded.
    let lead = env.cpd(&["session", "show", &session]).ok()["session"].clone();
    assert_eq!((&lead["role"], &lead["plan"], &lead["state"], &lead["tier"]), (&json!("lead"), &json!("pay"), &json!("stopped"), &json!("O3")));
    assert_eq!((&lead["cwd"], &lead["tmux_pane"], &lead["place"]), (&json!(real), &json!(pane), &json!("parked")));
    assert_eq!(panes(&env), vec![(pane.to_owned(), "park-pay".to_owned(), session.clone())], "the cockpit is created with the Lead's window");

    // The pane runs `cpd session run <id>` with the Plan's and plannotator's variables (§15.5).
    let pane_vars = pane_env(&env, &session);
    let pane_vars: Vec<&str> = pane_vars.lines().filter(|l| l.starts_with("CPD_") || l.starts_with("PLANNOTATOR_")).collect();
    for var in [
        format!("CPD_HOME={}", env.cpd_home().display()),
        format!("CPD_SESSION={session}"),
        "CPD_PLAN=pay".to_owned(),
        "PLANNOTATOR_PORT=19432-19439".to_owned(),
        "PLANNOTATOR_REMOTE=1".to_owned(),
    ] {
        assert!(pane_vars.contains(&var.as_str()), "{var} not in {pane_vars:?}");
    }
    let run = env.calls("cpd").into_iter().find(|c| c[0] == "session").unwrap();
    assert_eq!(run, ["session", "run", session.as_str()]);

    // Events: plan-created and session-registered together, then the record, then the window.
    let created = &events(&env, "plan-created")[0];
    assert_eq!(created["data"], json!({"title": "Retry payouts", "repo": real, "base": "main", "jira": "PAY-1", "lead_session": session, "forked_from": null}));
    let registered = events(&env, "session-registered");
    assert_eq!(registered.len(), 1);
    assert_eq!((&registered[0]["plan"], &registered[0]["session"]), (&json!("pay"), &json!(session)));
    assert_eq!(registered[0]["data"], json!({"role": "lead", "name": "pay", "forked_from": null, "tier": "O3"}));
    assert_eq!(registered[0]["id"].as_i64().unwrap(), created["id"].as_i64().unwrap() + 1);
    let moved = events(&env, "session-moved");
    assert_eq!(moved[0]["data"], json!({"pane": pane, "window": window, "place": "parked"}));

    // The `plan` record in the Context store.
    let records = env.cpd(&["context", "show", "--context", "ctx"]).ok()["records"].clone();
    let record = records.as_array().unwrap().iter().find(|r| r["kind"] == "plan").unwrap().clone();
    assert_eq!(record["text"], "Retry payouts");
    // Migration 3 dropped `plan_repo` from the index (R34 A12): the record read back has no `repo`.
    assert_eq!(record["plan"], json!({"id": "pay", "machine": "m1", "jira": "PAY-1"}));

    // The first message: the header, the brief, then every record (a fresh Lead).
    let message = fs::read_to_string(home(&env).lead_message(&pid("pay"))).unwrap();
    let header = format!("You are the Lead of Plan pay \"Retry payouts\" in Context ctx. Repo {real}, base main. Phase grounding: follow the cpd-lead skill.\n\nRetry the failed payouts.\n\n# Payments\n");
    assert!(message.starts_with(&header), "{message}");
    assert!(message.contains("One engine per machine."), "{message}");
    assert!(message.contains("m1-p1"), "{message}");
}

#[test]
fn new_again_with_the_same_flags_is_already_and_redoes_missing_steps() {
    let env = TestEnv::new();
    let repo = setup(&env);
    let first = new_plan(&env, "pay", &repo);
    let session = first["plan"]["lead_session"].as_str().unwrap().to_owned();
    let txns: i64 = query(&env, "SELECT count(*) FROM txns");

    let again = new_plan(&env, "pay", &repo);
    assert_eq!(again["already"], true);
    assert_eq!((&again["plan"], &again["record"]), (&first["plan"], &first["record"]));
    assert_eq!((&again["lead"]["pane"], &again["lead"]["window"]), (&first["lead"]["pane"], &first["lead"]["window"]));
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM txns"), txns, "nothing written");
    assert_eq!(panes(&env).len(), 1);

    // A crash after the first transaction: no pane, no message. Rerunning redoes both.
    store(&env).write("fixture", |tx| exec(tx, &format!("UPDATE sessions SET tmux_pane = NULL WHERE id = '{session}';"))).unwrap();
    fs::remove_file(home(&env).lead_message(&pid("pay"))).unwrap();
    env.tmux(&["kill-server"]);
    let redone = new_plan(&env, "pay", &repo);
    assert_eq!(redone["already"], true);
    assert_ne!(redone["lead"]["pane"], Value::Null);
    assert_eq!(panes(&env).len(), 1);
    assert!(home(&env).lead_message(&pid("pay")).exists());
    assert_eq!(events(&env, "session-moved").len(), 2);
    assert_eq!(events(&env, "plan-created").len(), 1);

    // Other flags: plan_exists.
    let repo_arg = repo.to_str().unwrap();
    let error = refused(&env.cpd(&["plan", "new", "pay", "--context", "ctx", "--repo", repo_arg, "--title", "Other"]), "plan_exists", 3);
    assert_eq!(error["details"]["plan"]["title"], "Plan pay");
    refused(&env.cpd(&["plan", "new", "pay", "--context", "ctx", "--repo", repo_arg, "--title", "Plan pay", "--name", "x"]), "plan_exists", 3);
}

#[test]
fn new_refusals_write_nothing() {
    let env = TestEnv::new();
    let repo = setup(&env);
    let repo_arg = repo.to_str().unwrap().to_owned();
    let new = |extra: &[&str]| {
        let mut args = vec!["plan", "new", "pay", "--title", "T"];
        args.extend(extra);
        env.cpd(&args)
    };
    let txns: i64 = query(&env, "SELECT count(*) FROM txns");

    refused(&new(&["--context", "nope", "--repo", &repo_arg]), "context_not_found", 4);
    let not_repo = env.root().join("plain");
    fs::create_dir_all(&not_repo).unwrap();
    refused(&new(&["--context", "ctx", "--repo", not_repo.to_str().unwrap()]), "not_a_git_repo", 3);
    refused(&new(&["--context", "ctx", "--repo", "/does/not/exist"]), "not_a_git_repo", 3);
    let error = refused(&new(&["--context", "ctx", "--repo", &repo_arg, "--base", "develop"]), "base_not_found", 4);
    assert_eq!(error["details"]["ref"], "refs/heads/develop");
    env.git(&repo, &["branch", "cpd/pay"]);
    refused(&new(&["--context", "ctx", "--repo", &repo_arg]), "branch_exists", 3);
    refused(&new(&["--context", "ctx", "--repo", &repo_arg, "--brief-file", "/no/such/brief"]), "file_unreadable", 2);
    refused(&env.cpd(&["plan", "new", "Bad", "--context", "ctx", "--repo", &repo_arg, "--title", "T"]), "invalid_plan_id", 2);
    refused(&env.cpd(&["plan", "new", "pay", "--context", "ctx", "--repo", &repo_arg]), "usage", 2);
    refused(&new(&["--context", "ctx", "--repo", &repo_arg, "--name", ""]), "invalid_name", 2);
    refused(&new(&["--context", "ctx", "--repo", &repo_arg, "--jira", "nope"]), "usage", 2);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM txns"), txns);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM plans"), 0);
}

#[test]
fn new_defaults_the_base_to_origin_head_and_needs_it_on_origin() {
    let env = TestEnv::new();
    let origin = setup(&env);
    env.git(&origin, &["branch", "trunk"]);
    env.git(&origin, &["symbolic-ref", "HEAD", "refs/heads/trunk"]);
    let clone = env.root().join("repos").join("clone");
    env.git(env.root(), &["clone", "-q", origin.to_str().unwrap(), clone.to_str().unwrap()]);
    // From a subdirectory of a linked worktree: the repo is the main worktree.
    let linked = env.root().join("repos").join("clone-linked");
    env.git(&clone, &["worktree", "add", "-q", linked.to_str().unwrap(), "-b", "side"]);
    fs::create_dir_all(linked.join("sub")).unwrap();

    let out = new_plan(&env, "pay", &linked.join("sub"));
    assert_eq!(out["plan"]["base"], "trunk");
    assert_eq!(out["plan"]["repo"], clone.canonicalize().unwrap().display().to_string());
    let error = refused(
        &env.cpd(&["plan", "new", "other", "--context", "ctx", "--repo", clone.to_str().unwrap(), "--title", "T", "--base", "side"]),
        "base_not_found",
        4,
    );
    assert_eq!(error["details"]["ref"], "refs/remotes/origin/side", "with an origin, the base must be on it");
}

// ---------- plan phase, the grounded Lead ----------

#[test]
fn phase_plan_grounds_the_context_once_and_later_plans_fork_it() {
    let env = TestEnv::new();
    let repo = setup(&env);
    let first = new_plan(&env, "pay", &repo);
    let lead = first["plan"]["lead_session"].as_str().unwrap().to_owned();

    let out = env.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    let grounded = out["grounded_lead"]["session"].as_str().unwrap().to_owned();
    assert_eq!((&out["plan"], &out["phase"]), (&json!("pay"), &json!("plan")));
    let seq: i64 = query(&env, "SELECT max(seq) FROM context_records WHERE context = 'ctx'");
    assert_eq!(query::<String>(&env, "SELECT lead_session FROM contexts WHERE id = 'ctx'"), grounded);
    assert_eq!(query::<i64>(&env, "SELECT grounded_seq FROM contexts WHERE id = 'ctx'"), seq);

    let row = env.cpd(&["session", "show", &grounded]).ok()["session"].clone();
    assert_eq!((&row["role"], &row["plan"], &row["forked_from"], &row["state"]), (&json!("lead"), &Value::Null, &json!(lead), &json!("stopped")));
    assert_eq!((&row["name"], &row["grounded_lead"], &row["place"]), (&json!("ctx (grounded)"), &json!(true), &json!("parked")));
    assert_eq!(row["tmux_pane"], out["grounded_lead"]["pane"]);
    let window: Vec<_> = panes(&env).into_iter().filter(|p| p.1 == "ctx-ctx").collect();
    assert_eq!(window, vec![(out["grounded_lead"]["pane"].as_str().unwrap().to_owned(), "ctx-ctx".to_owned(), grounded.clone())]);
    // The grounded Lead has no Plan: no CPD_PLAN and no plannotator port (contract §6.5).
    let vars: Vec<String> = pane_env(&env, &grounded).lines().filter(|l| l.starts_with("CPD_")).map(str::to_owned).collect();
    assert!(vars.contains(&format!("CPD_SESSION={grounded}")), "{vars:?}");
    assert!(!vars.iter().any(|l| l.starts_with("CPD_PLAN=")), "{vars:?}");

    let phase = &events(&env, "plan-phase")[0];
    assert_eq!((&phase["plan"], &phase["data"]), (&json!("pay"), &json!({"from": "grounding", "to": "plan"})));
    let ctx_grounded = &events(&env, "context-grounded")[0];
    assert_eq!((&ctx_grounded["session"], &ctx_grounded["plan"]), (&json!(grounded), &Value::Null));
    assert_eq!(ctx_grounded["data"], json!({"seq": seq, "forked_from": lead}));
    let registered = events(&env, "session-registered");
    assert_eq!(registered.last().unwrap()["data"], json!({"role": "lead", "name": "ctx (grounded)", "forked_from": lead, "tier": "O3"}));

    // Already in phase plan.
    let again = env.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    assert_eq!((&again["already"], &again["grounded_lead"]["session"]), (&json!(true), &json!(grounded)));
    assert_eq!(events(&env, "plan-phase").len(), 1);

    // A later Plan forks the grounded Lead, and its message holds only the newer records.
    env.cpd(&["context", "note", "Recorded after grounding.", "--context", "ctx"]).ok();
    let second = new_plan(&env, "refund", &repo);
    assert_eq!(second["lead"]["forked_from"], grounded);
    assert_eq!(events(&env, "plan-created").last().unwrap()["data"]["forked_from"], grounded);
    let message = fs::read_to_string(home(&env).lead_message(&pid("refund"))).unwrap();
    assert!(message.contains("Recorded after grounding."), "{message}");
    assert!(message.contains("m1-p2"), "{message}");
    assert!(!message.contains("m1-p1"), "records up to grounded_seq are in the fork already: {message}");

    // Its phase plan finds the Context grounded: no new grounded Lead.
    let out = env.cpd(&["plan", "phase", "plan", "--plan", "refund"]).ok();
    assert_eq!(out["grounded_lead"], Value::Null);
    assert_eq!(events(&env, "context-grounded").len(), 1);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM sessions WHERE plan IS NULL"), 1);
}

#[test]
fn phase_again_creates_the_window_a_crash_left_out() {
    let env = TestEnv::new();
    let repo = setup(&env);
    new_plan(&env, "pay", &repo);
    let out = env.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    let grounded = out["grounded_lead"]["session"].as_str().unwrap().to_owned();
    // A crash between the two transactions: the Session has no pane.
    store(&env).write("fixture", |tx| exec(tx, &format!("UPDATE sessions SET tmux_pane = NULL WHERE id = '{grounded}';"))).unwrap();
    let pane = out["grounded_lead"]["pane"].as_str().unwrap();
    env.tmux(&["kill-pane", "-t", pane]);

    let again = env.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    assert_eq!(again["already"], true);
    assert_eq!(again["grounded_lead"]["session"], grounded);
    assert_ne!(again["grounded_lead"]["pane"], Value::Null);
    assert_eq!(panes(&env).into_iter().filter(|p| p.1 == "ctx-ctx").count(), 1);
}

#[test]
fn phase_refusals() {
    let env = TestEnv::new();
    let repo = setup(&env);
    new_plan(&env, "pay", &repo);
    let error = refused(&env.cpd(&["plan", "phase", "dispatch", "--plan", "pay"]), "illegal_transition", 3);
    assert_eq!((&error["details"]["from"], &error["details"]["to"]), (&json!("grounding"), &json!("dispatch")));
    refused(&env.cpd(&["plan", "phase", "grounding", "--plan", "pay"]), "illegal_transition", 3);
    refused(&env.cpd(&["plan", "phase", "bogus", "--plan", "pay"]), "usage", 2);
    refused(&env.cpd(&["plan", "phase", "plan"]), "plan_required", 2);
    refused(&env.cpd(&["plan", "phase", "plan", "--plan", "nope"]), "plan_not_found", 4);
    store(&env)
        .write("fixture", |tx| exec(tx, "UPDATE plans SET phase = 'plan' WHERE id = 'pay'; UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';
                 UPDATE prs SET status = 'dispatch', lead_worktree = '/w', started_at = 't' WHERE plan = 'pay';"))
        .unwrap();
    let error = refused(&cpd_with(&env, &[("CPD_PLAN", "pay")], &["plan", "phase", "plan"]), "illegal_transition", 3);
    assert_eq!(error["details"]["from"], "dispatch");
}

/// Two Plans grounding at once produce one grounded Lead: the
/// compare-and-swap on `contexts.lead_session` (contract §3.1).
#[test]
fn plans_grounding_at_once_produce_one_grounded_lead() {
    let env = TestEnv::new();
    let repo = setup(&env);
    let plans = ["pa", "pb", "pc", "pd", "pe", "pf"];
    for plan in plans {
        new_plan(&env, plan, &repo);
    }
    let barrier = std::sync::Barrier::new(plans.len());
    let outs: Vec<Value> = std::thread::scope(|scope| {
        let handles: Vec<_> = plans
            .iter()
            .map(|plan| {
                let (env, barrier) = (&env, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    env.cpd(&["plan", "phase", "plan", "--plan", plan]).ok()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let grounded: Vec<&Value> = outs.iter().filter(|o| !o["grounded_lead"].is_null()).collect();
    assert_eq!(grounded.len(), 1, "{outs:?}");
    let session = grounded[0]["grounded_lead"]["session"].as_str().unwrap();
    assert_eq!(query::<String>(&env, "SELECT lead_session FROM contexts WHERE id = 'ctx'"), session);
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM sessions WHERE plan IS NULL"), 1);
    assert_eq!(events(&env, "context-grounded").len(), 1);
    assert_eq!(events(&env, "plan-phase").len(), plans.len());
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM plans WHERE phase = 'plan'"), plans.len() as i64);
}

// ---------- plan list, plan show ----------

#[test]
fn list_and_show_are_snapshots_with_an_events_cursor() {
    let env = TestEnv::new();
    let repo = setup(&env);
    planned(&env, &repo, "pay");
    new_plan(&env, "refund", &repo);
    env.cpd(&["context", "new", "other", "--title", "Other"]).ok();

    let list = env.cpd(&["plan", "list"]).ok();
    let cursor = list["events_cursor"].as_i64().unwrap();
    assert_eq!(cursor, env.cpd(&["events", "list", "--since", "0", "--limit", "5000"]).ok()["next"].as_i64().unwrap());
    let plans = list["plans"].as_array().unwrap();
    assert_eq!(plans.len(), 2);
    let pay = &plans[0];
    assert_eq!((&pay["id"], &pay["context"], &pay["phase"], &pay["pr"]), (&json!("pay"), &json!("ctx"), &json!("plan"), &Value::Null));
    assert_eq!(pay["counts"], json!({"draft": 2, "queued": 0, "running": 0, "merged": 0, "failed": 0, "cancelled": 0}));
    assert_eq!(pay["repo"], repo.canonicalize().unwrap().display().to_string());
    let ids = |args: &[&str]| -> Vec<String> {
        env.cpd(args).ok()["plans"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap().to_owned()).collect()
    };
    assert_eq!(ids(&["plan", "list", "--phase", "grounding"]), ["refund"]);
    assert_eq!(ids(&["plan", "list", "--context", "ctx", "--phase", "plan"]), ["pay"]);
    assert!(ids(&["plan", "list", "--context", "other"]).is_empty());
    assert_eq!(cpd_with(&env, &[("CPD_PLAN", "pay")], &["plan", "list"]).ok()["plans"].as_array().unwrap().len(), 2, "filters only");
    refused(&env.cpd(&["plan", "list", "--phase", "bogus"]), "usage", 2);

    let out = show(&env, "pay");
    let plan = &out["plan"];
    assert_eq!((&plan["id"], &plan["phase"], &plan["lead_worktree"], &plan["pr"]), (&json!("pay"), &json!("plan"), &Value::Null, &Value::Null));
    assert_eq!(plan["pr_seen"], json!({"checks": {}, "reviewIds": [], "commentIds": []}));
    let jobs = out["jobs"].as_array().unwrap();
    assert_eq!(jobs.len(), 2);
    assert_eq!((&jobs[1]["key"], &jobs[1]["needs"], &jobs[1]["status"]), (&json!("b"), &json!(["a"]), &json!("draft")));
    assert_eq!((&jobs[1]["touches"], &jobs[1]["attempts"], &jobs[1]["fingerprint"]), (&json!(["src/b.rs"]), &json!([]), &Value::Null));
    assert_eq!((&out["review"], &out["escalations"], &out["tamper"]), (&Value::Null, &json!([]), &json!([])));
    assert_eq!(out["counts"]["draft"], 2);
    assert!(out["events_cursor"].as_i64().unwrap() >= cursor);
    assert_eq!(cpd_with(&env, &[("CPD_PLAN", "pay")], &["plan", "show"]).ok()["plan"]["id"], "pay");

    refused(&env.cpd(&["plan", "show"]), "plan_required", 2);
    refused(&env.cpd(&["plan", "show", "--plan", "nope"]), "plan_not_found", 4);
    refused(&env.cpd(&["plan", "show", "--plan", "Bad"]), "invalid_plan_id", 2);
}

#[test]
fn show_lists_attempts_open_escalations_and_tampered_fingerprints() {
    let env = TestEnv::new();
    let repo = setup(&env);
    planned(&env, &repo, "pay");
    let fingerprint = "f".repeat(64);
    store(&env)
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';
                     UPDATE prs SET status = 'dispatch', lead_worktree = '/w', started_at = 't' WHERE plan = 'pay';
                     UPDATE jobs SET status = 'queued', fingerprint = '{fingerprint}' WHERE plan = 'pay';
                     UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'a';
                     INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
                       VALUES ('pay', 'a-a1', 'a', 1, 'S2', 'ladder', 'running', 'cpd/pay--a-a1', '/w', 't1');
                     INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
                       VALUES ('pay', 'a-a1-q1', 'a-a1', 1, 'Which?', 'open', 1, 't2');"
                ),
            )
        })
        .unwrap();
    let out = show(&env, "pay");
    let a = &out["jobs"][0];
    assert_eq!(
        a["attempts"],
        json!([{"id": "a-a1", "number": 1, "tier": "S2", "origin": "ladder", "status": "running", "outcome": null,
                "cause": null, "worker_session": null, "started_at": "t1", "ended_at": null}])
    );
    assert_eq!(out["escalations"], json!([{"id": "a-a1-q1", "attempt": "a-a1", "question": "Which?", "asked_at": "t2"}]));
    assert_eq!(out["counts"]["running"], 1);
    let tamper = out["tamper"].as_array().unwrap();
    assert_eq!(tamper.len(), 2, "both Locked Jobs differ from the stored fingerprint: {tamper:?}");
    assert_eq!((&tamper[0]["kind"], &tamper[0]["job"], &tamper[0]["expected"]), (&json!("fingerprint"), &json!("a"), &json!(fingerprint)));
}

// ---------- plan render ----------

#[test]
fn render_writes_the_plan_markdown_and_its_fingerprint() {
    let env = TestEnv::new();
    let repo = setup(&env);
    planned(&env, &repo, "pay");
    add_job(&env, "pay", "gone", &[]);
    store(&env).write("fixture", |tx| exec(tx, "UPDATE jobs SET status = 'cancelled', cancel_reason = 'x', cancel_requested_at = 't' WHERE key = 'gone';")).unwrap();

    let out = env.cpd(&["plan", "render", "--plan", "pay"]).ok();
    let path = home(&env).plan_md(&pid("pay"));
    assert_eq!(out["path"], path.display().to_string());
    assert_eq!(out["draft_jobs"], 2);
    let (pr_key, pr_repo, pr_base): (String, String, String) = store(&env)
        .read(|c| Ok(c.query_row("SELECT key, repo, base FROM prs WHERE plan = 'pay'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?))
        .unwrap();
    let spec = |key: &str, needs: &str| {
        format!(
            "{{\"acceptance\":[\"cargo test\"],\"context\":\"Context of {key}.\",\"goal\":\"Goal of {key}.\",\"key\":\"{key}\",\"needs\":[{needs}],\"out_of_scope\":[],\"pr\":\"{pr_key}\",\"tier\":\"S2\",\"title\":\"Job {key}\",\"touches\":[\"src/{key}.rs\"]}}"
        )
    };
    let prs = format!("[{{\"base\":\"{pr_base}\",\"key\":\"{pr_key}\",\"needs\":[],\"repo\":{}}}]", serde_json::to_string(&pr_repo).unwrap());
    assert_eq!(out["fingerprint"], sha256(&format!("{{\"jobs\":[{},{}],\"prs\":{prs}}}", spec("a", ""), spec("b", "\"a\""))));
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.starts_with("# Plan: Plan pay\n\nContext: `ctx` · Phase: `plan`\n"), "{text}");
    assert!(text.contains("| `b` | Job b | S2 | draft | `a` | `src/b.rs` |"), "{text}");
    assert!(text.contains("```mermaid\ngraph TD\n  j1[\"a\"]\n  j2[\"b\"]\n  j1 --> j2\n```"), "{text}");
    assert!(text.contains("## `a`: Job a\n"), "{text}");
    assert!(text.contains("### Goal\n\nGoal of b.\n"), "{text}");
    assert!(!text.contains("gone"), "cancelled Jobs are left out: {text}");

    // Idempotent: the same Jobs render the same file.
    let again = env.cpd(&["plan", "render", "--plan", "pay"]).ok();
    assert_eq!(again, out);
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    refused(&env.cpd(&["plan", "render"]), "plan_required", 2);
    refused(&env.cpd(&["plan", "render", "--plan", "nope"]), "plan_not_found", 4);
}

// ---------- plan review, plan decide ----------

#[test]
fn review_requests_a_pending_review_once_per_fingerprint() {
    let env = TestEnv::new();
    let repo = setup(&env);
    new_plan(&env, "pay", &repo);
    let error = refused(&env.cpd(&["plan", "review", "--plan", "pay"]), "illegal_phase", 3);
    assert_eq!(error["details"]["phase"], "grounding");
    env.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    refused(&env.cpd(&["plan", "review", "--plan", "pay"]), "no_draft_jobs", 3);
    add_job(&env, "pay", "a", &[]);

    let out = env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    let fingerprint = env.cpd(&["plan", "render", "--plan", "pay"]).ok()["fingerprint"].clone();
    let review_path = home(&env).plan_review_md(&pid("pay"), ReviewId::parse("r1").unwrap());
    assert_eq!((&out["review"]["id"], &out["review"]["status"], &out["review"]["fingerprint"]), (&json!("r1"), &json!("pending"), &fingerprint));
    assert_eq!(out["path"], review_path.display().to_string());
    assert_eq!(out["next"], "The User reviews the plan in plannotator. Stop and wait for the decision message.");
    assert_eq!(fs::read_to_string(&review_path).unwrap(), fs::read_to_string(home(&env).plan_md(&pid("pay"))).unwrap());
    let event = &events(&env, "plan-review")[0];
    assert_eq!((&event["plan"], &event["data"]), (&json!("pay"), &json!({"review": "r1", "fingerprint": fingerprint, "status": "pending"})));

    // Same Jobs: the pending review again.
    let again = cpd_with(&env, &[("CPD_PLAN", "pay")], &["plan", "review"]).ok();
    assert_eq!((&again["already"], &again["review"]), (&json!(true), &out["review"]));
    assert_eq!(events(&env, "plan-review").len(), 1);

    // Changed Jobs: a new review; the copy of r1 is what the User saw.
    let r1 = fs::read_to_string(&review_path).unwrap();
    edit_goal(&env, "pay", "a", "A new goal.");
    let r2 = env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    assert_eq!(r2["review"]["id"], "r2");
    assert_ne!(r2["review"]["fingerprint"], fingerprint);
    assert_eq!(fs::read_to_string(&review_path).unwrap(), r1);
    assert!(fs::read_to_string(r2["path"].as_str().unwrap()).unwrap().contains("A new goal."));
    assert_eq!(show(&env, "pay")["review"]["id"], "r2");
    refused(&env.cpd(&["plan", "review"]), "plan_required", 2);
}

#[test]
fn decide_records_the_decision_once() {
    let env = TestEnv::new();
    let repo = setup(&env);
    planned(&env, &repo, "pay");
    env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    let feedback = env.root().join("feedback.md");
    fs::write(&feedback, "Split job b.").unwrap();

    refused(&env.cpd(&["plan", "decide", "r1", "--plan", "pay"]), "usage", 2);
    refused(&env.cpd(&["plan", "decide", "r1", "--approve", "--reject", "--plan", "pay"]), "usage", 2);
    refused(&env.cpd(&["plan", "decide", "x1", "--approve", "--plan", "pay"]), "invalid_review_id", 2);
    refused(&env.cpd(&["plan", "decide", "r9", "--approve", "--plan", "pay"]), "review_not_found", 4);
    refused(&env.cpd(&["plan", "decide", "r1", "--approve", "--plan", "pay", "--feedback-file", "/no/file"]), "file_unreadable", 2);

    let out = env.cpd(&["plan", "decide", "r1", "--reject", "--feedback-file", feedback.to_str().unwrap(), "--plan", "pay"]).ok();
    assert_eq!((&out["review"]["status"], &out["review"]["feedback"], &out["current"], &out["next"]), (&json!("rejected"), &json!("Split job b."), &json!(true), &Value::Null));
    assert!(out["review"]["decided_at"].is_string());
    let decided = events(&env, "plan-review").last().unwrap().clone();
    assert_eq!(decided["data"]["status"], "rejected");
    assert_eq!(decided["data"]["feedback"], "Split job b.");
    let again = env.cpd(&["plan", "decide", "r1", "--reject", "--plan", "pay"]).ok();
    assert_eq!(again["already"], true);
    let error = refused(&env.cpd(&["plan", "decide", "r1", "--approve", "--plan", "pay"]), "review_decided", 3);
    assert_eq!(error["details"]["status"], "rejected");
    assert_eq!(events(&env, "plan-review").len(), 2);

    // Approved, then the Jobs change: no longer current, so no plan lock.
    let r2 = env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    assert_eq!(r2["review"]["id"], "r2");
    let out = env.cpd(&["plan", "decide", "r2", "--approve", "--plan", "pay"]).ok();
    assert_eq!((&out["review"]["status"], &out["review"]["feedback"], &out["current"], &out["next"]), (&json!("approved"), &Value::Null, &json!(true), &json!("cpd plan lock")));
    edit_goal(&env, "pay", "a", "Changed.");
    let again = env.cpd(&["plan", "decide", "r2", "--approve", "--plan", "pay"]).ok();
    assert_eq!((&again["already"], &again["current"], &again["next"]), (&json!(true), &json!(false), &Value::Null));
}

// ---------- plan lock ----------

/// A Plan `pay` in phase `plan` with an approved, current review.
fn approved(env: &TestEnv, repo: &Path) {
    planned(env, repo, "pay");
    env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    env.cpd(&["plan", "decide", "r1", "--approve", "--plan", "pay"]).ok();
}

fn watch_calls(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("cpd").into_iter().filter(|c| c[0] == "watch").collect()
}

#[test]
fn lock_creates_the_lead_worktree_queues_the_drafts_and_starts_the_engine() {
    let env = TestEnv::new();
    let repo = setup(&env);
    approved(&env, &repo);
    let head = env.git(&repo, &["rev-parse", "HEAD"]);

    let out = env.cpd(&["plan", "lock", "--plan", "pay"]).ok();
    let worktree = repo.parent().unwrap().canonicalize().unwrap().join("pay.pay");
    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "phase": "dispatch", "jobs": ["a", "b"],
               "prs": [{"key": "pay", "status": "dispatch", "needs": [], "lead_branch": "cpd/pay", "lead_worktree": worktree.display().to_string(),
                        "base_sha": head, "reused": false, "pushed": false}],
               "watch": {"started": true, "pane": "%9"}})
    );
    assert_eq!(env.git(&worktree, &["symbolic-ref", "--short", "HEAD"]), "cpd/pay");
    assert_eq!(watch_calls(&env), vec![vec!["watch".to_owned(), "--ensure".to_owned()]]);

    let shown = show(&env, "pay");
    assert_eq!((&shown["plan"]["phase"], &shown["plan"]["lead_worktree"]), (&json!("dispatch"), &json!(worktree.display().to_string())));
    assert!(shown["plan"]["locked_at"].is_string());
    for job in shown["jobs"].as_array().unwrap() {
        assert_eq!(job["status"], "queued");
        let needs: Vec<String> = job["needs"].as_array().unwrap().iter().map(|n| format!("\"{}\"", n.as_str().unwrap())).collect();
        let key = job["key"].as_str().unwrap();
        let canonical = format!(
            "{{\"acceptance\":[\"cargo test\"],\"context\":\"Context of {key}.\",\"goal\":\"Goal of {key}.\",\"needs\":[{}],\"out_of_scope\":[],\"tier\":\"S2\",\"touches\":[\"src/{key}.rs\"]}}",
            needs.join(",")
        );
        assert_eq!(job["fingerprint"], sha256(&canonical), "the Locked-Job fingerprint of {key}");
    }
    assert_eq!(shown["tamper"], json!([]));
    let phase = events(&env, "plan-phase").last().unwrap().clone();
    assert_eq!(phase["data"], json!({"from": "plan", "to": "dispatch", "jobs": ["a", "b"], "prs": ["pay"]}));
    let started = events(&env, "pr-started");
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!((&started[0]["data"]["pr"], &started[0]["data"]["by"]), (&json!("pay"), &json!("lock")));

    // Already: still makes sure the engine runs.
    let again = env.cpd(&["plan", "lock", "--plan", "pay"]).ok();
    assert_eq!((&again["already"], &again["phase"], &again["watch"]["started"]), (&json!(true), &json!("dispatch"), &json!(true)));
    assert_eq!(watch_calls(&env).len(), 2);
    assert_eq!(events(&env, "plan-phase").len(), 2);
}

// ---------- plan lock and the remote Lead Branch (R35) ----------

/// A bare `origin` for `repo`, holding `main`; returns its path.
fn with_origin(env: &TestEnv, repo: &Path) -> PathBuf {
    let origin = env.root().join("origins").join("pay.git");
    fs::create_dir_all(&origin).unwrap();
    env.git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    env.git(repo, &["remote", "add", "origin", origin.to_str().unwrap()]);
    env.git(repo, &["push", "-q", "origin", "main"]);
    origin
}

/// Commits an empty commit on `main` of `repo` and pushes it.
fn push_main_commit(env: &TestEnv, repo: &Path, message: &str) -> String {
    env.git(repo, &["commit", "-q", "--allow-empty", "-m", message]);
    env.git(repo, &["push", "-q", "origin", "main"]);
    env.git(repo, &["rev-parse", "HEAD"])
}

#[test]
fn lock_creates_and_pushes_the_lead_branch_when_the_remote_has_none() {
    let env = TestEnv::new();
    let repo = setup(&env);
    let origin = with_origin(&env, &repo);
    approved(&env, &repo);
    let head = env.git(&repo, &["rev-parse", "HEAD"]);

    let out = env.cpd(&["plan", "lock", "--plan", "pay"]).ok();

    assert_eq!((&out["prs"][0]["base_sha"], &out["prs"][0]["pushed"], &out["prs"][0]["reused"]), (&json!(head), &json!(true), &json!(false)));
    assert_eq!(env.git(&repo, &["ls-remote", "--heads", "origin", "cpd/pay"]), format!("{head}\trefs/heads/cpd/pay"));
    let worktree = repo.parent().unwrap().canonicalize().unwrap().join("pay.pay");
    assert_eq!(env.git(&worktree, &["rev-parse", "--abbrev-ref", "@{upstream}"]), "origin/cpd/pay");
    assert_eq!(env.git(&origin, &["rev-parse", "cpd/pay"]), head);
    assert!(events(&env, "error").is_empty());
}

#[test]
fn lock_reuses_a_remote_lead_branch_that_contains_the_base() {
    let env = TestEnv::new();
    let repo = setup(&env);
    with_origin(&env, &repo);
    approved(&env, &repo);
    // A commit only the remote has, on top of the base.
    env.git(&repo, &["commit", "-q", "--allow-empty", "-m", "only on the remote"]);
    let remote_tip = env.git(&repo, &["rev-parse", "HEAD"]);
    env.git(&repo, &["push", "-q", "origin", "HEAD:refs/heads/cpd/pay"]);
    env.git(&repo, &["reset", "-q", "--hard", "HEAD~1"]);
    assert_eq!(env.git(&repo, &["branch", "--list", "cpd/pay"]), "");

    let out = env.cpd(&["plan", "lock", "--plan", "pay"]).ok();

    let worktree = repo.parent().unwrap().canonicalize().unwrap().join("pay.pay");
    assert_eq!((&out["prs"][0]["base_sha"], &out["prs"][0]["reused"], &out["prs"][0]["pushed"]), (&json!(remote_tip), &json!(true), &json!(false)));
    assert_eq!(env.git(&worktree, &["rev-parse", "HEAD"]), remote_tip);
    assert_eq!(env.git(&repo, &["rev-parse", "refs/heads/cpd/pay"]), remote_tip);
    assert_eq!(env.git(&repo, &["ls-remote", "--heads", "origin", "cpd/pay"]), format!("{remote_tip}\trefs/heads/cpd/pay"));
    assert!(events(&env, "error").is_empty());
}

#[test]
fn lock_refuses_a_remote_lead_branch_that_does_not_contain_the_base() {
    let env = TestEnv::new();
    let repo = setup(&env);
    with_origin(&env, &repo);
    approved(&env, &repo);
    // cpd/pay at the old main; then main moves on, so origin/main is not its ancestor.
    env.git(&repo, &["push", "-q", "origin", "main:refs/heads/cpd/pay"]);
    let remote_branch = env.git(&repo, &["rev-parse", "HEAD"]);
    push_main_commit(&env, &repo, "main moves on");

    let error = refused(&env.cpd(&["plan", "lock", "--plan", "pay"]), "branch_exists", 3);

    assert_eq!((&error["details"]["branch"], &error["details"]["pr"]), (&json!("cpd/pay"), &json!("pay")));
    assert_eq!(env.git(&repo, &["branch", "--list", "cpd/pay"]), "");
    assert!(!repo.parent().unwrap().join("pay.pay").exists());
    assert_eq!(env.git(&repo, &["ls-remote", "--heads", "origin", "cpd/pay"]), format!("{remote_branch}\trefs/heads/cpd/pay"));
    assert_eq!(show(&env, "pay")["plan"]["phase"], "plan");
    assert!(watch_calls(&env).is_empty());
}

#[test]
fn lock_without_an_origin_remote_is_as_before() {
    let env = TestEnv::new();
    let repo = setup(&env);
    approved(&env, &repo);
    env.cpd(&["plan", "lock", "--plan", "pay"]).ok();
    assert_eq!(env.git(&repo, &["remote"]), "");
    assert_eq!(show(&env, "pay")["plan"]["phase"], "dispatch");
    assert!(events(&env, "error").is_empty());
}

#[test]
fn a_push_failure_at_lock_still_locks_and_appends_one_push_failed_error() {
    let env = TestEnv::new();
    let repo = setup(&env);
    let origin = with_origin(&env, &repo);
    let hook = origin.join("hooks").join("pre-receive");
    fs::write(&hook, "#!/bin/sh\necho refused by hook >&2\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    approved(&env, &repo);

    let out = env.cpd(&["plan", "lock", "--plan", "pay"]).ok();

    assert_eq!(out["phase"], "dispatch");
    assert_eq!(show(&env, "pay")["plan"]["phase"], "dispatch");
    assert_eq!(env.git(&repo, &["ls-remote", "--heads", "origin", "cpd/pay"]), "");
    let errors = events(&env, "error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["data"]["code"], "push_failed");
    assert_eq!(errors[0]["data"]["source"], "plan lock");
    assert!(errors[0]["data"]["details"]["stderr"].as_str().unwrap().contains("refused by hook"));
}

#[test]
fn lock_stays_locked_when_the_engine_cannot_start() {
    let env = TestEnv::new();
    let repo = setup(&env);
    approved(&env, &repo);
    fake_cpd(&env, WATCH_FAILS);
    let error = refused(&env.cpd(&["plan", "lock", "--plan", "pay"]), "watch_start_failed", 1);
    assert_eq!(error["details"]["next"], "cpd watch --ensure");
    assert_eq!(error["details"]["error"]["message"], "No lock.");
    assert_eq!(show(&env, "pay")["plan"]["phase"], "dispatch", "the Plan stays locked");

    // An engine that prints nothing usable fails the same way.
    fake_cpd(&env, "echo nonsense; exit 0");
    let error = refused(&env.cpd(&["plan", "lock", "--plan", "pay"]), "watch_start_failed", 1);
    assert_eq!(error["details"]["error"]["stdout"], "nonsense");
}

#[test]
fn lock_refusals_change_nothing() {
    let env = TestEnv::new();
    let repo = setup(&env);
    new_plan(&env, "pay", &repo);
    let lock = || env.cpd(&["plan", "lock", "--plan", "pay"]);
    let error = refused(&lock(), "illegal_transition", 3);
    assert_eq!(error["details"]["from"], "grounding");
    env.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    refused(&lock(), "no_draft_jobs", 3);

    add_job(&env, "pay", "b", &[]);
    add_job(&env, "pay", "a", &["b"]);
    let error = refused(&lock(), "plan_not_approved", 3);
    assert_eq!(error["details"]["review"], Value::Null);
    env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    refused(&lock(), "plan_not_approved", 3);
    env.cpd(&["plan", "decide", "r1", "--reject", "--plan", "pay"]).ok();
    let error = refused(&lock(), "plan_not_approved", 3);
    assert_eq!(error["details"]["review"]["status"], "rejected");
    env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    env.cpd(&["plan", "decide", "r2", "--approve", "--plan", "pay"]).ok();
    edit_goal(&env, "pay", "a", "Changed after approval.");
    refused(&lock(), "plan_not_approved", 3);

    // A cycle (b needs a, a needs b), approved.
    store(&env).write("fixture", |tx| exec(tx, "INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'b', 'a');")).unwrap();
    env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    env.cpd(&["plan", "decide", "r3", "--approve", "--plan", "pay"]).ok();
    let error = refused(&lock(), "dependency_cycle", 3);
    assert_eq!(error["details"]["cycle"].as_array().unwrap().len(), 3);
    store(&env).write("fixture", |tx| exec(tx, "DELETE FROM dependencies WHERE plan = 'pay' AND job = 'b';")).unwrap();
    env.cpd(&["plan", "review", "--plan", "pay"]).ok();
    env.cpd(&["plan", "decide", "r4", "--approve", "--plan", "pay"]).ok();

    // Something at the Lead worktree's path, then the branch.
    let path = repo.parent().unwrap().join("pay.pay");
    fs::create_dir_all(&path).unwrap();
    refused(&lock(), "worktree_exists", 3);
    fs::remove_dir(&path).unwrap();
    env.git(&repo, &["branch", "cpd/pay"]);
    refused(&lock(), "branch_exists", 3);
    env.git(&repo, &["branch", "-D", "cpd/pay"]);

    // pre-start fails: the worktree and branch are removed.
    fs::create_dir_all(repo.join(".config")).unwrap();
    fs::write(repo.join(".config/wt.toml"), "pre-start = \"echo setting up; exit 3\"\n").unwrap();
    env.git(&repo, &["add", "-A"]);
    env.git(&repo, &["commit", "-q", "-m", "setup"]);
    let error = refused(&lock(), "setup_failed", 1);
    assert_eq!(error["details"]["log"], home(&env).root().join("run/pay/logs/lead-pay.pre-start.log").display().to_string());
    assert_eq!((&error["details"]["pr"], &error["details"]["step"]), (&json!("pay"), &json!("pre-start")));
    assert!(!path.exists());
    assert_eq!(env.git(&repo, &["branch", "--list", "cpd/pay"]), "");

    assert_eq!(show(&env, "pay")["plan"]["phase"], "plan");
    assert_eq!(show(&env, "pay")["counts"]["draft"], 2);
    assert!(watch_calls(&env).is_empty());
    refused(&env.cpd(&["plan", "lock"]), "plan_required", 2);
}
