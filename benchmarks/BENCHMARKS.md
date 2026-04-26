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
