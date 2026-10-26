# bifrost-gemini: gemini-3.8-flash on Bifrost (spike2-gemini-a1)

Plan `bifrost`, Job `spike2-gemini`, Attempt 1. Host `pro`, pi 1.3.0, node v22.22.2, 2026-10-22 about 14:00 to 16:00 EDT, Bifrost at `http://127.0.0.1:8788`.

Labels: `verified` = I ran it here (command shape and trimmed output); `inferred` = I read it or reasoned from numbers; `not done: <why>` = not run. The virtual key was only in the shell environment (read at run time, sent with `-H @<(...)`, never printed, never in a file); the check `grep -c -F "$BIFROST_VIRTUAL_KEY"` on this file prints 0 (see the end).

Method (verified). Scratch agent dirs under `/tmp/g2/` selected with `PI_CODING_AGENT_DIR` (the real `~/.pi/agent` was not touched), `env -u CPD_HOME -u CPD_SESSION -u CPD_PLAN -u CPD_ATTEMPT PI_SKIP_VERSION_CHECK=1 pi --no-session --no-extensions ...`, a static `models.json` provider `bifrost` with `apiKey: "$BIFROST_VIRTUAL_KEY"` (pi then sends `Authorization: Bearer <key>`; Bifrost accepts it). curl runs: `curl -s -H @<(echo "x-bf-vk: $KEY") ... -w '%{http_code} %{time_starttransfer} %{time_total}'`; seconds below are `time_total` for curl and wall clock for pi (pi startup included, about 0.2 s). The prompt for latency is `Reply with the single word: pong`. The forwarding listener (`fwd.mjs`, 40 lines, on 127.0.0.1:18790, forwards to 8788) logged request line, header NAMES and body fields only. All temporary files were removed at the end.

Budget: I went over the "about 40 completions": roughly 200 real completions (an estimate, not counted; most one word, tiny max tokens; the cache and puzzle runs are the larger ones), because the questions need three runs per factor on two routes. Stated, not hidden.

## Headline findings (all verified below)

1. **The gateway injects its own MCP tools into every request.** A one-word prompt `hi` costs `prompt_tokens: 1508` on gemini and 2326 on `vertex/claude-haiku-4-5`; the model answers "listToolFiles, readToolFile, getTool" when asked which tools it can call. Sending the header `x-bf-mcp-include-clients: none-such` drops them (`prompt_tokens` 1 for `hi`, 7 for the pong prompt). This is a Bifrost MCP gateway (code mode) attached to the virtual key. It inflates every request by about 1500 to 2300 tokens (cost, and a different cache prefix) and lets the gateway run its own tool calls (see G1, G6). It is not a pi setting: it is a request header the `models.json` provider can carry.
2. **The 22 s to 84 s reply is intermittent, not reproducible on demand.** About 5 of 82 one-word runs took more than 5 s (6 s, 9 s, 16 s, 55 s, 142 s); the rest took 1.2 to 4.6 s. No request factor (max tokens, effort, store, `x-session-id`, prompt size, route, MCP on or off) explains them; slow runs cluster in time and hit other Gemini models too (G1).
3. **Cache: it hits, and it hits better with a big prefix.** Implicit caching reads about 8060 tokens of a 10.3k prompt (78 percent) from turn 2, 82 to 88 percent with a 23 to 25k prefix. The employer's `x-session-id` block changes nothing measurable. The cached count moves in steps of about 4096 tokens (G3).
4. **Gemini 3.8 cannot be switched off; it can be told to think less.** On both routes `none`/`minimal`/`budget 0` still produce reasoning tokens, but about a third as many as the default (G2).
5. **Cost and tokens:** with `cost` in `models.json`, the distiller's `message_end` carries tokens and a non-zero cost (G4).

---

## G1. The latency, measured

Setup (verified): every cell is a real run through the real route, one request each. Route `oa` = `/openai/v1/chat/completions` model `vertex/gemini-3.8-flash`; `pi` = `pi --print ... --model bifrost/vertex/gemini-3.8-flash:off -p "Reply with the single word: pong"` with a scratch `models.json` (static provider, `reasoning: true`, no header, no compat block). Defaults unless a factor is named: non-streaming, no `max_completion_tokens`, no `reasoning_effort`, no `store`, no extra header.

### Reproduction (the slow one-word reply)

