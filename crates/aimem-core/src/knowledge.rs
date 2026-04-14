//! Knowledge graph — temporal entity-relationship graph.
//!
//! Mirrors Python's knowledge_graph.py.  All stored in Turso (triples + entities tables).
//!
//! Usage:
//! ```rust,no_run
//! use aimem_core::{AimemDb, KnowledgeGraph};
//! # async fn run() -> anyhow::Result<()> {
//! let db = AimemDb::memory().await?;
//! let kg = KnowledgeGraph::new(db);
//! kg.add_triple("Max", "child_of", "Alice", Some("2015-04-01"), None).await?;
//! let facts = kg.query_entity("Max", None, "outgoing").await?;
//! # Ok(())
//! # }
//! ```

use chrono::Local;

use crate::{
    db::{AimemDb, DbError},
    types::{Entity, Triple},
};

#[derive(Debug, Clone)]
pub struct KnowledgeGraph {
    db: AimemDb,
}

impl KnowledgeGraph {
    pub fn new(db: AimemDb) -> Self {
        Self { db }
    }

    // ── Entities ─────────────────────────────────────────────────────────────

    /// Add or update an entity node.
    pub async fn add_entity(
        &self,
        name: &str,
        entity_type: &str,
        properties: Option<serde_json::Value>,
    ) -> Result<String, DbError> {
        let id = entity_id(name);
        let props = properties
            .map(|p| serde_json::to_string(&p).unwrap_or_else(|_| "{}".to_string()))
            .unwrap_or_else(|| "{}".to_string());
        let entity = Entity {
            id: id.clone(),
            name: name.to_string(),
            entity_type: entity_type.to_string(),
            properties: props,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.db.upsert_entity(&entity).await?;
        Ok(id)
    }

    // ── Triples ───────────────────────────────────────────────────────────────

    /// Add a triple (subject → predicate → object).
    ///
    /// Auto-creates entity nodes if they don't exist.
    /// Returns the triple ID, or the existing ID if an identical active triple exists.
    pub async fn add_triple(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        valid_from: Option<&str>,
        valid_to: Option<&str>,
    ) -> Result<String, DbError> {
        // Ensure entities exist
        self.add_entity(subject, "unknown", None).await?;
        self.add_entity(object, "unknown", None).await?;

        let sub_id = entity_id(subject);
        let obj_id = entity_id(object);
        let pred = normalize_predicate(predicate);

        // Dedup: if an identical active triple exists, return its ID
        let conn = self.db.read_conn()?;
        let mut existing = conn
            .query(
                "SELECT id FROM triples \
                 WHERE subject = ?1 AND predicate = ?2 AND object = ?3 AND valid_to IS NULL",
                [sub_id.as_str(), pred.as_str(), obj_id.as_str()],
            )
            .await?;
        if let Some(row) = existing.next().await? {
            if let Ok(turso::Value::Text(id)) = row.get_value(0) {
                return Ok(id);
            }
        }

        let now = chrono::Utc::now().to_rfc3339();
        let triple_id = {
            let input = format!("{sub_id}{pred}{obj_id}{now}");
            let digest = md5::compute(input.as_bytes());
            format!("t_{sub_id}_{pred}_{obj_id}_{digest:x}")
        };

        let triple = Triple {
            id: triple_id.clone(),
            subject: sub_id,
            predicate: pred,
            object: obj_id,
            valid_from: valid_from.map(str::to_string),
            valid_to: valid_to.map(str::to_string),
            confidence: 1.0,
            source_closet: None,
            source_file: None,
            extracted_at: now,
            current: true,
        };
        self.db.insert_triple(&triple).await?;
        Ok(triple_id)
    }

    /// Mark a triple as no longer valid.
    pub async fn invalidate(
        &self,
        subject: &str,
        predicate: &str,
        object: &str,
        ended: Option<&str>,
    ) -> Result<u64, DbError> {
        let sub_id = entity_id(subject);
        let obj_id = entity_id(object);
        let pred = normalize_predicate(predicate);
        let ended = ended
            .map(str::to_string)
            .unwrap_or_else(|| Local::now().date_naive().to_string());
        self.db
            .invalidate_triple(&sub_id, &pred, &obj_id, &ended)
            .await
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    /// Get all relationships for an entity.
    ///
    /// * `direction` — `"outgoing"` (entity→?), `"incoming"` (?→entity), `"both"`
    /// * `as_of`     — only facts valid at this date (YYYY-MM-DD)
    pub async fn query_entity(
        &self,
        name: &str,
        as_of: Option<&str>,
        direction: &str,
    ) -> Result<Vec<Triple>, DbError> {
        let eid = entity_id(name);
        let conn = self.db.read_conn()?;
        let mut results = Vec::new();

        if direction == "outgoing" || direction == "both" {
            let sql = build_entity_query("subject", as_of);
            let mut rows = conn.query(&sql, [eid.as_str()]).await?;
            while let Some(row) = rows.next().await? {
                results.push(row_to_triple(&row)?);
            }
        }

        if direction == "incoming" || direction == "both" {
            let sql = build_entity_query("object", as_of);
            let mut rows = conn.query(&sql, [eid.as_str()]).await?;
            while let Some(row) = rows.next().await? {
                results.push(row_to_triple(&row)?);
            }
        }

        Ok(results)
    }

    /// Chronological timeline of facts (optionally filtered by entity).
    pub async fn timeline(&self, entity: Option<&str>) -> Result<Vec<Triple>, DbError> {
        let conn = self.db.read_conn()?;
        let mut results = Vec::new();

        if let Some(name) = entity {
            let eid = entity_id(name);
            let mut rows = conn
                .query(
                    "SELECT id, subject, predicate, object, valid_from, valid_to, \
                     confidence, source_closet, source_file, extracted_at \
                     FROM triples \
                     WHERE subject = ?1 OR object = ?1 \
                     ORDER BY valid_from ASC LIMIT 100",
                    [eid.as_str()],
                )
                .await?;
            while let Some(row) = rows.next().await? {
                results.push(row_to_triple(&row)?);
            }
        } else {
            let mut rows = conn
                .query(
                    "SELECT id, subject, predicate, object, valid_from, valid_to, \
                     confidence, source_closet, source_file, extracted_at \
                     FROM triples \
                     ORDER BY valid_from ASC LIMIT 100",
                    (),
                )
                .await?;
            while let Some(row) = rows.next().await? {
                results.push(row_to_triple(&row)?);
            }
        }

        Ok(results)
    }

    /// Knowledge graph statistics.
    pub async fn stats(&self) -> Result<serde_json::Value, DbError> {
        let conn = self.db.read_conn()?;

        let entities = query_count(&conn, "SELECT COUNT(*) FROM entities").await?;
        let triples = query_count(&conn, "SELECT COUNT(*) FROM triples").await?;
        let current =
            query_count(&conn, "SELECT COUNT(*) FROM triples WHERE valid_to IS NULL").await?;

        let mut pred_rows = conn
            .query(
                "SELECT DISTINCT predicate FROM triples ORDER BY predicate",
                (),
            )
            .await?;
        let mut predicates = Vec::new();
        while let Some(row) = pred_rows.next().await? {
            if let Ok(turso::Value::Text(p)) = row.get_value(0) {
                predicates.push(p);
            }
        }

        Ok(serde_json::json!({
            "entities": entities,
            "triples": triples,
            "current_facts": current,
            "expired_facts": triples - current,
            "relationship_types": predicates,
        }))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn entity_id(name: &str) -> String {
    name.to_lowercase().replace(' ', "_").replace('\'', "")
}

fn normalize_predicate(pred: &str) -> String {
    pred.to_lowercase().replace(' ', "_")
}

fn build_entity_query(col: &str, as_of: Option<&str>) -> String {
    let mut q = format!(
        "SELECT id, subject, predicate, object, valid_from, valid_to, \
         confidence, source_closet, source_file, extracted_at \
         FROM triples WHERE {col} = ?1"
    );
    if let Some(date) = as_of {
        q.push_str(&format!(
            " AND (valid_from IS NULL OR valid_from <= '{date}') \
             AND (valid_to IS NULL OR valid_to >= '{date}')"
        ));
    }
    q
}

fn row_to_triple(row: &turso::Row) -> Result<Triple, DbError> {
    let valid_to_str = val_opt_str(row, 5);
    Ok(Triple {
        id: val_str(row, 0),
        subject: val_str(row, 1),
        predicate: val_str(row, 2),
        object: val_str(row, 3),
        valid_from: val_opt_str(row, 4),
        valid_to: valid_to_str.clone(),
        confidence: match row.get_value(6)? {
            turso::Value::Real(f) => f,
            turso::Value::Integer(i) => i as f64,
            _ => 1.0,
        },
        source_closet: val_opt_str(row, 7),
        source_file: val_opt_str(row, 8),
        extracted_at: val_str(row, 9),
        current: valid_to_str.is_none(),
    })
}

fn val_str(row: &turso::Row, idx: usize) -> String {
    match row.get_value(idx) {
        Ok(turso::Value::Text(s)) => s,
        Ok(turso::Value::Null) | Err(_) => String::new(),
        Ok(v) => format!("{v:?}"),
    }
}

fn val_opt_str(row: &turso::Row, idx: usize) -> Option<String> {
    match row.get_value(idx) {
        Ok(turso::Value::Text(s)) if !s.is_empty() => Some(s),
        _ => None,
    }
}

async fn query_count(conn: &turso::Connection, sql: &str) -> Result<i64, DbError> {
    let mut rows = conn.query(sql, ()).await?;
    let row = rows.next().await?;
    Ok(row
        .and_then(|r| r.get_value(0).ok())
        .and_then(|v| v.as_integer().copied())
        .unwrap_or(0))
}
