//! Engine step E8, the `loop-guard` Inbox item (contract §2.2 "Inbox item", §3.7; hs-d552, hs-d571): a Lead, fork or
//! Manager Session `blocked` with the label `cpd: loop guard` is one red item, whatever its Plan's phase and never
//! also a `dialog`, closed when the Session leaves that state.

use crate::common;

use std::fs;
use std::process::{Child, Stdio};
use std::time::Duration;

use common::{CPD, TestEnv, wait_until};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};

const GUARD: &str = "cpd: loop guard";

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

/// A running `cpd watch`, SIGTERMed on drop (killed if it does not stop).
struct Engine(Child);

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(self.0.id() as i32), nix::sys::signal::Signal::SIGTERM);
        common::poll(Duration::from_secs(10), || (!matches!(self.0.try_wait(), Ok(None))).then_some(()));
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A prepared store: Context `ctx`, Plan `pay` in `dispatch` with its Lead Session `lead1` (`working`, in a live tmux
/// pane so that E2 leaves it alone) and a Manager Session `mgr1` (`idle`, same pane); a tick of `common::TICK_SECONDS`.
struct Fx {
    env: TestEnv,
}

fn fx(extra_config: &str) -> Fx {
    fx_in("dispatch", extra_config)
}

/// As [`fx`], with Plan `pay` in `phase` (`grounding` is not locked).
fn fx_in(phase: &str, extra_config: &str) -> Fx {
    let locked = if phase == "grounding" { "NULL" } else { "'t'" };
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n{extra_config}[watch]\ntick_seconds = {}\n[tmux]\nsocket = \"{}\"\n",
            common::TICK_SECONDS,
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.fake("cpd", &format!("exec '{CPD}' \"$@\""));
    env.fake_gh(r#"echo '{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}'"#);
    let out = env.tmux(&["new-session", "-d", "-s", "x", "-P", "-F", "#{pane_id}", "sleep", "600"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    let fx = Fx { env };
    fx.fixture(&format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, tmux_pane, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', '{pane}', 'working', 't', 't');
         INSERT INTO sessions (id, role, context, name, cwd, tier, tmux_pane, state, state_at, created_at)
           VALUES ('mgr1', 'manager', 'ctx', 'Mgr', '/', 'O3', '{pane}', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', '{phase}', 'lead1', 't', {locked});
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '/nowhere', 'main', 'dispatch', 'cpd/pay', '/nowhere/lead', cpd_txn(), 't', 't');"
    ));
    fx
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap(), 5000).unwrap()
    }

    fn fixture(&self, sql: &str) {
        self.store().write("fixture", |tx| exec(tx, sql)).unwrap();
    }

    fn start(&self) -> Engine {
        let child = self.env.command(CPD).arg("watch").envs([common::PR_POLL_EVERY_TICK]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let engine = Engine(child);
        wait_until("watch-started", || self.env.query::<i64>("SELECT count(*) FROM events WHERE kind = 'watch-started'") == 1);
        engine
    }

    /// The open items: `(kind, urgency, session, label)`.
    fn open_items(&self) -> Vec<(String, String, Option<String>, String)> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare("SELECT kind, urgency, session, label FROM inbox_items WHERE closed_at IS NULL ORDER BY id")?;
            Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    fn events(&self, kind: &str) -> Vec<Value> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare("SELECT data FROM events WHERE kind = ?1 ORDER BY id")?;
            let rows = stmt.query_map([kind], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows.iter().map(|d| serde_json::from_str(d).unwrap()).collect())
        })
    }

    fn guard(&self, session: &str) {
        self.fixture(&format!("UPDATE sessions SET state = 'blocked', state_label = '{GUARD}' WHERE id = '{session}';"));
    }

    fn item(&self, kind: &str) -> Option<(String, String, Option<String>, String)> {
        self.open_items().into_iter().find(|i| i.0 == kind)
    }
}

