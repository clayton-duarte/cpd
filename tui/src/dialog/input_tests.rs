//! The dialog kit's `input` drawn as the input box inside the dialog's frame (commands.md §13.2; hs-d578): the box on
//! popup row 2, `W - 2` cells wide between the walls, and the refusal rows under it. A popup `w` wide holds a box `w - 2` wide.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::validate::Validator;
use super::view::{Dialog, Field, Mode, REFUSAL_ROWS, wrap};
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
    (0..buf.area.height).map(|y| row(buf, y)).collect()
}

fn press(d: &mut Dialog, code: KeyCode) {
    assert_eq!(d.press_key(KeyEvent::new(code, KeyModifiers::NONE)), None);
}

fn type_str(d: &mut Dialog, s: &str) {
    for c in s.chars() {
        press(d, KeyCode::Char(c));
    }
}

/// A popup row: the wall, the `inner` cells (the box's own margins are the dialog's padding), the wall.
fn wall(inner: &str) -> String {
    format!("│{inner}│")
}

/// The box's top row `n` cells wide (its margin cells included).
fn top(label: &str, n: usize) -> String {
    if label.is_empty() {
        format!(" ╭{}╮ ", "─".repeat(n - 4))
    } else {
        format!(" ╭─{L}{label}{R}{}╮ ", "─".repeat(n - label.chars().count() - 7))
    }
}

fn body(text: &str, n: usize) -> String {
    format!(" │ {text:<tw$} │ ", tw = n - 6)
}

fn bottom(n: usize) -> String {
    format!(" ╰{}╯ ", "─".repeat(n - 4))
}

fn blank(w: usize) -> String {
    wall(&" ".repeat(w - 2))
}

fn buttons(w: usize) -> String {
    let gap = w - 4 - 4 - 8;
    wall(&format!(" {L}OK{R}{}{L}Cancel{R} ", " ".repeat(gap)))
}

#[test]
fn input_dialog_draws_the_prompt_as_the_label_of_the_box_inside_the_walls_above_the_buttons() {
    let mut d = input("Fork cpd-ui into a new Plan, name:", "abc", Validator::PlanId);
    let buf = render(&mut d, 46, 10);
    let rows = rows(&buf);
    assert!(rows[0].contains("Rename"), "{rows:?}");
    assert_eq!(rows[1], blank(46));
    assert_eq!(rows[2], wall(&top("Fork cpd-ui into a new Plan, name", 44)));
    assert_eq!(rows[3], wall(&body("abc ", 44)));
    assert_eq!(rows[4], wall(&bottom(44)));
    assert_eq!(rows[5..8], [blank(46), blank(46), blank(46)]);
    assert_eq!(rows[8], buttons(46));
    assert!(palette::check::is_selected(&buf[(7, 3)]));
    assert!(!palette::check::is_selected(&buf[(6, 3)]));
}

#[test]
fn input_dialog_label_drops_the_prompts_trailing_colon_and_spaces() {
    for (prompt, label) in [("Title: ", "Title"), ("Name", "Name"), ("Name :", "Name"), ("", "")] {
        let mut d = input(prompt, "", Validator::Name);
        let rows = rows(&render(&mut d, 46, 10));
        assert_eq!(rows[2], wall(&top(label, 44)), "prompt {prompt:?}");
    }
}

#[test]
fn input_dialog_box_is_always_focused_blue_12_with_text_15() {
    let mut d = input("Title:", "abc", Validator::Title);
    let buf = render(&mut d, 46, 10);
    for x in [2, 3] {
        assert_eq!(buf[(x, 2)].fg, numbered::C12, "top {x}");
    }
    for x in 11..43 {
        assert_eq!(buf[(x, 2)].fg, numbered::C12, "fill {x}");
    }
    assert_eq!(buf[(43, 2)].fg, numbered::C12);
    for x in 2..44 {
        assert_eq!(buf[(x, 4)].fg, numbered::C12, "bottom {x}");
    }
    assert_eq!((buf[(2, 3)].fg, buf[(43, 3)].fg), (numbered::C12, numbered::C12));
    for x in [4, 5, 6] {
        assert_eq!(buf[(x, 3)].fg, numbered::C15, "text {x}");
    }
}

