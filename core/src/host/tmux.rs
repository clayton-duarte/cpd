//! The typed tmux wrapper (contract §6.1, §6.3, with commands.md §19
//! amendment 9: `respawn-pane`).
//!
//! Every call is `tmux -L <socket> -f /dev/null …`, spawned from an argv (never
//! a shell), with `TMUX` and `TMUX_PANE` removed from its environment. `-f
//! /dev/null` only matters to the call that starts the server, but passing it
//! on every call means no call can ever start one that reads the User's config.
//! A call that exits non-zero where cpd did not expect it is `tmux_failed`
//! (exit 1) with `argv`, `exit` and the last 60 lines of stderr (commands.md §1.3).
//! A call that runs past [`DEADLINE`] is killed and fails `tool_timeout` (§6.3),
//! except the [`INTERACTIVE`] ones.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::time::Duration;

use crate::error::{CpdError, ErrorCode};
use crate::ids::SessionId;

use super::deadline::{self, Failed};
use super::layout::View;

/// Every tmux call's deadline (contract §6.3): a local socket answers in
/// milliseconds, so a longer wait means a hung server.
const DEADLINE: Duration = Duration::from_secs(10);
/// The calls with no deadline (contract §6.3): `display-popup` returns when the
/// User closes the popup, which may be any time later.
const INTERACTIVE: &[&str] = &["display-popup"];

/// The deadline of the call `args`; `None` for an [`INTERACTIVE`] one.
fn deadline_of(args: &[String]) -> Option<Duration> {
    match args.first() {
        Some(call) if INTERACTIVE.contains(&call.as_str()) => None,
        _ => Some(DEADLINE),
    }
}

/// tmux's `-x`/`-y` for a new session (contract §6.3).
pub const SESSION_WIDTH: u32 = 215;
pub const SESSION_HEIGHT: u32 = 62;

/// The pane option naming the Session a pane runs (contract §6.2).
pub const OPT_SESSION: &str = "@cpd_session";
/// The pane option holding a Lead pane's header: its Plan's title, or a side
/// fork's name. A label for people that cpd keeps current, the one exception
/// to "pane options identify panes, never state" (contract §6.2).
pub const OPT_TITLE: &str = "@cpd_title";
/// The pane option naming the TUI view a pane runs (contract §6.2, amendment 2).
pub const OPT_VIEW: &str = "@cpd_view";
/// The pane option naming the Session a history pane shows (commands.md §10.12).
pub const OPT_HISTORY: &str = "@cpd_history";

/// The pane border titles (contract §6.1), keyed by the pane options, first
/// match: a Lead's pane (`@cpd_session`) shows its `@cpd_title` when set, else
/// nothing; the chat placeholder, a history pane and the
/// StatusBar have none, since pi and the placeholder say what they are; the
/// views have their column titles; any other pane shows its command. Every
/// title is drawn reversed (`#[reverse]`..`#[noreverse]`), a block in the border's
/// colour with the text in the background colour.
pub const BORDER_FORMAT: &str = "#{?#{@cpd_session},#{?#{@cpd_title},#[reverse] #{@cpd_title} #[noreverse],},#{?#{==:#{@cpd_view},chat},,#{?#{==:#{@cpd_view},history},,#{?#{==:#{@cpd_view},statusbar},,\
#[reverse]#{?#{==:#{@cpd_view},worker}, Worker · #{@cpd_job} (read-only) ,#{?#{==:#{@cpd_view},tree}, Context ,\
#{?#{==:#{@cpd_view},plan}, Plan ,#{?#{==:#{@cpd_view},widgets}, Deploy ,#{pane_current_command}}}}}#[noreverse]}}}}";

/// The `pane-died` hook (contract §6.1): only a pane that carries `@cpd_view`
/// (a view of the cockpit) is respawned when its program dies; a Lead or
/// engine pane stays dead, as `remain-on-exit` leaves it, so engine E2 sees
/// its death. tmux runs it with the dead pane as the current target.
pub const PANE_DIED_HOOK: &str = r##"if -F "#{@cpd_view}" "respawn-pane""##;

