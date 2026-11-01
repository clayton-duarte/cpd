//! The merge action (commands.md §13.2; hs-d273, hs-d473; pro-d111) in this process on `Fx::with_ports`: `cpd tui action merge`,
//! and the `pr merge` and `pr poll` it runs, through `run_with`: `gh` is a `Handler` route of the `FakeRunner` that answers `pr
//! view`, `pr merge` and the poll, and notes whether the merge marker exists at each of its calls (no child process, no
//! polling). The click starts `cpd tui action merge <ref> --client <name>` in the background: that action runs `pr merge`
//! and then the poll, and opens the cockpit's one failure dialog when the merge fails.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::TestEnv;
use common::fx::Fx;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::host::spawn::{Launch, Pid, Spawn};
use cpd_core::store::Store;

const REF: &str = "o/r#7";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
/// The client the view was opened on.
const CLIENT: &str = "/dev/pts/9";
/// What `gh api graphql` does when the poll is to succeed and when it is to fail: the search lists nothing, and the direct
/// read of the stored open `o/r#7` the search missed (`p0: repository`) finds it merged (the merge the action just made).
/// The reply of the direct read of `o/r#7` (`p0: repository`): merged.
macro_rules! merged_lookup {
    () => {
        r#"{"data":{"p0":{"pullRequest":{"repository":{"nameWithOwner":"o/r"},"number":7,"url":"https://github.com/o/r/pull/7","title":"Add the thing","isDraft":false,"state":"MERGED","createdAt":"t","updatedAt":"U1","mergedAt":"2026-10-20T10:00:00Z","reviewDecision":null,"mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","viewerDidAuthor":true,"viewerLatestReviewRequest":null,"commits":{"nodes":[]}}}}}"#
    };
}
/// The reply of the search: no pull request.
macro_rules! empty_search {
    () => {
        r#"{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}"#
    };
}
/// An `Fx` on fake ports with the config the views read and the pull request `o/r#7` in its store.
fn fx() -> Fx {
    let fx = Fx::with_ports();
    configure(&fx.env);
    fx
}

