# aimem-mcp

**语言 / Language / 言語：** [English](https://github.com/v1cc0/aimem/blob/main/crates/aimem-mcp/README.md) | 简体中文 | [日本語](https://github.com/v1cc0/aimem/blob/main/crates/aimem-mcp/README.ja.md)

`aimem-mcp` 是 AiMem 的 stdio MCP server。

## 0.5.0 升级：破坏性变更

- **默认向量模型已更换：** 本地 MiniLM `384d` / fastembed 改为 EmbeddingGemma 2 ONNX Q8 `768d`。旧向量和远程 Gemini 向量均不能与新模型共用向量库。先停止写入并备份旧数据库及其 WAL sidecar，再从原始文本向独立数据库重新生成向量；不要修改已有 profile 绕过检查，不提供自动向量转换。
- **最低要求 Rust 1.95。** 首次本地运行下载约 314 MB 权重及分词器，之后可使用缓存离线运行。
- **Turso 升至 0.8.3：** 下游应用若直接依赖 Turso，应统一版本，避免类型不兼容或 native 符号重复。

[完整发布说明与迁移步骤](https://github.com/v1cc0/aimem/blob/v0.5.0/CHANGELOG.md).

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
- `codex_record_command`
- `codex_record_round_summary`
- `codex_delete_experience`
- `codex_search_experience`
- `codex_context`

## 说明

- `aimem_status` 会返回当前 embedding profile。
- `aimem_search` 在有 embedder 时会走 hybrid 关键词 + 向量排序，否则回退到关键词搜索。
- keyword fallback 包含面向中文 / 日文查询的 Unicode / CJK / Kana n-gram scoring。
- `codex_*` 工具是面向 Codex 类 agent 的私有编码经验层：把已编辑 repo profile、紧凑 experience card 和任务上下文写入同一个本地 AiMem DB。`codex_record_repo` 只围绕显式传入的 `repo_path` 做保守检测（repo root、已知 manifest、可能的 test commands、`.git` HEAD/origin）。`codex_record_command` 会记录验证 / 诊断命令；`codex_record_round_summary` 会记录 handoff summary、changed files、commands 和 next steps。`codex_context` 会用同 repo、incident、同 language 和关键词重叠 boost 对相关卡片排序。experience card 使用基于 repo/kind/problem/solution 的稳定指纹做重复检测：完全 replay 会跳过，但 command result 或 round summary 改变会作为新证据卡记录。`codex_delete_experience` 可按 drawer ID 删除私有 Codex 卡片。它们**不会**自动爬取文件系统，也不会自动导入完整对话 transcript。
- 默认 DB 路径为 `~/.aimem/aimem.db`；Turso 可能在旁边创建 `.db-wal` / `.db-tshm` sidecar。
- private Codex MCP smoke test: [`docs/private-codex-mcp-smoke-test.md`](https://github.com/v1cc0/aimem/blob/main/docs/private-codex-mcp-smoke-test.md)
