//! `cpd doctor` (commands.md §14.1): the machine's checks, in-process on fake ports. Every external tool is a route of
//! the `FakeRunner` (`Fx::healthy_tools`), and a hung one is `tool_timeout` on the `ManualClock`: no process runs and no
//! test waits.

use crate::common;

use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::time::Duration;

use common::fx::Fx;
use common::{Run, TestEnv};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Reply};
use cpd_core::store::Store;
use nix::fcntl::{Flock, FlockArg};
use rusqlite::Connection;
use serde_json::{Value, json};

const NAMES: [&str; 15] = [
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
    "worker_runner",
    "host_runner",
    "lead_packages",
    "watch",
];

/// The deadline of each probe (`doctor::DEADLINE`, commands.md §14.1): a probe that takes this long is `tool_timeout`.
const DEADLINE: Duration = Duration::from_secs(10);

/// A rule of the `FakeRunner`: `reply` answers a call of `program` that `matches`. It wins over the healthy route.
fn script(
    fx: &Fx,
    program: &str,
    matches: impl Fn(&Call) -> bool + Send + Sync + 'static,
    reply: Reply,
) {
    let _ = (*fx.fakes.runner).clone().on(program, matches, reply);
}

/// Whether the call is `gh auth ...`, the `gh_auth` probe.
fn is_auth(call: &Call) -> bool {
    call.args.first().map(String::as_str) == Some("auth")
}

/// A reply that takes far past [`DEADLINE`]: the tool hangs.
fn hang() -> Reply {
    Reply::ok("").takes(Duration::from_secs(120))
}

fn write_config(fx: &Fx, extra: &str) {
    let adapter = fx.env.root().join("adapter");
    fs::create_dir_all(&adapter).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    let text = format!(
        "version = 1\nmachine = \"hs\"\npi_adapter = \"{}\"\n{extra}\n",
        adapter.display()
    );
    fx.env.write_config(&text);
    machine_models(fx, ["p/m-opus", "p/m-sonnet", "p/m-low", "p/m-aux"]);
}

/// The machine policy's `[model]`: the refs of `O`, `S`, `H` and `G`.
fn machine_models(fx: &Fx, [o, s, h, g]: [&str; 4]) {
    let path = fx.env.cpd_home().join("policy/host.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        path,
        format!("[model]\nO = \"{o}\"\nS = \"{s}\"\nH = \"{h}\"\nG = \"{g}\"\n"),
    )
    .unwrap();
}

/// Role defaults that are rungs of the healthy catalog (G `p/m-aux` 0-4, H `p/m-low` 0-2, S and O 0-4), ascending: the
/// built-in GX and H3 are no rungs of it.
const ROLES: &str = "context_distill_tier = \"G0\"\nworker_tier = \"H2\"\nlead_tier = \"S4\"";

/// [`write_config`] with `extra`, then the `[experimental]` table with `durable_workers = <durable>` after the keys.
fn write_config_durable(fx: &Fx, extra: &str, durable: bool) {
    write_config(fx, extra);
    let path = fx.env.cpd_home().join("config.toml");
    let mut text = fs::read_to_string(&path).unwrap();
    text.push_str(&format!("\n[experimental]\ndurable_workers = {durable}\n"));
    fs::write(&path, text).unwrap();
}

/// A machine where every check passes: on fake ports, with the healthy tools ([`Fx::healthy_tools`]) and a config.
fn healthy() -> Fx {
    let fx = Fx::with_ports();
    write_config(&fx, ROLES);
    fx.healthy_tools();
    fx
}

/// `cpd doctor` with `PATH` = the test's `bin/` only, and `vars` added.
fn doctor_with(fx: &Fx, vars: &[(&str, &str)]) -> Run {
    let bin = fx.env.bin();
    let mut all = vec![("PATH", bin.to_str().unwrap())];
    all.extend(vars.iter().copied());
    fx.cpd_with(fx.env.root(), &["doctor"], &all)
}

fn doctor(fx: &Fx) -> Run {
    doctor_with(fx, &[])
}

/// The checks of a success, or of a `doctor_failed`.
fn checks(run: &Run) -> Vec<Value> {
    let json = run.json();
    let list = if json["ok"] == true {
        &json["checks"]
    } else {
        &json["error"]["details"]["checks"]
    };
    list.as_array()
        .unwrap_or_else(|| panic!("no checks in {run:?}"))
        .clone()
}

