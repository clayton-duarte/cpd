//! `cpd session retitle` (commands.md §10.14, contract §2.2 Session `title`,
//! §5.2 `session-titled`; hs-d257), run in this process (`Fx::with_ports`) with a `pi` route of the `FakeRunner`
//! (`FakePi`: its stdin, argv, environment and cwd are the runner's record, its answer, exit and stderr are scripted)
//! over a Tier catalog seeded in the cache, and the fake tmux server.

use crate::common;

use std::fs::{self, File};
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::Arc;

use common::fx::Fx;
use common::session::{FakePi, show};
use common::{Run, TestEnv, refused};
use cpd_core::host::runner::Reply;
use serde_json::{Value, json};

const LEAD: &str = "lead1";
const MANAGER: &str = "manager1";
const WORKER: &str = "worker1";
const ARCHIVED: &str = "lead2";
const NO_FILE: &str = "lead3";

/// A session file of `messages`, each `(role, text)`, as pi writes them, with entries that are
/// not messages of the conversation between them.
fn session_file(env: &TestEnv, name: &str, messages: &[(&str, String)]) -> PathBuf {
    let mut lines = vec![json!({"type": "session", "id": name}).to_string()];
    for (i, (role, text)) in messages.iter().enumerate() {
        let content = if i % 2 == 0 {
            json!([{"type": "text", "text": text}])
        } else {
            json!(text)
        };
        lines.push(
            json!({"type": "message", "message": {"role": role, "content": content}}).to_string(),
        );
        lines.push(json!({"type": "message", "message": {"role": "assistant", "content": [{"type": "toolCall", "name": "bash", "arguments": {}}]}}).to_string());
        lines.push(json!({"type": "message", "message": {"role": "toolResult", "content": [{"type": "text", "text": "TOOL OUTPUT"}]}}).to_string());
        lines.push(
            json!({"type": "custom_message", "customType": "cpd", "content": "CPD EVENT"})
                .to_string(),
        );
    }
    let path = env.root().join(format!("{name}.jsonl"));
    fs::write(&path, lines.join("\n") + "\n").unwrap();
    path
}

/// `n` alternating user and assistant messages `message <i>`.
fn numbered(n: usize) -> Vec<(&'static str, String)> {
    (1..=n)
        .map(|i| {
            (
                if i % 2 == 1 { "user" } else { "assistant" },
                format!("message {i}"),
            )
        })
        .collect()
}

/// `config.toml`, the fake pi install and its catalog (the cache holds O, S, H only: G is served from the config), Context `ctx`, Plan `pay` with its Lead `lead1`, a Manager, a Worker, an archived Lead
/// and a Lead with no session file. `lead1`'s and `manager1`'s files hold a short conversation.
fn setup(fx: &Fx) -> Arc<FakePi> {
    let env = &fx.env;
    env.write_models();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    let pi = FakePi::install(fx);
    answer(&pi, "TITLE: Dynamic session titles\n", 0);
    fs::create_dir_all(env.root().join("install")).unwrap();
    fs::write(env.root().join("install/current-version"), "1.0.0\n").unwrap();
    let scope = env
        .root()
        .join("install/releases/1.0.0/node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    let lead_file = session_file(env, LEAD, &numbered(4));
    let manager_file = session_file(env, MANAGER, &numbered(2));
    env.sql(
        &format!(
            "INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES
               ('1.0.0', 'O', 'p/opus', '[0,1,2,3,4,5]', 't'), ('1.0.0', 'S', 'p/sonnet', '[1,2,3,4,5]', 't'),
               ('1.0.0', 'H', 'p/haiku', '[1,2,3]', 't');
             INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
             INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, state, state_at, created_at, archived_at) VALUES
               ('lead1', 'lead', 'ctx', 'pay', NULL, 'Lead one', '{lead}', '/', 'O3', 'idle', 't', 't1', NULL),
               ('manager1', 'manager', 'ctx', NULL, NULL, 'Manager', '{manager}', '/', 'O5', 'idle', 't', 't0', NULL),
               ('worker1', 'worker', 'ctx', 'pay', 'api-a1', 'pay api a1', '{lead}', '/', 'S2', 'working', 't', 't3', NULL),
               ('lead2', 'lead', 'ctx', 'pay', NULL, 'Lead two', '{lead}', '/', 'O3', 'stopped', 't', 't2', 't9'),
               ('lead3', 'lead', 'ctx', 'pay', NULL, 'Lead three', NULL, '/', 'O3', 'idle', 't', 't4', NULL);
             INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pay', 'ctx', 'Pay', 'grounding', 'lead1', 't');
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
               VALUES ('pay', 'r', '/r', 'main', 'waiting', 'cpd/pay', cpd_txn(), 't');
             INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
               VALUES ('pay', 'r', 'api', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fp}', cpd_txn(), 't');
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
               VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', 'cpd/pay--api-a1', '/w', 'worker1', 't');",
            lead = lead_file.display(),
            manager = manager_file.display(),
            fp = "a".repeat(64),
        ),
    );
    pi
}

