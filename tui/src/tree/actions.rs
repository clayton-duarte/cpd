//! The tree's actions (commands.md §13.2, §17): the view changes state only by
//! running `cpd` commands as subprocesses. `cpd` is resolved on `PATH`, as in
//! every pane cpd starts ([`PROGRAM`]). Besides those, it lays out the cockpit
//! as the reference does: a Session opened in the chat slot gets the focus
//! (`select-pane`), and `?` opens the legend popup.
//! The menu's Open, Close, Archive, Unarchive, Rename…, Fork…, New session…, New Plan… and New Context… are
//! the same commands.
//!
//! Text the User typed is passed as `--flag=value`, or after `--` when it is a positional, so a value that starts
//! with `-` is still a value.

use std::process::{Command, Stdio};

use serde_json::Value;

use cpd_core::config::Config;
use cpd_core::host::layout::MAIN_WINDOW;
use cpd_core::host::{Tmux, chat_slot};
use cpd_core::ids::{ContextId, PlanId, SessionId};
use cpd_core::session::PROGRAM;
use crate::widgets::open_legend;

/// The object an archive or unarchive is about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Subject {
    Context(ContextId),
    Plan(PlanId),
    Session(SessionId),
}

impl Subject {
    /// The words of the command: `context archive <id>`, `plan archive --plan=<id>`, `session archive <id>`.
    fn command(&self, verb: &str) -> Vec<String> {
        let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        match self {
            Subject::Context(id) => words(&["context", verb, id.as_str()]),
            Subject::Plan(id) => vec!["plan".to_owned(), verb.to_owned(), format!("--plan={id}")],
            Subject::Session(id) => words(&["session", verb, id.as_str()]),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// `session open <id>` (§10.4), then the focus on the chat slot: Enter, or a click on a Session.
    Open(SessionId),
    /// `plan new <id> --context=<c> --repo=<path> --title=<title>` (§4.1): `n` (the title is the id) and New Plan….
    /// The Lead forks the Context's grounded Lead (R15).
    NewPlan { context: ContextId, id: PlanId, repo: String, title: String },
    /// `context new <id> --title=<title>` (§3.1): New Context….
    NewContext { id: ContextId, title: String },
    /// `context rename <id> --title=<title>` (§3.7).
    RenameContext { id: ContextId, title: String },
    /// `plan rename --plan=<id> --title=<title>` (§4.9).
    RenamePlan { id: PlanId, title: String },
    /// `session rename --session=<id> -- <name>` (§10.7).
    RenameSession { id: SessionId, name: String },
    /// `session fork <source> --name=<name>` (§10.3): Fork… and New session….
    Fork { source: SessionId, name: String },
    /// `session close <id>` (§10.9): `x`.
    Close(SessionId),
    /// `context|plan|session archive` (§3.8, §4.10, §10.10): the menu's Archive, after its y/N.
    Archive(Subject),
    /// `context|plan|session unarchive` (§3.9, §4.11, §10.11): the menu's Unarchive.
    Unarchive(Subject),
}

/// The refusal of `x` on the Session in the chat slot (the reference's), also when `session close` refuses it
/// because it moved there while the prompt was open.
pub const IN_CHAT_SLOT: &str = "switch to another session first";

impl Action {
    /// The arguments of `cpd`.
    pub fn argv(&self) -> Vec<String> {
        let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        match self {
            Action::Open(id) => words(&["session", "open", id.as_str()]),
            Action::NewPlan { context, id, repo, title } => {
                let mut argv = words(&["plan", "new", id.as_str()]);
                argv.extend([format!("--context={context}"), format!("--repo={repo}"), format!("--title={title}")]);
                argv
            }
            Action::NewContext { id, title } => vec!["context".into(), "new".into(), id.to_string(), format!("--title={title}")],
            Action::RenameContext { id, title } => vec!["context".into(), "rename".into(), id.to_string(), format!("--title={title}")],
            Action::RenamePlan { id, title } => vec!["plan".into(), "rename".into(), format!("--plan={id}"), format!("--title={title}")],
            Action::RenameSession { id, name } => {
                vec!["session".into(), "rename".into(), format!("--session={id}"), "--".into(), name.clone()]
            }
            Action::Fork { source, name } => vec!["session".into(), "fork".into(), source.to_string(), format!("--name={name}")],
            Action::Close(id) => words(&["session", "close", id.as_str()]),
            Action::Archive(subject) => subject.command("archive"),
            Action::Unarchive(subject) => subject.command("unarchive"),
        }
    }

    /// The note after the command succeeded, from its reply: the reference's words.
    pub fn done(&self, reply: &Value) -> String {
        match self {
            Action::Open(_) => String::new(),
            Action::NewPlan { id, .. } => format!("forking {id} in window park-{id}"),
            Action::NewContext { id, .. } => format!("created {id}"),
            Action::RenameContext { .. } | Action::RenamePlan { .. } | Action::RenameSession { .. } if reply["already"] == true => {
                "already named so".into()
            }
            Action::RenameContext { .. } | Action::RenamePlan { .. } | Action::RenameSession { .. } => "renamed".into(),
            Action::Fork { name, .. } => format!("forking {name}"),
            Action::Close(_) if reply["already"] == true => "session is already gone".into(),
            Action::Close(_) => "closed".into(),
            Action::Archive(_) if reply["already"] == true => "already archived".into(),
            Action::Archive(_) => "archived".into(),
            Action::Unarchive(_) if reply["already"] == true => "already unarchived".into(),
            Action::Unarchive(_) => "unarchived".into(),
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
    fn focus_chat(&self) -> Result<(), cpd_core::error::CpdError> {
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
            (Action::Archive(Subject::Context(ContextId::parse("alpha").unwrap())), vec!["context", "archive", "alpha"]),
            (Action::Archive(Subject::Plan(PlanId::parse("ship").unwrap())), vec!["plan", "archive", "--plan=ship"]),
            (Action::Archive(Subject::Session(sid("f-idle"))), vec!["session", "archive", "f-idle"]),
            (Action::Unarchive(Subject::Context(ContextId::parse("alpha").unwrap())), vec!["context", "unarchive", "alpha"]),
            (Action::Unarchive(Subject::Plan(PlanId::parse("ship").unwrap())), vec!["plan", "unarchive", "--plan=ship"]),
            (Action::Unarchive(Subject::Session(sid("f-idle"))), vec!["session", "unarchive", "f-idle"]),
            (
                Action::NewPlan {
                    context: ContextId::parse("alpha").unwrap(),
                    id: PlanId::parse("pay-4521").unwrap(),
                    repo: "/work/pay".into(),
                    title: "Pay 4521".into(),
                },
                vec!["plan", "new", "pay-4521", "--context=alpha", "--repo=/work/pay", "--title=Pay 4521"],
            ),
            (
                Action::NewContext { id: ContextId::parse("building-cpd").unwrap(), title: "Building cpd!".into() },
                vec!["context", "new", "building-cpd", "--title=Building cpd!"],
            ),
            (
                Action::RenameContext { id: ContextId::parse("alpha").unwrap(), title: "-Dash".into() },
                vec!["context", "rename", "alpha", "--title=-Dash"],
            ),
            (Action::RenamePlan { id: PlanId::parse("ship").unwrap(), title: "Ship it".into() }, vec!["plan", "rename", "--plan=ship", "--title=Ship it"]),
            (
                Action::RenameSession { id: sid("f-idle"), name: "-x y".into() },
                vec!["session", "rename", "--session=f-idle", "--", "-x y"],
            ),
            (Action::Fork { source: sid("l-ship"), name: "ship (fork)".into() }, vec!["session", "fork", "l-ship", "--name=ship (fork)"]),
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
        let new = Action::NewPlan { context: ContextId::parse("alpha").unwrap(), id: PlanId::parse("pay").unwrap(), repo: "/r".into(), title: "Pay".into() };
        assert_eq!(new.done(&json!({"ok": true})), "forking pay in window park-pay");
        let context = Action::NewContext { id: ContextId::parse("alpha").unwrap(), title: "Alpha".into() };
        assert_eq!(context.done(&json!({"ok": true})), "created alpha");
        let rename = Action::RenameSession { id: sid("f-idle"), name: "x".into() };
        assert_eq!(rename.done(&json!({"ok": true, "name": "x"})), "renamed");
        assert_eq!(rename.done(&json!({"ok": true, "already": true})), "already named so");
        let fork = Action::Fork { source: sid("l-ship"), name: "ship (fork)".into() };
        assert_eq!(fork.done(&json!({"ok": true})), "forking ship (fork)");
        let archive = Action::Archive(Subject::Session(sid("f-idle")));
        assert_eq!(archive.done(&json!({"ok": true, "archived": [], "closed": []})), "archived");
        assert_eq!(archive.done(&json!({"ok": true, "already": true})), "already archived");
        let unarchive = Action::Unarchive(Subject::Session(sid("f-idle")));
        assert_eq!(unarchive.done(&json!({"ok": true, "archived": [], "closed": []})), "unarchived");
        assert_eq!(unarchive.done(&json!({"ok": true, "already": true})), "already unarchived");
    }

    #[test]
    fn closing_the_session_in_the_chat_slot_is_refused_in_the_references_words() {
        let close = Action::Close(sid("l-ship"));
        assert_eq!(close.failed("in_chat_slot", "It is in the chat slot."), IN_CHAT_SLOT);
        assert_eq!(close.failed("not_a_lead", "Not a Lead."), "not_a_lead: Not a Lead.");
        assert_eq!(Action::Open(sid("l-ship")).failed("in_chat_slot", "x"), "in_chat_slot: x");
        let archive = Action::Archive(Subject::Session(sid("l-ship")));
        assert_eq!(archive.failed("archive_blocked", "A Session is working."), "archive_blocked: A Session is working.");
    }
}
