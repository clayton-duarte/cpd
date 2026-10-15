//! `cpd2 tui view tree` (commands.md §13.2, §17), end to end: the built binary
//! runs in a pane of a private tmux server (its terminal), over a prepared
//! store; a test reads the screen (`capture-pane`), presses keys (`send-keys`)
//! and, for the actions, finds a fake `cpd2` first on `PATH` that records its
//! command line. The rendering of single rows, colours and keys is unit-tested
//! in `cli/src/tui/tree/`.

mod common;

use std::time::{Duration, Instant};

use common::{CPD2, TestEnv};
use cpd2::error::CpdError;
use cpd2::home::CpdHome;
use cpd2::store::Store;

// ---------- the prepared store ----------

/// An env with the config `cpd2` needs.
fn env() -> TestEnv {
    let env = TestEnv::new();
    std::fs::write(env.cpd_home().join("config.toml"), "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n").unwrap();
    env
}

fn open(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

fn sql(store: &mut Store, text: &str) {
    store
        .write("fixture", |tx| {
            tx.execute_batch(text)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

fn context(id: &str, grounded: Option<&str>) -> String {
    let (lead, seq) = grounded.map_or(("NULL".to_owned(), "NULL"), |g| (format!("'{g}'"), "0"));
    format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on, lead_session, grounded_seq)
         VALUES ('{id}', 'Title of {id}', NULL, '2026-01-01T00:00:00.000Z', 'm1', {lead}, {seq});"
    )
}

/// `pr`: the Plan has a PR (the schema ties phases `pr` and `done` to one).
fn plan_row(id: &str, context: &str, phase: &str, lead: &str, created: &str, pr: bool) -> String {
    let late = !matches!(phase, "grounding" | "plan");
    let worktree = if late { "'/wt'" } else { "NULL" };
    let locked = if late { "'2026-01-02T00:00:00.000Z'" } else { "NULL" };
    let done = if phase == "done" { "'2026-01-09T00:00:00.000Z'" } else { "NULL" };
    let (number, url, opened, polled) = if pr {
        ("7", "'https://example.test/pull/7'", "'2026-01-05T00:00:00.000Z'", "'2026-01-06T00:00:00.000Z'")
    } else {
        ("NULL", "NULL", "NULL", "NULL")
    };
    format!(
        "INSERT INTO plans (id, context, title, repo, base, lead_branch, lead_worktree, phase, lead_session, pr_number,
                            pr_url, pr_opened_at, pr_seen, pr_polled_at, created_at, locked_at, done_at)
         VALUES ('{id}', '{context}', 'Plan {id}', '/repo/{id}', 'main', 'cpd/{id}', {worktree}, '{phase}', '{lead}',
                 {number}, {url}, {opened}, '{{\"checks\":{{}},\"reviewIds\":[],\"commentIds\":[]}}', {polled},
                 '{created}', {locked}, {done});"
    )
}

fn session_row(id: &str, name: &str, context: &str, plan: &str, state: &str, label: Option<&str>, created: &str) -> String {
    let plan = if plan.is_empty() { "NULL".to_owned() } else { format!("'{plan}'") };
    let label = label.map_or("NULL".to_owned(), |l| format!("'{l}'"));
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('{id}', 'lead', '{context}', {plan}, NULL, '{name}', '/sessions/{id}.jsonl', '/repo', 'O3',
                 NULL, '{state}', {label}, '2026-01-03T00:00:00.000Z', '{created}');"
    )
}

fn job_row(plan: &str, key: &str, status: &str) -> String {
    let fingerprint = if status == "draft" { "NULL".to_owned() } else { format!("'{}'", "0".repeat(64)) };
    let merged = if status == "merged" { "'abc123'" } else { "NULL" };
    let failed = if status == "failed" { "'crashed'" } else { "NULL" };
    format!(
        "INSERT INTO jobs (plan, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, cancel_reason, cancel_requested_at, failed_cause, merged_sha, created_at)
         VALUES ('{plan}', '{key}', 'Job {key}', 'S3', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, {fingerprint},
                 cpd_txn(), NULL, NULL, {failed}, {merged}, '2026-01-02T00:00:00.000Z');"
    )
}

fn moved(context: &str, plan: &str, session: &str, place: &str) -> String {
    format!(
        "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-01-03T00:00:00.000Z', 'session-moved', '{context}', '{plan}', '{session}', 'fixture',
                 '{{\"pane\":\"%1\",\"window\":\"@1\",\"place\":\"{place}\"}}');"
    )
}

