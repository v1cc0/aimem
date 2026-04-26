# AiMem Benchmarks

This directory contains reproducible memory retrieval benchmarks.

The first committed benchmark is intentionally tiny: a 9-question English /
Chinese / Japanese text fixture used to prove the harness and expose language
regressions before we wire LongMemEval-scale or multimodal runs.

## Micro tri-language benchmark

Keyword-only, no embedding model:

```bash
cargo run -p aimem-bench -- tri-memory \
  --dataset benchmarks/fixtures/tri_memory_micro.jsonl \
  --mode keyword-only \
  --top-k 10 \
  --out benchmarks/results_tri_keyword_only.jsonl
```

Hybrid search, current local AiMem embedder (`all-MiniLM-L6-v2`) plus keyword
fusion:

```bash
cargo run -p aimem-bench -- tri-memory \
  --dataset benchmarks/fixtures/tri_memory_micro.jsonl \
  --mode hybrid \
  --top-k 10 \
  --out benchmarks/results_tri_hybrid.jsonl
```

Each run writes:

- per-question JSONL rows at `--out`;
- aggregate metrics at `<out>.summary.json`.

The committed fixture is small enough for CI once the benchmark command is wired
into the workflow. Larger LongMemEval and multimodal result files should not be
committed until size and license boundaries are checked.

## Deterministic multimodal truth-text benchmark

This fixture models the "extraction already succeeded" upper bound for image,
scanned-PDF, and document memories in English / Chinese / Japanese. It does not
read the referenced attachment paths and does not call a remote multimodal model;
the fixture `truth_text` is rendered as the already-extracted evidence.

```bash
cargo run -p aimem-bench -- tri-memory \
  --dataset benchmarks/fixtures/multimodal_truth_micro.jsonl \
  --mode keyword-only \
  --top-k 10 \
  --out benchmarks/results_mm_truth_keyword_only.jsonl

cargo run -p aimem-bench -- tri-memory \
  --dataset benchmarks/fixtures/multimodal_truth_micro.jsonl \
  --mode hybrid \
  --top-k 10 \
  --out benchmarks/results_mm_truth_hybrid.jsonl
```

Current micro results:

| Dataset | Mode | R@1 | R@5 | Note |
|---|---:|---:|---:|---|
| `tri_memory_micro` | keyword-only | 0.333 | 0.333 | English-only wins; CJK has no useful keyword hits yet. |
| `tri_memory_micro` | hybrid | 1.000 | 1.000 | Tiny smoke fixture, not a public quality claim. |
| `multimodal_truth_micro` | keyword-only | 0.333 | 0.333 | Same CJK keyword/tokenization gap. |
| `multimodal_truth_micro` | hybrid | 0.778 | 1.000 | CJK document questions are found by top-5, not always rank 1. |
