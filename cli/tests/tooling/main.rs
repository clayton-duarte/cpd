//! The `tooling` integration-test binary of `cpd` (docs/implementing.md, Tests: the area table): every file of this
//! directory is a module. The harness, `cli/tests/common/`, is shared by every area's binary.

#[path = "../common/mod.rs"]
mod common;

mod cargo_script;
mod dead_code;
mod gate_run_smoke;
mod fake_tmux;
mod fx;
mod fx_exec;
mod guards;
mod harness;
mod install_script;
mod interface;
mod layout;
mod no_gh_pr_edit;
mod no_repo_lock;
mod pill_parity;
mod pill_sites;
mod pnpm_fresh;
mod preview_script;
mod retired_terms;
mod run_with;
mod smoke;
mod slot;
mod testkit;
mod testkit_hosts;
mod testkit_screen;
mod testkit_tree;
mod usage_grammar;
mod tripwire_script;
mod version;
