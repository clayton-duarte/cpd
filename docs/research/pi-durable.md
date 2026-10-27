# Pi Durable as a headless Worker runtime: research handoff

Research for a later Plan; nothing here is decided except the record cited. Written 2026-10-27 (EDT) by the `cpd` Manager from a
conversation with the User. Every fact below names its source; anything not checked says so.

## Why this exists

`pro-d109` (User, 2026-10-27 13:15 EDT): cpd is a CLI. The pi extension (the adapter, `pi/`) is one interface to cpd, like the TUI;
nothing in cpd may rely on a pi extension. Workers are never seen by the User, so a Worker runtime needs no UI. The question was
whether Pi Durable could run cpd's Workers headless, and what that would take.

## What Pi Durable is

- **A library, not an app.** `@earendil-works/pi-durable` 1.4.0 (npm, 2026-10-21) is a durable agent harness: conversations, model
  turns, tool calls and the program's own state are committed to storage before anything is shown; a process that dies mid-turn is
  picked up where it stopped when the storage is reopened (`packages/durable/README.md`, tag `v1.4.0`).
- **No CLI.** `packages/durable/package.json` has no `bin` (checked at `v1.4.0`). There is nothing to install and type.
- **Where it sits.** It replaces pi's agent loop (`pi-agent-core`), not the coding agent; like the loop it sits on `pi-ai` (Hugging
  Face video, below; matches the dependency graph).
- **Dependencies** (`npm view @earendil-works/pi-durable@1.4.0`): `pi-ai`, `chord`, `pi-codemode`, `pi-telemetry`,
  `@silvia-odwyer/photon`, `diff`, `typebox`. `pi-ai` and `chord` depend only on `pi-telemetry`; `pi-codemode` and `pi-telemetry`
  on no `@earendil-works` package. `packages/durable/src` imports neither `pi-agent-core` nor `pi-coding-agent`. So it does **not**
  need the pi agent.
- **Install line** `npm install @earendil-works/pi-durable @earendil-works/pi-ai @earendil-works/chord` (README "Installation"):
  `pi-ai` (models) and `chord` (state and context types, e.g. `BACKGROUND_CONTEXT` from `@earendil-works/chord/context`) are listed
  because the program imports them itself and Node resolves only packages the project declares; they would be installed anyway as
  dependencies.
