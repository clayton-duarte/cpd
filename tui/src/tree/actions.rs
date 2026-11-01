//! The tree's actions (commands.md §13.2, §17): the view changes state only by
//! running `cpd` commands as subprocesses. `cpd` is resolved on `PATH`, as in
//! every pane cpd starts ([`PROGRAM`]). Besides those, it lays out the cockpit
//! as the reference does: a Session opened in the chat slot gets the focus
//! (`select-pane`), and `?` opens the legend popup.
//! A click on a Context row runs `context manager` (§3.10), then the Open of the Manager it returns.
//! The menu's Open, Close, Archive, Unarchive, Rename…, Fork…, New session…, New Plan… and New Context… are
//! the same commands (all but Open and Unarchive after a dialog popup, [`super::ask`]); Copy id is the one that runs `tmux set-buffer -w` instead (OSC 52). After New Plan…, Fork… and
//! New session… succeed, the new Lead is opened in the chat slot (`session open`, then the focus, as Open does). Two flows run more than one: an Archive of the chat slot's Lead frees the slot first
//! ([`Free`], F3), and a click on an archived row unarchives its outermost archived object and opens its Session ([`Action::Resume`]).
//! `session history` is no click any more (hs-d515); it stays a command.
//!
//! Text the User typed is passed as `--flag=value`, or after `--` when it is a positional, so a value that starts
//! with `-` is still a value.

use std::sync::Arc;

use serde_json::Value;

use super::ask::{Dialogs, Question};
use crate::dialog::{self, validate::Validator};
use crate::widgets::open_legend;
use cpd_core::config::Config;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::deadline::Failed as RunFailed;
use cpd_core::host::layout::MAIN_WINDOW;
use cpd_core::host::runner::{Call, NO_DEADLINE, Runner as ProcessRunner};
use cpd_core::host::{Tmux, chat_slot};
use cpd_core::ids::{ContextId, PlanId, SessionId};
use cpd_core::ports::Ports;
use cpd_core::session::PROGRAM;
use cpd_core::store::Store;

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

/// What a delete is about: only a Plan and a Context are deleted (a Lead goes with its Plan).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Deletable {
    Context(ContextId),
    Plan(PlanId),
}