fn check(run: &Run, name: &str) -> Value {
    checks(run)
        .into_iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no check {name} in {run:?}"))
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
    assert!(
        c["message"].as_str().unwrap_or_default().contains(message),
        "{message:?} not in {c}"
    );
    c
}

/// The result is `doctor_failed` (exit 1) with every check in `details.checks`.
fn assert_doctor_failed(run: &Run) {
    assert_eq!(run.exit, 1, "{run:?}");
    assert_eq!(run.error()["code"], "doctor_failed", "{run:?}");
    let names: Vec<_> = checks(run)
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, NAMES);
}

fn open_store(env: &TestEnv) -> Store {
    Store::open(
        &CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(),
        5000,
    )
    .unwrap()
}

// ------------------------------------------------------------------ success

/// pro-d1: the holographic engine is removed: no check of that name, and a config without engine keys is healthy.
#[test]
fn doctor_has_no_holographic_check() {
    let fx = healthy();
    let run = doctor(&fx);
    run.ok();
    let all = checks(&run);
    assert!(all.iter().all(|c| c["name"] != "holographic"), "{all:?}");
    assert!(all.iter().all(|c| c["ok"] == true), "{all:?}");
}

#[test]
fn a_healthy_machine_passes_every_check_in_order() {
    let fx = healthy();
    let run = doctor(&fx);
    let json = run.ok();
    let all = checks(&run);
    let names: Vec<_> = all.iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, NAMES);
    for c in &all {
        assert_eq!(c["ok"], true, "{c}");
        assert!(
            ["error", "warn"].contains(&c["severity"].as_str().unwrap()),
            "{c}"
        );
    }
    assert_eq!(
        json.as_object().unwrap().len(),
        2,
        "only ok and checks: {json}"
    );

    let tool = |name: &str, found: &str, expected: &str| {
        let c = check(&run, name);
        assert_eq!(
            (c["found"].as_str(), c["expected"].as_str()),
            (Some(found), Some(expected)),
            "{c}"
        );
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
    let fx = healthy();
    // No store yet: the first `doctor` creates and migrates it.
    fs::remove_file(fx.env.cpd_home().join("cpd.db")).unwrap();
    let first = doctor(&fx);
    first.ok();
    let db = fx.env.cpd_home().join("cpd.db");
    let txns = |fx: &Fx| -> i64 {
        let conn = Connection::open(fx.env.cpd_home().join("cpd.db")).unwrap();
        conn.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))
            .unwrap()
    };
    assert!(db.exists());
    assert_eq!(txns(&fx), 1, "the migration's transaction only");
    let again = doctor(&fx);
    assert_eq!(again.stdout, first.stdout);
    assert_eq!(txns(&fx), 1);
    // The tiers check asks pi and does not fill the cache.
    let conn = Connection::open(db).unwrap();
    let cached: i64 = conn
        .query_row("SELECT count(*) FROM tier_catalog", [], |r| r.get(0))
        .unwrap();
    assert_eq!(cached, 0);
}

#[test]
fn an_argument_is_usage() {
    let fx = healthy();
    let run = fx.cpd(&["doctor", "--bogus"]);
    assert_eq!(run.exit, 2);
}

// ------------------------------------------------------------------- config

#[test]
fn a_missing_config_fails_and_what_needs_it_is_not_checked() {
    let fx = healthy();
    fs::remove_file(fx.env.cpd_home().join("config.toml")).unwrap();
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    assert_fails(&run, "config", "error", "config_missing");
    for name in [
        "store",
        "tiers",
        "pi_adapter",
        "worker_runner",
        "host_runner",
    ] {
        assert_fails(&run, name, "error", "not checked");
    }
    assert_fails(&run, "lead_packages", "warn", "not checked");
    assert_passes(&run, "git");
}

