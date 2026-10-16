//! Contexts (contract §2.2, §4.6; commands.md §3): every command run as the
//! built binary, the index rebuilt from the files, and `context sync` against
//! a local bare repository.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Run, TestEnv};
use cpd_core::config::Config;
use cpd_core::context::{Contexts, Trigger};
use cpd_core::home::CpdHome;
use cpd_core::ids::ContextId;
use cpd_core::store::Store;
use serde_json::{Value, json};

const CTX: &str = "building-cpd";

/// A machine `name` with a config file.
fn machine(name: &str) -> TestEnv {
    let env = TestEnv::new();
    fs::create_dir_all(env.cpd_home()).unwrap();
    fs::write(env.cpd_home().join("config.toml"), format!("version = 1\nmachine = \"{name}\"\n")).unwrap();
    env
}

fn hs() -> TestEnv {
    machine("hs")
}

fn dir(env: &TestEnv, context: &str) -> PathBuf {
    env.cpd_home().join("contexts").join(context)
}

fn records_file(env: &TestEnv, context: &str, machine: &str) -> PathBuf {
    dir(env, context).join("records").join(format!("{machine}.jsonl"))
}

fn lines(path: &Path) -> Vec<Value> {
    fs::read_to_string(path).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

fn new_context(env: &TestEnv) {
    env.cpd(&["context", "new", CTX, "--title", "Building cpd"]).ok();
}

fn code(run: &Run) -> String {
    run.error()["code"].as_str().unwrap().to_owned()
}

fn refused(run: Run, exit: i32, expected: &str) -> Value {
    let error = run.error();
    assert_eq!(run.exit, exit, "{run:?}");
    assert_eq!(error["code"], expected, "{error}");
    assert_eq!(run.stderr, "", "{run:?}");
    error
}

fn events(env: &TestEnv, kind: &str) -> Vec<Value> {
    let run = env.cpd(&["events", "list", "--kind", kind]);
    run.ok()["events"].as_array().unwrap().clone()
}

fn library(env: &TestEnv) -> (CpdHome, Config, Store) {
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    let config = Config::load(&home).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (home, config, store)
}

/// A bare repository, as the Context stores' private remote.
fn bare(env: &TestEnv, name: &str) -> PathBuf {
    let path = env.root().join(name);
    env.git(env.root(), &["init", "-q", "--bare", "-b", "main", path.to_str().unwrap()]);
    path
}

fn add_origin(env: &TestEnv, context: &str, remote: &Path) {
    env.git(&dir(env, context), &["remote", "add", "origin", remote.to_str().unwrap()]);
}

fn git_log(env: &TestEnv, context: &str) -> Vec<String> {
    env.git(&dir(env, context), &["log", "--format=%an <%ae>|%s"]).lines().map(str::to_owned).collect()
}

fn fixture_plan(env: &TestEnv, plan: &str, context: &str) {
    let (_, _, mut store) = library(env);
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead-{plan}', 'lead', '{context}', '{plan}', 'Lead', '/', 'O3', 'idle', 't', 't');
                 INSERT INTO plans (id, context, title, repo, base, lead_branch, phase, lead_session, created_at)
                   VALUES ('{plan}', '{context}', 'The {plan} plan', '/r', 'main', 'cpd/{plan}', 'grounding', 'lead-{plan}', 't');"
            ))?;
            Ok(())
        })
        .unwrap();
}

// ---- context new ----------------------------------------------------------

#[test]
fn new_creates_the_store_the_row_and_the_event() {
    let env = hs();
    let out = env.cpd(&["context", "new", CTX, "--title", "Building cpd", "--jira", "CPD-12"]).ok();
    let context = &out["context"];
    assert_eq!(context["id"], CTX);
    assert_eq!(context["title"], "Building cpd");
    assert_eq!(context["jira"], "CPD-12");
    assert_eq!(context["created_on"], "hs");
    assert_eq!(context["dir"], dir(&env, CTX).to_str().unwrap());
    assert!(context["created_at"].as_str().unwrap().ends_with('Z'));
    assert!(out.get("already").is_none());

    let d = dir(&env, CTX);
    let manifest: Value = serde_json::from_str(&fs::read_to_string(d.join("context.json")).unwrap()).unwrap();
    assert_eq!(manifest["v"], 1);
    assert_eq!(manifest["id"], CTX);
    assert_eq!(manifest["jira"], "CPD-12");
    assert_eq!(manifest["created_on"], "hs");
    assert_eq!(fs::read_to_string(d.join(".gitattributes")).unwrap(), "records/*.jsonl merge=union\n");
    assert_eq!(fs::read_to_string(d.join(".gitignore")).unwrap(), "*.tmp\n");
    assert_eq!(fs::read_to_string(records_file(&env, CTX, "hs")).unwrap(), "");
    assert_eq!(env.git(&d, &["branch", "--show-current"]), "main");
    assert_eq!(env.git(&d, &["status", "--porcelain"]), "");
    // Committed as cpd <machine>, whatever the machine's git identity is.
    assert_eq!(git_log(&env, CTX), ["cpd hs <cpd@hs.invalid>|context building-cpd: Building cpd"]);
    assert!(env.git(&d, &["remote"]).is_empty());

    let created = events(&env, "context-created");
    assert_eq!(created.len(), 1);
    assert_eq!(created[0]["context"], CTX);
    assert_eq!(created[0]["data"], json!({"title": "Building cpd", "jira": "CPD-12", "created_on": "hs"}));
}

#[test]
fn new_is_idempotent_for_the_same_title_and_jira() {
    let env = hs();
    env.cpd(&["context", "new", CTX, "--title", "Building cpd"]).ok();
    let again = env.cpd(&["context", "new", CTX, "--title", "Building cpd"]).ok();
    assert_eq!(again["already"], true);
    assert_eq!(again["context"]["id"], CTX);
    assert_eq!(git_log(&env, CTX).len(), 1);
    assert_eq!(events(&env, "context-created").len(), 1);
}

#[test]
fn new_refuses_an_existing_context_with_another_title_or_jira() {
    let env = hs();
    new_context(&env);
    for args in [
        vec!["context", "new", CTX, "--title", "Another"],
        vec!["context", "new", CTX, "--title", "Building cpd", "--jira", "CPD-1"],
    ] {
        refused(env.cpd(&args), 3, "context_exists");
    }
}

#[test]
fn new_refuses_a_directory_the_index_does_not_know() {
    let env = hs();
    new_context(&env);
    fs::remove_file(env.cpd_home().join("cpd.db")).unwrap();
    let error = refused(env.cpd(&["context", "new", CTX, "--title", "Building cpd"]), 3, "context_exists");
    assert_eq!(error["details"]["next"], format!("cpd context sync --context {CTX}"));
}

