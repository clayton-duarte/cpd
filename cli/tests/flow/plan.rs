//! Plans (contract §2.3, §3.1; commands.md §4, §15.5 and §19 amendments 6
//! and 8), run in-process (`Fx::with_ports`) against a temporary `$CPD_HOME`: git and tmux are the fake routes of the
//! `FakeRunner`, and `cpd watch --ensure` (`plan lock`) is a scripted reply of it.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::fx::Fx;
use common::{Run, TestEnv, refused};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::Reply;
use cpd_core::ids::PlanId;
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

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

/// A draft Job `key` of `plan` needing `needs`, written as `job add` would in phase `plan`.
fn add_job(env: &TestEnv, plan: &str, key: &str, needs: &[&str]) {
    add_draft(env, plan, key, needs, false);
}

/// A draft Job `key` of `plan` needing `needs`, `appended` as `job add --draft` writes it after the lock.
fn add_draft(env: &TestEnv, plan: &str, key: &str, needs: &[&str], appended: bool) {
    store(env)
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, created_txn, created_at)
                     VALUES ('{plan}', (SELECT key FROM workflows WHERE plan = '{plan}'), '{key}', 'Job {key}', 'S2', 'Goal of {key}.', 'Context of {key}.', '[\"src/{key}.rs\"]',
                             '[\"cargo test\"]', '[]', 'draft', {appended}, {id}, 't');",
                    appended = i32::from(appended),
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
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "UPDATE jobs SET goal = '{goal}' WHERE plan = '{plan}' AND key = '{key}';"
                ),
            )
        })
        .unwrap();
}

fn sha256(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// The `## Workflow <key>` section of a render: from its heading to the next `## ` one.
fn section<'a>(text: &'a str, pr: &str) -> &'a str {
    let start = text
        .find(&format!("\n## Workflow {pr}\n"))
        .unwrap_or_else(|| panic!("no PR {pr} section:\n{text}"));
    let rest = &text[start..];
    let end = rest[1..].find("\n## ").map_or(rest.len(), |i| i + 1);
    &rest[..end]
}

/// The fixtures of the migrated tests, on `Fx::with_ports()`: every external (tmux, git, `cpd watch --ensure`) is a
/// route or a rule of the `FakeRunner`. The tests not yet migrated use the binary and the helpers above.
mod inproc {
    use super::*;

    /// What `cpd watch --ensure` prints when the engine starts.
    pub const WATCH_OK: &str = r#"{"ok": true, "started": true, "pane": "%9"}"#;

    /// What it prints (exit 1) when it cannot take its lock.
    pub const WATCH_FAILS: &str = r#"{"ok": false, "error": {"code": "watch_start_failed", "message": "No lock.", "details": {}}}"#;

    /// Scripts the reply of `cpd watch --ensure` (`plan lock` runs it, `plan/lock.rs`): the first rule that matches
    /// answers, so a test that changes the reply scripts it with [`watch_once`] before this.
    pub fn watch(fx: &Fx, reply: Reply) {
        let _ = (*fx.fakes.runner).clone().on("cpd", is_watch, reply);
    }

    /// [`watch`] for the next call only.
    pub fn watch_once(fx: &Fx, reply: Reply) {
        let _ = (*fx.fakes.runner).clone().once("cpd", is_watch, reply);
    }

    fn is_watch(call: &cpd_core::host::runner::Call) -> bool {
        call.args == ["watch", "--ensure"]
    }

    /// The env with its config, Context `ctx` and the repo `pay`.
    pub fn setup(fx: &Fx) -> PathBuf {
        fx.env.write_tmux_config("");
        fx.cpd(&[
            "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
        ])
        .ok();
        fx.keyed_repo("pay")
    }

    /// [`setup`], and `cpd watch --ensure` answers [`WATCH_OK`]: for a test that locks a Plan.
    pub fn locking(fx: &Fx) -> PathBuf {
        let repo = setup(fx);
        watch(fx, Reply::ok(WATCH_OK));
        repo
    }

    pub fn events(fx: &Fx, kind: &str) -> Vec<Value> {
        fx.cpd(&["events", "list", "--kind", kind]).ok()["events"]
            .as_array()
            .unwrap()
            .clone()
    }

    /// `plan new` from the test root, with no PR.
    pub fn new_plan_only(fx: &Fx, id: &str) -> Value {
        fx.cpd(&[
            "plan",
            "new",
            id,
            "--context",
            "ctx",
            "--title",
            &format!("Plan {id}"),
        ])
        .ok()
    }

    /// `plan new` and the Plan's one PR, keyed `pay` (R34 A12: a Plan has no repo of its own).
    pub fn new_plan(fx: &Fx, id: &str, repo: &Path) -> Value {
        let out = new_plan_only(fx, id);
        fx.github_origin(repo);
        // `--shape config`: the template's stub drafts, `pay-config` and its opener, which these tests do not use:
        // they are cancelled, as a Lead cancels a stub it does not need.
        fx.cpd(&[
            "workflow",
            "add",
            "pay",
            "--repo",
            repo.to_str().unwrap(),
            "--title",
            "Change the PR",
            "--shape",
            "config",
            "--plan",
            id,
        ])
        .ok();
        fx.cpd(&[
            "job",
            "cancel",
            "pay-config",
            "--reason",
            "a stub of the template",
            "--plan",
            id,
        ])
        .ok();
        fx.cpd(&[
            "job",
            "cancel",
            "open-pay",
            "--reason",
            "a stub of the template",
            "--plan",
            id,
        ])
        .ok();
        fx.drop_unreachable_origin(repo);
        out
    }

    pub fn show(fx: &Fx, plan: &str) -> Value {
        fx.cpd(&["plan", "show", "--plan", plan]).ok()
    }

    /// The panes of the fake server: `(pane, window name, @cpd_session)`.
    pub fn panes(fx: &Fx) -> Vec<(String, String, String)> {
        fx.panes()
            .into_iter()
            .map(|p| (p.pane, p.window_name, p.session))
            .collect()
    }

    /// A Plan `id` in phase `plan` with draft Jobs `a` and `b` (needs `a`).
    pub fn planned(fx: &Fx, repo: &Path, id: &str) {
        new_plan(fx, id, repo);
        fx.cpd(&["plan", "phase", "plan", "--plan", id]).ok();
        add_job(&fx.env, id, "a", &[]);
        add_job(&fx.env, id, "b", &["a"]);
    }

    /// A Plan `pay` in phase `plan` with an approved, current review.
    pub fn approved(fx: &Fx, repo: &Path) {
        planned(fx, repo, "pay");
        fx.outline_for_review("pay");
        fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
            .ok();
        fx.cpd(&["plan", "decide", "r1", "--approve", "--plan", "pay"])
            .ok();
    }

    /// `job add <key> --draft --title <t>` in Plan `pay`, a stub, with `extra` flags.
    pub fn stub(fx: &Fx, key: &str, extra: &[&str]) -> Value {
        let title = format!("Planned {key}");
        let mut args = vec![
            "job", "add", key, "--draft", "--title", &title, "--plan", "pay",
        ];
        args.extend(extra);
        fx.cpd(&args).ok()
    }

    /// The argv (after `cpd`) of every `cpd watch --ensure` run so far.
    pub fn watch_calls(fx: &Fx) -> Vec<Vec<String>> {
        fx.calls("cpd")
            .into_iter()
            .map(|c| c.args)
            .filter(|a| a[0] == "watch")
            .collect()
    }
}

// ---------- plan new ----------

#[test]
fn new_registers_the_plan_and_its_lead_and_starts_it_in_the_callers_cwd() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    fx.cpd(&[
        "context",
        "decide",
        "One engine per machine.",
        "--context",
        "ctx",
    ])
    .ok();
    let brief = fx.env.root().join("brief.md");
    fs::write(&brief, "Retry the failed payouts.\n").unwrap();
    // Not the repo, not the root: the directory the User runs `plan new` from.
    let home_dir = fx.env.root().join("home");
    fs::create_dir_all(&home_dir).unwrap();
    let cwd = home_dir.canonicalize().unwrap().display().to_string();

    let args = [
        "plan",
        "new",
        "pay",
        "--context",
        "ctx",
        "--title",
        "Retry payouts",
        "--jira",
        "PAY-1",
        "--brief-file",
        brief.to_str().unwrap(),
    ];
    let out = fx.cpd_with(&home_dir, &args, &[]).ok();
    let session = out["plan"]["lead_session"].as_str().unwrap().to_owned();
    assert_eq!(
        out["plan"],
        json!({"id": "pay", "context": "ctx", "title": "Retry payouts", "jira": "PAY-1", "phase": "grounding", "lead_session": session, "workflows": []})
    );
    assert_eq!(
        (
            &out["lead"]["session"],
            &out["lead"]["name"],
            &out["lead"]["forked_from"]
        ),
        (&json!(session), &json!("pay"), &Value::Null)
    );
    assert_eq!(out["lead"]["cwd"], cwd);
    assert_eq!(out["record"], "m1-p1");
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM workflows"),
        0,
        "a new Plan has no PR"
    );
    let (pane, window) = (
        out["lead"]["pane"].as_str().unwrap(),
        out["lead"]["window"].as_str().unwrap(),
    );

    // The Lead: stopped, in its parked window, with its pane recorded, in the caller's cwd.
    let lead = fx.cpd(&["session", "show", &session]).ok()["session"].clone();
    assert_eq!(
        (&lead["role"], &lead["plan"], &lead["state"], &lead["tier"]),
        (
            &json!("lead"),
            &json!("pay"),
            &json!("stopped"),
            &json!("O2")
        )
    );
    assert_eq!(
        (&lead["cwd"], &lead["tmux_pane"], &lead["place"]),
        (&json!(cwd), &json!(pane), &json!("parked"))
    );
    assert_eq!(
        inproc::panes(&fx),
        vec![(pane.to_owned(), "park-pay".to_owned(), session.clone())],
        "the cockpit is created with the Lead's window"
    );
    // What cpd asked tmux for (`-c` and `-e` of `new-session`), read from the route (a real pane would inherit it).
    let state = fx
        .tmux_state()
        .expect("plan new started the cockpit server");
    let started = state
        .panes()
        .into_iter()
        .find(|p| p.id == pane)
        .expect("the Lead's pane");
    assert_eq!(started.cwd, cwd, "the pane starts in the caller's cwd");

    // The pane runs `cpd session run <id>` with the Plan's variables (§15.5).
    let pane_vars = state.pane_env(pane);
    for (key, value) in [
        ("CPD_HOME", fx.env.cpd_home().display().to_string()),
        ("CPD_SESSION", session.clone()),
        ("CPD_PLAN", "pay".to_owned()),
    ] {
        assert!(
            pane_vars.contains(&(key.to_owned(), value.clone())),
            "{key}={value} not in {pane_vars:?}"
        );
    }
    assert_eq!(
        started.argv[started.argv.len() - 3..],
        ["session", "run", session.as_str()]
    );

    // Events: plan-created and session-registered together, then the record, then the window.
    let created = &inproc::events(&fx, "plan-created")[0];
    assert_eq!(
        created["data"],
        json!({"title": "Retry payouts", "jira": "PAY-1", "lead_session": session, "forked_from": null, "cwd": cwd})
    );
    let registered = inproc::events(&fx, "session-registered");
    assert_eq!(registered.len(), 1);
    assert_eq!(
        (&registered[0]["plan"], &registered[0]["session"]),
        (&json!("pay"), &json!(session))
    );
    assert_eq!(
        registered[0]["data"],
        json!({"role": "lead", "name": "pay", "forked_from": null, "tier": "O2"})
    );
    assert_eq!(
        registered[0]["id"].as_i64().unwrap(),
        created["id"].as_i64().unwrap() + 1
    );
    let moved = inproc::events(&fx, "session-moved");
    assert_eq!(
        moved[0]["data"],
        json!({"pane": pane, "window": window, "place": "parked"})
    );

    // The `plan` record in the Context store: no `repo`; `context show` lists the Plan's PR repos.
    let shown = fx.cpd(&["context", "show", "--context", "ctx"]).ok();
    let record = shown["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "plan")
        .unwrap()
        .clone();
    assert_eq!(record["text"], "Retry payouts");
    assert_eq!(
        record["plan"],
        json!({"id": "pay", "machine": "m1", "jira": "PAY-1"})
    );
    assert_eq!(record["repos"], json!([]), "no PR yet");
    assert!(!record.to_string().contains("\"repo\""), "{record}");
    assert!(
        !fx.env.cpd_home().join("contexts").exists(),
        "the record is in the database only"
    );
    let real = repo.canonicalize().unwrap().display().to_string();
    let other = fx.keyed_repo("web");
    fx.cpd(&[
        "workflow",
        "add",
        "api",
        "--repo",
        &real,
        "--title",
        "Change the PR",
        "--shape",
        "config",
        "--plan",
        "pay",
    ])
    .ok();
    fx.cpd(&[
        "workflow",
        "add",
        "api2",
        "--repo",
        &real,
        "--title",
        "Change the PR",
        "--shape",
        "config",
        "--plan",
        "pay",
    ])
    .ok();
    fx.cpd(&[
        "workflow",
        "add",
        "web",
        "--repo",
        other.to_str().unwrap(),
        "--title",
        "Change the PR",
        "--shape",
        "config",
        "--plan",
        "pay",
    ])
    .ok();
    fx.cpd(&[
        "workflow",
        "add",
        "gone",
        "--repo",
        fx.keyed_repo("old").to_str().unwrap(),
        "--title",
        "Change the PR",
        "--shape",
        "config",
        "--plan",
        "pay",
    ])
    .ok();
    fx.cpd(&[
        "job",
        "cancel",
        "gone-config",
        "--reason",
        "a stub of the template",
        "--plan",
        "pay",
    ])
    .ok();
    fx.cpd(&["workflow", "cancel", "gone", "--plan", "pay"])
        .ok();
    let shown = fx.cpd(&["context", "show", "--context", "ctx"]).ok();
    let record = shown["records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "plan")
        .unwrap()
        .clone();
    let web = other.canonicalize().unwrap().display().to_string();
    assert_eq!(
        record["repos"],
        json!([real, web]),
        "each repo once, in PR key order, no cancelled PR"
    );
    assert!(
        shown["markdown"].as_str().unwrap().contains(&format!(
            "pay (on m1, repos {real}, {web}, PAY-1): Retry payouts"
        )),
        "{shown}"
    );

    // The first message: the header, the brief, then the pointer line; no record is listed (#418, docs/commands.md §4.1 step 3).
    let message = fs::read_to_string(home(&fx.env).lead_message(&pid("pay"))).unwrap();
    let header = "You are the Lead of Plan pay \"Retry payouts\" in Context ctx. Phase grounding: follow the cpd-lead skill.\n\nRetry the failed payouts.\n\n";
    assert!(message.starts_with(header), "{message}");
    assert!(message.contains("Context on demand: cpd context search <query> · cpd context show --since <seq> · cpd context show --record <id>"), "{message}");
    assert!(
        !message.contains("One engine per machine."),
        "no record is listed: {message}"
    );
}

/// G11: `set -p … @cpd_session` fails after `new-window`: the window is killed, the error returned.
#[test]
fn new_leaves_no_window_when_a_step_after_new_window_fails() {
    let fx = Fx::with_ports();
    inproc::setup(&fx);
    inproc::new_plan_only(&fx, "first");
    let windows = || {
        String::from_utf8(
            fx.tmux(&["list-windows", "-a", "-F", "#{window_name}"])
                .stdout,
        )
        .unwrap()
    };
    let before = windows();
    assert!(before.contains("park-first"), "{before}");

    // The route answers every other call; this one rule refuses `set … @cpd_session` (it comes before the route).
    let _ = (*fx.fakes.runner).clone().on(
        "tmux",
        |call| {
            call.args.iter().any(|a| a == "set") && call.args.iter().any(|a| a == "@cpd_session")
        },
        Reply::exit(1, "", "refused\n"),
    );
    let run = fx.cpd(&[
        "plan",
        "new",
        "second",
        "--context",
        "ctx",
        "--title",
        "Plan second",
    ]);

    assert_eq!(run.error()["code"], "tmux_failed", "{run:?}");
    assert_eq!(windows(), before, "plan new left a window");
}

/// `plan new` with a brief file whose text is `text`.
fn new_with_brief(fx: &Fx, id: &str, text: &str) -> Run {
    let file = fx.env.root().join(format!("{id}-brief.md"));
    fs::write(&file, text).unwrap();
    fx.cpd(&[
        "plan",
        "new",
        id,
        "--context",
        "ctx",
        "--title",
        &format!("Plan {id}"),
        "--brief-file",
        file.to_str().unwrap(),
    ])
}

fn briefs(fx: &Fx) -> Vec<Value> {
    let shown = fx.cpd(&["context", "show", "--context", "ctx"]).ok();
    shown["records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["kind"] == "brief")
        .cloned()
        .collect()
}

#[test]
fn new_with_a_brief_file_writes_one_brief_record_and_the_lead_gets_it_in_full() {
    let fx = Fx::with_ports();
    inproc::setup(&fx);
    let text = "Retry the failed payouts.\n\nMore detail, second paragraph.\n";
    let out = new_with_brief(&fx, "pay", text).ok();
    assert_eq!(
        out["record"], "m1-p1",
        "the output still names the plan record"
    );

    // One `brief` record, letter `b`, its Plan `{id, machine}`, the file's text, one `context-record` Event.
    let rows: Vec<(String, String, String, Option<String>)> = store(&fx.env)
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT id, text, plan_id || '/' || plan_machine, plan_jira FROM context_records WHERE kind = 'brief'")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap();
    assert_eq!(
        rows,
        [(
            "m1-b1".to_owned(),
            text.to_owned(),
            "pay/m1".to_owned(),
            None
        )]
    );
    let recorded: Vec<Value> = inproc::events(&fx, "context-record")
        .into_iter()
        .filter(|e| e["data"]["kind"] == "brief")
        .collect();
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    assert_eq!(recorded[0]["data"]["record"], "m1-b1");

    // `context show` lists it short, under its Plan; `--record` prints it whole.
    let shown = fx.cpd(&["context", "show", "--context", "ctx"]).ok();
    let brief = &briefs(&fx)[0];
    assert_eq!(
        (&brief["text"], &brief["short"], &brief["plan"]),
        (
            &json!("Retry the failed payouts."),
            &json!(true),
            &json!({"id": "pay", "machine": "m1"})
        )
    );
    let md = shown["markdown"].as_str().unwrap();
    assert!(
        md.contains("  - brief m1-b1 (") && md.contains("): Retry the failed payouts.\n"),
        "{md}"
    );
    assert!(!md.contains("second paragraph"), "{md}");
    let full = fx
        .cpd(&["context", "show", "--context", "ctx", "--record", "m1-b1"])
        .ok();
    assert_eq!(full["records"][0]["text"], text);
    assert!(full["records"][0].get("short").is_none());
    assert!(
        full["markdown"]
            .as_str()
            .unwrap()
            .contains("second paragraph")
    );

    // The Lead's first message: header, the full brief, then the pointer line; its own brief not listed twice.
    let message = fs::read_to_string(home(&fx.env).lead_message(&pid("pay"))).unwrap();
    let header = "You are the Lead of Plan pay \"Plan pay\" in Context ctx. Phase grounding: follow the cpd-lead skill.\n\nRetry the failed payouts.\n\nMore detail, second paragraph.\n\n";
    assert!(message.starts_with(header), "{message}");
    assert_eq!(
        message.matches("Retry the failed payouts.").count(),
        1,
        "{message}"
    );

    // Another Plan's Lead is pointed at the Context and is not given this brief, short or full (#418).
    new_with_brief(&fx, "web", "Build the web page.\nSecond line.\n").ok();
    let message = fs::read_to_string(home(&fx.env).lead_message(&pid("web"))).unwrap();
    assert!(message.contains("Context on demand: cpd context search <query> · cpd context show --since <seq> · cpd context show --record <id>"), "{message}");
    assert!(
        !message.contains("[m1-b1] brief") && !message.contains("Retry the failed payouts."),
        "{message}"
    );
    assert!(!message.contains("second paragraph"), "{message}");
    assert_eq!(
        message.matches("Build the web page.").count(),
        1,
        "{message}"
    );
    assert_eq!(briefs(&fx).len(), 2);
}

#[test]
fn new_again_with_the_brief_writes_no_second_brief_record() {
    let fx = Fx::with_ports();
    inproc::setup(&fx);
    new_with_brief(&fx, "pay", "The brief.\n").ok();
    let txns: i64 = fx.env.query("SELECT count(*) FROM txns");
    let again = new_with_brief(&fx, "pay", "The brief.\n").ok();
    assert_eq!(again["already"], true);
    assert_eq!(briefs(&fx).len(), 1);
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM txns"),
        txns,
        "nothing written"
    );

    // A first run that stopped before the brief record (here: run without the file): the rerun writes it, once.
    inproc::new_plan_only(&fx, "web");
    assert_eq!(briefs(&fx).len(), 1);
    assert_eq!(
        new_with_brief(&fx, "web", "The web brief.\n").ok()["already"],
        true
    );
    assert_eq!(
        new_with_brief(&fx, "web", "The web brief.\n").ok()["already"],
        true
    );
    assert_eq!(briefs(&fx).len(), 2);

    // A Plan without a brief file has no brief record.
    inproc::new_plan_only(&fx, "api");
    assert_eq!(briefs(&fx).len(), 2);
}

// ---------- the first message: the digest and the records after it (hs-d194 D6) ----------

#[test]
fn new_refuses_an_empty_brief_file_and_writes_nothing() {
    let fx = Fx::with_ports();
    inproc::setup(&fx);
    let txns: i64 = fx.env.query("SELECT count(*) FROM txns");
    refused(&new_with_brief(&fx, "pay", ""), "usage", 2);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM txns"), txns);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plans"), 0);
}

