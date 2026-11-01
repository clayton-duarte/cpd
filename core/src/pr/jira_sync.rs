//! The Jira transition queue `jira_transitions` (contract §4.4; pro-d126, pro-d128; Plan jira-sync): any step appends in its own
//! transaction, engine E19 drains it oldest first. A data row: it is never deleted but by `plan delete`.
//! A row is PENDING, SETTLED (`settled_at`, `outcome`) or PARKED (`refused_at`, `refusal`, `refused_with`).

use rusqlite::{Connection, OptionalExtension};
use serde_json::json;

use crate::error::{CpdError, ErrorCode};
use crate::events::{append, NewEvent};
use crate::ids::PlanId;
use crate::store::WriteTxn;

/// The status a row moves its PR's ticket to (the policy key `pr.jira.<target>` names it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    InProgress,
    InReview,
    Done,
}

impl Target {
    pub fn as_str(self) -> &'static str {
        match self {
            Target::InProgress => "in_progress",
            Target::InReview => "in_review",
            Target::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Option<Target> {
        match s {
            "in_progress" => Some(Target::InProgress),
            "in_review" => Some(Target::InReview),
            "done" => Some(Target::Done),
            _ => None,
        }
    }
}

/// How a settled row ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Moved,
    Unchanged,
    Skipped,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Moved => "moved",
            Outcome::Unchanged => "unchanged",
            Outcome::Skipped => "skipped",
        }
    }
}

/// A queued transition that is due.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Due {
    pub id: i64,
    pub plan: String,
    pub workflow: String,
    pub target: Target,
    pub ticket: String,
}

/// A parked row; `tried` is `<ticket>|<status name>` as it was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parked {
    pub due: Due,
    pub tried: String,
}

/// Why the helper refused a row, with the ticket and the status name the run used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub reason: String,
    pub message: String,
    pub ticket: String,
    pub status: String,
}

const SELECT: &str = "SELECT t.id, t.plan, t.pr, t.target, p.jira, t.refused_with FROM jira_transitions t JOIN workflows p ON p.plan = t.plan AND p.key = t.pr";

fn corrupt(what: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, what.into())
}

fn due(r: &rusqlite::Row<'_>) -> rusqlite::Result<(Due, Option<String>)> {
    let target: String = r.get(3)?;
    let target = Target::parse(&target).ok_or_else(|| corrupt(format!("unknown jira target {target}")))?;
    let ticket: Option<String> = r.get(4)?;
    let row = Due { id: r.get(0)?, plan: r.get(1)?, workflow: r.get(2)?, target, ticket: ticket.unwrap_or_default() };
    Ok((row, r.get(5)?))
}

/// Queues `target` for PR `pr` of `plan`; `true` when newly queued, `false` when the row exists.
pub fn queue(tx: &WriteTxn<'_>, plan: &str, workflow: &str, target: Target) -> Result<bool, CpdError> {
    let rows = tx.execute(
        "INSERT INTO jira_transitions (plan, pr, target, queued_at) VALUES (?1, ?2, ?3, (SELECT at FROM txns WHERE id = cpd_txn()))
         ON CONFLICT (plan, pr, target) DO NOTHING",
        [plan, workflow, target.as_str()],
    )?;
    Ok(rows == 1)
}

/// The oldest PENDING row whose id is not in `skip`, whose PR has a ticket and is not `cancelled`.
pub fn next_due(conn: &Connection, skip: &[i64]) -> Result<Option<Due>, CpdError> {
    let skip = serde_json::to_string(skip).expect("integers serialize");
    let sql = format!(
        "{SELECT} WHERE t.settled_at IS NULL AND t.refused_at IS NULL AND p.jira IS NOT NULL AND p.status <> 'cancelled'
           AND t.id NOT IN (SELECT value FROM json_each(?1)) ORDER BY t.id LIMIT 1"
    );
    Ok(conn.query_row(&sql, [skip], due).optional()?.map(|(due, _)| due))
}

/// Settles a PENDING row (`settled_at` = the transaction's time); `false` when nothing changed.
pub fn mark_settled(tx: &WriteTxn<'_>, id: i64, outcome: Outcome) -> Result<bool, CpdError> {
    let rows = tx.execute(
        "UPDATE jira_transitions SET settled_at = (SELECT at FROM txns WHERE id = cpd_txn()), outcome = ?2
          WHERE id = ?1 AND settled_at IS NULL AND refused_at IS NULL",
        rusqlite::params![id, outcome.as_str()],
    )?;
    Ok(rows == 1)
}

