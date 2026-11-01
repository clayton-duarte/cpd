//! `run_with` runs the whole command on the ports it is given, also the areas that read them through `Ports::current()`
//! (docs/implementing.md, "Ports, Spawn and run_with"): `secret list` runs the 1Password CLI and `host repair` runs
//! `git`, and both must reach the `FakeRunner` of the test, not a real process.
//!
//! The commands read the process's environment (`CPD_HOME`, `HOME`, `PATH`, `CPD_SESSION`): a test takes `LOCK`, sets them to
//! a temporary home and `Drop` puts them back (nextest gives a test a process of its own; under `cargo test` the lock
//! keeps two of these apart). While red, the real ports run the 1Password CLI (not on a test machine: `NotFound`, at
//! once) and `git` in a directory that does not exist (it fails at once): nothing real is needed or changed.
//! `PATH` is the empty temporary home for the same reason: a machine that has the real `op` must never prompt.

use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard, PoisonError};

use cpd_core::home::CpdHome;
use cpd_core::host::host_socket::Reach;
use cpd_core::host::runner::{Call, Reply};
use cpd_core::output::Output;
use cpd_core::ports::Fakes;
use cpd_core::store::Store;

use super::run_with;

/// Held by every [`Home`] from its creation to its drop.
static LOCK: Mutex<()> = Mutex::new(());

/// The variables a command reads, as they were before the test.
const VARS: [&str; 5] = ["CPD_HOME", "HOME", "PATH", "CPD_SESSION", "CPD_ATTEMPT"];

/// A temporary `$CPD_HOME` (a config and a store) that is also `$HOME`, the process environment pointing at it.
struct Home {
    dir: tempfile::TempDir,
    saved: Vec<(&'static str, Option<OsString>)>,
    _lock: MutexGuard<'static, ()>,
}

impl Home {
    /// `CPD_SESSION` is `session` when given, else unset; `CPD_ATTEMPT` is unset.
    fn enter(session: Option<&str>) -> Home {
        let lock = LOCK.lock().unwrap_or_else(PoisonError::into_inner);
        let dir = tempfile::tempdir().expect("a temporary home");
        let saved = VARS.iter().map(|name| (*name, std::env::var_os(name))).collect();
        let home = Home { dir, saved, _lock: lock };
        std::fs::write(home.dir.path().join("config.toml"), "version = 1\nmachine = \"m1\"\n").expect("write the config");
        // SAFETY: `LOCK` serializes these tests, and nextest gives each a process of its own.
        unsafe {
            std::env::set_var("CPD_HOME", home.dir.path());
            std::env::set_var("HOME", home.dir.path());
            // No program is found by name: a real `op` or `git` on the machine is never started.
            std::env::set_var("PATH", home.dir.path());
            std::env::remove_var("CPD_ATTEMPT");
            match session {
                Some(id) => std::env::set_var("CPD_SESSION", id),
                None => std::env::remove_var("CPD_SESSION"),
            }
        }
        home
    }

    /// A Manager Session `mgr` in the store, so `secret` knows who is calling.
    fn with_manager(self) -> Home {
        let home = CpdHome::resolve(Some(self.dir.path().as_os_str().to_owned()), None).expect("the home");
        let mut store = Store::open(&home, 5000).expect("open the store");
        store
            .write("seed", |tx| {
                tx.execute_batch(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                     INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
                       VALUES ('mgr', 'manager', 'ctx', 'Manager', '/', 'O3', 'working', 't', 't');",
                )?;
                Ok(())
            })
            .expect("seed the Manager");
        self
    }

