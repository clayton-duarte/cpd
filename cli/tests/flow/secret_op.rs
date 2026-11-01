//! The op calls, the use of the grants and the output of `cpd secret` (commands.md §24.1-§24.7, contract §8.4),
//! driving the built `cpd` against a fake `op`. Real op output is unverified: the shapes are those of the docs.

use crate::common::Run;
use crate::common::fx::{Fx, bare_with_store};
use crate::secret::{Who, approve_args, args, denied, strs};

use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::time::Duration;

use cpd_core::host::runner::Reply;
use rusqlite::types::ValueRef;
use serde_json::{Value, json};

const ACCOUNT: &str = "--account my.1password.com";
const NO_ITEM: &str =
    "[ERROR] 2026/10/22 10:00:00 \"deployment\" isn't an item in the \"cpd\" vault\n";

/// Context `ctx`; Plan `pl` (Lead `lead1`, Manager `mgr1`) with the Jobs `a` (Attempt `a-a1`, running, Worker `w-a`)
/// and `e` (Attempt `e-a1`, ended); Plan `pl2` (Lead `lead2`) with Job `b` (Attempt `b-a1`, running): what
/// `secret::Fx::new` seeds (secret.rs, not this Job's file), over an env that starts no process. `op` is the
/// scripted route of the `FakeRunner` (`Fx::op_says`): no `op` binary, no script.
fn fixture() -> Fx {
    let env = bare_with_store();
    env.write_config("version = 1\nmachine = \"m1\"\n");
    let fp = "0".repeat(64);
    let job = |plan: &str, key: &str, pr: &str| {
        format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
               VALUES ('{plan}', '{pr}', '{key}', 'J', 'S2', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'draft', 0, NULL, cpd_txn(), 't');
             UPDATE jobs SET status = 'queued', fingerprint = '{fp}' WHERE key = '{key}';
             UPDATE jobs SET status = 'running' WHERE key = '{key}';
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
               VALUES ('{plan}', '{key}-a1', '{key}', 1, 'S2', 'ladder', 'running', 'cpd/{plan}--{key}-a1', '/w', 'w-{key}', 't');
             INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
               VALUES ('w-{key}', 'worker', 'ctx', '{plan}', '{key}-a1', '{key}-a1', '/w', 'S2', 'working', 't', 't');"
        )
    };
    env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'C', NULL, 't', 'm1');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pl', 'ctx', 'P', 'grounding', 'lead1', 't'), ('pl2', 'ctx', 'P2', 'grounding', 'lead2', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pl', 'L', '/', 'O3', 'idle', 't', 't'),
                  ('lead2', 'lead', 'ctx', 'pl2', 'L2', '/', 'O3', 'idle', 't', 't');
         INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
           VALUES ('mgr1', 'manager', 'ctx', 'M', '/', 'O3', 'idle', 't', 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, name, created_txn, created_at)
           VALUES ('pl', 'p1', '/r', 'main', 'waiting', 'cpd/pl/p1', NULL, cpd_txn(), 't'),
                  ('pl2', 'p2', '/r', 'main', 'waiting', 'cpd/pl2/p2', NULL, cpd_txn(), 't');
         {} {} {}",
        job("pl", "a", "p1"),
        job("pl", "e", "p1"),
        job("pl2", "b", "p2")
    ));
    env.sql("UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = 't' WHERE id = 'e-a1';");
    let fx = Fx::with_ports_over(env);
    fx.op();
    fx
}

/// `cpd <args>` as `who`, with `stdin` (none: nothing is read).
fn run(fx: &Fx, who: Who, args: &[&str], stdin: Option<&str>) -> Run {
    let vars: &[(&str, &str)] = match who {
        Who::Nobody => &[],
        Who::Empty => &[("CPD_SESSION", "")],
        Who::Unknown => &[("CPD_SESSION", "no-such-session")],
        Who::Lead => &[("CPD_SESSION", "lead1")],
        Who::Manager => &[("CPD_SESSION", "mgr1")],
        Who::Worker => &[
            ("CPD_SESSION", "w-a"),
            ("CPD_ATTEMPT", "a-a1"),
            ("CPD_PLAN", "pl"),
        ],
    };
    match stdin {
        None => fx.cpd_with(fx.env.root(), args, vars),
        Some(text) => fx.cpd_stdin(args, vars, text),
    }
}

fn secret(
    fx: &Fx,
    who: Who,
    command: &str,
    vault: &str,
    item: &str,
    field: &str,
    stdin: Option<&str>,
) -> Run {
    run(fx, who, &strs(&args(command, vault, item, field)), stdin)
}