/// Parks a PENDING row and appends the one `error` Event `jira_refused` in the same transaction (`source` is its source);
/// `false`, nothing written, when the row is not PENDING.
pub fn refuse(tx: &WriteTxn<'_>, source: &str, id: i64, refusal: &Refusal) -> Result<bool, CpdError> {
    let rows = tx.execute(
        "UPDATE jira_transitions SET refused_at = (SELECT at FROM txns WHERE id = cpd_txn()), refusal = ?2, refused_with = ?3
          WHERE id = ?1 AND settled_at IS NULL AND refused_at IS NULL",
        rusqlite::params![id, refusal.reason, format!("{}|{}", refusal.ticket, refusal.status)],
    )?;
    if rows != 1 {
        return Ok(false);
    }
    let (plan, workflow, target): (String, String, String) =
        tx.query_row("SELECT plan, pr, target FROM jira_transitions WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    let details = json!({
        "id": id, "plan": plan, "pr": workflow, "ticket": refusal.ticket, "target": target, "status": refusal.status, "reason": refusal.reason,
    });
    let text = format!(
        "Jira refused to move ticket {} of PR {workflow} of Plan {plan} to `{}`: {} ({}). Nothing is tried again until the PR's ticket (cpd pr edit {workflow} --jira <KEY>) or the policy key pr.jira.{target} changes.",
        refusal.ticket, refusal.status, refusal.reason, refusal.message
    );
    let event = NewEvent::error(text.clone(), source, ErrorCode::JiraRefused.as_str(), &text, details).plan(PlanId::parse(&plan)?);
    append(tx, event)?;
    Ok(true)
}

