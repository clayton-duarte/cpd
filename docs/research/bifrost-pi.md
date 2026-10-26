# pi 1.3.0 with pi-bifrost-provider 0.2.0 against live Bifrost

Spike `spike-pi`, Attempt 1, run on `pro` on 2026-10-22 (node v22.22.2, pi 1.3.0 from `~/.pi/agent/install/releases/1.3.0`, package unpacked at `/tmp/cpd-bifrost/pkg/package`). Every claim is `verified` (command and trimmed output) or `inferred` (read, not run).

Method (verified): every run used a scratch agent dir, `PI_CODING_AGENT_DIR=/tmp/spk-a1/<name>` (pi starts and runs from it; the real `~/.pi/agent` was not touched), through `env -u CPD_HOME -u CPD_SESSION -u CPD_PLAN -u CPD_ATTEMPT PI_SKIP_VERSION_CHECK=1 pi --no-session ...`. The key was read into the environment at run time and never printed. Nine real completions plus one curl completion (gemini latency) were used. "Listener" below is a 30-line `node` http server on 127.0.0.1 that logged request line, header names and non-secret body fields and answered `/openai/v1/models` and a fake SSE completion.

## Things found that bear on the Plan's decisions (flags)

1. **verified** Bifrost's `GET /openai/v1/models` returns only `{"id","object","owned_by"}` per model (`node -e` over the 47 entries; sonnet: `{"id":"vertex/claude-sonnet-5-5","object":"model","owned_by":""}`). No `reasoning`, no pricing, no limits. So the package registers every model with `reasoning:false`, `cost` all 0, limits from models.dev (see P2). Consequences: pi offers no thinking levels for the four A4 models, and the cost in pi's usage is always `0` (the distiller's `runs.jsonl` gets tokens but `cost: 0`).
2. **verified** cpd's `catalog.mjs` (A4/pro-n4) sees none of the Bifrost refs, even when the scratch `models-store.json` already holds the persisted catalog: `PI_CODING_AGENT_DIR=<scratch> node --input-type=module -e "$(cat core/src/config/catalog.mjs)" -- <pi-coding-agent>/dist/index.js <pi-ai>/dist/index.js O=bifrost/vertex/claude-opus-5-5 S=... H=... G=...` printed `{"O":null,"S":null,"H":null,"G":null}`. A built-in Bifrost Tier would be `tier_model_unknown` unless the catalog check is changed.
3. **verified** `-e <the package>` while the same package is declared in settings `packages` makes pi exit 1 (flag conflict). cpd's rule is right for that case; but `--no-extensions` also switches off the settings packages, so auxiliaries must always pass `-e` (P1).
4. **verified** `--model bifrost/vertex/claude-sonnet-5-5:bogus` is not an error: pi warns and sends the model id `vertex/claude-sonnet-5-5:bogus` to Bifrost (P2).
5. **verified** the package writes a notice to stderr on every non-TTY run (`Bifrost reported no context window for 7 model(s)...`), including the title writer and distiller (P3). The title writer passes pi's stderr to the engine log.
6. **verified** the package adds about 2.2 s to every pi start that has `BIFROST_URL` set (P3), 15 s at worst when discovery times out.
7. **verified** a pi bash tool process inherits `BIFROST_VIRTUAL_KEY` (P4).
8. **verified** gemini-3.8-flash is slow through Bifrost: 84 s (pi) and 22 s (curl) for a one-word answer (P2/P3).

## P1. Loading

**Local path with `--no-extensions`** (verified):
`BIFROST_URL=http://127.0.0.1:8788 pi --no-session --no-extensions -e /tmp/cpd-bifrost/pkg/package --list-models | grep -c ^bifrost` printed rows `bifrost  vertex/claude-opus-5-5  1M  128K  no  no`; 47 bifrost rows, real 1.6 s on the first run (models.dev fetched).

**`-e npm:pi-bifrost-provider@0.2.0` without a global install** (verified): works. Command: `PI_CODING_AGENT_DIR=…/ag-p1 pi --no-session --no-extensions -e npm:pi-bifrost-provider@0.2.0 --list-models`. pi installs it into `<agent dir>/tmp/extensions/npm/<hash>/node_modules/pi-bifrost-provider` (listing: `ag-p1/tmp/extensions/npm/f35b2129/{node_modules,package.json,package-lock.json}`), it needs the npm registry the first time. With the temp dir removed each time (npm's own cache warm, so this is not a cold-network figure, unverified): 0.78, 0.75, 0.79 s (median 0.78 s); with the temp install present: 0.26, 0.27, 0.27 s (median 0.27 s), no network. A cold npm cache and an offline machine were not tried (inferred: the first run then needs the registry and fails without it).

**Settings declare the package** (verified, scratch `settings.json` = `{"packages":["npm:pi-bifrost-provider@0.2.0"]}`, `--list-models` unless stated):

| command | bifrost rows | requests to listener | result |
|---|---|---|---|
| no `-e`, no `--no-extensions` | 4 | 1 | works (pi installs the package to `<agent dir>/npm` itself, 0.98 s first run, 0.27 s after) |
| `--no-extensions` | 0 | 0 | settings `packages` are switched off |
| `-e npm:pi-bifrost-provider@0.2.0` | 4 | 2 | two copies load |
| `--no-extensions -e npm:pi-bifrost-provider@0.2.0` | 4 | 1 | works |

With `-p ping --model bifrost/vertex/claude-haiku-4-5` (a completion, not a listing), settings package plus `-e npm:pi-bifrost-provider@0.2.0` printed:
`Error: Failed to load extension ".../ag-p1b/npm/node_modules/pi-bifrost-provider/index.ts": Flag "--bifrost-url" conflicts with .../tmp/extensions/npm/f35b2129/node_modules/pi-b...` (also for `--bifrost-api-key` and `--bifrost-virtual-key`), `Hint: Start without extensions using "pi -ne".`, **exit=1, stdout empty**. The same with `-e /tmp/cpd-bifrost/pkg/package` (a local path, a different identity from the settings npm entry) fails the same way: `Flag "--bifrost-url" conflicts with /tmp/cpd-bifrost/pkg/package/index.ts`. `--list-models` printed none of these errors (exit 0), so a listing is not a test of the duplicate.

**cpd's rule** (`core/src/host/pi/packages.rs` `extensions_to_pass`, inferred from reading it; its tests say "skips npm specs declared globally whatever the version"): a candidate `npm:pi-bifrost-provider@0.2.0` against a settings entry `npm:pi-bifrost-provider@0.2.0` has the same identity (`Npm("pi-bifrost-provider")`), so no `-e` is passed, which is the right outcome (verified above: `-e` there exits 1). The version is ignored, so a settings entry at another version silently wins (inferred). The rule is **not enough for the auxiliaries**: they pass `--no-extensions`, which turns the settings packages off (verified: 0 rows), so the rule's "already declared" skip would leave the title writer and distiller without the provider. They need `-e` unconditionally (verified: the `--no-extensions -e` row works). Today `retitle.rs` and the distiller launch do not use `extensions_to_pass` (inferred from `grep -rn extensions_to_pass core/src`; only `driver/worker.rs` and `session/run.rs` call it).

verdict: `-e` of the local path and of `npm:pi-bifrost-provider@0.2.0` both work with `--no-extensions`; with the package in settings, a second `-e` of it exits 1, so cpd's identity rule is right for Sessions that load settings and must not be applied to `--no-extensions` auxiliaries, which always need `-e`.

## P2. A Worker-shaped run on Bifrost

Worker-shaped (verified): `BIFROST_URL=http://127.0.0.1:8788 pi --approve --no-skills --no-session --no-extensions -e <pkg> --exclude-tools ask_user_question,cpd_plan_review,plannotator,plannotator_mark_done,plannotator_submit_plan,plannotator_inbox --model bifrost/vertex/claude-sonnet-5-5:medium -p "Use the read tool to read /tmp/spk-a1/small.txt and answer with only the file's first word."` (file: `hello-world is the first word`) printed `hello-world`, 2.9 s. The answer can only come from the file, so the tool round trip worked. The two-slash ref `bifrost/vertex/claude-sonnet-5-5` resolves (verified; pi splits at the first slash).

One-line completions (verified, `--mode json`, prompt `Reply with the single word: pong`): each gave content `pong`, `stopReason: stop`.

What pi knows about the models (verified): the persisted catalog in the scratch `models-store.json` for all four: `"reasoning":false`, `"cost":{"input":0,"output":0,...}`, `"compat":{"supportsReasoningEffort":false,...,"maxTokensField":"max_completion_tokens"}`; `contextWindow`/`maxTokens` come from models.dev: opus 1000000/128000, sonnet 1000000/128000, haiku 200000/64000, gemini 1048576/65536. `pi --list-models` shows `thinking: no` for all four. Bifrost sends no `reasoning.supported_efforts` (flag 1), so the package's `thinkingLevelMap` is never built (inferred from index.ts: it needs `model.reasoning.supported_efforts`).

Unsupported levels (verified, listener, `--model bifrost/vertex/claude-sonnet-5-5:<level>`, body keys of the POST): for `:off`, `:minimal`, `:low`, `:medium`, `:high`, `:xhigh` the body is always `max_completion_tokens,messages,model,store,stream,stream_options` with **no `reasoning_effort`**; pi exits 0 and prints no warning. So any level is silently ignored (clamped to off). `:bogus` is not a level: `Warning: Model "vertex/claude-sonnet-5-5:bogus" not found for provider "bifrost". Using custom model id.` and the POST carried `"model":"vertex/claude-sonnet-5-5:bogus"`. The upstream's own default thinking still happens: the gemini run with `:high` reported `"reasoning":16` tokens in usage.

| model | completion / round trip | thinking levels pi offers | unsupported level |
|---|---|---|---|
| `bifrost/vertex/claude-opus-5-5` | `:xhigh`: ok (`pong`, 2.1 s, in 3511/out 4) | none (`reasoning:false`, list `thinking no`) | silently ignored, no `reasoning_effort` sent |
| `bifrost/vertex/claude-sonnet-5-5` | tool call round trip ok (read tool, `hello-world`) | none | silently ignored (all six levels checked on the listener) |
| `bifrost/vertex/claude-haiku-4-5` | `:low`: ok (`pong`, 1.2 s, in 2867/out 5) | none | silently ignored |
| `bifrost/vertex/gemini-3.8-flash` | `:high`: ok (`pong`, 83.8 s, in 2035/out 17, reasoning 16); `:off`: ok (84.9 s); curl direct: 22.2 s | none | silently ignored; Gemini thinks by default anyway |

Usage and cost (verified): every run's `message.usage.cost.total` was `0` (`"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}`), tokens were reported (`input`, `output`, `reasoning`, `totalTokens`). The input size of a `--no-tools --no-context-files` "pong" is 2000 to 3500 tokens (pi's system prompt).

Gemini latency (verified): `curl` of `/openai/v1/chat/completions` with the same one-word prompt took `total=22.186355s ttfb=22.185970s`; pi took 83.8 s and 84.9 s on two runs and 40.5 s (including a 15 s discovery timeout) in the title shape. This is a hs-d562 concern for a title writer on G.

verdict: the Worker shape and all four models work on Bifrost with the package; there are no thinking levels and no cost for any of the four, any `:level` is silently dropped, and gemini is slow.

## P3. The auxiliaries

**Title writer** (verified): `pi --print --no-session --no-extensions -e <pkg> --model bifrost/vertex/gemini-3.8-flash:off -- "Write a title of at most 6 words for this conversation. Answer with one line: TITLE: <title>" < convo.txt` → exit 0, stdout `TITLE: Refactor Billing for Idempotent Invoices`, 40.5 s. stderr had `pi-bifrost-provider: startup model discovery failed, continuing without a preloaded catalog: The operation was aborted due to timeout` (the 15 s discovery timeout fired once; five `curl` calls to `/models` just after took 0.9 to 1.7 s, so it was transient); the run still worked because the scratch `models-store.json` held the catalog from earlier runs (inferred; with no stored catalog a failed discovery makes the model unknown, see P7).

**Distiller** (verified): `pi --mode json --no-session --no-extensions --no-skills --no-context-files --no-tools --model bifrost/vertex/claude-haiku-4-5:low -e <pkg> -- @dp.txt` → exit 0, 5.2 s, events `session,agent_start,turn_start,message_start,message_end,...,message_update×…,message_end,turn_end,agent_end,agent_settled`. The final assistant `message_end` carries `usage: {"input":2901,"output":50,"cacheRead":0,"cacheWrite":0,"reasoning":0,"totalTokens":2951,"cost":{...,"total":0}}`. So tokens are reported for a Bifrost model, cost is `0` (flag 1). The package's stderr notice (`Bifrost reported no context window for 7 model(s)...`) appears on every run when stdout is not a TTY (flag 5); the distiller's pi stdout (the JSON) is unaffected.

**Startup delay of the package** (verified; `pi --no-session --no-extensions [-e <pkg>] --list-models`, three real runs each, seconds, median in brackets):

| case | runs | median |
|---|---|---|
| no `-e` (baseline) | 0.19, 0.18, 0.18 | 0.18 |
| `-e`, `BIFROST_URL` set, models.dev cache warm | 2.16, 2.42, 2.35 | 2.35 |
| `-e`, `BIFROST_URL` set, models.dev cache removed | 2.85, 2.55, 2.35 | 2.55 |
| `-e`, `BIFROST_URL` unset | 0.21, 0.22, 0.27 | 0.22 |

So discovery adds about 2.2 s (one `/models` fetch of about 1 s plus loading) to every pi start, a cold models.dev fetch about 0.2 s more; the cache is `~/.cache/pi-bifrost-provider/models-dev.json` (6.06 MB, 24 h TTL per index.ts, inferred). With the URL unset there is no cost. A Lead, Manager or Worker pays this once per start, the title writer and the distiller on each run.

verdict: both auxiliary shapes work with `-e`; the distiller's stream reports tokens but a cost of 0; the package costs about 2.2 s per pi start with Bifrost on and nothing off.

## P4. Inertness

Setup (verified): the listener on `127.0.0.1:18788`, `pi --no-session --no-extensions -e <pkg> --list-models`, fresh agent dir per case.

| environment | requests the listener saw | bifrost rows | stderr |
|---|---|---|---|
| `BIFROST_URL`, `BIFROST_VIRTUAL_KEY`, `BIFROST_API_KEY` unset | **0** | 0 | none |
| all three set to the empty string | **0** | 0 | none |
| `BIFROST_URL=http://127.0.0.1:18788`, `BIFROST_VIRTUAL_KEY` set | **1** (`GET /openai/v1/models`, headers `accept,accept-encoding,accept-language,connection,host,sec-fetch-mode,user-agent,x-bf-vk`) | 4 | not recorded |

Further (verified, same method): `BIFROST_URL=` empty with `BIFROST_VIRTUAL_KEY` set: 0 requests, so an empty URL alone is enough and an empty `BIFROST_API_KEY` is not needed for inertness (inferred from `optionalConfigFromEnvironment`: no URL, no config). An ambient non-empty `BIFROST_API_KEY` with a URL set would be sent as `Authorization: Bearer` (inferred from `bifrostHeaders`), so a work Session should not inherit one it does not mean to send. The same inert result holds with the package loaded from settings (`{"packages":["npm:pi-bifrost-provider@0.2.0"]}`, no `-e`): unset and empty → exit 0, 0 rows, 0 stderr lines, 0 requests. Models persisted by an earlier run do not show up once the env is gone (same agent dir, env unset: 0 bifrost rows), so a personal Context does not see a work catalog. With the env empty, `--model bifrost/vertex/claude-haiku-4-5 -p ping` fails loud: `No API key found for bifrost. Use /login to log into a provider via OAuth or API key.`, exit 1.

**Worker's bash tool** (verified): haiku was asked to run `if [ -n "$BIFROST_VIRTUAL_KEY" ]; then echo VK_SET_LEN=${#BIFROST_VIRTUAL_KEY}; ...` through the bash tool; `tool_execution_end` result: `VK_SET_LEN=42\nURL_SET\n`. So a process started by pi's bash tool inherits `BIFROST_VIRTUAL_KEY` and `BIFROST_URL`: the model, any command it runs and any repo script can read the key. Risk note only; the virtual key is a work credential for tracked usage.

verdict: unset and empty `BIFROST_URL` (with the other two) both make the package inert with 0 requests, 0 output and no catalog; an empty `BIFROST_URL` alone suffices (`BIFROST_VIRTUAL_KEY` empty too is harmless); the key is visible to a Worker's bash tool.

## P5. The compat block

Setup (verified): listener at `127.0.0.1:18788` (fake SSE answer), `--model bifrost/vertex/claude-sonnet-5-5:medium -p ping`, `<agent dir>/models.json` absent versus `{"providers":{"bifrost":{"compat":{"sendSessionAffinityHeaders":true,"sessionAffinityFormat":"openrouter"}}}}`.

POST `/openai/v1/chat/completions` differences:
- headers: without the block `accept,accept-encoding,accept-language,connection,content-length,content-type,host,sec-fetch-mode,user-agent,x-bf-vk,x-stainless-arch,x-stainless-lang,x-stainless-os,x-stainless-package-version,x-stainless-retry-count,x-stainless-runtime,x-stainless-runtime-version,x-stainless-timeout`; with it the same plus **`x-session-id`**. Nothing else differs. (Header values of `x-bf-vk` and `authorization` were not logged; no `authorization` header is sent, since the package suppresses it without `BIFROST_API_KEY`.)
- body: identical, keys `max_completion_tokens,messages,model,store,stream,stream_options`, with `"model":"vertex/claude-sonnet-5-5"`, `"max_completion_tokens":128000`, `"store":false`, `"stream_options":{"include_usage":true}`; no session field in the body.

Read at all for an extension-registered provider? **verified: yes**, `x-session-id` appears only with the block. The `models.json` block needs no `baseUrl` and no `models` (that is what was run). Adding them: with `"baseUrl":"http://127.0.0.1:9/openai/v1"` the request still went to the listener on 18788 (the package's `auth.baseUrl` wins) and `x-session-id` was sent; with `"models":[{"id":"x/y"}]` the sonnet model still resolved and `x-session-id` was sent. An invalid value (`"sessionAffinityFormat":"nope"`) is accepted silently: exit 0, no warning, and no `x-session-id` (so a typo in the block fails quiet). The `x-session-id` appears with `--no-session`; the value was not logged (inferred: pi's session id).

verdict: the block is read from `models.json` for the extension-registered provider and adds one header, `x-session-id`; it needs no `baseUrl` or `models`, extra ones are ignored, and an invalid value is silently ignored.

## P6. LAN TLS

Root certificate (verified, `openssl x509 -in "$HOME/Library/Application Support/Caddy/pki/authorities/local/root.crt" -noout -subject -issuer -dates -fingerprint -sha256`):
`subject= /CN=Caddy Local Authority - 2026 ECC Root`, `issuer=` the same, `notBefore=Oct 21 18:15:16 2026 GMT`, `notAfter=Aug 29 18:15:16 2036 GMT`,
**`SHA256 Fingerprint=07:BA:A2:34:4B:9A:DB:C2:20:B9:B8:05:C7:A2:01:AB:32:93:7C:9B:78:E5:8C:6C:CC:F4:2B:84:F4:E1:5A:E5`**.

Leaf (verified): `openssl s_client -connect 192.168.3.100:8787 -CAfile <root.crt>` → `Verify return code: 0 (ok)`; chain: leaf (issuer `/CN=Caddy Local Authority - ECC Intermediate`) → `Caddy Local Authority - ECC Intermediate` (issuer `/CN=Caddy Local Authority - 2026 ECC Root`) → root. Node's view of the leaf: `valid Oct 22 10:31:23 2026 GMT -> Oct 22 22:31:23 2026 GMT`, `san IP Address:192.168.3.100`, `authorized false UNABLE_TO_GET_ISSUER_CERT_LOCALLY` without the root. The proxy serves the intermediate with the leaf, so trusting the root is enough. The leaf lives 12 h (inferred: Caddy renews it itself); do not pin the leaf.

Without the CA (verified):
- `curl -sS -m 20 -H "x-bf-vk: …" https://192.168.3.100:8787/openai/v1/models` → `curl: (60) SSL certificate problem: unable to get local issuer certificate`.
- Node: `fetch("https://192.168.3.100:8787/openai/v1/models")` → `fetch failed | cause: UNABLE_TO_GET_ISSUER_CERT_LOCALLY`.
- pi with the package: `pi-bifrost-provider: startup model discovery failed, continuing without a preloaded catalog: fetch failed`; **pi still starts** (`--list-models` exit 0). The message does not say TLS (the cause is dropped). A completion then fails: with an empty store `Error: Model "bifrost/vertex/claude-haiku-4-5" not found.` (exit 1); with a stored catalog (`NODE_USE_SYSTEM_CA=1` run below) `Connection error.` (exit 1).

With the CA (verified): **`NODE_EXTRA_CA_CERTS="$HOME/Library/Application Support/Caddy/pki/authorities/local/root.crt"`** (the environment variable of the pi process): `curl --cacert` → `200`; `pi --list-models` with `BIFROST_URL=https://192.168.3.100:8787` → exit 0, 47 bifrost rows, real 0.66 s, discovery worked (the usual limits notice, no failure); a completion `--model bifrost/vertex/claude-haiku-4-5 -p "Reply with the single word: pong"` → `pong`, exit 0, 1.6 s.

System CA store (verified on macOS): `NODE_USE_SYSTEM_CA=1` and `NODE_OPTIONS=--use-system-ca` are accepted by Node 22.22 (`node --help`: `--use-system-ca  use system's CA store`) but do **not** help here: the discovery failed the same `fetch failed` and the completion gave `Connection error.`. Reason (verified): `security find-certificate -a -c "Caddy"` lists no certificate, i.e. the Caddy root is in no macOS keychain on pro; whether the flag works once the root is installed (`caddy trust`, or `security add-trusted-cert`) was **not tested** (it would change the keychain). On Linux: not tested; **inferred** from Node's documentation that `--use-system-ca` reads the OpenSSL/distribution store (`/etc/ssl/certs`, `SSL_CERT_FILE`/`SSL_CERT_DIR`), so it works there only if the root is installed in it (`update-ca-certificates`). `NODE_EXTRA_CA_CERTS` is the same on every OS and needs only the file.

Note (verified): pi's launcher puts `~/.local/share/pi-node/current/bin` first on `PATH` when it exists; on pro it does not exist, so pi runs under `/usr/local/bin/node`. Another machine may run another Node; check its version (`--use-system-ca` needs Node 22.15 or newer, inferred from Node's release notes; the package's `engines` is `>=22.19.0`).

Needed on hs and air (not testable here, no ssh), for each machine, as the user that runs pi:
1. `curl -sS -m 10 -o /dev/null -w '%{http_code}\n' -H "x-bf-vk: $BIFROST_VIRTUAL_KEY" https://192.168.3.100:8787/openai/v1/models` reaches pro (TCP and routing on the LAN; 401 means reached, no key) and prints `200` with the key and `--cacert <root.crt>`.
2. The root certificate file exists there at a stable path, and `openssl x509 -in <file> -noout -fingerprint -sha256` prints `07:BA:A2:34:4B:9A:DB:C2:20:B9:B8:05:C7:A2:01:AB:32:93:7C:9B:78:E5:8C:6C:CC:F4:2B:84:F4:E1:5A:E5` (and the file is the root, not pro's intermediate or a leaf).
3. `node -v` of the Node pi uses there (the `pi-node` Node if `~/.local/share/pi-node/current/bin/node` exists, else `node` on `PATH`).
4. `NODE_EXTRA_CA_CERTS=<file> BIFROST_URL=https://192.168.3.100:8787 pi --no-session --no-extensions -e <pkg> --list-models` lists the bifrost rows, and a one-line `-p` completion on `bifrost/vertex/claude-haiku-4-5` answers; the same without the variable fails with `fetch failed`.
5. If the system store is wanted instead: whether `NODE_USE_SYSTEM_CA=1` works after the root is installed in the system store (macOS keychain on air, `/usr/local/share/ca-certificates` plus `update-ca-certificates` on hs).
6. The system clock on the machine (a leaf is valid 12 h; a skewed clock fails TLS).

verdict: with `NODE_EXTRA_CA_CERTS` set to Caddy's `root.crt` (fingerprint above) pi, the discovery and a completion all work over `https://192.168.3.100:8787`; without it pi starts but cannot discover or complete (`fetch failed`, which hides the TLS cause); `NODE_USE_SYSTEM_CA`/`--use-system-ca` do not work on pro because the root is in no keychain, and hs and air still need the checks above.

## P7. Failure modes

Setup (verified): `BIFROST_VIRTUAL_KEY=sk-bf-invalid` (made-up), `pi --no-session --no-extensions -e <pkg> --no-tools --no-context-files --no-skills --model bifrost/vertex/claude-haiku-4-5 -p ping`, a fresh agent dir ("no store") or one holding a catalog from a good run ("store"). Wall time measured with `date`.

| case | catalog | exit | wall | what the user sees |
|---|---|---|---|---|
| `BIFROST_URL=http://127.0.0.1:9` (closed port) | no store | 1 | 0 s | `pi-bifrost-provider: startup model discovery failed, continuing without a preloaded catalog: fetch failed`, then `Error: Model "bifrost/vertex/claude-haiku-4-5" not found. Use --list-models to see available models.` |
| closed port | store | 1 | 14 s | the same discovery line, then `Connection error.` (pi retries) |
| `BIFROST_URL=http://10.255.255.1:8788` (unreachable) | no store | 1 | 11 s | discovery `fetch failed` after about 10 s (connect timeout, shorter than the package's 15 s), then `Model ... not found` |
| unreachable host | store | 1 | 67 s | discovery `fetch failed`, then `Request timed out.` |
| live Bifrost, wrong key | no store | 1 | 0 s | `startup model discovery failed ...: Bifrost model discovery failed (401): access not found. The provided credential does not exist or has been revoked.`, then `Model ... not found` |
| live Bifrost, wrong key | store | 1 | 0 s | the same discovery line, then `401: {"message":"access not found. The provided credential does not exist or has been revoked."}` |
| `BIFROST_URL` empty (inert) | store | 1 | 0 s | `No API key found for bifrost. Use /login ...` |

Notes: pi starts in every case (the package never throws at load: discovery failures are only notices). A `--model bifrost/...` run therefore fails loud with exit 1 and never hangs forever, but the wait can be long (67 s with a stored catalog and an unreachable host, 15 s at most for discovery itself, inferred from `AbortSignal.timeout(15_000)` in index.ts; the timeout fired once on live Bifrost in the P3 title run, whose total was 40.5 s). The 401 text names the credential but not the variable, and the "not found" line does not say why discovery failed; a caller (cpd) that wants a clear error has to read pi's stderr for the discovery line or check `bifrost_url`/the key itself beforehand (inferred, not designed here). In an interactive Session the notice is queued and shown through the UI on `session_start` (inferred from index.ts; not run, no TTY).

verdict: pi always starts and a Bifrost-model run always exits 1 with a message (never a silent hang), but an unreachable host with a stored catalog takes 67 s before it fails, and the failure text for a missing catalog is the generic "Model not found".
