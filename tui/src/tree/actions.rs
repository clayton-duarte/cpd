//! The tree's actions (commands.md §13.2, §17): the view changes state only by
//! running `cpd` commands as subprocesses. `cpd` is resolved on `PATH`, as in
//! every pane cpd starts ([`PROGRAM`]). Besides those, it lays out the cockpit
//! as the reference does: a Session opened in the chat slot gets the focus
//! (`select-pane`), and `?` opens the legend popup.
//! A click on a Context row runs `context manager` (§3.10), then the Open of the Manager it returns.
//! The menu's Open, Close, Archive, Unarchive, Rename…, Fork…, New session…, New Plan… and New Context… are
//! the same commands; Copy id is the one that runs `tmux set-buffer -w` instead (OSC 52). After New Plan…, Fork… and
//! New session… succeed, the new Lead is opened in the chat slot (`session open`, then the focus, as Open does). Two flows run more than one: an Archive of the chat slot's Lead frees the slot first
//! ([`Free`], F3), and a click on an archived Session runs `session history` and focuses the chat (F5).
//!
//! The Inbox's menu and keys (`s`, `d`) run `inbox snooze <id> <15m|1h|tomorrow>` and `inbox dismiss <id>`.
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

/// How long an Inbox item is snoozed: the words of `inbox snooze` (§21.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Until {
    /// `15m`.
    Minutes15,
    /// `1h`.
    Hour1,
    /// `tomorrow`: the next 09:00.
    Tomorrow,
}

impl Until {
    fn arg(self) -> &'static str {
        match self {
            Until::Minutes15 => "15m",
            Until::Hour1 => "1h",
            Until::Tomorrow => "tomorrow",
        }
    }
}

/// How the tree frees the chat slot before it archives the Lead that holds it (F3, §13.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Free {
    /// `session open <id>` the next live Session (else the previous one), then the focus on the chat slot.
    Open(SessionId),
    /// `session park`: no other live Session, so the placeholder holds the slot.
    Park,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// `session open <id>` (§10.4), then the focus on the chat slot: Enter, or a click on a Session.
    Open(SessionId),
    /// `context manager --context=<c>`, then [`Action::Open`] of the Manager it returns: a click on a Context row (B3-A6).
    OpenManager(ContextId),
    /// `session park` (§10.5): the Lead in the chat slot goes back to a parked window. Only a step of an archive.
    Park,
    /// `session history <id>` (§10.12), then the focus on the chat slot: a click on an archived Session.
    History(SessionId),
    /// `plan new <id> --context=<c> --title=<title>` (§4.1): `n` (the title is the id) and New Plan….
    /// The Lead forks the Context's grounded Lead (R15).
    NewPlan { context: ContextId, id: PlanId, title: String },
    /// `context new <id> --title=<title> --owner=<login>` (§3.1): New Context….
    NewContext { id: ContextId, title: String, owner: String },
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
    /// `context|plan|session archive` (§3.8, §4.10, §10.10): the menu's Archive, at once, after `free` when the
    /// archived Lead holds the chat slot.
    Archive { subject: Subject, free: Option<Free> },
    /// `context|plan|session unarchive` (§3.9, §4.11, §10.11): the menu's Unarchive.
    Unarchive(Subject),
    /// `tmux set-buffer -w -- <id>` (hs-d102): the menu's Copy id; the id is the one the row shows. The only action
    /// that is no `cpd` command.
    CopyId(String),
    /// `inbox snooze <id> <15m|1h|tomorrow>` (§21.3): the Inbox menu's and `s`'s Snooze entries.
    Snooze { item: i64, until: Until },
    /// `inbox dismiss <id>` (§21.4): the Inbox menu's Dismiss and `d`.
    Dismiss(i64),
}

/// The refusal of `x` on the Session in the chat slot (the reference's), also when `session close` refuses it
/// because it moved there while the prompt was open.
pub const IN_CHAT_SLOT: &str = "switch to another session first";

