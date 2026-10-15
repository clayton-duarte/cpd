//! The tree's actions (commands.md §13.2, §17): the view changes state only by
//! running `cpd2` commands as subprocesses. `cpd2` is resolved on `PATH`, as in
//! every pane cpd starts ([`PROGRAM`]). Besides those, it lays out the cockpit
//! as the reference does: a Session opened in the chat slot gets the focus
//! (`select-pane`), and `?` opens the legend popup.
//!
//! Text the User typed is passed as `--flag=value`, so a value that starts with
//! `-` is still a value.

use std::process::{Command, Stdio};

use serde_json::Value;

use crate::config::Config;
use crate::host::layout::MAIN_WINDOW;
use crate::host::{Tmux, chat_slot};
use crate::ids::{ContextId, PlanId, SessionId};
use crate::session::PROGRAM;
use crate::tui::widgets::open_legend;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// `session open <id>` (§10.4), then the focus on the chat slot: Enter, or a click on a Session.
    Open(SessionId),
    /// `plan new <id> --context=<c> --repo=<path> --title=<id>` (§4.1): `n`. The Lead forks the Context's grounded
    /// Lead (R15).
    NewPlan { context: ContextId, id: PlanId, repo: String },
    /// `session close <id>` (§10.9): `x`.
    Close(SessionId),
}

/// The refusal of `x` on the Session in the chat slot (the reference's), also when `session close` refuses it
/// because it moved there while the prompt was open.
pub const IN_CHAT_SLOT: &str = "switch to another session first";

impl Action {
    /// The arguments of `cpd2`.
    pub fn argv(&self) -> Vec<String> {
        let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        match self {
            Action::Open(id) => words(&["session", "open", id.as_str()]),
            Action::NewPlan { context, id, repo } => {
                let mut argv = words(&["plan", "new", id.as_str()]);
                argv.extend([format!("--context={context}"), format!("--repo={repo}"), format!("--title={id}")]);
                argv
            }
            Action::Close(id) => words(&["session", "close", id.as_str()]),
        }
    }

    /// The note after the command succeeded, from its reply: the reference's words.
    pub fn done(&self, reply: &Value) -> String {
        match self {
            Action::Open(_) => String::new(),
            Action::NewPlan { id, .. } => format!("forking {id} in window park-{id}"),
            Action::Close(_) if reply["already"] == true => "session is already gone".into(),
            Action::Close(_) => "closed".into(),
        }
    }

    /// The note after the command failed with `code` and `message`.
    pub fn failed(&self, code: &str, message: &str) -> String {
        match (self, code) {
            (Action::Close(_), "in_chat_slot") => IN_CHAT_SLOT.into(),
            _ => format!("{code}: {message}"),
        }
    }
}

/// Runs actions against the cockpit of `config`.
pub struct Runner {
    tmux: Tmux,
    cockpit: String,
}

impl Runner {
    pub fn new(config: &Config) -> Runner {
        Runner { tmux: Tmux::new(config.tmux.socket.clone()), cockpit: config.tmux.cockpit_session.clone() }
    }

    /// Runs the action: its note, or why it failed.
    pub fn run(&self, action: &Action) -> Result<String, String> {
        let note = command(action)?;
        if matches!(action, Action::Open(_)) {
            self.focus_chat().map_err(|e| e.message().to_owned())?;
        }
        Ok(note)
    }

    /// `select-pane` on the chat slot, so typing goes to the chat (the reference's `switch_to`).
    fn focus_chat(&self) -> Result<(), crate::error::CpdError> {
        let panes: Vec<_> =
            self.tmux.list_panes()?.into_iter().filter(|p| p.session_name == self.cockpit && p.window_name == MAIN_WINDOW).collect();
        if let Some(chat) = chat_slot(&panes, &self.cockpit) {
            self.tmux.run(&["select-pane", "-t", chat.pane.as_str()].map(str::to_owned))?;
        }
        Ok(())
    }

    /// `?`: the legend popup; returns when it closes.
    pub fn legend(&self) -> Result<(), String> {
        open_legend(&self.tmux).map_err(|e| e.message().to_owned())
    }
}

/// Runs the command of `action`: its note, or the message of the error it printed (or why it could not run).
fn command(action: &Action) -> Result<String, String> {
    let output = Command::new(PROGRAM)
        .args(action.argv())
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot run {PROGRAM}: {e}"))?;
    let reply: Value = serde_json::from_slice(&output.stdout).map_err(|_| {
        let stderr = String::from_utf8_lossy(&output.stderr);
        format!("{PROGRAM} {} printed no JSON (exit {}): {}", action.argv().join(" "), output.status, stderr.trim())
    })?;
    if reply["ok"] == true {
        Ok(action.done(&reply))
    } else {
        let error = &reply["error"];
        Err(action.failed(error["code"].as_str().unwrap_or("error"), error["message"].as_str().unwrap_or("failed")))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn sid(s: &str) -> SessionId {
        SessionId::parse(s).unwrap()
    }

    #[test]
    fn each_action_is_one_command_of_the_contract() {
        let cases = [
            (Action::Open(sid("l-ship")), vec!["session", "open", "l-ship"]),
            (Action::Close(sid("l-ship")), vec!["session", "close", "l-ship"]),
            (
                Action::NewPlan { context: ContextId::parse("alpha").unwrap(), id: PlanId::parse("pay-4521").unwrap(), repo: "/work/pay".into() },
                vec!["plan", "new", "pay-4521", "--context=alpha", "--repo=/work/pay", "--title=pay-4521"],
            ),
        ];
        for (action, argv) in cases {
            assert_eq!(action.argv(), argv, "{action:?}");
        }
    }

    #[test]
    fn the_note_says_what_the_reference_says() {
        let open = Action::Open(sid("l-ship"));
        assert_eq!(open.done(&json!({"ok": true, "session": "l-ship", "pane": "%1", "displaced": null})), "");
        let close = Action::Close(sid("l-ship"));
        assert_eq!(close.done(&json!({"ok": true, "session": "l-ship", "pane": "%4"})), "closed");
        assert_eq!(close.done(&json!({"ok": true, "already": true})), "session is already gone");
        let new = Action::NewPlan { context: ContextId::parse("alpha").unwrap(), id: PlanId::parse("pay").unwrap(), repo: "/r".into() };
        assert_eq!(new.done(&json!({"ok": true})), "forking pay in window park-pay");
    }

    #[test]
    fn closing_the_session_in_the_chat_slot_is_refused_in_the_references_words() {
        let close = Action::Close(sid("l-ship"));
        assert_eq!(close.failed("in_chat_slot", "It is in the chat slot."), IN_CHAT_SLOT);
        assert_eq!(close.failed("not_a_lead", "Not a Lead."), "not_a_lead: Not a Lead.");
        assert_eq!(Action::Open(sid("l-ship")).failed("in_chat_slot", "x"), "in_chat_slot: x");
    }
}