fn assert_op_not_called(fx: &Fx, r: &Run) {
    assert_eq!(fx.op_log(), Vec::<String>::new(), "op was called: {r:?}");
}

fn item_json() -> Value {
    json!({
        "id": "abc123",
        "title": "deployment",
        "category": "SECURE_NOTE",
        "fields": [
            {"id": "password", "label": "password", "type": "CONCEALED", "value": "old"},
            {"id": "user", "label": "user", "type": "STRING", "value": "bob"}
        ]
    })
}

fn approvals(fx: &Fx) -> Vec<PathBuf> {
    match fs::read_dir(fx.env.cpd_home().join("run/secret/approve")) {
        Ok(entries) => entries.map(|e| e.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    }
}

#[test]
fn get_reads_one_field_and_pins_the_account() {
    let fx = fixture();
    fx.op_says("get", "hunter2\n\n", "", 0);
    let r = secret(&fx, Who::Lead, "get", "cpd", "deployment", "password", None);
    let out = r.ok();
    assert_eq!(out["vault"], "cpd", "{r:?}");
    assert_eq!(out["item"], "deployment", "{r:?}");
    assert_eq!(out["field"], "password", "{r:?}");
    assert_eq!(out["value"], "hunter2\n", "{r:?}");
    let log = fx.op_log();
    assert_eq!(log.len(), 1, "{log:?}");
    for part in [
        ACCOUNT,
        "item",
        "get",
        "deployment",
        "--vault cpd",
        "--fields label=password",
        "--reveal",
    ] {
        assert!(log[0].contains(part), "{part} not in {}", log[0]);
    }
    assert!(!log[0].contains("--session"), "{}", log[0]);
}

#[test]
fn list_returns_ids_and_titles_only() {
    let fx = fixture();
    fx.op_says(
        "list",
        r#"[{"id":"abc123","title":"deployment","version":2,"vault":{"id":"v1","name":"cpd"},"category":"SECURE_NOTE","fields":[{"value":"leak"}]},
            {"id":"xyz789","title":"slack-webhook","version":1,"category":"SECURE_NOTE"}]"#,
        "",
        0,
    );
    let r = run(&fx, Who::Lead, &strs(&args("list", "cpd", "", "")), None);
    let out = r.ok();
    assert_eq!(
        out["items"],
        json!([{"id": "abc123", "title": "deployment"}, {"id": "xyz789", "title": "slack-webhook"}]),
        "{r:?}"
    );
    assert!(!r.stdout.contains("leak"), "{r:?}");
    let log = fx.op_log();
    assert_eq!(log.len(), 1, "{log:?}");
    for part in [ACCOUNT, "item list", "--vault cpd", "--format json"] {
        assert!(log[0].contains(part), "{part} not in {}", log[0]);
    }
}

#[test]
fn create_sends_a_template_on_stdin_and_never_the_value_in_argv() {
    let fx = fixture();
    fx.op_says("get", "", NO_ITEM, 1);
    fx.op_says("create", "{\"id\":\"new1\"}\n", "", 0);
    let value = "pa ss\"word";
    let r = secret(
        &fx,
        Who::Lead,
        "create",
        "cpd",
        "deployment",
        "password",
        Some(&format!("{value}\n")),
    );
    let out = r.ok();
    assert_eq!(out["created"], true, "{r:?}");
    let log = fx.op_log();
    assert_eq!(log.len(), 2, "{log:?}");
    for part in [
        ACCOUNT,
        "item get deployment",
        "--vault cpd",
        "--format json",
    ] {
        assert!(log[0].contains(part), "{part} not in {}", log[0]);
    }
    for part in [ACCOUNT, "item create", "--vault cpd -"] {
        assert!(log[1].contains(part), "{part} not in {}", log[1]);
    }
    let sent: Value = serde_json::from_str(&fx.op_stdin("create")).expect("the template is JSON");
    assert_eq!(
        sent,
        json!({"title": "deployment", "category": "SECURE_NOTE",
               "fields": [{"id": "password", "label": "password", "type": "CONCEALED", "value": value}]})
    );
    for line in &log {
        assert!(!line.contains("pa ss"), "the value is in argv: {line}");
    }
    assert!(
        !r.stdout.contains("pa ss") && !r.stderr.contains("pa ss"),
        "{r:?}"
    );
}

