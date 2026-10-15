//! Following Events (commands.md §17): one `cpd2 events follow --since <cursor>`
//! subprocess, which the view watches for the kinds that change what it shows.
//! The view then takes a new snapshot (coalesced by the caller): the tree
//! shows counts and states that the Events' `data` does not carry.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};

use serde_json::Value;

/// The kinds the tree acts on: §17's list, with the Job, Attempt, Escalation and Plan review kinds that
/// change a Plan's counters and whether it needs the User.
pub const KINDS: &[&str] = &[
    "context-created",
    "context-grounded",
    "plan-created",
    "plan-phase",
    "plan-review",
    "plan-done",
    "job-added",
    "job-edited",
    "job-merged",
    "job-failed",
    "job-retried",
    "job-cancelled",
    "attempt-status",
    "escalation",
    "escalation-answered",
    "escalation-expired",
    "session-registered",
    "session-state",
    "session-moved",
    "session-renamed",
];

/// What the follower says.
#[derive(Debug, PartialEq, Eq)]
pub enum Message {
    /// An Event of one of [`KINDS`].
    Event,
    /// The follower ended (it was killed, or refused: `cursor_ahead` after the database was replaced).
    Ended,
}

pub struct Follower {
    child: Child,
    pub messages: Receiver<Message>,
}

impl Follower {
    /// Starts `<this binary> events follow --since <cursor>`. The same binary, not `cpd2` on `PATH`: the view and its follower must agree on the schema.
    pub fn start(cursor: i64) -> std::io::Result<Follower> {
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["events", "follow", "--since", &cursor.to_string()]);
        for kind in KINDS {
            command.args(["--kind", kind]);
        }
        let mut child = command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
        let stdout = child.stdout.take().expect("stdout is piped");
        let (tx, messages) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                // an Event of a kind we asked for, one per line (§1.2); anything else is not one
                if serde_json::from_str::<Value>(&line).is_ok_and(|v| v["kind"].is_string()) && tx.send(Message::Event).is_err() {
                    return;
                }
            }
            let _ = tx.send(Message::Ended);
        });
        Ok(Follower { child, messages })
    }
}

impl Drop for Follower {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Kind;

    #[test]
    fn every_kind_followed_exists_and_the_tree_lists_of_17_are_among_them() {
        for kind in KINDS {
            assert!(Kind::ALL.iter().any(|k| k.as_str() == *kind), "{kind} is no Event kind");
        }
        for kind in [
            "context-created",
            "context-grounded",
            "plan-created",
            "plan-phase",
            "plan-done",
            "session-registered",
            "session-state",
            "session-moved",
            "session-renamed",
        ] {
            assert!(KINDS.contains(&kind), "{kind}");
        }
    }
}
