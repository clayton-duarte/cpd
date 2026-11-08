# Dango v3 + shadcn/ui: design contract (foundation and cockpit shell)

Scope: Plan `webui-phase3-good`, Workflow `ui`, package `web/` (`cpd-web-client`). This file is the contract for the foundation (Tailwind wiring, tokens, fonts, glyphs, `dango-status.ts`, `cn`, shadcn parts, typeset) and the cockpit shell (Context Tree, Status Bar, Shell split, `App.tsx` wiring; the Header bar is removed, 3a). The plan view and inbox are in `DESIGN-panes.md`, the transcript, folds and tool rows in `DESIGN-chat.md`, the composer, footer and cards in `DESIGN-compose.md`. Tests are written from this document alone, the implementation from the tests and this document.

Sources: the design handoff (`HANDOFF.md`, `COMPONENTS.md`, `app/globals.css`, `lib/dango-status.ts`) and the boards (`Main.dc.html` and the others). Records: pro-d16, pro-d18, pro-d19, pro-d21, pro-n11 to pro-n15. Accepted assumptions A1 to A6 are restated where they bind.

**Contradiction rule.** Where the handoff files contradict each other, or a file contradicts a board, the board's rendered value wins, and the resolution is stated in one line marked **Resolution**. Where the handoff or a board contradicts Phase 2 behaviour (data, ARIA, tests), Phase 2 behaviour wins unless this file says otherwise.

**File boundary (pro-n15).** The Plan touches only `web/src/ui/**`, `web/src/lib/**`, `web/vite.config.ts` and `web/VERIFY.md`, plus the one field of 3d (`PlanCounts.merging`, pro-d43) and the one pin of `web/package.json` for `@base-ui/react` (pro-d72). One dependency is added: `@base-ui/react` 1.9.0, exact pin. **Component convention:** every WebUI part comes from shadcn's base registry (Base UI), never Radix UI and never React Aria; this supersedes the "for the `ScrollArea` wrapper only (2f)" clause and the "`radix-ui` (the other primitives)" gloss in the list below. Otherwise only what `web/package.json` on main already has is used: `@shadcn/react` 0.3.1, `@tailwindcss/vite` 4.3.3, `tailwindcss` 4.3.3, `radix-ui` (in `package.json` on main; per the 2f convention no part wraps it after the migration), `lucide-react`, `class-variance-authority`, `clsx`, `tailwind-merge`, `tw-animate-css`, `react-resizable-panels` 4.15.3.

**No backend change (A6).** *Superseded in part by pro-d46 (3g): the menu action kinds, `readmodel.ts` and `events.ts` change.* No change to `web/src/protocol/**`, `web/src/state/**` or `web/src/server/**`, except the one field `PlanCounts.merging?: number` in `web/src/protocol/cockpit.ts`, parsed by the read model (`web/src/server/cockpit/readmodel.ts`), pro-d43 (3d); and, pro-d40, the status snapshot the server sends to the Status Bar (its sources are in 3b, "Data sources") and the optional cost of a Context or Plan row (3d). Their wire shape is set by the implementation Job's tests, not here. A design element whose data is absent there renders only when an optional prop is passed, is omitted otherwise, and is never faked. Each is listed under "Not wired".

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
3. `@import "../lib/shadcn-tailwind.css";` (2h: the shadcn utilities: keyframes, custom variants, `no-scrollbar`, `scroll-fade*`, `shimmer*`; pro-d55)
4. `@import "../lib/typeset.css";` (2g: the shadcn typeset file, hand-copied, `web/src/lib/typeset.css`)
5. `@custom-variant dark (&:is(.dark *));`
6. `@theme inline { ... }` (2b: tokens)
7. `:root, .dark { ... }` (2b: raw values)
8. `@layer base { ... }` (base rules below)
9. `@utility glyph...` blocks and `@keyframes dango-spin` and the reduced-motion media query (2c)
10. the `.typeset-chat` rule set (2g)
11. `@keyframes prompt-border-pulse` and `.prompt-border-pulse` (2h; pro-d58)
12. the structural rules that stay plain CSS (table below)

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
| `[data-shell] { display:flex; flex-direction:column; height:100vh; overflow:hidden }` | **replaced by flex (pro-d87)**; the Phase 2 grid rule is gone | No grid, no tracks, no gap (pro-d87, A2). The columns group `cpd-columns` (3c) is the first child and takes the free height (`min-h-0 flex-1`); the Status Bar footer is the last child. The three columns are the panels of that group, not grid tracks. Widths are the boards' 272px (tree) and 340px (inbox). |
| `[data-shell] > * { min-height: 0 }` | **kept** | pinned by test 1 |
| `[data-shell] > aside { grid-row:2; overflow-y:auto; min-width:0 }` | **replaced** | `[data-shell] aside { height: 100%; overflow: hidden; min-width: 0 }` (pro-d69, A9, A10). The asides are inside the tree and inbox Panels (3c), not direct children of `[data-shell]`; the descendant selector matches them. The asides clip and never scroll themselves; their content scrolls through `ScrollArea` (3c, Scrolling). |
| `[data-column] { grid-row:2; overflow:auto; min-width:0; display:flex; flex-direction:column }` | **kept** except `grid-row` and `overflow`: `height: 100%; overflow: hidden; min-width: 0; display: flex; flex-direction: column` (the split panels scroll, not the column; the column fills its Panel) | `[data-column="center"] { min-height: 0 }` kept (test 3) |
| `[aria-label="Plan pane"] { max-height:30%; overflow-y:auto; flex:none }` | **replaced** | the resizable plan Panel (3c). Its clip is `[data-part="plan-panel"] { overflow: hidden; min-height: 0 }` (pro-d69, A10); the Plan content scrolls through `ScrollArea` inside its section (3c, Scrolling). The `max-height` is replaced by the Panel `maxSize="80%"`. Test 3 changes: the `declsMatching(/plan/i)` check asserts `overflow: hidden` (A10) and drops the `max-height` assertion (owned by `ui-impl-shell`). |
| `[data-column] > main { flex:1; min-height:0; overflow:auto }` | **removed** | `main[aria-label="Chat"]` is inside the chat Panel and fills it with `h-full min-h-0` utilities |
| `[data-chat-dropzone] {...}`, `[data-part="transcript"] {...}` | **kept verbatim** | pinned by test 2 and the transcript virtualization contract |
| `[role="log"] {...}`, `[data-action="jump-to-bottom"] {...}` | **removed** (pro-d55) | the transcript is a shadcn `MessageScroller` (DESIGN-chat 2.8): its viewport and its `MessageScrollerButton` (`data-action="jump-to-bottom"`) position and style themselves |
| `[data-shell] aside[aria-label="Context tree column"] { overflow: hidden }` | **added** (pro-d50) | the tree column never scrolls itself; the Sidebar's `SidebarContent` scrolls its sections |
| `[data-diff="add"] { background:#e6ffed }`, `[data-diff="remove"] { background:#ffeef0 }` | **removed** | light-mode hex on a dark UI. The `data-diff` attributes are KEPT on `CodeBlockLine`; the tone utilities are in `DESIGN-chat.md` 3.6 (`bg-status-done/[0.11]`, `bg-status-blocked/10`). |
| `[data-side-by-side] { display:grid; grid-template-columns:1fr 1fr; gap:.5rem }` | **replaced (defect fix)** | Known defect: `AskCard` emits `data-layout="side-by-side"` (CONTRACT 1500), but the rule styles `[data-side-by-side]`, which matches nothing, so the two-column layout never applied. The selector becomes `[data-layout="side-by-side"]`; the final layout is in `DESIGN-compose.md`. |
| `button:disabled, textarea:disabled, select:disabled { opacity:.5; cursor:not-allowed }` | **kept** | |
| `[data-shell] > footer { flex-shrink: 0 }` | **changed (pro-d87)**; `[data-shell] > header` **removed** | The `[data-shell] > header` rule and the `grid-row` / `grid-column` rules go with the Header bar and the grid (pro-d87, A2). The footer no longer takes a grid row; it does not shrink. |

Not built (CSS that stays): there is no responsive layout. The board's `@media (max-width: 980px)` stacking and the `Sheet` drawers of COMPONENTS §2 are not built; below 980px the shell scrolls nothing and clips. Panel sizes are not persisted across reloads (3c).

The tests for new CSS (`web/src/ui/dango.css.test.ts`) read `app.css` as text: they assert the first non-comment line is `@import "tailwindcss";`, that every token of 2b and every glyph rule of 2c exists with exactly the values given there, and that `main.tsx` still imports only `./ui/app.css`.

### 2b. Tokens and fonts

Every token is defined once, in `app.css`. Raw values on `:root, .dark`; Tailwind names in `@theme inline` (`--color-<name>: var(--<name>)`), so `bg-card`, `text-fg-dim`, `border-line` and so on exist as utilities. Hex is the sRGB value of the oklch given by the handoff (checked by the scout).

**Surface ladder, darkest to lightest**

| Var | Hex | oklch | Utility | Use |
|---|---|---|---|---|
| `--surface-sunken` | `#19171c` | `oklch(0.209 0.010 303.6)` | `bg-surface-sunken` | status bar |
| `--background` | `#1c1a20` | `oklch(0.223 0.012 300.6)` | `bg-background` | page, canvas |
| `--card` | `#211f26` | `oklch(0.245 0.013 298.5)` | `bg-card` | cards, workflow cards, the composer (assistant text has no card, DESIGN-chat 2.4) |
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
| `--font-sans` | `"Roboto", sans-serif` | `font-sans` (UI and prose; bundled Roboto) |
| `--font-mono` | `"Roboto Mono", monospace` | `font-mono` (keys, metrics, code; bundled Roboto Mono) |
| `--font-display` | `var(--font-sans)` | `font-display` (the φ mark only; display = sans) |
| `--font-heading` | `var(--font-sans)` | (read by typeset, 2g) |
| `--text-meta` / `--text-meta--line-height` | `0.6875rem` (11px) / `1rem` | `text-meta` |
| `--text-key` / `--text-key--line-height` | `0.78125rem` (12.5px) / `1.125rem` | `text-key` |
| `--text-body` / `--text-body--line-height` | `0.8125rem` (13px) / `1.45` | `text-body` |
| `--text-title` / `--text-title--line-height` | `0.9375rem` (15px) / `1.3` | `text-title` |

Roboto and Roboto Mono are bundled (decision pro-d48): the variable fonts ship in `web/src/ui/fonts` (normal and italic each), are loaded with four `@font-face` rules in `app.css` (`font-display: block`), and their SIL OFL licences are kept next to the files. Display is the sans family. `font-variant-ligatures: none` applies to `code, kbd, samp, pre, .font-mono` so `->` and `!=` read literally.

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

**Resolution (Inbox, DESIGN-panes 2):** the Inbox draws no tier groups, no per-tier counts and no tier labels. It uses `tone` and `glyph` only, so `tier` and `tierLabel` stay as data and are not drawn by the Inbox.

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

Written by hand (A2) from shadcn's **base registry**, style `base-nova`: `https://ui.shadcn.com/r/styles/base-nova/<name>.json` (`files[0].content`); no shadcn CLI, no dependency beyond the `@base-ui/react` 1.9.0 pin (File boundary). Four rewrites applied to each registry source: (1) `import { cn } from "cn"` becomes `import { cn } from "../../lib/utils.ts"`; (2) a registry-internal part import (`@/registry/<style>/ui/button`) becomes `./button.tsx` (and `./collapsible.tsx` in `trace.tsx`); (3) `IconPlaceholder` becomes the named `lucide-react` icon (`ArrowDownIcon`, `CheckIcon`, `ChevronRightIcon`, `CopyIcon`), keeping its className and props; (4) imports of other parts carry the `.tsx` extension. Each file keeps its `data-slot` attributes.

**Base UI convention.** Every WebUI part comes from shadcn's base registry (Base UI): never Radix UI, never React Aria. Rows in the table below that name a `radix-ui` primitive are superseded: the part wraps the matching Base UI primitive instead (`collapsible` → `@base-ui/react/collapsible`, `context-menu` → `@base-ui/react/context-menu`, `dialog` → `@base-ui/react/dialog`, `dropdown-menu` → `@base-ui/react/menu`, `hover-card` → `@base-ui/react/preview-card`, `progress` → `@base-ui/react/progress`, `select` → `@base-ui/react/select`, `tooltip` → `@base-ui/react/tooltip`; `sidebar.tsx`'s `radix-ui` `Slot` (`asChild`) composition becomes Base UI's `render` prop). Two API shifts against the Radix registry, recorded for every converted part: (1) composition is a `render` prop — `render={<Element />}` or `render={(props, state) => …}` — never `asChild`: every `asChild` prop named in a row below is a `render` prop; (2) open/closed styling keys on the discrete boolean attributes `data-open` / `data-closed` (and the animation phases `data-starting-style` / `data-ending-style`), never `data-state="open"` / `data-state="closed"`. The cpd hooks of 1.1 (`data-part`, `data-empty`, …) are unchanged by this. `web/components.json`'s `tailwind.css` field is stale — it points at `src/index.css`, which does not exist — and must be corrected to `src/ui/app.css` (2a).

