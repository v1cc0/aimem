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
- `codex_record_repo`
- `codex_record_experience`
- `codex_search_experience`
- `codex_context`

## 说明

- `aimem_status` 会返回当前 embedding profile。
- `aimem_search` 在有 embedder 时会走 hybrid 关键词 + 向量排序，否则回退到关键词搜索。
- keyword fallback 包含面向中文 / 日文查询的 Unicode / CJK / Kana n-gram scoring。
- `codex_*` 工具是面向 Codex 类 agent 的私有编码经验层：把已编辑 repo profile、紧凑 experience card 和任务上下文写入同一个本地 AiMem DB。它们**不会**自动爬取文件系统，也不会自动导入完整对话 transcript。
- 默认 DB 路径为 `~/.aimem/aimem.db`；Turso 可能在旁边创建 `.db-wal` / `.db-tshm` sidecar。
