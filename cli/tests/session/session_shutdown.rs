//! The end of archive-on-quit (decision pro-d358, commands.md §10.8): a Lead or a Manager whose pi exits 0 in `session
//! run` is left `stopped`, never archived, and its Plan and Context stay; the wrapper offers the resume in place and
//! returns at the end of its input. In this process, on the fake tmux and virtual time (`common::wrapper`).

use crate::common;

use std::fs;

use common::fx::Fx;
use common::session::{setup, show};
use common::wrapper::{Ended, Pane, Wrapper, installed_cpd};

/// `session run <id>` in a new window of the cockpit, its first pi started, which then exits 0 (the User's `/quit`).
/// Returns the wrapper and its pane, with the prompt on the screen.
fn quit(fx: &Fx, id: &str, name: &str) -> (Wrapper, Pane) {
    setup(&fx.env);
    fx.cockpit();
    let dir = fx.env.home().join(".pi/agent/sessions/g");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(format!("2026_{id}.jsonl")),
        "{\"type\":\"session\"}\n",
    )
    .unwrap();
    let pane = Pane::new(fx, id, &installed_cpd(fx));
    fx.env.sql(&format!(
        "UPDATE sessions SET tmux_pane = '{}' WHERE id = '{id}'",
        pane.pane
    ));
    let mut w = pane.run(&["session", "run", id]);
    w.until("pi number 1", || !pane.spawn.pis().is_empty());
    let pi = *pane.spawn.pids().last().expect("a pi runs");
    fx.fakes.spawn.exit(pi, 0);
    let prompt = format!("pi exited. Press Enter to resume {name} here.");
    w.until(&format!("{prompt:?} on the screen"), || {
        pane.screen().contains(&prompt)
    });
    (w, pane)
}

fn assert_left_stopped(fx: &Fx, id: &str, mut w: Wrapper) {
    let row = show(fx, id);
    assert_eq!(row["state"], "stopped", "{row}");
    assert!(row["archived_at"].is_null(), "archived: {row}");
    for table in ["plans", "contexts"] {
        assert_eq!(
            fx.env.query::<i64>(&format!(
                "SELECT count(*) FROM {table} WHERE archived_at IS NOT NULL"
            )),
            0,
            "{table} archived"
        );
    }
    // No Enter typed: the end of input ends the wrapper.
    assert!(
        matches!(w.finish(), Ended::Returned { .. }),
        "{:?}",
        w.ended()
    );
}

#[test]
fn a_lead_whose_pi_exits_zero_is_left_stopped_and_not_archived() {
    let fx = Fx::with_ports();
    // `lead2`: the own Lead of Plan `other`, which has nothing running.
    let (w, _pane) = quit(&fx, "lead2", "Lead two");
    assert_left_stopped(&fx, "lead2", w);
}

#[test]
fn a_manager_whose_pi_exits_zero_is_left_stopped_and_not_archived() {
    let fx = Fx::with_ports();
    let (w, _pane) = quit(&fx, "manager1", "Manager");
    assert_left_stopped(&fx, "manager1", w);
}