#[test]
fn an_invalid_config_fails_with_its_key() {
    let fx = healthy();
    fs::write(
        fx.env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"hs\"\nconcurrency = 0\n",
    )
    .unwrap();
    let run = doctor(&fx);
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
    // Each case is the healthy machine with one tool changed (doctor runs every tool afresh), on an `Fx` of its own.
    for (tool, output, passes, found) in cases {
        let fx = healthy();
        // `gh auth status` stays healthy: only the version probe is changed.
        script(
            &fx,
            tool,
            |call| !is_auth(call),
            Reply::ok(&format!("{output}\n")),
        );
        let run = doctor(&fx);
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
    for tool in ["git", "tmux", "pi", "gh"] {
        let fx = healthy();
        fx.fake(tool, Reply::io(ErrorKind::NotFound));
        if tool == "pi" {
            fs::remove_file(fx.env.bin().join("pi")).unwrap();
        }
        let run = doctor(&fx);
        assert_doctor_failed(&run);
        let c = assert_fails(&run, tool, "error", "not found");
        assert!(c["expected"].is_string(), "{c}");
    }
}

#[test]
fn a_tool_that_prints_no_version_or_fails_is_a_failed_check() {
    let fx = healthy();
    fx.fake("git", Reply::ok("git version unknown\n"));
    fx.fake("tmux", Reply::exit(1, "", "boom\n"));
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    assert_fails(&run, "git", "error", "no version");
    assert_fails(&run, "tmux", "error", "exited 1");
}

#[test]
fn gh_auth_is_a_warning_that_never_fails_doctor() {
    let fx = healthy();
    script(&fx, "gh", is_auth, Reply::exit(1, "", "not logged in\n"));
    let run = doctor(&fx);
    run.ok();
    assert_fails(&run, "gh_auth", "warn", "exited 1");
    assert_passes(&run, "gh");
    assert_eq!(
        fx.calls("gh")
            .iter()
            .filter(|c| c.args == ["auth", "status"])
            .count(),
        1
    );
}

#[test]
fn pi_node_must_run() {
    {
        let fx = healthy();
        fx.fake("node", Reply::io(ErrorKind::NotFound));
        let run = doctor(&fx);
        assert_doctor_failed(&run);
        assert_fails(&run, "pi_node", "error", "not found");
    }

    // pi's own Node wins over the one on PATH, and its failure is the check's.
    let fx = healthy();
    let pi_node = fx.env.home().join(".local/share/pi-node/current/bin");
    fs::create_dir_all(&pi_node).unwrap();
    fs::write(pi_node.join("node"), "").unwrap();
    let program = pi_node.join("node").display().to_string();
    script(
        &fx,
        "node",
        move |call| call.program == program,
        Reply::exit(7, "", "node broke\n"),
    );
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    assert_fails(&run, "pi_node", "error", "exited 7");
}

// ------------------------------------------------------------ pi and tiers

#[test]
fn tiers_fail_for_a_model_pi_does_not_have() {
    let fx = healthy();
    fs::write(
        fx.env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"hs\"\npi_adapter = \"{}\"\n",
            fx.env.root().join("adapter").display()
        ),
    )
    .unwrap();
    machine_models(&fx, ["p/m-opus", "p/nope", "p/m-low", "p/m-aux"]);
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    let c = assert_fails(&run, "tiers", "error", "tier_model_unknown");
    assert_eq!(c["found"]["missing"][0]["model"], "p/nope");
}

#[test]
fn tiers_fail_for_a_lead_tier_its_model_does_not_support() {
    let fx = healthy();
    // H is p/m-low: efforts 0 to 2.
    write_config(&fx, "lead_tier = \"H4\"");
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    assert_fails(&run, "tiers", "error", "tier_invalid");
}

/// A role default that is no rung of the catalog fails the check and names its key (it is no error at config parse).
/// The healthy catalog: every model has levels 0 to 4, but H is `p/m-low` (0 to 2) and G is `p/m-aux` (0 to 4; `GX` is valid on any model).
#[test]
fn tiers_fail_for_a_default_that_is_not_a_catalog_rung() {
    // Every default is a rung: G0 (G has levels), H2, O2, O4.
    let fx = healthy();
    write_config(
        &fx,
        "context_distill_tier = \"G0\"\nworker_tier = \"H2\"\nlead_tier = \"S4\"",
    );
    assert_passes(&doctor(&fx), "tiers");
    // `worker_tier` H3 is no rung: H offers only 0 to 2.
    write_config(&fx, "context_distill_tier = \"G0\"\nworker_tier = \"H3\"");
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    assert_fails(&run, "tiers", "error", "worker_tier");
    // A lead_tier outside the catalog is still `tier_invalid`, checked first.
    write_config(&fx, "lead_tier = \"H4\"");
    assert_fails(&doctor(&fx), "tiers", "error", "tier_invalid");
}

/// `GX` is valid on every model (contract §9, R2): the default `context_distill_tier` passes though G is `p/m-aux`, which
/// offers levels 0 to 4.
#[test]
fn doctor_does_not_report_context_distill_tier_x_on_a_model_with_efforts() {
    let fx = healthy();
    write_config(
        &fx,
        "context_distill_tier = \"GX\"\nworker_tier = \"H2\"\nlead_tier = \"S4\"",
    );
    let run = doctor(&fx);
    assert_passes(&run, "tiers");
}

