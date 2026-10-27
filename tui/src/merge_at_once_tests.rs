//! Merge needs no confirmation (decision pro-d111, commands.md §13.2): the Deploy column's merge icon, the Deploy PR
//! menu's Merge and the Inbox `pr-ready` item's Merge run `cpd tui action merge <owner>/<repo>#<n> --client <name>`
//! through tmux `run-shell -b` at once; no `merge` popup opens and `cpd tui view merge` is gone. The Deploy column is driven as
//! `inbox_link_tests.rs` drives it: a store, a scripted pump, the `ManualClock`, a `TestBackend` and a `FakeRunner`
//! that records the tmux calls. No real tmux, `open` or process. The rig is shared with `tree/inbox_merge_tests.rs`.

use std::ffi::OsString;
use std::io;
use std::sync::Arc;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::pump::{Feed, Signal};
use crate::test_text::row;
use crate::view_ports::{Ctx, Input};
use crate::widgets::{prepare, show_with};
use cpd_core::clock::ManualClock;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, FakeRunner, Reply};
use cpd_core::host::spawn::{Launch, Spawn};
use cpd_core::output::Output;
use cpd_core::ports::{Fakes, Ports};
use cpd_core::store::Store;

pub(crate) const WIDTH: u16 = 41;
pub(crate) const HEIGHT: u16 = 30;
/// The body's last column at `WIDTH`: where an item's age ends.
pub(crate) const AGE_END: u16 = 37;
pub(crate) const URL: &str = "https://github.com/acme/api/pull/3";
/// The merge icon (md-source_merge).
pub(crate) const MERGE: &str = "\u{F062D}";

struct NoInput;

impl Input for NoInput {
    fn poll(&mut self, _: std::time::Duration) -> io::Result<bool> {
        panic!("the loop polls ctx.input: its events come from the pump");
    }
    fn read(&mut self) -> io::Result<Event> {
        panic!("the loop reads ctx.input: its events come from the pump");
    }
}

fn no_command(_: Vec<OsString>) -> Result<Output, CpdError> {
    Err(CpdError::new(ErrorCode::StoreBusy, "no command runs in this test"))
}

/// A home with the open pull request `acme/api#3` in `merge_state` and the `pr-ready` Inbox item of it (label `api#3 ready`,
/// with its url: a `pr-ready` item always has one).
pub(crate) fn home_with(merge_state: &str) -> (tempfile::TempDir, CpdHome) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().into()), None).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let sql = format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, merge_state, base_ref, default_branch, created_at, merged_at, changed_at)
         VALUES ('acme/api', 3, '{URL}', 'Title 3', 0, 'open', 'none', 'passed', '{merge_state}', 'main', 'main', '2026-01-03T00:00:00.000Z', NULL, '2026-01-09T00:00:00.000Z');
         INSERT INTO inbox_items (kind, urgency, subject, label, url, opened_at)
         VALUES ('pr-ready', 'yellow', 'pr:acme/api#3', 'api#3 ready', '{URL}', '2026-01-01T00:00:00.000Z');"
    );
    store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
    (dir, home)
}

pub(crate) fn click(column: u16, row: u16) -> Signal {
    mouse(MouseButton::Left, column, row)
}

pub(crate) fn right_click(column: u16, row: u16) -> Signal {
    mouse(MouseButton::Right, column, row)
}

fn mouse(button: MouseButton, column: u16, row: u16) -> Signal {
    Signal::Input(Event::Mouse(MouseEvent { kind: MouseEventKind::Down(button), column, row, modifiers: KeyModifiers::NONE }))
}

pub(crate) fn enter() -> Signal {
    Signal::Input(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)))
}

/// What a scripted run left behind.
pub(crate) struct Seen {
    pub(crate) buf: Buffer,
    /// The arguments of every `open` call.
    pub(crate) opened: Vec<Vec<String>>,
    /// Every call of `tmux`, as its arguments.
    pub(crate) tmux: Vec<Vec<String>>,
}

