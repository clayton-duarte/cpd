//! Red tests for retiring CPD.app, electron build, and legacy shell release tooling.
//!
//! Contract: pro-d567, docs/architecture/client.md, docs/commands.md.
//! CLI rejects `cpd desktop release` and `cpd install shell` subcommands,
//! and prevents any attempt to fetch legacy app-v* release assets.

mod retire_app_test {
    #[test]
    fn desktop_release_command_removed() {
        panic!("stub: not implemented yet");
    }

    #[test]
    fn shell_install_removed() {
        panic!("stub: not implemented yet");
    }
}
