//! `no_write_to_the_terminal_outside_the_frame` (hs-d363): a view owns its terminal, so what it writes there outside
//! its frame breaks the drawn frame (the tree's `engine was down: …` line did). A view's lines go to the note (inside
//! the frame) and to its log file, `$CPD_HOME/run/tui/<view>.log` (commands.md §17).

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// What writes to the terminal behind the frame's back: the print macros, a handle on stdout or stderr, a child that
    /// inherits them. `eprintln!` and `println!` contain `print`: each is listed whole.
    const WRITES: [&str; 8] =
        ["eprintln!", "eprint!", "println!", "print!", "dbg!", "stdout()", "stderr()", "Stdio::inherit"];

    /// A write a view may make, with why. A new one needs a reviewer to add it here.
    struct Exempt {
        /// The file, relative to `tui/src/`.
        file: &'static str,
        /// What the offending line contains; `""` for the whole file.
        line: &'static str,
        why: &'static str,
    }

    const EXEMPT: [Exempt; 8] = [
        Exempt {
            file: "cockpit.rs",
            line: "",
            why: "`cpd tui`'s own CLI output, in the User's shell before the cockpit exists or after it: it runs outside every view",
        },
        Exempt {
            file: "lib.rs",
            line: "Starting again in",
            why: "the view runner's restart message: `run_view` has already dropped its `Restore`, so the alternate screen is left",
        },
        Exempt {
            file: "lib.rs",
            line: "is_terminal()",
            why: "`on_a_terminal` asks whether stdout is a terminal; it writes nothing",
        },
        Exempt {
            file: "runner.rs",
            line: "execute!(stdout(), Enable",
            why: "terminal setup: mouse and focus reporting on, before the first frame",
        },
        Exempt {
            file: "runner.rs",
            line: "execute!(stdout(), Disable",
            why: "terminal teardown: mouse and focus reporting off, in `Restore::drop`",
        },
        Exempt {
            file: "merge/mod.rs",
            line: "execute!(std::io::stdout(), ",
            why: "terminal setup and teardown of the popup's mouse capture, around its frames",
        },
        Exempt {
            file: "dialog/mod.rs",
            line: "execute!(std::io::stdout(), ",
            why: "terminal setup and teardown of the popup's mouse capture, around its frames",
        },
        Exempt {
            file: "terminal_writes.rs",
            line: "",
            why: "this guard names the patterns it looks for",
        },
    ];

    fn exempt(file: &str, line: &str) -> Option<usize> {
        EXEMPT.iter().position(|e| e.file == file && line.contains(e.line))
    }

    fn walk(root: &Path, dir: &Path, offenders: &mut Vec<String>, used: &mut [bool; EXEMPT.len()]) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, offenders, used);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let file = path.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                let text = std::fs::read_to_string(&path).unwrap();
                for (n, line) in text.lines().enumerate() {
                    if line.trim_start().starts_with("//") || !WRITES.iter().any(|w| line.contains(w)) {
                        continue;
                    }
                    match exempt(&file, line) {
                        Some(i) => used[i] = true,
                        None => offenders.push(format!("{file}:{}: {}", n + 1, line.trim())),
                    }
                }
            }
        }
    }

    /// No view writes to its terminal outside its frame: not the restart line, not the copy message, not an action's
    /// result. Every exemption is named above, with its reason.
    #[test]
    fn no_write_to_the_terminal_outside_the_frame() {
        let (mut offenders, mut used) = (Vec::new(), [false; EXEMPT.len()]);
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        walk(&root, &root, &mut offenders, &mut used);
        assert!(
            offenders.is_empty(),
            "a view's lines go to the note and to run/tui/<view>.log, not to its terminal:\n{}",
            offenders.join("\n")
        );
        let stale: Vec<_> = EXEMPT.iter().zip(used).filter(|(_, used)| !used).map(|(e, _)| format!("{}: {}", e.file, e.why)).collect();
        assert!(stale.is_empty(), "exemptions that match nothing (delete them):\n{}", stale.join("\n"));
    }
}
