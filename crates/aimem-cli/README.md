# aimem-cli

**Language / 语言 / 言語:** English | [简体中文](https://github.com/v1cc0/aimem/blob/main/crates/aimem-cli/README.zh-CN.md) | [日本語](https://github.com/v1cc0/aimem/blob/main/crates/aimem-cli/README.ja.md)

`aimem-cli` provides the `aimem` command-line interface for AiMem.

## What the current CLI supports

- project mining
- conversation import
- hybrid keyword + vector search
- wake-up context generation
- store status inspection
- opt-in remote embedding

## Install

```bash
cargo install aimem-cli
```

## Usage

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

## Notes

- `aimem status` shows the current embedding profile.
- `aimem search` and `aimem mine` can use `--gemini-key` or `GEMINI_API_KEY`.
- `aimem search` uses hybrid keyword + vector ranking when an embedder is available.
- Keyword-only fallback now includes Unicode/CJK/Kana n-gram scoring for Chinese and Japanese queries.
- default DB path is `~/.aimem/aimem.db`; Turso may create `.db-wal` / `.db-tshm` sidecars next to it.