impl Deletable {
    /// `plan delete <id>` or `context delete <id>`, then `--yes`: without it the command only says what would go.
    fn command(&self, yes: bool) -> Vec<String> {
        let (area, id) = match self {
            Deletable::Context(id) => ("context", id.to_string()),
            Deletable::Plan(id) => ("plan", id.to_string()),
        };
        let mut argv = vec![area.to_owned(), "delete".to_owned(), id];
        if yes {
            argv.push("--yes".to_owned());
        }
        argv
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
    NewPlan {
        context: ContextId,
        id: PlanId,
        title: String,
    },
    /// `context new <id> --title=<title> --owner=<login>` (§3.1): New Context….
    NewContext {
        id: ContextId,
        title: String,
        owner: String,
    },
    /// `context rename <id> --title=<title>` (§3.7).
    RenameContext { id: ContextId, title: String },
    /// `plan rename --plan=<id> --title=<title>` (§4.9).
    RenamePlan { id: PlanId, title: String },
    /// `session rename --session=<id> -- <name>` (§10.7).
    RenameSession { id: SessionId, name: String },
    /// `session fork <source> --name=<name>` (§10.3): Fork… and New session….
    Fork { source: SessionId, name: String },
    /// `session close <id>` (§10.9): `x` and the menu's Close, at once.
    Close(SessionId),
    /// `context|plan|session archive` (§3.8, §4.10, §10.10): the menu's Archive, at once, after
    /// `free` when the archived Lead holds the chat slot.
    Archive {
        subject: Subject,
        free: Option<Free>,
    },
    /// `context|plan|session unarchive` (§3.9, §4.11, §10.11): the menu's Unarchive.
    Unarchive(Subject),
    /// A click on an archived row: `unarchive` of its outermost archived object, then `then` (the Open of its Session, or
    /// the OpenManager of a Context row): one action that resumes the chat (contract §2.2 "Asleep", hs-d515).
    Resume {
        unarchive: Subject,
        then: Box<Action>,
    },
    /// `tmux set-buffer -w -- <id>` (hs-d102): the menu's Copy id; the id is the one the row shows. The only action
    /// that is no `cpd` command.
    CopyId(String),
    /// `tmux set-buffer -w -- <url>`: a click on an item that only has a link (no Plan to open); the
    /// click says nothing (the label is a `Link` whose click opens the page).
    CopyUrl(String),
    /// `session resume <id>` (§10.17): a click on a `loop-guard` item, in the background with no dialog.
    #[allow(dead_code)] // stub: the click that builds it is not written yet
    SessionResume(SessionId),
    /// `watch --ensure` (§12): a click on an `engine-down` item starts the engine; idempotent.
    EnsureWatch,
    /// `session send <manager> --unblocks <why> -- <text>` (§10.13): the engine's failure to start, to the most recently active live
    /// Manager. No sender, so `from` is the User's side; send always wakes. Only a step of the tree's own restart of the engine.
    Tell { manager: SessionId, text: String },
    /// `plan delete <id>` or `context delete <id>` (§4.14, §3.14): the menu's Delete. Without `yes` the command deletes
    /// nothing and lists what would go, for the popup ([`super::ask::Question::Delete`]); with it, it deletes.
    Delete { what: Deletable, yes: bool },
}

/// The refusal of `x` on the Session in the chat slot (the reference's), also when `session close` refuses it
/// because it moved there while the popup was open.
pub const IN_CHAT_SLOT: &str = "switch to another session first";

impl Action {
    /// The arguments of the command: `cpd`'s, or `tmux`'s for Copy id (the [`Tmux`] adds `-L <socket>`).
    pub fn argv(&self) -> Vec<String> {
        let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        match self {
            Action::Open(id) => words(&["session", "open", id.as_str()]),
            Action::OpenManager(context) => vec![
                "context".into(),
                "manager".into(),
                format!("--context={context}"),
                "--by-user".into(),
            ],
            Action::Park => words(&["session", "park"]),
            Action::History(id) => words(&["session", "history", id.as_str()]),
            Action::NewPlan { context, id, title } => {
                let mut argv = words(&["plan", "new", id.as_str()]);
                argv.extend([
                    format!("--context={context}"),
                    format!("--title={title}"),
                    "--by-user".into(),
                ]);
                argv
            }
            Action::NewContext { id, title, owner } => {
                vec![
                    "context".into(),
                    "new".into(),
                    id.to_string(),
                    format!("--title={title}"),
                    format!("--owner={owner}"),
                ]
            }
            Action::RenameContext { id, title } => vec![
                "context".into(),
                "rename".into(),
                id.to_string(),
                format!("--title={title}"),
            ],
            Action::RenamePlan { id, title } => vec![
                "plan".into(),
                "rename".into(),
                format!("--plan={id}"),
                format!("--title={title}"),
            ],
            Action::RenameSession { id, name } => {
                vec![
                    "session".into(),
                    "rename".into(),
                    format!("--session={id}"),
                    "--".into(),
                    name.clone(),
                ]
            }
            Action::Fork { source, name } => vec![
                "session".into(),
                "fork".into(),
                source.to_string(),
                format!("--name={name}"),
            ],
            Action::Close(id) => words(&["session", "close", id.as_str()]),
            Action::Archive { subject, .. } => subject.command("archive"),
            Action::Unarchive(subject)
            | Action::Resume {
                unarchive: subject, ..
            } => subject.command("unarchive"),
            Action::CopyId(text) | Action::CopyUrl(text) => {
                words(&["set-buffer", "-w", "--", text.as_str()])
            }
            Action::SessionResume(id) => words(&["session", "resume", id.as_str()]),
            Action::EnsureWatch => words(&["watch", "--ensure"]),
            Action::Tell { manager, text } => vec![
                "session".into(),
                "send".into(),
                manager.to_string(),
                "--unblocks".into(),
                "the engine has been down 60 s".into(),
                "--".into(),
                text.clone(),
            ],
            Action::Delete { what, yes } => what.command(*yes),
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
            Action::Archive {
                subject,
                free: Some(free),
            } => vec![
                match free {
                    Free::Open(id) => Action::Open(id.clone()),
                    Free::Park => Action::Park,
                },
                Action::Archive {
                    subject: subject.clone(),
                    free: None,
                },
            ],
            Action::Resume { unarchive, then } => {
                let mut steps = vec![Action::Unarchive(unarchive.clone())];
                steps.extend(then.steps());
                steps
            }
            action => vec![action.clone()],
        }
    }

