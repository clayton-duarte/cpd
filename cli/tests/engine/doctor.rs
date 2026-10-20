//! `cpd doctor` (commands.md §14.1): the machine's checks, each against fake
//! tools on a `PATH` that holds only the test's `bin/`.

use crate::common;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use common::{CPD, Run, TestEnv, write_executable};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use nix::fcntl::{Flock, FlockArg};
use rusqlite::Connection;
use serde_json::{Value, json};

const NAMES: [&str; 13] = [
    "config",
    "store",
    "tamper",
    "git",
    "tmux",
    "pi",
    "gh",
    "gh_auth",
    "pi_node",
    "tiers",
    "pi_adapter",
    "lead_packages",
    "watch",
];

const CODING_AGENT: &str = r#"
export const ModelRuntime = {
  async create(options) {
    if (options.allowModelNetwork !== false) throw new Error("network must be off");
    return { getModel: (provider, id) => (provider === "p" && id.startsWith("m-") ? { provider, id } : undefined) };
  },
};
"#;
const PI_AI: &str = r#"
export function getSupportedThinkingLevels(model) {
  return model.id === "m-low" ? ["off", "minimal", "low", "medium"] : ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
}
"#;

fn real_node() -> std::path::PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .map(|d| d.join("node"))
        .find(|p| p.is_file())
        .expect("node is on PATH: the Gate needs it for the TypeScript checks")
}

fn write_config(env: &TestEnv, extra: &str) {
    let adapter = env.root().join("adapter");
    fs::create_dir_all(&adapter).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    let text = format!(
        "version = 1\nmachine = \"hs\"\npi_adapter = \"{}\"\n{extra}\n[tiers]\nO = \"p/m-opus\"\nS = \"p/m-sonnet\"\nH = \"p/m-low\"\n",
        adapter.display()
    );
    // `extra` holds top-level keys, so it must come before the `[tiers]` table.
    env.write_config(&text);
}

/// A fake pi install whose agent directory is the test root: `bin/pi` answering
/// `--version`, and a release with the two entry points the catalog module imports.
fn install_pi(env: &TestEnv, version: &str) {
    env.fake_pi(&format!("echo '{version}'"));
    fs::create_dir_all(env.root().join("install")).unwrap();
    fs::write(env.root().join("install/current-version"), "1.2.0\n").unwrap();
    let scope = env.root().join("install/releases/1.2.0/node_modules/@earendil-works");
    for (package, source) in [("pi-coding-agent", CODING_AGENT), ("pi-ai", PI_AI)] {
        let dist = scope.join(package).join("dist");
        fs::create_dir_all(&dist).unwrap();
        fs::write(scope.join(package).join("package.json"), "{\"type\": \"module\"}").unwrap();
        fs::write(dist.join("index.js"), source).unwrap();
    }
}

/// A machine where every check passes.
fn healthy() -> TestEnv {
    healthy_in(TestEnv::new())
}

/// [`healthy`] in `env`.
fn healthy_in(env: TestEnv) -> TestEnv {
    write_config(&env, "");
    healthy_tools(&env);
    symlink(real_node(), env.bin().join("node")).unwrap();
    env
}

/// The fake git, tmux, gh and pi of [`healthy`], (re)installed in `env`.
fn healthy_tools(env: &TestEnv) {
    env.fake("git", "echo 'git version 2.43.0'");
    env.fake("tmux", "case \" $* \" in *\" -V \"*) echo 'tmux 3.6';; esac; exit 0");
    env.fake_gh("[ \"$1\" = auth ] && exit 0; echo 'gh version 2.45.0 (2025-07-18 Ubuntu 2.45.0-1ubuntu0.3)'");
    install_pi(env, "1.2.0");
}

