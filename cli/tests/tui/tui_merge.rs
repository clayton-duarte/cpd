//! The `merge` view (commands.md §13.2; hs-d273, hs-d473) on a private tmux server: the dialog's text, the keys and the
//! clicks that confirm or cancel, and what the confirm starts. The confirm closes the popup at once and starts
//! `cpd tui action merge <ref> --client <name>` with tmux `run-shell -b`: that action runs `pr merge` and then the poll
//! in the background, and opens the cockpit's one failure dialog when the merge fails. The fake `gh` of the env says
//! what ran (its recorded argv) and blocks `pr merge` until the test releases it; a fake `tmux` and a fake `cpd` record
//! what the view and the action ask of them. The click is raw mouse bytes sent to the pane.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use common::fx::Fx;
use common::{CPD, TestEnv, wait_until, wait_until_for};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use cpd_tui::merge;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const REF: &str = "o/r#7";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
/// The client the view was opened on, for the tests that have none attached.
const CLIENT: &str = "/dev/pts/9";
/// What `gh api graphql` does when the poll is to succeed and when it is to fail: the search lists nothing, and the direct
/// read of the stored open `o/r#7` the search missed (`p0: repository`) finds it merged (the merge the action just made).
const POLL_OK: &str = r#"case "$4" in *"p0: repository"*) echo '{"data":{"p0":{"pullRequest":{"repository":{"nameWithOwner":"o/r"},"number":7,"url":"https://github.com/o/r/pull/7","title":"Add the thing","isDraft":false,"state":"MERGED","createdAt":"t","mergedAt":"2026-10-20T10:00:00Z","reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","viewerDidAuthor":true,"viewerLatestReviewRequest":null,"commits":{"nodes":[]}}}}}'; exit 0;; esac; echo '{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}'; exit 0"#;
const POLL_FAILS: &str = "echo 'poll said no' >&2; exit 9";
/// How long the view may take to close once confirmed while `gh pr merge` blocks. The claim is that it does not wait
/// for `gh`, so the wait is for the view's own work only.
const CLOSES: Duration = Duration::from_secs(15);

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
    seed(&env);
    env
}

/// The pull request `o/r#7` in the env's store.
fn seed(env: &TestEnv) {
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
}

/// The file the fake `gh pr merge` waits for, when the test gives it one.
fn release_file(env: &TestEnv) -> PathBuf {
    env.root().join("release")
}

/// A `gh` whose `pr view` says OPEN with `merge_state`, whose `pr merge` first waits for `release` to exist (at most a
/// minute; none: it does not wait) and then exits `merge_exit` with `merge said no`, and whose `api graphql`, the poll,
/// runs `graphql` ([`POLL_OK`], [`POLL_FAILS`]).
fn gh_with(env: &TestEnv, merge_state: &str, merge_exit: i32, release: Option<&Path>, graphql: &str) {
    let wait = release.map_or(String::new(), |file| {
        format!("_i=0; while [ ! -e '{}' ] && [ $_i -lt 1200 ]; do _i=$((_i+1)); sleep 0.05; done; ", file.display())
    });
    env.fake_gh(&format!(
        "case \"$1 $2\" in\n\"pr view\") echo '{{\"state\":\"OPEN\",\"mergeStateStatus\":\"{merge_state}\",\"headRefOid\":\"{SHA}\",\"title\":\"Add the thing\"}}'; exit 0;;\n\"pr merge\") {wait}echo 'merge said no' >&2; exit {merge_exit};;\n\"api graphql\") {graphql};;\nesac\nexit 9"
    ));
}

/// A `gh` that merges at once; the poll fails.
fn gh(env: &TestEnv, merge_state: &str, merge_exit: i32) {
    gh_with(env, merge_state, merge_exit, None, POLL_FAILS);
}

fn release(env: &TestEnv) {
    fs::write(release_file(env), "").unwrap();
}

/// A `cpd` on `PATH` that records its calls and runs the built binary: the one tmux's `run-shell` and the popup run.
/// After a `tui action` it creates [`action_done`]: the positive signal that the background run ended.
fn fake_cpd(env: &TestEnv) {
    env.fake(
        "cpd",
        &format!(
            "case \"$1 $2\" in\n\"tui action\") '{CPD}' \"$@\"; _rc=$?; true > '{done}'; exit $_rc;;\nesac\nexec '{CPD}' \"$@\"",
            done = action_done(env).display()
        ),
    );
}

