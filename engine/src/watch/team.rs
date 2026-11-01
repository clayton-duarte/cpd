//! E11, the team review poll (contract §3.7, §2.2 "The team review rule"; commands.md §21.5, §21.6): reads the team
//! Slack channel through the helper `pi/lib/slack-channel.ts` and caches its posts in `run/team-review.json`, which E8
//! reads. It only decides and hands over, like E9, and it never holds the tick: the helper runs on its own thread,
//! under its own deadline, and a later tick collects its answer from a channel. A run starts `watch.team_review_seconds`
//! after the previous one **ended**, so two never overlap; the first starts with the engine.
//!
//! The cache is replaced only by a helper that answered `ok` with its `posts` (the helper itself refuses a payload
//! that parses into no message, `slack_unparseable`): every failure leaves the last good copy, and is reported once as
//! an `error` Event until a run succeeds: the engine tells the Manager of it (hs-d314), no Inbox item.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::engine::{Engine, incident_holds};
use super::pr::printed;
use super::report::{About, Failure, Step};
use cpd_core::clock::timestamp;
use cpd_core::config::PiInstall;
use cpd_core::config::catalog::node;
use cpd_core::config::locate_pi;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::host::deadline::Failed;
use cpd_core::host::runner::Runner;
use cpd_core::ports::Ports;

/// The helper's deadline (contract §3.7): above its 45 s channel read and its thread reads of 30 s, eight at a time.
const DEADLINE: Duration = Duration::from_secs(120);

/// `run/team-review.json`: the posts of the team channel as the helper last read them (contract §1.4).
pub fn cache_path(home: &CpdHome) -> PathBuf {
    home.run_dir().join("team-review.json")
}

/// What E11 remembers between ticks.
#[derive(Default)]
pub(super) struct State {
    /// The run in flight: its answer arrives here.
    running: Option<Receiver<Result<Value, Failure>>>,
    /// When the last run ended (or failed to start).
    last_end: Option<Instant>,
}

pub(super) fn run(engine: &mut Engine) {
    if incident_holds(engine, Step::E11) {
        engine.log.line("E11: holding: incident mode is on");
        return;
    }
    if engine.config.inbox.team_channel.is_empty() {
        // Switched off: nothing runs and nothing it reported stays.
        engine.clear(Step::E11, &About::Machine);
        return;
    }
    if let Some(running) = &engine.team.running {
        let answer = match running.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(Failure::from(&CpdError::new(
                ErrorCode::SpawnFailed,
                "The team review helper's reader ended without an answer.",
            ))),
        };
        engine.team.running = None;
        engine.team.last_end = Some(engine.ports.clock.now());
        match answer.and_then(|value| store(engine, &value)) {
            Ok(posts) => {
                engine
                    .log
                    .line(&format!("E11: cached {posts} post(s) of the team channel"));
                engine.clear(Step::E11, &About::Machine);
            }
            Err(failure) => engine.fail(Step::E11, About::Machine, failure),
        }
        return;
    }
    let period = Duration::from_secs(u64::from(engine.config.watch.team_review_seconds));
    if engine
        .team
        .last_end
        .is_some_and(|end| engine.ports.clock.now().saturating_duration_since(end) < period)
    {
        return;
    }
    match start(engine) {
        Ok(running) => engine.team.running = Some(running),
        Err(failure) => {
            // The next try is a period away, as after a run that failed.
            engine.team.last_end = Some(engine.ports.clock.now());
            engine.fail(Step::E11, About::Machine, failure);
        }
    }
}

/// Starts the helper on its own thread; returns where its answer will arrive.
fn start(engine: &Engine) -> Result<Receiver<Result<Value, Failure>>, Failure> {
    let pi = locate_pi().map_err(|e| Failure::from(&e))?;
    start_with(Ports::current().runner, engine, &pi, &node())
}

/// [`start`] on `runner`, with pi located as `pi` and pi's Node `node`.
pub(super) fn start_with(
    runner: Arc<dyn Runner>,
    engine: &Engine,
    pi: &PiInstall,
    node: &Path,
) -> Result<Receiver<Result<Value, Failure>>, Failure> {
    let script = engine
        .config
        .pi_adapter_dir()
        .map_err(|e| Failure::from(&e))?
        .join("lib")
        .join("slack-channel.ts");
    let args = [
        script.display().to_string(),
        "--coding-agent".to_owned(),
        pi.coding_agent_entry.display().to_string(),
        "--channel".to_owned(),
        engine.config.inbox.team_channel.clone(),
        "--me".to_owned(),
        engine.config.inbox.slack_user_id.clone(),
    ];
    let argv: Vec<String> = std::iter::once(node.display().to_string())
        .chain(args.iter().cloned())
        .collect();
    let mut cmd = Command::new(node);
    cmd.args(&args)
        .env("PI_CODING_AGENT_DIR", &pi.agent_dir)
        .env("CPD_HOME", engine.home.root())
        // The engine's own selection is not the helper's.
        .env_remove("CPD_PLAN")
        .env_remove("CPD_SESSION")
        .env_remove("CPD_ATTEMPT")
        .current_dir(engine.home.root())
        .stdin(Stdio::null());
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let answer = match runner.output(&mut cmd, "node", DEADLINE) {
            Ok(out) => printed(&out, &argv.iter().map(String::as_str).collect::<Vec<_>>()),
            Err(Failed::Io(e)) => Err(Failure::from(
                &CpdError::new(
                    ErrorCode::SpawnFailed,
                    format!("Cannot run {}: {e}.", argv.join(" ")),
                )
                .with("argv", json!(argv)),
            )),
            Err(Failed::Timeout(e)) => Err(Failure::from(&e)),
        };
        // The engine may have stopped: nobody to tell.
        let _ = tx.send(answer);
    });
    Ok(rx)
}

/// Writes the cache from the helper's answer, whole, by rename; returns how many posts it holds. An answer without
/// `posts` is refused and the last good copy stays.
fn store(engine: &Engine, answer: &Value) -> Result<usize, Failure> {
    let (Some(posts), Some(read)) = (answer["posts"].as_array(), answer["read"].as_u64()) else {
        return Err(Failure {
            code: "slack_helper_failed".to_owned(),
            message: "The team review helper answered ok with no `posts` and `read`.".to_owned(),
            details: json!({"stdout": answer.to_string()}),
        });
    };
    let cache = json!({
        "version": 1,
        "polled_at": timestamp(engine.ports.clock.utc()),
        "channel": engine.config.inbox.team_channel,
        "read": read,
        "posts": posts,
    });
    let path = cache_path(&engine.home);
    let tmp = path.with_extension("json.tmp");
    let io = |what: &str, e: std::io::Error| {
        Failure::from(
            &CpdError::new(
                ErrorCode::StoreFailed,
                format!("Cannot {what} {}: {e}.", path.display()),
            )
            .with("path", path.display().to_string()),
        )
    };
    fs::create_dir_all(engine.home.run_dir()).map_err(|e| io("create the directory of", e))?;
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(&cache).expect("a JSON value serializes"),
    )
    .map_err(|e| io("write", e))?;
    fs::rename(&tmp, &path).map_err(|e| io("replace", e))?;
    Ok(posts.len())
}
