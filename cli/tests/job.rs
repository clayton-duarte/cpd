//! Jobs (contract §2.3, §3.2; commands.md §5.1–§5.4, §5.6, §5.7), run as the
//! built binary against a temporary `$CPD_HOME`. Plans, Attempts and the
//! states only the engine reaches are written straight into the database, as
//! fixtures; the commands under test are the only ones that write Jobs.
//! pi is a fake install whose Tier catalog is seeded in the cache, so no node runs.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{Run, TestEnv};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use cpd_core::plan::jobs;
use cpd_core::plan::locked_fingerprint;
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};

// ---------- fixtures ----------

fn store(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

/// The pull request row (`acme/<repo>#<number>`, migration 5) that a PR in status `open` (or `merged`) points to.
fn pull_request(repo: &str, number: i64, state: &str) -> String {
    let merged = if state == "merged" { "'t'" } else { "NULL" };
    format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
         VALUES ('acme/{repo}', {number}, 'https://github.com/acme/{repo}/pull/{number}', 'T', 0, '{state}', 'none', 'none', 't', {merged}, 't');"
    )
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
    // (phase, what else the Plan gets, its PR's status and what else the PR gets)
    const STEPS: [(&str, &str, &str, &str); 4] = [
        ("plan", "", "waiting", ""),
        ("dispatch", ", locked_at = 't'", "dispatch", ", lead_worktree = '/wt', started_at = 't'"),
        ("deploy", "", "open", ", pr_number = 1, pr_url = 'https://github.com/acme/{id}/pull/1', pr_opened_at = 't', pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/{id}')"),
        ("done", ", done_at = 't'", "merged", ", merged_at = 't'"),
    ];
    let mut sql = format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at)
         VALUES ('{id}', 'ctx', 'Plan {id}', 'grounding', 'lead-{id}', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
         VALUES ('{id}', 'repo', '/repo', 'main', 'waiting', 'cpd/{id}', cpd_txn(), 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
         VALUES ('lead-{id}', 'lead', 'ctx', '{id}', '{id}', '/repo', 'O3', 'stopped', 't', 't');"
    );
    if phase != "grounding" {
        let last = STEPS.iter().position(|(p, ..)| *p == phase).unwrap_or_else(|| panic!("{phase}"));
        for (step, set, status, pr_set) in &STEPS[..=last] {
            sql += &format!("UPDATE plans SET phase = '{step}'{set} WHERE id = '{id}';");
            if *step == "deploy" {
                sql += &pull_request(id, 1, "open");
            }
            sql += &format!("UPDATE prs SET status = '{status}'{} WHERE plan = '{id}';", pr_set.replace("{id}", id));
            if *step == "done" {
                sql += &format!("UPDATE pull_requests SET state = 'merged', merged_at = 't' WHERE repo = 'acme/{id}';");
            }
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
    let mut sql = format!(
        "UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = '{plan_id}';
         UPDATE prs SET status = 'dispatch', lead_worktree = '/wt', started_at = 't' WHERE plan = '{plan_id}';"
    );
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
    assert_eq!(out["job"], json!({"key": "a", "pr": "repo", "status": "draft", "appended": false, "tier": "S2", "needs": [], "review": false}));
    let out = job_add(&env, "pl", "b", "O3", &["a"]).ok();
    assert_eq!(out["job"], json!({"key": "b", "pr": "repo", "status": "draft", "appended": false, "tier": "O3", "needs": ["a"], "review": false}));
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
    assert_eq!(added[1]["data"], json!({"pr": "repo", "status": "draft", "appended": false, "tier": "O3", "needs": ["a"], "review": false}));
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM history WHERE tbl = 'jobs' AND op = 'insert'"), 2);
}

#[test]
fn add_in_phase_dispatch_or_deploy_appends_a_queued_job_with_its_fingerprint() {
    for phase in ["dispatch", "deploy"] {
        let env = TestEnv::new();
        setup(&env);
        plan(&env, "pl", "plan");
        job_add(&env, "pl", "a", "S2", &[]).ok();
        lock(&env, "pl");
        if phase == "deploy" {
            fixture(
                &env,
                &format!(
                    "UPDATE plans SET phase = 'deploy' WHERE id = 'pl';
                     {}
                     UPDATE prs SET status = 'open', pr_number = 1, pr_url = 'https://github.com/acme/pl/pull/1', pr_opened_at = 't',
                            pull_request = (SELECT id FROM pull_requests WHERE number = 1) WHERE plan = 'pl';",
                    pull_request("pl", 1, "open")
                ),
            );
        }
        let out = job_add(&env, "pl", "late", "H1", &["a"]).ok();
        assert_eq!(out["job"], json!({"key": "late", "pr": "repo", "status": "queued", "appended": true, "tier": "H1", "needs": ["a"], "review": false}), "{phase}");
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
    assert_eq!(result.unwrap_err().code(), cpd_core::error::ErrorCode::JobLocked);
    let result = store(&env).write("fixture", |tx| exec(tx, "DELETE FROM dependencies WHERE job = 'b';"));
    assert_eq!(result.unwrap_err().code(), cpd_core::error::ErrorCode::JobLocked);
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
        json!({"key": "a", "title": "Title of a", "status": "merged", "tier": "S2", "appended": false, "needs": [], "attempts": 1, "merged_sha": "c0ffee", "review": false})
    );
    assert_eq!(jobs[1], json!({"key": "b", "title": "Title of b", "status": "queued", "tier": "S2", "appended": false, "needs": ["a"], "attempts": 0, "merged_sha": null, "review": false}));
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

// ---------- --pr (R34, commands.md §1.3, §5.1, §5.2) ----------

/// Adds PR `key` to Plan `plan_id` in `status`, walking the statuses the way the engine would.
fn add_pr(env: &TestEnv, plan_id: &str, key: &str, status: &str) {
    const STEPS: [(&str, &str); 3] = [
        ("dispatch", ", lead_worktree = '/wt', started_at = 't'"),
        ("open", ", pr_number = 1, pr_url = 'https://github.com/acme/{repo}/pull/1', pr_opened_at = 't', pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/{repo}')"),
        ("merged", ", merged_at = 't'"),
    ];
    let repo = format!("{plan_id}-{key}");
    let mut sql = format!(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
         VALUES ('{plan_id}', '{key}', '/{key}', 'main', 'waiting', 'cpd/{plan_id}/{key}', cpd_txn(), 't');"
    );
    match status {
        "waiting" => {}
        "cancelled" => sql += &format!("UPDATE prs SET status = 'cancelled' WHERE plan = '{plan_id}' AND key = '{key}';"),
        _ => {
            let last = STEPS.iter().position(|(s, _)| *s == status).unwrap_or_else(|| panic!("{status}"));
            for (step, set) in &STEPS[..=last] {
                if *step == "open" {
                    sql += &pull_request(&repo, 1, "open");
                }
                sql += &format!("UPDATE prs SET status = '{step}'{} WHERE plan = '{plan_id}' AND key = '{key}';", set.replace("{repo}", &repo));
                if *step == "merged" {
                    sql += &format!("UPDATE pull_requests SET state = 'merged', merged_at = 't' WHERE repo = 'acme/{repo}';");
                }
            }
        }
    }
    fixture(env, &sql);
}

fn job_add_in(env: &TestEnv, plan_id: &str, key: &str, pr: Option<&str>, needs: &[&str]) -> Run {
    let spec = spec_file(env);
    let title = format!("Title of {key}");
    let needs = needs.join(",");
    let mut args = vec!["job", "add", key, "--title", &title, "--tier", "S2", "--spec", &spec, "--plan", plan_id];
    if let Some(pr) = pr {
        args.extend(["--pr", pr]);
    }
    if !needs.is_empty() {
        args.extend(["--needs", &needs]);
    }
    env.cpd(&args)
}

/// Plan `pl` in phase `plan` with PRs `repo` (the fixture's) and `web`, and a draft Job `a` in `repo`.
fn two_prs(env: &TestEnv) {
    setup(env);
    plan(env, "pl", "plan");
    add_pr(env, "pl", "web", "waiting");
    job_add_in(env, "pl", "a", Some("repo"), &[]).ok();
}

#[test]
fn add_defaults_to_the_one_pr_and_names_the_pr_it_was_given() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    // One PR: `--pr` may be omitted or given.
    assert_eq!(job_add_in(&env, "pl", "a", None, &[]).ok()["job"]["pr"], "repo");
    assert_eq!(job_add_in(&env, "pl", "b", Some("repo"), &["a"]).ok()["job"]["pr"], "repo");
    // A cancelled PR does not count: the one that is not cancelled is the default.
    add_pr(&env, "pl", "old", "cancelled");
    assert_eq!(job_add_in(&env, "pl", "c", None, &[]).ok()["job"]["pr"], "repo");

    add_pr(&env, "pl", "web", "waiting");
    let out = job_add_in(&env, "pl", "w", Some("web"), &[]).ok();
    assert_eq!(out["job"]["pr"], "web");
    assert_eq!(job_row(&env, "pl", "w", "pr"), "web");
    assert_eq!(job_row(&env, "pl", "a", "pr"), "repo");
    assert_eq!(events(&env, "job-added").last().unwrap()["data"]["pr"], "web");

    // Rerun: the same PR is `already`, another PR is `job_key_exists`.
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
    assert_eq!(job_add_in(&env, "pl", "w", Some("web"), &[]).ok()["already"], true);
    refused(&job_add_in(&env, "pl", "w", Some("repo"), &[]), "job_key_exists", 3);
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
}

#[test]
fn add_refuses_a_missing_unknown_or_ended_pr_and_writes_nothing() {
    let env = TestEnv::new();
    two_prs(&env);
    add_pr(&env, "pl", "gone", "cancelled");
    let (txns, evs, rows) = (count(&env, "txns"), count(&env, "events"), count(&env, "jobs"));

    // Two PRs that are not cancelled: `--pr` is required.
    let e = refused(&job_add_in(&env, "pl", "k", None, &[]), "pr_required", 2);
    assert_eq!(e["details"]["prs"], json!(["repo", "web"]));
    let e = refused(&job_add_in(&env, "pl", "k", Some("nope"), &[]), "pr_not_found", 4);
    assert_eq!(e["details"]["pr"], "nope");
    let e = refused(&job_add_in(&env, "pl", "k", Some("gone"), &[]), "pr_ended", 3);
    assert_eq!(e["details"]["status"], "cancelled");
    refused(&job_add_in(&env, "pl", "k", Some("Web"), &[]), "invalid_pr_id", 2);
    assert_eq!((count(&env, "txns"), count(&env, "events"), count(&env, "jobs")), (txns, evs, rows));

    // No PR that is not cancelled: `pr_required` too, and its message names `cpd pr add`.
    fixture(&env, "UPDATE jobs SET status = 'cancelled', cancel_reason = 'x', cancel_requested_at = 't' WHERE plan = 'pl';
                   UPDATE prs SET status = 'cancelled' WHERE plan = 'pl';");
    let e = refused(&job_add_in(&env, "pl", "k", None, &[]), "pr_required", 2);
    assert!(e["message"].as_str().unwrap().contains("cpd pr add"), "{e}");
}

#[test]
fn add_into_a_merged_pr_is_refused_and_into_a_waiting_pr_is_appended() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    add_pr(&env, "pl", "web", "waiting");
    job_add_in(&env, "pl", "a", Some("repo"), &[]).ok();
    lock(&env, "pl");
    // `lock` starts every PR; put `web` back as the engine would find a PR waiting on `repo`.
    fixture(
        &env,
        &format!(
            "{} {}
             UPDATE prs SET status = 'open', pr_number = 1, pr_url = 'https://github.com/acme/repo/pull/1', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/repo') WHERE plan = 'pl' AND key = 'repo';
             UPDATE prs SET status = 'open', pr_number = 2, pr_url = 'https://github.com/acme/web/pull/2', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/web') WHERE plan = 'pl' AND key = 'web';
             UPDATE prs SET status = 'merged', merged_at = 't' WHERE plan = 'pl' AND key = 'repo';",
            pull_request("repo", 1, "merged"),
            pull_request("web", 2, "open")
        ),
    );
    let e = refused(&job_add_in(&env, "pl", "k", Some("repo"), &[]), "pr_ended", 3);
    assert_eq!(e["details"]["status"], "merged");
    let out = job_add_in(&env, "pl", "k", Some("web"), &[]).ok();
    assert_eq!(out["job"]["pr"], "web");
    assert_eq!(out["job"]["status"], "queued");
    assert_eq!(out["job"]["appended"], true);
}

#[test]
fn add_needs_must_be_jobs_of_the_same_pr() {
    let env = TestEnv::new();
    two_prs(&env);
    job_add_in(&env, "pl", "w", Some("web"), &[]).ok();
    job_add_in(&env, "pl", "a2", Some("repo"), &["a"]).ok();
    let (txns, evs, rows) = (count(&env, "txns"), count(&env, "events"), count(&env, "jobs"));

    let e = refused(&job_add_in(&env, "pl", "k", Some("web"), &["w", "a"]), "needs_other_pr", 3);
    assert_eq!(e["details"]["jobs"], json!(["a"]));
    let e = refused(&job_add_in(&env, "pl", "k", Some("repo"), &["w", "a", "a2"]), "needs_other_pr", 3);
    assert_eq!(e["details"]["jobs"], json!(["w"]));
    // An unknown need is reported first.
    refused(&job_add_in(&env, "pl", "k", Some("web"), &["a", "ghost"]), "needs_unknown", 3);
    assert_eq!((count(&env, "txns"), count(&env, "events"), count(&env, "jobs")), (txns, evs, rows));
    job_add_in(&env, "pl", "w2", Some("web"), &["w"]).ok();
}

#[test]
fn edit_moves_a_draft_job_to_another_pr() {
    let env = TestEnv::new();
    two_prs(&env);
    job_add_in(&env, "pl", "w", Some("web"), &[]).ok();
    job_add_in(&env, "pl", "b", Some("repo"), &[]).ok();

    let out = job(&env, &["edit", "b", "--pr", "web", "--needs", "w"], "pl").ok();
    assert_eq!(out["fields"], json!(["pr", "needs"]));
    assert_eq!(job_row(&env, "pl", "b", "pr"), "web");
    assert_eq!(query::<String>(&env, "SELECT needs FROM dependencies WHERE job = 'b'"), "w");
    assert_eq!(events(&env, "job-edited").last().unwrap()["data"], json!({"fields": ["pr", "needs"]}));

    // A Job with old needs may move with `--no-needs`.
    let out = job(&env, &["edit", "b", "--pr", "repo", "--no-needs"], "pl").ok();
    assert_eq!(out["fields"], json!(["pr", "needs"]));
    assert_eq!(query::<i64>(&env, "SELECT count(*) FROM dependencies"), 0);
    // A Job with no needs and no dependents moves alone.
    let out = job(&env, &["edit", "b", "--pr", "web"], "pl").ok();
    assert_eq!(out["fields"], json!(["pr"]));
    assert_eq!(job_row(&env, "pl", "b", "pr"), "web");

    // The PR it is already in changes nothing, whatever its needs.
    job(&env, &["edit", "b", "--needs", "w"], "pl").ok();
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
    let out = job(&env, &["edit", "b", "--pr", "web"], "pl").ok();
    assert_eq!(out["already"], true);
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
}

#[test]
fn edit_pr_refusals_write_nothing() {
    let env = TestEnv::new();
    two_prs(&env);
    add_pr(&env, "pl", "gone", "cancelled");
    job_add_in(&env, "pl", "w", Some("web"), &[]).ok();
    job_add_in(&env, "pl", "b", Some("repo"), &["a"]).ok();
    job_add_in(&env, "pl", "c", Some("repo"), &[]).ok();
    job(&env, &["cancel", "c", "--reason", "No."], "pl").ok();
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));

    let e = refused(&job(&env, &["edit", "a", "--pr", "nope"], "pl"), "pr_not_found", 4);
    assert_eq!(e["details"]["pr"], "nope");
    let e = refused(&job(&env, &["edit", "a", "--pr", "gone"], "pl"), "pr_ended", 3);
    assert_eq!(e["details"]["status"], "cancelled");
    refused(&job(&env, &["edit", "a", "--pr", "Web"], "pl"), "invalid_pr_id", 2);
    refused(&job(&env, &["edit", "a", "--pr", "web", "--needs", "ghost"], "pl"), "needs_unknown", 3);
    refused(&job(&env, &["edit", "a", "--pr", "web", "--needs", "c"], "pl"), "needs_unavailable", 3);
    // The Job keeps no old needs: `b` needs `a`.
    let e = refused(&job(&env, &["edit", "b", "--pr", "web"], "pl"), "needs_other_pr", 3);
    assert_eq!(e["details"]["jobs"], json!(["a"]));
    // No Job may need it: `b` needs `a`.
    let e = refused(&job(&env, &["edit", "a", "--pr", "web"], "pl"), "needs_other_pr", 3);
    assert_eq!(e["details"]["jobs"], json!(["b"]));
    let e = refused(&job(&env, &["edit", "a", "--pr", "web", "--no-needs"], "pl"), "needs_other_pr", 3);
    assert_eq!(e["details"]["jobs"], json!(["b"]));
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
    // The new needs are Jobs of the new PR.
    job_add_in(&env, "pl", "w2", Some("web"), &[]).ok();
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
    let e = refused(&job(&env, &["edit", "w", "--pr", "repo", "--needs", "w2", "--tier", "S3"], "pl"), "needs_other_pr", 3);
    assert_eq!(e["details"]["jobs"], json!(["w2"]));
    // Without `--pr` the needs are those of the Job's own PR.
    let e = refused(&job(&env, &["edit", "w", "--needs", "a"], "pl"), "needs_other_pr", 3);
    assert_eq!(e["details"]["jobs"], json!(["a"]));
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
    assert_eq!(job_row(&env, "pl", "a", "pr"), "repo");
}

