# aimem-mcp

**Language / 语言 / 言語:** [English](https://github.com/v1cc0/aimem/blob/main/crates/aimem-mcp/README.md) | [简体中文](https://github.com/v1cc0/aimem/blob/main/crates/aimem-mcp/README.zh-CN.md) | 日本語

`aimem-mcp` は AiMem の stdio MCP サーバーです。

## インストール

```bash
cargo install aimem-mcp
```

## 実行

```bash
aimem-mcp
```

## ツール

- `aimem_status`
- `aimem_list_wings`
- `aimem_list_rooms`
- `aimem_get_taxonomy`
- `aimem_get_aaak_spec`
- `aimem_search`
- `aimem_check_duplicate`
- `aimem_add_drawer`
- `aimem_delete_drawer`

## メモ

- `aimem_status` は embedding profile も返します。
- `aimem_search` は embedder が使える場合に semantic search を優先します。
