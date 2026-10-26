//! The helper run and its answer (contract §3.7 E15 "The helper's answer"): `node <pi_adapter>/lib/models-list.ts`,
//! no argument, under pi's Node, in its own process group, with a deadline; the answer is validated here, once.

use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Map, Value, json};

use super::{code_of, safe_name};
use crate::config::catalog::node;
use crate::config::{Config, locate_pi};
use crate::error::{CpdError, ErrorCode};
use crate::git::{TAIL_LINES, tail};
use crate::home::CpdHome;
use crate::host::deadline::Failed;
use crate::host::runner::Runner;
use crate::ports::Ports;

/// The helper's deadline (contract §3.7 E15): above its 60 s fetch of one provider.
const DEADLINE: Duration = Duration::from_secs(120);

/// A provider the helper listed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub provider: String,
    /// `oauth`, `api_key` or `custom`.
    pub auth: String,
    pub base: Option<String>,
    pub ids: Vec<String>,
    pub known: Option<Vec<String>>,
}

/// A provider the helper could not list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderFailure {
    pub provider: String,
    pub code: String,
    pub message: String,
    pub details: Map<String, Value>,
}

impl ProviderFailure {
    /// `{provider, error: {code, message, details}}`, as the helper printed it.
    pub fn entry(&self) -> Value {
        json!({"provider": self.provider, "error": {"code": self.code, "message": self.message, "details": self.details}})
    }

    /// The failure as an error: the helper's `models_*` code, else `models_helper_failed`; `details.provider` names it.
    pub fn error(&self) -> CpdError {
        let mut details = self.details.clone();
        details.insert("provider".to_owned(), json!(self.provider));
        error_of(&self.code, &self.message, details)
    }
}

/// An error with the helper's `models_*` code, else `models_helper_failed` (the unknown code in `details.code`).
fn error_of(code: &str, message: &str, mut details: Map<String, Value>) -> CpdError {
    let known = code_of(code);
    if known.is_none() {
        details.insert("code".to_owned(), json!(code));
    }
    let mut error = CpdError::new(known.unwrap_or(ErrorCode::ModelsHelperFailed), message);
    for (key, value) in details {
        error = error.with(&key, value);
    }
    error
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Listed(Listed),
    Failed(ProviderFailure),
}

/// The helper's answer of a run that did not fail as a whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub pi_version: String,
    /// One entry per listed or failed provider, in the helper's order (the answer's `providers` array).
    pub entries: Vec<Entry>,
}

fn invalid(why: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::ModelsHelperFailed, format!("The model-discovery helper's answer is not the contract's: {why}."))
}

fn texts(value: &Value, what: &str) -> Result<Vec<String>, CpdError> {
    let list = value.as_array().ok_or_else(|| invalid(format!("{what} is not an array")))?;
    list.iter()
        .map(|v| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned).ok_or_else(|| invalid(format!("{what} holds a value that is not a non-empty text"))))
        .collect()
}

fn failure_of(error: &Value, provider: &str) -> Result<ProviderFailure, CpdError> {
    let text = |key: &str| error[key].as_str().map(str::to_owned).ok_or_else(|| invalid(format!("the error of {provider:?} has no text `{key}`")));
    let details = match &error["details"] {
        Value::Object(map) => map.clone(),
        Value::Null => Map::new(),
        _ => return Err(invalid(format!("the error of {provider:?} has `details` that is not an object"))),
    };
    Ok(ProviderFailure { provider: provider.to_owned(), code: text("code")?, message: text("message")?, details })
}

fn entry_of(value: &Value) -> Result<Entry, CpdError> {
    let provider = value["provider"].as_str().filter(|p| safe_name(p)).ok_or_else(|| invalid("an entry's `provider` is not a file-name-safe text"))?;
    if value.get("error").is_some() {
        return Ok(Entry::Failed(failure_of(&value["error"], provider)?));
    }
    let auth = value["auth"].as_str().filter(|a| ["oauth", "api_key", "custom"].contains(a)).ok_or_else(|| invalid(format!("the `auth` of {provider:?} is not oauth, api_key or custom")))?;
    let base = match &value["base"] {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        _ => return Err(invalid(format!("the `base` of {provider:?} is not text or null"))),
    };
    let ids = texts(&value["ids"], &format!("the `ids` of {provider:?}"))?;
    let known = match value.get("known") {
        None | Some(Value::Null) => None,
        Some(known) => Some(texts(known, &format!("the `known` of {provider:?}"))?),
    };
    Ok(Entry::Listed(Listed { provider: provider.to_owned(), auth: auth.to_owned(), base, ids, known }))
}

