# Chat-slot pi-pane padding (1 cell, uniform)

Research for decision hs-d684 item 3: the chat slot's pi pane gets one plain 1-cell padding
on all four sides, then every ad-hoc space added for it is removed.

Read from the vendored pi 1.3.0 source (read-only) and this repo. Nothing was run; every
claim below is file:line evidence unless labelled *unverified*.

Paths under `node_modules/@earendil-works/pi-coding-agent/` and `.../pi-tui/` are the vendored
pi 1.3.0 install.

## Answer

A fixed 1-cell pad on all four sides **is achievable**, but not through pi's documented
`ctx.ui.*` regions alone (see "Why `ctx.ui.*` alone cannot do it" below). The mechanism is to
reach the fullscreen TUI through `ctx.ui.setWidget` and replace its layout root with a 1-cell
inset built from pi-tui's own layout primitives:

```ts
import { HStack, Spacer, Text, VStack } from "@earendil-works/pi-tui";
import type { Component } from "@earendil-works/pi-tui";

const padRow = () => ({ component: new Spacer(1), basis: 1, minSize: 1, maxSize: 1 });
const padCol = () => new Text(" ", 0, 0); // one blank column, any row

function padded(root: Component): Component {
  return new VStack([
    padRow(),
    {
      component: new HStack([
        { component: padCol(), basis: 1, minSize: 1, maxSize: 1 },
        { component: root, grow: 1, shrink: 1, minSize: 1 },
        { component: padCol(), basis: 1, minSize: 1, maxSize: 1 },
      ]),
      grow: 1, shrink: 1, minSize: 1,
    },
    padRow(),
  ]);
}
```

The wrapper is installed by hooking the setter `pi` itself calls. cpd already monkey-patches
pi-tui renderer methods (`compact-tools-viewport.ts:176` hooks `TuiAltScreen.prototype.doRender`),
so hook `TuiAltScreen.prototype.setLayoutRoot` the same way and wrap the argument inside the hook
(pass `undefined` through unchanged). `pi` calls `tui.setLayoutRoot(this.fullscreenLayoutRoot)` at
every mount (`interactive-mode.js:593`), so the hook catches it. Using the TUI handle from the
`ctx.ui.setWidget` factory (`compact-tools-viewport.ts:212`) is how cpd reaches the renderer.

The pad sits *outside* the transcript ScrollView, so the top row does not scroll away: it is a
sibling in a new outer `VStack`, not a child of the scroll view.

Evidence:

- The fullscreen root is exactly two stacked regions with no padding:
  `chat-viewport.js:4` (transcript `ScrollView`), `:12` (dock `VStack` of
  pendingMessages/status/widgetsAbove/editor/widgetsBelow/footer), `:22-26` (root `VStack`
  `[transcript, dock]`).
- The layout engine lays a `VStack`/`HStack` out and renders each child at its allocated
  rect: `layout.js:98` (`vstack`), `:150-177` (`hstack`), `:28`/`:31` (measure), `:275`
  (`renderLayoutFrame`), and honours `basis`/`grow`/`shrink`/`minSize`/`maxSize`
  (`stack.js:99-110`).
- `setLayoutRoot` is public on the viewport TUI: `tui.d.ts:248-252` (`ViewportTUI.setLayoutRoot`)
  and `tui.d.ts:251`; `isViewportTUI` at `tui.d.ts:253`.
- The widget factory receives the TUI: `interactive-mode.js:1881` (`content(this.ui, theme)`)
  and the API type `types.d.ts:102-104`. cpd already uses this to reach the TUI:
  `compact-tools-viewport.ts:212`.
- `Spacer` renders blank lines (`spacer.js`), and `HStack` composites a fixed-width column
  (`h-stack.js`); both are exported (`pi-tui/dist/index.d.ts:7,16,19`).

Trade-off: this uses pi-tui internals (`ViewportTUI.setLayoutRoot` / the `TuiAltScreen`
prototype), not a documented padding option, so it must be re-applied to every fresh TUI
(a `/settings` mode switch or a `/reload` builds a new one) and it can break on a pi-tui
upgrade. It is also *unverified*: the sketch above has not been run.

### Why `ctx.ui.*` alone cannot do it

pi's extension UI exposes only these fixed regions: `pendingMessages`, `status`,
`widgetsAbove`, `editor`, `widgetsBelow`, `footer` (`chat-viewport.js:12-20`). A widget can
only be placed `"aboveEditor"` or `"belowEditor"` (`types.d.ts:47-51`), and there is no
`ctx.ui` call that wraps the root or the transcript.

The only top-of-pane lever, `ctx.ui.setHeader`, is *inside* the scroll view: the
`documentContainer` (header + chat) is the ScrollView's child
(`interactive-mode.js:388-391`, `chat-viewport.js:4`), and `setHeader` swaps a component into
`headerContainer` (`interactive-mode.js:1980-2015`). So its blank first row scrolls away with
the content — exactly the "failed top-row attempt". No settings option adds a viewport pad:
`outputPad` pads only the transcript (`settings.md:103`, `settings-manager.d.ts:241`) and
`editorPaddingX` only the editor (`settings.md:102`, `settings-manager.d.ts:239`).

