# Dango v3 + shadcn/ui: design contract (foundation and cockpit shell)

Scope: Plan `webui-phase3-good`, Workflow `ui`, package `web/` (`cpd-web-client`). This file is the contract for the foundation (Tailwind wiring, tokens, fonts, glyphs, `dango-status.ts`, `cn`, shadcn parts, typeset) and the cockpit shell (Header, Context Tree, Status Bar, Shell split, `App.tsx` wiring). The plan view and inbox are in `DESIGN-panes.md`, the transcript, folds and tool rows in `DESIGN-chat.md`, the composer, footer and cards in `DESIGN-compose.md`. Tests are written from this document alone, the implementation from the tests and this document.

Sources: the design handoff (`HANDOFF.md`, `COMPONENTS.md`, `app/globals.css`, `lib/dango-status.ts`) and the boards (`Main.dc.html` and the others). Records: pro-d16, pro-d18, pro-d19, pro-d21, pro-n11 to pro-n15. Accepted assumptions A1 to A6 are restated where they bind.

**Contradiction rule.** Where the handoff files contradict each other, or a file contradicts a board, the board's rendered value wins, and the resolution is stated in one line marked **Resolution**. Where the handoff or a board contradicts Phase 2 behaviour (data, ARIA, tests), Phase 2 behaviour wins unless this file says otherwise.

**File boundary (pro-n15).** The Plan touches only `web/src/ui/**`, `web/src/lib/**`, `web/vite.config.ts` and `web/VERIFY.md`. No dependency is added; only what `web/package.json` on main already has is used: `@shadcn/react` 0.3.1, `@tailwindcss/vite` 4.3.3, `tailwindcss` 4.3.3, `radix-ui`, `lucide-react`, `class-variance-authority`, `clsx`, `tailwind-merge`, `tw-animate-css`, `react-resizable-panels` 4.15.3.

**No backend change (A6).** No change to `web/src/protocol/**`, `web/src/state/**` or `web/src/server/**`. A design element whose data is absent there renders only when an optional prop is passed, is omitted otherwise, and is never faked. Each is listed under "Not wired".

---

## 1. Conventions

### 1.1 Hook names (used by every DESIGN*.md)

| Hook | Meaning | Values |
|---|---|---|
| `data-part="<name>"` | Names a structural slot of a component. kebab-case, unique within one component's DOM. Tests select slots by it. | per component |
| `data-tone="<Tone>"` | The Dango tone of an element that carries status. Always the value that came out of `dango-status.ts`, never hand-written. | `blocked`, `landing`, `decision`, `active`, `focus`, `done`, `unread`, `neutral` |
| `data-glyph="<Glyph>"` | The glyph shape drawn by an element. Always paired with `data-tone` when status is shown. | `blocked`, `landing`, `decision`, `draft`, `active`, `focus`, `done`, `unread`, `queued`, `idle` |
| `data-state="<name>"` | The current state of a stateful component (open, collapsed, selected, sending ...). One attribute per element; the allowed values are listed with the component. Boolean states that already have an ARIA attribute use the ARIA attribute (`aria-current`, `aria-expanded`, `aria-pressed`, `aria-disabled`, `disabled`), not `data-state`. | per component |
| `data-slot="<name>"` | Written by the hand-written shadcn parts (section 2f). Never written by cpd components and never selected by cpd tests. | per part |

Phase 2 hooks (`data-shell`, `data-column`, `data-emblem`, `data-part`, `data-context`, `data-plan`, `data-session`, `data-select`, `data-dot`, `data-attention`, `data-archived`, `data-empty`, `data-workflow`, `data-job`, `data-status`, `data-urgency`, `data-kind`, `data-action`, `data-dialog`, `data-chat-dropzone`, `data-row-key`, `data-at-bottom`, `data-diff`, `data-layout` and every `aria-label` / `role` that exists today) are **KEPT** unchanged. New hooks are added next to them; none is renamed or removed. Where a component below says "KEPT" it lists the Phase 2 hooks of that component.

### 1.2 How styling is written

