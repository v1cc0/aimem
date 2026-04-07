# aimem-mcp

`aimem-mcp` is the stdio MCP server for AiMem.

## Install

```bash
cargo install aimem-mcp
```

## Run

```bash
aimem-mcp
```

The server exposes AiMem storage and retrieval tools over MCP, including:

- `aimem_status`
- `aimem_list_wings`
- `aimem_list_rooms`
- `aimem_get_taxonomy`
- `aimem_get_aaak_spec`
- `aimem_search`
- `aimem_check_duplicate`
- `aimem_add_drawer`
- `aimem_delete_drawer`

Default local database path: `~/.aimem/aimem.db`
