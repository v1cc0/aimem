//! # aimem-core
//!
//! **Give your AI a memory — reusable core library.**
//!
//! `aimem-core` is a pure-Rust library that stores, retrieves, and searches
//! text memories using [Turso](https://github.com/tursodatabase/turso) (an in-process
//! SQLite engine with native vector support) as the sole storage backend.
//! No external vector database, no cloud service, no API key.
//!
//! ## Feature overview
//!
//! | Feature | Implementation |
//! |---------|---------------|
//! | Vector storage | Turso `vector32()` BLOB column |
//! | Semantic search | `vector_distance_cos()` — SimSIMD-accelerated cosine |
//! | Full-text search | SQL `LIKE` scan (Turso FTS index optional) |
//! | Embedding generation | [`fastembed`](https://crates.io/crates/fastembed) — `all-MiniLM-L6-v2` locally |
//! | Knowledge graph | Temporal entity-relationship triples in SQLite |
//! | Graph traversal | BFS over wing/room adjacency — no graph DB needed |
//! | MCP protocol | Implemented in `aimem-mcp` (separate crate) |
//!
//! ## Architecture: 4-Layer Memory Stack
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │  L0  Identity text           ~100 tokens   always loaded │
//! │  L1  Essential story         ~500-800 tok  always loaded │
//! │  L2  On-demand recall        ~200-500 tok  loaded on ask │
//! │  L3  Deep semantic search    unlimited     full palace   │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Storage model
//!
//! Everything lives in a **single Turso DB file** (default: `~/.aimem/palace.db`):
//!
//! ```text
//! palace.db
//! ├── drawers   — verbatim text chunks + vector32 embeddings
//! ├── entities  — knowledge graph nodes
//! └── triples   — temporal (subject → predicate → object) edges
//! ```
//!
//! ## Concepts
//!
//! | Term | Meaning |
//! |------|---------|
//! | **Palace** | The entire memory store (one DB file) |
//! | **Wing** | A named project or domain (e.g. `"my_app"`, `"journal"`) |
//! | **Room** | A topic within a wing (e.g. `"backend"`, `"decisions"`) |
//! | **Drawer** | One verbatim text chunk (800 chars) with its embedding |
//! | **Tunnel** | A room that spans multiple wings — a conceptual bridge |
//!
//! ## Quick start
//!
//! ```toml
//! # Cargo.toml
//! [dependencies]
//! aimem-core = "0.1.0"
//! tokio = { version = "1", features = ["full"] }
//! ```
//!
//! ```rust,no_run
//! use aimem_core::{Config, Embedder, MemoryStack, PalaceDb};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     // 1. Load config (env > ~/.aimem/config.json > defaults)
//!     let cfg = Config::load()?;
//!
//!     // 2. Open the palace (creates DB + schema on first run)
//!     let db = PalaceDb::open(&cfg.db_path).await?;
//!
//!     // 3. Load local embedding model (downloads ~23 MB on first use)
//!     let embedder = Embedder::new()?;
//!
//!     // 4. Build the memory stack
//!     let stack = MemoryStack::new(db, embedder, &cfg);
//!
//!     // 5. Wake-up prompt: L0 (identity) + L1 (top memories) — inject into LLM context
//!     println!("{}", stack.wake_up(None).await?);
//!
//!     // 6. Semantic search — uses Turso vector_distance_cos() natively
//!     println!("{}", stack.search("why did we choose Rust?", None, None).await?);
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Mining a project
//!
//! ```rust,no_run
//! use aimem_core::{Config, Embedder, Miner, PalaceDb};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let cfg = Config::load()?;
//!     let db = PalaceDb::open(&cfg.db_path).await?;
//!     let embedder = Embedder::new()?;
//!
//!     let miner = Miner::new(db, Some(embedder));
//!
//!     // Reads aimem.yaml in the project directory for wing/room config
//!     let stats = miner.mine("~/projects/my_app", None, "claude", 0, false).await?;
//!
//!     println!(
//!         "Mined {} drawers from {} files",
//!         stats.drawers_added,
//!         stats.files_scanned - stats.files_skipped,
//!     );
//!     Ok(())
//! }
//! ```
//!
//! ## Keyword-only search without embeddings
//!
//! If you mine with `Miner::new(db, None)` or `ConvoMiner::new(db, None)`,
//! drawers still participate in SQL keyword search even though they have no embeddings.
//!
//! ```rust,no_run
//! use aimem_core::{Drawer, PalaceDb, Searcher};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let db = PalaceDb::memory().await?;
//!     let drawer = Drawer {
//!         id: "drawer_kw_001".into(),
//!         wing: "demo".into(),
//!         room: "backend".into(),
//!         content: "Turso keeps the backend local and simple.".into(),
//!         source_file: None,
//!         chunk_index: 0,
//!         added_by: "example".into(),
//!         filed_at: chrono::Utc::now().to_rfc3339(),
//!     };
//!     db.insert_drawer(&drawer, None).await?;
//!
//!     let searcher = Searcher::keyword_only(db);
//!     let hits = searcher.keyword_search("Turso", Some("demo"), None, 5).await?;
//!     assert_eq!(hits.len(), 1);
//!     Ok(())
//! }
//! ```
//!
//! ## Knowledge graph
//!
//! ```rust,no_run
//! use aimem_core::{KnowledgeGraph, PalaceDb};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let db = PalaceDb::memory().await?; // in-memory for demo
//!     let kg = KnowledgeGraph::new(db);
//!
//!     // Store a temporal fact
//!     kg.add_triple("Max", "does", "swimming", Some("2025-01-01"), None).await?;
//!     kg.add_triple("Max", "loves", "chess", Some("2025-10-01"), None).await?;
//!
//!     // Query all outgoing facts for "Max"
//!     let facts = kg.query_entity("Max", None, "outgoing").await?;
//!     for f in &facts {
//!         println!("{} → {} → {}", f.subject, f.predicate, f.object);
//!     }
//!
//!     // Invalidate a fact (set valid_to = today)
//!     kg.invalidate("Max", "does", "swimming", None).await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Low-level storage operations
//!
//! ```rust,no_run
//! use aimem_core::{Drawer, Embedder, PalaceDb};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let db = PalaceDb::memory().await?;
//!     let embedder = Embedder::new()?;
//!
//!     let content = "We decided to use Rust for performance and memory safety.";
//!     let embedding = embedder.embed_one(content)?;
//!
//!     let drawer = Drawer {
//!         id: "drawer_test_decision_001".to_string(),
//!         wing: "my_project".to_string(),
//!         room: "decisions".to_string(),
//!         content: content.to_string(),
//!         source_file: Some("DECISIONS.md".to_string()),
//!         chunk_index: 0,
//!         added_by: "claude".to_string(),
//!         filed_at: chrono::Utc::now().to_rfc3339(),
//!     };
//!
//!     let inserted = db.insert_drawer(&drawer, Some(&embedding)).await?;
//!     println!("Inserted: {inserted}");
//!     println!("Total drawers: {}", db.drawer_count().await?);
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Crate feature flags
//!
//! `aimem-core` has no optional features itself — the embedding model
//! is always available. The `hf-hub` feature of `fastembed` controls whether
//! `Embedder::new()` can download models automatically.
//!
//! ## Reusing in other projects
//!
//! Because this is a library crate with no binary, any project can depend on it:
//!
//! ```toml
//! [dependencies]
//! aimem-core = { git = "https://github.com/v1cc0/aimem" }
//! # or local path:
//! aimem-core = { path = "/path/to/aimem/crates/aimem-core" }
//! ```
//!
//! The public re-exports in this file form the stable API surface.
//! Everything else is implementation detail.

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
pub use db::PalaceDb;
pub use embedder::Embedder;
pub use graph::{PalaceGraph, TraversalNode};
pub use knowledge::KnowledgeGraph;
pub use layers::MemoryStack;
pub use miner::Miner;
pub use search::Searcher;
pub use types::{Drawer, DrawerMeta, Entity, RoomNode, SearchResult, Triple, Tunnel};
