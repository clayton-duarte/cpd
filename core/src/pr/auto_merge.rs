//! Squash auto-merge of a posted PR (commands.md §9.8b; pro-d115, pro-d117): the one read query and the GraphQL mutation
//! `enablePullRequestAutoMerge`, never `gh pr merge`. `pr post` and the engine's E18 both run [`attempt`].

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
    /// The repo does not allow auto-merge: nothing was requested.
    NotAllowed,
    /// GitHub answered the mutation with an error (its message); never retried.
    Refused(String),
    /// `gh` itself failed (timeout, HTTP 5xx, bad JSON): retried later.
    Failed(CpdError),
}

/// Whether PR `pr` of `plan` is stacked: its stored `base` is the `lead_branch` of any `prs` row (any Plan, any status).
pub fn stacked(conn: &Connection, plan: &str, pr: &str) -> Result<bool, CpdError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM prs b WHERE b.lead_branch = (SELECT base FROM prs WHERE plan = ?1 AND key = ?2))",
        [plan, pr],
        |r| r.get(0),
    )?)
}

/// The read query, then the mutation unless the repo disallows it or the request already exists. `cwd` is any directory of the repo.
pub fn attempt(runner: &dyn Runner, cwd: &Path, repo: &str, number: i64) -> Outcome {
    let read = match gh::auto_merge_read(runner, cwd, repo, number) {
        Ok(read) => read,
        Err(e) => return Outcome::Failed(e),
    };
    if !read.allowed {
        return Outcome::NotAllowed;
    }
    if read.requested {
        return Outcome::Enabled;
    }
    match gh::enable_auto_merge(runner, cwd, &read.id) {
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
        conn.execute_batch("CREATE TABLE prs (plan TEXT, key TEXT, base TEXT, lead_branch TEXT)").unwrap();
        for (plan, key, base) in rows {
            conn.execute("INSERT INTO prs (plan, key, base, lead_branch) VALUES (?1, ?2, ?3, 'cpd/' || ?1 || '/' || ?2)", [plan, key, base]).unwrap();
        }
        conn
    }

    #[test]
    fn a_pr_based_on_a_lead_branch_of_any_plan_is_stacked() {
        let conn = conn(&[("a", "one", "main"), ("b", "two", "cpd/a/one"), ("b", "three", "main")]);
        assert!(stacked(&conn, "b", "two").unwrap());
        assert!(!stacked(&conn, "a", "one").unwrap());
        assert!(!stacked(&conn, "b", "three").unwrap());
    }
}