#[test]
fn the_input_box_sits_inside_the_walls_on_row_two_with_its_label_pill_in_the_border_colour() {
    let mut d = input("Name:", "sketch", Validator::PlanId);
    let buf = render(&mut d, 42, 10);
    let rows = rows(&buf);
    assert_eq!(rows[1], blank(42));
    assert_eq!(rows[2], wall(&top("Name", 40)));
    assert_eq!(rows[3], wall(&body("sketch ", 40)));
    assert_eq!(rows[4], wall(&bottom(40)));
    assert_eq!(rows[5..8], [blank(42), blank(42), blank(42)]);
    for (y, row) in rows.iter().enumerate().take(5) {
        assert_eq!(row.chars().count(), 42, "row {y}");
    }
    // walls 8, the box's own border and label pill 12: caps 12 on the terminal's background, text inverted in 12
    for y in 0..10 {
        assert_eq!((buf[(0, y)].fg, buf[(41, y)].fg), (numbered::C8, numbered::C8), "wall row {y}");
    }
    for x in [2, 3, 39] {
        assert_eq!(buf[(x, 2)].fg, numbered::C12, "border {x}");
    }
    let reset = ratatui::buffer::Cell::EMPTY.bg;
    assert_eq!((buf[(4, 2)].symbol(), buf[(4, 2)].fg, buf[(4, 2)].bg), (L, numbered::C12, reset));
    for x in 5..=8 {
        let cell = &buf[(x, 2)];
        assert_eq!((cell.fg, cell.bg), (numbered::C12, reset), "pill text {x}");
        assert!(cell.modifier.contains(ratatui::style::Modifier::REVERSED), "pill text {x} is reversed: {cell:?}");
    }
    assert_eq!((buf[(9, 2)].symbol(), buf[(9, 2)].fg, buf[(9, 2)].bg), (R, numbered::C12, reset));
}

#[test]
fn the_refusal_rows_follow_the_box_inside_the_padding() {
    let why = Validator::PlanId.check("Bad").unwrap_err();
    let mut d = input("Name:", "Bad", Validator::PlanId);
    press(&mut d, KeyCode::Enter);
    let buf = render(&mut d, 42, 10);
    let rows = rows(&buf);
    assert_eq!(rows[4], wall(&bottom(40)));
    let drawn = wrap(&why, 38);
    for n in 0..2 {
        let text = drawn.get(n).map_or(String::new(), |t| format!(" {t}"));
        assert_eq!(rows[5 + n], wall(&format!("{text:<40}")), "refusal row {n}");
    }
    // the text starts at column 2, in the note colour, and stays inside the padding
    assert_eq!(buf[(2, 5)].symbol(), why.chars().next().unwrap().to_string());
    for (x, c) in drawn[0].chars().enumerate().filter(|(_, c)| *c != ' ') {
        assert_eq!((buf[(2 + x as u16, 5)].symbol(), Some(buf[(2 + x as u16, 5)].fg)), (c.to_string().as_str(), palette::note().fg));
    }
    assert_eq!(rows[7], blank(42));
    assert_eq!(rows[8], buttons(42));
    const _: () = assert!(REFUSAL_ROWS >= 1);
}

#[test]
fn input_dialog_box_grows_with_a_long_value_and_the_refusal_follows_it() {
    let mut d = input("Name:", "", Validator::Name);
    type_str(&mut d, &"a".repeat(81));
    let rows1 = rows(&render(&mut d, 46, 13));
    assert_eq!(rows1[2], wall(&top("Name", 44)));
    assert_eq!(rows1[3], wall(&body(&"a".repeat(38), 44)));
    assert_eq!(rows1[4], wall(&body(&"a".repeat(38), 44)));
    assert_eq!(rows1[5], wall(&body("aaaaa ", 44)));
    assert_eq!(rows1[6], wall(&bottom(44)));
    press(&mut d, KeyCode::Enter);
    let rows2 = rows(&render(&mut d, 46, 13));
    assert!(rows2[7].contains("1 to 80"), "{rows2:?}");
    assert_eq!(rows2[6], wall(&bottom(44)));
    press(&mut d, KeyCode::Backspace);
    type_str(&mut d, "a");
    let rows3 = rows(&render(&mut d, 46, 13));
    assert!(!rows3.iter().any(|l| l.contains("1 to 80")), "{rows3:?}");
    // 81 characters: 81 / 38 + 1 = 3 text rows; the last holds 5 characters and the cursor
    assert_eq!(rows3[6], wall(&bottom(44)));
    press(&mut d, KeyCode::Backspace);
    let rows4 = rows(&render(&mut d, 46, 13));
    // 80 characters: 80 / 38 + 1 = 3 text rows still
    assert_eq!(rows4[6], wall(&bottom(44)));
    assert_eq!(rows4[7], blank(46));
}

#[test]
fn input_dialog_box_shows_the_rows_around_the_cursor_in_a_short_area() {
    let text: String = (0..80).map(|i| char::from(b'a' + (i / 38) as u8)).collect();
    let mut d = input("Name:", &text, Validator::Name);
    // 46 x 11: avail = 2; 80 characters at tw 38 are rows of 38 `a`, 38 `b` and 4 `c`
    let rows1 = rows(&render(&mut d, 46, 11));
    assert_eq!(rows1[2], wall(&top("Name", 44)));
    assert_eq!(rows1[3], wall(&body(&"b".repeat(38), 44)));
    assert_eq!(rows1[4], wall(&body(&format!("{} ", "c".repeat(4)), 44)));
    assert_eq!(rows1[5], wall(&bottom(44)));
    press(&mut d, KeyCode::Home);
    let rows2 = rows(&render(&mut d, 46, 11));
    assert_eq!(rows2[2], wall(&top("Name", 44)));
    assert_eq!(rows2[3], wall(&body(&"a".repeat(38), 44)));
    assert_eq!(rows2[4], wall(&body(&"b".repeat(38), 44)));
    assert_eq!(rows2[5], wall(&bottom(44)));
}
