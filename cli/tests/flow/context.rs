//! Contexts (contract §2.2, §4.6; commands.md §3): every command run as the
//! built binary. The records live only in the database: no directory, no git,
//! no lock file.

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::{Run, TestEnv};
use cpd_core::config::Config;
use cpd_core::context::{By, Draft, Record, digest, inforce};
use cpd_core::home::CpdHome;
use cpd_core::ids::{RecordId, RecordKind};
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

/// `$CPD_HOME/contexts`: where the git stores of earlier versions lived.
fn old_dir(env: &TestEnv) -> PathBuf {
    env.cpd_home().join("contexts")
}

fn new_context(env: &TestEnv) {
    env.cpd(&["context", "new", CTX, "--title", "Building cpd", "--owner", "acme"]).ok();
}

/// [`common::refused`], and nothing on stderr.
fn refused(run: Run, exit: i32, expected: &str) -> Value {
    let error = common::refused(&run, expected, exit);
    assert_eq!(run.stderr, "", "{run:?}");
    error
}

fn events(env: &TestEnv, kind: &str) -> Vec<Value> {
    let run = env.cpd(&["events", "list", "--kind", kind]);
    run.ok()["events"].as_array().unwrap().clone()
}

/// The ids of the Context's records, in index order.
fn record_ids(env: &TestEnv) -> Vec<String> {
    let shown = env.cpd(&["context", "show", "--context", CTX]).ok();
    shown["records"].as_array().unwrap().iter().map(|r| r["id"].as_str().unwrap().to_owned()).collect()
}

fn library(env: &TestEnv) -> (CpdHome, Config, Store) {
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    let config = Config::load(&home).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    (home, config, store)
}

fn fixture_plan(env: &TestEnv, plan: &str, context: &str) {
    let (_, _, mut store) = library(env);
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead-{plan}', 'lead', '{context}', '{plan}', 'Lead', '/', 'O3', 'idle', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at)
                   VALUES ('{plan}', '{context}', 'The {plan} plan', 'grounding', 'lead-{plan}', 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                   VALUES ('{plan}', 'r', '/r', 'main', 'waiting', 'cpd/{plan}', cpd_txn(), 't');"
            ))?;
            Ok(())
        })
        .unwrap();
}

// ---- context new ----------------------------------------------------------

#[test]
fn new_writes_the_row_and_the_event_and_nothing_else() {
    let env = hs();
    let out = env.cpd(&["context", "new", CTX, "--title", "Building cpd", "--owner", "acme", "--jira", "CPD-12"]).ok();
    let context = &out["context"];
    assert_eq!(context["id"], CTX);
    assert_eq!(context["title"], "Building cpd");
    assert_eq!(context["jira"], "CPD-12");
    assert_eq!(context["created_on"], "hs");
    assert!(context["created_at"].as_str().unwrap().ends_with('Z'));
    assert!(context.get("dir").is_none(), "{context}");
    assert!(out.get("already").is_none());
    assert!(!old_dir(&env).exists(), "no contexts/ directory");

    let created = events(&env, "context-created");
    assert_eq!(created.len(), 1);
    assert_eq!(created[0]["context"], CTX);
    assert_eq!(created[0]["data"], json!({"title": "Building cpd", "jira": "CPD-12", "created_on": "hs"}));
    assert_eq!(env.cpd(&["context", "list"]).ok()["contexts"][0]["id"], CTX);
}

#[test]
fn new_is_idempotent_for_the_same_title_and_jira() {
    let env = hs();
    env.cpd(&["context", "new", CTX, "--title", "Building cpd", "--owner", "acme"]).ok();
    let again = env.cpd(&["context", "new", CTX, "--title", "Building cpd", "--owner", "acme"]).ok();
    assert_eq!(again["already"], true);
    assert_eq!(again["context"]["id"], CTX);
    assert_eq!(events(&env, "context-created").len(), 1);
}

#[test]
fn new_refuses_an_existing_context_with_another_title_or_jira() {
    let env = hs();
    new_context(&env);
    for args in [
        vec!["context", "new", CTX, "--title", "Another", "--owner", "acme"],
        vec!["context", "new", CTX, "--title", "Building cpd", "--owner", "acme", "--jira", "CPD-1"],
    ] {
        refused(env.cpd(&args), 3, "context_exists");
    }
}

