//! `cpd tui view tree` (commands.md §13.2, §17), end to end: the built binary
//! runs in a pane of a private tmux server (its terminal), over a prepared
//! store; a test reads the screen (`capture-pane`), presses keys (`send-keys`)
//! and, for the actions, finds a fake `cpd` first on `PATH` that records its
//! command line. The rendering of single rows, colours and keys is unit-tested
//! in `tui/src/tree/`.

mod common;

use std::time::{Duration, Instant};

use common::{CPD, TestEnv};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::{ContextId, PlanId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::store::Store;
use nix::fcntl::{Flock, FlockArg};

// ---------- the prepared store ----------

/// An env with the config `cpd` needs and an engine running (`engine-down` is no row of the Inbox); its tmux server is
/// the env's private one. The engine's lock is held until the test process ends.
fn env() -> TestEnv {
    let env = env_without_engine();
    let path = env.cpd_home().join("locks/watch.lock");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lock = Flock::lock(std::fs::File::create(path).unwrap(), FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap();
    std::mem::forget(lock);
    env
}

/// [`env`] with no engine.
fn env_without_engine() -> TestEnv {
    let env = TestEnv::new();
    let config = format!(
        "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n[tmux]\nsocket = \"{}\"\n",
        env.tmux_socket()
    );
    std::fs::write(env.cpd_home().join("config.toml"), config).unwrap();
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

fn context(id: &str) -> String {
    format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on)
         VALUES ('{id}', 'Title of {id}', NULL, '2026-01-01T00:00:00.000Z', 'm1');"
    )
}

/// `pr`: the Plan has a PR (the schema ties phases `deploy` and `done` to one).
fn plan_row(id: &str, context: &str, phase: &str, lead: &str, created: &str, pr: bool) -> String {
    let late = !matches!(phase, "grounding" | "plan");
    let worktree = if late { "'/wt'" } else { "NULL" };
    let locked = if late { "'2026-01-02T00:00:00.000Z'" } else { "NULL" };
    let done = if phase == "done" { "'2026-01-09T00:00:00.000Z'" } else { "NULL" };
    let (row, number, url, opened, pull_request) = if pr {
        let state = if phase == "done" { "merged" } else { "open" };
        let merged = if phase == "done" { "'2026-01-09T00:00:00.000Z'" } else { "NULL" };
        (
            format!(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                 VALUES ('acme/{id}', 7, 'https://github.com/acme/{id}/pull/7', 'PR of {id}', 0, '{state}', 'none', 'none',
                         '2026-01-05T00:00:00.000Z', {merged}, '2026-01-06T00:00:00.000Z');"
            ),
            "7",
            format!("'https://github.com/acme/{id}/pull/7'"),
            "'2026-01-05T00:00:00.000Z'",
            format!("(SELECT id FROM pull_requests WHERE repo = 'acme/{id}')"),
        )
    } else {
        (String::new(), "NULL", "NULL".to_owned(), "NULL", "NULL".to_owned())
    };
    let status = match phase {
        "grounding" | "plan" => "waiting",
        "dispatch" => "dispatch",
        "deploy" => "open",
        _ => "merged",
    };
    format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
         VALUES ('{id}', '{context}', 'Plan {id}', '{phase}', '{lead}', '{created}', {locked}, {done});
         {row}
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                          pull_request, created_txn, created_at, started_at, merged_at)
         VALUES ('{id}', 'repo', '/repo/{id}', 'main', '{status}', 'cpd/{id}', {worktree}, {number}, {url}, {opened},
                 {pull_request}, cpd_txn(), '{created}', {locked}, {done});"
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

/// A Context's Manager Session (contract §3.1): idle, never a row of the tree.
fn manager_row(id: &str, context: &str) -> String {
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('{id}', 'manager', '{context}', NULL, NULL, '{context}-manager', '/sessions/{id}.jsonl', '/repo', 'O5',
                 NULL, 'idle', NULL, '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:07.000Z');"
    )
}

/// What the engine's E8 would open for `populate`'s store (the tree reads the Inbox, it does not compute it).
fn condition(kind: Kind, urgency: Urgency, session: &str, plan: Option<&str>, context: &str) -> Condition {
    Condition {
        kind,
        urgency,
        subject: format!("session:{session}"),
        label: format!("{session} label"),
        url: None,
        context: Some(ContextId::parse(context).unwrap()),
        plan: plan.map(|p| PlanId::parse(p).unwrap()),
        job: None,
        session: Some(SessionId::parse(session).unwrap()),
        record: None,
        event: None,
    }
}

/// The items `populate` leaves open: red dialogs of the fork `f-blocked` and of the Lead `l-rel`, and, for the two
/// Contexts' dots, a white `unread` of alpha's Manager and a magenta `dialog` of beta's.
fn items() -> Vec<Condition> {
    vec![
        condition(Kind::Dialog, Urgency::Red, "f-blocked", Some("ship"), "alpha"),
        condition(Kind::Dialog, Urgency::Red, "l-rel", Some("rel"), "beta"),
        condition(Kind::Unread, Urgency::White, "m-alpha", None, "alpha"),
        condition(Kind::Dialog, Urgency::Magenta, "m-beta", None, "beta"),
    ]
}

