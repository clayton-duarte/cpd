//! The Inbox section in the `widgets` view (commands.md §13.2, pro-d55): the store the tests prepare, the rows the section
//! draws at `W` columns, and the view's loop run on a script that records every frame it draws.
//!
//! The view runs in the test process (`cpd_tui::widgets::show_with`) on the `ManualClock`, with a scripted pump that
//! hands over `Signal`s in order and no sleep. A [`World`] is what the world does while it draws: a step runs at the first
//! frame that satisfies its condition. [`show`] returns every frame, so a test asserts on rows after the view has ended.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::Path;
use std::rc::Rc;

use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::Reply;
use cpd_core::ids::{ContextId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::store::Store;
use cpd_tui::ViewError;
use nix::fcntl::{Flock, FlockArg};
use cpd_tui::palette;
use cpd_tui::pump::{Feed, Signal};
use cpd_tui::view_ports::Ctx;
use cpd_tui::widgets;
use ratatui::crossterm::event::{Event, KeyCode, MouseEventKind};

use super::fx::Fx;
use super::screen::{NoInput, Screen, SharedBackend, left_click, wheel};
use super::tree::stamp;
use super::{TestEnv, sql};

/// The pane's width in the tests: the title's `flush_title` and the item rows are laid out for it.
pub const W: usize = 46;

/// Row 1 of an item (trimmed): ` <glyph> <label>`, spaces, and the `age` ending at column `W - 2`; the last column is blank.
/// Every item is two rows now: [`item_rows`] returns both.
pub fn item_row(glyph: &str, label: &str, age: &str) -> String {
    format!(" {glyph} {label}{}{age}", " ".repeat(W - 4 - label.chars().count() - age.chars().count()))
}

/// The two rows of an item (trimmed): Row 1 is [`item_row`]; Row 2 is three spaces then the `subtitle` (the column, then the
/// two-space lead), or empty when the `subtitle` is (a one-row label).
pub fn item_rows(glyph: &str, label: &str, subtitle: &str, age: &str) -> [String; 2] {
    let second = if subtitle.is_empty() { String::new() } else { format!("   {subtitle}") };
    [item_row(glyph, label, age), second]
}

/// [`item_row`] (Row 1 only) of a link kind: the label is followed by a space and the link cue, and the age keeps its column.
/// The link covers Row 1 only.
pub fn linked_item_row(glyph: &str, label: &str, age: &str) -> String {
    item_row(glyph, &format!("{label} {}", cpd_tui::link::CUE), age)
}

// ---------- the prepared store ----------

pub fn open(env: &TestEnv) -> Store {
    open_at(&env.cpd_home())
}

/// The store of the home `home`, for a step of a [`World`] (which owns what it captures).
pub fn open_at(home: &Path) -> Store {
    Store::open(&CpdHome::resolve(Some(home.into()), None).unwrap(), 5000).unwrap()
}

/// The engine's lock of the home `home`, held until the guard drops (`engine-down` is no item): `tree::engine` for a step of a
/// [`World`], which cannot hold the `Fx`.
pub fn engine_at(home: &Path) -> Flock<File> {
    let path = home.join("locks/watch.lock");
    std::fs::create_dir_all(path.parent().expect("the locks directory")).expect("create the locks directory");
    Flock::lock(File::create(path).expect("the engine lock file"), FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).expect("lock the engine")
}

/// An `Fx` whose config is the one a view needs (machine `m1`, the private tmux server) and whose machine reads fail: the
/// machine row shows the host's name and no usage.
pub fn view_fx() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_tmux_config("");
    fx.fake("hostname", Reply::ok("homeserver\n"));
    for probe in ["sysctl", "ioreg"] {
        fx.fake(probe, Reply::exit(1, "", ""));
    }
    fx
}

/// A Context (the Inbox's items belong to one).
fn context(id: &str) -> String {
    format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on)
         VALUES ('{id}', 'Title of {id}', NULL, '2026-01-01T00:00:00.000Z', 'm1');"
    )
}

/// A Plan of `context` in `phase` and its one PR row, whose Lead is the Session `lead` (insert the Session too). The Plan is
/// created at `created`.
pub fn plan_row(id: &str, context: &str, phase: &str, lead: &str, created: &str) -> String {
    let late = !matches!(phase, "grounding" | "plan");
    let worktree = if late { "'/wt'" } else { "NULL" };
    let locked = if late { "'2026-01-02T00:00:00.000Z'" } else { "NULL" };
    let status = if late { "dispatch" } else { "waiting" };
    format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
         VALUES ('{id}', '{context}', 'Plan {id}', '{phase}', '{lead}', '{created}', {locked}, NULL);
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                          pull_request, created_txn, created_at, started_at, merged_at)
         VALUES ('{id}', 'repo', '/repo/{id}', 'main', '{status}', 'cpd/{id}', {worktree}, NULL, NULL, NULL,
                 NULL, cpd_txn(), '{created}', {locked}, NULL);"
    )
}

