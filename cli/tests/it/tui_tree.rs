//! `cpd tui view tree` (commands.md §13.2, §17), end to end: the built binary
//! runs in a pane of a private tmux server (its terminal), over a prepared
//! store; a test reads the screen (`capture-pane`), presses keys (`send-keys`)
//! and, for the actions, finds a fake `cpd` first on `PATH` that records its
//! command line. The rendering of single rows, colours and keys is unit-tested
//! in `tui/src/tree/`.

use crate::common;

use std::time::{Duration, Instant};

use common::{CPD, TestEnv, sql};
use cpd_core::home::CpdHome;
use cpd_core::ids::{ContextId, PlanId, SessionId};
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::store::Store;
use cpd_tui::palette::{IDLE, NEEDS_YOU, SPINNER, STOPPED, WAITING};
use nix::fcntl::{Flock, FlockArg};

const WORKING: &str = SPINNER[0];

// The tree's glyphs. `cpd_tui::tree` is private, so the tests repeat them (the unit tests of `tui/src/tree/` use the constants).
const ACTIVE_MARK: &str = "\u{F040A}"; // md-play
/// The closing row's arrows (md-arrow_down_drop_circle_outline, md-arrow_up_drop_circle_outline) and the pill's caps.
const DOWN: &str = "\u{F004B}";
const UP: &str = "\u{F0063}";
const CAP_LEFT: &str = "\u{E0B6}";
const CAP_RIGHT: &str = "\u{E0B4}";

/// The closing row of a block that folds, folded, after the pane's margin.
fn expand() -> String {
    format!(" ╰{DOWN} expand")
}

/// The same, expanded.
fn collapse() -> String {
    format!(" ╰{UP} collapse")
}

/// The Inbox's pill title for `count` items in `inner` columns (the pane less its margins), after the margin.
fn pill(inner: usize, count: usize) -> String {
    format!(" ╭─{CAP_LEFT}Inbox{CAP_RIGHT}{}{count}", "─".repeat(inner - 2 - "Inbox".len() - 2 - count.to_string().len()))
}

/// An Inbox item row in `inner` columns: the muted `│`, the icon, the label and the age (`5m`) at the last column.
fn item_row_of(inner: usize, label: &str) -> String {
    format!(" │ {NEEDS_YOU} {label}{}5m", " ".repeat(inner - 4 - label.len() - 2))
}

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
        "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n[events]\npoll_ms = 50\n[tmux]\nsocket = \"{}\"\n",
        env.tmux_socket()
    );
    std::fs::write(env.cpd_home().join("config.toml"), config).unwrap();
    env
}

fn open(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
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
/// The Inbox ([`items`]) gives the dots their filled dot: `f-blocked` and `l-rel` have a red dialog; the Contexts alpha and beta
/// have a Manager with an item, which alone fills a Context row's filled dot.
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

/// The Inbox section of `populate`'s four items in a pane `width` wide: the empty row, the pill title, three rows and the
/// fold widget's `╰󰁋 expand`.
fn inbox_section(width: usize) -> Vec<String> {
    let inner = width - 2;
    vec![
        String::new(),
        pill(inner, 4),
        item_row_of(inner, "f-blocked label"),
        item_row_of(inner, "l-rel label"),
        item_row_of(inner, "m-beta label"),
        expand(),
    ]
}

// ---------- the pane ----------

const SESSION: &str = "tree";

/// The note the legend (`?`) leaves where no tmux client is attached.
const NO_CLIENT: &str = "No tmux client is attached to the cockpit.";

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

    /// The screen with its escapes (the cursor and the colours), the turning working dot as in [`Pane::screen`].
    fn styled_screen(&self) -> String {
        let out = self.env.tmux(&["capture-pane", "-p", "-e", "-t", SESSION]);
        String::from_utf8(out.stdout).unwrap().replace(['◓', '◑', '◒'], "◐")
    }

    /// Waits until `done` holds of the screen; panics with the last screen.
    fn wait(&self, what: &str, done: impl Fn(&[String]) -> bool) -> Vec<String> {
        common::wait_screen(what, || self.screen(), done)
    }

    /// Waits until the screen is `expected` and returns it. A whole-screen assertion on a screen caught before the view finished drawing is a race.
    fn screen_is(&self, expected: &[String]) -> Vec<String> {
        self.rows_are(0, expected)
    }

    /// Like `screen_is`, for the rows from `first` on: the screen's rows `first..first + expected.len()`.
    /// Panics with the rows wanted and the last screen when they never show.
    fn rows_are(&self, first: usize, expected: &[String]) -> Vec<String> {
        let what = format!("rows {first}.. to be\n{}", expected.join("\n"));
        self.wait(&what, |screen| if first == 0 { screen == expected } else { screen.get(first..first + expected.len()) == Some(expected) })
    }

    /// Waits until the tree has been drawn.
    fn ready(&self) -> Vec<String> {
        self.wait("the tree", |s| s.iter().any(|r| r.contains("alpha")))
    }

    /// Waits until the view has handled every key sent so far: it handles them in order, and `?` always leaves a mark
    /// (no tmux client is attached, so the legend fails and the note says so). The positive signal that nothing came
    /// of the keys before it. Only where no note is shown yet, and the note is the marker's.
    fn settled(&self) {
        self.keys(&["?"]);
        self.wait("the view to handle the keys sent before", |_| self.footer_has(NO_CLIENT));
    }

    /// [`Pane::settled`] where a fake tmux has a client (see [`popups`]): `?` then opens the legend's popup, the mark.
    fn settled_with_a_client(&self) {
        let before = popups(self.env).len();
        self.keys(&["?"]);
        let popups = common::wait_some("the view to handle the input sent before", common::WAIT, || {
            let popups = popups(self.env);
            if popups.len() > before { Ok(popups) } else { Err(format!("popups: {popups:?}")) }
        });
        assert_eq!(popups.len(), before + 1, "{popups:?}");
        assert!(popups[before].contains(&" legend ".to_owned()), "the legend's: {:?}", popups[before]);
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
    common::wait_some(&format!("{n} calls of cpd"), common::WAIT, || {
        let calls = calls(env);
        if calls.len() >= n { Ok(calls) } else { Err(format!("got {calls:?}")) }
    })
}

fn words(w: &[&str]) -> Vec<String> {
    w.iter().map(|s| (*s).to_owned()).collect()
}

// ---------- the dialogs ----------

/// A fake tmux with a client attached (`list-clients`) whose `display-popup` plays the dialog kit's popup: it writes
/// the env's answer ([`confirm`], [`cancel`], [`type_in`]; a cancel until one is given) to the `--out` file of the
/// popup's command, as the view would; every other command is the real tmux's. The popups it was asked for are in
/// [`popups`]. (The popup's own drawing and keys are `tui_dialog.rs`'s.)
fn fake_dialogs(env: &TestEnv) {
    cancel(env);
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

fn answer(env: &TestEnv, json: &serde_json::Value) {
    std::fs::write(env.root().join("dialog-answer.json"), json.to_string()).unwrap();
}

/// The next popups are confirmed (a `confirm`'s button, an `input`'s Enter with no text).
fn confirm(env: &TestEnv) {
    answer(env, &serde_json::json!({"confirmed": true}));
}

/// The next popups are cancelled.
fn cancel(env: &TestEnv) {
    answer(env, &serde_json::json!({"confirmed": false}));
}

/// The next `input` popups accept `text`.
fn type_in(env: &TestEnv, text: &str) {
    answer(env, &serde_json::json!({"confirmed": true, "value": text}));
}

/// Waits for `n` popups; the command each ran, without its `--out` (a temporary file).
fn wait_for_popups(env: &TestEnv, n: usize) -> Vec<String> {
    common::wait_some(&format!("{n} popups"), common::WAIT, || {
        let popups = popups(env);
        if popups.len() >= n { Ok(popups) } else { Err(format!("got {popups:?}")) }
    })
    .iter()
    .map(|p| {
        let command = p.last().unwrap();
        let start = command.find(" --out='").expect("the popup's result file");
        let end = start + " --out='".len() + command[start + " --out='".len()..].find('\'').unwrap() + 1;
        format!("{}{}", &command[..start], &command[end..])
    })
    .collect()
}

// ---------- tests ----------

#[test]
fn the_tree_of_a_prepared_store() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env, 46, 19);
    let row = |left: &str, right: &str| format!(" {left:<w$}{right}", w = 44 - right.chars().count());
    let mut expected = vec![
        String::new(),
        format!(" ╭{NEEDS_YOU} alpha"),
        row(&format!("{ACTIVE_MARK}─{WORKING} ship"), "1⋅1⋅0⋅1⋅2"),
        row(&format!("├─{IDLE} sketch"), "2⋅0⋅0⋅0⋅0"),
        format!(" ├─{IDLE} ship·f-id"),
        format!(" ├─{NEEDS_YOU} ship·f-bl"),
        " ╰─".to_owned(),
        format!(" ╭{NEEDS_YOU} beta"),
        row(&format!("├─{NEEDS_YOU} rel"), "0⋅0⋅0⋅0⋅1"),
        row(&format!("├─{STOPPED} old"), "0⋅0⋅0⋅0⋅0"),
        " ╰─".to_owned(),
        format!(" ╭{STOPPED} gamma"),
        " ╰─".to_owned(),
    ]; // each Context is a fold: its rows behind `├`, then a plain `╰─` (nothing archived); the Inbox section is pinned below
    expected.extend(inbox_section(46));
    let screen = pane.screen_is(&expected);
    assert_eq!(
        screen,
        expected,
        "the Context and Plan ids, the forks' `<plan id>·<4 characters of the Session id>`; the Contexts' plain `╰─` rows (nothing archived); the forks are siblings of the Plans, after them, under their Context (B1); the Lead with no Plan, g-alpha, is no row; the Jobs: queued, running, merging, failed, merged; no state text; the Inbox section at the bottom: an empty row, its pill title, three items with their icon and age, `╰󰁋 expand`"
    );
}

