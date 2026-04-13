# AiMem

**Language / 语言 / 言語:** [English](https://github.com/v1cc0/aimem/blob/main/README.md) | [简体中文](https://github.com/v1cc0/aimem/blob/main/README.zh-CN.md) | 日本語

> GitHub と crates.io にはこの場所でネイティブな README タブ機能がないため、AiMem は上部の言語切替リンクを使います。

[![crates.io: aimem-core](https://img.shields.io/crates/v/aimem-core)](https://crates.io/crates/aimem-core)
[![crates.io: aimem-cli](https://img.shields.io/crates/v/aimem-cli)](https://crates.io/crates/aimem-cli)
[![crates.io: aimem-mcp](https://img.shields.io/crates/v/aimem-mcp)](https://crates.io/crates/aimem-mcp)

AiMem は AI エージェント向けの Rust-first ローカルメモリ基盤です。

長期記憶を単一の Turso データベースに保存し、以下を提供します：

- `aimem-core` — 保存、マイニング、検索、メモリレイヤー、知識グラフ
- `aimem` — CLI
- `aimem-mcp` — stdio MCP サーバー

## 0.3.x の主な改善点

- async embedding flow
- `LocalEmbedder` と opt-in `Gemini2Embedder`
- マルチモーダル `ContentPart`
- embedding store profile：provider / model / dimension
- mixed store を拒否する profile guard
- `Drawer` helper API
- `MemoryStack::file_text(...)`
- `MemoryStack::file_drawer_with_id(...)`
- `MemoryStack::file_drawers_with_ids(...)`
- CLI / MCP status に embedding profile を表示
- より安全に絞り込んだ extractor と多言語回帰テスト
- CI の `cargo audit`

## 特徴

- 単一のローカル Turso DB：`~/.aimem/aimem.db`
- キーワード検索 + セマンティック検索
- プロジェクトマイニングと会話インポート
- 4-layer wake-up memory stack
- マルチモーダル content model
- デフォルトはローカル embedding
- opt-in の Gemini remote embedding
- エージェント向け MCP 統合

## Embedding モード

### Local

- `LocalEmbedder`
- `fastembed` ベース
- デフォルト推奨

### Remote

- `Gemini2Embedder`
- 明示的 opt-in
- 明示的に渡したデータだけを送信

安全境界：

- URI-only の multimedia part からローカルファイルを自動読み込みしてアップロードすることはありません。

## Store 互換性ガード

AiMem は embedding profile を store に記録し、書き込みとセマンティック検索時に以下を検証します：

- provider
- model
- dimension

そのため local `384d` と remote `768d` を同じ DB に静かに混ぜることはありません。

添付ファイル系の取り込みでは、
`MemoryStack::file_drawers_with_ids(...)` で呼び出し側が用意した stable-ID
drawer 群をまとめて filing できます。下流アプリは 1 ファイル分の summary +
chunk drawers を 1 バッチにして 1 回の embedding 呼び出しにまとめられ、
リトライ時には既存 ID を embedding 前にスキップできます。

## インストール

```bash
cargo install aimem-cli
cargo install aimem-mcp
cargo add aimem-core
```

## クイックスタート

```bash
aimem mine /path/to/project --no-embed
aimem status
aimem search "why did we choose Turso?"
aimem wake-up
```

Remote embedding を使う場合：

```bash
export GEMINI_API_KEY=...
aimem search "why did we choose Turso?" --gemini-key "$GEMINI_API_KEY"
```

## Rust の最小例

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
    Ok(())
}
```