#[test]
fn new_checks_its_arguments() {
    let env = hs();
    refused(env.cpd(&["context", "new", "Bad_Id", "--title", "T"]), 2, "invalid_context_id");
    refused(env.cpd(&["context", "new", CTX, "--title", "T", "--jira", "cpd-1"]), 2, "usage");
    refused(env.cpd(&["context", "new", CTX, "--title", "T", "--jira", "C-1"]), 2, "usage");
    refused(env.cpd(&["context", "new", CTX, "--title", ""]), 2, "usage");
    refused(env.cpd(&["context", "new", CTX]), 2, "usage");
    assert!(!dir(&env, CTX).exists());
    let no_config = TestEnv::new();
    refused(no_config.cpd(&["context", "new", CTX, "--title", "T"]), 1, "config_missing");
}

// ---- context decide, note, ask --------------------------------------------

#[test]
fn decide_note_and_ask_append_records() {
    let env = hs();
    new_context(&env);

    let d1 = env.cpd(&["context", "decide", "One engine per machine.", "--context", CTX]).ok();
    assert_eq!(d1["record"]["id"], "hs-d1");
    assert_eq!(d1["record"]["kind"], "decision");
    assert_eq!(d1["record"]["machine"], "hs");
    assert_eq!(d1["record"]["text"], "One engine per machine.");
    assert_eq!(d1["record"]["by"], json!({"session": null, "plan": null}));
    assert!(d1["record"].get("supersedes").is_none());
    assert_eq!(d1["seq"], 1);
    assert!(d1.get("already").is_none());

    let n1 = env.cpd(&["context", "note", "A note.", "--context", CTX]).ok();
    assert_eq!(n1["record"]["id"], "hs-n1");
    assert_eq!(n1["seq"], 2);
    let q1 = env.cpd(&["context", "ask", "Why?", "--context", CTX]).ok();
    assert_eq!(q1["record"]["id"], "hs-q1");
    assert_eq!(q1["record"]["kind"], "question");
    let d2 = env.cpd(&["context", "decide", "Two engines.", "--supersedes", "hs-d1", "--context", CTX]).ok();
    assert_eq!(d2["record"]["id"], "hs-d2");
    assert_eq!(d2["record"]["supersedes"], "hs-d1");
    assert_eq!(d2["seq"], 4);

    // The file holds one line per record, with "v": 1; the index agrees.
    let file = lines(&records_file(&env, CTX, "hs"));
    assert_eq!(file.len(), 4);
    assert!(file.iter().all(|l| l["v"] == 1));
    assert_eq!(file[3]["supersedes"], "hs-d1");
    assert_eq!(file[0]["by"], json!({"session": null, "plan": null}));
    let shown = env.cpd(&["context", "show", "--context", CTX]);
    let ids: Vec<_> = shown.ok()["records"].as_array().unwrap().iter().map(|r| r["id"].clone()).collect();
    assert_eq!(ids, ["hs-d1", "hs-n1", "hs-q1", "hs-d2"]);

    // One git commit per record, named "<kind> <id>: <text>".
    let log = git_log(&env, CTX);
    assert_eq!(log[0], "cpd hs <cpd@hs.invalid>|decision hs-d2: Two engines.");
    assert_eq!(log[1], "cpd hs <cpd@hs.invalid>|question hs-q1: Why?");
    assert_eq!(env.git(&dir(&env, CTX), &["status", "--porcelain"]), "");

    // One context-record Event each.
    let recorded = events(&env, "context-record");
    assert_eq!(recorded.len(), 4);
    assert_eq!(recorded[0]["data"], json!({"record": "hs-d1", "kind": "decision", "machine": "hs", "seq": 1}));
    assert_eq!(recorded[0]["context"], CTX);
}

#[test]
fn a_record_names_its_session_and_plan() {
    let env = hs();
    new_context(&env);
    fixture_plan(&env, "cpd2", CTX);
    let run = |env: &TestEnv, session_var: &str, session: &str, args: &[&str]| {
        let out = env.command(common::CPD).args(args).env(session_var, session).output().unwrap();
        serde_json::from_slice::<Value>(&out.stdout).unwrap()
    };
    // The Context comes from the selected Plan.
    let out = run(&env, "CPD_SESSION", "lead-cpd2", &["context", "note", "From the Lead.", "--plan", "cpd2"]);
    assert_eq!(out["ok"], true, "{out}");
    assert_eq!(out["record"]["by"], json!({"session": "lead-cpd2", "plan": "cpd2"}));
    let out = run(&env, "PI_SESSION_ID", "pi-session", &["context", "note", "From pi.", "--plan", "cpd2"]);
    assert_eq!(out["record"]["by"], json!({"session": "pi-session", "plan": "cpd2"}));
    assert_eq!(lines(&records_file(&env, CTX, "hs"))[1]["by"], json!({"session": "pi-session", "plan": "cpd2"}));
}

#[test]
fn a_record_write_needs_a_context() {
    let env = hs();
    new_context(&env);
    refused(env.cpd(&["context", "note", "x"]), 2, "context_required");
    refused(env.cpd(&["context", "decide", "x"]), 2, "context_required");
    refused(env.cpd(&["context", "ask", "x"]), 2, "context_required");
    refused(env.cpd(&["context", "answer", "hs-q1", "x"]), 2, "context_required");
    refused(env.cpd(&["context", "show"]), 2, "context_required");
    refused(env.cpd(&["context", "note", "x", "--context", "nope-ctx"]), 4, "context_not_found");
    refused(env.cpd(&["context", "note", "x", "--plan", "nope-plan"]), 4, "plan_not_found");
    refused(env.cpd(&["context", "note", "x", "--context", "Bad_Id"]), 2, "invalid_context_id");
    refused(env.cpd(&["context", "note", "x", "--plan", "Bad_Id"]), 2, "invalid_plan_id");
    refused(env.cpd(&["context", "note", ""]), 2, "usage");
}

#[test]
fn decide_refuses_a_missing_or_wrong_kind_of_supersedes() {
    let env = hs();
    new_context(&env);
    env.cpd(&["context", "note", "A note.", "--context", CTX]).ok();
    for target in ["hs-d9", "hs-n1"] {
        let run = env.cpd(&["context", "decide", "New.", "--supersedes", target, "--context", CTX]);
        refused(run, 3, "record_not_found");
    }
    refused(env.cpd(&["context", "decide", "New.", "--supersedes", "nonsense", "--context", CTX]), 2, "invalid_record_id");
    // Nothing was written by the refusals.
    assert_eq!(lines(&records_file(&env, CTX, "hs")).len(), 1);
    assert_eq!(git_log(&env, CTX).len(), 2);
}