#[test]
fn new_takes_neither_repo_nor_base() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    let repo_arg = repo.to_str().unwrap();
    let txns: i64 = fx.env.query("SELECT count(*) FROM txns");
    for extra in [["--repo", repo_arg], ["--base", "main"]] {
        let mut args = vec!["plan", "new", "pay", "--context", "ctx", "--title", "T"];
        args.extend(extra);
        refused(&fx.cpd(&args), "usage", 2);
    }
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM txns"), txns);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plans"), 0);
    assert!(inproc::panes(&fx).is_empty());
}

#[test]
fn new_again_with_the_same_flags_is_already_and_redoes_missing_steps() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    let first = inproc::new_plan_only(&fx, "pay");
    let session = first["plan"]["lead_session"].as_str().unwrap().to_owned();
    let txns: i64 = fx.env.query("SELECT count(*) FROM txns");

    // From another directory: still `already`, and the stored cwd stays.
    let again = fx
        .cpd_in(
            &repo,
            &[
                "plan",
                "new",
                "pay",
                "--context",
                "ctx",
                "--title",
                "Plan pay",
            ],
        )
        .ok();
    assert_eq!(again["already"], true);
    assert_eq!(
        (&again["plan"], &again["record"]),
        (&first["plan"], &first["record"])
    );
    assert_eq!(
        (&again["lead"]["pane"], &again["lead"]["window"]),
        (&first["lead"]["pane"], &first["lead"]["window"])
    );
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM txns"),
        txns,
        "nothing written"
    );
    assert_eq!(inproc::panes(&fx).len(), 1);

    // A crash after the first transaction: no pane, no message. Rerunning redoes both.
    store(&fx.env)
        .write("fixture", |tx| {
            exec(
                tx,
                &format!("UPDATE sessions SET tmux_pane = NULL WHERE id = '{session}';"),
            )
        })
        .unwrap();
    fs::remove_file(home(&fx.env).lead_message(&pid("pay"))).unwrap();
    fx.kill_tmux_server();
    let redone = fx
        .cpd_in(
            &repo,
            &[
                "plan",
                "new",
                "pay",
                "--context",
                "ctx",
                "--title",
                "Plan pay",
            ],
        )
        .ok();
    assert_eq!(redone["already"], true);
    assert_eq!(redone["lead"]["cwd"], first["lead"]["cwd"]);
    let pane = redone["lead"]["pane"].as_str().unwrap();
    let state = fx.tmux_state().expect("the rerun started the server again");
    let redone_pane = state
        .panes()
        .into_iter()
        .find(|p| p.id == pane)
        .expect("the redone Lead's pane");
    assert_eq!(
        redone_pane.cwd,
        first["lead"]["cwd"].as_str().unwrap(),
        "redone in the stored cwd"
    );
    assert_ne!(redone["lead"]["pane"], Value::Null);
    assert_eq!(inproc::panes(&fx).len(), 1);
    assert!(home(&fx.env).lead_message(&pid("pay")).exists());
    assert_eq!(inproc::events(&fx, "session-moved").len(), 2);
    assert_eq!(inproc::events(&fx, "plan-created").len(), 1);

    // Other flags: plan_exists.
    let error = refused(
        &fx.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Other"]),
        "plan_exists",
        3,
    );
    assert_eq!(error["details"]["plan"]["title"], "Plan pay");
    refused(
        &fx.cpd(&[
            "plan",
            "new",
            "pay",
            "--context",
            "ctx",
            "--title",
            "Plan pay",
            "--name",
            "x",
        ]),
        "plan_exists",
        3,
    );
}

#[test]
fn new_again_in_an_archived_context_is_already_not_archived() {
    let fx = Fx::with_ports();
    inproc::setup(&fx);
    let first = inproc::new_plan_only(&fx, "pay");
    fx.cpd(&["context", "archive", "ctx"]).ok();
    let again = fx
        .cpd(&[
            "plan",
            "new",
            "pay",
            "--context",
            "ctx",
            "--title",
            "Plan pay",
        ])
        .ok();
    assert_eq!(again["already"], true);
    assert_eq!(again["plan"], first["plan"]);
    // A new Plan, and a rerun with other flags, are still refused in this order.
    refused(
        &fx.cpd(&["plan", "new", "other", "--context", "ctx", "--title", "T"]),
        "archived",
        3,
    );
    refused(
        &fx.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Other"]),
        "plan_exists",
        3,
    );
}

#[test]
fn new_and_rename_titles_are_1_to_200_characters_with_no_control_characters() {
    let fx = Fx::with_ports();
    inproc::setup(&fx);
    let (long, ok) = ("a".repeat(201), "a".repeat(200));
    let txns: i64 = fx.env.query("SELECT count(*) FROM txns");
    for title in [long.as_str(), "a\nb", "a\tb", "a\u{7f}b"] {
        refused(
            &fx.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", title]),
            "usage",
            2,
        );
        refused(
            &fx.cpd(&["plan", "rename", "--plan", "nope", "--title", title]),
            "usage",
            2,
        );
    }
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM txns"), txns);
    fx.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", &ok])
        .ok();
    for title in [long.as_str(), "a\nb", "a\tb"] {
        refused(
            &fx.cpd(&["plan", "rename", "--plan", "pay", "--title", title]),
            "usage",
            2,
        );
    }
    assert!(inproc::events(&fx, "plan-renamed").is_empty());
    let renamed = "b".repeat(200);
    assert_eq!(
        fx.cpd(&["plan", "rename", "--plan", "pay", "--title", &renamed])
            .ok()["plan"]["title"],
        renamed.as_str()
    );
}

#[test]
fn new_refusals_write_nothing() {
    let fx = Fx::with_ports();
    inproc::setup(&fx);
    let new = |extra: &[&str]| {
        let mut args = vec!["plan", "new", "pay", "--title", "T"];
        args.extend(extra);
        fx.cpd(&args)
    };
    let txns: i64 = fx.env.query("SELECT count(*) FROM txns");

    refused(&new(&["--context", "nope"]), "context_not_found", 4);
    refused(
        &new(&["--context", "ctx", "--brief-file", "/no/such/brief"]),
        "file_unreadable",
        2,
    );
    refused(
        &fx.cpd(&["plan", "new", "Bad", "--context", "ctx", "--title", "T"]),
        "invalid_plan_id",
        2,
    );
    refused(
        &fx.cpd(&["plan", "new", "pay", "--context", "ctx"]),
        "usage",
        2,
    );
    refused(&new(&["--context", "ctx", "--name", ""]), "invalid_name", 2);
    refused(&new(&["--context", "ctx", "--jira", "nope"]), "usage", 2);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM txns"), txns);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plans"), 0);
}

#[test]
fn new_in_a_context_with_no_owner_is_owner_unset_and_writes_nothing() {
    let fx = Fx::with_ports();
    inproc::setup(&fx);
    // A Context made before the policy keys has no owner.
    store(&fx.env)
        .write("test", |tx| {
            Ok(tx
                .execute("UPDATE contexts SET owner = NULL", [])
                .map(|_| ())?)
        })
        .unwrap();
    let txns: i64 = fx.env.query("SELECT count(*) FROM txns");
    let error = refused(
        &fx.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "T"]),
        "owner_unset",
        3,
    );
    assert_eq!(error["details"]["context"], "ctx");
    assert!(
        error["details"].get("next").is_none()
            && error["message"]
                .as_str()
                .unwrap()
                .contains("cpd context owner <login> --context ctx"),
        "{error}"
    );
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM txns"), txns);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plans"), 0);
    // After `context owner` the Plan is created.
    fx.cpd(&["context", "owner", "acme", "--context", "ctx"])
        .ok();
    inproc::new_plan_only(&fx, "pay");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plans"), 1);
}