fn action_done(env: &TestEnv) -> PathBuf {
    env.root().join("action-done")
}

fn wait_action_done(env: &TestEnv) {
    wait_until("the background action to end", || action_done(env).exists());
}

/// The real `tmux`'s path.
fn real_tmux(env: &TestEnv) -> String {
    let out = env.command("sh").args(["-c", "command -v tmux"]).output().unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// A `tmux` on `PATH` that records its calls and runs the real one, after the shell code `before` (which sees `"$@"`).
fn fake_tmux_before(env: &TestEnv, before: &str) {
    env.fake("tmux", &format!("{before}\nexec '{}' \"$@\"", real_tmux(env)));
}

fn fake_tmux(env: &TestEnv) {
    fake_tmux_before(env, "");
}

/// The tmux calls that name `word` as one of their arguments.
fn tmux_calls(env: &TestEnv, word: &str) -> Vec<Vec<String>> {
    env.calls("tmux").into_iter().filter(|c| c.iter().any(|a| a == word)).collect()
}

/// The calls of `cpd tui action …` the fake `cpd` saw.
fn action_calls(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("cpd").into_iter().filter(|c| c[..2] == ["tui", "action"]).collect()
}

fn merges(env: &TestEnv) -> Vec<Vec<String>> {
    env.calls("gh").into_iter().filter(|c| c[..2] == ["pr", "merge"]).collect()
}

/// The `gh` calls' first words, in order: `pr view`, `pr merge`, `api graphql` ...
fn call_order(env: &TestEnv) -> Vec<String> {
    env.calls("gh").into_iter().map(|c| c[..2].join(" ")).collect()
}

/// The lines of the merge log, `$CPD_HOME/run/tui/merge.log`, without their time stamps.
fn merge_log(env: &TestEnv) -> Vec<String> {
    let text = fs::read_to_string(env.cpd_home().join("run/tui/merge.log")).unwrap_or_default();
    text.lines().map(|l| l.split_once(' ').expect("a time stamp, a space, the line").1.to_owned()).collect()
}

fn tmux_out(env: &TestEnv, args: &[&str]) -> String {
    let out = env.tmux(args);
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// `cpd tui view merge o/r#7 --client <client>` in a window of the popup, 60x8 (the dialog draws its own frame).
fn start_on(env: &TestEnv, client: &str) -> String {
    let args = [
        "new-session", "-d", "-s", "view", "-x", "60", "-y", "8", "-P", "-F", "#{pane_id}", CPD, "tui", "view", "merge", REF, "--client",
        client,
    ];
    tmux_out(env, &args).trim().to_owned()
}

fn start(env: &TestEnv) -> String {
    start_on(env, CLIENT)
}

fn screen(env: &TestEnv, pane: &str) -> Vec<String> {
    tmux_out(env, &["capture-pane", "-p", "-t", pane]).lines().map(|l| l.trim_end().to_owned()).collect()
}

fn wait_screen(env: &TestEnv, pane: &str, what: &str, done: impl FnMut(&[String]) -> bool) -> Vec<String> {
    common::wait_screen(what, || screen(env, pane), done)
}

fn wait_dialog(env: &TestEnv, pane: &str) -> Vec<String> {
    wait_screen(env, pane, "the dialog", |r| r.iter().any(|l| l.contains(&pill("Merge"))))
}

/// A pill: its label between the caps U+E0B6 and U+E0B4.
fn pill(label: &str) -> String {
    format!("\u{E0B6}{label}\u{E0B4}")
}

/// A row of the dialog's frame, 58 wide inside the bars.
fn inside(text: &str) -> String {
    format!("│{text:<58}│")
}

fn closed(env: &TestEnv) -> bool {
    !env.tmux(&["has-session", "-t", "view"]).status.success()
}

/// A left button press at 1-based `(column, row)` of the pane, as the terminal sends it (SGR mouse reporting). The
/// release is not sent: the view acts on the press, and may be gone by the time a release would arrive.
fn click(env: &TestEnv, pane: &str, column: u16, row: u16) {
    tmux_out(env, &["send-keys", "-t", pane, "-l", &format!("\x1b[<0;{column};{row}M")]);
}

/// The three ways to confirm: Enter, `y` and a click on the Merge pill (0-based columns 2 to 8, row 6).
fn confirm(env: &TestEnv, pane: &str, how: &str) {
    match how {
        "click" => click(env, pane, 5, 7),
        key => drop(tmux_out(env, &["send-keys", "-t", pane, key])),
    }
}

// ---------- a real client, for the tests that look at popups ----------

/// A second private tmux server whose one pane is a real client of the env's server (`tmux -L <socket> attach -t
/// cockpit`), so popups open and draw; killed on drop. (`tui.rs` has a twin of it.)
struct Outer<'a> {
    env: &'a TestEnv,
    socket: String,
}

impl<'a> Outer<'a> {
    /// Starts the session `cockpit` (the one the client shows) on the env's server, and attaches a client of
    /// `width` x `height`; returns it with the client's name.
    fn attach(env: &'a TestEnv, width: u16, height: u16) -> (Outer<'a>, String) {
        tmux_out(env, &["new-session", "-d", "-s", "cockpit", "-x", &width.to_string(), "-y", &height.to_string(), &format!("sleep {}", common::SLEEP_SECONDS)]);
        let outer = Outer { env, socket: format!("{}-outer", env.tmux_socket()) };
        let attach = format!("env -u TMUX tmux -L {} attach -t cockpit", env.tmux_socket());
        let (w, h) = (width.to_string(), height.to_string());
        outer.run(&["set", "-g", "status", "off", ";", "new-session", "-d", "-s", "o", "-x", &w, "-y", &h, &attach]);
        let mut client = String::new();
        wait_until("the client", || {
            client = tmux_out(env, &["list-clients", "-F", "#{client_name}"]).trim().to_owned();
            !client.is_empty()
        });
        (outer, client)
    }

    fn run(&self, args: &[&str]) -> String {
        let out = self.env.command("tmux").args(["-L", &self.socket, "-f", "/dev/null"]).args(args).output().unwrap();
        assert!(out.status.success(), "outer tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    }

    fn screen(&self) -> Vec<String> {
        self.run(&["capture-pane", "-p", "-t", "o"]).lines().map(|l| l.trim_end().to_owned()).collect()
    }

    fn wait_screen(&self, what: &str, done: impl FnMut(&[String]) -> bool) -> Vec<String> {
        common::wait_screen(what, || self.screen(), done)
    }

    fn keys(&self, keys: &[&str]) {
        self.run(&[&["send-keys", "-t", "o"][..], keys].concat());
    }
}

impl Drop for Outer<'_> {
    fn drop(&mut self) {
        let _ = self.env.command("tmux").args(["-L", &self.socket, "kill-server"]).stderr(Stdio::null()).status();
    }
}

fn has(rows: &[String], text: &str) -> bool {
    rows.iter().any(|l| l.contains(text))
}

// ---------- the dialog, before the confirm (unchanged) ----------

/// The dialog of `arg` as the view draws it in the 60x8 popup, over the store of `fx`: its rows, each without trailing blanks.
fn dialog_rows(fx: &Fx, arg: &str) -> Vec<String> {
    let mut store = fx.env.store();
    let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
    terminal.draw(|f| merge::draw_dialog(&mut store, f, arg).unwrap()).unwrap();
    let buf = terminal.backend().buffer();
    (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_owned()).collect()
}

#[test]
fn a_pull_request_the_store_does_not_have_shows_no_title() {
    let fx = Fx::new();
    seed(&fx.env);
    let rows = dialog_rows(&fx, "o/r#8");
    assert_eq!(rows[2..5], [inside(" o/r#8"), inside(""), inside(" method: squash")]);
}

#[test]
fn the_view_needs_a_terminal_a_pull_request_reference_and_a_client() {
    let env = env();
    let run = env.cpd(&["tui", "view", "merge", REF, "--client", CLIENT]);
    assert_eq!((run.exit, run.error()["code"].as_str()), (3, Some("not_a_terminal")), "{run:?}");
    for bad in [
        &["tui", "view", "merge", "--client", CLIENT][..],
        &["tui", "view", "merge", "o/r", "--client", CLIENT],
        &["tui", "view", "merge", "o/r#0", "--client", CLIENT],
        &["tui", "view", "merge", REF],
    ] {
        assert_eq!(env.cpd(bad).exit, 2, "{bad:?}");
    }
}

// ---------- the popup that opens it ----------

#[test]
fn the_popup_on_a_client_closes_at_once_on_the_confirm_while_gh_is_merging() {
    let env = env();
    gh_with(&env, "CLEAN", 0, Some(&release_file(&env)), POLL_OK);
    fake_cpd(&env);
    fake_tmux(&env);
    let (outer, client) = Outer::attach(&env, 120, 30);
    // what the opener runs: the popup on the client, over the cockpit (`display-popup` returns when the popup closes)
    let popup = merge::popup_args(&client, (120, 30), REF);
    let mut open = env.command("tmux");
    open.args(["-L", env.tmux_socket()]).args(&popup).stdout(Stdio::null()).stderr(Stdio::piped());
    let mut open = common::spawn(&mut open).unwrap();
    outer.wait_screen("the merge popup", |r| has(r, &pill("Merge")) && has(r, "method: squash"));
    outer.keys(&["y"]);

    // the popup is gone while `gh pr merge` still blocks: the merge runs in the background, owned by the server
    wait_until("pr merge to start in the background", || !merges(&env).is_empty());
    common::wait_until_for("the popup's command to end", CLOSES, || open.try_wait().unwrap().is_some());
    outer.wait_screen("the popup to be gone", |r| !has(r, &pill("Merge")) && !has(r, "merging"));
    assert_eq!(call_order(&env), ["pr view", "pr merge"], "the poll waits for the merge");
    assert!(!action_done(&env).exists());

    release(&env);
    wait_action_done(&env);
    let order = call_order(&env);
    assert_eq!(order[..3], ["pr view", "pr merge", "pr view"], "{order:?}");
    assert!(order.len() > 3 && order[3..].iter().all(|c| c == "api graphql"), "the poll ran after the merge: {order:?}");
    assert!(tmux_calls(&env, "display-popup").len() == 1, "a success opens no dialog: {:?}", tmux_calls(&env, "display-popup"));
    assert_eq!(merge_log(&env), Vec::<String>::new());
}

// ---------- the confirm ----------

#[test]
fn the_confirm_closes_the_view_at_once_and_the_merge_then_the_poll_run_in_the_background() {
    for how in ["y", "Enter", "click"] {
        let env = env();
        gh_with(&env, "CLEAN", 0, Some(&release_file(&env)), POLL_OK);
        fake_cpd(&env);
        fake_tmux(&env);
        // the view is the server's only session: without another one the server exits with it and the
        // pending `run-shell -b` job is lost (the real cockpit has other sessions)
        env.tmux(&["new-session", "-d", "-s", "keep", "sleep 120"]);
        let pane = start(&env);
        wait_dialog(&env, &pane);
        confirm(&env, &pane, how);

        // `gh pr merge` blocks until released: the view must be gone before it ends
        wait_until_for(&format!("{how}: the view to close while pr merge blocks"), CLOSES, || closed(&env));
        wait_until(&format!("{how}: pr merge to start"), || !merges(&env).is_empty());
        assert!(closed(&env), "{how}: the view is gone while pr merge blocks");
        assert_eq!(
            merges(&env),
            [["pr", "merge", "7", "--repo", "o/r", "--squash", "--delete-branch", "--match-head-commit", SHA].map(str::to_owned)],
            "{how}"
        );
        assert_eq!(call_order(&env), ["pr view", "pr merge"], "{how}: the poll waits for the merge");
        // the view started the background run with run-shell, and the command line reached `cpd` unchanged
        assert_eq!(action_calls(&env), [["tui", "action", "merge", REF, "--client", CLIENT].map(str::to_owned)], "{how}");
        assert_eq!(tmux_calls(&env, "run-shell").len(), 1, "{how}: {:?}", env.calls("tmux"));
        assert!(tmux_calls(&env, "run-shell")[0].iter().any(|a| a == "-b"), "{how}: in the background");

        release(&env);
        wait_action_done(&env);
        let order = call_order(&env);
        assert_eq!(order[..3], ["pr view", "pr merge", "pr view"], "{how}: {order:?}");
        assert!(order.len() > 3 && order[3..].iter().all(|c| c == "api graphql"), "{how}: the poll ran after the merge: {order:?}");
        // a success says nothing: no dialog, no message, no log line
        assert!(tmux_calls(&env, "display-popup").is_empty(), "{how}: {:?}", env.calls("tmux"));
        assert!(tmux_calls(&env, "display-message").is_empty(), "{how}: {:?}", env.calls("tmux"));
        assert_eq!(merge_log(&env), Vec::<String>::new(), "{how}");
    }
}

// ---------- the action: what it runs, and what a failure does ----------

/// `cpd tui action merge o/r#7 --client <client>`, which prints nothing and exits 0 whatever happens (§13.2).
fn action(env: &TestEnv, client: &str) {
    let run = env.cpd(&["tui", "action", "merge", REF, "--client", client]);
    assert_eq!((run.exit, run.stdout.as_str(), run.stderr.as_str()), (0, "", ""), "{run:?}");
}

#[test]
fn the_action_merges_then_polls_and_a_success_says_nothing() {
    let env = env();
    gh_with(&env, "CLEAN", 0, None, POLL_OK);
    fake_tmux(&env);
    action(&env, CLIENT);
    let order = call_order(&env);
    assert_eq!(order[..3], ["pr view", "pr merge", "pr view"], "{order:?}");
    assert!(order.len() > 3 && order[3..].iter().all(|c| c == "api graphql"), "{order:?}");
    assert_eq!(merges(&env).len(), 1);
    assert!(tmux_calls(&env, "display-popup").is_empty() && tmux_calls(&env, "display-message").is_empty(), "{:?}", env.calls("tmux"));
    assert_eq!(merge_log(&env), Vec::<String>::new());
}

#[test]
fn a_poll_that_fails_after_a_merge_is_a_log_line_and_no_dialog() {
    let env = env();
    gh_with(&env, "CLEAN", 0, None, POLL_FAILS);
    fake_tmux(&env);
    action(&env, CLIENT);
    // the pull request did merge: the poll ran after it, and its failure is a line of the log, nothing else
    assert_eq!(call_order(&env)[..4], ["pr view", "pr merge", "pr view", "api graphql"]);
    let log = merge_log(&env);
    assert_eq!(log.len(), 1, "{log:?}");
    assert!(log[0].starts_with("pr poll after merge o/r#7: gh_failed: "), "{log:?}");
    assert!(tmux_calls(&env, "display-popup").is_empty() && tmux_calls(&env, "display-message").is_empty(), "{:?}", env.calls("tmux"));
}

#[test]
fn a_refused_merge_opens_the_failure_dialog_on_the_client_logs_it_and_runs_no_poll() {
    // `pr merge` refuses (not CLEAN any more), or `gh pr merge` fails with `merge said no`
    let blocked = "merge o/r#7: merge_not_clean: o/r#7 is OPEN and BLOCKED: only an OPEN pull request that is CLEAN is merged.";
    for (state, exit, text, merge_calls) in [("BLOCKED", 0, blocked, 0), ("CLEAN", 1, "merge o/r#7: gh_failed: ", 1)] {
        let env = env();
        gh_with(&env, state, exit, None, POLL_OK);
        fake_cpd(&env);
        fake_tmux(&env);
        let (outer, client) = Outer::attach(&env, 120, 30);
        // the User confirms in the popup's view; the failure is the background action's
        let pane = start_on(&env, &client);
        wait_dialog(&env, &pane);
        tmux_out(&env, &["send-keys", "-t", &pane, "y"]);

        // the cockpit's one failure dialog, a `text` dialog of the kit, on that client: `<what failed>: <code>: <message>`
        let shown = outer.wait_screen("the failure dialog", |r| has(r, &text[..text.len().min(40)]));
        assert!(has(&shown, &text[..text.len().min(40)]), "{state}: {shown:?}");
        let popups = tmux_calls(&env, "display-popup");
        assert_eq!(popups.len(), 1, "{state}: one dialog: {popups:?}");
        assert!(popups[0].windows(2).any(|w| w == ["-c", &client]), "{state}: on the client: {popups:?}");
        assert!(popups[0].last().unwrap().starts_with("cpd tui view dialog text "), "{state}: a `text` dialog: {popups:?}");
        // the log line is there with the dialog: the same text
        let log = merge_log(&env);
        assert_eq!(log.len(), 1, "{state}: {log:?}");
        assert!(log[0].starts_with(text), "{state}: {log:?}");
        assert!(log[0].starts_with("merge o/r#7: ") && log[0].len() > "merge o/r#7: gh_failed: ".len(), "{state}: {log:?}");

        outer.keys(&["q"]);
        wait_action_done(&env);
        // a failed merge runs no poll
        assert_eq!(merges(&env).len(), merge_calls, "{state}");
        assert!(call_order(&env).iter().all(|c| c != "api graphql"), "{state}: {:?}", call_order(&env));
        assert_eq!(tmux_calls(&env, "display-popup").len(), 1, "{state}: never a second dialog");
    }
}

#[test]
fn when_the_dialog_cannot_open_the_log_line_is_first_then_the_client_gets_the_message() {
    let env = env();
    gh(&env, "BLOCKED", 0);
    // every tmux call notes whether the log line was already there; a `display-popup` fails (the dialog cannot open)
    let seen = env.root().join("tmux-saw-log");
    let log = env.cpd_home().join("run/tui/merge.log");
    fake_tmux_before(
        &env,
        &format!(
            "if [ -s '{log}' ]; then echo log >> '{seen}'; else echo no-log >> '{seen}'; fi\ncase \"$*\" in *display-popup*) echo 'no popup' >&2; exit 1;; esac",
            log = log.display(),
            seen = seen.display()
        ),
    );
    // no client of this name, no tmux server even: nothing can show the error, and the action still ends quietly
    action(&env, "/dev/pts/nobody");
    let text = "merge o/r#7: merge_not_clean: ";
    let log = merge_log(&env);
    assert_eq!(log.len(), 1, "{log:?}");
    assert!(log[0].starts_with(text), "{log:?}");
    assert!(call_order(&env).iter().all(|c| c != "api graphql"), "no poll: {:?}", call_order(&env));
    // after the dialog, the message on that client, as every `tui action` does for its own failures
    let messages = tmux_calls(&env, "display-message");
    assert_eq!(messages.len(), 1, "{:?}", env.calls("tmux"));
    assert!(messages[0].windows(2).any(|w| w == ["-c", "/dev/pts/nobody"]), "{messages:?}");
    assert!(messages[0].last().unwrap().contains(text), "{messages:?}");
    // the log line was written before the first tmux call of the action, so before the dialog was tried
    let saw = fs::read_to_string(&seen).unwrap();
    assert!(saw.lines().count() >= 1 && saw.lines().all(|l| l == "log"), "{saw:?}");
}

// ---------- the merge marker (hs-d495; commands.md §13.2 "A merge in progress", contract §1.4) ----------
//
// `cpd tui action merge` writes `$CPD_HOME/run/tui/merging/<owner>__<repo>__<n>.json` first, before `cpd pr merge`, and
// removes it when the request ends. The fake `gh` of these tests notes whether that marker exists at each of its calls.

/// The directory of the merge markers.
pub(crate) fn markers(env: &TestEnv) -> PathBuf {
    env.cpd_home().join("run/tui/merging")
}

/// The names of the files in it.
pub(crate) fn marker_files(env: &TestEnv) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(markers(env))
        .map(|entries| entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

/// A pid that is not alive: a child that has ended and been waited for.
pub(crate) fn dead_pid() -> u32 {
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

/// Writes the marker `file` (`<owner>__<repo>__<n>.json`) as the action does: `{"pid", "at"}`, by rename.
pub(crate) fn put_marker(env: &TestEnv, file: &str, pid: u32) {
    let dir = markers(env);
    fs::create_dir_all(&dir).unwrap();
    let temporary = dir.join(format!("{file}.tmp"));
    fs::write(&temporary, format!(r#"{{"pid": {pid}, "at": "2026-10-20T12:00:00Z"}}"#)).unwrap();
    fs::rename(&temporary, dir.join(file)).unwrap();
}

/// The marker of [`REF`].
fn marker(env: &TestEnv) -> PathBuf {
    markers(env).join("o__r__7.json")
}

/// What the fake `gh` noted for `call` (`view`, `merge`, `poll`): one `present` or `absent` line per call, by whether
/// [`marker`] existed then.
fn saw(env: &TestEnv, call: &str) -> Vec<String> {
    fs::read_to_string(env.root().join(format!("saw-{call}"))).unwrap_or_default().lines().map(str::to_owned).collect()
}

/// [`gh_with`] that notes, at each `pr view`, `pr merge` and `api graphql` (the poll), whether the marker exists
/// ([`saw`]) and, at `pr merge`, keeps a copy of its text in `<root>/saw-merge-text`.
fn gh_noting(env: &TestEnv, merge_state: &str, merge_exit: i32, release: Option<&Path>) {
    let wait = release.map_or(String::new(), |file| {
        format!("_i=0; while [ ! -e '{}' ] && [ $_i -lt 1200 ]; do _i=$((_i+1)); sleep 0.05; done; ", file.display())
    });
    env.fake_gh(&format!(
        "_m='{marker}'; _r='{root}'\nnote() {{ if [ -e \"$_m\" ]; then echo present >> \"$_r/saw-$1\"; else echo absent >> \"$_r/saw-$1\"; fi; }}\n\
         case \"$1 $2\" in\n\"pr view\") note view; echo '{{\"state\":\"OPEN\",\"mergeStateStatus\":\"{merge_state}\",\"headRefOid\":\"{SHA}\",\"title\":\"Add the thing\"}}'; exit 0;;\n\
         \"pr merge\") note merge; [ -e \"$_m\" ] && cp \"$_m\" \"$_r/saw-merge-text\"; {wait}echo 'merge said no' >&2; exit {merge_exit};;\n\
         \"api graphql\") note poll; {POLL_OK};;\nesac\nexit 9",
        marker = marker(env).display(),
        root = env.root().display()
    ));
}

/// `cpd tui action merge o/r#7 --client <client>`, running in the background; killed when dropped.
struct Running(std::process::Child);

impl Running {
    fn start(env: &TestEnv, client: &str) -> Running {
        let mut cmd = env.command(CPD);
        cmd.args(["tui", "action", "merge", REF, "--client", client]).stdout(Stdio::null()).stderr(Stdio::null());
        Running(common::spawn(&mut cmd).unwrap())
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }

    /// Waits for it to end; whether it exited 0.
    fn wait_ok(&mut self) -> bool {
        let mut status = None;
        wait_until("the action to end", || {
            status = self.0.try_wait().unwrap();
            status.is_some()
        });
        status.unwrap().success()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The marker's JSON: its `pid` and its `at`, which must be an RFC 3339 time, and nothing else.
fn marker_json(env: &TestEnv) -> serde_json::Value {
    let text = fs::read_to_string(marker(env)).unwrap_or_else(|e| panic!("the marker {}: {e}", marker(env).display()));
    let json: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{text:?}: {e}"));
    let at = json["at"].as_str().unwrap_or_else(|| panic!("an `at` string: {text}"));
    assert!(time::OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339).is_ok(), "{at:?} is RFC 3339");
    assert_eq!(json.as_object().unwrap().len(), 2, "pid and at only: {text}");
    json
}

#[test]
fn the_action_writes_the_marker_before_the_merge_keeps_it_through_the_merge_and_the_poll_and_removes_it_after_a_success() {
    let env = env();
    gh_noting(&env, "CLEAN", 0, Some(&release_file(&env)));
    fake_tmux(&env);
    let mut action = Running::start(&env, CLIENT);
    wait_until("pr merge to start", || !merges(&env).is_empty());
    // first: it was there at the first `gh` call of `pr merge`, and while `gh pr merge` runs it names this action
    assert_eq!(saw(&env, "view"), ["present"], "written before `cpd pr merge` runs anything");
    assert_eq!(saw(&env, "merge"), ["present"]);
    let json = marker_json(&env);
    assert_eq!(json["pid"], action.pid(), "the action's pid: {json}");
    assert_eq!(marker_files(&env), ["o__r__7.json"], "one file, and no temporary file beside it");

    release(&env);
    assert!(action.wait_ok(), "the action exits 0");
    assert!(!saw(&env, "poll").is_empty() && saw(&env, "poll").iter().all(|s| s == "present"), "the marker stays through the poll: {:?}", saw(&env, "poll"));
    assert_eq!(marker_files(&env), Vec::<String>::new(), "removed after the poll, and nothing left behind");
    assert_eq!(merge_log(&env), Vec::<String>::new(), "a success says nothing");
}

#[test]
fn a_refused_or_failed_merge_removes_the_marker_before_the_failure_dialog_opens() {
    for (state, exit) in [("BLOCKED", 0), ("CLEAN", 1)] {
        let env = env();
        gh_noting(&env, state, exit, None);
        // the client is there (`display -p -c` answers its size), and `display-popup` notes whether the marker exists when the
        // dialog is opened, and fails
        let marker = marker(&env);
        fake_tmux_before(
            &env,
            &format!(
                "case \"$*\" in *\"display -p -c\"*) echo '100 30'; exit 0;; *display-popup*) if [ -e '{m}' ]; then echo present >> '{r}/saw-popup'; else echo absent >> '{r}/saw-popup'; fi; echo 'no popup' >&2; exit 1;; esac",
                m = marker.display(),
                r = env.root().display()
            ),
        );
        action(&env, "/dev/pts/nobody");
        assert_eq!(saw(&env, "view"), ["present"], "{state}: written before `cpd pr merge` runs anything");
        assert_eq!(saw(&env, "popup"), ["absent"], "{state}: the dialog opens after the marker is gone");
        assert_eq!(marker_files(&env), Vec::<String>::new(), "{state}: no exit leaves it behind");
        assert!(call_order(&env).iter().all(|c| c != "api graphql"), "{state}: a failed merge runs no poll");
    }
}

#[test]
fn a_second_action_while_a_live_marker_exists_runs_no_merge_opens_no_dialog_and_logs_merge_already_running() {
    let env = env();
    gh_noting(&env, "CLEAN", 0, None);
    fake_tmux(&env);
    // a merge of o/r#7 is running: its marker names a live process (this one)
    put_marker(&env, "o__r__7.json", std::process::id());
    let before = fs::read_to_string(marker(&env)).unwrap();
    action(&env, CLIENT);
    assert_eq!(env.calls("gh"), Vec::<Vec<String>>::new(), "no `pr merge` and not a `gh` call at all: {:?}", env.calls("gh"));
    assert!(tmux_calls(&env, "display-popup").is_empty() && tmux_calls(&env, "display-message").is_empty(), "no dialog: {:?}", env.calls("tmux"));
    assert_eq!(fs::read_to_string(marker(&env)).unwrap(), before, "the running merge's marker is not the second action's to touch");
    let log = merge_log(&env);
    assert_eq!(log.len(), 1, "{log:?}");
    assert!(log[0].contains("merge already running"), "{log:?}");
}

#[test]
fn a_stale_marker_is_replaced_by_the_next_merge_action_and_another_pull_requests_marker_is_not_touched() {
    let env = env();
    gh_noting(&env, "CLEAN", 0, Some(&release_file(&env)));
    fake_tmux(&env);
    put_marker(&env, "o__r__7.json", dead_pid());
    put_marker(&env, "o__r__8.json", std::process::id());
    let other = fs::read_to_string(markers(&env).join("o__r__8.json")).unwrap();
    let mut action = Running::start(&env, CLIENT);
    wait_until("pr merge to start", || !merges(&env).is_empty());
    assert_eq!(marker_json(&env)["pid"], action.pid(), "the dead pid's marker is replaced by this action's");
    release(&env);
    assert!(action.wait_ok());
    assert_eq!(merges(&env).len(), 1, "the stale marker did not stop the merge");
    assert_eq!(marker_files(&env), ["o__r__8.json"], "its own marker is removed, the other one stays");
    assert_eq!(fs::read_to_string(markers(&env).join("o__r__8.json")).unwrap(), other);
}
