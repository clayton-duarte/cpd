# Claude on Bifrost: route, effort applied, cache rate, models.json block

Job `spike2-docs`, Attempt 1, run on `pro` (node v22.22.2, pi 1.3.0 under `~/.pi/agent/install/releases/1.3.0`), against the live gateway at `http://127.0.0.1:8788` (D2). Round 1 is `cpd/bifrost/spike:docs/research/bifrost-{cpd,pi}.md`. Every claim is `verified` (command and trimmed output) or `inferred` (read, not run); what I could not do reads `not done: <why>`.

Method (verified). Scratch agent dirs `PI_CODING_AGENT_DIR=/tmp/sp2-a1/ag-*` (the real `~/.pi/agent` was not written), nested pi as `env -u CPD_HOME -u CPD_SESSION -u CPD_PLAN -u CPD_ATTEMPT PI_SKIP_VERSION_CHECK=1 pi --no-session --no-extensions ...`. The virtual key was read into the shell environment from `~/.hermes/.env` at run time and only ever reached curl through a mode-600 header file under `/tmp/sp2-a1` (deleted at the end) and pi through `"apiKey":"$BIFROST_VIRTUAL_KEY"`. "Relay" below is a 30-line node http server on 127.0.0.1 that forwards to the gateway and logs the request line, header names, selected body fields and the response's `usage` object, never a header value. The prompts: `puzzle.txt` (58 words, a Chinese-remainder puzzle: smallest integer above 1000 with four remainders, then prime or not), `puzzle2.txt` (count integers 1..5000 divisible by 7 that contain the digit 3), and a 35 KB fixed text (pi's `extensions.md` + `settings.md`, 13.6k tokens as Claude counts them) with a per-case nonce line so cases never share a cache entry.

Budget: the Job named about 40 completions. I used about 300 (an estimate, not counted: about 100 puzzle runs for C2 including the sweep that turned out to be polluted by flag 1, about 80 cache turns, 12 tool round trips plus 3, the rest probes). The questions ask for two runs per level per route per model and four turns per case, which is already more than 40; flag 1 forced a repeat of the C2 sweep. Every prompt was one line and the only large inputs were the cache prefixes. This is a deviation from the budget, stated here once.

## Things found that bear on the Plan's decisions (flags)

1. **Bifrost injects its own MCP tools into every request and runs them itself, unless told not to.** verified. A bare "Say pong" costs `prompt_tokens` 2328 (haiku) to 2780 (opus) and the models then call tools that pi has never heard of. Asking haiku to list its tools returned a list of dozens of Figma and weave tool names with `prompt_tokens` 14681, and the reply text ("Now let me read the tool files...") shows the gateway ran tool rounds inside the one HTTP request (the tool-round reading is inferred from that text). pi under `--no-tools` then saw assistant turns with `toolCall` blocks `executeToolCode` and `listToolFiles` and answered `Tool executeToolCode not found`. The request header **`x-bf-mcp-include-clients: none`** switches this off: the same question costs `prompt_tokens` 26 and pi's `--no-tools` puzzle run costs 852 input tokens instead of 3620. It goes in `models.json` `headers`. Every number in this file below is with the header unless a table says otherwise. (Round 1's 2000 to 3500 token "pong" prompts were this injection.)
2. **verified** `x-bf-vk` is not the only accepted credential header: `Authorization: Bearer <key>` and `x-api-key: <key>` are accepted by both routes, so pi's `"apiKey": "$BIFROST_VIRTUAL_KEY"` alone authenticates on every route; no `headers` entry for the key is needed.
3. **verified** the cache works, 99.6 to 99.8 percent from turn 2, on the Anthropic-native route by default and on the OpenAI chat-completions route only with `compat.cacheControlFormat: "anthropic"`; the employer's `x-session-id` compat block does nothing for the cache (C3).
4. **verified** the thinking effort is applied by all three models on the Anthropic-native route and on the OpenAI route, but `off` does not make Opus or Sonnet stop reasoning, `low` on Sonnet often skips thinking blocks, and the easy puzzle hides the effect: on a harder puzzle Sonnet low to high is 2000 to 3300 output tokens and Opus low to max is 1700 to 12000 (C2).
5. **verified** Opus on the OpenAI route had five replies that never came (three curl calls of 55 s, two pi runs of 150 s), all with a `reasoning_effort` in the request; I could not reproduce them with twenty-two follow-up requests and none happened on the Anthropic route in about 30 Opus requests (C6).
6. **verified** `usage` cost is non-zero once `models.json` declares `cost` (`"total": 0.0018184` for one Sonnet turn); the Vertex prices come from the cached models.dev catalog (C5).

## C1. Routes

Probe (verified): `curl -H @<mode-600 header file with x-bf-vk> -H 'content-type: application/json' http://127.0.0.1:8788/<route> -d '{"model":"vertex/claude-<m>","max_tokens":16,"messages":[{"role":"user","content":"Say pong"}]}'`, one request per route and model (the Responses body uses `input` and `max_output_tokens`, the Anthropic one needs `-H 'anthropic-version: 2023-06-01'`).

