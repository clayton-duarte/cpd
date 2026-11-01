//! The end-to-end flow of a durable Lead with a mocked host (contract §6.20, research row P1.11, checkpoint
//! CP3): `plan new` with `lead_runner = "durable"` makes a durable Lead, the engine's `hosts` step starts a
//! host, a reply is tailed through `cpd session tail`, the host dies and the engine restarts it inside the
//! bound, E2 leaves it alone, and the archive cascade stops it. On the fixture of `testkit.rs` and the fake
//! ports (`ManualClock`, `FakeRunner`, `FakeSpawn`, `FakeHostConn`): no socket, no pi-durable, no model, no
//! process. Red until the implementation Job writes `core/src/session/tail.rs`; the stub answers
//! `not implemented`, so the tail step panics and nothing after it runs.

use std::sync::Arc;

use clap::Parser;

use cpd_core::clock::ManualClock;
use cpd_core::host::runner::FakeRunner;
use cpd_core::host::spawn::{FakeSpawn, Signal, Spawn};
use cpd_core::ids::SessionId;
use cpd_core::ports::Ports;
use cpd_core::session::tail::FakeHostConn;

use super::testkit::Fx;
use super::{archive_leads, hosts, liveness};

#[derive(Parser)]
struct PlanCli {
    #[command(subcommand)]
    command: cpd_core::plan::Command,
}

#[derive(Parser)]
struct SessionCli {
    #[command(subcommand)]
    command: cpd_core::session::Command,
}

fn plan(words: &[&str]) -> cpd_core::plan::Command {
    PlanCli::try_parse_from(std::iter::once("cpd").chain(words.iter().copied()))
        .unwrap()
        .command
}

fn session(words: &[&str]) -> cpd_core::session::Command {
    SessionCli::try_parse_from(std::iter::once("cpd").chain(words.iter().copied()))
        .unwrap()
        .command
}

/// The engine on the fixture home with the durable `lead_runner`, the fake ports and the host socket the test
/// plays. `$CPD_HOME` is this test's home, for the commands that open the store from the environment.
fn flow_fx() -> (Fx, Arc<FakeSpawn>, Arc<ManualClock>, Arc<FakeHostConn>) {
    let mut fx = Fx::new("lead_runner = \"durable\"");
    let clock = Arc::new(ManualClock::new());
    let spawn = Arc::new(FakeSpawn::new());
    let host_conn = Arc::new(FakeHostConn::new());
    fx.engine.ports = Ports {
        clock: clock.clone(),
        runner: Arc::new(FakeRunner::new((*clock).clone())),
        spawn: spawn.clone(),
        host_conn: host_conn.clone(),
        ..Ports::real()
    };
    // SAFETY: nextest runs one test per process; this is the only test in it.
    unsafe {
        std::env::set_var("CPD_HOME", fx.engine.home.root());
        std::env::remove_var("CPD_SESSION");
    }
    (fx, spawn, clock, host_conn)
}

fn line(s: &str) -> Option<String> {
    Some(s.to_owned())
}

