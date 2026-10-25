# aimem-core

**Language / 语言 / 言語:** [English](https://github.com/v1cc0/aimem/blob/main/crates/aimem-core/README.md) | [简体中文](https://github.com/v1cc0/aimem/blob/main/crates/aimem-core/README.zh-CN.md) | 日本語

`aimem-core` は AiMem の Rust コアライブラリです。

提供機能：

- Turso ベースの保存
- ファイル backed store の Turso multiprocess WAL coordination（`.db-tshm` sidecar）
- プロジェクトマイニングと会話インポート
- hybrid キーワード + ベクトル検索
- memory layers / wake-up context
- 知識グラフ
- embedding profile guard
- マルチモーダル `ContentPart`
- 中国語 / 日本語検索向けの Unicode / CJK / Kana keyword fallback
- リポジトリ内の再現可能なテキストおよび deterministic multimodal truth benchmark

## 0.5.0 への更新：破壊的変更

- **既定の embedding モデルを変更：** ローカル MiniLM `384d` / fastembed を EmbeddingGemma 2 ONNX Q8 `768d` に置き換えました。旧ベクトルやリモート Gemini のベクトルは新モデルの store と混在できません。書き込みを停止し、旧 DB と WAL sidecar をバックアップしてから、元のテキストを別の DB に再取り込みしてください。profile を書き換えて互換性チェックを回避しないでください。ベクトルの自動変換は行いません。
- **Rust 1.95 以降が必要です。** 初回のローカル実行で約 314 MB の重みと tokenizer を取得し、以後はキャッシュを使ってオフライン実行できます。
- **Turso 0.8.3：** Turso に直接依存する下流アプリは、型の不一致や native symbol の重複を避けるためバージョンを揃えてください。
- **Rust API：** `EmbedError::Fastembed` の match を `EmbedError::Local` に更新してください。ローカル provider/model/dimension 定数も変更され、手動の検索ベクトル生成には `embed_query` を使います。

[リリースノートと移行手順](https://github.com/v1cc0/aimem/blob/v0.5.0/CHANGELOG.md).

## Current capabilities

- async embedding flow
- `LocalEmbedder`
- ONNX Runtime で [`onnx-community/embeddinggemma-2-ONNX`](https://huggingface.co/onnx-community/embeddinggemma-2-ONNX) の Q8 テキストモデルを実行し、正規化された `768d` ベクトルを生成
- 初回に約 314 MB の重みと tokenizer を Hugging Face cache に取得（`HF_HOME` / `HF_HUB_CACHE` 対応）。取得後はオフライン実行可能
- ローカルではテキストのみ処理し、multimodal input からのテキスト抽出は従来どおり。最大 8,192 token
- `embed` / `embed_one` は文書 prefix、`embed_query` は検索 prefix を付加。`Searcher` は query API を使用
- opt-in `Gemini2Embedder`
- embedding profile: provider / model / dimension
- mixed store を防ぐ profile guard
- `Drawer` helper API
- `MemoryStack::file_text(...)`
- `MemoryStack::file_drawer_with_id(...)`
- `MemoryStack::file_drawers_with_ids(...)`
- Turso FTS + RRF ベースの hybrid 検索
- embedding なし検索向けの CJK / 日本語 keyword fallback
- 再現可能な EN/ZH/JA benchmark fixtures と result snapshots
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
- ONNX Runtime で [`onnx-community/embeddinggemma-2-ONNX`](https://huggingface.co/onnx-community/embeddinggemma-2-ONNX) の Q8 テキストモデルを実行し、正規化された `768d` ベクトルを生成
- 初回に約 314 MB の重みと tokenizer を Hugging Face cache に取得（`HF_HOME` / `HF_HUB_CACHE` 対応）。取得後はオフライン実行可能
- ローカルではテキストのみ処理し、multimodal input からのテキスト抽出は従来どおり。最大 8,192 token
- `embed` / `embed_one` は文書 prefix、`embed_query` は検索 prefix を付加。`Searcher` は query API を使用
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

旧 MiniLM の `384d`、ローカル EmbeddingGemma 2 の `768d`、リモート Gemini の `768d` は混在できません。移行時は元のテキストから別の DB を再作成してください。既存 DB の自動変換や削除は行いません。

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

## Hybrid 検索

`Searcher::hybrid_search(...)` は Turso FTS のキーワード面と vector 検索面を RRF で融合します。

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
