use aimem_core::prelude::*;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = AimemDb::open("./aimem-semantic-example.db").await?;

    // Use local embedder by default for examples
    let embedder = Arc::new(LocalEmbedder::new()?);

    let content = "We moved the memory backend to Turso so semantic search stays local.";
    let embedding = embedder.embed_one(content).await?;

    let drawer = Drawer::new(
        "drawer_example_semantic_001",
        "demo_app",
        "decisions",
        content,
        "example",
    )
    .with_source_file("DECISIONS.md");

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
