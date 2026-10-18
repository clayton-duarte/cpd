//! The Attempt driver reloads after an install (hs-d33, hs-d38; contract §3.4
//! "Reload on install"): `cpd attempt run`, started by `job dispatch` from a
//! copy of the built binary, re-executes the file renamed over that copy at its
//! next safe point, keeping its driver lock on fd 3, and the Attempt still ends
//! `merged`. An install here is a new script renamed over the copy; it records
//! its pid and arguments and then becomes the built `cpd`.
//!
//! The tests run one at a time: writing an executable while another thread
//! forks can make its `exec` fail with ETXTBSY.

mod common;

use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use common::{CPD, TestEnv};
use cpd_core::error::CpdError;
use cpd_core::git::{Setup, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::plan::{jobs, locked_fingerprint};
use cpd_core::store::{Store, WriteTxn};
use serde_json::Value;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

const PLAN: &str = "pay";
const ATTEMPT: &str = "api-a1";
const RELOAD: &str = "reload: new cpd installed, re-executing";
const SKIPPED: &str = "reload skipped";

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

fn executable(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct Fx {
    env: TestEnv,
    repo: PathBuf,
    lead: PathBuf,
    /// The copy of `cpd` that dispatches, and so runs, the driver.
    installed: PathBuf,
}

/// Plan `pay` with Job `api` queued, on a private tmux server, and a fake pi
/// Worker that waits for the file `go`, then commits `api.txt` and submits.
fn fx() -> Fx {
    let env = TestEnv::new();
    let repo = env.git_repo("pay").canonicalize().unwrap();
    for (key, value) in [("user.name", "cpd test"), ("user.email", "test@cpd.invalid"), ("commit.gpgsign", "false")] {
        env.git(&repo, &["config", key, value]);
    }
    let setup = Setup { timeout: Duration::from_secs(30), log: env.root().join("lead.log") };
    let lead = create_lead_worktree(&repo, &PlanId::parse(PLAN).unwrap(), "main", &setup).unwrap().path;
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    fs::create_dir_all(env.root().join("install")).unwrap();
    fs::write(env.root().join("install/current-version"), "1.0.0\n").unwrap();
    let scope = env.root().join("install/releases/1.0.0/node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    for args in [&["new-session", "-d", "-s", "cpd-work", "-n", "watch", "sleep", "600"][..], &["set", "-g", "remain-on-exit", "on"]] {
        assert!(env.tmux(args).status.success(), "tmux {args:?}");
    }
    let installed = env.root().join("cpd");
    fs::copy(CPD, &installed).unwrap();
    let go = env.root().join("go");
    env.fake_pi(&format!(
        "CPD='{CPD}'\n\
         until \"$CPD\" attempt show \"$CPD_ATTEMPT\" | grep -q '\"status\":\"running\"'; do sleep 0.1; done\n\
         until [ -e '{}' ]; do sleep 0.1; done\n\
         echo api > api.txt\ngit add -A && git commit -qm api\n\
         \"$CPD\" attempt submit \"$CPD_ATTEMPT\" > ../submit.json 2>&1\n\
         sleep 120",
        go.display()
    ));
    let fx = Fx { env, repo, lead, installed };
    let catalog: String = [("O", "p/opus", "[0,1,2,3,4,5]"), ("S", "p/sonnet", "[1,2,3,4,5]"), ("H", "p/haiku", "[0,1,2,3]")]
        .iter()
        .map(|(letter, model, efforts)| {
            format!("INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{model}', '{efforts}', 't');")
        })
        .collect();
    fx.fixture(&format!(
        "{catalog}
         INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'main', 'dispatch', 'cpd/pay', '{lead}', cpd_txn(), 't', 't');
         INSERT INTO jobs (plan, key, pr, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
           VALUES ('pay', 'api', 'repo', 'Title of api', 'S2', 'Do api.', 'Because.', '[\"api.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't');",
        repo = fx.repo.display(),
        lead = fx.lead.display()
    ));
    let plan = PlanId::parse(PLAN).unwrap();
    let all = fx.store().read(|conn| jobs::load(conn, &plan)).unwrap();
    let job = all.iter().find(|j| j.key == "api").unwrap();
    fx.fixture(&format!(
        "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = 'pay' AND key = 'api';",
        locked_fingerprint(job)
    ));
    fx
}

impl Fx {
    fn home(&self) -> CpdHome {
        CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap()
    }

    fn store(&self) -> Store {
        Store::open(&self.home(), 5000).unwrap()
    }

    fn fixture(&self, sql: &str) {
        self.store().write("fixture", |tx| exec(tx, sql)).unwrap();
    }

    fn query<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        self.store().read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
    }

    fn path(&self, name: &str) -> PathBuf {
        self.env.root().join(name)
    }

    /// `job dispatch api` by the copy: its driver's executable is the copy. Returns the driver's pid.
    fn dispatch(&self) -> i64 {
        let out = self.env.command(&self.installed).args(["job", "dispatch", "api", "--plan", PLAN]).output().unwrap();
        let json: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {out:?}"));
        assert_eq!(json["ok"], true, "{json}");
        json["pid"].as_i64().unwrap()
    }

    fn status(&self) -> String {
        self.env.cpd(&["attempt", "show", ATTEMPT, "--plan", PLAN]).ok()["attempt"]["status"].as_str().unwrap().to_owned()
    }

    fn log(&self) -> String {
        fs::read_to_string(self.env.cpd_home().join(format!("run/pay/logs/attempt-{ATTEMPT}.log"))).unwrap_or_default()
    }

    /// A new install: a new script renamed over the copy. It answers `--version`
    /// as the built `cpd` does (the driver asks before it reloads), and records
    /// every other call.
    fn install(&self) {
        self.install_script(&format!("exec '{CPD}' --version"));
    }

    /// An older `cpd` installed over the driver: its `--version` says it supports
    /// store schema 5 while the database is at 6.
    fn install_older(&self) {
        self.install_script("echo '{\"ok\":true,\"version\":\"x\",\"schema\":5,\"store_schema\":6}'; exit 0");
    }

    fn install_script(&self, on_version: &str) {
        let new = self.installed.with_extension("new");
        executable(
            &new,
            &format!(
                "#!/bin/sh\ncase \"$1\" in --version) {on_version};; esac\necho \"$$ $*\" >> '{}'\nexec '{CPD}' \"$@\"\n",
                self.path("installed.runs").display()
            ),
        );
        fs::rename(&new, &self.installed).unwrap();
    }

    /// What the installed script recorded: `<pid> <arguments>` per run.
    fn runs(&self) -> Vec<String> {
        fs::read_to_string(self.path("installed.runs")).map(|t| t.lines().map(str::to_owned).collect()).unwrap_or_default()
    }

    /// The driver lock is held, and by a process that is still alive.
    fn assert_lock_held_by(&self, pid: i64) {
        let lock = self.home().attempt_lock(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(ATTEMPT).unwrap());
        let file = File::options().read(true).write(true).open(lock).unwrap();
        assert!(matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock)), "the driver lock is free: {}", self.log());
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).expect("the driver is alive");
    }

    /// Lets the Worker submit and waits for the Attempt to end and its driver to exit.
    fn finish(&self, pid: i64) {
        wait_until("the Attempt to end", || self.status() == "ended");
        let pid = nix::unistd::Pid::from_raw(pid as i32);
        wait_until("the driver to exit", || nix::sys::signal::kill(pid, None).is_err());
    }

    fn assert_merged(&self, old_lead: &str) {
        let log = self.log();
        let last: Value = serde_json::from_str(log.lines().last().unwrap()).unwrap_or_else(|e| panic!("{e}: {log}"));
        assert_eq!(last["outcome"], "merged", "{log}");
        assert_eq!(last["cause"], "merged", "{log}");
        let lead = self.env.git(&self.repo, &["rev-parse", "refs/heads/cpd/pay"]);
        assert_ne!(lead, old_lead);
        assert_eq!(fs::read_to_string(self.lead.join("api.txt")).unwrap(), "api\n");
        let status: String = self.query("SELECT status FROM jobs WHERE plan = 'pay' AND key = 'api'");
        assert_eq!(status, "merged");
        assert_eq!(self.query::<i64>("SELECT count(*) FROM events WHERE kind = 'attempt-retry'"), 0);
    }
}

