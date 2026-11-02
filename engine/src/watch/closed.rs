//! The closed-pull-request wake: the Lead of a Plan with a cpd Workflow `open` whose pull request is `closed` is told once per
//! (Workflow, pull request number), by a message from null with `wake` true. The message is the record that it was sent.
//! The step never cancels, reopens or edits anything.

use rusqlite::{Connection, params};

use super::SOURCE;
use super::engine::Engine;
use super::parked::leads;
use super::report::{About, Step};
use cpd_core::error::CpdError;
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::pr::closed::{ClosedUnmerged, closed_unmerged};
use cpd_core::session::tell;

pub(super) fn run(engine: &mut Engine) {
    let found = leads(engine).and_then(|leads| {
        let rows = engine.store.read(|conn| closed_unmerged(conn, None))?;
        Ok((leads, rows))
    });
    let (leads, rows) = match found {
        Ok(found) => found,
        Err(e) => return engine.fail(Step::E20, About::Machine, &e),
    };
    for row in rows {
        let Some(lead) = row.lead_session.clone() else {
            continue;
        };
        if !leads
            .iter()
            .any(|(plan, l)| *plan == row.plan && *l == lead)
        {
            continue;
        }
        match wake(engine, &row, &lead) {
            Ok(true) => engine.log.line(&format!(
                "E20: woke the Lead of Plan {}: Workflow {} has a closed pull request",
                row.plan, row.workflow
            )),
            Ok(false) => {}
            Err(e) => engine.fail(Step::E20, About::Plan(row.plan.clone()), &e),
        }
    }
    engine.clear(Step::E20, &About::Machine);
}

/// What the message begins with: identifies (Workflow key, pull request number).
fn prefix(plan: &PlanId, row: &ClosedUnmerged) -> String {
    format!(
        "Workflow {} of Plan {plan} is open in cpd but its pull request #{} was closed without a merge.",
        row.workflow, row.number
    )
}

fn text(plan: &PlanId, row: &ClosedUnmerged) -> String {
    format!(
        "{} {} A person or GitHub closed it (for example GitHub does when a stacked pull request's base branch is deleted). \
         cpd has not cancelled the Workflow and will not. Decide: ask the User, reopen the pull request on GitHub, or give the Workflow up \
         with `cpd workflow cancel --workflow {}`.",
        prefix(plan, row),
        row.url,
        row.workflow
    )
}

fn sent(
    conn: &Connection,
    plan: &PlanId,
    row: &ClosedUnmerged,
    lead: &SessionId,
) -> Result<bool, CpdError> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM events WHERE session = ?1 AND kind = 'session-message'
            AND json_extract(data, '$.from') IS NULL AND instr(json_extract(data, '$.text'), ?2) = 1)",
        params![lead.as_str(), prefix(plan, row)],
        |r| r.get(0),
    )?)
}

/// Whether it sent the message. The marker is read again inside the transaction that sends.
fn wake(engine: &mut Engine, row: &ClosedUnmerged, lead: &SessionId) -> Result<bool, CpdError> {
    let plan = &row.plan;
    if engine.store.read(|conn| sent(conn, plan, row, lead))? {
        return Ok(false);
    }
    engine.store.write(SOURCE, |tx| {
        if sent(tx, plan, row, lead)? {
            return Ok(false);
        }
        tell(tx, lead, &text(plan, row)).map(|_| true)
    })
}
