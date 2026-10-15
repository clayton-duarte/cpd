# t6: A supported source for each model's valid thinking levels

Tier: H2 · Report: `~/projects/cpd/docs/trials/t6-tier-catalog.md` · Budget: 25 minutes · Read `_common.md` first (herdr is not needed for this trial).

## Goal

`cpd` must reject invalid Tiers because pi silently clamps unsupported thinking levels. Find the most stable way for a TypeScript (Node 22) `cpd` CLI to read, for each model, which pi thinking levels are valid. Deliver a small generator that reproduces this table:

| Tier letter | Model | Valid efforts (0=off, 1=low, 2=medium, 3=high, 4=xhigh, 5=max) |
|---|---|---|
| O | `github-copilot/claude-opus-5.5` | 1–5 |
| S | `github-copilot/claude-sonnet-5.5` | 0–5 |
| H | `github-copilot/claude-haiku-4.5` | 0–3 |

The table above was derived by the Lead from `~/.pi/agent/models-store.json` (`thinkingLevelMap`) plus `getSupportedThinkingLevels` in `~/.pi/agent/install/releases/1.1.0/node_modules/@earendil-works/pi-ai/dist/models.js` (around lines 680–708). pi's `minimal` level is deliberately unused.

## Candidates to evaluate

1. A **public export** of `@earendil-works/pi-ai` or `@earendil-works/pi-coding-agent` (e.g. `getSupportedThinkingLevels`, a model registry or `ModelRuntime`) imported from the installed pi tree by a Node script. Check the package `exports` maps and the docs (`docs/models.md`, `docs/sdk.md`).
2. The **pi CLI** (`pi --list-models`, any JSON output mode, RPC commands in `docs/rpc-commands.md` such as model listing or available thinking levels).
3. **Reading `~/.pi/agent/models-store.json`** and reimplementing the rule (internal file).

## Acceptance criteria

1. **must** For each candidate: does it work today (command and output excerpt), is it a documented or public interface, and how likely is it to break on `pi update`?
2. **must** A working generator `/tmp/cpd-trials/t6/tiers.mjs` (Node 22, no dependencies outside the pi install) that prints the table above as JSON `{ "O": {"model": "...", "efforts": [1,2,3,4,5]}, ... }` using the recommended candidate. Paste its source and output in the report.
3. **must** Show the generator failing loudly (non-zero exit, clear message) when a configured model id is not in pi's catalog.
4. **should** How the generator locates the pi install robustly (it moves between releases: `~/.pi/agent/install/releases/<version>/…`).