Code-block, `use-highlighted-code` and `highlighter` are **not** written (`shiki` is not a dependency; the CSS-only code-block is not needed by the contracts).

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
| `resizable.tsx` | `ResizablePanelGroup`, `ResizablePanel`, `ResizableHandle`, `usePanelRef` | thin wrappers over `react-resizable-panels` 4.15.3 (`Group`, `Panel`, `Separator`, `usePanelRef`), no new dependency. `ResizablePanelGroup` is `flex h-full w-full` and turns vertical with `aria-[orientation=vertical]`; `ResizablePanel` is `min-h-0 min-w-0`; `ResizableHandle` `withHandle` draws a grip (`h-6 w-1`, `rounded-lg`) on a 1px `bg-border` bar with a 8px hit area. Every resizable split in the Shell uses these parts (3c). `data-slot` `resizable-panel-group`, `resizable-panel`, `resizable-handle`. | shadcn/ui `resizable`, hand-written as above |
| `context-menu.tsx` | `ContextMenu`, `ContextMenuTrigger`, `ContextMenuContent`, `ContextMenuItem`, `ContextMenuSeparator` | shadcn/ui `context-menu`, hand-written as above, over the `ContextMenu` primitive of `radix-ui` (already in `web/package.json`; the export is **unverified** until the part compiles). Used by the Inbox row (DESIGN-panes 2d) and the tree rows (3g). `data-slot` `context-menu-*`. | shadcn/ui `context-menu`, hand-written as above |
| `dialog.tsx` | `Dialog`, `DialogContent`, `DialogHeader`, `DialogTitle`, `DialogDescription`, `DialogFooter` | wrap the `radix-ui` `Dialog` primitive (already in `web/package.json`), hand-written as above; `data-slot` `dialog`, `dialog-content`. The three popups of the Status Bar (3b) use it. Not yet in `components/` (pro-d40). `DialogContent` scrolls through `ScrollArea` (pro-d72; 3c, Scrolling). | shadcn/ui `dialog`, hand-written as above |
| `scroll-area.tsx` | `ScrollArea`, `ScrollBar` | the base variant over `ScrollArea` of `@base-ui/react/scroll-area` (exact pin 1.9.0, pro-d72; `radix-ui` keeps the other primitives). Parts `Root`, `Viewport`, `Content`, `Scrollbar`, `Thumb`, `Corner`. `ScrollArea` takes the Base UI Root props, wraps its children in `Content` (`data-slot` `scroll-area-content`) inside the Viewport, and always renders a vertical and a horizontal `ScrollBar` (pro-d72: every ScrollArea gets the horizontal bar by default); `ScrollBar` is vertical by default and `orientation="horizontal"` for the other axis, and sets `data-orientation`. `data-slot` `scroll-area`, `scroll-area-viewport`, `scroll-area-content`, `scroll-area-scrollbar`, `scroll-area-thumb`. A Viewport's `render` prop replaces its element (the transcript, DESIGN-chat 2.2). Users: 3c, Scrolling. | shadcn/ui `scroll-area`, base variant (pro-d72) |
| `sidebar.tsx` | `Sidebar`, `SidebarContent`, `SidebarGroup`, `SidebarGroupAction`, `SidebarGroupContent`, `SidebarGroupLabel`, `SidebarMenu`, `SidebarMenuAction`, `SidebarMenuBadge`, `SidebarMenuButton`, `SidebarMenuItem`, `SidebarMenuSub`, `SidebarMenuSubButton`, `SidebarMenuSubItem` | styled wrappers only, no state and no context: no `SidebarProvider`, no `useSidebar`, no cookie or storage. `Sidebar` `collapsible`: `offcanvas`, `icon`, `none` (the Context Tree uses `none`). Uses `radix-ui` `Slot` (`asChild`) and `class-variance-authority`, both already in `web/package.json`. `data-slot` and `data-sidebar` on each part. The Context Tree (3d) is the only consumer. | shadcn/ui `sidebar`, hand-written as above |
| `message-scroller.tsx` | `MessageScrollerProvider`, `MessageScroller`, `MessageScrollerViewport`, `MessageScrollerContent`, `MessageScrollerItem`, `MessageScrollerButton`, `useMessageScroller`, `useMessageScrollerScrollable`, `useMessageScrollerVisibility` | built on `@shadcn/react/message-scroller` (namespace `Provider`, `Root`, `Viewport`, `Content`, `Item`, `Button`). Props that matter: provider `autoScroll`, `defaultScrollPosition: "start" \| "end" \| "last-anchor"`, `scrollEdgeThreshold`; viewport `preserveScrollOnPrepend`; item `messageId`, `scrollAnchor`; button `direction: "start" \| "end"`. The file is provided for `DESIGN-chat.md`; the Phase 2 `Transcript` windowing and sticky auto-scroll (`data-row-key`, `data-at-bottom`) are **not** replaced by it. | registry `message-scroller` (uses `./button.tsx`, `ArrowDownIcon`) |
| `questionnaire.tsx` | `Questionnaire`, `QuestionnaireActions`, `QuestionnaireChoice`, `QuestionnaireChoiceDescription`, `QuestionnaireChoices`, `QuestionnaireDescription`, `QuestionnaireError`, `QuestionnaireInput`, `QuestionnaireItem`, `QuestionnaireNext`, `QuestionnairePrevious`, `QuestionnaireProgress`, `QuestionnaireSkip`, `QuestionnaireSubmit`, `QuestionnaireTitle` | built on `@shadcn/react/questionnaire`; `shortcuts: "letters" \| "numbers"`; item `multiple`. | registry `questionnaire` (uses `./button.tsx`, `CheckIcon`) |
| `alert.tsx` | `Alert`, `AlertTitle`, `AlertDescription` | `variant`: `default`, `destructive` (cva); `data-slot` `alert`. Used by the cockpit error toast in `App.tsx` (DESIGN-panes 4.8). | shadcn/ui `alert`, hand-written (pro-d55) |
| `breadcrumb.tsx` | `Breadcrumb`, `BreadcrumbList`, `BreadcrumbItem`, `BreadcrumbLink` (`render` prop), `BreadcrumbPage`, `BreadcrumbSeparator`, `BreadcrumbEllipsis` | `nav` root, `ol` list, `li` item; `BreadcrumbLink` renders through `render`; `BreadcrumbPage` is the current crumb (`aria-current="page"`). Its only cockpit user, the Header, is removed by pro-d87 (DESIGN-panes 4.1); pinned by its own tests. | shadcn/ui `breadcrumb`, hand-written (pro-d60) |
| `code-block.tsx` | `CodeBlock`, `CodeBlockHeader`, `CodeBlockTitle`, `CodeBlockActions`, `CodeBlockCopyButton`, `CodeBlockContent`, `CodeBlockLine`, `CodeBlockFooter` | `CodeBlockLine` carries `data-diff` and a `variant`; imports the `Button` part. Used for tool output and edit diffs (DESIGN-chat 3.6). | shadcn/ui `code-block`, hand-written (pro-d55) |
| `dropdown-menu.tsx` | `DropdownMenu`, `DropdownMenuTrigger`, `DropdownMenuContent`, `DropdownMenuItem`, `DropdownMenuCheckboxItem`, `DropdownMenuRadioGroup`, `DropdownMenuRadioItem`, `DropdownMenuLabel`, `DropdownMenuSeparator`, `DropdownMenuShortcut`, `DropdownMenuGroup`, `DropdownMenuPortal`, `DropdownMenuSub`, `DropdownMenuSubTrigger`, `DropdownMenuSubContent` | over the `DropdownMenu` primitive of `radix-ui` (already a dependency). Used by the TierSelector's model and Tier menus (DESIGN-compose 2). | shadcn/ui `dropdown-menu`, hand-written (pro-d55) |
| `empty.tsx` | `Empty`, `EmptyHeader`, `EmptyMedia` (`variant="icon"`), `EmptyTitle`, `EmptyDescription`, `EmptyContent` | plain `div`/`p` parts. The transcript's empty state (DESIGN-chat 2.2). | shadcn/ui `empty`, hand-written (pro-d55) |
| `hover-card.tsx` | `HoverCard`, `HoverCardTrigger`, `HoverCardContent` | over the `HoverCard` primitive of `radix-ui`. The Footer context card (DESIGN-compose 3). | shadcn/ui `hover-card`, hand-written (pro-d56) |
| `input-group.tsx` | `InputGroup`, `InputGroupAddon` (`align` `block-start` \| `block-end` \| `inline-start` \| `inline-end`), `InputGroupButton`, `InputGroupText`, `InputGroupInput`, `InputGroupTextarea` | the frame of the composer (DESIGN-compose 1). `InputGroupButton` wraps the `Button` part; `InputGroupInput`/`InputGroupTextarea` wrap `Input`/`Textarea`. | shadcn/ui `input-group`, hand-written (pro-d54) |
| `message.tsx` | `MessageGroup`, `Message` (`align` `start` \| `end`), `MessageAvatar`, `MessageContent`, `MessageHeader`, `MessageFooter` | plain `div` parts with `data-slot` and `data-align`. A transcript turn is a `Message` (DESIGN-chat 2.3, 2.4). | shadcn/ui `message`, hand-written (pro-d56) |
| `progress.tsx` | `Progress` | over the `Progress` primitive of `radix-ui`; `value` 0 to 100. The Footer context bar (DESIGN-compose 3). | shadcn/ui `progress`, hand-written (pro-d56) |
| `select.tsx` | `Select`, `SelectTrigger`, `SelectContent`, `SelectItem`, `SelectValue`, `SelectGroup`, `SelectLabel`, `SelectSeparator`, `SelectScrollUpButton`, `SelectScrollDownButton` | over the `Select` primitive of `radix-ui`. Written and exported; no cockpit or chat part imports it yet (the TierSelector uses `dropdown-menu`). | shadcn/ui `select`, hand-written (pro-d55) |
| `spinner.tsx` | `Spinner` | a lucide `Loader2Icon` with `animate-spin`; the running marks of the Trace rows, tool groups and the Plan review's pending decision (DESIGN-chat 2.4.2, 3.4; DESIGN-compose 5). The Inbox async action uses `LoaderCircleIcon` directly (DESIGN-panes 4.8). | shadcn/ui `spinner`, hand-written (pro-d53) |
| `tooltip.tsx` | `TooltipProvider`, `Tooltip`, `TooltipTrigger`, `TooltipContent` | over the `Tooltip` primitive of `radix-ui`. The message Copy action (DESIGN-chat 2.3). | shadcn/ui `tooltip`, hand-written (pro-d56) |

`@shadcn/react` has only two subpath exports, `@shadcn/react/message-scroller` and `@shadcn/react/questionnaire`; `import … from "@shadcn/react"` fails. The twelve parts above (`alert`, `breadcrumb`, `code-block`, `dropdown-menu`, `empty`, `hover-card`, `input-group`, `message`, `progress`, `select`, `spinner`, `tooltip`) are new files in `web/src/ui/components/`, written by the same method (A2). `message.tsx` and `code-block.tsx` are written here; the earlier text that they were not is superseded. Parts use only Tailwind utilities and the tokens of 2b; their registry class strings are kept except where a token name does not exist here (none known; unverified until the parts are compiled).

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
### 2h. Shadcn utilities and the prompt border pulse (pro-d55, pro-d58)

**`web/src/lib/shadcn-tailwind.css`** is one file, imported by `app.css` right after `tw-animate-css` (2a item 3). It is not a package and adds no dependency. Its content, which the tests may rely on by name:

- `@theme inline` keyframes `accordion-down` and `accordion-up` (height from `--radix-accordion-content-height`).
- Custom variants: `data-open` (`[data-state="open"]` or `[data-open]` not `"false"`), `data-closed`, `data-checked`, `data-unchecked`, `data-selected` (`[data-selected="true"]`), `data-disabled`, `data-active`, `data-horizontal` (`[data-orientation="horizontal"]`), `data-vertical`.
- Utilities: `no-scrollbar`; `scroll-fade`, `scroll-fade-y`, `scroll-fade-x`, `scroll-fade-t|b|l|r|s|e` and their `-*` size forms, `scroll-fade-none`; `shimmer` with `shimmer-once`, `shimmer-reverse`, `shimmer-none`, `shimmer-color-*`, `shimmer-duration-*`, `shimmer-spread-*`, `shimmer-angle-*`. A reduced-motion rule turns `shimmer` off.
- `@property` declarations for `--scroll-fade-*`, `--shimmer-*`.
- `@theme inline` keyframes `collapsible-down` and `collapsible-up`, and utilities `animate-collapsible-down` (200 ms) and `animate-collapsible-up` (160 ms). Contract in 5.1.

The transcript's streaming marker text `Working…` uses `shimmer` (DESIGN-chat 2.4).

**`prompt-border-pulse`** is defined in `app.css` (unlayered, after the typeset rules):

```css
@keyframes prompt-border-pulse {
  0%, 100% { border-color: var(--border); box-shadow: 0 0 0 0 transparent; }
  50%      { border-color: var(--primary); box-shadow: 0 0 0 3px color-mix(in oklab, var(--primary) 28%, transparent); }
}
.prompt-border-pulse { animation: prompt-border-pulse 1.8s ease-in-out infinite; }
```

The class is added to the composer's `InputGroup` (DESIGN-compose 1.2) **only while** the prompt is busy: `busy` is `view.busy || view.pending.some(p => p.state === "sending")` (DESIGN-compose 0). The static check `view.ref.state === "working"` is dropped (pro-d58). The rule has no reduced-motion variant in the preview (unverified whether one is needed).

## 3. Cockpit shell

Files: `web/src/ui/cockpit/StatusBar.tsx`, `ContextTree.tsx`, `Shell.tsx`, `web/src/ui/App.tsx` (`Header.tsx` is deleted, 3a). All imports relative. Tests: `shell.dango.test.tsx`, `tree.dango.test.tsx` (new) and the updated Phase 2 tests (`App.test.tsx`, `AppMemo.test.tsx`, `Frame.test.tsx`, `Shell.test.tsx`, `ContextTree.test.tsx`, `app.css.test.ts`).

Layout, outside in: `div[data-shell]` is a flex column (`display: flex; flex-direction: column`), `height: 100vh`, `overflow: hidden`, no gap, background `bg-background`, text `text-foreground text-body font-sans`. The top row is one horizontal resizable group, `cpd-columns` (`min-h-0 flex-1`), with three panels: the Context tree (20%), the centre (58%) and the Inbox (22%), separated by two resizable handles. Each column opens with its `TopPaneHeader` (3i); together the three headers are the unified top row of the cockpit (pro-d87: there is no full-width Header bar, 3a). The Status Bar is the bottom row (30px). The centre panel holds a vertical resizable group, `cpd-center`, with the Plan over the chat. The page never scrolls; each column scrolls on its own.

