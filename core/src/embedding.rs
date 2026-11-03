//! The embedding port: text to a vector.

use crate::error::CpdError;

/// A vector with the model that made it.
#[derive(Clone, Debug, PartialEq)]
pub struct Embedded {
    pub vector: Vec<f32>,
    pub model: String,
    pub tokens: Option<u64>,
}

/// Embeds text at a dimension count.
pub trait EmbeddingPort: Send + Sync {
    fn embed(&self, text: &str, dims: usize) -> Result<Embedded, CpdError>;
}