#[test]
fn pr_add_defaults_the_base_to_origin_head_and_needs_it_on_origin() {
    let fx = Fx::with_ports();
    let origin = inproc::setup(&fx);
    fx.git(&origin, &["branch", "trunk"]);
    // The fake git route has no writing `symbolic-ref`: `switch` moves HEAD to `trunk` (the same commit, so the same tree).
    fx.git(&origin, &["switch", "-q", "trunk"]);
    let clone = fx.env.root().join("repos").join("clone");
    fx.git(
        fx.env.root(),
        &[
            "clone",
            "-q",
            origin.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    // Keyed by its GitHub URL; git still reaches the local repo.
    fx.git(
        &clone,
        &[
            "remote",
            "set-url",
            "origin",
            "https://github.com/acme/clone.git",
        ],
    );
    fx.env.repo_policy("acme", "clone", "");
    fx.git(
        &clone,
        &[
            "config",
            &format!("url.{}.insteadOf", origin.display()),
            "https://github.com/acme/clone.git",
        ],
    );
    // From a subdirectory of a linked worktree: the repo is the main worktree.
    let linked = fx.env.root().join("repos").join("clone-linked");
    fx.git(
        &clone,
        &[
            "worktree",
            "add",
            "-q",
            linked.to_str().unwrap(),
            "-b",
            "side",
        ],
    );
    fs::create_dir_all(linked.join("sub")).unwrap();

    inproc::new_plan_only(&fx, "pay");
    let out = fx
        .cpd(&[
            "workflow",
            "add",
            "pay",
            "--repo",
            linked.join("sub").to_str().unwrap(),
            "--title",
            "Change the PR",
            "--shape",
            "config",
            "--plan",
            "pay",
        ])
        .ok();
    assert_eq!(out["workflow"]["base"], "trunk");
    assert_eq!(
        out["workflow"]["repo"],
        clone.canonicalize().unwrap().display().to_string()
    );
    let error = refused(
        &fx.cpd(&[
            "workflow",
            "add",
            "other",
            "--repo",
            clone.to_str().unwrap(),
            "--title",
            "Change the PR",
            "--base",
            "side",
            "--shape",
            "config",
            "--plan",
            "pay",
        ]),
        "base_not_found",
        4,
    );
    assert_eq!(
        error["details"]["ref"], "refs/remotes/origin/side",
        "with an origin, the base must be on it"
    );
}

// ---------- plan phase ----------

#[test]
fn phase_plan_writes_the_phase_only_and_a_later_plan_is_a_fresh_lead() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    let first = inproc::new_plan(&fx, "pay", &repo);
    let lead = first["plan"]["lead_session"].as_str().unwrap().to_owned();
    let (sessions, windows, events_before) = (
        fx.env.query::<i64>("SELECT count(*) FROM sessions"),
        inproc::panes(&fx).len(),
        fx.env.query::<i64>("SELECT count(*) FROM events"),
    );

    let out = fx.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "phase": "plan"}),
        "no `grounded_lead`"
    );
    let phase = &inproc::events(&fx, "plan-phase")[0];
    assert_eq!(
        (&phase["plan"], &phase["data"]),
        (&json!("pay"), &json!({"from": "grounding", "to": "plan"}))
    );
    // No Session, no window, no Event but `plan-phase`; no `context-grounded` ever.
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM sessions"),
        sessions
    );
    assert_eq!(inproc::panes(&fx).len(), windows);
    assert!(inproc::panes(&fx).iter().all(|p| p.1 != "ctx-ctx"));
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM events"),
        events_before + 1
    );
    assert_eq!(inproc::events(&fx, "context-grounded").len(), 0);
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM sessions WHERE plan IS NULL"),
        0
    );

    // Already in phase plan: nothing is written.
    let again = fx.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    assert_eq!(
        again,
        json!({"ok": true, "plan": "pay", "phase": "plan", "already": true})
    );
    assert_eq!(inproc::events(&fx, "plan-phase").len(), 1);
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM events"),
        events_before + 1
    );

    // A later Plan of the Context is a fresh Lead, never a fork: its first message points at the records, it lists none.
    fx.cpd(&[
        "context",
        "note",
        "Recorded after the first Plan.",
        "--context",
        "ctx",
    ])
    .ok();
    let second = inproc::new_plan(&fx, "refund", &repo);
    assert_eq!(second["lead"]["forked_from"], Value::Null);
    let session = second["lead"]["session"].as_str().unwrap();
    assert_ne!(session, lead);
    assert_eq!(
        fx.env.query::<Option<String>>(&format!(
            "SELECT forked_from FROM sessions WHERE id = '{session}'"
        )),
        None
    );
    assert_eq!(
        inproc::events(&fx, "plan-created").last().unwrap()["data"]["forked_from"],
        Value::Null
    );
    assert_eq!(
        inproc::events(&fx, "session-registered").last().unwrap()["data"]["forked_from"],
        Value::Null
    );
    let message = fs::read_to_string(home(&fx.env).lead_message(&pid("refund"))).unwrap();
    assert!(message.starts_with("You are the Lead of Plan refund \"Plan refund\" in Context ctx. Phase grounding: follow the cpd-lead skill.\n\n"), "{message}");
    assert!(message.contains("Context on demand: cpd context search <query> · cpd context show --since <seq> · cpd context show --record <id>"), "{message}");
    assert!(
        !message.contains("Recorded after the first Plan.") && !message.contains("m1-n1"),
        "no record is listed: {message}"
    );
}

#[test]
fn phase_refusals() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    inproc::new_plan(&fx, "pay", &repo);
    let error = refused(
        &fx.cpd(&["plan", "phase", "dispatch", "--plan", "pay"]),
        "illegal_transition",
        3,
    );
    assert_eq!(
        (&error["details"]["from"], &error["details"]["to"]),
        (&json!("grounding"), &json!("dispatch"))
    );
    refused(
        &fx.cpd(&["plan", "phase", "grounding", "--plan", "pay"]),
        "illegal_transition",
        3,
    );
    refused(
        &fx.cpd(&["plan", "phase", "bogus", "--plan", "pay"]),
        "usage",
        2,
    );
    refused(&fx.cpd(&["plan", "phase", "plan"]), "plan_required", 2);
    refused(
        &fx.cpd(&["plan", "phase", "plan", "--plan", "nope"]),
        "plan_not_found",
        4,
    );
    store(&fx.env)
        .write("fixture", |tx| exec(tx, "UPDATE plans SET phase = 'plan' WHERE id = 'pay'; UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';
                 UPDATE workflows SET status = 'dispatch', lead_worktree = '/w', started_at = 't' WHERE plan = 'pay';"))
        .unwrap();
    let error = refused(
        &fx.cpd_with(
            fx.env.root(),
            &["plan", "phase", "plan"],
            &[("CPD_PLAN", "pay")],
        ),
        "illegal_transition",
        3,
    );
    assert_eq!(error["details"]["from"], "dispatch");
}

/// A Plan with no PR yet (`plan new` alone) is found by every Plan read: phase, list, show, archive and unarchive.
#[test]
fn a_plan_with_no_pr_is_read_by_phase_list_show_archive_and_unarchive() {
    let fx = Fx::with_ports();
    inproc::setup(&fx);
    inproc::new_plan_only(&fx, "pay");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM workflows"), 0);

    assert_eq!(inproc::show(&fx, "pay")["workflows"], json!([]));
    let ids = |fx: &Fx| -> Vec<String> {
        fx.cpd(&["plan", "list"]).ok()["plans"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(ids(&fx), ["pay"]);

    let archived = fx.cpd(&["plan", "archive", "--plan", "pay"]).ok();
    assert_eq!(
        archived["archived"][0],
        json!({"kind": "plan", "id": "pay"})
    );
    assert_ne!(inproc::show(&fx, "pay")["plan"]["archived_at"], Value::Null);
    fx.cpd(&["plan", "unarchive", "--plan", "pay"]).ok();
    assert_eq!(inproc::show(&fx, "pay")["plan"]["archived_at"], Value::Null);

    let out = fx.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    assert_eq!(
        (&out["plan"], &out["phase"]),
        (&json!("pay"), &json!("plan"))
    );
    assert_eq!(inproc::show(&fx, "pay")["plan"]["phase"], "plan");
    // A Plan with no PR has no Job: lock keeps its own refusal.
    refused(
        &fx.cpd(&["plan", "lock", "--plan", "pay"]),
        "no_draft_jobs",
        3,
    );
}

/// Plans moving to phase `plan` at once all move, and none registers a Session. Six threads call `run_with` on the
/// one store of the env behind a `Barrier` (the store is ours; the lock and the busy timeout are SQLite's, in-process).
#[test]
fn plans_moving_to_phase_plan_at_once_all_move() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    let plans = ["pa", "pb", "pc", "pd", "pe", "pf"];
    for plan in plans {
        inproc::new_plan(&fx, plan, &repo);
    }
    let sessions = fx.env.query::<i64>("SELECT count(*) FROM sessions");
    let barrier = std::sync::Barrier::new(plans.len());
    // The store's setup lock is a real `flock` the six threads take in turn; a waiter naps on the clock, and on the
    // `ManualClock` each nap is a virtual 20 ms, so a few real milliseconds of contention would read as the
    // 5 s `store_busy_ms`. These threads yield instead: no time passes, none is slept, and the wait ends with the holder.
    struct Yield;
    impl cpd_core::clock::Clock for Yield {
        fn now(&self) -> std::time::Instant {
            std::time::Instant::now()
        }
        fn sleep(&self, _: std::time::Duration) {
            std::thread::yield_now();
        }
        fn utc(&self) -> time::OffsetDateTime {
            time::OffsetDateTime::now_utc()
        }
    }
    let mut yielding = fx.fakes.ports.clone();
    yielding.clock = std::sync::Arc::new(Yield);
    let ports = &yielding;
    std::thread::scope(|scope| {
        let handles: Vec<_> = plans
            .iter()
            .map(|plan| {
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    let argv = ["cpd", "plan", "phase", "plan", "--plan", plan]
                        .map(std::ffi::OsString::from);
                    let mut stdout = Vec::new();
                    let code = cpd::run_with(ports, argv, &mut stdout);
                    (code, String::from_utf8(stdout).unwrap())
                })
            })
            .collect();
        for handle in handles {
            let (code, stdout) = handle.join().unwrap();
            assert_eq!(code, std::process::ExitCode::SUCCESS, "{stdout}");
        }
    });
    assert_eq!(
        fx.env.query::<i64>("SELECT count(*) FROM sessions"),
        sessions
    );
    assert_eq!(inproc::events(&fx, "context-grounded").len(), 0);
    assert_eq!(inproc::events(&fx, "plan-phase").len(), plans.len());
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM plans WHERE phase = 'plan'"),
        plans.len() as i64
    );
}

// ---------- plan list, plan show ----------

#[test]
fn list_and_show_are_snapshots_with_an_events_cursor() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    inproc::planned(&fx, &repo, "pay");
    inproc::new_plan(&fx, "refund", &repo);
    fx.cpd(&[
        "context", "new", "other", "--title", "Other", "--owner", "acme",
    ])
    .ok();

    let list = fx.cpd(&["plan", "list"]).ok();
    let cursor = list["events_cursor"].as_i64().unwrap();
    assert_eq!(
        cursor,
        fx.cpd(&["events", "list", "--since", "0", "--limit", "5000"])
            .ok()["next"]
            .as_i64()
            .unwrap()
    );
    let plans = list["plans"].as_array().unwrap();
    assert_eq!(plans.len(), 2);
    let pay = &plans[0];
    assert_eq!(
        (&pay["id"], &pay["context"], &pay["phase"]),
        (&json!("pay"), &json!("ctx"), &json!("plan"))
    );
    // The two cancelled are the stubs of the template `new_plan` cancels (`pay-config` and its opener).
    assert_eq!(
        pay["counts"],
        json!({"draft": 2, "queued": 0, "running": 0, "merged": 0, "failed": 0, "cancelled": 2})
    );
    assert_eq!(
        pay["workflows"][0]["repo"],
        repo.canonicalize().unwrap().display().to_string()
    );
    assert_eq!(
        (
            &pay["workflows"][0]["key"],
            &pay["workflows"][0]["base_workflow"]
        ),
        (&json!("pay"), &Value::Null)
    );
    let ids = |args: &[&str]| -> Vec<String> {
        fx.cpd(args).ok()["plans"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(ids(&["plan", "list", "--phase", "grounding"]), ["refund"]);
    assert_eq!(
        ids(&["plan", "list", "--context", "ctx", "--phase", "plan"]),
        ["pay"]
    );
    assert!(ids(&["plan", "list", "--context", "other"]).is_empty());
    assert_eq!(
        fx.cpd_with(fx.env.root(), &["plan", "list"], &[("CPD_PLAN", "pay")])
            .ok()["plans"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "filters only"
    );
    refused(&fx.cpd(&["plan", "list", "--phase", "bogus"]), "usage", 2);

    let out = inproc::show(&fx, "pay");
    let plan = &out["plan"];
    assert_eq!(
        (&plan["id"], &plan["phase"]),
        (&json!("pay"), &json!("plan"))
    );
    let pr = &out["workflows"][0];
    assert_eq!(
        (&pr["lead_worktree"], &pr["base_workflow"]),
        (&Value::Null, &Value::Null)
    );
    assert_eq!(pr["pull_request"], Value::Null);
    // Not the stub of the template, which `new_plan` cancelled.
    let jobs: Vec<&Value> = out["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|j| j["status"] != "cancelled")
        .collect();
    assert_eq!(jobs.len(), 2);
    assert_eq!(
        (&jobs[1]["key"], &jobs[1]["needs"], &jobs[1]["status"]),
        (&json!("b"), &json!(["a"]), &json!("draft"))
    );
    assert_eq!(
        (
            &jobs[1]["touches"],
            &jobs[1]["attempts"],
            &jobs[1]["fingerprint"]
        ),
        (&json!(["src/b.rs"]), &json!([]), &Value::Null)
    );
    assert_eq!(
        (&out["review"], &out["escalations"], &out["tamper"]),
        (&Value::Null, &json!([]), &json!([]))
    );
    assert_eq!(out["counts"]["draft"], 2);
    assert!(out["events_cursor"].as_i64().unwrap() >= cursor);
    assert_eq!(
        fx.cpd_with(fx.env.root(), &["plan", "show"], &[("CPD_PLAN", "pay")])
            .ok()["plan"]["id"],
        "pay"
    );

    refused(&fx.cpd(&["plan", "show"]), "plan_required", 2);
    refused(
        &fx.cpd(&["plan", "show", "--plan", "nope"]),
        "plan_not_found",
        4,
    );
    refused(
        &fx.cpd(&["plan", "show", "--plan", "Bad"]),
        "invalid_plan_id",
        2,
    );
}

#[test]
fn show_lists_attempts_open_escalations_and_tampered_fingerprints() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    inproc::planned(&fx, &repo, "pay");
    let fingerprint = "f".repeat(64);
    store(&fx.env)
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';
                     UPDATE workflows SET status = 'dispatch', lead_worktree = '/w', started_at = 't' WHERE plan = 'pay';
                     UPDATE jobs SET status = 'queued', fingerprint = '{fingerprint}' WHERE plan = 'pay' AND status = 'draft';
                     UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'a';
                     INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
                       VALUES ('pay', 'a-a1', 'a', 1, 'S2', 'ladder', 'running', 'cpd/pay--a-a1', '/w', 't1');
                     INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
                       VALUES ('pay', 'a-a1-q1', 'a-a1', 1, 'Which?', 'open', 1, 't2');"
                ),
            )
        })
        .unwrap();
    let out = inproc::show(&fx, "pay");
    let a = out["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["key"] == "a")
        .unwrap();
    assert_eq!(
        a["attempts"],
        json!([{"id": "a-a1", "number": 1, "tier": "S2", "origin": "ladder", "status": "running", "outcome": null,
                "cause": null, "worker_session": null, "started_at": "t1", "ended_at": null}])
    );
    assert_eq!(
        out["escalations"],
        json!([{"id": "a-a1-q1", "attempt": "a-a1", "question": "Which?", "asked_at": "t2"}])
    );
    assert_eq!(out["counts"]["running"], 1);
    let tamper = out["tamper"].as_array().unwrap();
    assert_eq!(
        tamper.len(),
        2,
        "both Locked Jobs differ from the stored fingerprint: {tamper:?}"
    );
    assert_eq!(
        (
            &tamper[0]["kind"],
            &tamper[0]["job"],
            &tamper[0]["expected"]
        ),
        (&json!("fingerprint"), &json!("a"), &json!(fingerprint))
    );
}

// ---------- plan render ----------