#[test]
fn a_retry_returns_the_existing_record() {
    let env = hs();
    new_context(&env);
    let first = env.cpd(&["context", "note", "Same.", "--context", CTX]).ok();
    let again = env.cpd(&["context", "note", "Same.", "--context", CTX]).ok();
    assert_eq!(again["already"], true);
    assert_eq!(again["record"], first["record"]);
    assert_eq!(again["seq"], first["seq"]);
    assert_eq!(lines(&records_file(&env, CTX, "hs")).len(), 1);
    assert_eq!(events(&env, "context-record").len(), 1);

    // Only the latest record counts: after another write, the same text is new.
    env.cpd(&["context", "note", "Other.", "--context", CTX]).ok();
    let third = env.cpd(&["context", "note", "Same.", "--context", CTX]).ok();
    assert!(third.get("already").is_none());
    assert_eq!(third["record"]["id"], "hs-n3");

    // A different session is a different write.
    let out = env
        .command(common::CPD)
        .args(["context", "note", "Same.", "--context", CTX])
        .env("CPD_SESSION", "other-session")
        .output()
        .unwrap();
    let out: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(out["record"]["id"], "hs-n4");
}

#[test]
fn a_failed_commit_leaves_no_record() {
    let env = hs();
    new_context(&env);
    let hook = dir(&env, CTX).join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\necho no >&2\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let error = refused(env.cpd(&["context", "note", "x", "--context", CTX]), 1, "git_failed");
    assert!(error["details"]["argv"].as_array().unwrap().iter().any(|a| a == "commit"));
    assert_eq!(error["details"]["exit"], 1);
    assert!(error["details"]["stderr"].as_str().unwrap().contains("no"));
    assert_eq!(fs::read_to_string(records_file(&env, CTX, "hs")).unwrap(), "");
    assert!(env.cpd(&["context", "show", "--context", CTX]).ok()["records"].as_array().unwrap().is_empty());
    fs::remove_file(hook).unwrap();
    assert_eq!(env.cpd(&["context", "note", "x", "--context", CTX]).ok()["record"]["id"], "hs-n1");
}

// ---- context answer -------------------------------------------------------

#[test]
fn answer_records_the_answer() {
    let env = hs();
    new_context(&env);
    env.cpd(&["context", "ask", "Per machine?", "--context", CTX]).ok();
    let out = env.cpd(&["context", "answer", "hs-q1", "Yes, per machine.", "--context", CTX]).ok();
    assert_eq!(out["record"]["id"], "hs-a1");
    assert_eq!(out["record"]["kind"], "answer");
    assert_eq!(out["record"]["question"], "hs-q1");
    assert_eq!(out["seq"], 2);
    assert_eq!(lines(&records_file(&env, CTX, "hs"))[1]["question"], "hs-q1");
    assert_eq!(git_log(&env, CTX)[0], "cpd hs <cpd@hs.invalid>|answer hs-a1: Yes, per machine.");

    // The same answer again is a retry; another answer is refused.
    let again = env.cpd(&["context", "answer", "hs-q1", "Yes, per machine.", "--context", CTX]).ok();
    assert_eq!(again["already"], true);
    assert_eq!(again["record"]["id"], "hs-a1");
    let error = refused(env.cpd(&["context", "answer", "hs-q1", "No.", "--context", CTX]), 3, "question_answered");
    assert_eq!(error["details"]["answer"], "hs-a1");
    assert_eq!(lines(&records_file(&env, CTX, "hs")).len(), 2);
}

#[test]
fn answer_refuses_what_is_not_a_question() {
    let env = hs();
    new_context(&env);
    env.cpd(&["context", "note", "A note.", "--context", CTX]).ok();
    for question in ["hs-q7", "hs-n1"] {
        refused(env.cpd(&["context", "answer", question, "x", "--context", CTX]), 3, "record_not_found");
    }
    refused(env.cpd(&["context", "answer", "q1", "x", "--context", CTX]), 2, "invalid_record_id");
    assert_eq!(lines(&records_file(&env, CTX, "hs")).len(), 1);
}

// ---- context list and show ------------------------------------------------

#[test]
fn list_summarises_every_context() {
    let env = hs();
    assert_eq!(env.cpd(&["context", "list"]).ok()["contexts"], json!([]));
    new_context(&env);
    env.cpd(&["context", "new", "home-lab", "--title", "Home lab", "--jira", "LAB-3"]).ok();
    env.cpd(&["context", "ask", "One?", "--context", CTX]).ok();
    env.cpd(&["context", "ask", "Two?", "--context", CTX]).ok();
    env.cpd(&["context", "answer", "hs-q1", "Yes.", "--context", CTX]).ok();
    env.cpd(&["context", "note", "n", "--context", CTX]).ok();
    fixture_plan(&env, "cpd2", CTX);

    let out = env.cpd(&["context", "list"]).ok();
    let contexts = out["contexts"].as_array().unwrap();
    assert_eq!(contexts.len(), 2);
    assert_eq!(
        contexts[0],
        json!({"id": CTX, "title": "Building cpd", "jira": null, "archived_at": null, "grounded": false, "lead_session": null,
               "grounded_seq": null, "records": 4, "open_questions": 1, "sync_failing": false,
               "plans": [{"id": "cpd2", "title": "The cpd2 plan", "phase": "grounding", "lead_session": "lead-cpd2", "archived_at": null}]})
    );
    assert_eq!(contexts[1]["id"], "home-lab");
    assert_eq!(contexts[1]["jira"], "LAB-3");
    assert_eq!(contexts[1]["records"], 0);
    assert_eq!(contexts[1]["plans"], json!([]));
    let cursor = out["events_cursor"].as_i64().unwrap();
    let highest = env.cpd(&["events", "list"]).ok()["events"].as_array().unwrap().last().unwrap()["id"].as_i64().unwrap();
    assert_eq!(cursor, highest);
}

