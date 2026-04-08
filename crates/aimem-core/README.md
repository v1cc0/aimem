# aimem-core

`aimem-core` 是 AiMem 的 Rust 核心库：负责把记忆存进一个 Turso 数据库里，并提供项目挖掘、会话导入、关键词/语义搜索、知识图谱和 wake-up context 这些基础能力。

它是**库**，不是 CLI。CLI 在 `aimem-cli`，MCP server 在 `aimem-mcp`。

## 0.3.x 关键能力

当前这一代 `aimem-core` 已经不只是最早的“文本 + 本地 embedding”版本，而是包含了这些 0.3.x 能力：

- async embedding flow
- `LocalEmbedder`
- opt-in `Gemini2Embedder`
- 多模态 `ContentPart`
- embedding profile 元数据：
  - provider
  - model
  - dimension
- profile guard，防止混库
- `Drawer` helper API
- `MemoryStack::file_text(...)`
- 收窄后的 extractor 规则与多语言回归测试

## 安装

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
```

```bash
cargo add aimem-core
```

`aimem-core` 自己会依赖 Turso；你通常不需要手工再加一份 `turso`，除非你要直接操作底层连接。

## API 稳定性

`aimem-core` 建议优先使用两种高层入口：

### 1. `aimem_core::prelude::*`

```rust
use aimem_core::prelude::*;
```

适合快速接入、示例代码、内部工具和绝大多数应用层集成。

### 2. crate 根导出

```rust
use aimem_core::{Config, AimemDb, Embedder, Miner, ConvoMiner, Searcher, KnowledgeGraph, MemoryStack};
```

适合你想显式控制导入面的时候。

不要直接依赖更深层的模块路径，例如：

```rust
use aimem_core::miner::Miner;
```

原因很简单：直接依赖更深层模块路径会把你的代码绑到更宽的 API 面上。新接入方最好把依赖面收敛到 `prelude`、crate 根 re-export 和少数核心数据类型上。

## 什么时候该用它

- 你想把 AiMem 嵌入自己的 Rust agent / daemon / backend
- 你想直接控制 drawer、entity、triple 的落库方式
- 你想自己实现 UI、CLI 或 MCP server，但复用现成的存储与搜索逻辑

## 核心概念

- **Store**：整个 AiMem 记忆库，一个 Turso DB 文件
- **Wing**：项目或领域，例如 `my_app`
- **Room**：主题，例如 `backend`、`decisions`
- **Drawer**：一段原文切片
- **ContentPart**：drawer 的多模态部件（text / image / audio / video）
- **L0/L1/L2/L3**：唤醒文本、核心故事、按需回忆、深度搜索

## Embedding 模式

### Local

- `LocalEmbedder`
- 默认推荐
- 本地生成 embedding

### Remote

- `Gemini2Embedder`
- 明确 opt-in
- 只会发送你显式提供的文本 / data URI / raw bytes

安全边界：

- URI-only 的多媒体 part 不会自动读本地文件再上传。

## Store compatibility guard

`aimem-core` 会把 embedding profile 记录到 store 里，并在写入和语义查询时校验：

- provider
- model
- dimension

这样可以防止：

- local `384d` 和 remote `768d` 混在一个库里
- 同维度但不同模型静默混用

## 用法总览

### 1. 打开/创建一个 AiMem DB

```rust,no_run
use aimem_core::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = AimemDb::open("./aimem.db").await?;
    println!("drawers = {}", db.drawer_count().await?);
    Ok(())
}
```

### 2. 手工写入 drawer（不生成 embedding 也可以）

```rust,no_run
use aimem_core::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = AimemDb::memory().await?;

    let drawer = Drawer::new(
        "drawer_demo_001",
        "demo_app",
        "decisions",
        "We chose Rust and Turso for the memory layer.",
        "example",
    ).with_source_file("DECISIONS.md");

    db.insert_drawer(&drawer, None).await?;
    Ok(())
}
```

你也可以用 builder 风格继续补字段：

```rust,no_run
use aimem_core::prelude::*;

let drawer = Drawer::new("id", "wing", "room", "content", "agent")
    .with_source_file("README.md")
    .with_chunk_index(3);
