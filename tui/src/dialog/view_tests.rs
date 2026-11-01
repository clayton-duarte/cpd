//! The dialog kit's view (commands.md §13.2, "The dialog kit"; hs-d578): the frame, the rows, the pill buttons and
//! their hit rects, by the rows and cells drawn. Coordinates are popup columns and rows, 0-based.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::style::Color;

use super::*;
use crate::palette::numbered::{C8, C12, C15};

const L: &str = "\u{E0B6}";
const R: &str = "\u{E0B4}";

fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn click(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn wheel(kind: MouseEventKind) -> MouseEvent {
    MouseEvent {
        kind,
        column: 2,
        row: 2,
        modifiers: KeyModifiers::NONE,
    }
}

fn confirm() -> Dialog {
    Dialog::new(
        "Archive".into(),
        "Archive cpd-ui?".into(),
        Mode::Confirm {
            yes: "Archive".into(),
        },
    )
}

fn input(initial: &str) -> Dialog {
    let mode = Mode::Input {
        validator: Validator::PlanId,
        field: Field::new(initial),
        refused: None,
    };
    Dialog::new("New Plan".into(), "Name:".into(), mode)
}

fn text(lines: usize) -> Dialog {
    let body = (1..=lines)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    Dialog::new("Legend".into(), body, Mode::Text)
}

fn no_cockpit() -> Dialog {
    Dialog::new(
        "session open l-ship".into(),
        "no_cockpit: No cockpit.".into(),
        Mode::Text,
    )
}

fn checklist() -> Dialog {
    let row = |id: &str, text: &str, detail: &str| Row {
        id: id.into(),
        text: text.into(),
        detail: detail.into(),
    };
    let rows = vec![
        row("r-1", "use sqlite as the store", "duplicate r-0"),
        row("r-2", "wire format", "documented docs/a.md"),
    ];
    Dialog::new(
        "Retire records of cpd".into(),
        String::new(),
        Mode::Checklist {
            rows,
            ticked: vec![false, false],
            cursor: 0,
        },
    )
}

fn render(d: &mut Dialog, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| d.draw(f)).unwrap();
    terminal.backend().buffer().clone()
}

/// The rows as drawn, every cell, nothing trimmed.
fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

fn type_str(d: &mut Dialog, s: &str) {
    for c in s.chars() {
        assert_eq!(d.press_key(press(KeyCode::Char(c))), None);
    }
}

// ---- the drawings of commands.md §13.2, 42 columns wide ----

fn pill(label: &str) -> String {
    format!("{L}{label}{R}")
}

/// A row of a popup `w` wide: wall, padding, `inner` cut or padded to `w - 4`, padding, wall.
fn line(inner: &str, w: usize) -> String {
    format!("│ {inner:<tw$} │", tw = w - 4)
}

fn top(title: &str, w: usize) -> String {
    format!(
        "╭─{}{}╮",
        pill(title),
        "─".repeat(w - 5 - title.chars().count())
    )
}

fn bottom(w: usize) -> String {
    format!("╰{}╯", "─".repeat(w - 2))
}

fn buttons(accept: &str, other: Option<&str>, w: usize) -> String {
    let accept = pill(accept);
    match other {
        Some(other) => {
            let other = pill(other);
            let gap = w - 4 - accept.chars().count() - other.chars().count();
            line(&format!("{accept}{}{other}", " ".repeat(gap)), w)
        }
        None => line(&accept, w),
    }
}

