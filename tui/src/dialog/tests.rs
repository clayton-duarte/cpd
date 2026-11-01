//! Tests of the dialog kit's openers, popup and result file (the view's are in `view_tests.rs` and `input_tests.rs`).

use super::*;

use std::sync::Arc;

use cpd_core::clock::ManualClock;
use cpd_core::fakes::tmux::TmuxRoute;
use cpd_core::host::runner::FakeRunner;

/// A fake tmux (the `tmux` route of a `FakeRunner`) with a server in `dir` and one attached client, `c1`, of 100 x 30. Its
/// popup writes no dialog outcome, so a dialog's result is the "no usable result" error of a popup closed some other way.
/// The runner records every call, to read the popup's argv back with [`popup_of`].
fn recording_tmux(dir: &tempfile::TempDir) -> (Tmux, Arc<FakeRunner>) {
    let route = Arc::new(TmuxRoute::new());
    let runner = Arc::new(FakeRunner::new(ManualClock::new()).route("tmux", route.clone()));
    let tmux = Tmux::with_tmpdir("test", dir.path()).with_runner(runner.clone());
    tmux.run(
        &[
            "new-session",
            "-d",
            "-s",
            "cockpit",
            "-x",
            "100",
            "-y",
            "30",
            "sleep 1",
        ]
        .map(str::to_owned),
    )
    .unwrap();
    route.fake().add_client("c1", 100, 30);
    (tmux, runner)
}

/// The arguments of the `display-popup` call the runner saw, from `display-popup` on.
fn popup_of(runner: &FakeRunner) -> Vec<String> {
    let calls = runner.calls();
    let call = calls
        .iter()
        .find(|c| c.args.iter().any(|a| a == "display-popup"))
        .unwrap_or_else(|| panic!("no display-popup in {calls:?}"));
    call.args
        .iter()
        .skip_while(|a| *a != "display-popup")
        .cloned()
        .collect()
}

#[test]
fn show_error_opens_a_text_popup_titled_with_what_failed_and_saying_why() {
    let dir = tempfile::tempdir().unwrap();
    let (tmux, runner) = recording_tmux(&dir);
    let _ = show_error(&tmux, "session open l-ship", "no_cockpit: No cockpit.");
    let popup = popup_of(&runner);
    assert!(
        !popup.contains(&"-T".to_owned()),
        "the popup passes no tmux title: the view draws it once: {popup:?}"
    );
    let command = popup.last().unwrap();
    assert!(
        command.starts_with("cpd tui view dialog text --title='session open l-ship' --out="),
        "{command}"
    );
    assert!(
        command.ends_with("--body='no_cockpit: No cockpit.'"),
        "the body is why: {command}"
    );
}

#[test]
fn a_failed_command_of_a_view_is_a_log_line_and_the_error_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let (tmux, runner) = recording_tmux(&dir);
    let home =
        cpd_core::home::CpdHome::resolve(Some(dir.path().join("home").into()), None).unwrap();
    let log = ViewLog::of(&home, "plan");
    let err = CpdError::new(ErrorCode::TmuxFailed, "No tmux client is attached.");
    report(&tmux, &log, "Open Worker", &err);
    let logged = std::fs::read_to_string(dir.path().join("home/run/tui/plan.log")).unwrap();
    let first = logged.lines().next().unwrap();
    assert!(
        first.ends_with(" failed: Open Worker: tmux_failed: No tmux client is attached."),
        "{logged}"
    );
    let popup = popup_of(&runner);
    assert!(
        popup
            .last()
            .unwrap()
            .ends_with("--body='tmux_failed: No tmux client is attached.'"),
        "{popup:?}"
    );
}

