//! `Ports::real`, `Fakes` and `Clock::utc` (docs/implementing.md, "Ports, Spawn and run_with").
//!
//! Which implementation a `Ports` holds is read through the `kind()` method this Job added to the `Clock`, `Runner`
//! and `Spawn` traits (default `"other"`; `SystemClock`, `SystemRunner` and `ProcessSpawn` answer their own name).
//! The one wall-clock read of these tests is `the_system_clock_utc_is_within_a_minute_of_now_utc`: it is what
//! `SystemClock::utc` is for.

use std::time::Duration;

use time::OffsetDateTime;

use crate::clock::{Clock, ManualClock, SystemClock};
use crate::host::runner::{Call, Reply};
use crate::host::spawn::{Launch, Spawn};
use crate::ports::{Fakes, Ports};

const SECS: fn(u64) -> Duration = Duration::from_secs;

fn call(program: &str, deadline: Duration) -> Call {
    Call {
        tool: program.to_owned(),
        program: program.to_owned(),
        args: Vec::new(),
        envs: Vec::new(),
        cwd: None,
        deadline,
        stdin: None,
        stderr_to: None,
    }
}

// ---- Ports::real ---------------------------------------------------------------------------------------------------

#[test]
fn real_ports_hold_the_system_clock() {
    assert_eq!(Ports::real().clock.kind(), "SystemClock");
}

#[test]
fn real_ports_hold_the_system_runner() {
    assert_eq!(Ports::real().runner.kind(), "SystemRunner");
}

#[test]
fn real_ports_hold_the_process_spawn() {
    assert_eq!(Ports::real().spawn.kind(), "ProcessSpawn");
}

// ---- Fakes ---------------------------------------------------------------------------------------------------------

#[test]
fn fakes_hold_no_real_seam() {
    let fakes = Fakes::new();
    assert_ne!(fakes.ports.clock.kind(), "SystemClock");
    assert_ne!(fakes.ports.runner.kind(), "SystemRunner");
    assert_ne!(fakes.ports.spawn.kind(), "ProcessSpawn");
}

#[test]
fn fakes_share_one_clock_between_the_ports_and_the_typed_handle() {
    let fakes = Fakes::new();
    let before = fakes.ports.clock.now();
    fakes.clock.advance(SECS(7));
    assert_eq!(fakes.ports.clock.now() - before, SECS(7));
    fakes.ports.clock.sleep(SECS(3));
    assert_eq!(fakes.clock.elapsed(), SECS(10));
    assert_eq!(fakes.clock.naps(), vec![SECS(3)]);
}

#[test]
fn a_fake_runner_reply_that_takes_time_moves_the_clock_of_the_ports() {
    let fakes = Fakes::new();
    // A clone of the runner shares its script: this scripts the one inside `fakes.ports`.
    let _ = (*fakes.runner)
        .clone()
        .on("gh", |_| true, Reply::ok("done").takes(SECS(2)));
    let before = fakes.ports.clock.now();
    let out = fakes
        .ports
        .runner
        .run(&call("gh", SECS(30)))
        .expect("the scripted reply");
    assert_eq!(out.stdout, b"done");
    assert_eq!(fakes.clock.elapsed(), SECS(2));
    assert_eq!(fakes.ports.clock.now() - before, SECS(2));
}

#[test]
fn the_runner_of_the_ports_is_the_typed_runner() {
    let fakes = Fakes::new();
    fakes
        .ports
        .runner
        .run(&call("tmux", SECS(1)))
        .expect_err("no route for tmux");
    assert_eq!(fakes.runner.calls().len(), 1);
    assert_eq!(fakes.runner.calls()[0].program, "tmux");
}

#[test]
fn the_spawn_of_the_ports_is_the_typed_spawn() {
    let fakes = Fakes::new();
    let launch = Launch {
        program: "cpd".into(),
        args: vec!["watch".into()],
        ..Launch::default()
    };
    let pid = fakes
        .ports
        .spawn
        .spawn(&launch)
        .expect("the fake starts nothing and cannot fail");
    assert_eq!(fakes.spawn.launches(), vec![launch]);
    assert!(fakes.spawn.alive(pid));
}

#[test]
fn two_fakes_share_nothing() {
    let one = Fakes::new();
    let two = Fakes::new();
    one.clock.advance(SECS(5));
    one.spawn.spawn(&Launch::default()).unwrap();
    assert_eq!(two.clock.elapsed(), Duration::ZERO);
    assert!(two.spawn.launches().is_empty());
}

// ---- Clock::utc ----------------------------------------------------------------------------------------------------

#[test]
fn a_manual_clock_utc_is_the_same_fixed_epoch_for_every_new_clock() {
    assert_eq!(ManualClock::new().utc(), ManualClock::new().utc());
}

#[test]
fn a_manual_clock_utc_does_not_move_by_itself() {
    let clock = ManualClock::new();
    assert_eq!(clock.utc(), clock.utc());
}

#[test]
fn a_manual_clock_utc_is_not_the_wall_clock() {
    let wall = OffsetDateTime::now_utc();
    let utc = ManualClock::new().utc();
    assert!(
        (wall - utc).abs() > time::Duration::days(1),
        "a fixed epoch, not now: {utc}"
    );
}

#[test]
fn a_manual_clock_advance_moves_utc_by_the_time_advanced() {
    let clock = ManualClock::new();
    let epoch = clock.utc();
    clock.advance(SECS(90));
    assert_eq!(clock.utc() - epoch, time::Duration::seconds(90));
}

#[test]
fn a_manual_clock_sleep_moves_utc_by_the_nap() {
    let clock = ManualClock::new();
    let epoch = clock.utc();
    clock.sleep(Duration::from_millis(1500));
    assert_eq!(clock.utc() - epoch, time::Duration::milliseconds(1500));
}

#[test]
fn a_manual_clock_utc_is_the_epoch_plus_elapsed() {
    let clock = ManualClock::new();
    let epoch = clock.utc();
    clock.advance(SECS(10));
    clock.sleep(SECS(5));
    assert_eq!(clock.utc(), epoch + clock.elapsed());
}

#[test]
fn a_manual_clock_clone_shares_utc() {
    let clock = ManualClock::new();
    let other = clock.clone();
    other.advance(SECS(4));
    assert_eq!(clock.utc(), other.utc());
    assert_eq!(
        clock.utc() - ManualClock::new().utc(),
        time::Duration::seconds(4)
    );
}

#[test]
fn the_system_clock_utc_is_within_a_minute_of_now_utc() {
    let utc = SystemClock.utc();
    let wall = OffsetDateTime::now_utc();
    assert!(
        (wall - utc).abs() < time::Duration::minutes(1),
        "{utc} against {wall}"
    );
}
