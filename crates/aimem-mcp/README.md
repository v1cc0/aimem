# aimem-mcp

**Language / 语言 / 言語:** English | [简体中文](https://github.com/v1cc0/aimem/blob/main/crates/aimem-mcp/README.zh-CN.md) | [日本語](https://github.com/v1cc0/aimem/blob/main/crates/aimem-mcp/README.ja.md)

`aimem-mcp` is the stdio MCP server for AiMem.

## Install

```bash
cargo install aimem-mcp
```

## Run

```bash
aimem-mcp
```

## Tools

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

## Notes

- `aimem_status` reports the current embedding profile.
- `aimem_search` uses hybrid keyword + vector ranking when an embedder is available and falls back to keyword search otherwise.
- Keyword fallback includes Unicode/CJK/Kana n-gram scoring for Chinese and Japanese queries.
- The `codex_*` tools are a private coding-experience layer for Codex-style agents: they record edited repo profiles, compact experience cards, and task context into the same local AiMem DB. `codex_record_repo` performs only conservative detection around the explicit `repo_path` (repo root, known manifests, likely test commands, and `.git` HEAD/origin). They do **not** crawl your filesystem or ingest transcripts automatically.
- default DB path is `~/.aimem/aimem.db`; Turso may create `.db-wal` / `.db-tshm` sidecars next to it.