/// `cpd doctor` with `PATH` = the test's `bin/` only, and `vars` added.
fn doctor_with(env: &TestEnv, vars: &[(&str, &str)]) -> Run {
    let mut cmd = env.command(CPD);
    cmd.arg("doctor").env("PATH", env.bin()).env_remove("PLANNOTATOR_PORT").env_remove("PLANNOTATOR_REMOTE");
    cmd.envs(vars.iter().copied());
    let out = cmd.output().expect("run cpd");
    Run {
        exit: out.status.code().expect("exit code"),
        stdout: String::from_utf8(out.stdout).expect("UTF-8"),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn doctor(env: &TestEnv) -> Run {
    doctor_with(env, &[])
}

/// The checks of a success, or of a `doctor_failed`.
fn checks(run: &Run) -> Vec<Value> {
    let json = run.json();
    let list = if json["ok"] == true { &json["checks"] } else { &json["error"]["details"]["checks"] };
    list.as_array().unwrap_or_else(|| panic!("no checks in {run:?}")).clone()
}

fn check(run: &Run, name: &str) -> Value {
    checks(run).into_iter().find(|c| c["name"] == name).unwrap_or_else(|| panic!("no check {name} in {run:?}"))
}

fn assert_passes(run: &Run, name: &str) {
    let c = check(run, name);
    assert_eq!(c["ok"], true, "{c}");
}

/// A failed check: its severity is as given and its message contains `message`.
fn assert_fails(run: &Run, name: &str, severity: &str, message: &str) -> Value {
    let c = check(run, name);
    assert_eq!(c["ok"], false, "{c}");
    assert_eq!(c["severity"], severity, "{c}");
    assert!(c["message"].as_str().unwrap_or_default().contains(message), "{message:?} not in {c}");
    c
}

/// The result is `doctor_failed` (exit 1) with every check in `details.checks`.
fn assert_doctor_failed(run: &Run) {
    assert_eq!(run.exit, 1, "{run:?}");
    assert_eq!(run.error()["code"], "doctor_failed", "{run:?}");
    let names: Vec<_> = checks(run).iter().map(|c| c["name"].as_str().unwrap().to_owned()).collect();
    assert_eq!(names, NAMES);
}

fn open_store(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

// ------------------------------------------------------------------ success

#[test]
fn a_healthy_machine_passes_every_check_in_order() {
    let env = healthy();
    let run = doctor(&env);
    let json = run.ok();
    let all = checks(&run);
    let names: Vec<_> = all.iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, NAMES);
    for c in &all {
        assert_eq!(c["ok"], true, "{c}");
        assert!(["error", "warn"].contains(&c["severity"].as_str().unwrap()), "{c}");
    }
    assert_eq!(json.as_object().unwrap().len(), 2, "only ok and checks: {json}");

    let tool = |name: &str, found: &str, expected: &str| {
        let c = check(&run, name);
        assert_eq!((c["found"].as_str(), c["expected"].as_str()), (Some(found), Some(expected)), "{c}");
        assert_eq!(c["severity"], "error");
    };
    tool("git", "2.43.0", ">= 2.43.0");
    tool("tmux", "3.6", ">= 3.6");
    tool("pi", "1.2.0", ">= 1.2.0 < 2.0.0");
    tool("gh", "2.45.0", ">= 2.45.0");
    for warn in ["gh_auth", "lead_packages", "watch"] {
        assert_eq!(check(&run, warn)["severity"], "warn", "{warn}");
    }
}

#[test]
fn doctor_is_idempotent_and_writes_nothing_but_the_migration() {
    let env = healthy_in(TestEnv::empty());
    let first = doctor(&env);
    first.ok();
    let db = env.cpd_home().join("cpd.db");
    let txns = |env: &TestEnv| -> i64 {
        let conn = Connection::open(env.cpd_home().join("cpd.db")).unwrap();
        conn.query_row("SELECT count(*) FROM txns", [], |r| r.get(0)).unwrap()
    };
    assert!(db.exists());
    assert_eq!(txns(&env), 1, "the migration's transaction only");
    let again = doctor(&env);
    assert_eq!(again.stdout, first.stdout);
    assert_eq!(txns(&env), 1);
    // The tiers check asks pi and does not fill the cache.
    let conn = Connection::open(db).unwrap();
    let cached: i64 = conn.query_row("SELECT count(*) FROM tier_catalog", [], |r| r.get(0)).unwrap();
    assert_eq!(cached, 0);
}

#[test]
fn an_argument_is_usage() {
    let env = healthy();
    let out = env.command(CPD).args(["doctor", "--bogus"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

// ------------------------------------------------------------------- config

#[test]
fn a_missing_config_fails_and_what_needs_it_is_not_checked() {
    let env = healthy();
    fs::remove_file(env.cpd_home().join("config.toml")).unwrap();
    let run = doctor(&env);
    assert_doctor_failed(&run);
    assert_fails(&run, "config", "error", "config_missing");
    for name in ["store", "tiers", "pi_adapter"] {
        assert_fails(&run, name, "error", "not checked");
    }
    assert_fails(&run, "lead_packages", "warn", "not checked");
    assert_passes(&run, "git");
}

#[test]
fn an_invalid_config_fails_with_its_key() {
    let env = healthy();
    fs::write(env.cpd_home().join("config.toml"), "version = 1\nmachine = \"hs\"\nconcurrency = 0\n").unwrap();
    let run = doctor(&env);
    assert_doctor_failed(&run);
    let c = assert_fails(&run, "config", "error", "config_invalid");
    assert_eq!(c["found"]["key"], "concurrency");
}

// -------------------------------------------------------------------- tools

#[test]
fn each_tool_must_be_at_or_above_its_minimum() {
    // (tool, version output, passes, found)
    let cases = [
        ("git", "git version 2.42.9", false, "2.42.9"),
        ("git", "git version 2.43.0", true, "2.43.0"),
        ("git", "git version 2.50.1 (Apple Git-155)", true, "2.50.1"),
        ("tmux", "tmux 3.3a", false, "3.3"),
        ("tmux", "tmux 3.4", false, "3.4"),
        ("tmux", "tmux 3.5a", false, "3.5"),
        ("tmux", "tmux 3.6", true, "3.6"),
        ("tmux", "tmux 3.8", true, "3.8"),
        ("tmux", "tmux next-3.9", true, "next-3.9"),
        ("pi", "1.1.9", false, "1.1.9"),
        ("pi", "1.9.9", true, "1.9.9"),
        ("pi", "2.0.0", false, "2.0.0"),
        ("gh", "gh version 2.44.9 (2025-01-01)", false, "2.44.9"),
        ("gh", "gh version 2.46.0 (2025-01-01)", true, "2.46.0"),
    ];
    // One env: each case is the healthy machine with one tool changed (doctor runs every tool afresh).
    let env = healthy();
    for (tool, output, passes, found) in cases {
        healthy_tools(&env);
        if tool == "pi" {
            install_pi(&env, output);
        } else {
            env.fake(tool, &format!("[ \"$1\" = auth ] && exit 0; echo '{output}'"));
        }
        let run = doctor(&env);
        let c = check(&run, tool);
        assert_eq!(c["ok"], passes, "{tool} {output:?}: {c}");
        assert_eq!(c["found"], found, "{tool} {output:?}: {c}");
        assert_eq!(c["severity"], "error");
        if passes {
            run.ok();
        } else {
            assert_doctor_failed(&run);
            assert!(c["message"].is_string(), "{c}");
        }
    }
}

#[test]
fn a_tool_that_is_not_on_path_fails() {
    let env = healthy();
    for tool in ["git", "tmux", "pi", "gh"] {
        healthy_tools(&env);
        fs::remove_file(env.bin().join(tool)).unwrap();
        let run = doctor(&env);
        assert_doctor_failed(&run);
        let c = assert_fails(&run, tool, "error", "not found");
        assert!(c["expected"].is_string(), "{c}");
    }
}

#[test]
fn a_tool_that_prints_no_version_or_fails_is_a_failed_check() {
    let env = healthy();
    env.fake("git", "echo 'git version unknown'");
    env.fake("tmux", "echo 'boom' >&2; exit 1");
    let run = doctor(&env);
    assert_doctor_failed(&run);
    assert_fails(&run, "git", "error", "no version");
    assert_fails(&run, "tmux", "error", "exited 1");
}

#[test]
fn gh_auth_is_a_warning_that_never_fails_doctor() {
    let env = healthy();
    env.fake_gh("[ \"$1\" = auth ] && { echo 'not logged in' >&2; exit 1; }; echo 'gh version 2.45.0 (2025-07-18)'");
    let run = doctor(&env);
    run.ok();
    assert_fails(&run, "gh_auth", "warn", "exited 1");
    assert_passes(&run, "gh");
    assert_eq!(env.calls("gh").iter().filter(|c| c.as_slice() == ["auth", "status"]).count(), 1);
}

#[test]
fn pi_node_must_run() {
    let env = healthy();
    fs::remove_file(env.bin().join("node")).unwrap();
    let run = doctor(&env);
    assert_doctor_failed(&run);
    assert_fails(&run, "pi_node", "error", "not found");

    // pi's own Node wins over the one on PATH, and its failure is the check's.
    let env = healthy();
    let pi_node = env.home().join(".local/share/pi-node/current/bin");
    fs::create_dir_all(&pi_node).unwrap();
    write_executable(&pi_node.join("node"), "#!/bin/sh\necho 'node broke' >&2; exit 7\n");
    let run = doctor(&env);
    assert_doctor_failed(&run);
    assert_fails(&run, "pi_node", "error", "exited 7");
}

// ------------------------------------------------------------ pi and tiers

#[test]
fn tiers_fail_for_a_model_pi_does_not_have() {
    let env = healthy();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!("version = 1\nmachine = \"hs\"\npi_adapter = \"{}\"\n[tiers]\nO = \"p/m-opus\"\nS = \"p/nope\"\n", env.root().join("adapter").display()),
    )
    .unwrap();
    let run = doctor(&env);
    assert_doctor_failed(&run);
    let c = assert_fails(&run, "tiers", "error", "tier_model_unknown");
    assert_eq!(c["found"]["missing"][0]["model"], "p/nope");
}

#[test]
fn tiers_fail_for_a_lead_tier_its_model_does_not_support() {
    let env = healthy();
    // H is p/m-low: efforts 0 to 2.
    write_config(&env, "lead_tier = \"H4\"");
    let run = doctor(&env);
    assert_doctor_failed(&run);
    assert_fails(&run, "tiers", "error", "tier_invalid");
}

#[test]
fn tiers_fail_when_pi_has_no_release() {
    let env = healthy();
    fs::remove_file(env.root().join("install/current-version")).unwrap();
    let run = doctor(&env);
    assert_doctor_failed(&run);
    assert_fails(&run, "tiers", "error", "pi_not_found");
}

#[test]
fn the_adapter_directory_needs_a_package_json() {
    // The default, beside the binary, does not exist in a build tree.
    let env = healthy();
    fs::write(env.cpd_home().join("config.toml"), "version = 1\nmachine = \"hs\"\n[tiers]\nO = \"p/m-opus\"\nS = \"p/m-sonnet\"\nH = \"p/m-low\"\n").unwrap();
    let run = doctor(&env);
    assert_doctor_failed(&run);
    let c = assert_fails(&run, "pi_adapter", "error", "package.json");
    assert!(c["found"].as_str().unwrap().ends_with("share/cpd/pi"), "{c}");

    // A directory removed after the config was written.
    let env = healthy();
    fs::remove_file(env.root().join("adapter/package.json")).unwrap();
    let run = doctor(&env);
    assert_doctor_failed(&run);
    assert_fails(&run, "config", "error", "package.json");
}

#[test]
fn lead_packages_say_how_a_lead_gets_each() {
    let env = healthy();
    let adapter = env.root().join("adapter").display().to_string();
    let settings = env.home().join(".pi/agent/settings.json");
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(&settings, r#"{"packages": ["npm:@plannotator/pi-extension@0.1.0", {"source": "npm:@juicesharp/rpiv-ask-user-question"}]}"#).unwrap();
    let run = doctor(&env);
    let c = check(&run, "lead_packages");
    assert_eq!(c["ok"], true, "{c}");
    let by_settings = json!({"by": "settings", "settings": settings.display().to_string()});
    assert_eq!(
        c["found"],
        json!([
            {"package": adapter, "by": "-e"},
            {"package": "npm:@plannotator/pi-extension@0.28.17", "by": by_settings["by"], "settings": by_settings["settings"]},
            {"package": "npm:@juicesharp/rpiv-ask-user-question@2.13.8", "by": by_settings["by"], "settings": by_settings["settings"]},
        ])
    );

    // Declared by the project settings of the current directory, relative paths from that file.
    let env = healthy();
    write_config(&env, "lead_pi_packages = [\"./local-pkg\"]");
    let project = env.root().join(".pi/settings.json");
    fs::create_dir_all(project.parent().unwrap()).unwrap();
    fs::write(&project, r#"{"packages": ["../local-pkg"]}"#).unwrap();
    let run = doctor(&env);
    let c = check(&run, "lead_packages");
    assert_eq!(c["ok"], true, "{c}");
    assert_eq!(c["found"][1], json!({"package": "./local-pkg", "by": "settings", "settings": project.display().to_string()}));
}

#[test]
fn an_entry_neither_declared_nor_resolvable_is_a_warning() {
    let env = healthy();
    write_config(&env, "lead_pi_packages = [\"/nonexistent/pi-package\", \"npm:left-pad@1.0.0\"]");
    let run = doctor(&env);
    run.ok();
    let c = assert_fails(&run, "lead_packages", "warn", "/nonexistent/pi-package");
    assert_eq!(c["found"][1], json!({"package": "/nonexistent/pi-package", "by": "-e", "resolvable": false}));
    assert_eq!(c["found"][2], json!({"package": "npm:left-pad@1.0.0", "by": "-e"}));
}

#[test]
fn doctor_has_no_plannotator_port_check() {
    let env = healthy();
    for vars in [&[][..], &[("PLANNOTATOR_REMOTE", "1")][..], &[("PLANNOTATOR_REMOTE", "1"), ("PLANNOTATOR_PORT", "19432")][..]] {
        let run = doctor_with(&env, vars);
        let checks = run.ok()["checks"].clone();
        assert!(checks.as_array().unwrap().iter().all(|c| c["name"] != "plannotator_port"), "{checks}");
    }
}

// -------------------------------------------------------------- the store

#[test]
fn a_changed_schema_fails_store_and_tamper() {
    let env = healthy();
    open_store(&env);
    Connection::open(env.cpd_home().join("cpd.db")).unwrap().execute_batch("DROP TRIGGER contexts_no_delete").unwrap();
    let run = doctor(&env);
    assert_doctor_failed(&run);
    let c = assert_fails(&run, "store", "error", "schema");
    assert!(c["found"].is_string() && c["expected"].is_string() && c["found"] != c["expected"], "{c}");
    let c = assert_fails(&run, "tamper", "error", "outside cpd");
    assert_eq!(c["found"][0]["kind"], "schema");
}

#[test]
fn a_history_row_without_a_txn_fails_tamper_only() {
    let env = healthy();
    open_store(&env);
    let conn = Connection::open(env.cpd_home().join("cpd.db")).unwrap();
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    conn.create_scalar_function("cpd_txn", 0, rusqlite::functions::FunctionFlags::SQLITE_UTF8, |_| Ok(999i64)).unwrap();
    conn.execute_batch("INSERT INTO history (txn, tbl, pk, op, before, after) VALUES (999, 'contexts', '[\"x\"]', 'insert', NULL, '{}')").unwrap();
    let run = doctor(&env);
    assert_doctor_failed(&run);
    assert_passes(&run, "store");
    let c = assert_fails(&run, "tamper", "error", "outside cpd");
    assert_eq!(c["found"], json!([{"kind": "history", "id": 1, "txn": 999}]));
}

/// Plan `pl` in `plan` with Locked Jobs `a` and `b` (queued, fingerprints stored
/// the way `plan lock` does), and the Draft `c`, which has no fingerprint.
fn locked_jobs(env: &TestEnv) {
    let mut store = open_store(env);
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Context', 't', 'hs');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead-pl', 'lead', 'ctx', 'pl', 'pl', '/repo', 'O3', 'stopped', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at)
                   VALUES ('pl', 'ctx', 'Plan', 'plan', 'lead-pl', 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                   VALUES ('pl', 'repo', '/repo', 'main', 'waiting', 'cpd/pl', cpd_txn(), 't');",
            )?;
            for key in ["a", "b", "c"] {
                tx.execute(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                                       created_txn, created_at)
                     VALUES ('pl', 'repo', ?1, 'T', 'S2', 'Do it.', 'Because.', '[\"x\"]', '[\"y\"]', '[]', 'draft', 0, cpd_txn(), 't')",
                    [key],
                )?;
            }
            Ok(())
        })
        .unwrap();
    let id = cpd_core::ids::PlanId::parse("pl").unwrap();
    let jobs = store.read(|conn| cpd_core::plan::jobs::load(conn, &id)).unwrap();
    store
        .write("fixture", |tx| {
            for job in jobs.iter().filter(|j| j.key != "c") {
                tx.execute(
                    "UPDATE jobs SET status = 'queued', fingerprint = ?1 WHERE plan = 'pl' AND key = ?2",
                    [cpd_core::plan::locked_fingerprint(job).as_str(), job.key.as_str()],
                )?;
            }
            Ok(())
        })
        .unwrap();
}

fn row_counts(env: &TestEnv) -> Vec<i64> {
    let conn = Connection::open(env.cpd_home().join("cpd.db")).unwrap();
    ["txns", "events", "history", "jobs", "plans"]
        .iter()
        .map(|t| conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0)).unwrap())
        .collect()
}

