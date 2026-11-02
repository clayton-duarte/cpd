//! The docked `Standing` section of `cpd tui view tree` (commands.md §13.2, "The Standing section"; pro-d339): the
//! Permanent Contexts `cpd`, `host` and `codereview` that exist in the store, in that fixed order, under the scrolling project
//! tree: one blank row, the header (the pill `Standing` on the left, the fold glyph as a pill on the right; pro-d347), one
//! reserved row, the rows, one reserved row. The view runs in the test process on a `Tree` (`common/tree.rs`) over a prepared
//! store, on a `TestBackend`, with the fake `cpd` and the fake tmux. The panes are 30 columns wide; every test pins whole
//! screens, built by [`pane`] from the rows the docs give. Between two Context blocks there is one blank spacer row, in
//! the section as in the project tree (the Lead's answer to `dock-tests-a1-q1`; the docs' worked example omits them).

use crate::common::fx::Fx;
use crate::common::sql;
use crate::common::tree::{self as drive, Tree, engine, fx};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use ratatui::crossterm::event::{Event, KeyModifiers, MouseEvent, MouseEventKind};

/// The pane's width.
const W: usize = 30;
/// A Context's and a Plan's dot when nothing needs the User (md-circle_small as the tree draws it).
const DOT: &str = "\u{E3D5}";
/// The closing row `╰ new` of a Context with no Plan.
const NEW: &str = "\u{F0B2A}";
/// The section's glyph open and folded: the opposite of the `collapse`/`expand` rows' (pro-d347: the section is docked
/// at the bottom, so it folds down and opens up): md-arrow_down_drop_circle_outline open, ..._up_... folded.
const OPEN: &str = "\u{F0B26}";
const FOLDED: &str = "\u{F0B2C}";
/// A Context's closing rows keep their own rule: `collapse` (expanded) is U+F0B2C, `expand` (folded) is U+F0B26.
const COLLAPSE: &str = "\u{F0B2C}";
const EXPAND: &str = "\u{F0B26}";
const CAP_LEFT: &str = "\u{E0B6}";
const CAP_RIGHT: &str = "\u{E0B4}";

// ---------- the prepared store ----------