    /// What the log says when the command succeeded, from its reply: the reference's words. The view draws nothing.
    pub fn done(&self, reply: &Value) -> String {
        match self {
            Action::Open(_)
            | Action::OpenManager(_)
            | Action::Park
            | Action::History(_)
            | Action::Resume { .. } => String::new(),
            Action::NewPlan { id, .. } => format!("forking {id} in window park-{id}"),
            Action::NewContext { id, .. } => format!("created {id}"),
            Action::RenameContext { .. }
            | Action::RenamePlan { .. }
            | Action::RenameSession { .. }
                if reply["already"] == true =>
            {
                "already named so".into()
            }
            Action::RenameContext { .. }
            | Action::RenamePlan { .. }
            | Action::RenameSession { .. } => "renamed".into(),
            Action::Fork { name, .. } => format!("forking {name}"),
            Action::Close(_) if reply["already"] == true => "session is already gone".into(),
            Action::Close(_) => "closed".into(),
            Action::Archive { .. } if reply["already"] == true => "already archived".into(),
            Action::Archive { .. } => "archived".into(),
            Action::Unarchive(_) if reply["already"] == true => "already unarchived".into(),
            Action::Unarchive(_) => "unarchived".into(),
            Action::CopyId(id) => format!("copied {id}"),
            Action::CopyUrl(url) => format!("copied {url}: Cmd-click the row to open it"),
            Action::EnsureWatch if reply["already"] == true => "already running".into(),
            Action::EnsureWatch => "engine started".into(),
            Action::SessionResume(id) => format!("resumed {id}"),
            Action::Tell { manager, .. } => format!("told {manager}"),
            Action::Delete { .. } if reply["already"] == true => "already deleted".into(),
            Action::Delete { .. } => "deleted".into(),
        }
    }

    /// Why the command failed with `code` and `message`, as the dialog says it.
    pub fn failed(&self, code: &str, message: &str) -> String {
        match (self, code) {
            (Action::Close(_), "in_chat_slot") => IN_CHAT_SLOT.into(),
            _ => format!("{code}: {message}"),
        }
    }
}

/// Runs actions against the cockpit of `config`.
pub struct Runner {
    home: CpdHome,
    busy_ms: u64,
    tmux: Tmux,
    cockpit: String,
    /// What runs the `cpd` commands, as it runs the tmux calls.
    processes: Arc<dyn ProcessRunner>,
}

impl Runner {
    /// A runner whose commands and tmux calls run on the runner and the clock of `ports`.
    pub fn new(home: CpdHome, config: &Config, ports: &Ports) -> Runner {
        Runner {
            home,
            busy_ms: u64::from(config.timeouts.store_busy_ms),
            tmux: Tmux::new(config.tmux.socket.clone())
                .with_runner(ports.runner.clone())
                .with_clock(ports.clock.clone()),
            cockpit: config.tmux.cockpit_session.clone(),
            processes: ports.runner.clone(),
        }
    }

    /// Runs the command of `action`: its reply, or why it failed.
    fn command(&self, action: &Action) -> Result<Value, String> {
        command_on(&*self.processes, action)
    }

    /// The live Managers of the machine (role `manager`, not archived), oldest first: who hears that the engine cannot be started.
    pub fn live_managers(&self) -> Result<Vec<SessionId>, String> {
        let mut store =
            Store::open(&self.home, self.busy_ms).map_err(|e| e.message().to_owned())?;
        store
            .read(|conn| {
                let mut stmt =
                    conn.prepare("SELECT id FROM sessions WHERE role = 'manager' AND archived_at IS NULL ORDER BY state_at DESC, id LIMIT 1")?;
                let ids = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(ids)
            })
            .and_then(|ids| ids.iter().map(|id| SessionId::parse(id)).collect())
            .map_err(|e| e.message().to_owned())
    }