#[test]
fn tiers_fail_when_pi_has_no_release() {
    let fx = healthy();
    fs::remove_file(fx.env.root().join("install/current-version")).unwrap();
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    assert_fails(&run, "tiers", "error", "pi_not_found");
}

#[test]
fn the_adapter_directory_needs_a_package_json() {
    {
        // The default, beside the binary, does not exist in a build tree.
        let fx = healthy();
        fs::write(
            fx.env.cpd_home().join("config.toml"),
            "version = 1\nmachine = \"hs\"\n",
        )
        .unwrap();
        let run = doctor(&fx);
        assert_doctor_failed(&run);
        let c = assert_fails(&run, "pi_adapter", "error", "package.json");
        assert!(
            c["found"].as_str().unwrap().ends_with("share/cpd/pi"),
            "{c}"
        );
    }

    // A directory removed after the config was written.
    let fx = healthy();
    fs::remove_file(fx.env.root().join("adapter/package.json")).unwrap();
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    assert_fails(&run, "config", "error", "package.json");
}

#[test]
fn lead_packages_say_how_a_lead_gets_each() {
    {
        let fx = healthy();
        write_config(&fx, "lead_pi_packages = [\"npm:@example/ext@0.28.17\"]");
        let adapter = fx.env.root().join("adapter").display().to_string();
        let settings = fx.env.home().join(".pi/agent/settings.json");
        fs::create_dir_all(settings.parent().unwrap()).unwrap();
        fs::write(&settings, r#"{"packages": ["npm:@example/ext@0.1.0"]}"#).unwrap();
        let run = doctor(&fx);
        let c = check(&run, "lead_packages");
        assert_eq!(c["ok"], true, "{c}");
        let by_settings = json!({"by": "settings", "settings": settings.display().to_string()});
        assert_eq!(
            c["found"],
            json!([
                {"package": adapter, "by": "-e"},
                {"package": "npm:@example/ext@0.28.17", "by": by_settings["by"], "settings": by_settings["settings"]},
            ])
        );
    }

    // Declared by the project settings of the current directory, relative paths from that file.
    let fx = healthy();
    write_config(&fx, "lead_pi_packages = [\"./local-pkg\"]");
    let project = fx.env.root().join(".pi/settings.json");
    fs::create_dir_all(project.parent().unwrap()).unwrap();
    fs::write(&project, r#"{"packages": ["../local-pkg"]}"#).unwrap();
    let run = doctor(&fx);
    let c = check(&run, "lead_packages");
    assert_eq!(c["ok"], true, "{c}");
    assert_eq!(
        c["found"][1],
        json!({"package": "./local-pkg", "by": "settings", "settings": project.canonicalize().unwrap().display().to_string()})
    );
}

#[test]
fn an_entry_neither_declared_nor_resolvable_is_a_warning() {
    let fx = healthy();
    write_config(
        &fx,
        &format!(
            "{ROLES}\nlead_pi_packages = [\"/nonexistent/pi-package\", \"npm:left-pad@1.0.0\"]"
        ),
    );
    let run = doctor(&fx);
    run.ok();
    let c = assert_fails(&run, "lead_packages", "warn", "/nonexistent/pi-package");
    assert_eq!(
        c["found"][1],
        json!({"package": "/nonexistent/pi-package", "by": "-e", "resolvable": false})
    );
    assert_eq!(
        c["found"][2],
        json!({"package": "npm:left-pad@1.0.0", "by": "-e"})
    );
}

// -------------------------------------------------------------- the store

#[test]
fn a_changed_schema_fails_store_and_tamper() {
    let fx = healthy();
    open_store(&fx.env);
    Connection::open(fx.env.cpd_home().join("cpd.db"))
        .unwrap()
        .execute_batch("DROP TRIGGER contexts_no_delete")
        .unwrap();
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    let c = assert_fails(&run, "store", "error", "schema");
    assert!(
        c["found"].is_string() && c["expected"].is_string() && c["found"] != c["expected"],
        "{c}"
    );
    let c = assert_fails(&run, "tamper", "error", "outside cpd");
    assert_eq!(c["found"][0]["kind"], "schema");
}

#[test]
fn a_history_row_without_a_txn_fails_tamper_only() {
    let fx = healthy();
    open_store(&fx.env);
    let conn = Connection::open(fx.env.cpd_home().join("cpd.db")).unwrap();
    conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
    conn.create_scalar_function(
        "cpd_txn",
        0,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8,
        |_| Ok(999i64),
    )
    .unwrap();
    conn.execute_batch("INSERT INTO history (txn, tbl, pk, op, before, after) VALUES (999, 'contexts', '[\"x\"]', 'insert', NULL, '{}')").unwrap();
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    assert_passes(&run, "store");
    let c = assert_fails(&run, "tamper", "error", "outside cpd");
    assert_eq!(
        c["found"],
        json!([{"kind": "history", "id": 1, "txn": 999}])
    );
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
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                   VALUES ('pl', 'repo', '/repo', 'main', 'waiting', 'cpd/pl', cpd_txn(), 't');",
            )?;
            for key in ["a", "b", "c"] {
                tx.execute(
                    "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                                       created_txn, created_at)
                     VALUES ('pl', 'repo', ?1, 'T', 'S2', 'Do it.', 'Because.', '[\"x\"]', '[\"y\"]', '[]', 'draft', 0, cpd_txn(), 't')",
                    [key],
                )?;
            }
            Ok(())
        })
        .unwrap();
    let id = cpd_core::ids::PlanId::parse("pl").unwrap();
    let jobs = store
        .read(|conn| cpd_core::plan::jobs::load(conn, &id))
        .unwrap();
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
        .map(|t| {
            conn.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
                .unwrap()
        })
        .collect()
}