/// Contexts in the order given (created one second apart, so the project tree, newest first, reverses them), each with
/// its Plans in the order given (the oldest first: the tree lists the newest first), each Plan with an idle Lead
/// `l-<plan>`.
fn populate(fx: &Fx, contexts: &[(&str, &[&str])]) {
    let mut store = Store::open(
        &CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap(),
        5000,
    )
    .unwrap();
    let mut s = String::new();
    let mut n = 0;
    for (id, plans) in contexts {
        n += 1;
        s += &format!(
            "INSERT INTO contexts (id, title, jira, created_at, created_on)
             VALUES ('{id}', 'Title of {id}', NULL, '2026-01-01T00:00:{n:02}.000Z', 'm1');"
        );
        for plan in *plans {
            n += 1;
            s += &format!(
                "INSERT INTO plans (id, context, title, phase, lead_session, created_at)
                 VALUES ('{plan}', '{id}', 'Plan {plan}', 'grounding', 'l-{plan}', '2026-01-02T00:00:{n:02}.000Z');
                 INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
                 VALUES ('l-{plan}', 'lead', '{id}', '{plan}', NULL, 'lead of {plan}', '/sessions/l-{plan}.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                         '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:{n:02}.000Z');"
            );
        }
    }
    sql(&mut store, &s);
}

// ---------- the rows ----------

fn title(id: &str) -> String {
    format!(" ╭{DOT} {id}")
}

fn new() -> String {
    format!(" ╰{NEW} new")
}

/// A Plan's row; the last of its Context's rows closes with `╰─`.
fn plan(id: &str, last: bool) -> String {
    format!(" {}─{DOT} {id}", if last { "╰" } else { "├" })
}

/// A Context's block: its title, its Plans (given newest first) and, with none, the row `╰ new`.
fn block(id: &str, plans: &[&str]) -> Vec<String> {
    let mut rows = vec![title(id)];
    if plans.is_empty() {
        rows.push(new());
    }
    for (i, p) in plans.iter().enumerate() {
        rows.push(plan(p, i + 1 == plans.len()));
    }
    rows
}

/// The Context blocks with one blank spacer row between two of them.
fn blocks(list: &[Vec<String>]) -> Vec<String> {
    let mut rows = Vec::new();
    for (i, b) in list.iter().enumerate() {
        if i > 0 {
            rows.push(String::new());
        }
        rows.extend(b.iter().cloned());
    }
    rows
}

/// A cue row: `▼ <n> more` / `▲ <n> more` ends at column `W - 2`.
fn cue(arrow: &str, n: usize) -> String {
    format!("{:>1$}", format!("{arrow} {n} more"), W - 1)
}

/// The header row of a section: `───`, the pill of `title` (`Standing` or `Archive`), `─`s, the pill of the fold glyph (both
/// with their caps) and `─` at the last column.
fn header(title: &str, glyph: &str) -> String {
    let left = format!("{CAP_LEFT}{title}{CAP_RIGHT}");
    let right = format!("{CAP_LEFT}{glyph}{CAP_RIGHT}");
    let fill = W - 3 - title.len() - 2 - 1 - 2 - 1;
    format!("───{left}{}{right}─", "─".repeat(fill))
}

/// An open section's rows with its reserved blank row above and below (blank when nothing is hidden).
fn fit(rows: &[String]) -> Vec<String> {
    let mut all = vec![String::new()];
    all.extend(rows.iter().cloned());
    all.push(String::new());
    all
}

/// The rows of the docked group (R2 of the docs), from its blank row to its last row, built from the sections given as
/// `(open, rows)`: `rows` of an open section are its own `▲` cue row, its list and its `▼` cue row (see [`fit`]); a folded
/// section is its header and ONE blank row below it (pro-d351). Between Standing and Archive there is exactly ONE row:
/// Standing's reserved end row when Standing is open, Standing's own blank row below its header when it is folded. No section, no group: no rows at all.
fn group(standing: Option<(bool, &[String])>, archive: Option<(bool, &[String])>) -> Vec<String> {
    if standing.is_none() && archive.is_none() {
        return Vec::new();
    }
    let mut rows = vec![String::new()];
    for (title, section) in [("Standing", standing), ("Archive", archive)] {
        let Some((open, list)) = section else {
            continue;
        };
        rows.push(header(title, if open { OPEN } else { FOLDED }));
        if open {
            rows.extend(list.iter().cloned());
        } else {
            rows.push(String::new());
        }
    }
    rows
}

/// The screen of an `h`-row pane, as `Tree::screen` reads it (rows without trailing blanks, no empty rows at the end): the
/// project tree's `above` cue row, its `list` rows, its `below` cue row, then the docked `group` at the bottom of the pane.
/// The project tree takes the rows the group leaves.
fn docked(h: usize, above: &str, list: &[String], below: &str, group: &[String]) -> Vec<String> {
    let region = h - group.len();
    let mut screen = vec![above.to_owned()];
    screen.extend(list.iter().cloned());
    screen.resize(region - 1, String::new());
    screen.push(below.to_owned());
    screen.extend(group.iter().cloned());
    assert_eq!(screen.len(), h, "the expected pane has {h} rows");
    while screen.last().is_some_and(String::is_empty) {
        screen.pop();
    }
    screen
}

/// [`docked`] with the Standing section alone, as the first tests of this file pin it: with `standing` given, the blank
/// row, the header and, open, the section's `rows`.
fn pane(
    h: usize,
    above: &str,
    list: &[String],
    below: &str,
    standing: Option<(bool, &[String])>,
) -> Vec<String> {
    docked(h, above, list, below, &group(standing, None))
}

fn wheel(tree: &mut Tree, down: bool, row: u16) {
    tree.event(Event::Mouse(MouseEvent {
        kind: if down {
            MouseEventKind::ScrollDown
        } else {
            MouseEventKind::ScrollUp
        },
        column: 5,
        row,
        modifiers: KeyModifiers::NONE,
    }));
}

fn row_of(screen: &[String], text: &str) -> u16 {
    screen
        .iter()
        .position(|r| r.contains(text))
        .unwrap_or_else(|| panic!("no row with {text}: {screen:?}")) as u16
}

// ---------- the Archive section ----------

/// Archives the Contexts `ids` as `context archive` leaves them: the Context and its Plans carry `archived_at`.
fn archive(fx: &Fx, ids: &[&str]) {
    let mut s = String::new();
    for id in ids {
        s += &format!(
            "UPDATE contexts SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = '{id}';
             UPDATE plans SET archived_at = '2026-02-01T00:00:00.000Z' WHERE context = '{id}';"
        );
    }
    sql(
        &mut Store::open(
            &CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap(),
            5000,
        )
        .unwrap(),
        &s,
    );
}

/// The view's folds file before it starts: the archived Contexts `ids` have their own closing toggle open (an archived
/// Context starts folded); the Archive section itself is open when `archive_open` (the file's `"archived":true`), else folded
/// (its default: no `archived` key).
fn expand(fx: &Fx, ids: &[&str], archive_open: bool) {
    let dir = fx.env.cpd_home().join("run").join("tui");
    std::fs::create_dir_all(&dir).unwrap();
    let open: Vec<String> = ids.iter().map(|id| format!("[\"{id}\"]")).collect();
    let archived = if archive_open {
        ",\"archived\":true"
    } else {
        ""
    };
    std::fs::write(
        dir.join("tree-folds.json"),
        format!(
            "{{\"collapsed\":[],\"expanded\":[{}]{archived}}}",
            open.join(",")
        ),
    )
    .unwrap();
}

/// An archived Context's block as the section draws it (the project tree's own rows, muted): its title, then, with its
/// own closing toggle open, its archived Plans (newest first) and `collapse`; folded, `expand` alone.
fn archived_block(id: &str, plans: &[&str], open: bool) -> Vec<String> {
    let mut rows = vec![title(id)];
    if open {
        rows.extend(plans.iter().map(|p| plan(p, false)));
        rows.push(format!(" ╰{COLLAPSE} collapse"));
    } else {
        rows.push(format!(" ╰{EXPAND} expand"));
    }
    rows
}

/// `▲ n more` above a scrolled list, or the blank reserved row when nothing is hidden above.
fn above_cue(offset: usize) -> String {
    if offset > 0 {
        cue("▲", offset)
    } else {
        String::new()
    }
}

/// `▼ n more` under a list showing `n` of `total` rows from `offset`, or the blank reserved row when nothing is hidden.
fn below_cue(total: usize, offset: usize, n: usize) -> String {
    if total > offset + n {
        cue("▼", total - offset - n)
    } else {
        String::new()
    }
}

// ---------- the tests ----------

#[test]
fn the_standing_section_is_docked_at_the_bottom_with_its_pill_and_the_permanent_contexts_in_fixed_order()
 {
    let fx = fx();
    let _engine = engine(&fx);
    // the Permanent Contexts are the oldest: sorted by age they would come last, newest first
    populate(
        &fx,
        &[
            ("cpd", &["pc"]),
            ("host", &[]),
            ("codereview", &[]),
            ("aa", &[]),
        ],
    );
    let tree = Tree::start(&fx, W as u16, 24);
    let standing = blocks(&[
        block("cpd", &["pc"]),
        block("host", &[]),
        block("codereview", &[]),
    ]);
    assert_eq!(
        tree.screen(),
        pane(24, "", &block("aa", &[]), "", Some((true, &fit(&standing))))
    );
}

#[test]
fn only_the_permanent_contexts_that_exist_are_drawn_and_none_draws_no_section() {
    // one: the header and its rows
    let one = fx();
    let _engine = engine(&one);
    populate(&one, &[("aa", &[]), ("host", &[])]);
    let tree = Tree::start(&one, W as u16, 12);
    assert_eq!(
        tree.screen(),
        pane(
            12,
            "",
            &block("aa", &[]),
            "",
            Some((true, &fit(&block("host", &[]))))
        ),
        "one Permanent Context: its row under the header"
    );
    drop(tree);
    drop(_engine);
    // none: the pane is the project tree alone, with no blank row and no header
    drop(one);
    let none = fx();
    let _engine = engine(&none);
    populate(&none, &[("aa", &[]), ("bb", &[])]);
    let tree = Tree::start(&none, W as u16, 12);
    assert_eq!(
        tree.screen(),
        pane(
            12,
            "",
            &blocks(&[block("bb", &[]), block("aa", &[])]),
            "",
            None
        ),
        "no Permanent Context: no section"
    );
}

#[test]
fn project_contexts_scroll_above_the_section_and_it_never_moves() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(
        &fx,
        &[
            ("cpd", &[]),
            ("host", &[]),
            ("p1", &[]),
            ("p2", &[]),
            ("p3", &[]),
            ("p4", &[]),
        ],
    );
    let mut tree = Tree::start(&fx, W as u16, 18);
    let standing = blocks(&[block("cpd", &[]), block("host", &[])]);
    // the project tree: p4, p3, p2, p1 (11 rows with the spacers); 7 list rows between its two cue rows
    let all = blocks(&[
        block("p4", &[]),
        block("p3", &[]),
        block("p2", &[]),
        block("p1", &[]),
    ]);
    let shown = |from: usize, above: String, below: String| {
        pane(
            18,
            &above,
            &all[from..from + 7],
            &below,
            Some((true, &fit(&standing))),
        )
    };
    assert_eq!(tree.screen(), shown(0, String::new(), cue("▼", 4)));
    wheel(&mut tree, true, 3); // over the project tree: 3 rows a notch
    assert_eq!(tree.screen(), shown(3, cue("▲", 3), cue("▼", 1)));
    wheel(&mut tree, true, 3);
    wheel(&mut tree, true, 3); // clamped at the end
    assert_eq!(tree.screen(), shown(4, cue("▲", 4), String::new()));
    wheel(&mut tree, true, 12); // over the section's rows, which have nothing to scroll
    assert_eq!(
        tree.screen(),
        shown(4, cue("▲", 4), String::new()),
        "the wheel over the section moves nothing"
    );
    for _ in 0..4 {
        wheel(&mut tree, false, 3);
    }
    assert_eq!(tree.screen(), shown(0, String::new(), cue("▼", 4)));
}

