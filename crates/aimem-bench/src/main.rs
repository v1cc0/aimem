use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use aimem_core::{AimemDb, Drawer, Embedder, LocalEmbedder, Searcher};
use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use tempfile::tempdir;

#[derive(Debug, Parser)]
#[command(name = "aimem-bench")]
#[command(about = "Reproducible AiMem memory retrieval benchmarks")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Run the tiny EN/ZH/JA memory retrieval benchmark.
    TriMemory {
        /// JSONL fixture path.
        #[arg(long)]
        dataset: PathBuf,
        /// Retrieval mode.
        #[arg(long, value_enum, default_value_t = BenchMode::KeywordOnly)]
        mode: BenchMode,
        /// Number of ranked items to keep per question.
        #[arg(long, default_value_t = 10)]
        top_k: usize,
        /// Optional maximum question count.
        #[arg(long)]
        limit: Option<usize>,
        /// Per-question JSONL output.
        #[arg(long)]
        out: PathBuf,
        /// Aggregate summary JSON output. Defaults to `<out>.summary.json`.
        #[arg(long)]
        summary: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
enum BenchMode {
    /// Turso FTS/LIKE keyword path, no embedding model.
    KeywordOnly,
    /// AiMem hybrid path: keyword + local all-MiniLM-L6-v2 vector search.
    Hybrid,
}

impl BenchMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::KeywordOnly => "keyword-only",
            Self::Hybrid => "hybrid",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct BenchmarkCase {
    question_id: String,
    language: String,
    query_language: String,
    corpus_language: String,
    question_type: String,
    modality: String,
    question: String,
    answer: String,
    expected_session_ids: Vec<String>,
    #[serde(default)]
    expected_source_ids: Vec<String>,
    haystack_sessions: Vec<BenchSession>,
}

#[derive(Debug, Clone, Deserialize)]
struct BenchSession {
    session_id: String,
    date: String,
    turns: Vec<BenchTurn>,
    #[serde(default)]
    attachments: Vec<BenchAttachment>,
}

#[derive(Debug, Clone, Deserialize)]
struct BenchTurn {
    role: String,
    content: String,
}

#[derive(Debug, Clone, Deserialize)]
struct BenchAttachment {
    source_id: String,
    kind: String,
    mime: String,
    path: Option<String>,
    truth_text: Option<String>,
}

#[derive(Debug, Serialize)]
struct ResultRow {
    question_id: String,
    language: String,
    query_language: String,
    corpus_language: String,
    question_type: String,
    modality: String,
    mode: String,
    embed_provider: String,
    embed_model: String,
    top_k: usize,
    question: String,
    answer: String,
    expected_ids: Vec<String>,
    ranked: Vec<RankedRow>,
    recall_at_1: bool,
    recall_at_5: bool,
    recall_at_10: bool,
    mrr_at_10: f64,
    ndcg_at_10: f64,
    ingest_ms: u128,
    query_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
struct RankedRow {
    rank: usize,
    id: String,
    source_id: String,
    score: f32,
    keyword_score: Option<f32>,
    semantic_similarity: Option<f32>,
    content_excerpt: String,
}

#[derive(Debug, Default, Serialize)]
struct Summary {
    questions: usize,
    mode: String,
    embed_provider: String,
    embed_model: String,
    top_k: usize,
    overall: MetricSummary,
    by_language: BTreeMap<String, MetricSummary>,
    by_modality: BTreeMap<String, MetricSummary>,
    by_question_type: BTreeMap<String, MetricSummary>,
}

#[derive(Debug, Default)]
struct MetricAccumulator {
    count: usize,
    recall_at_1: f64,
    recall_at_5: f64,
    recall_at_10: f64,
    mrr_at_10: f64,
    ndcg_at_10: f64,
    ingest_ms: u128,
    query_ms: u128,
}

#[derive(Debug, Default, Serialize)]
struct MetricSummary {
    count: usize,
    recall_at_1: f64,
    recall_at_5: f64,
    recall_at_10: f64,
    mrr_at_10: f64,
    ndcg_at_10: f64,
    avg_ingest_ms: f64,
    avg_query_ms: f64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::TriMemory {
            dataset,
            mode,
            top_k,
            limit,
            out,
            summary,
        } => run_tri_memory(&dataset, mode, top_k, limit, &out, summary.as_deref()).await,
    }
}

async fn run_tri_memory(
    dataset: &Path,
    mode: BenchMode,
    top_k: usize,
    limit: Option<usize>,
    out: &Path,
    summary_path: Option<&Path>,
) -> Result<()> {
    if top_k == 0 {
        bail!("--top-k must be greater than zero");
    }

    let cases = load_cases(dataset, limit)?;
    if cases.is_empty() {
        bail!("dataset contains no benchmark cases: {}", dataset.display());
    }

    if let Some(parent) = out.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating output directory {}", parent.display()))?;
    }
    let summary_path = summary_path
        .map(PathBuf::from)
        .unwrap_or_else(|| default_summary_path(out));
    if let Some(parent) = summary_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating summary directory {}", parent.display()))?;
    }

    let embedder = match mode {
        BenchMode::KeywordOnly => None,
        BenchMode::Hybrid => Some(Arc::new(
            LocalEmbedder::new().context("initializing local AiMem embedder")?,
        )),
    };
    let (embed_provider, embed_model) = embedder
        .as_ref()
        .map(|embedder| {
            (
                embedder.provider_name().to_string(),
                embedder.model_name().to_string(),
            )
        })
        .unwrap_or_else(|| ("none".to_string(), "none".to_string()));

    let mut writer = BufWriter::new(
        File::create(out).with_context(|| format!("creating result file {}", out.display()))?,
    );
    let mut summary = Summary {
        questions: cases.len(),
        mode: mode.as_str().to_string(),
        embed_provider,
        embed_model,
        top_k,
        ..Summary::default()
    };
    let mut overall = MetricAccumulator::default();
    let mut by_language = BTreeMap::<String, MetricAccumulator>::new();
    let mut by_modality = BTreeMap::<String, MetricAccumulator>::new();
    let mut by_question_type = BTreeMap::<String, MetricAccumulator>::new();

    for case in &cases {
        let row = run_case(case, mode, top_k, embedder.clone()).await?;
        writeln!(writer, "{}", serde_json::to_string(&row)?)?;
        accumulate(&mut overall, &row);
        accumulate(by_language.entry(row.language.clone()).or_default(), &row);
        accumulate(by_modality.entry(row.modality.clone()).or_default(), &row);
        accumulate(
            by_question_type
                .entry(row.question_type.clone())
                .or_default(),
            &row,
        );
    }
    writer.flush()?;

    summary.overall = overall.into_summary();
    summary.by_language = into_summary_map(by_language);
    summary.by_modality = into_summary_map(by_modality);
    summary.by_question_type = into_summary_map(by_question_type);

    std::fs::write(
        &summary_path,
        serde_json::to_string_pretty(&summary)? + "\n",
    )
    .with_context(|| format!("writing summary file {}", summary_path.display()))?;

    println!(
        "aimem-bench tri-memory: mode={} questions={} R@1={:.3} R@5={:.3} MRR@10={:.3} results={} summary={}",
        summary.mode,
        summary.questions,
        summary.overall.recall_at_1,
        summary.overall.recall_at_5,
        summary.overall.mrr_at_10,
        out.display(),
        summary_path.display()
    );

    Ok(())
}

