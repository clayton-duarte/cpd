# What a native ratatui chat deletes from `pi/` and tmux: inventory

Evidence file `inventory` of Plan `ratatui-chat-research` (Job `inventory`, attempt `inventory-a1`), question 3: what building our own ratatui chat pane eliminates. Research only: no product code changed. Written 2026-10-28 EDT.

**Measured at `origin/main` = `f69f1eb0721db73f29fcbbc6120e530d265a308f`** (commit date 2026-10-28, "Cache balloon rows per Markdown child, not re-lexed every frame (#572)"). I first measured at `d943c49c` and re-extracted `pi/` after **ran** `git rev-parse origin/main` showed `origin/main` had moved to `f69f1eb0` (the diff touches `compact-tools-silent.ts` +41 lines and adds `pi/test/balloon-cache.test.ts`, 228 lines; **ran** `git diff --stat d943c49 f69f1eb0 -- pi core engine cli tui`). Every `pi/` number below is at `f69f1eb0`; the Rust numbers are unchanged between the two hashes (same diff command: nothing under `core`, `engine`, `cli`, `tui`).

Labels: **read** = I opened the file at that line; **ran** = a command I ran, quoted; **unverified** = inferred, with the reason. A verdict or classification is my judgement from **read** heads and exports and is labelled in the column header.

## How this was measured

```sh
git fetch origin
H=f69f1eb0721db73f29fcbbc6120e530d265a308f
T=$(mktemp -d); git archive $H pi core engine cli tui docs/research | tar -x -C $T
cd $T
find pi -type f -not -path 'pi/test/*' -not -name '*.test.ts' | xargs wc -l   # non-test pi/ files
find pi/test -type f | xargs wc -l                                           # pi tests
wc -l core/src/host/tmux.rs core/src/host/tmux/*.rs core/src/host/pi/*.rs core/src/host/input.rs
awk '/#\[cfg\(test\)\]/{exit} {n++} END{print n+0}' <rust file>              # non-test lines, split at the first #[cfg(test)]
git grep -n -E 'swap_pane|respawn_pane|chat slot|chat_slot|@cpd_view|set_pane_view|send_keys|send-keys|paste|bracketed' $H -- core engine cli tui
```

- Rust function lengths are `fn` line to its closing brace at the same indent (an awk script, so a nested closure ending the same way can be off by a few lines); a row that lists several functions sums them. **unverified** to the line, **ran** for the order of magnitude.
- Tests: `pi/test/**` (167 files, **ran** `find pi/test -type f | wc -l`), `pi/skills/skill.test.ts`, `*_tests.rs`, `cli/tests/**` and the `#[cfg(test)]` tail of a Rust file. Goldens and fixtures (the 10 non-`.ts` files of `pi/test/compact-tools`: 48,744 lines, **ran** `find pi/test/compact-tools -type f | grep -v '\.ts$' | xargs cat | wc -l`) are counted apart from test code.
- **Assumption, stated once (unverified: the Job does not fix the layout).** *Scenario A*: the chat becomes a ratatui widget in `cpd-tui`, drawn in the existing chat-slot tmux pane by `cpd tui view chat`; Leads and Managers run as host processes (not tmux panes); Worker panes stay in tmux as the Job says. Under this scenario `View::Chat` and the `@cpd_view` pane stay, and `swap-pane` of Lead panes goes. A scenario where the whole cockpit becomes one process would also delete more of `tui/src/cockpit.rs`; I did not measure that.
- **Verdict meaning.** DELETED = the file's code is removed. "Re-implemented in ratatui? = Y" says the behaviour the file gives the User (a fold, a balloon, a cursor, a dialog) must exist in the native widget, so the deletion is not a saving of that behaviour; N = the file only works around pi's TUI (patches, guards, caches) and its behaviour has no counterpart. KEPT = runner-neutral cpd logic. MOVED = needed with any runner, rewritten for a Node `pi-durable` host or in Rust.

## 1. `pi/` files (non-test)

Totals per verdict are in section 6; the per-file tests column holds the test files I could map to one source file by their imports (**read** `from` lines of each test, **ran** `grep -ohE 'from "(\.\./|\./)[^"]*"' pi/test/*.ts`). Tests that cover a whole group (compact-tools, ask-user-question, top-level compact-tools tests) are in the table after the four below.

#### compact-tools

| File | Lines | Tests (lines) | Verdict | Re-implemented in ratatui? | Purpose (read: head and exports) |
|---|---:|---:|---|---|---|
| `pi/compact-tools/header.ts` | 20 |  | DELETED | N | Header comment/constants of the vendored extension |
| `pi/compact-tools/schemas/compact-tools.schema.json` | 114 |  | DELETED | N | JSON schema of the compact-tools display config |
| `pi/compact-tools/extensions/compact-tools-activity.ts` | 329 |  | DELETED | Y | Silent-mode activity labels; patches UserMessage.render and two containers via hookMethod |
| `pi/compact-tools/extensions/compact-tools-balloon.ts` | 49 |  | DELETED | Y | Draws chat balloons (borders, tails, wrapped rows) |
| `pi/compact-tools/extensions/compact-tools-claude.ts` | 239 |  | DELETED | Y | Claude-Code style summary rows ("… +N lines") |
| `pi/compact-tools/extensions/compact-tools-clock.ts` | 55 |  | DELETED | N | One shared animation clock for indicators and labels |
| `pi/compact-tools/extensions/compact-tools-codex-rows.ts` | 474 |  | DELETED | Y | Codex-style row drawing (file/shell/custom) |
| `pi/compact-tools/extensions/compact-tools-codex.ts` | 459 |  | DELETED | Y | Codex-style preview rows of a call's output |
| `pi/compact-tools/extensions/compact-tools-command.ts` | 193 |  | DELETED | N | The /compact-tools pi command: saves display style to settings |
| `pi/compact-tools/extensions/compact-tools-compaction.ts` | 91 |  | DELETED | Y | Re-draws pi's compaction notice as a fold (hookMethod on updateDisplay) |
| `pi/compact-tools/extensions/compact-tools-config.ts` | 195 |  | DELETED | N | Reads/validates the compact-tools config, DEFAULT_CONFIG |
| `pi/compact-tools/extensions/compact-tools-core.ts` | 32 |  | DELETED | N | Row status type + pure timing/text helpers |
| `pi/compact-tools/extensions/compact-tools-cursor.ts` | 128 |  | DELETED | Y | Blinking Dango input cursor (3 hookMethod sites on Editor) |
| `pi/compact-tools/extensions/compact-tools-custom.ts` | 108 |  | DELETED | N | Routes custom tool rows by patching ToolExecutionComponent (5 sites) |
| `pi/compact-tools/extensions/compact-tools-double-load.ts` | 76 |  | DELETED | N | Says once per process that npm copy and vendored copy both load |
| `pi/compact-tools/extensions/compact-tools-editor.ts` | 231 |  | DELETED | Y | Draws pi-tui Editor as a rounded box (4 hookMethod sites) |
| `pi/compact-tools/extensions/compact-tools-grouping.ts` | 487 |  | DELETED | Y | Claude-style grouping of "looked around" tool calls; patches chat.render |
| `pi/compact-tools/extensions/compact-tools-hook.ts` | 47 |  | DELETED | N | hookMethod: the once-per-process prototype wrapper every patch uses |
| `pi/compact-tools/extensions/compact-tools-indices.ts` | 105 |  | DELETED | Y | The 0-15 indexed palette compact-tools draws with |
| `pi/compact-tools/extensions/compact-tools-invocation.ts` | 531 |  | DELETED | Y | Formats a tool call's invocation text per tool |
| `pi/compact-tools/extensions/compact-tools-italic.ts` | 17 |  | DELETED | N | Wraps Theme.fg to emit SGR 3 for four theme tokens |
| `pi/compact-tools/extensions/compact-tools-layout.ts` | 571 |  | DELETED | Y | Row/fold layout; Container cache that avoids re-copying lines per frame |
| `pi/compact-tools/extensions/compact-tools-loader.ts` | 43 |  | DELETED | Y | The 28-phase moon loader frames |
| `pi/compact-tools/extensions/compact-tools-nested.ts` | 168 |  | DELETED | Y | Bounded bookkeeping of nested call summaries |
| `pi/compact-tools/extensions/compact-tools-progress.ts` | 146 |  | DELETED | Y | Working-row progress text; hooks editor render |
| `pi/compact-tools/extensions/compact-tools-rendering.ts` | 86 |  | DELETED | N | Fingerprints of pi's tool renderers (to detect overrides) |
| `pi/compact-tools/extensions/compact-tools-runtime.ts` | 295 |  | DELETED | N | Render-frame throttle for running rows, execution timings |
| `pi/compact-tools/extensions/compact-tools-shell.ts` | 277 |  | DELETED | Y | Tokenises shell commands for summary rows |
| `pi/compact-tools/extensions/compact-tools-silent.ts` | 495 |  | DELETED | Y | Silent mode: patches render of 4 pi components; hides rows |
| `pi/compact-tools/extensions/compact-tools-thinking.ts` | 385 |  | DELETED | Y | Thinking-block fold, label, click painter (hookMethod updateContent) |
| `pi/compact-tools/extensions/compact-tools-transcript.ts` | 93 |  | DELETED | N | attemptUI: collects UI failures instead of throwing in pi's render |
| `pi/compact-tools/extensions/compact-tools-types.ts` | 85 |  | DELETED | N | Display style types (compact, claude, codex, off) |
| `pi/compact-tools/extensions/compact-tools-viewport.ts` | 234 |  | DELETED | N | Patches TuiAltScreen.doRender to restore scroll across redraws |
| `pi/compact-tools/extensions/compact-tools.ts` | 996 |  | DELETED | Y | Extension entry: wires every controller and renderer into pi |
| `pi/compact-tools/LICENSE` | 21 |  | DELETED | N | MIT text of upstream |
| `pi/compact-tools/NOTICE` | 17 |  | DELETED | N | Fork provenance (upstream 0.16.6, 8be1a8d) |
| **Subtotal compact-tools** (36 files) | **7892** | **0** (mapped only) | | | |

