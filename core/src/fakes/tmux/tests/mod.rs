mod chain;
mod create;
mod format;
mod hooks;
mod kill;
mod options;
mod pane_io;
mod server;
mod target;

use super::{Invocation, Outcome, State, run};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

/// A temp TMUX_TMPDIR per test; every call goes through `run`.
pub struct T {
    pub dir: tempfile::TempDir,
    pub env: Vec<(String, String)>,
}

impl T {
    pub fn new() -> T {
        T {
            dir: tempfile::tempdir().unwrap(),
            env: vec![("FAKE_TMUX_SERVER_PID".into(), "4242".into())],
        }
    }
    pub fn path(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }
    pub fn uid(&self) -> u32 {
        std::fs::metadata(self.dir.path()).unwrap().uid()
    }
    /// Runs `tmux -u -L <socket> -f /dev/null <args>`.
    pub fn on(&self, socket: &str, args: &[&str]) -> Outcome {
        let mut argv: Vec<String> = ["-u", "-L", socket, "-f", "/dev/null"]
            .map(String::from)
            .to_vec();
        argv.extend(args.iter().map(|a| a.to_string()));
        run(&Invocation {
            argv,
            tmux_tmpdir: self.path(),
            cwd: self.path(),
            env: self.env.clone(),
        })
    }
    /// Socket `t`.
    pub fn t(&self, args: &[&str]) -> Outcome {
        self.on("t", args)
    }
    /// Runs a call and requires exit 0; returns stdout.
    pub fn ok(&self, args: &[&str]) -> String {
        let o = self.t(args);
        assert_eq!(o.code, 0, "{args:?}: {o:?}");
        o.stdout
    }
    pub fn state(&self) -> Option<State> {
        State::open(self.dir.path(), "t")
    }
    /// A server with session `s`, window `w` (`@0`, pane `%0`).
    pub fn server(&self) {
        self.ok(&["new-session", "-d", "-s", "s", "-n", "w"]);
    }
    pub fn fail(&self, args: &[&str], code: i32, stderr: &str) {
        let o = self.t(args);
        assert_eq!((o.code, o.stderr.trim_end()), (code, stderr), "{args:?}");
    }
}
