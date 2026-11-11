//! Tests for dropped desktop command.

#[test]
fn desktop_command_is_gone() {
    use clap::Parser;
    use cpd::cli::Cli;

    let err =
        Cli::try_parse_from(["cpd", "desktop", "--status"]).expect_err("desktop command dropped");
    assert_eq!(err.kind(), clap::error::ErrorKind::InvalidSubcommand);
}
