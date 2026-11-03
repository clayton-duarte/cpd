//! An in-process [`EmbeddingPort`]: scripted replies, no network.

use std::sync::Mutex;

use crate::embedding::{Embedded, EmbeddingPort};
use crate::error::CpdError;

/// Answers each `embed` from its script (in order, the last repeats) and records `(text, dims)`.
#[derive(Debug, Default)]
pub struct FakeEmbedding {
    replies: Mutex<Vec<Result<Embedded, CpdError>>>,
    calls: Mutex<Vec<(String, usize)>>,
}

impl FakeEmbedding {
    /// A fake with scripted replies, used in order; the last repeats.
    pub fn new(replies: Vec<Result<Embedded, CpdError>>) -> FakeEmbedding {
        FakeEmbedding {
            replies: Mutex::new(replies),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// A fake whose every `embed` answers this.
    pub fn answering(embedded: Embedded) -> FakeEmbedding {
        FakeEmbedding::new(vec![Ok(embedded)])
    }

    /// Every `(text, dims)`, in order.
    pub fn calls(&self) -> Vec<(String, usize)> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }
}

impl EmbeddingPort for FakeEmbedding {
    fn embed(&self, text: &str, dims: usize) -> Result<Embedded, CpdError> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((text.to_string(), dims));
        }
        let mut replies = self.replies.lock().expect("script lock");
        if replies.len() > 1 {
            replies.remove(0)
        } else {
            replies
                .first()
                .cloned()
                .expect("FakeEmbedding needs at least one scripted reply")
        }
    }
}
