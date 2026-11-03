//! The classifier port: a small model picks one label for a question about a state.

use crate::error::CpdError;

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
