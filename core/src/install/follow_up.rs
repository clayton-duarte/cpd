//! The follow-ups of `cpd install` on the new binary and the tag (docs/implementing.md "The install's engine
//! order"; contract §3.7): `cpd config`, `cpd watch --check`, the views, the engine restart, then the tagger.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::Install;
use crate::host::runner::NO_DEADLINE;
use crate::host::spawn::{Pid, Signal};
use crate::host::tmux;
use crate::ports::Ports;

/// How long a tmux listing or kill may take.
const TMUX_DEADLINE: Duration = Duration::from_secs(10);
/// The step of that wait.
const LOCK_STEP: Duration = Duration::from_millis(250);
/// The longest the install waits for the prepared WebUI to listen, and the step of that wait.
const WEBUI_READY_WAIT: Duration = Duration::from_secs(120);

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

/// The tmux part of `cpd config`: the socket and the work session; an answer that has none leaves them empty.
#[derive(Default)]
struct TmuxConf {
    socket: String,
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

    fn has_live_pane(&self, socket: &str, session: &str, window: &str) -> bool {
        let target = format!("={session}:{window}");
        let out = self.tmux(socket, &["list-panes", "-t", &target, "-F", "#{pane_dead}"]);
        out.ok() && out.stdout.lines().any(|l| l.trim() == "0")
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
            && !conf.work.is_empty()
            && self.has_window(&conf.socket, &conf.work, "watch")
        {
            self.restart_engine(&conf);
        }
    }

    /// The pid the old WebUI recorded in `run/webui.pid`, if any.
    fn old_webui_pid(&self) -> Option<Pid> {
        let text =
            std::fs::read_to_string(self.install.cpd_home.run_dir().join("webui.pid")).ok()?;
        text.trim().parse().ok().map(Pid)
    }

    /// `cpd webui --prepare`: starts the new WebUI beside the old one; its pid. A failure is reported and the old
    /// WebUI is left to the new engine's own supervision.
    fn prepare_webui(&mut self) -> Option<Pid> {
        let prepared = self.cpd(&["webui", "--prepare"]);
        let pid = prepared.ok().then(|| {
            serde_json::from_str::<serde_json::Value>(&prepared.stdout)
                .ok()?
                .get("pid")?
                .as_u64()
                .and_then(|p| u32::try_from(p).ok())
        });
        match pid.flatten() {
            Some(pid) => Some(Pid(pid)),
            None => {
                self.fail(format!(
                    "cpd webui --prepare: {} {}; the old WebUI keeps serving",
                    describe(&prepared.stdout),
                    prepared.stderr
                ));
                None
            }
        }
    }

    /// Waits (on the clock, bounded) for `cpd webui --ready`, then and only then stops the old WebUI.
    fn hand_off_webui(&mut self, old: Option<Pid>, new: Pid) {
        let mut waited = Duration::ZERO;
        loop {
            let ready = self.cpd(&["webui", "--ready"]);
            let is_ready = ready.ok()
                && serde_json::from_str::<serde_json::Value>(&ready.stdout)
                    .ok()
                    .and_then(|v| v.get("ready")?.as_bool())
                    == Some(true);
            if is_ready {
                break;
            }
            if waited >= WEBUI_READY_WAIT {
                self.fail(format!(
                    "install_failed: the new WebUI (pid {}) was not ready after {}s; the old WebUI keeps serving",
                    new.0,
                    WEBUI_READY_WAIT.as_secs()
                ));
                return;
            }
            self.ports.clock.sleep(LOCK_STEP);
            waited += LOCK_STEP;
        }
        if let Some(old) = old.filter(|old| *old != new)
            && let Err(e) = self.ports.spawn.signal(old, Signal::Term)
        {
            self.fail(format!("cannot stop the old WebUI (pid {}): {e}", old.0));
        }
    }

    /// Retires the old engine and starts the new one.
    fn restart_engine(&mut self, conf: &TmuxConf) {
        let _ = writeln!(
            self.err,
            "restarting the running engine (cpd watch) to run the new binary"
        );
        // Prepare the new WebUI beside the old one, which keeps serving until the hand-off (pro-d592).
        let old_webui = self.old_webui_pid();
        let prepared = self.prepare_webui();
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
            // Hand off: after the full wait (an explicit shorter `lock_wait` is a caller's own bound and is not handed
            // off), a detached waiter starts the engine once the lock frees, so the install never leaves it down.
            let handoff = if self.install.lock_wait >= super::DEFAULT_LOCK_WAIT {
                write_install_state(self.ports, self.install, "restarting");
                if self.cpd(&WAITER_ARGS).ok() {
                    "; a detached waiter starts it when the lock frees"
                } else {
                    "; the restart waiter could not be started: run cpd watch --ensure"
                }
            } else {
                ""
            };
            self.fail(format!(
                "the old engine still holds {} after {}s: it was told to stop and has not; the new engine was not started{handoff}",
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
                if !self.has_live_pane(&conf.socket, &conf.work, "watch") {
                    self.fail(format!(
                        "cpd watch --ensure answered {}, and {} is held, but the engine's window {}:watch has no live pane: a process holds the lock outside its window and is not supervised; run cpd watch --ensure",
                        ensured.stdout,
                        lock.display(),
                        conf.work
                    ));
                    return;
                }
                let _ = writeln!(self.err, "{}", ensured.stdout);
                if let Some(new) = prepared {
                    self.hand_off_webui(old_webui, new);
                }
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

/// The argv of the installed `cpd` that is the detached restart waiter (the install hands off to it when the old engine
/// still holds `locks/watch.lock` after `lock_wait`). The command detaches itself and returns at once.
pub(super) const WAITER_ARGS: [&str; 2] = ["watch", "--ensure-when-free"];

/// The longest the waiter waits for the lock: an hour, so a lock that never frees ends it.
#[allow(dead_code)] // the `watch --ensure-when-free` command that calls the waiter is a later change
const WAITER_BOUND: Duration = Duration::from_secs(3600);

/// Writes `run/cpd-install.json` as `{"status":<status>,"since":<now>}`.
fn write_install_state(ports: &Ports, install: &Install, status: &str) {
    use time::format_description::well_known::Rfc3339;
    let run_dir = install.cpd_home.run_dir();
    let since = ports
        .clock
        .utc()
        .to_offset(time::UtcOffset::UTC)
        .format(&Rfc3339)
        .unwrap_or_default();
    let line = format!("{{\"status\":\"{status}\",\"since\":\"{since}\"}}\n");
    let _ = std::fs::create_dir_all(&run_dir);
    let _ = std::fs::write(run_dir.join("cpd-install.json"), line);
}

/// The waiter's body: waits (on the clock) for `locks/watch.lock` to be free, then runs `cpd watch --ensure` once,
/// and marks the install `ok` again. Idempotent: `cpd watch --ensure` is.
#[allow(dead_code)] // the `watch --ensure-when-free` command that calls the waiter is a later change
pub(super) fn restart_waiter(ports: &Ports, install: &Install, dest: &Path) {
    let lock = install.cpd_home.watch_lock();
    let mut waited = Duration::ZERO;
    while waited < WAITER_BOUND && !FollowUps::lock_free(&lock).unwrap_or(true) {
        ports.clock.sleep(LOCK_STEP);
        waited += LOCK_STEP;
    }
    let mut cmd = Command::new(dest);
    cmd.args(["watch", "--ensure"])
        .env("CPD_HOME", install.cpd_home.root());
    if done(ports.runner.output(&mut cmd, "cpd", NO_DEADLINE)).ok() {
        write_install_state(ports, install, "ok");
    }
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
