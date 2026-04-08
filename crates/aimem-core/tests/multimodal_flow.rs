use aimem_core::db::DbError;
use aimem_core::{AimemDb, ContentPart, Drawer, Embedder, LocalEmbedder, Searcher};
use anyhow::Result;
use std::sync::Arc;
use std::sync::OnceLock;

fn shared_local_embedder() -> Arc<LocalEmbedder> {
    static EMBEDDER: OnceLock<Arc<LocalEmbedder>> = OnceLock::new();
    EMBEDDER
        .get_or_init(|| Arc::new(LocalEmbedder::new().expect("local embedder should initialize")))
        .clone()
}

#[tokio::test]
async fn multimodal_drawer_persistence_works() -> Result<()> {
    let db = AimemDb::memory().await?;

    let parts = vec![
        ContentPart::text("Look at this fish:"),
        ContentPart::Image {
            uri: Some("file://fish.jpg".to_string()),
            mime: "image/jpeg".to_string(),
            data: None, // Data is not stored in DB
        },
    ];

    let drawer = Drawer {
        id: "d_multimodal_001".into(),
        wing: "test_wing".into(),
        room: "test_room".into(),
        content: "Look at this fish: (image: fish.jpg)".into(),
        parts: parts.clone(),
        source_file: None,
        chunk_index: 0,
        added_by: "tester".into(),
        filed_at: "2026-04-09T12:00:00Z".into(),
    };

    // Insert
    let inserted = db.insert_drawer(&drawer, None).await?;
    assert!(inserted);

    // Fetch back
    let drawers = db
        .fetch_drawers(Some("test_wing"), Some("test_room"), 1)
        .await?;
    assert_eq!(drawers.len(), 1);
    let fetched = &drawers[0];

    assert_eq!(fetched.id, "d_multimodal_001");
    assert_eq!(fetched.parts.len(), 2);

    if let ContentPart::Image { uri, .. } = &fetched.parts[1] {
        assert_eq!(uri.as_deref(), Some("file://fish.jpg"));
    } else {
        panic!("expected image part at index 1");
    }

    Ok(())
}

#[tokio::test]
async fn local_embedder_trait_works_with_new_async_api() -> Result<()> {
    let embedder = shared_local_embedder();

    // Test single embed
    let vec1 = embedder.embed_one("hello world").await?;
    assert_eq!(vec1.len(), 384);

    // Test batch embed
    let vecs = embedder
        .embed(&[
            vec![ContentPart::text("rust")],
            vec![ContentPart::text("turso")],
        ])
        .await?;
    assert_eq!(vecs.len(), 2);
    assert_eq!(vecs[0].len(), 384);
    assert_eq!(vecs[1].len(), 384);

    Ok(())
}

#[tokio::test]
async fn searcher_works_with_multimodal_drawers() -> Result<()> {
    let db = AimemDb::memory().await?;
    let embedder = shared_local_embedder();
    let searcher = Searcher::new(db.clone(), embedder.clone());

    let content = "I like salmon.";
    let embedding = embedder.embed_one(content).await?;

    let drawer = Drawer {
        id: "d_001".into(),
        wing: "food".into(),
        room: "preferences".into(),
        content: content.into(),
        parts: vec![],
        source_file: None,
        chunk_index: 0,
        added_by: "tester".into(),
        filed_at: "2026-04-09T12:00:00Z".into(),
    };

    db.insert_drawer(&drawer, Some(&embedding)).await?;

    // Search
    let results = searcher
        .vector_search("what fish do I like?", None, None, 1)
        .await?;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].drawer.id, "d_001");
    assert!(results[0].similarity > 0.5);
    assert_eq!(results[0].drawer.parts.len(), 0);

    Ok(())
}

#[tokio::test]
async fn local_embedder_handles_multimodal_by_extracting_text() -> Result<()> {
    let embedder = shared_local_embedder();

    let parts = vec![
        ContentPart::text("The color of the sky is"),
        ContentPart::Image {
            uri: Some("blue.jpg".into()),
            mime: "image/jpeg".into(),
            data: Some(vec![0, 1, 2]), // Dummy data
        },
        ContentPart::text("blue."),
    ];

    // Local embedder should join "The color of the sky is" and "blue."
    let vec = embedder.embed(&[parts]).await?;
    assert_eq!(vec.len(), 1);
    assert_eq!(vec[0].len(), 384);

    Ok(())
}

#[tokio::test]
async fn embedding_dimension_guard_rejects_mixed_vector_sizes() -> Result<()> {
    let db = AimemDb::memory().await?;

    let drawer = Drawer {
        id: "d_dim_001".into(),
        wing: "demo".into(),
        room: "guard".into(),
        content: "dimension guard".into(),
        parts: vec![],
        source_file: None,
        chunk_index: 0,
        added_by: "tester".into(),
        filed_at: "2026-04-09T12:00:02Z".into(),
    };

    assert!(db.insert_drawer(&drawer, Some(&vec![0.0; 384])).await?);

    let err = db
        .assert_embedding_dimension(768)
        .await
        .expect_err("dimension mismatch should be rejected");
    assert!(matches!(
        err,
        DbError::EmbeddingDimensionMismatch {
            expected: 384,
            actual: 768
        }
    ));

    Ok(())
}
