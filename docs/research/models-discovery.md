# models-discovery: listing the models of Bifrost and GitHub Copilot from cpd (spike2-disco-a1)

Plan `bifrost`, Job `spike2-disco`, Attempt 1. Host `pro`, pi 1.3.0 (`~/.pi/agent/install/releases/1.3.0`), node v22.22.2, Bifrost reached at `http://127.0.0.1:8788` (loopback passthrough) and `https://192.168.3.100:8787` (LAN).

Labels: `verified` = I ran it in this Attempt (command and trimmed output given); `inferred` = I read it and did not run it; `not done: <why>` = not answered. No model completion was made (budget: list calls only). The virtual key was only ever in the environment (`. /tmp/sd2-a1/key.sh`, a one-line file that exports it from `~/.hermes/.env`; headers given to curl by `-H @<(printf …)`), no file of the repo holds it. Scratch work lived in `/tmp/sd2-a1` (removed at the end). Copilot was run against the real `~/.pi/agent` twice, with `models-store.json` copied aside first and restored after each run (`shasum -c` OK both times); `auth.json` was never read, printed or copied, and its mtime was the same before and after (`stat -f %m`: 1792696578 both times).

Names used below: the helper = a script run under pi's Node with pi's coding-agent module (the pattern of `pi/lib/slack-channel.ts`), `rt` = `ModelRuntime.create({allowModelNetwork:false})` of pi's `dist/index.js`. My scratch helper `list.mjs` is not repo code.

## Summary of what the questions found

1. **Bifrost** (D1). The helper needs only pi: `rt.models.providers.get("bifrost").baseUrl` (or `rt.getModel("bifrost", <id>).baseUrl`) gives the machine's URL, `rt.getAuth(model)` gives the key pi resolved from `$BIFROST_VIRTUAL_KEY`, and `fetch(<baseUrl>/models)` with `Authorization: Bearer <key>` returns the 47 ids. Five failure kinds are told apart by the error object (table in D1).
2. **Copilot** (D2). pi has **no per-account discovery call of its own to hook**: its `refreshModels` for `github-copilot` (and for 40 other providers) only overlays the pi.dev catalog, and it never removes a model. The list of models the **User's account** offers is `credential.availableModelIds`, which pi stores in the OAuth credential each time it refreshes the Copilot token (`GET <copilot>/models`); the helper reads it without touching `auth.json` through `rt.models.getAvailable("github-copilot")` (11 ids today; 36 are compiled into pi 1.3.0).
3. **The two lists** (D3). The four Tier models exist on both providers; one rule (`strip "<provider>/"`, `.` → `-`, lower case) matches 16 of Copilot's 36 ids to 16 of Bifrost's 47, with no collision and no ambiguity.
4. **Surprises you must know** (D6): a richer Bifrost route `GET /v1/models` carries pricing and `is_deprecated`, and marks **`vertex/claude-haiku-4-5`, the H Tier model, deprecated**; with a Copilot API-key env var instead of the login, pi's "available" list becomes all 36 compiled ids (a false "25 appeared"); contract's Inbox has **no cyan tier** (blue is the "cpd notice" tier); an engine restart would restart a 7-day cron if it were kept in memory as E11 does.

---

## D1. Bifrost: listing the models through pi

### What `rt` gives the helper once a static `models.json` provider exists

Scratch agent dir `/tmp/sd2-a1/agB` with `models.json`: provider `bifrost`, `baseUrl: "http://127.0.0.1:8788/openai/v1"`, `api: "openai-completions"`, `apiKey: "$BIFROST_VIRTUAL_KEY"`, four Tier models (the D12 shape). Command: `. ./key.sh; PI_CODING_AGENT_DIR=$PWD/agB node bf.mjs`:

```
baseUrl http://127.0.0.1:8788/openai/v1 api openai-completions provider bifrost
provider.baseUrl http://127.0.0.1:8788/openai/v1 refreshModels undefined authKeys [ 'apiKey' ]
```

- verified: `rt.getModel("bifrost","vertex/claude-opus-5-5").baseUrl` and `rt.models.providers.get("bifrost").baseUrl` are both the literal `baseUrl` of `models.json`, so the helper learns the machine's URL from pi and cpd parses no `models.json`. The provider form needs no Tier model id: use it.
- verified: `refreshModels` is `undefined` for a `models.json` provider: pi has no discovery for it, and `rt.refresh()` does nothing for `bifrost`. The helper must do the `GET` itself.
- verified: `rt.getAuth(model)` resolves the key: `. ./key.sh; node bf2.mjs` printed `getAuth keys auth,source`, `source configured API key`, `auth keys apiKey,headers`, `apiKey is string string equals env key true len 42`, `header names []` (the value itself never printed). So pi resolves `$BIFROST_VIRTUAL_KEY` and the helper receives it as `res.auth.apiKey`; it adds no header of its own. It must never log it.

### The exact request

verified (`list.mjs`, header names only printed): `request: GET http://127.0.0.1:8788/openai/v1/models header names: Authorization`. That is `GET <baseUrl>/models` with `Authorization: Bearer <apiKey>` (what pi itself sends on a completion). The other form, header `x-bf-vk: <key>`, also works: three runs each, all `HTTP 200 … ids 47` (table below). No body, no query.

### The response

verified (`curl -H @<(printf 'x-bf-vk: %s\n' "$BIFROST_VIRTUAL_KEY") http://127.0.0.1:8788/openai/v1/models`, `node -e` over the JSON): `200 0.366s`; top-level keys `object` (`""`) and `data`; 47 entries; every entry has exactly `id`, `object` (always `"model"`), `owned_by` (always `""`). So **the id is the only stable field** on this route. The 47 ids are printed in D3. Over the LAN (`https://192.168.3.100:8787/openai/v1`, `NODE_EXTRA_CA_CERTS="$HOME/Library/Application Support/Caddy/pki/authorities/local/root.crt"`, the public root certificate on this machine): the same 47 (three runs, `HTTP 200 ids 47`).

### Other routes of the gateway (probed, `-H @<(…)` with the key, status and size only)

