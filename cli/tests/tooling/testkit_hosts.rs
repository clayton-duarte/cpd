//! The git and tmux fixtures of `common::fx::Fx` over the in-process routes of the `FakeRunner` (docs/implementing.md,
//! "The git and tmux fixtures of `Fx`"): `git_repo`, `keyed_repo`, `local_origin`, `git` and `commit_file` go through the
//! git route, `tmux`, `tmux_state`, `cockpit`, `panes` and `fake_cpd_views` through the tmux route. The names are
//! `TestEnv`'s, so a migration is a rename.
//!
//! Every test here is in-process: none starts a process, runs real git or tmux, sleeps or reads the wall clock.
//!
//! Assumptions where the Spec left a doubt: `cockpit()` builds session `cpd`, window `main` with three panes
//! (`@cpd_view` = `tree`, `plan`, `chat`, in pane order) and no `@cpd_session`; `panes()` returns the rows of
//! `common::fx::PaneRow`, from the state, in pane order; `commit_file` returns nothing; `fx.tmux` returns an
//! `Output` like `TestEnv::tmux`.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::Duration;

use cpd_core::host::runner::{Call, Reply};
use cpd_core::host::tmux::SERVER_OPTIONS;
use nix::errno::Errno;
use nix::sys::wait::{WaitPidFlag, waitpid};

use crate::common::fx::Fx;

const CONFIG: &str = "version = 1\nmachine = \"m1\"\n";
const TESTS_TABLE: &str = "[test-commands]\nbuild = \"true\"\nrun = \"true\"\n";

/// An `Fx` on fake ports whose `$CPD_HOME` has the config every command but `config` needs.
fn fx() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_config(CONFIG);
    fx
}

/// An `Fx` on fake ports whose config names the env's private tmux socket (`[tmux] socket`).
fn tmux_fx() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_tmux_config("");
    fx
}

/// The text of the panic `f` raises; fails when it does not panic.
fn panics(f: impl FnOnce()) -> String {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(()) => panic!("expected a panic, there was none"),
        Err(payload) => message(payload),
    }
}

fn message(payload: Box<dyn Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(text) => *text,
        Err(payload) => payload
            .downcast::<&str>()
            .map(|text| (*text).to_owned())
            .unwrap_or_default(),
    }
}

/// A call of `program` with `args`, as a `Runner` receives it from the code under test.
fn call(program: &str, args: &[&str]) -> Call {
    Call {
        tool: program.to_owned(),
        program: program.to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        envs: Vec::new(),
        cwd: None,
        deadline: Duration::from_secs(30),
        stdin: None,
        stderr_to: None,
    }
}

/// No child of this process is alive or waiting: nothing the test did started a process.
fn no_child() {
    assert_eq!(
        waitpid(None, Some(WaitPidFlag::WNOHANG)),
        Err(Errno::ECHILD),
        "a child process lived"
    );
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).expect("UTF-8")
}

/// The subjects of the commits of `dir`, newest first.
fn subjects(fx: &Fx, dir: &Path) -> Vec<String> {
    fx.git(dir, &["log", "--format=%s"])
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A bare repo of the Universe at `<root>/<name>.git`, made through the route.
fn bare(fx: &Fx, name: &str) -> PathBuf {
    let origin = fx.env.root().join(format!("{name}.git"));
    fx.git(
        fx.env.root(),
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    );
    origin
}

/// Context `ctx` (owner `acme`) with Plan `pay-1` in `grounding`, and the repo policy of `acme/pay`: what `pr add` needs.
fn plan_pay(fx: &Fx) {
    fx.env.repo_policy("acme", "pay", TESTS_TABLE);
    fx.env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay-1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pay-1', 'ctx', 'Pay', 'grounding', 'lead1', '1999-12-31T00:00:00.000Z');",
    );
}

/// `cpd pr add web` on `repo`, in Plan `pay-1`, in this process.
fn pr_add(fx: &Fx, repo: &Path) -> crate::common::Run {
    fx.cpd(&["workflow", "add", "web", "--repo", repo.to_str().unwrap(), "--title", "Change web", "--shape", "config", "--plan", "pay-1"])
}