#[test]
fn a_driver_reloads_while_running_keeps_its_lock_and_the_attempt_merges() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    let fx = fx();
    let old_lead = fx.env.git(&fx.repo, &["rev-parse", "refs/heads/cpd/pay"]);
    let pid = fx.dispatch();
    wait_until("the Attempt to run", || fx.status() == "running");
    let txns = fx.query::<i64>("SELECT count(*) FROM txns");

    fx.install();
    wait_until("the driver to reload", || !fx.runs().is_empty());

    assert_eq!(fx.runs(), [format!("{pid} attempt run {ATTEMPT} --plan {PLAN}")], "the same process, the same argv");
    fx.assert_lock_held_by(pid);
    assert_eq!(fx.status(), "running");
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM txns"), txns, "the reload writes nothing");

    fs::write(fx.path("go"), "").unwrap();
    fx.finish(pid);
    fx.assert_merged(&old_lead);
    let log = fx.log();
    assert_eq!(log.matches(RELOAD).count(), 1, "{log}");
    assert_eq!(log.matches("A1: resuming at A4 after a reload").count(), 1, "{log}");
    assert_eq!(fx.runs().len(), 1);
}

#[test]
fn a_driver_reloads_at_the_merging_safe_point_and_completes_the_merge() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    let fx = fx();
    let old_lead = fx.env.git(&fx.repo, &["rev-parse", "refs/heads/cpd/pay"]);
    // A `tmux` that holds the driver's `list-panes` (the start of its A4 tick)
    // while `hold` exists: the Attempt can be made `merging`, and the new cpd
    // installed, before the driver's next tick looks at the Attempt.
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|dir| dir.join("tmux"))
        .find(|p| p.is_file())
        .expect("tmux on PATH");
    executable(
        &fx.env.bin().join("tmux"),
        &format!(
            "#!/bin/sh\n\
             case \" $* \" in *\" list-panes \"*)\n\
               if [ -e '{hold}' ]; then : > '{blocked}'; while [ -e '{hold}' ]; do sleep 0.05; done; fi;;\n\
             esac\n\
             exec '{real}' \"$@\"\n",
            hold = fx.path("hold").display(),
            blocked = fx.path("blocked").display(),
            real = real.display()
        ),
    );
    let pid = fx.dispatch();
    wait_until("the Attempt to run", || fx.status() == "running");

    fs::write(fx.path("hold"), "").unwrap();
    wait_until("the driver to be held", || fx.path("blocked").exists());
    fs::write(fx.path("go"), "").unwrap();
    wait_until("the Worker to submit", || fx.status() == "merging");
    assert!(fx.runs().is_empty() && !fx.log().contains(RELOAD));
    fx.install();
    fs::remove_file(fx.path("hold")).unwrap();
    wait_until("the driver to reload", || !fx.runs().is_empty());

    assert_eq!(fx.runs(), [format!("{pid} attempt run {ATTEMPT} --plan {PLAN}")], "the same process, the same argv");
    fx.finish(pid);
    fx.assert_merged(&old_lead);
    let log = fx.log();
    assert_eq!(log.matches(RELOAD).count(), 1, "{log}");
    assert_eq!(log.matches("A8: merging").count(), 1, "the merge starts once, after the reload: {log}");
    assert!(log.find(RELOAD) < log.find("A8: merging"), "{log}");
    assert_eq!(log.matches("A1: resuming").count(), 0, "{log}");
}