/// One kind at the size of its drawing: how to make it, its size, its title, its buttons with their columns, and the
/// drawing's rows.
struct Kind {
    make: fn() -> Dialog,
    size: (u16, u16),
    title: &'static str,
    accept: (&'static str, u16, u16),
    other: Option<(&'static str, u16, u16)>,
    accepted: Outcome,
    drawing: Vec<String>,
}

impl Kind {
    fn dialog(&self) -> Dialog {
        (self.make)()
    }

    fn buttons_row(&self) -> u16 {
        self.size.1 - 2
    }

    fn draw(&self) -> (Dialog, Buffer) {
        let mut d = self.dialog();
        let buf = render(&mut d, self.size.0, self.size.1);
        (d, buf)
    }
}

fn kinds() -> Vec<Kind> {
    let blank = line("", 42);
    let confirm_kind = Kind {
        make: confirm,
        size: (42, 6),
        title: "Archive",
        accept: ("Archive", 2, 10),
        other: Some(("Cancel", 32, 39)),
        accepted: Outcome::confirmed(),
        drawing: vec![
            top("Archive", 42),
            blank.clone(),
            line("Archive cpd-ui?", 42),
            blank.clone(),
            buttons("Archive", Some("Cancel"), 42),
            bottom(42),
        ],
    };
    let input_kind = Kind {
        make: || input("sketch"),
        size: (42, 10),
        title: "New Plan",
        accept: ("OK", 2, 5),
        other: Some(("Cancel", 32, 39)),
        accepted: Outcome::value("sketch".into()),
        drawing: vec![
            top("New Plan", 42),
            blank.clone(),
            line(&format!("╭─{}{}╮", pill("Name"), "─".repeat(29)), 42),
            line(&format!("│ {:<34} │", "sketch "), 42),
            line(&format!("╰{}╯", "─".repeat(36)), 42),
            blank.clone(),
            blank.clone(),
            blank.clone(),
            buttons("OK", Some("Cancel"), 42),
            bottom(42),
        ],
    };
    let text_kind = Kind {
        make: no_cockpit,
        size: (42, 6),
        title: "session open l-ship",
        accept: ("Close", 2, 8),
        other: None,
        accepted: Outcome::confirmed(),
        drawing: vec![
            top("session open l-ship", 42),
            blank.clone(),
            line("no_cockpit: No cockpit.", 42),
            blank.clone(),
            buttons("Close", None, 42),
            bottom(42),
        ],
    };
    let checklist_kind = Kind {
        make: checklist,
        size: (42, 10),
        title: "Retire records of cpd",
        accept: ("Approve", 2, 10),
        other: Some(("Later", 33, 39)),
        accepted: Outcome::confirmed(),
        drawing: vec![
            top("Retire records of cpd", 42),
            blank.clone(),
            line("[ ] r-1 — duplicate r-0", 42),
            line("    use sqlite as the store", 42),
            blank.clone(),
            line("[ ] r-2 — documented docs/a.md", 42),
            line("    wire format", 42),
            blank,
            buttons("Approve", Some("Later"), 42),
            bottom(42),
        ],
    };
    vec![confirm_kind, input_kind, text_kind, checklist_kind]
}

/// The pill at (`x`, `y`) reads `label`, its text is inverted in `colour` and its caps are `colour` on the terminal's background.
fn assert_pill(buf: &Buffer, x: u16, y: u16, label: &str, colour: Color) {
    let width = u16::try_from(label.chars().count()).unwrap();
    let reset = Cell::EMPTY.bg;
    let drawn: String = (x..x + width + 2).map(|x| buf[(x, y)].symbol()).collect();
    assert_eq!(drawn, pill(label), "the pill at ({x}, {y})");
    for cap in [x, x + width + 1] {
        let cell = &buf[(cap, y)];
        assert_eq!(
            (cell.fg, cell.bg),
            (colour, reset),
            "the cap at ({cap}, {y}): {cell:?}"
        );
    }
    for at in x + 1..=x + width {
        let cell = &buf[(at, y)];
        assert_eq!(
            (cell.fg, cell.bg),
            (colour, reset),
            "the text at ({at}, {y}): {cell:?}"
        );
        assert!(
            cell.modifier.contains(ratatui::style::Modifier::REVERSED),
            "the text at ({at}, {y}) is reversed: {cell:?}"
        );
    }
}

#[test]
fn every_kind_draws_a_rounded_frame_with_its_title_pill_on_the_top_border() {
    for k in kinds() {
        let (_, buf) = k.draw();
        let rows = rows(&buf);
        let (w, h) = (usize::from(k.size.0), usize::from(k.size.1));
        assert_eq!(rows[0], top(k.title, w), "{}", k.title);
        assert_eq!(rows[h - 1], bottom(w), "{}", k.title);
        for (y, row) in rows.iter().enumerate().take(h - 1).skip(1) {
            let cells: Vec<char> = row.chars().collect();
            assert_eq!(
                (cells[0], cells[w - 1], cells.len()),
                ('│', '│', w),
                "{} row {y}: {row:?}",
                k.title
            );
        }
        // the title is shown there and nowhere else
        assert!(
            rows.iter()
                .skip(1)
                .all(|r| !r.contains(&format!("──{}", pill(k.title)))),
            "{}: the old section-title row is gone: {rows:?}",
            k.title
        );
    }
}

#[test]
fn the_frame_is_colour_8_and_the_title_pill_is_the_grey_read_only_pill() {
    for k in kinds() {
        let (_, buf) = k.draw();
        let (w, h) = k.size;
        let pill_end = 2 + u16::try_from(k.title.chars().count()).unwrap() + 1;
        for x in (0..w).filter(|x| !(2..=pill_end).contains(x)) {
            assert_eq!(buf[(x, 0)].fg, C8, "{} top {x}", k.title);
        }
        for x in 0..w {
            assert_eq!(buf[(x, h - 1)].fg, C8, "{} bottom {x}", k.title);
        }
        for y in 1..h - 1 {
            assert_eq!(
                (buf[(0, y)].fg, buf[(w - 1, y)].fg),
                (C8, C8),
                "{} walls row {y}",
                k.title
            );
        }
        crate::section::check::grey_pill(&buf, 2, 0, k.title);
    }
}

#[test]
fn the_rows_are_the_title_border_a_blank_row_the_body_a_blank_row_the_buttons_and_the_closing_border()
 {
    for k in kinds() {
        let (_, buf) = k.draw();
        assert_eq!(rows(&buf), k.drawing, "{}", k.title);
    }
    // H = b + 5: the body of 5 rows at 42 x 10, 3 at 42 x 8
    let rows10 = rows(&render(&mut text(20), 42, 10));
    assert_eq!(rows10[1], line("", 42));
    assert_eq!(
        rows10[2..7],
        (1..=5)
            .map(|n| line(&format!("line {n}"), 42))
            .collect::<Vec<_>>()
    );
    assert_eq!(rows10[7], line("", 42));
    assert!(
        rows10[8].starts_with(&format!("│ {L}Close{R}")),
        "{:?}",
        rows10[8]
    );
    let rows8 = rows(&render(&mut text(20), 42, 8));
    assert_eq!(
        rows8[2..5],
        (1..=3)
            .map(|n| line(&format!("line {n}"), 42))
            .collect::<Vec<_>>()
    );
    assert_eq!(rows8[5], line("", 42));
}

#[test]
fn the_content_has_one_column_of_padding_inside_each_wall() {
    for k in kinds() {
        let (_, buf) = k.draw();
        let (w, h) = k.size;
        for y in 1..h - 1 {
            for x in [1, w - 2] {
                assert_eq!(buf[(x, y)].symbol(), " ", "{} padding ({x}, {y})", k.title);
            }
        }
        // content starts at column 2 and no padding row above or below it: the body's first row is row 2 (row 1 is spacing)
        assert_ne!(buf[(2, 2)].symbol(), " ", "{}", k.title);
        assert_ne!(buf[(2, h - 2)].symbol(), " ", "{}", k.title);
    }
    // a long line is wrapped at W - 4 = 38 and ends at column 39, the padding at 40 stays blank
    let mut d = Dialog::new(
        "T".into(),
        "x".repeat(100),
        Mode::Confirm { yes: "Yes".into() },
    );
    let drawn = rows(&render(&mut d, 42, 10));
    assert_eq!(drawn[2], line(&"x".repeat(38), 42));
    assert_eq!(drawn[3], line(&"x".repeat(38), 42));
    assert_eq!(drawn[4], line(&"x".repeat(24), 42));
    let mut d = Dialog::new("T".into(), "x".repeat(100), Mode::Text);
    assert_eq!(rows(&render(&mut d, 42, 10))[2], line(&"x".repeat(38), 42));
}

#[test]
fn the_accept_pill_is_flush_left_and_the_cancel_pill_flush_right_on_the_buttons_row() {
    for k in kinds() {
        let (_, buf) = k.draw();
        let row = &rows(&buf)[usize::from(k.buttons_row())];
        let cells: Vec<char> = row.chars().collect();
        let (accept, from, to) = k.accept;
        assert_eq!(
            (cells[usize::from(from)], cells[usize::from(to)]),
            ('\u{E0B6}', '\u{E0B4}'),
            "{accept}"
        );
        assert_eq!(
            to - from + 1,
            u16::try_from(accept.chars().count()).unwrap() + 2,
            "{accept}"
        );
        let mut at = usize::from(to) + 1;
        if let Some((other, from, to)) = k.other {
            assert_eq!(
                (cells[usize::from(from)], cells[usize::from(to)]),
                ('\u{E0B6}', '\u{E0B4}'),
                "{other}"
            );
            assert_eq!(
                usize::from(to),
                usize::from(k.size.0) - 3,
                "{other} is flush right"
            );
            assert!(
                cells[at..usize::from(from)].iter().all(|c| *c == ' '),
                "nothing is drawn between the pills: {row:?}"
            );
            at = usize::from(to) + 1;
        }
        assert!(
            cells[at..usize::from(k.size.0) - 2]
                .iter()
                .all(|c| *c == ' '),
            "{row:?}"
        );
        assert_eq!(*row, k.drawing[usize::from(k.buttons_row())]);
    }
}

#[test]
fn the_accept_pill_is_blue_12_and_the_other_pill_is_white_15() {
    for k in kinds() {
        let (_, buf) = k.draw();
        let y = k.buttons_row();
        let (accept, from, _) = k.accept;
        assert_pill(&buf, from, y, accept, C12);
        if let Some((other, from, _)) = k.other {
            assert_pill(&buf, from, y, other, C15);
        }
    }
}

#[test]
fn a_click_on_a_pill_acts_caps_included_and_nowhere_else_on_the_row_does() {
    for k in kinds() {
        let y = k.buttons_row();
        let acts = |column: u16| {
            let (mut d, _) = k.draw();
            d.press_mouse(click(column, y))
        };
        let (_, from, to) = k.accept;
        for column in from..=to {
            assert_eq!(
                acts(column),
                Some(k.accepted.clone()),
                "{} accept column {column}",
                k.title
            );
        }
        let mut pills = vec![(from, to)];
        if let Some((_, from, to)) = k.other {
            for column in from..=to {
                assert_eq!(
                    acts(column),
                    Some(Outcome::cancelled()),
                    "{} cancel column {column}",
                    k.title
                );
            }
            pills.push((from, to));
        }
        for column in
            (0..k.size.0).filter(|c| !pills.iter().any(|(from, to)| (from..=to).contains(&c)))
        {
            assert_eq!(
                acts(column),
                None,
                "{} column {column} of the buttons row",
                k.title
            );
        }
        // a right click is not a click, and a click on the row above or below the pills is none
        let (mut d, _) = k.draw();
        assert_eq!(
            d.press_mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Right),
                ..click(from, y)
            }),
            None
        );
    }
}