impl Action {
    /// The arguments of the command: `cpd`'s, or `tmux`'s for Copy id (the [`Tmux`] adds `-L <socket>`).
    pub fn argv(&self) -> Vec<String> {
        let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        match self {
            Action::Open(id) => words(&["session", "open", id.as_str()]),
            Action::OpenManager(context) => vec!["context".into(), "manager".into(), format!("--context={context}")],
            Action::Park => words(&["session", "park"]),
            Action::History(id) => words(&["session", "history", id.as_str()]),
            Action::NewPlan { context, id, title } => {
                let mut argv = words(&["plan", "new", id.as_str()]);
                argv.extend([format!("--context={context}"), format!("--title={title}")]);
                argv
            }
            Action::NewContext { id, title, owner } => {
                vec!["context".into(), "new".into(), id.to_string(), format!("--title={title}"), format!("--owner={owner}")]
            }
            Action::RenameContext { id, title } => vec!["context".into(), "rename".into(), id.to_string(), format!("--title={title}")],
            Action::RenamePlan { id, title } => vec!["plan".into(), "rename".into(), format!("--plan={id}"), format!("--title={title}")],
            Action::RenameSession { id, name } => {
                vec!["session".into(), "rename".into(), format!("--session={id}"), "--".into(), name.clone()]
            }
            Action::Fork { source, name } => vec!["session".into(), "fork".into(), source.to_string(), format!("--name={name}")],
            Action::Close(id) => words(&["session", "close", id.as_str()]),
            Action::Archive { subject, .. } => subject.command("archive"),
            Action::Unarchive(subject) => subject.command("unarchive"),
            Action::CopyId(id) => words(&["set-buffer", "-w", "--", id.as_str()]),
            Action::Snooze { item, until } => words(&["inbox", "snooze", &item.to_string(), until.arg()]),
            Action::Dismiss(item) => words(&["inbox", "dismiss", &item.to_string()]),
        }
    }

    /// The step that opens the Lead the action created, from its reply (`plan new`'s `lead.session`, `session fork`'s
    /// `session.id`), or the Manager `context manager` returned (`manager.session`); none for the other actions. A reply with no Session id is an error.
    pub fn open_new(&self, reply: &Value) -> Result<Option<Action>, String> {
        let (command, id) = match self {
            Action::NewPlan { .. } => ("plan new", &reply["lead"]["session"]),
            Action::Fork { .. } => ("session fork", &reply["session"]["id"]),
            Action::OpenManager(_) => ("context manager", &reply["manager"]["session"]),
            _ => return Ok(None),
        };
        match id.as_str().and_then(|id| SessionId::parse(id).ok()) {
            Some(id) => Ok(Some(Action::Open(id))),
            None => Err(format!("{command} printed no Session id to open")),
        }
    }

    /// The commands the action runs, in order, each stopping the flow when it is refused: the archive of the chat
    /// slot's Lead is preceded by the step that frees the slot (F3); every other action is its own one step.
    pub fn steps(&self) -> Vec<Action> {
        match self {
            Action::Archive { subject, free: Some(free) } => vec![
                match free {
                    Free::Open(id) => Action::Open(id.clone()),
                    Free::Park => Action::Park,
                },
                Action::Archive { subject: subject.clone(), free: None },
            ],
            action => vec![action.clone()],
        }
    }