// ---- git_repo

#[test]
fn a_git_repo_is_a_directory_of_the_env_named_by_the_fixture() {
    let fx = fx();

    let repo = fx.git_repo("pay");

    assert_eq!(repo, fx.env.root().join("repos").join("pay"));
}

#[test]
fn a_git_repo_has_its_files_in_a_real_temp_dir() {
    let fx = fx();

    let repo = fx.git_repo("pay");

    assert_eq!(
        std::fs::read_to_string(repo.join("README.md")).unwrap(),
        "# pay\n"
    );
}

#[test]
fn a_git_repo_is_on_main_with_one_commit() {
    let fx = fx();
    let repo = fx.git_repo("pay");

    assert_eq!(fx.git(&repo, &["symbolic-ref", "--short", "HEAD"]), "main");
    assert_eq!(subjects(&fx, &repo), ["initial"]);
}

#[test]
fn a_git_repo_has_no_real_git_directory() {
    let fx = fx();

    let repo = fx.git_repo("pay");

    for name in ["objects", "refs", "config", "HEAD"] {
        assert!(
            !repo.join(".git").join(name).exists(),
            ".git/{name} exists: a real repo that real git could read"
        );
    }
}

#[test]
fn a_git_repo_is_made_with_no_process_and_no_call_of_git_outside_the_route() {
    let fx = fx();

    fx.git_repo("pay");

    no_child();
    assert!(
        fx.fakes.runner.unscripted().is_empty(),
        "{:?}",
        fx.fakes.runner.unscripted()
    );
}

// ---- Fx::git

#[test]
fn git_returns_the_trimmed_stdout_of_the_route() {
    let fx = fx();
    let repo = fx.git_repo("pay");

    let out = fx.git(&repo, &["log", "-1", "--format=%s"]);

    assert_eq!(out, "initial", "no trailing newline");
}

#[test]
fn git_runs_through_the_route_with_the_directory_as_the_cwd() {
    let fx = fx();
    let repo = fx.git_repo("pay");
    let before = fx.calls("git").len();

    fx.git(&repo, &["symbolic-ref", "--short", "HEAD"]);

    let calls = fx.calls("git");
    assert_eq!(calls.len(), before + 1, "{calls:?}");
    let last = calls.last().unwrap();
    assert_eq!(last.args, ["symbolic-ref", "--short", "HEAD"]);
    assert_eq!(last.cwd.as_deref(), Some(repo.as_path()));
}

#[test]
fn git_fails_the_test_naming_the_args_when_the_route_exits_non_zero() {
    let fx = fx();
    let repo = fx.git_repo("pay");

    let text = panics(|| {
        fx.git(&repo, &["rev-parse", "--verify", "refs/heads/nope"]);
    });

    assert!(text.contains("rev-parse"), "{text}");
    assert!(
        text.contains("fatal"),
        "the message holds the route's stderr: {text}"
    );
}

// ---- commit_file

#[test]
fn commit_file_writes_the_file_in_the_working_tree() {
    let fx = fx();
    let repo = fx.git_repo("pay");

    fx.commit_file(&repo, "src/lib.rs", "pub fn pay() {}\n", "add lib");

    assert_eq!(
        std::fs::read_to_string(repo.join("src/lib.rs")).unwrap(),
        "pub fn pay() {}\n"
    );
}

#[test]
fn commit_file_commits_it_on_the_branch_with_the_message() {
    let fx = fx();
    let repo = fx.git_repo("pay");

    fx.commit_file(&repo, "src/lib.rs", "pub fn pay() {}\n", "add lib");

    assert_eq!(subjects(&fx, &repo), ["add lib", "initial"]);
    assert_eq!(
        fx.git(&repo, &["status", "--porcelain"]),
        "",
        "the tree is clean after the commit"
    );
}

