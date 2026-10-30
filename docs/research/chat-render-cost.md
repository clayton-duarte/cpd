# Chat render cost: why the Manager's chat burns a core (hs-n563)

## Summary

- Symptom: the building-cpd Manager's chat (pi 1.3.0, cpd adapter, compact-tools; a 24 MB session, about 445 chat components) burns about 101% of one core while a tool row animates, and typing lags.
- Cause: the chat pad (`pi/lib/pad.ts`) gives the real root and the inner HStack no `basis`, so pi-tui measures them by rendering them: the whole chat is rendered 2-3 times per frame.
- The fix landed on main as #644 (commit 9bfec3be, 2026-10-30 15:19 EDT, Plan type-lag, found independently): `basis: 0` on those two entries in `pi/lib/pad.ts`. The pad looks the same; only how pi-tui sizes it changes. This PR is the research note and adds no code.
- It leaves: pi-tui's `paintBox` fast path (skipped at x = 1, about 5-8 ms per frame, upstream, U1), the footer's session-name scan (upstream, U2) and the compact-tools rows that re-render per frame. A later PR addresses them only if the after-numbers show the cost still grows with the chat.
- `docs/research/type-lag.md` is the type-lag Plan's recipe and report for the same symptom (typing lag).
- The 20 fps animation rate is not changed.

## Measurements before the fix

Source: `/home/clayton/.cpd/run/chat-render-cost/explore/repro2.md`. Bash-row = a fake provider's running `sleep 300` bash row animating. CPU is utime+stime of the pi process root over 5 s windows (100% = one core), mean of 4 windows. Frame ms is the mean of the harness's per-frame render log (bash row), with p50 in brackets. Idle CPU is focused. Typing is the median latency in ms with the pane idle (10 keys, first sample dropped).

| session | chat components (idle to bash row) | bash-row CPU% | frame ms mean (p50) | idle CPU% | typing latency ms (idle median) |
|---|---|---|---|---|---|
| cut10 | 16 to 20 | 27.0 | 19.1 (15.7) | 2.3 | 21.5 |
| cut50 | 53 to 57 | 30.2 | 16.0 (14.6) | 4.0 | 20.8 |
| cut100 | 89 to 97 | 38.0 | 23.2 (20.1) | 4.2 | 35.6 |
| cut200 | 160 to 168 | 53.7 | 27.5 (26.2) | 4.8 | 42.6 |
| small1 | 118 to 126 | 46.2 | 22.9 (21.8) | 4.8 | 27.2 |
| mid8 | 612 to 620 | 69.4 | 27.6 (28.9) | 8.4 | 34.1 |
| big (24 MB copy) | 356 to 364 | 79.5 | 34.7 (34.5) | 8.2 | 54.2 |

The cut series is the tail of the same real session (the last K entries after its last compaction).

### The fit

Least squares over cut10, cut50, cut100, cut200, with components counted with the bash row:

| y | per component | intercept | r2 |
|---|---|---|---|
| CPU ms per frame | 0.097 ms | 11.6 ms | 0.97 |
| frame ms p50 | 0.079 ms | 12.5 ms | 0.90 |

Cost per frame grows with the number of chat components. mid8 and big sit off the line (mid8 far below it; reason not determined: unverified).

### The live reading

The live Manager chat read about 101% over 5 s (24 MB, about 445 components). The harness `big` reads 79.5% (72.8 to 84.0) with 364 components. The cut-series line (0.186 %-points per component, intercept 21.3) gives 104% at 445 components, an extrapolation of 4 points. Both bracket the live reading; not a proof.

### The harness and its limits

- The harness lived in `/tmp/chat-render-cost` and is not in the repo.
- It used a private tmux socket, a scratch agent dir, and a fake provider that produces the running bash row. The cpd adapter was INACTIVE (all `CPD_*` removed from the environment); compact-tools was active. The pane was 200x50.
- Limits: a single run per label (4 windows of 5 s), load average 5 to 26 during the runs, mid8 and big off the line, and the active adapter not covered (it can only add cost). The harness did not reproduce "whole words typed before any shows": typing in `big` lags about 2-3 frames.

## The profile

Sources: `prof-row.md`, `prof-key.md` and `pad-fast.md` in `/home/clayton/.cpd/run/chat-render-cost/explore/`. Profile windows are 25 s of a bash row animating; `big` was 453 frames at 48.7 ms mean (88% busy), `cut50` 545 frames at 13.1 ms (28% busy). Profiling overhead makes both slower than the table above.

