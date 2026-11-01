//! The source scan the guard tests share: every `*.rs` under `tui/src`, its path relative to `tui/src`, and its
//! product text only (test files left out; each file cut at its inline test module, so the tests a file holds last
//! never hide the product code above).

use std::path::Path;

/// The product text of one source file: `text` cut at its first inline test module, a `#[cfg(test)]` line whose next
/// line is `mod <name> {`. A `#[cfg(test)] mod <name>;` declaration (whose next line ends in `;`) hides nothing:
/// scanning continues past it.
pub(crate) fn product_text(text: &str) -> String {
    let mut out = String::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line.trim() == "#[cfg(test)]"
            && let Some(next) = lines.clone().next()
        {
            let next = next.trim();
            if next.starts_with("mod ") && next.ends_with('{') {
                break;
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Every product source under `tui/src`: `(path relative to tui/src, product text)`, sorted by path. Files whose
/// name contains `tests` are the tests themselves and are left out.
pub(crate) fn product_sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = Vec::new();
    walk(&root, &root, &mut sources);
    sources.sort();
    sources
}

fn walk(root: &Path, dir: &Path, sources: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk(root, &path, sources);
        } else if path.extension().is_some_and(|e| e == "rs") {
            if path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("tests")
            {
                continue;
            }
            let file = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let text = std::fs::read_to_string(&path).unwrap();
            sources.push((file, product_text(&text)));
        }
    }
}

/// The helper's own test, on literals: an inline test module cuts the product text, a `mod <name>;` declaration
/// cuts nothing.
#[test]
fn product_text_stops_at_an_inline_test_module_only() {
    let inline = "fn product() {}\n#[cfg(test)]\nmod tests {\n    fn hidden() {}\n}\n";
    assert_eq!(product_text(inline), "fn product() {}\n");

    let declaration = "fn product() {}\n#[cfg(test)]\nmod tests;\nfn more_product() {}\n";
    assert_eq!(product_text(declaration), declaration);
}