impl Seen {
    /// The `run-shell` calls: the commands tmux ran at once.
    pub(crate) fn run_shell(&self) -> Vec<&Vec<String>> {
        self.tmux.iter().filter(|a| a.iter().any(|w| w == "run-shell")).collect()
    }

    /// The popups tmux opened.
    pub(crate) fn popups(&self) -> Vec<&Vec<String>> {
        self.tmux.iter().filter(|a| a.iter().any(|w| w == "display-popup")).collect()
    }

    /// Asserts that exactly one `run-shell -b` ran the merge of `acme/api#3` for a client, and that no popup opened.
    pub(crate) fn assert_merged_at_once(&self) {
        assert!(self.popups().is_empty(), "no popup opens: {:?}", self.popups());
        assert!(!self.tmux.iter().flatten().any(|w| w.contains("view merge")), "no `tui view merge`: {:?}", self.tmux);
        let shells = self.run_shell();
        assert_eq!(shells.len(), 1, "one run-shell: {:?}", self.tmux);
        let args = shells[0];
        assert!(args.iter().any(|w| w == "-b"), "in the background: {args:?}");
        let command = args.last().unwrap();
        assert!(command.contains("tui action merge"), "the merge action: {command:?}");
        assert!(command.contains("acme/api##3"), "of the pull request, `#` doubled for run-shell: {command:?}");
        assert!(command.contains("--client"), "for the client: {command:?}");
        assert!(command.contains("/dev/ttys001"), "the newest client's name: {command:?}");
    }
}

fn named(call: &Call, program: &str) -> bool {
    std::path::Path::new(&call.program).file_name().is_some_and(|n| n == program)
}

/// Runs the Deploy column on `home` at `WIDTH` x `HEIGHT` through `script`: `open` answers exit 0, tmux lists one client
/// `/dev/ttys001`. `setup` runs on the fakes before the view reads anything (a live merge, say).
pub(crate) fn run_with_setup(home: &CpdHome, setup: impl FnOnce(&Fakes, &CpdHome), mut script: Vec<Signal>) -> Seen {
    let clock = ManualClock::new();
    let runner = Arc::new(
        FakeRunner::new(clock.clone())
            .on("open", |_| true, Reply::ok(""))
            .on("tmux", |c| c.args.iter().any(|a| a == "list-clients"), Reply::ok("1700000000 200 50 /dev/ttys001\n"))
            .on("tmux", |_| true, Reply::ok("")),
    );
    let fakes = Fakes::new();
    setup(&fakes, home);
    let ports = Ports { clock: Arc::new(clock), runner: runner.clone(), spawn: fakes.spawn.clone() };
    let prepared = prepare(&ports, home.clone()).unwrap();
    script.push(Signal::InputFailed(io::Error::other("gone")));
    let feed = Feed::scripted(ports.clock.clone(), script);
    let mut input = NoInput;
    let mut ctx = Ctx::new(&ports, Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap(), &mut input);
    match show_with(&mut ctx, feed, prepared, no_command) {
        Err(crate::ViewError::Terminal(e)) => assert_eq!(e.to_string(), "gone"),
        Err(crate::ViewError::Cpd(e)) => panic!("the view failed: {}", e.message()),
        Ok(()) => panic!("the script ends the loop"),
    }
    let calls = runner.calls();
    Seen {
        buf: ctx.terminal.backend().buffer().clone(),
        opened: calls.iter().filter(|c| named(c, "open")).map(|c| c.args.clone()).collect(),
        tmux: calls.iter().filter(|c| named(c, "tmux")).map(|c| c.args.clone()).collect(),
    }
}

pub(crate) fn run(home: &CpdHome, script: Vec<Signal>) -> Seen {
    run_with_setup(home, |_, _| {}, script)
}