#[test]
fn new_checks_its_arguments() {
    let env = hs();
    refused(env.cpd(&["context", "new", "Bad_Id", "--title", "T", "--owner", "acme"]), 2, "invalid_context_id");
    refused(env.cpd(&["context", "new", CTX, "--title", "T", "--owner", "acme", "--jira", "cpd-1"]), 2, "usage");
    refused(env.cpd(&["context", "new", CTX, "--title", "T", "--owner", "acme", "--jira", "C-1"]), 2, "usage");
    refused(env.cpd(&["context", "new", CTX, "--title", "", "--owner", "acme"]), 2, "usage");
    refused(env.cpd(&["context", "new", CTX]), 2, "usage");
    assert_eq!(env.cpd(&["context", "list"]).ok()["contexts"], json!([]));
    let no_config = TestEnv::new();
    refused(no_config.cpd(&["context", "new", CTX, "--title", "T", "--owner", "acme"]), 1, "config_missing");
}

#[test]
fn new_requires_an_owner_with_a_policy_file_and_stores_it_lowercased() {
    let env = hs();
    // No `--owner`: a clap usage error.
    refused(env.cpd(&["context", "new", CTX, "--title", "T"]), 2, "usage");
    refused(env.cpd(&["context", "new", CTX, "--title", "T", "--owner", "-bad-"]), 2, "usage");
    // An owner without `policy/owners/<login>.toml`.
    let error = refused(env.cpd(&["context", "new", CTX, "--title", "T", "--owner", "nobody"]), 1, "owner_unknown");
    assert_eq!(error["details"]["owner"], "nobody");
    assert_eq!(env.cpd(&["context", "list"]).ok()["contexts"], json!([]), "nothing written");
    // The login is lowercased before the file is looked up.
    let out = env.cpd(&["context", "new", CTX, "--title", "T", "--owner", "ACME"]).ok();
    assert_eq!(out["context"]["owner"], "acme");
    // The same Context with another owner is `context_exists`.
    fs::write(env.cpd_home().join("policy/owners/other.toml"), "").unwrap();
    refused(env.cpd(&["context", "new", CTX, "--title", "T", "--owner", "other"]), 3, "context_exists");
}

/// Makes a Context that has no owner, as one made before the policy keys.
fn clear_owner(env: &TestEnv) {
    let (_, _, mut store) = library(env);
    store.write("test", |tx| Ok(tx.execute("UPDATE contexts SET owner = NULL", []).map(|_| ())?)).unwrap();
}

#[test]
fn owner_sets_the_owner_once_and_appends_context_owner() {
    let env = hs();
    new_context(&env);
    clear_owner(&env);
    fs::write(env.cpd_home().join("policy/owners/other.toml"), "").unwrap();

    let error = refused(env.cpd(&["context", "owner", "nobody", "--context", CTX]), 1, "owner_unknown");
    assert_eq!(error["details"]["owner"], "nobody");
    assert_eq!(events(&env, "context-owner").len(), 0);
    refused(env.cpd(&["context", "owner", "acme", "--context", "no-such"]), 4, "context_not_found");
    refused(env.cpd(&["context", "owner", "acme"]), 2, "context_required");

    let out = env.cpd(&["context", "owner", "ACME", "--context", CTX]).ok();
    assert_eq!(out["context"], json!({"id": CTX, "owner": "acme"}));
    assert!(out.get("already").is_none());
    let owner = events(&env, "context-owner");
    assert_eq!(owner.len(), 1);
    assert_eq!((owner[0]["context"].clone(), owner[0]["data"].clone()), (json!(CTX), json!({"owner": "acme"})));

    // The same owner again writes nothing; another owner is `owner_set`.
    let again = env.cpd(&["context", "owner", "acme", "--context", CTX]).ok();
    assert_eq!(again["already"], true);
    assert_eq!(events(&env, "context-owner").len(), 1);
    let error = refused(env.cpd(&["context", "owner", "other", "--context", CTX]), 3, "owner_set");
    assert_eq!(error["details"]["context"], CTX);
    assert_eq!(error["details"]["owner"], "acme");
    assert_eq!(events(&env, "context-owner").len(), 1);
}

#[test]
fn sync_is_gone() {
    let env = hs();
    new_context(&env);
    refused(env.cpd(&["context", "sync"]), 2, "usage");
    refused(env.cpd(&["context", "sync", "--context", CTX]), 2, "usage");
}

#[test]
fn no_record_write_runs_git_or_makes_a_directory() {
    let env = hs();
    env.fake("git", "echo 'no git here' >&2; exit 99");
    new_context(&env);
    env.cpd(&["context", "decide", "D.", "--context", CTX]).ok();
    env.cpd(&["context", "ask", "Q?", "--context", CTX]).ok();
    env.cpd(&["context", "answer", "hs-q1", "A.", "--context", CTX]).ok();
    env.cpd(&["context", "note", "N.", "--context", CTX]).ok();
    env.cpd(&["context", "rename", CTX, "--title", "Renamed"]).ok();
    assert_eq!(record_ids(&env), ["hs-d1", "hs-q1", "hs-a1", "hs-n1"]);
    assert_eq!(env.calls("git"), Vec::<Vec<String>>::new());
    assert!(!old_dir(&env).exists(), "no contexts/ directory");
    assert!(!env.cpd_home().join("locks").join(format!("context-{CTX}.lock")).exists(), "no lock file");
}

