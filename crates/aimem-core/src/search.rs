//! Semantic + keyword search over the AiMem store.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::{
    db::{AimemDb, DbError},
    embedder::Embedder,
    types::{Drawer, HybridSearchResult, KeywordSearchResult, SearchResult},
};
use thiserror::Error;
use tracing::warn;

const HYBRID_RRF_K: f32 = 60.0;
const HYBRID_VECTOR_WEIGHT: f32 = 1.0;
const HYBRID_KEYWORD_WEIGHT: f32 = 1.0;
const HYBRID_CANDIDATE_MULTIPLIER: usize = 4;
const HYBRID_MIN_CANDIDATES: usize = 12;
const KEYWORD_FALLBACK_CANDIDATE_MULTIPLIER: usize = 64;
const KEYWORD_FALLBACK_MIN_CANDIDATES: usize = 256;
const KEYWORD_FALLBACK_MAX_CANDIDATES: usize = 4096;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("db error: {0}")]
    Db(#[from] DbError),
    #[error("embed error: {0}")]
    Embed(#[from] crate::embedder::EmbedError),
    #[error("embedder returned {actual} vectors for {expected} inputs")]
    EmbedBatchSizeMismatch { expected: usize, actual: usize },
    #[error("semantic search requires an embedder")]
    EmbedderUnavailable,
    #[error("turso error: {0}")]
    Turso(#[from] turso::Error),
}

/// Searcher — vector + keyword search over the AiMem store.
#[derive(Clone)]
pub struct Searcher {
    db: AimemDb,
    embedder: Option<Arc<dyn Embedder>>,
}

impl std::fmt::Debug for Searcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Searcher")
            .field("db", &self.db)
            .field(
                "embedder",
                &self.embedder.as_ref().map(|_| "Some(Arc<dyn Embedder>)"),
            )
            .finish()
    }
}

impl Searcher {
    pub fn new(db: AimemDb, embedder: Arc<dyn Embedder>) -> Self {
        Self {
            db,
            embedder: Some(embedder),
        }
    }

    pub fn keyword_only(db: AimemDb) -> Self {
        Self { db, embedder: None }
    }

    pub fn embedder(&self) -> Option<Arc<dyn Embedder>> {
        self.embedder.clone()
    }

