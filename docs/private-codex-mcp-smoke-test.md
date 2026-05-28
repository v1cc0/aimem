# Private Codex MCP Smoke Test

This is a tiny manual JSON-RPC smoke test for the private `codex_*` MCP tools.
It uses a temporary DB so it does not touch your normal `~/.aimem/aimem.db`.

The examples are newline-delimited JSON because `aimem-mcp` speaks MCP over stdio.

## Run against a temporary DB

```bash
export AIMEM_DB_PATH=/tmp/aimem-codex-smoke.db
rm -f "$AIMEM_DB_PATH" "$AIMEM_DB_PATH"-wal "$AIMEM_DB_PATH"-tshm
```

Then paste the JSON-RPC lines below into `aimem-mcp`, or pipe them as one stream.

```bash
cat <<'JSONRPC' | aimem-mcp
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"codex_record_repo","arguments":{"repo_path":"/tmp/aimem-smoke","repo_name":"aimem-smoke","language":"rust","summary":"Temporary smoke-test repo profile."}}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"codex_record_experience","arguments":{"repo_path":"/tmp/aimem-smoke","repo_name":"aimem-smoke","language":"rust","kind":"testing","problem":"Need to prove private Codex MCP record/search works.","solution":"Record one compact experience card, then search for it.","outcome":"Smoke-test card is searchable.","commands":["cargo test -p aimem-mcp"]}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"codex_record_command","arguments":{"repo_path":"/tmp/aimem-smoke","language":"rust","command":"cargo test -p aimem-mcp","cwd":"/tmp/aimem-smoke","purpose":"Verify MCP tool handlers","result":"passed","output_summary":"MCP tests passed."}}}
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"codex_record_round_summary","arguments":{"repo_path":"/tmp/aimem-smoke","language":"rust","summary":"Smoke-tested private Codex MCP record/search/context flow.","changed_files":["crates/aimem-mcp/src/main.rs"],"commands":["cargo test -p aimem-mcp"],"next_steps":["Delete the temporary smoke-test card if desired."]}}}
{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"codex_search_experience","arguments":{"query":"private Codex MCP record search smoke","repo_path":"/tmp/aimem-smoke","language":"rust","limit":5}}}
{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"codex_context","arguments":{"repo_path":"/tmp/aimem-smoke","task":"private Codex MCP smoke test","limit":5}}}
JSONRPC
```

## Expected checks

The output is JSON-RPC. For a healthy smoke test:

- response `id=1` has `serverInfo.name = "aimem"`.
- response `id=2` records a `codex_repo` drawer.
- response `id=3` records a `codex_experience` drawer.
- response `id=4` records a `kind: command` card.
- response `id=5` records a `kind: round_summary` card.
- response `id=6` returns search results containing the smoke-test cards.
- response `id=7` returns `repo_profile` and `relevant_experiences`.

## Delete a smoke-test card

Copy a drawer `id` from one of the search/context responses, then call:

```json
{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"codex_delete_experience","arguments":{"drawer_id":"PASTE_DRAWER_ID_HERE"}}}
```

A successful delete returns `"deleted": true`.

## Cleanup

```bash
rm -f /tmp/aimem-codex-smoke.db /tmp/aimem-codex-smoke.db-wal /tmp/aimem-codex-smoke.db-tshm
```

## Scope reminder

The `codex_*` tools only store what the caller explicitly sends, plus conservative metadata around an explicit `repo_path` for `codex_record_repo`. They do not crawl your filesystem, ingest transcripts automatically, or collect secrets.