#[test]
fn a_locked_job_cannot_change_pr() {
    let env = TestEnv::new();
    two_prs(&env);
    lock(&env, "pl");
    refused(&job(&env, &["edit", "a", "--pr", "web"], "pl"), "job_locked", 3);
    assert_eq!(job_row(&env, "pl", "a", "pr"), "repo");
}

// ---------- drafts in a running Plan (hs-d173, hs-d174, hs-d179; commands.md §5.1–§5.3) ----------

/// `job add <key> --draft` in Plan `plan_id`, with `needs` and the `extra` flags.
fn job_add_draft(env: &TestEnv, plan_id: &str, key: &str, needs: &[&str], extra: &[&str]) -> Run {
    let spec = spec_file(env);
    let title = format!("Title of {key}");
    let needs = needs.join(",");
    let mut args = vec!["job", "add", key, "--title", &title, "--tier", "S2", "--spec", &spec, "--draft", "--plan", plan_id];
    if !needs.is_empty() {
        args.extend(["--needs", &needs]);
    }
    args.extend(extra);
    env.cpd(&args)
}

#[test]
fn add_draft_after_the_lock_is_an_appended_draft_that_stays_editable_and_cancellable() {
    for phase in ["dispatch", "deploy"] {
        let env = TestEnv::new();
        setup(&env);
        plan(&env, "pl", phase);
        job_add(&env, "pl", "a", "S2", &[]).ok();

        let out = job_add_draft(&env, "pl", "d", &["a"], &[]).ok();
        assert_eq!(out["job"], json!({"key": "d", "pr": "repo", "status": "draft", "appended": true, "tier": "S2", "needs": ["a"], "review": false}), "{phase}");
        assert_eq!((job_row(&env, "pl", "d", "status"), job_row(&env, "pl", "d", "appended")), (json!("draft"), json!(1)));
        assert_eq!(job_row(&env, "pl", "d", "fingerprint"), Value::Null);
        assert_eq!(
            events(&env, "job-added").last().unwrap()["data"],
            json!({"pr": "repo", "status": "draft", "appended": true, "tier": "S2", "needs": ["a"], "review": false})
        );
        // Its needs may be drafts; an identical rerun is `already`, as for any add.
        job_add_draft(&env, "pl", "e", &["d"], &[]).ok();
        let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
        assert_eq!(job_add_draft(&env, "pl", "e", &["d"], &[]).ok()["already"], true);
        assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));

        // Editable and cancellable in this phase.
        let out = job(&env, &["edit", "d", "--title", "New title", "--tier", "S3"], "pl").ok();
        assert_eq!((&out["job"]["status"], &out["fields"]), (&json!("draft"), &json!(["title", "tier"])), "{phase}");
        job(&env, &["edit", "e", "--needs", "a,d"], "pl").ok();
        assert_eq!(job_row(&env, "pl", "d", "fingerprint"), Value::Null);
        let out = job(&env, &["cancel", "e", "--reason", "Not this batch."], "pl").ok();
        assert_eq!((&out["status"], &out["cancel_requested"]), (&json!("cancelled"), &json!(false)));
        assert_eq!(job_row(&env, "pl", "e", "fingerprint"), Value::Null);
    }

    // In phase plan every Job is a draft: --draft changes nothing.
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    let out = job_add_draft(&env, "pl", "a", &[], &[]).ok();
    assert_eq!((&out["job"]["status"], &out["job"]["appended"]), (&json!("draft"), &json!(false)));
    assert_eq!(job_row(&env, "pl", "a", "appended"), 0);

    // In phase done --draft passes the phase, and finds every PR ended until `pr add` reopens the Plan;
    // without --draft the phase refuses.
    plan(&env, "dn", "done");
    let e = refused(&job_add_draft(&env, "dn", "k", &[], &[]), "pr_ended", 3);
    assert_eq!(e["details"]["status"], "merged");
    refused(&job_add(&env, "dn", "k", "S2", &[]), "illegal_phase", 3);
}

