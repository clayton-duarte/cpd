//! The `merge` view (commands.md §13.2; hs-d273) on a private tmux server: the dialog's text, the keys and the clicks
//! that confirm or cancel, and the result or the error that follows. `pr merge` runs in the view's process and reaches
//! the fake `gh` of the env, whose recorded argv says whether a merge ran. The click is raw mouse bytes sent to the pane.

mod common;

use std::fs;
use std::time::Duration;

use common::{CPD, TestEnv, wait_until};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use cpd_tui::merge;

const REF: &str = "o/r#7";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

/// A test env with the config the views read, and the pull request `o/r#7` in the store.
fn env() -> TestEnv {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\nconcurrency = 4\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
             [tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    let mut store = Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                 VALUES ('o/r', 7, 'https://github.com/o/r/pull/7', 'Add the thing', 0, 'open', 'approved', 'passed',
                         '2026-01-01T00:00:00.000Z', NULL, '2026-01-02T00:00:00.000Z');",
            )?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
    env
}

/// A `gh` whose `pr view` says OPEN with `merge_state` and whose `pr merge` exits `merge_exit`.
fn gh(env: &TestEnv, merge_state: &str, merge_exit: i32) {
    env.fake_gh(&format!(
        "case \"$1 $2\" in\n\"pr view\") echo '{{\"state\":\"OPEN\",\"mergeStateStatus\":\"{merge_state}\",\"headRefOid\":\"{SHA}\",\"title\":\"Add the thing\"}}'; exit 0;;\n\"pr merge\") echo 'merge said no' >&2; exit {merge_exit};;\nesac\nexit 9"
    ));
}

fn merges(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("gh").into_iter().filter(|c| c[..2] == ["pr", "merge"]).collect()
}

