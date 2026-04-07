//! Semantic search and FTS over the palace.

use std::collections::HashSet;

use crate::{
    db::{AimemDb, DbError},
    embedder::Embedder,
    types::{Drawer, SearchResult},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("db error: {0}")]
    Db(#[from] DbError),
    #[error("embed error: {0}")]
    Embed(#[from] crate::embedder::EmbedError),
    #[error("semantic search requires an embedder")]
    EmbedderUnavailable,
    #[error("turso error: {0}")]
    Turso(#[from] turso::Error),
}

/// Searcher — vector + keyword search over the palace.
#[derive(Debug, Clone)]
pub struct Searcher {
    db: AimemDb,
    embedder: Option<Embedder>,
}

impl Searcher {
    pub fn new(db: AimemDb, embedder: Embedder) -> Self {
        Self {
            db,
            embedder: Some(embedder),
        }
    }

    pub fn keyword_only(db: AimemDb) -> Self {
        Self { db, embedder: None }
    }

    /// Semantic vector search using Turso's `vector_distance_cos`.
    pub async fn vector_search(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let qvec = self
            .embedder
            .as_ref()
            .ok_or(SearchError::EmbedderUnavailable)?
            .embed_one(query)?;
        let qvec_json = Embedder::to_vector32_json(&qvec);

        let conn = self.db.conn()?;

        // Build dynamic SQL + gather rows using turso params! macro
        let sql = build_vector_sql(wing, room);
        let mut rows = match (wing, room) {
            (Some(w), Some(r)) => {
                conn.query(&sql, turso::params![qvec_json.as_str(), w, r, limit as i64])
                    .await?
            }
            (Some(w), None) => {
                conn.query(&sql, turso::params![qvec_json.as_str(), w, limit as i64])
                    .await?
            }
            (None, Some(r)) => {
                conn.query(&sql, turso::params![qvec_json.as_str(), r, limit as i64])
                    .await?
            }
            (None, None) => {
                conn.query(&sql, turso::params![qvec_json.as_str(), limit as i64])
                    .await?
            }
        };

        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            let drawer = row_to_drawer(&row);
            let dist = match row.get_value(8)? {
                turso::Value::Real(f) => f as f32,
                turso::Value::Integer(i) => i as f32,
                _ => 1.0,
            };
            let similarity = (1.0_f32 - dist).clamp(0.0, 1.0);
            results.push(SearchResult { drawer, similarity });
        }
        Ok(results)
    }

    /// Keyword search using SQL LIKE.
    pub async fn keyword_search(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Drawer>, SearchError> {
        let conn = self.db.conn()?;
        let like = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        let sql = build_keyword_sql(wing, room);

        let mut rows = match (wing, room) {
            (Some(w), Some(r)) => {
                conn.query(&sql, turso::params![like.as_str(), w, r, limit as i64])
                    .await?
            }
            (Some(w), None) => {
                conn.query(&sql, turso::params![like.as_str(), w, limit as i64])
                    .await?
            }
            (None, Some(r)) => {
                conn.query(&sql, turso::params![like.as_str(), r, limit as i64])
                    .await?
            }
            (None, None) => {
                conn.query(&sql, turso::params![like.as_str(), limit as i64])
                    .await?
            }
        };

        let mut drawers = Vec::new();
        while let Some(row) = rows.next().await? {
            drawers.push(row_to_drawer(&row));
        }
        Ok(drawers)
    }

    /// Keyword search with token fallback.
    pub async fn keyword_fallback_search(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Drawer>, SearchError> {
        let exact = self.keyword_search(query, wing, room, limit).await?;
        if !exact.is_empty() || limit == 0 {
            return Ok(exact);
        }

        let mut merged = Vec::new();
        let mut seen = HashSet::new();

        for token in query
            .split_whitespace()
            .map(|part| part.trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-'))
            .filter(|part| part.len() >= 2)
        {
            for drawer in self.keyword_search(token, wing, room, limit).await? {
                if seen.insert(drawer.id.clone()) {
                    merged.push(drawer);
                    if merged.len() >= limit {
                        return Ok(merged);
                    }
                }
            }
        }

        Ok(merged)
    }

    /// Duplicate check.
    pub async fn find_duplicates(
        &self,
        content: &str,
        threshold: f32,
        limit: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let results = self.vector_search(content, None, None, limit).await?;
        Ok(results
            .into_iter()
            .filter(|r| r.similarity >= threshold)
            .collect())
    }
}

// ── SQL builders ──────────────────────────────────────────────────────────────

fn build_vector_sql(wing: Option<&str>, room: Option<&str>) -> String {
    let filter = match (wing, room) {
        (Some(_), Some(_)) => "AND wing = ?2 AND room = ?3",
        (Some(_), None) => "AND wing = ?2",
        (None, Some(_)) => "AND room = ?2",
        (None, None) => "",
    };
    let limit_param = match (wing, room) {
        (Some(_), Some(_)) => "?4",
        (Some(_), None) | (None, Some(_)) => "?3",
        (None, None) => "?2",
    };
    format!(
        "SELECT id, wing, room, content, source_file, chunk_index, added_by, filed_at, \
         vector_distance_cos(embedding, vector32(?1)) AS dist \
         FROM drawers \
         WHERE embedding IS NOT NULL {filter} \
         ORDER BY dist LIMIT {limit_param}"
    )
}

fn build_keyword_sql(wing: Option<&str>, room: Option<&str>) -> String {
    let filter = match (wing, room) {
        (Some(_), Some(_)) => "AND wing = ?2 AND room = ?3",
        (Some(_), None) => "AND wing = ?2",
        (None, Some(_)) => "AND room = ?2",
        (None, None) => "",
    };
    let limit_param = match (wing, room) {
        (Some(_), Some(_)) => "?4",
        (Some(_), None) | (None, Some(_)) => "?3",
        (None, None) => "?2",
    };
    format!(
        "SELECT id, wing, room, content, source_file, chunk_index, added_by, filed_at \
         FROM drawers \
         WHERE content LIKE ?1 ESCAPE '\\' {filter} \
         ORDER BY filed_at DESC LIMIT {limit_param}"
    )
}

fn row_to_drawer(row: &turso::Row) -> Drawer {
    Drawer {
        id: val_str(row, 0),
        wing: val_str(row, 1),
        room: val_str(row, 2),
        content: val_str(row, 3),
        source_file: {
            let s = val_str(row, 4);
            if s.is_empty() { None } else { Some(s) }
        },
        chunk_index: row
            .get_value(5)
            .ok()
            .and_then(|v| v.as_integer().copied())
            .unwrap_or(0),
        added_by: val_str(row, 6),
        filed_at: val_str(row, 7),
    }
}

fn val_str(row: &turso::Row, idx: usize) -> String {
    match row.get_value(idx) {
        Ok(turso::Value::Text(s)) => s,
        Ok(turso::Value::Null) | Err(_) => String::new(),
        Ok(v) => format!("{v:?}"),
    }
}
