//! `cpd tui view tree` in the test process, with a test that reads like a person at a terminal (docs/implementing.md,
//! "The in-process test kit").
//!
//! The view's loop runs `cpd_tui::tree::run_on` on a thread of its own, on the `Fx`'s fake ports, a [`Display`] and an
//! [`Input`] that hands the loop what the test says. The loop and the test never run at the same time: every call of
//! [`Tree`] that moves the view (a key, a click, a nap) returns only when the loop is polling its input again, with
//! nothing left to do before the next event or the next time. So a test reads the screen and the calls of the fake
//! `cpd` at once, and no assertion waits on the wall clock. What takes time takes the `ManualClock`'s: [`Tree::idle`]
//! lets the loop's poll nap its wait, [`Tree::advance`] moves the clock first, [`Tree::until`] naps until a condition holds.
//!
//! The thread ends with the [`Tree`] (its drop closes the input, which the loop reports as the end of the view) and is
//! joined there; a panic of the view is the test's.

use std::fs::File;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpd_core::fakes::tmux::TmuxRoute;
use cpd_core::clock::Clock;
use cpd_core::error::CpdError;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_tui::palette::SPINNER;
use cpd_tui::view_ports::Input;
use nix::fcntl::{Flock, FlockArg};
use ratatui::Terminal;
use ratatui::crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::fx::Fx;
use super::screen::{Bytes, Display, FakeTty, Screen, ch, key};

/// What the loop is told when its input is closed: the end of the view.
const ENDED: &str = "the test ended the view";

/// The virtual time [`Tree::until`] waits at most.
const LIMIT: Duration = Duration::from_secs(600);

enum Cmd {
    /// An event is there.
    Event(Event),
    /// Nothing comes: the poll naps its wait on the clock and says no.
    Nap,
}

/// The input of the loop, on its thread: every poll tells the test that the loop is idle, then waits for what the test says.
struct Remote {
    clock: Arc<dyn Clock>,
    idle: Sender<()>,
    cmds: Receiver<Cmd>,
    pending: Option<Event>,
    /// Whether a poll while a command runs is a poll like any other.
    eager: Arc<AtomicBool>,
    /// How many snapshots the loop said it took ([`Input::snapshot_read`]).
    snapshots: Arc<AtomicUsize>,
}

impl Remote {
    fn ask(&mut self, wait: Duration) -> io::Result<bool> {
        if self.idle.send(()).is_err() {
            return Err(io::Error::other(ENDED));
        }
        match self.cmds.recv() {
            Ok(Cmd::Event(event)) => {
                self.pending = Some(event);
                Ok(true)
            }
            Ok(Cmd::Nap) => {
                self.clock.sleep(wait);
                Ok(false)
            }
            Err(_) => Err(io::Error::other(ENDED)),
        }
    }
}

impl Input for Remote {
    fn poll(&mut self, wait: Duration) -> io::Result<bool> {
        self.ask(wait)
    }

    fn read(&mut self) -> io::Result<Event> {
        Ok(self.pending.take().expect("the loop reads only what a poll said is there"))
    }

    fn ready_while_busy(&mut self) -> io::Result<bool> {
        if self.eager.load(Ordering::SeqCst) { self.ask(Duration::ZERO) } else { Ok(false) }
    }

    fn snapshot_read(&mut self) {
        self.snapshots.fetch_add(1, Ordering::SeqCst);
    }
}

/// The tree view, running, and the test's hand on it.
pub struct Tree {
    display: Display,
    out: Bytes,
    clock: Arc<cpd_core::clock::ManualClock>,
    home: std::path::PathBuf,
    cmds: Option<Sender<Cmd>>,
    idle: Receiver<()>,
    eager: Arc<AtomicBool>,
    snapshots: Arc<AtomicUsize>,
    thread: Option<JoinHandle<Result<Output, CpdError>>>,
}

/// A view's entry: what [`Tree::run`] calls on the view's thread (`cpd_tui::<view>::run_on`, its arguments bound).
pub type Entry = dyn FnOnce(&Ports, &mut FakeTty, &mut Bytes, Terminal<Display>, &mut dyn Input) -> Result<Output, CpdError> + Send;

