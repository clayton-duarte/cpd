//! `cpd app` (commands.md §14.4a): the clap grammar, and the command run in process on the `Fx`'s fake ports (a runner
//! rule for `sh`, the checkout named by `app-root`). No real preview script, pnpm or Electron.

use clap::Parser;
use cpd::cli::{Cli, Command};
use cpd_core::host::runner::Reply;

use crate::common::fx::Fx;

fn parse(args: &[&str]) -> Result<cpd::app::Args, clap::Error> {
    let argv = ["cpd", "app"].into_iter().chain(args.iter().copied());
    match Cli::try_parse_from(argv)?.command {
        Some(Command::App(args)) => Ok(args),
        other => panic!("not `cpd app`: {other:?}"),
    }
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
fn app_flags_are_mutually_exclusive() {
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
    // Plain `cpd app` takes no flag, and each form alone parses.
    let bare = parse(&[]).unwrap();
    assert!(!(bare.preview || bare.reload || bare.off || bare.status));
    for flag in ["--preview", "--reload", "--off", "--status"] {
        parse(&[flag]).unwrap();
    }
    // The refused forms reach no script; an accepted one runs its subcommand.
    let (fx, script) = fx();
    let refused = fx.cpd(&["app", "--preview", "--off"]);
    assert_eq!(refused.exit, 2, "{refused:?}");
    assert_eq!(sh_calls(&fx), Vec::<Vec<String>>::new());
    let off = fx.cpd(&["app", "--off"]);
    assert_eq!(off.exit, 0, "{off:?}");
    assert_eq!(off.json()["action"], "preview_off");
    assert_eq!(sh_calls(&fx), vec![vec![script, "off".to_owned()]]);
}

#[test]
fn app_preview_takes_an_optional_worktree() {
    let with = parse(&["--preview", "/wt"]).unwrap();
    assert!(with.preview);
    assert_eq!(with.worktree.as_deref(), Some(std::path::Path::new("/wt")));
    let reload = parse(&["--reload", "/wt"]).unwrap();
    assert!(reload.reload);
    assert_eq!(
        reload.worktree.as_deref(),
        Some(std::path::Path::new("/wt"))
    );
    assert_eq!(parse(&["--preview"]).unwrap().worktree, None);
    assert_eq!(parse(&["--reload"]).unwrap().worktree, None);
    // A worktree with no form is refused.
    assert!(parse(&["/wt"]).is_err());
    let (fx, script) = fx();
    let on = fx.cpd(&["app", "--preview", "/wt"]);
    assert_eq!(on.exit, 0, "{on:?}");
    let json = on.json();
    assert_eq!(json["action"], "preview_on");
    assert_eq!(json["output"], "script text\n");
    let bare = fx.cpd(&["app", "--preview"]);
    assert_eq!(bare.exit, 0, "{bare:?}");
    let reload = fx.cpd(&["app", "--reload", "/wt"]);
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