/// Edits Locked Job `a` outside cpd, the way a local user could: the `jobs_locked`
/// trigger is dropped, the row changed, the trigger recreated exactly, so the
/// schema check stays quiet and only the fingerprint tells.
fn edit_locked_job_outside_cpd(env: &TestEnv) {
    let conn = Connection::open(env.cpd_home().join("cpd.db")).unwrap();
    let txn: i64 = conn.query_row("SELECT max(id) FROM txns", [], |r| r.get(0)).unwrap();
    conn.create_scalar_function("cpd_txn", 0, rusqlite::functions::FunctionFlags::SQLITE_UTF8, move |_| Ok(txn)).unwrap();
    let sql: String = conn.query_row("SELECT sql FROM sqlite_schema WHERE name = 'jobs_locked'", [], |r| r.get(0)).unwrap();
    conn.execute_batch("DROP TRIGGER jobs_locked").unwrap();
    conn.execute("UPDATE jobs SET goal = 'Something else.' WHERE plan = 'pl' AND key = 'a'", []).unwrap();
    conn.execute_batch(&sql).unwrap();
}

#[test]
fn untouched_locked_jobs_pass_tamper() {
    let env = healthy();
    locked_jobs(&env);
    let run = doctor(&env);
    run.ok();
    assert_passes(&run, "tamper");
}