/// The PARKED rows, oldest first.
pub fn parked(conn: &Connection) -> Result<Vec<Parked>, CpdError> {
    let sql = format!("{SELECT} WHERE t.refused_at IS NOT NULL ORDER BY t.id");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map([], |r| {
            let (due, tried) = due(r)?;
            Ok(Parked { due, tried: tried.unwrap_or_default() })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Clears `refused_at`, `refusal` and `refused_with` of a PARKED row; `false` when it was not parked.
pub fn reopen(tx: &WriteTxn<'_>, id: i64) -> Result<bool, CpdError> {
    let rows = tx.execute(
        "UPDATE jira_transitions SET refused_at = NULL, refusal = NULL, refused_with = NULL WHERE id = ?1 AND refused_at IS NOT NULL",
        [id],
    )?;
    Ok(rows == 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::home::CpdHome;
    use crate::store::Store;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        let workflow = |key: &str, jira: &str, status: &str| {
            let (started, worktree) = if status == "cancelled" { ("NULL", "NULL".to_owned()) } else { ("'t'", format!("'/nowhere/{key}'")) };
            format!(
                "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at, jira)
                   VALUES ('pay', '{key}', '/nowhere', 'main', '{status}', 'cpd/pay/{key}', {worktree}, cpd_txn(), 't', {started}, {jira});"
            )
        };
        let sql = format!(
            "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
               VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
             {}{}{}",
            workflow("api", "'ABC-1'", "dispatch"),
            workflow("web", "NULL", "dispatch"),
            workflow("old", "'ABC-3'", "cancelled"),
        );
        store
            .write("fixture", |tx| {
                tx.execute_batch(&sql)?;
                Ok(())
            })
            .unwrap();
        (dir, store)
    }

    fn q(store: &mut Store, workflow: &str, target: Target) -> bool {
        store.write("t", |tx| queue(tx, "pay", workflow, target)).unwrap()
    }

    fn refusal(status: &str) -> Refusal {
        Refusal { reason: "no_transition".into(), message: "none".into(), ticket: "ABC-1".into(), status: status.into() }
    }

    #[test]
    fn target_and_outcome_names_round_trip() {
        for t in [Target::InProgress, Target::InReview, Target::Done] {
            assert_eq!(Target::parse(t.as_str()), Some(t));
        }
        assert_eq!(Target::parse("nope"), None);
        assert_eq!(
            [Outcome::Moved, Outcome::Unchanged, Outcome::Skipped].map(Outcome::as_str),
            ["moved", "unchanged", "skipped"]
        );
    }

    #[test]
    fn queueing_twice_is_a_no_op() {
        let (_dir, mut store) = store();
        assert!(q(&mut store, "api", Target::InProgress));
        assert!(!q(&mut store, "api", Target::InProgress));
        assert!(q(&mut store, "api", Target::Done));
        let n: i64 = store.read(|c| Ok(c.query_row("SELECT count(*) FROM jira_transitions", [], |r| r.get(0))?)).unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn the_oldest_pending_row_is_due_unless_skipped() {
        let (_dir, mut store) = store();
        q(&mut store, "api", Target::InProgress);
        q(&mut store, "api", Target::InReview);
        let first = store.read(|c| next_due(c, &[])).unwrap().unwrap();
        assert_eq!((first.id, first.target, first.ticket.as_str()), (1, Target::InProgress, "ABC-1"));
        assert_eq!((first.plan.as_str(), first.workflow.as_str()), ("pay", "api"));
        assert_eq!(store.read(|c| next_due(c, &[1])).unwrap().map(|d| d.id), Some(2));
        assert_eq!(store.read(|c| next_due(c, &[1, 2])).unwrap(), None);
    }

    #[test]
    fn a_pr_without_a_ticket_or_cancelled_is_never_due() {
        let (_dir, mut store) = store();
        q(&mut store, "web", Target::InProgress);
        q(&mut store, "old", Target::Done);
        assert_eq!(store.read(|c| next_due(c, &[])).unwrap(), None);
        let n: i64 = store.read(|c| Ok(c.query_row("SELECT count(*) FROM jira_transitions WHERE settled_at IS NULL AND refused_at IS NULL", [], |r| r.get(0))?)).unwrap();
        assert_eq!(n, 2, "nothing else is written for them");
    }

    #[test]
    fn a_row_settles_once_and_is_no_longer_due() {
        let (_dir, mut store) = store();
        q(&mut store, "api", Target::InProgress);
        assert!(store.write("t", |tx| mark_settled(tx, 1, Outcome::Moved)).unwrap());
        assert!(!store.write("t", |tx| mark_settled(tx, 1, Outcome::Unchanged)).unwrap());
        assert!(!store.write("t", |tx| refuse(tx, "watch", 1, &refusal("X"))).unwrap(), "a settled row is not parked");
        assert_eq!(store.read(|c| next_due(c, &[])).unwrap(), None);
        let outcome: String = store.read(|c| Ok(c.query_row("SELECT outcome FROM jira_transitions WHERE id = 1", [], |r| r.get(0))?)).unwrap();
        assert_eq!(outcome, "moved");
    }

    #[test]
    fn a_refused_row_is_parked_once_with_one_event_and_reopens() {
        let (_dir, mut store) = store();
        q(&mut store, "api", Target::InReview);
        assert!(store.write("t", |tx| refuse(tx, "watch", 1, &refusal("In Review"))).unwrap());
        assert!(!store.write("t", |tx| refuse(tx, "watch", 1, &refusal("In Review"))).unwrap(), "already parked: nothing written");
        assert!(!store.write("t", |tx| mark_settled(tx, 1, Outcome::Moved)).unwrap(), "a parked row is not settled");
        let (events, plan, details): (i64, Option<String>, String) = store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT count(*), max(plan), max(json_extract(data, '$.details.reason')) FROM events WHERE kind = 'error'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?)
            })
            .unwrap();
        assert_eq!((events, plan.as_deref(), details.as_str()), (1, Some("pay"), "no_transition"));
        assert_eq!(store.read(|c| next_due(c, &[])).unwrap(), None, "a parked row is never due");
        let rows = store.read(parked).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].due.id, rows[0].tried.as_str()), (1, "ABC-1|In Review"));
        assert!(store.write("t", |tx| reopen(tx, 1)).unwrap());
        assert!(!store.write("t", |tx| reopen(tx, 1)).unwrap(), "not parked any more");
        assert_eq!(store.read(|c| next_due(c, &[])).unwrap().map(|d| d.id), Some(1));
        assert!(store.read(parked).unwrap().is_empty());
    }

    #[test]
    fn parked_rows_come_oldest_first() {
        let (_dir, mut store) = store();
        q(&mut store, "api", Target::InProgress);
        q(&mut store, "api", Target::InReview);
        store.write("t", |tx| refuse(tx, "watch", 2, &refusal("B"))).unwrap();
        store.write("t", |tx| refuse(tx, "watch", 1, &refusal("A"))).unwrap();
        let ids: Vec<i64> = store.read(parked).unwrap().iter().map(|p| p.due.id).collect();
        assert_eq!(ids, [1, 2]);
    }
}
