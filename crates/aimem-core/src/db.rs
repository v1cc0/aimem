//! PalaceDb — Turso connection wrapper + schema bootstrap.
//!
//! # Overview
//!
//! [`PalaceDb`] wraps a [`turso::Database`] and owns the full schema lifecycle.
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
//! use aimem_core::{Drawer, PalaceDb};
//! use chrono::Utc;
//!
//! # #[tokio::main] async fn main() -> anyhow::Result<()> {
//! // In-memory DB — perfect for tests
//! let db = PalaceDb::memory().await?;
//!
//! let drawer = Drawer {
//!     id: "d_001".into(),
//!     wing: "my_project".into(),
//!     room: "decisions".into(),
//!     content: "We chose Turso for native vector support.".into(),
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

use crate::types::{Drawer, Entity, Triple};

#[derive(Debug, Error)]
pub enum DbError {
    #[error("Turso error: {0}")]
    Turso(#[from] turso::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Type conversion: {0}")]
    Conversion(String),
}

pub type DbResult<T> = Result<T, DbError>;

/// SQL to initialize all tables.
const INIT_SQL: &str = "
CREATE TABLE IF NOT EXISTS drawers (
    id          TEXT PRIMARY KEY,
    wing        TEXT NOT NULL,
    room        TEXT NOT NULL,
    content     TEXT NOT NULL,
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
pub struct PalaceDb {
    db: Database,
}

impl std::fmt::Debug for PalaceDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PalaceDb").finish_non_exhaustive()
    }
}

impl PalaceDb {
    /// Open (or create) the palace DB at the given path and run schema migrations.
    pub async fn open(path: impl AsRef<Path>) -> DbResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let path_str = path.to_string_lossy();
        let db = Builder::new_local(path_str.as_ref()).build().await?;
        let palace = Self { db };
        palace.migrate().await?;
        Ok(palace)
    }

    /// Open an in-memory DB (for tests).
    pub async fn memory() -> DbResult<Self> {
        let db = Builder::new_local(":memory:").build().await?;
        let palace = Self { db };
        palace.migrate().await?;
        Ok(palace)
    }

    /// Acquire a connection from the database.
    pub fn conn(&self) -> DbResult<Connection> {
        Ok(self.db.connect()?)
    }

    /// Run schema bootstrap — idempotent (`CREATE TABLE IF NOT EXISTS`).
    async fn migrate(&self) -> DbResult<()> {
        let conn = self.conn()?;
        conn.execute_batch(INIT_SQL).await?;
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
        let conn = self.conn()?;

        // Serialize embedding as JSON array for vector32()
        let emb_json: Option<String> = embedding.map(|v| {
            let nums: Vec<String> = v.iter().map(|f| f.to_string()).collect();
            format!("[{}]", nums.join(","))
        });

        // Use vector32(?) to store embedding in Turso's native format
        let sql = if emb_json.is_some() {
            "INSERT OR IGNORE INTO drawers \
             (id, wing, room, content, embedding, source_file, chunk_index, added_by, filed_at) \
             VALUES (?1, ?2, ?3, ?4, vector32(?5), ?6, ?7, ?8, ?9)"
        } else {
            "INSERT OR IGNORE INTO drawers \
             (id, wing, room, content, embedding, source_file, chunk_index, added_by, filed_at) \
             VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8)"
        };

        let rows_affected = if let Some(ref emb) = emb_json {
            conn.execute(
                sql,
                turso::params![
                    drawer.id.as_str(),
                    drawer.wing.as_str(),
                    drawer.room.as_str(),
                    drawer.content.as_str(),
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
                "SELECT id, wing, room, content, source_file, chunk_index, added_by, filed_at \
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
                "SELECT id, wing, room, content, source_file, chunk_index, added_by, filed_at \
                 FROM drawers WHERE wing = ?1 AND room = ?2 \
                 ORDER BY filed_at DESC LIMIT ?3"
                    .to_string(),
                vec![w.to_string(), r.to_string(), limit.to_string()],
            ),
            (Some(w), None) => (
                "SELECT id, wing, room, content, source_file, chunk_index, added_by, filed_at \
                 FROM drawers WHERE wing = ?1 \
                 ORDER BY filed_at DESC LIMIT ?2"
                    .to_string(),
                vec![w.to_string(), limit.to_string()],
            ),
            (None, Some(r)) => (
                "SELECT id, wing, room, content, source_file, chunk_index, added_by, filed_at \
                 FROM drawers WHERE room = ?1 \
                 ORDER BY filed_at DESC LIMIT ?2"
                    .to_string(),
                vec![r.to_string(), limit.to_string()],
            ),
            (None, None) => (
                "SELECT id, wing, room, content, source_file, chunk_index, added_by, filed_at \
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
        source_file: {
            let s = val_to_string(row, 4)?;
            if s.is_empty() { None } else { Some(s) }
        },
        chunk_index: row.get_value(5)?.as_integer().copied().unwrap_or(0),
        added_by: val_to_string(row, 6)?,
        filed_at: val_to_string(row, 7)?,
    })
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
