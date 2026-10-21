//! Driver and Worker priority (contract, "Priority (hs-d528)"; Linux only): a dispatched Attempt's driver and its
//! Worker's pi run at nice 19 (I/O class idle), and the caller keeps its own nice. The fixture is a trimmed copy of
//! `driver.rs`'s (its items are private to that module).

#![cfg(target_os = "linux")]
#![allow(dead_code)]

use crate::common;

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use common::{CPD, Run, TestEnv};
use cpd_core::error::CpdError;
use cpd_core::git::{Setup, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use cpd_core::plan::{jobs, locked_fingerprint};
use cpd_core::store::{Store, WriteTxn};
use serde_json::Value;

const PLAN: &str = "pay";
const JOB: &str = "api";
const PR_TITLE: &str = "Charge the card at checkout";
const ATTEMPT: &str = "api-a1";

const DRIVER_ENV: [(&str, &str); 2] = [("CPD_TEST_DRIVER_POLL", "0.05"), ("CPD_TEST_KILL_PAUSE", "0.05")];
const EFFORTS: [(&str, &str, &str); 3] =
    [("O", "p/opus", "[0,1,2,3,4,5]"), ("S", "p/sonnet", "[1,2,3,4,5]"), ("H", "p/haiku", "[0,1,2,3]")];

struct Fx {
    env: TestEnv,
    repo: PathBuf,
    lead: PathBuf,
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

/// A repo `pay` with the Lead worktree of Plan `pay` (phase `dispatch`), a
/// config on this env's tmux server, a fake pi install whose Tier catalog is
/// seeded in the cache, and Job `api` (`queued`, Tier S2, Locked).
fn fx() -> Fx {
    fx_with("")
}

fn fx_with(config_extra: &str) -> Fx {
    let env = TestEnv::new();
    let repo = env.git_repo("pay").canonicalize().unwrap();
    // git runs in this process too, whose environment is not the TestEnv's.
    for (key, value) in [("user.name", "cpd test"), ("user.email", "test@cpd.invalid"), ("commit.gpgsign", "false")] {
        env.git(&repo, &["config", key, value]);
    }
    let setup = Setup { timeout: Duration::from_secs(30), log: env.root().join("lead.log") };
    let lead = create_lead_worktree(&repo, &PlanId::parse(PLAN).unwrap(), "main", &setup).unwrap().path;
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n{config_extra}\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n[tmux]\nsocket = \"{}\"\n",
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
    // The work session as the engine leaves it: its `watch` window, and
    // remain-on-exit (§6.1) so a dead Worker pane stays to be read.
    for args in [&["new-session", "-d", "-s", "cpd-work", "-n", "watch", "sleep", "600"][..], &["set", "-g", "remain-on-exit", "on"]] {
        assert!(env.tmux(args).status.success(), "tmux {args:?}");
    }
    let fx = Fx { env, repo, lead };
    fx.pi("exit 0");
    let catalog: String = EFFORTS
        .iter()
        .map(|(letter, model, efforts)| {
            format!("INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{model}', '{efforts}', 't');")
        })
        .collect();
    fx.fixture(&format!(
        "{catalog}
         INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'main', 'dispatch', 'cpd/pay', '{lead}', '{PR_TITLE}', 'config', cpd_txn(), 't', 't');",
        repo = fx.repo.display(),
        lead = fx.lead.display()
    ));
    fx.add_job(JOB, &[]);
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

    /// Job `key` of PR `repo` of Plan `pay`, queued with its Locked-Job fingerprint.
    fn add_job(&self, key: &str, needs: &[&str]) {
        self.add_job_in("repo", key, needs);
    }

    /// [`Fx::add_job`] in PR `pr`.
    fn add_job_in(&self, pr: &str, key: &str, needs: &[&str]) {
        let mut sql = format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('pay', '{pr}', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't');"
        );
        for need in needs {
            sql += &format!("INSERT INTO dependencies (plan, job, needs) VALUES ('pay', '{key}', '{need}');");
        }
        self.fixture(&sql);
        let plan = PlanId::parse(PLAN).unwrap();
        let all = self.store().read(|conn| jobs::load(conn, &plan)).unwrap();
        let job = all.iter().find(|j| j.key == key).unwrap();
        self.fixture(&format!(
            "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = 'pay' AND key = '{key}';",
            locked_fingerprint(job)
        ));
    }

    /// The fake `pi`: `body` runs in the Worker's pane, cwd the worktree, with
    /// `$CPD` the binary and `$LEAD` the Lead worktree, once the Attempt is
    /// `running` (the driver records that right after the pane starts; a real
    /// pi takes far longer to boot than this fake).
    fn pi(&self, body: &str) {
        self.env.fake_pi(&format!(
            "CPD='{CPD}'\nLEAD='{}'\n\
             until \"$CPD\" attempt show \"$CPD_ATTEMPT\" | grep -q '\"status\":\"running\"'; do sleep 0.1; done\n{body}",
            self.lead.display()
        ));
    }

    /// `job dispatch <key>`, its driver with [`DRIVER_ENV`].
    fn dispatch(&self, key: &str) -> Run {
        let mut cmd = self.env.command(CPD);
        cmd.envs(DRIVER_ENV).args(["job", "dispatch", key, "--plan", PLAN]).current_dir(self.env.root());
        let out = common::output(&mut cmd).expect("run cpd");
        Run {
            exit: out.status.code().expect("cpd was killed by a signal"),
            stdout: String::from_utf8(out.stdout).expect("stdout is UTF-8"),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    fn attempt(&self, id: &str) -> Value {
        self.env.cpd(&["attempt", "show", id, "--plan", PLAN]).ok()["attempt"].clone()
    }

    fn job_status(&self, key: &str) -> String {
        self.env.query(&format!("SELECT status FROM jobs WHERE plan = 'pay' AND key = '{key}'"))
    }

    /// Waits until Attempt `id` is `status` in the store (at most 60 s); returns it as `attempt show` then gives it.
    fn wait_status(&self, id: &str, status: &str) -> Value {
        common::wait_some(&format!("Attempt {id} to become {status}"), Duration::from_secs(60), || {
            let now: Option<String> = self.env.query(&format!("SELECT (SELECT status FROM attempts WHERE plan = '{PLAN}' AND id = '{id}')"));
            if now.as_deref() == Some(status) { Ok(()) } else { Err(format!("status {now:?}\nlog: {}", self.log(id))) }
        });
        self.attempt(id)
    }

    /// Waits until the Attempt has ended and its driver has exited; returns
    /// the Attempt and the driver's output, the last line of its log.
    fn wait_end(&self, id: &str, pid: &Value) -> (Value, Value) {
        let attempt = self.wait_status(id, "ended");
        let pid = nix::unistd::Pid::from_raw(pid.as_i64().unwrap() as i32);
        common::wait_some(&format!("the driver of {id} to exit"), Duration::from_secs(30), || {
            if nix::sys::signal::kill(pid, None).is_ok() { Err(format!("log: {}", self.log(id))) } else { Ok(()) }
        });
        let log = self.log(id);
        let last = log.lines().last().unwrap_or_else(|| panic!("empty log of {id}"));
        let out: Value = serde_json::from_str(last).unwrap_or_else(|e| panic!("{e}: {log}"));
        (attempt, out)
    }

    fn log(&self, id: &str) -> String {
        fs::read_to_string(self.env.cpd_home().join(format!("run/pay/logs/attempt-{id}.log"))).unwrap_or_default()
    }

}

/// The nice of process `pid`: field 19 of `/proc/<pid>/stat`, counted after the `)` that closes the comm field
/// (the field after it is field 3, so field 19 is the 17th).
fn nice_of(pid: &str) -> i64 {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_else(|e| panic!("/proc/{pid}/stat: {e}"));
    stat[stat.rfind(')').unwrap() + 2..].split(' ').nth(16).unwrap().parse().unwrap()
}

#[test]
fn a_dispatched_attempts_driver_runs_at_nice_19_and_the_caller_keeps_its_nice() {
    let fx = fx();
    let release = fx.env.root().join("release");
    fx.pi(&format!("until [ -e '{}' ]; do sleep 0.1; done\nexit 0", release.display()));
    let before = nice_of("self");

    let out = fx.dispatch(JOB).ok();
    assert_eq!(out["attempt"], ATTEMPT);
    let pid = out["pid"].as_i64().unwrap().to_string();
    let driver_nice = nice_of(&pid);
    let after = nice_of("self");

    // Let the Attempt end before asserting, so that nothing leaks.
    fs::write(&release, "").unwrap();
    fx.wait_end(ATTEMPT, &out["pid"]);

    assert_eq!(driver_nice, 19, "nice of the driver, pid {pid}");
    assert_eq!(after, before, "the caller keeps its nice");
}

#[test]
fn the_worker_pi_runs_at_nice_19_and_io_class_idle() {
    let fx = fx();
    let seen = fx.env.root().join("seen.txt");
    fx.pi(&format!(
        "{{ sed 's/.*) //' /proc/$$/stat | cut -d' ' -f17; ionice -p $$; }} > '{}' 2>&1\nexit 0",
        seen.display()
    ));

    let out = fx.dispatch(JOB).ok();
    fx.wait_end(ATTEMPT, &out["pid"]);

    let text = fs::read_to_string(&seen).expect("the fake pi wrote its priority");
    assert_eq!(text.lines().next().map(str::trim), Some("19"), "nice of the Worker's pi; output: {text:?}");
    assert!(text.contains("idle"), "I/O class of the Worker's pi: {text:?}");
}