/// What the fake pi answers next: `stdout`, and its exit status.
fn answer(pi: &FakePi, stdout: &str, status: i32) {
    pi.say(Reply::exit(status, stdout, ""));
}

/// The chat slot's width the tests retitle at: the limit is 68 - 8 = 60 characters.
const WIDTH: &str = "68";

fn retitle(fx: &Fx, session: &str) -> Run {
    fx.cpd(&["session", "retitle", session, "--width", WIDTH])
}

fn titled_events(fx: &Fx) -> Vec<Value> {
    fx.cpd(&["events", "list", "--kind", "session-titled"]).ok()["events"]
        .as_array()
        .unwrap()
        .clone()
}

/// What the last run of pi read on its stdin.
fn pi_stdin(fx: &Fx) -> String {
    let stdin = fx
        .calls("pi")
        .pop()
        .expect("pi was called")
        .stdin
        .expect("pi was given a stdin");
    String::from_utf8(stdin).unwrap()
}

/// Nothing was written: no title, no `titled_at`, no `session-titled` Event.
fn nothing_written(fx: &Fx, session: &str) {
    let row = show(fx, session);
    assert_eq!(
        (&row["title"], &row["titled_at"]),
        (&Value::Null, &Value::Null),
        "{row}"
    );
    assert!(titled_events(fx).is_empty());
}

// ---------- pi's run ----------

