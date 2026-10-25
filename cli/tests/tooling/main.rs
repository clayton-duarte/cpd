//! The `tooling` integration-test binary of `cpd` (docs/implementing.md, Tests: the area table): every file of this
//! directory is a module. The harness, `cli/tests/common/`, is shared by every area's binary.

#[path = "../common/mod.rs"]
mod common;

mod cargo_script;
mod dead_code;
mod gate_run;
mod fake_tmux;
mod fx;
mod guards;
mod harness;
mod install_script;
mod interface;
mod layout;
mod no_repo_lock;
mod pill_parity;
mod pill_sites;
mod npm_fresh;
mod preview_script;
mod run_with;
mod smoke;
mod slot;
mod tag_script;
mod test_changed_script;
mod testkit;
mod testkit_screen;
mod tests_script;
mod tripwire_script;
mod version;