#[test]
fn show_renders_the_records() {
    let env = hs();
    new_context(&env);
    env.cpd(&["context", "decide", "Old.", "--context", CTX]).ok();
    env.cpd(&["context", "ask", "Why?", "--context", CTX]).ok();
    env.cpd(&["context", "decide", "New.", "--supersedes", "hs-d1", "--context", CTX]).ok();
    env.cpd(&["context", "answer", "hs-q1", "Because.", "--context", CTX]).ok();
    env.cpd(&["context", "ask", "When?", "--context", CTX]).ok();
    env.cpd(&["context", "note", "A note.", "--context", CTX]).ok();

    let out = env.cpd(&["context", "show", "--context", CTX]).ok();
    assert_eq!(out["context"], json!({"id": CTX, "title": "Building cpd", "jira": null, "grounded": false, "grounded_seq": null}));
    let records = out["records"].as_array().unwrap();
    assert_eq!(records.len(), 6);
    assert_eq!(records[0]["seq"], 1);
    assert_eq!(records[2]["supersedes"], "hs-d1");
    assert_eq!(records[3]["question"], "hs-q1");
    let md = out["markdown"].as_str().unwrap();
    assert!(md.starts_with("# Building cpd\n"), "{md}");
    let positions: Vec<usize> = ["## Decisions", "## Superseded decisions", "## Open questions", "## Answered questions", "## Notes"]
        .iter()
        .map(|h| md.find(h).unwrap_or_else(|| panic!("{h} in {md}")))
        .collect();
    assert!(positions.windows(2).all(|w| w[0] < w[1]), "{md}");
    assert!(md.contains("hs-d1") && md.contains("superseded by hs-d2"), "{md}");
    assert!(md.contains("hs-q2") && md.contains("When?"), "{md}");
    assert!(md.contains("answer hs-a1") && md.contains("Because."), "{md}");
    assert!(md.contains(&format!("({})", &records[0]["at"].as_str().unwrap()[..10])), "{md}");
}

#[test]
fn show_since_lists_only_the_later_records() {
    let env = hs();
    new_context(&env);
    for text in ["one", "two", "three"] {
        env.cpd(&["context", "note", text, "--context", CTX]).ok();
    }
    let out = env.cpd(&["context", "show", "--context", CTX, "--since", "1"]).ok();
    let seqs: Vec<_> = out["records"].as_array().unwrap().iter().map(|r| r["seq"].as_i64().unwrap()).collect();
    assert_eq!(seqs, [2, 3]);
    let md = out["markdown"].as_str().unwrap();
    assert!(md.contains("two") && md.contains("three") && !md.contains("one"), "{md}");
    let none = env.cpd(&["context", "show", "--context", CTX, "--since", "3"]).ok();
    assert_eq!(none["records"], json!([]));
    refused(env.cpd(&["context", "show", "--context", CTX, "--since", "x"]), 2, "usage");
    refused(env.cpd(&["context", "show", "--context", "nope-ctx"]), 4, "context_not_found");
}

#[test]
fn show_takes_the_context_of_the_selected_plan() {
    let env = hs();
    new_context(&env);
    fixture_plan(&env, "cpd2", CTX);
    let out = env.command(common::CPD).args(["context", "show"]).env("CPD_PLAN", "cpd2").output().unwrap();
    let out: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(out["context"]["id"], CTX, "{out}");
}

// ---- the index is rebuilt from the files ---------------------------------

#[test]
fn the_index_is_rebuilt_from_the_files_alone() {
    let env = hs();
    new_context(&env);
    env.cpd(&["context", "decide", "D.", "--context", CTX]).ok();
    env.cpd(&["context", "ask", "Q?", "--context", CTX]).ok();
    env.cpd(&["context", "answer", "hs-q1", "A.", "--context", CTX]).ok();
    env.cpd(&["context", "note", "N.", "--context", CTX]).ok();
    let before = env.cpd(&["context", "show", "--context", CTX]).ok();

    for suffix in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(env.cpd_home().join(format!("cpd.db{suffix}")));
    }
    assert_eq!(env.cpd(&["context", "list"]).ok()["contexts"], json!([]));
    let synced = env.cpd(&["context", "sync"]).ok();
    assert_eq!(
        synced["contexts"],
        json!([{"id": CTX, "synced": false, "reason": "no_remote", "imported": 4, "pushed": false}])
    );
    let after = env.cpd(&["context", "show", "--context", CTX]).ok();
    assert_eq!(after["records"], before["records"]);
    assert_eq!(after["markdown"], before["markdown"]);
    // The Context and its records are Events again, in file order.
    assert_eq!(events(&env, "context-created").len(), 1);
    assert_eq!(events(&env, "context-record").len(), 4);

    // Import is idempotent.
    assert_eq!(env.cpd(&["context", "sync"]).ok()["contexts"][0]["imported"], 0);
    assert_eq!(events(&env, "context-record").len(), 4);
    // Numbering goes on from the files.
    assert_eq!(env.cpd(&["context", "note", "M.", "--context", CTX]).ok()["record"]["id"], "hs-n2");
}