| route | result | note |
|---|---|---|
| `/openai/v1/models`, `/openai/models` | 200, 3206 B | `{id, object, owned_by}` |
| **`/v1/models`** | **200, 12624 B** | per entry `id, name, normalized_name, context_length, max_input_tokens, max_output_tokens, pricing{prompt, completion, web_search, image, input_cache_read, input_cache_write}, is_deprecated`; top level also `extra_fields`, `key_statuses` (an object; not printed, it may describe the key). Same 47 ids as the OpenAI route (`every openai id in v1: true`). 42 of 47 have `pricing`, 34 have `context_length`, 4 are `is_deprecated`: `vertex/claude-haiku-4-5`, `vertex/gemini-2.5-flash`, `vertex/gemini-2.5-flash-lite`, `vertex/gemini-2.5-pro`. Sample (opus 5.5): `context_length 1000000`, `max_output_tokens 128000`, `pricing.prompt "0.0000040000"`, `completion "0.0000200000"`, `input_cache_read "0.0000002000"`, `input_cache_write "0.0000050000"` |
| `/anthropic/v1/models` | 200, 5596 B | 47 entries, ids **without** `vertex/` (`claude-fable-5`, `claude-fable-5-1`, …), `type model`, `display_name` |
| `/genai/v1beta/models` | 200, 12603 B | 47 entries named `models/<id without vertex/>`; `supportedGenerationMethods` null for all 47 and `thinking false` even for opus: useless for discovery |
| `/api/models`, `/api/models/details` | 401 `Unauthorized` | the management API wants another credential: not a client route |
| `/health` | 200 `{"components":{"db_pings":"ok"},"status":"ok"}` | no key needed |

Consequences (verified, from the table): round 1's "Bifrost carries no pricing" is true of the OpenAI route only. `GET /v1/models` (the same host, `baseUrl` minus `/openai/v1` plus `/v1/models`: inferred as a rule for a `baseUrl` of that shape; on the LAN the same path was **not** probed: `not done: only the loopback proxy was probed for /v1/models`) holds the prices the distiller's `cost: 0` lacks, the limits, and a deprecation flag. The discovery diff does not need them; D6 asks whether to use them.

### Failure cases (the helper's `list.mjs`, one `fetch`, `AbortSignal.timeout`; seconds are the script's clock from process start, which includes about 0.55 s of loading pi)

Made-up key `sk-bf-invalid` for all but the first row and the success rows; the real key only for the success rows.

| case | how provoked | what the helper sees (verified) | seconds |
|---|---|---|---|
| key unset | `env -u BIFROST_VIRTUAL_KEY` | `rt.getAuth` **throws** `ModelsError`, `code: "auth"`, message `API key auth failed for provider bifrost: Failed to resolve API key for provider "bifrost" from environment variable: BIFROST_VIRTUAL_KEY`; no request is made | 0.61 (process 0.71) |
| 401 | key `sk-bf-invalid`, loopback | `HTTP 401`, `application/json`, body `{"type":"access_not_found","is_bifrost_error":false,"status_code":401,"error":{"message":"access not found. The provided credential does not exist or has been revoked."},…}` | 3.12 (loopback); 0.89, 0.71 (LAN, with the CA) |
| URL unreachable, refused | `baseUrl` `http://127.0.0.1:18999/openai/v1` (closed port) | `TypeError: fetch failed`, `cause.code` **`ECONNREFUSED`**, `connect ECONNREFUSED 127.0.0.1:18999` | 0.63, 0.68, 0.62 |
| (port 9) | `http://127.0.0.1:9/…` | `fetch failed`, cause `Error: bad port`: undici refuses some ports (9 is one) before connecting: do not use them in tests | 0.97, 0.55, 0.61 |
| URL unreachable, silent | `http://10.255.255.1:8788/…`, `AbortSignal.timeout(5000)` | `TimeoutError` (`code 23`), `The operation was aborted due to timeout`, no `cause` | 5.62, 5.73 |
| TLS, no CA | LAN URL, **no** `NODE_EXTRA_CA_CERTS` | `TypeError: fetch failed`, `cause.code` **`UNABLE_TO_GET_ISSUER_CERT_LOCALLY`**, `unable to get local issuer certificate` | 0.65, 0.65 |
| TLS, with CA | same, `NODE_EXTRA_CA_CERTS=<root.crt>` | passes TLS; with `sk-bf-invalid` `HTTP 401`; with the real key `HTTP 200 ids 47` | 0.89, 0.71 / 1.17, 0.94, 1.12 |
| provider not configured | `models.json` with a JSON error (my own typo) | `rt.getModel(...)` is `undefined` and `rt.getError()` returns `Failed to parse models.json: Bad escaped character in JSON at position 104 …`; an absent `bifrost` provider gives `getModel` `undefined` too (inferred for the absent case) | 0.60 |
| **gateway stall** | real key, loopback, during about 2 minutes of the run | `GET /openai/v1/models`: `HTTP 200` after **11.80 s**, then **three `TimeoutError` at 15 s**, then (curl, 40 s cap) `000`, `000`, `200 after 39.6 s`, `200 after 0.47 s`; before and after it the same call took 0.37 to 1.1 s | see D6 |

