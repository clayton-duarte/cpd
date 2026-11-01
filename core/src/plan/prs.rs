//! The `prs` row as read (contract §2.2 PR, R34 A6): one struct for every
//! reader, parsed once, so what follows holds valid values.

use std::collections::BTreeMap;
use std::path::PathBuf;

use rusqlite::Connection;

use crate::error::CpdError;
use crate::ids::{PlanId, WorkflowKey};

/// The pull request of a PR that is `open` or `merged` (contract §2.2 PR `pr`).
#[derive(Clone, Debug, PartialEq)]
pub struct Pull {
    pub number: i64,
    pub url: String,
    pub opened_at: String,
}

/// One PR of a Plan.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkflowRow {
    pub key: WorkflowKey,
    /// As stored: the fingerprint and `pr add` compare it as text.
    pub repo: String,
    pub base: String,
    pub lead_branch: String,
    /// Set from the status `dispatch` on (`prs` CHECK).
    pub lead_worktree: Option<PathBuf>,
    /// `waiting`, `dispatch`, `open`, `merged` or `cancelled` (§3.8).
    pub status: String,
    /// The PRs it needs, sorted, as PR references (contract §2.2): the bare key of a PR of its Plan,
    /// `<plan>/<key>` of a PR of another.
    pub needs: Vec<String>,
    pub pull: Option<Pull>,
    /// The id of the PR's `pull_requests` row: set exactly when the status is `open` or `merged` (`prs` CHECK).
    pub pull_request: Option<i64>,
    /// The repo's GitHub name (`prs.name`); null for a PR added before it existed.
    pub name: Option<String>,
    /// The PR's title (`prs.title`, hs-d285): what the summary says of it; null until set.
    pub title: Option<String>,
    /// The PR's own Jira ticket (`prs.jira`): null until set.
    pub jira: Option<String>,
    /// The PR's stored shape (`prs.shape`, hs-d497): `tdd`, `docs`, `config`, or `free` for a PR that existed before
    /// migration `job-kinds`.
    pub shape: String,
    /// The head branch of the pull request cpd adopted (`prs.head_ref`, pro-n8); null for a cpd PR.
    pub head_ref: Option<String>,
    /// When the Lead Branch started (`prs.started_at`, ISO with milliseconds); null before `dispatch`.
    pub started_at: Option<String>,
}

impl WorkflowRow {
    /// The branch every push, fetch, `gh pr list --head` and merged check names: `head_ref` of an adopted PR, else the Lead Branch.
    pub fn remote_branch(&self) -> &str {
        self.head_ref.as_deref().unwrap_or(&self.lead_branch)
    }

    /// The instant a GitHub pull request of the head must not predate to be this PR's: the row's start. An adopted PR
    /// (`head_ref` set) pre-exists its row by design, so it has none. A cpd PR with no `started_at` is a bad row.
    pub fn bound_since(&self) -> Result<Option<jiff::Timestamp>, CpdError> {
        if self.head_ref.is_some() {
            return Ok(None);
        }
        let started = self.started_at.as_deref().ok_or_else(|| CpdError::bad_row(format!("PR {} has no started_at", self.key)))?;
        started.parse().map(Some).map_err(|e| CpdError::bad_row(format!("PR {} has the started_at {started:?}: {e}", self.key)))
    }
}

#[cfg(test)]
impl WorkflowRow {
    /// A PR of the fields the fingerprint, `plan render` and `--pr` selection read.
    pub fn fixture(key: &str, repo: &str, base: &str, needs: &[&str], status: &str) -> WorkflowRow {
        WorkflowRow {
            key: WorkflowKey::parse(key).unwrap(),
            repo: repo.to_owned(),
            base: base.to_owned(),
            lead_branch: format!("cpd/fixture/{key}"),
            lead_worktree: None,
            status: status.to_owned(),
            needs: needs.iter().map(|n| (*n).to_owned()).collect(),
            pull: None,
            pull_request: None,
            name: None,
            title: None,
            jira: None,
            shape: "free".to_owned(),
            head_ref: None,
            started_at: None,
        }
    }
}

type Raw = (String, String, String, String, Option<String>, String, (Option<i64>, Option<String>, Option<String>), Option<i64>, (Option<String>, Option<String>, String, Option<String>, Option<String>, Option<String>));