#[test]
fn a_record_committed_but_not_indexed_is_repaired_by_the_next_write() {
    let env = hs();
    new_context(&env);
    env.cpd(&["context", "note", "One.", "--context", CTX]).ok();
    // A crash after the commit and before the index insert: the line is in the
    // file and committed, the index never heard of it.
    let file = records_file(&env, CTX, "hs");
    let mut line = lines(&file)[0].clone();
    line["id"] = json!("hs-n2");
    line["text"] = json!("Crashed.");
    let mut text = fs::read_to_string(&file).unwrap();
    text.push_str(&format!("{line}\n"));
    fs::write(&file, text).unwrap();
    env.git(&dir(&env, CTX), &["commit", "-q", "-a", "-m", "crash"]);

    let next = env.cpd(&["context", "note", "Three.", "--context", CTX]).ok();
    assert_eq!(next["record"]["id"], "hs-n3");
    let ids: Vec<_> = env.cpd(&["context", "show", "--context", CTX]).ok()["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids, ["hs-n1", "hs-n2", "hs-n3"]);
}

#[test]
fn a_malformed_record_file_is_context_store_invalid() {
    let env = hs();
    new_context(&env);
    env.cpd(&["context", "note", "One.", "--context", CTX]).ok();
    let file = records_file(&env, CTX, "hs");
    let good = fs::read_to_string(&file).unwrap();
    let cases = [
        ("not json\n".to_owned(), "line 1"),
        (good.replace("\"v\":1", "\"v\":9"), "version"),
        (good.replace("\"machine\":\"hs\"", "\"machine\":\"zz\""), "machine"),
        (good.replace("\"kind\":\"note\"", "\"kind\":\"idea\""), "kind"),
        (format!("{good}{good}"), "twice"),
        (good.trim_end().to_owned(), "newline"),
    ];
    for (content, why) in cases {
        fs::write(&file, content).unwrap();
        let sync = env.cpd(&["context", "sync", "--context", CTX]);
        let error = refused(sync, 1, "context_store_invalid");
        assert!(error["details"]["path"].as_str().unwrap().ends_with("hs.jsonl"), "{why}: {error}");
        refused(env.cpd(&["context", "note", "x", "--context", CTX]), 1, "context_store_invalid");
    }
    fs::write(&file, &good).unwrap();
    env.cpd(&["context", "sync", "--context", CTX]).ok();

    // A broken manifest, a store that is not a repository, a manifest of another Context.
    let manifest = dir(&env, CTX).join("context.json");
    let good = fs::read_to_string(&manifest).unwrap();
    for broken in ["{", &good.replace(CTX, "other-ctx"), &good.replace("\"v\":1", "\"v\":2")] {
        fs::write(&manifest, broken).unwrap();
        refused(env.cpd(&["context", "sync", "--context", CTX]), 1, "context_store_invalid");
    }
    fs::write(&manifest, good).unwrap();
    fs::rename(dir(&env, CTX).join(".git"), env.root().join("moved-git")).unwrap();
    refused(env.cpd(&["context", "sync", "--context", CTX]), 1, "context_store_invalid");
}

// ---- context sync ---------------------------------------------------------

#[test]
fn sync_without_a_remote_does_nothing() {
    let env = hs();
    new_context(&env);
    let out = env.cpd(&["context", "sync", "--context", CTX]).ok();
    assert_eq!(out["contexts"], json!([{"id": CTX, "synced": false, "reason": "no_remote", "imported": 0, "pushed": false}]));
    refused(env.cpd(&["context", "sync", "--context", "nope-ctx"]), 4, "context_not_found");
    refused(env.cpd(&["context", "sync", "--context", "Bad_Id"]), 2, "invalid_context_id");
    // With no Context at all, there is nothing to sync.
    let empty = hs();
    assert_eq!(empty.cpd(&["context", "sync"]).ok()["contexts"], json!([]));
}

#[test]
fn sync_pushes_and_a_second_machine_imports() {
    let a = hs();
    let b = machine("lap");
    let remote = bare(&a, "remote.git");
    new_context(&a);
    a.cpd(&["context", "decide", "From hs.", "--context", CTX]).ok();
    add_origin(&a, CTX, &remote);
    let out = a.cpd(&["context", "sync", "--context", CTX]).ok();
    assert_eq!(out["contexts"], json!([{"id": CTX, "synced": true, "imported": 0, "pushed": true}]));
    assert_eq!(a.git(&remote, &["log", "--format=%s", "main"]).lines().count(), 2);
    // A second run pushes and imports nothing.
    let again = a.cpd(&["context", "sync", "--context", CTX]).ok();
    assert_eq!(again["contexts"], json!([{"id": CTX, "synced": true, "imported": 0, "pushed": false}]));

    // Machine b clones the store and syncs: the Context is first seen.
    fs::create_dir_all(b.cpd_home().join("contexts")).unwrap();
    b.git(b.root(), &["clone", "-q", remote.to_str().unwrap(), dir(&b, CTX).to_str().unwrap()]);
    let out = b.cpd(&["context", "sync"]).ok();
    assert_eq!(out["contexts"], json!([{"id": CTX, "synced": true, "imported": 1, "pushed": false}]));
    assert_eq!(events(&b, "context-created").len(), 1);
    let show = b.cpd(&["context", "show", "--context", CTX]).ok();
    assert_eq!(show["context"]["title"], "Building cpd");
    assert_eq!(show["records"][0]["id"], "hs-d1");

    // b writes to its own file with its own numbering; a imports it.
    let note = b.cpd(&["context", "note", "From lap.", "--context", CTX]).ok();
    assert_eq!(note["record"]["id"], "lap-n1");
    assert_eq!(note["record"]["machine"], "lap");
    b.cpd(&["context", "decide", "Also lap.", "--supersedes", "hs-d1", "--context", CTX]).ok();
    assert!(records_file(&b, CTX, "lap").exists());
    let out = b.cpd(&["context", "sync"]).ok();
    assert_eq!(out["contexts"][0]["pushed"], true);
    let out = a.cpd(&["context", "sync"]).ok();
    assert_eq!(out["contexts"], json!([{"id": CTX, "synced": true, "imported": 2, "pushed": false}]));
    let ids: Vec<_> = a.cpd(&["context", "show", "--context", CTX]).ok()["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids, ["hs-d1", "lap-n1", "lap-d1"]);
    assert_eq!(events(&a, "context-record").len(), 3);
    // hs-d1 is superseded by the other machine's decision.
    assert!(a.cpd(&["context", "show", "--context", CTX]).ok()["markdown"].as_str().unwrap().contains("superseded by lap-d1"));
    // Both machines have written, and each file is only its own machine's.
    assert!(lines(&records_file(&a, CTX, "lap")).iter().all(|l| l["machine"] == "lap"));
}

#[test]
fn sync_of_two_machines_writing_at_once_merges_cleanly() {
    let a = hs();
    let b = machine("lap");
    let remote = bare(&a, "remote.git");
    new_context(&a);
    add_origin(&a, CTX, &remote);
    a.cpd(&["context", "sync"]).ok();
    fs::create_dir_all(b.cpd_home().join("contexts")).unwrap();
    b.git(b.root(), &["clone", "-q", remote.to_str().unwrap(), dir(&b, CTX).to_str().unwrap()]);
    b.cpd(&["context", "sync"]).ok();

    a.cpd(&["context", "note", "A writes.", "--context", CTX]).ok();
    b.cpd(&["context", "note", "B writes.", "--context", CTX]).ok();
    b.cpd(&["context", "sync"]).ok();
    let out = a.cpd(&["context", "sync"]).ok();
    // a had to merge b's push into its own commit, then push the merge.
    assert_eq!(out["contexts"], json!([{"id": CTX, "synced": true, "imported": 1, "pushed": true}]));
    assert_eq!(a.git(&dir(&a, CTX), &["status", "--porcelain"]), "");
}

#[test]
fn sync_refuses_a_conflict_and_changes_nothing() {
    let a = hs();
    let b = machine("lap");
    let remote = bare(&a, "remote.git");
    new_context(&a);
    add_origin(&a, CTX, &remote);
    a.cpd(&["context", "sync"]).ok();
    // The same Context id created independently on another machine.
    b.cpd(&["context", "new", CTX, "--title", "Building cpd, again"]).ok();
    add_origin(&b, CTX, &remote);
    let head = b.git(&dir(&b, CTX), &["rev-parse", "HEAD"]);
    let run = b.cpd(&["context", "sync", "--context", CTX]);
    let error = refused(run, 3, "context_sync_conflict");
    assert_eq!(error["details"]["files"], json!(["context.json"]));
    assert_eq!(b.git(&dir(&b, CTX), &["rev-parse", "HEAD"]), head);
    assert_eq!(b.git(&dir(&b, CTX), &["status", "--porcelain"]), "");
    assert!(!dir(&b, CTX).join(".git/MERGE_HEAD").exists());
    assert_eq!(b.cpd(&["context", "show", "--context", CTX]).ok()["context"]["title"], "Building cpd, again");
}

#[test]
fn sync_fails_when_the_remote_cannot_be_reached() {
    let env = hs();
    new_context(&env);
    add_origin(&env, CTX, &env.root().join("no-such-remote.git"));
    let run = env.cpd(&["context", "sync", "--context", CTX]);
    let error = refused(run, 1, "context_sync_failed");
    assert!(error["details"]["argv"].as_array().unwrap().iter().any(|a| a == "fetch"));
    assert!(!error["details"]["stderr"].as_str().unwrap().is_empty());
}

/// A pre-push hook that moves the remote under the push, `times` times: the
/// remote moved between the fetch and the push.
fn move_remote_on_push(env: &TestEnv, remote: &Path, times: usize) {
    let scratch = env.root().join("scratch");
    if !scratch.exists() {
        env.git(env.root(), &["clone", "-q", remote.to_str().unwrap(), scratch.to_str().unwrap()]);
    }
    let count = env.root().join("moves");
    let hook = dir(env, CTX).join(".git/hooks/pre-push");
    fs::write(
        &hook,
        format!(
            "#!/bin/sh\nunset GIT_DIR GIT_INDEX_FILE GIT_WORK_TREE\nn=$(cat '{count}' 2>/dev/null || echo 0)\n\
             [ \"$n\" -ge {times} ] && exit 0\necho $((n + 1)) > '{count}'\n\
             git -C '{scratch}' pull -q origin main\n\
             git -C '{scratch}' commit -q --allow-empty -m moved\n\
             git -C '{scratch}' push -q origin main\n",
            count = count.display(),
            scratch = scratch.display(),
        ),
    )
    .unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn sync_repeats_when_the_remote_moved_and_gives_up_after_three_rounds() {
    let env = hs();
    let remote = bare(&env, "remote.git");
    new_context(&env);
    add_origin(&env, CTX, &remote);
    env.cpd(&["context", "sync"]).ok();
    env.cpd(&["context", "note", "Local.", "--context", CTX]).ok();

    // Moved once: the second round succeeds.
    move_remote_on_push(&env, &remote, 1);
    let out = env.cpd(&["context", "sync", "--context", CTX]).ok();
    assert_eq!(out["contexts"][0]["pushed"], true, "{out}");
    assert_eq!(env.git(&remote, &["rev-parse", "main"]), env.git(&dir(&env, CTX), &["rev-parse", "HEAD"]));
    assert_eq!(fs::read_to_string(env.root().join("moves")).unwrap().trim(), "1");

    // Moved every time: three rounds, then context_sync_failed.
    env.cpd(&["context", "note", "Local 2.", "--context", CTX]).ok();
    fs::remove_file(env.root().join("moves")).unwrap();
    move_remote_on_push(&env, &remote, 99);
    let error = refused(env.cpd(&["context", "sync", "--context", CTX]), 1, "context_sync_failed");
    assert!(error["details"]["argv"].as_array().unwrap().iter().any(|a| a == "push"), "{error}");
    assert_eq!(fs::read_to_string(env.root().join("moves")).unwrap().trim(), "3");
}

#[test]
fn sync_of_every_context_reports_each_and_fails_with_the_first_failure() {
    let env = hs();
    new_context(&env);
    env.cpd(&["context", "new", "home-lab", "--title", "Home lab"]).ok();
    add_origin(&env, "home-lab", &env.root().join("no-such-remote.git"));
    let remote = bare(&env, "remote.git");
    env.cpd(&["context", "new", "zeta", "--title", "Zeta"]).ok();
    add_origin(&env, "zeta", &remote);

    let run = env.cpd(&["context", "sync"]);
    let error = refused(run, 1, "context_sync_failed");
    let entries = error["details"]["contexts"].as_array().unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0], json!({"id": CTX, "synced": false, "reason": "no_remote", "imported": 0, "pushed": false}));
    assert_eq!(entries[1]["id"], "home-lab");
    assert_eq!(entries[1]["error"]["code"], "context_sync_failed");
    assert!(entries[1]["error"]["message"].is_string());
    // The others are still synced.
    assert_eq!(entries[2], json!({"id": "zeta", "synced": true, "imported": 0, "pushed": true}));
    assert_eq!(env.git(&remote, &["log", "--format=%s", "main"]), "context zeta: Zeta");
}

#[test]
fn a_sync_failure_from_the_engine_is_recorded_once() {
    let env = hs();
    new_context(&env);
    let remote = env.root().join("later.git");
    add_origin(&env, CTX, &remote);
    let (home, config, mut store) = library(&env);
    let id = ContextId::parse(CTX).unwrap();

    for _ in 0..2 {
        let error = Contexts::new(&home, &config, &mut store).sync(&id, Trigger::Engine).unwrap_err();
        assert_eq!(error.code().as_str(), "context_sync_failed");
    }
    let errors = events(&env, "error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0]["context"], CTX);
    assert_eq!(errors[0]["data"]["source"], "context sync");
    assert_eq!(errors[0]["data"]["code"], "context_sync_failed");
    assert!(errors[0]["data"]["message"].is_string() && errors[0]["data"]["details"].is_object());
    let listed = env.cpd(&["context", "list"]).ok();
    assert_eq!(listed["contexts"][0]["sync_failing"], true);

    // Run by hand, a failure only reports; and nothing is recorded again.
    assert_eq!(code(&env.cpd(&["context", "sync"])), "context_sync_failed");
    assert_eq!(events(&env, "error").len(), 1);

    // A success clears it; the next failure is reported again.
    bare_at(&env, &remote);
    let out = Contexts::new(&home, &config, &mut store).sync(&id, Trigger::Engine).unwrap();
    assert!(out.synced && out.pushed);
    assert_eq!(env.cpd(&["context", "list"]).ok()["contexts"][0]["sync_failing"], false);
    fs::remove_dir_all(&remote).unwrap();
    Contexts::new(&home, &config, &mut store).sync(&id, Trigger::Engine).unwrap_err();
    assert_eq!(events(&env, "error").len(), 2);
}

fn bare_at(env: &TestEnv, path: &Path) {
    env.git(env.root(), &["init", "-q", "--bare", "-b", "main", path.to_str().unwrap()]);
}

// ---- context rename -------------------------------------------------------

fn rename_events(env: &TestEnv) -> Vec<Value> {
    env.cpd(&["events", "list", "--kind", "context-renamed"]).ok()["events"].as_array().unwrap().clone()
}

#[test]
fn rename_sets_the_title_and_appends_one_event() {
    let env = hs();
    new_context(&env);
    let out = env.cpd(&["context", "rename", CTX, "--title", "Building cpd 2"]).ok();
    assert_eq!(out["context"], json!({"id": CTX, "title": "Building cpd 2"}));
    assert!(out.get("already").is_none());
    assert_eq!(env.cpd(&["context", "list"]).ok()["contexts"][0]["title"], "Building cpd 2");
    let events = rename_events(&env);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["context"], CTX);
    assert_eq!(events[0]["data"], json!({"title": "Building cpd 2"}));
    // The git store is not touched.
    let stored: Value = serde_json::from_str(&fs::read_to_string(dir(&env, CTX).join("context.json")).unwrap()).unwrap();
    assert_eq!(stored["title"], "Building cpd");
}

#[test]
fn rename_to_the_same_title_is_already_and_writes_nothing() {
    let env = hs();
    new_context(&env);
    assert_eq!(env.cpd(&["context", "rename", CTX, "--title", "Building cpd"]).ok()["already"], true);
    assert!(rename_events(&env).is_empty());
}

#[test]
fn rename_refusals() {
    let env = hs();
    new_context(&env);
    refused(env.cpd(&["context", "rename", "nope", "--title", "X"]), 4, "context_not_found");
    refused(env.cpd(&["context", "rename", CTX, "--title", ""]), 2, "usage");
    assert!(rename_events(&env).is_empty());
}

// ---- context archive and unarchive ----------------------------------------

use common::SLEEP_SECONDS;

/// `hs()` on its private tmux server.
fn hs_tmux() -> TestEnv {
    let env = hs();
    let config = format!("version = 1\nmachine = \"hs\"\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket());
    fs::write(env.cpd_home().join("config.toml"), config).unwrap();
    env
}

fn sql(env: &TestEnv, sql: &str) {
    let (_, _, mut store) = library(env);
    store.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok(())
    })
    .unwrap();
}

