//! `pr describe` (commands.md §9.5a) and `pr open` with the stored description (§9.1, contract §7.6): the Lead stores the PR's
//! body, the open Job gives it to GitHub verbatim, and a PR without one is refused `description_required`. In-process over the
//! fake ports and the fake `gh` of `pr.rs` ([`Poll`]), against a Plan written straight into the database.

use std::fs;

use serde_json::json;

use super::workflow::{DESCRIPTION, Poll, Rows, code, in_git_repo, open_jobs, poll_fx};

impl Poll {
    /// `cpd pr describe --workflow repo --file <file holding text> --plan pay`.
    fn describe(&self, text: &str) -> crate::common::Run {
        let file = self.env.root().join("body.md");
        fs::write(&file, text).unwrap();
        self.cpd(&[
            "workflow",
            "describe",
            "repo",
            "--file",
            file.to_str().unwrap(),
            "--plan",
            "pay",
        ])
    }

    /// The stored `description` of PR `repo`.
    fn description(&self) -> Option<String> {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT description FROM workflows WHERE plan = 'pay' AND key = 'repo'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }

    /// Runs `sql` on the Plan's rows.
    fn exec(&self, sql: &str) {
        // `workflows_head_ref_fixed` refuses any later `head_ref` change (contract §4.4): this test database drops it to adopt a PR, then
        // puts it back as the migration has it, so the schema check of the next `cpd` still passes.
        let (drop, restore) = if sql.contains("head_ref") {
            (
                "DROP TRIGGER workflows_head_ref_fixed;",
                "CREATE TRIGGER workflows_head_ref_fixed BEFORE UPDATE OF head_ref ON workflows\nBEGIN SELECT RAISE(ABORT, 'workflow_locked'); END;",
            )
        } else {
            ("", "")
        };
        self.store()
            .write("fixture", |tx| {
                Ok(tx.execute_batch(&format!("{drop}{sql};{restore}"))?)
            })
            .unwrap();
    }
}

#[test]
fn pr_describe_stores_the_body_and_a_second_describe_replaces_it() {
    let fx = poll_fx("dispatch");
    let first = "  ## What and why\n\nCharge the card.\n\n";

    let out = fx.describe(first).ok();

    assert_eq!(
        out,
        json!({"ok": true, "workflow": {"key": "repo", "status": "dispatch"}})
    );
    assert_eq!(fx.description().as_deref(), Some(first), "stored verbatim");
    assert_eq!(
        fx.events("workflow-described")
            .iter()
            .map(|e| e.1.clone())
            .collect::<Vec<_>>(),
        [json!({"workflow": "repo"})]
    );

    // Another text replaces it.
    let second = "## What and why\n\nCharge it twice.\n";
    assert_eq!(fx.describe(second).ok()["workflow"]["key"], "repo");
    assert_eq!(fx.description().as_deref(), Some(second));
    assert_eq!(fx.events("workflow-described").len(), 2);

    // The same text again is a no-op: `already`, no transaction, no Event.
    let txns = fx.txns();
    let out = fx.describe(second).ok();
    assert_eq!(out["already"], true);
    assert_eq!(fx.txns(), txns);
    assert_eq!(fx.events("workflow-described").len(), 2);
    assert_eq!(fx.description().as_deref(), Some(second));
}

#[test]
fn pr_describe_refuses_an_empty_or_oversize_body() {
    let fx = poll_fx("dispatch");
    fx.exec("UPDATE workflows SET description = NULL");
    let txns = fx.txns();

    for bad in ["".to_owned(), "  \n\t\n".to_owned(), "a".repeat(65537)] {
        let run = fx.describe(&bad);
        assert_eq!(
            code(&run),
            ("description_invalid".into(), 3),
            "{} characters",
            bad.chars().count()
        );
        assert_eq!(
            (fx.description(), fx.txns()),
            (None, txns),
            "nothing stored"
        );
    }

    // 65536 characters pass (characters, not bytes), 65537 fail.
    let limit = "é".repeat(65536);
    fx.describe(&limit).ok();
    assert_eq!(fx.description().as_deref(), Some(limit.as_str()));
    let run = fx.describe(&"é".repeat(65537));
    assert_eq!(code(&run), ("description_invalid".into(), 3));
    assert_eq!(fx.description().as_deref(), Some(limit.as_str()));
}

#[test]
fn pr_describe_refuses_an_adopted_or_closed_pr() {
    // One in-process `Fx` at a time: each block drops its own before the next.
    {
        let fx = poll_fx("dispatch");
        fx.exec("UPDATE workflows SET head_ref = 'feature/x'");
        let txns = fx.txns();

        let run = fx.describe("New body\n");

        assert_eq!(code(&run), ("workflow_adopted".into(), 3));
        assert_eq!(run.error()["details"]["pr"], "repo");
        assert_eq!(
            (fx.description().as_deref(), fx.txns()),
            (Some(DESCRIPTION), txns)
        );
        assert!(fx.gh_calls().is_empty(), "no gh call");
    }
    {
        let fx = poll_fx("done");
        let txns = fx.txns();

        let run = fx.describe("New body\n");

        assert_eq!(code(&run), ("workflow_ended".into(), 3));
        assert_eq!(
            (fx.description().as_deref(), fx.txns()),
            (Some(DESCRIPTION), txns)
        );
        assert!(fx.gh_calls().is_empty(), "no gh call");
    }
    let fx = poll_fx("plan");
    fx.exec("UPDATE workflows SET status = 'cancelled'");
    let txns = fx.txns();

    let run = fx.describe("New body\n");

    assert_eq!(code(&run), ("workflow_ended".into(), 3));
    assert_eq!(
        (fx.description().as_deref(), fx.txns()),
        (Some(DESCRIPTION), txns)
    );
}

