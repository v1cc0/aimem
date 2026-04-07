# AiMem

[![crates.io: aimem-core](https://img.shields.io/crates/v/aimem-core)](https://crates.io/crates/aimem-core)
[![crates.io: aimem-cli](https://img.shields.io/crates/v/aimem-cli)](https://crates.io/crates/aimem-cli)
[![crates.io: aimem-mcp](https://img.shields.io/crates/v/aimem-mcp)](https://crates.io/crates/aimem-mcp)

Inspired by https://github.com/milla-jovovich/mempalace
Vibe Alert: Entirely vibe coding by AI, issues are welcome, can't handle PR :(

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