#[test]
fn a_click_on_the_border_or_a_wall_does_nothing() {
    for k in kinds() {
        let (w, h) = k.size;
        for (column, row) in (0..w)
            .flat_map(|x| [(x, 0), (x, h - 1)])
            .chain((0..h).flat_map(|y| [(0, y), (w - 1, y)]))
        {
            let (mut d, _) = k.draw();
            assert_eq!(
                d.press_mouse(click(column, row)),
                None,
                "{} at {column},{row}",
                k.title
            );
        }
        // the padding beside a pill is not the pill
        for column in [1, w - 2] {
            let (mut d, _) = k.draw();
            assert_eq!(
                d.press_mouse(click(column, k.buttons_row())),
                None,
                "{} padding {column}",
                k.title
            );
        }
    }
}

#[test]
fn the_scroll_position_is_muted_on_the_buttons_row_between_the_pills() {
    // `text`: no right pill, so the position ends at column W - 3: `1-5/20` at columns 34 to 39
    let buf = render(&mut text(20), 42, 10);
    let row = &rows(&buf)[8];
    assert_eq!(
        *row,
        line(&format!("{}{}1-5/20", pill("Close"), " ".repeat(25)), 42)
    );
    for x in 34..=39 {
        assert_eq!(buf[(x, 8)].fg, C8, "position column {x}");
    }
    // `confirm`: the last character is one blank before the right pill's left cap
    let mut d = Dialog::new(
        "T".into(),
        (1..=20)
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n"),
        Mode::Confirm {
            yes: "Archive".into(),
        },
    );
    let buf = render(&mut d, 42, 10);
    let row = &rows(&buf)[8];
    assert_eq!(
        *row,
        line(
            &format!(
                "{}{}1-5/20 {}",
                pill("Archive"),
                " ".repeat(14),
                pill("Cancel")
            ),
            42
        )
    );
    for x in 25..=30 {
        assert_eq!(buf[(x, 8)].fg, C8, "position column {x}");
    }
    // it is omitted when it would leave no blank cell between it and the left pill
    let mut d = Dialog::new(
        "T".into(),
        (1..=20)
            .map(|n| format!("l{n}"))
            .collect::<Vec<_>>()
            .join("\n"),
        Mode::Confirm {
            yes: "Archive".into(),
        },
    );
    let narrow = &rows(&render(&mut d, 28, 10))[8];
    assert!(!narrow.contains("1-5/20"), "{narrow:?}");
    let fits = &rows(&render(&mut d, 29, 10))[8];
    assert_eq!(
        *fits,
        line(
            &format!("{}{}1-5/20 {}", pill("Archive"), " ", pill("Cancel")),
            29
        )
    );
}