    /// The tree as of `now` (contract §1.3 text): what [`crate::widgets::inbox::click`] reads the Sessions from.
    pub fn tree(&self, now: &str) -> Result<crate::read::Tree, String> {
        let mut store =
            Store::open(&self.home, self.busy_ms).map_err(|e| e.message().to_owned())?;
        crate::read::tree_at(&mut store, Some(now)).map_err(|e| e.message().to_owned())
    }

    /// Runs the action's steps: what the log says of the last, or the step that failed and why (the flow stops there).
    pub fn run(&self, action: &Action) -> Result<String, Failed> {
        self.run_all(&action.steps())
    }

    /// Runs each of `steps` (themselves expanded to their own steps), stopping at the first that fails: what the log
    /// says of the last.
    pub fn run_all(&self, steps: &[Action]) -> Result<String, Failed> {
        let mut said = String::new();
        for step in steps.iter().flat_map(Action::steps) {
            let fail = |why: String| Failed {
                step: step.clone(),
                why,
            };
            let reply = match step {
                Action::CopyId(_) | Action::CopyUrl(_) => self
                    .tmux
                    .run(&step.argv())
                    .map(|_| Value::Null)
                    .map_err(|e| fail(e.message().to_owned()))?,
                _ => self.command(&step).map_err(fail)?,
            };
            said = step.done(&reply);
            if let Some(open) = step.open_new(&reply).map_err(fail)? {
                // the Plan or fork stays created when this is refused
                self.command(&open).map_err(|why| Failed {
                    step: open.clone(),
                    why,
                })?;
                self.focus_chat()
                    .map_err(|e| fail(e.message().to_owned()))?;
            }
            if matches!(step, Action::Open(_) | Action::History(_)) {
                self.focus_chat()
                    .map_err(|e| fail(e.message().to_owned()))?;
            }
        }
        Ok(said)
    }

    /// `select-pane` on the chat slot, so typing goes to the chat (the reference's `switch_to`).
    fn focus_chat(&self) -> Result<(), cpd_core::error::CpdError> {
        let panes: Vec<_> = self
            .tmux
            .list_panes()?
            .into_iter()
            .filter(|p| p.session_name == self.cockpit && p.window_name == MAIN_WINDOW)
            .collect();
        if let Some(chat) = chat_slot(&panes, &self.cockpit) {
            self.tmux
                .run(&["select-pane", "-t", chat.pane.as_str()].map(str::to_owned))?;
        }
        Ok(())
    }

    /// Opens the dialog popup of `question` and returns when it closes: the command its answer makes, none when the
    /// User cancelled.
    pub fn ask(&self, question: Question) -> Result<Option<Action>, String> {
        question.ask(self)
    }

    /// A click on a `curate-retire` or `curate-docs` item: `cpd context curation`, the checklist popup of what is
    /// pending, and on approve the command of the ticks (`cpd context retire|docs`); cancel runs nothing. A refusal is
    /// a `text` dialog (commands.md §17 "No notes"); returns when the dialogs close.
    pub fn curate(
        &self,
        kind: cpd_core::inbox::model::Kind,
        context: &ContextId,
    ) -> Result<(), String> {
        let title = if kind == cpd_core::inbox::model::Kind::CurateDocs {
            "Documentation candidates"
        } else {
            "Retire records"
        };
        match self.curate_flow(kind, context) {
            Ok(()) => Ok(()),
            Err(why) => self.error(title, &why),
        }
    }

    fn curate_flow(
        &self,
        kind: cpd_core::inbox::model::Kind,
        context: &ContextId,
    ) -> Result<(), String> {
        use crate::widgets::inbox::{approved, checklist};
        let call_cpd = |argv: Vec<String>| {
            call_argv(&*self.processes, argv).map_err(|failure| match failure {
                Failure::Refused { code, message } => format!("{code}: {message}"),
                Failure::Broken(why) => why,
            })
        };
        let reply = call_cpd(crate::widgets::inbox::curation_argv(context))?;
        let Some(list) = checklist(kind, context, &reply) else {
            return Ok(());
        };
        let rows: Vec<_> = list
            .rows
            .into_iter()
            .map(|r| (r.id, r.text, r.detail))
            .collect();
        let checked = dialog::checklist(&self.tmux, &list.title, &rows)
            .map_err(|e| e.message().to_owned())?;
        if checked.approved {
            call_cpd(approved(kind, context, &checked.ticked, &checked.unticked))?;
        }
        Ok(())
    }