#[test]
fn render_writes_the_plan_markdown_and_its_fingerprint() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    inproc::planned(&fx, &repo, "pay");
    add_job(&fx.env, "pay", "gone", &[]);
    store(&fx.env).write("fixture", |tx| exec(tx, "UPDATE jobs SET status = 'cancelled', cancel_reason = 'x', cancel_requested_at = 't' WHERE key = 'gone';")).unwrap();

    let out = fx.cpd(&["plan", "render", "--plan", "pay"]).ok();
    let path = home(&fx.env).plan_md(&pid("pay"));
    assert_eq!(out["path"], path.display().to_string());
    assert_eq!(out["draft_jobs"], 2);
    let (pr_key, pr_repo, pr_base): (String, String, String) = store(&fx.env)
        .read(|c| {
            Ok(c.query_row(
                "SELECT key, repo, base FROM workflows WHERE plan = 'pay'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    let spec = |key: &str, needs: &str| {
        format!(
            "{{\"acceptance\":[\"cargo test\"],\"context\":\"Context of {key}.\",\"goal\":\"Goal of {key}.\",\"key\":\"{key}\",\"needs\":[{needs}],\"out_of_scope\":[],\"pr\":\"{pr_key}\",\"tier\":\"S2\",\"title\":\"Job {key}\",\"touches\":[\"src/{key}.rs\"]}}"
        )
    };
    // The PR is `config`: its shape is in the fingerprint, which a PR from before `job-kinds` (`free`) would leave out.
    let prs = format!(
        "[{{\"base\":\"{pr_base}\",\"key\":\"{pr_key}\",\"needs\":[],\"repo\":{},\"shape\":\"config\"}}]",
        serde_json::to_string(&pr_repo).unwrap()
    );
    assert_eq!(
        out["fingerprint"],
        sha256(&format!(
            "{{\"jobs\":[{},{}],\"prs\":{prs}}}",
            spec("a", ""),
            spec("b", "\"a\"")
        ))
    );
    let text = fs::read_to_string(&path).unwrap();
    assert!(
        text.starts_with("# Plan: Plan pay\n\nContext: `ctx` · Phase: `plan`\n"),
        "{text}"
    );
    assert!(
        text.contains("| `b` | Job b | work | S2 | draft | `a` | `src/b.rs` |"),
        "{text}"
    );
    assert!(
        text.contains("```mermaid\ngraph TD\n  j1[\"a\"]\n  j2[\"b\"]\n  j1 --> j2\n```"),
        "{text}"
    );
    assert!(text.contains("\n## Workflow pay\n"), "{text}");
    assert!(text.contains("\n### `a`: Job a\n"), "{text}");
    assert!(text.contains("\n#### Goal\n\nGoal of b.\n"), "{text}");
    assert_eq!(out["workflows"], 1);
    // a cancelled Job is shown with its status, and drawn in no graph (hs-d518)
    assert!(
        text.contains("| `gone` |") && !text.contains("[\"gone\"]"),
        "{text}"
    );

    // Idempotent: the same Jobs render the same file.
    let again = fx.cpd(&["plan", "render", "--plan", "pay"]).ok();
    assert_eq!(again, out);
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    refused(&fx.cpd(&["plan", "render"]), "plan_required", 2);
    refused(
        &fx.cpd(&["plan", "render", "--plan", "nope"]),
        "plan_not_found",
        4,
    );
}

#[test]
fn render_shows_the_planned_opener_and_no_placeholder() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    inproc::new_plan_only(&fx, "pay");
    // A template PR: `pr add` lays out its opener stub beside the shape's stubs; its other Job is merged away.
    fx.cpd(&[
        "workflow",
        "add",
        "pay",
        "--repo",
        repo.to_str().unwrap(),
        "--title",
        "Change the PR",
        "--shape",
        "config",
        "--plan",
        "pay",
    ])
    .ok();
    fx.cpd(&[
        "job",
        "cancel",
        "pay-config",
        "--reason",
        "not needed",
        "--plan",
        "pay",
    ])
    .ok();
    // An older PR, one whose Plan holds no open Job: it predates the template's opener stub, so its row is written
    // directly (the store is append-only: a Job row is never deleted).
    let old = fx.keyed_repo("old");
    store(&fx.env)
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO workflows (plan, key, repo, name, base, title, shape, status, lead_branch, created_txn, created_at)
                     VALUES ('pay', 'old', '{}', 'old', 'main', 'Old change', 'docs', 'waiting', 'cpd/pay/old', cpd_txn(), 't');",
                    old.display()
                ),
            )
        })
        .unwrap();
    fx.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();

    let out = fx.cpd(&["plan", "render", "--plan", "pay"]).ok();
    let text = fs::read_to_string(out["path"].as_str().unwrap()).unwrap();

    // The template PR holds its planned opener: the render shows the Job's own row and section, with no placeholder.
    let pay = section(&text, "pay");
    assert!(
        pay.lines().any(|l| l.starts_with("| `open-pay` |")
            && l.contains("Open the pull request: Change the PR")
            && l.contains("| open |")
            && l.contains("| draft |")),
        "{pay}"
    );
    assert!(
        pay.contains("\n### `open-pay`: Open the pull request: Change the PR\n"),
        "{pay}"
    );
    assert!(pay.contains("Status: `draft` · Kind: `open`"), "{pay}");
    assert!(
        !pay.contains("Added by cpd when the PR starts"),
        "no placeholder for a PR whose Plan holds an open Job:\n{pay}"
    );
    assert!(
        !pay.contains("every other Job of the Workflow"),
        "no placeholder `Needs` cell:\n{pay}"
    );

    // An older PR with no open Job still shows the placeholder, as before the template laid one out.
    let old = section(&text, "old");
    assert!(
        old.contains("| `open-old` | Open the pull request: Old change | open |")
            && old.contains("every other Job of the Workflow"),
        "{old}"
    );
    assert!(
        old.contains("\n### `open-old`: Open the pull request: Old change\n"),
        "{old}"
    );
    assert!(old.contains("Status: — · Kind: `open`"), "{old}");
    assert!(
        old.contains("Added by cpd when the Workflow starts"),
        "{old}"
    );
}

// ---------- plan review, plan decide ----------

#[test]
fn review_requests_a_pending_review_once_per_fingerprint() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    inproc::new_plan(&fx, "pay", &repo);
    let error = refused(
        &fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"]),
        "illegal_phase",
        3,
    );
    assert_eq!(error["details"]["phase"], "grounding");
    fx.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    refused(
        &fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"]),
        "no_draft_jobs",
        3,
    );
    add_job(&fx.env, "pay", "a", &[]);

    fx.outline_for_review("pay");
    let out = fx
        .cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
        .ok();
    let fingerprint = fx.cpd(&["plan", "render", "--plan", "pay"]).ok()["fingerprint"].clone();
    let summary_path = home(&fx.env)
        .plan_md(&pid("pay"))
        .with_file_name("summary-r1.md");
    assert_eq!(
        (
            &out["review"]["id"],
            &out["review"]["status"],
            &out["review"]["fingerprint"]
        ),
        (&json!("r1"), &json!("pending"), &fingerprint)
    );
    assert_eq!(out["summary"], summary_path.display().to_string());
    assert!(out.get("path").is_none(), "{out}");
    assert_eq!(
        out["next"],
        "The User reviews the plan summary in your chat and decides there. Stop and wait for the decision message."
    );
    // The review writes its summary copy, headed by the review's title; `plan.md` keeps its own heading (hs-d291).
    let (copy, plan_md) = (
        fs::read_to_string(&summary_path).unwrap(),
        fs::read_to_string(home(&fx.env).plan_md(&pid("pay"))).unwrap(),
    );
    let plan_title = inproc::show(&fx, "pay")["plan"]["title"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        copy.starts_with(&format!(
            "# Review title\nPlan pay: {plan_title} · review r1\n"
        )),
        "{copy}"
    );
    assert!(
        plan_md.starts_with(&format!("# Plan: {plan_title}\n")),
        "{plan_md}"
    );
    let event = &inproc::events(&fx, "plan-review")[0];
    assert_eq!(
        (&event["plan"], &event["data"]),
        (
            &json!("pay"),
            &json!({"review": "r1", "fingerprint": fingerprint, "status": "pending"})
        )
    );

    // Same Jobs: the pending review again.
    let again = fx
        .cpd_with(
            fx.env.root(),
            &["plan", "review", "--title", "Review title"],
            &[("CPD_PLAN", "pay")],
        )
        .ok();
    assert_eq!(
        (&again["already"], &again["review"]),
        (&json!(true), &out["review"])
    );
    assert_eq!(inproc::events(&fx, "plan-review").len(), 1);

    // Changed Jobs: a new review; the copy of r1 is what the User saw.
    let r1 = fs::read_to_string(&summary_path).unwrap();
    edit_goal(&fx.env, "pay", "a", "A new goal.");
    fx.outline_for_review("pay");
    let r2 = fx
        .cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
        .ok();
    assert_eq!(r2["review"]["id"], "r2");
    assert_ne!(r2["review"]["fingerprint"], fingerprint);
    assert_eq!(fs::read_to_string(&summary_path).unwrap(), r1);
    assert!(
        fs::read_to_string(home(&fx.env).plan_md(&pid("pay")))
            .unwrap()
            .contains("A new goal.")
    );
    assert_eq!(inproc::show(&fx, "pay")["review"]["id"], "r2");
    refused(
        &fx.cpd(&["plan", "review", "--title", "Review title"]),
        "plan_required",
        2,
    );
}

#[test]
fn review_requires_a_title_and_stores_it() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    inproc::new_plan(&fx, "pay", &repo);
    fx.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    add_job(&fx.env, "pay", "a", &[]);
    refused(&fx.cpd(&["plan", "review", "--plan", "pay"]), "usage", 2);
    for bad in ["", "two\nlines", &"x".repeat(73)] {
        refused(
            &fx.cpd(&["plan", "review", "--plan", "pay", "--title", bad]),
            "usage",
            2,
        );
    }
    assert!(
        inproc::events(&fx, "plan-review").is_empty(),
        "a refused request changes nothing"
    );
    fx.outline_for_review("pay");
    fx.cpd(&[
        "plan",
        "review",
        "--plan",
        "pay",
        "--title",
        &"x".repeat(72),
    ])
    .ok();
    let db = rusqlite::Connection::open(fx.env.cpd_home().join("cpd.db")).unwrap();
    let title: String = db
        .query_row(
            "SELECT title FROM plan_reviews WHERE plan = 'pay' AND id = 'r1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(title, "x".repeat(72));
}

#[test]
fn decide_records_the_decision_once() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    inproc::planned(&fx, &repo, "pay");
    fx.outline_for_review("pay");
    fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
        .ok();
    let feedback = fx.env.root().join("feedback.md");
    fs::write(&feedback, "Split job b.").unwrap();

    refused(
        &fx.cpd(&["plan", "decide", "r1", "--plan", "pay"]),
        "usage",
        2,
    );
    refused(
        &fx.cpd(&[
            "plan",
            "decide",
            "r1",
            "--approve",
            "--reject",
            "--plan",
            "pay",
        ]),
        "usage",
        2,
    );
    refused(
        &fx.cpd(&["plan", "decide", "x1", "--approve", "--plan", "pay"]),
        "invalid_review_id",
        2,
    );
    refused(
        &fx.cpd(&["plan", "decide", "r9", "--approve", "--plan", "pay"]),
        "review_not_found",
        4,
    );
    refused(
        &fx.cpd(&[
            "plan",
            "decide",
            "r1",
            "--approve",
            "--plan",
            "pay",
            "--feedback-file",
            "/no/file",
        ]),
        "file_unreadable",
        2,
    );

    let out = fx
        .cpd(&[
            "plan",
            "decide",
            "r1",
            "--reject",
            "--feedback-file",
            feedback.to_str().unwrap(),
            "--plan",
            "pay",
        ])
        .ok();
    assert_eq!(
        (
            &out["review"]["status"],
            &out["review"]["feedback"],
            &out["current"],
            &out["next"]
        ),
        (
            &json!("rejected"),
            &json!("Split job b."),
            &json!(true),
            &Value::Null
        )
    );
    assert!(out["review"]["decided_at"].is_string());
    let decided = inproc::events(&fx, "plan-review").last().unwrap().clone();
    assert_eq!(decided["data"]["status"], "rejected");
    assert_eq!(decided["data"]["feedback"], "Split job b.");
    let again = fx
        .cpd(&["plan", "decide", "r1", "--reject", "--plan", "pay"])
        .ok();
    assert_eq!(again["already"], true);
    let error = refused(
        &fx.cpd(&["plan", "decide", "r1", "--approve", "--plan", "pay"]),
        "review_decided",
        3,
    );
    assert_eq!(error["details"]["status"], "rejected");
    assert_eq!(inproc::events(&fx, "plan-review").len(), 2);

    // Approved, then the Jobs change: no longer current, so no plan lock.
    fx.outline_for_review("pay");
    let r2 = fx
        .cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
        .ok();
    assert_eq!(r2["review"]["id"], "r2");
    let out = fx
        .cpd(&["plan", "decide", "r2", "--approve", "--plan", "pay"])
        .ok();
    assert_eq!(
        (
            &out["review"]["status"],
            &out["review"]["feedback"],
            &out["current"],
            &out["next"]
        ),
        (
            &json!("approved"),
            &Value::Null,
            &json!(true),
            &json!("cpd plan lock")
        )
    );
    edit_goal(&fx.env, "pay", "a", "Changed.");
    let again = fx
        .cpd(&["plan", "decide", "r2", "--approve", "--plan", "pay"])
        .ok();
    assert_eq!(
        (&again["already"], &again["current"], &again["next"]),
        (&json!(true), &json!(false), &Value::Null)
    );
}

// ---------- a review of the drafts in a phase after plan (hs-d173, hs-d174, hs-d179) ----------

/// The Locked-Job fingerprint of a Job written by `add_job` (contract §2.3).
fn locked_fingerprint_of(key: &str, needs: &[&str]) -> String {
    let needs: Vec<String> = needs.iter().map(|n| format!("\"{n}\"")).collect();
    sha256(&format!(
        "{{\"acceptance\":[\"cargo test\"],\"context\":\"Context of {key}.\",\"goal\":\"Goal of {key}.\",\"needs\":[{}],\"out_of_scope\":[],\"tier\":\"S2\",\"touches\":[\"src/{key}.rs\"]}}",
        needs.join(",")
    ))
}

fn job_status(env: &TestEnv, key: &str) -> String {
    env.query(&format!(
        "SELECT status FROM jobs WHERE plan = 'pay' AND key = '{key}'"
    ))
}