```

### 3. 只做关键词搜索（离线、无 embedding、最稳）

如果你导入时用了 `Miner::new(db, None)`，那 drawer 没有 embedding，但仍然可以做关键词搜索。

```rust,no_run
use aimem_core::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = AimemDb::memory().await?;
    let drawer = Drawer::new(
        "drawer_demo_001",
        "demo_app",
        "backend",
        "The backend uses Turso and Rust.",
        "example",
    );
    db.insert_drawer(&drawer, None).await?;

    let searcher = Searcher::keyword_only(db);
    let hits = searcher.keyword_search("Turso", Some("demo_app"), None, 5).await?;
    assert_eq!(hits.len(), 1);
    Ok(())
}
```

### 4. 做语义搜索（需要 embedding 模型）

第一次 `LocalEmbedder::new()` 会下载 `all-MiniLM-L6-v2` 模型缓存。

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

### 4.1 手工写入时的 profile-aware API

如果你自己生成了 embedding，并且想把 profile 元数据一起写进 store，用：

```rust,no_run
use std::sync::Arc;
use aimem_core::prelude::*;

# #[tokio::main]
# async fn main() -> anyhow::Result<()> {
let db = AimemDb::memory().await?;
let embedder = Arc::new(LocalEmbedder::new()?);
let embedding = embedder.embed_one("hello memory").await?;

let drawer = Drawer::new("d1", "demo", "general", "hello memory", "example");
db.insert_drawer_with_profile(
    &drawer,
    Some(&embedding),
    embedder.provider_name(),
    embedder.model_name(),
).await?;
# Ok(())
# }
```

### 5. 挖掘一个项目目录

项目根目录需要 `aimem.yaml`：

```yaml
wing: demo_app
rooms:
  - name: backend
    description: backend code and docs
    keywords: [router, handler, database]
  - name: decisions
    description: design decisions
    keywords: [decided, chose, tradeoff]
```

然后：

```rust,no_run
use aimem_core::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = AimemDb::open("./aimem.db").await?;

    // None = 只入文本，不生成 embedding
    let miner = Miner::new(db, None);
    let stats = miner.mine("./demo-app", None, "example", 0, false).await?;

    println!("drawers added = {}", stats.drawers_added);
    Ok(())
}
```

### 6. 导入对话导出文件

```rust,no_run
use aimem_core::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = AimemDb::open("./aimem.db").await?;
    let miner = ConvoMiner::new(db, None);

    let stats = miner
        .mine("./chat-exports", "team_memory", "conversations", "example", 0, false)
        .await?;

    println!("conversation drawers = {}", stats.drawers_added);
    Ok(())
}
```

### 7. 生成 wake-up context

```rust,no_run
use std::sync::Arc;
use aimem_core::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Config::load()?;
    let db = AimemDb::open(&cfg.db_path).await?;
    let embedder = Arc::new(LocalEmbedder::new()?);
    let stack = MemoryStack::new(db, embedder, &cfg);

    let wakeup = stack.wake_up(None).await?;
    println!("{wakeup}");
    Ok(())
}
```

### 7.1 直接 filing 文本

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

### 8. 知识图谱

```rust,no_run
use aimem_core::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = AimemDb::memory().await?;
    let kg = KnowledgeGraph::new(db);

    kg.add_triple("Alice", "works_on", "AiMem", Some("2026-01-01"), None).await?;
    let facts = kg.query_entity("Alice", None, "outgoing").await?;

    assert_eq!(facts.len(), 1);
    Ok(())
}
```

## Example 程序

这个 crate 附带了可编译示例：

- `cargo run -p aimem-core --example basic_aimem`
- `cargo run -p aimem-core --example semantic_search`
- `cargo run -p aimem-core --example knowledge_graph`

## 测试

```bash
cargo test -p aimem-core
cargo test -p aimem-core --test usage_flows
cargo test -p aimem-core --test performance_smoke -- --ignored
```

## 实用注意事项

- `AimemDb::open()` 会自动建表。
- `Miner::new(db, None)` / `ConvoMiner::new(db, None)` 适合离线、无模型、先把文本塞进去。
- 没有 embedding 的 drawer 依然可以被 `keyword_search()` 找到，但不会出现在 `vector_search()` 结果里。
- `AimemDb::embedding_profile()` 可以查看当前库已经绑定到什么 embedding profile。
- `aimem-core` 会拒绝 provider / model / dimension 不匹配的语义写入与查询。
- 如果你要做服务端封装，优先把 `AimemDb` clone 出去复用；它是轻量句柄。
