//! Jobs (contract §2.3, §3.2; commands.md §5.1–§5.4, §5.6, §5.7), run by the
//! command tree in this process (`Fx`) against a temporary `$CPD_HOME`. Plans, Attempts and the
//! states only the engine reaches are written straight into the database, as
//! fixtures; the commands under test are the only ones that write Jobs.
//! pi is a fake install whose Tier catalog is seeded in the cache, so no node runs.

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::fx::Fx;
use common::{Run, TestEnv, refused};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use cpd_core::plan::jobs;
use cpd_core::plan::locked_fingerprint;
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};

// ---------- fixtures ----------

pub(super) fn store(env: &TestEnv) -> Store {
    Store::open(
        &CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(),
        5000,
    )
    .unwrap()
}

pub(super) fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

/// The pull request row (`acme/<repo>#<number>`, migration 5) that a PR in status `open` (or `merged`) points to.
pub(super) fn pull_request(repo: &str, number: i64, state: &str) -> String {
    let merged = if state == "merged" { "'t'" } else { "NULL" };
    format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
         VALUES ('acme/{repo}', {number}, 'https://github.com/acme/{repo}/pull/{number}', 'T', 0, '{state}', 'none', 'none', 't', {merged}, 't');"
    )
}

/// Runs `sql` in one write transaction.
pub(super) fn fixture(env: &TestEnv, sql: &str) {
    store(env).write("fixture", |tx| exec(tx, sql)).unwrap();
}

/// O supports efforts 0–5, S 0–5, H 0–3.
const EFFORTS: [(&str, &str, &str); 3] = [
    ("O", "p/opus", "[0,1,2,3,4,5]"),
    ("S", "p/sonnet", "[0,1,2,3,4,5]"),
    ("H", "p/haiku", "[0,1,2,3]"),
];

/// `config.toml`, a fake pi install, and its Tier catalog in the cache.
pub(super) fn setup(env: &TestEnv) {
    env.write_models();
    fs::write(
        env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"m1\"\n",
    )
    .unwrap();
    env.fake_pi("exit 0");
    fs::create_dir_all(env.root().join("install")).unwrap();
    fs::write(env.root().join("install/current-version"), "1.0.0\n").unwrap();
    let scope = env
        .root()
        .join("install/releases/1.0.0/node_modules/@earendil-works");
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
    // The Context's owner is the key of the PRs' policy, where a Job's Tier models are set: its file is empty.
    env.owner_policy("acme", "");
    fixture(
        env,
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Context', 'acme', 't', 'm1');",
    );
}

/// Plan `id` in `phase`, with its Lead session, moved there one phase at a time.
pub(super) fn plan(env: &TestEnv, id: &str, phase: &str) {
    // (phase, what else the Plan gets, its PR's status and what else the PR gets)
    const STEPS: [(&str, &str, &str, &str); 4] = [
        ("plan", "", "waiting", ""),
        (
            "dispatch",
            ", locked_at = 't'",
            "dispatch",
            ", lead_worktree = '/wt', started_at = 't'",
        ),
        (
            "deploy",
            "",
            "open",
            ", pr_number = 1, pr_url = 'https://github.com/acme/{id}/pull/1', pr_opened_at = 't', pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/{id}')",
        ),
        ("done", ", done_at = 't'", "merged", ", merged_at = 't'"),
    ];
    let mut sql = format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at)
         VALUES ('{id}', 'ctx', 'Plan {id}', 'grounding', 'lead-{id}', 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
         VALUES ('{id}', 'repo', '/repo', 'main', 'waiting', 'cpd/{id}', cpd_txn(), 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
         VALUES ('lead-{id}', 'lead', 'ctx', '{id}', '{id}', '/repo', 'O3', 'stopped', 't', 't');"
    );
    if phase != "grounding" {
        let last = STEPS
            .iter()
            .position(|(p, ..)| *p == phase)
            .unwrap_or_else(|| panic!("{phase}"));
        for (step, set, status, pr_set) in &STEPS[..=last] {
            sql += &format!("UPDATE plans SET phase = '{step}'{set} WHERE id = '{id}';");
            if *step == "deploy" {
                sql += &pull_request(id, 1, "open");
            }
            sql += &format!(
                "UPDATE workflows SET status = '{status}'{} WHERE plan = '{id}';",
                pr_set.replace("{id}", id)
            );
            if *step == "done" {
                sql += &format!(
                    "UPDATE pull_requests SET state = 'merged', merged_at = 't' WHERE repo = 'acme/{id}';"
                );
            }
        }
    }
    fixture(env, &sql);
}

pub(super) fn spec_json() -> Value {
    json!({"goal": "Do it.", "context": "Because.", "touches": ["src/a.rs"], "acceptance": ["cargo test"], "out_of_scope": ["Everything else."]})
}

pub(super) fn write_spec(env: &TestEnv, name: &str, text: &str) -> String {
    let path: PathBuf = env.root().join(name);
    fs::write(&path, text).unwrap();
    path.to_str().unwrap().to_owned()
}

pub(super) fn spec_file(env: &TestEnv) -> String {
    write_spec(env, "spec.json", &spec_json().to_string())
}

pub(super) fn job_add(fx: &Fx, plan: &str, key: &str, tier: &str, needs: &[&str]) -> Run {
    let spec = spec_file(&fx.env);
    let title = format!("Title of {key}");
    let needs = needs.join(",");
    let mut args = vec![
        "job", "add", key, "--title", &title, "--tier", tier, "--spec", &spec, "--plan", plan,
    ];
    if !needs.is_empty() {
        args.extend(["--needs", &needs]);
    }
    fx.cpd(&args)
}

pub(super) fn job(env: &Fx, args: &[&str], plan: &str) -> Run {
    let mut all = vec!["job"];
    all.extend(args);
    all.extend(["--plan", plan]);
    env.cpd(&all)
}

pub(super) fn events(fx: &Fx, kind: &str) -> Vec<Value> {
    fx.cpd(&["events", "list", "--kind", kind]).ok()["events"]
        .as_array()
        .unwrap()
        .clone()
}

pub(super) fn count(env: &TestEnv, table: &str) -> i64 {
    env.query(&format!("SELECT count(*) FROM {table}"))
}

/// A Plan `p` in phase `plan` with draft Jobs `a` and `b` (needs `a`).
pub(super) fn drafted(fx: &Fx) {
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    job_add(fx, "pl", "a", "S2", &[]).ok();
    job_add(fx, "pl", "b", "S2", &["a"]).ok();
}

/// What `plan lock` does to the Jobs: phase `dispatch`, every draft `queued`
/// with its Locked-Job fingerprint.
pub(super) fn lock(env: &TestEnv, plan_id: &str) {
    let id = PlanId::parse(plan_id).unwrap();
    let all = store(env).read(|conn| jobs::load(conn, &id)).unwrap();
    let mut sql = format!(
        "UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = '{plan_id}';
         UPDATE workflows SET status = 'dispatch', lead_worktree = '/wt', started_at = 't' WHERE plan = '{plan_id}';"
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
pub(super) fn engine(env: &TestEnv, plan_id: &str, key: &str, status: &str) {
    let attempt = format!("INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
        VALUES ('{plan_id}', '{key}-a1', '{key}', 1, 'S2', 'ladder', 'running', 'cpd/{plan_id}--{key}-a1', '/w', 't');");
    let ended = |outcome: &str, cause: &str| {
        format!(
            "UPDATE attempts SET status = 'ended', outcome = '{outcome}', cause = '{cause}', ended_at = 't' WHERE plan = '{plan_id}' AND id = '{key}-a1';"
        )
    };
    let set =
        |sql: &str| format!("UPDATE jobs SET {sql} WHERE plan = '{plan_id}' AND key = '{key}';");
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

pub(super) fn job_row(env: &TestEnv, plan_id: &str, key: &str, column: &str) -> Value {
    store(env)
        .read(|conn| {
            Ok(conn.query_row(
                &format!("SELECT {column} FROM jobs WHERE plan = ?1 AND key = ?2"),
                [plan_id, key],
                |r| {
                    Ok(match r.get_ref(0)? {
                        rusqlite::types::ValueRef::Null => Value::Null,
                        rusqlite::types::ValueRef::Integer(i) => json!(i),
                        rusqlite::types::ValueRef::Text(t) => json!(String::from_utf8_lossy(t)),
                        other => panic!("{other:?}"),
                    })
                },
            )?)
        })
        .unwrap()
}

// ---------- job add ----------

#[test]
fn add_in_phase_plan_writes_a_draft_job_its_needs_and_an_event() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let out = job_add(&fx, "pl", "a", "S2", &[]).ok();
    assert_eq!(
        out["job"],
        json!({"key": "a", "pr": "repo", "kind": "work", "status": "draft", "appended": false, "tier": "S2", "needs": [], "urgent": false})
    );
    let out = job_add(&fx, "pl", "b", "S3", &["a"]).ok();
    assert_eq!(
        out["job"],
        json!({"key": "b", "pr": "repo", "kind": "work", "status": "draft", "appended": false, "tier": "S3", "needs": ["a"], "urgent": false})
    );
    assert!(out.get("already").is_none());

    assert_eq!(job_row(&fx.env, "pl", "b", "title"), "Title of b");
    assert_eq!(job_row(&fx.env, "pl", "b", "goal"), "Do it.");
    assert_eq!(job_row(&fx.env, "pl", "b", "context"), "Because.");
    assert_eq!(job_row(&fx.env, "pl", "b", "touches"), "[\"src/a.rs\"]");
    assert_eq!(
        job_row(&fx.env, "pl", "b", "acceptance"),
        "[\"cargo test\"]"
    );
    assert_eq!(
        job_row(&fx.env, "pl", "b", "out_of_scope"),
        "[\"Everything else.\"]"
    );
    assert_eq!(job_row(&fx.env, "pl", "b", "status"), "draft");
    assert_eq!(job_row(&fx.env, "pl", "b", "appended"), 0);
    assert_eq!(job_row(&fx.env, "pl", "b", "fingerprint"), Value::Null);
    assert!(
        job_row(&fx.env, "pl", "b", "seq").as_i64() > job_row(&fx.env, "pl", "a", "seq").as_i64()
    );
    assert_eq!(
        fx.env
            .query::<String>("SELECT needs FROM dependencies WHERE job = 'b'"),
        "a"
    );

    let added = events(&fx, "job-added");
    assert_eq!(added.len(), 2);
    assert_eq!(added[1]["job"], "b");
    assert_eq!(added[1]["plan"], "pl");
    assert_eq!(added[1]["context"], "ctx");
    assert_eq!(
        added[1]["data"],
        json!({"workflow": "repo", "kind": "work", "status": "draft", "appended": false, "tier": "S3", "needs": ["a"], "urgent": false})
    );
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM history WHERE tbl = 'jobs' AND op = 'insert'"),
        2
    );
}

#[test]
fn add_in_phase_dispatch_or_deploy_appends_a_queued_job_with_its_fingerprint() {
    for phase in ["dispatch", "deploy"] {
        let fx = Fx::new();
        setup(&fx.env);
        plan(&fx.env, "pl", "plan");
        job_add(&fx, "pl", "a", "S2", &[]).ok();
        lock(&fx.env, "pl");
        if phase == "deploy" {
            fixture(
                &fx.env,
                &format!(
                    "UPDATE plans SET phase = 'deploy' WHERE id = 'pl';
                     {}
                     UPDATE workflows SET status = 'open', pr_number = 1, pr_url = 'https://github.com/acme/pl/pull/1', pr_opened_at = 't',
                            pull_request = (SELECT id FROM pull_requests WHERE number = 1) WHERE plan = 'pl';",
                    pull_request("pl", 1, "open")
                ),
            );
        }
        let out = job_add(&fx, "pl", "late", "H3", &["a"]).ok();
        assert_eq!(
            out["job"],
            json!({"key": "late", "pr": "repo", "kind": "work", "status": "queued", "appended": true, "tier": "H3", "needs": ["a"], "urgent": false}),
            "{phase}"
        );
        assert_eq!(job_row(&fx.env, "pl", "late", "appended"), 1);
        let id = PlanId::parse("pl").unwrap();
        let all = store(&fx.env).read(|conn| jobs::load(conn, &id)).unwrap();
        let late = all.iter().find(|j| j.key == "late").unwrap();
        assert_eq!(
            late.fingerprint.as_deref(),
            Some(locked_fingerprint(late).as_str()),
            "{phase}"
        );
        assert_eq!(
            events(&fx, "job-added").last().unwrap()["data"]["status"],
            "queued"
        );
        // It is Locked from its first transaction: nothing can change it.
        assert_eq!(
            job(&fx, &["edit", "late", "--title", "x"], "pl").error()["code"],
            "job_locked"
        );
    }
}

#[test]
fn add_again_with_the_same_fields_is_already_and_writes_nothing() {
    let fx = Fx::new();
    drafted(&fx);
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    let out = job_add(&fx, "pl", "b", "S2", &["a"]).ok();
    assert_eq!(out["already"], true);
    assert_eq!(out["job"]["key"], "b");
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );

    let run = job_add(&fx, "pl", "b", "S3", &["a"]);
    refused(&run, "job_key_exists", 3);
    let run = job_add(&fx, "pl", "b", "S2", &[]);
    refused(&run, "job_key_exists", 3);
    let other_spec = write_spec(&fx.env, "other.json", &json!({"goal": "Else.", "context": "c", "touches": ["x"], "acceptance": ["y"], "out_of_scope": []}).to_string());
    let run = fx.cpd(&[
        "job",
        "add",
        "b",
        "--title",
        "Title of b",
        "--tier",
        "S2",
        "--spec",
        &other_spec,
        "--needs",
        "a",
        "--plan",
        "pl",
    ]);
    refused(&run, "job_key_exists", 3);
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );
}

#[test]
fn add_refusals_write_nothing() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "gr", "grounding");
    plan(&fx.env, "dn", "done");
    plan(&fx.env, "pl", "plan");
    job_add(&fx, "pl", "a", "S2", &[]).ok();
    job_add(&fx, "pl", "x", "S2", &[]).ok();
    job_add(&fx, "pl", "y", "S2", &[]).ok();
    lock(&fx.env, "pl");
    engine(&fx.env, "pl", "x", "failed");
    job(&fx, &["cancel", "y", "--reason", "No."], "pl").ok();
    let (txns, evs, rows) = (
        count(&fx.env, "txns"),
        count(&fx.env, "events"),
        count(&fx.env, "jobs"),
    );

    // The phase.
    for plan_id in ["gr", "dn"] {
        let e = refused(&job_add(&fx, plan_id, "k", "S2", &[]), "illegal_phase", 3);
        assert_eq!(
            e["details"]["phase"],
            if plan_id == "gr" { "grounding" } else { "done" }
        );
    }
    refused(&job_add(&fx, "nope", "k", "S2", &[]), "plan_not_found", 4);
    // The needs.
    let e = refused(
        &job_add(&fx, "pl", "k", "S2", &["a", "ghost", "ghost2"]),
        "needs_unknown",
        3,
    );
    assert_eq!(e["details"]["needs"], json!(["ghost", "ghost2"]));
    let e = refused(
        &job_add(&fx, "pl", "k", "S2", &["x", "y", "a"]),
        "needs_unavailable",
        3,
    );
    assert_eq!(
        e["details"]["needs"],
        json!([{"key": "x", "status": "failed"}, {"key": "y", "status": "cancelled"}])
    );
    // The flags.
    let spec = spec_file(&fx.env);
    let e = refused(
        &fx.cpd(&[
            "job", "add", "k", "--title", "T", "--tier", "H4", "--spec", &spec, "--plan", "pl",
        ]),
        "tier_invalid",
        2,
    );
    assert_eq!(e["details"]["efforts"], json!([0, 1, 2, 3]));
    refused(
        &fx.cpd(&[
            "job", "add", "k", "--title", "T", "--tier", "H4", "--spec", &spec, "--plan", "pl",
        ]),
        "tier_invalid",
        2,
    );
    refused(
        &fx.cpd(&[
            "job", "add", "k", "--title", "T", "--tier", "S9", "--spec", &spec, "--plan", "pl",
        ]),
        "tier_invalid",
        2,
    );
    refused(
        &fx.cpd(&[
            "job",
            "add",
            "k",
            "--title",
            "T",
            "--tier",
            "S2",
            "--spec",
            "/no/such/file",
            "--plan",
            "pl",
        ]),
        "file_unreadable",
        2,
    );
    refused(
        &fx.cpd(&[
            "job", "add", "k", "--title", "T", "--tier", "S2", "--spec", &spec, "--needs", "a,a",
            "--plan", "pl",
        ]),
        "usage",
        2,
    );
    refused(
        &fx.cpd(&[
            "job", "add", "k", "--title", "T", "--tier", "S2", "--spec", &spec, "--needs", "A",
            "--plan", "pl",
        ]),
        "invalid_job_id",
        2,
    );
    refused(
        &fx.cpd(&[
            "job", "add", "K", "--title", "T", "--tier", "S2", "--spec", &spec, "--plan", "pl",
        ]),
        "invalid_job_id",
        2,
    );
    refused(
        &fx.cpd(&[
            "job", "add", "k", "--tier", "S2", "--spec", &spec, "--plan", "pl",
        ]),
        "usage",
        2,
    );
    refused(
        &fx.cpd(&[
            "job", "add", "k", "--title", "T", "--tier", "S2", "--spec", &spec,
        ]),
        "plan_required",
        2,
    );
    assert_eq!(
        (
            count(&fx.env, "txns"),
            count(&fx.env, "events"),
            count(&fx.env, "jobs")
        ),
        (txns, evs, rows)
    );
}

#[test]
fn add_refuses_a_spec_that_is_not_complete_naming_the_json_path() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
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
        let file = write_spec(&fx.env, "bad.json", &text);
        let run = fx.cpd(&[
            "job", "add", "k", "--title", "T", "--tier", "S2", "--spec", &file, "--plan", "pl",
        ]);
        let e = refused(&run, "spec_invalid", 2);
        assert_eq!(e["details"]["path"], path, "{text}");
    }
    assert_eq!(count(&fx.env, "jobs"), 0);
    // An empty out_of_scope is fine.
    let file = write_spec(&fx.env, "ok.json", &with("out_of_scope", json!([])));
    fx.cpd(&[
        "job", "add", "k", "--title", "T", "--tier", "S2", "--spec", &file, "--plan", "pl",
    ])
    .ok();
}

