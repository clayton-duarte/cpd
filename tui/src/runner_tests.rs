//! The tui's child processes on a [`FakeRunner`]: `actions::call_with` (`cpd <argv>`), `host::run_with` and the macOS reads
//! of the Dispatch column (`ioreg`, `sysctl`, `ps`, `vm_stat`), and `hostname_with`.

use std::io::ErrorKind;
use std::path::Path;

use serde_json::json;

use crate::tree::actions::{Action, Failure, call_with};
use crate::widgets::host::{Usage, macos_usage_with, run_with};
use crate::widgets::hostname_with;
use cpd_core::clock::ManualClock;
use cpd_core::host::runner::{FakeRunner, Reply};

fn runner() -> FakeRunner {
    FakeRunner::new(ManualClock::new())
}

// ---- actions::call_with -------------------------------------------------------------------------------------------

#[test]
fn call_with_runs_cpd_with_the_argv_of_the_action_and_no_environment_of_its_own() {
    let fake = runner().on("cpd", |_| true, Reply::ok(&json!({"ok": true}).to_string()));
    call_with(&fake, &Action::Park).unwrap();
    let calls = fake.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        (calls[0].program.as_str(), calls[0].args.as_slice()),
        ("cpd", ["session".to_owned(), "park".to_owned()].as_slice())
    );
    assert!(calls[0].envs.is_empty(), "{:?}", calls[0].envs);
    assert_eq!(
        (
            calls[0].cwd.as_deref(),
            calls[0].stdin.as_ref(),
            calls[0].stderr_to.as_ref()
        ),
        (None, None, None)
    );
}

#[test]
fn call_with_returns_the_printed_reply_when_ok_is_true() {
    let reply = json!({"ok": true, "session": "s1"});
    let fake = runner().on("cpd", |_| true, Reply::ok(&reply.to_string()));
    assert_eq!(call_with(&fake, &Action::Park), Ok(reply));
}

#[test]
fn call_with_a_printed_refusal_is_refused_with_its_code_and_message() {
    let printed = json!({"ok": false, "error": {"code": "no_cockpit", "message": "No cockpit."}});
    let fake = runner().on("cpd", |_| true, Reply::exit(1, &printed.to_string(), ""));
    assert_eq!(
        call_with(&fake, &Action::Park),
        Err(Failure::Refused {
            code: "no_cockpit".into(),
            message: "No cockpit.".into()
        })
    );
}

#[test]
fn call_with_a_refusal_without_an_error_is_the_default_code_and_message() {
    let fake = runner().on(
        "cpd",
        |_| true,
        Reply::ok(&json!({"ok": false}).to_string()),
    );
    assert_eq!(
        call_with(&fake, &Action::Park),
        Err(Failure::Refused {
            code: "error".into(),
            message: "failed".into()
        })
    );
}

#[test]
fn call_with_no_json_is_broken_and_says_the_command_the_exit_and_the_trimmed_stderr() {
    let fake = runner().on("cpd", |_| true, Reply::exit(2, "not json", "  boom \n"));
    let Err(Failure::Broken(why)) = call_with(&fake, &Action::Park) else {
        panic!("expected Broken")
    };
    assert!(
        why.starts_with("cpd session park printed no JSON (exit "),
        "{why}"
    );
    assert!(
        why.contains("exit code: 2") && why.ends_with("): boom"),
        "{why}"
    );
}

#[test]
fn call_with_a_program_that_cannot_start_is_broken_with_cannot_run() {
    let fake = runner().on("cpd", |_| true, Reply::io(ErrorKind::NotFound));
    let Err(Failure::Broken(why)) = call_with(&fake, &Action::Park) else {
        panic!("expected Broken")
    };
    assert!(why.starts_with("cannot run cpd: "), "{why}");
}

// ---- widgets::host::run_with --------------------------------------------------------------------------------------

#[test]
fn run_with_is_the_stdout_of_a_program_that_exits_0() {
    let fake = runner().on("sysctl", |_| true, Reply::ok("8\n"));
    assert_eq!(
        run_with(&fake, "sysctl", &["-n", "hw.ncpu"]).as_deref(),
        Some("8\n")
    );
    let calls = fake.calls();
    assert_eq!(
        (calls[0].program.as_str(), calls[0].args.as_slice()),
        ("sysctl", ["-n".to_owned(), "hw.ncpu".to_owned()].as_slice())
    );
}

#[test]
fn run_with_a_program_that_exits_non_zero_is_none() {
    let fake = runner().on("sysctl", |_| true, Reply::exit(1, "8\n", "err"));
    assert_eq!(run_with(&fake, "sysctl", &["-n", "hw.ncpu"]), None);
}

#[test]
fn run_with_a_program_that_cannot_start_is_none() {
    let fake = runner().on("sysctl", |_| true, Reply::io(ErrorKind::NotFound));
    assert_eq!(run_with(&fake, "sysctl", &["-n", "hw.ncpu"]), None);
}

// ---- the macOS reads of the Dispatch column -------------------------------------------------------------------------

const IOREG_ONE: &str = "+-o AGXAcceleratorG13X  <class AGXAcceleratorG13X, id 0x100000385, registered, matched, active, busy 0 (0 ms), retain 28>\n\
    {\n\
    \x20 \"IOClass\" = \"AGXAcceleratorG13X\"\n\
    \x20 \"PerformanceStatistics\" = {\"Alloc system memory\"=50167808,\"Device Utilization %\"=37,\"Renderer Utilization %\"=21,\"Tiler Utilization %\"=30}\n\
    }\n\
    \n";
