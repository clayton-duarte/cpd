//! `common::fx::Fx` (cli/tests/common/fx.rs): `cpd` run in the test process, not as a child.

use crate::common::TestEnv;
use crate::common::fx::Fx;

use nix::errno::Errno;
use nix::sys::wait::{WaitPidFlag, waitpid};
use serde_json::json;
use std::os::fd::AsFd;

const CONFIG: &str = "version = 1\nmachine = \"m1\"\n";

/// An `Fx` whose `$CPD_HOME` has the config every command but `config` needs.
fn fx() -> Fx {
    let fx = Fx::new();
    fx.env.write_config(CONFIG);
    fx
}

/// The process's environment, by name.
fn vars() -> std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString> {
    std::env::vars_os().collect()
}

/// The names whose value differs between two environments (never the values: they may be secrets).
fn changed(
    a: &std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>,
    b: &std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>,
) -> Vec<std::ffi::OsString> {
    a.keys()
        .chain(b.keys())
        .filter(|k| a.get(*k) != b.get(*k))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// The newest `txns` row of the env's store: (`cwd`, `session`), what the product read from the process when it wrote.
fn last_txn(env: &TestEnv) -> (String, Option<String>) {
    env.read(|conn| {
        conn.query_row(
            "SELECT cwd, session FROM txns ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    })
}

#[test]
fn fx_cpd_returns_what_the_binary_returns_for_config_and_a_store_command() {
    let env = TestEnv::new();
    env.write_config(CONFIG);
    let argvs: [&[&str]; 3] = [
        &["config"],
        &["events", "list"],
        &["context", "pending", "--context", "nope"],
    ];
    let binary: Vec<_> = argvs.iter().map(|argv| env.cpd(argv)).collect();
    let fx = Fx::with(env);
    for (argv, binary) in argvs.iter().zip(&binary) {
        let run = fx.cpd(argv);
        assert_eq!(
            (run.exit, run.json()),
            (binary.exit, binary.json()),
            "{argv:?}"
        );
    }
    assert_eq!(
        binary[2].exit, 4,
        "the refusal is a failure envelope: {:?}",
        binary[2]
    );
}

/// Set by the handler of SIGCHLD, which the kernel sends this process whenever a child of it ends.
static A_CHILD_ENDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

extern "C" fn child_ended(_: i32) {
    A_CHILD_ENDED.store(true, std::sync::atomic::Ordering::SeqCst);
}

#[test]
fn fx_cpd_starts_no_child_process() {
    // A child that ends raises SIGCHLD, and `waitpid(-1)` says ECHILD only when the process has no child left: between
    // them, no child lived while the commands ran. Both say so about the whole process: they need the process of its own
    // that nextest gives each test (under a plain `cargo test` another test's child would show here).
    use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};
    use std::sync::atomic::Ordering;
    let fx = fx();
    let action = SigAction::new(
        SigHandler::Handler(child_ended),
        SaFlags::empty(),
        SigSet::empty(),
    );
    // SAFETY: the handler only stores into an atomic.
    unsafe { sigaction(Signal::SIGCHLD, &action) }.unwrap();
    fx.cpd(&["config"]).ok();
    fx.cpd(&["events", "list"]).ok();
    assert!(
        !A_CHILD_ENDED.load(Ordering::SeqCst),
        "a child process ended while the commands ran"
    );
    assert_eq!(
        waitpid(None, Some(WaitPidFlag::WNOHANG)),
        Err(Errno::ECHILD)
    );
}

#[test]
fn cpd_with_sets_the_variables_for_the_call_and_restores_them() {
    let before = vars();
    let fx = fx();
    let own = vars();
    assert_eq!(
        std::env::var_os("CPD_HOME"),
        Some(fx.env.cpd_home().into()),
        "Fx::new sets the env's variables"
    );
    let created = fx.cpd_with(
        fx.env.root(),
        &["context", "new", "c1", "--title", "T", "--owner", "acme"],
        &[("CPD_SESSION", "s1"), ("PI_SESSION_ID", "s2")],
    );
    created.ok();
    assert_eq!(
        last_txn(&fx.env).1.as_deref(),
        Some("s1"),
        "the call saw the variable"
    );
    assert_eq!(
        changed(&vars(), &own),
        Vec::<std::ffi::OsString>::new(),
        "the call restored the environment"
    );
    fx.cpd(&["context", "new", "c2", "--title", "T", "--owner", "acme"])
        .ok();
    assert_eq!(last_txn(&fx.env).1, None, "the next call did not see it");
    drop(fx);
    assert_eq!(
        changed(&vars(), &before),
        Vec::<std::ffi::OsString>::new(),
        "dropping the Fx restored the process's environment"
    );
}

#[test]
fn cpd_in_runs_in_the_directory_and_restores_the_current_directory() {
    let before = std::env::current_dir().unwrap();
    let fx = fx();
    let own = std::env::current_dir().unwrap();
    assert_eq!(
        own.canonicalize().unwrap(),
        fx.env.root().canonicalize().unwrap(),
        "Fx::new works from the env's root"
    );
    let elsewhere = fx.env.home();
    fx.cpd_in(
        &elsewhere,
        &["context", "new", "c1", "--title", "T", "--owner", "acme"],
    )
    .ok();
    assert_eq!(
        std::path::Path::new(&last_txn(&fx.env).0)
            .canonicalize()
            .unwrap(),
        elsewhere.canonicalize().unwrap()
    );
    assert_eq!(
        std::env::current_dir().unwrap(),
        own,
        "the call restored the directory"
    );
    drop(fx);
    assert_eq!(
        std::env::current_dir().unwrap(),
        before,
        "dropping the Fx restored the process's directory"
    );
}

#[test]
fn two_fx_do_not_overlap() {
    let first = fx();
    assert!(
        Fx::try_new().is_none(),
        "a second Fx cannot start while the first lives"
    );
    // A second Fx that waits for the first gets in once it is dropped.
    let second = std::thread::spawn(|| {
        let fx = fx();
        fx.cpd(&["config"]).ok()
    });
    first.cpd(&["config"]).ok();
    drop(first);
    assert_eq!(second.join().unwrap()["ok"], json!(true));
    assert!(Fx::try_new().is_some(), "the lock is free again");
}

#[test]
fn a_command_that_owns_its_stdout_is_refused_loudly_and_the_process_is_restored() {
    let fx = fx();
    fx.cpd(&["context", "new", "c1", "--title", "T", "--owner", "acme"])
        .ok();
    let (before, cwd) = (vars(), std::env::current_dir().unwrap());
    // `--markdown` prints with `print!` on the process's stdout (Output::Owned), not on the writer of `main_with`.
    let owned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        fx.cpd_with(
            &fx.env.home(),
            &["context", "pending", "--context", "c1", "--markdown"],
            &[("CPD_SESSION", "s1")],
        )
    }));
    assert!(owned.is_err(), "Fx::cpd has no stdout to return for it");
    assert_eq!(changed(&vars(), &before), Vec::<std::ffi::OsString>::new());
    assert_eq!(std::env::current_dir().unwrap(), cwd);
}

