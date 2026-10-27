//! The dialog kit's `form` (commands.md §13.2, pro-d114): a labelled text box and a labelled radio list in the kit's
//! frame, with `[OK]` and `[Cancel]`; its command line, keys and result file. The drawing is pinned by what each row
//! holds and in which order, not by how many blank rows lie between (the docs give no sample).

use clap::Parser;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::view::Dialog;
use super::{Args, Outcome, write_result};
use crate::palette::numbered;
use cpd_core::error::ErrorCode;

const L: &str = "\u{E0B6}";
const R: &str = "\u{E0B4}";

fn opts(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| (*s).to_owned()).collect()
}

fn dialog(names: &[&str]) -> Dialog {
    Dialog::form("New Context".into(), "Title: ".into(), "Owner:".into(), opts(names))
}

fn render(d: &mut Dialog, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| d.draw(f)).unwrap();
    terminal.backend().buffer().clone()
}

fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()).collect()
}

fn key(d: &mut Dialog, code: KeyCode) -> Option<Outcome> {
    d.press_key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn type_str(d: &mut Dialog, s: &str) {
    for c in s.chars() {
        assert_eq!(key(d, KeyCode::Char(c)), None, "{c:?}");
    }
}

fn row_of(rows: &[String], needle: &str) -> usize {
    rows.iter().position(|r| r.contains(needle)).unwrap_or_else(|| panic!("no row has {needle:?}: {rows:#?}"))
}

/// The marker (`x` or blank) of the option `name`.
fn marker(rows: &[String], name: &str) -> char {
    let row = &rows[row_of(rows, &format!(") {name}"))];
    let at = row.find(&format!(") {name}")).unwrap();
    row[..at].chars().last().unwrap()
}

#[test]
fn form_dialog_args_refuse_bad_options() {
    #[derive(Parser, Debug)]
    struct Cli {
        #[command(flatten)]
        args: Args,
    }
    let parts = |options: &[&str]| {
        let mut argv = vec!["dialog", "form", "--title=New Context", "--out=/o", "--input-label=Title: ", "--select-label=Owner:"];
        let flags: Vec<String> = options.iter().map(|o| format!("--option={o}")).collect();
        argv.extend(flags.iter().map(String::as_str));
        Cli::try_parse_from(argv).map(|cli| cli.args.kind.parts().map(|_| ()))
    };
    // it parses
    assert!(matches!(parts(&["a", "b"]), Ok(Ok(()))), "{:?}", parts(&["a", "b"]));
    let longest = "x".repeat(70);
    assert!(matches!(parts(&[longest.as_str()]), Ok(Ok(()))));
    // no option at all is no command line
    assert!(parts(&[]).is_err(), "at least one --option");
    // the others are `args_invalid`, as `select`'s
    let too_long = "x".repeat(71);
    for bad in [&[""][..], &["a", "a"], &["a\nb"], &["a\tb"], &[too_long.as_str()]] {
        let refused = parts(bad).unwrap_or_else(|e| panic!("{bad:?} parses: {e}"));
        assert_eq!(refused.unwrap_err().code(), ErrorCode::ArgsInvalid, "{bad:?}");
    }
}

#[test]
fn form_dialog_draws_the_title_box_owner_rows_and_buttons() {
    let mut d = dialog(&["clayton-duarte", "other"]);
    type_str(&mut d, "Hi");
    let buf = render(&mut d, 42, 14);
    let rows = rows(&buf);
    let last = rows.len() - 1;
    let wall = |s: &str| format!("│{s:<40}│");
    // the frame
    assert!(rows[0].starts_with("╭─") && rows[0].contains(&format!("{L}New Context{R}")) && rows[0].ends_with('╮'), "{rows:#?}");
    assert_eq!(rows[1], wall(""));
    assert_eq!(rows[last], format!("╰{}╯", "─".repeat(40)));
    // the input box: label `Title` (the prompt's trailing space and colon dropped) on its top border, the typed text, then its bottom
    let top = row_of(&rows, "Title");
    assert_eq!(top, 2, "the box starts on popup row 2: {rows:#?}");
    assert_eq!(rows[top], format!("│ ╭─{L}Title{R}{}╮ │", "─".repeat(40 - 5 - 7)));
    assert_eq!(rows[top + 1], format!("│ │ {:<34} │ │", "Hi"));
    assert_eq!(rows[top + 2], format!("│ ╰{}╯ │", "─".repeat(36)));
    // the owner list: its label, then one radio row per option, the first chosen
    let owner = row_of(&rows, "Owner:");
    assert!(owner > top + 2, "{rows:#?}");
    assert_eq!(rows[owner], wall(" Owner:"));
    assert_eq!(rows[owner + 1], wall(" (x) clayton-duarte"));
    assert_eq!(rows[owner + 2], wall(" ( ) other"));
    // the buttons row is the one above the bottom border
    assert_eq!(rows[last - 1], wall(&format!(" {L}OK{R}{}{L}Cancel{R} ", " ".repeat(40 - 1 - 4 - 8 - 1))));
    assert_eq!(rows[last - 2], wall(""), "a blank row above the buttons");
}

#[test]
fn form_dialog_tab_moves_focus_and_arrows_pick_the_owner() {
    let mut d = dialog(&["a", "b", "c"]);
    let border = |d: &mut Dialog| {
        let buf = render(d, 42, 14);
        let top = row_of(&rows(&buf), "Title");
        buf[(20, top as u16)].fg
    };
    let shown = |d: &mut Dialog| rows(&render(d, 42, 14));
    // the box has the focus at the start: it is blue 12, and the keys are typed, `j` included
    assert_eq!(border(&mut d), numbered::C12);
    type_str(&mut d, "Hij");
    assert!(shown(&mut d)[row_of(&shown(&mut d), "Title") + 1].contains("Hij"));
    assert_eq!(marker(&shown(&mut d), "a"), 'x');
    // Tab moves the focus to the list: the box turns white 15, and the keys move the choice instead of typing
    assert_eq!(key(&mut d, KeyCode::Tab), None);
    assert_eq!(border(&mut d), numbered::C15);
    let steps = [(KeyCode::Down, "b"), (KeyCode::Down, "c"), (KeyCode::Down, "c"), (KeyCode::Up, "b"), (KeyCode::Up, "a"), (KeyCode::Up, "a")];
    for (code, chosen) in steps {
        assert_eq!(key(&mut d, code), None);
        let rows = shown(&mut d);
        let marks: Vec<char> = ["a", "b", "c"].iter().map(|n| marker(&rows, n)).collect();
        let want: Vec<char> = ["a", "b", "c"].iter().map(|n| if *n == chosen { 'x' } else { ' ' }).collect();
        assert_eq!(marks, want, "{code:?}");
    }
    assert_eq!(key(&mut d, KeyCode::Char('z')), None);
    let rows = shown(&mut d);
    assert!(rows[row_of(&rows, "Title") + 1].contains("Hij") && !rows.iter().any(|r| r.contains("Hijz")), "a letter in the list is not typed: {rows:#?}");
    // the second Tab gives the focus back to the box
    assert_eq!(key(&mut d, KeyCode::Tab), None);
    assert_eq!(border(&mut d), numbered::C12);
    assert_eq!(key(&mut d, KeyCode::Char('!')), None);
    let rows = shown(&mut d);
    assert!(rows[row_of(&rows, "Title") + 1].contains("Hij!"), "{rows:#?}");
    // the choice made in the list is the one accepted
    key(&mut d, KeyCode::Tab);
    key(&mut d, KeyCode::Down);
    assert_eq!(key(&mut d, KeyCode::Enter), Some(Outcome::form("Hij!".into(), "b".into())));
}

#[test]
fn form_dialog_enter_with_an_empty_title_is_refused_and_stays_open() {
    let mut d = dialog(&["a", "b"]);
    assert_eq!(key(&mut d, KeyCode::Enter), None, "an empty title is refused");
    let rows = rows(&render(&mut d, 42, 14));
    assert!(rows.iter().any(|r| r.contains("1 to 200")), "the refusal shows its reason: {rows:#?}");
    assert!(rows[0].contains("New Context"), "the dialog is still drawn: {rows:#?}");
    // a title of blanks is no title; so is one of 201 characters
    type_str(&mut d, "   ");
    assert_eq!(key(&mut d, KeyCode::Enter), None);
    let mut long = dialog(&["a"]);
    type_str(&mut long, &"x".repeat(201));
    assert_eq!(key(&mut long, KeyCode::Enter), None);
    // typing a title takes the refusal away and Enter accepts it, trimmed
    for _ in 0..3 {
        key(&mut d, KeyCode::Backspace);
    }
    type_str(&mut d, " Building cpd! ");
    assert_eq!(key(&mut d, KeyCode::Enter), Some(Outcome::form("Building cpd!".into(), "a".into())));
}

#[test]
fn form_dialog_result_file_carries_value_and_choice() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.json");
    let read = || serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&path).unwrap()).unwrap();

    let mut d = dialog(&["a", "b"]);
    type_str(&mut d, "Building cpd!");
    key(&mut d, KeyCode::Tab);
    key(&mut d, KeyCode::Down);
    let accepted = key(&mut d, KeyCode::Enter).expect("a title and an owner are accepted");
    write_result(&path, &d, &accepted).unwrap();
    assert_eq!(read(), serde_json::json!({"confirmed": true, "value": "Building cpd!", "choice": "b"}));

    let mut d = dialog(&["a", "b"]);
    type_str(&mut d, "Building cpd!");
    let cancelled = key(&mut d, KeyCode::Esc).expect("Esc cancels");
    write_result(&path, &d, &cancelled).unwrap();
    assert_eq!(read(), serde_json::json!({"confirmed": false}));
}