#[test]
fn a_queued_job_may_not_need_a_draft() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "dispatch");
    job_add(&env, "pl", "a", "S2", &[]).ok();
    job_add_draft(&env, "pl", "d", &[], &[]).ok();
    let (txns, evs, rows) = (count(&env, "txns"), count(&env, "events"), count(&env, "jobs"));

    let e = refused(&job_add(&env, "pl", "q", "S2", &["a", "d"]), "needs_unavailable", 3);
    assert_eq!(e["details"]["jobs"], json!(["d"]));
    assert!(e["message"].as_str().unwrap().contains("--draft"), "{e}");
    // A review Job needs every Job of its PR, the draft among them.
    let spec = spec_file(&env);
    let review = ["job", "add", "r", "--title", "Review", "--tier", "S2", "--spec", spec.as_str(), "--review", "--plan", "pl"];
    let e = refused(&env.cpd(&review), "needs_unavailable", 3);
    assert_eq!(e["details"]["jobs"], json!(["d"]));
    assert_eq!((count(&env, "txns"), count(&env, "events"), count(&env, "jobs")), (txns, evs, rows));

    // As drafts both are added: they are queued together when the review of the drafts is approved.
    assert_eq!(job_add_draft(&env, "pl", "q", &["a", "d"], &[]).ok()["job"]["status"], "draft");
    let out = job_add_draft(&env, "pl", "r", &[], &["--review"]).ok();
    assert_eq!((&out["job"]["status"], &out["job"]["needs"]), (&json!("draft"), &json!(["a", "d", "q"])));
}