fn job_row(plan: &str, key: &str, status: &str) -> String {
    let fingerprint = if status == "draft" { "NULL".to_owned() } else { format!("'{}'", "0".repeat(64)) };
    let merged = if status == "merged" { "'abc123'" } else { "NULL" };
    let failed = if status == "failed" { "'crashed'" } else { "NULL" };
    format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, cancel_reason, cancel_requested_at, failed_cause, merged_sha, created_at)
         VALUES ('{plan}', 'repo', '{key}', 'Job {key}', 'S3', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, {fingerprint},
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
/// - `alpha` (also a Lead with no Plan, `g-alpha`, as migration 0010 leaves a grounded Lead): `ship` (dispatch; Lead `l-ship` working, in the chat slot; side forks
///   `f-idle` and `f-blocked`; Jobs queued, running, failed and two merged), `sketch` (plan; Lead idle; two drafts);
/// - `beta`: `rel` (pr; its Lead is blocked), `old` (done; its Lead is stopped);
/// - `gamma`: nothing.
///
/// The Inbox ([`items`]) gives the dots their `●`: `f-blocked` and `l-rel` have a red dialog; the Contexts alpha and beta
/// have a Manager with an item, which alone fills a Context row's `●`.
fn populate(env: &TestEnv) {
    let mut store = open(env);
    let mut s = String::new();
    s += &context("alpha");
    s += &context("beta");
    s += &context("gamma");
    s += &plan_row("ship", "alpha", "dispatch", "l-ship", "2026-01-02T00:00:00.000Z", false);
    s += &plan_row("sketch", "alpha", "plan", "l-sketch", "2026-01-02T00:00:01.000Z", false);
    s += &plan_row("rel", "beta", "deploy", "l-rel", "2026-01-02T00:00:02.000Z", true);
    s += &plan_row("old", "beta", "done", "l-old", "2026-01-02T00:00:03.000Z", true);
    s += &session_row("g-alpha", "grounded", "alpha", "", "idle", None, "2026-01-02T00:00:00.000Z");
    s += &session_row("l-ship", "ship lead", "alpha", "ship", "working", None, "2026-01-02T00:00:00.000Z");
    s += &session_row("f-idle", "ship fork", "alpha", "ship", "idle", None, "2026-01-02T00:00:08.000Z");
    s += &session_row("f-blocked", "ship review", "alpha", "ship", "blocked", Some("cpd: Worker question"), "2026-01-02T00:00:09.000Z");
    s += &session_row("l-sketch", "sketch", "alpha", "sketch", "idle", None, "2026-01-02T00:00:01.000Z");
    s += &session_row("l-rel", "rel lead", "beta", "rel", "blocked", Some("cpd: PR feedback"), "2026-01-02T00:00:02.000Z");
    s += &session_row("l-old", "old lead", "beta", "old", "stopped", None, "2026-01-02T00:00:03.000Z");
    s += &manager_row("m-alpha", "alpha", "idle", None);
    s += &manager_row("m-beta", "beta", "blocked", Some("cpd: asking"));
    for (key, status) in [("a", "merged"), ("b", "merged"), ("c", "running"), ("d", "queued"), ("e", "failed")] {
        s += &job_row("ship", key, status);
    }
    for key in ["s1", "s2"] {
        s += &job_row("sketch", key, "draft");
    }
    s += &job_row("rel", "r1", "merged");
    s += &moved("alpha", "ship", "l-ship", "chat");
    s += &manager_row("m-alpha", "alpha");
    s += &manager_row("m-beta", "beta");
    sql(&mut store, &s);
    reconcile(&mut store, &items(), &opened_ago()).unwrap();
}

/// When `populate`'s Inbox items were opened: five minutes and a half ago, so that their age reads `5m` for the half
/// minute a test takes at most.
fn opened_ago() -> String {
    let at = time::OffsetDateTime::now_utc() - time::Duration::seconds(330);
    at.format(&time::macros::format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z")).unwrap()
}

/// The Inbox section of `populate`'s four items in a pane `width` wide: the header, three rows and `+1 more`.
fn inbox_section(width: usize) -> Vec<String> {
    let inner = width - 2;
    let title = " Inbox 4 ";
    let item = |label: &str| format!(" ● {label}{}5m", " ".repeat(inner - 2 - label.len() - 2));
    vec![
        format!(" ──{title}{}", "─".repeat(inner - 2 - title.len())),
        item("f-blocked label"),
        item("l-rel label"),
        item("m-beta label"),
        " +1 more".to_owned(),
    ]
}

// ---------- the pane ----------

const SESSION: &str = "tree";

struct Pane<'a> {
    env: &'a TestEnv,
}

impl<'a> Pane<'a> {
    /// Runs `cpd tui view tree` in a `width` × `height` pane of the env's private tmux server.
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
            &format!("{CPD} tui view tree"),
        ]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        Pane { env }
    }

    /// The screen, each row without trailing blanks, without the empty rows at the end. The working dot turns with the
    /// wall clock (`SPINNER`), so on the row of the one working Lead (its counters `1⋅1⋅0⋅1⋅2`) every frame reads as
    /// the first: a test cannot know which one it catches. Elsewhere a glyph is what it is: the last frame is the needs-you dot.
    fn screen(&self) -> Vec<String> {
        let out = self.env.tmux(&["capture-pane", "-p", "-t", SESSION]);
        let text: String = String::from_utf8(out.stdout).unwrap();
        let text: String = text
            .lines()
            .map(|l| {
                if l.trim_end().ends_with("1⋅1⋅0⋅1⋅2") {
                    l.chars().map(|c| if SPINNER.iter().any(|g| g.starts_with(c)) { WORKING.chars().next().unwrap() } else { c }).collect()
                } else {
                    l.to_owned()
                }
            })
            .collect::<Vec<String>>()
            .join("\n");
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

    /// A left click at 0-based `column` and `row`, as the terminal sends it (SGR mouse reporting, 1-based).
    fn click(&self, column: u16, row: u16) {
        for end in ['M', 'm'] {
            self.type_text(&format!("\x1b[<0;{};{}{end}", column + 1, row + 1));
        }
    }

    /// A right click at 0-based `column` and `row`: SGR button 2, which tmux calls button 3.
    fn right_click(&self, column: u16, row: u16) {
        for end in ['M', 'm'] {
            self.type_text(&format!("\x1b[<2;{};{}{end}", column + 1, row + 1));
        }
    }

    /// Whether the pane's app has asked for mouse events (any mode): what makes tmux's default `MouseDown3Pane`
    /// pass a right click to it instead of showing its own menu.
    fn captures_the_mouse(&self) -> bool {
        let out = self.env.tmux(&["display-message", "-p", "-t", SESSION, "#{mouse_any_flag}"]);
        String::from_utf8_lossy(&out.stdout).trim() == "1"
    }

    fn dead(&self) -> bool {
        let out = self.env.tmux(&["list-panes", "-t", SESSION, "-F", "#{pane_dead}"]);
        String::from_utf8_lossy(&out.stdout).trim() == "1"
    }

    fn footer_has(&self, text: &str) -> bool {
        self.screen().iter().any(|r| r.contains(text))
    }
}

/// A fake `cpd` that succeeds with `reply`.
fn fake_cpd(env: &TestEnv, reply: &str) {
    env.fake("cpd", &format!("echo '{reply}'"));
}

fn calls(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("cpd")
}

fn wait_for_calls(env: &TestEnv, n: usize) -> Vec<Vec<String>> {
    let start = Instant::now();
    loop {
        let calls = calls(env);
        if calls.len() >= n {
            return calls;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "waiting for {n} calls of cpd; got {calls:?}");
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
    let pane = Pane::start(&env, 46, 18);
    let screen = pane.ready();
    let row = |left: &str, right: &str| format!(" {left:<w$}{right}", w = 44 - right.chars().count());
    let mut expected = vec![
        String::new(),
        " ▾ ● alpha".to_owned(),
        row("⏵ ├ ◐ ship", "1⋅1⋅0⋅1⋅2"),
        row("  ├ ○ sketch", "2⋅0⋅0⋅0⋅0"),
        "   ├ ○ ship·f-id".to_owned(),
        "   └ ● ship·f-bl".to_owned(),
        " ▾ ● beta".to_owned(),
        row("  ├ ● rel", "0⋅0⋅0⋅0⋅1"),
        row("  └ ◌ old", "0⋅0⋅0⋅0⋅0"),
        " ▾ ◌ gamma".to_owned(),
    ];
    expected.extend(["", "", ""].map(String::from)); // the rest of the tree's rows, then the Inbox section pinned below
    expected.extend(inbox_section(46));
    assert_eq!(
        screen,
        expected,
        "the Context and Plan ids, the forks' `<plan id>·<4 characters of the Session id>`; the forks are siblings of the Plans, after them, under their Context (B1); the Lead with no Plan, g-alpha, is no row; the Jobs: queued, running, merging, failed, merged; no state text; the Inbox section at the bottom: its header, three items with their age, `+1 more`"
    );
}

#[test]
fn the_tree_leaves_the_first_and_the_last_column_blank() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env, 46, 18);
    let screen = pane.ready();
    // `screen` is trimmed at the right, so the last column is blank when no row is longer than 45 columns
    assert!(screen.iter().all(|r| r.is_empty() || r.starts_with(' ')), "column 0: {screen:?}");
    assert!(screen.iter().all(|r| r.chars().count() <= 45), "the last column: {screen:?}");
    assert!(screen.iter().any(|r| r.chars().count() == 45), "the counters end at width − 2: {screen:?}");
}

