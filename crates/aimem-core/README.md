# aimem-core

**Language / 语言 / 言語:** English | [简体中文](https://github.com/v1cc0/aimem/blob/main/crates/aimem-core/README.zh-CN.md) | [日本語](https://github.com/v1cc0/aimem/blob/main/crates/aimem-core/README.ja.md)

`aimem-core` is AiMem's Rust core library.

It provides:

- Turso-backed storage
- multiprocess WAL coordination for file-backed stores (`.db-tshm` sidecar)
- project mining and conversation import
- hybrid keyword + vector retrieval
- memory layers / wake-up context generation
- knowledge graph support
- embedding profile guards
- multimodal content primitives
- Unicode/CJK/Kana-aware keyword fallback for Chinese and Japanese retrieval
- reproducible text and deterministic multimodal truth benchmarks in the repository

CLI lives in `aimem-cli`. MCP server lives in `aimem-mcp`.

## 0.5.0 upgrade: breaking changes

- **New embedding model:** local MiniLM `384d` / fastembed is replaced by EmbeddingGemma 2 ONNX Q8 `768d`. Old vectors and remote Gemini vectors cannot share the new model's store. Stop writers, back up the old DB and its WAL sidecars, then re-embed original text into a separate database. Do not relabel stored profiles; there is no automatic vector conversion.
- **Rust 1.95+** is now required. First local use downloads about 314 MB of weights plus the tokenizer; subsequent runs can use the cache offline.
- **Turso 0.8.3:** downstream applications using Turso directly should align versions to avoid incompatible types or duplicate native symbols.
- **Rust API:** replace matches on `EmbedError::Fastembed` with `EmbedError::Local`. Local provider/model/dimension constants have changed; use `embed_query` for manual retrieval queries.

[Full release notes and migration instructions](https://github.com/v1cc0/aimem/blob/v0.5.0/CHANGELOG.md).

## Current capabilities

Current `aimem-core` includes:

- async embedding flow
- `LocalEmbedder`
- runs [`onnx-community/embeddinggemma-2-ONNX`](https://huggingface.co/onnx-community/embeddinggemma-2-ONNX) through ONNX Runtime using Q8 text weights and normalized `768d` vectors
- downloads about 314 MB of weights plus the tokenizer to the Hugging Face cache on first use (`HF_HOME` / `HF_HUB_CACHE` supported); works offline once cached
- embeds text only, preserving text extraction from multimodal inputs; truncates at 8,192 tokens
- `embed` / `embed_one` add the document prefix; `embed_query` adds the retrieval query prefix, which `Searcher` uses automatically
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
- `MemoryStack::file_drawers_with_ids(...)`
- Turso FTS + RRF-based hybrid search
- CJK/Japanese keyword fallback for no-embedding search
- reproducible EN/ZH/JA benchmark fixtures and result snapshots
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
- runs [`onnx-community/embeddinggemma-2-ONNX`](https://huggingface.co/onnx-community/embeddinggemma-2-ONNX) through ONNX Runtime using Q8 text weights and normalized `768d` vectors
- downloads about 314 MB of weights plus the tokenizer to the Hugging Face cache on first use (`HF_HOME` / `HF_HUB_CACHE` supported); works offline once cached
- embeds text only, preserving text extraction from multimodal inputs; truncates at 8,192 tokens
- `embed` / `embed_one` add the document prefix; `embed_query` adds the retrieval query prefix, which `Searcher` uses automatically
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

Legacy MiniLM `384d` vectors cannot be mixed with the new local `768d` vectors. Local EmbeddingGemma 2 and remote Gemini also cannot share a store even though both produce `768d` vectors. To migrate, rebuild a separate database from the original text; existing databases are never converted or deleted automatically.

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

When you already have a related set of stable-ID drawers (for example one
attachment summary plus chunk drawers), batch them in one call:

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
    .file_drawers_with_ids(&[
        DrawerFilingRequest::new(
            "attachment.summary.001",
            "attachments",
            "user-123",
            "Attachment summary".to_string(),
            "example",
        )
        .with_parts(vec![ContentPart::text("Attachment summary")])
        .with_source_file("report.pdf"),
        DrawerFilingRequest::new(
            "attachment.chunk.001",
            "attachments",
            "user-123",
            "Attachment chunk 1".to_string(),
            "example",
        )
        .with_parts(vec![ContentPart::text("Attachment chunk 1")])
        .with_source_file("report.pdf")
        .with_chunk_index(1),
    ])
    .await?;

println!("inserted={inserted:?}");
# Ok(())
# }
```

AiMem checks existing stable IDs before embedding, so retrying the same batch
does not spend another remote embedding call on drawers that are already stored.

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

## Hybrid search example

```rust,no_run
use std::sync::Arc;
use aimem_core::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = AimemDb::memory().await?;
    let embedder = Arc::new(LocalEmbedder::new()?);

    let content = "We moved the memory backend to Turso for local hybrid search.";
    let embedding = embedder.embed_one(content).await?;

    let drawer = Drawer::new("drawer_demo_001", "demo_app", "backend", content, "example");
    db.insert_drawer_with_profile(
        &drawer,
        Some(&embedding),
        embedder.provider_name(),
        embedder.model_name(),
    ).await?;

    let searcher = Searcher::new(db, embedder);
    let hits = searcher.hybrid_search("local memory database", Some("demo_app"), None, 5).await?;
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
- File-backed `AimemDb::open()` enables Turso multiprocess WAL coordination and may create `.db-wal` / `.db-tshm` sidecars beside the DB file.
- `Miner::new(db, None)` and `ConvoMiner::new(db, None)` work fine for text-only filing.
- drawers without embeddings still appear in keyword / fallback search.
- `AimemDb::embedding_profile()` reports the current store profile.