#[test]
fn a_locked_job_edited_outside_cpd_fails_tamper_and_nothing_is_written() {
    let env = healthy();
    locked_jobs(&env);
    edit_locked_job_outside_cpd(&env);
    let before = row_counts(&env);
    let run = doctor(&env);
    assert_doctor_failed(&run);
    assert_passes(&run, "store");
    let c = assert_fails(&run, "tamper", "error", "outside cpd");
    let found = c["found"].as_array().unwrap();
    assert_eq!(found.len(), 1, "only the edited Job is reported: {c}");
    assert_eq!((&found[0]["kind"], &found[0]["plan"], &found[0]["job"]), (&json!("fingerprint"), &json!("pl"), &json!("a")));
    let (expected, got) = (found[0]["expected"].as_str().unwrap(), found[0]["found"].as_str().unwrap());
    assert!(expected.len() == 64 && got.len() == 64 && expected != got, "{c}");
    assert_eq!(row_counts(&env), before, "doctor writes nothing");
}

#[test]
fn a_database_that_is_not_cpds_fails_store() {
    let env = healthy();
    fs::write(env.cpd_home().join("cpd.db"), "not a database, not a database, not a database").unwrap();
    let run = doctor(&env);
    assert_doctor_failed(&run);
    assert_fails(&run, "store", "error", "store_failed");
    assert_fails(&run, "tamper", "error", "not checked");
}