#[test]
fn an_empty_store_says_so() {
    let env = env();
    drop(open(&env));
    let pane = Pane::start(&env, 30, 5);
    let header = format!(" ── Inbox 0 {}", "─".repeat(28 - 2 - " Inbox 0 ".len()));
    assert_eq!(pane.wait("the empty tree", |s| !s.is_empty()), ["", "  no Contexts", "", "", header.as_str()], "the section is there with no item");
}

#[test]
fn the_view_needs_a_terminal() {
    let env = env();
    let run = env.cpd(&["tui", "view", "tree"]);
    assert_eq!(run.exit, 3, "{run:?}");
    let error = run.error();
    assert_eq!(error["code"], "not_a_terminal");
    assert_eq!(error["details"]["command"], "tui view tree");
}

#[test]
fn enter_opens_the_lead_of_a_plan_and_a_fork_with_session_open() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true,"session":"x","pane":"%1","displaced":null}"#);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    pane.down(3); // the cursor appears on ship (Context rows are not selectable), then sketch, then f-idle
    pane.keys(&["Enter"]);
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "open", "f-idle"])]);
    pane.keys(&["k", "k", "Enter"]);
    assert_eq!(wait_for_calls(&env, 2)[1], words(&["session", "open", "l-ship"]));
}

#[test]
fn a_click_on_a_context_row_opens_its_manager_and_its_fold_glyph_folds_it() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true,"manager":{"session":"m-alpha","name":"alpha-manager","state":"idle","pane":"%1","window":"@1"},"started":false,"session":"m-alpha","displaced":null}"#);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    pane.click(8, 1); // the label of the Context `alpha`
    assert_eq!(
        wait_for_calls(&env, 2),
        [words(&["context", "manager", "--context=alpha"]), words(&["session", "open", "m-alpha"])]
    );
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(calls(&env).len(), 2, "no more commands");
    let screen = pane.screen();
    assert!(screen[1].starts_with(" ▾ ") && screen.iter().any(|r| r.contains("ship")), "the label click folds nothing: {screen:?}");
    pane.click(1, 1); // its fold glyph
    let folded = pane.wait("alpha folded", |s| !s.iter().any(|r| r.contains("ship")));
    assert_eq!(folded[1], format!(" ▸ {IDLE} alpha 2"), "a folded Context draws ▸ and its number of Plans");
    assert_eq!(calls(&env).len(), 2, "the glyph runs no command");
    pane.click(1, 1);
    pane.wait("alpha unfolded", |s| s.iter().any(|r| r.contains("ship")));
    assert_eq!(calls(&env).len(), 2);
}

#[test]
fn a_refused_context_manager_shows_its_error_and_opens_nothing() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":false,"error":{"code":"archived","message":"The Context is archived."}}"#);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.click(8, 1);
    pane.wait("the error", |_| pane.footer_has("archived: The Context is archived."));
    assert_eq!(calls(&env), [words(&["context", "manager", "--context=alpha"])], "no session open");
}

#[test]
fn enter_n_and_x_before_any_arrow_key_do_nothing() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    pane.keys(&["Enter", "x", "n"]);
    std::thread::sleep(Duration::from_millis(500));
    assert!(calls(&env).is_empty());
    assert!(!pane.footer_has("Fork") && !pane.footer_has("Close"));
}

#[test]
fn no_key_but_the_references_does_anything() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 46, 14);
    let before = pane.ready();
    pane.down(2); // on sketch
    pane.keys(&["q", "f", "r", "p", "Space", "G", "Escape"]);
    std::thread::sleep(Duration::from_millis(500));
    assert!(!pane.dead(), "q does not end the view");
    assert!(calls(&env).is_empty());
    assert_eq!(pane.screen(), before, "no prompt, no note, nothing folded");
}

#[test]
fn n_starts_a_plan_named_in_the_prompt_with_plan_new() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true,"plan":{"id":"pay-4521"},"lead":{"session":"l-new"}}"#);
    let pane = Pane::start(&env, 80, 14);
    pane.ready();
    pane.down(2); // ship, sketch
    pane.keys(&["n"]);
    pane.wait("the prompt", |_| pane.footer_has("Fork Plan sketch into a new Plan, name: █ (Enter starts, Esc cancels)"));
    pane.keys(&["Enter"]);
    pane.wait("the refusal", |_| pane.footer_has("A name is required. Fork Plan sketch into a new Plan, name:"));
    pane.type_text("pay-4521");
    pane.keys(&["Enter"]);
    // the Context of the cursor's Session's Plan; the name is the id and the title
    assert_eq!(
        wait_for_calls(&env, 2),
        [words(&["plan", "new", "pay-4521", "--context=alpha", "--title=pay-4521"]), words(&["session", "open", "l-new"])],
        "the new Lead is opened"
    );
    pane.wait("the note", |_| pane.footer_has("forking pay-4521 in window park-pay-4521"));
}

#[test]
fn x_closes_a_session_after_y_with_session_close() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true,"session":"l-sketch","pane":"%4"}"#);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.down(2); // sketch
    pane.keys(&["x"]);
    pane.wait("the prompt", |_| pane.footer_has("Close Plan sketch? pi stops; the session file stays (y/N)"));
    pane.keys(&["n"]);
    pane.wait("the prompt gone", |_| !pane.footer_has("Close Plan sketch?"));
    pane.keys(&["x"]);
    pane.wait("the prompt again", |_| pane.footer_has("Close Plan sketch?"));
    pane.keys(&["y"]);
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "close", "l-sketch"])]);
    pane.wait("the note", |_| pane.footer_has("closed"));
}

#[test]
fn x_refuses_the_session_in_the_chat_slot() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.down(1); // ship, whose Lead is in the chat slot
    pane.keys(&["x"]);
    pane.wait("the refusal", |_| pane.footer_has("switch to another session first"));
    assert!(calls(&env).is_empty());
}

#[test]
fn x_refuses_a_session_that_moved_to_the_chat_slot_while_the_prompt_was_open() {
    let env = env();
    populate(&env);
    env.fake("cpd", r#"echo '{"ok":false,"error":{"code":"in_chat_slot","message":"In the chat slot.","details":{}}}'; exit 3"#);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.down(2); // sketch
    pane.keys(&["x"]);
    pane.wait("the prompt", |_| pane.footer_has("Close Plan sketch?"));
    pane.keys(&["y"]);
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "close", "l-sketch"])]);
    pane.wait("the refusal", |_| pane.footer_has("switch to another session first"));
}

#[test]
fn question_mark_opens_the_legend_popup() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    // `?` needs no cursor; with no client attached to the private server, the popup has nowhere to open
    pane.keys(&["?"]);
    pane.wait("the failure", |_| pane.footer_has("No tmux client is attached to the cockpit."));
}