#[test]
fn a_review_after_the_lock_covers_the_drafts_and_its_approval_queues_exactly_them() {
    let fx = Fx::with_ports();
    let repo = inproc::locking(&fx);
    inproc::approved(&fx, &repo);
    fx.cpd(&["plan", "lock", "--plan", "pay"]).ok();
    // Locked, every Job queued: there is nothing to review.
    refused(
        &fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"]),
        "no_draft_jobs",
        3,
    );

    add_draft(&fx.env, "pay", "c", &[], true);
    add_draft(&fx.env, "pay", "d", &["c"], true);
    fx.outline_for_review("pay");
    let out = fx
        .cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
        .ok();
    let spec = |key: &str, needs: &str| {
        format!(
            "{{\"acceptance\":[\"cargo test\"],\"context\":\"Context of {key}.\",\"goal\":\"Goal of {key}.\",\"key\":\"{key}\",\"needs\":[{needs}],\"out_of_scope\":[],\"pr\":\"pay\",\"tier\":\"S2\",\"title\":\"Job {key}\",\"touches\":[\"src/{key}.rs\"]}}"
        )
    };
    // The fingerprint covers exactly the drafts, and no PR, and the outline (contract §2.3) that `outline_for_review` set.
    let outline = "{\"assumptions\":[],\"decided\":[],\"decisions\":[],\"done_when\":{\"pay\":[\"Done\"]},\"goal\":\"Goal\",\"out_of_scope\":[],\"risks\":[]}";
    let fingerprint = sha256(&format!(
        "{{\"jobs\":[{},{}],\"outline\":{outline},\"prs\":[]}}",
        spec("c", ""),
        spec("d", "\"c\"")
    ));
    assert_eq!(
        (
            &out["review"]["id"],
            &out["review"]["status"],
            &out["review"]["fingerprint"]
        ),
        (&json!("r2"), &json!("pending"), &json!(fingerprint))
    );
    assert_eq!(
        inproc::events(&fx, "plan-review").last().unwrap()["data"],
        json!({"review": "r2", "fingerprint": fingerprint, "status": "pending"})
    );

    // The render marks the drafts under review, and the Locked Jobs as context.
    assert!(out.get("path").is_none(), "{out}");
    let text = fs::read_to_string(home(&fx.env).plan_md(&pid("pay"))).unwrap();
    assert!(
        text.contains("| Key | Title | Kind | Tier | Status | Needs | Touches | Review |"),
        "{text}"
    );
    assert!(
        text.contains("| `c` | Job c | work | S2 | draft | none | `src/c.rs` | **under review** |"),
        "{text}"
    );
    assert!(
        text.contains("| `a` | Job a | work | S2 | queued | none | `src/a.rs` | Locked |"),
        "{text}"
    );
    assert!(
        text.contains("\nStatus: `draft` · **under review** · Tier: `S2` · Needs: `c`\n"),
        "{text}"
    );
    // `b` is queued but cannot start until `a` merges: shown as waiting (hs-d521)
    assert!(
        text.contains(
            "\nStatus: `waiting` · waiting on [`a`](#a-job-a) · Locked · Tier: `S2` · Needs: `a`\n"
        ),
        "{text}"
    );
    assert_eq!(
        fx.cpd(&["plan", "render", "--plan", "pay"]).ok()["draft_jobs"],
        2
    );

    // A PR appended after the lock is not reviewed: the review stays current.
    let web = fx.keyed_repo("web");
    // Its stubs are legal after the lock on their own, incomplete (hs-d822); a complete draft that waits would be parked.
    fx.cpd(&[
        "workflow",
        "add",
        "web",
        "--repo",
        web.to_str().unwrap(),
        "--title",
        "Change the Workflow",
        "--base",
        "main",
        "--shape",
        "config",
        "--needs",
        "pay",
        "--plan",
        "pay",
    ])
    .ok();
    // Its stub drafts are drafts of the review's kind: cancelled, they are none, and the review stays current.
    assert_eq!(
        inproc::show(&fx, "pay")["review"]["current"],
        false,
        "a stub draft is a draft the review did not cover"
    );
    fx.cpd(&[
        "job",
        "cancel",
        "web-config",
        "--reason",
        "a stub of the template",
        "--plan",
        "pay",
    ])
    .ok();
    fx.cpd(&[
        "job",
        "cancel",
        "open-web",
        "--reason",
        "a stub of the template",
        "--plan",
        "pay",
    ])
    .ok();
    assert_eq!(inproc::show(&fx, "pay")["review"]["current"], true);

    // Rejected: the drafts stay drafts, editable.
    let out = fx
        .cpd(&["plan", "decide", "r2", "--reject", "--plan", "pay"])
        .ok();
    assert!(out.get("jobs").is_none(), "{out}");
    assert!(
        inproc::events(&fx, "plan-review").last().unwrap()["data"]
            .get("jobs")
            .is_none()
    );
    assert_eq!(
        (job_status(&fx.env, "c"), job_status(&fx.env, "d")),
        ("draft".into(), "draft".into())
    );
    edit_goal(&fx.env, "pay", "d", "Goal of d.");

    // Approved and current: in one transaction exactly the drafts are queued with their fingerprints.
    let phase_events = inproc::events(&fx, "plan-phase").len();
    fx.outline_for_review("pay");
    fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
        .ok();
    let out = fx
        .cpd(&["plan", "decide", "r3", "--approve", "--plan", "pay"])
        .ok();
    assert_eq!(
        (
            &out["review"]["status"],
            &out["current"],
            &out["next"],
            &out["jobs"]
        ),
        (
            &json!("approved"),
            &json!(true),
            &Value::Null,
            &json!(["c", "d"])
        )
    );
    let decided = inproc::events(&fx, "plan-review").last().unwrap().clone();
    assert_eq!(
        decided["data"],
        json!({"review": "r3", "fingerprint": fingerprint, "status": "approved", "feedback": null, "jobs": ["c", "d"]})
    );
    assert!(
        decided["text"]
            .as_str()
            .unwrap()
            .contains("no lock is needed"),
        "{decided}"
    );
    let shown = inproc::show(&fx, "pay");
    assert_eq!(shown["plan"]["phase"], "dispatch");
    assert_eq!(
        inproc::events(&fx, "plan-phase").len(),
        phase_events,
        "the phase does not change"
    );
    for (key, needs) in [("c", &[] as &[&str]), ("d", &["c"])] {
        let job = shown["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["key"] == key)
            .unwrap();
        assert_eq!(
            (&job["status"], &job["appended"]),
            (&json!("queued"), &json!(true)),
            "{key}"
        );
        assert_eq!(
            job["fingerprint"],
            locked_fingerprint_of(key, needs),
            "{key}"
        );
    }
    // The tamper check holds for them: the fingerprints match what the engine recomputes.
    assert_eq!(shown["tamper"], json!([]));
    // Idempotent: a rerun reports the same Jobs and writes nothing.
    let again = fx
        .cpd(&["plan", "decide", "r3", "--approve", "--plan", "pay"])
        .ok();
    assert_eq!(
        (&again["already"], &again["jobs"], &again["next"]),
        (&json!(true), &json!(["c", "d"]), &Value::Null)
    );
    assert_eq!(
        inproc::events(&fx, "plan-review").len(),
        6,
        "r1 and r2 requested and decided, r3 requested and approved once"
    );

    // A stale approval queues nothing and asks for a new review.
    add_draft(&fx.env, "pay", "e", &[], true);
    fx.outline_for_review("pay");
    fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
        .ok();
    edit_goal(&fx.env, "pay", "e", "Changed.");
    let out = fx
        .cpd(&["plan", "decide", "r4", "--approve", "--plan", "pay"])
        .ok();
    assert_eq!(
        (&out["current"], &out["next"]),
        (&json!(false), &json!("cpd plan review"))
    );
    assert!(out.get("jobs").is_none(), "{out}");
    assert!(
        inproc::events(&fx, "plan-review").last().unwrap()["data"]
            .get("jobs")
            .is_none()
    );
    assert_eq!(job_status(&fx.env, "e"), "draft");
}

/// A `draft` `open` Job of PR `pr` as engine E4 inserts it while a draft of the PR remains (hs-d573 B1): `appended`, no fingerprint.
/// The template lays one out (`open-pay`), cancelled by `new_plan`: this adds the draft E4 would insert under the next free key.
fn add_draft_open(env: &TestEnv, plan: &str, key: &str) {
    store(env)
        .write("fixture", |tx| {
            exec(
                tx,
                &format!(
                    "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, kind, created_txn, created_at)
                     VALUES ('{plan}', (SELECT key FROM workflows WHERE plan = '{plan}'), '{key}', 'Open the pull request', 'S2', 'Open it.', 'Because.', '[\"{key}.txt\"]',
                             '[\"it opens\"]', '[]', 'draft', 1, 'open', {id}, 't');",
                    id = tx.id()
                ),
            )
        })
        .unwrap();
}

/// B1 (hs-d573 O3, O4): E4's draft `open` Job is no Job under review: the fingerprint ignores it, so a pending review
/// stays current, and an approval queues it in the same transaction with every other draft it covers (hs-n677).
#[test]
fn a_draft_open_job_does_not_void_a_pending_review_and_an_approval_queues_it_with_the_drafts() {
    let fx = Fx::with_ports();
    let repo = inproc::locking(&fx);
    inproc::approved(&fx, &repo);
    fx.cpd(&["plan", "lock", "--plan", "pay"]).ok();
    // A complete draft that waits on the queued Job `a`: the review covers it and an approval queues it too (hs-n677).
    add_draft(&fx.env, "pay", "c", &["a"], true);
    fx.outline_for_review("pay");
    let fingerprint = fx.cpd(&["plan", "render", "--plan", "pay"]).ok()["fingerprint"].clone();
    let out = fx
        .cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
        .ok();
    assert_eq!(out["review"]["fingerprint"], fingerprint);
    let review = out["review"]["id"].as_str().unwrap().to_owned();

    add_draft_open(&fx.env, "pay", "open-pay-2");
    assert_eq!(
        fx.cpd(&["plan", "render", "--plan", "pay"]).ok()["fingerprint"],
        fingerprint,
        "the open Job is not in the fingerprint"
    );
    assert_eq!(inproc::show(&fx, "pay")["review"]["current"], true);

    let out = fx
        .cpd(&["plan", "decide", &review, "--approve", "--plan", "pay"])
        .ok();
    assert_eq!(
        (&out["review"]["status"], &out["current"], &out["jobs"]),
        (
            &json!("approved"),
            &json!(true),
            &json!(["c", "open-pay-2"])
        )
    );
    let decided = inproc::events(&fx, "plan-review").last().unwrap().clone();
    assert_eq!(decided["data"]["jobs"], json!(["c", "open-pay-2"]));
    for key in ["c", "open-pay-2"] {
        assert_eq!(job_status(&fx.env, key), "queued", "{key}");
        assert!(
            fx.env.query::<bool>(&format!(
                "SELECT fingerprint IS NOT NULL FROM jobs WHERE plan = 'pay' AND key = '{key}'"
            )),
            "{key} has its fingerprint"
        );
    }
}

/// B1 (hs-d573 O4): `draft_jobs` and `no_draft_jobs` do not count a draft `open` Job, and the review document shows it as context.
#[test]
fn a_draft_open_job_is_no_job_under_review() {
    let fx = Fx::with_ports();
    let repo = inproc::locking(&fx);
    inproc::approved(&fx, &repo);
    fx.cpd(&["plan", "lock", "--plan", "pay"]).ok();

    // The only draft is the open Job: nothing to review.
    add_draft_open(&fx.env, "pay", "open-pay-2");
    refused(
        &fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"]),
        "no_draft_jobs",
        3,
    );
    assert_eq!(
        fx.cpd(&["plan", "render", "--plan", "pay"]).ok()["draft_jobs"],
        0
    );

    add_draft(&fx.env, "pay", "c", &["a"], true);
    fx.outline_for_review("pay");
    assert_eq!(
        fx.cpd(&["plan", "render", "--plan", "pay"]).ok()["draft_jobs"],
        1
    );
    let out = fx
        .cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
        .ok();
    assert!(out.get("path").is_none(), "{out}");
    let text = fs::read_to_string(home(&fx.env).plan_md(&pid("pay"))).unwrap();
    let row = text
        .lines()
        .find(|l| l.starts_with("| `open-pay-2` |"))
        .unwrap_or_else(|| panic!("no row for the open Job: {text}"));
    assert!(!row.contains("**under review**"), "{row}");
    assert!(
        text.lines()
            .any(|l| l.starts_with("| `c` |") && l.contains("**under review**")),
        "{text}"
    );
}

/// hs-n677: a stub no longer escapes review completeness by waiting: `plan review` refuses `draft_incomplete` for it
/// exactly as for any stub, and once completed and no longer waiting the review covers it and an approval queues it.
#[test]
fn a_waiting_stub_refuses_a_review_request_like_any_stub() {
    let fx = Fx::with_ports();
    let repo = inproc::locking(&fx);
    inproc::approved(&fx, &repo);
    fx.cpd(&["plan", "lock", "--plan", "pay"]).ok();

    // A stub with no Tier and no Spec, waiting on the queued Job `a`: legal on its own, but not under review.
    let out = inproc::stub(&fx, "waiting-stub", &["--needs", "a"]);
    assert_eq!(
        (&out["job"]["status"], &out["job"]["tier"]),
        (&json!("draft"), &Value::Null)
    );

    // The waiting stub refuses the review `draft_incomplete`, exactly as a stub that waits for nothing would.
    fx.outline_for_review("pay");
    let e = refused(
        &fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"]),
        "draft_incomplete",
        3,
    );
    assert_eq!(
        e["details"]["jobs"],
        json!([{"key": "waiting-stub", "missing": ["tier", "spec"]}]),
        "{e}"
    );

    // Drop the need that makes it wait and complete it: the review covers it and an approval queues it.
    seed_tiers(&fx.env);
    let spec = spec_file(&fx.env, "waiting");
    fx.cpd(&[
        "job",
        "edit",
        "waiting-stub",
        "--no-needs",
        "--tier",
        "S2",
        "--spec",
        &spec,
        "--plan",
        "pay",
    ])
    .ok();
    let out = fx
        .cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
        .ok();
    let review = out["review"]["id"].as_str().unwrap().to_owned();
    assert_eq!(out["review"]["status"], "pending");
    let out = fx
        .cpd(&["plan", "decide", &review, "--approve", "--plan", "pay"])
        .ok();
    assert_eq!(out["jobs"], json!(["waiting-stub"]));
    assert_eq!(job_status(&fx.env, "waiting-stub"), "queued");
}

/// hs-n677: after the lock an approval queues every draft the review covers, whether it waits or not.
#[test]
fn an_approval_queues_every_covered_draft() {
    let fx = Fx::with_ports();
    let repo = inproc::locking(&fx);
    inproc::approved(&fx, &repo);
    fx.cpd(&["plan", "lock", "--plan", "pay"]).ok();

    // A complete draft that waits on the queued Job `a`, and a complete draft that waits for nothing.
    add_draft(&fx.env, "pay", "waiting-full", &["a"], true);
    add_draft(&fx.env, "pay", "fresh", &[], true);

    fx.outline_for_review("pay");
    let out = fx
        .cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
        .ok();
    let review = out["review"]["id"].as_str().unwrap().to_owned();

    let out = fx
        .cpd(&["plan", "decide", &review, "--approve", "--plan", "pay"])
        .ok();
    assert_eq!(out["jobs"], json!(["fresh", "waiting-full"]));
    assert_eq!(job_status(&fx.env, "fresh"), "queued");
    assert_eq!(job_status(&fx.env, "waiting-full"), "queued");
    let decided = inproc::events(&fx, "plan-review").last().unwrap().clone();
    assert_eq!(decided["data"]["jobs"], json!(["fresh", "waiting-full"]));
}

// ---------- plan lock ----------

#[test]
fn lock_creates_the_lead_worktree_queues_the_drafts_and_starts_the_engine() {
    let fx = Fx::with_ports();
    let repo = inproc::locking(&fx);
    inproc::approved(&fx, &repo);
    let head = fx.git(&repo, &["rev-parse", "HEAD"]);

    let out = fx.cpd(&["plan", "lock", "--plan", "pay"]).ok();
    let worktree = repo
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap()
        .join("pay.pay.pay");
    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "phase": "dispatch", "jobs": ["a", "b"],
               "workflows": [{"key": "pay", "status": "dispatch", "needs": [], "lead_branch": "pay/pay", "lead_worktree": worktree.display().to_string(),
                        "base_sha": head, "reused": false, "pushed": false, "base_workflow": null}],
               "watch": {"started": true, "pane": "%9"}})
    );
    assert_eq!(
        fx.git(&worktree, &["symbolic-ref", "--short", "HEAD"]),
        "pay/pay"
    );
    assert_eq!(
        inproc::watch_calls(&fx),
        vec![vec!["watch".to_owned(), "--ensure".to_owned()]]
    );

    let shown = inproc::show(&fx, "pay");
    assert_eq!(
        (
            &shown["plan"]["phase"],
            &shown["workflows"][0]["lead_worktree"]
        ),
        (&json!("dispatch"), &json!(worktree.display().to_string()))
    );
    assert!(shown["plan"]["locked_at"].is_string());
    for job in shown["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|j| ["a", "b"].contains(&j["key"].as_str().unwrap()))
    {
        assert_eq!(job["status"], "queued");
        let needs: Vec<String> = job["needs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| format!("\"{}\"", n.as_str().unwrap()))
            .collect();
        let key = job["key"].as_str().unwrap();
        let canonical = format!(
            "{{\"acceptance\":[\"cargo test\"],\"context\":\"Context of {key}.\",\"goal\":\"Goal of {key}.\",\"needs\":[{}],\"out_of_scope\":[],\"tier\":\"S2\",\"touches\":[\"src/{key}.rs\"]}}",
            needs.join(",")
        );
        assert_eq!(
            job["fingerprint"],
            sha256(&canonical),
            "the Locked-Job fingerprint of {key}"
        );
    }
    assert_eq!(shown["tamper"], json!([]));
    let phase = inproc::events(&fx, "plan-phase").last().unwrap().clone();
    assert_eq!(
        phase["data"],
        json!({"from": "plan", "to": "dispatch", "jobs": ["a", "b"], "workflows": ["pay"]})
    );
    let started = inproc::events(&fx, "workflow-started");
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!(
        (&started[0]["data"]["workflow"], &started[0]["data"]["by"]),
        (&json!("pay"), &json!("lock"))
    );

    // Already: still makes sure the engine runs.
    let again = fx.cpd(&["plan", "lock", "--plan", "pay"]).ok();
    assert_eq!(
        (
            &again["already"],
            &again["phase"],
            &again["watch"]["started"]
        ),
        (&json!(true), &json!("dispatch"), &json!(true))
    );
    assert_eq!(inproc::watch_calls(&fx).len(), 2);
    assert_eq!(inproc::events(&fx, "plan-phase").len(), 2);
}

