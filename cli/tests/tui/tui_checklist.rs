//! The `checklist` kind of the dialog kit (commands.md §13.2; hs-d549, hs-d551, hs-d568, hs-d569 CD2, CD3) on a private
//! tmux server: entries `[ ] <id> — <detail>` with the record's full text wrapped under them, Space toggles, `a`
//! toggles all, `[Approve] [Later]`, the outcome file.

use crate::common;
use crate::tui_dialog::{bottom, buttons, column_of, pill, row, top};

use std::fs;
use std::path::PathBuf;

use common::{CPD, TestEnv, wait_until};
use serde_json::{Value, json};

fn tmux_out(env: &TestEnv, args: &[&str]) -> String {
    let out = env.tmux(args);
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

const SQLITE: &str = "use sqlite as the store";
const LONG: &str = "w01 w02 w03 w04 w05 w06 w07 w08 w09 w10 w11 w12 w13 w14 w15 w16 w17 w18 w19 w20 w21 w22 w23 w24 w25 w26 w27 w28 w29 w30";

/// `cpd tui view dialog checklist` over `rows` (`{id, text, detail}`), in a pane of 60 columns by `height` rows.
fn start_with(env: &TestEnv, rows: Value, height: u16) -> (String, PathBuf) {
    start_sized(env, "Title", rows, 60, height)
}

/// [`start_with`] titled `title`, in a pane of `width` by `height`: the popup's whole size (it is opened with `-B`).
fn start_sized(env: &TestEnv, title: &str, rows: Value, width: u16, height: u16) -> (String, PathBuf) {
    let items = env.root().join("items.json");
    fs::write(&items, rows.to_string()).unwrap();
    let out = env.root().join("checklist-out.json");
    let (items_arg, out_arg) = (format!("--items={}", items.display()), format!("--out={}", out.display()));
    let (width, height, title_arg) = (width.to_string(), height.to_string(), format!("--title={title}"));
    let args = ["new-session", "-d", "-s", "view", "-x", &width, "-y", &height, "-P", "-F", "#{pane_id}", CPD, "tui", "view", "dialog", "checklist", &title_arg, &items_arg, &out_arg];
    (tmux_out(env, &args).trim().to_owned(), out)
}

/// Three entries, the second with a long text, in a pane of 60 by 22.
fn start(env: &TestEnv) -> (String, PathBuf) {
    let rows = json!([
        {"id": "r-1", "text": SQLITE, "detail": "duplicate r-0"},
        {"id": "r-2", "text": LONG, "detail": "superseded r-9"},
        {"id": "r-3", "text": "wire format", "detail": "documented docs/a.md"},
    ]);
    start_with(env, rows, 22)
}

fn screen(env: &TestEnv, pane: &str) -> Vec<String> {
    tmux_out(env, &["capture-pane", "-p", "-t", pane]).lines().map(|l| l.trim_end().to_owned()).collect()
}

/// The screen once `done`, with each row of the frame reduced to its content (the wall and the padding cut off both
/// sides, the right trimmed): the rows are as the content is drawn at column 0. The border rows stay as they are.
fn wait_screen(env: &TestEnv, pane: &str, what: &str, done: impl FnMut(&[String]) -> bool) -> Vec<String> {
    common::wait_screen(what, || screen(env, pane), done).iter().map(|l| inside(l)).collect()
}

fn inside(line: &str) -> String {
    let cells: Vec<char> = line.chars().collect();
    if cells.first() == Some(&'│') && cells.last() == Some(&'│') && cells.len() >= 4 {
        cells[2..cells.len() - 2].iter().collect::<String>().trim_end().to_owned()
    } else {
        line.to_owned()
    }
}

fn keys(env: &TestEnv, pane: &str, keys: &[&str]) {
    let mut args = vec!["send-keys", "-t", pane];
    args.extend_from_slice(keys);
    tmux_out(env, &args);
}

/// A left button press at 0-based `(column, row)`, as the terminal sends it (SGR mouse reporting).
fn click(env: &TestEnv, pane: &str, column: usize, row: usize) {
    tmux_out(env, &["send-keys", "-t", pane, "-l", &format!("\x1b[<0;{};{}M", column + 1, row + 1)]);
}

fn outcome(env: &TestEnv, out: &PathBuf) -> Value {
    wait_until("the view to close", || !env.tmux(&["has-session", "-t", "view"]).status.success());
    serde_json::from_str(&fs::read_to_string(out).expect("the view wrote its outcome")).unwrap()
}

fn ready(r: &[String]) -> bool {
    r.iter().any(|l| l.contains(&pill("Approve")))
}

/// The row of the line that contains `want`.
fn row_of(rows: &[String], want: &str) -> usize {
    rows.iter().position(|l| l.contains(want)).unwrap_or_else(|| panic!("{want:?} in {rows:?}"))
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

#[test]
fn checklist_draws_each_entry_with_its_detail_and_its_full_text_wrapped() {
    let env = TestEnv::new();
    let (pane, _) = start(&env);
    let rows = wait_screen(&env, &pane, "the dialog", ready);
    let (h1, h2, h3) = (row_of(&rows, "[ ] r-1 — duplicate r-0"), row_of(&rows, "[ ] r-2 — superseded r-9"), row_of(&rows, "[ ] r-3 — documented docs/a.md"));
    assert!(rows.iter().all(|l| !l.contains("use sqlite as the store —")), "the text is not on the header: {rows:?}");
    let margin = indent(&rows[h1]);
    assert_eq!(margin, 0, "the content starts at the padding: {rows:?}");
    // the text of an entry: under its header, four columns in, then one empty row
    assert_eq!(rows[h1 + 1], format!("{}{SQLITE}", " ".repeat(margin + 4)), "{rows:?}");
    assert_eq!(rows[h1 + 2], "", "one empty row between entries: {rows:?}");
    assert_eq!(h2, h1 + 3, "{rows:?}");
    let long = &rows[h2 + 1..h3 - 1];
    assert!(long.len() >= 2, "the long text wraps: {rows:?}");
    assert!(long.iter().all(|l| indent(l) == margin + 4 && l.chars().count() <= 56), "indented by four, within the frame: {long:?}");
    let joined = long.iter().map(|l| l.trim()).collect::<Vec<_>>().join(" ");
    assert_eq!(joined, LONG, "the full text, nothing cut");
    assert_eq!(rows[h3 - 1], "", "{rows:?}");
    assert_eq!(rows[h3 + 1].trim(), "wire format", "{rows:?}");
    assert!(rows.iter().any(|l| l.contains(&pill("Approve")) && l.contains(&pill("Later"))), "{rows:?}");
}

#[test]
fn the_checklist_is_drawn_whole_as_a_rounded_box_with_approve_left_and_later_right() {
    let env = TestEnv::new();
    let rows = json!([
        {"id": "r-1", "text": SQLITE, "detail": "duplicate r-0"},
        {"id": "r-2", "text": "wire format", "detail": "documented docs/a.md"},
    ]);
    let (pane, out) = start_sized(&env, "Retire records of cpd", rows, 42, 10);
    let raw = common::wait_screen("the dialog", || screen(&env, &pane), |r| r.iter().any(|l| l.contains(&pill("Later"))));
    let want = [
        top(42, "Retire records of cpd"),
        row(42, ""),
        row(42, "[ ] r-1 — duplicate r-0"),
        row(42, "    use sqlite as the store"),
        row(42, ""),
        row(42, "[ ] r-2 — documented docs/a.md"),
        row(42, "    wire format"),
        row(42, ""),
        buttons(42, "Approve", Some("Later")),
        bottom(42),
    ];
    assert_eq!(raw, want);
    // Later is the pill flush right, at 0-based columns 33 to 39: a click on column 36 of row 8 closes it
    click(&env, &pane, 36, 8);
    assert_eq!(outcome(&env, &out)["approved"], json!(false));
}

#[test]
fn space_toggles_and_approve_writes_ticked_and_unticked() {
    let env = TestEnv::new();
    let (pane, out) = start(&env);
    wait_screen(&env, &pane, "the dialog", ready);
    // tick the first entry, move down two entries (not two rows) and tick the third
    keys(&env, &pane, &["Space", "Down", "Down", "Space"]);
    let rows = wait_screen(&env, &pane, "the ticks", |r| r.iter().any(|l| l.contains("[x] r-3 — documented")));
    assert!(rows.iter().any(|l| l.contains("[x] r-1 — duplicate")), "{rows:?}");
    assert!(rows.iter().any(|l| l.contains("[ ] r-2 — superseded")), "{rows:?}");
    keys(&env, &pane, &["Enter"]);
    assert_eq!(outcome(&env, &out), json!({"approved": true, "ticked": ["r-1", "r-3"], "unticked": ["r-2"]}));
}

#[test]
fn later_writes_not_approved_on_esc_q_and_a_click_and_there_is_no_cancel_button() {
    for how in ["Escape", "q", "click"] {
        let env = TestEnv::new();
        let (pane, out) = start(&env);
        let rows = wait_screen(&env, &pane, "the dialog", ready);
        assert!(rows.iter().all(|l| !l.contains(&pill("Cancel"))), "no [Cancel] on a checklist: {rows:?}");
        let buttons = row_of(&rows, &pill("Later"));
        let raw = screen(&env, &pane);
        keys(&env, &pane, &["a"]);
        match how {
            "click" => click(&env, &pane, column_of(&raw[buttons], "Later") + 2, buttons),
            key => keys(&env, &pane, &[key]),
        }
        assert_eq!(outcome(&env, &out)["approved"], json!(false), "{how}");
    }
}

#[test]
fn a_click_on_the_text_of_an_entry_toggles_it() {
    let env = TestEnv::new();
    let (pane, out) = start(&env);
    let rows = wait_screen(&env, &pane, "the dialog", ready);
    let (h2, h3) = (row_of(&rows, "r-2 — "), row_of(&rows, "r-3 — "));
    // a click on the second line of the second entry's text ticks that entry and moves the cursor there
    click(&env, &pane, 10, h2 + 2);
    wait_screen(&env, &pane, "the tick", |r| r.iter().any(|l| l.contains("[x] r-2 —")));
    click(&env, &pane, 10, h3 + 1);
    let rows = wait_screen(&env, &pane, "the tick", |r| r.iter().any(|l| l.contains("[x] r-3 —")));
    assert!(rows.iter().any(|l| l.contains("[x] r-2 —")), "{rows:?}");
    // the cursor is on the third entry: Space unticks it
    keys(&env, &pane, &["Space"]);
    wait_screen(&env, &pane, "the untick", |r| r.iter().any(|l| l.contains("[ ] r-3 —")));
    keys(&env, &pane, &["Enter"]);
    assert_eq!(outcome(&env, &out), json!({"approved": true, "ticked": ["r-2"], "unticked": ["r-1", "r-3"]}));
}

#[test]
fn the_cursor_moves_by_entry_and_its_header_stays_in_view_in_a_short_pane() {
    let env = TestEnv::new();
    let rows: Vec<Value> = (1..=8).map(|n| json!({"id": format!("r-{n}"), "text": format!("text {n}"), "detail": "d"})).collect();
    let (pane, out) = start_with(&env, Value::Array(rows), 10);
    wait_screen(&env, &pane, "the dialog", ready);
    keys(&env, &pane, &["j", "j", "j", "j", "j", "j"]);
    let rows = wait_screen(&env, &pane, "the cursor entry in view", |r| r.iter().any(|l| l.contains("[ ] r-7 — d")));
    assert!(rows.iter().all(|l| !l.contains("r-1 — d")), "the first entry scrolled out: {rows:?}");
    keys(&env, &pane, &["Space", "k", "k", "k", "k", "k", "k"]);
    wait_screen(&env, &pane, "the first entry back in view", |r| r.iter().any(|l| l.contains("[ ] r-1 — d")));
    keys(&env, &pane, &["Enter"]);
    assert_eq!(
        outcome(&env, &out),
        json!({"approved": true, "ticked": ["r-7"], "unticked": ["r-1", "r-2", "r-3", "r-4", "r-5", "r-6", "r-8"]})
    );
}

#[test]
fn an_items_file_with_a_short_field_is_refused() {
    let env = TestEnv::new();
    let items = env.root().join("items.json");
    let stdout = env.root().join("stdout.json");
    let run = |rows: Value, redirect: bool| {
        fs::write(&items, rows.to_string()).unwrap();
        let to = if redirect { format!(" > {}", stdout.display()) } else { String::new() };
        let shell = format!("{CPD} tui view dialog checklist --title=Title --items={} --out={}/o.json{to}", items.display(), env.root().display());
        tmux_out(&env, &["new-session", "-d", "-s", "view", "-x", "60", "-y", "12", "-P", "-F", "#{pane_id}", "sh", "-c", &shell]).trim().to_owned()
    };
    run(json!([{"id": "r-1", "short": "use sqlite", "text": SQLITE, "detail": "duplicate r-0"}]), true);
    wait_until("the view to close", || !env.tmux(&["has-session", "-t", "view"]).status.success());
    let reply: Value = serde_json::from_str(&fs::read_to_string(&stdout).unwrap()).unwrap();
    assert_eq!(reply["ok"], json!(false), "{reply}");
    assert_eq!(reply["error"]["code"], json!("args_invalid"), "{reply}");
    assert!(reply["error"]["message"].as_str().unwrap().contains("short"), "the refusal names the field: {reply}");
    // the same entry without `short` is the shape: drawn, its text under the header
    let pane = run(json!([{"id": "r-1", "text": SQLITE, "detail": "duplicate r-0"}]), false);
    let rows = wait_screen(&env, &pane, "the dialog", ready);
    let h = row_of(&rows, "[ ] r-1 — duplicate r-0");
    assert_eq!(rows[h + 1].trim(), SQLITE, "{rows:?}");
}
