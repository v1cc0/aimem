# Contributing to aimem

## Development

```bash
git clone https://github.com/v1cc0/aimem.git
cd aimem
cargo build
cargo test
```

## Workspace

- `crates/aimem-core` — reusable Rust library
- `crates/aimem-cli` — CLI binary
- `crates/aimem-mcp` — MCP server binary

## Before opening a PR

```bash
cargo fmt
cargo test
cargo test -p aimem-core --test performance_smoke -- --ignored
```

## Release flow

1. Update `CHANGELOG.md`.
2. Bump workspace/package versions if needed.
3. Create a tag like `v0.1.0`.
4. Push the tag to trigger `.github/workflows/release.yml`.

## Principles

- local-first
- reproducible behavior
- minimal dependencies
- exact storage before summarization
- keep CLI/MCP logic thin and push reusable behavior into `aimem-core`
