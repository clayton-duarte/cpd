//! Engine step E14 for a review Plan (contract §3.7 E14, §3.1 phase `done`; commands.md §31.1): a finished (`done`) Plan of
//! Context `codereview` is archived as soon as its Lead is `idle` or `stopped`, without waiting for sleep. The engine runs
//! in-process on virtual time, its `cpd pr poll` answered, with the fake tmux of the tmux route. The Leads here never slept
//! (`asleep_at` NULL): that is the point.

use crate::common;

use common::fx::Fx as Ip;

/// Ticks of the engine for "a later tick writes nothing more".
const LATER_TICKS: usize = 5;

struct Fx {
    env: Ip,
}

fn fx() -> Fx {
    let env = Ip::with_ports();
    env.engine_config_on_tmux("");
    env.engine_routes_on_tmux();
    let fx = Fx { env };
    fx.tmux_ok(&["new-session", "-d", "-s", "x", "sleep", "600"]);
    let pane = fx.pane("mgr-review");
    fx.fixture(&format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('codereview', 'Code review', 't', 'm1'), ('cpd', 'Cpd', 't', 'm1');
         INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, tmux_pane, created_at)
           VALUES ('mgr-review', 'manager', 'codereview', 'Mgr', '/', 'O3', 'idle', '{now}', '{pane}', 't');",
        now = fx.env.stamp_ago(0)
    ));
    fx
}

impl Fx {
    fn fixture(&self, sql: &str) {
        self.env.env.sql(sql);
    }

