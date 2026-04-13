# aimem-core

**Language / 语言 / 言語:** [English](https://github.com/v1cc0/aimem/blob/main/crates/aimem-core/README.md) | [简体中文](https://github.com/v1cc0/aimem/blob/main/crates/aimem-core/README.zh-CN.md) | 日本語

`aimem-core` は AiMem の Rust コアライブラリです。

提供機能：

- Turso ベースの保存
- プロジェクトマイニングと会話インポート
- キーワード / セマンティック検索
- memory layers / wake-up context
- 知識グラフ
- embedding profile guard
- マルチモーダル `ContentPart`

## 0.3.x の主な点

- async embedding flow
- `LocalEmbedder`
- opt-in `Gemini2Embedder`
- embedding profile: provider / model / dimension
- mixed store を防ぐ profile guard
- `Drawer` helper API
- `MemoryStack::file_text(...)`
- `MemoryStack::file_drawer_with_id(...)`
- `MemoryStack::file_drawers_with_ids(...)`
- extractor の回帰テスト強化

## インストール

```bash
cargo add aimem-core
```

## 推奨 API 面

```rust
use aimem_core::prelude::*;
```

## Embedding モード

### Local
- `LocalEmbedder`
- デフォルト推奨

### Remote
- `Gemini2Embedder`
- 明示的 opt-in
- 明示的に渡したデータのみ送信

URI-only の multimedia part からローカルファイルを自動アップロードすることはありません。

## Store 互換性保護

semantic write / query は以下で検証されます：

- provider
- model
- dimension

## Drawer helper

```rust,no_run
use aimem_core::prelude::*;

let drawer = Drawer::new("id", "wing", "room", "content", "agent")
    .with_source_file("README.md");
```

`MemoryStack` 経由の stable-ID filing:

```rust,no_run
use std::sync::Arc;
use aimem_core::prelude::*;

# #[tokio::main]
# async fn main() -> anyhow::Result<()> {
let cfg = Config::load()?;
let db = AimemDb::open(&cfg.db_path).await?;
let embedder = Arc::new(LocalEmbedder::new()?);
let stack = MemoryStack::new(db, embedder, &cfg);

let inserted = stack
    .file_drawer_with_id(
        "attachment.chunk.001",
        "attachments",
        "user-123",
        "Attachment chunk body".to_string(),
        vec![ContentPart::text("Attachment chunk body")],
        Some("report.pdf"),
        1,
        "example",
    )
    .await?;

println!("inserted={inserted}");
# Ok(())
# }
```

関連する stable-ID drawer 群（たとえば同じ添付ファイルの summary + chunk
drawers）がすでにあるなら、1 回でまとめて filing できます：

```rust,no_run
use std::sync::Arc;
use aimem_core::prelude::*;

# #[tokio::main]
# async fn main() -> anyhow::Result<()> {
let cfg = Config::load()?;
let db = AimemDb::open(&cfg.db_path).await?;
let embedder = Arc::new(LocalEmbedder::new()?);
let stack = MemoryStack::new(db, embedder, &cfg);

let inserted = stack
    .file_drawers_with_ids(&[
        DrawerFilingRequest::new(
            "attachment.summary.001",
            "attachments",
            "user-123",
            "Attachment summary".to_string(),
            "example",
        )
        .with_parts(vec![ContentPart::text("Attachment summary")])
        .with_source_file("report.pdf"),
        DrawerFilingRequest::new(
            "attachment.chunk.001",
            "attachments",
            "user-123",
            "Attachment chunk 1".to_string(),
            "example",
        )
        .with_parts(vec![ContentPart::text("Attachment chunk 1")])
        .with_source_file("report.pdf")
        .with_chunk_index(1),
    ])
    .await?;

println!("inserted={inserted:?}");
# Ok(())
# }
```

AiMem は embedding 前に既存 stable ID を確認するため、同じバッチを
再試行しても、すでに保存済みの drawer に対して余計な remote embedding 呼び出しを行いません。

## 直接 text を filing

```rust,no_run
use std::sync::Arc;
use aimem_core::prelude::*;

# #[tokio::main]
# async fn main() -> anyhow::Result<()> {
let cfg = Config::load()?;
let db = AimemDb::open(&cfg.db_path).await?;
let embedder = Arc::new(LocalEmbedder::new()?);
let stack = MemoryStack::new(db, embedder, &cfg);
let id = stack.file_text("demo", "notes", "remember this", "example").await?;
println!("{id}");
# Ok(())
# }
```
