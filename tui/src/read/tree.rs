//! The `tree` view's snapshot (commands.md §17): Contexts → Plans → Lead
//! Sessions, each with its state, label and name (`context list`, `session list`).
//! A Plan keeps its side forks in `sessions` (its Events and `CPD_PLAN` are the Plan's), but the tree draws them
//! under the Context ([`ContextNode::forks`], B1).

use std::collections::BTreeMap;

use cpd_core::error::CpdError;
use cpd_core::events::highest_id;
use cpd_core::ids::ContextId;
use cpd_core::store::Store;

use super::shared::{PlanHead, SessionNode, id, plan_heads, sessions};

/// Every Context of the machine, by id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tree {
    pub contexts: Vec<ContextNode>,
    /// The highest Event id in this snapshot's read transaction (§17).
    pub events_cursor: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextNode {
    pub id: ContextId,
    pub title: String,
    pub jira: Option<String>,
    pub records: usize,
    /// Questions no answer refers to.
    pub open_questions: usize,
    /// When it was archived (contract §4.5); archived Contexts are in the model.
    pub archived_at: Option<String>,
    /// Its Plans, oldest first.
    pub plans: Vec<PlanNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanNode {
    pub head: PlanHead,
    /// The Plan's Lead first, then its side forks, oldest first.
    pub sessions: Vec<SessionNode>,
}

impl PlanNode {
    pub fn needs_you(&self) -> bool {
        self.head.needs_you(&self.sessions)
    }

    /// The Lead of the Plan, which is in the list unless the database lost it.
    pub fn lead(&self) -> Option<&SessionNode> {
        self.sessions.iter().find(|s| s.plan_lead)
    }
}

impl ContextNode {
    pub fn is_archived(&self) -> bool {
        self.archived_at.is_some()
    }

    /// The Sessions forked by `session fork` (B1, contract §6.8), each with the Plan it belongs to, oldest first
    /// (ties by Plan, then by the order in the Plan). The tree draws them directly under the Context, after the
    /// Plans, whatever their Plan or source.
    pub fn forks(&self) -> Vec<(&PlanNode, &SessionNode)> {
        let mut forks: Vec<(&PlanNode, &SessionNode)> =
            self.plans.iter().flat_map(|p| p.sessions.iter().filter(|s| !s.plan_lead).map(move |s| (p, s))).collect();
        forks.sort_by(|a, b| a.1.created_at.cmp(&b.1.created_at)); // stable
        forks
    }

    pub fn needs_you(&self) -> bool {
        self.plans.iter().any(PlanNode::needs_you)
    }
}

/// The tree's snapshot: one read transaction, a fixed set of queries whatever the number of Contexts and Plans.
pub fn tree(store: &mut Store) -> Result<Tree, CpdError> {
    store.read(|conn| {
        let mut records: BTreeMap<String, usize> = BTreeMap::new();
        let mut stmt = conn.prepare("SELECT context, count(*) FROM context_records GROUP BY context")?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize)))? {
            let (context, n) = row?;
            records.insert(context, n);
        }
        let mut questions: BTreeMap<String, usize> = BTreeMap::new();
        let mut stmt = conn.prepare(
            "SELECT q.context, count(*) FROM context_records q WHERE q.kind = 'question'
               AND NOT EXISTS (SELECT 1 FROM context_records a
                               WHERE a.context = q.context AND a.kind = 'answer' AND a.question = q.id)
              GROUP BY q.context",
        )?;
        for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize)))? {
            let (context, n) = row?;
            questions.insert(context, n);
        }

        let leads: Vec<SessionNode> = sessions(conn, "s.role = 'lead'", &[])?;
        let mut heads = plan_heads(conn, None)?;

        let mut stmt = conn
            .prepare("SELECT id, title, jira, archived_at FROM contexts ORDER BY id")?;
        let mut rows = stmt.query([])?;
        let mut contexts = Vec::new();
        while let Some(r) = rows.next()? {
            let cid: String = r.get(0)?;
            let context = id("Context id", &cid, ContextId::parse)?;
            let (mine, rest): (Vec<PlanHead>, Vec<PlanHead>) = heads.drain(..).partition(|h| h.context == context);
            heads = rest;
            let plans = mine
                .into_iter()
                .map(|head| {
                    let mut own: Vec<SessionNode> = leads.iter().filter(|s| s.plan.as_ref() == Some(&head.id)).cloned().collect();
                    own.sort_by_key(|s| !s.plan_lead); // stable: the Lead first, the rest oldest first
                    PlanNode { head, sessions: own }
                })
                .collect();
            contexts.push(ContextNode {
                title: r.get(1)?,
                jira: r.get(2)?,
                records: records.remove(&cid).unwrap_or(0),
                open_questions: questions.remove(&cid).unwrap_or(0),
                archived_at: r.get(3)?,
                plans,
                id: context,
            });
        }
        Ok(Tree { contexts, events_cursor: highest_id(conn)? })
    })
}
