//! Characterization (pro-d408): a click action whose command fails writes its error to `widgets.log` and opens the text
//! dialog, and the row is not touched (scout-actions, "Shared path").

use std::ffi::OsString;
use std::sync::Arc;

use super::Opener;
use super::inbox::Choice;
use crate::tree::actions::Runner;
use crate::viewlog::ViewLog;
use cpd_core::clock::ManualClock;
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::fakes::tmux::TmuxRoute;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::FakeRunner;
use cpd_core::host::tmux::Tmux;
use cpd_core::output::Output;
use cpd_core::ports::Fakes;

fn refuses(_: Vec<OsString>) -> Result<Output, CpdError> {
    Err(CpdError::new(
        ErrorCode::ItemClosed,
        "Inbox item 7 is closed.",
    ))
}

/// An `Opener` on a temp home whose `run_command` is [`refuses`] and whose tmux is a fake with one client.
fn opener(dir: &tempfile::TempDir) -> (Opener, Arc<FakeRunner>) {
    let home = CpdHome::resolve(Some(dir.path().join("home").into()), None).unwrap();
    std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    let config = Config::load(&home).unwrap();
    let route = Arc::new(TmuxRoute::new());
    let fakes = Fakes::new();
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
    let opener = Opener {
        home: home.clone(),
        busy_ms: 5000,
        run_command: refuses,
        tmux,
        log: ViewLog::of(&home, "widgets"),
        runner: Runner::new(home, &config, &fakes.ports),
    };
    (opener, runner)
}

#[test]
fn a_refused_click_action_logs_its_error_and_opens_a_text_dialog() {
    let dir = tempfile::tempdir().unwrap();
    let (opener, runner) = opener(&dir);
    opener.choose(Choice::Dismiss, 7);
    let logged = std::fs::read_to_string(dir.path().join("home/run/tui/widgets.log")).unwrap();
    assert!(
        logged
            .lines()
            .next()
            .unwrap()
            .ends_with(" failed: inbox dismiss 7: item_closed: Inbox item 7 is closed."),
        "{logged}"
    );
    let calls = runner.calls();
    let popup = calls
        .iter()
        .find(|c| c.args.iter().any(|a| a == "display-popup"))
        .unwrap_or_else(|| panic!("no display-popup in {calls:?}"));
    let command = popup.args.last().unwrap();
    assert!(
        command.starts_with("cpd tui view dialog text --title='inbox dismiss 7' --out="),
        "{command}"
    );
    assert!(
        command.ends_with("--body='item_closed: Inbox item 7 is closed.'"),
        "{command}"
    );
}

#[test]
fn a_refused_click_action_leaves_the_inbox_row_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let (opener, _) = opener(&dir);
    let mut store = cpd_core::store::Store::open(&opener.home, 5000).unwrap();
    let condition = cpd_core::inbox::model::Condition {
        kind: cpd_core::inbox::model::Kind::Dialog,
        urgency: cpd_core::inbox::model::Urgency::Red,
        subject: "core-docs".into(),
        label: "inbox/core-docs failed".into(),
        url: None,
        context: None,
        plan: None,
        job: None,
        session: None,
        record: None,
        event: None,
        opened_at: None,
    };
    cpd_core::inbox::model::reconcile(&mut store, &[condition], "2026-01-01T00:00:00.000Z")
        .unwrap();
    let rows = |store: &mut cpd_core::store::Store| {
        store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT closed_at, snoozed_until, dismissed_at, claimed_at, (SELECT count(*) FROM events) FROM inbox_items WHERE id = 1",
                    [],
                    |r| Ok(format!("{:?}", (r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, i64>(4)?))),
                )?)
            })
            .unwrap()
    };
    let before = rows(&mut store);
    opener.choose(Choice::Dismiss, 1);
    assert_eq!(rows(&mut store), before);
}
