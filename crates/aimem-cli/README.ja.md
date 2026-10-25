# aimem-cli

**Language / 语言 / 言語:** [English](https://github.com/v1cc0/aimem/blob/main/crates/aimem-cli/README.md) | [简体中文](https://github.com/v1cc0/aimem/blob/main/crates/aimem-cli/README.zh-CN.md) | 日本語

`aimem-cli` は AiMem の `aimem` コマンドラインフロントエンドです。

## 0.5.0 への更新：破壊的変更

- **既定の embedding モデルを変更：** ローカル MiniLM `384d` / fastembed を EmbeddingGemma 2 ONNX Q8 `768d` に置き換えました。旧ベクトルやリモート Gemini のベクトルは新モデルの store と混在できません。書き込みを停止し、旧 DB と WAL sidecar をバックアップしてから、元のテキストを別の DB に再取り込みしてください。profile を書き換えて互換性チェックを回避しないでください。ベクトルの自動変換は行いません。
- **Rust 1.95 以降が必要です。** 初回のローカル実行で約 314 MB の重みと tokenizer を取得し、以後はキャッシュを使ってオフライン実行できます。
- **Turso 0.8.3：** Turso に直接依存する下流アプリは、型の不一致や native symbol の重複を避けるためバージョンを揃えてください。

[リリースノートと移行手順](https://github.com/v1cc0/aimem/blob/v0.5.0/CHANGELOG.md).

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