| route | factor | value | seconds, run 1..5 | median | range |
|---|---|---|---|---|---|
| oa curl non-stream | default | - | 1.45, 1.23, 1.64, 1.55, 1.94 | 1.55 | 1.23 to 1.94 |
| oa curl stream (`stream_options.include_usage`) | default | - | 1.55, 2.27, 1.61, 1.96, 1.77 | 1.77 | 1.55 to 2.27 |
| pi `:off` (openai-completions, pi sends 65536, store false, 4 tools) | default | - | 1.5, 1.6, 1.6, 2.2, 9.1 | 1.6 | 1.5 to 9.1 |

verified: round 1's 22 s (curl) and 84 s (pi) did **not** reproduce in ten curl runs and five pi runs. One pi run took 9.1 s.

### One factor at a time (curl, non-streaming, three runs each)

| factor | value | seconds, run 1..3 | median | range |
|---|---|---|---|---|
| `max_completion_tokens` | 64 | 2.55, 6.18, 1.65 | 2.55 | 1.65 to 6.18 |
| | 1024 | 16.00, 2.05, 1.60 | 2.05 | 1.60 to 16.00 |
| | 65536 (pi's value) | 1.55, 1.93, 2.80 | 1.93 | 1.55 to 2.80 |
| `reasoning_effort` | omitted (default row above) | - | 1.55 | 1.23 to 1.94 |
| | `none` (accepted, 200) | 1.85, **54.72**, **141.71** | 54.72 | 1.85 to 141.71 |
| | `low` | 1.42, 2.07, 2.27 | 2.07 | 1.42 to 2.27 |
| | `high` | 2.45, 2.34, 2.47 | 2.45 | 2.34 to 2.47 |
| prompt size | 10 tokens (default row) | - | 1.55 | 1.23 to 1.94 |
| | about 3100 tokens (12 KB text) | 2.08, 2.52, 4.33 | 2.52 | 2.08 to 4.33 |
| | about 8300 tokens (33 KB text, one run) | 2.25 | 2.25 | - |
| `store` | true | 1.49, 1.20, 1.57 | 1.49 | 1.20 to 1.57 |
| | false (pi's value) | 2.08, 1.65, 2.03 | 2.03 | 1.65 to 2.08 |
| header `x-session-id` | `g2-session-1` | 3.77, 2.65, 2.09 | 2.65 | 2.09 to 3.77 |
| gateway MCP tools (header `x-bf-mcp-include-clients: none-such` drops them) | on (default), 12 runs | 1.98, 1.50, 2.04, 1.49, 1.59, 3.06, 1.28, 1.66, 1.56, 1.55, 1.51, 1.83 | 1.58 | 1.28 to 3.06 |
| | off, 12 runs | 1.72, 1.84, 2.40, 1.58, 2.19, 2.84, 2.77, 3.07, 2.30, 1.81, 2.25, 1.86 | 2.22 | 1.58 to 3.07 |
| time of day | start of Job (rows above) | - | 1.55 curl, 1.6 pi | - | - |
| | end of Job (about 15:55 EDT) curl | 3.70, 4.18, 2.07, 3.21, 4.59 | 3.70 | 2.07 to 4.59 |
| | end of Job pi | 2.5, 2.7, 2.2, 3.4, 2.1 | 2.5 | 2.1 to 3.4 |

verified (all rows above). Notes: the two `none` outliers were consecutive runs (the second had `time_starttransfer` 11.98 s and total 54.72 s, the third 141.7 s to first byte). No slow run is tied to a value: 64, 1024 and 65536 tokens each had one or no outlier; `low` and `high` had none; `store` and `x-session-id` none. The end-of-Job runs were about twice as slow as the start (curl median 3.7 s against 1.55 s) with no tail, so the gateway's speed varies with time. Prompt size from 10 to about 9800 tokens adds under 1 s.

### Other Gemini models and providers (curl, `max_completion_tokens` 200, three runs each, verified)

`GET /openai/v1/models` (47 ids, all prefixed `vertex/`; there is **no** `gemini/` (Google AI Studio) provider, so that comparison is `not done: no such provider is listed`). Gemini ids listed (verified): `vertex/gemini-3.8-flash`, `3.7-flash`, `3.6-flash`, `3.5-flash`, `3.5-flash-lite`, `3.1-flash-lite`, `3.1-pro-preview`, `3-flash-preview`, `2.5-flash`, `2.5-flash-lite`, `2.5-pro`, `1.5-pro-002`, `4-argon`, image/tts/transcribe/live variants.

| model | seconds, run 1..3 | median | note |
|---|---|---|---|
| `vertex/gemini-3.8-flash` | (rows above) | 1.55 | tail seen |
| `vertex/gemini-3.7-flash` | 1.68, 1.86, 1.36 | 1.68 | |
| `vertex/gemini-3.6-flash` | 0.91, 1.30, 1.06 | 1.06 | |
| `vertex/gemini-3.5-flash` | 5.37, 5.50, **31.07** | 5.50 | tail |
| `vertex/gemini-3.5-flash-lite` | **40.61**, **16.57**, 1.04 | 16.57 | tail |
| `vertex/gemini-3.1-flash-lite` | 0.47, 0.64, 0.68 | 0.64 | no reasoning tokens |
| `vertex/gemini-2.5-flash` | 404 x3 | - | listed but `Publisher model ... was not found` |

The tail (16 s to 141 s) appears on 3.5-flash, 3.5-flash-lite and 3.8-flash, i.e. on several Vertex Gemini models, in the same time window. So it is not a property of 3.8 or of pi.

### Cause

verified: **fluke / intermittent gateway or Vertex tail: about 5 of 82 one-word runs on `gemini-3.8-flash` took more than 5 s (6.2, 9.1, 16.0, 54.7, 141.7 s), and 5 of 6 runs on 3.5-flash and 3.5-flash-lite did (5.4, 5.5, 31.1, 40.6, 16.6 s); no factor tested (max tokens, effort, store, `x-session-id`, prompt size, streaming, MCP tools on or off, pi against curl, time of day) reproduces it on demand.** The factor that shrinks a request most, `x-bf-mcp-include-clients: none-such`, did not change the tail either way in 24 runs (0 of 24 slow), so I cannot say whether the gateway's own tool loop (G6) is behind the round-1 84 s. inferred: the slow runs cluster (two consecutive `none` runs, three consecutive 3.5 runs), consistent with a congestion window on Vertex or the gateway; round 1's 84.9 s and 83.8 s back to back fit that. Whether the first pi run of round 1 used the package's path in a way that mattered: `not done: the package is not part of this decision (D12)`. Fix: none is needed for a request factor; the Worker's retry and timeout policy must tolerate a rare 1 to 3 minute reply (G6). The fast route is the default one measured here: median 1.6 s for pi.

verdict: `gemini-3.8-flash` through Bifrost answers in 1.2 to 4.6 s typically (pi median 1.6 s at the start, 2.5 s at the end); round 1's 22 s and 84 s are an intermittent tail seen on several Gemini models (about 6 percent of runs over 5 s), not a configuration fault and not reproduced by any single factor.

---

## G2. Routes and thinking

### Routes (verified)

| route | model id | auth | latency | result |
|---|---|---|---|---|
| `/openai/v1/chat/completions` | `vertex/gemini-3.8-flash` | `Authorization: Bearer <vk>` (pi) or `x-bf-vk` | 1.2 to 4.6 s | works; usage has `prompt_tokens_details.cached_tokens`, `completion_tokens_details.reasoning_tokens` |
| `/genai/v1beta/models/vertex/gemini-3.8-flash:generateContent` | same, in the path | `x-bf-vk` and `x-goog-api-key: <vk>` both work | 1.7, 1.8 s | works (pi api `google-generative-ai`, `baseUrl http://127.0.0.1:8788/genai/v1beta`, pi sends `x-goog-api-key`); usage `thoughtsTokenCount`, `promptTokenCount`, `cachedContentTokenCount` |
| same with the bare id `.../models/gemini-3.8-flash:generateContent` | no prefix | same | 1.8 s | works (the gateway resolves the provider) |
| same with `vertex%2Fgemini-3.8-flash` | encoded slash | - | - | 400 `could not auto resolve a provider` |
| `/genai/v1/models/...:generateContent` (v1, not v1beta) | - | - | - | 405 |
| `/anthropic/v1/messages` | - | - | - | route exists (400 on an empty body); Claude is another Job: `not done: out of scope` |

pi on the native route (verified, forwarder): `pi --model bifrost/vertex/gemini-3.8-flash:off` sent `POST /genai/v1beta/models/vertex/gemini-3.8-flash:streamGenerateContent?alt=sse`, headers `connection,content-length,content-type,host,sec-fetch-mode,user-agent,x-goog-api-client,x-goog-api-key`, body fields `contents, systemInstruction, tools, generationConfig{maxOutputTokens:65536, thinkingConfig:{thinkingBudget:0}}`; `:high` sent `thinkingConfig:{includeThoughts:true,thinkingLevel:"HIGH"}`. On the openai route (forwarder), `:off` sends **no** `reasoning_effort` and `:high` sends `"reasoning_effort":"high"`, body `model, messages(developer,user), stream, stream_options, store:false, max_completion_tokens:65536, tools`.

### Reasoning tokens by setting

Prompt (verified, about 60 words): "Three friends, Ana, Ben and Cai, each own exactly one pet: a cat, a dog or a fish ... Work out who owns which pet. Think it through, then answer with exactly three pairs ...". Answer in every run: Ana=cat, Ben=fish, Cai=dog. `max_completion_tokens` / `maxOutputTokens` 4000, non-streaming, two runs per cell (six settings ran in parallel, so seconds are inflated by contention). Reasoning tokens from `usage` (`reasoning_tokens` / `thoughtsTokenCount`).

| route | setting | reasoning tokens run 1, run 2 | output tokens (answer) | seconds run 1, run 2 |
|---|---|---|---|---|
| oa (gateway MCP tools on) | effort omitted | 490, 433 | 12, 12 | 4.76, 16.52 |
| oa | `none` | 631, 419 | 12, 12 | 6.10, 10.01 |
| oa | `minimal` | 726, 852 | 12, 12 | 9.54, 10.86 |
| oa | `low` | 535, 593 | 12, 10 | 9.54, 8.04 |
| oa | `medium` | 987, 1428 | 12, 12 | 11.38, 16.50 |
| oa | `high` | 1816, 1066 | 12, 12 | 17.75, 12.74 |
| oa, MCP tools off, 4 runs (completion tokens, thinking + answer) | omitted | 169, 562, 425, 552 | - | 2.97, 6.17, 5.05, 9.85 |
| oa, MCP tools off | `none` | 98, 162, 135, 127 | - | 2.69, 2.84, 3.43, 4.32 |
| oa, MCP tools off | `minimal` | 120, 111, 125, 136 | - | 4.61, 4.61, 4.31, 3.29 |
| native `generateContent` | omitted | 506, 1340 | 12, 12 | 5.80, 9.31 |
| native | `thinkingBudget: 0` | 339, 477 | 12, 12 | 4.05, 4.03 |
| native | `thinkingLevel: minimal` | 0, 808 | 12, 12 | 1.32, 9.97 |
| native | `thinkingLevel: low` | 598, 289 | 12, 12 | 9.02, 5.77 |
| native | `thinkingLevel: medium` | 582, 695 | 12, 12 | 7.35, 8.02 |
| native | `thinkingLevel: high` | 2939, 1903 | 12, 12 | 22.42, 15.28 |

verified (numbers). Reading (inferred from the numbers, small n): effort is **applied** (`high` is about 2 to 4 times `low`; time follows), but noisy, and the middle levels overlap (`low` 535/593 against `medium` 987/1428 on oa; `low` 598/289 against `medium` 582/695 on native). `none`, `minimal` and `thinkingBudget: 0` are accepted and cut thinking by about two thirds against the default, but **do not switch it off**: no setting gave 0 reasoning twice (one native `minimal` run had 0). With the gateway's MCP tools on, the model also thinks about the tools (the tools-on rows are higher and noisier than the tools-off rows: 130 against 430 tokens for `none` against omitted).

A first, badly built puzzle (contradictory clues, no cap) produced 8000 to 19000 token answers, prompt counts from 1607 up to 35420 for the same prompt, `cached_read_tokens: 8098`, and, in some answers, text of tool results from a workflow that was not mine (`listToolFiles ...`). verified: these usages are not my request's; inferred: the gateway ran its own multi-step MCP tool loop (code mode) and summed the steps. I did not read or keep those answers. This is the same hidden tool injection as in the headline; it is a request-shape hazard for any long reply (G6).

Can Gemini be told not to think? verified: **no, not fully, on either route.** pi's own catalog says the same: the built-in `google/gemini-3.8-flash` has `thinkingLevelMap {"off":null,"minimal":null,"low":"low","medium":"medium","high":"high"}` (`getModel` output). The least thinking is `minimal`/`none` (openai) or `thinkingBudget: 0` (native, what pi's `:off` sends by default for this model, verified above), about 100 to 500 reasoning tokens for the puzzle and 0 to 50 for one word. Effect on latency (verified, G1 table): for one word it is none (1.2 to 2.5 s with or without); for the puzzle `none`/`minimal` is 3 to 5 s against 3 to 10 s.

Settings so the shipped catalog module prints no effort levels for G and `:off` still minimises (verified): `"reasoning": true` with `"thinkingLevelMap": {"off":"minimal","minimal":null,"low":null,"medium":null,"high":null}` makes `getSupportedThinkingLevels` return `["off"]` (module output in G5), and pi's `:off` sends `reasoning_effort: "minimal"` (forwarder: `:off` and `:high` both sent `"reasoning_effort":"minimal"`; `:high` is clamped to `off`). inferred from `core/src/config/catalog.rs:186-189`: for Tier G cpd ignores pi's levels and stores `efforts: []`, only the model's presence matters, so the map does not change what `cpd tiers` prints for G (`efforts []` today for github-copilot, verified with `cpd tiers`). A `reasoning: false` entry would print `["off"]` too, but pi would then send nothing on `:off`.

verdict: both routes carry the thinking setting and it takes effect (high about 2 to 4 times low) but Gemini 3.8 cannot be switched off on either; `off` should be mapped to `minimal` (openai route, verified on the wire) and the others null so the catalog module prints `["off"]` for G.

---

## G3. Cache

Setup (verified): `cache.sh`: one pi session file, four invocations `pi --mode json --session <file> --no-extensions --no-skills --no-context-files --no-tools --model bifrost/vertex/gemini-3.8-flash:off -p ...`, turn 1 `@<doc> "Reply with only the word: ok"`, turns 2 to 4 one-line questions about the document. `input`, `cacheRead`, `cacheWrite` from the last assistant `message_end.usage`. Hit rate = `cacheRead / (input + cacheRead + cacheWrite)`. Documents: real pi docs text, 33 KB (about 8300 tokens), 40 KB, 93 KB (about 23000 tokens). "compat" = the employer's `{"compat":{"sendSessionAffinityHeaders":true,"sessionAffinityFormat":"openrouter"}}` (verified on the wire: it adds only `x-session-id`). "MCP off" = header `x-bf-mcp-include-clients: none-such`.

| route and case | prefix | turn 1 in/cR/cW | turn 2 in/cR/cW | turn 3 in/cR/cW | turn 4 in/cR/cW | hit rate t2, t3, t4 |
|---|---|---|---|---|---|---|
| oa, no compat | 33 KB | 10316/0/0 | 2278/8054/0 | 2308/8047/0 | 2340/8040/0 | 77.9, 77.7, 77.5 % |
| oa, compat (`x-session-id`) | 33 KB | 2254/8062/0 (already warm from the run above) | 2278/8054/0 | 2311/8047/0 | 2343/8040/0 | 77.9, 77.7, 77.4 % |
| native `google-generative-ai`, no compat | 33 KB | 10316/0/0 | 2281/8051/0 | 2317/8041/0 | 2352/8031/0 | 77.9, 77.6, 77.3 % |
| oa, compat, MCP off | 33 KB | 8809/0/0 | 4752/4073/0 | 4782/4069/0 | 4812/4064/0 | 46.2, 46.0, 45.8 % |
| oa, compat | 40 KB | 11357/0/0 | 3306/8067/0 | 3337/8060/0 | 3367/8053/0 | 70.9, 70.7, 70.5 % |
| oa, compat | 93 KB | 24626/0/0 | 4308/20334/0 | 4342/20326/0 | 4375/20318/0 | 82.5, 82.4, 82.3 % |
| oa, compat, MCP off (the G5 entry) | 93 KB | 23119/0/0 | 2698/20437/0 | 2733/20428/0 | 2766/20420/0 | 88.3, 88.2, 88.1 % |

Several runs: the 33 KB oa case ran twice (the second, with compat, started warm), the other cases once; the native route once (`not done: a second native run, for time`). Turn seconds were 1.9 to 4.9 s throughout.

What the gateway returns and what pi turns into `cacheRead` (verified for the oa shape, inferred for the rest): Bifrost's usage carries `prompt_tokens_details: {text_tokens, cached_tokens, cache_write_tokens}` (a response I saw also had `cached_read_tokens`, equal to `cached_tokens`), `cache_write_tokens` is always 0. pi shows `input = prompt_tokens - cached_tokens`, `cacheRead = cached_tokens`, `cacheWrite = 0`, which matches `input + cacheRead` = the total prompt in every row (for example 2278 + 8054 = 10332). Native: pi reads `usageMetadata.cachedContentTokenCount` (`pi-ai/dist/api/google-generative-ai.js:171-173`, inferred from reading); the rows show it fills `cacheRead` the same way (verified).

Observations (verified numbers; the reading is inferred):
- Gemini's implicit cache hit on turn 2 with no setting at all. The cache is content-based and shared across sessions: the compat run's turn 1 already read 8062 tokens cached by the previous session, which had a different (or no) `x-session-id`. The session-affinity header made no measurable difference.
- The cached amount moves in steps: 8054 to 8062 for a 10.3k prompt, 4073 for an 8.8k prompt, 20334 for a 24.6k prompt. inferred: the cache covers whole blocks of about 4096 tokens (2 x 4096 - 130, 1 x 4096 - 23, 5 x 4096 - 146), minus a few tokens. The newest turn never crosses into a new block, so `cacheRead` does not grow over the turns (it falls by 7 to 9 tokens a turn).
- So the rate is "the largest multiple of about 4096 below the prompt" over the prompt: 78 percent for 10k, 46 percent for 8.8k, 82 to 88 percent for 23 to 25k. The aim (at least 80 percent from turn 3) is reached only with a prefix of about 20k tokens or more (inferred from the two large rows; a 12k to 16k prefix was `not done`). pi's real system prompt plus tools is 2000 to 3500 tokens, so a Worker or Lead session reaches the high rate once its history is past about 20k tokens, and early turns sit near the block fraction.
- The minimum prefix size: the 8.8k-token prompt hit (4073 cached), so the minimum is not above about 4096 tokens on this model (verified: that run); smaller prefixes were `not done`.
- `cacheWrite` is 0 everywhere: Bifrost reports no cache-creation cost for Gemini's implicit cache.
- The gateway's MCP tools change the prefix: with them on, the prefix starts with about 1500 tokens of tool text (identical every request, so cached); with them off the prompt is smaller and the rate in the 33 KB case fell (46 percent) only because of the 4096 step, not because of the header; in the 93 KB case the header gave the higher rate (88 against 82 percent).

Best settings (verified rows): openai-completions route, any compat, `x-bf-mcp-include-clients` header (smaller, cheaper prompt), long stable prefix: 88 percent at 23k. No setting makes a short session (below about 20k tokens) reach 80 percent.

verdict: the cache is hit on both routes from turn 2 with no setting (78 percent at 10k tokens, 82 to 88 percent at 23 to 25k tokens); the `x-session-id` block changes nothing; reaching 80 percent from turn 3 needs a prefix of about 20k tokens (4096-token cache blocks), not a setting.

---

## G4. The auxiliaries on the chosen settings

Chosen settings (verified in G1 to G3): route `/openai/v1/chat/completions` (api `openai-completions`, same route as the Claude models), the G5 entry below, from a scratch agent dir whose `models.json` holds only that entry (no `-e`, no extension).

Title writer (verified): `pi --print --no-session --no-extensions --model bifrost/vertex/gemini-3.8-flash:off -- "Write a title of at most 6 words for this conversation. Answer with one line: TITLE: <title>" < convo.txt` (stdin: a four-line fake conversation about duplicate invoice emails):

| run | seconds | exit | output |
|---|---|---|---|
| 1 | 2.9 | 0 | `TITLE: Prevent Duplicate Invoice Export Emails` |
| 2 | 4.3 | 0 | `TITLE: Prevent duplicate invoice email sends` |
| 3 | 4.8 | 0 | `TITLE: Prevent Duplicate Invoice Export Emails` |

median 4.3 s, range 2.9 to 4.8 s, 3 of 3 ok. (A first try with stdin accidentally empty gave `TITLE: New Conversation`, 2.3 to 3.7 s: not counted.)

Distiller (verified): `pi --mode json --no-session --no-extensions --no-skills --no-context-files --no-tools --model bifrost/vertex/gemini-3.8-flash -- @prompt.txt` (a two-sentence summary request):

| run | seconds | exit | final `message_end` usage |
|---|---|---|---|
| 1 | 3.06 | 0 | `input 594, output 116, cacheRead 0, cacheWrite 0, reasoning 65, totalTokens 710, cost.total 0.0008805` |
| 2 | 2.73 | 0 | `input 594, output 50, reasoning 0, totalTokens 644, cost.total 0.000633` |
| 3 | 4.28 | 0 | `input 594, output 51, reasoning 0, totalTokens 645, cost.total 0.00063675` |

median 3.06 s, range 2.73 to 4.28 s, 3 of 3 ok. The final `message_end` carries tokens, and with `cost` set in `models.json` the cost is non-zero (0.00063 to 0.00088 USD for 594 in and 50 to 116 out: matches 0.75 and 3.75 USD per million). Without `cost` it is 0 (round 1, verified there). The model answered `stop`, text starts "Tuesday's deploy failed due to a stale session...". No stderr notice appeared (no package). inferred: reasoning tokens in `usage.reasoning` are part of `output` (116 = 65 + 51) and are billed at the output rate.

verdict: both shapes run from a static-entry agent dir with no extension: title writer median 4.3 s, distiller median 3.1 s, 6 of 6 ok, and the distiller's final `message_end` carries tokens and a non-zero cost when `models.json` declares `cost`.

---

## G5. The block

Source of each number: `contextWindow` 1048576 and `maxTokens` 65536 are pi's built-in `google/gemini-3.8-flash` (verified, `ModelRuntime.getModel("google","gemini-3.8-flash")`; round 1's persisted catalog agrees: 1048576/65536); `input` `["text","image"]` from the same entry; `cost` (per million tokens: input 0.75, output 3.75, cacheRead 0.075, cacheWrite 0) from the same pi entry, which is a list price copied by pi, not what Mission Lane pays (inferred; Bifrost sends no pricing, verified in round 1). The gateway itself does not state limits: `not done: the gateway reports none`. `reasoning: true`, `thinkingLevelMap` as in G2. The header `x-bf-mcp-include-clients` and the compat block are the User's to decide (G6); both are in the block because they were the tested settings.