#[test]
fn add_to_a_pr_already_all_merged_clears_all_merged_at_and_only_that_pr() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    job_add(&env, "pl", "a", "S2", &[]).ok();
    lock(&env, "pl");
    fixture(&env, "UPDATE prs SET all_merged_at = 't' WHERE plan = 'pl';");
    let marked = |env: &TestEnv| store(env).read(|c| Ok(c.query_row("SELECT all_merged_at FROM prs WHERE plan = 'pl'", [], |r| r.get::<_, Option<String>>(0))?)).unwrap();
    // A rerun of an existing Job writes nothing.
    job_add(&env, "pl", "a", "S2", &[]).ok();
    assert_eq!(marked(&env), Some("t".into()));
    job_add(&env, "pl", "late", "S2", &[]).ok();
    assert_eq!(marked(&env), None);
}

// ---------- stub drafts (hs-d241, R1; commands.md §5.1, §5.2) ----------

/// `job add <key> --draft --title <t>` with `extra` flags (`--tier`, `--spec`), in Plan `plan_id`.
fn stub_add(env: &TestEnv, plan_id: &str, key: &str, extra: &[&str]) -> Run {
    let title = format!("Title of {key}");
    let mut args = vec!["job", "add", key, "--draft", "--title", &title, "--plan", plan_id];
    args.extend(extra);
    env.cpd(&args)
}

