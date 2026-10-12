# W4 (review, diff, git/worktree and PR plugins): research report

Read at: 2026-10-12 (herdr source `2563803`, version 0.9.3; pi docs 1.1.0). Items reviewed: 15 listed (14 repos, `worktrunk` counted with `herdr-worktrunk`, `pi-simplify` and `pi-lens` counted separately) plus 12 additional finds. Items not reviewed: none from the cluster list. Not read to the bottom: the 23k-line `herdr-plugin-odysseus` (manifest, README, GitHub-feedback code only), `plannotator` browser UI internals, `pi-lens` internals beyond guards.

All clones are in `/tmp/cpd-research/repos/<owner>__<repo>`; SHAs are given per item. Nothing was installed or run; every claim below is from source, manifests, docs or the GitHub/npm API, and inferences are labelled.

## Summary

### Best candidate per need

| Need | Best item | Coverage | Notes |
|---|---|---|---|
| N4 Reviewer as read-only fork | none in this cluster | none | Every review plugin here is a human-driven UI. Reviewer fork stays with pi-herdr-subagents (`session-mode: fork`, `tools`/`deny-tools`). |
| N9 Worktree per Attempt + setup | herdr built-in `worktree create --branch --base` + `lamngockhuong/herdr-worktree-setup` (on `worktree.created`) | full (with a race, see Gaps) | Built-in creates via `git worktree add -b <branch> <path> <base>` (`/tmp/herdr-src/src/worktree.rs:239-258`) and emits `worktree.created` (`src/app/api/worktrees/deferred.rs:502`). Setup config lives in the repo (`.herdr-worktree.toml`) and has a trust gate. `tdi/herdr-worktree-setup` is the simpler alternative, but its config is global per machine, not per repo. |
| N10 Merge into Lead Branch, keep commits, serialised | `max-sixty/worktrunk` CLI (`wt merge <lead-branch> --branch <job-branch> --no-squash --no-remove`) | partial | Fast-forward-only with a compare-and-swap ref update, so concurrent merges fail cleanly instead of corrupting; no queue, so CPD serialises. Rebases the Job branch onto the target first (new SHAs, same commit messages), unless `--no-rebase`. The herdr plugin wrapper (`herdr-worktrunk`) is interactive-only; call `wt` directly. |
| N11 Mechanical Gate | worktrunk `[[pre-merge]]` hooks in `.config/wt.toml` | partial | Runs after rebase, before the merge, in the Job worktree; failure aborts the merge. It could run the repo `verify` script, so the Gate is tested on the rebased result. Project hooks need first-run approval (`--yes` bypasses). |
| N12 PR feedback loop | none ready-made. Borrow `gh pr view --json statusCheckRollup,reviews,comments` + `gh api .../pulls/<n>/comments` from `jpolec/herdr-plugin-odysseus` (`src/github/mod.rs:161-215`); `jsmenzies/mergr` for the startup-daemon poller pattern | borrow only | `wyattjoh/herdr-plugin-gh-pr` and `mergr` only paint sidebar labels. Nothing turns CI failures into Jobs. |
| N14 Plan approval UI | `backnotprop/plannotator` pi extension (`@plannotator/pi-extension`) | full for plan approval; no structured questions | Non-blocking: submit returns at once, the decision arrives later as a follow-up user message and as a `plannotator:review-result` event. Works without its plan mode (`plan-review` request needs only `planContent`). |
| N15 Visibility | `jsmenzies/mergr`, `wyattjoh/herdr-plugin-gh-pr` (PR/CI tokens); `minhtran3124/herdr-worktree-agents` (worktree board) | partial | Optional, display only. |
| N19 Building blocks | worktrunk hook templates; `tdi/herdr-worktree-from-linear` `lib/worktree.js` (verified create-vs-open semantics); `jhochenbaum/herdr-hunk-diff` `src/herdr.ts` (a typed `herdr` CLI adapter); `persiyanov/herdr-reviewr` `src/herdr_socket.rs` (socket subscription handling) | n/a | See items. |
| N1, N2, N3, N5–N8, N13, N16–N18 | not covered by this cluster | none | |

### Overlaps and conflicts