### 3a. Header bar (removed by pro-d87)

The full-width Header bar is removed (pro-d87). Its φ home link (`[data-emblem]` text `C-P-D`), its `Location` breadcrumb and its engine pill go with it: no file renders them, `Shell` has no `header` slot, and `App` builds no crumbs. The Shell's top row is the three column `TopPaneHeader`s (3i), which form the unified top row of the cockpit. The φ mark stays in the Context tree header (3d).

Known loss (pro-d87, A3): the engine running/stopped pill and the breadcrumb navigation are not rebuilt. The breadcrumb contract of DESIGN-panes 4.1 is superseded. The search button and `⌘K` (never wired) leave with the bar. `web/src/ui/components/breadcrumb.tsx` stays as an upstream primitive (2f), with no cockpit user.

### 3b. Status Bar (30px)

Files: `web/src/ui/cockpit/StatusBar.tsx` (the bar), `web/src/ui/cockpit/LegendDialog.tsx`, `web/src/ui/cockpit/LogsDialog.tsx`, `web/src/ui/cockpit/AgentTiersDialog.tsx` (the three popups). Records: **pro-d40** and the User's answers. The bar **mirrors the TUI status bar** (`core/src/host/tmux.rs:476-520`, the cockpit's tmux status line) and its popups, one element per row of the table below, left to right. The Phase 2 `State:` segment leaves the bar: it is not one of the TUI status-bar elements. The cockpit name and the engine state are not in the bar either.

The web draws the TUI's pills as coloured text on `bg-surface-sunken`: the TUI's caps and reversed blocks are tmux drawing, not data. Buttons are shadcn/ui `Button` (`ghost`), dialogs are shadcn/ui `Dialog` (2f), all on the Dango tokens (2b). The TUI's `━` joins are not drawn; the gap between elements separates them. Icons are lucide icons of the same meaning as the TUI glyphs (named in the table); the lucide names are checked at implementation.

Status data is read by the server (`web/src/server/cockpit/`) and sent to the client; the client never reads a file or runs a command. Sources are in "Data sources" below.

| # | Element (`data-part`) | Shown when | Exact text, or accessible name | Source | TUI colour (index → token) |
|---|---|---|---|---|---|
| 1 | Reset button (`reset-layout`) | always | accessible name `Reset layout`; TUI glyph U+F0A1D; lucide icon | none | 15 → `text-fg-strong` |
| 2 | Legend button (`legend-button`) | always | `Legend`; TUI glyph U+F0A01 | none | 15 → `text-fg-strong` |
| 3 | Logs button (`logs-button`) | always | `Logs`; TUI glyph U+F09ED | none | 15 → `text-fg-strong` |
| 4 | Costs button (`costs-button`, `aria-pressed`) | always | `Costs`; TUI glyph U+EEC1 | the costs flag (below) | 15 → `text-fg-strong` |
| 5 | Settings button (`settings-button`) | always | `Settings`; TUI glyph U+F013 | none | 15 → `text-fg-strong` |
| 6 | Runner (`runner`) | the runner is known | `pi` or `durable` | `cpd config experimental` | 15 → `text-fg-strong` |
| 7 | Preview (`preview`) | tmux option `@cpd_preview` is set | `preview: <value>` | tmux `@cpd_preview` | 3 (review, yellow) → `text-status-landing` |
| 8 | fill | always | none | none | none |
| 9 | Incident (`incident`) | `cpd incident status` reports the incident on | `Incident Mode: <elapsed>`, italic | `cpd incident status` | 9 (failed, bright) → `text-status-blocked-bright` |
| 10 | Workers (`workers`) | tmux option `@cpd_workers` is set | `workers Q·W·N/M` | tmux `@cpd_workers` | 8 → `text-fg-dim` |
| 11 | Version (`version`, `data-alert`) | the version is known | `cpd: <version>`; red when the host alert is set | `cpd --version`; `run/cpd-install.json` | 8 → `text-fg-dim`; alert 9 → `text-status-blocked-bright` |

DOM (inside `footer[aria-label="Status bar"]`, which `Shell` writes and styles, 3c; the footer is 30px):

```
div[data-part="status-bar"]                                flex h-full w-full items-center gap-1 overflow-hidden px-2 text-[11.5px]
├─ Button[type="button"][data-part="reset-layout"][aria-label="Reset layout"]    ghost, h-[22px] px-2, text-fg-strong, hover:bg-row-hover; icon svg size 14, aria-hidden
├─ Button[type="button"][data-part="legend-button"][aria-label="Legend"]         same
├─ Button[type="button"][data-part="logs-button"][aria-label="Logs"]             same
├─ Button[type="button"][data-part="costs-button"][aria-label="Costs"][aria-pressed="true|false"]   same; aria-pressed="true" → bg-surface-pressed
├─ Button[type="button"][data-part="settings-button"][aria-label="Settings"]     same
├─ span[data-part="runner"]        "pi" | "durable"; only when the runner is known; font-mono text-meta text-fg-strong
├─ span[data-part="preview"]       "preview: <value>"; only when set; font-mono text-meta text-status-landing
├─ span.flex-1                     the fill
├─ span[data-part="incident"]      "Incident Mode: <elapsed>"; only when on; font-mono text-meta italic text-status-blocked-bright
├─ span[data-part="workers"]       "workers Q·W·N/M"; only when set; font-mono text-meta text-fg-dim
└─ span[data-part="version"][data-alert="true|false"]   "cpd: <version>"; only when known; font-mono text-meta text-fg-dim, text-status-blocked-bright when data-alert="true"
```

- **Runner.** The machine's runner, not the selected session's (Phase 2 read `session.ref.runner`). It is `durable` when `experimental.durable_workers` is on, else `pi`: the engine's rule that writes `@cpd_runner` (`core/src/config/file.rs:229-232`, `engine/src/watch/workers.rs:93-94`). The pill shows the value only, with no label.
- **Preview.** `preview: <value>`, where `<value>` is the text of the tmux option `@cpd_preview`, `<worktree basename> <sha>`, set by `scripts/preview-tui.sh` (`docs/ui-preview.md`).
- **Incident.** `<elapsed>` is the TUI's text (contract 6.1.1, `engine/src/watch/workers.rs:137-170`), from the incident's `started_at` to now, in whole minutes: under 60 minutes `<m>m`; under 24 hours `<h>h`, plus ` <m>m` when the minutes are not zero; else `<d>d`, plus ` <h>h` when the hours are not zero. A `started_at` in the future reads `0m`.
- **Workers.** `workers Q·W·N/M`: Q = Jobs with status `queued`; W = Ready Jobs; N = tenant Jobs of pool `workers`; M = the concurrency setting (`engine/src/watch/workers.rs:27-40`, `:81-133`). The TUI puts a glyph (U+F0798) between the four numbers; the web's separator is `·`. The engine writes the option on each tick, so a stopped engine leaves the last value on screen.
- **Version.** `cpd: <version>`, the `version` field of `cpd --version`. The text is the Phase 2 text, kept. The segment is absent while the version is unknown (the TUI draws the pill only when it is set).
- **Host alert.** The version segment turns red (`data-alert="true"`) when `$CPD_HOME/run/cpd-install.json` has `status` `skipped`; the alert is its `reason` (`core/src/host/install_state.rs:135-141`). The bar shows the colour only.

**Buttons, behaviour** (pro-d40):

- **Reset layout** resets the resizable panel sizes to the column defaults of pro-d50 and nothing else: tree 20%, centre 50%, inbox 30% of the columns; plan 40% and chat 60% of the centre (3c). The values were 20 / 58 / 22 under the TUI defaults until pro-d50, which is newer and governs (the Lead's decision). `cockpit/layout.ts` changes to match. No confirmation, no respawn, no reload, no change to the selection or to the costs flag. A collapsed panel is resized to its default, so it opens. The TUI reset only resizes panes (`tui/src/cockpit.rs`, `reset_layout`). The panel handles are local to `Shell` (3c); how the bar reaches them is the implementation's choice (a ref or a context inside `Shell`). The behaviour is fixed here.
- **Legend** opens the Legend dialog (below).
- **Logs** opens the Logs dialog (below).
- **Costs** flips the cost display of the Context and Plan rows (3d). It opens no popup. The flag is React state of `App`, off at every load, and is never stored (not in `sessionStorage`, unlike the selection). `aria-pressed` shows it.
- **Settings** opens the Agent Tiers dialog (below).
- Keyboard: the buttons are native; Enter and Space activate them; standard focus ring. No shortcut keys are built; the TUI's `?` key for the Legend is not built (see the bar's list of not built items below).

**Legend dialog** (`LegendDialog`, `data-part="legend-dialog"`). Title `Legend: What The Cockpit's Colours And Icons Mean (Any Key Closes)`. Static: no props, no data, no live values. Its sections, in this order, carry the TUI's legend content (`tui/src/widgets/legend.rs`, colours from `pi/lib/pill-spec.json` and `tui/src/palette.rs`):

1. Meanings → Colour (one source for every view)
2. Jobs: Plan Pills And Context Counters
3. Session Dots
4. Workflows (Dispatch)
5. Inbox
6. Pulse
7. PR Checks

The sections scroll through `ScrollArea` (`components/scroll-area.tsx`, pro-d69; 3c, Scrolling). Keys: `↑` `↓` and `j` `k` move one row; `PgUp` `PgDn` move a page; `End` and `G` go to the end. Any other key closes the dialog (`Esc` included). The mouse wheel scrolls.