#[test]
fn a_database_newer_than_this_binary_fails_store() {
    let env = healthy();
    open_store(&env);
    Connection::open(env.cpd_home().join("cpd.db")).unwrap().pragma_update(None, "user_version", 99).unwrap();
    let run = doctor(&env);
    assert_doctor_failed(&run);
    let c = assert_fails(&run, "store", "error", "store_too_new");
    // `found` is the verdict's refusal (commands.md §14.1).
    assert_eq!(c["found"]["code"], "store_too_new", "{c}");
    assert!(c["found"]["message"].as_str().unwrap().contains("newer cpd"), "{c}");
}

#[test]
fn a_store_whose_breaking_migration_this_binary_lacks_fails_store_with_the_verdicts_refusal() {
    let env = healthy();
    apply_newer(&env, "29991231T2359-breaking", "breaking");
    let run = doctor(&env);
    assert_doctor_failed(&run);
    let c = assert_fails(&run, "store", "error", "store_too_new");
    assert_eq!(c["found"]["code"], "store_too_new", "{c}");
}

#[test]
fn additive_migrations_this_binary_lacks_are_no_failure_and_have_no_check_of_their_own() {
    let env = healthy();
    apply_newer(&env, "29991231T2359-additive", "additive");
    let run = doctor(&env);
    run.ok();
    assert_passes(&run, "store");
    assert_passes(&run, "tamper");
    let names: Vec<_> = checks(&run).iter().map(|c| c["name"].as_str().unwrap().to_owned()).collect();
    assert_eq!(names, NAMES, "no store_window check (hs-d178 D1)");
}

