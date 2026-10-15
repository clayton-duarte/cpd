//! The cockpit, `cpd2 tui` (commands.md §13.1; contract §6.1, §6.2 with
//! commands.md §19 amendments 1, 2 and 9) and the routing of `cpd2 tui view`
//! (§13.2), on a private tmux server with a fake `cpd2` on `PATH` whose
//! not-yet-implemented views just sleep.

mod common;

use std::fs;
use std::time::{Duration, Instant};

use common::{CPD2, TestEnv};
use cpd2::error::CpdError;
use cpd2::home::CpdHome;
use cpd2::store::Store;
use serde_json::{Value, json};

const PLACEHOLDER: &str = "No Lead here: pick a Plan in the session tree";

/// An env with a config on its private tmux server, and a fake `cpd2` that
/// runs the real one except for the views whose Jobs have not landed, which
/// sleep so that their panes stay alive.
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
    env.fake(
        "cpd2",
        &format!("if [ \"$1\" = tui ] && [ \"$2\" = view ] && [ \"$3\" != chat ]; then exec sleep 3600; fi\nexec '{CPD2}' \"$@\""),
    );
    env
}

fn tmux_out(env: &TestEnv, args: &[&str]) -> String {
    let out = env.tmux(args);
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// Waits until `done` holds, at most 30 s.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// One pane of `cpd:main`.
#[derive(Debug, Clone, PartialEq)]
struct Pane {
    id: String,
    view: String,
    session: String,
    dead: bool,
    pid: u32,
    left: u32,
    top: u32,
    width: u32,
    height: u32,
}

fn main_panes(env: &TestEnv) -> Vec<Pane> {
    let format = "#{pane_id}\t#{@cpd_view}\t#{@cpd_session}\t#{pane_dead}\t#{pane_pid}\t#{pane_left}\t#{pane_top}\t#{pane_width}\t#{pane_height}";
    tmux_out(env, &["list-panes", "-t", "=cpd:main", "-F", format])
        .lines()
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            Pane {
                id: f[0].into(),
                view: f[1].into(),
                session: f[2].into(),
                dead: f[3] == "1",
                pid: f[4].parse().unwrap(),
                left: f[5].parse().unwrap(),
                top: f[6].parse().unwrap(),
                width: f[7].parse().unwrap(),
                height: f[8].parse().unwrap(),
            }
        })
        .collect()
}

fn view<'a>(panes: &'a [Pane], name: &str) -> &'a Pane {
    panes.iter().find(|p| p.view == name).unwrap_or_else(|| panic!("no {name} pane: {panes:?}"))
}

fn windows(env: &TestEnv, session: &str) -> Vec<String> {
    tmux_out(env, &["list-windows", "-t", &format!("={session}"), "-F", "#{window_name}"]).lines().map(str::to_owned).collect()
}

fn views_started(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("cpd2").into_iter().filter(|c| c.first().map(String::as_str) == Some("tui")).collect()
}

/// Asserts `main` holds the layout of §13.1: the tree left, the plan view
/// above the chat slot in the centre, the widgets right, the one-row StatusBar
/// across the bottom, each marked.
fn assert_layout(panes: &[Pane]) {
    assert_eq!(panes.len(), 5, "{panes:?}");
    let (tree, plan, widgets) = (view(panes, "tree"), view(panes, "plan"), view(panes, "widgets"));
    let bar = view(panes, "statusbar");
    assert_eq!((bar.left, bar.width, bar.height), (0, 215, 1), "{panes:?}");
    assert!(panes.iter().filter(|p| p.id != bar.id).all(|p| p.top < bar.top), "{panes:?}");
    let slot = panes.iter().find(|p| p.view == "chat" || !p.session.is_empty()).expect("a chat slot");
    assert_eq!(tree.left, 0, "{panes:?}");
    assert_eq!(plan.left, slot.left, "{panes:?}");
    assert!(plan.left > tree.left && widgets.left > plan.left, "{panes:?}");
    assert!(slot.top > plan.top, "{panes:?}");
    // 20 % of the 215 columns each, give or take tmux's rounding and borders.
    for side in [tree, widgets] {
        assert!((40..=46).contains(&side.width), "{panes:?}");
    }
    assert!(panes.iter().all(|p| !p.dead), "{panes:?}");
}