#### ask-user-question

| File | Lines | Tests (lines) | Verdict | Re-implemented in ratatui? | Purpose (read: head and exports) |
|---|---:|---:|---|---|---|
| `pi/ask-user-question/NOTICE` | 15 |  | DELETED | N | Fork provenance (rpiv v2.13.8, afc21bc) |
| `pi/ask-user-question/LICENSE` | 21 |  | DELETED | N | MIT text of upstream |
| `pi/ask-user-question/index.ts` | 30 |  | DELETED | N | Extension entry: role gate, double-load, registers tool |
| `pi/ask-user-question/ask-user-question.ts` | 462 |  | DELETED | Y | Tool registration and ctx.ui.custom mount of the questionnaire |
| `pi/ask-user-question/config.ts` | 108 |  | DELETED | N | Reads collapse-key config of the overlay |
| `pi/ask-user-question/double-load.ts` | 68 |  | DELETED | N | Stands down when the npm package is also listed |
| `pi/ask-user-question/events.ts` | 59 |  | DELETED | N | rpiv:* prompt/blocked event contract (no consumer found in pi/lib) |
| `pi/ask-user-question/limit-text.ts` | 7 |  | MOVED | - | Builds the limit sentence from askguard LIMIT for the tool description |
| `pi/ask-user-question/presentation.ts` | 122 |  | DELETED | Y | Inline/overlay row budget for the dialog |
| `pi/ask-user-question/reconcile.ts` | 47 |  | DELETED | N | Strips/re-adds the tool by ctx.hasUI |
| `pi/ask-user-question/rpc-fallback.ts` | 203 |  | DELETED | N | select/input walker for RPC hosts |
| `pi/ask-user-question/state/build-questionnaire.ts` | 342 |  | DELETED | Y | Builds questionnaire state from params |
| `pi/ask-user-question/state/external-editor.ts` | 62 |  | DELETED | Y | Spawns $EDITOR with pi-tui stop/start |
| `pi/ask-user-question/state/i18n-bridge.ts` | 28 |  | DELETED | N | Identity t() shim |
| `pi/ask-user-question/state/key-router.ts` | 355 |  | DELETED | Y | Maps pi-tui keys to row intents |
| `pi/ask-user-question/state/questionnaire-session.ts` | 289 |  | DELETED | Y | Questionnaire session object (answers, tabs, submit) |
| `pi/ask-user-question/state/row-intent.ts` | 143 |  | DELETED | Y | Row intent metadata (select, other, chat, next) |
| `pi/ask-user-question/state/selectors/contract.ts` | 24 |  | DELETED | Y | Selector contracts for views |
| `pi/ask-user-question/state/selectors/derivations.ts` | 55 |  | DELETED | Y | Derived values for views |
| `pi/ask-user-question/state/selectors/focus.ts` | 17 |  | DELETED | Y | Active-view selector |
| `pi/ask-user-question/state/selectors/projections.ts` | 109 |  | DELETED | Y | Projections of state into view props |
| `pi/ask-user-question/state/state-reducer.ts` | 352 |  | DELETED | Y | Reducer of the dialog state machine |
| `pi/ask-user-question/state/state.ts` | 65 |  | DELETED | Y | Dialog state types |
| `pi/ask-user-question/tool/format-answer.ts` | 32 |  | MOVED | - | Formats one answer for the model |
| `pi/ask-user-question/tool/normalize-params.ts` | 102 |  | MOVED | - | Normalises the model's tool params |
| `pi/ask-user-question/tool/response-envelope.ts` | 69 |  | MOVED | - | Builds the tool result sent back to the model |
| `pi/ask-user-question/tool/types.ts` | 165 |  | MOVED | - | Tool parameter schema and result types |
| `pi/ask-user-question/tool/validate-questionnaire.ts` | 56 |  | MOVED | - | Validates questions (counts, options) |
| `pi/ask-user-question/view/component-binding.ts` | 45 |  | DELETED | N | Binds selectors to pi-tui components |
| `pi/ask-user-question/view/components/inline-input.ts` | 66 |  | DELETED | Y | pi-tui inline text input row |
| `pi/ask-user-question/view/components/multi-select-view.ts` | 194 |  | DELETED | Y | pi-tui checkbox list view |
| `pi/ask-user-question/view/components/option-list-view.ts` | 68 |  | DELETED | Y | pi-tui option list view |
| `pi/ask-user-question/view/components/preview/markdown-content-cache.ts` | 77 |  | DELETED | Y | Cache of rendered markdown previews |
| `pi/ask-user-question/view/components/preview/preview-block-renderer.ts` | 149 |  | DELETED | Y | Renders preview blocks |
| `pi/ask-user-question/view/components/preview/preview-box-renderer.ts` | 86 |  | DELETED | Y | Draws preview box |
| `pi/ask-user-question/view/components/preview/preview-layout-decider.ts` | 207 |  | DELETED | Y | Chooses side-by-side or stacked preview |
| `pi/ask-user-question/view/components/preview/preview-pane.ts` | 318 |  | DELETED | Y | Preview pane component |
| `pi/ask-user-question/view/components/submit-picker.ts` | 67 |  | DELETED | Y | Review/submit tab |
| `pi/ask-user-question/view/components/tab-bar.ts` | 58 |  | DELETED | Y | Tab bar component |
| `pi/ask-user-question/view/components/wrapping-select.ts` | 297 |  | DELETED | Y | Wrapping select list |
| `pi/ask-user-question/view/dialog-builder.ts` | 260 |  | DELETED | Y | Assembles the dialog component tree |
| `pi/ask-user-question/view/dialog-copy.ts` | 51 |  | DELETED | Y | Dialog strings |
| `pi/ask-user-question/view/props-adapter.ts` | 117 |  | DELETED | N | State to component props |
| `pi/ask-user-question/view/scroll-window.ts` | 73 |  | DELETED | Y | Scroll-to-focus partition |
| `pi/ask-user-question/view/stateful-view.ts` | 24 |  | DELETED | N | Stateful view interface |
| `pi/ask-user-question/view/tab-components.ts` | 16 |  | DELETED | N | Tab component registry |
| `pi/ask-user-question/view/tab-content-strategy.ts` | 348 |  | DELETED | Y | Per-tab content strategy |
| **Subtotal ask-user-question** (47 files) | **5928** | **0** (mapped only) | | | |

#### lib