/// What a newer cpd does: a migration this binary lacks, recorded under `cpd_txn()` (stood in by
/// the newest `txns` id), with a new table.
fn apply_newer(env: &TestEnv, name: &str, compat: &str) {
    use rusqlite::functions::FunctionFlags;
    drop(open_store(env));
    let conn = Connection::open(env.cpd_home().join("cpd.db")).unwrap();
    let txn: i64 = conn.query_row("SELECT max(id) FROM txns", [], |r| r.get(0)).unwrap();
    conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, move |_| Ok(txn)).unwrap();
    conn.execute_batch("CREATE TABLE newer_things (id INTEGER PRIMARY KEY) STRICT;").unwrap();
    conn.execute(
        "INSERT INTO schema_migrations (name, compat, checksum, seq, txn, converted)
         VALUES (?1, ?2, lower(hex(zeroblob(32))), (SELECT max(seq) + 1 FROM schema_migrations), ?3, 0)",
        rusqlite::params![name, compat, txn],
    )
    .unwrap();
}

// ----------------------------------------------------------------- contexts

#[test]
fn doctor_has_no_contexts_check_and_ignores_an_old_store_folder() {
    let env = healthy();
    drop(open_store(&env));
    // What an earlier version left: the check of `contexts/` is gone with the git store.
    fs::create_dir_all(env.cpd_home().join("contexts/no-git")).unwrap();
    let run = doctor(&env);
    run.ok();
    assert!(checks(&run).iter().all(|c| c["name"] != "contexts"), "{run:?}");
}

