//! `Ports::scope` and `Ports::current`, the ambient ports (docs/implementing.md, "Ports, Spawn and run_with"), and the
//! guard that no non-test file builds the real ports by hand outside the allowlist.
//!
//! Which `Ports` is current is read by the handle: `Fakes` hands its own `Arc`s in, and `current()` must give the same
//! ones back. The real ports answer `kind() == "SystemRunner"`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ports::{Fakes, Ports};

/// Whether `ports` holds the runner of `fakes` (the same `Arc`, not an equal one).
fn is(ports: &Ports, fakes: &Fakes) -> bool {
    Arc::as_ptr(&ports.runner).cast::<()>() == Arc::as_ptr(&fakes.runner).cast::<()>()
}

fn is_real(ports: &Ports) -> bool {
    ports.runner.kind() == "SystemRunner"
        && ports.spawn.kind() == "ProcessSpawn"
        && ports.clock.kind() == "SystemClock"
}

#[test]
fn current_is_the_real_ports_with_no_scope() {
    assert!(is_real(&Ports::current()));
}

#[test]
fn current_inside_a_scope_is_the_ports_given() {
    let fakes = Fakes::new();
    let inside = Ports::scope(&fakes.ports, Ports::current);
    assert!(is(&inside, &fakes), "the runner is the Fakes' own");
    assert!(
        Arc::ptr_eq(&inside.spawn, &fakes.ports.spawn),
        "the spawn is the Fakes' own"
    );
    assert!(
        Arc::ptr_eq(&inside.clock, &fakes.ports.clock),
        "the clock is the Fakes' own"
    );
}

#[test]
fn a_scope_returns_what_its_closure_returns() {
    let fakes = Fakes::new();
    assert_eq!(Ports::scope(&fakes.ports, || 7), 7);
}

#[test]
fn the_ports_are_real_again_after_the_scope() {
    let fakes = Fakes::new();
    let ran = Ports::scope(&fakes.ports, || is(&Ports::current(), &fakes));
    assert!(ran, "inside, the ports are the Fakes'");
    assert!(is_real(&Ports::current()), "after, the real ones");
}

#[test]
fn scopes_nest_and_the_outer_is_restored() {
    let (outer, inner) = (Fakes::new(), Fakes::new());
    let seen = Ports::scope(&outer.ports, || {
        let before = is(&Ports::current(), &outer);
        let within = Ports::scope(&inner.ports, || {
            (is(&Ports::current(), &inner), is(&Ports::current(), &outer))
        });
        let after = is(&Ports::current(), &outer);
        (before, within, after)
    });
    assert_eq!(
        seen,
        (true, (true, false), true),
        "outer, then inner (and not outer), then outer again"
    );
    assert!(
        is_real(&Ports::current()),
        "and the real ports once both are left"
    );
}

#[test]
fn the_scope_is_restored_after_a_panic() {
    let (outer, inner) = (Fakes::new(), Fakes::new());
    let entered = std::cell::Cell::new(false);
    let panicked = Ports::scope(&outer.ports, || {
        let result = catch_unwind(AssertUnwindSafe(|| {
            Ports::scope(&inner.ports, || {
                entered.set(true);
                panic!("the closure of the scope panics");
            })
        }));
        (result.is_err(), is(&Ports::current(), &outer))
    });
    assert!(entered.get(), "the closure ran");
    assert_eq!(
        panicked,
        (true, true),
        "the panic got out, and the outer scope is the current one again"
    );
    assert!(
        is_real(&Ports::current()),
        "and the real ports once the outer scope is left"
    );
}

#[test]
fn another_thread_sees_no_scope() {
    let fakes = Fakes::new();
    let there = Ports::scope(&fakes.ports, || {
        assert!(is(&Ports::current(), &fakes), "this thread is scoped");
        std::thread::spawn(|| is_real(&Ports::current()))
            .join()
            .expect("the thread does not panic")
    });
    assert!(there, "a spawned thread has the real ports");
}