    /// A failure the User must see: a `text` popup titled `title` (what failed) saying `text` (why); returns when it closes.
    pub fn error(&self, title: &str, text: &str) -> Result<(), String> {
        dialog::show_error(&self.tmux, title, text).map_err(|e| e.message().to_owned())
    }

    /// `?`: the legend popup; returns when it closes.
    pub fn legend(&self) -> Result<(), String> {
        open_legend(&self.tmux).map_err(|e| e.message().to_owned())
    }
}

impl Dialogs for Runner {
    fn input(
        &self,
        title: &str,
        prompt: &str,
        initial: &str,
        validator: Validator,
    ) -> Result<Option<String>, CpdError> {
        dialog::input(&self.tmux, title, prompt, initial, validator)
    }

    fn confirm(&self, title: &str, text: &str, yes_label: &str) -> Result<bool, CpdError> {
        dialog::confirm(&self.tmux, title, text, yes_label)
    }

    fn select(
        &self,
        title: &str,
        prompt: &str,
        options: &[String],
    ) -> Result<Option<String>, CpdError> {
        dialog::select(&self.tmux, title, prompt, options)
    }

    fn form(
        &self,
        title: &str,
        input_label: &str,
        select_label: &str,
        options: &[String],
    ) -> Result<Option<(String, String)>, CpdError> {
        dialog::form(&self.tmux, title, input_label, select_label, options)
    }

    fn preview(&self, what: &Deletable) -> Result<Value, String> {
        self.command(&Action::Delete {
            what: what.clone(),
            yes: false,
        })
    }
}

/// The step of a flow that did not succeed, and why (`<code>: <message>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failed {
    pub step: Action,
    pub why: String,
}

/// Why a command did not succeed: `cpd` refused it (`code` and `message` of its error), or it could not be run or printed
/// no JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Refused { code: String, message: String },
    Broken(String),
}

impl Failure {
    /// The error's `code`; `error` for a command that could not be run.
    pub fn code(&self) -> &str {
        match self {
            Failure::Refused { code, .. } => code,
            Failure::Broken(_) => "error",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Failure::Refused { message, .. } | Failure::Broken(message) => message,
        }
    }
}

/// Runs the command of `action` on `runner`: its reply, or why it failed.
pub fn call_with(runner: &dyn ProcessRunner, action: &Action) -> Result<Value, Failure> {
    call_argv(runner, action.argv())
}

/// [`call_with`] of the `cpd` arguments `argv`.
fn call_argv(runner: &dyn ProcessRunner, argv: Vec<String>) -> Result<Value, Failure> {
    let call = Call {
        tool: PROGRAM.to_owned(),
        program: PROGRAM.to_owned(),
        args: argv.clone(),
        envs: Vec::new(),
        cwd: None,
        deadline: NO_DEADLINE,
        stdin: None,
        stderr_to: None,
    };
    let output = runner
        .run(&call)
        .map_err(|e| Failure::Broken(format!("cannot run {PROGRAM}: {}", failed_reason(e))))?;
    let reply: Value = serde_json::from_slice(&output.stdout).map_err(|_| {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Failure::Broken(format!(
            "{PROGRAM} {} printed no JSON (exit {}): {}",
            argv.join(" "),
            output
                .status
                .code()
                .map_or_else(|| output.status.to_string(), |c| format!("exit code: {c}")),
            stderr.trim()
        ))
    })?;
    if reply["ok"] == true {
        Ok(reply)
    } else {
        let error = &reply["error"];
        Err(Failure::Refused {
            code: error["code"].as_str().unwrap_or("error").to_owned(),
            message: error["message"].as_str().unwrap_or("failed").to_owned(),
        })
    }
}

fn failed_reason(failed: RunFailed) -> String {
    match failed {
        RunFailed::Io(e) => e.to_string(),
        RunFailed::Timeout(e) => e.message().to_owned(),
    }
}

/// [`call_with`], the failure in the words of the dialog.
fn command_on(runner: &dyn ProcessRunner, action: &Action) -> Result<Value, String> {
    call_with(runner, action).map_err(|failure| match failure {
        Failure::Refused { code, message } => action.failed(&code, &message),
        Failure::Broken(why) => why,
    })
}