#[test]
fn a_refused_command_shows_its_error_until_the_next_n_x_or_click() {
    let env = env();
    populate(&env);
    env.fake("cpd", r#"echo '{"ok":false,"error":{"code":"no_cockpit","message":"No cockpit.","details":{}}}'; exit 3"#);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    pane.down(1);
    pane.keys(&["Enter"]);
    pane.wait("the error", |_| pane.footer_has("no_cockpit: No cockpit."));
    assert_eq!(calls(&env), [words(&["session", "open", "l-ship"])]);
    pane.keys(&["j"]);
    std::thread::sleep(Duration::from_millis(300));
    assert!(pane.footer_has("no_cockpit"), "an arrow keeps it");
    pane.keys(&["n", "Escape"]);
    pane.wait("the note gone", |_| !pane.footer_has("no_cockpit"));
}

#[test]
fn the_tree_follows_the_events_of_the_store() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    // session-state
    env.cpd(&["session", "state", "blocked", "--label", "cpd: Ask", "--session", "l-sketch"]).ok();
    std::thread::sleep(Duration::from_millis(300));
    assert!(pane.screen().iter().any(|r| r.contains("├ ○ sketch")), "a blocked Session is no ● until the Inbox has its dialog");
    // inbox-opened: the engine's E8 opens the dialog
    let mut store = open(&env);
    let mut now = items();
    now.push(condition(Kind::Dialog, Urgency::Magenta, "l-sketch", Some("sketch"), "alpha"));
    reconcile(&mut store, &now, "2026-01-04T00:01:00.000Z").unwrap();
    pane.wait("the opened item", |s| s.iter().any(|r| r.contains("├ ● sketch")));
    // inbox-closed: its cause ends
    reconcile(&mut store, &items(), "2026-01-04T00:02:00.000Z").unwrap();
    pane.wait("the closed item", |s| s.iter().any(|r| r.contains("├ ○ sketch")));
    // inbox-dismissed, by the command: the red dialog of the fork falls back to its glyph
    let screen = pane.screen();
    assert!(screen.iter().any(|r| r.contains("└ ● ship·f-bl")));
    let id = store_item(&mut store, "session:f-blocked");
    env.cpd(&["inbox", "dismiss", &id.to_string()]).ok();
    pane.wait("the dismissed item", |s| s.iter().any(|r| r.contains("└ ○ ship·f-bl")));
}

/// The id of the open item of `subject`.
fn store_item(store: &mut Store, subject: &str) -> i64 {
    store.read(|c| Ok(c.query_row("SELECT id FROM inbox_items WHERE subject = ?1 AND closed_at IS NULL", [subject], |r| r.get(0))?)).unwrap()
}

/// `populate`, with the Context `gamma` and the Plan `old` of `beta` (and its Lead) archived.
fn archive(env: &TestEnv) {
    sql(
        &mut open(env),
        "UPDATE contexts SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'gamma';
         UPDATE plans SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'old';
         UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'l-old';",
    );
}

#[test]
fn archived_contexts_and_plans_collapse_into_more_rows_and_a_click_expands_and_collapses_them() {
    let env = env();
    populate(&env);
    archive(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 46, 20);
    let screen = pane.ready();
    let row = |left: &str, right: &str| format!(" {left:<w$}{right}", w = 44 - right.chars().count());
    let mut expected = vec![
        String::new(),
        " ▾ ● alpha".to_owned(),
        row("⏵ ├ ◐ ship", "1⋅1⋅0⋅1⋅2"),
        row("  ├ ○ sketch", "2⋅0⋅0⋅0⋅0"),
        "   ├ ○ ship·f-id".to_owned(),
        "   └ ● ship·f-bl".to_owned(),
        " ▾ ● beta".to_owned(),
        row("  ├ ● rel", "0⋅0⋅0⋅0⋅1"),
        "   └ …".to_owned(),
        " …".to_owned(),
    ];
    expected.extend(["", "", "", "", ""].map(String::from)); // the rest of the tree's rows
    expected.extend(inbox_section(46));
    assert_eq!(
        screen,
        expected,
        "the archived Plan `old` is behind beta's `…`, the archived Context `gamma` behind the last"
    );
    // a click on beta's `…` shows the archived Plan at the depth it would have if live, with the normal guide
    pane.click(4, 8);
    let screen = pane.wait("the archived Plan", |s| s.iter().any(|r| r.contains(&format!("└ {STOPPED} old"))));
    assert_eq!(screen[7..11], [row(&format!("  ├ {NEEDS_YOU} rel"), "0⋅0⋅0⋅0⋅1"), "   ├ …".to_owned(), row(&format!("  └ {STOPPED} old"), "0⋅0⋅0⋅0⋅0"), " …".to_owned()]);
    // a click on the archived Plan shows its history (below); then the other `…` shows the archived Context
    pane.click(10, 9);
    wait_for_calls(&env, 1);
    pane.click(1, 10);
    let screen = pane.wait("the archived Context", |s| s.iter().any(|r| r == &format!(" ▾ {STOPPED} gamma")));
    assert_eq!(screen[9..12], [row(&format!("  └ {STOPPED} old"), "0⋅0⋅0⋅0⋅0"), " …".to_owned(), format!(" ▾ {STOPPED} gamma").to_owned()]);
    // a click on beta's `…` collapses its group again
    pane.click(4, 8);
    let screen = pane.wait("the group collapsed", |s| s.iter().all(|r| !r.contains(&format!("└ {STOPPED} old"))));
    assert_eq!(screen[7..9], [row(&format!("  ├ {NEEDS_YOU} rel"), "0⋅0⋅0⋅0⋅1"), "   └ …".to_owned()]);
    assert_eq!(calls(&env), [words(&["session", "history", "l-old"])], "only the click on the archived Session ran a command");
}

#[test]
fn the_cursor_skips_archived_rows_and_enter_never_opens_one() {
    let env = env();
    populate(&env);
    archive(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 46, 16);
    pane.ready();
    pane.click(4, 8); // beta's `…`: its archived Plan is drawn
    pane.wait("the archived Plan", |s| s.iter().any(|r| r.contains("└ ◌ old")));
    pane.down(5); // ship, sketch, f-idle, f-blocked, rel
    pane.keys(&["Enter"]);
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "open", "l-rel"])]);
    pane.keys(&["x"]);
    pane.wait("the prompt", |_| pane.footer_has("Close Plan rel?"));
    pane.keys(&["n"]); // any key but `y` answers it
    pane.down(1); // then nowhere: the archived Plan `old` is not a stop, the Inbox's first item is
    pane.keys(&["Enter"]);
    assert_eq!(wait_for_calls(&env, 2)[1], words(&["session", "open", "f-blocked"]));
}

#[test]
fn the_tree_follows_the_archive_events() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env, 46, 16);
    pane.ready();
    // the kinds of migration 3, written as the commands will: the row, then its Event
    sql(
        &mut open(&env),
        "UPDATE plans SET archived_at = '2026-02-02T00:00:00.000Z' WHERE id = 'old';
         INSERT INTO events (txn, at, kind, context, plan, text, data)
         VALUES (cpd_txn(), '2026-02-02T00:00:00.000Z', 'plan-archived', 'beta', 'old', 'fixture', '{\"archived\":true}');",
    );
    pane.wait("the Plan behind `…`", |s| s.iter().any(|r| r == "   └ …") && s.iter().all(|r| !r.contains(&format!("└ {STOPPED} old"))));
    sql(
        &mut open(&env),
        "UPDATE contexts SET archived_at = '2026-02-02T00:00:00.000Z' WHERE id = 'gamma';
         INSERT INTO events (txn, at, kind, context, text, data)
         VALUES (cpd_txn(), '2026-02-02T00:00:01.000Z', 'context-archived', 'gamma', 'fixture', '{\"archived\":true}');",
    );
    pane.wait("the Context behind `…`", |s| s.iter().any(|r| r == " …") && s.iter().all(|r| !r.contains("gamma")));
    sql(
        &mut open(&env),
        "UPDATE sessions SET archived_at = '2026-02-03T00:00:00.000Z' WHERE id = 'f-idle';
         INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-02-03T00:00:00.000Z', 'session-archived', 'alpha', 'ship', 'f-idle', 'fixture', '{\"archived\":true}');",
    );
    pane.wait("the fork behind `…`", |s| s.iter().all(|r| !r.contains("ship·f-id")) && s.iter().filter(|r| r.as_str() == "   └ …").count() == 2);
}