#[test]
fn pr_describe_on_an_open_pr_edits_the_pull_request_body() {
    {
        let fx = poll_fx("deploy");
        let body = "## What and why\n\nNew words.\n";

        let out = fx.describe(body).ok();

        assert_eq!(out["workflow"], json!({"key": "repo", "status": "open"}));
        let calls = fx.calls("gh");
        assert_eq!(calls.len(), 1, "{:?}", fx.gh_calls());
        assert_eq!(
            calls[0].args,
            [
                "api",
                "-X",
                "PATCH",
                "repos/acme/pay/pulls/812",
                "--input",
                "-"
            ]
        );
        let sent: serde_json::Value =
            serde_json::from_slice(calls[0].stdin.as_deref().expect("a JSON body on stdin"))
                .unwrap();
        assert_eq!(sent, json!({"body": body}));
        assert_eq!(fx.description().as_deref(), Some(body));
    }
    {
        // A failing GitHub update stores nothing.
        let fx = poll_fx("deploy");
        fx.fail_update("HTTP 422: nope");
        let txns = fx.txns();

        let run = fx.describe("New words.\n");

        assert_eq!(code(&run), ("gh_failed".into(), 1));
        assert_eq!(
            (fx.description().as_deref(), fx.txns()),
            (Some(DESCRIPTION), txns)
        );
    }
    for phase in ["plan", "dispatch"] {
        let fx = poll_fx(phase);

        fx.describe("New words.\n").ok();

        assert!(
            fx.gh_calls().is_empty(),
            "{phase}: no gh call, {:?}",
            fx.gh_calls()
        );
        assert_eq!(fx.description().as_deref(), Some("New words.\n"));
    }
}

#[test]
fn pr_open_creates_the_pull_request_with_the_stored_description() {
    let fx = poll_fx("dispatch");
    let body = "## What and why\n\nOne line, no title, no table.\n\n## Links\n\n- none\n";
    fx.exec(&format!("UPDATE workflows SET description = '{body}'"));
    let (attempt, _, _) = fx.submitted_opener();

    fx.merge(&attempt);

    let calls = fx.gh_calls();
    let create = calls
        .iter()
        .find(|c| c[1] == "create")
        .unwrap_or_else(|| panic!("no gh pr create: {calls:?}"));
    assert_eq!(
        create[..8],
        [
            "pr",
            "create",
            "--base",
            "main",
            "--head",
            "cpd/pay",
            "--title",
            "Charge the card at checkout"
        ]
    );
    assert_eq!(
        create[8..],
        ["--body", body],
        "the body is exactly the stored text"
    );
    assert!(
        !create[9].contains("Generated by cpd")
            && !create[9].contains("| api |")
            && !create[9].starts_with("# "),
        "{}",
        create[9]
    );
}

#[test]
fn pr_open_refuses_description_required_without_a_body() {
    {
        let fx = poll_fx("dispatch");
        fx.exec("UPDATE workflows SET description = NULL");
        let (txns, remote) = (fx.txns(), fx.remote_branch("cpd/pay"));

        let run = fx.pr("open");

        assert_eq!(code(&run), ("description_required".into(), 3));
        assert_eq!(run.error()["details"], json!({"plan": "pay", "pr": "repo"}));
        assert_eq!(
            (fx.plan_row().0, fx.txns(), open_jobs(&fx)),
            ("dispatch".to_owned(), txns, Vec::<(String, String)>::new())
        );
        assert_eq!(fx.remote_branch("cpd/pay"), remote, "nothing pushed");
        assert!(fx.gh_calls().is_empty(), "no gh call");
    }
    // An adopted PR keeps its own body: no description needed.
    let fx = poll_fx("dispatch");
    fx.exec("UPDATE workflows SET description = NULL, head_ref = 'feature/x'");

    fx.pr("open").ok();

    assert_eq!(open_jobs(&fx).len(), 1, "the open Job is queued");
}

#[test]
fn pr_describe_of_an_open_pr_succeeds_from_a_cwd_that_is_not_a_repository() {
    let fx = poll_fx("deploy");
    let body = "## What and why\n\nFrom anywhere.\n";

    let out = fx.describe(body).ok();

    assert_eq!(out["workflow"], json!({"key": "repo", "status": "open"}));
    let calls = fx.calls("gh");
    assert_eq!(calls.len(), 1, "{:?}", fx.gh_calls());
    let cwd = calls[0].cwd.clone().expect("gh runs with a cwd");
    assert!(
        !in_git_repo(&cwd, fx.env.root()),
        "{cwd:?} must not be a git repository: the fake's refusal is live"
    );
    assert_eq!(fx.description().as_deref(), Some(body));
}

#[test]
fn pr_describe_refuses_an_open_pull_request_repo_that_is_not_owner_name() {
    let fx = poll_fx("deploy");
    fx.exec("UPDATE pull_requests SET repo = 'acme/pay x'");
    let txns = fx.txns();

    let run = fx.describe("New words.\n");

    assert_eq!(code(&run), ("gh_failed".into(), 1));
    assert_eq!(run.error()["details"]["repo"], "acme/pay x");
    assert!(
        fx.gh_calls().is_empty(),
        "nothing sent: {:?}",
        fx.gh_calls()
    );
    assert_eq!(
        (fx.description().as_deref(), fx.txns()),
        (Some(DESCRIPTION), txns)
    );
}
