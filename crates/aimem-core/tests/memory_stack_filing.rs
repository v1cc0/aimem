use aimem_core::{AimemDb, Config, ContentPart, LocalEmbedder, MemoryStack};
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
async fn file_drawer_with_id_inserts_once_and_keeps_metadata() -> Result<()> {
    let db = AimemDb::memory().await?;
    let cfg = Config::default();
    let stack = MemoryStack::new(db.clone(), shared_local_embedder(), &cfg);

    let inserted = stack
        .file_drawer_with_id(
            "attachment.chunk.001",
            "attachments",
            "wxmem.bot-a.direct.user-1",
            "Attachment document / 文件 'report.pdf' chunk 1".to_string(),
            vec![ContentPart::text(
                "Attachment document / 文件 'report.pdf' chunk 1",
            )],
            Some("report.pdf"),
            1,
            "bot-a",
        )
        .await?;
    assert!(inserted);

    let inserted_again = stack
        .file_drawer_with_id(
            "attachment.chunk.001",
            "attachments",
            "wxmem.bot-a.direct.user-1",
            "Attachment document / 文件 'report.pdf' chunk 1".to_string(),
            vec![ContentPart::text(
                "Attachment document / 文件 'report.pdf' chunk 1",
            )],
            Some("report.pdf"),
            1,
            "bot-a",
        )
        .await?;
    assert!(!inserted_again);

    let drawers = db
        .fetch_drawers(Some("attachments"), Some("wxmem.bot-a.direct.user-1"), 10)
        .await?;
    assert_eq!(drawers.len(), 1);
    assert_eq!(drawers[0].id, "attachment.chunk.001");
    assert_eq!(drawers[0].source_file.as_deref(), Some("report.pdf"));
    assert_eq!(drawers[0].chunk_index, 1);

    Ok(())
}
