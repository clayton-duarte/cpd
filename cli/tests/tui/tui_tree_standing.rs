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

/// The header row: `───`, the pill `Standing`, `─`, the pill of the fold glyph (both with their caps) and `─` at the last column.
fn header(glyph: &str) -> String {
    let left = format!("{CAP_LEFT}Standing{CAP_RIGHT}");
    let right = format!("{CAP_LEFT}{glyph}{CAP_RIGHT}");
    let fill = W - 3 - "Standing".len() - 2 - 1 - 2 - 1;
    format!("───{left}{}{right}─", "─".repeat(fill))
}

/// An open section's rows with its reserved blank row above and below (blank when nothing is hidden).
fn fit(rows: &[String]) -> Vec<String> {
    let mut all = vec![String::new()];
    all.extend(rows.iter().cloned());
    all.push(String::new());
    all
}

/// The screen of an `h`-row pane, as `Tree::screen` reads it (rows without trailing blanks, no empty rows at the end): the
/// project tree's `above` cue row, its `list` rows, its `below` cue row, then, when `standing` is given, the blank row, the
/// header and, open, the section's `rows` (its own above cue row, the list, its below cue row: see [`fit`]); folded: none.
/// The tree takes the rows the section leaves.
fn pane(
    h: usize,
    above: &str,
    list: &[String],
    below: &str,
    standing: Option<(bool, &[String])>,
) -> Vec<String> {
    let rows: &[String] = match standing {
        Some((true, rows)) => rows,
        _ => &[],
    };
    let section = standing.map_or(0, |_| 2 + rows.len());
    let region = h - section;
    let mut screen = vec![above.to_owned()];
    screen.extend(list.iter().cloned());
    screen.resize(region - 1, String::new());
    screen.push(below.to_owned());
    if let Some((open, _)) = standing {
        screen.push(String::new());
        screen.push(header(if open { OPEN } else { FOLDED }));
        screen.extend(rows.iter().cloned());
    }
    assert_eq!(screen.len(), h, "the expected pane has {h} rows");
    while screen.last().is_some_and(String::is_empty) {
        screen.pop();
    }
    screen
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
        "folded: the blank row and the header remain, at the pane's bottom"
    );
    tree.click(5, 17);
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
