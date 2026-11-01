//! `no_view_starts_the_terminal_by_hand` (audit-plan F5, audit-shell F5): `run_view` owns the terminal, its
//! `Restore` guard turning mouse reporting off on every exit, a panic included. A view that calls `ratatui::init`,
//! enables mouse capture and restores by hand leaves mouse reporting on when it panics between them.

use crate::guard_tests::product_sources;

/// The calls only `runner.rs` and `view_ports.rs` may make: `run_view`'s setup and its `Restore` guard.
const FORBIDDEN: [&str; 5] = [
    "ratatui::init",
    "ratatui::try_init",
    "ratatui::restore",
    "EnableMouseCapture",
    "DisableMouseCapture",
];

/// A view never starts the terminal: `run_view` does, and only its `Restore` guard may stop it. The failure lists
/// every file that does it by hand.
#[test]
fn no_view_starts_the_terminal_by_hand() {
    let mut offenders = Vec::new();
    for (file, text) in product_sources() {
        if file == "runner.rs" || file == "view_ports.rs" {
            continue;
        }
        for (n, line) in text.lines().enumerate() {
            if FORBIDDEN.iter().any(|call| line.contains(call)) {
                offenders.push(format!("{file}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "only runner.rs and view_ports.rs start the terminal: run_view's Restore guard turns mouse reporting off on every exit, a \
         panic included, so no view may call ratatui::init / restore or toggle mouse capture by hand:\n{}",
        offenders.join("\n")
    );
}
