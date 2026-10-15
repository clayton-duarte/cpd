//! The `tree` view's snapshot (commands.md §17): Contexts → Plans → Lead
//! Sessions, each with its state, label and name (`context list`, `session list`).

use std::collections::BTreeMap;

use cpd_core::error::CpdError;
use cpd_core::events::highest_id;
use cpd_core::ids::{ContextId, SessionId};
use cpd_core::store::Store;

use super::shared::{PlanHead, SessionNode, bad_row, id, plan_heads, sessions};

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
    /// This machine's grounded Lead, when the Context has one.
    pub grounded_lead: Option<SessionNode>,
    pub records: usize,
    /// Questions no answer refers to.
    pub open_questions: usize,
    pub sync_failing: bool,
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

        let mut leads: Vec<SessionNode> = sessions(conn, "s.role = 'lead'", &[])?;
        let mut heads = plan_heads(conn, None)?;

        let mut stmt = conn
            .prepare("SELECT id, title, jira, lead_session, sync_failing FROM contexts ORDER BY id")?;
        let mut rows = stmt.query([])?;
        let mut contexts = Vec::new();
        while let Some(r) = rows.next()? {
            let cid: String = r.get(0)?;
            let context = id("Context id", &cid, ContextId::parse)?;
            let grounded: Option<String> = r.get(3)?;
            let grounded = grounded
                .map(|g| {
                    let g = SessionId::parse(&g)
                        .map_err(|_| bad_row(format!("Context {context} has lead_session {g:?}")))?;
                    leads
                        .iter()
                        .position(|s| s.id == g)
                        .map(|i| leads.remove(i))
                        .ok_or_else(|| bad_row(format!("Context {context} has the grounded Lead {g}, which is no Lead")))
                })
                .transpose()?;
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
                grounded_lead: grounded,
                records: records.remove(&cid).unwrap_or(0),
                open_questions: questions.remove(&cid).unwrap_or(0),
                sync_failing: r.get(4)?,
                plans,
                id: context,
            });
        }
        Ok(Tree { contexts, events_cursor: highest_id(conn)? })
    })
}