// ---------- the right-click menu ----------

/// The screen has a menu box whose first inner row (after the top border) holds `first`.
fn menu_open(screen: &[String], first: &str) -> bool {
    screen.windows(2).any(|w| w[0].contains('┌') && w[0].contains('┐') && w[1].contains(&format!("│ {first}")))
}

fn the_menu_is_closed(screen: &[String]) -> bool {
    screen.iter().all(|r| !r.contains('┌') && !r.contains('┘'))
}

#[test]
fn the_tree_captures_the_mouse_so_tmuxs_default_right_click_binding_passes_it_on() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    pane.wait("the mouse capture", |_| pane.captures_the_mouse());
}

#[test]
fn a_right_click_opens_the_menu_of_the_row_and_esc_or_a_click_outside_closes_it_running_nothing() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 46, 18);
    pane.ready();
    pane.right_click(8, 4); // the fork `ship fork`
    let screen = pane.wait("the fork's menu", |s| menu_open(s, "Open"));
    for item in ["Open", "Rename…", "Fork…", "Close", "Archive"] {
        assert!(screen.iter().any(|r| r.contains(&format!("│ {item} "))), "{item} in\n{}", screen.join("\n"));
    }
    pane.keys(&["Escape"]);
    pane.wait("the menu closed", the_menu_is_closed);
    pane.right_click(5, 1); // the Context `alpha`
    let screen = pane.wait("the Context's menu", |s| menu_open(s, "New Plan…"));
    assert!(screen.iter().any(|r| r.contains("│ Rename… ")) && screen.iter().any(|r| r.contains("│ Archive ")));
    pane.click(40, 12); // outside it
    pane.wait("the menu closed", the_menu_is_closed);
    pane.right_click(8, 2); // the Plan `ship`
    pane.wait("the Plan's menu", |s| menu_open(s, "New session…"));
    pane.right_click(45, 13); // a right click outside closes it too
    pane.wait("the menu closed", the_menu_is_closed);
    pane.right_click(5, 12); // the empty area
    pane.wait("the empty area's menu", |s| menu_open(s, "New Context…"));
    pane.keys(&["Escape"]);
    pane.wait("the menu closed", the_menu_is_closed);
    std::thread::sleep(Duration::from_millis(300));
    assert!(calls(&env).is_empty(), "{:?}", calls(&env));
    assert!(!pane.footer_has("Archive"), "no prompt");
}

#[test]
fn archive_on_a_fork_runs_session_archive_at_once_with_no_prompt() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true,"archived":[{"kind":"session","id":"f-idle"}],"closed":[]}"#);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(8, 4);
    pane.wait("the menu", |s| menu_open(s, "Open"));
    pane.keys(&["Down", "Down", "Down", "Down", "Down", "Enter"]); // Archive is the sixth item
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "archive", "f-idle"])], "no y/N, and `f-idle` is not in the chat slot");
    pane.wait("the note", |_| pane.footer_has("archived"));
    assert!(!pane.footer_has("(y/N)"));
    assert!(the_menu_is_closed(&pane.screen()), "the menu closes when an item is picked");
}

#[test]
fn archive_on_the_chat_slots_lead_opens_the_next_session_first_then_archives() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(5, 1); // the Context `alpha`, whose Lead `l-ship` holds the chat slot: Archive is its fourth item
    pane.wait("the menu", |s| menu_open(s, "New Plan…"));
    pane.click(7, 5);
    assert_eq!(
        wait_for_calls(&env, 2),
        [words(&["session", "open", "l-rel"]), words(&["context", "archive", "alpha"])],
        "the first live Session after the Context, then the archive"
    );
    pane.right_click(8, 2); // the Plan `ship`: its Lead is the chat slot's, so the Plan below it opens first
    pane.wait("the menu", |s| menu_open(s, "New session…"));
    pane.click(10, 6);
    assert_eq!(
        wait_for_calls(&env, 4)[2..],
        [words(&["session", "open", "l-sketch"]), words(&["plan", "archive", "--plan=ship"])]
    );
}

#[test]
fn a_refusal_of_a_step_shows_in_the_note_row_and_stops_the_archive() {
    let env = env();
    populate(&env);
    env.fake("cpd", r#"echo '{"ok":false,"error":{"code":"no_cockpit","message":"There is no cockpit.","details":{}}}'; exit 3"#);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(8, 2);
    pane.wait("the menu", |s| menu_open(s, "New session…"));
    pane.click(10, 6);
    pane.wait("the refusal", |_| pane.footer_has("no_cockpit: There is no cockpit."));
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(calls(&env), [words(&["session", "open", "l-sketch"])], "the archive did not run");
}

#[test]
fn archive_on_the_only_live_lead_in_the_chat_slot_parks_it_first() {
    let env = env();
    populate(&env);
    sql(
        &mut open(&env),
        "UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id IN ('f-idle', 'f-blocked', 'l-sketch', 'l-rel', 'l-old');
         UPDATE plans SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id IN ('sketch', 'rel', 'old');",
    );
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(8, 2); // the Plan `ship`
    pane.wait("the menu", |s| menu_open(s, "New session…"));
    pane.click(10, 6);
    assert_eq!(wait_for_calls(&env, 2), [words(&["session", "park"]), words(&["plan", "archive", "--plan=ship"])]);
}

#[test]
fn a_click_on_an_archived_fork_runs_session_history() {
    let env = env();
    populate(&env);
    sql(&mut open(&env), "UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'f-blocked';");
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 60, 14);
    let screen = pane.ready();
    assert_eq!(screen[5], "   └ …", "the archived fork is behind alpha's `…`");
    pane.click(4, 5);
    pane.wait("the archived fork", |s| s.iter().any(|r| r.contains("└ ○ ship·f-bl")));
    pane.click(10, 6);
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "history", "f-blocked"])]);
    pane.click(8, 4); // a live fork still opens
    assert_eq!(wait_for_calls(&env, 2)[1], words(&["session", "open", "f-idle"]));
}