#[test]
fn a_click_on_the_header_folds_and_opens_the_section_with_the_fold_glyph() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx, &[("cpd", &[]), ("host", &[]), ("aa", &[])]);
    let mut tree = Tree::start(&fx, W as u16, 18);
    let standing = blocks(&[block("cpd", &[]), block("host", &[])]);
    let open = pane(18, "", &block("aa", &[]), "", Some((true, &fit(&standing))));
    assert_eq!(tree.screen(), open);
    tree.click(5, 10); // the header: tree rows 0 to 8, the blank row 9
    assert_eq!(
        tree.screen(),
        pane(18, "", &block("aa", &[]), "", Some((false, &[]))),
        "folded: the blank row, the header and one blank row below it remain, at the pane's bottom"
    );
    tree.click(5, 16); // the folded header: the blank row above it is 15, the blank row below it is the last row, 17
    assert_eq!(tree.screen(), open, "a second click opens it");
    assert!(
        drive::calls(&fx).is_empty(),
        "folding runs no command: {:?}",
        drive::calls(&fx)
    );
}

#[test]
fn a_permanent_context_expands_to_its_plans_like_any_context() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx, &[("cpd", &["a0", "a1", "a2"]), ("aa", &[])]);
    sql(
        &mut Store::open(
            &CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap(),
            5000,
        )
        .unwrap(),
        "UPDATE plans SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'a0';",
    );
    let mut tree = Tree::start(&fx, W as u16, 18);
    // expanded by default, as every Context: the live Plans, the archived one, then `collapse`
    let expanded = vec![
        title("cpd"),
        plan("a2", false),
        plan("a1", false),
        plan("a0", false),
        format!(" ╰{COLLAPSE} collapse"),
    ];
    assert_eq!(
        tree.screen(),
        pane(18, "", &block("aa", &[]), "", Some((true, &fit(&expanded))))
    );
    tree.click(5, 16); // the closing row `collapse`, in the section, above its reserved last row
    let folded = vec![
        title("cpd"),
        plan("a2", false),
        plan("a1", false),
        format!(" ╰{EXPAND} expand"),
    ];
    assert_eq!(
        tree.screen(),
        pane(18, "", &block("aa", &[]), "", Some((true, &fit(&folded))))
    );
    tree.click(5, 16); // `expand`: the section grew up, so it is on the same row
    assert_eq!(
        tree.screen(),
        pane(18, "", &block("aa", &[]), "", Some((true, &fit(&expanded))))
    );
}

