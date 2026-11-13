//! Integration test for WebUI argv parity against the cpd CLI parser.

use std::path::{Path, PathBuf};
use std::process::Command;

use clap::Parser;
use clap::error::ErrorKind;

use cpd::cli::Cli;

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn load_webui_argvs() -> Vec<Vec<String>> {
    let script = format!(
        r#"import("{}/web/src/server/cpd-argv.ts").then(m => process.stdout.write(m.generateArgvFixturesJson()))"#,
        repo_root().display()
    );
    let output = Command::new("node")
        .args(["--no-warnings", "-e", &script])
        .output()
        .expect("failed to execute node to generate argv fixtures");

    assert!(
        output.status.success(),
        "node failed to generate argv fixtures: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    serde_json::from_slice(&output.stdout).expect("failed to deserialize argv fixtures JSON")
}

#[test]
fn all_webui_argvs_parse_in_cli() {
    let argvs = load_webui_argvs();
    assert!(
        !argvs.is_empty(),
        "expected non-empty WebUI argv list from allCpdCommands()"
    );
    assert!(
        argvs.len() >= 50,
        "expected at least 50 WebUI commands, got {}",
        argvs.len()
    );

    let mut failures = Vec::new();
    for argv in &argvs {
        let full_argv = std::iter::once("cpd").chain(argv.iter().map(String::as_str));
        match Cli::try_parse_from(full_argv) {
            Ok(_) => {}
            Err(err)
                if matches!(
                    err.kind(),
                    ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
                ) => {}
            Err(err) => {
                failures.push((argv.clone(), err.to_string()));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "The following WebUI argvs failed to parse in cpd CLI:\n{}",
        failures
            .iter()
            .map(|(a, err)| format!("  argv: {:?}\n  error: {}\n", a, err.trim()))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