#[test]
fn create_refuses_an_existing_item() {
    let fx = fixture();
    fx.op_says("get", &item_json().to_string(), "", 0);
    let r = secret(
        &fx,
        Who::Lead,
        "create",
        "cpd",
        "deployment",
        "password",
        Some("v\n"),
    );
    denied(&r, "secret_exists", 3);
    let log = fx.op_log();
    assert_eq!(log.len(), 1, "{log:?}");
    assert!(!log.iter().any(|l| l.contains("item create")), "{log:?}");
}

#[test]
fn edit_updates_the_field_in_a_template_on_stdin() {
    let fx = fixture();
    fx.op_says("get", &item_json().to_string(), "", 0);
    fx.op_says("edit", "", "", 0);
    let r = secret(
        &fx,
        Who::Lead,
        "edit",
        "cpd",
        "deployment",
        "password",
        Some("brand new\n"),
    );
    assert_eq!(r.ok()["edited"], true, "{r:?}");
    let log = fx.op_log();
    assert_eq!(log.len(), 2, "{log:?}");
    for part in [ACCOUNT, "item edit deployment", "--vault cpd"] {
        assert!(log[1].contains(part), "{part} not in {}", log[1]);
    }
    let mut expected = item_json();
    expected["fields"][0]["value"] = json!("brand new");
    let sent: Value = serde_json::from_str(&fx.op_stdin("edit")).expect("the edited item is JSON");
    assert_eq!(sent, expected);
    assert!(
        !log.iter().any(|l| l.contains("brand new")),
        "the value is in argv: {log:?}"
    );
    assert!(
        !r.stdout.contains("brand new") && !r.stderr.contains("brand new"),
        "{r:?}"
    );
}