/// Edits Locked Job `a` outside cpd, the way a local user could: the `jobs_locked`
/// trigger is dropped, the row changed, the trigger recreated exactly, so the
/// schema check stays quiet and only the fingerprint tells.
fn edit_locked_job_outside_cpd(env: &TestEnv) {
    let conn = Connection::open(env.cpd_home().join("cpd.db")).unwrap();
    let txn: i64 = conn
        .query_row("SELECT max(id) FROM txns", [], |r| r.get(0))
        .unwrap();
    conn.create_scalar_function(
        "cpd_txn",
        0,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8,
        move |_| Ok(txn),
    )
    .unwrap();
    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name = 'jobs_locked'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute_batch("DROP TRIGGER jobs_locked").unwrap();
    conn.execute(
        "UPDATE jobs SET goal = 'Something else.' WHERE plan = 'pl' AND key = 'a'",
        [],
    )
    .unwrap();
    conn.execute_batch(&sql).unwrap();
}

#[test]
fn untouched_locked_jobs_pass_tamper() {
    let fx = healthy();
    locked_jobs(&fx.env);
    let run = doctor(&fx);
    run.ok();
    assert_passes(&run, "tamper");
}

#[test]
fn a_locked_job_edited_outside_cpd_fails_tamper_and_nothing_is_written() {
    let fx = healthy();
    locked_jobs(&fx.env);
    edit_locked_job_outside_cpd(&fx.env);
    let before = row_counts(&fx.env);
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    assert_passes(&run, "store");
    let c = assert_fails(&run, "tamper", "error", "outside cpd");
    let found = c["found"].as_array().unwrap();
    assert_eq!(found.len(), 1, "only the edited Job is reported: {c}");
    assert_eq!(
        (&found[0]["kind"], &found[0]["plan"], &found[0]["job"]),
        (&json!("fingerprint"), &json!("pl"), &json!("a"))
    );
    let (expected, got) = (
        found[0]["expected"].as_str().unwrap(),
        found[0]["found"].as_str().unwrap(),
    );
    assert!(
        expected.len() == 64 && got.len() == 64 && expected != got,
        "{c}"
    );
    assert_eq!(row_counts(&fx.env), before, "doctor writes nothing");
}

#[test]
fn a_database_that_is_not_cpds_fails_store() {
    let fx = healthy();
    fs::write(
        fx.env.cpd_home().join("cpd.db"),
        "not a database, not a database, not a database",
    )
    .unwrap();
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    assert_fails(&run, "store", "error", "store_failed");
    assert_fails(&run, "tamper", "error", "not checked");
}

#[test]
fn a_database_newer_than_this_binary_fails_store() {
    let fx = healthy();
    open_store(&fx.env);
    Connection::open(fx.env.cpd_home().join("cpd.db"))
        .unwrap()
        .pragma_update(None, "user_version", 99)
        .unwrap();
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    let c = assert_fails(&run, "store", "error", "store_too_new");
    // `found` is the verdict's refusal (commands.md §14.1).
    assert_eq!(c["found"]["code"], "store_too_new", "{c}");
    assert!(
        c["found"]["message"]
            .as_str()
            .unwrap()
            .contains("newer cpd"),
        "{c}"
    );
}