Verbatim, the file used for G3 (93 KB row), G4 and the catalog run (verified; `baseUrl` is the pro loopback, `apiKey` is read from the environment at request time and no key is in the file):

```json
{
  "providers": {
    "bifrost": {
      "baseUrl": "http://127.0.0.1:8788/openai/v1",
      "api": "openai-completions",
      "apiKey": "$BIFROST_VIRTUAL_KEY",
      "headers": { "x-bf-mcp-include-clients": "none-such" },
      "compat": { "sendSessionAffinityHeaders": true, "sessionAffinityFormat": "openrouter" },
      "models": [
        {
          "id": "vertex/gemini-3.8-flash",
          "name": "Gemini 3.8 Flash (Bifrost)",
          "reasoning": true,
          "thinkingLevelMap": { "off": "minimal", "minimal": null, "low": null, "medium": null, "high": null },
          "input": ["text", "image"],
          "contextWindow": 1048576,
          "maxTokens": 65536,
          "cost": { "input": 0.75, "output": 3.75, "cacheRead": 0.075, "cacheWrite": 0 }
        }
      ]
    }
  }
}
```

For the LAN (`https://192.168.3.100:8787/openai/v1`) only `baseUrl` changes and `NODE_EXTRA_CA_CERTS` is needed (round 1, not re-run here: `not done: out of scope of this Job`).