    /// Semantic vector search using Turso's `vector_distance_cos`.
    pub async fn vector_search(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let embedder = self
            .embedder
            .as_ref()
            .ok_or(SearchError::EmbedderUnavailable)?;
        let qvec = embedder.embed_one(query).await?;
        self.db
            .assert_embedding_profile(qvec.len(), embedder.provider_name(), embedder.model_name())
            .await?;
        let qvec_json = crate::embedder::to_vector32_json(&qvec);

        let conn = self.db.read_conn()?;
        let sql = build_vector_sql(wing, room);
        let mut rows = match (wing, room) {
            (Some(w), Some(r)) => {
                conn.query(&sql, turso::params![qvec_json.as_str(), w, r, limit as i64])
                    .await?
            }
            (Some(w), None) => {
                conn.query(&sql, turso::params![qvec_json.as_str(), w, limit as i64])
                    .await?
            }
            (None, Some(r)) => {
                conn.query(&sql, turso::params![qvec_json.as_str(), r, limit as i64])
                    .await?
            }
            (None, None) => {
                conn.query(&sql, turso::params![qvec_json.as_str(), limit as i64])
                    .await?
            }
        };

        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            let drawer = row_to_drawer(&row)?;
            let dist = match row.get_value(9)? {
                turso::Value::Real(f) => f as f32,
                turso::Value::Integer(i) => i as f32,
                _ => 1.0,
            };
            let similarity = (1.0_f32 - dist).clamp(0.0, 1.0);
            results.push(SearchResult { drawer, similarity });
        }
        Ok(results)
    }

    /// Keyword search with scores.
    ///
    /// Uses Turso FTS/Tantivy when available, and falls back to LIKE-based
    /// scoring for resilience or substring-style queries.
    pub async fn keyword_search_scored(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> Result<Vec<KeywordSearchResult>, SearchError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        match self.keyword_fts_search(query, wing, room, limit).await {
            Ok(results) if !results.is_empty() => Ok(results),
            Ok(_) => self.keyword_like_search(query, wing, room, limit).await,
            Err(err) if should_fallback_to_like_search(&err) => {
                warn!(
                    error = %err,
                    query,
                    "AiMem FTS keyword search unavailable; falling back to LIKE search"
                );
                self.keyword_like_search(query, wing, room, limit).await
            }
            Err(err) => Err(err),
        }
    }

    /// Keyword search returning drawers only.
    pub async fn keyword_search(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Drawer>, SearchError> {
        Ok(self
            .keyword_search_scored(query, wing, room, limit)
            .await?
            .into_iter()
            .map(|result| result.drawer)
            .collect())
    }

    /// Keyword search with token fallback.
    pub async fn keyword_fallback_search(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Drawer>, SearchError> {
        let exact = self.keyword_search(query, wing, room, limit).await?;
        if !exact.is_empty() || limit == 0 {
            return Ok(exact);
        }

        let mut merged = Vec::new();
        let mut seen = HashSet::new();

        for token in keyword_tokens(query) {
            for drawer in self.keyword_search(&token, wing, room, limit).await? {
                if seen.insert(drawer.id.clone()) {
                    merged.push(drawer);
                    if merged.len() >= limit {
                        return Ok(merged);
                    }
                }
            }
        }

        Ok(merged)
    }

    /// Hybrid keyword + vector search fused with reciprocal-rank fusion (RRF).
    pub async fn hybrid_search(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> Result<Vec<HybridSearchResult>, SearchError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let candidate_limit = limit
            .saturating_mul(HYBRID_CANDIDATE_MULTIPLIER)
            .max(limit)
            .max(HYBRID_MIN_CANDIDATES);

        if self.embedder.is_none() {
            return Ok(fuse_hybrid_results(
                self.keyword_search_scored(query, wing, room, candidate_limit)
                    .await?,
                Vec::new(),
                limit,
            ));
        }

        let keyword_future = self.keyword_search_scored(query, wing, room, candidate_limit);
        let vector_future = self.vector_search(query, wing, room, candidate_limit);
        let (keyword_results, vector_results) = tokio::join!(keyword_future, vector_future);

        match (keyword_results, vector_results) {
            (Ok(keyword_hits), Ok(vector_hits)) => {
                Ok(fuse_hybrid_results(keyword_hits, vector_hits, limit))
            }
            (Ok(keyword_hits), Err(err)) => {
                warn!(
                    error = %err,
                    query,
                    "AiMem vector branch failed during hybrid search; returning keyword-only results"
                );
                Ok(fuse_hybrid_results(keyword_hits, Vec::new(), limit))
            }
            (Err(err), Ok(vector_hits)) => {
                warn!(
                    error = %err,
                    query,
                    "AiMem keyword branch failed during hybrid search; returning vector-only results"
                );
                Ok(fuse_hybrid_results(Vec::new(), vector_hits, limit))
            }
            (Err(keyword_err), Err(vector_err)) => {
                warn!(
                    keyword_error = %keyword_err,
                    vector_error = %vector_err,
                    query,
                    "AiMem hybrid search failed in both keyword and vector branches"
                );
                Err(keyword_err)
            }
        }
    }

    /// Duplicate check.
    pub async fn find_duplicates(
        &self,
        content: &str,
        threshold: f32,
        limit: usize,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let results = self.vector_search(content, None, None, limit).await?;
        Ok(results
            .into_iter()
            .filter(|r| r.similarity >= threshold)
            .collect())
    }

    async fn keyword_fts_search(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> Result<Vec<KeywordSearchResult>, SearchError> {
        // Turso's FTS/custom-index path may perform internal index work while
        // evaluating fts_match/fts_score, so it cannot run on the query_only
        // read connection even though this is logically a read query.
        let conn = self.db.conn()?;
        let sql = build_keyword_fts_sql(wing, room);

        let mut rows = match (wing, room) {
            (Some(w), Some(r)) => {
                conn.query(&sql, turso::params![query, w, r, limit as i64])
                    .await?
            }
            (Some(w), None) => {
                conn.query(&sql, turso::params![query, w, limit as i64])
                    .await?
            }
            (None, Some(r)) => {
                conn.query(&sql, turso::params![query, r, limit as i64])
                    .await?
            }
            (None, None) => {
                conn.query(&sql, turso::params![query, limit as i64])
                    .await?
            }
        };

        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            let drawer = row_to_drawer(&row)?;
            let score = match row.get_value(9)? {
                turso::Value::Real(f) => f as f32,
                turso::Value::Integer(i) => i as f32,
                _ => 0.0,
            };
            results.push(KeywordSearchResult { drawer, score });
        }
        Ok(results)
    }

    async fn keyword_like_search(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        limit: usize,
    ) -> Result<Vec<KeywordSearchResult>, SearchError> {
        let conn = self.db.read_conn()?;
        let candidate_limit = limit
            .saturating_mul(KEYWORD_FALLBACK_CANDIDATE_MULTIPLIER)
            .max(limit)
            .max(KEYWORD_FALLBACK_MIN_CANDIDATES)
            .min(KEYWORD_FALLBACK_MAX_CANDIDATES);
        let sql = build_keyword_fallback_sql(wing, room);

        let mut rows = match (wing, room) {
            (Some(w), Some(r)) => {
                conn.query(&sql, turso::params![w, r, candidate_limit as i64])
                    .await?
            }
            (Some(w), None) => {
                conn.query(&sql, turso::params![w, candidate_limit as i64])
                    .await?
            }
            (None, Some(r)) => {
                conn.query(&sql, turso::params![r, candidate_limit as i64])
                    .await?
            }
            (None, None) => {
                conn.query(&sql, turso::params![candidate_limit as i64])
                    .await?
            }
        };

        let mut results = Vec::new();
        while let Some(row) = rows.next().await? {
            let drawer = row_to_drawer(&row)?;
            let score = like_match_score(&drawer, query);
            if score > 0.0 {
                results.push(KeywordSearchResult { score, drawer });
            }
        }

        results.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| right.drawer.filed_at.cmp(&left.drawer.filed_at))
        });
        results.truncate(limit);
        Ok(results)
    }
}