#[test]
fn a_commit_made_through_the_fixture_is_seen_by_pr_add_in_one_library_call() {
    let fx = fx();
    plan_pay(&fx);
    let repo = fx.keyed_repo("pay").canonicalize().unwrap();
    fx.commit_file(&repo, "src/lib.rs", "pub fn pay() {}\n", "add lib");
    // The branch named after the Plan clashes with the Lead Branch pay-1/web: `pr add` must find it.
    fx.git(&repo, &["branch", "pay-1"]);

    let run = pr_add(&fx, &repo);

    let error = run.error();
    assert_eq!(error["code"], "branch_exists", "{run:?}");
    assert_eq!(error["details"]["branch"], "pay-1", "{error}");
    no_child();
}

#[test]
fn pr_add_through_fx_cpd_keys_a_keyed_repo_of_the_route() {
    let fx = fx();
    plan_pay(&fx);
    let repo = fx.keyed_repo("pay").canonicalize().unwrap();

    let out = pr_add(&fx, &repo).ok();

    assert_eq!(out["workflow"]["base"], "main", "{out}");
    no_child();
}

// ---- keyed_repo and local_origin

#[test]
fn a_keyed_repo_has_a_github_origin_named_after_the_repo() {
    let fx = fx();

    let repo = fx.keyed_repo("pay");

    assert_eq!(
        fx.git(&repo, &["config", "--get", "remote.origin.url"]),
        "https://github.com/acme/pay.git"
    );
}

#[test]
fn a_keyed_repo_has_the_remote_tracking_branch_main_at_the_commit() {
    let fx = fx();
    let repo = fx.keyed_repo("pay");

    let main = fx.git(&repo, &["rev-parse", "main"]);

    assert_eq!(
        fx.git(&repo, &["rev-parse", "refs/remotes/origin/main"]),
        main
    );
}

#[test]
fn nothing_is_reachable_at_the_github_url_of_a_keyed_repo() {
    let fx = fx();
    let repo = fx.keyed_repo("pay");

    let text = panics(|| {
        fx.git(&repo, &["push", "-q", "origin", "main"]);
    });

    assert!(text.contains("Could not resolve host"), "{text}");
}

#[test]
fn local_origin_lets_a_keyed_repo_push_to_the_bare_repo() {
    let fx = fx();
    let origin = bare(&fx, "origin");
    let repo = fx.keyed_repo("pay");

    fx.local_origin(&repo, &origin);
    fx.git(&repo, &["push", "-q", "origin", "main"]);

    assert_eq!(
        fx.git(&origin, &["rev-parse", "main"]),
        fx.git(&repo, &["rev-parse", "main"])
    );
}

#[test]
fn local_origin_lets_a_second_repo_of_the_universe_fetch_what_was_pushed() {
    let fx = fx();
    let origin = bare(&fx, "origin");
    let repo = fx.keyed_repo("pay");
    fx.local_origin(&repo, &origin);
    fx.commit_file(&repo, "src/lib.rs", "pub fn pay() {}\n", "add lib");
    fx.git(&repo, &["push", "-q", "origin", "main"]);
    let other = fx.git_repo("other");
    fx.git(
        &other,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );

    fx.git(&other, &["fetch", "-q", "origin"]);

    assert_eq!(
        fx.git(&other, &["rev-parse", "refs/remotes/origin/main"]),
        fx.git(&repo, &["rev-parse", "main"])
    );
}

#[test]
fn local_origin_leaves_the_configured_url_a_github_one() {
    let fx = fx();
    let origin = bare(&fx, "origin");
    let repo = fx.keyed_repo("pay");

    fx.local_origin(&repo, &origin);

    assert_eq!(
        fx.git(&repo, &["config", "--get", "remote.origin.url"]),
        "https://github.com/acme/pay.git"
    );
}

#[test]
fn drop_unreachable_origin_removes_the_github_origin_of_a_keyed_repo() {
    let fx = fx();
    let repo = fx.keyed_repo("pay");

    fx.drop_unreachable_origin(&repo);

    assert_eq!(fx.git(&repo, &["remote"]), "");
}

#[test]
fn drop_unreachable_origin_keeps_an_origin_that_local_origin_made_reachable() {
    let fx = fx();
    let origin = bare(&fx, "origin");
    let repo = fx.keyed_repo("pay");
    fx.local_origin(&repo, &origin);

    fx.drop_unreachable_origin(&repo);

    assert_eq!(fx.git(&repo, &["remote"]), "origin");
}