#[test]
fn the_outcome_is_a_small_json_value() {
    let json = |o: &Outcome| serde_json::to_string(o).unwrap();
    assert_eq!(json(&Outcome::confirmed()), r#"{"confirmed":true}"#);
    assert_eq!(json(&Outcome::cancelled()), r#"{"confirmed":false}"#);
    assert_eq!(
        json(&Outcome::value("a \"b\"".into())),
        r#"{"confirmed":true,"value":"a \"b\""}"#
    );
}

#[test]
fn a_result_file_is_read_and_deleted() {
    let dir = tempfile::tempdir().unwrap();
    for outcome in [
        Outcome::confirmed(),
        Outcome::cancelled(),
        Outcome::value("v é".into()),
    ] {
        let path = dir.path().join("out.json");
        write_outcome(&path, &outcome).unwrap();
        assert_eq!(read_outcome(&path).unwrap(), outcome);
        assert!(!path.exists());
    }
}

#[test]
fn a_missing_or_bad_result_file_is_an_error_never_a_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.json");
    assert_eq!(read_outcome(&path).unwrap_err().code(), ErrorCode::Internal);
    for bad in [
        "",
        "{",
        "[]",
        r#"{"confirmed":"yes"}"#,
        r#"{}"#,
        r#"{"confirmed":false,"value":"x"}"#,
        r#"{"confirmed":true,"extra":1}"#,
    ] {
        std::fs::write(&path, bad).unwrap();
        let err = read_outcome(&path).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Internal, "{bad:?}");
        assert!(!path.exists(), "{bad:?}: the file is deleted");
    }
}

#[test]
fn a_result_file_that_cannot_be_written_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let err = write_outcome(
        &dir.path().join("no/such/dir/out.json"),
        &Outcome::confirmed(),
    )
    .unwrap_err();
    assert_eq!(err.code(), ErrorCode::Internal);
}

#[test]
fn temporary_files_are_under_the_temp_dir_with_the_process_id_and_differ() {
    let (a, b) = (temp_path(), temp_path());
    assert_ne!(a, b);
    assert!(a.starts_with(std::env::temp_dir()));
    assert!(
        a.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with(&format!("cpd-dialog-{}-", std::process::id()))
    );
}

#[test]
fn the_popup_runs_the_view_with_every_value_quoted_and_named_by_equals() {
    let command = view_command(
        "confirm",
        "Archive",
        Path::new("/tmp/o.json"),
        &[("text", "It's -x?"), ("yes-label", "Archive")],
    );
    assert_eq!(
        command,
        r"cpd tui view dialog confirm --title='Archive' --out='/tmp/o.json' --text='It'\''s -x?' --yes-label='Archive'"
    );
    assert_eq!(
        popup_args((50, 7), &command),
        [
            "display-popup",
            "-E",
            "-B",
            "-w",
            "50",
            "-h",
            "7",
            command.as_str()
        ]
    );
}

#[test]
fn the_popup_opens_without_tmuxs_border_and_without_a_tmux_title() {
    let command = "cpd tui view dialog text --title='T' --out='/tmp/o.json' --body='B'";
    assert_eq!(
        popup_args((42, 6), command),
        ["display-popup", "-E", "-B", "-w", "42", "-h", "6", command]
    );
    // the popup the kit opens on the newest client, and on a named one (a client of 100 x 30)
    let dir = tempfile::tempdir().unwrap();
    let (tmux, runner) = recording_tmux(&dir);
    let _ = show_error(&tmux, "session open l-ship", "no_cockpit: No cockpit.");
    let popup = popup_of(&runner);
    assert_eq!(
        popup[..7],
        ["display-popup", "-E", "-B", "-w", "42", "-h", "6"],
        "{popup:?}"
    );
    assert!(!popup.contains(&"-T".to_owned()), "{popup:?}");
    let dir = tempfile::tempdir().unwrap();
    let (tmux, runner) = recording_tmux(&dir);
    let _ = show_error_on(
        &tmux,
        "c1",
        "session open l-ship",
        "no_cockpit: No cockpit.",
    );
    let popup = popup_of(&runner);
    assert_eq!(
        popup[..9],
        [
            "display-popup",
            "-c",
            "c1",
            "-E",
            "-B",
            "-w",
            "42",
            "-h",
            "6"
        ],
        "{popup:?}"
    );
    assert!(!popup.contains(&"-T".to_owned()), "{popup:?}");
}

