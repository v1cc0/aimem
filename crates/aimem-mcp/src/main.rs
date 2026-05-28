use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aimem_core::{
    AimemDb, Config, Drawer, Embedder, HybridSearchResult, LocalEmbedder, SearchResult, Searcher,
};
use anyhow::{Context, Result};
use chrono::Utc;
use serde_json::{Map, Value, json};
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

const AIMEM_PROTOCOL: &str = "IMPORTANT — AiMem Memory Protocol:\n1. ON WAKE-UP: Call aimem_status to load the AiMem overview + AAAK spec.\n2. BEFORE RESPONDING about any person, project, or past event: call aimem_search FIRST. Never guess — verify.\n3. IF UNSURE about a fact: say 'let me check' and query AiMem. Wrong is worse than slow.\n4. STORAGE is not memory; storage + retrieval protocol is memory.";

const AAAK_SPEC: &str = "AAAK is AiMem's compressed memory dialect.\n- ENTITIES: short uppercase codes\n- EMOTIONS: *markers* inline\n- STRUCTURE: compact pipe-separated fields\n- DATES: ISO-8601\nRead it naturally; write it tightly.";

const CODEX_REPO_WING: &str = "codex_repo";
const CODEX_EXPERIENCE_WING: &str = "codex_experience";
const CODEX_INCIDENT_WING: &str = "codex_incident";
const CODEX_DEFAULT_LIMIT: usize = 5;

#[derive(Clone)]
struct ServerState {
    cfg: Config,
    db: AimemDb,
    embedder: Arc<Mutex<Option<Arc<dyn Embedder>>>>,
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
        let embedding_profile = self.db.embedding_profile().await?;