    /// The note after the command succeeded, from its reply: the reference's words.
    pub fn done(&self, reply: &Value) -> String {
        match self {
            Action::Open(_) | Action::OpenManager(_) | Action::Park | Action::History(_) => String::new(),
            Action::NewPlan { id, .. } => format!("forking {id} in window park-{id}"),
            Action::NewContext { id, .. } => format!("created {id}"),
            Action::RenameContext { .. } | Action::RenamePlan { .. } | Action::RenameSession { .. } if reply["already"] == true => {
                "already named so".into()
            }
            Action::RenameContext { .. } | Action::RenamePlan { .. } | Action::RenameSession { .. } => "renamed".into(),
            Action::Fork { name, .. } => format!("forking {name}"),
            Action::Close(_) if reply["already"] == true => "session is already gone".into(),
            Action::Close(_) => "closed".into(),
            Action::Archive { .. } if reply["already"] == true => "already archived".into(),
            Action::Archive { .. } => "archived".into(),
            Action::Unarchive(_) if reply["already"] == true => "already unarchived".into(),
            Action::Unarchive(_) => "unarchived".into(),
            Action::CopyId(id) => format!("copied {id}"),
            Action::Snooze { .. } => "snoozed".into(),
            Action::Dismiss(_) => "dismissed".into(),
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

    /// Runs the action's steps: the note of the last, or why one failed (the flow stops there).
    pub fn run(&self, action: &Action) -> Result<String, String> {
        let mut note = String::new();
        for step in action.steps() {
            let reply = match step {
                Action::CopyId(_) => self.tmux.run(&step.argv()).map(|_| Value::Null).map_err(|e| e.message().to_owned())?,
                _ => command(&step)?,
            };
            note = step.done(&reply);
            if let Some(open) = step.open_new(&reply)? {
                // the Plan or fork stays created when this is refused
                command(&open)?;
                self.focus_chat().map_err(|e| e.message().to_owned())?;
            }
            if matches!(step, Action::Open(_) | Action::History(_)) {
                self.focus_chat().map_err(|e| e.message().to_owned())?;
            }
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

    /// A click on an `engine-error` or `engine-down` Inbox item: the logs popup at the Event (the bottom with none);
    /// returns when it closes.
    pub fn logs(&self, event: Option<i64>) -> Result<(), String> {
        crate::logs::open_at(&self.tmux, event).map_err(|e| e.message().to_owned())
    }

    /// `?`: the legend popup; returns when it closes.
    pub fn legend(&self) -> Result<(), String> {
        open_legend(&self.tmux).map_err(|e| e.message().to_owned())
    }
}

/// Runs the command of `action`: its reply, or the message of the error it printed (or why it could not run).
fn command(action: &Action) -> Result<Value, String> {
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
        Ok(reply)
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

    fn archive(subject: Subject) -> Action {
        Action::Archive { subject, free: None }
    }

    fn sequence(action: &Action) -> Vec<Vec<String>> {
        action.steps().iter().map(Action::argv).collect()
    }

    #[test]
    fn an_archive_of_the_chat_slots_lead_frees_the_slot_first_and_any_other_action_is_one_step() {
        let plan = Subject::Plan(PlanId::parse("ship").unwrap());
        let open = Action::Archive { subject: plan.clone(), free: Some(Free::Open(sid("l-sketch"))) };
        assert_eq!(sequence(&open), [vec!["session", "open", "l-sketch"], vec!["plan", "archive", "--plan=ship"]]);
        let park = Action::Archive { subject: Subject::Session(sid("f-idle")), free: Some(Free::Park) };
        assert_eq!(sequence(&park), [vec!["session", "park"], vec!["session", "archive", "f-idle"]]);
        assert_eq!(sequence(&archive(plan)), [vec!["plan", "archive", "--plan=ship"]]);
        assert_eq!(sequence(&Action::History(sid("f-idle"))), [vec!["session", "history", "f-idle"]]);
        assert_eq!(sequence(&Action::Open(sid("l-ship"))), [vec!["session", "open", "l-ship"]]);
    }

    #[test]
    fn each_action_is_one_command_of_the_contract() {
        let cases = [
            (Action::Open(sid("l-ship")), vec!["session", "open", "l-ship"]),
            (Action::Close(sid("l-ship")), vec!["session", "close", "l-ship"]),
            (Action::OpenManager(ContextId::parse("alpha").unwrap()), vec!["context", "manager", "--context=alpha"]),
            (Action::Park, vec!["session", "park"]),
            (Action::History(sid("f-idle")), vec!["session", "history", "f-idle"]),
            (archive(Subject::Context(ContextId::parse("alpha").unwrap())), vec!["context", "archive", "alpha"]),
            (archive(Subject::Plan(PlanId::parse("ship").unwrap())), vec!["plan", "archive", "--plan=ship"]),
            (archive(Subject::Session(sid("f-idle"))), vec!["session", "archive", "f-idle"]),
            (Action::Unarchive(Subject::Context(ContextId::parse("alpha").unwrap())), vec!["context", "unarchive", "alpha"]),
            (Action::Unarchive(Subject::Plan(PlanId::parse("ship").unwrap())), vec!["plan", "unarchive", "--plan=ship"]),
            (Action::Unarchive(Subject::Session(sid("f-idle"))), vec!["session", "unarchive", "f-idle"]),
            (
                Action::NewPlan {
                    context: ContextId::parse("alpha").unwrap(),
                    id: PlanId::parse("pay-4521").unwrap(),
                    title: "Pay 4521".into(),
                },
                vec!["plan", "new", "pay-4521", "--context=alpha", "--title=Pay 4521"],
            ),
            (
                Action::NewContext {
                    id: ContextId::parse("building-cpd").unwrap(),
                    title: "Building cpd!".into(),
                    owner: "clayton-duarte".into(),
                },
                vec!["context", "new", "building-cpd", "--title=Building cpd!", "--owner=clayton-duarte"],
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
            (Action::Snooze { item: 12, until: Until::Minutes15 }, vec!["inbox", "snooze", "12", "15m"]),
            (Action::Snooze { item: 12, until: Until::Hour1 }, vec!["inbox", "snooze", "12", "1h"]),
            (Action::Snooze { item: 7, until: Until::Tomorrow }, vec!["inbox", "snooze", "7", "tomorrow"]),
            (Action::Dismiss(12), vec!["inbox", "dismiss", "12"]),
        ];
        for (action, argv) in cases {
            assert_eq!(action.argv(), argv, "{action:?}");
        }
    }

    #[test]
    fn open_manager_is_followed_by_session_open_of_the_manager_from_the_reply() {
        let manager = Action::OpenManager(ContextId::parse("alpha").unwrap());
        let reply = json!({"ok": true, "manager": {"session": "m-alpha", "name": "alpha-manager", "state": "idle", "pane": "%3", "window": "@2"}, "started": true});
        assert_eq!(manager.open_new(&reply), Ok(Some(Action::Open(sid("m-alpha")))));
        assert!(manager.open_new(&json!({"ok": true})).is_err());
        assert!(manager.open_new(&json!({"ok": true, "manager": {"session": "Not an id"}})).is_err());
    }

    #[test]
    fn copy_id_is_tmux_set_buffer_w_with_the_shown_id() {
        let ids = ["alpha", "ship", "ship·7f3a", "f-idle", "-dash"]; // a Context, a Plan, a fork, a fork with no Plan, a `-` id
        for id in ids {
            let copy = Action::CopyId(id.into());
            assert_eq!(sequence(&copy), [vec!["set-buffer", "-w", "--", id]], "{id}");
            assert_eq!(copy.done(&json!(null)), format!("copied {id}"));
        }
        let argv = Tmux::new("sock").argv(&Action::CopyId("ship".into()).argv());
        assert_eq!(argv, ["tmux", "-L", "sock", "-f", "/dev/null", "set-buffer", "-w", "--", "ship"]);
    }

    #[test]
    fn new_plan_and_fork_are_followed_by_session_open_of_the_new_session_from_the_reply() {
        let new = Action::NewPlan { context: ContextId::parse("alpha").unwrap(), id: PlanId::parse("pay").unwrap(), title: "Pay".into() };
        let reply = json!({"ok": true, "plan": {"id": "pay", "lead_session": "8d0e"}, "lead": {"session": "8d0e", "pane": "%41"}});
        let open = new.open_new(&reply).unwrap().unwrap();
        assert_eq!(
            [new.argv(), open.argv()],
            [vec!["plan", "new", "pay", "--context=alpha", "--title=Pay"], vec!["session", "open", "8d0e"]]
        );
        let fork = Action::Fork { source: sid("l-ship"), name: "ship (fork)".into() };
        let reply = json!({"ok": true, "session": {"id": "a41b", "name": "ship (fork)", "plan": "ship", "pane": "%52"}});
        let open = fork.open_new(&reply).unwrap().unwrap();
        assert_eq!(
            [fork.argv(), open.argv()],
            [vec!["session", "fork", "l-ship", "--name=ship (fork)"], vec!["session", "open", "a41b"]]
        );
        // a reply without the id is an error, not a silent skip; the other actions open nothing
        assert!(new.open_new(&json!({"ok": true})).is_err());
        assert!(fork.open_new(&json!({"ok": true, "lead": {"session": "8d0e"}})).is_err());
        for action in [Action::Open(sid("l-ship")), Action::Close(sid("l-ship")), Action::CopyId("x".into())] {
            assert_eq!(action.open_new(&json!({"ok": true, "session": {"id": "a41b"}})), Ok(None), "{action:?}");
        }
    }

    #[test]
    fn the_note_says_what_the_reference_says() {
        let open = Action::Open(sid("l-ship"));
        assert_eq!(open.done(&json!({"ok": true, "session": "l-ship", "pane": "%1", "displaced": null})), "");
        let close = Action::Close(sid("l-ship"));
        assert_eq!(close.done(&json!({"ok": true, "session": "l-ship", "pane": "%4"})), "closed");
        assert_eq!(close.done(&json!({"ok": true, "already": true})), "session is already gone");
        let new = Action::NewPlan { context: ContextId::parse("alpha").unwrap(), id: PlanId::parse("pay").unwrap(), title: "Pay".into() };
        assert_eq!(new.done(&json!({"ok": true})), "forking pay in window park-pay");
        let context = Action::NewContext { id: ContextId::parse("alpha").unwrap(), title: "Alpha".into(), owner: "o".into() };
        assert_eq!(context.done(&json!({"ok": true})), "created alpha");
        let rename = Action::RenameSession { id: sid("f-idle"), name: "x".into() };
        assert_eq!(rename.done(&json!({"ok": true, "name": "x"})), "renamed");
        assert_eq!(rename.done(&json!({"ok": true, "already": true})), "already named so");
        let fork = Action::Fork { source: sid("l-ship"), name: "ship (fork)".into() };
        assert_eq!(fork.done(&json!({"ok": true})), "forking ship (fork)");
        let archive = archive(Subject::Session(sid("f-idle")));
        assert_eq!(archive.done(&json!({"ok": true, "archived": [], "closed": []})), "archived");
        assert_eq!(archive.done(&json!({"ok": true, "already": true})), "already archived");
        let unarchive = Action::Unarchive(Subject::Session(sid("f-idle")));
        assert_eq!(unarchive.done(&json!({"ok": true, "archived": [], "closed": []})), "unarchived");
        assert_eq!(unarchive.done(&json!({"ok": true, "already": true})), "already unarchived");
        assert_eq!(Action::Snooze { item: 1, until: Until::Hour1 }.done(&json!({"ok": true})), "snoozed");
        assert_eq!(Action::Dismiss(1).done(&json!({"ok": true})), "dismissed");
        assert_eq!(Action::Dismiss(1).failed("item_closed", "Item 1 is closed."), "item_closed: Item 1 is closed.");
    }

    #[test]
    fn closing_the_session_in_the_chat_slot_is_refused_in_the_references_words() {
        let close = Action::Close(sid("l-ship"));
        assert_eq!(close.failed("in_chat_slot", "It is in the chat slot."), IN_CHAT_SLOT);
        assert_eq!(close.failed("not_a_lead", "Not a Lead."), "not_a_lead: Not a Lead.");
        assert_eq!(Action::Open(sid("l-ship")).failed("in_chat_slot", "x"), "in_chat_slot: x");
        let archive = archive(Subject::Session(sid("l-ship")));
        assert_eq!(archive.failed("archive_blocked", "A Session is working."), "archive_blocked: A Session is working.");
    }
}
