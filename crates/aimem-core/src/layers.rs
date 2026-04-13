//! 4-layer memory stack — mirrors Python's layers.py.
//!
//! ```text
//! L0  Identity        (~100 tokens)   Always loaded — identity.txt
//! L1  Essential Story (~500-800 tok)  Top drawers from AiMem
//! L2  On-Demand       (~200-500 tok)  Wing/room filtered retrieval
//! L3  Deep Search     (unlimited)     Turso vector_distance_cos semantic search
//! ```

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use thiserror::Error;

use crate::{
    config::Config,
    db::{AimemDb, DbError},
    embedder::Embedder,
    search::{SearchError, Searcher},
    types::{Drawer, DrawerFilingRequest},
};

#[derive(Debug, Error)]
pub enum LayerError {
    #[error("db: {0}")]
    Db(#[from] DbError),
    #[error("search: {0}")]
    Search(#[from] SearchError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

// ── L0 — Identity ────────────────────────────────────────────────────────────

/// Layer 0: identity text (~100 tokens, always loaded).
pub async fn l0_render(identity_path: &Path) -> String {
    if identity_path.exists() {
        match tokio::fs::read_to_string(identity_path).await {
            Ok(text) => return text.trim().to_string(),
            Err(_) => {}
        }
    }
    "## L0 — IDENTITY\nNo identity configured. Create ~/.aimem/identity.txt".to_string()
}

// ── L1 — Essential Story ─────────────────────────────────────────────────────

const L1_MAX_DRAWERS: usize = 15;
const L1_MAX_CHARS: usize = 3200;

/// Layer 1: top drawers from AiMem, grouped by room (~500-800 tokens).
pub async fn l1_generate(db: &AimemDb, wing: Option<&str>) -> Result<String, LayerError> {
    let drawers = db.fetch_drawers(wing, None, 200).await?;

    if drawers.is_empty() {
        return Ok("## L1 — No memories yet.".to_string());
    }

    // Group by room, take top L1_MAX_DRAWERS
    let mut by_room: HashMap<String, Vec<Drawer>> = HashMap::new();
    for d in drawers.into_iter().take(L1_MAX_DRAWERS) {
        by_room.entry(d.room.clone()).or_default().push(d);
    }

    let mut lines = vec!["## L1 — ESSENTIAL STORY".to_string()];
    let mut total_len = 0usize;

    let mut rooms: Vec<_> = by_room.iter().collect();
    rooms.sort_by_key(|(r, _)| r.as_str());

    for (room, entries) in rooms {
        let room_line = format!("\n[{}]", room);
        lines.push(room_line.clone());
        total_len += room_line.len();

        for d in entries {
            let snippet: String = d
                .content
                .trim()
                .replace('\n', " ")
                .chars()
                .take(200)
                .collect();
            let snippet = if d.content.len() > 200 {
                format!("{}...", snippet)
            } else {
                snippet
            };

            let source_tag = d
                .source_file
                .as_deref()
                .and_then(|s| Path::new(s).file_name())
                .map(|n| format!("  ({})", n.to_string_lossy()))
                .unwrap_or_default();

            let entry = format!("  - {}{}", snippet, source_tag);

            if total_len + entry.len() > L1_MAX_CHARS {
                lines.push("  ... (more in L3 search)".to_string());
                return Ok(lines.join("\n"));
            }
            total_len += entry.len();
            lines.push(entry);
        }
    }

    Ok(lines.join("\n"))
}

// ── L2 — On-Demand ───────────────────────────────────────────────────────────

/// Layer 2: on-demand wing/room filtered retrieval (~200-500 tokens).
pub async fn l2_retrieve(
    db: &AimemDb,
    wing: Option<&str>,
    room: Option<&str>,
    n: usize,
) -> Result<String, LayerError> {
    let drawers = db.fetch_drawers(wing, room, n).await?;

    if drawers.is_empty() {
        let label = match (wing, room) {
            (Some(w), Some(r)) => format!("wing={w} room={r}"),
            (Some(w), None) => format!("wing={w}"),
            (None, Some(r)) => format!("room={r}"),
            (None, None) => "all".to_string(),
        };
        return Ok(format!("No drawers found for {label}."));
    }

    let mut lines = vec![format!("## L2 — ON-DEMAND ({} drawers)", drawers.len())];
    for d in &drawers {
        let snippet: String = d
            .content
            .trim()
            .replace('\n', " ")
            .chars()
            .take(300)
            .collect();
        let snippet = if d.content.len() > 300 {
            format!("{}...", snippet)
        } else {
            snippet
        };
        let source_tag = d
            .source_file
            .as_deref()
            .and_then(|s| Path::new(s).file_name())
            .map(|n| format!("  ({})", n.to_string_lossy()))
            .unwrap_or_default();
        lines.push(format!("  [{}] {}{}", d.room, snippet, source_tag));
    }
    Ok(lines.join("\n"))
}

// ── L3 — Deep Search ─────────────────────────────────────────────────────────

/// Layer 3: full semantic search via Turso `vector_distance_cos`.
pub async fn l3_search(
    searcher: &Searcher,
    query: &str,
    wing: Option<&str>,
    room: Option<&str>,
    n: usize,
) -> Result<String, LayerError> {
    let results = searcher.vector_search(query, wing, room, n).await?;

    if results.is_empty() {
        return Ok("No results found.".to_string());
    }

    let mut lines = vec![format!("## L3 — SEARCH RESULTS for \"{}\"", query)];
    for (i, r) in results.iter().enumerate() {
        let snippet: String = r
            .drawer
            .content
            .trim()
            .replace('\n', " ")
            .chars()
            .take(300)
            .collect();
        let snippet = if r.drawer.content.len() > 300 {
            format!("{}...", snippet)
        } else {
            snippet
        };
        let source = r
            .drawer
            .source_file
            .as_deref()
            .and_then(|s| Path::new(s).file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        lines.push(format!(
            "  [{}] {}/{} (sim={:.3})",
            i + 1,
            r.drawer.wing,
            r.drawer.room,
            r.similarity,
        ));
        lines.push(format!("      {}", snippet));
        if !source.is_empty() {
            lines.push(format!("      src: {}", source));
        }
    }
    Ok(lines.join("\n"))
}

// ── MemoryStack — unified interface ──────────────────────────────────────────

/// Full 4-layer memory stack.
#[derive(Debug, Clone)]
pub struct MemoryStack {
    db: AimemDb,
    searcher: Searcher,
    identity_path: std::path::PathBuf,
}

impl MemoryStack {
    pub fn new(db: AimemDb, embedder: Arc<dyn Embedder>, cfg: &Config) -> Self {
        let searcher = Searcher::new(db.clone(), embedder);
        Self {
            db,
            searcher,
            identity_path: cfg.identity_path.clone(),
        }
    }

    /// Generate wake-up context: L0 (identity) + L1 (essential story).
    /// ~600-900 tokens total. Inject into system prompt or first message.
    pub async fn wake_up(&self, wing: Option<&str>) -> Result<String, LayerError> {
        let l0 = l0_render(&self.identity_path).await;
        let l1 = l1_generate(&self.db, wing).await?;
        Ok(format!("{}\n\n{}", l0, l1))
    }

    /// On-demand L2 retrieval filtered by wing/room.
    pub async fn recall(
        &self,
        wing: Option<&str>,
        room: Option<&str>,
    ) -> Result<String, LayerError> {
        l2_retrieve(&self.db, wing, room, 10).await
    }

    /// Deep L3 semantic search.
    pub async fn search(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
    ) -> Result<String, LayerError> {
        l3_search(&self.searcher, query, wing, room, 5).await
    }

    /// File a new memory (drawer) into the L1-L3 store.
    /// This handles embedding generation and DB insertion automatically.
    pub async fn file_drawer(
        &self,
        wing: &str,
        room: &str,
        content: String,
        parts: Vec<crate::types::ContentPart>,
        agent: &str,
    ) -> Result<String, LayerError> {
        let now = chrono::Utc::now().to_rfc3339();
        let digest = md5::compute(format!("{wing}{room}{content}{now}").as_bytes());
        let id = format!("mem_{wing}_{digest:x}");

        self.file_drawer_with_id(&id, wing, room, content, parts, None, 0, agent)
            .await?;
        Ok(id)
    }

    /// File a new memory with a caller-supplied stable drawer ID and optional metadata.
    /// Returns `true` when a new drawer row was inserted and `false` when the ID already existed.
    pub async fn file_drawer_with_id(
        &self,
        id: &str,
        wing: &str,
        room: &str,
        content: String,
        parts: Vec<crate::types::ContentPart>,
        source_file: Option<&str>,
        chunk_index: i64,
        agent: &str,
    ) -> Result<bool, LayerError> {
        let request = build_filing_request(
            id,
            wing,
            room,
            content,
            parts,
            source_file,
            chunk_index,
            agent,
        );
        let mut results = self.file_drawers_with_ids(&[request]).await?;
        Ok(results.pop().unwrap_or(false))
    }

    /// File multiple stable-ID drawers in one embedding batch.
    ///
    /// This is useful for attachment-style ingestion where a caller has a set of
    /// related chunks (for example one file's summary + chunk drawers) and wants
    /// to avoid one remote embedding request per drawer.
    ///
    /// The returned vector is aligned with `requests`: `true` means a new drawer
    /// row was inserted, `false` means the stable ID already existed.
    ///
    /// Existing IDs are checked before embedding so retries do not spend another
    /// remote embedding call on already-filed drawers.
    pub async fn file_drawers_with_ids(
        &self,
        requests: &[DrawerFilingRequest],
    ) -> Result<Vec<bool>, LayerError> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }

        let mut results = vec![false; requests.len()];
        let mut pending_indexes = Vec::new();
        let mut pending_parts = Vec::new();
        let mut seen_ids = HashSet::new();

        for (index, request) in requests.iter().enumerate() {
            if !seen_ids.insert(request.id.clone()) {
                continue;
            }
            if self.db.drawer_exists(&request.id).await? {
                continue;
            }
            pending_indexes.push(index);
            pending_parts.push(parts_for_embedding(request));
        }

        let embedder = self.searcher.embedder();
        let embeddings = if let Some(ref embedder) = embedder {
            if pending_parts.is_empty() {
                Some(Vec::new())
            } else {
                let embeddings = embedder
                    .embed(&pending_parts)
                    .await
                    .map_err(SearchError::from)?;
                if embeddings.len() != pending_parts.len() {
                    return Err(SearchError::EmbedBatchSizeMismatch {
                        expected: pending_parts.len(),
                        actual: embeddings.len(),
                    }
                    .into());
                }
                Some(embeddings)
            }
        } else {
            None
        };

        let default_filed_at = chrono::Utc::now().to_rfc3339();

        for (pending_offset, request_index) in pending_indexes.into_iter().enumerate() {
            let request = &requests[request_index];
            let drawer = drawer_from_request(request, &default_filed_at);
            let inserted = if let (Some(embedder), Some(embeddings)) =
                (embedder.as_ref(), embeddings.as_ref())
            {
                self.db
                    .insert_drawer_with_profile(
                        &drawer,
                        Some(embeddings[pending_offset].as_slice()),
                        embedder.provider_name(),
                        embedder.model_name(),
                    )
                    .await?
            } else {
                self.db.insert_drawer(&drawer, None).await?
            };
            results[request_index] = inserted;
        }

        Ok(results)
    }

    /// File a plain-text memory without manually constructing a parts vector.
    pub async fn file_text(
        &self,
        wing: &str,
        room: &str,
        content: impl Into<String>,
        agent: &str,
    ) -> Result<String, LayerError> {
        self.file_drawer(wing, room, content.into(), Vec::new(), agent)
            .await
    }

    /// Access the underlying searcher.
    pub fn searcher(&self) -> &Searcher {
        &self.searcher
    }

    /// Status of the whole stack.
    pub async fn status(&self) -> Result<serde_json::Value, LayerError> {
        let count = self.db.drawer_count().await?;
        let (wings, rooms) = self.db.taxonomy().await?;
        let l0_exists = self.identity_path.exists();
        let l0_tokens = if l0_exists {
            std::fs::read_to_string(&self.identity_path)
                .map(|s| s.len() / 4)
                .unwrap_or(0)
        } else {
            0
        };

        Ok(serde_json::json!({
            "total_drawers": count,
            "identity_path": self.identity_path.display().to_string(),
            "identity_exists": l0_exists,
            "identity_tokens_est": l0_tokens,
            "wings": wings.into_iter().map(|(w, c)| serde_json::json!({"wing": w, "count": c})).collect::<Vec<_>>(),
            "rooms": rooms.into_iter().map(|(r, c)| serde_json::json!({"room": r, "count": c})).collect::<Vec<_>>(),
        }))
    }
}

fn build_filing_request(
    id: &str,
    wing: &str,
    room: &str,
    content: String,
    parts: Vec<crate::types::ContentPart>,
    source_file: Option<&str>,
    chunk_index: i64,
    agent: &str,
) -> DrawerFilingRequest {
    let mut request = DrawerFilingRequest::new(id, wing, room, content, agent)
        .with_parts(parts)
        .with_chunk_index(chunk_index);
    if let Some(source_file) = source_file {
        request = request.with_source_file(source_file);
    }
    request
}

fn parts_for_embedding(request: &DrawerFilingRequest) -> Vec<crate::types::ContentPart> {
    if request.parts.is_empty() {
        vec![crate::types::ContentPart::text(request.content.clone())]
    } else {
        request.parts.clone()
    }
}

fn drawer_from_request(request: &DrawerFilingRequest, default_filed_at: &str) -> Drawer {
    let mut drawer = Drawer::new(
        request.id.clone(),
        request.wing.clone(),
        request.room.clone(),
        request.content.clone(),
        request.added_by.clone(),
    )
    .with_parts(request.parts.clone())
    .with_chunk_index(request.chunk_index)
    .with_filed_at(
        request
            .filed_at
            .clone()
            .unwrap_or_else(|| default_filed_at.to_string()),
    );

    if let Some(source_file) = request.source_file.as_deref() {
        drawer = drawer.with_source_file(source_file);
    }

    drawer
}
