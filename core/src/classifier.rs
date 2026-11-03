//! The classifier port: a small model picks one label for a question about a state.

use std::path::PathBuf;
use std::sync::Arc;

use crate::error::CpdError;
use crate::home::CpdHome;
use crate::host::runner::Runner;

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

/// The real classifier adapter (STUB, `unimplemented!()`): `node <script>` over `runner` with the choice JSON
/// `{state,question,labels}` on stdin. `script` is `pi/lib/classify.ts`; its answer is one JSON line
/// (`{ok:true,label,confidence?,model,usage?}` or `{ok:false,error}`). On `classifier_unreachable` it raises
/// one deduplicated `error` Event, kept by the marker `<home>/run/classifier-unreachable.json`; a success removes it.
#[allow(dead_code)] // a stub: the implementing Job reads the fields
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
}

impl ClassifierPort for PiClassifier {
    fn choose(&self, _q: &Choice) -> Result<Chosen, CpdError> {
        unimplemented!("PiClassifier::choose")
    }
}
