//! What the plan view does when the User acts (commands.md §13.2, §17): it changes state only through cpd commands,
//! and lays out its own panes. Nothing here runs unless a key or a click asks for it: the view's reads, its Event
//! follower and its drawing run nothing. `session open` runs in this process through the command tree, as `cpd tui`
//! does.

use cpd_core::config::Config;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;
use cpd_core::ids::SessionId;

use crate::viewlog::ViewLog;
use crate::{RunCommand, dialog, widgets};

pub struct Actions {
    tmux: Tmux,
    cockpit: String,
    run_command: RunCommand,
    log: ViewLog,
}

impl Actions {
    pub fn new(home: CpdHome, config: &Config, run_command: RunCommand) -> Actions {
        let log = ViewLog::of(&home, "plan");
        Actions {
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
    /// (shared with the Dispatch column's rows).
    pub fn open_lead(&self, session: &SessionId) -> Result<(), CpdError> {
        widgets::open_lead(self.run_command, &self.tmux, &self.cockpit, session)
    }
}