#[test]
fn tui_builds_the_cockpit_once_and_starts_the_engine() {
    let env = env();
    let out = env.cpd2(&["tui"]).ok();
    let socket = env.tmux_socket();
    assert_eq!(out["built"], true, "{out}");
    assert_eq!(out["socket"], socket);
    assert_eq!(out["session"], "cpd");
    assert_eq!(out["attach"], json!(["tmux", "-L", socket, "attach", "-t", "cpd"]));
    assert_eq!(out["watch"]["started"], true, "{out}");
    let engine = out["watch"]["pane"].as_str().unwrap();

    let panes = main_panes(&env);
    assert_layout(&panes);
    let chat = view(&panes, "chat");
    assert_eq!(windows(&env, "cpd"), ["main"]);
    assert_eq!(windows(&env, "cpd-work"), ["watch"]);
    let views: Vec<Vec<String>> = ["tree", "plan", "widgets", "chat", "statusbar"].iter().map(|v| vec!["tui".into(), "view".into(), (*v).into()]).collect();
    assert_eq!(views_started(&env), views);
    // The keyboard goes to the chat slot.
    assert_eq!(tmux_out(&env, &["display", "-p", "-t", "=cpd:main", "#{pane_id}"]).trim(), chat.id);

    // The real placeholder view draws its text in the slot.
    wait_until("the placeholder text", || {
        String::from_utf8_lossy(&env.tmux(&["capture-pane", "-p", "-t", &chat.id]).stdout).contains(PLACEHOLDER)
    });

    // The server shows §6.1's options, hyperlinks included, and the TUI's border titles.
    let features = tmux_out(&env, &["show", "-s", "terminal-features"]);
    for feature in ["xterm*:extkeys", "xterm-ghostty:extkeys", "xterm-ghostty:hyperlinks"] {
        assert!(features.contains(feature), "{feature} not in {features}");
    }
    assert_eq!(tmux_out(&env, &["show", "-gv", "remain-on-exit"]).trim(), "on");
    let border = tmux_out(&env, &["show", "-gv", "pane-border-format"]);
    assert!(border.contains("#{==:#{@cpd_view},tree}, Context "), "{border}");
    // The StatusBar has no title, and a click on it never selects it.
    assert!(border.contains("#{?#{==:#{@cpd_view},statusbar},,"), "{border}");
    let binding = tmux_out(&env, &["list-keys", "-T", "root", "MouseDown1Pane"]);
    assert!(binding.contains("#{==:#{@cpd_view},statusbar}"), "{binding}");
    assert!(binding.contains("send-keys -M"), "{binding}");

    // A second run builds nothing and starts nothing.
    let again = env.cpd2(&["tui"]).ok();
    assert_eq!(again["built"], false, "{again}");
    assert_eq!(again["watch"], json!({"started": false, "already": true}));
    assert_eq!(main_panes(&env).iter().map(|p| &p.id).collect::<Vec<_>>(), panes.iter().map(|p| &p.id).collect::<Vec<_>>());
    assert_eq!(views_started(&env).len(), 5);
    assert_eq!(windows(&env, "cpd-work"), ["watch"]);
    assert!(tmux_out(&env, &["list-panes", "-t", engine, "-F", "#{pane_dead}"]).trim() == "0");
}

