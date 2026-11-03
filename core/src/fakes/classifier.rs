//! An in-process [`ClassifierPort`]: scripted replies, no network.

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
    // The script and the record are the implementer's to add.
}

impl FakeClassifier {
    pub fn new(_replies: Vec<Result<Chosen, CpdError>>) -> FakeClassifier {
        unimplemented!()
    }

    pub fn answering(_chosen: Chosen) -> FakeClassifier {
        unimplemented!()
    }

    pub fn calls(&self) -> Vec<ChoiceCall> {
        unimplemented!()
    }
}

impl ClassifierPort for FakeClassifier {
    fn choose(&self, _q: &Choice) -> Result<Chosen, CpdError> {
        unimplemented!()
    }
}
