# aimem-cli

`aimem-cli` provides the `aimem` command-line interface for AiMem.

It is the practical frontend for:

- project mining
- conversation import
- keyword search
- semantic search
- wake-up context generation
- store status inspection

## Install

```bash
cargo install aimem-cli
```

## What the CLI reflects in 0.3.x

The current CLI already supports the newer 0.3.x behavior:

- async embedding flow
- local embedding by default via `LocalEmbedder`
- opt-in remote embedding via `Gemini2Embedder`
- status output includes embedding profile
- mining / search support `--gemini-key` or `GEMINI_API_KEY`

## Usage

```bash
aimem status
aimem wake-up
aimem search "vector search"
aimem mine /path/to/project --no-embed
```

### Remote embedding

```bash
export GEMINI_API_KEY=...
aimem search "vector search" --gemini-key "$GEMINI_API_KEY"
```

Or rely on the environment variable directly:

```bash
aimem search "vector search"
```

when `GEMINI_API_KEY` is already set.

## Common commands

### Show store status

```bash
aimem status
```

Includes:

- DB path
- drawer count
- identity presence
- wings / rooms counts
- embedding profile

### Mine a project

```bash
aimem mine /path/to/project --no-embed
```

With remote embedding:

```bash
aimem mine /path/to/project --gemini-key "$GEMINI_API_KEY"
```

### Mine conversation exports

```bash
aimem mine ./chat-exports --mode convos --wing team_memory --room conversations
```

### Search

```bash
aimem search "why did we choose Turso?"
```

The CLI prefers semantic search when an embedder is available, and otherwise falls back to keyword search.

## Defaults

Default local database path:

- `~/.aimem/aimem.db`

## Environment

- `AIMEM_DB_PATH`
- `AIMEM_IDENTITY_PATH`
- `GEMINI_API_KEY`