- **`worktree.created` consumers.** Five plugins hook it, and the hooks all run: `tdi/worktree-setup`, `lamngockhuong/worktree-setup`, `razajamil/workspace-manager` (applies a layout, optionally starting an agent), `persiyanov/reviewr` (auto-opens a pane, `auto_open` can disable it), `wyattjoh/gh-pr` (label). Installing more than one setup plugin would run setup twice. Per-plugin enable/disable exists (`plugin.enable`/`plugin.disable`).
- **Event hooks are not ordered with agent start.** Event hooks are separate processes. The docs do not say whether `worktree create` waits for them; I infer not, because `tdi/worktree-setup` publishes a `setup: running|done|failed` workspace token precisely so a user can see setup still running (`src/setup.js:7-27`). If CPD starts the Worker in the new pane immediately, deps may not be installed yet. Fix: CPD runs the repo setup itself (or waits on the token) before starting the Worker. Unverified for 0.9.3: whether the response waits for hooks.
- **`wyattjoh/herdr-plugin-renamer` renames branches that start with `worktree/`.** It renames branch and workspace of auto-named worktrees from the agent's first prompt (README "What it renames"). CPD passes explicit `--branch`, so it should not trigger, but avoid `worktree/` as a CPD branch prefix if this plugin is installed.
- **`pi-lens` git-guard and read-guard** intercept `tool_call` (`clients/runtime-tool-call.ts`, `clients/git-guard.ts`): they hold `git commit`/`push` while findings are unresolved and block edits without a prior read. This can stall a Worker's commit. Plan accordingly if installed.
- **`plannotator` plan mode** calls `pi.setModel`, `pi.setThinkingLevel` and `pi.setActiveTools` on phase changes (`apps/pi-extension/index.ts:684,696,746-759`). It would fight the Lead's Tier if plan mode is on. Use only its event API.
- **Hook-ownership claim.** `razajamil/workspace-manager` says UI-created worktrees "historically" emitted neither `worktree.created` nor `workspace.created`. In 0.9.3 source, `worktree.created` is emitted from the deferred create path (`deferred.rs:502`), and CHANGELOG:526 says plugin/UI flows preserve lifecycle events (#657, #662, #686). So the claim looks outdated, but I did not test the UI path.

### Recommendation for CPD v0

1. **N9: use herdr built-in `worktree create --branch job-N --base <lead-branch>` (`--json`).** It does exactly the base-ref control CPD needs. If the branch exists locally it is checked out and `--base` is ignored (docs `cli-reference.mdx:182`; confirmed independently in `tdi/herdr-worktree-from-linear` `lib/worktree.js:19-45` comment "verified live on herdr 0.7.1"). For a fresh Attempt use a fresh branch name; `worktree open --branch` fails if no worktree exists.
2. **Per-repo setup: reuse `lamngockhuong/herdr-worktree-setup`** (config in-repo, `post_create`, template vars, teardown on `worktree.removed`). The trust file (`trusted-repos.txt` with a command hash) must be pre-seeded by CPD or the machine owner, or `post_create` is silently skipped (toast only). Alternative if the in-repo config is unwanted: `tdi/herdr-worktree-setup`, which has no licence file. Either way, do not let the Worker start until setup finishes (see conflicts above).
3. **N10/N11: use the `wt` CLI directly, not `herdr-worktrunk`.** Proposed step for the Lead (a script, no LLM): `wt merge <lead-branch> --branch <job-branch> --no-squash --no-remove --yes -C <lead-worktree>` with the repo's `.config/wt.toml` `[[pre-merge]] verify = "<repo verify script>"`. Wrap it in a per-Workflow file lock so merges are serial. Spike first: confirm `--no-squash` keeps commits after the rebase, confirm the target worktree updates, and check what happens when the Lead worktree has uncommitted changes (`push.rs` refuses a dirty overlapping path).
4. **If `wt` is rejected as a dependency**, the fallback is plain git: `git rebase <lead>` in the Job worktree, run `verify`, then `git merge --ff-only` in the Lead worktree under a lock. That is about 15 lines and reimplements what worktrunk already does; worktrunk wins on "never vendor what upstream owns" as long as the spike passes.
5. **N14: reuse `@plannotator/pi-extension` for plan approval.** Have the Lead's own CPD extension emit `plannotator:request` `{action:"plan-review", payload:{planContent}}` and listen for `plannotator:review-result` (README "Shared Plannotator event API"). The Plan phase's Job graph must be rendered as markdown for this. It does not give structured question forms, so N14's "structured questions" still needs another mechanism.
6. **N4: nothing here helps.** Do not build the Reviewer on `reviewr`, `hunk-diff` or `annotate`. They are human tools that write into an agent pane after a human keypress.
7. **N12: build, borrow code.** The only reusable pieces are the `gh` queries and the daemon pattern. Cheapest design: a CPD-owned poller (startup hook or Lead loop) on `gh pr view --json statusCheckRollup,reviews,comments` plus inline comments, whose output becomes Jobs.
8. **Human review of failed Attempts / final PR (optional):** `persiyanov/herdr-reviewr` (most active and polished) or `jhochenbaum/herdr-hunk-diff` for the User. Not on the critical path; User only talks to the Lead anyway.
9. **Skip:** `herdr-pickr`, `herdr-sidebar`, `herdr-sessionizer`, `herdr-worktrunk` (plugin), `herdr-renamer`, `pi-lens`, `pi-simplify`.

## Items

### persiyanov/herdr-reviewr (no npm; prebuilt Rust binary)
- Meta: ★874 · pushed 2026-10-10 · created 2026-06-26 · MIT · v0.46.0 (30 tags) · SHA `859dd5b` · 41 open issues · 499 commits by one author (`persiyanov`), 14 other contributors with 1–3 each.
- What: a persistent diff-review pane for a worktree; line comments are pasted into an agent's input.
- Mechanism:
  - Manifest: one `pane` (split), actions `toggle/open/close`, events `worktree.created` and `worktree.opened` → `--action auto-open` (`herdr-plugin.toml`; `src/actions.rs:111` gates on `auto_open`). `min_herdr_version = "0.9.3"`.
  - Delivery to agent: `pane.send_text` over the raw socket with bracketed-paste markers, unsubmitted (`src/herdr.rs:604-620`, `pasted()` at 849-860). It checks `agent list` first and refuses if the agent is `blocked` (`ensure_ready`, `herdr.rs:576-590`), then focuses the pane (`src/export.rs:191-195`).
  - Targeting: the agent is picked by a human; one agent in the workspace is taken automatically, several open a picker (README Limitations). No CLI/argument path to say "send to pane X".
  - Subscribes to `pane.agent_status_changed`, `pane.created`, etc. over the socket (`src/herdr_socket.rs:34-38`); "last turn" scope uses agent status transitions.
  - Reads PRs with `gh`/`glab`/`az`, read-only.
- Maturity: CI (fmt, clippy, tests on ubuntu + Windows), audit and release workflows, 11 integration test files in `tests/`, a fake-herdr example, changelog, releases every few days. Single-maintainer.
- CPD fit: N4 none. N12 none (PR tab is read-only). Useful only as a human review UI for the User over a Job worktree.
- Gaps/risks: no programmatic feedback path; comments are in-memory and lost on close (README); `unsubmitted paste` means a pi pane keeps the text in its editor until a human presses Enter, which would also clobber a half-typed input if aimed at the Lead.
- Verdict: BORROW. Best human diff UI in the ecosystem; the paste-with-readiness-check code (`herdr.rs:576-620`) is a good reference for safe `pane.send_text`; not part of the Dispatch loop.

### jhochenbaum/herdr-hunk-diff (no npm)
- Meta: ★143 · pushed 2026-10-12 · created 2026-08-12 · MIT · v0.5.0 (6 tags) · SHA `ac5eef7` · 5 open issues · 25 commits by `jhochenbaum`.
- What: opens `hunk` (diff TUI) on a worktree and sends the human's inline comments to the agent that owns that worktree.
- Mechanism:
  - Events `pane.agent_status_changed`, `pane.exited`, `pane.closed`, `worktree.removed` → `dist/bin/event.js` (manifest). Association worktree→agent comes from `herdr agent list` (`pane_id`, `cwd`, `agent_status`) (`src/events.ts:168-185`, `src/herdr.ts:96-117`).
  - Reads comments with `hunk session comment list --repo <wt>` (`src/hunk.ts:169-178`), formats a prompt (`src/courier.ts`) and calls `herdr agent prompt <pane> <text>` (`src/herdr.ts:73-76`, `src/runtime.ts:215-225`), which submits. Delivery is idempotent: sent IDs are recorded (`runtime.ts:226-240`).
  - The agent can also annotate the human's review through `hunk session comment apply --stdin` (`src/notes.ts`, skill `skills/hunk-herdr-review/SKILL.md`).
  - Action `send-review` takes no arguments: the skill states "`herdr plugin action invoke` accepts no arguments and no stdin" (SKILL.md, "Address the right session").
  - Optional `auto_open` on agent state `idle` (default off).
- Maturity: CI + `sync-verified-hunk` workflow, 30 test files in `tests/`, pinned `hunkdiff` dependency; recent. Needs Node ≥22.12, herdr ≥0.8.0.
- CPD fit: N4 none. It is the closest thing here to a programmatic route (worktree→agent targeting, submit, idempotence), but a human still triggers `send-review`.
- Gaps/risks: `agent prompt` returns `agent_blocked` if the agent is at a permission prompt; hunk loopback port 47657 dependency (`hunk.ts:146`); young.
- Verdict: BORROW. Reference for worktree→agent routing and the typed herdr CLI adapter (`src/herdr.ts`). Not needed for v0.

### plannotator/herdr-annotate (no npm; prebuilt Rust binary)
- Meta: ★658 · pushed 2026-10-12 · created 2026-08-09 · MIT · `rust-lite-v0.2.0` (plugin manifest 0.9.1) · SHA `ceab050` · 9 open issues · `backnotprop` 45 commits.
- What: select terminal text and annotate it (Lite), or review documents/agent replies in `plannotator-tui` (Full), then send to the focused agent.
- Mechanism: actions `capture`, `copy-context`, `paste-archive`, `send-archive`, `manage`, `open`, `last`, `terminal`; popup/overlay panes; no event hooks (`herdr-plugin.toml`). Delivery: `herdr agent get <pane>` readiness check, then `herdr agent prompt` (send) or `herdr pane send-text` with bracketed paste (paste) (`rust/src/agent_delivery.rs:151-165`). Target is always the focused pane's agent (`agent_delivery.rs:73,153`).
- Maturity: Rust crate with `rust/tests/commands.rs`, CI for lite and Windows-full; variants for three OSes.
- CPD fit: none for Dispatch. Could let the User annotate a Lead reply, which is already possible in the pi session via the plannotator pi extension.
- Gaps/risks: focused-pane only; no programmatic target.
- Verdict: SKIP.

### backnotprop/plannotator, npm `@plannotator/pi-extension`
- Meta: ★9,367 · pushed 2026-10-12 · created 2025-12-28 · Apache-2.0 (npm: `MIT OR Apache-2.0`) · v0.28.17 (30 tags) · npm 133,560 downloads/month · SHA `94c02b8` · 143 open issues · `backnotprop` 1,231 commits, a few others with 13–15.
- What: pi extension with plan mode, browser UI for plan approval/annotation, code/PR review UI and last-message annotation.
- Mechanism (pi APIs, `apps/pi-extension/index.ts`):
  - `pi.registerTool`: `plannotator_submit_plan` (1712), plan-mark-done (1671), optional `plannotator` agent tool (1564, off by default on pi), optional inbox tool.
  - `pi.registerCommand`: `/plannotator-plan-mode`, `-review`, `-annotate`, `-last` (875, 1514-1537). Flag `--plan` (534), shortcut Ctrl+Alt+P (1662).
  - Hooks: `session_start`, `tool_call` (2108), `before_agent_start` (2128), `turn_end`, `agent_end`, `session_compact`, `session_tree`, `session_shutdown`.
  - Decision delivery: `pi.sendUserMessage(text, {deliverAs:"followUp"})` (2038), so the submit tool returns immediately and the decision lands later without blocking and without typing into the editor. State persists with `pi.appendEntry`.
  - Event API for other extensions: channels `plannotator:request`, `plannotator:review-result`, `plannotator:plan-approved` (`plannotator-events.ts:83-85`); actions `plan-mode`, `plan-review`, `review-status`, `code-review` (patch file or PR URL), `annotate`, `annotate-last`, `archive`. `plan-review` only needs `planContent` (`plannotator-events.ts:443-451`, no plan-mode prerequisite); decisions are also stored in `~/.pi/plannotator-review-status.json` for recovery after `/reload`.
  - `executionMode: "external"` makes approval emit `plannotator:plan-approved` and go idle so another extension runs the plan.
  - Per-phase `model`/`thinking`/`activeTools` config in `~/.pi/agent/plannotator.json` or `.pi/plannotator.json`.
- Maturity: very high. About 33 `*.test.ts` files in the extension, workflows for release, security, DAST, e2e; peer dependency `@earendil-works/pi-coding-agent >=0.79.1` (we run 1.1.0); also supports OMP and other agents. Heavy: browser server, HTML bundles.
- CPD fit: N14 plan approval UI full, via the event API. Code review UI (`code-review` with `prUrl`) could serve the final-PR human review. N4 none. Structured question UI: not provided.
- Gaps/risks: opens a browser (or `PLANNOTATOR_REMOTE`), so it assumes a User at a desktop; session-bound (cannot route to another pane); Plan must be one markdown file; project-local config is gated by pi's trust prompt; plan-mode side effects on model/tools (see conflicts); large dependency surface for a "plan approved?" gate.
- Verdict: REUSE for N14 (event API only, plan mode off). Evaluate in a spike whether the Job-graph markdown renders legibly; otherwise a hand-rolled approval prompt is the fallback.

### tomasvarga/herdr-pickr (no npm)
- Meta: ★20 · pushed 2026-07-13 · created 2026-07-12 · MIT · v0.1.2 · SHA `e393ef5` · 0 issues · single author, 16 commits.
- What: Ctrl+click a GitHub PR/GitLab MR link, choose a reviewer tool (tuicr/hunk/diff/browser) in an overlay.
- Mechanism: `[[link_handlers]]` for `github.com/.../pull/N` and GitLab MR URLs → action `route` → overlay pane `chooser`; bash scripts only (`herdr-plugin.toml`, `bin/pickr-*`). No agent interaction; the herdr calls seen are `notification show` (`bin/pickr-toggle`) and the plugin's own pane/action plumbing (`bin/pickr-open`).
- Maturity: v0.1, no tests, no CI.
- CPD fit: none. No feedback to an agent at all.
- Verdict: SKIP.

### devashish2203/herdr-worktrunk + max-sixty/worktrunk

**herdr-worktrunk (plugin)**
- Meta: ★181 · pushed 2026-10-06 · created 2026-06-28 · MIT · v0.7.0 (manifest `0.1.0`, inconsistent) · SHA `f9df9ba` · 5 open issues · author 5 commits, 15 contributors total.
- What: fzf-driven herdr actions that call `wt switch/merge/remove` and register the result as a herdr worktree workspace.
- Mechanism:
  - Creation is by worktrunk (`wt switch --create --base <ref>`, `picker.sh:102`), then the checkout is registered with `herdr worktree open --cwd <repo> ...` (`picker.sh:191`); it does not call `herdr worktree create`, so it does not fire `worktree.created` (it fires `worktree.opened`; inference from manifest and scripts, untested).
  - Actions `open`, `open-current`, `open-with-remotes`, `remove`, `merge`, `merge-no-squash`; no `[[events]]` in the manifest.
  - Merge: `wt merge --no-remove -C <wtpath> [merge_flags]` then `wt remove --foreground -C <main>` then closes the herdr workspace via `herdr workspace close` (`merge.sh:56-77`, `lifecycle.sh:65-78`). Flags are configurable (`merge_flags = "--no-squash --no-rebase"` in `config.toml`).
  - Every path goes through an interactive fzf picker in a pane. There is no non-interactive entry point.
- Maturity: tests for every script (bash and PowerShell), no CI workflow, no `.github`. Requires worktrunk ≥0.60.
- CPD fit: N9/N10 only as a code reference; the interactive pickers cannot be driven by the Lead.
- Gaps/risks: interactive-only; merge removes the worktree and closes the UI in one go (CPD keeps failed Attempts' worktrees).
- Verdict: BORROW (the sequence "`wt merge --no-remove`, then explicit remove, then close workspace" and the Windows ordering notes). Do not install.

**max-sixty/worktrunk (CLI `wt`)**
- Meta: ★9,204 · pushed 2026-10-12 · created 2025-10-17 · `MIT OR Apache-2.0` (Cargo.toml:66; GitHub API shows NOASSERTION because of two licence files) · v0.80.0 · not on npm; install via brew/cargo/winget/pacman (README:113-141) · SHA `f98bd11` · 67 open issues · `max-sixty` 4,060 commits plus a bot with 750 · very active.
- What: git worktree manager with lifecycle hooks and a merge pipeline.
- Mechanism and semantics (docs in `docs/public/merge.md`, `hook.md`, `switch.md`, `step.md`; code in `src/commands/worktree/push.rs`):
  - **Create:** `wt switch --create <branch> --base <ref>` (shortcuts `^`, `@`, `pr:N`); path from a template or `--path`. It is plain `git worktree add`. Hooks: `pre-switch`, `pre-start` (blocking, stops on failure), `post-start`, `post-switch` (background). Project hooks live in `.config/wt.toml` and need one-time approval stored in `~/.config/worktrunk/approvals.toml`; `--yes` skips prompts, `--no-hooks` skips hooks. `wt step copy-ignored` copies gitignored files (env) between worktrees.
  - **Setup:** pipelines of `[[post-start]]` blocks with an ordered/concurrent model, template variables (`{{ branch }}`, `hash_port`).
  - **Merge:** `wt merge [TARGET] --branch <src>`: commit, squash (default), rebase onto target, pre-merge hooks, fast-forward the target, remove worktree (merge.md "Pipeline"). Flags: `--no-squash` (keep individual commits), `--no-ff`, `--no-rebase` (require fast-forward to the exact tip), `--no-commit`, `--no-remove`, `--stage`, `--format json`, `-C <path>`. Target defaults to the default branch; pass the Lead Branch name for CPD. "Merges into the *local* target branch and never fetches."
  - **Atomicity:** `advance_target` does a compare-and-swap `update-ref` against the old SHA and syncs the target's checked-out worktree with a two-tree merge, rolling back if a dirty file overlaps (`push.rs:362-395`). Concurrent mergers therefore fail with "not a fast-forward" (`push.rs:183`) rather than overwrite. There is no queue or lock.
  - **Cleanup:** `wt remove` deletes worktree and branch; `post-remove` hooks.
  - Rebase conflicts are left open in the source worktree.
  - Pi integration: `wt config plugins pi install` writes an activity-tracking extension into `~/.pi/agent/extensions/worktrunk.ts` (claude-code.md:57-60). Optional.
- Maturity: extensive tests, CI, nightly failure automation, signed Windows binaries, 80 releases. Single dominant maintainer.
- CPD fit:
  - N9: full (create with explicit base; copy-ignored; hooks).
  - N10: partial: preserves commits (`--no-squash`) but rebases them; fast-forward only; serial by caller. Open question: the Lead Branch worktree must not have uncommitted overlapping changes.
  - N11: partial via `[[pre-merge]]` (needs the repo to define it).
- Gaps/risks: a second dependency to install; `--no-squash` still rewrites SHAs through the rebase unless `--no-rebase` (which then requires the Job branch to be based on the Lead Branch HEAD, so merges must be serialised, each Job rebased onto the new tip); project-hook approval needs `--yes`; background `post-*` hooks of `wt merge` race with follow-up actions (the plugin works around it with `--no-remove`).
- Verdict: REUSE the CLI for N10/N11 after a spike (see recommendation 3). The herdr plugin on top of it is SKIP.

### tdi/herdr-worktree-setup (no npm)
- Meta: ★28 · pushed 2026-09-11 · created 2026-07-01 · **no licence** · v0.3.0 · SHA `c13ef57` · 0 issues · `tdi` 22 commits.
- What: runs configured shell steps in a new worktree on `worktree.created`.
- Mechanism: `[[events]] on = "worktree.created"` → `node src/setup.js` (manifest). Path from `HERDR_PLUGIN_EVENT_JSON` `data.worktree.path` (`src/worktree.js:12-35`); main repo from `git worktree list --porcelain`. Config is in the plugin's config dir (`config.toml`, `[default]` or `[[project]] path=...`), not in the repo. Steps run sequentially via `sh -c`, fail-fast, env `HERDR_MAIN_REPO`, `HERDR_WORKTREE`, `HERDR_BRANCH`; log to `HERDR_PLUGIN_STATE_DIR/setup-<ts>.log`; progress token `setup: running/done/failed` via `workspace report-metadata` (`setup.js:12-26`, needs herdr ≥0.7.4).
- Maturity: tests in `test/`, no CI; one dependency (`npm ci` build).
- CPD fit: N9 setup partial. Per-repo steps live in `~/.config/herdr/plugins/config/...`, which CPD's "per-repo setup lives where the plugin expects" accepts but is not versioned with the repo. Setup failure does not stop the Worker; the token is the only completion signal (expires after 5 s for `done`).
- Gaps/risks: no licence; global config; async relative to agent start.
- Verdict: BORROW (token pattern). Prefer `lamngockhuong/herdr-worktree-setup` (see additional finds).

### razajamil/herdr-plugin-workspace-manager (no npm; compiles on first use)
- Meta: ★48 · pushed 2026-08-23 · created 2026-06-16 · MIT · v0.8.0 · SHA `8302970` · 1 open issue · 4 contributors.
- What: declarative YAML tab/pane layouts applied to new worktrees, optionally starting an agent per pane.
- Mechanism: events `worktree.created`, `workspace.created`, `workspace.focused` (deduped), startup hook, actions `apply/validate/remove-gone` (manifest). Layout `setup.command` with `blocking: true` holds other panes until it finishes; `agent: <kind>` panes start via `herdr agent start` and wait for readiness; pane options `agentArgs`, `prompt`, `agentTimeoutMs`, `promptTimeoutMs` (README option table). Layout picked per repo/branch glob.
- Maturity: CI, `tests/integration.rs`, changelog, 11 tags; needs a Rust toolchain at install.
- CPD fit: N9 partial/ordering. The one thing here no one else has is "run setup to completion, then start the agent" in a layout, which solves the race above. But the prompt and model are static config (`agentArgs`, `prompt`), not per-spawn, so it cannot be N1.
- Gaps/risks: static; layouts duplicate what a pi spawn tool would do; compile-on-first-use.
- Verdict: BORROW (the blocking-setup-then-agent idea). Skip as a dependency.

### andrewchng/herdr-sessionizer (no npm)
- Meta: ★52 · pushed 2026-10-09 · created 2026-06-16 · MIT · v0.10.0 (19 tags) · SHA `d21a9e5` · 11 open issues · `andrewchng` 110 commits.
- What: fuzzy pickers to open projects/worktrees as herdr workspaces with layouts.
- Mechanism: actions and overlay panes only, no events (`herdr-plugin.toml`). Worktrees via `herdr worktree create` (`src/ops/worktrees.ts:49-55`, `--base` supported); can create `pr-<n>` worktrees from open PRs via `gh pr list` (README); per-repo layout in `<repo>/.sessionizer/config.toml`; destructive "remove worktree" flow.
- Maturity: CI, lefthook, many unit tests, ADRs and PRDs, release scripts.
- CPD fit: none for automation (interactive pickers). N9 only as a reference for per-repo layout files.
- Verdict: SKIP.

### wyattjoh/herdr-plugin-gh-pr (no npm)
- Meta: ★22 · pushed 2026-07-16 · created 2026-06-17 · no licence · v0.4.0 · SHA `6fe22de` · 4 open issues · `wyattjoh` 25 commits.
- What: sidebar label for the focused agent pane's branch PR (`#123 ✓/✗/●`).
- Mechanism: events `pane.focused`, `worktree.opened`, `worktree.created` → `bun bin/update-pr-status.ts`; calls `gh pr view <branch> --json number,state,url` and `gh pr checks <branch> --json bucket` (`src/main.ts:73,92`), writes `herdr pane report-metadata --token pr=...` (`main.ts:102`). Throttled to once per 30 s per pane; "herdr has no background-poll mechanism" (README). Actions `refresh`, `open-pr`.
- Maturity: unit tests for label and throttle, CI, release-please.
- CPD fit: N15 partial (PR/CI visibility); N12 none: it never acts on failures.
- Verdict: BORROW (token plumbing, and the `gh pr checks --json bucket` query). Skip as a dependency.

### wyattjoh/herdr-plugin-renamer (no npm)
- Meta: ★19 · pushed 2026-10-12 · created 2026-06-29 · MIT · no release/tag · SHA `e8d6dcd` · 1 open issue · 26 commits by `wyattjoh`.
- What: names panes (and renames auto-generated `worktree/...` branches and workspaces) from the agent's first prompt using an on-device model or Codex.
- Mechanism: event `pane.agent_status_changed`; Rust binary; reads the agent's transcript (`src/transcript.rs`, `src/codex.rs`); needs `herdr integration install pi|claude|codex`. Branch renames are local only and only for branches starting `worktree/`.
- Maturity: CI; no tests visible beyond the Rust sources; builds a Swift helper on macOS.
- CPD fit: none (cosmetic). Risk of surprise rename (see conflicts).
- Verdict: SKIP.

### tdi/herdr-worktree-from-linear (no npm)
- Meta: ★19 · pushed 2026-09-11 · created 2026-07-03 · no licence · v0.6.0 · SHA `e7fb804` · 5 open issues · `tdi` 31 commits.
- What: picks a Linear issue and creates/opens a herdr worktree on its branch.
- Mechanism: action `pick`, overlay picker, issue pane; `lib/worktree.js:19-45` builds `herdr worktree create --cwd R --branch B --base origin/<default> --focus --json`, or `worktree open --branch B` when a worktree for B already exists; optional `git fetch origin <base>` first; base choices `default | head | <branch>` (`lib/base.js`); publishes a `linear` workspace token. Comments in `worktree.js:24-31` record live-verified behaviour: `create --branch B` checks out an existing local branch ignoring `--base`; `open --branch B` errors if no worktree exists.
- Maturity: unit tests for each lib file, no CI.
- CPD fit: N9 reference only (the verified semantics above). Linear itself is out of scope.
- Verdict: BORROW (`lib/worktree.js` semantics).

### alexarthurs/herdr-sidebar (no npm; manifest in `plugins/herdr-sidebar/`)
- Meta: ★464 · pushed 2026-10-10 · created 2026-07-17 · MIT · v0.15.1 (18 tags) · SHA `081ed13` · 7 open issues · `alexarthurs` 187 commits.
- What: VS Code-style explorer and git source-control pane (staging, diffs, AI commit messages).
- Mechanism: panes `sidebar`, `source-control`; many actions (open, show-git, quick-open); Rust binary; uses `pane.send_input` for opening editors (`src/actions.rs:690`). No agent feedback path, no worktree events found in the manifest.
- Maturity: CI + agent-command-guard workflow; Windows/macOS/Linux.
- CPD fit: none (human UI).
- Verdict: SKIP.

### MattDevy/pi-extensions: `pi-simplify` (npm `pi-simplify`)
- Meta: ★151 (monorepo) · pushed 2026-10-03 · created 2026-03-26 · MIT · v0.2.3 · 42,743 downloads/month · SHA read `86cbbbb` · 13 open issues · `MattDevy` 94 commits.
- What: a `/simplify` command that builds a "review changed lines for clarity" prompt from `git diff`.
- Mechanism: `pi.registerCommand("simplify")` (`src/index.ts`), `getChangedFiles` via `git diff`, `pi.sendUserMessage(prompt, {deliverAs:"followUp"})` (`src/simplify-command.ts:~55`). Nothing else. ~70 lines of logic plus a prompt template (`src/prompt-builder.ts`).
- Maturity: vitest unit tests, changelog.
- CPD fit: none for v0. It is a prompt for the same session, not a Reviewer, and it does not restrict tools. The prompt could seed an optional "simplify pass" Job.
- Verdict: SKIP (BORROW the prompt if a simplify pass is ever wanted).

### apmantza/pi-lens (npm `pi-lens`)
- Meta: ★470 · pushed 2026-10-12 · created 2026-03-22 · MIT · v4.4.3 · 101,483 downloads/month · SHA `287b1e4` · 293 open issues · `wastedC` 6,469 commits, `apmantza` 4,568 (AI-assisted commits also credited: `claude` 297).
- What: pi extension giving the agent LSP diagnostics, linters, formatters, ast-grep, a dependency map, secrets scanning on every write/edit.
- Mechanism: many `pi.registerTool`/`registerCommand` (index.ts:1014-2108); hooks `tool_call` (read-guard and git-guard: block edits without a prior read, hold `git commit/push` while findings are unresolved), `tool_result` (post-write pipeline), `turn_end` (findings injection), `before_agent_start`/context injection; shared-checkout guard refuses worktree-mutating git commands when another live pi-lens session shares the tree (`clients/shared-checkout-guard.ts`); env `PI_LENS_CONCURRENT_SESSION_GUARD`; context injection can be disabled with `--no-lens-context` / `PI_LENS_NO_CONTEXT_INJECTION` (`docs/environment-variables.md:18,190,195`). Its `docs/merge-queue-rollout.md` concerns the project's own GitHub merge queue, not a feature.
- Maturity: very large test suite, mutation testing config, TLA+ models, CI; 293 open issues; very large surface.
- CPD fit: N11 none (not the Gate; it is per-edit feedback). Per-Worker memory cost: warm LSP pool per session (`docs/lsp-idle-eviction.md`); on an 8 GB MacBook Air with 3 Workers this is a risk (unverified, measure).
- Gaps/risks: guards interfere with commit flow; large attack and memory surface for headless Workers.
- Verdict: SKIP for v0 (optional Worker add-on later; evaluate with `--no-lens-context`).

## Additional finds

Scan of `/tmp/herdr-index.json` (≥5★, review/diff/worktree/PR/CI/merge/gate keywords). Items below were read at manifest level plus the mechanism noted; the rest of the matches are pure human UIs (listed at the end).

### lamngockhuong/herdr-worktree-setup
- Meta: ★7 · pushed 2026-10-10 · created 2026-09-14 · MIT · v0.4.0 · SHA `7f27cc4` · 3 open issues.
- What: on `worktree.created`, copy per-checkout config (`.env*`, `.envrc`, `.npmrc`, `*.tfvars`, Rails keys; auto-detected from gitignored files), symlink shared dirs, run `post_create`; on `worktree.removed` run `post_remove`.
- Mechanism: events `worktree.created` → `src/index.mjs`, `worktree.removed` → `src/remove.mjs`; per-repo config `.herdr-worktree.toml` in the repo; `post_create` / `post_remove` run only if the machine's `trusted-repos.txt` has a line `<repo path> sha256:<hash of the command blocks>` (README "Setup commands and trust"; `HERDR_WORKTREE_SETUP_TRUST_ALL=1` disables the gate); template vars `branch`, `worktree_path`, `repo_name`, `{{ branch | hash_port }}`; `notification show` toast on finish (`src/report.mjs:97`); steps stop at the first failure; Windows PowerShell supported.
- Maturity: CI (`ci.yml`), release-please, tests (`test/`), docs/recipes; 1 maintainer, 7★.
- CPD fit: N9 full for env copy plus install deps; closest to "per-repo setup" because config travels with the repo. Gap: completion is only a toast and a log, no machine-readable done signal (unlike `tdi`'s token).
- Verdict: REUSE (preferred setup plugin), after confirming the trust-file flow suits headless Workflows.

### jpolec/herdr-plugin-odysseus (herdr-orchestrator)
- Meta: ★6 · pushed 2026-09-25 · created 2026-09-23 · MIT · v0.4.0 · min herdr 0.9.0 · 23k lines Rust.
- What: a full governed workflow engine: task → worktree → agent → tests with retry → review → policy → approval → draft PR.
- Relevant mechanism: `gh pr view --json number,url,state,reviews,comments,statusCheckRollup` plus inline review comments turned into follow-up feedback text (`src/github/mod.rs:161-215,270`); "tasks start when their dependencies are merged" (README); runner kinds include `pi` as a generic pane agent (`docs/RUNNERS.md:55`), but the rich profiles are Claude/Codex; uses `herdr agent prompt --wait`.
- Maturity: CI and test suite (`tests/e2e_herdr.rs`), but only 3 weeks old.
- CPD fit: overlaps CPD's whole concept (competes rather than composes); N12 code is borrowable.
- Verdict: BORROW (N12 `feedback_from_json`); skip the product.

### jsmenzies/mergr
- Meta: ★5 · pushed 2026-07-30 · created 2026-07-28 · MIT · v0.3.2 · CI + release workflow.
- What: PR status in the sidebar via a background daemon.
- Mechanism: `[[startup]] target/release/mergr daemon` (a long-lived process despite the docs' "one-shot startup hooks" wording), `gh` for data, `workspace report-metadata` tokens (`src/daemon.rs`, `src/repository.rs`).
- CPD fit: N12 pattern for a poller process; N15 display.
- Verdict: BORROW (daemon-via-startup-hook pattern).

### tomasvarga/herdr-sniffr
- Meta: ★6 · pushed 2026-07-14 · MIT · v0.8.0. Wraps a standalone `sniffr` engine: background AI review writes local draft comments into tuicr/hunk for a human. Agent-agnostic. CPD fit: N4 none (human-facing drafts). Verdict: SKIP.

### DnzzL/herdr-automations
- Meta: ★16 · pushed 2026-10-08 · MIT · v0.8.2 · Go. Cron prompts to an agent in a fresh worktree, via a `[[startup]]` daemon; `model` per automation. CPD fit: pattern for startup daemon plus a per-run worktree; not Job semantics. Verdict: SKIP (BORROW the daemon pattern if one is needed).

### plannotator/herdr-plannotator
- Meta: ★26 · pushed 2026-07-29 · MIT · v0.1.0 · opens Plannotator review pages in a Herdr Browser pane (needs `herdr-browser`, Bun, Chrome, experimental Kitty graphics). Alternative to a desktop browser for N14; heavy dependencies. Verdict: SKIP.

### tdi/herdr-worktree-from-pr (★7), ogulcancelik/herdr-plugin-github-start (★17)
- Create a worktree from a PR (picker); find/resume/start a named pi/Codex/Claude session. Interactive. Verdict: SKIP.

### Other worktree-setup plugins (all `worktree.created` hooks, ★5–7, interactive-free)
- `freethinkel/herdr-plugin-git-worktree-hooks` (★7): one YAML config outside repos, hooks on created/removed, also hooks `workspace.created`/`workspace.focused`.
- `timofey-TK/herdr-worktree-hooks` (★6): Python, hooks on created/opened/removed, TOML config.
- `jlimas/herdr-worktree-seed` (★5): copy-on-write `node_modules` seeding and base-freshness check on `worktree.opened` (`check-base-freshness.sh`, which compares the base to a fresh origin, relevant to N9's "from Lead Branch HEAD" goal but it assumes `origin`).
- `tanshio/herdr-worktreeinclude` (★5): copies files matching a repo `.worktreeinclude`.
- `hmu332233/herdr-symlink-worktree` (★6): symlinks shared files from the main repo.
- Verdicts: BORROW selectively (`jlimas` CoW `node_modules` seeding is the fastest deps option if installs are slow); otherwise redundant with `lamngockhuong`.

### Display/human-only plugins matched by the scan (not reviewed beyond manifest/description)
`minhtran3124/herdr-worktree-agents` (worktree board with PR/CI, uses `workspace.focused`/`worktree.created`, ★7), `Matovidlo/herdr-pr-tracker` (Claude-only PR board, ★12), `ChmaraX/herdr-gitview`, `Crokily/herdr-lazygit`, `JeHwanYoo/herdr-git`, `smarzban/herdr-file-viewer`, `edmundmiller/herdr-plugin-hunk`, `scott306lr/herdr-plugin-hunk-autodiff` (opens hunk when an agent finishes with uncommitted changes), `JacquesvanWyk/herdr-hunk`, `osolmaz/ghzinga` (GitHub issue/PR TUI), `ezcorp-org/herdr-git-status`, `thomasschafer/herdr-kiosk`, `e2b-dev/herdr-e2b-sandbox` (N16: mirrors a worktree into an E2B sandbox, per-agent fleet; not read, relevant post-MVP), `MattDevy/pi-extensions` `pi-code-review` (npm 100/month; calls Haiku directly for structured review when an Anthropic key is present; N18 pattern, unverified beyond README).

## Details CPD should carry forward (facts verified in source)

- `herdr worktree create` = `git worktree add -b <branch> <path> <base>` for a new branch; existing branch is checked out as is (`/tmp/herdr-src/src/worktree.rs:239-258`; docs `cli-reference.mdx:182`). Default path: `<worktrees.directory>/<repo>/<branch-slug>`. `worktree remove` never deletes the branch (docs `cli-reference.mdx:184`).
- Events: `worktree.created` (data: `workspace`, `worktree`), `worktree.opened`, `worktree.removed` (`socket-api.mdx:837`). Emitted after the checkout and workspace exist (`src/app/api/worktrees/deferred.rs:477-502`).
- `herdr agent prompt <target> <text> [--wait]` submits and returns `agent_blocked` if the agent is at a prompt (`cli-reference.mdx:359`); `pane send-text` only types (non-submitting) (`cli-reference.mdx:269`).
- Neither herdr nor any plugin in this cluster offers a merge queue, a Gate runner, or CI-to-Job conversion; CPD owns those.
