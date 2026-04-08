# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

## [0.3.3] - 2026-04-09

### Changed
- Added English / Simplified Chinese / Japanese README variants for the repository and all published crates.
- Added explicit language-switch links at the top of GitHub and crates.io-facing README content.
- Documented that GitHub / crates.io use README language links rather than native tabs.


## [0.3.2] - 2026-04-09

### Changed
- Refreshed top-level and crate READMEs to properly document the 0.3.x async embedding, multimodal content, embedding profile guard, helper APIs, CLI/MCP status output, and remote embedding safety boundaries.
- Updated examples and installation/usage sections so GitHub and crates.io docs reflect the actually shipped 0.3.x behavior.


## [0.3.1] - 2026-04-09

### Added
- Added async embedding support with `LocalEmbedder` and opt-in `Gemini2Embedder`.
- Added multimodal `ContentPart` support plus helper APIs like `Drawer::new(...)`, `Drawer::multimodal(...)`, and `MemoryStack::file_text(...)`.
- Added embedding store metadata/profile tracking (`provider` / `model` / `dimension`) and surfaced it in CLI/MCP status output.
- Added CI dependency auditing with `cargo audit`.

### Changed
- Updated mining, conversation import, search, MCP, examples, and docs to use the new async embedding flow.
- Tightened extractor heuristics and added multilingual regression coverage for a narrower, safer EN/ZH/CAN/JP rule subset.

### Fixed
- Prevented remote embedding from automatically reading local files from URI-only parts.
- Prevented mixed embedding stores by rejecting provider/model/dimension mismatches at write and query time.


## [0.2.1] - 2026-04-08

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
