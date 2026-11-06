//! The classifier port: a small model picks one label for a question about a state.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::clock::{Clock, SystemClock};
use crate::embedding::{Codes, run_helper};
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::runner::Runner;
use crate::swr::health::record_result;

/// A question with its labels, over a JSON state.
#[derive(Clone, Copy, Debug)]
pub struct Choice<'a> {
    pub state: &'a serde_json::Value,
    pub question: &'a str,
    pub labels: &'a [Label<'a>],
}

/// One label the classifier may choose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Label<'a> {
    pub name: &'a str,
    pub definition: &'a str,
}

/// Token usage of one classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// The label chosen.
#[derive(Clone, Debug, PartialEq)]
pub struct Chosen {
    pub label: String,
    pub confidence: Option<f32>,
    pub usage: Option<Usage>,
    pub model: String,
}

/// Chooses one label.
pub trait ClassifierPort: Send + Sync {
    fn choose(&self, q: &Choice) -> Result<Chosen, CpdError>;
}

/// The real classifier adapter: `node <script>` over `runner` with the choice JSON
/// `{state,question,labels}` on stdin. `script` is `pi/lib/classify.ts`; its answer is one JSON line
/// (`{ok:true,label,confidence?,model,usage?}` or `{ok:false,error}`). Each result is recorded on
/// the `ollama.lab` endpoint (`swr::health`); the engine raises the outage.
pub struct PiClassifier {
    runner: Arc<dyn Runner>,
    script: PathBuf,
    agent_dir: PathBuf,
    home: CpdHome,
    clock: Arc<dyn Clock>,
}

impl PiClassifier {
    pub fn new(
        runner: Arc<dyn Runner>,
        script: PathBuf,
        agent_dir: PathBuf,
        home: &CpdHome,
    ) -> PiClassifier {
        PiClassifier {
            runner,
            script,
            agent_dir,
            home: home.clone(),
            clock: Arc::new(SystemClock),
        }
    }

    /// The clock of the endpoint-health records.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> PiClassifier {
        self.clock = clock;
        self
    }

    fn ask(&self, q: &Choice) -> Result<Chosen, CpdError> {
        let labels: Vec<Value> = q
            .labels
            .iter()
            .map(|l| json!({"name": l.name, "definition": l.definition}))
            .collect();
        let stdin = json!({"state": q.state, "question": q.question, "labels": labels});
        let answer = run_helper(
            &*self.runner,
            &self.script,
            &self.agent_dir,
            Vec::new(),
            stdin.to_string().into_bytes(),
            Codes {
                unreachable: ErrorCode::ClassifierUnreachable,
                refused: ErrorCode::ClassifierRefused,
                bad_reply: ErrorCode::ClassifierBadReply,
            },
        )?;
        let bad = |why: &str| CpdError::new(ErrorCode::ClassifierBadReply, why.to_owned());
        let label = answer
            .get("label")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("The answer has no label."))?;
        if !q.labels.iter().any(|l| l.name == label) {
            return Err(bad("The label is not one of the choice.").with("label", label));
        }
        let model = answer
            .get("model")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("The answer has no model."))?
            .to_owned();
        let usage = answer.get("usage").and_then(|u| {
            Some(Usage {
                input_tokens: u.get("input_tokens")?.as_u64()?,
                output_tokens: u.get("output_tokens")?.as_u64()?,
            })
        });
        Ok(Chosen {
            label: label.to_owned(),
            confidence: answer
                .get("confidence")
                .and_then(Value::as_f64)
                .map(|c| c as f32),
            usage,
            model,
        })
    }
}

impl ClassifierPort for PiClassifier {
    fn choose(&self, q: &Choice) -> Result<Chosen, CpdError> {
        let result = self.ask(q);
        record_result(&self.home, "ollama.lab", self.clock.utc(), &result);
        result
    }
}

#[cfg(test)]
#[path = "classifier_swr_tests.rs"]
mod classifier_swr_tests;
