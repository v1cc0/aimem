use aimem_core::prelude::*;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = AimemDb::open("./aimem-semantic-example.db").await?;

    // Use local embedder by default for examples
    let embedder = Arc::new(LocalEmbedder::new()?);

    let content = "We moved the memory backend to Turso so semantic search stays local.";
    let embedding = embedder.embed_one(content).await?;

    let drawer = Drawer {
        id: "drawer_example_semantic_001".into(),
        wing: "demo_app".into(),
        room: "decisions".into(),
        content: content.into(),
        parts: vec![],
        source_file: Some("DECISIONS.md".into()),
        chunk_index: 0,
        added_by: "example".into(),
        filed_at: chrono::Utc::now().to_rfc3339(),
    };

    db.insert_drawer_with_profile(
        &drawer,
        Some(&embedding),
        embedder.provider_name(),
        embedder.model_name(),
    )
    .await?;

    let searcher = Searcher::new(db, embedder);
    let hits = searcher
        .vector_search("local semantic search backend", Some("demo_app"), None, 5)
        .await?;

    for hit in hits {
        println!("Hit (sim={:.3}): {}", hit.similarity, hit.drawer.content);
    }

    Ok(())
}