#[test]
fn an_inbox_of_two_items_is_the_pill_title_and_two_rows_with_no_toggle_and_a_plain_close() {
    let env = env();
    populate(&env);
    reconcile(&mut open(&env), &items()[..2], &opened_ago()).unwrap();
    let pane = Pane::start(&env, 46, 18);
    let expected = [
        String::new(),
        pill(44, 2),
        item_row_of(44, "f-blocked label"),
        item_row_of(44, "l-rel label"),
        " ╰─".to_owned(),
    ];
    let screen = pane.wait("the two items", |s| s.len() == 18 && s[13..] == expected);
    assert!(screen.iter().all(|r| !r.contains("expand")), "two items have nothing to expand: {screen:?}");
}

#[test]
fn the_tree_leaves_the_first_and_the_last_column_blank() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env, 46, 19);
    pane.ready();
    let screen = pane.wait("the whole tree", |s| s.last().is_some_and(|r| *r == expand()));
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
    let header = pill(28, 0);
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
fn a_click_on_a_context_title_opens_its_manager_and_only_its_closing_row_folds_its_archived_rows() {
    let env = env();
    populate(&env);
    sql(&mut open(&env), "UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'f-blocked';");
    fake_cpd(&env, r#"{"ok":true,"manager":{"session":"m-alpha","name":"alpha-manager","state":"idle","pane":"%1","window":"@1"},"started":false,"session":"m-alpha","displaced":null}"#);
    let pane = Pane::start(&env, 46, 20);
    pane.ready();
    pane.click(8, 1); // the title row of the Context `alpha`
    assert_eq!(
        wait_for_calls(&env, 2),
        [words(&["context", "manager", "--context=alpha"]), words(&["session", "open", "m-alpha"])]
    );
    pane.settled();
    assert_eq!(calls(&env).len(), 2, "no more commands");
    let screen = pane.screen();
    assert!(screen[1].starts_with(&format!(" ╭{NEEDS_YOU} alpha")) && screen.iter().any(|r| r.contains("ship")), "the title click folds nothing: {screen:?}");
    assert_eq!(screen[6], collapse(), "alpha's closing row");
    pane.click(1, 1); // the first cell of the title row: its dot
    pane.settled();
    assert_eq!(wait_for_calls(&env, 4).len(), 4, "the dot is the title row's: it opens the Manager too");
    pane.click(8, 6); // its `╰󰁣 collapse` row
    let folded = pane.wait("alpha's archived row hidden", |s| !s.iter().any(|r| r.contains("ship·f-bl")));
    assert_eq!((folded[1].as_str(), folded[4].as_str(), folded[5].as_str()), (format!(" ╭{NEEDS_YOU} alpha").as_str(), format!(" ├─{IDLE} ship·f-id").as_str(), expand().as_str()), "a folded Context draws its Manager's dot (its unread item), its live rows and a `╰󰁋 expand` row: {folded:?}");
    assert!(folded.iter().any(|r| r.contains("ship")) && folded.iter().any(|r| r.contains("sketch")), "the live rows stay: {folded:?}");
    assert_eq!(calls(&env).len(), 4, "the closing row runs no command");
    pane.click(8, 5);
    pane.wait("alpha's archived row back", |s| s.iter().any(|r| r.contains("ship·f-bl")));
    assert_eq!(calls(&env).len(), 4);
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
    fake_dialogs(&env);
    confirm(&env);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    pane.keys(&["Enter", "x", "n"]);
    pane.settled_with_a_client();
    assert!(calls(&env).is_empty());
    assert_eq!(popups(&env).len(), 1, "no popup but the legend's: {:?}", popups(&env));
}

#[test]
fn no_key_but_the_references_does_anything() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 46, 22);
    pane.ready();
    let before = pane.wait("the whole view, the Inbox section's last row included", |s| s.last().is_some_and(|r| *r == expand()));
    pane.down(2); // on sketch
    pane.keys(&["q", "f", "r", "p", "Space", "G", "Escape"]);
    pane.settled();
    assert!(!pane.dead(), "q does not end the view");
    assert!(calls(&env).is_empty());
    // The marker's note is the one change: the rows without it are what they were. Its row is taken from the blank
    // rows between the tree and the Inbox pinned below (the pane is tall enough to have some), so they are left out.
    let without_note = |rows: Vec<String>| -> Vec<String> { rows.into_iter().filter(|r| !r.is_empty() && !r.contains(NO_CLIENT)).collect() };
    assert_eq!(without_note(pane.screen()), without_note(before), "no prompt, nothing folded");
}

#[test]
fn n_opens_an_input_popup_for_the_plans_id_and_runs_plan_new_when_it_is_accepted() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true,"plan":{"id":"pay-4521"},"lead":{"session":"l-new"}}"#);
    fake_dialogs(&env);
    type_in(&env, "pay-4521");
    let pane = Pane::start(&env, 80, 14);
    pane.ready();
    pane.down(2); // ship, sketch
    pane.keys(&["n"]);
    assert_eq!(
        wait_for_popups(&env, 1),
        ["cpd tui view dialog input --title='New Plan' --prompt='Fork Plan sketch into a new Plan, name:' --initial='' --validate='plan-id'"]
    );
    // the Context of the cursor's Session's Plan; the name is the id and the title
    assert_eq!(
        wait_for_calls(&env, 2),
        [words(&["plan", "new", "pay-4521", "--context=alpha", "--title=pay-4521"]), words(&["session", "open", "l-new"])],
        "the new Lead is opened"
    );
    pane.wait("the note", |_| pane.footer_has("forking pay-4521 in window park-pay-4521"));
    assert!(!pane.footer_has("name:"), "no prompt row");
}

#[test]
fn n_and_x_run_nothing_when_their_popup_is_cancelled() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    fake_dialogs(&env);
    cancel(&env);
    let pane = Pane::start(&env, 80, 14);
    pane.ready();
    pane.down(2); // sketch
    pane.keys(&["n"]);
    wait_for_popups(&env, 1);
    pane.keys(&["x"]);
    let popups = wait_for_popups(&env, 2);
    assert_eq!(
        popups[1],
        "cpd tui view dialog confirm --title='Close' --text='Close Plan sketch? pi stops; the session file stays' --yes-label='Close'"
    );
    pane.settled_with_a_client();
    assert!(calls(&env).is_empty(), "{:?}", calls(&env));
    assert!(!pane.footer_has("Close Plan sketch") && !pane.footer_has("name:"), "no prompt row");
}

