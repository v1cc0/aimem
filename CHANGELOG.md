# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added
- README quick start with minimal CLI and Rust embedding examples.
- README architecture diagram for the core/cli/mcp split.

### Changed
- Explicit `rust-version = 1.85` for the 2024 edition workspace.
- Documented that `0.1.x` integrations should prefer `aimem_core::{...}` root re-exports as the stable API surface.
- Tightened top-level README wording for a more production-facing project description.

## [0.1.0] - 2026-04-08

### Added
- Initial Rust workspace MVP with `aimem-core`, `aimem` CLI, and `aimem-mcp`.
- GitHub Actions CI plus tag-triggered GitHub release automation.

### Changed
- New default local database path is `~/.aimem/aimem.db`.

### Fixed
- Existing `~/.aimem/palace.db` is still auto-detected so older local installs keep working.
