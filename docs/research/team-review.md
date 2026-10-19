# Team review Inbox: reading the Slack channel through pi's MCP client

Plan `inbox`, Job `team-research`, Attempt 1. Context decision "Team review Inbox" (User, 2026-10-19): the Inbox's review item opens only for pull requests posted in Slack **#team-private-external-experience** (`C02JU6C2Z1D`) that hermes' cockpit classifies `NEEDS_REVIEW`. This note answers one question, how cpd reads that channel without an HTTP client in Rust (hs-d107) and without handling the credential, and records what was verified and what was not. The rule itself is in the contract (§2.2 "Inbox item", §3.7 E10, §8.2) and in commands.md §21.5.

Paths below are relative to `/home/clayton/.pi/agent/install/releases/1.3.0/node_modules/@earendil-works/` (pi 1.3.0, the installed release). `dist/` is the compiled JavaScript, whose line numbers are cited.

## 1. Evidence: what pi 1.3.0 offers

### 1.1 pi has its own MCP client, not the official SDK

- `pi-mcp` is "a small, standalone Model Context Protocol client. It does not depend on the official MCP SDK or other pi packages" (`pi-mcp/README.md:3`). The brief's "MCP SDK pi bundles" is therefore `@earendil-works/pi-mcp` (`McpClient`, `StreamableHttpTransport`, `pi-mcp/dist/index.d.ts:2,7`), and its OAuth subset `@earendil-works/pi-mcp/oauth` (`pi-mcp/package.json:9-21` exports `.`, `./oauth`, `./testing`).
- pi's built-in MCP extension wraps it in `pi-coding-agent/dist/extensions/mcp/`: `config.js` (reads `mcp.json`), `oauth.js` (token store and refresh), `runtime.js` (`McpServerConnection`), `cli.js` (`pi mcp …`).
- Tokens: `~/.pi/agent/mcp-auth.json`, keyed `mcp__<server>|<url>` (`extensions/mcp/oauth.js:78-81`, file chosen at `oauth.js:88`); pi "refreshes access tokens when they expire or the server rejects them" (`docs/mcp.md:129`).

### 1.2 Where pi refreshes, and who may call it

- `McpOAuthCredentialStore` (`extensions/mcp/oauth.js:83`) owns the file; its `forServer(name, url)` returns the `load`/`save`/`withRefreshLock` triple (`oauth.js:91-108`). The refresh lock is a lock file per server in the agent directory, so a second process (a pi session, this helper) never spends a rotated refresh token twice (`oauth.js:109-136`, `213-224` comment).
- `createMcpAuthProvider` (`oauth.js:225`): `token()` sends the stored access token and refreshes it when it is within 30 s of `tokensExpireAt` (`oauth.js:260-270`); `onUnauthorized()` refreshes after a 401 (`oauth.js:271-281`). It never opens a browser: when a sign-in is needed it throws `McpOAuthAuthorizationRequiredError` (`oauth.js:4-8`, `239`, `252`).
- `McpServerConnection` (`extensions/mcp/runtime.js:87`) builds that provider from the credential store given to its constructor (`runtime.js:113-134`; options typed at `runtime.d.ts:49-61`), connects lazily, `callTool(name, args, options)` (`runtime.js:176`), reconnects once on an expired session (`runtime.js:198-232`), turns "needs sign-in" into the state `needs-auth` and the message `MCP server "<name>" requires sign-in. Run /mcp to sign in.` (`runtime.js:31-34`, `241-257`, `332-336`), and `close()` waits for a refresh that the server already answered, so a rotated token is saved (`runtime.js:366-375`).
- `loadMcpConfig({agentDir, cwd, projectTrusted})` reads `<agentDir>/mcp.json` (`extensions/mcp/config.js:105-117`); with `projectTrusted: false` the project file is ignored.
- `getAgentDir()` honours `PI_CODING_AGENT_DIR` (`dist/config.js:476`, `491-497`).
- `pi mcp list|login|logout|add|remove` is the CLI (`docs/cli.md:324-329`). It has **no command that calls a tool**. `pi mcp list --json` connects and reports `state: "needs-auth"` (`extensions/mcp/cli.js:347-389`), which is a probe, not a read. `pi mcp`'s own code does exactly the construction a helper needs: `createConnection` (`cli.js:67-76`: `new McpServerConnection({entry, cwd, createTransport: createDefaultTransport, credentials, log, onTools})`).

### 1.3 What is exported, and what is not