#[test]
fn confirm_draws_the_question_and_both_buttons() {
    let buf = rows(&render(&mut confirm(), 42, 6));
    assert_eq!(buf[2], line("Archive cpd-ui?", 42));
    assert_eq!(buf[4], buttons("Archive", Some("Cancel"), 42));
}

#[test]
fn a_long_question_wraps_at_the_width() {
    let mut d = Dialog::new(
        "T".into(),
        "aaaa bbbb cccc dddd".into(),
        Mode::Confirm { yes: "Yes".into() },
    );
    let buf = rows(&render(&mut d, 14, 10));
    assert_eq!(
        buf[2..5],
        [line("aaaa bbbb", 14), line("cccc dddd", 14), line("", 14)]
    );
}

#[test]
fn wrap_breaks_at_spaces_cuts_long_words_and_keeps_line_breaks() {
    assert_eq!(wrap("aaa bbb ccc", 7), ["aaa bbb", "ccc"]);
    assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    assert_eq!(wrap("a\n\nb\n", 5), ["a", "", "b"]);
    assert_eq!(wrap("", 5), Vec::<String>::new());
    assert_eq!(wrap("tab\there", 20), ["tab here"]);
    assert_eq!(wrap("  ab cd ef", 6), ["  ab", "cd ef"]);
}