#[test]
fn the_section_is_capped_at_half_the_pane_and_scrolls_on_its_own() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(
        &fx,
        &[
            ("cpd", &["c1", "c2", "c3", "c4"]),
            ("host", &["h1", "h2", "h3", "h4"]),
            ("codereview", &["r1", "r2", "r3", "r4"]),
            ("aa", &[]),
        ],
    );
    let mut tree = Tree::start(&fx, W as u16, 12);
    // 17 rows (three blocks of 5 and two spacers) in the 12-row pane: the section is 6 rows (half), the blank row, the
    // header and 4 rows (2 + 4): a cue row, 2 list rows, a cue row
    let all = blocks(&[
        block("cpd", &["c4", "c3", "c2", "c1"]),
        block("host", &["h4", "h3", "h2", "h1"]),
        block("codereview", &["r4", "r3", "r2", "r1"]),
    ]);
    assert_eq!(all.len(), 17);
    let tree_rows = block("aa", &[]);
    let section = |from: usize, above: String, below: String| {
        let mut rows = vec![above];
        rows.extend(all[from..from + 2].iter().cloned());
        rows.push(below);
        rows
    };
    let start = section(0, String::new(), cue("▼", 15));
    let first = pane(12, "", &tree_rows, "", Some((true, &start)));
    assert_eq!(tree.screen(), first);
    wheel(&mut tree, true, 5); // over the project tree: nothing to scroll there, the section does not move
    assert_eq!(tree.screen(), first, "the project tree's wheel is its own");
    wheel(&mut tree, true, 10); // over the section's rows
    let moved = tree.screen();
    assert_ne!(moved, first, "the section's wheel scrolls its rows");
    assert_eq!(
        moved[..6],
        first[..6],
        "the project tree above does not scroll with the section"
    );
    for _ in 0..10 {
        wheel(&mut tree, true, 10);
    }
    let end = section(15, cue("▲", 15), String::new());
    assert_eq!(
        tree.screen(),
        pane(12, "", &tree_rows, "", Some((true, &end))),
        "scrolled to the end: the last two rows"
    );
    for _ in 0..10 {
        wheel(&mut tree, false, 10);
    }
    assert_eq!(tree.screen(), first);
}

