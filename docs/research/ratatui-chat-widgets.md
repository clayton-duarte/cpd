# Ratatui chat: widgets, risks and Rust estimates

Evidence file `widgets` of the Plan `ratatui-chat-research` (decision pro-d166). Written 2026-10-28 EDT in worktree `cpd.ratatui-chat-research--widgets-a1`, cut from `origin/main` at `d943c49c` (**ran** `git log --oneline | head -2`). Research only: no product file changed. Crates are not surveyed here: a candidate is named only as "see crates.md". Every claim carries **read** (file and line opened), **ran** (command and output) or **unverified** (inferred, with why).

## 1. Answer in six lines

1. **MVP** (read-only chat view plus a one-line input): about **1,500–2,550 lines** of production Rust, about **2,700–5,700 lines** with tests, in **4 PRs**. Basis: section 6 (**read**, arithmetic in section 6).
2. **Full parity** with today's pi chat: about **7,150–12,450 lines** of production Rust, about **12,800–27,900 lines** with tests, in **16–18 PRs**. Basis: section 6.
3. **The spec's premise about scrollback is stale.** The User's pi chat is already a fullscreen alt-screen TUI that captures the mouse and does its own text selection and copy (section 5, R1). A native ratatui chat does not lose native scrollback: it has to re-implement pi's own selection (component 8, 400–700 lines).
4. **The largest items are the markdown renderer with no parser crate (1,400–2,500), the multi-line editor (1,700–2,900) and the `ask_user_question` modal (1,200–2,000).** All three are bigger than any single view in `tui/` today (`tree/view.rs` is 597 production lines, **ran** section 3 command M3).
5. **A chat pane (`cpd tui view chat <session>`) removes almost nothing from `core/src/host/tmux.rs`** (component 9): messages are not delivered through `send-keys` today, and `swap-pane` stays. What goes away is the pi launch and the monkey-patching in `pi/`.
6. **No benchmark was run.** Long-transcript performance is argued from the code and from existing docs; timings are **unverified** (R2).

## 2. What today's pi chat shows and does (the parity target)

All **read**, file:line cited. "compact" is the one display style cpd uses (`pi/compact-tools` also has `claude`, `codex` and `off` styles, `compact-tools-claude.ts`, `compact-tools-codex.ts`: not in scope, **unverified** that cpd never selects them: `pi/lib/render.ts:204` treats `off` as a pass-through).

**The chat (transcript):**

