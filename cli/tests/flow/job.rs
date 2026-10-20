//! Jobs (contract §2.3, §3.2; commands.md §5.1–§5.4, §5.6, §5.7), run as the
//! built binary against a temporary `$CPD_HOME`. Plans, Attempts and the
//! states only the engine reaches are written straight into the database, as
//! fixtures; the commands under test are the only ones that write Jobs.
//! pi is a fake install whose Tier catalog is seeded in the cache, so no node runs.

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::{Run, TestEnv, refused};
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

fn events(env: &TestEnv, kind: &str) -> Vec<Value> {
    env.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
}

fn count(env: &TestEnv, table: &str) -> i64 {
    env.query(&format!("SELECT count(*) FROM {table}"))
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
    assert_eq!(out["job"], json!({"key": "a", "pr": "repo", "kind": "work", "status": "draft", "appended": false, "tier": "S2", "needs": [], "review": false, "urgent": false}));
    let out = job_add(&env, "pl", "b", "S3", &["a"]).ok();
    assert_eq!(out["job"], json!({"key": "b", "pr": "repo", "kind": "work", "status": "draft", "appended": false, "tier": "S3", "needs": ["a"], "review": false, "urgent": false}));
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
    assert_eq!(env.query::<String>("SELECT needs FROM dependencies WHERE job = 'b'"), "a");

    let added = events(&env, "job-added");
    assert_eq!(added.len(), 2);
    assert_eq!(added[1]["job"], "b");
    assert_eq!(added[1]["plan"], "pl");
    assert_eq!(added[1]["context"], "ctx");
    assert_eq!(added[1]["data"], json!({"pr": "repo", "kind": "work", "status": "draft", "appended": false, "tier": "S3", "needs": ["a"], "review": false, "urgent": false}));
    assert_eq!(env.query::<i64>("SELECT count(*) FROM history WHERE tbl = 'jobs' AND op = 'insert'"), 2);
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
        assert_eq!(out["job"], json!({"key": "late", "pr": "repo", "kind": "work", "status": "queued", "appended": true, "tier": "H1", "needs": ["a"], "review": false, "urgent": false}), "{phase}");
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

    let run = job(&env, &["edit", "b", "--tier", "S4", "--needs", "a,c", "--title", "Title of b"], "pl");
    let out = run.ok();
    assert_eq!(out["job"], json!({"key": "b", "status": "draft"}));
    assert_eq!(out["fields"], json!(["tier", "needs"]));
    assert_eq!(job_row(&env, "pl", "b", "tier"), "S4");
    assert_eq!(env.query::<String>("SELECT group_concat(needs) FROM (SELECT needs FROM dependencies WHERE job = 'b' ORDER BY needs)"), "a,c");

    let out = job(&env, &["edit", "b", "--spec", &spec, "--title", "New title"], "pl").ok();
    assert_eq!(out["fields"], json!(["title", "goal", "out_of_scope"]));
    assert_eq!(job_row(&env, "pl", "b", "goal"), "New goal.");
    assert_eq!(job_row(&env, "pl", "b", "out_of_scope"), "[]");
    assert_eq!(job_row(&env, "pl", "b", "title"), "New title");

    let out = job(&env, &["edit", "b", "--no-needs"], "pl").ok();
    assert_eq!(out["fields"], json!(["needs"]));
    assert_eq!(env.query::<i64>("SELECT count(*) FROM dependencies WHERE job = 'b'"), 0);

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
    assert_eq!(env.query::<i64>("SELECT count(*) FROM dependencies"), 2);
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
    assert_eq!(env.query::<Option<String>>("SELECT stop_request FROM attempts WHERE id = 'w-a1'"), None, "E2 sets the stop request, not the command");
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
    let out = job(&env, &["retry", "a", "--tier", "S4"], "pl").ok();
    assert_eq!(out, json!({"ok": true, "job": "a", "status": "queued", "tier": "S4"}));
    assert_eq!(job_row(&env, "pl", "a", "status"), "queued");
    assert_eq!(job_row(&env, "pl", "a", "retry_tier"), "S4");
    assert_eq!(job_row(&env, "pl", "a", "failed_cause"), Value::Null);
    assert_eq!(job_row(&env, "pl", "a", "tier"), "S2", "the Job's own Tier is Locked");
    let retried = events(&env, "job-retried");
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0]["data"], json!({"tier": "S4"}));
    assert_eq!(retried[0]["job"], "a");

    // Again with the same Tier and no Attempt since: already, nothing written.
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
    let out = job(&env, &["retry", "a", "--tier", "S4"], "pl").ok();
    assert_eq!(out["already"], true);
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
    // Another Tier is not the same retry.
    refused(&job(&env, &["retry", "a", "--tier", "S3"], "pl"), "job_not_failed", 3);
    // Once an Attempt has started since, the retry is spent: the Job is only queued again by the ladder.
    fixture(
        &env,
        "INSERT INTO events (txn, at, kind, context, plan, job, attempt, text, data)
         SELECT id, at, 'attempt-started', 'ctx', 'pl', 'a', 'a-a2', 'Started.', '{}' FROM txns WHERE id = cpd_txn();",
    );
    refused(&job(&env, &["retry", "a", "--tier", "S4"], "pl"), "job_not_failed", 3);
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
    let trigger: String = env.query("SELECT sql FROM sqlite_schema WHERE name = 'jobs_locked'");
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
    job_add(&env, "pl", "c", "S3", &[]).ok();
    lock(&env, "pl");
    engine(&env, "pl", "a", "merged");
    engine(&env, "pl", "c", "failed");

    let out = job(&env, &["list"], "pl").ok();
    let jobs = out["jobs"].as_array().unwrap();
    let keys: Vec<&str> = jobs.iter().map(|j| j["key"].as_str().unwrap()).collect();
    assert_eq!(keys, ["a", "b", "c"]);
    assert_eq!(
        jobs[0],
        json!({"key": "a", "title": "Title of a", "kind": "work", "status": "merged", "tier": "S2", "appended": false, "needs": [], "attempts": 1, "merged_sha": "c0ffee", "review": false, "urgent": false})
    );
    assert_eq!(jobs[1], json!({"key": "b", "title": "Title of b", "kind": "work", "status": "queued", "tier": "S2", "appended": false, "needs": ["a"], "attempts": 0, "merged_sha": null, "review": false, "urgent": false}));
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
    assert_eq!(env.query::<String>("SELECT needs FROM dependencies WHERE job = 'b'"), "w");
    assert_eq!(events(&env, "job-edited").last().unwrap()["data"], json!({"fields": ["pr", "needs"]}));

    // A Job with old needs may move with `--no-needs`.
    let out = job(&env, &["edit", "b", "--pr", "repo", "--no-needs"], "pl").ok();
    assert_eq!(out["fields"], json!(["pr", "needs"]));
    assert_eq!(env.query::<i64>("SELECT count(*) FROM dependencies"), 0);
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
        assert_eq!(out["job"], json!({"key": "d", "pr": "repo", "kind": "work", "status": "draft", "appended": true, "tier": "S2", "needs": ["a"], "review": false, "urgent": false}), "{phase}");
        assert_eq!((job_row(&env, "pl", "d", "status"), job_row(&env, "pl", "d", "appended")), (json!("draft"), json!(1)));
        assert_eq!(job_row(&env, "pl", "d", "fingerprint"), Value::Null);
        assert_eq!(
            events(&env, "job-added").last().unwrap()["data"],
            json!({"pr": "repo", "kind": "work", "status": "draft", "appended": true, "tier": "S2", "needs": ["a"], "review": false, "urgent": false})
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
        assert_eq!(out["job"], json!({"key": "k", "pr": "repo", "kind": "work", "status": "draft", "appended": appended, "tier": null, "needs": [], "review": false, "urgent": false}), "{phase}");
        for column in ["tier", "goal", "context", "touches", "acceptance", "out_of_scope", "fingerprint"] {
            assert_eq!(job_row(&env, "pl", "k", column), Value::Null, "{phase}: {column}");
        }
        assert_eq!((job_row(&env, "pl", "k", "title"), job_row(&env, "pl", "k", "status")), (json!("Title of k"), json!("draft")));
        assert_eq!(
            events(&env, "job-added").last().unwrap()["data"],
            json!({"pr": "repo", "kind": "work", "status": "draft", "appended": appended, "tier": null, "needs": [], "review": false, "urgent": false})
        );

        // Either half may be given: the Tier alone, the Spec alone; needs work as for any draft.
        let spec = spec_file(&env);
        let out = stub_add(&env, "pl", "t", &["--tier", "S3", "--needs", "a"]).ok();
        assert_eq!((&out["job"]["tier"], &out["job"]["needs"]), (&json!("S3"), &json!(["a"])), "{phase}");
        assert_eq!((job_row(&env, "pl", "t", "tier"), job_row(&env, "pl", "t", "goal")), (json!("S3"), Value::Null));
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
        assert_eq!(job(&env, &["edit", "two", "--tier", "S3"], "pl").ok()["fields"], json!(["tier"]), "{phase}");
        assert_eq!((job_row(&env, "pl", "two", "tier"), job_row(&env, "pl", "two", "goal")), (json!("S3"), Value::Null));
        assert_eq!(job(&env, &["edit", "two", "--spec", &spec], "pl").ok()["fields"], json!(["spec"]), "{phase}");
        assert_eq!((job_row(&env, "pl", "two", "tier"), job_row(&env, "pl", "two", "goal")), (json!("S3"), json!("Do it.")));

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
    assert_eq!(out["job"], json!({"key": "hot", "pr": "repo", "kind": "work", "status": "draft", "appended": false, "tier": "S2", "needs": [], "review": false, "urgent": true}));
    assert_eq!(job_row(&env, "pl", "hot", "urgent"), 1);
    assert_eq!(events(&env, "job-added")[1]["data"], json!({"pr": "repo", "kind": "work", "status": "draft", "appended": false, "tier": "S2", "needs": [], "review": false, "urgent": true}));

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
    assert_eq!((&out["job"], &out["fields"]), (&json!({"key": "a", "status": "queued", "urgent": true}), &json!(["urgent"])));
    assert!(out.get("already").is_none());
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

// ---------- kinds (contract §2.2 "Job kinds"; commands.md §5.1, §5.2, §5.6, §5.7) ----------

/// A Spec file for a `test` Job: the Spec of every Job and the named tests with the files that hold them.
fn test_spec_file(env: &TestEnv, name: &str, tests: Value, files: Value) -> String {
    let mut spec = spec_json();
    spec["tests"] = tests;
    spec["test_files"] = files;
    write_spec(env, name, &spec.to_string())
}

/// `job add <key> --kind <kind> --spec <spec>` into PR `pr` of Plan `plan`, a draft, with the Tier S2.
fn add_kind(env: &TestEnv, plan: &str, key: &str, kind: &str, spec: &str, pr: &str) -> Run {
    env.cpd(&["job", "add", key, "--title", &format!("Title of {key}"), "--tier", "S2", "--spec", spec, "--kind", kind, "--pr", pr, "--plan", plan])
}

/// PR `key` of Plan `plan`, `waiting`, of the stored `shape`.
fn shaped_pr(env: &TestEnv, plan: &str, key: &str, shape: &str) {
    fixture(
        env,
        &format!(
            "INSERT INTO prs (plan, key, repo, base, status, lead_branch, shape, created_txn, created_at)
             VALUES ('{plan}', '{key}', '/repo', 'main', 'waiting', 'cpd/{plan}/{key}', '{shape}', cpd_txn(), 't');"
        ),
    );
}

#[test]
fn add_kind_stores_the_kind_and_a_test_jobs_tests_and_shows_them() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    let spec = spec_file(&env);
    let tests = test_spec_file(&env, "tests.json", json!(["pr_open_job::merges_the_base", "pr_open_job::opens"]), json!(["cli/tests/pr_open_job.rs", "core/src/pr/"]));
    // The default is `work`; the kinds are the Lead's: `docs`, `test` and `impl`.
    assert_eq!(job_add(&env, "pl", "w", "S2", &[]).ok()["job"]["kind"], "work");
    let docs = add_kind(&env, "pl", "d", "docs", &spec, "repo").ok();
    assert_eq!((&docs["job"]["kind"], &docs["job"]["status"]), (&json!("docs"), &json!("draft")));
    let test = add_kind(&env, "pl", "t", "test", &tests, "repo").ok();
    assert_eq!(test["job"]["kind"], "test");
    assert_eq!(add_kind(&env, "pl", "i", "impl", &spec, "repo").ok()["job"]["kind"], "impl");
    assert_eq!(job(&env, &["add", "c", "--title", "C", "--tier", "S2", "--spec", &spec, "--kind", "work"], "pl").ok()["job"]["kind"], "work");
    assert_eq!(
        job_row(&env, "pl", "t", "tests"),
        json!("[\"pr_open_job::merges_the_base\",\"pr_open_job::opens\"]")
    );
    assert_eq!(job_row(&env, "pl", "t", "test_files"), json!("[\"cli/tests/pr_open_job.rs\",\"core/src/pr/\"]"));
    assert_eq!(job_row(&env, "pl", "d", "tests"), Value::Null);
    assert_eq!(job_row(&env, "pl", "i", "kind"), json!("impl"));

    // The Event carries the kind; `job list` and `job show` show it, and a `test` Job's tests and files.
    let added = events(&env, "job-added");
    let kinds: Vec<(&Value, &Value)> = added.iter().map(|e| (&e["job"], &e["data"]["kind"])).collect();
    assert_eq!(kinds, [(&json!("w"), &json!("work")), (&json!("d"), &json!("docs")), (&json!("t"), &json!("test")), (&json!("i"), &json!("impl")), (&json!("c"), &json!("work"))]);
    let listed = job(&env, &["list"], "pl").ok();
    let by_key: Vec<(&str, &str)> = listed["jobs"].as_array().unwrap().iter().map(|j| (j["key"].as_str().unwrap(), j["kind"].as_str().unwrap())).collect();
    assert_eq!(by_key, [("w", "work"), ("d", "docs"), ("t", "test"), ("i", "impl"), ("c", "work")]);
    let shown = job(&env, &["show", "t"], "pl").ok();
    assert_eq!(shown["job"]["kind"], "test");
    assert_eq!(shown["job"]["tests"], json!(["pr_open_job::merges_the_base", "pr_open_job::opens"]));
    assert_eq!(shown["job"]["test_files"], json!(["cli/tests/pr_open_job.rs", "core/src/pr/"]));
    let shown = job(&env, &["show", "i"], "pl").ok();
    assert_eq!((&shown["job"]["kind"], &shown["job"]["test_approved"]), (&json!("impl"), &Value::Null));
    assert!(job(&env, &["show", "d"], "pl").ok()["job"].get("tests").is_none(), "a `docs` Job has no tests");
}

