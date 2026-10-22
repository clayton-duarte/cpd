//! `cpd session retitle <session> --width <cols>` as hs-d543 amends it (commands.md §10.14): the input is the last 2
//! turns, the limit is N = cols - 8 characters (never cut, no word cap) and a width below 9 is a usage refusal. Run as
//! the built binary with a fake `pi` install whose Tier catalog is seeded in the cache. The helpers are copies of
//! those of `session_titles.rs`, cut down to one Lead.

use crate::common;

use std::fs;

use common::{Run, TestEnv, refused};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};

const LEAD: &str = "lead1";

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

fn fixture(env: &TestEnv, sql: &str) {
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    Store::open(&home, 5000).unwrap().write("fixture", |tx| exec(tx, sql)).unwrap();
}

/// The fake pi records its stdin, prints `answer` (a file the test writes) on stdout and exits 0.
const FAKE_PI: &str = r#"d="$(dirname "$0")/.."
cat > "$d/stdin.txt"
cat "$d/answer""#;

/// A session file of `messages`, each `(role, text)`, as pi writes them, with entries that are not messages of the
/// conversation between them.
fn session_file(env: &TestEnv, name: &str, messages: &[(&str, String)]) -> std::path::PathBuf {
    let mut lines = vec![json!({"type": "session", "id": name}).to_string()];
    for (role, text) in messages {
        lines.push(json!({"type": "message", "message": {"role": role, "content": [{"type": "text", "text": text}]}}).to_string());
        lines.push(json!({"type": "message", "message": {"role": "assistant", "content": [{"type": "toolCall", "name": "bash", "arguments": {}}]}}).to_string());
        lines.push(json!({"type": "message", "message": {"role": "toolResult", "content": [{"type": "text", "text": "TOOL OUTPUT"}]}}).to_string());
        lines.push(json!({"type": "custom_message", "customType": "cpd", "content": "CPD EVENT"}).to_string());
    }
    let path = env.root().join(format!("{name}.jsonl"));
    fs::write(&path, lines.join("\n") + "\n").unwrap();
    path
}

/// `turns` turns, each a `user` message `question <i>` and an `assistant` message `answer <i>`.
fn turns(turns: usize) -> Vec<(&'static str, String)> {
    (1..=turns).flat_map(|i| [("user", format!("question {i}")), ("assistant", format!("answer {i}"))]).collect()
}

/// `config.toml`, the fake pi install and its catalog, Context `ctx`, Plan `pay` and its Lead `lead1`, whose file holds
/// 5 turns.
fn setup(env: &TestEnv) {
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.fake_pi(FAKE_PI);
    answer(env, "TITLE: Dynamic session titles\n");
    fs::create_dir_all(env.root().join("install")).unwrap();
    fs::write(env.root().join("install/current-version"), "1.0.0\n").unwrap();
    let scope = env.root().join("install/releases/1.0.0/node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    let file = session_file(env, LEAD, &turns(5));
    fixture(
        env,
        &format!(
            "INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES
               ('1.0.0', 'O', 'p/opus', '[0,1,2,3,4,5]', 't'), ('1.0.0', 'S', 'p/sonnet', '[1,2,3,4,5]', 't'),
               ('1.0.0', 'H', 'p/haiku', '[1,2,3]', 't');
             INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
             INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, state, state_at, created_at, archived_at) VALUES
               ('lead1', 'lead', 'ctx', 'pay', NULL, 'Lead one', '{file}', '/', 'O3', 'idle', 't', 't1', NULL);
             INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pay', 'ctx', 'Pay', 'grounding', 'lead1', 't');",
            file = file.display(),
        ),
    );
}

/// What the fake pi answers next.
fn answer(env: &TestEnv, stdout: &str) {
    fs::write(env.root().join("answer"), stdout).unwrap();
    fs::write(env.root().join("status"), "0").unwrap();
}

fn retitle(env: &TestEnv, width: &str) -> Run {
    env.cpd(&["session", "retitle", LEAD, "--width", width])
}

fn show(env: &TestEnv) -> Value {
    env.cpd(&["session", "show", LEAD]).ok()["session"].clone()
}

fn pi_stdin(env: &TestEnv) -> String {
    fs::read_to_string(env.root().join("stdin.txt")).unwrap()
}

/// Nothing was written: no title and no `titled_at`.
fn nothing_written(env: &TestEnv) {
    let row = show(env);
    assert_eq!((&row["title"], &row["titled_at"]), (&Value::Null, &Value::Null), "{row}");
}

// ---------- the input ----------

#[test]
fn stdin_holds_only_the_last_two_turns() {
    let env = TestEnv::new();
    setup(&env);
    retitle(&env, "80").ok();
    let stdin = pi_stdin(&env);
    let found: Vec<usize> =
        (1..=5).filter(|i| stdin.contains(&format!("question {i}")) || stdin.contains(&format!("answer {i}"))).collect();
    assert_eq!(found, [4, 5], "only turns 4 and 5: {stdin}");
    let at = |text: &str| stdin.find(text).unwrap_or_else(|| panic!("{text:?} is in stdin: {stdin}"));
    let order = ["question 4", "answer 4", "question 5", "answer 5"].map(at);
    assert!(order.windows(2).all(|w| w[0] < w[1]), "in order: {stdin}");
    for other in ["TOOL OUTPUT", "CPD EVENT", "toolCall", "bash"] {
        assert!(!stdin.contains(other), "only user and assistant text, not {other:?}: {stdin}");
    }
}

// ---------- the limit ----------

#[test]
fn the_limit_is_the_width_less_8() {
    let env = TestEnv::new();
    setup(&env);
    let forty_two = "a".repeat(42);
    answer(&env, &format!("TITLE: {forty_two}\n"));
    let out = retitle(&env, "50").ok();
    assert_eq!(out["title"], json!(forty_two));
    assert_eq!(show(&env)["title"], json!(forty_two));
    let calls = env.calls("pi");
    assert_eq!(calls.len(), 1, "{calls:?}");
    let prompt = calls[0].last().unwrap();
    assert!(prompt.contains("42"), "the prompt names the limit, 50 - 8 = 42: {prompt}");

    // One character more is not a title: malformed, nothing written, never cut.
    let env = TestEnv::new();
    setup(&env);
    answer(&env, &format!("TITLE: {}\n", "a".repeat(43)));
    let error = refused(&retitle(&env, "50"), "title_failed", 1);
    assert_eq!(error["details"]["reason"], "malformed");
    nothing_written(&env);
}

#[test]
fn a_title_may_have_more_than_ten_words() {
    let env = TestEnv::new();
    setup(&env);
    let title = "Reviewing the retitle tests for the width limit after the last two turns ended";
    assert_eq!(title.split_whitespace().count(), 14);
    assert!(title.chars().count() <= 100 - 8);
    answer(&env, &format!("TITLE: {title}\n"));
    let out = retitle(&env, "100").ok();
    assert_eq!(out["title"], json!(title));
    assert_eq!(show(&env)["title"], json!(title));
}

// ---------- the width ----------

#[test]
fn a_width_below_9_is_refused() {
    let env = TestEnv::new();
    setup(&env);
    for width in ["8", "1", "0"] {
        refused(&retitle(&env, width), "usage", 2);
    }
    assert!(env.calls("pi").is_empty(), "refused before pi");
    nothing_written(&env);
}