#[test]
fn tui_builds_main_in_a_cockpit_that_has_only_a_park_window() {
    let env = env();
    // As plan new leaves it: the cockpit session with the first Lead's park window.
    tmux_out(&env, &["new-session", "-d", "-s", "cpd", "-n", "park-pay", "-x", "215", "-y", "62", "sleep", "3600"]);
    let out = env.cpd2(&["tui"]).ok();
    assert_eq!(out["built"], true, "{out}");
    assert_layout(&main_panes(&env));
    assert_eq!(windows(&env, "cpd"), ["park-pay", "main"]);
    assert_eq!(env.cpd2(&["tui"]).ok()["built"], false);
}

#[test]
fn tui_respawns_a_dead_pane_of_the_layout_in_place() {
    let env = env();
    env.cpd2(&["tui"]).ok();
    let before = main_panes(&env);
    let tree = view(&before, "tree").clone();
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(tree.pid as i32), nix::sys::signal::Signal::SIGKILL).unwrap();
    wait_until("the dead tree pane", || view(&main_panes(&env), "tree").dead);

    let out = env.cpd2(&["tui"]).ok();
    assert_eq!(out["built"], false, "{out}");
    let after = main_panes(&env);
    assert_layout(&after);
    assert_eq!(after.iter().map(|p| &p.id).collect::<Vec<_>>(), before.iter().map(|p| &p.id).collect::<Vec<_>>());
    assert_ne!(view(&after, "tree").pid, tree.pid);
    assert_eq!(views_started(&env).iter().filter(|c| c[2] == "tree").count(), 2);
}

#[test]
fn tui_respawns_a_dead_statusbar_in_place() {
    let env = env();
    env.cpd2(&["tui"]).ok();
    let before = main_panes(&env);
    let bar = view(&before, "statusbar").clone();
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(bar.pid as i32), nix::sys::signal::Signal::SIGKILL).unwrap();
    wait_until("the dead statusbar pane", || view(&main_panes(&env), "statusbar").dead);

    env.cpd2(&["tui"]).ok();
    let after = main_panes(&env);
    assert_layout(&after);
    assert_eq!(view(&after, "statusbar").id, bar.id);
    assert_ne!(view(&after, "statusbar").pid, bar.pid);
    assert_eq!(after.len(), before.len());
}

#[test]
fn tui_adds_a_missing_statusbar_to_an_existing_main_once() {
    let env = env();
    env.cpd2(&["tui"]).ok();
    let full = main_panes(&env);
    tmux_out(&env, &["kill-pane", "-t", &view(&full, "statusbar").id]);
    let before = main_panes(&env);
    assert_eq!(before.len(), 4, "{before:?}");

    let out = env.cpd2(&["tui"]).ok();
    assert_eq!(out["built"], false, "{out}");
    let after = main_panes(&env);
    assert_eq!(after.len(), 5, "{after:?}");
    let bar = view(&after, "statusbar");
    assert_eq!((bar.left, bar.width, bar.height), (0, 215, 1), "{after:?}");
    // The other panes are the same panes, still alive; the layout was not reset.
    for old in &before {
        let new = after.iter().find(|p| p.id == old.id).unwrap_or_else(|| panic!("{} is gone: {after:?}", old.id));
        assert_eq!((new.view.as_str(), new.pid, new.dead, new.left, new.width), (old.view.as_str(), old.pid, false, old.left, old.width));
    }

    // Rerunning adds none.
    env.cpd2(&["tui"]).ok();
    assert_eq!(main_panes(&env).len(), 5);
}

