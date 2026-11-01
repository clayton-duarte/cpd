//! The dialog kit's `select`: a radio list in the kit's frame (commands.md §13.2), its keys, clicks and scrolling, the
//! check of its options and its opener.

use std::sync::{Arc, Mutex};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use super::view::{Dialog, Mode};
use super::{Outcome, check_options, select};
use cpd_core::clock::ManualClock;
use cpd_core::error::ErrorCode;
use cpd_core::fakes::tmux::{FakeTmux, TmuxRoute};
use cpd_core::host::Tmux;
use cpd_core::host::runner::FakeRunner;

const L: &str = "\u{E0B6}";
const R: &str = "\u{E0B4}";

fn opts(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| (*s).to_owned()).collect()
}

fn dialog(names: &[&str]) -> Dialog {
    Dialog::new(
        "New Context".into(),
        "Owner:".into(),
        Mode::Select {
            options: opts(names),
            cursor: 0,
        },
    )
}

fn render(d: &mut Dialog, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| d.draw(f)).unwrap();
    let buf: &Buffer = terminal.backend().buffer();
    (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect())
        .collect()
}

fn key(d: &mut Dialog, code: KeyCode) -> Option<Outcome> {
    d.press_key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn click(d: &mut Dialog, column: u16, row: u16) -> Option<Outcome> {
    d.press_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

/// The marker of each of `names` in the drawn rows: `x` for the choice, a blank for the others.
fn markers(rows: &[String], names: &[&str]) -> Vec<char> {
    names
        .iter()
        .map(|n| {
            let row = rows
                .iter()
                .find(|r| r.contains(&format!(") {n}")))
                .unwrap_or_else(|| panic!("{n} is not drawn: {rows:#?}"));
            let at = row.find(&format!(") {n}")).unwrap();
            row[..at].chars().last().unwrap()
        })
        .collect()
}

#[test]
fn select_draws_the_prompt_and_a_radio_row_per_option() {
    let mut d = dialog(&["clayton-duarte", "other"]);
    let rows = render(&mut d, 42, 8);
    let wall = |s: &str| format!("│{s:<40}│");
    assert!(
        rows[0].starts_with("╭─")
            && rows[0].contains(&format!("{L}New Context{R}"))
            && rows[0].ends_with('╮'),
        "{rows:#?}"
    );
    assert_eq!(rows[1], wall(""));
    assert_eq!(rows[2], wall(" Owner:"));
    assert_eq!(rows[3], wall(" (x) clayton-duarte"));
    assert_eq!(rows[4], wall(" ( ) other"));
    assert_eq!(rows[5], wall(""));
    assert_eq!(
        rows[6],
        wall(&format!(
            " {L}OK{R}{}{L}Cancel{R}",
            " ".repeat(40 - 1 - 4 - 8 - 1)
        ))
    );
    assert_eq!(rows[7], format!("╰{}╯", "─".repeat(40)));
}

#[test]
fn the_cursor_row_is_the_choice_and_arrows_and_j_k_move_it_within_the_options() {
    let names = ["a", "b", "c"];
    let mut d = dialog(&names);
    assert_eq!(markers(&render(&mut d, 42, 9), &names), ['x', ' ', ' ']);
    for (code, want) in [
        (KeyCode::Up, ['x', ' ', ' ']),
        (KeyCode::Down, [' ', 'x', ' ']),
        (KeyCode::Char('j'), [' ', ' ', 'x']),
        (KeyCode::Down, [' ', ' ', 'x']),
        (KeyCode::Char('k'), [' ', 'x', ' ']),
        (KeyCode::Up, ['x', ' ', ' ']),
        (KeyCode::Char('k'), ['x', ' ', ' ']),
    ] {
        assert_eq!(key(&mut d, code), None);
        assert_eq!(
            markers(&render(&mut d, 42, 9), &names),
            want,
            "after {code:?}"
        );
    }
}

#[test]
fn enter_y_or_a_click_on_ok_confirms_with_the_option_under_the_cursor() {
    let names = ["a", "b", "c"];
    for first in [None, Some(KeyCode::Down), Some(KeyCode::Char('j'))] {
        let want = if first.is_some() { "b" } else { "a" };
        for confirm in [Some(KeyCode::Enter), Some(KeyCode::Char('y')), None] {
            let mut d = dialog(&names);
            render(&mut d, 42, 9);
            if let Some(code) = first {
                assert_eq!(key(&mut d, code), None);
            }
            let outcome = match confirm {
                Some(code) => key(&mut d, code),
                None => click(&mut d, 4, 7),
            };
            assert_eq!(
                outcome,
                Some(Outcome::value(want.into())),
                "{first:?} {confirm:?}"
            );
        }
    }
}

#[test]
fn esc_n_q_or_a_click_on_cancel_cancel_with_no_value() {
    let names = ["a", "b"];
    for cancel in [
        Some(KeyCode::Esc),
        Some(KeyCode::Char('n')),
        Some(KeyCode::Char('q')),
        None,
    ] {
        let mut d = dialog(&names);
        render(&mut d, 42, 8);
        key(&mut d, KeyCode::Down);
        let outcome = match cancel {
            Some(code) => key(&mut d, code),
            None => click(&mut d, 36, 6),
        };
        assert_eq!(outcome, Some(Outcome::cancelled()), "{cancel:?}");
    }
}

#[test]
fn a_click_on_an_option_row_moves_the_cursor_and_does_not_close() {
    let names = ["a", "b", "c"];
    let mut d = dialog(&names);
    render(&mut d, 42, 9);
    assert_eq!(click(&mut d, 6, 5), None);
    assert_eq!(markers(&render(&mut d, 42, 9), &names), [' ', ' ', 'x']);
    assert_eq!(click(&mut d, 6, 3), None);
    assert_eq!(markers(&render(&mut d, 42, 9), &names), ['x', ' ', ' ']);
    assert_eq!(
        key(&mut d, KeyCode::Enter),
        Some(Outcome::value("a".into()))
    );
}

#[test]
fn a_list_taller_than_the_popup_scrolls_to_keep_the_cursor_row_visible() {
    let names = ["o0", "o1", "o2", "o3", "o4", "o5"];
    let mut d = dialog(&names);
    // 8 rows: a body of 3 rows for the prompt and 6 options
    let shown = |rows: &[String], n: &str| rows.iter().any(|r| r.contains(&format!(") {n} ")));
    let rows = render(&mut d, 42, 8);
    assert!(shown(&rows, "o0") && !shown(&rows, "o5"), "{rows:#?}");
    for _ in 0..5 {
        key(&mut d, KeyCode::Down);
        let rows = render(&mut d, 42, 8);
        assert!(
            rows.iter().any(|r| r.contains("(x) ")),
            "the cursor row is drawn: {rows:#?}"
        );
    }
    let rows = render(&mut d, 42, 8);
    assert!(rows.iter().any(|r| r.contains("(x) o5")), "{rows:#?}");
    for _ in 0..5 {
        key(&mut d, KeyCode::Up);
    }
    let rows = render(&mut d, 42, 8);
    assert!(
        rows.iter().any(|r| r.contains("(x) o0")),
        "scrolled back: {rows:#?}"
    );
}

#[test]
fn the_options_are_refused_when_none_empty_repeated_or_with_a_control_character() {
    check_options(&opts(&["a", "b"])).unwrap();
    check_options(&opts(&[&"x".repeat(70)])).unwrap();
    let refused = [
        opts(&[]),
        opts(&["a", ""]),
        opts(&["a", "b", "a"]),
        opts(&["a\tb"]),
        opts(&["a\nb"]),
        opts(&["a\u{1b}[0m"]),
        opts(&[&"x".repeat(71)]),
    ];
    for bad in refused {
        assert_eq!(
            check_options(&bad).unwrap_err().code(),
            ErrorCode::ArgsInvalid,
            "{bad:?}"
        );
    }
}

/// The result a popup writes (none: it closes with no usable result), set by the test.
type Answer = Arc<Mutex<Option<String>>>;

/// A fake tmux (the `tmux` route of a `FakeRunner`) with a server in `dir` and one client of 100 x 30: on `display-popup`
/// it writes the text in the [`Answer`] it returns, if there is one, to the `--out` file named in the popup's command.
fn mock_tmux(dir: &tempfile::TempDir) -> (Tmux, Arc<FakeRunner>, Answer) {
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
    let result = Answer::default();
    let answer = result.clone();
    FakeTmux::on_popup(route.fake(), move |_| answer.lock().unwrap().clone());
    (tmux, runner, result)
}

#[test]
fn the_opener_runs_a_select_popup_with_one_option_flag_each_and_returns_the_chosen_option() {
    let dir = tempfile::tempdir().unwrap();
    let (tmux, runner, result) = mock_tmux(&dir);
    let options = opts(&["a", "b"]);

    *result.lock().unwrap() = Some(r#"{"confirmed":true,"value":"b"}"#.to_owned());
    assert_eq!(
        select(&tmux, "New Context", "Owner:", &options).unwrap(),
        Some("b".to_owned())
    );
    let popups: Vec<_> = runner
        .calls()
        .into_iter()
        .filter(|c| c.args.iter().any(|a| a == "display-popup"))
        .collect();
    assert_eq!(popups.len(), 1, "{popups:?}");
    let command = popups[0].args.last().unwrap();
    assert!(
        command.starts_with("cpd tui view dialog select --title='New Context' --out='"),
        "{command}"
    );
    assert!(
        command.ends_with("' --prompt='Owner:' --option='a' --option='b'"),
        "{command}"
    );

    *result.lock().unwrap() = Some(r#"{"confirmed":false}"#.to_owned());
    assert_eq!(
        select(&tmux, "New Context", "Owner:", &options).unwrap(),
        None
    );

    *result.lock().unwrap() = None;
    assert!(select(&tmux, "New Context", "Owner:", &options).is_err());
}
