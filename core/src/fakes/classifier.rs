//! An in-process [`ClassifierPort`]: scripted replies, no network.

use std::sync::Mutex;

use serde_json::Value;

use crate::classifier::{Choice, Chosen, ClassifierPort};
use crate::error::CpdError;

/// One recorded request: the state, the question and the `(name, definition)` labels.
#[derive(Clone, Debug, PartialEq)]
pub struct ChoiceCall {
    pub state: Value,
    pub question: String,
    pub labels: Vec<(String, String)>,
}

/// Answers each `choose` from its script (in order, the last repeats) and records the requests.
#[derive(Debug, Default)]
pub struct FakeClassifier {
    replies: Mutex<Vec<Result<Chosen, CpdError>>>,
    calls: Mutex<Vec<ChoiceCall>>,
}

impl FakeClassifier {
    /// A fake with scripted replies, used in order; the last repeats.
    pub fn new(replies: Vec<Result<Chosen, CpdError>>) -> FakeClassifier {
        FakeClassifier {
            replies: Mutex::new(replies),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// A fake whose every `choose` answers this.
    pub fn answering(chosen: Chosen) -> FakeClassifier {
        FakeClassifier::new(vec![Ok(chosen)])
    }

    /// Every request, in order.
    pub fn calls(&self) -> Vec<ChoiceCall> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }
}

impl ClassifierPort for FakeClassifier {
    fn choose(&self, q: &Choice) -> Result<Chosen, CpdError> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(ChoiceCall {
                state: q.state.clone(),
                question: q.question.to_string(),
                labels: q
                    .labels
                    .iter()
                    .map(|l| (l.name.to_string(), l.definition.to_string()))
                    .collect(),
            });
        }
        let mut replies = self.replies.lock().expect("script lock");
        if replies.len() > 1 {
            replies.remove(0)
        } else {
            replies
                .first()
                .cloned()
                .expect("FakeClassifier needs at least one scripted reply")
        }
    }
}
