//! `cpd mcp attach` (commands.md §24.1, contract §5.2 `mcp-attached`, §8.3 "The machine scope"; pro-d2, pro-d8), run
//! as the built binary on machine `pro`, whose policy file is written into `$CPD_HOME/policy/host.toml`.
//!
//! Every test here fails before the command exists, for the reason its comment gives.

use crate::common::{TestEnv, refused};
use crate::common::session::{events, setup};

use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};

const MANAGER: &str = "manager1";
const LEAD: &str = "lead1";
const WORKER: &str = "worker1";
const OTHER: &str = "lead2";

/// The machine file of §22.1's example (`slack`) and a second server with only a `url`.
const MACHINE: &str = r#"
[mcp.servers.slack]
url = "https://mcp.slack.com/mcp"
description = "Slack: read channels and threads, post messages, and manage reactions."
exposure = "hidden"

[mcp.servers.slack.oauth]
clientId = "1601185624273.8899143856786"
callbackUrl = "http://localhost:3118/callback"
scope = "channels:history chat:write reactions:write"

[mcp.servers.slack.toolExposure]
slack_read_channel = "direct"
slack_read_thread = "direct"
slack_add_reaction = "direct"
slack_get_reactions = "direct"
slack_search_emojis = "direct"
slack_read_user_profile = "direct"
slack_send_message = "direct"

[mcp.servers.files]
url = "https://files.example.com/mcp"
"#;

fn slack() -> Value {
    json!({
        "url": "https://mcp.slack.com/mcp",
        "description": "Slack: read channels and threads, post messages, and manage reactions.",
        "exposure": "hidden",
        "oauth": {
            "clientId": "1601185624273.8899143856786",
            "callbackUrl": "http://localhost:3118/callback",
            "scope": "channels:history chat:write reactions:write"
        },
        "toolExposure": {
            "slack_read_channel": "direct",
            "slack_read_thread": "direct",
            "slack_add_reaction": "direct",
            "slack_get_reactions": "direct",
            "slack_search_emojis": "direct",
            "slack_read_user_profile": "direct",
            "slack_send_message": "direct"
        }
    })
}

fn files() -> Value {
    json!({"url": "https://files.example.com/mcp"})
}

fn host_file(env: &TestEnv) -> PathBuf {
    env.cpd_home().join("policy/host.toml")
}

/// `session.rs`'s fixture on machine `pro`, with no machine file.
fn bare(env: &TestEnv) {
    setup(env);
    env.write_config(&format!("version = 1\nmachine = \"pro\"\nlead_pi_packages = []\n\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket()));
}

/// The fixture and the machine file `text`.
fn with_file(env: &TestEnv, text: &str) {
    bare(env);
    fs::create_dir_all(host_file(env).parent().unwrap()).unwrap();
    fs::write(host_file(env), text).unwrap();
}

fn fixture(env: &TestEnv) {
    with_file(env, MACHINE);
}

/// `cpd mcp attach <name>` as `session`, through `CPD_SESSION`.
fn attach(env: &TestEnv, session: &str, name: &str) -> crate::common::Run {
    env.cpd_with(env.root(), &["mcp", "attach", name], &[("CPD_SESSION", session)])
}

fn attached(env: &TestEnv) -> Vec<Value> {
    events(env, "mcp-attached")
}

fn all_events(env: &TestEnv) -> Vec<Value> {
    env.cpd(&["events", "list", "--limit", "5000"]).ok()["events"].as_array().unwrap().clone()
}

/// Every table's row count, by table name, from the store itself.
fn row_counts(env: &TestEnv) -> Vec<(String, i64)> {
    env.read(|conn| {
        let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?;
        let names: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        let mut out = Vec::new();
        for name in names {
            let n: i64 = conn.query_row(&format!("SELECT count(*) FROM \"{name}\""), [], |r| r.get(0))?;
            out.push((name, n));
        }
        Ok(out)
    })
}

/// Every file under `$CPD_HOME` except the database's own, by relative path and size.
fn files_of_home(env: &TestEnv) -> Vec<(String, u64)> {
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<(String, u64)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let rel = path.strip_prefix(root).unwrap().display().to_string();
            if rel.starts_with("cpd.db") {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                out.push((rel, fs::metadata(&path).unwrap().len()));
            }
        }
    }
    let root = env.cpd_home();
    let mut out = Vec::new();
    walk(&root, &root, &mut out);
    out.sort();
    out
}

