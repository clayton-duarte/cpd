//! The tree's **Delete** menu item and its popup (commands.md §13.2; hs-d527). The tests run in the test process on a
//! `Tree` (`common/tree.rs`) over a prepared store, with the fake `cpd` that records its command lines and the fake tmux
//! that plays the dialog kit's popup. The tree runs `plan delete <id>` for the popup's list and, on `y`,
//! `plan delete <id> --yes`.

use crate::common::tree::{self as drive, Tree, engine, fx};
use crate::common::{TestEnv, sql};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;

// ---------- the prepared store ----------

fn open(env: &TestEnv) -> Store {
    Store::open(
        &CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(),
        5000,
    )
    .unwrap()
}

fn plan_row(id: &str, phase: &str, lead: &str, created: &str) -> String {
    let done = phase == "done";
    let (pr, prs) = if done {
        (
            format!(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                 VALUES ('acme/{id}', 7, 'https://github.com/acme/{id}/pull/7', 'PR', 0, 'merged', 'none', 'none', '2026-01-05T00:00:00.000Z',
                         '2026-01-09T00:00:00.000Z', '2026-01-06T00:00:00.000Z');"
            ),
            format!(
                "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                                  created_txn, created_at, started_at, merged_at)
                 VALUES ('{id}', 'repo', '/repo/{id}', 'main', 'merged', 'cpd/{id}', '/wt', 7, 'https://github.com/acme/{id}/pull/7',
                         '2026-01-05T00:00:00.000Z', (SELECT id FROM pull_requests WHERE repo = 'acme/{id}'), cpd_txn(), '{created}',
                         '2026-01-02T00:00:00.000Z', '2026-01-09T00:00:00.000Z');"
            ),
        )
    } else {
        (String::new(), String::new())
    };
    let (locked, done_at) = if done {
        ("'2026-01-02T00:00:00.000Z'", "'2026-01-09T00:00:00.000Z'")
    } else {
        ("NULL", "NULL")
    };
    format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
         VALUES ('{id}', 'alpha', 'Plan {id}', '{phase}', '{lead}', '{created}', {locked}, {done_at});
         {pr}{prs}"
    )
}

fn lead_row(id: &str, plan: &str, state: &str, created: &str) -> String {
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('{id}', 'lead', 'alpha', '{plan}', NULL, '{plan}', '/sessions/{id}.jsonl', '/repo', 'O3', NULL, '{state}', NULL,
                 '2026-01-03T00:00:00.000Z', '{created}');"
    )
}

/// Context `alpha` with three Plans: `live` (phase `plan`, its Lead idle), `old` (done, archived with its Lead) and `nap`
/// (done; asleep after [`put_to_sleep`]).
fn populate(env: &TestEnv) {
    let mut s = String::from(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('alpha', 'Title of alpha', NULL, '2026-01-01T00:00:00.000Z', 'm1');",
    );
    s += &plan_row("live", "plan", "l-live", "2026-01-02T00:00:00.000Z");
    s += &plan_row("old", "done", "l-old", "2026-01-02T00:00:01.000Z");
    s += &plan_row("nap", "done", "l-nap", "2026-01-02T00:00:02.000Z");
    s += &lead_row("l-live", "live", "idle", "2026-01-02T00:00:00.000Z");
    s += &lead_row("l-old", "old", "stopped", "2026-01-02T00:00:01.000Z");
    s += &lead_row("l-nap", "nap", "stopped", "2026-01-02T00:00:02.000Z");
    s += "UPDATE plans SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'old';
          UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'l-old';";
    sql(&mut open(env), &s);
}

/// `nap`'s Lead is asleep (contract §2.2, hs-d515): a timestamp of the Session, the Plan not archived. Needs the sleep PR's column.
fn put_to_sleep(env: &TestEnv) {
    // with the `session-asleep` Event that engine E13 writes in the same transaction (contract §3.7): it is what the tree follows
    sql(
        &mut open(env),
        "UPDATE sessions SET asleep_at = '2026-02-01T00:00:00.000Z' WHERE id = 'l-nap';
         INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-02-01T00:00:00.000Z', 'session-asleep', 'alpha', 'nap', 'l-nap', 'Session nap is asleep and out of view.', '{}');",
    );
}

fn has_item(screen: &[String], item: &str) -> bool {
    screen.iter().any(|r| r.contains(&format!("│ {item} ")))
}

/// What `plan delete old` prints without `--yes`.
const PREVIEW: &str = r#"{"ok":true,"preview":true,"jobs":47,"attempts":121,"sessions":3,"worktrees":11,"disk_freed_mb":342}"#;

// ---------- tests ----------

