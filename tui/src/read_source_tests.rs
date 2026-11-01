//! A source check of the read model, `tui/src/read/` (commands.md §17): it starts no process and writes nothing. It lives
//! beside the directory, not in it, because it names the forbidden strings and the scan reads every file there. The
//! scan reads the product files: a `*_tests.rs` file (a `#[cfg(test)]` module) builds its fixture store by writing to it.

#[test]
fn the_read_model_spawns_nothing_and_has_no_write_statement() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/read");
    let mut seen = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().ends_with("_tests.rs"))
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        // The module docs and comments may name these; code may not.
        let code: String = text
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for forbidden in [
            "std::process",
            "process::Command",
            "Command::new",
            "crate::host",
            "crate::git",
            ".write(",
            "INSERT ",
            "UPDATE ",
            "DELETE ",
            "execute",
        ] {
            assert!(
                !code.contains(forbidden),
                "{} contains {forbidden:?}",
                path.display()
            );
        }
        seen += 1;
    }
    assert!(seen >= 5);
}