#[test]
fn export_sets_the_variable_for_the_fx_and_dropping_it_puts_the_environment_back() {
    let before = vars();
    let fx = fx();
    fx.export("FX_SET_VAR_TEST", "kept");
    assert_eq!(std::env::var("FX_SET_VAR_TEST").as_deref(), Ok("kept"));
    drop(fx);
    assert_eq!(changed(&vars(), &before), Vec::<std::ffi::OsString>::new());
}

/// What stdin and stdout are: the device and inode each points at.
fn std_files() -> [(u64, u64); 2] {
    let id = |fd: std::os::fd::BorrowedFd| {
        let stat = nix::sys::stat::fstat(fd).unwrap();
        (stat.st_dev as u64, stat.st_ino as u64)
    };
    [id(std::io::stdin().as_fd()), id(std::io::stdout().as_fd())]
}

#[test]
fn cpd_without_a_terminal_refuses_a_view_and_puts_stdin_and_stdout_back() {
    let fx = fx();
    let before = std_files();
    let run = fx.cpd_without_a_terminal(&["tui", "view", "legend"]);
    assert_eq!(run.exit, 3, "{run:?}");
    assert_eq!(run.error()["code"], "not_a_terminal");
    assert_eq!(std_files(), before, "stdin and stdout point where they did");
}