fn count(env: &TestEnv, sql: &str) -> i64 {
    let (_, _, mut store) = library(env);
    store.read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
}

/// The cockpit: session `cpd`, window `main` with a tree pane.
fn cockpit(env: &TestEnv) {
    let out = env.tmux(&["new-session", "-d", "-s", "cpd", "-n", "main", "-P", "-F", "#{pane_id}", &format!("sleep {SLEEP_SECONDS}")]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let tree = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    assert!(env.tmux(&["set", "-p", "-t", &tree, "@cpd_view", "tree"]).status.success());
}

/// Gives Session `id` a live parked pane in the cockpit, as `@cpd_session`.
fn pane(env: &TestEnv, id: &str) -> String {
    let started =
        env.tmux(&["new-window", "-d", "-t", "=cpd:", "-n", &format!("park-{id}"), "-P", "-F", "#{pane_id}", &format!("sleep {SLEEP_SECONDS}")]);
    assert!(started.status.success(), "{}", String::from_utf8_lossy(&started.stderr));
    let pane = String::from_utf8(started.stdout).unwrap().trim().to_owned();
    assert!(env.tmux(&["set", "-p", "-t", &pane, "@cpd_session", id]).status.success());
    sql(env, &format!("UPDATE sessions SET tmux_pane = '{pane}' WHERE id = '{id}'"));
    pane
}

fn live_panes(env: &TestEnv) -> Vec<String> {
    let out = env.tmux(&["list-panes", "-a", "-F", "#{pane_id}"]);
    String::from_utf8(out.stdout).unwrap().lines().map(str::to_owned).collect()
}

fn archived_ats(env: &TestEnv) -> i64 {
    count(
        env,
        "SELECT count(*) FROM (SELECT archived_at FROM contexts UNION ALL SELECT archived_at FROM plans
                              UNION ALL SELECT archived_at FROM sessions) WHERE archived_at IS NOT NULL",
    )
}

