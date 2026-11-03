//! The embedding port: text to a vector.

use std::path::PathBuf;
use std::sync::Arc;

use crate::error::CpdError;
use crate::host::runner::Runner;

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

/// The real embedding adapter (STUB, `unimplemented!()`): `node <script>` over `runner` with the text on stdin,
/// `PI_CODING_AGENT_DIR=<agent_dir>` in its environment, and no key anywhere in argv, envs or an error.
/// `script` is `pi/lib/embed.ts`; its answer is one JSON line (`{ok:true,vector,model,tokens?}` or `{ok:false,error}`).
#[allow(dead_code)] // a stub: the implementing Job reads the fields
pub struct BifrostEmbedding {
    runner: Arc<dyn Runner>,
    script: PathBuf,
    agent_dir: PathBuf,
}

impl BifrostEmbedding {
    pub fn new(runner: Arc<dyn Runner>, script: PathBuf, agent_dir: PathBuf) -> BifrostEmbedding {
        BifrostEmbedding {
            runner,
            script,
            agent_dir,
        }
    }
}

impl EmbeddingPort for BifrostEmbedding {
    fn embed(&self, _text: &str, _dims: usize) -> Result<Embedded, CpdError> {
        unimplemented!("BifrostEmbedding::embed")
    }
}