#[test]
fn add_draft_needs_only_a_title_and_stores_the_tier_and_the_spec_as_null() {
    for phase in ["plan", "dispatch"] {
        let env = TestEnv::new();
        setup(&env);
        plan(&env, "pl", phase);
        job_add_draft(&env, "pl", "a", &[], &[]).ok();

        let out = stub_add(&env, "pl", "k", &[]).ok();
        let appended = phase != "plan";
        assert_eq!(out["job"], json!({"key": "k", "pr": "repo", "status": "draft", "appended": appended, "tier": null, "needs": [], "review": false}), "{phase}");
        for column in ["tier", "goal", "context", "touches", "acceptance", "out_of_scope", "fingerprint"] {
            assert_eq!(job_row(&env, "pl", "k", column), Value::Null, "{phase}: {column}");
        }
        assert_eq!((job_row(&env, "pl", "k", "title"), job_row(&env, "pl", "k", "status")), (json!("Title of k"), json!("draft")));
        assert_eq!(
            events(&env, "job-added").last().unwrap()["data"],
            json!({"pr": "repo", "status": "draft", "appended": appended, "tier": null, "needs": [], "review": false})
        );

        // Either half may be given: the Tier alone, the Spec alone; needs work as for any draft.
        let spec = spec_file(&env);
        let out = stub_add(&env, "pl", "t", &["--tier", "O3", "--needs", "a"]).ok();
        assert_eq!((&out["job"]["tier"], &out["job"]["needs"]), (&json!("O3"), &json!(["a"])), "{phase}");
        assert_eq!((job_row(&env, "pl", "t", "tier"), job_row(&env, "pl", "t", "goal")), (json!("O3"), Value::Null));
        stub_add(&env, "pl", "s", &["--spec", &spec]).ok();
        assert_eq!((job_row(&env, "pl", "s", "tier"), job_row(&env, "pl", "s", "goal")), (Value::Null, json!("Do it.")));
        assert_eq!(job_row(&env, "pl", "s", "touches"), "[\"src/a.rs\"]");

        // An identical rerun is `already` and writes nothing; the same key with other fields is a clash.
        let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
        assert_eq!(stub_add(&env, "pl", "k", &[]).ok()["already"], true);
        assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
        refused(&stub_add(&env, "pl", "k", &["--tier", "S2"]), "job_key_exists", 3);

        // The stub is shown as it is, and can be cancelled.
        let shown = job(&env, &["show", "k"], "pl").ok();
        assert_eq!((&shown["job"]["tier"], &shown["job"]["goal"], &shown["job"]["touches"]), (&Value::Null, &Value::Null, &Value::Null), "{phase}");
        let listed = job(&env, &["list"], "pl").ok();
        let stub = listed["jobs"].as_array().unwrap().iter().find(|j| j["key"] == "k").unwrap().clone();
        assert_eq!((&stub["tier"], &stub["status"]), (&Value::Null, &json!("draft")));
        let out = job(&env, &["cancel", "k", "--reason", "Not needed."], "pl").ok();
        assert_eq!((&out["status"], &out["cancel_requested"]), (&json!("cancelled"), &json!(false)));
        assert_eq!((job_row(&env, "pl", "k", "status"), job_row(&env, "pl", "k", "tier")), (json!("cancelled"), Value::Null));
    }
}

