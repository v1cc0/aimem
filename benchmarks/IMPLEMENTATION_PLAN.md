# AiMem Benchmark Implementation Plan

Status: planning note, 2026-04-26.

Reference used: MemPalace's benchmark harness and documentation from
<https://github.com/MemPalace/mempalace.git>, especially
`benchmarks/BENCHMARKS.md` and `benchmarks/longmemeval_bench.py`.

## Purpose

AiMem needs a benchmark suite that proves retrieval quality independently from
any bot runtime. The first public number should be retrieval recall, not generated
answer quality.

MemPalace's useful benchmark discipline:

- rebuild a clean memory/index for each question;
- evaluate `R@K`, `MRR`, and `NDCG`, not vague demos;
- store full per-question ranked results;
- mark LLM rerank as optional and separate from the no-LLM baseline;
- avoid quoting retrieval recall as if it were QA accuracy.

AiMem must extend that discipline for three realities MemPalace does not cover:

1. English, Chinese, and Japanese retrieval.
2. Cross-lingual query/corpus combinations.
3. Multimodal memory through `ContentPart::{Text, Image, Audio, Video}` and
   text extracted from attachments.

## Benchmark Crate

Add a non-published workspace member:

```toml
[package]
name = "aimem-bench"
publish = false
```

Keep benchmark-only dependencies out of `aimem-core`.

Proposed commands:

```bash
cargo run -p aimem-bench -- tri-memory \
  --dataset benchmarks/fixtures/tri_memory_micro.jsonl \
  --mode hybrid \
  --embed-model all-minilm-l6-v2 \
  --out benchmarks/results_tri_hybrid_minilm.jsonl

cargo run -p aimem-bench -- longmemeval \
  --dataset /tmp/longmemeval-data/longmemeval_s_cleaned.json \
  --mode raw-session \
  --top-k 10 \
  --out benchmarks/results_lme_raw_session.jsonl

cargo run -p aimem-bench -- multimodal \
  --dataset benchmarks/fixtures/multimodal_micro.jsonl \
  --mode direct-gemini \
  --out benchmarks/results_mm_direct_gemini.jsonl
```

## Dataset Schema

Use JSONL so result rows are append-friendly and diffable:

```json
{
  "question_id": "tri_zh_0001",
  "language": "zh",
  "query_language": "zh",
  "corpus_language": "zh",
  "question_type": "preference",
  "modality": "text",
  "question": "我平时喜欢喝什么？",
  "answer": "冻柠茶",
  "expected_session_ids": ["sess_zh_0001"],
  "expected_source_ids": ["sess_zh_0001"],
  "haystack_sessions": [
    {
      "session_id": "sess_zh_0001",
      "date": "2026-04-26T10:00:00Z",
      "turns": [
        {"role": "user", "content": "我平时喜欢喝冻柠茶，不要太甜。"}
      ],
      "attachments": []
    }
  ]
}
```

For multimodal cases, `attachments` contains stable source IDs, paths, MIME, and
private scoring truth. The truth text is for scoring and deterministic upper-bound
runs; it must not be injected into direct multimodal modes.

## Modes

| Mode | Ingest | Query | API/LLM |
|---|---|---|---|
| `keyword-only` | `AimemDb::insert_drawer*` without embedder | `Searcher::keyword_search_scored` | No |
| `raw-session` | one drawer per session | vector or hybrid | No by default |
| `all-turn-session` | user + assistant turns per session | vector or hybrid | No by default |
| `turn` | one drawer per turn | vector or hybrid | No by default |
| `hybrid` | normal drawers | `Searcher::hybrid_search` | No by default |
| `text-only-truth` | multimodal truth text as normal drawers | hybrid | No |
| `direct-gemini` | actual image/audio/video bytes in `ContentPart` | vector/hybrid | Yes |
| `llm-rerank` | top-N from any mode | external reranker | Yes, later |

## Multilingual Plan

Run the same harness on:

- English corpus / English query.
- Chinese corpus / Chinese query.
- Japanese corpus / Japanese query.
- English corpus / Chinese query.
- English corpus / Japanese query.
- Chinese/Japanese corpus / English operator query.

Fixture sources:

1. **Micro fixtures committed to git**: small hand-audited EN/ZH/JA set for CI.
2. **LongMemEval adapter**: English full benchmark.
3. **Frozen translated LongMemEval derivatives**: generated locally, with
   generator version and checksums recorded. Do not commit large derived data
   until license/size is checked.

Required engine work before treating multilingual results as representative:

- Make local embedding model configurable:
  - current `all-MiniLM-L6-v2`;
  - `multilingual-e5-small/base/large`;
  - `bge-m3`;
  - optional Gemini remote embedding.
- Add CJK/Japanese-aware keyword fallback. Current keyword tokenization is
  whitespace-oriented and ASCII-lowercase-heavy, so the keyword branch is weak
  for Chinese and Japanese unless vector search saves it.

## Multimodal Plan

AiMem can store multimodal `ContentPart`s, but the benchmark must split two
different claims:

1. **Perfect extraction upper bound**: file the fixture truth text and measure
   retrieval. This says whether the memory engine can retrieve the fact once text
   exists.
2. **Direct multimodal retrieval**: file image/audio/video bytes through
   `Gemini2Embedder` and measure source recall. This is remote/API-labelled.

Fixture classes:

- generated PNG receipts/screenshots/cards/whiteboards in EN/ZH/JA;
- text PDFs and scanned/image-only PDFs;
- DOCX/CSV/JSON/TXT;
- audio only after the upstream transcription path is explicit enough.

Metrics:

- source R@1/R@5/R@10;
- evidence/session R@1/R@5/R@10;
- MRR@10 and NDCG@10;
- ingest/query latency p50/p95;
- embedding provider/model/dimension;
- remote cost estimate for Gemini modes.

## Output Format

Every question emits one JSONL row:

```json
{
  "question_id": "tri_ja_0007",
  "language": "ja",
  "modality": "image",
  "mode": "direct-gemini",
  "embed_provider": "gemini",
  "embed_model": "gemini-embedding-2-preview",
  "expected_ids": ["sess_ja_0007"],
  "ranked": [
    {"rank": 1, "id": "sess_ja_0007", "source_id": "img_ja_0007", "score": 0.91}
  ],
  "recall_at_5": true,
  "mrr": 1.0,
  "latency_ms": 42
}
```

Each run also writes `summary.json`:

- aggregate R@K, MRR, NDCG;
- breakdown by language, query/corpus language pair, modality, question type;
- latency percentiles;
- git commit and dependency versions.

## CI/Local Strategy

- `bench:micro`: committed fixtures, no network, no API key, suitable for CI.
- `bench:full-text`: LongMemEval English and translated derivatives, local only.
- `bench:full-mm`: multimodal extraction/direct remote embedding, local only.
- `bench:compare`: fail if micro R@5 or MRR drops by more than 1 percentage point,
  or p95 query latency regresses by more than 25%.

## First Patch

Keep the first implementation deliberately small:

1. Add schema structs and JSONL loader/writer.
2. Add 9 text fixtures: 3 English, 3 Chinese, 3 Japanese.
3. Implement `tri-memory` with `keyword-only` and current `hybrid`.
4. Write result JSONL and summary JSON.

Only after that baseline should we add LongMemEval, translated data, configurable
embedding models, and multimodal fixtures.