// -------------------------------------------------------------------- watch

/// The pull request row of an `open` PR (`prs.pull_request`, migration 5).
const PULL_REQUEST: &str = "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
     VALUES ('acme/r', 1, 'https://github.com/acme/r/pull/1', 'Pay', 0, 'open', 'none', 'none', 't', 't');";

fn plan_in_phase(env: &TestEnv, phase: &str) {
    let (worktree, locked, status, pr) = match phase {
        "grounding" | "plan" => ("NULL", "NULL", "waiting", ""),
        "dispatch" => ("'/w'", "'t'", "dispatch", ""),
        "deploy" => ("'/w'", "'t'", "open", ", pr_number, pr_url, pr_opened_at, pull_request"),
        other => panic!("{other}"),
    };
    let pr_values = if phase == "deploy" { ", 1, 'https://github.com/acme/r/pull/1', 't', (SELECT id FROM pull_requests WHERE number = 1)" } else { "" };
    // `pr open` writes the pull request's row first (D6): an `open` PR points to one.
    let pull = if phase == "deploy" { PULL_REQUEST } else { "" };
    open_store(env)
        .write("test", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'hs');
                 INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'Lead', '/', 'O3', 'idle', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('pay', 'ctx', 'Pay', '{phase}', 'lead1', 't', {locked});
                 {pull}
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at{pr})
                   VALUES ('pay', 'r', '/r', 'main', '{status}', 'cpd/pay', {worktree}, {locked}, cpd_txn(), 't'{pr_values});"
            ))?;
            Ok(())
        })
        .unwrap();
}

fn hold_watch_lock(env: &TestEnv) -> Flock<fs::File> {
    let lock = env.cpd_home().join("locks/watch.lock");
    fs::create_dir_all(lock.parent().unwrap()).unwrap();
    Flock::lock(fs::File::create(&lock).unwrap(), FlockArg::LockExclusiveNonblock).map_err(|e| e.1).unwrap()
}

#[test]
fn the_engine_must_run_when_a_plan_is_in_dispatch_or_deploy() {
    for phase in ["dispatch", "deploy"] {
        let env = healthy();
        plan_in_phase(&env, phase);
        let run = doctor(&env);
        run.ok();
        assert_fails(&run, "watch", "warn", "no engine runs");

        // A lock file nobody holds is no engine either.
        let lock = hold_watch_lock(&env);
        drop(lock);
        let run = doctor(&env);
        assert_fails(&run, "watch", "warn", "no engine runs");

        let _held = hold_watch_lock(&env);
        let run = doctor(&env);
        run.ok();
        assert_passes(&run, "watch");
    }
}

#[test]
fn no_engine_is_needed_when_no_plan_is_in_dispatch_or_pr() {
    let env = healthy();
    let run = doctor(&env);
    assert_passes(&run, "watch");
    let env = healthy();
    plan_in_phase(&env, "plan");
    let run = doctor(&env);
    assert_passes(&run, "watch");
    assert!(!Path::new(&env.cpd_home().join("locks/watch.lock")).exists(), "doctor must not create the lock");
}

