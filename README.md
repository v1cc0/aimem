# AiMem

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

The `0.3.x` line introduced the main architectural improvements that were missing from the old docs:

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

## Architecture

```text
project files / chat exports / manual filing
                  │
                  ▼
           aimem CLI / MCP
                  │
                  ▼
               aimem-core
      ┌────────────┼────────────┐
      │            │            │
      ▼            ▼            ▼
    mining       search      memory stack
      │            │            │
      └────────────┴────────────┘
                  │
                  ▼
          ~/.aimem/aimem.db
     + embedding profile metadata
```

The shape is intentionally simple: one DB file, thin frontends, reusable core logic.

## Embedding modes

AiMem currently supports two embedding modes:

### 1. Local

- `LocalEmbedder`
- backed by `fastembed`
- default recommendation
- keeps embedding generation local

### 2. Remote

- `Gemini2Embedder`
- explicit opt-in through CLI/API
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

So switching from local `384d` embeddings to remote `768d` embeddings in the same DB will fail fast instead of silently corrupting search quality.

## API stability

Prefer one of the high-level integration surfaces:

### 1. Prelude

```rust
use aimem_core::prelude::*;
```

### 2. Explicit root imports

```rust
use aimem_core::{AimemDb, Config, ConvoMiner, Drawer, LocalEmbedder, MemoryStack, Miner, Searcher};
```

New integrations should avoid depending on deep internal module paths when root exports or `prelude` are enough.

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

## Build

```bash
cargo build
```

## Test

```bash
cargo test
cargo test -p aimem-core --test performance_smoke -- --ignored
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

- `aimem status` now shows the current embedding profile stored in the DB.
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

`aimem_status` now also reports the current embedding profile.

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
