//! File miner — scan a project directory and file chunks into AiMem.
//!
//! Mirrors Python's miner.py.  Reads `aimem.yaml` from the project root
//! to discover the wing name and room definitions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use walkdir::WalkDir;

use crate::{
    db::{AimemDb, DbError},
    embedder::{EmbedError, Embedder},
    types::Drawer,
};

#[derive(Debug, Error)]
pub enum MinerError {
    #[error("db: {0}")]
    Db(#[from] DbError),
    #[error("embed: {0}")]
    Embed(#[from] EmbedError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("yaml: {0}")]
    Yaml(#[from] serde_yaml::Error),
    #[error("config not found at {0}")]
    ConfigNotFound(String),
}

// ── Constants ────────────────────────────────────────────────────────────────

const CHUNK_SIZE: usize = 800; // chars per drawer
const CHUNK_OVERLAP: usize = 100; // overlap between adjacent chunks
pub const MIN_CHUNK: usize = 50; // skip tiny chunks

static READABLE_EXT: &[&str] = &[
    "txt",
    "md",
    "py",
    "js",
    "ts",
    "jsx",
    "tsx",
    "json",
    "yaml",
    "yml",
    "html",
    "css",
    "java",
    "go",
    "rs",
    "rb",
    "sh",
    "csv",
    "sql",
    "toml",
    "c",
    "cpp",
    "h",
    "hpp",
    "swift",
    "kt",
    "dart",
    "lua",
    "vim",
    "conf",
    "ini",
    "env",
    "Makefile",
    "Dockerfile",
];

static SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    "env",
    "dist",
    "build",
    ".next",
    "coverage",
    ".aimem",
    "target",
    ".cargo",
    ".idea",
    ".vscode",
];

static SKIP_FILES: &[&str] = &[
    "aimem.yaml",
    "aimem.yml",
    ".gitignore",
    "package-lock.json",
    "Cargo.lock",
];

// ── Project config (aimem.yaml) ─────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RoomDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub keywords: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectConfig {
    pub wing: String,
    #[serde(default = "default_rooms")]
    pub rooms: Vec<RoomDef>,
}

fn default_rooms() -> Vec<RoomDef> {
    vec![RoomDef {
        name: "general".to_string(),
        description: "All project files".to_string(),
        keywords: vec![],
    }]
}

impl ProjectConfig {
    pub fn load(project_dir: impl AsRef<Path>) -> Result<Self, MinerError> {
        let dir = project_dir.as_ref();
        let yaml = dir.join("aimem.yaml");
        let path = if yaml.exists() {
            yaml
        } else {
            return Err(MinerError::ConfigNotFound(dir.display().to_string()));
        };
        let raw = std::fs::read_to_string(&path)?;
        Ok(serde_yaml::from_str(&raw)?)
    }
}

// ── Mining stats ─────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct MineStats {
    pub files_scanned: usize,
    pub files_skipped: usize, // already mined
    pub drawers_added: usize,
    pub rooms: HashMap<String, usize>,
}

// ── Miner ────────────────────────────────────────────────────────────────────

/// Project file miner.
#[derive(Debug, Clone)]
pub struct Miner {
    db: AimemDb,
    embedder: Option<Embedder>,
}

impl Miner {
    /// Create a miner with optional embedding support.
    /// Pass `None` for `embedder` to store text only (no vector search).
    pub fn new(db: AimemDb, embedder: Option<Embedder>) -> Self {
        Self { db, embedder }
    }

