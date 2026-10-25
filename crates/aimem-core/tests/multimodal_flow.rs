use aimem_core::db::DbError;
use aimem_core::embedder::{
    GEMINI_EMBED_MODEL, GEMINI_EMBED_PROVIDER, LOCAL_EMBED_DIMENSION, LOCAL_EMBED_MODEL,
    LOCAL_EMBED_PROVIDER,
};
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

#[test]
fn drawer_helper_sets_expected_defaults() {
    let drawer = Drawer::new("d_helper_001", "demo", "notes", "hello", "tester")
        .with_source_file("README.md")
        .with_chunk_index(7)
        .with_filed_at("2026-04-09T12:00:04Z");

    assert_eq!(drawer.id, "d_helper_001");
    assert_eq!(drawer.wing, "demo");
    assert_eq!(drawer.room, "notes");
    assert_eq!(drawer.content, "hello");
    assert!(drawer.parts.is_empty());
    assert_eq!(drawer.source_file.as_deref(), Some("README.md"));
    assert_eq!(drawer.chunk_index, 7);
    assert_eq!(drawer.added_by, "tester");
    assert_eq!(drawer.filed_at, "2026-04-09T12:00:04Z");
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

    let drawer = Drawer::multimodal(
        "d_multimodal_001",
        "test_wing",
        "test_room",
        "Look at this fish: (image: fish.jpg)",
        parts.clone(),
        "tester",
    )
    .with_filed_at("2026-04-09T12:00:00Z");

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
    assert_eq!(vec1.len(), LOCAL_EMBED_DIMENSION);

    // Test batch embed
    let vecs = embedder
        .embed(&[
            vec![ContentPart::text("rust")],
            vec![ContentPart::text("turso")],
        ])
        .await?;
    assert_eq!(vecs.len(), 2);
    assert_eq!(vecs[0].len(), LOCAL_EMBED_DIMENSION);
    assert_eq!(vecs[1].len(), LOCAL_EMBED_DIMENSION);

    Ok(())
}

#[tokio::test]
async fn searcher_works_with_multimodal_drawers() -> Result<()> {
    let db = AimemDb::memory().await?;
    let embedder = shared_local_embedder();
    let searcher = Searcher::new(db.clone(), embedder.clone());

    let content = "I like salmon.";
    let embedding = embedder.embed_one(content).await?;

    let drawer = Drawer::new("d_001", "food", "preferences", content, "tester")
        .with_filed_at("2026-04-09T12:00:00Z");

    db.insert_drawer_with_profile(
        &drawer,
        Some(&embedding),
        embedder.provider_name(),
        embedder.model_name(),
    )
    .await?;

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
    assert_eq!(vec[0].len(), LOCAL_EMBED_DIMENSION);

    Ok(())
}

