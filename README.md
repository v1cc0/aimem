# AiMem

[![Rust CI](https://github.com/v1cc0/aimem/actions/workflows/rust.yml/badge.svg)](https://github.com/v1cc0/aimem/actions/workflows/rust.yml)
[![crates.io: aimem-core](https://img.shields.io/crates/v/aimem-core)](https://crates.io/crates/aimem-core)
[![crates.io: aimem-cli](https://img.shields.io/crates/v/aimem-cli)](https://crates.io/crates/aimem-cli)
[![crates.io: aimem-mcp](https://img.shields.io/crates/v/aimem-mcp)](https://crates.io/crates/aimem-mcp)

Inspired by https://github.com/milla-jovovich/mempalace

Small solo project. Issues are welcome.

Rust-first local memory infrastructure for AI agents.

`aimem` keeps long-term memory in a single Turso database and exposes three layers:

- `aimem-core` — storage, mining, search, memory layers, knowledge graph
- `aimem` — CLI
- `aimem-mcp` — stdio MCP server

## Workspace layout

```text
crates/
├── aimem-core/
├── aimem-cli/
└── aimem-mcp/
```

## Highlights

- local-only storage with Turso
- keyword + semantic retrieval
- project mining and conversation mining
- 4-layer wake-up memory stack
- MCP integration for agent tooling
- no Python runtime in this repository

## Architecture

```text
project files / chat exports
           │
           ▼
    aimem CLI / MCP tools
           │
           ▼
        aimem-core
   ┌────────┼────────┐
   │        │        │
   ▼        ▼        ▼
 mining   search   memory layers
   │        │        │
   └────────┴────────┘
           │
           ▼
   ~/.aimem/aimem.db
     (single Turso DB)
```

The shape is intentionally simple: one local DB file, thin CLI/MCP frontends, and reusable logic in `aimem-core`.

## API stability

For `0.1.x`, prefer the root-level `aimem_core::{...}` re-exports as the stable integration surface.

Direct module imports such as `aimem_core::miner::...` or `aimem_core::extractor::...` are still public today for backward compatibility with `0.1.0`, but they are a wider surface than we want long term. New integrations should bias toward the root types:

- `Config`
- `PalaceDb`
- `Embedder`
- `Miner`
- `ConvoMiner`
- `Searcher`
- `KnowledgeGraph`
- `MemoryStack`
- core data types like `Drawer`, `SearchResult`, `Triple`

## Build

```bash
cargo build
```

## Test

```bash
cargo test
cargo test -p aimem-core --test performance_smoke -- --ignored
```

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

```toml
[dependencies]
aimem-core = "0.1.0"
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

Minimal Rust embedding:

```rust
use aimem_core::{Drawer, PalaceDb, Searcher};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = PalaceDb::open("./aimem.db").await?;

    let drawer = Drawer {
        id: "drawer_demo_001".into(),
        wing: "demo_app".into(),
        room: "backend".into(),
        content: "We chose Turso so storage and retrieval stay local.".into(),
        source_file: Some("DECISIONS.md".into()),
        chunk_index: 0,
        added_by: "example".into(),
        filed_at: chrono::Utc::now().to_rfc3339(),
    };

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

Project mining expects an `aimem.yaml` file in the target project root.

## MCP

```bash
cargo run -p aimem-mcp
```

Supported tools in the current MVP:

- `aimem_status`
- `aimem_list_wings`
- `aimem_list_rooms`
- `aimem_get_taxonomy`
- `aimem_get_aaak_spec`
- `aimem_search`
- `aimem_check_duplicate`
- `aimem_add_drawer`
- `aimem_delete_drawer`

## Config

Default local paths:

- database: `~/.aimem/aimem.db`
- identity: `~/.aimem/identity.txt`

If an older `~/.aimem/palace.db` already exists, AiMem still opens it automatically for backward compatibility.

Environment overrides:

- `AIMEM_DB_PATH`
- `AIMEM_IDENTITY_PATH`

## Repository

- repo: `https://github.com/v1cc0/aimem`
- license: MIT
