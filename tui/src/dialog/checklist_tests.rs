//! The `checklist` kind of the dialog kit (commands.md §13.2; hs-d549, hs-d551, hs-d568, hs-d569 CD2, CD3) in its pane:
//! entries `[ ] <id> — <detail>` with the record's full text wrapped under them, Space toggles, `a` toggles all,
//! `[Approve] [Later]`, the outcome file.

use serde_json::{Value, json};

use super::pane::{Pane, bottom, buttons, column_of, has_pill, pill, row, top};

const SQLITE: &str = "use sqlite as the store";
const LONG: &str = "w01 w02 w03 w04 w05 w06 w07 w08 w09 w10 w11 w12 w13 w14 w15 w16 w17 w18 w19 w20 w21 w22 w23 w24 w25 w26 w27 w28 w29 w30";

/// A `checklist` over `rows` (`{id, text, detail}`), in a pane of 60 columns by `height` rows.
fn start_with(rows: &Value, height: u16) -> Pane {
    Pane::checklist("Title", rows, 60, height)
}

/// Three entries, the second with a long text, in a pane of 60 by 22.
fn start() -> Pane {
    let rows = json!([
        {"id": "r-1", "text": SQLITE, "detail": "duplicate r-0"},
        {"id": "r-2", "text": LONG, "detail": "superseded r-9"},
        {"id": "r-3", "text": "wire format", "detail": "documented docs/a.md"},
    ]);
    start_with(&rows, 22)
}

/// The screen, with each row of the frame reduced to its content (the wall and the padding cut off both sides, the
/// right trimmed): the rows are as the content is drawn at column 0. The border rows stay as they are.
fn screen(pane: &mut Pane) -> Vec<String> {
    pane.screen().iter().map(|l| inside(l)).collect()
}

fn inside(line: &str) -> String {
    let cells: Vec<char> = line.chars().collect();
    if cells.first() == Some(&'│') && cells.last() == Some(&'│') && cells.len() >= 4 {
        cells[2..cells.len() - 2]
            .iter()
            .collect::<String>()
            .trim_end()
            .to_owned()
    } else {
        line.to_owned()
    }
}

fn ready(r: &[String]) -> bool {
    has_pill(r, "Approve")
}

/// The row of the line that contains `want`.
fn row_of(rows: &[String], want: &str) -> usize {
    rows.iter()
        .position(|l| l.contains(want))
        .unwrap_or_else(|| panic!("{want:?} in {rows:?}"))
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

#[test]
fn checklist_draws_each_entry_with_its_detail_and_its_full_text_wrapped() {
    let mut pane = start();
    let rows = screen(&mut pane);
    assert!(ready(&rows));
    let (h1, h2, h3) = (
        row_of(&rows, "[ ] r-1 — duplicate r-0"),
        row_of(&rows, "[ ] r-2 — superseded r-9"),
        row_of(&rows, "[ ] r-3 — documented docs/a.md"),
    );
    assert!(
        rows.iter()
            .all(|l| !l.contains("use sqlite as the store —")),
        "the text is not on the header: {rows:?}"
    );
    let margin = indent(&rows[h1]);
    assert_eq!(margin, 0, "the content starts at the padding: {rows:?}");
    // the text of an entry: under its header, four columns in, then one empty row
    assert_eq!(
        rows[h1 + 1],
        format!("{}{SQLITE}", " ".repeat(margin + 4)),
        "{rows:?}"
    );
    assert_eq!(rows[h1 + 2], "", "one empty row between entries: {rows:?}");
    assert_eq!(h2, h1 + 3, "{rows:?}");
    let long = &rows[h2 + 1..h3 - 1];
    assert!(long.len() >= 2, "the long text wraps: {rows:?}");
    assert!(
        long.iter()
            .all(|l| indent(l) == margin + 4 && l.chars().count() <= 56),
        "indented by four, within the frame: {long:?}"
    );
    let joined = long.iter().map(|l| l.trim()).collect::<Vec<_>>().join(" ");
    assert_eq!(joined, LONG, "the full text, nothing cut");
    assert_eq!(rows[h3 - 1], "", "{rows:?}");
    assert_eq!(rows[h3 + 1].trim(), "wire format", "{rows:?}");
    assert!(
        rows.iter()
            .any(|l| l.contains(&pill("Approve")) && l.contains(&pill("Later"))),
        "{rows:?}"
    );
}

#[test]
fn the_checklist_is_drawn_whole_as_a_rounded_box_with_approve_left_and_later_right() {
    let rows = json!([
        {"id": "r-1", "text": SQLITE, "detail": "duplicate r-0"},
        {"id": "r-2", "text": "wire format", "detail": "documented docs/a.md"},
    ]);
    let mut pane = Pane::checklist("Retire records of cpd", &rows, 42, 10);
    let raw = pane.screen();
    assert!(raw.iter().any(|l| l.contains(&pill("Later"))));
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
    pane.click(36, 8);
    assert_eq!(pane.outcome()["approved"], json!(false));
}

#[test]
fn space_toggles_and_approve_writes_ticked_and_unticked() {
    let mut pane = start();
    assert!(ready(&screen(&mut pane)));
    // tick the first entry, move down two entries (not two rows) and tick the third
    pane.keys(&["Space", "Down", "Down", "Space"]);
    let rows = screen(&mut pane);
    assert!(
        rows.iter().any(|l| l.contains("[x] r-3 — documented")),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|l| l.contains("[x] r-1 — duplicate")),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|l| l.contains("[ ] r-2 — superseded")),
        "{rows:?}"
    );
    pane.keys(&["Enter"]);
    assert_eq!(
        pane.outcome(),
        json!({"approved": true, "ticked": ["r-1", "r-3"], "unticked": ["r-2"]})
    );
}

