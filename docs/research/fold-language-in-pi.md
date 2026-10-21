# The cockpit's fold language in the pi chat: every folding renderer and a route for each

Plan `pi-pin`, Job `ft-research` (hs-d484 part 2; applies hs-d449 inside pi). Base: `main` at `829e79d1`. Research only: no code changed, no upstream issue, PR or fork opened.
Read on 2026-10-20: pi `1.3.0` (`~/.pi/agent/install/releases/1.3.0/node_modules/@earendil-works/pi-coding-agent`, `dist/` paths below are under it), `npm:pi-compact-tools@0.16.6` (`~/.pi/agent/npm/node_modules/pi-compact-tools`, `extensions/` paths below), `gh` and `npm view` for the repo. Everything is from reading source; **I did not draw any row in a live pi pane** (unverified by running).

## 0. The target: the cockpit's own toggle

One widget draws it, `tui/src/fold.rs`; the tree and the Inbox both call it.

| Part | Where | What |
|---|---|---|
| Closing row | `tui/src/fold.rs:52` `toggle(expanded)` | `format!("╰{arrow} {word}")`: `╰`, the arrow **straight against it**, **one** space, the word. Folded `╰󰁋 expand` (`DOWN`, `\u{F004B}`, `fold.rs:35`), expanded `╰󰁣 collapse` (`UP`, `\u{F0063}`, `fold.rs:37`). |
| Colour | `fold.rs:39` `muted()` = `palette::fg(Meaning::Nothing)`; `tui/src/palette.rs:85` `Meaning::Nothing => Color::DarkGray` | The **whole row** is muted (corner, arrow, word); not bold. |
| Margin | `tui/src/tree/view.rs:949-951` (`" ╰\u{F004B} expand"`, `" ╰\u{F0063} collapse"`); `tui/src/tree/look.rs:109,138` | One leading space in the tree column, which is pi's `MARGIN = " "` (`pi/lib/render.ts:168`). |
| Title / rail | `fold.rs:44` `title`, `fold.rs:11-12` `BRANCH = "│"`; the Inbox rows are `│ <icon> <item>` (`tree/inbox.rs:1-4`) | `╭` + bold title; content rows led by a muted `│ `. |
| Nothing to fold | `fold.rs:59` `plain()` | `╰─` and no word. |
| Spec | `docs/commands.md` §17 "Collapsible blocks", closing row at line 2931 | |

So in pi the row to draw is `" ╰󰁋 expand"` / `" ╰󰁣 collapse"` (margin, `╰`, arrow, space, word), `theme.fg("muted", …)` over all of it, with `\u{F004B}` / `\u{F0063}` written as code points.

## 1. Every message that folds in hs's pi chat

"Folds" = has a collapsed and an expanded look the user switches with Ctrl+O (`app.tools.expand`, `core/keybindings.js:37`) or a click. pi toggles all of them from one place: `toggleToolOutputExpansion` → `setToolsExpanded` (`modes/interactive/interactive-mode.js:3737-3754`), which calls `setExpanded` on every child that has one. **No pi setting or theme token controls the fold row's glyphs or shape**: `docs/settings.md` has only `collapseChangelog` (a startup notice), `hideThinkingBlock`, and the theme (`docs/themes.md:110-111`) only colours (`customMessage*`, `thinkingText`, `toolTitle`, `toolOutput`, …). The hint text is `keyHint(...)` (`components/keybinding-hints.js:30`), a dim key name plus a muted description, hard-coded.

`~/.pi/agent/settings.json` packages: `pi-ghostty-theme-sync`, `@plannotator/pi-extension@0.28.17`, `@juicesharp/rpiv-ask-user-question@2.13.8`, `pi-compact-tools@0.16.6`. hs has **no** `~/.pi/agent/compact-tools.json`, so compact-tools runs its defaults (`compact-tools-config.ts:16-35`): `style: "compact"`, `tools: [read, write, edit, bash]`, `custom_tools: {enabled: true, auto_compact: true}`.

### 1.1 cpd's adapter

