//! The end of archive-on-quit (decision pro-d358, commands.md §10.8): a Lead or a Manager whose pi exits 0 in `session
//! run` is left `stopped`, never archived, and its Plan and Context stay; the wrapper offers the resume in place and
//! returns at the end of its input. In this process, on the fake tmux and virtual time (`common::wrapper`).

use crate::common;

use std::fs;
use std::path::PathBuf;

use cpd_core::ids::SessionId;

use common::fx::Fx;
use common::session::{events, home, setup, show};
use common::wrapper::{Ended, Pane, Wrapper, installed_cpd};

/// The marker file the User's `/quit` leaves for `id` (pro-d361); the mock pi never types, so a test writes it.
fn marker(fx: &Fx, id: &str) -> PathBuf {
    home(&fx.env).quit_marker(&SessionId::parse(id).unwrap())
}

fn write_marker(fx: &Fx, id: &str) {
    let path = marker(fx, id);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "").unwrap();
}

/// `session run <id>` in a new window of the cockpit, its first pi started and running. A `stale` marker is already
/// there before `session run` starts.
fn running(fx: &Fx, id: &str, stale: bool) -> (Wrapper, Pane) {
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
    if stale {
        write_marker(fx, id);
    }
    let mut w = pane.run(&["session", "run", id]);
    w.until("pi number 1", || !pane.spawn.pis().is_empty());
    (w, pane)
}

/// pi of `pane` exits with `code`, with the User's marker written first when `quit_marker`.
fn pi_exits(fx: &Fx, id: &str, pane: &Pane, quit_marker: bool, code: i32) {
    if quit_marker {
        write_marker(fx, id);
    }
    let pi = *pane.spawn.pids().last().expect("a pi runs");
    fx.fakes.spawn.exit(pi, code);
}

/// `running`, then pi exits 0 (the User's `/quit` with no marker). Returns the wrapper and its pane, with the prompt on
/// the screen.
fn quit(fx: &Fx, id: &str, name: &str) -> (Wrapper, Pane) {
    quit_with(fx, id, name, false, 0)
}

fn quit_with(fx: &Fx, id: &str, name: &str, quit_marker: bool, code: i32) -> (Wrapper, Pane) {
    let (mut w, pane) = running(fx, id, false);
    pi_exits(fx, id, &pane, quit_marker, code);
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
    assert_nothing_archived(fx);
    // No Enter typed: the end of input ends the wrapper.
    assert!(
        matches!(w.finish(), Ended::Returned { .. }),
        "{:?}",
        w.ended()
    );
}

fn assert_nothing_archived(fx: &Fx) {
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM sessions WHERE archived_at IS NOT NULL"),
        0,
        "a Session archived"
    );
    for kind in ["context-archived", "plan-archived", "session-archived"] {
        assert!(events(fx, kind).is_empty(), "{kind} was written");
    }
    for table in ["plans", "contexts"] {
        assert_eq!(
            fx.env.query::<i64>(&format!(
                "SELECT count(*) FROM {table} WHERE archived_at IS NOT NULL"
            )),
            0,
            "{table} archived"
        );
    }
}

#[test]
fn a_lead_whose_pi_exits_zero_is_left_stopped_and_not_archived() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    // `lead2`: the own Lead of Plan `other`, which has nothing running.
    let (w, _pane) = quit(&fx, "lead2", "Lead two");
    assert_left_stopped(&fx, "lead2", w);
}

/// One Context with a Manager and a Plan with its own Lead: quitting either archives nothing.
#[test]
fn a_manager_whose_pi_exits_zero_is_left_stopped_and_not_archived() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    let (w, _manager_pane) = quit(&fx, "manager1", "Manager");
    assert_left_stopped(&fx, "manager1", w);
    let (w, _lead_pane) = quit(&fx, "lead2", "Lead two");
    assert_left_stopped(&fx, "lead2", w);
}

