//! Every consumer reads its Tier's model from the `[model]` table of the policy key of its row (contract §8.2, §8.3
//! "Models" (5), §9; commands.md §10.2, §14.2), and config.toml has no `[tiers]` any more (pro-d27). The built binary's
//! command tree, in this process (`Fx`), over the fixtures the existing tests of each consumer use: `Live`
//! (driver_open.rs) for a Worker's argv and the ladder, `job.rs`'s rows for the Job's Tier check, `session_titles.rs`
//! for the title writer, `context_distill.rs` for the distiller and `session show` for a Lead and a Manager.
//!
//! Deviation from the Job text, said once: a `Live` has one repo (`acme/pay`), so "two repos of one owner" is one test
//! per ref, with the repo files of other repos set to refs that must NOT be read. Nothing here needs a stub: every
//! consumer is driven through the CLI with the existing fakes (pi, node, tmux, git).

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::fx::Fx;
use common::pi::FakePi as DistillPi;
use common::session::FakePi as TitlePi;
use common::{TestEnv, refused};
use cpd_core::context::{By, Draft, Record};
use cpd_core::host::runner::Reply;
use cpd_core::ids::{ContextId, PlanId, RecordId};
use cpd_core::plan::{jobs, locked_fingerprint};
use serde_json::{Value, json};

use super::driver::{JOB, PLAN, fixture, store};
use super::driver_open::{Live, live};

// ---------- fixtures ----------

/// A `[model]` table of four refs `github-copilot/<prefix>-<letter>`.
fn table(prefix: &str) -> String {
    format!(
        "[model]\nO = \"{}\"\nS = \"{}\"\nH = \"{}\"\nG = \"{}\"\n",
        model_ref(prefix, "o"),
        model_ref(prefix, "s"),
        model_ref(prefix, "h"),
        model_ref(prefix, "g")
    )
}

fn model_ref(prefix: &str, letter: &str) -> String {
    format!("github-copilot/{prefix}-{letter}")
}