/// Marks the merge of `acme/api#3` as running: a marker whose pid is alive on the fake spawn.
pub(crate) fn merging(fakes: &Fakes, home: &CpdHome) {
    let pid = fakes.spawn.spawn(&Launch { program: "cpd".into(), args: vec!["pr".into(), "merge".into()], ..Launch::default() }).unwrap();
    let dir = home.run_dir().join("tui/merging");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("acme__api__3.json"), format!(r#"{{"pid": {}, "at": "2026-01-01T00:00:00Z"}}"#, pid.0)).unwrap();
}

/// Where `text` starts in the frame: its column and row.
pub(crate) fn locate(buf: &Buffer, text: &str) -> (u16, u16) {
    for y in 0..buf.area.height {
        let joined = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>();
        if let Some(at) = joined.find(text) {
            return (u16::try_from(joined[..at].chars().count()).unwrap(), y);
        }
    }
    panic!("{text:?} is not in the frame: {:?}", (0..buf.area.height).map(|y| row(buf, y)).collect::<Vec<_>>());
}

/// The rows of the frame, as text.
pub(crate) fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height).map(|y| row(buf, y)).collect()
}

/// Whether some row of the frame holds `text`.
pub(crate) fn has(buf: &Buffer, text: &str) -> bool {
    rows(buf).iter().any(|r| r.contains(text))
}

#[test]
fn merge_icon_click_runs_the_merge_action_at_once_with_no_popup() {
    let (_dir, home) = home_with("clean");
    let (x, y) = locate(&run(&home, vec![]).buf, MERGE);
    let seen = run(&home, vec![click(x, y)]);
    seen.assert_merged_at_once();
}

#[test]
fn deploy_menu_merge_runs_the_merge_action_at_once_with_no_popup() {
    let (_dir, home) = home_with("clean");
    let (x, y) = locate(&run(&home, vec![]).buf, "Title 3");
    let opened = run(&home, vec![right_click(x, y)]);
    let (_, menu_row) = locate(&opened.buf, "Merge");
    let (_, copy_row) = locate(&opened.buf, "Copy Link");
    assert_eq!(copy_row, menu_row + 1, "Merge is the menu's first entry");
    let seen = run(&home, vec![right_click(x, y), enter()]);
    seen.assert_merged_at_once();
}

#[test]
fn plain_click_on_a_pr_ready_row_outside_its_link_does_nothing() {
    use crate::tree::fixture::sample;
    use crate::widgets::inbox::{item, jump};
    use cpd_core::inbox::model::{Kind, Urgency};
    let mut ready = item(1, Kind::PrReady, Urgency::Yellow, "api#3 ready", "2026-01-01T00:00:00.000Z");
    ready.subject = "pr:acme/api#3".to_owned();
    ready.url = Some(URL.to_owned());
    assert_eq!(jump(&sample(), &ready), None, "a plain click on a pr-ready item does nothing");

    let (_dir, home) = home_with("clean");
    let baseline = run(&home, vec![]);
    let (x, y) = locate(&baseline.buf, "api#3 ready");
    for (what, column) in [("the glyph", x - 2), ("the age", AGE_END)] {
        let seen = run(&home, vec![click(column, y)]);
        assert_eq!(seen.tmux, baseline.tmux, "a click on {what} runs nothing and opens no popup");
        assert!(seen.opened.is_empty(), "a click on {what} opens no page: {:?}", seen.opened);
    }
    let seen = run(&home, vec![click(x, y)]);
    assert_eq!(seen.opened, [vec![URL.to_owned()]], "the click on the link opens the pull request page");
    assert_eq!(seen.tmux, baseline.tmux, "and nothing else");
}

#[test]
fn tui_view_merge_is_not_a_command() {
    use clap::Parser;
    #[derive(Parser, Debug)]
    struct Cli {
        #[command(subcommand)]
        view: crate::View,
    }
    let parsed = Cli::try_parse_from(["view", "merge", "acme/api#3", "--client", "/dev/ttys001"]);
    assert!(parsed.is_err(), "`tui view merge <ref>` is refused, got {parsed:?}");
    assert!(Cli::try_parse_from(["view", "tree"]).is_ok(), "the other views still parse");
}
