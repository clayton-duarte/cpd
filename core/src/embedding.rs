//! The embedding port: text to a vector.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::config::catalog::node;
use crate::error::{CpdError, ErrorCode};
use crate::host::deadline::Failed;
use crate::host::runner::{Call, Runner};

/// A vector with the model that made it.
#[derive(Clone, Debug, PartialEq)]
pub struct Embedded {
    pub vector: Vec<f32>,
    pub model: String,
    pub tokens: Option<u64>,
}

/// Embeds text at a dimension count.
pub trait EmbeddingPort: Send + Sync {
    fn embed(&self, text: &str, dims: usize) -> Result<Embedded, CpdError>;
}

/// The deadline of one helper run.
const DEADLINE: Duration = Duration::from_secs(30);

/// The three error codes of one helper (contract A6).
#[derive(Clone, Copy)]
pub(crate) struct Codes {
    pub unreachable: ErrorCode,
    pub refused: ErrorCode,
    pub bad_reply: ErrorCode,
}

/// Runs `node <script> <args>` with `stdin`, and parses its one JSON line once, at the edge: the `ok:true` object,
/// or the helper's own `ok:false` error mapped to `codes`. Every spawn, timeout and exit failure with no
/// answer is `unreachable`; a successful run with no answer is `bad_reply`. Stderr is never shown (it may hold a key).
pub(crate) fn run_helper(
    runner: &dyn Runner,
    script: &Path,
    agent_dir: &Path,
    extra_envs: Vec<(String, Option<String>)>,
    stdin: Vec<u8>,
    codes: Codes,
) -> Result<Value, CpdError> {
    let mut envs = vec![(
        "PI_CODING_AGENT_DIR".to_owned(),
        Some(agent_dir.display().to_string()),
    )];
    envs.extend(extra_envs);
    let call = Call {
        tool: "node".to_owned(),
        program: node().display().to_string(),
        args: vec![script.display().to_string()],
        envs,
        cwd: None,
        deadline: DEADLINE,
        stdin: Some(stdin),
        stderr_to: None,
    };
    let unreachable = |why: &str| CpdError::new(codes.unreachable, why.to_owned());
    let out = match runner.run(&call) {
        Ok(out) => out,
        Err(Failed::Timeout(_)) => return Err(unreachable("The helper timed out.")),
        Err(Failed::Io(_)) => return Err(unreachable("The helper cannot be run.")),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let Ok(answer) = serde_json::from_str::<Value>(stdout.trim()) else {
        return Err(if out.status.success() {
            CpdError::new(codes.bad_reply, "The helper printed no JSON answer.")
        } else {
            unreachable("The helper failed with no answer.")
        });
    };
    match answer.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(answer),
        Some(false) => {
            let error = answer.get("error");
            let field = |k: &str| error.and_then(|e| e.get(k)).and_then(Value::as_str);
            let code = field("code");
            let message = field("message").unwrap_or("The helper failed.");
            let code = if code == Some(codes.unreachable.as_str()) {
                codes.unreachable
            } else if code == Some(codes.refused.as_str()) {
                codes.refused
            } else {
                codes.bad_reply
            };
            let mut err = CpdError::new(code, message.to_owned());
            if code == codes.refused
                && let Some(status) = error
                    .and_then(|e| e.get("details"))
                    .and_then(|d| d.get("status"))
            {
                err = err.with("status", status.clone());
            }
            Err(err)
        }
        None => Err(CpdError::new(
            codes.bad_reply,
            "The helper's answer has no `ok`.",
        )),
    }
}

/// The real embedding adapter: `node <script>` over `runner` with the text on stdin, `PI_CODING_AGENT_DIR=<agent_dir>`
/// and `CPD_EMBED_DIMS=<dims>` in its environment (the test pins argv to the script alone), and no key anywhere.
/// `script` is `pi/lib/embed.ts`; its answer is one JSON line (`{ok:true,vector,model,tokens?}` or `{ok:false,error}`).
pub struct BifrostEmbedding {
    runner: Arc<dyn Runner>,
    script: PathBuf,
    agent_dir: PathBuf,
}

impl BifrostEmbedding {
    pub fn new(runner: Arc<dyn Runner>, script: PathBuf, agent_dir: PathBuf) -> BifrostEmbedding {
        BifrostEmbedding {
            runner,
            script,
            agent_dir,
        }
    }
}

impl EmbeddingPort for BifrostEmbedding {
    fn embed(&self, text: &str, dims: usize) -> Result<Embedded, CpdError> {
        let answer = run_helper(
            &*self.runner,
            &self.script,
            &self.agent_dir,
            vec![("CPD_EMBED_DIMS".to_owned(), Some(dims.to_string()))],
            text.as_bytes().to_vec(),
            Codes {
                unreachable: ErrorCode::EmbeddingUnreachable,
                refused: ErrorCode::EmbeddingRefused,
                bad_reply: ErrorCode::EmbeddingBadReply,
            },
        )?;
        let bad = |why: &str| CpdError::new(ErrorCode::EmbeddingBadReply, why.to_owned());
        let numbers = answer
            .get("vector")
            .and_then(Value::as_array)
            .ok_or_else(|| bad("The answer has no vector."))?;
        let vector = numbers
            .iter()
            .map(|n| n.as_f64().map(|f| f as f32).filter(|f| f.is_finite()))
            .collect::<Option<Vec<f32>>>()
            .ok_or_else(|| bad("The vector holds a value that is not a finite number."))?;
        if vector.len() != dims {
            return Err(bad("The vector has the wrong dimension.")
                .with("expected", dims)
                .with("got", vector.len()));
        }
        let model = answer
            .get("model")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("The answer has no model."))?
            .to_owned();
        Ok(Embedded {
            vector,
            model,
            tokens: answer.get("tokens").and_then(Value::as_u64),
        })
    }
}
