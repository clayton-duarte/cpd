//! The in-process test kit of `common::fx::Fx` over fake ports (docs/implementing.md, "The in-process test kit"):
//! `Fx::with_ports` runs `cpd` through `run_with` on a `ManualClock`, a `FakeRunner` and a `FakeSpawn`, so a command that
//! runs `gh`, waits on time or launches a `cpd` child needs no process, no sleep and no real external.
//!
//! Every test here is in-process: none starts a process, sleeps or reads the wall clock.

use std::any::Any;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use cpd_core::host::runner::{Call, Reply};
use cpd_core::host::spawn::Launch;
use nix::errno::Errno;
use nix::sys::wait::{WaitPidFlag, waitpid};
use serde_json::json;

use crate::common::fx::Fx;

const CONFIG: &str = "version = 1\nmachine = \"m1\"\n";

/// An `Fx` on fake ports whose `$CPD_HOME` has the config every command but `config` needs.
fn fx() -> Fx {
    let fx = Fx::with_ports();
    fx.env.write_config(CONFIG);
    fx
}

/// The process's environment, by name.
fn vars() -> BTreeMap<OsString, OsString> {
    std::env::vars_os().collect()
}

/// The text of a panic payload.
fn message(payload: Box<dyn Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(text) => *text,
        Err(payload) => payload.downcast::<&str>().map(|text| (*text).to_owned()).unwrap_or_default(),
    }
}

/// The text of the panic `f` raises; fails when it does not panic.
fn panics(f: impl FnOnce()) -> String {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(()) => panic!("expected a panic, there was none"),
        Err(payload) => message(payload),
    }
}

