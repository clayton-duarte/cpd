//! The follow-ups of `cpd install` on the new binary and the tag (docs/implementing.md "The install's engine
//! order"; contract §3.7): `cpd config`, `cpd watch --check`, the views, the engine restart, then the tagger.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::Install;
use crate::host::runner::NO_DEADLINE;
use crate::host::tmux;
use crate::ports::Ports;

/// How long a tmux listing or kill may take.
const TMUX_DEADLINE: Duration = Duration::from_secs(10);
/// The step of that wait.
const LOCK_STEP: Duration = Duration::from_millis(250);

/// What a finished call left: its exit (`-1` when it did not start), stdout and stderr.
struct Done {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Done {
    fn ok(&self) -> bool {
        self.code == 0
    }
    /// `<stdout> <stderr>` as the script's `$out $(cat errfile)` was.
    fn text(&self) -> String {
        format!("{} {}", self.stdout, self.stderr)
    }
}

fn done(out: Result<std::process::Output, crate::host::deadline::Failed>) -> Done {
    use crate::host::deadline::Failed;
    match out {
        Ok(out) => Done {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        },
        Err(Failed::Io(e)) => Done {
            code: -1,
            stdout: String::new(),
            stderr: e.to_string(),
        },
        Err(Failed::Timeout(e)) => Done {
            code: -1,
            stdout: String::new(),
            stderr: e.message().to_owned(),
        },
    }
}

/// The code and the message of a cpd error value, else its text as it is.
pub(super) fn describe(text: &str) -> String {
    let error = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v.get("error").cloned());
    let field = |name: &str| {
        error
            .as_ref()
            .and_then(|e| e.get(name))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    match (field("code"), field("message")) {
        (Some(code), Some(message)) => format!("{message} ({code})"),
        _ => text.to_owned(),
    }
}

/// The tmux part of `cpd config`: the socket and the two sessions; an answer that has none leaves them empty.
#[derive(Default)]
struct TmuxConf {
    socket: String,
    cockpit: String,
    work: String,
}

fn tmux_conf(config: &str) -> TmuxConf {
    let value: serde_json::Value = serde_json::from_str(config).unwrap_or_default();
    let tmux = value
        .get("config")
        .and_then(|c| c.get("tmux"))
        .or_else(|| value.get("tmux"));
    let field = |name: &str| {
        tmux.and_then(|t| t.get(name))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned()
    };
    TmuxConf {
        socket: field("socket"),
        cockpit: field("cockpit_session"),
        work: field("work_session"),
    }
}

struct FollowUps<'a> {
    ports: &'a Ports,
    install: &'a Install,
    dest: &'a Path,
    err: &'a mut dyn Write,
    failed: bool,
}

