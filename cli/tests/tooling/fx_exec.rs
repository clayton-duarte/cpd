//! `Fx`'s `exec` and fd 3 over fake ports (`common/fx.rs`): `exec_replaces_the_process` ends the run that `exec`s as `exec(2)` ends a
//! process, keeping fd 3; `run_launch` gives a launch with `stdout_to` the log a process would have; a `HandOver` closes only the
//! file it handed over. Every test here is in-process: none starts a process, sleeps or reads the wall clock.

use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
use std::panic::{AssertUnwindSafe, catch_unwind};

use cpd_core::host::spawn::Launch;
use serde_json::json;

use crate::common::fx::{Exec, Fx};

fn fx() -> Fx {
    let mut fx = Fx::with_ports();
    fx.env.write_config("version = 1\nmachine = \"m1\"\n");
    fx.exec_replaces_the_process();
    fx
}

fn launch(args: &[&str]) -> Launch {
    Launch {
        program: "/opt/cpd/bin/cpd".to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        ..Launch::default()
    }
}

/// What `exec` of the fake ports unwinds with.
fn exec_of(fx: &Fx, l: &Launch) -> Exec {
    match catch_unwind(AssertUnwindSafe(|| fx.fakes.ports.spawn.exec(l))) {
        Ok(failed) => panic!("exec returned: {failed}"),
        Err(payload) => *payload.downcast::<Exec>().expect("the unwind of an exec"),
    }
}

/// The identity of the file on descriptor `fd`, or `None` when it is closed.
fn identity(fd: i32) -> Option<(u64, u64)> {
    // SAFETY: only reads the descriptor, which `ManuallyDrop` never closes.
    let file = std::mem::ManuallyDrop::new(unsafe { File::from_raw_fd(fd) });
    file.metadata().ok().map(|m| (m.dev(), m.ino()))
}

fn fd3() -> Option<(u64, u64)> {
    identity(3)
}

#[test]
fn exec_records_its_launch_and_unwinds_with_it() {
    let fx = fx();
    let new = launch(&["attempt", "run", "a1", "--plan", "pay"]);

    let exec = exec_of(&fx, &new);

    assert_eq!(exec.launch, new);
    assert_eq!(fx.fakes.spawn.launches(), [new]);
    assert_eq!(exec.fd3_cloexec, None, "fd 3 was closed");
}

#[test]
fn exec_keeps_fd_3_open_on_the_same_file_and_reports_whether_it_was_close_on_exec() {
    let fx = fx();
    let path = fx.env.root().join("lock");
    std::fs::write(&path, "").unwrap();
    let on_disk = std::fs::metadata(&path).unwrap();
    let handed = fx.hand_over(File::open(&path).unwrap());
    // `hand_over` clears close-on-exec, as a spawn leaves it.
    let mut exec = exec_of(&fx, &launch(&["attempt", "run"]));
    assert_eq!(exec.fd3_cloexec, Some(false));

    // The driver closing fd 3 on its way out does not close what `exec` kept: the file, and a lock on it, live on.
    drop(handed);
    assert_eq!(fd3(), None);
    let kept = exec.fd3();
    assert_eq!(
        (
            kept.metadata().unwrap().dev(),
            kept.metadata().unwrap().ino()
        ),
        (on_disk.dev(), on_disk.ino())
    );
    assert_ne!(kept.as_raw_fd(), 3);
}

#[test]
fn a_hand_over_closes_fd_3_only_when_it_is_still_its_file() {
    let fx = fx();
    let (one, other) = (fx.env.root().join("one"), fx.env.root().join("other"));
    std::fs::write(&one, "").unwrap();
    std::fs::write(&other, "").unwrap();
    let handed = fx.hand_over(File::open(&one).unwrap());
    // The driver closed it and another file took the number.
    // SAFETY: closes fd 3, which `handed` made.
    unsafe { nix::libc::close(3) };
    let taken = File::open(&other).unwrap();
    assert_eq!(taken.as_raw_fd(), 3);

    drop(handed);

    assert_eq!(
        fd3(),
        Some((
            taken.metadata().unwrap().dev(),
            taken.metadata().unwrap().ino()
        )),
        "the other file is still open"
    );
}

#[test]
fn run_launch_until_exec_returns_the_run_of_a_launch_that_does_not_exec() {
    let fx = fx();

    let run = fx
        .run_launch_until_exec(&launch(&["events", "list"]), |_, _| {})
        .ok()
        .expect("no exec");

    assert_eq!(
        (run.exit, run.json()),
        (0, json!({"ok": true, "events": [], "next": 0}))
    );
}

#[test]
fn a_launch_with_stdout_to_has_its_result_as_the_last_line_of_that_log_and_stderr_is_put_back() {
    let fx = fx();
    let log = fx.env.root().join("logs/attempt.log");
    let before = identity(2);
    let launch = Launch {
        stdout_to: Some(log.clone()),
        ..launch(&["events", "list"])
    };

    let run = fx.run_launch(&launch);
    let again = fx.run_launch(&launch);

    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        format!("{}{}", run.stdout, again.stdout),
        "appended, as a process appends"
    );
    assert_eq!(identity(2), before);
}