#[test]
fn edit_adds_a_missing_field() {
    let fx = fixture();
    fx.op_says("get", &item_json().to_string(), "", 0);
    fx.op_says("edit", "", "", 0);
    let r = secret(
        &fx,
        Who::Lead,
        "edit",
        "cpd",
        "deployment",
        "token",
        Some("t0k\n"),
    );
    assert_eq!(r.ok()["edited"], true, "{r:?}");
    let sent: Value = serde_json::from_str(&fx.op_stdin("edit")).expect("the edited item is JSON");
    let fields = sent["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 3, "{sent}");
    assert_eq!(fields[0], item_json()["fields"][0]);
    assert_eq!(fields[1], item_json()["fields"][1]);
    assert_eq!(fields[2]["label"], "token");
    assert_eq!(fields[2]["type"], "CONCEALED");
    assert_eq!(fields[2]["value"], "t0k");
}

#[test]
fn edit_of_a_missing_item_is_not_found() {
    let fx = fixture();
    fx.op_says("get", "", NO_ITEM, 1);
    let r = secret(
        &fx,
        Who::Lead,
        "edit",
        "cpd",
        "deployment",
        "password",
        Some("v\n"),
    );
    denied(&r, "secret_not_found", 4);
    let log = fx.op_log();
    assert_eq!(log.len(), 1, "{log:?}");
    assert!(!log.iter().any(|l| l.contains("item edit")), "{log:?}");
}

#[test]
fn delete_archives_the_item() {
    let fx = fixture();
    let r = run(
        &fx,
        Who::Lead,
        &strs(&args("delete", "cpd", "deployment", "")),
        None,
    );
    assert_eq!(r.ok()["archived"], true, "{r:?}");
    let log = fx.op_log();
    assert_eq!(log.len(), 1, "{log:?}");
    for part in [
        ACCOUNT,
        "item delete deployment",
        "--vault cpd",
        "--archive",
    ] {
        assert!(log[0].contains(part), "{part} not in {}", log[0]);
    }
}

#[test]
fn a_missing_item_is_not_found() {
    let fx = fixture();
    for stderr in [
        NO_ITEM,
        "[ERROR] 2026/10/22 10:00:00 \"password\" isn't a field in the \"deployment\" item\n",
    ] {
        fx.op_says("get", "", stderr, 1);
        let r = secret(&fx, Who::Lead, "get", "cpd", "deployment", "password", None);
        denied(&r, "secret_not_found", 4);
    }
}

#[test]
fn not_signed_in_is_its_own_code() {
    let fx = fixture();
    fx.op_says(
        "get",
        "",
        "[ERROR] 2026/10/22 10:00:00 account is not signed in\n",
        1,
    );
    let r = secret(&fx, Who::Lead, "get", "cpd", "deployment", "password", None);
    let error = denied(&r, "secret_not_signed_in", 3);
    let message = error["message"].as_str().unwrap().to_lowercase();
    assert!(message.contains("sign") && message.contains("op"), "{r:?}");
}

#[test]
fn another_op_failure_keeps_its_stderr() {
    let fx = fixture();
    fx.op_says("get", "", "[ERROR] something odd happened\n", 7);
    let r = secret(&fx, Who::Lead, "get", "cpd", "deployment", "password", None);
    let error = denied(&r, "secret_op_failed", 1);
    assert_eq!(error["details"]["exit"], 7, "{r:?}");
    assert!(
        error["details"]["stderr"]
            .to_string()
            .contains("something odd happened"),
        "{r:?}"
    );
}

#[test]
fn a_missing_op_is_secret_op_missing() {
    let fx = fixture();
    // No `op` on PATH: the runner cannot start the program, as the system's does not find it.
    fx.fake_op(Reply::io(ErrorKind::NotFound));
    let r = secret(&fx, Who::Lead, "get", "cpd", "deployment", "password", None);
    denied(&r, "secret_op_missing", 1);
    assert_op_not_called(&fx, &r);
}

#[test]
fn a_manager_writes_in_homeserver() {
    let fx = fixture();
    fx.op_says("get", "", NO_ITEM, 1);
    let r = secret(
        &fx,
        Who::Manager,
        "create",
        "homeserver",
        "token",
        "password",
        Some("v1\n"),
    );
    assert_eq!(r.ok()["created"], true, "{r:?}");
    assert!(
        fx.op_log()
            .last()
            .unwrap()
            .contains("item create --vault homeserver -"),
        "{:?}",
        fx.op_log()
    );

    fx.op_says("get", &item_json().to_string(), "", 0);
    let r = secret(
        &fx,
        Who::Manager,
        "edit",
        "homeserver",
        "token",
        "password",
        Some("v2\n"),
    );
    assert_eq!(r.ok()["edited"], true, "{r:?}");
    assert!(
        fx.op_log()
            .last()
            .unwrap()
            .contains("item edit token --vault homeserver"),
        "{:?}",
        fx.op_log()
    );

    let r = run(
        &fx,
        Who::Manager,
        &strs(&args("delete", "homeserver", "token", "")),
        None,
    );
    assert_eq!(r.ok()["archived"], true, "{r:?}");
    assert!(
        fx.op_log()
            .last()
            .unwrap()
            .contains("item delete token --vault homeserver --archive"),
        "{:?}",
        fx.op_log()
    );
}

#[test]
fn an_approval_runs_one_command_once() {
    let fx = fixture();
    fx.op_says("get", "v\n", "", 0);
    let get = || {
        secret(
            &fx,
            Who::Lead,
            "get",
            "other",
            "deployment",
            "password",
            None,
        )
    };
    let r = get();
    denied(&r, "secret_grant_required", 3);
    assert_op_not_called(&fx, &r);

    let a = approve_args("get", "other", Some("deployment"), Some("password"));
    run(&fx, Who::Lead, &strs(&a), None).ok();
    assert_eq!(approvals(&fx).len(), 1);

    let r = get();
    assert_eq!(r.ok()["value"], "v", "{r:?}");
    assert!(approvals(&fx).is_empty());
    assert!(
        fx.op_log()[0].contains("--vault other"),
        "{:?}",
        fx.op_log()
    );

    let r = get();
    denied(&r, "secret_grant_required", 3);
    assert_eq!(fx.op_log().len(), 1, "{:?}", fx.op_log());
}

#[test]
fn an_approval_is_bound_to_its_session_and_arguments_and_expires() {
    let fx = fixture();
    fx.op_says("get", "v\n", "", 0);
    let a = approve_args("get", "other", Some("deployment"), Some("password"));
    run(&fx, Who::Lead, &strs(&a), None).ok();
    let files = approvals(&fx);
    assert_eq!(files.len(), 1);

    let refused = |who: Who, command: &str, vault: &str, item: &str, field: &str| {
        let r = run(
            &fx,
            who,
            &strs(&args(command, vault, item, field)),
            if command == "get" { None } else { Some("v\n") },
        );
        denied(&r, "secret_grant_required", 3);
        assert_eq!(
            approvals(&fx),
            files,
            "the approval was used or removed: {r:?}"
        );
        assert_op_not_called(&fx, &r);
    };
    refused(Who::Manager, "get", "other", "deployment", "password");
    refused(Who::Lead, "get", "other", "another-item", "password");
    refused(Who::Lead, "get", "other", "deployment", "another-field");
    refused(Who::Lead, "get", "other2", "deployment", "password");
    refused(Who::Lead, "list", "other", "", "");
    refused(Who::Lead, "delete", "other", "deployment", "");

    // The approval is old: virtual time passes the lifetime of an approval (10 minutes; it was written at the clock's time).
    fx.advance(Duration::from_secs(11 * 60));
    let r = secret(
        &fx,
        Who::Lead,
        "get",
        "other",
        "deployment",
        "password",
        None,
    );
    denied(&r, "secret_grant_required", 3);
    assert!(approvals(&fx).is_empty(), "the expired approval stays");
    assert_op_not_called(&fx, &r);
}

#[test]
fn an_allowance_lasts_until_the_attempt_ends() {
    let fx = fixture();
    fx.op_says("get", "v\n", "", 0);
    run(
        &fx,
        Who::Lead,
        &strs(&args("allow", "cpd", "deployment", "password")),
        None,
    )
    .ok();
    let file = fx.env.cpd_home().join("run/secret/allow/a-a1.json");

    for _ in 0..2 {
        let r = secret(
            &fx,
            Who::Worker,
            "get",
            "cpd",
            "deployment",
            "password",
            None,
        );
        assert_eq!(r.ok()["value"], "v", "{r:?}");
        assert!(file.exists());
    }
    assert_eq!(fx.op_log().len(), 2);

    for (vault, item, field) in [
        ("cpd", "other-item", "password"),
        ("cpd", "deployment", "other-field"),
        ("homeserver", "deployment", "password"),
    ] {
        let r = secret(&fx, Who::Worker, "get", vault, item, field, None);
        denied(&r, "secret_grant_required", 3);
    }
    assert_eq!(fx.op_log().len(), 2);

    fx.env.sql("UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'crashed', ended_at = 't' WHERE id = 'a-a1';");
    let r = secret(
        &fx,
        Who::Worker,
        "get",
        "cpd",
        "deployment",
        "password",
        None,
    );
    denied(&r, "secret_grant_required", 3);
    assert_eq!(fx.op_log().len(), 2);
}

#[test]
fn secret_commands_write_nothing_to_the_store() {
    let fx = fixture();
    let count = |t: &str| fx.env.query::<i64>(&format!("SELECT count(*) FROM {t}"));
    let before: Vec<i64> = ["events", "txns", "history"]
        .iter()
        .map(|t| count(t))
        .collect();
    let value = "sup3r-s3cret-value";
    let stdin = format!("{value}\n");

    fx.op_says("get", "", NO_ITEM, 1);
    secret(
        &fx,
        Who::Lead,
        "create",
        "cpd",
        "deployment",
        "password",
        Some(&stdin),
    )
    .ok();
    fx.op_says("get", &item_json().to_string(), "", 0);
    secret(
        &fx,
        Who::Lead,
        "edit",
        "cpd",
        "deployment",
        "password",
        Some(&stdin),
    )
    .ok();
    fx.op_says("get", &format!("{value}\n"), "", 0);
    let r = secret(&fx, Who::Lead, "get", "cpd", "deployment", "password", None);
    assert_eq!(r.ok()["value"], value, "{r:?}");
    fx.op_says("list", "[{\"id\":\"a\",\"title\":\"deployment\"}]", "", 0);
    run(&fx, Who::Lead, &strs(&args("list", "cpd", "", "")), None).ok();
    run(
        &fx,
        Who::Lead,
        &strs(&args("delete", "cpd", "deployment", "")),
        None,
    )
    .ok();
    run(
        &fx,
        Who::Lead,
        &strs(&args("allow", "cpd", "deployment", "password")),
        None,
    )
    .ok();
    run(
        &fx,
        Who::Lead,
        &strs(&approve_args(
            "get",
            "other",
            Some("deployment"),
            Some("password"),
        )),
        None,
    )
    .ok();

    let after: Vec<i64> = ["events", "txns", "history"]
        .iter()
        .map(|t| count(t))
        .collect();
    assert_eq!(before, after);
    // Every table of the store, through the test's own read handle: the secret is in no row of any of them.
    let leaked = fx.env.read(|conn| {
        let tables: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            )?
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        let mut leaked = Vec::new();
        for table in tables {
            let mut statement = conn.prepare(&format!("SELECT * FROM \"{table}\""))?;
            let columns = statement.column_count();
            let mut rows = statement.query([])?;
            while let Some(row) = rows.next()? {
                for column in 0..columns {
                    let text = match row.get_ref(column)? {
                        ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
                            String::from_utf8_lossy(bytes).into_owned()
                        }
                        other => format!("{other:?}"),
                    };
                    if text.contains(value) {
                        leaked.push(format!("{table}.{column}"));
                    }
                }
            }
        }
        Ok(leaked)
    });
    assert!(leaked.is_empty(), "the secret is in the store: {leaked:?}");
}