impl FollowUps<'_> {
    fn fail(&mut self, text: impl std::fmt::Display) {
        let _ = writeln!(self.err, "cpd install follow-up failed: {text}");
        self.failed = true;
    }

    /// `<dest>/cpd <args>` with the caller's `CPD_HOME`.
    fn cpd(&self, args: &[&str]) -> Done {
        let mut cmd = Command::new(self.dest);
        cmd.args(args).env("CPD_HOME", self.install.cpd_home.root());
        done(self.ports.runner.output(&mut cmd, "cpd", NO_DEADLINE))
    }

    /// `tmux -u -L <socket> -f /dev/null <args>`.
    fn tmux(&self, socket: &str, args: &[&str]) -> Done {
        let mut cmd = tmux::client("tmux");
        cmd.args(["-L", socket, "-f", "/dev/null"]).args(args);
        done(self.ports.runner.output(&mut cmd, "tmux", TMUX_DEADLINE))
    }

    /// Whether the session `target` has a window named `window`.
    fn has_window(&self, socket: &str, session: &str, window: &str) -> bool {
        let listed = self.tmux(
            socket,
            &[
                "list-windows",
                "-t",
                &format!("={session}"),
                "-F",
                "#{window_name}",
            ],
        );
        listed.ok() && listed.stdout.lines().any(|l| l == window)
    }

    /// Whether nobody holds the lock on `path` (the lock cpd takes, `crate::lock`).
    fn lock_free(path: &Path) -> io::Result<bool> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        crate::lock::try_lock(&file)
    }

    fn run(&mut self) {
        let untouched = "the engine was not touched: the engine that runs, if one does, is the old one and keeps running; the views were not respawned";
        let config = self.cpd(&["config"]);
        if !config.ok() {
            self.fail(format!("cpd config: {}; {untouched}", config.text()));
            return;
        }
        let conf = tmux_conf(&config.stdout);
        let checked = self.cpd(&["watch", "--check"]);
        if !checked.ok() {
            self.fail(format!(
                "cpd watch --check: {} {}; {untouched}",
                describe(&checked.stdout),
                checked.stderr
            ));
            return;
        }
        if !conf.socket.is_empty()
            && !conf.cockpit.is_empty()
            && self.has_window(&conf.socket, &conf.cockpit, "main")
        {
            let out = self.cpd(&["tui", "--respawn"]);
            if out.ok() {
                let _ = writeln!(self.err, "{}", out.stdout);
            } else {
                self.fail(format!(
                    "cpd tui --respawn exited {}: {} {}; the views still run the old binary",
                    out.code,
                    describe(&out.stdout),
                    out.stderr
                ));
            }
        }
        if !conf.socket.is_empty()
            && !conf.work.is_empty()
            && self.has_window(&conf.socket, &conf.work, "watch")
        {
            self.restart_engine(&conf);
        }
    }

    /// Retires the old engine and starts the new one.
    fn restart_engine(&mut self, conf: &TmuxConf) {
        let _ = writeln!(
            self.err,
            "restarting the running engine (cpd watch) to run the new binary"
        );
        let mark = self.cpd(&["watch", "--mark-restart"]);
        if !mark.ok() {
            self.fail(format!(
                "cpd watch --mark-restart: {}; the engine restart is not marked as planned",
                mark.text()
            ));
        }
        let target = format!("={}:watch", conf.work);
        let killed = self.tmux(&conf.socket, &["kill-window", "-t", &target]);
        if !killed.ok() {
            if self.has_window(&conf.socket, &conf.work, "watch") {
                self.fail(format!(
                    "tmux kill-window {}:watch failed: {}{}; the engine was not stopped and still runs the old binary",
                    conf.work, killed.stdout, killed.stderr
                ));
                return;
            }
            let _ = writeln!(self.err, "the engine had already left; nothing to kill");
        }
        // The killed engine releases its lock a moment later, possibly well after: wait for the lock itself,
        // bounded, then start the engine once and check that it holds the lock (hs-n288).
        let lock = self.install.cpd_home.watch_lock();
        if let Some(dir) = lock.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            self.fail(format!("cannot create {}: {e}", dir.display()));
            return;
        }
        let mut waited = Duration::ZERO;
        let free = loop {
            match Self::lock_free(&lock) {
                Ok(true) => break true,
                Ok(false) if waited >= self.install.lock_wait => break false,
                Ok(false) => {
                    self.ports.clock.sleep(LOCK_STEP);
                    waited += LOCK_STEP;
                }
                Err(e) => {
                    self.fail(format!("cannot probe {}: {e}", lock.display()));
                    return;
                }
            }
        };
        if !free {
            self.fail(format!(
                "the old engine still holds {} after {}s: it was told to stop and has not; the new engine was not started",
                lock.display(),
                self.install.lock_wait.as_secs()
            ));
            return;
        }
        let ensured = self.cpd(&["watch", "--ensure"]);
        if !ensured.ok() {
            self.fail(format!(
                "cpd watch --ensure: {} {}; the old engine was stopped and no engine is running: run cpd watch --ensure, the server's hooks also retry it every few seconds",
                describe(&ensured.stdout),
                ensured.stderr
            ));
            return;
        }
        match Self::lock_free(&lock) {
            Ok(true) => self.fail(format!(
                "cpd watch --ensure answered {}, but no engine holds {}: the old engine was stopped and no engine is running",
                ensured.stdout,
                lock.display()
            )),
            Ok(false) => {
                let _ = writeln!(self.err, "{}", ensured.stdout);
            }
            Err(e) => self.fail(format!("cannot probe {}: {e}", lock.display())),
        }
    }
}

/// Runs the follow-ups on `<dest>`; whether one failed.
pub(super) fn run(ports: &Ports, install: &Install, dest: &Path, err: &mut dyn Write) -> bool {
    let mut follow_ups = FollowUps {
        ports,
        install,
        dest,
        err,
        failed: false,
    };
    follow_ups.run();
    follow_ups.failed
}

/// The last step: the tagger. It never fails the install; the next install retries the tag.
pub(super) fn tag(ports: &Ports, install: &Install, err: &mut dyn Write) {
    let mut cmd = Command::new(install.root.join("scripts/tag-version.sh"));
    let out = done(ports.runner.output(&mut cmd, "tag-version", NO_DEADLINE));
    if !out.stderr.is_empty() {
        let _ = write!(err, "{}", out.stderr);
    }
    if !out.ok() {
        let _ = writeln!(err, "the next install retries the tag");
    }
}