async fn run_case(
    case: &BenchmarkCase,
    mode: BenchMode,
    top_k: usize,
    embedder: Option<Arc<LocalEmbedder>>,
) -> Result<ResultRow> {
    let dir = tempdir().context("creating per-question tempdir")?;
    let db_path = dir.path().join("aimem-bench.db");
    let db = AimemDb::open(&db_path)
        .await
        .with_context(|| format!("opening benchmark db {}", db_path.display()))?;

    let ingest_started = Instant::now();
    for session in &case.haystack_sessions {
        let content = render_session(session);
        if content.trim().is_empty() {
            continue;
        }
        let drawer = Drawer::new(
            format!("{}:{}", case.question_id, session.session_id),
            "bench",
            case.language.as_str(),
            content.clone(),
            "aimem-bench",
        )
        .with_source_file(session.session_id.clone())
        .with_filed_at(session.date.clone());

        if let Some(embedder) = embedder.as_ref() {
            let embedding = embedder
                .embed_one(&content)
                .await
                .with_context(|| format!("embedding session {}", session.session_id))?;
            db.insert_drawer_with_profile(
                &drawer,
                Some(&embedding),
                embedder.provider_name(),
                embedder.model_name(),
            )
            .await
            .with_context(|| format!("inserting embedded drawer {}", drawer.id))?;
        } else {
            db.insert_drawer(&drawer, None)
                .await
                .with_context(|| format!("inserting keyword drawer {}", drawer.id))?;
        }
    }
    let ingest_ms = ingest_started.elapsed().as_millis();

    let searcher = match (mode, embedder) {
        (BenchMode::KeywordOnly, _) => Searcher::keyword_only(db),
        (BenchMode::Hybrid, Some(embedder)) => Searcher::new(db, embedder),
        (BenchMode::Hybrid, None) => bail!("hybrid mode requires an embedder"),
    };

    let query_started = Instant::now();
    let ranked = match mode {
        BenchMode::KeywordOnly => searcher
            .keyword_search_scored(&case.question, None, None, top_k)
            .await?
            .into_iter()
            .enumerate()
            .map(|(index, result)| RankedRow {
                rank: index + 1,
                id: result.drawer.id,
                source_id: result.drawer.source_file.unwrap_or_default(),
                score: result.score,
                keyword_score: Some(result.score),
                semantic_similarity: None,
                content_excerpt: excerpt(&result.drawer.content),
            })
            .collect::<Vec<_>>(),
        BenchMode::Hybrid => searcher
            .hybrid_search(&case.question, None, None, top_k)
            .await?
            .into_iter()
            .enumerate()
            .map(|(index, result)| RankedRow {
                rank: index + 1,
                id: result.drawer.id,
                source_id: result.drawer.source_file.unwrap_or_default(),
                score: result.score,
                keyword_score: result.keyword_score,
                semantic_similarity: result.semantic_similarity,
                content_excerpt: excerpt(&result.drawer.content),
            })
            .collect::<Vec<_>>(),
    };
    let query_ms = query_started.elapsed().as_millis();

    let expected_ids = expected_ids(case);
    let recall_at_1 = hit_at(&ranked, &expected_ids, 1);
    let recall_at_5 = hit_at(&ranked, &expected_ids, 5);
    let recall_at_10 = hit_at(&ranked, &expected_ids, 10);
    let mrr_at_10 = reciprocal_rank(&ranked, &expected_ids, 10);
    let ndcg_at_10 = ndcg_at(&ranked, &expected_ids, 10);

    Ok(ResultRow {
        question_id: case.question_id.clone(),
        language: case.language.clone(),
        query_language: case.query_language.clone(),
        corpus_language: case.corpus_language.clone(),
        question_type: case.question_type.clone(),
        modality: case.modality.clone(),
        mode: mode.as_str().to_string(),
        embed_provider: searcher
            .embedder()
            .map(|embedder| embedder.provider_name().to_string())
            .unwrap_or_else(|| "none".to_string()),
        embed_model: searcher
            .embedder()
            .map(|embedder| embedder.model_name().to_string())
            .unwrap_or_else(|| "none".to_string()),
        top_k,
        question: case.question.clone(),
        answer: case.answer.clone(),
        expected_ids: expected_ids.into_iter().collect(),
        ranked,
        recall_at_1,
        recall_at_5,
        recall_at_10,
        mrr_at_10,
        ndcg_at_10,
        ingest_ms,
        query_ms,
    })
}

