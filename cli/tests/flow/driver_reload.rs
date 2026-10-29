//! The Attempt driver reloads after an install (hs-d33, hs-d38; contract §3.4 "Reload on install"): `cpd attempt run`, started by
//! `job dispatch` from `cpd`'s own executable, `exec`s the file renamed over that path at its next safe point, keeping its driver
//! lock on fd 3, and the Attempt still ends `merged`. In this process: the executable is a file of the test's, `Spawn::self_exe` of
//! the fake ports says so, and an install is a new file renamed over it. `exec(2)` is the `Spawn::exec` of
//! [`Fx::exec_replaces_the_process`]: it records the `Launch` on the `FakeSpawn`, keeps fd 3 and ends the run that made it. The
//! test then runs that launch, as the new program, on the same lock. The new cpd's `--version` is a scripted reply of `cpd`.
//!
//! What stays OS behaviour is not asserted on the kernel but on what ours does about it (`core/src/driver/lock.rs`'s own tests
//! hold `keep_across_exec` and `close_on_exec` to `FD_CLOEXEC`): fd 3 is not close-on-exec when `exec` is called, and is again
//! once the new driver has it.

use crate::common;

use std::fs::{self, File};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use common::fx::{Exec, Fx};
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::host::spawn::{Launch, Pid, Spawn};
use cpd_core::ids::{AttemptId, PlanId};
use serde_json::Value;

use super::driver::{JOB, PLAN};
use super::driver_open::{Live, live};

const ATTEMPT: &str = "api-a1";
const RELOAD: &str = "reload: new cpd installed, re-executing";
const SKIPPED: &str = "reload skipped";

/// `cpd --version` of a cpd that opens the store (contract §4.5).
const OPENS: &str = r#"{"ok":true,"version":"2","store":{"opens":true,"unknown":[],"pending":[]}}"#;
/// `cpd --version` of one that cannot: its verdict refuses (`store.opens` false).
const OLDER: &str = r#"{"ok":true,"version":"x","store":{"opens":false,"refusal":{"code":"store_too_new","message":"The database was migrated by a newer cpd."},"unknown":[],"pending":[]}}"#;

/// The reply of the installed `cpd` to `--version`, which an install changes.
struct Version {
    reply: Mutex<Reply>,
}

impl Handler for Version {
    fn handle(&self, _: &Call) -> Reply {
        self.reply.lock().unwrap().clone()
    }
}

/// Plan `pay` with Job `api` queued, in this process, and `cpd`'s own executable a file of the test's.
pub(super) struct Reload {
    pub(super) live: Live,
    installed: PathBuf,
    version: Arc<Version>,
}

pub(super) fn reload() -> Reload {
    let mut live = live("");
    live.fx.exec_replaces_the_process();
    let installed = live.fx.env.root().join("cpd");
    fs::write(&installed, "cpd 1\n").unwrap();
    live.fx.fakes.spawn.set_self_exe(&installed);
    let version = Arc::new(Version { reply: Mutex::new(Reply::ok(OPENS)) });
    let _ = (*live.fx.fakes.runner).clone().route("cpd", version.clone());
    Reload { live, installed, version }
}

impl Reload {
    pub(super) fn fx(&self) -> &Fx {
        &self.live.fx
    }

    /// `job dispatch api`: the driver's launch, recorded and not started.
    pub(super) fn dispatch(&self) -> Launch {
        let out = self.live.dispatch(JOB).ok();
        assert_eq!(out["attempt"], ATTEMPT);
        let launch = self.fx().take_launch(0);
        assert_eq!(launch.program, self.installed.display().to_string(), "the driver starts from cpd's own executable");
        launch
    }

    fn lock(&self) -> PathBuf {
        let home = cpd_core::home::CpdHome::resolve(Some(self.fx().env.cpd_home().into()), None).unwrap();
        home.attempt_lock(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(ATTEMPT).unwrap())
    }

    fn log(&self) -> String {
        let home = cpd_core::home::CpdHome::resolve(Some(self.fx().env.cpd_home().into()), None).unwrap();
        fs::read_to_string(home.driver_log(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(ATTEMPT).unwrap())).unwrap_or_default()
    }

    fn status(&self) -> String {
        self.live.attempt(ATTEMPT)["status"].as_str().unwrap().to_owned()
    }

    fn txns(&self) -> i64 {
        self.live.value("SELECT count(*) FROM txns")
    }

