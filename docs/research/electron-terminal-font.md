# Electron terminal font vs Ghostty

Package: `@aurigax-ai/ghostty-web` 0.3.9, source in `web/node_modules/@aurigax-ai/ghostty-web/lib/` (cited as `lib/…`). Nothing was launched; no pixels were compared. Anything not read from source is marked **guess:**.

## (1) ctx.font template, units, DPR

- Cell measuring: `lib/renderer.ts:165` `ctx.font = \`${fontWeight} ${fontSize}px ${fontFamily}\`` (CSS px, not pt). Width = `floor(measureText('W').width * dpr) / dpr` (:167); height = `ceil((ascent+descent) * dpr)/dpr` times lineHeight (:168-171).
- Drawing (2D): `lib/renderer.ts:586-590` `fontString(style,size)` = `${italic}${weight} ${size}px ${fontFamily}` (weight = `fontWeightBold` when the cell is bold, else `fontWeight`). Canvas is sized `cssSize * dpr` and `ctx.scale(dpr,dpr)` (:751-754), so size stays in CSS px.
- Drawing (WebGL, what `renderer.ts:32` selects): glyph atlas font `lib/webgl-renderer.ts:661` = `fontString(style, fontSize * devicePixelRatio)`, set on a scratch 2D canvas (:142-154, `willReadFrequently`), so glyphs are rasterised at device pixels, then drawn as textures.
- No `font-smooth`, `text-rendering` or `font-variation-settings` is set anywhere in lib (grep: none).
- Our values: `web/src/electron/terminal.ts:27-29` fontSize 13, fontFamily list; spread into `new Terminal` at `web/src/electron/renderer.ts:32`.

## (2) Options for weight / size / line height

`lib/interfaces.ts:15-20` (and `lib/terminal.ts:205-217`): `fontSize` (default 15), `fontFamily`, `fontWeight` ('normal'|'bold'|number, default 'normal'), `fontWeightBold` (default 'bold'), `lineHeight` (multiplies cell height, default 1). `devicePixelRatio` exists only on the renderer options (`lib/renderer.ts:62`, default `window.devicePixelRatio`). **No `letterSpacing` option** (only used internally, `lib/renderer.ts:736,910`). Runtime changes: `lib/terminal.ts:283-285,307-316`.
Repo typing gap: `web/src/electron/ghostty-web.d.ts:14-15` declares only `fontFamily` and `fontSize`; using `fontWeight`/`lineHeight` needs that d.ts widened (guess: it is a local shim, check it when editing).

## (3) Ghostty's effective size and rendering differences

- Ghostty default font-size: `ghostty +show-config --default` (run, timeout 10 s) prints `font-size = 13`, `font-thicken = false`, `font-thicken-strength = 255`. **Verified** on this machine. `~/.config/ghostty/config` has only `font-family = Maple Mono` and `font-family = Symbols Nerd Font Mono` (lines 11-12); no font-size, adjust-, bold-, freetype-, macos- lines.
- So the nominal size matches: 13 vs 13. Ghostty sizes in points scaled by DPI; on macOS 1pt = 1 CSS px at DPR scale, so equal in principle (guess: not measured). A "different size" is more likely cell metrics: Ghostty derives cell height from CoreText ascent+descent+leading and rounds; canvas uses `fontBoundingBox*` and `ceil` (`lib/renderer.ts:168-171`), and width from `W` floored, not the font's advance. Expect a slightly different cell width/height, and so a different column count.
- "Bolder": Ghostty rasterises through CoreText (default `font-thicken = false`, verified, so no extra thickening). Chromium canvas on macOS uses Skia/CoreText with greyscale AA; glyphs drawn light-on-dark into a WebGL texture atlas and alpha-blended come out heavier (guess: standard gamma/alpha-blend effect; not measured).
- Variable font: `~/Library/Fonts/MapleMono[wght].ttf` exists (checked by `ls`). Whether canvas `400 13px "Maple Mono"` selects the Regular instance and whether `bold` is synthesised was **not tested** (no browser launch allowed). guess: Chromium/CoreText matches a variable font's wght axis to 400 for `normal` and to 700 for `bold` with no synthesis, as the axis covers 100-700 (axis range unread).

