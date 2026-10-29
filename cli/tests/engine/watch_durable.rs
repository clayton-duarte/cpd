//! The engine's E2 for a `durable` Worker (contract §3.7 E2, §3.4 "The Worker runner and its seam", pro-d164, pro-d165,
//! pro-d180): a durable Worker has no pane by design, so E2 leaves the Session of a live one alone and stops every other
//! paneless Session; and E2 ends the runner of a dead driver's Attempt with SIGKILL, by the stored `worker_pid`, only when the
//! pid's `cmdline` holds the runner file and this Attempt's spec path (the pid may have been reused). All in-process on the
//! fakes: no node, no pi, no tmux server, no sleep.

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::fx::Fx;
use cpd_core::home::CpdHome;
use cpd_core::host::spawn::{Pid, Signal, Spawn};
use cpd_core::ids::{AttemptId, PlanId};

/// The adapter directory the engine's config names (`pi_adapter`, which must hold a `package.json`) in the temp home:
/// the runner file is `<adapter>/durable-worker/main.ts`.
fn adapter(fx: &Fx) -> PathBuf {
    fx.env.root().join("pi")
}

fn runner_file(fx: &Fx) -> String {
    adapter(fx).join("durable-worker/main.ts").display().to_string()
}

/// The engine of Plan `pay` with `retry_cap = 0` (a dead driver's Job fails: E3 starts nothing) and the adapter named.
fn engine() -> Fx {
    let fx = Fx::with_ports();
    fs::create_dir_all(adapter(&fx).join("durable-worker")).unwrap();
    fs::write(adapter(&fx).join("package.json"), "{}").unwrap();
    fs::write(adapter(&fx).join("durable-worker/main.ts"), "").unwrap();
    fx.engine_config(&format!("retry_cap = 0\npi_adapter = \"{}\"", adapter(&fx).display()));
    fx.env.sql("INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');");
    fx.engine_routes();
    fx.engine_plan();
    fx
}

fn ago_in(fx: &Fx, minutes: i64) -> String {
    fx.stamp_ago(minutes * 60_000)
}

/// `run/pay/specs/<attempt>.md`, where the engine's own home keeps the Attempt's Spec.
fn spec_path(fx: &Fx, attempt: &str) -> String {
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    home.worker_spec(&PlanId::parse("pay").unwrap(), &AttemptId::parse(attempt).unwrap()).display().to_string()
}

/// What `ps` shows of a durable runner started for `attempt` (contract §3.4, Start): `<node> <runner file> --model <pattern> --spec <path>`.
fn runner_cmdline(fx: &Fx, attempt: &str) -> String {
    format!("/opt/node/bin/node {} --model p/sonnet --spec {}", runner_file(fx), spec_path(fx, attempt))
}

/// Attempt `<key>-a1` running (driver pid 4242, its lock free until [`Fx::live_driver`] holds it), runner `durable` and its
/// `worker_pid` `pid`.
fn add_durable(fx: &Fx, key: &str, pid: Option<u32>) {
    fx.add_running(key, "running", &ago_in(fx, 1));
    let pid = pid.map_or("NULL".to_owned(), |p| p.to_string());
    fx.env.sql(&format!("UPDATE attempts SET runner = 'durable', worker_pid = {pid} WHERE plan = 'pay' AND id = '{key}-a1';"));
}

/// A Session of the Worker of Attempt `<key>-a1`, in `state`, with no pane.
fn add_worker_session(fx: &Fx, id: &str, key: &str, state: &str) {
    fx.env.sql(&format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at)
           VALUES ('{id}', 'worker', 'ctx', 'pay', '{key}-a1', 'Worker {key}', '/', 'S2', '{state}', 't', 't');"
    ));
}