/// Plan `other` finished: nothing live, so its Lead may be archived.
fn finish_plan_other(fx: &Fx) {
    done_plan_other(fx);
    fx.env
        .sql("UPDATE workflows SET status = 'cancelled' WHERE plan = 'other'");
}

/// Plan `other` in phase `done`, by the legal phases.
fn done_plan_other(fx: &Fx) {
    for phase in ["plan", "dispatch", "deploy", "done"] {
        fx.env.sql(&format!(
            "UPDATE plans SET phase = '{phase}', locked_at = {}, done_at = {} WHERE id = 'other'",
            if phase == "plan" { "NULL" } else { "'t'" },
            if phase == "done" { "'t'" } else { "NULL" }
        ));
    }
}

fn archived_count(fx: &Fx, table: &str) -> i64 {
    fx.env.query::<i64>(&format!(
        "SELECT count(*) FROM {table} WHERE archived_at IS NOT NULL"
    ))
}

#[test]
fn a_lead_whose_pi_exits_zero_after_the_quit_marker_is_archived() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    finish_plan_other(&fx);
    let (mut w, pane) = running(&fx, "lead2", false);
    pi_exits(&fx, "lead2", &pane, true, 0);
    w.finish();
    assert_eq!(archived_count(&fx, "plans"), 1, "the Plan is archived");
    assert!(
        !show(&fx, "lead2")["archived_at"].is_null(),
        "the Lead is archived"
    );
    assert_eq!(events(&fx, "plan-archived").len(), 1);
    assert!(!marker(&fx, "lead2").exists(), "the marker is removed");
}

#[test]
fn a_blocked_lead_whose_pi_exits_zero_after_the_quit_marker_is_not_archived() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    // The Plan is done but its Workflow still waits: a blocker.
    done_plan_other(&fx);
    let (w, pane) = quit_with(&fx, "lead2", "Lead two", true, 0);
    let shown = pane.screen();
    assert!(shown.contains("archive_blocked"), "{shown}");
    assert!(!marker(&fx, "lead2").exists(), "the marker is removed");
    assert_left_stopped(&fx, "lead2", w);
}

#[test]
fn a_lead_whose_pi_exits_nonzero_with_the_quit_marker_is_not_archived() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    finish_plan_other(&fx);
    let (w, _pane) = quit_with(&fx, "lead2", "Lead two", true, 1);
    assert!(!marker(&fx, "lead2").exists(), "the marker is consumed");
    assert_left_stopped(&fx, "lead2", w);
}

#[test]
fn a_manager_whose_pi_exits_zero_with_the_quit_marker_is_not_archived() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    let (w, _pane) = quit_with(&fx, "manager1", "Manager", true, 0);
    assert!(!marker(&fx, "manager1").exists(), "the marker is consumed");
    assert_left_stopped(&fx, "manager1", w);
}

/// A marker left by an earlier pi is removed before the next one starts, so a later signal exit never archives.
#[test]
fn a_stale_quit_marker_is_removed_before_pi_launches() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    finish_plan_other(&fx);
    let (mut w, pane) = running(&fx, "lead2", true);
    assert!(
        !marker(&fx, "lead2").exists(),
        "the stale marker was still there when pi launched"
    );
    // The signal exit: no marker of this run.
    pi_exits(&fx, "lead2", &pane, false, 0);
    let prompt = "pi exited. Press Enter to resume Lead two here.";
    w.until("the prompt", || pane.screen().contains(prompt));
    assert_left_stopped(&fx, "lead2", w);
}

#[test]
fn the_pi_launch_env_carries_the_quit_marker() {
    let fx = Fx::with_ports();
    setup(&fx.env);
    fx.cockpit();
    let (_w, pane) = running(&fx, "lead2", false);
    let launch = pane.spawn.pis().remove(0);
    assert_eq!(
        launch
            .env_set
            .iter()
            .find(|(k, _)| k == "CPD_QUIT_MARKER")
            .map(|(_, v)| v.as_str()),
        marker(&fx, "lead2").to_str(),
        "{:?}",
        launch.env_set
    );
}