/// A Session's attach: one Event for it, with its Context and Plan, the policy entry as `data.server`, and the output.
fn check_attach(env: &TestEnv, session: &str, plan: Value) {
    let before = attached(env).len();
    let out = attach(env, session, "slack").ok();
    let sent = attached(env);
    assert_eq!(sent.len(), before + 1, "exactly one Event: {sent:?}");
    let event = sent.last().unwrap();
    assert_eq!(event["session"], session);
    assert_eq!(event["context"], "ctx");
    assert_eq!(event["plan"], plan);
    assert_eq!(event["kind"], "mcp-attached");
    assert_eq!(event["data"], json!({"name": "slack", "server": slack()}));
    assert_eq!(event["text"], format!("Server slack attached to Session {session}."));
    assert_eq!(out["attached"], json!({"name": "slack", "session": session, "event": event["id"]}), "{out}");
}

// ---------- a: one Event, addressed to the Session ----------

/// A Lead. Fails now: `usage` (no such subcommand), exit 2, and no Event.
#[test]
fn mcp_attach_a_lead_appends_one_event_addressed_to_it() {
    let env = TestEnv::new();
    fixture(&env);
    check_attach(&env, LEAD, json!("pay"));
}

/// A Manager: its Event has no Plan. Fails now: `usage`, exit 2.
#[test]
fn mcp_attach_a_manager_appends_one_event_with_no_plan() {
    let env = TestEnv::new();
    fixture(&env);
    check_attach(&env, MANAGER, Value::Null);
}

/// A Worker's Session. Fails now: `usage`, exit 2.
#[test]
fn mcp_attach_a_worker_appends_one_event_addressed_to_it() {
    let env = TestEnv::new();
    fixture(&env);
    check_attach(&env, WORKER, json!("pay"));
}

/// `--session` selects the Session as `CPD_SESSION` does. Fails now: `usage`, exit 2.
#[test]
fn mcp_attach_session_flag_selects_the_session() {
    let env = TestEnv::new();
    fixture(&env);
    let out = env.cpd(&["mcp", "attach", "slack", "--session", LEAD]).ok();
    assert_eq!(out["attached"]["session"], LEAD, "{out}");
    let sent = attached(&env);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["session"], LEAD);
}

// ---------- b: another server, and a re-attach ----------

/// `files` has only a `url`, and attaching it leaves `slack` alone. Fails now: `usage`, exit 2.
#[test]
fn mcp_attach_another_server_does_not_touch_the_first() {
    let env = TestEnv::new();
    fixture(&env);
    attach(&env, LEAD, "slack").ok();
    let out = attach(&env, LEAD, "files").ok();
    assert_eq!(out["attached"]["name"], "files");
    let sent = attached(&env);
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!((sent[0]["data"].clone(), sent[1]["data"].clone()), (json!({"name": "slack", "server": slack()}), json!({"name": "files", "server": files()})));
    assert!(sent[1]["data"]["server"].as_object().unwrap().keys().eq(["url"]), "a key left out is absent");
}

/// Attaching twice appends two Events with equal `data`, both addressed to the Session. Fails now: `usage`, exit 2.
#[test]
fn mcp_attach_twice_appends_two_events_with_equal_data() {
    let env = TestEnv::new();
    fixture(&env);
    let first = attach(&env, WORKER, "slack").ok();
    let second = attach(&env, WORKER, "slack").ok();
    assert_ne!(first["attached"]["event"], second["attached"]["event"]);
    assert_eq!(second.get("already"), None);
    let sent = attached(&env);
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(sent[0]["data"], sent[1]["data"]);
    assert!(sent.iter().all(|e| e["session"] == WORKER));
}