/// The `txns` row the newest write left: (`cwd`, `session`), what the product read from the process when it wrote.
fn last_txn(fx: &Fx) -> (String, Option<String>) {
    fx.env.read(|conn| conn.query_row("SELECT cwd, session FROM txns ORDER BY id DESC LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?))))
}

/// The Lead worktree of PR `repo` in `open_pr`: a plain directory, where `gh` runs.
fn lead(fx: &Fx) -> std::path::PathBuf {
    fx.env.root().join("lead")
}

/// Plan `pay` with PR `repo` open as pull request #812 of `acme/pay`: `cpd pr view --plan pay` runs `gh pr view` in
/// [`lead`] and needs nothing else (no git).
fn open_pr(fx: &Fx) {
    std::fs::create_dir_all(lead(fx)).expect("create the Lead worktree");
    fx.env.owner_policy("acme", "");
    let sql = format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay the bills', 'deploy', 'lead1', 't', 't');
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
           VALUES ('acme/pay', 812, 'https://github.com/acme/pay/pull/812', 'Pay the bills', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                          pr_number, pr_url, pr_opened_at, pull_request)
           VALUES ('pay', 'repo', '/nowhere', 'main', 'open', 'cpd/pay', '{}', cpd_txn(), 't', 't',
                   812, 'https://github.com/acme/pay/pull/812', 'opened', (SELECT id FROM pull_requests));",
        lead(fx).display()
    );
    fx.env.sql(&sql);
}

/// A call of `program` with `args`, as a `Runner` receives it from the code under test.
fn call(program: &str, args: &[&str], stdin: Option<&[u8]>) -> Call {
    Call {
        tool: program.to_owned(),
        program: program.to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        envs: Vec::new(),
        cwd: None,
        deadline: Duration::from_secs(30),
        stdin: stdin.map(<[u8]>::to_vec),
        stderr_to: None,
    }
}

/// A launch of `cpd <args>`, as a command that starts a `cpd` child hands it to `Spawn`.
fn launch(args: &[&str]) -> Launch {
    Launch { program: "/opt/cpd/bin/cpd".to_owned(), args: args.iter().map(|a| (*a).to_owned()).collect(), ..Launch::default() }
}

// ---- Fx::cpd through run_with

#[test]
fn cpd_config_on_fake_ports_returns_the_config_json_with_no_process() {
    let fx = fx();

    // PATH is empty for the call: a command that tried to run any program could not find it.
    let run = fx.cpd_with(fx.env.root(), &["config"], &[("PATH", "")]);

    let json = run.ok();
    assert_eq!(json["config"]["machine"], "m1");
    assert_eq!(json["sources"]["machine"], "file");
    assert_eq!(json["path"], fx.env.cpd_home().join("config.toml").display().to_string());
    assert_eq!(waitpid(None, Some(WaitPidFlag::WNOHANG)), Err(Errno::ECHILD), "a child process lived while the command ran");
}

#[test]
fn cpd_on_fake_ports_returns_the_json_of_a_store_command() {
    let fx = fx();

    let run = fx.cpd_with(fx.env.root(), &["events", "list"], &[("PATH", "")]);

    assert_eq!((run.exit, run.json()), (0, json!({"ok": true, "events": [], "next": 0})));
}

#[test]
fn cpd_on_fake_ports_returns_a_refusal_as_a_failure_envelope() {
    let fx = fx();

    let run = fx.cpd(&["context", "pending", "--context", "nope"]);

    assert_eq!(run.exit, 4, "{run:?}");
    assert_eq!(run.error()["code"], "context_not_found");
}

#[test]
fn cpd_in_on_fake_ports_runs_in_the_directory_and_restores_it() {
    let fx = fx();
    let before = std::env::current_dir().unwrap();
    let elsewhere = fx.env.root().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();

    fx.cpd_in(&elsewhere, &["context", "new", "c1", "--title", "T", "--owner", "acme"]).ok();

    assert_eq!(std::path::PathBuf::from(last_txn(&fx).0).canonicalize().unwrap(), elsewhere.canonicalize().unwrap());
    assert_eq!(std::env::current_dir().unwrap(), before, "the directory is put back after the call");
}

#[test]
fn cpd_with_on_fake_ports_sets_the_variables_for_the_call_and_restores_them() {
    let fx = fx();

    fx.cpd_with(fx.env.root(), &["context", "new", "c1", "--title", "T", "--owner", "acme"], &[("CPD_SESSION", "s1")]).ok();

    assert_eq!(last_txn(&fx).1.as_deref(), Some("s1"), "the call saw the variable");
    assert_eq!(std::env::var_os("CPD_SESSION"), None, "the variable is gone after the call");
}

// ---- Fx::fake and calls

#[test]
fn a_command_that_runs_gh_reaches_the_scripted_reply() {
    let fx = fx();
    open_pr(&fx);
    fx.fake_gh(Reply::exit(1, "", "HTTP 401: bad credentials"));

    let run = fx.cpd(&["pr", "view", "--plan", "pay"]);

    assert_eq!((run.error()["code"].as_str().unwrap(), run.exit), ("gh_failed", 1));
    let details = &run.error()["details"];
    assert!(details["stderr"].as_str().unwrap().contains("bad credentials"), "{details}");
}

#[test]
fn calls_shows_the_argv_environment_and_directory_of_a_gh_call() {
    let fx = fx();
    open_pr(&fx);
    fx.fake_gh(Reply::exit(1, "", "HTTP 401: bad credentials"));

    fx.cpd(&["pr", "view", "--plan", "pay"]);

    let calls = fx.calls("gh");
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].tool, "gh");
    assert_eq!(
        calls[0].args,
        ["pr", "view", "812", "--json", "state,mergedAt,statusCheckRollup,reviews,comments,title,reviewDecision,url"]
    );
    assert!(calls[0].envs.contains(&("GH_PROMPT_DISABLED".to_owned(), Some("1".to_owned()))), "{:?}", calls[0].envs);
    assert!(calls[0].envs.contains(&("NO_COLOR".to_owned(), Some("1".to_owned()))), "{:?}", calls[0].envs);
    assert_eq!(calls[0].cwd.as_deref(), Some(lead(&fx).as_path()));
}