#[test]
fn tui_brings_the_last_chat_lead_into_a_new_slot() {
    let env = env();
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    // lead-b was in the chat last among Leads with a Plan; ground-c, a
    // Context's grounded Lead, came later but is never restored there.
    let sql = "INSERT INTO contexts (id, title, created_at, created_on, lead_session, grounded_seq) VALUES ('ctx', 'Ctx', 't', 'm1', 'ground-c', 1);
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at) VALUES
           ('lead-a', 'lead', 'ctx', 'pay', 'A', '/', 'O3', 'stopped', 't', 't'),
           ('lead-b', 'lead', 'ctx', 'pay', 'B', '/', 'O3', 'stopped', 't', 't'),
           ('ground-c', 'lead', 'ctx', NULL, 'C', '/', 'O3', 'stopped', 't', 't');
         INSERT INTO plans (id, context, title, repo, base, lead_branch, phase, lead_session, created_at)
           VALUES ('pay', 'ctx', 'Pay', '/', 'main', 'cpd/pay', 'grounding', 'lead-a', 't');
         INSERT INTO events (txn, at, kind, context, plan, session, text, data) VALUES
           (cpd_txn(), 't', 'session-moved', 'ctx', 'pay', 'lead-a', 'x', '{\"place\": \"chat\"}'),
           (cpd_txn(), 't', 'session-moved', 'ctx', 'pay', 'lead-b', 'x', '{\"place\": \"chat\"}'),
           (cpd_txn(), 't', 'session-moved', 'ctx', 'pay', 'lead-b', 'x', '{\"place\": \"parked\"}'),
           (cpd_txn(), 't', 'session-moved', 'ctx', NULL, 'ground-c', 'x', '{\"place\": \"chat\"}');";
    Store::open(&home, 5000)
        .unwrap()
        .write("fixture", |tx| {
            tx.execute_batch(sql)?;
            Ok::<_, CpdError>(())
        })
        .unwrap();
    tmux_out(&env, &["new-session", "-d", "-s", "cpd", "-n", "park-pay", "-x", "215", "-y", "62", "sleep", "3600"]);
    let lead_pane = tmux_out(&env, &["display", "-p", "-t", "=cpd:park-pay", "#{pane_id}"]).trim().to_owned();
    tmux_out(&env, &["set", "-p", "-t", &lead_pane, "@cpd_session", "lead-b"]);

    let out = env.cpd2(&["tui"]).ok();
    assert_eq!(out["built"], true, "{out}");
    let panes = main_panes(&env);
    assert_layout(&panes);
    // The Lead's own pane took the slot; the placeholder went, with its park window.
    let slot = panes.iter().find(|p| p.session == "lead-b").unwrap_or_else(|| panic!("{panes:?}"));
    assert_eq!(slot.id, lead_pane);
    assert!(panes.iter().all(|p| p.view != "chat"), "{panes:?}");
    assert_eq!(windows(&env, "cpd"), ["main"]);
    let moved: Value = Store::open(&home, 5000)
        .unwrap()
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT json_object('session', session, 'data', json(data)) FROM events WHERE kind = 'session-moved' ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get::<_, String>(0),
            )?)
        })
        .map(|s| serde_json::from_str(&s).unwrap())
        .unwrap();
    assert_eq!(moved["session"], "lead-b");
    assert_eq!(moved["data"]["place"], "chat");
    assert_eq!(moved["data"]["pane"], lead_pane);
}

#[test]
fn tui_attach_without_a_terminal_is_refused_before_anything_is_built() {
    let env = env();
    let run = env.cpd2(&["tui", "--attach"]);
    assert_eq!(run.exit, 3, "{run:?}");
    assert_eq!(run.error()["code"], "not_a_terminal");
    assert!(!env.tmux(&["list-sessions"]).status.success(), "a tmux server was started");
    assert!(env.calls("cpd2").is_empty());
}

#[test]
fn tui_view_routes_by_name() {
    let env = env();
    let run = env.cpd2(&["tui", "view", "chat"]);
    assert_eq!((run.exit, run.error()["code"].clone()), (3, json!("not_a_terminal")), "{run:?}");
    for words in [&["tui", "view", "nope"][..], &["tui", "view"], &["tui", "--bogus"]] {
        let run = env.cpd2(words);
        assert_eq!((run.exit, run.error()["code"].clone()), (2, json!("usage")), "{words:?}: {run:?}");
    }
}
