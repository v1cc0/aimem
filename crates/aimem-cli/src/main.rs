use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aimem_core::{
    AimemDb, Drawer, Embedder, Gemini2Embedder, LocalEmbedder, Miner, SearchResult, Searcher,
    config::Config,
    convo::ConvoMiner,
    layers::{l0_render, l1_generate},
};
use anyhow::{Context, Result};
use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "aimem",
    version,
    about = "AiMem — give your AI a memory.",
    arg_required_else_help = true
)]
struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        help = "Explicit path to the Turso DB file."
    )]
    db_path: Option<PathBuf>,

    #[arg(
        short,
        long,
        global = true,
        action = ArgAction::Count,
        help = "Increase log verbosity (-v, -vv)."
    )]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Mine project files or conversation exports into AiMem.
    Mine(MineArgs),
    /// Semantic search over AiMem.
    Search(SearchArgs),
    /// Render L0 + L1 wake-up context.
    #[command(name = "wake-up", alias = "wakeup")]
    WakeUp(WakeUpArgs),
    /// Show AiMem overview.
    Status,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
enum MineMode {
    Projects,
    Convos,
}

#[derive(Debug, Args)]
struct MineArgs {
    /// Directory to mine.
    dir: PathBuf,

    #[arg(long, value_enum, default_value_t = MineMode::Projects)]
    mode: MineMode,

    #[arg(
        long,
        help = "Override wing name. Projects mode otherwise uses aimem.yaml."
    )]
    wing: Option<String>,

    #[arg(long, default_value = "conversations", help = "Room for convos mode.")]
    room: String,

    #[arg(long, default_value = "aimem")]
    agent: String,

    #[arg(long, default_value_t = 0, help = "Max files to process (0 = all).")]
    limit: usize,

    #[arg(long, help = "Preview without writing drawers.")]
    dry_run: bool,

    #[arg(long, help = "Store text only without generating embeddings.")]
    no_embed: bool,

    #[arg(
        long,
        env = "GEMINI_API_KEY",
        help = "Use Gemini 2.0 remote embedding."
    )]
    gemini_key: Option<String>,
}

#[derive(Debug, Args)]
struct SearchArgs {
    /// Query to search for.
    query: String,

    #[arg(long)]
    wing: Option<String>,

    #[arg(long)]
    room: Option<String>,

    #[arg(long, default_value_t = 5)]
    results: usize,

    #[arg(
        long,
        env = "GEMINI_API_KEY",
        help = "Use Gemini 2.0 remote embedding."
    )]
    gemini_key: Option<String>,
}

#[derive(Debug, Args)]
struct WakeUpArgs {
    #[arg(long)]
    wing: Option<String>,
}

struct RuntimeContext {
    cfg: Config,
    db_path: PathBuf,
    db: AimemDb,
}

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let Cli {
        db_path,
        verbose,
        command,
    } = Cli::parse();

    init_tracing(verbose)?;

    match command {
        Command::Mine(args) => cmd_mine(db_path.as_deref(), args).await,
        Command::Search(args) => cmd_search(db_path.as_deref(), args).await,
        Command::WakeUp(args) => cmd_wake_up(db_path.as_deref(), args).await,
        Command::Status => cmd_status(db_path.as_deref()).await,
    }
}

fn init_tracing(verbose: u8) -> Result<()> {
    let default_filter = match verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .without_time()
        .try_init()
        .map_err(|err| anyhow::anyhow!("failed to initialize tracing subscriber: {err}"))?;

    Ok(())
}

fn load_embedder(gemini_key: Option<&str>) -> Result<Arc<dyn Embedder>> {
    if let Some(key) = gemini_key {
        tracing::info!("using Gemini 2.0 remote embedding");
        Ok(Arc::new(Gemini2Embedder::new(key.to_string())))
    } else {
        tracing::info!("using local fastembed-rs model (all-MiniLM-L6-v2)");
        Ok(Arc::new(LocalEmbedder::new().context(
            "failed to load local embedding model; try setting GEMINI_API_KEY for remote embedding",
        )?))
    }
}

