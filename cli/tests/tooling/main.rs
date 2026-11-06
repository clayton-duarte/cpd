//! The `tooling` integration-test binary of `cpd` (docs/implementing.md, Tests: the area table): every file of this
//! directory is a module. The harness, `cli/tests/common/`, is shared by every area's binary.

#[path = "../common/mod.rs"]
mod common;

mod app;
mod cargo_script;
mod dead_code;
mod fake_tmux;
mod fx;
mod fx_exec;
mod gate_run_smoke;
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
mod preview_web_script;
mod real_process_scan;
mod retired_terms;
mod run_with;
mod slot;
mod smoke;
mod testkit;
mod testkit_hosts;
mod testkit_screen;
mod testkit_tree;
mod tripwire_script;
mod usage_grammar;
mod version;