#[test]
fn the_open_section_reserves_a_blank_row_under_the_header_and_above_the_bottom() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(
        &fx,
        &[
            ("cpd", &[]),
            ("p1", &[]),
            ("p2", &[]),
            ("p3", &[]),
            ("p4", &[]),
        ],
    );
    let tree = Tree::start(&fx, W as u16, 12);
    // section: blank row, header, reserved row, 2 rows, reserved row = 6 rows, so the project tree has 6 rows: its cue row,
    // 4 list rows and `▼ 7 more` (11 rows, 4 shown). The last reserved row is blank and dropped by the screen.
    let expected = [
        "",
        " ╭\u{E3D5} p4",
        " ╰\u{F0B2A} new",
        "",
        " ╭\u{E3D5} p3",
        "                     ▼ 7 more",
        "",
        "───\u{E0B6}Standing\u{E0B4}─────────────\u{E0B6}\u{F0B26}\u{E0B4}─",
        "",
        " ╭\u{E3D5} cpd",
        " ╰\u{F0B2A} new",
    ];
    assert_eq!(tree.screen(), expected);
}

#[test]
fn the_cursor_crosses_between_the_project_tree_and_the_section() {
    let fx = fx();
    let _engine = engine(&fx);
    // `cpd` is the newest Context: in the project tree it would come first
    populate(&fx, &[("aa", &["pa"]), ("cpd", &["pc"])]);
    drive::fake_cpd(
        &fx,
        r#"{"ok":true,"session":"x","pane":"%1","displaced":null}"#,
    );
    let mut tree = Tree::start(&fx, W as u16, 16);
    tree.keys(&["j", "Enter"]); // the cursor appears on the first project row: the Lead of `pa`
    tree.keys(&["j", "Enter"]); // after the last project row, the first Standing row: the Lead of `pc`
    tree.keys(&["k", "Enter"]); // and back
    assert_eq!(
        drive::calls(&fx),
        [
            drive::words(&["session", "open", "l-pa"]),
            drive::words(&["session", "open", "l-pc"]),
            drive::words(&["session", "open", "l-pa"]),
        ]
    );
}

/// The menu's box on the screen: the rows from its top border to its bottom border, cut at its left column.
fn menu_box(screen: &[String]) -> Vec<String> {
    let first = row_of(screen, "│ New Plan…") as usize;
    let column = screen[first]
        .chars()
        .position(|c| c == '│')
        .expect("the box's left border");
    let rows: Vec<String> = screen[first - 1..]
        .iter()
        .map(|r| r.chars().skip(column).collect::<String>())
        .collect();
    let end = rows
        .iter()
        .position(|r| r.starts_with('╰'))
        .expect("the bottom border");
    rows[..=end]
        .iter()
        .map(|r| r.chars().take(13).collect())
        .collect()
}