#[test]
fn x_opens_a_confirm_popup_and_runs_session_close_when_it_is_confirmed() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true,"session":"l-sketch","pane":"%4"}"#);
    fake_dialogs(&env);
    confirm(&env);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.down(2); // sketch
    pane.keys(&["x"]);
    assert_eq!(
        wait_for_popups(&env, 1),
        ["cpd tui view dialog confirm --title='Close' --text='Close Plan sketch? pi stops; the session file stays' --yes-label='Close'"]
    );
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "close", "l-sketch"])]);
    pane.wait("the note", |_| pane.footer_has("closed"));
}

#[test]
fn x_warns_in_its_popup_when_the_session_is_working() {
    let env = env();
    populate(&env);
    sql(&mut open(&env), "UPDATE sessions SET state = 'working' WHERE id = 'f-idle';");
    fake_cpd(&env, r#"{"ok":true}"#);
    fake_dialogs(&env);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.down(3); // f-idle
    pane.keys(&["x"]);
    assert!(wait_for_popups(&env, 1)[0].contains("--text='ship fork is working. Close ship fork? pi stops;"), "{:?}", popups(&env));
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
    assert!(popups(&env).is_empty(), "no popup for a refused key");
}

#[test]
fn x_refuses_a_session_that_moved_to_the_chat_slot_while_the_popup_was_open() {
    let env = env();
    populate(&env);
    env.fake("cpd", r#"echo '{"ok":false,"error":{"code":"in_chat_slot","message":"In the chat slot.","details":{}}}'; exit 3"#);
    fake_dialogs(&env);
    confirm(&env);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.down(2); // sketch
    pane.keys(&["x"]);
    wait_for_popups(&env, 1);
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
    pane.wait("the failure", |_| pane.footer_has(NO_CLIENT));
}

#[test]
fn the_tree_keeps_drawing_and_reading_keys_while_a_command_runs() {
    let env = env();
    populate(&env);
    env.fake("cpd", r#"sleep 4; echo '{"ok":false,"error":{"code":"slow","message":"Took a while.","details":{}}}'; exit 3"#);
    let pane = Pane::start(&env, 80, 14);
    pane.ready();
    pane.down(1);
    let start = Instant::now();
    pane.keys(&["Enter"]);
    pane.wait("the running note", |_| pane.footer_has("running: session open l-ship"));
    // `?` asks for the legend popup, which needs a tmux client: none is attached, so its note comes at once (since #128
    // `n` asks in a popup too, the same way)
    pane.keys(&["?"]);
    pane.wait("the legend's note, while the command still runs", |_| pane.footer_has(NO_CLIENT));
    // time-claim: the key is read while the command (slower than 3 s) still runs
    assert!(start.elapsed() < Duration::from_secs(3), "the key was read at once, not after the command: {:?}", start.elapsed());
    pane.wait("the result", |_| pane.footer_has("slow: Took a while."));
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
    // The cursor is a style, not a glyph: it moved when the screen with its escapes changed.
    let before = pane.styled_screen();
    pane.keys(&["j"]);
    common::wait_until("the cursor to move", || pane.styled_screen() != before);
    assert!(pane.footer_has("no_cockpit"), "an arrow keeps it");
    pane.keys(&["n", "Escape"]);
    pane.wait("the note gone", |_| !pane.footer_has("no_cockpit"));
}

#[test]
fn a_session_waiting_for_an_agent_shows_the_waiting_dot_and_not_the_idle_one() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    env.cpd(&["session", "state", "waiting", "--label", "cpd: Worker question", "--session", "l-sketch"]).ok();
    env.cpd(&["session", "state", "working", "--session", "f-idle"]).ok();
    pane.wait("the waiting Lead", |s| s.iter().any(|r| r.contains(&format!("├─{WAITING} sketch"))));
    assert!(!pane.screen().iter().any(|r| r.contains(&format!("├─{IDLE} sketch"))), "not the idle glyph");
}

#[test]
fn the_tree_follows_the_events_of_the_store() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    // session-state
    env.cpd(&["session", "state", "blocked", "--label", "cpd: Ask", "--session", "l-sketch"]).ok();
    // a later change on the screen is the positive signal that the view has taken a snapshot with the blocked Session
    env.cpd(&["session", "state", "working", "--session", "f-idle"]).ok();
    pane.wait("the working fork", |s| s.iter().any(|r| r.contains("ship·f-id") && SPINNER.iter().any(|g| r.contains(g))));
    assert!(pane.screen().iter().any(|r| r.contains(&format!("├─{IDLE} sketch"))), "a blocked Session is no filled dot until the Inbox has its dialog");
    // inbox-opened: the engine's E8 opens the dialog
    let mut store = open(&env);
    let mut now = items();
    now.push(condition(Kind::Dialog, Urgency::Magenta, "l-sketch", Some("sketch"), "alpha"));
    reconcile(&mut store, &now, "2026-01-04T00:01:00.000Z").unwrap();
    pane.wait("the opened item", |s| s.iter().any(|r| r.contains(&format!("├─{NEEDS_YOU} sketch"))));
    // inbox-closed: its cause ends
    reconcile(&mut store, &items(), "2026-01-04T00:02:00.000Z").unwrap();
    pane.wait("the closed item", |s| s.iter().any(|r| r.contains(&format!("├─{IDLE} sketch"))));
    // inbox-dismissed, by the command: the red dialog of the fork falls back to its glyph
    let screen = pane.screen();
    assert!(screen.iter().any(|r| r.contains(&format!("├─{NEEDS_YOU} ship·f-bl"))));
    let id = store_item(&mut store, "session:f-blocked");
    env.cpd(&["inbox", "dismiss", &id.to_string()]).ok();
    pane.wait("the dismissed item", |s| s.iter().any(|r| r.contains(&format!("├─{IDLE} ship·f-bl"))));
}

/// The id of the open item of `subject`.
fn store_item(store: &mut Store, subject: &str) -> i64 {
    store.read(|c| Ok(c.query_row("SELECT id FROM inbox_items WHERE subject = ?1 AND closed_at IS NULL", [subject], |r| r.get(0))?)).unwrap()
}

/// A failed snapshot keeps the tree on screen and shows the error in the footer; the next refresh takes the snapshot
/// again, with no Event needed, and the error goes when it succeeds (tui-W4). The read is made to fail by renaming the
/// table it reads; the snapshot is taken because an Event came (a store opened before the rename appends it).
#[test]
fn a_failed_snapshot_keeps_the_tree_and_shows_the_error_until_a_snapshot_succeeds() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    let mut store = open(&env);
    // not through `Store`, which refuses a changed schema
    let raw = |text: &str| rusqlite::Connection::open(env.cpd_home().join("cpd.db")).unwrap().execute_batch(text).unwrap();
    raw("ALTER TABLE sessions RENAME TO sessions_gone;");
    sql(&mut store, "INSERT INTO events (txn, at, kind, text, data) VALUES (cpd_txn(), '2026-02-01T00:00:00.000Z', 'session-state', 'fixture', '{}');");
    let screen = pane.wait("the error in the footer", |s| s.iter().any(|r| r.contains("no such table")));
    assert!(screen.iter().any(|r| r.starts_with(&format!(" ╭{NEEDS_YOU} alpha"))), "the tree stays: {screen:?}");
    raw("ALTER TABLE sessions_gone RENAME TO sessions;");
    pane.wait("the error to go", |s| !s.iter().any(|r| r.contains("no such table")) && s.iter().any(|r| r.starts_with(&format!(" ╭{NEEDS_YOU} alpha"))));
}

