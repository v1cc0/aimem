# aimem-cli

**Language / 语言 / 言語:** [English](https://github.com/v1cc0/aimem/blob/main/crates/aimem-cli/README.md) | [简体中文](https://github.com/v1cc0/aimem/blob/main/crates/aimem-cli/README.zh-CN.md) | 日本語

`aimem-cli` は AiMem の `aimem` コマンドラインフロントエンドです。

## できること

- プロジェクトマイニング
- 会話インポート
- キーワード検索
- セマンティック検索
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
aimem search "vector search"
aimem mine /path/to/project --no-embed
```

## Remote embedding

```bash
export GEMINI_API_KEY=...
aimem search "vector search" --gemini-key "$GEMINI_API_KEY"
```