#[test]
fn the_plan_comes_from_cpd_plan_when_no_flag_names_it() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let spec = spec_file(&fx.env);
    let out = fx
        .env
        .command(common::CPD)
        .env("CPD_PLAN", "pl")
        .args([
            "job", "add", "k", "--title", "T", "--tier", "S2", "--spec", &spec,
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(count(&fx.env, "jobs"), 1);
}

// ---------- job edit ----------

#[test]
fn edit_changes_the_given_fields_and_lists_only_those_that_changed() {
    let fx = Fx::new();
    drafted(&fx);
    job_add(&fx, "pl", "c", "S2", &[]).ok();
    let spec = write_spec(&fx.env, "new.json", &json!({"goal": "New goal.", "context": "Because.", "touches": ["src/a.rs"], "acceptance": ["cargo test"], "out_of_scope": []}).to_string());

    let run = job(
        &fx,
        &[
            "edit",
            "b",
            "--tier",
            "S3",
            "--needs",
            "a,c",
            "--title",
            "Title of b",
        ],
        "pl",
    );
    let out = run.ok();
    assert_eq!(out["job"], json!({"key": "b", "status": "draft"}));
    assert_eq!(out["fields"], json!(["tier", "needs"]));
    assert_eq!(job_row(&fx.env, "pl", "b", "tier"), "S3");
    assert_eq!(fx.env.query::<String>("SELECT group_concat(needs) FROM (SELECT needs FROM dependencies WHERE job = 'b' ORDER BY needs)"), "a,c");

    let out = job(
        &fx,
        &["edit", "b", "--spec", &spec, "--title", "New title"],
        "pl",
    )
    .ok();
    assert_eq!(out["fields"], json!(["title", "goal", "out_of_scope"]));
    assert_eq!(job_row(&fx.env, "pl", "b", "goal"), "New goal.");
    assert_eq!(job_row(&fx.env, "pl", "b", "out_of_scope"), "[]");
    assert_eq!(job_row(&fx.env, "pl", "b", "title"), "New title");

    let out = job(&fx, &["edit", "b", "--no-needs"], "pl").ok();
    assert_eq!(out["fields"], json!(["needs"]));
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM dependencies WHERE job = 'b'"),
        0
    );

    let edited = events(&fx, "job-edited");
    assert_eq!(edited.len(), 3);
    assert_eq!(edited[0]["data"], json!({"fields": ["tier", "needs"]}));
    assert_eq!(edited[0]["job"], "b");
    assert_eq!(edited[2]["data"], json!({"fields": ["needs"]}));
    // `a` was never touched.
    assert_eq!(job_row(&fx.env, "pl", "a", "tier"), "S2");
}

#[test]
fn edit_that_changes_nothing_is_already_and_writes_nothing() {
    let fx = Fx::new();
    drafted(&fx);
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    let spec = spec_file(&fx.env);
    let out = job(
        &fx,
        &[
            "edit",
            "b",
            "--tier",
            "S2",
            "--needs",
            "a",
            "--spec",
            &spec,
            "--title",
            "Title of b",
        ],
        "pl",
    )
    .ok();
    assert_eq!(out["already"], true);
    assert_eq!(out["fields"], json!([]));
    let out = job(&fx, &["edit", "a", "--no-needs"], "pl").ok();
    assert_eq!(out["already"], true);
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );
}

#[test]
fn edit_refusals_write_nothing() {
    let fx = Fx::new();
    drafted(&fx);
    job_add(&fx, "pl", "c", "S2", &["b"]).ok();
    job_add(&fx, "pl", "gone", "S2", &[]).ok();
    job(&fx, &["cancel", "gone", "--reason", "No."], "pl").ok();
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));

    let run = job(&fx, &["edit", "b"], "pl");
    refused(&run, "nothing_to_edit", 2);
    refused(
        &job(&fx, &["edit", "b", "--needs", "a", "--no-needs"], "pl"),
        "usage",
        2,
    );
    refused(
        &job(&fx, &["edit", "ghost", "--tier", "S3"], "pl"),
        "job_not_found",
        4,
    );
    refused(
        &job(&fx, &["edit", "b", "--tier", "S3"], "nope"),
        "plan_not_found",
        4,
    );
    refused(
        &job(&fx, &["edit", "b", "--needs", "ghost"], "pl"),
        "needs_unknown",
        3,
    );
    let e = refused(
        &job(&fx, &["edit", "b", "--needs", "gone"], "pl"),
        "needs_unavailable",
        3,
    );
    assert_eq!(
        e["details"]["needs"],
        json!([{"key": "gone", "status": "cancelled"}])
    );
    let e = refused(
        &job(&fx, &["edit", "a", "--needs", "c"], "pl"),
        "dependency_cycle",
        3,
    );
    assert_eq!(e["details"]["cycle"], json!(["a", "c", "b", "a"]));
    refused(
        &job(&fx, &["edit", "a", "--needs", "b"], "pl"),
        "dependency_cycle",
        3,
    );
    refused(
        &job(&fx, &["edit", "a", "--needs", "a"], "pl"),
        "dependency_cycle",
        3,
    );
    refused(
        &job(&fx, &["edit", "b", "--tier", "S6"], "pl"),
        "tier_invalid",
        2,
    );
    refused(
        &job(&fx, &["edit", "b", "--spec", "/no/such/file"], "pl"),
        "file_unreadable",
        2,
    );
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM dependencies"), 2);
}

#[test]
fn a_locked_job_cannot_be_edited_by_the_command_or_by_the_database() {
    let fx = Fx::new();
    drafted(&fx);
    lock(&fx.env, "pl");
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    for args in [
        &["edit", "a", "--title", "New"][..],
        &["edit", "b", "--no-needs"],
        &["edit", "b", "--needs", "a"],
        &["edit", "a", "--tier", "S3"],
    ] {
        let e = refused(&job(&fx, args, "pl"), "job_locked", 3);
        assert_eq!(e["details"]["status"], "queued", "{args:?}");
    }
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );
    // The trigger of §4.4 refuses the same writes.
    let result = store(&fx.env).write("fixture", |tx| {
        exec(tx, "UPDATE jobs SET title = 'Else.' WHERE key = 'a';")
    });
    assert_eq!(
        result.unwrap_err().code(),
        cpd_core::error::ErrorCode::JobLocked
    );
    let result = store(&fx.env).write("fixture", |tx| {
        exec(tx, "DELETE FROM dependencies WHERE job = 'b';")
    });
    assert_eq!(
        result.unwrap_err().code(),
        cpd_core::error::ErrorCode::JobLocked
    );
    // Other statuses are Locked too.
    engine(&fx.env, "pl", "a", "running");
    refused(
        &job(&fx, &["edit", "a", "--title", "New"], "pl"),
        "job_locked",
        3,
    );
}

/// B1 (hs-d573 O5): an `open` or `update` Job is cpd's, so `job edit` is refused `job_locked`, also while the `open` Job is a `draft`.
#[test]
fn job_edit_refuses_an_open_job_even_while_it_is_a_draft() {
    let fx = Fx::new();
    drafted(&fx);
    lock(&fx.env, "pl");
    fixture(
        &fx.env,
        &format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, fingerprint, created_txn, created_at)
               VALUES ('pl', (SELECT key FROM workflows WHERE plan = 'pl'), 'open-x', 'Open it', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 1, 'open', NULL, cpd_txn(), 't');
             INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, fingerprint, created_txn, created_at)
               VALUES ('pl', (SELECT key FROM workflows WHERE plan = 'pl'), 'update-x', 'Update it', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 1, 'update', '{}', cpd_txn(), 't');",
            "f".repeat(64)
        ),
    );
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    for key in ["open-x", "update-x"] {
        refused(
            &job(&fx, &["edit", key, "--title", "Changed"], "pl"),
            "job_locked",
            3,
        );
        assert_ne!(
            job_row(&fx.env, "pl", key, "title"),
            json!("Changed"),
            "{key}"
        );
    }
    assert_eq!(job_row(&fx.env, "pl", "open-x", "title"), json!("Open it"));
    assert_eq!(
        job_row(&fx.env, "pl", "update-x", "title"),
        json!("Update it")
    );
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );
}

// ---------- job cancel ----------

#[test]
fn cancel_ends_a_draft_queued_or_failed_job_at_once() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    job_add(&fx, "pl", "d", "S2", &[]).ok();
    let out = job(&fx, &["cancel", "d", "--reason", "Not needed."], "pl").ok();
    assert_eq!(
        out,
        json!({"ok": true, "job": "d", "status": "cancelled", "cancel_requested": false})
    );
    assert_eq!(job_row(&fx.env, "pl", "d", "status"), "cancelled");
    assert_eq!(job_row(&fx.env, "pl", "d", "cancel_reason"), "Not needed.");
    assert!(job_row(&fx.env, "pl", "d", "cancel_requested_at").is_string());
    assert_eq!(job_row(&fx.env, "pl", "d", "fingerprint"), Value::Null);

    job_add(&fx, "pl", "q", "S2", &[]).ok();
    job_add(&fx, "pl", "f", "S2", &[]).ok();
    job_add(&fx, "pl", "dep", "S2", &["f"]).ok();
    lock(&fx.env, "pl");
    engine(&fx.env, "pl", "f", "failed");
    job(&fx, &["cancel", "q", "--reason", "Queued no more."], "pl").ok();
    let out = job(&fx, &["cancel", "f", "--reason", "Gave up."], "pl").ok();
    assert_eq!(out["status"], "cancelled");
    assert_eq!(job_row(&fx.env, "pl", "f", "failed_cause"), Value::Null);
    // Its queued dependent fails in the same transaction (contract §3.2 "Cancel", hs-d647).
    assert_eq!(job_row(&fx.env, "pl", "dep", "status"), "failed");
    assert_eq!(
        job_row(&fx.env, "pl", "dep", "failed_cause"),
        "its need f was cancelled"
    );
    let failed = events(&fx, "job-failed");
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0]["job"], "dep");

    let cancelled = events(&fx, "job-cancelled");
    assert_eq!(cancelled.len(), 3);
    assert_eq!(cancelled[0]["data"], json!({"reason": "Not needed."}));
    assert_eq!(cancelled[0]["job"], "d");
    assert_eq!(cancelled[0]["attempt"], Value::Null);
}

#[test]
fn cancel_fails_its_dependents_directly_and_transitively() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    job_add(&fx, "pl", "a", "S2", &[]).ok();
    job_add(&fx, "pl", "b", "S2", &["a"]).ok();
    job_add(&fx, "pl", "c", "S2", &["b"]).ok();
    lock(&fx.env, "pl");

    let out = job(&fx, &["cancel", "a", "--reason", "Gave up."], "pl").ok();
    assert_eq!(out["status"], "cancelled");
    assert_eq!(out["failed"], json!(["b", "c"]));
    for key in ["b", "c"] {
        assert_eq!(job_row(&fx.env, "pl", key, "status"), "failed", "{key}");
        assert_eq!(
            job_row(&fx.env, "pl", key, "failed_cause"),
            "its need a was cancelled",
            "{key}"
        );
    }
    let failed = events(&fx, "job-failed");
    assert_eq!(failed.len(), 2);
    for (i, key) in ["b", "c"].iter().enumerate() {
        assert_eq!(failed[i]["job"], *key);
    }

    // Idempotent: a second cancel writes nothing and a re-run changes no Job.
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    let out = job(&fx, &["cancel", "a", "--reason", "Again."], "pl").ok();
    assert_eq!(out["already"], true);
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );
    assert_eq!(job_row(&fx.env, "pl", "b", "status"), "failed");
    assert_eq!(job_row(&fx.env, "pl", "c", "status"), "failed");
}

#[test]
fn cancel_cancels_a_draft_dependent() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    job_add(&fx, "pl", "q", "S2", &[]).ok();
    lock(&fx.env, "pl");
    // A stub draft appended after the lock, needing the queued Job `q` (a complete one would be a parked draft).
    let run = fx.cpd(&[
        "job", "add", "dd", "--title", "T", "--needs", "q", "--draft", "--plan", "pl",
    ]);
    run.ok();
    assert_eq!(job_row(&fx.env, "pl", "dd", "status"), "draft");

    let out = job(&fx, &["cancel", "q", "--reason", "Not needed."], "pl").ok();
    assert_eq!(out["status"], "cancelled");
    assert_eq!(out["cancelled"], json!(["dd"]));
    assert_eq!(job_row(&fx.env, "pl", "dd", "status"), "cancelled");
    assert_eq!(
        job_row(&fx.env, "pl", "dd", "cancel_reason"),
        "its need q was cancelled"
    );
    let cancelled = events(&fx, "job-cancelled");
    assert!(
        cancelled
            .iter()
            .any(|e| e["job"] == "dd" && e["data"]["reason"] == "its need q was cancelled"),
        "one job-cancelled Event for dd: {cancelled:?}"
    );
}

#[test]
fn retry_refuses_a_job_whose_need_was_cancelled() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    job_add(&fx, "pl", "a", "S2", &[]).ok();
    job_add(&fx, "pl", "b", "S2", &["a"]).ok();
    lock(&fx.env, "pl");
    // Cancelling `a` leaves its queued dependent `b` failed (contract §3.2 "Cancel").
    job(&fx, &["cancel", "a", "--reason", "Gave up."], "pl").ok();
    assert_eq!(job_row(&fx.env, "pl", "b", "status"), "failed");

    let e = refused(
        &job(&fx, &["retry", "b", "--tier", "S2"], "pl"),
        "needs_unavailable",
        3,
    );
    assert_eq!(
        e["details"]["needs"],
        json!([{"key": "a", "status": "cancelled"}])
    );
    assert_eq!(job_row(&fx.env, "pl", "b", "status"), "failed");

    // Control: a `failed` need is legal, so its failed dependent retries (contract §5.4).
    drop(fx); // one `Fx` lives at a time in a process
    let ctl = Fx::new();
    setup(&ctl.env);
    plan(&ctl.env, "pl", "plan");
    job_add(&ctl, "pl", "a", "S2", &[]).ok();
    job_add(&ctl, "pl", "b", "S2", &["a"]).ok();
    lock(&ctl.env, "pl");
    engine(&ctl.env, "pl", "a", "failed");
    engine(&ctl.env, "pl", "b", "failed");
    let out = job(&ctl, &["retry", "b", "--tier", "S2"], "pl").ok();
    assert_eq!(out["status"], "queued");
    assert_eq!(job_row(&ctl.env, "pl", "b", "status"), "queued");
}

#[test]
fn cancel_of_a_running_job_only_records_the_request() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    job_add(&fx, "pl", "w", "S2", &[]).ok();
    lock(&fx.env, "pl");
    engine(&fx.env, "pl", "w", "running");

    let out = job(&fx, &["cancel", "w", "--reason", "Wrong approach."], "pl").ok();
    assert_eq!(
        out,
        json!({"ok": true, "job": "w", "status": "running", "cancel_requested": true})
    );
    assert_eq!(job_row(&fx.env, "pl", "w", "status"), "running");
    assert_eq!(
        job_row(&fx.env, "pl", "w", "cancel_reason"),
        "Wrong approach."
    );
    assert!(job_row(&fx.env, "pl", "w", "cancel_requested_at").is_string());
    assert_eq!(
        fx.env
            .query::<Option<String>>("SELECT stop_request FROM attempts WHERE id = 'w-a1'"),
        None,
        "E2 sets the stop request, not the command"
    );
    assert_eq!(events(&fx, "job-cancelled").len(), 0);
    let requested = events(&fx, "job-cancel-requested");
    assert_eq!(requested.len(), 1);
    assert_eq!(requested[0]["data"], json!({"reason": "Wrong approach."}));
    assert_eq!(requested[0]["attempt"], "w-a1");

    // A second request keeps the first reason.
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    let out = job(&fx, &["cancel", "w", "--reason", "Another reason."], "pl").ok();
    assert_eq!(
        out,
        json!({"ok": true, "job": "w", "status": "running", "cancel_requested": true, "already": true})
    );
    assert_eq!(
        job_row(&fx.env, "pl", "w", "cancel_reason"),
        "Wrong approach."
    );
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );
}

#[test]
fn cancel_again_is_already_and_a_merged_job_is_refused() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    job_add(&fx, "pl", "d", "S2", &[]).ok();
    job_add(&fx, "pl", "m", "S2", &[]).ok();
    job(&fx, &["cancel", "d", "--reason", "First."], "pl").ok();
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    let out = job(&fx, &["cancel", "d", "--reason", "Second."], "pl").ok();
    assert_eq!(
        out,
        json!({"ok": true, "job": "d", "status": "cancelled", "cancel_requested": false, "already": true})
    );
    assert_eq!(job_row(&fx.env, "pl", "d", "cancel_reason"), "First.");
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );

    lock(&fx.env, "pl");
    engine(&fx.env, "pl", "m", "merged");
    let e = refused(
        &job(&fx, &["cancel", "m", "--reason", "Too late."], "pl"),
        "job_merged",
        3,
    );
    assert_eq!(e["details"]["job"], "m");
    refused(
        &job(&fx, &["cancel", "ghost", "--reason", "x"], "pl"),
        "job_not_found",
        4,
    );
    refused(
        &job(&fx, &["cancel", "m", "--reason", "x"], "nope"),
        "plan_not_found",
        4,
    );
    refused(&job(&fx, &["cancel", "m"], "pl"), "usage", 2);
    assert_eq!(job_row(&fx.env, "pl", "m", "status"), "merged");
}

// ---------- job retry ----------

#[test]
fn retry_queues_a_failed_job_at_the_given_tier() {
    let fx = Fx::new();
    drafted(&fx);
    lock(&fx.env, "pl");
    engine(&fx.env, "pl", "a", "failed");
    let out = job(&fx, &["retry", "a", "--tier", "S3"], "pl").ok();
    assert_eq!(
        out,
        json!({"ok": true, "job": "a", "status": "queued", "tier": "S3"})
    );
    assert_eq!(job_row(&fx.env, "pl", "a", "status"), "queued");
    assert_eq!(job_row(&fx.env, "pl", "a", "retry_tier"), "S3");
    assert_eq!(job_row(&fx.env, "pl", "a", "failed_cause"), Value::Null);
    assert_eq!(
        job_row(&fx.env, "pl", "a", "tier"),
        "S2",
        "the Job's own Tier is Locked"
    );
    let retried = events(&fx, "job-retried");
    assert_eq!(retried.len(), 1);
    assert_eq!(retried[0]["data"], json!({"tier": "S3"}));
    assert_eq!(retried[0]["job"], "a");

    // Again with the same Tier and no Attempt since: already, nothing written.
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    let out = job(&fx, &["retry", "a", "--tier", "S3"], "pl").ok();
    assert_eq!(out["already"], true);
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );
    // Another Tier is not the same retry.
    refused(
        &job(&fx, &["retry", "a", "--tier", "S1"], "pl"),
        "job_not_failed",
        3,
    );
    // Once an Attempt has started since, the retry is spent: the Job is only queued again by the ladder.
    fixture(
        &fx.env,
        "INSERT INTO events (txn, at, kind, context, plan, job, attempt, text, data)
         SELECT id, at, 'attempt-started', 'ctx', 'pl', 'a', 'a-a2', 'Started.', '{}' FROM txns WHERE id = cpd_txn();",
    );
    refused(
        &job(&fx, &["retry", "a", "--tier", "S3"], "pl"),
        "job_not_failed",
        3,
    );
}

