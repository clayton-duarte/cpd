//! The one status row (contract §6.1, commands.md §13.2): `status on`, the line in `status-format[0]`, and a 20-row
//! client's window is 19 rows, so the panes reach row 19 and row 20 is the `─` line. The history view leaves the last
//! row of its pane empty (the chat slot's padding).

use crate::common;

use std::fs;

use serde_json::json;

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
fn the_status_line_is_one_row_and_the_panes_reach_the_row_above_it() {
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
        // The options: one status row, the line in row 0, no row 1.
        assert_eq!(tmux_out(&env, &["show", "-v", "-t", "=cpd:", "status"]).trim(), "on");
        assert!(tmux_out(&env, &["show", "-v", "-t", "=cpd:", "status-format[0]"]).contains("\u{2500}"));
        assert!(
            tmux_out(&env, &["show", "-v", "-t", "=cpd:", "status-format[1]"]).trim().is_empty(),
            "no status-format row sits between the panes and the line"
        );

        // The geometry: the window is 19 rows and the lowest pane ends on row 19 (1-based).
        let window = tmux_out(&env, &["display", "-p", "-t", "=cpd:main", "#{window_height}"]);
        assert_eq!(window.trim(), "19", "a 20-row client: the status line takes one row");
        let panes = tmux_out(&env, &["list-panes", "-t", "=cpd:main", "-F", "#{pane_top}\t#{pane_height}"]);
        let bottoms: Vec<u32> = panes
            .lines()
            .map(|l| {
                let (top, height) = l.split_once('\t').unwrap();
                top.parse::<u32>().unwrap() + height.parse::<u32>().unwrap()
            })
            .collect();
        assert_eq!(bottoms.iter().max(), Some(&19), "the lowest pane ends on row 19: {bottoms:?}");

        // The screen: row 20 is the `─` line.
        let screen = outer(&["capture-pane", "-p", "-t", "o"]);
        let rows: Vec<&str> = screen.lines().collect();
        assert!(rows.get(19).is_some_and(|r| r.contains('\u{2500}')), "row 20 is the ─ line: {screen}");
    }));
    let _ = env.command("tmux").args(["-L", &socket, "kill-server"]).stderr(std::process::Stdio::null()).status();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

#[test]
fn the_history_view_leaves_the_last_row_of_its_pane_empty() {
    let env = TestEnv::new();
    fs::write(env.cpd_home().join("config.toml"), "version = 1\nmachine = \"m1\"\n").unwrap();
    let path = env.root().join("lead.jsonl");
    let mut text = json!({"type": "session", "id": "l1"}).to_string() + "\n";
    for n in 0..40 {
        text += &json!({"type": "message", "timestamp": "2026-10-15T16:33:44.329Z",
            "message": {"role": "user", "content": [{"type": "text", "text": format!("row {n}")}]}})
        .to_string();
        text.push('\n');
    }
    fs::write(&path, text).unwrap();
    env.sql(&format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on)
         VALUES ('alpha', 'Title', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at,
                               created_at, archived_at)
         VALUES ('l1', 'lead', 'alpha', NULL, NULL, 'old lead', '{}', '/repo', 'O3', NULL, 'stopped', NULL,
                 '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z', '2026-01-04T00:00:00.000Z');",
        path.display()
    ));
    let out = env.tmux(&[
        "-u", "set-option", "-g", "remain-on-exit", "on", ";", "new-session", "-d", "-s", "history", "-x", "40", "-y", "12",
        &format!("{CPD} tui view history l1"),
    ]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let rows = || -> Vec<String> {
        let out = env.tmux(&["capture-pane", "-p", "-t", "history"]);
        String::from_utf8(out.stdout).unwrap().lines().map(|l| l.trim_end().to_owned()).collect()
    };
    let check = |rows: &[String]| {
        assert_eq!(rows.len(), 12, "{rows:?}");
        assert_eq!(rows[11], "", "row 12 is empty: {rows:?}");
        assert!(!rows[10].is_empty(), "the content fills rows 1..11: {rows:?}");
    };
    wait_until("the end of the conversation", || rows().iter().any(|r| r.contains("row 39")));
    check(&rows());
    assert!(env.tmux(&["send-keys", "-t", "history", "G"]).status.success());
    wait_until("the end again", || rows().iter().any(|r| r.contains("row 39")));
    check(&rows());
}
