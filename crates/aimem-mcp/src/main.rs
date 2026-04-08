use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use aimem_core::{AimemDb, Config, Drawer, Embedder, SearchResult, Searcher};
use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::{Map, Value, json};
use tracing_subscriber::EnvFilter;

const AIMEM_PROTOCOL: &str = "IMPORTANT — AiMem Memory Protocol:\n1. ON WAKE-UP: Call aimem_status to load the AiMem overview + AAAK spec.\n2. BEFORE RESPONDING about any person, project, or past event: call aimem_search FIRST. Never guess — verify.\n3. IF UNSURE about a fact: say 'let me check' and query AiMem. Wrong is worse than slow.\n4. STORAGE is not memory; storage + retrieval protocol is memory.";

const AAAK_SPEC: &str = "AAAK is AiMem's compressed memory dialect.\n- ENTITIES: short uppercase codes\n- EMOTIONS: *markers* inline\n- STRUCTURE: compact pipe-separated fields\n- DATES: ISO-8601\nRead it naturally; write it tightly.";

#[derive(Clone)]
struct ServerState {
    cfg: Config,
    db: AimemDb,
    embedder: Arc<Mutex<Option<Embedder>>>,
    embedder_loading_enabled: bool,
}

impl ServerState {
    async fn new() -> Result<Self> {
        let cfg = Config::load().context("failed to load config")?;
        Self::from_paths(cfg.db_path.clone(), cfg.identity_path.clone()).await
    }

    async fn from_paths(db_path: PathBuf, identity_path: PathBuf) -> Result<Self> {
        Self::from_paths_with_options(db_path, identity_path, true).await
    }

    async fn from_paths_with_options(
        db_path: PathBuf,
        identity_path: PathBuf,
        embedder_loading_enabled: bool,
    ) -> Result<Self> {
        let mut cfg = Config::default();
        cfg.db_path = db_path;
        cfg.identity_path = identity_path;

        let db = AimemDb::open(&cfg.db_path)
            .await
            .with_context(|| format!("failed to open AiMem DB at {}", cfg.db_path.display()))?;

        Ok(Self {
            cfg,
            db,
            embedder: Arc::new(Mutex::new(None)),
            embedder_loading_enabled,
        })
    }

    async fn tool_status(&self) -> Result<Value> {
        let total_drawers = self.db.drawer_count().await?;
        let (wings, rooms) = self.db.taxonomy().await?;

        Ok(json!({
            "total_drawers": total_drawers,
            "wings": counts_vec_to_map(wings),
            "rooms": counts_vec_to_map(rooms),
            "db_path": self.cfg.db_path.display().to_string(),
            "protocol": AIMEM_PROTOCOL,
            "aaak_dialect": AAAK_SPEC,
        }))
    }

    async fn tool_list_wings(&self) -> Result<Value> {
        let (wings, _) = self.db.taxonomy().await?;
        Ok(json!({ "wings": counts_vec_to_map(wings) }))
    }

    async fn tool_list_rooms(&self, wing: Option<&str>) -> Result<Value> {
        Ok(json!({
            "wing": wing.unwrap_or("all"),
            "rooms": self.room_counts(wing).await?,
        }))
    }

    async fn tool_get_taxonomy(&self) -> Result<Value> {
        Ok(json!({ "taxonomy": self.taxonomy_tree().await? }))
    }

    async fn tool_get_aaak_spec(&self) -> Result<Value> {
        Ok(json!({ "aaak_spec": AAAK_SPEC }))
    }

