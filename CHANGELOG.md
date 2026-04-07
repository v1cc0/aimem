# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Added
- Initial Rust workspace MVP with `aimem-core`, `aimem` CLI, and `aimem-mcp`.
- GitHub Actions CI plus tag-triggered GitHub release automation.

### Changed
- New default local database path is `~/.aimem/aimem.db`.
- Existing `~/.aimem/palace.db` is still auto-detected so older local installs keep working.