#[test]
fn add_without_draft_still_needs_the_tier_and_the_spec_and_a_review_job_never_is_a_stub() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    job_add(&env, "pl", "a", "S2", &[]).ok();
    let spec = spec_file(&env);
    let (txns, evs, rows) = (count(&env, "txns"), count(&env, "events"), count(&env, "jobs"));
    for args in [
        vec!["job", "add", "k", "--title", "T", "--plan", "pl"],
        vec!["job", "add", "k", "--title", "T", "--tier", "S2", "--plan", "pl"],
        vec!["job", "add", "k", "--title", "T", "--spec", spec.as_str(), "--plan", "pl"],
        vec!["job", "add", "k", "--title", "T", "--draft", "--review", "--plan", "pl"],
        vec!["job", "add", "k", "--title", "T", "--draft", "--review", "--tier", "S2", "--plan", "pl"],
    ] {
        let e = refused(&env.cpd(&args), "usage", 2);
        assert!(e["message"].as_str().unwrap().contains("--draft"), "{args:?}: {e}");
    }
    assert_eq!((count(&env, "txns"), count(&env, "events"), count(&env, "jobs")), (txns, evs, rows), "nothing is written");
    // After the lock too: a Job that is not a draft needs both.
    lock(&env, "pl");
    let (txns, evs, rows) = (count(&env, "txns"), count(&env, "events"), count(&env, "jobs"));
    refused(&env.cpd(&["job", "add", "k", "--title", "T", "--plan", "pl"]), "usage", 2);
    assert_eq!((count(&env, "txns"), count(&env, "events"), count(&env, "jobs")), (txns, evs, rows));
}

