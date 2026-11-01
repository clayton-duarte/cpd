//! `no_view_keeps_a_note` (hs-d469, commands.md §17 "No notes"): no view shows a transient note line (`already in focus`,
//! `copied <url>: …`, `engine down 60 s+: Managers told`, `fold state reset: …`, `no review page yet`, the result of an
//! action). What the User must see is a `text` dialog; the rest goes to the view's log, `run/tui/<view>.log`.
//!
//! The guard reads the code of every file under `tui/src/` that is not a test and fails on the word `note` as an
//! identifier (a field, a parameter, a variant, a constant: `self.note`, `Action::Note`, `DOWN_NOTE`, `note: &str`),
//! which is what a note is made of. It pins the mechanism, not a colour: `palette::note()` and `Ui::Note` are left out
//! (rule (c): they stay while a lasting text uses them, such as a view's error line or `no Plans yet`, rule (b)); the
//! frames a view draws are pinned by the tests of each view (`tree::view`, `plan`, `widgets::column`).

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// A file or a directory (relative to `tui/src/`) that may keep a `note`, with why. A new one needs a reviewer to add it.
    const EXEMPT: [(&str, &str); 2] = [
        (
            "palette.rs",
            "defines `note()` and `Ui::Note`, which rule (c) keeps while something else uses them",
        ),
        ("no_notes.rs", "this guard names the word it looks for"),
    ];

    /// The words of an identifier: `DOWN_NOTE` is `down`, `note`; `NoteStyle` is `note`, `style`.
    fn words(ident: &str) -> Vec<String> {
        let mut words = Vec::new();
        for part in ident.split('_') {
            let mut word = String::new();
            let mut previous_lower = false;
            for c in part.chars() {
                if c.is_uppercase() && previous_lower {
                    words.push(std::mem::take(&mut word));
                }
                previous_lower = c.is_lowercase() || c.is_ascii_digit();
                word.extend(c.to_lowercase());
            }
            words.push(word);
        }
        words
    }

    /// `line` of code: its string literals emptied and its trailing comment cut, so that only identifiers are left.
    fn code_of(line: &str) -> String {
        let (mut out, mut in_string, mut escaped) = (String::new(), false, false);
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            match (in_string, c) {
                (true, _) if escaped => escaped = false,
                (true, '\\') => escaped = true,
                (true, '"') => in_string = false,
                (true, _) => {}
                (false, '"') => in_string = true,
                (false, '/') if chars.peek() == Some(&'/') => break,
                (false, c) => out.push(c),
            }
        }
        out
    }

    /// The identifiers of `text`'s production code (the part before `mod tests`) that are a note, as `line: identifier`.
    fn notes_in(text: &str) -> Vec<String> {
        let production = text.split("#[cfg(test)]\nmod tests").next().unwrap_or(text);
        let mut found = Vec::new();
        for (n, line) in production.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            // rule (c): the palette's note colour is not the note
            let code = code_of(line)
                .replace("palette::note", "")
                .replace("Ui::Note", "");
            for ident in code
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .filter(|i| !i.is_empty())
            {
                if words(ident).iter().any(|w| w == "note") {
                    found.push(format!("{}: {ident}", n + 1));
                }
            }
        }
        found
    }

    fn walk(root: &Path, dir: &Path, offenders: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, offenders);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let file = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                if EXEMPT.iter().any(|(exempt, _)| file.starts_with(exempt)) {
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap();
                offenders.extend(
                    notes_in(&text)
                        .into_iter()
                        .map(|found| format!("{file}:{found}")),
                );
            }
        }
    }

    #[test]
    fn the_scan_finds_a_note_and_nothing_else() {
        let found = |text: &str| notes_in(text).len();
        assert_eq!(found("self.note = why;"), 1);
        assert_eq!(found("Action::Note(note) => note.clone(),"), 3);
        assert_eq!(found("const DOWN_NOTE: &str = \"x\";"), 1);
        assert_eq!(found("fn draw(f: &mut Frame, note: &str)"), 1);
        assert_eq!(
            found("Paragraph::new(line).style(palette::note())"),
            0,
            "rule (c)"
        );
        assert_eq!(found("palette::ui(palette::Ui::Note)"), 0, "rule (c)");
        assert_eq!(
            found("let notes = denote(\"a note\"); // note"),
            0,
            "other words, strings, comments"
        );
        assert_eq!(
            found("/// the note\nfn f() {}\n#[cfg(test)]\nmod tests {\n let note = 1;\n}"),
            0,
            "tests may name it"
        );
    }

    /// No view keeps a note: not the tree and its Inbox, the Plan view, the Dispatch column, nor the history and the chat
    /// views. A refusal or a failure is a dialog, the rest a line of the log.
    #[test]
    fn no_view_keeps_a_note() {
        let mut offenders = Vec::new();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        walk(&root, &root, &mut offenders);
        assert!(
            offenders.is_empty(),
            "no view draws a transient note (commands.md §17 \"No notes\"): a failure the User must see is a `text` dialog, \
             the rest goes to run/tui/<view>.log:\n{}",
            offenders.join("\n")
        );
    }
}