Success timings (real key, helper's clock): loopback `Authorization` 1.03, **11.80**, then (after the stall) 0.99, 1.10, 1.03; loopback `x-bf-vk` 1.08, 0.94, 1.01 (three before them hit the 15 s limit during the stall); LAN 1.17, 0.94, 1.12. Median of the ten normal runs: 1.03 s; range 0.94 to 1.17 s; stalls of 11.8 s and 40 s or more happened in one window.

What the helper can tell apart, by where the error comes from: **key unset** (`ModelsError` `code` `auth` thrown by `getAuth`), **not configured** (no provider or no model, `rt.getError()` text), **401** (an HTTP status), **refused** (`cause.code` `ECONNREFUSED`), **TLS** (`cause.code` `UNABLE_TO_GET_ISSUER_CERT_LOCALLY`, or any `cause.code` starting `CERT_`/`ERR_TLS`: inferred, only this one seen), **timeout** (`name` `TimeoutError`). Anything else is "other" with the message. The deadline of the engine's step must be above the helper's own `fetch` timeout, which should be longer than 15 s: a 40 s stall happened (suggest 60 s fetch and the engine's 120 s kill, as E11).

verdict: **a helper under pi's Node can list Bifrost with pi's machinery only**: URL from `rt.models.providers.get("bifrost").baseUrl`, key from `rt.getAuth(model).auth.apiKey`, one `GET <baseUrl>/models` (`Authorization: Bearer`, or `x-bf-vk`), answer `{object, data:[{id,object,owned_by}]}` with 47 ids; the five failures are told apart in the table, in 0.6 s except a silent URL (the timeout) and the intermittent gateway stall (11.8 s to 40 s observed).

---

## D2. GitHub Copilot: what pi 1.3.0 does

### Compiled list, pi.dev overlay, or a network call?

- verified: pi's compiled Copilot catalog has 36 ids (`GITHUB_COPILOT_MODELS` of `pi-ai/dist/providers/github-copilot.models.js`: `node -e … Object.keys(…).length` → `compiled 36`). `ModelRuntime.create` with no network and no stored catalog shows the same 36 (`copilot models 36`).
- verified: **every one of the 42 built-in providers of pi 1.3.0 has a `refreshModels`**: `node prov.mjs` over `rt.models.providers` printed `providers 42`, `without:` (empty), and `with refreshModels:` — amazon-bedrock, ant-ling, anthropic, azure, baseten, cerebras, cloudflare-ai-gateway, cloudflare-workers-ai, deepseek, fireworks, **github-copilot**, google, google-vertex, groq, huggingface, kimi-coding, meta, minimax, minimax-cn, mistral, moonshotai, moonshotai-cn, nvidia, openai, openai-codex, opencode, opencode-go, openrouter, qwen-token-plan, qwen-token-plan-cn, qwen-token-plan-individual, radius, together, typesafe, vercel-ai-gateway, xai, xiaomi, xiaomi-token-plan-ams, xiaomi-token-plan-cn, xiaomi-token-plan-sgp, zai, zai-coding-cn. `github-copilot` is among them.
- inferred (read `pi-coding-agent/dist/core/model-runtime.js:84-96`, `remote-catalog-provider.js`): those `refreshModels` are not per-provider discoveries. `model-runtime.js` wraps every builtin except `radius` in `withRemoteCatalog` (the pi.dev overlay), and `openrouter` in `withPersonalizedCatalog` (it also calls OpenRouter with the key); `radius` has its own (`pi-ai/dist/providers/radius.js:26`). `pi-ai/dist/models.js:573` adds a `refreshModels` to any provider that passes `fetchModels` to `createProvider`: `grep -ln fetchModels providers/*.js` is empty, so no builtin provider does. A `models.json` provider has none (verified for `bifrost`, above); the llama.cpp extension has its own (`pi-coding-agent/dist/extensions/llama/provider.js:177`).
- verified: what the Copilot refresh calls. A fetch wrapper that prints method, host+path and status (never headers) around `rt.refresh({allowNetwork:true, providers:["github-copilot"]})` against the real agent dir:

```
FETCH GET pi.dev/api/models/providers/github-copilot 304 0.52s
refresh secs 0.529 errors []
avail 11
```
and the same with `force:true`: `304 0.71s`. So the whole network part of pi's Copilot refresh is **one request to `https://pi.dev/api/models/providers/github-copilot?types=chat,image,classifier`** with `If-None-Match` (a 304 here), no login header (inferred from `requestCatalog`: `accept`, `User-Agent`, `if-none-match`), 4 s per attempt. The answer unauthenticated: `curl https://pi.dev/api/models/providers/github-copilot?types=…` → `200 0.22s`, 26 ids, `etag`, `last-modified`. The overlay is merged **additively** (`mergeModels`, `remote-catalog-provider.js:25`): pi never drops a compiled model, and uses the overlay only if its `last-modified` is newer than the bundled data (`isNewer`). Evidence: the stored overlay (`models-store.json`, `github-copilot`: 26 models) and the compiled 36 give `after refresh all 36 … all added [] removed []`: ten compiled ids (`claude-opus-4.7`, `claude-sonnet-4.6`, `gemini-3.5-flash`, `gemini-3.6-flash`, `gemini-3.7-flash`, `gpt-5-mini`, `gpt-5.4`, `gpt-5.4-mini`, `gpt-5.5`, `mai-code-1-flash-picker`) are not in pi.dev's 26 any more, and pi still lists them.
- verified: **without a login pi makes no request at all**: in a scratch agent dir (no `auth.json`) `rt.refresh({allowNetwork:true, providers:["github-copilot"]})` took 0.018 s, errors none, `models-store.json` stayed `{}`. `refresh` fetches only when `resolveRefreshCredential` returns a credential (`pi-ai/dist/models.js:190-201`, inferred from reading); the pi.dev call is made with the login present.
- verified: the refresh interval of the pi.dev overlay is 4 h (`REMOTE_CATALOG_REFRESH_INTERVAL_MS`, `remote-catalog-provider.js:7`), skipped unless `force`. `pi update --models` forces it (`docs/models.md:15`, inferred: not run). `PI_OFFLINE` switches pi's model network off (`model-runtime.js:98`, inferred).

### The list of what the User's account can use

- inferred (`pi-ai/dist/providers/github-copilot.js:18-28`, `auth/oauth/github-copilot.js:64-98,127-146,244-282`): `filterModels` keeps only the models whose id is in `credential.availableModelIds`, when the credential is OAuth and has that array; otherwise it keeps **all**. `availableModelIds` is written **when pi refreshes the Copilot token** (`refreshGitHubCopilotToken` → `fetchGitHubCopilotModels`): `GET <copilot base>/models` (`https://api.individual.githubcopilot.com/models` for an individual account) with `Authorization: Bearer <copilot token>`, `Accept: application/json`, `User-Agent: GitHubCopilotChat/0.35.0`, `Editor-Version: vscode/1.107.0`, `Editor-Plugin-Version: copilot-chat/0.35.0`, `Copilot-Integration-Id: vscode-chat`, `X-GitHub-Api-Version: 2026-06-01`; entries with `capabilities.supports.tool_calls === false` are dropped, the rest are kept when `model_picker_enabled` and `policy.state !== "disabled"` (individual accounts fall back to `policy.state === "enabled"` when no entry is a picker model). The token expires after `expires_at - 5 min`; refresh happens in `rt.refresh` when the stored OAuth credential is expired (`models.js:235-243`).
- verified (real agent dir, `getAvailable` after `refresh`): `rt.models.getAvailable("github-copilot")` → 11 ids, the same before and after the refresh (the stored token was not expired, so pi made no Copilot call: the wrapper saw only the pi.dev fetch): `claude-haiku-4.5, claude-opus-4.8, claude-opus-5, claude-opus-5.5, claude-sonnet-5, claude-sonnet-5.5, gemini-3.8-flash, gpt-5.3-codex, gpt-5.6-luna, gpt-5.6-sol, gpt-5.6-terra`. The other 25 compiled ids are not offered to this account. So **the helper gets the account's list without reading `auth.json`**, from pi's API.
- verified: `rt.models.checkAuth("github-copilot")` → `{"source":"OAuth","type":"oauth"}` (real dir), `undefined` (scratch, no login, and `getAvailable` is 0), and with only `COPILOT_GITHUB_TOKEN=x` in the environment `{"source":"COPILOT_GITHUB_TOKEN","type":"api_key"}` and `getAvailable` **36**: an API-key login has no `availableModelIds`, so pi calls everything available. A helper must record the auth `type` and refuse to diff across a change of it.
- not done: forcing a fresh `availableModelIds` in a test. The credential refresh is made only when the stored token is expired and I may not alter the User's login; so "the list is at most one token life old (inferred: a Copilot token lives about half an hour: unverified), refreshed by the first `rt.refresh` after expiry, which a 7-day cron always meets" is inferred, not measured.

### What the helper must pass, and what it costs

verified: `PI_CODING_AGENT_DIR=<real agent dir>` (so pi finds the login and `models-store.json`), pi's Node, `ModelRuntime.create({allowModelNetwork:false})` then `await rt.refresh({allowNetwork:true, force:true, providers:["github-copilot"]})` then `rt.models.getAvailable("github-copilot")`; 0.43 s total in the first run (0.53 s, 0.71 s in the wrapper runs). The refresh **writes**: `models-store.json` gets a new `checkedAt` (verified: only `checkedAt` of `github-copilot` changed, `1792686223412` → `1792696329050`; models, etag, lastModified unchanged), and a token refresh would write `auth.json` (pi's own store, with its own lock). Both are pi's normal behaviour for the same login the Sessions use. The helper never touches `models-store.json` or `auth.json` itself. `allowModelNetwork:false` at `create` plus an explicit `refresh` is the shape of `catalog.mjs` plus one call.

### What "a model went away" means for Copilot

- verified: pi's `getAllModels()` of Copilot (36) never shrinks with a pi.dev refresh (additive), and shrinks only with a pi update that edits the compiled file: **it cannot report a removal**. The list that can is `getAvailable` (the account's list). Record both: `known` (what pi 1.3.0 knows, to attribute "appeared in pi" to a pi version) and `available` (what the account can use); only changes of `available` are the User's news. A pi update that adds a model to the compiled list changes `known`, not `available`, until Copilot offers it.
- inferred: a 7-day cron is right for `available` (it follows Copilot's `/models` at each token refresh, which pi does whenever the User works) and is never fresher than pi's token refresh; for `known` the news is "pi's compiled list changes with a pi update" (`pi_version` in the cache says why).

verdict: **pi 1.3.0 has a `refreshModels` on all 42 built-in providers, `github-copilot` included, but it is the pi.dev catalog overlay (one unauthenticated `GET pi.dev/api/models/providers/<id>`, 4 h, additive, never removes), on top of a list compiled into pi-ai per pi version (36 ids).** The account's real list (11 ids) is `availableModelIds`, written by pi's Copilot token refresh (`GET <copilot>/models`, the User's login), and is read through `rt.models.getAvailable("github-copilot")` with no `auth.json` access; the helper passes the real agent dir and pi's Node and must check `checkAuth().type === "oauth"`.

---

## D3. The two lists

### Bifrost, 47 ids (verified: `GET /openai/v1/models`, ids only)

```
vertex/claude-fable-5  vertex/claude-fable-5-1  vertex/claude-haiku-4-5  vertex/claude-haiku-5-5
vertex/claude-opus-4-5  vertex/claude-opus-4-6  vertex/claude-opus-4-7  vertex/claude-opus-4-8
vertex/claude-opus-5  vertex/claude-opus-5-5  vertex/claude-sonnet-4-6  vertex/claude-sonnet-5  vertex/claude-sonnet-5-5
vertex/codestral-2
vertex/gemini-1.5-pro-002  vertex/gemini-2.5-flash  vertex/gemini-2.5-flash-lite  vertex/gemini-2.5-flash-preview-04-17
vertex/gemini-2.5-flash-tts  vertex/gemini-2.5-pro  vertex/gemini-2.5-pro-exp-03-25  vertex/gemini-2.5-pro-tts
vertex/gemini-3-flash-preview  vertex/gemini-3-pro-image  vertex/gemini-3.1-flash-image  vertex/gemini-3.1-flash-image-preview
vertex/gemini-3.1-flash-lite  vertex/gemini-3.1-flash-lite-image  vertex/gemini-3.1-pro-preview
vertex/gemini-3.5-flash  vertex/gemini-3.5-flash-lite  vertex/gemini-3.5-live-translate-preview
vertex/gemini-3.5-transcribe  vertex/gemini-3.5-transcribe-live  vertex/gemini-3.5-transcribe-live-preview  vertex/gemini-3.5-transcribe-preview
vertex/gemini-3.6-flash  vertex/gemini-3.7-flash  vertex/gemini-3.8-flash  vertex/gemini-4-argon
vertex/gemini-embedding-2  vertex/gemini-live-2.5-flash-native-audio  vertex/gemini-nano-banana-2.1  vertex/gemini-omni-1.1-flash-preview
vertex/mistral-medium-3  vertex/mistral-ocr-2505  vertex/mistral-small-2503
```
Claude ids use dashes for the version (`claude-opus-5-5`), Gemini ids keep dots (`gemini-3.8-flash`); all carry `vertex/`.

### GitHub Copilot, pi 1.3.0

Known to pi (36, compiled; all `type: chat`, verified `types {"chat":36}`):
```
claude-fable-5 claude-fable-5.1 claude-haiku-4.5 claude-haiku-5.5 claude-opus-4.7 claude-opus-4.8 claude-opus-5 claude-opus-5.5
claude-sonnet-4.6 claude-sonnet-5 claude-sonnet-5.5 gemini-3.5-flash gemini-3.6-flash gemini-3.7-flash gemini-3.8-flash gemini-4-argon
kimi-k2.7-code kimi-k3 gpt-5-mini gpt-5.3-codex gpt-5.4 gpt-5.4-mini gpt-5.4-nano gpt-5.5 gpt-5.6-luna gpt-5.6-sol gpt-5.6-terra
gpt-6-astra gpt-6-luna gpt-6-sol gpt-6.1-sol grok-4.5 grok-4.6 grok-4.7 mai-code-1-flash-picker mai-code-1.1-flash
```
Offered to this account (11, `getAvailable`): `claude-haiku-4.5 claude-opus-4.8 claude-opus-5 claude-opus-5.5 claude-sonnet-5 claude-sonnet-5.5 gemini-3.8-flash gpt-5.3-codex gpt-5.6-luna gpt-5.6-sol gpt-5.6-terra`.

### The four Tier models (verified by reading the lists above and by `node norm.mjs`)

| Tier | Copilot id (offered to this account?) | Bifrost id | same model? |
|---|---|---|---|
| O | `claude-opus-5.5` (yes) | `vertex/claude-opus-5-5` | yes |
| S | `claude-sonnet-5.5` (yes) | `vertex/claude-sonnet-5-5` | yes |
| H | `claude-haiku-4.5` (yes) | `vertex/claude-haiku-4-5` (**`is_deprecated: true`** on `/v1/models`) | yes |
| G(X) | `gemini-3.8-flash` (yes) | `vertex/gemini-3.8-flash` | yes |

All four are on both providers, so D14 ("the same model on both whenever both offer it") holds for the four today. That "same model" is by name only: that Bifrost routes `vertex/claude-sonnet-5-5` to the model GitHub serves as `claude-sonnet-5.5` is the name's claim, not tested (`not done: no completion made, budget`).

### Normalisation rule

`canonical(id) = lower-case(id) with the leading "<segment>/" removed (a Bifrost provider prefix: "vertex/") and every "." replaced by "-"`. Two ids are the same model when their canonical forms are equal. Version suffixes (`-preview`, `-002`, `-2505`, `-exp-03-25`, `-lite`) are **kept**: they are different models.

Test (verified, `node norm.mjs` over the two full lists):

```
bifrost ids 47 canonical 47 collisions []
copilot all ids 36 canonical 36 collisions []
copilot avail ids 11 canonical 11 collisions []
copilot(all)∩bifrost: 16
copilot avail ∩ bifrost: 7  avail only copilot: gpt-5-3-codex gpt-5-6-luna gpt-5-6-sol gpt-5-6-terra
```
The 16 matches (Copilot = Bifrost): `claude-fable-5`, `claude-fable-5.1 = vertex/claude-fable-5-1`, `claude-haiku-4.5`, `claude-haiku-5.5`, `claude-opus-4.7`, `claude-opus-4.8`, `claude-opus-5`, `claude-opus-5.5`, `claude-sonnet-4.6`, `claude-sonnet-5`, `claude-sonnet-5.5`, `gemini-3.5-flash`, `gemini-3.6-flash`, `gemini-3.7-flash`, `gemini-3.8-flash`, `gemini-4-argon` (each with `vertex/` and dashes for the Claude ones on Bifrost). No collision inside any list. **Unmatched**: Copilot only (20): `kimi-*`, all `gpt-*`, `grok-*`, `mai-code-*`; Bifrost only (31): the Claude 4.5/4.6 Opus, `codestral-2`, 22 Gemini variants (1.5, 2.5, 3, 3.1, `3.5-flash-lite`, the audio/image/embedding ones, `gemini-omni`, `nano-banana`) and the Mistral ones. **Ambiguous ids: none found.** A residual risk (inferred): `.`→`-` would merge `gemini-3.5-flash` with a hypothetical `gemini-3-5-flash`; none exists in either list, and the diff never needs the rule (it compares ids within one provider, D4): the rule only serves "is this Tier model on both".

### What is not a chat model, and how to tell

- Copilot: nothing in pi's list (verified: 36 of type `chat`). pi's own parsing drops `/models` entries without tool calls (`tool_calls === false`, inferred from `parseGitHubCopilotModelCatalog`) and non-picker entries, so embeddings never arrive. `gpt-5.3-codex` is a chat (responses API) model.
- Bifrost: **no field says so on `/openai/v1/models`** (verified: ids only). By id (inferred from the names; none of them was called): embedding `gemini-embedding-2`; speech `gemini-2.5-flash-tts`, `gemini-2.5-pro-tts`; transcription `gemini-3.5-transcribe`, `-transcribe-live`, `-transcribe-live-preview`, `-transcribe-preview`; live/audio `gemini-3.5-live-translate-preview`, `gemini-live-2.5-flash-native-audio`; image `gemini-3-pro-image`, `gemini-3.1-flash-image`, `gemini-3.1-flash-image-preview`, `gemini-3.1-flash-lite-image`, `gemini-nano-banana-2.1`; OCR `mistral-ocr-2505` (15 ids); unclear: `gemini-omni-1.1-flash-preview`. On `/v1/models` the `pricing` keys help only for images (an `image` price key on 5 of the 5 image ids, verified) and tell nothing for embedding/TTS/transcribe (`prompt`/`completion` only). A discovery that must not call image or embedding models "new chat models" needs the id pattern `/embed|tts|transcribe|live|image|nano-banana|ocr/`; as the diff is by id, the simplest is to **list everything and mark the class** (inferred), the Tier models being chosen statically anyway.

verdict: the four Tier models are on both providers (O `claude-opus-5.5` = `vertex/claude-opus-5-5`, S `claude-sonnet-5.5` = `vertex/claude-sonnet-5-5`, H `claude-haiku-4.5` = `vertex/claude-haiku-4-5`, G `gemini-3.8-flash` = `vertex/gemini-3.8-flash`); the rule (drop `<provider>/`, `.`→`-`, lower case) matches 16 ids across the full lists with no collision, no ambiguity; 15 Bifrost ids are not chat models and only the id says so.

---

## D4. What the cache and the diff need

### Fields

- **Compare ids only** (verified for both): Bifrost's OpenAI route has no other field (`owned_by` is always `""`); Copilot's list as pi gives it is a set of ids (the other fields are pi's compiled data). `is_deprecated`, price and limits of `/v1/models` flip without the model going away and are not part of "available/unavailable" (inferred; D6 asks if a `deprecated` notice is wanted).
- Keep the order out of the comparison: compare sets; store sorted.

### A `models-<provider>.json` cache file (inputs, not a design)

`run/models-bifrost.json` and `run/models-github-copilot.json`, beside `run/team-review.json` (contract §1.4, line 133), written whole by `.tmp` and rename, derived and not in the store (the E11 pattern):

```
{ "version": 1,
  "provider": "bifrost" | "github-copilot",
  "polled_at": "<time>",              // of the last GOOD list: the 7 days are counted from it
  "pi_version": "1.3.0",              // Copilot: the compiled list changes with pi; Bifrost: informational
  "source": { "kind": "openai-models" | "copilot-available", "auth": "oauth"|"virtual-key", "base": "<scheme://host:port>" },
  "ids": [ "…sorted…" ],
  "known": [ "…" ],                   // Copilot only: getAllModels() ids (36), to say "pi learned it"
  "changes": { "at": "<time>", "appeared": [ … ], "went_away": [ … ] }   // the diff to the previous good list; absent on the first run
}
```
`base` is the host, never a key; for Copilot there is no URL. (inferred: a list taken through the loopback proxy and through the LAN are the same list, verified 47 = 47, so the base is not part of the comparison.)

### Available and unavailable

`appeared = ids(new) − ids(old)`, `went_away = ids(old) − ids(new)`, on the last two **good** lists of the **same** provider and the same `auth` (verified need: Copilot's list is 11 ids with the login and 36 with an API key). Nothing else.

### What a failed or partial list must do

The rule is the contract's for E11 (line 857): a failure is an `error` Event; the last good file stays; and **unknown is not empty**: never write a list that is empty or taken in a failed state, and never diff against it.

| provider | looks like | verified or inferred | treat as |
|---|---|---|---|
| Bifrost | key unset, 401, refused, TLS, timeout, provider not configured | verified (D1 table) | `error` with a code each; keep the file |
| Bifrost | a `200` whose JSON has no `data` array, or one whose entries lack a string `id` | inferred | `error` (`models_unparseable`); keep the file |
| Bifrost | `200` with `data: []` | not done: not produced (a key with no model, not available to try) | an **empty list is an error**, never "all models went away"; keep the file |
| Bifrost | a stall, then late `200` | verified: 11.8 s and 39.6 s | fine if the deadline allows; a 40 s answer is a valid list |
| Bifrost | the list is partial (the gateway hides a provider while it is down) | not done: no way to provoke | cannot be detected by shape; a guard "drop of more than half the ids is held for one more poll before it is reported" is a design question (D6) |
| Copilot | no login: `checkAuth` `undefined`, `getAvailable` **0** | verified | `error` (`not_logged_in`); keep the file |
| Copilot | API-key credential: `getAvailable` **36** (all compiled) | verified | refuse to diff (auth type changed); keep the file |
| Copilot | `refresh` errors (`result.errors`), credential store read failing | inferred (`ModelsError` `auth`, `model_source`) | `error`; the helper must read `result.errors` and raise it: pi **swallows** a refresh failure into `errors` and still lets `getAvailable` answer from the stored list |
| Copilot | token not expired: `availableModelIds` as stored | verified (no Copilot call made) | valid, at most one token life old |

The first run (no previous file): write the file with no `changes`, **no message** (inferred from the requirement: "when models become available or unavailable"; there is nothing to compare). A provider whose list could not be read at the first run leaves no file, and the next success is again a first run.

verdict: compare ids only, as sets, per provider and per auth type; the cache holds the sorted ids, `polled_at` of the last good list, `pi_version`, and the last `changes`; failures, empty answers and a changed auth type keep the last good file and report an `error`, never a mass "went away"; the first run writes the file and says nothing.

---

## D5. Where the pieces go in cpd (all inferred from reading, with the file:line)

### The engine step pattern: E11

| piece | where |
|---|---|
| step spec | `docs/contract.md:857` (E11), deadline `:863`, a step's failure `:879`, the cache's place in the run directory `:133` |
| one module per step, registered in the tick | `engine/src/watch/mod.rs:3-12` (module list), `engine/src/watch/engine.rs:116-139` (`steps: [fn(&mut Engine); 14]`, `team::run` at `:131`) |
| cache path | `engine/src/watch/team.rs:34-36` (`cache_path`: `home.run_dir().join("team-review.json")`; `CpdHome::run_dir` `core/src/home.rs:183`) |
| state between ticks | `team.rs:39-45` (`State {running, last_end}`: in memory), field `engine.rs:65-66` |
| detached run with a deadline | `team.rs:87-124`: `Command` for pi's `node` (`core/src/config/catalog.rs:94` `node()`), args from `locate_pi()` (`catalog.rs:32-41,49`: `agent_dir`, `version`, `coding_agent_entry`, `pi_ai_entry`), script `config.pi_adapter_dir()/lib/slack-channel.ts` (`core/src/config/file.rs:407`), `PI_CODING_AGENT_DIR` and `CPD_HOME` env, `CPD_*` selection removed, `std::thread::spawn` + `mpsc::channel`; `deadline::output(&mut cmd, "node", DEADLINE)` (`core/src/host/deadline.rs:46`: its own process group, killed at the deadline, `tool_timeout`); `DEADLINE = 120 s` at `team.rs:31` |
| the helper's printed answer | `engine/src/watch/pr.rs:248-269` `printed`: `{"ok":true,…}` or `{"ok":false,"error":{code,message,details}}` become `Ok(Value)` or a `Failure`; anything else is `spawn_failed`; the helper side: `pi/lib/slack-channel.ts:24-37` (`Refusal` with a code and an exit status), `:191-204` (prints one JSON value) |
| the cache written whole, by rename | `team.rs:126-152` (`store`: validate the answer, `.json.tmp`, `fs::rename`); on failure the old file is untouched |
| failure → `error` Event, once | `team.rs:60-62` → `engine.fail(Step::E11, About::Machine, failure)`; `engine/src/watch/report.rs:198-231` (`fail`: an `error` Event once until `clear`, repeated after `watch.error_repeat_seconds`), `:233` (`clear`), step names `:30-59`; contract `:879` (the Event goes to the Manager as a message, hs-d314, not to the Inbox) |
| timing | the period after the **end** of the previous run: `team.rs:72-75` (`Duration::from_secs(team_review_seconds)` against `last_end: Instant`); key `watch.team_review_seconds`, default 1800: `core/src/config/file.rs:195,337`, `docs/contract.md:3446,3488`; the first run starts with the engine (`team.rs:2-5`) |
| the helper's tests | `pi/test/slack-channel.test.ts`; engine test `cli/tests/engine/watch_team.rs` |

**An important difference for a 7-day cron** (verified by reading `team.rs:45,72-75` and the contract text "the first starts with the engine"): E11's "last run" is an `Instant` in memory, so an engine restart (every install restarts it, contract §3.7) starts E11 at once. A 7-day step must read the age from the file's `polled_at` (and from `last_end` only for the failed-run backoff), or every restart refreshes and, on a failing provider, every tick would retry (the failed run leaves `polled_at` old): E11 solves the second by `last_end = now` on a failed start (`team.rs:79-82`) and after any answer (`team.rs:62`), the same is needed here.

### Where Inbox kinds are defined and coloured

- Kinds: `core/src/inbox/model.rs:21-39` (`enum Kind`), `:58-71` (`as_str`, `parse`); the contract table `docs/contract.md:517-530` (the kinds, with the `curate-*` rows at `:528-529`); the store check `kind IN (…)` in the latest rebuild `core/src/store/migrations/20261022T1540-loop-guard.sql:5` (a new kind is a **named migration** like `loop-guard`/`blue-tier`, and a breaking one when an older binary would write a row the check refuses: contract §4.5).
- Colours: `Urgency` enum `core/src/inbox/model.rs:78-89` (`White < Blue < Magenta < Yellow < Red`), the check `urgency IN ('red','yellow','magenta','blue','white')` `core/src/store/migrations/20261022T1449-blue-tier.sql:6`, the contract's field table `docs/contract.md:502`. **There is no cyan Inbox tier.** The palette (`docs/contract.md:2352` onwards, the meanings at `:2369-2375`): **cyan (6, 14) = agent activity** (running Jobs, the Lead-working dot, what a Lead handles, a log error addressed to a Lead), **blue (4, 12) = "focus and selection, and a cpd notice, purely informational"** (hs-d570): "the Inbox's blue tier (a notice that blocks nothing and decides nothing before the work runs: `curate-docs` and `curate-retire` items)".
- So: "an Inbox message coloured cyan" is either the **existing blue tier** (colour 4/12, the cpd-notice tier, what a "models appeared/went away" notice is by its own definition) or **a new cyan tier**, which would use the colour of "agent activity" for a notice, need a new `Urgency` value, a sixth tier in the rank order, the `urgency` check rebuilt in a breaking migration, §6.1.1, §2.2, §9 precedence, `commands.md` §13.2 and §17 amended. `not done: which one the User means`: D6.

### Does an informational item fit E8?

- E8 (contract `:854`, `engine/src/watch/inbox.rs:17-36`, `core/src/inbox/rules.rs:54` `conditions_with`, `core/src/inbox/model.rs:352` `reconcile`) is "condition recomputed from state": every tick it computes the set of conditions from the store, files and clock, and opens or closes items to match; a closed item is not reopened, a **dismissed** one stays hidden "for the rest of its life" while its cause lives (`docs/commands.md` §21.4).
- A "models changed" notice is an **event**, not a lasting state: nothing in "the Bifrost list has 47 models" stays true or false. It fits E8 only if E8 can recompute it from something that stays. Two shapes in use:
  1. **A condition from a file with a window**: `review-requested` (`rules.rs:444-485`, `kept_review_items` `:491`): E8 reads `run/team-review.json`, takes the posts of the last 7 days (`team::WINDOW`) and the item closes when its post leaves the window or its cause changes; a missing or bad file keeps the open items as they are. For models: E8 reads `changes` of each `models-<provider>.json`, opens one item per provider while `changes.at` is within a window (N days) and `changes` is non-empty, subject `models:<provider>/<changes.at>` (a new change is a new subject, as `pr:` and `distill:<context>/<upto_seq>` are), closes it after the window. The User's action is **dismiss** (`inbox dismiss`, which hides it). No new state; nothing in the store but the Inbox rows.
  2. **A condition from rows the User decides**: `curate-retire`/`curate-docs` (`rules.rs:251-285`; the comment at `:323`: "the item follows it without closing, so a dismissal stays"): open while N rows are `pending`; closes when the User decides them by a command (`context retire`, `context docs`; contract `:528-529`). For models this needs a table (or a `acknowledged` field) and a command `cpd models ack` and a migration plus a protocol bump (contract §4.5).
- Answer: **shape 1 fits E8 with no new state** and its close is time plus the existing `dismiss`; a notice that must close **only** by the User's action needs shape 2 (store and command). `curate-*` is the existing pattern for the latter and is already blue.
- Do not mix: an `error` of the discovery step is not an Inbox item (contract `:513`: the engine's step errors go to the Manager); only a real change is a User notice.

### Where a CLI command would live

- Top-level commands are registered in `cli/src/cli.rs` (`Command::Tiers` `:80`, dispatch `:133`); the behaviour lives in `core/src/<area>/`: `core/src/config/mod.rs:46` (`tiers`, `config`) and the `inbox` area `core/src/inbox/cmd.rs:304` (`dismiss`). The docs: `docs/commands.md` §14 "`doctor`, `config`, `tiers`" (`:3115`; `tiers` at `:3179`) is the nearest (a read of pi's state about models), or a new numbered section after §23; the index §2 (`:178`) and the generated `docs/interface.md`, checked by `cli/tests/interface.rs`, list the command with its callers and `cockpit`/`internal` flags; a User's-shell command with no callers is the form of `doctor`. Proposed name `cpd models refresh [--provider bifrost|github-copilot]` and `cpd models show` (or `list`): `refresh` runs the same helper as the engine step with the same deadline and prints the `changes`; its failures are the step's codes (contract §1.2, line 82 lists the `slack_*` codes as the pattern: new codes `models_*`, exit 1; a missing login like `slack_auth` exit 3).
- Who writes the file when the User refreshes by hand while the engine could write it too: both write whole by rename, the later wins (inferred: no lock exists for `team-review.json`; none is needed if the diff is computed by the writer from the file it replaces, but two concurrent writers can produce a `changes` computed from the same base: take `locks/` for the write, a design question).

### The 7-day key

Existing keys of `[watch]` (`docs/contract.md:3443-3452`, `core/src/config/file.rs:186-199`): `tick_seconds`, `pr_poll_seconds`, `team_review_seconds`, `error_repeat_seconds`, `pr_poll_transient_minutes` (and `inbox.stuck_job_minutes`, `timeouts.sleep_seconds`): all integers, units in the name, `positive(...)` of at least 1 (`file.rs:335-339`). By that convention: **`watch.models_discovery_seconds`, default `604800`** (7 days), ≥ 1; the `_days` unit has no precedent. The helper's deadline stays a constant next to `DEADLINE` (`team.rs:31`, 120 s), as E11's. A new step is `E14` (E12 and E13 exist, `engine.rs:116` comment) added to `Step` (`report.rs:30-59`) and to the `steps` array.

verdict: E11 is the template (detached helper under pi's Node, 120 s `deadline::output`, answer `printed`, cache by rename, failure = one `error` Event with the last good file kept) at `team.rs:31,34,47-152`, `report.rs:198-233`, `engine.rs:131`, contract `:857,863,879`; the Inbox has **no cyan tier** (red, yellow, magenta, blue, white; cyan is "agent activity", blue is "a cpd notice, purely informational": `model.rs:78`, `contract.md:502,2372,2374`); a change notice fits E8 as a windowed file condition (like `review-requested`) closed by time and `dismiss`, and needs the `curate-*` store pattern only if it must close by an explicit acknowledgement; `cpd models` belongs beside `tiers` (§14) or as a new section; the key is `watch.models_discovery_seconds` = 604800, counted from the file's `polled_at`, not from an in-memory `Instant`.

---

## D6. Risks, unknowns and decisions for the User

1. **Cyan versus blue** (decide). The Inbox has five tiers and no cyan; the requested notice is by definition the blue tier (hs-d570). Using blue is free; a cyan tier is a breaking migration plus a colour-meaning conflict (cyan is "agent activity"). `not done: what the User meant by "cyan"`.
2. **The gateway stalls.** One window of about 2 minutes: `GET /openai/v1/models` took 11.8 s, three times timed out at 15 s, curl timed out twice at 40 s, one 200 took 39.6 s; before and after it 0.37 s to 1.2 s (verified). It affects D14's suspicion about gemini (22 s, 84 s): a slow gateway, not a slow model, is a candidate (inferred: not measured on a completion). `not done`: cause (load, my own parallel Jobs on the same gateway, a Vertex quota); no completion was made. Any helper needs a deadline ≥ 60 s and one retry is cheap.
3. **The Tier H model is flagged deprecated** by Bifrost (`vertex/claude-haiku-4-5`, `is_deprecated: true` on `/v1/models`, verified). The User should know; a deprecation notice would need `/v1/models`, not the OpenAI route.
4. **Prices exist.** `GET /v1/models` has `pricing` for 42 of 47 models and `context_length`/`max_output_tokens` for 34 (verified), so the static `models.json` `cost`/limits (D12) can be filled from it at render time, and the distiller's `cost: 0` need not stay 0. This is a finding for the dotfiles PR, outside discovery.
5. **Copilot's "known" list is additive**: it never shrinks except with a pi update, and pi.dev overlays 26 of 36 ids; the account list (`getAvailable`, 11 ids) is the only source of "went away", and it is a function of the credential **type** (11 with the login, 36 with `COPILOT_GITHUB_TOKEN`, 0 with none: verified). The helper must record the auth type and the diff must refuse across a change.
6. **A refresh that fails is silent in pi**: `rt.refresh` returns `errors` rather than throwing, and `getAvailable` then answers from what is stored. The helper must treat `result.errors` as the step's failure (inferred from `models.js:183-216`; the success path was verified, the failure path not).
7. **The helper writes pi's files.** `rt.refresh` updates `models-store.json` (`checkedAt`; verified) and may rewrite the OAuth credential in `auth.json`; both are pi's own writes for the same login, under pi's locks, but a cron that is not a Session now touches them (inferred: no conflict seen; `not done` under load).
8. **"No HTTP client" and a `fetch`.** D1's helper makes its own `fetch` under pi's Node (as `slack-channel.ts` calls Slack through pi's MCP client, not `fetch`). Contract (hs-d107) says cpd has no HTTP client in Rust; a TypeScript helper using Node's global `fetch` stays on the allowed side by the letter, but it is the first helper that calls an HTTP API **itself**, holding the key in its memory (pi resolved it). The User should confirm this reading, or choose to make pi do it: `not done: there is no pi API that lists an OpenAI-compatible provider's models for a `models.json` provider` (verified: `refreshModels undefined`; `docs/models.md:191` sends discovery to an extension, which D12 excluded).
9. **TLS on the LAN.** The helper under the engine's environment needs `NODE_EXTRA_CA_CERTS` on hs and air (verified error otherwise: `UNABLE_TO_GET_ISSUER_CERT_LOCALLY`); the engine must pass it to the helper as pi's other runs get it, or the Bifrost discovery fails with a TLS error on those machines every 7 days.
10. **A cached list is a snapshot of one key.** Bifrost lists what the virtual key may use (inferred: the 401 text and `key_statuses`); a rotated key with different rights changes the list without any model change. Record the key's identity? Not the key: `not done: no non-secret key identifier found (key_statuses was not read)`.
11. **Partial lists** cannot be detected by shape (D4). Decide whether a drop of more than half the ids is held for one poll before it is reported.
12. **The 7-day clock** must come from `polled_at` in the file, not from an in-memory `Instant` (D5): otherwise every engine restart refreshes.
13. **Not done in this Job** (outside its budget: list calls only): the D14 cache-rate and thinking-effort measurements over several turns and runs. Nothing here measures `usage` (`cacheRead`, `cacheWrite`, reasoning tokens), the effect of `x-session-id`, or gemini's latency through pi. `not done: no completion was allowed`.
14. **Things the User must decide**: (a) blue or a new cyan tier; (b) one notice per provider and change, closed by time and `dismiss`, or by an explicit acknowledgement (needs a store table and a command); (c) whether `appeared` includes non-chat models (list everything and mark the class, or only chat-looking ids); (d) whether discovery also reports `is_deprecated` flips; (e) the TypeScript `fetch` reading in item 8; (f) whether the Copilot notice is about `available` only (recommended) or also `known`.

verdict: the discovery can be built inside the existing rules (helper under pi's Node, no credential of cpd's, E11's cache and failure pattern, E8 windowed condition); the User must decide the colour (no cyan tier exists), the close rule, and the helper's own `fetch`; the one thing that cannot be built as E11 is the 7-day clock, which must live in the file.