/// The tree follows Events in its own process (commands.md §17): it starts no `cpd events follow` (tui-F4, tui-G4), whose
/// spawn could fail on a path that is gone and be swallowed (tui-W2).
#[test]
fn the_tree_starts_no_subprocess() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env, 46, 14);
    pane.ready();
    let pid = String::from_utf8(env.tmux(&["display", "-p", "-t", SESSION, "#{pane_pid}"]).stdout).unwrap().trim().to_owned();
    // follow an Event and take a new snapshot: the fork's red dialog is dismissed
    let id = store_item(&mut open(&env), "session:f-blocked");
    env.cpd(&["inbox", "dismiss", &id.to_string()]).ok();
    pane.wait("the new state", |s| s.iter().any(|r| r.contains(&format!("├─{IDLE} ship·f-bl"))));
    let children = env.command("ps").args(["-o", "pid=,args=", "--ppid", &pid]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&children.stdout).trim(), "", "children of the view");
}

/// A follow that fails is shown in the footer, not swallowed (tui-W2): the cursor is from another database (the highest
/// Event id fell below it), which the follow refuses as `cursor_ahead`.
#[test]
fn a_follow_that_fails_is_shown_in_the_footer() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env, 100, 14);
    pane.ready();
    // not through `Store`, which keeps the Events append-only
    let raw = |text: &str| rusqlite::Connection::open(env.cpd_home().join("cpd.db")).unwrap().execute_batch(text).unwrap();
    raw("DROP TRIGGER events_no_delete; DELETE FROM events;");
    let screen = pane.wait("the error in the footer", |s| s.iter().any(|r| r.contains("Event cursor")));
    assert!(screen.iter().any(|r| r.starts_with(&format!(" ╭{NEEDS_YOU} alpha"))), "the tree stays: {screen:?}");
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
fn archived_plans_are_inside_their_contexts_block_and_their_closing_row_hides_only_them() {
    let env = env();
    populate(&env);
    archive(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 46, 26);
    let row = |left: &str, right: &str| format!(" {left:<w$}{right}", w = 44 - right.chars().count());
    let mut expected = vec![
        String::new(),
        format!(" ╭{NEEDS_YOU} alpha"),
        row(&format!("{ACTIVE_MARK}─{WORKING} ship"), "1⋅1⋅0⋅1⋅2"),
        row(&format!("├─{IDLE} sketch"), "2⋅0⋅0⋅0⋅0"),
        format!(" ├─{IDLE} ship·f-id"),
        format!(" ├─{NEEDS_YOU} ship·f-bl"),
        " ╰─".to_owned(),
        format!(" ╭{NEEDS_YOU} beta"),
        row(&format!("├─{NEEDS_YOU} rel"), "0⋅0⋅0⋅0⋅1"),
        row(&format!("├─{STOPPED} old"), "0⋅0⋅0⋅0⋅0"),
        collapse(),
        format!(" ╭{STOPPED} gamma"),
        " ╰─".to_owned(),
    ];
    expected.extend(["", "", "", "", "", "", ""].map(String::from)); // the rest of the tree's rows
    expected.extend(inbox_section(46));
    let screen = pane.screen_is(&expected);
    assert_eq!(
        screen,
        expected,
        "the archived Plan `old` is inside beta's block after the live rows, with beta's one closing row `╰󰁣 collapse`; alpha and the archived Context `gamma` have nothing to fold and close with a plain `╰─`; no `󰇘` row"
    );
    assert!(screen.iter().all(|r| !r.contains('\u{F01D8}')), "no `󰇘` row: {screen:?}");
    // a click on the archived Plan shows its history; the archived Context's title and its plain closing row do nothing
    pane.click(10, 9);
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "history", "l-old"])]);
    pane.click(5, 11);
    pane.click(1, 12);
    pane.settled();
    // beta's closing row hides its archived row only: the live row stays
    pane.click(5, 10);
    let shown = [format!(" ╭{NEEDS_YOU} beta")];
    let screen = pane.wait("beta folded", |s| s.get(9) == Some(&expand()));
    assert_eq!(screen[7..8], shown);
    assert!(screen[8].starts_with(&format!(" ├─{NEEDS_YOU} rel")), "the live row stays: {screen:?}");
    assert!(!screen.iter().any(|r| r.contains(" old")), "the archived row is hidden: {screen:?}");
    assert_eq!((screen[10].as_str(), screen[11].as_str()), (format!(" ╭{STOPPED} gamma").as_str(), " ╰─"));
    assert_eq!(calls(&env), [words(&["session", "history", "l-old"])], "only the click on the archived Session ran a command");
}

#[test]
fn the_cursor_skips_archived_rows_and_enter_never_opens_one() {
    let env = env();
    populate(&env);
    archive(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    fake_dialogs(&env);
    let pane = Pane::start(&env, 46, 22);
    pane.ready();
    pane.wait("the archived Plan", |s| s.iter().any(|r| r.contains(&format!("├─{STOPPED} old"))));
    pane.down(5); // ship, sketch, f-idle, f-blocked, rel
    pane.keys(&["Enter"]);
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "open", "l-rel"])]);
    pane.keys(&["x"]);
    wait_for_popups(&env, 1); // cancelled
    pane.down(1); // then nowhere: the archived Plan `old` is not a stop, the Inbox's first item is
    pane.keys(&["Enter"]);
    assert_eq!(wait_for_calls(&env, 2)[1], words(&["session", "open", "f-blocked"]));
}

#[test]
fn the_tree_follows_the_archive_events() {
    let env = env();
    populate(&env);
    let pane = Pane::start(&env, 46, 20);
    pane.ready();
    // an archived row is drawn muted: its colours change, its text does not (the working dots turn, so a row, not the screen)
    let styled = |text: &str| pane.styled_screen().lines().find(|r| r.contains(text)).map(str::to_owned).unwrap_or_default();
    let before = styled(" old");
    // the kinds of migration 3, written as the commands will: the row, then its Event
    sql(
        &mut open(&env),
        "UPDATE plans SET archived_at = '2026-02-02T00:00:00.000Z' WHERE id = 'old';
         INSERT INTO events (txn, at, kind, context, plan, text, data)
         VALUES (cpd_txn(), '2026-02-02T00:00:00.000Z', 'plan-archived', 'beta', 'old', 'fixture', '{\"archived\":true}');",
    );
    common::wait_until("the Plan muted", || styled(" old") != before);
    let screen = pane.screen();
    assert!(screen.iter().any(|r| r.contains(&format!("├─{STOPPED} old"))), "an archived Plan stays in its Context's block: {screen:?}");
    let before = styled("gamma");
    sql(
        &mut open(&env),
        "UPDATE contexts SET archived_at = '2026-02-02T00:00:00.000Z' WHERE id = 'gamma';
         INSERT INTO events (txn, at, kind, context, text, data)
         VALUES (cpd_txn(), '2026-02-02T00:00:01.000Z', 'context-archived', 'gamma', 'fixture', '{\"archived\":true}');",
    );
    common::wait_until("the Context muted", || styled("gamma") != before);
    sql(
        &mut open(&env),
        "UPDATE sessions SET archived_at = '2026-02-03T00:00:00.000Z' WHERE id = 'f-idle';
         INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-02-03T00:00:00.000Z', 'session-archived', 'alpha', 'ship', 'f-idle', 'fixture', '{\"archived\":true}');",
    );
    pane.wait("the fork after the live ones", |s| {
        let at = |text: &str| s.iter().position(|r| r.contains(text));
        at("ship·f-bl").zip(at("ship·f-id")).is_some_and(|(live, archived)| live < archived)
    });
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
    let pane = Pane::start(&env, 46, 22);
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
    pane.right_click(5, 15); // the empty area
    pane.wait("the empty area's menu", |s| menu_open(s, "New Context…"));
    pane.keys(&["Escape"]);
    pane.right_click(5, 6); // the closing row of the Context `alpha`: no menu
    pane.settled();
    assert!(the_menu_is_closed(&pane.screen()), "a closing row has no menu");
    pane.wait("the menu closed", the_menu_is_closed);
    assert!(calls(&env).is_empty(), "{:?}", calls(&env));
    assert!(!pane.footer_has("Archive"), "no prompt");
}

const ARCHIVE_FORK: &str = "cpd tui view dialog confirm --title='Archive' --text='Archive ship fork?' --yes-label='Archive'";

#[test]
fn archive_on_a_fork_confirms_in_a_popup_first_and_runs_session_archive_when_confirmed() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true,"archived":[{"kind":"session","id":"f-idle"}],"closed":[]}"#);
    fake_dialogs(&env);
    confirm(&env);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(8, 4);
    pane.wait("the menu", |s| menu_open(s, "Open"));
    pane.keys(&["Down", "Down", "Down", "Down", "Down", "Enter"]); // Archive is the sixth item
    assert_eq!(wait_for_popups(&env, 1), [ARCHIVE_FORK], "Archive confirms first (hs-d297)");
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "archive", "f-idle"])], "`f-idle` is not in the chat slot");
    pane.wait("the note", |_| pane.footer_has("archived"));
    assert!(the_menu_is_closed(&pane.screen()), "the menu closes when an item is picked");
}

