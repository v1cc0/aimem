use aimem_core::prelude::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = AimemDb::open("./aimem-example.db").await?;

    let drawer = Drawer::new(
        "drawer_example_basic_001",
        "demo_app",
        "general",
        "AiMem stores text memories in Turso.",
        "example",
    )
    .with_source_file("README.md");

    db.insert_drawer(&drawer, None).await?;

    let searcher = Searcher::keyword_only(db.clone());
    let hits = searcher
        .keyword_search("Turso", Some("demo_app"), None, 5)
        .await?;

    println!("drawer_count = {}", db.drawer_count().await?);
    println!("keyword_hits = {}", hits.len());
    Ok(())
}