fn session(fx: &Fx, id: &str) -> (String, Option<String>) {
    fx.env.read(|c| Ok(c.query_row("SELECT state, tmux_pane FROM sessions WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))?))
}

fn attempt(fx: &Fx, id: &str) -> (String, Option<String>, Option<String>) {
    fx.env.read(|c| {
        Ok(c.query_row("SELECT status, outcome, cause FROM attempts WHERE plan = 'pay' AND id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?)
    })
}

fn events(fx: &Fx, kind: &str) -> i64 {
    fx.env.query(&format!("SELECT count(*) FROM events WHERE kind = '{kind}'"))
}

#[test]
fn e2_leaves_the_session_of_a_live_durable_attempt_running_without_a_pane() {
    let fx = engine();
    let pid = fx.fakes.spawn.add_process(Pid(1), &runner_cmdline(&fx, "api-a1"));
    add_durable(&fx, "api", Some(pid.0));
    add_worker_session(&fx, "w-api", "api", "working");
    let _driver = fx.live_driver("api-a1");

    fx.watch_ticks(2).unwrap();

    // R9: no pane is by design; E2 neither stops the Session nor moves it, and the live runner is not signalled.
    assert_eq!(session(&fx, "w-api"), ("working".to_owned(), None));
    assert_eq!((events(&fx, "session-state"), events(&fx, "session-moved")), (0, 0));
    assert_eq!(attempt(&fx, "api-a1").0, "running");
    assert_eq!(fx.fakes.spawn.signals(), []);
    assert!(fx.fakes.spawn.alive(pid));
    assert_eq!(events(&fx, "error"), 0);
}

#[test]
fn e2_still_stops_a_paneless_session_whose_attempt_is_pi_or_ended() {
    let fx = engine();
    // A pi Attempt with a live driver: its Worker's pane is gone, so E2 stops the Session as before.
    fx.add_running("api", "running", &ago_in(&fx, 1));
    add_worker_session(&fx, "w-pi", "api", "working");
    let _driver = fx.live_driver("api-a1");
    // A durable Attempt whose driver died: E2 ends it, and its Session, no longer a live Worker's, is stopped.
    add_durable(&fx, "web", None);
    add_worker_session(&fx, "w-ended", "web", "idle");
    // A Session with no Attempt (an archived Lead, so E2 does not restart it) and no pane.
    fx.env.sql(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, archived_at)
           VALUES ('lead2', 'lead', 'ctx', 'pay', 'Lead two', '/', 'O3', 'working', 't', 't', 't');",
    );

    fx.watch_ticks(2).unwrap();

    assert_eq!(attempt(&fx, "web-a1").0, "ended");
    assert_eq!(attempt(&fx, "api-a1").0, "running");
    for id in ["w-pi", "w-ended", "lead2"] {
        assert_eq!(session(&fx, id), ("stopped".to_owned(), None), "{id}");
    }
    assert_eq!(events(&fx, "session-state"), 3);
}

#[test]
fn e2_ends_a_durable_attempt_whose_driver_died_and_kills_its_runner_by_pid() {
    let fx = engine();
    let pid = fx.fakes.spawn.add_process(Pid(1), &runner_cmdline(&fx, "api-a1"));
    add_durable(&fx, "api", Some(pid.0));

    fx.watch_ticks(2).unwrap();

    assert_eq!(attempt(&fx, "api-a1"), ("ended".to_owned(), Some("failed".to_owned()), Some("crashed".to_owned())));
    // R8: SIGKILL at once (the runner's commits survive a kill, the last report is on disk): no SIGTERM, no wait.
    assert_eq!(fx.fakes.spawn.signals(), [(pid, Signal::Kill)]);
    assert!(!fx.fakes.spawn.alive(pid));
    assert_eq!(events(&fx, "error"), 0);
}

#[test]
fn e2_does_not_signal_a_pid_that_is_not_the_attempts_runner() {
    let fx = engine();
    // `api`: the runner, as the control: it is killed. The others hold a pid that is not their Attempt's runner.
    let runner = fx.fakes.spawn.add_process(Pid(1), &runner_cmdline(&fx, "api-a1"));
    add_durable(&fx, "api", Some(runner.0));
    // `web`: the pid was reused by another program.
    let reused = fx.fakes.spawn.add_process(Pid(1), "/usr/bin/vim notes.txt");
    add_durable(&fx, "web", Some(reused.0));
    // `db`: a durable runner, but of another Attempt (its spec path is `api-a1`'s).
    let other_attempt = fx.fakes.spawn.add_process(Pid(1), &runner_cmdline(&fx, "api-a1"));
    add_durable(&fx, "db", Some(other_attempt.0));
    // `ui`: this Attempt's spec path, but another runner file.
    let other_runner = fx.fakes.spawn.add_process(Pid(1), &format!("/opt/node/bin/node /opt/other/main.ts --spec {}", spec_path(&fx, "ui-a1")));
    add_durable(&fx, "ui", Some(other_runner.0));
    // `gone`: the pid is not running at all (it has no cmdline).
    add_durable(&fx, "gone", Some(99_999));

    fx.watch_ticks(2).unwrap();

    for key in ["api", "web", "db", "ui", "gone"] {
        assert_eq!(attempt(&fx, &format!("{key}-a1")).0, "ended", "{key}: the dead driver's Attempt ends whatever its pid is");
    }
    assert_eq!(fx.fakes.spawn.signals(), [(runner, Signal::Kill)]);
    for pid in [reused, other_attempt, other_runner] {
        assert!(fx.fakes.spawn.alive(pid), "pid {} must be left running", pid.0);
    }
}