impl Tree {
    /// Runs the tree view in a `width` x `height` terminal on the `Fx`'s fake ports and returns when it first waits for input.
    pub fn start(fx: &Fx, width: u16, height: u16) -> Tree {
        Tree::run(fx, "tree", width, height, Box::new(|ports, tty, out, terminal, input| cpd_tui::tree::run_on(ports, tty, out, terminal, input)))
    }

    /// Runs the view `name`, which `entry` runs (its `run_on`), in a `width` x `height` terminal on the `Fx`'s fake ports and
    /// returns when it first waits for input. What is said of the tree here holds of any view.
    pub fn run(fx: &Fx, name: &str, width: u16, height: u16, entry: Box<Entry>) -> Tree {
        let (display, out) = (Display::new(width, height), Bytes::default());
        let (cmds, from_test) = channel();
        let (to_test, idle) = channel();
        let eager = Arc::new(AtomicBool::new(false));
        let snapshots = Arc::new(AtomicUsize::new(0));
        let ports = fx.fakes.ports.clone();
        let mut remote = Remote {
            clock: ports.clock.clone(),
            idle: to_test,
            cmds: from_test,
            pending: None,
            eager: eager.clone(),
            snapshots: snapshots.clone(),
        };
        let (shown, written) = (display.clone(), out.clone());
        let thread = thread::Builder::new()
            .name(format!("{name} view"))
            .spawn(move || {
                let mut written = written;
                // what the view reaches through `Ports::current()` is the fake too, never the real ports
                Ports::scope(&ports, || entry(&ports, &mut FakeTty::new(true), &mut written, shown.terminal(), &mut remote))
            })
            .expect("a thread for the view");
        let mut tree = Tree {
            display,
            out,
            clock: fx.fakes.clock.clone(),
            home: fx.env.cpd_home(),
            cmds: Some(cmds),
            idle,
            eager,
            snapshots,
            thread: Some(thread),
        };
        tree.settle();
        tree
    }

    /// Returns when the loop polls its input, and fails the test, with how the view ended, when it ended instead.
    fn settle(&mut self) {
        if self.idle.recv().is_err() {
            let result = self.join();
            panic!("the view ended: {result:?}");
        }
    }

    fn join(&mut self) -> thread::Result<Result<Output, CpdError>> {
        self.cmds = None;
        self.thread.take().expect("the view's thread is joined once").join()
    }

    fn send(&mut self, cmd: Cmd) {
        let sent = self.cmds.as_ref().expect("the view is running").send(cmd);
        if sent.is_err() {
            let result = self.join();
            panic!("the view ended: {result:?}");
        }
        self.settle();
    }

    /// An event of the terminal.
    pub fn event(&mut self, event: Event) {
        self.send(Cmd::Event(event));
    }

    /// The presses of `keys`, named as tmux names them (`Enter`, `Escape`, `Down`, `Up`, `Space`, a single character).
    pub fn keys(&mut self, keys: &[&str]) {
        for name in keys {
            self.event(named(name));
        }
    }

