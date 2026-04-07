use std::fs;

use aimem_core::{
    AimemDb, KnowledgeGraph, Miner, Searcher, convo::ConvoMiner, layers::l1_generate,
};
use anyhow::Result;
use tempfile::tempdir;

#[tokio::test]
async fn project_mining_and_keyword_search_flow_works() -> Result<()> {
    let temp = tempdir()?;
    let project_dir = temp.path().join("demo-app");
    fs::create_dir_all(project_dir.join("src"))?;

    fs::write(
        project_dir.join("aimem.yaml"),
        r#"
wing: demo_app
rooms:
  - name: backend
    description: backend code
    keywords: [router, database, rust]
  - name: decisions
    description: decisions
    keywords: [decided, tradeoff]
"#,
    )?;

    fs::write(
        project_dir.join("src/lib.rs"),
        "We decided to keep the backend in Rust and Turso because the router and database logic must stay local and predictable.",
    )?;

    let db_path = temp.path().join("aimem.db");
    let db = AimemDb::open(&db_path).await?;
    let miner = Miner::new(db.clone(), None);

    let stats = miner
        .mine(&project_dir, None, "usage-test", 0, false)
        .await?;

    assert_eq!(stats.files_scanned, 1);
    assert_eq!(stats.files_skipped, 0);
    assert!(stats.drawers_added >= 1);

    let summary = l1_generate(&db, Some("demo_app")).await?;
    assert!(summary.contains("ESSENTIAL STORY"));
    assert!(summary.contains("backend"));
    assert!(summary.contains("Turso"));

    let searcher = Searcher::keyword_only(db.clone());
    let hits = searcher
        .keyword_search("Turso", Some("demo_app"), None, 5)
        .await?;
    assert_eq!(hits.len(), 1);
    assert!(hits[0].content.contains("Turso"));

    let (wings, rooms) = db.taxonomy().await?;
    assert_eq!(wings[0].0, "demo_app");
    assert_eq!(rooms[0].0, "backend");

    Ok(())
}

#[tokio::test]
async fn convo_miner_imports_marked_transcript() -> Result<()> {
    let temp = tempdir()?;
    let convo_dir = temp.path().join("exports");
    fs::create_dir_all(&convo_dir)?;

    fs::write(
        convo_dir.join("session.md"),
        "> user: Why did we move to Turso?\nWe needed local state and simpler deployment.\n\n> assistant: Because vector search and the database now live in one place.\nThat removed an external dependency.\n\n> user: Record that decision please.\nAbsolutely — the system now keeps memory local and simpler to reason about.\n",
    )?;

    let db = AimemDb::memory().await?;
    let miner = ConvoMiner::new(db.clone(), None);
    let stats = miner
        .mine(
            &convo_dir,
            "team_memory",
            "conversations",
            "usage-test",
            0,
            false,
        )
        .await?;

    assert_eq!(stats.files_scanned, 1);
    assert!(stats.drawers_added >= 1);

    let drawers = db
        .fetch_drawers(Some("team_memory"), Some("conversations"), 10)
        .await?;
    assert!(!drawers.is_empty());
    assert!(drawers[0].content.contains("Turso"));

    Ok(())
}

#[tokio::test]
async fn knowledge_graph_round_trip_works() -> Result<()> {
    let db = AimemDb::memory().await?;
    let kg = KnowledgeGraph::new(db);

    let triple_id = kg
        .add_triple("Alice", "works_on", "AiMem", Some("2026-01-01"), None)
        .await?;
    assert!(triple_id.starts_with("t_"));

    let facts = kg.query_entity("Alice", None, "outgoing").await?;
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].subject, "alice");
    assert_eq!(facts[0].predicate, "works_on");
    assert_eq!(facts[0].object, "aimem");
    assert!(facts[0].current);

    let invalidated = kg
        .invalidate("Alice", "works_on", "AiMem", Some("2026-02-01"))
        .await?;
    assert_eq!(invalidated, 1);

    let timeline = kg.timeline(Some("Alice")).await?;
    assert_eq!(timeline.len(), 1);
    assert_eq!(timeline[0].valid_to.as_deref(), Some("2026-02-01"));
    assert!(!timeline[0].current);

    let stats = kg.stats().await?;
    assert_eq!(stats["entities"], 2);
    assert_eq!(stats["triples"], 1);
    assert_eq!(stats["current_facts"], 0);

    Ok(())
}