#[test]
fn enter_or_y_confirms_and_esc_n_or_q_cancels() {
    for code in [KeyCode::Enter, KeyCode::Char('y')] {
        assert_eq!(
            confirm().press_key(press(code)),
            Some(Outcome::confirmed()),
            "{code:?}"
        );
        assert_eq!(
            text(3).press_key(press(code)),
            Some(Outcome::confirmed()),
            "{code:?}"
        );
    }
    for code in [KeyCode::Esc, KeyCode::Char('n'), KeyCode::Char('q')] {
        assert_eq!(
            confirm().press_key(press(code)),
            Some(Outcome::cancelled()),
            "{code:?}"
        );
    }
    for code in [KeyCode::Char('Y'), KeyCode::Char(' '), KeyCode::Tab] {
        assert_eq!(confirm().press_key(press(code)), None, "{code:?}");
    }
    assert_eq!(
        confirm().press_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL)),
        None
    );
    let mut release = press(KeyCode::Enter);
    release.kind = KeyEventKind::Release;
    assert_eq!(confirm().press_key(release), None);
}

#[test]
fn text_has_one_button_and_scrolls_with_keys_and_the_wheel() {
    let mut d = text(20);
    let buf = rows(&render(&mut d, 30, 10));
    // the top border, a blank row, 5 body rows, a blank row, the buttons, the bottom border
    assert_eq!(
        buf[2..7],
        (1..=5)
            .map(|n| line(&format!("line {n}"), 30))
            .collect::<Vec<_>>()
    );
    assert!(
        buf[8].starts_with(&format!("│ {L}Close{R}")),
        "{:?}",
        buf[8]
    );
    assert!(buf[8].ends_with("1-5/20 │"), "{:?}", buf[8]);
    let first = |d: &mut Dialog| rows(&render(d, 30, 10))[2].clone();
    assert_eq!(d.press_key(press(KeyCode::Down)), None);
    assert_eq!(first(&mut d), line("line 2", 30));
    assert_eq!(d.press_mouse(wheel(MouseEventKind::ScrollDown)), None);
    assert_eq!(first(&mut d), line("line 5", 30));
    d.press_key(press(KeyCode::PageDown));
    assert_eq!(first(&mut d), line("line 10", 30));
    d.press_mouse(wheel(MouseEventKind::ScrollUp));
    d.press_key(press(KeyCode::Char('k')));
    assert_eq!(first(&mut d), line("line 6", 30));
    d.press_key(press(KeyCode::Char('j')));
    d.press_key(press(KeyCode::PageUp));
    assert_eq!(first(&mut d), line("line 2", 30));
    for _ in 0..10 {
        d.press_mouse(wheel(MouseEventKind::ScrollDown));
    }
    let end = rows(&render(&mut d, 30, 10));
    assert_eq!(
        end[2..7],
        (16..=20)
            .map(|n| line(&format!("line {n}"), 30))
            .collect::<Vec<_>>()
    );
    assert!(end[8].ends_with("16-20/20 │"), "{:?}", end[8]);
}

