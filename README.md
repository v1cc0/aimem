# AiMem

**Language / 语言 / 言語:** English | [简体中文](https://github.com/v1cc0/aimem/blob/main/README.zh-CN.md) | [日本語](https://github.com/v1cc0/aimem/blob/main/README.ja.md)

> GitHub and crates.io do not provide native README language tabs here, so AiMem uses language switch links instead.

[![crates.io: aimem-core](https://img.shields.io/crates/v/aimem-core)](https://crates.io/crates/aimem-core)
[![crates.io: aimem-cli](https://img.shields.io/crates/v/aimem-cli)](https://crates.io/crates/aimem-cli)
[![crates.io: aimem-mcp](https://img.shields.io/crates/v/aimem-mcp)](https://crates.io/crates/aimem-mcp)

Inspired by https://github.com/milla-jovovich/mempalace

Small solo project. Issues are welcome.

AiMem is Rust-first local memory infrastructure for AI agents.

It stores long-term memory in a single Turso database and exposes:

- `aimem-core` — storage, mining, search, memory layers, knowledge graph
- `aimem` — CLI
- `aimem-mcp` — stdio MCP server

## What 0.3.x added

The `0.3.x` line introduced the main architectural improvements that were missing from older docs:

- async embedding flow
- `LocalEmbedder` and opt-in `Gemini2Embedder`
- multimodal `ContentPart`
- embedding store profile tracking:
  - provider
  - model
  - dimension
- profile guards that reject mixed embedding stores
- `Drawer` helper API
- `MemoryStack::file_text(...)`
- `MemoryStack::file_drawer_with_id(...)`
- `MemoryStack::file_drawers_with_ids(...)`
- CLI / MCP status now showing embedding profile
- tighter extractor heuristics with multilingual regression tests
- CI dependency auditing via `cargo audit`

## Workspace layout

```text
crates/
├── aimem-core/
├── aimem-cli/
└── aimem-mcp/
```

## Highlights

- one local Turso DB file: `~/.aimem/aimem.db`
- keyword + semantic retrieval
- project mining and conversation mining
- 4-layer wake-up memory stack
- multimodal content model
- local embedder by default
- opt-in remote Gemini embedding
- MCP integration for agent tooling
- no Python runtime in this repository

## Embedding modes

### Local

- `LocalEmbedder`
- backed by `fastembed`
- default recommendation
- keeps embedding generation local

### Remote

- `Gemini2Embedder`
- explicit opt-in
- sends only data you explicitly provide

Important safety boundary:

- URI-only multimodal parts are **not** automatically read from local disk and uploaded.
- Remote embedding requires explicit text, data URI, or raw bytes.

## Store compatibility guard

AiMem records embedding profile metadata inside the store and rejects mixed stores.

That means writes and semantic queries are checked against:

- provider
- model
- dimension

So switching from local `384d` embeddings to remote `768d` embeddings in the same DB fails fast instead of silently degrading retrieval quality.

For attachment-style ingestion, AiMem can now batch a caller-supplied set of
stable-ID drawers through `MemoryStack::file_drawers_with_ids(...)`. That lets
downstream apps group one file's summary + chunk drawers into one embedding call
and skip already-filed IDs before embedding on retries.

## Install

CLI:

```bash
cargo install aimem-cli
```

MCP server:

```bash
cargo install aimem-mcp
```

Library:

```bash
cargo add aimem-core
```

## Quick start

Create a tiny project config:

```yaml
# aimem.yaml
wing: demo_app
rooms:
  - name: backend
    description: backend code and docs
    keywords: [router, handler, database, rust]
  - name: decisions
    description: architecture and tradeoffs
    keywords: [decided, chose, tradeoff, because]
```

Mine a project into local memory:

```bash
aimem mine /path/to/project --no-embed
```

Query what was stored:

```bash
aimem status
aimem search "why did we choose Turso?"
aimem wake-up
```

For remote embedding:

```bash
export GEMINI_API_KEY=...
aimem search "why did we choose Turso?" --gemini-key "$GEMINI_API_KEY"
```

## Minimal Rust example

```rust
use aimem_core::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = AimemDb::open("./aimem.db").await?;

    let drawer = Drawer::new(
        "drawer_demo_001",
        "demo_app",
        "backend",
        "We chose Turso so storage and retrieval stay local.",
        "example",
    )
    .with_source_file("DECISIONS.md");

    db.insert_drawer(&drawer, None).await?;

    let hits = Searcher::keyword_only(db)
        .keyword_search("Turso", Some("demo_app"), None, 5)
        .await?;

    println!("hits = {}", hits.len());
    Ok(())
}
```

## CLI

```bash
aimem status
aimem wake-up
aimem search "vector search"
aimem mine /path/to/project --no-embed
```

Useful notes:

- `aimem status` shows the current embedding profile stored in the DB.
- `aimem search` and `aimem mine` can use `--gemini-key` or `GEMINI_API_KEY` for opt-in remote embedding.
- project mining expects an `aimem.yaml` file in the target project root.

## MCP

```bash
aimem-mcp
```

Current tools:

- `aimem_status`
- `aimem_list_wings`
- `aimem_list_rooms`
- `aimem_get_taxonomy`
- `aimem_get_aaak_spec`
- `aimem_search`
- `aimem_check_duplicate`
- `aimem_add_drawer`
- `aimem_delete_drawer`

`aimem_status` also reports the current embedding profile.

## Config

Default local paths:

- database: `~/.aimem/aimem.db`
- identity: `~/.aimem/identity.txt`

Environment overrides:

- `AIMEM_DB_PATH`
- `AIMEM_IDENTITY_PATH`
- `GEMINI_API_KEY`

## Repository

- repo: `https://github.com/v1cc0/aimem`
- license: MIT