#[test]
fn the_title_writer_runs_pi_on_gx() {
    let fx = Fx::with_ports();
    setup(&fx);
    retitle(&fx, LEAD).ok();
    let calls = fx.calls("pi");
    assert_eq!(calls.len(), 1, "{calls:?}");
    let call = &calls[0];
    let argv = &call.args;
    assert_eq!(
        argv[..6],
        [
            "--print",
            "--no-session",
            "--no-extensions",
            "--model",
            "p/gemini:off",
            "--"
        ],
        "{argv:?}"
    );
    assert_eq!(argv.len(), 7, "one prompt after `--`: {argv:?}");
    let prompt = &argv[6];
    for asked in [
        "title",
        "at most 60 characters",
        "last happening",
        "one line",
        "no quotes",
    ] {
        assert!(
            prompt.contains(asked),
            "the prompt asks for {asked:?}: {prompt}"
        );
    }
    let set = |key: &str| {
        call.envs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(
        set("PI_SKIP_VERSION_CHECK"),
        Some(Some("1".to_owned())),
        "{:?}",
        call.envs
    );
    // No `CPD_*` reaches pi: every one the process has is removed from its environment and none is set.
    assert!(
        call.envs
            .iter()
            .all(|(k, v)| !k.starts_with("CPD_") || v.is_none()),
        "{:?}",
        call.envs
    );
    assert_eq!(
        set("CPD_HOME"),
        Some(None),
        "CPD_HOME is removed from pi's environment: {:?}",
        call.envs
    );
    let cwd = call.cwd.as_ref().expect("pi runs in a directory");
    assert_eq!(
        fs::canonicalize(cwd).unwrap(),
        fs::canonicalize(fx.env.cpd_home().join("run/titles")).unwrap()
    );
}

#[test]
fn stdin_is_the_text_of_the_last_two_turns_in_order_and_the_whole_file_when_shorter() {
    let fx = Fx::with_ports();
    setup(&fx);
    session_file(&fx.env, LEAD, &numbered(40));
    retitle(&fx, LEAD).ok();
    // The second-to-last user message is message 37.
    assert_eq!(
        pi_stdin(&fx),
        "user: message 37\n\nassistant: message 38\n\nuser: message 39\n\nassistant: message 40"
    );
    session_file(&fx.env, LEAD, &numbered(2));
    retitle(&fx, LEAD).ok();
    assert_eq!(pi_stdin(&fx), "user: message 1\n\nassistant: message 2");
}

#[test]
fn stdin_is_cut_to_the_newest_8000_characters() {
    let fx = Fx::with_ports();
    setup(&fx);
    // Two turns of 4 messages of 3 000 characters each (a multi-byte character among them), the newest last.
    let messages: Vec<(&str, String)> = (0..8)
        .map(|i| {
            (
                if i % 2 == 0 { "user" } else { "assistant" },
                format!("{}é{}", char::from(b'a' + i as u8), "x".repeat(2998)),
            )
        })
        .collect();
    session_file(&fx.env, LEAD, &messages);
    retitle(&fx, LEAD).ok();
    let stdin = pi_stdin(&fx);
    assert_eq!(
        stdin.chars().count(),
        8000,
        "{} characters",
        stdin.chars().count()
    );
    assert!(stdin.ends_with(&messages[7].1), "the newest is kept whole");
    assert!(!stdin.contains(&messages[4].1), "the oldest are cut");
}

// ---------- the answer ----------

#[test]
fn the_answer_is_the_only_marked_line_unquoted_and_anything_else_is_malformed_and_never_cut() {
    let fx = Fx::with_ports();
    let pi = setup(&fx);
    let over_long = format!("TITLE: {} end", "word ".repeat(15));
    let sixty_one = format!("TITLE: {}", "a".repeat(61));
    // Not one checked `TITLE:` line (§10.14 step 4): `malformed`, nothing written, nothing cut.
    for said in [
        over_long.as_str(),
        sixty_one.as_str(),
        "Dynamic session titles\n",
        "TITLE: First title\nTITLE: Second title\n",
        "TITLE:\n",
        "TITLE: \"\"\n",
        "TITLE: A title that reads. As sentences\n",
        "TITLE: Ends with a full stop.\n",
    ] {
        answer(&pi, said, 0);
        let error = refused(&retitle(&fx, LEAD), "title_failed", 1);
        assert_eq!(error["details"]["reason"], "malformed", "{said:?}");
        nothing_written(&fx, LEAD);
    }
    // The title is 60 characters at most (the width 68 less 8), and exactly 60 passes.
    let sixty = "a".repeat(60);
    for (said, title) in [
        (
            "TITLE: \"Quoted session title\"\n".to_owned(),
            "Quoted session title".to_owned(),
        ),
        (
            "  TITLE:   'Single quoted'  \n".to_owned(),
            "Single quoted".to_owned(),
        ),
        (
            "TITLE: “Curly quotes”".to_owned(),
            "Curly quotes".to_owned(),
        ),
        (
            "  TITLE: Leading spaces\n".to_owned(),
            "Leading spaces".to_owned(),
        ),
        (
            "Now I understand the task.\nTITLE: After a preamble\n".to_owned(),
            "After a preamble".to_owned(),
        ),
        (
            "\n\nTITLE: After blank lines\nand a closing line\n".to_owned(),
            "After blank lines".to_owned(),
        ),
        (format!("TITLE: {sixty}"), sixty.clone()),
    ] {
        answer(&pi, &said, 0);
        let out = retitle(&fx, LEAD).ok();
        assert_eq!(out["title"], json!(title), "{said:?}");
        assert_eq!(show(&fx, LEAD)["title"], json!(title));
    }
}

#[test]
fn an_empty_answer_is_title_failed_and_writes_nothing() {
    let fx = Fx::with_ports();
    let pi = setup(&fx);
    for said in ["", "\n\n", "   \n"] {
        answer(&pi, said, 0);
        let error = refused(&retitle(&fx, LEAD), "title_failed", 1);
        assert_eq!(
            (
                error["details"]["session"].as_str(),
                error["details"]["reason"].as_str()
            ),
            (Some(LEAD), Some("empty")),
            "{said:?}"
        );
        nothing_written(&fx, LEAD);
    }
}

#[test]
fn an_answer_the_title_type_refuses_is_title_failed_malformed_and_writes_nothing() {
    let fx = Fx::with_ports();
    let pi = setup(&fx);
    // §10.14 step 4: an answer that is not a Title (a control character) is `malformed`, not `invalid`.
    answer(&pi, "TITLE: Tab\tinside\n", 0);
    let error = refused(&retitle(&fx, LEAD), "title_failed", 1);
    assert_eq!(error["details"]["reason"], "malformed");
    nothing_written(&fx, LEAD);
}

#[test]
fn a_pi_that_fails_is_title_failed_with_its_stderr_tail_and_writes_nothing() {
    let fx = Fx::with_ports();
    let pi = setup(&fx);
    pi.say(Reply::exit(
        3,
        "TITLE: A title anyway\n",
        "model overloaded\n",
    ));
    let error = refused(&retitle(&fx, LEAD), "title_failed", 1);
    let details = &error["details"];
    assert_eq!(
        (details["reason"].as_str(), details["exit"].as_i64()),
        (Some("exit"), Some(3)),
        "{error}"
    );
    assert!(
        details["tail"]
            .as_str()
            .unwrap()
            .contains("model overloaded"),
        "{error}"
    );
    nothing_written(&fx, LEAD);

    // pi cannot start (the runner could not run it, as a missing interpreter).
    pi.say(Reply::io(ErrorKind::NotFound));
    let error = refused(&retitle(&fx, LEAD), "title_failed", 1);
    assert_eq!(error["details"]["reason"], "spawn", "{error}");
    nothing_written(&fx, LEAD);
}

// ---------- the write ----------

#[test]
fn one_transaction_writes_title_titled_at_and_the_session_titled_event_and_a_rerun_writes_a_new_title()
 {
    let fx = Fx::with_ports();
    let pi = setup(&fx);
    let out = retitle(&fx, LEAD).ok();
    assert_eq!(
        (out["session"].as_str(), out["title"].as_str(), &out["pane"]),
        (Some(LEAD), Some("Dynamic session titles"), &Value::Null)
    );
    let row = show(&fx, LEAD);
    assert_eq!(
        (&row["title"], &row["titled_at"]),
        (&out["title"], &out["titled_at"])
    );
    assert!(
        out["titled_at"]
            .as_str()
            .is_some_and(|t| t.starts_with("20")),
        "{out}"
    );
    let events = titled_events(&fx);
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event["id"], out["event"]);
    assert_eq!(
        (
            &event["context"],
            &event["plan"],
            &event["session"],
            &event["data"]
        ),
        (
            &json!("ctx"),
            &json!("pay"),
            &json!(LEAD),
            &json!({"title": "Dynamic session titles"})
        )
    );
    assert_eq!(
        event["text"],
        format!("Session {LEAD} titled \"Dynamic session titles\".")
    );
    // The Event and the row are of one transaction.
    let same: i64 = fx.env.query(&format!(
        "SELECT count(*) FROM events e JOIN txns t ON t.id = e.txn JOIN sessions s ON s.titled_at = t.at WHERE e.kind = 'session-titled' AND s.id = '{LEAD}'"
    ));
    assert_eq!(same, 1);

    answer(&pi, "TITLE: Another subject now\n", 0);
    let again = retitle(&fx, LEAD).ok();
    assert_eq!(again["title"], "Another subject now");
    assert!(again.get("already").is_none());
    assert_eq!(show(&fx, LEAD)["title"], "Another subject now");
    assert_eq!(titled_events(&fx).len(), 2);
}

