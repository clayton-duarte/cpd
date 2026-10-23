//! Archive on a Context row whose Manager holds the chat slot (commands.md §13.2 "Archive of the chat slot's Lead", hs-d90):
//! the tree frees the slot (opens the next live Session, else `session park`), then runs `context archive`. End to end, as
//! `tui_tree.rs` does (its helpers are private, so the few needed are repeated here): a pane of a private tmux server over a
//! prepared store, a fake tmux whose popup confirms, and a fake `cpd` that records its calls and keeps the CLI's refusal:
//! `context archive` fails with `in_chat_slot` until a `session open` or `session park` ran before it.

use crate::common;

use common::{CPD, TestEnv, sql};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use nix::fcntl::{Flock, FlockArg};

const SESSION: &str = "tree";

fn env() -> TestEnv {
    let env = TestEnv::new();
    let config = format!(
        "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n[events]\npoll_ms = 50\n[tmux]\nsocket = \"{}\"\n",
        env.tmux_socket()
    );
    std::fs::write(env.cpd_home().join("config.toml"), config).unwrap();
    let path = env.cpd_home().join("locks/watch.lock");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lock = Flock::lock(std::fs::File::create(path).unwrap(), FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap();
    std::mem::forget(lock);
    env
}

fn session(id: &str, role: &str, context: &str, plan: Option<&str>, created: &str) -> String {
    let plan = plan.map_or("NULL".to_owned(), |p| format!("'{p}'"));
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('{id}', '{role}', '{context}', {plan}, NULL, 'name of {id}', '/sessions/{id}.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                 '2026-01-03T00:00:00.000Z', '{created}');"
    )
}

fn context(id: &str, plan: &str, lead: &str) -> String {
    format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('{id}', 'Title of {id}', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('{plan}', '{id}', 'Plan {plan}', 'grounding', '{lead}', '2026-01-02T00:00:00.000Z');"
    )
}

/// Context `alpha` (Plan `ship`, Lead `l-ship`) whose Manager `m-alpha` holds the chat slot; with `with_beta`, a Context `beta`
/// after it (Plan `rel`, Lead `l-rel`): the next live Session.
fn populate(env: &TestEnv, with_beta: bool) {
    let mut store = Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap();
    let mut s = context("alpha", "ship", "l-ship");
    s += &session("l-ship", "lead", "alpha", Some("ship"), "2026-01-02T00:00:00.000Z");
    s += &session("m-alpha", "manager", "alpha", None, "2026-01-02T00:00:07.000Z");
    if with_beta {
        s += &context("beta", "rel", "l-rel");
        s += &session("l-rel", "lead", "beta", Some("rel"), "2026-01-02T00:00:02.000Z");
    }
    s += "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
          VALUES (cpd_txn(), '2026-01-03T00:00:00.000Z', 'session-moved', 'alpha', NULL, 'm-alpha', 'fixture',
                  '{\"pane\":\"%1\",\"window\":\"@1\",\"place\":\"chat\"}');";
    sql(&mut store, &s);
}

/// The fake `cpd`: `context archive` is refused with `in_chat_slot` unless `session open` or `session park` ran first; its
/// success leaves `archived` in the env's root.
fn fake_cpd(env: &TestEnv) {
    let root = env.root().display().to_string();
    env.fake(
        "cpd",
        &format!(
            "case \"$1 $2\" in\n\
             \"session open\"|\"session park\") touch '{root}/freed'; echo '{{\"ok\":true}}';;\n\
             \"context archive\")\n\
               if [ -e '{root}/freed' ]; then touch '{root}/archived'; echo '{{\"ok\":true}}';\n\
               else echo '{{\"ok\":false,\"error\":{{\"code\":\"in_chat_slot\",\"message\":\"Lead {manager} is in the chat slot\",\"details\":{{}}}}}}'; exit 3; fi;;\n\
             *) echo '{{\"ok\":true}}';;\n\
             esac",
            manager = "m-alpha"
        ),
    );
}

/// A fake tmux with a client attached whose `display-popup` confirms (writes the answer to the popup's `--out` file).
fn fake_confirming_dialogs(env: &TestEnv) {
    std::fs::write(env.root().join("dialog-answer.json"), r#"{"confirmed":true}"#).unwrap();
    let real = String::from_utf8(env.command("sh").args(["-c", "command -v tmux"]).output().unwrap().stdout).unwrap();
    env.fake(
        "tmux",
        &format!(
            "case \"$*\" in\n *display-popup*) for _c; do :; done\n out=$(printf '%s' \"$_c\" | sed -n \"s/.*--out='\\([^']*\\)'.*/\\1/p\")\n cat '{answer}' > \"$out\";;\n *list-clients*) echo '100 200 60';;\n *) exec {real} \"$@\";;\nesac",
            answer = env.root().join("dialog-answer.json").display(),
            real = real.trim()
        ),
    );
}

fn screen(env: &TestEnv) -> Vec<String> {
    let out = env.tmux(&["capture-pane", "-p", "-t", SESSION]);
    String::from_utf8(out.stdout).unwrap().lines().map(|l| l.trim_end().to_owned()).collect()
}

fn type_text(env: &TestEnv, text: &str) {
    assert!(env.tmux(&["send-keys", "-t", SESSION, "-l", text]).status.success());
}

fn mouse(env: &TestEnv, button: u8, column: u16, row: u16) {
    for end in ['M', 'm'] {
        type_text(env, &format!("\x1b[<{button};{};{}{end}", column + 1, row + 1));
    }
}

fn log(env: &TestEnv) -> String {
    std::fs::read_to_string(env.cpd_home().join("run/tui/tree.log")).unwrap_or_default()
}

fn words(w: &[&str]) -> Vec<String> {
    w.iter().map(|s| (*s).to_owned()).collect()
}

/// Archive on the Context `alpha`'s row, confirmed; returns once its `context archive` call has been made.
fn archive_alpha(env: &TestEnv) {
    let out = env.tmux(&[
        "-u", "set-option", "-g", "remain-on-exit", "on", ";", "new-session", "-d", "-s", SESSION, "-x", "60", "-y", "14",
        &format!("{CPD} tui view tree"),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    common::wait_screen("the tree", || screen(env), |s| s.iter().any(|r| r.contains("alpha")));
    mouse(env, 2, 5, 1); // right click on the Context `alpha`: Archive is the fourth item of its menu
    common::wait_screen("the menu", || screen(env), |s| s.iter().any(|r| r.contains("New Plan…")));
    mouse(env, 0, 7, 5);
    common::wait_some("the archive call", common::WAIT, || {
        let calls = env.calls("cpd");
        if calls.iter().any(|c| c[..2] == words(&["context", "archive"])[..]) { Ok(()) } else { Err(format!("got {calls:?}, log: {}", log(env))) }
    });
}

#[test]
fn context_archive_frees_the_slot_held_by_its_manager() {
    let env = env();
    populate(&env, true);
    fake_cpd(&env);
    fake_confirming_dialogs(&env);
    archive_alpha(&env);
    assert_eq!(
        env.calls("cpd"),
        [words(&["session", "open", "l-rel"]), words(&["context", "archive", "alpha"])],
        "the next live Session after the Context, then the archive; log: {}",
        log(&env)
    );
    common::wait_until("the Context to be archived", || env.root().join("archived").exists());
    assert!(env.root().join("archived").exists(), "the Context ends archived (not refused in_chat_slot); log: {}", log(&env));
}

#[test]
fn context_archive_parks_when_no_other_session_is_live() {
    let env = env();
    populate(&env, false);
    fake_cpd(&env);
    fake_confirming_dialogs(&env);
    archive_alpha(&env);
    assert_eq!(
        env.calls("cpd"),
        [words(&["session", "park"]), words(&["context", "archive", "alpha"])],
        "no other live Session: the slot is parked, then the archive; log: {}",
        log(&env)
    );
    common::wait_until("the Context to be archived", || env.root().join("archived").exists());
    assert!(env.root().join("archived").exists(), "the Context ends archived (not refused in_chat_slot); log: {}", log(&env));
}