- `pi-coding-agent/package.json:14-22`: the package's `exports` are `.` and `./rpc-entry` only. `dist/index.d.ts:32-33` exports `createMcpExtension`, `McpExtensionOptions` (which has `credentials?: McpOAuthCredentialStore`, `extensions/mcp/index.d.ts:39`) and the types `McpServerConfig`, `McpServerEntry`, `LoadedMcpConfig`, but **not** `McpOAuthCredentialStore`, `McpServerConnection`, `createDefaultTransport` or `loadMcpConfig`.
- Run from this worktree: `import("@earendil-works/pi-coding-agent/dist/extensions/mcp/runtime.js")` fails with `ERR_PACKAGE_PATH_NOT_EXPORTED`. **There is no supported way, outside a pi session, to reach pi's token store or its connection class.**
- What is supported: (a) an SDK session with `createMcpExtension()` (`docs/sdk.md:114-116`), where a tool is called by the model or a codemode script, which needs an LLM turn (rejected below); (b) `@earendil-works/pi-mcp` and `/oauth`, whose store the caller supplies ("The package does not open a browser or choose where credentials are stored", `pi-mcp/README.md:110`): that means our own code reading `mcp-auth.json` and re-implementing the refresh lock, i.e. handling the credential (rejected below).

## 2. The mechanism chosen

**A small helper in cpd's `pi/` package, run by cpd as a subprocess, that imports pi's own modules by absolute file URL from the installed release and calls the tool through pi's own `McpServerConnection` and `McpOAuthCredentialStore`.** A file URL bypasses the `exports` map. cpd does this already: `core/src/config/catalog.mjs` imports pi's `dist/index.js` by `pathToFileURL(<entry from locate_pi>)` and runs under pi's Node (contract §9, `core/src/config/catalog.rs` `locate_pi`, `node`).

What the helper imports (all under `<coding-agent>/dist/`, the entry path coming from `locate_pi`):

| Import | From | Used for |
|---|---|---|
| `loadMcpConfig` | `extensions/mcp/config.js` | the `slack` entry of `<agentDir>/mcp.json` (global only: `projectTrusted: false`, so a repository cannot redirect the credential) |
| `McpServerConnection`, `McpOAuthCredentialStore`, `createDefaultTransport` | `extensions/mcp/runtime.js` | connect, refresh, `callTool`, `close` |
| (env) `PI_CODING_AGENT_DIR` | `config.js` | which agent directory pi's modules use: set to the one `locate_pi` found |

The helper never reads `mcp-auth.json` and never sees a token: the credential store reads and refreshes it inside pi's code, as `slack_fetch.py:21-28` did through `HermesTokenStorage`.

### 2.1 Verified, and how

A scratch script (not committed; it lived in `/tmp`) did the above against a local fake streamable-HTTP MCP server that answers 401 unless `Authorization: Bearer good`, with `PI_CODING_AGENT_DIR` pointing at a temporary agent directory holding a made-up `mcp.json` and `mcp-auth.json` (no real credential; the temporary files are removed):