#[test]
fn text_that_fits_shows_no_position_and_does_not_scroll() {
    let mut d = text(3);
    let buf = rows(&render(&mut d, 30, 10));
    assert_eq!(buf[8], buttons("Close", None, 30));
    d.press_mouse(wheel(MouseEventKind::ScrollDown));
    assert_eq!(rows(&render(&mut d, 30, 10))[2], line("line 1", 30));
}

#[test]
fn a_confirm_taller_than_its_area_scrolls_too() {
    let mut d = Dialog::new(
        "T".into(),
        "a\nb\nc\nd\ne".into(),
        Mode::Confirm { yes: "Yes".into() },
    );
    assert_eq!(
        rows(&render(&mut d, 30, 8))[2..5],
        [line("a", 30), line("b", 30), line("c", 30)]
    );
    d.press_key(press(KeyCode::Down));
    assert_eq!(
        rows(&render(&mut d, 30, 8))[2..5],
        [line("b", 30), line("c", 30), line("d", 30)]
    );
}

#[test]
fn input_types_edits_and_moves_the_cursor() {
    let mut d = input("");
    type_str(&mut d, "yqn");
    d.press_key(press(KeyCode::Left));
    d.press_key(press(KeyCode::Backspace));
    type_str(&mut d, "x");
    d.press_key(press(KeyCode::Home));
    d.press_key(press(KeyCode::Delete));
    type_str(&mut d, "a");
    d.press_key(press(KeyCode::End));
    type_str(&mut d, "z");
    let Mode::Input { field, .. } = &d.mode else {
        panic!()
    };
    assert_eq!(field.text(), "axnz");
    // control keys are not text
    assert_eq!(
        d.press_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
        None
    );
    let Mode::Input { field, .. } = &d.mode else {
        panic!()
    };
    assert_eq!(field.text(), "axnz");
}