#[test]
fn edit_completes_a_stub_draft_with_its_tier_and_its_spec() {
    for phase in ["plan", "dispatch"] {
        let env = TestEnv::new();
        setup(&env);
        plan(&env, "pl", phase);
        let spec = spec_file(&env);
        stub_add(&env, "pl", "k", &[]).ok();

        let out = job(&env, &["edit", "k", "--tier", "S2", "--spec", &spec], "pl").ok();
        assert_eq!((&out["job"], &out["fields"]), (&json!({"key": "k", "status": "draft"}), &json!(["tier", "spec"])), "{phase}");
        assert_eq!(job_row(&env, "pl", "k", "tier"), "S2");
        assert_eq!(job_row(&env, "pl", "k", "goal"), "Do it.");
        assert_eq!(job_row(&env, "pl", "k", "context"), "Because.");
        assert_eq!(job_row(&env, "pl", "k", "touches"), "[\"src/a.rs\"]");
        assert_eq!(job_row(&env, "pl", "k", "acceptance"), "[\"cargo test\"]");
        assert_eq!(job_row(&env, "pl", "k", "out_of_scope"), "[\"Everything else.\"]");
        assert_eq!(events(&env, "job-edited").last().unwrap()["data"], json!({"fields": ["tier", "spec"]}), "{phase}");
        assert_eq!(job_row(&env, "pl", "k", "fingerprint"), Value::Null);

        // A rerun changes nothing; the Spec is replaced as a whole, so its fields are listed one by one again.
        let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
        assert_eq!(job(&env, &["edit", "k", "--tier", "S2", "--spec", &spec], "pl").ok()["already"], true);
        assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
        let other = write_spec(&env, "other.json", &json!({"goal": "Other.", "context": "Because.", "touches": ["src/a.rs"], "acceptance": ["cargo test"], "out_of_scope": []}).to_string());
        assert_eq!(job(&env, &["edit", "k", "--spec", &other], "pl").ok()["fields"], json!(["goal", "out_of_scope"]), "{phase}");

        // The Tier alone, then the Spec alone, complete a stub in two edits.
        stub_add(&env, "pl", "two", &[]).ok();
        assert_eq!(job(&env, &["edit", "two", "--tier", "O3"], "pl").ok()["fields"], json!(["tier"]), "{phase}");
        assert_eq!((job_row(&env, "pl", "two", "tier"), job_row(&env, "pl", "two", "goal")), (json!("O3"), Value::Null));
        assert_eq!(job(&env, &["edit", "two", "--spec", &spec], "pl").ok()["fields"], json!(["spec"]), "{phase}");
        assert_eq!((job_row(&env, "pl", "two", "tier"), job_row(&env, "pl", "two", "goal")), (json!("O3"), json!("Do it.")));

        // A title edit leaves a stub a stub.
        stub_add(&env, "pl", "three", &[]).ok();
        assert_eq!(job(&env, &["edit", "three", "--title", "Renamed"], "pl").ok()["fields"], json!(["title"]));
        assert_eq!(job_row(&env, "pl", "three", "tier"), Value::Null);
    }
}

// ---------- urgent Jobs (contract §2.2, §3.7 E3; commands.md §5.1, §5.8) ----------

/// `job add` of `key` with `--urgent` (and `extra` flags).
fn job_add_urgent(env: &TestEnv, plan: &str, key: &str, extra: &[&str]) -> Run {
    let spec = spec_file(env);
    let title = format!("Title of {key}");
    let mut args = vec!["job", "add", key, "--title", &title, "--tier", "S2", "--spec", &spec, "--plan", plan, "--urgent"];
    args.extend(extra);
    env.cpd(&args)
}

#[test]
fn a_job_is_not_urgent_unless_added_with_urgent() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    let out = job_add(&env, "pl", "calm", "S2", &[]).ok();
    assert_eq!(out["job"]["urgent"], false);
    assert_eq!(job_row(&env, "pl", "calm", "urgent"), 0);
    assert_eq!(events(&env, "job-added")[0]["data"]["urgent"], false);

    let out = job_add_urgent(&env, "pl", "hot", &[]).ok();
    assert_eq!(out["job"], json!({"key": "hot", "pr": "repo", "status": "draft", "appended": false, "tier": "S2", "needs": [], "review": false, "urgent": true}));
    assert_eq!(job_row(&env, "pl", "hot", "urgent"), 1);
    assert_eq!(events(&env, "job-added")[1]["data"], json!({"pr": "repo", "status": "draft", "appended": false, "tier": "S2", "needs": [], "review": false, "urgent": true}));

    // The flag is not a Locked field: the same add with or without it is `already`, as for `--draft`.
    assert_eq!(job_add_urgent(&env, "pl", "calm", &[]).ok()["already"], true);
    assert_eq!(job_row(&env, "pl", "calm", "urgent"), 0, "an add that found the Job changes nothing");
    assert_eq!(job_add(&env, "pl", "hot", "S2", &[]).ok()["already"], true);

    // After the lock the appended Job is `queued` and urgent at once, its fingerprint stored.
    lock(&env, "pl");
    let out = job_add_urgent(&env, "pl", "late", &[]).ok();
    assert_eq!((&out["job"]["status"], &out["job"]["urgent"]), (&json!("queued"), &json!(true)));
    assert_eq!(job_row(&env, "pl", "late", "urgent"), 1);
    // `--draft` and `--urgent` together: a draft, urgent.
    let out = job_add_urgent(&env, "pl", "later", &["--draft"]).ok();
    assert_eq!((&out["job"]["status"], &out["job"]["urgent"]), (&json!("draft"), &json!(true)));
}