#[cfg(test)]
#[path = "actions_by_user_tests.rs"]
mod by_user_tests;

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn sid(s: &str) -> SessionId {
        SessionId::parse(s).unwrap()
    }

    fn archive(subject: Subject) -> Action {
        Action::Archive {
            subject,
            free: None,
        }
    }

    fn sequence(action: &Action) -> Vec<Vec<String>> {
        action.steps().iter().map(Action::argv).collect()
    }

    #[test]
    fn an_archive_of_the_chat_slots_lead_frees_the_slot_first_and_any_other_action_is_one_step() {
        let plan = Subject::Plan(PlanId::parse("ship").unwrap());
        let open = Action::Archive {
            subject: plan.clone(),
            free: Some(Free::Open(sid("l-sketch"))),
        };
        assert_eq!(
            sequence(&open),
            [
                vec!["session", "open", "l-sketch"],
                vec!["plan", "archive", "--plan=ship"]
            ]
        );
        let park = Action::Archive {
            subject: Subject::Session(sid("f-idle")),
            free: Some(Free::Park),
        };
        assert_eq!(
            sequence(&park),
            [
                vec!["session", "park"],
                vec!["session", "archive", "f-idle"]
            ]
        );
        assert_eq!(
            sequence(&archive(plan)),
            [vec!["plan", "archive", "--plan=ship"]]
        );
        assert_eq!(
            sequence(&Action::History(sid("f-idle"))),
            [vec!["session", "history", "f-idle"]]
        );
        assert_eq!(
            sequence(&Action::Open(sid("l-ship"))),
            [vec!["session", "open", "l-ship"]]
        );
    }

    #[test]
    fn each_action_is_one_command_of_the_contract() {
        let cases = [
            (
                Action::Open(sid("l-ship")),
                vec!["session", "open", "l-ship"],
            ),
            (
                Action::Close(sid("l-ship")),
                vec!["session", "close", "l-ship"],
            ),
            (
                Action::OpenManager(ContextId::parse("alpha").unwrap()),
                vec!["context", "manager", "--context=alpha", "--by-user"],
            ),
            (Action::Park, vec!["session", "park"]),
            (
                Action::History(sid("f-idle")),
                vec!["session", "history", "f-idle"],
            ),
            (
                archive(Subject::Context(ContextId::parse("alpha").unwrap())),
                vec!["context", "archive", "alpha"],
            ),
            (
                archive(Subject::Plan(PlanId::parse("ship").unwrap())),
                vec!["plan", "archive", "--plan=ship"],
            ),
            (
                archive(Subject::Session(sid("f-idle"))),
                vec!["session", "archive", "f-idle"],
            ),
            (
                Action::Unarchive(Subject::Context(ContextId::parse("alpha").unwrap())),
                vec!["context", "unarchive", "alpha"],
            ),
            (
                Action::Unarchive(Subject::Plan(PlanId::parse("ship").unwrap())),
                vec!["plan", "unarchive", "--plan=ship"],
            ),
            (
                Action::Unarchive(Subject::Session(sid("f-idle"))),
                vec!["session", "unarchive", "f-idle"],
            ),
            (
                Action::NewPlan {
                    context: ContextId::parse("alpha").unwrap(),
                    id: PlanId::parse("pay-4521").unwrap(),
                    title: "Pay 4521".into(),
                },
                vec![
                    "plan",
                    "new",
                    "pay-4521",
                    "--context=alpha",
                    "--title=Pay 4521",
                    "--by-user",
                ],
            ),
            (
                Action::NewContext {
                    id: ContextId::parse("building-cpd").unwrap(),
                    title: "Building cpd!".into(),
                    owner: "clayton-duarte".into(),
                },
                vec![
                    "context",
                    "new",
                    "building-cpd",
                    "--title=Building cpd!",
                    "--owner=clayton-duarte",
                ],
            ),
            (
                Action::RenameContext {
                    id: ContextId::parse("alpha").unwrap(),
                    title: "-Dash".into(),
                },
                vec!["context", "rename", "alpha", "--title=-Dash"],
            ),
            (
                Action::RenamePlan {
                    id: PlanId::parse("ship").unwrap(),
                    title: "Ship it".into(),
                },
                vec!["plan", "rename", "--plan=ship", "--title=Ship it"],
            ),
            (
                Action::RenameSession {
                    id: sid("f-idle"),
                    name: "-x y".into(),
                },
                vec!["session", "rename", "--session=f-idle", "--", "-x y"],
            ),
            (
                Action::Fork {
                    source: sid("l-ship"),
                    name: "ship (fork)".into(),
                },
                vec!["session", "fork", "l-ship", "--name=ship (fork)"],
            ),
            (Action::EnsureWatch, vec!["watch", "--ensure"]),
            (
                Action::Tell {
                    manager: sid("m-alpha"),
                    text: "The engine is down.".into(),
                },
                vec![
                    "session",
                    "send",
                    "m-alpha",
                    "--unblocks",
                    "the engine has been down 60 s",
                    "--",
                    "The engine is down.",
                ],
            ),
        ];
        for (action, argv) in cases {
            assert_eq!(action.argv(), argv, "{action:?}");
        }
    }

    #[test]
    fn open_manager_is_followed_by_session_open_of_the_manager_from_the_reply() {
        let manager = Action::OpenManager(ContextId::parse("alpha").unwrap());
        let reply = json!({"ok": true, "manager": {"session": "m-alpha", "name": "alpha-manager", "state": "idle", "pane": "%3", "window": "@2"}, "started": true});
        assert_eq!(
            manager.open_new(&reply),
            Ok(Some(Action::Open(sid("m-alpha"))))
        );
        assert!(manager.open_new(&json!({"ok": true})).is_err());
        assert!(
            manager
                .open_new(&json!({"ok": true, "manager": {"session": "Not an id"}}))
                .is_err()
        );
    }

    #[test]
    fn copy_id_is_tmux_set_buffer_w_with_the_shown_id() {
        let ids = ["alpha", "ship", "ship·7f3a", "f-idle", "-dash"]; // a Context, a Plan, a fork, a fork with no Plan, a `-` id
        for id in ids {
            let copy = Action::CopyId(id.into());
            assert_eq!(
                sequence(&copy),
                [vec!["set-buffer", "-w", "--", id]],
                "{id}"
            );
            assert_eq!(copy.done(&json!(null)), format!("copied {id}"));
        }
        let argv = Tmux::new("sock").argv(&Action::CopyId("ship".into()).argv());
        assert_eq!(
            argv,
            [
                "tmux",
                "-u",
                "-L",
                "sock",
                "-f",
                "/dev/null",
                "set-buffer",
                "-w",
                "--",
                "ship"
            ]
        );
    }

    #[test]
    fn copy_url_is_the_same_tmux_command_and_its_log_line_says_how_to_open_the_link() {
        let copy = Action::CopyUrl("https://x.io/1".into());
        assert_eq!(
            sequence(&copy),
            [vec!["set-buffer", "-w", "--", "https://x.io/1"]]
        );
        assert_eq!(
            copy.done(&json!(null)),
            "copied https://x.io/1: Cmd-click the row to open it"
        );
    }

    #[test]
    fn ensure_watch_says_whether_it_started_the_engine_() {
        assert_eq!(
            Action::EnsureWatch.done(&json!({"ok": true, "started": true, "pane": "%3"})),
            "engine started"
        );
        assert_eq!(
            Action::EnsureWatch.done(&json!({"ok": true, "started": false, "already": true})),
            "already running"
        );
        assert_eq!(
            Action::EnsureWatch.failed("watch_start_failed", "x"),
            "watch_start_failed: x"
        );
    }

    #[test]
    fn new_plan_and_fork_are_followed_by_session_open_of_the_new_session_from_the_reply() {
        let new = Action::NewPlan {
            context: ContextId::parse("alpha").unwrap(),
            id: PlanId::parse("pay").unwrap(),
            title: "Pay".into(),
        };
        let reply = json!({"ok": true, "plan": {"id": "pay", "lead_session": "8d0e"}, "lead": {"session": "8d0e", "pane": "%41"}});
        let open = new.open_new(&reply).unwrap().unwrap();
        assert_eq!(
            [new.argv(), open.argv()],
            [
                vec![
                    "plan",
                    "new",
                    "pay",
                    "--context=alpha",
                    "--title=Pay",
                    "--by-user"
                ],
                vec!["session", "open", "8d0e"]
            ]
        );
        let fork = Action::Fork {
            source: sid("l-ship"),
            name: "ship (fork)".into(),
        };
        let reply = json!({"ok": true, "session": {"id": "a41b", "name": "ship (fork)", "plan": "ship", "pane": "%52"}});
        let open = fork.open_new(&reply).unwrap().unwrap();
        assert_eq!(
            [fork.argv(), open.argv()],
            [
                vec!["session", "fork", "l-ship", "--name=ship (fork)"],
                vec!["session", "open", "a41b"]
            ]
        );
        // a reply without the id is an error, not a silent skip; the other actions open nothing
        assert!(new.open_new(&json!({"ok": true})).is_err());
        assert!(
            fork.open_new(&json!({"ok": true, "lead": {"session": "8d0e"}}))
                .is_err()
        );
        for action in [
            Action::Open(sid("l-ship")),
            Action::Close(sid("l-ship")),
            Action::CopyId("x".into()),
        ] {
            assert_eq!(
                action.open_new(&json!({"ok": true, "session": {"id": "a41b"}})),
                Ok(None),
                "{action:?}"
            );
        }
    }

    #[test]
    fn the_log_line_says_what_the_reference_says() {
        let open = Action::Open(sid("l-ship"));
        assert_eq!(
            open.done(&json!({"ok": true, "session": "l-ship", "pane": "%1", "displaced": null})),
            ""
        );
        let close = Action::Close(sid("l-ship"));
        assert_eq!(
            close.done(&json!({"ok": true, "session": "l-ship", "pane": "%4"})),
            "closed"
        );
        assert_eq!(
            close.done(&json!({"ok": true, "already": true})),
            "session is already gone"
        );
        let new = Action::NewPlan {
            context: ContextId::parse("alpha").unwrap(),
            id: PlanId::parse("pay").unwrap(),
            title: "Pay".into(),
        };
        assert_eq!(
            new.done(&json!({"ok": true})),
            "forking pay in window park-pay"
        );
        let context = Action::NewContext {
            id: ContextId::parse("alpha").unwrap(),
            title: "Alpha".into(),
            owner: "o".into(),
        };
        assert_eq!(context.done(&json!({"ok": true})), "created alpha");
        let rename = Action::RenameSession {
            id: sid("f-idle"),
            name: "x".into(),
        };
        assert_eq!(rename.done(&json!({"ok": true, "name": "x"})), "renamed");
        assert_eq!(
            rename.done(&json!({"ok": true, "already": true})),
            "already named so"
        );
        let fork = Action::Fork {
            source: sid("l-ship"),
            name: "ship (fork)".into(),
        };
        assert_eq!(fork.done(&json!({"ok": true})), "forking ship (fork)");
        let archive = archive(Subject::Session(sid("f-idle")));
        assert_eq!(
            archive.done(&json!({"ok": true, "archived": [], "closed": []})),
            "archived"
        );
        assert_eq!(
            archive.done(&json!({"ok": true, "already": true})),
            "already archived"
        );
        let unarchive = Action::Unarchive(Subject::Session(sid("f-idle")));
        assert_eq!(
            unarchive.done(&json!({"ok": true, "archived": [], "closed": []})),
            "unarchived"
        );
        assert_eq!(
            unarchive.done(&json!({"ok": true, "already": true})),
            "already unarchived"
        );
    }

    #[test]
    fn closing_the_session_in_the_chat_slot_is_refused_in_the_references_words() {
        let close = Action::Close(sid("l-ship"));
        assert_eq!(
            close.failed("in_chat_slot", "It is in the chat slot."),
            IN_CHAT_SLOT
        );
        assert_eq!(
            close.failed("not_a_lead", "Not a Lead."),
            "not_a_lead: Not a Lead."
        );
        assert_eq!(
            Action::Open(sid("l-ship")).failed("in_chat_slot", "x"),
            "in_chat_slot: x"
        );
        let archive = archive(Subject::Session(sid("l-ship")));
        assert_eq!(
            archive.failed("archive_blocked", "A Session is working."),
            "archive_blocked: A Session is working."
        );
    }
}