#[test]
fn the_kind_is_part_of_the_locked_fields_of_an_add_that_repeats() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    let spec = spec_file(&env);
    add_kind(&env, "pl", "d", "docs", &spec, "repo").ok();
    let writes = (count(&env, "txns"), count(&env, "events"));
    assert_eq!(add_kind(&env, "pl", "d", "docs", &spec, "repo").ok()["already"], true);
    assert_eq!((count(&env, "txns"), count(&env, "events")), writes);
    refused(&add_kind(&env, "pl", "d", "impl", &spec, "repo"), "job_key_exists", 3);
    refused(&job_add(&env, "pl", "d", "S2", &[]), "job_key_exists", 3);
    assert_eq!((count(&env, "txns"), count(&env, "events")), writes);
}

#[test]
fn add_kind_refusals_write_nothing() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    let spec = spec_file(&env);
    let tests = test_spec_file(&env, "tests.json", json!(["t::one"]), json!(["tests/t.rs"]));
    let writes = || (count(&env, "txns"), count(&env, "events"), count(&env, "jobs"));
    let before = writes();

    // `open` and `update` are cpd's (exit 3, `details.kind`).
    for kind in ["open", "update"] {
        let e = refused(&add_kind(&env, "pl", "k", kind, &spec, "repo"), "kind_reserved", 3);
        assert_eq!(e["details"]["kind"], kind);
    }
    // Not a kind (clap), and a review Job is a `work` Job.
    refused(&add_kind(&env, "pl", "k", "other", &spec, "repo"), "usage", 2);
    refused(&env.cpd(&["job", "add", "k", "--title", "T", "--tier", "S2", "--spec", &spec, "--kind", "docs", "--review", "--pr", "repo", "--plan", "pl"]), "usage", 2);
    // The Spec fits the kind: a `test` Job names its tests and files, every other kind neither.
    let e = refused(&add_kind(&env, "pl", "k", "test", &spec, "repo"), "spec_invalid", 2);
    assert_eq!(e["details"]["path"], "$.tests");
    for kind in ["work", "docs", "impl"] {
        let e = refused(&add_kind(&env, "pl", "k", kind, &tests, "repo"), "spec_invalid", 2);
        assert_eq!(e["details"]["path"], "$.tests", "{kind}");
    }
    let bad = test_spec_file(&env, "bad.json", json!(["t::one"]), json!(["../escape.rs"]));
    assert_eq!(refused(&add_kind(&env, "pl", "k", "test", &bad, "repo"), "spec_invalid", 2)["details"]["path"], "$.test_files[0]");
    let bad = test_spec_file(&env, "bad.json", json!(["t::one", "t::one"]), json!(["tests/t.rs"]));
    assert_eq!(refused(&add_kind(&env, "pl", "k", "test", &bad, "repo"), "spec_invalid", 2)["details"]["path"], "$.tests[1]");
    assert_eq!(writes(), before);
}