#[test]
fn archive_runs_nothing_when_its_popup_is_cancelled() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    fake_dialogs(&env);
    cancel(&env);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(8, 4);
    pane.wait("the menu", |s| menu_open(s, "Open"));
    pane.keys(&["Down", "Down", "Down", "Down", "Down", "Enter"]);
    assert_eq!(wait_for_popups(&env, 1), [ARCHIVE_FORK]);
    pane.settled_with_a_client();
    assert!(calls(&env).is_empty(), "{:?}", calls(&env));
    assert!(!pane.footer_has("Archive"), "no prompt row");
}

#[test]
fn archive_on_the_chat_slots_lead_opens_the_next_session_first_then_archives() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    fake_dialogs(&env);
    confirm(&env);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(5, 1); // the Context `alpha`, whose Lead `l-ship` holds the chat slot: Archive is its fourth item
    pane.wait("the menu", |s| menu_open(s, "New Plan…"));
    pane.click(7, 5);
    assert_eq!(wait_for_popups(&env, 1), ["cpd tui view dialog confirm --title='Archive' --text='Archive Title of alpha?' --yes-label='Archive'"]);
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
    fake_dialogs(&env);
    confirm(&env);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(8, 2);
    pane.wait("the menu", |s| menu_open(s, "New session…"));
    pane.click(10, 6);
    // The note comes when the whole action is over (the view runs it in its loop), so the archive step is not still to come.
    pane.wait("the refusal", |_| pane.footer_has("no_cockpit: There is no cockpit."));
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
    fake_dialogs(&env);
    confirm(&env);
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
    pane.ready();
    let shown = [format!(" ├─{IDLE} ship·f-bl"), collapse()];
    assert_eq!(pane.rows_are(5, &shown)[5..7], shown, "the archived fork is inside alpha's block, after the live rows");
    pane.click(10, 5);
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
    pane.wait("the archived fork", |s| s.iter().any(|r| r.contains(&format!("├─{IDLE} ship·f-bl"))));
    pane.click(10, 5);
    pane.wait("the refusal", |_| pane.footer_has("file_unreadable: The session file is missing."));
}

#[test]
fn a_click_on_an_item_picks_it_and_a_cancelled_popup_runs_nothing() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    fake_dialogs(&env);
    cancel(&env);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(5, 1); // the Context: New Plan…, Rename…, Archive at rows 1, 2, 3 of the box
    pane.wait("the menu", |s| menu_open(s, "New Plan…"));
    pane.click(7, 3); // Rename…
    assert_eq!(
        wait_for_popups(&env, 1),
        ["cpd tui view dialog input --title='Rename' --prompt='Title:' --initial='Title of alpha' --validate='title'"]
    );
    pane.settled_with_a_client();
    assert!(calls(&env).is_empty(), "{:?}", calls(&env));
    assert!(!pane.footer_has("Title:"), "no prompt row");
}

#[test]
fn open_and_close_in_the_menu_run_session_open_and_after_a_confirmed_popup_session_close() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true,"session":"x","pane":"%1","displaced":null}"#);
    fake_dialogs(&env);
    confirm(&env);
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(8, 5); // the fork `ship review`
    pane.wait("the menu", |s| menu_open(s, "Open"));
    pane.keys(&["Enter"]);
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "open", "f-blocked"])]);
    pane.right_click(8, 5);
    pane.wait("the menu", |s| menu_open(s, "Open"));
    pane.keys(&["Down", "Down", "Down", "Down", "Enter"]); // Close
    assert_eq!(
        wait_for_popups(&env, 1),
        ["cpd tui view dialog confirm --title='Close' --text='Close ship review? pi stops; the session file stays' --yes-label='Close'"]
    );
    assert_eq!(wait_for_calls(&env, 2)[1], words(&["session", "close", "f-blocked"]));
}

#[test]
fn unarchive_in_the_menu_runs_without_a_popup_and_a_refusal_shows_in_the_note_row() {
    let env = env();
    populate(&env);
    archive(&env);
    env.fake("cpd", r#"echo '{"ok":false,"error":{"code":"archived","message":"The Context is archived.","details":{}}}'; exit 3"#);
    let pane = Pane::start(&env, 46, 22);
    pane.ready();
    pane.wait("the archived Plan", |s| s.iter().any(|r| r.contains(&format!("├─{STOPPED} old"))));
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
    fake_dialogs(&env);
    type_in(&env, "Plan older");
    let pane = Pane::start(&env, 60, 22);
    pane.ready();
    pane.wait("the archived Plan", |s| s.iter().any(|r| r.contains(&format!("├─{STOPPED} old"))));
    pane.right_click(10, 9);
    pane.wait("the menu", |s| menu_open(s, "Rename…"));
    pane.keys(&["Enter"]);
    assert_eq!(
        wait_for_popups(&env, 1),
        ["cpd tui view dialog input --title='Rename' --prompt='Title:' --initial='Plan old' --validate='title'"]
    );
    assert_eq!(wait_for_calls(&env, 1), [words(&["plan", "rename", "--plan=old", "--title=Plan older"])]);
}

#[test]
fn rename_on_a_plan_row_starts_from_its_title_and_runs_plan_rename() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true,"plan":"ship","title":"Plan ship!"}"#);
    fake_dialogs(&env);
    type_in(&env, "Plan ship!");
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(8, 2); // the Plan `ship`
    pane.wait("the menu", |s| menu_open(s, "New session…"));
    pane.keys(&["Down", "Enter"]); // Rename…
    assert_eq!(
        wait_for_popups(&env, 1),
        ["cpd tui view dialog input --title='Rename' --prompt='Title:' --initial='Plan ship' --validate='title'"],
        "from the title"
    );
    pane.wait("the menu closed", the_menu_is_closed);
    assert_eq!(wait_for_calls(&env, 1), [words(&["plan", "rename", "--plan=ship", "--title=Plan ship!"])]);
    pane.wait("the note", |_| pane.footer_has("renamed"));
}

#[test]
fn new_context_opens_an_input_popup_for_a_title_and_runs_context_new_with_the_derived_id() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true,"context":{"id":"building-cpd"}}"#);
    fake_dialogs(&env);
    type_in(&env, "Building cpd!");
    let pane = Pane::start(&env, 60, 22);
    pane.ready();
    pane.right_click(5, 15); // the empty area
    pane.wait("the menu", |s| menu_open(s, "New Context…"));
    pane.keys(&["Enter"]);
    assert_eq!(
        wait_for_popups(&env, 1),
        ["cpd tui view dialog input --title='New Context' --prompt='Title:' --initial='' --validate='title'"]
    );
    // the instance has one owner policy (`acme`, which every test env has): used without asking
    assert_eq!(wait_for_calls(&env, 1), [words(&["context", "new", "building-cpd", "--title=Building cpd!", "--owner=acme"])]);
    pane.wait("the note", |_| pane.footer_has("created building-cpd"));
}