/// Contexts, Plans and Sessions in each state:
///
/// - `alpha` (grounded Lead `g-alpha`): `ship` (dispatch; Lead `l-ship` working, in the chat slot; side forks
///   `f-idle` and `f-blocked`; Jobs queued, running, failed and two merged), `sketch` (plan; Lead idle; two drafts);
/// - `beta`: `rel` (pr; its Lead is blocked), `old` (done; its Lead is stopped);
/// - `gamma`: nothing.
fn populate(env: &TestEnv) {
    let mut store = open(env);
    let mut s = String::new();
    s += &context("alpha", Some("g-alpha"));
    s += &context("beta", None);
    s += &context("gamma", None);
    s += &plan_row("ship", "alpha", "dispatch", "l-ship", "2026-01-02T00:00:00.000Z", false);
    s += &plan_row("sketch", "alpha", "plan", "l-sketch", "2026-01-02T00:00:01.000Z", false);
    s += &plan_row("rel", "beta", "pr", "l-rel", "2026-01-02T00:00:02.000Z", true);
    s += &plan_row("old", "beta", "done", "l-old", "2026-01-02T00:00:03.000Z", true);
    s += &session_row("g-alpha", "grounded", "alpha", "", "idle", None, "2026-01-02T00:00:00.000Z");
    s += &session_row("l-ship", "ship lead", "alpha", "ship", "working", None, "2026-01-02T00:00:00.000Z");
    s += &session_row("f-idle", "ship fork", "alpha", "ship", "idle", None, "2026-01-02T00:00:08.000Z");
    s += &session_row("f-blocked", "ship review", "alpha", "ship", "blocked", Some("cpd: Worker question"), "2026-01-02T00:00:09.000Z");
    s += &session_row("l-sketch", "sketch", "alpha", "sketch", "idle", None, "2026-01-02T00:00:01.000Z");
    s += &session_row("l-rel", "rel lead", "beta", "rel", "blocked", Some("cpd: PR feedback"), "2026-01-02T00:00:02.000Z");
    s += &session_row("l-old", "old lead", "beta", "old", "stopped", None, "2026-01-02T00:00:03.000Z");
    for (key, status) in [("a", "merged"), ("b", "merged"), ("c", "running"), ("d", "queued"), ("e", "failed")] {
        s += &job_row("ship", key, status);
    }
    for key in ["s1", "s2"] {
        s += &job_row("sketch", key, "draft");
    }
    s += &job_row("rel", "r1", "merged");
    s += &moved("alpha", "ship", "l-ship", "chat");
    sql(&mut store, &s);
}

// ---------- the pane ----------

const SESSION: &str = "tree";

struct Pane<'a> {
    env: &'a TestEnv,
}

