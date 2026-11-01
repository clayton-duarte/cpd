//! The fake tmux binary (docs/implementing.md, Tests, "The fake tmux"): builds an `Invocation` from the process
//! and hands it to `cpd::fakes::tmux::run`.

use cpd::fakes::tmux::{Invocation, run};
use std::io::Write;
use std::path::PathBuf;

fn main() {
    let argv: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let tmux_tmpdir = match std::env::var_os("TMUX_TMPDIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            eprintln!("error creating /tmp/tmux-<uid> (TMUX_TMPDIR is not set)");
            std::process::exit(1);
        }
    };
    let cwd = std::env::current_dir().unwrap_or_else(|e| {
        eprintln!("fake-tmux: no working directory: {e}");
        std::process::exit(1);
    });
    let out = run(&Invocation {
        argv,
        tmux_tmpdir,
        cwd,
        env: std::env::vars().collect(),
    });
    let _ = std::io::stdout().write_all(out.stdout.as_bytes());
    let _ = std::io::stderr().write_all(out.stderr.as_bytes());
    std::process::exit(out.code);
}
