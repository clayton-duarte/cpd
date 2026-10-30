# RFM curation classifiers: cost, speed and accuracy of every available classifier model

Plan `rfm-curation-models`, Job `classifiers-run`, measured 2026-10-29 23:41-23:53 EDT (homeserver clock 2026-10-30 03:41-03:53 UTC). Research only; no production code; nothing installed or pulled (decision pro-d209). Pairs and gold: `rfm-curation-classifier-pairs.json`, method in `rfm-curation-classifier-set.md`. Raw per-pair results: `rfm-curation-classifiers-results.json`.

## Verdict

- **Use `typesafe/jev-1.14` (pinned, not the moving `~typesafe/jev-latest`) as the cheap first pass, with a confidence gate of 0.8 and escalation to `vertex/gemini-3.8-flash`.** Measured on 103 gold pairs: 89.3% (92/103; Wilson 82-94%), p50 114 ms, p95 162 ms, $0.037 per 1,000 pairs, $0.0003 per 8-neighbour write.
- **Accuracy does not separate the top of the table.** 8 models sit at 88-90% with overlapping intervals (jev, Liquid d1-ultra, Perplexity decider, Inception mercury-decide, Nace drex-max); jev-latest is first by one pair. Choose among them on calibration, latency and cost, not on rank.
- **The classifier route beats the free-text judge on identical pairs:** `vertex/gemini-3.1-flash-lite` through Bifrost scored 68.0% (70/103) with the judge-file prompt and 70.9% (73/103) with the gold prompt; it calls 22-23 of the 24 gold `contradicts` pairs `supersedes`. It is also about 9x slower (p50 1017 ms) and about 16x dearer ($0.58 per 1,000 pairs).
- **The one error that matters is the partial overrule.** For the top models almost every wrong retire is a gold `contradicts` pair called `supersedes` (jev-1.14: 8 of 24); gold `independent` pairs retired: 1 of 70. Retire-class verdicts are right (gold `supersedes`/`duplicate`) only 9 of 18 times (gate off); at confidence >= 0.8: 8 of 9 (jev-1.14).
- **Confidence tracks correctness:** jev-1.14 is right on 70 of 72 pairs at confidence >= 0.75 (reliability table, ECE 0.038 on peak probability). `kev-4b` falls to 75.7% (it scored 92.5% on the 40 proxy-labelled pairs) and `tev1` to 65.0%: small samples flattered them.
- **Ladder** (classifier, then Flash on retire-class verdicts below the gate and on every `contradicts`): jev-1.14 escalates 26% of the 103 pairs (5% of the 59 unhunted pairs), ends at 99.0% with 0 wrong retires of gold `independent` and 1 of `contradicts`, for $0.77 per 1,000 pairs. The end accuracy is an upper bound: Flash's verdict is gold by construction.
- **Not run:** 17 listed models have no credentials here, 5 errored and a 6th (`scaledown/classify-1`) errored until re-run with its documented state (all listed below); chat models are not exposed as classifiers over OpenRouter; no model offers a `temperature` option on this route, so one setting per model.
- **Privacy:** every classifier call sends record text to OpenRouter, a third-party router, and on to the model vendor (which vendor varies per model; unverified), not only to Bifrost.
- **Unmeasured:** `duplicate` has 1 gold pair, `supersedes` 8; gold is two-model agreement, not human truth; run-to-run noise of one model was measured on 1 pair only (below).

## Ranking

103 gold pairs (70 independent, 24 contradicts, 8 supersedes, 1 duplicate; 15 disputed pairs excluded). Every model answered the same `choice` question with the same four labels and criteria, one call per pair (29 models x 103 pairs = 2,987 answered calls). Wrong retire = gold `independent` / `contradicts` called `supersedes` or `duplicate`, shown as errors/pairs of that gold class. Aliases (`~...-latest`) redirect to a vendor's newest model (OpenRouter description); whether an alias equals the pinned id is unverified (jev-latest and jev-1.14 differ on 1 pair, d1-ultra alias and pinned on 2, d1-latest and d1 on 5, the perplexity pairs on 0). `$` is pi's `usage.cost` (catalog price times tokens, per `docs/models.md`), not a billed amount (unverified). Latency: client wall time of one `classify` call from homeserver, 2 calls in flight, 103 calls per model, retried calls included. Models with no OpenRouter price report `$0`; none of the ranked ones did.

