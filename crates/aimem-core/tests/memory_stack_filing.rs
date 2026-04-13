use aimem_core::embedder::EmbedError;
use aimem_core::{
    AimemDb, Config, ContentPart, DrawerFilingRequest, Embedder, LocalEmbedder, MemoryStack,
};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

fn shared_local_embedder() -> Arc<LocalEmbedder> {
    static EMBEDDER: OnceLock<Arc<LocalEmbedder>> = OnceLock::new();
    EMBEDDER
        .get_or_init(|| Arc::new(LocalEmbedder::new().expect("local embedder should initialize")))
        .clone()
}

#[derive(Debug, Default)]
struct CountingEmbedder {
    batches: Mutex<Vec<usize>>,
}

impl CountingEmbedder {
    fn recorded_batches(&self) -> Vec<usize> {
        self.batches
            .lock()
            .expect("counting embedder mutex should not be poisoned")
            .clone()
    }
}

#[async_trait]
impl Embedder for CountingEmbedder {
    async fn embed(&self, inputs: &[Vec<ContentPart>]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.batches
            .lock()
            .expect("counting embedder mutex should not be poisoned")
            .push(inputs.len());

        Ok(inputs
            .iter()
            .enumerate()
            .map(|(index, parts)| {
                vec![
                    inputs.len() as f32,
                    index as f32,
                    parts
                        .iter()
                        .filter_map(|part| part.as_text())
                        .collect::<String>()
                        .chars()
                        .count() as f32,
                ]
            })
            .collect())
    }

    fn dimension(&self) -> usize {
        3
    }

    fn provider_name(&self) -> &str {
        "test"
    }

    fn model_name(&self) -> &str {
        "counting"
    }
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

#[tokio::test]
async fn file_drawers_with_ids_batches_related_drawers_in_one_embedding_call() -> Result<()> {
    let db = AimemDb::memory().await?;
    let cfg = Config::default();
    let embedder = Arc::new(CountingEmbedder::default());
    let stack = MemoryStack::new(db.clone(), embedder.clone(), &cfg);

    let inserted = stack
        .file_drawers_with_ids(&[
            DrawerFilingRequest::new(
                "attachment.summary.001",
                "attachments",
                "wxmem.bot-a.direct.user-2",
                "Attachment summary".to_string(),
                "bot-a",
            )
            .with_parts(vec![ContentPart::text("Attachment summary")])
            .with_source_file("report.pdf"),
            DrawerFilingRequest::new(
                "attachment.chunk.001",
                "attachments",
                "wxmem.bot-a.direct.user-2",
                "Attachment chunk 1".to_string(),
                "bot-a",
            )
            .with_parts(vec![ContentPart::text("Attachment chunk 1")])
            .with_source_file("report.pdf")
            .with_chunk_index(1),
            DrawerFilingRequest::new(
                "attachment.chunk.002",
                "attachments",
                "wxmem.bot-a.direct.user-2",
                "Attachment chunk 2".to_string(),
                "bot-a",
            )
            .with_parts(vec![ContentPart::text("Attachment chunk 2")])
            .with_source_file("report.pdf")
            .with_chunk_index(2),
        ])
        .await?;

    assert_eq!(inserted, vec![true, true, true]);
    assert_eq!(embedder.recorded_batches(), vec![3]);

    let drawers = db
        .fetch_drawers(Some("attachments"), Some("wxmem.bot-a.direct.user-2"), 10)
        .await?;
    assert_eq!(drawers.len(), 3);
    assert_eq!(
        drawers
            .iter()
            .filter(|drawer| drawer.chunk_index > 0)
            .count(),
        2
    );
    assert!(
        drawers
            .iter()
            .all(|drawer| drawer.source_file.as_deref() == Some("report.pdf"))
    );

    Ok(())
}

#[tokio::test]
async fn file_drawers_with_ids_skips_existing_ids_before_embedding() -> Result<()> {
    let db = AimemDb::memory().await?;
    let cfg = Config::default();
    let embedder = Arc::new(CountingEmbedder::default());
    let stack = MemoryStack::new(db.clone(), embedder.clone(), &cfg);

    let first = stack
        .file_drawers_with_ids(&[DrawerFilingRequest::new(
            "attachment.summary.010",
            "attachments",
            "wxmem.bot-a.direct.user-3",
            "Existing summary".to_string(),
            "bot-a",
        )
        .with_parts(vec![ContentPart::text("Existing summary")])
        .with_source_file("retry.pdf")])
        .await?;
    assert_eq!(first, vec![true]);

    let second = stack
        .file_drawers_with_ids(&[
            DrawerFilingRequest::new(
                "attachment.summary.010",
                "attachments",
                "wxmem.bot-a.direct.user-3",
                "Existing summary".to_string(),
                "bot-a",
            )
            .with_parts(vec![ContentPart::text("Existing summary")])
            .with_source_file("retry.pdf"),
            DrawerFilingRequest::new(
                "attachment.chunk.010",
                "attachments",
                "wxmem.bot-a.direct.user-3",
                "Fresh chunk".to_string(),
                "bot-a",
            )
            .with_parts(vec![ContentPart::text("Fresh chunk")])
            .with_source_file("retry.pdf")
            .with_chunk_index(1),
        ])
        .await?;

    assert_eq!(second, vec![false, true]);
    assert_eq!(embedder.recorded_batches(), vec![1, 1]);

    let drawers = db
        .fetch_drawers(Some("attachments"), Some("wxmem.bot-a.direct.user-3"), 10)
        .await?;
    assert_eq!(drawers.len(), 2);
    assert!(
        drawers
            .iter()
            .any(|drawer| drawer.id == "attachment.summary.010")
    );
    assert!(
        drawers
            .iter()
            .any(|drawer| drawer.id == "attachment.chunk.010")
    );

    Ok(())
}

#[tokio::test]
async fn file_drawers_with_ids_skips_duplicate_ids_inside_the_same_batch() -> Result<()> {
    let db = AimemDb::memory().await?;
    let cfg = Config::default();
    let embedder = Arc::new(CountingEmbedder::default());
    let stack = MemoryStack::new(db.clone(), embedder.clone(), &cfg);

    let inserted = stack
        .file_drawers_with_ids(&[
            DrawerFilingRequest::new(
                "attachment.chunk.020",
                "attachments",
                "wxmem.bot-a.direct.user-4",
                "Attachment chunk body".to_string(),
                "bot-a",
            )
            .with_parts(vec![ContentPart::text("Attachment chunk body")])
            .with_source_file("dup.pdf")
            .with_chunk_index(1),
            DrawerFilingRequest::new(
                "attachment.chunk.020",
                "attachments",
                "wxmem.bot-a.direct.user-4",
                "Attachment chunk body duplicate".to_string(),
                "bot-a",
            )
            .with_parts(vec![ContentPart::text("Attachment chunk body duplicate")])
            .with_source_file("dup.pdf")
            .with_chunk_index(1),
        ])
        .await?;

    assert_eq!(inserted, vec![true, false]);
    assert_eq!(embedder.recorded_batches(), vec![1]);

    let drawers = db
        .fetch_drawers(Some("attachments"), Some("wxmem.bot-a.direct.user-4"), 10)
        .await?;
    assert_eq!(drawers.len(), 1);
    assert_eq!(drawers[0].id, "attachment.chunk.020");
    assert_eq!(drawers[0].content, "Attachment chunk body");

    Ok(())
}