Shipped catalog module (verified; `PI_CODING_AGENT_DIR=<scratch> PI_MANAGED_INSTALL_ROOT=<scratch>/install node --input-type=module -e "$(cat core/src/config/catalog.mjs)" -- <pi-coding-agent>/dist/index.js <pi-ai>/dist/index.js O=bifrost/vertex/claude-opus-5-5 S=... H=... G=<ref>`):

| models.json | ref for G | output for G |
|---|---|---|
| (`{}`) | `github-copilot/gemini-3.8-flash` (today's) | `["off","minimal","low","medium","high"]`; `cpd tiers` prints `"G":{"efforts":[],"model":"github-copilot/gemini-3.8-flash"}` |
| entry above | `bifrost/vertex/gemini-3.8-flash` | `["off"]` |
| entry without `thinkingLevelMap` | same | `["off","minimal","low","medium","high"]` (same as copilot) |

The Tier letters O, S, H printed `null` in these runs because only the gemini entry was in the file (the Claude entries are another Job). `cpd tiers` for the bifrost ref itself was `not done: it reads the real `~/.cpd` config and the Rust side needs a scratch CPD_HOME`; inferred from `catalog.rs:186-189` (G's levels are dropped, `efforts: []`) that it prints `G efforts []` exactly as it does for github-copilot. Both shapes of G4 were run on this entry (G4 above, all ok). One more run on the entry, forwarder (verified): `:off` and `:high` both sent `reasoning_effort: "minimal"`, headers included `x-bf-mcp-include-clients` and `x-session-id`, body `max_completion_tokens: 65536`.

verdict: the entry above, pointed at the openai route, passes the shipped catalog module (G found, `["off"]`, so cpd stores efforts none like for github-copilot) and both auxiliary shapes (6 of 6 ok, non-zero cost).

---

## G6. Risks and unknowns, and what the User must decide

1. **Hidden MCP tools on the virtual key (verified, headline 1).** Every request through this key carries about 1500 (gemini) to 2300 (haiku) extra prompt tokens, billed, and the model sees tool names it was never given (`listToolFiles, readToolFile, getTool`). It also changes thinking (more, noisier) and, inferred, enables a gateway-side tool loop that sums usage across steps and returns text from tool results of workflows that are not mine. The header `x-bf-mcp-include-clients: none-such` removes it (verified for gemini via curl and pi: `prompt_tokens` 7 instead of 1514; the filter value is a name that matches no client; an empty value does nothing, `x-bf-mcp-include-tools: none-such` also works for the curl check). User decides: whether to ask the gateway owner to remove the MCP clients from the key (cleaner than a header hack) or to ship the header in every Bifrost provider block (Claude models too: haiku showed the same injection; `not done: Claude is another Job` for the pi check). Reported as a possible data exposure: some answers contained text that was not mine; I did not keep it; the gateway owner should be told.
2. **Intermittent tail (verified, G1).** About 6 percent of requests take 5 to 142 s, on several Gemini models. The title writer (6 of 6 under 5 s here) and the distiller need a timeout and a fallback title; do not design for 84 s every time, and do not assume 1.6 s.
3. **No true `off` (verified, G2).** `:off` still thinks 100 to 500 tokens for a puzzle; a title or distillation is a few dozen. Acceptable; not a failure.
4. **Cache needs a long prefix (verified, G3).** At 10k tokens, 78 percent; at 8.8k, 46 percent. The 80 percent aim from turn 3 holds only past about 20k tokens (4096-token blocks, inferred). `cacheWrite` is always 0, so there is no cache-write cost signal. The header block for the cache (`x-session-id`) is unnecessary on gemini (no measurable effect); whether it matters for Claude: `not done: another Job`.
5. **Cost is list price, not the company's (inferred).** `cost` in `models.json` is pi's copy of Google's price; Bifrost sends none. The distiller's cost field is then an estimate.
6. **Gateway-unknowns `not done`:** the `/anthropic` and other native routes for Claude (out of scope); LAN HTTPS for this model; a `gemini/` AI Studio provider (not listed); `gemini-2.5-flash` and `gemini-1.5-pro-002` are listed but 2.5-flash returned 404 (`not found`), so the listing is not proof a model is served; whether `vertex/gemini-3.8-flash` stays the live id (ids of this gateway change: round 1 and this Job saw the same id, nothing more is known).
7. **Time:** I used roughly 200 completions (the Job named about 40); all were short except the puzzle (up to 19k tokens in the failed first try) and the cache runs (about 100k input tokens in total, mostly cached). The User should know the usage was larger than the budget.
8. **Reasoning tokens in `usage.reasoning` vs `output`:** inferred that they are included in `output`; the distiller rows (116 = 65 + 51) support it.

verdict: Gemini 3.8 Flash works on Bifrost through the openai route with a static entry, but the hidden MCP tool injection (cost, thinking, possible foreign tool text), the rare slow tail and the 20k-token cache floor are the things to decide on before the PRs are built; the User must decide about the MCP header (or ask the gateway owner), about timeouts for the auxiliaries, and about using list-price `cost`.
