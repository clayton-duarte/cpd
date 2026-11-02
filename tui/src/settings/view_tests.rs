//! Red tests of the Agent Tiers view: the drawn rows (docs/commands.md §13.2, the three screens), the inline list, the
//! scrolling, the mouse, and the open and save sequences through fake commands. The loop runs on `Fakes`, a scripted
//! input that clicks on what is drawn (it finds the text on the screen at that moment) and a `TestBackend` it shares
//! with the test. The commands are a fake `RunFn` that records every argv (the words after `cpd`).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::rc::Rc;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::{Backend, ClearType, TestBackend, WindowSize};
use ratatui::buffer::{Buffer, Cell};
use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::layout::{Position, Rect, Size};
use ratatui::widgets::Widget;
use serde_json::{Value, json};

use super::run_on;
use crate::box_widget::SectionTitle;
use crate::palette;
use crate::test_text::rows as buffer_rows;
use crate::view_ports::{Input, Tty};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::output::Output;
use cpd_core::ports::Fakes;

// ---- the doubles ---------------------------------------------------------------------------------------------------

/// A `TestBackend` the test can read while the loop owns the terminal.
#[derive(Clone)]
struct Shared(Rc<RefCell<TestBackend>>);

impl Shared {
    fn new(width: u16, height: u16) -> Shared {
        Shared(Rc::new(RefCell::new(TestBackend::new(width, height))))
    }
    fn buffer(&self) -> Buffer {
        self.0.borrow().buffer().clone()
    }
}