/// An idle Session `id` of `role` in `context` (a Lead's `plan`, or `""`), created at `created`.
pub fn session_row(id: &str, role: &str, context: &str, plan: &str, created: &str) -> String {
    let plan = if plan.is_empty() { "NULL".to_owned() } else { format!("'{plan}'") };
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('{id}', '{role}', '{context}', {plan}, NULL, '{id} name', '/sessions/{id}.jsonl', '/repo', 'O3',
                 NULL, 'idle', NULL, '2026-01-03T00:00:00.000Z', '{created}');"
    )
}

/// What the engine's E8 would open: an item of the Context `alpha` named `label`.
pub fn condition(kind: Kind, urgency: Urgency, subject: &str, label: &str) -> Condition {
    Condition {
        kind,
        urgency,
        subject: subject.to_owned(),
        label: label.to_owned(),
        url: None,
        context: Some(ContextId::parse("alpha").unwrap()),
        plan: None,
        job: None,
        session: None,
        record: None,
        event: None,
        opened_at: None,
    }
}

/// The glyph and label (Row 1) of the items of [`four`], in the order they are drawn; each is one-row, so its Row 2 is blank.
pub const FOUR: [(&str, &str); 4] = [
    (palette::TIER_RED, "f-blocked label"),
    (palette::TIER_RED, "l-rel label"),
    (palette::TIER_MAGENTA, "m-beta label"),
    (palette::TIER_WHITE, "m-alpha label"),
];

/// Four items: two red, one magenta, one white (their order on screen is [`FOUR`]'s).
pub fn four() -> Vec<Condition> {
    vec![
        condition(Kind::Dialog, Urgency::Red, "session:f-blocked", "f-blocked label"),
        condition(Kind::Dialog, Urgency::Red, "session:l-rel", "l-rel label"),
        condition(Kind::Unread, Urgency::White, "session:m-alpha", "m-alpha label"),
        condition(Kind::Dialog, Urgency::Magenta, "session:m-beta", "m-beta label"),
    ]
}

/// `n` red one-row items (Row 2 blank) labelled `item 00`, `item 01`, … (drawn in that order).
pub fn many(n: usize) -> Vec<Condition> {
    (0..n).map(|i| condition(Kind::Dialog, Urgency::Red, &format!("session:item-{i:02}"), &format!("item {i:02}"))).collect()
}

/// The Contexts `alpha` and `beta`, and `items` opened `age` seconds before the clock's time.
pub fn seed(fx: &Fx, items: &[Condition], age: i64) {
    let mut store = open(&fx.env);
    sql(&mut store, &(context("alpha") + &context("beta")));
    reconcile(&mut store, items, &stamp(fx, -age)).unwrap();
}

/// A Manager Session of `context` (contract §3.1): with one live, `engine-down` is no item.
pub fn manager(env: &TestEnv, context: &str) {
    let id = SessionId::parse(&format!("m-{context}")).unwrap();
    sql(
        &mut open(env),
        &format!(
            "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
             VALUES ('{id}', 'manager', '{context}', NULL, NULL, '{context}-manager', '/sessions/{id}.jsonl', '/repo', 'O5',
                     NULL, 'idle', NULL, '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:07.000Z');"
        ),
    );
}

/// One open, authored pull request of `repo`, created at the time that `number` orders (oldest first).
fn pull_request(repo: &str, number: i64) -> String {
    let created = format!("2026-01-03T00:{number:02}:00.000Z");
    format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
         VALUES ('{repo}', {number}, 'https://github.com/{repo}/pull/{number}', 'A title', 0, 'open', 'none', 'passed',
                 '{created}', NULL, '2026-02-02T00:00:00.000Z');"
    )
}

/// The open rows `acme/repo#1` … `acme/repo#n`.
pub fn pull_requests(env: &TestEnv, n: i64) {
    let rows: String = (1..=n).map(|i| pull_request("acme/repo", i)).collect();
    sql(&mut open(env), &rows);
}

// ---------- the signals ----------

/// The end of a script: the terminal fails.
pub fn gone() -> Signal {
    Signal::InputFailed(io::Error::other("gone"))
}

pub fn click(column: u16, row: u16) -> Signal {
    Signal::Input(left_click(column, row))
}

pub fn notch(kind: MouseEventKind, column: u16, row: u16) -> Signal {
    Signal::Input(wheel(kind, column, row))
}

pub fn press(code: KeyCode) -> Signal {
    Signal::Input(super::screen::key(code))
}

pub fn typed(c: char) -> Signal {
    Signal::Input(super::screen::ch(c))
}

/// An event of the terminal.
pub fn input(event: Event) -> Signal {
    Signal::Input(event)
}

// ---------- the view, run ----------

/// What the world does while the view draws: each step runs at the first frame, after its predecessors, that satisfies its
/// condition (at most one step a frame). A step no frame reaches is not an error here: the test's assertions on the frames
/// say what did not happen.
#[derive(Clone, Default)]
pub struct World {
    steps: Rc<RefCell<VecDeque<(Box<dyn Fn(&Screen) -> bool>, Box<dyn FnMut()>)>>>,
}

