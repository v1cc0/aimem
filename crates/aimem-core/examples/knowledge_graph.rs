use aimem_core::{AimemDb, KnowledgeGraph};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = AimemDb::memory().await?;
    let kg = KnowledgeGraph::new(db);

    kg.add_triple("Alice", "works_on", "AiMem", Some("2026-01-01"), None)
        .await?;
    kg.add_triple("AiMem", "uses", "Turso", Some("2026-01-01"), None)
        .await?;

    let facts = kg.query_entity("Alice", None, "outgoing").await?;
    println!("alice_facts = {}", facts.len());
    Ok(())
}