### Closest achievable with `ctx.ui.*` only

- Left/right, transcript: `outputPad: 1` (default; `settings-defaults.d.ts:46`, `settings.md:103`).
- Left/right, editor: `editorPaddingX: 1` (`settings-defaults.d.ts:45`, `settings.md:102`).
- Bottom: `setStatus("cpd-pad", "")` — an empty extension status makes the footer draw a blank
  last row (`adapter.ts:38`; `footer.js:252-258`; `footer-data-provider.js:117`). Fixed.
- Top: none. `setHeader` scrolls (`adapter.ts:40`).

Trade-off: no fixed top row, and left/right is per-region (footer/status/widgets are not
covered), so it is not uniform. This is the current state if the root-wrapper is rejected.

## Ad-hoc chat-slot spacing sites

| Site (file:line) | What it is | Does the uniform pad replace it? |
| --- | --- | --- |
| `pi/lib/adapter.ts:38` `ctx.ui.setStatus("cpd-pad", "")` | blank bottom row through the footer's extension-status line (`footer.js:252-258`) | **Yes, remove.** The root pad's bottom spacer is the fixed blank row. |
| `pi/lib/adapter.ts:40` `ctx.ui.setHeader(() => ({ render: () => [""] }))` | blank top row through `headerContainer` (`interactive-mode.js:1980-2015`) | **Yes, remove.** It scrolls (the bug the decision names); the root pad's top spacer is fixed. |
| `pi/lib/render.ts:143` `const MARGIN = " "`, used `:169,173,181,186,188` | one leading space on every cpd fold row | **Yes, remove.** The root HStack supplies the left column. |
| `pi/lib/render.ts:133-135` `outputPadLines`, `:231` (`outputPad === 0 ? outputPadLines(draw(width+1), width) : draw(width)`) | reclaims that MARGIN column when `outputPad` is 0 | **Yes, obsolete.** Without MARGIN there is no column to reclaim. |
| `pi/compact-tools/extensions/compact-tools-codex.ts:20` `CODEX_MARGIN = " "` used `compact-tools-codex-rows.ts:235,239,242,259,262,294,319` and `compact-tools-grouping.ts:465` | one leading space on every Codex row | **Yes, remove.** The root HStack supplies it. |
| `pi/compact-tools/extensions/compact-tools-layout.ts:28-63` `outputPadLines`/`withOutputPad`; `compact-tools.ts:355,623-624,674`; `compact-tools-grouping.ts:362-375` | reclaims `CODEX_MARGIN` when `outputPad` is 0 | **Yes, obsolete.** Same reason as `render.ts`. |
| `pi/compact-tools/extensions/compact-tools-balloon.ts:37` agent `" " + row` | one-cell left margin on the agent balloon | **Yes, remove the left margin.** The balloon still aligns to the padded content edge. |
| `pi/compact-tools/extensions/compact-tools-balloon.ts:39-40` user `margin + row + " "` (`MIN_BALLOON_WIDTH` at `:11` counts the 4 margins) | one-cell right margin on the user balloon | **Yes, remove the outer right margin.** The root supplies it; the balloon's right alignment stays. |
| `tui/src/chat.rs:35` `Block::new().padding(Padding::left(1))` | the placeholder's own left pad | **Yes, replace.** The chat slot pad supplies the left cell. |
| `tui/src/history/view.rs:41-42` `Layout::vertical([Length(1), Min(0), Length(1)])`, comment "The pane's last row stays empty" | history's blank last row | **Yes, replace the last row.** The uniform bottom pad supplies it; the `Length(1)` **head** is the title, not padding — keep it. |
| `pi/compact-tools/extensions/compact-tools-editor.ts:18` `SIDE = 3` ("Margin + wall + padding on each side"), used `:65-95` | editor box: 1 outer margin + wall + inner pad on each side | **Related, decide in the pad Job.** The root pad makes the editor's *outer* left/right margin redundant; the wall/inner pad are the box's own chrome and stay. |

Not slot padding (keep under the uniform pad): `CODEX_INDENT`/`CODEX_COMMAND_INDENT`
(`compact-tools-codex.ts:16,18`), `CLAUDE_INDENT`/`CLAUDE_TITLE_INDENT`
(`compact-tools-claude.ts:31,33`), `compact-tools-activity.ts:121` indent. These are tree/code
indentation inside a row, not space around the chat slot.

## One-liner

pi 1.3.0 has no padding option on the fullscreen root, so `ctx.ui.*` alone cannot make a fixed
top pad; the concrete way to a true 1-cell pad on all four sides is to wrap the layout root
(`ViewportTUI.setLayoutRoot`) reached through `ctx.ui.setWidget`, then delete the ad-hoc
spaces above.