#[test]
fn a_store_whose_breaking_migration_this_binary_lacks_fails_store_with_the_verdicts_refusal() {
    let fx = healthy();
    apply_newer(&fx.env, "29991231T2359-breaking", "breaking");
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    let c = assert_fails(&run, "store", "error", "store_too_new");
    assert_eq!(c["found"]["code"], "store_too_new", "{c}");
}

#[test]
fn additive_migrations_this_binary_lacks_are_no_failure_and_have_no_check_of_their_own() {
    let fx = healthy();
    apply_newer(&fx.env, "29991231T2359-additive", "additive");
    let run = doctor(&fx);
    run.ok();
    assert_passes(&run, "store");
    assert_passes(&run, "tamper");
    let names: Vec<_> = checks(&run)
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, NAMES, "no store_window check (hs-d178 D1)");
}

/// What a newer cpd does: a migration this binary lacks, recorded under `cpd_txn()` (stood in by
/// the newest `txns` id), with a new table.
fn apply_newer(env: &TestEnv, name: &str, compat: &str) {
    use rusqlite::functions::FunctionFlags;
    drop(open_store(env));
    let conn = Connection::open(env.cpd_home().join("cpd.db")).unwrap();
    let txn: i64 = conn
        .query_row("SELECT max(id) FROM txns", [], |r| r.get(0))
        .unwrap();
    conn.create_scalar_function("cpd_txn", 0, FunctionFlags::SQLITE_UTF8, move |_| Ok(txn))
        .unwrap();
    conn.execute_batch("CREATE TABLE newer_things (id INTEGER PRIMARY KEY) STRICT;")
        .unwrap();
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
    let fx = healthy();
    drop(open_store(&fx.env));
    // What an earlier version left: the check of `contexts/` is gone with the git store.
    fs::create_dir_all(fx.env.cpd_home().join("contexts/no-git")).unwrap();
    let run = doctor(&fx);
    run.ok();
    assert!(
        checks(&run).iter().all(|c| c["name"] != "contexts"),
        "{run:?}"
    );
}

// -------------------------------------------------------------------- watch

/// The pull request row of an `open` PR (`prs.pull_request`, migration 5).
const PULL_REQUEST: &str = "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
     VALUES ('acme/r', 1, 'https://github.com/acme/r/pull/1', 'Pay', 0, 'open', 'none', 'none', 't', 't');";

fn plan_in_phase(env: &TestEnv, phase: &str) {
    let (worktree, locked, status, pr) = match phase {
        "grounding" | "plan" => ("NULL", "NULL", "waiting", ""),
        "dispatch" => ("'/w'", "'t'", "dispatch", ""),
        "deploy" => (
            "'/w'",
            "'t'",
            "open",
            ", pr_number, pr_url, pr_opened_at, pull_request",
        ),
        other => panic!("{other}"),
    };
    let pr_values = if phase == "deploy" {
        ", 1, 'https://github.com/acme/r/pull/1', 't', (SELECT id FROM pull_requests WHERE number = 1)"
    } else {
        ""
    };
    // `pr open` writes the pull request's row first (D6): an `open` PR points to one.
    let pull = if phase == "deploy" { PULL_REQUEST } else { "" };
    open_store(env)
        .write("test", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'hs');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('pay', 'ctx', 'Pay', '{phase}', 'lead1', 't', {locked});
                 {pull}
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at{pr})
                   VALUES ('pay', 'r', '/r', 'main', '{status}', 'cpd/pay', {worktree}, {locked}, cpd_txn(), 't'{pr_values});"
            ))?;
            Ok(())
        })
        .unwrap();
}

fn hold_watch_lock(env: &TestEnv) -> Flock<fs::File> {
    let lock = env.cpd_home().join("locks/watch.lock");
    fs::create_dir_all(lock.parent().unwrap()).unwrap();
    Flock::lock(
        fs::File::create(&lock).unwrap(),
        FlockArg::LockExclusiveNonblock,
    )
    .map_err(|e| e.1)
    .unwrap()
}