#[test]
fn retry_refusals_write_nothing() {
    let fx = Fx::new();
    drafted(&fx);
    job_add(&fx, "pl", "c", "S2", &[]).ok();
    lock(&fx.env, "pl");
    engine(&fx.env, "pl", "a", "running");
    job(&fx, &["cancel", "c", "--reason", "No."], "pl").ok();
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));

    let e = refused(
        &job(&fx, &["retry", "b", "--tier", "S2"], "pl"),
        "job_not_failed",
        3,
    );
    assert_eq!(e["details"]["status"], "queued");
    let e = refused(
        &job(&fx, &["retry", "a", "--tier", "S2"], "pl"),
        "job_not_failed",
        3,
    );
    assert_eq!(e["details"]["status"], "running");
    refused(
        &job(&fx, &["retry", "c", "--tier", "S2"], "pl"),
        "job_not_failed",
        3,
    );
    refused(
        &job(&fx, &["retry", "ghost", "--tier", "S2"], "pl"),
        "job_not_found",
        4,
    );
    refused(
        &job(&fx, &["retry", "b", "--tier", "S2"], "nope"),
        "plan_not_found",
        4,
    );
    let e = refused(
        &job(&fx, &["retry", "a", "--tier", "H4"], "pl"),
        "tier_invalid",
        2,
    );
    assert_eq!(e["details"]["efforts"], json!([0, 1, 2, 3]));
    refused(
        &job(&fx, &["retry", "a", "--tier", "Z1"], "pl"),
        "tier_invalid",
        2,
    );
    refused(&job(&fx, &["retry", "a"], "pl"), "usage", 2);
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );
}

#[test]
fn retry_refuses_a_job_whose_fingerprint_does_not_match() {
    let fx = Fx::new();
    drafted(&fx);
    lock(&fx.env, "pl");
    engine(&fx.env, "pl", "a", "failed");
    // Written around the trigger, the way a tamper would be; the schema is whole again afterwards.
    let trigger: String = fx
        .env
        .query("SELECT sql FROM sqlite_schema WHERE name = 'jobs_locked'");
    fixture(
        &fx.env,
        &format!(
            "DROP TRIGGER jobs_locked; UPDATE jobs SET goal = 'Changed.' WHERE key = 'a'; {trigger};"
        ),
    );
    let e = refused(
        &job(&fx, &["retry", "a", "--tier", "S3"], "pl"),
        "locked_job_tampered",
        3,
    );
    assert_eq!(e["details"]["job"], "a");
    assert_eq!(job_row(&fx.env, "pl", "a", "status"), "failed");
    let shown = job(&fx, &["show", "a"], "pl").ok();
    assert_eq!(shown["tamper"][0]["kind"], "fingerprint");
    assert_eq!(shown["tamper"][0]["job"], "a");
    // Cancelling is the way out of a tampered Job.
    job(&fx, &["cancel", "a", "--reason", "Tampered."], "pl").ok();
}

// ---------- job list ----------

#[test]
fn list_shows_every_job_in_creation_order_and_filters_by_status() {
    let fx = Fx::new();
    drafted(&fx);
    job_add(&fx, "pl", "c", "S3", &[]).ok();
    lock(&fx.env, "pl");
    engine(&fx.env, "pl", "a", "merged");
    engine(&fx.env, "pl", "c", "failed");

    let out = job(&fx, &["list"], "pl").ok();
    let jobs = out["jobs"].as_array().unwrap();
    let keys: Vec<&str> = jobs.iter().map(|j| j["key"].as_str().unwrap()).collect();
    assert_eq!(keys, ["a", "b", "c"]);
    assert_eq!(
        jobs[0],
        json!({"key": "a", "title": "Title of a", "kind": "work", "status": "merged", "tier": "S2", "appended": false, "needs": [], "attempts": 1, "merged_sha": "c0ffee", "urgent": false})
    );
    assert_eq!(
        jobs[1],
        json!({"key": "b", "title": "Title of b", "kind": "work", "status": "queued", "tier": "S2", "appended": false, "needs": ["a"], "attempts": 0, "merged_sha": null, "urgent": false})
    );
    assert_eq!(jobs[2]["attempts"], 1);

    let out = job(&fx, &["list", "--status", "queued"], "pl").ok();
    assert_eq!(out["jobs"].as_array().unwrap().len(), 1);
    let out = job(
        &fx,
        &["list", "--status", "queued", "--status", "failed"],
        "pl",
    )
    .ok();
    let keys: Vec<&str> = out["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["key"].as_str().unwrap())
        .collect();
    assert_eq!(keys, ["b", "c"]);
    assert_eq!(
        job(&fx, &["list", "--status", "draft"], "pl").ok()["jobs"],
        json!([])
    );
    refused(&job(&fx, &["list", "--status", "done"], "pl"), "usage", 2);
    refused(&job(&fx, &["list"], "nope"), "plan_not_found", 4);
    refused(&fx.cpd(&["job", "list"]), "plan_required", 2);
}

#[test]
fn list_and_show_write_nothing() {
    let fx = Fx::new();
    drafted(&fx);
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    job(&fx, &["list"], "pl").ok();
    job(&fx, &["show", "a"], "pl").ok();
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );
}

// ---------- job show ----------

#[test]
fn show_has_every_field_the_needs_and_the_attempts_with_their_escalations() {
    let fx = Fx::new();
    drafted(&fx);
    lock(&fx.env, "pl");
    fixture(
        &fx.env,
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
    let out = job(&fx, &["show", "a"], "pl").ok();
    let job_json = &out["job"];
    assert_eq!(job_json["key"], "a");
    assert_eq!(job_json["status"], "failed");
    assert_eq!(job_json["failed_cause"], "timed_out");
    assert_eq!(job_json["tier"], "S2");
    assert_eq!(job_json["goal"], "Do it.");
    assert_eq!(job_json["touches"], json!(["src/a.rs"]));
    assert_eq!(job_json["fingerprint"].as_str().unwrap().len(), 64);
    for field in [
        "seq",
        "title",
        "context",
        "acceptance",
        "out_of_scope",
        "appended",
        "retry_tier",
        "cancel_reason",
        "cancel_requested_at",
        "merged_sha",
        "created_at",
    ] {
        assert!(job_json.get(field).is_some(), "{field}");
    }
    assert_eq!(out["needs"], json!([]));
    assert_eq!(out["needed_by"], json!(["b"]));
    assert_eq!(out["tamper"], json!([]));

    let attempts = out["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 1);
    let attempt = &attempts[0];
    assert_eq!(attempt["id"], "a-a1");
    assert_eq!(
        (
            attempt["status"].as_str(),
            attempt["outcome"].as_str(),
            attempt["cause"].as_str()
        ),
        (Some("ended"), Some("failed"), Some("timed_out"))
    );
    assert_eq!(attempt["check_tail"], "tail");
    assert_eq!(attempt["metrics"], json!({"turns": 3}));
    for field in [
        "number",
        "tier",
        "origin",
        "branch",
        "worktree",
        "base_sha",
        "driver_pid",
        "worker_session",
        "spec_path",
        "submitted_sha",
        "submitted_at",
        "merged_sha",
        "gate_hook",
        "gate_command",
        "gate_exit",
        "nudged",
        "stop_request",
        "started_at",
        "ended_at",
        "actual_files",
    ] {
        assert!(attempt.get(field).is_some(), "{field}");
    }
    let escalations = attempt["escalations"].as_array().unwrap();
    assert_eq!(escalations.len(), 2);
    assert_eq!(
        (
            escalations[0]["id"].as_str(),
            escalations[0]["status"].as_str(),
            escalations[0]["answer"].as_str()
        ),
        (Some("a-a1-q1"), Some("answered"), Some("That."))
    );
    assert_eq!(escalations[1]["status"], "open");

    let out = job(&fx, &["show", "b"], "pl").ok();
    assert_eq!(out["needs"], json!(["a"]));
    assert_eq!(out["needed_by"], json!([]));
    assert_eq!(out["attempts"], json!([]));
    refused(&job(&fx, &["show", "ghost"], "pl"), "job_not_found", 4);
    refused(&job(&fx, &["show", "b"], "nope"), "plan_not_found", 4);
    refused(&fx.cpd(&["job", "show", "b"]), "plan_required", 2);
    refused(
        &fx.cpd(&["job", "show", "B", "--plan", "pl"]),
        "invalid_job_id",
        2,
    );
}

#[test]
fn show_prints_the_same_actual_files_as_attempt_show() {
    let fx = Fx::new();
    drafted(&fx);
    lock(&fx.env, "pl");
    fixture(
        &fx.env,
        "UPDATE jobs SET status = 'running' WHERE key = 'a';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, actual_files, started_at)
         VALUES ('pl', 'a-a1', 'a', 1, 'S2', 'ladder', 'running', 'cpd/pl--a-a1', '/w', '[\"a.rs\",\"docs/x.md\"]', 't');",
    );
    let out = job(&fx, &["show", "a"], "pl").ok();
    let printed = out["attempts"][0]["actual_files"].clone();
    assert_eq!(printed, json!(["a.rs", "docs/x.md"]));
    let shown = fx.cpd(&["attempt", "show", "a-a1", "--plan", "pl"]).ok();
    assert_eq!(shown["attempt"]["actual_files"], printed);
}

// ---------- --pr (R34, commands.md §1.3, §5.1, §5.2) ----------

/// Adds PR `key` to Plan `plan_id` in `status`, walking the statuses the way the engine would.
fn add_pr(env: &TestEnv, plan_id: &str, key: &str, status: &str) {
    const STEPS: [(&str, &str); 3] = [
        ("dispatch", ", lead_worktree = '/wt', started_at = 't'"),
        (
            "open",
            ", pr_number = 1, pr_url = 'https://github.com/acme/{repo}/pull/1', pr_opened_at = 't', pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/{repo}')",
        ),
        ("merged", ", merged_at = 't'"),
    ];
    let repo = format!("{plan_id}-{key}");
    let mut sql = format!(
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
         VALUES ('{plan_id}', '{key}', '/{key}', 'main', 'waiting', 'cpd/{plan_id}/{key}', cpd_txn(), 't');"
    );
    match status {
        "waiting" => {}
        "cancelled" => {
            sql += &format!(
                "UPDATE workflows SET status = 'cancelled' WHERE plan = '{plan_id}' AND key = '{key}';"
            )
        }
        _ => {
            let last = STEPS
                .iter()
                .position(|(s, _)| *s == status)
                .unwrap_or_else(|| panic!("{status}"));
            for (step, set) in &STEPS[..=last] {
                if *step == "open" {
                    sql += &pull_request(&repo, 1, "open");
                }
                sql += &format!(
                    "UPDATE workflows SET status = '{step}'{} WHERE plan = '{plan_id}' AND key = '{key}';",
                    set.replace("{repo}", &repo)
                );
                if *step == "merged" {
                    sql += &format!(
                        "UPDATE pull_requests SET state = 'merged', merged_at = 't' WHERE repo = 'acme/{repo}';"
                    );
                }
            }
        }
    }
    fixture(env, &sql);
}

fn job_add_in(fx: &Fx, plan_id: &str, key: &str, pr: Option<&str>, needs: &[&str]) -> Run {
    let spec = spec_file(&fx.env);
    let title = format!("Title of {key}");
    let needs = needs.join(",");
    let mut args = vec![
        "job", "add", key, "--title", &title, "--tier", "S2", "--spec", &spec, "--plan", plan_id,
    ];
    if let Some(pr) = pr {
        args.extend(["--workflow", pr]);
    }
    if !needs.is_empty() {
        args.extend(["--needs", &needs]);
    }
    fx.cpd(&args)
}

/// Plan `pl` in phase `plan` with PRs `repo` (the fixture's) and `web`, and a draft Job `a` in `repo`.
fn two_prs(fx: &Fx) {
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    add_pr(&fx.env, "pl", "web", "waiting");
    job_add_in(fx, "pl", "a", Some("repo"), &[]).ok();
}

#[test]
fn add_defaults_to_the_one_pr_and_names_the_pr_it_was_given() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    // One PR: `--pr` may be omitted or given.
    assert_eq!(
        job_add_in(&fx, "pl", "a", None, &[]).ok()["job"]["pr"],
        "repo"
    );
    assert_eq!(
        job_add_in(&fx, "pl", "b", Some("repo"), &["a"]).ok()["job"]["pr"],
        "repo"
    );
    // A cancelled PR does not count: the one that is not cancelled is the default.
    add_pr(&fx.env, "pl", "old", "cancelled");
    assert_eq!(
        job_add_in(&fx, "pl", "c", None, &[]).ok()["job"]["pr"],
        "repo"
    );

    add_pr(&fx.env, "pl", "web", "waiting");
    let out = job_add_in(&fx, "pl", "w", Some("web"), &[]).ok();
    assert_eq!(out["job"]["pr"], "web");
    assert_eq!(job_row(&fx.env, "pl", "w", "workflow"), "web");
    assert_eq!(job_row(&fx.env, "pl", "a", "workflow"), "repo");
    assert_eq!(
        events(&fx, "job-added").last().unwrap()["data"]["workflow"],
        "web"
    );

    // Rerun: the same PR is `already`, another PR is `job_key_exists`.
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    assert_eq!(
        job_add_in(&fx, "pl", "w", Some("web"), &[]).ok()["already"],
        true
    );
    refused(
        &job_add_in(&fx, "pl", "w", Some("repo"), &[]),
        "job_key_exists",
        3,
    );
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );
}

#[test]
fn add_refuses_a_missing_unknown_or_ended_pr_and_writes_nothing() {
    let fx = Fx::new();
    two_prs(&fx);
    add_pr(&fx.env, "pl", "gone", "cancelled");
    let (txns, evs, rows) = (
        count(&fx.env, "txns"),
        count(&fx.env, "events"),
        count(&fx.env, "jobs"),
    );

    // Two PRs that are not cancelled: `--pr` is required.
    let e = refused(
        &job_add_in(&fx, "pl", "k", None, &[]),
        "workflow_required",
        2,
    );
    assert_eq!(e["details"]["workflows"], json!(["repo", "web"]));
    let e = refused(
        &job_add_in(&fx, "pl", "k", Some("nope"), &[]),
        "workflow_not_found",
        4,
    );
    assert_eq!(e["details"]["pr"], "nope");
    let e = refused(
        &job_add_in(&fx, "pl", "k", Some("gone"), &[]),
        "workflow_ended",
        3,
    );
    assert_eq!(e["details"]["status"], "cancelled");
    refused(
        &job_add_in(&fx, "pl", "k", Some("Web"), &[]),
        "invalid_workflow_id",
        2,
    );
    assert_eq!(
        (
            count(&fx.env, "txns"),
            count(&fx.env, "events"),
            count(&fx.env, "jobs")
        ),
        (txns, evs, rows)
    );

    // No PR that is not cancelled: `pr_required` too, and its message names `cpd pr add`.
    fixture(&fx.env, "UPDATE jobs SET status = 'cancelled', cancel_reason = 'x', cancel_requested_at = 't' WHERE plan = 'pl';
                   UPDATE workflows SET status = 'cancelled' WHERE plan = 'pl';");
    let e = refused(
        &job_add_in(&fx, "pl", "k", None, &[]),
        "workflow_required",
        2,
    );
    assert!(e["message"].as_str().unwrap().contains("cpd workflow add"), "{e}");
}

#[test]
fn add_into_a_merged_pr_is_refused_and_into_a_waiting_pr_is_appended() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    add_pr(&fx.env, "pl", "web", "waiting");
    job_add_in(&fx, "pl", "a", Some("repo"), &[]).ok();
    lock(&fx.env, "pl");
    // `lock` starts every PR; put `web` back as the engine would find a PR waiting on `repo`.
    fixture(
        &fx.env,
        &format!(
            "{} {}
             UPDATE workflows SET status = 'open', pr_number = 1, pr_url = 'https://github.com/acme/repo/pull/1', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/repo') WHERE plan = 'pl' AND key = 'repo';
             UPDATE workflows SET status = 'open', pr_number = 2, pr_url = 'https://github.com/acme/web/pull/2', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/web') WHERE plan = 'pl' AND key = 'web';
             UPDATE workflows SET status = 'merged', merged_at = 't' WHERE plan = 'pl' AND key = 'repo';",
            pull_request("repo", 1, "merged"),
            pull_request("web", 2, "open")
        ),
    );
    let e = refused(
        &job_add_in(&fx, "pl", "k", Some("repo"), &[]),
        "workflow_ended",
        3,
    );
    assert_eq!(e["details"]["status"], "merged");
    let out = job_add_in(&fx, "pl", "k", Some("web"), &[]).ok();
    println!("OUT {out}");
    assert_eq!(out["job"]["pr"], "web");
    assert_eq!(out["job"]["status"], "queued");
    assert_eq!(out["job"]["appended"], true);
}

#[test]
fn add_needs_must_be_jobs_of_the_same_pr() {
    let fx = Fx::new();
    two_prs(&fx);
    job_add_in(&fx, "pl", "w", Some("web"), &[]).ok();
    job_add_in(&fx, "pl", "a2", Some("repo"), &["a"]).ok();
    let (txns, evs, rows) = (
        count(&fx.env, "txns"),
        count(&fx.env, "events"),
        count(&fx.env, "jobs"),
    );

    let e = refused(
        &job_add_in(&fx, "pl", "k", Some("web"), &["w", "a"]),
        "needs_other_workflow",
        3,
    );
    assert_eq!(e["details"]["jobs"], json!(["a"]));
    let e = refused(
        &job_add_in(&fx, "pl", "k", Some("repo"), &["w", "a", "a2"]),
        "needs_other_workflow",
        3,
    );
    assert_eq!(e["details"]["jobs"], json!(["w"]));
    // An unknown need is reported first.
    refused(
        &job_add_in(&fx, "pl", "k", Some("web"), &["a", "ghost"]),
        "needs_unknown",
        3,
    );
    assert_eq!(
        (
            count(&fx.env, "txns"),
            count(&fx.env, "events"),
            count(&fx.env, "jobs")
        ),
        (txns, evs, rows)
    );
    job_add_in(&fx, "pl", "w2", Some("web"), &["w"]).ok();
}

