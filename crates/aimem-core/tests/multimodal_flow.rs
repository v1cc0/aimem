use aimem_core::{AimemDb, ContentPart, Drawer, Searcher};
use anyhow::Result;

#[tokio::test]
async fn multimodal_drawer_round_trip_preserves_parts() -> Result<()> {
    let db = AimemDb::memory().await?;

    let drawer = Drawer {
        id: "drawer_mm_001".into(),
        wing: "demo".into(),
        room: "gallery".into(),
        content: "Look at this fish photo.".into(),
        parts: vec![
            ContentPart::text("Look at this fish photo."),
            ContentPart::Image {
                uri: "file:///tmp/fish.jpg".into(),
                mime: "image/jpeg".into(),
            },
        ],
        source_file: None,
        chunk_index: 0,
        added_by: "test".into(),
        filed_at: "2026-04-09T00:00:00Z".into(),
    };

    assert!(db.insert_drawer(&drawer, None).await?);

    let drawers = db.fetch_drawers(Some("demo"), Some("gallery"), 5).await?;
    assert_eq!(drawers.len(), 1);
    assert_eq!(drawers[0].parts, drawer.parts);

    Ok(())
}

#[tokio::test]
async fn keyword_search_returns_parts_for_multimodal_drawers() -> Result<()> {
    let db = AimemDb::memory().await?;

    let drawer = Drawer {
        id: "drawer_mm_002".into(),
        wing: "demo".into(),
        room: "food".into(),
        content: "I like salmon.".into(),
        parts: vec![
            ContentPart::text("I like salmon."),
            ContentPart::Image {
                uri: "https://example.invalid/salmon.png".into(),
                mime: "image/png".into(),
            },
        ],
        source_file: None,
        chunk_index: 0,
        added_by: "test".into(),
        filed_at: "2026-04-09T00:00:01Z".into(),
    };

    assert!(db.insert_drawer(&drawer, None).await?);

    let searcher = Searcher::keyword_only(db);
    let hits = searcher
        .keyword_search("salmon", Some("demo"), Some("food"), 5)
        .await?;

    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].parts, drawer.parts);

    Ok(())
}