**Logs dialog** (`LogsDialog`, `data-part="logs-dialog"`). The event log: the latest 500 Events, oldest first. One line per Event: `HH:MM:SS` (24-hour, the browser's local time), one space, then the Event's `text`, or its `kind` when `text` is empty; the line is cut to the row width with `…`. Rows of kind `error` are red (`text-status-blocked`). An empty log shows `none yet` in `text-fg-dim`. The rows scroll through a height-limited `ScrollArea` (`[&>[data-slot=scroll-area-viewport]]:max-h-[60vh]`, pro-d69; 3c, Scrolling).

- Follow: while the view is at the bottom, new Events scroll in. Scrolled up, the lines stay where they are; new Events are added to the list and shown once the view is at the bottom again. `End` or `G` go to the bottom and follow.
- Keys: `↑` `↓` `j` `k`, `PgUp` `PgDn`, `End` `G` scroll as in the Legend; `Esc` closes (Dialog).
- Data: the Context names no data source for the logs. Recorded here from the TUI reading, to be confirmed by the Lead: a snapshot from `cpd events follow --since 0 --until-idle` (one JSON line per Event, then the command exits; the server keeps the last 500), then live Events from the existing `cpd events follow --since <cursor>` (`web/src/server/cockpit/events.ts`). The snapshot's cost grows with the store (timing unverified).

**Agent Tiers dialog** (`AgentTiersDialog`, `data-part="agent-tiers-dialog"`). Title `Agent Tiers`. Fourteen rows in two sections. Labels as the TUI prints them.

| # | Section | Label | Control | Value | Read from |
|---|---|---|---|---|---|
| 0 | Models + Tiers | Preset | select, and a `Load` button | the preset name, or `(none)` | `cpd config` → `.config.presets` (names) |
| 1 | Models + Tiers | Provider | select | the provider of the O model | `cpd models show` → `.providers[].provider` |
| 2 | Models + Tiers | `O  opus` | select | the model id without its provider; `(unset)` in red | `cpd tiers` → `.tiers.O.model`; ids from `cpd models show` → `.providers[].ids` |
| 3 | Models + Tiers | `S  sonnet` | select | as row 2 | `.tiers.S.model` |
| 4 | Models + Tiers | `H  haiku` | select | as row 2 | `.tiers.H.model` |
| 5 | Models + Tiers | `G  gemini` | select | as row 2 | `.tiers.G.model` |
| 6 | Models + Tiers | auxiliar | select of Tiers | a Tier, e.g. `GX`; range `default only` | `cpd config` → `.config.context_distill_tier` |
| 7 | Models + Tiers | worker | select of Tiers | a Tier; range `<floor>..<ceiling>` or `no rung` | `cpd config` → `.config.worker_tier` |
| 8 | Models + Tiers | lead | select of Tiers | as row 7 | `cpd config` → `.config.lead_tier` |
| 9 | Models + Tiers | manager | select of Tiers | as row 7 | `cpd config` → `.config.manager_tier` |
| 10 | Experimental | durable_workers | on/off | `on` or `off` | `cpd config experimental` → `.experimental.durable_workers` |
| 11 | Experimental | write_curation | on/off | `on` or `off` | `cpd config experimental` → `.experimental.write_curation` |
| 12 | Experimental | failure_triage | on/off | `on` or `off` | `cpd config experimental` → `.experimental.failure_triage` |
| 13 | Experimental | turn_question_guard | on/off | `on` or `off` | `cpd config experimental` → `.experimental.turn_question_guard` |

A range is computed from the rungs (`.tiers.<role>.efforts`) of the chosen model, as the TUI draws it.

- **Save** runs the writes below in order, as the TUI does (`tui/src/settings/model.rs:681-733`, `tui/src/settings/mod.rs:115-181`). Each step runs only when its value changed.
  1. `cpd preset set --O <ref> --S <ref> --H <ref> --G <ref>` when a model or the Provider changed.
  2. `cpd config tiers set --auxiliar <T> --worker <T> --lead <T> --manager <T>`, with the changed Tiers.
  3. `cpd config experimental <feature> on|off`, once per changed switch.
  4. `cpd session retier --all`, only when step 1 or 2 changed something. It re-tiers every live Lead and Manager.
- Save is blocked, and the reason is shown in red: a model is unset, a re-read failed, or a Tier is not a rung of the model it runs on. Save with nothing changed closes the dialog. A failed step stops the run and shows its error; the next Save runs only the steps that did not succeed.
- **Cancel** closes the dialog and writes nothing.
- **Load** copies the preset's Provider and four model refs into rows 1 to 5, then re-reads the rungs with `cpd tiers --model O=… --model S=… --model H=… --model G=…`. It writes nothing until Save.

**Data sources** (server reads; no new cpd command):

| Read | Command or source | Field | Used by |
|---|---|---|---|
| Runner, switches | `cpd config experimental` | `.experimental.{durable_workers,write_curation,failure_triage,turn_question_guard}` | Runner, Agent Tiers rows 10 to 13 |
| Tiers, presets | `cpd config` | `.config.{context_distill_tier,worker_tier,lead_tier,manager_tier,presets}` | Agent Tiers rows 0, 6 to 9 |
| Models, rungs | `cpd tiers` | `.tiers.{O,S,H,G}.{model,efforts}` | Agent Tiers rows 2 to 5 and ranges |
| Provider lists | `cpd models show` | `.providers[].{provider,ids}` | Agent Tiers rows 1 and 2 to 5 |
| Incident | `cpd incident status` | `on`, `incident.started_at` | Incident |
| Version | `cpd --version` | `version` | Version |
| Workers | tmux option `@cpd_workers` (`show -gqv`) | text | Workers |
| Preview | tmux option `@cpd_preview` (`show -gqv`) | text | Preview |
| Host alert | `$CPD_HOME/run/cpd-install.json` | `status`, `reason` | Version colour |
| Cost | session JSONL files, and three cpd calls (below) | `usage.cost.total` | Costs toggle (3d) |
| Events | `cpd events follow` (above) | `at`, `kind`, `text` | Logs |

The tmux reads go to the cockpit's tmux server: socket `tmux.socket` from `config.toml` (default `cpd`). Refresh cadence: not set by the Context (open, for the Lead).

**Cost** (pro-d40: "cost by parsing session JSONL, usage.cost.total, summed, no dedup, cached by (mtime, size), 3 cpd calls per refresh"; the rules below are the TUI's, the contract for this value):

- **Session file.** Its cost is the sum of `usage.cost.total` over every counted entry: a `type` `"message"` entry whose `message.role` is `assistant`, or an entry of type `usage`, `compaction` or `branch_summary` with a top-level `usage` object. No dedup: every counted entry adds. In a fork (line 1 is a `session` header with a string `parentSession`), an entry whose timestamp is before the header's timestamp counts for nothing. A malformed line (not JSON, no string `type` or RFC 3339 `timestamp`, or a counted entry without `usage.cost.total`) makes the whole file malformed: the last good value is kept, never a partial sum. The value is cached per file by `(mtime, size)` and parsed again only when either changes (`core/src/metrics/mod.rs:147-240`, `tui/src/read/cost.rs:13-62`).
- **Attempt.** Its stored cost when it has one (the `data.cost_usd` of its `attempt-ended` Event), else the cost of its Worker Session's file. Never both (`tui/src/read/cost.rs:66-80`).
- **Plan.** The sum of its Lead Sessions (archived included) and its Attempts. **Context.** The sum of its Plans (archived included) and its Manager Sessions (archived included). A total with no known part is absent; otherwise it is the sum of the known parts (`tui/src/read/tree.rs:311-416`, `tui/src/read/cost.rs:82-88`).
- **The three cpd calls per refresh:** `cpd context list` (Plan ids); `cpd session list --archived` (`role`, `plan`, `context`, `attempt`, `file`); `cpd events list --kind attempt-ended --since <cursor> --limit 5000` (stored Attempt costs; repeated while a page is full). Today's read model calls `session list` without `--archived`; the cost needs it.
- **Format** (`core/src/metrics/mod.rs:244-253`): `NaN` or below `0.0005` → `$0.000`; below `9.9995` → `$` and 3 decimals (`$0.120`); below `99.995` → 2 decimals; below `999.95` → 1 decimal; below `9995` → thousands, 2 decimals and `k` (`$1.23k`); below `99950` → thousands, 1 decimal and `k`; otherwise thousands, 0 decimals and `k`. Exact ties round to even, as Rust's formatting does (JavaScript's `toFixed` does not).

**Not wired** (omitted, never faked): the runner choice (the `pi | durable` segmented control; the TUI runner pill flips `durable_workers`, and the Context does not decide that for the web), clicks on the runner, incident and version pills (the TUI's `incident` and `host` actions; not decided), the `?` key binding (TUI only), the version `HoverCard`, host `cpu / ram / disk` (not in the TUI bar), the TUI's short workers form and its drop of segments on a narrow client (the bar clips with `overflow-hidden`).

**Built by pro-d40** (absent in Phase 2): the Reset layout, Legend, Logs, Costs and Settings buttons (the Settings button is the Agent Tiers dialog, formerly "Tiers"); the Incident pill; the Workers pill (the Q·W·N/M meter); the Preview pill; the Runner pill as the machine's runner; the Version pill's host-alert colour.

**Changed from Phase 2:** the `State:` segment is removed; `Runner: <x>` is `<x>`, and is absent when unknown (was `Runner: none`); `cpd: unknown` is absent when the version is unknown; the reset and `$` buttons are replaced by the five buttons; `App` passes the status data and the handlers (3e). The Phase 2 tests this changes are in 3f.

### 3c. Shell and the resizable split

File `web/src/ui/cockpit/Shell.tsx`. The resizable parts are `web/src/ui/components/resizable.tsx` (2f).

```ts
export function Shell(props: {
  tree: ReactNode; plan: ReactNode; chat: ReactNode; inbox: ReactNode;   // KEPT
  status?: ReactNode;                                                     // KEPT
}): ReactElement;
```

**Every resizable split in the Shell is a shadcn/ui resizable group** (`ResizablePanelGroup`, `ResizablePanel`, `ResizableHandle` with `withHandle`, `usePanelRef`), over `react-resizable-panels` 4.15.3 (pro-d41). There are two groups. The outer horizontal group `cpd-columns` holds the three columns (Context tree, centre, Inbox). The inner vertical group `cpd-center` sits in the centre column and holds the Plan over the chat. No other split is drawn with a bespoke Separator.

DOM. Children of `div[data-shell]`, in order: the columns group (the top row), `footer[aria-label="Status bar"]` (only when `status !== undefined`). The `header` prop and its slot are removed (pro-d87, A1). The three columns are **not** direct children of `div[data-shell]`: they are the panels of the columns group.

```
div[data-shell]                                                   flex column (2a)
├─ ResizablePanelGroup id="cpd-columns" data-part="columns-group" orientation="horizontal"   min-h-0 flex-1   (top row; renders a div)
│   ├─ ResizablePanel id="tree" data-part="tree-panel"            defaultSize 20% · minSize 12% · maxSize 40% · collapsible · collapsedSize 0%
│   │   └─ aside[aria-label="Context tree column"]   h-full overflow-hidden      {tree}   (Sidebar, 3d)
│   ├─ ResizableHandle id="split-left" data-part="split-left" withHandle aria-label="Resize context tree and center"
│   ├─ ResizablePanel id="center" data-part="center-panel"        defaultSize 50% · minSize 30%   (not collapsible)
│   │   └─ div[data-column="center"][data-part="center-frame"][data-plan-state="open|collapsed|zoomed"]   relative flex h-full min-h-0 flex-col
│   │       └─ ResizablePanelGroup id="cpd-center" data-part="center-group" orientation="vertical"   h-full min-h-0 flex-1   (renders a div)
│   │           ├─ ResizablePanel id="plan" data-part="plan-panel" panelRef={planRef}   defaultSize 40% · minSize 20% · maxSize 80% · collapsible · collapsedSize 44px
│   │           │   └─ section[aria-label="Plan pane"][data-state="open|collapsed|zoomed"]   flex h-full min-h-0 flex-col overflow-hidden   provides `PlanCollapseContext` around {plan} (PlanView or ManagerView, 3e; 3c)   not inert while collapsed (pro-d88)
│   │           ├─ ResizableHandle id="split" data-part="split-handle" withHandle aria-label="Resize plan view and chat"
│   │           └─ ResizablePanel id="chat" data-part="chat-panel" panelRef={chatRef}   defaultSize 60% · minSize 20% · collapsible · collapsedSize 0%
│   │               └─ main[aria-label="Chat"][data-state="open|zoomed"]   h-full min-h-0   {chat}   inert while zoomed
│   ├─ ResizableHandle id="split-right" data-part="split-right" withHandle aria-label="Resize center and inbox"
│   └─ ResizablePanel id="inbox" data-part="inbox-panel"          defaultSize 30% · minSize 14% · maxSize 45% · collapsible · collapsedSize 0%
│       └─ aside[aria-label="Inbox column"]   h-full overflow-hidden      {inbox}   (DESIGN-panes 2)
└─ footer[aria-label="Status bar"]   flex items-center border-t border-border bg-surface-sunken   (bottom; shrink-0)
```

`div[data-column="center"]` (the centre panel's child, a plain `div`, KEPT hook) has exactly **1** child, the centre group. The Phase 2 descendant selectors `div[data-column="center"] section[aria-label="Plan pane"] #p` and `div[data-column="center"] main[aria-label="Chat"] #c` still match. The plan collapse control is not in the centre frame: `TopPaneHeader` (3i) renders it in the Plan header from `PlanCollapseContext` (3c, pro-d88).

`react-resizable-panels` 4.15.3, through the wrappers of 2f, exact props:

| Element | Props |
|---|---|
| `ResizablePanelGroup` outer (`cpd-columns`) | `orientation="horizontal"`, `id="cpd-columns"`, no `defaultLayout`, no storage (sizes are not persisted) |
| tree `ResizablePanel` | `id="tree"`, `defaultSize="20%"`, `minSize="12%"`, `maxSize="40%"`, `collapsible`, `collapsedSize="0%"` |
| `ResizableHandle` left | `id="split-left"`, `withHandle`, `aria-label="Resize context tree and center"`, `data-part="split-left"`, `onDoubleClick` → `treeRef.current?.resize("20%")` |
| centre `ResizablePanel` | `id="center"`, `defaultSize="50%"`, `minSize="30%"`, not collapsible |
| `ResizableHandle` right | `id="split-right"`, `withHandle`, `aria-label="Resize center and inbox"`, `data-part="split-right"`, `onDoubleClick` → `inboxRef.current?.resize("30%")` |
| inbox `ResizablePanel` | `id="inbox"`, `panelRef={inboxRef}`, `defaultSize="30%"`, `minSize="14%"`, `maxSize="45%"`, `collapsible`, `collapsedSize="0%"` |
| `ResizablePanelGroup` inner (`cpd-center`) | `orientation="vertical"`, `id="cpd-center"`, no `defaultLayout`, no storage |
| plan `ResizablePanel` | `id="plan"`, `panelRef={planRef}`, `defaultSize="40%"`, `minSize="20%"`, `maxSize="80%"`, `collapsible`, `collapsedSize="44px"` (the title row stays visible), `onResize={sync}` |
| chat `ResizablePanel` | `id="chat"`, `panelRef={chatRef}`, `defaultSize="60%"`, `minSize="20%"`, `collapsible`, `collapsedSize="0%"`, `onResize={sync}` |
| `ResizableHandle` inner | `id="split"`, `withHandle`, `aria-label="Resize plan view and chat"`, `data-part="split-handle"`. The part's own classes and grip replace the Phase 2 bar (`h-[9px]`, `border-t`, `span`). |

`planRef`, `chatRef` come from `usePanelRef()`. `sync()` recomputes `mode` from `planRef.current?.isCollapsed()` and `chatRef.current?.isCollapsed()`: both false → `open`; plan collapsed → `collapsed`; chat collapsed → `zoomed`. Both collapsed cannot happen (the sizes sum to 100%).

Handle semantics come from the library, not written by cpd: `role="separator"`, focusable (`tabindex="0"`), `aria-orientation` and `aria-value*` (unverified). The library's keyboard handler contains ArrowUp, ArrowDown, ArrowLeft, ArrowRight, Home, End, Enter and F6 (case labels read in `dist/react-resizable-panels.js`; the exact step and effect of each is unverified). Tests may pin only: the handle exists with `role="separator"` and its `aria-label`, and is focusable. The two outer handles override the library's double-click (see the table above); the inner handle keeps the library default (unverified).

**Plan collapse control (pro-d88).** The control lives in the Plan pane's header, not in a floating overlay. Shell owns the state and exports the context once; `TopPaneHeader` (3i) reads it:

```ts
// web/src/ui/Shell.tsx
export interface PlanCollapseControl {
  readonly collapsed: boolean;   // the plan Panel is collapsed (`data-state="collapsed"`)
  readonly toggle: () => void;   // expands when collapsed, else collapses, on the plan Panel ref
}
export const PlanCollapseContext = createContext<PlanCollapseControl | null>(null);
```

Shell provides `{collapsed, toggle}` (memoised) around `{props.plan}`, inside `section[aria-label="Plan pane"]`. `TopPaneHeader` renders the group and the button after `actions`: `div[data-part="plan-controls"][role="group"][aria-label="Plan pane controls"]` holding `Button variant="ghost" size="icon-sm"` with `className="size-[26px] rounded-[5px] text-fg-strong"`, icon lucide at size 14 (`ChevronUpIcon`, `ChevronDownIcon`; existence in lucide-react 1.65 unverified, an equal-meaning icon may replace one). Exact behaviour:

| Plan section `data-state` | Collapse button |
|---|---|
| `open` | label `Collapse plan view`, `ChevronUpIcon`, enabled; click → `toggle()` (collapses) |
| `collapsed` | label `Expand plan view`, `ChevronDownIcon`, enabled; click → `toggle()` (expands) |
| `zoomed` | label `Collapse plan view`, `ChevronUpIcon`, enabled; click → `toggle()`. The old disabled state is dropped with the floating controls. The result while the chat is collapsed is the library's (unverified). |

- The control is in the header, outside `main[aria-label="Chat"]`, so it stays operable when the chat is zoomed.
- No header, no control: with no Context or Plan loaded the Plan pane shows the flowchart (DESIGN-panes 1b), which has no header. Accepted (pro-d88).
- The Inbox and the Context tree pass `hasOverlayControls` false or sit outside the provider.
- After a click the state hooks update through `onResize → sync()`: `data-plan-state` on the centre element, `data-state` on `section[aria-label="Plan pane"]` (`open` | `collapsed` | `zoomed`) and on `main[aria-label="Chat"]` (`open` | `zoomed`; `main` never reads `collapsed`).
- `Enter` / `Space` on a focused button activates it (native). No global shortcut.
- The plan `section` is never `inert` (pro-d88: its header holds the control). When `collapsed`, the body below the header is hidden by CSS (`app.css`; DESIGN-panes 1j, 3i). When `zoomed`, `main[aria-label="Chat"]` gets `inert` (its Panel is 0% high).
- Dragging an inner handle below the plan `minSize` collapses the plan (library behaviour for a `collapsible` panel), above it expands it again; the same for the chat. Both use the same `sync()`.
- jsdom has no layout: a test mocks `react-resizable-panels` (external dependency) and asserts the props tables above and that the button calls `collapse()` / `expand()` on the Plan ref through `toggle()`; it does not assert pixel sizes.

**Columns (pro-d50).** The defaults are the Context tree at 20% of the row width (12% to 40%), the centre at 50% (min 30%), the Inbox at 30% (14% to 45%). The tree and inbox panels collapse when dragged below their minimum (library behaviour, `collapsedSize="0%"`). The handles resize; sizes are not persisted.

**Double-click resets (pro-d50).** A double-click on `split-left` resets the tree panel to 20% (`treeRef.current?.resize("20%")`); a double-click on `split-right` resets the Inbox panel to 30% (`inboxRef.current?.resize("30%")`). Neither resets the centre. The inner `split` handle has no override here (library default; unverified). Tests mock `react-resizable-panels` and assert the call, not pixels. The asides fill their panels (`[data-shell] aside`, 2a). Tree, plan panel, chat and inbox scroll independently: the asides and the plan panel clip (`overflow: hidden`, pro-d69 A9, A10) and their content scrolls inside them through `ScrollArea` (see Scrolling below); the transcript scrolls through the `ScrollArea` Viewport (DESIGN-chat 2.2).

**Scrolling (pro-d69, pro-d72).** Every scrollable area uses the shadcn `ScrollArea` and `ScrollBar` from `web/src/ui/components/scroll-area.tsx` (base variant over `@base-ui/react/scroll-area`, pro-d72), never a raw `overflow-y-auto` or `overflow-x-auto`. `ScrollArea` takes the Base UI Root props, has no orientation prop and always renders a vertical and a horizontal `ScrollBar` (pro-d72). A vertical-only pane (the Plan pane, the tree sections, the Inbox list) constrains its content width with the class pair `[&>[data-slot=scroll-area-viewport]>[data-slot=scroll-area-content]]:w-full [&>[data-slot=scroll-area-viewport]>[data-slot=scroll-area-content]]:!min-w-0`, so its content never overflows sideways. A horizontal scroller (the DAG, DESIGN-panes 1e) keeps its bar. A height-limited scroller puts its limit on the viewport: `className="[&>[data-slot=scroll-area-viewport]]:max-h-64"`. The Documentation dialog (`DocsDialog.tsx`) is the model. The asides and the plan panel clip and never scroll themselves. The scrolling areas are:
- Plan pane: the body under the header is a `ScrollArea` (`data-part="plan-scroll"`) in PlanView and ManagerView (DESIGN-panes 1, 1j), vertical only; the header stays fixed. Collapsed, the body and scrollbars are hidden (DESIGN-panes 1j).
- Inbox: the list (DESIGN-panes 2.2, vertical only) and the curate dialog's checklist (DESIGN-panes 2.6).
- Logs: the rows (3g), with the 60vh limit on the viewport.
- Legend: the sections (3g).
- WorkflowCard: `dag-body` (DESIGN-panes 1e) is a `ScrollArea` with a horizontal `ScrollBar`.
- Context tree: the Sidebar section bodies (3d), vertical only.
- Chat transcript: the scroll element is the `ScrollArea` Viewport, which renders the MessageScroller viewport (`render`, DESIGN-chat 2.2, pro-d72); pinning and auto-scroll are unchanged (DESIGN-chat 2.8.1).
- AskCard: the card is a `ScrollArea` (DESIGN-compose 4.2); PlanReviewCard: its body is a `ScrollArea` (DESIGN-compose 5.3) (pro-d72).
- Dialogs: `DialogContent` (`components/dialog.tsx`) wraps its children in a `ScrollArea`; the `max-h-[85vh]` limit moves to that ScrollArea's Viewport and the Content no longer has `overflow-y-auto` (pro-d72). Every dialog scrolls through it.

Raw scrollers that stay (pro-d72): `select.tsx` and `dropdown-menu.tsx` (menu primitives), `code-block.tsx`, `attachment.tsx` and the sidebar (primitive internals). Their scroll belongs to the primitive, so they are not converted to `ScrollArea`.

Built: the double-click resets above, and `Reset layout` (3g, values from pro-d50). Not built: persisted panel sizes, narrow-screen drawers.

### 3d. Context Tree (sidebar, 20% panel)

File `web/src/ui/cockpit/ContextTree.tsx`. pro-d48..pro-d61 name no change to the tree: the Sidebar below and its menus (3g) stand as written, and the approved preview's tree rework is not part of this contract (merge rule: main wins where no decision names the behaviour). The tree's only pro-d50 effect is its aside's `overflow-hidden` (3c). The tree is built on the shadcn/ui sidebar parts (`web/src/ui/components/sidebar.tsx`, 2f): `Sidebar collapsible="none"` with `SidebarContent` as its only child. There is no `SidebarHeader` and no `SidebarFooter`, and no `SidebarProvider` (the parts hold no state) (pro-d42). Props **KEPT**, unchanged:

```ts
export function ContextTree(props: {
  tree: ContextTreeModel | null;
  selectedSessionId: string | null;
  selectedPlanId: string | null;
  onSelectSession(sessionId: string): void;
  onSelectPlan(planId: string): void;
  /** New context. The "+" button is rendered only when passed (Not built). */
  onNewContext?: () => void;
}): ReactElement;
```

Data: `ContextTreeModel { contexts: ContextNode[]; cursor }`, `ContextNode { id, title, archived, records, openQuestions, sessions: SessionNode[], plans: PlanNode[] }`, `PlanNode { id, contextId, title, phase, archived, counts: PlanCounts, leadSessionId, sessions }`, `SessionNode { ref: SessionRef, contextId, planId, jobKey, archived, dot: SessionDot }` (`web/src/protocol/cockpit.ts`). `PlanCounts` gains `merging?: number` (pro-d43, 3d Counters).

**Section layout.** `SidebarContent` is a flex column (`h-full min-h-0 gap-0 overflow-hidden`, no outer scroll). Contexts and Archive are each a block `data-part="tree-section"` (`data-section` `contexts|archive`) and a direct flex child of `SidebarContent`. **The Contexts section is not collapsible** (pro-d84): it has no `data-state` and always takes the free height (`min-h-0 flex-1`). Its header is the shared `TopPaneHeader` (3i), `data-part="tree-header"` with `hasOverlayControls` false, not sticky: it sits outside its body's `ScrollArea` and never scrolls; its body is a `ScrollArea` > `SidebarGroupContent`. **Archive** stays a collapsible section (`data-state` `open|closed`): an open section is `min-h-0 flex-1`, so open sections share the height equally, and a closed one is `shrink-0`, just its label row. Shadcn `Separator`s (`shrink-0`) are direct children of `SidebarContent` between sections that exist (Archive renders only when something is archived). Inside Archive: `Collapsible` > `SidebarGroup` > a fixed `shrink-0` label row (the `SidebarGroupLabel` trigger) and `CollapsibleContent` > `ScrollArea` > `SidebarGroupContent`. The label is outside the `ScrollArea` and never scrolls; only the section body scrolls (pro-d47). The Contexts section is the one list of live Contexts: standing Contexts (`cpd`, `host`, `codereview`; pro-d79) are ordinary rows in it, sorted like the others by the Manager's dot tier, then newest first (`compareOrderKey`, `state/archiveNav.ts`), with no fixed place.

**Source of the rows.** The tree follows the cpd TUI tree (`docs/commands.md` section 13.2, read with `git show origin/main:docs/commands.md`; evidence `sc-tree-tui.md`): a Context is one row with its Manager's dot, a Plan is one row with its Lead's dot, and a fork is a row of its own. The web keeps its own markup (the Sidebar, below); it does not draw the TUI's fold glyphs, closing rows, Standing or Archive docks or cursor (Not built). Costs are drawn only while the Status Bar's costs toggle is on (3b, Cost below).

**Read model rules** (the read model keeps these; the component may rely on them):
- At most one live Manager per Context: role `manager`, `archived_at` null. When several match, the one with `file !== null` wins, then the newest.
- At most one Lead per Plan: `plan.lead_session`; else the newest live `plan_lead` session.
- The live lists drop sessions with role `worker` and archived sessions.
- A live non-lead Lead (a live session of role `lead` that is not its Plan's Lead) is listed once, as a fork under its Context (below).
- `PlanCounts.merging` is parsed by the read model (pro-d43). An absent count reads as 0.
- The Archived group is `details[data-archived]`, closed by default, with summary `Archived (n)`, where n is the archived Contexts in the group plus the archived Plans they contain. **Assumption:** the addendum's "Contexts + Plans in it" is read as this sum; a per-Context archived-Plans group keeps `Archived (n)` with n = its archived Plans.

DOM:

```
div[aria-label="Context tree"]                                    Sidebar collapsible="none"   bg-sidebar text-sidebar-foreground flex h-full w-full flex-col min-h-full bg-background
└─ SidebarContent                                                 the only child
   ├─ div[data-part="tree-section"][data-section="contexts"]   not collapsible (3d, Section layout)
   │   ├─ TopPaneHeader[data-part="tree-header"][hasOverlayControls=false]   the shared header (3i), h-11
   │   │   ├─ span[data-part="phi"][aria-hidden="true"]  "φ"   font-display text-[20px] font-bold text-fg-strong
   │   │   ├─ span.sr-only  "Contexts"
   │   │   └─ actions: Button[type="button"][data-action="new-context"][aria-label="New context"]   ghost, icon-sm, 26px; PlusIcon 14; only when onNewContext
   │   └─ ScrollArea > SidebarGroupContent
   │       ├─ p[data-part="loading"]  "Loading contexts…"           (tree === null)   px-2 text-fg-dim
   │       ├─ p[data-part="empty"]  "No contexts"                  (no live and no archived contexts)   px-2 text-fg-dim
   │       └─ SidebarMenu[role="tree"]                               KEPT   (the live contexts)
   │           └─ SidebarMenuItem[role="treeitem"][data-context="<id>"]     KEPT   one per live Context
   │               ├─ SidebarMenuButton[type="button"][data-select="context"][aria-current="true"?]   the Manager row
   │               │     ├─ DOT (the Manager)                      see Dot below
   │               │     └─ span[data-part="context-id"][title]  {context.id}   font-mono text-key font-semibold text-fg-strong truncate
   │               │         title = the context title, only when it differs from the id
   │               ├─ CollapsibleTrigger asChild → SidebarMenuAction[type="button"][aria-label="Toggle <id>"]   the ">" chevron (ChevronRightIcon); rendered only when the Context has Plans or forks
   │               └─ CollapsibleContent → SidebarMenuSub[role="group"]   the Plans and Forks of the Context; no border-l; pl-4 (1rem) more than the Manager row
   │                   ├─ SidebarMenuSubItem[role="treeitem"][data-plan="<id>"][data-part="plan-row"]   one per live Plan, model order
   │                   │     └─ SidebarMenuSubButton[type="button"][data-select="plan"][aria-current="true"?]
   │                   │           ├─ DOT (the Lead)               see Dot below
   │                   │           ├─ span[data-part="plan-id"][title={plan.title}]  {plan.id}   font-mono text-key truncate
   │                   │           └─ span[data-part="counters"]    Counters of this Plan (see Counters)
   │                   └─ SidebarMenuSubItem[data-part="session-row"]    one per fork (see Forks)
   │                         └─ SidebarMenuSubButton[type="button"][data-session="<ref.id>"][aria-current="true"?]
   │                               ├─ DOT (the fork's own dot)
   │                               └─ span  {label}   font-mono text-key truncate
   └─ details[data-archived]  (only when archived contexts exist)   KEPT   closed by default
       ├─ summary  "Archived (<n>)"                                  n as in the read model rules above
       └─ ul[role="tree"]  archived contexts, same item structure
```

The Manager row carries **no counters**: a Context row shows its Manager's dot, its id and the chevron, nothing else (pro-d43). Only Plan rows show counts.

**Dot** (`span[role="img"][data-dot="<state>"][data-attention="<urgency>|none"][data-tone][data-glyph][aria-label]`, className `glyphClass[g]`, KEPT hooks):
- `data-dot` = `dot.state`; `data-attention` = `attention ?? "none"`; `aria-label` = `state` when `attention === null`, else `` `${state}, needs you (${attention})` `` (KEPT; the Phase 2 test pins `aria-label="waiting"`).
- `data-tone` and `data-glyph` from `sessionDisplay[dotDisplay(dot)]`, as before.
- Source: a Context row shows its Manager's `dot`; a Plan row shows its Lead's `dot`; a fork row shows its own `dot`. A missing Manager or Lead draws `{ state: "stopped", attention: null }` (**assumption:** the TUI draws a Stopped dot with no live Manager, `sc-tree-tui.md` section 4; I infer the same for a missing Lead).
- Exactly one `[role="img"][data-dot]` per row: one on each Context row, Plan row and fork row, none elsewhere.
- **Export (pro-d76).** `Dot` is exported from this module so that `ManagerView` (DESIGN-panes 1j) draws the same dot. That view sorts its Plan rows with `compareOrderKey` from `state/archiveNav.ts`, the key this module's sorts use.

**Counters** (`span[data-part="counters"]` on a Plan row only, `ml-auto shrink-0 font-mono text-meta`) (pro-d43):
- Numbers only, no words. Each non-zero bucket is `span[data-count="<key>"]`, in the order `draft, queued, running, merging, failed, merged`. Colour per bucket: `draft` `text-status-decision`; `queued` `text-muted-foreground`; `running` `text-status-active`; `merging` `text-status-landing`; `failed` `text-status-blocked`; `merged` `text-status-done`.
- Buckets are separated by **one space** (a single text node, never ` · `).
- `cancelled` is not shown. Zero and absent buckets are omitted. With every bucket zero the span is not rendered.
- Example: queued 2, running 1, failed 1, merged 13 renders `2 1 1 13`.
- **Resolution (pro-d43):** the TUI's `active` counted `merging`. The web gives `merging` its own bucket, so the Phase 2 mapping (`planned` = draft + queued, `active` = running, `done` = merged) and the keys `planned`, `active` and `done` no longer apply.

**Cost** (pro-d40; the value rules are in 3b, "Cost"). Shown only while the Status Bar's costs toggle is on, as `span[data-part="row-cost"]`, `ml-1 shrink-0 font-mono text-meta text-fg-dim`, with the value of `format_cost` (`$0.120`, `$1.23k`):
- Context row: after `span[data-part="context-id"]`, separated by one space.
- Plan row: after `span[data-part="plan-id"]`, separated by one space.
- Absent when the value is unknown (never `$0.00`). Fork rows never show a cost, and the Manager (no row of its own, 3d above) and Workers have none. Archived rows show their cost, as in the TUI. When a row is narrow the label is cut before the cost.

**Order.** Live Contexts first, then `details[data-archived]`. Inside a live Context: the live Plans in model order, then the archived Plans (`details`), then the fork rows: the Context's own forks first, then the forks of each Plan in Plan order, archived Plans included.

**Forks.** Only forks have their own rows. A fork is a live session that is (a) not the Manager and has no Plan, labelled `ref.key`; or (b) not the Lead of its Plan and has `jobKey === null`, labelled `<plan id>·<first 4 characters of ref.id>`; or (c) a live non-lead Lead (see the read model rules). Each session is listed once. Workers (`jobKey !== null`), archived sessions and the Manager are never rows.

**Selection.** `aria-current="true"` marks the selected row (KEPT): the Plan button when `selectedPlanId === plan.id`; a Context button or a fork button when `selectedSessionId` is the Manager's id or the fork's `ref.id`. The selected row takes `bg-sidebar-accent text-fg-strong` (`SidebarMenuButton` and `SidebarMenuSubButton` with `isActive`).

**Clicks.** Clicking the Manager, a Plan or a Fork opens its chat (pro-d42).
- Manager row (the Context button): `onSelectSession(manager.ref.id)`. Without a Manager the button is `disabled` and calls nothing (Phase 2).
- Chevron (`SidebarMenuAction`): toggles the Plans and Forks of its Context. It does not select. **Assumption:** the Context is expanded by default, so the rows that Phase 2 always showed stay visible.
- Plan button: `onSelectPlan(plan.id)`, then `onSelectSession(lead.ref.id)` where `lead` is the session of `plan.sessions` whose `ref.id === plan.leadSessionId`. **Assumption:** with no such Lead only `onSelectPlan(plan.id)` is called.
- Fork button: `onSelectSession(ref.id)`.
- Keyboard: every row and the chevron are native `button`s: Tab order is document order, Enter and Space activate. The `role="tree"` has no arrow-key navigation (Not built).
- Scrolling: the column `aside` clips (`overflow: hidden`, 3c); the section bodies scroll through `ScrollArea` (pro-d69, Section layout); the `tree-header` is not sticky: it sits outside its body's `ScrollArea` (TopPaneHeader, 3i).

Not built: the Active section, `▾ N more` overflow rows (all rows render), aggregate glyphs on Context and Plan rows (replaced by the Manager's and Lead's dots), session wake and spinner, `New context` (button rendered only with `onNewContext`; `App` does not pass it).

### 3e. `App.tsx` wiring

File `web/src/ui/App.tsx`. `AppProps` (`transport`, `scheduler`, `reader`, `newId?`), the store and connection creation, the `memo()` wrapping of `ContextTree`, `PlanView`, `Inbox`, the `live = cockpit.tree !== null` gating and `useCallback` handlers are **KEPT** (`AppMemo.test`: streaming deltas must not re-render those three panes).

```tsx
<Shell
  status={<StatusBar status={cockpit.status} costsOn={costsOn} onToggleCosts={toggleCosts} />}
  tree={…KEPT…} plan={…KEPT…} chat={…KEPT…} inbox={…KEPT…} />
```

- Chat slot when no session is selected: `Empty[data-empty]` (the `Empty` part, 2f `empty.tsx`), text exactly "Select a session" (KEPT copy and hook), restyled `grid h-full place-items-center text-muted-foreground`.
- **Plan pane (pro-d70, pro-d75).** `visiblePlan !== null` renders `ErrorBoundary name="PlanView"` around `MemoPlan`, with `postableWorkflows` when the Inbox is loaded (DESIGN-panes 1f). Otherwise `ErrorBoundary name="ManagerView"` renders `MemoManager` (DESIGN-panes 1j) with `context` = the Context whose own `sessions`, or whose Plans' `sessions`, hold the selected session (`session?.id ?? selectedSessionId`), or `null` (then `CpdFlowCanvas`), and with `onSelectPlan` and `onSelectSession` as `App`'s handlers. `PlanView` gets `onSelectManager` (pro-d84, A5): it finds the Context's live Manager (its `manager` session that is not archived), calls `onSelectPlan(null)`, then `onSelectSession(manager.ref.id)` when there is one. The manager key of the title row (DESIGN-panes 1) calls it.
- The Plan, Inbox and Chat contents are restyled by `DESIGN-panes.md`, `DESIGN-chat.md` and `DESIGN-compose.md`; their props are not changed by this file.
- `cockpit.status` is the status snapshot the server sends (3b, Data sources). The Status Bar's buttons, dialogs and the reset of 3c are wired inside the bar and `Shell`; `App` passes the snapshot and the costs flag only. `costsOn` is `App` state (`useState(false)`), never stored (3b).
- Not built in `App`: `onOpenSearch`, `onNewContext`. `App` builds no breadcrumb crumbs and passes no `header` (pro-d87, A1); the engine pill and the breadcrumb navigation are a known loss, not rebuilt (3a).

### 3f. Phase 2 tests this section changes (owner `ui-impl-shell`)

| Test | Change |
|---|---|
| `Frame.test.tsx` 1 | none (`[data-emblem]` text `C-P-D`, now `sr-only`) |
| `Frame.test.tsx` 2, 3, 6 | **changed** by 3b (pro-d40): the tests that read `[data-part="runner"]`, `[data-part="state"]` and `[data-part="version"]` change. The `State:` segment is removed, so its assertions go; `Runner: <x>` becomes `<x>` and is absent when unknown; `cpd: unknown` becomes absent. The `state` prop of `StatusBar` is removed. |
| `Frame.test.tsx` 4, 5 | **changed** by 3c: the direct children of `[data-shell]` are the columns group (no `aria-label`) and `footer` (pro-d87: no `header`); the three column labels are read from the group's panels (`aside`, `div[data-column="center"]`, `aside`). `[data-column="center"]` still has 2 children: the centre group and the controls. |
| `Shell.test.tsx` | none (descendant selectors still match) |
| `ContextTree.test.tsx` 1 to 4 | **changed** by 3d: the Manager row is the Context button (its dot and selection); a Context row has no `data-part="counters"`; a Plan row's counts are `data-count` spans in the keys `draft, queued, running, merging, failed, merged`, separated by one space (`planned`, `active`, `done` and ` · ` are gone); `[role="img"][data-dot]` is one per row (2 for the fixture) |
| `tree.dango.test.tsx` 1 to 3 | **changed** by 3d: the counts text of a Plan row is its non-zero buckets separated by one space; the selected Manager is the Context button; the Context row has no counts; the `Archived (1)` summary and `button[data-action="new-context"]` are KEPT |
| `App.test.tsx` (line 108) | **changed** by 3d: the tree root is `div[aria-label="Context tree"]` (the Sidebar), not `nav` |
| `app.css.test.ts` 1, 2 | none |
| `app.css.test.ts` 3 | **changed** by 2a and 3c: the selector regex for the tree and inbox asides matches `[data-shell] aside` (not `[data-shell] > aside`); the `declsMatching(/plan/i)` assertion drops `max-height` |
| `AppMemo.test.tsx` | none |

### 3g. Tree context menus, live update and hot reload (pro-d46)

Record: pro-d46 (User approved live, 2026-11-04 17:38 EST). It supersedes "Not built: ... row context menus" in 3d and the A6 paragraph, for the wiring named here. The TUI is the contract: `tui/src/tree/menu.rs` (items and labels per kind), `tui/src/tree/actions.rs` (argv), `tui/src/tree/ask.rs` (prompts), `tui/src/tree/slug.rs` (ids) and `docs/commands.md` sections 3.8 to 3.15, 4.1 to 4.14 and 10.3 to 10.11 (argv).

**Context menus.** Every tree row is a shadcn `ContextMenu` (2f), opened by a right click (`contextmenu`). The trigger is the row's button (`data-select` or `data-session`). The content is `ContextMenuContent[data-menu-kind="<kind>"]`, with kind `context`, `permanent`, `plan`, `fork` or `archived`. Items are `ContextMenuItem` (`role="menuitem"`), in the TUI's order, with the TUI's labels (`…` where the TUI has it). The menu closes when an item is picked.

The tables below give, per row kind, each item, its web action kind (the `CockpitAction` kind of `web/src/protocol/cockpit.ts`), the cpd argv that `actionArgs` (`web/src/server/cockpit/actions.ts`) passes to `cpd.json`, and the TUI item it matches. `—` means no cpd call. `<planId>` is derived from the title (Prompts, below).

**Context row** (`data-menu-kind="context"`, a live Context, its button is the Manager)

| Item | Web action kind | cpd argv | TUI item |
|---|---|---|---|
| New Plan… | `plan.new` | `plan new <planId> --context=<contextId> --title=<title> --by-user` | `NewPlan` |
| Rename… | `context.rename` | `context rename <contextId> --title=<title>` | `Rename` |
| Copy id | — (clipboard) | none: `navigator.clipboard.writeText(contextId)` | `CopyId` |
| Archive | `context.archive` | `context archive <contextId>` | `Archive` |
| Delete, when quiet, last | `context.delete` | `context delete <contextId> --yes`, after the confirm | `Delete` (appended by `Menu::new`) |

Delete is offered when the Context is not Standing and it is archived, or it has Plans and every Plan is quiet (archived, or its Lead's dot is `stopped`, or it has no Lead).

**Standing Context row** (`data-menu-kind="permanent"`, the Contexts `cpd`, `host` and `codereview`)

| Item | Web action kind | cpd argv | TUI item |
|---|---|---|---|
| New Plan… | `plan.new` | as above | `NewPlan` |
| Copy id | — (clipboard) | none | `CopyId` |

A standing Context row is an ordinary row of the Contexts section (pro-d79), with the same Manager button and dot. Its name is followed by a bold phi, `span[data-part="phi"][aria-hidden="true"]` with text `φ` (`-translate-y-[1px] shrink-0 select-none font-display text-[14px] font-bold leading-none text-fg-strong`), placed immediately after `span[data-part="context-id"]` (pro-d80). The id and the phi sit in a `flex min-w-0 flex-1 items-center gap-1.5` wrapper, so the names stay aligned after the status dot; the id span keeps `truncate` and has no `flex-1`. Only standing Contexts show the phi.

No Rename, Archive or Delete. The TUI's Permanent menu has the same two items (`Kind::Permanent`); whether the TUI could append Delete to one is **unverified**.

**Plan row** (`data-menu-kind="plan"`, a live Plan, its button is the Lead)

| Item | Web action kind | cpd argv | TUI item |
|---|---|---|---|
| New session… | `session.fork` (source: the Plan's Lead) | `session fork <leadSessionId> --name=<name>` | `NewSession` (a fork of the Lead) |
| Rename… | `plan.rename` | `plan rename --plan=<planId> --title=<title>` | `Rename` |
| Copy id | — (clipboard) | none | `CopyId` |
| Archive | `plan.archive` | `plan archive --plan=<planId>` | `Archive` |
| Delete, when quiet, last | `plan.delete` | `plan delete <planId> --yes`, after the confirm | `Delete` (appended) |

Delete is offered when the Plan is archived, or its Lead's dot is `stopped`, or it has no Lead (the TUI does not count a Plan without a Lead as quiet; see Divergences).

**Fork row** (`data-menu-kind="fork"`, a live fork Session row)

| Item | Web action kind | cpd argv | TUI item |
|---|---|---|---|
| Open | — (client: selects the Session and attaches it, as a row click does) | none | `Open` (the TUI runs `session open <id>`) |
| Rename… | `session.rename` | `session rename --session=<sessionId> -- <name>` | `Rename` |
| Fork… | `session.fork` (source: the row's Session) | `session fork <sessionId> --name=<name>` | `Fork` |
| Copy id | — (clipboard) | none | `CopyId` |
| Close | `session.close` | `session close <sessionId>` | `Close` |
| Archive | `session.archive` | `session archive <sessionId>` | `Archive` |

**Archived row** (`data-menu-kind="archived"`, a row inside the Archived group: a Context, a Plan, or a fork Session)

| Item | Web action kind | cpd argv | TUI item |
|---|---|---|---|
| Rename… | `context.rename`, `plan.rename` or `session.rename` | as above for the object | `Rename` |
| Copy id | — (clipboard) | none | `CopyId` |
| Unarchive | `context.unarchive`, `plan.unarchive` or `session.unarchive` | `context unarchive <contextId>`, `plan unarchive --plan=<planId>` or `session unarchive <sessionId>` | `Unarchive` |
| Delete (archived Context and Plan only) | `context.delete` or `plan.delete` | as above for the object | `Delete` (always offered: an archived row is deletable) |

An archived fork Session has no Delete.

**Not offered.** No menu on the Manager (it has no row of its own on the web; the Context row above is its row), on the empty tree area (`New Context…` is not built: `CockpitAction` has no context-create kind and `data-action="new-context"` is never rendered), on the toggle, on the `New Plan` tree row or on the Archive tree row (the TUI has no menu on these either).

**Prompts.** One `TreeModal` (a shadcn `Dialog` (2f) with an `Input`). The `DialogTitle` is the title, the `DialogDescription` is the prompt, the `Input` has `aria-label` set to the prompt and `aria-invalid` on error, its error text sits under it, and the footer has `Cancel` and `OK`. The field is focused and holds its initial value when the dialog opens. The `Input` part (`web/src/ui/components/input.tsx`, shadcn/ui `input` on the Dango tokens) is not on this base; the implementation Job adds it.

| Action | Title | Prompt | Initial value | Checks before the command runs |
|---|---|---|---|---|
| New Plan… | New Plan | `Title:` | empty | trimmed; empty gives `Title is required.`; the id is derived from the title (below); no id gives `NO_ID_ERROR` in the dialog |
| Rename… (Context, Plan) | Rename | `Title:` | the current title | trimmed; empty gives `Title is required.` |
| Rename… (fork, archived fork) | Rename | `Name:` | the current label | trimmed; empty gives `Name is required.` |
| New session…, Fork… | New session, Fork | `Name:` | empty | trimmed; empty gives `Name is required.` |
| Delete… | Delete | (none) | (none) | text `Delete <id>?`; `Cancel` and `Delete` (destructive); `--yes` is sent at once |

**Plan id.** Derived from the title by the TUI's rules (`slug.rs`): keep ASCII letters and digits; every other run of characters becomes one `-`; drop leading non-letters; lowercase; cut to 24 characters and drop a trailing `-`. Fewer than 2 characters left gives no id. If the id is taken (a Plan id in any Context), try `<cut(base, 23 - len(n))>-<n>` for n = 2, 3, and so on; the web stops at n = 9999 and refuses.

**Chat slot.** Archive (Context, Plan, fork) and Close (fork) run their command. If it fails with `cpd_failed` whose `details.cpdCode` is `in_chat_slot`, the server runs `cpd session park` (no id: the chat-slot Session) and then the same argv once more. Any other error, and an error of the retry, is returned as it is (`runAction` in `actions.ts`).

**After a create.** On an `action.result` with `ok`, when the output has `plan.id`, the new Plan is selected. Its Session (`output.lead.session` for `plan.new`, `output.session.id` for `session.fork`) is selected and attached too when it differs from the selected one (`connection.ts`).

**Copy id.** `navigator.clipboard.writeText(id)`. Tests mock the clipboard.

**Divergences from the TUI (accepted, recorded so they are not mistaken for parity).**

- Archive and Close of the object in the chat slot: the TUI first opens the next live Session (or parks) and then archives, and it refuses Close (`switch to another session first`). The web parks and retries once, as above, so a Close can close the Session that held the slot. `docs/commands.md` section 10.9 says the chat-slot Lead is never closed; the web does not follow that line.
- New session… and Fork… start empty; the TUI pre-fills `<label> (fork)`.
- No length checks (the TUI: Title 1 to 200 characters, Name 1 to 80).
- Delete shows no preview counts; the TUI shows them (`delete_text`).
- Quiet: a Plan with no Lead counts as quiet on the web, not in the TUI. The web's `stopped` dot stands in for the TUI's asleep, since the web session list carries no `asleep_at`.
- Copy id uses the clipboard; the TUI uses `tmux set-buffer -w`.
- No New Context… (see Not offered).
- A Manager row gets the Context menu on the web; the TUI gives its Manager row the Fork menu.

**Live update** (`web/src/server/cockpit/events.ts`, `readmodel.ts`). Every event kind listed in `TREE_KINDS` (72 kinds) refreshes the tree, every kind in `INBOX_KINDS` (21 kinds) refreshes the inbox, and `panesFor` maps each kind to its panes. The lists are the source; this section does not repeat them.

- **Safety refresh.** Every `SAFETY_REFRESH_MS` (30 s) the tree and the inbox are refreshed, and the Plan too when a Plan is selected.
- **Queued refreshes.** One load runs at a time. A refresh that arrives while a load runs adds its panes to one pending set; the running loop takes that set as the next batch when the current one ends. A failed batch is reported and the loop goes on.
- **After an action.** Every `runAction` is followed by a refresh, success or failure (`web/src/server/cockpit/events.ts`, in a `finally`): the tree and the inbox at once, and the Plan when one is selected. That server refresh is the refetch; the client never refetches on its own. A failed action's error goes back to the caller as `action.result` with `ok: false`.
- **Optimistic merge (design, not built).** Only `pr.merge` is optimistic (decisions pro-d392, pro-d394). On the click, the client removes at once the `pr-ready` Inbox row whose subject is `pr:<owner>/<repo>#<n>` from the inbox, and keeps the inbox it replaced. On `action.result` with `ok: false`, it restores that inbox, unless the inbox has changed since (a newer `cockpit.snapshot` or `cockpit.patch` of the inbox wins). On success nothing is restored: the server refresh above delivers the inbox that follows the merge.
- **Null tolerance.** `buildPlanView` reads the `repo` and `base` of a workflow, and the `tier` of a Job and of an attempt, as nullable, and turns null into `""`. Draft and cancelled Jobs have a null tier.
- **Error isolation.** In `loadPanes`, a failing `plan show` does not drop the tree or inbox of the same batch: the Plan becomes `null` and the other panes apply. When the Plan is the only pane asked for, its error is thrown. A failure while building the Plan view is handled the same way.

**Server hot reload.** The Electron main process reloads the server and the renderer on `SIGUSR1`. The steps and their failure modes are in `docs/ui-preview.md` (Server hot reload).

### 3h. Archived selection and the disabled Composer (pro-d406)

Status: contract for the web Job that implements it. No reducer or `App` code does it yet.

**Navigation out of an archived selection (A1).** The web compares the `cockpit.tree` it holds with the next one (a `cockpit.snapshot`, or a `cockpit.patch` of pane `tree`). It navigates when the selected session, the selected Plan or the selected Context goes from live (`archived` false) to archived (`archived` true). That covers an archive the User clicked, `/q` or `/quit` (CONTRACT 6.6) and engine E14 alike. It does **not** navigate when the User dispatches an archive: an archive can be refused (`archive_blocked`), and the User must then stay where they are. Where the selection goes:

- **Context archived** (the selected row belongs to it): the next live Context in the 3d order, standing Contexts included (pro-d79), after the archived one's place and wrapping. Select its Manager; if it has no live Manager, its first live Plan Lead. Else the `cpd` Manager. Else the `host` Manager.
- **Plan or session archived, its Context still live:** the next live Plan Lead of the same Context (its live Plans in the 3d order, after the archived one's place, wrapping to the first), selecting that Plan and its Lead. Else that Context's live Manager (`planId` null).
- A Context archive takes the first rule when both apply (a Plan Lead archived with its Context).
- No target: the selection clears (`selectedSessionId` null) and the chat slot shows `Select a session` (3e).

**Archived rows the User opens (A2).** An archived row opened on purpose from the Archive section (3d) keeps the selection. Only a live-to-archived transition navigates. A selection that is already archived when the client first sees it (a fresh load, a restore from `sessionStorage`) does not navigate either; the disabled Composer below covers it.

**Composer disabled on an archived selection.** When the selected session, the selected Plan or the active Context (the Context of the selected session; the Header breadcrumb that showed it is removed, DESIGN-panes 4.1) is archived, the Composer's textarea, attach button and Send button are `disabled`, and the textarea placeholder reads exactly:

    Session is archived - unarchive to send messages

It replaces the Composer placeholder of DESIGN-compose.md while the condition holds. Unarchive (the archived row menu, 3g) clears it.

### 3i. TopPaneHeader, the shared pane header (pro-d84, pro-d85, pro-d86)

File `web/src/ui/cockpit/TopPaneHeader.tsx`. One component gives the column headers the same bar. It lives in its own file, not in `ManagerView.tsx`: `ManagerView` imports `Dot` from `ContextTree`, so a `TopPaneHeader` imported from `ManagerView` by `ContextTree` would be a cycle (A2). No other file defines a pane header.

```ts
export function TopPaneHeader(props: {
  /** The header's `data-part`. Default `pane-header`. */
  dataPart?: string;
  headerRef?: Ref<HTMLElement>;
  /** Shows the plan pane collapse control (3c, pro-d88) when `PlanCollapseContext` is provided. Default true. */
  hasOverlayControls?: boolean;
  /** The right slot. */
  actions?: ReactNode;
  /** The left content. */
  children: ReactNode;
}): ReactElement;
```

Markup:

```
header[data-part=dataPart]   flex h-11 shrink-0 items-center gap-2.5 border-b border-border bg-background px-4
├─ {children}                 the left content
├─ span.flex-1                the spacer that pushes the actions to the right
├─ {actions}                  the right content
└─ div[data-part="plan-controls"][role="group"][aria-label="Plan pane controls"]   flex shrink-0 items-center gap-0.5   (only when `hasOverlayControls` and `PlanCollapseContext` is non-null)
    └─ button[type="button"][data-action="plan-collapse"]   the collapse control (3c)
```

- `h-11` is 44 px. The header is not sticky. It is the first child of its pane's section, outside the body's `ScrollArea`, so it stays put while the body scrolls.
- `hasOverlayControls` (pro-d88) means 'show the plan pane collapse control'. The control renders only when it is true and `PlanCollapseContext` (3c) is non-null. The Plan and Manager headers pass true; the Context tree and the Inbox pass false. The control follows `actions`, with no `pr-12` and no absolute positioning.
- Where it is used:

| Header | `data-part` | `hasOverlayControls` | left content | `actions` | Contract |
|---|---|---|---|---|---|
| Context tree | `tree-header` | false | φ mark and sr-only `Contexts` | `New context` button, only with `onNewContext` | 3d |
| Plan | `title-row` | true | manager key, `/`, Plan key, phase stepper | Review and Escalations badges | DESIGN-panes 1 |
| Manager | `manager-header` | true | manager key, counters | none | DESIGN-panes 1j |
| Inbox | `inbox-header` | false | `INBOX` heading | tier filters | DESIGN-panes 2 |

- Collapsed Plan pane (pro-d84, A6; `app.css`): when `section[aria-label="Plan pane"][data-state="collapsed"]`, `[data-part="plan-body"]` (PlanView) and `[data-part="manager-body"]` (ManagerView) are hidden, and the bottom border of `title-row` and `manager-header` is transparent. Only the header shows, at the 44 px collapsed size (3c).

## 4. Amendments pro-d48..pro-d66: where each decision is contracted

Every decision is stated as a testable contract in the section named. The shell's own contracts are here (2b, 2h, 3a, 3c, 3g); the cockpit panes are in `DESIGN-panes.md`, the chat in `DESIGN-chat.md`, the composer and dialogs in `DESIGN-compose.md`, the wire and store in `CONTRACT.md` section 11, and the manual checks in `VERIFY.md` section 9.

| Decision | Contract | Where |
|---|---|---|
| pro-d48 fonts | Roboto and Roboto Mono bundled; `--font-sans`, `--font-mono`, `--font-display`, `--font-heading` | 2b (already on main) |
| pro-d49 tier filter | seven-item ToggleGroup, right-aligned, selection rules | DESIGN-panes 2.4 |
| pro-d50 columns and gap | `gap-8` title to filters (for the Inbox header superseded by pro-d85, 3i); popover collapse; defaults 20 / 50 / 30; double-click resets; Reset layout | 3c, 3g, DESIGN-panes 2.4, 4.2 |
| pro-d51 Inbox rows | `Item` rows in an `ItemGroup`, icon / text / actions columns | DESIGN-panes 2.5 |
| pro-d52 full-height columns | media and actions are full-height columns | DESIGN-panes 2.5 |
| pro-d53 links, selection, actions | link opens `row.url`; click selects Plan and Session; curate dialog; engine start; async action buttons; error Alert | DESIGN-panes 2.6, 2.7, 4.8; CONTRACT 11.2 |
| pro-d54 composer | PromptInput layout: header, body and textarea, footer with tools | DESIGN-compose 1 |
| pro-d55 shadcn parts | transcript, messages, reasoning, tools, folds, telemetry, composer, model selector and questionnaire use vanilla shadcn parts; `shadcn-tailwind.css` | 2h, DESIGN-chat 2, 3, DESIGN-compose 2, 4 |
| pro-d56 messages and dialogs | shadcn `Message` with muted `Bubble` and Copy `Tooltip`; AskCard Questionnaire; PlanReviewCard Confirmation Item; Footer context HoverCard and Progress; edit tool blocks closed | DESIGN-chat 2.3, 2.4, 3.2; DESIGN-compose 3, 4, 5 |
| pro-d57 turns and composer | 85% turns; no Thinking or Running loaders; no telemetry row; no header toggles; Send only, follow-up; auto-scroll at 160 px; tool paths truncated; sent time | DESIGN-chat 0.2, 1, 2.2, 2.3, 2.8.1, 3.2; DESIGN-compose 1.4, 3 |
| pro-d58 border pulse | `prompt-border-pulse` on the InputGroup while busy or a send is pending | 2h; DESIGN-compose 1.5 |
| pro-d59 no stamp | no session-key stamp on assistant messages | DESIGN-chat 2.4 |
| pro-d60 header and ChatHeader | ChatHeader title only; Header `Breadcrumb` crumbs; `ask_user_question` interactive for pi | 3a; DESIGN-panes 4.1; DESIGN-chat 1; CONTRACT 11.4 |
| pro-d61 chat items | Chain of Thought grouping; image thumbnails on optimistic and echoed user messages | DESIGN-chat 2.3.1, 2.4.2, 2.7; CONTRACT 11.3 |
| pro-d62 live micro-interactions | collapsible height animations; Chain of Thought opens while busy and collapses when the turn ends; synthetic `Thinking…` step; step and message entry animations; transcript auto-scroll rules | 5.1, 5.2 |
| pro-d65 PlanView follows the Lead | the Plan shown is the selected Lead session's Plan; `selectedPlanId` follows it; the empty state is the flowchart (pro-d68, 5.3) when no Context is selected, a selected Manager shows ManagerView (pro-d70, 3e); header `min-h-11`, title truncates | 5.3 |
| pro-d66 trace rows | `FoldRow`, `ToolRow` and `ToolGroup` unified with `ChainOfThoughtRow` and `ReasoningBlock`: φ mark, middle dot before the time, trace font classes, `max-w-[45%]` removed from `TraceDescription` | 5.4 |
| pro-d84, pro-d85, pro-d86 shared pane header | one `TopPaneHeader` (h-11, sticky) on the column headers: Context tree (φ, New context; not collapsible), Plan (manager key, plan key, phase; badges), Inbox (`INBOX`, tier filters) | 3i; 3d Section layout; DESIGN-panes 1, 1j, 2 |
| pro-d87 header bar removed | the full-width Header bar (φ link, Location breadcrumb, engine pill) and the Shell `header` slot are removed; the three column `TopPaneHeader`s form the top row; the engine pill and breadcrumb navigation are a known loss, not rebuilt | 2a, 3a, 3c, 3e; DESIGN-panes 4.1 (superseded) |
| pro-d88 fixed pane header and plan collapse control | the ManagerView and PlanView headers stay fixed while their bodies scroll (`plan-scroll`, DESIGN-panes 1, 1j); the collapse control moves into `TopPaneHeader` through `PlanCollapseContext`; `TopPaneHeader` is not sticky (supersedes the `sticky` of the pro-d84..86 row); Shell's ScrollArea around the plan and its floating controls are removed; the Plan pane is not `inert` when collapsed | 3c, 3i, 5.3; DESIGN-panes 1, 1b, 1j |
| air-d121 Inbox card interaction (Plan `inbox-cards`) | the card click opens the owning Lead or Manager (`onSelectPlan` + `onSelectSession`); a row with no owner opens its `url`, else nothing; the top-right slot is the external-link anchor (`ExternalLinkIcon`, `target="_blank"`, `rel="noopener noreferrer"`) whenever `row.url` is non-null, else `ChevronRight`; the title is not a link; the action button keeps the primary-action ladder (`row.actions` -> Start -> Approve -> Claim -> Merge/Post/RePost -> Curate -> Dismiss) and never an owner-link or an external link; action errors go through the cockpit error toast (air-d44), never inline | DESIGN-panes 2.5, 2.6, 2.7, 4.8; CONTRACT 6.7 |

**Resolutions recorded while writing (for the Lead).** (1) pro-d51 says the Inbox row is an `asChild <a>`; the approved preview renders a `role="button"` root, and DESIGN-panes 2.5 follows the preview. (2) pro-d57 says Send is always enabled; the preview disables it for an empty composer, and DESIGN-compose 1.4 follows the preview. (3) pro-d56 puts the Footer context HoverCard in the chat; the preview does not mount it, and DESIGN-compose 3 mounts it where main does. (4) The composer's drop cue is kept from main; the preview lacks it. (5) pi plan reviews are interactive, as the preview does; pro-d60 names only `ask_user_question`.

---

## 5. Live micro-interactions and the Lead-following PlanView (pro-d62, pro-d65)

Records: pro-d62 (User approved the live changes in the running preview; this section lands them) and pro-d65 (PlanView always reflects the selected Lead session's Plan). Where an older section says otherwise, this section wins (recency): it supersedes the Thinking-loader removal of pro-d57 (DESIGN-chat 2.4.3), the Plan-pane and Header bindings of 3e, and the Plan title row and empty state of DESIGN-panes 1.

### 5.1 Collapsible animation (pro-d62)

- `web/src/ui/components/collapsible.tsx`: `CollapsibleContent` always carries `overflow-hidden data-[state=open]:animate-collapsible-down data-[state=closed]:animate-collapsible-up`. The caller's `className` is merged after these. Every collapsible inherits it: the Context Tree (3d) and the trace collapsibles (`trace.tsx`).
- `web/src/lib/shadcn-tailwind.css` (2h):
  - keyframes `collapsible-down`: from `height: 0; opacity: 0` to `height: var(--radix-collapsible-content-height, var(--collapsible-panel-height, auto)); opacity: 1`;
  - keyframes `collapsible-up`: the same values in reverse;
  - utility `animate-collapsible-down`: `collapsible-down 200ms cubic-bezier(0.16, 1, 0.3, 1)`;
  - utility `animate-collapsible-up`: `collapsible-up 160ms cubic-bezier(0.16, 1, 0.3, 1)`.
- `web/src/ui/components/trace.tsx`: the chevron (`TraceIcon` of `TraceCollapsibleTrigger`) is `transition-transform duration-200 ease-out` and rotates 90° while open. `TraceCollapsibleContent` has no animation class of its own; it animates through `CollapsibleContent`.
- No reduced-motion variant is specified for the collapsibles (unverified whether one is needed).

### 5.2 Chain of Thought while busy, and the transcript (pro-d62)

**Busy.** `Transcript` takes `busy?: boolean`. `ChatPane` passes `busy` as `view.busy || view.pending.some(p => p.state === "sending")` (the same definition as 2h), and passes `loadingEarlier` (now required on `Transcript`).

**Chain of Thought** (`ChainOfThoughtRow`, `Trace[data-kind="chain-of-thought"]`) takes `active?: boolean`:
- It is running when `row.running || active`. It opens while running: `isOpen = open ?? (isRunning || defaultOpen)`. When the turn ends it collapses to its default. An explicit toggle by the User is kept until the default changes.
- **Synthetic step.** While `active` and the Chain has no running step (`showSyntheticThinking = active && !row.running`), the step count includes one more step and `LiveThinkingStep` renders after the last step: `Trace[data-part="live-thinking"]` with a running `TraceLine` (`Spinner`, and `TraceTitle data-part="label"` reading `Thinking…`). The shimmer comes from the running `TraceLine` (`shimmer` in `trace.tsx`, 2h). It fills the gap between tool steps.
- **Trailing live Chain.** While busy, when the last row is not a Chain or a send is pending (`lastRowIsChain = lastRow.kind === "chain" && pending.length === 0`; `showTrailingLiveChain = busy && !lastRowIsChain`), `Transcript` renders an empty `ChainOfThoughtRow` with `data-row="live-chain"` and `active`. The transcript is not `empty` while it shows. The last row of a busy transcript gets `activeChain = busy && isLast && pending.length === 0`.

**Entry animations.** Chain roots: `animate-in fade-in-0 slide-in-from-bottom-1 duration-200`. Each step wrapper (thinking, tool, tool group) and `LiveThinkingStep`: `animate-in fade-in-0 slide-in-from-top-1 duration-200`. User, assistant and pending messages: `animate-in fade-in-0 slide-in-from-bottom-1 duration-200`.

**Auto-scroll** (`TranscriptAutoScroll`). The edge is `SCROLL_EDGE_THRESHOLD = 160` px, the distance from the bottom of the viewport.
- Unpin only when `scrollTop` falls by more than 4 px since the last scroll event and the distance is above 160 px. Pin when the distance is at or below 160 px.
- A `ResizeObserver` on the content re-scrolls to the end while pinned, so content that grows at the bottom stays in view. Without a `ResizeObserver` (`typeof ResizeObserver === "undefined"`) no growth follow happens.
- Forced pin, scrolled to the end at once: the tail epoch changes, a pending or queued item is added, or a new user turn appears (the last user item's id changes). This covers "when the user sends".
- Otherwise, the transcript scrolls to the end on item or `busy` changes only while pinned. A user who scrolled up is never scrolled.
- The jump-to-end control re-pins.
- `MessageScrollerProvider` gets no `threshold` and no `scrollToEndOnMount` props. The installed `@shadcn/react` 0.3.1 declares neither (its `MessageScrollerProviderProps` has `scrollEdgeThreshold`, `defaultScrollPosition`, `autoScroll`, `scrollPreviousItemPeek`, `scrollMargin`), so the 160 px edge and the pin are this rule alone.

### 5.3 PlanView follows the active Lead (pro-d65)

- **The Plan shown is the selected Lead's.** `activeLeadPlanId` is the id of the Plan (PlanNode in the Context tree) that holds the active session. It is `null` when no session is selected, and when the session has no Plan (a Manager, or a Context session outside any Plan).
- **`selectedPlanId` follows it.** While the tree is live, the restore has run and a session is selected, `App` calls `connection.selectPlan(activeLeadPlanId)` whenever it differs from `selectedPlanId`. `selectedPlanId` is `string | null` (`web/src/ui/state/selection.ts`, `PersistedSelection.planId`), so `null` is saved and loaded. `ContextTree` receives `selectedPlanId={activeLeadPlanId}`.
- **`visiblePlan`.** The Plan pane (`MemoPlan`) uses `visiblePlan`: the loaded `cockpit.plan` when its `planId === activeLeadPlanId`, otherwise `null`. A Plan loaded for another Lead never shows.
- **Empty state (pro-d68).** When `plan === null`, `PlanView` renders `CpdFlowCanvas` (`web/src/ui/cockpit/FlowCanvas.tsx`; root `div[data-part="flow-canvas"]`): the Plan flowchart that the Documentation dialog shows (`DocsDialog.tsx`, its flowchart tab), with its step cards collapsed at start. With no selected session, or a session in no Context, the Plan pane shows it through ManagerView (3e). A selected Manager shows ManagerView, not the flowchart (pro-d70, DESIGN-panes 1j).
- **Header.** `div[data-part="title-row"]` is the shared `TopPaneHeader` (3i): `h-11` (44 px), no wrap, not sticky: it sits outside the body's `ScrollArea` (pro-d88). It supersedes the wrapping `min-h-11` row of pro-d65. Its left content is the manager key, `/`, and `h2[data-part="plan-key"]` with the Plan id (`title={plan.title}` as its native tooltip; the key truncates), then the phase Stepper. The Review and Escalations badges are in its `actions` slot (DESIGN-panes 1); the collapse control follows them (3c, 3i).

**Tests this section changes.** `cockpit/PlanView.test.tsx` expects `[data-part="flow-canvas"]` when `plan` is null. `AppPlanFollow.test.tsx` expects the flowchart only with no selection; with a Manager selected, or a Lead whose Plan is not loaded, it expects `[data-manager-view]` (pro-d70; the test changes with the ManagerView change). `app.css.test.ts` pins `overflow: hidden` on `[data-shell] aside` and `[data-part="plan-panel"]` (pro-d69, A10). The pro-d69 scroll tests assert `data-slot` markup, never scrollbars: `Shell.test.tsx` and `shell.consolidate.test.tsx` (the Plan pane content is a `[data-slot="scroll-area"]`; the Inbox aside is `overflow-hidden`), `Inbox.test.tsx` (the list is a `[data-slot="scroll-area"]`), `StatusDialogs.test.tsx` (the Legend sections and the Logs rows), `PlanView.test.tsx` (`dag-body` is a `[data-slot="scroll-area"]` with a horizontal `ScrollBar`). `chat/WorkingCue.test.tsx` pins the removed `working-cue` element; its pins are updated to the live step of 5.2.

### 5.4 Trace rows (pro-d66)

Record: pro-d66 (User approved the change in the live preview, `/Users/Clayton.Duarte/projects/cpd.preview.web`). It unifies the trace rows of the chat: `FoldRow` (`web/src/ui/chat/Folds.tsx`), `ToolRow` (`web/src/ui/chat/ToolRow.tsx`) and `ToolGroup` (`web/src/ui/chat/ToolGroup.tsx`) take the same typography as `ChainOfThoughtRow` and `ReasoningBlock`. Where DESIGN-chat.md sections 2.4.2, 3.1, 3.2 and 3.4 give other classes for these rows, this section wins (recency). DESIGN-chat.md is not edited here.

The contract is stated before the code and the tests. The code on this branch does not carry it yet; the code Job makes it so.

- **Phi mark (`FoldRow`).** `TraceIcon[data-part="phi"]` (`size-4`) renders U+03C6 (`φ`) with the class `font-display font-bold text-[18px] leading-none -translate-y-[1.5px] select-none`. It replaces the size and colour of DESIGN-chat.md 3.1 (`text-[15px] text-status-decision`).
- **Fold labels.** `TraceLine` takes the default `text-sm font-sans`, with no `text-xs` override. `span[data-part="source"]` takes the default `text-sm font-sans`. `TraceTitle[data-part="marker-text"]` is `font-mono text-xs text-muted-foreground/70`.
- **Middle dot before the time.** When the time is non-null, a `span` with `aria-hidden="true"` and class `shrink-0 font-mono text-xs text-muted-foreground/70` renders U+00B7 (`·`) immediately before `TraceDescription[data-part="time"]`. The time is `font-mono text-xs`. The dot is not rendered when the time is null.
- **Tool rows and tool groups.** `TraceLine` and `span[data-part="name"]` take the default `text-sm font-sans`. `TraceTitle[data-part="detail"]` (`ToolRow`) and `TraceTitle[data-part="summary"]` (`ToolGroup`) are `font-mono text-xs text-muted-foreground/70`. The `TraceDescription` elements of these rows (`delta`, `status`, `totals`) are `font-mono text-xs text-muted-foreground/70`.
- **Description width (`web/src/ui/components/trace.tsx`).** `max-w-[45%]` is removed from `TraceDescription`. Its base class is `min-w-0 shrink-0 truncate text-muted-foreground/70 tabular-nums`, and a `className` is merged after it.
- **Not changed.** The chevron and collapsible animations (5.1), the Chain of Thought (5.2), and the `Trace`, `TraceCollapsible` and `TraceCollapsibleTrigger` parts.