/// A `blocked` Lead with the label is one open red `loop-guard` item with the label, linked to its Session and no `dialog`.
#[test]
fn e8_opens_a_red_loop_guard_item_for_a_blocked_lead_with_the_label() {
    let fx = fx("");
    let _engine = fx.start();
    fx.guard("lead1");
    wait_until("the loop-guard item", || fx.item("loop-guard").is_some());
    assert_eq!(fx.open_items(), [("loop-guard".into(), "red".into(), Some("lead1".into()), "pay: loop guard, 21 turns in 10 min".into())]);
    let id: i64 = fx.env.query("SELECT id FROM inbox_items WHERE kind = 'loop-guard'");
    assert_eq!(fx.events("inbox-opened"), [json!({"item": id, "kind": "loop-guard", "urgency": "red", "label": "pay: loop guard, 21 turns in 10 min"})]);
    assert_eq!(fx.env.query::<String>("SELECT plan || '/' || session FROM inbox_items WHERE kind = 'loop-guard'"), "pay/lead1");
    assert!(fx.item("dialog").is_none());
    assert_eq!(fx.events("error"), Vec::<Value>::new());
}

/// The item is red in `grounding` and in `dispatch` (a dialog is magenta in `grounding`), and is never a dialog.
#[test]
fn e8_loop_guard_item_is_red_in_every_phase_and_is_no_dialog() {
    for phase in ["grounding", "dispatch"] {
        let fx = fx_in(phase, "");
        let _engine = fx.start();
        fx.guard("lead1");
        wait_until("the loop-guard item", || fx.item("loop-guard").is_some());
        assert_eq!(fx.item("loop-guard").unwrap().1, "red", "{phase}");
        assert_eq!(fx.open_items().len(), 1, "{phase}: {:?}", fx.open_items());
        assert!(fx.item("dialog").is_none(), "{phase}");
    }
}

/// Leaving `blocked` (to `working`, `idle` or `stopped`) closes the item with `inbox-closed`.
#[test]
fn e8_closes_the_loop_guard_item_when_the_session_leaves_blocked() {
    let fx = fx("");
    let _engine = fx.start();
    let mut closed = 0;
    for state in ["working", "idle", "stopped"] {
        fx.guard("lead1");
        wait_until("the item", || fx.item("loop-guard").is_some());
        let id: i64 = fx.env.query("SELECT id FROM inbox_items WHERE kind = 'loop-guard' AND closed_at IS NULL");
        fx.fixture(&format!("UPDATE sessions SET state = '{state}', state_label = NULL WHERE id = 'lead1';"));
        wait_until("the item to close", || fx.open_items().is_empty());
        closed += 1;
        let events = fx.events("inbox-closed");
        assert_eq!(events.len(), closed, "{state}: {events:?}");
        assert_eq!(events.last().unwrap(), &json!({"item": id, "kind": "loop-guard"}), "{state}");
        assert!(fx.env.query::<Option<String>>(&format!("SELECT closed_at FROM inbox_items WHERE id = {id}")).is_some());
    }
}

/// `loop_guard_turns = 3` makes the label say `4 turns`; a Manager's label starts with its Context id.
#[test]
fn e8_loop_guard_label_follows_loop_guard_turns_and_names_a_manager_by_its_context() {
    let fx = fx("loop_guard_turns = 3\n");
    let _engine = fx.start();
    fx.guard("lead1");
    wait_until("the lead's item", || fx.item("loop-guard").is_some());
    assert_eq!(fx.item("loop-guard").unwrap().3, "pay: loop guard, 4 turns in 10 min");

    fx.guard("mgr1");
    wait_until("both items", || fx.open_items().iter().filter(|i| i.0 == "loop-guard").count() == 2);
    let manager = fx.open_items().into_iter().find(|i| i.2.as_deref() == Some("mgr1")).unwrap();
    assert_eq!((manager.0.as_str(), manager.1.as_str(), manager.3.as_str()), ("loop-guard", "red", "ctx: loop guard, 4 turns in 10 min"));
    assert!(fx.item("dialog").is_none(), "{:?}", fx.open_items());
}