#[test]
fn a_job_cannot_need_an_open_or_an_update_job() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    job_add(&env, "pl", "a", "S2", &[]).ok();
    lock(&env, "pl");
    // What engine E4 adds: an `open` Job, queued, with its fingerprint and no Dependency.
    let fingerprint = "a".repeat(64);
    fixture(
        &env,
        &format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, kind, created_txn, created_at)
             VALUES ('pl', 'repo', 'open-repo', 'Open', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'queued', 1, '{fingerprint}', 'open', cpd_txn(), 't');"
        ),
    );
    let writes = (count(&env, "txns"), count(&env, "events"));
    let e = refused(&job_add(&env, "pl", "k", "S2", &["open-repo"]), "needs_unavailable", 3);
    assert_eq!(e["details"]["jobs"], json!(["open-repo"]));
    assert_eq!((count(&env, "txns"), count(&env, "events")), writes);
    // And a review Job does not take it as a Dependency.
    let spec = spec_file(&env);
    let review = env.cpd(&["job", "add", "rev", "--title", "R", "--tier", "S2", "--spec", &spec, "--review", "--draft", "--plan", "pl"]).ok();
    assert_eq!(review["job"]["needs"], json!(["a"]));
}

#[test]
fn the_stored_shape_decides_the_kinds_a_waiting_pr_may_have() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    for shape in ["tdd", "docs", "config"] {
        shaped_pr(&env, "pl", shape, shape);
    }
    let spec = spec_file(&env);
    let tests = test_spec_file(&env, "tests.json", json!(["t::one"]), json!(["tests/t.rs"]));
    let spec_of = |kind: &str| if kind == "test" { tests.clone() } else { spec.clone() };
    let allowed = |shape: &str| match shape {
        "tdd" => vec!["docs", "test", "impl"],
        "docs" => vec!["docs"],
        "config" => vec!["work"],
        _ => unreachable!(),
    };
    for shape in ["tdd", "docs", "config"] {
        for kind in ["work", "docs", "test", "impl"] {
            let key = format!("{shape}-{kind}");
            let run = add_kind(&env, "pl", &key, kind, &spec_of(kind), shape);
            if allowed(shape).contains(&kind) {
                assert_eq!(run.ok()["job"]["kind"], kind, "{shape} allows {kind}");
            } else {
                let e = refused(&run, "shape_violation", 3);
                assert_eq!(
                    e["details"],
                    json!({"plan": "pl", "pr": shape, "shape": shape, "kind": kind, "allowed": allowed(shape)}),
                    "{shape} refuses {kind}"
                );
            }
        }
    }
    // A review Job (kind `work`) is allowed in every shape; a PR from before the migration (`free`) allows every kind.
    for shape in ["tdd", "docs", "config"] {
        let key = format!("rev-{shape}");
        env.cpd(&["job", "add", &key, "--title", "R", "--tier", "S2", "--spec", &spec, "--review", "--draft", "--pr", shape, "--plan", "pl"]).ok();
        assert_eq!(job_row(&env, "pl", &key, "review"), json!(1));
    }
    for kind in ["work", "docs", "test", "impl"] {
        assert_eq!(add_kind(&env, "pl", &format!("free-{kind}"), kind, &spec_of(kind), "repo").ok()["job"]["kind"], kind);
    }
}