// ---------- c: not for another Session ----------

/// The Event is in its Session's stream and in no other Session's. Fails now: `usage`, exit 2.
#[test]
fn mcp_attach_event_is_not_in_another_sessions_stream() {
    let env = TestEnv::new();
    fixture(&env);
    let out = attach(&env, LEAD, "slack").ok();
    let id = out["attached"]["event"].clone();
    let mine = env.cpd(&["events", "list", "--session", LEAD, "--kind", "mcp-attached"]).ok();
    assert_eq!(mine["events"].as_array().unwrap().iter().map(|e| e["id"].clone()).collect::<Vec<_>>(), [id]);
    for other in [OTHER, MANAGER, WORKER] {
        let theirs = env.cpd(&["events", "list", "--session", other]).ok();
        assert!(theirs["events"].as_array().unwrap().iter().all(|e| e["kind"] != "mcp-attached"), "{other}: {theirs}");
    }
}

// ---------- d, e: an unknown name ----------

/// A name the machine policy does not list: `mcp_unknown`, exit 4, the names that are, and no Event. Fails now:
/// `usage`, exit 2.
#[test]
fn mcp_attach_unknown_name_is_refused_with_the_available_names() {
    let env = TestEnv::new();
    fixture(&env);
    let before = all_events(&env);
    let error = refused(&attach(&env, LEAD, "nothing"), "mcp_unknown", 4);
    assert_eq!(error["details"]["name"], "nothing");
    assert_eq!(error["details"]["machine"], "pro");
    assert_eq!(error["details"]["available"], json!(["files", "slack"]));
    assert_eq!(all_events(&env), before);
}

/// No machine file at all: `mcp_unknown` with `available` `[]`. Fails now: `usage`, exit 2.
#[test]
fn mcp_attach_with_no_machine_file_lists_nothing() {
    let env = TestEnv::new();
    bare(&env);
    assert!(!host_file(&env).exists());
    let before = all_events(&env);
    let error = refused(&attach(&env, LEAD, "slack"), "mcp_unknown", 4);
    assert_eq!((error["details"]["name"].clone(), error["details"]["machine"].clone(), error["details"]["available"].clone()), (json!("slack"), json!("pro"), json!([])));
    assert_eq!(all_events(&env), before);
}

// ---------- the machine scope is the one file host.toml ----------

/// The `host.toml` dotfiles renders: `[model]` first, then the machine's `cpd.policy` text with its servers.
#[test]
fn mcp_attach_reads_servers_from_a_host_toml_that_also_holds_the_model_table() {
    let env = TestEnv::new();
    with_file(
        &env,
        &format!("[model]\nO = \"github-copilot/o\"\nS = \"github-copilot/s\"\nH = \"github-copilot/h\"\nG = \"github-copilot/g\"\n\n{MACHINE}"),
    );
    attach(&env, LEAD, "slack").ok();
    assert_eq!(attached(&env)[0]["data"]["server"], slack());
}

/// A file of the old layout's `machines` directory is not read: its server is unknown, and no Event is appended.
#[test]
fn mcp_attach_does_not_read_a_file_of_the_old_machines_directory() {
    let env = TestEnv::new();
    bare(&env);
    let old = env.cpd_home().join("policy/machines");
    fs::create_dir_all(&old).unwrap();
    fs::write(old.join("pro.toml"), MACHINE).unwrap();
    let before = all_events(&env);
    let error = refused(&attach(&env, LEAD, "slack"), "mcp_unknown", 4);
    assert_eq!(error["details"]["available"], json!([]));
    assert_eq!(all_events(&env), before);
}

// ---------- f: an invalid machine file ----------

/// An unknown key in the machine file: `policy_invalid`, exit 1, and no Event. Fails now: `usage`, exit 2.
#[test]
fn mcp_attach_invalid_machine_file_is_policy_invalid_and_appends_nothing() {
    let env = TestEnv::new();
    with_file(&env, "[mcp.servers.slack]\nurl = \"https://mcp.slack.com/mcp\"\nheaders = \"x\"\n");
    let before = all_events(&env);
    refused(&attach(&env, LEAD, "slack"), "policy_invalid", 1);
    assert_eq!(all_events(&env), before);
}

