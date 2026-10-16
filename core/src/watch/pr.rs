//! E5, PR (contract §3.7, §7.6): for each Plan in `dispatch` or `pr` with an
//! `open` PR, every `pr_poll_seconds`, `cpd pr poll --plan <id>` as a
//! subprocess (a process boundary, cpd 1 §11 step 5); it polls every open PR of
//! the Plan. `cpd` is resolved on `PATH`, as in every pane cpd starts
//! (`session::PROGRAM`).

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::engine::Engine;
use super::report::{About, Failure, Step};
use crate::error::{CpdError, ErrorCode};
use crate::git::{TAIL_LINES, tail};
use crate::ids::PlanId;
use crate::session::PROGRAM;

pub(super) fn run(engine: &mut Engine) {
    let plans = engine.store.read(|conn| {
        let mut stmt = conn.prepare("SELECT id FROM plans p WHERE phase IN ('dispatch', 'pr') AND EXISTS (SELECT 1 FROM prs r WHERE r.plan = p.id AND r.status = 'open')
              ORDER BY id")?;
        let ids = stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        ids.iter().map(|id| PlanId::parse(id)).collect::<Result<Vec<_>, CpdError>>()
    });
    let plans = match plans {
        Ok(plans) => {
            engine.clear(Step::E5, &About::Machine);
            plans
        }
        Err(e) => return engine.fail(Step::E5, About::Machine, &e),
    };
    let every = Duration::from_secs(u64::from(engine.config.watch.pr_poll_seconds));
    for plan in plans {
        if engine.polled.get(&plan).is_some_and(|at| at.elapsed() < every) {
            continue;
        }
        engine.polled.insert(plan.clone(), Instant::now());
        let about = About::Plan(plan.clone());
        match poll(engine, &plan) {
            Ok(out) => {
                for pr in out["prs"].as_array().into_iter().flatten() {
                    let changes = &pr["changes"];
                    let new = ["checks", "reviews", "comments"].iter().any(|k| changes[k].as_array().is_some_and(|a| !a.is_empty()));
                    if pr["merged"] == true || new {
                        engine.log.line(&format!("E5: polled PR {} of Plan {plan}: state {}, merged {}", pr["pr"], pr["state"], pr["merged"]));
                    }
                }
                engine.clear(Step::E5, &about);
            }
            Err(failure) => engine.fail(Step::E5, about, failure),
        }
    }
}

/// Runs `cpd pr poll --plan <plan>` and returns its success value, or the
/// failure it printed.
fn poll(engine: &Engine, plan: &PlanId) -> Result<Value, Failure> {
    let argv = [PROGRAM, "pr", "poll", "--plan", plan.as_str()];
    let out = Command::new(argv[0])
        .args(&argv[1..])
        .env("CPD_HOME", engine.home.root())
        // The engine's own selection is not the subprocess's.
        .env_remove("CPD_PLAN")
        .env_remove("CPD_SESSION")
        .env_remove("CPD_ATTEMPT")
        .current_dir(engine.home.root())
        .stdin(Stdio::null())
        .output()
        .map_err(|e| {
            Failure::from(&CpdError::new(ErrorCode::SpawnFailed, format!("Cannot run {}: {e}.", argv.join(" "))).with("argv", json!(argv)))
        })?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let printed: Option<Value> = serde_json::from_str(stdout.trim()).ok();
    match printed {
        Some(value) if out.status.success() && value["ok"] == true => Ok(value),
        Some(value) if value["ok"] == false && value["error"]["code"].is_string() => {
            let error = &value["error"];
            Err(Failure {
                code: error["code"].as_str().unwrap_or_default().to_owned(),
                message: error["message"].as_str().unwrap_or_default().to_owned(),
                details: error["details"].clone(),
            })
        }
        _ => Err(Failure::from(
            &CpdError::new(ErrorCode::SpawnFailed, format!("{} printed no result.", argv.join(" ")))
                .with("argv", json!(argv))
                .with("exit", out.status.code())
                .with("stdout", tail(&stdout, TAIL_LINES))
                .with("stderr", tail(&String::from_utf8_lossy(&out.stderr), TAIL_LINES)),
        )),
    }
}
