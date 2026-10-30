# RFM curation: embedding model measurements

Plan `rfm-curation-models`, Job `curation-embed`, attempt `curation-embed-a1`. Measured 2026-10-29 (EDT) on Context `cpd` records only. Nothing was installed or pulled (pro-d209). Every local-hardware number is `unverified` arithmetic.

## Answer

- **v1 embedding model: `vertex/gemini-embedding-2` via Bifrost at 768 dimensions, input = full `text` (first 2,000 chars), fused with FTS5 by RRF.** It hit 5/5 recall@5 and @10 in both directions (n=5 pairs) and 56.7% top-5 topic agreement, 1.7 points under the best row (gemini-3072 `text`, 58.4%) and above all non-gemini rows (52.6-54.3%, `text`).
- **Lexical alone (FTS5/BM25 on `text`) is good enough for the gate step.** It also hit recall@10 5/5 in both directions (n=5 pairs) and is within 2.5 points of the best embedding on topic top-1 and 7.4 on top-5. The embedding gain is below this sample's resolution (5 pairs, 1 pair = 20 points).
- **`text` beats `short` as input** for every model on retirement recall (e.g. gemini-768: 5/5 vs 4/5 forward recall@5) and on topic top-1 for 7 of 8 embedding rows. `short` is not a reliable key for dedupe.
- **The sample is small.** Only 5 of the 13 retirement rows are usable (see Method). Treat the ranking between models as unproven; treat "all of them find the target in the top 10 when given `text`" as the measured fact.

## Method