#[test]
fn calls_of_a_program_no_call_reached_is_empty() {
    let fx = fx();
    fx.fake_gh(Reply::ok("{}"));

    assert!(fx.calls("gh").is_empty());
}

#[test]
fn fake_answers_a_program_by_its_file_name() {
    let fx = fx();
    fx.fake("curl", Reply::ok("body"));

    let out = fx.fakes.ports.runner.run(&call("/usr/bin/curl", &["-s", "https://example.test/x"], None)).expect("the scripted reply");

    assert_eq!((out.status.code(), out.stdout), (Some(0), b"body".to_vec()));
    assert_eq!(fx.calls("curl").len(), 1);
}

#[test]
fn fake_pi_answers_a_call_fed_on_stdin_and_exposes_the_stdin_it_got() {
    let fx = fx();
    fx.fake_pi(Reply::ok("{\"type\":\"agent_end\"}\n"));

    let out = fx.fakes.ports.runner.run(&call("pi", &["--mode", "json", "--no-session"], Some(b"the prompt"))).expect("the scripted reply");

    assert_eq!(out.stdout, b"{\"type\":\"agent_end\"}\n");
    let calls = fx.calls("pi");
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].args, ["--mode", "json", "--no-session"]);
    assert_eq!(calls[0].stdin.as_deref(), Some(b"the prompt".as_slice()));
}

#[test]
fn fake_op_answers_a_call_of_op() {
    let fx = fx();
    fx.fake_op(Reply::ok("s3cret\n"));

    let out = fx.fakes.ports.runner.run(&call("op", &["read", "op://vault/item/field"], None)).expect("the scripted reply");

    assert_eq!(out.stdout, b"s3cret\n");
    assert_eq!(fx.calls("op").len(), 1);
}

#[test]
fn fake_node_answers_a_call_of_node() {
    let fx = fx();
    fx.fake_node(Reply::exit(3, "", "boom"));

    let out = fx.fakes.ports.runner.run(&call("node", &["script.js"], None)).expect("the scripted reply");

    assert_eq!((out.status.code(), out.stderr), (Some(3), b"boom".to_vec()));
    assert_eq!(fx.calls("node").len(), 1);
}

#[test]
fn cpd_silent_passes_for_a_command_that_writes_nothing_and_refuses_one_that_does() {
    let fx = fx();

    // `tui action` prints nothing and exits 0 whatever happens (commands.md §13.2): here no tmux server answers.
    fx.allow_unscripted();
    fx.cpd_silent(&["tui", "action", "logs", "--client", "c1"]);

    let text = panics(|| fx.cpd_silent(&["events", "list"]));
    assert!(text.contains("events") && text.contains("expected exit 0 and nothing"), "{text}");
}

// ---- unscripted calls

#[test]
fn an_unscripted_external_call_is_refused_at_drop_naming_the_program_and_argv() {
    let fx = fx();
    open_pr(&fx);

    // No `fake_gh`: `pr view` calls `gh`, which nothing answers.
    fx.cpd(&["pr", "view", "--plan", "pay"]);
    let text = panics(|| drop(fx));

    assert!(text.contains("gh"), "{text}");
    assert!(text.contains("pr") && text.contains("view") && text.contains("812"), "the message names the argv: {text}");
}

#[test]
fn an_unscripted_call_of_another_program_is_refused_though_gh_is_scripted() {
    let fx = fx();
    fx.fake_gh(Reply::ok("{}"));
    let _ = fx.fakes.ports.runner.run(&call("curl", &["--silent", "https://example.test/x"], None));

    let text = panics(|| drop(fx));

    assert!(text.contains("curl"), "{text}");
    assert!(text.contains("--silent") && text.contains("https://example.test/x"), "the message names the argv: {text}");
}

