//! Jobs (contract §2.3, §3.2; commands.md §5.1–§5.4, §5.6, §5.7), run as the
//! built binary against a temporary `$CPD_HOME`. Plans, Attempts and the
//! states only the engine reaches are written straight into the database, as
//! fixtures; the commands under test are the only ones that write Jobs.
//! pi is a fake install whose Tier catalog is seeded in the cache, so no node runs.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{Run, TestEnv};
use cpd::error::CpdError;
use cpd::home::CpdHome;
use cpd::ids::PlanId;
use cpd::plan::jobs;
use cpd::plan::locked_fingerprint;
use cpd::store::{Store, WriteTxn};
use serde_json::{Value, json};

// ---------- fixtures ----------

fn store(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

/// Runs `sql` in one write transaction.
fn fixture(env: &TestEnv, sql: &str) {
    store(env).write("fixture", |tx| exec(tx, sql)).unwrap();
}

fn query<T: rusqlite::types::FromSql>(env: &TestEnv, sql: &str) -> T {
    store(env).read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
}

/// O supports efforts 0–5, S 1–5, H 0–3.
const EFFORTS: [(&str, &str, &str); 3] = [("O", "p/opus", "[0,1,2,3,4,5]"), ("S", "p/sonnet", "[1,2,3,4,5]"), ("H", "p/haiku", "[0,1,2,3]")];

/// `config.toml`, a fake pi install, and its Tier catalog in the cache.
fn setup(env: &TestEnv) {
    fs::write(
        env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n",
    )
    .unwrap();
    env.fake_pi("exit 0");
    fs::create_dir_all(env.root().join("install")).unwrap();
    fs::write(env.root().join("install/current-version"), "1.0.0\n").unwrap();
    let scope = env.root().join("install/releases/1.0.0/node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    let rows: String = EFFORTS
        .iter()
        .map(|(letter, model, efforts)| {
            format!("INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{model}', '{efforts}', 't');")
        })
        .collect();
    fixture(env, &rows);
    fixture(env, "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Context', 't', 'm1');");
}

/// Plan `id` in `phase`, with its Lead session, moved there one phase at a time.
fn plan(env: &TestEnv, id: &str, phase: &str) {
    const STEPS: [(&str, &str); 4] = [
        ("plan", ""),
        ("dispatch", ", lead_worktree = '/wt', locked_at = 't'"),
        ("pr", ", pr_number = 1, pr_url = 'u', pr_opened_at = 't'"),
        ("done", ", done_at = 't'"),
    ];
    let mut sql = format!(
        "INSERT INTO plans (id, context, title, repo, base, lead_branch, phase, lead_session, created_at)
         VALUES ('{id}', 'ctx', 'Plan {id}', '/repo', 'main', 'cpd/{id}', 'grounding', 'lead-{id}', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
         VALUES ('lead-{id}', 'lead', 'ctx', '{id}', '{id}', '/repo', 'O3', 'stopped', 't', 't');"
    );
    if phase != "grounding" {
        let last = STEPS.iter().position(|(p, _)| *p == phase).unwrap_or_else(|| panic!("{phase}"));
        for (step, set) in &STEPS[..=last] {
            sql += &format!("UPDATE plans SET phase = '{step}'{set} WHERE id = '{id}';");
        }
    }
    fixture(env, &sql);
}

fn spec_json() -> Value {
    json!({"goal": "Do it.", "context": "Because.", "touches": ["src/a.rs"], "acceptance": ["cargo test"], "out_of_scope": ["Everything else."]})
}

fn write_spec(env: &TestEnv, name: &str, text: &str) -> String {
    let path: PathBuf = env.root().join(name);
    fs::write(&path, text).unwrap();
    path.to_str().unwrap().to_owned()
}

fn spec_file(env: &TestEnv) -> String {
    write_spec(env, "spec.json", &spec_json().to_string())
}

fn job_add(env: &TestEnv, plan: &str, key: &str, tier: &str, needs: &[&str]) -> Run {
    let spec = spec_file(env);
    let title = format!("Title of {key}");
    let needs = needs.join(",");
    let mut args = vec!["job", "add", key, "--title", &title, "--tier", tier, "--spec", &spec, "--plan", plan];
    if !needs.is_empty() {
        args.extend(["--needs", &needs]);
    }
    env.cpd(&args)
}

fn job(env: &TestEnv, args: &[&str], plan: &str) -> Run {
    let mut all = vec!["job"];
    all.extend(args);
    all.extend(["--plan", plan]);
    env.cpd(&all)
}

fn refused(run: &Run, code: &str, exit: i32) -> Value {
    let error = run.error();
    assert_eq!(error["code"], code, "{run:?}");
    assert_eq!(run.exit, exit, "{run:?}");
    error
}

fn events(env: &TestEnv, kind: &str) -> Vec<Value> {
    env.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
}

fn count(env: &TestEnv, table: &str) -> i64 {
    query(env, &format!("SELECT count(*) FROM {table}"))
}

/// A Plan `p` in phase `plan` with draft Jobs `a` and `b` (needs `a`).
fn drafted(env: &TestEnv) {
    setup(env);
    plan(env, "pl", "plan");
    job_add(env, "pl", "a", "S2", &[]).ok();
    job_add(env, "pl", "b", "S2", &["a"]).ok();
}

/// What `plan lock` does to the Jobs: phase `dispatch`, every draft `queued`
/// with its Locked-Job fingerprint.
fn lock(env: &TestEnv, plan_id: &str) {
    let id = PlanId::parse(plan_id).unwrap();
    let all = store(env).read(|conn| jobs::load(conn, &id)).unwrap();
    let mut sql = format!("UPDATE plans SET phase = 'dispatch', lead_worktree = '/wt', locked_at = 't' WHERE id = '{plan_id}';");
    for job in all.iter().filter(|j| j.is_draft()) {
        sql += &format!(
            "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = '{plan_id}' AND key = '{}';",
            locked_fingerprint(job),
            job.key
        );
    }
    fixture(env, &sql);
}

/// Moves the queued Job `key` to `status` the way the engine would.
fn engine(env: &TestEnv, plan_id: &str, key: &str, status: &str) {
    let attempt = format!("INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
        VALUES ('{plan_id}', '{key}-a1', '{key}', 1, 'S2', 'ladder', 'running', 'cpd/{plan_id}--{key}-a1', '/w', 't');");
    let ended = |outcome: &str, cause: &str| {
        format!("UPDATE attempts SET status = 'ended', outcome = '{outcome}', cause = '{cause}', ended_at = 't' WHERE plan = '{plan_id}' AND id = '{key}-a1';")
    };
    let set = |sql: &str| format!("UPDATE jobs SET {sql} WHERE plan = '{plan_id}' AND key = '{key}';");
    let sql = match status {
        "running" => format!("{}{}", set("status = 'running'"), attempt),
        "failed" => format!(
            "{}{}{}{}",
            set("status = 'running'"),
            attempt,
            ended("failed", "timed_out"),
            set("status = 'failed', failed_cause = 'timed_out'")
        ),
        "merged" => format!(
            "{}{}{}",
            set("status = 'running'"),
            attempt,
            set("status = 'merged', merged_sha = 'c0ffee'")
        ),
        other => panic!("{other}"),
    };
    fixture(env, &sql);
}

fn job_row(env: &TestEnv, plan_id: &str, key: &str, column: &str) -> Value {
    store(env)
        .read(|conn| {
            Ok(conn.query_row(
                &format!("SELECT {column} FROM jobs WHERE plan = ?1 AND key = ?2"),
                [plan_id, key],
                |r| Ok(match r.get_ref(0)? {
                    rusqlite::types::ValueRef::Null => Value::Null,
                    rusqlite::types::ValueRef::Integer(i) => json!(i),
                    rusqlite::types::ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
                    other => panic!("{other:?}"),
                }),
            )?)
        })
        .unwrap()
}

// ---------- job add ----------

#[test]
fn add_in_phase_plan_writes_a_draft_job_its_needs_and_an_event() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    let out = job_add(&env, "pl", "a", "S2", &[]).ok();
    assert_eq!(out["job"], json!({"key": "a", "status": "draft", "appended": false, "tier": "S2", "needs": []}));
    let out = job_add(&env, "pl", "b", "O3", &["a"]).ok();
    assert_eq!(out["job"], json!({"key": "b", "status": "draft", "appended": false, "tier": "O3", "needs": ["a"]}));
    assert!(out.get("already").is_none());

    assert_eq!(job_row(&env, "pl", "b", "title"), "Title of b");
    assert_eq!(job_row(&env, "pl", "b", "goal"), "Do it.");
    assert_eq!(job_row(&env, "pl", "b", "context"), "Because.");
    assert_eq!(job_row(&env, "pl", "b", "touches"), "[\"src/a.rs\"]");
    assert_eq!(job_row(&env, "pl", "b", "acceptance"), "[\"cargo test\"]");
    assert_eq!(job_row(&env, "pl", "b", "out_of_scope"), "[\"Everything else.\"]");
    assert_eq!(job_row(&env, "pl", "b", "status"), "draft");
    assert_eq!(job_row(&env, "pl", "b", "appended"), 0);
    assert_eq!(job_row(&env, "pl", "b", "fingerprint"), Value::Null);
    assert!(job_row(&env, "pl", "b", "seq").as_i64() > job_row(&env, "pl", "a", "seq").as_i64());
    assert_eq!(query::<String>(&env, "SELECT needs FROM dependencies WHERE job = 'b'"), "a");

    let added = events(&env, "job-added");
    assert_eq!(added.len(), 2);
    assert_eq!(added[1]["job"], "b");
    assert_eq!(added[1]["plan"], "pl");
    assert_eq!(added[1]["context"], "ctx");
    assert_eq!(added[1]["data"], json!({"status": "draft", "appended": false, "tier": "O3", "needs": ["a"]}));
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM history WHERE tbl = 'jobs' AND op = 'insert'"), 2);
}

#[test]
fn add_in_phase_dispatch_or_pr_appends_a_queued_job_with_its_fingerprint() {
    for phase in ["dispatch", "pr"] {
        let env = TestEnv::new();
        setup(&env);
        plan(&env, "pl", "plan");
        job_add(&env, "pl", "a", "S2", &[]).ok();
        lock(&env, "pl");
        if phase == "pr" {
            fixture(&env, "UPDATE plans SET phase = 'pr', pr_number = 1, pr_url = 'u', pr_opened_at = 't' WHERE id = 'pl';");
        }
        let out = job_add(&env, "pl", "late", "H1", &["a"]).ok();
        assert_eq!(out["job"], json!({"key": "late", "status": "queued", "appended": true, "tier": "H1", "needs": ["a"]}), "{phase}");
        assert_eq!(job_row(&env, "pl", "late", "appended"), 1);
        let id = PlanId::parse("pl").unwrap();
        let all = store(&env).read(|conn| jobs::load(conn, &id)).unwrap();
        let late = all.iter().find(|j| j.key == "late").unwrap();
        assert_eq!(late.fingerprint.as_deref(), Some(locked_fingerprint(late).as_str()), "{phase}");
        assert_eq!(events(&env, "job-added").last().unwrap()["data"]["status"], "queued");
        // It is Locked from its first transaction: nothing can change it.
        assert_eq!(job(&env, &["edit", "late", "--title", "x"], "pl").error()["code"], "job_locked");
    }
}

#[test]
fn add_again_with_the_same_fields_is_already_and_writes_nothing() {
    let env = TestEnv::new();
    drafted(&env);
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
    let out = job_add(&env, "pl", "b", "S2", &["a"]).ok();
    assert_eq!(out["already"], true);
    assert_eq!(out["job"]["key"], "b");
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));

    let run = job_add(&env, "pl", "b", "S3", &["a"]);
    refused(&run, "job_key_exists", 3);
    let run = job_add(&env, "pl", "b", "S2", &[]);
    refused(&run, "job_key_exists", 3);
    let other_spec = write_spec(&env, "other.json", &json!({"goal": "Else.", "context": "c", "touches": ["x"], "acceptance": ["y"], "out_of_scope": []}).to_string());
    let run = env.cpd(&["job", "add", "b", "--title", "Title of b", "--tier", "S2", "--spec", &other_spec, "--needs", "a", "--plan", "pl"]);
    refused(&run, "job_key_exists", 3);
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
}

#[test]
fn add_refusals_write_nothing() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "gr", "grounding");
    plan(&env, "dn", "done");
    plan(&env, "pl", "plan");
    job_add(&env, "pl", "a", "S2", &[]).ok();
    job_add(&env, "pl", "x", "S2", &[]).ok();
    job_add(&env, "pl", "y", "S2", &[]).ok();
    lock(&env, "pl");
    engine(&env, "pl", "x", "failed");
    job(&env, &["cancel", "y", "--reason", "No."], "pl").ok();
    let (txns, evs, rows) = (count(&env, "txns"), count(&env, "events"), count(&env, "jobs"));

    // The phase.
    for plan_id in ["gr", "dn"] {
        let e = refused(&job_add(&env, plan_id, "k", "S2", &[]), "illegal_phase", 3);
        assert_eq!(e["details"]["phase"], if plan_id == "gr" { "grounding" } else { "done" });
    }
    refused(&job_add(&env, "nope", "k", "S2", &[]), "plan_not_found", 4);
    // The needs.
    let e = refused(&job_add(&env, "pl", "k", "S2", &["a", "ghost", "ghost2"]), "needs_unknown", 3);
    assert_eq!(e["details"]["needs"], json!(["ghost", "ghost2"]));
    let e = refused(&job_add(&env, "pl", "k", "S2", &["x", "y", "a"]), "needs_unavailable", 3);
    assert_eq!(e["details"]["needs"], json!([{"key": "x", "status": "failed"}, {"key": "y", "status": "cancelled"}]));
    // The flags.
    let spec = spec_file(&env);
    let e = refused(&env.cpd(&["job", "add", "k", "--title", "T", "--tier", "S0", "--spec", &spec, "--plan", "pl"]), "tier_invalid", 2);
    assert_eq!(e["details"]["efforts"], json!([1, 2, 3, 4, 5]));
    refused(&env.cpd(&["job", "add", "k", "--title", "T", "--tier", "H4", "--spec", &spec, "--plan", "pl"]), "tier_invalid", 2);
    refused(&env.cpd(&["job", "add", "k", "--title", "T", "--tier", "S9", "--spec", &spec, "--plan", "pl"]), "tier_invalid", 2);
    refused(&env.cpd(&["job", "add", "k", "--title", "T", "--tier", "S2", "--spec", "/no/such/file", "--plan", "pl"]), "file_unreadable", 2);
    refused(&env.cpd(&["job", "add", "k", "--title", "T", "--tier", "S2", "--spec", &spec, "--needs", "a,a", "--plan", "pl"]), "usage", 2);
    refused(&env.cpd(&["job", "add", "k", "--title", "T", "--tier", "S2", "--spec", &spec, "--needs", "A", "--plan", "pl"]), "invalid_job_id", 2);
    refused(&env.cpd(&["job", "add", "K", "--title", "T", "--tier", "S2", "--spec", &spec, "--plan", "pl"]), "invalid_job_id", 2);
    refused(&env.cpd(&["job", "add", "k", "--tier", "S2", "--spec", &spec, "--plan", "pl"]), "usage", 2);
    refused(&env.cpd(&["job", "add", "k", "--title", "T", "--tier", "S2", "--spec", &spec]), "plan_required", 2);
    assert_eq!((count(&env, "txns"), count(&env, "events"), count(&env, "jobs")), (txns, evs, rows));
}

