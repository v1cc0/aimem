# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added
- Added `aimem_core::prelude::*` as a curated high-level import surface.
- Updated official examples and top-level docs to demonstrate the `prelude`-first integration path.

### Changed
- Tightened the recommended high-level integration surface from “root imports only” to “`prelude` or explicit root imports”.

## [0.2.0] - 2026-04-08

### Added
- README quick start with minimal CLI and Rust embedding examples.
- README architecture diagram for the core/cli/mcp split.

### Changed
- Renamed the public database handle from `PalaceDb` to `AimemDb`.
- Renamed `PalaceGraph` to `AimemGraph`.
- Removed legacy compatibility paths around `palace.db`, `AIMEM_PALACE_PATH`, and the CLI `--palace` flag.
- Explicit `rust-version = 1.85` for the 2024 edition workspace.
- Documented that `aimem_core::{...}` root re-exports are the preferred stable API surface.
- Tightened top-level README wording for a more production-facing project description.

## [0.1.0] - 2026-04-08

### Added
- Initial Rust workspace MVP with `aimem-core`, `aimem` CLI, and `aimem-mcp`.
- GitHub Actions CI plus tag-triggered GitHub release automation.

### Changed
- New default local database path is `~/.aimem/aimem.db`.

### Fixed
- Existing `~/.aimem/palace.db` is still auto-detected so older local installs keep working.