impl<'a> Pane<'a> {
    /// Runs `cpd2 tui view tree` in a `width` × `height` pane of the env's private tmux server.
    fn start(env: &'a TestEnv, width: u16, height: u16) -> Pane<'a> {
        let out = env.tmux(&[
            "-u",
            "set-option",
            "-g",
            "remain-on-exit",
            "on",
            ";",
            "new-session",
            "-d",
            "-s",
            SESSION,
            "-x",
            &width.to_string(),
            "-y",
            &height.to_string(),
            &format!("{CPD2} tui view tree"),
        ]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        Pane { env }
    }

    /// The screen, each row without trailing blanks, without the empty rows at the end.
    fn screen(&self) -> Vec<String> {
        let out = self.env.tmux(&["capture-pane", "-p", "-t", SESSION]);
        let text = String::from_utf8(out.stdout).unwrap();
        let mut rows: Vec<String> = text.lines().map(|l| l.trim_end().to_owned()).collect();
        while rows.last().is_some_and(String::is_empty) {
            rows.pop();
        }
        rows
    }

    /// Waits until `done` holds of the screen; panics with the last screen.
    fn wait(&self, what: &str, done: impl Fn(&[String]) -> bool) -> Vec<String> {
        let start = Instant::now();
        loop {
            let screen = self.screen();
            if done(&screen) {
                return screen;
            }
            assert!(start.elapsed() < Duration::from_secs(10), "waiting for {what}; the screen is:\n{}", screen.join("\n"));
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits until the tree has been drawn.
    fn ready(&self) -> Vec<String> {
        self.wait("the tree", |s| s.iter().any(|r| r.contains("alpha")))
    }

    fn keys(&self, keys: &[&str]) {
        let mut args = vec!["send-keys", "-t", SESSION];
        args.extend(keys);
        assert!(self.env.tmux(&args).status.success());
    }

    /// Types `text` literally.
    fn type_text(&self, text: &str) {
        assert!(self.env.tmux(&["send-keys", "-t", SESSION, "-l", text]).status.success());
    }

    /// Moves the cursor down `n` rows (the first key shows it).
    fn down(&self, n: usize) {
        for _ in 0..n {
            self.keys(&["j"]);
        }
    }

    fn dead(&self) -> bool {
        let out = self.env.tmux(&["list-panes", "-t", SESSION, "-F", "#{pane_dead}"]);
        String::from_utf8_lossy(&out.stdout).trim() == "1"
    }

    fn footer_has(&self, text: &str) -> bool {
        self.screen().iter().any(|r| r.contains(text))
    }
}

/// A fake `cpd2` that succeeds with `reply`.
fn fake_cpd2(env: &TestEnv, reply: &str) {
    env.fake("cpd2", &format!("echo '{reply}'"));
}

fn calls(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("cpd2")
}

fn wait_for_calls(env: &TestEnv, n: usize) -> Vec<Vec<String>> {
    let start = Instant::now();
    loop {
        let calls = calls(env);
        if calls.len() >= n {
            return calls;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "waiting for {n} calls of cpd2; got {calls:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn words(w: &[&str]) -> Vec<String> {
    w.iter().map(|s| (*s).to_owned()).collect()
}

// ---------- tests ----------

#[test]
fn the_tree_of_a_prepared_store() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env, 46, 14);
    let screen = pane.ready();
    let row = |left: &str, right: &str| format!("{left:<w$}{right}", w = 46 - right.chars().count());
    assert_eq!(
        screen,
        [
            "● alpha".to_owned(),
            row("⏵ ├ ● ship lead working", "1⋅1⋅0⋅1⋅2"),
            "  │ ├ ○ ship fork".to_owned(),
            "  │ └ ● ship review cpd: Worker question".to_owned(),
            row("  └ ○ sketch", "2⋅0⋅0⋅0⋅0"),
            "● beta".to_owned(),
            row("  ├ ● rel lead cpd: PR feedback", "0⋅0⋅0⋅0⋅1"),
            row("  └ ○ old lead stopped", "0⋅0⋅0⋅0⋅0"),
            "○ gamma".to_owned(),
        ],
        "the grounded Lead g-alpha is no row; the Jobs: queued, running, merging, failed, merged"
    );
}

#[test]
fn an_empty_store_says_so() {
    let env = env();
    drop(open(&env));
    let pane = Pane::start(&env, 30, 5);
    assert_eq!(pane.wait("the empty tree", |s| !s.is_empty()), [" no Contexts"]);
}

#[test]
fn the_view_needs_a_terminal() {
    let env = env();
    let run = env.cpd2(&["tui", "view", "tree"]);
    assert_eq!(run.exit, 3, "{run:?}");
    let error = run.error();
    assert_eq!(error["code"], "not_a_terminal");
    assert_eq!(error["details"]["command"], "tui view tree");
}

#[test]
fn enter_opens_the_lead_of_a_plan_and_a_fork_with_session_open() {
    let env = env();
    populate(&env);
    fake_cpd2(&env, r#"{"ok":true,"session":"x","pane":"%1","displaced":null}"#);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    pane.down(3); // the cursor appears on alpha, then ship, then f-idle
    pane.keys(&["Enter"]);
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "open", "f-idle"])]);
    pane.wait("the note", |_| pane.footer_has("opened"));
    pane.keys(&["k", "Enter"]);
    assert_eq!(wait_for_calls(&env, 2)[1], words(&["session", "open", "l-ship"]));
}

#[test]
fn enter_before_any_arrow_key_does_nothing() {
    let env = env();
    populate(&env);
    fake_cpd2(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    pane.keys(&["Enter", "p", "n"]);
    std::thread::sleep(Duration::from_millis(500));
    assert!(calls(&env).is_empty());
    assert!(!pane.footer_has("New Plan"));
}

#[test]
fn p_parks_the_session_with_session_park() {
    let env = env();
    populate(&env);
    fake_cpd2(&env, r#"{"ok":true,"session":"l-ship","pane":"%1","window":"@3"}"#);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    pane.down(2);
    pane.keys(&["p"]);
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "park", "l-ship"])]);
    pane.wait("the note", |_| pane.footer_has("parked"));
}