| Rank | Model (all via OpenRouter, pi System One route) | Accuracy (k/n; 95% Wilson %) | Wrong retire: gold independent | Wrong retire: gold contradicts | p50 / p95 ms | $ per 1,000 pairs | $ per write, k=8 | Acc on the 36 old pairs |
|---|---|---|---|---|---|---|---|---|
| 1 | `~typesafe/jev-latest` | 90.3% (93/103; 83-95) | 1/70 | 8/24 | 113 / 678 | 0.037 | 0.00030 | 33/36 |
| 2 | `typesafe/jev-1.14` | 89.3% (92/103; 82-94) | 1/70 | 8/24 | 114 / 162 | 0.037 | 0.00030 | 33/36 |
| 3 | `~liquid/d1-ultra-latest` | 89.3% (92/103; 82-94) | 3/70 | 7/24 | 172 / 382 | 0.025 | 0.00020 | 35/36 |
| 4 | `perplexity/pplx-decider-v1.2` | 89.3% (92/103; 82-94) | 3/70 | 7/24 | 176 / 214 | 0.013 | 0.00010 | 34/36 |
| 5 | `~perplexity/pplx-decider-latest` | 89.3% (92/103; 82-94) | 3/70 | 7/24 | 185 / 778 | 0.013 | 0.00010 | 34/36 |
| 6 | `liquid/d1-ultra` | 88.3% (91/103; 81-93) | 4/70 | 8/24 | 158 / 683 | 0.025 | 0.00020 | 35/36 |
| 7 | `nace-ai/drex-max` | 88.3% (91/103; 81-93) | 3/70 | 8/24 | 220 / 596 | 0.038 | 0.00030 | 34/36 |
| 8 | `inception/mercury-decide` | 88.3% (91/103; 81-93) | 2/70 | 7/24 | 236 / 329 | 0.012 | 0.00010 | 34/36 |
| 9 | `~liquid/d1-latest` | 86.4% (89/103; 78-92) | 1/70 | 13/24 | 130 / 212 | 0.012 | 0.00009 | 34/36 |
| 10 | `liquid/d1` | 86.4% (89/103; 78-92) | 1/70 | 12/24 | 142 / 269 | 0.012 | 0.00009 | 34/36 |
| 11 | `baseten/mammoth-1` | 86.4% (89/103; 78-92) | 4/70 | 9/24 | 160 / 501 | 0.063 | 0.00051 | 33/36 |
| 12 | `openai/gpt-6-luna-decisions` | 86.4% (89/103; 78-92) | 2/70 | 8/24 | 232 / 695 | 0.062 | 0.00050 | 33/36 |
| 13 | `typesafe/jev-1.13` | 84.5% (87/103; 76-90) | 4/70 | 11/24 | 116 / 141 | 0.037 | 0.00030 | 31/36 |
| 14 | `liquid/d1.1` | 83.5% (86/103; 75-89) | 2/70 | 13/24 | 126 / 196 | 0.012 | 0.00009 | 34/36 |
| 15 | `cloudflare/clef` | 83.5% (86/103; 75-89) | 4/70 | 12/24 | 274 / 1197 | 0.162 | 0.00129 | 31/36 |
| 16 | `microsoft/microsoft-decision-1` | 81.6% (84/103; 73-88) | 1/70 | 10/24 | 224 / 282 | 0.023 | 0.00019 | 31/36 |
| 17 | `nace-ai/drex-v2.0` | 80.6% (83/103; 72-87) | 1/70 | 17/24 | 258 / 467 | 0.033 | 0.00027 | 32/36 |
| 18 | `cloudflare/clef-omni` | 79.6% (82/103; 71-86) | 4/70 | 15/24 | 187 / 233 | 0.101 | 0.00080 | 31/36 |
| 19 | `nace-ai/drex-v1.5` | 78.6% (81/103; 70-85) | 1/70 | 14/24 | 478 / 818 | 0.022 | 0.00018 | 30/36 |
| 20 | `respan/span-02` | 77.7% (80/103; 69-85) | 0/70 | 20/24 | 549 / 2209 | 0.007 | 0.00005 | 33/36 |
| 21 | `cloudflare/clef-flash` | 76.7% (79/103; 68-84) | 6/70 | 16/24 | 372 / 709 | 0.026 | 0.00020 | 30/36 |
| 22 | `jaredpalmer/kev-4b` | 75.7% (78/103; 67-83) | 9/70 | 15/24 | 771 / 1764 | 0.023 | 0.00019 | 31/36 |
| 23 | `~perplexity/pplx-decider-flash-latest` | 70.9% (73/103; 61-79) | 11/70 | 19/24 | 99 / 129 | 0.006 | 0.00005 | 30/36 |
| 24 | `perplexity/pplx-decider-flash-v1.2` | 70.9% (73/103; 61-79) | 11/70 | 19/24 | 99 / 124 | 0.006 | 0.00005 | 30/36 |
| 25 | `baseten/elephant-1` | 68.9% (71/103; 59-77) | 11/70 | 20/24 | 123 / 161 | 0.022 | 0.00018 | 29/36 |
| 26 | `scaledown/classify-1 (text-state variant)` | 68.0% (70/103; 58-76) | 12/70 | 17/24 | 274 / 638 | 0.035 | 0.00028 | 29/36 |
| 27 | `togethercomputer/tev1-4b-experimental` | 65.0% (67/103; 55-74) | 13/70 | 21/24 | 175 / 234 | 0.027 | 0.00021 | 29/36 |
| 28 | `upstage/solar-decide` | 62.1% (64/103; 52-71) | 15/70 | 24/24 | 2828 / 12547 | 0.043 | 0.00034 | 27/36 |
| 29 | `upstage/solar-decide-flash` | 46.6% (48/103; 37-56) | 30/70 | 24/24 | 558 / 717 | 0.043 | 0.00034 | 21/36 |

