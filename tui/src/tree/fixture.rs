//! Hand-built trees for the unit tests: no store, so a test names only what it looks at.
//! (The prepared store of `tests/tui_tree.rs` goes through the real read model.)

use cpd_core::config::Tier;
use cpd_core::ids::{ContextId, PlanId, SessionId};
use cpd_core::plan::Phase;
use cpd_core::session::{Role, State};
use crate::read::{ContextNode, Dot, Place, PlanHead, PlanNode, SessionNode, Tree};
use crate::read::JobCounts;

/// The title of a Context or Plan `id`: the id with a capital, so that a test can tell a title from an id and a name.
fn titled(id: &str) -> String {
    let mut chars = id.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

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
        dot: match state {
            State::Blocked => Dot::Red,
            State::Working => Dot::Working,
            State::Idle => Dot::Idle,
            State::Stopped => Dot::Stopped,
        },
        archived_at: None,
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
            title: titled(id),
            jira: None,
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
            archived_at: None,
        },
        sessions,
    }
}

/// The dot of a Context, as the read model takes it: the most urgent of its live (not archived) Sessions.
fn live_dot(plans: &[PlanNode]) -> Dot {
    plans.iter().flat_map(|p| &p.sessions).filter(|s| !s.is_archived()).map(|s| s.dot).max().unwrap_or(Dot::Stopped)
}

/// Take every Context's dot again, after a test archived some of its Sessions.
pub fn refresh_dots(t: &mut Tree) {
    for c in &mut t.contexts {
        c.dot = live_dot(&c.plans);
    }
}

pub fn context(id: &str, plans: Vec<PlanNode>) -> ContextNode {
    ContextNode {
        dot: live_dot(&plans),
        id: ContextId::parse(id).unwrap(),
        title: titled(id),
        jira: None,
        grounded_lead: None,
        records: 0,
        open_questions: 0,
        sync_failing: false,
        archived_at: None,
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

pub const WHEN: &str = "2026-02-01T00:00:00.000Z";

    /// `sample()` with archived objects: in `alpha`, the fork `f-blocked` of `ship` and the Plan `old`; and a
    /// whole archived Context `zeta` with its Plan `zp` (a Lead and a fork).
pub fn archived() -> Tree {
        let mut t = sample();
        t.contexts[0].plans[0].sessions[2].archived_at = Some(WHEN.into());
        t.contexts[0].plans[2].head.archived_at = Some(WHEN.into());
        t.contexts[0].plans[2].sessions[0].archived_at = Some(WHEN.into()); // archiving a Plan archives its Sessions
        refresh_dots(&mut t);
        let mut zp = plan("zp", "zeta", Phase::Dispatch, [0, 0, 0, 0, 0, 2], session("l-zp", "zp lead", State::Stopped, None, Place::None), vec![session("f-zp", "zp fork", State::Stopped, None, Place::None)]);
        zp.head.archived_at = Some(WHEN.into());
        for s in &mut zp.sessions {
            s.archived_at = Some(WHEN.into());
        }
        let mut zeta = context("zeta", vec![zp]);
        zeta.archived_at = Some(WHEN.into());
        t.contexts.push(zeta);
        t
    }