- **Storage:** memory, JSONL (`pi-durable/storage/jsonl/node`; several files per conversation, not pi's session format), SQLite
  (`pi-durable/storage/sqlite/node`, `NodeSqliteStorage`; `storage/sqlite/cloudflare`) and IndexedDB (`storage/indexeddb`) (README
  "Storage").
- **Tools:** `@earendil-works/pi-durable/tools` gives `read`, `write`, `edit`, `bash` and the `CodingTools` extension with all four;
  they touch files and processes only through the call's environment (README "Tools").
- **Replay safety:** each tool call is its own durable task, its intent committed before `execute()`. After a crash a tool reruns
  only when declared `replay: "safe"`; otherwise the model gets an `interrupted` error result with the output so far (README "Tools").
- **It loads nothing from `~/.pi` or `.pi`:** no `AGENTS.md`, no skills, no settings; only what the program installs (Hugging Face
  video; `packages/durable/src` mentions `AGENTS.md` only in `src/testing/env-conformance.ts`, a test helper; the README never does).

### Extensions (a correction)

Pi Durable **has extensions**. An earlier claim in the conversation that it had none was wrong: it came from the README of the
deleted experimental *app* at `v1.0.0` (`packages/coding-agent/src/experimental/durable/README.md`: "Not here: sessions list and
resume picker, forks and tree navigation, extensions, prompt templates, images, `/login`."), which was about that app not loading
pi's extensions.

The library's extensions (README "## Extensions", same text at `v1.4.0` and at `b3a29f11`):
`defineExtension({ name, tools, sections, hooks, wraps, tasks })`, installed in a registry the process owns
(`createRegistry()`, `registry.install(...)`). `hooks` include `hook(ToolTask, { beforeTool: (call) => ({ block: "…" } | undefined) })`.
Every conversation selects every installed extension by default, in install order; conversations store extension names, the registry
stores nothing. Extensions may keep their own durable state (examples 11, 27, 30, 31).

It is a **different API** from pi coding agent's `ExtensionAPI`: cpd's adapter cannot be loaded into it as is.

### Interfaces (a correction)

"Pi Durable has no interface" was too strong. Accurate: it ships no runnable interface; it has an interface API.

- `viewState()` (1.4.0) / `watch()` (after 1.4.0, see below) give a client the conversation's current view, then each change;
  `watchEvents()` turns commits into the coding agent's familiar events; any client can `submit` input with `whenBusy: "steer"`
  (README; blog post `pi-durable.md`).
- Built and **removed**: a TUI coding agent and a vacation-planner TUI at `v1.0.0`
  (`packages/coding-agent/src/experimental/{durable,vacation}`), and the client/server stack (`@earendil-works/pi-client`,
  `pi-server`, `pi-protocol` 1.1.0 on npm, 2026-10-07). All deleted in `7472cda0` (2026-10-12): "remove the experimental server stack
  and the client, protocol, and server packages". On `main`, `packages/coding-agent/src/experimental/` holds only `source-resolver.ts`.
- Exists: a multiplayer React web chat on Cloudflare Durable Objects (blog post `pi-durable-on-cloudflare.md`, repo
  `earendil-works/pi-durable-cloudflare`); 32 examples in `packages/durable/test/examples` (`00-conversation.ts` …
  `31-reload-and-restart.ts`), incl. `18-print.ts`, `19-json.ts`, `26-coding-agent.ts`.
- `pi-coding-agent` 1.4.0 does not depend on it.

### After 1.4.0 (`main` at `b3a29f11`, 2026-10-27 16:57 UTC)

README diff against `v1.4.0`:
- **Read-only watch from another process:** `readOnly: true` opens a session another process drives; it recovers nothing and every
  write, `resume()` and task-running call rejects with `ReadOnlySession`. cpd could watch a Worker without owning it.
- **Crash handling:** a model request the host died during is a failed attempt under `settings.retry` (backoff; with none left the run
  ends `runFailure` reason `interrupted`). Other work counts restarts; at `settings.restartLimit` (default 5) it ends (tool error
  result `restart_limit`, run `runFailure` `restart_limit`, own tasks `faulted`) instead of being parked; reports `task_interrupted`.
- **API change:** `viewState()` becomes `watch()`.

## Sources

- Repo `earendil-works/pi`: tag `v1.4.0` (`4a08025`, cloned at `/tmp/pi-src` during the research, not kept), `main` at `b3a29f11`
  and later; `packages/durable/README.md`, `packages/durable/package.json`, `packages/durable/test/examples/`.
- Blog: `earendil-works/website` `posts/pi-durable.md` and `posts/pi-durable-on-cloudflare.md` (2026-10-16).
- npm: `npm view @earendil-works/<pkg>@1.4.0 dependencies peerDependencies`.
- Video, Syntax, "You need to check out Pi Durable" (2026-10-16, 15 min, https://youtu.be/BLRReTwV8Fc): a Sentry webhook starts an
  agent that triages, finds the root cause, fixes, tests, opens a PR and resolves the issue; stopped mid-fix and resumed from the same
  step; per-tool rerun safety; every screen is a client (phone, browser, Slack, terminal), multiplayer; message the same agent later
  with its full history; ~8,000 lines of the presenter's own agent code deleted; runs on Node + SQLite, browser/IndexedDB, Android,
  Cloudflare. Mentions OpenCode 2 and Cloudflare's Agent SDK running Pi Durable (unverified).
- Video, Hugging Face, "Pi Durable EXPLAINED: Build Always-On Agents with Local Models" (2026-10-19, 30 min,
  https://www.youtube.com/watch?v=ti3vVYwzW4A): crash mid-tool-call: pi writes an error result and waits for "continue", Pi Durable
  resumes from its checkpoints; not an app, a building block at the agent-loop level on `pi-ai`; steps: models, registry
  (`CodingTools`), storage, open the harness, create/reopen a conversation, submit (await or stream); demo on Qwen3.5-0.8B via
  llama.cpp, stopped after a `read` and resumed; it does not load `AGENTS.md` or `.pi`.
- Transcripts were pulled with `uvx yt-dlp --skip-download --write-auto-subs`.

## What ties a cpd Worker to pi and tmux today (cpd `origin/main` `ebe50dd5`)

The adapter already talks to cpd only through the CLI (`cpd events follow`, `cpd session state`); the coupling is mostly cpd's side.

1. **Launch:** `core/src/host/pi/launch.rs` `fn worker` starts `pi --approve --no-skills --no-mcp --exclude-tools … -e <adapter>`
   in a tmux window, with the §6.5 priority prefix, `CPD_HOME`, `CPD_SESSION`, `CPD_PLAN`, `CPD_ATTEMPT`, `PI_SKIP_VERSION_CHECK=1`
   and `$CPD_HOME/bin` (the `git` wrapper) on `PATH` (contract §6.6).
2. **End of the Attempt (A5):** the driver reads tmux pane death and pi's session file: the last assistant `stopReason`,
   `errorMessage` and the turn count (`core/src/driver/run.rs` `judge`, ~l.105-125) for `crashed`, `provider_error`, `no_submit` and
   the nudge baseline (contract §3.4).
3. **Idle and nudges:** the adapter reports idle (`pi/lib/agent-state.ts`); nudges and Escalation answers reach the Worker through the
   adapter's follower (`pi/lib/follower.ts`, `pi/lib/delivery.ts`).
4. **Guards** in the adapter (`pi/lib/adapter.ts` `tool_call`): unfiltered test runs (`testguard.ts`), primary-checkout writes
   (`mainguard.ts`), secret bypass (`secretguard.ts`), host-approve bypass (`hostapprove.ts`). The `git stash` refusal is the `PATH`
   wrapper, already runner-neutral.
5. **Prompt sections** from the adapter: policy (`pi/lib/policy.ts`) and time zone (`pi/lib/time.ts`).
6. **Axioms:** pi loads `~/.pi/agent/AGENTS.md` for every Session cpd starts (hs-d611). Pi Durable would not.
7. **Metrics (A9):** parsed from pi's session file.
8. **End (A9):** kills the tmux window.
9. **Review Jobs (B2):** the Worker is a fork of the Lead's pi session file (`--fork`).

## What would have to be built (Workers only)

**In cpd (Rust): a runner-neutral Worker contract**
- The driver starts the Worker as its own child process instead of a tmux window, and reads its exit itself.
- The runner reports its end (stop reason, provider error or not, turns, usage), e.g. a field on `cpd session state` or a result file
  (to decide). A5 and A9 read that, not pi's session file.
- The pi Worker reports the same way through the adapter, so both runners share one contract and the pi Worker keeps working.

**The Pi Durable runner (Node; guess: a few hundred lines)**
- Opens a Harness with `NodeSqliteStorage` under `run/<plan>/…`, installs `CodingTools`.
- One cpd extension: `section`s for the axioms (read `~/.pi/agent/AGENTS.md`), policy and time zone; `beforeTool` hooks reusing the
  pure guard functions from `pi/lib`; a follower on `cpd events follow` submitting nudges and Escalation answers as input; state via
  `cpd session state`; exit code plus the end report.
- Policy chooses the runner per Worker.

**Open questions for the User**
- **Review Jobs:** Pi Durable cannot fork a pi session file. Keep review Jobs on pi, or start them from the Spec plus the diff.
- **Model login:** Pi Durable calls models through `pi-ai`; unverified whether it can use pi's `auth.json` subscription logins or
  needs API keys.
- **Crash resume:** the driver could restart the runner (Pi Durable resumes) instead of ending the Attempt `crashed` and starting a
  fresh Worker. Both videos name this as the main gain; optional, a second step.
- **Watching a Worker:** no pane. A `readOnly` watch could come later; `pro-d109` says it is not needed.

**Order:** the runner-neutral contract first (the pi Worker behind it), then the Pi Durable runner as a policy choice.

## Where the conversation stopped

The User was asked (not answered) which to start: (a) two Plans in order, the runner-neutral contract then a Pi Durable Worker;
(b) an experiment first, a Pi Durable Worker outside cpd on a few real Specs; (c) only the runner-neutral contract.