#[test]
fn add_refuses_a_spec_that_is_not_complete_naming_the_json_path() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    let good = spec_json();
    let without = |field: &str| {
        let mut v = good.clone();
        v.as_object_mut().unwrap().remove(field);
        v.to_string()
    };
    let with = |field: &str, value: Value| {
        let mut v = good.clone();
        v[field] = value;
        v.to_string()
    };
    let cases = [
        ("{not json".to_owned(), "$"),
        ("[]".to_owned(), "$"),
        (without("goal"), "$.goal"),
        (without("context"), "$.context"),
        (without("touches"), "$.touches"),
        (without("acceptance"), "$.acceptance"),
        (without("out_of_scope"), "$.out_of_scope"),
        (with("outOfScope", json!([])), "$.outOfScope"),
        (with("extra", json!(1)), "$.extra"),
        (with("goal", json!(3)), "$.goal"),
        (with("goal", json!("")), "$.goal"),
        (with("touches", json!([])), "$.touches"),
        (with("touches", json!(["a", ""])), "$.touches[1]"),
        (with("acceptance", json!([])), "$.acceptance"),
        (with("acceptance", json!("x")), "$.acceptance"),
        (with("out_of_scope", json!([4])), "$.out_of_scope[0]"),
    ];
    for (text, path) in cases {
        let file = write_spec(&env, "bad.json", &text);
        let run = env.cpd(&["job", "add", "k", "--title", "T", "--tier", "S2", "--spec", &file, "--plan", "pl"]);
        let e = refused(&run, "spec_invalid", 2);
        assert_eq!(e["details"]["path"], path, "{text}");
    }
    assert_eq!(count(&env, "jobs"), 0);
    // An empty out_of_scope is fine.
    let file = write_spec(&env, "ok.json", &with("out_of_scope", json!([])));
    env.cpd(&["job", "add", "k", "--title", "T", "--tier", "S2", "--spec", &file, "--plan", "pl"]).ok();
}