#[test]
fn the_engine_must_run_when_a_plan_is_in_dispatch_or_deploy() {
    for phase in ["dispatch", "deploy"] {
        let fx = healthy();
        plan_in_phase(&fx.env, phase);
        let run = doctor(&fx);
        run.ok();
        assert_fails(&run, "watch", "warn", "no engine runs");

        // A lock file nobody holds is no engine either.
        let lock = hold_watch_lock(&fx.env);
        drop(lock);
        let run = doctor(&fx);
        assert_fails(&run, "watch", "warn", "no engine runs");

        let _held = hold_watch_lock(&fx.env);
        let run = doctor(&fx);
        run.ok();
        assert_passes(&run, "watch");
    }
}

#[test]
fn no_engine_is_needed_when_no_plan_is_in_dispatch_or_pr() {
    {
        let fx = healthy();
        let run = doctor(&fx);
        assert_passes(&run, "watch");
    }
    let fx = healthy();
    plan_in_phase(&fx.env, "plan");
    let run = doctor(&fx);
    assert_passes(&run, "watch");
    assert!(
        !Path::new(&fx.env.cpd_home().join("locks/watch.lock")).exists(),
        "doctor must not create the lock"
    );
}

// ---------------------------------------------------------------- deadlines

/// Asserts `name` failed with `tool_timeout` naming `tool` and the probe's deadline of 10 s, with `argv` in its message.
fn assert_timed_out(run: &Run, name: &str, severity: &str, tool: &str, argv: &str) {
    let c = assert_fails(run, name, severity, "tool_timeout");
    let message = c["message"].as_str().unwrap();
    assert!(
        message.contains(&format!(
            "{tool} ran past its deadline of {}s",
            DEADLINE.as_secs()
        )),
        "{message}"
    );
    assert!(message.contains(argv), "{message}");
}

#[test]
fn a_hung_gh_auth_is_a_failed_warning_and_doctor_finishes() {
    let fx = healthy();
    script(&fx, "gh", is_auth, hang());
    let run = doctor(&fx);
    run.ok();
    assert_timed_out(&run, "gh_auth", "warn", "gh", "auth status");
    assert_eq!(checks(&run).len(), NAMES.len());
    assert_eq!(
        fx.fakes.clock.elapsed(),
        DEADLINE,
        "the hung probe took its deadline of virtual time"
    );
}

#[test]
fn a_hung_tool_version_is_a_failed_error_check_and_doctor_goes_on() {
    let fx = healthy();
    fx.fake("git", hang());
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    assert_timed_out(&run, "git", "error", "git", "--version");
    assert_passes(&run, "tmux");
    assert_passes(&run, "pi_node");
    assert_passes(&run, "tiers");
    assert_eq!(fx.fakes.clock.elapsed(), DEADLINE);
}

#[test]
fn a_hung_pi_node_is_a_failed_check() {
    let fx = healthy();
    fx.fake("node", hang());
    // Both probes hang: each takes its deadline of virtual time.
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    assert_timed_out(&run, "pi_node", "error", "node", "--version");
    assert_timed_out(&run, "tiers", "error", "node", "--input-type=module");
    assert_eq!(fx.fakes.clock.elapsed(), DEADLINE * 2);
}

// ------------------------------------------------------------ worker_runner

/// The runner file of the `durable` runner, under the adapter `write_config` made.
fn runner_file(fx: &Fx) -> std::path::PathBuf {
    fx.env.root().join("adapter/durable-worker/main.ts")
}

fn add_runner_file(fx: &Fx) {
    let file = runner_file(fx);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, "").unwrap();
}

/// The `node --version` calls: pi's Node, asked by `pi_node` and by `worker_runner` when it needs the version.
fn version_calls(fx: &Fx) -> usize {
    fx.calls("node")
        .iter()
        .filter(|c| c.args == ["--version"])
        .count()
}

#[test]
fn worker_runner_passes_for_pi_and_never_looks_at_the_adapter() {
    // No runner file, and a Node too old for the runner: `pi` does not care.
    for explicit in [false, true] {
        let fx = healthy();
        if explicit {
            write_config_durable(&fx, ROLES, false);
        }
        script(
            &fx,
            "node",
            |call| call.args == ["--version"],
            Reply::ok("v18.0.0\n"),
        );
        assert!(!runner_file(&fx).exists());
        let run = doctor(&fx);
        run.ok();
        assert_passes(&run, "worker_runner");
        let c = check(&run, "worker_runner");
        assert_eq!(c["severity"], "error", "{c}");
        assert_eq!(
            version_calls(&fx),
            1,
            "only `pi_node` asked for the version: explicit: {explicit}"
        );
    }
}