// ---- context decide, note, ask --------------------------------------------

#[test]
fn decide_note_and_ask_write_records() {
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
    assert_eq!(record_ids(&env), ["hs-d1", "hs-n1", "hs-q1", "hs-d2"]);

    // One context-record Event each, in the write's own transaction.
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
    let shown = env.cpd(&["context", "show", "--context", CTX]).ok();
    assert_eq!(shown["records"][1]["by"], json!({"session": "pi-session", "plan": "cpd2"}));
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
fn decide_refuses_a_missing_supersedes() {
    let env = hs();
    new_context(&env);
    env.cpd(&["context", "note", "A note.", "--context", CTX]).ok();
    for target in ["hs-d9", "hs-n9"] {
        let run = env.cpd(&["context", "decide", "New.", "--supersedes", target, "--context", CTX]);
        refused(run, 3, "record_not_found");
    }
    refused(env.cpd(&["context", "decide", "New.", "--supersedes", "nonsense", "--context", CTX]), 2, "invalid_record_id");
    // Nothing was written by the refusals.
    assert_eq!(record_ids(&env), ["hs-n1"]);
    assert_eq!(events(&env, "context-record").len(), 1);
}

// ---- notes in supersession (contract §2.2, §4.6 "A write"; commands.md §3.4) ----------

/// The line of `md` that starts with `- <id> (`.
fn item_line<'a>(md: &'a str, id: &str) -> &'a str {
    md.lines().find(|l| l.starts_with(&format!("- {id} ("))).unwrap_or_else(|| panic!("{id} in {md}"))
}

#[test]
fn a_note_supersedes_an_older_note_which_leaves_force_and_is_labelled_in_show() {
    let env = hs();
    new_context(&env);
    env.cpd(&["context", "note", "Old fact.", "--context", CTX]).ok();
    let n2 = env.cpd(&["context", "note", "New fact.", "--supersedes", "hs-n1", "--context", CTX]).ok();
    assert_eq!(n2["record"]["id"], "hs-n2", "{n2}");
    assert_eq!(n2["record"]["kind"], "note");
    assert_eq!(n2["record"]["supersedes"], "hs-n1");
    assert_eq!(n2["seq"], 2);
    assert_eq!(events(&env, "context-record").len(), 2);

    // JSON: both records stay, the successor names what it replaced.
    let shown = env.cpd(&["context", "show", "--context", CTX]).ok();
    let records = shown["records"].as_array().unwrap();
    assert_eq!(records.len(), 2, "{shown}");
    assert!(records[0].get("supersedes").is_none());
    assert_eq!(records[1]["supersedes"], "hs-n1");

    // Markdown: the older note is listed, labelled superseded and naming its successor, after the active one.
    let md = shown["markdown"].as_str().unwrap();
    let old = item_line(md, "hs-n1");
    assert!(old.contains("Old fact.") && old.contains("superseded by hs-n2"), "{md}");
    let new = item_line(md, "hs-n2");
    assert!(new.contains("New fact.") && !new.contains("superseded by"), "{md}");
    let (at_notes, at_old, at_new) = (md.find("## Notes").expect("## Notes"), md.find(old).unwrap(), md.find(new).unwrap());
    assert!(at_notes < at_new && at_new < at_old, "the active note comes before the superseded one: {md}");

    // The superseded note stays readable.
    let one = env.cpd(&["context", "show", "--record", "hs-n1", "--context", CTX]).ok();
    assert_eq!(one["records"][0]["text"], "Old fact.", "{one}");
}

#[test]
fn a_decision_supersedes_a_note_which_is_then_labelled_superseded() {
    let env = hs();
    new_context(&env);
    env.cpd(&["context", "note", "A fact.", "--context", CTX]).ok();
    let d1 = env.cpd(&["context", "decide", "Decided otherwise.", "--supersedes", "hs-n1", "--context", CTX]).ok();
    assert_eq!(d1["record"]["id"], "hs-d1", "{d1}");
    assert_eq!(d1["record"]["supersedes"], "hs-n1");
    let shown = env.cpd(&["context", "show", "--context", CTX]).ok();
    assert_eq!(shown["records"][1]["supersedes"], "hs-n1");
    let md = shown["markdown"].as_str().unwrap();
    let old = item_line(md, "hs-n1");
    assert!(old.contains("A fact.") && old.contains("superseded by hs-d1"), "{md}");
    assert!(!item_line(md, "hs-d1").contains("superseded by"), "{md}");
}