    /// Types `text`.
    pub fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            self.event(ch(c));
        }
    }

    /// Moves the cursor down `n` rows (the first key shows it).
    pub fn down(&mut self, n: usize) {
        for _ in 0..n {
            self.keys(&["j"]);
        }
    }

    /// A left click at 0-based `column` and `row`: the press and the release.
    pub fn click(&mut self, column: u16, row: u16) {
        self.mouse(MouseButton::Left, column, row);
    }

    /// A right click at 0-based `column` and `row`.
    pub fn right_click(&mut self, column: u16, row: u16) {
        self.mouse(MouseButton::Right, column, row);
    }

    fn mouse(&mut self, button: MouseButton, column: u16, row: u16) {
        for kind in [MouseEventKind::Down(button), MouseEventKind::Up(button)] {
            self.event(Event::Mouse(MouseEvent { kind, column, row, modifiers: KeyModifiers::NONE }));
        }
    }

    /// The loop's poll naps its wait on the clock and finds nothing: what is due by then happens.
    pub fn idle(&mut self) {
        self.send(Cmd::Nap);
    }

    /// Moves the clock forward by `d`, then [`Tree::idle`].
    pub fn advance(&mut self, d: Duration) {
        self.clock.advance(d);
        self.idle();
    }

    /// Naps until `done` holds of the screen's rows, as [`Tree::screen`] gives them; panics with the last screen when
    /// it never holds within [`LIMIT`] of virtual time.
    pub fn until(&mut self, what: &str, done: impl Fn(&[String]) -> bool) -> Vec<String> {
        let start = self.clock.elapsed();
        loop {
            let rows = self.screen();
            if done(&rows) {
                return rows;
            }
            assert!(self.clock.elapsed() - start < LIMIT, "waited {LIMIT:?} virtual time for {what}; the screen:\n{}", rows.join("\n"));
            self.idle();
        }
    }

    /// Naps until `done` holds (it reads the world: a file, the calls of a fake, the store); panics, with the screen,
    /// when it never holds within [`LIMIT`] of virtual time.
    pub fn until_that(&mut self, what: &str, mut done: impl FnMut(&Tree) -> bool) {
        let start = self.clock.elapsed();
        while !done(self) {
            assert!(self.clock.elapsed() - start < LIMIT, "waited {LIMIT:?} virtual time for {what}; the screen:\n{}", self.screen().join("\n"));
            self.idle();
        }
    }

    /// Naps until the log has a line with `text`.
    pub fn until_logged(&mut self, text: &str) {
        let home = self.home.clone();
        self.until_that(&format!("the log line {text:?}"), |_| read_log(&home).contains(text));
    }

    /// Naps until the first snapshot has been drawn: the screen has `text`.
    pub fn until_shows(&mut self, text: &str) -> Vec<String> {
        self.until(text, |rows| rows.iter().any(|r| r.contains(text)))
    }

    /// Whether a poll while a command runs hands the test the loop: the loop goes on reading the keys sent while a
    /// command of the fake `cpd` waits on a [`Gate`]. Off (the default), the loop waits for the command to end.
    pub fn eager(&mut self, eager: bool) {
        self.eager.store(eager, Ordering::SeqCst);
    }

    /// The screen with its styles.
    pub fn styled(&self) -> Screen {
        self.display.screen()
    }

    /// The screen, each row without trailing blanks, without the empty rows at the end. The working dot turns with the
    /// clock (`SPINNER`), so on the row of a working Lead with counters (`1⋅1⋅1⋅2`) every frame reads as the first: a test
    /// cannot know which one it catches. Elsewhere a glyph is what it is.
    pub fn screen(&self) -> Vec<String> {
        let working = SPINNER[0].chars().next().expect("a glyph");
        let mut rows: Vec<String> = (self.display.screen().rows().iter())
            .map(|r| r.trim_end().to_owned())
            .map(|r| if r.ends_with("1⋅1⋅1⋅2") { r.chars().map(|c| if SPINNER.iter().any(|g| g.starts_with(c)) { working } else { c }).collect() } else { r })
            .collect();
        while rows.last().is_some_and(String::is_empty) {
            rows.pop();
        }
        rows
    }

    /// Whether a row of the screen has `text`.
    pub fn shows(&self, text: &str) -> bool {
        self.screen().iter().any(|r| r.contains(text))
    }

    /// How many times the view flushed its terminal so far: one per frame it drew, even a frame that changed nothing.
    pub fn flushes(&self) -> usize {
        self.display.flushes()
    }

    /// How many snapshots of the store the loop took so far (the first read, before the first frame, is not one).
    pub fn snapshots(&self) -> usize {
        self.snapshots.load(Ordering::SeqCst)
    }

    /// Everything the view wrote to its terminal outside its frame: the modes it set, and unset.
    pub fn written(&self) -> Vec<u8> {
        self.out.bytes()
    }

    /// The view's log, `run/tui/tree.log`: empty before its first line.
    pub fn log(&self) -> String {
        read_log(&self.home)
    }
}

fn read_log(home: &Path) -> String {
    std::fs::read_to_string(home.join("run/tui/tree.log")).unwrap_or_default()
}

// ---------- the fixtures every tree test has ----------

/// An `Fx` on fake ports with the config the tree needs (`cpd` is faked: no real binary, no tmux server).
pub fn fx() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_models();
    // The socket is the env's own, the one `Fx::tmux` and `dialogs` start the fake server on.
    fx.env.write_config(&format!(
        "version = 1\nmachine = \"m1\"\n[events]\npoll_ms = 50\n[tmux]\nsocket = \"{}\"\n",
        fx.env.tmux_socket()
    ));
    fx
}

