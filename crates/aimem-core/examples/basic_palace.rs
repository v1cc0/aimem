use aimem_core::{Drawer, PalaceDb, Searcher};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = PalaceDb::open("./palace-example.db").await?;

    let drawer = Drawer {
        id: "drawer_example_basic_001".into(),
        wing: "demo_app".into(),
        room: "backend".into(),
        content: "The backend uses Rust and Turso for persistent memory.".into(),
        source_file: Some("README.md".into()),
        chunk_index: 0,
        added_by: "example".into(),
        filed_at: chrono::Utc::now().to_rfc3339(),
    };

    db.insert_drawer(&drawer, None).await?;

    let searcher = Searcher::keyword_only(db.clone());
    let hits = searcher
        .keyword_search("Turso", Some("demo_app"), None, 5)
        .await?;

    println!("drawer_count = {}", db.drawer_count().await?);
    println!("keyword_hits = {}", hits.len());
    Ok(())
}
