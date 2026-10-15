//! What the plan view does when the User acts (commands.md §13.2, §17): it changes state only through cpd2 commands,
//! and lays out its own panes. Nothing here runs unless a key or a click asks for it: the view's reads, its Event
//! follower and its drawing run nothing. `session open` runs in this process through the command tree, as `cpd2 tui`
//! does; the Worker pane is the view's own layout (`split-window`, `@cpd_view worker`, `@cpd_job`), not Ledger state.

use std::collections::BTreeMap;
use std::ffi::OsString;

use crate::config::Config;
use crate::error::CpdError;
use crate::home::CpdHome;
use crate::host::layout::MAIN_WINDOW;
use crate::host::{Pane, PaneId, Spawn, Split, Tmux, View, chat_slot, view_pane};
use crate::ids::{AttemptId, PlanId, SessionId};
use crate::session::PROGRAM;

/// The pane option holding the Job a Worker pane shows (its border title shows it), so a second click on the same Job
/// closes it.
const OPT_JOB: &str = "@cpd_job";

/// The pane option holding the Attempt a Worker pane shows.
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

    /// A click on a tab: `session open <id>`, which moves that Lead into the chat slot, then `select-pane` on the chat.
    pub fn open_lead(&self, session: &SessionId) -> Result<(), CpdError> {
        let argv = [PROGRAM, "session", "open", session.as_str()].map(OsString::from);
        crate::cli::run(argv)?;
        let panes = self.main_panes()?;
        if let Some(chat) = chat_slot(&panes, &self.cockpit) {
            self.tmux.run(&["select-pane", "-t", chat.pane.as_str()].map(str::to_owned))?;
        }
        Ok(())
    }

    /// The panes of the cockpit's `main` window.
    fn main_panes(&self) -> Result<Vec<Pane>, CpdError> {
        let all = self.tmux.list_panes()?;
        Ok(all.into_iter().filter(|p| p.session_name == self.cockpit && p.window_name == MAIN_WINDOW).collect())
    }

    /// The Job a Worker pane shows.
    fn shows(&self, pane: &PaneId) -> Result<String, CpdError> {
        let shown = self.tmux.run(&["display", "-p", "-t", pane.as_str(), &format!("#{{{OPT_JOB}}}")].map(str::to_owned))?;
        Ok(shown.trim_end_matches('\n').to_owned())
    }

    /// `set -p` of the Worker pane's options: the Job and its current Attempt.
    fn mark(&self, pane: &PaneId, job: &str, attempt: &str) -> Result<(), CpdError> {
        for (option, value) in [(OPT_JOB, job), (OPT_ATTEMPT, attempt)] {
            self.tmux.run(&["set", "-p", "-t", pane.as_str(), option, value].map(str::to_owned))?;
        }
        Ok(())
    }

    /// The Worker pane's program: `tui view worker <attempt> --plan <plan>`, in cpd's home.
    fn spawn(&self, plan: &PlanId, attempt: &str) -> Spawn {
        Spawn {
            cwd: Some(self.home.root().to_owned()),
            env: BTreeMap::from([("CPD_HOME".to_owned(), self.home.root().display().to_string())]),
            argv: [PROGRAM, "tui", "view", "worker", attempt, "--plan", plan.as_str()].map(str::to_owned).into(),
        }
    }

    /// Opens the read-only Worker pane beside the chat (`split-window -h -d -l 50%`, so the focus stays) for `job`'s
    /// `attempt`, retargets it (`respawn-pane -k`) when it shows another Job, and closes it when it already shows this
    /// Job. Returns whether it is open now.
    pub fn toggle_worker(&self, plan: &PlanId, job: &str, attempt: &AttemptId) -> Result<bool, CpdError> {
        let panes = self.main_panes()?;
        let attempt = attempt.to_string();
        if let Some(pane) = view_pane(&panes, &self.cockpit, View::Worker) {
            if self.shows(&pane.pane)? == job {
                self.tmux.kill_pane(&pane.pane)?;
                return Ok(false);
            }
            let spawn = self.spawn(plan, &attempt);
            let mut args = ["respawn-pane", "-k", "-t", pane.pane.as_str()].map(str::to_owned).to_vec();
            if let Some(cwd) = &spawn.cwd {
                args.extend(["-c".to_owned(), cwd.display().to_string()]);
            }
            for (key, value) in &spawn.env {
                args.extend(["-e".to_owned(), format!("{key}={value}")]);
            }
            args.push("--".to_owned());
            args.extend(spawn.argv);
            self.tmux.run(&args)?;
            self.mark(&pane.pane, job, &attempt)?;
            return Ok(true);
        }
        let Some(chat) = chat_slot(&panes, &self.cockpit) else {
            return Err(CpdError::new(crate::error::ErrorCode::TmuxFailed, "There is no chat slot to open the Worker beside."));
        };
        let pane = self.tmux.split_window(&chat.pane, Split::Right, Some("50%"), &self.spawn(plan, &attempt))?;
        self.tmux.set_pane_view(&pane, View::Worker)?;
        self.mark(&pane, job, &attempt)?;
        Ok(true)
    }

    /// Closes the Worker pane when it shows `job`; any other Worker pane stays.
    pub fn close_worker(&self, job: &str) -> Result<(), CpdError> {
        let panes = self.main_panes()?;
        if let Some(pane) = view_pane(&panes, &self.cockpit, View::Worker) {
            if self.shows(&pane.pane)? == job {
                self.tmux.kill_pane(&pane.pane)?;
            }
        }
        Ok(())
    }
}