| Renderer | Where | What it draws now | What configures it |
|---|---|---|---|
| `cpd`, `cpd-context`, `cpd-plan-decision` messages | `pi/lib/render.ts`: constants `:167-173`, `rowsOf` `:178-188` (the toggle at `:187`), `Rendering.render` `:205-238`, click `new MouseRegion` `:233`; registered at `pi/lib/adapter.ts:22` | `╭⦁ <bold title>`, ` │ ` content, ` ╰─ expand` / ` ╰─ collapse`, all muted | Only `style: off` of `compact-tools.json` (read at `render.ts:21-63`), which returns `undefined` and hands the message to pi's own box (`custom-message.js`, no fold). Nothing else. |

Change for part 1 (not part 2, listed so nothing is missed): `CLOSE = "╰─"` becomes `╰` plus the arrow; `render.ts:11` header comment; tests `pi/test/render.test.ts:44,52-74,218-231`; docs `docs/commands.md:2761,2765,2768,2771-2772,2774,2776` (§15.9 still says `╰─ expand`; lines 2771-2772 also describe `claude` / `codex` shapes that `render.ts` does not draw: it has one shape for every style but `off`).

### 1.2 pi-compact-tools 0.16.6 (installed: `~/.pi/agent/npm/node_modules/pi-compact-tools`)

The extension replaces the tool renderers (`registerTool` overrides) and patches pi's classes with `hookMethod` (`compact-tools-hook.ts`: tool-row `updateDisplay` `compact-tools-custom.ts:52`, chat `render` `compact-tools-grouping.ts:307` and `compact-tools-silent.ts:187`, `doRender` `compact-tools-viewport.ts:176`, user message and editor gap `compact-tools-activity.ts:180-217`). Its devDependency is pi `1.3.0`, peer `>=0.85.1`, so it matches hs's pi.