#[test]
fn the_plan_comes_from_cpd_plan_when_no_flag_names_it() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    let spec = spec_file(&env);
    let out = env.command(common::CPD).env("CPD_PLAN", "pl").args(["job", "add", "k", "--title", "T", "--tier", "S2", "--spec", &spec]).output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stdout));
    assert_eq!(count(&env, "jobs"), 1);
}

// ---------- job edit ----------

#[test]
fn edit_changes_the_given_fields_and_lists_only_those_that_changed() {
    let env = TestEnv::new();
    drafted(&env);
    job_add(&env, "pl", "c", "S2", &[]).ok();
    let spec = write_spec(&env, "new.json", &json!({"goal": "New goal.", "context": "Because.", "touches": ["src/a.rs"], "acceptance": ["cargo test"], "out_of_scope": []}).to_string());

    let run = job(&env, &["edit", "b", "--tier", "O4", "--needs", "a,c", "--title", "Title of b"], "pl");
    let out = run.ok();
    assert_eq!(out["job"], json!({"key": "b", "status": "draft"}));
    assert_eq!(out["fields"], json!(["tier", "needs"]));
    assert_eq!(job_row(&env, "pl", "b", "tier"), "O4");
    assert_eq!(query::<String>(&env, "SELECT group_concat(needs) FROM (SELECT needs FROM dependencies WHERE job = 'b' ORDER BY needs)"), "a,c");

    let out = job(&env, &["edit", "b", "--spec", &spec, "--title", "New title"], "pl").ok();
    assert_eq!(out["fields"], json!(["title", "goal", "out_of_scope"]));
    assert_eq!(job_row(&env, "pl", "b", "goal"), "New goal.");
    assert_eq!(job_row(&env, "pl", "b", "out_of_scope"), "[]");
    assert_eq!(job_row(&env, "pl", "b", "title"), "New title");

    let out = job(&env, &["edit", "b", "--no-needs"], "pl").ok();
    assert_eq!(out["fields"], json!(["needs"]));
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM dependencies WHERE job = 'b'"), 0);

    let edited = events(&env, "job-edited");
    assert_eq!(edited.len(), 3);
    assert_eq!(edited[0]["data"], json!({"fields": ["tier", "needs"]}));
    assert_eq!(edited[0]["job"], "b");
    assert_eq!(edited[2]["data"], json!({"fields": ["needs"]}));
    // `a` was never touched.
    assert_eq!(job_row(&env, "pl", "a", "tier"), "S2");
}

