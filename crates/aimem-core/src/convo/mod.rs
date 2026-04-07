//! Conversation miner — mine chat exports into the palace.
//!
//! Supported formats (via normalize module):
//!   - Plain text with `>` markers
//!   - Claude.ai JSON export
//!   - ChatGPT conversations.json
//!   - Claude Code JSONL
//!   - Slack JSON export

pub mod normalize;

use std::path::{Path, PathBuf};

use thiserror::Error;
use walkdir::WalkDir;

use crate::{
    db::{AimemDb, DbError},
    embedder::{EmbedError, Embedder},
    miner::{MIN_CHUNK, chunk_text},
    types::Drawer,
};

#[derive(Debug, Error)]
pub enum ConvoMinerError {
    #[error("db: {0}")]
    Db(#[from] DbError),
    #[error("embed: {0}")]
    Embed(#[from] EmbedError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

const CONVO_EXTENSIONS: &[&str] = &["txt", "md", "json", "jsonl"];
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    "env",
    "dist",
    "build",
    ".aimem",
    "target",
];

#[derive(Debug, Default)]
pub struct ConvoMineStats {
    pub files_scanned: usize,
    pub files_skipped: usize,
    pub drawers_added: usize,
}

/// Conversation miner.
#[derive(Debug, Clone)]
pub struct ConvoMiner {
    db: AimemDb,
    embedder: Option<Embedder>,
}

impl ConvoMiner {
    pub fn new(db: AimemDb, embedder: Option<Embedder>) -> Self {
        Self { db, embedder }
    }

    /// Mine a directory of conversation exports into the palace.
    pub async fn mine(
        &self,
        convo_dir: impl AsRef<Path>,
        wing: &str,
        room: &str,
        agent: &str,
        limit: usize,
        dry_run: bool,
    ) -> Result<ConvoMineStats, ConvoMinerError> {
        let convo_dir = convo_dir.as_ref();
        let files = scan_convo_files(convo_dir);
        let files: Vec<_> = if limit > 0 {
            files.into_iter().take(limit).collect()
        } else {
            files
        };

        let mut stats = ConvoMineStats::default();
        stats.files_scanned = files.len();

        for filepath in &files {
            let source = filepath.to_string_lossy().to_string();

            if !dry_run && self.db.source_already_mined(&source).await? {
                stats.files_skipped += 1;
                continue;
            }

            let content = match std::fs::read_to_string(filepath) {
                Ok(c) => c,
                Err(_) => continue,
            };

            // Normalize to plain transcript
            let normalized = normalize::normalize_content(&content, filepath);
            if normalized.trim().len() < MIN_CHUNK {
                continue;
            }

            let chunks = chunk_exchanges(&normalized);
            if chunks.is_empty() {
                continue;
            }

            if dry_run {
                tracing::info!(
                    "[DRY RUN] {} → {}/{} ({} chunks)",
                    filepath.file_name().unwrap_or_default().to_string_lossy(),
                    wing,
                    room,
                    chunks.len()
                );
                stats.drawers_added += chunks.len();
                continue;
            }

            let embeddings: Vec<Option<Vec<f32>>> = if let Some(ref emb) = self.embedder {
                let texts: Vec<&str> = chunks.iter().map(|c| c.as_str()).collect();
                match emb.embed(&texts) {
                    Ok(vecs) => vecs.into_iter().map(Some).collect(),
                    Err(e) => {
                        tracing::warn!("embedding failed for {source}: {e}");
                        vec![None; chunks.len()]
                    }
                }
            } else {
                vec![None; chunks.len()]
            };

            let now = chrono::Utc::now().to_rfc3339();
            for (i, (chunk, embedding)) in chunks.iter().zip(embeddings.iter()).enumerate() {
                let id = {
                    let input = format!("{wing}{room}{source}{i}");
                    let digest = md5::compute(input.as_bytes());
                    format!("drawer_{wing}_{room}_{digest:x}")
                };
                let drawer = Drawer {
                    id,
                    wing: wing.to_string(),
                    room: room.to_string(),
                    content: chunk.clone(),
                    source_file: Some(source.clone()),
                    chunk_index: i as i64,
                    added_by: agent.to_string(),
                    filed_at: now.clone(),
                };
                if self.db.insert_drawer(&drawer, embedding.as_deref()).await? {
                    stats.drawers_added += 1;
                }
            }
        }

        Ok(stats)
    }
}

/// Chunk a conversation by exchange pairs (user turn + AI response = 1 unit).
pub fn chunk_exchanges(content: &str) -> Vec<String> {
    let lines: Vec<&str> = content.lines().collect();
    let quote_count = lines
        .iter()
        .filter(|l| l.trim_start().starts_with('>'))
        .count();

    if quote_count >= 3 {
        chunk_by_exchange(&lines)
    } else {
        chunk_text(content)
    }
}

fn chunk_by_exchange(lines: &[&str]) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        if line.starts_with('>') {
            let user_turn = line.to_string();
            i += 1;

            let mut ai_lines = Vec::new();
            while i < lines.len() {
                let next = lines[i].trim();
                if next.starts_with('>') || next == "---" {
                    break;
                }
                ai_lines.push(lines[i]);
                i += 1;
            }

            let ai_response = ai_lines.join("\n");
            let chunk = format!("{}\n{}", user_turn, ai_response);
            if chunk.trim().len() >= MIN_CHUNK {
                chunks.push(chunk);
            }
        } else {
            i += 1;
        }
    }

    // Fallback: if we got nothing, just use paragraph chunking
    if chunks.is_empty() {
        chunk_text(&lines.join("\n"))
    } else {
        chunks
    }
}

fn scan_convo_files(dir: &Path) -> Vec<PathBuf> {
    WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !SKIP_DIRS.contains(&name.as_ref())
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            let ext = e
                .path()
                .extension()
                .map(|x| x.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            CONVO_EXTENSIONS.contains(&ext.as_str())
        })
        .map(|e| e.into_path())
        .collect()
}
