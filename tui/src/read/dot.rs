//! The dot of a Session row (commands.md §17, "The tree's dot rule"): one pure function of the Session's state
//! and of the facts about its Plan, with the precedence as the `Ord` of the result.

use cpd_core::plan::Phase;
use cpd_core::session::State;

use super::shared::Place;

/// The dot of a row, ordered by urgency: the greatest is the most urgent, `Red > Yellow > Magenta > White >
/// Working > Idle > Stopped` (§17, Precedence). A Context row's dot is the greatest of its live Sessions'.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dot {
    /// `◌`: pi stopped.
    Stopped,
    /// `○`: idle, nothing needed.
    Idle,
    /// `◐◓◑◒`: the Lead is working.
    Working,
    /// `●` white: an idle turn the User has not seen.
    White,
    /// `●` magenta: blocked outside dispatch and PR, or the Plan's review is pending.
    Magenta,
    /// `●` yellow: the Plan's Lead, with a pull request open whose checks have finished.
    Yellow,
    /// `●` red: blocked work in dispatch or PR.
    Red,
}

impl Dot {
    /// The Session needs the User: a `●` of any colour.
    pub fn needs_you(self) -> bool {
        self >= Dot::White
    }
}

/// What the dot of one Session depends on.
#[derive(Clone, Copy, Debug)]
pub struct DotInputs {
    pub state: State,
    pub place: Place,
    /// The id of its latest `session-state` Event with `to: "idle"`.
    pub settled: Option<i64>,
    /// The id of its latest `session-moved` Event with `place: "chat"`.
    pub seen: Option<i64>,
    /// It is its Plan's own Lead (never a fork).
    pub plan_lead: bool,
    /// Its Plan's phase; none for a Session without a Plan.
    pub phase: Option<Phase>,
    /// Its Plan's review is pending.
    pub review_pending: bool,
    /// At least one of its Plan's pull requests is `open` with checks `passed` or `failed`.
    pub pr_checked: bool,
}