#[test]
fn the_menu_of_a_permanent_context_has_no_archive_or_rename() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx, &[("host", &[]), ("aa", &[])]);
    let mut tree = Tree::start(&fx, W as u16, 24);
    let screen = tree.screen();
    tree.right_click(8, row_of(&screen, "aa"));
    assert_eq!(
        menu_box(&tree.screen()),
        [
            "╭───────────╮",
            "│ New Plan… │",
            "│ Rename…   │",
            "│ Copy id   │",
            "│ Archive   │",
            "╰───────────╯",
        ],
        "an ordinary Context keeps its four items"
    );
    tree.keys(&["Escape"]);
    let screen = tree.screen();
    tree.right_click(8, row_of(&screen, "host"));
    assert_eq!(
        menu_box(&tree.screen()),
        [
            "╭───────────╮",
            "│ New Plan… │",
            "│ Copy id   │",
            "╰───────────╯",
        ],
        "a Permanent Context: New Plan… and Copy id only"
    );
}

/// REGRESSION (passes before the section exists): a click on a Permanent Context's row runs `context manager --by-user`.
#[test]
fn a_click_on_a_permanent_context_runs_context_manager_by_user() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(
        &fx,
        &[
            ("cpd", &[]),
            ("host", &[]),
            ("codereview", &[]),
            ("aa", &[]),
        ],
    );
    drive::fake_cpd(
        &fx,
        r#"{"ok":true,"manager":{"session":"m-cr","name":"codereview-manager","state":"idle","pane":"%1","window":"@1"},"started":false,"session":"m-cr","displaced":null}"#,
    );
    let mut tree = Tree::start(&fx, W as u16, 24);
    let screen = tree.screen();
    tree.click(8, row_of(&screen, "codereview"));
    assert_eq!(
        drive::calls(&fx)[0],
        drive::words(&["context", "manager", "--context=codereview", "--by-user"])
    );
}

// ---------- the docked Archive section (pro-d348, pro-d349) ----------

#[test]
fn the_archive_section_is_docked_below_standing_folded_by_default_with_one_shared_blank_row() {
    let with = fx();
    let _engine = engine(&with);
    populate(&with, &[("cpd", &[]), ("aa", &[]), ("zed", &["zp"])]);
    archive(&with, &["zed"]);
    let tree = Tree::start(&with, W as u16, 24);
    let standing = fit(&block("cpd", &[]));
    // Standing open (its reserved end row is the one row shared with Archive), Archive folded: its header is the last row but one.
    // The project tree holds `aa` at the top: no `archive` row, no `zed`.
    assert_eq!(
        tree.screen(),
        docked(
            24,
            "",
            &block("aa", &[]),
            "",
            &group(Some((true, &standing)), Some((false, &[])))
        )
    );
    assert_eq!(
        tree.screen()[22],
        header("Archive", FOLDED),
        "the Archive header is the pane's last row but one, its glyph U+F0B2C"
    );
    assert_eq!(
        tree.screen().len(),
        23,
        "the pane's last row, 23, is the blank row below the folded header"
    );
    assert_eq!(
        tree.screen()[21],
        "",
        "Standing's reserved end row: the one blank row above Archive's header"
    );
    drop(tree);
    drop(_engine);
    drop(with);
    // with no archived Context: the Standing-only layout, no Archive header
    let without = fx();
    let _engine = engine(&without);
    populate(&without, &[("cpd", &[]), ("aa", &[]), ("zed", &["zp"])]);
    let tree = Tree::start(&without, W as u16, 24);
    assert_eq!(
        tree.screen(),
        pane(
            24,
            "",
            &blocks(&[block("zed", &["zp"]), block("aa", &[])]),
            "",
            Some((true, &standing))
        )
    );
    assert!(
        !tree.screen().iter().any(|r| r.contains("Archive")),
        "no archived Context, no Archive section: {:?}",
        tree.screen()
    );
}

