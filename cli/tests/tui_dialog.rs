//! The `dialog` view (commands.md §13.2, "The dialog kit"; hs-d295) on a private tmux server: each kind draws in the
//! section-header style, takes its keys, clicks and wheel, and writes its outcome to the `--out` file. The popup is a
//! window of the popup's inside (tmux gives a popup's program a pane of that size). The wheel and the click are raw
//! mouse bytes sent to the pane.

mod common;

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
    let out = env.root().join(format!("{kind}-out.json"));
    let (w, h) = (width.to_string(), height.to_string());
    let out_arg = format!("--out={}", out.display());
    let mut args = vec!["new-session", "-d", "-s", "view", "-x", &w, "-y", &h, "-P", "-F", "#{pane_id}", CPD, "tui", "view", "dialog", kind, "--title=Title", &out_arg];
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

const CONFIRM: [&str; 2] = ["--text=Archive cpd-ui?", "--yes-label=Archive"];

#[test]
fn confirm_draws_the_header_the_question_and_the_buttons() {
    let env = TestEnv::new();
    let (pane, _) = start(&env, "confirm", &CONFIRM, 40, 5);
    let rows = wait_screen(&env, &pane, "the dialog", |r| r.iter().any(|l| l.contains("[Archive]")));
    assert_eq!(rows[0], format!("── Title {}", "─".repeat(31)));
    assert_eq!(rows[1..5], [" Archive cpd-ui?", "", "", "  [Archive]   [Cancel]"]);
    // the title is the header's reversed block, as the cockpit's section headers
    let styled = tmux_out(&env, &["capture-pane", "-p", "-e", "-t", &pane]);
    assert!(styled.lines().next().unwrap().contains("\x1b[7m Title "), "{styled:?}");
}

