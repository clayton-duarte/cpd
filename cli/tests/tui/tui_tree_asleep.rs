//! Asleep Sessions in `cpd tui view tree` (commands.md §13.2; contract §2.2 "Asleep"; hs-d515, hs-d520), end to end: the
//! built binary runs in a pane of a private tmux server over a prepared store, with a fake `cpd` first on `PATH` that
//! records its command line (as `tui_tree.rs` does; its helpers are private, so the few needed are repeated here).

use crate::common;

use common::{CPD, TestEnv, sql};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use nix::fcntl::{Flock, FlockArg};

const SESSION: &str = "tree";
const WHEN: &str = "2026-02-01T00:00:00.000Z";

/// An env with the config `cpd` needs and an engine lock held (no `engine-down` item), and the fake `cpd`.
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
    env.fake("cpd", r#"echo '{"ok":true,"session":"x","pane":"%1","displaced":null}'"#);
    env
}

fn fork(id: &str, extra_column: &str, extra_value: &str, created: &str) -> String {
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at{extra_column})
         VALUES ('{id}', 'lead', 'alpha', 'ship', NULL, 'name of {id}', '/sessions/{id}.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                 '2026-01-03T00:00:00.000Z', '{created}'{extra_value});"
    )
}

/// Context `alpha` with Plan `ship` (its Lead `l-ship`, idle) and three forks of it: `f-slp` (asleep, the oldest), `f-live` and
/// `f-arc` (archived), all idle. `asleep_at` is the column the Job's migration adds.
fn populate(env: &TestEnv) {
    let mut store = Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap();
    let mut s = String::from(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('alpha', 'Title of alpha', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES ('ship', 'alpha', 'Plan ship', 'grounding', 'l-ship', '2026-01-02T00:00:00.000Z');",
    );
    s += &fork("l-ship", "", "", "2026-01-02T00:00:00.000Z");
    s += &fork("f-slp", ", asleep_at", &format!(", '{WHEN}'"), "2026-01-02T00:00:01.000Z");
    s += &fork("f-live", "", "", "2026-01-02T00:00:02.000Z");
    s += &fork("f-arc", ", archived_at", &format!(", '{WHEN}'"), "2026-01-02T00:00:03.000Z");
    sql(&mut store, &s);
}

struct Pane<'a> {
    env: &'a TestEnv,
}

impl<'a> Pane<'a> {
    fn start(env: &'a TestEnv) -> Pane<'a> {
        let out = env.tmux(&[
            "-u", "set-option", "-g", "remain-on-exit", "on", ";", "new-session", "-d", "-s", SESSION, "-x", "60", "-y", "20",
            &format!("{CPD} tui view tree"),
        ]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        Pane { env }
    }

    fn capture(&self, escapes: bool) -> Vec<String> {
        let mut args = vec!["capture-pane", "-p"];
        if escapes {
            args.push("-e");
        }
        args.extend(["-t", SESSION]);
        let out = self.env.tmux(&args);
        String::from_utf8(out.stdout).unwrap().lines().map(|l| l.trim_end().to_owned()).collect()
    }

    /// Waits until the rows of the three forks are drawn; returns the plain screen.
    fn ready(&self) -> Vec<String> {
        common::wait_screen("the three forks", || self.capture(false), |s| ["f-sl", "f-li", "f-ar"].iter().all(|f| s.iter().any(|r| r.contains(f))))
    }

    fn click(&self, column: u16, row: u16) {
        for end in ['M', 'm'] {
            let text = format!("\x1b[<0;{};{}{end}", column + 1, row + 1);
            assert!(self.env.tmux(&["send-keys", "-t", SESSION, "-l", &text]).status.success());
        }
    }
}

/// The index of the row of the fork labelled `ship·<suffix>`.
fn row_of(screen: &[String], suffix: &str) -> usize {
    screen.iter().position(|r| r.contains(&format!("ship·{suffix}"))).unwrap_or_else(|| panic!("no row of {suffix}: {screen:?}"))
}

/// The colour escapes (`ESC [ … m`) of a captured row, in order.
fn escapes(row: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = row;
    while let Some(start) = rest.find("\x1b[") {
        let tail = &rest[start..];
        let end = tail.find('m').expect("an escape ends with m");
        found.push(tail[..=end].to_owned());
        rest = &tail[end + 1..];
    }
    found
}

fn wait_for_calls(env: &TestEnv, n: usize) -> Vec<Vec<String>> {
    common::wait_some(&format!("{n} calls of cpd"), common::WAIT, || {
        let calls = env.calls("cpd");
        if calls.len() >= n { Ok(calls) } else { Err(format!("got {calls:?}")) }
    })
}

fn words(w: &[&str]) -> Vec<String> {
    w.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn asleep_row_is_muted_among_live_rows() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env);
    let plain = pane.ready();
    let (asleep, live, archived) = (row_of(&plain, "f-sl"), row_of(&plain, "f-li"), row_of(&plain, "f-ar"));
    assert!(asleep < live && live < archived, "the asleep row keeps its place among the live rows, the archived one is after them: {plain:?}");

    let styled = pane.capture(true);
    let (asleep, live, archived) = (escapes(&styled[asleep]), escapes(&styled[live]), escapes(&styled[archived]));
    assert!(!archived.is_empty(), "the archived row is coloured: {archived:?}");
    assert_eq!(asleep, archived, "an asleep row is drawn as an archived one: muted, grey glyph");
    assert_ne!(asleep, live, "and not as a live idle one");
}

#[test]
fn click_on_asleep_row_resumes_it_in_the_chat_slot() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env);
    let row = row_of(&pane.ready(), "f-sl");
    pane.click(8, row as u16);
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "open", "f-slp"])]);
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(env.calls("cpd"), [words(&["session", "open", "f-slp"])], "one action: no unarchive, no history");
}

#[test]
fn click_on_archived_session_unarchives_and_resumes_it() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env);
    let row = row_of(&pane.ready(), "f-ar");
    pane.click(8, row as u16);
    assert_eq!(
        wait_for_calls(&env, 2),
        [words(&["session", "unarchive", "f-arc"]), words(&["session", "open", "f-arc"])],
        "the archived Session is unarchived, then opened: not `session history`"
    );
}