| File | Lines | Tests (lines) | Verdict | Re-implemented in ratatui? | Purpose (read: head and exports) |
|---|---:|---:|---|---|---|
| `pi/lib/adapter.ts` | 103 |  | MOVED | - | Wires all adapter concerns to pi hooks (pi.on, registerMessageRenderer) |
| `pi/lib/agent-state.ts` | 92 |  | MOVED | - | pi hooks that feed the Session state facts (session state CLI) |
| `pi/lib/askguard.ts` | 104 | 280 | KEPT | - | Pure refusal rule for long ask_user_question calls (LIMIT 1500) |
| `pi/lib/balloon.ts` | 2 |  | DELETED | N | 2-line re-export of drawBalloon (no cache; see section 3) |
| `pi/lib/compaction-ratio.ts` | 69 | 121 | MOVED | - | Wraps pi SettingsManager reserve from compaction ratio |
| `pi/lib/cpd.ts` | 87 | 71 | KEPT | - | CpdError and envelope parsing for cpd CLI calls |
| `pi/lib/cursor.ts` | 16 |  | KEPT | - | Event cursor (last delivered Event id), not a UI cursor |
| `pi/lib/delivery.ts` | 44 | 382 | KEPT | - | Which Event kinds become messages and how (steer/follow-up) |
| `pi/lib/event.ts` | 98 | 203 | KEPT | - | Event kinds and PROTOCOL = 17 |
| `pi/lib/first-message.ts` | 40 | 118 | MOVED | - | Detects the delivered first message in pi session entries |
| `pi/lib/follower.ts` | 160 | 227 | KEPT | - | Spawns and restarts the cpd events follower process |
| `pi/lib/handle.ts` | 173 | 91 | MOVED | - | Follower lines to pi sendMessage/steer |
| `pi/lib/hint.ts` | 85 | 352 | DELETED | Y | Patches Theme.bg and Container.render for jump hint/tab bar colours |
| `pi/lib/host.ts` | 117 |  | MOVED | - | Shared adapter state; holds pi, options, activation |
| `pi/lib/hostapprove.ts` | 60 | 108 | MOVED | - | cpd_host_approve tool, raw-command guard, ui.select prompt |
| `pi/lib/italic.ts` | 4 | 11 | DELETED | Y | SGR 3 helpers |
| `pi/lib/jira-transition.ts` | 201 | 259 | KEPT | - | Jira transition helper script via pi's MCP client (no TUI) |
| `pi/lib/loop-guard.ts` | 60 | 267 | MOVED | - | Loop guard of Leads/Managers |
| `pi/lib/mainguard.ts` | 253 | 594 | KEPT | - | Refuses git/bash/edit writes to a primary checkout |
| `pi/lib/mcp.ts` | 18 | 159 | MOVED | - | Mounts pi's MCP extension with no file config |
| `pi/lib/mentions-refresh.ts` | 31 | 81 | DELETED | Y | Reloads Lead/Manager ids and publishes the painter |
| `pi/lib/mentions.ts` | 67 | 49 | DELETED | Y | Paints @mentions magenta in the pane |
| `pi/lib/models-list.ts` | 177 | 327 | KEPT | - | Lists provider models helper script (no TUI) |
| `pi/lib/noblock.ts` | 84 | 172 | KEPT | - | Refuses blocking waits by Managers/Leads |
| `pi/lib/pad.ts` | 82 | 105 | DELETED | N | 1-cell pad: wraps TuiAltScreen.setLayoutRoot |
| `pi/lib/phi.ts` | 9 |  | MOVED | - | Bold phi SGR wrapper for labels |
| `pi/lib/pi.ts` | 36 |  | MOVED | - | Narrowed pi extension API types |
| `pi/lib/pill-spec.json` | 21 |  | KEPT | - | Pill glyph/colour data shared with Rust (core/src/pill.rs) |
| `pi/lib/pill.ts` | 53 | 76 | DELETED | Y | pi adapter of the pill spec |
| `pi/lib/plan-review-command.ts` | 36 |  | MOVED | - | /plan-review pi command |
| `pi/lib/plan-review.ts` | 77 | 612 | KEPT | - | Plan-review entry types and texts |
| `pi/lib/plannotator.ts` | 78 |  | MOVED | - | plannotator event API client over pi.events |
| `pi/lib/pnguard.ts` | 40 | 102 | KEPT | - | Refuses untracked plannotator calls |
| `pi/lib/pntrack.ts` | 157 | 157 | KEPT | - | Mirrors plannotator's registry to cpd review |
| `pi/lib/policy.ts` | 25 | 101 | KEPT | - | Policy section for the system prompt |
| `pi/lib/protocol.ts` | 22 |  | KEPT | - | Protocol check against cpd |
| `pi/lib/reload-command.ts` | 30 |  | MOVED | - | The reload-after-install command guard |
| `pi/lib/reload.ts` | 300 | 639 | MOVED | - | Polls adapter dir, reloads pi after install |
| `pi/lib/reminder.ts` | 83 | 136 | KEPT | - | Turn-end reminder rule |
| `pi/lib/render.ts` | 237 | 678 | DELETED | Y | Renders every cpd message as a fold in pi's chat |
| `pi/lib/reply.ts` | 21 | 111 | KEPT | - | Decides whether a run answered the User |
| `pi/lib/retier.ts` | 73 | 190 | MOVED | - | Switches pi model on session-retiered |
| `pi/lib/review-chat.ts` | 71 | 602 | MOVED | - | Plan-review dialog loop via ui.select |
| `pi/lib/review.ts` | 267 |  | MOVED | - | Plan review in chat and via plannotator |
| `pi/lib/run.ts` | 94 |  | KEPT | - | session show parse and Run object |
| `pi/lib/secret.ts` | 58 | 86 | MOVED | - | cpd_secret_approve tool with ui.select |
| `pi/lib/secretguard.ts` | 23 | 80 | KEPT | - | Refuses the raw 1Password CLI |
| `pi/lib/shell.ts` | 47 |  | KEPT | - | Shell tokenizer shared by guards |
| `pi/lib/slack-channel.ts` | 206 | 246 | KEPT | - | Slack channel helper script |
| `pi/lib/slack-parse.ts` | 182 | 142 | KEPT | - | Slack parse rules |
| `pi/lib/slack-post.ts` | 61 | 107 | KEPT | - | Slack send helper |
| `pi/lib/slack-react.ts` | 49 | 142 | KEPT | - | Slack reaction helper |
| `pi/lib/startup.ts` | 268 | 261 | MOVED | - | Start/stop of the adapter run, reload log |
| `pi/lib/state.ts` | 164 | 305 | KEPT | - | Facts and the state they give |
| `pi/lib/testguard.ts` | 85 | 148 | KEPT | - | Refuses unfiltered test runs by Workers |
| `pi/lib/time.ts` | 56 | 65 | KEPT | - | Local-time formatters |
| `pi/lib/turns.ts` | 100 |  | MOVED | - | Context updates at turn start; reminder check |
| `pi/lib/updates.ts` | 323 | 526 | KEPT | - | Context update message text |
| **Subtotal lib** (58 files) | **5579** | **9489** (mapped only) | | | |

#### top of pi/

