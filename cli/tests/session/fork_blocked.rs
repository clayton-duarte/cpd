//! A fork whose source Lead is `blocked` reports it (contract §6.7 "A blocked source"): one `fork_source_blocked` error
//! Event and one waking `session-message` to the Manager per blocked episode, and a line on the fork's pane. The wait
//! itself is unchanged: `working` and `waiting` sources are waited for silently, and it ends when the source is idle.

use crate::common;

use std::fs;
use std::time::Duration;

use common::fx::Fx;
use common::session::{events, setup};
use common::wrapper::{Pane, Wrapper, installed_cpd};
use serde_json::{Value, json};

const SOURCE: &str = "lead1";
const FINISHED_TURN: &str = "{\"type\":\"session\"}\n{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"stop\"}}\n";

/// A fork of `lead1` with its wrapper running in its pane.
struct Fork {
    fx: Fx,
    w: Wrapper,
    pane: Pane,
}

impl Fork {
    fn new() -> Fork {
        let fx = Fx::with_ports();
        setup(&fx.env);
        fx.cockpit();
        let out = fx.cpd(&["session", "fork", SOURCE, "--name", "side"]).ok();
        let id = out["session"]["id"].as_str().unwrap().to_owned();
        let pane = Pane::at(
            &fx,
            out["session"]["pane"].as_str().unwrap(),
            &installed_cpd(&fx),
        );
        let w = pane.run(&["session", "run", &id]);
        Fork { fx, w, pane }
    }

    fn source_is(&self, state: &str, label: Option<&str>) {
        let label = label.map_or("NULL".to_owned(), |l| format!("'{l}'"));
        self.fx.env.sql(&format!(
            "UPDATE sessions SET state = '{state}', state_label = {label} WHERE id = '{SOURCE}'"
        ));
    }

    fn until_screen(&mut self, text: &str) {
        let pane = &self.pane;
        self.w.until(&format!("{text:?} on the screen"), || {
            pane.screen().contains(text)
        });
    }

    fn reports(&self) -> Vec<Value> {
        events(&self.fx, "error")
            .into_iter()
            .filter(|e| e["data"]["code"] == "fork_source_blocked")
            .collect()
    }

    fn wakes(&self) -> Vec<Value> {
        events(&self.fx, "session-message")
            .into_iter()
            .filter(|e| {
                e["text"]
                    .as_str()
                    .is_some_and(|t| t.contains("which is blocked"))
            })
            .collect()
    }
}

#[test]
fn fork_reports_blocked_source() {
    let mut fork = Fork::new();
    fork.source_is("blocked", Some("dialog"));
    fork.until_screen(
        "The source Lead one is blocked (dialog); the fork waits until it is idle or stopped.",
    );
    // Further polls of the same episode report nothing more.
    fork.w.steps(3);
    assert!(fork.pane.spawn.pis().is_empty());

    let reports = fork.reports();
    assert_eq!(reports.len(), 1, "{reports:?}");
    let report = &reports[0];
    let text = "Fork \"side\" waits for its source \"Lead one\", which is blocked: dialog.";
    assert_eq!(report["text"], text);
    assert_eq!(report["data"]["source"], "session run");
    assert_eq!(report["data"]["details"]["source"], SOURCE);
    assert_eq!(report["data"]["details"]["cause"], "dialog");
    assert_eq!(
        report["data"]["details"]["state_since"],
        "2020-01-01T00:00:01Z"
    );
    assert_eq!(report["context"], "ctx");
    assert_eq!(report["plan"], "pay");

    // The Manager is woken; the source is also the Plan's Lead, so no one else is.
    let wakes = fork.wakes();
    assert_eq!(wakes.len(), 1, "{wakes:?}");
    assert_eq!(wakes[0]["session"], "manager1");
    assert_eq!(wakes[0]["data"]["wake"], json!(true));
    assert_eq!(
        wakes[0]["text"],
        format!("{text} It forks once the source is idle or stopped.")
    );
}

#[test]
fn fork_waits_without_report_on_working_source() {
    let mut fork = Fork::new();
    fork.source_is("working", None);
    fork.until_screen("Waiting for the source Lead to finish its turn");
    fork.w.steps(3);
    fork.source_is("waiting", Some("waiting for a Worker"));
    fork.w.steps(3);
    assert!(fork.reports().is_empty(), "{:?}", fork.reports());
    assert!(fork.wakes().is_empty());

    // Blocked is reported once; back to working ends the episode and reports nothing; blocked again is a new episode.
    fork.source_is("blocked", Some("dialog"));
    fork.until_screen("is blocked (dialog)");
    fork.source_is("working", None);
    fork.w.steps(3);
    assert_eq!(fork.reports().len(), 1);
    fork.source_is("blocked", Some("verdict: ship"));
    let fx = &fork.fx;
    fork.w.until("a second report", || {
        events(fx, "error")
            .iter()
            .filter(|e| e["data"]["code"] == "fork_source_blocked")
            .count()
            >= 2
    });
    assert_eq!(fork.reports().len(), 2);
    assert_eq!(
        fork.reports()[1]["data"]["details"]["cause"],
        "verdict: ship"
    );
    assert!(fork.pane.spawn.pis().is_empty());
}

#[test]
fn fork_blocked_source_then_idle_starts() {
    let mut fork = Fork::new();
    fork.source_is("blocked", Some("dialog"));
    fork.until_screen("is blocked (dialog)");
    assert_eq!(fork.reports().len(), 1);

    let sessions = fork.fx.env.home().join(".pi/agent/sessions/g");
    fs::create_dir_all(&sessions).unwrap();
    let file = sessions.join("2026_lead1.jsonl");
    fs::write(&file, FINISHED_TURN).unwrap();
    fork.fx.backdate(&file, Duration::from_secs(3600));
    fork.w.steps(3);
    assert!(fork.pane.spawn.pis().is_empty(), "the source is blocked");

    fork.source_is("idle", None);
    let pane = &fork.pane;
    fork.w.until("pi", || !pane.spawn.pis().is_empty());
    let pi = fork.pane.spawn.pis()[0].args.clone();
    let at = pi.iter().position(|a| a == "--fork").expect("--fork");
    assert_eq!(pi[at + 1], file.display().to_string());
    // The report was one, and nothing more came with the start.
    assert_eq!(fork.reports().len(), 1);
    assert_eq!(fork.wakes().len(), 1);
}
