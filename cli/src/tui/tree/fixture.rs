//! Hand-built trees for the unit tests: no store, so a test names only what it looks at.
//! (The prepared store of `tests/tui_tree.rs` goes through the real read model.)

use crate::config::Tier;
use crate::ids::{ContextId, PlanId, SessionId};
use crate::plan::Phase;
use crate::session::{Role, State};
use crate::tui::read::{ContextNode, Place, PlanHead, PlanNode, SessionNode, Tree};
use crate::tui::read::JobCounts;

pub fn session(id: &str, name: &str, state: State, label: Option<&str>, place: Place) -> SessionNode {
    SessionNode {
        id: SessionId::parse(id).unwrap(),
        role: Role::Lead,
        context: ContextId::parse("alpha").unwrap(),
        plan: None,
        attempt: None,
        name: name.into(),
        file: None,
        forked_from: None,
        tier: Tier::parse("O3").unwrap(),
        tmux_pane: None,
        state,
        state_label: label.map(Into::into),
        state_at: "2026-01-01T00:00:00.000Z".into(),
        created_at: "2026-01-01T00:00:00.000Z".into(),
        place,
        plan_lead: false,
        grounded_lead: false,
        needs_you: state == State::Blocked,
    }
}

/// A Plan: `lead` is its Lead, `forks` its side forks. `counts` are
/// `[draft, queued, running, merging, failed, merged]`.
pub fn plan(id: &str, context: &str, phase: Phase, counts: [usize; 6], mut lead: SessionNode, forks: Vec<SessionNode>) -> PlanNode {
    let pid = PlanId::parse(id).unwrap();
    lead.plan = Some(pid.clone());
    lead.plan_lead = true;
    let mut sessions = vec![lead.clone()];
    for mut fork in forks {
        fork.plan = Some(pid.clone());
        sessions.push(fork);
    }
    let [draft, queued, running, merging, failed, merged] = counts;
    PlanNode {
        head: PlanHead {
            id: pid,
            context: ContextId::parse(context).unwrap(),
            title: format!("Title of {id}"),
            jira: None,
            repo: format!("/repo/{id}"),
            base: "main".into(),
            lead_branch: format!("cpd/{id}"),
            phase,
            lead_session: lead.id,
            pr: None,
            counts: JobCounts { draft, queued, running, merging, failed, merged, cancelled: 0 },
            open_escalations: 0,
            review_pending: false,
            created_at: "2026-01-01T00:00:00.000Z".into(),
            locked_at: None,
            all_merged_at: None,
            done_at: None,
        },
        sessions,
    }
}

pub fn context(id: &str, plans: Vec<PlanNode>) -> ContextNode {
    ContextNode {
        id: ContextId::parse(id).unwrap(),
        title: format!("Title of {id}"),
        jira: None,
        grounded_lead: None,
        records: 0,
        open_questions: 0,
        sync_failing: false,
        plans,
    }
}

pub fn tree(contexts: Vec<ContextNode>) -> Tree {
    Tree { contexts, events_cursor: 1 }
}

/// Every state of a Session, a side fork, a Plan in each phase and an empty Context:
///
/// - `alpha`: `ship` (dispatch; its Lead is working and in the chat slot; two forks, one idle, one blocked),
///   `sketch` (plan; idle), `old` (done; stopped);
/// - `beta`: `rel` (pr; its Lead is blocked on a Worker question);
/// - `gamma`: no Plan.
pub fn sample() -> Tree {
    use State::*;
    tree(vec![
        context(
            "alpha",
            vec![
                plan(
                    "ship",
                    "alpha",
                    Phase::Dispatch,
                    [0, 2, 1, 1, 0, 5],
                    session("l-ship", "ship lead", Working, None, Place::Chat),
                    vec![
                        session("f-idle", "ship fork", Idle, None, Place::Parked),
                        session("f-blocked", "ship review", Blocked, Some("cpd: Worker question"), Place::Parked),
                    ],
                ),
                plan("sketch", "alpha", Phase::Plan, [4, 0, 0, 0, 0, 0], session("l-sketch", "sketch", Idle, None, Place::Parked), vec![]),
                plan("old", "alpha", Phase::Done, [0, 0, 0, 0, 1, 36], session("l-old", "  ", Stopped, None, Place::None), vec![]),
            ],
        ),
        context(
            "beta",
            vec![plan("rel", "beta", Phase::Pr, [0, 0, 0, 0, 0, 3], session("l-rel", "rel lead", Blocked, Some("cpd: Plan review"), Place::Parked), vec![])],
        ),
        context("gamma", vec![]),
    ])
}