    /// A durable Manager Session `mgr` in the store, so `session open` has a durable Session to resolve.
    fn with_durable_manager(self) -> Home {
        let home = CpdHome::resolve(Some(self.dir.path().as_os_str().to_owned()), None).expect("the home");
        let mut store = Store::open(&home, 5000).expect("open the store");
        store
            .write("seed", |tx| {
                tx.execute_batch(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                     INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at, runner)
                       VALUES ('mgr', 'manager', 'ctx', 'Manager', '/', 'O3', 'stopped', 't', 't', 'durable');",
                )?;
                Ok(())
            })
            .expect("seed the durable Manager");
        self
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        // SAFETY: as in `enter`.
        unsafe {
            for (name, value) in &self.saved {
                match value {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }
}

fn argv(words: &[&str]) -> Vec<OsString> {
    std::iter::once("cpd").chain(words.iter().copied()).map(OsString::from).collect()
}

/// The calls of `program` (by file name) that `fakes` recorded.
fn calls(fakes: &Fakes, program: &str) -> Vec<Call> {
    fakes.runner.calls().into_iter().filter(|call| call.program.rsplit('/').next() == Some(program)).collect()
}

#[test]
fn secret_list_runs_the_password_cli_on_the_ports_it_is_given() {
    let _home = Home::enter(Some("mgr")).with_manager();
    let fakes = Fakes::new();
    let _ = (*fakes.runner).clone().on("op", |_| true, Reply::ok(r#"[{"id":"i1","title":"db"}]"#));

    let out = run_with(&fakes.ports, argv(&["secret", "list", "--vault", "cpd"]));

    let seen = calls(&fakes, "op");
    assert_eq!(seen.len(), 1, "one call reached the FakeRunner: {:?}", fakes.runner.calls());
    assert_eq!(&seen[0].args[..2], ["item", "list"], "{:?}", seen[0].args);
    assert!(seen[0].args.windows(2).any(|pair| pair == ["--vault", "cpd"]), "{:?}", seen[0].args);
    let Ok(Output::Success(fields)) = out else { panic!("secret list succeeds on the scripted reply: {out:?}") };
    assert_eq!(fields["items"][0]["title"], "db");
    assert!(fakes.runner.unscripted().is_empty(), "{:?}", fakes.runner.unscripted());
}

#[test]
fn host_repair_runs_git_on_the_ports_it_is_given() {
    let _home = Home::enter(None);
    let fakes = Fakes::new();
    let _ = (*fakes.runner).clone().on("git", |_| true, Reply::ok("0123456789abcdef\n"));

    let out = run_with(&fakes.ports, argv(&["host", "repair"]));

    let gits = calls(&fakes, "git");
    assert!(!gits.is_empty(), "git reached the FakeRunner: {:?}", fakes.runner.calls());
    let args = &gits[0].args;
    assert!(args.ends_with(&["rev-parse".to_owned(), "HEAD".to_owned()]), "the first git call of a repair: {args:?}");
    assert!(args.iter().any(|arg| arg.ends_with("projects/cpd")), "in the checkout: {args:?}");
    let Ok(Output::Success(fields)) = out else { panic!("host repair succeeds on the scripted git: {out:?}") };
    assert_eq!(fields["head"], "0123456789abcdef");
    assert!(fakes.runner.unscripted().is_empty(), "{:?}", fakes.runner.unscripted());
}

#[test]
fn session_open_waits_for_the_socket() {
    let _home = Home::enter(Some("mgr")).with_durable_manager();
    let fakes = Fakes::new();
    // The socket is absent on the first probe and accepts on the second (contract §6.19). Nothing real opens.
    fakes.host_socket.script(vec![Reach::Absent, Reach::Accepted]);

    let out = run_with(&fakes.ports, argv(&["session", "open", "mgr"]));

    let Ok(Output::Success(fields)) = out else {
        panic!("session open of a durable Session waits for the socket and returns: {out:?}")
    };
    assert_eq!(fields["session"], "mgr");
    assert_eq!(fields["pane"], serde_json::Value::Null, "a durable Session never enters a pane: {fields:?}");
    let panes: Vec<_> = fakes.runner.calls().into_iter().filter(|call| call.args.iter().any(|a| a == "new-window")).collect();
    assert!(panes.is_empty(), "no pane is opened: {panes:?}");
    assert_eq!(fakes.clock.naps().len(), 1, "one nap between the absent and the accepting probe");
    let paths = fakes.host_socket.paths();
    assert_eq!(paths.len(), 2, "probed until it accepted: {paths:?}");
    assert!(paths[0].ends_with("run/host/mgr.sock"), "{paths:?}");
}