| File | Lines | Tests (lines) | Verdict | Re-implemented in ratatui? | Purpose (read: head and exports) |
|---|---:|---:|---|---|---|
| `pi/extension.ts` | 7 |  | MOVED | - | Entry: adapter(pi) |
| `pi/mcp-extension.ts` | 4 |  | MOVED | - | Second entry: pi's MCP extension |
| `pi/package.json` | 17 |  | MOVED | - | pi package manifest (extensions, skills, themes) |
| `pi/themes/dango.json` | 62 | 174 | DELETED | Y | pi theme (Dango palette) |
| `pi/skills/cpd-lead/SKILL.md` | 409 |  | KEPT | - | Lead skill (agent instructions) |
| `pi/skills/cpd-lead/references/pr-description.md` | 104 |  | KEPT | - | Lead skill reference |
| `pi/skills/cpd-lead/references/questions.md` | 58 |  | KEPT | - | Lead skill reference |
| `pi/skills/cpd-lead/references/spec-files.md` | 104 |  | KEPT | - | Lead skill reference |
| `pi/skills/cpd-manager/SKILL.md` | 143 |  | KEPT | - | Manager skill |
| `pi/skills/cpd-manager/references/.gitkeep` | 0 |  | KEPT | - | empty placeholder |
| **Subtotal top of pi/** (10 files) | **908** | **174** (mapped only) | | | |


#### Tests not mapped to one file

| Group | Files | Lines | Verdict | Source of the number |
|---|---:|---:|---|---|
| `pi/test/compact-tools/*.ts` (tests, harnesses, theme fixtures as `.ts`) | 71 | 12,967 | DELETED | **ran** `find pi/test/compact-tools -name '*.ts' \| xargs cat \| wc -l` |
| `pi/test/compact-tools/` non-`.ts` data files | 10 | 48,744 | DELETED | **ran** (command above); `golden/indicator.json` alone is 40,793 lines |
| `pi/test/ask-user-question/*` | 5 | 356 | DELETED | **ran** `find pi/test/ask-user-question -type f \| xargs cat \| wc -l` |
| Top-level tests of compact-tools drawing: `balloon`, `balloon-margins`, `balloon-cache`, `chat-margins`, `rail-margins`, `icons` | 6 | 1,017 | DELETED | **read** they import `indicator-harness.ts`, `compact-tools-thinking.ts`, `compact-tools-config.ts`, `compact-tools-loader.ts`; **ran** `wc -l` of the six |
| Shared harness and guards: `fake.ts` 827, `pi-check` 429, `manager-errors` 267, `verdict-label` 149, `harness-teardown` 147, `user-commands` 110, `no-fixed-sleeps` 102, `script-main` 76, `answers.ts` 41, `skill-retry` 41, `tripwire-allow` 43, `gate` 47, `structure` 25, `mcp-harness` 82, `lead-errors` 16, `pi-types` 8 | 16 | 2,410 | KEPT/MOVED (not split) | **ran** `wc -l`; **unverified** which run on the adapter only |
| `pi/skills/skill.test.ts` (outside `pi/test`) | 1 | 858 | KEPT | **ran** `wc -l pi/skills/skill.test.ts` |

## 2. Rust and tmux side

Every non-test hit of the grep in section "How this was measured" is a row or inside a row's function (**ran**: the grep returns 517 lines in 101 files, 43 of those files non-test; the 58 test, fixture and fake files are summarised after the table). The `Lines` column is the function's lines (**ran** by the awk script, see the caveat above); `pane swapping` etc. name the tmux interaction the item exists for.
| Group | Function / item | file:line | Lines | What it does | tmux interaction | Verdict | Why |
|---|---|---|---:|---|---|---|---|
| tmux | `OPT_VIEW const` | `core/src/host/tmux.rs:79` | 1 | Names the `@cpd_view` pane option | view placeholders | KEPT | Views tree, plan, widgets and chat stay tmux panes |
| tmux | `BORDER_FORMAT` | `core/src/host/tmux.rs:114` | 15 | Pane border title keyed by `@cpd_session`/`@cpd_view` (chat and history: no title) | view placeholders, status line | KEPT | Worker and view panes still draw titles; the Lead-title branch cannot be split out by line |
| tmux | `PANE_DIED_HOOK, WINDOW_UNLINKED_HOOK` | `core/src/host/tmux.rs:148` | 20 | Respawn a pane that carries `@cpd_view` when its program dies; restart the engine | respawn | KEPT | The chat view pane is a `@cpd_view` pane too |
| tmux | `SERVER_OPTIONS (incl. `MouseDown1Pane select-pane ; send-keys -M`)` | `core/src/host/tmux.rs:171` | 36 | Server-wide options; the only `send-keys` in non-test source is the mouse bind at :202 | mouse and focus forwarding | KEPT | tmux forwards the mouse to a ratatui pane the same way |
| tmux | `status line (STATUS_CLICK … STATUS_OPTIONS)` | `core/src/host/tmux.rs:209` | 150 | The cockpit's tmux status line and its buttons | status line | KEPT | Not the chat's status; cockpit buttons stay |
| tmux | `Pane, LIST_PANES_FORMAT, Line` | `core/src/host/tmux.rs:416` | 30 | Pane record with `cpd_session`, `cpd_view`; `list-panes` format | view placeholders | KEPT | Still lists views and Worker panes |
| tmux | `set_pane_view` | `core/src/host/tmux.rs:860` | 3 | `set -p @cpd_view` | view placeholders | KEPT | Used for the four views (cockpit.rs:177-183) |
| tmux | `set_pane_session` | `core/src/host/tmux.rs:850` | 3 | `set -p @cpd_session` | pane swapping (identity) | KEPT | Also set for Worker panes (core/src/driver/worker.rs:375) |
| tmux | `set_pane_title` | `core/src/host/tmux.rs:855` | 3 | `set -p @cpd_title` | status line (border title) | KEPT | Worker panes; the chat widget draws its own title |
| tmux | `set_pane_history` | `core/src/host/tmux.rs:865` | 3 | `set -p @cpd_history` for the history pane | view placeholders | DELETED | Only caller: core/src/session/history.rs:47 |
| tmux | `swap_pane` | `core/src/host/tmux.rs:901` | 3 | `swap-pane` | pane swapping | DELETED | Callers: core/src/session/moves.rs:234, :294 only |
| tmux | `respawn_pane` | `core/src/host/tmux.rs:914` | 6 | `respawn-pane` | respawn | KEPT | Callers: engine/src/watch/ensure.rs:244, tui/src/cockpit.rs:458 |
| tmux | `respawn_pane_killing` | `core/src/host/tmux.rs:924` | 6 | `respawn-pane -k` | respawn | KEPT | Callers also history.rs:45 and liveness.rs:510 (both DELETED below); cockpit.rs:434 stays |
| tmux | `pane_width` | `core/src/host/tmux.rs:999` | 5 | `#{pane_width}` of the chat slot (title width) | resize and padding | MOVED | Only caller engine/src/watch/titles.rs:154; the widget reports its own width |
| tmux | `warn_unknown_view` | `core/src/host/tmux.rs:1169` | 6 | Logs an unknown `@cpd_view` | view placeholders | KEPT | Same views |
| tmux | `parse_pane` | `core/src/host/tmux.rs:1176` | 29 | Parses a `list-panes` line incl. `@cpd_view` | view placeholders | KEPT | Same views |
| layout | `View enum (Chat, History variants)` | `core/src/host/layout.rs:21` | 8 | `@cpd_view` values | view placeholders | MOVED | History variant DELETED, Chat kept; 1 variant of 5 goes |
| layout | `chat_slot` | `core/src/host/layout.rs:87` | 5 | Finds the main-window pane with `@cpd_session` or view chat/history | pane swapping | MOVED | Becomes `view_pane(View::Chat)`; `@cpd_session` branch goes |
| layout | `park_window` | `core/src/host/layout.rs:65` | 3 | Names a parked window | pane swapping | DELETED | No parked Lead windows |
| layout | `session_pane` | `core/src/host/layout.rs:100` | 3 | Pane of a Session by `@cpd_session` | pane swapping | KEPT | Worker panes |
| moves | `lead_spawn` | `core/src/session/moves.rs:32` | 10 | Spawn argv of a Lead pane | pane swapping | MOVED | Becomes a host-process start |
| moves | `parked_window` | `core/src/session/moves.rs:44` | 6 | Window name for a parked Lead | pane swapping | DELETED |  |
| moves | `placeholder_spawn` | `core/src/session/moves.rs:52` | 3 | `cpd tui view chat` placeholder spawn | view placeholders | DELETED | The chat view is always the widget |
| moves | `moved` | `core/src/session/moves.rs:57` | 13 | Output of `session open/park` | pane swapping | MOVED | Output stays |
| moves | `fork` | `core/src/session/moves.rs:101` | 64 | `session fork`: new window with a pi pane, then open | pane swapping | MOVED | Starts a host process instead of a window |
| moves | `kill_window_on_error` | `core/src/session/moves.rs:168` | 5 | Cleanup of a half-built window | pane swapping | DELETED |  |
| moves | `slot` | `core/src/session/moves.rs:175` | 3 | Chat slot or `no_cockpit` | pane swapping | DELETED |  |
| moves | `open` | `core/src/session/moves.rs:180` | 77 | `session open`: swap the Lead's pane into the chat slot | pane swapping | MOVED | Becomes "show this Session"; the swap goes, the Event and store write stay |
| moves | `park` | `core/src/session/moves.rs:259` | 20 | `session park`: move the Lead out of the slot | pane swapping | DELETED |  |
| moves | `park_lead` | `core/src/session/moves.rs:282` | 17 | New `park-<label>` window with placeholder, swap | pane swapping | DELETED |  |
| moves | `placed, gone, gone_one, reopen, reopen_one` | `core/src/session/moves.rs:320` | 80 | After a cockpit restart: which Session held which place; bring each back | pane swapping | MOVED | Rewritten for host processes |
| moves | `start_parked` | `core/src/session/moves.rs:417` | 34 | Starts a stopped Lead in a parked window | pane swapping | MOVED | Starts a host process |
| moves | `close_lead, close_own, close` | `core/src/session/moves.rs:463` | 63 | `session close`: kills the Lead's window; refuses `in_chat_slot` | pane swapping | MOVED | Refusal rule stays |
| history | `history_spawn, history` | `core/src/session/history.rs:15` | 34 | `session history`: respawn the slot pane with `cpd tui view history` | view placeholders | DELETED | Replaced by the widget reading the archive (see tui rows) |
| send | `send, needs_pane, send_as_user, write` | `core/src/session/send.rs:87` | 84 | `session send`: writes a `session-message` Event; wakes a stopped target | typing text into the pi pane (no send-keys) | KEPT | Message delivery is an Event, not keystrokes; Event stays |
| reload | `reload_due_with` | `core/src/session/reload.rs:122` | 14 | Reload waits CHAT_IDLE (10 min) when the Lead is in the chat slot | pane swapping | MOVED | Same rule, from the widget's focus |
| reload | `reload_with, stop_pi_with, may_exec` | `core/src/session/reload.rs:38` | 38 | Stops pi and re-execs `session run` after an install | respawn | MOVED | Needed with any runner that loads the adapter |
| run | `run (resume prompt, `host::input::read_line`)` | `core/src/session/run.rs:51` | 116 | Pane wrapper loop: runs pi, "Press Enter to resume here", exec after reload | respawn | MOVED | Supervision of the host process; Press-Enter prompt goes |
| input | `read_line, scope` | `core/src/host/input.rs:15` | 19 | Keyboard of a command that owns its terminal (`session run`) | typing text into the pi pane | MOVED | Prompt goes with the wrapper |
| archive | `archive_lead, archive_on_quit` | `core/src/session/archive.rs:136` | 77 | Parks the Lead holding the slot before archiving (:237) | pane swapping | MOVED | Park step DELETED, archive stays |
| archive | `refuse_chat_slot` | `core/src/plan/archive.rs:151` | 30 | Refuses archive when a Lead holds the slot (callers plan/cancel.rs:45, plan/delete.rs:414, context/delete.rs:66) | pane swapping | MOVED | Same refusal, no tmux read |
| titles | `chat_slot_width, run (width)` | `engine/src/watch/titles.rs:152` | 4 | E10 reads the chat slot's width for retitle (`--width`) | resize and padding | MOVED | Width comes from the widget |
| titles | `limit, prompt (retitle width)` | `core/src/session/retitle.rs:47` | 10 | Title length = slot width less margin | resize and padding | MOVED | Unchanged rule |
| liveness | `replace_with_placeholder` | `engine/src/watch/liveness.rs:505` | 10 | E2: a dead Lead pane in the slot becomes the placeholder (`respawn-pane -k`, `@cpd_view chat`) | view placeholders | DELETED |  |
| liveness | `stop_and_clean (slot branch)` | `engine/src/watch/liveness.rs:406` | 16 | E2: kills the window, replaces the pane in the slot | pane swapping | MOVED | Dead-process detection stays |
| sleep | `sleep (skip when User looks at it)` | `engine/src/watch/sleep.rs:94` | 27 | E14 does not sleep the Session in the chat slot | pane swapping | MOVED | Reads which Session the widget shows |
| ensure | `start (respawn of the engine pane)` | `engine/src/watch/ensure.rs:220` | 48 | Engine's own pane | respawn | KEPT | Not chat |
| archive_leads | `ready` | `engine/src/watch/archive_leads.rs:51` | 10 | Skips Leads refused with `in_chat_slot` | pane swapping | KEPT | Rule stays |
| cockpit | `build` | `tui/src/cockpit.rs:171` | 18 | Builds `main` with four panes incl. chat slot | view placeholders | KEPT | Chat pane remains as a view pane |
| cockpit | `add_missing_panes, split, remove_old_panes` | `tui/src/cockpit.rs:195` | 63 | Layout repair incl. "chat slot counts as present when it holds a Lead" | resize and padding | KEPT |  |
| cockpit | `reset_steps, reset_layout (chat slot never resized)` | `tui/src/cockpit.rs:310` | 17 | Resets pane sizes; the slot takes what is left | resize and padding | KEPT |  |
| cockpit | `respawn_live_views, respawn_dead, respawn_old_views` | `tui/src/cockpit.rs:425` | 52 | `--respawn`: restarts every live `@cpd_view` pane | respawn | KEPT |  |
| cockpit | `last_chat_lead, open_lead, open_lead_with` | `tui/src/cockpit.rs:467` | 30 | Opens the newest chat Lead into the new slot at start | pane swapping | MOVED | Becomes "show last Session" |
| cockpit | `main_views` | `tui/src/cockpit.rs:270` | 15 | `list-panes` with raw `@cpd_view` | view placeholders | KEPT |  |
| chat | `chat view (run, show, draw, render, rows)` | `tui/src/chat.rs:17` | 23 | The placeholder pane drawn while no Lead holds the slot | view placeholders | DELETED | Replaced by the widget's empty state |
| history | `history view (run, run_on, open, load, show_with)` | `tui/src/history/mod.rs:58` | 66 | Read-only viewer of an archived pi file in the slot | view placeholders | MOVED | Archived transcripts need a native reader |
| history | `draw (1-cell pad of the history view)` | `tui/src/history/view.rs:40` | 8 | Draws history with the chat slot's pad | resize and padding | MOVED |  |
| tree | `focus_chat` | `tui/src/tree/actions.rs:334` | 8 | `select-pane` on the chat slot so typing goes to the chat | mouse and focus forwarding | KEPT | The chat widget is still a pane that must be focused |
| tree | `command (Open/Park/History/free flows)` | `tui/src/tree/actions.rs:64` | 11 | Maps tree actions to `session open/park/history` and the focus step | pane swapping | MOVED | Park step of archive goes; open and history stay |
| tree | `free_for, close_of` | `tui/src/tree/view.rs:70` | 32 | Archive of the slot's Lead opens the next Session first; `x` refuses the slot's Session | pane swapping | MOVED | Same UI rule |
| widgets | `open_lead` | `tui/src/widgets/mod.rs:523` | 8 | Tab click: `session open` then `select-pane` on the chat slot | mouse and focus forwarding | KEPT |  |
| widgets | `click (inbox jump)` | `tui/src/widgets/inbox.rs:276` | 14 | Click on a Session row runs a command and focuses the slot | mouse and focus forwarding | KEPT |  |
| host_click | `run, host_manager, send` | `tui/src/host_click.rs:38` | 40 | Brings the host Manager into the slot and wakes it | pane swapping | KEPT | Calls `session open` |
| incident_click | `run` | `tui/src/incident_click.rs:18` | 9 | Opens an incident's Lead in the slot, sends nothing | pane swapping | KEPT | Calls `session open` |
| store | `chat_slot (read), manager_slot` | `tui/src/read/flow.rs:540` | 44 | Which Lead/Manager is in the slot, from `session-moved` Events (`place = chat`) | none (store only) | KEPT | Events stay |
| store | `unread` | `core/src/inbox/rules.rs:291` | 10 | White dot when a reply landed while the chat was not in the slot | none (store only) | KEPT |  |
| store | `snapshot_timed, concerns` | `tui/src/plan/mod.rs:156` | 20 | Plan view follows the Lead in the slot | none (store only) | KEPT |  |
| store | `selectable_name, context_text (blue name in the slot)` | `tui/src/tree/rows.rs:344` | 7 | Tree draws the slot's Session blue | none (store only) | KEPT |  |
| pi | `launch.rs: lead(), lead_env(), lead_candidates()` | `core/src/host/pi/launch.rs:117` | 50 | argv/env of a Lead or Manager pi | typing text into the pi pane (launch) | MOVED | Lead start moves to a host process; env contract stays |
| pi | `launch.rs: rest (worker, distiller, base_env, push_extensions, structs)` | `core/src/host/pi/launch.rs:1` | 168 | Worker and distiller pi launch; shared env | respawn | KEPT | Workers stay pi in tmux (non-test lines 1-218 less 50) |
| pi | `dirs.rs, packages.rs, session_file.rs, mod.rs (non-test)` | `core/src/host/pi/dirs.rs:1` | 345 | pi dirs, package identity, session file lookup | none | KEPT | Shared with Worker launch (85 + 201 + 34 + 25) |
| history | `history/render.rs (non-test)` | `tui/src/history/render.rs:1` | 65 | Pi session file to ratatui Lines (you/agent/tool blocks) | none | KEPT | Already a native ratatui reader of a pi file; the seed of a chat widget (201 lines with inline tests) |


**Findings on what tmux does for the chat today (all **read**):**

- **How a message reaches a Session: not `send-keys`.** `cpd session send` writes a `session-message` Event in the store (`core/src/session/send.rs:87`, `:166`); the adapter's follower delivers it to pi (**unverified**: I read the file names, not their bodies: `pi/lib/handle.ts`, `pi/lib/delivery.ts`). The User types straight into pi's pane: tmux forwards keys, and `focus_chat` (`tui/src/tree/actions.rs:334`, `select-pane`) only moves the focus. **ran** `git grep -n -E 'send-keys|send_keys' $H -- core/src engine/src cli/src tui/src | grep -v tests`: the only non-test `send-keys` is the mouse binding `select-pane -t = ; send-keys -M` at `core/src/host/tmux.rs:202` (mouse forwarding), and `session/run.rs:158` / `host/input.rs` read stdin for the "Press Enter to resume" prompt. No cpd code types text into pi. The "typing text into the pi pane" interaction therefore has no function to delete; what disappears is pi's own editor, which is `compact-tools-editor.ts`/`-cursor.ts` (section 1).
- **Bracketed paste:** **ran** the grep for `paste|bracketed` in non-test source: only `tui/src/plan/flow.rs:337` (a variable named `bracketed` about PR columns, unrelated). Nothing in cpd handles paste for the chat; pi and tmux do.
- **Spinner and status line:** the agent spinner (moon loader) is drawn inside pi by `compact-tools-loader.ts`; no tmux call is involved. The tmux status line (`core/src/host/tmux.rs:209-358`, 150 lines) holds the cockpit's buttons, not the chat's status, and stays.
- **What a chat-slot swap costs today:** **read** `core/src/session/moves.rs:180-257` `open` (77 lines) swaps the Lead's pane into the slot (`:234`), and `park`/`park_lead` (`:259-298`) build `park-<label>` windows with a placeholder (`:293-294`). Only two callers of `swap_pane` exist (**ran** `git grep -n 'swap_pane' $H -- core/src engine/src cli/src tui/src`: `moves.rs:234`, `:294`, plus the definition and fakes).
- **Test side (unverified how much is deleted):** the grep's 58 test, fixture and fake files (cli/tests, `*_tests.rs`, `fakes/`) total 30,536 lines and contain 258 lines matching `chat.slot|chat_slot|swap_pane|respawn_pane|@cpd_view` (**ran** `xargs wc -l` and `grep -c`); whole files, not the matching tests, so I do not call them deleted. The tmux fake `core/src/fakes/tmux/` has 3,425 lines in its top-level files (**ran** `wc -l core/src/fakes/tmux/*`; its `tests/` subdirectory is not counted) and stays while Worker panes use tmux. Inline tests of the files above: `tmux.rs` 369, `tmux/*_tests.rs` 454 (`pill_lines_tests` 292, `incident_tests` 91, `pill_tests` 28, `title_tests` 22, `no_inbox` 21), `host/tmux_adapter_tests.rs` 105, `host/tmux_runner_tests.rs` 119, `host/pi/*` 805 (`launch` 330 + `launch_no_mcp_tests` 144 + `dirs` 87 + `packages` 168 + `session_file` 32 + `plannotator_env_tests` 43 + `mod` 1), `layout.rs` 63, `moves.rs` 12 (**ran** the awk split).
- **Already in `cpd-tui`:** `tui/src/history/render.rs` (201 lines, 65 non-test, **ran** `awk` split) already renders a pi session file as ratatui `Line`s (**read** `tui/src/history/render.rs:1-12`); `tui/src/fold.rs` (130 lines) already draws the fold toggle that `pi/lib/render.ts` copies in TypeScript. Both are inputs for the widget Jobs, not deleted.
- **Sizes of the files in the Job's scope (non-test lines, **ran** awk split):** `tmux.rs` 1,227 (+369 test), `tmux/pill.rs` 31, `layout.rs` 103, `input.rs` 36, `host/pi/launch.rs` 218, `dirs.rs` 85, `packages.rs` 201, `session_file.rs` 34, `pi/mod.rs` 24, `session/moves.rs` 560, `session/history.rs` 49, `session/send.rs` 195, `tui/src/chat.rs` 38, `tui/src/cockpit.rs` 690.

## 3. Workarounds

Sizes are whole files **ran** `wc -l`; "Deleted or re-implemented" is my judgement from the file heads (**read**).

| Workaround | Where (non-test lines) | Tests (lines) | Deleted or re-implemented by a native widget? | Verified claim |
|---|---|---:|---|---|
| Prototype monkey-patches | `compact-tools-hook.ts` 47, `-custom.ts` 108, `-silent.ts` 495, `-activity.ts` 329, `-viewport.ts` 234, `-editor.ts` 231, `-cursor.ts` 128, `-grouping.ts` 487, `-thinking.ts` 385, `-compaction.ts` 91, `-italic.ts` 17, `lib/hint.ts` 85, `lib/pad.ts` 82 = 2,719 (the files whose job is a patch, whole files) | `ask-user-question/patches.test.ts` 83, `prototype-reload.test.ts` 110 (rest inside the 12,967) | **Deleted** as a mechanism: no pi classes to patch. The drawing behind most of them (rows, editor box, cursor, grouping, thinking fold) is **re-implemented**. `viewport`, `custom`, `hook`, `italic`, `pad` have no counterpart | **ran** `grep -rn 'hookMethod(' pi --include=*.ts \| grep -v '^pi/test' \| grep -v 'export function'` = **23 call lines** (`cursor` 3, `editor` 4, `custom` 5, `activity` 3, `grouping` 1, `silent` 1, `compaction` 1, `thinking` 1, `viewport` 1, `italic` 1, `lib/compaction-ratio.ts` 1, `lib/pad.ts` 1); plus `compact-tools-silent.ts` wraps `render` by hand per prototype (**read** `:293-311`, `ORIGINAL_RENDER`) and `lib/hint.ts` wraps `Theme.prototype.bg` and `Container.prototype.render` (**read** `hint.ts:43-59`), neither counted in the 23. `docs/research/prototype-patches.md` says 12 sites outside `thinking`, at base `09b72595` (2026-10-23): **stale**. `compaction-ratio.ts` (69) is a patch but not a TUI one: MOVED, not deleted |
| `/reload` handling | `lib/reload.ts` 300, `lib/reload-command.ts` 30, `core/src/session/reload.rs` 144 non-test = 474; plus `Symbol.for` state that survives a reload | `reload.test.ts` 639; `compact-tools/prototype-reload.test.ts` 110, `thinking-reload.test.ts` 130, `off-host-reload.test.ts` 102 | **MOVED, not deleted**: the adapter still has to be reloaded after an install on any runner (**read** `docs/research/pi-reload.md` M1: a reload runs in the adapter's own command). What a widget deletes is the survival of patched prototypes across a reload: 27 `Symbol.for` lines in 15 files (**ran** `grep -rn 'Symbol.for' pi --include=*.ts \| grep -v '^pi/test' \| wc -l` = 27, `grep -rln` = 15 files) | pi-reload.md and `core/src/session/reload.rs:122` agree on the 10-minute `CHAT_IDLE` rule (**read**) |
| Thinking-block fold and italics | `compact-tools-thinking.ts` 385, `compact-tools-italic.ts` 17, `lib/italic.ts` 4 = 406 | `thinking-*.test.ts` (7 files) 931, `italic-spans.test.ts` 60, `italic.test.ts` 11 = 1,002 | Fold: **re-implemented** (a ratatui block with a toggle). Italics hack (SGR 3 by wrapping `Theme.fg`, `compact-tools-italic.ts:14`): **deleted**, ratatui has `Modifier::ITALIC` (**unverified**: not checked against the crate here) | `thinking-live.md`: the pre-fix failure (no start-folded, no click, two reloads) was fixed in #457/#469 (**ran** `git log --oneline origin/main -- pi/compact-tools`, titles "Fix the Thinking block that a left click does not expand (#457)", "Italic Thinking… label, upright Thinking body (#469)") |
| Chat padding | `lib/pad.ts` 82 | `chat-pad.test.ts` 48, `chat-padding.test.ts` 57 (+ margins tests 413: `chat-margins` 160, `rail-margins` 115, `balloon-margins` 138) | **Deleted**: one `Padding` in a ratatui `Block` (the placeholder already does it: `Padding::uniform(1)`, **read** `tui/src/chat.rs:36`). | `chat-padding.md` says pi 1.3.0 has no padding option; `pad.ts:72` wraps `TuiAltScreen.prototype.setLayoutRoot` (**read**): consistent |
| Fold language | `lib/render.ts` 237 (draws every cpd message as a fold), fold-toggle config in `compact-tools-config.ts` (195) | `render.test.ts` 386, `folds.test.ts` 39, `pill*.test.ts` 284 | **Deleted and not re-implemented**: the cockpit's own `tui/src/fold.rs` (130 lines) is the original the TS copies; the widget calls `toggle()` | `fold-language-in-pi.md` is **stale**: it cites glyphs `F004B`/`F0063` and `fold.rs:52`, `render.ts:168` `MARGIN`; **ran** `grep -n 'F004B\|F0063' tui/src/fold.rs` (no match), `grep -n 'DOWN\|UP' tui/src/fold.rs` (`:38` `F0B26`, `:40` `F0B2C`, `toggle` at `:55`), `grep -n MARGIN pi/lib/render.ts` (no match) |
| Double-load guards | `compact-tools-double-load.ts` 76, `ask-user-question/double-load.ts` 68, the early returns in both `index.ts`/entry | `compact-tools/double-load.test.ts` 180, `ask-user-question/double-load.test.ts` 89 | **Deleted**: nothing to double-load once the vendored forks go | |
| Balloon cache | `lib/balloon.ts` 2 (a re-export), `compact-tools-balloon.ts` 49 (pure drawing), the cache itself in `compact-tools-silent.ts` (commit `f69f1eb0`, #572, +41 lines) | `balloon.test.ts` 292, `balloon-margins.test.ts` 138, `balloon-cache.test.ts` 228 | Cache: **deleted** (ratatui redraws from state; **unverified**: needs a measured frame budget, Jobs `widgets`/`crates`). Balloon drawing: **re-implemented** | **ran** `grep -n -i cache` on both = no match: the prompt's "balloon cache (`pi/lib/balloon.ts`)" is wrong; `balloon.ts` is 2 lines and the cache lives in `-silent.ts` |
| Spinner, clock, cursor | `compact-tools-clock.ts` 55, `-loader.ts` 43, `-cursor.ts` 128, `-runtime.ts` 295 = 521 | `animation-clock` 48, `cursor-blink` 243, `loader-frames` 29+8, `running-loader` 121, `working-pill` 109 = 558 | Clock, throttle, hookMethod plumbing: **deleted**. Moon loader frames and the blinking cursor: **re-implemented** (**unverified**: that the cockpit has a reusable loader; `docs/research/moon-loader-cost.md` (509 lines) measures one and I did not read it through) | `pi/lib/cursor.ts` (16 lines) is **not** a cursor hack: it is the Event cursor of the follower (**read** `cursor.ts:1-3`), KEPT |

## 4. Maintenance cost proxies

**ran** at `$H`, `git log --oneline $H --since=<date> -- <paths> | wc -l`. The repository's first commit is 2026-10-12 (**ran** `git log $H --format=%ad --date=short | tail -1`) and `$H` has 683 commits, so `--since=2026-09-01` and `--since=2026-10-01` give the same number: every commit is in both windows. I add `--since=2026-10-20` (385 commits of 683 in all paths) to show the recent rate.

| Area (paths) | Commits since 2026-09-01 | since 2026-10-01 | since 2026-10-20 |
|---|---:|---:|---:|
| `pi/compact-tools` | 36 | 36 | 36 |
| `pi/ask-user-question` | 1 | 1 | 1 |
| `pi/lib` | 103 | 103 | 69 |
| `pi/test` | 158 | 158 | 121 |
| `pi` (all) | 222 | 222 | 164 |
| `core/src/host/tmux.rs`, `core/src/host/tmux/` | 53 | 53 | 36 |
| `core/src/host/pi` | 13 | 13 | 11 |
| `core/src/host/layout.rs` | 6 | 6 | 1 |
| `core/src/session/moves.rs` | 16 | 16 | 2 |
| `core/src/session/history.rs` | 3 | 3 | 0 |
| `tui/src/cockpit.rs` | 23 | 23 | 15 |
| `tui/src/chat.rs`, `tui/src/history/` | 12 | 12 | 9 |
| Union of `pi` + the Rust paths above | 277 | | |
| Union of the pi-TUI files only (`compact-tools`, `ask-user-question`, `lib/{hint,pad,render,mentions,mentions-refresh,pill,italic,balloon}.ts`, `themes`) | 52 | | |

Share: 277 of 683 commits (41%) touch the embedded-chat area; 52 (8%) touch only files that exist to draw pi's TUI (**ran** the same command with those paths). **unverified**: a commit that touches `pi/` for a guard or an Event is counted too in the first row; the second row is the clean one.

**Research documents that exist only to work around pi's TUI** (**unverified** (titles and first lines of the candidates only), **ran** `ls docs/research | wc -l` = 182): 8 files, 1,560 lines:

| Document | Lines | Why it exists |
|---|---:|---|
| `prototype-patches.md` | 76 | first-load-wins audit of prototype patches |
| `pi-reload.md` | 217 | how the adapter reloads pi; patched prototypes after reload |
| `thinking-live.md` | 128 | thinking block click, reload, italics in a real pi |
| `chat-padding.md` | 127 | 1-cell pad of pi's fullscreen root |
| `fold-language-in-pi.md` | 121 | fold language copied into pi renderers |
| `message-time-display.md` | 220 | render-time timestamps in pi's chat |
| `ask-fork-design.md` | 162 | owning a fork of the `ask_user_question` widget |
| `moon-loader-cost.md` | 509 | CPU cost of the moon loader in a pi pane and in cockpit views |
| **Total (8)** | **1,560** | **ran** `wc -l` of the eight |

Partly about pi's TUI, not counted: `pill-spec.md` (273, three adapters incl. pi), `inbox-pane.md` (327, a tmux pane), `ask-limit-*.md`, `planning-ux-*.md` (the `ask_user_question` tool), `agent-comms.md` (125). **unverified**: classification by title and first lines only.

**Vendored forks** (**read** the two `NOTICE` files; **ran** `npm view <pkg> version time.modified`):

| Fork | Tracks | Upstream latest | Behind? |
|---|---|---|---|
| `pi/compact-tools/NOTICE` (17 lines) | `pi-compact-tools` release 0.16.6 at `8be1a8d`, via fork `clayton-duarte/pi-compact-tools` at `2c1fcad8` (adds `fold: "toggle"`); 28 of its 29 source files | `0.16.6` (modified 2026-10-19 per the registry) | no |
| `pi/ask-user-question/NOTICE` (15 lines) | `@juicesharp/rpiv-ask-user-question` v2.13.8 at `afc21bc` (`rpiv-mono`); 43 `.ts` files | `2.15.0` (modified 2026-10-24) | two minor versions; `pi/test/ask-user-question/notice.test.ts` (67) pins the NOTICE |

Both NOTICE files list the local changes (compact-tools: 6 bullets; ask-user-question: 7 numbered). `pi/compact-tools` has had 36 commits and `pi/ask-user-question` 1 commit since the fork (**ran** counts above): the ask fork is stable, compact-tools is not.

## 5. Re-verified counts and claims of the existing documents

| Claim (source) | Now | Evidence |
|---|---|---|
| 12 patch sites outside `thinking` (`prototype-patches.md`) | 23 `hookMethod(` call lines, + 4 hand-wrapped prototypes, + 2 in `hint.ts`; 3 of the doc's 12 "first-load-wins" defects: not re-checked (out of scope) | **ran** the commands in the table row above |
| "balloon cache (`pi/lib/balloon.ts`)" (this Job's prompt) | no cache in that file; cache in `compact-tools-silent.ts` since #572 | **ran** `grep -n -i cache` on the two balloon files: no match |
| "`cursor.ts` is a cursor hack" (prompt) | `lib/cursor.ts` is the Event cursor; the blinking cursor is `compact-tools-cursor.ts` | **read** `pi/lib/cursor.ts:1-4` |
| `fold-language-in-pi.md`: `fold.rs:52`, glyphs `F004B`/`F0063`, `render.ts:168` `MARGIN` | glyphs are `F0B26`/`F0B2C`, `toggle` at `fold.rs:55`, no `MARGIN` | **ran** the greps in section 3 |
| NOTICE: "the 43 .ts files" (ask) | 45 `.ts` files today: the 43 plus `double-load.ts` and `limit-text.ts`, which cpd added | **ran** `find pi/ask-user-question -name '*.ts' \| wc -l` = 45; **read** NOTICE change (4), (5) |
| NOTICE: compact-tools "28 of 29 source files" | 32 `.ts` under `extensions/` today, 4 more than the NOTICE's 28 (the NOTICE names `double-load` and `compaction` as new); `header.ts` sits beside them | **ran** `ls pi/compact-tools/extensions \| wc -l` = 32; **unverified** which other two are cpd's |

## 6. Totals

Non-test source in `pi/` (151 files; `wc -l` of every file listed in section 1, **ran** by summing, this equals `find pi -type f -not -path 'pi/test/*' -not -name '*.test.ts' | xargs wc -l` over those 151 files, 20,307 lines; it includes `NOTICE`, `LICENSE`, JSON and the skills' Markdown, 818 lines):

| Verdict | Files | Non-test lines | Of which behaviour re-implemented in ratatui | Mapped tests (lines) |
|---|---:|---:|---:|---:|
| DELETED, re-implemented (Y) | 54 | 11,459 | 11,459 | 1,421 |
| DELETED, workaround only (N) | 32 | 2,553 | 0 | 105 |
| **DELETED total** | **86** | **14,012** | 11,459 | 1,526 mapped + 12,967 compact-tools + 356 ask + 1,017 top-level = **15,866** test lines, plus 48,744 golden/fixture data lines |
| MOVED | 30 | 2,517 | | 2,642 mapped |
| KEPT | 35 | 3,778 | | 5,495 mapped (+ 858 `skill.test.ts`) |
| Total | 151 | 20,307 | | |

Rust measured rows of section 2 (71+ rows; these are the items I could tie to the chat slot, not all of the files):

| Verdict | Rows | Lines | Meaning |
|---|---:|---:|---|
| DELETED | 12 | 130 | `swap_pane`, `set_pane_history`, parked windows, placeholder spawn, `history`, placeholder view, `replace_with_placeholder` |
| MOVED | 26 | 907 | `session open/fork/park` bookkeeping, `run` wrapper loop, reload, titles width, history reader |
| KEPT (**Rust lines that stay**) | 34 | 1,356 | tmux options, views, hooks, status line, `respawn`, cockpit layout, `session send`, store reads, Worker launch |
| Total | 72 | 2,393 | |

Reading the totals:

- **ran** (sum): `pi/` loses 14,012 of 20,307 non-test lines (69%) under scenario A, and 15,866 test lines (+48,744 data lines). But 11,459 of the 14,012 (82%) are behaviour the widget must provide; only **2,553** non-test lines (13% of `pi/`) plus **105** mapped test lines are pure workaround with no counterpart. Counting the Rust side, **130** lines are deleted outright against **907** moved and **1,356** that stay. **The deletion in Rust is small: the tmux layer (`tmux.rs` 1,227 non-test lines) stays because views and Worker panes stay.**
- **unverified**: "Y" says the behaviour must exist, not that its line count carries over; Rust and ratatui widgets are shorter or longer than TypeScript on pi-tui. Cost estimates are other Jobs'.
- **unverified**: `lib/reload.ts` (300) and `core/src/session/reload.rs` are MOVED because I assume the host still reloads the adapter after an install; if the pi-durable host restarts instead, they join DELETED/N (474 more lines).

## 7. What is NOT deleted

- **Worker panes in tmux** and everything that serves them: `tmux.rs` (new_window, split_window, kill, list_panes), `core/src/driver/worker.rs`, `set_pane_session`/`set_pane_title`, the tmux fake (3,425 lines), `PANE_DIED_HOOK`, and `pi launch` for Workers (`core/src/host/pi/launch.rs` `worker()`, `distiller()`, `dirs.rs`, `packages.rs`, `session_file.rs`: 168 + 345 non-test lines KEPT).
- **The cockpit's other views** (tree, plan, widgets), the tmux status line, server options and mouse bindings (`tmux.rs:171-358`), `tui/src/cockpit.rs` (690 non-test lines).
- **The Inbox** and its widget code (`tui/src/widgets/inbox.rs`), and every store read of "which Session is in the chat" (`tui/src/read/flow.rs:540`, `pulse.rs:45`, `core/src/inbox/rules.rs:291`): they read Events, not tmux.
- **plannotator**: `lib/plannotator.ts`, `pnguard.ts`, `pntrack.ts`, `plan-review*.ts`, `review.ts`, `review-chat.ts` (MOVED or KEPT per the table; **unverified** whether a native dialog is needed for plan review).
- **The adapter's CLI glue**: follower, delivery, Event kinds (`PROTOCOL = 17`), guards (`mainguard`, `noblock`, `testguard`, `secretguard`, `askguard`, `loop-guard`), policy, Slack/Jira/model-list helper scripts. These are 3,778 KEPT lines plus 2,517 MOVED lines; **unverified**: how much of the glue is wired to pi's hooks (Job `durable-ui`'s question).
- **The `ask_user_question` tool contract**: `tool/*` (424 lines: types, normalisation, validation, answer format, envelope) and `limit-text.ts` stay as MOVED; only the 5,000-line dialog UI goes (and is re-implemented).
- **Prompts and skills** (`pi/skills`, 818 lines + 858 test lines).
- **`session send`** (195 lines): messages already travel as Events.
- **pi itself for Workers**, and its install and version handling, as long as Workers stay pi processes.
- **The history reader** `tui/src/history/render.rs` (65 non-test lines): it is a seed of the chat widget, not waste.

## 8. Conclusion

- A native ratatui chat deletes the *mechanism* of the embedded chat: **14,012 non-test lines in `pi/` and 15,866 test lines** (plus 48,744 lines of golden data), and **130 Rust lines**. That is the headline; it is also the number that overstates the saving.
- Of the 14,012, **11,459 lines (82%) draw things the User sees** (balloons, folds, rows, cursor, the questionnaire dialog); a native widget re-implements them. Only **2,553 lines of workaround (patches, double-load guards, caches, clock, reload survival, the `ask_user_question` RPC fallback and `reconcile`)** plus 130 Rust lines vanish with no counterpart.
- The Rust side stays mostly intact: 1,356 measured lines KEPT, 907 MOVED, 130 DELETED. The tmux layer is needed by Worker panes and the cockpit views; what goes is pane swapping (`swap_pane`, parked windows, placeholder, history pane).
- The recurring cost is real: 277 of 683 commits (41%) touch the embedded-chat area since 2026-10-12, 52 (8%) touch only pi-TUI drawing files; 36 commits changed `pi/compact-tools` in the last 8 days, and 8 research documents (1,560 lines) exist only to work around pi's TUI.
- **Prompt claims that were wrong:** no cache in `pi/lib/balloon.ts`; `pi/lib/cursor.ts` is not a spinner or cursor hack; the patch-site count is now 23 `hookMethod` calls (+6), not 12; `fold-language-in-pi.md` cites stale glyphs and lines.
