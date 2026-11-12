//! The `engine` integration-test binary of `cpd` (docs/implementing.md, Tests: the area table): every file of this
//! directory is a module. The harness, `cli/tests/common/`, is shared by every area's binary.

#[path = "../common/mod.rs"]
mod common;

mod doctor;
mod serve_test;
mod shaping_engine;
mod watch;
mod watch_auto_merge;
mod watch_curate_docs_gone;
mod watch_curate_inbox;
mod watch_distill_inbox;
mod watch_durable;
mod watch_host_alert;
mod watch_inbox_models;
mod watch_inbox_overdue;
mod watch_models;
mod watch_own_post;
mod watch_parked;
mod watch_parked_inbox;
mod watch_permanent;
mod watch_resources;
mod watch_review_archive;
mod watch_sleep;
mod watch_stand_down;
mod watch_team;
mod watch_titles_width;
mod watch_worker_archive;
mod write_executable;

mod main {
    use crate::common;
    use common::fx::Fx;
    use serde_json::Value;
    use std::fs;

    #[test]
    fn cli_retains_only_engine_and_infra_commands() {
        let mut stdout = Vec::new();
        cpd::main_with(["cpd".into(), "--help".into()], &mut stdout);
        let json: Value = serde_json::from_slice(&stdout).expect("valid JSON from --help");
        let help = json["help"].as_str().expect("help string");

        // Retained binary footprint (pi-native-core.md §6.2):
        // watch, install, gate, host, doctor, ask, attempt
        for retained in [
            "watch", "install", "gate", "host", "doctor", "ask", "attempt",
        ] {
            assert!(
                help.contains(&format!("  {retained}"))
                    || help.contains(&format!("\n  {retained} ")),
                "retained command '{retained}' missing from help text: {help}"
            );
        }

        // Dropped surfaces (pi-native-core.md §6.1, commands.md §2 "Removed"):
        // tui, desktop, cockpit, argv-check
        for dropped in ["tui", "desktop", "cockpit"] {
            assert!(
                !help.contains(&format!("  {dropped}"))
                    && !help.contains(&format!("\n  {dropped} ")),
                "dropped command '{dropped}' must not be present in help text: {help}"
            );
        }

        // Bare `cpd` without subcommands should no longer invoke or reference TUI / cockpit
        let mut bare_stdout = Vec::new();
        let code = cpd::main_with(["cpd".into()], &mut bare_stdout);
        let bare_json: Value = serde_json::from_slice(&bare_stdout).unwrap_or(Value::Null);
        assert_ne!(
            code,
            std::process::ExitCode::SUCCESS,
            "bare cpd without subcommands should refuse instead of opening cockpit"
        );
        if let Some(err_msg) = bare_json["error"]["message"].as_str() {
            assert!(
                !err_msg.to_lowercase().contains("cockpit"),
                "bare cpd error should not refer to cockpit: {err_msg}"
            );
        }

        // Verify cpd-tui crate is not linked / present as dependency in cli/Cargo.toml
        let cargo_toml = fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
            .expect("read cli/Cargo.toml");
        assert!(
            !cargo_toml.contains("cpd-tui"),
            "cli/Cargo.toml still references cpd-tui: {cargo_toml}"
        );
    }

    #[test]
    fn recovery_mode_entry_point_available() {
        let fx = Fx::with_ports();
        let adapter = fx.env.root().join("adapter");
        fs::create_dir_all(&adapter).unwrap();
        fs::write(adapter.join("package.json"), "{}").unwrap();
        let config_text = format!(
            "version = 1\nmachine = \"hs\"\npi_adapter = \"{}\"\ncontext_distill_tier = \"G0\"\nworker_tier = \"H2\"\nlead_tier = \"S4\"\n",
            adapter.display()
        );
        fx.env.write_config(&config_text);
        let policy_path = fx.env.cpd_home().join("policy/host.toml");
        fs::create_dir_all(policy_path.parent().unwrap()).unwrap();
        fs::write(
            policy_path,
            "[model]\nO = \"p/m-opus\"\nS = \"p/m-sonnet\"\nH = \"p/m-low\"\nG = \"p/m-aux\"\nJ = \"p/m-aux\"\n",
        )
        .unwrap();
        fx.healthy_tools();

        // Recovery mode entry point 1: doctor works directly for inspection and diagnosis (pi-native-core.md §5.4, contract.md §6.22)
        let out = fx.cpd(&["doctor"]);
        assert_eq!(
            out.exit, 0,
            "doctor must succeed as standalone recovery utility: {:?}",
            out.stdout
        );

        // Recovery mode entry point 2: watch --ensure works to restart core daemon (pi-native-core.md §5.4, §6.2)
        let ensure_out = fx.cpd(&["watch", "--ensure"]);
        // --ensure should be a valid subcommand of watch, not a usage error
        assert_ne!(
            ensure_out.exit, 2,
            "watch --ensure must be a recognized recovery entry point (not usage error): {:?}",
            ensure_out.stdout
        );

        // Recovery mode entry point 3: attach to tmux session for recovery mode (contract.md §6.22)
        // Attempting to invoke removed `tui` command must be refused as unrecognized subcommand
        let tui_out = fx.cpd(&["tui"]);
        assert_eq!(
            tui_out.exit, 2,
            "tui command must be dropped and rejected as usage error: {:?}",
            tui_out.stdout
        );
        let tui_json: Value = serde_json::from_str(&tui_out.stdout).expect("JSON response");
        assert_eq!(tui_json["error"]["code"], "usage");
        assert!(
            tui_json["error"]["message"]
                .as_str()
                .unwrap_or_default()
                .contains("unrecognized subcommand 'tui'"),
            "expected unrecognized subcommand 'tui', got: {:?}",
            tui_json
        );
    }
}