#[test]
fn an_open_pr_is_not_checked_against_its_shape_and_a_job_added_to_it_is_queued() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "deploy");
    // A `tdd` PR that has opened its pull request: a feedback fix is whatever it is, a `work` Job included.
    fixture(
        &env,
        &format!(
            "INSERT INTO prs (plan, key, repo, base, status, lead_branch, shape, created_txn, created_at)
             VALUES ('pl', 'fix', '/repo', 'main', 'waiting', 'cpd/pl/fix', 'tdd', cpd_txn(), 't');
             UPDATE prs SET status = 'dispatch', lead_worktree = '/wt2', started_at = 't' WHERE key = 'fix';
             {}
             UPDATE prs SET status = 'open', pr_number = 2, pr_url = 'https://github.com/acme/fix/pull/2', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/fix') WHERE key = 'fix';",
            pull_request("fix", 2, "open")
        ),
    );
    let out = env.cpd(&["job", "add", "late", "--title", "Fix", "--tier", "S2", "--spec", &spec_file(&env), "--pr", "fix", "--plan", "pl"]).ok();
    assert_eq!((&out["job"]["kind"], &out["job"]["status"]), (&json!("work"), &json!("queued")));
    // A `waiting` PR of the same shape refuses it.
    shaped_pr(&env, "pl", "next", "tdd");
    refused(
        &env.cpd(&["job", "add", "late2", "--title", "Fix", "--tier", "S2", "--spec", &spec_file(&env), "--pr", "next", "--plan", "pl"]),
        "shape_violation",
        3,
    );
}

#[test]
fn an_impl_job_that_is_not_a_draft_needs_a_test_job_of_its_pr() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    let spec = spec_file(&env);
    let tests = test_spec_file(&env, "tests.json", json!(["t::one"]), json!(["tests/t.rs"]));
    add_kind(&env, "pl", "t", "test", &tests, "repo").ok();
    add_kind(&env, "pl", "d", "docs", &spec, "repo").ok();
    lock(&env, "pl");
    let writes = (count(&env, "txns"), count(&env, "events"));
    // Appended and queued with no Dependency, or with one that is no test Job: refused, naming the test Jobs of the PR.
    let impl_job = |key: &str, needs: &str, draft: bool| {
        let mut args = vec!["job", "add", key, "--title", "I", "--tier", "S2", "--spec", &spec, "--kind", "impl", "--plan", "pl"];
        if !needs.is_empty() {
            args.extend(["--needs", needs]);
        }
        if draft {
            args.push("--draft");
        }
        env.cpd(&args)
    };
    let e = refused(&impl_job("i", "", false), "test_job_required", 3);
    assert_eq!((&e["details"]["job"], &e["details"]["jobs"]), (&json!("i"), &json!(["t"])));
    refused(&impl_job("i", "d", false), "test_job_required", 3);
    assert_eq!((count(&env, "txns"), count(&env, "events")), writes);
    // A draft is checked where it stops being one; a Job that needs the test Job is queued.
    assert_eq!(impl_job("i", "", true).ok()["job"]["status"], "draft");
    assert_eq!(impl_job("i2", "t", false).ok()["job"]["status"], "queued");
}

