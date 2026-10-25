# aimem-cli

**Language / 语言 / 言語:** English | [简体中文](https://github.com/v1cc0/aimem/blob/main/crates/aimem-cli/README.zh-CN.md) | [日本語](https://github.com/v1cc0/aimem/blob/main/crates/aimem-cli/README.ja.md)

`aimem-cli` provides the `aimem` command-line interface for AiMem.

## 0.5.0 upgrade: breaking changes

- **New embedding model:** local MiniLM `384d` / fastembed is replaced by EmbeddingGemma 2 ONNX Q8 `768d`. Old vectors and remote Gemini vectors cannot share the new model's store. Stop writers, back up the old DB and its WAL sidecars, then re-embed original text into a separate database. Do not relabel stored profiles; there is no automatic vector conversion.
- **Rust 1.95+** is now required. First local use downloads about 314 MB of weights plus the tokenizer; subsequent runs can use the cache offline.
- **Turso 0.8.3:** downstream applications using Turso directly should align versions to avoid incompatible types or duplicate native symbols.

[Full release notes and migration instructions](https://github.com/v1cc0/aimem/blob/v0.5.0/CHANGELOG.md).

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
- Default DB path is `~/.aimem/aimem.db`; Turso may create `.db-wal` / `.db-tshm` sidecars next to it.
