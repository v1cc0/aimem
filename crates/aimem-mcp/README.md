# aimem-mcp

`aimem-mcp` is the stdio MCP server for AiMem.

It exposes AiMem storage and retrieval capabilities to MCP-compatible clients over stdio.

## Install

```bash
cargo install aimem-mcp
```

## Run

```bash
aimem-mcp
```

## What the current MCP server exposes

The server reflects the newer 0.3.x capabilities too:

- keyword + semantic search
- duplicate checks
- manual drawer filing
- embedding-aware search behavior
- store status with embedding profile

## Tools

- `aimem_status`
- `aimem_list_wings`
- `aimem_list_rooms`
- `aimem_get_taxonomy`
- `aimem_get_aaak_spec`
- `aimem_search`
- `aimem_check_duplicate`
- `aimem_add_drawer`
- `aimem_delete_drawer`

## Notes

- `aimem_status` reports:
  - total drawers
  - wing / room counts
  - DB path
  - embedding profile
- `aimem_search` prefers semantic search when an embedder is available and falls back to keyword search otherwise.
- `aimem_add_drawer` uses duplicate checking before insertion.

Default local database path:

- `~/.aimem/aimem.db`