/// The engine's lock, held until the guard drops (`engine-down` is no row of the Inbox); declare it after the `Fx`.
pub fn engine(fx: &Fx) -> Flock<File> {
    let path = fx.env.cpd_home().join("locks/watch.lock");
    std::fs::create_dir_all(path.parent().expect("the locks directory")).expect("create the locks directory");
    Flock::lock(File::create(path).expect("the engine lock file"), FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).expect("lock the engine")
}

/// A fake `cpd` that succeeds with `reply`, and a cockpit with no pane and no client (see [`fake_cockpit`]).
pub fn fake_cpd(fx: &Fx, reply: &str) {
    cpd_says(fx, reply);
    fake_cockpit(fx);
}

/// A fake `cpd` that fails with `reply` (exit 3), and a cockpit with no pane and no client.
pub fn fake_cpd_fails(fx: &Fx, reply: &str) {
    cpd_fails(fx, reply);
    fake_cockpit(fx);
}

/// A fake `cpd` that succeeds with `reply`: the tmux route stays the fake server's (see [`dialogs`]).
pub fn cpd_says(fx: &Fx, reply: &str) {
    fx.fake("cpd", Reply::ok(reply));
}

/// A fake `cpd` that fails with `reply` (exit 3): the tmux route stays the fake server's.
pub fn cpd_fails(fx: &Fx, reply: &str) {
    fx.fake("cpd", Reply::exit(3, reply, ""));
}

/// The popups of the dialog kit on the fake tmux server of an `Fx` ([`dialogs`]): the User's answer to the next ones, and
/// the commands the popups ran. (The popup's own drawing and keys are `tui_dialog.rs`'s.)
pub struct Dialogs {
    answer: Arc<Mutex<String>>,
    route: Arc<TmuxRoute>,
    socket: String,
}

/// A fake tmux server with one client attached, whose `display-popup` plays the dialog kit's popup: it writes the answer
/// ([`Dialogs::confirm`], [`Dialogs::cancel`], [`Dialogs::type_in`]; a cancel until one is given) to the `--out` file of the
/// popup's command, as the view would (`FakeTmux::on_popup`); every other command is the fake's.
/// One helper for the tree files; declare it before the [`Tree`].
pub fn dialogs(fx: &Fx) -> Dialogs {
    let ok = |args: &[&str]| {
        let out = fx.tmux(args);
        assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    };
    ok(&["new-session", "-d", "-s", "cpd", "-x", "200", "-y", "60"]);
    fx.tmux_route.fake().add_client("/dev/pts/9", 200, 60);
    let dialogs = Dialogs { answer: Arc::default(), route: fx.tmux_route.clone(), socket: fx.env.tmux_socket().to_owned() };
    dialogs.cancel();
    let answer = dialogs.answer.clone();
    fx.tmux_route.fake().on_popup(move |_| Some(answer.lock().unwrap_or_else(PoisonError::into_inner).clone()));
    dialogs
}

impl Dialogs {
    fn answer(&self, json: &serde_json::Value) {
        *self.answer.lock().unwrap_or_else(PoisonError::into_inner) = json.to_string();
    }

    /// The next popups are confirmed (a `confirm`'s button, an `input`'s Enter with no text).
    pub fn confirm(&self) {
        self.answer(&serde_json::json!({"confirmed": true}));
    }

    /// The next popups are cancelled.
    pub fn cancel(&self) {
        self.answer(&serde_json::json!({"confirmed": false}));
    }

    /// The next `input` popups accept `text`.
    pub fn type_in(&self, text: &str) {
        self.answer(&serde_json::json!({"confirmed": true, "value": text}));
    }

    /// The next `form` popups accept the title `text` and the owner `choice`.
    pub fn fill(&self, text: &str, choice: &str) {
        self.answer(&serde_json::json!({"confirmed": true, "value": text, "choice": choice}));
    }

    /// Every popup opened so far: the command each ran, without its `--out` (a temporary file).
    pub fn popups(&self) -> Vec<String> {
        self.raw().iter().map(|p| without_out(p.last().expect("a popup has a command"))).collect()
    }

    /// The argv of every `display-popup` so far (the subcommand name first).
    pub fn raw(&self) -> Vec<Vec<String>> {
        self.route.fake().state(&self.socket).popups()
    }