// ---------------------------------------------------------------- deadlines

/// commands-W6: a fake that hangs on the probe named by `hang` (a shell test on its
/// arguments) sleeps past the deadline; `sleep` is not on the tests' `PATH`, so it is linked.
fn link_sleep(env: &TestEnv) {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let sleep = std::env::split_paths(&path).map(|d| d.join("sleep")).find(|p| p.is_file()).expect("sleep is on PATH");
    symlink(sleep, env.bin().join("sleep")).unwrap();
}

const HANG: &str = "exec sleep 120";

/// Asserts `name` failed with `tool_timeout` naming `tool` and its deadline of `seconds` (as the override set it),
/// and the other checks still ran.
fn assert_timed_out(run: &Run, name: &str, severity: &str, tool: &str, argv: &str, seconds: &str) {
    let c = assert_fails(run, name, severity, "tool_timeout");
    let message = c["message"].as_str().unwrap();
    assert!(message.contains(&format!("{tool} ran past its deadline of {seconds}s")), "{message}");
    assert!(message.contains(argv), "{message}");
}

#[test]
fn a_hung_gh_auth_is_a_failed_warning_and_doctor_finishes() {
    let env = healthy();
    link_sleep(&env);
    env.fake_gh(&format!("[ \"$1\" = auth ] && {HANG}; echo 'gh version 2.45.0 (2025-07-18)'"));
    let run = doctor_with(&env, &[("CPD_TEST_DEADLINE_GH", "1")]);
    run.ok();
    assert_timed_out(&run, "gh_auth", "warn", "gh", "auth status", "1");
    assert_eq!(checks(&run).len(), NAMES.len());
}

#[test]
fn a_hung_tool_version_is_a_failed_error_check_and_doctor_goes_on() {
    let env = healthy();
    link_sleep(&env);
    env.fake("git", HANG);
    let run = doctor_with(&env, &[("CPD_TEST_DEADLINE_GIT", "1")]);
    assert_doctor_failed(&run);
    assert_timed_out(&run, "git", "error", "git", "--version", "1");
    assert_passes(&run, "tmux");
    assert_passes(&run, "pi_node");
    assert_passes(&run, "tiers");
}

#[test]
fn a_hung_pi_node_is_a_failed_check() {
    let env = healthy();
    link_sleep(&env);
    fs::remove_file(env.bin().join("node")).unwrap();
    write_executable(&env.bin().join("node"), &format!("#!/bin/sh\n{HANG}\n"));
    // Both probes hang: a sub-second deadline (contract §1.2 "Test overrides") is all they wait.
    let run = doctor_with(&env, &[("CPD_TEST_DEADLINE_NODE", "0.3")]);
    assert_doctor_failed(&run);
    assert_timed_out(&run, "pi_node", "error", "node", "--version", "0.3");
    assert_timed_out(&run, "tiers", "error", "node", "--input-type=module", "0.3");
}

/// The catalog probe's deadline is doctor's (10 s, commands.md §14.1), not the 30 s of
/// the catalog's own cache fill: a node that answers `--version` and hangs on the module.
/// The test sets doctor's deadline to 1 s (`CPD_TEST_DOCTOR_DEADLINE`, contract §1.2), and the probe reports it.
#[test]
fn a_hung_catalog_probe_is_killed_at_doctors_deadline() {
    let env = healthy();
    link_sleep(&env);
    fs::remove_file(env.bin().join("node")).unwrap();
    write_executable(&env.bin().join("node"), &format!("#!/bin/sh\n[ \"$1\" = --version ] && {{ echo v22.0.0; exit 0; }}\n{HANG}\n"));
    let started = std::time::Instant::now();
    let run = doctor_with(&env, &[("CPD_TEST_DOCTOR_DEADLINE", "1")]);
    // time-claim: the probe is killed at doctor's deadline, long before the cache fill's 30 s; no per-poll signal from
    // the deadline.
    assert!(started.elapsed() < std::time::Duration::from_secs(20), "took {:?}", started.elapsed());
    assert_doctor_failed(&run);
    assert_passes(&run, "pi_node");
    let c = assert_fails(&run, "tiers", "error", "tool_timeout");
    assert_eq!(c["found"]["deadline_seconds"], 1, "{c}");
}
