//! The dialog kit's `input` drawn as the input box (commands.md §13.2).

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::input_popup_size;
use super::validate::Validator;
use super::view::{Dialog, Field, Mode, REFUSAL_ROWS};
use crate::palette::{self, numbered};

const L: &str = "\u{E0B6}";
const R: &str = "\u{E0B4}";

fn input(prompt: &str, initial: &str, validator: Validator) -> Dialog {
    let mode = Mode::Input { validator, field: Field::new(initial), refused: None };
    Dialog::new("Rename".into(), prompt.into(), mode)
}

fn render(d: &mut Dialog, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| d.draw(f)).unwrap();
    terminal.backend().buffer().clone()
}

fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height).map(|y| row(buf, y).trim_end().to_owned()).collect()
}

fn press(d: &mut Dialog, code: KeyCode) {
    assert_eq!(d.press_key(KeyEvent::new(code, KeyModifiers::NONE)), None);
}

fn type_str(d: &mut Dialog, s: &str) {
    for c in s.chars() {
        press(d, KeyCode::Char(c));
    }
}

fn top(label: &str, w: usize) -> String {
    if label.is_empty() {
        format!(" ╭{}╮", "─".repeat(w - 4))
    } else {
        format!(" ╭─{L}{label}{R}{}╮", "─".repeat(w - label.chars().count() - 7))
    }
}

fn body(text: &str, w: usize) -> String {
    format!(" │ {text:<tw$} │", tw = w - 6)
}

fn bottom(w: usize) -> String {
    format!(" ╰{}╯", "─".repeat(w - 4))
}

#[test]
fn input_dialog_draws_the_prompt_as_the_label_of_the_box_above_the_buttons() {
    let mut d = input("Fork cpd-ui into a new Plan, name:", "abc", Validator::PlanId);
    let buf = render(&mut d, 44, 10);
    let rows = rows(&buf);
    assert!(rows[1].contains("Rename"), "{rows:?}");
    assert_eq!(rows[3], top("Fork cpd-ui into a new Plan, name", 44));
    assert_eq!(rows[4], body("abc ", 44));
    assert_eq!(rows[5], bottom(44));
    assert_eq!(rows[6..9], ["", "", ""]);
    assert_eq!(rows[9], "  [OK]   [Cancel]");
    assert!(palette::check::is_selected(&buf[(6, 4)]));
    assert!(!palette::check::is_selected(&buf[(5, 4)]));
}

#[test]
fn input_dialog_label_drops_the_prompts_trailing_colon_and_spaces() {
    for (prompt, label) in [("Title: ", "Title"), ("Name", "Name"), ("Name :", "Name"), ("", "")] {
        let mut d = input(prompt, "", Validator::Name);
        let rows = rows(&render(&mut d, 44, 10));
        assert_eq!(rows[3], top(label, 44), "prompt {prompt:?}");
    }
}

#[test]
fn input_dialog_box_is_always_focused_blue_12_with_text_15() {
    let mut d = input("Title:", "abc", Validator::Title);
    let buf = render(&mut d, 44, 10);
    for x in [1, 2] {
        assert_eq!(buf[(x, 3)].fg, numbered::C12, "top {x}");
    }
    for x in 10..43 {
        assert_eq!(buf[(x, 3)].fg, numbered::C12, "fill {x}");
    }
    assert_eq!(buf[(42, 3)].fg, numbered::C12);
    for x in 1..43 {
        assert_eq!(buf[(x, 5)].fg, numbered::C12, "bottom {x}");
    }
    assert_eq!((buf[(1, 4)].fg, buf[(42, 4)].fg), (numbered::C12, numbered::C12));
    assert_eq!((buf[(3, 3)].symbol(), buf[(3, 3)].fg), (L, numbered::C7));
    for x in 4..9 {
        assert_eq!((buf[(x, 3)].bg, buf[(x, 3)].fg), (numbered::C7, numbered::C0), "pill {x}");
    }
    assert_eq!((buf[(9, 3)].symbol(), buf[(9, 3)].fg), (R, numbered::C7));
    for x in [3, 4, 5] {
        assert_eq!(buf[(x, 4)].fg, numbered::C15, "text {x}");
    }
}