#[test]
fn the_popup_is_sized_to_its_content_with_no_border_rows_and_capped_at_the_client() {
    let client = (200, 60);
    // short content: the narrowest inside (40) and its walls; blank + 1 row + blank + buttons + the two borders = 6
    assert_eq!(popup_size(client, "T", 10, |_| 1), (42, 6));
    // long content: the widest inside (76), wrapped at 74
    assert_eq!(
        popup_size(client, "T", 300, |w| 300usize.div_ceil(w)),
        (78, 10)
    );
    // a long title widens it
    assert_eq!(popup_size(client, &"t".repeat(50), 10, |_| 1).0, 58);
    // capped: a margin of one column each side, the height at the client's
    assert_eq!(popup_size((30, 5), "T", 300, |_| 40), (28, 5));
}

#[test]
fn a_popup_keeps_one_column_of_the_clients_edge_on_each_side() {
    assert_eq!(popup_size((42, 50), "T", 300, |_| 1).0, 40);
    assert_eq!(popup_size((41, 50), "T", 10, |_| 1).0, 39);
    assert_eq!(popup_size((3, 50), "T", 10, |_| 1).0, 1);
    assert_eq!(popup_size((1, 50), "T", 10, |_| 1).0, 1);
    // vertically there is no margin: the height is at most the client's
    assert_eq!(popup_size((200, 6), "T", 10, |_| 1).1, 6);
    assert_eq!(popup_size((200, 5), "T", 10, |_| 1).1, 5);
    assert_eq!(popup_size((200, 4), "T", 10, |_| 40).1, 4);
}

#[test]
fn the_inputs_popup_has_the_worked_sizes_of_the_contract() {
    let client = (200, 60);
    let size = |title, prompt, initial: &str, validator| {
        input_popup_size(client, title, prompt, initial, validator)
    };
    assert_eq!(size("Rename", "Title:", "", Validator::Title), (42, 15));
    assert_eq!(size("Rename", "Title:", "", Validator::Name), (42, 12));
    assert_eq!(size("Rename", "Title:", "", Validator::PlanId), (42, 10));
    let fork = "Fork Plan sketch into a new Plan, name:";
    assert_eq!(fork.len(), 39);
    assert_eq!(size("Fork", fork, "", Validator::PlanId), (47, 10));
    assert_eq!(
        size("Rename", "Name:", &"x".repeat(100), Validator::Name),
        (78, 11)
    );
    assert_eq!(
        input_popup_size((30, 8), "Rename", "Title:", "", Validator::Title),
        (28, 8)
    );
}

#[test]
fn the_widest_line_is_the_longest_in_characters() {
    let (a, b) = (widest("ab\nabcd\nabc"), widest(""));
    assert_eq!((a, b), (4, 0));
}

#[test]
fn the_command_line_takes_the_kinds_and_their_fields() {
    use clap::Parser;
    #[derive(Parser, Debug)]
    struct Cli {
        #[command(flatten)]
        args: Args,
    }
    let parse =
        |argv: &[&str]| Cli::try_parse_from(std::iter::once("dialog").chain(argv.iter().copied()));
    assert!(parse(&["confirm", "--title=T", "--out=/o", "--text=Q?"]).is_ok());
    assert!(
        parse(&[
            "confirm",
            "--title=T",
            "--out=/o",
            "--text=-Q?",
            "--yes-label=Go"
        ])
        .is_ok()
    );
    assert!(
        parse(&[
            "input",
            "--title=T",
            "--out=/o",
            "--prompt=P",
            "--initial=",
            "--validate=plan-id"
        ])
        .is_ok()
    );
    assert!(
        parse(&[
            "input",
            "--title=T",
            "--out=/o",
            "--prompt=P",
            "--validate=name"
        ])
        .is_ok()
    );
    assert!(parse(&["text", "--title=T", "--out=/o", "--body=B"]).is_ok());
    for bad in [
        &["input", "--title=T", "--out=/o", "--prompt=P"][..],
        &[
            "input",
            "--title=T",
            "--out=/o",
            "--prompt=P",
            "--validate=anything",
        ],
        &["confirm", "--title=T", "--text=Q"],
        &["text", "--out=/o", "--body=B"],
        &["other", "--title=T", "--out=/o"],
        &[],
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}