#[test]
fn later_writes_not_approved_on_esc_q_and_a_click_and_there_is_no_cancel_button() {
    for how in ["Escape", "q", "click"] {
        let mut pane = start();
        let rows = screen(&mut pane);
        assert!(ready(&rows));
        assert!(
            rows.iter().all(|l| !l.contains(&pill("Cancel"))),
            "no [Cancel] on a checklist: {rows:?}"
        );
        let buttons = row_of(&rows, &pill("Later"));
        let raw = pane.screen();
        pane.keys(&["a"]);
        match how {
            "click" => pane.click(
                u16::try_from(column_of(&raw[buttons], "Later") + 2).unwrap(),
                u16::try_from(buttons).unwrap(),
            ),
            key => pane.keys(&[key]),
        }
        assert_eq!(pane.outcome()["approved"], json!(false), "{how}");
    }
}

#[test]
fn a_click_on_the_text_of_an_entry_toggles_it() {
    let mut pane = start();
    let rows = screen(&mut pane);
    assert!(ready(&rows));
    let (h2, h3) = (row_of(&rows, "r-2 — "), row_of(&rows, "r-3 — "));
    // a click on the second line of the second entry's text ticks that entry and moves the cursor there
    pane.click(10, u16::try_from(h2 + 2).unwrap());
    assert!(
        screen(&mut pane).iter().any(|l| l.contains("[x] r-2 —")),
        "the tick"
    );
    pane.click(10, u16::try_from(h3 + 1).unwrap());
    let rows = screen(&mut pane);
    assert!(
        rows.iter().any(|l| l.contains("[x] r-3 —")),
        "the tick: {rows:?}"
    );
    assert!(rows.iter().any(|l| l.contains("[x] r-2 —")), "{rows:?}");
    // the cursor is on the third entry: Space unticks it
    pane.keys(&["Space"]);
    assert!(
        screen(&mut pane).iter().any(|l| l.contains("[ ] r-3 —")),
        "the untick"
    );
    pane.keys(&["Enter"]);
    assert_eq!(
        pane.outcome(),
        json!({"approved": true, "ticked": ["r-2"], "unticked": ["r-1", "r-3"]})
    );
}

#[test]
fn the_cursor_moves_by_entry_and_its_header_stays_in_view_in_a_short_pane() {
    let rows: Vec<Value> = (1..=8)
        .map(|n| json!({"id": format!("r-{n}"), "text": format!("text {n}"), "detail": "d"}))
        .collect();
    let mut pane = start_with(&Value::Array(rows), 10);
    assert!(ready(&screen(&mut pane)));
    pane.keys(&["j", "j", "j", "j", "j", "j"]);
    let rows = screen(&mut pane);
    assert!(
        rows.iter().any(|l| l.contains("[ ] r-7 — d")),
        "the cursor entry in view: {rows:?}"
    );
    assert!(
        rows.iter().all(|l| !l.contains("r-1 — d")),
        "the first entry scrolled out: {rows:?}"
    );
    pane.keys(&["Space", "k", "k", "k", "k", "k", "k"]);
    let rows = screen(&mut pane);
    assert!(
        rows.iter().any(|l| l.contains("[ ] r-1 — d")),
        "the first entry back in view: {rows:?}"
    );
    pane.keys(&["Enter"]);
    assert_eq!(
        pane.outcome(),
        json!({"approved": true, "ticked": ["r-7"], "unticked": ["r-1", "r-2", "r-3", "r-4", "r-5", "r-6", "r-8"]})
    );
}