#[test]
fn edit_moves_a_draft_job_to_another_pr() {
    let fx = Fx::new();
    two_prs(&fx);
    job_add_in(&fx, "pl", "w", Some("web"), &[]).ok();
    job_add_in(&fx, "pl", "b", Some("repo"), &[]).ok();

    let out = job(
        &fx,
        &["edit", "b", "--workflow", "web", "--needs", "w"],
        "pl",
    )
    .ok();
    assert_eq!(out["fields"], json!(["workflow", "needs"]));
    assert_eq!(job_row(&fx.env, "pl", "b", "workflow"), "web");
    assert_eq!(
        fx.env
            .query::<String>("SELECT needs FROM dependencies WHERE job = 'b'"),
        "w"
    );
    assert_eq!(
        events(&fx, "job-edited").last().unwrap()["data"],
        json!({"fields": ["workflow", "needs"]})
    );

    // A Job with old needs may move with `--no-needs`.
    let out = job(
        &fx,
        &["edit", "b", "--workflow", "repo", "--no-needs"],
        "pl",
    )
    .ok();
    assert_eq!(out["fields"], json!(["workflow", "needs"]));
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM dependencies"), 0);
    // A Job with no needs and no dependents moves alone.
    let out = job(&fx, &["edit", "b", "--workflow", "web"], "pl").ok();
    assert_eq!(out["fields"], json!(["workflow"]));
    assert_eq!(job_row(&fx.env, "pl", "b", "workflow"), "web");

    // The PR it is already in changes nothing, whatever its needs.
    job(&fx, &["edit", "b", "--needs", "w"], "pl").ok();
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    let out = job(&fx, &["edit", "b", "--workflow", "web"], "pl").ok();
    assert_eq!(out["already"], true);
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );
}

#[test]
fn edit_pr_refusals_write_nothing() {
    let fx = Fx::new();
    two_prs(&fx);
    add_pr(&fx.env, "pl", "gone", "cancelled");
    job_add_in(&fx, "pl", "w", Some("web"), &[]).ok();
    job_add_in(&fx, "pl", "b", Some("repo"), &["a"]).ok();
    job_add_in(&fx, "pl", "c", Some("repo"), &[]).ok();
    job(&fx, &["cancel", "c", "--reason", "No."], "pl").ok();
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));

    let e = refused(
        &job(&fx, &["edit", "a", "--workflow", "nope"], "pl"),
        "workflow_not_found",
        4,
    );
    assert_eq!(e["details"]["pr"], "nope");
    let e = refused(
        &job(&fx, &["edit", "a", "--workflow", "gone"], "pl"),
        "workflow_ended",
        3,
    );
    assert_eq!(e["details"]["status"], "cancelled");
    refused(
        &job(&fx, &["edit", "a", "--workflow", "Web"], "pl"),
        "invalid_workflow_id",
        2,
    );
    refused(
        &job(
            &fx,
            &["edit", "a", "--workflow", "web", "--needs", "ghost"],
            "pl",
        ),
        "needs_unknown",
        3,
    );
    refused(
        &job(
            &fx,
            &["edit", "a", "--workflow", "web", "--needs", "c"],
            "pl",
        ),
        "needs_unavailable",
        3,
    );
    // The Job keeps no old needs: `b` needs `a`.
    let e = refused(
        &job(&fx, &["edit", "b", "--workflow", "web"], "pl"),
        "needs_other_workflow",
        3,
    );
    assert_eq!(e["details"]["jobs"], json!(["a"]));
    // No Job may need it: `b` needs `a`.
    let e = refused(
        &job(&fx, &["edit", "a", "--workflow", "web"], "pl"),
        "needs_other_workflow",
        3,
    );
    assert_eq!(e["details"]["jobs"], json!(["b"]));
    let e = refused(
        &job(&fx, &["edit", "a", "--workflow", "web", "--no-needs"], "pl"),
        "needs_other_workflow",
        3,
    );
    assert_eq!(e["details"]["jobs"], json!(["b"]));
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );
    // The new needs are Jobs of the new PR.
    job_add_in(&fx, "pl", "w2", Some("web"), &[]).ok();
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    let e = refused(
        &job(
            &fx,
            &[
                "edit",
                "w",
                "--workflow",
                "repo",
                "--needs",
                "w2",
                "--tier",
                "S3",
            ],
            "pl",
        ),
        "needs_other_workflow",
        3,
    );
    assert_eq!(e["details"]["jobs"], json!(["w2"]));
    // Without `--pr` the needs are those of the Job's own PR.
    let e = refused(
        &job(&fx, &["edit", "w", "--needs", "a"], "pl"),
        "needs_other_workflow",
        3,
    );
    assert_eq!(e["details"]["jobs"], json!(["a"]));
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );
    assert_eq!(job_row(&fx.env, "pl", "a", "workflow"), "repo");
}

#[test]
fn a_locked_job_cannot_change_pr() {
    let fx = Fx::new();
    two_prs(&fx);
    lock(&fx.env, "pl");
    refused(
        &job(&fx, &["edit", "a", "--workflow", "web"], "pl"),
        "job_locked",
        3,
    );
    assert_eq!(job_row(&fx.env, "pl", "a", "workflow"), "repo");
}

// ---------- drafts in a running Plan (hs-d173, hs-d174, hs-d179; commands.md §5.1–§5.3) ----------

/// `job add <key> --draft` in Plan `plan_id`, with `needs` and the `extra` flags.
fn job_add_draft(fx: &Fx, plan_id: &str, key: &str, needs: &[&str], extra: &[&str]) -> Run {
    let spec = spec_file(&fx.env);
    let title = format!("Title of {key}");
    let needs = needs.join(",");
    let mut args = vec![
        "job", "add", key, "--title", &title, "--tier", "S2", "--spec", &spec, "--draft", "--plan",
        plan_id,
    ];
    if !needs.is_empty() {
        args.extend(["--needs", &needs]);
    }
    args.extend(extra);
    fx.cpd(&args)
}

#[test]
fn add_draft_after_the_lock_is_an_appended_draft_that_stays_editable_and_cancellable() {
    for phase in ["dispatch", "deploy"] {
        let fx = Fx::new();
        setup(&fx.env);
        plan(&fx.env, "pl", phase);
        job_add(&fx, "pl", "a", "S2", &[]).ok();
        // `a` is merged: a complete draft that needs it does not wait, so the door accepts `d`.
        engine(&fx.env, "pl", "a", "merged");

        let out = job_add_draft(&fx, "pl", "d", &["a"], &[]).ok();
        assert_eq!(
            out["job"],
            json!({"key": "d", "pr": "repo", "kind": "work", "status": "draft", "appended": true, "tier": "S2", "needs": ["a"], "urgent": false}),
            "{phase}"
        );
        assert_eq!(
            (
                job_row(&fx.env, "pl", "d", "status"),
                job_row(&fx.env, "pl", "d", "appended")
            ),
            (json!("draft"), json!(1))
        );
        assert_eq!(job_row(&fx.env, "pl", "d", "fingerprint"), Value::Null);
        assert_eq!(
            events(&fx, "job-added").last().unwrap()["data"],
            json!({"workflow": "repo", "kind": "work", "status": "draft", "appended": true, "tier": "S2", "needs": ["a"], "urgent": false})
        );
        // Its needs may be drafts; an identical rerun is `already`, as for any add.
        job_add_draft(&fx, "pl", "e", &["d"], &[]).ok();
        let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
        assert_eq!(
            job_add_draft(&fx, "pl", "e", &["d"], &[]).ok()["already"],
            true
        );
        assert_eq!(
            (count(&fx.env, "txns"), count(&fx.env, "events")),
            (txns, evs)
        );

        // Editable and cancellable in this phase.
        let out = job(
            &fx,
            &["edit", "d", "--title", "New title", "--tier", "S3"],
            "pl",
        )
        .ok();
        assert_eq!(
            (&out["job"]["status"], &out["fields"]),
            (&json!("draft"), &json!(["title", "tier"])),
            "{phase}"
        );
        job(&fx, &["edit", "e", "--needs", "a,d"], "pl").ok();
        assert_eq!(job_row(&fx.env, "pl", "d", "fingerprint"), Value::Null);
        let out = job(&fx, &["cancel", "e", "--reason", "Not this batch."], "pl").ok();
        assert_eq!(
            (&out["status"], &out["cancel_requested"]),
            (&json!("cancelled"), &json!(false))
        );
        assert_eq!(job_row(&fx.env, "pl", "e", "fingerprint"), Value::Null);
    }

    // In phase plan every Job is a draft: --draft changes nothing.
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let out = job_add_draft(&fx, "pl", "a", &[], &[]).ok();
    assert_eq!(
        (&out["job"]["status"], &out["job"]["appended"]),
        (&json!("draft"), &json!(false))
    );
    assert_eq!(job_row(&fx.env, "pl", "a", "appended"), 0);

    // In phase done --draft passes the phase, and finds every PR ended until `pr add` reopens the Plan;
    // without --draft the phase refuses.
    plan(&fx.env, "dn", "done");
    let e = refused(
        &job_add_draft(&fx, "dn", "k", &[], &[]),
        "workflow_ended",
        3,
    );
    assert_eq!(e["details"]["status"], "merged");
    refused(&job_add(&fx, "dn", "k", "S2", &[]), "illegal_phase", 3);
}

#[test]
fn a_queued_job_may_not_need_a_draft() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "dispatch");
    job_add(&fx, "pl", "a", "S2", &[]).ok();
    job_add_draft(&fx, "pl", "d", &[], &[]).ok();
    let (txns, evs, rows) = (
        count(&fx.env, "txns"),
        count(&fx.env, "events"),
        count(&fx.env, "jobs"),
    );

    let e = refused(
        &job_add(&fx, "pl", "q", "S2", &["a", "d"]),
        "needs_unavailable",
        3,
    );
    assert_eq!(e["details"]["jobs"], json!(["d"]));
    assert!(e["message"].as_str().unwrap().contains("--draft"), "{e}");
    assert_eq!(
        (
            count(&fx.env, "txns"),
            count(&fx.env, "events"),
            count(&fx.env, "jobs")
        ),
        (txns, evs, rows)
    );

    // A stub draft may need both the queued `a` and the draft `d`: only its incompleteness keeps it legal.
    assert_eq!(
        stub_add(&fx, "pl", "q", &["--needs", "a,d"]).ok()["job"]["status"],
        "draft"
    );
}

// ---------- stub drafts (hs-d241, R1; commands.md §5.1, §5.2) ----------

/// `job add <key> --draft --title <t>` with `extra` flags (`--tier`, `--spec`), in Plan `plan_id`.
fn stub_add(fx: &Fx, plan_id: &str, key: &str, extra: &[&str]) -> Run {
    let title = format!("Title of {key}");
    let mut args = vec![
        "job", "add", key, "--draft", "--title", &title, "--plan", plan_id,
    ];
    args.extend(extra);
    fx.cpd(&args)
}

#[test]
fn add_draft_needs_only_a_title_and_stores_the_tier_and_the_spec_as_null() {
    for phase in ["plan", "dispatch"] {
        let fx = Fx::new();
        setup(&fx.env);
        plan(&fx.env, "pl", phase);
        // `a` is queued after the lock; a stub is legal on its own, whether or not it waits (hs-d822).
        job_add(&fx, "pl", "a", "S2", &[]).ok();

        let out = stub_add(&fx, "pl", "k", &["--needs", "a"]).ok();
        let appended = phase != "plan";
        assert_eq!(
            out["job"],
            json!({"key": "k", "pr": "repo", "kind": "work", "status": "draft", "appended": appended, "tier": null, "needs": ["a"], "urgent": false}),
            "{phase}"
        );
        for column in [
            "tier",
            "goal",
            "context",
            "touches",
            "acceptance",
            "out_of_scope",
            "fingerprint",
        ] {
            assert_eq!(
                job_row(&fx.env, "pl", "k", column),
                Value::Null,
                "{phase}: {column}"
            );
        }
        assert_eq!(
            (
                job_row(&fx.env, "pl", "k", "title"),
                job_row(&fx.env, "pl", "k", "status")
            ),
            (json!("Title of k"), json!("draft"))
        );
        assert_eq!(
            events(&fx, "job-added").last().unwrap()["data"],
            json!({"workflow": "repo", "kind": "work", "status": "draft", "appended": appended, "tier": null, "needs": ["a"], "urgent": false})
        );

        // Either half may be given: the Tier alone, the Spec alone; needs work as for any draft.
        let spec = spec_file(&fx.env);
        let out = stub_add(&fx, "pl", "t", &["--tier", "S3", "--needs", "a"]).ok();
        assert_eq!(
            (&out["job"]["tier"], &out["job"]["needs"]),
            (&json!("S3"), &json!(["a"])),
            "{phase}"
        );
        assert_eq!(
            (
                job_row(&fx.env, "pl", "t", "tier"),
                job_row(&fx.env, "pl", "t", "goal")
            ),
            (json!("S3"), Value::Null)
        );
        stub_add(&fx, "pl", "s", &["--spec", &spec, "--needs", "a"]).ok();
        assert_eq!(
            (
                job_row(&fx.env, "pl", "s", "tier"),
                job_row(&fx.env, "pl", "s", "goal")
            ),
            (Value::Null, json!("Do it."))
        );
        assert_eq!(job_row(&fx.env, "pl", "s", "touches"), "[\"src/a.rs\"]");

        // An identical rerun is `already` and writes nothing; the same key with other fields is a clash.
        let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
        assert_eq!(
            stub_add(&fx, "pl", "k", &["--needs", "a"]).ok()["already"],
            true
        );
        assert_eq!(
            (count(&fx.env, "txns"), count(&fx.env, "events")),
            (txns, evs)
        );
        refused(
            &stub_add(&fx, "pl", "k", &["--tier", "S2", "--needs", "a"]),
            "job_key_exists",
            3,
        );

        // The stub is shown as it is, and can be cancelled.
        let shown = job(&fx, &["show", "k"], "pl").ok();
        assert_eq!(
            (
                &shown["job"]["tier"],
                &shown["job"]["goal"],
                &shown["job"]["touches"]
            ),
            (&Value::Null, &Value::Null, &Value::Null),
            "{phase}"
        );
        let listed = job(&fx, &["list"], "pl").ok();
        let stub = listed["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["key"] == "k")
            .unwrap()
            .clone();
        assert_eq!(
            (&stub["tier"], &stub["status"]),
            (&Value::Null, &json!("draft"))
        );
        let out = job(&fx, &["cancel", "k", "--reason", "Not needed."], "pl").ok();
        assert_eq!(
            (&out["status"], &out["cancel_requested"]),
            (&json!("cancelled"), &json!(false))
        );
        assert_eq!(
            (
                job_row(&fx.env, "pl", "k", "status"),
                job_row(&fx.env, "pl", "k", "tier")
            ),
            (json!("cancelled"), Value::Null)
        );
    }
}

#[test]
fn add_without_draft_still_needs_the_tier_and_the_spec() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    job_add(&fx, "pl", "a", "S2", &[]).ok();
    let spec = spec_file(&fx.env);
    let (txns, evs, rows) = (
        count(&fx.env, "txns"),
        count(&fx.env, "events"),
        count(&fx.env, "jobs"),
    );
    for args in [
        vec!["job", "add", "k", "--title", "T", "--plan", "pl"],
        vec![
            "job", "add", "k", "--title", "T", "--tier", "S2", "--plan", "pl",
        ],
        vec![
            "job",
            "add",
            "k",
            "--title",
            "T",
            "--spec",
            spec.as_str(),
            "--plan",
            "pl",
        ],
    ] {
        let e = refused(&fx.cpd(&args), "usage", 2);
        assert!(
            e["message"].as_str().unwrap().contains("--draft"),
            "{args:?}: {e}"
        );
    }
    assert_eq!(
        (
            count(&fx.env, "txns"),
            count(&fx.env, "events"),
            count(&fx.env, "jobs")
        ),
        (txns, evs, rows),
        "nothing is written"
    );
    // After the lock too: a Job that is not a draft needs both.
    lock(&fx.env, "pl");
    let (txns, evs, rows) = (
        count(&fx.env, "txns"),
        count(&fx.env, "events"),
        count(&fx.env, "jobs"),
    );
    refused(
        &fx.cpd(&["job", "add", "k", "--title", "T", "--plan", "pl"]),
        "usage",
        2,
    );
    assert_eq!(
        (
            count(&fx.env, "txns"),
            count(&fx.env, "events"),
            count(&fx.env, "jobs")
        ),
        (txns, evs, rows)
    );
}