#[test]
fn edit_that_changes_nothing_is_already_and_writes_nothing() {
    let env = TestEnv::new();
    drafted(&env);
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
    let spec = spec_file(&env);
    let out = job(&env, &["edit", "b", "--tier", "S2", "--needs", "a", "--spec", &spec, "--title", "Title of b"], "pl").ok();
    assert_eq!(out["already"], true);
    assert_eq!(out["fields"], json!([]));
    let out = job(&env, &["edit", "a", "--no-needs"], "pl").ok();
    assert_eq!(out["already"], true);
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
}

#[test]
fn edit_refusals_write_nothing() {
    let env = TestEnv::new();
    drafted(&env);
    job_add(&env, "pl", "c", "S2", &["b"]).ok();
    job_add(&env, "pl", "gone", "S2", &[]).ok();
    job(&env, &["cancel", "gone", "--reason", "No."], "pl").ok();
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));

    let run = job(&env, &["edit", "b"], "pl");
    refused(&run, "nothing_to_edit", 2);
    refused(&job(&env, &["edit", "b", "--needs", "a", "--no-needs"], "pl"), "usage", 2);
    refused(&job(&env, &["edit", "ghost", "--tier", "S3"], "pl"), "job_not_found", 4);
    refused(&job(&env, &["edit", "b", "--tier", "S3"], "nope"), "plan_not_found", 4);
    refused(&job(&env, &["edit", "b", "--needs", "ghost"], "pl"), "needs_unknown", 3);
    let e = refused(&job(&env, &["edit", "b", "--needs", "gone"], "pl"), "needs_unavailable", 3);
    assert_eq!(e["details"]["needs"], json!([{"key": "gone", "status": "cancelled"}]));
    let e = refused(&job(&env, &["edit", "a", "--needs", "c"], "pl"), "dependency_cycle", 3);
    assert_eq!(e["details"]["cycle"], json!(["a", "c", "b", "a"]));
    refused(&job(&env, &["edit", "a", "--needs", "b"], "pl"), "dependency_cycle", 3);
    refused(&job(&env, &["edit", "a", "--needs", "a"], "pl"), "dependency_cycle", 3);
    refused(&job(&env, &["edit", "b", "--tier", "S0"], "pl"), "tier_invalid", 2);
    refused(&job(&env, &["edit", "b", "--spec", "/no/such/file"], "pl"), "file_unreadable", 2);
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM dependencies"), 2);
}