// ---------- g: the name rule, before the policy is read ----------

/// A name that is not a server name is `usage` (exit 2), even when the machine file is invalid and the Session is
/// unknown: the name is checked first. Fails now at its last line: a valid name is `usage` while the subcommand is
/// missing, where the invalid file must give `policy_invalid`.
#[test]
fn mcp_attach_a_bad_name_is_usage_before_the_policy_is_read() {
    let env = TestEnv::new();
    with_file(&env, "this is not toml [");
    let before = all_events(&env);
    for bad in ["Slack", "slack_x", "", "1slack", "slack.x", &"a".repeat(33)] {
        let run = env.cpd_with(env.root(), &["mcp", "attach", bad, "--session", "nobody"], &[]);
        refused(&run, "usage", 2);
        refused(&attach(&env, LEAD, bad), "usage", 2);
    }
    assert_eq!(all_events(&env), before);
    // The same file is read for a valid name: this is what the bad names did not reach.
    refused(&attach(&env, LEAD, "slack"), "policy_invalid", 1);
}

// ---------- h: the Session ----------

/// No `--session` and no `CPD_SESSION`: `session_required`, exit 2; an unknown one: `session_not_found`, exit 4.
/// Fails now: `usage`, exit 2, for both.
#[test]
fn mcp_attach_needs_a_known_session() {
    let env = TestEnv::new();
    fixture(&env);
    let before = all_events(&env);
    refused(&env.cpd(&["mcp", "attach", "slack"]), "session_required", 2);
    let error = refused(&env.cpd(&["mcp", "attach", "slack", "--session", "nobody"]), "session_not_found", 4);
    assert_eq!(error["details"]["session"], "nobody");
    refused(&attach(&env, "nobody", "slack"), "session_not_found", 4);
    assert_eq!(all_events(&env), before);
}

// ---------- i: nothing else is written ----------

/// The Event count grows by one and nothing else changes: the Events before are the same and the one new is
/// `mcp-attached`; no other table gains a row except `events`, and no file of `$CPD_HOME` changes. Fails now: `usage`.
#[test]
fn mcp_attach_writes_nothing_but_the_event() {
    let env = TestEnv::new();
    fixture(&env);
    let events_before = all_events(&env);
    let rows_before = row_counts(&env);
    let files_before = files_of_home(&env);

    attach(&env, LEAD, "slack").ok();

    let events_after = all_events(&env);
    assert_eq!(events_after.len(), events_before.len() + 1);
    assert_eq!(events_after[..events_before.len()], events_before[..]);
    assert_eq!(events_after.last().unwrap()["kind"], "mcp-attached");
    let rows_after = row_counts(&env);
    let changed: Vec<_> = rows_before.iter().zip(&rows_after).filter(|(a, b)| a != b).collect();
    // The `txns` row is the transaction's own bookkeeping, not a row of the command (commands.md §1 Effects, contract §4.2).
    assert!(changed.iter().all(|(a, _)| a.0 == "events" || a.0 == "txns"), "only the events table grows, and txns by its row: {changed:?}");
    let txns = |rows: &[(String, i64)]| rows.iter().find(|r| r.0 == "txns").unwrap().1;
    assert_eq!(txns(&rows_after), txns(&rows_before) + 1);
    assert_eq!(files_of_home(&env), files_before);
}

// ---------- j: the help ----------

