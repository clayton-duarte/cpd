//! The `dialog` view (commands.md §13.2, "The dialog kit"; hs-d295, hs-d578) as a user sees it in its pane: each kind
//! draws its own rounded frame with the title pill on the border, takes its keys, clicks and wheel, and writes its
//! outcome to the `--out` file. The pane is a `TestBackend` (`pane.rs`), the wheel and the click are mouse events.

use serde_json::json;

use super::pane::{Pane, bottom, buttons, has_pill, pill, row, top};

/// `cpd tui view dialog <kind> --title=Title …` in a pane of `width` by `height`.
fn start(kind: &str, fields: &[&str], width: u16, height: u16) -> Pane {
    Pane::start(kind, "Title", fields, width, height)
}

const CONFIRM: [&str; 2] = ["--text=Archive cpd-ui?", "--yes-label=Archive"];

#[test]
fn confirm_draws_the_question_between_blank_rows_and_the_buttons() {
    let mut pane = start("confirm", &CONFIRM, 42, 6);
    let rows = pane.screen();
    assert!(has_pill(&rows, "Archive"));
    // the title is on the border only; a blank row, the question, a blank row, the buttons
    assert_eq!(
        rows[1..5],
        [
            row(42, ""),
            row(42, "Archive cpd-ui?"),
            row(42, ""),
            buttons(42, "Archive", Some("Cancel"))
        ]
    );
}

#[test]
fn every_kind_is_drawn_whole_as_a_rounded_box_with_the_title_pill_on_the_border() {
    // confirm
    let mut pane = Pane::start("confirm", "Archive", &CONFIRM, 42, 6);
    let rows = pane.screen();
    assert!(has_pill(&rows, "Archive") && has_pill(&rows, "Cancel"));
    let want = [
        top(42, "Archive"),
        row(42, ""),
        row(42, "Archive cpd-ui?"),
        row(42, ""),
        buttons(42, "Archive", Some("Cancel")),
        bottom(42),
    ];
    assert_eq!(rows, want);

    // input: the box is rows 2 to 4, the two refusal rows and a blank row follow, the buttons are on row 8
    let fields = ["--prompt=Name:", "--initial=sketch", "--validate=plan-id"];
    let mut pane = Pane::start("input", "New Plan", &fields, 42, 10);
    let rows = pane.screen();
    assert!(has_pill(&rows, "OK") && has_pill(&rows, "Cancel"));
    let box_top = format!("╭─{}{}╮", pill("Name"), "─".repeat(29));
    let box_text = format!("│ sketch{} │", " ".repeat(34 - "sketch".len()));
    let want = [
        top(42, "New Plan"),
        row(42, ""),
        row(42, &box_top),
        row(42, &box_text),
        row(42, &format!("╰{}╯", "─".repeat(36))),
        row(42, ""),
        row(42, ""),
        row(42, ""),
        buttons(42, "OK", Some("Cancel")),
        bottom(42),
    ];
    assert_eq!(rows, want);

    // text
    let mut pane = Pane::start(
        "text",
        "session open l-ship",
        &["--body=no_cockpit: No cockpit."],
        42,
        6,
    );
    let rows = pane.screen();
    assert!(has_pill(&rows, "Close"));
    let want = [
        top(42, "session open l-ship"),
        row(42, ""),
        row(42, "no_cockpit: No cockpit."),
        row(42, ""),
        buttons(42, "Close", None),
        bottom(42),
    ];
    assert_eq!(rows, want);
}

#[test]
fn confirm_y_and_enter_and_a_click_on_the_first_button_confirm() {
    for how in ["y", "Enter", "click"] {
        let mut pane = start("confirm", &CONFIRM, 42, 6);
        assert!(has_pill(&pane.screen(), "Archive"));
        match how {
            // the accept pill is at 0-based columns 2 to 10, row 4
            "click" => pane.click(5 - 1, 5 - 1),
            key => pane.keys(&[key]),
        }
        assert_eq!(pane.outcome(), json!({"confirmed": true}), "{how}");
    }
}