#[test]
fn edit_kind_changes_a_drafts_kind_and_its_spec_must_fit() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    let spec = spec_file(&env);
    let tests = test_spec_file(&env, "tests.json", json!(["t::one"]), json!(["tests/t.rs"]));
    for key in ["a", "b", "c"] {
        job_add(&env, "pl", key, "S2", &[]).ok();
    }
    shaped_pr(&env, "pl", "docs", "docs");
    // A kind alone: `work` to `docs`, and back.
    let out = job(&env, &["edit", "a", "--kind", "docs"], "pl").ok();
    assert_eq!(out["fields"], json!(["kind"]));
    assert_eq!(job_row(&env, "pl", "a", "kind"), json!("docs"));
    assert_eq!(events(&env, "job-edited").last().unwrap()["data"], json!({"fields": ["kind"]}));
    let writes = (count(&env, "txns"), count(&env, "events"));
    assert_eq!(job(&env, &["edit", "a", "--kind", "docs"], "pl").ok()["already"], true);
    assert_eq!((count(&env, "txns"), count(&env, "events")), writes);

    // To `test`, the Spec it has names no tests: refused; with a Spec that does, it changes together.
    let e = refused(&job(&env, &["edit", "a", "--kind", "test"], "pl"), "spec_invalid", 2);
    assert_eq!(e["details"]["path"], "$.tests");
    let out = job(&env, &["edit", "a", "--kind", "test", "--spec", &tests], "pl").ok();
    assert_eq!(out["fields"], json!(["kind", "tests", "test_files"]));
    assert_eq!(job_row(&env, "pl", "a", "tests"), json!("[\"t::one\"]"));
    // Out of `test`, the Spec still names tests: refused, and given a Spec that does not, it goes.
    refused(&job(&env, &["edit", "a", "--kind", "impl"], "pl"), "spec_invalid", 2);
    let out = job(&env, &["edit", "a", "--kind", "impl", "--spec", &spec], "pl").ok();
    assert_eq!(out["fields"], json!(["kind", "tests", "test_files"]));
    assert_eq!((job_row(&env, "pl", "a", "tests"), job_row(&env, "pl", "a", "test_files")), (Value::Null, Value::Null));
    // A Spec with tests for a Job that stays `impl` is refused too.
    refused(&job(&env, &["edit", "a", "--spec", &tests], "pl"), "spec_invalid", 2);

    // `open` and `update` are cpd's; the stored shape of the PR holds for an edit as for an add.
    for kind in ["open", "update"] {
        assert_eq!(refused(&job(&env, &["edit", "a", "--kind", kind], "pl"), "kind_reserved", 3)["details"]["kind"], kind);
    }
    // Moving a Job to a PR whose shape does not allow its kind is the same refusal.
    let e = refused(&job(&env, &["edit", "c", "--pr", "docs"], "pl"), "shape_violation", 3);
    assert_eq!((&e["details"]["pr"], &e["details"]["kind"], &e["details"]["allowed"]), (&json!("docs"), &json!("work"), &json!(["docs"])));
    job(&env, &["edit", "c", "--pr", "docs", "--kind", "docs"], "pl").ok();
    assert_eq!((job_row(&env, "pl", "c", "pr"), job_row(&env, "pl", "c", "kind")), (json!("docs"), json!("docs")));
}

#[test]
fn a_locked_jobs_kind_cannot_be_edited() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    job_add(&env, "pl", "a", "S2", &[]).ok();
    lock(&env, "pl");
    refused(&job(&env, &["edit", "a", "--kind", "docs"], "pl"), "job_locked", 3);
    assert_eq!(job_row(&env, "pl", "a", "kind"), json!("work"));
    // Nor by the database: the column is in `jobs_locked`.
    let e = fixture_error(&env, "UPDATE jobs SET kind = 'docs' WHERE key = 'a'");
    assert!(e.contains("job_locked"), "{e}");
}

/// The message of the error `sql` fails with.
fn fixture_error(env: &TestEnv, sql: &str) -> String {
    store(env).write("fixture", |tx| exec(tx, sql)).unwrap_err().to_string()
}

// ---------- Tier rules (hs-d506): Workers use only S0-S4 and H0-H4 ----------

/// The refusal of hs-d506: `tier_not_allowed`, exit 2, naming the rule, its reason and the allowed range.
fn not_allowed(run: &Run, tier: &str) {
    let e = refused(run, "tier_not_allowed", 2);
    assert_eq!(e["details"]["tier"], tier, "{run:?}");
    assert_eq!(e["details"]["allowed"], "S0-S4, H0-H4", "{run:?}");
    assert_eq!(e["details"]["rule"], "hs-d506", "{run:?}");
    let message = e["message"].as_str().unwrap();
    for part in ["hs-d506", "cost", "S0-S4", "H0-H4", "never O", "never effort 5"] {
        assert!(message.contains(part), "`{part}` missing from: {message}");
    }
}

#[test]
fn add_refuses_an_o_tier_or_effort_5_and_writes_nothing() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    let (txns, evs, rows) = (count(&env, "txns"), count(&env, "events"), count(&env, "jobs"));
    for tier in ["O1", "O2", "O3", "O4", "S5"] {
        not_allowed(&job_add(&env, "pl", "k", tier, &[]), tier);
    }
    // A stub draft's Tier is checked too.
    not_allowed(&stub_add(&env, "pl", "k", &["--tier", "O3"]), "O3");
    not_allowed(&stub_add(&env, "pl", "k", &["--tier", "S5"]), "S5");
    assert_eq!((count(&env, "txns"), count(&env, "events"), count(&env, "jobs")), (txns, evs, rows));
}

#[test]
fn add_accepts_s1_to_s4_and_h1_to_h3() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    for tier in ["S1", "S2", "S3", "S4", "H1", "H2", "H3"] {
        let key = format!("j-{}", tier.to_lowercase());
        assert_eq!(job_add(&env, "pl", &key, tier, &[]).ok()["job"]["tier"], tier);
        assert_eq!(job_row(&env, "pl", &key, "tier"), tier);
    }
}

#[test]
fn add_says_tier_invalid_before_tier_not_allowed() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    refused(&job_add(&env, "pl", "k", "O9", &[]), "tier_invalid", 2);
    refused(&job_add(&env, "pl", "k", "X5", &[]), "tier_invalid", 2);
}

#[test]
fn edit_refuses_an_o_tier_or_effort_5_and_writes_nothing() {
    let env = TestEnv::new();
    drafted(&env);
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
    for tier in ["O1", "O3", "O4", "S5"] {
        not_allowed(&job(&env, &["edit", "b", "--tier", tier], "pl"), tier);
        not_allowed(&job(&env, &["edit", "b", "--tier", tier, "--title", "New"], "pl"), tier);
    }
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
    assert_eq!(job_row(&env, "pl", "b", "tier"), "S2");
    assert_eq!(job_row(&env, "pl", "b", "title"), "Title of b");
}

#[test]
fn edit_accepts_s1_to_s4_and_h1_to_h3() {
    let env = TestEnv::new();
    drafted(&env);
    for tier in ["S1", "S4", "H1", "H2", "H3", "S3"] {
        assert_eq!(job(&env, &["edit", "b", "--tier", tier], "pl").ok()["fields"], json!(["tier"]), "{tier}");
        assert_eq!(job_row(&env, "pl", "b", "tier"), tier);
    }
}

