//! The tree's actions (commands.md §13.2, §17): the views change state only by
//! running `cpd2` commands as subprocesses. `cpd2` is resolved on `PATH`, as in
//! every pane cpd starts ([`PROGRAM`]).
//!
//! Text the User typed is passed as `--flag=value`, so a value that starts with
//! `-` is still a value; a name given to `session rename` follows `--`.

use std::process::{Command, Stdio};

use serde_json::Value;

use crate::args::Name;
use crate::ids::{ContextId, PlanId, SessionId};
use crate::session::PROGRAM;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// `session open <id>`: bring a Lead into the chat slot (§10.4).
    Open(SessionId),
    /// `session park <id>`: move a Lead out of the chat slot (§10.5).
    Park(SessionId),
    /// `session fork <source> [--name=<name>]`: a side fork of a Plan's Lead (§10.3).
    Fork { source: SessionId, name: Option<Name> },
    /// `session rename --session <id> -- <name>` (§10.7).
    Rename { session: SessionId, name: Name },
    /// `plan new <id> --context=<c> --repo=<path> --title=<text>` (§4.1): the Lead forks the Context's grounded Lead.
    NewPlan { context: ContextId, id: PlanId, title: String, repo: String },
}

impl Action {
    /// The arguments of `cpd2`.
    pub fn argv(&self) -> Vec<String> {
        let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        match self {
            Action::Open(id) => words(&["session", "open", id.as_str()]),
            Action::Park(id) => words(&["session", "park", id.as_str()]),
            Action::Fork { source, name } => {
                let mut argv = words(&["session", "fork", source.as_str()]);
                argv.extend(name.iter().map(|n| format!("--name={n}")));
                argv
            }
            Action::Rename { session, name } => words(&["session", "rename", "--session", session.as_str(), "--", name.as_str()]),
            Action::NewPlan { context, id, title, repo } => {
                let mut argv = words(&["plan", "new", id.as_str()]);
                argv.extend([format!("--context={context}"), format!("--repo={repo}"), format!("--title={title}")]);
                argv
            }
        }
    }

    /// The note after the command succeeded, from its reply.
    pub fn done(&self, reply: &Value) -> String {
        let already = reply["already"] == true;
        match self {
            Action::Open(_) if already => "already in the chat slot".into(),
            Action::Open(_) => "opened".into(),
            Action::Park(_) if already => "not in the chat slot".into(),
            Action::Park(_) => "parked".into(),
            Action::Fork { .. } => format!("forked as {}", reply["session"]["name"].as_str().unwrap_or("?")),
            Action::Rename { name, .. } => format!("renamed to {name}"),
            Action::NewPlan { id, .. } => format!("Plan {id} started"),
        }
    }
}

/// Runs the action: its note, or the message of the error it printed (or why it could not run).
pub fn run(action: &Action) -> Result<String, String> {
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
        Err(format!("{}: {}", error["code"].as_str().unwrap_or("error"), error["message"].as_str().unwrap_or("failed")))
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
        let name = |n: &str| n.parse::<Name>().unwrap();
        let cases = [
            (Action::Open(sid("l-ship")), vec!["session", "open", "l-ship"]),
            (Action::Park(sid("l-ship")), vec!["session", "park", "l-ship"]),
            (Action::Fork { source: sid("l-ship"), name: None }, vec!["session", "fork", "l-ship"]),
            (Action::Fork { source: sid("l-ship"), name: Some(name("my fork")) }, vec!["session", "fork", "l-ship", "--name=my fork"]),
            (
                Action::Rename { session: sid("l-ship"), name: name("-odd") },
                vec!["session", "rename", "--session", "l-ship", "--", "-odd"],
            ),
            (
                Action::NewPlan {
                    context: ContextId::parse("alpha").unwrap(),
                    id: PlanId::parse("pay-4521").unwrap(),
                    title: "Retry failed payouts".into(),
                    repo: "/work/pay".into(),
                },
                vec!["plan", "new", "pay-4521", "--context=alpha", "--repo=/work/pay", "--title=Retry failed payouts"],
            ),
        ];
        for (action, argv) in cases {
            assert_eq!(action.argv(), argv, "{action:?}");
        }
    }

    #[test]
    fn the_note_says_what_the_reply_says() {
        let open = Action::Open(sid("l-ship"));
        assert_eq!(open.done(&json!({"ok": true, "session": "l-ship", "pane": "%1", "displaced": null})), "opened");
        assert_eq!(open.done(&json!({"ok": true, "already": true})), "already in the chat slot");
        assert_eq!(Action::Park(sid("l-ship")).done(&json!({"ok": true, "already": true})), "not in the chat slot");
        let fork = Action::Fork { source: sid("l-ship"), name: None };
        assert_eq!(fork.done(&json!({"ok": true, "session": {"name": "ship (fork)"}})), "forked as ship (fork)");
    }
}