- **Records**: `/tmp/cpd-rfm-curation-models/records.json`, 579 rows of Context `cpd`. 363 have a `topic` (the Spec said 357 curated and 221 uncurated; the file has 363 and 216, and these numbers are used here).
- **Inputs**: `text` = first 2,000 chars of `text` (mean record length 735 chars, max 4,675). `short` = the curated `short`; for the 216 uncurated rows (no `short`) the first 200 chars of `text` stand in, so `short` rows mix two forms. Embeddings are cosine, L2-normalised.
- **Retirement pairs**: `retirements.json` has 13 rows: 8 `documented` (target is a file, skipped), 4 `superseded`, 1 `duplicate`: **5 usable pairs** (pro-d129>pro-d130, pro-d173>pro-d174, pro-n115>pro-n122, pro-n125 dup pro-n124, pro-n75>pro-d64). All 5 are `approved`. Pool = all 579 records minus the 11 approved-retired ones ("in-force").
  - `fwd`: query = the retired record, target = the record that replaced it (the Spec's question).
  - `rev`: query = the newer record, target = the retired one, with only that retired record added to the pool. This is the write-time direction (the new write looks for what it replaces).
  - recall@k = pairs whose target ranks within k, out of 5. One pair = 20 points.
- **Topic agreement**: for each of the 363 curated records, rank the other 362 curated records; top-1 = nearest neighbour shares `topic`; top-5 maj = at least 3 of the 5 nearest share `topic`. `topic` labels are the small-model distiller's own output, so this measures agreement with that labeller, not truth.
- **Lexical**: Python `sqlite3` FTS5 table of the pool, query = OR of the distinct alphanumeric tokens (2+ chars) of the query input, ranked by `bm25()`. Same pool and same input form as the embeddings.
- **Hybrid**: reciprocal rank fusion, k=60, over the full FTS5 rank list and the full embedding rank list.
- **Calls**: Bifrost `POST /openai/v1/embeddings` from this machine; OpenRouter `POST /v1/embeddings` from `homeserver` (key read there from `~/.pi/agent/auth.json`; data copied to `/tmp` and deleted after). Scripts and raw output are in `/tmp/rfm-curation-models/curation-embed/`, not committed.

## Candidates

OpenRouter `GET /api/v1/embeddings/models` exists (verified, 38 models listed, no key needed). Four small ones were picked for what a 4-6 GB GPU or an M4 Pro could run later; price is per 1M input tokens as listed 2026-10-29.

| Model | Via | Dims | Max input tokens | $/1M tokens | Params (published, from memory: unverified) |
|---|---|---|---|---|---|
| `vertex/gemini-embedding-2` | Bifrost | 3072 (native; `dimensions` works) | 8192 (OpenRouter listing; Bifrost not measured) | not listed by Bifrost; OpenRouter `google/gemini-embedding-2` lists 0.20 | n/a (hosted) |
| `sentence-transformers/all-minilm-l6-v2` | OpenRouter | 384 | 512 (listed) | 0.005 | 22.7M |
| `baai/bge-base-en-v1.5` | OpenRouter | 768 | 512 (listed) | 0.005 | 109M |
| `baai/bge-m3` | OpenRouter | 1024 | 8194 (listed) | 0.01 | 568M |
| `perplexity/pplx-embed-v1-0.6b` | OpenRouter | 1024 | 32000 (listed) | 0.004 | ~0.6B |

- Not available through OpenRouter or Bifrost, so not measured: Qwen3-Embedding-0.6B (OpenRouter lists only the 4B and 8B), nomic-embed, BGE-small. No quality claim is made for them.
- **Bifrost `vertex/gemini-embedding-2` checks (2026-10-29, `hello`)**: default returns 3072 dims, 1 token. `dimensions`=256 and 768 return 256 and 768 dims; each equals the L2-normalised first-N of the 3072 vector (cosine 1.0000, 1 probe each), so the API parameter and Matryoshka truncation give the same vector. The native vector has norm 1.0000. 512 was measured by truncation only. A two-item `input` array is refused with HTTP 400 ("does not support batch embedding inputs"): **one call per record**.
- `baai/bge-base-en-v1.5` rejected 76 of 579 `text` inputs (HTTP 400, "513 input tokens ... context length is only 512"). Those 76 were re-sent truncated to 1,500 chars (59) or 1,200 chars (17) and used so; its `text` row is therefore a mixed form. The other models accepted all 2,000-char inputs (OpenRouter's handling of over-512-token inputs for all-MiniLM was not checked: it may truncate silently).

## Quality: retrieval and topic agreement

Sample sizes: topic n=363 queries; retirement n=5 pairs per direction. Columns: R@k = pairs with target in top k out of 5; ranks are the target's rank for the 5 pairs in the order listed in Method (999 = absent; no row had one). "(trunc)" = Matryoshka truncation of the 3072 vector, renormalised.

**Input form `text (first 2,000 chars)`**

| Model / method | topic top-1 (n=363) | topic top-5 maj (n=363) | fwd R@5 (n=5) | fwd R@10 (n=5) | rev R@5 (n=5) | rev R@10 (n=5) | fwd ranks | rev ranks |
|---|---|---|---|---|---|---|---|---|
| lexical FTS5/BM25 | 69.4% | 51.0% | 5/5 | 5/5 | 4/5 | 5/5 | [2, 1, 1, 1, 2] | [9, 2, 1, 1, 4] |
| gemini-3072 | 70.8% | 58.4% | 5/5 | 5/5 | 4/5 | 5/5 | [4, 1, 1, 1, 4] | [6, 1, 1, 1, 4] |
| gemini-3072 +lex RRF | 70.2% | 55.4% | 5/5 | 5/5 | 4/5 | 5/5 | [3, 1, 1, 1, 2] | [6, 2, 1, 1, 4] |
| gemini-768(trunc) | 70.0% | 56.7% | 5/5 | 5/5 | 5/5 | 5/5 | [4, 1, 1, 1, 3] | [5, 1, 1, 1, 4] |
| gemini-768(trunc) +lex RRF | 70.0% | 54.8% | 5/5 | 5/5 | 4/5 | 5/5 | [3, 1, 1, 1, 2] | [7, 2, 1, 1, 4] |
| gemini-512(trunc) | 69.7% | 55.1% | 5/5 | 5/5 | 4/5 | 5/5 | [4, 1, 1, 1, 3] | [6, 1, 1, 1, 5] |
| gemini-512(trunc) +lex RRF | 70.0% | 55.9% | 5/5 | 5/5 | 4/5 | 5/5 | [3, 1, 1, 1, 2] | [6, 2, 1, 1, 4] |
| gemini-256(trunc) | 68.3% | 56.2% | 5/5 | 5/5 | 4/5 | 5/5 | [4, 1, 1, 1, 4] | [5, 1, 1, 1, 9] |
| gemini-256(trunc) +lex RRF | 70.2% | 55.1% | 5/5 | 5/5 | 4/5 | 5/5 | [2, 1, 1, 1, 2] | [6, 2, 1, 1, 4] |
| all-minilm-l6-v2 | 66.7% | 52.6% | 5/5 | 5/5 | 4/5 | 5/5 | [3, 1, 1, 1, 3] | [1, 1, 1, 1, 10] |
| all-minilm-l6-v2 +lex RRF | 69.4% | 54.8% | 5/5 | 5/5 | 5/5 | 5/5 | [2, 1, 1, 1, 1] | [4, 2, 1, 1, 5] |
| bge-m3 | 71.9% | 53.7% | 5/5 | 5/5 | 5/5 | 5/5 | [5, 1, 1, 1, 1] | [5, 1, 1, 1, 1] |
| bge-m3 +lex RRF | 70.8% | 55.4% | 5/5 | 5/5 | 4/5 | 5/5 | [3, 1, 1, 1, 1] | [6, 2, 1, 1, 2] |
| bge-base-en-v1.5 | 65.6% | 54.3% | 4/5 | 4/5 | 4/5 | 5/5 | [3, 1, 1, 1, 12] | [2, 1, 1, 1, 10] |
| bge-base-en-v1.5 +lex RRF | 67.5% | 54.8% | 5/5 | 5/5 | 5/5 | 5/5 | [2, 1, 1, 1, 3] | [5, 2, 1, 1, 4] |
| pplx-embed-v1-0.6b | 68.9% | 52.9% | 5/5 | 5/5 | 4/5 | 5/5 | [4, 1, 1, 1, 5] | [8, 1, 1, 1, 3] |
| pplx-embed-v1-0.6b +lex RRF | 68.6% | 55.4% | 5/5 | 5/5 | 4/5 | 5/5 | [2, 1, 1, 1, 2] | [6, 2, 1, 1, 3] |

**Input form `short (curated `short`; 216 uncurated records: first 200 chars of text)`**

| Model / method | topic top-1 (n=363) | topic top-5 maj (n=363) | fwd R@5 (n=5) | fwd R@10 (n=5) | rev R@5 (n=5) | rev R@10 (n=5) | fwd ranks | rev ranks |
|---|---|---|---|---|---|---|---|---|
| lexical FTS5/BM25 | 65.0% | 45.5% | 4/5 | 4/5 | 4/5 | 4/5 | [17, 1, 1, 1, 2] | [20, 1, 1, 1, 2] |
| gemini-3072 | 69.1% | 57.9% | 4/5 | 4/5 | 3/5 | 4/5 | [47, 1, 1, 1, 3] | [47, 1, 1, 1, 9] |
| gemini-3072 +lex RRF | 66.7% | 54.5% | 4/5 | 4/5 | 4/5 | 4/5 | [24, 1, 1, 1, 2] | [32, 1, 1, 1, 4] |
| gemini-768(trunc) | 67.8% | 57.6% | 4/5 | 4/5 | 3/5 | 4/5 | [50, 1, 1, 1, 2] | [56, 1, 1, 1, 8] |
| gemini-768(trunc) +lex RRF | 67.2% | 54.3% | 4/5 | 4/5 | 4/5 | 4/5 | [23, 1, 1, 1, 2] | [32, 1, 1, 1, 4] |
| gemini-512(trunc) | 66.7% | 56.5% | 4/5 | 4/5 | 3/5 | 4/5 | [51, 1, 1, 1, 2] | [54, 1, 1, 1, 9] |
| gemini-512(trunc) +lex RRF | 65.6% | 54.0% | 4/5 | 4/5 | 4/5 | 4/5 | [24, 1, 1, 1, 2] | [31, 1, 1, 1, 3] |
| gemini-256(trunc) | 66.7% | 54.3% | 4/5 | 4/5 | 3/5 | 3/5 | [36, 1, 1, 1, 5] | [42, 1, 1, 1, 13] |
| gemini-256(trunc) +lex RRF | 67.2% | 55.9% | 4/5 | 4/5 | 4/5 | 4/5 | [19, 1, 1, 1, 2] | [25, 1, 1, 1, 4] |
| all-minilm-l6-v2 | 62.5% | 50.4% | 4/5 | 4/5 | 4/5 | 4/5 | [13, 1, 2, 1, 2] | [12, 1, 1, 1, 3] |
| all-minilm-l6-v2 +lex RRF | 67.2% | 51.0% | 4/5 | 5/5 | 4/5 | 4/5 | [10, 1, 1, 1, 2] | [13, 1, 1, 1, 1] |
| bge-m3 | 66.7% | 48.8% | 4/5 | 5/5 | 4/5 | 5/5 | [6, 1, 2, 1, 2] | [8, 1, 1, 1, 2] |
| bge-m3 +lex RRF | 68.6% | 49.6% | 4/5 | 5/5 | 4/5 | 5/5 | [6, 1, 1, 1, 2] | [10, 1, 1, 1, 1] |
| bge-base-en-v1.5 | 66.7% | 48.5% | 4/5 | 5/5 | 4/5 | 4/5 | [6, 1, 2, 1, 2] | [12, 1, 1, 1, 1] |
| bge-base-en-v1.5 +lex RRF | 65.8% | 52.6% | 4/5 | 5/5 | 4/5 | 4/5 | [6, 1, 1, 1, 2] | [12, 1, 1, 1, 1] |
| pplx-embed-v1-0.6b | 64.7% | 51.5% | 3/5 | 3/5 | 3/5 | 3/5 | [17, 1, 1, 1, 45] | [25, 1, 1, 1, 32] |
| pplx-embed-v1-0.6b +lex RRF | 68.0% | 52.6% | 3/5 | 4/5 | 3/5 | 4/5 | [16, 1, 1, 1, 8] | [20, 1, 1, 1, 7] |

Reading the tables:

- Retirement: with `text`, every embedding row and the lexical row find the target in the top 10 for 5/5 pairs forward and 5/5 reverse, except bge-base (4/5 forward; its mixed-length `text` row) and pplx-embed under `short` (3/5). Under `short`, the hard pairs are pro-d129>pro-d130 (rank 47-50 for gemini, 6-17 for others, 17 lexical) and pro-n75>pro-d64 for pplx (rank 45); every other pair ranks 1-3 in every row.
- Topic: all rows span 62.5-71.9% top-1 and 45.5-58.4% top-5 majority. Best top-5: gemini-3072 `text` 58.4%. Lexical `text` is 69.4% / 51.0%, so embeddings add about 1-7 points on top-5. A 95% interval on 363 samples is about +-5 points, so differences under 5 points between embedding rows are noise.
- Hybrid RRF does not beat the best single method here: it lifts weaker rankers (minilm `short` topic top-1 62.5 to 67.2; bge-base `text` 65.6 to 67.5; gemini `short` pro-d129 rank 47 to 24) and slightly lowers gemini topic scores (3072 `text` 70.8/58.4 to 70.2/55.4).
- Matryoshka: gemini 768 loses nothing against 3072 on `text` (70.0 vs 70.8 top-1, 56.7 vs 58.4 top-5, retrieval equal or better); 256 loses 2.5 points top-1 and 2.2 points top-5 on `text`, and one reverse pair falls to rank 9 (still within 10).
- Quantisation of gemini-768 (real vectors, same eval): `text` int8 70.2 / 57.0 topic, 5/5 and 5/5 recall@5 forward and reverse; binary 71.9 / 57.0 and the same recall. `short`: int8 4/5 forward and 3/5 reverse at @5. So int8 and 1-bit binary hold on `text`.

## Cost and speed

Single-call latency: 20 sequential calls, first 20 records, `short` inputs (Bifrost also `text`). Bifrost measured from this machine, OpenRouter from `homeserver`; the four OpenRouter jobs ran concurrently, so their batch throughput is contended and latency may overlap another job. Throughput = 579 records / wall time for one form; Bifrost with 8 threads of single calls, OpenRouter in batches of 16.

| Model | p50 / p95 single-call latency | Records/s `short` | Records/s `text` | Tokens, 579 `short` + 579 `text` | Cost, both forms |
|---|---|---|---|---|---|
| gemini-embedding-2 (Bifrost) | 405 / 487 ms (`text`: 391 / 494 ms) | 15.9 | 19.7 | 17,023 + 113,000 = 130,023 | about $0.026 at OpenRouter's 0.20/M price (Bifrost's own price unverified) |
| all-minilm-l6-v2 | 1327 / 2393 ms | 11.2 | 10.5 | 103,616 | $0.00052 (reported by OpenRouter) |
| bge-base-en-v1.5 | 290 / 1676 ms | 27.6 | n/a (76 retries) | 91,085 + 34,724 (retry call) | $0.00046 + $0.00017 retry |
| bge-m3 | 331 / 598 ms | 21.3 | 18.8 | 147,005 | $0.00147 |
| pplx-embed-v1-0.6b | 226 / 327 ms | 49.4 | 61.6 | 122,405 | $0.00049 |

- Embedding all 579 records costs under 3 cents on every model, so cost does not decide the model. Per write, one `text` call is about 195 tokens (gemini, 113,000 / 579), about $0.00004 at 0.20/M.
- Write-time latency of about 0.4 s for the Bifrost model is paid once per record, inside a write path that already waits on a small-model call.
- OpenRouter's latency figures are noisy (p95 up to 2.4 s) and from one host at one time.

## Fit: vector size and brute-force kNN

Bytes per record: float32 = 4 x dims; int8 = dims; binary = dims / 8. Totals are bytes per record x count (vectors only, no index or metadata).

| Dims (model) | float32 / record | int8 / record | binary / record | float32 1,000 | float32 10,000 | int8 1,000 | int8 10,000 | binary 1,000 | binary 10,000 |
|---|---|---|---|---|---|---|---|---|---|
| 3072 (gemini native) | 12,288 B | 3,072 B | 384 B | 12.3 MB | 123 MB | 3.1 MB | 30.7 MB | 384 KB | 3.84 MB |
| 1024 (bge-m3, pplx) | 4,096 B | 1,024 B | 128 B | 4.1 MB | 41 MB | 1.0 MB | 10.2 MB | 128 KB | 1.28 MB |
| 768 (gemini-768, bge-base) | 3,072 B | 768 B | 96 B | 3.1 MB | 30.7 MB | 0.77 MB | 7.7 MB | 96 KB | 0.96 MB |
| 512 (gemini-512) | 2,048 B | 512 B | 64 B | 2.0 MB | 20.5 MB | 0.51 MB | 5.1 MB | 64 KB | 0.64 MB |
| 384 (minilm) | 1,536 B | 384 B | 48 B | 1.5 MB | 15.4 MB | 0.38 MB | 3.8 MB | 48 KB | 0.48 MB |
| 256 (gemini-256) | 1,024 B | 256 B | 32 B | 1.0 MB | 10.2 MB | 0.26 MB | 2.6 MB | 32 KB | 0.32 MB |

Brute-force kNN, numpy 2.0.2 float32 matrix-vector product plus `argpartition` top-10, one query, 100 runs, M4 Pro mac-pro. The 1,000 and 10,000 matrices are the 579 real gemini `short` vectors tiled to size (time does not depend on values; no synthetic vectors).

| Dims | 1,000 vectors p50 / p95 | 10,000 vectors p50 / p95 |
|---|---|---|
| 3072 | 0.054 / 0.057 ms | 0.743 / 0.858 ms |
| 768 | 0.011 / 0.013 ms | 0.223 / 0.265 ms |
| 256 | 0.007 / 0.008 ms | 0.049 / 0.057 ms |

- Brute force at 10,000 records is under 1 ms even at 3072 dims, so the kNN time is not a constraint at the live store's scale (1,145 records on 2026-10-29). Whether to use sqlite-vec is Plan 1's question: `docs/research/rfm-memory-retrieval.md` does not exist on origin/main as of this run (fetched 2026-10-29), so it is not cited.
- numpy printed spurious divide/overflow warnings from its matmul on this machine; every vector was checked finite and the rankings are sane, so results are used.

## Local run arithmetic (all `unverified`: nothing installed, nothing run)

Method: per-write FLOPs = 2 x params x tokens, with tokens = 225 per record (guess: 195 gemini tokens, rounded up for other tokenizers) and params from the table above (from memory). Effective throughput (guess, about 30% of peak): GTX 1050 Ti 0.6 TFLOPS fp32; RTX 2060 3 TFLOPS fp16; M4 Pro GPU/Metal 1.4 TFLOPS; Ryzen 7 3800X CPU 0.3 TFLOPS. Memory = params x 2 bytes (fp16) plus about 20% runtime overhead (guess:). These are upper-bound ballparks, not measurements.

| Model | fp16 weights | Fits 1050 Ti (4 GB) | Per-write `text` embed: 1050 Ti | 2060 | M4 Pro | Ryzen CPU |
|---|---|---|---|---|---|---|
| all-minilm-l6-v2 | 45 MB | yes | about 17 ms | about 3 ms | about 7 ms | about 34 ms |
| bge-base-en-v1.5 | 218 MB | yes | about 82 ms | about 16 ms | about 35 ms | about 164 ms |
| bge-m3 | 1.14 GB | yes (about 1.4 GB with overhead) | about 430 ms | about 85 ms | about 180 ms | about 850 ms |
| pplx-embed-v1-0.6b | 1.2 GB | yes (about 1.4 GB, size unverified) | about 450 ms | about 90 ms | about 190 ms | about 900 ms |

- **Within 2 points of gemini-768 `text` recall@10 and would fit the 1050 Ti**: on the measured 5 pairs, all-minilm-l6-v2, bge-m3 and pplx-embed all matched it (5/5 forward and reverse at @10; bge-base missed one forward pair, 4/5). The smallest is all-minilm-l6-v2 (22.7M params, 384 dims). The 5-pair sample cannot separate these; on topic top-5 gemini-768 leads minilm by 4.1 points and bge-m3 by 3.0, so the tie is on retirement recall only.
- all-minilm and bge-base cap at 512 tokens: `text` above that is cut or rejected (bge-base rejected 76 of 579). A local candidate that takes the full 2,000 chars is bge-m3 (8194 tokens).
- Quality for any model not listed on OpenRouter or Bifrost (Qwen3-Embedding-0.6B, nomic-embed, BGE-small) is not claimed.

## Recommendation (v1)

| Item | Choice |
|---|---|
| Embedding model | `vertex/gemini-embedding-2` through Bifrost |
| Dimension | 768 (`dimensions: 768`; same as truncating 3072, 4x smaller, no measured loss on `text`) |
| Input form | `text`, first 2,000 chars (not `short`) |
| Candidate step | FTS5 on `text` for the lexical candidates plus embedding top-k, fused with RRF, 5-10 neighbours |
| Is lexical alone enough for the gate? | Yes on this data: FTS5 `text` found all 5 retirement targets in the top 10 (both directions). Add the embedding for topic-level neighbours and paraphrases; do not block v1 on it. |
| Fallback to local later | bge-m3 (1024 dims, 8194 tokens) matched gemini on the 5 pairs; local speed `unverified` |

Caveat: 5 pairs, 363 topic queries from a single Context. Re-measure when `entry_reads` or more retirements give a larger ground truth.

## Findings to record

- Context `cpd` has only 5 usable duplicate/superseded retirement pairs of 13 rows (the other 8 are `documented`, target a file), so retrieval recall has a 5-pair resolution.
- With full `text` input, FTS5 and every embedding model except bge-base ranked all 5 retirement targets in the top 10 in both directions; `short` input is weaker (3-4 of 5 at top 5).
- Topic agreement of nearest neighbour vs distiller `topic`: lexical text 69.4% top-1, best embedding gemini-3072 text 70.8% top-1 and 58.4% top-5 majority (n=363).
- Bifrost `vertex/gemini-embedding-2` takes `dimensions` (256/768 verified) equal to Matryoshka truncation, refuses batched input, p50 405 ms per call; embedding all 579 records in both forms is 130,023 tokens, about $0.026 at OpenRouter's price (Bifrost's price unverified).
- Gemini at 768 dims loses nothing measurable to 3072 and int8 or binary quantisation holds on `text`; 10,000 float32 768-dim vectors are 30.7 MB and brute-force kNN takes 0.22 ms.
- Recommendation v1: gemini-embedding-2, 768 dims, `text` input, FTS5+RRF; lexical alone suffices for the gate step; all local-GPU figures are `unverified`.
