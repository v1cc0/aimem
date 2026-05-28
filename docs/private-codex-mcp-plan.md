# Private Codex MCP Plan

## Goal

Build a private MCP server on top of AiMem so Codex can:

1. Record every repository Codex edits, including decisions, fixes, failures, tests, and release lessons.
2. Retrieve and apply those coding experiences automatically while working in future repositories.

This is not a chat-log dump. It is a local, searchable engineering memory layer: small, structured facts; concrete incidents; reproducible commands; and repo-specific conventions.

## Non-goals

- Do not index unrelated user directories or credentials.
- Do not store private secrets, API keys, SSH keys, tokens, or raw `.env` contents.
- Do not require a hosted service for the MVP.
- Do not make Codex blindly obey stored memories. Retrieved experience is evidence, not authority.
- Do not turn this into a social/team knowledge base before the solo workflow works.

## Design stance

The useful abstraction is not “memory”. It is “experience that can change the next coding action”.

Bad design: store giant transcripts and hope vector search saves us.

Good design: store compact experience cards with stable metadata:

- where it happened
- what problem was solved
- what was tried
- what failed
- what finally worked
- what commands verified it
- what future Codex should do differently

## Current repo leverage

AiMem already has most of the substrate:

- `aimem-core`: local Turso-backed storage, drawers, metadata, keyword + vector hybrid search.
- `aimem-mcp`: JSON-RPC MCP server over stdio with status/search/add/delete tools.
- Existing local workflow files: `PROGRESS.md` and `INCIDENTS.md`.
- Existing duplicate checking and stable-ish search behavior.

The private Codex MCP should be a focused extension, not a rewrite.

## Data model

Use AiMem drawers first; add typed conventions before adding new tables. If this becomes painful, promote the conventions into first-class tables later.

### Wings

- `codex_repo`: repository profiles and conventions.
- `codex_experience`: reusable coding lessons.
- `codex_incident`: mistakes, regressions, broken workflows, failed assumptions.
- `codex_command`: commands that validated or broke something.
- `codex_release`: release and publishing checklists/outcomes.

### Rooms

Suggested room names:

- `repo_profile`
- `architecture`
- `workflow`
- `dependency`
- `testing`
- `debugging`
- `incident`
- `release`
- `style`
- `mcp_usage`

### Experience card format

Store the content as concise Markdown with a YAML-ish header that is still useful as plain text:

```text
repo: /home/vc/gits/aimem
repo_name: aimem
language: rust
scope: workspace
kind: incident | decision | fix | command | release | convention
confidence: high | medium | low
created_at: 2026-05-28T00:00:00Z
source_files:
  - crates/aimem-mcp/src/main.rs
commands:
  - cargo test
problem: one sentence
solution: one sentence
---
Details:
- What happened.
- What worked.
- What not to repeat.
Future Codex rule:
- Concrete imperative.
```

### Required metadata

The MVP can encode these in content, but the MCP tool should accept them as arguments:

- `repo_path`
- `repo_name`
- `git_remote`
- `git_head`
- `language`
- `kind`
- `tags`
- `source_files`
- `commands`
- `outcome`
- `confidence`

## MCP tool surface

Keep tools boring and explicit. Fancy agent magic belongs after the data proves useful.

### Read tools

1. `codex_context`
   - Input: `repo_path`, optional `task`, optional `limit`.
   - Output: repo profile, top conventions, recent incidents, and task-relevant experiences.
   - Codex should call this at the start of a coding session.

2. `codex_search_experience`
   - Input: `query`, optional `repo_path`, optional `language`, optional `kind`, optional `limit`.
   - Output: ranked experience cards.
   - Codex should call this before touching unfamiliar code, fixing test failures, changing release flow, or repeating a known pattern.

3. `codex_repo_profile`
   - Input: `repo_path`.
   - Output: language, package manager, test commands, release rules, local-only files, known hazards.

4. `codex_incidents`
   - Input: optional `repo_path`, optional `limit`.
   - Output: high-priority “do not repeat” records.

### Write tools

5. `codex_record_repo`
   - Input: `repo_path`, detected metadata, summary.
   - Records or updates the repo profile.

6. `codex_record_experience`
   - Input: structured card fields.
   - Records a reusable lesson after a meaningful coding event.

7. `codex_record_incident`
   - Input: what happened, impact, root cause, prevention.
   - Records an incident. This should be mirrored into local `INCIDENTS.md` when it is repo-specific.

8. `codex_record_command`
   - Input: command, cwd, purpose, result, important output summary.
   - Records verification commands, not every shell command.

9. `codex_record_round_summary`
   - Input: repo_path, changed files, summary, tests, next steps.
   - Records the final state at handoff. This maps naturally to `PROGRESS.md`.

## Codex usage protocol

The MCP cannot force intelligence by itself. Codex needs a strict retrieval protocol.

### Session start

When entering a repo:

1. Read local `AGENTS.md`, `INCIDENTS.md`, and `PROGRESS.md` if present.
2. Call `codex_context(repo_path, task)`.
3. Treat returned incidents and repo rules as constraints.
4. If local files conflict with MCP memories, live repo files win; record a correction later.

### Before coding

