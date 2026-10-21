//! `context delete` (commands.md §3.14; contract §2.2 "Deleted", hs-d527), run as the built binary. The fixtures are
//! `plan_delete.rs`'s.

use crate::common;
use crate::plan_delete::{archive, base, count, events, fixture, plan_rows, world};

use common::{TestEnv, refused};

/// A Manager Session of Context `ctx` (contract §3.1): idle, in no pane.
fn manager(env: &TestEnv) {
    fixture(
        env,
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('m-ctx', 'manager', 'ctx', NULL, NULL, 'ctx-manager', '/sessions/m-ctx.jsonl', '/repo', 'O5', NULL, 'idle', NULL, 't', 't');",
    );
}

#[test]
fn deletes_the_context_its_plans_and_its_manager() {
    let env = TestEnv::new();
    base(&env);
    env.cpd(&["context", "new", "keep", "--title", "Kept", "--owner", "acme"]).ok();
    manager(&env);
    let (a, b) = (world(&env, "pay"), world(&env, "ship"));
    archive(&env, "pay");
    archive(&env, "ship");

    // the preview deletes nothing and counts everything
    let preview = env.cpd(&["context", "delete", "ctx"]).ok();
    assert_eq!(preview["preview"], true, "{preview}");
    assert_eq!(preview["plans"].as_array().map(Vec::len), Some(2), "{preview}");
    assert_eq!(preview["attempts"], 2, "{preview}");
    assert_eq!(preview["worktrees"], 4, "{preview}");
    assert!(plan_rows(&env, "pay") > 0 && a.run_dir.exists());

    let out = env.cpd(&["context", "delete", "ctx", "--yes"]).ok();
    assert_eq!((out["preview"].clone(), out["already"].clone()), (false.into(), false.into()), "{out}");

    for w in [&a, &b] {
        assert_eq!(plan_rows(&env, &w.plan), 0, "{}", w.plan);
        assert!(!w.run_dir.exists() && !w.lead_worktree.exists() && !w.attempt_worktree.exists(), "{}", w.plan);
    }
    assert_eq!(count(&env, "SELECT count(*) FROM sessions WHERE context = 'ctx'"), 0, "the Manager and every Session");
    assert_eq!(count(&env, "SELECT count(*) FROM context_records WHERE context = 'ctx'"), 0, "the Context's records");
    assert_ne!(env.query::<Option<String>>("SELECT deleted_at FROM contexts WHERE id = 'ctx'"), None, "the Context row is marked");
    assert_eq!(events(&env, "context-deleted").len(), 1);
    assert_eq!(env.store().tamper().unwrap(), Vec::<serde_json::Value>::new());

    // the other Context is untouched
    assert_eq!(env.query::<i64>("SELECT count(*) FROM contexts WHERE id = 'keep' AND deleted_at IS NULL"), 1);
    // the remote branches stay
    assert!(crate::plan_delete::remote_branches(&env, &a).contains(&a.lead_branch()));
    // idempotent
    assert_eq!(env.cpd(&["context", "delete", "ctx", "--yes"]).ok()["already"], true);
}

#[test]
fn refuses_when_a_plan_has_live_work() {
    let env = TestEnv::new();
    base(&env);
    manager(&env);
    let (a, b) = (world(&env, "pay"), world(&env, "ship"));
    archive(&env, "pay");
    archive(&env, "ship");
    // a Session of one Plan working: nothing is deleted, not even the quiet Plan
    fixture(&env, &format!("UPDATE sessions SET state = 'working' WHERE id = '{}'", b.lead));

    let error = refused(&env.cpd(&["context", "delete", "ctx", "--yes"]), "archive_blocked", 3);
    assert!(error["details"]["blockers"].as_array().is_some_and(|x| !x.is_empty()), "{error}");

    assert!(plan_rows(&env, "pay") > 0 && plan_rows(&env, "ship") > 0);
    assert!(a.run_dir.exists() && b.run_dir.exists() && a.lead_worktree.exists());
    assert_eq!(env.query::<i64>("SELECT count(*) FROM contexts WHERE id = 'ctx' AND deleted_at IS NULL"), 1);
}
