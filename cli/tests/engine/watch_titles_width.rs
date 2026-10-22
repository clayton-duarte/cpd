//! Engine step E10 passes the chat slot's width to `session retitle` (contract §3.7 E10, commands.md §10.14; hs-d543),
//! on a prepared database with a private tmux server. The width is seen where it has an effect: the prompt of the fake
//! `pi` names the limit N = width - 8. The helpers are cut down from those of `watch.rs` and `watch_sleep.rs`.

use crate::common;

use std::fs;
use std::process::{Child, Stdio};

use common::{CPD, TestEnv, wait_until};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::{Store, WriteTxn};
use time::OffsetDateTime;
use time::macros::format_description;

/// The chat slot's width, odd and large so that N = 115 occurs nowhere else in a prompt.
const WIDTH: u32 = 123;

/// A time `minutes` ago, as cpd writes times (contract §1.3).
fn ago(minutes: i64) -> String {
    (OffsetDateTime::now_utc() - time::Duration::minutes(minutes))
        .format(format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"))
        .unwrap()
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

struct Engine(Child);

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Fx {
    env: TestEnv,
}

/// A one-second-ish tick, a fake pi install with its Tier catalog, a fake `gh` that finds no pull requests, a fake `pi`
/// that answers a title, Context `ctx` and one idle Lead `due` with a live pane (session `x`) whose turn ended a minute
/// ago and that has no title: E10 retitles it as soon as there is a chat slot.
fn fx() -> Fx {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n[watch]\ntick_seconds = {}\n[tmux]\nsocket = \"{}\"\n",
            common::TICK_SECONDS,
            env.tmux_socket()
        ),
    )
    .unwrap();
    fs::create_dir_all(env.root().join("install")).unwrap();
    fs::write(env.root().join("install/current-version"), "1.0.0\n").unwrap();
    let scope = env.root().join("install/releases/1.0.0/node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    env.fake_pi("cat > /dev/null\necho 'TITLE: Width aware titles'");
    env.fake("cpd", &format!("exec '{CPD}' \"$@\""));
    env.fake_gh(r#"echo '{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}'"#);
    let out = env.tmux(&["new-session", "-d", "-s", "x", "-P", "-F", "#{pane_id}", "sleep", "600"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    let file = env.root().join("due.jsonl");
    let lines = [
        serde_json::json!({"type": "session", "id": "due"}),
        serde_json::json!({"type": "message", "message": {"role": "user", "content": "How do we title Sessions?"}}),
        serde_json::json!({"type": "message", "message": {"role": "assistant", "content": [{"type": "text", "text": "With an auxiliary agent."}]}}),
    ];
    fs::write(&file, lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n") + "\n").unwrap();
    let fx = Fx { env };
    fx.fixture(&format!(
        "INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES
           ('1.0.0', 'O', 'p/opus', '[0,1,2,3,4,5]', 't'), ('1.0.0', 'S', 'p/sonnet', '[1,2,3,4,5]', 't'),
           ('1.0.0', 'H', 'p/haiku', '[0,1,2,3]', 't');
         INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, name, file, cwd, tier, tmux_pane, state, state_at, created_at)
           VALUES ('due', 'manager', 'ctx', 'due', '{file}', '/', 'O3', '{pane}', 'idle', 't', 't');
         INSERT INTO events (txn, at, kind, context, session, text, data)
           VALUES (cpd_txn(), '{at}', 'session-state', 'ctx', 'due', 'x', '{{\"from\":\"working\",\"label\":null,\"to\":\"idle\"}}');",
        file = file.display(),
        at = ago(1)
    ));
    fx
}

impl Fx {
    fn fixture(&self, sql: &str) {
        let home = CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap();
        Store::open(&home, 5000).unwrap().write("fixture", |tx| exec(tx, sql)).unwrap();
    }

    fn start(&self) -> Engine {
        let child = self.env.command(CPD).arg("watch").envs([common::PR_POLL_EVERY_TICK]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let engine = Engine(child);
        wait_until("watch-started", || self.env.query::<i64>("SELECT count(*) FROM events WHERE kind = 'watch-started'") == 1);
        engine
    }

    /// The cockpit's `main` window, `WIDTH` columns wide, with one pane in it: the chat slot (the placeholder).
    fn chat_slot(&self) {
        let out = self.env.tmux(&["new-session", "-d", "-s", "cpd", "-n", "main", "-x", &WIDTH.to_string(), "-y", "30", "-P", "-F", "#{pane_id}", "sleep", "600"]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
        assert!(self.env.tmux(&["set", "-p", "-t", &pane, "@cpd_view", "chat"]).status.success());
        let width = self.env.tmux(&["display", "-p", "-t", &pane, "#{pane_width}"]);
        assert_eq!(String::from_utf8(width.stdout).unwrap().trim(), WIDTH.to_string(), "the slot is {WIDTH} columns wide");
    }
}

/// With a chat slot of `WIDTH` columns E10 retitles the due Session with `--width` = that width: the prompt names
/// N = 115.
#[test]
fn e10_passes_the_chat_slot_width_to_retitle() {
    let fx = fx();
    fx.chat_slot();
    let _engine = fx.start();
    wait_until("the Session is retitled", || fx.env.query::<i64>("SELECT count(*) FROM events WHERE kind = 'session-titled'") == 1);
    let calls = fx.env.calls("pi");
    assert_eq!(calls.len(), 1, "{calls:?}");
    let prompt = calls[0].last().unwrap();
    assert!(prompt.contains(&(WIDTH - 8).to_string()), "the prompt names the limit, {WIDTH} - 8: {prompt}");
}

/// With no chat slot pane in the cockpit E10 starts no retitle: after whole ticks there is no run and no pi call, and
/// the Session stays due: when the slot appears, it is retitled.
#[test]
fn e10_starts_no_retitle_without_a_chat_slot() {
    let fx = fx();
    // The cockpit exists but its `main` window has no chat slot: no pane with `@cpd_session` or `@cpd_view` `chat`/`history`.
    let out = fx.env.tmux(&["new-session", "-d", "-s", "cpd", "-n", "main", "sleep", "600"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let engine = fx.start();
    fx.env.wait_whole_ticks(3);
    assert!(fx.env.calls("pi").is_empty(), "no retitle without a chat slot: {:?}", fx.env.calls("pi"));
    assert!(!fx.env.cpd_home().join("run/titles").join("due.log").exists(), "no run was started");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM events WHERE kind = 'session-titled'"), 0);
    assert_eq!(fx.env.query::<Option<String>>("SELECT titled_at FROM sessions WHERE id = 'due'"), None);

    // The Session stayed due: with the slot in place it is retitled.
    let out = fx.env.tmux(&["list-panes", "-t", "cpd:main", "-F", "#{pane_id}"]);
    let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    assert!(fx.env.tmux(&["set", "-p", "-t", &pane, "@cpd_view", "chat"]).status.success());
    wait_until("the Session is retitled once the slot exists", || {
        fx.env.query::<i64>("SELECT count(*) FROM events WHERE kind = 'session-titled'") == 1
    });
    drop(engine);
}
