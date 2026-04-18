# aimem-mcp

**语言 / Language / 言語：** [English](https://github.com/v1cc0/aimem/blob/main/crates/aimem-mcp/README.md) | 简体中文 | [日本語](https://github.com/v1cc0/aimem/blob/main/crates/aimem-mcp/README.ja.md)

`aimem-mcp` 是 AiMem 的 stdio MCP server。

## 安装

```bash
cargo install aimem-mcp
```

## 运行

```bash
aimem-mcp
```

## 工具

- `aimem_status`
- `aimem_list_wings`
- `aimem_list_rooms`
- `aimem_get_taxonomy`
- `aimem_get_aaak_spec`
- `aimem_search`
- `aimem_check_duplicate`
- `aimem_add_drawer`
- `aimem_delete_drawer`

## 说明

- `aimem_status` 会返回当前 embedding profile。
- `aimem_search` 在有 embedder 时会走 hybrid 关键词 + 向量排序，否则回退到关键词搜索。
- 默认 DB 路径为 `~/.aimem/aimem.db`。
