//! `write_executable` and `copy_executable` must never open an executable that is in
//! use for writing: they must write a temp file and rename it over the target.
//!
//! Inferred, unverified: the audit1 Gate flake (`cpd::engine watch::a_tmux_failure_of_the_workers_step_
//! is_one_error_and_the_engine_goes_on`, `write script: the busy-executable error`) is the same class: a file
//! opened for writing while it is being executed, in that case the execve window of the engine's
//! `bin/tmux`. A *running* script is not text-busy (the kernel releases a script's write denial once
//! its interpreter has exec'd), so that window has no deterministic trigger; a running ELF holds the
//! denial for its whole life, which is why this test runs an authored `cpd` itself. It pins the
//! contract the fix gives: no in-place open of an executable in use, a temp file then a rename.

#[cfg(target_os = "linux")]
use crate::common;

#[cfg(target_os = "linux")]
use std::fs;
#[cfg(target_os = "linux")]
use std::process::{Child, Stdio};

#[cfg(target_os = "linux")]
use common::{CPD, TestEnv, copy_executable, spawn, write_executable};

#[cfg(target_os = "linux")]
/// The running copy of `cpd`: killed and reaped on drop, so no process is left,
/// whether the test passes or panics.
struct Running(Child);

#[cfg(target_os = "linux")]
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// Opening a running executable for writing is refused by the Linux kernel (the busy-executable error); macOS lets the open succeed. The test describes the Linux behaviour.
#[cfg(target_os = "linux")]
#[test]
fn write_executable_over_a_running_script_does_not_fail() {
    let env = TestEnv::new();
    env.write_config("version = 1\nmachine = \"m1\"\n");
    // A Lead Session, so `cpd secret create` passes `identify` and `rules::check`
    // (vault `cpd`) and reaches `read_value`, which blocks on stdin to EOF.
    env.sql(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'ctx', 't', 't');\
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)\
         VALUES ('s1', 'lead', 'ctx', NULL, 'Lead', '/', 'S3', 'idle', 't', 't');",
    );
    let path = env.bin().join("running");
    copy_executable(CPD, &path);

    // stdin is a pipe this test holds and never writes: the child blocks in read_value,
    // so it cannot exit while the ELF is text-busy. PATH is the env's bin alone, so no
    // real `op` can be reached.
    let mut cmd = env.command(&path);
    cmd.env("PATH", env.bin())
        .env("CPD_SESSION", "s1")
        .args([
            "secret", "create", "item", "--vault", "cpd", "--field", "field",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = Running(spawn(&mut cmd).expect("spawn the copied cpd"));

    // The precondition: a child that died early (identify or rules failed) would make
    // the rewrite pass for the wrong reason. A running ELF fails an open for writing
    // with the kernel's busy-executable error (26) for as long as it lives.
    let busy = fs::OpenOptions::new().write(true).open(&path);
    assert_eq!(
        busy.as_ref().err().and_then(|e| e.raw_os_error()),
        Some(26),
        "the running cpd is text-busy before the rewrite; child: {:?}",
        child.0.try_wait()
    );
    drop(busy);

    write_executable(&path, "#!/bin/sh\necho rewrote\n");
}