#[tokio::test]
async fn embedding_dimension_guard_rejects_mixed_vector_sizes() -> Result<()> {
    let db = AimemDb::memory().await?;

    let drawer = Drawer::new("d_dim_001", "demo", "guard", "dimension guard", "tester")
        .with_filed_at("2026-04-09T12:00:02Z");

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

#[tokio::test]
async fn embedding_profile_tracks_provider_and_model() -> Result<()> {
    let db = AimemDb::memory().await?;

    let drawer = Drawer::new("d_profile_001", "demo", "guard", "profile guard", "tester")
        .with_filed_at("2026-04-09T12:00:03Z");

    db.insert_drawer_with_profile(
        &drawer,
        Some(&vec![0.0; LOCAL_EMBED_DIMENSION]),
        LOCAL_EMBED_PROVIDER,
        LOCAL_EMBED_MODEL,
    )
    .await?;

    let profile = db.embedding_profile().await?;
    assert_eq!(profile.provider.as_deref(), Some(LOCAL_EMBED_PROVIDER));
    assert_eq!(profile.model.as_deref(), Some(LOCAL_EMBED_MODEL));
    assert_eq!(profile.dimension, Some(LOCAL_EMBED_DIMENSION));

    let err = db
        .assert_embedding_profile(
            LOCAL_EMBED_DIMENSION,
            GEMINI_EMBED_PROVIDER,
            GEMINI_EMBED_MODEL,
        )
        .await
        .expect_err("model mismatch should be rejected");
    assert!(matches!(err, DbError::EmbeddingModelMismatch { .. }));

    Ok(())
}

#[tokio::test]
async fn legacy_minilm_vectors_keep_their_original_model_profile() -> Result<()> {
    let db = AimemDb::memory().await?;
    let drawer = Drawer::new("legacy", "demo", "guard", "old local vector", "tester");
    db.insert_drawer(&drawer, Some(&vec![0.0; 384])).await?;
    // Simulate a database created before provider/model metadata existed.
    db.conn()?
        .execute("DELETE FROM aimem_meta WHERE key != 'embedding_dim'", ())
        .await?;
    let profile = db.embedding_profile().await?;
    assert_eq!(profile.provider.as_deref(), Some("fastembed"));
    assert_eq!(profile.model.as_deref(), Some("all-MiniLM-L6-v2"));
    assert!(matches!(
        db.assert_embedding_profile(
            LOCAL_EMBED_DIMENSION,
            LOCAL_EMBED_PROVIDER,
            LOCAL_EMBED_MODEL
        )
        .await,
        Err(DbError::EmbeddingDimensionMismatch {
            expected: 384,
            actual: 768
        })
    ));
    Ok(())
}

#[tokio::test]
async fn local_embeddings_are_normalized_multilingual_and_rank_relevant_text() -> Result<()> {
    let embedder = shared_local_embedder();
    assert_eq!(embedder.dimension(), LOCAL_EMBED_DIMENSION);
    assert_eq!(embedder.provider_name(), LOCAL_EMBED_PROVIDER);
    assert_eq!(embedder.model_name(), LOCAL_EMBED_MODEL);
    assert!(embedder.embed(&[]).await?.is_empty());
    let documents = embedder
        .embed(&[
            vec![ContentPart::text("Mars is known as the Red Planet.")],
            vec![ContentPart::text("Bananas are a yellow fruit.")],
            vec![ContentPart::text("日本語と中文のメモを保存します。")],
        ])
        .await?;
    let query = embedder
        .embed_query("Which planet is known as the Red Planet?")
        .await?;
    for vector in documents.iter().chain(std::iter::once(&query)) {
        assert_eq!(vector.len(), LOCAL_EMBED_DIMENSION);
        assert!(vector.iter().all(|v| v.is_finite()));
        assert!((vector.iter().map(|v| v * v).sum::<f32>() - 1.0).abs() < 1e-4);
    }
    let score = |v: &[f32]| query.iter().zip(v).map(|(a, b)| a * b).sum::<f32>();
    assert!(score(&documents[0]) > score(&documents[1]));
    let single = embedder
        .embed_one("Mars is known as the Red Planet.")
        .await?;
    assert!(
        single
            .iter()
            .zip(&documents[0])
            .all(|(a, b)| (a - b).abs() < 1e-5)
    );
    Ok(())
}

#[tokio::test]
async fn legacy_gemini_vectors_are_not_reclassified_as_local_at_the_same_dimension() -> Result<()> {
    let db = AimemDb::memory().await?;
    let drawer = Drawer::new("legacy_remote", "demo", "guard", "remote vector", "tester");
    db.insert_drawer(&drawer, Some(&vec![0.0; 768])).await?;
    db.conn()?
        .execute("DELETE FROM aimem_meta WHERE key != 'embedding_dim'", ())
        .await?;
    let profile = db.embedding_profile().await?;
    assert_eq!(profile.provider.as_deref(), Some(GEMINI_EMBED_PROVIDER));
    assert_eq!(profile.model.as_deref(), Some(GEMINI_EMBED_MODEL));
    assert!(matches!(
        db.assert_embedding_profile(
            LOCAL_EMBED_DIMENSION,
            LOCAL_EMBED_PROVIDER,
            LOCAL_EMBED_MODEL
        )
        .await,
        Err(DbError::EmbeddingModelMismatch { .. })
    ));
    Ok(())
}