impl Dot {
    pub fn of(i: DotInputs) -> Dot {
        let blocked = i.state == State::Blocked;
        if blocked && matches!(i.phase, Some(Phase::Dispatch | Phase::Pr)) {
            return Dot::Red;
        }
        if i.plan_lead && i.phase == Some(Phase::Pr) && i.pr_checked {
            return Dot::Yellow;
        }
        if blocked || (i.plan_lead && i.review_pending) {
            return Dot::Magenta;
        }
        // Event ids order commits; never in the slot counts as older than everything.
        if i.state == State::Idle && i.place != Place::Chat && i.settled.unwrap_or(0) > i.seen.unwrap_or(0) {
            return Dot::White;
        }
        match i.state {
            State::Working => Dot::Working,
            State::Stopped => Dot::Stopped,
            State::Idle | State::Blocked => Dot::Idle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An idle Lead of a Plan in `phase`, nothing pending, never settled.
    fn lead(state: State, phase: Option<Phase>) -> DotInputs {
        DotInputs {
            state,
            place: Place::Parked,
            settled: None,
            seen: None,
            plan_lead: phase.is_some(),
            phase,
            review_pending: false,
            pr_checked: false,
        }
    }

    fn fork(state: State, phase: Phase) -> DotInputs {
        DotInputs { plan_lead: false, ..lead(state, Some(phase)) }
    }

    #[test]
    fn the_glyphs_without_a_colour() {
        assert_eq!(Dot::of(lead(State::Working, Some(Phase::Dispatch))), Dot::Working);
        assert_eq!(Dot::of(lead(State::Idle, Some(Phase::Dispatch))), Dot::Idle);
        assert_eq!(Dot::of(lead(State::Stopped, Some(Phase::Dispatch))), Dot::Stopped);
        assert_eq!(Dot::of(lead(State::Idle, None)), Dot::Idle);
    }

    #[test]
    fn blocked_is_red_in_dispatch_and_pr() {
        for phase in [Phase::Dispatch, Phase::Pr] {
            assert_eq!(Dot::of(lead(State::Blocked, Some(phase))), Dot::Red);
            assert_eq!(Dot::of(fork(State::Blocked, phase)), Dot::Red, "a fork too");
        }
    }

    #[test]
    fn blocked_is_magenta_in_grounding_plan_done_and_without_a_plan() {
        for phase in [Some(Phase::Grounding), Some(Phase::Plan), Some(Phase::Done), None] {
            assert_eq!(Dot::of(lead(State::Blocked, phase)), Dot::Magenta, "{phase:?}");
        }
    }

    #[test]
    fn a_pending_review_is_magenta_on_the_plans_lead_only() {
        let pending = |i: DotInputs| DotInputs { review_pending: true, ..i };
        for state in [State::Working, State::Idle, State::Stopped] {
            assert_eq!(Dot::of(pending(lead(state, Some(Phase::Plan)))), Dot::Magenta, "{state:?}");
        }
        assert_eq!(Dot::of(pending(fork(State::Idle, Phase::Plan))), Dot::Idle, "not on a fork");
    }

    #[test]
    fn yellow_is_the_plans_lead_in_pr_with_a_pull_request_open_and_checked() {
        let checked = |i: DotInputs| DotInputs { pr_checked: true, ..i };
        for state in [State::Working, State::Idle, State::Stopped] {
            assert_eq!(Dot::of(checked(lead(state, Some(Phase::Pr)))), Dot::Yellow, "{state:?}");
        }
        assert_eq!(Dot::of(checked(fork(State::Working, Phase::Pr))), Dot::Working, "never a fork");
        assert_eq!(Dot::of(lead(State::Idle, Some(Phase::Pr))), Dot::Idle, "it clears when no such pull request remains");
        for phase in [Phase::Grounding, Phase::Plan, Phase::Dispatch, Phase::Done] {
            assert_ne!(Dot::of(checked(lead(State::Idle, Some(phase)))), Dot::Yellow, "only in pr: {phase:?}");
        }
    }

    #[test]
    fn white_is_an_idle_turn_not_seen_outside_the_slot() {
        let idle = |place, settled, seen| DotInputs { place, settled, seen, ..lead(State::Idle, Some(Phase::Dispatch)) };
        assert_eq!(Dot::of(idle(Place::Parked, Some(5), None)), Dot::White, "never in the slot is older than everything");
        assert_eq!(Dot::of(idle(Place::Parked, Some(5), Some(4))), Dot::White, "idle again after being seen");
        assert_eq!(Dot::of(idle(Place::Parked, Some(5), Some(6))), Dot::Idle, "seen after it settled");
        assert_eq!(Dot::of(idle(Place::Parked, Some(5), Some(5))), Dot::Idle);
        assert_eq!(Dot::of(idle(Place::Parked, None, None)), Dot::Idle, "no turn");
        assert_eq!(Dot::of(idle(Place::Chat, Some(5), None)), Dot::Idle, "in the slot");
        let working = DotInputs { settled: Some(5), ..lead(State::Working, Some(Phase::Dispatch)) };
        assert_eq!(Dot::of(working), Dot::Working, "only an idle Session is white");
    }

    #[test]
    fn red_beats_yellow_beats_magenta_beats_white() {
        let all = DotInputs { settled: Some(5), review_pending: true, pr_checked: true, ..lead(State::Blocked, Some(Phase::Pr)) };
        assert_eq!(Dot::of(all), Dot::Red, "blocked in pr with a finished-checks PR");
        assert_eq!(Dot::of(DotInputs { state: State::Idle, ..all }), Dot::Yellow, "an idle Lead in pr with passed checks");
        assert_eq!(Dot::of(DotInputs { pr_checked: false, state: State::Idle, ..all }), Dot::Magenta, "the review beats white");
        assert_eq!(Dot::of(DotInputs { pr_checked: false, review_pending: false, state: State::Idle, ..all }), Dot::White);
    }

    #[test]
    fn the_order_is_the_precedence() {
        let order = [Dot::Red, Dot::Yellow, Dot::Magenta, Dot::White, Dot::Working, Dot::Idle, Dot::Stopped];
        assert!(order.windows(2).all(|w| w[0] > w[1]), "red > yellow > magenta > white > ◐ > ○ > ◌");
        assert_eq!(order.iter().copied().max(), Some(Dot::Red));
        assert_eq!(order.iter().filter(|d| d.needs_you()).count(), 4, "the ● are the four colours");
    }
}
