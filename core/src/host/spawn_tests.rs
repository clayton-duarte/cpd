//! `FakeSpawn` (docs/implementing.md, "Ports, Spawn and run_with"): the launch log, the pids, the signals, the exits
//! and the facts a test configures. It starts nothing: no process, no wait.

use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;

use super::spawn::{FakeSpawn, Launch, Pid, Signal, Spawn};

fn launch(program: &str) -> Launch {
    Launch {
        program: program.to_owned(),
        ..Launch::default()
    }
}

const SIGTERM: i32 = 15;
const SIGKILL: i32 = 9;

// ---- pids and the launch log ---------------------------------------------------------------------------------------

#[test]
fn a_fake_spawn_hands_out_increasing_pids() {
    let spawn = FakeSpawn::new();
    let first = spawn.spawn(&launch("a")).unwrap();
    let second = spawn.spawn(&launch("b")).unwrap();
    let third = spawn.spawn(&launch("c")).unwrap();
    assert!(
        first < second && second < third,
        "{first:?} {second:?} {third:?}"
    );
}

#[test]
fn a_fake_spawn_hands_out_a_pid_not_given_before() {
    let spawn = FakeSpawn::new();
    let pids: Vec<Pid> = (0..5).map(|_| spawn.spawn(&launch("x")).unwrap()).collect();
    let mut sorted = pids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 5);
}

#[test]
fn a_new_fake_spawn_has_no_launch_and_no_signal() {
    let spawn = FakeSpawn::new();
    assert!(spawn.launches().is_empty());
    assert!(spawn.signals().is_empty());
}

#[test]
fn launches_records_every_launch_in_order() {
    let spawn = FakeSpawn::new();
    spawn.spawn(&launch("first")).unwrap();
    spawn.spawn(&launch("second")).unwrap();
    let programs: Vec<String> = spawn.launches().into_iter().map(|l| l.program).collect();
    assert_eq!(programs, ["first", "second"]);
}

#[test]
fn launches_records_every_field_of_a_launch() {
    let spawn = FakeSpawn::new();
    let l = Launch {
        program: "/usr/local/bin/cpd".into(),
        args: vec!["attempt".into(), "run".into(), "a1".into()],
        env_set: vec![("CPD_ATTEMPT".into(), "a1".into())],
        env_remove: vec!["CPD_SESSION".into()],
        cwd: Some(PathBuf::from("/work/tree")),
        setsid: true,
        stdout_to: Some(PathBuf::from("/log/a1.log")),
        hand_over: Some(3),
        idle: true,
        foreground: true,
    };
    spawn.spawn(&l).unwrap();
    assert_eq!(spawn.launches(), vec![l]);
}

#[test]
fn stream_records_its_launch_and_hands_out_the_next_pid() {
    let spawn = FakeSpawn::new();
    let first = spawn.spawn(&launch("a")).unwrap();
    let (streamed, _lines) = spawn.stream(&launch("nvidia-smi")).unwrap();
    assert!(streamed > first);
    assert_eq!(
        spawn.launches().last().map(|l| l.program.as_str()),
        Some("nvidia-smi")
    );
}

// ---- alive and exit ------------------------------------------------------------------------------------------------

#[test]
fn a_spawned_pid_is_alive() {
    let spawn = FakeSpawn::new();
    let pid = spawn.spawn(&launch("driver")).unwrap();
    assert!(spawn.alive(pid));
}

#[test]
fn a_pid_is_alive_until_it_exits() {
    let spawn = FakeSpawn::new();
    let pid = spawn.spawn(&launch("driver")).unwrap();
    assert!(spawn.alive(pid));
    spawn.exit(pid, 0);
    assert!(!spawn.alive(pid));
}

#[test]
fn exit_of_one_pid_leaves_the_others_alive() {
    let spawn = FakeSpawn::new();
    let one = spawn.spawn(&launch("a")).unwrap();
    let two = spawn.spawn(&launch("b")).unwrap();
    spawn.exit(one, 1);
    assert!(!spawn.alive(one));
    assert!(spawn.alive(two));
}

#[test]
fn a_pid_never_handed_out_is_not_alive() {
    let spawn = FakeSpawn::new();
    let pid = spawn.spawn(&launch("a")).unwrap();
    assert!(!spawn.alive(Pid(pid.0 + 1000)));
}

// ---- try_wait ------------------------------------------------------------------------------------------------------

#[test]
fn try_wait_of_a_running_pid_is_none() {
    let spawn = FakeSpawn::new();
    let pid = spawn.spawn(&launch("driver")).unwrap();
    assert!(spawn.try_wait(pid).unwrap().is_none());
}

#[test]
fn try_wait_of_an_exited_pid_is_its_exit_code() {
    let spawn = FakeSpawn::new();
    let pid = spawn.spawn(&launch("driver")).unwrap();
    spawn.exit(pid, 3);
    let status = spawn
        .try_wait(pid)
        .unwrap()
        .expect("an exited pid has a status");
    assert_eq!(status.code(), Some(3));
}