#[test]
fn urgent_marks_a_queued_job_once_with_one_job_edited_and_changes_no_fingerprint() {
    let env = TestEnv::new();
    drafted(&env);
    lock(&env, "pl");
    let fingerprint = job_row(&env, "pl", "a", "fingerprint");
    assert_ne!(fingerprint, Value::Null);
    let edited = events(&env, "job-edited").len();

    let out = job(&env, &["urgent", "a"], "pl").ok();
    assert_eq!(out, json!({"job": {"key": "a", "status": "queued", "urgent": true}, "fields": ["urgent"]}));
    assert_eq!(job_row(&env, "pl", "a", "urgent"), 1);
    assert_eq!((job_row(&env, "pl", "a", "status"), job_row(&env, "pl", "a", "fingerprint")), (json!("queued"), fingerprint));
    assert_eq!(job_row(&env, "pl", "b", "urgent"), 0, "only the named Job");
    let all = events(&env, "job-edited");
    assert_eq!(all.len(), edited + 1);
    assert_eq!((&all.last().unwrap()["job"], &all.last().unwrap()["data"]), (&json!("a"), &json!({"fields": ["urgent"]})));
    // The Locked Job still matches its fingerprint: the flag is in none.
    assert_eq!(job(&env, &["show", "a"], "pl").ok()["tamper"], json!([]));

    // Idempotent: already, nothing written.
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
    let again = job(&env, &["urgent", "a"], "pl").ok();
    assert_eq!(again["already"], true);
    assert_eq!(again["job"]["urgent"], true);
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
}

#[test]
fn urgent_does_not_change_the_plan_fingerprint_of_a_draft() {
    let env = TestEnv::new();
    drafted(&env);
    let before = env.cpd(&["plan", "render", "--plan", "pl"]).ok()["fingerprint"].clone();
    assert_eq!(job(&env, &["urgent", "a"], "pl").ok()["job"], json!({"key": "a", "status": "draft", "urgent": true}));
    assert_eq!(env.cpd(&["plan", "render", "--plan", "pl"]).ok()["fingerprint"], before);
}

#[test]
fn urgent_is_refused_for_a_job_that_is_not_queued_or_draft_and_for_an_unknown_one() {
    let env = TestEnv::new();
    drafted(&env);
    job_add(&env, "pl", "c", "S2", &[]).ok();
    job_add(&env, "pl", "d", "S2", &[]).ok();
    job_add(&env, "pl", "e", "S2", &[]).ok();
    lock(&env, "pl");
    engine(&env, "pl", "a", "merged");
    engine(&env, "pl", "c", "running");
    engine(&env, "pl", "d", "failed");
    job(&env, &["cancel", "e", "--reason", "No."], "pl").ok();
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
    for (key, status) in [("a", "merged"), ("c", "running"), ("d", "failed"), ("e", "cancelled")] {
        let error = refused(&job(&env, &["urgent", key], "pl"), "illegal_transition", 3);
        assert_eq!(error["details"]["status"], status, "{key}");
        assert_eq!(job_row(&env, "pl", key, "urgent"), 0, "{key}");
    }
    refused(&job(&env, &["urgent", "ghost"], "pl"), "job_not_found", 4);
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs), "refusals write nothing");
}

#[test]
fn list_and_show_carry_urgent() {
    let env = TestEnv::new();
    drafted(&env);
    job(&env, &["urgent", "b"], "pl").ok();
    let listed = job(&env, &["list"], "pl").ok();
    let flags: Vec<(&Value, &Value)> = listed["jobs"].as_array().unwrap().iter().map(|j| (&j["key"], &j["urgent"])).collect();
    assert_eq!(flags, [(&json!("a"), &json!(false)), (&json!("b"), &json!(true))]);
    assert_eq!(job(&env, &["show", "b"], "pl").ok()["job"]["urgent"], true);
    assert_eq!(job(&env, &["show", "a"], "pl").ok()["job"]["urgent"], false);
    let shown = env.cpd(&["plan", "show", "--plan", "pl"]).ok();
    let flags: Vec<&Value> = shown["jobs"].as_array().unwrap().iter().map(|j| &j["urgent"]).collect();
    assert_eq!(flags, [&json!(false), &json!(true)]);
}