impl World {
    pub fn new() -> World {
        World::default()
    }

    /// At the first frame whose rows satisfy `when`: `then` runs.
    pub fn at(self, when: impl Fn(&[String]) -> bool + 'static, then: impl FnMut() + 'static) -> World {
        self.steps.borrow_mut().push_back((Box::new(move |s| when(&s.shown_rows())), Box::new(then)));
        self
    }
}

/// The root command tree, in this process, on the ports the view runs on.
fn run_command(argv: Vec<OsString>) -> Result<Output, CpdError> {
    cpd::cli::run_with(&Ports::current(), argv)
}

/// Runs the `widgets` view's loop on the store of `fx` in a `W` x `height` pane (one thread, the `ManualClock`, a scripted
/// pump handing `signals` over in order) and returns every frame it drew, first to last. The script ends the view with
/// [`gone`].
pub fn show(fx: &Fx, height: u16, signals: Vec<Signal>, world: &World) -> Vec<Screen> {
    show_with(fx, height, signals, world, run_command)
}

/// [`show`] with `run_command` as what a click runs `cpd` through (the in-process command tree, or a test's own).
pub fn show_with(fx: &Fx, height: u16, signals: Vec<Signal>, world: &World, run_command: cpd_tui::RunCommand) -> Vec<Screen> {
    show_over(fx, height, signals, world, run_command, &[])
}

/// [`show`] on a machine whose `/proc` files are `files` (path, text; Linux reads them): what is not scripted is unreadable.
pub fn show_proc(fx: &Fx, height: u16, signals: Vec<Signal>, world: &World, files: &[(&str, &str)]) -> Vec<Screen> {
    show_over(fx, height, signals, world, run_command, files)
}

fn show_over(fx: &Fx, height: u16, signals: Vec<Signal>, world: &World, run_command: cpd_tui::RunCommand, files: &[(&str, &str)]) -> Vec<Screen> {
    let files: std::collections::HashMap<String, String> = files.iter().map(|(p, t)| ((*p).to_owned(), (*t).to_owned())).collect();
    let proc: widgets::ProcReader = std::sync::Arc::new(move |path| files.get(path).cloned());
    let backend = SharedBackend::new(W as u16, height);
    let frames = Rc::new(RefCell::new(Vec::new()));
    {
        let (frames, steps) = (Rc::clone(&frames), Rc::clone(&world.steps));
        backend.on_frame(move |screen| {
            frames.borrow_mut().push(screen.clone());
            let step = {
                let mut steps = steps.borrow_mut();
                match steps.front() {
                    Some((when, _)) if when(screen) => steps.pop_front(),
                    _ => None,
                }
            };
            if let Some((_, mut then)) = step {
                then();
            }
        });
    }
    let ports = &fx.fakes.ports;
    let ended = Ports::scope(ports, || {
        let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
        let prepared = widgets::prepare_with(ports, home, proc).unwrap();
        let mut input = NoInput;
        let mut ctx = Ctx::new(ports, backend.terminal(), &mut input);
        widgets::show_with(&mut ctx, Feed::scripted(ports.clock.clone(), signals), prepared, run_command)
    });
    match ended {
        Err(ViewError::Terminal(e)) => assert_eq!(e.to_string(), "gone", "the view's terminal failed"),
        Err(ViewError::Cpd(e)) => panic!("the view's own work failed: {}", e.message()),
        Ok(()) => panic!("the view ended with no terminal failure: no key ends it"),
    }
    let frames = frames.borrow().clone();
    frames
}

// ---------- reading the frames ----------

/// The index, from `from` on, of the first frame whose rows satisfy `pred`; the failure names what it looked for and shows
/// the last frame.
pub fn frame_with(frames: &[Screen], from: usize, what: &str, pred: impl Fn(&[String]) -> bool) -> usize {
    let found = (from..frames.len()).find(|&i| pred(&frames[i].shown_rows()));
    assert!(
        found.is_some(),
        "no frame from #{from} on shows {what}; {} frames, the last:\n{}",
        frames.len(),
        frames.last().map(|f| f.shown_rows().join("\n")).unwrap_or_default()
    );
    found.unwrap_or_default()
}

/// [`frame_with`] for exact rows: the first frame from `from` on whose last rows are `bottom`.
pub fn frame_ending_with(frames: &[Screen], from: usize, what: &str, bottom: &[String]) -> usize {
    let what = format!("{what}, its last rows being\n{}\n", bottom.join("\n"));
    frame_with(frames, from, &what, |rows| rows.len() >= bottom.len() && rows[rows.len() - bottom.len()..] == *bottom)
}

/// The rows of frame `i`.
pub fn rows(frames: &[Screen], i: usize) -> Vec<String> {
    frames[i].shown_rows()
}
