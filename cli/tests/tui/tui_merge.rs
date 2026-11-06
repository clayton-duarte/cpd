//! The merge action (commands.md §13.2; hs-d273, hs-d473; pro-d111; swr-github.md §9) in this process on `Fx::with_ports`: `cpd tui
//! action merge`, and the `pr merge` it runs, through `run_with`. The click starts `cpd tui action merge <ref> --client <name>` in
//! the background: that action runs `pr merge`, which queues the merge on the outbox (no `gh` call, no marker file, no poll), and
//! opens the cockpit's one failure dialog when the merge is refused. The spinner is the queued mutation.

use crate::common;

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use common::TestEnv;
use common::fx::Fx;
use cpd_core::host::runner::Reply;

const REF: &str = "o/r#7";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
/// The client the view was opened on.
const CLIENT: &str = "/dev/pts/9";

/// An `Fx` on fake ports with the config the views read and the pull request `o/r#7` cached `merge_state`.
fn fx_with(merge_state: &str) -> Fx {
    let fx = Fx::with_ports();
    let env = &fx.env;
    env.write_models();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\nconcurrency = 4\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.sql(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at,
                                    merge_state, base_ref, default_branch, head_oid)
           VALUES ('o/r', 7, 'https://github.com/o/r/pull/7', 'Add the thing', 0, 'open', 'approved', 'passed',
                   '2026-01-01T00:00:00.000Z', NULL, '2026-01-02T00:00:00.000Z', '{merge_state}', 'main', 'main', '{SHA}');
         INSERT INTO swr_repos (repo, default_branch, is_archived, auto_merge_allowed, squash_merge_allowed, merge_commit_allowed,
                                rebase_merge_allowed, delete_branch_on_merge, fetched_at)
           VALUES ('o/r', 'main', 0, 1, 1, 0, 0, 1, 't');"
    ));
    fx
}

fn fx() -> Fx {
    fx_with("clean")
}

fn naming(calls: Vec<Vec<String>>, word: &str) -> Vec<Vec<String>> {
    calls
        .into_iter()
        .filter(|c| c.iter().any(|a| a == word))
        .collect()
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

/// `cpd tui action merge o/r#7 --client <client>` in this process through `run_with` on the ports of `fx`: it exits 0 and
/// writes nothing to the output, whatever happens (§13.2).
fn act(fx: &Fx, client: &str) {
    fx.cpd_silent(&["tui", "action", "merge", REF, "--client", client]);
}

/// The merge mutations on the outbox: `(target_key, status)`.
fn queued(env: &TestEnv) -> Vec<(String, String)> {
    env.read(|c| {
        let mut stmt = c.prepare(
            "SELECT target_key, status FROM swr_mutations WHERE op_kind = 'merge' ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
}

/// Queues the merge of `pull` (`<owner>/<repo>#<n>`) in the home `home`, as `pr merge` does: the spinner of the Dispatch column.
pub(crate) fn queue_merge_at(home: &Path, pull: &str) {
    let home = cpd_core::home::CpdHome::resolve(Some(home.into()), None).unwrap();
    let mut store = cpd_core::store::Store::open(&home, 5000).unwrap();
    let op = cpd_core::swr::outbox::Op {
        system: "github".into(),
        target_key: format!("github:pr:{pull}"),
        op_kind: "merge".into(),
        vars: serde_json::json!({"expected_head_oid": SHA, "method": "squash"}),
        patch: Some(serde_json::json!({"state": "merging"})),
        context: None,
    };
    let now = cpd_core::clock::Clock::utc(&cpd_core::clock::ManualClock::new());
    cpd_core::swr::outbox::enqueue(
        &mut store,
        now,
        op,
        cpd_core::swr::outbox::Coalesce::LatestWins,
    )
    .unwrap();
}

/// The merge of every pull request in `home` is over: GitHub refused it (the mutation `failed`), so its spinner goes.
pub(crate) fn fail_merges_at(home: &Path) {
    let home = cpd_core::home::CpdHome::resolve(Some(home.into()), None).unwrap();
    let mut store = cpd_core::store::Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute(
                "UPDATE swr_mutations SET status = 'failed' WHERE op_kind = 'merge'",
                [],
            )?;
            Ok::<(), cpd_core::error::CpdError>(())
        })
        .unwrap();
}

#[test]
fn the_action_queues_the_merge_calls_no_gh_and_a_success_says_nothing() {
    let fx = fx();
    act(&fx, CLIENT);
    assert_eq!(
        queued(&fx.env),
        [("github:pr:o/r#7".to_owned(), "pending".to_owned())]
    );
    assert!(args_of(&fx, "gh").is_empty(), "{:?}", args_of(&fx, "gh"));
    let tmux = args_of(&fx, "tmux");
    assert!(
        naming(tmux.clone(), "display-popup").is_empty()
            && naming(tmux.clone(), "display-message").is_empty(),
        "{tmux:?}"
    );
    assert_eq!(merge_log(&fx.env), Vec::<String>::new());
}

/// An `Fx` like [`fx_with`]'s with the fake server of the cockpit and the client `client` attached (120 x 30), whose popups
/// write the outcome of a dialog the User closed (`confirmed`).
fn fx_on_client(client: &str, merge_state: &str) -> Fx {
    let fx = fx_with(merge_state);
    fx.cockpit();
    fx.tmux_route.fake().add_client(client, 120, 30);
    fx.tmux_route
        .fake()
        .on_popup(|_| Some(r#"{"confirmed":true}"#.to_owned()));
    fx
}

#[test]
fn a_refused_merge_opens_the_failure_dialog_on_the_client_logs_it_and_queues_nothing() {
    let text = "merge o/r#7: merge_not_clean: o/r#7 is open and blocked: only an open pull request that is clean is merged.";
    let fx = fx_on_client(CLIENT, "blocked");
    act(&fx, CLIENT);

    let popups = naming(args_of(&fx, "tmux"), "display-popup");
    assert_eq!(popups.len(), 1, "one dialog: {popups:?}");
    assert!(
        popups[0].windows(2).any(|w| w == ["-c", CLIENT]),
        "on the client: {popups:?}"
    );
    assert!(
        popups[0]
            .last()
            .unwrap()
            .starts_with("cpd tui view dialog text "),
        "a `text` dialog: {popups:?}"
    );
    assert!(
        popups[0]
            .last()
            .unwrap()
            .contains(&format!("--body='{text}")),
        "its text: {popups:?}"
    );
    let log = merge_log(&fx.env);
    assert_eq!(log.len(), 1, "{log:?}");
    assert!(log[0].starts_with(text), "{log:?}");
    assert!(queued(&fx.env).is_empty());
    assert!(args_of(&fx, "gh").is_empty());
    assert!(
        naming(args_of(&fx, "tmux"), "display-message").is_empty(),
        "the dialog opened, no message"
    );
}

#[test]
fn when_the_dialog_cannot_open_the_log_line_is_first_then_the_client_gets_the_message() {
    let fx = fx_with("blocked");
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

#[test]
fn a_refused_merge_leaves_the_stored_state_of_the_read() {
    let fx = fx_with("blocked");
    let row = "SELECT state || '|' || merge_state || '|' || changed_at FROM pull_requests WHERE number = 7";
    let before = fx.env.query::<String>(row);
    act(&fx, CLIENT);
    assert_eq!(fx.env.query::<String>(row), before);
}