/// The 00:02Z incident: a cpd that supports an older store schema than the
/// database's is installed over a running driver. The driver does not `exec` it,
/// says so once, and the Attempt runs on; a cpd that can open the store, installed
/// next, is exec'd as before.
#[test]
fn a_driver_does_not_reload_into_a_cpd_older_than_the_store_and_reloads_into_one_that_can_open_it() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    let fx = fx();
    let old_lead = fx.env.git(&fx.repo, &["rev-parse", "refs/heads/cpd/pay"]);
    let pid = fx.dispatch();
    wait_until("the Attempt to run", || fx.status() == "running");

    fx.install_older();
    wait_until("the driver to refuse the reload", || fx.log().contains(SKIPPED));
    // Several ticks: it is not asked again, nor exec'd.
    std::thread::sleep(Duration::from_secs(4));
    let log = fx.log();
    assert_eq!(log.matches(SKIPPED).count(), 1, "{log}");
    assert!(log.contains(&format!("reload skipped: {} supports store schema 5, the database is at 6", fx.installed.display())), "{log}");
    assert!(!log.contains(RELOAD), "{log}");
    assert!(fx.runs().is_empty(), "the older cpd was exec'd: {:?}", fx.runs());
    assert_eq!(fx.status(), "running");
    fx.assert_lock_held_by(pid);

    fx.install();
    wait_until("the driver to reload", || !fx.runs().is_empty());
    assert_eq!(fx.runs(), [format!("{pid} attempt run {ATTEMPT} --plan {PLAN}")], "the same process, the same argv");
    fx.assert_lock_held_by(pid);

    fs::write(fx.path("go"), "").unwrap();
    fx.finish(pid);
    fx.assert_merged(&old_lead);
    let log = fx.log();
    assert_eq!(log.matches(SKIPPED).count(), 1, "{log}");
    assert_eq!(log.matches(RELOAD).count(), 1, "{log}");
}