/// The config the views read and the pull request `o/r#7` in `env`'s store.
fn configure(env: &TestEnv) {
    env.write_models();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\nconcurrency = 4\n\
             [tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    seed(env);
}

/// The pull request `o/r#7` in the env's store.
fn seed(env: &TestEnv) {
    let mut store = Store::open(
        &CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(),
        5000,
    )
    .unwrap();
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

fn naming(calls: Vec<Vec<String>>, word: &str) -> Vec<Vec<String>> {
    calls
        .into_iter()
        .filter(|c| c.iter().any(|a| a == word))
        .collect()
}

fn merges_of(gh_calls: Vec<Vec<String>>) -> Vec<Vec<String>> {
    gh_calls
        .into_iter()
        .filter(|c| c[..2] == ["pr", "merge"])
        .collect()
}

fn order_of(gh_calls: Vec<Vec<String>>) -> Vec<String> {
    gh_calls.into_iter().map(|c| c[..2].join(" ")).collect()
}

/// The arguments of every call of `program` the `FakeRunner` of `fx` saw, in order.
fn args_of(fx: &Fx, program: &str) -> Vec<Vec<String>> {
    fx.calls(program)
        .into_iter()
        .map(|call| call.args)
        .collect()
}

/// The lines of the merge log, `$CPD_HOME/run/tui/merge.log`, without their time stamps.
fn merge_log(env: &TestEnv) -> Vec<String> {
    let text = fs::read_to_string(env.cpd_home().join("run/tui/merge.log")).unwrap_or_default();
    text.lines()
        .map(|l| {
            l.split_once(' ')
                .expect("a time stamp, a space, the line")
                .1
                .to_owned()
        })
        .collect()
}

// ---------- the action: what it runs, and what a failure does ----------

/// `cpd tui action merge o/r#7 --client <client>` in this process through `run_with` on the ports of `fx`: it exits 0 and
/// writes nothing to the output, whatever happens (§13.2).
fn act(fx: &Fx, client: &str) {
    fx.cpd_silent(&["tui", "action", "merge", REF, "--client", client]);
}

/// What `gh api graphql`, the poll, answers.
#[derive(Clone, Copy)]
enum Poll {
    /// The search lists nothing and the direct read of `o/r#7` finds it merged.
    Merged,
    /// `poll said no`, exit 9.
    Fails,
}

/// The fake `gh` of an action: `pr view` says OPEN and CLEAN, `pr merge` merges, `api graphql` answers `poll`. At each
/// `pr view`, `pr merge` and poll call it notes whether the merge marker file exists then, and at the first `pr merge` it
/// keeps the marker's text and the names of the files beside it: the check that a marker is there while `gh` runs is made
/// in the reply, at the moment of the call.
struct Gh {
    poll: Poll,
    /// `mergeStateStatus` of the `pr view`: `CLEAN` merges, anything else is refused.
    merge_state: &'static str,
    /// The exit code of `gh pr merge`; not 0 prints `merge said no`.
    merge_exit: i32,
    /// The marker of [`REF`] and the directory of the markers.
    marker: PathBuf,
    markers: PathBuf,
    /// The calls noted, in order: `view`, `merge` or `poll`, and whether the marker existed.
    seen: Mutex<Vec<(&'static str, bool)>>,
    /// At the first `pr merge`: the marker's text and the files of the marker directory.
    at_merge: Mutex<Option<(String, Vec<String>)>>,
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let words: Vec<&str> = call.args.iter().map(String::as_str).collect();
        let present = self.marker.exists();
        let note = |what| self.seen.lock().unwrap().push((what, present));
        match words[..] {
            ["pr", "view", ..] => {
                note("view");
                Reply::ok(&format!(
                    "{{\"state\":\"OPEN\",\"mergeStateStatus\":\"{}\",\"headRefOid\":\"{SHA}\",\"title\":\"Add the thing\"}}",
                    self.merge_state
                ))
            }
            ["pr", "merge", ..] => {
                note("merge");
                let mut at_merge = self.at_merge.lock().unwrap();
                if at_merge.is_none() && present {
                    *at_merge = Some((
                        fs::read_to_string(&self.marker).unwrap(),
                        files_in(&self.markers),
                    ));
                }
                match self.merge_exit {
                    0 => Reply::ok(""),
                    code => Reply::exit(code, "", "merge said no\n"),
                }
            }
            ["repo", "view", _, _, json] if json.contains("squashMergeAllowed") => Reply::ok(
                "{\"squashMergeAllowed\":true,\"mergeCommitAllowed\":false,\"rebaseMergeAllowed\":false}\n",
            ),
            ["repo", "view", ..] => Reply::ok("{\"deleteBranchOnMerge\":true}\n"),
            ["api", "graphql", _, query, ..] => {
                note("poll");
                match self.poll {
                    Poll::Merged if query.contains("p0: repository") => Reply::ok(merged_lookup!()),
                    Poll::Merged => Reply::ok(empty_search!()),
                    Poll::Fails => Reply::exit(9, "", "poll said no\n"),
                }
            }
            _ => Reply::exit(9, "", ""),
        }
    }
}

/// Makes the `gh` of `fx` the [`Gh`] answering the poll as `poll` says; the handler, to read what it noted.
fn gh_route(fx: &Fx, poll: Poll) -> Arc<Gh> {
    gh_route_with(fx, poll, "CLEAN", 0)
}

/// [`gh_route`] whose `pr view` says OPEN with `merge_state` and whose `pr merge` exits `merge_exit`.
fn gh_route_with(fx: &Fx, poll: Poll, merge_state: &'static str, merge_exit: i32) -> Arc<Gh> {
    let gh = Arc::new(Gh {
        poll,
        merge_state,
        merge_exit,
        marker: marker(&fx.env),
        markers: markers(&fx.env),
        seen: Mutex::default(),
        at_merge: Mutex::default(),
    });
    let _ = (*fx.fakes.runner).clone().route("gh", gh.clone());
    gh
}

#[test]
fn the_action_merges_then_polls_and_a_success_says_nothing() {
    let fx = fx();
    gh_route(&fx, Poll::Merged);
    act(&fx, CLIENT);
    let order = order_of(args_of(&fx, "gh"));
    assert_eq!(
        order[..5],
        ["pr view", "repo view", "pr merge", "pr view", "repo view"],
        "{order:?}"
    );
    assert!(
        order.len() > 5 && order[5..].iter().all(|c| c == "api graphql"),
        "{order:?}"
    );
    assert_eq!(merges_of(args_of(&fx, "gh")).len(), 1);
    let tmux = args_of(&fx, "tmux");
    assert!(
        naming(tmux.clone(), "display-popup").is_empty()
            && naming(tmux.clone(), "display-message").is_empty(),
        "{tmux:?}"
    );
    assert_eq!(merge_log(&fx.env), Vec::<String>::new());
}

#[test]
fn a_poll_that_fails_after_a_merge_is_a_log_line_and_no_dialog() {
    let fx = fx();
    gh_route(&fx, Poll::Fails);
    act(&fx, CLIENT);
    // the pull request did merge: the poll ran after it, and its failure is a line of the log, nothing else
    assert_eq!(
        order_of(args_of(&fx, "gh"))[..6],
        [
            "pr view",
            "repo view",
            "pr merge",
            "pr view",
            "repo view",
            "api graphql"
        ]
    );
    let log = merge_log(&fx.env);
    assert_eq!(log.len(), 1, "{log:?}");
    assert!(
        log[0].starts_with("pr poll after merge o/r#7: gh_failed: "),
        "{log:?}"
    );
    let tmux = args_of(&fx, "tmux");
    assert!(
        naming(tmux.clone(), "display-popup").is_empty()
            && naming(tmux.clone(), "display-message").is_empty(),
        "{tmux:?}"
    );
}

/// An `Fx` like [`fx`]'s with the fake server of the cockpit and the client `client` attached (120 x 30), whose popups
/// write the outcome of a dialog the User closed (`confirmed`).
fn fx_on_client(client: &str) -> Fx {
    let fx = fx();
    fx.cockpit();
    fx.tmux_route.fake().add_client(client, 120, 30);
    fx.tmux_route
        .fake()
        .on_popup(|_| Some(r#"{"confirmed":true}"#.to_owned()));
    fx
}

#[test]
fn a_refused_merge_opens_the_failure_dialog_on_the_client_logs_it_and_runs_no_poll() {
    // `pr merge` refuses (not CLEAN any more), or `gh pr merge` fails with `merge said no`
    let blocked = "merge o/r#7: merge_not_clean: o/r#7 is OPEN and BLOCKED: only an OPEN pull request that is CLEAN is merged.";
    for (state, exit, text, merge_calls) in [
        ("BLOCKED", 0, blocked, 0),
        ("CLEAN", 1, "merge o/r#7: gh_failed: ", 1),
    ] {
        let fx = fx_on_client(CLIENT);
        gh_route_with(&fx, Poll::Merged, state, exit);
        // the failure is the background action's: the User's confirm in the popup's view started it (`run-shell -b`)
        act(&fx, CLIENT);

        // the cockpit's one failure dialog, a `text` dialog of the kit, on that client: `<what failed>: <code>: <message>`
        let popups = naming(args_of(&fx, "tmux"), "display-popup");
        assert_eq!(popups.len(), 1, "{state}: one dialog: {popups:?}");
        assert!(
            popups[0].windows(2).any(|w| w == ["-c", CLIENT]),
            "{state}: on the client: {popups:?}"
        );
        assert!(
            popups[0]
                .last()
                .unwrap()
                .starts_with("cpd tui view dialog text "),
            "{state}: a `text` dialog: {popups:?}"
        );
        assert!(
            popups[0]
                .last()
                .unwrap()
                .contains(&format!("--body='{text}")),
            "{state}: its text: {popups:?}"
        );
        // the log line is there with the dialog: the same text
        let log = merge_log(&fx.env);
        assert_eq!(log.len(), 1, "{state}: {log:?}");
        assert!(log[0].starts_with(text), "{state}: {log:?}");
        assert!(
            log[0].starts_with("merge o/r#7: ") && log[0].len() > "merge o/r#7: gh_failed: ".len(),
            "{state}: {log:?}"
        );

        // a failed merge runs no poll
        assert_eq!(merges_of(args_of(&fx, "gh")).len(), merge_calls, "{state}");
        let order = order_of(args_of(&fx, "gh"));
        assert!(
            order.iter().all(|c| c != "api graphql"),
            "{state}: {order:?}"
        );
        assert_eq!(
            naming(args_of(&fx, "tmux"), "display-popup").len(),
            1,
            "{state}: never a second dialog"
        );
        assert!(
            naming(args_of(&fx, "tmux"), "display-message").is_empty(),
            "{state}: the dialog opened, no message"
        );
    }
}

#[test]
fn when_the_dialog_cannot_open_the_log_line_is_first_then_the_client_gets_the_message() {
    let fx = fx();
    gh_route_with(&fx, Poll::Merged, "BLOCKED", 0);
    // every tmux call notes whether the log line was already there (a rule that matches nothing, so the route answers);
    // a `display-popup` fails (the dialog cannot open)
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = fx.env.cpd_home().join("run/tui/merge.log");
    let noted = seen.clone();
    let runner = (*fx.fakes.runner).clone();
    let _ = runner
        .on(
            "tmux",
            move |_| {
                noted
                    .lock()
                    .unwrap()
                    .push(fs::metadata(&log).is_ok_and(|m| m.len() > 0));
                false
            },
            Reply::ok(""),
        )
        .on(
            "tmux",
            |call| call.args.iter().any(|a| a == "display-popup"),
            Reply::exit(1, "", "no popup\n"),
        );
    // no client of this name, no tmux server even: nothing can show the error, and the action still ends quietly
    act(&fx, "/dev/pts/nobody");
    let text = "merge o/r#7: merge_not_clean: ";
    let log = merge_log(&fx.env);
    assert_eq!(log.len(), 1, "{log:?}");
    assert!(log[0].starts_with(text), "{log:?}");
    let order = order_of(args_of(&fx, "gh"));
    assert!(
        order.iter().all(|c| c != "api graphql"),
        "no poll: {order:?}"
    );
    // after the dialog, the message on that client, as every `tui action` does for its own failures
    let messages = naming(args_of(&fx, "tmux"), "display-message");
    assert_eq!(messages.len(), 1, "{:?}", args_of(&fx, "tmux"));
    assert!(
        messages[0]
            .windows(2)
            .any(|w| w == ["-c", "/dev/pts/nobody"]),
        "{messages:?}"
    );
    assert!(messages[0].last().unwrap().contains(text), "{messages:?}");
    // the log line was written before the first tmux call of the action, so before the dialog was tried
    let seen = seen.lock().unwrap();
    assert!(
        !seen.is_empty() && seen.iter().all(|logged| *logged),
        "{seen:?}"
    );
}

// ---------- the merge marker (hs-d495; commands.md §13.2 "A merge in progress", contract §1.4) ----------
//
// `cpd tui action merge` writes `$CPD_HOME/run/tui/merging/<owner>__<repo>__<n>.json` first, before `cpd pr merge`, and
// removes it when the request ends. The fake `gh` of these tests notes whether that marker exists at each of its calls.

/// The directory of the merge markers.
pub(crate) fn markers(env: &TestEnv) -> PathBuf {
    markers_at(&env.cpd_home())
}

/// The directory of the merge markers of the home `home`.
pub(crate) fn markers_at(home: &Path) -> PathBuf {
    home.join("run/tui/merging")
}

/// The names of the files in it.
pub(crate) fn marker_files(env: &TestEnv) -> Vec<String> {
    files_in(&markers(env))
}

/// The sorted names of the files of `dir`; none when it is not there.
fn files_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// Writes the marker `file` (`<owner>__<repo>__<n>.json`) as the action does: `{"pid", "at"}`, by rename.
pub(crate) fn put_marker(env: &TestEnv, file: &str, pid: u32) {
    put_marker_at(&env.cpd_home(), file, pid);
}

/// [`put_marker`] in the home `home`.
pub(crate) fn put_marker_at(home: &Path, file: &str, pid: u32) {
    let dir = markers_at(home);
    fs::create_dir_all(&dir).unwrap();
    let temporary = dir.join(format!("{file}.tmp"));
    fs::write(
        &temporary,
        format!(r#"{{"pid": {pid}, "at": "2026-10-20T12:00:00Z"}}"#),
    )
    .unwrap();
    fs::rename(&temporary, dir.join(file)).unwrap();
}

/// The marker of [`REF`].
fn marker(env: &TestEnv) -> PathBuf {
    markers(env).join("o__r__7.json")
}

/// The marker's JSON in `text`: its `pid` and its `at`, which must be an RFC 3339 time, and nothing else.
fn marker_json(text: &str) -> serde_json::Value {
    let json: serde_json::Value =
        serde_json::from_str(text).unwrap_or_else(|e| panic!("{text:?}: {e}"));
    let at = json["at"]
        .as_str()
        .unwrap_or_else(|| panic!("an `at` string: {text}"));
    assert!(
        time::OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339).is_ok(),
        "{at:?} is RFC 3339"
    );
    assert_eq!(
        json.as_object().unwrap().len(),
        2,
        "pid and at only: {text}"
    );
    json
}

/// What [`Gh`] noted for `call` (`view`, `merge`, `poll`): one `present` or `absent` per call, by whether the marker existed then.
fn noted(gh: &Gh, call: &str) -> Vec<&'static str> {
    gh.seen
        .lock()
        .unwrap()
        .iter()
        .filter(|(what, _)| *what == call)
        .map(|(_, present)| if *present { "present" } else { "absent" })
        .collect()
}

/// A process the `FakeSpawn` of `fx` runs: its pid is alive.
pub(crate) fn live_pid(fx: &Fx) -> u32 {
    let launch = Launch {
        program: "cpd".into(),
        args: vec!["pr".into(), "merge".into()],
        ..Launch::default()
    };
    fx.fakes.spawn.spawn(&launch).unwrap().0
}

/// A pid that is not alive: a process of the `FakeSpawn` that has ended.
pub(crate) fn ended_pid(fx: &Fx) -> u32 {
    let pid = live_pid(fx);
    fx.fakes.spawn.exit(Pid(pid), 0);
    pid
}

#[test]
fn the_action_writes_the_marker_before_the_merge_keeps_it_through_the_merge_and_the_poll_and_removes_it_after_a_success()
 {
    let fx = fx();
    let gh = gh_route(&fx, Poll::Merged);
    act(&fx, CLIENT);
    // first: it was there at the first `gh` call of `pr merge`, and while `gh pr merge` runs it names this action
    let order: Vec<_> = gh
        .seen
        .lock()
        .unwrap()
        .iter()
        .map(|(what, _)| *what)
        .collect();
    assert_eq!(order[..2], ["view", "merge"], "{order:?}");
    assert_eq!(
        noted(&gh, "view")[..1],
        ["present"],
        "written before `cpd pr merge` runs anything"
    );
    assert_eq!(noted(&gh, "merge"), ["present"]);
    let (text, files) = gh
        .at_merge
        .lock()
        .unwrap()
        .clone()
        .expect("the marker was there while `gh pr merge` ran");
    let json = marker_json(&text);
    assert_eq!(json["pid"], std::process::id(), "the action's pid: {json}");
    assert_eq!(
        files,
        ["o__r__7.json"],
        "one file, and no temporary file beside it"
    );

    assert!(
        !noted(&gh, "poll").is_empty() && noted(&gh, "poll").iter().all(|s| *s == "present"),
        "the marker stays through the poll: {:?}",
        noted(&gh, "poll")
    );
    assert_eq!(
        marker_files(&fx.env),
        Vec::<String>::new(),
        "removed after the poll, and nothing left behind"
    );
    assert_eq!(
        merge_log(&fx.env),
        Vec::<String>::new(),
        "a success says nothing"
    );
}

#[test]
fn a_refused_or_failed_merge_removes_the_marker_before_the_failure_dialog_opens() {
    for (state, exit) in [("BLOCKED", 0), ("CLEAN", 1)] {
        let fx = fx();
        let gh = gh_route_with(&fx, Poll::Merged, state, exit);
        // the client is there (`display -p -c` answers its size), and `display-popup` notes whether the marker exists when the
        // dialog is opened, and fails
        fx.cockpit();
        fx.tmux_route.fake().add_client("/dev/pts/nobody", 100, 30);
        let popup_saw = Arc::new(Mutex::new(Vec::new()));
        let (note, marker) = (popup_saw.clone(), marker(&fx.env));
        let _ = (*fx.fakes.runner).clone().on(
            "tmux",
            move |call| {
                let popup = call.args.iter().any(|a| a == "display-popup");
                if popup {
                    note.lock()
                        .unwrap()
                        .push(if marker.exists() { "present" } else { "absent" });
                }
                popup
            },
            Reply::exit(1, "", "no popup\n"),
        );
        act(&fx, "/dev/pts/nobody");
        assert_eq!(
            noted(&gh, "view"),
            ["present"],
            "{state}: written before `cpd pr merge` runs anything"
        );
        assert_eq!(
            *popup_saw.lock().unwrap(),
            ["absent"],
            "{state}: the dialog opens after the marker is gone"
        );
        assert_eq!(
            marker_files(&fx.env),
            Vec::<String>::new(),
            "{state}: no exit leaves it behind"
        );
        let order = order_of(args_of(&fx, "gh"));
        assert!(
            order.iter().all(|c| c != "api graphql"),
            "{state}: a failed merge runs no poll: {order:?}"
        );
    }
}

#[test]
fn a_second_action_while_a_live_marker_exists_runs_no_merge_opens_no_dialog_and_logs_merge_already_running()
 {
    let fx = fx();
    gh_route(&fx, Poll::Merged);
    // a merge of o/r#7 is running: its marker names a live process
    put_marker(&fx.env, "o__r__7.json", live_pid(&fx));
    let before = fs::read_to_string(marker(&fx.env)).unwrap();
    act(&fx, CLIENT);
    assert_eq!(
        args_of(&fx, "gh"),
        Vec::<Vec<String>>::new(),
        "no `pr merge` and not a `gh` call at all: {:?}",
        args_of(&fx, "gh")
    );
    let tmux = args_of(&fx, "tmux");
    assert!(
        naming(tmux.clone(), "display-popup").is_empty()
            && naming(tmux.clone(), "display-message").is_empty(),
        "no dialog: {tmux:?}"
    );
    assert_eq!(
        fs::read_to_string(marker(&fx.env)).unwrap(),
        before,
        "the running merge's marker is not the second action's to touch"
    );
    let log = merge_log(&fx.env);
    assert_eq!(log.len(), 1, "{log:?}");
    assert!(log[0].contains("merge already running"), "{log:?}");
}

#[test]
fn a_stale_marker_is_replaced_by_the_next_merge_action_and_another_pull_requests_marker_is_not_touched()
 {
    let fx = fx();
    let gh = gh_route(&fx, Poll::Merged);
    put_marker(&fx.env, "o__r__7.json", ended_pid(&fx));
    put_marker(&fx.env, "o__r__8.json", live_pid(&fx));
    let other = fs::read_to_string(markers(&fx.env).join("o__r__8.json")).unwrap();
    act(&fx, CLIENT);
    let (text, _) = gh
        .at_merge
        .lock()
        .unwrap()
        .clone()
        .expect("the marker was there while `gh pr merge` ran");
    assert_eq!(
        marker_json(&text)["pid"],
        std::process::id(),
        "the dead pid's marker is replaced by this action's"
    );
    assert_eq!(
        merges_of(args_of(&fx, "gh")).len(),
        1,
        "the stale marker did not stop the merge"
    );
    assert_eq!(
        marker_files(&fx.env),
        ["o__r__8.json"],
        "its own marker is removed, the other one stays"
    );
    assert_eq!(
        fs::read_to_string(markers(&fx.env).join("o__r__8.json")).unwrap(),
        other
    );
}
