//! The dot of a row (commands.md §17, "The tree's dot rule"): the filled `●` comes only from the open, visible
//! Inbox items, in the colour of the most urgent one; else the glyph of the Session's state. Pure functions, with
//! the precedence as the `Ord` of the result.

use cpd_core::ids::{PlanId, SessionId};
use cpd_core::inbox::model::{Item, Kind, Urgency};
use cpd_core::plan::Phase;
use cpd_core::session::State;

/// What a working Lead is working on: its spinner colour (planning magenta 13, dispatch cyan 14, review yellow 11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Work {
    Planning,
    Dispatch,
    Review,
}

impl Work {
    /// What a Plan's Lead works on (pro-d200): drafts -> Planning; else git (or phase Deploy) -> Review; else
    /// Grounding, Plan or Done -> Planning; else Dispatch.
    pub fn of(phase: Phase, drafts: bool, git: bool) -> Work {
        if drafts {
            Work::Planning
        } else if git || phase == Phase::Deploy {
            Work::Review
        } else if phase == Phase::Dispatch {
            Work::Dispatch
        } else {
            Work::Planning
        }
    }
}

/// The dot of a row, ordered by urgency: the greatest is the most urgent, `Red > Yellow > Magenta > Cyan > White >
/// Working > Waiting > Idle > Stopped` (§17, Precedence); `Magenta > Cyan > White` since hs-d570.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dot {
    /// `palette::STOPPED`: pi stopped.
    Stopped,
    /// `palette::IDLE`: idle, nothing needed.
    Idle,
    /// `palette::WAITING` in `Meaning::Waiting`: the Session waits for another agent (a Worker for its Lead, a Lead
    /// for a Worker), not for the User (hs-d303). Never red, never a filled dot.
    Waiting,
    /// `palette::SPINNER`: the Lead is working; `Some(phase)` is the phase of the Plan it leads, `None` for a Session
    /// that leads no Plan (a fork, a Manager, a Worker), drawn cyan (hs-d622).
    Working(Option<Work>),
    /// The red loader: the Plan has a failed Job being worked on (pro-d88), whatever its Lead's state.
    Fixing,
    /// `palette::NEEDS_YOU` white: an Inbox item of urgency `white`.
    White,
    /// `palette::NEEDS_YOU` cyan: an Inbox item of urgency `cyan` (hs-d570).
    Cyan,
    /// `palette::NEEDS_YOU` magenta.
    Magenta,
    /// `palette::NEEDS_YOU` yellow.
    Yellow,
    /// `palette::NEEDS_YOU` red.
    Red,
}

/// The key [`Dot::order_key`] returns.
pub type OrderKey = (u8, std::cmp::Reverse<String>);

impl Dot {
    /// The Session needs the User: a `palette::NEEDS_YOU` of any colour.
    pub fn needs_you(self) -> bool {
        self >= Dot::White
    }

    /// The dot-urgency tier the tree orders by (hs-d686): red 0, yellow 1, magenta 2, cyan 3, and 4 for every dot
    /// that needs no attention (white, the red loader `Fixing`, and the state glyphs). The red loader is a row with
    /// no Inbox item (A1), so it sits below the four coloured dots.
    pub fn tier(self) -> u8 {
        match self {
            Dot::Red => 0,
            Dot::Yellow => 1,
            Dot::Magenta => 2,
            Dot::Cyan => 3,
            Dot::White | Dot::Working(_) | Dot::Fixing | Dot::Waiting | Dot::Idle | Dot::Stopped => 4,
        }
    }

    /// The tree's order key (hs-d686, pro-d48): the dot's tier, then creation, newest first (`created_at` is
    /// fixed-width UTC text, so it compares as text). Sort stably: a tie keeps the read model's order.
    pub fn order_key(self, created_at: &str) -> OrderKey {
        (self.tier(), std::cmp::Reverse(created_at.to_owned()))
    }

    /// The dot of a working Lead: the red loader when its Plan has a failed Job being worked on (pro-d88), else its
    /// Plan's phase (`None`: no Plan, cyan).
    pub fn working(failed: bool, work: Option<Work>) -> Dot {
        if failed { Dot::Fixing } else { Dot::Working(work) }
    }

    /// The glyph of a state, for a row with no item: the spinner, `palette::WAITING`, `palette::IDLE` or
    /// `palette::STOPPED`.
    pub fn glyph(state: State) -> Dot {
        match state {
            State::Working => Dot::Working(None),
            State::Stopped => Dot::Stopped,
            State::Waiting => Dot::Waiting,
            State::Idle | State::Blocked => Dot::Idle,
        }
    }

