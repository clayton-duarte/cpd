//! The `dialog` view (commands.md §13.2, "The dialog kit"; hs-d295, hs-d578) on a private tmux server: each kind draws
//! its own rounded frame with the title pill on the border, takes its keys, clicks and wheel, and writes its outcome to
//! the `--out` file. The popup is opened with `-B`, so tmux gives its program a pane of the popup's whole size, which
//! is the pane these tests start. The wheel and the click are raw mouse bytes sent to the pane.

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::{CPD, TestEnv, wait_until};
use serde_json::{Value, json};

fn tmux_out(env: &TestEnv, args: &[&str]) -> String {
    let out = env.tmux(args);
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// `cpd tui view dialog <kind> --title=Title --out=<out> <fields>` in a pane of `width` by `height`.
fn start(env: &TestEnv, kind: &str, fields: &[&str], width: u16, height: u16) -> (String, PathBuf) {
    start_titled(env, kind, "Title", fields, width, height)
}

/// [`start`] with the title `title`.
fn start_titled(env: &TestEnv, kind: &str, title: &str, fields: &[&str], width: u16, height: u16) -> (String, PathBuf) {
    let out = env.root().join(format!("{kind}-out.json"));
    let (w, h) = (width.to_string(), height.to_string());
    let (out_arg, title_arg) = (format!("--out={}", out.display()), format!("--title={title}"));
    let mut args = vec!["new-session", "-d", "-s", "view", "-x", &w, "-y", &h, "-P", "-F", "#{pane_id}", CPD, "tui", "view", "dialog", kind, &title_arg, &out_arg];
    args.extend_from_slice(fields);
    (tmux_out(env, &args).trim().to_owned(), out)
}

fn screen(env: &TestEnv, pane: &str) -> Vec<String> {
    tmux_out(env, &["capture-pane", "-p", "-t", pane]).lines().map(|l| l.trim_end().to_owned()).collect()
}

fn wait_screen(env: &TestEnv, pane: &str, what: &str, done: impl FnMut(&[String]) -> bool) -> Vec<String> {
    common::wait_screen(what, || screen(env, pane), done)
}

fn keys(env: &TestEnv, pane: &str, keys: &[&str]) {
    let mut args = vec!["send-keys", "-t", pane];
    args.extend_from_slice(keys);
    tmux_out(env, &args);
}

fn closed(env: &TestEnv) -> bool {
    !env.tmux(&["has-session", "-t", "view"]).status.success()
}

/// Waits for the view to close; its outcome file.
fn outcome(env: &TestEnv, out: &PathBuf) -> Value {
    wait_until("the view to close", || closed(env));
    serde_json::from_str(&fs::read_to_string(out).expect("the view wrote its outcome")).unwrap()
}

/// A left button press at 1-based `(column, row)`, as the terminal sends it (SGR mouse reporting).
fn click(env: &TestEnv, pane: &str, column: u16, row: u16) {
    tmux_out(env, &["send-keys", "-t", pane, "-l", &format!("\x1b[<0;{column};{row}M")]);
}

/// One wheel notch down (`64` is up) at 1-based `(column, row)`.
fn wheel(env: &TestEnv, pane: &str, button: u8, column: u16, row: u16) {
    tmux_out(env, &["send-keys", "-t", pane, "-l", &format!("\x1b[<{button};{column};{row}M")]);
}

/// A pill: its label between the caps U+E0B6 and U+E0B4, no padding.
pub(crate) fn pill(label: &str) -> String {
    format!("\u{E0B6}{label}\u{E0B4}")
}

/// The top border of a popup `w` wide: `╭─`, the title pill, `─` up to `╮`.
pub(crate) fn top(w: usize, title: &str) -> String {
    format!("╭─{}{}╮", pill(title), "─".repeat(w - 5 - title.chars().count()))
}

/// The bottom border of a popup `w` wide.
pub(crate) fn bottom(w: usize) -> String {
    format!("╰{}╯", "─".repeat(w - 2))
}

/// A row of the inside of a popup `w` wide: wall, padding, `content` (blank to the width `w - 4`), padding, wall.
pub(crate) fn row(w: usize, content: &str) -> String {
    format!("│ {content}{} │", " ".repeat(w - 4 - content.chars().count()))
}

/// The buttons row of a popup `w` wide: the pill `accept` flush left and, when given, the pill `other` flush right.
pub(crate) fn buttons(w: usize, accept: &str, other: Option<&str>) -> String {
    let left = pill(accept);
    match other {
        Some(other) => {
            let right = pill(other);
            let gap = w - 4 - left.chars().count() - right.chars().count();
            row(w, &format!("{left}{}{right}", " ".repeat(gap)))
        }
        None => row(w, &left),
    }
}

/// The 0-based column of the left cap of the pill `label` in `line`.
pub(crate) fn column_of(line: &str, label: &str) -> usize {
    let at = line.find(&pill(label)).unwrap_or_else(|| panic!("{label:?} in {line:?}"));
    line[..at].chars().count()
}

fn has_pill(rows: &[String], label: &str) -> bool {
    rows.iter().any(|l| l.contains(&pill(label)))
}

const CONFIRM: [&str; 2] = ["--text=Archive cpd-ui?", "--yes-label=Archive"];

#[test]
fn confirm_draws_the_question_between_blank_rows_and_the_buttons() {
    let env = TestEnv::new();
    let (pane, _) = start(&env, "confirm", &CONFIRM, 42, 6);
    let rows = wait_screen(&env, &pane, "the dialog", |r| has_pill(r, "Archive"));
    // the title is on the border only; a blank row, the question, a blank row, the buttons
    assert_eq!(rows[1..5], [row(42, ""), row(42, "Archive cpd-ui?"), row(42, ""), buttons(42, "Archive", Some("Cancel"))]);
}

#[test]
fn every_kind_is_drawn_whole_as_a_rounded_box_with_the_title_pill_on_the_border() {
    // confirm
    let env = TestEnv::new();
    let (pane, _) = start_titled(&env, "confirm", "Archive", &CONFIRM, 42, 6);
    let rows = wait_screen(&env, &pane, "the confirm", |r| has_pill(r, "Archive") && has_pill(r, "Cancel"));
    let want = [top(42, "Archive"), row(42, ""), row(42, "Archive cpd-ui?"), row(42, ""), buttons(42, "Archive", Some("Cancel")), bottom(42)];
    assert_eq!(rows, want);

    // input: the box is rows 2 to 4, the two refusal rows and a blank row follow, the buttons are on row 8
    let env = TestEnv::new();
    let fields = ["--prompt=Name:", "--initial=sketch", "--validate=plan-id"];
    let (pane, _) = start_titled(&env, "input", "New Plan", &fields, 42, 10);
    let rows = wait_screen(&env, &pane, "the input", |r| has_pill(r, "OK") && has_pill(r, "Cancel"));
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
    let env = TestEnv::new();
    let (pane, _) = start_titled(&env, "text", "session open l-ship", &["--body=no_cockpit: No cockpit."], 42, 6);
    let rows = wait_screen(&env, &pane, "the text", |r| has_pill(r, "Close"));
    let want = [top(42, "session open l-ship"), row(42, ""), row(42, "no_cockpit: No cockpit."), row(42, ""), buttons(42, "Close", None), bottom(42)];
    assert_eq!(rows, want);
}

#[test]
fn confirm_y_and_enter_and_a_click_on_the_first_button_confirm() {
    for how in ["y", "Enter", "click"] {
        let env = TestEnv::new();
        let (pane, out) = start(&env, "confirm", &CONFIRM, 42, 6);
        wait_screen(&env, &pane, "the dialog", |r| has_pill(r, "Archive"));
        match how {
            // the accept pill is at 0-based columns 2 to 10, row 4
            "click" => click(&env, &pane, 5, 5),
            key => keys(&env, &pane, &[key]),
        }
        assert_eq!(outcome(&env, &out), json!({"confirmed": true}), "{how}");
    }
}

#[test]
fn confirm_esc_n_q_and_a_click_on_the_second_button_cancel() {
    for how in ["Escape", "n", "q", "click"] {
        let env = TestEnv::new();
        let (pane, out) = start(&env, "confirm", &CONFIRM, 42, 6);
        wait_screen(&env, &pane, "the dialog", |r| has_pill(r, "Cancel"));
        match how {
            // the Cancel pill is flush right, at 0-based columns 32 to 39, row 4
            "click" => click(&env, &pane, 36, 5),
            key => keys(&env, &pane, &[key]),
        }
        assert_eq!(outcome(&env, &out), json!({"confirmed": false}), "{how}");
    }
}

#[test]
fn a_click_between_the_pills_on_the_padding_or_the_wall_does_nothing() {
    let env = TestEnv::new();
    let (pane, _) = start(&env, "confirm", &CONFIRM, 42, 6);
    wait_screen(&env, &pane, "the dialog", |r| has_pill(r, "Archive"));
    // 1-based columns: the wall (1), the padding (2), between the pills (15), the padding (41), the wall (42); then the
    // accept pill's row elsewhere: the blank row above the buttons (4)
    for (column, row) in [(1, 5), (2, 5), (15, 5), (41, 5), (42, 5), (5, 4), (5, 6)] {
        click(&env, &pane, column, row);
    }
    let rows = wait_screen(&env, &pane, "the dialog", |r| has_pill(r, "Archive"));
    assert!(!closed(&env), "{rows:?}");
}

const INPUT: [&str; 3] = ["--prompt=Fork cpd-ui into a new Plan, name:", "--initial=ab", "--validate=plan-id"];

#[test]
fn input_types_a_refused_value_stays_open_and_a_fixed_one_is_written() {
    let env = TestEnv::new();
    let (pane, out) = start(&env, "input", &INPUT, 60, 10);
    let rows = wait_screen(&env, &pane, "the dialog", |r| has_pill(r, "OK"));
    assert_eq!(rows[0], top(60, "Title"));
    assert!(rows[2].contains("Fork cpd-ui into a new Plan, name") && !rows[2].contains("name:"), "{rows:?}");
    assert!(rows[3].starts_with("│ │ ab"), "{rows:?}");
    // "abBad" is no Plan id: refused inside the popup, which stays
    keys(&env, &pane, &["B", "a", "d", "Enter"]);
    let rows = wait_screen(&env, &pane, "the refusal", |r| r.iter().any(|l| l.contains("Invalid Plan id")));
    assert!(rows[3].starts_with("│ │ abBad"), "{rows:?}");
    assert!(rows[5].contains("Invalid Plan id"), "the refusal sits under the box: {rows:?}");
    assert!(!closed(&env));
    assert!(!out.exists());
    // the User keeps typing: y, n and q are text; the refusal goes at the first edit
    keys(&env, &pane, &["BSpace", "BSpace", "BSpace", "y", "n", "q", "-", "1"]);
    let rows = wait_screen(&env, &pane, "the edit", |r| r.iter().any(|l| l.starts_with("│ │ abynq-1")));
    assert!(!rows.iter().any(|l| l.contains("Invalid")), "{rows:?}");
    keys(&env, &pane, &["Enter"]);
    assert_eq!(outcome(&env, &out), json!({"confirmed": true, "value": "abynq-1"}));
}

#[test]
fn input_esc_cancels_and_writes_no_value() {
    let env = TestEnv::new();
    let (pane, out) = start(&env, "input", &INPUT, 60, 10);
    wait_screen(&env, &pane, "the dialog", |r| has_pill(r, "OK"));
    keys(&env, &pane, &["x", "Escape"]);
    assert_eq!(outcome(&env, &out), json!({"confirmed": false}));
}

#[test]
fn input_the_ok_button_accepts_and_validates() {
    let env = TestEnv::new();
    let (pane, out) = start(&env, "input", &["--prompt=Title: ", "--initial=Old title", "--validate=title"], 60, 10);
    wait_screen(&env, &pane, "the dialog", |r| has_pill(r, "OK"));
    // the OK pill is at 0-based columns 2 to 5, row 8
    click(&env, &pane, 4, 9);
    assert_eq!(outcome(&env, &out), json!({"confirmed": true, "value": "Old title"}));
}

#[test]
fn input_an_empty_title_is_refused_by_the_titles_check() {
    let env = TestEnv::new();
    let (pane, _out) = start(&env, "input", &["--prompt=Title: ", "--initial=", "--validate=title"], 60, 10);
    wait_screen(&env, &pane, "the dialog", |r| has_pill(r, "OK"));
    keys(&env, &pane, &["Enter"]);
    wait_screen(&env, &pane, "the refusal", |r| r.iter().any(|l| l.contains("1 to 200")));
    assert!(!closed(&env));
}

#[test]
fn input_box_in_a_popup_shows_the_label_and_grows_with_the_value() {
    let env = TestEnv::new();
    let (pane, _out) = start(&env, "input", &["--prompt=Title: ", "--initial=", "--validate=title"], 44, 15);
    let rows = wait_screen(&env, &pane, "the dialog", |r| has_pill(r, "OK"));
    // the box is 40 wide (the dialog's padding is its margin): `tw` is 36
    assert_eq!(rows[2], format!("│ ╭─{}{}╮ │", pill("Title"), "─".repeat(30)));
    assert!(rows[3].starts_with("│ │ "), "{rows:?}");
    assert!(rows[4].starts_with("│ ╰"), "{rows:?}");
    let x = "x".repeat(45);
    keys(&env, &pane, &["-l", &x]);
    let rows = wait_screen(&env, &pane, "the box grown", |r| r.get(5).is_some_and(|l| l.starts_with("│ ╰")) && r[4].starts_with(&format!("│ │ {}", "x".repeat(9))));
    assert!(rows[3].starts_with(&format!("│ │ {}", "x".repeat(36))), "{rows:?}");
    assert!(rows[4].starts_with(&format!("│ │ {}", "x".repeat(9))), "{rows:?}");
}

fn body() -> String {
    (1..=20).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n")
}

#[test]
fn text_the_wheel_and_the_keys_scroll_it() {
    let env = TestEnv::new();
    let body = format!("--body={}", body());
    let (pane, out) = start(&env, "text", &[&body], 30, 10);
    let rows = wait_screen(&env, &pane, "the dialog", |r| has_pill(r, "Close"));
    assert_eq!(rows[0], top(30, "Title"));
    // the body is rows 2 to 6; the position is on the buttons row, muted, after the pill
    assert_eq!(rows[2], row(30, "line 1"));
    assert_eq!(rows[6], row(30, "line 5"));
    assert!(rows[8].ends_with("1-5/20 │"), "{rows:?}");
    // the wheel: 3 rows a notch
    wheel(&env, &pane, 65, 5, 5);
    let rows = wait_screen(&env, &pane, "the wheel down", |r| r[2] == row(30, "line 4"));
    assert!(rows[8].ends_with("4-8/20 │"), "{rows:?}");
    wheel(&env, &pane, 64, 5, 5);
    wait_screen(&env, &pane, "the wheel up", |r| r[2] == row(30, "line 1"));
    // the keys
    keys(&env, &pane, &["j"]);
    wait_screen(&env, &pane, "j", |r| r[2] == row(30, "line 2"));
    keys(&env, &pane, &["NPage"]);
    wait_screen(&env, &pane, "PgDn", |r| r[2] == row(30, "line 7"));
    keys(&env, &pane, &["Down", "Down"]);
    wait_screen(&env, &pane, "Down", |r| r[2] == row(30, "line 9"));
    keys(&env, &pane, &["k"]);
    keys(&env, &pane, &["PPage"]);
    wait_screen(&env, &pane, "k and PgUp", |r| r[2] == row(30, "line 3"));
    keys(&env, &pane, &["Up"]);
    wait_screen(&env, &pane, "Up", |r| r[2] == row(30, "line 2"));
    // the end of the text stops the scroll
    for _ in 0..8 {
        wheel(&env, &pane, 65, 5, 5);
    }
    let rows = wait_screen(&env, &pane, "the end", |r| r[6] == row(30, "line 20"));
    assert_eq!(rows[2], row(30, "line 16"));
    // Enter closes: confirmed
    keys(&env, &pane, &["Enter"]);
    assert_eq!(outcome(&env, &out), json!({"confirmed": true}));
}

#[test]
fn text_esc_closes_it_and_a_click_on_close_closes_it() {
    let env = TestEnv::new();
    let (pane, out) = start(&env, "text", &["--body=short"], 30, 6);
    wait_screen(&env, &pane, "the dialog", |r| has_pill(r, "Close"));
    keys(&env, &pane, &["Escape"]);
    assert_eq!(outcome(&env, &out), json!({"confirmed": false}));

    let env = TestEnv::new();
    let (pane, out) = start(&env, "text", &["--body=short"], 30, 6);
    let rows = wait_screen(&env, &pane, "the dialog", |r| has_pill(r, "Close"));
    assert_eq!(rows[2], row(30, "short"));
    // the Close pill is at 0-based columns 2 to 8, row 4
    click(&env, &pane, 4, 5);
    assert_eq!(outcome(&env, &out), json!({"confirmed": true}));
}

#[test]
fn the_view_needs_a_terminal_and_its_arguments() {
    let env = TestEnv::new();
    let run = env.cpd(&["tui", "view", "dialog", "confirm", "--title=T", "--out=/tmp/x", "--text=Q"]);
    assert_eq!((run.exit, run.error()["code"].as_str()), (3, Some("not_a_terminal")), "{run:?}");
    for bad in [
        &["tui", "view", "dialog"][..],
        &["tui", "view", "dialog", "confirm", "--title=T", "--text=Q"],
        &["tui", "view", "dialog", "input", "--title=T", "--out=/tmp/x", "--prompt=P", "--validate=anything"],
        &["tui", "view", "dialog", "input", "--title=T", "--out=/tmp/x", "--prompt=P"],
        &["tui", "view", "dialog", "text", "--out=/tmp/x", "--body=B"],
    ] {
        assert_eq!(env.cpd(bad).exit, 2, "{bad:?}");
    }
}