        Ok(json!({
            "total_drawers": total_drawers,
            "wings": counts_vec_to_map(wings),
            "rooms": counts_vec_to_map(rooms),
            "embedding_profile": {
                "provider": embedding_profile.provider,
                "model": embedding_profile.model,
                "dimension": embedding_profile.dimension,
            },
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

        match self.ensure_embedder().await {
            Ok(embedder) => {
                let searcher = Searcher::new(self.db.clone(), embedder);
                let hybrid_results = searcher.hybrid_search(query, wing, room, limit).await?;

                if !hybrid_results.is_empty() {
                    Ok(search_payload(
                        query,
                        limit,
                        wing,
                        room,
                        "hybrid",
                        hybrid_results,
                        Vec::new(),
                    ))
                } else if !keyword_results.is_empty() {
                    Ok(search_payload(
                        query,
                        limit,
                        wing,
                        room,
                        "keyword",
                        Vec::new(),
                        keyword_results,
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
                tracing::warn!("hybrid search unavailable, falling back to keyword search: {err}");
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

        let semantic_matches = match self.ensure_embedder().await {
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

        let (embedding, embedding_error, embedding_profile) = match self.ensure_embedder().await {
            Ok(embedder) => {
                let profile = Some((
                    embedder.provider_name().to_string(),
                    embedder.model_name().to_string(),
                ));
                match embedder.embed_one(content).await {
                    Ok(embedding) => (Some(embedding), None::<String>, profile),
                    Err(err) => (None, Some(err.to_string()), profile),
                }
            }
            Err(err) => (None, Some(err.to_string()), None),
        };

        let filed_at = Utc::now().to_rfc3339();
        let mut drawer = Drawer::new(
            drawer_id(wing, room, content, source_file, &filed_at),
            wing,
            room,
            content,
            added_by.unwrap_or("mcp"),
        )
        .with_filed_at(filed_at);
        if let Some(source_file) = source_file {
            drawer = drawer.with_source_file(source_file);
        }

        let inserted = if let (Some(embedding), Some((provider, model))) =
            (embedding.as_deref(), embedding_profile.as_ref())
        {
            self.db
                .insert_drawer_with_profile(&drawer, Some(embedding), provider, model)
                .await?
        } else {
            self.db.insert_drawer(&drawer, embedding.as_deref()).await?
        };

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

    async fn tool_codex_delete_experience(&self, drawer_id: &str) -> Result<Value> {
        let deleted = self.db.delete_drawer(drawer_id).await?;
        Ok(json!({
            "deleted": deleted,
            "drawer_id": drawer_id,
            "scope": "codex_experience",
        }))
    }

    async fn tool_codex_record_repo(&self, arguments: &Map<String, Value>) -> Result<Value> {
        let repo_path = required_str(arguments, "repo_path")?;
        let detected = detect_repo_profile(repo_path);
        let profile = merge_detected_repo_profile(arguments, detected);
        let filed_at = Utc::now().to_rfc3339();
        let content = format_codex_card(
            "repo_profile",
            repo_path,
            &profile,
            &[
                "repo_name",
                "repo_root",
                "git_remote",
                "git_head",
                "language",
                "summary",
            ],
            &filed_at,
        );
        let id = codex_stable_id("repo", repo_path);
        let _ = self.db.delete_drawer(&id).await?;
        let drawer = Drawer::new(
            id.clone(),
            CODEX_REPO_WING,
            "repo_profile",
            content,
            "codex_mcp",
        )
        .with_source_file(repo_path)
        .with_filed_at(filed_at);
        let inserted = self.db.insert_drawer(&drawer, None).await?;

        Ok(json!({
            "recorded": inserted,
            "updated": true,
            "drawer": drawer_to_json(drawer),
        }))
    }

    async fn tool_codex_record_experience(&self, arguments: &Map<String, Value>) -> Result<Value> {
        let repo_path = required_str(arguments, "repo_path")?;
        let kind = arguments
            .get("kind")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("experience");
        let room = codex_room_for_kind(kind);
        let wing = if kind == "incident" {
            CODEX_INCIDENT_WING
        } else {
            CODEX_EXPERIENCE_WING
        };
        let filed_at = Utc::now().to_rfc3339();
        let content = format_codex_card(
            kind,
            repo_path,
            arguments,
            &[
                "repo_name",
                "language",
                "problem",
                "solution",
                "outcome",
                "confidence",
                "future_rule",
                "cwd",
                "purpose",
                "result",
                "output_summary",
                "summary",
            ],
            &filed_at,
        );
        let id = codex_experience_id(kind, repo_path, arguments);
        if self.db.drawer_exists(&id).await? {
            return Ok(json!({
                "recorded": false,
                "duplicate": true,
                "drawer_id": id,
                "fingerprint": codex_experience_fingerprint(kind, repo_path, arguments),
            }));
        }

        let drawer = Drawer::new(id.clone(), wing, room, content, "codex_mcp")
            .with_source_file(repo_path)
            .with_filed_at(filed_at);
        let inserted = self.db.insert_drawer(&drawer, None).await?;

        Ok(json!({
            "recorded": inserted,
            "duplicate": false,
            "drawer": drawer_to_json(drawer),
            "fingerprint": codex_experience_fingerprint(kind, repo_path, arguments),
        }))
    }

    async fn tool_codex_record_command(&self, arguments: &Map<String, Value>) -> Result<Value> {
        let repo_path = required_str(arguments, "repo_path")?;
        let command = required_str(arguments, "command")?;
        let purpose = arguments
            .get("purpose")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("Verification command");
        let result = arguments
            .get("result")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .unwrap_or("recorded");

        let mut card = arguments.clone();
        card.insert("kind".to_string(), Value::String("command".to_string()));
        card.insert(
            "problem".to_string(),
            Value::String(format!("Command: {command}")),
        );
        card.insert(
            "solution".to_string(),
            Value::String(format!("{purpose} -> {result}")),
        );
        card.insert("outcome".to_string(), Value::String(result.to_string()));
        card.entry("commands".to_string())
            .or_insert_with(|| Value::Array(vec![Value::String(command.to_string())]));
        card.entry("details".to_string()).or_insert_with(|| {
            Value::String(format!(
                "cwd: {}\npurpose: {}\nresult: {}",
                arguments
                    .get("cwd")
                    .and_then(Value::as_str)
                    .unwrap_or(repo_path),
                purpose,
                result
            ))
        });

        self.tool_codex_record_experience(&card).await
    }

    async fn tool_codex_record_round_summary(
        &self,
        arguments: &Map<String, Value>,
    ) -> Result<Value> {
        let repo_path = required_str(arguments, "repo_path")?;
        let summary = required_str(arguments, "summary")?;
        let mut card = arguments.clone();
        card.insert(
            "kind".to_string(),
            Value::String("round_summary".to_string()),
        );
        card.insert(
            "problem".to_string(),
            Value::String("Coding round handoff summary".to_string()),
        );
        card.insert("solution".to_string(), Value::String(summary.to_string()));
        card.insert("outcome".to_string(), Value::String(summary.to_string()));
        card.entry("details".to_string()).or_insert_with(|| {
            let mut details = vec![format!("summary: {summary}")];
            if let Some(next_steps) = string_array(arguments.get("next_steps")) {
                details.push("next_steps:".to_string());
                details.extend(next_steps.into_iter().map(|step| format!("- {step}")));
            }
            Value::String(details.join("\n"))
        });

        let mut response = self.tool_codex_record_experience(&card).await?;
        if let Some(object) = response.as_object_mut() {
            object.insert(
                "repo_path".to_string(),
                Value::String(repo_path.to_string()),
            );
        }
        Ok(response)
    }

    async fn tool_codex_search_experience(&self, arguments: &Map<String, Value>) -> Result<Value> {
        let query = required_str(arguments, "query")?;
        let limit = positive_usize(arguments, "limit").unwrap_or(CODEX_DEFAULT_LIMIT);
        let repo_path = arguments.get("repo_path").and_then(Value::as_str);
        let language = arguments.get("language").and_then(Value::as_str);
        let kind = arguments.get("kind").and_then(Value::as_str);
        let mut hits = self
            .codex_search_drawers(query, repo_path, language, kind, limit)
            .await?;
        hits.truncate(limit);

        Ok(json!({
            "query": query,
            "repo_path": repo_path,
            "language": language,
            "kind": kind,
            "results": hits.into_iter().map(drawer_to_json).collect::<Vec<_>>(),
        }))
    }

    async fn tool_codex_context(&self, arguments: &Map<String, Value>) -> Result<Value> {
        let repo_path = required_str(arguments, "repo_path")?;
        let task = arguments
            .get("task")
            .and_then(Value::as_str)
            .unwrap_or(repo_path);
        let limit = positive_usize(arguments, "limit").unwrap_or(CODEX_DEFAULT_LIMIT);

        let profiles = self
            .db
            .fetch_drawers(Some(CODEX_REPO_WING), Some("repo_profile"), 25)
            .await?
            .into_iter()
            .filter(|drawer| {
                drawer.content.contains(repo_path)
                    || drawer.source_file.as_deref() == Some(repo_path)
            })
            .collect::<Vec<_>>();
        let repo_language = profiles
            .iter()
            .find_map(|drawer| codex_card_scalar(&drawer.content, "language"));

        let incidents = rank_codex_drawers(
            self.db
                .fetch_drawers(
                    Some(CODEX_INCIDENT_WING),
                    None,
                    limit.saturating_mul(4).max(16),
                )
                .await?
                .into_iter()
                .filter(|drawer| drawer.content.contains(repo_path))
                .collect(),
            task,
            repo_path,
            repo_language.as_deref(),
        )
        .into_iter()
        .take(limit)
        .collect::<Vec<_>>();

        let relevant = self
            .codex_search_drawers(task, Some(repo_path), repo_language.as_deref(), None, limit)
            .await?;

        Ok(json!({
            "repo_path": repo_path,
            "task": task,
            "protocol": "Read local AGENTS/INCIDENTS/PROGRESS first; use these MCP records as searchable experience, not as authority over live repo files.",
            "repo_profile": profiles.into_iter().map(drawer_to_json).collect::<Vec<_>>(),
            "recent_incidents": incidents.into_iter().map(drawer_to_json).collect::<Vec<_>>(),
            "relevant_experiences": relevant.into_iter().take(limit).map(drawer_to_json).collect::<Vec<_>>(),
        }))
    }

    async fn codex_search_drawers(
        &self,
        query: &str,
        repo_path: Option<&str>,
        language: Option<&str>,
        kind: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Drawer>> {
        let searcher = Searcher::keyword_only(self.db.clone());
        let search_limit = limit.saturating_mul(4).max(16);
        let mut results = Vec::new();

        for wing in [CODEX_EXPERIENCE_WING, CODEX_INCIDENT_WING] {
            let mut drawers = searcher
                .keyword_fallback_search(query, Some(wing), None, search_limit)
                .await?;
            results.append(&mut drawers);
        }

        let mut seen = std::collections::BTreeSet::new();
        results.retain(|drawer| {
            seen.insert(drawer.id.clone())
                && repo_path.is_none_or(|value| {
                    drawer.content.contains(value) || drawer.source_file.as_deref() == Some(value)
                })
                && language
                    .is_none_or(|value| drawer.content.contains(&format!("language: {value}")))
                && kind.is_none_or(|value| drawer.content.contains(&format!("kind: {value}")))
        });

        Ok(rank_codex_drawers(
            results,
            query,
            repo_path.unwrap_or_default(),
            language,
        ))
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

    async fn ensure_embedder(&self) -> Result<Arc<dyn Embedder>> {
        if !self.embedder_loading_enabled {
            anyhow::bail!("embedding disabled for this server state");
        }

        let mut guard = self.embedder.lock().await;
        if let Some(ref embedder) = *guard {
            return Ok(embedder.clone());
        }

        let embedder: Arc<dyn Embedder> =
            Arc::new(LocalEmbedder::new().context("failed to load local embedding model")?);
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
    let mut lines = io::BufReader::new(stdin).lines();

    while let Some(Ok(line)) = lines.next() {
        if line.trim().is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(err) => {
                let response = error_response(Value::Null, -32700, &format!("invalid JSON: {err}"));
                let mut stdout = io::stdout();
                writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
                stdout.flush()?;
                continue;
            }
        };

        if let Some(response) = handle_request(&state, &request).await {
            let mut stdout = io::stdout();
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
        "codex_record_repo" => state.tool_codex_record_repo(&arguments).await,
        "codex_record_experience" => state.tool_codex_record_experience(&arguments).await,
        "codex_record_command" => state.tool_codex_record_command(&arguments).await,
        "codex_record_round_summary" => state.tool_codex_record_round_summary(&arguments).await,
        "codex_delete_experience" => {
            let drawer_id = match arguments.get("drawer_id").and_then(Value::as_str) {
                Some(drawer_id) if !drawer_id.is_empty() => drawer_id,
                _ => {
                    return error_response(
                        req_id,
                        -32602,
                        "codex_delete_experience requires `drawer_id`",
                    );
                }
            };
            state.tool_codex_delete_experience(drawer_id).await
        }
        "codex_search_experience" => state.tool_codex_search_experience(&arguments).await,
        "codex_context" => state.tool_codex_context(&arguments).await,
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
            "Search AiMem. Uses hybrid keyword + vector ranking when an embedder is available, otherwise falls back to keyword search.",
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
        tool_spec(
            "codex_record_repo",
            "Record or update a Codex-edited repository profile for future coding context.",
            json!({
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" },
                    "repo_name": { "type": "string" },
                    "git_remote": { "type": "string" },
                    "git_head": { "type": "string" },
                    "language": { "type": "string" },
                    "summary": { "type": "string" },
                    "repo_root": { "type": "string", "description": "Optional override; otherwise conservatively detected from repo_path." },
                    "manifests": { "type": "array", "items": { "type": "string" } },
                    "test_commands": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["repo_path"]
            }),
        ),
        tool_spec(
            "codex_record_experience",
            "Record a compact reusable coding experience card.",
            json!({
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" },
                    "repo_name": { "type": "string" },
                    "language": { "type": "string" },
                    "kind": { "type": "string" },
                    "problem": { "type": "string" },
                    "solution": { "type": "string" },
                    "outcome": { "type": "string" },
                    "confidence": { "type": "string" },
                    "details": { "type": "string" },
                    "future_rule": { "type": "string" },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "source_files": { "type": "array", "items": { "type": "string" } },
                    "commands": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["repo_path", "problem", "solution"]
            }),
        ),
        tool_spec(
            "codex_record_command",
            "Record a verification or diagnostic command as a reusable Codex experience card.",
            json!({
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" },
                    "repo_name": { "type": "string" },
                    "language": { "type": "string" },
                    "command": { "type": "string" },
                    "cwd": { "type": "string" },
                    "purpose": { "type": "string" },
                    "result": { "type": "string" },
                    "output_summary": { "type": "string" },
                    "tags": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["repo_path", "command"]
            }),
        ),
        tool_spec(
            "codex_record_round_summary",
            "Record a coding-round handoff summary with changed files, tests, and next steps.",
            json!({
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" },
                    "repo_name": { "type": "string" },
                    "language": { "type": "string" },
                    "summary": { "type": "string" },
                    "changed_files": { "type": "array", "items": { "type": "string" } },
                    "commands": { "type": "array", "items": { "type": "string" } },
                    "next_steps": { "type": "array", "items": { "type": "string" } },
                    "tags": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["repo_path", "summary"]
            }),
        ),
        tool_spec(
            "codex_delete_experience",
            "Delete a private Codex experience card by drawer ID.",
            json!({
                "type": "object",
                "properties": {
                    "drawer_id": { "type": "string", "description": "Codex experience drawer ID to remove." }
                },
                "required": ["drawer_id"]
            }),
        ),
        tool_spec(
            "codex_search_experience",
            "Search private Codex coding experience cards.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "repo_path": { "type": "string" },
                    "language": { "type": "string" },
                    "kind": { "type": "string" },
                    "limit": { "type": "integer" }
                },
                "required": ["query"]
            }),
        ),
        tool_spec(
            "codex_context",
            "Return repo-prioritized Codex context and relevant coding experiences for a task.",
            json!({
                "type": "object",
                "properties": {
                    "repo_path": { "type": "string" },
                    "task": { "type": "string" },
                    "limit": { "type": "integer" }
                },
                "required": ["repo_path"]
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
    hybrid_results: Vec<HybridSearchResult>,
    keyword_results: Vec<Drawer>,
) -> Value {
    let results = if !hybrid_results.is_empty() {
        hybrid_results
            .into_iter()
            .map(hybrid_search_result_to_json)
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

fn hybrid_search_result_to_json(result: HybridSearchResult) -> Value {
    json!({
        "id": result.drawer.id,
        "wing": result.drawer.wing,
        "room": result.drawer.room,
        "content": result.drawer.content,
        "parts": result.drawer.parts,
        "source_file": result.drawer.source_file,
        "score": result.score,
        "semantic_similarity": result.semantic_similarity,
        "keyword_score": result.keyword_score,
    })
}

fn rank_codex_drawers(
    mut drawers: Vec<Drawer>,
    query: &str,
    repo_path: &str,
    language: Option<&str>,
) -> Vec<Drawer> {
    drawers.sort_by(|a, b| {
        let a_score = codex_rank_score(a, query, repo_path, language);
        let b_score = codex_rank_score(b, query, repo_path, language);
        b_score
            .cmp(&a_score)
            .then_with(|| b.filed_at.cmp(&a.filed_at))
            .then_with(|| a.id.cmp(&b.id))
    });
    drawers
}

fn codex_rank_score(drawer: &Drawer, query: &str, repo_path: &str, language: Option<&str>) -> i64 {
    let mut score = 0;
    if !repo_path.is_empty()
        && (drawer.content.contains(repo_path) || drawer.source_file.as_deref() == Some(repo_path))
    {
        score += 1_000;
    }
    if drawer.wing == CODEX_INCIDENT_WING || drawer.room == "incident" {
        score += 250;
    }
    if let Some(language) = language {
        if drawer.content.contains(&format!("language: {language}")) {
            score += 150;
        }
    }
    score += codex_keyword_overlap(query, &drawer.content) as i64 * 20;
    score
}

fn codex_keyword_overlap(query: &str, content: &str) -> usize {
    let content = content.to_ascii_lowercase();
    query
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| token.len() >= 3)
        .map(str::to_ascii_lowercase)
        .filter(|token| content.contains(token))
        .count()
}

fn codex_card_scalar(content: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}: ");
    content
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn detect_repo_profile(repo_path: &str) -> Map<String, Value> {
    let mut profile = Map::new();
    let Some(root) = detect_repo_root(Path::new(repo_path)) else {
        return profile;
    };

    profile.insert(
        "repo_root".to_string(),
        Value::String(root.display().to_string()),
    );
    if let Some(name) = root.file_name().and_then(|name| name.to_str()) {
        profile.insert("repo_name".to_string(), Value::String(name.to_string()));
    }

    let manifests = detect_manifests(&root);
    if !manifests.is_empty() {
        profile.insert(
            "manifests".to_string(),
            Value::Array(manifests.iter().cloned().map(Value::String).collect()),
        );
    }
    if let Some(language) = detect_language(&manifests) {
        profile.insert("language".to_string(), Value::String(language.to_string()));
    }
    let test_commands = detect_test_commands(&manifests);
    if !test_commands.is_empty() {
        profile.insert(
            "test_commands".to_string(),
            Value::Array(test_commands.into_iter().map(Value::String).collect()),
        );
    }

    if let Some(git_head) = read_git_head(&root) {
        profile.insert("git_head".to_string(), Value::String(git_head));
    }
    if let Some(remote) = read_git_remote(&root) {
        profile.insert("git_remote".to_string(), Value::String(remote));
    }

    profile
}

fn merge_detected_repo_profile(
    arguments: &Map<String, Value>,
    mut detected: Map<String, Value>,
) -> Map<String, Value> {
    for (key, value) in arguments {
        detected.insert(key.clone(), value.clone());
    }
    detected
}

fn detect_repo_root(path: &Path) -> Option<PathBuf> {
    let mut current = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };

    loop {
        if current.join(".git").exists() || has_known_manifest(&current) {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn has_known_manifest(dir: &Path) -> bool {
    [
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "go.mod",
        "pom.xml",
        "build.gradle",
        "settings.gradle",
        "deno.json",
    ]
    .iter()
    .any(|name| dir.join(name).is_file())
}

fn detect_manifests(root: &Path) -> Vec<String> {
    [
        "Cargo.toml",
        "package.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "package-lock.json",
        "pyproject.toml",
        "go.mod",
        "pom.xml",
        "build.gradle",
        "settings.gradle",
        "deno.json",
    ]
    .iter()
    .filter(|name| root.join(name).is_file())
    .map(|name| name.to_string())
    .collect()
}

fn detect_language(manifests: &[String]) -> Option<&'static str> {
    if manifests.iter().any(|item| item == "Cargo.toml") {
        Some("rust")
    } else if manifests.iter().any(|item| item == "package.json") {
        Some("javascript/typescript")
    } else if manifests.iter().any(|item| item == "pyproject.toml") {
        Some("python")
    } else if manifests.iter().any(|item| item == "go.mod") {
        Some("go")
    } else if manifests.iter().any(|item| {
        matches!(
            item.as_str(),
            "pom.xml" | "build.gradle" | "settings.gradle"
        )
    }) {
        Some("jvm")
    } else if manifests.iter().any(|item| item == "deno.json") {
        Some("deno")
    } else {
        None
    }
}

fn detect_test_commands(manifests: &[String]) -> Vec<String> {
    let mut commands = Vec::new();
    if manifests.iter().any(|item| item == "Cargo.toml") {
        commands.push("cargo test".to_string());
    }
    if manifests.iter().any(|item| item == "package.json") {
        commands.push("npm test".to_string());
    }
    if manifests.iter().any(|item| item == "pnpm-lock.yaml") {
        commands.push("pnpm test".to_string());
    }
    if manifests.iter().any(|item| item == "pyproject.toml") {
        commands.push("pytest".to_string());
    }
    if manifests.iter().any(|item| item == "go.mod") {
        commands.push("go test ./...".to_string());
    }
    if manifests.iter().any(|item| item == "pom.xml") {
        commands.push("mvn test".to_string());
    }
    if manifests
        .iter()
        .any(|item| matches!(item.as_str(), "build.gradle" | "settings.gradle"))
    {
        commands.push("gradle test".to_string());
    }
    if manifests.iter().any(|item| item == "deno.json") {
        commands.push("deno test".to_string());
    }
    commands
}

fn read_git_head(root: &Path) -> Option<String> {
    let git = root.join(".git");
    let head_path = if git.is_dir() {
        git.join("HEAD")
    } else {
        return None;
    };
    let head = fs::read_to_string(head_path).ok()?;
    let head = head.trim();
    if let Some(reference) = head.strip_prefix("ref: ") {
        let hash = fs::read_to_string(git.join(reference)).ok();
        if let Some(hash) = hash {
            return Some(format!("{} {}", reference, hash.trim()));
        }
    }
    Some(head.to_string())
}

fn read_git_remote(root: &Path) -> Option<String> {
    let config = fs::read_to_string(root.join(".git").join("config")).ok()?;
    let mut in_origin = false;
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_origin = trimmed == r#"[remote "origin"]"#;
            continue;
        }
        if in_origin {
            if let Some(url) = trimmed.strip_prefix("url =") {
                return Some(url.trim().to_string());
            }
        }
    }
    None
}

fn required_str<'a>(arguments: &'a Map<String, Value>, key: &str) -> Result<&'a str> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("missing required `{key}`"))
}

fn codex_room_for_kind(kind: &str) -> &'static str {
    match kind {
        "incident" => "incident",
        "decision" => "architecture",
        "command" => "workflow",
        "round_summary" => "workflow",
        "release" => "release",
        "dependency" => "dependency",
        "test" | "testing" => "testing",
        "style" => "style",
        _ => "workflow",
    }
}

fn codex_stable_id(kind: &str, repo_path: &str) -> String {
    let digest = md5::compute(format!("codex\u{1f}{kind}\u{1f}{repo_path}").as_bytes());
    format!("codex_{kind}_{digest:x}")
}

fn codex_experience_id(kind: &str, repo_path: &str, arguments: &Map<String, Value>) -> String {
    let fingerprint = codex_experience_fingerprint(kind, repo_path, arguments);
    let digest = md5::compute(fingerprint.as_bytes());
    format!(
        "codex_{}_{}",
        slugish(kind),
        hex_prefix(&format!("{digest:x}"), 24)
    )
}

fn codex_experience_fingerprint(
    kind: &str,
    repo_path: &str,
    arguments: &Map<String, Value>,
) -> String {
    let problem = arguments
        .get("problem")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let solution = arguments
        .get("solution")
        .and_then(Value::as_str)
        .unwrap_or_default();
    [kind, repo_path, problem, solution]
        .into_iter()
        .map(canonical_fingerprint_part)
        .collect::<Vec<_>>()
        .join("\u{1f}")
}

fn canonical_fingerprint_part(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn hex_prefix(value: &str, len: usize) -> &str {
    let end = value.len().min(len);
    &value[..end]
}

fn format_codex_card(
    kind: &str,
    repo_path: &str,
    arguments: &Map<String, Value>,
    scalar_keys: &[&str],
    filed_at: &str,
) -> String {
    let mut lines = Vec::new();
    lines.push(format!("repo_path: {}", sanitize_codex_value(repo_path)));
    lines.push(format!("kind: {}", sanitize_codex_value(kind)));
    lines.push(format!("created_at: {filed_at}"));

    for key in scalar_keys {
        if let Some(value) = arguments.get(*key).and_then(Value::as_str) {
            if !value.is_empty() {
                lines.push(format!("{key}: {}", sanitize_codex_value(value)));
            }
        }
    }

    for key in [
        "tags",
        "source_files",
        "commands",
        "manifests",
        "test_commands",
        "changed_files",
        "next_steps",
    ] {
        if let Some(items) = string_array(arguments.get(key)) {
            if !items.is_empty() {
                lines.push(format!("{key}:"));
                for item in items {
                    lines.push(format!("  - {}", sanitize_codex_value(&item)));
                }
            }
        }
    }

    lines.push("---".to_string());
    if let Some(details) = arguments.get("details").and_then(Value::as_str) {
        if !details.is_empty() {
            lines.push("Details:".to_string());
            lines.push(sanitize_codex_value(details));
        }
    }
    if let Some(rule) = arguments.get("future_rule").and_then(Value::as_str) {
        if !rule.is_empty() {
            lines.push("Future Codex rule:".to_string());
            lines.push(format!("- {}", sanitize_codex_value(rule)));
        }
    }

    lines.join("\n")
}

fn string_array(value: Option<&Value>) -> Option<Vec<String>> {
    let array = value?.as_array()?;
    Some(
        array
            .iter()
            .filter_map(Value::as_str)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect(),
    )
}

fn sanitize_codex_value(value: &str) -> String {
    value
        .lines()
        .map(redact_secretish_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_secretish_line(line: &str) -> String {
    let upper = line.to_ascii_uppercase();
    let secretish = ["TOKEN", "SECRET", "PASSWORD", "API_KEY", "PRIVATE KEY"];
    if secretish.iter().any(|needle| upper.contains(needle)) {
        if let Some((key, _)) = line.split_once('=') {
            return format!("{}=<redacted>", key.trim());
        }
        if let Some((key, _)) = line.split_once(':') {
            return format!("{}: <redacted>", key.trim());
        }
        return "<redacted secret-like line>".to_string();
    }
    line.to_string()
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
        let drawer = Drawer::new(
            "drawer_mcp_test_001",
            "demo_app",
            "backend",
            "Turso keeps AiMem local and searchable.",
            "test",
        )
        .with_source_file("README.md")
        .with_filed_at("2026-04-07T00:00:00Z");
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
        assert!(names.contains(&"codex_record_repo"));
        assert!(names.contains(&"codex_record_experience"));
        assert!(names.contains(&"codex_record_command"));
        assert!(names.contains(&"codex_record_round_summary"));
        assert!(names.contains(&"codex_delete_experience"));
        assert!(names.contains(&"codex_search_experience"));
        assert!(names.contains(&"codex_context"));
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
        assert_eq!(parsed["embedding_profile"]["provider"], Value::Null);
        assert_eq!(parsed["embedding_profile"]["model"], Value::Null);
        assert_eq!(parsed["embedding_profile"]["dimension"], Value::Null);
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
    async fn codex_record_repo_upserts_profile() {
        let state = test_state("codex-repo").await;

        let first = call_tool(
            &state,
            "codex_record_repo",
            json!({
                "repo_path": "/tmp/demo",
                "repo_name": "demo",
                "language": "rust",
                "summary": "Private MCP test repo."
            }),
        )
        .await;
        let second = call_tool(
            &state,
            "codex_record_repo",
            json!({
                "repo_path": "/tmp/demo",
                "repo_name": "demo",
                "language": "rust",
                "summary": "Updated profile."
            }),
        )
        .await;

        assert_eq!(first["recorded"], true);
        assert_eq!(second["recorded"], true);
        assert_eq!(state.db.drawer_count().await.expect("drawer count"), 1);
        assert!(
            second["drawer"]["content"]
                .as_str()
                .expect("content")
                .contains("Updated profile")
        );
    }

    #[tokio::test]
    async fn codex_record_repo_detects_conservative_repo_profile() {
        let state = test_state("codex-repo-detect").await;
        let (db_path, _) = test_paths("codex-repo-detect-fixture");
        let root = db_path.parent().expect("temp dir").join("repo");
        fs::create_dir_all(root.join(".git").join("refs").join("heads")).expect("create git dirs");
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").expect("write manifest");
        fs::write(root.join(".git").join("HEAD"), "ref: refs/heads/main\n").expect("write head");
        fs::write(
            root.join(".git").join("refs").join("heads").join("main"),
            "0123456789abcdef0123456789abcdef01234567\n",
        )
        .expect("write ref");
        fs::write(
            root.join(".git").join("config"),
            "[remote \"origin\"]\n\turl = git@example.test:demo/repo.git\n",
        )
        .expect("write config");

        let parsed = call_tool(
            &state,
            "codex_record_repo",
            json!({ "repo_path": root.join("src").display().to_string() }),
        )
        .await;
        let content = parsed["drawer"]["content"].as_str().expect("content");

        assert!(content.contains("repo_root:"));
        assert!(content.contains("language: rust"));
        assert!(content.contains("git_remote: git@example.test:demo/repo.git"));
        assert!(content.contains("refs/heads/main"));
        assert!(content.contains("manifests:\n  - Cargo.toml"));
        assert!(content.contains("test_commands:\n  - cargo test"));
    }
    #[tokio::test]
    async fn codex_record_and_search_experience() {
        let state = test_state("codex-experience").await;

        let recorded = call_tool(
            &state,
            "codex_record_experience",
            json!({
                "repo_path": "/tmp/aimem",
                "repo_name": "aimem",
                "language": "rust",
                "kind": "dependency",
                "problem": "Turso 0.6.1 requires the fts feature for USING fts indexes.",
                "solution": "Enable the turso fts feature explicitly when default features are disabled.",
                "outcome": "MCP tests pass again.",
                "confidence": "high",
                "source_files": ["Cargo.toml"],
                "commands": ["cargo test -p aimem-mcp"]
            }),
        )
        .await;

        assert_eq!(recorded["recorded"], true);
        assert_eq!(recorded["drawer"]["wing"], CODEX_EXPERIENCE_WING);

        let found = call_tool(
            &state,
            "codex_search_experience",
            json!({
                "query": "Turso fts feature",
                "repo_path": "/tmp/aimem",
                "language": "rust",
                "kind": "dependency",
                "limit": 3
            }),
        )
        .await;

        let results = found["results"].as_array().expect("results array");
        assert_eq!(results.len(), 1);
        assert!(
            results[0]["content"]
                .as_str()
                .expect("content")
                .contains("fts feature")
        );
    }

    #[tokio::test]
    async fn codex_record_experience_deduplicates_by_stable_fingerprint() {
        let state = test_state("codex-dedupe").await;
        let args = json!({
            "repo_path": "/tmp/aimem",
            "language": "rust",
            "kind": "testing",
            "problem": "MCP handlers need stable duplicate checks.",
            "solution": "Hash repo, kind, problem, and solution instead of timestamped content.",
            "outcome": "First write wins."
        });

        let first = call_tool(&state, "codex_record_experience", args.clone()).await;
        let second = call_tool(&state, "codex_record_experience", args).await;

        assert_eq!(first["recorded"], true);
        assert_eq!(second["recorded"], false);
        assert_eq!(second["duplicate"], true);
        assert_eq!(first["fingerprint"], second["fingerprint"]);
        assert_eq!(state.db.drawer_count().await.expect("drawer count"), 1);
    }
    #[tokio::test]
    async fn codex_context_returns_profile_and_relevant_experience() {
        let state = test_state("codex-context").await;

        call_tool(
            &state,
            "codex_record_repo",
            json!({
                "repo_path": "/tmp/context-demo",
                "repo_name": "context-demo",
                "language": "rust",
                "summary": "Demo repo profile."
            }),
        )
        .await;
        call_tool(
            &state,
            "codex_record_experience",
            json!({
                "repo_path": "/tmp/context-demo",
                "language": "rust",
                "kind": "testing",
                "problem": "MCP handlers need regression tests.",
                "solution": "Call tools through JSON-RPC helper tests.",
                "outcome": "Tool payloads are verified."
            }),
        )
        .await;

        let context = call_tool(
            &state,
            "codex_context",
            json!({
                "repo_path": "/tmp/context-demo",
                "task": "MCP regression tests",
                "limit": 5
            }),
        )
        .await;

        assert_eq!(
            context["repo_profile"].as_array().expect("profiles").len(),
            1
        );
        assert_eq!(
            context["relevant_experiences"]
                .as_array()
                .expect("experiences")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn codex_record_command_is_searchable() {
        let state = test_state("codex-command").await;

        let recorded = call_tool(
            &state,
            "codex_record_command",
            json!({
                "repo_path": "/tmp/aimem",
                "language": "rust",
                "command": "cargo test -p aimem-mcp",
                "cwd": "/tmp/aimem",
                "purpose": "Verify MCP tool handlers",
                "result": "passed",
                "output_summary": "13 tests passed"
            }),
        )
        .await;

        assert_eq!(recorded["recorded"], true);
        assert!(
            recorded["drawer"]["content"]
                .as_str()
                .expect("content")
                .contains("kind: command")
        );

        let found = call_tool(
            &state,
            "codex_search_experience",
            json!({
                "query": "cargo test aimem mcp",
                "repo_path": "/tmp/aimem",
                "kind": "command",
                "limit": 3
            }),
        )
        .await;
        assert_eq!(found["results"].as_array().expect("results").len(), 1);
    }

    #[tokio::test]
    async fn codex_record_round_summary_is_searchable() {
        let state = test_state("codex-round").await;

        let recorded = call_tool(
            &state,
            "codex_record_round_summary",
            json!({
                "repo_path": "/tmp/aimem",
                "language": "rust",
                "summary": "Implemented private MCP command and round summary tools.",
                "changed_files": ["crates/aimem-mcp/src/main.rs"],
                "commands": ["cargo test -p aimem-mcp"],
                "next_steps": ["Add smoke-test examples"]
            }),
        )
        .await;

        assert_eq!(recorded["recorded"], true);
        let content = recorded["drawer"]["content"].as_str().expect("content");
        assert!(content.contains("kind: round_summary"));
        assert!(content.contains("changed_files:"));
        assert!(content.contains("next_steps:"));

        let found = call_tool(
            &state,
            "codex_search_experience",
            json!({
                "query": "private MCP command round summary",
                "repo_path": "/tmp/aimem",
                "kind": "round_summary",
                "limit": 3
            }),
        )
        .await;
        assert_eq!(found["results"].as_array().expect("results").len(), 1);
    }
    #[tokio::test]
    async fn codex_context_ranks_same_repo_and_incidents_first() {
        let state = test_state("codex-rank").await;

        call_tool(
            &state,
            "codex_record_repo",
            json!({
                "repo_path": "/tmp/ranked",
                "repo_name": "ranked",
                "language": "rust",
                "summary": "Ranking demo."
            }),
        )
        .await;
        call_tool(
            &state,
            "codex_record_experience",
            json!({
                "repo_path": "/tmp/other",
                "language": "rust",
                "kind": "testing",
                "problem": "MCP ranking regression tests need helpers.",
                "solution": "Use another repo to ensure same-repo boost wins."
            }),
        )
        .await;
        call_tool(
            &state,
            "codex_record_experience",
            json!({
                "repo_path": "/tmp/ranked",
                "language": "rust",
                "kind": "testing",
                "problem": "MCP ranking regression tests need helpers.",
                "solution": "Same repo records should outrank cross-repo records."
            }),
        )
        .await;
        call_tool(
            &state,
            "codex_record_experience",
            json!({
                "repo_path": "/tmp/ranked",
                "language": "rust",
                "kind": "incident",
                "problem": "MCP ranking regression tests failed before handoff.",
                "solution": "Surface incidents before ordinary experience cards."
            }),
        )
        .await;

        let context = call_tool(
            &state,
            "codex_context",
            json!({
                "repo_path": "/tmp/ranked",
                "task": "MCP ranking regression tests",
                "limit": 5
            }),
        )
        .await;
        let experiences = context["relevant_experiences"]
            .as_array()
            .expect("experiences");

        assert!(experiences.len() >= 2);
        assert_eq!(experiences[0]["wing"], CODEX_INCIDENT_WING);
        assert!(
            experiences[0]["content"]
                .as_str()
                .expect("content")
                .contains("/tmp/ranked")
        );
        assert!(
            experiences[1]["content"]
                .as_str()
                .expect("content")
                .contains("/tmp/ranked")
        );
    }

    #[tokio::test]
    async fn codex_delete_experience_removes_card() {
        let state = test_state("codex-delete").await;

        let recorded = call_tool(
            &state,
            "codex_record_experience",
            json!({
                "repo_path": "/tmp/delete-demo",
                "kind": "testing",
                "problem": "Temporary Codex memory needs cleanup.",
                "solution": "Delete it by drawer ID."
            }),
        )
        .await;
        let drawer_id = recorded["drawer"]["id"].as_str().expect("drawer id");

        let deleted = call_tool(
            &state,
            "codex_delete_experience",
            json!({ "drawer_id": drawer_id }),
        )
        .await;

        assert_eq!(deleted["deleted"], true);
        assert_eq!(state.db.drawer_count().await.expect("drawer count"), 0);
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