impl Backend for Shared {
    type Error = io::Error;
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.0.borrow_mut().draw(content).map_err(io::Error::other)
    }
    fn hide_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn show_cursor(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(Position::new(0, 0))
    }
    fn set_cursor_position<P: Into<Position>>(&mut self, _: P) -> io::Result<()> {
        Ok(())
    }
    fn clear(&mut self) -> io::Result<()> {
        self.0.borrow_mut().clear().map_err(io::Error::other)
    }
    fn clear_region(&mut self, t: ClearType) -> io::Result<()> {
        self.0
            .borrow_mut()
            .clear_region(t)
            .map_err(io::Error::other)
    }
    fn size(&self) -> io::Result<Size> {
        self.0.borrow().size().map_err(io::Error::other)
    }
    fn window_size(&mut self) -> io::Result<WindowSize> {
        self.0.borrow_mut().window_size().map_err(io::Error::other)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A terminal that is one.
struct Tty0;

impl Tty for Tty0 {
    fn is_terminal(&self) -> bool {
        true
    }
    fn raw_on(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn raw_off(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// One thing the User does, or the test looks at.
enum Act {
    Key(KeyCode),
    Type(&'static str),
    /// A left click on the first cell of `target` in the first row that holds `anchor`.
    Click(&'static str, &'static str),
    Wheel(MouseEventKind),
    /// The test takes the screen as it is, and how many commands have run.
    Snap,
}

/// What a [`Act::Snap`] took.
struct Snap {
    buffer: Buffer,
    calls: usize,
}

impl Snap {
    fn rows(&self) -> Vec<String> {
        buffer_rows(&self.buffer)
    }
}

/// The scripted [`Input`]: an event is there while acts remain; a script that is over is a loop that did not end.
struct Script {
    acts: VecDeque<Act>,
    screen: Shared,
    calls: Rc<RefCell<Vec<Vec<String>>>>,
    snaps: Rc<RefCell<Vec<Snap>>>,
}

impl Script {
    fn event_of(&self, act: Act) -> Event {
        let key = |code| {
            Event::Key(KeyEvent {
                code,
                modifiers: KeyModifiers::NONE,
                kind: KeyEventKind::Press,
                state: KeyEventState::NONE,
            })
        };
        let mouse = |kind, column, row| {
            Event::Mouse(MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            })
        };
        match act {
            Act::Key(code) => key(code),
            Act::Type(_) | Act::Snap => unreachable!("handled by the caller"),
            Act::Wheel(kind) => mouse(kind, 10, 5),
            Act::Click(anchor, target) => {
                let rows = buffer_rows(&self.screen.buffer());
                let y = rows
                    .iter()
                    .position(|r| r.contains(anchor))
                    .unwrap_or_else(|| panic!("no row shows {anchor:?}:\n{}", rows.join("\n")));
                let byte = rows[y]
                    .find(target)
                    .unwrap_or_else(|| panic!("{target:?} is not in {:?}", rows[y]));
                let x = rows[y][..byte].chars().count();
                mouse(
                    MouseEventKind::Down(MouseButton::Left),
                    x as u16 + 1,
                    y as u16,
                )
            }
        }
    }

    /// Takes the snaps at the front of the script, and expands a typed text into its keys.
    fn settle(&mut self) {
        loop {
            match self.acts.front() {
                Some(Act::Snap) => {
                    self.acts.pop_front();
                    let snap = Snap {
                        buffer: self.screen.buffer(),
                        calls: self.calls.borrow().len(),
                    };
                    self.snaps.borrow_mut().push(snap);
                }
                Some(Act::Type(text)) => {
                    let text = *text;
                    self.acts.pop_front();
                    for c in text.chars().rev() {
                        self.acts.push_front(Act::Key(KeyCode::Char(c)));
                    }
                }
                _ => return,
            }
        }
    }
}

impl Input for Script {
    fn poll(&mut self, _: Duration) -> io::Result<bool> {
        self.settle();
        assert!(
            !self.acts.is_empty(),
            "the script is over and the loop still polls"
        );
        Ok(true)
    }
    fn read(&mut self) -> io::Result<Event> {
        self.settle();
        let act = self
            .acts
            .pop_front()
            .expect("the script is over and the loop still reads");
        Ok(self.event_of(act))
    }
}

// ---- the commands --------------------------------------------------------------------------------------------------

const BIFROST: [&str; 4] = [
    "bifrost/vertex/gemini-4-argon",
    "bifrost/vertex/claude-sonnet-5-5",
    "bifrost/vertex/claude-haiku-5-5",
    "bifrost/vertex/gemini-3.8-flash",
];

fn config() -> Value {
    json!({"config": {
        "context_distill_tier": "GX", "worker_tier": "H3", "lead_tier": "S4", "manager_tier": "O3",
        "presets": {"bifrost": {"O": BIFROST[0], "S": BIFROST[1], "H": BIFROST[2], "G": BIFROST[3]}},
    }})
}

fn tiers() -> Value {
    json!({"pi_version": "1.5.0", "tiers": {
        "O": {"model": BIFROST[0], "efforts": [0, 2, 3, 4]},
        "S": {"model": BIFROST[1], "efforts": [0, 1, 2, 3, 4]},
        "H": {"model": BIFROST[2], "efforts": [0, 1, 2, 3, 4]},
        "G": {"model": BIFROST[3], "efforts": []},
    }})
}

fn models() -> Value {
    json!({"providers": [
        {"provider": "bifrost", "ids": ["vertex/gemini-4-argon", "vertex/claude-sonnet-5-5",
            "vertex/claude-haiku-5-5", "vertex/gemini-3.8-flash"]},
        {"provider": "github-copilot", "ids": ["claude-opus-5.5", "claude-sonnet-5.5",
            "claude-haiku-4.5", "gemini-3.8-flash"]},
    ]})
}

fn refusal(code: ErrorCode, message: &str) -> CpdError {
    CpdError::new(code, message)
}

/// The fake commands: every argv recorded; `fail` answers an argv (by its first words) with an error, the first
/// `times` times it runs.
struct Commands {
    calls: Rc<RefCell<Vec<Vec<String>>>>,
    fail: RefCell<Vec<(Vec<&'static str>, usize, CpdError)>>,
}

impl Commands {
    fn new() -> Commands {
        Commands {
            calls: Rc::default(),
            fail: RefCell::default(),
        }
    }

    fn failing(self, prefix: &[&'static str], times: usize, error: CpdError) -> Commands {
        self.fail.borrow_mut().push((prefix.to_vec(), times, error));
        self
    }

    fn run(&self, argv: Vec<OsString>) -> Result<Output, CpdError> {
        let words: Vec<String> = argv
            .iter()
            .map(|w| w.to_string_lossy().into_owned())
            .collect();
        self.calls.borrow_mut().push(words.clone());
        for (prefix, times, error) in self.fail.borrow_mut().iter_mut() {
            if *times > 0
                && words.len() >= prefix.len()
                && words.iter().zip(prefix.iter()).all(|(w, p)| w == p)
            {
                *times -= 1;
                return Err(error.clone());
            }
        }
        let first: Vec<&str> = words.iter().map(String::as_str).collect();
        Ok(match first.as_slice() {
            ["config"] => Output::success(config()),
            ["tiers", ..] => Output::success(tiers()),
            ["models", "show"] => Output::success(models()),
            ["preset", "set", ..] => Output::success(json!({"preset": "set"})),
            ["config", "tiers", "set", ..] => Output::success(json!({"tiers": "set"})),
            ["session", "retier", "--all"] => Output::success(json!({"retiered": []})),
            other => panic!("the view ran a command nobody expects: {other:?}"),
        })
    }
}

fn call(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| (*w).to_owned()).collect()
}

const READS: [&[&str]; 3] = [&["config"], &["tiers"], &["models", "show"]];

fn reads() -> Vec<Vec<String>> {
    READS.iter().map(|c| call(c)).collect()
}

fn candidates(o: &str) -> Vec<String> {
    call(&[
        "tiers",
        "--model",
        &format!("O={o}"),
        "--model",
        &format!("S={}", BIFROST[1]),
        "--model",
        &format!("H={}", BIFROST[2]),
        "--model",
        &format!("G={}", BIFROST[3]),
    ])
}

/// What a run gave: its result, the argv list, the snaps, and the last screen.
struct Run {
    result: Result<Output, CpdError>,
    calls: Vec<Vec<String>>,
    snaps: Vec<Snap>,
    screen: Shared,
}

fn drive(commands: &Commands, size: (u16, u16), acts: Vec<Act>) -> Run {
    let fakes = Fakes::new();
    let screen = Shared::new(size.0, size.1);
    let snaps = Rc::new(RefCell::new(Vec::new()));
    let mut input = Script {
        acts: acts.into(),
        screen: screen.clone(),
        calls: Rc::clone(&commands.calls),
        snaps: Rc::clone(&snaps),
    };
    let run = |argv: Vec<OsString>| commands.run(argv);
    let result = run_on(
        &fakes.ports,
        &run,
        &mut Tty0,
        &mut Vec::new(),
        Terminal::new(screen.clone()).unwrap(),
        &mut input,
    );
    let snaps = snaps.take();
    Run {
        result,
        calls: commands.calls.borrow().clone(),
        snaps,
        screen,
    }
}

/// The popup's inside, 64 x 24.
fn popup(commands: &Commands, acts: Vec<Act>) -> Run {
    drive(commands, (64, 24), acts)
}

// ---- the screens ---------------------------------------------------------------------------------------------------

/// The title row, as the legend draws a section title in 64 columns.
fn title_row() -> String {
    let area = Rect::new(0, 0, 64, 3);
    let mut buf = Buffer::empty(area);
    SectionTitle::new("Agent Tiers", None).render(area, &mut buf);
    buffer_rows(&buf)[1].clone()
}

fn screen(rows: &[&str]) -> Vec<String> {
    let mut all: Vec<String> = rows.iter().map(|r| (*r).to_owned()).collect();
    all[1] = title_row();
    all
}

fn first_screen() -> Vec<String> {
    screen(&[
        "",
        "",
        "",
        " Models  one provider per machine",
        "   ▸ Preset    bifrost ▾  [ Load ]",
        "     Provider  bifrost ▾",
        "     O  opus   vertex/gemini-4-argon ▾",
        "     S  sonnet vertex/claude-sonnet-5-5 ▾",
        "     H  haiku  vertex/claude-haiku-5-5 ▾",
        "     G  gemini vertex/gemini-3.8-flash ▾",
        "",
        " Tiers  the default of each role; its range follows",
        "     auxiliar  GX ▾  default only",
        "     worker    H3 ▾  H3..S3",
        "     lead      S4 ▾  S4..O2",
        "     manager   O3 ▾  O3..O4",
        "",
        "                        [ Save ]  [ Cancel ]",
        "",
        "",
        "",
        "",
        "",
        "",
    ])
}

fn cell_of(buf: &Buffer, text: &str) -> (u16, u16) {
    let rows = buffer_rows(buf);
    let y = rows
        .iter()
        .position(|r| r.contains(text))
        .unwrap_or_else(|| panic!("no row shows {text:?}:\n{}", rows.join("\n")));
    let byte = rows[y].find(text).unwrap();
    (rows[y][..byte].chars().count() as u16, y as u16)
}

fn red() -> Option<ratatui::style::Color> {
    palette::error().fg
}

#[test]
fn the_initial_screen_is_exactly_the_documented_rows() {
    let run = popup(&Commands::new(), vec![Act::Snap, Act::Key(KeyCode::Esc)]);
    assert!(run.result.is_ok(), "{:?}", run.result.err());
    assert_eq!(run.snaps[0].rows(), first_screen());
}

#[test]
fn an_open_list_is_drawn_inline_under_its_row_with_the_filter_line() {
    let mut acts: Vec<Act> = (0..8).map(|_| Act::Key(KeyCode::Down)).collect();
    acts.extend([Act::Key(KeyCode::Enter), Act::Snap, Act::Key(KeyCode::Esc)]);
    acts.extend([Act::Snap, Act::Key(KeyCode::Esc)]);
    let run = popup(&Commands::new(), acts);
    assert!(run.result.is_ok(), "{:?}", run.result.err());
    assert_eq!(
        run.snaps[0].rows(),
        screen(&[
            "",
            "",
            "",
            "     O  opus   vertex/gemini-4-argon ▾",
            "     S  sonnet vertex/claude-sonnet-5-5 ▾",
            "     H  haiku  vertex/claude-haiku-5-5 ▾",
            "     G  gemini vertex/gemini-3.8-flash ▾",
            "",
            " Tiers  the default of each role; its range follows",
            "     auxiliar  GX ▾  default only",
            "     worker    H3 ▾  H3..S3",
            "   ▸ lead      S4 ▴  S4..O2",
            "               filter:",
            "               ( ) H4",
            "               ( ) S0",
            "               ( ) S1",
            "               ( ) S2",
            "               ( ) S3",
            "               (•) S4",
            "               ( ) O0",
            "               ( ) O2",
            "",
            "                        [ Save ]  [ Cancel ]",
            "",
        ])
    );
    // Esc closed the list only: the view is still open and the first screen is back, the focus on the lead row.
    assert_eq!(run.snaps[1].rows()[14], "   ▸ lead      S4 ▾  S4..O2");
    assert!(run.snaps[1].rows()[12].starts_with("     auxiliar"));
}

#[test]
fn typing_narrows_the_list_by_substring() {
    // not in the listed names on purpose: pins the filter line `filter: <text>` the docs give
    let mut acts: Vec<Act> = (0..8).map(|_| Act::Key(KeyCode::Down)).collect();
    acts.extend([
        Act::Key(KeyCode::Enter),
        Act::Type("O"),
        Act::Snap,
        Act::Key(KeyCode::Esc),
        Act::Key(KeyCode::Esc),
    ]);
    let run = popup(&Commands::new(), acts);
    let rows = run.snaps[0].rows();
    assert!(
        rows.contains(&"               filter: O".to_owned()),
        "{rows:#?}"
    );
    assert!(
        rows.contains(&"               ( ) O2".to_owned()),
        "{rows:#?}"
    );
    assert!(!rows.iter().any(|r| r.contains("( ) S")), "{rows:#?}");
}

#[test]
fn a_changed_value_ends_with_a_star_and_an_invalid_one_is_red_and_ends_with_a_bang() {
    let acts = vec![
        Act::Click("manager", "O3 ▾"),
        Act::Click("( ) O4", "( ) O4"),
        Act::Click("Provider  bifrost", "bifrost ▾"),
        Act::Click("( ) github-copilot", "( ) github-copilot"),
        Act::Snap,
        Act::Key(KeyCode::Esc),
    ];
    let run = popup(&Commands::new(), acts);
    let rows = run.snaps[0].rows();
    let has = |row: &str| rows.iter().any(|r| r == row);
    assert!(has("     manager   O4* ▾  O4..O4"), "{rows:#?}");
    for row in [
        "     O  opus   (unset)! ▾",
        "     S  sonnet (unset)! ▾",
        "     H  haiku  (unset)! ▾",
        "     G  gemini (unset)! ▾",
    ] {
        assert!(has(row), "{row:?} in {rows:#?}");
    }
    let (x, y) = cell_of(&run.snaps[0].buffer, "(unset)! ▾");
    assert_eq!(run.snaps[0].buffer[(x, y)].fg, red().unwrap());
    let (x, y) = cell_of(&run.snaps[0].buffer, "O4*");
    assert_ne!(
        run.snaps[0].buffer[(x, y)].fg,
        red().unwrap(),
        "a changed value is not an invalid one"
    );
    // the footer names why Save is blocked
    assert_eq!(rows[23], "Model O is not set.");
}

#[test]
fn the_footer_shows_a_refusal_in_red() {
    let commands = Commands::new().failing(
        &["preset", "set"],
        1,
        refusal(
            ErrorCode::PresetModelUnoffered,
            "bifrost does not offer O's model",
        ),
    );
    let acts = vec![
        Act::Click("O  opus", "vertex/gemini-4-argon ▾"),
        Act::Click(
            "( ) vertex/claude-sonnet-5-5",
            "( ) vertex/claude-sonnet-5-5",
        ),
        Act::Click("[ Save ]", "[ Save ]"),
        Act::Snap,
        Act::Key(KeyCode::Esc),
    ];
    let run = popup(&commands, acts);
    let snap = &run.snaps[0];
    let rows = snap.rows();
    assert_eq!(rows[6], "   ▸ O  opus   vertex/claude-sonnet-5-5* ▾");
    assert_eq!(
        rows[23],
        "preset_model_unoffered: bifrost does not offer O's model"
    );
    assert_eq!(snap.buffer[(0, 23)].fg, red().unwrap());
}

#[test]
fn the_body_scrolls_with_the_wheel_when_it_is_taller_than_the_popup() {
    let acts = vec![
        Act::Snap,
        Act::Wheel(MouseEventKind::ScrollDown),
        Act::Snap,
        Act::Wheel(MouseEventKind::ScrollUp),
        Act::Snap,
        Act::Key(KeyCode::Esc),
    ];
    let run = drive(&Commands::new(), (64, 12), acts);
    assert!(run.result.is_ok(), "{:?}", run.result.err());
    let at = |i: usize| run.snaps[i].rows();
    assert_eq!(
        at(0),
        first_screen()[..11]
            .to_vec()
            .into_iter()
            .chain([String::new()])
            .collect::<Vec<_>>()
    );
    assert_eq!(
        at(1),
        screen(&[
            "",
            "",
            "",
            "     O  opus   vertex/gemini-4-argon ▾",
            "     S  sonnet vertex/claude-sonnet-5-5 ▾",
            "     H  haiku  vertex/claude-haiku-5-5 ▾",
            "     G  gemini vertex/gemini-3.8-flash ▾",
            "",
            " Tiers  the default of each role; its range follows",
            "     auxiliar  GX ▾  default only",
            "     worker    H3 ▾  H3..S3",
            "",
        ])
    );
    assert_eq!(at(2), at(0), "the wheel up is back at the top");
}

#[test]
fn a_click_on_a_value_opens_its_list_and_a_click_on_an_entry_picks_it() {
    let acts = vec![
        Act::Click("O  opus", "vertex/gemini-4-argon ▾"),
        Act::Snap,
        Act::Click(
            "( ) vertex/claude-sonnet-5-5",
            "( ) vertex/claude-sonnet-5-5",
        ),
        Act::Snap,
        Act::Key(KeyCode::Esc),
    ];
    let commands = Commands::new();
    let run = popup(&commands, acts);
    let open = run.snaps[0].rows();
    assert!(open.iter().any(|r| r.trim() == "filter:"), "{open:#?}");
    assert!(
        open.iter().any(|r| r.trim() == "(•) vertex/gemini-4-argon"),
        "{open:#?}"
    );
    assert!(
        open.iter()
            .any(|r| r.contains("O  opus   vertex/gemini-4-argon ▴"))
    );
    let picked = run.snaps[1].rows();
    assert!(
        picked
            .iter()
            .any(|r| r == "   ▸ O  opus   vertex/claude-sonnet-5-5* ▾"),
        "{picked:#?}"
    );
    assert!(
        !picked.iter().any(|r| r.contains("filter:")),
        "the list closed"
    );
    let mut want = reads();
    want.push(candidate_call());
    assert_eq!(
        run.calls, want,
        "a pick re-reads the rungs with the four candidates"
    );
}

fn change_manager_and_o_then(finish: Act) -> Vec<Act> {
    vec![
        Act::Click("manager", "O3 ▾"),
        Act::Click("( ) O4", "( ) O4"),
        Act::Click("O  opus", "vertex/gemini-4-argon ▾"),
        Act::Click(
            "( ) vertex/claude-sonnet-5-5",
            "( ) vertex/claude-sonnet-5-5",
        ),
        finish,
    ]
}

fn candidate_call() -> Vec<String> {
    candidates(BIFROST[1])
}

fn preset_set() -> Vec<String> {
    call(&[
        "preset", "set", "--O", BIFROST[1], "--S", BIFROST[1], "--H", BIFROST[2], "--G", BIFROST[3],
    ])
}

fn tiers_set() -> Vec<String> {
    call(&[
        "config",
        "tiers",
        "set",
        "--auxiliar",
        "GX",
        "--worker",
        "H3",
        "--lead",
        "S4",
        "--manager",
        "O4",
    ])
}

fn retier() -> Vec<String> {
    call(&["session", "retier", "--all"])
}

#[test]
fn a_click_on_save_runs_the_steps_through_the_commands_in_order_and_closes() {
    let commands = Commands::new();
    // no key after the click: the view closes by itself
    let run = popup(
        &commands,
        change_manager_and_o_then(Act::Click("[ Save ]", "[ Save ]")),
    );
    assert!(run.result.is_ok(), "{:?}", run.result.err());
    let mut want = reads();
    want.extend([candidate_call(), preset_set(), tiers_set(), retier()]);
    assert_eq!(run.calls, want);
}

#[test]
fn a_refused_step_keeps_the_view_open_and_a_second_save_retries_only_that_step() {
    let commands = Commands::new().failing(
        &["config", "tiers", "set"],
        1,
        refusal(ErrorCode::TierInvalid, "worker is not below lead"),
    );
    let mut acts = change_manager_and_o_then(Act::Click("[ Save ]", "[ Save ]"));
    acts.extend([Act::Snap, Act::Click("[ Save ]", "[ Save ]")]);
    let run = popup(&commands, acts);
    assert!(run.result.is_ok(), "{:?}", run.result.err());
    assert_eq!(
        run.snaps[0].rows()[23],
        "tier_invalid: worker is not below lead",
        "refused, and the view is still open"
    );
    let mut want = reads();
    want.extend([
        candidate_call(),
        preset_set(),
        tiers_set(),
        tiers_set(),
        retier(),
    ]);
    assert_eq!(
        run.calls, want,
        "the preset step succeeded: it is the baseline, so only the failed step and the retier run again"
    );
}

#[test]
fn a_click_on_cancel_closes_without_running_a_write_command() {
    let commands = Commands::new();
    let acts = vec![
        Act::Click("manager", "O3 ▾"),
        Act::Click("( ) O4", "( ) O4"),
        Act::Click("[ Cancel ]", "[ Cancel ]"),
    ];
    let run = popup(&commands, acts);
    assert!(run.result.is_ok(), "{:?}", run.result.err());
    assert_eq!(run.calls, reads(), "only the reads ran");
}

#[test]
fn the_view_reads_config_tiers_and_models_before_it_draws() {
    let run = popup(&Commands::new(), vec![Act::Snap, Act::Key(KeyCode::Esc)]);
    assert_eq!(
        run.snaps[0].calls, 3,
        "the three reads ran before the first thing the User could act on"
    );
    assert_eq!(run.calls, reads(), "in this order, and nothing else");
    assert!(
        run.snaps[0]
            .rows()
            .iter()
            .any(|r| r.contains("Agent Tiers")),
        "and the first frame is drawn from them"
    );
}

#[test]
fn a_failed_read_is_the_commands_error_and_nothing_is_drawn() {
    let commands = Commands::new().failing(
        &["config"],
        1,
        refusal(ErrorCode::ConfigMissing, "no config"),
    );
    let run = popup(&commands, Vec::new());
    let error = run.result.unwrap_err();
    assert_eq!(
        (error.code(), error.to_string()),
        (
            ErrorCode::ConfigMissing,
            "config_missing: no config".to_owned()
        )
    );
    assert_eq!(
        run.calls,
        [call(&["config"])],
        "the first failed read stops"
    );
    assert!(
        buffer_rows(&run.screen.buffer())
            .iter()
            .all(String::is_empty),
        "nothing is drawn"
    );
}