#[test]
fn outline_for_review_outlines_the_plan_so_that_plan_review_is_not_refused_for_the_outline() {
    let fx = fx();
    let repo = fx.keyed_repo("pay");
    plan_pay(&fx);
    pr_add(&fx, &repo).ok();

    fx.outline_for_review("pay-1");

    let shown = fx.cpd(&["plan", "show", "--plan", "pay-1"]).ok();
    assert_eq!(
        shown["plan"]["outline"]["done_when"]["web"],
        serde_json::json!(["Done"]),
        "{shown}"
    );
}

// ---- unsupported git

#[test]
fn an_unsupported_git_subcommand_fails_the_test_at_drop_with_the_unsupported_list() {
    let fx = fx();
    let repo = fx.git_repo("pay");
    let out = fx
        .fakes
        .ports
        .runner
        .run(&call("git", &["-C", repo.to_str().unwrap(), "frobnicate"]))
        .expect("the route answers");
    assert_eq!(
        out.status.code(),
        Some(1),
        "the route answers exit 1, which a caller could read as an answer"
    );

    let text = panics(|| drop(fx));

    assert!(text.contains("fake-git"), "{text}");
    assert!(
        text.contains("frobnicate"),
        "the message lists the refused call: {text}"
    );
}

#[test]
fn a_git_repo_that_asked_nothing_unsupported_drops_quietly() {
    let fx = fx();
    let repo = fx.git_repo("pay");
    fx.commit_file(&repo, "a.txt", "a\n", "a");

    assert!(
        catch_unwind(AssertUnwindSafe(|| drop(fx))).is_ok(),
        "drop refused calls the route models"
    );
}

// ---- Fx::tmux and tmux_state

#[test]
fn no_tmux_server_exists_before_the_first_tmux_call() {
    let fx = tmux_fx();

    assert!(fx.tmux_state().is_none());
}

#[test]
fn a_tmux_probe_reads_the_server_a_step_holds_and_records_no_call() {
    let fx = tmux_fx();
    fx.tmux(&["new-session", "-d", "-s", "work", "-n", "w1", "sleep 1"]);
    let calls = fx.calls("tmux").len();
    let probe = fx.tmux_probe();
    let held = probe.clone();

    assert_eq!(
        held.run(&["list-windows", "-t", "=work", "-F", "#{window_name}"]),
        "w1"
    );
    assert_eq!(probe.state().sessions().len(), 1);
    assert_eq!(
        fx.calls("tmux").len(),
        calls,
        "the probe is no call of the product's"
    );
}

#[test]
fn a_new_session_made_through_tmux_lands_in_the_state() {
    let fx = tmux_fx();

    fx.tmux(&["new-session", "-d", "-s", "work", "-n", "w1", "sleep 1"]);

    let state = fx.tmux_state().expect("the call started a server");
    assert_eq!(
        state
            .sessions()
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["work"]
    );
    assert_eq!(
        state
            .windows()
            .iter()
            .map(|w| w.name.as_str())
            .collect::<Vec<_>>(),
        ["w1"]
    );
    assert_eq!(
        state
            .panes()
            .iter()
            .map(|p| p.argv.clone())
            .collect::<Vec<_>>(),
        [["sleep 1"]]
    );
}

#[test]
fn tmux_returns_the_output_of_the_route() {
    let fx = tmux_fx();

    let out = fx.tmux(&[
        "new-session",
        "-d",
        "-s",
        "work",
        "-P",
        "-F",
        "#{pane_id}",
        "sleep 1",
    ]);

    assert_eq!(
        (out.status.code(), text(out.stdout), text(out.stderr)),
        (Some(0), "%0\n".to_owned(), String::new())
    );
}

