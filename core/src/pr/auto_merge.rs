//! Auto-merge of a posted Workflow (commands.md §9.8b; pro-d115, pro-d117): the one read query and the GraphQL mutation
//! `enablePullRequestAutoMerge` with the first method the repo allows, never `gh pr merge`. `pr post` reads once before
//! its send and runs [`after_read`]; the engine's E18 runs [`attempt`] (read, then [`after_read`]).

use std::path::Path;

use rusqlite::Connection;

use super::gh;
use crate::error::CpdError;
use crate::host::runner::Runner;

/// How one attempt to turn on auto-merge ended.
#[derive(Debug)]
pub enum Outcome {
    /// GitHub accepted the request, or the pull request already had one.
    Enabled,
    /// The repo does not allow auto-merge: nothing was requested, and that is a stated condition, not an error.
    NotAllowed,
    /// GitHub answered the mutation with an error (its message); never retried.
    Refused(String),
    /// The repo allows no merge method (its message): nothing was requested; never retried.
    NoMethod(String),
    /// `gh` itself failed (timeout, HTTP 5xx, bad JSON): retried later.
    Failed(CpdError),
}

/// Whether Workflow `pr` of `plan` is stacked: its stored `base` is the `lead_branch` of any `prs` row (any Plan, any status).
pub fn stacked(conn: &Connection, plan: &str, workflow: &str) -> Result<bool, CpdError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM workflows b WHERE b.lead_branch = (SELECT base FROM workflows WHERE plan = ?1 AND key = ?2))",
        [plan, workflow],
        |r| r.get(0),
    )?)
}

/// The read query, then [`after_read`]. `cwd` is any directory of the repo.
pub fn attempt(runner: &dyn Runner, cwd: &Path, repo: &str, number: i64) -> Outcome {
    match gh::auto_merge_read(runner, cwd, repo, number) {
        Ok(read) => after_read(runner, cwd, &read),
        Err(e) => Outcome::Failed(e),
    }
}

/// What follows an already-made `read`: not allowed, already requested, no usable method, else the mutation.
pub fn after_read(runner: &dyn Runner, cwd: &Path, read: &gh::AutoMergeRead) -> Outcome {
    if !read.allowed {
        return Outcome::NotAllowed;
    }
    if read.requested {
        return Outcome::Enabled;
    }
    let Some(method) = read.methods.choose() else {
        return Outcome::NoMethod("the repo allows no merge method".to_owned());
    };
    match gh::enable_auto_merge(runner, cwd, &read.id, method) {
        Ok(Ok(())) => Outcome::Enabled,
        Ok(Err(message)) => Outcome::Refused(message),
        Err(e) => Outcome::Failed(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(rows: &[(&str, &str, &str)]) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE workflows (plan TEXT, key TEXT, base TEXT, lead_branch TEXT)",
        )
        .unwrap();
        for (plan, key, base) in rows {
            conn.execute("INSERT INTO workflows (plan, key, base, lead_branch) VALUES (?1, ?2, ?3, 'cpd/' || ?1 || '/' || ?2)", [plan, key, base]).unwrap();
        }
        conn
    }

    #[test]
    fn a_pr_based_on_a_lead_branch_of_any_plan_is_stacked() {
        let conn = conn(&[
            ("a", "one", "main"),
            ("b", "two", "cpd/a/one"),
            ("b", "three", "main"),
        ]);
        assert!(stacked(&conn, "b", "two").unwrap());
        assert!(!stacked(&conn, "a", "one").unwrap());
        assert!(!stacked(&conn, "b", "three").unwrap());
    }

    /// The send half of `pr_post_auto_merge`'s `an_already_enabled_request_counts_as_enabled_without_a_second_mutation`
    /// (the enqueue half stays in that file): a pull request that already has a request is enabled, with no mutation.
    #[test]
    fn an_already_enabled_request_counts_as_enabled_without_a_second_mutation() {
        let fakes = crate::ports::Fakes::new();
        let read = gh::AutoMergeRead {
            allowed: true,
            id: "PR_1".to_owned(),
            requested: true,
            methods: crate::pr::merge_method::MergeMethods {
                squash: true,
                merge: false,
                rebase: false,
            },
        };
        assert!(matches!(
            after_read(&*fakes.runner, Path::new("."), &read),
            Outcome::Enabled
        ));
        assert!(
            fakes.runner.calls().is_empty(),
            "no mutation: {:?}",
            fakes.runner.calls()
        );
    }
}
