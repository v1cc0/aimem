# AiMem

**[English](https://github.com/v1cc0/aimem/blob/main/README.md) | 简体中文 | [日本語](https://github.com/v1cc0/aimem/blob/main/README.ja.md)**

[![crates.io: aimem-core](https://img.shields.io/crates/v/aimem-core)](https://crates.io/crates/aimem-core)
[![crates.io: aimem-cli](https://img.shields.io/crates/v/aimem-cli)](https://crates.io/crates/aimem-cli)
[![crates.io: aimem-mcp](https://img.shields.io/crates/v/aimem-mcp)](https://crates.io/crates/aimem-mcp)

AiMem 是面向 AI agent 的 Rust-first 本地记忆基础设施。

它把长期记忆存进一个 Turso 数据库，并提供：

- `aimem-core` — 存储、挖掘、搜索、memory layers、知识图谱
- `aimem` — CLI
- `aimem-mcp` — stdio MCP server

## 工作区结构

```text
crates/
├── aimem-core/
├── aimem-cli/
└── aimem-mcp/
```

## 核心特点

- 单一本地 Turso DB 文件：`~/.aimem/aimem.db`
- 文件型 store 使用 Turso multiprocess WAL 协调，旁边可能出现 `.db-wal` / `.db-tshm` sidecar
- hybrid 关键词 + 向量检索
- 不使用 embedding 时的 CJK / 日文 keyword fallback
- 可复现 memory benchmark
- 项目挖掘与会话导入
- 4 层 wake-up memory stack
- 多模态内容模型
- 默认本地 embedder
- 可选远程 Gemini embedding
- 面向 agent tooling 的 MCP 接入
- 基于本地 AiMem DB 的私有 Codex 编码经验工具
- 仓库内不依赖 Python runtime

## Embedding 模式

### Local

- `LocalEmbedder`
- 基于 `fastembed`
- 默认推荐
- embedding 在本地生成

### Remote

- `Gemini2Embedder`
- 显式 opt-in
- 只发送你显式提供的数据

重要安全边界：

- URI-only 的多媒体 part **不会**自动读取本地文件再上传。
- Remote embedding 只接受显式文本、data URI 或 raw bytes。

## Store 兼容性保护

AiMem 会把 embedding profile 元数据写入 store，并拒绝混用不同 embedding store。

写入和语义查询会检查：

- provider
- model
- dimension

所以从本地 `384d` embedding 切到远程 `768d` embedding 时，同一个 DB 不会静默降级，而是会快速失败。

对于附件类写入，AiMem 可以通过 `MemoryStack::file_drawers_with_ids(...)` 批量提交调用方提供的 stable-ID drawers。下游应用可以把同一个文件的 summary + chunk drawers 合并到一次 embedding 调用，并在重试时先跳过已写入的 ID。

## 安装

CLI：

```bash
cargo install aimem-cli
```

MCP server：

```bash
cargo install aimem-mcp
```

Library：

```bash
cargo add aimem-core
```

## 快速开始

创建一个极小的项目配置：

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

把项目挖进本地记忆：

```bash
aimem mine /path/to/project --no-embed
```

查询已存内容：

```bash
aimem status
aimem search "why did we choose Turso?"
aimem wake-up
```

启用 remote embedding：

```bash
export GEMINI_API_KEY=...
aimem search "why did we choose Turso?" --gemini-key "$GEMINI_API_KEY"
```

## 最小 Rust 例子

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
aimem search "hybrid search"
aimem mine /path/to/project --no-embed
```

补充说明：

- `aimem status` 会显示 DB 中当前的 embedding profile。
- `aimem search` / `aimem mine` 可通过 `--gemini-key` 或 `GEMINI_API_KEY` 启用 opt-in remote embedding。
- 当 embedder 可用时，`aimem search` 会走 hybrid 关键词 + 向量排序。
- 项目挖掘要求目标项目根目录存在 `aimem.yaml`。

## MCP

```bash
aimem-mcp
```

当前工具：

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

`aimem_status` 也会返回当前 embedding profile。

`codex_*` 工具是面向 Codex 类 agent 的私有、本地编码经验工具。它们把已编辑 repo profile、紧凑可复用 experience card 和任务相关上下文存在同一个 AiMem DB 中。`codex_record_repo` 只围绕显式传入的 `repo_path` 做保守检测：repo root、已知 manifest、可能的 test commands、`.git` HEAD/origin。`codex_record_command` 会记录验证 / 诊断命令；`codex_record_round_summary` 会记录 handoff summary、changed files、commands 和 next steps。`codex_context` 会用同 repo、incident、同 language 和关键词重叠 boost 对相关卡片排序。experience card 使用基于 repo/kind/problem/solution 的稳定指纹做重复检测：完全 replay 会跳过，但 command result 或 round summary 改变会作为新证据卡记录。`codex_delete_experience` 可按 drawer ID 删除私有 Codex 卡片。设计上保持显式：不自动爬取文件系统、不自动导入完整对话 transcript，也不采集 secret。

- Private Codex MCP smoke test: [`docs/private-codex-mcp-smoke-test.md`](https://github.com/v1cc0/aimem/blob/main/docs/private-codex-mcp-smoke-test.md)

## 配置

默认本地路径：

- database: `~/.aimem/aimem.db`
- identity: `~/.aimem/identity.txt`

环境变量覆盖：

- `AIMEM_DB_PATH`
- `AIMEM_IDENTITY_PATH`
- `GEMINI_API_KEY`

## Repository

- repo: `https://github.com/v1cc0/aimem`
- license: MIT
- Inspired by https://github.com/milla-jovovich/mempalace
- 欢迎 issue。AI 生成的 PR 可能会被忽略。