    async fn tool_search(
        &self,
        query: &str,
        limit: usize,
        wing: Option<&str>,
        room: Option<&str>,
    ) -> Result<Value> {
        let keyword_searcher = Searcher::keyword_only(self.db.clone());
        let keyword_results = keyword_searcher
            .keyword_fallback_search(query, wing, room, limit)
            .await?;

        if !keyword_results.is_empty() {
            return Ok(search_payload(
                query,
                limit,
                wing,
                room,
                "keyword",
                Vec::new(),
                keyword_results,
            ));
        }

        match self.ensure_embedder() {
            Ok(embedder) => {
                let searcher = Searcher::new(self.db.clone(), embedder);
                let semantic_results = searcher.vector_search(query, wing, room, limit).await?;

                if !semantic_results.is_empty() {
                    Ok(search_payload(
                        query,
                        limit,
                        wing,
                        room,
                        "semantic",
                        semantic_results,
                        Vec::new(),
                    ))
                } else {
                    Ok(search_payload(
                        query,
                        limit,
                        wing,
                        room,
                        "none",
                        Vec::new(),
                        Vec::new(),
                    ))
                }
            }
            Err(err) if !keyword_results.is_empty() => {
                tracing::warn!(
                    "semantic search unavailable, falling back to keyword search: {err}"
                );
                Ok(search_payload(
                    query,
                    limit,
                    wing,
                    room,
                    "keyword",
                    Vec::new(),
                    keyword_results,
                ))
            }
            Err(err) => Ok(json!({
                "query": query,
                "limit": limit,
                "wing": wing,
                "room": room,
                "strategy": "none",
                "semantic_error": err.to_string(),
                "results": [],
            })),
        }
    }

    async fn tool_check_duplicate(
        &self,
        content: &str,
        threshold: f32,
        limit: usize,
    ) -> Result<Value> {
        let exact_matches = self
            .db
            .find_drawers_by_exact_content(content, limit)
            .await?;

        if !exact_matches.is_empty() {
            return Ok(json!({
                "is_duplicate": true,
                "exact_matches": exact_matches.into_iter().map(drawer_to_json).collect::<Vec<_>>(),
                "semantic_matches": [],
                "threshold": threshold,
            }));
        }

        let semantic_matches = match self.ensure_embedder() {
            Ok(embedder) => {
                let searcher = Searcher::new(self.db.clone(), embedder);
                searcher
                    .find_duplicates(content, threshold, limit)
                    .await?
                    .into_iter()
                    .collect::<Vec<_>>()
            }
            Err(err) => {
                tracing::warn!("duplicate semantic check unavailable: {err}");
                Vec::new()
            }
        };

        Ok(json!({
            "is_duplicate": !exact_matches.is_empty() || !semantic_matches.is_empty(),
            "exact_matches": exact_matches.into_iter().map(drawer_to_json).collect::<Vec<_>>(),
            "semantic_matches": semantic_matches.into_iter().map(search_result_to_json).collect::<Vec<_>>(),
            "threshold": threshold,
        }))
    }

    async fn tool_add_drawer(
        &self,
        wing: &str,
        room: &str,
        content: &str,
        source_file: Option<&str>,
        added_by: Option<&str>,
    ) -> Result<Value> {
        let duplicate_report = self.tool_check_duplicate(content, 0.9, 5).await?;
        let is_duplicate = duplicate_report
            .get("is_duplicate")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        if is_duplicate {
            return Ok(json!({
                "added": false,
                "duplicate": true,
                "matches": duplicate_report,
            }));
        }

        let (embedding, embedding_error) = match self.ensure_embedder() {
            Ok(embedder) => match embedder.embed_one(content) {
                Ok(embedding) => (Some(embedding), None::<String>),
                Err(err) => (None, Some(err.to_string())),
            },
            Err(err) => (None, Some(err.to_string())),
        };

        let filed_at = Utc::now().to_rfc3339();
        let drawer = Drawer {
            id: drawer_id(wing, room, content, source_file, &filed_at),
            wing: wing.to_string(),
            room: room.to_string(),
            content: content.to_string(),
            parts: vec![],
            source_file: source_file.map(str::to_string),
            chunk_index: 0,
            added_by: added_by.unwrap_or("mcp").to_string(),
            filed_at,
        };

        let inserted = self.db.insert_drawer(&drawer, embedding.as_deref()).await?;

        Ok(json!({
            "added": inserted,
            "duplicate": false,
            "drawer": drawer_to_json(drawer),
            "embedding_stored": embedding.is_some(),
            "embedding_error": embedding_error,
        }))
    }

    async fn tool_delete_drawer(&self, drawer_id: &str) -> Result<Value> {
        let deleted = self.db.delete_drawer(drawer_id).await?;
        Ok(json!({
            "deleted": deleted,
            "drawer_id": drawer_id,
        }))
    }