// ---------- plan lock and the remote Lead Branch (R35) ----------

/// A bare `origin` for `repo`, holding `main`; returns its path.
fn with_origin(fx: &Fx, repo: &Path) -> PathBuf {
    let origin = fx.env.root().join("origins").join("pay.git");
    fs::create_dir_all(&origin).unwrap();
    fx.git(&origin, &["init", "-q", "--bare", "-b", "main"]);
    fx.local_origin(repo, &origin);
    fx.git(repo, &["push", "-q", "origin", "main"]);
    origin
}

#[test]
fn lock_creates_and_pushes_the_lead_branch_when_the_remote_has_none() {
    let fx = Fx::with_ports();
    let repo = inproc::locking(&fx);
    let origin = with_origin(&fx, &repo);
    inproc::approved(&fx, &repo);
    let head = fx.git(&repo, &["rev-parse", "HEAD"]);

    let out = fx.cpd(&["plan", "lock", "--plan", "pay"]).ok();

    assert_eq!(
        (
            &out["workflows"][0]["base_sha"],
            &out["workflows"][0]["pushed"],
            &out["workflows"][0]["reused"]
        ),
        (&json!(head), &json!(true), &json!(false))
    );
    assert_eq!(
        fx.git(&repo, &["ls-remote", "--heads", "origin", "pay/pay"]),
        format!("{head}\trefs/heads/pay/pay")
    );
    // No upstream: nothing in the shared .git/config (hs-d243 G1).
    let config = fx.git_output(&repo, &["config", "--get-regexp", r"^branch\."]);
    assert_eq!(String::from_utf8(config.stdout).unwrap(), "");
    assert_eq!(fx.git(&origin, &["rev-parse", "pay/pay"]), head);
    assert!(inproc::events(&fx, "error").is_empty());
}

#[test]
fn lock_reuses_a_remote_lead_branch_that_contains_the_base() {
    let fx = Fx::with_ports();
    let repo = inproc::locking(&fx);
    with_origin(&fx, &repo);
    inproc::approved(&fx, &repo);
    // A commit only the remote has, on top of the base.
    fx.git(
        &repo,
        &["commit", "-q", "--allow-empty", "-m", "only on the remote"],
    );
    let remote_tip = fx.git(&repo, &["rev-parse", "HEAD"]);
    fx.git(&repo, &["push", "-q", "origin", "HEAD:refs/heads/pay/pay"]);
    fx.git(&repo, &["reset", "-q", "--hard", "HEAD~1"]);
    assert_eq!(fx.git(&repo, &["branch", "--list", "pay/pay"]), "");

    let out = fx.cpd(&["plan", "lock", "--plan", "pay"]).ok();

    let worktree = repo
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap()
        .join("pay.pay.pay");
    assert_eq!(
        (
            &out["workflows"][0]["base_sha"],
            &out["workflows"][0]["reused"],
            &out["workflows"][0]["pushed"]
        ),
        (&json!(remote_tip), &json!(true), &json!(false))
    );
    assert_eq!(fx.git(&worktree, &["rev-parse", "HEAD"]), remote_tip);
    assert_eq!(
        fx.git(&repo, &["rev-parse", "refs/heads/pay/pay"]),
        remote_tip
    );
    assert_eq!(
        fx.git(&repo, &["ls-remote", "--heads", "origin", "pay/pay"]),
        format!("{remote_tip}\trefs/heads/pay/pay")
    );
    assert!(inproc::events(&fx, "error").is_empty());
}

/// `pay/pay` is on the remote one commit past the old `main`, writing `clash.txt`; then `main` moves on, writing
/// `clash.txt` differently (`conflicting`, so the branch conflicts with it) or `moved.txt` (so it only lacks it).
/// Returns the branch's tip.
fn remote_lead_branch_then_main_moves(fx: &Fx, repo: &Path, conflicting: bool) -> String {
    let write = |path: &str, content: &str, message: &str| {
        std::fs::write(repo.join(path), content).unwrap();
        fx.git(repo, &["add", "-A"]);
        fx.git(repo, &["commit", "-q", "-m", message]);
    };
    write("clash.txt", "the branch's\n", "the branch");
    let tip = fx.git(repo, &["rev-parse", "HEAD"]);
    fx.git(repo, &["push", "-q", "origin", "HEAD:refs/heads/pay/pay"]);
    fx.git(repo, &["reset", "-q", "--hard", "HEAD~1"]);
    let (path, content) = if conflicting {
        ("clash.txt", "main's\n")
    } else {
        ("moved.txt", "main moved\n")
    };
    write(path, content, "main moves on");
    fx.git(repo, &["push", "-q", "origin", "main"]);
    tip
}

#[test]
fn lock_refuses_a_remote_lead_branch_that_conflicts_with_its_base_and_reuses_one_merely_behind_it()
{
    // hs-d490: only a conflict refuses; a base that merely moved ahead of the branch does not.
    let fx = Fx::with_ports();
    let repo = inproc::locking(&fx);
    with_origin(&fx, &repo);
    inproc::approved(&fx, &repo);
    let remote_branch = remote_lead_branch_then_main_moves(&fx, &repo, true);

    let error = refused(
        &fx.cpd(&["plan", "lock", "--plan", "pay"]),
        "branch_exists",
        3,
    );

    assert_eq!(
        (&error["details"]["branch"], &error["details"]["workflow"]),
        (&json!("pay/pay"), &json!("pay"))
    );
    assert_eq!(error["details"]["files"], json!(["clash.txt"]));
    assert_eq!(fx.git(&repo, &["branch", "--list", "pay/pay"]), "");
    assert!(!repo.parent().unwrap().join("pay.pay.pay").exists());
    assert_eq!(
        fx.git(&repo, &["ls-remote", "--heads", "origin", "pay/pay"]),
        format!("{remote_branch}\trefs/heads/pay/pay")
    );
    assert_eq!(inproc::show(&fx, "pay")["plan"]["phase"], "plan");
    assert!(inproc::watch_calls(&fx).is_empty());
}

#[test]
fn lock_reuses_a_remote_lead_branch_that_lacks_a_base_that_merely_moved_ahead() {
    let fx = Fx::with_ports();
    let repo = inproc::locking(&fx);
    with_origin(&fx, &repo);
    inproc::approved(&fx, &repo);
    let remote_branch = remote_lead_branch_then_main_moves(&fx, &repo, false);

    let out = fx.cpd(&["plan", "lock", "--plan", "pay"]).ok();

    assert_eq!(
        (
            &out["workflows"][0]["reused"],
            &out["workflows"][0]["pushed"]
        ),
        (&json!(true), &json!(false))
    );
    assert_eq!(
        fx.git(&repo, &["ls-remote", "--heads", "origin", "pay/pay"]),
        format!("{remote_branch}\trefs/heads/pay/pay")
    );
    assert_eq!(inproc::show(&fx, "pay")["plan"]["phase"], "dispatch");
}

#[test]
fn lock_without_an_origin_remote_is_as_before() {
    let fx = Fx::with_ports();
    let repo = inproc::locking(&fx);
    inproc::approved(&fx, &repo);
    fx.cpd(&["plan", "lock", "--plan", "pay"]).ok();
    assert_eq!(fx.git(&repo, &["remote"]), "");
    assert_eq!(inproc::show(&fx, "pay")["plan"]["phase"], "dispatch");
    assert!(inproc::events(&fx, "error").is_empty());
}

#[test]
fn a_push_failure_at_lock_still_locks_and_appends_one_push_failed_error() {
    let fx = Fx::with_ports();
    let repo = inproc::locking(&fx);
    with_origin(&fx, &repo);
    inproc::approved(&fx, &repo);
    // The remote refuses the push (a `pre-receive` hook of its own is git's behaviour, not ours: scripted).
    let _ = (*fx.fakes.runner).clone().on(
        "git",
        |call| call.args.iter().any(|a| a == "push"),
        Reply::exit(1, "", "refused by hook\n"),
    );

    let out = fx.cpd(&["plan", "lock", "--plan", "pay"]).ok();

    assert_eq!(out["phase"], "dispatch");
    assert_eq!(inproc::show(&fx, "pay")["plan"]["phase"], "dispatch");
    assert_eq!(
        fx.git(&repo, &["ls-remote", "--heads", "origin", "pay/pay"]),
        ""
    );
    let errors = inproc::events(&fx, "error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["data"]["code"], "push_failed");
    assert_eq!(errors[0]["data"]["source"], "plan lock");
    assert!(
        errors[0]["data"]["details"]["stderr"]
            .as_str()
            .unwrap()
            .contains("refused by hook")
    );
}

#[test]
fn lock_stays_locked_when_the_engine_cannot_start() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    inproc::approved(&fx, &repo);
    inproc::watch_once(
        &fx,
        Reply::exit(1, &format!("{}\n", inproc::WATCH_FAILS), ""),
    );
    let error = refused(
        &fx.cpd(&["plan", "lock", "--plan", "pay"]),
        "watch_start_failed",
        1,
    );
    assert_eq!(error["details"]["next"], "cpd watch --ensure");
    assert_eq!(error["details"]["error"]["message"], "No lock.");
    assert_eq!(
        inproc::show(&fx, "pay")["plan"]["phase"],
        "dispatch",
        "the Plan stays locked"
    );

    // An engine that prints nothing usable fails the same way.
    inproc::watch_once(&fx, Reply::ok("nonsense\n"));
    let error = refused(
        &fx.cpd(&["plan", "lock", "--plan", "pay"]),
        "watch_start_failed",
        1,
    );
    assert_eq!(error["details"]["error"]["stdout"], "nonsense");
}

#[test]
fn lock_refusals_change_nothing() {
    let fx = Fx::with_ports();
    let repo = inproc::locking(&fx);
    inproc::new_plan(&fx, "pay", &repo);
    let lock = || fx.cpd(&["plan", "lock", "--plan", "pay"]);
    let error = refused(&lock(), "illegal_transition", 3);
    assert_eq!(error["details"]["from"], "grounding");
    fx.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    refused(&lock(), "no_draft_jobs", 3);

    add_job(&fx.env, "pay", "b", &[]);
    add_job(&fx.env, "pay", "a", &["b"]);
    let error = refused(&lock(), "plan_not_approved", 3);
    assert_eq!(error["details"]["review"], Value::Null);
    fx.outline_for_review("pay");
    fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
        .ok();
    refused(&lock(), "plan_not_approved", 3);
    fx.cpd(&["plan", "decide", "r1", "--reject", "--plan", "pay"])
        .ok();
    let error = refused(&lock(), "plan_not_approved", 3);
    assert_eq!(error["details"]["review"]["status"], "rejected");
    fx.outline_for_review("pay");
    fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
        .ok();
    fx.cpd(&["plan", "decide", "r2", "--approve", "--plan", "pay"])
        .ok();
    edit_goal(&fx.env, "pay", "a", "Changed after approval.");
    refused(&lock(), "plan_not_approved", 3);

    // A cycle (b needs a, a needs b), approved.
    store(&fx.env)
        .write("fixture", |tx| {
            exec(
                tx,
                "INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'b', 'a');",
            )
        })
        .unwrap();
    fx.outline_for_review("pay");
    // A review refuses the cycle and writes nothing; the approved review r3 is a fixture row for the plan as it is.
    let error = refused(
        &fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"]),
        "dependency_cycle",
        3,
    );
    assert_eq!(error["details"]["cycle"].as_array().unwrap().len(), 3);
    let fingerprint = fx.cpd(&["plan", "render", "--plan", "pay"]).ok()["fingerprint"]
        .as_str()
        .unwrap()
        .to_owned();
    fx.env.sql(&format!(
        "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at, decided_at) VALUES ('pay', 'r3', 3, '{fingerprint}', 'approved', 't', 't');"
    ));
    let error = refused(&lock(), "dependency_cycle", 3);
    assert_eq!(error["details"]["cycle"].as_array().unwrap().len(), 3);
    store(&fx.env)
        .write("fixture", |tx| {
            exec(
                tx,
                "DELETE FROM dependencies WHERE plan = 'pay' AND job = 'b';",
            )
        })
        .unwrap();
    fx.outline_for_review("pay");
    fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
        .ok();
    fx.cpd(&["plan", "decide", "r4", "--approve", "--plan", "pay"])
        .ok();

    // Something at the Lead worktree's path, then the branch.
    let path = repo.parent().unwrap().join("pay.pay.pay");
    fs::create_dir_all(&path).unwrap();
    refused(&lock(), "worktree_exists", 3);
    fs::remove_dir(&path).unwrap();
    fx.git(&repo, &["branch", "pay/pay"]);
    refused(&lock(), "branch_exists", 3);
    fx.git(&repo, &["branch", "-D", "pay/pay"]);

    // pre-start fails: the worktree and branch are removed.
    fx.env.repo_policy(
        "acme",
        "pay",
        "[hooks]\npre-start = \"echo setting up; exit 3\"\n",
    );
    // The hook is a shell run (external): its failing answer is scripted.
    fx.fake("sh", Reply::exit(3, "setting up\n", ""));
    let error = refused(&lock(), "setup_failed", 1);
    assert_eq!(
        error["details"]["log"],
        home(&fx.env)
            .root()
            .join("run/pay/logs/lead-pay.pre-start.log")
            .display()
            .to_string()
    );
    assert_eq!(
        (&error["details"]["workflow"], &error["details"]["step"]),
        (&json!("pay"), &json!("pre-start"))
    );
    assert!(!path.exists());
    assert_eq!(fx.git(&repo, &["branch", "--list", "pay/pay"]), "");

    assert_eq!(inproc::show(&fx, "pay")["plan"]["phase"], "plan");
    assert_eq!(inproc::show(&fx, "pay")["counts"]["draft"], 2);
    assert!(inproc::watch_calls(&fx).is_empty());
    refused(&fx.cpd(&["plan", "lock"]), "plan_required", 2);
}

// ---- plan rename ----------------------------------------------------------