#[test]
fn y_n_and_q_are_text_in_an_input_and_do_not_close_it() {
    let mut d = input("");
    type_str(&mut d, "ynq");
    let Mode::Input { field, .. } = &d.mode else {
        panic!()
    };
    assert_eq!(field.text(), "ynq");
}

#[test]
fn input_enter_accepts_a_valid_value_trimmed_and_esc_cancels() {
    let mut d = input("");
    type_str(&mut d, "  pay-4521 ");
    assert_eq!(
        d.press_key(press(KeyCode::Enter)),
        Some(Outcome::value("pay-4521".into()))
    );
    assert_eq!(
        input("abc").press_key(press(KeyCode::Esc)),
        Some(Outcome::cancelled())
    );
}

#[test]
fn input_refuses_a_bad_value_inside_the_popup_and_the_user_keeps_typing() {
    let mut d = input("");
    type_str(&mut d, "Bad");
    assert_eq!(d.press_key(press(KeyCode::Enter)), None);
    let buf = rows(&render(&mut d, 60, 11));
    assert!(buf[5].starts_with("│ Invalid Plan id"), "{buf:?}");
    assert!(buf[3].starts_with("│ │ Bad"), "{buf:?}");
    // typing clears the refusal, and a fixed value is accepted
    for _ in 0..3 {
        d.press_key(press(KeyCode::Backspace));
    }
    assert_eq!(rows(&render(&mut d, 60, 11))[5], line("", 60));
    type_str(&mut d, "good");
    assert_eq!(
        d.press_key(press(KeyCode::Enter)),
        Some(Outcome::value("good".into()))
    );
}

#[test]
fn an_empty_input_is_refused() {
    let mut d = input("   ");
    assert_eq!(d.press_key(press(KeyCode::Enter)), None);
    assert!(matches!(
        &d.mode,
        Mode::Input {
            refused: Some(_),
            ..
        }
    ));
}

#[test]
fn the_buttons_of_an_input_accept_and_cancel() {
    let mut d = input("ok-id");
    render(&mut d, 44, 10);
    assert_eq!(
        d.press_mouse(click(3, 8)),
        Some(Outcome::value("ok-id".into()))
    );
    assert_eq!(d.press_mouse(click(36, 8)), Some(Outcome::cancelled()));
    let mut bad = input("Bad");
    render(&mut bad, 44, 10);
    assert_eq!(bad.press_mouse(click(3, 8)), None);
    assert!(matches!(
        &bad.mode,
        Mode::Input {
            refused: Some(_),
            ..
        }
    ));
}

#[test]
fn a_checklist_entry_is_ticked_by_a_click_inside_the_frame() {
    let mut d = checklist();
    render(&mut d, 42, 10);
    assert_eq!(d.press_mouse(click(5, 5)), None);
    assert_eq!(
        d.ticks(),
        Some((vec!["r-2".to_owned()], vec!["r-1".to_owned()]))
    );
    assert_eq!(
        rows(&render(&mut d, 42, 10))[5],
        line("[x] r-2 — documented docs/a.md", 42)
    );
}

#[test]
fn a_dialog_in_a_tiny_area_draws_without_panicking() {
    let sizes = (0..=3).flat_map(|w| (0..=3).map(move |h| (w, h))).chain([
        (10, 2),
        (58, 3),
        (5, 40),
        (4, 4),
        (6, 5),
    ]);
    for (w, h) in sizes {
        for mut d in [confirm(), input("abc"), text(20), checklist()] {
            render(&mut d, w, h);
            d.press_mouse(click(0, 0));
            d.press_mouse(click(w.saturating_sub(1), h.saturating_sub(2)));
            d.press_key(press(KeyCode::Down));
        }
        let mut d = input("Bad");
        d.press_key(press(KeyCode::Enter));
        render(&mut d, w, h);
    }
}