#[test]
fn a_click_that_shows_a_history_is_refused_in_the_note_row() {
    let env = env();
    populate(&env);
    sql(&mut open(&env), "UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'f-blocked';");
    env.fake("cpd", r#"echo '{"ok":false,"error":{"code":"file_unreadable","message":"The session file is missing.","details":{}}}'; exit 1"#);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.click(4, 5);
    pane.wait("the archived fork", |s| s.iter().any(|r| r.contains("└ ○ ship·f-bl")));
    pane.click(10, 6);
    pane.wait("the refusal", |_| pane.footer_has("file_unreadable: The session file is missing."));
}

#[test]
fn a_click_on_an_item_picks_it_and_a_prompt_cancels() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(5, 1); // the Context: New Plan…, Rename…, Archive at rows 1, 2, 3 of the box
    pane.wait("the menu", |s| menu_open(s, "New Plan…"));
    pane.click(7, 3); // Rename…
    pane.wait("the prompt", |_| pane.footer_has("Title: Title of alpha"));
    pane.keys(&["Escape"]);
    pane.wait("the prompt gone", |_| !pane.footer_has("Title: "));
    assert!(calls(&env).is_empty(), "{:?}", calls(&env));
}

#[test]
fn open_and_close_in_the_menu_run_session_open_and_after_y_session_close() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true,"session":"x","pane":"%1","displaced":null}"#);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(8, 5); // the fork `ship review`
    pane.wait("the menu", |s| menu_open(s, "Open"));
    pane.keys(&["Enter"]);
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "open", "f-blocked"])]);
    pane.right_click(8, 5);
    pane.wait("the menu", |s| menu_open(s, "Open"));
    pane.keys(&["Down", "Down", "Down", "Down", "Enter"]); // Close
    pane.wait("the prompt", |_| pane.footer_has("Close ship review? pi stops; the session file stays (y/N)"));
    pane.keys(&["y"]);
    assert_eq!(wait_for_calls(&env, 2)[1], words(&["session", "close", "f-blocked"]));
}

#[test]
fn unarchive_in_the_menu_runs_without_a_prompt_and_a_refusal_shows_in_the_note_row() {
    let env = env();
    populate(&env);
    archive(&env);
    env.fake("cpd", r#"echo '{"ok":false,"error":{"code":"archived","message":"The Context is archived.","details":{}}}'; exit 3"#);
    let pane = Pane::start(&env, 46, 16);
    pane.ready();
    pane.click(4, 8); // beta's `…`
    pane.wait("the archived Plan", |s| s.iter().any(|r| r.contains(&format!("└ {STOPPED} old"))));
    pane.right_click(10, 9); // the archived Plan `old`
    let screen = pane.wait("the menu", |s| menu_open(s, "Rename…"));
    assert!(screen.iter().any(|r| r.contains("│ Rename… ")), "{screen:?}");
    assert!(screen.iter().all(|r| !r.contains("New Plan") && !r.contains("Archive ")), "Rename…, Copy id and Unarchive only: {screen:?}");
    pane.keys(&["Down", "Down", "Enter"]); // Unarchive
    assert_eq!(wait_for_calls(&env, 1), [words(&["plan", "unarchive", "--plan=old"])]);
    pane.wait("the refusal", |_| pane.footer_has("archived: The Context is archived."));
}

#[test]
fn rename_on_an_archived_plan_runs_plan_rename() {
    let env = env();
    populate(&env);
    archive(&env);
    fake_cpd(&env, r#"{"ok":true,"plan":"old","title":"Older"}"#);
    let pane = Pane::start(&env, 60, 16);
    pane.ready();
    pane.click(4, 8); // beta's `…`
    pane.wait("the archived Plan", |s| s.iter().any(|r| r.contains(&format!("└ {STOPPED} old"))));
    pane.right_click(10, 9);
    pane.wait("the menu", |s| menu_open(s, "Rename…"));
    pane.keys(&["Enter"]);
    pane.wait("the prompt", |_| pane.footer_has("Title: Plan old"));
    pane.type_text("er");
    pane.keys(&["Enter"]);
    assert_eq!(wait_for_calls(&env, 1), [words(&["plan", "rename", "--plan=old", "--title=Plan older"])]);
}

#[test]
fn rename_on_a_plan_row_starts_from_its_title_and_runs_plan_rename() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true,"plan":"ship","title":"Plan ship!"}"#);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(8, 2); // the Plan `ship`
    pane.wait("the menu", |s| menu_open(s, "New session…"));
    pane.keys(&["Down", "Enter"]); // Rename…
    pane.wait("the prompt, from the title", |_| pane.footer_has("Title: Plan ship█ (Enter confirms, Esc cancels)"));
    assert!(the_menu_is_closed(&pane.screen()));
    pane.type_text("!");
    pane.keys(&["Enter"]);
    assert_eq!(wait_for_calls(&env, 1), [words(&["plan", "rename", "--plan=ship", "--title=Plan ship!"])]);
    pane.wait("the note", |_| pane.footer_has("renamed"));
}

#[test]
fn new_context_asks_a_title_and_runs_context_new_with_the_derived_id() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true,"context":{"id":"building-cpd"}}"#);
    let pane = Pane::start(&env, 60, 18);
    pane.ready();
    pane.right_click(5, 12); // the empty area
    pane.wait("the menu", |s| menu_open(s, "New Context…"));
    pane.keys(&["Enter"]);
    pane.wait("the prompt", |_| pane.footer_has("Title: █ (Enter confirms, Esc cancels)"));
    pane.keys(&["Enter"]);
    pane.wait("the refusal", |_| pane.footer_has("A title is required. Title:"));
    pane.type_text("Building cpd!");
    pane.keys(&["Enter"]);
    assert_eq!(wait_for_calls(&env, 1), [words(&["context", "new", "building-cpd", "--title=Building cpd!"])]);
    pane.wait("the note", |_| pane.footer_has("created building-cpd"));
}

#[test]
fn new_plan_and_fork_run_plan_new_without_a_repo_and_session_fork() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true,"lead":{"session":"l-new"},"session":{"id":"l-new"}}"#);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(5, 1); // the Context `alpha`
    pane.wait("the menu", |s| menu_open(s, "New Plan…"));
    pane.keys(&["Enter"]);
    pane.wait("the prompt", |_| pane.footer_has("Title: █"));
    pane.type_text("Ship");
    pane.keys(&["Enter"]);
    // `ship` is taken in the tree; no repo (R34 A12)
    assert_eq!(
        wait_for_calls(&env, 2),
        [words(&["plan", "new", "ship-2", "--context=alpha", "--title=Ship"]), words(&["session", "open", "l-new"])]
    );
    pane.wait("the prompt gone", |_| !pane.footer_has("Title:"));
    pane.right_click(8, 4); // the fork `ship fork`
    pane.wait("the menu", |s| menu_open(s, "Open"));
    pane.keys(&["Down", "Down", "Enter"]); // Fork…
    pane.wait("the prompt", |_| pane.footer_has("Name: ship fork (fork)█"));
    pane.keys(&["Enter"]);
    let calls = wait_for_calls(&env, 4);
    assert_eq!(calls[2], words(&["session", "fork", "f-idle", "--name=ship fork (fork)"]));
    assert_eq!(calls[3], words(&["session", "open", "l-new"]), "the new fork is opened");
}

#[test]
fn a_refused_open_of_the_new_lead_shows_its_message_and_the_plan_stays_created() {
    let env = env();
    populate(&env);
    env.fake(
        "cpd",
        r#"case "$1" in
  session) echo '{"ok":false,"error":{"code":"no_cockpit","message":"There is no cockpit.","details":{}}}'; exit 3;;
  *) echo '{"ok":true,"lead":{"session":"l-new"}}';;
