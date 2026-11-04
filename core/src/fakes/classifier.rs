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
    /// `(label name, reply)`: a request that offers the label gets the reply, whatever its order.
    routes: Vec<(String, Result<Chosen, CpdError>)>,
}

impl FakeClassifier {
    /// A fake with scripted replies, used in order; the last repeats.
    pub fn new(replies: Vec<Result<Chosen, CpdError>>) -> FakeClassifier {
        FakeClassifier {
            replies: Mutex::new(replies),
            calls: Mutex::new(Vec::new()),
            routes: Vec::new(),
        }
    }

    /// A fake that answers by the labels a request offers: the first route whose label name is offered replies.
    /// A request no route matches is answered from `fallback`.
    pub fn routed(
        routes: Vec<(&str, Result<Chosen, CpdError>)>,
        fallback: Result<Chosen, CpdError>,
    ) -> FakeClassifier {
        FakeClassifier {
            routes: routes
                .into_iter()
                .map(|(label, reply)| (label.to_string(), reply))
                .collect(),
            ..FakeClassifier::new(vec![fallback])
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
        if let Some((_, reply)) = self
            .routes
            .iter()
            .find(|(label, _)| q.labels.iter().any(|l| l.name == label))
        {
            return reply.clone();
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