1. **Tailwind utilities first.** Every cpd component styles itself with Tailwind v4 utility classes in `className`, built with `cn(...)` (2e). No inline `style` (the one exception, `Transcript`, is Phase 2 and unchanged). No CSS module.
2. **Tokens, never hex.** Colours are the token utilities of 2b (`bg-background`, `text-fg-strong`, `border-border`, `text-status-active`, ...). No hex value and no Tailwind palette colour (`red-500`) in a `.tsx` file. The only hex values in the codebase are in `app.css` (`:root, .dark`).
3. **Status colour only through `dango-status.ts`.** A status-coloured element takes its classes from `toneClasses[tone]`, its glyph from `glyphClass[glyph]`, its label from the resolver (2d). Status is never colour alone: every tone is paired with its glyph shape and a visible or `sr-only` label.
4. **Glyph classes.** A glyph is `<span className={glyphClass[g]} aria-hidden />` (the string `glyph glyph-<name>`), or, when it is the only carrier of the state, `<span role="img" aria-label="<label>" className={glyphClass[g]} />`. The glyph CSS is in `app.css` (2c).
5. **Structural rules that tests read stay plain CSS** in `app.css`, unlayered (`[data-shell]`, `[role="log"]`, ...; see 2a). An unlayered rule beats Tailwind utilities (which live in `@layer utilities`), so no element may carry both an unlayered `app.css` rule and a Tailwind utility for the same property.
6. **Text sizes** are the four theme sizes: `text-meta` 11px, `text-key` 12.5px, `text-body` 13px, `text-title` 15px (2b). A board value that is not one of them (12px, 11.5px, 13.5px) is written as an arbitrary value, `text-[12px]`, `text-[11.5px]`, `text-[13.5px]`.
7. **Mono** is for keys, ids, paths, metrics, code and `kbd`: class `font-mono` (ligatures off, 2b). UI labels and prose are `font-sans`.
8. **Focus ring.** Every focusable element shows the Ramune ring on keyboard focus: `outline: 2px solid var(--ring); outline-offset: 1px` (the board's `#a1c6ef`), set once in `@layer base` (2a). **Resolution:** the board adds `border-radius: 5px` to the focus style; that would reshape pills and round buttons on focus, so the radius is not part of the rule.
9. **Hover.** A list row hovers to `bg-row-hover` (`#24212a`, the board's `.row:hover`). A selected row is never hover-tinted over its selection. **Resolution:** the handoff has no hover token between card and raised; the board's rendered `#24212a` wins and is added as the token `--row-hover`.
10. **Disabled** controls keep their label and get the shadcn `disabled:opacity-50 disabled:pointer-events-none` of the Button part plus the `app.css` rule `button:disabled { cursor: not-allowed }`. Where the reason is known it is the `title` of the control.

---

## 2. Foundation

### 2a. Tailwind v4 wiring

**`web/vite.config.ts`** adds the plugin and nothing else (A1):

```ts
import tailwindcss from "@tailwindcss/vite";
// plugins: [react(), tailwindcss(), cpdWebPlugin()]
```

Plugin order is `react()`, `tailwindcss()`, `cpdWebPlugin()`; `server: { port: 5173, strictPort: true }` unchanged. No `@/` alias, no `resolve.alias`, no tsconfig or vitest.config change (A1): every import in `web/src/ui/**` and `web/src/lib/**` is relative with the `.ts` / `.tsx` extension, as today. `web/components.json` is not read by anything and is left alone.

**`web/src/ui/app.css`** stays the single CSS entry; `web/src/main.tsx` keeps its one `import "./ui/app.css";` (no change to main.tsx). Order of its content, top to bottom:

1. `@import "tailwindcss";` (first line, nothing before it)
2. `@import "tw-animate-css";`
3. `@import "../lib/typeset.css";` (2g: the shadcn typeset file, hand-copied, `web/src/lib/typeset.css`)
4. `@custom-variant dark (&:is(.dark *));`
5. `@theme inline { ... }` (2b: tokens)
6. `:root, .dark { ... }` (2b: raw values)
7. `@layer base { ... }` (base rules below)
8. `@utility glyph...` blocks and `@keyframes dango-spin` and the reduced-motion media query (2c)
9. the `.typeset-chat` rule set (2g)
10. the structural rules that stay plain CSS (table below)

`@import "shadcn/tailwind.css"` is **not** used: the `shadcn` package is not a dependency. The cockpit is dark only: the Dango values are set on `:root` and on `.dark`, so components render the same with or without `<html class="dark">`. No light theme.

`@layer base`:

```css
@layer base {
  * { @apply border-border outline-ring/50; }
  html { @apply bg-background; color-scheme: dark; }
  body { @apply bg-background text-foreground font-sans text-body antialiased; margin: 0; }
  code, kbd, samp, pre, .font-mono { font-variant-ligatures: none; }
  ::selection { background-color: var(--line); color: var(--fg-strong); }
  input, textarea, [contenteditable] { caret-color: var(--sakura-bright); }
  button:focus-visible, a:focus-visible, textarea:focus-visible, input:focus-visible,
  select:focus-visible, [role="separator"]:focus-visible, [tabindex]:focus-visible {
    outline: 2px solid var(--ring); outline-offset: 1px;
  }
}
```

Cursor is `#ecaec7` (`--sakura-bright`) and selection is `#5a504a` (`--line`) (record pro-n13).

**Existing `app.css` rules and `app.css.test.ts` assertions: what keeps, what is replaced.** The current file has 17 lines; the test (`web/src/ui/app.css.test.ts`, 3 tests) reads it as text with a regex over `selector { decls }`.

| Existing rule (today) | Fate | Replaced by / reason |
|---|---|---|
| `[data-shell] { display:grid; grid-template-columns: 20rem 1fr 22rem; grid-template-rows: auto minmax(0, 1fr) auto; height:100vh; overflow:hidden; gap:.5rem }` | **kept, columns and gap changed** | `grid-template-columns: 272px minmax(0, 1fr) 340px; grid-template-rows: auto minmax(0, 1fr) auto; height: 100vh; overflow: hidden; gap: 0`. Same selector; `display`, `height`, `overflow`, `grid-template-rows` values are exactly the ones test 1 pins, so test 1 is unchanged. Widths are the boards' 272px (tree) and 340px (inbox). |
| `[data-shell] > * { min-height: 0 }` | **kept** | pinned by test 1 |
| `[data-shell] > aside { grid-row:2; overflow-y:auto; min-width:0 }` | **kept**, borders added by utilities on the asides | pinned by test 3 (tree and inbox scroll independently) |
| `[data-column] { grid-row:2; overflow:auto; min-width:0; display:flex; flex-direction:column }` | **kept** except `overflow: hidden` (the split panels scroll, not the column) | `[data-column="center"] { min-height: 0 }` kept (test 3) |
| `[aria-label="Plan pane"] { max-height:30%; overflow-y:auto; flex:none }` | **replaced** | the resizable plan Panel (3c). Its scrolling is `[data-part="plan-panel"] { overflow-y: auto; min-height: 0 }`. The `max-height` is replaced by the Panel `maxSize="80%"`. Test 3 changes: the `declsMatching(/plan/i)` check keeps `overflow-y: auto` and drops the `max-height` assertion (owned by `ui-impl-shell`). |
| `[data-column] > main { flex:1; min-height:0; overflow:auto }` | **removed** | `main[aria-label="Chat"]` is inside the chat Panel and fills it with `h-full min-h-0` utilities |
| `[data-chat-dropzone] {...}`, `[data-part="transcript"] {...}`, `[role="log"] {...}`, `[data-action="jump-to-bottom"] {...}` | **kept verbatim** | pinned by test 2 and the transcript virtualization contract |
| `[data-diff="add"] { background:#e6ffed }`, `[data-diff="remove"] { background:#ffeef0 }` | **replaced** | light-mode hex on a dark UI; replaced by tone utilities in `DESIGN-chat.md` (`bg-status-done/[0.11]`, `bg-status-blocked/10`). The `data-diff` attributes are KEPT. |
| `[data-side-by-side] { display:grid; grid-template-columns:1fr 1fr; gap:.5rem }` | **replaced (defect fix)** | Known defect: `AskCard` emits `data-layout="side-by-side"` (CONTRACT 1500), but the rule styles `[data-side-by-side]`, which matches nothing, so the two-column layout never applied. The selector becomes `[data-layout="side-by-side"]`; the final layout is in `DESIGN-compose.md`. |
| `button:disabled, textarea:disabled, select:disabled { opacity:.5; cursor:not-allowed }` | **kept** | |
| `[data-shell] > header { grid-row:1; grid-column:1 / -1 }`, `[data-shell] > footer { grid-row:3; grid-column:1 / -1 }` | **kept** | |

Not wired (CSS that stays): there is no responsive layout. The board's `@media (max-width: 980px)` stacking and the `Sheet` drawers of COMPONENTS §2 are not built; below 980px the shell scrolls nothing and clips. Panel sizes are not persisted across reloads.

The tests for new CSS (`web/src/ui/dango.css.test.ts`) read `app.css` as text: they assert the first non-comment line is `@import "tailwindcss";`, that every token of 2b and every glyph rule of 2c exists with exactly the values given there, and that `main.tsx` still imports only `./ui/app.css`.

### 2b. Tokens and fonts

Every token is defined once, in `app.css`. Raw values on `:root, .dark`; Tailwind names in `@theme inline` (`--color-<name>: var(--<name>)`), so `bg-card`, `text-fg-dim`, `border-line` and so on exist as utilities. Hex is the sRGB value of the oklch given by the handoff (checked by the scout).

**Surface ladder, darkest to lightest**

| Var | Hex | oklch | Utility | Use |
|---|---|---|---|---|
| `--surface-sunken` | `#19171c` | `oklch(0.209 0.010 303.6)` | `bg-surface-sunken` | status bar |
| `--background` | `#1c1a20` | `oklch(0.223 0.012 300.6)` | `bg-background` | page, canvas |
| `--card` | `#211f26` | `oklch(0.245 0.013 298.5)` | `bg-card` | cards, workflow cards, assistant message |
| `--surface-raised` | `#26232c` | `oklch(0.263 0.017 300.4)` | `bg-surface-raised` | user message, popovers, `muted`, `accent` |
| `--surface-pressed` | `#2b2731` | `oklch(0.281 0.019 303.4)` | `bg-surface-pressed` | pressed / neutral selected, `secondary` |
| `--border` | `#2e2a33` | `oklch(0.293 0.017 305.4)` | `border-border` | hairlines (structure) |
| `--input` | `#35313b` | `oklch(0.321 0.018 303.5)` | `border-input` | control outlines |
| `--edge` | `#3a3640` | `oklch(0.341 0.018 303.6)` | `border-edge` | chip and kbd outlines |
| `--row-hover` | `#24212a` | (board value) | `bg-row-hover` | list row hover (1.2 item 9) |

**Text ladder**

| Var | Hex | Utility | Use |
|---|---|---|---|
| `--fg-strong` | `#f3ead9` | `text-fg-strong` | titles, clickable neutral text |
| `--foreground` | `#d8d2cc` | `text-foreground` | body text |
| `--muted-foreground` | `#aba3af` | `text-muted-foreground` | secondary text, on any surface |
| `--fg-dim` | `#87808c` | `text-fg-dim` | tertiary text; **only** on `bg-background` and `bg-surface-sunken` |
| `--line` | `#5a504a` | `border-line`, `bg-line` | connectors, separators, dashed borders, selection; never text |

**Dango colour slots** (14: seven hues, base = read-only status, `-bright` = interactive)

| Hue | Base var / hex | Bright var / hex | Meaning |
|---|---|---|---|
| anko | `--anko` `#e17f6e` | `--anko-bright` `#ffa796` | failed, error, blocked |
| mitarashi | `--mitarashi` `#c29653` | `--mitarashi-bright` `#e4bb7f` | landing, checking, merging, ready to merge |
| sakura | `--sakura` `#ca88a3` | `--sakura-bright` `#ecaec7` | waiting on the human; φ; caret |
| mizu | `--mizu` `#67ada7` | `--mizu-bright` `#92d1ca` | agent working |
| ramune | `--ramune` `#7aa2ce` | `--ramune-bright` `#a1c6ef` | focus, selection, the default action |
| matcha | `--matcha` `#8da96e` | `--matcha-bright` `#b3cd96` | merged, passed, done |
| shiro | `--shiro` `#aba3af` | `--shiro-bright` `#f3ead9` | neutral (no status alias) |

Tailwind names: `text-anko`, `bg-mizu-bright`, ... (`--color-<hue>`, `--color-<hue>-bright`). Semantic status aliases (`@theme inline`), the only names status UI uses:

| Alias (`--color-…`) | Base → | Bright → | Utilities |
|---|---|---|---|
| `status-blocked` | `var(--anko)` | `var(--anko-bright)` | `text-status-blocked`, `border-status-blocked`, `bg-status-blocked/10`, `text-status-blocked-bright` |
| `status-landing` | `var(--mitarashi)` | `var(--mitarashi-bright)` | same pattern |
| `status-decision` | `var(--sakura)` | `var(--sakura-bright)` | same pattern |
| `status-active` | `var(--mizu)` | `var(--mizu-bright)` | same pattern |
| `status-focus` | `var(--ramune)` | `var(--ramune-bright)` | same pattern |
| `status-done` | `var(--matcha)` | `var(--matcha-bright)` | same pattern |

**shadcn roles** (`:root, .dark`; names required by shadcn), each also mapped `--color-<role>: var(--<role>)`:

| Role | Value | Role | Value |
|---|---|---|---|
| `--card-foreground` | `var(--foreground)` | `--secondary` / `-foreground` | `var(--surface-pressed)` / `var(--fg-strong)` |
| `--popover` / `-foreground` | `var(--surface-raised)` / `var(--foreground)` | `--muted` | `var(--surface-raised)` |
| `--primary` / `-foreground` | `var(--ramune)` / `var(--background)` | `--accent` / `-foreground` | `var(--surface-raised)` / `var(--fg-strong)` |
| `--destructive` / `-foreground` | `var(--anko)` / `var(--background)` | `--ring` | `var(--ramune-bright)` (`#a1c6ef`) |
| `--chart-1 … --chart-5` | ramune, matcha, mitarashi, sakura, mizu | `--sidebar` / `-foreground` | `var(--background)` / `var(--foreground)` |
| `--sidebar-primary` / `-foreground` | `var(--ramune)` / `var(--background)` | `--sidebar-accent` / `-foreground` | `oklch(0.700 0.078 251.3 / 16%)` (`rgba(122,162,206,.16)`, the selected tree row) / `var(--fg-strong)` |
| `--sidebar-border` / `--sidebar-ring` | `var(--border)` / `var(--ramune-bright)` | `--radius` | `0.5rem` |

**Radius** (`@theme inline`): `--radius-sm: calc(var(--radius) * 0.6)` (about 5px, tree rows), `--radius-md: calc(var(--radius) * 0.8)` (about 6px, buttons, job nodes), `--radius-lg: var(--radius)` (8px), `--radius-xl: calc(var(--radius) * 1.4)` (about 11px), `--radius-2xl: calc(var(--radius) * 1.8)`, `--radius-3xl: calc(var(--radius) * 2.2)`, `--radius-4xl: calc(var(--radius) * 2.6)`.

**Type** (`@theme inline`)

| Token | Value | Utility |
|---|---|---|
| `--font-sans` | `"IBM Plex Sans", ui-sans-serif, system-ui, sans-serif` | `font-sans` (UI and prose) |
| `--font-mono` | `"Maple Mono", "JetBrains Mono", ui-monospace, SFMono-Regular, monospace` | `font-mono` (keys, metrics, code) |
| `--font-display` | `"EB Garamond", ui-serif, Georgia, serif` | `font-display` (the φ mark only) |
| `--font-heading` | `var(--font-sans)` | (read by typeset, 2g) |
| `--text-meta` / `--text-meta--line-height` | `0.6875rem` (11px) / `1rem` | `text-meta` |
| `--text-key` / `--text-key--line-height` | `0.78125rem` (12.5px) / `1.125rem` | `text-key` |
| `--text-body` / `--text-body--line-height` | `0.8125rem` (13px) / `1.45` | `text-body` |
| `--text-title` / `--text-title--line-height` | `0.9375rem` (15px) / `1.3` | `text-title` |

Fonts are CSS stacks only (A3): no `@font-face`, no `@import` of a font, nothing bundled. When a family is not installed the next one in the stack renders. **Resolution:** the board loads JetBrains Mono from Google Fonts and names Maple Mono without loading it; here neither is loaded. `font-variant-ligatures: none` applies to `code, kbd, samp, pre, .font-mono` so `->` and `!=` read literally.

**Motion**: `--animate-glyph-spin: dango-spin 1.1s linear infinite;` (`@theme inline`).

**Colour rules** (from the handoff, binding): colour carries meaning, never decoration; non-status UI uses the neutral ladder `fg-strong > foreground > muted-foreground > fg-dim > line`; base hue = read-only status, `-bright` = clickable; one filled `bg-primary` button per surface (the ⏎ default action); state = 1px border plus an 8 to 16% tint (`border-status-active bg-status-active/10`), solid fills only for glyphs, ticks and the incident banner. **Resolution:** the handoff colours inline code `text-mizu-bright` although mizu means "agent working"; the board renders it (`#92d1ca`), so it stays (2g).

### 2c. Glyph classes (exact CSS)

In `app.css`. Tailwind v4 `@utility` blocks (names are the handoff's; the brief's `.g-run` names do not exist, A3):

```css
@utility glyph {
  display: inline-block;
  flex: none;
  width: 8px;
  height: 8px;
  box-sizing: border-box;
}
@utility glyph-blocked {   /* triangle: failed, error */
  width: 9px;
  background-color: var(--anko);
  clip-path: polygon(50% 0, 100% 100%, 0 100%);
}
@utility glyph-landing {   /* down arrow: checking, merging, ready */
  background-color: var(--mitarashi);
  clip-path: polygon(32% 0, 68% 0, 68% 45%, 100% 45%, 50% 100%, 0 45%, 32% 45%);
}
@utility glyph-decision {  /* diamond: waiting on you */
  background-color: var(--sakura);
  transform: rotate(45deg) scale(0.86);
}
@utility glyph-draft {     /* hollow diamond: draft job */
  border: 1.5px solid var(--sakura);
  transform: rotate(45deg) scale(0.86);
}
@utility glyph-active {    /* spinning arc: running */
  border-radius: 9999px;
  border: 2px solid var(--mizu);
  border-right-color: transparent;
  animation: var(--animate-glyph-spin);
}
@utility glyph-focus {     /* square: review requested */
  border-radius: 2px;
  background-color: var(--ramune);
}
@utility glyph-done {      /* solid dot: merged, done */
  border-radius: 9999px;
  background-color: var(--matcha);
}
@utility glyph-unread {    /* solid neutral dot: unread reply */
  border-radius: 9999px;
  background-color: var(--foreground);
}
@utility glyph-queued {    /* dashed ring */
  border-radius: 9999px;
  border: 1.5px dashed var(--fg-dim);
}
@utility glyph-idle {      /* ring: idle, asleep, cancelled */
  border-radius: 9999px;
  border: 1.5px solid var(--fg-dim);
}

@keyframes dango-spin {
  to { transform: rotate(360deg); }
}

@media (prefers-reduced-motion: reduce) {
  .glyph-active { animation: none; border-right-color: var(--mizu); border-top-color: transparent; }
}
```

| Class | Colour | Shape | Pairs with tone |
|---|---|---|---|
| `glyph-blocked` | anko `#e17f6e` | triangle, 9px wide | `blocked` |
| `glyph-landing` | mitarashi `#c29653` | down arrow | `landing` |
| `glyph-decision` | sakura `#ca88a3` | diamond | `decision` |
| `glyph-draft` | sakura 1.5px border | hollow diamond | `decision` |
| `glyph-active` | mizu 2px arc, right side open, spins 1.1s | arc | `active` |
| `glyph-focus` | ramune `#7aa2ce` | square, 2px radius | `focus` |
| `glyph-done` | matcha `#8da96e` | solid dot | `done` |
| `glyph-unread` | foreground `#d8d2cc` | solid dot | `unread` |
| `glyph-queued` | fg-dim `#87808c` 1.5px dashed | dashed ring | `neutral` |
| `glyph-idle` | fg-dim `#87808c` 1.5px | ring | `neutral` |

**Resolution:** the board's `.g-run` has no reduced-motion rule; the handoff's has, and it is kept (the arc stops, the open side moves to the top). A `.sr-only` utility is Tailwind's own; no custom `.sr` class.

### 2d. `web/src/ui/dango-status.ts`

The single mapping from cpd domain states to Dango tones. Pure module, no React, no DOM; imports only types from `../protocol/cockpit.ts` and `../protocol/domain.ts`. Class strings are literal so Tailwind's scanner finds them. Every `Record` is exhaustive over a closed union; there is no `default` branch and no `as` cast anywhere in the file.

```ts
import type { SessionDot, Urgency } from "../protocol/cockpit.ts";
import type { SessionState } from "../protocol/domain.ts";

export type Tone = "blocked" | "landing" | "decision" | "active" | "focus" | "done" | "unread" | "neutral";
export type Glyph = "blocked" | "landing" | "decision" | "draft" | "active" | "focus" | "done" | "unread" | "queued" | "idle";

export interface ToneClasses { readonly text: string; readonly textBright: string; readonly border: string; readonly tint: string }
export const toneClasses: Readonly<Record<Tone, ToneClasses>>;
export const glyphClass: Readonly<Record<Glyph, string>>;

export interface Display { readonly tone: Tone; readonly glyph: Glyph; readonly label: string }

/** The unknown case. cpd's status strings are open (`string` in cockpit.ts); a value outside the union is `unknown`, carrying the raw text. */
export type Resolved<T> =
  | { readonly kind: "known"; readonly value: T }
  | { readonly kind: "unknown"; readonly raw: string };
export const UNKNOWN_DISPLAY: { readonly tone: "neutral"; readonly glyph: "idle" };

// jobs
export type JobStatus = "draft" | "queued" | "running" | "landing" | "checking" | "merging" | "merged" | "failed" | "cancelled";
export const jobStatus: Readonly<Record<JobStatus, Display>>;
export function parseJobStatus(raw: string): Resolved<JobStatus>;
export function jobDisplay(raw: string): Display;               // known: jobStatus[v]; unknown: { tone: "neutral", glyph: "idle", label: raw }

// sessions (the tree's dot) and plans
export type SessionDisplay = "error" | "landing" | "dialog" | "working" | "unread" | "idle" | "asleep";
export const sessionDisplay: Readonly<Record<SessionDisplay, Display>>;
export function dotDisplay(dot: SessionDot): SessionDisplay;    // domain dot -> display state
export const planPrecedence: readonly SessionDisplay[];         // ["error","landing","dialog","working","unread","idle","asleep"]
export function planState(conditions: readonly SessionDisplay[]): SessionDisplay;

// inbox
export type InboxTier = 1 | 2 | 3 | 4 | 5 | 6;
export interface InboxDisplay extends Display { readonly tier: InboxTier; readonly tierLabel: string }
export const urgencyDisplay: Readonly<Record<Urgency, InboxDisplay>>;
export function inboxDisplay(urgency: Urgency): InboxDisplay;    // = urgencyDisplay[urgency]
export const knownInboxKinds: readonly string[];                 // the 11 kinds of the table below, in table order

// plan phases
export type Phase = "grounding" | "plan" | "dispatch" | "deploy" | "done";
export const phaseTone: Readonly<Record<Phase, Tone>>;
export const phaseLabel: Readonly<Record<Phase, string>>;
export function parsePhase(raw: string): Resolved<Phase>;
```

**Naming resolutions.** (1) The handoff exports `SessionState`/`sessionState`; the web domain already has `SessionState = "working" | "waiting" | "blocked" | "idle" | "stopped"` (`web/src/protocol/domain.ts`), so the Dango display union is `SessionDisplay` / `sessionDisplay` here, to avoid two meanings of one name. (2) The handoff's `Phase` has `planning`; cpd's phase string is `plan` (`PlanNode.phase`: `grounding | plan | dispatch | deploy | done`, and the Plan's own phase line reads `plan`), so the union is `"plan"` and its display label is `planning`. (3) The handoff's `inboxKind`/`InboxKind` keyed by `pr-failing`/`pr-pending` is **not** ported (pro-n11): the inbox resolves from `(urgency, kind)` as cpd gives it, below. (4) `unread` tone: the handoff comment says shiro, its code uses foreground; **Resolution:** code wins, `unread` is `text-foreground` / glyph `#d8d2cc`, and the boards agree (`.g-unread` `#d8d2cc`).

**`toneClasses` (exact strings)**

| Tone | `text` | `textBright` | `border` | `tint` |
|---|---|---|---|---|
| blocked | `text-status-blocked` | `text-status-blocked-bright` | `border-status-blocked` | `bg-status-blocked/10` |
| landing | `text-status-landing` | `text-status-landing-bright` | `border-status-landing` | `bg-status-landing/10` |
| decision | `text-status-decision` | `text-status-decision-bright` | `border-status-decision` | `bg-status-decision/10` |
| active | `text-status-active` | `text-status-active-bright` | `border-status-active` | `bg-status-active/10` |
| focus | `text-status-focus` | `text-status-focus-bright` | `border-status-focus` | `bg-status-focus/15` |
| done | `text-status-done` | `text-status-done-bright` | `border-border` | `""` |
| unread | `text-foreground` | `text-fg-strong` | `border-border` | `""` |
| neutral | `text-muted-foreground` | `text-fg-strong` | `border-dashed border-line` | `""` |

**`glyphClass` (exact strings):** `blocked` `"glyph glyph-blocked"`, `landing` `"glyph glyph-landing"`, `decision` `"glyph glyph-decision"`, `draft` `"glyph glyph-draft"`, `active` `"glyph glyph-active"`, `focus` `"glyph glyph-focus"`, `done` `"glyph glyph-done"`, `unread` `"glyph glyph-unread"`, `queued` `"glyph glyph-queued"`, `idle` `"glyph glyph-idle"`.

**Jobs: `jobStatus`** (`JobView.status` is an open string in `cockpit.ts`; `parseJobStatus` is the single boundary)

| status | tone | glyph | label |
|---|---|---|---|
| `draft` | decision | draft | `draft` |
| `queued` | neutral | queued | `queued` |
| `running` | active | active | `running` |
| `landing` | landing | landing | `landing` |
| `checking` | landing | landing | `checking` |
| `merging` | landing | landing | `merging` |
| `merged` | done | done | `merged` |
| `failed` | blocked | blocked | `failed` |
| `cancelled` | neutral | idle | `cancelled` |
| any other string `s` | neutral | idle | `s` (verbatim); `parseJobStatus` returns `{ kind: "unknown", raw: s }` |

**Sessions: `sessionDisplay`** (Context Tree dots)

| `SessionDisplay` | tone | glyph | label |
|---|---|---|---|
| `error` | blocked | blocked | `Needs attention` |
| `landing` | landing | landing | `Landing` |
| `dialog` | decision | decision | `Waiting on you` |
| `working` | active | active | `Working` |
| `unread` | unread | unread | `Unread reply` |
| `idle` | neutral | idle | `Idle` |
| `asleep` | neutral | idle | `Asleep` |

**`dotDisplay(dot: SessionDot)`**, the mapping from the domain `SessionDot { state: SessionState; attention: Urgency | null }` to `SessionDisplay`. Rule: `state === "blocked"` wins; otherwise a non-null `attention` decides; otherwise `state`.

| `dot.state` | `dot.attention` | result |
|---|---|---|
| `blocked` | any | `error` |
| not `blocked` | `red` | `error` |
| not `blocked` | `yellow` | `landing` |
| not `blocked` | `magenta` | `dialog` |
| not `blocked` | `blue` | `dialog` (review requested has no session display; "waiting on you" is nearest; inferred, unverified) |
| not `blocked` | `cyan` | `working` (in-flight item; inferred, unverified) |
| not `blocked` | `white` | `unread` |
| `working` | `null` | `working` |
| `waiting` | `null` | `idle` |
| `idle` | `null` | `idle` |
| `stopped` | `null` | `asleep` |

(`attention` is always `null` while `state === "working"`, per `cockpit.ts`.)

**`planState(conditions)`** returns the first element of `planPrecedence` that is contained in `conditions`, else `"idle"`. `planPrecedence = ["error", "landing", "dialog", "working", "unread", "idle", "asleep"]`. The Context Tree uses it for the aggregate glyph of a plan row (over `plan.sessions`) and of a context row (over `context.sessions` and every non-archived plan's sessions), after `dotDisplay` on each session.

**Inbox: `urgencyDisplay`, `inboxDisplay(urgency)`.** `InboxRow` carries `urgency: Urgency` (six values) and `kind: string` (open). The board's six inbox groups, the six urgency colours of `cockpit.ts` and the handoff's six tiers line up one to one (red/amber/blue/pink/teal/grey in the board header counts), so **the urgency alone decides the display**; `kind` never changes tone, glyph or tier. A kind outside the known list is therefore not an error: it renders by its urgency.

| `Urgency` | `tier` | `tierLabel` | tone | glyph | label (= `tierLabel`) |
|---|---|---|---|---|---|
| `red` | 1 | `blocking` | blocked | blocked | `blocking` |
| `yellow` | 2 | `delivery` | landing | landing | `delivery` |
| `blue` | 3 | `review` | focus | focus | `review` |
| `magenta` | 4 | `decide` | decision | decision | `decide` |
| `cyan` | 5 | `in flight` | active | active | `in flight` |
| `white` | 6 | `unread` | unread | unread | `unread` |

Every `(urgency, kind)` pair: the 11 kinds known from `web/src/protocol/cockpit.ts` (seen live: `pr-ready`, `dialog`, `unread`, `curate-retire`; documented: `plan-review`, `loop-guard`, `drafts-parked`, `pr-closed`, `review-requested`, `deploy`, `engine-down`) crossed with the six urgencies give 66 pairs, and **every pair `(u, k)` resolves to `urgencyDisplay[u]`**. The "expected urgency" below is what cpd is expected to send for that kind (from the handoff tiers; only `red` and `magenta` are seen live, `docs/commands.md:3910` for the rest; unverified) and fixes the glyph a person sees for a kind in practice:

| `kind` | expected urgency | resolves to (tone / glyph / tier) |
|---|---|---|
| `engine-down` | `red` | blocked / blocked / 1 |
| `loop-guard` | `red` | blocked / blocked / 1 |
| `deploy` | `yellow` | landing / landing / 2 |
| `pr-ready` | `yellow` | landing / landing / 2 |
| `pr-closed` | `yellow` | landing / landing / 2 (inferred) |
| `review-requested` | `blue` | focus / focus / 3 |
| `dialog` | `magenta` | decision / decision / 4 |
| `plan-review` | `magenta` | decision / decision / 4 |
| `drafts-parked` | `magenta` | decision / decision / 4 (inferred) |
| `curate-retire` | `cyan` | active / active / 5 |
| `unread` | `white` | unread / unread / 6 |

`knownInboxKinds` is these 11 strings in this order. If urgency and expected urgency disagree, urgency wins and nothing throws.

**Phases: `phaseTone`, `phaseLabel`, `parsePhase`**

| `Phase` (cpd string) | label | tone |
|---|---|---|
| `grounding` | `grounding` | active |
| `plan` | `planning` | decision |
| `dispatch` | `dispatch` | active |
| `deploy` | `deploy` | landing |
| `done` | `done` | done |
| any other string `s` | `s` | `parsePhase` returns `{ kind: "unknown", raw: s }`; display is `UNKNOWN_DISPLAY` tone and glyph, label `s` |

**Unknown typing, in one line:** cpd strings enter through `parseJobStatus` / `parsePhase` (open `string` in, `Resolved<Union>` out); inside, only the closed unions exist, every table is an exhaustive `Record<Union, …>`, and a caller must branch on `kind` (`"known"` | `"unknown"`) before indexing a table. `UNKNOWN_DISPLAY = { tone: "neutral", glyph: "idle" }` is the only fallback and it is a named constant, not a default branch.

### 2e. `web/src/lib/utils.ts`

```ts
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
```

Pure, no other export. Every shadcn registry item imports an npm package named `cn` that does not exist in `web/package.json`; each part imports this one instead (`../../lib/utils.ts` from `web/src/ui/components/`). `cn("a", false, undefined, ["b"])` is `"a b"`; later Tailwind classes win over earlier conflicting ones (`cn("p-2", "p-4")` is `"p-4"`).

### 2f. Hand-written shadcn parts: `web/src/ui/components/`

Written by hand (A2) from `https://ui.shadcn.com/r/styles/radix-rhea/<name>.json` (`files[0].content`); no shadcn CLI, no new dependency. Four rewrites applied to each registry source: (1) `import { cn } from "cn"` becomes `import { cn } from "../../lib/utils.ts"`; (2) `@/registry/radix-rhea/ui/button` becomes `./button.tsx` (and `./collapsible.tsx` in `trace.tsx`); (3) `IconPlaceholder` becomes the named `lucide-react` icon (`ArrowDownIcon`, `CheckIcon`, `ChevronRightIcon`, `CopyIcon`), keeping its className and props; (4) imports of other parts carry the `.tsx` extension. Each file keeps its `data-slot` attributes. Code-block, `use-highlighted-code` and `highlighter` are **not** written (`shiki` is not a dependency; the CSS-only code-block is not needed by the contracts).

| File | Exports | Variants / props | Source |
|---|---|---|---|
| `button.tsx` | `Button`, `buttonVariants` | `variant`: `default` (primary fill), `outline`, `secondary`, `ghost`, `destructive`, `link`; `size`: `default` (h-8), `xs` (h-6), `sm` (h-7), `lg` (h-9), `icon` (size-8), `icon-xs` (size-6), `icon-sm` (size-7), `icon-lg` (size-9); `asChild`; **added** `tone?: Tone` (Dango): when set, appends `toneClasses[tone].text`, `.border`, `.tint` to an `outline` button (Approve deploy = `landing`, Claim = `focus`, Stop = `blocked`). Writes `data-slot="button" data-variant data-size`. | registry `button` |
| `badge.tsx` | `Badge`, `badgeVariants` | `variant`: `default`, `secondary`, `destructive`, `outline`, `ghost`, `link`; `asChild`; **added** `tone?: Tone`: when set the badge is `outline` plus `toneClasses[tone].text/border/tint`. `data-slot="badge"`. | registry `badge` |
| `textarea.tsx` | `Textarea` | passes through `React.ComponentProps<"textarea">`; `data-slot="textarea"`. | registry `textarea` |
| `collapsible.tsx` | `Collapsible`, `CollapsibleTrigger`, `CollapsibleContent` | wrap `radix-ui` `Collapsible`; `data-slot="collapsible"`, `collapsible-trigger`, `collapsible-content`. | registry `collapsible` |
| `bubble.tsx` | `BubbleGroup`, `Bubble`, `BubbleContent`, `BubbleReactions` | `Bubble` `variant`: `default`, `secondary`, `muted`, `tinted`, `outline`, `ghost`, `destructive`; `align`: `start` \| `end`. | registry `bubble` |
| `marker.tsx` | `Marker`, `MarkerIcon`, `MarkerContent`, `markerVariants` | `variant`: `default`, `separator`, `border`. | registry `marker` |
| `attachment.tsx` | `Attachment`, `AttachmentGroup`, `AttachmentMedia`, `AttachmentContent`, `AttachmentTitle`, `AttachmentDescription`, `AttachmentActions`, `AttachmentAction`, `AttachmentTrigger` | `size`: `default`, `sm`, `xs`; `orientation`: `horizontal`, `vertical`; `data-state`: `idle`, `done`, `error`; `AttachmentMedia` `variant`: `icon`, `image`. | registry `attachment` (uses `./button.tsx`) |
| `trace.tsx` | `Trace`, `TraceLine`, `TraceIcon`, `TraceTitle`, `TraceDescription`, `TraceCollapsible`, `TraceCollapsibleTrigger`, `TraceCollapsibleContent` | `TraceLine` `status`: `running` \| `error` \| `pending`. | registry `trace` (uses `./collapsible.tsx`, `ChevronRightIcon`) |
| `message-scroller.tsx` | `MessageScrollerProvider`, `MessageScroller`, `MessageScrollerViewport`, `MessageScrollerContent`, `MessageScrollerItem`, `MessageScrollerButton`, `useMessageScroller`, `useMessageScrollerScrollable`, `useMessageScrollerVisibility` | built on `@shadcn/react/message-scroller` (namespace `Provider`, `Root`, `Viewport`, `Content`, `Item`, `Button`). Props that matter: provider `autoScroll`, `defaultScrollPosition: "start" \| "end" \| "last-anchor"`, `scrollEdgeThreshold`; viewport `preserveScrollOnPrepend`; item `messageId`, `scrollAnchor`; button `direction: "start" \| "end"`. The file is provided for `DESIGN-chat.md`; the Phase 2 `Transcript` windowing and sticky auto-scroll (`data-row-key`, `data-at-bottom`) are **not** replaced by it. | registry `message-scroller` (uses `./button.tsx`, `ArrowDownIcon`) |
| `questionnaire.tsx` | `Questionnaire`, `QuestionnaireActions`, `QuestionnaireChoice`, `QuestionnaireChoiceDescription`, `QuestionnaireChoices`, `QuestionnaireDescription`, `QuestionnaireError`, `QuestionnaireInput`, `QuestionnaireItem`, `QuestionnaireNext`, `QuestionnairePrevious`, `QuestionnaireProgress`, `QuestionnaireSkip`, `QuestionnaireSubmit`, `QuestionnaireTitle` | built on `@shadcn/react/questionnaire`; `shortcuts: "letters" \| "numbers"`; item `multiple`. | registry `questionnaire` (uses `./button.tsx`, `CheckIcon`) |

`@shadcn/react` has only two subpath exports, `@shadcn/react/message-scroller` and `@shadcn/react/questionnaire`; `import … from "@shadcn/react"` fails. `message.tsx` and `code-block.tsx` are not written. Parts use only Tailwind utilities and the tokens of 2b; their registry class strings are kept except where a token name does not exist here (none known; unverified until the parts are compiled).

### 2g. Typeset

shadcn's `typeset.css` is not a registry item and not a package: it is one file, `https://ui.shadcn.com/typeset.css` (12605 bytes, docs `https://ui.shadcn.com/docs/typeset`). It is hand-copied **verbatim** to `web/src/lib/typeset.css` and imported by `app.css` right after Tailwind (2a). Upstream owns its content; it is never edited. It lives in `@layer components` with `:where()` selectors, so Tailwind utilities win, and it reads `--color-foreground`, `--color-muted-foreground`, `--color-border`, `--font-heading`, `--font-mono` (all defined in 2b).

Its tunables and upstream defaults: `--typeset-font-body: inherit`, `--typeset-font-heading: var(--font-heading)`, `--typeset-font-mono: var(--font-mono)`, `--typeset-size: 1em`, `--typeset-leading: 1.75`, `--typeset-flow: 1.25em`. The default `.typeset` font-size is `calc(var(--typeset-size) * 1.125)` below 48rem and `var(--typeset-size)` from 48rem.

The chat preset, in `app.css`, **unlayered** (it must beat the layered upstream defaults):

```css
.typeset-chat {
  --typeset-size: 1em;       /* the 13px UI body size (text-body) of the parent; the 1.125 mobile bump is cancelled below */
  --typeset-leading: 1.45;   /* the board's 13px / 1.45 */
  --typeset-flow: 0.5em;     /* about 6.5px between blocks: the board's 6px gap */
  font-size: var(--typeset-size);
}
.typeset-chat :where(ol, ul) { padding-inline-start: 20px; }
.typeset-chat :where(li + li) { margin-block-start: 2px; }
.typeset-chat :where(:not(pre) > code) {
  color: var(--mizu-bright);          /* #92d1ca */
  font-family: var(--font-mono);
  font-size: 12px;
}
```

Applied by `Markdown.tsx` (`web/src/ui/chat/Markdown.tsx`): the root element of every rendered Markdown block is `<div className="typeset typeset-chat">`. `typeset-chat` is not applied anywhere else; the user bubble and the shell use plain utilities. **Resolution:** list indent 20px and 2px list gap and the `#92d1ca` 12px inline code are the board's rendered values (the handoff colour rule would forbid the mizu code colour; the board wins).

---
## 3. Cockpit shell

Files: `web/src/ui/cockpit/Header.tsx`, `StatusBar.tsx`, `ContextTree.tsx`, `Shell.tsx`, `web/src/ui/App.tsx`. All imports relative. Tests: `shell.dango.test.tsx`, `tree.dango.test.tsx` (new) and the updated Phase 2 tests (`App.test.tsx`, `AppMemo.test.tsx`, `Frame.test.tsx`, `Shell.test.tsx`, `ContextTree.test.tsx`, `app.css.test.ts`).

Layout, outside in: `div[data-shell]` is a CSS grid, 3 columns `272px | minmax(0,1fr) | 340px`, 3 rows `auto | minmax(0,1fr) | auto`, `height: 100vh`, `overflow: hidden`, no gap, background `bg-background`, text `text-foreground text-body font-sans`. Header spans the top (row 1, 44px), Status Bar the bottom (row 3, 30px), the three columns are row 2. The page never scrolls; each column scrolls on its own.

### 3a. Header (44px)

File `web/src/ui/cockpit/Header.tsx`.

```ts
export interface HeaderProps {
  /** Context and Plan of the Plan pane (from `cockpit.plan`: contextId, planId). null/absent: no breadcrumb. */
  readonly location?: { readonly context: string; readonly plan: string | null } | null;
  /** `inbox.engineRunning`; null before the first snapshot. Absent is treated as null. */
  readonly engine?: boolean | null;
  /** Command palette (⌘K). Absent: the search button is not rendered (Not wired). */
  readonly onOpenSearch?: () => void;
}
export function Header(props: HeaderProps): ReactElement;
```

`<Header />` with no props must render (the Phase 2 call).

DOM (the surrounding `header[aria-label="Cockpit header"]` is written by `Shell`, KEPT):

```
div[data-part="header-bar"]                                  flex h-full items-center gap-4 px-[14px] border-b border-border
├─ a[data-part="home"][href="#"][aria-label="cpd home"]      flex items-center no-underline
│   ├─ span[data-emblem].sr-only      text "C-P-D"            KEPT: the hook and the exact text (Frame.test 1)
│   └─ span[data-part="phi"][aria-hidden="true"]  text "φ"   font-display font-bold text-[28px] leading-none text-fg-strong
├─ nav[aria-label="Location"][data-part="breadcrumb"]        font-mono text-[12px] text-muted-foreground flex items-center gap-1.5   (only when location !== null)
│   ├─ span[data-part="breadcrumb-context"]  {location.context}      text-muted-foreground
│   ├─ span[aria-hidden="true"]  "/"                                  text-line     (only when location.plan !== null)
│   └─ span[data-part="breadcrumb-plan"]     {location.plan}          text-fg-strong (only when location.plan !== null)
├─ span.flex-1
├─ button[data-part="search"][type="button"]                 only when onOpenSearch is passed
│     flex items-center gap-2.5 w-[420px] max-w-[40vw] h-7 px-2.5 rounded-[7px] border border-input bg-card text-muted-foreground text-[12.5px]
│   ├─ svg (lucide SearchIcon) size 14, aria-hidden
│   ├─ span.flex-1  "Jump to context, plan, job, PR…"            (U+2026)
│   └─ kbd.font-mono  "⌘K"     text-meta text-muted-foreground border border-edge rounded-[4px] px-[5px]
├─ span.flex-1                                               (second spacer only when the search button is rendered)
└─ span[data-part="engine"][data-state="running|stopped|unknown"]   flex items-center gap-[7px] text-[12px] text-muted-foreground
    ├─ span[aria-hidden="true"]  className={glyphClass[g]}   g: running → done, stopped → blocked, unknown → idle
    ├─ span.font-mono  "engine"
    └─ span.text-fg-dim  "running" | "not running" | "unknown"
```

- The search button has no `aria-label`; its accessible name is its text `Jump to context, plan, job, PR…`. Click and `⌘K` / `Ctrl+K` (a `keydown` listener on `document` while the Header is mounted, `metaKey || ctrlKey` and `key === "k"`, `preventDefault()`) both call `onOpenSearch`; the listener is registered only when `onOpenSearch` is passed.
- Engine copy is exactly `engine` + `running` | `not running` | `unknown`; `data-state` is `running` when `engine === true`, `stopped` when `false`, `unknown` otherwise. The board's `connected · pi + durable` is replaced: the data is `InboxModel.engineRunning` only. `not running` is the Phase 2 inbox phrase (`Engine is not running`).
- **Resolution:** the φ colour is `#f3ead9` (`text-fg-strong`), as the board renders it; COMPONENTS §9 says Shiro (`#aba3af`) and is overruled. **Resolution:** Phase 2 pins `[data-emblem]` text `C-P-D`; the φ is drawn by a separate aria-hidden span so that text stays exact.
- Not wired: command palette (`onOpenSearch` is not passed by `App`, so no search button), breadcrumb as a link, the engine pill's `pi + durable` detail.

### 3b. Status Bar (30px)

File `web/src/ui/cockpit/StatusBar.tsx`.

```ts
export interface StatusBarProps {
  readonly runner: Runner | null;                 // KEPT
  readonly state: SessionState | null;            // KEPT; narrowed from string (App already passes ref.state)
  readonly version: Stat<string> | null;          // KEPT
  /** `$` cost toggle. Rendered only when `onToggleCost` is passed. */
  readonly showCost?: boolean;
  readonly onToggleCost?: () => void;
  /** Reset layout. Rendered only when passed. */
  readonly onResetLayout?: () => void;
}
export function StatusBar(props: StatusBarProps): ReactElement;
```

`Runner`, `SessionState`, `Stat` are the types of `web/src/protocol/domain.ts`.

DOM (inside `footer[aria-label="Status bar"]`, which `Shell` writes and styles `flex items-center border-t border-border bg-surface-sunken`; the footer is exactly 30px):

```
div[data-part="status-bar"]                                  flex h-full w-full items-center gap-1 overflow-hidden px-2 text-[11.5px]
├─ button[data-part="reset-layout"][type="button"] "Reset layout"   only when onResetLayout; h-[22px] px-2 rounded-[4px] text-fg-strong hover:bg-row-hover
├─ button[data-part="cost-toggle"][type="button"][aria-pressed] "$"  only when onToggleCost; font-mono h-[22px] px-2 rounded-[4px] text-fg-strong; aria-pressed="true" → bg-surface-pressed
├─ span.flex-1
└─ span[data-part="status-right"]                            flex items-center gap-2 font-mono text-meta text-muted-foreground
    ├─ span[data-part="runner"]   "Runner: pi" | "Runner: durable" | "Runner: none"   KEPT, exact text
    ├─ span[aria-hidden="true"] "│"  text-line
    ├─ span[data-part="state"][data-tone][data-glyph]   KEPT hook; exact text "State: <state>" | "State: none"
    │     leading glyph span[aria-hidden] className={glyphClass[g]} when state !== null (g from dotDisplay({state, attention:null}) → sessionDisplay[...].glyph; data-tone from the same)
    ├─ span[aria-hidden="true"] "│"  text-line
    └─ span[data-part="version"]  "cpd: <value>" | "cpd: unknown"        KEPT, exact text
```

- The text of `[data-part="runner"]`, `[data-part="state"]`, `[data-part="version"]` is **exactly** the Phase 2 text (`Runner: durable`, `State: idle`, `cpd: 0.21.102`, `cpd: unknown` when `version` is `null` or `{state:"unknown"}`); the glyph span has no text, so `textContent` is unchanged (`Frame.test` 2, 3, 6 stay green). `data-tone`/`data-glyph` are on the `state` span only when `state !== null`.
- Colours: runner and version text `text-muted-foreground`; the `State:` value is not coloured (the glyph carries the tone); separators `text-line`; the bar sits on `bg-surface-sunken`, so `text-fg-dim` is allowed there and used for `State: none` and `Runner: none` (the empty values).
- Keyboard: both buttons are native, Enter and Space activate, standard focus ring. No shortcut keys.
- Not wired (omitted, never faked): Legend `?`, Logs, Tiers, the `pi | durable` segmented control (the runner is shown, not chosen), the Incident pill, the Workers `52 → 13 → 10/10` meter, version `HoverCard`, host `cpu / ram / disk`. `App` passes none of `showCost`, `onToggleCost`, `onResetLayout`: the cost toggle is meaningless without cost data in the tree or plan models, and layout reset needs the Shell's group handle (not exposed).

### 3c. Shell and the plan/chat split

File `web/src/ui/cockpit/Shell.tsx`.

```ts
export function Shell(props: {
  tree: ReactNode; plan: ReactNode; chat: ReactNode; inbox: ReactNode;   // KEPT
  header?: ReactNode; status?: ReactNode;                                 // KEPT
}): ReactElement;
```

DOM. Children of `div[data-shell]`, in order, KEPT (Frame.test 4 and 5): `header[aria-label="Cockpit header"]` (only when `header !== undefined`), `aside[aria-label="Context tree column"]`, `div[data-column="center"]`, `aside[aria-label="Inbox column"]`, `footer[aria-label="Status bar"]` (only when `status !== undefined`).

```
header[aria-label="Cockpit header"]      bg-background                          (row 1)
aside[aria-label="Context tree column"]  border-r border-border                 (row 2, col 1; overflow-y auto from app.css)
div[data-column="center"][data-part="center-frame"][data-plan-state="open|collapsed|zoomed"]
│                                        relative flex h-full min-h-0 flex-col   (row 2, col 2; app.css: min-height 0)
├─ <Group data-part="center-group" orientation="vertical">   h-full min-h-0 flex-1      (renders a div; its direct DOM children are Panel, Separator, Panel)
│   ├─ <Panel id="plan" data-part="plan-panel">               overflow-y auto (app.css), min-h-0
│   │   └─ section[aria-label="Plan pane"][data-state="open|collapsed|zoomed"]   {plan}    inert while collapsed
│   ├─ <Separator data-part="split-handle" aria-label="Resize plan view and chat">
│   └─ <Panel id="chat" data-part="chat-panel">
│       └─ main[aria-label="Chat"][data-state="open|zoomed"]   h-full min-h-0   {chat}    inert while zoomed
└─ div[data-part="plan-controls"][role="group"][aria-label="Plan pane controls"]   absolute right-4 top-[9px] z-10 flex gap-0.5
    ├─ button[type="button"][data-action="plan-collapse"]                 26x26, aria-label "Collapse plan view" | "Expand plan view"
    └─ button[type="button"][data-action="plan-zoom"][aria-pressed]        26x26, aria-label "Zoom plan view"
aside[aria-label="Inbox column"]         border-l border-border                 (row 2, col 3)
footer[aria-label="Status bar"]          flex items-center border-t border-border bg-surface-sunken   (row 3)
```

`div[data-column="center"]` (the grid child, a plain `div`, KEPT hook) has exactly **2** children, the Group and the controls, so `Frame.test` 4 (`[data-column="center"]` has 2 children) is unchanged. The Group is the library's `Group`; `data-part` and `className` are passed through to its `div`. The Phase 2 descendant selectors `div[data-column="center"] section[aria-label="Plan pane"] #p` and `div[data-column="center"] main[aria-label="Chat"] #c` still match (`Shell.test` unchanged). The plan controls are Shell-owned: the board puts them in the plan title row, but the Plan view is another contract; `DESIGN-panes.md` keeps the right 64px of the plan title row free for them.

`react-resizable-panels` 4.15.3 (named imports `Group`, `Panel`, `Separator`, `usePanelRef`), exact props:

| Element | Props |
|---|---|
| `Group` | `orientation="vertical"`, `id="cpd-center"`, no `defaultLayout`, no storage (sizes are not persisted) |
| plan `Panel` | `id="plan"`, `panelRef={planRef}`, `defaultSize="40%"`, `minSize="20%"`, `maxSize="80%"`, `collapsible`, `collapsedSize="44px"` (the title row stays visible), `onResize={sync}` |
| chat `Panel` | `id="chat"`, `panelRef={chatRef}`, `defaultSize="60%"`, `minSize="20%"`, `collapsible`, `collapsedSize="0%"`, `onResize={sync}` |
| `Separator` | `id="split"`, `aria-label="Resize plan view and chat"`, `data-part="split-handle"`; className `grid h-[9px] place-items-center border-t border-border cursor-row-resize`; one child `span[aria-hidden]` `h-[3px] w-9 rounded-[2px] bg-input` |

`planRef`, `chatRef` come from `usePanelRef()`. `sync()` recomputes `mode` from `planRef.current?.isCollapsed()` and `chatRef.current?.isCollapsed()`: both false → `open`; plan collapsed → `collapsed`; chat collapsed → `zoomed`. Both collapsed cannot happen (the sizes sum to 100%).

Separator semantics come from the library, not written by cpd: `role="separator"`, focusable (`tabindex="0"`), `aria-orientation="horizontal"` (a vertical group has a horizontal bar; unverified), `aria-valuenow`/`aria-valuemin`/`aria-valuemax` (unverified). The library's keyboard handler contains ArrowUp, ArrowDown, ArrowLeft, ArrowRight, Home, End, Enter and F6 (case labels read in `dist/react-resizable-panels.js`; the exact step and effect of each is unverified). Tests may pin only: the separator exists with `role="separator"` and the `aria-label` above, and is focusable. Double-click on it resets the panels to their default sizes (library default). The label `Resize plan view and chat` is the board's.

**Plan controls.** Buttons: both `Button variant="ghost" size="icon-sm"` with `className="size-[26px] rounded-[5px] text-fg-strong"`, icons lucide at size 14 (`ChevronUpIcon`, `ChevronDownIcon`, `MaximizeIcon`, `MinimizeIcon`; existence in lucide-react 1.65 unverified, an equal-meaning icon may replace one). Exact behaviour:

| State (`data-plan-state`) | Collapse button | Zoom button |
|---|---|---|
| `open` | label `Collapse plan view`, `ChevronUpIcon`, enabled; click → `planRef.collapse()` | label `Zoom plan view`, `aria-pressed="false"`, `MaximizeIcon`, enabled; click → `chatRef.collapse()` |
| `collapsed` | label `Expand plan view`, `ChevronDownIcon`, enabled; click → `planRef.expand()` | label `Zoom plan view`, `aria-pressed="false"`, **disabled** |
| `zoomed` | label `Collapse plan view`, `ChevronUpIcon`, **disabled** | label `Zoom plan view`, `aria-pressed="true"`, `MinimizeIcon`, enabled; click → `chatRef.expand()` (restores the previous split) |

- After a click the state hooks update through `onResize → sync()`: `data-plan-state` on the center element, `data-state` on `section[aria-label="Plan pane"]` (`open` | `collapsed` | `zoomed`) and on `main[aria-label="Chat"]` (`open` | `zoomed`; `main` never reads `collapsed`).
- `Enter` / `Space` on a focused button activates it (native). No global shortcut.
- When `collapsed`, the plan `section` gets the `inert` attribute (its content below the 44px title row is clipped and must not take focus); when `zoomed`, `main[aria-label="Chat"]` gets `inert` (its Panel is 0% high). The controls are outside both, so they stay operable.
- Dragging the separator below the plan `minSize` collapses the plan (library behaviour for a `collapsible` panel), above it expands it again; the same for the chat. Both use the same `sync()`.
- jsdom has no layout: a test mocks `react-resizable-panels` (external dependency) and asserts the props table above and that each button calls `collapse()` / `expand()` on the right ref; it does not assert pixel sizes.

Columns: the Context Tree column is `272px`, the Inbox `340px` (CSS only, 2a). Tree, plan panel, chat and inbox scroll independently (`overflow-y: auto` on both asides and on the plan panel; the transcript `[role="log"]` scrolls itself).

Not wired: persisted panel sizes, `Reset layout` (Status Bar), resizable tree and inbox columns, narrow-screen drawers, the board's `Resize` double-click custom behaviour.

### 3d. Context Tree (272px column)

File `web/src/ui/cockpit/ContextTree.tsx`. Props **KEPT**, unchanged:

```ts
export function ContextTree(props: {
  tree: ContextTreeModel | null;
  selectedSessionId: string | null;
  selectedPlanId: string | null;
  onSelectSession(sessionId: string): void;
  onSelectPlan(planId: string): void;
  /** New context. The "+" button is rendered only when passed (Not wired). */
  onNewContext?: () => void;
}): ReactElement;
```

Data: `ContextTreeModel { contexts: ContextNode[]; cursor }`, `ContextNode { id, title, archived, records, openQuestions, sessions: SessionNode[], plans: PlanNode[] }`, `PlanNode { id, contextId, title, phase, archived, counts: PlanCounts, leadSessionId, sessions }`, `SessionNode { ref: SessionRef, contextId, planId, jobKey, archived, dot: SessionDot }` (`web/src/protocol/cockpit.ts`).

DOM:

```
nav[aria-label="Context tree"]                                  KEPT   flex min-h-full flex-col
├─ div[data-part="tree-header"]                                 sticky top-0 z-10 flex h-9 items-center gap-2 bg-background px-[14px]
│   ├─ span  "Contexts"                                         text-[11px] font-semibold uppercase tracking-[.08em] text-muted-foreground
│   ├─ span.flex-1
│   └─ button[data-action="new-context"][aria-label="New context"][type="button"]   only when onNewContext; size-6 grid place-items-center rounded-[5px] text-fg-strong; PlusIcon 14
├─ (tree === null)   p[data-part="loading"]  "Loading contexts…"                 px-[14px] text-fg-dim
├─ (no live and no archived contexts)   p[data-part="empty"]  "No contexts"      px-[14px] text-fg-dim   (new copy)
├─ ul[role="tree"]                                              KEPT   flex flex-col gap-0.5 px-1.5 pb-2.5   (live contexts)
│   └─ li[role="treeitem"][data-context="<id>"]                 KEPT   mb-1
│       ├─ div[data-part="context-row"]       flex h-7 items-center gap-2 rounded-sm px-2
│       │   ├─ span[aria-hidden]  "▾"         font-mono text-[10px] w-[10px] text-fg-dim
│       │   ├─ span[data-part="aggregate"][data-tone][data-glyph]  aggregate glyph (see below)
│       │   └─ span  {context.title}          font-mono text-key font-semibold text-fg-strong flex-1 truncate
│       ├─ ul[role="group"]   plan items (live), then Archived plans (details)         KEPT
│       │   └─ li[role="treeitem"][data-plan="<id>"]            KEPT
│       │       ├─ div[data-part="plan-row"]  flex h-[26px] items-center gap-2 rounded-sm pl-8 pr-2
│       │       │   ├─ span[data-part="aggregate"][data-tone][data-glyph]
│       │       │   ├─ button[type="button"][data-select="plan"][aria-current="true"?]   KEPT   {plan.title}
│       │       │   │     font-mono text-key flex-1 min-w-0 truncate text-left; text-foreground; text-muted-foreground when aggregate is idle/asleep; aria-current → bg-sidebar-accent text-fg-strong; hover:bg-row-hover (not when current)
│       │       │   └─ span[data-part="counts"]  see below
│       │       └─ ul   session rows of the plan          KEPT (no role)
│       └─ ul   session rows of the context (Manager, forks)   KEPT (no role)
└─ details[data-archived]  (only when archived contexts exist)      KEPT
    ├─ summary  "Archived (<n>)"                                    KEPT copy; flex h-7 items-center gap-2 px-2 text-muted-foreground   (mx-1.5)
    └─ ul[role="tree"]  archived contexts, same item structure
```

**Session row** (KEPT structure; the dot and the button are siblings inside one wrapper, `Phase 2 test: button.parentElement.querySelector("[data-dot]")`):

```
li
└─ span[data-part="session-row"]    flex h-[26px] items-center gap-2 rounded-sm pl-8 pr-2   (context sessions and plan sessions alike)
    ├─ span[role="img"][data-dot="<state>"][data-attention="<urgency>|none"][data-tone][data-glyph][aria-label]   KEPT; className={glyphClass[g]}
    └─ button[type="button"][data-session="<ref.id>"][aria-current="true"?]    KEPT
          text exactly `{key}` or `{key} — {title}` (U+2014 with spaces): <span class="font-mono">{key}</span><span class="text-muted-foreground"> — {title}</span>
```

- `data-dot` = `node.dot.state` (domain value); `data-attention` = `attention ?? "none"`; `aria-label` = `state` when `attention === null`, else `` `${state}, needs you (${attention})` `` (all KEPT, Phase 2 test pins `aria-label="waiting"`). **New:** `data-tone` and `data-glyph` from `sessionDisplay[dotDisplay(node.dot)]`; the glyph class is its `glyphClass`. The `role="img"` + `data-dot` hook belongs to **session dots only**: exactly one per session, no other element carries both (Phase 2 test pins the count, 2 for its fixture).
- **Aggregate glyph** (`data-part="aggregate"`, context row and plan row): `planState(sessions.map((s) => dotDisplay(s.dot)))`, then `sessionDisplay[...]`; a `span` with `aria-hidden`, `data-tone`, `data-glyph`, no `role`, className `glyphClass[glyph]`, followed by an `sr-only` span with the display label (`Working`, `Needs attention`, ...). Plan row: sessions = `plan.sessions`; context row: sessions = `context.sessions` plus the `sessions` of every non-archived plan. No sessions → `planState([])` = `idle`.
- **Counts** (`span[data-part="counts"]` on the plan row, `font-mono text-meta min-w-[52px] text-right`): **KEPT text**: the non-zero counts in the order `draft, queued, merged, running, failed, cancelled`, each as `<key> <n>`, joined by ` · ` (`merged 13 · running 1`; Phase 2 pins `toContain("merged 13 · running 1")`). **New:** each count is `<span data-count="<key>">`, the separator ` · ` a text node in `text-line`; tone: `running` `text-status-active`, `merged` `text-status-done`, `failed` `text-status-blocked`, `queued`, `draft`, `cancelled` `text-muted-foreground`; the whole span is `text-fg-dim` when every count is zero (renders empty). **Resolution:** the board draws bare unlabelled numbers `r · q · m · f` and lists no legend (its caveat 7); Phase 2's labelled text wins, so the words are kept.
- **Selection:** `aria-current="true"` on the selected plan button or session button (KEPT); a selected plan row is `bg-sidebar-accent` (`rgba(122,162,206,.16)`) with `text-fg-strong`; a selected session row likewise.
- **Order and grouping** are Phase 2: live contexts, then `details[data-archived]`; inside a context the plans come first (live then archived `details`), then the context's own sessions. **Resolution:** the board's `Active` / `Standing` sections and per-row `$` cost have no source field; they are not drawn.
- **Archived plans** inside a context keep `li > details[data-archived] > summary "Archived (<n>)"` (KEPT).
- Keyboard: every row is a native `button`: Tab order is document order, Enter and Space select. The `role="tree"` has no arrow-key navigation (Phase 2 has none; Not wired).
- Scrolling: the column `aside` scrolls (`overflow-y: auto`); the `tree-header` is sticky.

Not wired: Active/Standing sections, per-context and per-plan cost, `▾ N more` overflow rows (all rows render), collapse and expand of a context (the `▾` caret is static and decorative), the `Archive` count row of the board (replaced by `Archived (n)` details), row context menus, session wake and spinner, `New context` (button rendered only with `onNewContext`; `App` does not pass it).

### 3e. `App.tsx` wiring

File `web/src/ui/App.tsx`. `AppProps` (`transport`, `scheduler`, `reader`, `newId?`), the store and connection creation, the `memo()` wrapping of `ContextTree`, `PlanView`, `Inbox`, the `live = cockpit.tree !== null` gating and `useCallback` handlers are **KEPT** (`AppMemo.test`: streaming deltas must not re-render those three panes).

```tsx
<Shell
  header={<Header
    location={cockpit.plan === null ? null : { context: cockpit.plan.contextId, plan: cockpit.plan.planId }}
    engine={cockpit.inbox === null ? null : cockpit.inbox.engineRunning} />}
  status={<StatusBar runner={session?.ref?.runner ?? null} state={session?.ref?.state ?? null} version={null} />}
  tree={…KEPT…} plan={…KEPT…} chat={…KEPT…} inbox={…KEPT…} />
```

- Chat slot when no session is selected: `<p data-empty>Select a session</p>` (KEPT copy and hook), restyled `grid h-full place-items-center text-muted-foreground`.
- The Plan, Inbox and Chat contents are restyled by `DESIGN-panes.md`, `DESIGN-chat.md` and `DESIGN-compose.md`; their props are not changed by this file.
- `version={null}` stays (the cpd version is not in the store): the Status Bar shows `cpd: unknown`.
- Not wired in `App`: `onOpenSearch`, `onNewContext`, `showCost`/`onToggleCost`, `onResetLayout`; the Header's `location` shows only when a Plan is loaded in the Plan pane (`cockpit.plan`), the breadcrumb is absent for a Manager-only selection.

### 3f. Phase 2 tests this section changes (owner `ui-impl-shell`)

| Test | Change |
|---|---|
| `Frame.test.tsx` 1 | none (`[data-emblem]` text `C-P-D`, now `sr-only`) |
| `Frame.test.tsx` 2, 3, 6 | none (text and hooks KEPT); the `state` prop is typed `SessionState | null` so a test literal like `"idle"` still type-checks |
| `Frame.test.tsx` 4, 5 | none (`labels(c)` of `[data-shell]` children; `[data-column="center"]` has 2 children: Group and controls) |
| `Shell.test.tsx` | none (descendant selectors still match) |
| `ContextTree.test.tsx` 1 to 4 | none (hooks, copy and counts text KEPT) |
| `app.css.test.ts` 1, 2 | none |
| `app.css.test.ts` 3 | the `declsMatching(/plan/i)` assertion drops `max-height` (2a) |
| `AppMemo.test.tsx`, `App.test.tsx` | none |
