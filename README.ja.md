# AiMem

**[English](https://github.com/v1cc0/aimem/blob/main/README.md) | [简体中文](https://github.com/v1cc0/aimem/blob/main/README.zh-CN.md) | 日本語**

[![crates.io: aimem-core](https://img.shields.io/crates/v/aimem-core)](https://crates.io/crates/aimem-core)
[![crates.io: aimem-cli](https://img.shields.io/crates/v/aimem-cli)](https://crates.io/crates/aimem-cli)
[![crates.io: aimem-mcp](https://img.shields.io/crates/v/aimem-mcp)](https://crates.io/crates/aimem-mcp)

AiMem は AI エージェント向けの Rust-first ローカルメモリ基盤です。

長期記憶を単一の Turso データベースに保存し、以下を提供します：

- `aimem-core` — 保存、マイニング、検索、メモリレイヤー、知識グラフ
- `aimem` — CLI
- `aimem-mcp` — stdio MCP サーバー

## Workspace layout

```text
crates/
├── aimem-core/
├── aimem-cli/
└── aimem-mcp/
```

## Highlights

- 単一のローカル Turso DB ファイル：`~/.aimem/aimem.db`
- ファイル backed store は Turso multiprocess WAL coordination を使い、`.db-wal` / `.db-tshm` sidecar を作成することがあります
- hybrid キーワード + ベクトル検索
- embedding なし検索向けの CJK / 日本語 keyword fallback
- 再現可能な memory benchmark
- プロジェクトマイニングと会話インポート
- 4-layer wake-up memory stack
- マルチモーダル content model
- デフォルトはローカル embedder
- opt-in の Gemini remote embedding
- エージェント向け MCP 統合
- ローカル AiMem DB 上の private Codex coding-experience tools
- この repository に Python runtime は不要

## Embedding modes

### Local

- `LocalEmbedder`
- `fastembed` ベース
- デフォルト推奨
- embedding はローカルで生成

### Remote

- `Gemini2Embedder`
- 明示的 opt-in
- 明示的に渡したデータだけを送信

重要な安全境界：

- URI-only の multimodal part からローカルファイルを自動読み込みしてアップロードすることはありません。
- Remote embedding は明示的な text、data URI、raw bytes のみを受け取ります。

## Store compatibility guard

AiMem は embedding profile metadata を store に記録し、mixed store を拒否します。

書き込みと semantic query は以下を検証します：

- provider
- model
- dimension

そのため local `384d` embedding から remote `768d` embedding に切り替えた場合、同じ DB で静かに検索品質を落とすのではなく、早期に失敗します。

添付ファイル系の取り込みでは、`MemoryStack::file_drawers_with_ids(...)` で呼び出し側が用意した stable-ID drawers をまとめて filing できます。下流アプリは 1 ファイル分の summary + chunk drawers を 1 回の embedding call にまとめられ、retry 時には既存 ID を embedding 前に skip できます。

## Install

CLI:

```bash
cargo install aimem-cli
```

MCP server:

```bash
cargo install aimem-mcp
```

Library:

```bash
cargo add aimem-core
```

## Quick start

小さな project config を作成します：

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

プロジェクトをローカルメモリに取り込みます：

```bash
aimem mine /path/to/project --no-embed
```

保存内容を検索します：

```bash
aimem status
aimem search "why did we choose Turso?"
aimem wake-up
```

Remote embedding を使う場合：

```bash
export GEMINI_API_KEY=...
aimem search "why did we choose Turso?" --gemini-key "$GEMINI_API_KEY"
```

## Minimal Rust example

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

Useful notes:

- `aimem status` は DB に保存された現在の embedding profile を表示します。
- `aimem search` / `aimem mine` は `--gemini-key` または `GEMINI_API_KEY` で opt-in remote embedding を使えます。
- embedder が利用可能な場合、`aimem search` は hybrid キーワード + ベクトル ranking を使います。
- project mining は対象 project root に `aimem.yaml` が必要です。

## MCP

```bash
aimem-mcp
```

Current tools:

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

`aimem_status` も現在の embedding profile を返します。

`codex_*` tools は Codex 系 agent 向けの private/local coding-experience tools です。編集済み repo profile、compact reusable experience card、task-focused context を同じ AiMem DB に保存します。`codex_record_repo` は明示された `repo_path` の周辺だけを保守的に検出します：repo root、既知 manifest、likely test commands、`.git` HEAD/origin。`codex_record_command` は verification / diagnostic command を記録し、`codex_record_round_summary` は handoff summary、changed files、commands、next steps を記録します。`codex_context` は same-repo、incident、same-language、keyword-overlap boost で関連カードを ranking します。experience card は repo/kind/problem/solution に基づく stable fingerprint で重複検出します。完全な replay は skip されますが、command result や round summary が変わった場合は新しい evidence card として記録されます。`codex_delete_experience` は drawer ID で private Codex card を削除します。明示的な設計であり、filesystem crawler、会話 transcript の自動取り込み、secret 収集は行いません。

- Private Codex MCP smoke test: [`docs/private-codex-mcp-smoke-test.md`](https://github.com/v1cc0/aimem/blob/main/docs/private-codex-mcp-smoke-test.md)

## Config

Default local paths:

- database: `~/.aimem/aimem.db`
- identity: `~/.aimem/identity.txt`

Environment overrides:

- `AIMEM_DB_PATH`
- `AIMEM_IDENTITY_PATH`
- `GEMINI_API_KEY`

## Repository

- repo: `https://github.com/v1cc0/aimem`
- license: MIT
- Inspired by https://github.com/milla-jovovich/mempalace
- Issues are welcome. AI-generated PRs may be ignored.
