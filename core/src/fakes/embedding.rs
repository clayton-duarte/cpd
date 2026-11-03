//! An in-process [`EmbeddingPort`]: scripted replies, no network.

use crate::embedding::{Embedded, EmbeddingPort};
use crate::error::CpdError;

/// Answers each `embed` from its script (in order, the last repeats) and records `(text, dims)`.
#[derive(Debug, Default)]
pub struct FakeEmbedding {
    // The script and the record are the implementer's to add.
}

impl FakeEmbedding {
    pub fn new(_replies: Vec<Result<Embedded, CpdError>>) -> FakeEmbedding {
        unimplemented!()
    }

    pub fn answering(_embedded: Embedded) -> FakeEmbedding {
        unimplemented!()
    }

    pub fn calls(&self) -> Vec<(String, usize)> {
        unimplemented!()
    }
}

impl EmbeddingPort for FakeEmbedding {
    fn embed(&self, _text: &str, _dims: usize) -> Result<Embedded, CpdError> {
        unimplemented!()
    }
}
