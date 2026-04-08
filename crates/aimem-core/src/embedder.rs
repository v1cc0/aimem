//! Embedder — text-to-vector abstraction and implementations.
//!
//! Supports local (fastembed) and remote (Gemini 2.0) embedding generation.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::warn;

use crate::types::ContentPart;

pub const LOCAL_EMBED_PROVIDER: &str = "fastembed";
pub const LOCAL_EMBED_MODEL: &str = "all-MiniLM-L6-v2";
pub const GEMINI_EMBED_PROVIDER: &str = "gemini";
pub const GEMINI_EMBED_MODEL: &str = "gemini-embedding-2-preview";

#[derive(Debug, Error)]
pub enum EmbedError {
    #[error("fastembed error: {0}")]
    Fastembed(#[from] anyhow::Error),
    #[error("mutex poisoned")]
    Poison,
    #[error("remote api error: {0}")]
    Remote(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Core trait for generating multimodal embeddings.
#[async_trait]
pub trait Embedder: Send + Sync {
    /// Embed a slice of multimodal content, returning one `Vec<f32>` per input.
    async fn embed(&self, inputs: &[Vec<ContentPart>]) -> Result<Vec<Vec<f32>>, EmbedError>;

    /// Convenience: embed a single string.
    async fn embed_one(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let mut results = self.embed(&[vec![ContentPart::text(text)]]).await?;
        results
            .pop()
            .ok_or_else(|| EmbedError::Remote("empty embedding result".to_string()))
    }

    /// Dimension of the vectors produced by this embedder.
    fn dimension(&self) -> usize;

    /// Provider identifier used for store compatibility checks.
    fn provider_name(&self) -> &str;

    /// Model identifier used for store compatibility checks.
    fn model_name(&self) -> &str;
}

/// Local embedder using fastembed-rs.
/// Currently only uses the text parts of multimodal input.
#[derive(Clone)]
pub struct LocalEmbedder {
    inner: Arc<Mutex<TextEmbedding>>,
}

impl std::fmt::Debug for LocalEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalEmbedder")
            .field("model", &LOCAL_EMBED_MODEL)
            .finish()
    }
}

impl LocalEmbedder {
    /// Create a new local embedder with the default model (all-MiniLM-L6-v2, 384 dims).
    pub fn new() -> Result<Self, EmbedError> {
        let model = TextEmbedding::try_new(
            TextInitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(true),
        )?;
        Ok(Self {
            inner: Arc::new(Mutex::new(model)),
        })
    }
}

#[async_trait]
impl Embedder for LocalEmbedder {
    async fn embed(&self, inputs: &[Vec<ContentPart>]) -> Result<Vec<Vec<f32>>, EmbedError> {
        // Local embedder only supports text. Flatten multimodal parts into a single string.
        let texts: Vec<String> = inputs
            .iter()
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p.as_text())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();

        let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();

        let mut guard = self.inner.lock().map_err(|_| EmbedError::Poison)?;
        let results = guard.embed(text_refs, None)?;
        Ok(results)
    }

    fn dimension(&self) -> usize {
        384
    }

    fn provider_name(&self) -> &str {
        LOCAL_EMBED_PROVIDER
    }

    fn model_name(&self) -> &str {
        LOCAL_EMBED_MODEL
    }
}

/// Remote embedder using Google Gemini Embedding 2 API.
pub struct Gemini2Embedder {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

impl Gemini2Embedder {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            model: GEMINI_EMBED_MODEL.to_string(),
            client: reqwest::Client::new(),
        }
    }
}

#[derive(Serialize)]
struct GeminiEmbedRequest {
    requests: Vec<EmbedContentRequest>,
}

#[derive(Serialize)]
struct EmbedContentRequest {
    model: String,
    content: GeminiContent,
}

#[derive(Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum GeminiPart {
    Text(String),
    InlineData { mime_type: String, data: String },
}

#[derive(Deserialize)]
struct GeminiBatchEmbedResponse {
    embeddings: Vec<GeminiEmbedding>,
}

#[derive(Deserialize)]
struct GeminiEmbedding {
    values: Vec<f32>,
}

#[async_trait]
impl Embedder for Gemini2Embedder {
    async fn embed(&self, inputs: &[Vec<ContentPart>]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:batchEmbedContents?key={}",
            self.model, self.api_key
        );

        let mut requests = Vec::new();
        for parts in inputs {
            let mut gemini_parts = Vec::new();
            for part in parts {
                match part {
                    ContentPart::Text { text } => {
                        gemini_parts.push(GeminiPart::Text(text.clone()));
                    }
                    ContentPart::Image { uri, mime, data }
                    | ContentPart::Audio { uri, mime, data }
                    | ContentPart::Video { uri, mime, data } => {
                        if let Some(bytes) = data {
                            // Explicitly provided bytes (Recommended)
                            let encoded = base64::Engine::encode(
                                &base64::engine::general_purpose::STANDARD,
                                bytes,
                            );
                            gemini_parts.push(GeminiPart::InlineData {
                                mime_type: mime.clone(),
                                data: encoded,
                            });
                        } else if let Some(uri_str) = uri {
                            if uri_str.starts_with("data:") {
                                // Explicitly provided data URI
                                if let Some(comma_pos) = uri_str.find(',') {
                                    let data_part = uri_str[comma_pos + 1..].to_string();
                                    gemini_parts.push(GeminiPart::InlineData {
                                        mime_type: mime.clone(),
                                        data: data_part,
                                    });
                                }
                            } else {
                                // We NO LONGER read local files automatically.
                                // If the caller wants to embed a file, they must read it into bytes first.
                                warn!(
                                    "Skipping remote embedding for URI-only content part: {}. Provide explicit data bytes for remote embedding.",
                                    uri_str
                                );
                            }
                        }
                    }
                }
            }

            if gemini_parts.is_empty() {
                return Err(EmbedError::Remote(
                    "remote embedding requires explicit text, data URI, or raw bytes; uri-only parts are not uploaded automatically".to_string(),
                ));
            }

            requests.push(EmbedContentRequest {
                model: format!("models/{}", self.model),
                content: GeminiContent {
                    parts: gemini_parts,
                },
            });
        }

        let req_body = GeminiEmbedRequest { requests };
        let resp = self
            .client
            .post(&url)
            .json(&req_body)
            .send()
            .await
            .map_err(|e| EmbedError::Remote(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err_text = resp
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(EmbedError::Remote(format!(
                "Gemini API error ({status}): {err_text}"
            )));
        }

        let resp_body: GeminiBatchEmbedResponse = resp
            .json()
            .await
            .map_err(|e| EmbedError::Remote(e.to_string()))?;

        Ok(resp_body.embeddings.into_iter().map(|e| e.values).collect())
    }

    fn dimension(&self) -> usize {
        768
    }

    fn provider_name(&self) -> &str {
        GEMINI_EMBED_PROVIDER
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

/// Helper to serialize a `Vec<f32>` to a JSON array string for Turso vector32.
pub fn to_vector32_json(v: &[f32]) -> String {
    let nums: Vec<String> = v.iter().map(|f| f.to_string()).collect();
    format!("[{}]", nums.join(","))
}