#[test]
fn a_locked_job_cannot_be_edited_by_the_command_or_by_the_database() {
    let env = TestEnv::new();
    drafted(&env);
    lock(&env, "pl");
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
    for args in [&["edit", "a", "--title", "New"][..], &["edit", "b", "--no-needs"], &["edit", "b", "--needs", "a"], &["edit", "a", "--tier", "S3"]] {
        let e = refused(&job(&env, args, "pl"), "job_locked", 3);
        assert_eq!(e["details"]["status"], "queued", "{args:?}");
    }
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
    // The trigger of §4.4 refuses the same writes.
    let result = store(&env).write("fixture", |tx| exec(tx, "UPDATE jobs SET goal = 'Else.' WHERE key = 'a';"));
    assert_eq!(result.unwrap_err().code(), cpd::error::ErrorCode::JobLocked);
    let result = store(&env).write("fixture", |tx| exec(tx, "DELETE FROM dependencies WHERE job = 'b';"));
    assert_eq!(result.unwrap_err().code(), cpd::error::ErrorCode::JobLocked);
    // Other statuses are Locked too.
    engine(&env, "pl", "a", "running");
    refused(&job(&env, &["edit", "a", "--title", "New"], "pl"), "job_locked", 3);
}

// ---------- job cancel ----------

