# aimem-core

`aimem-core` 是 AiMem 的 Rust 核心库：负责把记忆存进一个 Turso 数据库里，并提供项目挖掘、会话导入、关键词/语义搜索、知识图谱和 wake-up context 这些基础能力。

它是**库**，不是 CLI。CLI 在 `aimem-cli`，MCP server 在 `aimem-mcp`。

## 安装

```toml
[dependencies]
aimem-core = "0.1.0"
tokio = { version = "1", features = ["full"] }
```

`aimem-core` 自己会依赖 Turso；你通常不需要手工再加一份 `turso`，除非你要直接操作底层连接。

## API 稳定性（0.1.x）

`aimem-core` 在 `0.1.x` 阶段建议把 crate 根导出的类型当作稳定入口使用，也就是优先从：

```rust
use aimem_core::{Config, PalaceDb, Embedder, Miner, ConvoMiner, Searcher, KnowledgeGraph, MemoryStack};
```

来接入，而不是直接依赖更深层的模块路径。

原因很简单：`0.1.0` 已经把一些内部 helper 也暴露成了 public，这意味着它们现在不能在小版本里随便拿掉，否则会直接破坏 userspace。后续版本会继续兼容这批已公开 API，但新接入方最好把依赖面收敛到 crate 根 re-export 和少数核心数据类型上。

## 什么时候该用它

- 你想把 AiMem 嵌入自己的 Rust agent / daemon / backend
- 你想直接控制 drawer、entity、triple 的落库方式
- 你想自己实现 UI、CLI 或 MCP server，但复用现成的存储与搜索逻辑

## 核心概念

- **Palace**：整个记忆库，一个 Turso DB 文件
- **Wing**：项目或领域，例如 `my_app`
- **Room**：主题，例如 `backend`、`decisions`
- **Drawer**：一段原文切片
- **L0/L1/L2/L3**：唤醒文本、核心故事、按需回忆、深度搜索

## 用法总览

### 1. 打开/创建一个 palace

```rust,no_run
use aimem_core::PalaceDb;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = PalaceDb::open("./aimem.db").await?;
    println!("drawers = {}", db.drawer_count().await?);
    Ok(())
}
```

### 2. 手工写入 drawer（不生成 embedding 也可以）

```rust,no_run
use aimem_core::{Drawer, PalaceDb};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = PalaceDb::memory().await?;

    let drawer = Drawer {
        id: "drawer_demo_001".into(),
        wing: "demo_app".into(),
        room: "decisions".into(),
        content: "We chose Rust and Turso for the memory layer.".into(),
        source_file: Some("DECISIONS.md".into()),
        chunk_index: 0,
        added_by: "example".into(),
        filed_at: chrono::Utc::now().to_rfc3339(),
    };

    db.insert_drawer(&drawer, None).await?;
    Ok(())
}
```

### 3. 只做关键词搜索（离线、无 embedding、最稳）

如果你导入时用了 `Miner::new(db, None)`，那 drawer 没有 embedding，但仍然可以做关键词搜索。

```rust,no_run
use aimem_core::{Drawer, PalaceDb, Searcher};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = PalaceDb::memory().await?;
    let drawer = Drawer {
        id: "drawer_demo_001".into(),
        wing: "demo_app".into(),
        room: "backend".into(),
        content: "The backend uses Turso and Rust.".into(),
        source_file: None,
        chunk_index: 0,
        added_by: "example".into(),
        filed_at: chrono::Utc::now().to_rfc3339(),
    };
    db.insert_drawer(&drawer, None).await?;

    let searcher = Searcher::keyword_only(db);
    let hits = searcher.keyword_search("Turso", Some("demo_app"), None, 5).await?;
    assert_eq!(hits.len(), 1);
    Ok(())
}
```

### 4. 做语义搜索（需要 embedding 模型）

第一次 `Embedder::new()` 会下载 `all-MiniLM-L6-v2` 模型缓存。

```rust,no_run
use aimem_core::{Drawer, Embedder, PalaceDb, Searcher};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = PalaceDb::memory().await?;
    let embedder = Embedder::new()?;

    let content = "We moved the memory backend to Turso for local vector search.";
    let embedding = embedder.embed_one(content)?;

    let drawer = Drawer {
        id: "drawer_demo_001".into(),
        wing: "demo_app".into(),
        room: "backend".into(),
        content: content.into(),
        source_file: None,
        chunk_index: 0,
        added_by: "example".into(),
        filed_at: chrono::Utc::now().to_rfc3339(),
    };
    db.insert_drawer(&drawer, Some(&embedding)).await?;

    let searcher = Searcher::new(db, embedder);
    let hits = searcher.vector_search("local vector database", Some("demo_app"), None, 5).await?;
    assert!(!hits.is_empty());
    Ok(())
}
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
use aimem_core::{Miner, PalaceDb};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = PalaceDb::open("./aimem.db").await?;

    // None = 只入文本，不生成 embedding
    let miner = Miner::new(db, None);
    let stats = miner.mine("./demo-app", None, "example", 0, false).await?;

    println!("drawers added = {}", stats.drawers_added);
    Ok(())
}
```

### 6. 导入对话导出文件

```rust,no_run
use aimem_core::{convo::ConvoMiner, PalaceDb};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = PalaceDb::open("./aimem.db").await?;
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
use aimem_core::{Config, Embedder, MemoryStack, PalaceDb};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Config::load()?;
    let db = PalaceDb::open(&cfg.db_path).await?;
    let embedder = Embedder::new()?;
    let stack = MemoryStack::new(db, embedder, &cfg);

    let wakeup = stack.wake_up(None).await?;
    println!("{wakeup}");
    Ok(())
}
```

### 8. 知识图谱

```rust,no_run
use aimem_core::{KnowledgeGraph, PalaceDb};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = PalaceDb::memory().await?;
    let kg = KnowledgeGraph::new(db);

    kg.add_triple("Alice", "works_on", "AiMem", Some("2026-01-01"), None).await?;
    let facts = kg.query_entity("Alice", None, "outgoing").await?;

    assert_eq!(facts.len(), 1);
    Ok(())
}
```

## Example 程序

这个 crate 附带了可编译示例：

- `cargo run -p aimem-core --example basic_palace`
- `cargo run -p aimem-core --example semantic_search`
- `cargo run -p aimem-core --example knowledge_graph`

## 测试

```bash
cargo test -p aimem-core
cargo test -p aimem-core --test usage_flows
cargo test -p aimem-core --test performance_smoke -- --ignored
```

## 实用注意事项

- `PalaceDb::open()` 会自动建表。
- `Miner::new(db, None)` / `ConvoMiner::new(db, None)` 适合离线、无模型、先把文本塞进去。
- 没有 embedding 的 drawer 依然可以被 `keyword_search()` 找到，但不会出现在 `vector_search()` 结果里。
- 如果你要做服务端封装，优先把 `PalaceDb` clone 出去复用；它是轻量句柄。