#[test]
fn input_dialog_box_grows_with_a_long_value_and_the_refusal_follows_it() {
    let mut d = input("Name:", "", Validator::Name);
    type_str(&mut d, &"a".repeat(81));
    let rows1 = rows(&render(&mut d, 44, 13));
    assert_eq!(rows1[3], top("Name", 44));
    assert_eq!(rows1[4], body(&"a".repeat(38), 44));
    assert_eq!(rows1[5], body(&"a".repeat(38), 44));
    assert_eq!(rows1[6], body("aaaaa ", 44));
    assert_eq!(rows1[7], bottom(44));
    press(&mut d, KeyCode::Enter);
    let rows2 = rows(&render(&mut d, 44, 13));
    assert!(rows2[8].contains("1 to 80"), "{rows2:?}");
    assert_eq!(rows2[7], bottom(44));
    assert!(REFUSAL_ROWS >= 1);
    press(&mut d, KeyCode::Backspace);
    type_str(&mut d, "a");
    let rows3 = rows(&render(&mut d, 44, 13));
    assert!(!rows3.iter().any(|l| l.contains("1 to 80")), "{rows3:?}");
    // 81 characters: 81 / 38 + 1 = 3 text rows; the last holds 5 characters and the cursor
    assert_eq!(rows3[7], bottom(44));
    press(&mut d, KeyCode::Backspace);
    let rows4 = rows(&render(&mut d, 44, 13));
    // 80 characters: 80 / 38 + 1 = 3 text rows still
    assert_eq!(rows4[7], bottom(44));
    assert_eq!(rows4[8], "");
}

#[test]
fn input_dialog_box_shows_the_rows_around_the_cursor_in_a_short_area() {
    let text: String = (0..80).map(|i| char::from(b'a' + (i / 38) as u8)).collect();
    let mut d = input("Name:", &text, Validator::Name);
    // 44 x 11: avail = 2; 80 characters at tw 38 are rows of 38 `a`, 38 `b` and 4 `c`
    let rows1 = rows(&render(&mut d, 44, 11));
    assert_eq!(rows1[3], top("Name", 44));
    assert_eq!(rows1[4], body(&"b".repeat(38), 44));
    assert_eq!(rows1[5], body(&format!("{} ", "c".repeat(4)), 44));
    assert_eq!(rows1[6], bottom(44));
    press(&mut d, KeyCode::Home);
    let rows2 = rows(&render(&mut d, 44, 11));
    assert_eq!(rows2[3], top("Name", 44));
    assert_eq!(rows2[4], body(&"a".repeat(38), 44));
    assert_eq!(rows2[5], body(&"b".repeat(38), 44));
    assert_eq!(rows2[6], bottom(44));
}

#[test]
fn input_popup_size_reserves_the_validators_longest_value() {
    let client = (200, 60);
    assert_eq!(input_popup_size(client, "Rename", "Title:", "", Validator::Title), (42, 17));
    assert_eq!(input_popup_size(client, "Rename", "Title:", "", Validator::Name), (42, 14));
    assert_eq!(input_popup_size(client, "Rename", "Title:", "", Validator::PlanId), (42, 12));
}

#[test]
fn input_popup_size_fits_the_label_the_initial_text_and_the_client() {
    let prompt = "Fork Plan sketch into a new Plan, name:";
    assert_eq!(input_popup_size((200, 60), "Fork", prompt, "", Validator::PlanId), (47, 12));
    let initial = "x".repeat(100);
    assert_eq!(input_popup_size((200, 60), "Rename", "Name:", &initial, Validator::Name), (78, 13));
    assert_eq!(input_popup_size((30, 8), "Rename", "Title:", "", Validator::Title), (30, 8));
}
