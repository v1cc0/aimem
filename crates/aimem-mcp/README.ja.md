# aimem-mcp

**Language / 语言 / 言語:** [English](https://github.com/v1cc0/aimem/blob/main/crates/aimem-mcp/README.md) | [简体中文](https://github.com/v1cc0/aimem/blob/main/crates/aimem-mcp/README.zh-CN.md) | 日本語

`aimem-mcp` は AiMem の stdio MCP サーバーです。

## インストール

```bash
cargo install aimem-mcp
```

## 実行

```bash
aimem-mcp
```

## ツール

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
- `codex_search_experience`
- `codex_context`

## メモ

- `aimem_status` は embedding profile も返します。
- `aimem_search` は embedder が使える場合に hybrid キーワード + ベクトル検索を使います。
- keyword fallback は中国語 / 日本語クエリ向けの Unicode / CJK / Kana n-gram scoring を含みます。
- `codex_*` ツールは Codex 系 agent 向けの private coding-experience layer です。編集済み repo profile、compact experience card、task context を同じローカル AiMem DB に保存します。`codex_record_repo` は明示された `repo_path` の周辺だけを保守的に検出します（repo root、既知 manifest、likely test commands、`.git` HEAD/origin）。`codex_context` は same-repo、incident、same-language、keyword-overlap boost で関連カードを ranking します。experience card は repo/kind/problem/solution に基づく stable fingerprint で重複検出します。ファイルシステムの自動クロールや会話 transcript の自動取り込みは行いません。
- デフォルト DB は `~/.aimem/aimem.db` で、Turso が隣に `.db-wal` / `.db-tshm` sidecar を作成することがあります。