#[test]
fn worker_runner_durable_passes_with_the_runner_file_and_node_22_19_0() {
    let fx = healthy();
    write_config_durable(&fx, ROLES, true);
    add_runner_file(&fx);
    script(
        &fx,
        "node",
        |call| call.args == ["--version"],
        Reply::ok("v22.19.0\n"),
    );
    let run = doctor(&fx);
    run.ok();
    let c = check(&run, "worker_runner");
    assert_eq!(c["ok"], true, "{c}");
    assert_eq!(c["severity"], "error", "{c}");
    assert_eq!(
        (c["found"].as_str(), c["expected"].as_str()),
        (Some("22.19.0"), Some(">= 22.19.0")),
        "{c}"
    );
}

#[test]
fn worker_runner_durable_fails_when_the_runner_file_is_missing() {
    let fx = healthy();
    write_config_durable(&fx, "", true);
    assert!(!runner_file(&fx).exists());
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    let c = assert_fails(&run, "worker_runner", "error", "main.ts");
    assert!(
        c["message"]
            .as_str()
            .unwrap()
            .contains(runner_file(&fx).to_str().unwrap()),
        "names the file it looked for: {c}"
    );
}

#[test]
fn worker_runner_durable_fails_when_node_is_older_than_22_19_0() {
    for old in ["v22.18.9", "v20.11.1"] {
        let fx = healthy();
        write_config_durable(&fx, "", true);
        add_runner_file(&fx);
        script(
            &fx,
            "node",
            |call| call.args == ["--version"],
            Reply::ok(&format!("{old}\n")),
        );
        let run = doctor(&fx);
        assert_doctor_failed(&run);
        let c = check(&run, "worker_runner");
        assert_eq!(
            (c["ok"].clone(), c["severity"].clone()),
            (json!(false), json!("error")),
            "{old}: {c}"
        );
        assert_eq!(
            (c["found"].as_str(), c["expected"].as_str()),
            (Some(&old[1..]), Some(">= 22.19.0")),
            "{old}: {c}"
        );
        assert!(
            c["message"].as_str().is_some_and(|m| m.contains("22.19.0")),
            "{old}: {c}"
        );
    }
}

// ------------------------------------------------------------ host_runner

/// A live durable Session (`runner = 'durable'`, `archived_at` null) of `role` (`lead` or `manager`), with a Context
/// for it: the row `host_runner` must see under an otherwise-`pi` config (contract §6.19).
fn live_durable_session(env: &TestEnv, role: &str) {
    // A Lead is the one Lead of a Plan (`sessions_one_lead`); a Manager has none.
    let (plan, plan_row) = if role == "lead" {
        (
            "'pay'",
            "INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pay', 'ctx', 'Pay', 'grounding', 'durable-lead', 't');",
        )
    } else {
        ("NULL", "")
    };
    open_store(env)
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Context', 't', 'hs');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, runner)
                   VALUES ('durable-{role}', '{role}', 'ctx', {plan}, '{role}', '/repo', 'O3', 'idle', 't', 't', 'durable');
                 {plan_row}"
            ))?;
            Ok(())
        })
        .unwrap();
}

/// The durable host file the check looks for under the adapter `write_config` made.
fn host_file(fx: &Fx) -> std::path::PathBuf {
    fx.env.root().join("adapter/durable-host/main.ts")
}

#[test]
fn host_runner_durable_fails_for_a_live_durable_lead_under_pi_config() {
    let fx = healthy();
    live_durable_session(&fx.env, "lead");
    assert!(!host_file(&fx).exists());
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    let c = assert_fails(&run, "host_runner", "error", "main.ts");
    assert!(
        c["message"]
            .as_str()
            .unwrap()
            .contains(host_file(&fx).to_str().unwrap()),
        "names the file it looked for: {c}"
    );
}

#[test]
fn host_runner_durable_fails_for_a_live_durable_manager_under_pi_config() {
    let fx = healthy();
    live_durable_session(&fx.env, "manager");
    assert!(!host_file(&fx).exists());
    let run = doctor(&fx);
    assert_doctor_failed(&run);
    let c = assert_fails(&run, "host_runner", "error", "main.ts");
    assert!(
        c["message"]
            .as_str()
            .unwrap()
            .contains(host_file(&fx).to_str().unwrap()),
        "names the file it looked for: {c}"
    );
}
