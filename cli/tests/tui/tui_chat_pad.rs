//! The blank row at the bottom of the chat slot's area (contract §6.1 "The blank row", commands.md §13.2; hs-d561):
//! `status 2`, `status-format[0]` one space, and a 20-row client's window is 18 rows, so the chat slot's last content
//! row is row 18 (1-based), row 19 is blank and row 20 is the `─` line.

use crate::common;

use std::fs;

use common::{CPD, SLEEP_SECONDS, TestEnv, wait_until};

fn env() -> TestEnv {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
             [tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.fake_gh(r#"echo '{"data":{"search":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}'"#);
    env.fake(
        "cpd",
        &format!("if [ \"$1\" = tui ] && [ \"$2\" = view ] && [ \"$3\" != chat ]; then exec sleep {SLEEP_SECONDS}; fi\nexec '{CPD}' \"$@\""),
    );
    env
}

fn tmux_out(env: &TestEnv, args: &[&str]) -> String {
    let out = env.tmux(args);
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn the_chat_slot_ends_in_one_blank_row() {
    let env = env();
    env.cpd(&["tui"]).ok();

    // A real client of 20 rows, on a server of its own, killed at the end.
    let socket = format!("{}-pad", env.tmux_socket());
    let outer = |args: &[&str]| {
        let out = env.command("tmux").args(["-L", &socket, "-f", "/dev/null"]).args(args).output().unwrap();
        assert!(out.status.success(), "outer tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    };
    let attach = format!("env -u TMUX tmux -L {} attach -t cpd", env.tmux_socket());
    outer(&["set", "-g", "status", "off", ";", "new-session", "-d", "-s", "o", "-x", "215", "-y", "20", &attach]);
    wait_until("the client is attached", || !tmux_out(&env, &["list-clients", "-t", "=cpd"]).trim().is_empty());

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // The options: two status rows, row 0 one space, row 1 the line.
        assert_eq!(tmux_out(&env, &["show", "-v", "-t", "=cpd:", "status"]).trim(), "2");
        assert_eq!(tmux_out(&env, &["show", "-v", "-t", "=cpd:", "status-format[0]"]).trim_end_matches('\n'), " ");
        assert!(tmux_out(&env, &["show", "-v", "-t", "=cpd:", "status-format[1]"]).contains("\u{2500}"));

        // The geometry: the window is 18 rows; the chat slot's last content row is row 18 (1-based, 0-based 17) and
        // the border row above the panes takes row 1, so no pane reaches the blank row 19.
        let window = tmux_out(&env, &["display", "-p", "-t", "=cpd:main", "#{window_height}"]);
        assert_eq!(window.trim(), "18", "a 20-row client: the window is one row shorter for the blank row");
        let panes = tmux_out(&env, &["list-panes", "-t", "=cpd:main", "-F", "#{pane_top}\t#{pane_height}"]);
        let bottoms: Vec<u32> = panes
            .lines()
            .map(|l| {
                let (top, height) = l.split_once('\t').unwrap();
                top.parse::<u32>().unwrap() + height.parse::<u32>().unwrap()
            })
            .collect();
        assert!(!bottoms.is_empty() && bottoms.iter().all(|b| *b <= 18), "no pane ends below row 18 (1-based): {bottoms:?}");
        assert_eq!(bottoms.iter().max(), Some(&18), "the chat slot's last content row is row 18: {bottoms:?}");

        // The screen: row 19 is blank, row 20 is the `─` line.
        let screen = outer(&["capture-pane", "-p", "-t", "o"]);
        let rows: Vec<&str> = screen.lines().collect();
        assert_eq!(rows.get(18).map(|r| r.trim()), Some(""), "row 19 is blank: {screen}");
        assert!(rows.get(19).is_some_and(|r| r.contains('\u{2500}')), "row 20 is the ─ line: {screen}");
    }));
    let _ = env.command("tmux").args(["-L", &socket, "kill-server"]).stderr(std::process::Stdio::null()).status();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}
