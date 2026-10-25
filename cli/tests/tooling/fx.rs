//! `common::fx::Fx` (cli/tests/common/fx.rs): `cpd` run in the test process, not as a child.

use crate::common::fx::Fx;
use crate::common::TestEnv;

use nix::errno::Errno;
use nix::sys::wait::{WaitPidFlag, waitpid};
use serde_json::json;

/// The newest `txns` row of the env's store: (`cwd`, `session`), what the product read from the process when it wrote.
fn last_txn(env: &TestEnv) -> (String, Option<String>) {
    env.read(|conn| conn.query_row("SELECT cwd, session FROM txns ORDER BY id DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?))))
}

#[test]
fn fx_cpd_returns_what_the_binary_returns_for_config_and_a_store_command() {
    let env = TestEnv::new();
    let argvs: [&[&str]; 3] = [&["config"], &["events", "list"], &["context", "pending", "--context", "nope"]];
    let binary: Vec<_> = argvs.iter().map(|argv| env.cpd(argv)).collect();
    let fx = Fx::with(env);
    for (argv, binary) in argvs.iter().zip(&binary) {
        let run = fx.cpd(argv);
        assert_eq!((run.exit, run.json()), (binary.exit, binary.json()), "{argv:?}");
    }
    assert_eq!(binary[2].exit, 4, "the refusal is a failure envelope: {:?}", binary[2]);
}

#[test]
fn fx_cpd_starts_no_child_process() {
    // `waitpid(-1)` says ECHILD only when this process has no child at all, running or not yet waited for: it needs the
    // process of its own that nextest gives each test (under `cargo test` another test's child would show here).
    let fx = Fx::new();
    fx.cpd(&["config"]).ok();
    fx.cpd(&["events", "list"]).ok();
    assert_eq!(waitpid(None, Some(WaitPidFlag::WNOHANG)), Err(Errno::ECHILD));
}

#[test]
fn cpd_with_sets_the_variables_for_the_call_and_restores_them() {
    let before: Vec<_> = std::env::vars_os().collect();
    let fx = Fx::new();
    let own: Vec<_> = std::env::vars_os().collect();
    assert_eq!(std::env::var_os("CPD_HOME"), Some(fx.env.cpd_home().into()), "Fx::new sets the env's variables");
    let created = fx.cpd_with(fx.env.root(), &["context", "new", "c1", "--title", "T", "--owner", "acme"], &[("CPD_SESSION", "s1"), ("PI_SESSION_ID", "s2")]);
    created.ok();
    assert_eq!(last_txn(&fx.env).1.as_deref(), Some("s1"), "the call saw the variable");
    assert_eq!(std::env::vars_os().collect::<Vec<_>>(), own, "the call restored the environment");
    fx.cpd(&["context", "new", "c2", "--title", "T", "--owner", "acme"]).ok();
    assert_eq!(last_txn(&fx.env).1, None, "the next call did not see it");
    drop(fx);
    assert_eq!(std::env::vars_os().collect::<Vec<_>>(), before, "dropping the Fx restored the process's environment");
}

#[test]
fn cpd_in_runs_in_the_directory_and_restores_the_current_directory() {
    let before = std::env::current_dir().unwrap();
    let fx = Fx::new();
    let own = std::env::current_dir().unwrap();
    assert_eq!(own.canonicalize().unwrap(), fx.env.root().canonicalize().unwrap(), "Fx::new works from the env's root");
    let elsewhere = fx.env.home();
    fx.cpd_in(&elsewhere, &["context", "new", "c1", "--title", "T", "--owner", "acme"]).ok();
    assert_eq!(std::path::Path::new(&last_txn(&fx.env).0).canonicalize().unwrap(), elsewhere.canonicalize().unwrap());
    assert_eq!(std::env::current_dir().unwrap(), own, "the call restored the directory");
    drop(fx);
    assert_eq!(std::env::current_dir().unwrap(), before, "dropping the Fx restored the process's directory");
}

#[test]
fn two_fx_do_not_overlap() {
    let first = Fx::new();
    assert!(Fx::try_new().is_none(), "a second Fx cannot start while the first lives");
    // A second Fx that waits for the first gets in once it is dropped.
    let second = std::thread::spawn(|| {
        let fx = Fx::new();
        fx.cpd(&["config"]).ok()
    });
    first.cpd(&["config"]).ok();
    drop(first);
    assert_eq!(second.join().unwrap()["ok"], json!(true));
    assert!(Fx::try_new().is_some(), "the lock is free again");
}
