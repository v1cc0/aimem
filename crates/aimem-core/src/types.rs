//! Shared data types.

use serde::{Deserialize, Serialize};

/// A single piece of a multimodal drawer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text {
        text: String,
    },
    Image {
        /// Optional URI for reference (e.g. local path or remote URL).
        uri: Option<String>,
        mime: String,
        /// Raw bytes. This is NOT serialized to the DB to keep JSON small.
        #[serde(skip)]
        data: Option<Vec<u8>>,
    },
    Audio {
        uri: Option<String>,
        mime: String,
        #[serde(skip)]
        data: Option<Vec<u8>>,
    },
    Video {
        uri: Option<String>,
        mime: String,
        #[serde(skip)]
        data: Option<Vec<u8>>,
    },
}

impl ContentPart {
    pub fn text(t: impl Into<String>) -> Self {
        Self::Text { text: t.into() }
    }

    pub fn image(mime: impl Into<String>, data: Vec<u8>) -> Self {
        Self::Image {
            uri: None,
            mime: mime.into(),
            data: Some(data),
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }
}

/// A drawer — one verbatim chunk (text or multimodal) stored in AiMem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Drawer {
    pub id: String,
    pub wing: String,
    pub room: String,
    /// Primary text content (fallback for text-only clients).
    pub content: String,
    /// Multimodal parts (optional).
    #[serde(default)]
    pub parts: Vec<ContentPart>,
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

/// A room-level graph node for AiMem graph traversal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomNode {
    pub room: String,
    pub wings: Vec<String>,
    pub drawer_count: i64,
}

/// An edge in the AiMem graph — a room that spans multiple wings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tunnel {
    pub room: String,
    pub wings: Vec<String>,
    pub drawer_count: i64,
}
