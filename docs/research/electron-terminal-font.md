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