#[test]
fn f_forks_the_lead_with_session_fork() {
    let env = env();
    populate(&env);
    fake_cpd2(&env, r#"{"ok":true,"session":{"id":"a41b","name":"my fork","plan":"ship"}}"#);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    pane.down(2);
    pane.keys(&["f"]);
    pane.wait("the prompt", |_| pane.footer_has("Fork ship lead, name:"));
    pane.type_text("my fork");
    pane.keys(&["Enter"]);
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "fork", "l-ship", "--name=my fork"])]);
    pane.wait("the note", |_| pane.footer_has("forked as my fork"));
    // an empty name keeps the command's default
    pane.keys(&["f", "Enter"]);
    assert_eq!(wait_for_calls(&env, 2)[1], words(&["session", "fork", "l-ship"]));
}

#[test]
fn r_renames_the_session_with_session_rename() {
    let env = env();
    populate(&env);
    fake_cpd2(&env, r#"{"ok":true,"session":"l-sketch","name":"x"}"#);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    pane.down(5); // alpha, ship, f-idle, f-blocked, sketch
    pane.keys(&["r"]);
    pane.wait("the prompt", |_| pane.footer_has("Rename sketch to: sketch"));
    pane.keys(&["BSpace", "BSpace", "BSpace", "BSpace", "BSpace", "BSpace"]);
    pane.type_text("draft");
    pane.keys(&["Enter"]);
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "rename", "--session", "l-sketch", "--", "draft"])]);
}

#[test]
fn n_starts_a_plan_with_plan_new() {
    let env = env();
    populate(&env);
    fake_cpd2(&env, r#"{"ok":true,"plan":{"id":"pay-4521"}}"#);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.down(5); // alpha, ship, f-idle, f-blocked, sketch
    pane.keys(&["n"]);
    pane.wait("the id prompt", |_| pane.footer_has("New Plan in alpha, id:"));
    pane.type_text("pay-4521");
    pane.keys(&["Enter"]);
    pane.wait("the title prompt", |_| pane.footer_has("title:"));
    pane.type_text("Retry failed payouts");
    pane.keys(&["Enter"]);
    // the repo of the Context's newest Plan
    assert_eq!(
        wait_for_calls(&env, 1),
        [words(&["plan", "new", "pay-4521", "--context=alpha", "--repo=/repo/sketch", "--title=Retry failed payouts"])]
    );
    pane.wait("the note", |_| pane.footer_has("Plan pay-4521 started"));
}

#[test]
fn n_in_a_context_without_plans_asks_for_the_repo() {
    let env = env();
    populate(&env);
    fake_cpd2(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.keys(&["G"]); // no such key: nothing
    pane.down(9); // down to gamma, the last row
    pane.keys(&["n"]);
    pane.type_text("first");
    pane.keys(&["Enter"]);
    pane.type_text("The first");
    pane.keys(&["Enter"]);
    pane.wait("the repo prompt", |_| pane.footer_has("New Plan in gamma, repo:"));
    pane.type_text("/work/repo");
    pane.keys(&["Enter"]);
    assert_eq!(
        wait_for_calls(&env, 1),
        [words(&["plan", "new", "first", "--context=gamma", "--repo=/work/repo", "--title=The first"])]
    );
}

#[test]
fn a_refused_command_shows_its_error() {
    let env = env();
    populate(&env);
    env.fake("cpd2", r#"echo '{"ok":false,"error":{"code":"no_cockpit","message":"No cockpit.","details":{}}}'; exit 3"#);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    pane.down(2);
    pane.keys(&["Enter"]);
    pane.wait("the error", |_| pane.footer_has("no_cockpit: No cockpit."));
    assert_eq!(calls(&env), [words(&["session", "open", "l-ship"])]);
    pane.keys(&["j"]); // any key clears it
    pane.wait("the note gone", |_| !pane.footer_has("no_cockpit"));
}

#[test]
fn q_ends_the_view() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    assert!(!pane.dead());
    pane.keys(&["q"]);
    let start = Instant::now();
    while !pane.dead() {
        assert!(start.elapsed() < Duration::from_secs(10), "the view did not end");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn the_tree_follows_the_events_of_the_store() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    // session-renamed
    let run = env.cpd2(&["session", "rename", "--session", "l-sketch", "--", "sketch renamed"]);
    run.ok();
    pane.wait("the new name", |s| s.iter().any(|r| r.contains("└ ○ sketch renamed")));
    // session-state
    env.cpd2(&["session", "state", "blocked", "--label", "cpd: Ask", "--session", "l-sketch"]).ok();
    let screen = pane.wait("the new state", |s| s.iter().any(|r| r.contains("└ ● sketch renamed cpd: Ask")));
    assert!(screen.iter().any(|r| r.starts_with("● alpha")));
}
