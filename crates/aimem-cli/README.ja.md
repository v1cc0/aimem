# aimem-cli

**Language / 语言 / 言語:** [English](https://github.com/v1cc0/aimem/blob/main/crates/aimem-cli/README.md) | [简体中文](https://github.com/v1cc0/aimem/blob/main/crates/aimem-cli/README.zh-CN.md) | 日本語

`aimem-cli` は AiMem の `aimem` コマンドラインフロントエンドです。

## できること

- プロジェクトマイニング
- 会話インポート
- hybrid キーワード + ベクトル検索
- wake-up context 生成
- store status 表示
- opt-in remote embedding

## インストール

```bash
cargo install aimem-cli
```

## 利用例

```bash
aimem status
aimem wake-up
aimem search "hybrid search"
aimem mine /path/to/project --no-embed
```

## Remote embedding

```bash
export GEMINI_API_KEY=...
aimem search "hybrid search" --gemini-key "$GEMINI_API_KEY"
```

## メモ

- `aimem search` は embedder が使える場合に hybrid キーワード + ベクトル順位付けを使います。
- keyword-only fallback は中国語 / 日本語クエリ向けの Unicode / CJK / Kana n-gram scoring を含みます。
- デフォルト DB は `~/.aimem/aimem.db` で、Turso が隣に `.db-wal` / `.db-tshm` sidecar を作成することがあります。