#[derive(Debug)]
struct HybridAccumulator {
    drawer: Drawer,
    raw_rrf: f32,
    semantic_similarity: Option<f32>,
    keyword_score: Option<f32>,
}

// ── SQL builders ──────────────────────────────────────────────────────────────

fn build_vector_sql(wing: Option<&str>, room: Option<&str>) -> String {
    let filter = match (wing, room) {
        (Some(_), Some(_)) => "AND wing = ?2 AND room = ?3",
        (Some(_), None) => "AND wing = ?2",
        (None, Some(_)) => "AND room = ?2",
        (None, None) => "",
    };
    let limit_param = match (wing, room) {
        (Some(_), Some(_)) => "?4",
        (Some(_), None) | (None, Some(_)) => "?3",
        (None, None) => "?2",
    };
    format!(
        "SELECT id, wing, room, content, parts, source_file, chunk_index, added_by, filed_at, \
         vector_distance_cos(embedding, vector32(?1)) AS dist \
         FROM drawers \
         WHERE embedding IS NOT NULL {filter} \
         ORDER BY dist LIMIT {limit_param}"
    )
}

fn build_keyword_fts_sql(wing: Option<&str>, room: Option<&str>) -> String {
    let filter = match (wing, room) {
        (Some(_), Some(_)) => "AND d.wing = ?2 AND d.room = ?3",
        (Some(_), None) => "AND d.wing = ?2",
        (None, Some(_)) => "AND d.room = ?2",
        (None, None) => "",
    };
    let limit_param = match (wing, room) {
        (Some(_), Some(_)) => "?4",
        (Some(_), None) | (None, Some(_)) => "?3",
        (None, None) => "?2",
    };
    format!(
        "SELECT d.id, d.wing, d.room, d.content, d.parts, d.source_file, d.chunk_index, d.added_by, d.filed_at, \
         fts_score(search_text, ?1) AS score \
         FROM drawers_fts \
         JOIN drawers d ON d.id = drawers_fts.drawer_id \
         WHERE fts_match(search_text, ?1) {filter} \
         ORDER BY score DESC LIMIT {limit_param}"
    )
}