Free-text baseline on the identical 103 pairs (vertex through Bifrost from this Mac, 103 calls each, `temperature` 0, `max_tokens` 2000, 2 in flight, JSON verdict; $ = measured tokens at $0.25 in / $1.50 out per M, which reproduces the judge file's $0.570; Vertex price `unverified`):

| Rank | Model (all via OpenRouter, pi System One route) | Accuracy (k/n; 95% Wilson %) | Wrong retire: gold independent | Wrong retire: gold contradicts | p50 / p95 ms | $ per 1,000 pairs | $ per write, k=8 | Acc on the 36 old pairs |
|---|---|---|---|---|---|---|---|---|
| - | `vertex/gemini-3.1-flash-lite (judge-file prompt)` | 68.0% (70/103; 58-76) | 10/70 | 23/24 | 1017 / 2110 | 0.584 | 0.00467 | 31/36 |
| - | `vertex/gemini-3.1-flash-lite (gold prompt)` | 70.9% (73/103; 61-79) | 8/70 | 22/24 | 858 / 1007 | 0.591 | 0.00473 | 31/36 |

Mean tokens per call for the baseline: about 2,070-2,100 in, 45 out (the classifiers: about 560-900 in, 0-80 out). Confusion (gold row, predicted dup/sup/con/ind), judge-file prompt: dup 1,0,0,0; sup 0,8,0,0; con 0,23,1,0; ind 5,5,0,60. Gold prompt: dup 1,0,0,0; sup 0,8,0,0; con 0,22,2,0; ind 2,6,0,62. The judge-file prompt has no partial-overrule rule; the gold prompt has it and the baseline still barely uses `contradicts`.

### Not run (every listed classifier that could not run)

`models.getModelsOfType("classifier")` listed 51 models; `getAvailableOfType` returned the 34 OpenRouter ones. All 34 use the `typesafe-system-one` API: **no chat model is exposed as a classifier over OpenRouter** (chat models become classifiers only through a llama.cpp router, `docs/llama-cpp.md#classification`), so the three chat models through the llama-cpp-classify route were `not run`. `gpt-6-luna` direct needs `OPENAI_API_KEY`; the OpenRouter `openai/gpt-6-luna-decisions` ran. Julia-1, lev and OpenJev are not in the list (the doc names them for a llama.cpp router, unverified here); Laya is listed only through `vercel-ai-gateway`, no credentials (below). 17 models failed one probe call each with the error shown; 6 OpenRouter models failed 6 consecutive calls (the 5 below plus `scaledown/classify-1`, which I re-ran with its documented `{text}` state, ranked above as the text-state variant).

| Model | `not run`: error text |
|---|---|
| `cloudflare-workers-ai/@cf/cloudflare/clef` | Provider is not configured: cloudflare-workers-ai |
| `cloudflare-workers-ai/@cf/cloudflare/clef-flash` | Provider is not configured: cloudflare-workers-ai |
| `cloudflare-workers-ai/typesafe/jev` | Provider is not configured: cloudflare-workers-ai |
| `openai/gpt-6-luna` | Provider is not configured: openai |
| `opencode/jev-1.13` | Provider is not configured: opencode |
| `opencode/jev-1.13-free` | Provider is not configured: opencode |
| `typesafe/jev-latest` | Provider is not configured: typesafe |
| `vercel-ai-gateway/convaiinnovations/laya` | Provider is not configured: vercel-ai-gateway |
| `vercel-ai-gateway/convaiinnovations/laya-free` | Provider is not configured: vercel-ai-gateway |
| `vercel-ai-gateway/liquid/d1` | Provider is not configured: vercel-ai-gateway |
| `vercel-ai-gateway/microsoft/microsoft-decision-1` | Provider is not configured: vercel-ai-gateway |
| `vercel-ai-gateway/openai/gpt-6-luna-decisions` | Provider is not configured: vercel-ai-gateway |
| `vercel-ai-gateway/perplexity/pplx-decider-v1.1-27b` | Provider is not configured: vercel-ai-gateway |
| `vercel-ai-gateway/scaledown/scaledown-classify-1` | Provider is not configured: vercel-ai-gateway |
| `vercel-ai-gateway/typesafe-ai/jev` | Provider is not configured: vercel-ai-gateway |
| `vercel-ai-gateway/typesafe-ai/jev-1.13.0` | Provider is not configured: vercel-ai-gateway |
| `vercel-ai-gateway/typesafe-ai/jev-1.14.0` | Provider is not configured: vercel-ai-gateway |
| `openrouter/inception/mercury-decide:free` | System One API error (404): This model is unavailable for free. The paid version is available now - use this slug instead: inception/mercury-decide; 6 consecutive failed calls, run aborted |
| `openrouter/perplexity/pplx-decider-v1.1-27b` | System One API error (404): No endpoints found for perplexity/pplx-decider-v1.1-27b.; 6 consecutive failed calls, run aborted |
| `openrouter/respan/span-01` | System One API error (400): Respan state must be a string or an object with only input (a message array) and output (a message), where each message has a string content and a rol; 6 consecutive failed calls, run aborted |
| `openrouter/respan/span-01-lite` | System One API error (400): Respan state must be a string or an object with only input (a message array) and output (a message), where each message has a string content and a rol; 6 consecutive failed calls, run aborted |
| `openrouter/respan/span-01-lite:free` | System One API error (400): Respan state must be a string or an object with only input (a message array) and output (a message), where each message has a string content and a rol; 6 consecutive failed calls, run aborted |
| `openrouter/scaledown/classify-1` | System One API error (400): ScaleDown state must be a string or an object with only text, document, and document_mime_type, including text or document; 6 consecutive failed calls, run aborted |

The direct Cloudflare, OpenAI, TypeSafe, opencode and Vercel entries did not run; the same model families ran through OpenRouter where OpenRouter lists them (clef, clef-flash, jev-1.13/1.14, gpt-6-luna-decisions, d1, decision-1).

## Method

- **Pairs:** the 103 non-disputed pairs of `rfm-curation-classifier-pairs.json`; text rebuilt from `records.json` of Context `cpd` (579 records) by id; newer = the incoming record, as in the gold file. Only Context `cpd` records were sent.
- **Call:** pi 1.3.0 `ModelRegistry.classify(model, {state, questions})` from a Node script on homeserver (the call `models.classify` in codemode wraps; I used the SDK because codemode needs a chat model to write the script: unverified that the two paths are identical). State: `{older_record: {id, kind, text}, new_record: {id, kind, text}}`; one `choice` question `verdict`, instructions = the gold prompt's framing sentences (new record is later, recency wins, judge as data), criteria = its four definitions verbatim (partial-overrule rule included), labels `duplicate`, `supersedes`, `contradicts`, `independent`. Key read from pi's auth store, never printed; copied data deleted from homeserver afterwards.
- **Settings:** one call per pair and model, 2 in flight, 60 s per call, one retry. `confidence` = pi's `(n*peak-1)/(n-1)`; the verdict is the arg-max label.
- **Temperature:** the doc says `temperature` softens probabilities for llama-cpp-classify; on the OpenRouter System One route it did nothing visible. Test (1 pair, jev-latest, 3 calls: default, 1, 2): `supersedes` probability 0.34, 0.25, 0.37, not monotonic; kev-4b: 0.7679, 0.7703, 0.7703. So it is ignored (inference) and each model has one setting; no held-out tuning was done (`not run`).
- **Repeatability:** the same jev-latest call gave `supersedes` 0.34 / 0.25 / 0.37 on one pair (3 calls): probabilities are not deterministic; run-to-run noise over all pairs is unmeasured.
- **Sample-size effect** (same pairs, same gold): 36 of the 40 old pairs are non-disputed; their gold has independent 30, contradicts 3, supersedes 2, duplicate 1.

| Model | Judge file: 40 pairs, proxy label | Same 36 old pairs, new gold | All 103 gold pairs |
|---|---|---|---|
| `~typesafe/jev-latest` | 90.0% (36/40) | 91.7% (33/36) | 90.3% (93/103) |
| `jaredpalmer/kev-4b` | 92.5% (37/40) | 86.1% (31/36) | 75.7% (78/103) |
| `togethercomputer/tev1-4b-experimental` | 82.5% (33/40) | 80.6% (29/36) | 65.0% (67/103) |

The old 36 pairs are 83% `independent` (30 of 36): accuracy there mostly measures the easy class, so models that retire too eagerly look fine on them and fail on the 67 new pairs (kev-4b 31/36 vs 47/67 on the other 67).

## Per-class results and confusion (top models, gold row, predicted dup / sup / con / ind)

| Model | Duplicate | Confusion | Precision / recall |
|---|---|---|---|
| `typesafe/jev-1.14` | 1/1 dup; dup→1,0,0,0 ; sup→0,8,0,0 ; con→0,8,16,0 ; ind→0,1,2,67 | dup P 1/1 R 1/1 / sup P 8/17 R 8/8 / con P 16/18 R 16/24 / ind P 67/67 R 67/70 |
| `~typesafe/jev-latest` | 1/1 dup; dup→1,0,0,0 ; sup→0,8,0,0 ; con→0,8,16,0 ; ind→0,1,1,68 | dup P 1/1 R 1/1 / sup P 8/17 R 8/8 / con P 16/17 R 16/24 / ind P 68/68 R 68/70 |
| `~liquid/d1-ultra-latest` | 1/1 dup; dup→1,0,0,0 ; sup→0,8,0,0 ; con→0,7,17,0 ; ind→0,3,1,66 | dup P 1/1 R 1/1 / sup P 8/18 R 8/8 / con P 17/18 R 17/24 / ind P 66/66 R 66/70 |
| `~perplexity/pplx-decider-latest` | 1/1 dup; dup→1,0,0,0 ; sup→0,8,0,0 ; con→0,7,16,1 ; ind→1,2,0,67 | dup P 1/2 R 1/1 / sup P 8/17 R 8/8 / con P 16/16 R 16/24 / ind P 67/68 R 67/70 |
| `inception/mercury-decide` | 1/1 dup; dup→1,0,0,0 ; sup→0,8,0,0 ; con→0,7,16,1 ; ind→2,0,2,66 | dup P 1/3 R 1/1 / sup P 8/15 R 8/8 / con P 16/18 R 16/24 / ind P 66/67 R 66/70 |
| `nace-ai/drex-max` | 1/1 dup; dup→1,0,0,0 ; sup→0,8,0,0 ; con→0,8,15,1 ; ind→2,1,0,67 | dup P 1/3 R 1/1 / sup P 8/17 R 8/8 / con P 15/15 R 15/24 / ind P 67/68 R 67/70 |
| `jaredpalmer/kev-4b` | 1/1 dup; dup→1,0,0,0 ; sup→0,8,0,0 ; con→0,15,8,1 ; ind→4,5,0,61 | dup P 1/5 R 1/1 / sup P 8/28 R 8/8 / con P 8/8 R 8/24 / ind P 61/62 R 61/70 |
| `togethercomputer/tev1-4b-experimental` | 1/1 dup; dup→1,0,0,0 ; sup→0,8,0,0 ; con→0,21,1,2 ; ind→0,13,0,57 | dup P 1/1 R 1/1 / sup P 8/42 R 8/8 / con P 1/1 R 1/24 / ind P 57/59 R 57/70 |

Reading: each model in this table gets all 8 gold `supersedes` and the 1 `duplicate` (recall 8/8, 1/1; n tiny), and the precision of `supersedes` is about 8/17 because 7-8 gold `contradicts` pairs are called `supersedes` (the partial overrule). Gold `contradicts` recall is 15-17 of 24 for the top models. `independent` precision is 67/67 (jev-1.14), 68/68 (jev-latest) and 66/66 (d1-ultra): an `independent` verdict hid no relation for them. The Wilson interval of 90% on 103 pairs is 83-95%, about 12 points wide: ranks 1-8 are not distinguishable. `duplicate` has 1 gold pair, so its precision and recall are `unmeasured`.

## Confidence gate

Cell = share of the 103 pairs answered at confidence >= t · accuracy of those answers · retire-class verdicts at >= t that are right (gold `supersedes` or `duplicate`) / retire-class verdicts at >= t. Pairs below t would be escalated. `mercury-decide` is a poor gate: it is over-confident (87% of pairs still answered at 0.9, 8 of 15 retire verdicts right); `kev-4b` answers only 4% at 0.9.

| Model | t = 0.5 | t = 0.7 | t = 0.8 | t = 0.9 |
|---|---|---|---|---|
| `typesafe/jev-1.14` | 88% · 93.4% · 9/15 | 73% · 94.7% · 8/12 | 66% · 98.5% · 8/9 | 50% · 98.0% · 7/8 |
| `~typesafe/jev-latest` | 84% · 93.1% · 9/15 | 72% · 94.6% · 8/12 | 65% · 97.0% · 8/10 | 54% · 98.2% · 8/9 |
| `~liquid/d1-ultra-latest` | 81% · 95.2% · 9/13 | 65% · 97.0% · 9/11 | 48% · 100.0% · 9/9 | 26% · 100.0% · 4/4 |
| `~perplexity/pplx-decider-latest` | 86% · 94.4% · 9/14 | 77% · 96.2% · 8/11 | 70% · 95.8% · 8/11 | 52% · 98.1% · 8/9 |
| `inception/mercury-decide` | 99% · 89.2% · 9/17 | 97% · 90.0% · 9/16 | 97% · 90.0% · 9/16 | 87% · 91.1% · 8/15 |
| `nace-ai/drex-max` | 92% · 92.6% · 9/16 | 84% · 95.4% · 8/12 | 79% · 96.3% · 8/11 | 66% · 98.5% · 8/9 |
| `openai/gpt-6-luna-decisions` | 82% · 95.2% · 9/13 | 63% · 95.4% · 9/12 | 51% · 96.2% · 7/9 | 42% · 95.3% · 4/6 |
| `baseten/mammoth-1` | 91% · 89.4% · 9/19 | 75% · 89.6% · 9/17 | 68% · 91.4% · 9/15 | 53% · 94.5% · 8/11 |
| `jaredpalmer/kev-4b` | 54% · 87.5% · 8/15 | 34% · 91.4% · 3/6 | 18% · 100.0% · 2/2 | 4% · 100.0% · 0/0 |

At t = 0.8 jev-1.14 answers 66% of pairs at 98.5% accuracy; its retire-class verdicts are 8 of 9 right (the miss: `pro-d177>pro-d171`, gold contradicts, called supersedes at 0.95). d1-ultra-latest is 9 of 9 at 0.8 but answers only 48%. n of retire verdicts at the gate is 9-11: one pair moves precision 10 points.

## Reliability (4 confidence bins)

Cell = correct / answered in the bin of pi's `confidence` (0-0.25, 0.25-0.5, 0.5-0.75, 0.75-1.0). ECE = 4-bin expected calibration error on the peak label probability (a probability, unlike `confidence`).

| Model | 0-0.25 | 0.25-0.5 | 0.5-0.75 | 0.75-1.0 | ECE |
|---|---|---|---|---|---|
| `typesafe/jev-1.14` | 2/3 (67%, mean conf 0.16) | 5/9 (56%, mean conf 0.38) | 15/19 (79%, mean conf 0.63) | 70/72 (97%, mean conf 0.94) | 0.038 |
| `~typesafe/jev-latest` | 2/3 (67%, mean conf 0.18) | 10/13 (77%, mean conf 0.42) | 14/18 (78%, mean conf 0.65) | 67/69 (97%, mean conf 0.95) | 0.048 |
| `~liquid/d1-ultra-latest` | 5/6 (83%, mean conf 0.17) | 8/14 (57%, mean conf 0.38) | 24/27 (89%, mean conf 0.66) | 55/56 (98%, mean conf 0.90) | 0.102 |
| `~perplexity/pplx-decider-latest` | 0/2 (0%, mean conf 0.21) | 8/12 (67%, mean conf 0.41) | 11/13 (85%, mean conf 0.64) | 73/76 (96%, mean conf 0.93) | 0.055 |
| `inception/mercury-decide` | 0/0 | 0/1 (0%, mean conf 0.49) | 1/2 (50%, mean conf 0.62) | 90/100 (90%, mean conf 0.97) | 0.084 |
| `nace-ai/drex-max` | 0/1 (0%, mean conf 0.18) | 3/7 (43%, mean conf 0.37) | 7/11 (64%, mean conf 0.62) | 81/84 (96%, mean conf 0.95) | 0.017 |
| `jaredpalmer/kev-4b` | 9/18 (50%, mean conf 0.16) | 20/29 (69%, mean conf 0.38) | 24/29 (83%, mean conf 0.65) | 25/27 (93%, mean conf 0.84) | 0.103 |

jev-1.14 and drex-max are the best calibrated (ECE 0.038, 0.017) and accuracy mostly rises with the bin; d1-ultra is better at the top bin but its ECE is 0.102. Most mass sits in the top bin, so the three lower bins hold 3-27 pairs: one pair moves a bin's rate 4-33 points.

## Escalation ladder

Rule (computed from the per-pair results above, no new calls): the classifier answers; if its verdict is `contradicts`, or is `supersedes`/`duplicate` with confidence below the gate, the pair goes to `vertex/gemini-3.8-flash`; an `independent` verdict, and a retire-class verdict at or above the gate, are accepted. Gate = the lowest of 0.5, 0.7, 0.8, 0.9 with the fewest wrong retires kept (chosen on these same 103 pairs: tuned on the test, so read it as optimistic). **Circular:** Flash's escalated verdict is taken as gold, because gold needs Flash and Sonnet to agree on both runs; so end accuracy is an upper bound. If Flash were right on 89% of escalated pairs (guess: the 89.0% two-model agreement rate of the set file), end accuracy of jev-1.14 would be about 96.1%. Flash cost $2.78 per 1,000 pairs (judge file, 2,020 in / 336 out per call, 52-run fit; unverified for this prompt). "Unhunted" = the 59 gold pairs from the same-topic and cross-topic strata, closest to a live write mix.

| Classifier | Gate | Escalated (pairs) | End accuracy | Wrong retire kept: indep / contradicts | Retire missed | $ per 1,000 pairs | $ per write k=8 | Escalated on the 59 unhunted pairs |
|---|---|---|---|---|---|---|---|---|
| `typesafe/jev-1.14` | 0.8 | 26% (27) | 99.0% | 0 / 1 | 0 | 0.766 | 0.00613 | 5% (3/59) |
| `~typesafe/jev-latest` | 0.9 | 25% (26) | 99.0% | 0 / 1 | 0 | 0.739 | 0.00591 | 3% (2/59) |
| `~liquid/d1-ultra-latest` | 0.8 | 27% (28) | 100.0% | 0 / 0 | 0 | 0.780 | 0.00624 | 2% (1/59) |
| `~perplexity/pplx-decider-latest` | 0.9 | 25% (26) | 98.1% | 0 / 1 | 0 | 0.714 | 0.00571 | 3% (2/59) |
| `inception/mercury-decide` | 0.7 | 19% (20) | 92.2% | 1 / 6 | 0 | 0.552 | 0.00442 | 5% (3/59) |
| `nace-ai/drex-max` | 0.9 | 25% (26) | 98.1% | 0 / 1 | 0 | 0.739 | 0.00592 | 3% (2/59) |
| `openai/gpt-6-luna-decisions` | 0.8 | 27% (28) | 97.1% | 0 / 2 | 0 | 0.818 | 0.00654 | 5% (3/59) |
| `baseten/mammoth-1` | 0.9 | 26% (27) | 97.1% | 0 / 3 | 0 | 0.792 | 0.00634 | 5% (3/59) |
| `jaredpalmer/kev-4b` | 0.8 | 38% (39) | 99.0% | 0 / 0 | 0 | 1.076 | 0.00861 | 8% (5/59) |
| `togethercomputer/tev1-4b-experimental` | 0.9 | 25% (26) | 88.3% | 2 / 8 | 0 | 0.728 | 0.00583 | 12% (7/59) |

Free-text ladder for comparison (gemini-3.1-flash-lite, no usable confidence, so every retire-class verdict and `contradicts` goes to Flash): 41 of 103 pairs escalated (39.8%, gold prompt), end accuracy 100% (circular), $1.70 per 1,000 pairs; judge-file prompt 43 of 103 (41.7%), $1.74. The classifier ladder escalates 26-27% at about 0.45x the cost, from a first pass that is 16x cheaper and 9x faster. No retire-class gold pair was missed by any ladder in the table (the 9 retire-class gold pairs are called `independent` by none of them).

## Local run arithmetic (`unverified`: nothing installed or pulled)

Every figure here is `unverified` arithmetic, not a measurement. Weights public (OpenRouter `hugging_face_id`, `/api/v1/models?output_modalities=all`, 2026-10-29): `jaredpalmer/kev-4b` (LoRA + pointer head on Qwen3.5-4B-Base), `nace-ai/drex-v1.5` (9B), `cloudflare/clef-flash` (9B, Qwen3.5-9B fine-tune), `baseten/mammoth-1` (27B). Not public (no `hugging_face_id`, size not stated): jev, Liquid d1, Perplexity decider (27B stated for v1.2, flash 4B, no weights id), mercury-decide, drex-max, gpt-6-luna. The three best-value public-weight models (accuracy per dollar, small enough to consider): kev-4b, drex-v1.5, clef-flash; mammoth-1 is the best public-weight accuracy (86.4%) and is shown for contrast.

Formula (same assumptions as `rfm-curation-judge.md`, `unverified`): Q4 size = params x 0.606 GB (about 4.85 bits per weight); needs size + 0.15 GB x (params/8B) KV + 0.5 GB runtime; decode t/s = 0.6 x memory bandwidth / size; prefill t/s = efficiency x peak TFLOPS / (2 x params), efficiency 0.2 (RTX 2060) or 0.3; seconds per pair = tokens in / prefill + tokens out / decode, tokens in/out = the measured means per model (kev 555 / 81, drex 555 / 81, clef-flash 673 / 0, mammoth 634 / 1; whether a local System One server decodes those output tokens is `unverified`). Bandwidth and peak from memory, `unverified`: RTX 2060 336 GB/s and 52 TFLOPS FP16 tensor, GTX 1050 Ti 112 GB/s and 2.1 TFLOPS, M4 Pro 273 GB/s and 9.2 TFLOPS. A write is k sequential pair calls; the hosted column is the measured p50 per call times k (hosted calls could also run in parallel: not measured).

| Model | Params | Q4 size | RTX 2060 6 GB | GTX 1050 Ti 4 GB | M4 Pro 48 GB | Hosted, measured p50 |
|---|---|---|---|---|---|---|
| `jaredpalmer/kev-4b` | 4B | 2.4 GB (need 3.0) | 1.4 s/pair; 7 s (k=5); 11 s (k=8) | 10.0 s/pair; 50 s (k=5); 80 s (k=8) | 2.8 s/pair; 14 s (k=5); 22 s (k=8) | 0.77 s/pair; 3.9 s (k=5); 6.2 s (k=8) |
| `nace-ai/drex-v1.5` | 9B | 5.5 GB (need 6.1) | does not fit (needs 6.1 GB) | does not fit (needs 6.1 GB) | 6.3 s/pair; 32 s (k=5); 50 s (k=8) | 0.48 s/pair; 2.4 s (k=5); 3.8 s (k=8) |
| `cloudflare/clef-flash` | 9B | 5.5 GB (need 6.1) | does not fit (needs 6.1 GB) | does not fit (needs 6.1 GB) | 4.4 s/pair; 22 s (k=5); 35 s (k=8) | 0.37 s/pair; 1.9 s (k=5); 3.0 s (k=8) |
| `baseten/mammoth-1` | 27B | 16.4 GB (need 17.4) | does not fit (needs 17.4 GB) | does not fit (needs 17.4 GB) | 12.5 s/pair; 63 s (k=5); 100 s (k=8) | 0.16 s/pair; 0.8 s (k=5); 1.3 s (k=8) |

Reading (`unverified`): only the 4B model fits the 4 GB card, and then at 10 s per pair; the 9B models just miss the 6 GB card at Q4 (6.1 GB needed vs 6.0). Hosted jev-1.14 (114 ms p50, 0.9 s for k=8 sequentially, `guess` from the measured p50) is faster than every local estimate, and the best local candidate (kev-4b, 75.7%) is 14 points less accurate. **Hosting path:** pi lists llama.cpp decision models (Julia-1, Laya, Kev, lev, OpenJev) through the router's `/v1/systemone` endpoint, which needs llama.cpp 0.6.0 or later (`docs/llama-cpp.md#classification`); chat models are listed as classifiers with the `llama-cpp-classify` API and read next-token probabilities. `not run`: no router was started; whether the hosted classifiers above exist as GGUF decision models is unverified.

## Recommendation

| Decision | Choice |
|---|---|
| Classifier for v1 | `typesafe/jev-1.14` (pinned), pi `choice` question as above, shadow mode first. Second candidate for the shadow run: `~liquid/d1-ultra-latest` (0 wrong retires at gate 0.8, ECE 0.102, cheaper at $0.025 per 1,000 pairs). Neither is separable from the others on 103 pairs |
| Threshold | confidence >= 0.8 (jev-1.14: 66% of pairs answered, 98.5% accurate); chosen on the test pairs, so confirm on new pairs before relying on it |
| Escalation rule | `contradicts`, or a retire-class verdict below 0.8, goes to `vertex/gemini-3.8-flash`; an `independent` verdict is accepted. Even above 0.8 a retire-class verdict stays a pending proposal for the User (1 of 9 such verdicts was a wrong retire) |
| Numbers | ladder escalates 26% of the hunted-mix pairs and 5% of the unhunted pairs (3 of 59): $0.77 and $0.18 per 1,000 pairs, $0.0061 and $0.0014 per 8-neighbour write (hosted; Flash price from the judge file); first pass p50 114 ms |
| Free-text judge | keep `vertex/gemini-3.1-flash-lite` only as the comparison: 68-71% on the same pairs, wrong on 22-23 of 24 partial overrules |

Still unmeasured: `duplicate` (1 gold pair) and `supersedes` (8) are too few for precision or recall; 21 of 24 gold `contradicts` pairs were hunted; whether a partial overrule should be `contradicts` is a User decision (the gold says yes; the classifiers split on it); gold is two-model agreement and the 15 disputed pairs are excluded; run-to-run noise of a classifier (1 pair, 3 calls only); one prompt layout only (state key names and the criteria wording were not varied); live write mix and top-k retrieval quality (Plan 1); the escalated accuracy of Flash against humans; billed (not catalog) cost; throughput limits and rate limits (mercury-decide:free failed with 404, 5 models with 400). **Privacy:** record text goes to OpenRouter, a third-party router, and from there to each model vendor; Bifrost is the only route that stays inside our gateway. Using these classifiers on private Contexts needs a decision on that.

## Findings to record

- 29 classifier runs (28 models + 1 variant) on 103 gold pairs, 2,987 calls: top 8 at 88-90% (Wilson 81-95%), jev-latest first by one pair; 17 listed models had no credentials, 6 errored; no chat model is exposed as a classifier; `temperature` has no effect on this route.
- Free-text vertex/gemini-3.1-flash-lite on the same 103 pairs: 68.0% (judge prompt) and 70.9% (gold prompt), p50 1.0 s, $0.58 per 1,000 pairs; it calls 22-23 of 24 gold `contradicts` pairs `supersedes`. jev-1.14: 89.3%, p50 114 ms, $0.037.
- The costly error is a partial overrule called `supersedes`: jev-1.14 gets 8 of 24 gold `contradicts` wrong that way and 1 of 70 gold `independent`; kev-4b falls from 92.5% (40 proxy pairs) to 75.7% (103 gold pairs), tev1 to 65.0%.
- Confidence gate 0.8 on jev-1.14: 66% of pairs answered at 98.5% accuracy, retire-class verdicts 8 of 9 right (n=9); reliability: 70 of 72 right at confidence >= 0.75, ECE 0.038. All figures are measured on the same pairs the gate was chosen on.
- Ladder (classifier, then gemini-3.8-flash for `contradicts` and retire verdicts under 0.8): jev-1.14 escalates 26% (5% on unhunted pairs), $0.77 and $0.18 per 1,000 pairs, 1 wrong retire kept; end accuracy 99% is an upper bound (Flash verdict is gold by construction).
- Local arithmetic, all `unverified`: kev-4b Q4 fits a 4 GB card at about 10 s per pair; 9B models do not fit 6 GB at Q4; hosted jev p50 114 ms beats every local estimate; hosting would use llama.cpp 0.6.0+ `/v1/systemone`, not run.
- Recommendation: jev-1.14 pinned, gate 0.8, escalate `contradicts` and low-confidence retire verdicts to Flash, retire proposals still approved by the User; unmeasured: `duplicate` (1 pair), `supersedes` (8), run-to-run noise, human gold.
- Privacy: classifier calls send record text to OpenRouter, a third-party router, and on to the model vendor; Bifrost keeps text inside our gateway.