/// Every global option, key binding and hook of contract §6.1, each as the
/// arguments after `tmux -L <socket>`, in the order of the reference's
/// `tui/scripts/tmux.conf` (lines 2–17), renamed (`@cpd_view`, `@cpd_job`).
/// Set at every server start, whichever command starts it (see
/// [`new_session_args`]), and again by [`Tmux::apply_server_options`].
///
/// The click binding: a click on the StatusBar is passed to it and never
/// selects it, so the chat keeps the keyboard focus; every other pane is
/// selected, then gets the click.
pub const SERVER_OPTIONS: &[&[&str]] = &[
    &["set", "-g", "mouse", "on"],
    &["set", "-g", "focus-events", "on"],
    &["set", "-g", "default-terminal", "tmux-256color"],
    &["set", "-g", "extended-keys", "on"],
    &["set", "-as", "terminal-features", TERMINAL_FEATURES],
    &["set", "-g", "escape-time", "10"],
    &["set", "-g", "status", "off"],
    &["set", "-g", "pane-border-status", "top"],
    &["set", "-g", "pane-border-lines", "heavy"],
    &["set", "-g", "pane-border-format", BORDER_FORMAT],
    &["set", "-g", "pane-active-border-style", "fg=blue"],
    &["set", "-g", "remain-on-exit", "on"],
    &["set-hook", "-g", "pane-died", PANE_DIED_HOOK],
    &["bind", "-T", "root", "MouseDown1Pane", "if", "-F", "#{==:#{@cpd_view},statusbar}", "send-keys -M", "select-pane -t = ; send-keys -M"],
];

/// The value `terminal-features` is appended with (`set -as`), the one entry of
/// [`SERVER_OPTIONS`] that is not idempotent: it runs once, when cpd starts
/// the server, and [`Tmux::apply_server_options`] skips it.
const TERMINAL_FEATURES: &str = ",xterm*:extkeys,xterm-ghostty:extkeys,xterm-ghostty:hyperlinks";

/// How many stderr lines a `tmux_failed` error carries (commands.md §1.3).
const STDERR_TAIL_LINES: usize = 60;

/// A tmux pane id, `%<n>`. Holding one proves the shape.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PaneId(String);

/// A tmux window id, `@<n>`. Holding one proves the shape.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WindowId(String);

