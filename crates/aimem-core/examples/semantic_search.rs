use aimem_core::{Drawer, Embedder, PalaceDb, Searcher};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = PalaceDb::open("./aimem-semantic-example.db").await?;
    let embedder = Embedder::new()?;

    let content = "We moved the memory backend to Turso so semantic search stays local.";
    let embedding = embedder.embed_one(content)?;

    let drawer = Drawer {
        id: "drawer_example_semantic_001".into(),
        wing: "demo_app".into(),
        room: "decisions".into(),
        content: content.into(),
        source_file: Some("DECISIONS.md".into()),
        chunk_index: 0,
        added_by: "example".into(),
        filed_at: chrono::Utc::now().to_rfc3339(),
    };

    db.insert_drawer(&drawer, Some(&embedding)).await?;

    let searcher = Searcher::new(db, embedder);
    let hits = searcher
        .vector_search("local semantic search backend", Some("demo_app"), None, 5)
        .await?;

    println!("semantic_hits = {}", hits.len());
    Ok(())
}
