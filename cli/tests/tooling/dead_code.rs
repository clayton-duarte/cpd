//! Dead code is deleted, not silenced (finding commands-G1): an `allow(dead_code)` stays only with a reason on its
//! own line, and that reason is not a Job that finished long ago.

use std::fs;
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn every_allow_dead_code_states_a_live_reason() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for dir in ["core/src", "tui/src", "cli/src", "cli/tests"] {
        rust_files(&root.join(dir), &mut files);
    }
    let own = Path::new(file!()).file_name().unwrap();
    let mut bad = Vec::new();
    for file in files.iter().filter(|f| f.file_name() != Some(own)) {
        for (n, line) in fs::read_to_string(file).unwrap().lines().enumerate() {
            if !line.contains("allow(dead_code)") {
                continue;
            }
            let reason = line.split_once("//").map(|(_, r)| r.trim()).unwrap_or("");
            if reason.is_empty() || reason.contains("Job `") {
                bad.push(format!(
                    "{}:{}: {}",
                    file.strip_prefix(&root).unwrap().display(),
                    n + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "allow(dead_code) without a live reason:\n{}",
        bad.join("\n")
    );
}