fn event_count(env: &TestEnv) -> i64 {
    count(env, "SELECT count(*) FROM events")
}

/// Context `building-cpd` with a grounded Lead, Plans `pay` and `ship` (each with a Lead),
/// a side fork of `pay`, and a Worker of `pay`'s Job `api`; another Context `other` with Plan
/// `side`. Live parked panes: `lead-pay`, `fork-pay` and `grounded`.
fn archivable(env: &TestEnv) {
    new_context(env);
    env.cpd(&["context", "new", "other", "--title", "Other"]).ok();
    fixture_plan(env, "pay", CTX);
    fixture_plan(env, "ship", CTX);
    fixture_plan(env, "side", "other");
    sql(
        env,
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, forked_from)
           VALUES ('fork-pay', 'lead', 'building-cpd', 'pay', 'Fork', '/', 'O3', 'stopped', 't', 't1', 'lead-pay'),
                  ('grounded', 'lead', 'building-cpd', NULL, 'Grounded', '/', 'O3', 'idle', 't', 't0', NULL);
         UPDATE contexts SET lead_session = 'grounded', grounded_seq = 0 WHERE id = 'building-cpd';
         INSERT INTO jobs (plan, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
           VALUES ('pay', 'api', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, 1, 't');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
           VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', 'cpd/pay--api-a1', '/w', 'worker1', 't');
         INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
           VALUES ('worker1', 'worker', 'building-cpd', 'pay', 'api-a1', 'Worker', '/', 'S2', 'idle', 't', 't2');",
    );
    cockpit(env);
    for id in ["lead-pay", "fork-pay", "grounded"] {
        pane(env, id);
    }
}

fn ids(out: &Value, kind: &str) -> Vec<String> {
    let mut ids: Vec<String> =
        out["archived"].as_array().unwrap().iter().filter(|o| o["kind"] == kind).map(|o| o["id"].as_str().unwrap().to_owned()).collect();
    ids.sort();
    ids
}

