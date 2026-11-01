//! Engine step E13, Sleep (contract §3.7, §2.2 "Asleep", §8.2 `timeouts.sleep_seconds`; hs-d515, hs-d520), in-process on a
//! prepared database with the fake tmux server of the tmux route. Every test has a control: a finished Lead that must go to
//! sleep, so that "stays awake" is asserted after the engine has shown it can put a Session to sleep.

use crate::common;

use common::fx::Fx as Ip;

/// Ticks of the engine for "a later tick writes nothing more": three seconds of virtual time (a nap is a tenth of a second).
const LATER_TICKS: usize = 30;

/// Contexts `ctx` and `other`, `timeouts.sleep_seconds = 1`, a tmux server with one live pane (session `x`) and the engine
/// in-process on virtual time, its `cpd pr poll` answered.
struct Fx {
    env: Ip,
    pane: String,
}

fn fx() -> Fx {
    let env = Ip::with_ports();
    env.engine_config_on_tmux("[timeouts]\nsleep_seconds = 1\n");
    env.engine_routes_on_tmux();
    let out = env.tmux(&["new-session", "-d", "-s", "x", "-P", "-F", "#{pane_id}", "sleep", "600"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    let fx = Fx { env, pane };
    fx.fixture(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1'), ('other', 'Other', 't', 'm1');",
    );
    fx
}

/// Two hours before the virtual clock's now, as cpd writes times (contract §1.3).
fn ago(fx: &Fx) -> String {
    fx.env.stamp_ago(7_200_000)
}

impl Fx {
    fn fixture(&self, sql: &str) {
        self.env.env.sql(sql);
    }

    /// Plan `plan` of `context`, in `phase` (`done`: finished two hours ago), its Lead `<plan>-lead` (`stopped`, no pane,
    /// since two hours) and one `merged` PR.
    fn plan(&self, context: &str, plan: &str, phase: &str) {
        let (done_at, locked_at) = (if phase == "done" { format!("'{}'", ago(self)) } else { "NULL".into() }, "'t'");
        self.fixture(&format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('{plan}-lead', 'lead', '{context}', '{plan}', 'Lead {plan}', '/', 'O3', 'stopped', '{at}', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
               VALUES ('{plan}', '{context}', '{plan}', '{phase}', '{plan}-lead', 't', {locked_at}, {done_at});
             {pull}
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at, merged_at)
               VALUES ('{plan}', 'repo', '/nowhere', 'main', '{pr}', 'cpd/{plan}', '/nowhere/lead', {number}, {url}, {opened}, {pull_request}, cpd_txn(), 't', 't', {merged});",
            at = ago(self),
            pull = if phase == "done" {
                format!(
                    "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                     VALUES ('acme/{plan}', 7, 'https://github.com/acme/x/pull/7', 'PR', 0, 'merged', 'none', 'none', 't', 't', 't');"
                )
            } else {
                String::new()
            },
            pr = if phase == "done" { "merged" } else { "dispatch" },
            pull_request = if phase == "done" { format!("(SELECT id FROM pull_requests WHERE repo = 'acme/{plan}')") } else { "NULL".into() },
            url = if phase == "done" { "'https://github.com/acme/x/pull/7'" } else { "NULL" },
            opened = if phase == "done" { "'t'" } else { "NULL" },
            number = if phase == "done" { "7" } else { "NULL" },
            merged = if phase == "done" { format!("'{}'", ago(self)) } else { "NULL".into() }
        ));
    }

    fn manager(&self, context: &str) {
        self.fixture(&format!(
            "INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
               VALUES ('mgr-{context}', 'manager', '{context}', 'Mgr', '/', 'O3', 'stopped', '{}', 't');",
            ago(self)
        ));
    }

    /// The engine runs `ticks` ticks of virtual time, in this process.
    fn run(&self, ticks: usize) {
        self.env.watch_ticks(ticks).unwrap();
    }

    fn asleep(&self, session: &str) -> bool {
        self.env.env.query::<Option<String>>(&format!("SELECT asleep_at FROM sessions WHERE id = '{session}'")).is_some()
    }

    /// The Sessions E13 put to sleep, in order.
    fn slept(&self) -> Vec<String> {
        self.env.env.read(|conn| {
            let mut stmt = conn.prepare("SELECT session FROM events WHERE kind = 'session-asleep' ORDER BY id")?;
            stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()
        })
    }
}

#[test]
fn done_plan_lead_sleeps_after_the_timeout() {
    let fx = fx();
    fx.plan("ctx", "pay", "done");
    fx.run(1);
    assert!(fx.asleep("pay-lead"));
    assert_eq!(fx.slept(), ["pay-lead"]);
    let row: (String, String, Option<String>) = fx
        .env
        .env
        .read(|c| c.query_row("SELECT state, asleep_at, archived_at FROM sessions WHERE id = 'pay-lead'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))));
    assert_eq!((row.0.as_str(), row.2.as_deref()), ("stopped", None), "asleep is no state and no archive: {row:?}");

    // Idempotent: a later tick writes nothing more.
    fx.run(LATER_TICKS);
    assert_eq!(fx.slept(), ["pay-lead"]);
}

#[test]
fn lead_with_an_open_inbox_item_stays_awake() {
    let fx = fx();
    fx.plan("other", "ctl", "done");
    fx.plan("ctx", "pay", "done");
    // `pay-lead` is blocked in a live pane: E8 opens a dialog item for it, a filled dot.
    fx.fixture(&format!(
        "UPDATE sessions SET tmux_pane = '{}', state = 'blocked', state_label = 'cpd: question', state_at = '{}' WHERE id = 'pay-lead';",
        fx.pane,
        ago(&fx)
    ));
    fx.run(1);
    assert!(fx.asleep("ctl-lead"));
    fx.run(LATER_TICKS);
    assert!(!fx.asleep("pay-lead"), "an open Inbox item keeps its Session awake");
    assert_eq!(fx.slept(), ["ctl-lead"]);
}

#[test]
fn lead_with_a_live_or_draft_job_stays_awake() {
    let fx = fx();
    fx.plan("other", "ctl", "done");
    fx.plan("ctx", "draft", "done");
    fx.plan("ctx", "queued", "done");
    let job = |plan: &str, status: &str| {
        format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
               VALUES ('{plan}', 'repo', 'j', 'Title', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', 0, {fp}, cpd_txn(), 't');",
            fp = if status == "draft" { "NULL".to_owned() } else { format!("'{}'", "a".repeat(64)) }
        )
    };
    fx.fixture(&job("draft", "draft"));
    fx.fixture(&job("queued", "queued"));
    fx.run(1);
    assert!(fx.asleep("ctl-lead"));
    fx.run(LATER_TICKS);
    assert!(!fx.asleep("draft-lead") && !fx.asleep("queued-lead"), "a draft or queued Job keeps its Lead awake");
    assert_eq!(fx.slept(), ["ctl-lead"]);
}

#[test]
fn lead_in_the_chat_slot_stays_awake() {
    let fx = fx();
    fx.plan("other", "ctl", "done");
    fx.plan("ctx", "pay", "done");
    // The cockpit session is `cpd`: its `main` window holds `pay-lead`'s pane, the chat slot.
    let out = fx.env.tmux(&["new-session", "-d", "-s", "cpd", "-n", "main", "-P", "-F", "#{pane_id}", "sleep", "600"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    assert!(fx.env.tmux(&["set", "-p", "-t", &pane, "@cpd_session", "pay-lead"]).status.success());
    fx.fixture(&format!("UPDATE sessions SET tmux_pane = '{pane}', state = 'idle', state_at = '{}' WHERE id = 'pay-lead';", ago(&fx)));
    fx.run(1);
    assert!(fx.asleep("ctl-lead"));
    fx.run(LATER_TICKS);
    assert!(!fx.asleep("pay-lead"), "the Lead the User is looking at stays awake");
    assert_eq!(fx.slept(), ["ctl-lead"]);
}

#[test]
fn manager_sleeps_when_every_plan_is_done() {
    let fx = fx();
    fx.plan("ctx", "plan-a", "done");
    fx.plan("ctx", "plan-b", "done");
    fx.manager("ctx");
    fx.run(1);
    assert!(fx.asleep("mgr-ctx"));
    let slept = fx.slept();
    assert!(slept.contains(&"mgr-ctx".to_owned()), "{slept:?}");
    assert_eq!(slept.iter().filter(|s| *s == "mgr-ctx").count(), 1, "put to sleep once");
}

#[test]
fn manager_with_a_plan_not_done_stays_awake() {
    let fx = fx();
    fx.plan("other", "ctl", "done");
    fx.manager("other");
    fx.plan("ctx", "plan-a", "done");
    fx.plan("ctx", "plan-b", "dispatch");
    fx.manager("ctx");
    fx.run(1);
    assert!(fx.asleep("mgr-other"));
    fx.run(LATER_TICKS);
    assert!(!fx.asleep("mgr-ctx"), "a Plan in dispatch keeps the Manager awake");
}
