//! What the plan view does when the User acts (commands.md §13.2, §17): it changes state only through cpd commands,
//! and lays out its own panes. Nothing here runs unless a key or a click asks for it: the view's reads, its Event
//! follower and its drawing run nothing. `session open` runs in this process through the command tree, as `cpd tui`
//! does; the Worker pane is the view's own layout (`split-window`, `@cpd_view worker`, `@cpd_job`), not Ledger state.

use cpd_core::config::Config;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;
use cpd_core::ids::{AttemptId, PlanId, SessionId};

use crate::viewlog::ViewLog;
use crate::{RunCommand, dialog, widgets};

pub struct Actions {
    home: CpdHome,
    tmux: Tmux,
    cockpit: String,
    run_command: RunCommand,
    log: ViewLog,
}

impl Actions {
    pub fn new(home: CpdHome, config: &Config, run_command: RunCommand) -> Actions {
        let log = ViewLog::of(&home, "plan");
        Actions {
            home,
            tmux: Tmux::new(config.tmux.socket.clone()),
            cockpit: config.tmux.cockpit_session.clone(),
            run_command,
            log,
        }
    }

    /// A command failed: a line of `run/tui/plan.log` and the error dialog (commands.md §17 "No notes"); `what` is what
    /// failed. Returns when the dialog closes.
    pub fn report(&self, what: &str, err: &CpdError) {
        dialog::report(&self.tmux, &self.log, what, err);
    }

    /// A click on a tab: `session open <id>`, which moves that Lead into the chat slot, then `select-pane` on the chat
    /// (shared with the Deploy column's rows).
    pub fn open_lead(&self, session: &SessionId) -> Result<(), CpdError> {
        widgets::open_lead(self.run_command, &self.tmux, &self.cockpit, session)
    }

    /// Opens the read-only Worker pane for `job`'s `attempt`, or closes it when it already shows `job`
    /// ([`widgets::toggle_worker`]). Returns whether it is open now.
    pub fn toggle_worker(&self, plan: &PlanId, job: &str, attempt: Option<&AttemptId>) -> Result<bool, CpdError> {
        widgets::toggle_worker(&self.home, &self.tmux, &self.cockpit, plan, job, attempt)
    }

    /// Closes the Worker pane when it shows `job`; any other Worker pane stays.
    pub fn close_worker(&self, job: &str) -> Result<(), CpdError> {
        widgets::close_worker(&self.tmux, &self.cockpit, job)
    }
}
