# aimem-core

**Language / 语言 / 言語:** English | [简体中文](https://github.com/v1cc0/aimem/blob/main/crates/aimem-core/README.zh-CN.md) | [日本語](https://github.com/v1cc0/aimem/blob/main/crates/aimem-core/README.ja.md)

`aimem-core` is AiMem's Rust core library.

It provides:

- Turso-backed storage
- project mining and conversation import
- keyword and semantic retrieval
- memory layers / wake-up context generation
- knowledge graph support
- embedding profile guards
- multimodal content primitives

CLI lives in `aimem-cli`. MCP server lives in `aimem-mcp`.

## 0.3.x capabilities

Current `aimem-core` includes:

- async embedding flow
- `LocalEmbedder`
- opt-in `Gemini2Embedder`
- multimodal `ContentPart`
- embedding profile metadata:
  - provider
  - model
  - dimension
- profile guards against mixed stores
- `Drawer` helper API
- `MemoryStack::file_text(...)`
- `MemoryStack::file_drawer_with_id(...)`
- tighter extractor heuristics with multilingual regression tests

## Install

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
```

```bash
cargo add aimem-core
```

## Recommended API surface

Prefer either:

```rust
use aimem_core::prelude::*;
```

or explicit root imports like:

```rust
use aimem_core::{AimemDb, Drawer, LocalEmbedder, MemoryStack, Miner, Searcher};
```

## Core concepts

- **Store** — one AiMem DB
- **Wing** — project or domain
- **Room** — topic inside a wing
- **Drawer** — one stored chunk
- **ContentPart** — multimodal part inside a drawer
- **L0/L1/L2/L3** — identity, essential story, recall, deep search

## Embedding modes

### Local

- `LocalEmbedder`
- default recommendation
- embedding stays local

### Remote

- `Gemini2Embedder`
- explicit opt-in
- only explicit text / data URI / raw bytes are sent

URI-only multimedia parts are not automatically read from disk and uploaded.

## Store compatibility guard

`aimem-core` records embedding profile metadata and validates semantic writes and queries against:

- provider
- model
- dimension

This prevents silent mixing of incompatible embedding stores.

## Drawer helpers

```rust,no_run
use aimem_core::prelude::*;

let drawer = Drawer::new("id", "wing", "room", "content", "agent")
    .with_source_file("README.md")
    .with_chunk_index(3);
```

Stable-ID filing through `MemoryStack`:

```rust,no_run
use std::sync::Arc;
use aimem_core::prelude::*;

# #[tokio::main]
# async fn main() -> anyhow::Result<()> {
let cfg = Config::load()?;
let db = AimemDb::open(&cfg.db_path).await?;
let embedder = Arc::new(LocalEmbedder::new()?);
let stack = MemoryStack::new(db, embedder, &cfg);

let inserted = stack
    .file_drawer_with_id(
        "attachment.chunk.001",
        "attachments",
        "user-123",
        "Attachment chunk body".to_string(),
        vec![ContentPart::text("Attachment chunk body")],
        Some("report.pdf"),
        1,
        "example",
    )
    .await?;

println!("inserted={inserted}");
# Ok(())
# }
```

For multimodal drawers:

```rust,no_run
use aimem_core::prelude::*;

let drawer = Drawer::multimodal(
    "id",
    "wing",
    "room",
    "Look at this image",
    vec![ContentPart::text("Look at this image")],
    "agent",
);
```

## Semantic search example

```rust,no_run
use std::sync::Arc;
use aimem_core::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = AimemDb::memory().await?;
    let embedder = Arc::new(LocalEmbedder::new()?);

    let content = "We moved the memory backend to Turso for local vector search.";
    let embedding = embedder.embed_one(content).await?;

    let drawer = Drawer::new("drawer_demo_001", "demo_app", "backend", content, "example");
    db.insert_drawer_with_profile(
        &drawer,
        Some(&embedding),
        embedder.provider_name(),
        embedder.model_name(),
    ).await?;

    let searcher = Searcher::new(db, embedder);
    let hits = searcher.vector_search("local vector database", Some("demo_app"), None, 5).await?;
    assert!(!hits.is_empty());
    Ok(())
}
```

## Filing text directly

```rust,no_run
use std::sync::Arc;
use aimem_core::prelude::*;

# #[tokio::main]
# async fn main() -> anyhow::Result<()> {
let cfg = Config::load()?;
let db = AimemDb::open(&cfg.db_path).await?;
let embedder = Arc::new(LocalEmbedder::new()?);
let stack = MemoryStack::new(db, embedder, &cfg);

let id = stack.file_text("demo_app", "notes", "remember this", "example").await?;
println!("{id}");
# Ok(())
# }
```

## Notes

- `AimemDb::open()` bootstraps schema automatically.
- `Miner::new(db, None)` and `ConvoMiner::new(db, None)` work fine for text-only filing.
- drawers without embeddings still appear in keyword search.
- `AimemDb::embedding_profile()` reports the current store profile.
