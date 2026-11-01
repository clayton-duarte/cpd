//! `cpd::run_with` (docs/implementing.md, "Ports, Spawn and run_with"): the whole root command tree in the test
//! process, on fake ports, with no process started.

use std::ffi::OsString;
use std::process::ExitCode;

use cpd_core::ports::Fakes;

use crate::common::fx::Fx;

fn argv(words: &[&str]) -> Vec<OsString> {
    words.iter().map(OsString::from).collect()
}

#[test]
fn run_with_version_succeeds_and_writes_the_version_json_with_no_process() {
    // `--version` reads `$CPD_HOME` (the `store` field): `Fx` points it at a temporary home, and the real one is never read.
    let _fx = Fx::new();
    let fakes = Fakes::new();
    let mut out = Vec::new();

    let code = cpd::run_with(&fakes.ports, argv(&["cpd", "--version"]), &mut out);

    assert_eq!(code, ExitCode::SUCCESS);
    let text = String::from_utf8(out).expect("the output is UTF-8");
    assert_eq!(
        text.lines().count(),
        1,
        "one JSON value on one line: {text:?}"
    );
    let json: serde_json::Value = serde_json::from_str(&text).expect("the output is JSON");
    assert_eq!(json["ok"], true);
    assert_eq!(json["version"], cpd::cli::version());
    assert_eq!(json["protocol"], cpd_core::events::PROTOCOL);
    assert!(fakes.runner.calls().is_empty(), "--version runs no program");
    assert!(
        fakes.spawn.launches().is_empty(),
        "--version spawns nothing"
    );
}