**Style `compact` (hs's style).** What a row is, from source:

| Part | Where | Draws |
|---|---|---|
| Head | `compact-tools.ts:220` `renderRowCallContent` → `:236` title, `renderToolCall` `compact-tools-layout.ts:275-288` (`firstPrefix = " "` at `:282`, continuation ` │ ` `:279`) | ` ⦁ <bold name> <details>`; the indicator is `⦁` (`compact-tools-core.ts:27-28` `indicatorGlyph`), coloured by status; wrapped lines behind ` │ ` |
| Body (expanded, or preview when `auto_compact` is false) | `compact-tools.ts:415` `renderRowResult`, `:456-460`; rail ` │ ` in `renderOutput` `compact-tools-layout.ts:294`, `railComponent` `:779`, `renderArguments` `:787` | ` │ <output line>` |
| Closing row | `compact-tools.ts:177` `renderControls`, **prefix `" └ "` at `:185`**, called at `:462` | ` └ Done in 0.3s (12 lines)` / ` └ Failed in …` / running timer. **It is a status line, not a toggle: there is no `expand` / `collapse` word anywhere in the compact style.** Expanding is click or Ctrl+O only (`README.md`: "Click a row/group or press `Ctrl+O`"). |
| Toggle state | `compact-tools-runtime.ts:158-170` `syncExpansion`; click is pi's `createResultRegion` (`components/tool-execution.js:112-118`) | |

So the compact row already has the cockpit's rail (` │ `, same margin and glyph as `render.ts` `MARGIN + RAIL`) and the same `⦁`, but has **no `╭` and no toggle row**: it closes with the metadata line `└ Done …`.
Which rows this covers in hs: `read`, `write`, `edit`, `bash` (`tools`), plus **every custom tool** (`custom_tools.enabled`): `cpd_plan_review` (`pi/lib/review.ts:173`, no `renderCall` / `renderResult`), the plannotator and ask-user-question tools (no renderers either: `rg renderCall|renderResult` finds none in `@plannotator`, `@juicesharp`), and `codemode` (a special case, `compact-tools.ts:431`).

**Style `claude`** (not hs's): head ` ⦁ Bash(…)`, result under `   ⎿  ` (`compact-tools-claude.ts:29`), `… +N lines (ctrl+o to expand)` (`compact-tools.ts:311` `claudeMoreLines`), group summary line ending `(click to expand)` (`compact-tools-grouping.ts:46,423`).
**Style `codex`** (not hs's): ` • Ran …` under `   └ ` (`compact-tools-codex.ts:14`), `(ctrl+o to expand)` hint and `− Show less` (`compact-tools-codex.ts:46-51`), `• Explored` groups (`compact-tools-grouping.ts:436`).
**Thinking block** (all styles but `off`; Ctrl+T, click): `compact-tools-thinking.ts` `renderThinkingSections` `:150-185`: summary line, `│` detail connector, and a control row `└ ctrl+t toggle • click to hide` (`:154,168`, the text at `:236`). Three views (hidden / summary / detail), not two.
**Silent mode** hides rows; nothing folds.

What configures all of it: `compact-tools.json`, global `~/.pi/agent/compact-tools.json`, project `<cwd>/.pi/compact-tools.json` when trusted (`compact-tools-config.ts:150-157`). Keys (`:16-35`, `schemas/compact-tools.schema.json`, `additionalProperties: false`): `mode`, `style`, `tools`, `custom_tools`, `previewLines`, `auto_compact`. **None sets a glyph, a corner or a toggle row.** (The schema file also rejects unknown keys, so a cpd-only key cannot be smuggled in.)

### 1.3 pi's own built-ins (what remains when a tool is not in `tools`, or under `style: off`)

All of these fold with the same click / Ctrl+O, and none draws a row in the cockpit's language: each is plain text under the content, a dim/muted hint, not a corner row, and no theme or setting changes it.

| Renderer | File:line | Draws now | Configures it |
|---|---|---|---|
| `grep` result | `core/tools/renderers/grep.js:29-39` `formatGrepResult` (15 lines) | `… (N more lines, ctrl+o to expand)` | none in pi; **`compact-tools.json` `tools` can take it over** |
| `find` result | `core/tools/renderers/find.js:27-37` (20 lines) | same hint | same |
| `ls` result | `core/tools/renderers/ls.js:21-31` (20 lines) | same hint | same |
| `read` result and call | `core/tools/renderers/read.js:82-96` (10 lines); compact call for skills / pi docs `:68-80` `formatCompactReadCall` | same hint; ` (ctrl+o to expand)` after the path | `tools` (hs has `read` in it) |
| `write` call | `core/tools/renderers/write.js:86-106` | `… (N more lines, T total, ctrl+o to expand)` | `tools` (in it) |
| `edit` | `core/tools/renderers/edit.js:55,137` | diff in a box; no hint of its own | `tools` (in it) |
| `bash` result | `core/tools/renderers/bash.js:48-49` via `VisualLinePreview` (`components/visual-truncate.js:38-60`) | `… (N earlier lines, ctrl+o to expand)` | `tools` (in it) |
| Generic tool (no renderer) | `components/tool-execution.js:96-110` `createResultFallback` | `… (N more lines, ctrl+o to expand)` | `custom_tools` takes it over; native under `style: off` |
| User `!cmd` | `components/bash-execution.js:88-150` (`:143` `to collapse`, `:146` `… N more lines (ctrl+o to expand)`) | muted status line | **none; compact-tools does not hook it** |
| Compaction summary | `components/compaction-summary-message.js:29-52` | `[compaction]` label, `Compacted from N tokens (ctrl+o to expand)`, click | none; created by `interactive-mode.js:3175` |
| Branch summary | `components/branch-summary-message.js:29-52` | `[branch]`, `Branch summary (ctrl+o to expand)` | none; `interactive-mode.js:3182` |
| Skill block (`/skill:`) | `components/skill-invocation-message.js:30-52` | `[skill] name (ctrl+o to expand)` | none; `interactive-mode.js:3198` |
| Thinking (native) | `components/assistant-message.js:109-124` | `Thinking...` label (hs: `hideThinkingBlock: true`), click flips it; no fold row | `hideThinkingBlock`, `setHiddenThinkingLabel`; replaced by compact-tools' transformer while it is on |
| Built-in extensions' tools | `extensions/mcp/tools.js:299`; `extensions/codemode/renderer.js:20,94` | `… (N more lines / earlier calls, ctrl+o to expand)` | covered by compact-tools' custom / `codemode` rows unless `off` |
| Startup header, loaded-resources sections | `interactive-mode.js:102-113,759,1345` (`ExpandableText`) | `Press ctrl+o to show full startup help` | `quietStartup` (hs: true); not a chat message, out of scope |
| Custom entries | `components/custom-entry.js` (extension `registerEntryRenderer`) | nothing registered in hs | n/a |

### 1.4 Other packages and local extensions

- `@plannotator/pi-extension`: no message or tool renderer (`registerMessageRenderer`, `renderCall`, `renderResult` absent; only `ctx.ui.notify` / `setWidget`). Its tool rows are drawn by compact-tools' custom path. No fold of its own.
- `@juicesharp/rpiv-ask-user-question`: its `expanded` is a questionnaire state (`state/questionnaire-session.ts`), an interactive dialog, not a chat fold; the tool row is a compact-tools custom row.
- `@ogulcancelik/pi-ghostty-theme-sync`: theme only.
- `~/.pi/agent/extensions/{cpd-aux-agents,herdr-agent-state,notify}.ts`: none registers a renderer.

## 2. pi-compact-tools: upstream facts

All from `gh` and `npm view`, 2026-10-20.

- **Repo** `nedleeds/pi-compact-tools`, created 2026-09-12, default branch `main`, not archived, 11 stars, 1 fork (`tanavamsikrishna/pi-compact-tools`, diverged 1 ahead / 2 behind; not inspected). npm `pi-compact-tools`, one maintainer (`nedleeds`, Dong Hyeong Lee).
- **License: MIT** (`LICENSE`, `package.json`; GitHub `mit`). Forking and pinning is allowed.
- **Activity**: 40 npm versions in 38 days (0.3.1 on 2026-09-13 to 0.16.6 on 2026-10-19); last push 2026-10-19, the day before this note; releases are one commit each ("Release v0.16.6: …") straight on `main`, 70 commits, all by the owner. Latest GitHub release `v0.16.6`, 2026-10-19. Outside contributions: **0 pull requests ever** (open or closed); **1 issue ever** (#1 "truncating issue" by a user, 2026-09-23), which the owner answered and fixed in **v0.10.0 the next day** (closed 2026-09-24). So: very active, quick on a report, but there is **no evidence yet about how a PR is treated**; no `CONTRIBUTING`, `.github/` holds workflows only. The repo has tests (`npm test` = `tsx --test tests/*.test.ts`) and a JSON schema that a config change must update.
- **How a fold row is drawn**: §1.2.
- **Config loader**: `compact-tools-config.ts` `mergeConfig` `:121-144` (reads `mode`, `style`, `tools`, `auto_compact`, `custom_tools`, `previewLines`; unknown keys are ignored in code and rejected by the schema), `loadConfig` `:150-157`. Types `compact-tools-types.ts:23-31`. No fold or glyph option exists.
- **Smallest upstream change** (a sketch, not done; compact style only, the one hs uses):
  - Config: one key, `fold: "status" | "toggle"` (default `"status"` = today, so no behaviour change for anyone), and, because upstream cannot assume a Nerd Font, `toggle: { expand: string, collapse: string }` glyph overrides with a plain default (say `▸` / `▾`). Files: `compact-tools-types.ts` (two fields), `compact-tools-config.ts` (`DEFAULT_CONFIG`, `mergeConfig`, a `parseChoice` call for `fold` and a string check for the two glyphs), `schemas/compact-tools.schema.json`, `examples/`, README row.
  - Drawing, when `fold` is `"toggle"` (about four sites): (1) `renderToolCall` `compact-tools-layout.ts:282` takes `╭` in place of the leading space of the head; (2) `renderControls` `compact-tools.ts:177-204` returns ` ╰<glyph> expand|collapse` (muted, `chromePainter`) in place of ` └ Done …`, drawn once the row `hasResult` (`RowState.hasResult`, `compact-tools-types.ts:37`), and none while running or with no body; the body rail ` │ ` is already the cockpit's; (3) `renderRowResult` `:456-463` passes `state.expanded` to it; (4) tests in `tests/` for both states.
  - dotfiles then set `{"fold":"toggle","toggle":{"expand":"\u{F004B}","collapse":"\u{F0063}"}}` in `~/.pi/agent/compact-tools.json` (the file does not exist today).
  - **Open question for the Lead / User**: the ` └ Done in 0.3s (12 lines)` metadata has no place in the cockpit's closing row. Options: drop it in `toggle` mode; or keep it on the head row after the title; or append it after the toggle (`╰󰁋 expand · Done in 0.3s`), which changes the exact cockpit row. Not decided here.
  - The thinking block's `└ ctrl+t toggle • click to hide` (`compact-tools-thinking.ts:168`) would take the same key if the User counts thinking as a folding message; it has three views, so it needs its own decision.

## 3. Proposed route per renderer

Order of preference from hs-d484: configuration, else upstream PR, else pinned fork. Package pins are in `~/.pi/agent/settings.json` (`packages`), changed by dotfiles; pi accepts a pinned git source (`docs/packages.md:34-38`: `git:github.com/<owner>/pi-compact-tools@<sha>`: tags and commits are pinned and updates do not move the ref).

| # | Renderer | Route | Reason | What dotfiles changes |
|---|---|---|---|---|
| 1 | cpd adapter `render.ts` | **In-repo edit** (part 1 of hs-d484; none of the three routes) | cpd owns the file | nothing |
| 2 | compact-tools compact-style rows: `read`/`write`/`edit`/`bash` and all custom tools (incl. `cpd_plan_review`, plannotator, ask-user-question, codemode, MCP) | **Upstream PR** to `nedleeds/pi-compact-tools` (the `fold` / `toggle` sketch in §2); **pinned fork** as the fallback if it is not merged and released in the Lead's window | Configuration cannot do it: no key touches the corner or the closing row. Upstream is MIT, released several times a week, answered its one issue in a day, has no CONTRIBUTING and has never had a PR (risk: unknown PR policy, so ask before investing, e.g. an issue first, still the Lead's call). The change is small, off by default and additive, which suits a maintainer of a style-switching tool. A fork carries 7.6k lines of TypeScript plus its tests and must be rebased at every bump (releases come every 1-5 days); a commit pin means it only moves when someone bumps it. | After a release: `packages` entry `npm:pi-compact-tools@<release with fold>`, and a new `~/.pi/agent/compact-tools.json` with the two keys. Fork: `packages` entry `git:github.com/<fork>/pi-compact-tools@<sha>` plus the same file. |
| 3 | compact-tools `claude` and `codex` styles, groups | **Not applicable** | hs runs `compact`; these draw Claude Code's `⎿` and Codex's `•`/`└` on purpose, and `render.ts` already draws one shape for every style but `off`. Revisit only if hs switches style. | nothing |
| 4 | compact-tools thinking block | **Not applicable until the User says thinking counts**; if it does, **same upstream PR** | it folds in three views with its own `ctrl+t` hint, hs hides thinking (`hideThinkingBlock: true`), and it is not a "message" in the cockpit sense | nothing now |
| 5 | pi native `grep`, `find`, `ls` tool rows (and `read`/`write`/`edit`/`bash` if they leave `tools`) | **Configuration**, then covered by #2 | They fold with a muted `… (N more lines, ctrl+o to expand)` hint, not a fold row, and pi has no knob. But `compact-tools.json` `tools` accepts `grep`, `find`, `ls` (`compact-tools-types.ts:3`, default `auto_compact: true` for each), which puts them into the compact rows that #2 restyles. | `~/.pi/agent/compact-tools.json`: `{"tools":["read","write","edit","bash","grep","find","ls"]}` (needs a decision: it also changes how those results look today) |
| 6 | pi native compaction summary, branch summary, skill block | **Not applicable**; leave native | Built-in message classes (`interactive-mode.js:3175-3198`), not `registerMessageRenderer` types; no setting or theme controls them. Only a patch of pi itself (pi is a pinned binary, hs-d395) or a compact-tools fork that hooks those prototypes (as `compact-tools-hook.ts` does for tool rows; feasibility unverified) could change them. They are rare (compaction, `/tree` summary, `/skill:`). Flag to the Lead as a conscious gap. | nothing |
| 7 | pi native user `!cmd` (`BashExecutionComponent`) | **Not applicable**; leave native | not hooked by compact-tools, no knob, same reasons as #6 | nothing |
| 8 | Built-in extensions' tool rows (MCP, codemode) and the generic fallback | **Covered by #2**; native only under `style: off` | compact-tools' `custom_tools` and `codemode` rows replace them | nothing |
| 9 | plannotator, ask-user-question, ghostty-theme-sync, local extensions | **Not applicable** | no fold renderer of their own (§1.4) | nothing |
| 10 | pi startup header and resources, native thinking label | **Not applicable** | not chat messages (`quietStartup`), or no fold row (`hideThinkingBlock`) | nothing |

**Recommended order for the Lead:** (1) ship part 1 in cpd; (2) ask upstream (the user-visible rows are all one change, #2); (3) decide the metadata question and #5 (a `tools` list change) with the User before the PR, since they change what the row says; (4) fork only if upstream has not merged by a date the Lead names. No action on #6-#10 unless the User wants them.