    async fn room_counts(&self, wing: Option<&str>) -> Result<BTreeMap<String, i64>> {
        let conn = self.db.conn()?;
        let mut rows = match wing {
            Some(wing) => {
                conn.query(
                    "SELECT room, COUNT(*) AS cnt FROM drawers WHERE wing = ?1 GROUP BY room ORDER BY cnt DESC, room ASC",
                    [wing],
                )
                .await?
            }
            None => {
                conn.query(
                    "SELECT room, COUNT(*) AS cnt FROM drawers GROUP BY room ORDER BY cnt DESC, room ASC",
                    (),
                )
                .await?
            }
        };

        let mut rooms = BTreeMap::new();
        while let Some(row) = rows.next().await? {
            if let Some((name, count)) = row_count_pair(&row)? {
                rooms.insert(name, count);
            }
        }
        Ok(rooms)
    }

    async fn taxonomy_tree(&self) -> Result<BTreeMap<String, BTreeMap<String, i64>>> {
        let conn = self.db.conn()?;
        let mut rows = conn
            .query(
                "SELECT wing, room, COUNT(*) AS cnt FROM drawers GROUP BY wing, room ORDER BY wing ASC, cnt DESC, room ASC",
                (),
            )
            .await?;

        let mut taxonomy = BTreeMap::new();
        while let Some(row) = rows.next().await? {
            let wing = value_to_string(row.get_value(0)?);
            let room = value_to_string(row.get_value(1)?);
            let count = row.get_value(2)?.as_integer().copied().unwrap_or(0);
            taxonomy
                .entry(wing)
                .or_insert_with(BTreeMap::new)
                .insert(room, count);
        }
        Ok(taxonomy)
    }

    fn ensure_embedder(&self) -> Result<Embedder> {
        if !self.embedder_loading_enabled {
            anyhow::bail!("embedding disabled for this server state");
        }

        if let Some(embedder) = self
            .embedder
            .lock()
            .expect("embedder mutex poisoned")
            .clone()
        {
            return Ok(embedder);
        }

        let embedder = Embedder::new().context("failed to load local embedding model")?;
        let mut guard = self.embedder.lock().expect("embedder mutex poisoned");
        *guard = Some(embedder.clone());
        Ok(embedder)
    }
}

fn init_tracing() -> Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .try_init()
        .map_err(|err| anyhow::anyhow!("failed to initialize tracing subscriber: {err}"))?;
    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    init_tracing()?;
    let state = ServerState::new().await?;

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line.context("failed to read stdin line")?;
        if line.trim().is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(err) => {
                let response = error_response(Value::Null, -32700, &format!("invalid JSON: {err}"));
                writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
                stdout.flush()?;
                continue;
            }
        };

        if let Some(response) = handle_request(&state, &request).await {
            writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
            stdout.flush()?;
        }
    }

    Ok(())
}

async fn handle_request(state: &ServerState, request: &Value) -> Option<Value> {
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let req_id = request.get("id").cloned().unwrap_or(Value::Null);
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

    match method {
        "initialize" => Some(success_response(
            req_id,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": "aimem",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }),
        )),
        "notifications/initialized" => None,
        "tools/list" => Some(success_response(req_id, json!({ "tools": tool_specs() }))),
        "tools/call" => Some(handle_tool_call(state, req_id, &params).await),
        _ => Some(error_response(
            req_id,
            -32601,
            &format!("Unknown method: {method}"),
        )),
    }
}