#[test]
fn flow_plan_new_to_archive_with_a_mocked_host() {
    let (mut fx, spawn, clock, host_conn) = flow_fx();
    let ports = fx.engine.ports.clone();

    // 1. `plan new` with the durable key makes a durable Lead: stopped, runner durable, no pane, no launch.
    Ports::scope(&ports, || {
        cpd_core::plan::run(plan(&[
            "new",
            "flow",
            "--context",
            "ctx",
            "--title",
            "Flow",
        ]))
    })
    .unwrap();
    let lead: String = fx.query("SELECT lead_session FROM plans WHERE id = 'flow'");
    let (state, runner, pane): (String, String, Option<String>) = fx
        .engine
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT state, runner, tmux_pane FROM sessions WHERE id = ?1",
                [&lead],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!(
        (state.as_str(), runner.as_str(), pane),
        ("stopped", "durable", None),
        "a durable Lead starts with no pane"
    );
    assert!(
        spawn.launches().is_empty(),
        "plan new starts no host; the engine's hosts step does"
    );

    // 1a. `plan new` writes the Lead's first message (commands.md §1.4): the source the durable host now
    //     consumes at startup (hs-d812), since the launch passes no `@<lead-message.md>` argv.
    let message = fx
        .engine
        .home
        .lead_message(&cpd_core::ids::PlanId::parse("flow").unwrap());
    assert!(
        message.exists(),
        "plan new writes the Lead's first message: {message:?}"
    );
    let first_message =
        std::fs::read_to_string(&message).expect("the Lead's first message is readable");
    assert!(
        !first_message.trim().is_empty(),
        "the Lead's first message is non-empty: {first_message:?}"
    );

    // 2. The hosts step (§6.19) launches the mocked host, and the mock reports idle as the real host does.
    hosts::run(&mut fx.engine);
    let launches = spawn.launches();
    assert_eq!(launches.len(), 1, "one host: {launches:?}");
    let launch = &launches[0];
    assert!(
        launch
            .args
            .first()
            .is_some_and(|a| a.ends_with("durable-host/main.ts")),
        "{:?}",
        launch.args
    );
    let named = launch.args.iter().any(|a| a == &lead);
    assert!(named, "the launch names the Session: {:?}", launch.args);
    assert!(
        launch
            .env_set
            .iter()
            .any(|(k, v)| k == "CPD_SESSION" && v == &lead),
        "{:?}",
        launch.env_set
    );
    assert!(
        launch.env_set.iter().any(|(k, _)| k == "CPD_HOME"),
        "{:?}",
        launch.env_set
    );
    assert!(
        launch.env_set.iter().any(|(k, _)| k == "PATH"),
        "{:?}",
        launch.env_set
    );
    Ports::scope(&ports, || {
        cpd_core::session::run(session(&["state", "idle", "--session", &lead]))
    })
    .unwrap();

    // 3. Mid-life delivery parity (requirement 3): the fresh durable Lead's first turn is its message file read by
    //    the host at startup (asserted above); `session send` later writes a message Event the mocked reply is tailed through.
    Ports::scope(&ports, || {
        cpd_core::session::run(session(&[
            "send",
            &lead,
            "run the checks",
            "--unblocks",
            "flow",
        ]))
    })
    .unwrap();
    assert_eq!(
        fx.count("session-message"),
        1,
        "{:?}",
        fx.events("session-message")
    );
    host_conn.serve(vec![
        line(r#"{"type":"welcome","protocol":1,"host_version":"2.7.0","pi_durable":"1.3.0","session":"flow","role":"lead","epoch":"e1","client_id":"c1","cwd":"/","limits":{"client_line":1048576}}"#),
        line(r#"{"type":"attached","req":"r1","mode":"snapshot","epoch":"e1","from_seq":1,"head_seq":2,"client_id":"c1"}"#),
        line(r#"{"type":"snapshot","epoch":"e1","seq":1,"snapshot":{"type":"snapshot","entries":[{"id":1,"conversationId":1,"kind":"pi.user","model":[{"role":"user","content":"run the checks","timestamp":1}]}],"tools":[],"nestedTools":[],"compactions":[],"inbox":[],"agent":{},"usage":{"models":{},"tools":{}}}}"#),
        line(r#"{"type":"events","epoch":"e1","seq":2,"events":[{"type":"message_start","message":{"role":"assistant","content":[{"type":"text","text":"All green. Ready for review."}],"api":"faux","provider":"faux","model":"faux-1","usage":{},"stopReason":"stop","timestamp":2}},{"type":"message_end","entry":{"id":2,"conversationId":1,"kind":"pi.assistant","model":[{"role":"assistant","content":[{"type":"text","text":"All green. Ready for review."}],"api":"faux","provider":"faux","model":"faux-1","usage":{},"stopReason":"stop","timestamp":2}]}}]}"#),
        line(r#"{"type":"bye","reason":"detached","reconnect":false}"#),
    ]);
    let home = fx.engine.home.clone();
    let sid = SessionId::parse(&lead).unwrap();
    let mut out = Vec::new();
    Ports::scope(&ports, || {
        cpd_core::session::tail::tail(&home, &sid, &mut out)
    })
    .unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("assistant: All green. Ready for review."),
        "the reply is in the transcript: {text:?}"
    );

    // 4. The mock host is killed: the FakeSpawn pid dies (the `kill -9` of the row).
    let pid = spawn
        .with_env("CPD_SESSION", &lead)
        .into_iter()
        .next()
        .expect("a live host");
    spawn.exit(pid, 9);

    // 5. The engine restarts it within the bound (hs-d785): a second launch.
    hosts::run(&mut fx.engine);
    assert_eq!(
        spawn.launches().len(),
        2,
        "a restart inside the bound: {:?}",
        spawn.launches()
    );

    // 6. E2 leaves the durable Session alone: no state, no pane and no restart.
    clock.advance(std::time::Duration::from_secs(11 * 60));
    liveness::run(&mut fx.engine);
    let (state, pane): (String, Option<String>) = fx
        .engine
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT state, tmux_pane FROM sessions WHERE id = ?1",
                [&lead],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!(
        (state.as_str(), pane),
        ("idle", None),
        "E2 does not stop, restart or pane a durable Lead"
    );
    assert!(
        !fx.engine
            .home
            .run_dir()
            .join("session-restarts.json")
            .exists(),
        "E2 recorded no restart"
    );

    // 7. The archive cascade (E14) stops the host: the Plan's own Lead is archived by `plan archive`, not by
    //    `session archive`, and its host is stopped through the engine's hook.
    let old = "2020-01-01T00:00:00.000Z";
    // The trigger walks the phases one legal step at a time (migration 0008): grounding>plan>dispatch>deploy>done,
    // and `locked_at` is NOT NULL from `dispatch` on. The Lead approved this fixture edit (Job p1-11-impl-r).
    fx.fixture(&format!(
        "UPDATE plans SET phase = 'plan' WHERE id = 'flow';
         UPDATE plans SET locked_at = 't', phase = 'dispatch' WHERE id = 'flow';
         UPDATE plans SET phase = 'deploy' WHERE id = 'flow';
         UPDATE plans SET phase = 'done', done_at = '{old}' WHERE id = 'flow';
         UPDATE sessions SET asleep_at = '{old}' WHERE id = '{lead}';"
    ));
    archive_leads::run(&mut fx.engine);
    assert!(
        spawn.signals().iter().any(|(_, s)| *s == Signal::Term),
        "the archived host is stopped: {:?}",
        spawn.signals()
    );
    let archived: Option<String> = fx.query(&format!(
        "SELECT archived_at FROM sessions WHERE id = '{lead}'"
    ));
    assert!(archived.is_some(), "the Lead reads archived");
    let launches = spawn.launches().len();
    hosts::run(&mut fx.engine);
    assert_eq!(
        spawn.launches().len(),
        launches,
        "an archived Session starts no host"
    );
}

/// `flow_fx` with empty config: both `lead_runner` and `manager_runner` are unset and default to `pi`. The fake
/// ports are the same (`ManualClock`, `FakeRunner`, `FakeSpawn`, `FakeHostConn`): no socket, no pi-durable, no
/// process.
fn pi_config_fx() -> (Fx, Arc<FakeSpawn>) {
    let mut fx = Fx::new("");
    let clock = Arc::new(ManualClock::new());
    let spawn = Arc::new(FakeSpawn::new());
    fx.engine.ports = Ports {
        clock: clock.clone(),
        runner: Arc::new(FakeRunner::new((*clock).clone())),
        spawn: spawn.clone(),
        host_conn: Arc::new(FakeHostConn::new()),
        ..Ports::real()
    };
    // SAFETY: nextest runs one test per process; this is the only test in it.
    unsafe {
        std::env::set_var("CPD_HOME", fx.engine.home.root());
        std::env::remove_var("CPD_SESSION");
    }
    (fx, spawn)
}

/// The runner is authoritative to the engine (contract §6.19, A3): with both config keys defaulting to `pi`, a
/// `--runner durable` Lead is left `stopped` with no pane and no host by `plan new`, and the engine's `hosts` step
/// starts the host because `sessions.runner` says `durable`, not because the config does.
#[test]
fn a_plan_new_runner_flag_starts_the_host_under_pi_config() {
    let (mut fx, spawn) = pi_config_fx();
    let ports = fx.engine.ports.clone();

    Ports::scope(&ports, || {
        cpd_core::plan::run(plan(&[
            "new",
            "flag",
            "--context",
            "ctx",
            "--title",
            "Flag",
            "--runner",
            "durable",
        ]))
    })
    .unwrap();
    let lead: String = fx.query("SELECT lead_session FROM plans WHERE id = 'flag'");
    let (state, runner, pane): (String, String, Option<String>) = fx
        .engine
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT state, runner, tmux_pane FROM sessions WHERE id = ?1",
                [&lead],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!(
        (state.as_str(), runner.as_str(), pane),
        ("stopped", "durable", None),
        "a durable Lead starts with no pane under pi config"
    );
    assert!(
        spawn.launches().is_empty(),
        "plan new starts no host; the engine's hosts step does"
    );

    hosts::run(&mut fx.engine);
    let launches = spawn.launches();
    assert_eq!(launches.len(), 1, "one host: {launches:?}");
    let launch = &launches[0];
    assert!(
        launch
            .args
            .first()
            .is_some_and(|a| a.ends_with("durable-host/main.ts")),
        "{:?}",
        launch.args
    );
    assert!(
        launch.args.iter().any(|a| a == "--session"),
        "the host launch carries --session: {:?}",
        launch.args
    );
    assert!(
        launch.args.iter().any(|a| a == &lead),
        "the launch names the Session: {:?}",
        launch.args
    );
}