#[test]
fn cancel_ends_a_draft_queued_or_failed_job_at_once() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    job_add(&env, "pl", "d", "S2", &[]).ok();
    let out = job(&env, &["cancel", "d", "--reason", "Not needed."], "pl").ok();
    assert_eq!(out, json!({"ok": true, "job": "d", "status": "cancelled", "cancel_requested": false}));
    assert_eq!(job_row(&env, "pl", "d", "status"), "cancelled");
    assert_eq!(job_row(&env, "pl", "d", "cancel_reason"), "Not needed.");
    assert!(job_row(&env, "pl", "d", "cancel_requested_at").is_string());
    assert_eq!(job_row(&env, "pl", "d", "fingerprint"), Value::Null);

    job_add(&env, "pl", "q", "S2", &[]).ok();
    job_add(&env, "pl", "f", "S2", &[]).ok();
    job_add(&env, "pl", "dep", "S2", &["f"]).ok();
    lock(&env, "pl");
    engine(&env, "pl", "f", "failed");
    job(&env, &["cancel", "q", "--reason", "Queued no more."], "pl").ok();
    let out = job(&env, &["cancel", "f", "--reason", "Gave up."], "pl").ok();
    assert_eq!(out["status"], "cancelled");
    assert_eq!(job_row(&env, "pl", "f", "failed_cause"), Value::Null);
    // Dependents stay blocked: cancelling does not touch them.
    assert_eq!(job_row(&env, "pl", "dep", "status"), "queued");

    let cancelled = events(&env, "job-cancelled");
    assert_eq!(cancelled.len(), 3);
    assert_eq!(cancelled[0]["data"], json!({"reason": "Not needed."}));
    assert_eq!(cancelled[0]["job"], "d");
    assert_eq!(cancelled[0]["attempt"], Value::Null);
}

#[test]
fn cancel_of_a_running_job_only_records_the_request() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    job_add(&env, "pl", "w", "S2", &[]).ok();
    lock(&env, "pl");
    engine(&env, "pl", "w", "running");

    let out = job(&env, &["cancel", "w", "--reason", "Wrong approach."], "pl").ok();
    assert_eq!(out, json!({"ok": true, "job": "w", "status": "running", "cancel_requested": true}));
    assert_eq!(job_row(&env, "pl", "w", "status"), "running");
    assert_eq!(job_row(&env, "pl", "w", "cancel_reason"), "Wrong approach.");
    assert!(job_row(&env, "pl", "w", "cancel_requested_at").is_string());
    assert_eq!(query::<Option<String>>(&env, "SELECT stop_request FROM attempts WHERE id = 'w-a1'"), None, "E2 sets the stop request, not the command");
    assert_eq!(events(&env, "job-cancelled").len(), 0);
    let requested = events(&env, "job-cancel-requested");
    assert_eq!(requested.len(), 1);
    assert_eq!(requested[0]["data"], json!({"reason": "Wrong approach."}));
    assert_eq!(requested[0]["attempt"], "w-a1");

    // A second request keeps the first reason.
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
    let out = job(&env, &["cancel", "w", "--reason", "Another reason."], "pl").ok();
    assert_eq!(out, json!({"ok": true, "job": "w", "status": "running", "cancel_requested": true, "already": true}));
    assert_eq!(job_row(&env, "pl", "w", "cancel_reason"), "Wrong approach.");
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
}

#[test]
fn cancel_again_is_already_and_a_merged_job_is_refused() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    job_add(&env, "pl", "d", "S2", &[]).ok();
    job_add(&env, "pl", "m", "S2", &[]).ok();
    job(&env, &["cancel", "d", "--reason", "First."], "pl").ok();
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
    let out = job(&env, &["cancel", "d", "--reason", "Second."], "pl").ok();
    assert_eq!(out, json!({"ok": true, "job": "d", "status": "cancelled", "cancel_requested": false, "already": true}));
    assert_eq!(job_row(&env, "pl", "d", "cancel_reason"), "First.");
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));

    lock(&env, "pl");
    engine(&env, "pl", "m", "merged");
    let e = refused(&job(&env, &["cancel", "m", "--reason", "Too late."], "pl"), "job_merged", 3);
    assert_eq!(e["details"]["job"], "m");
    refused(&job(&env, &["cancel", "ghost", "--reason", "x"], "pl"), "job_not_found", 4);
    refused(&job(&env, &["cancel", "m", "--reason", "x"], "nope"), "plan_not_found", 4);
    refused(&job(&env, &["cancel", "m"], "pl"), "usage", 2);
    assert_eq!(job_row(&env, "pl", "m", "status"), "merged");
}

// ---------- job retry ----------