#[test]
fn try_wait_of_a_pid_that_exited_with_zero_is_a_success() {
    let spawn = FakeSpawn::new();
    let pid = spawn.spawn(&launch("driver")).unwrap();
    spawn.exit(pid, 0);
    assert!(spawn.try_wait(pid).unwrap().expect("a status").success());
}

// ---- signal --------------------------------------------------------------------------------------------------------

#[test]
fn a_term_signal_marks_the_pid_exited_by_that_signal() {
    let spawn = FakeSpawn::new();
    let pid = spawn.spawn(&launch("driver")).unwrap();
    spawn.signal(pid, Signal::Term).unwrap();
    assert!(!spawn.alive(pid));
    let status = spawn
        .try_wait(pid)
        .unwrap()
        .expect("a signalled pid has a status");
    assert_eq!(status.signal(), Some(SIGTERM));
    assert_eq!(status.code(), None);
}

#[test]
fn a_kill_signal_marks_the_pid_exited_by_that_signal() {
    let spawn = FakeSpawn::new();
    let pid = spawn.spawn(&launch("driver")).unwrap();
    spawn.signal(pid, Signal::Kill).unwrap();
    assert!(!spawn.alive(pid));
    assert_eq!(
        spawn.try_wait(pid).unwrap().expect("a status").signal(),
        Some(SIGKILL)
    );
}

#[test]
fn a_signal_leaves_the_other_pids_alive() {
    let spawn = FakeSpawn::new();
    let one = spawn.spawn(&launch("a")).unwrap();
    let two = spawn.spawn(&launch("b")).unwrap();
    spawn.signal(one, Signal::Kill).unwrap();
    assert!(spawn.alive(two));
}

#[test]
fn signals_records_every_signal_in_order() {
    let spawn = FakeSpawn::new();
    let one = spawn.spawn(&launch("a")).unwrap();
    let two = spawn.spawn(&launch("b")).unwrap();
    spawn.signal(one, Signal::Term).unwrap();
    spawn.signal(two, Signal::Kill).unwrap();
    assert_eq!(
        spawn.signals(),
        vec![(one, Signal::Term), (two, Signal::Kill)]
    );
}

// ---- exec ----------------------------------------------------------------------------------------------------------

#[test]
fn exec_returns_an_error_whose_text_is_faked_exec() {
    let spawn = FakeSpawn::new();
    let err = spawn.exec(&launch("cpd"));
    assert_eq!(err.to_string(), "faked exec");
}

#[test]
fn exec_records_its_launch_and_starts_no_pid() {
    let spawn = FakeSpawn::new();
    let l = Launch {
        program: "/bin/cpd".into(),
        args: vec!["watch".into()],
        ..Launch::default()
    };
    let _ = spawn.exec(&l);
    assert_eq!(spawn.launches(), vec![l]);
    assert!(spawn.signals().is_empty());
}

// ---- self_exe, exe_of, cmdline -------------------------------------------------------------------------------------

#[test]
fn self_exe_is_the_configured_path() {
    let spawn = FakeSpawn::new();
    spawn.set_self_exe("/opt/cpd/bin/cpd");
    assert_eq!(spawn.self_exe().unwrap(), PathBuf::from("/opt/cpd/bin/cpd"));
}

#[test]
fn self_exe_with_none_configured_is_an_error() {
    assert!(FakeSpawn::new().self_exe().is_err());
}

#[test]
fn exe_of_is_what_the_test_configured_for_that_pid() {
    let spawn = FakeSpawn::new();
    let one = spawn.spawn(&launch("a")).unwrap();
    let two = spawn.spawn(&launch("b")).unwrap();
    spawn.set_exe_of(one, "/bin/one");
    assert_eq!(spawn.exe_of(one), Some(PathBuf::from("/bin/one")));
    assert_eq!(spawn.exe_of(two), None);
}

#[test]
fn cmdline_is_what_the_test_configured_for_that_pid() {
    let spawn = FakeSpawn::new();
    let one = spawn.spawn(&launch("a")).unwrap();
    let two = spawn.spawn(&launch("b")).unwrap();
    spawn.set_cmdline(one, "cpd attempt run a1");
    assert_eq!(spawn.cmdline(one).as_deref(), Some("cpd attempt run a1"));
    assert_eq!(spawn.cmdline(two), None);
}

#[test]
fn descendants_are_the_live_processes_below_the_root_at_any_depth_and_the_root_is_not_one() {
    let spawn = FakeSpawn::new();
    let (pane, other) = (Pid(10001), Pid(10002));
    let child = spawn.add_process(pane, "sh -c build");
    let grandchild = spawn.add_process(child, "sleep 60");
    let stranger = spawn.add_process(other, "sleep 61");

    assert_eq!(spawn.descendants(pane), [child, grandchild]);
    assert_eq!(spawn.descendants(other), [stranger]);
    assert_eq!(spawn.cmdline(grandchild).as_deref(), Some("sleep 60"));

    spawn.signal(child, Signal::Term).unwrap();
    assert_eq!(
        spawn.descendants(pane),
        [grandchild],
        "an exited process is no descendant"
    );
}