async fn cmd_mine(db_path: Option<&Path>, args: MineArgs) -> Result<()> {
    let runtime = load_runtime(db_path).await?;
    let embedder = if args.no_embed {
        None
    } else {
        Some(load_embedder(args.gemini_key.as_deref())?)
    };

    match args.mode {
        MineMode::Projects => {
            let miner = Miner::new(runtime.db, embedder);
            let stats = miner
                .mine(
                    &args.dir,
                    args.wing.as_deref(),
                    &args.agent,
                    args.limit,
                    args.dry_run,
                )
                .await
                .with_context(|| {
                    format!("failed to mine project directory {}", args.dir.display())
                })?;

            print_project_summary(&runtime.db_path, &args, &stats);
        }
        MineMode::Convos => {
            let wing = args
                .wing
                .clone()
                .unwrap_or_else(|| infer_wing_name(&args.dir, "conversations"));
            let miner = ConvoMiner::new(runtime.db, embedder);
            let stats = miner
                .mine(
                    &args.dir,
                    &wing,
                    &args.room,
                    &args.agent,
                    args.limit,
                    args.dry_run,
                )
                .await
                .with_context(|| {
                    format!(
                        "failed to mine conversation directory {}",
                        args.dir.display()
                    )
                })?;

            print_convo_summary(&runtime.db_path, &args, &wing, &stats);
        }
    }

    Ok(())
}

async fn cmd_search(db_path: Option<&Path>, args: SearchArgs) -> Result<()> {
    let runtime = load_runtime(db_path).await?;
    let keyword_searcher = Searcher::keyword_only(runtime.db.clone());
    let keyword_results = keyword_searcher
        .keyword_fallback_search(
            &args.query,
            args.wing.as_deref(),
            args.room.as_deref(),
            args.results,
        )
        .await
        .with_context(|| format!("keyword search failed for query {:?}", args.query))?;

    match load_embedder(args.gemini_key.as_deref()) {
        Ok(embedder) => {
            let searcher = Searcher::new(runtime.db, embedder);
            let vector_results = searcher
                .vector_search(
                    &args.query,
                    args.wing.as_deref(),
                    args.room.as_deref(),
                    args.results,
                )
                .await
                .with_context(|| format!("search failed for query {:?}", args.query))?;

            if !vector_results.is_empty() {
                println!("{}", render_vector_results(&args.query, &vector_results));
            } else if !keyword_results.is_empty() {
                println!("{}", render_keyword_results(&args.query, &keyword_results));
            } else {
                println!("No results found.");
            }
        }
        Err(err) if !keyword_results.is_empty() => {
            tracing::warn!("semantic search unavailable, falling back to keyword search: {err}");
            println!("{}", render_keyword_results(&args.query, &keyword_results));
        }
        Err(err) => {
            return Err(err).context(
                "failed to load embedding model; no keyword fallback results were found for this query",
            );
        }
    }

    Ok(())
}

async fn cmd_wake_up(db_path: Option<&Path>, args: WakeUpArgs) -> Result<()> {
    let runtime = load_runtime(db_path).await?;
    let l0 = l0_render(&runtime.cfg.identity_path).await;
    let l1 = l1_generate(&runtime.db, args.wing.as_deref()).await?;
    println!("{l0}\n\n{l1}");
    Ok(())
}

async fn cmd_status(db_path: Option<&Path>) -> Result<()> {
    let runtime = load_runtime(db_path).await?;
    let total_drawers = runtime.db.drawer_count().await?;
    let (wings, rooms) = runtime.db.taxonomy().await?;
    let identity_exists = runtime.cfg.identity_path.exists();
    let identity_tokens_est = if identity_exists {
        std::fs::read_to_string(&runtime.cfg.identity_path)
            .map(|text| text.len() / 4)
            .unwrap_or(0)
    } else {
        0
    };

    let mut out = String::new();
    writeln!(&mut out, "AiMem status")?;
    writeln!(&mut out, "=============")?;
    writeln!(&mut out, "db: {}", runtime.db_path.display())?;
    writeln!(&mut out, "drawers: {total_drawers}")?;
    writeln!(
        &mut out,
        "identity: {} ({}, ~{} tokens)",
        runtime.cfg.identity_path.display(),
        if identity_exists {
            "present"
        } else {
            "missing"
        },
        identity_tokens_est,
    )?;

    if total_drawers == 0 {
        writeln!(
            &mut out,
            "hint: run `aimem mine <dir>` to file your first memories"
        )?;
    }

    append_counts(&mut out, "wings", &wings)?;
    append_counts(&mut out, "rooms", &rooms)?;

    print!("{out}");
    Ok(())
}

async fn load_runtime(db_path: Option<&Path>) -> Result<RuntimeContext> {
    let mut cfg = Config::load().context("failed to load config")?;
    let resolved_db_path = resolve_db_path(&cfg, db_path);
    cfg.db_path = resolved_db_path.clone();

    let db = AimemDb::open(&resolved_db_path)
        .await
        .with_context(|| format!("failed to open AiMem DB at {}", resolved_db_path.display()))?;

    Ok(RuntimeContext {
        cfg,
        db_path: resolved_db_path,
        db,
    })
}