| Feature | Where it is drawn today |
|---|---|
| User balloon (right-aligned rounded box, borders colour 8) and agent balloon (left, with the agent's machine-local time in the bottom border) | `pi/compact-tools/extensions/compact-tools-balloon.ts:29-49`; `pi/lib/balloon.ts:2` is a one-line re-export; called from `compact-tools-silent.ts:91` |
| Markdown in assistant messages: headings, lists, code blocks with syntax highlight, tables, strikethrough, LaTeX | pi-tui `components/markdown.js` (`marked`, 820 lines, `:1`), highlight through pi-coding-agent `theme.js:752` `highlightCode` |
| Thinking blocks as folds (`hidden`, summary = folded, detail = expanded; click or Ctrl+T toggles) | `compact-tools-thinking.ts:1-14` (385 lines) |
| Tool-call rows: `╭─` head, `│ ` rail, `╰󰬦 expand` toggle, per-tool summaries (read, write, edit, bash, grep, find, ls, MCP, codemode), running / done / failed states, timers | `compact-tools-layout.ts:1-10` (571 lines), `compact-tools-invocation.ts` (531), `compact-tools-rendering.ts` (86), `compact-tools-shell.ts` (277: a command that only reads folds into a "looked around" group) |
| Tool groups ("looked around"), silent mode that hides tool rows, nested-call summaries | `compact-tools-grouping.ts` (487), `compact-tools-silent.ts` (460), `compact-tools-nested.ts` (168) |
| Compaction fold | `compact-tools-compaction.ts:1-12` (91 lines) |
| cpd messages (`customType` cpd, cpd-context, plan-review decision and summary) as one fold with a magenta `φ` pill, a one-sentence summary, a body and a time | `pi/lib/render.ts:12-236` (237 lines) |
| Mention highlighting: whole tokens that are a Session id, its 8-char prefix, a Plan or Context id become magenta italic, only in Lead and Manager panes, not in code | `pi/lib/mentions.ts:7-63` (the spec calls this "@file mention completion": it is not, see section 7) |
| Spinner (28 moon phases, 50 ms), working words, tool progress messages, the "light" activity line | `compact-tools-loader.ts:1-8`, `compact-tools-progress.ts`, `compact-tools-activity.ts:63` |
| Scroll: wheel, scrollbar, follow-the-end, a scroll position that survives an expand above the viewport | pi-tui `components/scroll-view.js:15-194`; `compact-tools-viewport.ts:79-165` (anchor search by matching lines) |
| 1-cell pad on all four sides | `pi/lib/pad.ts:26-58` |
| Footer: cwd, token and context usage, git branch, extension statuses | pi-coding-agent `components/footer.js:44-118` (264 lines; the cpd compact style does not change it, **unverified** that cpd shows nothing else) |
| Images | pi-tui `components/image.js` (122 lines) and `terminal-image.js` (602) exist; `compact-tools-invocation.ts:483` only tests for an image part. **Unverified** that a cpd chat ever shows one (not opened: no session file with an image part was searched) |

**The input editor** (pi-tui `components/editor.js`, 2,043 lines, drawn as a rounded box by `compact-tools-editor.ts:1-14`, 231 lines):

| Feature | Evidence |
|---|---|
| Multi-line text, wrap, cursor, fake cursor markers, blinking cursor in a palette index | `editor.js`; `compact-tools-cursor.ts:1-12` (128 lines) |
| Undo | `editor.js:597` (`tui.editor.undo`) |
| Word motions including CJK breaks, jump-to-character | `word-navigation.js` (95 lines); `editor.js:153`, `:795` |
| Kill ring | `kill-ring.js` (43 lines), `editor.js:3` |
| Input history (100 entries, draft kept) | `editor.js:317`, `:341` |
| Bracketed paste, large pastes collapsed to a `[paste #n]` marker | `terminal.js:128` (`?2004h`), `editor.js:15`, `:570`, `:579` |
| Slash-command and `@file` completion popups (the file search spawns the external `fd`) | `autocomplete.js:157` (`spawn(fdPath…)`), `:206` `CombinedAutocompleteProvider`, `:301`, `:315` (692 lines) |
| External editor | `interactive-mode.js:5739` (`app.editor.external`), `ask-user-question/state/external-editor.ts` (62 lines) |
| Focus colour: working 6, focused 12, unfocused 15 | `compact-tools-editor.ts:5-9` |

**The `ask_user_question` modal** (`pi/ask-user-question/`, a vendored fork): tabs, single and multi-select option lists, previews rendered as markdown, inline free text, notes, a submit tab, an external editor. **Ran** `find pi/ask-user-question/view -name '*.ts' | xargs cat | wc -l` → 2521; the same for `state` → 1841; for the whole directory → 5892.

**Plan review**: in chat it is a three-option select (Approve, Change, Open full plan) and an editor for Change (`pi/lib/review-chat.ts:5-16`, 71 lines); plannotator is a browser page opened by `pi/lib/plannotator.ts` (78 lines), tracked by `pntrack.ts` (157) and guarded by `pnguard.ts` (40).

## 3. What `tui/` has, measured

All commands **ran** from `tui/src` in the worktree, 2026-10-28 EDT.

| Id | Command | Output |
|---|---|---|
| M1 | `find . -name '*.rs' \( -name '*_tests.rs' -o -name tests.rs \) \| xargs cat \| wc -l` | 12729 (external test files, 91 files) |
| M2 | A script that counts, per non-test file, the lines before the first `#[cfg(test)]` followed by `mod tests {`, and the lines after it | production 16019, inline tests 7147, external tests 12729 |
| M3 | Same script, per file (production / inline-test lines) | `fold.rs` 81/49; `input_box.rs` 103/0; `history/render.rs` 65/136; `history/view.rs` 49/93; `history/mod.rs` 140/0; `scroll.rs` 135/52; `dialog/mod.rs` 577/0; `dialog/view.rs` 712/0; `dialog/scroll.rs` 106/138; `dialog/validate.rs` 54/51; `tree/view.rs` 597/1364; `tree/rows.rs` 690/898; `tree/folds.rs` 141/133; `link.rs` 136/0; `box_widget.rs` 179/0; `cells.rs` 38/0; `view_ports.rs` 223/0; `pump.rs` 259/0 |
| M4 | `find . -path './dialog/*' -name '*.rs' \| xargs wc -l \| tail -1` | 3711 (all of `dialog/`, tests included); external tests in it 1891 |
| M5 | `find . -name 'input_box*_tests.rs' \| xargs wc -l \| tail -1` | 236 |

**The repo's test ratio.** Tests over production lines in `tui/src`: external `*_tests.rs` files 12729 / 16019 = **0.79**; with the inline `mod tests` blocks added, (12729 + 7147) / 16019 = **1.24** (**ran**, M1 and M2). A naive "test files over all other lines" gives 12729 / 23166 = 0.55, which is wrong because the "other" lines include inline tests (**ran**: `A=$(find . -name '*.rs' | xargs cat | wc -l)` → 35895, minus 12729 = 23166). This file uses **0.79 for the low end and 1.24 for the high end**.

**What `tui/` already has for a chat**, by file (**read**):

| Piece | File:line | Fit |
|---|---|---|
| History view: renders a pi session file as lines | `history/render.rs:19-64`; `history/view.rs:17-48`; `history/mod.rs:58-63` | The only transcript renderer. See "what History lacks" below |
| Wrap and scroll with follow-the-end | `scroll.rs:19-80` (`Scroller`), `:84-134` (`wrap`); `dialog/scroll.rs` (`Scroll`: `by`, `offset`, `max_offset`, `key`, `mouse`) | Reusable for key and wheel handling; the wrapping is per character and not per block (R2) |
| Fold look | `fold.rs:47-80` (`title`, `toggle`, `plain`, `content`) | The same glyphs and colours as pi's folds (`render.rs:148-157` quotes `tui/src/fold.rs`). Builds `Line`s only: it has no state, no hit-test, no body wrapping |
| Input box | `input_box.rs:94-103` (`boxed`), `:29-45` wrapping, `:49-53` `cursor_at` | One cursor, no selection, no undo, no word motion, no history. Cursor arithmetic is `before / tw`, i.e. a fixed-width model that already handles wide characters (`cells.rs:12-17`) |
| Width arithmetic | `cells.rs:1-38` | Wide characters take 2 cells through `ratatui` `Span::width` (unicode-width, a transitive dependency: **ran** `grep -c '^name = "unicode-width"' Cargo.lock` → 1). Grapheme clusters, ZWJ emoji and variation selectors are not handled: `char_width` is per `char` (`cells.rs:16`) |
| Dialog kit | `dialog/mod.rs:1-12`, `dialog/view.rs`: confirm, input, select, form, checklist, text, as tmux popups | `select` and `checklist` cover single and multi-select. No tabs, no preview pane, no inline free text (**unverified**: only `mod.rs:1-12` and `commands.md:3326-3327` were read, not every function of `view.rs`) |
| Link widget, no OSC 8 | `link.rs:1-30`; `osc_8_guard_tests.rs:1` (decision pro-d60) | Cells plus a click map; reusable for chat links |
| Spinner and frame clock | `palette.rs:401-407` (28 moon phases), `:480-486` (`FRAME_MS` = 50, `frame`); `pump.rs:221-231` (`Needs.frame`, `wake`) | The one loader of pi and the cockpit (hs-d616). `pace.rs` is **not** a clock: it is the newest-wins read queue (`pace.rs:1`, `:19`) |
| Box widget | `box_widget.rs:1-179` | Rounded boxes with a title pill, could draw balloons |
| View runner | `view_ports.rs:160-182` (`run_view_with`: raw mode, `EnterAlternateScreen`, mouse capture), `runner.rs:15-30` (`Capture`) | Each view is its own process with its own terminal |
| Chat slot placeholder | `chat.rs:17-37` | The pane that a pi pane is swapped against |

**What History lacks** (**read** `history/render.rs:19-64`): it prints only `message` entries with `text` and `toolCall` and `toolResult` parts; `thinking` parts are skipped (test at `:89`, `:112`); a user message is cut at 12 lines (`:40`); a tool result shows its last non-empty line cut to 160 characters (`:56-57`); no markdown, no balloons, no folds, no compaction entries, no custom (cpd) messages, no mention highlighting; it reads the whole file once (`history/mod.rs:111`, `:126`) with no follow. The checked-in fixture has 12 lines and renders 23 rows (**ran** `wc -l history/fixtures/*` → 12; **read** `history/render.rs:198`).

## 4. Components: pi today, `tui/` today, gap and estimate

Estimates are production Rust lines for a hand-written implementation **with no new crate**; if `crates.md` names a crate the Plan accepts, the markdown parser, the syntax highlighter and some of the editor rows shrink (not estimated here). Lines of the pi components are TypeScript (a Rust line is not a TypeScript line: the estimates use the Rust comparables of section 3 first, and the pi size only as a sanity check, **unverified** that the ratio is 1:1).

| # | Component | What pi does today | What `tui/` has | Gap | Estimate (production lines) | Basis and command |
|---|---|---|---|---|---|---|
| 1 | Transcript viewport | `scroll-view.js:186-194` (renders the whole child, then slices), follow-the-end `:15-41`, wheel, scrollbar; anchor kept on expand `compact-tools-viewport.ts:79-165` | `scroll.rs:19-80`, `dialog/scroll.rs`, `history/view.rs:40-47` (title row plus body, 1-cell pad) | A block model (one entry per message part) with wrapped rows cached per width, a prefix-sum of heights, drawing only the visible blocks, streaming append that touches only the last block, an anchor (block id + row offset) kept when a fold above toggles (replaces `findAnchorTop`), jump to top and end, scrollbar | **600–1,000** (MVP: 350–550, no anchor or scrollbar) | Comparable: `scroll.rs` 135 + `history/view.rs` 49 + `dialog/scroll.rs` 106 = 290 (**ran** M3). 2–3.5× for the block cache and the anchor. pi: `scroll-view.js` 194 + `viewport.ts` 234 = 428 TypeScript (**ran** `wc -l`) |
| 2 | Markdown, code highlight, tables | `markdown.js` 820 lines on top of the `marked` parser; `highlightCode` (cli-highlight) in the theme | Nothing: no parser and no highlighter in `Cargo.lock` (**ran** `grep -c '^name = "pulldown-cmark"' Cargo.lock` → 0, same for `syntect`, `comrak`, `tui-markdown` → 0) | Block parser (paragraph, heading, list, quote, fence, table, rule), inline parser (emphasis, code, link, strikethrough), width-aware table layout, wrap that keeps styles, a small highlighter; streaming: re-parse only the last block (blocks split at a blank line outside a fence) | **1,400–2,500** (MVP: 250–400: paragraphs, code fences without colour, inline code, bold) | Renderer 500–800 against `markdown.js` 820 (without LaTeX); parser 600–1,000, **unverified**: `tui/` has no parser to measure, the number is the author's judgement; highlighter 300–700, **unverified**, "see crates.md" for what would replace it. `history/render.rs` (65) and `scroll.rs` `wrap` (50) are the only text code today (**ran** M3) |
| 3 | Balloons, tool, thinking and compaction folds | Balloons `balloon.ts` 49; thinking 385; compaction 91; tool rows 571 + 531; grouping 487; cpd messages `render.ts` 237 | `fold.rs` 81 (title, toggle, plain, content), `tree/folds.rs` 141 (fold state and clicks in the tree), `box_widget.rs` 179 | Balloon geometry (gap 7 or 8, tails, time in the border), fold state with hit-test per block (a click anywhere on a message toggles it, `render.ts:230-235`), the tool-row summary per tool, running timers, failed rows, thinking summary, compaction, cpd message pill, mention highlighting, "looked around" groups | **1,150–2,000** (MVP: 350–600: balloons and one-line tool rows, no folds). Split: balloons 120–200, thinking and compaction folds 150–250, tool rows 500–900, grouping 300–500, mentions and cpd messages 80–150 | Comparables: `fold.rs` 81 + `tree/folds.rs` 141 + `box_widget.rs` 179 = 401 (**ran** M3), times 3–5 because the tool summaries have no comparable. pi: 49 + 385 + 91 + 571 + 531 + 487 + 237 = 2,351 TypeScript (**ran** `wc -l`) |
| 4 | Multi-line input editor | `editor.js` 2,043 + `autocomplete.js` 692 + `word-navigation.js` 95 + `kill-ring.js` 43 + `fuzzy.js` 110 (**ran** `wc -l`) = 2,983 TypeScript | `input_box.rs` 103 (wrap, `cursor_at`, a block cursor) + 236 test lines (**ran** M3, M5) | Buffer with graphemes, cursor, selection, undo, word motions (CJK), kill ring, bracketed paste (`Event::Paste`, needs `EnableBracketedPaste`: `view_ports.rs:173-175` does not enable it) and `[paste #n]` markers, history, completion popups for `/` and `@` (a file search spawns `fd` through a `Runner`, as `link.rs:116` `open_url` does for `open`), external editor (leave raw mode, run `$EDITOR`, come back), key bindings | **1,700–2,900** (MVP one-line input: 250–400, cursor motion, Enter, paste). Split: buffer, cursor, wrap 400–600; selection 150–250; undo 100–200; word motions 100–150; kill ring 50; paste 100–200; history 80–150; autocomplete 400–700; external editor 80–150; keybindings 100–200 | Comparable: `input_box.rs` 103 is a render-only box, so the buffer logic has no Rust comparable; sized on pi's 2,983 lines at 0.55–1.0 Rust per TypeScript line, **unverified** ratio |
| 5 | `ask_user_question` modal | `pi/ask-user-question/` 5,892 lines (view 2,521, state 1,841, tool 424 and the rest); previews `view/components/preview/*` 837 | `dialog/mod.rs` 577 + `view.rs` 712 + `scroll.rs` 106 + `validate.rs` 54 = 1,449 (**ran** M3), as tmux popups: select, checklist, form, input, text | Tabs and a submit tab, option lists with descriptions and previews (markdown, so it needs component 2), multi-select with free text row, notes, an external editor, overflow handling | **1,200–2,000** (MVP: 80–150 of glue that opens the existing `dialog select` popup) | Comparable: `dialog/` production 1,449 (**ran** M3) for the same kinds of forms; the tab bar, previews and state machine add 0–40 %. The tool schema, validation and answer format (`tool/` 424 lines, **ran** `find pi/ask-user-question/tool -name '*.ts' | xargs cat | wc -l`) are not UI and are not counted: they belong to the host that runs the tool (**unverified**, another Job) |
| 6 | Plannotator and plan-review handoff | `review-chat.ts` 71, `plan-review.ts` 77, `plannotator.ts` 78, `pntrack.ts` 157, `pnguard.ts` 40 = 423 lines, mostly adapter logic | `dialog select` and `input` popups; `link.rs:1-30` `open_url` | The chat only needs the three-option select, an editor and a click that opens the plannotator URL; the tracking and guards stay in the adapter | **150–300** (MVP: 0) | Comparable: `link.rs` 136 + a `dialog select` call. The adapter logic is not part of the chat widget (**unverified**: depends on the pi-durable host, another Job) |
| 7 | Status footer, spinner, animation clock | `footer.js` 264; `compact-tools-clock.ts` 55, `loader.ts` 43, `progress.ts` 146, `cursor.ts` 128, `activity.ts` 329, `editor.ts` focus colour | `palette.rs:401-486` (28 frames, `FRAME_MS` 50), `pump.rs:221-231` (`Needs.frame`, `wake`): a view that sets `needs.frame` redraws on each frame | Footer line (cwd, tokens and context %, branch, state), the working loader with words and tool progress, a blinking cursor tied to focus, the "light" activity line (optional) | **300–600** (MVP: 80–150: loader and a one-line footer) | Comparable: `plan/pulse.rs` 117 production and `palette.rs` spinner code (**ran** M3). pi: 264 + 55 + 43 + 146 + 128 + 329 = 965 TypeScript |
| 8 | Text selection and copy, links, images | `tui-alt-screen.js` 1,549 lines own selection (anchor and focus, character, word and line granularity, drag auto-scroll `:53-61`, copy on select `:81`, `:1135`), `alt-screen-search.js` 281, clipboard via `pbcopy` or OSC 52 (`pi-coding-agent clipboard.js:8-140`); links as OSC 8 (`markdown.js:3` `hyperlink`) | `link.rs` (cells and a click map, no OSC 8: `osc_8_guard_tests.rs:1`); no selection | Selection model over wrapped rows (screen cell to grapheme), highlight, double and triple click, drag with auto-scroll (needs a timer: `pump.rs` `Needs.at`), copy through a `Runner` (`pbcopy`), copy text that leaves out fold and balloon chrome, clickable links in markdown through `Links`; images: leave out | **500–850** (MVP: 0, the tmux mouse selection stays, see R1). Split: selection 400–700; links 80–150; images 0 | pi: selection part of the 1,549 lines, unmeasured (**unverified** which share). Comparable: `link.rs` 136 for links (**ran** M3); the selection has no Rust comparable |
| 9 | Where the chat lives | A pi process in a tmux pane, swapped into the chat slot | Views are processes in panes: `view_ports.rs:160-182`; `chat.rs:17-37` is the placeholder; `history/mod.rs:58-63` is a chat-slot view | Option A: `cpd tui view chat <session>` runs in the slot or in a parked window. Option B: one ratatui process for the whole cockpit | A: **150–300** (the view's loop, arguments, `Capture`, the wiring that replaces the pi launch); B: not estimated, see below | Comparable: `history/mod.rs` 140 + `chat.rs` 38 (**ran** M3) |

### Component 9 in detail

- **The cockpit is not one ratatui process.** `cpd tui` builds a tmux layout and attaches (`tui/src/lib.rs:1-5`, **read**); the tree, the plan view and the widgets are separate `cpd tui view <name>` processes in panes (`tree/mod.rs:144`, `widgets/mod.rs:87`, **read**), each with its own `EnterAlternateScreen` and mouse capture (`view_ports.rs:171-175`, **read**). "A widget inside the cockpit process" therefore means rebuilding the cockpit as a single application, which also replaces tmux's layout. That is a one-way door, and it is bigger than this Job's scope (**unverified**: not estimated).
- **Option A, its own pane running a `tui view chat` process, costs 150–300 lines and keeps the architecture.** The chat slot logic stays: `swap-pane` is called from `core/src/session/moves.rs:234` and `:294` and defined at `core/src/host/tmux.rs:901` (**read**); the slot is found by `@cpd_session` or `@cpd_view` (`core/src/host/layout.rs:84-91`, **read**).
- **What Option A removes from `core/src/host/tmux.rs`: nearly nothing.** **Ran** `grep -rn "send-keys\|paste-buffer\|capture-pane\|load-buffer" --include=*.rs core engine cli tui | grep -v "tests\|fakes"` → only the two lines of the mouse binding `MouseDown1Pane` (`tmux.rs:168`, `:202`). So messages reach pi through the adapter and not through tmux keys, and the native chat deletes no tmux input code. It would delete the pi launch (`core/src/host/pi/launch.rs` 548 lines and `packages.rs` 369, **ran** `wc -l core/src/host/pi/*.rs`) and the TypeScript in `pi/` (`compact-tools` 7,705 lines, **ran** `find pi/compact-tools -name '*.ts' | xargs cat | wc -l`; `ask-user-question` 5,892; `lib/render.ts` 237 and `lib/pad.ts` 82), which is where the deletion win is.
- **Option A also keeps `mouse on`** (`tmux.rs:173`, **read**): tmux owns the mouse in the cockpit window, as it does for pi today (R1).

## 5. Technical risks, ranked

Ranked by how likely each is to block parity or make the result worse than today, and by the cost of finding out late.

### R1. Mouse capture and text selection (high)

- **Today.** The User's pi chat is **not** in the terminal's scrollback. pi's default mode is fullscreen: `tuiMode: "fullscreen"` (**read** `node_modules/@earendil-works/pi-coding-agent/dist/core/settings-defaults.js:61`), the repo's own research ran in it as "the User's mode" (**read** `docs/research/thinking-live.md:58`, `:124`). The fullscreen renderer enters the alternate screen (`pi-tui/dist/tui-alt-screen.js:12`, `\x1b[?1049h`), enables mouse reporting (`:16`, `?1000h ?1002h ?1006h`) and owns selection: anchor and focus, character, word and line granularity, drag auto-scroll, copy on select (`:53-61`, `:81`, `:1135`), with the clipboard through `pbcopy` or OSC 52 (`clipboard.js:8-140`). tmux has `mouse on` (`core/src/host/tmux.rs:173`) and a root binding that forwards a click to the pane (`:202`, `send-keys -M`) (**read**). So a pi chat user already scrolls by wheel inside the alt screen and copies with pi's own selection.
- **The spec's guess is stale.** The Job text says pi "renders into the terminal's scrollback". That is `regular` mode (`settings-manager.js:988`: `tuiMode === "regular"`), not the default and not the mode the repo's research used. **Ran** `grep -rn tuiMode --include=*.ts --include=*.rs --include=*.json . | grep -v node_modules` → only `docs/research/thinking-live.md` and `docs/contract.md:3414` mention it: cpd does not set `tuiMode`, so the User's choice (default `fullscreen`, **unverified** that the User never switched to `regular` with `/settings`) decides it.
- **What a ratatui chat costs.** Every cockpit view already is alt-screen with mouse capture (`view_ports.rs:171-175`, **read**; `Capture::Mouse` for history `history/mod.rs:60`), so the behaviour matches today's fullscreen pi, but the native selection and copy that pi has must be re-written: component 8, 400–700 lines. Until then a drag in the pane is tmux's (**unverified**: standard tmux behaviour with `mouse on`, not tried here), and a Shift-drag is the terminal's.
- **Risk if the User uses `regular` mode.** Then terminal scrollback and native selection are lost with a native widget, and no widget can restore them inside an alt screen. Mitigation: ask the User which mode they use before the work is decided (**unverified** which is true).

### R2. Long-transcript performance: re-wrap and re-render per frame (medium)

- **What the repo does today.** `Scroller::draw` re-wraps all lines only when the width changed (`scroll.rs:69-71`) and draws only the visible slice (`:77`, **read**), but `set_lines` throws the whole wrapped cache away (`scroll.rs:33-36`) and `wrap` builds a `Vec<(char, Style)>` per long line (`:89-91`, **read**). The History view reads the file once (`history/mod.rs:111`) and never follows. A streaming chat that calls `set_lines` per token would re-render the entire transcript at every token: O(transcript) per token. That is why component 1 needs a per-block cache.
- **What the repo measured.** The only measurement of a view that re-reads and re-renders a growing session file is in `docs/research/anim-perf-views.md:7`: "each change re-reads and re-renders the whole 1.2 MB file on the drawing thread: read p50 11.5 ms, p95 20 ms, max 37 ms, draw p50 3.5 ms. Fine at this size." **Stale:** that view was the `worker` view, which no longer exists in `tui/src` (**ran** `grep -rln 'fn run.*worker\|"worker"' tui/src` → only `read/pulse_tests.rs`; `docs/commands.md:4580` lists `tui view worker` flags as removed), so the figure is evidence about a renderer that has been deleted. The loader measurements (`anim-perf-tree.md:5`, `anim-perf-after.md:41`) show a 100 ms frame loop with `draw_us` p50 3.0–4.7 ms for the tree; those are the budget a chat frame lives in, not a measurement of a chat.
- **What pi does.** `scroll-view.js:186-188` renders the whole child on every frame and slices; pi relies on component-level caches (`compact-tools-layout.ts:24` `CachedComponent`, **read**). So pi has the same shape of risk and no reported problem (**unverified**: no pi timing was taken).
- **Timing the existing renderer was not done.** The allowed measurement is "only the existing `history/` renderer on its checked-in fixture, under `timeout`, and only if a test target already runs it without a full build of the workspace". The fixture is 12 lines (**ran** `wc -l history/fixtures/session.jsonl` → 12), a time on it says nothing about long transcripts, and running it needs a compiled test binary, which this Job's rules forbid building. **Every timing of a ratatui chat is unverified.**
- **Mitigation, designed not measured:** per-block cache keyed by (block id, width, revision); prefix-sum of block heights; layout only the blocks that intersect the viewport; the streaming block is the only one invalidated.

### R3. Keeping the look identical to pi's chat (medium)

The look is spread over 7,705 lines of `pi/compact-tools` plus `lib/render.ts` and `lib/pad.ts` (**ran** section 4, row 3 and row 9 commands). The cockpit already owns the fold glyphs (`fold.rs:34-40`, `render.rs:148-157`, **read**) and the palette indices (`compact-tools-indices.ts`), so the folds can be made identical. The balloon geometry (`balloon.ts` gap 7 and 8, tails, time in the border) and the per-tool summaries have no Rust copy, and pi draws with ANSI strings where ratatui draws with cells: the two wrap differently (pi `wrapTextWithAnsi`, `render.ts:131`; cell-by-cell wrap in `scroll.rs:84-134`), so identical line breaks are not free (**unverified**: not compared). Mitigation: snapshot tests that draw one fixture in both (pi side not run in this Job).

### R4. Wide characters, emoji and grapheme clusters (medium)

`cells::char_width` measures one `char` at a time (`cells.rs:16-17`) and `input_box::rows` wraps by `char` (`input_box.rs:29-45`), `scroll::chunk_end` too (`scroll.rs:110-122`) (**read**): a ZWJ emoji, a flag or a letter with a combining mark is several `char`s that must stay together and measure as one cluster. East Asian wide characters are right (`cells.rs:1-6`, `input_box.rs:27-28`). pi uses a grapheme segmenter (`editor.js:153` word segmenter, `tui-alt-screen.js:10` `getGraphemeCellRange`, **read**). The workspace has `unicode-segmentation` only transitively through ratatui (**ran** `grep -c '^name = "unicode-segmentation"' Cargo.lock` → 1), so using it directly is a dependency change: see crates.md. Terminal emoji width also varies by terminal (**unverified**, not tried).

### R5. IME and bracketed paste (medium)

`view_ports.rs:171-175` enables the alt screen and mouse (and focus) and **no** bracketed paste (**read**), so a paste arrives as key events today; the editor needs `EnableBracketedPaste` and `Event::Paste` handling (`editor.js:570-579` is pi's, **read**). IME composition in a TUI is the terminal's: the cursor position the terminal reports for the composition window follows the hardware cursor, and the cockpit's input box draws a fake block cursor with `Span` styles (`input_box.rs:78-83`, **read**), so the hardware cursor is not placed on the text. **Unverified** how a macOS IME behaves with that; the fix is `Frame::set_cursor_position` in the editor (a small change, 10–30 lines, included in component 4's 100–150 for the buffer's cursor).

### R6. Streaming markdown that re-parses as tokens arrive (medium)

Unclosed fences, half-written tables and `$`/`\[` LaTeX are the cases pi handles with `pending` tokens (`markdown.js:73`, `:95-99`, **read**). A hand parser needs the same: parse by block and re-parse only the last open block, and render an unclosed fence as code. Without a parser crate this is the largest piece of new logic (component 2, parser 600–1,000, **unverified**). LaTeX (`latex.js` 1,377 lines) is left out of every estimate (**unverified** that cpd chats use it).

### R7. Fold toggling shifts the view (low)

pi needs 234 lines of `viewport.ts` and `findAnchorTop` (a line-matching search with a 200-line tie-break, `:79-112`) because its scroll offset is a row number. A block model can anchor by (block id, row offset) with none of that search (component 1). This is an upside, listed here because the prototype's hook on `TuiAltScreen.prototype.doRender` (`viewport.ts:176`) is one of the patches the decision wants gone.

### R8. Adding the input to a process that also reads events (low)

The existing `pump.rs` merges input, store commits and reads into one wait (`pump.rs:1`, `:221-231`, **read**), so a chat that also follows a Session file fits. The cost is that the editor, the blinking cursor (`compact-tools-cursor.ts`) and the spinner all raise `needs.frame`, and the 50 ms frame (`palette.rs:481`) redraws the transcript: another reason for R2's per-block cache.

## 6. Totals, PRs

Arithmetic for every total (**ran** `python3`, 2026-10-28 EDT):

```text
full  production low  = 600+1400+1150+1700+1200+150+300+500+150     = 7150
full  production high = 1000+2500+2000+2900+2000+300+600+850+300    = 12450
full  tests low  = 7150  * 0.79 = 5648   (external test files only)
full  tests high = 12450 * 1.24 = 15438  (with inline test modules)
full  total low  = 7150 + 5648  = 12798
full  total high = 12450 + 15438 = 27888
MVP   production low  = 350+250+350+250+80+0+80+0+150               = 1510
MVP   production high = 550+400+600+400+150+0+150+0+300             = 2550
MVP   tests low  = 1510 * 0.79 = 1193
MVP   tests high = 2550 * 1.24 = 3162
MVP   total low  = 1510 + 1193 = 2703
MVP   total high = 2550 + 3162 = 5712
```

(The `python3` call printed `7150 12450 5648 15438 12798 27888` and `1510 2550 1193 3162 2703 5712`.)

| Set | Production lines | Test lines (0.79–1.24 × production, section 3) | Total | PRs |
|---|---|---|---|---|
| **Minimum viable**: a read-only chat view plus a one-line input | **1,510–2,550** | 1,193–3,162 | **2,703–5,712** | **4** |
| **Full parity** (components 1–9, no images, no LaTeX, no option B) | **7,150–12,450** | 5,648–15,438 | **12,798–27,888** | **16–18** |

**What the MVP contains:** component 1 without anchor or scrollbar; component 2 minimal; component 3 balloons and one-line tool rows without folds; component 4 as a one-line input with paste; component 5 as glue to the existing `dialog select` popup; component 7 loader and a one-line footer; component 9 as option A. It leaves out folds, the multi-line editor, completion, `ask_user_question` as a native modal, plan review, selection and copy. **Unverified** whether such an MVP could replace pi for any Session: an agent that calls `ask_user_question` or a plan review, or a User who needs to paste a long text, would still need pi.

**PRs** (each one reviewable change; sizes are production plus tests at the section 3 ratio):

| PR | Contents | Component | In MVP |
|---|---|---|---|
| 1 | `tui view chat <session>` loop, block model, transcript viewport with follow and wheel | 1, 9 | yes |
| 2 | Balloons, one-line tool rows, loader, one-line footer | 3, 7 | yes |
| 3 | Markdown, minimal | 2 | yes |
| 4 | One-line input, bracketed paste, `dialog select` glue for `ask_user_question` | 4, 5 | yes |
| 5 | Markdown parser: lists, quotes, tables, links, streaming | 2 | |
| 6 | Syntax highlighting (see crates.md) | 2 | |
| 7 | Thinking, compaction and tool-output folds with fold state, anchor on toggle | 3, 1 | |
| 8 | Tool groups, silent mode, cpd messages, mention highlighting | 3 | |
| 9 | Editor core: multi-line, selection, undo, word motions | 4 | |
| 10 | Editor: history, kill ring, external editor, key bindings | 4 | |
| 11 | Completion popups for slash commands and `@` files | 4 | |
| 12 | `ask_user_question` modal: tabs, single and multi-select, free text | 5 | |
| 13 | `ask_user_question` previews and notes | 5, 2 | |
| 14 | Text selection and copy | 8 | |
| 15 | Links, full footer, cursor blink, activity animation | 7, 8 | |
| 16 | Plan-review handoff | 6 | |
| 17–18 | Overflow: whichever of PRs 5, 9, 12 and 14 is over a reviewable size | any | |

## 7. Corrections to the Job text and to existing docs

- **"pi renders into the terminal's scrollback"**: true only in `regular` mode. Default is `fullscreen` (`settings-defaults.js:61`, **read**); see R1.
- **"`@file` mention completion `pi/lib/mentions.ts`"**: `mentions.ts` colours Session, Plan and Context ids magenta (`mentions.ts:7-63`, **read**). The `@file` completion is pi-tui's `CombinedAutocompleteProvider` and `fd` (`autocomplete.js:157`, `:206`, **read**).
- **"`pi/lib/balloon.ts`"** holds one `export` line (`balloon.ts:2`, **read**); the code is `compact-tools-balloon.ts`.
- **"`pace.rs` (animation clock)"**: `pace.rs` is the newest-wins read queue (`pace.rs:1`, `:19`, **read**). The animation clock is `palette.rs:480-486` and `pump.rs:221-231` (**read**).
- **`docs/research/anim-perf-views.md:7`** measures a `worker` view that no longer exists (R2).
- **`docs/research/box-widget.md`, `docs/research/fold-language-in-pi.md`, `docs/ui-preview.md`, `docs/interface.md`, `docs/research/pi-durable.md` and `inbox-pane.md`, `agent-comms.md`** were not read in full by this Job; only the files cited above were. Nothing in them is relied on.

## 8. Not done

- No timing of any renderer, editor or markdown path (R2: **unverified**).
- No pi side-by-side render, so look parity (R3) is **unverified**.
- Images, LaTeX and `claude`, `codex` and `off` styles are not estimated.
- The cockpit-as-one-process option (component 9, B) is named and not estimated.
- Crate options (a markdown parser, a highlighter, a text-area widget, grapheme segmentation, clipboard) are for `ratatui-chat-crates.md`.
