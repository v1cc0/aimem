# aimem-cli

**语言 / Language / 言語：** [English](https://github.com/v1cc0/aimem/blob/main/crates/aimem-cli/README.md) | 简体中文 | [日本語](https://github.com/v1cc0/aimem/blob/main/crates/aimem-cli/README.ja.md)

`aimem-cli` 提供 AiMem 的 `aimem` 命令行接口。

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
aimem search "vector search"
aimem mine /path/to/project --no-embed
```

## Remote embedding

```bash
export GEMINI_API_KEY=...
aimem search "vector search" --gemini-key "$GEMINI_API_KEY"
```

## 说明

- `aimem status` 会显示当前 embedding profile。
- `aimem search` / `aimem mine` 可使用 `--gemini-key` 或 `GEMINI_API_KEY`。
- 默认 DB 路径是 `~/.aimem/aimem.db`。