#[test]
fn a_draft_stored_on_a_refused_tier_is_repaired_by_edit_and_other_edits_are_not_blocked() {
    let env = TestEnv::new();
    drafted(&env);
    // A store written before hs-d506.
    fixture(&env, "UPDATE jobs SET tier = 'O3' WHERE plan = 'pl' AND key = 'b';");
    assert_eq!(job(&env, &["edit", "b", "--title", "Renamed"], "pl").ok()["fields"], json!(["title"]));
    assert_eq!(job_row(&env, "pl", "b", "tier"), "O3", "no migration and no edit rewrites it");
    not_allowed(&job(&env, &["edit", "b", "--tier", "O4"], "pl"), "O4");
    assert_eq!(job(&env, &["edit", "b", "--tier", "S3"], "pl").ok()["fields"], json!(["tier"]));
    assert_eq!(job_row(&env, "pl", "b", "tier"), "S3");
}

#[test]
fn retry_refuses_an_o_tier_or_effort_5_checks_it_first_and_writes_nothing() {
    let env = TestEnv::new();
    drafted(&env);
    job_add(&env, "pl", "c", "S2", &[]).ok();
    lock(&env, "pl");
    engine(&env, "pl", "a", "failed");
    engine(&env, "pl", "c", "running");
    let (txns, evs) = (count(&env, "txns"), count(&env, "events"));
    for tier in ["O1", "O3", "O4", "O5", "S5"] {
        not_allowed(&job(&env, &["retry", "a", "--tier", tier], "pl"), tier);
    }
    // Checked first: a Job that is not failed still gets the Tier refusal.
    not_allowed(&job(&env, &["retry", "c", "--tier", "O4"], "pl"), "O4");
    assert_eq!((count(&env, "txns"), count(&env, "events")), (txns, evs));
    assert_eq!(job_row(&env, "pl", "a", "status"), "failed");
    assert_eq!(job_row(&env, "pl", "a", "retry_tier"), Value::Null);
}

#[test]
fn retry_accepts_s1_to_s4_and_h1_to_h3() {
    for tier in ["S1", "S4", "H1", "H3"] {
        let env = TestEnv::new();
        drafted(&env);
        lock(&env, "pl");
        engine(&env, "pl", "a", "failed");
        let out = job(&env, &["retry", "a", "--tier", tier], "pl").ok();
        assert_eq!(out, json!({"ok": true, "job": "a", "status": "queued", "tier": tier}));
        assert_eq!(job_row(&env, "pl", "a", "retry_tier"), tier);
    }
}

// ---------- the shape's `shape_incomplete` and `test_job_required` at the review, the lock and the cancel; `job approve-tests` ----------
// (contract §2.2 "Job kinds", §7.7; commands.md §4.6, §4.8, §5.3, §5.9)

/// `plan review` of Plan `plan`.
fn review(env: &TestEnv, plan: &str) -> Run {
    env.cpd(&["plan", "review", "--title", "The batch", "--plan", plan])
}

/// `plan lock` of Plan `plan`.
fn plan_lock(env: &TestEnv, plan: &str) -> Run {
    env.cpd(&["plan", "lock", "--plan", plan])
}

/// `job add <key> --kind <kind> --needs <needs>` into PR `pr`, with the Spec of the kind (a `test` Job names `t::<key>` in `tests/<key>.rs`).
fn add_needing(env: &TestEnv, plan: &str, key: &str, kind: &str, needs: &[&str], pr: &str) -> Run {
    let spec = if kind == "test" {
        test_spec_file(env, &format!("{key}.json"), json!([format!("t::{key}")]), json!([format!("tests/{key}.rs")]))
    } else {
        spec_file(env)
    };
    let needs = needs.join(",");
    let title = format!("Title of {key}");
    let mut args = vec!["job", "add", key, "--title", &title, "--tier", "S2", "--spec", &spec, "--kind", kind, "--pr", pr, "--plan", plan];
    if !needs.is_empty() {
        args.extend(["--needs", &needs]);
    }
    env.cpd(&args)
}

#[test]
fn plan_review_and_plan_lock_refuse_an_impl_draft_that_needs_no_test_job_of_its_pr() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    add_kind(&env, "pl", "t", "test", &test_spec_file(&env, "t.json", json!(["t::one"]), json!(["tests/t.rs"])), "repo").ok();
    // A draft is added; it is checked where it stops being one. It needs a Job of its PR that is no test Job.
    job_add(&env, "pl", "w", "S2", &[]).ok();
    add_needing(&env, "pl", "i", "impl", &["w"], "repo").ok();
    let writes = (count(&env, "txns"), count(&env, "events"));
    let e = refused(&review(&env, "pl"), "test_job_required", 3);
    assert_eq!(e["details"]["jobs"], json!(["i"]));
    // `plan lock` refuses it before it looks at the review (which does not exist).
    let e = refused(&plan_lock(&env, "pl"), "test_job_required", 3);
    assert_eq!(e["details"]["jobs"], json!(["i"]));
    assert_eq!((count(&env, "txns"), count(&env, "events"), count(&env, "plan_reviews")), (writes.0, writes.1, 0), "a refusal writes nothing");

    // It needs the test Job: the review is requested.
    job(&env, &["edit", "i", "--needs", "t"], "pl").ok();
    let out = review(&env, "pl").ok();
    assert_eq!(out["review"]["status"], "pending");
}

