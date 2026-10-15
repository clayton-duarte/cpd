//! What the plan view does when the User acts (commands.md §13.2, §17): it changes state only through cpd2 commands,
//! and lays out its own panes. Nothing here runs unless a key or a click asks for it: the view's reads, its Event
//! follower and its drawing run nothing. `session open` runs in this process through the command tree, as `cpd2 tui`
//! does; the Worker pane is the view's own layout (`split-window`, `@cpd_view worker`), not Ledger state.

use std::collections::BTreeMap;
use std::ffi::OsString;

use crate::config::Config;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::host::layout::MAIN_WINDOW;
use crate::host::{Pane, PaneId, Spawn, Split, Tmux, View, chat_slot, view_pane};
use crate::ids::{AttemptId, PlanId, SessionId};
use crate::session::PROGRAM;

/// The pane option holding the Attempt a Worker pane shows, so a second click on the same Job closes it.
const OPT_ATTEMPT: &str = "@cpd_attempt";

pub struct Actions {
    home: CpdHome,
    tmux: Tmux,
    cockpit: String,
}

impl Actions {
    pub fn new(home: CpdHome, config: &Config) -> Actions {
        Actions { home, tmux: Tmux::new(config.tmux.socket.clone()), cockpit: config.tmux.cockpit_session.clone() }
    }

    /// A click on a tab: `session open <id>`, which moves that Lead into the chat slot.
    pub fn open_lead(&self, session: &SessionId) -> Result<(), CpdError> {
        let argv = [PROGRAM, "session", "open", session.as_str()].map(OsString::from);
        crate::cli::run(argv).map(drop)
    }

    /// The panes of the cockpit's `main` window.
    fn main_panes(&self) -> Result<Vec<Pane>, CpdError> {
        let all = self.tmux.list_panes()?;
        Ok(all.into_iter().filter(|p| p.session_name == self.cockpit && p.window_name == MAIN_WINDOW).collect())
    }

    /// The Attempt a Worker pane shows.
    fn shows(&self, pane: &PaneId) -> Result<String, CpdError> {
        let shown = self.tmux.run(&["display", "-p", "-t", pane.as_str(), &format!("#{{{OPT_ATTEMPT}}}")].map(str::to_owned))?;
        Ok(shown.trim_end_matches('\n').to_owned())
    }

    /// Opens the read-only Worker pane beside the chat for `attempt`, retargets it when it shows another Attempt, and
    /// closes it when it already shows this one. Returns whether it is open now.
    pub fn toggle_worker(&self, plan: &PlanId, attempt: &AttemptId) -> Result<bool, CpdError> {
        let panes = self.main_panes()?;
        let shown = view_pane(&panes, &self.cockpit, View::Worker);
        if let Some(pane) = shown {
            let same = self.shows(&pane.pane)? == attempt.to_string();
            self.tmux.kill_pane(&pane.pane)?;
            if same {
                return Ok(false);
            }
        }
        let panes = self.main_panes()?;
        let Some(chat) = chat_slot(&panes, &self.cockpit) else {
            return Err(CpdError::new(crate::error::ErrorCode::TmuxFailed, "There is no chat slot to open the Worker beside."));
        };
        let attempt = attempt.to_string();
        let spawn = Spawn {
            cwd: Some(self.home.root().to_owned()),
            env: BTreeMap::from([("CPD_HOME".to_owned(), self.home.root().display().to_string())]),
            argv: [PROGRAM, "tui", "view", "worker", &attempt, "--plan", plan.as_str()].map(str::to_owned).into(),
        };
        let pane = self.tmux.split_window(&chat.pane, Split::Right, Some("50%"), &spawn)?;
        self.tmux.set_pane_view(&pane, View::Worker)?;
        self.tmux.run(&["set", "-p", "-t", pane.as_str(), OPT_ATTEMPT, &attempt].map(str::to_owned))?;
        Ok(true)
    }

    /// Closes the Worker pane when it shows `attempt`; any other Worker pane stays.
    pub fn close_worker(&self, attempt: &AttemptId) -> Result<(), CpdError> {
        let panes = self.main_panes()?;
        if let Some(pane) = view_pane(&panes, &self.cockpit, View::Worker) {
            if self.shows(&pane.pane)? == attempt.to_string() {
                self.tmux.kill_pane(&pane.pane)?;
            }
        }
        Ok(())
    }
}
