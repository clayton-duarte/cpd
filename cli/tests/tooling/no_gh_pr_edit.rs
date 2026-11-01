//! Product code never runs `gh pr edit` (hs-n569): it fails on repos with the deprecated classic Projects. The body is set
//! through `gh api -X PATCH`, like the title. A text scan of the product sources: no process runs.

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

/// True when the code of `text` (comment lines dropped, whitespace removed) holds the argv literal of `gh pr edit`.
fn runs_gh_pr_edit(text: &str) -> bool {
    let code: String = text
        .lines()
        .filter(|l| !l.trim().starts_with("//"))
        .flat_map(|l| l.chars())
        .filter(|c| !c.is_whitespace())
        .collect();
    code.contains(concat!("\"pr\",", "\"edit\""))
}

#[test]
fn no_product_code_runs_gh_pr_edit() {
    // Control: a broken scanner cannot pass.
    assert!(runs_gh_pr_edit("let args = [\"pr\", \"edit\", &n];"));
    assert!(runs_gh_pr_edit("let args = [\"pr\",\n    \"edit\", &n];"));
    assert!(!runs_gh_pr_edit("/// Not gh pr edit: [\"pr\", \"edit\"]"));

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for dir in ["core/src", "engine/src", "cli/src", "tui/src"] {
        rust_files(&root.join(dir), &mut files);
    }
    assert!(files.len() > 100, "scanned only {} files", files.len());
    let bad: Vec<String> = files
        .iter()
        .filter(|f| runs_gh_pr_edit(&fs::read_to_string(f).unwrap()))
        .map(|f| f.strip_prefix(&root).unwrap().display().to_string())
        .collect();
    assert!(
        bad.is_empty(),
        "product code runs `gh pr edit`:\n{}",
        bad.join("\n")
    );
}
