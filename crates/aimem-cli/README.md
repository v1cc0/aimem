# aimem-cli

`aimem-cli` provides the `aimem` command-line interface for AiMem.

## Install

```bash
cargo install aimem-cli
```

## Usage

```bash
aimem status
aimem wake-up
aimem search "vector search"
aimem mine /path/to/project --no-embed
```

Default local database path: `~/.aimem/aimem.db`

Legacy compatibility:

- if `~/.aimem/palace.db` already exists, AiMem still opens it automatically
- `--palace` keeps the legacy `<DIR>/palace.db` behavior
