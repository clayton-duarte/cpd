//! `cpd pr rebase` refuses `attempt_running` while an Attempt of the PR runs (commands.md §9.3, error table): the Lead steers
//! that Worker to do the rebase instead. In-process on the fixture of the rebase tests (`pr.rs`): the git route, the Gate's `sh`
//! and the `gh` that nothing may call are mocks. A running Attempt is an `attempts` row of status `running`.

use crate::pr::{Poll, code, rebase_fx};

use rusqlite::params;
use serde_json::json;

use crate::pr::Rows;

impl Poll {
    /// A Job `key` of PR `repo` of `kind` `running`, with a running Attempt `<key>-a1`.
    fn running_attempt(&self, key: &str, kind: &str) {
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, kind, fingerprint, created_txn, created_at)
                     VALUES ('pay', 'repo', ?1, 'T', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'running', 1, ?2, ?3, ?4, 't')",
                    params![key, kind, "e".repeat(64), tx.id()],
                )?;
                tx.execute(
                    "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, driver_pid, started_at)
                     VALUES ('pay', ?1, ?2, 1, 'S2', 'ladder', 'running', 'cpd/pay/repo', '/w', 1, 't')",
                    params![format!("{key}-a1"), key],
                )?;
                Ok(())
            })
            .unwrap();
    }

    /// The refused run of `pr rebase` changed nothing: the Lead Branch and the remote as they were, no `git`, `gh` or Gate call.
    fn assert_untouched_since(
        &self,
        tip: &str,
        remote: Option<String>,
        git_calls: usize,
        after: usize,
    ) {
        assert_eq!(self.lead_sha(), tip);
        assert_eq!(self.remote_branch("cpd/pay"), remote);
        assert_eq!(after, git_calls, "checked before the fetch");
        assert!(self.gh_calls().is_empty());
        assert_eq!(self.gate_log(), "");
    }
}

fn refused(fx: &Poll, flag: Option<&str>, jobs: serde_json::Value, attempts: serde_json::Value) {
    let tip = fx.lead_sha();
    let remote = fx.remote_branch("cpd/pay");
    let git_calls = fx.calls("git").len();

    let run = match flag {
        Some(flag) => fx.rebase(flag),
        None => fx.pr("rebase"),
    };

    let after = fx.calls("git").len();
    assert_eq!(code(&run), ("attempt_running".into(), 3));
    let error = run.error();
    assert_eq!(
        error["details"],
        json!({"pr": "repo", "jobs": jobs, "attempts": attempts})
    );
    assert!(
        error["message"].as_str().unwrap().contains("steer"),
        "{}",
        error["message"]
    );
    fx.assert_untouched_since(&tip, remote, git_calls, after);
}

#[test]
fn rebase_refuses_attempt_running_while_a_work_job_of_the_pr_has_a_running_attempt() {
    let fx = rebase_fx("deploy");
    fx.advance_base("base.txt", "base\n");
    fx.running_attempt("fix", "work");

    refused(&fx, None, json!(["fix"]), json!(["fix-a1"]));
}

#[test]
fn rebase_refuses_while_an_update_job_attempt_runs() {
    let fx = rebase_fx("deploy");
    fx.advance_base("base.txt", "base\n");
    fx.running_attempt("up-repo", "update");

    refused(&fx, None, json!(["up-repo"]), json!(["up-repo-a1"]));
}

#[test]
fn rebase_continue_refuses_attempt_running_and_leaves_the_stopped_rebase_alone() {
    let fx = rebase_fx("deploy");
    fx.hooks("pre-merge = \"git rev-parse HEAD\"\n");
    fx.sh_prints_head();
    fx.conflicting(&["lead"]);
    assert_eq!(code(&fx.pr("rebase")).0, "rebase_conflict");
    fx.resolve("resolved");
    fx.running_attempt("fix", "work");

    refused_continue(&fx);
}

fn refused_continue(fx: &Poll) {
    let tip = fx.lead_sha();
    let remote = fx.remote_branch("cpd/pay");
    let git_calls = fx.calls("git").len();

    let run = fx.rebase("--continue");
    let after = fx.calls("git").len();

    assert_eq!(code(&run), ("attempt_running".into(), 3));
    assert_eq!(
        run.error()["details"],
        json!({"pr": "repo", "jobs": ["fix"], "attempts": ["fix-a1"]})
    );
    // The rebase still waits, resolved but not continued.
    assert!(fx.rebasing());
    fx.assert_untouched_since(&tip, remote, git_calls, after);
}

/// Regression pin, not a listed red test: `--abort` is never refused, it is how a stopped rebase is given up.
#[test]
fn rebase_abort_is_not_refused_while_an_attempt_runs() {
    let fx = rebase_fx("deploy");
    fx.hooks("pre-merge = \"git rev-parse HEAD\"\n");
    fx.sh_prints_head();
    let before = fx.conflicting(&["lead"]);
    assert_eq!(code(&fx.pr("rebase")).0, "rebase_conflict");
    fx.running_attempt("fix", "work");

    let out = fx.rebase("--abort").ok();

    assert_eq!(out["aborted"], true);
    assert!(!fx.rebasing());
    assert_eq!(fx.lead_sha(), before);
}