/// The row of the Plan `id` (its label is the id) on `tree`.
fn row_of_plan(tree: &mut Tree, id: &str) -> u16 {
    let screen = tree.until(&format!("the row of {id}"), |s| {
        s.iter().any(|r| r.contains(&format!(" {id}")))
    });
    screen
        .iter()
        .position(|r| r.contains(&format!(" {id}")))
        .unwrap() as u16
}

/// Right-clicks the Plan `id` and returns the screen with its menu open.
fn menu_of_plan(tree: &mut Tree, id: &str) -> Vec<String> {
    let row = row_of_plan(tree, id);
    tree.right_click(8, row);
    tree.until("the menu", |s| {
        s.iter().any(|r| r.contains('╮')) && s.iter().any(|r| r.contains("│ Copy id "))
    })
}

fn close_plan_menu(tree: &mut Tree) {
    tree.keys(&["Escape"]);
    tree.until("the menu closed", |s| s.iter().all(|r| !r.contains('╮')));
}

#[test]
fn delete_item_only_on_archived_or_asleep_rows() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx.env);
    drive::fake_cpd(&fx, r#"{"ok":true}"#);
    let mut tree = Tree::start(&fx, 60, 16);

    let live = menu_of_plan(&mut tree, "live");
    assert!(
        has_item(&live, "New session…") && !has_item(&live, "Delete"),
        "a live Plan has no Delete: {live:?}"
    );
    close_plan_menu(&mut tree);
    let archived = menu_of_plan(&mut tree, "old");
    assert!(
        has_item(&archived, "Unarchive") && has_item(&archived, "Delete"),
        "an archived Plan has Delete: {archived:?}"
    );
    close_plan_menu(&mut tree);
    put_to_sleep(&fx.env);
    // the tree reads the Event when its poll comes: a menu opened before that is of the old snapshot, so try again
    let mut tries = 0;
    let asleep = loop {
        let menu = menu_of_plan(&mut tree, "nap");
        close_plan_menu(&mut tree);
        if has_item(&menu, "Delete") {
            break menu;
        }
        tries += 1;
        assert!(
            tries < 100,
            "the Delete item of the asleep Plan never shows; the last menu: {menu:?}"
        );
        tree.idle();
    };
    assert!(
        has_item(&asleep, "Delete"),
        "an asleep Plan has Delete: {asleep:?}"
    );
    assert!(
        drive::calls(&fx).is_empty(),
        "opening menus runs nothing: {:?}",
        drive::calls(&fx)
    );
}

#[test]
fn popup_lists_what_goes_and_n_keeps_it() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx.env);
    drive::cpd_says(&fx, PREVIEW);
    let dialogs = drive::dialogs(&fx);
    dialogs.cancel(); // the User answers N
    let mut tree = Tree::start(&fx, 60, 16);

    let screen = menu_of_plan(&mut tree, "old");
    let row = screen
        .iter()
        .position(|r| r.contains("│ Delete "))
        .expect("Delete in the menu") as u16;
    tree.click(10, row);

    // the popup's list is the preview's: the first call is the command without `--yes`
    tree.until_that("the popup", |_| !dialogs.popups().is_empty());
    assert_eq!(
        drive::calls(&fx)[0],
        drive::words(&["plan", "delete", "old"]),
        "the list comes from `plan delete <id>`"
    );
    let popup = &dialogs.popups()[0];
    assert!(
        popup.contains("dialog confirm"),
        "a y/N confirm popup: {popup}"
    );
    assert!(popup.contains("Delete"), "{popup}");
    for count in ["47", "121", "3", "11", "342"] {
        assert!(
            popup.contains(count),
            "the popup lists the count {count}: {popup}"
        );
    }
    // N: nothing is deleted; the legend's popup comes after the view handled the answer
    dialogs.settled(&mut tree);
    assert_eq!(
        drive::calls(&fx),
        [drive::words(&["plan", "delete", "old"])],
        "no `--yes` after N"
    );
}

#[test]
fn y_deletes_the_plan() {
    let fx = fx();
    let _engine = engine(&fx);
    populate(&fx.env);
    drive::cpd_says(&fx, PREVIEW);
    let dialogs = drive::dialogs(&fx);
    dialogs.confirm(); // the User answers y
    let mut tree = Tree::start(&fx, 60, 16);

    let screen = menu_of_plan(&mut tree, "old");
    let row = screen
        .iter()
        .position(|r| r.contains("│ Delete "))
        .expect("Delete in the menu") as u16;
    tree.click(10, row);

    tree.until_that("the delete with --yes", |_| drive::calls(&fx).len() >= 2);
    assert_eq!(
        drive::calls(&fx),
        [
            drive::words(&["plan", "delete", "old"]),
            drive::words(&["plan", "delete", "old", "--yes"])
        ]
    );
    assert_eq!(dialogs.popups().len(), 1, "one popup, before the delete");
}