#[test]
fn allow_unscripted_lifts_the_refusal_at_drop() {
    let fx = fx();
    fx.allow_unscripted();
    let _ = fx.fakes.ports.runner.run(&call("curl", &["--silent", "https://example.test/x"], None));

    assert!(catch_unwind(AssertUnwindSafe(|| drop(fx))).is_ok(), "drop refused a call that was allowed");
}

#[test]
fn scripted_calls_alone_are_not_refused_at_drop() {
    let fx = fx();
    fx.fake_op(Reply::ok("x"));
    let _ = fx.fakes.ports.runner.run(&call("op", &["whoami"], None));

    assert!(catch_unwind(AssertUnwindSafe(|| drop(fx))).is_ok(), "drop refused a scripted call");
}

#[test]
fn an_fx_that_called_nothing_drops_quietly() {
    let fx = fx();

    assert!(catch_unwind(AssertUnwindSafe(|| drop(fx))).is_ok());
}

// ---- virtual time

#[test]
fn advance_moves_the_manual_clock() {
    let fx = fx();

    fx.advance(Duration::from_secs(5));

    assert_eq!(fx.fakes.clock.elapsed(), Duration::from_secs(5));
}

#[test]
fn advance_adds_up_and_is_no_nap() {
    let fx = fx();

    fx.advance(Duration::from_secs(5));
    fx.advance(Duration::from_millis(500));

    assert_eq!(fx.fakes.clock.elapsed(), Duration::from_millis(5500));
    assert!(fx.fakes.clock.naps().is_empty(), "an advance is time passing, not a caller sleeping");
}

#[test]
fn until_returns_at_once_without_advancing_when_the_condition_holds() {
    let fx = fx();
    let mut looks = 0;

    fx.until(|| {
        looks += 1;
        true
    });

    assert_eq!(looks, 1);
    assert_eq!(fx.fakes.clock.elapsed(), Duration::ZERO);
}

#[test]
fn until_returns_as_soon_as_the_condition_holds_and_advances_in_equal_steps() {
    let fx = fx();
    let mut seen = Vec::new();

    fx.until(|| {
        seen.push(fx.fakes.clock.elapsed());
        seen.len() == 4
    });

    assert_eq!(seen.len(), 4, "it looked again after each step, and stopped at the first true: {seen:?}");
    assert_eq!(seen[0], Duration::ZERO, "the first look is before any step");
    let step = seen[1] - seen[0];
    assert!(step > Duration::ZERO, "{seen:?}");
    assert_eq!([seen[2] - seen[1], seen[3] - seen[2]], [step, step], "a step is the clock's tick every time: {seen:?}");
    assert_eq!(fx.fakes.clock.elapsed(), seen[3], "it advanced no further once the condition held");
}

#[test]
fn until_makes_a_condition_that_waits_on_virtual_time_hold() {
    let fx = fx();
    let deadline = Duration::from_millis(100);

    fx.until(|| fx.fakes.clock.elapsed() >= deadline);

    assert!(fx.fakes.clock.elapsed() >= deadline);
}

#[test]
fn until_fails_with_the_virtual_time_it_waited_when_the_condition_never_holds() {
    let fx = fx();
    let mut looks = 0;

    let text = panics(|| {
        fx.until(|| {
            looks += 1;
            false
        })
    });

    let waited = text.split_once("waited ").and_then(|(_, rest)| rest.split_once(" virtual time")).map(|(limit, _)| limit.to_owned());
    assert!(waited.is_some_and(|limit| !limit.trim().is_empty()), "the text is `waited <limit> virtual time`: {text}");
    assert!(looks > 1, "it looked again after each step: {looks}");
    assert!(fx.fakes.clock.elapsed() > Duration::ZERO, "the limit is virtual time that passed");
}

// ---- launched children

#[test]
fn take_launch_returns_the_launch_the_fake_spawn_recorded() {
    let fx = fx();
    let first = launch(&["attempt", "run", "a1", "--plan", "pay"]);
    let second = launch(&["pr", "poll"]);
    fx.fakes.ports.spawn.spawn(&first).expect("the fake records it");
    fx.fakes.ports.spawn.spawn(&second).expect("the fake records it");

    assert_eq!(fx.take_launch(0), first);
    assert_eq!(fx.take_launch(1), second);
}