    /// Mine a project directory into AiMem.
    ///
    /// # Arguments
    /// * `project_dir`   — path to the project root (must contain `aimem.yaml`)
    /// * `wing_override` — override the wing name from config
    /// * `agent`         — who is filing these drawers
    /// * `limit`         — max files to process (0 = all)
    /// * `dry_run`       — scan and report without writing
    pub async fn mine(
        &self,
        project_dir: impl AsRef<Path>,
        wing_override: Option<&str>,
        agent: &str,
        limit: usize,
        dry_run: bool,
    ) -> Result<MineStats, MinerError> {
        let project_dir = project_dir.as_ref().canonicalize()?;
        let cfg = ProjectConfig::load(&project_dir)?;
        let wing = wing_override.unwrap_or(&cfg.wing).to_string();

        let files = scan_files(&project_dir);
        let files: Vec<_> = if limit > 0 {
            files.into_iter().take(limit).collect()
        } else {
            files
        };

        let mut stats = MineStats::default();
        stats.files_scanned = files.len();

        for filepath in &files {
            let source = filepath.to_string_lossy().to_string();

            // Skip already-mined files
            if !dry_run && self.db.source_already_mined(&source).await? {
                stats.files_skipped += 1;
                continue;
            }

            let content = match std::fs::read_to_string(filepath) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let content = content.trim().to_string();
            if content.len() < MIN_CHUNK {
                continue;
            }

            let room = detect_room(filepath, &content, &cfg.rooms, &project_dir);
            let chunks = chunk_text(&content);

            if dry_run {
                tracing::info!(
                    "[DRY RUN] {} → room:{} ({} chunks)",
                    filepath.file_name().unwrap_or_default().to_string_lossy(),
                    room,
                    chunks.len()
                );
                stats.drawers_added += chunks.len();
                *stats.rooms.entry(room.clone()).or_default() += 1;
                continue;
            }

            // Optionally embed all chunks at once (batch is faster)
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
            let mut file_drawers = 0usize;
            for (i, (chunk, embedding)) in chunks.iter().zip(embeddings.iter()).enumerate() {
                let id = drawer_id(&wing, &room, &source, i);
                let drawer = Drawer {
                    id,
                    wing: wing.clone(),
                    room: room.clone(),
                    content: chunk.clone(),
                    parts: vec![],
                    source_file: Some(source.clone()),
                    chunk_index: i as i64,
                    added_by: agent.to_string(),
                    filed_at: now.clone(),
                };
                if self.db.insert_drawer(&drawer, embedding.as_deref()).await? {
                    file_drawers += 1;
                }
            }

            if file_drawers > 0 {
                stats.drawers_added += file_drawers;
                *stats.rooms.entry(room).or_default() += 1;
                tracing::debug!(
                    "✓ {} +{}",
                    filepath.file_name().unwrap_or_default().to_string_lossy(),
                    file_drawers
                );
            }
        }

        Ok(stats)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Scan a project directory for all readable files.
pub fn scan_files(project_dir: &Path) -> Vec<PathBuf> {
    WalkDir::new(project_dir)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !SKIP_DIRS.contains(&name.as_ref())
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            let name = e.file_name().to_string_lossy();
            if SKIP_FILES.contains(&name.as_ref()) {
                return false;
            }
            let ext = e
                .path()
                .extension()
                .map(|x| x.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            // Accept files with known extensions OR no extension (Makefile, Dockerfile…)
            READABLE_EXT.contains(&ext.as_str()) || ext.is_empty()
        })
        .map(|e| e.into_path())
        .collect()
}

/// Route a file to the best-matching room.
///
/// Priority:
/// 1. Directory path component matches a room name
/// 2. Filename stem matches a room name
/// 3. Content keyword scoring
/// 4. Fallback: "general"
pub fn detect_room(
    filepath: &Path,
    content: &str,
    rooms: &[RoomDef],
    project_root: &Path,
) -> String {
    let relative = filepath
        .strip_prefix(project_root)
        .unwrap_or(filepath)
        .to_string_lossy()
        .to_lowercase();
    let stem = filepath
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let content_lower = &content[..content.len().min(2000)].to_lowercase();
    let path_parts: Vec<&str> = relative.split('/').collect();

    // Priority 1: directory component
    for part in path_parts.iter().take(path_parts.len().saturating_sub(1)) {
        for room in rooms {
            let rn = room.name.to_lowercase();
            if rn == *part || part.contains(rn.as_str()) {
                return room.name.clone();
            }
        }
    }

    // Priority 2: filename stem
    for room in rooms {
        let rn = room.name.to_lowercase();
        if stem.contains(rn.as_str()) || rn.contains(stem.as_str()) {
            return room.name.clone();
        }
    }

    // Priority 3: keyword scoring
    let mut scores: Vec<(usize, &str)> = rooms
        .iter()
        .map(|r| {
            let mut score = 0usize;
            let all_kw: Vec<String> = r
                .keywords
                .iter()
                .chain(std::iter::once(&r.name))
                .cloned()
                .collect();
            for kw in &all_kw {
                let kw_lower = kw.to_lowercase();
                let mut pos = 0;
                while let Some(idx) = content_lower[pos..].find(kw_lower.as_str()) {
                    score += 1;
                    pos += idx + kw_lower.len();
                }
            }
            (score, r.name.as_str())
        })
        .collect();
    scores.sort_by(|a, b| b.0.cmp(&a.0));
    if let Some((score, name)) = scores.first() {
        if *score > 0 {
            return name.to_string();
        }
    }

    "general".to_string()
}

/// Split content into overlapping chunks, preferring paragraph boundaries.
pub fn chunk_text(content: &str) -> Vec<String> {
    let content = content.trim();
    if content.is_empty() {
        return vec![];
    }

    let mut chunks = Vec::new();
    let mut start = 0usize;

    while start < content.len() {
        let end = (start + CHUNK_SIZE).min(content.len());

        // Try to break at paragraph boundary
        let end = if end < content.len() {
            let slice = &content[start..end];
            if let Some(pos) = slice.rfind("\n\n") {
                if pos > CHUNK_SIZE / 2 {
                    start + pos
                } else if let Some(pos2) = slice.rfind('\n') {
                    if pos2 > CHUNK_SIZE / 2 {
                        start + pos2
                    } else {
                        end
                    }
                } else {
                    end
                }
            } else if let Some(pos) = slice.rfind('\n') {
                if pos > CHUNK_SIZE / 2 {
                    start + pos
                } else {
                    end
                }
            } else {
                end
            }
        } else {
            end
        };

        let chunk = content[start..end].trim().to_string();
        if chunk.len() >= MIN_CHUNK {
            chunks.push(chunk);
        }

        // Advance with overlap
        start = if end < content.len() {
            end.saturating_sub(CHUNK_OVERLAP)
        } else {
            end
        };
    }

    chunks
}

/// Deterministic drawer ID from wing/room/source/chunk_index.
fn drawer_id(wing: &str, room: &str, source_file: &str, chunk_index: usize) -> String {
    let input = format!("{wing}{room}{source_file}{chunk_index}");
    let digest = md5::compute(input.as_bytes());
    format!("drawer_{wing}_{room}_{digest:x}")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_text_basic() {
        let text = "hello world".repeat(200);
        let chunks = chunk_text(&text);
        assert!(!chunks.is_empty());
        for c in &chunks {
            assert!(c.len() <= CHUNK_SIZE + CHUNK_OVERLAP);
        }
    }

    #[test]
    fn test_detect_room_fallback() {
        let rooms = vec![RoomDef {
            name: "general".to_string(),
            description: "all".to_string(),
            keywords: vec![],
        }];
        let path = PathBuf::from("/proj/src/main.rs");
        let room = detect_room(&path, "fn main() {}", &rooms, Path::new("/proj"));
        assert_eq!(room, "general");
    }

    #[test]
    fn test_detect_room_by_keyword() {
        let rooms = vec![
            RoomDef {
                name: "backend".to_string(),
                description: "".to_string(),
                keywords: vec!["database".to_string(), "query".to_string()],
            },
            RoomDef {
                name: "general".to_string(),
                description: "".to_string(),
                keywords: vec![],
            },
        ];
        let path = PathBuf::from("/proj/main.rs");
        let room = detect_room(
            &path,
            "SELECT * FROM users; database query optimization",
            &rooms,
            Path::new("/proj"),
        );
        assert_eq!(room, "backend");
    }
}