    /// The dot of a Session in `state` whose most urgent open, visible item has urgency `item`. `failed` is true for
    /// a Plan's own Lead whose Plan has a failed Job being worked on (pro-d88, `PlanHead::fixing`) and decides alone:
    /// the red dot when the item is red, else the red loader, whatever the state. Otherwise own state first
    /// (hs-d183): a working Session shows the spinner whatever its items; any other state shows its most urgent
    /// item, else its glyph. `work` is what the Plan the Session leads (`None` for any other Session); only a
    /// working Session uses it.
    pub fn of(state: State, item: Option<Urgency>, failed: bool, work: Option<Work>) -> Dot {
        if failed {
            return if item == Some(Urgency::Red) { Dot::Red } else { Dot::Fixing };
        }
        if state == State::Working {
            return Dot::working(false, work);
        }
        item.and_then(Dot::from_urgency).unwrap_or_else(|| Dot::glyph(state))
    }
}

impl Dot {
    /// The dot of an Inbox urgency; `None` for blue, which fills no Session dot (B4): no `Dot::Blue` exists.
    pub fn from_urgency(urgency: Urgency) -> Option<Dot> {
        match urgency {
            Urgency::Red => Some(Dot::Red),
            Urgency::Yellow => Some(Dot::Yellow),
            Urgency::Magenta => Some(Dot::Magenta),
            Urgency::White => Some(Dot::White),
            Urgency::Cyan => Some(Dot::Cyan),
            Urgency::Blue => None,
        }
    }
}

/// The kinds a Plan's own Lead shows besides the ones linked to it (§17, "Which items fill which row").
const PLAN_KINDS: [Kind; 3] = [Kind::PlanReview, Kind::JobStuck, Kind::DraftsParked];