fn build_keyword_fallback_sql(wing: Option<&str>, room: Option<&str>) -> String {
    let filter = match (wing, room) {
        (Some(_), Some(_)) => "WHERE wing = ?1 AND room = ?2",
        (Some(_), None) => "WHERE wing = ?1",
        (None, Some(_)) => "WHERE room = ?1",
        (None, None) => "",
    };
    let limit_param = match (wing, room) {
        (Some(_), Some(_)) => "?3",
        (Some(_), None) | (None, Some(_)) => "?2",
        (None, None) => "?1",
    };
    format!(
        "SELECT id, wing, room, content, parts, source_file, chunk_index, added_by, filed_at \
         FROM drawers \
         {filter} \
         ORDER BY filed_at DESC LIMIT {limit_param}"
    )
}

fn fuse_hybrid_results(
    keyword_hits: Vec<KeywordSearchResult>,
    vector_hits: Vec<SearchResult>,
    limit: usize,
) -> Vec<HybridSearchResult> {
    if limit == 0 {
        return Vec::new();
    }

    let mut fused = HashMap::<String, HybridAccumulator>::new();

    for (rank, result) in vector_hits.into_iter().enumerate() {
        let id = result.drawer.id.clone();
        let similarity = result.similarity;
        let entry = fused.entry(id).or_insert_with(|| HybridAccumulator {
            drawer: result.drawer,
            raw_rrf: 0.0,
            semantic_similarity: None,
            keyword_score: None,
        });
        entry.raw_rrf += rrf_component(rank, HYBRID_VECTOR_WEIGHT);
        entry.semantic_similarity = Some(
            entry
                .semantic_similarity
                .map(|current| current.max(similarity))
                .unwrap_or(similarity),
        );
    }

    for (rank, result) in keyword_hits.into_iter().enumerate() {
        let id = result.drawer.id.clone();
        let score = result.score;
        let entry = fused.entry(id).or_insert_with(|| HybridAccumulator {
            drawer: result.drawer,
            raw_rrf: 0.0,
            semantic_similarity: None,
            keyword_score: None,
        });
        entry.raw_rrf +=
            rrf_component(rank, HYBRID_KEYWORD_WEIGHT) * keyword_score_multiplier(score);
        entry.keyword_score = Some(
            entry
                .keyword_score
                .map(|current| current.max(score))
                .unwrap_or(score),
        );
    }

    let total_weight = (if fused
        .values()
        .any(|entry| entry.semantic_similarity.is_some())
    {
        HYBRID_VECTOR_WEIGHT
    } else {
        0.0
    }) + (if fused.values().any(|entry| entry.keyword_score.is_some()) {
        HYBRID_KEYWORD_WEIGHT
    } else {
        0.0
    });

    if total_weight <= 0.0 {
        return Vec::new();
    }

    let scale = (HYBRID_RRF_K + 1.0) / total_weight;
    let mut results = fused
        .into_values()
        .map(|entry| HybridSearchResult {
            drawer: entry.drawer,
            score: (entry.raw_rrf * scale).clamp(0.0, 1.0),
            semantic_similarity: entry.semantic_similarity,
            keyword_score: entry.keyword_score,
        })
        .collect::<Vec<_>>();

    results.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                right
                    .keyword_score
                    .unwrap_or_default()
                    .partial_cmp(&left.keyword_score.unwrap_or_default())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                right
                    .semantic_similarity
                    .unwrap_or_default()
                    .partial_cmp(&left.semantic_similarity.unwrap_or_default())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| right.drawer.filed_at.cmp(&left.drawer.filed_at))
    });
    results.truncate(limit);
    results
}

