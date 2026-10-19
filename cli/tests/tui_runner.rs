//! `run_view`, which every view of `cpd tui view` runs through: whatever way a view ends, by a panic or by an error, the
//! terminal is restored and mouse reporting is off again.
//!
//! The view is this test binary itself, run in a pane of a private tmux server as the `#[ignore]`d `a_view` (the
//! parent runs it by name); the pane's `#{mouse_any_flag}` and `#{alternate_on}` are tmux's own record of what the
//! program asked of the terminal.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use common::{SLEEP_SECONDS, TestEnv, wait_until};
use cpd_tui::{Capture, ViewError, run_view};

/// The child: a view that waits for the file named by `CPD_TEST_VIEW_GO`, then ends the way `CPD_TEST_VIEW_ENDS` says.
#[test]
#[ignore = "run in a tmux pane by the tests below"]
fn a_view() {
    let go = PathBuf::from(std::env::var("CPD_TEST_VIEW_GO").expect("CPD_TEST_VIEW_GO"));
    let ends = std::env::var("CPD_TEST_VIEW_ENDS").expect("CPD_TEST_VIEW_ENDS");
    let result = run_view(
        "test",
        Capture::MouseAndFocus,
        || Ok(()),
        |terminal, ()| {
            while !go.exists() {
                terminal.draw(|_| {})?;
                std::thread::sleep(Duration::from_millis(50));
            }
            match ends.as_str() {
                "panic" => panic!("the view panicked"),
                _ => Err(ViewError::Terminal(std::io::Error::other("the view failed"))),
            }
        },
    );
    if let Err(e) = result {
        println!("{}", e.message());
    }
}

fn tmux_out(env: &TestEnv, args: &[&str]) -> String {
    let out = env.tmux(args);
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// Starts the child view in a pane, waits until it owns the terminal, lets it end as `ends` says, and returns the
/// pane's text and its `(mouse_any_flag, alternate_on)` after it ended.
fn view_ending(ends: &str, until: &str) -> (String, String) {
    let env = TestEnv::new();
    let go = env.root().join("go");
    let exe = std::env::current_exe().unwrap();
    let command = format!(
        "CPD_TEST_VIEW_GO='{}' CPD_TEST_VIEW_ENDS={ends} '{}' --exact a_view --ignored --nocapture; sleep {SLEEP_SECONDS}",
        go.display(),
        exe.display()
    );
    let pane = tmux_out(&env, &["new-session", "-d", "-s", "v", "-x", "80", "-y", "10", "-P", "-F", "#{pane_id}", &command])
        .trim()
        .to_owned();
    let flags = |env: &TestEnv| tmux_out(env, &["display-message", "-p", "-t", &pane, "#{mouse_any_flag} #{alternate_on}"]);
    wait_until("the view to own the terminal", || flags(&env).trim() == "1 1");
    std::fs::write(&go, "").unwrap();
    let mut text = String::new();
    wait_until(until, || {
        text = tmux_out(&env, &["capture-pane", "-p", "-t", &pane]);
        text.contains(until)
    });
    (text, flags(&env).trim().to_owned())
}

#[test]
fn a_panic_in_a_view_leaves_the_terminal_restored_and_the_mouse_off() {
    let (text, flags) = view_ending("panic", "test result: FAILED");
    assert_eq!(flags, "0 0", "mouse reporting and the alternate screen are off after a panic:\n{text}");
}

#[test]
fn an_error_in_a_view_leaves_the_terminal_restored_and_the_mouse_off() {
    let (text, flags) = view_ending("error", "the view failed");
    assert_eq!(flags, "0 0", "mouse reporting and the alternate screen are off after an error:\n{text}");
}