fn tmux_id(s: &str, sigil: char) -> bool {
    s.strip_prefix(sigil).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

impl PaneId {
    /// `%<n>`, else `None`.
    pub fn parse(s: &str) -> Option<PaneId> {
        tmux_id(s, '%').then(|| PaneId(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl WindowId {
    /// `@<n>`, else `None`.
    pub fn parse(s: &str) -> Option<WindowId> {
        tmux_id(s, '@').then(|| WindowId(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Display for WindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One pane as `list-panes -a` reports it (contract §6.3, liveness).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pane {
    pub pane: PaneId,
    pub window: WindowId,
    pub session_name: String,
    /// The window's name: a label for people, used only to find `cpd:main`.
    pub window_name: String,
    /// `#{pane_dead}`: the program has exited and the pane remains (`remain-on-exit`).
    pub dead: bool,
    /// `#{pane_dead_status}`: the exit status of a dead pane's program, when it exited normally.
    pub dead_status: Option<i32>,
    /// `#{pane_pid}`: `None` once the pane's tty has closed on tmux >= 3.8,
    /// which then prints it empty: when the pane is dead, and also while it
    /// is dying (its program not yet reaped, so `dead` is still false).
    pub pid: Option<u32>,
    /// `@cpd_session`.
    pub cpd_session: Option<SessionId>,
    /// `@cpd_view`.
    pub cpd_view: Option<View>,
}

/// The fields of [`Pane`], tab-separated: the liveness format of contract §6.3
/// plus `window_name` and `@cpd_view`, which find the chat slot (§6.2). Tabs,
/// not spaces, because `pane_dead_status` and the options are empty when unset.
const LIST_PANES_FORMAT: &str = "#{pane_id}\t#{window_id}\t#{session_name}\t#{window_name}\t#{pane_dead}\t#{pane_dead_status}\t#{pane_pid}\t#{@cpd_session}\t#{@cpd_view}";

/// What a new window or pane runs: its argv and environment.
///
/// tmux runs a command of several arguments directly and a single argument
/// through a shell (tmux(1)), so `argv` should have at least two elements.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Spawn {
    /// Working directory (`-c`); none keeps tmux's default.
    pub cwd: Option<PathBuf>,
    /// Added environment (`-e K=V`).
    pub env: BTreeMap<String, String>,
    /// The program and its arguments, after `--`.
    pub argv: Vec<String>,
}

impl Spawn {
    fn push_args(&self, args: &mut Vec<String>) {
        if let Some(cwd) = &self.cwd {
            args.push("-c".into());
            args.push(cwd.display().to_string());
        }
        for (key, value) in &self.env {
            args.push("-e".into());
            args.push(format!("{key}={value}"));
        }
    }

    fn push_argv(&self, args: &mut Vec<String>) {
        if !self.argv.is_empty() {
            args.push("--".into());
            args.extend(self.argv.iter().cloned());
        }
    }
}

/// Where `split-window` puts the new pane relative to the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    /// Side by side (`-h`).
    Right,
    /// Stacked (`-v`).
    Below,
    /// Full width at the bottom of the window (`-v -f`).
    Bottom,
}

impl Split {
    /// The `split-window` flags that place the pane.
    fn flags(self) -> &'static [&'static str] {
        match self {
            Split::Right => &["-h"],
            Split::Below => &["-v"],
            Split::Bottom => &["-v", "-f"],
        }
    }
}

/// The tmux server cpd uses: `tmux -L <socket>` (contract §6.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tmux {
    socket: String,
}

impl Tmux {
    /// The server on socket `socket` (config `tmux.socket`).
    pub fn new(socket: impl Into<String>) -> Tmux {
        Tmux { socket: socket.into() }
    }

    pub fn socket(&self) -> &str {
        &self.socket
    }

    /// The full argv of a call: `tmux -L <socket> -f /dev/null <args>`.
    pub fn argv(&self, args: &[String]) -> Vec<String> {
        let mut argv = vec!["tmux".to_owned(), "-L".to_owned(), self.socket.clone(), "-f".to_owned(), "/dev/null".to_owned()];
        argv.extend(args.iter().cloned());
        argv
    }

    /// The `Command` for a call: its argv, without `TMUX` and `TMUX_PANE`.
    fn command(&self, args: &[String]) -> Command {
        let argv = self.argv(args);
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]).env_remove("TMUX").env_remove("TMUX_PANE").stdin(Stdio::null());
        cmd
    }

    fn output(&self, args: &[String]) -> Result<Output, CpdError> {
        let argv = self.argv(args);
        let mut cmd = self.command(args);
        let ran = match deadline_of(args) {
            Some(limit) => deadline::output(&mut cmd, "tmux", limit),
            // In its own process group, as every other call.
            None => cmd.process_group(0).output().map_err(Failed::Io),
        };
        ran.map_err(|e| match e {
            Failed::Io(e) => CpdError::new(ErrorCode::TmuxFailed, format!("Cannot run tmux: {e}."))
                .with("argv", argv.clone())
                .with("exit", serde_json::Value::Null)
                .with("stderr", e.to_string()),
            Failed::Timeout(e) => e,
        })
    }

    fn failed(&self, args: &[String], out: &Output) -> CpdError {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let lines: Vec<&str> = stderr.lines().collect();
        let tail = lines[lines.len().saturating_sub(STDERR_TAIL_LINES)..].join("\n");
        let first = lines.first().copied().unwrap_or("no output");
        CpdError::new(ErrorCode::TmuxFailed, format!("tmux {} failed: {first}", args.first().map_or("", String::as_str)))
            .with("argv", self.argv(args))
            .with("exit", out.status.code())
            .with("stderr", tail)
    }

    /// Runs a call that must succeed; returns its stdout.
    pub fn run(&self, args: &[String]) -> Result<String, CpdError> {
        let out = self.output(args)?;
        if !out.status.success() {
            return Err(self.failed(args, &out));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Runs a call whose exit 1 means "no": true on 0, false on 1, else `tmux_failed`.
    fn check(&self, args: &[String]) -> Result<bool, CpdError> {
        let out = self.output(args)?;
        match out.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(self.failed(args, &out)),
        }
    }

    /// Whether the server is running (it has at least one session). False
    /// only when tmux says no server runs ([`no_server`]); any other failure
    /// is `tmux_failed` (contract §6.3 liveness, engine-W8).
    pub fn server_running(&self) -> Result<bool, CpdError> {
        let args = strings(&["list-sessions", "-F", "#{session_id}"]);
        let out = self.output(&args)?;
        if out.status.success() {
            return Ok(true);
        }
        if no_server(&String::from_utf8_lossy(&out.stderr)) {
            return Ok(false);
        }
        Err(self.failed(&args, &out))
    }

    /// `has-session -t =<name>`: an exact name match. False when no server runs.
    pub fn has_session(&self, name: &str) -> Result<bool, CpdError> {
        self.check(&strings(&["has-session", "-t", &format!("={name}")]))
    }

    /// Makes sure session `name` exists (contract §6.1, §6.3): when it is
    /// missing, `new-session -d -s <name> -n <window> -x 215 -y 62 -c <dir>
    /// [-- argv]`; when that starts the server, sets [`SERVER_OPTIONS`] first
    /// (see [`new_session_args`]).
    /// Returns whether it created the session. Idempotent; a concurrent creator
    /// winning the race is not an error.
    pub fn ensure_session(&self, name: &str, window: &str, spawn: &Spawn) -> Result<bool, CpdError> {
        Ok(self.create_session(name, window, spawn)?.is_some())
    }

    /// [`Tmux::ensure_session`], returning the ids of the first window and its
    /// pane when it created the session (`-P -F '#{window_id} #{pane_id}'`),
    /// `None` when the session already existed.
    ///
    /// `new-session -e` sets the variables in the new session's environment,
    /// which every later window of the session would inherit (tmux(1)); they
    /// are unset from it at once (`set-environment -u`), so like
    /// [`Tmux::new_window`]'s `-e` they reach only this first pane.
    pub fn create_session(&self, name: &str, window: &str, spawn: &Spawn) -> Result<Option<(WindowId, PaneId)>, CpdError> {
        let fresh = !self.server_running()?;
        if !fresh && self.has_session(name)? {
            return Ok(None);
        }
        let args = new_session_args(name, window, spawn, fresh);
        let out = self.output(&args)?;
        if !out.status.success() {
            if self.has_session(name)? {
                return Ok(None);
            }
            return Err(self.failed(&args, &out));
        }
        let printed = String::from_utf8_lossy(&out.stdout);
        let ids = printed.trim_end_matches('\n');
        let parsed = ids.split_once(' ').and_then(|(w, p)| Some((WindowId::parse(w)?, PaneId::parse(p)?)));
        let parsed = parsed.ok_or_else(|| self.unexpected(&args, ids))?;
        for key in spawn.env.keys() {
            self.run(&strings(&["set-environment", "-t", &format!("={name}"), "-u", key]))?;
        }
        Ok(Some(parsed))
    }

    /// Sets [`SERVER_OPTIONS`] on a running server (contract §6.1: `cpd tui`
    /// applies them again), all but the `terminal-features` append, which the
    /// server's start already made. Rerunning is safe.
    pub fn apply_server_options(&self) -> Result<(), CpdError> {
        for option in SERVER_OPTIONS.iter().filter(|o| o[1] != "-as") {
            self.run(&strings(option))?;
        }
        Ok(())
    }

    /// `display -p '#{version}'`: the version of the tmux the server runs
    /// (`3.4`, `next-3.7`), which is the one it was started with.
    pub fn server_version(&self) -> Result<String, CpdError> {
        Ok(self.run(&strings(&["display", "-p", "#{version}"]))?.trim().to_owned())
    }

    /// The server's own tmux program: `/proc/<server pid>/exe` (Linux), which
    /// execs the binary the server runs even when an upgrade has replaced its
    /// file. A client of another version cannot attach to the server (hs-n224).
    pub fn server_exe(&self) -> Result<PathBuf, CpdError> {
        let out = self.run(&strings(&["display", "-p", "#{pid}"]))?;
        let exe = PathBuf::from(format!("/proc/{}/exe", out.trim()));
        std::fs::metadata(&exe).map_err(|e| {
            CpdError::new(ErrorCode::TmuxFailed, format!("Cannot find the tmux server's own binary {}: {e}.", exe.display()))
                .with("path", exe.display().to_string())
        })?;
        Ok(exe)
    }

    /// `show -gv <option>`: a global option's value, without the newline.
    pub fn global_option(&self, option: &str) -> Result<String, CpdError> {
        Ok(self.run(&strings(&["show", "-gv", option]))?.trim_end_matches('\n').to_owned())
    }

    /// `set -p -t <pane> @cpd_session <id>`.
    pub fn set_pane_session(&self, pane: &PaneId, session: &SessionId) -> Result<(), CpdError> {
        self.run(&strings(&["set", "-p", "-t", pane.as_str(), OPT_SESSION, session.as_str()])).map(drop)
    }

    /// `set -p -t <pane> @cpd_title <title>`.
    pub fn set_pane_title(&self, pane: &PaneId, title: &str) -> Result<(), CpdError> {
        self.run(&strings(&["set", "-p", "-t", pane.as_str(), OPT_TITLE, title])).map(drop)
    }

    /// `set -p -t <pane> @cpd_view <view>`.
    pub fn set_pane_view(&self, pane: &PaneId, view: View) -> Result<(), CpdError> {
        self.run(&strings(&["set", "-p", "-t", pane.as_str(), OPT_VIEW, view.as_str()])).map(drop)
    }

    /// `set -p -t <pane> @cpd_history <id>`.
    pub fn set_pane_history(&self, pane: &PaneId, session: &SessionId) -> Result<(), CpdError> {
        self.run(&strings(&["set", "-p", "-t", pane.as_str(), OPT_HISTORY, session.as_str()])).map(drop)
    }

    /// `new-window -d -t =<session>: -n <name> [-c <cwd>] [-e K=V …] -P -F
    /// '#{window_id} #{pane_id}' -- <argv…>` (contract §6.3).
    pub fn new_window(&self, session: &str, name: &str, spawn: &Spawn) -> Result<(WindowId, PaneId), CpdError> {
        let mut args = strings(&["new-window", "-d", "-t", &format!("={session}:"), "-n", name]);
        spawn.push_args(&mut args);
        args.extend(strings(&["-P", "-F", "#{window_id} #{pane_id}"]));
        spawn.push_argv(&mut args);
        let out = self.run(&args)?;
        let ids = out.trim_end_matches('\n');
        let parsed = ids.split_once(' ').and_then(|(w, p)| Some((WindowId::parse(w)?, PaneId::parse(p)?)));
        parsed.ok_or_else(|| self.unexpected(&args, ids))
    }

    /// `split-window -d -t <pane> -h|-v [-l <size>] [-c <cwd>] [-e K=V …] -P -F
    /// '#{pane_id}' -- <argv…>`: a new pane beside `target` (the cockpit's
    /// layout, commands.md §13.1). `size` is tmux's `-l`: cells, or `NN%`.
    pub fn split_window(&self, target: &PaneId, split: Split, size: Option<&str>, spawn: &Spawn) -> Result<PaneId, CpdError> {
        let mut args = strings(&["split-window", "-d", "-t", target.as_str()]);
        args.extend(strings(split.flags()));
        if let Some(size) = size {
            args.extend(strings(&["-l", size]));
        }
        spawn.push_args(&mut args);
        args.extend(strings(&["-P", "-F", "#{pane_id}"]));
        spawn.push_argv(&mut args);
        let out = self.run(&args)?;
        let id = out.trim_end_matches('\n');
        PaneId::parse(id).ok_or_else(|| self.unexpected(&args, id))
    }

    /// `swap-pane -d -s <source> -t <target>`: both processes keep their pids,
    /// and pane options move with the panes (contract §6.2, §6.3).
    pub fn swap_pane(&self, source: &PaneId, target: &PaneId) -> Result<(), CpdError> {
        self.run(&strings(&["swap-pane", "-d", "-s", source.as_str(), "-t", target.as_str()])).map(drop)
    }

    /// `rename-window -t <window> <name>`.
    pub fn rename_window(&self, window: &WindowId, name: &str) -> Result<(), CpdError> {
        self.run(&strings(&["rename-window", "-t", window.as_str(), name])).map(drop)
    }

    /// `respawn-pane -t <pane> [-c <cwd>] [-e K=V …] [-- <argv…>]`: restarts a
    /// dead pane of the cockpit's layout (amendment 9). Without `-k`, so a live
    /// pane is refused (`tmux_failed`), never killed. An empty argv reruns the
    /// pane's original command.
    pub fn respawn_pane(&self, pane: &PaneId, spawn: &Spawn) -> Result<(), CpdError> {
        let mut args = strings(&["respawn-pane", "-t", pane.as_str()]);
        spawn.push_args(&mut args);
        spawn.push_argv(&mut args);
        self.run(&args).map(drop)
    }

    /// `respawn-pane -k -t <pane> [-c <cwd>] [-e K=V …] [-- <argv…>]`: kills
    /// the pane's program, live or dead, and starts it again in place (contract
    /// §6.3: `cpd tui --respawn`). An empty argv reruns the pane's own command.
    pub fn respawn_pane_killing(&self, pane: &PaneId, spawn: &Spawn) -> Result<(), CpdError> {
        let mut args = strings(&["respawn-pane", "-k", "-t", pane.as_str()]);
        spawn.push_args(&mut args);
        spawn.push_argv(&mut args);
        self.run(&args).map(drop)
    }

    /// `kill-pane -t <pane>`; the window goes with its last pane.
    pub fn kill_pane(&self, pane: &PaneId) -> Result<(), CpdError> {
        self.run(&strings(&["kill-pane", "-t", pane.as_str()])).map(drop)
    }

    /// `kill-window -t <window>`.
    pub fn kill_window(&self, window: &WindowId) -> Result<(), CpdError> {
        self.run(&strings(&["kill-window", "-t", window.as_str()])).map(drop)
    }

    /// `list-panes -a`: every pane of the server; none only when tmux says no
    /// server runs ([`Tmux::server_running`]), else `tmux_failed`.
    pub fn list_panes(&self) -> Result<Vec<Pane>, CpdError> {
        let args = strings(&["list-panes", "-a", "-F", LIST_PANES_FORMAT]);
        let mut out = self.output(&args)?;
        if !out.status.success() {
            if !self.server_running()? {
                return Ok(Vec::new());
            }
            // The server was starting (another caller's `new-session`) when the
            // first call connected: list again, once.
            out = self.output(&args)?;
            if !out.status.success() {
                return Err(self.failed(&args, &out));
            }
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let mut panes = Vec::new();
        for line in stdout.lines() {
            match parse_pane(line).ok_or_else(|| self.unexpected(&args, line))? {
                Line::Pane(pane) => panes.push(pane),
                Line::UnknownView(view) => warn_unknown_view(&view),
            }
        }
        Ok(panes)
    }

    /// `tmux_failed` for output cpd cannot parse.
    fn unexpected(&self, args: &[String], output: &str) -> CpdError {
        CpdError::new(ErrorCode::TmuxFailed, format!("tmux {} printed unexpected output.", args[0]))
            .with("argv", self.argv(args))
            .with("exit", 0)
            .with("stdout", output.to_owned())
    }
}

/// The version of the installed tmux, the one on `PATH`: `tmux -V` without its
/// `tmux ` prefix, in the form of [`Tmux::server_version`].
pub fn installed_version() -> Result<String, CpdError> {
    let argv = vec!["tmux".to_owned(), "-V".to_owned()];
    let out = Command::new("tmux").arg("-V").env_remove("TMUX").env_remove("TMUX_PANE").stdin(Stdio::null()).output().map_err(|e| {
        CpdError::new(ErrorCode::TmuxFailed, format!("Cannot run tmux: {e}."))
            .with("argv", argv.clone())
            .with("exit", serde_json::Value::Null)
            .with("stderr", e.to_string())
    })?;
    let text = String::from_utf8_lossy(&out.stdout);
    match (out.status.success(), text.trim().strip_prefix("tmux ")) {
        (true, Some(version)) => Ok(version.to_owned()),
        _ => Err(CpdError::new(ErrorCode::TmuxFailed, "tmux -V did not print `tmux <version>`.")
            .with("argv", argv)
            .with("exit", out.status.code())
            .with("stderr", String::from_utf8_lossy(&out.stderr).into_owned())),
    }
}

/// Whether a failed call's stderr is tmux saying that no server runs: its
/// socket file is missing (`error connecting to <socket> (No such file or
/// directory)`) or nothing listens on it (`no server running on <socket>`),
/// as tmux 3.4 prints them.
fn no_server(stderr: &str) -> bool {
    stderr.lines().any(|line| {
        line.starts_with("no server running on ")
            || (line.starts_with("error connecting to ") && line.ends_with("(No such file or directory)"))
    })
}

/// The arguments of [`Tmux::create_session`]'s call: `new-session -d -s <name>
/// -n <window> -x 215 -y 62 [-c <cwd>] [-e K=V …] -P -F '#{window_id}
/// #{pane_id}' [-- argv…]`. On a `fresh` server it is preceded, in the same
/// invocation (`;` separates tmux commands), by [`SERVER_OPTIONS`]: tmux runs
/// them before it spawns the first pane, so a program that exits at once
/// leaves a dead pane (`remain-on-exit`) instead of taking the session and
/// the server with it.
fn new_session_args(name: &str, window: &str, spawn: &Spawn, fresh: bool) -> Vec<String> {
    let mut args = Vec::new();
    if fresh {
        for option in SERVER_OPTIONS {
            args.extend(strings(option));
            args.push(";".into());
        }
    }
    args.extend(strings(&["new-session", "-d", "-s", name, "-n", window]));
    args.extend(strings(&["-x", &SESSION_WIDTH.to_string(), "-y", &SESSION_HEIGHT.to_string()]));
    spawn.push_args(&mut args);
    args.extend(strings(&["-P", "-F", "#{window_id} #{pane_id}"]));
    spawn.push_argv(&mut args);
    args
}

/// A parsed `list-panes` line.
#[derive(Debug, PartialEq, Eq)]
enum Line {
    Pane(Pane),
    /// A well-formed pane whose `@cpd_view` this binary does not know (§6.3): a
    /// newer cpd made it, so it is skipped, not an error.
    UnknownView(String),
}

/// Logs an unknown `@cpd_view` value to stderr, once per value and process.
fn warn_unknown_view(view: &str) {
    static SEEN: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());
    if SEEN.lock().unwrap_or_else(|e| e.into_inner()).insert(view.to_owned()) {
        eprintln!("cpd: skipping tmux panes with the unknown @cpd_view {view:?}: a newer cpd made them");
    }
}

fn parse_pane(line: &str) -> Option<Line> {
    let fields: Vec<&str> = line.split('\t').collect();
    let [pane, window, session_name, window_name, dead, dead_status, pid, cpd_session, cpd_view] = fields[..] else {
        return None;
    };
    let pane = Pane {
        pane: PaneId::parse(pane)?,
        window: WindowId::parse(window)?,
        session_name: session_name.to_owned(),
        window_name: window_name.to_owned(),
        dead: match dead {
            "0" => false,
            "1" => true,
            _ => return None,
        },
        dead_status: if dead_status.is_empty() { None } else { Some(dead_status.parse().ok()?) },
        pid: if pid.is_empty() { None } else { Some(pid.parse().ok()?) },
        cpd_session: if cpd_session.is_empty() { None } else { Some(SessionId::parse(cpd_session).ok()?) },
        cpd_view: None,
    };
    if cpd_view.is_empty() {
        return Some(Line::Pane(pane));
    }
    match View::parse(cpd_view) {
        Some(view) => Some(Line::Pane(Pane { cpd_view: Some(view), ..pane })),
        None => Some(Line::UnknownView(cpd_view.to_owned())),
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_split_is_placed_by_its_flags() {
        assert_eq!(Split::Right.flags(), ["-h"]);
        assert_eq!(Split::Below.flags(), ["-v"]);
        assert_eq!(Split::Bottom.flags(), ["-v", "-f"]);
    }

    #[test]
    fn ids_have_their_sigil() {
        assert_eq!(PaneId::parse("%12").unwrap().as_str(), "%12");
        assert_eq!(WindowId::parse("@3").unwrap().to_string(), "@3");
        for bad in ["", "%", "12", "@1", "%1a", "% 1"] {
            assert!(PaneId::parse(bad).is_none(), "{bad:?}");
        }
        assert!(WindowId::parse("%1").is_none());
    }

    fn pane_of(line: &str) -> Pane {
        match parse_pane(line) {
            Some(Line::Pane(pane)) => pane,
            other => panic!("{line:?}: {other:?}"),
        }
    }

    #[test]
    fn parses_a_list_panes_line() {
        let live = pane_of("%4\t@2\tcpd\tmain\t0\t\t4242\t596d007f-aaaa\t");
        assert_eq!(live.pane.as_str(), "%4");
        assert_eq!(live.window.as_str(), "@2");
        assert_eq!(live.session_name, "cpd");
        assert_eq!(live.window_name, "main");
        assert!(!live.dead);
        assert_eq!(live.dead_status, None);
        assert_eq!(live.pid, Some(4242));
        assert_eq!(live.cpd_session.unwrap().as_str(), "596d007f-aaaa");
        assert_eq!(live.cpd_view, None);

        let dead = pane_of("%5\t@2\tcpd\tmain\t1\t3\t4243\t\tchat");
        assert!(dead.dead);
        assert_eq!(dead.dead_status, Some(3));
        assert_eq!(dead.cpd_view, Some(View::Chat));
        assert_eq!(dead.pid, Some(4243));
    }

    #[test]
    fn a_pane_whose_tty_closed_has_no_pid() {
        // tmux >= 3.8 prints `#{pane_pid}` empty once the pane's tty has
        // closed: dead, or dying (not yet reaped, so `#{pane_dead}` is 0).
        let dead = pane_of("%5\t@2\tcpd-work\twatch\t1\t7\t\t\t");
        assert_eq!((dead.dead, dead.dead_status, dead.pid), (true, Some(7), None));
        let dying = pane_of("%4\t@2\tcpd\tmain\t0\t\t\t\t");
        assert_eq!((dying.dead, dying.pid), (false, None));
        assert!(parse_pane("%4\t@2\tcpd\tmain\t1\t\tx\t\t").is_none());
    }

    #[test]
    fn refuses_a_malformed_line() {
        assert!(parse_pane("%4 @2 cpd main 0  4242  ").is_none());
        assert!(parse_pane("%4\t@2\tcpd\tmain\t2\t\t4242\t\t").is_none());
    }

    #[test]
    fn a_pane_with_an_unknown_view_is_a_line_to_skip_not_a_malformed_one() {
        assert_eq!(parse_pane("%4\t@2\tcpd\tmain\t0\t\t4242\t\tnope"), Some(Line::UnknownView("nope".into())));
        // Any other defect still fails.
        assert_eq!(parse_pane("%4\t@2\tcpd\tmain\t2\t\t4242\t\tnope"), None);
    }

    #[test]
    fn a_fresh_server_gets_its_options_before_the_first_pane_spawns() {
        let spawn = Spawn { cwd: None, env: BTreeMap::new(), argv: strings(&["cpd", "watch"]) };
        let new_session = strings(&[
            "new-session", "-d", "-s", "cpd-work", "-n", "watch", "-x", "215", "-y", "62", "-P", "-F", "#{window_id} #{pane_id}", "--",
            "cpd", "watch",
        ]);
        assert_eq!(new_session_args("cpd-work", "watch", &spawn, false), new_session);

        let fresh = new_session_args("cpd-work", "watch", &spawn, true);
        let commands: Vec<&[String]> = fresh.split(|a| a == ";").collect();
        let (last, options) = commands.split_last().unwrap();
        assert_eq!(*last, new_session.as_slice());
        let options: Vec<Vec<String>> = options.iter().map(|c| c.to_vec()).collect();
        let expected: Vec<Vec<String>> = SERVER_OPTIONS.iter().map(|o| strings(o)).collect();
        assert_eq!(options, expected);
    }

    #[test]
    fn every_call_has_the_deadline_but_display_popup() {
        assert_eq!(deadline_of(&strings(&["list-panes", "-a"])), Some(Duration::from_secs(10)));
        assert_eq!(deadline_of(&strings(&["kill-window", "-t", "@1"])), Some(Duration::from_secs(10)));
        assert_eq!(deadline_of(&strings(&["display-popup", "-E", "-T", "t", "cpd tui view legend"])), None);
    }

    #[test]
    fn every_call_names_the_socket_and_no_config_and_drops_tmux_vars() {
        let tmux = Tmux::new("cpdt");
        assert_eq!(tmux.argv(&strings(&["kill-server"])), ["tmux", "-L", "cpdt", "-f", "/dev/null", "kill-server"]);
        let cmd = tmux.command(&strings(&["kill-server"]));
        assert_eq!(cmd.get_program(), "tmux");
        let removed: Vec<_> = cmd.get_envs().filter(|(_, v)| v.is_none()).map(|(k, _)| k.to_owned()).collect();
        assert_eq!(removed, ["TMUX", "TMUX_PANE"]);
    }
}