| Case | Result |
|---|---|
| no `mcp-auth.json` | `state: needs-auth`, message `MCP server "fake" requires sign-in. Run /mcp to sign in.`, exit 3 |
| stored token `good` | `state: connected`; `callTool("slack_read_channel", {channel_id, limit})` returned the server's text content |
| stored token `stale` (rejected, no refresh token) | `needs-auth`, same message |
| stored token with `tokensExpireAt` in the past, no refresh token | the old token is sent (pi's "failures fall through", `oauth.js:268`) and works |
| no `fake` entry in `mcp.json` | helper's own `no_server` |
| bare-specifier deep import from the worktree | `ERR_PACKAGE_PATH_NOT_EXPORTED` |

So the import route, the `needs-auth` detection and the call work with pi 1.3.0's modules; the helper is independent of the entry's `exposure` (`runtime.js` never reads it).

### 2.2 Not verified (labelled)

- **A refresh against a real authorization server.** I read the path (`oauth.js:229-258`, `authorizeMcp` in `pi-mcp/dist/oauth/flow.js`) but did not run it: no stored Slack credential exists on this machine (`~/.pi/agent/mcp.json` and `mcp-auth.json` are absent) and the Job forbids touching one.
- **That Slack's MCP server accepts pi as an OAuth client.** `pi mcp login slack` registers dynamically as `pi` (`docs/mcp.md:150`). Some servers refuse unknown clients; pi's escape hatches are `oauth.clientId`/`clientSecret`/`callbackPort` and `clientName` (`docs/mcp.md:133-160`; `pi mcp add … --oauth-client-id …`, `docs/cli.md:325`). I could not try it without the User's browser sign-in: **guess: dynamic registration works as it did for Hermes' `slack` entry; if `pi mcp login slack` fails, the User adds `--oauth-client-id`.**
- **The tool's real output shape.** Everything about `slack_read_channel`'s text (`=== Message from <name> (<id>) ===`, `Message TS: <ts>`, `Reactions: eyes (1), …`, `<@U…|Name>` mentions, the `{"result": …}` envelope that is sometimes truncated) is taken from hermes' parsers (`refresh.py`, `classify.py`), not from a live call.

## 3. What the User runs once

```
pi mcp add slack --url https://mcp.slack.com/mcp --exposure hidden
pi mcp login slack
pi mcp list            # slack: connected
```

`--exposure hidden` (`docs/cli.md:331`, `docs/mcp.md:198`): the entry lives in the user-level `~/.pi/agent/mcp.json`, which every pi session reads, so without it every Lead and Worker would get the Slack tools (including any send tool) reachable from codemode. The helper does not need the tools exposed. Whether a hidden server still connects in every session is unverified (`docs/mcp.md:100` says enabled servers connect; the cost is one idle connection). `pi mcp login` needs a browser, so it is the User's step, once; cpd never runs it.

## 4. Rejected mechanisms

| Option | Why not |
|---|---|
| An LLM turn in a pi session calling the tool (`pi -p`, SDK session + `createMcpExtension`) | The failure hermes already measured: 160 s, and a model that twice returned prose about the payload, which parsed to zero PRs and rendered as an empty queue (`slack_fetch.py:5-15`). A data fetch is not a model's job |
| `@earendil-works/pi-mcp` + our own `McpOAuthStateStore` over `mcp-auth.json` | Our code would read and write the credential and copy the refresh lock logic (`oauth.js:109-136`); the Job's contract forbids handling it |
| `pi mcp list --json` | Probe only: no call, and it prints tool names, not data |
| An HTTP client in Rust with the token | hs-d107; and it would read the credential |

## 5. What breaks on a pi upgrade

The helper depends on four **internal** facts of pi 1.3.0, none of them covered by pi's semver promise: the module paths `dist/extensions/mcp/{config,runtime}.js`; the names `loadMcpConfig`, `McpServerConnection`, `McpOAuthCredentialStore`, `createDefaultTransport`; the constructor's option shape (`runtime.d.ts:49-61`); and `PI_CODING_AGENT_DIR`. cpd already depends on pi's internals the same way (`catalog.mjs` calls `ModelRuntime.create`). On a break the helper's `import` or constructor throws; it reports `slack_helper_failed` (exit 1, the thrown message in `details`), the engine step reports an `error` and the Inbox shows `engine E10: slack_helper_failed` (red), and the last good `run/team-review.json` stays. It is never silent. Two guards for the code Jobs: a test that imports the four names from the pinned devDependency `@earendil-works/pi-coding-agent` (`package.json`, 1.3.0) by the same file URL, so a bump of the pin fails the test before an install does; and the pi-reload rule (`docs/research/pi-reload.md`) for when pi itself changes.

## 6. hermes rules ported, with their source

hermes-config `plugins/cockpit/` (read from a clone at the time of writing). The contract and commands.md cite these per rule; this table is the index.

| Rule | hermes source |
|---|---|
| Direct MCP call: tool `slack_read_channel` `{channel_id, limit}`, no LLM; text = concatenated `text` blocks; `isError` and empty text are failures | `slack_fetch.py` `_call_tool` 97-161, `_result_text` 164-170, `_read_channel` 173-180 |
| Whole-call timeout 45 s; failures raise, never return empty | `slack_fetch.py` `fetch_channel_text` 183-201 |
| Auth-looking failure says "run login" | `slack_fetch.py` `format_mcp_error` 81-94, `_AUTH_MARKERS` 55; missing token 127-132 |
| Channel `C02JU6C2Z1D`, server `slack`, `https://mcp.slack.com/mcp` | `slack_fetch.py` 40-42 |
| `MSG_LIMIT` 200 | `refresh.py` 56 |
| Validate that the payload **parses into messages** before the cache is replaced; otherwise keep the last good copy and fail | `refresh.py` `fetch_slack` 59-93 (`unwrap(text).count("=== Message from ") < 1`) |
| `unwrap`, `_unescape` (the truncated or re-escaped envelope) | `refresh.py` 96-142 |
| `parse_messages`: split on `=== Message from `, `Message TS: `, cutoff, poster = text before the first `(` and `<`, `extract_prs`, newest posting of a PR wins, **primary** = the first PR URL of the post, `reactions` | `refresh.py` 145-194 |
| 7-day window: `--since-hours` defaults to `RETAIN_DAYS * 24` | `refresh.py` `RETAIN_DAYS` 496, `main` 626-641 |
| Retention: a PR that scrolled out of the read keeps its last row until its `ts` is 7 days old; unresolved carried rows are re-checked on GitHub, `DONE` is terminal | `refresh.py` `retain` 547-583, `_refresh_retained` 499-545 |
| `extract_prs` (`PR_URL_RE`, trailing `.,)>|` stripped), `parse_reactions` (`Reactions: eyes (1), …`) | `classify.py` 74-75, 105-127 |
| `slack_claim`: `CLAIM_REACTIONS = {"eyes"}` claims; a mention of someone else claims ("assigned to someone in Slack"); a mention of the User withdraws the claim; the app footer `*Sent using* <@U…|Name>` is stripped first | `classify.py` `CLAIM_REACTIONS` 72, `APP_ATTRIBUTION_RE` 82-84, `mentions_me` 130-138, `slack_claim` 141-174; `SLACK_ME_ID` 55 |
| `classify`: draft → hidden; own PR → hidden; merged/closed → `DONE`; `APPROVED` → `DONE`; `CHANGES_REQUESTED` → its own status; a human (not in `BOT_REVIEWERS`, not `ME`) in `latestReviews` → `IN_REVIEW`; bots only → `NEEDS_REVIEW` `AI review only`; none → `NEEDS_REVIEW` `no review yet` | `classify.py` `classify` 264-341, `BOT_REVIEWERS` 58-62, `ME` 50, `bot_reviewers` 344-358 |
| Slack overrides after `classify`: `NEEDS_REVIEW` + claimed → `IN_REVIEW`, unless the User's own `:eyes:` is the only claim (`NEEDS_REVIEW` "you claimed this"); a mention of the User in the thread → `NEEDS_REVIEW` "you were tagged in the thread" | `refresh.py` `build` 435-462, `claimed_by_me` 251-281, `thread_replies_text` 320-331; `slack_fetch.py` `thread_text` 211-231, `reaction_users` 324-369 |
| A PR whose GitHub state cannot be read is dropped from this run, and a run where **every** PR is unreadable refuses to replace the queue | `refresh.py` `build` 409-418, `main` 670-683 |
| Only `NEEDS_REVIEW` is Clayton's to act on | `classify.py` docstring 28-34 |

**Not ported (writes and UI):** `post_pr.py`, `approve.py`, `request_changes.py`, `claim.py`, `add_reaction`, `post_message` and `mark_merged` write to Slack or GitHub; the Inbox only reads. `verdict.py` (AI verdicts), the pane and `storage.py` (hermes' own store) are not the Inbox's.

**Deviations from hermes, each deliberate:**
1. `ME` (the User's GitHub login) is read from GitHub (`viewer { login }`) instead of being a constant; `SLACK_ME_ID`, the channel and the bot logins are config keys (`inbox.slack_user_id`, `inbox.team_channel`, `inbox.bot_reviewers`, contract §8.2), so nothing about one person is in the code. Only the bot list has hermes' values as its default; the channel and the user id are empty, which switches the item off until the User sets them.
2. Who reacted: hermes matches the reactor's display name against `"clayton duarte"`/`"clayton"` after stripping the `(U…)` id (`refresh.py` 248, `slack_fetch.py` 361). The helper matches the **Slack user id** in that parenthesis (the shape in `slack_fetch.py:353`: `:eyes: × 2 — Ryan Case (U039194B7D2), clayton (U04DB4MTUMR)`), against `inbox.slack_user_id`: one identity instead of three spellings. Unverified against the live server.

## 7. Open questions for the Lead

See the end of the Job's final message; repeated here so they travel with the file.

1. **Where the state lives.** The contract keeps the classified posts in `run/team-review.json` (a derived cache like `run/context-digests/`, §1.4), not in a table: a table is a migration (§4.4/§4.5) the Job's file list does not cover. Its cost: E8 reads a file, and a missing file means "unknown", not "empty". Say if you want a table (`team_posts`) instead.
2. **`pull_requests.review_requested` and the second search alias** are read by nothing after this change. The contract says so and leaves them; deleting them is a migration and a poll change for a code Job.
3. **Thread and reaction reads.** hermes' two Slack overrides cost extra calls per refresh: `slack_read_thread` for every distinct post in the 7-day window (`refresh.py` 334-362: "EVERY ref … with no caching", 8 in parallel) and `slack_get_reactions` for claimed posts. At a 5-minute cadence that is the heaviest Slack load of the design; the Job's brief names neither. The contract ports both (`thread_tags_me`, `eyes_me`); say if they should be cut.
4. **Engine stall.** E10 is synchronous like E5 (contract §3.7), up to its 120 s deadline; a detached run like E9 would not hold the tick but needs its own error path.
5. **Slack accepts pi as an OAuth client?** (§2.2.) First `pi mcp login slack` is the test.
6. **Cadence.** `watch.team_review_seconds` defaults to 300 as the brief proposes; hermes' code says "five-minute poll" (`refresh.py:191`) but its cron job ran every 15 minutes (`cron-jobs.json` backup of 2026-10-15). The Slack load of a 5-minute poll (the channel, every thread, one GitHub call) is unmeasured.
7. **hs id** of the decision "Team review Inbox": the contract cites it by name; fill in the `hs-d` number.
