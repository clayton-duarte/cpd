//! The classifier port: a small model picks one label for a question about a state.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};

use crate::atomic::write_atomic;
use crate::embedding::{Codes, run_helper};
use crate::error::{CpdError, ErrorCode};
use crate::events::{NewEvent, append};
use crate::home::CpdHome;
use crate::host::runner::Runner;
use crate::store::Store;

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

/// The notice's text (pro-d369).
const UNREACHABLE: &str =
    "http://ollama.lab/v1 is unreachable (connect Tailscale or disable in [experimental])";

/// The real classifier adapter: `node <script>` over `runner` with the choice JSON
/// `{state,question,labels}` on stdin. `script` is `pi/lib/classify.ts`; its answer is one JSON line
/// (`{ok:true,label,confidence?,model,usage?}` or `{ok:false,error}`). On `classifier_unreachable` it raises
/// one deduplicated `error` Event, kept by the marker `<home>/run/classifier-unreachable.json`; a success removes it.
pub struct PiClassifier {
    runner: Arc<dyn Runner>,
    script: PathBuf,
    agent_dir: PathBuf,
    home: CpdHome,
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
        }
    }

    fn marker(&self) -> PathBuf {
        self.home.run_dir().join("classifier-unreachable.json")
    }

    /// One `error` Event per outage; every failure is swallowed.
    fn notify(&self) {
        let marker = self.marker();
        if marker.exists() {
            return;
        }
        let _ = (|| -> Result<(), CpdError> {
            let mut store = Store::open(&self.home, 5000)?;
            store.write("classifier", |tx| {
                append(
                    tx,
                    NewEvent::error(
                        UNREACHABLE,
                        "classifier",
                        ErrorCode::ClassifierUnreachable.as_str(),
                        UNREACHABLE,
                        serde_json::json!({}),
                    ),
                )
                .map(drop)
            })?;
            let _ = write_atomic(&marker, b"{}");
            Ok(())
        })();
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
        match &result {
            Ok(_) => {
                let _ = std::fs::remove_file(self.marker());
            }
            Err(e) if e.code() == ErrorCode::ClassifierUnreachable => self.notify(),
            Err(_) => {}
        }
        result
    }
}
