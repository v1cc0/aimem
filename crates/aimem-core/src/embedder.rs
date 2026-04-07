//! Embedder — local text-to-vector using fastembed-rs.
//!
//! # Model
//!
//! Uses `all-MiniLM-L6-v2` (384 dimensions), the same model ChromaDB defaults to,
//! so embeddings produced here are semantically comparable to an existing ChromaDB palace.
//!
//! The ONNX model (~23 MB) is downloaded from HuggingFace Hub on first call and
//! cached at `~/.cache/huggingface/hub/`. Subsequent calls are instant.
//!
//! # Threading
//!
//! [`Embedder`] is `Clone + Send + Sync`. The inner ONNX session is wrapped in
//! `Arc<Mutex<...>>` so multiple tasks can share one instance without contention —
//! embed calls are batched internally by fastembed.
//!
//! # Example
//!
//! ```rust,no_run
//! use aimem_core::Embedder;
//!
//! let embedder = Embedder::new()?;
//!
//! // Batch embedding (preferred — amortizes ONNX overhead)
//! let vecs = embedder.embed(&["hello world", "turso vector search"])?;
//! assert_eq!(vecs[0].len(), 384); // all-MiniLM-L6-v2 dim
//!
//! // Single embedding
//! let v = embedder.embed_one("semantic memory")?;
//! assert_eq!(v.len(), 384);
//!
//! // Serialize to Turso vector32 JSON for SQL params
//! let json = Embedder::to_vector32_json(&v);
//! assert!(json.starts_with('['));
//! # anyhow::Ok(())
//! ```
//! ```

use std::sync::{Arc, Mutex};

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EmbedError {
    #[error("fastembed error: {0}")]
    Fastembed(#[from] anyhow::Error),
    #[error("mutex poisoned")]
    Poison,
}

/// Dimension of the default embedding model (all-MiniLM-L6-v2).
pub const EMBED_DIM: usize = 384;

/// Thread-safe wrapper around a fastembed `TextEmbedding` instance.
///
/// Clone is cheap — inner state is Arc-wrapped.
#[derive(Clone)]
pub struct Embedder {
    inner: Arc<Mutex<TextEmbedding>>,
}

impl std::fmt::Debug for Embedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Embedder")
            .field("model", &"AllMiniLML6V2")
            .finish()
    }
}

impl Embedder {
    /// Create a new embedder with the default model (all-MiniLM-L6-v2).
    ///
    /// On first call this will download the ONNX model (~23 MB) to
    /// `~/.cache/huggingface/hub/` if not already cached.
    pub fn new() -> Result<Self, EmbedError> {
        let model = TextEmbedding::try_new(
            TextInitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(true),
        )?;
        Ok(Self {
            inner: Arc::new(Mutex::new(model)),
        })
    }

    /// Embed a slice of strings, returning one `Vec<f32>` per input.
    pub fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let mut guard = self.inner.lock().map_err(|_| EmbedError::Poison)?;
        let results = guard.embed(texts, None)?;
        Ok(results)
    }

    /// Convenience: embed a single string.
    pub fn embed_one(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let mut results = self.embed(&[text])?;
        results
            .pop()
            .ok_or_else(|| EmbedError::Fastembed(anyhow::anyhow!("empty embedding result")))
    }

    /// Serialize a `Vec<f32>` to a JSON array string suitable for `vector32(?)`
    /// in Turso SQL.
    pub fn to_vector32_json(v: &[f32]) -> String {
        let nums: Vec<String> = v.iter().map(|f| f.to_string()).collect();
        format!("[{}]", nums.join(","))
    }
}
