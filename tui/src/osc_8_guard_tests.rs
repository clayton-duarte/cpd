//! Guard (decision pro-d60): the cockpit's links are ratatui cells, never OSC 8. No product line of the tui writes an OSC 8
//! sequence (`ESC ]` then `8;`), written in source as `\x1b]8;` or `\u{1b}]8;`. Test files (`*_tests.rs`) and `#[cfg(test)]`
//! items are not product; there is no allowlist. Red while `link::link` writes one.

use std::path::{Path, PathBuf};

/// The non-test `.rs` files under `dir` (not `*_tests.rs`, `tests.rs`, nor a `tests` directory).
fn product_files(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display())) {
        let path = entry.expect("a directory entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_owned();
        if path.is_dir() {
            if name != "tests" {
                product_files(&path, found);
            }
        } else if name.ends_with(".rs") && name != "tests.rs" && !name.ends_with("_tests.rs") {
            found.push(path);
        }
    }
}

/// The product lines of `text` with their numbers: what is outside the `#[cfg(test)]` items, comments cut.
fn product_lines(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut kept = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim() == "#[cfg(test)]" {
            // Skip the attributes, then the item: one line ending in `;`, or up to the closing brace.
            i += 1;
            while i < lines.len() && lines[i].trim().starts_with("#[") {
                i += 1;
            }
            let mut depth = 0i32;
            while i < lines.len() {
                let item = lines[i];
                depth += item.matches('{').count() as i32 - item.matches('}').count() as i32;
                i += 1;
                if depth <= 0 && (item.trim_end().ends_with(';') || item.trim_end().ends_with('}'))
                {
                    break;
                }
            }
            continue;
        }
        let code = lines[i].split("//").next().unwrap_or_default().trim();
        if !code.is_empty() {
            kept.push((i + 1, code.to_owned()));
        }
        i += 1;
    }
    kept
}

#[test]
fn no_product_file_of_the_tui_writes_an_osc_8_sequence() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    product_files(&src, &mut files);
    assert!(
        files.len() > 20,
        "the walk found the tui's product files: {}",
        files.len()
    );
    let mut offenders = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(&src)
            .expect("under src")
            .to_string_lossy()
            .into_owned();
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        for (number, code) in product_lines(&text) {
            if code.contains("\\x1b]8;")
                || code.contains("\\u{1b}]8;")
                || code.contains('\u{1b}') && code.contains("]8;")
            {
                offenders.push(format!("tui/src/{rel}:{number}: {code}"));
            }
        }
    }
    offenders.sort();
    assert!(
        offenders.is_empty(),
        "these lines write an OSC 8 sequence; draw a `link::Link` instead:\n{}",
        offenders.join("\n")
    );
}