fn resolve_db_path(cfg: &Config, db_path: Option<&Path>) -> PathBuf {
    if let Some(path) = db_path {
        return expand_home(path);
    }

    cfg.db_path.clone()
}

fn expand_home(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if let Some(stripped) = raw.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(stripped)
    } else {
        path.to_path_buf()
    }
}

fn infer_wing_name(dir: &Path, fallback: &str) -> String {
    dir.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn print_project_summary(db_path: &Path, args: &MineArgs, stats: &aimem_core::miner::MineStats) {
    println!("mode: projects");
    println!("db: {}", db_path.display());
    println!("dir: {}", args.dir.display());
    println!("files scanned: {}", stats.files_scanned);
    println!("files skipped: {}", stats.files_skipped);
    println!(
        "drawers {}: {}",
        if args.dry_run { "previewed" } else { "added" },
        stats.drawers_added
    );

    if !stats.rooms.is_empty() {
        let mut rooms: Vec<_> = stats.rooms.iter().collect();
        rooms.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        println!("rooms touched:");
        for (room, count) in rooms {
            println!("  - {room}: {count}");
        }
    }
}

fn print_convo_summary(
    db_path: &Path,
    args: &MineArgs,
    wing: &str,
    stats: &aimem_core::convo::ConvoMineStats,
) {
    println!("mode: convos");
    println!("db: {}", db_path.display());
    println!("dir: {}", args.dir.display());
    println!("wing: {wing}");
    println!("room: {}", args.room);
    println!("files scanned: {}", stats.files_scanned);
    println!("files skipped: {}", stats.files_skipped);
    println!(
        "drawers {}: {}",
        if args.dry_run { "previewed" } else { "added" },
        stats.drawers_added
    );
}

fn append_counts(out: &mut String, label: &str, counts: &[(String, i64)]) -> Result<()> {
    writeln!(out)?;
    writeln!(out, "{label}:")?;

    if counts.is_empty() {
        writeln!(out, "  (none)")?;
        return Ok(());
    }

    for (name, count) in counts {
        writeln!(out, "  - {name}: {count}")?;
    }

    Ok(())
}

fn render_vector_results(query: &str, results: &[SearchResult]) -> String {
    let mut lines = vec![format!("## L3 — SEARCH RESULTS for \"{}\"", query)];

    for (i, result) in results.iter().enumerate() {
        lines.extend(render_drawer_block(
            i + 1,
            &result.drawer,
            Some(format!("sim={:.3}", result.similarity)),
        ));
    }

    lines.join("\n")
}

fn render_keyword_results(query: &str, drawers: &[Drawer]) -> String {
    let mut lines = vec![format!("## KEYWORD SEARCH RESULTS for \"{}\"", query)];

    for (i, drawer) in drawers.iter().enumerate() {
        lines.extend(render_drawer_block(
            i + 1,
            drawer,
            Some("keyword fallback".to_string()),
        ));
    }

    lines.join("\n")
}

fn render_drawer_block(index: usize, drawer: &Drawer, suffix: Option<String>) -> Vec<String> {
    let snippet: String = drawer
        .content
        .trim()
        .replace('\n', " ")
        .chars()
        .take(300)
        .collect();
    let snippet = if drawer.content.len() > 300 {
        format!("{}...", snippet)
    } else {
        snippet
    };

    let mut header = format!("  [{}] {}/{}", index, drawer.wing, drawer.room);
    if let Some(suffix) = suffix {
        header.push_str(&format!(" ({suffix})"));
    }

    let mut lines = vec![header, format!("      {}", snippet)];

    if let Some(source) = drawer
        .source_file
        .as_deref()
        .and_then(|s| Path::new(s).file_name())
        .map(|name| name.to_string_lossy().to_string())
    {
        lines.push(format!("      src: {}", source));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_command() {
        let cli = Cli::parse_from(["aimem", "search", "vector distance", "--wing", "core"]);
        match cli.command {
            Command::Search(args) => {
                assert_eq!(args.query, "vector distance");
                assert_eq!(args.wing.as_deref(), Some("core"));
                assert_eq!(args.results, 5);
            }
            other => panic!("expected search command, got {other:?}"),
        }
    }

    #[test]
    fn parses_convo_mine_command() {
        let cli = Cli::parse_from([
            "aimem",
            "mine",
            "./exports",
            "--mode",
            "convos",
            "--room",
            "decisions",
            "--no-embed",
        ]);

        match cli.command {
            Command::Mine(args) => {
                assert_eq!(args.mode, MineMode::Convos);
                assert_eq!(args.room, "decisions");
                assert!(args.no_embed);
            }
            other => panic!("expected mine command, got {other:?}"),
        }
    }
}