async fn handle_tool_call(state: &ServerState, req_id: Value, params: &Value) -> Value {
    let tool_name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let arguments = params
        .get("arguments")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let result = match tool_name {
        "aimem_status" => state.tool_status().await,
        "aimem_list_wings" => state.tool_list_wings().await,
        "aimem_list_rooms" => {
            state
                .tool_list_rooms(arguments.get("wing").and_then(Value::as_str))
                .await
        }
        "aimem_get_taxonomy" => state.tool_get_taxonomy().await,
        "aimem_get_aaak_spec" => state.tool_get_aaak_spec().await,
        "aimem_search" => {
            let query = match arguments.get("query").and_then(Value::as_str) {
                Some(query) if !query.is_empty() => query,
                _ => {
                    return error_response(
                        req_id,
                        -32602,
                        "aimem_search requires a non-empty `query`",
                    );
                }
            };
            let limit = positive_usize(&arguments, "limit").unwrap_or(5);
            state
                .tool_search(
                    query,
                    limit,
                    arguments.get("wing").and_then(Value::as_str),
                    arguments.get("room").and_then(Value::as_str),
                )
                .await
        }
        "aimem_check_duplicate" => {
            let content = match arguments.get("content").and_then(Value::as_str) {
                Some(content) if !content.is_empty() => content,
                _ => {
                    return error_response(
                        req_id,
                        -32602,
                        "aimem_check_duplicate requires non-empty `content`",
                    );
                }
            };
            state
                .tool_check_duplicate(
                    content,
                    arguments
                        .get("threshold")
                        .and_then(Value::as_f64)
                        .map(|value| value as f32)
                        .unwrap_or(0.9),
                    positive_usize(&arguments, "limit").unwrap_or(5),
                )
                .await
        }
        "aimem_add_drawer" => {
            let wing = match arguments.get("wing").and_then(Value::as_str) {
                Some(wing) if !wing.is_empty() => wing,
                _ => return error_response(req_id, -32602, "aimem_add_drawer requires `wing`"),
            };
            let room = match arguments.get("room").and_then(Value::as_str) {
                Some(room) if !room.is_empty() => room,
                _ => return error_response(req_id, -32602, "aimem_add_drawer requires `room`"),
            };
            let content = match arguments.get("content").and_then(Value::as_str) {
                Some(content) if !content.is_empty() => content,
                _ => {
                    return error_response(
                        req_id,
                        -32602,
                        "aimem_add_drawer requires non-empty `content`",
                    );
                }
            };
            state
                .tool_add_drawer(
                    wing,
                    room,
                    content,
                    arguments.get("source_file").and_then(Value::as_str),
                    arguments.get("added_by").and_then(Value::as_str),
                )
                .await
        }
        "aimem_delete_drawer" => {
            let drawer_id = match arguments.get("drawer_id").and_then(Value::as_str) {
                Some(drawer_id) if !drawer_id.is_empty() => drawer_id,
                _ => {
                    return error_response(
                        req_id,
                        -32602,
                        "aimem_delete_drawer requires `drawer_id`",
                    );
                }
            };
            state.tool_delete_drawer(drawer_id).await
        }
        _ => return error_response(req_id, -32601, &format!("Unknown tool: {tool_name}")),
    };

    match result {
        Ok(payload) => tool_success(req_id, payload),
        Err(err) => error_response(req_id, -32000, &err.to_string()),
    }
}

fn tool_specs() -> Vec<Value> {
    vec![
        tool_spec(
            "aimem_status",
            "AiMem overview — total drawers, wing and room counts.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool_spec(
            "aimem_list_wings",
            "List all wings with drawer counts.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool_spec(
            "aimem_list_rooms",
            "List rooms within a wing, or all rooms if no wing is provided.",
            json!({
                "type": "object",
                "properties": {
                    "wing": { "type": "string", "description": "Wing to filter by (optional)." }
                }
            }),
        ),
        tool_spec(
            "aimem_get_taxonomy",
            "Full taxonomy: wing → room → drawer count.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool_spec(
            "aimem_get_aaak_spec",
            "Return the AAAK dialect reference used by AiMem.",
            json!({ "type": "object", "properties": {} }),
        ),
        tool_spec(
            "aimem_search",
            "Search AiMem. Prefers semantic results when available, falls back to keyword search.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "What to search for." },
                    "limit": { "type": "integer", "description": "Max results (default 5)." },
                    "wing": { "type": "string", "description": "Wing filter (optional)." },
                    "room": { "type": "string", "description": "Room filter (optional)." }
                },
                "required": ["query"]
            }),
        ),
        tool_spec(
            "aimem_check_duplicate",
            "Check whether content already exists in AiMem.",
            json!({
                "type": "object",
                "properties": {
                    "content": { "type": "string", "description": "Content to check." },
                    "threshold": { "type": "number", "description": "Semantic threshold (default 0.9)." },
                    "limit": { "type": "integer", "description": "Max matches to return (default 5)." }
                },
                "required": ["content"]
            }),
        ),
        tool_spec(
            "aimem_add_drawer",
            "File verbatim content into AiMem, checking duplicates first.",
            json!({
                "type": "object",
                "properties": {
                    "wing": { "type": "string", "description": "Wing name." },
                    "room": { "type": "string", "description": "Room name." },
                    "content": { "type": "string", "description": "Verbatim drawer content." },
                    "source_file": { "type": "string", "description": "Source file path (optional)." },
                    "added_by": { "type": "string", "description": "Agent/user label (default: mcp)." }
                },
                "required": ["wing", "room", "content"]
            }),
        ),
        tool_spec(
            "aimem_delete_drawer",
            "Delete a drawer by ID.",
            json!({
                "type": "object",
                "properties": {
                    "drawer_id": { "type": "string", "description": "Drawer ID to remove." }
                },
                "required": ["drawer_id"]
            }),
        ),
    ]
}

