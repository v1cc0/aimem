//! Shared data types.

use serde::{Deserialize, Serialize};

/// A drawer — one verbatim text chunk stored in the palace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Drawer {
    pub id: String,
    pub wing: String,
    pub room: String,
    pub content: String,
    pub source_file: Option<String>,
    pub chunk_index: i64,
    pub added_by: String,
    pub filed_at: String,
}

/// Lightweight metadata-only view of a drawer (no content).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawerMeta {
    pub id: String,
    pub wing: String,
    pub room: String,
    pub source_file: Option<String>,
    pub chunk_index: i64,
    pub filed_at: String,
}

/// A semantic / FTS search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub drawer: Drawer,
    /// Cosine similarity (0..=1, higher = more similar).
    pub similarity: f32,
}

/// A knowledge-graph entity node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub name: String,
    pub entity_type: String,
    /// Arbitrary properties stored as a JSON string.
    pub properties: String,
    pub created_at: String,
}

/// A temporal knowledge-graph triple: subject → predicate → object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Triple {
    pub id: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub valid_from: Option<String>,
    pub valid_to: Option<String>,
    pub confidence: f64,
    pub source_closet: Option<String>,
    pub source_file: Option<String>,
    pub extracted_at: String,
    /// Convenience flag: `valid_to` is None.
    pub current: bool,
}

/// A room-level graph node for palace graph traversal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomNode {
    pub room: String,
    pub wings: Vec<String>,
    pub drawer_count: i64,
}

/// An edge in the palace graph — a room that spans multiple wings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tunnel {
    pub room: String,
    pub wings: Vec<String>,
    pub drawer_count: i64,
}
