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
    ("context show", &[Lead, Manager, Worker], false),
    ("context search", &[Lead, Manager, Worker], false),
    ("context reindex", &[Lead, Manager], false),
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
    ("context owner", &[Lead, Manager], false),
    ("context delete", &[], true),
    ("plan new", &[Lead, Manager], true),
    ("plan list", &[Lead, Manager], false),
    ("plan show", &[Lead, Manager], false),
    ("plan phase", &[Lead], false),
    ("plan render", &[Lead], false),
    ("plan review", &[Lead], false),
    ("plan outline", &[Lead], false),
    ("plan lock", &[Lead], false),
    ("plan rename", &[], true),
    ("plan archive", &[], true),
    ("plan unarchive", &[], true),
    ("plan cancel", &[Lead], false),
    ("plan delete", &[], true),
    ("job add", &[Lead], false),
    ("job edit", &[Lead], false),
    ("job cancel", &[Lead], false),
    ("job retry", &[Lead], false),
    ("job urgent", &[Lead, Manager], false),
    ("job approve-tests", &[Lead], false),
    ("job addendum", &[Lead], false),
    ("job dispatch", &[], false),
    ("job list", &[Lead], false),
    ("job show", &[Lead], false),
    ("attempt submit", &[Worker], false),
    ("attempt read", &[Lead], false),
    ("attempt show", &[Lead], false),
    ("attempt list", &[Lead], false),
    ("ask", &[Worker], false),
    ("answer", &[Lead], false),
    ("board post", &[Lead, Manager], false),
    ("board list", &[Lead, Manager], false),
    ("pr add", &[Lead], false),
    ("pr edit", &[Lead], false),
    ("pr describe", &[Lead], false),
    ("pr cancel", &[Lead], false),
    ("pr start", &[Lead], false),
    ("pr open", &[Lead], false),
    ("pr poll", &[Lead], false),
    ("pr rebase", &[Lead], false),
    ("pr push", &[Lead], false),
    ("pr view", &[Lead], false),
    ("pr reply", &[Lead], false),
    ("pr rerun", &[Lead], false),
    ("pr post", &[Lead], false),
    ("pr merge", &[Lead], true),
    ("review submit", &[Lead, Manager], false),
    ("policy show", &[Lead, Manager, Worker], false),
    ("mcp attach", &[Manager, Lead, Worker], false),
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
    ("session resume", &[], true),
    ("session retier", &[Manager], false),
    ("session rename", &[Lead], true),
    ("events list", &[Manager], false),
    ("events follow", &[], false),
    ("incident start", &[Manager], false),
    ("incident end", &[Manager], false),
    ("incident status", &[Manager, Lead, Worker], false),
    ("inbox list", &[], false),
    ("inbox show", &[], false),
    ("inbox snooze", &[], false),
    ("inbox dismiss", &[], false),
    ("inbox claim", &[], false),
    ("inbox approve", &[], false),
    ("watch", &[Lead], false),
    ("tui", &[], false),
    ("doctor", &[], false),
    ("config", &[], false),
    ("tiers", &[Lead], false),
    ("models refresh", &[Manager, Lead], false),
    ("models show", &[Manager, Lead], false),
    ("preset list", &[Manager, Lead], false),
    ("preset show", &[Manager, Lead], false),
    ("preset apply", &[Manager], false),
    ("install", &[], false),
    ("secret get", &[Lead, Manager, Worker], false),
    ("secret list", &[Lead, Manager], false),
    ("secret create", &[Lead, Manager], false),
    ("secret edit", &[Lead, Manager], false),
    ("secret delete", &[Lead, Manager], false),
    ("secret allow", &[Lead], false),
    ("gate finish", &[], false),
    ("host repair", &[Manager], false),
    ("test changed", &[Manager, Lead, Worker], false),
    ("test run", &[Manager, Lead, Worker], false),
    ("test list", &[Manager, Lead, Worker], false),
    ("test build", &[Manager, Lead, Worker], false),
    ("resource list", &[Manager, Lead, Worker], false),
    ("resource run", &[Manager, Lead, Worker], false),
    ("prune", &[], false),
    ("janitor", &[], false),
    ("tag", &[Lead], false),
    ("version sync", &[Lead], false),
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
