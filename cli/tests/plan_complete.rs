//! Tests for plan completion, draft dependency validation, outline done_when phase enforcement,
//! done refusal, plan check, and plan reopen (b17, pro-d549, pro-d550).

#[path = "common/mod.rs"]
mod common;

pub mod plan_complete {
    #[test]
    fn draft_without_waiting_on_is_refused() {
        panic!("stub");
    }

    #[test]
    fn plan_lock_refuses_unregistered_done_when_phases() {
        panic!("stub");
    }

    #[test]
    fn plan_done_refuses_unregistered_declared_phases() {
        panic!("stub");
    }

    #[test]
    fn plan_done_refuses_unresolved_drafts() {
        panic!("stub");
    }

    #[test]
    fn plan_check_reports_unregistered_phases() {
        panic!("stub");
    }

    #[test]
    fn plan_reopen_restores_dispatch_and_clears_terminal_state() {
        panic!("stub");
    }
}