### The cause

`pi/lib/pad.ts:20-47` (PaddedRoot) wraps pi's layout root as VStack[Spacer(1), HStack[Spacer(1), root, Spacer(1)], Spacer(1)] to inset the chat by one cell on every side. The entries for the real root and for the inner HStack carry no `basis` (`pad.ts:31-45`). pi-tui measures such an entry by rendering it: `node_modules/@earendil-works/pi-tui/dist/layout.js:28-33` (`measureHeight`, `measureWidth`) call `renderCached` (`layout.js:14`). Per frame:

- the outer VStack measures the inner HStack with `measureHeight`, which renders the whole real root once;
- the inner HStack measures the root with `measureWidth` at the full width: a second full render;
- `measureHeight` at width minus 2 and `layoutComponent` render it a third time (cached within the frame).

Without the pad the root is the layout root and renders once. This is why every chat component renders 5 to 6 times per frame (prof-key: 5.0 calls per ToolExecutionComponent, Assistant, User and MouseRegion per frame, 6.0 for the footer; 7585 `render()` calls per frame on `big`, 1136 on `cut50`), at widths 200 and 198, and why a frame's cost grows with the chat.

### The three costs found

| # | cost and where | owner | grows with | size |
|---|---|---|---|---|
| 1 | the pad's measuring renders: `pi/lib/pad.ts:31-45`, `layout.js:28-33`, `:14` | ours (the missing `basis`, added by #644) driving pi-tui | chat components | most of the pad's cost on `big` (see the table below) |
| 2 | the `paintBox` fast-path skip: at x = 1 every row goes through `compositeTuiLine` and `sliceWithWidth` (`layout.js:243`, `utils.js:1150`, `tui.js:157`) | upstream pi-tui, triggered by the pad's offset | screen rows (about 50), not components | about 6 ms per frame (11.3% of the `big` window, 13.1% of `cut50`) |
| 3 | the footer's session-name scan: `getSessionName` walks the entries back until a `session_info` entry (`session-manager.js:927`, `footer.js:145`) | upstream pi | session size | 3.1 ms per frame in `big`, 2 ms in `cut50` per prof-row; the footer renders 6 times per frame, 1.5 ms per call (8.9 ms per frame, probed) |

Also seen, not explained: compact-tools' per-component render wrappers (`compact-tools-silent.ts:318-321`, `compact-tools-thinking.ts:124`, `:267`, `compact-tools-layout.ts:87`) run every frame and account for 17.4% self time in `big` against 3.0% in `cut50`. Markdown and Text keep one cached width (`pi-tui/dist/components/markdown.js:185`) and are asked at 200 and 198, so most calls miss (prof-key). Whether removing the second width fixes that was not tested in prof-key; the pad-fast run below is the evidence.

### The pad-fast measurement

From `pad-fast.md`: scratch copies, bash row, 4 windows each. `pad on` is the unmodified `pi/`; `pad on + basis 0` adds `basis: 0` to the root entry and the HStack entry; `pad off` makes `registerChatPad()` return at once.

| session | pad on | pad on + basis 0 | pad off |
|---|---|---|---|
| 10-row copy (cut10), frame ms | 9-18 | 7.7-9.7 | 0.9 |
| 24 MB copy (big), frame ms | 43-62 | 13-18 | 7-17 |
| 24 MB copy (big), CPU% | 85 | 31-36 | 12-23 |

Caveat: single runs, load average 8 to 29 during them (other agents), runs overlapped in pairs, the first window of a run is often high, and the same cut10 pad-on configuration reads 9 to 35 ms per window. Medians and the large gaps are reliable; exact milliseconds need a quiet-box rerun. The profile-level confirmation of the measuring renders is unverified (inferred from the code and the `basis: 0` run). On `big`, `basis: 0` gives back about 70-85% of the pad's cost (43-62 to 13-18 ms); the remaining 5-8 ms is the composite slow path.

## Upstream issue texts

For the User to file against pi. cpd does not file them.

### U1: pi-tui `paintBox` fast path needs x === 0, so an offset layout root paints every row through `compositeTuiLine`