#[test]
fn a_note_naming_a_decision_is_supersedes_invalid_and_writes_nothing() {
    let env = hs();
    new_context(&env);
    env.cpd(&["context", "decide", "A decision.", "--context", CTX]).ok();
    let run = env.cpd(&["context", "note", "A fact.", "--supersedes", "hs-d1", "--context", CTX]);
    let error = refused(run, 3, "supersedes_invalid");
    assert_eq!(error["details"]["record"], "hs-d1", "{error}");
    assert_eq!(error["details"]["kind"], "decision", "{error}");
    assert_eq!(error["details"]["allowed"], json!(["note"]), "{error}");
    assert_eq!(record_ids(&env), ["hs-d1"]);
    assert_eq!(events(&env, "context-record").len(), 1);
}

#[test]
fn a_record_of_another_kind_cannot_be_superseded_by_a_decision_or_a_note() {
    let env = hs();
    new_context(&env);
    env.cpd(&["context", "ask", "Why?", "--context", CTX]).ok();
    env.cpd(&["context", "answer", "hs-q1", "Because.", "--context", CTX]).ok();
    for (target, kind) in [("hs-q1", "question"), ("hs-a1", "answer")] {
        let run = env.cpd(&["context", "decide", "New.", "--supersedes", target, "--context", CTX]);
        let error = refused(run, 3, "supersedes_invalid");
        assert_eq!(error["details"]["record"], target, "{error}");
        assert_eq!(error["details"]["kind"], kind, "{error}");
        assert_eq!(error["details"]["allowed"], json!(["decision", "note"]), "{error}");

        let run = env.cpd(&["context", "note", "New.", "--supersedes", target, "--context", CTX]);
        let error = refused(run, 3, "supersedes_invalid");
        assert_eq!(error["details"]["record"], target, "{error}");
        assert_eq!(error["details"]["kind"], kind, "{error}");
        assert_eq!(error["details"]["allowed"], json!(["note"]), "{error}");
    }
    assert_eq!(record_ids(&env), ["hs-q1", "hs-a1"]);
    assert_eq!(events(&env, "context-record").len(), 2);
}

#[test]
fn a_note_superseding_an_unknown_record_is_record_not_found() {
    let env = hs();
    new_context(&env);
    env.cpd(&["context", "new", "other-ctx", "--title", "Other", "--owner", "acme"]).ok();
    env.cpd(&["context", "note", "In the other Context.", "--context", "other-ctx"]).ok();
    env.cpd(&["context", "note", "Also in the other Context.", "--context", "other-ctx"]).ok();
    env.cpd(&["context", "note", "Here.", "--context", CTX]).ok();
    // hs-n9 exists nowhere; hs-n2 is a note of the other Context only, so it is not in this one.
    for target in ["hs-n9", "hs-n2"] {
        let run = env.cpd(&["context", "note", "New.", "--supersedes", target, "--context", CTX]);
        refused(run, 3, "record_not_found");
    }
    refused(env.cpd(&["context", "note", "New.", "--supersedes", "nonsense", "--context", CTX]), 2, "invalid_record_id");
    assert_eq!(record_ids(&env), ["hs-n1"]);
    assert_eq!(events(&env, "context-record").len(), 3);
}

/// `S2` is the distiller's Tier: a fake `pi` whose digest cites every record of `records.md`.
fn distilling(env: &TestEnv) {
    fs::write(
        env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"hs\"\nlead_pi_packages = []\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n",
    )
    .unwrap();
    env.fake_pi(
        r##"digest() {
  echo "# Digest"
  while IFS= read -r line; do
    case "$line" in "["*) echo "- $line" ;; esac
  done < records.md
}
text=$(digest | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' -e 's/$/\\n/' | tr -d '\n')
printf '%s\n' '{"type":"session","version":3,"id":"x","timestamp":"2026-10-19T01:59:25.851Z","cwd":"/"}' '{"type":"agent_start"}'
printf '{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"%s"}],"usage":{"input":2,"output":4,"cacheRead":0,"cacheWrite":0,"totalTokens":6,"cost":{"total":0.1}},"stopReason":"stop"}}\n' "$text"
printf '%s\n' '{"type":"agent_settled","aborted":false}'"##,
    );
}

