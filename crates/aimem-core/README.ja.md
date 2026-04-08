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