    fn tmux_ok(&self, args: &[&str]) -> String {
        let out = self.env.tmux(args);
        assert!(
            out.status.success(),
            "tmux {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    /// A live pane of Session `session` in a window of its own (session `x`).
    fn pane(&self, session: &str) -> String {
        let pane = self.tmux_ok(&[
            "new-window",
            "-d",
            "-t",
            "x",
            "-P",
            "-F",
            "#{pane_id}",
            "sleep",
            "600",
        ]);
        self.tmux_ok(&["set", "-p", "-t", &pane, "@cpd_session", session]);
        pane
    }

    /// Plan `plan` of `context` in `phase` (`done`: finished just now, no Workflow, no Job), its Lead `<plan>-lead` in
    /// `state` with a live pane, never asleep.
    fn plan(&self, context: &str, plan: &str, phase: &str, state: &str) -> String {
        let pane = self.pane(&format!("{plan}-lead"));
        // Finished just now: E13 (sleep) must not get there first.
        let ago = self.env.stamp_ago(0);
        let (locked, done) = if phase == "done" {
            (format!("'{ago}'"), format!("'{ago}'"))
        } else {
            ("NULL".into(), "NULL".into())
        };
        self.fixture(&format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, tmux_pane, created_at)
               VALUES ('{plan}-lead', 'lead', '{context}', '{plan}', 'Lead {plan}', '/', 'O3', '{state}', '{ago}', '{pane}', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
               VALUES ('{plan}', '{context}', '{plan}', '{phase}', '{plan}-lead', 't', {locked}, {done});"
        ));
        pane
    }

    fn run(&self, ticks: usize) {
        self.env.watch_ticks(ticks).unwrap();
    }

    fn set_state(&self, session: &str, state: &str) {
        self.fixture(&format!(
            "UPDATE sessions SET state = '{state}' WHERE id = '{session}';"
        ));
    }

    fn archived(&self, table: &str, id: &str) -> bool {
        self.env
            .env
            .query::<Option<String>>(&format!(
                "SELECT archived_at FROM {table} WHERE id = '{id}'"
            ))
            .is_some()
    }

    fn phase(&self, plan: &str) -> String {
        self.env
            .env
            .query::<String>(&format!("SELECT phase FROM plans WHERE id = '{plan}'"))
    }

    fn pane_alive(&self, pane: &str) -> bool {
        self.env.panes().iter().any(|p| p.pane == pane)
    }

    fn count(&self, sql: &str) -> i64 {
        self.env.env.query::<i64>(sql)
    }

    /// Every row the engine wrote: the Events and the archive marks.
    fn written(&self) -> (i64, i64) {
        (
            self.count("SELECT count(*) FROM events WHERE kind NOT LIKE 'watch-%'"),
            self.count(
                "SELECT (SELECT count(*) FROM plans WHERE archived_at IS NOT NULL) + (SELECT count(*) FROM sessions WHERE archived_at IS NOT NULL)",
            ),
        )
    }
}

#[test]
fn review_done_plan_is_archived_when_its_lead_is_idle() {
    let fx = fx();
    let pane = fx.plan("codereview", "rv", "done", "idle");
    fx.run(1);
    assert!(fx.archived("plans", "rv"), "the Plan is archived");
    assert!(fx.archived("sessions", "rv-lead"), "its Lead is archived");
    assert_eq!(fx.phase("rv"), "done");
    assert!(!fx.pane_alive(&pane), "the Lead's pane is closed");
}

#[test]
fn review_done_plan_waits_while_its_lead_is_working_then_is_archived() {
    let fx = fx();
    let pane = fx.plan("codereview", "rv", "done", "working");
    fx.run(LATER_TICKS);
    assert!(!fx.archived("plans", "rv"), "a working Lead keeps its Plan");
    assert!(fx.pane_alive(&pane), "its pane stays");
    fx.set_state("rv-lead", "idle");
    fx.run(1);
    assert!(fx.archived("plans", "rv"), "archived once the Lead is idle");
    assert!(fx.archived("sessions", "rv-lead"));
    assert!(!fx.pane_alive(&pane));
}

#[test]
fn review_done_plan_in_the_chat_slot_is_tried_again_next_tick() {
    let fx = fx();
    fx.env.cockpit();
    let pane = fx.plan("codereview", "rv", "done", "idle");
    // The Lead's pane takes the chat slot: it goes into the cockpit's `main` window in place of the placeholder.
    let placeholder = fx
        .env
        .panes()
        .into_iter()
        .find(|p| p.view == "chat")
        .expect("the chat placeholder")
        .pane;
    fx.tmux_ok(&["swap-pane", "-d", "-s", &pane, "-t", &placeholder]);
    fx.run(1);
    assert!(
        !fx.archived("plans", "rv"),
        "the Lead the User is looking at is kept"
    );
    assert!(fx.pane_alive(&pane));
    assert_eq!(
        fx.count("SELECT count(*) FROM events WHERE kind = 'error'"),
        0,
        "the refusal is no error Event"
    );
    // The pane leaves the chat slot: back to its window, the placeholder returns.
    fx.tmux_ok(&["swap-pane", "-d", "-s", &pane, "-t", &placeholder]);
    fx.run(1);
    assert!(fx.archived("plans", "rv"), "archived once out of the slot");
    assert!(!fx.pane_alive(&pane));
}

#[test]
fn review_done_plan_that_is_not_finished_is_kept() {
    let fx = fx();
    let job = |plan: &str, status: &str| {
        format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
               VALUES ('{plan}', 'repo', 'j', 'Title', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', 0, '{}', cpd_txn(), 't');",
            "a".repeat(64)
        )
    };
    // A cancelled Workflow, for the Jobs that need one.
    let workflow = |plan: &str| {
        format!(
            "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
               VALUES ('{plan}', 'repo', '/nowhere', 'main', 'cancelled', 'cpd/{plan}', NULL, cpd_txn(), 't', NULL);"
        )
    };
    // An open visible Inbox item of its Lead.
    let inbox = fx.plan("codereview", "inbox", "done", "idle");
    fx.fixture(
        "INSERT INTO inbox_items (kind, urgency, subject, context, plan, session, label, opened_at)
           VALUES ('dialog', 'red', 'inbox-lead', 'codereview', 'inbox', 'inbox-lead', 'cpd: question', 't');",
    );
    // A running Job.
    let running = fx.plan("codereview", "running", "done", "idle");
    fx.fixture(&workflow("running"));
    fx.fixture(&job("running", "running"));
    // An open Escalation.
    let escalated = fx.plan("codereview", "escalated", "done", "idle");
    fx.fixture(&format!(
        "{}
         INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, failed_cause, appended, fingerprint, created_txn, created_at)
           VALUES ('escalated', 'repo', 'j', 'T', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'failed', 'crashed', 0, 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', cpd_txn(), 't');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at)
           VALUES ('escalated', 'j-a1', 'j', 1, 'S2', 'ladder', 'ended', 'failed', 'crashed', 'cpd/escalated--j-a1', '/w', 't', 't');
         INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
           VALUES ('escalated', 'j-a1-q1', 'j-a1', 1, 'Why?', 'open', 4242, 't');",
        workflow("escalated")
    ));
    // A pending Plan review.
    let reviewed = fx.plan("codereview", "reviewed", "done", "idle");
    fx.fixture(&format!(
        "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at) VALUES ('reviewed', 'r1', 1, '{}', 'pending', 't');",
        "a".repeat(64)
    ));
    // Phase `grounding`.
    let grounding = fx.plan("codereview", "ground", "grounding", "idle");
    // Control: a finished Plan next to them is archived, so the engine did run E14 for the review Context.
    let control = fx.plan("codereview", "control", "done", "idle");

    fx.run(LATER_TICKS);
    assert!(
        fx.archived("plans", "control"),
        "control: the finished Plan is archived"
    );
    assert!(!fx.pane_alive(&control));
    for (plan, pane, why) in [
        ("inbox", inbox, "an open Inbox item"),
        ("running", running, "a running Job"),
        ("escalated", escalated, "an open Escalation"),
        ("reviewed", reviewed, "a pending Plan review"),
        ("ground", grounding, "phase grounding"),
    ] {
        assert!(!fx.archived("plans", plan), "{why} keeps its Plan");
        assert!(
            !fx.archived("sessions", &format!("{plan}-lead")),
            "{why} keeps its Lead"
        );
        assert!(fx.pane_alive(&pane), "{why} keeps its pane");
    }
}

#[test]
fn review_archive_never_messages_or_wakes_the_manager() {
    let fx = fx();
    let pane = fx.plan("codereview", "rv", "done", "idle");
    let manager = "SELECT state || '|' || state_at || '|' || coalesce(tmux_pane, '') || '|' || coalesce(asleep_at, '') || '|' || coalesce(archived_at, '') FROM sessions WHERE id = 'mgr-review'";
    let before: String = fx.env.env.query(manager);
    let rows = fx.count("SELECT count(*) FROM sessions");
    let panes = fx.env.panes().len();
    fx.run(1);
    assert!(fx.archived("plans", "rv"), "the Plan is archived");
    assert!(!fx.pane_alive(&pane));
    assert_eq!(
        fx.count("SELECT count(*) FROM events WHERE kind = 'session-message'"),
        0,
        "no session-message Event"
    );
    assert_eq!(fx.env.panes().len(), panes - 1, "no pane is started");
    assert_eq!(fx.count("SELECT count(*) FROM sessions"), rows);
    assert_eq!(fx.env.env.query::<String>(manager), before);
}

#[test]
fn done_plan_of_another_context_is_not_archived_before_it_sleeps() {
    let fx = fx();
    let pane = fx.plan("cpd", "pay", "done", "idle");
    fx.run(LATER_TICKS);
    assert!(!fx.archived("plans", "pay"));
    assert!(!fx.archived("sessions", "pay-lead"));
    assert!(fx.pane_alive(&pane));
}

#[test]
fn an_archived_review_plan_is_not_selected_again() {
    let fx = fx();
    fx.plan("codereview", "rv", "done", "idle");
    fx.run(LATER_TICKS);
    let after_first = fx.written();
    fx.run(LATER_TICKS);
    assert_eq!(fx.written(), after_first, "a later pass writes nothing");
}
