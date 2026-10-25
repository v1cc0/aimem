//! Embedder — text-to-vector abstraction and implementations.
//!
//! Supports local EmbeddingGemma 2 (ONNX) and remote Gemini embedding generation.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ort::{session::Session, value::Tensor};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokenizers::{Tokenizer, TruncationParams};
use tracing::warn;

use crate::types::ContentPart;

pub const LOCAL_EMBED_PROVIDER: &str = "onnx";
pub const LOCAL_EMBED_MODEL: &str = "onnx-community/embeddinggemma-2-ONNX";
pub const LOCAL_EMBED_DIMENSION: usize = 768;
const LOCAL_MODEL_REVISION: &str = "daa72c51243991dfcaf9f9137d2c573d8f7790c0";
const LOCAL_MAX_TOKENS: usize = 8192;
pub const GEMINI_EMBED_PROVIDER: &str = "gemini";
pub const GEMINI_EMBED_MODEL: &str = "gemini-embedding-2-preview";

#[derive(Debug, Error)]
pub enum EmbedError {
    #[error("local embedding error: {0}")]
    Local(#[from] anyhow::Error),
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

    /// Embed a retrieval query. Providers without task prefixes reuse `embed_one`.
    async fn embed_query(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        self.embed_one(text).await
    }

    /// Dimension of the vectors produced by this embedder.
    fn dimension(&self) -> usize;

    /// Provider identifier used for store compatibility checks.
    fn provider_name(&self) -> &str;

    /// Model identifier used for store compatibility checks.
    fn model_name(&self) -> &str;
}

/// Local text embedder using EmbeddingGemma 2's quantized ONNX text model.
/// Only text parts of multimodal inputs are embedded; media bytes stay untouched.
#[derive(Clone)]
pub struct LocalEmbedder {
    inner: Arc<Mutex<LocalModel>>,
}

struct LocalModel {
    session: Session,
    tokenizer: Tokenizer,
}

impl std::fmt::Debug for LocalEmbedder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalEmbedder")
            .field("model", &LOCAL_EMBED_MODEL)
            .finish()
    }
}

impl LocalEmbedder {
    /// Load the 768-dimensional text model, downloading it to the HF cache once.
    pub fn new() -> Result<Self, EmbedError> {
        let model = (|| -> anyhow::Result<LocalModel> {
            let repo =
                hf_hub::HFClientSync::new()?.model("onnx-community", "embeddinggemma-2-ONNX");
            let download = |name: &str| -> anyhow::Result<std::path::PathBuf> {
                let request = repo
                    .download_file()
                    .filename(name)
                    .revision(LOCAL_MODEL_REVISION);
                // Pinned revisions resolve locally without network access once cached.
                Ok(request.send()?)
            };
            let tokenizer_path = download("tokenizer.json")?;
            let model_path = download("onnx/model_quantized.onnx")?;
            download("onnx/model_quantized.onnx_data")?;
            let mut tokenizer = Tokenizer::from_file(tokenizer_path)
                .map_err(|err| anyhow::anyhow!(err.to_string()))?;
            tokenizer
                .with_truncation(Some(TruncationParams {
                    max_length: LOCAL_MAX_TOKENS,
                    ..Default::default()
                }))
                .map_err(|err| anyhow::anyhow!(err.to_string()))?;
            tokenizer.with_padding(None);
            let env = ort::init().build()?;
            let session = Session::builder(&env)?
                .with_intra_threads(std::thread::available_parallelism()?.get().min(4))
                .map_err(ort::Error::<()>::from)?
                .commit_from_file(model_path)?;
            Ok(LocalModel { session, tokenizer })
        })()?;
        Ok(Self {
            inner: Arc::new(Mutex::new(model)),
        })
    }

    async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let mut model = inner.lock().map_err(|_| EmbedError::Poison)?;
            texts.iter().map(|text| model.embed(text)).collect()
        })
        .await
        .map_err(|err| EmbedError::Local(err.into()))?
    }
}

impl LocalModel {
    fn embed(&mut self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let mut run = || -> anyhow::Result<Vec<f32>> {
            let encoding = self
                .tokenizer
                .encode(text, true)
                .map_err(|err| anyhow::anyhow!(err.to_string()))?;
            let ids: Vec<i64> = encoding.get_ids().iter().map(|&id| i64::from(id)).collect();
            let mask: Vec<i64> = encoding
                .get_attention_mask()
                .iter()
                .map(|&m| i64::from(m))
                .collect();
            let shape = [1, ids.len()];
            // The text graph requires empty feature tensors for the unused media encoders.
            let outputs = self.session.run(ort::inputs! {
                "input_ids" => Tensor::from_array((shape, ids))?,
                "attention_mask" => Tensor::from_array((shape, mask))?,
                "image_features" => Tensor::from_array(([0, 512], Vec::<f32>::new()))?,
                "video_features" => Tensor::from_array(([0, 512], Vec::<f32>::new()))?,
                "audio_features" => Tensor::from_array(([0, 512], Vec::<f32>::new()))?,
            })?;
            let output = outputs
                .get("sentence_embedding")
                .ok_or_else(|| anyhow::anyhow!("missing sentence_embedding output"))?;
            let (shape, values) = output.try_extract_tensor::<f32>()?;
            anyhow::ensure!(
                shape.as_ref() == [1, LOCAL_EMBED_DIMENSION as i64],
                "unexpected embedding shape: {shape:?}"
            );
            let mut vector = values.to_vec();
            let norm = vector
                .iter()
                .map(|&v| f64::from(v).powi(2))
                .sum::<f64>()
                .sqrt();
            anyhow::ensure!(norm.is_finite() && norm > 0.0, "invalid embedding norm");
            for value in &mut vector {
                *value = (f64::from(*value) / norm) as f32;
            }
            Ok(vector)
        };
        Ok(run()?)
    }
}

#[async_trait]
impl Embedder for LocalEmbedder {
    async fn embed(&self, inputs: &[Vec<ContentPart>]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let texts = inputs
            .iter()
            .map(|parts| {
                let text = parts
                    .iter()
                    .filter_map(|part| part.as_text())
                    .collect::<Vec<_>>()
                    .join(" ");
                format!("title: none | text: {text}")
            })
            .collect();
        self.embed_texts(texts).await
    }

    async fn embed_query(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        self.embed_texts(vec![format!("task: search result | query: {text}")])
            .await?
            .pop()
            .ok_or_else(|| EmbedError::Local(anyhow::anyhow!("empty query embedding")))
    }

    fn dimension(&self) -> usize {
        LOCAL_EMBED_DIMENSION
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