fn rrf_component(rank: usize, weight: f32) -> f32 {
    weight / (HYBRID_RRF_K + rank as f32 + 1.0)
}

fn keyword_score_multiplier(score: f32) -> f32 {
    1.0 + (score / (score + 1.0)).clamp(0.0, 1.0)
}

fn keyword_tokens(query: &str) -> Vec<String> {
    let normalized = normalize_keyword_text(query);
    let mut tokens = normalized
        .split_whitespace()
        .map(|part| {
            part.trim_matches(|c: char| !is_keyword_char(c) && c != '_' && c != '-')
                .to_string()
        })
        .filter(|part| part.len() >= 2)
        .collect::<Vec<_>>();

    let compact = compact_keyword_text(&normalized);
    if contains_cjk_or_kana(&compact) {
        tokens.extend(char_ngrams(&compact, 2));
        tokens.extend(char_ngrams(&compact, 3));
    }

    if tokens.is_empty() {
        if !compact.is_empty() {
            tokens.push(compact);
        }
    }
    tokens.sort();
    tokens.dedup();
    tokens
}

fn like_match_score(drawer: &Drawer, query: &str) -> f32 {
    let raw_haystack = format!(
        "{}\n{}\n{}\n{}",
        drawer.content,
        drawer.wing,
        drawer.room,
        drawer.source_file.as_deref().unwrap_or("")
    );
    let haystack = normalize_keyword_text(&raw_haystack);
    let haystack_compact = compact_keyword_text(&haystack);
    let normalized_query = normalize_keyword_text(query);
    let normalized_query = normalized_query.trim();
    let query_compact = compact_keyword_text(normalized_query);
    let tokens = keyword_tokens(query);

    let mut score = 0.0;
    if !normalized_query.is_empty() && haystack.contains(&normalized_query) {
        score += 2.0;
    }

    if !tokens.is_empty() {
        let hits = tokens
            .iter()
            .filter(|token| haystack.contains(token.as_str()))
            .count() as f32;
        score += hits / tokens.len() as f32;
    }

    score += ngram_overlap_score(&query_compact, &haystack_compact, 2).max(ngram_overlap_score(
        &query_compact,
        &haystack_compact,
        3,
    ));

    score
}

fn normalize_keyword_text(text: &str) -> String {
    text.chars()
        .flat_map(|ch| ch.to_lowercase())
        .map(|ch| if is_keyword_char(ch) { ch } else { ' ' })
        .collect::<String>()
}

fn compact_keyword_text(text: &str) -> String {
    text.chars().filter(|ch| is_keyword_char(*ch)).collect()
}

fn is_keyword_char(ch: char) -> bool {
    ch.is_alphanumeric() || contains_cjk_or_kana_char(ch)
}

fn contains_cjk_or_kana(text: &str) -> bool {
    text.chars().any(contains_cjk_or_kana_char)
}

fn contains_cjk_or_kana_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3040..=0x309F // Hiragana
            | 0x30A0..=0x30FF // Katakana
            | 0x3400..=0x4DBF // CJK Extension A
            | 0x4E00..=0x9FFF // CJK Unified Ideographs
            | 0xF900..=0xFAFF // CJK Compatibility Ideographs
            | 0xFF66..=0xFF9F // Halfwidth Katakana
    )
}

fn char_ngrams(text: &str, n: usize) -> Vec<String> {
    let chars = text.chars().collect::<Vec<_>>();
    if n == 0 || chars.len() < n {
        return Vec::new();
    }
    chars
        .windows(n)
        .map(|window| window.iter().collect())
        .collect()
}

fn ngram_overlap_score(query: &str, haystack: &str, n: usize) -> f32 {
    let query_ngrams = char_ngrams(query, n);
    if query_ngrams.is_empty() {
        return 0.0;
    }
    let haystack_ngrams = char_ngrams(haystack, n).into_iter().collect::<HashSet<_>>();
    let hits = query_ngrams
        .iter()
        .filter(|gram| haystack_ngrams.contains(*gram))
        .count() as f32;
    hits / query_ngrams.len() as f32
}