#[test]
fn confirm_esc_n_q_and_a_click_on_the_second_button_cancel() {
    for how in ["Escape", "n", "q", "click"] {
        let mut pane = start("confirm", &CONFIRM, 42, 6);
        assert!(has_pill(&pane.screen(), "Cancel"));
        match how {
            // the Cancel pill is flush right, at 0-based columns 32 to 39, row 4
            "click" => pane.click(36 - 1, 5 - 1),
            key => pane.keys(&[key]),
        }
        assert_eq!(pane.outcome(), json!({"confirmed": false}), "{how}");
    }
}

#[test]
fn a_click_between_the_pills_on_the_padding_or_the_wall_does_nothing() {
    let mut pane = start("confirm", &CONFIRM, 42, 6);
    assert!(has_pill(&pane.screen(), "Archive"));
    // 1-based columns: the wall (1), the padding (2), between the pills (15), the padding (41), the wall (42); then the
    // accept pill's row elsewhere: the blank row above the buttons (4)
    for (column, row) in [(1, 5), (2, 5), (15, 5), (41, 5), (42, 5), (5, 4), (5, 6)] {
        pane.click(column - 1, row - 1);
    }
    let rows = pane.screen();
    assert!(has_pill(&rows, "Archive"));
    assert!(!pane.closed(), "{rows:?}");
}

const INPUT: [&str; 3] = [
    "--prompt=Fork cpd-ui into a new Plan, name:",
    "--initial=ab",
    "--validate=plan-id",
];

#[test]
fn input_types_a_refused_value_stays_open_and_a_fixed_one_is_written() {
    let mut pane = start("input", &INPUT, 60, 10);
    let rows = pane.screen();
    assert!(has_pill(&rows, "OK"));
    assert_eq!(rows[0], top(60, "Title"));
    assert!(
        rows[2].contains("Fork cpd-ui into a new Plan, name") && !rows[2].contains("name:"),
        "{rows:?}"
    );
    assert!(rows[3].starts_with("│ │ ab"), "{rows:?}");
    // "abBad" is no Plan id: refused inside the popup, which stays
    pane.keys(&["B", "a", "d", "Enter"]);
    let rows = pane.screen();
    assert!(
        rows.iter().any(|l| l.contains("Invalid Plan id")),
        "{rows:?}"
    );
    assert!(rows[3].starts_with("│ │ abBad"), "{rows:?}");
    assert!(
        rows[5].contains("Invalid Plan id"),
        "the refusal sits under the box: {rows:?}"
    );
    assert!(!pane.closed());
    assert!(!pane.out().exists());
    // the User keeps typing: y, n and q are text; the refusal goes at the first edit
    pane.keys(&["BSpace", "BSpace", "BSpace", "y", "n", "q", "-", "1"]);
    let rows = pane.screen();
    assert!(
        rows.iter().any(|l| l.starts_with("│ │ abynq-1")),
        "{rows:?}"
    );
    assert!(!rows.iter().any(|l| l.contains("Invalid")), "{rows:?}");
    pane.keys(&["Enter"]);
    assert_eq!(
        pane.outcome(),
        json!({"confirmed": true, "value": "abynq-1"})
    );
}

#[test]
fn input_esc_cancels_and_writes_no_value() {
    let mut pane = start("input", &INPUT, 60, 10);
    assert!(has_pill(&pane.screen(), "OK"));
    pane.keys(&["x", "Escape"]);
    assert_eq!(pane.outcome(), json!({"confirmed": false}));
}

#[test]
fn input_the_ok_button_accepts_and_validates() {
    let mut pane = start(
        "input",
        &[
            "--prompt=Title: ",
            "--initial=Old title",
            "--validate=title",
        ],
        60,
        10,
    );
    assert!(has_pill(&pane.screen(), "OK"));
    // the OK pill is at 0-based columns 2 to 5, row 8
    pane.click(4 - 1, 9 - 1);
    assert_eq!(
        pane.outcome(),
        json!({"confirmed": true, "value": "Old title"})
    );
}

#[test]
fn input_an_empty_title_is_refused_by_the_titles_check() {
    let mut pane = start(
        "input",
        &["--prompt=Title: ", "--initial=", "--validate=title"],
        60,
        10,
    );
    assert!(has_pill(&pane.screen(), "OK"));
    pane.keys(&["Enter"]);
    let rows = pane.screen();
    assert!(rows.iter().any(|l| l.contains("1 to 200")), "{rows:?}");
    assert!(!pane.closed());
}