#[test]
fn plan_review_and_plan_lock_refuse_a_tdd_pr_with_no_test_or_no_impl_job() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    shaped_pr(&env, "pl", "api", "tdd");
    // A PR of a shape that is not `tdd`, and a PR from before the migration, are not asked for a test Job.
    shaped_pr(&env, "pl", "docs", "docs");
    shaped_pr(&env, "pl", "cfg", "config");
    add_kind(&env, "pl", "docs-d", "docs", &spec_file(&env), "docs").ok();
    add_needing(&env, "pl", "cfg-w", "work", &[], "cfg").ok();
    add_needing(&env, "pl", "free-w", "work", &[], "repo").ok();
    // `api` has a docs Job only: it lacks both.
    add_kind(&env, "pl", "api-d", "docs", &spec_file(&env), "api").ok();
    let writes = (count(&env, "txns"), count(&env, "events"));
    let e = refused(&review(&env, "pl"), "shape_incomplete", 3);
    assert_eq!((&e["details"]["pr"], &e["details"]["missing"]), (&json!("api"), &json!(["test", "impl"])));
    let e = refused(&plan_lock(&env, "pl"), "shape_incomplete", 3);
    assert_eq!((&e["details"]["pr"], &e["details"]["missing"]), (&json!("api"), &json!(["test", "impl"])));
    assert_eq!((count(&env, "txns"), count(&env, "events"), count(&env, "plan_reviews")), (writes.0, writes.1, 0));

    // A test Job but no impl Job.
    add_needing(&env, "pl", "api-t", "test", &["api-d"], "api").ok();
    let e = refused(&review(&env, "pl"), "shape_incomplete", 3);
    assert_eq!(e["details"]["missing"], json!(["impl"]));
    // A cancelled one does not count.
    add_needing(&env, "pl", "api-i", "impl", &["api-t"], "api").ok();
    fixture(&env, "UPDATE jobs SET status = 'cancelled', cancel_reason = 'x', cancel_requested_at = 't' WHERE key = 'api-i'");
    assert_eq!(refused(&review(&env, "pl"), "shape_incomplete", 3)["details"]["missing"], json!(["impl"]));
    // A live one completes the PR: the review is requested, and `plan lock` goes on to the review it lacks.
    add_needing(&env, "pl", "api-i2", "impl", &["api-t"], "api").ok();
    assert_eq!(review(&env, "pl").ok()["review"]["status"], "pending");
    refused(&plan_lock(&env, "pl"), "plan_not_approved", 3);
}

#[test]
fn job_cancel_keeps_the_last_test_and_the_last_impl_job_of_a_tdd_pr_that_has_not_opened() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    shaped_pr(&env, "pl", "api", "tdd");
    add_kind(&env, "pl", "d", "docs", &spec_file(&env), "api").ok();
    add_needing(&env, "pl", "t", "test", &["d"], "api").ok();
    add_needing(&env, "pl", "i", "impl", &["t"], "api").ok();
    // A stub the PR does not need goes.
    job(&env, &["cancel", "d", "--reason", "No docs."], "pl").ok();
    // The last `impl` and the last `test` Job do not: `details` `{pr, missing}`, nothing written.
    let writes = (count(&env, "txns"), count(&env, "events"));
    let e = refused(&job(&env, &["cancel", "i", "--reason", "No."], "pl"), "shape_incomplete", 3);
    assert_eq!((&e["details"]["pr"], &e["details"]["missing"]), (&json!("api"), &json!(["impl"])));
    let e = refused(&job(&env, &["cancel", "t", "--reason", "No."], "pl"), "shape_incomplete", 3);
    assert_eq!((&e["details"]["pr"], &e["details"]["missing"]), (&json!("api"), &json!(["test"])));
    assert_eq!((count(&env, "txns"), count(&env, "events")), writes);
    assert_eq!((job_row(&env, "pl", "i", "status"), job_row(&env, "pl", "t", "status")), (json!("draft"), json!("draft")));
    // A replacement is added first; then the first one goes, and the replacement is the last.
    add_needing(&env, "pl", "i2", "impl", &["t"], "api").ok();
    job(&env, &["cancel", "i", "--reason", "Replaced."], "pl").ok();
    assert_eq!(job_row(&env, "pl", "i", "status"), json!("cancelled"));
    let e = refused(&job(&env, &["cancel", "i2", "--reason", "No."], "pl"), "shape_incomplete", 3);
    assert_eq!(e["details"]["missing"], json!(["impl"]));

    // After the lock (a `queued` Job) it is the same while the PR has not opened.
    lock(&env, "pl");
    let e = refused(&job(&env, &["cancel", "i2", "--reason", "No."], "pl"), "shape_incomplete", 3);
    assert_eq!(e["details"]["missing"], json!(["impl"]));
    assert_eq!(job_row(&env, "pl", "i2", "status"), json!("queued"));
}

#[test]
fn job_cancel_checks_no_shape_for_a_pr_that_is_not_tdd_or_that_has_opened() {
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "plan");
    for shape in ["docs", "config", "tdd"] {
        shaped_pr(&env, "pl", shape, shape);
    }
    add_kind(&env, "pl", "d", "docs", &spec_file(&env), "docs").ok();
    add_needing(&env, "pl", "c", "work", &[], "config").ok();
    // A PR from before the migration (`free`) has any kinds, and any of them is cancelled.
    add_kind(&env, "pl", "free-i", "impl", &spec_file(&env), "repo").ok();
    // The contrast: the lone `docs` Job of a `tdd` PR goes, its lone `impl` Job does not.
    add_kind(&env, "pl", "tdd-d", "docs", &spec_file(&env), "tdd").ok();
    add_kind(&env, "pl", "tdd-i", "impl", &spec_file(&env), "tdd").ok();
    refused(&job(&env, &["cancel", "tdd-i", "--reason", "No."], "pl"), "shape_incomplete", 3);
    job(&env, &["cancel", "tdd-d", "--reason", "No."], "pl").ok();
    for key in ["d", "c", "free-i"] {
        job(&env, &["cancel", key, "--reason", "No."], "pl").ok();
        assert_eq!(job_row(&env, "pl", key, "status"), json!("cancelled"), "{key}");
    }

    // A `tdd` PR that has opened its pull request: a fix is whatever it is, and the last `impl` Job may go.
    let env = TestEnv::new();
    setup(&env);
    plan(&env, "pl", "deploy");
    fixture(
        &env,
        &format!(
            "INSERT INTO prs (plan, key, repo, base, status, lead_branch, shape, created_txn, created_at)
             VALUES ('pl', 'fix', '/repo', 'main', 'waiting', 'cpd/pl/fix', 'tdd', cpd_txn(), 't');
             UPDATE prs SET status = 'dispatch', lead_worktree = '/wt2', started_at = 't' WHERE key = 'fix';
             {}
             UPDATE prs SET status = 'open', pr_number = 2, pr_url = 'https://github.com/acme/fix/pull/2', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/fix') WHERE key = 'fix';
             INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                               kind, tests, test_files, created_txn, created_at)
             VALUES ('pl', 'fix', 'fix-t', 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 0, printf('%064d', 1),
                     'test', '[\"t::a\"]', '[\"tests/a.rs\"]', cpd_txn(), 't'),
                    ('pl', 'fix', 'fix-i', 'I', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 0, printf('%064d', 2),
                     'impl', NULL, NULL, cpd_txn(), 't');",
            pull_request("fix", 2, "open")
        ),
    );
    job(&env, &["cancel", "fix-i", "--reason", "Not needed."], "pl").ok();
    job(&env, &["cancel", "fix-t", "--reason", "Not needed."], "pl").ok();
    assert_eq!((job_row(&env, "pl", "fix-i", "status"), job_row(&env, "pl", "fix-t", "status")), (json!("cancelled"), json!("cancelled")));
}

