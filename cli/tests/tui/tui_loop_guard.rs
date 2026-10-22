//! The `loop-guard` Inbox item in the tree view (commands.md §13.2, §21; hs-d552, hs-d571), end to end: the built binary
//! runs in a pane of a private tmux server over a prepared store. The click and the dot are unit-tested in
//! `tui/src/tree/inbox.rs`.

use crate::common;

use common::{CPD, TestEnv, sql};
use cpd_core::home::CpdHome;
use cpd_core::ids::{ContextId, PlanId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::store::Store;
use cpd_tui::palette;
use nix::fcntl::{Flock, FlockArg};

/// An env with an engine running (`engine-down` is no row of the Inbox: its lock is held until the process ends).
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

/// Context `alpha` with its Plan `sketch`, whose Lead `l-sketch` is blocked by the loop guard, and the open `kind` item the
/// engine's E8 would open for it: red, linked to that Session.
fn tripped(env: &TestEnv, kind: Kind) {
    let mut store = Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap();
    sql(
        &mut store,
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('alpha', 'Title of alpha', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
         VALUES ('sketch', 'alpha', 'Plan sketch', 'plan', 'l-sketch', '2026-01-02T00:00:01.000Z', NULL, NULL);
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                          pull_request, created_txn, created_at, started_at, merged_at)
         VALUES ('sketch', 'repo', '/repo/sketch', 'main', 'waiting', 'cpd/sketch', NULL, NULL, NULL, NULL,
                 NULL, cpd_txn(), '2026-01-02T00:00:01.000Z', NULL, NULL);
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('l-sketch', 'lead', 'alpha', 'sketch', NULL, 'sketch', '/sessions/l-sketch.jsonl', '/repo', 'O3',
                 NULL, 'blocked', 'cpd: loop guard', '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:01.000Z');",
    );
    let item = Condition {
        kind,
        urgency: Urgency::Red,
        subject: "session:l-sketch".into(),
        label: "sketch: loop guard tripped".into(),
        url: None,
        context: Some(ContextId::parse("alpha").unwrap()),
        plan: Some(PlanId::parse("sketch").unwrap()),
        job: None,
        session: Some(SessionId::parse("l-sketch").unwrap()),
        record: None,
        event: None,
    };
    reconcile(&mut store, &[item], "2026-01-03T00:00:00.000Z").expect("the store holds the item");
}

/// `cpd tui view tree` in a 60 x 14 pane of the env's private tmux server; the screen, with its escapes, once `done` holds of
/// its plain rows.
fn screen_when(env: &TestEnv, done: impl FnMut(&[String]) -> bool) -> String {
    let out = env.tmux(&["-u", "set-option", "-g", "remain-on-exit", "on", ";", "new-session", "-d", "-s", "tree", "-x", "60", "-y", "14", &format!("{CPD} tui view tree")]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let capture = |flags: &[&str]| {
        let mut args = vec!["capture-pane", "-p"];
        args.extend_from_slice(flags);
        args.extend_from_slice(&["-t", "tree"]);
        String::from_utf8(env.tmux(&args).stdout).unwrap()
    };
    common::wait_screen("the tree", || capture(&[]).lines().map(|l| l.trim_end().to_owned()).collect(), done);
    capture(&["-e"])
}

#[test]
fn a_loop_guard_item_is_drawn_red_with_its_label() {
    let env = env();
    tripped(&env, Kind::LoopGuard);
    let row = format!("│ {} sketch: loop guard tripped", palette::TIER_RED);
    screen_when(&env, |rows| rows.iter().any(|l| l.contains(&row)));
}

#[test]
fn a_loop_guard_item_fills_its_sessions_tree_dot_red() {
    let env = env();
    tripped(&env, Kind::LoopGuard);
    let styled = screen_when(&env, |rows| rows.iter().any(|l| l.contains(&format!("{} sketch", palette::NEEDS_YOU))));
    let row = styled.lines().find(|l| l.contains("sketch") && !l.contains("loop guard")).expect("the Lead's row");
    assert!(row.contains(&format!("\x1b[38;5;9m{}", palette::NEEDS_YOU)), "the filled dot in red (colour 9): {row:?}");
}