#[test]
fn a_click_on_the_archive_header_opens_and_folds_it_with_the_fold_glyph() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx, &[("cpd", &[]), ("aa", &[]), ("zed", &["zp"])]);
    archive(&fx, &["zed"]);
    expand(&fx, &["zed"], false);
    let mut tree = Tree::start(&fx, W as u16, 24);
    let standing = fit(&block("cpd", &[]));
    let folded = docked(
        24,
        "",
        &block("aa", &[]),
        "",
        &group(Some((true, &standing)), Some((false, &[]))),
    );
    assert_eq!(tree.screen(), folded, "folded by default");
    tree.click(5, 22); // the Archive header, the pane's last row but one
    // open (glyph U+F0B26): a reserved blank row under the header, zed's block (title, its archived Plan, its closing row),
    // the reserved last row at the bottom (dropped by the screen). Group: 12 rows, the half of the pane, so nothing scrolls.
    let archived = fit(&archived_block("zed", &["zp"], true));
    let open = docked(
        24,
        "",
        &block("aa", &[]),
        "",
        &group(Some((true, &standing)), Some((true, &archived))),
    );
    assert_eq!(tree.screen(), open);
    assert_eq!(
        tree.screen()[18],
        header("Archive", OPEN),
        "the header moved up with the section: group rows 12 to 23, the header at 18"
    );
    tree.click(5, 18);
    assert_eq!(tree.screen(), folded, "a second click folds it back");
    assert!(
        drive::calls(&fx).is_empty(),
        "folding runs no command: {:?}",
        drive::calls(&fx)
    );
}

#[test]
fn a_folded_standing_leaves_one_blank_row_above_the_archive_header() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx, &[("cpd", &[]), ("aa", &[]), ("zed", &["zp"])]);
    archive(&fx, &["zed"]);
    let mut tree = Tree::start(&fx, W as u16, 24);
    // the group: blank row, Standing's header (row 17), 4 rows of its own, Archive's header, a blank row; Standing's header is row 17
    tree.click(5, 17);
    // the rows at the bottom: blank row, Standing header (U+F0B2C), one blank row, Archive header, then the pane's last row blank
    let folded = group(Some((false, &[])), Some((false, &[])));
    assert_eq!(
        folded,
        [
            String::new(),
            header("Standing", FOLDED),
            String::new(),
            header("Archive", FOLDED),
            String::new()
        ]
    );
    assert_eq!(
        tree.screen(),
        docked(24, "", &block("aa", &[]), "", &folded)
    );
    assert_eq!(tree.screen()[22], header("Archive", FOLDED));
    assert_eq!(tree.screen()[21], "");
    assert_eq!(tree.screen()[20], header("Standing", FOLDED));
    assert_eq!(tree.screen()[19], "");
    assert_eq!(tree.screen().len(), 23, "the pane's last row is blank");
}