- Title: `paintBox` skips its fast path for any layout root not at x = 0 (a padded root), compositing every painted row each frame
- pi version: 1.3.0 (`@earendil-works/pi-tui` 1.3.0).
- Measured: with a one-cell pad around pi's layout root (every box at x = 1, width = total minus 2), about 6 ms per frame in `compositeTuiLine` and `sliceWithWidth` (11.3% of a 25 s window on a 24 MB session, 13.1% on a 57-component one), constant with the chat size, scaling with screen rows (about 50). With the pad's other cost removed (`basis: 0`), the pad still costs about 5-8 ms per frame: 7.7-9.7 ms with the pad on against 0.9 ms with it off on a 10-row session. Single runs at load average 8 to 29.
- Code at fault: `node_modules/@earendil-works/pi-tui/dist/layout.js:243`: `if (box.rect.x === 0 && box.rect.width >= totalWidth && (isImageLine(line) || !screen[row])) screen[row] = line; else screen[row] = compositeTuiLine(...)` (else-branch `layout.js:246`; `compositeTuiLine` `tui.js:157`, `sliceWithWidth` `utils.js:1150`). Both halves fail for an offset, narrower box.
- Reproduction: pi 1.3.0, a layout root wrapped as VStack[Spacer(1), HStack[Spacer(1), root, Spacer(1)], Spacer(1)] (with `basis: 0` on the root and the HStack entries so measuring does not add cost); run a long bash tool call so a row animates; profile with `node --cpu-prof` and compare `sliceWithWidth` self time with the pad removed.
- Suggested direction: widen the fast path to x > 0 when the row is untouched, for example `screen[row] = " ".repeat(x) + line`, since the right-hand pad cell is blank by the erase-line and width clamp the comment at `layout.js:238-242` describes. Unverified for rows with a trailing background: check background-coloured rows at the right edge. Alternatively a padding or margin option on stack layout nodes (pi-tui 1.3.0 has only `gap` and `align`, `layout-node.d.ts:19-21`).
- Meanwhile cpd keeps the pad, with `basis: 0`, and accepts the remaining cost. No fork, nothing vendored.

### U2: the interactive footer's `getSessionName` scans all entries on every render

- Title: footer calls `getSessionName()` on every render; it scans every entry when the session has no `session_info`, so frame cost grows with session size
- pi version: 1.3.0 (`@earendil-works/pi-coding-agent` 1.3.0).
- Measured: on a 24 MB session (7655 entries, 364 chat components, a bash row animating, pane 200x50) `getSessionName` is 1403 ms of a 25 s window: 5.6%, 3.1 ms per frame (2 ms per frame on a 57-component copy). The footer renders about 6 times per frame (1.5 ms per call, 8.9 ms per frame, with probe overhead). Whether the session had a `session_info` entry is inferred, not checked (I infer none: unverified).
- Code at fault: `node_modules/@earendil-works/pi-coding-agent/dist/core/session-manager.js:927` (`getSessionName`, walks `fileEntries` backwards until a `session_info` entry) called from `node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/components/footer.js:145`.
- Reproduction: pi 1.3.0 with a session of several thousand entries and no `session_info` entry (a large, never-named session); start pi with `node --cpu-prof`, run a long bash tool call so frames render at 20 fps, stop it after 25 s and read the self time of `getSessionName`.
- Suggested direction: cache the name and invalidate it when a `session_info` entry is appended, or keep the latest `session_info` while appending, so the read is O(1).
- Meanwhile cpd does nothing: the cost is left as is. A cpd-side footer cache (invalidated on `session_info_changed`, `agent-state.ts:84`) is a candidate for a later PR, only if the after-numbers show the cost still grows with the chat.

## What was fixed and what is left

Fixed (#644, commit 9bfec3be, 2026-10-30 15:19 EDT, Plan type-lag, found independently; this PR changes no code):

- `pi/lib/pad.ts`: `basis: 0` on the real root's entry and on the inner HStack's entry (grow, shrink and minSize kept), so pi-tui computes the layout from grow and shrink alone and never by rendering. The pad's look does not change. This supersedes hs-d742's "the pad is NOT changed" (the User's review of this Plan): only how pi-tui sizes the pad changes. hs-d742's other terms hold: no pi fork, nothing vendored, TypeScript only.

Left:

- The `paintBox` fast-path skip (about 5-8 ms per frame): upstream, issue text U1.
- The footer's session-name scan: upstream, issue text U2.
- The compact-tools rows and Markdown/Text that re-render per frame, and the keystroke that costs a full frame: a later PR addresses them only if the after-numbers show the cost still grows with the chat.
- The 20 fps animation rate is not changed (hs-d742).

## After the fix

Measured after the fix by the verify Job of this PR.
