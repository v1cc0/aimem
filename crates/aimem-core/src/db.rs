//! AimemDb — Turso connection wrapper + schema bootstrap.
//!
//! # Overview
//!
//! [`AimemDb`] wraps a [`turso::Database`] and owns the full schema lifecycle.
//! It is cheaply cloneable (Arc-backed internally) and safe to share across
//! async tasks.
//!
//! ## Schema
//!
//! Three tables live in one file:
//!
//! | Table | Purpose |
//! |-------|---------|
//! | `drawers` | Verbatim text chunks + `vector32` embeddings |
//! | `entities` | Knowledge-graph entity nodes |
//! | `triples` | Temporal (subject→predicate→object) edges |
//!
//! ## Example
//!
//! ```rust,no_run
//! use aimem_core::{Drawer, AimemDb};
//! use chrono::Utc;
//!
//! # #[tokio::main] async fn main() -> anyhow::Result<()> {
//! // In-memory DB — perfect for tests
//! let db = AimemDb::memory().await?;
//!
//! let drawer = Drawer {
//!     id: "d_001".into(),
//!     wing: "my_project".into(),
//!     room: "decisions".into(),
//!     content: "We chose Turso for native vector support.".into(),
//!     parts: vec![],
//!     source_file: None,
//!     chunk_index: 0,
//!     added_by: "claude".into(),
//!     filed_at: Utc::now().to_rfc3339(),
//! };
//!
//! let inserted = db.insert_drawer(&drawer, None).await?;
//! assert!(inserted);
//! assert_eq!(db.drawer_count().await?, 1);
//! # Ok(())
//! # }
//! ```

use std::path::Path;

use thiserror::Error;
use turso::{Builder, Connection, Database};

use crate::embedder::{
    GEMINI_EMBED_MODEL, GEMINI_EMBED_PROVIDER, LOCAL_EMBED_MODEL, LOCAL_EMBED_PROVIDER,
};
use crate::types::{Drawer, Entity, Triple};

#[derive(Debug, Error)]
pub enum DbError {
    #[error("Turso error: {0}")]
    Turso(#[from] turso::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Type conversion: {0}")]
    Conversion(String),
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Embedding dimension mismatch: store uses {expected}, attempted {actual}")]
    EmbeddingDimensionMismatch { expected: usize, actual: usize },
    #[error(
        "Embedding model mismatch: store uses {expected_provider}/{expected_model}, attempted {actual_provider}/{actual_model}"
    )]
    EmbeddingModelMismatch {
        expected_provider: String,
        expected_model: String,
        actual_provider: String,
        actual_model: String,
    },
}

pub type DbResult<T> = Result<T, DbError>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmbeddingStoreProfile {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub dimension: Option<usize>,
}

/// SQL to initialize all tables.
const INIT_SQL: &str = "
CREATE TABLE IF NOT EXISTS drawers (
    id          TEXT PRIMARY KEY,
    wing        TEXT NOT NULL,
    room        TEXT NOT NULL,
    content     TEXT NOT NULL,
    parts       TEXT NOT NULL DEFAULT '[]',
    embedding   BLOB,
    source_file TEXT,
    chunk_index INTEGER NOT NULL DEFAULT 0,
    added_by    TEXT    NOT NULL DEFAULT 'aimem',
    filed_at    TEXT    NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_drawers_wing     ON drawers(wing);
CREATE INDEX IF NOT EXISTS idx_drawers_room     ON drawers(room);
CREATE INDEX IF NOT EXISTS idx_drawers_wing_room ON drawers(wing, room);
CREATE INDEX IF NOT EXISTS idx_drawers_source   ON drawers(source_file);

CREATE TABLE IF NOT EXISTS aimem_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS entities (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    type        TEXT NOT NULL DEFAULT 'unknown',
    properties  TEXT NOT NULL DEFAULT '{}',
    created_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS triples (
    id            TEXT PRIMARY KEY,
    subject       TEXT NOT NULL,
    predicate     TEXT NOT NULL,
    object        TEXT NOT NULL,
    valid_from    TEXT,
    valid_to      TEXT,
    confidence    REAL NOT NULL DEFAULT 1.0,
    source_closet TEXT,
    source_file   TEXT,
    extracted_at  TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (subject) REFERENCES entities(id),
    FOREIGN KEY (object)  REFERENCES entities(id)
);

CREATE INDEX IF NOT EXISTS idx_triples_subject   ON triples(subject);
CREATE INDEX IF NOT EXISTS idx_triples_object    ON triples(object);
CREATE INDEX IF NOT EXISTS idx_triples_predicate ON triples(predicate);
CREATE INDEX IF NOT EXISTS idx_triples_valid     ON triples(valid_from, valid_to);
";

/// Shared Turso database handle.
///
/// Clone is cheap — the underlying `Database` is `Arc`-wrapped.
#[derive(Clone)]
pub struct AimemDb {
    db: Database,
}

impl std::fmt::Debug for AimemDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AimemDb").finish_non_exhaustive()
    }
}