#[test]
fn archive_covers_plans_leads_forks_and_the_grounded_lead_with_one_event_each() {
    let env = hs_tmux();
    archivable(&env);
    let panes = live_panes(&env);
    let out = env.cpd(&["context", "archive", CTX]).ok();
    assert_eq!(out["archived"][0], json!({"kind": "context", "id": CTX}));
    assert_eq!(ids(&out, "context"), [CTX]);
    assert_eq!(ids(&out, "plan"), ["pay", "ship"]);
    assert_eq!(ids(&out, "session"), ["fork-pay", "grounded", "lead-pay", "lead-ship"]);
    assert_eq!(out["archived"].as_array().unwrap().len(), 7);
    assert_eq!(out["closed"].as_array().unwrap().len(), 3);
    // Every pane the cascade closed is gone; the cockpit's other panes are not ours to count.
    let left = live_panes(&env);
    assert!(out["closed"].as_array().unwrap().iter().all(|p| panes.contains(&p.as_str().unwrap().to_owned()) && !left.contains(&p.as_str().unwrap().to_owned())));

    assert_eq!(events(&env, "context-archived").len(), 1);
    assert_eq!(events(&env, "plan-archived").len(), 2);
    assert_eq!(events(&env, "session-archived").len(), 4);
    assert!(events(&env, "context-archived")[0]["data"] == json!({"archived": true}));
    assert_eq!(archived_ats(&env), 7);
    // The Worker's Session and the other Context are not touched.
    assert_eq!(count(&env, "SELECT count(*) FROM sessions WHERE id = 'worker1' AND archived_at IS NULL"), 1);
    assert_eq!(count(&env, "SELECT count(*) FROM plans WHERE id = 'side' AND archived_at IS NULL"), 1);
    assert_eq!(count(&env, "SELECT count(*) FROM sessions WHERE tmux_pane IS NOT NULL"), 0);

    let list = env.cpd(&["context", "list"]).ok();
    let context = |id: &str| list["contexts"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap().clone();
    assert!(context(CTX)["archived_at"].is_string());
    assert!(context(CTX)["plans"].as_array().unwrap().iter().all(|p| p["archived_at"].is_string()));
    assert_eq!(context("other")["archived_at"], Value::Null);
    assert_eq!(context("other")["plans"][0]["archived_at"], Value::Null);

    let events_before = event_count(&env);
    let again = env.cpd(&["context", "archive", CTX]).ok();
    assert_eq!(again, json!({"ok": true, "archived": [], "closed": [], "already": true}));
    assert_eq!(event_count(&env), events_before);
}

#[test]
fn archive_refused_by_any_one_blocker_changes_nothing() {
    let fingerprint = "a".repeat(64);
    let blockers = [
        ("job", format!("UPDATE jobs SET status = 'queued', fingerprint = '{fingerprint}' WHERE key = 'api'")),
        (
            "escalation",
            "INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
             VALUES ('pay', 'api-a1-q1', 'api-a1', 1, 'Which?', 'open', 1, 't')"
                .to_owned(),
        ),
        (
            "review",
            format!("INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at) VALUES ('ship', 'r1', 1, '{fingerprint}', 'pending', 't')"),
        ),
        ("session", "UPDATE sessions SET state = 'working' WHERE id = 'fork-pay'".to_owned()),
    ];
    for (kind, apply) in blockers {
        let env = hs_tmux();
        archivable(&env);
        sql(&env, &apply);
        let (events_before, panes_before) = (event_count(&env), live_panes(&env));
        let error = refused(env.cpd(&["context", "archive", CTX]), 3, "archive_blocked");
        let listed = error["details"]["blockers"].as_array().unwrap();
        assert_eq!(listed.len(), 1, "{kind}: {error}");
        assert_eq!(listed[0]["kind"], kind, "{error}");
        assert_eq!(archived_ats(&env), 0, "{kind}");
        assert_eq!(event_count(&env), events_before, "{kind}");
        assert_eq!(live_panes(&env), panes_before, "{kind}");
        assert_eq!(count(&env, "SELECT count(*) FROM sessions WHERE tmux_pane IS NOT NULL"), 3, "{kind}");
    }
}

#[test]
fn archive_refuses_a_lead_in_the_chat_slot_before_closing_any_pane() {
    let env = hs_tmux();
    archivable(&env);
    // The chat slot: the pane of window `main` running the grounded Lead's pane.
    let grounded = env.tmux(&["list-panes", "-a", "-F", "#{pane_id}\t#{@cpd_session}"]);
    let grounded = String::from_utf8(grounded.stdout).unwrap();
    let grounded = grounded.lines().find_map(|l| l.strip_suffix("\tgrounded")).unwrap().to_owned();
    assert!(env.tmux(&["rename-window", "-t", &grounded, "main"]).status.success());
    let (events_before, panes_before) = (event_count(&env), live_panes(&env));
    let error = refused(env.cpd(&["context", "archive", CTX]), 3, "in_chat_slot");
    assert_eq!(error["details"]["next"], "cpd session park");
    assert_eq!((event_count(&env), live_panes(&env), archived_ats(&env)), (events_before, panes_before, 0));
}

#[test]
fn unarchive_clears_the_cascade_with_one_event_each_and_touches_no_pane() {
    let env = hs_tmux();
    archivable(&env);
    env.cpd(&["context", "archive", CTX]).ok();
    let (events_before, panes_before) = (event_count(&env), live_panes(&env));

    let out = env.cpd(&["context", "unarchive", CTX]).ok();
    assert_eq!(out["archived"][0], json!({"kind": "context", "id": CTX}));
    assert_eq!(ids(&out, "plan"), ["pay", "ship"]);
    assert_eq!(ids(&out, "session"), ["fork-pay", "grounded", "lead-pay", "lead-ship"]);
    assert_eq!(out["closed"], json!([]));
    assert_eq!(archived_ats(&env), 0);
    assert_eq!(event_count(&env), events_before + 7);
    assert_eq!(live_panes(&env), panes_before);
    assert_eq!(events(&env, "context-archived")[1]["data"], json!({"archived": false}));
    assert_eq!(env.cpd(&["context", "list"]).ok()["contexts"][0]["archived_at"], Value::Null);

    let again = env.cpd(&["context", "unarchive", CTX]).ok();
    assert_eq!(again, json!({"ok": true, "archived": [], "closed": [], "already": true}));
    assert_eq!(event_count(&env), events_before + 7);
}

#[test]
fn archive_and_unarchive_of_an_unknown_context_are_refused() {
    let env = hs_tmux();
    refused(env.cpd(&["context", "archive", "nope"]), 4, "context_not_found");
    refused(env.cpd(&["context", "unarchive", "nope"]), 4, "context_not_found");
}