/// After a reload exec, fd 3 of the driver is the lock and nothing the new driver
/// starts holds it (§3.4 step 4, "Reload on install"). A `tmux` stand-in, which the
/// driver starts at each A4 tick, lists its own open files once `probe` exists.
#[cfg(target_os = "linux")]
#[test]
fn after_a_reload_fd_3_is_the_lock_and_the_drivers_children_do_not_hold_it() {
    let _serial = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
    let fx = fx();
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|dir| dir.join("tmux"))
        .find(|p| p.is_file())
        .expect("tmux on PATH");
    executable(
        &fx.env.bin().join("tmux"),
        &format!(
            "#!/bin/sh\n\
             case \" $* \" in *\" list-panes \"*)\n\
               if [ -e '{probe}' ] && [ ! -e '{out}' ]; then\n\
                 for fd in /proc/$$/fd/*; do readlink \"$fd\"; done > '{tmp}'\n\
                 mv '{tmp}' '{out}'\n\
               fi;;\n\
             esac\n\
             exec '{real}' \"$@\"\n",
            probe = fx.path("probe").display(),
            out = fx.path("probe.out").display(),
            tmp = fx.path("probe.tmp").display(),
            real = real.display()
        ),
    );
    let pid = fx.dispatch();
    wait_until("the Attempt to run", || fx.status() == "running");
    let lock = fx.home().attempt_lock(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(ATTEMPT).unwrap()).canonicalize().unwrap();
    let fd3 = || fs::read_link(format!("/proc/{pid}/fd/3")).ok();
    assert_eq!(fd3().as_deref(), Some(lock.as_path()), "before the reload");

    fx.install();
    wait_until("the driver to reload", || !fx.runs().is_empty());
    assert_eq!(fx.runs(), [format!("{pid} attempt run {ATTEMPT} --plan {PLAN}")], "the same process");
    assert_eq!(fd3().as_deref(), Some(lock.as_path()), "after the reload fd 3 is the lock");

    fs::write(fx.path("probe"), "").unwrap();
    wait_until("a child of the new driver to list its files", || fx.path("probe.out").exists());
    let open = fs::read_to_string(fx.path("probe.out")).unwrap();
    assert!(!open.trim().is_empty(), "the child listed no files");
    let lock = lock.display().to_string();
    assert!(open.lines().all(|l| l.trim_end_matches(" (deleted)") != lock), "a child of the driver holds the lock:\n{open}");
    assert_eq!(fd3().as_deref(), Some(std::path::Path::new(&lock)), "the driver still holds it");

    fs::write(fx.path("go"), "").unwrap();
    fx.finish(pid);
}