#[test]
fn edit_completes_a_stub_draft_with_its_tier_and_its_spec() {
    for phase in ["plan", "dispatch"] {
        let fx = Fx::new();
        setup(&fx.env);
        plan(&fx.env, "pl", phase);
        let spec = spec_file(&fx.env);
        // `a` is queued after the lock; a stub that needs it is legal (hs-d822).
        job_add(&fx, "pl", "a", "S2", &[]).ok();
        stub_add(&fx, "pl", "k", &[]).ok();

        let out = job(&fx, &["edit", "k", "--tier", "S2", "--spec", &spec], "pl").ok();
        assert_eq!(
            (&out["job"], &out["fields"]),
            (
                &json!({"key": "k", "status": "draft"}),
                &json!(["tier", "spec"])
            ),
            "{phase}"
        );
        assert_eq!(job_row(&fx.env, "pl", "k", "tier"), "S2");
        assert_eq!(job_row(&fx.env, "pl", "k", "goal"), "Do it.");
        assert_eq!(job_row(&fx.env, "pl", "k", "context"), "Because.");
        assert_eq!(job_row(&fx.env, "pl", "k", "touches"), "[\"src/a.rs\"]");
        assert_eq!(
            job_row(&fx.env, "pl", "k", "acceptance"),
            "[\"cargo test\"]"
        );
        assert_eq!(
            job_row(&fx.env, "pl", "k", "out_of_scope"),
            "[\"Everything else.\"]"
        );
        assert_eq!(
            events(&fx, "job-edited").last().unwrap()["data"],
            json!({"fields": ["tier", "spec"]}),
            "{phase}"
        );
        assert_eq!(job_row(&fx.env, "pl", "k", "fingerprint"), Value::Null);

        // A rerun changes nothing; the Spec is replaced as a whole, so its fields are listed one by one again.
        let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
        assert_eq!(
            job(&fx, &["edit", "k", "--tier", "S2", "--spec", &spec], "pl").ok()["already"],
            true
        );
        assert_eq!(
            (count(&fx.env, "txns"), count(&fx.env, "events")),
            (txns, evs)
        );
        let other = write_spec(&fx.env, "other.json", &json!({"goal": "Other.", "context": "Because.", "touches": ["src/a.rs"], "acceptance": ["cargo test"], "out_of_scope": []}).to_string());
        assert_eq!(
            job(&fx, &["edit", "k", "--spec", &other], "pl").ok()["fields"],
            json!(["goal", "out_of_scope"]),
            "{phase}"
        );

        // The Tier alone, then the Spec alone, complete a stub in two edits.
        stub_add(&fx, "pl", "two", &[]).ok();
        assert_eq!(
            job(&fx, &["edit", "two", "--tier", "S3"], "pl").ok()["fields"],
            json!(["tier"]),
            "{phase}"
        );
        assert_eq!(
            (
                job_row(&fx.env, "pl", "two", "tier"),
                job_row(&fx.env, "pl", "two", "goal")
            ),
            (json!("S3"), Value::Null)
        );
        assert_eq!(
            job(&fx, &["edit", "two", "--spec", &spec], "pl").ok()["fields"],
            json!(["spec"]),
            "{phase}"
        );
        assert_eq!(
            (
                job_row(&fx.env, "pl", "two", "tier"),
                job_row(&fx.env, "pl", "two", "goal")
            ),
            (json!("S3"), json!("Do it."))
        );

        // A title edit leaves a waiting stub a stub: a stub that needs a queued Job is legal on its own.
        stub_add(&fx, "pl", "three", &["--needs", "a"]).ok();
        assert_eq!(
            job(&fx, &["edit", "three", "--title", "Renamed"], "pl").ok()["fields"],
            json!(["title"])
        );
        assert_eq!(job_row(&fx.env, "pl", "three", "tier"), Value::Null);
    }
}

// ---------- urgent Jobs (contract §2.2, §3.7 E3; commands.md §5.1, §5.8) ----------

/// `job add` of `key` with `--urgent` (and `extra` flags).
fn job_add_urgent(fx: &Fx, plan: &str, key: &str, extra: &[&str]) -> Run {
    let spec = spec_file(&fx.env);
    let title = format!("Title of {key}");
    let mut args = vec![
        "job", "add", key, "--title", &title, "--tier", "S2", "--spec", &spec, "--plan", plan,
        "--urgent",
    ];
    args.extend(extra);
    fx.cpd(&args)
}

#[test]
fn a_job_is_not_urgent_unless_added_with_urgent() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let out = job_add(&fx, "pl", "calm", "S2", &[]).ok();
    assert_eq!(out["job"]["urgent"], false);
    assert_eq!(job_row(&fx.env, "pl", "calm", "urgent"), 0);
    assert_eq!(events(&fx, "job-added")[0]["data"]["urgent"], false);

    let out = job_add_urgent(&fx, "pl", "hot", &[]).ok();
    assert_eq!(
        out["job"],
        json!({"key": "hot", "pr": "repo", "kind": "work", "status": "draft", "appended": false, "tier": "S2", "needs": [], "urgent": true})
    );
    assert_eq!(job_row(&fx.env, "pl", "hot", "urgent"), 1);
    assert_eq!(
        events(&fx, "job-added")[1]["data"],
        json!({"workflow": "repo", "kind": "work", "status": "draft", "appended": false, "tier": "S2", "needs": [], "urgent": true})
    );

    // The flag is not a Locked field: the same add with or without it is `already`, as for `--draft`.
    assert_eq!(job_add_urgent(&fx, "pl", "calm", &[]).ok()["already"], true);
    assert_eq!(
        job_row(&fx.env, "pl", "calm", "urgent"),
        0,
        "an add that found the Job changes nothing"
    );
    assert_eq!(job_add(&fx, "pl", "hot", "S2", &[]).ok()["already"], true);

    // After the lock the appended Job is `queued` and urgent at once, its fingerprint stored.
    lock(&fx.env, "pl");
    let out = job_add_urgent(&fx, "pl", "late", &[]).ok();
    assert_eq!(
        (&out["job"]["status"], &out["job"]["urgent"]),
        (&json!("queued"), &json!(true))
    );
    assert_eq!(job_row(&fx.env, "pl", "late", "urgent"), 1);
    // `--draft` and `--urgent` together: a draft, urgent.
    let out = job_add_urgent(&fx, "pl", "later", &["--draft"]).ok();
    assert_eq!(
        (&out["job"]["status"], &out["job"]["urgent"]),
        (&json!("draft"), &json!(true))
    );
}

#[test]
fn urgent_marks_a_queued_job_once_with_one_job_edited_and_changes_no_fingerprint() {
    let fx = Fx::new();
    drafted(&fx);
    lock(&fx.env, "pl");
    let fingerprint = job_row(&fx.env, "pl", "a", "fingerprint");
    assert_ne!(fingerprint, Value::Null);
    let edited = events(&fx, "job-edited").len();

    let out = job(&fx, &["urgent", "a"], "pl").ok();
    assert_eq!(
        (&out["job"], &out["fields"]),
        (
            &json!({"key": "a", "status": "queued", "urgent": true}),
            &json!(["urgent"])
        )
    );
    assert!(out.get("already").is_none());
    assert_eq!(job_row(&fx.env, "pl", "a", "urgent"), 1);
    assert_eq!(
        (
            job_row(&fx.env, "pl", "a", "status"),
            job_row(&fx.env, "pl", "a", "fingerprint")
        ),
        (json!("queued"), fingerprint)
    );
    assert_eq!(
        job_row(&fx.env, "pl", "b", "urgent"),
        0,
        "only the named Job"
    );
    let all = events(&fx, "job-edited");
    assert_eq!(all.len(), edited + 1);
    assert_eq!(
        (&all.last().unwrap()["job"], &all.last().unwrap()["data"]),
        (&json!("a"), &json!({"fields": ["urgent"]}))
    );
    // The Locked Job still matches its fingerprint: the flag is in none.
    assert_eq!(job(&fx, &["show", "a"], "pl").ok()["tamper"], json!([]));

    // Idempotent: already, nothing written.
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    let again = job(&fx, &["urgent", "a"], "pl").ok();
    assert_eq!(again["already"], true);
    assert_eq!(again["job"]["urgent"], true);
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );
}

#[test]
fn urgent_does_not_change_the_plan_fingerprint_of_a_draft() {
    let fx = Fx::new();
    drafted(&fx);
    let before = fx.cpd(&["plan", "render", "--plan", "pl"]).ok()["fingerprint"].clone();
    assert_eq!(
        job(&fx, &["urgent", "a"], "pl").ok()["job"],
        json!({"key": "a", "status": "draft", "urgent": true})
    );
    assert_eq!(
        fx.cpd(&["plan", "render", "--plan", "pl"]).ok()["fingerprint"],
        before
    );
}

#[test]
fn urgent_is_refused_for_a_job_that_is_not_queued_or_draft_and_for_an_unknown_one() {
    let fx = Fx::new();
    drafted(&fx);
    job_add(&fx, "pl", "c", "S2", &[]).ok();
    job_add(&fx, "pl", "d", "S2", &[]).ok();
    job_add(&fx, "pl", "e", "S2", &[]).ok();
    lock(&fx.env, "pl");
    engine(&fx.env, "pl", "a", "merged");
    engine(&fx.env, "pl", "c", "running");
    engine(&fx.env, "pl", "d", "failed");
    job(&fx, &["cancel", "e", "--reason", "No."], "pl").ok();
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    for (key, status) in [
        ("a", "merged"),
        ("c", "running"),
        ("d", "failed"),
        ("e", "cancelled"),
    ] {
        let error = refused(&job(&fx, &["urgent", key], "pl"), "illegal_transition", 3);
        assert_eq!(error["details"]["status"], status, "{key}");
        assert_eq!(job_row(&fx.env, "pl", key, "urgent"), 0, "{key}");
    }
    refused(&job(&fx, &["urgent", "ghost"], "pl"), "job_not_found", 4);
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs),
        "refusals write nothing"
    );
}

#[test]
fn list_and_show_carry_urgent() {
    let fx = Fx::new();
    drafted(&fx);
    job(&fx, &["urgent", "b"], "pl").ok();
    let listed = job(&fx, &["list"], "pl").ok();
    let flags: Vec<(&Value, &Value)> = listed["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| (&j["key"], &j["urgent"]))
        .collect();
    assert_eq!(
        flags,
        [(&json!("a"), &json!(false)), (&json!("b"), &json!(true))]
    );
    assert_eq!(job(&fx, &["show", "b"], "pl").ok()["job"]["urgent"], true);
    assert_eq!(job(&fx, &["show", "a"], "pl").ok()["job"]["urgent"], false);
    let shown = fx.cpd(&["plan", "show", "--plan", "pl"]).ok();
    let flags: Vec<&Value> = shown["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| &j["urgent"])
        .collect();
    assert_eq!(flags, [&json!(false), &json!(true)]);
}

// ---------- kinds (contract §2.2 "Job kinds"; commands.md §5.1, §5.2, §5.6, §5.7) ----------

/// A Spec file for a `test` Job: the Spec of every Job and the named tests with the files that hold them.
pub(super) fn test_spec_file(env: &TestEnv, name: &str, tests: Value, files: Value) -> String {
    let mut spec = spec_json();
    spec["tests"] = tests;
    spec["test_files"] = files;
    write_spec(env, name, &spec.to_string())
}

/// `job add <key> --kind <kind> --spec <spec>` into PR `pr` of Plan `plan`, a draft, with the Tier S2.
pub(super) fn add_kind(fx: &Fx, plan: &str, key: &str, kind: &str, spec: &str, pr: &str) -> Run {
    fx.cpd(&[
        "job",
        "add",
        key,
        "--title",
        &format!("Title of {key}"),
        "--tier",
        "S2",
        "--spec",
        spec,
        "--kind",
        kind,
        "--workflow",
        pr,
        "--plan",
        plan,
    ])
}

/// PR `key` of Plan `plan`, `waiting`, of the stored `shape`.
pub(super) fn shaped_pr(env: &TestEnv, plan: &str, key: &str, shape: &str) {
    fixture(
        env,
        &format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, shape, created_txn, created_at)
             VALUES ('{plan}', '{key}', '/repo', 'main', 'waiting', 'cpd/{plan}/{key}', '{shape}', cpd_txn(), 't');"
        ),
    );
}

#[test]
fn add_kind_stores_the_kind_and_a_test_jobs_tests_and_shows_them() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let spec = spec_file(&fx.env);
    let tests = test_spec_file(
        &fx.env,
        "tests.json",
        json!(["pr_open_job::merges_the_base", "pr_open_job::opens"]),
        json!(["cli/tests/pr_open_job.rs", "core/src/pr/"]),
    );
    // The default is `work`; the kinds are the Lead's: `docs`, `test` and `impl`.
    assert_eq!(
        job_add(&fx, "pl", "w", "S2", &[]).ok()["job"]["kind"],
        "work"
    );
    let docs = add_kind(&fx, "pl", "d", "docs", &spec, "repo").ok();
    assert_eq!(
        (&docs["job"]["kind"], &docs["job"]["status"]),
        (&json!("docs"), &json!("draft"))
    );
    let test = add_kind(&fx, "pl", "t", "test", &tests, "repo").ok();
    assert_eq!(test["job"]["kind"], "test");
    assert_eq!(
        add_kind(&fx, "pl", "i", "impl", &spec, "repo").ok()["job"]["kind"],
        "impl"
    );
    assert_eq!(
        job(
            &fx,
            &[
                "add", "c", "--title", "C", "--tier", "S2", "--spec", &spec, "--kind", "work"
            ],
            "pl"
        )
        .ok()["job"]["kind"],
        "work"
    );
    assert_eq!(
        job_row(&fx.env, "pl", "t", "tests"),
        json!("[\"pr_open_job::merges_the_base\",\"pr_open_job::opens\"]")
    );
    assert_eq!(
        job_row(&fx.env, "pl", "t", "test_files"),
        json!("[\"cli/tests/pr_open_job.rs\",\"core/src/pr/\"]")
    );
    assert_eq!(job_row(&fx.env, "pl", "d", "tests"), Value::Null);
    assert_eq!(job_row(&fx.env, "pl", "i", "kind"), json!("impl"));

    // The Event carries the kind; `job list` and `job show` show it, and a `test` Job's tests and files.
    let added = events(&fx, "job-added");
    let kinds: Vec<(&Value, &Value)> = added
        .iter()
        .map(|e| (&e["job"], &e["data"]["kind"]))
        .collect();
    assert_eq!(
        kinds,
        [
            (&json!("w"), &json!("work")),
            (&json!("d"), &json!("docs")),
            (&json!("t"), &json!("test")),
            (&json!("i"), &json!("impl")),
            (&json!("c"), &json!("work"))
        ]
    );
    let listed = job(&fx, &["list"], "pl").ok();
    let by_key: Vec<(&str, &str)> = listed["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| (j["key"].as_str().unwrap(), j["kind"].as_str().unwrap()))
        .collect();
    assert_eq!(
        by_key,
        [
            ("w", "work"),
            ("d", "docs"),
            ("t", "test"),
            ("i", "impl"),
            ("c", "work")
        ]
    );
    let shown = job(&fx, &["show", "t"], "pl").ok();
    assert_eq!(shown["job"]["kind"], "test");
    assert_eq!(
        shown["job"]["tests"],
        json!(["pr_open_job::merges_the_base", "pr_open_job::opens"])
    );
    assert_eq!(
        shown["job"]["test_files"],
        json!(["cli/tests/pr_open_job.rs", "core/src/pr/"])
    );
    let shown = job(&fx, &["show", "i"], "pl").ok();
    assert_eq!(
        (&shown["job"]["kind"], &shown["job"]["test_approved"]),
        (&json!("impl"), &Value::Null)
    );
    assert!(
        job(&fx, &["show", "d"], "pl").ok()["job"]
            .get("tests")
            .is_none(),
        "a `docs` Job has no tests"
    );
}

#[test]
fn the_kind_is_part_of_the_locked_fields_of_an_add_that_repeats() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let spec = spec_file(&fx.env);
    add_kind(&fx, "pl", "d", "docs", &spec, "repo").ok();
    let writes = (count(&fx.env, "txns"), count(&fx.env, "events"));
    assert_eq!(
        add_kind(&fx, "pl", "d", "docs", &spec, "repo").ok()["already"],
        true
    );
    assert_eq!((count(&fx.env, "txns"), count(&fx.env, "events")), writes);
    refused(
        &add_kind(&fx, "pl", "d", "impl", &spec, "repo"),
        "job_key_exists",
        3,
    );
    refused(&job_add(&fx, "pl", "d", "S2", &[]), "job_key_exists", 3);
    assert_eq!((count(&fx.env, "txns"), count(&fx.env, "events")), writes);
}

#[test]
fn add_kind_refusals_write_nothing() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let spec = spec_file(&fx.env);
    let tests = test_spec_file(
        &fx.env,
        "tests.json",
        json!(["t::one"]),
        json!(["tests/t.rs"]),
    );
    let writes = || {
        (
            count(&fx.env, "txns"),
            count(&fx.env, "events"),
            count(&fx.env, "jobs"),
        )
    };
    let before = writes();

    // `open` and `update` are cpd's (exit 3, `details.kind`).
    for kind in ["open", "update"] {
        let e = refused(
            &add_kind(&fx, "pl", "k", kind, &spec, "repo"),
            "kind_reserved",
            3,
        );
        assert_eq!(e["details"]["kind"], kind);
    }
    // Not a kind (clap).
    refused(
        &add_kind(&fx, "pl", "k", "other", &spec, "repo"),
        "usage",
        2,
    );
    // The Spec fits the kind: a `test` Job names its tests and files, every other kind neither.
    let e = refused(
        &add_kind(&fx, "pl", "k", "test", &spec, "repo"),
        "spec_invalid",
        2,
    );
    assert_eq!(e["details"]["path"], "$.tests");
    for kind in ["work", "docs", "impl"] {
        let e = refused(
            &add_kind(&fx, "pl", "k", kind, &tests, "repo"),
            "spec_invalid",
            2,
        );
        assert_eq!(e["details"]["path"], "$.tests", "{kind}");
    }
    let bad = test_spec_file(
        &fx.env,
        "bad.json",
        json!(["t::one"]),
        json!(["../escape.rs"]),
    );
    assert_eq!(
        refused(
            &add_kind(&fx, "pl", "k", "test", &bad, "repo"),
            "spec_invalid",
            2
        )["details"]["path"],
        "$.test_files[0]"
    );
    let bad = test_spec_file(
        &fx.env,
        "bad.json",
        json!(["t::one", "t::one"]),
        json!(["tests/t.rs"]),
    );
    assert_eq!(
        refused(
            &add_kind(&fx, "pl", "k", "test", &bad, "repo"),
            "spec_invalid",
            2
        )["details"]["path"],
        "$.tests[1]"
    );
    assert_eq!(writes(), before);
}

#[test]
fn a_job_cannot_need_an_open_or_an_update_job() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    job_add(&fx, "pl", "a", "S2", &[]).ok();
    lock(&fx.env, "pl");
    // What engine E4 adds: an `open` Job, queued, with its fingerprint and no Dependency.
    let fingerprint = "a".repeat(64);
    fixture(
        &fx.env,
        &format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, kind, created_txn, created_at)
             VALUES ('pl', 'repo', 'open-repo', 'Open', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'queued', 1, '{fingerprint}', 'open', cpd_txn(), 't');"
        ),
    );
    let writes = (count(&fx.env, "txns"), count(&fx.env, "events"));
    let e = refused(
        &job_add(&fx, "pl", "k", "S2", &["open-repo"]),
        "needs_unavailable",
        3,
    );
    assert_eq!(e["details"]["jobs"], json!(["open-repo"]));
    assert_eq!((count(&fx.env, "txns"), count(&fx.env, "events")), writes);
}