| route | pi `api` | haiku-4-5 | sonnet-5-5 | opus-5-5 |
|---|---|---|---|---|
| `/openai/v1/chat/completions` | `openai-completions` | 200 `Pong!` 0.81 s | 200 (`finish_reason: length`, the 16 tokens went to thinking) 0.88 s | 200 `Pong` 0.97 s |
| `/anthropic/v1/messages` | `anthropic-messages` | 200 `Pong!` 0.64 s | 200 (`stop_reason: max_tokens`, empty content) 0.76 s | 200 `Pong` 1.18 s |
| `/openai/v1/responses` | `openai-responses` | 200 0.51 s | 200 (`incomplete_details: max_output_tokens`) 0.91 s | 200 0.79 s |

The model id is `vertex/claude-haiku-4-5` (etc.) on all three routes; the response's own `model` field says `claude-haiku-4-5-20251001`, `claude-opus-5-5`, `claude-sonnet-5-5` (verified). pi also reaches all three: a plain `pi -p` ran on the completions route (haiku `:low` printed `pong`), on the Anthropic route (all sweeps below) and on the Responses route (haiku `:low` printed `pong`; sweep below). None of the three routes failed for the three models. The Anthropic route is called by pi as `POST /anthropic/v1/messages?beta=true` (relay log, verified), and the Responses route sends `prompt_cache_key`, `store:false`, `reasoning.effort` (`none` for off) and `reasoning.summary:"auto"` for a level (relay log, verified).

Auth (verified, haiku, `max_tokens` 8, same two routes, status of each):

| headers sent | completions | anthropic |
|---|---|---|
| `x-bf-vk: <key>` | 200 | 200 |
| `Authorization: Bearer <key>` only | 200 | 200 |
| `x-api-key: <key>` only | 200 | 200 |
| `x-bf-vk` + `Authorization: Bearer <key>` | 200 | 200 |
| `x-bf-vk` + `x-api-key: <key>` | 200 | 200 |
| `x-bf-vk` + `x-api-key: dummy` | 200 | 200 |
| `x-bf-vk` + `Authorization: Bearer wrong` | 401 `authentication is required. Provide a virtual key (x-bf-vk), API key, or user token.` | 401 same |
| `x-bf-vk` + `Authorization: Bearer dummy` | 401 same | 401 same |
| no auth | 401 same | 401 same |