#[test]
fn rename_sets_the_title_and_appends_one_event() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    inproc::new_plan(&fx, "pay", &repo);
    let out = fx
        .cpd(&[
            "plan",
            "rename",
            "--plan",
            "pay",
            "--title",
            "Retry payouts",
        ])
        .ok();
    assert_eq!(
        out["plan"],
        json!({"id": "pay", "context": "ctx", "title": "Retry payouts"})
    );
    assert!(out.get("already").is_none());
    assert_eq!(inproc::show(&fx, "pay")["plan"]["title"], "Retry payouts");
    let renamed = inproc::events(&fx, "plan-renamed");
    assert_eq!(renamed.len(), 1);
    assert_eq!(
        (renamed[0]["context"].as_str(), renamed[0]["plan"].as_str()),
        (Some("ctx"), Some("pay"))
    );
    assert_eq!(renamed[0]["data"], json!({"title": "Retry payouts"}));

    // CPD_PLAN selects the Plan; the same title is already and writes nothing.
    let again = fx
        .cpd_with(
            fx.env.root(),
            &["plan", "rename", "--title", "Retry payouts"],
            &[("CPD_PLAN", "pay")],
        )
        .ok();
    assert_eq!(again["already"], true);
    assert_eq!(inproc::events(&fx, "plan-renamed").len(), 1);
}

/// `@cpd_title` of `pane`.
fn pane_title(fx: &Fx, pane: &str) -> String {
    let out = fx.tmux(&["show", "-pqv", "-t", pane, "@cpd_title"]);
    String::from_utf8(out.stdout)
        .unwrap()
        .trim_end_matches('\n')
        .to_owned()
}

#[test]
fn the_leads_pane_carries_the_plans_title_and_plan_rename_keeps_it_current() {
    let fx = Fx::with_ports();
    inproc::setup(&fx);
    let out = fx
        .cpd(&[
            "plan",
            "new",
            "pay",
            "--context",
            "ctx",
            "--title",
            "Retry payouts",
        ])
        .ok();
    let (lead, pane) = (
        out["lead"]["session"].as_str().unwrap().to_owned(),
        out["lead"]["pane"].as_str().unwrap().to_owned(),
    );
    assert_eq!(pane_title(&fx, &pane), "Retry Payouts");

    fx.cpd(&[
        "plan",
        "rename",
        "--plan",
        "pay",
        "--title",
        "Retry the payouts",
    ])
    .ok();
    assert_eq!(pane_title(&fx, &pane), "Retry The Payouts");

    // A side fork shows its own name; renaming the Plan leaves it alone.
    let fork = fx.cpd(&["session", "fork", &lead, "--name", "side"]).ok()["session"].clone();
    let fork_pane = fork["pane"].as_str().unwrap();
    assert_eq!(pane_title(&fx, fork_pane), "Side");
    fx.cpd(&["plan", "rename", "--plan", "pay", "--title", "Third"])
        .ok();
    assert_eq!(
        (pane_title(&fx, &pane), pane_title(&fx, fork_pane)),
        ("Third".to_owned(), "Side".to_owned())
    );

    // Renaming the Plan's Lead Session never replaces the Plan's title.
    fx.cpd(&["session", "rename", "lead name", "--session", &lead])
        .ok();
    assert_eq!(pane_title(&fx, &pane), "Third");

    // A rerun with the same title repairs a header that was cleared.
    fx.tmux(&["set", "-p", "-u", "-t", &pane, "@cpd_title"]);
    assert_eq!(pane_title(&fx, &pane), "");
    let again = fx
        .cpd(&["plan", "rename", "--plan", "pay", "--title", "Third"])
        .ok();
    assert_eq!(again["already"], true);
    assert_eq!(pane_title(&fx, &pane), "Third");

    // No pane, nothing to do.
    fx.tmux(&["kill-pane", "-t", &pane]);
    fx.cpd(&["plan", "rename", "--plan", "pay", "--title", "Fourth"])
        .ok();
}

#[test]
fn a_leads_dynamic_title_survives_plan_rename_and_session_rename() {
    let fx = Fx::with_ports();
    inproc::setup(&fx);
    let out = fx
        .cpd(&[
            "plan",
            "new",
            "pay",
            "--context",
            "ctx",
            "--title",
            "Retry payouts",
        ])
        .ok();
    let (lead, pane) = (
        out["lead"]["session"].as_str().unwrap().to_owned(),
        out["lead"]["pane"].as_str().unwrap().to_owned(),
    );
    // Untitled: the header follows the rename (the test above). Titled: it keeps the title.
    store(&fx.env)
        .write("fixture", |tx| exec(tx, &format!("UPDATE sessions SET title = 'Dynamic subject', titled_at = 't' WHERE id = '{lead}';")))
        .unwrap();
    fx.tmux(&["set", "-p", "-t", &pane, "@cpd_title", "Dynamic subject"]);

    fx.cpd(&[
        "plan",
        "rename",
        "--plan",
        "pay",
        "--title",
        "Retry the payouts",
    ])
    .ok();
    assert_eq!(pane_title(&fx, &pane), "Dynamic Subject");
    assert_eq!(
        fx.env
            .query::<String>("SELECT title FROM plans WHERE id = 'pay'"),
        "Retry the payouts"
    );
    assert_eq!(inproc::events(&fx, "plan-renamed").len(), 1);

    fx.cpd(&["session", "rename", "lead name", "--session", &lead])
        .ok();
    assert_eq!(pane_title(&fx, &pane), "Dynamic Subject");

    // A rerun with the same title repairs a cleared header to the dynamic title.
    fx.tmux(&["set", "-p", "-u", "-t", &pane, "@cpd_title"]);
    let again = fx
        .cpd(&[
            "plan",
            "rename",
            "--plan",
            "pay",
            "--title",
            "Retry the payouts",
        ])
        .ok();
    assert_eq!(again["already"], true);
    assert_eq!(pane_title(&fx, &pane), "Dynamic Subject");
}

#[test]
fn rename_refusals() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    inproc::new_plan(&fx, "pay", &repo);
    refused(
        &fx.cpd(&["plan", "rename", "--plan", "nope", "--title", "X"]),
        "plan_not_found",
        4,
    );
    refused(
        &fx.cpd(&["plan", "rename", "--title", "X"]),
        "plan_required",
        2,
    );
    refused(
        &fx.cpd(&["plan", "rename", "--plan", "pay", "--title", ""]),
        "usage",
        2,
    );
    assert!(inproc::events(&fx, "plan-renamed").is_empty());
}

#[test]
fn rename_works_on_an_archived_plan() {
    let fx = Fx::with_ports();
    inproc::setup(&fx);
    inproc::new_plan_only(&fx, "pay");
    fx.cpd(&["plan", "archive", "--plan", "pay"]).ok();
    let out = fx
        .cpd(&[
            "plan",
            "rename",
            "--plan",
            "pay",
            "--title",
            "Archived and renamed",
        ])
        .ok();
    assert_eq!(out["plan"]["title"], "Archived and renamed");
    let plan = inproc::show(&fx, "pay");
    assert_eq!(plan["plan"]["title"], "Archived and renamed");
    assert_ne!(plan["plan"]["archived_at"], Value::Null, "still archived");
    assert_eq!(inproc::events(&fx, "plan-renamed").len(), 1);
}

// ---- plan archive and unarchive -------------------------------------------

/// Plan `pay` with its Lead and one side fork, each with a live parked pane.
fn plan_with_fork(fx: &Fx) -> (String, String) {
    let repo = inproc::setup(fx);
    let lead = inproc::new_plan(fx, "pay", &repo)["lead"]["session"]
        .as_str()
        .unwrap()
        .to_owned();
    let fork = fx.cpd(&["session", "fork", &lead]).ok()["session"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    (lead, fork)
}

fn has_pane(fx: &Fx, session: &str) -> bool {
    inproc::panes(fx).iter().any(|p| p.2 == session)
}

fn archived_ats(env: &TestEnv) -> i64 {
    env.query(
        "SELECT count(*) FROM (SELECT archived_at FROM contexts UNION ALL SELECT archived_at FROM plans
                              UNION ALL SELECT archived_at FROM sessions) WHERE archived_at IS NOT NULL",
    )
}

fn event_count(env: &TestEnv) -> i64 {
    env.query("SELECT count(*) FROM events")
}

#[test]
fn archive_covers_the_plan_its_lead_and_forks_kills_their_panes_and_appends_one_event_each() {
    let fx = Fx::with_ports();
    let (lead, fork) = plan_with_fork(&fx);
    let (lead_pane, fork_pane) = (
        inproc::panes(&fx)
            .into_iter()
            .find(|p| p.2 == lead)
            .unwrap()
            .0,
        inproc::panes(&fx)
            .into_iter()
            .find(|p| p.2 == fork)
            .unwrap()
            .0,
    );
    let listed = fx.cpd(&["plan", "list"]).ok();
    assert_eq!(listed["plans"][0]["archived_at"], Value::Null);
    assert_eq!(inproc::show(&fx, "pay")["plan"]["archived_at"], Value::Null);

    let out = fx.cpd(&["plan", "archive", "--plan", "pay"]).ok();
    assert_eq!(out["archived"][0], json!({"kind": "plan", "id": "pay"}));
    let mut sessions: Vec<&str> = out["archived"].as_array().unwrap()[1..]
        .iter()
        .map(|o| o["id"].as_str().unwrap())
        .collect();
    sessions.sort();
    let mut expected = vec![lead.as_str(), fork.as_str()];
    expected.sort();
    assert_eq!(sessions, expected);
    assert!(
        out["archived"].as_array().unwrap()[1..]
            .iter()
            .all(|o| o["kind"] == "session"),
        "{out}"
    );
    let mut closed: Vec<&str> = out["closed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p.as_str().unwrap())
        .collect();
    closed.sort();
    let mut panes_closed = vec![lead_pane.as_str(), fork_pane.as_str()];
    panes_closed.sort();
    assert_eq!(closed, panes_closed);
    assert!(!has_pane(&fx, &lead) && !has_pane(&fx, &fork));

    assert_eq!(archived_ats(&fx.env), 3);
    let plan_events = inproc::events(&fx, "plan-archived");
    assert_eq!(plan_events.len(), 1);
    assert_eq!(
        (plan_events[0]["plan"].as_str(), &plan_events[0]["data"]),
        (Some("pay"), &json!({"archived": true}))
    );
    let session_events = inproc::events(&fx, "session-archived");
    assert_eq!(session_events.len(), 2);
    assert!(
        session_events
            .iter()
            .all(|e| e["data"] == json!({"archived": true}) && e["plan"] == "pay")
    );
    let at = inproc::show(&fx, "pay")["plan"]["archived_at"].clone();
    assert!(at.is_string());
    assert_eq!(
        fx.cpd(&["plan", "list"]).ok()["plans"][0]["archived_at"],
        at
    );
    assert_eq!(
        fx.env
            .query::<String>(&format!("SELECT state FROM sessions WHERE id = '{lead}'")),
        "stopped"
    );

    // Again: nothing is written.
    let before = event_count(&fx.env);
    let again = fx.cpd(&["plan", "archive", "--plan", "pay"]).ok();
    assert_eq!(
        again,
        json!({"ok": true, "archived": [], "closed": [], "already": true})
    );
    assert_eq!(event_count(&fx.env), before);
}

#[test]
fn unarchive_clears_the_cascade_with_one_event_each_and_touches_no_pane() {
    let fx = Fx::with_ports();
    let (lead, fork) = plan_with_fork(&fx);
    fx.cpd(&["plan", "archive", "--plan", "pay"]).ok();
    let events_before = event_count(&fx.env);

    let out = fx.cpd(&["plan", "unarchive", "--plan", "pay"]).ok();
    assert_eq!(out["archived"].as_array().unwrap().len(), 3, "{out}");
    assert_eq!(out["archived"][0], json!({"kind": "plan", "id": "pay"}));
    assert_eq!(out["closed"], json!([]));
    assert_eq!(archived_ats(&fx.env), 0);
    assert_eq!(event_count(&fx.env), events_before + 3);
    assert!(inproc::events(&fx, "plan-archived")[1]["data"] == json!({"archived": false}));
    assert!(
        !has_pane(&fx, &lead) && !has_pane(&fx, &fork),
        "no pane is started"
    );
    assert_eq!(inproc::show(&fx, "pay")["plan"]["archived_at"], Value::Null);

    let again = fx.cpd(&["plan", "unarchive", "--plan", "pay"]).ok();
    assert_eq!(
        again,
        json!({"ok": true, "archived": [], "closed": [], "already": true})
    );
    assert_eq!(event_count(&fx.env), events_before + 3);
}

#[test]
fn unarchive_inside_an_archived_context_is_refused() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    inproc::new_plan(&fx, "pay", &repo);
    fx.cpd(&["context", "archive", "ctx"]).ok();
    let events_before = event_count(&fx.env);
    let archived = archived_ats(&fx.env);
    let error = refused(
        &fx.cpd(&["plan", "unarchive", "--plan", "pay"]),
        "archived",
        3,
    );
    assert_eq!(error["details"]["next"], "cpd context unarchive ctx");
    assert_eq!(
        (event_count(&fx.env), archived_ats(&fx.env)),
        (events_before, archived)
    );
    refused(
        &fx.cpd(&["plan", "unarchive", "--plan", "nope"]),
        "plan_not_found",
        4,
    );
    refused(
        &fx.cpd(&["plan", "archive", "--plan", "nope"]),
        "plan_not_found",
        4,
    );
    refused(&fx.cpd(&["plan", "archive"]), "plan_required", 2);
}

#[test]
fn plan_new_in_an_archived_context_is_refused_before_any_write() {
    let fx = Fx::with_ports();
    inproc::setup(&fx);
    fx.cpd(&["context", "archive", "ctx"]).ok();
    let events_before = event_count(&fx.env);
    let args = ["plan", "new", "pay", "--context", "ctx", "--title", "Pay"];
    let error = refused(&fx.cpd(&args), "archived", 3);
    assert_eq!(error["details"]["next"], "cpd context unarchive ctx");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plans"), 0);
    assert_eq!(event_count(&fx.env), events_before);
    assert!(inproc::panes(&fx).is_empty());

    fx.cpd(&["context", "unarchive", "ctx"]).ok();
    fx.cpd(&args).ok();
}

const JOB: &str = "INSERT INTO jobs (plan, key, workflow, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'api', 'pay', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '$STATUS', 0, '$FP', 1, 't');";
/// Job `api` of `pay`; a locked one (not `draft`) has its fingerprint.
fn job(status: &str) -> String {
    let fingerprint = if status == "draft" {
        "NULL".to_owned()
    } else {
        format!("'{}'", "a".repeat(64))
    };
    JOB.replace("'$FP'", &fingerprint)
        .replace("$STATUS", status)
}

const ATTEMPT: &str = "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
                       VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', '$STATUS', 'cpd/pay--api-a1', '/w', 't');";

#[test]
fn archive_refused_by_any_one_blocker_changes_nothing() {
    let review = format!(
        "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at) VALUES ('pay', 'r1', 1, '{}', 'pending', 't');",
        "a".repeat(64)
    );
    let escalation = format!(
        "{}{}INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
         VALUES ('pay', 'api-a1-q1', 'api-a1', 1, 'Which?', 'open', 1, 't');",
        job("draft").replace("'draft'", "'cancelled'").replace("status,", "cancel_reason, cancel_requested_at, status,").replace("'cancelled'", "'a fixture', 't', 'cancelled'"),
        ATTEMPT.replace("$STATUS", "running")
    );
    // One blocker at a time, each in its own environment: Jobs are append-only.
    for (kind, apply) in [
        ("job", job("queued")),
        ("job", job("running")),
        ("escalation", escalation),
        ("review", review),
        ("session", String::new()),
    ] {
        let fx = Fx::with_ports();
        let (_, fork) = plan_with_fork(&fx);
        let apply = if kind == "session" {
            format!("UPDATE sessions SET state = 'working' WHERE id = '{fork}'")
        } else {
            apply
        };
        store(&fx.env)
            .write("fixture", |tx| exec(tx, &apply))
            .unwrap();
        let (events_before, panes_before) = (event_count(&fx.env), inproc::panes(&fx));
        let error = refused(
            &fx.cpd(&["plan", "archive", "--plan", "pay"]),
            "archive_blocked",
            3,
        );
        let listed = error["details"]["blockers"].as_array().unwrap();
        assert_eq!(listed.len(), 1, "{kind}: {error}");
        assert_eq!(listed[0]["kind"], kind, "{error}");
        assert_eq!(archived_ats(&fx.env), 0, "{kind}");
        assert_eq!(event_count(&fx.env), events_before, "{kind}");
        assert_eq!(inproc::panes(&fx), panes_before, "{kind}");
    }
}