/// A Plan `pl` locked with the test Job `t` (`tests/t.rs`, `tests/more/`) and the impl Job `i` that needs it, both queued.
fn locked_tdd(env: &TestEnv) {
    setup(env);
    plan(env, "pl", "plan");
    let tests = test_spec_file(env, "t.json", json!(["t::one"]), json!(["tests/t.rs", "tests/more/"]));
    add_kind(env, "pl", "t", "test", &tests, "repo").ok();
    add_needing(env, "pl", "i", "impl", &["t"], "repo").ok();
}

fn approve(env: &TestEnv, args: &[&str]) -> Run {
    let mut all = vec!["job", "approve-tests"];
    all.extend(args);
    all.extend(["--plan", "pl"]);
    env.cpd(&all)
}

#[test]
fn approve_tests_records_the_paths_the_impl_job_may_change_and_tells_the_event() {
    let env = TestEnv::new();
    locked_tdd(&env);
    // A draft is not a Job that can be checked yet.
    refused(&approve(&env, &["i", "--path", "tests/t.rs", "--reason", "Wrong."]), "illegal_transition", 3);
    lock(&env, "pl");
    let out = approve(&env, &["i", "--path", "tests/t.rs", "--reason", "The assertion is wrong."]).ok();
    assert_eq!(
        out,
        json!({"ok": true, "job": {"key": "i", "status": "queued", "kind": "impl"}, "approved": ["tests/t.rs"], "fields": ["test_approved"]})
    );
    assert_eq!(job_row(&env, "pl", "i", "test_approved"), json!("[\"tests/t.rs\"]"));
    let edited = events(&env, "job-edited");
    let last = edited.last().unwrap();
    assert_eq!((&last["job"], &last["data"]), (&json!("i"), &json!({"fields": ["test_approved"]})));
    let text = last["text"].as_str().unwrap();
    assert!(text.contains("tests/t.rs") && text.contains("The assertion is wrong."), "{text}");

    // The union of what was approved; a path inside a guarded directory counts; paths already approved write nothing.
    let out = approve(&env, &["i", "--path", "tests/more/m.rs", "--path", "tests/t.rs", "--reason", "More."]).ok();
    let sorted = |v: &Value| {
        let mut all: Vec<String> = serde_json::from_value(v.clone()).unwrap();
        all.sort();
        all
    };
    assert_eq!(sorted(&out["approved"]), ["tests/more/m.rs", "tests/t.rs"], "`approved` is what the Job has now: the union");
    let stored: Value = serde_json::from_str(job_row(&env, "pl", "i", "test_approved").as_str().unwrap()).unwrap();
    assert_eq!(sorted(&stored), ["tests/more/m.rs", "tests/t.rs"]);
    let writes = (count(&env, "txns"), count(&env, "events"));
    let out = approve(&env, &["i", "--path", "tests/t.rs", "--reason", "Again."]).ok();
    assert_eq!((&out["already"], &out["fields"]), (&json!(true), &json!([])));
    assert_eq!((count(&env, "txns"), count(&env, "events")), writes);

    // Not a Locked field: the Job's fingerprint is what it was.
    assert_eq!(job_row(&env, "pl", "i", "fingerprint"), json!(locked_fingerprint_of(&env, "i")));
}

/// The Locked-Job fingerprint of Job `key` of Plan `pl`, recomputed.
fn locked_fingerprint_of(env: &TestEnv, key: &str) -> String {
    let id = PlanId::parse("pl").unwrap();
    let all = store(env).read(|conn| jobs::load(conn, &id)).unwrap();
    locked_fingerprint(all.iter().find(|j| j.key == key).unwrap())
}

#[test]
fn approve_tests_is_the_leads_and_not_a_workers() {
    let env = TestEnv::new();
    locked_tdd(&env);
    lock(&env, "pl");
    let args = ["job", "approve-tests", "i", "--path", "tests/t.rs", "--reason", "Wrong.", "--plan", "pl"];
    let writes = (count(&env, "txns"), count(&env, "events"));
    refused(&env.cpd_with(env.root(), &args, &[("CPD_ATTEMPT", "i-a1")]), "not_a_lead", 3);
    assert_eq!((count(&env, "txns"), count(&env, "events")), writes);
    assert_eq!(job_row(&env, "pl", "i", "test_approved"), Value::Null);
    env.cpd_with(env.root(), &args, &[]).ok();
}

#[test]
fn approve_tests_refuses_what_it_cannot_approve_and_writes_nothing() {
    let env = TestEnv::new();
    locked_tdd(&env);
    lock(&env, "pl");
    let writes = || (count(&env, "txns"), count(&env, "events"));
    let before = writes();
    refused(&approve(&env, &["ghost", "--path", "tests/t.rs", "--reason", "Wrong."]), "job_not_found", 4);
    // Only an `impl` Job is approved for; a `test` Job guards its own files.
    refused(&approve(&env, &["t", "--path", "tests/t.rs", "--reason", "Wrong."]), "not_an_impl_job", 3);
    // Every path must be guarded by the `test_files` of a `test` Job it needs; the unguarded are listed and nothing is approved.
    let e = refused(&approve(&env, &["i", "--path", "tests/t.rs", "--path", "src/lib.rs", "--path", "tests/other.rs", "--reason", "Wrong."]), "path_not_guarded", 3);
    assert_eq!(e["details"]["paths"], json!(["src/lib.rs", "tests/other.rs"]));
    assert_eq!(job_row(&env, "pl", "i", "test_approved"), Value::Null);
    // A path and a reason are required.
    refused(&approve(&env, &["i", "--reason", "Wrong."]), "usage", 2);
    refused(&approve(&env, &["i", "--path", "tests/t.rs"]), "usage", 2);
    assert_eq!(writes(), before);
    // A Job that has ended is not approved for.
    engine(&env, "pl", "i", "merged");
    refused(&approve(&env, &["i", "--path", "tests/t.rs", "--reason", "Wrong."]), "illegal_transition", 3);
}