#[test]
fn the_stored_shape_decides_the_kinds_a_waiting_pr_may_have() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    for shape in ["tdd", "docs", "config"] {
        shaped_pr(&fx.env, "pl", shape, shape);
    }
    let spec = spec_file(&fx.env);
    let tests = test_spec_file(
        &fx.env,
        "tests.json",
        json!(["t::one"]),
        json!(["tests/t.rs"]),
    );
    let spec_of = |kind: &str| {
        if kind == "test" {
            tests.clone()
        } else {
            spec.clone()
        }
    };
    let allowed = |shape: &str| match shape {
        "tdd" => vec!["docs", "test", "impl", "verify"],
        "docs" => vec!["docs"],
        "config" => vec!["work"],
        _ => unreachable!(),
    };
    for shape in ["tdd", "docs", "config"] {
        for kind in ["work", "docs", "test", "impl"] {
            let key = format!("{shape}-{kind}");
            let run = add_kind(&fx, "pl", &key, kind, &spec_of(kind), shape);
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
    // A PR from before the migration (`free`) allows every kind.
    for kind in ["work", "docs", "test", "impl"] {
        assert_eq!(
            add_kind(
                &fx,
                "pl",
                &format!("free-{kind}"),
                kind,
                &spec_of(kind),
                "repo"
            )
            .ok()["job"]["kind"],
            kind
        );
    }
}

#[test]
fn an_impl_job_that_is_not_a_draft_needs_a_test_job_of_its_pr() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let spec = spec_file(&fx.env);
    let tests = test_spec_file(
        &fx.env,
        "tests.json",
        json!(["t::one"]),
        json!(["tests/t.rs"]),
    );
    add_kind(&fx, "pl", "t", "test", &tests, "repo").ok();
    add_kind(&fx, "pl", "d", "docs", &spec, "repo").ok();
    lock(&fx.env, "pl");
    let writes = (count(&fx.env, "txns"), count(&fx.env, "events"));
    // Appended and queued with no Dependency, or with one that is no test Job: refused, naming the test Jobs of the PR.
    let impl_job = |key: &str, needs: &str, draft: bool| {
        let mut args = vec![
            "job", "add", key, "--title", "I", "--tier", "S2", "--spec", &spec, "--kind", "impl",
            "--plan", "pl",
        ];
        if !needs.is_empty() {
            args.extend(["--needs", needs]);
        }
        if draft {
            args.push("--draft");
        }
        fx.cpd(&args)
    };
    let e = refused(&impl_job("i", "", false), "test_job_required", 3);
    assert_eq!(
        (&e["details"]["job"], &e["details"]["jobs"]),
        (&json!("i"), &json!(["t"]))
    );
    refused(&impl_job("i", "d", false), "test_job_required", 3);
    assert_eq!((count(&fx.env, "txns"), count(&fx.env, "events")), writes);
    // A draft is checked where it stops being one; a Job that needs the test Job is queued.
    assert_eq!(impl_job("i", "", true).ok()["job"]["status"], "draft");
    assert_eq!(impl_job("i2", "t", false).ok()["job"]["status"], "queued");
}

#[test]
fn edit_kind_changes_a_drafts_kind_and_its_spec_must_fit() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let spec = spec_file(&fx.env);
    let tests = test_spec_file(
        &fx.env,
        "tests.json",
        json!(["t::one"]),
        json!(["tests/t.rs"]),
    );
    for key in ["a", "b", "c"] {
        job_add(&fx, "pl", key, "S2", &[]).ok();
    }
    shaped_pr(&fx.env, "pl", "docs", "docs");
    // A kind alone: `work` to `docs`, and back.
    let out = job(&fx, &["edit", "a", "--kind", "docs"], "pl").ok();
    assert_eq!(out["fields"], json!(["kind"]));
    assert_eq!(job_row(&fx.env, "pl", "a", "kind"), json!("docs"));
    assert_eq!(
        events(&fx, "job-edited").last().unwrap()["data"],
        json!({"fields": ["kind"]})
    );
    let writes = (count(&fx.env, "txns"), count(&fx.env, "events"));
    assert_eq!(
        job(&fx, &["edit", "a", "--kind", "docs"], "pl").ok()["already"],
        true
    );
    assert_eq!((count(&fx.env, "txns"), count(&fx.env, "events")), writes);

    // To `test`, the Spec it has names no tests: refused; with a Spec that does, it changes together.
    let e = refused(
        &job(&fx, &["edit", "a", "--kind", "test"], "pl"),
        "spec_invalid",
        2,
    );
    assert_eq!(e["details"]["path"], "$.tests");
    let out = job(
        &fx,
        &["edit", "a", "--kind", "test", "--spec", &tests],
        "pl",
    )
    .ok();
    assert_eq!(out["fields"], json!(["kind", "tests", "test_files"]));
    assert_eq!(job_row(&fx.env, "pl", "a", "tests"), json!("[\"t::one\"]"));
    // Out of `test`, the Spec still names tests: refused, and given a Spec that does not, it goes.
    refused(
        &job(&fx, &["edit", "a", "--kind", "impl"], "pl"),
        "spec_invalid",
        2,
    );
    let out = job(&fx, &["edit", "a", "--kind", "impl", "--spec", &spec], "pl").ok();
    assert_eq!(out["fields"], json!(["kind", "tests", "test_files"]));
    assert_eq!(
        (
            job_row(&fx.env, "pl", "a", "tests"),
            job_row(&fx.env, "pl", "a", "test_files")
        ),
        (Value::Null, Value::Null)
    );
    // A Spec with tests for a Job that stays `impl` is refused too.
    refused(
        &job(&fx, &["edit", "a", "--spec", &tests], "pl"),
        "spec_invalid",
        2,
    );

    // `open` and `update` are cpd's; the stored shape of the PR holds for an edit as for an add.
    for kind in ["open", "update"] {
        assert_eq!(
            refused(
                &job(&fx, &["edit", "a", "--kind", kind], "pl"),
                "kind_reserved",
                3
            )["details"]["kind"],
            kind
        );
    }
    // Moving a Job to a PR whose shape does not allow its kind is the same refusal.
    let e = refused(
        &job(&fx, &["edit", "c", "--workflow", "docs"], "pl"),
        "shape_violation",
        3,
    );
    assert_eq!(
        (
            &e["details"]["pr"],
            &e["details"]["kind"],
            &e["details"]["allowed"]
        ),
        (&json!("docs"), &json!("work"), &json!(["docs"]))
    );
    job(
        &fx,
        &["edit", "c", "--workflow", "docs", "--kind", "docs"],
        "pl",
    )
    .ok();
    assert_eq!(
        (
            job_row(&fx.env, "pl", "c", "workflow"),
            job_row(&fx.env, "pl", "c", "kind")
        ),
        (json!("docs"), json!("docs"))
    );
}

#[test]
fn a_locked_jobs_kind_cannot_be_edited() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    job_add(&fx, "pl", "a", "S2", &[]).ok();
    lock(&fx.env, "pl");
    refused(
        &job(&fx, &["edit", "a", "--kind", "docs"], "pl"),
        "job_locked",
        3,
    );
    assert_eq!(job_row(&fx.env, "pl", "a", "kind"), json!("work"));
    // Nor by the database: the column is in `jobs_locked`.
    let e = fixture_error(&fx.env, "UPDATE jobs SET kind = 'docs' WHERE key = 'a'");
    assert!(e.contains("job_locked"), "{e}");
}

/// The message of the error `sql` fails with.
fn fixture_error(env: &TestEnv, sql: &str) -> String {
    store(env)
        .write("fixture", |tx| exec(tx, sql))
        .unwrap_err()
        .to_string()
}

// ---------- Tier rules (pro-d82, hs-d554): Workers use only H0-H3 and S0-S3 ----------

/// The refusal of pro-d82 (hs-d554 for the cost reason): `tier_not_allowed`, exit 2, naming the rule, its reason and the allowed range.
fn not_allowed(run: &Run, tier: &str) {
    let e = refused(run, "tier_not_allowed", 2);
    assert_eq!(e["details"]["tier"], tier, "{run:?}");
    assert_eq!(e["details"]["allowed"], "H0-H3, S0-S3", "{run:?}");
    assert_eq!(e["details"]["rule"], "pro-d82", "{run:?}");
    let message = e["message"].as_str().unwrap();
    for part in ["pro-d82", "hs-d554", "cost", "H0-H3, S0-S3"] {
        assert!(message.contains(part), "`{part}` missing from: {message}");
    }
}

#[test]
fn add_refuses_a_tier_outside_the_worker_range_and_writes_nothing() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    fixture(
        &fx.env,
        "UPDATE tier_catalog SET efforts = '[0,1,2,3,4]' WHERE letter = 'H';",
    ); // H4 reaches the Worker range check
    let (txns, evs, rows) = (
        count(&fx.env, "txns"),
        count(&fx.env, "events"),
        count(&fx.env, "jobs"),
    );
    for tier in ["H4", "O1", "O3", "S4", "S5"] {
        not_allowed(&job_add(&fx, "pl", "k", tier, &[]), tier);
    }
    // A stub draft's Tier is checked too.
    for tier in ["H4", "O1", "O3", "S4", "S5"] {
        not_allowed(&stub_add(&fx, "pl", "k", &["--tier", tier]), tier);
    }
    assert_eq!(
        (
            count(&fx.env, "txns"),
            count(&fx.env, "events"),
            count(&fx.env, "jobs")
        ),
        (txns, evs, rows)
    );
}

#[test]
fn add_accepts_h0_to_h3_and_s0_to_s3() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    for tier in ["H0", "H1", "H2", "H3", "S0", "S1", "S2", "S3"] {
        let key = format!("j-{}", tier.to_lowercase());
        assert_eq!(
            job_add(&fx, "pl", &key, tier, &[]).ok()["job"]["tier"],
            tier
        );
        assert_eq!(job_row(&fx.env, "pl", &key, "tier"), tier);
    }
}

#[test]
fn add_says_tier_invalid_before_tier_not_allowed() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    refused(&job_add(&fx, "pl", "k", "O9", &[]), "tier_invalid", 2);
    refused(&job_add(&fx, "pl", "k", "X5", &[]), "tier_invalid", 2);
    // H has no effort 9 (hs-n632): the brief's `H9` case is `tier_invalid`, and nothing is written.
    refused(&job_add(&fx, "pl", "k", "H9", &[]), "tier_invalid", 2);
}

/// B (hs-d796, hs-d797): a Lead replaces a cancelled `update` Job with a plain `work` Job at the chosen
/// Tier (§5.1's `-r` recipe, contract §3.8 "Updating"), and cancelling re-queues no `update` Job:
/// the poll (§3.7 E5, `core/src/pr/poll.rs` `update_for_conflict`) fires only on a transition of
/// `mergeable` to `conflicting`, so the cancel leaves none behind and a later transition queues one as always.
#[test]
fn a_lead_replaces_a_cancelled_update_job_with_a_work_job_at_the_chosen_tier() {
    let fx = Fx::new();
    // Plan `pl` in `deploy` whose PR `p` is `open`, of shape `tdd` (as job_fix.rs::open_tdd builds one).
    setup(&fx.env);
    plan(&fx.env, "pl", "deploy");
    fixture(
        &fx.env,
        &format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, shape, created_txn, created_at)\n             VALUES ('pl', 'p', '/repo', 'main', 'waiting', 'cpd/pl/p', 'tdd', cpd_txn(), 't');\n             UPDATE workflows SET status = 'dispatch', lead_worktree = '/wt2', started_at = 't' WHERE key = 'p';\n             {}\n             UPDATE workflows SET status = 'open', pr_number = 2, pr_url = 'https://github.com/acme/p/pull/2', pr_opened_at = 't',\n                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/p') WHERE key = 'p';\n             INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, fingerprint, created_txn, created_at)\n             VALUES ('pl', (SELECT key FROM workflows WHERE plan = 'pl' AND key = 'p'), 'up-p', 'Update it', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 1, 'update', '{}', cpd_txn(), 't');",
            pull_request("p", 2, "open"),
            "f".repeat(64)
        ),
    );
    assert_eq!(job_row(&fx.env, "pl", "up-p", "status"), json!("queued"));

    // The Lead cancels the wrong-Tier `update` Job and adds its replacement at the chosen Tier.
    let out = job(&fx, &["cancel", "up-p", "--reason", "wrong Tier"], "pl").ok();
    assert_eq!(
        out,
        json!({"ok": true, "job": "up-p", "status": "cancelled", "cancel_requested": false})
    );
    let spec = spec_file(&fx.env);
    let out = fx
        .cpd(&[
            "job",
            "add",
            "up-p-r1",
            "--title",
            "Bring the pull request up to date",
            "--tier",
            "S1",
            "--spec",
            &spec,
            "--workflow",
            "p",
            "--plan",
            "pl",
        ])
        .ok();
    assert_eq!(
        out["job"],
        json!({"key": "up-p-r1", "pr": "p", "kind": "work", "status": "queued", "appended": true, "tier": "S1", "needs": [], "urgent": false})
    );
    assert_eq!(job_row(&fx.env, "pl", "up-p-r1", "tier"), "S1");
    assert_eq!(
        events(&fx, "job-added").last().unwrap()["data"],
        json!({"workflow": "p", "kind": "work", "status": "queued", "appended": true, "tier": "S1", "needs": [], "urgent": false})
    );
    // E3 does not re-queue an `update` Job on the cancel (hs-d797).
    assert_eq!(
        fx.env.query::<i64>(
            "SELECT count(*) FROM jobs WHERE kind = 'update' AND status IN ('queued', 'running')"
        ),
        0
    );
    assert_eq!(job_row(&fx.env, "pl", "up-p", "status"), json!("cancelled"));
}

#[test]
fn edit_refuses_a_tier_outside_the_worker_range_and_writes_nothing() {
    let fx = Fx::new();
    drafted(&fx);
    fixture(
        &fx.env,
        "UPDATE tier_catalog SET efforts = '[0,1,2,3,4]' WHERE letter = 'H';",
    ); // H4 reaches the Worker range check
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    for tier in ["H4", "O1", "O3", "S4", "S5"] {
        not_allowed(&job(&fx, &["edit", "b", "--tier", tier], "pl"), tier);
        not_allowed(
            &job(&fx, &["edit", "b", "--tier", tier, "--title", "New"], "pl"),
            tier,
        );
    }
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );
    assert_eq!(job_row(&fx.env, "pl", "b", "tier"), "S2");
    assert_eq!(job_row(&fx.env, "pl", "b", "title"), "Title of b");
}

#[test]
fn edit_accepts_h0_to_h3_and_s0_to_s3() {
    let fx = Fx::new();
    drafted(&fx);
    for tier in ["H0", "H1", "H2", "S0", "S1", "S2", "H3", "S3"] {
        assert_eq!(
            job(&fx, &["edit", "b", "--tier", tier], "pl").ok()["fields"],
            json!(["tier"]),
            "{tier}"
        );
        assert_eq!(job_row(&fx.env, "pl", "b", "tier"), tier);
    }
}

#[test]
fn a_draft_stored_on_a_refused_tier_is_repaired_by_edit_and_other_edits_are_not_blocked() {
    let fx = Fx::new();
    drafted(&fx);
    // A store written before hs-d554.
    fixture(
        &fx.env,
        "UPDATE jobs SET tier = 'O3' WHERE plan = 'pl' AND key = 'b';",
    );
    assert_eq!(
        job(&fx, &["edit", "b", "--title", "Renamed"], "pl").ok()["fields"],
        json!(["title"])
    );
    assert_eq!(
        job_row(&fx.env, "pl", "b", "tier"),
        "O3",
        "no migration and no edit rewrites it"
    );
    not_allowed(&job(&fx, &["edit", "b", "--tier", "O4"], "pl"), "O4");
    assert_eq!(
        job(&fx, &["edit", "b", "--tier", "S3"], "pl").ok()["fields"],
        json!(["tier"])
    );
    assert_eq!(job_row(&fx.env, "pl", "b", "tier"), "S3");
    // Second pass: Tiers that were allowed before hs-d554.
    {
        let stored = "S4";
        fixture(
            &fx.env,
            &format!("UPDATE jobs SET tier = '{stored}' WHERE plan = 'pl' AND key = 'b';"),
        );
        assert_eq!(
            job(
                &fx,
                &["edit", "b", "--title", &format!("Renamed {stored}")],
                "pl"
            )
            .ok()["fields"],
            json!(["title"])
        );
        assert_eq!(job_row(&fx.env, "pl", "b", "tier"), stored);
        assert_eq!(
            job(&fx, &["edit", "b", "--tier", "S3"], "pl").ok()["fields"],
            json!(["tier"])
        );
        assert_eq!(job_row(&fx.env, "pl", "b", "tier"), "S3");
    }
}

#[test]
fn retry_refuses_a_tier_outside_the_worker_range_checks_it_first_and_writes_nothing() {
    let fx = Fx::new();
    drafted(&fx);
    job_add(&fx, "pl", "c", "S2", &[]).ok();
    lock(&fx.env, "pl");
    engine(&fx.env, "pl", "a", "failed");
    engine(&fx.env, "pl", "c", "running");
    fixture(
        &fx.env,
        "UPDATE tier_catalog SET efforts = '[0,1,2,3,4]' WHERE letter = 'H';",
    ); // H4 reaches the Worker range check
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    for tier in ["H4", "O1", "O3", "O5", "S4", "S5"] {
        not_allowed(&job(&fx, &["retry", "a", "--tier", tier], "pl"), tier);
    }
    // Checked first: a Job that is not failed still gets the Tier refusal.
    not_allowed(&job(&fx, &["retry", "c", "--tier", "O4"], "pl"), "O4");
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs)
    );
    assert_eq!(job_row(&fx.env, "pl", "a", "status"), "failed");
    assert_eq!(job_row(&fx.env, "pl", "a", "retry_tier"), Value::Null);
}