fn should_fallback_to_like_search(err: &SearchError) -> bool {
    let SearchError::Turso(inner) = err else {
        return false;
    };
    let message = inner.to_string().to_ascii_lowercase();
    message.contains("fts")
        || message.contains("syntax error")
        || message.contains("no such function")
        || message.contains("no such table")
}

fn row_to_drawer(row: &turso::Row) -> Result<Drawer, DbError> {
    Ok(Drawer {
        id: val_str(row, 0),
        wing: val_str(row, 1),
        room: val_str(row, 2),
        content: val_str(row, 3),
        parts: parse_parts(&val_str(row, 4))?,
        source_file: {
            let s = val_str(row, 5);
            if s.is_empty() { None } else { Some(s) }
        },
        chunk_index: row
            .get_value(6)
            .ok()
            .and_then(|v| v.as_integer().copied())
            .unwrap_or(0),
        added_by: val_str(row, 7),
        filed_at: val_str(row, 8),
    })
}

fn parse_parts(raw: &str) -> Result<Vec<crate::types::ContentPart>, DbError> {
    if raw.is_empty() || raw == "[]" {
        Ok(Vec::new())
    } else {
        Ok(serde_json::from_str(raw)?)
    }
}

fn val_str(row: &turso::Row, idx: usize) -> String {
    match row.get_value(idx) {
        Ok(turso::Value::Text(s)) => s,
        Ok(turso::Value::Null) | Err(_) => String::new(),
        Ok(v) => format!("{v:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ContentPart;
    use async_trait::async_trait;
    use tempfile::tempdir;

    #[derive(Debug)]
    struct TestEmbedder;

    #[async_trait]
    impl Embedder for TestEmbedder {
        async fn embed(
            &self,
            inputs: &[Vec<ContentPart>],
        ) -> Result<Vec<Vec<f32>>, crate::embedder::EmbedError> {
            Ok(inputs
                .iter()
                .map(|parts| {
                    let text = parts
                        .iter()
                        .filter_map(ContentPart::as_text)
                        .collect::<Vec<_>>()
                        .join(" ");
                    embed_text(&text)
                })
                .collect())
        }

        fn dimension(&self) -> usize {
            2
        }

        fn provider_name(&self) -> &str {
            "test"
        }

        fn model_name(&self) -> &str {
            "test-2d"
        }
    }

    fn embed_text(text: &str) -> Vec<f32> {
        let lower = text.to_ascii_lowercase();
        if lower.contains("semantic-only") {
            vec![1.0, 0.0]
        } else if lower.contains("alpha-42") || lower.contains("timeout") {
            vec![0.85, 0.15]
        } else {
            vec![0.0, 1.0]
        }
    }

    #[tokio::test]
    async fn keyword_search_scored_backfills_missing_fts_rows_on_reopen() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("aimem.db");

        let db = AimemDb::open(&db_path).await?;
        let drawer = Drawer::new(
            "drawer_alpha",
            "attachments",
            "subject-1",
            "alpha-42 timeout note for worker restart",
            "tester",
        );
        db.insert_drawer(&drawer, None).await?;

        let conn = db.conn()?;
        conn.execute(
            "DELETE FROM drawers_fts WHERE drawer_id = ?1",
            [drawer.id.as_str()],
        )
        .await?;
        drop(db);

        let db = AimemDb::open(&db_path).await?;
        let searcher = Searcher::keyword_only(db);
        let hits = searcher
            .keyword_search_scored("alpha-42", Some("attachments"), Some("subject-1"), 5)
            .await?;

        assert_eq!(
            hits.first().map(|hit| hit.drawer.id.as_str()),
            Some("drawer_alpha")
        );
        Ok(())
    }

    #[tokio::test]
    async fn hybrid_search_rrf_promotes_exact_keyword_hit_over_semantic_only_hit()
    -> anyhow::Result<()> {
        let db = AimemDb::memory().await?;
        let embedder = Arc::new(TestEmbedder);
        let searcher = Searcher::new(db.clone(), embedder);

        let semantic_only = Drawer::new(
            "drawer_semantic",
            "attachments",
            "subject-1",
            "semantic-only memory about retries and restarts",
            "tester",
        );
        db.insert_drawer_with_profile(&semantic_only, Some(&[1.0, 0.0]), "test", "test-2d")
            .await?;

        let keyword_hit = Drawer::new(
            "drawer_keyword",
            "attachments",
            "subject-1",
            "alpha-42 timeout happened after worker restart",
            "tester",
        );
        db.insert_drawer_with_profile(&keyword_hit, Some(&[0.85, 0.15]), "test", "test-2d")
            .await?;

        let hits = searcher
            .hybrid_search(
                "alpha-42 timeout",
                Some("attachments"),
                Some("subject-1"),
                5,
            )
            .await?;

        assert_eq!(
            hits.first().map(|hit| hit.drawer.id.as_str()),
            Some("drawer_keyword")
        );
        Ok(())
    }

    #[tokio::test]
    async fn keyword_search_scored_uses_cjk_ngram_fallback_when_fts_has_no_phrase_hit()
    -> anyhow::Result<()> {
        let db = AimemDb::memory().await?;
        let target = Drawer::new(
            "drawer_cjk_target",
            "attachments",
            "subject-1",
            "文档里的项目代号是青竹。",
            "tester",
        );
        db.insert_drawer(&target, None).await?;
        let distractor = Drawer::new(
            "drawer_cjk_distractor",
            "attachments",
            "subject-1",
            "扫描PDF里的预约时间是6月18日早上九点半。",
            "tester",
        );
        db.insert_drawer(&distractor, None).await?;

        let searcher = Searcher::keyword_only(db);
        let hits = searcher
            .keyword_search_scored(
                "文档里的项目代号是什么？",
                Some("attachments"),
                Some("subject-1"),
                5,
            )
            .await?;

        assert_eq!(
            hits.first().map(|hit| hit.drawer.id.as_str()),
            Some("drawer_cjk_target")
        );
        Ok(())
    }

    #[tokio::test]
    async fn keyword_search_scored_uses_japanese_ngram_fallback_when_fts_has_no_phrase_hit()
    -> anyhow::Result<()> {
        let db = AimemDb::memory().await?;
        let target = Drawer::new(
            "drawer_ja_target",
            "attachments",
            "subject-1",
            "資料のプロジェクト名は北風ノート。",
            "tester",
        );
        db.insert_drawer(&target, None).await?;
        let distractor = Drawer::new(
            "drawer_ja_distractor",
            "attachments",
            "subject-1",
            "レシートのアレルギー欄にはピーナッツ抜きと書かれている。",
            "tester",
        );
        db.insert_drawer(&distractor, None).await?;

        let searcher = Searcher::keyword_only(db);
        let hits = searcher
            .keyword_search_scored(
                "資料のプロジェクト名は何ですか？",
                Some("attachments"),
                Some("subject-1"),
                5,
            )
            .await?;

        assert_eq!(
            hits.first().map(|hit| hit.drawer.id.as_str()),
            Some("drawer_ja_target")
        );
        Ok(())
    }

    #[tokio::test]
    async fn delete_drawer_removes_keyword_hits_from_fts_sidecar() -> anyhow::Result<()> {
        let db = AimemDb::memory().await?;
        let drawer = Drawer::new(
            "drawer_delete",
            "attachments",
            "subject-1",
            "remove alpha-42 from keyword index",
            "tester",
        );
        db.insert_drawer(&drawer, None).await?;

        let searcher = Searcher::keyword_only(db.clone());
        assert!(
            !searcher
                .keyword_search_scored("alpha-42", Some("attachments"), Some("subject-1"), 5)
                .await?
                .is_empty()
        );

        assert!(db.delete_drawer("drawer_delete").await?);
        assert!(
            searcher
                .keyword_search_scored("alpha-42", Some("attachments"), Some("subject-1"), 5)
                .await?
                .is_empty()
        );
        Ok(())
    }
}
