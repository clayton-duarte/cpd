//! The interface tree printed by `cpd --help` (commands.md §1.2, §2): clap's
//! command tree plus, for each runnable non-internal command, its callers and
//! cockpit flag, kept here by hand.

use serde::Serialize;
use serde_json::{Value, json};

/// An agent role that is meant to run a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Caller {
    Lead,
    Manager,
    Worker,
}

use Caller::{Lead, Manager, Worker};

/// One entry: command path, callers, cockpit. In clap's order.
pub type Entry = (&'static str, &'static [Caller], bool);

pub const TABLE: &[Entry] = &[
    ("context new", &[Lead], true),
    ("context list", &[Lead, Manager], false),
    ("context show", &[Lead, Manager], false),
    ("context decide", &[Lead, Manager], false),
    ("context note", &[Lead, Manager], false),
    ("context ask", &[Lead, Manager], false),
    ("context answer", &[Lead, Manager], false),
    ("context rename", &[], true),
    ("context archive", &[], true),
    ("context unarchive", &[], true),
    ("context manager", &[Manager], true),
    ("context pending", &[Lead, Manager], false),
    ("context distill", &[Lead], false),
    ("plan new", &[Lead, Manager], true),
    ("plan list", &[Lead, Manager], false),
    ("plan show", &[Lead, Manager], false),
    ("plan phase", &[Lead], false),
    ("plan render", &[Lead], false),
    ("plan review", &[Lead], false),
    ("plan lock", &[Lead], false),
    ("plan rename", &[], true),
    ("plan archive", &[], true),
    ("plan unarchive", &[], true),
    ("plan cancel", &[Lead], false),
    ("job add", &[Lead], false),
    ("job edit", &[Lead], false),
    ("job cancel", &[Lead], false),
    ("job retry", &[Lead], false),
    ("job dispatch", &[], false),
    ("job list", &[Lead], false),
    ("job show", &[Lead], false),
    ("attempt submit", &[Worker], false),
    ("attempt read", &[Lead], false),
    ("attempt show", &[Lead], false),
    ("attempt list", &[Lead], false),
    ("ask", &[Worker], false),
    ("answer", &[Lead], false),
    ("pr add", &[Lead], false),
    ("pr edit", &[Lead], false),
    ("pr cancel", &[Lead], false),
    ("pr start", &[Lead], false),
    ("pr open", &[Lead], false),
    ("pr poll", &[Lead], false),
    ("pr rebase", &[Lead], false),
    ("pr push", &[Lead], false),
    ("pr view", &[Lead], false),
    ("pr merge", &[], true),
    ("session list", &[Lead, Manager], false),
    ("session show", &[Lead, Manager], false),
    ("session fork", &[Lead], true),
    ("session open", &[Lead, Manager], true),
    ("session park", &[Lead], true),
    ("session close", &[Lead], true),
    ("session history", &[], true),
    ("session archive", &[], true),
    ("session unarchive", &[], true),
    ("session send", &[Lead, Manager], false),
    ("session rename", &[Lead], true),
    ("events list", &[Manager], false),
    ("events follow", &[], false),
    ("inbox list", &[], false),
    ("inbox show", &[], false),
    ("inbox snooze", &[], false),
    ("inbox dismiss", &[], false),
    ("watch", &[Lead], false),
    ("tui", &[], false),
    ("doctor", &[], false),
    ("config", &[], false),
    ("tiers", &[Lead], false),
];

/// The tree of `cpd --help`: clap's commands depth-first in clap's order,
/// without clap's generated `help`.
pub fn tree(cli: &clap::Command) -> Value {
    Value::Array(nodes(cli, "", false))
}

fn nodes(parent: &clap::Command, prefix: &str, parent_internal: bool) -> Vec<Value> {
    parent
        .get_subcommands()
        .filter(|c| c.get_name() != "help")
        .map(|c| node(c, prefix, parent_internal))
        .collect()
}

fn node(c: &clap::Command, prefix: &str, parent_internal: bool) -> Value {
    let path = if prefix.is_empty() { c.get_name().to_owned() } else { format!("{prefix} {}", c.get_name()) };
    let internal = parent_internal || c.is_hide_set();
    let about = c.get_about().map(|a| a.to_string()).unwrap_or_default();
    let children = nodes(c, &path, internal);
    let mut node = json!({ "name": c.get_name(), "about": about });
    let runnable = children.is_empty() || !c.is_subcommand_required_set();
    if runnable {
        let (callers, cockpit): (&[Caller], bool) = if internal {
            (&[], false)
        } else {
            match TABLE.iter().find(|(p, _, _)| *p == path) {
                Some((_, callers, cockpit)) => (callers, *cockpit),
                None => panic!("command `{path}` has no entry in cli/src/interface.rs"),
            }
        };
        node["callers"] = json!(callers);
        node["cockpit"] = json!(cockpit);
        node["internal"] = json!(internal);
    }
    if !children.is_empty() {
        node["commands"] = Value::Array(children);
    }
    node
}