#[test]
fn tmux_sends_the_private_socket_and_the_tmpdir_of_the_env() {
    let fx = tmux_fx();

    fx.tmux(&["new-session", "-d", "-s", "work", "sleep 1"]);

    let calls = fx.calls("tmux");
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(
        calls[0].args[..2],
        ["-L".to_owned(), fx.env.tmux_socket().to_owned()]
    );
    assert_eq!(
        calls[0].args[2..],
        ["new-session", "-d", "-s", "work", "sleep 1"]
    );
    let tmpdir = fx.env.tmux_tmpdir().display().to_string();
    assert!(
        calls[0]
            .envs
            .contains(&("TMUX_TMPDIR".to_owned(), Some(tmpdir))),
        "{:?}",
        calls[0].envs
    );
}

#[test]
fn tmux_makes_no_socket_no_state_file_and_no_process() {
    let fx = tmux_fx();

    fx.tmux(&["new-session", "-d", "-s", "work", "sleep 1"]);

    let tmpdir = fx.env.tmux_tmpdir();
    assert!(
        !cpd::fakes::tmux::state::db_path(&tmpdir, fx.env.tmux_socket()).exists(),
        "the state is in memory"
    );
    let sockets: Vec<_> = std::fs::read_dir(&tmpdir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("tmux-"))
        .collect();
    assert!(
        sockets.is_empty(),
        "a tmux socket directory exists: {sockets:?}"
    );
    no_child();
}

#[test]
fn a_plan_new_window_shows_its_window_and_pane_options_in_the_state() {
    let fx = tmux_fx();
    fx.fake_cpd_views();
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
    assert!(
        fx.tmux_state().is_none(),
        "no server before the Lead's window"
    );

    let out = fx
        .cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Pay"])
        .ok();

    let lead = out["lead"]["session"].as_str().unwrap().to_owned();
    let state = fx
        .tmux_state()
        .expect("plan new started the cockpit server");
    assert_eq!(
        state
            .windows()
            .iter()
            .map(|w| w.name.as_str())
            .collect::<Vec<_>>(),
        ["park-pay"]
    );
    let panes = state.panes();
    assert_eq!(panes.len(), 1, "{panes:?}");
    assert!(
        panes[0]
            .options
            .contains(&("@cpd_session".to_owned(), lead.clone())),
        "{:?}",
        panes[0].options
    );
    assert_eq!(
        panes[0].argv[panes[0].argv.len() - 3..],
        ["session", "run", lead.as_str()]
    );
    assert!(
        state
            .pane_env(&panes[0].id)
            .contains(&("CPD_SESSION".to_owned(), lead)),
        "the pane's environment is the one the product gave it"
    );
}

// ---- cockpit and panes

#[test]
fn the_cockpit_is_session_cpd_with_window_main() {
    let fx = tmux_fx();

    fx.cockpit();

    let state = fx.tmux_state().expect("the cockpit started a server");
    assert_eq!(
        state
            .sessions()
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["cpd"]
    );
    assert_eq!(
        state
            .windows()
            .iter()
            .map(|w| w.name.as_str())
            .collect::<Vec<_>>(),
        ["main"]
    );
}

#[test]
fn the_cockpit_has_three_panes_the_tree_the_plan_and_the_chat() {
    let fx = tmux_fx();

    fx.cockpit();

    let views: Vec<String> = fx.panes().into_iter().map(|p| p.view).collect();
    assert_eq!(views, ["tree", "plan", "chat"]);
}

#[test]
fn the_cockpit_panes_are_alive_in_the_main_window_with_no_session() {
    let fx = tmux_fx();

    fx.cockpit();

    let panes = fx.panes();
    assert_eq!(panes.len(), 3, "{panes:?}");
    assert!(
        panes
            .iter()
            .all(|p| p.window_name == "main" && p.session.is_empty() && p.history.is_empty()),
        "{panes:?}"
    );
    assert!(fx.tmux_state().unwrap().panes().iter().all(|p| !p.dead));
}

#[test]
fn the_cockpit_server_has_the_server_options_of_cpd() {
    let fx = tmux_fx();

    fx.cockpit();

    let options = fx.tmux_state().unwrap().options();
    for option in SERVER_OPTIONS
        .iter()
        .filter(|o| o.len() == 4 && o[0] == "set" && o[1] == "-g")
    {
        assert!(
            options.contains(&(option[2].clone(), option[3].clone())),
            "{} is not set: {options:?}",
            option[2]
        );
    }
}