esac"#,
    );
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(5, 1); // the Context `alpha`
    pane.wait("the menu", |s| menu_open(s, "New Plan…"));
    pane.keys(&["Enter"]);
    pane.wait("the prompt", |_| pane.footer_has("Title: █"));
    pane.type_text("Pay");
    pane.keys(&["Enter"]);
    pane.wait("the refusal", |_| pane.footer_has("no_cockpit: There is no cockpit."));
    assert_eq!(
        calls(&env),
        [words(&["plan", "new", "pay", "--context=alpha", "--title=Pay"]), words(&["session", "open", "l-new"])]
    );
}

#[test]
fn copy_id_puts_the_shown_id_in_tmuxs_buffer_and_says_so() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    for (column, row, id) in [(5, 1, "alpha"), (8, 2, "ship"), (8, 4, "ship·f-id")] {
        pane.right_click(column, row);
        pane.wait("the menu", |s| menu_open(s, if id == "alpha" { "New Plan…" } else if id == "ship" { "New session…" } else { "Open" }));
        let downs = if id == "ship·f-id" { 3 } else { 2 };
        pane.keys(&[&["Down"; 3][..downs], &["Enter"]].concat());
        pane.wait("the note", |_| pane.footer_has(&format!("copied {id}")));
        let buffer = env.tmux(&["show-buffer"]);
        assert_eq!(String::from_utf8_lossy(&buffer.stdout), id);
    }
    assert!(calls(&env).is_empty(), "Copy id runs no cpd command: {:?}", calls(&env));
}

// ---------- the Inbox section (commands.md §13.2) ----------

#[test]
fn the_inbox_sections_more_row_expands_the_list_inline_and_less_folds_it() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 46, 20);
    let screen = pane.ready();
    // the section is pinned to the bottom, the Session rows above it
    assert_eq!(screen[15..], inbox_section(46));
    pane.click(10, 19); // `+1 more`
    let screen = pane.wait("the expanded list", |s| s.last().is_some_and(|r| r == " − less"));
    let item = |label: &str| format!(" ● {label}{}5m", " ".repeat(44 - 2 - label.len() - 2));
    assert_eq!(screen[14..], [inbox_section(46)[..4].to_vec(), vec![item("m-alpha label"), " − less".to_owned()]].concat());
    pane.click(3, 19); // `− less`
    let screen = pane.wait("the folded list", |s| s.last().is_some_and(|r| r == " +1 more"));
    assert_eq!(screen[15..], inbox_section(46));
    assert!(calls(&env).is_empty(), "expanding and folding are no commands");
}

/// `populate`'s items and one of each kind that jumps or links; the engine-error is caused by Event `event`.
fn jumping_items(event: i64) -> Vec<Condition> {
    let other = |kind, urgency, subject: &str, label: &str| Condition {
        subject: subject.to_owned(),
        label: label.to_owned(),
        session: None,
        ..condition(kind, urgency, "none", Some("ship"), "alpha")
    };
    let mut items = items();
    items.extend([
        Condition { event: Some(event), plan: None, context: None, ..other(Kind::EngineError, Urgency::Red, "engine:store", "engine: store busy") },
        Condition {
            url: Some("https://github.com/acme/api/pull/12".into()),
            ..other(Kind::PrFailing, Urgency::Yellow, "pr:acme/api#12", "acme/api#12 failing")
        },
        other(Kind::JobStuck, Urgency::Yellow, "job:ship/c", "ship: c is stuck"),
        Condition { plan: None, ..other(Kind::Question, Urgency::Magenta, "question:q1", "alpha: a question") },
    ]);
    items
}

#[test]
fn a_click_on_an_inbox_item_runs_the_jump_of_its_kind() {
    let env = env();
    populate(&env);
    let mut store = open(&env);
    let event = store.read(cpd_core::events::highest_id).unwrap();
    reconcile(&mut store, &jumping_items(event), &opened_ago()).unwrap();
    fake_cpd(&env, r#"{"ok":true,"manager":{"session":"m-alpha"}}"#);
    // a fake tmux: one client, the popup recorded and not run, the rest the real tmux
    let real = String::from_utf8(env.command("sh").args(["-c", "command -v tmux"]).output().unwrap().stdout).unwrap();
    env.fake(
        "tmux",
        &format!("case \"$*\" in\n *list-clients*) echo '100 200 60';;\n *display-popup*) ;;\n *) exec {} \"$@\";;\nesac", real.trim()),
    );
    let pane = Pane::start(&env, 46, 26);
    pane.ready();
    pane.wait("the more row", |s| s.last().is_some_and(|r| r.starts_with(" +")));
    let more = pane.screen().len() as u16 - 1;
    pane.click(10, more);
    pane.wait("the expanded list", |s| s.last().is_some_and(|r| r == " − less"));
    let row_of = |label: &str| {
        let screen = pane.screen();
        screen.iter().position(|r| r.contains(label)).unwrap_or_else(|| panic!("{label} on\n{}", screen.join("\n"))) as u16
    };
    // dialog: `session open <its Session>`
    pane.click(10, row_of("f-blocked label"));
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "open", "f-blocked"])]);
    // job-stuck: the Lead of its Plan
    pane.click(10, row_of("ship: c is stuck"));
    assert_eq!(wait_for_calls(&env, 2)[1], words(&["session", "open", "l-ship"]));
    // question: the Context's Manager
    pane.click(10, row_of("alpha: a question"));
    assert_eq!(wait_for_calls(&env, 4)[2..], [words(&["context", "manager", "--context=alpha"]), words(&["session", "open", "m-alpha"])]);
    // pr-failing: the label is an OSC 8 link, the view runs nothing, and tmux draws the label whole
    let failing = row_of("acme/api#12 failing");
    pane.click(10, failing);
    // engine-error: the logs popup at the item's Event
    pane.click(10, row_of("engine: store busy"));
    let popups = || -> Vec<_> { env.calls("tmux").into_iter().filter(|c| c.iter().any(|a| a == "display-popup")).collect() };
    wait_for(|| !popups().is_empty());
    let popups = popups();
    assert_eq!(popups.len(), 1, "{popups:?}");
    assert_eq!(popups[0].last().unwrap(), &format!("cpd tui view logs --event {event}"));
    assert!(popups[0].contains(&" logs ".to_owned()), "{:?}", popups[0]);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(calls(&env).len(), 4, "the link and the logs ran no cpd command: {:?}", calls(&env));
}

#[test]
fn with_no_engine_the_inbox_leads_with_engine_down_and_a_click_on_it_opens_the_logs_at_the_bottom() {
    let env = env_without_engine();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let real = String::from_utf8(env.command("sh").args(["-c", "command -v tmux"]).output().unwrap().stdout).unwrap();
    env.fake(
        "tmux",
        &format!("case \"$*\" in\n *list-clients*) echo '100 200 60';;\n *display-popup*) ;;\n *) exec {} \"$@\";;\nesac", real.trim()),
    );
    let pane = Pane::start(&env, 46, 20);
    let screen = pane.ready();
    // counted in the header and in `+N more`, first of the rows
    let section = inbox_section(46);
    let title = " Inbox 5 ";
    assert_eq!(screen[15], format!(" ──{title}{}", "─".repeat(44 - 2 - title.len())));
    assert!(screen[16].starts_with(" ● engine is down"), "{:?}", screen[16]);
    assert_eq!(screen[17..19], section[1..3]);
    assert_eq!(screen[19], " +2 more");
    pane.click(10, 16);
    let popups = || -> Vec<_> { env.calls("tmux").into_iter().filter(|c| c.iter().any(|a| a == "display-popup")).collect() };
    wait_for(|| !popups().is_empty());
    let popups = popups();
    assert_eq!(popups.len(), 1, "{popups:?}");
    assert_eq!(popups[0].last().unwrap(), "cpd tui view logs");
    std::thread::sleep(Duration::from_millis(300));
    assert!(calls(&env).is_empty(), "no cpd command ran: {:?}", calls(&env));
}