fn load_cases(path: &Path, limit: Option<usize>) -> Result<Vec<BenchmarkCase>> {
    let reader = BufReader::new(
        File::open(path).with_context(|| format!("opening dataset {}", path.display()))?,
    );
    let mut cases = Vec::new();
    for (line_index, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("reading line {}", line_index + 1))?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        cases.push(
            serde_json::from_str(trimmed)
                .with_context(|| format!("parsing JSONL line {}", line_index + 1))?,
        );
        if limit.is_some_and(|limit| cases.len() >= limit) {
            break;
        }
    }
    Ok(cases)
}

fn render_session(session: &BenchSession) -> String {
    let mut rendered = session
        .turns
        .iter()
        .filter(|turn| !turn.content.trim().is_empty())
        .map(|turn| format!("{}: {}", turn.role.trim(), turn.content.trim()))
        .collect::<Vec<_>>();

    for attachment in &session.attachments {
        if let Some(truth_text) = attachment.truth_text.as_deref() {
            if !truth_text.trim().is_empty() {
                rendered.push(format!(
                    "attachment {} {} {} {}: {}",
                    attachment.source_id,
                    attachment.kind,
                    attachment.mime,
                    attachment.path.as_deref().unwrap_or(""),
                    truth_text.trim()
                ));
            }
        }
    }

    rendered.join("\n")
}

