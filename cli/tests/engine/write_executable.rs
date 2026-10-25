//! `write_executable` over a script that is already running (ETXTBSY). The engine
//! runs a fake `bin/tmux` on every tick while a test rewrites it with
//! [`write_executable`]: opening a file for writing while a process executes it
//! fails with `Text file busy`.

use crate::common;

use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;

use common::{TestEnv, spawn, write_executable};

#[test]
fn write_executable_over_a_running_script_does_not_fail() {
    let env = TestEnv::new();
    let path = env.bin().join("running");
    write_executable(&path, "#!/bin/sh\necho ready\nread line\necho \"got $line\"\n");

    let mut cmd = env.command(&path);
    let mut child = spawn(cmd.stdin(Stdio::piped()).stdout(Stdio::piped())).expect("spawn the script");

    let mut stdout = BufReader::new(child.stdout.take().expect("stdout is piped"));
    let mut line = String::new();
    stdout.read_line(&mut line).expect("read the script's first line");
    assert_eq!(line, "ready\n", "the script is running before the rewrite");

    // The rewrite the engine's next tick forces: it must not open a running script
    // for writing in place, or it fails with ETXTBSY.
    write_executable(&path, "#!/bin/sh\necho rewrote\n");

    let mut stdin = child.stdin.take().expect("stdin is piped");
    writeln!(stdin, "stop").expect("release the script's read");
    drop(stdin);
    let status = child.wait().expect("wait for the script");
    assert!(status.success(), "the script exits cleanly: {status}");
}
