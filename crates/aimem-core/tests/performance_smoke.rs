use std::time::{Duration, Instant};

use aimem_core::{Drawer, PalaceDb, Searcher, layers::l1_generate};
use anyhow::Result;

fn make_drawer(i: usize, room: &str, content: String) -> Drawer {
    Drawer {
        id: format!("drawer_perf_{room}_{i}"),
        wing: "perf_wing".into(),
        room: room.into(),
        content,
        source_file: Some(format!("file_{i}.md")),
        chunk_index: 0,
        added_by: "perf-test".into(),
        filed_at: chrono::Utc::now().to_rfc3339(),
    }
}

#[tokio::test]
#[ignore = "performance smoke test; run explicitly"]
async fn keyword_search_handles_thousands_of_drawers() -> Result<()> {
    let db = PalaceDb::memory().await?;

    for i in 0..3_000usize {
        let content = if i % 250 == 0 {
            format!(
                "needle document {i}: Turso keeps local memory searchable without extra services."
            )
        } else {
            format!("background document {i}: ordinary project notes for throughput testing.")
        };
        db.insert_drawer(&make_drawer(i, "search", content), None)
            .await?;
    }

    let searcher = Searcher::keyword_only(db);
    let start = Instant::now();
    let hits = searcher
        .keyword_search("needle", Some("perf_wing"), Some("search"), 20)
        .await?;
    let elapsed = start.elapsed();

    assert!(!hits.is_empty());
    assert!(
        elapsed < Duration::from_secs(5),
        "keyword search too slow: {elapsed:?}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "performance smoke test; run explicitly"]
async fn l1_generation_stays_bounded_on_large_palace() -> Result<()> {
    let db = PalaceDb::memory().await?;

    for i in 0..4_000usize {
        let room = if i % 2 == 0 { "backend" } else { "decisions" };
        let content = format!(
            "drawer {i}: this is a long enough memory record about Rust, Turso, architecture decisions, and implementation details to stress L1 rendering without using embeddings."
        );
        db.insert_drawer(&make_drawer(i, room, content), None)
            .await?;
    }

    let start = Instant::now();
    let summary = l1_generate(&db, Some("perf_wing")).await?;
    let elapsed = start.elapsed();

    assert!(summary.contains("ESSENTIAL STORY"));
    assert!(
        summary.len() <= 4_000,
        "L1 summary grew unexpectedly: {} chars",
        summary.len()
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "L1 generation too slow: {elapsed:?}"
    );
    Ok(())
}