const VM_STAT: &str = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
    Pages free:                               12345.\n\
    Pages active:                            100000.\n\
    Pages inactive:                           90000.\n\
    Pages speculative:                         1000.\n\
    Pages wired down:                         50000.\n\
    Pages occupied by compressor:             10000.\n";
const PS: &str = "  12.5\n   0.0\n 100.0\n  37.5\n";

fn mac() -> FakeRunner {
    runner()
        .on(
            "ioreg",
            |c| c.args == ["-r", "-d", "1", "-w", "0", "-c", "IOAccelerator"],
            Reply::ok(IOREG_ONE),
        )
        .on("sysctl", |c| c.args == ["-n", "hw.ncpu"], Reply::ok("4\n"))
        .on(
            "sysctl",
            |c| c.args == ["-n", "hw.memsize"],
            Reply::ok("8589934592\n"),
        )
        .on("ps", |c| c.args == ["-A", "-o", "%cpu="], Reply::ok(PS))
        .on("vm_stat", |c| c.args.is_empty(), Reply::ok(VM_STAT))
}

#[test]
fn the_macos_usage_is_parsed_from_the_replies_of_ioreg_sysctl_ps_and_vm_stat() {
    // cpu: 150 / 4 = 37.5 -> 38; memory: 160000 pages * 16384 of 8 GiB -> 31
    assert_eq!(
        macos_usage_with(&mac()),
        Usage {
            cpu: Some(38),
            gpus: vec![Some(37)],
            mem: Some(31)
        }
    );
}

#[test]
fn the_macos_usage_asks_each_tool_with_its_arguments_and_nothing_unscripted() {
    let fake = mac();
    macos_usage_with(&fake);
    assert!(fake.unscripted().is_empty(), "{:?}", fake.unscripted());
    let mut programs: Vec<String> = fake.calls().iter().map(|c| c.program.clone()).collect();
    programs.sort();
    assert_eq!(programs, ["ioreg", "ps", "sysctl", "sysctl", "vm_stat"]);
}

#[test]
fn the_macos_usage_has_no_gpu_when_ioreg_fails_and_keeps_the_rest() {
    let failing = runner()
        .on("ioreg", |_| true, Reply::io(ErrorKind::NotFound))
        .on("sysctl", |c| c.args == ["-n", "hw.ncpu"], Reply::ok("4\n"))
        .on(
            "sysctl",
            |c| c.args == ["-n", "hw.memsize"],
            Reply::ok("8589934592\n"),
        )
        .on("ps", |_| true, Reply::ok(PS))
        .on("vm_stat", |_| true, Reply::ok(VM_STAT));
    assert_eq!(
        macos_usage_with(&failing),
        Usage {
            cpu: Some(38),
            gpus: vec![],
            mem: Some(31)
        }
    );
}

#[test]
fn the_macos_cpu_is_none_when_sysctl_fails_and_the_memory_is_none_when_vm_stat_fails() {
    let fake = runner()
        .on("ioreg", |_| true, Reply::ok(IOREG_ONE))
        .on("sysctl", |_| true, Reply::exit(1, "", "no"))
        .on("ps", |_| true, Reply::ok(PS))
        .on("vm_stat", |_| true, Reply::ok(VM_STAT));
    assert_eq!(
        macos_usage_with(&fake),
        Usage {
            cpu: None,
            gpus: vec![Some(37)],
            mem: None
        }
    );
}

// ---- widgets::hostname_with ---------------------------------------------------------------------------------------

const NO_FILE: &str = "/no-such-dir/hostname";

#[test]
fn hostname_with_trims_the_reply_of_the_hostname_program() {
    let fake = runner().on("hostname", |_| true, Reply::ok("  mac-mini.local \n"));
    assert_eq!(
        hostname_with(&fake, Path::new(NO_FILE), None),
        "mac-mini.local"
    );
    assert_eq!(fake.calls().len(), 1);
    assert!(fake.calls()[0].args.is_empty());
}

#[test]
fn hostname_with_a_failing_program_is_a_question_mark() {
    let fake = runner().on("hostname", |_| true, Reply::exit(1, "x\n", ""));
    assert_eq!(hostname_with(&fake, Path::new(NO_FILE), None), "?");
}

#[test]
fn hostname_with_a_program_that_cannot_start_is_a_question_mark() {
    let fake = runner().on("hostname", |_| true, Reply::io(ErrorKind::NotFound));
    assert_eq!(hostname_with(&fake, Path::new(NO_FILE), None), "?");
}

#[test]
fn hostname_with_a_blank_reply_is_a_question_mark() {
    let fake = runner().on("hostname", |_| true, Reply::ok(" \n"));
    assert_eq!(hostname_with(&fake, Path::new(NO_FILE), None), "?");
}

#[test]
fn hostname_with_the_environment_name_asks_no_process() {
    let fake = runner();
    assert_eq!(
        hostname_with(&fake, Path::new(NO_FILE), Some(" box \n")),
        "box"
    );
    assert!(fake.calls().is_empty());
}

#[test]
fn hostname_with_a_blank_environment_name_falls_through_to_the_program() {
    let fake = runner().on("hostname", |_| true, Reply::ok("real\n"));
    assert_eq!(hostname_with(&fake, Path::new(NO_FILE), Some("  ")), "real");
}

#[test]
fn hostname_with_the_kernel_file_asks_no_process() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("hostname");
    std::fs::write(&file, "kernel-name\nsecond line\n").unwrap();
    let fake = runner();
    assert_eq!(hostname_with(&fake, &file, Some("env")), "kernel-name");
    assert!(fake.calls().is_empty());
}