#[test]
fn new_plan_and_fork_run_plan_new_without_a_repo_and_session_fork() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true,"lead":{"session":"l-new"},"session":{"id":"l-new"}}"#);
    fake_dialogs(&env);
    type_in(&env, "Ship");
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(5, 1); // the Context `alpha`
    pane.wait("the menu", |s| menu_open(s, "New Plan…"));
    pane.keys(&["Enter"]);
    assert_eq!(
        wait_for_popups(&env, 1),
        ["cpd tui view dialog input --title='New Plan' --prompt='Title:' --initial='' --validate='title'"]
    );
    // `ship` is taken in the tree; no repo (R34 A12)
    assert_eq!(
        wait_for_calls(&env, 2),
        [words(&["plan", "new", "ship-2", "--context=alpha", "--title=Ship"]), words(&["session", "open", "l-new"])]
    );
    type_in(&env, "ship fork (fork)");
    pane.right_click(8, 4); // the fork `ship fork`
    pane.wait("the menu", |s| menu_open(s, "Open"));
    pane.keys(&["Down", "Down", "Enter"]); // Fork…
    assert_eq!(
        wait_for_popups(&env, 2)[1],
        "cpd tui view dialog input --title='Fork' --prompt='Name:' --initial='ship fork (fork)' --validate='name'"
    );
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
    fake_dialogs(&env);
    type_in(&env, "Pay");
    let pane = Pane::start(&env, 60, 14);
    pane.ready();
    pane.right_click(5, 1); // the Context `alpha`
    pane.wait("the menu", |s| menu_open(s, "New Plan…"));
    pane.keys(&["Enter"]);
    wait_for_popups(&env, 1);
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

// ---------- the folds survive a restart of the view (hs-d260) ----------

fn folds_file(env: &TestEnv) -> std::path::PathBuf {
    env.cpd_home().join("run").join("tui").join("tree-folds.json")
}

/// The tree's rows above the Inbox section (whose own fold row is a `╰󰁋 expand` too).
fn above_inbox(screen: &[String]) -> &[String] {
    &screen[..screen.iter().position(|r| r.contains("Inbox")).unwrap_or(screen.len())]
}

/// `populate`, with `alpha`'s fork `f-blocked` archived (alpha has an archived row, so its closing row is a toggle) and
/// the Context `beta` archived with its Plans (its rows are all archived: it starts folded, with a toggle). `gamma` has
/// no row to fold and stays plain.
fn archive_for_folds(env: &TestEnv) {
    sql(
        &mut open(env),
        "UPDATE sessions SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'f-blocked';
         UPDATE contexts SET archived_at = '2026-02-01T00:00:00.000Z' WHERE id = 'beta';",
    );
}

/// `alpha`, the live Context with an archived row, is expanded: its block shows the archived row and ends in `╰󰁣 collapse`.
fn alpha_is_expanded(screen: &[String]) -> bool {
    let tree = above_inbox(screen);
    tree.iter().any(|r| r.trim() == format!("╰{UP} collapse")) && tree.iter().any(|r| r.contains("ship·f-bl"))
}

/// Waits until `tree-folds.json` holds `want` (it is written at most once a second).
fn wait_for_folds(env: &TestEnv, want: &str) {
    common::wait_until(&format!("the folds file to hold {want}"), || std::fs::read_to_string(folds_file(env)).is_ok_and(|text| text == want));
}