#[test]
fn input_box_in_a_popup_shows_the_label_and_grows_with_the_value() {
    let mut pane = start(
        "input",
        &["--prompt=Title: ", "--initial=", "--validate=title"],
        44,
        15,
    );
    let rows = pane.screen();
    assert!(has_pill(&rows, "OK"));
    // the box is 40 wide (the dialog's padding is its margin): `tw` is 36
    assert_eq!(
        rows[2],
        format!("│ ╭─{}{}╮ │", pill("Title"), "─".repeat(30))
    );
    assert!(rows[3].starts_with("│ │ "), "{rows:?}");
    assert!(rows[4].starts_with("│ ╰"), "{rows:?}");
    let x = "x".repeat(45);
    pane.literal(&x);
    let rows = pane.screen();
    assert!(
        rows.get(5).is_some_and(|l| l.starts_with("│ ╰"))
            && rows[4].starts_with(&format!("│ │ {}", "x".repeat(9))),
        "{rows:?}"
    );
    assert!(
        rows[3].starts_with(&format!("│ │ {}", "x".repeat(36))),
        "{rows:?}"
    );
    assert!(
        rows[4].starts_with(&format!("│ │ {}", "x".repeat(9))),
        "{rows:?}"
    );
}

fn body() -> String {
    (1..=20)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn text_the_wheel_and_the_keys_scroll_it() {
    let body = format!("--body={}", body());
    let mut pane = start("text", &[&body], 30, 10);
    let rows = pane.screen();
    assert!(has_pill(&rows, "Close"));
    assert_eq!(rows[0], top(30, "Title"));
    // the body is rows 2 to 6; the position is on the buttons row, muted, after the pill
    assert_eq!(rows[2], row(30, "line 1"));
    assert_eq!(rows[6], row(30, "line 5"));
    assert!(rows[8].ends_with("1-5/20 │"), "{rows:?}");
    // the wheel: 3 rows a notch
    pane.wheel_down(5 - 1, 5 - 1);
    let rows = pane.screen();
    assert_eq!(rows[2], row(30, "line 4"), "the wheel down");
    assert!(rows[8].ends_with("4-8/20 │"), "{rows:?}");
    pane.wheel_up(5 - 1, 5 - 1);
    assert_eq!(pane.screen()[2], row(30, "line 1"), "the wheel up");
    // the keys
    pane.keys(&["j"]);
    assert_eq!(pane.screen()[2], row(30, "line 2"), "j");
    pane.keys(&["NPage"]);
    assert_eq!(pane.screen()[2], row(30, "line 7"), "PgDn");
    pane.keys(&["Down", "Down"]);
    assert_eq!(pane.screen()[2], row(30, "line 9"), "Down");
    pane.keys(&["k"]);
    pane.keys(&["PPage"]);
    assert_eq!(pane.screen()[2], row(30, "line 3"), "k and PgUp");
    pane.keys(&["Up"]);
    assert_eq!(pane.screen()[2], row(30, "line 2"), "Up");
    // the end of the text stops the scroll
    for _ in 0..8 {
        pane.wheel_down(5 - 1, 5 - 1);
    }
    let rows = pane.screen();
    assert_eq!(rows[6], row(30, "line 20"), "the end");
    assert_eq!(rows[2], row(30, "line 16"));
    // Enter closes: confirmed
    pane.keys(&["Enter"]);
    assert_eq!(pane.outcome(), json!({"confirmed": true}));
}

#[test]
fn text_esc_closes_it_and_a_click_on_close_closes_it() {
    let mut pane = start("text", &["--body=short"], 30, 6);
    assert!(has_pill(&pane.screen(), "Close"));
    pane.keys(&["Escape"]);
    assert_eq!(pane.outcome(), json!({"confirmed": false}));

    let mut pane = start("text", &["--body=short"], 30, 6);
    let rows = pane.screen();
    assert!(has_pill(&rows, "Close"));
    assert_eq!(rows[2], row(30, "short"));
    // the Close pill is at 0-based columns 2 to 8, row 4
    pane.click(4 - 1, 5 - 1);
    assert_eq!(pane.outcome(), json!({"confirmed": true}));
}