impl AimemDb {
    /// Open (or create) the AiMem DB at the given path and run schema migrations.
    pub async fn open(path: impl AsRef<Path>) -> DbResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let path_str = path.to_string_lossy();
        let db = Builder::new_local(path_str.as_ref()).build().await?;
        let aimem_db = Self { db };
        aimem_db.migrate().await?;
        Ok(aimem_db)
    }

    /// Open an in-memory DB (for tests).
    pub async fn memory() -> DbResult<Self> {
        let db = Builder::new_local(":memory:").build().await?;
        let aimem_db = Self { db };
        aimem_db.migrate().await?;
        Ok(aimem_db)
    }

    /// Acquire a connection from the database.
    pub fn conn(&self) -> DbResult<Connection> {
        Ok(self.db.connect()?)
    }

    /// Run schema bootstrap — idempotent (`CREATE TABLE IF NOT EXISTS`).
    async fn migrate(&self) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute_batch(INIT_SQL).await?;

        if let Err(err) = conn
            .execute(
                "ALTER TABLE drawers ADD COLUMN parts TEXT NOT NULL DEFAULT '[]'",
                (),
            )
            .await
        {
            let msg = err.to_string();
            if !msg.contains("duplicate column name: parts") {
                return Err(err.into());
            }
        }

        Ok(())
    }

    async fn meta_value(&self, key: &str) -> DbResult<Option<String>> {
        let conn = self.conn()?;
        let mut rows = conn
            .query("SELECT value FROM aimem_meta WHERE key = ?1 LIMIT 1", [key])
            .await?;
        let Some(row) = rows.next().await? else {
            return Ok(None);
        };
        Ok(Some(val_to_string(&row, 0)?))
    }

    async fn set_meta_value(&self, key: &str, value: &str) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO aimem_meta (key, value) VALUES (?1, ?2)",
            [key, value],
        )
        .await?;
        Ok(())
    }

    fn infer_profile_from_dimension(dim: usize) -> Option<(&'static str, &'static str)> {
        match dim {
            384 => Some((LOCAL_EMBED_PROVIDER, LOCAL_EMBED_MODEL)),
            768 => Some((GEMINI_EMBED_PROVIDER, GEMINI_EMBED_MODEL)),
            _ => None,
        }
    }

    /// Return the configured embedding dimension, inferring it from existing
    /// stored vectors when upgrading older databases that predate metadata.
    pub async fn embedding_dimension(&self) -> DbResult<Option<usize>> {
        if let Some(raw) = self.meta_value("embedding_dim").await? {
            let dim = raw.parse::<usize>().map_err(|err| {
                DbError::Conversion(format!("invalid embedding_dim metadata {raw:?}: {err}"))
            })?;
            return Ok(Some(dim));
        }

        let conn = self.conn()?;

        let mut rows = conn
            .query(
                "SELECT length(embedding) FROM drawers WHERE embedding IS NOT NULL LIMIT 1",
                (),
            )
            .await?;
        let Some(row) = rows.next().await? else {
            return Ok(None);
        };

        let bytes = row.get_value(0)?.as_integer().copied().ok_or_else(|| {
            DbError::Conversion("length(embedding) did not return an integer".to_string())
        })?;
        if bytes <= 0 || bytes % 4 != 0 {
            return Err(DbError::Conversion(format!(
                "unexpected embedding blob length {bytes}; cannot infer dimension"
            )));
        }

        let dim = (bytes / 4) as usize;
        self.set_meta_value("embedding_dim", &dim.to_string())
            .await?;
        Ok(Some(dim))
    }

    pub async fn embedding_profile(&self) -> DbResult<EmbeddingStoreProfile> {
        let dimension = self.embedding_dimension().await?;
        let mut provider = self.meta_value("embedding_provider").await?;
        let mut model = self.meta_value("embedding_model").await?;

        if let Some(dim) = dimension {
            if let Some((inferred_provider, inferred_model)) =
                Self::infer_profile_from_dimension(dim)
            {
                if provider.is_none() {
                    self.set_meta_value("embedding_provider", inferred_provider)
                        .await?;
                    provider = Some(inferred_provider.to_string());
                }
                if model.is_none() {
                    self.set_meta_value("embedding_model", inferred_model)
                        .await?;
                    model = Some(inferred_model.to_string());
                }
            }
        }

        Ok(EmbeddingStoreProfile {
            provider,
            model,
            dimension,
        })
    }

    /// Validate that a query vector matches the store dimension when known.
    pub async fn assert_embedding_dimension(&self, dim: usize) -> DbResult<()> {
        if let Some(expected) = self.embedding_dimension().await? {
            if expected != dim {
                return Err(DbError::EmbeddingDimensionMismatch {
                    expected,
                    actual: dim,
                });
            }
        }
        Ok(())
    }

    pub async fn assert_embedding_profile(
        &self,
        dim: usize,
        provider: &str,
        model: &str,
    ) -> DbResult<()> {
        let profile = self.embedding_profile().await?;
        if let Some(expected) = profile.dimension {
            if expected != dim {
                return Err(DbError::EmbeddingDimensionMismatch {
                    expected,
                    actual: dim,
                });
            }
        }

        if let (Some(expected_provider), Some(expected_model)) = (profile.provider, profile.model) {
            if expected_provider != provider || expected_model != model {
                return Err(DbError::EmbeddingModelMismatch {
                    expected_provider,
                    expected_model,
                    actual_provider: provider.to_string(),
                    actual_model: model.to_string(),
                });
            }
        }

        Ok(())
    }

    /// Validate or initialize the store dimension before writing an embedding.
    async fn ensure_embedding_dimension_for_write(&self, dim: usize) -> DbResult<()> {
        let conn = self.conn()?;
        if let Some(expected) = self.embedding_dimension().await? {
            if expected != dim {
                return Err(DbError::EmbeddingDimensionMismatch {
                    expected,
                    actual: dim,
                });
            }
            return Ok(());
        }

        conn.execute(
            "INSERT OR REPLACE INTO aimem_meta (key, value) VALUES ('embedding_dim', ?1)",
            [dim.to_string()],
        )
        .await?;
        Ok(())
    }

    async fn ensure_embedding_profile_for_write(
        &self,
        dim: usize,
        provider: &str,
        model: &str,
    ) -> DbResult<()> {
        let profile = self.embedding_profile().await?;
        if let Some(expected) = profile.dimension {
            if expected != dim {
                return Err(DbError::EmbeddingDimensionMismatch {
                    expected,
                    actual: dim,
                });
            }
        }

        if let (Some(expected_provider), Some(expected_model)) =
            (profile.provider.clone(), profile.model.clone())
        {
            if expected_provider != provider || expected_model != model {
                return Err(DbError::EmbeddingModelMismatch {
                    expected_provider,
                    expected_model,
                    actual_provider: provider.to_string(),
                    actual_model: model.to_string(),
                });
            }
        }

        self.set_meta_value("embedding_dim", &dim.to_string())
            .await?;
        self.set_meta_value("embedding_provider", provider).await?;
        self.set_meta_value("embedding_model", model).await?;
        Ok(())
    }

    // ──────────────────────────────────────────────────────────────────────
    // Drawer operations
    // ──────────────────────────────────────────────────────────────────────

    /// Insert a drawer.  Returns `false` if the ID already exists (no-op).
    pub async fn insert_drawer(
        &self,
        drawer: &Drawer,
        embedding: Option<&[f32]>,
    ) -> DbResult<bool> {
        if let Some(v) = embedding {
            if let Some((provider, model)) = Self::infer_profile_from_dimension(v.len()) {
                self.ensure_embedding_profile_for_write(v.len(), provider, model)
                    .await?;
            } else {
                self.ensure_embedding_dimension_for_write(v.len()).await?;
            }
        }

        self.insert_drawer_inner(drawer, embedding).await
    }

    /// Insert a drawer and validate the embedding profile against store metadata.
    pub async fn insert_drawer_with_profile(
        &self,
        drawer: &Drawer,
        embedding: Option<&[f32]>,
        provider: &str,
        model: &str,
    ) -> DbResult<bool> {
        if let Some(v) = embedding {
            self.ensure_embedding_profile_for_write(v.len(), provider, model)
                .await?;
        }

        self.insert_drawer_inner(drawer, embedding).await
    }

    async fn insert_drawer_inner(
        &self,
        drawer: &Drawer,
        embedding: Option<&[f32]>,
    ) -> DbResult<bool> {
        let conn = self.conn()?;

        // Serialize embedding as JSON array for vector32()
        let emb_json: Option<String> = embedding.map(|v| {
            let nums: Vec<String> = v.iter().map(|f| f.to_string()).collect();
            format!("[{}]", nums.join(","))
        });
        let parts_json = serde_json::to_string(&drawer.parts)?;

        // Use vector32(?) to store embedding in Turso's native format
        let sql = if emb_json.is_some() {
            "INSERT OR IGNORE INTO drawers \
             (id, wing, room, content, parts, embedding, source_file, chunk_index, added_by, filed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, vector32(?6), ?7, ?8, ?9, ?10)"
        } else {
            "INSERT OR IGNORE INTO drawers \
             (id, wing, room, content, parts, embedding, source_file, chunk_index, added_by, filed_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8, ?9)"
        };

        let rows_affected = if let Some(ref emb) = emb_json {
            conn.execute(
                sql,
                turso::params![
                    drawer.id.as_str(),
                    drawer.wing.as_str(),
                    drawer.room.as_str(),
                    drawer.content.as_str(),
                    parts_json.as_str(),
                    emb.as_str(),
                    drawer.source_file.as_deref(),
                    drawer.chunk_index,
                    drawer.added_by.as_str(),
                    drawer.filed_at.as_str(),
                ],
            )
            .await?
        } else {
            conn.execute(
                sql,
                turso::params![
                    drawer.id.as_str(),
                    drawer.wing.as_str(),
                    drawer.room.as_str(),
                    drawer.content.as_str(),
                    parts_json.as_str(),
                    drawer.source_file.as_deref(),
                    drawer.chunk_index,
                    drawer.added_by.as_str(),
                    drawer.filed_at.as_str(),
                ],
            )
            .await?
        };

        Ok(rows_affected > 0)
    }

    /// Check if a source file has already been mined.
    pub async fn source_already_mined(&self, source_file: &str) -> DbResult<bool> {
        let conn = self.conn()?;
        let mut rows = conn
            .query(
                "SELECT 1 FROM drawers WHERE source_file = ?1 LIMIT 1",
                [source_file],
            )
            .await?;
        Ok(rows.next().await?.is_some())
    }

    /// Delete a drawer by ID.
    pub async fn delete_drawer(&self, id: &str) -> DbResult<bool> {
        let conn = self.conn()?;
        let n = conn
            .execute("DELETE FROM drawers WHERE id = ?1", [id])
            .await?;
        Ok(n > 0)
    }

    /// Find drawers whose content exactly matches the provided text.
    pub async fn find_drawers_by_exact_content(
        &self,
        content: &str,
        limit: usize,
    ) -> DbResult<Vec<Drawer>> {
        let conn = self.conn()?;
        let mut rows = conn
            .query(
                "SELECT id, wing, room, content, parts, source_file, chunk_index, added_by, filed_at \
                 FROM drawers WHERE content = ?1 ORDER BY filed_at DESC LIMIT ?2",
                turso::params![content, limit as i64],
            )
            .await?;

        let mut drawers = Vec::new();
        while let Some(row) = rows.next().await? {
            drawers.push(row_to_drawer(&row)?);
        }
        Ok(drawers)
    }

    /// Total drawer count.
    pub async fn drawer_count(&self) -> DbResult<i64> {
        let conn = self.conn()?;
        let mut rows = conn.query("SELECT COUNT(*) FROM drawers", ()).await?;
        let row = rows
            .next()
            .await?
            .ok_or_else(|| DbError::Conversion("COUNT returned no row".to_string()))?;
        Ok(row.get_value(0)?.as_integer().copied().unwrap_or(0))
    }

    /// Return wing → count and room → count aggregates.
    pub async fn taxonomy(&self) -> DbResult<(Vec<(String, i64)>, Vec<(String, i64)>)> {
        let conn = self.conn()?;

        let mut wing_rows = conn
            .query(
                "SELECT wing, COUNT(*) as cnt FROM drawers GROUP BY wing ORDER BY cnt DESC",
                (),
            )
            .await?;
        let mut wings: Vec<(String, i64)> = Vec::new();
        while let Some(row) = wing_rows.next().await? {
            let wing = match row.get_value(0)? {
                turso::Value::Text(s) => s,
                turso::Value::Null => String::new(),
                v => format!("{v:?}"),
            };
            let cnt = row.get_value(1)?.as_integer().copied().unwrap_or(0);
            wings.push((wing, cnt));
        }

        let mut room_rows = conn
            .query(
                "SELECT room, COUNT(*) as cnt FROM drawers GROUP BY room ORDER BY cnt DESC",
                (),
            )
            .await?;
        let mut rooms: Vec<(String, i64)> = Vec::new();
        while let Some(row) = room_rows.next().await? {
            let room = match row.get_value(0)? {
                turso::Value::Text(s) => s,
                turso::Value::Null => String::new(),
                v => format!("{v:?}"),
            };
            let cnt = row.get_value(1)?.as_integer().copied().unwrap_or(0);
            rooms.push((room, cnt));
        }

        Ok((wings, rooms))
    }

    /// Fetch all drawers in a wing (optionally filtered by room), ordered by filing time desc.
    pub async fn fetch_drawers(
        &self,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> DbResult<Vec<Drawer>> {
        let conn = self.conn()?;
        let limit = limit as i64;

        let (sql, params_vec): (String, Vec<String>) = match (wing, room) {
            (Some(w), Some(r)) => (
                "SELECT id, wing, room, content, parts, source_file, chunk_index, added_by, filed_at \
                 FROM drawers WHERE wing = ?1 AND room = ?2 \
                 ORDER BY filed_at DESC LIMIT ?3"
                    .to_string(),
                vec![w.to_string(), r.to_string(), limit.to_string()],
            ),
            (Some(w), None) => (
                "SELECT id, wing, room, content, parts, source_file, chunk_index, added_by, filed_at \
                 FROM drawers WHERE wing = ?1 \
                 ORDER BY filed_at DESC LIMIT ?2"
                    .to_string(),
                vec![w.to_string(), limit.to_string()],
            ),
            (None, Some(r)) => (
                "SELECT id, wing, room, content, parts, source_file, chunk_index, added_by, filed_at \
                 FROM drawers WHERE room = ?1 \
                 ORDER BY filed_at DESC LIMIT ?2"
                    .to_string(),
                vec![r.to_string(), limit.to_string()],
            ),
            (None, None) => (
                "SELECT id, wing, room, content, parts, source_file, chunk_index, added_by, filed_at \
                 FROM drawers ORDER BY filed_at DESC LIMIT ?1"
                    .to_string(),
                vec![limit.to_string()],
            ),
        };

        let params: Vec<&str> = params_vec.iter().map(|s| s.as_str()).collect();
        let mut rows = match params.len() {
            1 => conn.query(&sql, [params[0]]).await?,
            2 => conn.query(&sql, [params[0], params[1]]).await?,
            3 => conn.query(&sql, [params[0], params[1], params[2]]).await?,
            _ => unreachable!(),
        };

        let mut drawers = Vec::new();
        while let Some(row) = rows.next().await? {
            drawers.push(row_to_drawer(&row)?);
        }
        Ok(drawers)
    }

    // ──────────────────────────────────────────────────────────────────────
    // Entity / Triple operations (delegated to knowledge module)
    // ──────────────────────────────────────────────────────────────────────

    /// Insert or replace an entity.
    pub async fn upsert_entity(&self, entity: &Entity) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO entities (id, name, type, properties, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            turso::params![
                entity.id.as_str(),
                entity.name.as_str(),
                entity.entity_type.as_str(),
                entity.properties.as_str(),
                entity.created_at.as_str(),
            ],
        )
        .await?;
        Ok(())
    }

    /// Insert a triple (ignore if already exists).
    pub async fn insert_triple(&self, triple: &Triple) -> DbResult<bool> {
        let conn = self.conn()?;
        let n = conn
            .execute(
                "INSERT OR IGNORE INTO triples \
                 (id, subject, predicate, object, valid_from, valid_to, confidence, \
                  source_closet, source_file, extracted_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                turso::params![
                    triple.id.as_str(),
                    triple.subject.as_str(),
                    triple.predicate.as_str(),
                    triple.object.as_str(),
                    triple.valid_from.as_deref(),
                    triple.valid_to.as_deref(),
                    triple.confidence,
                    triple.source_closet.as_deref(),
                    triple.source_file.as_deref(),
                    triple.extracted_at.as_str(),
                ],
            )
            .await?;
        Ok(n > 0)
    }

    /// Mark a triple as expired (set valid_to).
    pub async fn invalidate_triple(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        ended: &str,
    ) -> DbResult<u64> {
        let conn = self.conn()?;
        let n = conn
            .execute(
                "UPDATE triples SET valid_to = ?1 \
                 WHERE subject = ?2 AND predicate = ?3 AND object = ?4 AND valid_to IS NULL",
                turso::params![ended, subject, predicate, object],
            )
            .await?;
        Ok(n)
    }
}