fn wait_for(done: impl Fn() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_link_kinds_label_reads_whole_on_the_screen_through_tmux() {
    let env = env();
    populate(&env);
    let mut store = open(&env);
    let event = store.read(cpd_core::events::highest_id).unwrap();
    reconcile(&mut store, &jumping_items(event), &opened_ago()).unwrap();
    let pane = Pane::start(&env, 46, 26);
    pane.ready();
    pane.wait("the more row", |s| s.last().is_some_and(|r| r.starts_with(" +")));
    pane.click(10, pane.screen().len() as u16 - 1);
    let screen = pane.wait("the link's row", |s| s.iter().any(|r| r.contains("acme/api#12")));
    let expected = format!(" ● acme/api#12 failing{}5m", " ".repeat(44 - 2 - "acme/api#12 failing".len() - 2));
    let row = screen.iter().find(|r| r.contains("acme/api#12")).unwrap();
    assert_eq!(row, &expected, "the OSC 8 pieces leave no stray cell on the row");
}

// ---------- the Inbox's menu and keys (commands.md §13.2, §21) ----------

/// The row of the Inbox item labelled `label`, as the screen has it now (a note below the section moves it up).
fn item_row(pane: &Pane, label: &str) -> u16 {
    let screen = pane.screen();
    screen.iter().position(|r| r.contains(label)).unwrap_or_else(|| panic!("{label} on\n{}", screen.join("\n"))) as u16
}

/// The cursor on the Inbox's first item: down through the Sessions and into the section, then back up to the first.
fn cursor_on_the_first_item(pane: &Pane) {
    pane.down(15); // more than there are Sessions and shown items: it stops on the last shown item
    pane.keys(&["k", "k"]);
}

#[test]
fn the_cursor_reaches_the_items_and_d_dismisses_and_s_snoozes_the_one_it_is_on() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 46, 20);
    pane.ready();
    cursor_on_the_first_item(&pane);
    pane.keys(&["d"]);
    pane.wait("dismissed", |_| pane.footer_has("dismissed"));
    assert_eq!(calls(&env), [words(&["inbox", "dismiss", "1"])]);
    pane.keys(&["j", "s"]);
    pane.wait("the snooze choices", |s| menu_open(s, "Snooze 15 min"));
    let screen = pane.screen();
    assert!(!screen.iter().any(|r| r.contains("Dismiss")), "the choices are the three snoozes:\n{}", screen.join("\n"));
    pane.keys(&["Down", "Enter"]);
    pane.wait("snoozed", |_| pane.footer_has("snoozed"));
    assert_eq!(wait_for_calls(&env, 2)[1], words(&["inbox", "snooze", "2", "1h"]));
    pane.keys(&["j", "s", "Down", "Down", "Enter"]);
    assert_eq!(wait_for_calls(&env, 3)[2], words(&["inbox", "snooze", "4", "tomorrow"]));
    pane.keys(&["s"]);
    pane.wait("the snooze choices", |s| menu_open(s, "Snooze 15 min"));
    pane.keys(&["Escape"]);
    pane.wait("the menu closed", the_menu_is_closed);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(calls(&env).len(), 3, "Esc ran nothing: {:?}", calls(&env));
}

#[test]
fn s_and_d_do_nothing_on_a_session_row_or_before_an_arrow_key() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 46, 20);
    pane.ready();
    pane.keys(&["s", "d"]);
    pane.down(1); // on the first Session
    pane.keys(&["s", "d"]);
    std::thread::sleep(Duration::from_millis(300));
    assert!(the_menu_is_closed(&pane.screen()));
    assert!(calls(&env).is_empty(), "{:?}", calls(&env));
}

#[test]
fn enter_on_an_item_jumps_as_a_click_does() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 46, 20);
    pane.ready();
    cursor_on_the_first_item(&pane);
    pane.keys(&["Enter"]);
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "open", "f-blocked"])]);
}

#[test]
fn a_right_click_on_an_item_opens_its_four_entries_and_each_runs_its_command() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 46, 20);
    pane.ready();
    pane.right_click(10, item_row(&pane, "l-rel label")); // `l-rel label`, item 2
    let screen = pane.wait("the item's menu", |s| menu_open(s, "Snooze 15 min"));
    for entry in ["Snooze 15 min", "Snooze 1 h", "Snooze until tomorrow 09:00", "Dismiss"] {
        assert!(screen.iter().any(|r| r.contains(&format!("│ {entry}"))), "{entry} in\n{}", screen.join("\n"));
    }
    pane.keys(&["Enter"]);
    assert_eq!(wait_for_calls(&env, 1), [words(&["inbox", "snooze", "2", "15m"])]);
    for (i, expected) in [["inbox", "snooze", "2", "1h"], ["inbox", "snooze", "2", "tomorrow"]].into_iter().enumerate() {
        pane.wait("the menu closed", the_menu_is_closed);
        pane.right_click(10, item_row(&pane, "l-rel label"));
        pane.wait("the item's menu", |s| menu_open(s, "Snooze 15 min"));
        pane.keys(&vec!["Down"; i + 1]);
        pane.keys(&["Enter"]);
        assert_eq!(wait_for_calls(&env, 2 + i)[1 + i], words(&expected));
    }
    pane.wait("the menu closed", the_menu_is_closed);
    pane.right_click(10, item_row(&pane, "m-beta label")); // item 4
    pane.wait("the item's menu", |s| menu_open(s, "Snooze 15 min"));
    pane.keys(&["Down", "Down", "Down", "Enter"]);
    assert_eq!(wait_for_calls(&env, 4)[3], words(&["inbox", "dismiss", "4"]));
    // the header has no menu
    pane.wait("the menu closed", the_menu_is_closed);
    pane.right_click(10, item_row(&pane, "── Inbox"));
    std::thread::sleep(Duration::from_millis(300));
    assert!(the_menu_is_closed(&pane.screen()));
    assert_eq!(calls(&env).len(), 4);
}

#[test]
fn a_refusal_of_an_item_command_shows_in_the_prompt_row() {
    let env = env();
    populate(&env);
    env.fake("cpd", r#"echo '{"ok":false,"error":{"code":"item_closed","message":"Item 1 is closed.","details":{}}}'; exit 3"#);
    let pane = Pane::start(&env, 46, 20);
    pane.ready();
    cursor_on_the_first_item(&pane);
    pane.keys(&["d"]);
    pane.wait("the refusal", |_| pane.footer_has("item_closed: Item 1 is closed."));
    assert_eq!(calls(&env), [words(&["inbox", "dismiss", "1"])]);
}