fn tmux_out(env: &TestEnv, args: &[&str]) -> String {
    let out = env.tmux(args);
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// `cpd tui view merge o/r#7` in a window of the popup's inside, 58x5.
fn start(env: &TestEnv) -> String {
    let args = ["new-session", "-d", "-s", "view", "-x", "58", "-y", "5", "-P", "-F", "#{pane_id}", CPD, "tui", "view", "merge", REF];
    tmux_out(env, &args).trim().to_owned()
}

fn screen(env: &TestEnv, pane: &str) -> Vec<String> {
    tmux_out(env, &["capture-pane", "-p", "-t", pane]).lines().map(|l| l.trim_end().to_owned()).collect()
}

fn wait_screen(env: &TestEnv, pane: &str, what: &str, done: impl FnMut(&[String]) -> bool) -> Vec<String> {
    common::wait_screen(what, || screen(env, pane), done)
}

fn wait_dialog(env: &TestEnv, pane: &str) -> Vec<String> {
    wait_screen(env, pane, "the dialog", |r| r.iter().any(|l| l.contains("[Merge]")))
}

fn closed(env: &TestEnv) -> bool {
    !env.tmux(&["has-session", "-t", "view"]).status.success()
}

fn wait_closed(env: &TestEnv) {
    wait_until("the view to close", || closed(env));
}

/// A left button press at 1-based `(column, row)` of the pane, as the terminal sends it (SGR mouse reporting). The
/// release is not sent: the view acts on the press, and may be gone by the time a release would arrive.
fn click(env: &TestEnv, pane: &str, column: u16, row: u16) {
    tmux_out(env, &["send-keys", "-t", pane, "-l", &format!("\x1b[<0;{column};{row}M")]);
}

#[test]
fn the_dialog_shows_the_pull_request_its_title_the_method_and_both_buttons() {
    let env = env();
    gh(&env, "CLEAN", 0);
    let pane = start(&env);
    let rows = wait_dialog(&env, &pane);
    assert_eq!(rows[..5], [" o/r#7", " Add the thing", " method: squash", "", "  [Merge]   [Cancel]"]);
    assert!(env.calls("gh").is_empty(), "opening the dialog runs nothing");
}

#[test]
fn n_q_and_esc_close_it_and_run_no_merge() {
    for key in ["n", "q", "Escape"] {
        let env = env();
        gh(&env, "CLEAN", 0);
        let pane = start(&env);
        wait_dialog(&env, &pane);
        tmux_out(&env, &["send-keys", "-t", &pane, key]);
        wait_closed(&env);
        assert!(env.calls("gh").is_empty(), "{key}");
    }
}

#[test]
fn a_click_on_cancel_closes_it_and_runs_no_merge() {
    let env = env();
    gh(&env, "CLEAN", 0);
    let pane = start(&env);
    wait_dialog(&env, &pane);
    // `[Cancel]` is at 0-based column 12, row 4
    click(&env, &pane, 14, 5);
    wait_closed(&env);
    assert!(env.calls("gh").is_empty());
}

#[test]
fn y_or_enter_squash_merges_and_the_result_shows_until_a_key() {
    for key in ["y", "Enter"] {
        let env = env();
        gh(&env, "CLEAN", 0);
        let pane = start(&env);
        wait_dialog(&env, &pane);
        tmux_out(&env, &["send-keys", "-t", &pane, key]);
        let rows = wait_screen(&env, &pane, "the result", |r| r.iter().any(|l| l.contains("Merged o/r#7")));
        assert_eq!(rows[2], " Merged o/r#7 (squash).", "{key}");
        assert_eq!(
            merges(&env),
            [["pr", "merge", "7", "--repo", "o/r", "--squash", "--delete-branch", "--match-head-commit", SHA].map(str::to_owned)],
            "{key}"
        );
        // time-claim: the result stays until a key, over 300 ms after it shows; no per-poll signal from the view, which
        // blocks on its next terminal event.
        std::thread::sleep(Duration::from_millis(300));
        assert!(!closed(&env), "{key}");
        tmux_out(&env, &["send-keys", "-t", &pane, "x"]);
        wait_closed(&env);
    }
}

#[test]
fn a_click_on_merge_merges() {
    let env = env();
    gh(&env, "CLEAN", 0);
    let pane = start(&env);
    wait_dialog(&env, &pane);
    // `[Merge]` is at 0-based columns 2 to 8, row 4
    click(&env, &pane, 4, 5);
    wait_screen(&env, &pane, "the result", |r| r.iter().any(|l| l.contains("Merged o/r#7")));
    assert_eq!(merges(&env).len(), 1);
}

#[test]
fn an_error_of_pr_merge_is_shown_until_a_key() {
    // refused: the pull request is not CLEAN any more
    let env = env();
    gh(&env, "BLOCKED", 0);
    let pane = start(&env);
    wait_dialog(&env, &pane);
    tmux_out(&env, &["send-keys", "-t", &pane, "y"]);
    let rows = wait_screen(&env, &pane, "the error", |r| r.iter().any(|l| l.contains("BLOCKED")));
    assert!(rows[2].starts_with(" o/r#7 is OPEN and BLOCKED"), "{rows:?}");
    assert_eq!(rows[4], " any key closes");
    assert!(merges(&env).is_empty());
    // time-claim: the error stays until a key, over 300 ms after it shows; no per-poll signal from the view, which
    // blocks on its next terminal event.
    std::thread::sleep(Duration::from_millis(300));
    assert!(!closed(&env));
    tmux_out(&env, &["send-keys", "-t", &pane, "Enter"]);
    wait_closed(&env);

    // `gh pr merge` fails
    let env = self::env();
    gh(&env, "CLEAN", 1);
    let pane = start(&env);
    wait_dialog(&env, &pane);
    tmux_out(&env, &["send-keys", "-t", &pane, "y"]);
    let rows = wait_screen(&env, &pane, "the error", |r| r.iter().any(|l| l.contains("any key closes")));
    assert!(!rows[2].contains("Merged"), "{rows:?}");
    assert!(rows[2].trim().len() > 1, "{rows:?}");
    assert_eq!(merges(&env).len(), 1);
}

#[test]
fn a_pull_request_the_store_does_not_have_shows_no_title() {
    let env = env();
    gh(&env, "CLEAN", 0);
    let args = ["new-session", "-d", "-s", "view", "-x", "58", "-y", "5", "-P", "-F", "#{pane_id}", CPD, "tui", "view", "merge", "o/r#8"];
    let pane = tmux_out(&env, &args).trim().to_owned();
    let rows = wait_dialog(&env, &pane);
    assert_eq!(rows[..3], [" o/r#8", "", " method: squash"]);
}

#[test]
fn the_view_needs_a_terminal_and_a_pull_request_reference() {
    let env = env();
    let run = env.cpd(&["tui", "view", "merge", REF]);
    assert_eq!((run.exit, run.error()["code"].as_str()), (3, Some("not_a_terminal")), "{run:?}");
    for bad in [&["tui", "view", "merge"][..], &["tui", "view", "merge", "o/r"], &["tui", "view", "merge", "o/r#0"]] {
        assert_eq!(env.cpd(bad).exit, 2, "{bad:?}");
    }
}

#[test]
fn the_popup_runs_the_view_with_the_argument_quoted_and_is_capped_at_the_client() {
    assert_eq!(
        merge::popup_args((200, 60), REF),
        ["display-popup", "-E", "-T", " merge ", "-w", "60", "-h", "7", "cpd tui view merge 'o/r#7'"]
    );
    let small = merge::popup_args((50, 6), REF);
    assert_eq!((small[5].as_str(), small[7].as_str()), ("50", "6"));
}