#[test]
fn a_collapsed_context_and_an_expanded_archived_context_survive_a_restart_of_the_view() {
    let env = env();
    populate(&env);
    archive_for_folds(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 46, 26);
    let screen = pane.ready();
    assert!(alpha_is_expanded(&screen), "{screen:#?}");
    assert!(!folds_file(&env).exists(), "a start with no fold writes nothing");
    // alpha's `╰󰁣 collapse` row (the fold widget's toggle row is the only one that folds, commands.md §13.2): its
    // archived row goes, its live rows stay
    let toggle = screen.iter().position(|r| r.trim() == format!("╰{UP} collapse")).unwrap() as u16;
    pane.click(5, toggle);
    let screen = pane.wait("alpha folded", |s| s.get(5).is_some_and(|r| r.trim() == format!("╰{DOWN} expand")));
    assert!(screen.iter().any(|r| r.contains("sketch")) && !screen.iter().any(|r| r.contains("ship·f-bl")), "{screen:#?}");
    // the archived Context beta starts folded: its `╰󰁋 expand` row, right under its title row
    let beta = screen.iter().position(|r| r.contains(" beta")).unwrap();
    assert_eq!(screen[beta + 1].trim(), format!("╰{DOWN} expand"), "{screen:#?}");
    pane.click(5, beta as u16 + 1);
    let before = pane.wait("beta expanded", |s| s.iter().position(|r| r.contains(" beta")).is_some_and(|b| s[b + 1].contains("rel")));
    wait_for_folds(&env, r#"{"collapsed":["alpha"],"expanded":[["beta"]]}"#);

    // respawn the pane: the same screen comes back (frames can arrive in two writes, hs-n206: poll for all of it)
    let out = env.tmux(&["respawn-pane", "-k", "-t", SESSION, &format!("{CPD} tui view tree")]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let after = pane.wait("the folds restored", |s| s == before.as_slice());
    assert_eq!(after, before);
    assert_eq!(calls(&env), Vec::<Vec<String>>::new(), "no command ran");
    let leftovers: Vec<_> = std::fs::read_dir(folds_file(&env).parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(leftovers, ["tree-folds.json"], "no temporary file is left");
}

#[test]
fn an_invalid_folds_file_starts_the_tree_with_the_defaults_and_a_note_and_the_next_fold_replaces_it() {
    let env = env();
    populate(&env);
    archive_for_folds(&env);
    std::fs::create_dir_all(folds_file(&env).parent().unwrap()).unwrap();
    std::fs::write(folds_file(&env), "{not json").unwrap();
    let pane = Pane::start(&env, 46, 26);
    let screen = pane.wait("the note", |s| s.iter().any(|r| r.contains("fold state reset")));
    assert!(alpha_is_expanded(&screen), "the defaults: alpha is expanded: {screen:?}");
    assert_eq!(std::fs::read_to_string(folds_file(&env)).unwrap(), "{not json", "nothing is written until a fold changes");
    let toggle = screen.iter().position(|r| r.trim() == format!("╰{UP} collapse")).unwrap() as u16;
    pane.click(5, toggle);
    pane.wait("alpha folded", |s| s.get(5).is_some_and(|r| r.trim() == format!("╰{DOWN} expand")));
    wait_for_folds(&env, r#"{"collapsed":["alpha"],"expanded":[]}"#);
}

#[test]
fn a_missing_folds_file_is_every_context_open() {
    let env = env();
    populate(&env);
    archive_for_folds(&env);
    let pane = Pane::start(&env, 46, 26);
    let screen = pane.ready();
    assert!(alpha_is_expanded(&screen), "{screen:?}");
    let beta = screen.iter().position(|r| r.contains(" beta")).unwrap();
    assert_eq!(screen[beta + 1].trim(), format!("╰{DOWN} expand"), "the archived Context starts folded: {screen:?}");
    assert!(screen.iter().all(|r| !r.contains("fold state")), "{screen:?}");
    assert!(!folds_file(&env).exists(), "nothing is written");
}

// ---------- the Inbox section (commands.md §13.2) ----------

#[test]
fn the_inbox_sections_toggle_row_expands_the_list_inline_and_folds_it_and_the_title_does_not() {
    let env = env();
    populate(&env);
    fake_cpd(&env, r#"{"ok":true}"#);
    let pane = Pane::start(&env, 46, 20);
    pane.ready();
    // the section is pinned to the bottom, the Session rows above it
    assert_eq!(pane.rows_are(14, &inbox_section(46))[14..], inbox_section(46));
    pane.click(10, 19); // `╰󰁋 expand`
    let expanded = [inbox_section(46)[..5].to_vec(), vec![item_row_of(44, "m-alpha label"), collapse()]].concat();
    assert_eq!(pane.rows_are(13, &expanded)[13..], expanded);
    pane.click(3, 19); // `╰󰁣 collapse`
    assert_eq!(pane.rows_are(14, &inbox_section(46))[14..], inbox_section(46));
    assert!(calls(&env).is_empty(), "expanding and folding are no commands");
    pane.click(10, 15); // the title: the pill toggles nothing
    pane.settled();
    let without_note = |rows: Vec<String>| -> Vec<String> { rows.into_iter().filter(|r| !r.contains(NO_CLIENT)).collect() };
    let rows = without_note(pane.screen());
    assert_eq!(rows[rows.len() - 6..], inbox_section(46), "still folded");
}

/// `populate`'s items and one of each kind that jumps or links; the old engine-error row is caused by Event `event`.
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
        Condition { url: Some("http://10.0.0.5:20000/".into()), ..other(Kind::PlanReview, Urgency::Magenta, "review:ship/r1", "ship plan review") },
        Condition { url: Some("https://x.io/ready".into()), ..other(Kind::PrReady, Urgency::White, "pr:acme/api#3", "acme/api#3 ready") },
        Condition {
            url: Some("https://x.io/changes".into()),
            plan: None,
            ..other(Kind::PrChanges, Urgency::Yellow, "pr:acme/web#4", "acme/web#4 changes requested")
        },
        Condition {
            url: Some("https://x.io/review".into()),
            plan: None,
            context: None,
            ..other(Kind::ReviewRequested, Urgency::Yellow, "pr:acme/web#5", "acme/web#5 review requested")
        },
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
    let pane = Pane::start(&env, 60, 40);
    pane.ready();
    pane.wait("the more row", |s| s.last().is_some_and(|r| *r == expand()));
    let more = pane.screen().len() as u16 - 1;
    pane.click(10, more);
    pane.wait("the expanded list", |s| s.last().is_some_and(|r| *r == collapse()));
    let row_of = |label: &str| {
        let screen = pane.screen();
        screen.iter().position(|r| r.contains(label)).unwrap_or_else(|| panic!("{label} on\n{}", screen.join("\n"))) as u16
    };
    let copies = || -> Vec<Vec<String>> { env.calls("tmux").into_iter().filter(|c| c.iter().any(|a| a == "set-buffer")).collect() };
    let copied = |n: usize, url: &str| {
        let copy = common::wait_some(&format!("copy {n} of {url}"), common::WAIT, || {
            let all = copies();
            if all.len() >= n { Ok(all[n - 1].clone()) } else { Err(format!("got {all:?}\n{}", pane.screen().join("\n"))) }
        });
        assert_eq!(copy[copy.len() - 3..], words(&["-w", "--", url]));
    };
    // a command runs off the UI thread with `running: …` in the footer, which moves the Inbox rows: the next row is
    // looked up once it has ended
    let ended = || pane.wait("the command to end", |_| !pane.footer_has("running:"));
    // dialog: `session open <its Session>`
    pane.click(10, row_of("f-blocked label"));
    assert_eq!(wait_for_calls(&env, 1), [words(&["session", "open", "f-blocked"])]);
    ended();
    // question: the Context's Manager
    pane.click(10, row_of("alpha: a question"));
    assert_eq!(wait_for_calls(&env, 3)[1..], [words(&["context", "manager", "--context=alpha"]), words(&["session", "open", "m-alpha"])]);
    ended();
    // plan-review: the Lead of its Plan, then its page's url copied, and the note says how to open it
    pane.click(10, row_of("ship plan review"));
    assert_eq!(wait_for_calls(&env, 4)[3], words(&["session", "open", "l-ship"]));
    ended();
    copied(1, "http://10.0.0.5:20000/");
    pane.wait("the note", |_| pane.footer_has("copied http://10.0.0.5:20000/: Cmd-click the row to open"));
    // pr-ready: the merge popup of its pull request, whether a Plan owns it or not, and no command
    pane.click(10, row_of("acme/api#3 ready"));
    common::wait_until("the merge popup", || !popups(&env).is_empty());
    assert_eq!(popups(&env).len(), 1, "{:?}", popups(&env));
    assert_eq!(popups(&env)[0].last().unwrap(), "cpd tui view merge 'acme/api#3'");
    assert!(popups(&env)[0].contains(&" merge ".to_owned()), "{:?}", popups(&env)[0]);
    ended();
    // a pull request, a Plan's or not, and a review request: the url is copied, and the note says how to open it
    pane.click(10, row_of("acme/web#4 changes requested"));
    copied(2, "https://x.io/changes");
    pane.wait("the note", |_| pane.footer_has("copied https://x.io/changes: Cmd-click the row to open it"));
    pane.click(10, row_of("acme/web#5 review requested"));
    copied(3, "https://x.io/review");
    pane.wait("the note", |_| pane.footer_has("copied https://x.io/review: Cmd-click the row to open it"));
    pane.click(10, row_of("acme/api#12 failing"));
    copied(4, "https://github.com/acme/api/pull/12");
    ended();
    pane.wait("the note", |_| pane.footer_has("copied https://github.com/acme/api/pull/12: Cmd-click the"));
    // engine-error and job-stuck, rows an older engine opened (hs-d314, hs-d318): no click; `settled_with_a_client` fails on any popup but the legend's
    pane.click(10, row_of("engine: store busy"));
    pane.click(10, row_of("ship: c is stuck"));
    pane.settled_with_a_client();
    assert_eq!(calls(&env).len(), 4, "the copies and the popups ran no cpd command: {:?}", calls(&env));
}

/// Every Manager archived: none is live (`populate` makes one per Context).
fn archive_managers(env: &TestEnv) {
    sql(&mut open(env), "UPDATE sessions SET archived_at = '2026-01-04T00:00:00.000Z' WHERE role = 'manager'");
}

/// A fake `tmux` that has no popup and no client sizes, the rest being the real one.
fn fake_tmux_without_popups(env: &TestEnv) {
    let real = String::from_utf8(env.command("sh").args(["-c", "command -v tmux"]).output().unwrap().stdout).unwrap();
    env.fake(
        "tmux",
        &format!("case \"$*\" in\n *list-clients*) echo '100 200 60';;\n *display-popup*) ;;\n *) exec {} \"$@\";;\nesac", real.trim()),
    );
}

#[test]
fn with_no_engine_and_no_manager_the_inbox_leads_with_engine_down_and_a_click_on_it_runs_watch_ensure() {
    let env = env_without_engine();
    populate(&env);
    archive_managers(&env);
    fake_cpd(&env, r#"{"ok":true,"started":true,"pane":"%3"}"#);
    fake_tmux_without_popups(&env);
    let pane = Pane::start(&env, 46, 20);
    pane.ready();
    // the tree restarted the engine on its own, as soon as it saw it down; the fake leaves the lock free, so it is down still
    assert_eq!(wait_for_calls(&env, 1), [words(&["watch", "--ensure"])]);
    // counted in the header, first of the rows (the note of the restart takes two rows of the narrow pane)
    let screen = pane.wait("the item", |s| s.iter().any(|r| r.contains("engine is down")));
    let first = screen.iter().position(|r| r.contains("engine is down")).unwrap();
    assert!(screen[first - 1].contains("Inbox") && screen[first - 1].ends_with('5'), "{:?}", screen[first - 1]);
    pane.click(10, u16::try_from(first).unwrap());
    assert_eq!(wait_for_calls(&env, 2), [words(&["watch", "--ensure"]), words(&["watch", "--ensure"])]);
    pane.wait("the note", |_| pane.footer_has("engine started"));
    pane.settled_with_a_client();
    assert_eq!(popups(&env).len(), 1, "only the legend's, from the settling: {:?}", popups(&env));
    assert_eq!(calls(&env).len(), 2, "the restart and the click ran `watch --ensure`: {:?}", calls(&env));
}

#[test]
fn a_stopped_engine_is_restarted_once_and_not_again_within_a_minute_and_no_engine_down_shows_with_a_manager() {
    let env = env_without_engine();
    populate(&env); // its Contexts have Managers: the engine's absence is no Inbox item
    fake_cpd(&env, r#"{"ok":true,"started":true,"pane":"%3"}"#);
    let pane = Pane::start(&env, 46, 20);
    pane.ready();
    assert_eq!(wait_for_calls(&env, 1), [words(&["watch", "--ensure"])]);
    pane.wait("the note", |_| pane.footer_has("engine was down: cpd watch --ensure started"));
    let screen = pane.screen();
    assert!(!screen.iter().any(|r| r.contains("engine is down")), "a Manager handles it: {screen:?}");
    // the view takes a snapshot every 2 s and sees the engine down each time, but tries once a minute
    // time-claim: the tree takes a snapshot every 2 s and tries `watch --ensure` once a minute, so none comes in 6 s; no per-poll signal from the view.
    std::thread::sleep(std::time::Duration::from_secs(6));
    assert_eq!(calls(&env), [words(&["watch", "--ensure"])], "once, and no message: it started");
}

#[test]
fn a_failed_restart_reaches_each_live_manager_once_and_none_without_a_manager() {
    let env = env_without_engine();
    populate(&env);
    env.fake(
        "cpd",
        "if [ \"$1\" = watch ]; then echo '{\"ok\":false,\"error\":{\"code\":\"watch_start_failed\",\"message\":\"no tmux\"}}'; else echo '{\"ok\":true}'; fi",
    );
    let pane = Pane::start(&env, 46, 20);
    pane.ready();
    let text = "The cpd engine is down and 'cpd watch --ensure' failed: watch_start_failed: no tmux. Start it with cpd watch --ensure.";
    let expected = [words(&["watch", "--ensure"]), words(&["session", "send", "m-alpha", text]), words(&["session", "send", "m-beta", text])];
    assert_eq!(wait_for_calls(&env, 3), expected);
    pane.wait("the note", |_| pane.footer_has("engine was down: cpd watch --ensure failed:"));
    // time-claim: the tree takes a snapshot every 2 s and tries `watch --ensure` once a minute, so none comes in 4 s; no per-poll signal from the view.
    std::thread::sleep(std::time::Duration::from_secs(4));
    assert_eq!(calls(&env), expected, "nothing more within the minute");

    // with no live Manager, nothing is sent: the Inbox item is the User's
    let env = env_without_engine();
    populate(&env);
    archive_managers(&env);
    env.fake(
        "cpd",
        "if [ \"$1\" = watch ]; then echo '{\"ok\":false,\"error\":{\"code\":\"watch_start_failed\",\"message\":\"no tmux\"}}'; else echo '{\"ok\":true}'; fi",
    );
    let pane = Pane::start(&env, 46, 20);
    pane.ready();
    assert_eq!(wait_for_calls(&env, 1), [words(&["watch", "--ensure"])]);
    pane.wait("the note", |_| pane.footer_has("engine was down: cpd watch --ensure failed"));
    pane.wait("the item", |s| s.iter().any(|r| r.contains("engine is down")));
    // time-claim: the tree takes a snapshot every 2 s and tries `watch --ensure` once a minute, so none comes in 4 s; no per-poll signal from the view.
    std::thread::sleep(std::time::Duration::from_secs(4));
    assert_eq!(calls(&env), [words(&["watch", "--ensure"])], "no Manager to tell");
}

/// An Attempt of Job `job` of `ship`, ended failed with `cause` (`Some`), or running.
fn attempt_row(job: &str, number: u32, ended: Option<&str>) -> String {
    let (status, outcome, ended_at) = match ended {
        Some(cause) => ("ended", format!("'failed', '{cause}'"), "'2026-01-03T00:05:00.000Z'"),
        None => ("running", "NULL, NULL".to_owned(), "NULL"),
    };
    format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at)
         VALUES ('ship', '{job}-a{number}', '{job}', {number}, 'S3', 'ladder', '{status}', {outcome}, 'cpd/ship--{job}-a{number}',
                 '/w/{job}-a{number}', '2026-01-03T00:01:00.000Z', {ended_at});"
    )
}

/// hs-d318: a stuck Job is a message to the Manager, not an Inbox item, so a row an older engine opened for one has no click:
/// no command runs, no Lead opens and no Worker pane starts beside the chat.
#[test]
fn a_click_on_a_stuck_job_item_an_older_engine_opened_does_nothing() {
    let env = env();
    populate(&env);
    let mut store = open(&env);
    // job `e` of `ship` failed twice: the click used to open the Worker of its latest Attempt, the second
    sql(&mut store, &(attempt_row("e", 1, Some("timed_out")) + &attempt_row("e", 2, Some("crashed"))));
    let stuck = |job: &str, label: &str| Condition {
        subject: format!("job:ship/{job}"),
        label: label.to_owned(),
        session: None,
        job: Some(cpd_core::ids::JobKey::parse(job).unwrap()),
        ..condition(Kind::JobStuck, Urgency::Red, "none", Some("ship"), "alpha")
    };
    let mut conditions = items();
    conditions.extend([stuck("e", "ship: e is stuck"), stuck("d", "ship: d is stuck")]);
    reconcile(&mut store, &conditions, &opened_ago()).unwrap();
    // the cockpit's centre on the private server: the chat slot; a Worker view would be a program that stays
    env.fake(
        "cpd",
        &format!("if [ \"$1\" = tui ] && [ \"$3\" = worker ]; then exec sleep {}; fi\necho '{{\"ok\":true}}'", common::SLEEP_SECONDS),
    );
    let out = env.tmux(&["new-session", "-d", "-s", "cpd", "-n", "main", "-x", "200", "-y", "40", "-P", "-F", "#{pane_id}", "sleep", "3600"]);
    let chat = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    assert!(env.tmux(&["set", "-p", "-t", &chat, "@cpd_view", "chat"]).status.success());
    let views = || String::from_utf8(env.tmux(&["list-panes", "-t", "=cpd:main", "-F", "#{@cpd_view}"]).stdout).unwrap();
    let pane = Pane::start(&env, 60, 40);
    pane.ready();
    pane.wait("the more row", |s| s.last().is_some_and(|r| *r == expand()));
    pane.click(10, pane.screen().len() as u16 - 1);
    pane.wait("the expanded list", |s| s.last().is_some_and(|r| *r == collapse()));
    // a Job with Attempts and a Job with none: neither click does anything
    pane.click(10, item_row(&pane, "ship: e is stuck"));
    pane.click(10, item_row(&pane, "ship: d is stuck"));
    pane.settled();
    assert!(!views().contains("worker"), "{}", views());
    assert_eq!(calls(&env), Vec::<Vec<String>>::new());
}

/// Two changes of the Inbox have no Event (§17): an engine that starts takes `engine-down` away, and a snooze that ends
/// brings its item back. The tree reads them then, with no timer of its own.
#[test]
fn the_inbox_follows_an_engine_that_starts_and_a_snooze_that_ends_with_no_event() {
    let env = env_without_engine();
    populate(&env);
    sql(
        &mut open(&env),
        "UPDATE inbox_items SET snoozed_until = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '+8 seconds') WHERE subject = 'session:m-beta';",
    );
    // The engine is down, but the Context has live Managers: `engine-down` is no item (hs-d318, the tree restarts the
    // engine itself; `engine_down_*` tests cover the item with no Manager). The snooze ends by the clock, with no Event.
    let path = env.cpd_home().join("locks/watch.lock");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let _engine = Flock::lock(std::fs::File::create(path).unwrap(), FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap();
    let pane = Pane::start(&env, 46, 20);
    pane.ready();
    pane.wait("three stored items, one snoozed", |s| s.iter().any(|r| r.contains("Inbox") && r.ends_with('3')));
    pane.wait("the snoozed item back", |s| s.iter().any(|r| r.contains("Inbox") && r.ends_with('4')));
}

/// The `display-popup` calls of a fake tmux.
fn popups(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("tmux").into_iter().filter(|c| c.iter().any(|a| a == "display-popup")).collect()
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
    pane.wait("the more row", |s| s.last().is_some_and(|r| *r == expand()));
    pane.click(10, pane.screen().len() as u16 - 1);
    let screen = pane.wait("the link's row", |s| s.iter().any(|r| r.contains("acme/api#12")));
    let expected = item_row_of(44, "acme/api#12 failing");
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
    pane.settled();
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
    pane.settled();
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
    // the title has no menu
    pane.wait("the menu closed", the_menu_is_closed);
    pane.right_click(10, item_row(&pane, "Inbox"));
    pane.settled();
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
