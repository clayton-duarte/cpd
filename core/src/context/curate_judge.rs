//! Write-time curation: the Bifrost judge The real one runs `node <pi adapter>/lib/judge.ts`
//! through the [`Runner`]; the helper resolves the key, which is never in argv, env, output or error.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use super::curate::{JudgeUsage, Judged, Verdict};
use crate::config::catalog::node;
use crate::config::{Config, locate_pi};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::deadline::Failed;
use crate::host::runner::{Call, Runner};
use crate::ids::{RecordId, RecordKind};

/// A record as the judge sees it.
#[derive(Clone, Debug)]
pub struct RecordView<'a> {
    pub id: &'a RecordId,
    pub kind: RecordKind,
    pub at: &'a str,
    pub topic: Option<&'a str>,
    pub text: &'a str,
}

/// One question to the judge; `neighbour: None` is a label request (topic and short only).
#[derive(Clone, Debug)]
pub struct JudgeRequest<'a> {
    pub new: RecordView<'a>,
    pub neighbour: Option<RecordView<'a>>,
}

pub trait JudgePort: Send + Sync {
    fn judge(&self, request: &JudgeRequest) -> Result<Judged, CpdError>;
}

/// The helper's deadline: above its own 90 s fetch of the gateway.
const DEADLINE: Duration = Duration::from_secs(120);

/// The model the judge asks (decision pro-d366).
const MODEL: &str = "vertex/gemini-3.8-flash";

pub struct BifrostJudge {
    home: CpdHome,
    config: Config,
    runner: Arc<dyn Runner>,
}

impl BifrostJudge {
    pub fn new(home: &CpdHome, config: &Config, runner: Arc<dyn Runner>) -> Self {
        BifrostJudge {
            home: home.clone(),
            config: config.clone(),
            runner,
        }
    }
}

fn view(record: &RecordView<'_>) -> Value {
    let mut value = json!({"id": record.id.to_string(), "kind": record.kind.as_str(),
                           "at": record.at, "text": record.text});
    if let Some(topic) = record.topic {
        value["topic"] = json!(topic);
    }
    value
}

fn unreachable_error(why: impl std::fmt::Display) -> CpdError {
    CpdError::new(
        ErrorCode::JudgeUnreachable,
        format!("The judge is unreachable: {why}."),
    )
}

fn bad_reply(why: impl std::fmt::Display) -> CpdError {
    CpdError::new(
        ErrorCode::JudgeBadReply,
        format!("The judge's answer is not the expected one: {why}."),
    )
}

/// What the helper prints on success: every field required.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    ok: bool,
    verdict: Verdict,
    quote: String,
    reason: String,
    topic: String,
    short: String,
    #[serde(default)]
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireUsage {
    tokens_in: u64,
    tokens_out: u64,
    #[serde(default)]
    cost: Option<f64>,
}

/// The helper's one line, parsed once into the typed answer or the error it carries.
fn parse(line: &str) -> Result<Judged, CpdError> {
    let value: Value = serde_json::from_str(line.trim()).map_err(bad_reply)?;
    match value["ok"] {
        Value::Bool(true) => {
            let w: Wire = serde_json::from_value(value).map_err(bad_reply)?;
            if !w.ok {
                return Err(bad_reply("a success answer with `ok` false"));
            }
            Ok(Judged {
                verdict: w.verdict,
                quote: w.quote,
                reason: w.reason,
                topic: w.topic,
                short: w.short,
                usage: w.usage.map(|u| JudgeUsage {
                    tokens_in: u.tokens_in,
                    tokens_out: u.tokens_out,
                    cost: u.cost,
                }),
            })
        }
        Value::Bool(false) => {
            let error = &value["error"];
            let code = match error["code"].as_str() {
                Some("judge_unreachable") => ErrorCode::JudgeUnreachable,
                Some("judge_refused") => ErrorCode::JudgeRefused,
                Some("judge_bad_reply") => ErrorCode::JudgeBadReply,
                _ => return Err(bad_reply("an error envelope with an unknown code")),
            };
            let message = error["message"].as_str().unwrap_or("The judge failed.");
            let mut failure = CpdError::new(code, message);
            if let Some(details) = error["details"].as_object() {
                for (key, value) in details {
                    failure = failure.with(key, value.clone());
                }
            }
            Err(failure)
        }
        _ => Err(bad_reply("`ok` is not a boolean")),
    }
}

impl JudgePort for BifrostJudge {
    fn judge(&self, request: &JudgeRequest) -> Result<Judged, CpdError> {
        let script = self.config.pi_adapter_dir()?.join("lib").join("judge.ts");
        let body = json!({"model": MODEL, "new": view(&request.new),
                          "neighbour": request.neighbour.as_ref().map(view)});
        let mut envs = vec![
            (
                "CPD_HOME".to_owned(),
                Some(self.home.root().display().to_string()),
            ),
            ("CPD_PLAN".to_owned(), None),
            ("CPD_SESSION".to_owned(), None),
            ("CPD_ATTEMPT".to_owned(), None),
        ];
        // pi's agent directory, when pi is on PATH; else the caller's own `PI_CODING_AGENT_DIR` stands, and the
        // helper says so if it is unset.
        if let Ok(pi) = locate_pi() {
            envs.push((
                "PI_CODING_AGENT_DIR".to_owned(),
                Some(pi.agent_dir.display().to_string()),
            ));
        }
        let call = Call {
            tool: "node".to_owned(),
            program: node().display().to_string(),
            args: vec![script.display().to_string()],
            envs,
            cwd: Some(self.home.root().to_path_buf()),
            deadline: DEADLINE,
            stdin: Some(body.to_string().into_bytes()),
            stderr_to: None,
        };
        let out = match self.runner.run(&call) {
            Ok(out) => out,
            Err(Failed::Io(e)) => return Err(unreachable_error(format!("cannot run node: {e}"))),
            Err(Failed::Timeout(_)) => {
                return Err(unreachable_error("the helper ran past its deadline"));
            }
        };
        // The helper's stderr is never shown: it could echo what the gateway sent back.
        if !out.status.success() {
            return Err(unreachable_error(format!(
                "the helper exited with {:?}",
                out.status.code()
            )));
        }
        parse(&String::from_utf8_lossy(&out.stdout))
    }
}
