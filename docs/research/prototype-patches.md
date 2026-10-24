# Prototype patches: does the newest load win after a /reload?

Job `thinking-audit` of Plan `thinking`. Changes no code. Base `09b72595`.

**Verdict.** `pi/` has 12 patch sites outside `compact-tools-thinking.ts` (which Plan `thinking` fixes). **3 are defects of the first-load-wins shape; 9 are newest-load-wins.** The defects are `compact-tools-custom.ts` `patchToolRows` (two rows: the four routed methods and the `updateDisplay` hook, both behind one `PATCHED_KEY` guard) and the `engaged` predicate that `compact-tools-silent.ts` `patchRender` closes over for the tool and notice prototypes. The custom defect is the one the Manager suspected. Today none of the three shows a wrong frame by itself: the state they read is process-global, so they bite only when the code of a closed-over function changes between loads (an update plus /reload). Separately, an older-release hazard is found in `activity.userRender` (a legacy marker cuts the wrapper chain and drops silent mode's wrapper for good); it is not a first-load-wins site, it is listed under "Older releases".

Labels: **ran** = executed here in a scratch script (deleted afterwards, nothing left in the worktree); **read** = source only; **unverified** = inferred, not run.

## Re-deriving the list of sites

```
grep -rn 'prototype' pi --include=*.ts | grep -v '^pi/test'
grep -rn 'Symbol.for' pi | grep -v '^pi/test'
grep -rn 'hookMethod' pi | grep -v '^pi/test'
grep -rnE '\.(render|handleInput|doRender|invalidate|updateContent|bg|fg|handleMouse) = ' pi --include=*.ts | grep -v '^pi/test'
grep -rn 'defineProperty\|Reflect.set\|setPrototypeOf' pi --include=*.ts | grep -v '^pi/test'
```

Every `Symbol.for` outside the table is state only, no patch: `EXECUTION_TIMINGS_KEY` (runtime.ts:20), `TRUST_KEY` (compact-tools.ts:101), `SUMMARY_KEY` (nested.ts:5), `SAID` (double-load.ts:13), `PAINTER_KEY` (silent.ts:43, mentions.ts:65), `RESOLVER_KEY`/`STATE_KEY`s (read-through slots, below). `runtime.ts:144` (`indicator.invalidate = invalidate`) assigns on a per-row instance, not a class or shared instance. All `pi/` paths below are under `pi/compact-tools/extensions/` unless they start with `lib/`.

## Table

`hookMethod` (compact-tools-hook.ts:20-47) wraps once per process under `Symbol.for('pi-compact-tools.hook.<name>')`; a second call replaces `record.behavior` and returns. So a `hookMethod` row is newest-wins exactly when every load calls `hookMethod` again, and when the behavior's state is global or the new load's.

| # | Site (what is wrapped) | file:line | Guard, and what a second load does | State / behaviour lives in | Verdict |
|---|---|---|---|---|---|
| 1 | `ToolExecutionComponent.prototype`: `getCallRenderer`, `getResultRenderer`, `getRenderShell`, `hasRendererDefinition` (routing, plus the `decisions`/`owners` WeakMaps, `changed`, `resolve`) | custom.ts:47-95 (guard :51, assignments :80-91) | `Object.hasOwn(prototype, PATCHED_KEY 'v2')` → `return true`; the body is skipped | The wrappers are closures of load 1. Only the resolver is a global slot (`RESOLVER_KEY`, custom.ts:8, :36), so what to render follows the newest load; how it is routed and cached does not | **FIRST load wins (defect)** |
| 2 | `ToolExecutionComponent.prototype.updateDisplay` via `hookMethod 'custom.refreshShell'` | custom.ts:52-55 | Same `PATCHED_KEY` guard (:51) sits before the call, so a second load never reaches `hookMethod` and `record.behavior` is never replaced | `refreshRowShell` is imported by load 1's `compact-tools-rendering.ts` (rendering.ts:69) | **FIRST load wins (defect)** |
| 3 | Four prototypes' `render` (`ToolExecutionComponent`, `AssistantMessageComponent`, `UserMessageComponent`, `CustomMessageComponent`): the override | silent.ts:249-266 (guard :258), called from :329-332 | `Object.hasOwn(prototype, ORIGINAL_RENDER 'v2')` → `return true` | The wrapper calls `delegate(key)` (silent.ts:309), which reads `OVERRIDES_KEY` on `globalThis` (:31, :268-270; published :272-275); `installPatches()` republishes it on every `bind` (:395) and `apply` (:413) | **newest load wins** (the override); see row 4 |
| 4 | Same wrappers: the `engaged` predicate for tool and notice (`isSilent`) | silent.ts:253, :261, :329, :332 | Same guard as row 3: the predicate passed by load 1 is the one kept | Load 1's `isSilent` function (:120). It reads the global `STATE_KEY` slot, so its result follows state; its logic does not follow the newest load | **FIRST load wins (defect, narrow, latent)** |
| 5 | `chat.render` (an instance) via `hookMethod 'silent.chatNotices'` | silent.ts:237-243, called by the widget factory at :388 | `hookMethod` replaces `behavior` each time the factory runs (once per `bind`, :386-390) | Behavior closes over this load's `renderWithoutNotices`; flags are in the global state | **newest load wins** (read; needs a `bind` per load, which `setWidget` of the same key forces) |
| 6 | `UserMessageComponent.prototype.render` via `hookMethod 'activity.userRender'` | activity.ts:192-215 (called from `show`, :293) | `hookMethod` replaces behavior; `show()` runs at the next agent start after a reload | Behavior is this load's; state is `ACTIVITY_STATE` (activity.ts:8, global) | **newest load wins** (but see "Older releases") |
| 7 | The document container's `render` via `hookMethod 'activity.transcriptRender'` | activity.ts:157-163 (from the factory at :303) | `hookMethod` replaces behavior each time the factory runs | Closes over `scroll` of the TUI at that moment and this load's `bottomAlignTranscript`; gated by global `active` | **newest load wins**: `active` is set only by the new load's `show()` (:296), just before the factory replaces the behavior (read) |
| 8 | The editor-gap container's `render` via `hookMethod 'activity.editorGap'` | activity.ts:184-188 (from :304) | As row 7 | This load's closure, global `active` | **newest load wins** (read) |
| 9 | `TuiAltScreen.prototype.doRender` via `hookMethod 'viewport.doRender'` | viewport.ts:174-187, called from `bind` (:207) | `hookMethod` replaces behavior at every `bind` | `restoreViewport` of this load; `pending` in global `STATE_KEY` (viewport.ts:8) | **newest load wins** (read) |
| 10 | `Editor.prototype.render` and `.handleMouse` via `hookMethod 'editor.box.render'` / `'editor.box.mouse'` | editor.ts:105-116 (`hookEditor`), called from editor.ts:125 and progress.ts:94 | Replaced at every `bind` of either controller | `boxRows` of this load; state in global `STATE_KEY` (editor.ts:14) | **newest load wins** (read) |
| 11 | `chat.render` (an instance) via `hookMethod 'claude.groups'` | grouping.ts:320-336, inside the widget factory of `bind` (:314-339) | Replaced when the factory runs, once per `bind` | The behavior closes over `this` (the new `ToolGroupController`) and `this.runtime`; the old controller's `context` is cleared by `dispose` (:339) | **newest load wins** (read) |
| 12 | `Theme.prototype.bg` and `Container.prototype.render` (tab bar) | lib/hint.ts:44-86 (guards :47 `container[TAB_KEY]`, :61 `proto[KEY]`) | Skipped when the key is set, **but** the adapter's `session_shutdown` calls the removal (lib/adapter.ts:41-44), which restores the original and deletes the key; the next load's `session_start` (adapter.ts:32) wraps afresh | Wrapper and constants (`HINT`, `HEADER`, `titlePill`, `tabLine`) are of the load that wrapped, which is the newest because the old one unwrapped first | **newest load wins** (via unwrap at shutdown, not via lookup) |

Row 12's two wraps are counted as one site. Count: rows 1-12 = 12 sites; defects = rows 1, 2, 4 = 3.

## Defects

### D1. `patchToolRows` routing (row 1) and `custom.refreshShell` (row 2)

- **Failing behaviour after a /reload.** The new load's `patchToolRows` returns `true` at :51 and installs nothing. The row's `getCallRenderer`, `getResultRenderer`, `getRenderShell`, `hasRendererDefinition` keep load 1's routing and decision cache, and `updateDisplay` keeps load 1's `refreshRowShell`. A fix to any of them (for example the shell choice in `refreshRowShell`, or `changed`'s renderer invalidation) does not take effect until pi restarts. The resolver itself does follow the newest load (global slot), so the compact renderers shown are current while the code that routes to them is old. **Ran:** with two loads of custom.ts on one fake prototype (`?load=1`, `?load=2`), the second `patchToolRows` replaced neither the routed methods nor `updateDisplay` (`second load replaced routing? false hook? false`). That the old closure then runs is read, not run.
- **Red-test sketch** (`pi/test/compact-tools/`, after the thinking test Job's `thinking-reload.test.ts`; pattern of docs/contract.md section 5.3):
  1. Copy `pi/compact-tools/extensions/` twice to `A/` and `B/` under a temp dir inside the repo (so `@earendil-works/*` resolves), and in `B/compact-tools-rendering.ts` make `refreshRowShell` push `"B"` to a global array (`A/` pushes `"A"`).
  2. Build a fake prototype with the five methods; `await import(A + "/compact-tools-custom.ts")`, call `patchToolRows(proto)`; then the same for `B`.
  3. Call `proto.updateDisplay.call(rowLike)`. Assert the array is `["B"]`. Today it is `["A"]`. A second assertion: in `B`, make `changed`'s routing return a marker for `getRenderShell`, and assert `proto.getRenderShell.call(row)` returns it.
- **Likely fix** (not done here): put the four routed methods behind `hookMethod` too (one name each), always called, with the `decisions`/`owners` maps in a `Symbol.for` slot; drop the `PATCHED_KEY` early return.

### D2. `patchRender`'s `engaged` predicate (row 4)

- **Failing behaviour after a /reload.** Only for the tool and notice prototypes: `engaged` is `isSilent` of load 1. If a later release changes when silent mode hides rows (the definition of `isSilent`, :120), the old rule keeps deciding whether the new override is called at all, until pi restarts. With today's identical code nothing visible differs. Not run (needs a changed `isSilent` between loads); the guard and closure are read.
- **Red-test sketch:** load silent.ts twice from two copied trees as in D1, with `B`'s `isSilent` returning `true` unconditionally and `OVERRIDES_KEY.tool` returning `["B"]`. Patch a fake `{ render }` class through `A`'s `patchRender` then `B`'s with `engaged = isSilent` each. Assert `new C().render(10)` is `["B"]`. Today the original rows come back.
- **Likely fix:** the wrapper looks `engaged` up in the shared slot beside the overrides (publish it with `publishOverrides`), as it already does for the override.

## Older releases: do wrappers left by a previous version heal?

All of this is **unverified** where marked: the older code is not in `git` history of this repo beyond the squashed commits, only the comments describe it.

- **hint.ts (row 12): heals.** The first release with `installHint` (c6249884, read with `git show`) already unwrapped at `session_shutdown` (its adapter.ts:37) and used the same key `cpd.hint.original`, so the old process's own shutdown restores the original before the new load wraps. **Ran:** `installHint` from load 1, a second `installHint` from load 2 does not rewrap, `off()` restores the original, a further `installHint` wraps again. The tab-bar key `cpd.tabbar.original` is newer (138f529a); no older release left one. Not run: a real pi reload, so "the old load's `session_shutdown` fires" is read from docs/research/pi-reload.md. A foreign wrapper stacked on `Theme.prototype.bg` or `Container.prototype.render` between the wrap and the removal is dropped by the removal (`proto.bg = original`): inferred from :81-83, unverified.
- **custom.ts (rows 1-2): leaves it, but inert** (unverified). The comment at :5-7 and :37 says a v1 patch (keys without `.v2`) may survive an update plus /reload; the v2 patch is not blocked by it (own-key check on `PATCHED_KEY 'v2'`), so it wraps on top, and `setRowResolver` empties the legacy resolver slot (:37), so the v1 layer falls through. The v1 wrapper stays for good as an extra call layer.
- **silent.ts (rows 3-4): leaves it, with its behaviour** (unverified). The comment at :27-29 says the v1 key (`originalRender`, without `.v2`) was wrapped by an older instance, and "a new key wraps once more"; nothing removes the v1 wrapper, so its old override stays in the chain under the v2 one until pi restarts.
- **activity.ts (row 6): heals the activity wrapper, and breaks a neighbour.** `hookMethod`'s `legacy` argument takes the original out of the old marker (hook.ts:32-39) and wraps that, dropping the old activity wrapper. But the same chain also holds silent's `UserMessageComponent` wrapper (row 3), and silent wrapped on top of the old activity wrapper. **Ran** with a fake prototype and the real `hookMethod` / `patchRender`:
  ```
  before:          silent>oldActivity>real
  after hookMethod newActivity>real          (silent's wrapper is gone)
  after patchRender again: newActivity>real  (its ORIGINAL_RENDER guard still says "patched")
  ```
  So in a process that ran the older release, after one /reload the user balloons (silent's `user` override) are lost for good, until pi restarts. The same cut applies to `activity.transcriptRender` and `activity.editorGap`, but their targets have no second wrapper, so nothing else is dropped. This is the "unclear → defect" for `userRender` only under an old-release process; a fresh pi is not affected. Red-test sketch: the script above as a test (legacy marker set, silent wrapper on top, then `hookMethod(..., [LEG])`), asserting the chain still contains the silent wrapper.

## Not audited

`compact-tools-thinking.ts:305-306` (Plan `thinking` fixes it through `hookMethod`). Pi's own classes beyond what a wrap needs.