## (4) Smallest changes (all unverified)

1. guess: `fontWeight: 300` (or `'300'` equivalent number) in `TERMINAL_FONT` (`terminal.ts:27`) to offset the heavier alpha-blended look; needs the d.ts widened. Try 300, then 350 (variable axis allows it).
2. guess: `lineHeight` tuned (e.g. 1.0-1.1) to match Ghostty's cell height; compare row counts in both windows.
3. guess: CSS `-webkit-font-smoothing` on `#terminal canvas`: expected to have no effect on canvas/WebGL text; do not rely on it.
4. Keep `fontSize: 13`: it is verified equal to Ghostty's default; changing it to 14 has no source support.
Match needs a side-by-side screenshot to confirm each; none has been done.

## (5) Ligatures and fallback

- Ligatures: lib draws text by batches with `letterSpacing` (`lib/renderer.ts:736,783,908-910`), and in WebGL per-glyph atlas entries; ligature use is not set anywhere (guess: Maple Mono's `calt` ligatures may be off in the atlas path, but Ghostty enables `calt` by default; unverified either way).
- Fallback order: canvas font-family lists the same first two families as Ghostty (`Maple Mono`, `Symbols Nerd Font Mono`), so Nerd glyphs come from the same font. Differences: the list adds `MesloLGS Nerd Font Mono`, `MesloLGS NF`, `monospace` after; Ghostty falls back through CoreText's own cascade. Per-glyph fallback inside canvas follows Chromium's rules, not Ghostty's; only glyphs missing from both fonts differ.

## Patch: ghostty-web 0.3.9 (pro-d33)

The patch itself is not written yet; a later Job creates it under `patches/`. Line numbers are for `@aurigax-ai/ghostty-web` 0.3.9 (`lib/…`) as read in this repo. Evidence: `$CPD_HOME/run/electron-tui-polish/explore/scout-render.md` (§1, §2.3, §3) and `scout-glyphs.md` (§4, §5). Decisions: pro-d30 (`padX = ceil(cellW)`) and pro-d33 (geometry for Powerline U+E0B0-E0B7). Accepted assumption A5: bugs 4 and 5 are fixed by this one patch, pinned in `web/package.json`.

### Why

- **Bug 4, wide glyphs clipped.** The WebGL glyph atlas clips each glyph to its slot. The slot pads each side by `padX = Math.ceil(cellW / 2)` (`lib/webgl-renderer.ts:399`; slot width `slotW` in `glyphFor`, `:643`). Ink beyond 1.5 cells from the glyph origin is cut when the glyph is rasterised, and the cut result is cached. Measured on Symbols Nerd Font Mono icons (scout-glyphs §4): about 11 % of a 13 px icon's right edge is lost. The canvas renderer does not clip glyphs (`lib/renderer.ts:945`).
- **Bug 5, dark line on top of pills.** Powerline caps U+E0B0-E0BF are not drawn from geometry. `isBoxGlyph` covers only U+2500-U+259F (`lib/box-glyphs.ts:112-115`), so the font draws them. Maple Mono's cap measures 35 device rows at 13 px, DPR 2, against a 34-row cell (scout-glyphs §5). The cap overshoots the cell top by one row, and the pill body shows a dark step. Both renderers draw the font glyph, so both show the step.

### The two changes

1. **`padX` in `WebglRenderer.measureCellPixels()`**, `lib/webgl-renderer.ts:399`:

   ```ts
   this.padX = Math.ceil(this.cellW / 2);   // before
   this.padX = Math.ceil(this.cellW);       // after
   ```

   The slot in `glyphFor` (`:643`) grows, so the clip now sits at 2 cells from the origin instead of 1.5. Cost: a 1-cell glyph's slot goes from about 2 cells wide to 3 (1.5x, my arithmetic on `:643`; scout-render §4.1 says ~2x, unreconciled). **CanvasRenderer needs nothing:** it has no atlas and no per-glyph clip (`lib/renderer.ts:945`). Its one clip is the block-cursor redraw (`lib/renderer.ts:1018-1024`), which keeps the glyph under a block cursor inside its cell by design; the patch does not touch it.

2. **Powerline geometry in `lib/box-glyphs.ts`**: add U+E0B0-E0B7 to `isBoxGlyph` (`:112`) and draw them in `drawBoxGlyph` (`:121`), inside the cell box `[0,w] x [0,h]` with full height and no overshoot:

   | Codepoint | Glyph | Geometry |
   |---|---|---|
   | U+E0B0 | solid right-pointing triangle | (0,0), (w,h/2), (0,h) |
   | U+E0B1 | right chevron, outline | same points, stroked |
   | U+E0B2 | solid left-pointing triangle | (w,0), (0,h/2), (w,h) |
   | U+E0B3 | left chevron, outline | same points, stroked |
   | U+E0B4 | solid right half-ellipse | flat side at x=0, centre (0,h/2), radii w and h/2 |
   | U+E0B5 | right half-ellipse, outline | same arc, stroked |
   | U+E0B6 | solid left half-ellipse | flat side at x=w, centre (w,h/2), radii w and h/2 |
   | U+E0B7 | left half-ellipse, outline | same arc, stroked |

   - Outline stroke width is the file's existing convention: `light = Math.max(1, Math.round(Math.min(w, h) / 9))` (`:133`), set as `ctx.lineWidth` as `drawArc` does (`:220`). `drawArc` also clips to the cell rectangle (`:217-218`); the outline must do the same so a centred stroke cannot overshoot the cell.
   - No other code changes. WebGL reaches the new branch through `isBoxGlyph` (`lib/webgl-renderer.ts:647`) and `drawBoxGlyph` (`:652`). Canvas reaches it because non-ASCII cells go through `renderCellText` (`lib/renderer.ts:831`), which calls `isBoxGlyph` and `drawBoxGlyph` (`:956-960`).

### How to regenerate

Run from the repo root. The pnpm version is `pnpm@12.6.0` (`package.json:6`).

1. `pnpm patch @aurigax-ai/ghostty-web@0.3.9`: prints the edit directory (`--edit-dir <dir>` to choose it).
2. Edit `lib/webgl-renderer.ts` (line 399) and `lib/box-glyphs.ts` (`isBoxGlyph`, `drawBoxGlyph`) in that directory, and nothing else.
3. `pnpm patch-commit <edit-dir>`: writes the patch file under `patches/` and records it in `pnpm-workspace.yaml` under `patchedDependencies`, keyed `'@aurigax-ai/ghostty-web@0.3.9'`. **Unverified:** the exact file name. I expect pnpm to write `patches/@aurigax-ai__ghostty-web@0.3.9.patch` (scoped `/` becomes `__`); `pnpm patch --help` and `pnpm patch-commit --help` do not state it. Confirm on the patching Job, and confirm that the lockfile is updated too.
4. Commit the patch file, `pnpm-workspace.yaml` and the lockfile together.

The package is pinned exactly at `"@aurigax-ai/ghostty-web": "0.3.9"` (`web/package.json:22`). **Bumping that version** means regenerating the patch against the new source, not just editing the pin. Re-check that both anchors still exist (`this.padX = Math.ceil(this.cellW / 2)` and the `isBoxGlyph`/`drawBoxGlyph` code), and that the patch still applies. Unit tests pin the patched source and geometry; the pixels are checked by hand (accepted assumption A7).

### When to remove

Remove the patch when the ghostty-web version we run does both:

- draws Powerline caps U+E0B0-E0BF from geometry (today only U+2500-U+259F is, `lib/box-glyphs.ts:112-115`), and
- has no half-cell atlas padding (today `padX = ceil(cellW / 2)`, `lib/webgl-renderer.ts:399`).

To remove: delete the file under `patches/`, remove the `patchedDependencies` entry from `pnpm-workspace.yaml`, update the lockfile, and check the pills and wide icons by hand.
