//! What the in-memory tests share: a temp `TMUX_TMPDIR` and calls through one `FakeTmux`.

use super::{FakeTmux, Invocation, Outcome};
use crate::host::runner::Runner;
use std::path::PathBuf;
use std::sync::Arc;

pub struct M {
    pub dir: tempfile::TempDir,
    pub tmux: Arc<FakeTmux>,
}

impl M {
    pub fn new() -> M {
        M {
            dir: tempfile::tempdir().unwrap(),
            tmux: Arc::new(FakeTmux::new()),
        }
    }
    /// A `FakeTmux` whose `run-shell` calls `runner`.
    pub fn with_runner(runner: Arc<dyn Runner>) -> M {
        M {
            dir: tempfile::tempdir().unwrap(),
            tmux: Arc::new(FakeTmux::new().with_runner(runner)),
        }
    }
    pub fn path(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }
    /// `tmux -u -L <socket> -f /dev/null <args>`, as `Tmux::command` builds it.
    pub fn argv(socket: &str, args: &[&str]) -> Vec<String> {
        let mut argv: Vec<String> = ["-u", "-L", socket, "-f", "/dev/null"]
            .map(String::from)
            .to_vec();
        argv.extend(args.iter().map(|a| a.to_string()));
        argv
    }
    pub fn invocation(&self, argv: Vec<String>) -> Invocation {
        Invocation {
            argv,
            tmux_tmpdir: self.path(),
            cwd: self.path(),
            env: vec![("PATH".into(), "/usr/bin".into())],
        }
    }
    pub fn on(&self, socket: &str, args: &[&str]) -> Outcome {
        self.tmux.run(&self.invocation(M::argv(socket, args)))
    }
    /// Socket `t`.
    pub fn t(&self, args: &[&str]) -> Outcome {
        self.on("t", args)
    }
    /// A call on socket `t` that must exit 0; its stdout.
    pub fn ok(&self, args: &[&str]) -> String {
        let o = self.t(args);
        assert_eq!(o.code, 0, "{args:?}: {o:?}");
        o.stdout
    }
    /// A server on socket `t` with session `s`, window `w` (`@0`, pane `%0`).
    pub fn server(&self) {
        self.ok(&["new-session", "-d", "-s", "s", "-n", "w"]);
    }
}
