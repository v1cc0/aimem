//! # aimem-core
//!
//! **Give your AI a memory — reusable core library.**
//!
//! `aimem-core` is a pure-Rust library that stores, retrieves, and searches
//! text memories using [Turso](https://github.com/tursodatabase/turso) (an in-process
//! SQLite engine with native vector support) as the sole storage backend.
//!
//! ## Feature overview
//!
//! | Feature | Implementation |
//! |---------|---------------|
//! | Vector storage | Turso `vector32()` BLOB column |
//! | Semantic search | `vector_distance_cos()` — SimSIMD-accelerated cosine |
//! | Full-text search | SQL `LIKE` scan (Turso FTS index optional) |
//! | Embedding generation | Gemini 2.0 (Remote) or `fastembed` (Local) |
//! | Knowledge graph | Temporal entity-relationship triples in SQLite |
//! | Graph traversal | BFS over wing/room adjacency — no graph DB needed |
//!
//! ## Architecture: 4-Layer Memory Stack
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │  L0  Identity text           ~100 tokens   always loaded │
//! │  L1  Essential story         ~500-800 tok  always loaded │
//! │  L2  On-demand recall        ~200-500 tok  loaded on ask │
//! │  L3  Deep semantic search    unlimited     full memory   │
//! └─────────────────────────────────────────────────────────┘
//! ```

pub mod config;
pub mod convo;
pub mod db;
pub mod embedder;
pub mod extractor;
pub mod graph;
pub mod knowledge;
pub mod layers;
pub mod miner;
pub mod search;
pub mod types;

pub use config::Config;
pub use convo::{ConvoMineStats, ConvoMiner};
pub use db::{AimemDb, EmbeddingStoreProfile};
pub use embedder::{Embedder, Gemini2Embedder, LocalEmbedder};
pub use graph::{AimemGraph, TraversalNode};
pub use knowledge::KnowledgeGraph;
pub use layers::MemoryStack;
pub use miner::Miner;
pub use search::Searcher;
pub use types::{ContentPart, Drawer, DrawerMeta, Entity, RoomNode, SearchResult, Triple, Tunnel};

/// Curated high-level imports for typical AiMem integrations.
pub mod prelude {
    pub use crate::{
        AimemDb, AimemGraph, Config, ContentPart, ConvoMineStats, ConvoMiner, Drawer, DrawerMeta,
        Embedder, EmbeddingStoreProfile, Entity, Gemini2Embedder, KnowledgeGraph, LocalEmbedder,
        MemoryStack, Miner, RoomNode, SearchResult, Searcher, Triple, Tunnel,
    };
}
