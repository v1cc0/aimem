# aimem-core

**语言 / Language / 言語：** [English](https://github.com/v1cc0/aimem/blob/main/crates/aimem-core/README.md) | 简体中文 | [日本語](https://github.com/v1cc0/aimem/blob/main/crates/aimem-core/README.ja.md)

`aimem-core` 是 AiMem 的 Rust 核心库。

它提供：

- 基于 Turso 的存储
- 项目挖掘与会话导入
- 关键词 / 语义检索
- memory layers / wake-up context
- 知识图谱
- embedding profile guard
- 多模态内容原语

CLI 在 `aimem-cli`，MCP server 在 `aimem-mcp`。

## 0.3.x 能力

当前这一代 `aimem-core` 已包含：

- async embedding flow
- `LocalEmbedder`
- opt-in `Gemini2Embedder`
- 多模态 `ContentPart`
- embedding profile 元数据：
  - provider
  - model
  - dimension
- 防止混库的 profile guard
- `Drawer` helper API
- `MemoryStack::file_text(...)`
- 收窄后的 extractor 与多语言回归测试

## 安装

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
```

```bash
cargo add aimem-core
```

## 推荐 API 面

优先使用：

```rust
use aimem_core::prelude::*;
```

或者 crate 根导出：

```rust
use aimem_core::{AimemDb, Drawer, LocalEmbedder, MemoryStack, Miner, Searcher};
```

## 核心概念

- **Store** — 一个 AiMem DB
- **Wing** — 项目 / 领域
- **Room** — 主题
- **Drawer** — 一段记忆切片
- **ContentPart** — drawer 内的多模态部件
- **L0/L1/L2/L3** — identity、essential story、recall、deep search

## Embedding 模式

### Local

- `LocalEmbedder`
- 默认推荐
- embedding 本地生成

### Remote

- `Gemini2Embedder`
- 明确 opt-in
- 只发送显式提供的文本 / data URI / raw bytes

URI-only 的多媒体 part 不会自动读取本地文件再上传。

## Store 兼容性保护

`aimem-core` 会把 embedding profile 写入 store，并在语义写入和查询时校验：

- provider
- model
- dimension

这样可以防止不兼容 embedding 静默混库。

## Drawer helper

```rust,no_run
use aimem_core::prelude::*;

let drawer = Drawer::new("id", "wing", "room", "content", "agent")
    .with_source_file("README.md")
    .with_chunk_index(3);
```

多模态 drawer：

```rust,no_run
use aimem_core::prelude::*;

let drawer = Drawer::multimodal(
    "id",
    "wing",
    "room",
    "Look at this image",
    vec![ContentPart::text("Look at this image")],
    "agent",
);
```

## 语义搜索例子

```rust,no_run
use std::sync::Arc;
use aimem_core::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = AimemDb::memory().await?;
    let embedder = Arc::new(LocalEmbedder::new()?);

    let content = "We moved the memory backend to Turso for local vector search.";
    let embedding = embedder.embed_one(content).await?;

    let drawer = Drawer::new("drawer_demo_001", "demo_app", "backend", content, "example");
    db.insert_drawer_with_profile(
        &drawer,
        Some(&embedding),
        embedder.provider_name(),
        embedder.model_name(),
    ).await?;

    let searcher = Searcher::new(db, embedder);
    let hits = searcher.vector_search("local vector database", Some("demo_app"), None, 5).await?;
    assert!(!hits.is_empty());
    Ok(())
}
```

## 直接 filing 文本

```rust,no_run
use std::sync::Arc;
use aimem_core::prelude::*;

# #[tokio::main]
# async fn main() -> anyhow::Result<()> {
let cfg = Config::load()?;
let db = AimemDb::open(&cfg.db_path).await?;
let embedder = Arc::new(LocalEmbedder::new()?);
let stack = MemoryStack::new(db, embedder, &cfg);

let id = stack.file_text("demo_app", "notes", "remember this", "example").await?;
println!("{id}");
# Ok(())
# }
```

## 注意事项

- `AimemDb::open()` 会自动建表。
- `Miner::new(db, None)` / `ConvoMiner::new(db, None)` 适合 text-only 落库。
- 没有 embedding 的 drawer 依然能参与关键词搜索。
- `AimemDb::embedding_profile()` 可查看当前 store profile。