#[test]
fn retry_queues_a_failed_job_at_the_given_tier() {
    let env = TestEnv::new();
    drafted(&env);
    lock(&env, "pl");
    engine(&env, "pl", "a", "failed");
    let out = job(&env, &["retry", "a", "--tier", "O4"], "pl").ok();
    assert_eq!(out, json!({"ok": true, "job": "a", "status": "queued", "tier": "O4"}));
    assert_eq!(job_row(&env, "pl", "a", "status"), "queued");
    assert_eq!(job_row(&env, "pl", "a", "retry_tier"), "O4");
    assert_eq!(job_row(&env, "pl", "a", "failed_cause"), Value::Null);
    assert_eq!(job_row(&env, "pl", "a", "tier"), "S2", "the Job's own Tier is Locked");
    let retried = events(&env, "job-retried");
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0]["data"], json!({"tier": "O4"}));
    assert_eq!(retried[0]["job"], "a");

    // Again with the same Tier and no Attempt since: already, nothing written.
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
    let out = job(&env, &["retry", "a", "--tier", "O4"], "pl").ok();
    assert_eq!(out["already"], true);
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
    // Another Tier is not the same retry.
    refused(&job(&env, &["retry", "a", "--tier", "O5"], "pl"), "job_not_failed", 3);
    // Once an Attempt has started since, the retry is spent: the Job is only queued again by the ladder.
    fixture(
        &env,
        "INSERT INTO events (txn, at, kind, context, plan, job, attempt, text, data)
         SELECT id, at, 'attempt-started', 'ctx', 'pl', 'a', 'a-a2', 'Started.', '{}' FROM txns WHERE id = cpd_txn();",
    );
    refused(&job(&env, &["retry", "a", "--tier", "O4"], "pl"), "job_not_failed", 3);
}

#[test]
fn retry_refusals_write_nothing() {
    let env = TestEnv::new();
    drafted(&env);
    job_add(&env, "pl", "c", "S2", &[]).ok();
    lock(&env, "pl");
    engine(&env, "pl", "a", "running");
    job(&env, &["cancel", "c", "--reason", "No."], "pl").ok();
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));

    let e = refused(&job(&env, &["retry", "b", "--tier", "S2"], "pl"), "job_not_failed", 3);
    assert_eq!(e["details"]["status"], "queued");
    let e = refused(&job(&env, &["retry", "a", "--tier", "S2"], "pl"), "job_not_failed", 3);
    assert_eq!(e["details"]["status"], "running");
    refused(&job(&env, &["retry", "c", "--tier", "S2"], "pl"), "job_not_failed", 3);
    refused(&job(&env, &["retry", "ghost", "--tier", "S2"], "pl"), "job_not_found", 4);
    refused(&job(&env, &["retry", "b", "--tier", "S2"], "nope"), "plan_not_found", 4);
    let e = refused(&job(&env, &["retry", "a", "--tier", "S0"], "pl"), "tier_invalid", 2);
    assert_eq!(e["details"]["efforts"], json!([1, 2, 3, 4, 5]));
    refused(&job(&env, &["retry", "a", "--tier", "Z1"], "pl"), "tier_invalid", 2);
    refused(&job(&env, &["retry", "a"], "pl"), "usage", 2);
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
}

#[test]
fn retry_refuses_a_job_whose_fingerprint_does_not_match() {
    let env = TestEnv::new();
    drafted(&env);
    lock(&env, "pl");
    engine(&env, "pl", "a", "failed");
    // Written around the trigger, the way a tamper would be; the schema is whole again afterwards.
    let trigger: String = query(&env, "SELECT sql FROM sqlite_schema WHERE name = 'jobs_locked'");
    fixture(&env, &format!("DROP TRIGGER jobs_locked; UPDATE jobs SET goal = 'Changed.' WHERE key = 'a'; {trigger};"));
    let e = refused(&job(&env, &["retry", "a", "--tier", "S3"], "pl"), "locked_job_tampered", 3);
    assert_eq!(e["details"]["job"], "a");
    assert_eq!(job_row(&env, "pl", "a", "status"), "failed");
    let shown = job(&env, &["show", "a"], "pl").ok();
    assert_eq!(shown["tamper"][0]["kind"], "fingerprint");
    assert_eq!(shown["tamper"][0]["job"], "a");
    // Cancelling is the way out of a tampered Job.
    job(&env, &["cancel", "a", "--reason", "Tampered."], "pl").ok();
}

// ---------- job list ----------

#[test]
fn list_shows_every_job_in_creation_order_and_filters_by_status() {
    let env = TestEnv::new();
    drafted(&env);
    job_add(&env, "pl", "c", "O3", &[]).ok();
    lock(&env, "pl");
    engine(&env, "pl", "a", "merged");
    engine(&env, "pl", "c", "failed");

    let out = job(&env, &["list"], "pl").ok();
    let jobs = out["jobs"].as_array().unwrap();
    let keys: Vec<&str> = jobs.iter().map(|j| j["key"].as_str().unwrap()).collect();
    assert_eq!(keys, ["a", "b", "c"]);
    assert_eq!(
        jobs[0],
        json!({"key": "a", "title": "Title of a", "status": "merged", "tier": "S2", "appended": false, "needs": [], "attempts": 1, "merged_sha": "c0ffee"})
    );
    assert_eq!(jobs[1], json!({"key": "b", "title": "Title of b", "status": "queued", "tier": "S2", "appended": false, "needs": ["a"], "attempts": 0, "merged_sha": null}));
    assert_eq!(jobs[2]["attempts"], 1);

    let out = job(&env, &["list", "--status", "queued"], "pl").ok();
    assert_eq!(out["jobs"].as_array().unwrap().len(), 1);
    let out = job(&env, &["list", "--status", "queued", "--status", "failed"], "pl").ok();
    let keys: Vec<&str> = out["jobs"].as_array().unwrap().iter().map(|j| j["key"].as_str().unwrap()).collect();
    assert_eq!(keys, ["b", "c"]);
    assert_eq!(job(&env, &["list", "--status", "draft"], "pl").ok()["jobs"], json!([]));
    refused(&job(&env, &["list", "--status", "done"], "pl"), "usage", 2);
    refused(&job(&env, &["list"], "nope"), "plan_not_found", 4);
    refused(&env.cpd(&["job", "list"]), "plan_required", 2);
}

