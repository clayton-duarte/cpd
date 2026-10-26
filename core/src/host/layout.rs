//! Sessions, windows and panes (contract §6.2, with commands.md §19
//! amendments 1 and 2: the chat slot rule and its placeholder).
//!
//! tmux targets are always pane or window ids; the names built here are
//! labels for people. Panes are identified by their `@cpd_*` options, never by
//! name or position, and the options carry identity, never state.

use crate::ids::{AttemptId, ContextId, PlanId, SessionId};

use super::tmux::Pane;

/// The cockpit's window holding the TUI views and the chat slot.
pub const MAIN_WINDOW: &str = "main";
/// The window of `cpd watch`, in the work session (D5).
pub const WATCH_WINDOW: &str = "watch";
/// The placeholder's text (amendment 2).
pub const PLACEHOLDER_TEXT: &str = "No Lead here: pick a Plan in the session tree";

/// A TUI view, the value of `@cpd_view` (contract §6.2, amendment 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum View {
    Tree,
    Plan,
    Widgets,
    /// The chat slot's placeholder.
    Chat,
    /// The chat slot showing an archived Session's history (`cpd session history`).
    History,
}

impl View {
    pub const ALL: [View; 5] = [View::Tree, View::Plan, View::Widgets, View::Chat, View::History];

    pub fn as_str(self) -> &'static str {
        match self {
            View::Tree => "tree",
            View::Plan => "plan",
            View::Widgets => "widgets",
            View::Chat => "chat",
            View::History => "history",
        }
    }

    pub fn parse(s: &str) -> Option<View> {
        View::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// The `@cpd_view` values of the panes of a cockpit built by an older cpd: the
/// StatusBar (moved to tmux's status line, contract §6.1) and the Worker pane
/// (removed with the Job pill's click). No longer [`View`]s: `list-panes` skips
/// such a pane quietly, and `cpd tui` kills it (commands.md §13.1).
pub const LEGACY_VIEWS: [&str; 2] = ["statusbar", "worker"];

/// A Session's label: its Plan id, or the first 8 characters of its id when it
/// has no Plan.
pub fn session_label(session: &SessionId, plan: Option<&PlanId>) -> String {
    match plan {
        Some(plan) => plan.to_string(),
        None => session.as_str().chars().take(8).collect(),
    }
}

/// `park-<label>`: a parked Lead's window in the cockpit session.
pub fn park_window(session: &SessionId, plan: Option<&PlanId>) -> String {
    format!("park-{}", session_label(session, plan))
}

/// `ctx-<context>`: a Context's Manager's window in the cockpit session.
pub fn context_window(context: &ContextId) -> String {
    format!("ctx-{context}")
}

/// `<plan>--<attempt>`: a Worker's window in the work session.
pub fn attempt_window(plan: &PlanId, attempt: &AttemptId) -> String {
    format!("{plan}--{attempt}")
}

/// Whether `pane` is in the cockpit's `main` window.
fn in_main(pane: &Pane, cockpit_session: &str) -> bool {
    pane.session_name == cockpit_session && pane.window_name == MAIN_WINDOW
}

/// The chat slot: the pane of `<cockpit>:main` that has `@cpd_session`, or
/// `@cpd_view` `chat` or `history` (amendment 1, amendment 28). Options move with a pane on `swap-pane`,
/// which is why the slot is found by this rule and not by a fixed option.
pub fn chat_slot<'a>(panes: &'a [Pane], cockpit_session: &str) -> Option<&'a Pane> {
    panes.iter().find(|p| {
        in_main(p, cockpit_session) && (p.cpd_session.is_some() || matches!(p.cpd_view, Some(View::Chat | View::History)))
    })
}

/// The pane of `<cockpit>:main` running `view` (not the chat placeholder,
/// which is found with [`chat_slot`]).
pub fn view_pane<'a>(panes: &'a [Pane], cockpit_session: &str, view: View) -> Option<&'a Pane> {
    panes.iter().find(|p| in_main(p, cockpit_session) && p.cpd_view == Some(view))
}

/// The pane running Session `session` (`@cpd_session`), wherever it is.
pub fn session_pane<'a>(panes: &'a [Pane], session: &SessionId) -> Option<&'a Pane> {
    panes.iter().find(|p| p.cpd_session.as_ref() == Some(session))
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;
    use crate::host::tmux::{PaneId, WindowId};
    use crate::ids::JobKey;

    fn pane(id: &str, session_name: &str, window: &str, session: Option<&str>, view: Option<View>) -> Pane {
        Pane {
            pane: PaneId::parse(id).unwrap(),
            window: WindowId::parse("@1").unwrap(),
            session_name: session_name.into(),
            window_name: window.into(),
            dead: false,
            dead_status: None,
            pid: Some(1),
            cpd_session: session.map(|s| SessionId::parse(s).unwrap()),
            cpd_view: view,
        }
    }

    #[test]
    fn names_windows() {
        let session = SessionId::parse("596d007f-1111-4222-8333-444455556666").unwrap();
        let plan = PlanId::parse("cpd2").unwrap();
        assert_eq!(park_window(&session, Some(&plan)), "park-cpd2");
        assert_eq!(park_window(&session, None), "park-596d007f");
        assert_eq!(context_window(&ContextId::parse("building-cpd").unwrap()), "ctx-building-cpd");
        let attempt = AttemptId::new(JobKey::parse("rs-host").unwrap(), NonZeroU32::new(1).unwrap());
        assert_eq!(attempt_window(&plan, &attempt), "cpd2--rs-host-a1");
    }

    #[test]
    fn views_round_trip() {
        for view in View::ALL {
            assert_eq!(View::parse(view.as_str()), Some(view));
        }
        assert_eq!(View::parse("lead"), None);
    }

    #[test]
    fn the_chat_slot_is_the_main_pane_with_a_session_or_the_placeholder() {
        let worker = pane("%5", "cpd", "main", None, Some(View::Widgets));
        let tree = pane("%1", "cpd", "main", None, Some(View::Tree));
        let parked = pane("%9", "cpd", "park-x", Some("lead-b"), None);
        let placeholder = pane("%3", "cpd", "main", None, Some(View::Chat));
        let lead = pane("%7", "cpd", "main", Some("lead-a"), None);
        let other = pane("%8", "other", "main", Some("lead-c"), None);

        let panes = vec![worker.clone(), tree.clone(), parked.clone(), other.clone(), placeholder.clone()];
        assert_eq!(chat_slot(&panes, "cpd"), Some(&placeholder));
        let panes = vec![worker.clone(), tree.clone(), parked.clone(), other.clone(), lead.clone()];
        assert_eq!(chat_slot(&panes, "cpd"), Some(&lead));
        let history = pane("%4", "cpd", "main", None, Some(View::History));
        let panes = vec![worker.clone(), tree.clone(), parked.clone(), other.clone(), history.clone()];
        assert_eq!(chat_slot(&panes, "cpd"), Some(&history));
        assert_eq!(chat_slot(&[worker, tree.clone(), parked.clone(), other], "cpd"), None);

        assert_eq!(view_pane(&[parked.clone(), tree.clone()], "cpd", View::Tree), Some(&tree));
        assert_eq!(session_pane(&[tree, parked.clone()], &SessionId::parse("lead-b").unwrap()), Some(&parked));
    }
}