#[test]
fn the_cockpit_is_built_with_no_process_and_no_call_outside_the_route() {
    let fx = tmux_fx();

    fx.cockpit();

    no_child();
    assert!(
        fx.fakes.runner.unscripted().is_empty(),
        "{:?}",
        fx.fakes.runner.unscripted()
    );
}

#[test]
fn panes_lists_a_pane_made_after_the_cockpit_with_its_window_session_and_view() {
    let fx = tmux_fx();
    fx.cockpit();
    let pane = text(
        fx.tmux(&[
            "new-window",
            "-d",
            "-t",
            "=cpd:",
            "-n",
            "park-pay",
            "-P",
            "-F",
            "#{pane_id}",
            "sleep 1",
        ])
        .stdout,
    );
    let pane = pane.trim();
    fx.tmux(&["set", "-p", "-t", pane, "@cpd_session", "lead1"]);

    let rows = fx.panes();

    let row = rows
        .iter()
        .find(|p| p.pane == pane)
        .unwrap_or_else(|| panic!("{pane} is not in {rows:?}"));
    assert_eq!(
        (
            row.window_name.as_str(),
            row.session.as_str(),
            row.view.as_str()
        ),
        ("park-pay", "lead1", "")
    );
    assert_eq!(
        rows.len(),
        4,
        "the cockpit's three and the new one: {rows:?}"
    );
}

#[test]
fn panes_is_empty_while_no_server_exists() {
    let fx = tmux_fx();

    assert!(fx.panes().is_empty());
}

// ---- fake_cpd_views

#[test]
fn fake_cpd_views_answers_the_chat_view_at_once() {
    let fx = fx();
    fx.fake_cpd_views();

    let out = fx
        .fakes
        .ports
        .runner
        .run(&call("/opt/cpd/bin/cpd", &["tui", "view", "chat"]))
        .expect("scripted");

    assert_eq!(out.status.code(), Some(0));
    assert_eq!(fx.calls("cpd").len(), 1);
}

#[test]
fn fake_cpd_views_answers_the_history_view_at_once() {
    let fx = fx();
    fx.fake_cpd_views();

    let out = fx
        .fakes
        .ports
        .runner
        .run(&call("cpd", &["tui", "view", "history", "s1"]))
        .expect("scripted");

    assert_eq!(out.status.code(), Some(0));
    assert!(
        fx.fakes.clock.naps().is_empty() && fx.fakes.clock.elapsed() == Duration::ZERO,
        "it answers at once, in no virtual time"
    );
}

#[test]
fn fake_cpd_views_answers_no_other_view() {
    let fx = fx();
    fx.fake_cpd_views();
    let _ = fx
        .fakes
        .ports
        .runner
        .run(&call("cpd", &["tui", "view", "widgets"]));

    let text = panics(|| drop(fx));

    assert!(text.contains("widgets"), "{text}");
}

// ---- refusals at drop

#[test]
fn an_unscripted_call_beside_the_host_fixtures_is_refused_at_drop() {
    let fx = tmux_fx();
    let repo = fx.git_repo("pay");
    fx.tmux(&["new-session", "-d", "-s", "work", "sleep 1"]);
    let _ = fx
        .fakes
        .ports
        .runner
        .run(&call("curl", &["--silent", "https://example.test/x"]));

    let text = panics(|| drop(fx));

    assert!(
        text.contains("curl") && text.contains("https://example.test/x"),
        "{text}"
    );
    assert!(
        !text.contains(repo.to_str().unwrap()),
        "the git and tmux calls the routes answered are not named: {text}"
    );
}

#[test]
fn a_scripted_reply_still_wins_over_the_git_route() {
    let fx = fx();
    let repo = fx.git_repo("pay");
    fx.fake("git", Reply::exit(128, "", "fatal: bad object"));

    let text = panics(|| {
        fx.git(&repo, &["log", "-1"]);
    });

    assert!(text.contains("bad object"), "{text}");
}