/// `mcp attach` is in `cpd --help`'s tree under `mcp`. Fails now: no `mcp` node.
#[test]
fn mcp_attach_is_in_the_help_tree_under_mcp() {
    let env = TestEnv::new();
    let help = env.cpd(&["--help"]).json();
    let mcp = help["tree"].as_array().unwrap().iter().find(|n| n["name"] == "mcp").unwrap_or_else(|| panic!("no `mcp` in the tree: {help}"));
    let names: Vec<&str> = mcp["commands"].as_array().unwrap().iter().map(|n| n["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["attach"]);
}

/// `cpd mcp attach --help` shows `<name>`. Fails now: the subcommand does not exist.
#[test]
fn mcp_attach_help_shows_the_name_argument() {
    let env = TestEnv::new();
    let run = env.cpd(&["mcp", "attach", "--help"]);
    let text = format!("{}{}", run.stdout, run.stderr);
    assert_eq!(run.exit, 0, "{text}");
    assert!(text.contains("<name>") || text.contains("<NAME>"), "no <name> in: {text}");
}

#[test]
fn the_event_of_a_server_with_oauth_client_name_carries_the_entry_verbatim() {
    let env = TestEnv::new();
    with_file(
        &env,
        "[mcp.servers.figma]\nurl = \"https://mcp.figma.com/mcp\"\nexposure = \"deferred\"\n\n[mcp.servers.figma.oauth]\nclientName = \"Claude Code\"\n",
    );
    attach(&env, LEAD, "figma").ok();
    let sent = attached(&env);
    assert_eq!(sent.len(), 1, "exactly one Event: {sent:?}");
    let entry = json!({"url": "https://mcp.figma.com/mcp", "exposure": "deferred", "oauth": {"clientName": "Claude Code"}});
    assert_eq!(sent[0]["data"], json!({"name": "figma", "server": entry}));
}

// ---------- k: a durable Worker has no MCP client (pro-d253) ----------

fn set_runner(env: &TestEnv, runner: &str) {
    env.sql(&format!("UPDATE attempts SET runner = '{runner}' WHERE id = 'api-a1'"));
}

/// Fails now: the command refuses `runner_no_tools`. A durable Worker attaches like any Session (pro-d263, pro-d269).
#[test]
fn mcp_attach_a_durable_worker_attaches_like_any_other_session() {
    let env = TestEnv::new();
    fixture(&env);
    set_runner(&env, "durable");
    let before = all_events(&env);
    let rows_before = row_counts(&env);
    let out = attach(&env, WORKER, "slack").ok();
    let after = all_events(&env);
    assert_eq!(after.len(), before.len() + 1, "exactly one new Event: {after:?}");
    assert_eq!(after[..before.len()], before[..]);
    let event = after.last().unwrap();
    assert_eq!(event["kind"], "mcp-attached", "no error Event: {event}");
    assert_eq!(event["session"], WORKER);
    assert_eq!(event["data"], json!({"name": "slack", "server": slack()}));
    assert_eq!(out["attached"], json!({"name": "slack", "session": WORKER, "event": event["id"]}), "{out}");
    assert_eq!(attached(&env).len(), 1);
    let rows_after = row_counts(&env);
    let changed: Vec<_> = rows_before.iter().zip(&rows_after).filter(|(a, b)| a != b).collect();
    assert!(changed.iter().all(|(a, _)| a.0 == "events" || a.0 == "txns"), "only events (and txns by its row) change: {changed:?}");
}

/// Passes now and after: a pi Worker attaches as before.
#[test]
fn mcp_attach_a_pi_worker_still_appends_one_event() {
    let env = TestEnv::new();
    fixture(&env);
    set_runner(&env, "pi");
    check_attach(&env, WORKER, json!("pay"));
}

/// Passes now and after: only the caller's own Attempt runner counts.
#[test]
fn mcp_attach_a_lead_still_attaches_while_a_worker_attempt_is_durable() {
    let env = TestEnv::new();
    fixture(&env);
    set_runner(&env, "durable");
    check_attach(&env, LEAD, json!("pay"));
}

/// Passes now and after: the unknown name is checked before the runner.
#[test]
fn mcp_attach_unknown_name_on_a_durable_worker_is_still_mcp_unknown() {
    let env = TestEnv::new();
    fixture(&env);
    set_runner(&env, "durable");
    let before = all_events(&env);
    refused(&attach(&env, WORKER, "nothing"), "mcp_unknown", 4);
    assert_eq!(all_events(&env), before);
}