#[test]
fn confirm_y_and_enter_and_a_click_on_the_first_button_confirm() {
    for how in ["y", "Enter", "click"] {
        let env = TestEnv::new();
        let (pane, out) = start(&env, "confirm", &CONFIRM, 40, 5);
        wait_screen(&env, &pane, "the dialog", |r| r.iter().any(|l| l.contains("[Archive]")));
        match how {
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
        let (pane, out) = start(&env, "confirm", &CONFIRM, 40, 5);
        wait_screen(&env, &pane, "the dialog", |r| r.iter().any(|l| l.contains("[Cancel]")));
        match how {
            // `[Cancel]` is at 0-based columns 14 to 21, row 4
            "click" => click(&env, &pane, 16, 5),
            key => keys(&env, &pane, &[key]),
        }
        assert_eq!(outcome(&env, &out), json!({"confirmed": false}), "{how}");
    }
}

const INPUT: [&str; 3] = ["--prompt=Fork cpd-ui into a new Plan, name:", "--initial=ab", "--validate=plan-id"];

#[test]
fn input_types_a_refused_value_stays_open_and_a_fixed_one_is_written() {
    let env = TestEnv::new();
    let (pane, out) = start(&env, "input", &INPUT, 60, 7);
    let rows = wait_screen(&env, &pane, "the dialog", |r| r.iter().any(|l| l.contains("[OK]")));
    assert_eq!(rows[0], format!("── Title {}", "─".repeat(51)));
    assert_eq!(rows[1..3], [" Fork cpd-ui into a new Plan, name:", " ab"]);
    // "abBad" is no Plan id: refused inside the popup, which stays
    keys(&env, &pane, &["B", "a", "d", "Enter"]);
    let rows = wait_screen(&env, &pane, "the refusal", |r| r.iter().any(|l| l.contains("Invalid Plan id")));
    assert_eq!(rows[2], " abBad");
    assert!(!closed(&env));
    assert!(!out.exists());
    // the User keeps typing: y, n and q are text; the refusal goes at the first edit
    keys(&env, &pane, &["BSpace", "BSpace", "BSpace", "y", "n", "q", "-", "1"]);
    let rows = wait_screen(&env, &pane, "the edit", |r| r.iter().any(|l| l == " abynq-1"));
    assert!(!rows.iter().any(|l| l.contains("Invalid")), "{rows:?}");
    keys(&env, &pane, &["Enter"]);
    assert_eq!(outcome(&env, &out), json!({"confirmed": true, "value": "abynq-1"}));
}

#[test]
fn input_esc_cancels_and_writes_no_value() {
    let env = TestEnv::new();
    let (pane, out) = start(&env, "input", &INPUT, 60, 7);
    wait_screen(&env, &pane, "the dialog", |r| r.iter().any(|l| l.contains("[OK]")));
    keys(&env, &pane, &["x", "Escape"]);
    assert_eq!(outcome(&env, &out), json!({"confirmed": false}));
}

#[test]
fn input_the_ok_button_accepts_and_validates() {
    let env = TestEnv::new();
    let (pane, out) = start(&env, "input", &["--prompt=Title: ", "--initial=Old title", "--validate=title"], 60, 7);
    wait_screen(&env, &pane, "the dialog", |r| r.iter().any(|l| l.contains("[OK]")));
    // `[OK]` is at 0-based columns 2 to 5, row 6
    click(&env, &pane, 4, 7);
    assert_eq!(outcome(&env, &out), json!({"confirmed": true, "value": "Old title"}));
}

#[test]
fn input_an_empty_title_is_refused_by_the_titles_check() {
    let env = TestEnv::new();
    let (pane, _out) = start(&env, "input", &["--prompt=Title: ", "--initial=", "--validate=title"], 60, 7);
    wait_screen(&env, &pane, "the dialog", |r| r.iter().any(|l| l.contains("[OK]")));
    keys(&env, &pane, &["Enter"]);
    wait_screen(&env, &pane, "the refusal", |r| r.iter().any(|l| l.contains("1 to 200")));
    assert!(!closed(&env));
}

fn body() -> String {
    (1..=20).map(|n| format!("line {n}")).collect::<Vec<_>>().join("\n")
}

#[test]
fn text_the_wheel_and_the_keys_scroll_it() {
    let env = TestEnv::new();
    let body = format!("--body={}", body());
    let (pane, out) = start(&env, "text", &[&body], 30, 8);
    let rows = wait_screen(&env, &pane, "the dialog", |r| r.iter().any(|l| l.contains("[Close]")));
    assert_eq!(rows[0], format!("── Title {}", "─".repeat(21)));
    assert_eq!(rows[1], " line 1");
    assert_eq!(rows[5], " line 5");
    assert!(rows[7].ends_with("1-5/20"), "{rows:?}");
    // the wheel: 3 rows a notch
    wheel(&env, &pane, 65, 5, 3);
    let rows = wait_screen(&env, &pane, "the wheel down", |r| r[1] == " line 4");
    assert!(rows[7].ends_with("4-8/20"), "{rows:?}");
    wheel(&env, &pane, 64, 5, 3);
    wait_screen(&env, &pane, "the wheel up", |r| r[1] == " line 1");
    // the keys
    keys(&env, &pane, &["j"]);
    wait_screen(&env, &pane, "j", |r| r[1] == " line 2");
    keys(&env, &pane, &["NPage"]);
    wait_screen(&env, &pane, "PgDn", |r| r[1] == " line 7");
    keys(&env, &pane, &["Down", "Down"]);
    wait_screen(&env, &pane, "Down", |r| r[1] == " line 9");
    keys(&env, &pane, &["k"]);
    keys(&env, &pane, &["PPage"]);
    wait_screen(&env, &pane, "k and PgUp", |r| r[1] == " line 3");
    keys(&env, &pane, &["Up"]);
    wait_screen(&env, &pane, "Up", |r| r[1] == " line 2");
    // the end of the text stops the scroll
    for _ in 0..8 {
        wheel(&env, &pane, 65, 5, 3);
    }
    let rows = wait_screen(&env, &pane, "the end", |r| r[5] == " line 20");
    assert_eq!(rows[1], " line 16");
    // Enter closes: confirmed
    keys(&env, &pane, &["Enter"]);
    assert_eq!(outcome(&env, &out), json!({"confirmed": true}));
}

#[test]
fn text_esc_closes_it_and_a_click_on_close_closes_it() {
    let env = TestEnv::new();
    let (pane, out) = start(&env, "text", &["--body=short"], 30, 6);
    wait_screen(&env, &pane, "the dialog", |r| r.iter().any(|l| l.contains("[Close]")));
    keys(&env, &pane, &["Escape"]);
    assert_eq!(outcome(&env, &out), json!({"confirmed": false}));

    let env = TestEnv::new();
    let (pane, out) = start(&env, "text", &["--body=short"], 30, 6);
    let rows = wait_screen(&env, &pane, "the dialog", |r| r.iter().any(|l| l.contains("[Close]")));
    assert_eq!(rows[1], " short");
    click(&env, &pane, 4, 6);
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