#[test]
fn list_and_show_write_nothing() {
    let env = TestEnv::new();
    drafted(&env);
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
    job(&env, &["list"], "pl").ok();
    job(&env, &["show", "a"], "pl").ok();
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
}

// ---------- job show ----------

#[test]
fn show_has_every_field_the_needs_and_the_attempts_with_their_escalations() {
    let env = TestEnv::new();
    drafted(&env);
    lock(&env, "pl");
    fixture(
        &env,
        "UPDATE jobs SET status = 'running' WHERE key = 'a';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, check_tail, metrics, started_at)
         VALUES ('pl', 'a-a1', 'a', 1, 'S2', 'ladder', 'running', 'cpd/pl--a-a1', '/w', 'tail', '{\"turns\": 3}', 't');
         INSERT INTO escalations (plan, id, attempt, number, question, status, answer, asker_pid, asked_at, answered_at)
         VALUES ('pl', 'a-a1-q1', 'a-a1', 1, 'Which?', 'answered', 'That.', 7, 't1', 't2');
         INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
         VALUES ('pl', 'a-a1-q2', 'a-a1', 2, 'Again?', 'open', 8, 't3');
         UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'timed_out', ended_at = 't' WHERE id = 'a-a1';
         UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE key = 'a';",
    );
    let out = job(&env, &["show", "a"], "pl").ok();
    let job_json = &out["job"];
    assert_eq!(job_json["key"], "a");
    assert_eq!(job_json["status"], "failed");
    assert_eq!(job_json["failed_cause"], "timed_out");
    assert_eq!(job_json["tier"], "S2");
    assert_eq!(job_json["goal"], "Do it.");
    assert_eq!(job_json["touches"], json!(["src/a.rs"]));
    assert_eq!(job_json["fingerprint"].as_str().unwrap().len(), 64);
    for field in ["seq", "title", "context", "acceptance", "out_of_scope", "appended", "retry_tier", "cancel_reason", "cancel_requested_at", "merged_sha", "created_at"] {
        assert!(job_json.get(field).is_some(), "{field}");
    }
    assert_eq!(out["needs"], json!([]));
    assert_eq!(out["needed_by"], json!(["b"]));
    assert_eq!(out["tamper"], json!([]));

    let attempts = out["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 1);
    let attempt = &attempts[0];
    assert_eq!(attempt["id"], "a-a1");
    assert_eq!((attempt["status"].as_str(), attempt["outcome"].as_str(), attempt["cause"].as_str()), (Some("ended"), Some("failed"), Some("timed_out")));
    assert_eq!(attempt["check_tail"], "tail");
    assert_eq!(attempt["metrics"], json!({"turns": 3}));
    for field in ["number", "tier", "origin", "branch", "worktree", "base_sha", "driver_pid", "worker_session", "spec_path", "submitted_sha", "submitted_at", "merged_sha", "nudged", "stop_request", "started_at", "ended_at"] {
        assert!(attempt.get(field).is_some(), "{field}");
    }
    let escalations = attempt["escalations"].as_array().unwrap();
    assert_eq!(escalations.len(), 2);
    assert_eq!((escalations[0]["id"].as_str(), escalations[0]["status"].as_str(), escalations[0]["answer"].as_str()), (Some("a-a1-q1"), Some("answered"), Some("That.")));
    assert_eq!(escalations[1]["status"], "open");

    let out = job(&env, &["show", "b"], "pl").ok();
    assert_eq!(out["needs"], json!(["a"]));
    assert_eq!(out["needed_by"], json!([]));
    assert_eq!(out["attempts"], json!([]));
    refused(&job(&env, &["show", "ghost"], "pl"), "job_not_found", 4);
    refused(&job(&env, &["show", "b"], "nope"), "plan_not_found", 4);
    refused(&env.cpd(&["job", "show", "b"]), "plan_required", 2);
    refused(&env.cpd(&["job", "show", "B", "--plan", "pl"]), "invalid_job_id", 2);
}