fn policy_file(env: &TestEnv, rel: &str, text: &str) {
    let path = env.cpd_home().join("policy").join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// `config.toml` as it is once `[tiers]` is gone: no models in it.
fn config_without_tiers(env: &TestEnv, extra: &str) {
    env.write_config(&format!(
        "version = 1\nmachine = \"m1\"\n{extra}[tmux]\nsocket = \"{}\"\n",
        env.tmux_socket()
    ));
}

/// A fake pi install (`pi` on `PATH`, `current-version`, the two entry points), as `job.rs` makes it.
fn pi_install(env: &TestEnv) {
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
}

/// The cached catalog of the refs `github-copilot/<prefix>-{o,s,h}`: `efforts` for O, S, H in order.
fn cache(env: &TestEnv, prefix: &str, efforts: [&str; 3]) {
    let rows: String = [("O", "o"), ("S", "s"), ("H", "h")]
        .iter()
        .zip(efforts)
        .map(|((letter, low), efforts)| {
            format!(
                "INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{}', '{efforts}', 't');",
                model_ref(prefix, low)
            )
        })
        .collect();
    fixture(env, &rows);
}

const LEVELS: &str = r#"{"O":["off","low","medium","high","xhigh","max"],"S":["off","low","medium","high","xhigh","max"],"H":["off","low","medium","high"],"G":["off"]}"#;

const ALL: [&str; 3] = ["[0,1,2,3,4,5]", "[0,1,2,3,4,5]", "[0,1,2,3]"];

// ---------- c1: config.toml has no [tiers] ----------

#[test]
fn c1_a_tiers_section_in_config_toml_is_config_invalid() {
    let fx = Fx::new();
    fx.env
        .write_config("version = 1\nmachine = \"hs\"\n[tiers]\nO = \"github-copilot/opus\"\n");
    let error = refused(&fx.cpd(&["config"]), "config_invalid", 1);
    assert_eq!(error["details"]["key"], "tiers", "{error}");
}

#[test]
fn c1_cpd_config_prints_no_tiers_object_and_no_tiers_source() {
    let fx = Fx::new();
    fx.env.write_config("version = 1\nmachine = \"hs\"\n");
    let out = fx.cpd(&["config"]).ok();
    assert!(out["config"].get("tiers").is_none(), "{out}");
    let sources: Vec<&String> = out["sources"]
        .as_object()
        .unwrap()
        .keys()
        .filter(|k| k.starts_with("tiers"))
        .collect();
    assert!(sources.is_empty(), "{sources:?}");
}

// ---------- c2: the Job's Tier check ----------

/// Context `ctx` of owner `acme`, Plan `pl` in `plan` with PRs `r1` and `r2` (repos `acme/r1` and `acme/r2`).
/// r1's S ref is cached with efforts `[0,1]`, r2's with `[0,1,2,3]`.
fn two_repos() -> Fx {
    let fx = Fx::with_ports();
    config_without_tiers(&fx.env, "");
    pi_install(&fx.env);
    policy_file(&fx.env, "acme/r1.toml", &table("r1"));
    policy_file(&fx.env, "acme/r2.toml", &table("r2"));
    cache(&fx.env, "r1", ["[0,1,2,3,4,5]", "[0,1]", "[0,1,2,3]"]);
    cache(&fx.env, "r2", ["[0,1,2,3,4,5]", "[0,1,2,3]", "[0,1,2,3]"]);
    fixture(
        &fx.env,
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Context', 'acme', 't', 'm1');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pl', 'ctx', 'Plan', 'plan', 'lead-pl', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead-pl', 'lead', 'ctx', 'pl', 'pl', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, created_txn, created_at)
           VALUES ('pl', 'r1', '/r1', 'r1', 'main', 'waiting', 'cpd/pl/r1', cpd_txn(), 't'),
                  ('pl', 'r2', '/r2', 'r2', 'main', 'waiting', 'cpd/pl/r2', cpd_txn(), 't');",
    );
    fx
}

fn add(fx: &Fx, pr: &str, tier: &str) -> common::Run {
    let spec = fx.env.root().join("spec.json");
    let body = json!({"goal": "Do it.", "context": "Because.", "touches": ["a.rs"], "acceptance": ["cargo test"], "out_of_scope": []});
    fs::write(&spec, body.to_string()).unwrap();
    fx.cpd(&[
        "job",
        "add",
        &format!("k-{pr}"),
        "--title",
        "T",
        "--tier",
        tier,
        "--spec",
        spec.to_str().unwrap(),
        "--workflow",
        pr,
        "--plan",
        "pl",
    ])
}

#[test]
fn c2_a_job_tier_is_checked_against_the_efforts_cached_for_its_repos_s_ref() {
    let fx = two_repos();
    // pi's catalog (the fake node) would allow S2 for every ref: only the cached efforts of the repo's own ref refuse it.
    fx.fake_node(Reply::ok(&format!("{LEVELS}\n")));
    let error = refused(&add(&fx, "r1", "S2"), "tier_invalid", 2);
    assert!(
        error.to_string().contains("github-copilot/r1-s"),
        "the error names r1's S model: {error}"
    );
    // The same Tier on r2, whose S ref is cached with efforts up to 3.
    let out = add(&fx, "r2", "S2").ok();
    assert_eq!(out["job"]["tier"], "S2", "{out}");
}

// ---------- c3: a Worker Attempt ----------

/// The argv of every pi the fake tmux was told to start (what follows `--`, from `pi` on).
fn pi_calls(live: &Live) -> Vec<Vec<String>> {
    live.fx
        .calls("tmux")
        .into_iter()
        .filter(|call| {
            call.args
                .iter()
                .any(|a| a == "new-session" || a == "new-window")
        })
        .filter_map(|call| {
            let after = call.args.iter().position(|a| a == "--")?;
            let argv = &call.args[after + 1..];
            let pi = argv.iter().position(|a| a == "pi")?;
            Some(argv[pi + 1..].to_vec())
        })
        .collect()
}

fn model_of(argv: &[String]) -> String {
    let at = argv
        .iter()
        .position(|a| a == "--model")
        .unwrap_or_else(|| panic!("no --model in {argv:?}"));
    argv[at + 1].clone()
}

/// The Worker's pi ends with `status`: its pane is dead, as `driver.rs` does it.
fn worker_exits(live: &Live, id: &str, status: u8) {
    let window = format!("{PLAN}--{id}");
    let pane = live
        .fx
        .panes()
        .into_iter()
        .find(|p| p.window_name == window)
        .unwrap_or_else(|| panic!("no pane in window {window}"));
    let out = live.fx.tmux(&[
        "__ctl",
        "pane-exit",
        &pane.pane,
        "--status",
        &status.to_string(),
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// `live`'s Plan `pay` (PR `repo` of the repo `acme/pay`, owner `acme`, Job `api` on `S2`), with no `[tiers]` in
/// its config and every policy file empty; `prefix`es are cached.
fn live_without_tiers(prefixes: &[&str]) -> Live {
    let live = live("");
    config_without_tiers(&live.fx.env, "");
    live.fx.fake_node(Reply::ok(&format!("{LEVELS}\n")));
    pi_install(&live.fx.env);
    for rel in ["host.toml", "acme/owner.toml", "acme/pay.toml"] {
        policy_file(&live.fx.env, rel, "");
    }
    for prefix in prefixes {
        cache(&live.fx.env, prefix, ALL);
    }
    live
}

/// The first Attempt's Worker started and ended at once; the model of its pi.
fn first_attempt_model(live: &Live) -> String {
    live.drive(JOB, |live, id, nap| {
        if nap == 1 {
            worker_exits(live, id, 4);
        }
    });
    let calls = pi_calls(live);
    model_of(&calls[0])
}

#[test]
fn c3_a_worker_runs_on_the_s_ref_of_its_repos_policy() {
    let live = live_without_tiers(&["repo", "owner", "mach", "other"]);
    policy_file(&live.fx.env, "host.toml", &table("mach"));
    policy_file(&live.fx.env, "acme/owner.toml", &table("owner"));
    policy_file(&live.fx.env, "acme/pay.toml", &table("repo"));
    // Another repo of the owner, with its own ref, is not this Job's key.
    policy_file(&live.fx.env, "acme/other.toml", &table("other"));
    assert_eq!(first_attempt_model(&live), "github-copilot/repo-s:medium");
}

#[test]
fn c3_a_worker_of_a_repo_with_no_model_runs_on_its_owners() {
    let live = live_without_tiers(&["owner", "mach"]);
    policy_file(&live.fx.env, "host.toml", &table("mach"));
    policy_file(&live.fx.env, "acme/owner.toml", &table("owner"));
    assert_eq!(first_attempt_model(&live), "github-copilot/owner-s:medium");
}

#[test]
fn c3_a_worker_of_a_repo_and_owner_with_no_model_runs_on_the_machines() {
    let live = live_without_tiers(&["mach"]);
    policy_file(&live.fx.env, "host.toml", &table("mach"));
    assert_eq!(first_attempt_model(&live), "github-copilot/mach-s:medium");
}

// ---------- c4: a Lead and a Manager ----------

/// Contexts `owned` (owner `acme`) and `bare` (none), each with a Lead on `O2` and a Manager on `O2`.
fn sessions() -> Fx {
    let fx = Fx::with_ports();
    config_without_tiers(&fx.env, "");
    fx.fake_node(Reply::ok(&format!("{LEVELS}\n")));
    pi_install(&fx.env);
    cache(&fx.env, "owner", ALL);
    cache(&fx.env, "mach", ALL);
    fixture(
        &fx.env,
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('owned', 'Owned', 'acme', 't', 'm1'), ('bare', 'Bare', NULL, 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at) VALUES
           ('lead-owned', 'lead', 'owned', 'p1', 'Lead owned', '/', 'O2', 'idle', 't', 't1'),
           ('mgr-owned', 'manager', 'owned', NULL, 'Mgr owned', '/', 'O2', 'idle', 't', 't2'),
           ('lead-bare', 'lead', 'bare', 'p2', 'Lead bare', '/', 'O2', 'idle', 't', 't3'),
           ('mgr-bare', 'manager', 'bare', NULL, 'Mgr bare', '/', 'O2', 'idle', 't', 't4');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES
           ('p1', 'owned', 'P1', 'grounding', 'lead-owned', 't'), ('p2', 'bare', 'P2', 'grounding', 'lead-bare', 't');",
    );
    policy_file(&fx.env, "host.toml", &table("mach"));
    policy_file(&fx.env, "acme/owner.toml", &table("owner"));
    fx
}

fn shown_model(fx: &Fx, id: &str) -> Value {
    fx.cpd(&["session", "show", id]).ok()["session"]["model"].clone()
}

#[test]
fn c4_a_lead_of_a_context_with_an_owner_has_the_o_ref_of_its_owners_policy() {
    let fx = sessions();
    assert_eq!(shown_model(&fx, "lead-owned"), "github-copilot/owner-o");
}

#[test]
fn c4_a_manager_of_a_context_with_an_owner_has_the_o_ref_of_its_owners_policy() {
    let fx = sessions();
    assert_eq!(shown_model(&fx, "mgr-owned"), "github-copilot/owner-o");
}

#[test]
fn c4_a_lead_of_a_context_with_no_owner_has_the_machine_layers_o_ref_then_the_built_ins() {
    let fx = sessions();
    assert_eq!(shown_model(&fx, "lead-bare"), "github-copilot/mach-o");
    policy_file(&fx.env, "host.toml", "");
    assert_eq!(
        shown_model(&fx, "lead-bare"),
        "github-copilot/claude-opus-5.5"
    );
}

#[test]
fn c4_a_manager_of_a_context_with_no_owner_has_the_machine_layers_o_ref() {
    let fx = sessions();
    assert_eq!(shown_model(&fx, "mgr-bare"), "github-copilot/mach-o");
}

// ---------- c5: the title writer and the distiller ----------

/// Leads of the Contexts `owned` and `bare`, each with a session file of a short conversation, and a pi that titles.
fn titles() -> (Fx, std::sync::Arc<TitlePi>) {
    let fx = Fx::with_ports();
    config_without_tiers(&fx.env, "");
    pi_install(&fx.env);
    fx.fake_node(Reply::ok(&format!("{LEVELS}\n")));
    let pi = TitlePi::install(&fx);
    pi.say(Reply::exit(0, "TITLE: A title\n", ""));
    cache(&fx.env, "owner", ALL);
    cache(&fx.env, "mach", ALL);
    let file: PathBuf = fx.env.root().join("lead.jsonl");
    let lines = [
        json!({"type": "session", "id": "lead"}),
        json!({"type": "message", "message": {"role": "user", "content": [{"type": "text", "text": "message 1"}]}}),
        json!({"type": "message", "message": {"role": "assistant", "content": "message 2"}}),
    ];
    fs::write(
        &file,
        lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    fixture(
        &fx.env,
        &format!(
            "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('owned', 'Owned', 'acme', 't', 'm1'), ('bare', 'Bare', NULL, 't', 'm1');
             INSERT INTO sessions (id, role, context, plan, name, file, cwd, tier, state, state_at, created_at) VALUES
               ('lead-owned', 'lead', 'owned', 'p1', 'Lead owned', '{f}', '/', 'O2', 'idle', 't', 't1'),
               ('lead-bare', 'lead', 'bare', 'p2', 'Lead bare', '{f}', '/', 'O2', 'idle', 't', 't3');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES
               ('p1', 'owned', 'P1', 'grounding', 'lead-owned', 't'), ('p2', 'bare', 'P2', 'grounding', 'lead-bare', 't');",
            f = file.display()
        ),
    );
    policy_file(&fx.env, "host.toml", &table("mach"));
    policy_file(&fx.env, "acme/owner.toml", &table("owner"));
    (fx, pi)
}

fn title_model(fx: &Fx, session: &str) -> String {
    fx.cpd(&["session", "retitle", session, "--width", "68"])
        .ok();
    let calls = fx.calls("pi");
    model_of(&calls.last().expect("pi was called").args)
}

#[test]
fn c5_the_title_writer_of_a_context_with_an_owner_runs_on_the_g_ref_of_its_owners_policy() {
    let (fx, _pi) = titles();
    assert_eq!(title_model(&fx, "lead-owned"), "github-copilot/owner-g:off");
}

#[test]
fn c5_the_title_writer_of_a_context_with_no_owner_runs_on_the_machine_layers_g_ref() {
    let (fx, _pi) = titles();
    assert_eq!(title_model(&fx, "lead-bare"), "github-copilot/mach-g:off");
}

/// A Context `ctx` (owner `acme`, or none) with one record, the policy files of `titles`, pi's catalog from the fake node.
fn distiller(owner: Option<&str>) -> (Fx, std::sync::Arc<DistillPi>) {
    let fx = Fx::with_ports();
    config_without_tiers(
        &fx.env,
        "lead_pi_packages = []\ncontext_curation_min_records = 1\n",
    );
    fx.fake_node(Reply::ok(&format!("{LEVELS}\n")));
    pi_install(&fx.env);
    let pi = DistillPi::install(&fx);
    policy_file(&fx.env, "host.toml", &table("mach"));
    policy_file(&fx.env, "acme/owner.toml", &table("owner"));
    match owner {
        Some(owner) => {
            fx.cpd(&["context", "new", "ctx", "--title", "Ctx", "--owner", owner])
                .ok();
        }
        None => fixture(
            &fx.env,
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');",
        ),
    }
    let record = Record {
        id: RecordId::parse("hs-d1").unwrap(),
        at: "2020-01-01T00:00:00.000Z".into(),
        by: By {
            session: None,
            plan: None,
        },
        text: "Use sqlite.".into(),
        draft: Draft::Decision { supersedes: None },
    };
    store(&fx.env)
        .write("fixture", |tx| {
            Ok(record
                .insert(tx, &ContextId::parse("ctx").unwrap())
                .unwrap())
        })
        .unwrap();
    (fx, pi)
}

fn distill_model(fx: &Fx) -> String {
    fx.cpd(&["context", "distill", "--context", "ctx"]).ok();
    model_of(&fx.calls("pi")[0].args)
}

#[test]
fn c5_the_distiller_of_a_context_with_an_owner_runs_on_the_g_ref_of_its_owners_policy() {
    let (fx, _pi) = distiller(Some("acme"));
    assert_eq!(distill_model(&fx), "github-copilot/owner-g:off");
}

#[test]
fn c5_the_distiller_of_a_context_with_no_owner_runs_on_the_machine_layers_g_ref() {
    let (fx, _pi) = distiller(None);
    assert_eq!(distill_model(&fx), "github-copilot/mach-g:off");
}

// ---------- c6: the ladder ----------

/// Job `hi` of PR `repo` on `H3`, queued with its fingerprint, as `driver.rs`'s `add_job_in` queues one.
fn h3_job(live: &Live) {
    fixture(
        &live.fx.env,
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
         VALUES ('pay', 'repo', 'hi', 'Title of hi', 'H3', 'Do hi.', 'Because.', '[\"hi.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't');",
    );
    let plan = PlanId::parse(PLAN).unwrap();
    let all = store(&live.fx.env)
        .read(|conn| jobs::load(conn, &plan))
        .unwrap();
    let job = all.iter().find(|j| j.key == "hi").unwrap();
    fixture(
        &live.fx.env,
        &format!(
            "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = 'pay' AND key = 'hi';",
            locked_fingerprint(job)
        ),
    );
}

fn crash_the_worker(live: &Live) -> String {
    live.drive("hi", |live, id, nap| {
        if nap == 1 {
            worker_exits(live, id, 4);
        }
    })
    .0
}

#[test]
fn c6_a_crash_at_h3_retries_at_h3_of_the_same_repos_model() {
    let live = live_without_tiers(&["repo", "owner", "other"]);
    policy_file(&live.fx.env, "acme/owner.toml", &table("owner"));
    policy_file(&live.fx.env, "acme/pay.toml", &table("repo"));
    policy_file(&live.fx.env, "acme/other.toml", &table("other"));
    h3_job(&live);

    crash_the_worker(&live);
    crash_the_worker(&live);

    let calls = pi_calls(&live);
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(model_of(&calls[0]), "github-copilot/repo-h:high");
    assert_eq!(model_of(&calls[1]), "github-copilot/repo-h:high");
}

#[test]
fn c6_a_crash_at_h3_is_retried_at_h3_even_when_the_repo_has_no_s_effort_from_1_up() {
    let live = live_without_tiers(&["owner"]);
    policy_file(&live.fx.env, "acme/pay.toml", &table("repo"));
    cache(&live.fx.env, "repo", ["[0,1,2,3,4,5]", "[0]", "[0,1,2,3]"]);
    h3_job(&live);

    crash_the_worker(&live);

    let retries: Vec<Value> = live
        .events("hi-a1")
        .into_iter()
        .filter(|e| e["kind"] == "attempt-retry")
        .collect();
    assert_eq!(retries.len(), 1, "{retries:?}");
    assert_eq!(retries[0]["data"]["next_tier"], "H3");
    assert_eq!(live.job_status("hi"), "queued");
}
