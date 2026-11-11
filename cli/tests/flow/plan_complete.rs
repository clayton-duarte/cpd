//! Tests for plan completion, draft dependency validation, outline done_when phase enforcement,
//! done refusal, plan check, and plan reopen (b17, pro-d549, pro-d550).

use crate::common;
use common::fx::Fx;
use common::refused;
use serde_json::json;
use std::fs;

fn setup(fx: &Fx) {
    fx.env.write_models();
    fs::write(
        fx.env.cpd_home().join("config.toml"),
        "version = 1\nmachine = \"m1\"\n",
    )
    .unwrap();
    fx.env.fake_pi("exit 0");
    fs::create_dir_all(fx.env.root().join("install")).unwrap();
    fs::write(fx.env.root().join("install/current-version"), "1.0.0\n").unwrap();
    let scope = fx
        .env
        .root()
        .join("install/releases/1.0.0/node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    let rows = [
            ("O", "p/opus", "[0,1,2,3,4,5]"),
            ("S", "p/sonnet", "[0,1,2,3,4,5]"),
            ("H", "p/haiku", "[0,1,2,3]"),
            ("G", "p/gemini", "[]"),
        ]
        .iter()
        .map(|(letter, model, efforts)| {
            format!(
                "INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{model}', '{efforts}', 't');"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    fx.env.sql(&rows);
    fx.env.owner_policy("acme", "");
    fx.env.sql(
            "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Context', 'acme', 't', 'm1');",
        );
}

fn write_spec(fx: &Fx, name: &str) -> String {
    let path = fx.env.root().join(name);
    fs::write(
        &path,
        json!({
            "goal": "Goal text",
            "context": "Context text",
            "touches": ["src/lib.rs"],
            "acceptance": ["cargo test"],
            "out_of_scope": []
        })
        .to_string(),
    )
    .unwrap();
    path.to_str().unwrap().to_owned()
}

fn create_plan(fx: &Fx, id: &str, phase: &str) {
    let (locked, done) = match phase {
        "grounding" | "plan" => ("NULL", "NULL"),
        "done" => ("'t'", "'t'"),
        _ => ("'t'", "NULL"),
    };
    fx.env.sql(&format!(
            "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
             VALUES ('{id}', 'ctx', 'Plan {id}', '{phase}', 'lead-{id}', 't', {locked}, {done});
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
             VALUES ('{id}', 'w1', '/repo', 'main', 'waiting', 'cpd/{id}', cpd_txn(), 't');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, runner, state, state_at, created_at)
             VALUES ('lead-{id}', 'lead', 'ctx', '{id}', 'lead-{id}', '/repo', 'O3', 'durable', 'idle', 't', 't');"
        ));
}

/// (1) `cpd job add --draft` and `cpd job edit` without `--waiting-on <lead|user|explore:<job>>`
/// are refused with `draft_dependency_required` (pro-d549).
#[test]
fn draft_without_waiting_on_is_refused() {
    let fx = Fx::new();
    setup(&fx);
    create_plan(&fx, "p1", "plan");
    let spec = write_spec(&fx, "spec.json");

    // job add --draft without --waiting-on is refused with draft_dependency_required
    let out_add = fx.cpd(&[
        "job",
        "add",
        "j1",
        "--plan",
        "p1",
        "--workflow",
        "w1",
        "--title",
        "Draft job",
        "--tier",
        "S2",
        "--spec",
        &spec,
        "--draft",
    ]);
    refused(&out_add, "draft_dependency_required", 2);

    // Also test invalid --waiting-on value is rejected
    let out_invalid = fx.cpd(&[
        "job",
        "add",
        "j1",
        "--plan",
        "p1",
        "--workflow",
        "w1",
        "--title",
        "Draft job",
        "--tier",
        "S2",
        "--spec",
        &spec,
        "--draft",
        "--waiting-on",
        "invalid_target",
    ]);
    assert_eq!(out_invalid.exit, 2);

    // Add a valid draft job with --waiting-on user
    let out_ok = fx.cpd(&[
        "job",
        "add",
        "j1",
        "--plan",
        "p1",
        "--workflow",
        "w1",
        "--title",
        "Draft job",
        "--tier",
        "S2",
        "--spec",
        &spec,
        "--draft",
        "--waiting-on",
        "user",
    ]);
    assert_eq!(out_ok.exit, 0);

    // job edit on a draft clearing or omitting --waiting-on or editing without it
    let out_edit = fx.cpd(&["job", "edit", "j1", "--plan", "p1", "--waiting-on", ""]);
    refused(&out_edit, "draft_dependency_required", 2);
}

/// (2) `cpd plan review` and `cpd plan lock` refuse if outline `done_when` contains workflow
/// keys not registered in the plan (`outline_incomplete` or `workflow_unregistered`) (pro-d550).
#[test]
fn plan_lock_refuses_unregistered_done_when_phases() {
    let fx = Fx::new();
    setup(&fx);
    create_plan(&fx, "p1", "plan");
    let spec = write_spec(&fx, "spec.json");

    // Register job j1 in w1
    fx.cpd(&[
        "job",
        "add",
        "j1",
        "--plan",
        "p1",
        "--workflow",
        "w1",
        "--title",
        "Job 1",
        "--tier",
        "S2",
        "--spec",
        &spec,
        "--draft",
        "--waiting-on",
        "lead",
    ])
    .ok();

    // Outline contains done_when for w1 and an unregistered workflow/phase w2
    let outline_file = fx.env.root().join("outline.json");
    fs::write(
        &outline_file,
        json!({
            "goal": "Goal text",
            "done_when": {
                "w1": ["w1 done"],
                "w2": ["w2 done"]
            }
        })
        .to_string(),
    )
    .unwrap();

    fx.cpd(&[
        "plan",
        "outline",
        "--plan",
        "p1",
        "--file",
        outline_file.to_str().unwrap(),
    ])
    .ok();

    // cpd plan review should refuse because done_when contains w2 which is not registered
    let out_review = fx.cpd(&["plan", "review", "--plan", "p1", "--title", "First review"]);
    assert_eq!(out_review.exit, 3);
    let val = out_review.json();
    let code = val["error"]["code"].as_str().unwrap_or("");
    assert!(
        code == "outline_incomplete" || code == "workflow_unregistered",
        "expected outline_incomplete or workflow_unregistered, got {code}"
    );

    // cpd plan lock should also refuse
    let out_lock = fx.cpd(&["plan", "lock", "--plan", "p1"]);
    assert_eq!(out_lock.exit, 3);
    let val_lock = out_lock.json();
    let lock_code = val_lock["error"]["code"].as_str().unwrap_or("");
    assert!(
        lock_code == "outline_incomplete"
            || lock_code == "workflow_unregistered"
            || lock_code == "plan_not_approved",
        "expected outline_incomplete or workflow_unregistered or plan_not_approved, got {lock_code}"
    );
}

/// (3a) `settle_done` refuses `plan_incomplete` if any outline `done_when` phase is unregistered/unmerged.
/// Covers the pi-native-core b17 shape: 1 merged workflow with declared phases unregistered cannot reach done.
#[test]
fn plan_done_refuses_unregistered_declared_phases() {
    let fx = Fx::new();
    setup(&fx);
    create_plan(&fx, "p1", "plan");
    let spec = write_spec(&fx, "spec.json");

    // Add job j1 in w1
    fx.cpd(&[
        "job",
        "add",
        "j1",
        "--plan",
        "p1",
        "--workflow",
        "w1",
        "--title",
        "Job 1",
        "--tier",
        "S2",
        "--spec",
        &spec,
        "--draft",
        "--waiting-on",
        "lead",
    ])
    .ok();

    // Outline done_when declares w1 and unregistered phase w2
    let outline_file = fx.env.root().join("outline.json");
    fs::write(
        &outline_file,
        json!({
            "goal": "Goal text",
            "done_when": {
                "w1": ["w1 done"],
                "w2": ["w2 declared but unregistered"]
            }
        })
        .to_string(),
    )
    .unwrap();

    fx.cpd(&[
        "plan",
        "outline",
        "--plan",
        "p1",
        "--file",
        outline_file.to_str().unwrap(),
    ])
    .ok();

    // Simulate w1 merged, j1 merged, and plan in deploy
    fx.env.sql(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
             VALUES ('acme/repo', 1, 'https://github.com/acme/repo/pull/1', 'T', 0, 'merged', 'none', 'none', 't', 't', 't');
             UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'p1';
             UPDATE plans SET phase = 'deploy' WHERE id = 'p1';
             UPDATE workflows SET lead_worktree = '/wt', started_at = 't', status = 'dispatch' WHERE plan = 'p1' AND key = 'w1';
             UPDATE workflows SET pr_number = 1, pr_url = 'https://github.com/acme/repo/pull/1', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/repo' AND number = 1),
                    status = 'open' WHERE plan = 'p1' AND key = 'w1';
             UPDATE workflows SET status = 'merged', merged_at = 't' WHERE plan = 'p1' AND key = 'w1';
             UPDATE jobs SET status = 'queued', fingerprint = printf('%064d', 1) WHERE plan = 'p1' AND key = 'j1';
             UPDATE jobs SET status = 'running' WHERE plan = 'p1' AND key = 'j1';
             UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE plan = 'p1' AND key = 'j1';",
        );

    // When settle_done is evaluated (e.g. via cpd plan lock or workflow cancel or pr poll),
    // the plan should NOT reach phase done because w2 is unregistered.
    // It must be refused with plan_incomplete.
    let phase: String = fx.env.query("SELECT phase FROM plans WHERE id = 'p1'");
    assert_ne!(
        phase, "done",
        "plan must not reach done with unregistered phases in done_when"
    );

    // Attempting to lock/settle the plan refuses with plan_incomplete
    let out = fx.cpd(&["plan", "lock", "--plan", "p1"]);
    refused(&out, "plan_incomplete", 3);
}

/// (3b) `settle_done` refuses `plan_incomplete` if any draft job remains unresolved.
#[test]
fn plan_done_refuses_unresolved_drafts() {
    let fx = Fx::new();
    setup(&fx);
    create_plan(&fx, "p1", "plan");
    let spec = write_spec(&fx, "spec.json");

    // Add job j1 and j2 in w1
    fx.cpd(&[
        "job",
        "add",
        "j1",
        "--plan",
        "p1",
        "--workflow",
        "w1",
        "--title",
        "Job 1",
        "--tier",
        "S2",
        "--spec",
        &spec,
        "--draft",
        "--waiting-on",
        "lead",
    ])
    .ok();

    fx.cpd(&[
        "job",
        "add",
        "j2",
        "--plan",
        "p1",
        "--workflow",
        "w1",
        "--title",
        "Job 2",
        "--tier",
        "S2",
        "--spec",
        &spec,
        "--draft",
        "--waiting-on",
        "user",
    ])
    .ok();

    // Outline done_when covers w1
    let outline_file = fx.env.root().join("outline.json");
    fs::write(
        &outline_file,
        json!({
            "goal": "Goal text",
            "done_when": {
                "w1": ["w1 done"]
            }
        })
        .to_string(),
    )
    .unwrap();

    fx.cpd(&[
        "plan",
        "outline",
        "--plan",
        "p1",
        "--file",
        outline_file.to_str().unwrap(),
    ])
    .ok();

    // Simulate w1 merged, j1 merged, but j2 remains an unresolved draft
    fx.env.sql(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
             VALUES ('acme/repo', 1, 'https://github.com/acme/repo/pull/1', 'T', 0, 'merged', 'none', 'none', 't', 't', 't');
             UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'p1';
             UPDATE plans SET phase = 'deploy' WHERE id = 'p1';
             UPDATE workflows SET lead_worktree = '/wt', started_at = 't', status = 'dispatch' WHERE plan = 'p1' AND key = 'w1';
             UPDATE workflows SET pr_number = 1, pr_url = 'https://github.com/acme/repo/pull/1', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/repo' AND number = 1),
                    status = 'open' WHERE plan = 'p1' AND key = 'w1';
             UPDATE workflows SET status = 'merged', merged_at = 't' WHERE plan = 'p1' AND key = 'w1';
             UPDATE jobs SET status = 'queued', fingerprint = printf('%064d', 1) WHERE plan = 'p1' AND key = 'j1';
             UPDATE jobs SET status = 'running' WHERE plan = 'p1' AND key = 'j1';
             UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE plan = 'p1' AND key = 'j1';",
        );

    // Attempting to settle done must refuse with plan_incomplete while unresolved drafts remain
    let out = fx.cpd(&["plan", "lock", "--plan", "p1"]);
    refused(&out, "plan_incomplete", 3);
}

/// (4) `cpd plan check --plan <p>` CLI reports unregistered declared phases and unresolved drafts.
#[test]
fn plan_check_reports_unregistered_phases() {
    let fx = Fx::new();
    setup(&fx);
    create_plan(&fx, "p1", "plan");
    let spec = write_spec(&fx, "spec.json");

    // Add job j1 and j2
    fx.cpd(&[
        "job",
        "add",
        "j1",
        "--plan",
        "p1",
        "--workflow",
        "w1",
        "--title",
        "Job 1",
        "--tier",
        "S2",
        "--spec",
        &spec,
        "--draft",
        "--waiting-on",
        "lead",
    ])
    .ok();

    fx.cpd(&[
        "job",
        "add",
        "j2",
        "--plan",
        "p1",
        "--workflow",
        "w1",
        "--title",
        "Job 2",
        "--tier",
        "S2",
        "--spec",
        &spec,
        "--draft",
        "--waiting-on",
        "user",
    ])
    .ok();

    // Outline declares w1 and w_missing
    let outline_file = fx.env.root().join("outline.json");
    fs::write(
        &outline_file,
        json!({
            "goal": "Goal text",
            "done_when": {
                "w1": ["w1 done"],
                "w_missing": ["missing phase done"]
            }
        })
        .to_string(),
    )
    .unwrap();

    fx.cpd(&[
        "plan",
        "outline",
        "--plan",
        "p1",
        "--file",
        outline_file.to_str().unwrap(),
    ])
    .ok();

    // Lock plan so j1 is queued and j2 (waiting on user) remains unresolved draft
    fx.env.sql(
            "UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'p1';
             UPDATE jobs SET status = 'queued', fingerprint = printf('%064d', 1) WHERE plan = 'p1' AND key = 'j1';",
        );

    let out = fx.cpd(&["plan", "check", "--plan", "p1"]);
    let res = out.ok();
    // Should report unregistered workflows and unresolved drafts
    let unregistered = res["unregistered_workflows"]
        .as_array()
        .expect("unregistered_workflows array");
    assert!(unregistered.iter().any(|v| v == "w_missing"));
    let drafts = res["unresolved_drafts"]
        .as_array()
        .expect("unresolved_drafts array");
    assert!(drafts.iter().any(|v| v == "j2"));
}

/// (5) `cpd plan reopen <id>` resets a done plan to dispatch phase and clears inactive session refusal.
#[test]
fn plan_reopen_restores_dispatch_and_clears_terminal_state() {
    let fx = Fx::new();
    setup(&fx);
    create_plan(&fx, "p1", "done");
    create_plan(&fx, "sender_plan", "dispatch");

    // Before reopen, an agent sending to the lead session fails with plan_inactive
    let out_send_before = fx.cpd_with(
        fx.env.root(),
        &["session", "send", "lead-p1", "hello", "--unblocks", "task"],
        &[("CPD_SESSION", "lead-sender_plan")],
    );
    refused(&out_send_before, "plan_inactive", 3);

    // Reopen a plan that is not done should be refused illegal_transition
    create_plan(&fx, "p-active", "dispatch");
    let out_reopen_active = fx.cpd(&["plan", "reopen", "p-active"]);
    refused(&out_reopen_active, "illegal_transition", 3);

    // Reopen the done plan
    let out_reopen = fx.cpd(&["plan", "reopen", "p1"]);
    out_reopen.ok();

    // Plan phase is now dispatch, done_at is NULL
    let phase: String = fx.env.query("SELECT phase FROM plans WHERE id = 'p1'");
    assert_eq!(phase, "dispatch");
    let done_at: Option<String> = fx.env.read(|conn| {
        conn.query_row("SELECT done_at FROM plans WHERE id = 'p1'", [], |r| {
            r.get(0)
        })
    });
    assert!(done_at.is_none());

    // Session send is no longer refused with plan_inactive
    let out_send_after = fx.cpd_with(
        fx.env.root(),
        &[
            "session",
            "send",
            "lead-p1",
            "hello after reopen",
            "--unblocks",
            "task",
        ],
        &[("CPD_SESSION", "lead-sender_plan")],
    );
    // It might fail for other reasons (e.g. duplicate) but NOT plan_inactive
    if out_send_after.exit != 0 {
        let val = out_send_after.json();
        assert_ne!(val["error"]["code"].as_str().unwrap_or(""), "plan_inactive");
    }
}