// ──────────────────────────────────────────────────────────────────────────
// Row helpers
// ──────────────────────────────────────────────────────────────────────────

fn row_to_drawer(row: &turso::Row) -> DbResult<Drawer> {
    Ok(Drawer {
        id: val_to_string(row, 0)?,
        wing: val_to_string(row, 1)?,
        room: val_to_string(row, 2)?,
        content: val_to_string(row, 3)?,
        parts: parse_parts(&val_to_string(row, 4)?)?,
        source_file: {
            let s = val_to_string(row, 5)?;
            if s.is_empty() { None } else { Some(s) }
        },
        chunk_index: row.get_value(6)?.as_integer().copied().unwrap_or(0),
        added_by: val_to_string(row, 7)?,
        filed_at: val_to_string(row, 8)?,
    })
}

fn parse_parts(raw: &str) -> Result<Vec<crate::types::ContentPart>, serde_json::Error> {
    if raw.is_empty() || raw == "[]" {
        Ok(Vec::new())
    } else {
        serde_json::from_str(raw)
    }
}

fn val_to_string(row: &turso::Row, idx: usize) -> DbResult<String> {
    match row.get_value(idx)? {
        turso::Value::Text(s) => Ok(s),
        turso::Value::Null => Ok(String::new()),
        turso::Value::Integer(i) => Ok(i.to_string()),
        turso::Value::Real(f) => Ok(f.to_string()),
        turso::Value::Blob(_) => Ok(String::new()),
    }
}
