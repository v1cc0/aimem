# aimem-cli

**语言 / Language / 言語：** [English](https://github.com/v1cc0/aimem/blob/main/crates/aimem-cli/README.md) | 简体中文 | [日本語](https://github.com/v1cc0/aimem/blob/main/crates/aimem-cli/README.ja.md)

`aimem-cli` 提供 AiMem 的 `aimem` 命令行接口。

## 0.5.0 升级：破坏性变更

- **默认向量模型已更换：** 本地 MiniLM `384d` / fastembed 改为 EmbeddingGemma 2 ONNX Q8 `768d`。旧向量和远程 Gemini 向量均不能与新模型共用向量库。先停止写入并备份旧数据库及其 WAL sidecar，再从原始文本向独立数据库重新生成向量；不要修改已有 profile 绕过检查，不提供自动向量转换。
- **最低要求 Rust 1.95。** 首次本地运行下载约 314 MB 权重及分词器，之后可使用缓存离线运行。
- **Turso 升至 0.8.3：** 下游应用若直接依赖 Turso，应统一版本，避免类型不兼容或 native 符号重复。

[完整发布说明与迁移步骤](https://github.com/v1cc0/aimem/blob/v0.5.0/CHANGELOG.md).

## 当前 CLI 支持

- 项目挖掘
- 会话导入
- 关键词搜索
- 语义搜索
- wake-up context 生成
- store 状态查看
- opt-in remote embedding

## 安装

```bash
cargo install aimem-cli
```

## 用法

```bash
aimem status
aimem wake-up
aimem search "hybrid search"
aimem mine /path/to/project --no-embed
```

## Remote embedding

```bash
export GEMINI_API_KEY=...
aimem search "hybrid search" --gemini-key "$GEMINI_API_KEY"
```

## 说明

- `aimem status` 会显示当前 embedding profile。
- `aimem search` / `aimem mine` 可使用 `--gemini-key` 或 `GEMINI_API_KEY`。
- 当 embedder 可用时，`aimem search` 会走 hybrid 关键词 + 向量排序。
- keyword-only fallback 现在包含面向中文 / 日文查询的 Unicode / CJK / Kana n-gram scoring。
- 默认 DB 路径是 `~/.aimem/aimem.db`；Turso 可能在旁边创建 `.db-wal` / `.db-tshm` sidecar。