#[test]
fn a_superseded_note_is_absent_from_the_records_a_distill_run_reads() {
    let env = hs();
    new_context(&env);
    distilling(&env);
    env.cpd(&["context", "decide", "Keep this decision.", "--context", CTX]).ok();
    env.cpd(&["context", "note", "Stale fact one.", "--context", CTX]).ok();
    env.cpd(&["context", "note", "Stale fact two.", "--context", CTX]).ok();
    env.cpd(&["context", "note", "Fresh fact.", "--supersedes", "hs-n1", "--context", CTX]).ok();
    env.cpd(&["context", "decide", "Decided over a fact.", "--supersedes", "hs-n2", "--context", CTX]).ok();

    let out = env.cpd(&["context", "distill", "--context", CTX]).ok();
    // In force: hs-d1, hs-n3, hs-d2.
    assert_eq!((out["distilled"].clone(), out["records_in_force"].clone()), (json!(true), json!(3)), "{out}");
    let dir = out["digest"]["file"].as_str().map(|f| PathBuf::from(f).parent().unwrap().to_owned()).expect("a digest file");
    let records = fs::read_to_string(dir.join("records.md")).unwrap();
    for gone in ["[hs-n1]", "[hs-n2]", "Stale fact"] {
        assert!(!records.contains(gone), "{gone} is out of force: {records}");
    }
    for kept in ["[hs-d1]", "[hs-n3]", "[hs-d2]"] {
        assert!(records.contains(kept), "{kept} is in force: {records}");
    }
    // No digest cites a record out of force.
    let digest = fs::read_to_string(out["digest"]["file"].as_str().unwrap()).unwrap();
    assert!(!digest.contains("hs-n1") && !digest.contains("hs-n2"), "{digest}");
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
    assert_eq!(record_ids(&env), ["hs-n1"]);
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
fn a_record_write_is_one_transaction_with_its_event() {
    let env = hs();
    new_context(&env);
    env.cpd(&["context", "note", "One.", "--context", CTX]).ok();
    let (_, _, mut store) = library(&env);
    let (txns, rows) = store
        .read(|conn| {
            let q = |sql: &str| conn.query_row(sql, [], |r| r.get::<_, i64>(0));
            Ok((
                q("SELECT count(DISTINCT txn) FROM events WHERE kind = 'context-record'")?,
                q("SELECT count(*) FROM history h JOIN events e ON e.txn = h.txn
                    WHERE e.kind = 'context-record' AND h.tbl = 'context_records' AND h.op = 'insert'")?,
            ))
        })
        .unwrap();
    assert_eq!((txns, rows), (1, 1));
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

    // The same answer again is a retry; another answer is refused.
    let again = env.cpd(&["context", "answer", "hs-q1", "Yes, per machine.", "--context", CTX]).ok();
    assert_eq!(again["already"], true);
    assert_eq!(again["record"]["id"], "hs-a1");
    let error = refused(env.cpd(&["context", "answer", "hs-q1", "No.", "--context", CTX]), 3, "question_answered");
    assert_eq!(error["details"]["answer"], "hs-a1");
    assert_eq!(record_ids(&env), ["hs-q1", "hs-a1"]);
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
    assert_eq!(record_ids(&env), ["hs-n1"]);
}

// ---- context list and show ------------------------------------------------

#[test]
fn list_summarises_every_context() {
    let env = hs();
    assert_eq!(env.cpd(&["context", "list"]).ok()["contexts"], json!([]));
    new_context(&env);
    env.cpd(&["context", "new", "home-lab", "--title", "Home lab", "--owner", "acme", "--jira", "LAB-3"]).ok();
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
        json!({"id": CTX, "title": "Building cpd", "jira": null, "archived_at": null, "records": 4, "open_questions": 1,
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
    assert_eq!(out["context"], json!({"id": CTX, "title": "Building cpd", "jira": null}));
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
fn show_lists_the_repos_of_each_plans_prs_from_the_database() {
    let env = hs();
    new_context(&env);
    // A `plan` record has no repo (§2.2); `plan.machine` says whose database holds the Plan.
    let (_, _, mut store) = library(&env);
    store
        .write("fixture", |tx| {
            for (n, plan, machine) in [(1, "old", "hs"), (2, "two", "hs"), (3, "none", "hs"), (4, "far", "zz")] {
                tx.execute(
                    "INSERT INTO context_records (context, id, kind, at, machine, text, plan_id, plan_machine)
                     VALUES (?1, ?2, 'plan', '2026-10-14T21:12:00.000Z', 'hs', ?3, ?4, ?5)",
                    rusqlite::params![CTX, format!("hs-p{n}"), format!("The {plan} plan"), plan, machine],
                )?;
            }
            Ok(())
        })
        .unwrap();
    drop(store);
    for name in ["old", "two"] {
        fixture_plan(&env, name, CTX);
    }
    let (_, _, mut store) = library(&env);
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "UPDATE prs SET repo = '/r/b', key = 'b' WHERE plan = 'two';
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                   VALUES ('two', 'a', '/r/a', 'main', 'waiting', 'cpd/two/a', cpd_txn(), 't'),
                          ('two', 'c', '/r/b', 'main', 'waiting', 'cpd/two/c', cpd_txn(), 't'),
                          ('two', 'd', '/r/dropped', 'main', 'cancelled', 'cpd/two/d', cpd_txn(), 't');",
            )?;
            Ok(())
        })
        .unwrap();

    let out = env.cpd(&["context", "show", "--context", CTX]).ok();
    let records = out["records"].as_array().unwrap();
    assert_eq!(records[0]["plan"], json!({"id": "old", "machine": "hs"}));
    let repos: Vec<&Value> = records.iter().map(|r| &r["repos"]).collect();
    assert_eq!(repos, [&json!(["/r"]), &json!(["/r/a", "/r/b"]), &json!([]), &json!([])], "{records:?}");
    let md = out["markdown"].as_str().unwrap();
    assert!(md.contains("old (on hs, repos /r): The old plan"), "{md}");
    assert!(md.contains("two (on hs, repos /r/a, /r/b): The two plan"), "{md}");
    assert!(md.contains("none (on hs): The none plan") && md.contains("far (on zz): The far plan"), "{md}");
    assert!(!md.contains("/r/dropped"), "{md}");
}

/// A Plan `plan` of `CTX` with a `plan` record `hs-p<n>` and a `brief` record `hs-b<n>` (written as `plan new` does).
fn fixture_brief(env: &TestEnv, n: u32, plan: &str, text: &str) {
    let (_, _, mut store) = library(env);
    store
        .write("fixture", |tx| {
            for (letter, kind, text) in [("p", "plan", format!("The {plan} plan")), ("b", "brief", text.to_owned())] {
                tx.execute(
                    "INSERT INTO context_records (context, id, kind, at, machine, text, plan_id, plan_machine)
                     VALUES (?1, ?2, ?3, '2026-10-14T21:12:00.000Z', 'hs', ?4, ?5, 'hs')",
                    rusqlite::params![CTX, format!("hs-{letter}{n}"), kind, text, plan],
                )?;
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn show_lists_a_brief_short_under_its_plan_and_record_prints_it_whole() {
    let env = hs();
    new_context(&env);
    let text = "Retry payouts.\nSecond line, with detail.\n";
    fixture_brief(&env, 1, "pay", text);
    fixture_brief(&env, 2, "web", "Web page.");
    env.cpd(&["context", "note", "A note.", "--context", CTX]).ok();

    let out = env.cpd(&["context", "show", "--context", CTX]).ok();
    let records = out["records"].as_array().unwrap();
    let brief = records.iter().find(|r| r["id"] == "hs-b1").unwrap();
    assert_eq!(brief["kind"], "brief");
    assert_eq!(brief["text"], "Retry payouts.");
    assert_eq!(brief["short"], true);
    assert_eq!(brief["plan"], json!({"id": "pay", "machine": "hs"}));
    assert!(brief.get("repos").is_none(), "{brief}");
    assert!(records.iter().filter(|r| r["kind"] != "brief").all(|r| r.get("short").is_none()));
    let md = out["markdown"].as_str().unwrap();
    assert!(md.contains("- hs-p1 (2026-10-14): pay (on hs): The pay plan\n  - brief hs-b1 (2026-10-14): Retry payouts.\n"), "{md}");
    assert!(md.contains("- hs-p2 (2026-10-14): web (on hs): The web plan\n  - brief hs-b2 (2026-10-14): Web page.\n"), "{md}");
    assert!(!md.contains("Second line"), "{md}");

    // `--since` past the Plans but before nothing else: only later records; a new brief brings its Plan.
    let since = env.cpd(&["context", "show", "--context", CTX, "--since", "3"]).ok();
    assert_eq!(since["records"].as_array().unwrap().len(), 2, "{since}");
    let md = since["markdown"].as_str().unwrap();
    assert!(md.contains("hs-p2") && md.contains("brief hs-b2") && !md.contains("hs-p1"), "{md}");

    // `--record`: that one record, in full.
    let one = env.cpd(&["context", "show", "--context", CTX, "--record", "hs-b1"]).ok();
    let records = one["records"].as_array().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!((&records[0]["id"], &records[0]["text"], &records[0]["seq"]), (&json!("hs-b1"), &json!(text), &json!(2)));
    assert!(records[0].get("short").is_none(), "{one}");
    let md = one["markdown"].as_str().unwrap();
    assert!(md.starts_with("# Building cpd\n"), "{md}");
    assert!(md.contains("hs-b1") && md.contains("Retry payouts.\nSecond line, with detail."), "{md}");
    assert!(!md.contains("hs-p1") && !md.contains("Web page."), "{md}");
    let note = env.cpd(&["context", "show", "--context", CTX, "--record", "hs-n1"]).ok();
    assert_eq!(note["records"][0]["text"], "A note.");
}

#[test]
fn show_record_refusals() {
    let env = hs();
    new_context(&env);
    fixture_brief(&env, 1, "pay", "Retry payouts.");
    let error = refused(env.cpd(&["context", "show", "--context", CTX, "--record", "hs-b9"]), 3, "record_not_found");
    assert_eq!(error["details"]["record"], "hs-b9");
    refused(env.cpd(&["context", "show", "--context", CTX, "--record", "nope"]), 2, "invalid_record_id");
    refused(env.cpd(&["context", "show", "--context", CTX, "--record", "hs-b1", "--since", "0"]), 2, "usage");
    refused(env.cpd(&["context", "show", "--context", "nope-ctx", "--record", "hs-b1"]), 4, "context_not_found");
    // A record of another Context is not found.
    env.cpd(&["context", "new", "other", "--title", "Other", "--owner", "acme"]).ok();
    refused(env.cpd(&["context", "show", "--context", "other", "--record", "hs-b1"]), 3, "record_not_found");
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

#[test]
fn rename_works_on_an_archived_context() {
    let env = hs();
    new_context(&env);
    env.cpd(&["context", "archive", CTX]).ok();
    let out = env.cpd(&["context", "rename", CTX, "--title", "Archived and renamed"]).ok();
    assert_eq!(out["context"], json!({"id": CTX, "title": "Archived and renamed"}));
    let listed = env.cpd(&["context", "list"]).ok();
    assert_eq!(listed["contexts"][0]["title"], "Archived and renamed");
    assert!(listed["contexts"][0]["archived_at"].is_string(), "{listed}");
    assert_eq!(rename_events(&env).len(), 1);
}

#[test]
fn titles_are_1_to_200_characters_with_no_control_characters() {
    let env = hs();
    let (long, ok) = ("a".repeat(201), "a".repeat(200));
    for title in [long.as_str(), "a\nb", "a\tb", "a\u{7f}b"] {
        refused(env.cpd(&["context", "new", CTX, "--title", title, "--owner", "acme"]), 2, "usage");
    }
    env.cpd(&["context", "new", CTX, "--title", &ok, "--owner", "acme"]).ok();
    for title in [long.as_str(), "a\nb", "a\tb"] {
        refused(env.cpd(&["context", "rename", CTX, "--title", title]), 2, "usage");
        // Validated before the Context is looked up.
        refused(env.cpd(&["context", "rename", "nope", "--title", title]), 2, "usage");
    }
    assert!(rename_events(&env).is_empty());
    let renamed = "b".repeat(200);
    assert_eq!(env.cpd(&["context", "rename", CTX, "--title", &renamed]).ok()["context"]["title"], renamed.as_str());
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
    env.sql(&format!("UPDATE sessions SET tmux_pane = '{pane}' WHERE id = '{id}'"));
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

/// Context `building-cpd` with its Manager, Plans `pay` and `ship` (each with a Lead),
/// a side fork of `pay`, and a Worker of `pay`'s Job `api`; another Context `other` with Plan
/// `side`. Live parked panes: `lead-pay`, `fork-pay` and `manager`.
fn archivable(env: &TestEnv) {
    new_context(env);
    env.cpd(&["context", "new", "other", "--title", "Other", "--owner", "acme"]).ok();
    fixture_plan(env, "pay", CTX);
    fixture_plan(env, "ship", CTX);
    fixture_plan(env, "side", "other");
    env.sql(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, forked_from)
           VALUES ('fork-pay', 'lead', 'building-cpd', 'pay', 'Fork', '/', 'O3', 'stopped', 't', 't1', 'lead-pay'),
                  ('manager', 'manager', 'building-cpd', NULL, 'Manager', '/', 'O5', 'idle', 't', 't0', NULL);
         INSERT INTO jobs (plan, key, pr, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
           VALUES ('pay', 'api', 'r', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'draft', 0, 1, 't');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
           VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', 'cpd/pay--api-a1', '/w', 'worker1', 't');
         INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
           VALUES ('worker1', 'worker', 'building-cpd', 'pay', 'api-a1', 'Worker', '/', 'S2', 'idle', 't', 't2');",
    );
    cockpit(env);
    for id in ["lead-pay", "fork-pay", "manager"] {
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
fn archive_covers_plans_leads_forks_and_the_manager_with_one_event_each() {
    let env = hs_tmux();
    archivable(&env);
    let panes = live_panes(&env);
    let out = env.cpd(&["context", "archive", CTX]).ok();
    assert_eq!(out["archived"][0], json!({"kind": "context", "id": CTX}));
    assert_eq!(ids(&out, "context"), [CTX]);
    assert_eq!(ids(&out, "plan"), ["pay", "ship"]);
    assert_eq!(ids(&out, "session"), ["fork-pay", "lead-pay", "lead-ship", "manager"]);
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
        env.sql(&apply);
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
    // The chat slot: the pane of window `main` running the Manager's pane.
    let manager = env.tmux(&["list-panes", "-a", "-F", "#{pane_id}\t#{@cpd_session}"]);
    let manager = String::from_utf8(manager.stdout).unwrap();
    let manager = manager.lines().find_map(|l| l.strip_suffix("\tmanager")).unwrap().to_owned();
    assert!(env.tmux(&["rename-window", "-t", &manager, "main"]).status.success());
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
    assert_eq!(ids(&out, "session"), ["fork-pay", "lead-pay", "lead-ship", "manager"]);
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

// ---- context show --digest (contract §4.6, commands.md §3.3) --------------

fn digest_dir(env: &TestEnv) -> PathBuf {
    let (home, _, _) = library(env);
    home.context_digest_dir(&CTX.parse().unwrap())
}

#[test]
fn show_digest_without_a_digest_is_null_and_not_an_error() {
    let env = hs();
    new_context(&env);
    env.cpd(&["context", "note", "A note.", "--context", CTX]).ok();
    let out = env.cpd(&["context", "show", "--context", CTX, "--digest"]).ok();
    assert_eq!(out["context"], json!({"id": CTX, "title": "Building cpd", "jira": null}));
    assert_eq!(out["digest"], Value::Null);
    assert_eq!(out["markdown"], Value::Null);
    assert!(out.get("records").is_none(), "no record is read: {out}");
    assert!(!digest_dir(&env).exists(), "reading writes nothing");
}

#[test]
fn show_digest_prints_the_latest_digest_file() {
    let env = hs();
    new_context(&env);
    let dir = digest_dir(&env);
    fs::create_dir_all(&dir).unwrap();
    let text = "# Building cpd: digest\n\nOne engine per machine [hs-d7] — é.\n";
    for (name, body) in [("9.md", "old"), ("10.md", text), ("11.md.tmp", "not yet"), ("prompt.md", "p")] {
        fs::write(dir.join(name), body).unwrap();
    }
    let out = env.cpd(&["context", "show", "--context", CTX, "--digest"]).ok();
    assert_eq!(out["digest"], json!({"upto_seq": 10, "file": dir.join("10.md"), "bytes": text.len()}));
    assert_eq!(out["markdown"], text);
    assert!(out.get("records").is_none());
}

#[test]
fn show_digest_is_exclusive_with_since_and_record_and_needs_a_known_context() {
    let env = hs();
    new_context(&env);
    refused(env.cpd(&["context", "show", "--context", CTX, "--digest", "--since", "1"]), 2, "usage");
    refused(env.cpd(&["context", "show", "--context", CTX, "--digest", "--record", "hs-d1"]), 2, "usage");
    refused(env.cpd(&["context", "show", "--context", "nope", "--digest"]), 4, "context_not_found");
}

// ---- nothing is lost (contract §4.6, D7) ------------------------------------

/// The record ids `docs/research/unplanned.md` lists as decided work no Plan
/// holds: the ids in the parentheses after each MISSING item's title, and the
/// ones in the Records column of "Owned by a record only".
fn unplanned_ids() -> Vec<RecordId> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/research/unplanned.md");
    let text = fs::read_to_string(path).unwrap();
    let section = |title: &str| {
        let body = text.split(&format!("\n## {title}")).nth(1).unwrap_or_else(|| panic!("no section {title}"));
        body.split("\n## ").next().unwrap().to_owned()
    };
    let mut ids: Vec<RecordId> = Vec::new();
    let mut take = |from: &str| {
        let mut rest = from;
        while let Some(at) = rest.find("hs-") {
            rest = &rest[at..];
            let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).unwrap_or(rest.len());
            if let Ok(id) = RecordId::parse(&rest[..end]) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            rest = &rest[end..];
        }
    };
    for line in section("MISSING").lines().filter(|l| l.starts_with("**")) {
        take(&line[line.rfind('(').expect("an item's title ends with its records in parentheses")..]);
    }
    for row in section("Owned by a record only").lines().filter(|l| l.starts_with("| ") && !l.starts_with("| Work") && !l.starts_with("|---")) {
        take(row.split('|').nth(2).expect("a Records column"));
    }
    ids
}

fn record_of(id: &RecordId) -> Record {
    let draft = match id.kind() {
        RecordKind::Decision => Draft::Decision { supersedes: None },
        RecordKind::Note => Draft::Note,
        RecordKind::Question => Draft::Question,
        RecordKind::Answer => Draft::Answer { question: "hs-q1".parse().unwrap() },
        kind => panic!("{id} is a {kind:?} record, which is never in force"),
    };
    Record { id: id.clone(), at: "2026-10-18T05:53:00.000Z".into(), by: By { session: None, plan: None }, text: id.to_string(), draft }
}