impl Answer {
    /// The helper's printed value: the answer, or the failure of the whole run as an error.
    pub fn parse(value: &Value) -> Result<Answer, CpdError> {
        match value["ok"] {
            Value::Bool(false) => {
                let failure = failure_of(&value["error"], "")?;
                Err(error_of(&failure.code, &failure.message, failure.details))
            }
            Value::Bool(true) => {
                let pi_version = value["pi_version"].as_str().ok_or_else(|| invalid("`pi_version` is not text"))?.to_owned();
                let list = value["providers"].as_array().ok_or_else(|| invalid("`providers` is not an array"))?;
                let entries = list.iter().map(entry_of).collect::<Result<Vec<_>, _>>()?;
                let names: std::collections::BTreeSet<&str> = entries
                    .iter()
                    .map(|e| match e {
                        Entry::Listed(l) => l.provider.as_str(),
                        Entry::Failed(f) => f.provider.as_str(),
                    })
                    .collect();
                if names.len() != entries.len() {
                    return Err(invalid("a provider is listed twice"));
                }
                Ok(Answer { pi_version, entries })
            }
            _ => Err(invalid("`ok` is not a boolean")),
        }
    }
}

/// The helper, ready to run: the command and its argv as the caller shows it.
pub struct Helper {
    cmd: Command,
    argv: Vec<String>,
}

impl Helper {
    /// `node <pi_adapter>/lib/models-list.ts` for the machine's pi; the caller's environment is inherited. Errors:
    /// `pi_not_found`, `config_invalid` (the adapter's directory).
    pub fn new(config: &Config, home: &CpdHome) -> Result<Helper, CpdError> {
        let pi = locate_pi()?;
        let script = config.pi_adapter_dir()?.join("lib").join("models-list.ts");
        let node = node();
        let argv = vec![node.display().to_string(), script.display().to_string()];
        let mut cmd = Command::new(&node);
        cmd.arg(&script)
            .env("PI_CODING_AGENT_DIR", &pi.agent_dir)
            .env("CPD_HOME", home.root())
            // The caller's own selection is not the helper's.
            .env_remove("CPD_PLAN")
            .env_remove("CPD_SESSION")
            .env_remove("CPD_ATTEMPT")
            .current_dir(home.root())
            .stdin(Stdio::null());
        Ok(Helper { cmd, argv })
    }

    /// [`Helper::run_with`] the runner of the ports in scope ([`Ports::current`]).
    pub fn run(self) -> Result<Answer, CpdError> {
        let runner = Ports::current().runner;
        self.run_with(&*runner)
    }

    /// Runs the helper through `runner` to its end or its deadline (the real runner is `deadline::output`: the helper
    /// in its own process group, killed past the deadline). Errors: `spawn_failed`, `tool_timeout`, the helper's own
    /// code for a run that failed as a whole, `models_helper_failed`.
    pub fn run_with(mut self, runner: &dyn Runner) -> Result<Answer, CpdError> {
        let out = match runner.output(&mut self.cmd, "node", DEADLINE) {
            Ok(out) => out,
            Err(Failed::Io(e)) => {
                return Err(CpdError::new(ErrorCode::SpawnFailed, format!("Cannot run {}: {e}.", self.argv.join(" "))).with("argv", json!(self.argv)));
            }
            Err(Failed::Timeout(e)) => return Err(e),
        };
        let stdout = String::from_utf8_lossy(&out.stdout);
        match serde_json::from_str::<Value>(stdout.trim()) {
            Ok(value) => Answer::parse(&value),
            Err(_) => Err(CpdError::new(ErrorCode::SpawnFailed, format!("{} printed no result.", self.argv.join(" ")))
                .with("argv", json!(self.argv))
                .with("exit", out.status.code())
                .with("stdout", tail(&stdout, TAIL_LINES))
                .with("stderr", tail(&String::from_utf8_lossy(&out.stderr), TAIL_LINES))),
        }
    }
}