#[test]
fn the_docked_group_is_capped_at_half_the_pane_and_each_section_scrolls_on_its_own() {
    let fx = fx();
    let _engine = engine(&fx);
    // Standing: the three Permanent Contexts (the only ones), 3 Plans each; Archive: z1, z2, z3, 2 Plans each, their own
    // toggle open; the project tree: `aa`. (Archived Contexts are listed by id.)
    populate(
        &fx,
        &[
            ("cpd", &["c1", "c2", "c3"]),
            ("host", &["h1", "h2", "h3"]),
            ("codereview", &["r1", "r2", "r3"]),
            ("aa", &[]),
            ("z1", &["a1", "a2"]),
            ("z2", &["b1", "b2"]),
            ("z3", &["d1", "d2"]),
        ],
    );
    archive(&fx, &["z1", "z2", "z3"]);
    expand(&fx, &["z1", "z2", "z3"], true);
    // A pane of 20 rows: B = floor(20 / 2) = 10. Standing: 3 blocks of 4 rows and 2 spacers, n_S = 14; Archive: 3 blocks of
    // 4 rows (title, 2 Plans, `collapse`) and 2 spacers, n_A = 14. The group wants 1 + (3 + 14) + (3 + 14) = 35 > 10, so
    // the fixed rows F = 1 (blank) + 2 (headers) + 4 (reserved rows) = 7 stay and L = 10 - 7 = 3 list rows are shared:
    // Archive is open with rows, so a = 1, n_S' = min(14, 3 - 1) = 2 and n_A' = min(14, 3 - 2) = 1. The group is 10 rows,
    // the project region 10 rows: screen rows 0 to 9 (the project tree), 10 the blank row, 11 Standing's header, 12 its
    // `▲` row, 13 and 14 its list, 15 its `▼` row (shared with Archive), 16 Archive's header, 17 its `▲` row, 18 its
    // list row, 19 its `▼` row. A wheel notch scrolls 3 rows (view::WHEEL_ROWS, the tree's notch; guess: the sections
    // keep it) and the offsets clamp at 14 - 2 = 12 and 14 - 1 = 13.
    let all_s = blocks(&[
        block("cpd", &["c3", "c2", "c1"]),
        block("host", &["h3", "h2", "h1"]),
        block("codereview", &["r3", "r2", "r1"]),
    ]);
    let all_a = blocks(&[
        archived_block("z1", &["a2", "a1"], true),
        archived_block("z2", &["b2", "b1"], true),
        archived_block("z3", &["d2", "d1"], true),
    ]);
    assert_eq!((all_s.len(), all_a.len()), (14, 14));
    let shown = |s: usize, a: usize| {
        let mut standing = vec![above_cue(s)];
        standing.extend(all_s[s..s + 2].iter().cloned());
        standing.push(below_cue(14, s, 2));
        let mut archived = vec![above_cue(a)];
        archived.extend(all_a[a..a + 1].iter().cloned());
        archived.push(below_cue(14, a, 1));
        docked(
            20,
            "",
            &block("aa", &[]),
            "",
            &group(Some((true, &standing)), Some((true, &archived))),
        )
    };
    let mut tree = Tree::start(&fx, W as u16, 20);
    assert_eq!(tree.screen(), shown(0, 0), "the start");
    assert_eq!(tree.screen()[15], cue("▼", 12));
    assert_eq!(tree.screen()[19], cue("▼", 13));
    wheel(&mut tree, true, 3); // over the project region: neither section moves
    assert_eq!(
        tree.screen(),
        shown(0, 0),
        "the project region's wheel is its own"
    );
    wheel(&mut tree, true, 13); // over Standing's list rows: only Standing's rows move
    assert_eq!(tree.screen(), shown(3, 0), "only Standing scrolled");
    wheel(&mut tree, true, 18); // over Archive's list row: only Archive's rows move
    assert_eq!(tree.screen(), shown(3, 3), "only Archive scrolled");
    for _ in 0..6 {
        wheel(&mut tree, true, 14);
    }
    assert_eq!(
        tree.screen(),
        shown(12, 3),
        "Standing at its end: its last two rows, no `▼`"
    );
    for _ in 0..6 {
        wheel(&mut tree, true, 18);
    }
    assert_eq!(
        tree.screen(),
        shown(12, 13),
        "Archive at its end: its last row, no `▼`"
    );
    wheel(&mut tree, true, 3);
    assert_eq!(
        tree.screen(),
        shown(12, 13),
        "the project region's wheel moves neither"
    );
    for _ in 0..6 {
        wheel(&mut tree, false, 14);
    }
    assert_eq!(
        tree.screen(),
        shown(0, 13),
        "Standing back at its start, Archive stays"
    );
    for _ in 0..6 {
        wheel(&mut tree, false, 18);
    }
    assert_eq!(tree.screen(), shown(0, 0));
}

/// REGRESSION (a click on an archived Context's title keeps its command; it fails today because the row is not drawn there
/// yet): with Archive open, the click runs `context unarchive <id>`, then `context manager --context=<id> --by-user`.
#[test]
fn a_click_on_an_archived_contexts_title_in_the_archive_section_unarchives_it_and_opens_its_manager()
 {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx, &[("aa", &[]), ("zed", &["zp"])]);
    archive(&fx, &["zed"]);
    drive::fake_cpd(
        &fx,
        r#"{"ok":true,"manager":{"session":"m-zed","name":"zed-manager","state":"idle","pane":"%1","window":"@1"},"started":false,"session":"m-zed","displaced":null}"#,
    );
    let mut tree = Tree::start(&fx, W as u16, 24);
    // no Standing: the group is the blank row, Archive's header (row 22) and the blank last row
    assert_eq!(
        tree.screen(),
        docked(
            24,
            "",
            &block("aa", &[]),
            "",
            &group(None, Some((false, &[])))
        )
    );
    tree.click(5, 22);
    // open, zed folded: the group is 1 + 3 + 2 = 6 rows; header 19, `▲` row 20, title 21, `expand` 22, `▼` row 23
    let archived = fit(&archived_block("zed", &["zp"], false));
    assert_eq!(
        tree.screen(),
        docked(
            24,
            "",
            &block("aa", &[]),
            "",
            &group(None, Some((true, &archived)))
        )
    );
    tree.click(8, 21);
    assert_eq!(
        drive::calls(&fx),
        [
            drive::words(&["context", "unarchive", "zed"]),
            drive::words(&["context", "manager", "--context=zed", "--by-user"]),
            drive::words(&["session", "open", "m-zed"]),
        ]
    );
}