    /// A new install: a new file renamed over the executable, which answers `--version` with `reply`.
    fn install_as(&self, reply: &str) {
        *self.version.reply.lock().unwrap() = Reply::ok(reply);
        let new = self.installed.with_extension("new");
        fs::write(&new, "cpd 2\n").unwrap();
        fs::rename(&new, &self.installed).unwrap();
    }

    /// A new cpd that can open the store.
    pub(super) fn install(&self) {
        self.install_as(OPENS);
    }

    /// A cpd that cannot open the store installed over the driver.
    fn install_older(&self) {
        self.install_as(OLDER);
    }

    /// The `--version` questions asked of the installed cpd.
    fn versions_asked(&self) -> usize {
        self.fx().calls("cpd").iter().filter(|c| c.args == ["--version"]).count()
    }

    /// What the driver `exec`ed: every launch after the driver's own.
    pub(super) fn execs(&self) -> Vec<Launch> {
        self.fx().fakes.spawn.launches().split_off(1)
    }

    /// The driver lock is held, by someone other than this call's own descriptor.
    fn assert_lock_held(&self) {
        let file = File::options().read(true).write(true).open(self.lock()).unwrap();
        assert!(matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock)), "the driver lock is free: {}", self.log());
    }

    /// What the exec recorded is the same argv, of the new file, and the driver was alive and held its lock across it.
    pub(super) fn assert_exec_of_the_driver(&self, exec: &Exec, pid: Pid) {
        assert_eq!(self.execs(), [exec.launch.clone()]);
        assert_eq!(exec.launch.program, self.installed.display().to_string());
        assert_eq!(exec.launch.args, ["attempt", "run", ATTEMPT, "--plan", PLAN], "the same argv");
        assert_eq!(exec.fd3_cloexec, Some(false), "fd 3 is not close-on-exec at the exec: the lock stays held with no gap");
        assert!(self.fx().fakes.spawn.alive(pid), "the driver, one process throughout, is alive");
        self.assert_lock_held();
    }

    /// The Worker's pi submits: it commits `api.txt` in its worktree and runs `attempt submit`.
    pub(super) fn submit(&self) -> Value {
        let wt = self.live.worktree(ATTEMPT);
        self.live.commit(&wt, "api.txt", "api\n", "api");
        self.fx().cpd_with(&wt, &["attempt", "submit", ATTEMPT], &[("CPD_PLAN", PLAN), ("CPD_ATTEMPT", ATTEMPT)]).json()
    }

    /// The new program of an `exec`, run on the lock the old one kept; its log is the same file. `hook` is called after each nap.
    pub(super) fn run_new(&self, mut exec: Exec, hook: impl FnMut(&Fx, usize)) -> common::Run {
        let launch = Launch { stdout_to: self.fx().take_launch(0).stdout_to, ..exec.launch.clone() };
        let _fd3 = self.fx().hand_over(exec.fd3());
        let ran = self.fx().run_launch_with(&launch, hook);
        assert_eq!(ran.exit, 0, "{}", ran.stdout);
        ran
    }

    /// The Attempt ended `merged` on the Lead Branch, and no retry was queued.
    pub(super) fn assert_merged(&self, old_lead: &str) {
        let log = self.log();
        let last: Value = serde_json::from_str(log.lines().last().unwrap()).unwrap_or_else(|e| panic!("{e}: {log}"));
        assert_eq!(last["outcome"], "merged", "{log}");
        assert_eq!(last["cause"], "merged", "{log}");
        assert_ne!(self.live.lead_sha(), old_lead);
        assert_eq!(fs::read_to_string(self.live.lead.join("api.txt")).unwrap(), "api\n");
        assert_eq!(self.live.job_status("api"), "merged");
        assert_eq!(self.live.value::<i64>("SELECT count(*) FROM events WHERE kind = 'attempt-retry'"), 0);
    }
}

/// The first run of the driver, on the lock (fd 3), until it `exec`s; `hook` is called after each nap.
pub(super) fn run_until_exec(re: &Reload, launch: &Launch, hook: impl FnMut(&Fx, usize)) -> (Exec, common::fx::HandOver) {
    // As the starter leaves it: the lock taken on the file the driver is handed (a kernel lock on a file of ours, in the env's directory).
    let lock = File::open(re.lock()).unwrap();
    lock.try_lock().unwrap();
    let fd3 = re.fx().hand_over(lock);
    match re.fx().run_launch_until_exec(launch, hook) {
        Err(exec) => (exec, fd3),
        Ok(ran) => panic!("the driver ended without reloading: {ran:?}\n{}", re.log()),
    }
}