/// The urgency of the most urgent of the `visible` items that fill the dot of `session`: its `dialog`, `unread` and `review-waiting`
/// items (`inbox_items.session`) and, when it is the Lead of `lead_of`, the Plan's `plan-review`, `job-stuck` and
/// `pr-ready` items, the last only when yellow or red (§17; `pr-failing` and `pr-changes` are no longer opened and fill no dot, hs-d470). `visible` is the open items that are neither snoozed nor dismissed.
pub fn urgency(visible: &[Item], session: &SessionId, lead_of: Option<&PlanId>) -> Option<Urgency> {
    visible
        .iter()
        .filter(|i| match i.kind {
            Kind::Dialog | Kind::Unread | Kind::ReviewWaiting | Kind::LoopGuard => i.session.as_ref() == Some(session),
            // a cyan or white `pr-ready` item fills no dot (pro-d130)
            Kind::PrReady => lead_of.is_some() && i.plan.as_ref() == lead_of && matches!(i.urgency, Urgency::Yellow | Urgency::Red),
            k => lead_of.is_some() && i.plan.as_ref() == lead_of && PLAN_KINDS.contains(&k),
        })
        .map(|i| i.urgency)
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: Kind, urgency: Urgency, session: Option<&str>, plan: Option<&str>) -> Item {
        Item {
            id: 1,
            kind,
            urgency,
            subject: "s".into(),
            label: "l".into(),
            url: None,
            context: None,
            plan: plan.map(|p| PlanId::parse(p).unwrap()),
            job: None,
            session: session.map(|s| SessionId::parse(s).unwrap()),
            record: None,
            event: None,
            opened_at: "2026-01-01T00:00:00.000Z".into(),
            closed_at: None,
            snoozed_until: None,
            dismissed_at: None,
            claimed_at: None,
        }
    }

    fn id(s: &str) -> SessionId {
        SessionId::parse(s).unwrap()
    }

    fn plan(p: &str) -> PlanId {
        PlanId::parse(p).unwrap()
    }

    #[test]
    fn the_glyphs_without_an_item() {
        assert_eq!(Dot::of(State::Working, None, false, None), Dot::Working(None));
        assert_eq!(Dot::of(State::Idle, None, false, None), Dot::Idle);
        assert_eq!(Dot::of(State::Stopped, None, false, None), Dot::Stopped);
        assert_eq!(Dot::of(State::Blocked, None, false, None), Dot::Idle, "blocked is no dot of its own: its dialog item is");
        assert_eq!(Dot::of(State::Waiting, None, false, None), Dot::Waiting, "a Session waiting for an agent shows the waiting dot, not idle");
        for state in [State::Working, State::Idle, State::Waiting, State::Blocked, State::Stopped] {
            assert!(!Dot::of(state, None, false, None).needs_you(), "no item, never a filled dot: {state:?}");
        }
    }

    #[test]
    fn an_item_gives_its_urgency_colour_unless_the_session_is_working() {
        for urgency in [Urgency::Red, Urgency::Yellow, Urgency::Magenta, Urgency::Cyan, Urgency::White] {
            assert_eq!(Dot::of(State::Working, Some(urgency), false, None), Dot::Working(None), "own state first (hs-d183): {urgency:?}");
        }
        for state in [State::Idle, State::Waiting, State::Blocked, State::Stopped] {
            for (urgency, dot) in
                [(Urgency::Red, Dot::Red), (Urgency::Yellow, Dot::Yellow), (Urgency::Magenta, Dot::Magenta), (Urgency::Cyan, Dot::Cyan), (Urgency::White, Dot::White)]
            {
                assert_eq!(Dot::of(state, Some(urgency), false, None), dot, "{state:?} {urgency:?}");
                assert!(dot.needs_you());
            }
        }
    }

    #[test]
    fn a_session_shows_its_dialog_and_unread_items_only() {
        let a = id("s-a");
        let items = [
            item(Kind::Unread, Urgency::White, Some("s-a"), Some("pp")),
            item(Kind::Dialog, Urgency::Magenta, Some("s-b"), Some("pp")),
        ];
        assert_eq!(urgency(&items, &a, None), Some(Urgency::White));
        assert_eq!(urgency(&items, &id("s-b"), Some(&plan("pp"))), Some(Urgency::Magenta));
        assert_eq!(urgency(&items, &id("s-c"), None), None, "an item linked to another Session");
        assert_eq!(urgency(&[], &a, Some(&plan("pp"))), None, "no item, no dot");
    }

    #[test]
    fn the_most_urgent_item_wins() {
        let items = [
            item(Kind::Unread, Urgency::White, Some("s-a"), None),
            item(Kind::Dialog, Urgency::Red, Some("s-a"), None),
            item(Kind::Dialog, Urgency::Magenta, Some("s-a"), None),
        ];
        assert_eq!(urgency(&items, &id("s-a"), None), Some(Urgency::Red));
    }

    #[test]
    fn a_plans_items_fill_its_lead_only() {
        let kinds = [
            (Kind::PlanReview, Urgency::Magenta),
            (Kind::JobStuck, Urgency::Red),
            (Kind::PrReady, Urgency::Yellow),
        ];
        for (kind, u) in kinds {
            let items = [item(kind, u, None, Some("pp"))];
            assert_eq!(urgency(&items, &id("s-lead"), Some(&plan("pp"))), Some(u), "the Lead of pp: {kind:?}");
            assert_eq!(urgency(&items, &id("s-fork"), None), None, "a fork: {kind:?}");
            assert_eq!(urgency(&items, &id("s-lead"), Some(&plan("qq"))), None, "the Lead of another Plan: {kind:?}");
        }
    }

    /// A Plan-owned pull request's failure reaches its Lead as `pr-feedback`, never red (hs-d302); the kinds are for
    /// the ones no Plan owns, which belong to no Plan.
    #[test]
    fn pr_failing_and_pr_changes_never_fill_a_plans_lead() {
        for kind in [Kind::PrFailing, Kind::PrChanges] {
            let items = [item(kind, Urgency::Red, None, Some("pp"))];
            assert_eq!(urgency(&items, &id("s-lead"), Some(&plan("pp"))), None, "{kind:?}");
        }
    }

    /// hs-d470: the dropped kinds (an older engine's history rows) fill no dot, whoever they name.
    #[test]
    fn pr_failing_and_pr_changes_rows_fill_no_dot_at_all() {
        for kind in [Kind::PrFailing, Kind::PrChanges] {
            for (session, plan) in [(None, None), (None, Some("pp")), (Some("s-lead"), None), (Some("s-lead"), Some("pp"))] {
                let items = [item(kind, Urgency::Red, session, plan)];
                let p = plan.map(self::plan);
                assert_eq!(urgency(&items, &id("s-lead"), p.as_ref()), None, "{kind:?} {session:?} {plan:?}");
            }
        }
    }

    #[test]
    fn items_that_belong_to_no_session_fill_no_dot() {
        let items = [
            item(Kind::EngineError, Urgency::Red, None, None),
            item(Kind::Question, Urgency::Magenta, None, None),
            item(Kind::ReviewRequested, Urgency::Yellow, None, Some("pp")),
        ];
        assert_eq!(urgency(&items, &id("s-lead"), Some(&plan("pp"))), None);
    }

    /// hs-d570 BL5: a Session whose strongest open item is cyan shows the cyan dot, between magenta and white.
    #[test]
    fn a_cyan_item_gives_its_session_a_cyan_dot_between_magenta_and_white() {
        assert_eq!(Dot::from_urgency(Urgency::Cyan), Some(Dot::Cyan));
        assert_eq!(Dot::from_urgency(Urgency::Blue), None, "blue fills no dot");
        assert_eq!(Dot::of(State::Idle, Some(Urgency::Cyan), false, None), Dot::Cyan);
        assert!(Dot::Cyan.needs_you());
        assert!(Dot::Magenta > Dot::Cyan && Dot::Cyan > Dot::White);
        let unread = item(Kind::Unread, Urgency::White, Some("s-lead"), None);
        let dialog = item(Kind::Dialog, Urgency::Magenta, Some("s-lead"), None);
        let cyan_dialog = item(Kind::Dialog, Urgency::Cyan, Some("s-lead"), None);
        assert_eq!(urgency(&[unread.clone(), cyan_dialog.clone()], &id("s-lead"), None), Some(Urgency::Cyan), "cyan beats white");
        assert_eq!(urgency(&[dialog, cyan_dialog], &id("s-lead"), None), Some(Urgency::Magenta), "magenta beats cyan");
    }

    /// hs-d581: a working Lead of a Plan with a failed Job shows the red loader; its own red item keeps the red dot;
    /// only a working Session is changed.
    #[test]
    fn a_working_lead_of_a_plan_with_a_failed_job_shows_the_red_loader_unless_its_item_is_red() {
        assert_eq!(Dot::of(State::Working, None, true, None), Dot::Fixing);
        for urgency in [Urgency::Yellow, Urgency::Magenta, Urgency::Cyan, Urgency::White] {
            assert_eq!(Dot::of(State::Working, Some(urgency), true, None), Dot::Fixing, "{urgency:?}");
        }
        assert_eq!(Dot::of(State::Working, Some(Urgency::Red), true, None), Dot::Red);
        for state in [State::Idle, State::Waiting, State::Stopped] {
            assert_eq!(Dot::of(state, None, true, None), Dot::Fixing, "pro-d88: whatever the state, {state:?}");
        }
    }

    #[test]
    fn the_order_is_the_precedence() {
        let order = [Dot::Red, Dot::Yellow, Dot::Magenta, Dot::Cyan, Dot::White, Dot::Fixing, Dot::Working(None), Dot::Waiting, Dot::Idle, Dot::Stopped];
        assert!(order.windows(2).all(|w| w[0] > w[1]), "red > yellow > magenta > cyan > white > fixing > working > waiting > idle > stopped");
        assert_eq!(order.iter().copied().max(), Some(Dot::Red));
        assert!(!Dot::Fixing.needs_you());
        assert_eq!(order.iter().filter(|d| d.needs_you()).count(), 5, "the needs-you dots are the five colours");
    }

    const PHASES: [Phase; 4] = [Phase::Grounding, Phase::Plan, Phase::Dispatch, Phase::Done];

    fn all_phases() -> Vec<Phase> {
        let mut v = PHASES.to_vec();
        v.push(Phase::Deploy);
        v
    }

    #[test]
    fn dots_work_of_drafts_beat_every_phase_and_git() {
        for phase in all_phases() {
            for git in [false, true] {
                assert_eq!(Work::of(phase, true, git), Work::Planning, "{phase:?} git={git}");
            }
        }
    }

    #[test]
    fn dots_work_of_git_or_deploy_is_review() {
        for phase in all_phases() {
            assert_eq!(Work::of(phase, false, true), Work::Review, "{phase:?} with git");
        }
        assert_eq!(Work::of(Phase::Deploy, false, false), Work::Review, "deploy alone");
    }

    #[test]
    fn dots_work_of_phase_alone() {
        for phase in [Phase::Grounding, Phase::Plan, Phase::Done] {
            assert_eq!(Work::of(phase, false, false), Work::Planning, "{phase:?}");
        }
        assert_eq!(Work::of(Phase::Dispatch, false, false), Work::Dispatch);
    }

    #[test]
    fn dots_rule_failed_is_red_loader_and_no_plan_is_white() {
        for work in [None, Some(Work::Planning), Some(Work::Dispatch), Some(Work::Review)] {
            assert_eq!(Dot::working(true, work), Dot::Fixing, "{work:?}");
            assert_eq!(Dot::working(false, work), Dot::Working(work), "{work:?}");
            assert_eq!(Dot::of(State::Working, None, true, work), Dot::Fixing, "{work:?}");
            assert_eq!(Dot::of(State::Working, Some(Urgency::Red), true, work), Dot::Red, "own red item: {work:?}");
            assert_eq!(Dot::of(State::Working, Some(Urgency::Yellow), false, work), Dot::Working(work), "{work:?}");
            for state in [State::Idle, State::Waiting, State::Blocked, State::Stopped] {
                assert_eq!(Dot::of(state, None, false, work), Dot::glyph(state), "a non-working state ignores work: {state:?}");
            }
        }
        assert_eq!(Dot::of(State::Working, None, false, None), Dot::Working(None), "no Plan: white");
        for work in [Work::Planning, Work::Dispatch, Work::Review] {
            assert!(!Dot::Working(Some(work)).needs_you());
        }
        // the dot of a Plan of drafts and an open PR is planning, whatever the phase: through the one rule
        assert_eq!(Dot::of(State::Working, None, false, Some(Work::of(Phase::Dispatch, true, true))), Dot::Working(Some(Work::Planning)));
    }
}