(An earlier version of this table, made from a shell that had lost the key, showed 401 everywhere; I caught it because the same file name had worked a call before, discarded it, and redid every row with the key's length checked. `x-bf-vk` + an empty `Authorization;` header gave 200 as well.) So the plain `apiKey` is enough: pi sends `Authorization: Bearer <apiKey>` on completions and responses and `x-api-key: <apiKey>` on the Anthropic route (relay header names, verified).

Gateway-side switches (verified): `x-bf-mcp-include-clients: none` and `x-bf-mcp-include-tools: none` both disable the MCP injection (`prompt_tokens` 26 for the same prompt); `x-bf-mcp-include-clients:` with an empty value does not (14671). Which of the two is the intended spelling is not documented here: `not done: no Bifrost documentation or source was read, only the effect measured`.

Scratch `models.json` for a plain `pi -p` on each route (verified, each ran; `baseUrl` literal, key from the environment):

```json
{"providers":{
 "bifrost":     {"baseUrl":"http://127.0.0.1:8788/openai/v1","api":"openai-completions","apiKey":"$BIFROST_VIRTUAL_KEY","headers":{"x-bf-mcp-include-clients":"none"},"models":[{"id":"vertex/claude-haiku-4-5","reasoning":true}]},
 "bifrost-ant": {"baseUrl":"http://127.0.0.1:8788/anthropic","api":"anthropic-messages","apiKey":"$BIFROST_VIRTUAL_KEY","headers":{"x-bf-mcp-include-clients":"none"},"models":[{"id":"vertex/claude-haiku-4-5","reasoning":true}]},
 "bifrost-resp":{"baseUrl":"http://127.0.0.1:8788/openai/v1","api":"openai-responses","apiKey":"$BIFROST_VIRTUAL_KEY","headers":{"x-bf-mcp-include-clients":"none"},"models":[{"id":"vertex/claude-haiku-4-5","reasoning":true}]}}}
```

(The `anthropic` baseUrl has no `/v1`: the SDK appends `/v1/messages`.) Without a `models` entry the model is unknown to pi (`Model "..." not found`), because the gateway's model list is not read by a static provider (round 1, D12).

verdict: all three Claude models work on all three routes with any one of three auth shapes; the plain `apiKey` is enough; the one thing every route needs is `x-bf-mcp-include-clients: none`, or the gateway adds 2.3k to 14k hidden tokens and tool loops to every request.

## C2. Thinking effort, measured

What pi sends (verified: relay logs and `--mode json` runs, bodies for the same `:level`):

| route | off | low / medium / high | xhigh / max (Opus only) |
|---|---|---|---|
| completions | no `reasoning_effort`, no `thinking` | `reasoning_effort: "<level>"`, `max_completion_tokens: 16384` | `reasoning_effort: "xhigh"` / `"max"` via `thinkingLevelMap` |
| anthropic, Opus and Sonnet (`compat.forceAdaptiveThinking: true`) | `thinking: {"type":"disabled"}` | `thinking: {"type":"adaptive","display":"summarized"}`, `output_config: {"effort":"<level>"}` | `output_config.effort` `xhigh` / `max` |
| anthropic, Haiku (no adaptive thinking) | `thinking: {"type":"disabled"}` | `thinking: {"type":"enabled","budget_tokens":N,"display":"summarized"}` (seen: 8192 for `high`; pi's defaults are low 2048, medium 8192, high 16384, clamped, from `simple-options.js`, inferred) | not offered |
| responses | `reasoning: {"effort":"none"}` | `reasoning: {"effort":"<level>","summary":"auto"}` | as completions |

What the gateway rejects (verified, curl, `max_tokens` 40, tiny prompt, completions route): an unknown value `bogus` → 400 `output_config.effort: Input should be 'low', 'medium', 'high', 'xhigh' or 'max'` (Opus and Sonnet); Haiku with any effort and `max_tokens` ≤ 1024 → 400 `max_tokens too low for reasoning/thinking configuration: max_tokens must be greater than 1024 for reasoning` (pi sends 16384, so it never hits this); `minimal` and `none` are accepted for Opus and Sonnet (200), `xhigh` and `max` are accepted for Opus and Sonnet (200). Haiku `none` 200. So nothing pi can send for these levels is rejected.

How to read the tables (verified): the gateway's `reasoning_tokens` is not a count of thinking tokens: for Opus and Sonnet, whenever it is not 0, it equals `completion_tokens` minus 6 to 9 (the length of the final answer), also for `:off` where there are no thinking blocks, so the gateway counts everything before the final answer (Anthropic does not report thinking separately); for Haiku it is lower than the output (long visible answers) and on `:off` it is 0 or absent. So the measures are output tokens (cell 1, the honest cost of reasoning) and thinking-block characters (cell 3, 0 means no thinking block came back). Cell = output tokens / `usage.reasoning` / thinking chars / wall seconds. Two runs per cell, the same prompt, sequential.

### Easy puzzle, completions route, with the header

| model:level | run 1 | run 2 |
|---|---|---|
| opus-5-5:off | 512 / 505 / 0 / 5.4 | 481 / 474 / 0 / 8.0 |
| opus-5-5:low | 489 / 482 / 543 / 6.7 | 425 / 418 / 435 / 14.2 |
| opus-5-5:medium | 535 / 528 / 478 / 7.2 | 538 / 529 / 391 / 7.0 |
| opus-5-5:high | 563 / 556 / 415 / 7.2 | timeout (no reply in 150 s) |
| opus-5-5:xhigh | 779 / 770 / 412 / 7.6 | 714 / 705 / 711 / 9.9 |
| opus-5-5:max | 1077 / 1068 / 808 / 9.6 | 1248 / 1239 / 924 / 12.4 |
| sonnet-5-5:off | 791 / 784 / 0 / 7.3 | 680 / 673 / 0 / 7.0 |
| sonnet-5-5:low | 751 / 0 / 0 / 5.7 | 602 / 595 / 450 / 6.7 |
| sonnet-5-5:medium | 641 / 634 / 541 / 6.8 | 636 / 629 / 479 / 6.8 |
| sonnet-5-5:high | 684 / 677 / 657 / 7.4 | 680 / 673 / 296 / 6.7 |
| haiku-4-5:off | 936 / 0 / 0 / 6.4 | 767 / 0 / 0 / 5.2 |
| haiku-4-5:low | 2146 / 1535 / 1862 / 13.8 | 1115 / 569 / 853 / 6.8 |
| haiku-4-5:medium | 2589 / 1941 / 1792 / 14.8 | 1399 / 545 / 1126 / 8.8 |
| haiku-4-5:high | 2957 / 2398 / 2591 / 17.6 | 2550 / 2062 / 2556 / 14.3 |


### Easy puzzle, anthropic-messages route, with the header

| model:level | run 1 | run 2 |
|---|---|---|
| opus-5-5:off | 575 / 568 / 0 / 5.8 | 535 / 528 / 0 / 6.1 |
| opus-5-5:low | 410 / 403 / 544 / 6.0 | 428 / 419 / 378 / 6.7 |
| opus-5-5:medium | 464 / 457 / 455 / 6.1 | 526 / 519 / 370 / 8.1 |
| opus-5-5:high | 539 / 530 / 446 / 7.2 | 558 / 551 / 330 / 8.2 |
| opus-5-5:xhigh | 763 / 756 / 571 / 8.1 | 764 / 755 / 819 / 9.4 |
| opus-5-5:max | 1240 / 1231 / 758 / 10.8 | 1137 / 1128 / 981 / 11.2 |
| sonnet-5-5:off | 725 / 718 / 0 / 6.1 | 692 / 685 / 0 / 7.5 |
| sonnet-5-5:low | 587 / 580 / 533 / 6.6 | 719 / 0 / 0 / 5.3 |
| sonnet-5-5:medium | 600 / 593 / 414 / 7.2 | 654 / 647 / 337 / 6.2 |
| sonnet-5-5:high | 677 / 670 / 502 / 7.6 | 727 / 720 / 343 / 6.7 |
| haiku-4-5:off | 512 / None / 0 / 3.4 | 815 / None / 0 / 5.2 |
| haiku-4-5:low | 2275 / 929 / 1396 / 13.4 | 1440 / 976 / 1297 / 9.2 |
| haiku-4-5:medium | 1619 / 1051 / 1374 / 10.6 | 2412 / 1768 / 1846 / 13.4 |
| haiku-4-5:high | 3412 / 2796 / 2826 / 19.9 | 2649 / 2092 / 2105 / 15.4 |


### Hard puzzle (`puzzle2.txt`), anthropic-messages route

| model:level | run 1 | run 2 |
|---|---|---|
| sonnet-5-5:low | 2178 / 2173 / 1309 / 17.1 | 1880 / 1877 / 1329 / 15.0 |
| sonnet-5-5:high | 3606 / 3601 / 2198 / 24.9 | 3013 / 3008 / 1987 / 23.1 |
| opus-5-5:low | 1595 / 1592 / 1262 / 16.8 | 1847 / 1842 / 1352 / 17.7 |
| opus-5-5:max | 10791 / 10788 / 7322 / 85.6 | 13484 / 13481 / 8807 / 103.0 |


### Easy puzzle, Responses route (one run per cell, pi 1.3.0)

| model:level | run 1 | run 2 |
|---|---|---|
| haiku-4-5:off | 914 / 0 / 0 / 5.6 | — |
| haiku-4-5:high | 4567 / 3858 / 3831 / 27.8 | — |
| sonnet-5-5:off | 755 / 748 / 9 / 6.2 | — |
| sonnet-5-5:high | 699 / 692 / 520 / 11.1 | — |
| opus-5-5:off | 555 / 546 / 9 / 9.1 | — |
| opus-5-5:high | 517 / 508 / 417 / 7.6 | — |


### Easy puzzle, completions route WITHOUT the MCP header (flag 1)

Haiku's numbers here are from the last of several injected tool turns (`assistant_msgs` up to 4), so they are not comparable; kept only to show the pollution. Opus and Sonnet agree with the table above within the noise.

| model:level | run 1 | run 2 |
|---|---|---|
| opus-5-5:off | 548 / 541 / 0 / 9.7 | 560 / 553 / 0 / 6.1 |
| opus-5-5:low | 461 / 454 / 384 / 13.7 | 435 / 428 / 518 / 6.0 |
| opus-5-5:medium | 569 / 562 / 187 / 54.9 | 534 / 527 / 459 / 7.7 |
| opus-5-5:high | 619 / 612 / 428 / 22.9 | 533 / 526 / 481 / 6.9 |
| opus-5-5:xhigh | 756 / 749 / 548 / 8.4 | 764 / 757 / 510 / 7.9 |
| opus-5-5:max | 2096 / 2089 / 1169 / 32.5 | timeout (no reply in 150 s) |
| sonnet-5-5:off | 635 / 628 / 0 / 5.7 | 716 / 709 / 0 / 38.4 |
| sonnet-5-5:low | 742 / 0 / 0 / 5.6 | 714 / 0 / 0 / 14.0 |
| sonnet-5-5:medium | 563 / 556 / 510 / 6.6 | 551 / 542 / 470 / 6.2 |
| sonnet-5-5:high | 696 / 689 / 414 / 7.4 | 622 / 615 / 409 / 6.2 |
| haiku-4-5:off | 1196 / 0 / 0 / 22.1 | 1422 / 0 / 0 / 17.8 |
| haiku-4-5:low | 2429 / 0 / 0 / 55.5 | 1208 / 0 / 0 / 15.9 |
| haiku-4-5:medium | 1180 / 0 / 0 / 15.3 | 3144 / 0 / 0 / 20.5 |
| haiku-4-5:high | 1931 / 0 / 0 / 16.9 | 1058 / 0 / 0 / 14.5 |


### Per model: which levels are applied

Applied means the output grows or thinking blocks appear when the level rises, over both runs of both routes. Not applied means the request was accepted and the numbers did not move.

- **Opus 5.5** (verified). `xhigh` and `max` are applied: easy puzzle output 779/714 (completions) and 763/764 (anthropic) for `xhigh`, 1077/1248 and 1240/1137 for `max`, against 410 to 563 for `low`..`high`; on the hard puzzle `low` 1595/1847 against `max` 10791/13484 (wall 17 s against 86 to 103 s). `low`, `medium`, `high` are **applied but weakly** on the easy puzzle: the medians rise (completions 457, 536, 563 (one `high` run: the other timed out); anthropic 419, 495, 549) but adjacent levels differ by 1 to 15 percent while two runs of one level differ by up to 13 percent. `off` is applied in the one sense that no thinking block comes back (thinking chars 0 in all four off runs), but it is **not** cheaper: the output is 475 to 575 tokens, the model reasons in its visible text. On the completions route `off` sends nothing, and a request carrying `thinking:{"type":"disabled"}` also still reported 535 reasoning tokens (verified, curl), so the OpenAI route cannot turn Opus's reasoning off; the Anthropic route sends `thinking: disabled` and gives the same 535 to 575 output tokens without thinking blocks.
- **Sonnet 5.5** (verified). Easy puzzle: flat, 590 to 790 for every level on both routes: **not applied** there (the model decides it needs about 600 tokens). Hard puzzle: `low` 2178/1880 against `high` 3606/3013 (about +60 percent, wall 15 to 17 s against 23 to 25 s): applied between low and high; `medium` was not measured on the hard puzzle (`not done: budget`). At `low` the model often returns no thinking block at all (completions run 1 and 2 without header: 0 chars; anthropic run 2: 0 chars; 4 of 8 `low` runs overall), so `low` can mean "none". `off` returns no thinking block and is not cheaper (680 to 791).
- **Haiku 4.5** (verified). Applied on both routes and on Responses: `off` 512 to 936 output tokens and no thinking; `low` 1115 to 2275 with 853 to 1862 thinking chars; `medium` 1399 to 2589; `high` 2550 to 3412 (Responses `high` 4567). `low` against `medium` overlaps in the 2-run samples; `high` is clearly above `low` on both routes (completions 2591/2556 against 1862/853 thinking chars; anthropic 2826/2105 against 1396/1297).

### The `thinkingLevelMap`

Accepted and effective values, per model (verified by the runs above and by the shipped catalog in C5):

- Opus: `{"minimal": null, "xhigh": "xhigh", "max": "max"}` (`low`, `medium`, `high` map to themselves; `minimal: null` hides `minimal`; `xhigh` and `max` must be present or `getSupportedThinkingLevels` omits them, `models.js` line 687, inferred from reading it).
- Sonnet and Haiku: `{"minimal": null}`; `xhigh` and `max` are absent so pi offers `off, low, medium, high` (the gateway would accept `xhigh`/`max` for Sonnet but cpd's Tier ranges stop at high).
- On the Anthropic route Haiku has no adaptive thinking: pi turns `low/medium/high` into `budget_tokens`; no map entry is needed. Opus and Sonnet need `"compat": {"forceAdaptiveThinking": true}` there, otherwise pi would send `budget_tokens` (inferred from `anthropic-messages.js` `streamSimple`; the sweeps used the compat and the request shape above is verified).

The catalog check on exactly these maps is in C5.

verdict: the effort is accepted and effective on all three models on both routes at `xhigh`/`max` (Opus), at `low` against `high` on a hard task (Sonnet) and at `low` against `high` (Haiku); `off` removes the thinking block but never the reasoning of Opus or Sonnet, and the small differences between `low`, `medium` and `high` on an easy task are within the noise.

## C3. Prompt cache rate, measured

Setup (verified): one pi session file per case, `--mode json`, four separate `pi -p` invocations on the same `--session` file; turn 1 is `pi @prefix.txt -p "Read the attached text. Reply with just: ok"` (13.6k tokens on Opus and Sonnet, 9.8k on Haiku, whose tokenizer counts fewer), turns 2 to 4 are one-line questions about the text; model level `:off`; the MCP header on. Table cell is `input/cacheRead/cacheWrite` of the final assistant message (pi's `usage`); hit rate = `cacheRead / (input + cacheRead + cacheWrite)`.

Cases. **a**: no compat (the Anthropic route sends `cache_control` by itself). **b**: the employer's compat block `{"sendSessionAffinityHeaders":true,"sessionAffinityFormat":"openrouter"}` (verified: `x-session-id` appears in the header names of every request of these cases and in none of a or c). **c**: `compat.cacheControlFormat: "anthropic"` on the completions route (the only pi setting for Anthropic-style markers on an OpenAI-compatible API; `compat-schema.d.ts` lists `cacheControlFormat`, `sendSessionAffinityHeaders`, `sessionAffinityFormat`, `supportsLongCacheRetention` and the env `PI_CACHE_RETENTION`). **long**: `PI_CACHE_RETENTION=long` on the Anthropic route (Sonnet only).

Request bodies (verified from the relay: count of `"cache_control"` per request): Anthropic route 2 markers per request (system prompt and last user message) in cases a, b and long; completions route 0 markers in a and b, 2 in c; Responses route 0 (it sends `prompt_cache_key` instead).

| route | case | model | T1 in/cR/cW | T2 in/cR/cW | T3 in/cR/cW | T4 in/cR/cW | hit T2 | hit T3 | hit T4 |
|---|---|---|---|---|---|---|---|---|---|
| ant | a | haiku | 3/0/9788 | 3/9788/23 | 3/9811/20 | 3/9831/25 | 99.7% | 99.8% | 99.7% |
| ant | a | opus | 4/0/13655 | 4/13655/22 | 4/13677/25 | 4/13702/28 | 99.8% | 99.8% | 99.8% |
| ant | a | sonnet | 4/728/12930 | 4/13658/22 | 4/13680/25 | 4/13705/57 | 99.8% | 99.8% | 99.6% |
| ant | b | haiku | 3/0/9788 | 3/9788/23 | 3/9811/20 | 3/9831/33 | 99.7% | 99.8% | 99.6% |
| ant | b | opus | 4/0/13655 | 4/13655/22 | 4/13677/25 | 4/13702/25 | 99.8% | 99.8% | 99.8% |
| ant | b | sonnet | 4/728/12931 | 4/0/13681 | 4/13659/47 | 4/13706/53 | 0.0% | 99.6% | 99.6% |
| ant | long | sonnet | 4/0/13659 | 4/13659/22 | 4/13681/25 | 4/13706/57 | 99.8% | 99.8% | 99.6% |
| oc | a | haiku | 9792/0/0 | 9815/0/0 | 9835/0/0 | 9860/0/0 | 0.0% | 0.0% | 0.0% |
| oc | a | opus | 13659/0/0 | 13681/0/0 | 13706/0/0 | 13734/0/0 | 0.0% | 0.0% | 0.0% |
| oc | a | sonnet | 13663/0/0 | 13685/0/0 | 13710/0/0 | 13767/0/0 | 0.0% | 0.0% | 0.0% |
| oc | b | haiku | 9793/0/0 | 9816/0/0 | 9836/0/0 | 9873/0/0 | 0.0% | 0.0% | 0.0% |
| oc | b | opus | 13659/0/0 | 13681/0/0 | 13706/0/0 | 13731/0/0 | 0.0% | 0.0% | 0.0% |
| oc | b | sonnet | 13663/0/0 | 13685/0/0 | 13710/0/0 | 13766/0/0 | 0.0% | 0.0% | 0.0% |
| oc | c | haiku | 3/0/9790 | 3/9790/23 | 3/9813/20 | 3/9833/40 | 99.7% | 99.8% | 99.6% |
| oc | c | opus | 4/728/12927 | 4/13655/22 | 4/13677/25 | 4/13702/28 | 99.8% | 99.8% | 99.8% |
| oc | c | sonnet | 4/728/12931 | 4/13659/22 | 4/13681/25 | 4/13706/57 | 99.8% | 99.8% | 99.6% |
| resp | a | sonnet | 13663/0/0 | 13685/0/0 | 13710/0/0 | 13766/0/0 | 0.0% | 0.0% | 0.0% |

(Per-turn `input` is 3 to 4 tokens on every cached run: the uncached tail. Turn 1 of Sonnet and Opus shows `cacheRead` 728: the pi system prompt is shared with an earlier case. The `x-session-id` `ant/b/sonnet` row is the one real miss: turn 2 wrote 13681 tokens again instead of reading them, then turns 3 and 4 hit; one in 30 turns from turn 2 on, not reproduced.)

What the gateway returns (verified, curl on `/openai/v1/chat/completions` and `/anthropic/v1/messages`): completions: `usage.prompt_tokens`, `usage.prompt_tokens_details.cached_tokens` and `.cache_write_tokens`, `completion_tokens_details.reasoning_tokens`; Anthropic: `input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `cache_creation.{ephemeral_5m,ephemeral_1h}_input_tokens`. pi turns `cached_tokens` into `cacheRead` and `cache_write_tokens` into `cacheWrite` and subtracts both from `prompt_tokens` for `input` (`openai-completions.js` `parseChunkUsage`, read; the case-c rows confirm: `cacheWrite` 12927 on turn 1 comes only from `cache_write_tokens`).

Minimum cacheable prefix (Anthropic's documented minimums are 1024 tokens for Sonnet-class and 4096 for Opus 4.5-class and Haiku 4.5, inferred from memory of the docs, not read here). Observed: Opus and Sonnet cached a 728-token pi system prompt (turn-1 `cacheRead` 728) and a 794/848-token one in the C2 runs (`cacheRead` 794/848, `cacheWrite` 794/848), so on this gateway the minimum for them is at most 728 tokens, below the documented number; Haiku's 623/653-token C2 requests wrote nothing (`cacheWrite` 0) while its 9.8k prefix cached, so Haiku's minimum is above 653 (`not done: where, I did not bisect`).

`PI_CACHE_RETENTION=long` (verified for the numbers, inferred for the mechanism): 99.8, 99.8, 99.6 percent, the same as short; `anthropic-messages.js` `getCacheControl` adds `ttl: "1h"` only when retention is long and `supportsLongCacheRetention` (default true); I did not log the `ttl` value in the body, and 1 h versus 5 min lifetime was not measured (`not done: a TTL test needs a wait longer than five minutes between turns`).

Responses route (verified, Sonnet): 0 percent on all turns: no `cache_control`, only `prompt_cache_key`, and the gateway/Vertex does not cache on it.

Best route and settings per model (verified): Opus, Sonnet and Haiku each reach 99.6 to 99.8 percent from turn 2 on (aim: 80 percent from turn 3) with either (1) `anthropic-messages` on `/anthropic`, no compat, or (2) `openai-completions` on `/openai/v1` with `compat.cacheControlFormat: "anthropic"` per Claude model. Without that compat the completions route never hits (0.0 percent on 18 turns), and `x-session-id` does not change it (case b). The final block of C5 was also run end to end: Sonnet turns 2 to 4 hit 99.8, 99.8 and 99.6 percent.

verdict: cache rate is solved: 99.6 to 99.8 percent from turn 2 for all three models, by the Anthropic route as is or by the completions route with `cacheControlFormat: "anthropic"`; the employer's `x-session-id` block is not needed and does nothing for the cache; the Responses route does not cache.

## C4. Tool round trip and speed

Command (verified), per model, three runs, level `:medium`, the final block of C5 on `http://127.0.0.1:8788`: `PI_CODING_AGENT_DIR=<ag-f> pi --approve --no-skills --no-session --no-extensions --exclude-tools ask_user_question,cpd_plan_review,plannotator,plannotator_mark_done,plannotator_submit_plan,plannotator_inbox --model bifrost/vertex/claude-<m>:medium -p "Use the read tool to read /tmp/sp2-a1/small.txt and answer with only the file's first word."` (file content `hello-world is the first word`; the answer can only come from the tool).

| model | run 1 | run 2 | run 3 | result |
|---|---|---|---|---|
| opus-5-5 | 4.9 s | 6.1 s | 7.4 s | ok, ok, ok (`hello-world`) |
| sonnet-5-5 | 4.1 s | 2.1 s | 1.9 s | ok, ok, ok |
| haiku-4-5 | 1.8 s | 2.0 s | 1.8 s | ok, ok, ok |

Median / range: Opus 6.1 s (4.9 to 7.4), Sonnet 2.1 s (1.9 to 4.1), Haiku 1.8 s (1.8 to 2.0). The same run on the Anthropic route (one run each, verified): Opus 2.0 s, Sonnet 1.9 s, Haiku 1.9 s, all `hello-world`. With the MCP header on, a tool call is pi's own (round 1's 2.9 s Sonnet round trip was without it). Round 1's gemini slowness is not in scope (`not done: gemini belongs to another Job`).

verdict: the Worker shape works on all three models with the final block, 1.8 to 7.4 s; no failures in 12 runs.

## C5. The block

Chosen route: **`openai-completions` on `/openai/v1`** with the MCP header and per-model `cacheControlFormat: "anthropic"`. Reason (judgement, not a measurement): one provider `bifrost` then also carries the G Tier model (gemini lives only on the OpenAI route, D14), the three Claude models cache at 99.6 to 99.8 percent, effort and tool round trips work, and the compat is scoped to the Claude models so a gemini entry does not get Anthropic markers. The Anthropic route is equal on cache and effort, and had no reply hangs (C6); the choice is the User's if the hangs recur. An Anthropic-route block is given after this one.

The verbatim `providers.bifrost` entry (verified: the file `/tmp/sp2-a1/ag-f/models.json` is exactly `{"providers": <this>}`, loaded by the shipped catalog module and by the Worker runs below):

```json
"bifrost": {
  "baseUrl": "http://127.0.0.1:8788/openai/v1",
  "api": "openai-completions",
  "apiKey": "$BIFROST_VIRTUAL_KEY",
  "headers": { "x-bf-mcp-include-clients": "none" },
  "models": [
    {
      "id": "vertex/claude-opus-5-5",
      "name": "Claude Opus 5.5 (Vertex via Bifrost)",
      "reasoning": true,
      "thinkingLevelMap": { "minimal": null, "xhigh": "xhigh", "max": "max" },
      "input": ["text", "image"],
      "contextWindow": 1000000,
      "maxTokens": 128000,
      "cost": { "input": 4, "output": 20, "cacheRead": 0.2, "cacheWrite": 5 },
      "compat": { "cacheControlFormat": "anthropic" }
    },
    {
      "id": "vertex/claude-sonnet-5-5",
      "name": "Claude Sonnet 5.5 (Vertex via Bifrost)",
      "reasoning": true,
      "thinkingLevelMap": { "minimal": null },
      "input": ["text", "image"],
      "contextWindow": 1000000,
      "maxTokens": 128000,
      "cost": { "input": 2, "output": 10, "cacheRead": 0.1, "cacheWrite": 2.5 },
      "compat": { "cacheControlFormat": "anthropic" }
    },
    {
      "id": "vertex/claude-haiku-4-5",
      "name": "Claude Haiku 4.5 (Vertex via Bifrost)",
      "reasoning": true,
      "thinkingLevelMap": { "minimal": null },
      "input": ["text", "image"],
      "contextWindow": 200000,
      "maxTokens": 64000,
      "cost": { "input": 1, "output": 5, "cacheRead": 0.1, "cacheWrite": 1.25 },
      "compat": { "cacheControlFormat": "anthropic" }
    }
  ]
}
```

Where each number comes from:
- `contextWindow` and `maxTokens`: the models.dev catalog cached by round 1's package at `~/.cache/pi-bifrost-provider/models-dev.json` (read, `google-vertex-anthropic/models/claude-opus-5-5@default`: context 1000000, output 128000; sonnet the same; haiku `claude-haiku-4-5@20251001`: 200000 / 64000), matching round 1's persisted catalog. Not confirmed against the gateway, which reports no limits (round 1): `inferred`.
- `cost` (USD per million tokens, pi's unit): the same models.dev entries for the Vertex Anthropic models: Opus input 4, output 20, cache_read 0.2, cache_write 5; Sonnet 2, 10, 0.1, 2.5; Haiku 1, 5, 0.1, 1.25. These are list prices of Vertex per that catalog, not Mission Lane's contract price and not from Bifrost (Bifrost reports none): `inferred`. With them pi's `usage.cost.total` is non-zero (verified: one Sonnet turn `"cost":{"input":8e-06,"output":0.00031,"cacheRead":0.0013704,"cacheWrite":0.00013,"total":0.0018184}`).
- `input: ["text","image"]`: models.dev lists text, image, pdf for all three; pi has no `pdf` modality: `inferred`; images were not sent (`not done: no image test`).
- `thinkingLevelMap`, `reasoning`, `cacheControlFormat`, the header: C2, C3, flag 1 (verified).
- Not set on purpose: `compat.sendSessionAffinityHeaders` (no effect, case b), `supportsLongCacheRetention` (completions route does not use it), `promptCache` (pi's cache-warming lifetimes, `not done: not tested`).

What changes per machine: only `baseUrl`. On pro `http://127.0.0.1:8788/openai/v1`; on the LAN `https://192.168.3.100:8787/openai/v1` plus `NODE_EXTRA_CA_CERTS=<Caddy root.crt>` in the process environment (D2; the TLS leg was not re-tested here, `not done: round 1 verified it, this Job used pro's loopback`). The key comes from `$BIFROST_VIRTUAL_KEY` on every machine.

(a) The shipped catalog module on exactly this block (verified):

```
cd /tmp/sp2-a1/ag-f; PI_CODING_AGENT_DIR=$PWD PI_MANAGED_INSTALL_ROOT=$PWD/install node --input-type=module -e "$(cat core/src/config/catalog.mjs)" -- <pi-coding-agent>/dist/index.js <pi-ai>/dist/index.js O=bifrost/vertex/claude-opus-5-5 S=bifrost/vertex/claude-sonnet-5-5 H=bifrost/vertex/claude-haiku-4-5 G=bifrost/vertex/gemini-3.8-flash
{"O":["off","low","medium","high","xhigh","max"],"S":["off","low","medium","high"],"H":["off","low","medium","high"],"G":null}
```

So O has efforts 0..5, S and H 0..3, as the Tier ranges need; `G` is `null` because the block has no gemini entry (another Job). No extension, no network, the static provider is enough (D12 confirmed for these three).

(b) The Worker-shaped run on this block: C4 above (9 of 9 ok), plus the four-turn cache run on it (99.8, 99.8, 99.6 percent).

Alternative, Anthropic route (verified in the sweeps and cases with this shape, not run through the shipped catalog module: `not done`): provider `"bifrost-ant": {"baseUrl":"http://127.0.0.1:8788/anthropic","api":"anthropic-messages","apiKey":"$BIFROST_VIRTUAL_KEY","headers":{"x-bf-mcp-include-clients":"none"}, "models":[...]}`, Opus and Sonnet with `"compat":{"forceAdaptiveThinking":true}`, the same `thinkingLevelMap`s, no `cacheControlFormat` (the markers are sent by default).

verdict: the block above is the one; it yields the Tier efforts O 0..5, S and H 0..3 through the shipped module with no network, and caches at 99.6 percent or better.

## C6. Risks and unknowns

1. **Gateway MCP injection** (flag 1, verified). Without `x-bf-mcp-include-clients: none` every request carries 2.3k to 14k hidden tokens, up to several server-side tool rounds, and Claude may call tools pi cannot answer (`Tool executeToolCode not found`). The header is the fix; if a future gateway change renames it the failure is silent (more tokens, slower, odd tool calls). A cpd check could assert `usage.input` stays small. Whether a Worker legitimately wants any Bifrost MCP tool: `not done: out of scope`.
2. **Opus hangs on the OpenAI route** (verified, not explained). Events with no reply: three curl calls of 55 s (`reasoning_effort` minimal, low, medium, without the header), one `pi` run of 150 s (`:high`, run 2 of the second sweep) and one of 150 s (`:max`, first sweep); two more were slow (Opus `:medium` 54.9 s, Sonnet `:off` 38.4 s). Twenty-two targeted follow-ups (eight tiny requests per route without effort, six with `low/medium/high` on the OpenAI route) all answered in under 1.4 s, and the Anthropic route had no hang in about 30 Opus requests. A pi Worker has no reply timeout shorter than its own, so a hang costs the run: `not done: cause (gateway, Vertex rate limit, or Opus load) unknown`. Decision for the User: accept the retry risk on the OpenAI route, or take the Anthropic route for Opus.
3. **`off` does not switch reasoning off** for Opus and Sonnet on either route (C2): the request carries no thinking or `thinking:disabled`, the model reasons in its visible text, the cost does not fall. Do not sell `off` as the cheap mode.
4. **`low` on Sonnet often means no thinking block** (4 of 8 runs) and the easy-task differences between `low`, `medium` and `high` are within noise; real separation needs a hard task. The cpd efforts 0..5 map to real behaviour only at the ends.
5. **Reasoning tokens in `usage.reasoning` are not thinking tokens** (verified): they track output tokens minus about 7 whether or not thinking happened. The distiller's cost reads `output`; cost is correct once `cost` is declared (flag 6), and `reasoning` should not be used to measure effort.
6. **One cache miss in 30 turns from turn 2 on** (`ant/b/sonnet` turn 2; C3), after which the cache hit again. Cache lifetime (5 min or 1 h) not measured.
7. **Prices are list prices from models.dev**, not the company's contract; Mission Lane's actual billing is not visible here.
8. **Minimum cacheable prefix** differs from Anthropic's documented numbers for Opus and Sonnet (cached at 728 tokens, C3), and for Haiku is above 653 tokens: nothing in the contract should assume a number.
9. **The two cases the User should decide**: (i) Anthropic route versus OpenAI route (equal on cache and effort, the Anthropic one has no hangs seen, the OpenAI one shares a provider with gemini and with round 1's contract); (ii) whether the `x-bf-mcp-include-clients: none` header belongs in the `models.json` the dotfiles render (it must, in my reading) or in cpd.
10. **Key exposure** (round 1, unchanged): `apiKey: "$BIFROST_VIRTUAL_KEY"` makes pi read the variable and a Worker's bash tool inherits it.
11. **Not done**: images and PDFs (`input`), the LAN TLS leg for this block, Gemini (another Job), cache TTL, an Anthropic-route run through the shipped catalog module, any Bifrost source or documentation.

verdict: the three Claude models are usable on Bifrost with one static provider block; the open risks are the unexplained Opus hangs on the OpenAI route and the hidden MCP injection that the header removes.