#[test]
fn a_manager_is_titled_with_no_plan_on_its_event() {
    let fx = Fx::with_ports();
    setup(&fx);
    retitle(&fx, MANAGER).ok();
    let events = titled_events(&fx);
    assert_eq!(
        (
            &events[0]["session"],
            &events[0]["plan"],
            &events[0]["context"]
        ),
        (&json!(MANAGER), &Value::Null, &json!("ctx"))
    );
    assert_eq!(show(&fx, MANAGER)["title"], "Dynamic session titles");
}

#[test]
fn session_list_and_show_carry_title_and_titled_at() {
    let fx = Fx::with_ports();
    setup(&fx);
    let list = fx.cpd(&["session", "list"]).ok();
    for session in list["sessions"].as_array().unwrap() {
        assert_eq!(
            (&session["title"], &session["titled_at"]),
            (&Value::Null, &Value::Null),
            "{session}"
        );
    }
    let out = retitle(&fx, LEAD).ok();
    let list = fx.cpd(&["session", "list"]).ok();
    let lead = list["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == LEAD)
        .unwrap()
        .clone();
    assert_eq!(
        (&lead["title"], &lead["titled_at"]),
        (&out["title"], &out["titled_at"])
    );
}

// ---------- tmux ----------

#[test]
fn the_title_is_set_as_cpd_title_on_the_sessions_live_pane() {
    let fx = Fx::with_ports();
    setup(&fx);
    let out = fx.tmux(&[
        "new-session",
        "-d",
        "-s",
        "cpd",
        "-P",
        "-F",
        "#{pane_id}",
        "sleep 120",
    ]);
    assert!(out.status.success(), "{out:?}");
    let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    fx.tmux(&["set", "-p", "-t", &pane, "@cpd_session", LEAD]);
    fx.tmux(&["set", "-p", "-t", &pane, "@cpd_title", "Pay"]);
    let out = retitle(&fx, LEAD).ok();
    assert_eq!(out["pane"], json!(pane));
    let title = fx.tmux(&["show", "-pqv", "-t", &pane, "@cpd_title"]);
    assert_eq!(
        String::from_utf8(title.stdout)
            .unwrap()
            .trim_end_matches('\n'),
        "Dynamic Session Titles"
    );

    // Another Session's pane is not touched.
    let out = retitle(&fx, MANAGER).ok();
    assert_eq!(out["pane"], Value::Null);
    let title = fx.tmux(&["show", "-pqv", "-t", &pane, "@cpd_title"]);
    assert_eq!(
        String::from_utf8(title.stdout)
            .unwrap()
            .trim_end_matches('\n'),
        "Dynamic Session Titles"
    );
}

// ---------- refusals and skips ----------

#[test]
fn an_archived_session_a_worker_and_an_unknown_one_are_refused_before_pi() {
    let fx = Fx::with_ports();
    setup(&fx);
    let error = refused(&retitle(&fx, ARCHIVED), "archived", 3);
    assert_eq!(
        error["details"]["next"],
        format!("cpd session unarchive {ARCHIVED}")
    );
    refused(&retitle(&fx, WORKER), "not_a_lead", 3);
    refused(&retitle(&fx, "nobody"), "session_not_found", 4);
    assert!(fx.calls("pi").is_empty());
    nothing_written(&fx, ARCHIVED);
}

#[test]
fn a_session_with_no_messages_is_skipped_with_no_pi_and_nothing_written() {
    let fx = Fx::with_ports();
    setup(&fx);
    let skip = |session: &str| json!({"ok": true, "session": session, "title": null, "skipped": "no_messages"});
    // No file yet.
    assert_eq!(retitle(&fx, NO_FILE).json(), skip(NO_FILE));
    // A file that does not exist yet (pi writes it lazily), and one with no user or assistant text.
    let missing = fx.env.root().join("missing.jsonl");
    let empty = session_file(
        &fx.env,
        "empty",
        &[
            ("toolResult", "only tools".to_owned()),
            ("assistant", "  ".to_owned()),
        ],
    );
    for file in [missing, empty] {
        fx.env.sql(&format!(
            "UPDATE sessions SET file = '{}' WHERE id = '{NO_FILE}'",
            file.display()
        ));
        assert_eq!(retitle(&fx, NO_FILE).json(), skip(NO_FILE));
    }
    assert!(fx.calls("pi").is_empty());
    nothing_written(&fx, NO_FILE);
}

#[test]
fn a_held_lock_is_title_failed_running_with_no_pi() {
    let fx = Fx::with_ports();
    setup(&fx);
    let path = fx.env.cpd_home().join(format!("locks/title-{LEAD}.lock"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = File::create(&path).unwrap();
    file.try_lock().unwrap();
    let error = refused(&retitle(&fx, LEAD), "title_failed", 1);
    assert_eq!(error["details"]["reason"], "running");
    assert!(fx.calls("pi").is_empty());
    nothing_written(&fx, LEAD);
    drop(file);
    retitle(&fx, LEAD).ok();
}
