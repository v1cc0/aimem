# AiMem

**语言 / Language / 言語：** [English](https://github.com/v1cc0/aimem/blob/main/README.md) | 简体中文 | [日本語](https://github.com/v1cc0/aimem/blob/main/README.ja.md)

> GitHub 和 crates.io 在这里都不提供原生 README 语言 tabs，所以 AiMem 使用顶部语言切换链接。

[![crates.io: aimem-core](https://img.shields.io/crates/v/aimem-core)](https://crates.io/crates/aimem-core)
[![crates.io: aimem-cli](https://img.shields.io/crates/v/aimem-cli)](https://crates.io/crates/aimem-cli)
[![crates.io: aimem-mcp](https://img.shields.io/crates/v/aimem-mcp)](https://crates.io/crates/aimem-mcp)

AiMem 是面向 AI agent 的 Rust-first 本地记忆基础设施。

它把长期记忆放进一个单独的 Turso 数据库文件里，并暴露三层交付形态：

- `aimem-core` — 存储、挖掘、搜索、memory layers、知识图谱
- `aimem` — CLI
- `aimem-mcp` — stdio MCP server

## 0.3.x 实际新增了什么

`0.3.x` 这一代的真实改进包括：

- async embedding flow
- `LocalEmbedder` 与 opt-in `Gemini2Embedder`
- 多模态 `ContentPart`
- embedding store profile：
  - provider
  - model
  - dimension
- 阻止混库的 profile guard
- `Drawer` helper API
- `MemoryStack::file_text(...)`
- CLI / MCP status 暴露 embedding profile
- 收窄后的 extractor 与多语言回归测试
- CI 中的 `cargo audit`

## 工作区结构

```text
crates/
├── aimem-core/
├── aimem-cli/
└── aimem-mcp/
```

## 核心特点

- 单一本地 Turso DB：`~/.aimem/aimem.db`
- 关键词 + 语义检索
- 项目挖掘与会话导入
- 4 层 wake-up memory stack
- 多模态内容模型
- 默认本地 embedding
- 可选远程 Gemini embedding
- 面向 agent tooling 的 MCP 接入
- 仓库内不再依赖 Python runtime

## Embedding 模式

### Local

- `LocalEmbedder`
- 基于 `fastembed`
- 默认推荐
- embedding 本地生成

### Remote

- `Gemini2Embedder`
- 需要显式 opt-in
- 只发送你显式提供的数据

安全边界：

- URI-only 的多媒体 part **不会**自动读取本地文件再上传。
- Remote embedding 只接受显式文本、data URI 或 raw bytes。

## Store 兼容性保护

AiMem 会把 embedding profile 元数据写入 store，并在写入和语义查询时校验：

- provider
- model
- dimension

所以本地 `384d` 和远程 `768d` embedding 切换时，不会静默混库，而是直接失败。

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

先准备一个极小的 `aimem.yaml`：

```yaml
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

查询：

```bash
aimem status
aimem search "why did we choose Turso?"
aimem wake-up
```

如果要启用 remote embedding：

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
aimem search "vector search"
aimem mine /path/to/project --no-embed
```

补充说明：

- `aimem status` 会显示当前 DB 的 embedding profile。
- `aimem search` / `aimem mine` 可通过 `--gemini-key` 或 `GEMINI_API_KEY` 启用 remote embedding。
- 项目挖掘要求目标根目录里有 `aimem.yaml`。

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

`aimem_status` 也会返回当前 embedding profile。

## 配置

默认本地路径：

- database: `~/.aimem/aimem.db`
- identity: `~/.aimem/identity.txt`

环境变量覆盖：

- `AIMEM_DB_PATH`
- `AIMEM_IDENTITY_PATH`
- `GEMINI_API_KEY`