fn tool_spec(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema,
    })
}

fn search_payload(
    query: &str,
    limit: usize,
    wing: Option<&str>,
    room: Option<&str>,
    strategy: &str,
    semantic_results: Vec<SearchResult>,
    keyword_results: Vec<Drawer>,
) -> Value {
    let results = if !semantic_results.is_empty() {
        semantic_results
            .into_iter()
            .map(search_result_to_json)
            .collect::<Vec<_>>()
    } else {
        keyword_results
            .into_iter()
            .map(drawer_to_json)
            .collect::<Vec<_>>()
    };

    json!({
        "query": query,
        "limit": limit,
        "wing": wing,
        "room": room,
        "strategy": strategy,
        "results": results,
    })
}

fn drawer_to_json(drawer: Drawer) -> Value {
    json!({
        "id": drawer.id,
        "wing": drawer.wing,
        "room": drawer.room,
        "content": drawer.content,
        "parts": drawer.parts,
        "source_file": drawer.source_file,
        "chunk_index": drawer.chunk_index,
        "added_by": drawer.added_by,
        "filed_at": drawer.filed_at,
    })
}

fn search_result_to_json(result: SearchResult) -> Value {
    json!({
        "id": result.drawer.id,
        "wing": result.drawer.wing,
        "room": result.drawer.room,
        "content": result.drawer.content,
        "parts": result.drawer.parts,
        "source_file": result.drawer.source_file,
        "similarity": result.similarity,
    })
}

fn drawer_id(
    wing: &str,
    room: &str,
    content: &str,
    source_file: Option<&str>,
    filed_at: &str,
) -> String {
    let digest = md5::compute(
        format!(
            "{wing}\u{1f}{room}\u{1f}{}\u{1f}{content}\u{1f}{filed_at}",
            source_file.unwrap_or("")
        )
        .as_bytes(),
    );
    format!("drawer_{}_{}_{digest:x}", slugish(wing), slugish(room),)
}

