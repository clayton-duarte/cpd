//! The tree's **Delete** menu item and its popup (commands.md §13.2; hs-d527), end to end like `tui_tree.rs`: the built
//! binary in a pane of a private tmux server over a prepared store, a fake `cpd` that records its command lines and a fake
//! `tmux` that plays the dialog kit's popup. The tree runs `plan delete <id>` for the popup's list and, on `y`,
//! `plan delete <id> --yes`.

use crate::common;

use common::{CPD, TestEnv, sql};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use nix::fcntl::{Flock, FlockArg};

// ---------- the prepared store ----------

/// An env with its config and an engine running (`engine-down` is no row of the Inbox).
fn env() -> TestEnv {
    let env = TestEnv::new();
    let config = format!(
        "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n[events]\npoll_ms = 50\n[tmux]\nsocket = \"{}\"\n",
        env.tmux_socket()
    );
    std::fs::write(env.cpd_home().join("config.toml"), config).unwrap();
    let path = env.cpd_home().join("locks/watch.lock");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lock = Flock::lock(std::fs::File::create(path).unwrap(), FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap();
    std::mem::forget(lock);
    env
}

fn open(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
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
    let (locked, done_at) = if done { ("'2026-01-02T00:00:00.000Z'", "'2026-01-09T00:00:00.000Z'") } else { ("NULL", "NULL") };
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

// ---------- the pane ----------

const SESSION: &str = "tree";

struct Pane<'a> {
    env: &'a TestEnv,
}

impl<'a> Pane<'a> {
    fn start(env: &'a TestEnv, width: u16, height: u16) -> Pane<'a> {
        let out = env.tmux(&[
            "-u", "set-option", "-g", "remain-on-exit", "on", ";", "new-session", "-d", "-s", SESSION, "-x", &width.to_string(), "-y", &height.to_string(),
            &format!("{CPD} tui view tree"),
        ]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        Pane { env }
    }

    fn screen(&self) -> Vec<String> {
        let out = self.env.tmux(&["capture-pane", "-p", "-t", SESSION]);
        String::from_utf8(out.stdout).unwrap().lines().map(|l| l.trim_end().to_owned()).collect()
    }

    fn wait(&self, what: &str, done: impl Fn(&[String]) -> bool) -> Vec<String> {
        common::wait_screen(what, || self.screen(), done)
    }

    fn type_text(&self, text: &str) {
        assert!(self.env.tmux(&["send-keys", "-t", SESSION, "-l", text]).status.success());
    }

    fn right_click(&self, column: u16, row: u16) {
        for end in ['M', 'm'] {
            self.type_text(&format!("\x1b[<2;{};{}{end}", column + 1, row + 1));
        }
    }

    fn click(&self, column: u16, row: u16) {
        for end in ['M', 'm'] {
            self.type_text(&format!("\x1b[<0;{};{}{end}", column + 1, row + 1));
        }
    }

    fn keys(&self, keys: &[&str]) {
        let mut args = vec!["send-keys", "-t", SESSION];
        args.extend(keys);
        assert!(self.env.tmux(&args).status.success());
    }

    /// The row of the Plan `id` (its label is the id), once the tree shows it.
    fn row_of(&self, id: &str) -> u16 {
        let screen = self.wait(&format!("the row of {id}"), |s| s.iter().any(|r| r.contains(&format!(" {id}"))));
        screen.iter().position(|r| r.contains(&format!(" {id}"))).unwrap() as u16
    }

    /// Right-clicks the Plan `id` and returns the screen with its menu open.
    fn menu_of(&self, id: &str) -> Vec<String> {
        let row = self.row_of(id);
        self.right_click(8, row);
        self.wait("the menu", |s| s.iter().any(|r| r.contains('╮')) && s.iter().any(|r| r.contains("│ Copy id ")))
    }

    fn close_menu(&self) {
        self.keys(&["Escape"]);
        self.wait("the menu closed", |s| s.iter().all(|r| !r.contains('╮')));
    }
}

fn has_item(screen: &[String], item: &str) -> bool {
    screen.iter().any(|r| r.contains(&format!("│ {item} ")))
}

fn fake_cpd(env: &TestEnv, reply: &str) {
    env.fake("cpd", &format!("echo '{reply}'"));
}

fn calls(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("cpd")
}

fn wait_for_calls(env: &TestEnv, n: usize) -> Vec<Vec<String>> {
    common::wait_some(&format!("{n} calls of cpd"), common::WAIT, || {
        let calls = calls(env);
        if calls.len() >= n { Ok(calls) } else { Err(format!("got {calls:?}")) }
    })
}

fn words(w: &[&str]) -> Vec<String> {
    w.iter().map(|s| (*s).to_owned()).collect()
}

/// A fake tmux with a client whose `display-popup` writes the env's answer to the popup's `--out` file (`tui_tree.rs`'s).
fn fake_dialogs(env: &TestEnv, confirmed: bool) {
    std::fs::write(env.root().join("dialog-answer.json"), serde_json::json!({"confirmed": confirmed}).to_string()).unwrap();
    let real = String::from_utf8(env.command("sh").args(["-c", "command -v tmux"]).output().unwrap().stdout).unwrap();
    env.fake(
        "tmux",
        &format!(
            "case \"$*\" in\n *display-popup*) for _c; do :; done\n out=$(printf '%s' \"$_c\" | sed -n \"s/.*--out='\\([^']*\\)'.*/\\1/p\")\n cat '{answer}' > \"$out\";;\n *list-clients*) echo '100 200 60';;\n *) exec {real} \"$@\";;\nesac",
            answer = env.root().join("dialog-answer.json").display(),
            real = real.trim()
        ),
    );
}

/// The command each `display-popup` ran.
fn popups(env: &TestEnv) -> Vec<String> {
    env.calls("tmux").into_iter().filter(|c| c.iter().any(|a| a == "display-popup")).map(|c| c.last().unwrap().clone()).collect()
}

fn wait_for_popups(env: &TestEnv, n: usize) -> Vec<String> {
    common::wait_some(&format!("{n} popups"), common::WAIT, || {
        let p = popups(env);
        if p.len() >= n { Ok(p) } else { Err(format!("got {p:?}")) }
    })
}

/// What `plan delete old` prints without `--yes`.
const PREVIEW: &str = r#"{"ok":true,"preview":true,"jobs":47,"attempts":121,"sessions":3,"worktrees":11,"disk_freed_mb":342}"#;

// ---------- tests ----------

#[test]
fn delete_item_only_on_archived_or_asleep_rows() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 60, 16);

    let live = pane.menu_of("live");
    assert!(has_item(&live, "New session…") && !has_item(&live, "Delete"), "a live Plan has no Delete: {live:?}");
    pane.close_menu();
    let archived = pane.menu_of("old");
    assert!(has_item(&archived, "Unarchive") && has_item(&archived, "Delete"), "an archived Plan has Delete: {archived:?}");
    pane.close_menu();
    put_to_sleep(&env);
    // the tree reads the Event a moment after the write: a menu opened before that is of the old snapshot, so try again
    let asleep = common::wait_some("the Delete item of the asleep Plan", common::WAIT, || {
        let menu = pane.menu_of("nap");
        pane.close_menu();
        if has_item(&menu, "Delete") { Ok(menu) } else { Err(format!("the menu is {menu:?}")) }
    });
    assert!(has_item(&asleep, "Delete"), "an asleep Plan has Delete: {asleep:?}");
    assert!(calls(&env).is_empty(), "opening menus runs nothing: {:?}", calls(&env));
}

#[test]
fn popup_lists_what_goes_and_n_keeps_it() {
    let env = env();
    populate(&env);
    fake_cpd(&env, PREVIEW);
    fake_dialogs(&env, false); // the User answers N
    let pane = Pane::start(&env, 60, 16);

    let screen = pane.menu_of("old");
    let row = screen.iter().position(|r| r.contains("│ Delete ")).expect("Delete in the menu") as u16;
    pane.click(10, row);

    // the popup's list is the preview's: the first call is the command without `--yes`
    assert_eq!(wait_for_calls(&env, 1)[0], words(&["plan", "delete", "old"]), "the list comes from `plan delete <id>`");
    let popup = &wait_for_popups(&env, 1)[0];
    assert!(popup.contains("dialog confirm"), "a y/N confirm popup: {popup}");
    assert!(popup.contains("Delete"), "{popup}");
    for count in ["47", "121", "3", "11", "342"] {
        assert!(popup.contains(count), "the popup lists the count {count}: {popup}");
    }
    // N: nothing is deleted
    // time-claim: no `--yes` follows an N; 0.5 s is more than the tree takes to run a command after a popup answer (the Y test needs under 0.2 s); a negative has no signal to wait for.
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(calls(&env), [words(&["plan", "delete", "old"])], "no `--yes` after N");
}

#[test]
fn y_deletes_the_plan() {
    let env = env();
    populate(&env);
    fake_cpd(&env, PREVIEW);
    fake_dialogs(&env, true); // the User answers y
    let pane = Pane::start(&env, 60, 16);

    let screen = pane.menu_of("old");
    let row = screen.iter().position(|r| r.contains("│ Delete ")).expect("Delete in the menu") as u16;
    pane.click(10, row);

    assert_eq!(wait_for_calls(&env, 2), [words(&["plan", "delete", "old"]), words(&["plan", "delete", "old", "--yes"])]);
    assert_eq!(wait_for_popups(&env, 1).len(), 1, "one popup, before the delete");
}