#[test]
fn a_driver_reloads_while_running_keeps_its_lock_and_the_attempt_merges() {
    let re = reload();
    let old_lead = re.live.lead_sha();
    let launch = re.dispatch();
    let pid = Pid(1000);
    let mut txns = 0;

    let (exec, _fd3) = run_until_exec(&re, &launch, |_, nap| {
        if nap == 1 {
            assert_eq!(re.status(), "running");
            txns = re.txns();
            re.install();
        }
    });

    re.assert_exec_of_the_driver(&exec, pid);
    assert_eq!(re.status(), "running");
    assert_eq!(re.txns(), txns, "the reload writes nothing");

    re.run_new(exec, |_, nap| {
        if nap == 1 {
            re.submit();
        }
    });

    re.assert_merged(&old_lead);
    let log = re.log();
    assert_eq!(log.matches(RELOAD).count(), 1, "{log}");
    assert_eq!(log.matches("A1: resuming at A4 after a reload").count(), 1, "{log}");
    assert_eq!(re.execs().len(), 1);
}

/// A driver whose Worker submitted meanwhile reloads at the safe point before A8's rebase: the new cpd resumes at A8, once.
#[test]
fn a_driver_reloads_at_the_merging_safe_point_and_completes_the_merge() {
    let re = reload();
    let old_lead = re.live.lead_sha();
    let launch = re.dispatch();

    // The Worker submits and the new cpd is installed before the driver's next look at the Attempt.
    let (exec, _fd3) = run_until_exec(&re, &launch, |_, nap| {
        if nap == 1 {
            re.submit();
            assert_eq!(re.status(), "merging");
            assert!(re.execs().is_empty() && !re.log().contains(RELOAD));
            re.install();
        }
    });

    re.assert_exec_of_the_driver(&exec, Pid(1000));
    re.run_new(exec, |_, nap| panic!("the merge takes no poll: nap {nap}"));

    re.assert_merged(&old_lead);
    let log = re.log();
    assert_eq!(log.matches(RELOAD).count(), 1, "{log}");
    assert_eq!(log.matches("A8: merging").count(), 1, "the merge starts once, after the reload: {log}");
    assert!(log.find(RELOAD) < log.find("A8: merging"), "{log}");
    assert_eq!(log.matches("A1: resuming").count(), 0, "{log}");
}

/// The 00:02Z incident: a cpd that cannot open the database (its verdict, contract §4.5) is
/// installed over a running driver. The driver does not `exec` it,
/// says so once, and the Attempt runs on; a cpd that can open the store, installed
/// next, is exec'd as before.
#[test]
fn a_driver_does_not_reload_into_a_cpd_older_than_the_store_and_reloads_into_one_that_can_open_it() {
    let re = reload();
    let old_lead = re.live.lead_sha();
    let launch = re.dispatch();

    let (exec, _fd3) = run_until_exec(&re, &launch, |_, nap| {
        if nap == 1 {
            re.install_older();
        }
        // The older cpd is neither asked again nor exec'd over eight polls of A4 (the 1st of them is the one that found it).
        if nap == 10 {
            let log = re.log();
            assert_eq!(log.matches(SKIPPED).count(), 1, "{log}");
            assert!(
                log.contains(&format!("reload skipped: {} cannot open the store: The database was migrated by a newer cpd.", re.installed.display())),
                "{log}"
            );
            assert!(!log.contains(RELOAD), "{log}");
            assert_eq!(re.versions_asked(), 1, "asked once, not at each poll");
            assert!(re.execs().is_empty(), "the older cpd was exec'd: {:?}", re.execs());
            assert_eq!(re.status(), "running");
            re.assert_lock_held();

            re.install();
        }
    });

    re.assert_exec_of_the_driver(&exec, Pid(1000));
    assert_eq!(re.versions_asked(), 2, "the cpd installed next is asked again");

    re.run_new(exec, |_, nap| {
        if nap == 1 {
            re.submit();
        }
    });

    re.assert_merged(&old_lead);
    let log = re.log();
    assert_eq!(log.matches(SKIPPED).count(), 1, "{log}");
    assert_eq!(log.matches(RELOAD).count(), 1, "{log}");
}