fn slugish(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

fn positive_usize(arguments: &Map<String, Value>, key: &str) -> Option<usize> {
    arguments
        .get(key)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .filter(|value| *value > 0)
}

fn counts_vec_to_map(items: Vec<(String, i64)>) -> BTreeMap<String, i64> {
    items.into_iter().collect()
}

fn row_count_pair(row: &turso::Row) -> Result<Option<(String, i64)>> {
    let name = value_to_string(row.get_value(0)?);
    let count = row.get_value(1)?.as_integer().copied().unwrap_or(0);
    if name.is_empty() {
        Ok(None)
    } else {
        Ok(Some((name, count)))
    }
}

fn value_to_string(value: turso::Value) -> String {
    match value {
        turso::Value::Text(text) => text,
        turso::Value::Null => String::new(),
        other => format!("{other:?}"),
    }
}

fn success_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn tool_success(id: Value, payload: Value) -> Value {
    success_response(
        id,
        json!({
            "content": [
                {
                    "type": "text",
                    "text": serde_json::to_string_pretty(&payload).expect("payload serialization failed"),
                }
            ]
        }),
    )
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_paths(name: &str) -> (PathBuf, PathBuf) {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("aimem-mcp-{name}-{suffix}"));
        fs::create_dir_all(&dir).expect("failed to create temp dir");
        (dir.join("aimem.db"), dir.join("identity.txt"))
    }

    async fn test_state(name: &str) -> ServerState {
        let (db_path, identity_path) = test_paths(name);
        ServerState::from_paths_with_options(db_path, identity_path, false)
            .await
            .expect("failed to build test state")
    }

    async fn seed_drawer(state: &ServerState) -> String {
        let drawer = Drawer {
            id: "drawer_mcp_test_001".into(),
            wing: "demo_app".into(),
            room: "backend".into(),
            content: "Turso keeps AiMem local and searchable.".into(),
            parts: vec![],
            source_file: Some("README.md".into()),
            chunk_index: 0,
            added_by: "test".into(),
            filed_at: "2026-04-07T00:00:00Z".into(),
        };
        let id = drawer.id.clone();
        state
            .db
            .insert_drawer(&drawer, None)
            .await
            .expect("failed to insert drawer");
        id
    }

    async fn call_tool(state: &ServerState, name: &str, arguments: Value) -> Value {
        let request = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": name,
                "arguments": arguments,
            }
        });
        let response = handle_request(state, &request)
            .await
            .expect("missing response");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("missing tool text");
        serde_json::from_str(payload).expect("payload should be JSON")
    }

    #[tokio::test]
    async fn initialize_returns_capabilities() {
        let state = test_state("initialize").await;
        let request = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} });
        let response = handle_request(&state, &request)
            .await
            .expect("missing response");

        assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(response["result"]["serverInfo"]["name"], "aimem");
    }

    #[tokio::test]
    async fn tools_list_exposes_read_and_write_tools() {
        let state = test_state("tools-list").await;
        let request = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} });
        let response = handle_request(&state, &request)
            .await
            .expect("missing response");
        let tools = response["result"]["tools"]
            .as_array()
            .expect("tools should be an array");
        let names: Vec<_> = tools
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();

        assert!(names.contains(&"aimem_status"));
        assert!(names.contains(&"aimem_search"));
        assert!(names.contains(&"aimem_check_duplicate"));
        assert!(names.contains(&"aimem_add_drawer"));
        assert!(names.contains(&"aimem_delete_drawer"));
    }

    #[tokio::test]
    async fn search_tool_returns_keyword_results() {
        let state = test_state("search").await;
        seed_drawer(&state).await;

        let parsed = call_tool(
            &state,
            "aimem_search",
            json!({ "query": "Turso", "wing": "demo_app", "limit": 5 }),
        )
        .await;

        assert_eq!(parsed["strategy"], "keyword");
        assert_eq!(
            parsed["results"].as_array().expect("results array").len(),
            1
        );
        assert_eq!(parsed["results"][0]["room"], "backend");
    }

    #[tokio::test]
    async fn status_tool_reports_counts() {
        let state = test_state("status").await;
        seed_drawer(&state).await;

        let parsed = call_tool(&state, "aimem_status", json!({})).await;

        assert_eq!(parsed["total_drawers"], 1);
        assert_eq!(parsed["wings"]["demo_app"], 1);
        assert_eq!(parsed["rooms"]["backend"], 1);
    }

    #[tokio::test]
    async fn check_duplicate_reports_exact_match() {
        let state = test_state("duplicate").await;
        seed_drawer(&state).await;

        let parsed = call_tool(
            &state,
            "aimem_check_duplicate",
            json!({ "content": "Turso keeps AiMem local and searchable." }),
        )
        .await;

        assert_eq!(parsed["is_duplicate"], true);
        assert_eq!(
            parsed["exact_matches"]
                .as_array()
                .expect("exact array")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn add_drawer_writes_new_content() {
        let state = test_state("add").await;

        let parsed = call_tool(
            &state,
            "aimem_add_drawer",
            json!({
                "wing": "demo_app",
                "room": "decisions",
                "content": "We chose Turso so the memory system stays local.",
                "source_file": "DECISIONS.md",
            }),
        )
        .await;

        assert_eq!(parsed["added"], true);
        assert_eq!(parsed["duplicate"], false);
        assert_eq!(state.db.drawer_count().await.expect("drawer count"), 1);
    }

    #[tokio::test]
    async fn delete_drawer_removes_existing_row() {
        let state = test_state("delete").await;
        let drawer_id = seed_drawer(&state).await;

        let parsed = call_tool(
            &state,
            "aimem_delete_drawer",
            json!({ "drawer_id": drawer_id }),
        )
        .await;

        assert_eq!(parsed["deleted"], true);
        assert_eq!(state.db.drawer_count().await.expect("drawer count"), 0);
    }
}
