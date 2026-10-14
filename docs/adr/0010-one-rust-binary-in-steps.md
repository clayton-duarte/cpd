---
status: accepted
---

# `cpd` becomes one Rust binary, in steps

The User prefers Rust and wants `cpd` to be its own binary with few system dependencies, such as tmux, pi and git. The cockpit (`cpd-tui`) is Rust now, a crate in the cpd repo under `tui/`, and reads the TypeScript CLI's JSON. The CLI is ported to Rust later, so that CLI and TUI become one binary `cpd` (for example `cpd tui` opens the cockpit). A forked Lead, "cpd rust lead", leads the port.

The pi adapter stays TypeScript in `pi/`, because pi loads extensions as TS/JS. This amends nothing in ADR-0005: the control plane is still the `cpd` CLI.

## Consequences

- The Gate must gain `cargo build` and `cargo test`. `verify.sh` is a protected file, so this is a User decision.
- The repo's end-state layout is not yet decided. Probably a Cargo workspace with a shared core (inferred); the Rust refactor Lead plans it.
- The database is chosen in the Rust port (ADR-0012).
- System dependencies are tools such as tmux, pi and git, not a Node runtime for the CLI once it is ported.
