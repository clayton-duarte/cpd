//! No architecture note holds a runnable `cpd context show --record` command: Contexts are machine-local
//! (docs/contract.md §4.6). Reads repo-authored files only; no process, no sleep.

use std::fs;
use std::path::PathBuf;

#[test]
fn architecture_notes_name_no_context_show_record_command() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../docs/architecture");
    let mut offenders = Vec::new();
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap();
        let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if flat.contains("cpd context show --record") {
            offenders.push(path.file_name().unwrap().to_string_lossy().into_owned());
        }
    }
    offenders.sort();
    assert!(
        offenders.is_empty(),
        "architecture notes name `cpd context show --record`: {offenders:?}"
    );
}