Call `codex_search_experience` when the task involves:

- release/publish flow
- dependency upgrade
- flaky tests
- MCP protocol behavior
- database migration/search behavior
- anything similar to an existing incident

### After coding

Record only useful information:

- non-obvious bug cause
- command that proved the fix
- repo convention discovered
- release gotcha
- dependency/API mismatch
- incident or near-miss

Do not record noise such as “ran ls”, “opened file”, or obvious edits.

## Privacy and safety rules

- Default database path should live under the user config/data directory, not inside a repo.
- Redact obvious secret patterns before storing:
  - `*_TOKEN`
  - `*_KEY`
  - `SECRET`
  - `.env` values
  - SSH/private-key blocks
- Store file paths and command names, but avoid full raw logs unless explicitly useful.
- Never auto-index the whole home directory.
- Only record repos that Codex actually edits or the user explicitly registers.
- Add a delete tool before broad auto-capture.

## Implementation phases

### Phase 0 — Plan and boundaries

Deliverables:

- This plan.
- Clear scope: private local MCP, repo-experience memory, no secret harvesting.

Acceptance:

- The project has a committed design document.
- `PROGRESS.md` points to the next implementation step.

### Phase 1 — Minimal private Codex tools inside `aimem-mcp`

Add tool names while reusing existing drawer storage:

- `codex_record_repo`
- `codex_record_experience`
- `codex_search_experience`
- `codex_context`

Implementation details:

- Add typed argument parsing in `crates/aimem-mcp/src/main.rs` or split MCP tool handlers into modules if the file becomes ugly.
- Store cards using existing `Drawer` rows.
- Use deterministic IDs for repo profile cards, e.g. hash of canonical repo path + card kind.
- Use timestamped IDs for event cards.
- Search should combine explicit filters with existing hybrid search.

Acceptance:

- Unit tests prove tools are listed.
- Unit tests can record an experience and retrieve it by repo/task query.
- Existing `aimem_*` tools keep working.

### Phase 2 — Repo scanner and summarizer helper

Add a small helper layer that detects:

- repo root
- remote URL
- current branch/head
- languages/package manager from manifests
- likely test commands
- local instruction files (`AGENTS.md`, `PROGRESS.md`, `INCIDENTS.md`)

This should be conservative. It records facts, not speculation.

Acceptance:

- `codex_record_repo` can populate a useful profile from `/home/vc/gits/aimem`.
- No unrelated directories are read.
- Secrets are redacted in any captured snippets.

### Phase 3 — Round summary workflow

Add:

- `codex_record_command`
- `codex_record_incident`
- `codex_record_round_summary`

Map these to the current solo workflow:

- `PROGRESS.md` remains the repo-local latest state.
- `INCIDENTS.md` remains the repo-local high-priority warning file.
- MCP stores cross-repo searchable experience.

Acceptance:

- A completed coding round can store changed files, verification commands, and next steps.
- Incident records are searchable from a future unrelated repo.

### Phase 4 — Retrieval quality and ranking

Improve search behavior for coding memory:

- Boost same repo.
- Boost same language/framework.
- Boost incidents over generic notes for safety-sensitive operations.
- Prefer recent records only when equally relevant.
- Return compact cards with decisive lines first.

Acceptance:

- Query “release README crates.io mistake” retrieves the existing release incident.
- Query “Turso FTS MVCC” retrieves the hybrid-search runtime constraint.
- Query “MCP tool add search” retrieves the implementation pattern in `aimem-mcp`.

### Phase 5 — Codex client integration

Configure Codex to use this private MCP as an always-available local server.

Expected behavior:

- At session start, Codex asks for repo context.
- Before risky edits, Codex searches relevant experience.
- At handoff, Codex records only durable lessons.

Acceptance:

- A fresh Codex session in this repo can recall prior incidents without reading the whole history.
- A fresh Codex session in another Rust repo can retrieve relevant AiMem lessons.

## MVP task list

1. Add `docs/private-codex-mcp-plan.md`.
2. Refactor `aimem-mcp` tool handling enough to avoid making `main.rs` worse.
3. Add card formatting/parsing helpers for Codex experience records.
4. Add `codex_record_experience` and `codex_search_experience`.
5. Add tests for record/search.
6. Add `codex_context` with repo-prioritized search.
7. Add `codex_record_repo` with explicit user/repo-provided metadata first.
8. Add conservative repo auto-detection later.

## First implementation cut

Do the dumb thing that works:

- No new database schema.
- No background daemon.
- No filesystem crawler.
- No transcript ingestion.
- Just four MCP tools over existing AiMem drawers.

If that is useful for two real coding rounds, then add structure. If not, fix the card shape before touching storage.

## Risks

- Garbage memory: solved by recording only durable lessons, not every action.
- Retrieval spam: solved by compact cards, repo/language filters, and low default limits.
- Secret leakage: solved by redaction and no broad indexing.
- Broken user workflow: solved by keeping existing `aimem_*` tools compatible.
- Overengineering: solved by delaying new tables and automation until the MCP tools prove value.

## Suggested next step

Implement Phase 1 in `crates/aimem-mcp` and validate with unit tests only. Do not touch release flow until the private Codex tools work locally.