    /// Waits, in virtual time, until the view has handled what was sent before: `?` opens the legend's popup, the mark.
    pub fn settled(&self, tree: &mut Tree) {
        let before = self.raw().len();
        tree.keys(&["?"]);
        tree.until_that("the legend's popup", |_| self.raw().len() > before);
        let popups = self.raw();
        assert_eq!(popups.len(), before + 1, "{popups:?}");
        assert!(popups[before].contains(&" legend ".to_owned()), "the legend's: {:?}", popups[before]);
    }
}

/// `command` without its ` --out='<path>'`, when it has one.
fn without_out(command: &str) -> String {
    const OUT: &str = " --out='";
    let Some(start) = command.find(OUT) else { return command.to_owned() };
    let end = start + OUT.len() + command[start + OUT.len()..].find('\'').expect("the closing quote of --out");
    format!("{}{}", &command[..start], &command[end + 1..])
}

/// A tmux server that lists no pane and no client: what a command that opens a Session reads, and what a dialog needs
/// a client for (it is then logged, not shown).
pub fn fake_cockpit(fx: &Fx) {
    fx.fake("tmux", Reply::ok(""));
}

/// What the tmux calls so far ask for: the verb of each (`list-panes`, `display-popup`, ...).
pub fn tmux_verbs(fx: &Fx) -> Vec<String> {
    // `tmux -u -L <socket> -f /dev/null <verb> ...`
    fx.calls("tmux").iter().map(|call| call.args.get(5).cloned().unwrap_or_default()).collect()
}

/// The arguments of every `cpd` call so far.
pub fn calls(fx: &Fx) -> Vec<Vec<String>> {
    fx.calls("cpd").into_iter().map(|call| call.args).collect()
}

pub fn words(w: &[&str]) -> Vec<String> {
    w.iter().map(|s| (*s).to_owned()).collect()
}

/// The clock's time plus `secs`, as the store writes a time (contract §1.3).
pub fn stamp(fx: &Fx, secs: i64) -> String {
    cpd_core::clock::timestamp(fx.fakes.clock.utc() + time::Duration::seconds(secs))
}

impl Drop for Tree {
    fn drop(&mut self) {
        if self.thread.is_none() {
            return;
        }
        let result = self.join();
        if thread::panicking() {
            return;
        }
        match result {
            Err(panic) => std::panic::resume_unwind(panic),
            Ok(Err(error)) if error.message().contains(ENDED) => {}
            Ok(other) => panic!("the view ended by itself: {other:?}"),
        }
    }
}

/// An event named as tmux's `send-keys` names it.
pub fn named(name: &str) -> Event {
    let code = match name {
        "Enter" => KeyCode::Enter,
        "Escape" => KeyCode::Esc,
        "Down" => KeyCode::Down,
        "Up" => KeyCode::Up,
        "Space" => KeyCode::Char(' '),
        "Tab" => KeyCode::Tab,
        one if one.chars().count() == 1 => return ch(one.chars().next().expect("one character")),
        other => panic!("no key named {other:?}"),
    };
    key(code)
}

/// A fake `cpd` that answers `reply` when the test opens the gate: the call waits (a real wait, for the test, not for
/// the clock) until [`Gate::open`], or until the gate is dropped.
pub struct Gate {
    opened: Mutex<Receiver<()>>,
    reply: Reply,
}

impl Gate {
    /// A gate with its key: `open` lets every waiting call of the fake answer `reply`.
    pub fn new(reply: Reply) -> (Arc<Gate>, GateKey) {
        let (send, recv) = channel();
        (Arc::new(Gate { opened: Mutex::new(recv), reply }), GateKey(Some(send)))
    }
}

impl Handler for Gate {
    fn handle(&self, _call: &Call) -> Reply {
        // `Err` is the key dropped: the test is over, the call goes on
        let _ = self.opened.lock().unwrap_or_else(PoisonError::into_inner).recv();
        self.reply.clone()
    }
}

/// What opens a [`Gate`]; dropping it opens it too. A test that holds one declares it after its [`Tree`], so that it
/// drops first: a view that waits on a command that waits on the gate cannot end.
pub struct GateKey(Option<Sender<()>>);

impl GateKey {
    /// Lets the waiting call answer.
    pub fn open(mut self) {
        if let Some(send) = self.0.take() {
            let _ = send.send(());
        }
    }
}