fn expected_ids(case: &BenchmarkCase) -> BTreeSet<String> {
    let mut ids = case
        .expected_session_ids
        .iter()
        .chain(case.expected_source_ids.iter())
        .filter_map(|id| {
            let trimmed = id.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .collect::<BTreeSet<_>>();
    if ids.is_empty() {
        ids.extend(case.expected_session_ids.iter().cloned());
    }
    ids
}

fn hit_at(ranked: &[RankedRow], expected: &BTreeSet<String>, k: usize) -> bool {
    ranked
        .iter()
        .take(k)
        .any(|row| expected.contains(&row.source_id) || expected.contains(&row.id))
}

fn reciprocal_rank(ranked: &[RankedRow], expected: &BTreeSet<String>, k: usize) -> f64 {
    ranked
        .iter()
        .take(k)
        .find(|row| expected.contains(&row.source_id) || expected.contains(&row.id))
        .map(|row| 1.0 / row.rank as f64)
        .unwrap_or(0.0)
}

fn ndcg_at(ranked: &[RankedRow], expected: &BTreeSet<String>, k: usize) -> f64 {
    let dcg = ranked
        .iter()
        .take(k)
        .enumerate()
        .filter(|(_, row)| expected.contains(&row.source_id) || expected.contains(&row.id))
        .map(|(index, _)| 1.0 / ((index + 2) as f64).log2())
        .sum::<f64>();

    let ideal_hits = expected.len().min(k);
    if ideal_hits == 0 {
        return 0.0;
    }
    let ideal = (0..ideal_hits)
        .map(|index| 1.0 / ((index + 2) as f64).log2())
        .sum::<f64>();
    if ideal == 0.0 { 0.0 } else { dcg / ideal }
}

fn excerpt(content: &str) -> String {
    const LIMIT: usize = 220;
    let trimmed = content.trim();
    if trimmed.chars().count() <= LIMIT {
        return trimmed.to_string();
    }
    let mut out = trimmed.chars().take(LIMIT).collect::<String>();
    out.push('…');
    out
}

fn accumulate(acc: &mut MetricAccumulator, row: &ResultRow) {
    acc.count += 1;
    acc.recall_at_1 += if row.recall_at_1 { 1.0 } else { 0.0 };
    acc.recall_at_5 += if row.recall_at_5 { 1.0 } else { 0.0 };
    acc.recall_at_10 += if row.recall_at_10 { 1.0 } else { 0.0 };
    acc.mrr_at_10 += row.mrr_at_10;
    acc.ndcg_at_10 += row.ndcg_at_10;
    acc.ingest_ms += row.ingest_ms;
    acc.query_ms += row.query_ms;
}

impl MetricAccumulator {
    fn into_summary(self) -> MetricSummary {
        if self.count == 0 {
            return MetricSummary::default();
        }
        let count = self.count as f64;
        MetricSummary {
            count: self.count,
            recall_at_1: self.recall_at_1 / count,
            recall_at_5: self.recall_at_5 / count,
            recall_at_10: self.recall_at_10 / count,
            mrr_at_10: self.mrr_at_10 / count,
            ndcg_at_10: self.ndcg_at_10 / count,
            avg_ingest_ms: self.ingest_ms as f64 / count,
            avg_query_ms: self.query_ms as f64 / count,
        }
    }
}

fn into_summary_map(
    accumulators: BTreeMap<String, MetricAccumulator>,
) -> BTreeMap<String, MetricSummary> {
    accumulators
        .into_iter()
        .map(|(key, acc)| (key, acc.into_summary()))
        .collect()
}

fn default_summary_path(out: &Path) -> PathBuf {
    let mut raw = out.as_os_str().to_os_string();
    raw.push(".summary.json");
    PathBuf::from(raw)
}