/// pro-d82: H4 is refused as tier_not_allowed even when the catalog has H4 (the catalog check passes, the Worker range does not).
#[test]
fn add_edit_and_retry_refuse_h4_even_when_the_catalog_has_h4() {
    let fx = Fx::new();
    drafted(&fx);
    job_add(&fx, "pl", "c", "S2", &[]).ok();
    lock(&fx.env, "pl");
    engine(&fx.env, "pl", "a", "failed");
    fixture(
        &fx.env,
        "UPDATE tier_catalog SET efforts = '[0,1,2,3,4]' WHERE letter = 'H';",
    );
    let (txns, evs, rows) = (
        count(&fx.env, "txns"),
        count(&fx.env, "events"),
        count(&fx.env, "jobs"),
    );
    not_allowed(&job_add(&fx, "pl", "k", "H4", &[]), "H4");
    not_allowed(
        &stub_add(&fx, "pl", "k", &["--tier", "H4", "--needs", "c"]),
        "H4",
    );
    not_allowed(&job(&fx, &["edit", "b", "--tier", "H4"], "pl"), "H4");
    not_allowed(&job(&fx, &["retry", "a", "--tier", "H4"], "pl"), "H4");
    assert_eq!(
        (
            count(&fx.env, "txns"),
            count(&fx.env, "events"),
            count(&fx.env, "jobs")
        ),
        (txns, evs, rows)
    );
    assert_eq!(job_row(&fx.env, "pl", "b", "tier"), "S2");
    assert_eq!(job_row(&fx.env, "pl", "a", "status"), "failed");
    assert_eq!(job_row(&fx.env, "pl", "a", "retry_tier"), Value::Null);
}

/// hs-d562: `GX` is the auxiliaries' Tier, never a Worker's: refused as `S4` is, after `tier_invalid`.
#[test]
fn add_edit_and_retry_refuse_gx_as_a_worker_tier() {
    let fx = Fx::new();
    drafted(&fx);
    job_add(&fx, "pl", "c", "S2", &[]).ok();
    lock(&fx.env, "pl");
    engine(&fx.env, "pl", "a", "failed");
    engine(&fx.env, "pl", "c", "running");
    let (txns, evs, rows) = (
        count(&fx.env, "txns"),
        count(&fx.env, "events"),
        count(&fx.env, "jobs"),
    );
    not_allowed(&job_add(&fx, "pl", "k", "GX", &[]), "GX");
    not_allowed(
        &stub_add(&fx, "pl", "k", &["--tier", "GX", "--needs", "c"]),
        "GX",
    );
    not_allowed(&job(&fx, &["edit", "b", "--tier", "GX"], "pl"), "GX");
    not_allowed(
        &job(&fx, &["edit", "b", "--tier", "GX", "--title", "New"], "pl"),
        "GX",
    );
    not_allowed(&job(&fx, &["retry", "a", "--tier", "GX"], "pl"), "GX");
    // Checked first: a Job that is not failed still gets the Tier refusal.
    not_allowed(&job(&fx, &["retry", "c", "--tier", "GX"], "pl"), "GX");
    assert_eq!(
        (
            count(&fx.env, "txns"),
            count(&fx.env, "events"),
            count(&fx.env, "jobs")
        ),
        (txns, evs, rows)
    );
    assert_eq!(job_row(&fx.env, "pl", "b", "tier"), "S2");
    assert_eq!(job_row(&fx.env, "pl", "b", "title"), "Title of b");
    assert_eq!(job_row(&fx.env, "pl", "a", "status"), "failed");
    assert_eq!(job_row(&fx.env, "pl", "a", "retry_tier"), Value::Null);
}

/// hs-d562: G has no number; `G0` is `tier_invalid` (not `tier_not_allowed`) and the message says to write `GX`.
#[test]
fn a_job_tier_g0_is_tier_invalid_and_says_write_gx() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let e = refused(&job_add(&fx, "pl", "k", "G0", &[]), "tier_invalid", 2);
    let message = e["message"].as_str().unwrap();
    assert!(message.contains("GX"), "{message}");
    assert!(message.contains("hs-d562"), "{message}");
}

#[test]
fn retry_accepts_h0_to_h3_and_s0_to_s3() {
    for tier in ["H0", "H1", "H2", "S0", "S3", "H3"] {
        let fx = Fx::new();
        drafted(&fx);
        lock(&fx.env, "pl");
        engine(&fx.env, "pl", "a", "failed");
        let out = job(&fx, &["retry", "a", "--tier", tier], "pl").ok();
        assert_eq!(
            out,
            json!({"ok": true, "job": "a", "status": "queued", "tier": tier})
        );
        assert_eq!(job_row(&fx.env, "pl", "a", "retry_tier"), tier);
    }
}

// ---------- the shape's `shape_incomplete` and `test_job_required` at the review, the lock and the cancel; `job approve-tests` ----------
// (contract §2.2 "Job kinds", §7.7; commands.md §4.6, §4.8, §5.3, §5.9)

/// `plan review` of Plan `plan`.
pub(super) fn review(fx: &Fx, plan: &str) -> Run {
    fx.cpd(&["plan", "review", "--title", "The batch", "--plan", plan])
}

/// `plan lock` of Plan `plan`.
pub(super) fn plan_lock(fx: &Fx, plan: &str) -> Run {
    fx.cpd(&["plan", "lock", "--plan", plan])
}

/// `job add <key> --kind <kind> --needs <needs>` into PR `pr`, with the Spec of the kind (a `test` Job names `t::<key>` in `tests/<key>.rs`).
pub(super) fn add_needing(
    fx: &Fx,
    plan: &str,
    key: &str,
    kind: &str,
    needs: &[&str],
    pr: &str,
) -> Run {
    let spec = if kind == "test" {
        test_spec_file(
            &fx.env,
            &format!("{key}.json"),
            json!([format!("t::{key}")]),
            json!([format!("tests/{key}.rs")]),
        )
    } else {
        spec_file(&fx.env)
    };
    let needs = needs.join(",");
    let title = format!("Title of {key}");
    let mut args = vec![
        "job",
        "add",
        key,
        "--title",
        &title,
        "--tier",
        "S2",
        "--spec",
        &spec,
        "--kind",
        kind,
        "--workflow",
        pr,
        "--plan",
        plan,
    ];
    if !needs.is_empty() {
        args.extend(["--needs", &needs]);
    }
    fx.cpd(&args)
}

#[test]
fn plan_review_and_plan_lock_refuse_an_impl_draft_that_needs_no_test_job_of_its_pr() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    add_kind(
        &fx,
        "pl",
        "t",
        "test",
        &test_spec_file(&fx.env, "t.json", json!(["t::one"]), json!(["tests/t.rs"])),
        "repo",
    )
    .ok();
    // A draft is added; it is checked where it stops being one. It needs a Job of its PR that is no test Job.
    job_add(&fx, "pl", "w", "S2", &[]).ok();
    add_needing(&fx, "pl", "i", "impl", &["w"], "repo").ok();
    // The outline review needs first (hs-d502, #205), so the refusal tested is the shape's.
    fx.env.outline_for_review("pl");
    let writes = (count(&fx.env, "txns"), count(&fx.env, "events"));
    let e = refused(&review(&fx, "pl"), "test_job_required", 3);
    assert_eq!(e["details"]["jobs"], json!(["i"]));
    // `plan lock` refuses it before it looks at the review (which does not exist).
    let e = refused(&plan_lock(&fx, "pl"), "test_job_required", 3);
    assert_eq!(e["details"]["jobs"], json!(["i"]));
    assert_eq!(
        (
            count(&fx.env, "txns"),
            count(&fx.env, "events"),
            count(&fx.env, "plan_reviews")
        ),
        (writes.0, writes.1, 0),
        "a refusal writes nothing"
    );

    // It needs the test Job: the review is requested.
    job(&fx, &["edit", "i", "--needs", "t"], "pl").ok();
    let out = review(&fx, "pl").ok();
    assert_eq!(out["review"]["status"], "pending");
}

#[test]
fn plan_review_and_plan_lock_refuse_a_tdd_pr_with_no_test_or_no_impl_job() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    shaped_pr(&fx.env, "pl", "api", "tdd");
    // A PR of a shape that is not `tdd`, and a PR from before the migration, are not asked for a test Job.
    shaped_pr(&fx.env, "pl", "docs", "docs");
    shaped_pr(&fx.env, "pl", "cfg", "config");
    add_kind(&fx, "pl", "docs-d", "docs", &spec_file(&fx.env), "docs").ok();
    add_needing(&fx, "pl", "cfg-w", "work", &[], "cfg").ok();
    add_needing(&fx, "pl", "free-w", "work", &[], "repo").ok();
    // `api` has a docs Job only: it lacks both.
    add_kind(&fx, "pl", "api-d", "docs", &spec_file(&fx.env), "api").ok();
    // The outline review needs first (hs-d502, #205), so the refusal tested is the shape's.
    fx.env.outline_for_review("pl");
    let writes = (count(&fx.env, "txns"), count(&fx.env, "events"));
    let e = refused(&review(&fx, "pl"), "shape_incomplete", 3);
    assert_eq!(
        (&e["details"]["pr"], &e["details"]["missing"]),
        (&json!("api"), &json!(["test", "impl"]))
    );
    let e = refused(&plan_lock(&fx, "pl"), "shape_incomplete", 3);
    assert_eq!(
        (&e["details"]["pr"], &e["details"]["missing"]),
        (&json!("api"), &json!(["test", "impl"]))
    );
    assert_eq!(
        (
            count(&fx.env, "txns"),
            count(&fx.env, "events"),
            count(&fx.env, "plan_reviews")
        ),
        (writes.0, writes.1, 0)
    );

    // A test Job but no impl Job.
    add_needing(&fx, "pl", "api-t", "test", &["api-d"], "api").ok();
    let e = refused(&review(&fx, "pl"), "shape_incomplete", 3);
    assert_eq!(e["details"]["missing"], json!(["impl"]));
    // A cancelled one does not count.
    add_needing(&fx, "pl", "api-i", "impl", &["api-t"], "api").ok();
    fixture(
        &fx.env,
        "UPDATE jobs SET status = 'cancelled', cancel_reason = 'x', cancel_requested_at = 't' WHERE key = 'api-i'",
    );
    assert_eq!(
        refused(&review(&fx, "pl"), "shape_incomplete", 3)["details"]["missing"],
        json!(["impl"])
    );
    // A live one completes the PR: the review is requested, and `plan lock` goes on to the review it lacks.
    add_needing(&fx, "pl", "api-i2", "impl", &["api-t"], "api").ok();
    assert_eq!(review(&fx, "pl").ok()["review"]["status"], "pending");
    refused(&plan_lock(&fx, "pl"), "plan_not_approved", 3);
}

#[test]
fn job_cancel_keeps_the_last_test_and_the_last_impl_job_of_a_tdd_pr_that_has_not_opened() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    shaped_pr(&fx.env, "pl", "api", "tdd");
    add_kind(&fx, "pl", "d", "docs", &spec_file(&fx.env), "api").ok();
    // The test Job does not need the docs one: cancelling a need cascades to its dependents (contract §3.2
    // "Cancel", hs-d647), and this test is about the shape, not the cascade.
    add_needing(&fx, "pl", "t", "test", &[], "api").ok();
    add_needing(&fx, "pl", "i", "impl", &["t"], "api").ok();
    // A stub the PR does not need goes.
    job(&fx, &["cancel", "d", "--reason", "No docs."], "pl").ok();
    // The last `impl` and the last `test` Job do not: `details` `{pr, missing}`, nothing written.
    let writes = (count(&fx.env, "txns"), count(&fx.env, "events"));
    let e = refused(
        &job(&fx, &["cancel", "i", "--reason", "No."], "pl"),
        "shape_incomplete",
        3,
    );
    assert_eq!(
        (&e["details"]["pr"], &e["details"]["missing"]),
        (&json!("api"), &json!(["impl"]))
    );
    let e = refused(
        &job(&fx, &["cancel", "t", "--reason", "No."], "pl"),
        "shape_incomplete",
        3,
    );
    assert_eq!(
        (&e["details"]["pr"], &e["details"]["missing"]),
        (&json!("api"), &json!(["test"]))
    );
    assert_eq!((count(&fx.env, "txns"), count(&fx.env, "events")), writes);
    assert_eq!(
        (
            job_row(&fx.env, "pl", "i", "status"),
            job_row(&fx.env, "pl", "t", "status")
        ),
        (json!("draft"), json!("draft"))
    );
    // A replacement is added first; then the first one goes, and the replacement is the last.
    add_needing(&fx, "pl", "i2", "impl", &["t"], "api").ok();
    job(&fx, &["cancel", "i", "--reason", "Replaced."], "pl").ok();
    assert_eq!(job_row(&fx.env, "pl", "i", "status"), json!("cancelled"));
    let e = refused(
        &job(&fx, &["cancel", "i2", "--reason", "No."], "pl"),
        "shape_incomplete",
        3,
    );
    assert_eq!(e["details"]["missing"], json!(["impl"]));

    // After the lock (a `queued` Job) it is the same while the PR has not opened.
    lock(&fx.env, "pl");
    let e = refused(
        &job(&fx, &["cancel", "i2", "--reason", "No."], "pl"),
        "shape_incomplete",
        3,
    );
    assert_eq!(e["details"]["missing"], json!(["impl"]));
    assert_eq!(job_row(&fx.env, "pl", "i2", "status"), json!("queued"));
}

#[test]
fn job_cancel_checks_no_shape_for_a_pr_that_is_not_tdd_or_that_has_opened() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    for shape in ["docs", "config", "tdd"] {
        shaped_pr(&fx.env, "pl", shape, shape);
    }
    add_kind(&fx, "pl", "d", "docs", &spec_file(&fx.env), "docs").ok();
    add_needing(&fx, "pl", "c", "work", &[], "config").ok();
    // A PR from before the migration (`free`) has any kinds, and any of them is cancelled.
    add_kind(&fx, "pl", "free-i", "impl", &spec_file(&fx.env), "repo").ok();
    // The contrast: the lone `docs` Job of a `tdd` PR goes, its lone `impl` Job does not.
    add_kind(&fx, "pl", "tdd-d", "docs", &spec_file(&fx.env), "tdd").ok();
    add_kind(&fx, "pl", "tdd-i", "impl", &spec_file(&fx.env), "tdd").ok();
    refused(
        &job(&fx, &["cancel", "tdd-i", "--reason", "No."], "pl"),
        "shape_incomplete",
        3,
    );
    job(&fx, &["cancel", "tdd-d", "--reason", "No."], "pl").ok();
    for key in ["d", "c", "free-i"] {
        job(&fx, &["cancel", key, "--reason", "No."], "pl").ok();
        assert_eq!(
            job_row(&fx.env, "pl", key, "status"),
            json!("cancelled"),
            "{key}"
        );
    }

    // A `tdd` PR that has opened its pull request: a fix is whatever it is, and the last `impl` Job may go.
    drop(fx); // one `Fx` lives at a time in a process
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "deploy");
    fixture(
        &fx.env,
        &format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, shape, created_txn, created_at)
             VALUES ('pl', 'fix', '/repo', 'main', 'waiting', 'cpd/pl/fix', 'tdd', cpd_txn(), 't');
             UPDATE workflows SET status = 'dispatch', lead_worktree = '/wt2', started_at = 't' WHERE key = 'fix';
             {}
             UPDATE workflows SET status = 'open', pr_number = 2, pr_url = 'https://github.com/acme/fix/pull/2', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/fix') WHERE key = 'fix';
             INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                               kind, tests, test_files, created_txn, created_at)
             VALUES ('pl', 'fix', 'fix-t', 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 0, printf('%064d', 1),
                     'test', '[\"t::a\"]', '[\"tests/a.rs\"]', cpd_txn(), 't'),
                    ('pl', 'fix', 'fix-i', 'I', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 0, printf('%064d', 2),
                     'impl', NULL, NULL, cpd_txn(), 't');",
            pull_request("fix", 2, "open")
        ),
    );
    job(&fx, &["cancel", "fix-i", "--reason", "Not needed."], "pl").ok();
    job(&fx, &["cancel", "fix-t", "--reason", "Not needed."], "pl").ok();
    assert_eq!(
        (
            job_row(&fx.env, "pl", "fix-i", "status"),
            job_row(&fx.env, "pl", "fix-t", "status")
        ),
        (json!("cancelled"), json!("cancelled"))
    );
}

/// A Plan `pl` locked with the test Job `t` (`tests/t.rs`, `tests/more/`) and the impl Job `i` that needs it, both queued.
pub(super) fn locked_tdd(fx: &Fx) {
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let tests = test_spec_file(
        &fx.env,
        "t.json",
        json!(["t::one"]),
        json!(["tests/t.rs", "tests/more/"]),
    );
    add_kind(fx, "pl", "t", "test", &tests, "repo").ok();
    add_needing(fx, "pl", "i", "impl", &["t"], "repo").ok();
}

pub(super) fn approve(fx: &Fx, args: &[&str]) -> Run {
    let mut all = vec!["job", "approve-tests"];
    all.extend(args);
    all.extend(["--plan", "pl"]);
    fx.cpd(&all)
}

