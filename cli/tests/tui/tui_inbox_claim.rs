//! The Claim entry of the Inbox right-click menu (commands.md §13.2, §17): the view runs in the test process on a `Tree`
//! (`common/tree.rs`) over a prepared store, with the fake `cpd` and the fake tmux of the `Fx`; the menu is read from the drawn
//! buffer and the commands the view ran are the fake `cpd`'s calls.

use crate::common;

use common::TestEnv;
use common::tree::{self as drive, Tree, engine, fx, stamp};
use cpd_core::home::CpdHome;
use cpd_core::ids::{ContextId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::store::Store;

const ID: i64 = 12;

fn open(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

fn condition(kind: Kind, urgency: Urgency, subject: &str, label: &str, url: Option<&str>) -> Condition {
    Condition {
        kind,
        urgency,
        subject: subject.to_owned(),
        label: label.to_owned(),
        url: url.map(str::to_owned),
        context: Some(ContextId::parse("alpha").unwrap()),
        plan: None,
        job: None,
        session: Some(SessionId::parse("m-alpha").unwrap()),
        record: None,
        event: None,
    }
}

/// The Context `alpha` with its Manager, a `review-requested` item (id 12) and an `unread` one.
fn populate(fx: &common::fx::Fx) {
    let mut store = open(&fx.env);
    common::sql(
        &mut store,
        "INSERT INTO contexts (id, title, jira, created_at, created_on)
         VALUES ('alpha', 'Title of alpha', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('m-alpha', 'manager', 'alpha', NULL, NULL, 'alpha-manager', '/sessions/m-alpha.jsonl', '/repo', 'O5',
                 NULL, 'idle', NULL, '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:07.000Z');",
    );
    let items = [
        condition(Kind::ReviewRequested, Urgency::Blue, "pr:acme/repo#7", "review label", Some("https://github.com/acme/repo/pull/7")),
        condition(Kind::Unread, Urgency::White, "session:m-alpha", "unread label", None),
    ];
    reconcile(&mut store, &items, &stamp(fx, -330)).unwrap();
    common::sql(&mut store, &format!("UPDATE inbox_items SET id = {ID} WHERE kind = 'review-requested';"));
}

fn words(w: &[&str]) -> Vec<String> {
    w.iter().map(|s| (*s).to_owned()).collect()
}

fn row_of(screen: &[String], label: &str) -> u16 {
    screen.iter().position(|r| r.contains(label)).unwrap_or_else(|| panic!("{label} on\n{}", screen.join("\n"))) as u16
}

/// The entries of the open menu, top to bottom: the rows between its `╮` row and its `╯` row.
fn menu_entries(screen: &[String]) -> Vec<String> {
    let top = screen.iter().position(|r| r.contains('╮')).unwrap_or_else(|| panic!("no menu on\n{}", screen.join("\n")));
    let bottom = screen.iter().position(|r| r.contains('╯')).unwrap_or_else(|| panic!("no menu bottom on\n{}", screen.join("\n")));
    // the menu is drawn over the rows: its columns are those of the `╭` and `╮` of the top border
    let chars: Vec<char> = screen[top].chars().collect();
    let right = chars.iter().position(|c| *c == '╮').unwrap();
    let left = chars[..right].iter().rposition(|c| *c == '╭').expect("the top border's corner");
    screen[top + 1..bottom].iter().map(|r| r.chars().skip(left + 1).take(right - left - 1).collect::<String>().trim_matches(|c: char| c == '│' || c.is_whitespace()).to_owned()).collect()
}

fn menu_open(screen: &[String]) -> bool {
    screen.iter().any(|r| r.contains('╮'))
}

fn start(fx: &common::fx::Fx) -> Tree {
    populate(fx);
    let tree = Tree::start(fx, 46, 20);
    assert!(tree.screen().iter().any(|r| r.contains("alpha")), "the tree: {:?}", tree.screen());
    tree
}

#[test]
fn a_right_click_on_a_review_requested_row_opens_claim_snooze_and_dismiss_and_no_other_row_has_claim() {
    let fx = fx();
    let _engine = engine(&fx);
    let mut tree = start(&fx);
    tree.right_click(10, row_of(&tree.screen(), "review label"));
    let screen = tree.until("the item's menu", menu_open);
    assert_eq!(menu_entries(&screen), ["Claim", "Snooze 15 min", "Snooze 1 h", "Snooze until tomorrow 09:00", "Dismiss"]);
    tree.keys(&["Escape"]);
    tree.until("the menu closed", |s| !menu_open(s));
    tree.right_click(10, row_of(&tree.screen(), "unread label"));
    let screen = tree.until("the item's menu", menu_open);
    assert_eq!(menu_entries(&screen), ["Snooze 15 min", "Snooze 1 h", "Snooze until tomorrow 09:00", "Dismiss"]);
}

#[test]
fn choosing_claim_runs_inbox_claim_with_the_items_id() {
    let fx = fx();
    let _engine = engine(&fx);
    drive::fake_cpd(&fx, r#"{"ok":true}"#);
    let mut tree = start(&fx);
    tree.right_click(10, row_of(&tree.screen(), "review label"));
    tree.until("the item's menu", menu_open);
    tree.keys(&["Enter"]); // Claim is the first entry
    assert_eq!(drive::calls(&fx), [words(&["inbox", "claim", &ID.to_string()])]);
    tree.until_logged("claimed");
}

#[test]
fn a_refused_claim_opens_a_text_dialog_with_the_refusal() {
    let fx = fx();
    let _engine = engine(&fx);
    drive::cpd_fails(
        &fx,
        r#"{"ok":false,"error":{"code":"review_context_unset","message":"No review Context is set.","details":{}}}"#,
    );
    let dialogs = drive::dialogs(&fx);
    let mut tree = start(&fx);
    tree.right_click(10, row_of(&tree.screen(), "review label"));
    tree.until("the item's menu", menu_open);
    tree.keys(&["Enter"]);
    assert_eq!(drive::calls(&fx), [words(&["inbox", "claim", &ID.to_string()])]);
    let popups = dialogs.popups();
    assert!(popups.first().is_some_and(|p| p.starts_with("cpd tui view dialog text ")), "a `text` dialog: {popups:?}");
    assert!(
        popups[0].contains("inbox claim 12") && popups[0].contains("review_context_unset: No review Context is set."),
        "`<what failed>: <code>: <message>`: {popups:?}"
    );
    dialogs.settled(&mut tree);
    assert!(!tree.shows("review_context_unset"), "no note: {:?}", tree.screen());
}
