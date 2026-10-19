//! The dot of a row (commands.md §17, "The tree's dot rule"): the filled `●` comes only from the open, visible
//! Inbox items, in the colour of the most urgent one; else the glyph of the Session's state. Pure functions, with
//! the precedence as the `Ord` of the result.

use cpd_core::ids::{PlanId, SessionId};
use cpd_core::inbox::model::{Item, Kind, Urgency};
use cpd_core::session::State;

/// The dot of a row, ordered by urgency: the greatest is the most urgent, `Red > Yellow > Magenta > White >
/// Working > Idle > Stopped` (§17, Precedence).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dot {
    /// `palette::STOPPED`: pi stopped.
    Stopped,
    /// `palette::IDLE`: idle, nothing needed.
    Idle,
    /// `palette::SPINNER`: the Lead is working.
    Working,
    /// `palette::NEEDS_YOU` white: an Inbox item of urgency `white`.
    White,
    /// `palette::NEEDS_YOU` magenta.
    Magenta,
    /// `palette::NEEDS_YOU` yellow.
    Yellow,
    /// `palette::NEEDS_YOU` red.
    Red,
}

impl Dot {
    /// The Session needs the User: a `palette::NEEDS_YOU` of any colour.
    pub fn needs_you(self) -> bool {
        self >= Dot::White
    }

    /// The glyph of a state, for a row with no item: the spinner, `palette::IDLE` or `palette::STOPPED`.
    pub fn glyph(state: State) -> Dot {
        match state {
            State::Working => Dot::Working,
            State::Stopped => Dot::Stopped,
            State::Working | State::Idle | State::Blocked => Dot::Idle,
        }
    }

    /// The dot of a Session in `state` whose most urgent open, visible item has urgency `item`. Own state first
    /// (hs-d183): a working Session shows the spinner whatever its items, so a Lead at work is never yellow, magenta
    /// or white; any other state shows its most urgent item, else its glyph.
    pub fn of(state: State, item: Option<Urgency>) -> Dot {
        if state == State::Working {
            return Dot::Working;
        }
        item.map_or_else(|| Dot::glyph(state), Dot::from)
    }
}

impl From<Urgency> for Dot {
    fn from(urgency: Urgency) -> Dot {
        match urgency {
            Urgency::Red => Dot::Red,
            Urgency::Yellow => Dot::Yellow,
            Urgency::Magenta => Dot::Magenta,
            Urgency::White => Dot::White,
        }
    }
}

/// The kinds a Plan's own Lead shows besides the ones linked to it (§17, "Which items fill which row").
const PLAN_KINDS: [Kind; 5] = [Kind::PlanReview, Kind::JobStuck, Kind::PrFailing, Kind::PrChanges, Kind::PrReady];

/// The urgency of the most urgent of the `visible` items that fill the dot of `session`: its `dialog` and `unread`
/// items (`inbox_items.session`) and, when it is the Lead of `lead_of`, the Plan's `plan-review`, `job-stuck` and
/// `pr-*` items (§17). `visible` is the open items that are neither snoozed nor dismissed.
pub fn urgency(visible: &[Item], session: &SessionId, lead_of: Option<&PlanId>) -> Option<Urgency> {
    visible
        .iter()
        .filter(|i| match i.kind {
            Kind::Dialog | Kind::Unread => i.session.as_ref() == Some(session),
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
        assert_eq!(Dot::of(State::Working, None), Dot::Working);
        assert_eq!(Dot::of(State::Idle, None), Dot::Idle);
        assert_eq!(Dot::of(State::Stopped, None), Dot::Stopped);
        assert_eq!(Dot::of(State::Blocked, None), Dot::Idle, "blocked is no dot of its own: its dialog item is");
        for state in [State::Working, State::Idle, State::Blocked, State::Stopped] {
            assert!(!Dot::of(state, None).needs_you(), "no item, never a filled dot: {state:?}");
        }
    }

    #[test]
    fn an_item_gives_its_urgency_colour_unless_the_session_is_working() {
        for urgency in [Urgency::Red, Urgency::Yellow, Urgency::Magenta, Urgency::White] {
            assert_eq!(Dot::of(State::Working, Some(urgency)), Dot::Working, "own state first (hs-d183): {urgency:?}");
        }
        for state in [State::Idle, State::Blocked, State::Stopped] {
            for (urgency, dot) in
                [(Urgency::Red, Dot::Red), (Urgency::Yellow, Dot::Yellow), (Urgency::Magenta, Dot::Magenta), (Urgency::White, Dot::White)]
            {
                assert_eq!(Dot::of(state, Some(urgency)), dot, "{state:?} {urgency:?}");
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
            (Kind::PrFailing, Urgency::Red),
            (Kind::PrChanges, Urgency::Red),
            (Kind::PrReady, Urgency::Yellow),
        ];
        for (kind, u) in kinds {
            let items = [item(kind, u, None, Some("pp"))];
            assert_eq!(urgency(&items, &id("s-lead"), Some(&plan("pp"))), Some(u), "the Lead of pp: {kind:?}");
            assert_eq!(urgency(&items, &id("s-fork"), None), None, "a fork: {kind:?}");
            assert_eq!(urgency(&items, &id("s-lead"), Some(&plan("qq"))), None, "the Lead of another Plan: {kind:?}");
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

    #[test]
    fn the_order_is_the_precedence() {
        let order = [Dot::Red, Dot::Yellow, Dot::Magenta, Dot::White, Dot::Working, Dot::Idle, Dot::Stopped];
        assert!(order.windows(2).all(|w| w[0] > w[1]), "red > yellow > magenta > white > working > idle > stopped");
        assert_eq!(order.iter().copied().max(), Some(Dot::Red));
        assert_eq!(order.iter().filter(|d| d.needs_you()).count(), 4, "the needs-you dots are the four colours");
    }
}
