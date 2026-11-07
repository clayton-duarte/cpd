//! `cpd desktop` (commands.md §14.4a): the clap grammar, and the command run in process on the `Fx`'s fake ports (a
//! runner rule for `sh`, the checkout named by `app-root`). No real preview script, pnpm or Electron.

use clap::Parser;
use cpd::cli::Cli;
use cpd_core::host::runner::Reply;

use crate::common::fx::Fx;

fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
    let argv = ["cpd", "desktop"].into_iter().chain(args.iter().copied());
    Cli::try_parse_from(argv)
}

/// An `Fx` whose `$HOME` names a checkout (`app-root`), and whose `sh` prints `script text`.
fn fx() -> (Fx, String) {
    let fx = Fx::with_ports();
    let root = fx.env.root().join("checkout");
    let share = fx.env.home().join(".local/share/cpd");
    std::fs::create_dir_all(&share).unwrap();
    std::fs::write(share.join("app-root"), format!("{}\n", root.display())).unwrap();
    fx.fake("sh", Reply::ok("script text\n"));
    let script = root.join("scripts/preview-web.sh");
    (fx, script.display().to_string())
}

/// The argv of every `sh` call of the runner.
fn sh_calls(fx: &Fx) -> Vec<Vec<String>> {
    fx.fakes
        .runner
        .calls()
        .into_iter()
        .filter(|c| c.program == "sh")
        .map(|c| c.args)
        .collect()
}

#[test]
fn command_is_named_desktop() {
    parse(&["--status"]).expect("`cpd desktop --status` is the command (commands.md §14.4a)");
}

#[test]
fn app_command_is_gone() {
    // D1: `cpd app` is removed outright, no alias.
    let err = Cli::try_parse_from(["cpd", "app", "--status"]).expect_err("`cpd app` is rejected");
    assert_eq!(err.kind(), clap::error::ErrorKind::InvalidSubcommand, "{err}");
}

#[test]
fn not_installed_error_code_is_desktop_not_installed() {
    // No app-root file: every form is `desktop_not_installed`, exit 1 (commands.md §14.4a).
    let fx = Fx::with_ports();
    let run = fx.cpd(&["desktop", "--status"]);
    assert_eq!(run.exit, 1, "{run:?}");
    assert_eq!(run.json()["error"]["code"], "desktop_not_installed");
}

#[test]
fn flags_are_mutually_exclusive() {
    for pair in [
        ["--preview", "--off"],
        ["--preview", "--reload"],
        ["--reload", "--status"],
        ["--off", "--status"],
    ] {
        let err = parse(&pair).unwrap_err();
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::ArgumentConflict,
            "{pair:?}"
        );
    }
    // A worktree after --off or --status is refused too.
    assert!(parse(&["--off", "/wt"]).is_err());
    assert!(parse(&["--status", "/wt"]).is_err());
    // Plain `cpd desktop` takes no flag, and each form alone parses.
    parse(&[]).expect("plain `cpd desktop`");
    for flag in ["--preview", "--reload", "--off", "--status"] {
        parse(&[flag]).unwrap_or_else(|err| panic!("`cpd desktop {flag}`: {err}"));
    }
    // The refused forms reach no script; an accepted one runs its subcommand.
    let (fx, script) = fx();
    let refused = fx.cpd(&["desktop", "--preview", "--off"]);
    assert_eq!(refused.exit, 2, "{refused:?}");
    assert_eq!(sh_calls(&fx), Vec::<Vec<String>>::new());
    let off = fx.cpd(&["desktop", "--off"]);
    assert_eq!(off.exit, 0, "{off:?}");
    assert_eq!(off.json()["action"], "preview_off");
    assert_eq!(sh_calls(&fx), vec![vec![script, "off".to_owned()]]);
}

#[test]
fn preview_takes_an_optional_worktree() {
    parse(&["--preview", "/wt"]).expect("--preview takes a worktree");
    parse(&["--reload", "/wt"]).expect("--reload takes a worktree");
    parse(&["--preview"]).expect("--preview without a worktree");
    parse(&["--reload"]).expect("--reload without a worktree");
    // A worktree with no form is refused.
    assert!(parse(&["/wt"]).is_err());
    let (fx, script) = fx();
    let on = fx.cpd(&["desktop", "--preview", "/wt"]);
    assert_eq!(on.exit, 0, "{on:?}");
    let json = on.json();
    assert_eq!(json["action"], "preview_on");
    assert_eq!(json["output"], "script text\n");
    let bare = fx.cpd(&["desktop", "--preview"]);
    assert_eq!(bare.exit, 0, "{bare:?}");
    let reload = fx.cpd(&["desktop", "--reload", "/wt"]);
    assert_eq!(reload.json()["action"], "reloaded_preview");
    assert_eq!(
        sh_calls(&fx),
        vec![
            vec![script.clone(), "on".to_owned(), "/wt".to_owned()],
            vec![script.clone(), "on".to_owned()],
            vec![script, "reload".to_owned(), "/wt".to_owned()],
        ]
    );
}