/// The needs of every PR of `plan` by PR key, each list sorted, as keys of the Plan (contract §2.2).
pub fn load_needs(conn: &Connection, plan: &PlanId) -> Result<BTreeMap<String, Vec<String>>, CpdError> {
    let mut needs: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut stmt = conn.prepare("SELECT workflow, needs FROM workflow_needs WHERE plan = ?1")?;
    for row in stmt.query_map([plan.as_str()], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (workflow, need) = row?;
        needs.entry(workflow).or_default().push(need);
    }
    needs.values_mut().for_each(|list| list.sort());
    Ok(needs)
}

/// Every PR of `plan` in key order, `cancelled` ones included.
pub fn load_workflows(conn: &Connection, plan: &PlanId) -> Result<Vec<WorkflowRow>, CpdError> {
    let mut needs = load_needs(conn, plan)?;
    let mut stmt = conn.prepare(
        "SELECT key, repo, base, lead_branch, lead_worktree, status, pr_number, pr_url, pr_opened_at, pull_request, name, title, shape, head_ref, started_at, jira
           FROM workflows WHERE plan = ?1 ORDER BY key",
    )?;
    let raw: Vec<Raw> = stmt
        .query_map([plan.as_str()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, (r.get(6)?, r.get(7)?, r.get(8)?), r.get(9)?, (r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?, r.get(14)?, r.get(15)?)))
        })?
        .collect::<rusqlite::Result<_>>()?;
    raw.into_iter()
        .map(|(key, repo, base, lead_branch, lead_worktree, status, pull, pull_request, (name, title, shape, head_ref, started_at, jira))| {
            let needs = needs.remove(&key).unwrap_or_default();
            let key = WorkflowKey::parse(&key).map_err(|_| CpdError::bad_row(format!("Plan {plan} has a PR key {key:?}")))?;
            let pull = match pull {
                (Some(number), Some(url), Some(opened_at)) => Some(Pull { number, url, opened_at }),
                _ => None,
            };
            Ok(WorkflowRow { key, repo, base, lead_branch, lead_worktree: lead_worktree.map(PathBuf::from), status, needs, pull, pull_request, name, title, shape, head_ref, started_at, jira })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::home::CpdHome;
    use crate::store::Store;

    /// commands-G3: one PR row, read once, with every column the three
    /// readers (`WorkflowSpec`, `start::Workflow`, `row::WorkflowRow`) had between them.
    #[test]
    fn a_pr_row_carries_the_columns_every_reader_needs() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        store
            .write("fixture", |tx| {
                tx.execute_batch(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
                     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                       VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
                     INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('pay', 'ctx', 'Pay', 'grounding', 'lead1', 't');
                     INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
                       VALUES ('pay', 'api', '/r/api', 'main', 'waiting', 'cpd/pay/api', cpd_txn(), 't'),
                              ('pay', 'web', '/r/web', 'dev', 'waiting', 'cpd/pay/web', cpd_txn(), 't');
                     INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('pay', 'web', 'api');
                     UPDATE workflows SET status = 'dispatch', lead_worktree = '/w/api', started_at = 't' WHERE key = 'api';
                     INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
                       VALUES ('acme/api', 7, 'https://x/7', 'T', 0, 'open', 'none', 'none', 't', 't');
                     UPDATE workflows SET status = 'open', pr_number = 7, pr_url = 'https://x/7', pr_opened_at = 'o',
                            pull_request = (SELECT id FROM pull_requests WHERE number = 7) WHERE key = 'api';",
                )?;
                Ok(())
            })
            .unwrap();
        let plan = PlanId::parse("pay").unwrap();
        let workflows = store.read(|conn| load_workflows(conn, &plan)).unwrap();
        let [api, web] = workflows.as_slice() else { panic!("{prs:?}") };
        assert_eq!((api.key.as_str(), api.repo.as_str(), api.base.as_str(), api.lead_branch.as_str()), ("api", "/r/api", "main", "cpd/pay/api"));
        assert_eq!((api.status.as_str(), api.lead_worktree.as_deref(), api.needs.as_slice()), ("open", Some(Path::new("/w/api")), &[][..]));
        let pull = api.pull.as_ref().unwrap();
        assert_eq!((pull.number, pull.url.as_str(), pull.opened_at.as_str()), (7, "https://x/7", "o"));
        assert!(api.pull_request.is_some());
        assert_eq!((web.key.as_str(), web.status.as_str(), web.lead_worktree.as_ref(), web.needs.as_slice()), ("web", "waiting", None, &["api".to_owned()][..]));
        assert!(web.pull.is_none() && web.pull_request.is_none());
    }
}