/// After a reload exec, fd 3 of the driver is the lock and nothing the new driver starts holds it (§3.4 step 4, "Reload on
/// install"): it is close-on-exec but for the `exec` itself, the one call that must keep it.
#[test]
fn after_a_reload_fd_3_is_the_lock_and_the_drivers_children_do_not_hold_it() {
    let re = reload();
    let launch = re.dispatch();
    let lock = fs::metadata(re.lock()).unwrap();
    let fd3 = |what: &str| {
        // SAFETY: only reads fd 3, which `ManuallyDrop` never closes.
        let file = std::mem::ManuallyDrop::new(unsafe { std::os::fd::FromRawFd::from_raw_fd(3) });
        let file: &File = &file;
        let meta = file.metadata().unwrap_or_else(|e| panic!("fd 3 {what}: {e}"));
        assert_eq!((meta.dev(), meta.ino()), (lock.dev(), lock.ino()), "fd 3 is not the lock {what}");
        let flags = nix::fcntl::fcntl(file, nix::fcntl::FcntlArg::F_GETFD).unwrap();
        nix::fcntl::FdFlag::from_bits_truncate(flags).contains(nix::fcntl::FdFlag::FD_CLOEXEC)
    };

    let (exec, _fd3) = run_until_exec(&re, &launch, |_, nap| {
        if nap == 1 {
            assert!(fd3("before the reload"), "a child of the driver would hold the lock: fd 3 is not close-on-exec");
            re.install();
        }
    });
    assert_eq!(exec.fd3_cloexec, Some(false), "the exec keeps fd 3");
    assert_eq!(re.execs().len(), 1);

    let mut seen = false;
    re.run_new(exec, |_, nap| {
        if nap == 1 {
            assert!(fd3("after the reload"), "a child of the new driver would hold the lock: fd 3 is not close-on-exec");
            seen = true;
            re.submit();
        }
    });
    assert!(seen, "the new driver ran");
    assert_ne!(File::open(re.lock()).map(|f| f.as_raw_fd()).ok(), Some(3));
}

/// engine-W6: the nudge baseline survives a reload. The Worker is nudged, the
/// driver reloads, the Worker ends another turn without submitting: `no_submit`,
/// not `timed_out` after the Attempt's whole active time.
#[test]
fn a_nudged_worker_that_idles_again_after_a_reload_ends_no_submit() {
    let re = reload();
    let launch = re.dispatch();
    let dir = re.fx().env.home().join(".pi/agent/sessions/w");
    let turn = r#"{"type":"message","timestamp":"2026-10-14T21:00:05.000Z","message":{"role":"assistant","usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{"total":0}},"stopReason":"stop"}}"#;
    let session = || re.live.value::<String>("SELECT worker_session FROM attempts WHERE plan = 'pay' AND id = 'api-a1'");
    // What pi does in its pane: `cpd session state <to>` with the Session in its environment.
    let state = |to: &str| {
        let id = session();
        let env = [("CPD_SESSION", id.as_str()), ("CPD_PLAN", PLAN), ("CPD_ATTEMPT", ATTEMPT)];
        re.fx().cpd_with(&re.live.worktree(ATTEMPT), &["session", "state", to], &env).ok();
    };
    let file = || dir.join(format!("2026_{}.jsonl", session()));
    let nudges = || re.live.value::<i64>("SELECT count(*) FROM events WHERE kind = 'attempt-nudge'");

    let (exec, _fd3) = run_until_exec(&re, &launch, |_, nap| match nap {
        // pi ends its first turn, idle.
        1 => {
            fs::create_dir_all(&dir).unwrap();
            fs::write(file(), format!("{}\n{turn}\n", r#"{"type":"session","timestamp":"2026-10-14T21:00:00.000Z"}"#)).unwrap();
            state("idle");
        }
        // The driver nudged it; the new cpd is installed.
        2 => {
            assert_eq!(nudges(), 1);
            re.install();
        }
        _ => {}
    });
    re.assert_exec_of_the_driver(&exec, Pid(1000));

    // pi works on the nudge and ends another turn, still without submitting.
    re.run_new(exec, |_, nap| {
        if nap == 1 {
            state("working");
            let mut text = fs::read_to_string(file()).unwrap();
            text += &format!("{turn}\n");
            fs::write(file(), text).unwrap();
            state("idle");
        }
    });

    let log = re.log();
    let last: Value = serde_json::from_str(log.lines().last().unwrap()).unwrap_or_else(|e| panic!("{e}: {log}"));
    assert_eq!((&last["outcome"], &last["cause"]), (&Value::from("failed"), &Value::from("no_submit")), "{log}");
    assert_eq!(log.matches(RELOAD).count(), 1, "{log}");
    assert_eq!(nudges(), 1);
}