#[test]
fn archive_lists_every_blocker() {
    let fx = Fx::with_ports();
    plan_with_fork(&fx);
    store(&fx.env)
        .write("fixture", |tx| exec(tx, &job("queued")))
        .unwrap();
    store(&fx.env)
        .write("fixture", |tx| {
            exec(tx, &format!("INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at) VALUES ('pay', 'r1', 1, '{}', 'pending', 't');", "a".repeat(64)))
        })
        .unwrap();
    let error = refused(
        &fx.cpd(&["plan", "archive", "--plan", "pay"]),
        "archive_blocked",
        3,
    );
    let kinds: Vec<&str> = error["details"]["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["job", "review"]);
}

#[test]
fn a_plan_in_phase_pr_with_no_live_job_archives() {
    let fx = Fx::with_ports();
    let (lead, _) = plan_with_fork(&fx);
    fx.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    store(&fx.env)
        .write("fixture", |tx| {
            exec(tx, "UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay'")?;
            exec(tx, "UPDATE workflows SET status = 'dispatch', lead_worktree = '/w', started_at = 't' WHERE plan = 'pay'")?;
            exec(
                tx,
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
                 VALUES ('acme/pay', 1, 'https://github.com/acme/pay/pull/1', 'Pay', 0, 'open', 'none', 'none', 't', 't')",
            )?;
            exec(
                tx,
                "UPDATE workflows SET status = 'open', pr_number = 1, pr_url = 'https://github.com/acme/pay/pull/1', pr_opened_at = 't',
                        pull_request = (SELECT id FROM pull_requests WHERE number = 1) WHERE plan = 'pay'",
            )?;
            exec(tx, "UPDATE plans SET phase = 'deploy' WHERE id = 'pay'")
        })
        .unwrap();
    let out = fx.cpd(&["plan", "archive", "--plan", "pay"]).ok();
    assert_eq!(out["archived"][0], json!({"kind": "plan", "id": "pay"}));
    assert!(!has_pane(&fx, &lead));
    assert_eq!(inproc::show(&fx, "pay")["plan"]["phase"], "deploy");
    assert!(inproc::show(&fx, "pay")["plan"]["archived_at"].is_string());
}

#[test]
fn archive_refuses_a_lead_in_the_chat_slot_before_closing_any_pane() {
    let fx = Fx::with_ports();
    let (lead, fork) = plan_with_fork(&fx);
    // The chat slot: the pane of window `main` running a Lead (the fork).
    let fork_pane = inproc::panes(&fx)
        .into_iter()
        .find(|p| p.2 == fork)
        .unwrap()
        .0;
    fx.tmux(&["rename-window", "-t", &fork_pane, "main"]);
    let (events_before, panes_before) = (event_count(&fx.env), inproc::panes(&fx));
    let error = refused(
        &fx.cpd(&["plan", "archive", "--plan", "pay"]),
        "in_chat_slot",
        3,
    );
    assert_eq!(error["details"]["next"], "cpd session park");
    assert!(has_pane(&fx, &lead));
    assert_eq!(
        (
            event_count(&fx.env),
            inproc::panes(&fx),
            archived_ats(&fx.env)
        ),
        (events_before, panes_before, 0)
    );
}

// ---------- stub drafts (hs-d241, R1; commands.md §4.5, §4.6, §4.8, §5.1, §5.2) ----------

/// What `job edit --tier` checks: the `[model]` of the machine policy and a fake pi install whose Tier catalog
/// is cached, as in `cli/tests/job.rs`, so no node runs.
fn seed_tiers(env: &TestEnv) {
    env.write_models();
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
    let rows: String = [("O", "p/opus", "[0,1,2,3,4,5]"), ("S", "p/sonnet", "[1,2,3,4,5]"), ("H", "p/haiku", "[0,1,2,3]")]
        .iter()
        .map(|(letter, model, efforts)| {
            format!("INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{model}', '{efforts}', 't');")
        })
        .collect();
    store(env).write("fixture", |tx| exec(tx, &rows)).unwrap();
}

/// A Spec file for `job edit --spec`.
fn spec_file(env: &TestEnv, name: &str) -> String {
    let path = env.root().join(format!("{name}.json"));
    let spec = json!({"goal": format!("Goal of {name}."), "context": "Why.", "touches": [format!("{name}.rs")], "acceptance": ["cargo test"], "out_of_scope": []});
    fs::write(&path, spec.to_string()).unwrap();
    path.to_str().unwrap().to_owned()
}

/// hs-n677: a stub added in dispatch refuses the review `draft_incomplete` whether it waits or not; completed and no
/// longer waiting, every draft is under the review and the approval queues it.
#[test]
fn a_stub_added_in_dispatch_refuses_the_review_until_it_is_completed_then_is_approved_and_queued() {
    let fx = Fx::with_ports();
    let repo = inproc::locking(&fx);
    inproc::approved(&fx, &repo);
    fx.cpd(&["plan", "lock", "--plan", "pay"]).ok();

    // A follow-up batch is registered at once: two stubs and a complete draft, all waiting on the queued Job `a`,
    // and a stub that is dropped again.
    let out = inproc::stub(&fx, "next-1", &["--needs", "a"]);
    assert_eq!(
        (
            &out["job"]["status"],
            &out["job"]["tier"],
            &out["job"]["appended"]
        ),
        (&json!("draft"), &Value::Null, &json!(true))
    );
    inproc::stub(&fx, "next-2", &["--needs", "next-1"]);
    inproc::stub(&fx, "dropped", &["--needs", "a"]);
    fx.cpd(&[
        "job",
        "cancel",
        "dropped",
        "--reason",
        "Not needed.",
        "--plan",
        "pay",
    ])
    .ok();
    add_draft(&fx.env, "pay", "done-1", &["a"], true);
    let shown = inproc::show(&fx, "pay");
    let listed = shown["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["key"] == "next-1")
        .unwrap();
    assert_eq!(
        (
            &listed["status"],
            &listed["tier"],
            &listed["goal"],
            &listed["touches"]
        ),
        (&json!("draft"), &Value::Null, &Value::Null, &Value::Null)
    );
    assert_eq!(shown["counts"]["draft"], 3);
    // The Plan view of the Context holds the stub's registered work too.
    let pending = fx.cpd(&["context", "pending", "--context", "ctx"]).ok();
    let jobs = &pending["plans"][0]["workflows"][0]["jobs"];
    let pending_stub = jobs
        .as_array()
        .unwrap()
        .iter()
        .find(|j| j["key"] == "next-1")
        .unwrap();
    assert_eq!(
        (&pending_stub["status"], &pending_stub["touches"]),
        (&json!("draft"), &json!([]))
    );

    // The stubs count as incomplete whether they wait or not: the review is refused, naming next-1 and next-2.
    fx.outline_for_review("pay");
    let e = refused(
        &fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"]),
        "draft_incomplete",
        3,
    );
    assert_eq!(
        e["details"]["jobs"],
        json!([
            {"key": "next-1", "missing": ["tier", "spec"]},
            {"key": "next-2", "missing": ["tier", "spec"]}
        ]),
        "{e}"
    );

    // Completed, every draft is under the review: the request succeeds and the approval queues them all.
    seed_tiers(&fx.env);
    let spec = spec_file(&fx.env, "batch");
    fx.cpd(&[
        "job",
        "edit",
        "next-1",
        "--no-needs",
        "--tier",
        "S2",
        "--spec",
        &spec,
        "--plan",
        "pay",
    ])
    .ok();
    fx.cpd(&[
        "job", "edit", "next-2", "--tier", "S2", "--spec", &spec, "--plan", "pay",
    ])
    .ok();
    let out = fx
        .cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
        .ok();
    assert_eq!(
        (&out["review"]["id"], &out["review"]["status"]),
        (&json!("r2"), &json!("pending"))
    );
    let out = fx
        .cpd(&["plan", "decide", "r2", "--approve", "--plan", "pay"])
        .ok();
    assert_eq!(out["current"], true);
    assert_eq!(out["jobs"], json!(["done-1", "next-1", "next-2"]));
    let decided = inproc::events(&fx, "plan-review").last().unwrap().clone();
    assert_eq!(
        decided["data"]["jobs"],
        json!(["done-1", "next-1", "next-2"])
    );
    let shown = inproc::show(&fx, "pay");
    for key in ["done-1", "next-1", "next-2"] {
        assert_eq!(job_status(&fx.env, key), "queued", "{key}");
    }
    assert_eq!(job_status(&fx.env, "dropped"), "cancelled");
    assert_eq!(shown["tamper"], json!([]));
    assert_eq!(shown["counts"]["draft"], 0);
}

#[test]
fn a_stub_in_phase_plan_refuses_the_review_and_the_lock_and_is_never_queued() {
    let fx = Fx::with_ports();
    let repo = inproc::locking(&fx);
    seed_tiers(&fx.env);
    inproc::approved(&fx, &repo);
    inproc::stub(&fx, "later", &[]);
    inproc::stub(&fx, "also", &["--tier", "S2"]);
    let lock = || fx.cpd(&["plan", "lock", "--plan", "pay"]);

    // The approved review no longer covers the Jobs; the lock names the stubs before it names the review.
    let error = refused(&lock(), "draft_incomplete", 3);
    assert_eq!(
        error["details"]["jobs"],
        json!([{"key": "also", "missing": ["spec"]}, {"key": "later", "missing": ["tier", "spec"]}])
    );
    let error = refused(
        &fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"]),
        "draft_incomplete",
        3,
    );
    assert_eq!(
        error["details"]["jobs"],
        json!([{"key": "also", "missing": ["spec"]}, {"key": "later", "missing": ["tier", "spec"]}])
    );
    assert_eq!(inproc::show(&fx, "pay")["plan"]["phase"], "plan");
    assert_eq!(job_status(&fx.env, "later"), "draft");
    assert!(inproc::watch_calls(&fx).is_empty());

    // Cancelled, a stub is no draft and blocks nothing; the other is completed.
    fx.cpd(&["job", "cancel", "later", "--reason", "No.", "--plan", "pay"])
        .ok();
    fx.cpd(&[
        "job",
        "edit",
        "also",
        "--spec",
        &spec_file(&fx.env, "also"),
        "--plan",
        "pay",
    ])
    .ok();
    fx.outline_for_review("pay");
    fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Review title"])
        .ok();
    fx.cpd(&["plan", "decide", "r2", "--approve", "--plan", "pay"])
        .ok();
    let out = lock().ok();
    assert_eq!(out["jobs"], json!(["a", "b", "also"]), "in creation order");
    assert_eq!(job_status(&fx.env, "also"), "queued");
    assert_eq!(job_status(&fx.env, "later"), "cancelled");
}

#[test]
fn render_shows_what_a_stub_lacks_and_the_fingerprint_covers_it_as_it_is() {
    let fx = Fx::with_ports();
    let repo = inproc::setup(&fx);
    seed_tiers(&fx.env);
    inproc::new_plan(&fx, "pay", &repo);
    fx.cpd(&["plan", "phase", "plan", "--plan", "pay"]).ok();
    inproc::stub(&fx, "bare", &[]);
    inproc::stub(&fx, "tiered", &["--tier", "S3"]);
    add_job(&fx.env, "pay", "full", &[]);
    // Another Plan's stub in the same repo does not break the overlap check.
    inproc::new_plan(&fx, "oth", &repo);
    fx.cpd(&["plan", "phase", "plan", "--plan", "oth"]).ok();
    fx.cpd(&[
        "job",
        "add",
        "elsewhere",
        "--draft",
        "--title",
        "Elsewhere",
        "--plan",
        "oth",
    ])
    .ok();

    let out = fx.cpd(&["plan", "render", "--plan", "pay"]).ok();
    assert_eq!(
        (&out["draft_jobs"], &out["overlaps"]),
        (&json!(3), &json!([]))
    );
    let text = fs::read_to_string(home(&fx.env).plan_md(&pid("pay"))).unwrap();
    // The table row and the section show a missing Tier as `—`; the Spec of a stub is not written yet.
    assert!(
        text.contains("| `bare` | Planned bare | work | — | draft | none | — |"),
        "{text}"
    );
    assert!(
        text.contains("| `tiered` | Planned tiered | work | S3 | draft | none | — |"),
        "{text}"
    );
    assert!(
        text.contains("| `full` | Job full | work | S2 | draft | none | `src/full.rs` |"),
        "{text}"
    );
    assert!(text.contains("\n### `bare`: Planned bare\n\nStatus: `draft` · Tier: — · Needs: none\n\nSpec: not written yet\n"), "{text}");
    assert!(text.contains("\n### `tiered`: Planned tiered\n\nStatus: `draft` · Tier: `S3` · Needs: none\n\nSpec: not written yet\n"), "{text}");
    assert!(text.contains("\n### `full`: Job full\n\nStatus: `draft` · Tier: `S2` · Needs: none\n\n#### Goal\n\nGoal of full.\n"), "{text}");
    assert_eq!(text.matches("Spec: not written yet").count(), 2, "{text}");

    // The fingerprint hashes a stub as it is: its missing fields are null.
    let (pr_repo, pr_base): (String, String) = store(&fx.env)
        .read(|c| {
            Ok(c.query_row(
                "SELECT repo, base FROM workflows WHERE plan = 'pay'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    let null_spec = |key: &str, tier: &str| {
        format!(
            "{{\"acceptance\":null,\"context\":null,\"goal\":null,\"key\":\"{key}\",\"needs\":[],\"out_of_scope\":null,\"pr\":\"pay\",\"tier\":{tier},\"title\":\"Planned {key}\",\"touches\":null}}"
        )
    };
    let full = "{\"acceptance\":[\"cargo test\"],\"context\":\"Context of full.\",\"goal\":\"Goal of full.\",\"key\":\"full\",\"needs\":[],\"out_of_scope\":[],\"pr\":\"pay\",\"tier\":\"S2\",\"title\":\"Job full\",\"touches\":[\"src/full.rs\"]}";
    let prs = format!(
        "[{{\"base\":\"{pr_base}\",\"key\":\"pay\",\"needs\":[],\"repo\":{},\"shape\":\"config\"}}]",
        serde_json::to_string(&pr_repo).unwrap()
    );
    assert_eq!(
        out["fingerprint"],
        sha256(&format!(
            "{{\"jobs\":[{},{full},{}],\"prs\":{prs}}}",
            null_spec("bare", "null"),
            null_spec("tiered", "\"S3\"")
        ))
    );

    // Completing a stub changes it, and the render shows the Tier and the Spec.
    fx.cpd(&[
        "job",
        "edit",
        "bare",
        "--tier",
        "S2",
        "--spec",
        &spec_file(&fx.env, "bare"),
        "--plan",
        "pay",
    ])
    .ok();
    let completed = fx.cpd(&["plan", "render", "--plan", "pay"]).ok();
    assert_ne!(completed["fingerprint"], out["fingerprint"]);
    let text = fs::read_to_string(home(&fx.env).plan_md(&pid("pay"))).unwrap();
    assert!(
        text.contains("| `bare` | Planned bare | work | S2 | draft | none | `bare.rs` |"),
        "{text}"
    );
    assert_eq!(text.matches("Spec: not written yet").count(), 1, "{text}");
}