#[test]
fn a_thread_scopes_the_ports_it_captured_and_the_scope_is_its_own() {
    let fakes = Fakes::new();
    let (inside, here) = Ports::scope(&fakes.ports, || {
        let captured = Ports::current();
        let inside = std::thread::scope(|s| {
            s.spawn(|| Ports::scope(&captured, || is(&Ports::current(), &fakes)))
                .join()
                .expect("the thread does not panic")
        });
        (inside, is(&Ports::current(), &fakes))
    });
    assert!(
        inside,
        "the thread that scoped the captured ports sees them"
    );
    assert!(here, "this thread's scope is untouched");
}

/// What a `Ports::real()`-building line of a non-test file names.
const REAL: [&str; 3] = ["SystemRunner", "ProcessSpawn", "Ports::real()"];

/// The files that may name the real ports whatever the line, each with its reason: a path, or a `dir/` prefix.
const FILES: &[(&str, &str)] = &[
    (
        "core/src/ports.rs",
        "the definition: `Ports::real()` builds them, `Ports::current()` falls back to them",
    ),
    (
        "core/src/host/runner.rs",
        "the real adapter of the `Runner`",
    ),
    ("core/src/host/spawn.rs", "the real adapter of the `Spawn`"),
    (
        "core/src/fakes/",
        "the tmux route's delegation to a real tmux, for the tests of the route itself",
    ),
];

/// The lines (trimmed, a substring of one) that may name them in the files that start the process on the real ports by
/// design, each with its reason. A new line in these files is a new wrapper: it reads `Ports::current()`.
const LINES: &[(&str, &str, &str)] = &[
    (
        "cli/src/lib.rs",
        "run_with(&cpd_core::ports::Ports::real(), argv, stdout)",
        "`main_with`: the binary's entry",
    ),
    (
        "cli/src/cli.rs",
        "run_with(&Ports::real(), argv)",
        "`run`: the entry of the real ports, which `run_with` scopes",
    ),
    (
        "engine/src/watch/engine.rs",
        "Engine::with_ports(home, tmux, &Ports::real())",
        "`Engine::new`: `cpd watch` on the real ports",
    ),
    (
        "engine/src/watch/engine.rs",
        "serve(&Ports::real(), &home, &STOP, catch_signals)",
        "`serve`: `cpd watch` on the real ports",
    ),
    (
        "tui/src/lib.rs",
        "run_with(&Ports::real(), args, version, run_command, restart)",
        "`run`: the cockpit on the real ports",
    ),
    (
        "core/src/session/run.rs",
        "spawn.kind() == \"ProcessSpawn\"",
        "asks which `Spawn` it was handed, starts nothing",
    ),
    (
        "core/src/host/tmux.rs",
        "spawn.kind() == \"ProcessSpawn\"",
        "asks which `Spawn` it was handed, starts nothing",
    ),
];

/// `text` without its comments and without the items and modules behind `#[cfg(test)]`: what is product code.
fn product_lines(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut kept = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if line == "#[cfg(test)]" {
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

/// The `.rs` files under `dir` that are not test files (`*_tests.rs`, `tests.rs`, a `tests` directory).
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

/// A new wrapper must read `Ports::current()`: no non-test file of the four crates names `SystemRunner`,
/// `ProcessSpawn` or `Ports::real()` outside the allowlists above. Red until the wrappers read the ambient ports.
#[test]
fn no_product_file_builds_the_real_ports_by_hand() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the workspace root");
    let mut offenders = Vec::new();
    for krate in ["core", "engine", "tui", "cli"] {
        let mut files = Vec::new();
        product_files(&root.join(krate).join("src"), &mut files);
        for path in files {
            let rel = path
                .strip_prefix(root)
                .expect("under the root")
                .to_string_lossy()
                .into_owned();
            if FILES.iter().any(|(allowed, _)| {
                rel == *allowed || (allowed.ends_with('/') && rel.starts_with(allowed))
            }) {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
            for (number, code) in product_lines(&text) {
                let allowed = LINES
                    .iter()
                    .any(|(file, line, _)| *file == rel && code.contains(line));
                if REAL.iter().any(|name| code.contains(name)) && !allowed {
                    offenders.push(format!("{rel}:{number}: {code}"));
                }
            }
        }
    }
    offenders.sort();
    assert!(
        offenders.is_empty(),
        "these lines name the real ports; read `Ports::current()` instead (or allowlist the entry, with its reason):\n{}",
        offenders.join("\n")
    );
}