#[test]
fn approve_tests_records_the_paths_the_impl_job_may_change_and_tells_the_event() {
    let fx = Fx::new();
    locked_tdd(&fx);
    // A draft is not a Job that can be checked yet.
    refused(
        &approve(&fx, &["i", "--path", "tests/t.rs", "--reason", "Wrong."]),
        "illegal_transition",
        3,
    );
    lock(&fx.env, "pl");
    let out = approve(
        &fx,
        &[
            "i",
            "--path",
            "tests/t.rs",
            "--reason",
            "The assertion is wrong.",
        ],
    )
    .ok();
    assert_eq!(
        out,
        json!({"ok": true, "job": {"key": "i", "status": "queued", "kind": "impl"}, "approved": ["tests/t.rs"], "fields": ["test_approved"]})
    );
    assert_eq!(
        job_row(&fx.env, "pl", "i", "test_approved"),
        json!("[\"tests/t.rs\"]")
    );
    let edited = events(&fx, "job-edited");
    let last = edited.last().unwrap();
    assert_eq!(
        (&last["job"], &last["data"]),
        (&json!("i"), &json!({"fields": ["test_approved"]}))
    );
    let text = last["text"].as_str().unwrap();
    assert!(
        text.contains("tests/t.rs") && text.contains("The assertion is wrong."),
        "{text}"
    );

    // The union of what was approved; a path inside a guarded directory counts; paths already approved write nothing.
    let out = approve(
        &fx,
        &[
            "i",
            "--path",
            "tests/more/m.rs",
            "--path",
            "tests/t.rs",
            "--reason",
            "More.",
        ],
    )
    .ok();
    let sorted = |v: &Value| {
        let mut all: Vec<String> = serde_json::from_value(v.clone()).unwrap();
        all.sort();
        all
    };
    assert_eq!(
        sorted(&out["approved"]),
        ["tests/more/m.rs", "tests/t.rs"],
        "`approved` is what the Job has now: the union"
    );
    let stored: Value = serde_json::from_str(
        job_row(&fx.env, "pl", "i", "test_approved")
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(sorted(&stored), ["tests/more/m.rs", "tests/t.rs"]);
    let writes = (count(&fx.env, "txns"), count(&fx.env, "events"));
    let out = approve(&fx, &["i", "--path", "tests/t.rs", "--reason", "Again."]).ok();
    assert_eq!(
        (&out["already"], &out["fields"]),
        (&json!(true), &json!([]))
    );
    assert_eq!((count(&fx.env, "txns"), count(&fx.env, "events")), writes);

    // Not a Locked field: the Job's fingerprint is what it was.
    assert_eq!(
        job_row(&fx.env, "pl", "i", "fingerprint"),
        json!(locked_fingerprint_of(&fx.env, "i"))
    );
}

/// The Locked-Job fingerprint of Job `key` of Plan `pl`, recomputed.
pub(super) fn locked_fingerprint_of(env: &TestEnv, key: &str) -> String {
    let id = PlanId::parse("pl").unwrap();
    let all = store(env).read(|conn| jobs::load(conn, &id)).unwrap();
    locked_fingerprint(all.iter().find(|j| j.key == key).unwrap())
}

#[test]
fn approve_tests_is_the_leads_and_not_a_workers() {
    let fx = Fx::new();
    locked_tdd(&fx);
    lock(&fx.env, "pl");
    let args = [
        "job",
        "approve-tests",
        "i",
        "--path",
        "tests/t.rs",
        "--reason",
        "Wrong.",
        "--plan",
        "pl",
    ];
    let writes = (count(&fx.env, "txns"), count(&fx.env, "events"));
    refused(
        &fx.cpd_with(fx.env.root(), &args, &[("CPD_ATTEMPT", "i-a1")]),
        "not_a_lead",
        3,
    );
    assert_eq!((count(&fx.env, "txns"), count(&fx.env, "events")), writes);
    assert_eq!(job_row(&fx.env, "pl", "i", "test_approved"), Value::Null);
    fx.cpd_with(fx.env.root(), &args, &[]).ok();
}

#[test]
fn approve_tests_refuses_what_it_cannot_approve_and_writes_nothing() {
    let fx = Fx::new();
    locked_tdd(&fx);
    lock(&fx.env, "pl");
    let writes = || (count(&fx.env, "txns"), count(&fx.env, "events"));
    let before = writes();
    refused(
        &approve(
            &fx,
            &["ghost", "--path", "tests/t.rs", "--reason", "Wrong."],
        ),
        "job_not_found",
        4,
    );
    // Only an `impl` Job is approved for; a `test` Job guards its own files.
    refused(
        &approve(&fx, &["t", "--path", "tests/t.rs", "--reason", "Wrong."]),
        "not_an_impl_job",
        3,
    );
    // Every path must be guarded by the `test_files` of a `test` Job it needs; the unguarded are listed and nothing is approved.
    let e = refused(
        &approve(
            &fx,
            &[
                "i",
                "--path",
                "tests/t.rs",
                "--path",
                "src/lib.rs",
                "--path",
                "tests/other.rs",
                "--reason",
                "Wrong.",
            ],
        ),
        "path_not_guarded",
        3,
    );
    assert_eq!(
        e["details"]["paths"],
        json!(["src/lib.rs", "tests/other.rs"])
    );
    assert_eq!(job_row(&fx.env, "pl", "i", "test_approved"), Value::Null);
    // A path and a reason are required.
    refused(&approve(&fx, &["i", "--reason", "Wrong."]), "usage", 2);
    refused(&approve(&fx, &["i", "--path", "tests/t.rs"]), "usage", 2);
    assert_eq!(writes(), before);
    // A Job that has ended is not approved for.
    engine(&fx.env, "pl", "i", "merged");
    refused(
        &approve(&fx, &["i", "--path", "tests/t.rs", "--reason", "Wrong."]),
        "illegal_transition",
        3,
    );
}

// ---------- the touches check (hs-d512; commands.md §5.1, §5.3) ----------

/// Plan `pl` in `plan`, whose PR's repo is a repo of the fake git (a real directory, its objects in the `Universe`)
/// with `src/a.rs` on `main` (the tree a Job of the unstarted PR starts from).
pub(super) fn touches_fx() -> Fx {
    let fx = Fx::with_ports();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let repo = fx.git_repo("pay").canonicalize().unwrap();
    fx.commit_file(&repo, "src/a.rs", "// a\n", "src");
    fixture(
        &fx.env,
        &format!(
            "UPDATE workflows SET repo = '{}' WHERE plan = 'pl';",
            repo.display()
        ),
    );
    fx
}

/// `job add <key> --spec` of a Spec whose `touches` are `touches`, into Plan `pl`.
pub(super) fn add_touching(fx: &Fx, key: &str, touches: &[&str]) -> Run {
    let mut spec = spec_json();
    spec["touches"] = json!(touches);
    let spec = write_spec(&fx.env, &format!("{key}.json"), &spec.to_string());
    fx.cpd(&[
        "job",
        "add",
        key,
        "--title",
        &format!("Title of {key}"),
        "--tier",
        "S2",
        "--spec",
        &spec,
        "--plan",
        "pl",
    ])
}

#[test]
fn job_add_refuses_a_touches_path_missing_from_the_tree() {
    let fx = touches_fx();
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    let error = refused(
        &add_touching(&fx, "a", &["src/a.rs", "nope/x.rs"]),
        "touches_missing",
        3,
    );
    assert_eq!(error["details"]["missing"], json!(["nope/x.rs"]), "{error}");
    assert!(error["details"]["tree"].is_string(), "{error}");
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs),
        "a refusal writes nothing"
    );
    assert_eq!(count(&fx.env, "jobs"), 0);
}

#[test]
fn job_add_accepts_a_new_file_in_an_existing_directory() {
    let fx = touches_fx();
    // The check is on: a path whose directory is missing too is refused, while the entries below pass.
    refused(
        &add_touching(&fx, "bad", &["nope/x.rs"]),
        "touches_missing",
        3,
    );
    add_touching(&fx, "a", &["src/new.rs"]).ok();
    assert_eq!(job_row(&fx.env, "pl", "a", "touches"), "[\"src/new.rs\"]");
}

#[test]
fn job_add_accepts_a_glob_that_matches() {
    let fx = touches_fx();
    // The check is on: a path whose directory is missing too is refused, while the entries below pass.
    refused(
        &add_touching(&fx, "bad", &["nope/x.rs"]),
        "touches_missing",
        3,
    );
    add_touching(&fx, "a", &["src/*.rs"]).ok();
    assert_eq!(job_row(&fx.env, "pl", "a", "touches"), "[\"src/*.rs\"]");
}

#[test]
fn job_edit_refuses_a_touches_path_missing_from_the_tree() {
    let fx = touches_fx();
    add_touching(&fx, "a", &["src/a.rs"]).ok();
    let (txns, evs) = (count(&fx.env, "txns"), count(&fx.env, "events"));
    let mut spec = spec_json();
    spec["touches"] = json!(["nope/x.rs"]);
    let spec = write_spec(&fx.env, "edit.json", &spec.to_string());
    let error = refused(
        &job(&fx, &["edit", "a", "--spec", &spec], "pl"),
        "touches_missing",
        3,
    );
    assert_eq!(error["details"]["missing"], json!(["nope/x.rs"]), "{error}");
    assert_eq!(
        (count(&fx.env, "txns"), count(&fx.env, "events")),
        (txns, evs),
        "a refusal writes nothing"
    );
    assert_eq!(job_row(&fx.env, "pl", "a", "touches"), "[\"src/a.rs\"]");
}

// ---------- a Spec names no primary checkout (commands.md §5.1, §5.2) ----------

/// Points Plan `pl`'s one PR (`repo`) at `path`, the primary checkout the Spec check searches for, and
/// returns the absolute path the check reports in `details.primary`. The path is not made on disk: the
/// check is of the stored path, and a real directory would make the `touches` tree check run `git`.
fn primary(env: &TestEnv, path: PathBuf) -> String {
    let root = path.to_str().unwrap().to_owned();
    fixture(
        env,
        &format!("UPDATE workflows SET repo = '{root}' WHERE plan = 'pl';"),
    );
    root
}

#[test]
fn job_add_refuses_a_spec_that_names_a_primary_checkout_in_any_field() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let root = primary(&fx.env, fx.env.root().join("repos/primary"));
    let writes = (
        count(&fx.env, "txns"),
        count(&fx.env, "events"),
        count(&fx.env, "jobs"),
    );

    // `goal` is `<root>` followed by a space; every other field holds `<root>/docs/x.md`.
    let with_root = |field: &str| {
        let value = if field == "goal" {
            format!("{root} is the checkout.")
        } else {
            format!("{root}/docs/x.md")
        };
        let mut spec = spec_json();
        match field {
            "goal" => spec["goal"] = json!(value),
            "context" => spec["context"] = json!(value),
            "touches[0]" => spec["touches"] = json!([value]),
            "acceptance[1]" => spec["acceptance"] = json!(["cargo test", value]),
            "out_of_scope[0]" => spec["out_of_scope"] = json!([value]),
            "tests[0]" => {
                spec["tests"] = json!([value]);
                spec["test_files"] = json!(["cli/tests/flow/job.rs"]);
            }
            "test_files[0]" => {
                spec["tests"] = json!(["job::one"]);
                spec["test_files"] = json!([value]);
            }
            _ => unreachable!(),
        }
        write_spec(&fx.env, "primary.json", &spec.to_string())
    };

    let cases = [
        ("goal", "work", "$.goal"),
        ("context", "work", "$.context"),
        ("touches[0]", "work", "$.touches[0]"),
        ("acceptance[1]", "work", "$.acceptance[1]"),
        ("out_of_scope[0]", "work", "$.out_of_scope[0]"),
        ("tests[0]", "test", "$.tests[0]"),
        ("test_files[0]", "test", "$.test_files[0]"),
    ];
    for (i, (field, kind, path)) in cases.into_iter().enumerate() {
        let spec = with_root(field);
        let key = format!("k{i}");
        let run = fx.cpd(&[
            "job", "add", &key, "--title", "T", "--tier", "S2", "--spec", &spec, "--kind", kind,
            "--plan", "pl",
        ]);
        let error = refused(&run, "spec_invalid", 2);
        assert_eq!(error["details"]["path"].as_str(), Some(path), "{field}");
        assert_eq!(
            error["details"]["primary"].as_str(),
            Some(root.as_str()),
            "{field}"
        );
        assert_eq!(error["details"]["pr"].as_str(), Some("repo"), "{field}");
    }
    assert_eq!(
        (
            count(&fx.env, "txns"),
            count(&fx.env, "events"),
            count(&fx.env, "jobs")
        ),
        writes
    );
}

#[test]
fn job_add_refuses_the_tilde_and_home_forms_of_the_root() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let root = primary(&fx.env, fx.env.home().join("repos/primary"));
    let writes = (
        count(&fx.env, "txns"),
        count(&fx.env, "events"),
        count(&fx.env, "jobs"),
    );

    for (i, value) in [
        "~/repos/primary/docs/x.md",
        "$HOME/repos/primary",
        "${HOME}/repos/primary",
    ]
    .into_iter()
    .enumerate()
    {
        let mut spec = spec_json();
        spec["goal"] = json!(value);
        let spec = write_spec(&fx.env, "home.json", &spec.to_string());
        let key = format!("k{i}");
        let run = fx.cpd(&[
            "job", "add", &key, "--title", "T", "--tier", "S2", "--spec", &spec, "--plan", "pl",
        ]);
        let error = refused(&run, "spec_invalid", 2);
        assert_eq!(error["details"]["path"].as_str(), Some("$.goal"), "{value}");
        assert_eq!(
            error["details"]["primary"].as_str(),
            Some(root.as_str()),
            "{value}"
        );
        assert_eq!(error["details"]["pr"].as_str(), Some("repo"), "{value}");
    }
    assert_eq!(
        (
            count(&fx.env, "txns"),
            count(&fx.env, "events"),
            count(&fx.env, "jobs")
        ),
        writes
    );
}

#[test]
fn job_add_accepts_repo_relative_paths_a_sibling_directory_and_a_worktree_path() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let root = primary(&fx.env, fx.env.root().join("repos/primary"));

    // A Spec with `docs/x.md` paths only.
    let mut spec = spec_json();
    spec["goal"] = json!("Follow docs/x.md.");
    spec["context"] = json!("docs/x.md");
    spec["touches"] = json!(["docs/x.md"]);
    spec["acceptance"] = json!(["docs/x.md"]);
    spec["out_of_scope"] = json!(["docs/x.md"]);
    let spec = write_spec(&fx.env, "relative.json", &spec.to_string());
    fx.cpd(&[
        "job", "add", "a", "--title", "A", "--tier", "S2", "--spec", &spec, "--plan", "pl",
    ])
    .ok();

    // A sibling directory, a linked worktree and a numbered one merely begin with the root string.
    let mut spec = spec_json();
    spec["goal"] = json!(format!(
        "{root}-old/x, {root}.wt/x and {root}2 are other checkouts."
    ));
    spec["touches"] = json!(["docs/x.md"]);
    let spec = write_spec(&fx.env, "sibling.json", &spec.to_string());
    fx.cpd(&[
        "job", "add", "b", "--title", "B", "--tier", "S2", "--spec", &spec, "--plan", "pl",
    ])
    .ok();

    // The root of a repo that is not a PR of this Plan.
    let other = fx.env.root().join("repos/other");
    let mut spec = spec_json();
    spec["goal"] = json!(format!(
        "{} is not one of the Plan's repos.",
        other.display()
    ));
    spec["touches"] = json!(["docs/x.md"]);
    let spec = write_spec(&fx.env, "other.json", &spec.to_string());
    fx.cpd(&[
        "job", "add", "c", "--title", "C", "--tier", "S2", "--spec", &spec, "--plan", "pl",
    ])
    .ok();
    assert_eq!(count(&fx.env, "jobs"), 3);
}

#[test]
fn job_add_checks_the_repo_of_every_pr_of_the_plan() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let second = fx.env.root().join("repos/second");
    primary(&fx.env, fx.env.root().join("repos/first"));
    fixture(
        &fx.env,
        &format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
             VALUES ('pl', 'two', '{}', 'main', 'waiting', 'cpd/pl/two', cpd_txn(), 't');",
            second.display()
        ),
    );
    let writes = (
        count(&fx.env, "txns"),
        count(&fx.env, "events"),
        count(&fx.env, "jobs"),
    );

    let mut spec = spec_json();
    spec["goal"] = json!(format!("{} is the other checkout.", second.display()));
    let spec = write_spec(&fx.env, "second.json", &spec.to_string());
    let run = fx.cpd(&[
        "job",
        "add",
        "a",
        "--title",
        "A",
        "--tier",
        "S2",
        "--spec",
        &spec,
        "--workflow",
        "repo",
        "--plan",
        "pl",
    ]);
    let error = refused(&run, "spec_invalid", 2);
    assert_eq!(error["details"]["path"].as_str(), Some("$.goal"));
    assert_eq!(error["details"]["primary"].as_str(), second.to_str());
    assert_eq!(error["details"]["pr"].as_str(), Some("two"));
    assert_eq!(
        (
            count(&fx.env, "txns"),
            count(&fx.env, "events"),
            count(&fx.env, "jobs")
        ),
        writes
    );
}

#[test]
fn job_edit_refuses_a_spec_that_names_a_primary_checkout() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let root = primary(&fx.env, fx.env.root().join("repos/primary"));
    job_add(&fx, "pl", "a", "S2", &[]).ok();
    let writes = (count(&fx.env, "txns"), count(&fx.env, "events"));

    let mut bad = spec_json();
    bad["goal"] = json!(format!("{root}/docs/x.md"));
    let bad = write_spec(&fx.env, "bad.json", &bad.to_string());
    let error = refused(
        &job(&fx, &["edit", "a", "--spec", &bad], "pl"),
        "spec_invalid",
        2,
    );
    assert_eq!(error["details"]["path"].as_str(), Some("$.goal"));
    assert_eq!(error["details"]["primary"].as_str(), Some(root.as_str()));
    assert_eq!(error["details"]["pr"].as_str(), Some("repo"));
    assert_eq!(job_row(&fx.env, "pl", "a", "goal"), "Do it.");
    assert_eq!(job_row(&fx.env, "pl", "a", "context"), "Because.");
    assert_eq!((count(&fx.env, "txns"), count(&fx.env, "events")), writes);

    // A clean Spec edit works.
    let mut clean = spec_json();
    clean["goal"] = json!("Do it well.");
    let clean = write_spec(&fx.env, "clean.json", &clean.to_string());
    let out = job(&fx, &["edit", "a", "--spec", &clean], "pl").ok();
    assert_eq!(out["fields"], json!(["goal"]));
    assert_eq!(job_row(&fx.env, "pl", "a", "goal"), "Do it well.");
}

#[test]
fn a_stub_draft_and_a_stored_job_are_not_checked_for_a_primary_checkout() {
    let fx = Fx::new();
    setup(&fx.env);
    plan(&fx.env, "pl", "plan");
    let root = primary(&fx.env, fx.env.root().join("repos/primary"));

    // A stub draft needs only a title.
    fx.cpd(&[
        "job", "add", "stub", "--title", "Stub", "--draft", "--plan", "pl",
    ])
    .ok();
    assert_eq!(job_row(&fx.env, "pl", "stub", "goal"), Value::Null);

    // A draft whose stored Spec names the root (a fixture row) can still be edited with `--title` and cancelled.
    fixture(
        &fx.env,
        &format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, created_txn, created_at)
             VALUES ('pl', 'repo', 'stored', 'Stored', 'S2', '{root}/docs/x.md', 'c', '[\"docs/x.md\"]', '[\"a\"]', '[]', 'draft', 0, 'work', cpd_txn(), 't');"
        ),
    );
    let out = job(&fx, &["edit", "stored", "--title", "Renamed"], "pl").ok();
    assert_eq!(out["fields"], json!(["title"]));
    assert_eq!(job_row(&fx.env, "pl", "stored", "title"), "Renamed");
    job(
        &fx,
        &["cancel", "stored", "--reason", "No longer needed."],
        "pl",
    )
    .ok();
    assert_eq!(job_row(&fx.env, "pl", "stored", "status"), "cancelled");
}