#[test]
fn run_launch_runs_the_recorded_cpd_through_run_with_and_returns_its_run() {
    let fx = fx();
    fx.fakes.ports.spawn.spawn(&launch(&["events", "list"])).expect("the fake records it");
    let recorded = fx.take_launch(0);

    let run = fx.run_launch(&recorded);

    assert_eq!((run.exit, run.json()), (0, json!({"ok": true, "events": [], "next": 0})));
    assert_eq!(waitpid(None, Some(WaitPidFlag::WNOHANG)), Err(Errno::ECHILD), "a child process lived while the launch ran");
}

#[test]
fn run_launch_sets_the_launch_variables_for_the_run_and_restores_them() {
    let fx = fx();
    let launch = Launch {
        env_set: vec![("CPD_SESSION".to_owned(), "s1".to_owned())],
        ..launch(&["context", "new", "c1", "--title", "T", "--owner", "acme"])
    };

    fx.run_launch(&launch).ok();

    assert_eq!(last_txn(&fx).1.as_deref(), Some("s1"), "the launched cpd saw the variable");
    assert_eq!(std::env::var_os("CPD_SESSION"), None, "the variable is gone after the run");
}

#[test]
fn run_launch_removes_the_variables_the_launch_removes_for_the_run() {
    let fx = fx();
    let launch = Launch {
        env_remove: vec!["CPD_SESSION".to_owned()],
        ..launch(&["context", "new", "c1", "--title", "T", "--owner", "acme"])
    };

    // The test process has an identity of its own while the launch runs: the launch removes it for the child.
    // `Fx::export` sets it under the `Fx`'s lock; `Fx`'s drop removes a variable it did not find.
    fx.export("CPD_SESSION", "outer");
    fx.run_launch(&launch).ok();

    assert_eq!(last_txn(&fx).1, None, "the launched cpd saw no CPD_SESSION");
    assert_eq!(std::env::var_os("CPD_SESSION"), Some("outer".into()), "the variable is put back after the run");
}

#[test]
fn run_launch_runs_in_the_launch_directory_and_restores_the_directory() {
    let fx = fx();
    let before = std::env::current_dir().unwrap();
    let there = fx.env.root().join("there");
    std::fs::create_dir_all(&there).unwrap();
    let launch = Launch { cwd: Some(there.clone()), ..launch(&["context", "new", "c1", "--title", "T", "--owner", "acme"]) };

    fx.run_launch(&launch).ok();

    assert_eq!(std::path::PathBuf::from(last_txn(&fx).0).canonicalize().unwrap(), there.canonicalize().unwrap());
    assert_eq!(std::env::current_dir().unwrap(), before);
}

// ---- the lock and the environment

#[test]
fn two_fx_in_one_process_do_not_overlap() {
    let fx = fx();

    assert!(Fx::try_new().is_none(), "another Fx lived while this one did");
    drop(fx);
    assert!(Fx::try_new().is_some(), "the lock was not released at drop");
}

#[test]
fn the_environment_is_restored_when_an_fx_on_fake_ports_drops() {
    let before = vars();
    let cwd = std::env::current_dir().unwrap();
    let fx = fx();
    assert_eq!(std::env::var_os("CPD_HOME"), Some(fx.env.cpd_home().into()), "the environment is set once, at the start");
    fx.cpd(&["config"]).ok();

    drop(fx);

    assert_eq!(vars(), before);
    assert_eq!(std::env::current_dir().unwrap(), cwd);
}

#[test]
fn fx_new_still_runs_cpd_for_a_file_that_is_not_migrated() {
    drop(fx());

    let fx = Fx::new();
    fx.env.write_config(CONFIG);

    assert_eq!(fx.cpd(&["config"]).ok()["config"]["machine"], "m1");
}
