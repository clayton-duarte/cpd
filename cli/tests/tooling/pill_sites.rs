//! Guards against a hand-assembled pill (docs/research/pill-spec.md, section 5, "Parity test"): the caps and reverse
//! video live in the spec file and the three adapters only. Red until the `pill-sites-*` Jobs migrate the sites.

use std::path::{Path, PathBuf};

const DIRS: [&str; 7] = [
    "tui/src",
    "core/src",
    "engine/src",
    "cli/src",
    "pi/lib",
    "pi/compact-tools",
    "pi/ask-user-question",
];

/// The repo root: `cli/..`.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn is_source(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "rs" || e == "ts")
}

/// Test code: a name with `test`, a `tests` directory, or a `*_colours.rs` / `pill_*` module the tui declares under
/// `#[cfg(test)]` in its lib.rs.
fn is_test_file(path: &Path) -> bool {
    let name = path.file_name().unwrap().to_string_lossy();
    path.components()
        .any(|c| c.as_os_str() == "tests" || c.as_os_str() == "test")
        || name.contains("test")
        || name.ends_with("_colours.rs")
        || name.starts_with("pill_")
}

/// Appends every non-test source file under `dir`, sorted.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.map(|e| e.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "node_modules") {
                continue;
            }
            sources(&path, out);
        } else if is_source(&path) && !is_test_file(&path) {
            out.push(path);
        }
    }
}

/// The lines of product code, as `(number, text)`: a `.rs` file stops at its first column-0 `#[cfg(test)]` (an inline
/// test module closes the file; the `#[cfg(test)]` blocks of a product file cannot be told apart by line otherwise),
/// and comment-only lines are skipped (a doc comment may name a glyph).
fn product_lines(path: &Path) -> Vec<(usize, String)> {
    let text = std::fs::read_to_string(path).unwrap();
    let rs = path.extension().is_some_and(|e| e == "rs");
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if rs && line.starts_with("#[cfg(test)]") {
            break;
        }
        let t = line.trim_start();
        if t.starts_with("//") || t.starts_with("/*") || t.starts_with("* ") || t == "*" {
            continue;
        }
        out.push((n + 1, line.trim().to_string()));
    }
    out
}

/// `path:line: text` for each product line of `dirs` holding one of `forbidden` (matched lower-case), except in
/// `allowed` files (paths relative to the root, with the reason in the caller).
fn offenders(forbidden: &[&str], allowed: &[&str]) -> Vec<String> {
    let root = root();
    let mut files = Vec::new();
    for dir in DIRS {
        sources(&root.join(dir), &mut files);
    }
    let mut found = Vec::new();
    for path in files {
        let rel = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .to_string();
        if allowed.contains(&rel.as_str()) {
            continue;
        }
        for (n, line) in product_lines(&path) {
            let lower = line.to_lowercase();
            if forbidden.iter().any(|f| lower.contains(f)) {
                found.push(format!("{rel}:{n}: {line}"));
            }
        }
    }
    found
}

#[test]
fn no_pill_cap_glyph_in_non_test_source_outside_the_spec_and_the_adapters() {
    // The spelling is built from parts so that this file does not name it as a glyph or an escape.
    let forbidden = ["\u{e0b6}", "\u{e0b4}", "e0b6", "e0b4"];
    // The spec file is `pi/lib/pill-spec.json` (not a scanned extension). The adapters (`tui/src/pill.rs`,
    // `core/src/host/tmux/pill.rs`, `pi/lib/pill.ts`) read the caps from it and spell none. No other exception.
    let allowed = [
        // Test-only modules (declared under `#[cfg(test)]`) whose names do not say so: they spell the caps to assert them.
        "tui/src/widgets/inbox_indent.rs",
        "tui/src/tree/look.rs",
        "core/src/host/tmux/no_inbox.rs",
        "core/src/host/tmux/palette.rs",
    ];
    let found = offenders(&forbidden, &allowed);
    assert!(
        found.is_empty(),
        "a hand-assembled pill cap outside the spec:\n{}",
        found.join("\n")
    );
}

#[test]
fn no_reverse_video_pill_outside_the_three_adapters() {
    let forbidden = ["\x1b[7m", "#[reverse]", "\\x1b[7m", "\\u001b[7m"];
    let allowed = [
        "tui/src/palette.rs",
        "core/src/host/tmux/pill.rs",
        "pi/lib/pill.ts",
        // A text cursor, not a pill: the character under the cursor is drawn in reverse video.
        "pi/ask-user-question/view/components/inline-input.ts",
    ];
    let found = offenders(&forbidden, &allowed);
    assert!(
        found.is_empty(),
        "reverse video outside the three adapters:\n{}",
        found.join("\n")
    );
}
