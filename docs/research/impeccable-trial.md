# Impeccable trial: findings

Job `docs-docs` (Plan `impeccable-trial`, Attempt `docs-docs-a1`). Source: the trial report `$CPD_HOME/run/impeccable-trial/explore/trial-report.md` (Job `trial-report`, dated 2026-11-10 16:35 EST), which ran Impeccable 4.8.2 against `web/src/ui`. This document promotes its findings; it does not re-run the trial. Repo facts cited below were checked against the tree at this commit (`web/src/ui/DESIGN.md` §2b, `web/src/ui/app.css`, `web/src/ui/dango.css.test.ts`).

## Summary

- **Design-check hook on `web/src/ui`:** 5 `overused-font` warnings (exit 2), all for Roboto, all sanctioned by `web/src/ui/DESIGN.md` §2b. 0 AI-slop tells.
- **Cockpit audit and critique:** 0 static anti-patterns in the five cockpit components. Agreed: keyboard navigation across split panes, touch targets under 44 px, error recovery in the Composer. Contested: looser row spacing, empty-state illustrations, decorative colour.
- **Design system fit:** aligns with `DESIGN.md`, the Tailwind tokens and the shadcn parts (on Base UI, not Radix). It adds static guardrails against AI tells and adds no conflicting CSS.
- **Cost and Gate:** `impeccable detect` runs in 0.25 s with 0 model calls and 0 network requests, so it is viable offline in the web Gate. Audit and critique cost 12k–35k tokens ($0.04–$0.12) per run and must stay out of CI.
- **Pi:** install upstream `impeccable@4.8.2` with `--providers=pi --project`, pinned by convention over configuration. The community package `@herbertgao/pi-impeccable@0.3.0` vendors upstream and breaks the axiom.

## 1. Design-check hook on `web/src/ui`

Method: `impeccable detect --json .` over `web/src/ui` (182 files: `.css`, `.ts`, `.tsx`), static regex and AST matching, no browser, no network, no model.

| Rule | Category | Severity | Count | Where |
|---|---|---|---|---|
| `overused-font` | slop | warning | 5 | `web/src/ui/app.css` lines 7, 14, 21, 28; `web/src/ui/dango.css.test.ts` line 36 |

Total: 5 warnings, 0 errors, 0 advisories.

- **Rule.** `overused-font` flags Roboto, Inter, Geist, Fraunces, Plus Jakarta Sans and Space Grotesk, the faces that AI-generated UIs converge on.
- **Verdict: sanctioned, not a defect.** `web/src/ui/DESIGN.md` §2b pins `--font-sans` to Roboto Variable and `--font-mono` to Roboto Mono Variable, bundled locally with no network font fetch (decision pro-d48). The test in `dango.css.test.ts` asserts the same contract.
- **Resolution (not applied in this Job):** `impeccable ignores add-value overused-font Roboto --reason "DESIGN.md Section 2b pins Roboto and Roboto Mono"`. The suppression is recorded in `.impeccable/config.json`. After it, a scan reports 0 findings over 182 files.
- **AI slop.** 0 occurrences of: side-tab accent borders, gradient text, AI colour palettes, dark or radial glows, nested cards, marketing buzzwords, pulsing dots, marquees.

## 2. Cockpit audit and critique

Scope: `cockpit/ContextTree.tsx`, `cockpit/ManagerView.tsx`, `cockpit/PlanView.tsx`, `cockpit/Inbox.tsx`, `composer/Composer.tsx`.

- **Static detector:** 0 mechanical anti-patterns across all five.
- **Method:** Impeccable's `/audit` (technical quality) and `/critique` (UX heuristics), applied by hand to the component code.

### Agreed

1. **Keyboard navigation and focus across panes** (ContextTree, Inbox, PlanView). Arrow-key roaming exists in parts of the tree but has no `aria-activedescendant` or roving `tabIndex` when focus jumps between sections, such as from ContextTree to the Inspector or the Plan DAG. Heuristics: Nielsen #3 and #7, WCAG 2.1.1. Agreed: complete keyboard navigation without focus traps is a real gain for a power-user cockpit.
2. **Touch targets on compact controls** (ContextTree, Inbox). Sub-item buttons, row menu buttons, snooze triggers and counter badges measure 20×20 or 22×22 px against WCAG 2.5.8's 24 px minimum. Agreed in principle, with a qualification: the WebUI is a desktop operator console, not a touch-first app. `touch-action: manipulation` or an invisible hit-box extension (`after:absolute after:-inset-1`) would help trackpads and tablets without changing density.
3. **Error recovery and stale state** (Composer, PlanView). A rejected attachment or an unknown DAG node state surfaces as a console warning or a silent input rejection, not an inline recoverable message. Agreed. Draft persistence (`loadDraft`, `saveDraft`) already prevents data loss; inline error badges would improve diagnosis.

### Contested

1. **Row spacing and "monotonous" density** (ContextTree, Inbox, PlanView). Impeccable's rubric calls the 28 px rows and 11/12.5 px type "cramped" and suggests 12–16 px padding. Contested: the WebUI is a dense operator cockpit modelled on Bloomberg and TUI interfaces. Looser rows halve the visible context and force scrolling. Typography follows the four text sizes in `web/src/ui/DESIGN.md` §1.2 and `DESIGN-panes.md` (`text-meta` 11 px, `text-key` 12.5 px, `text-body` 13 px, `text-title` 15 px). The trial report calls this "Operate mode"; that term does not appear in the repo's design docs (unverified as a named contract).
2. **Onboarding illustrations for empty states** (`Empty.tsx`, `Inbox.tsx`). Impeccable's `/onboard` and `/critique` advocate illustrated or guided empty states. Contested: an empty inbox or an idle plan is the normal operating state. The minimal `<Empty>` (`components/empty.tsx`, DESIGN §2f) carries mono status headers. Decorative illustrations would be slop. (The trial cites `DESIGN.md` §10 for this; no §10 exists in the repo's design docs.)
3. **Expressive colour and `/colorize`.** Impeccable's critique calls the UI dark and monochrome and suggests decorative category hues. Contested: all colour is semantic, derived only from the `dango-status.ts` tones (`blocked`, `landing`, `decision`, `active`, `focus`, `done`, `unread`, `neutral`). `DESIGN.md` §1.2 rule 3 forbids decorative colour and requires every tone to pair with a glyph shape, so status is never colour alone.

## 3. Fit with the design system

| CPD foundation | Impeccable equivalent | Fit |
|---|---|---|
| `DESIGN.md` and `DESIGN-*.md` | `DESIGN.md` and `PRODUCT.md` | Strong. Impeccable reads the local `DESIGN.md` (145 KB, detected by `impeccable context`). Its drift rules (`design-system-font`, `design-system-color`, `design-system-radius`) enforce what these files already define. |
| Tailwind tokens (`app.css`, `globals.css`) | Tailwind v4 tokens and CSS custom properties | No conflict. Both prefer utilities and variables such as `var(--background)` over hard-coded values. Impeccable flags inline hex and RGB in TSX, matching `DESIGN.md` §1.2 ("tokens, never hex"). |
| shadcn base components (`@base-ui/react`) | shadcn/ui pattern library | Same lineage. Impeccable's authoring skills recommend shadcn structures (Collapsible, Dialog, Sidebar, ScrollArea). It adds no competing runtime UI library. **Correction to the trial report:** CPD's parts wrap Base UI, not Radix. `web/src/ui/DESIGN.md` §2f says "never Radix UI, never React Aria", and composition uses a `render` prop, not `asChild`. |
| `dango-status.ts` (tones and glyphs) | Semantic colour and status rules | Complementary. Impeccable bans decorative pulses and glows on status indicators, which reinforces the glyph-plus-tone rule. |

What Impeccable adds: (1) mechanical lint for AI tells, run before commits; (2) 25 slash commands (`/critique`, `/audit`, `/harden`, `/typeset`, `/polish` and others) as review checklists for feature shaping; (3) rules for mobile adaptation, WCAG contrast and touch boundaries.

## 4. Cost and web Gate

### Cost per command

| Command | Execution | Model calls | Input tokens | Output tokens | Est. cost (Claude 3.7 Sonnet) |
|---|---|---|---|---|---|
| `impeccable detect` | native Rust binary (regex, AST, CDP) | 0 | 0 | 0 | $0.00 |
| `impeccable ignores` | local CLI, JSON CRUD | 0 | 0 | 0 | $0.00 |
| `impeccable context` | local CLI, metadata | 0 | 0 | 0 | $0.00 |
| `/impeccable audit` | agent playbook, technical scan | 1–2 | ~15,000 | ~2,500 | ~$0.06–$0.08 |
| `/impeccable critique` | agent, dual isolated subagents | 2–3 | ~25,000 | ~4,500 | ~$0.10–$0.14 |
| `/impeccable polish` | agent playbook with file edits | 2–4 | ~35,000 | ~6,000 | ~$0.14–$0.22 |
| `/impeccable shape` / `init` | multi-round discovery interview | 4–8 | ~60,000 | ~10,000 | ~$0.25–$0.40 |

Token and cost figures are the trial's estimates, not metered runs.

### Spend context (b14)

`pro-n419` records that durable worker logging and long agent sessions made up about $65 of a $115 Bifrost spend burst, and `pro-n418` records short-TTL idle collapses. `/critique` runs two or three subagents, so running it on every PR or commit would repeat that pattern.

**Decision for the Gate:** the LLM-driven commands (`/critique`, `/shape`, `/polish`, `/audit`) are barred from headless CI and unattended worker loops. They run only when a human triggers them or in a dedicated design explore Job.

### Gate viability

- `impeccable detect` ships as a compiled native binary (`@impeccable/cli-darwin-arm64`, `@impeccable/cli-linux-x64` and others) at `bin/impeccable`. With the binary cached it needs no Node, Bun, Python or network.
- 182 files in `web/src/ui` scanned in 0.25 s.
- Exit codes: 0 clean or advisories only, 1 operational failure, 2 primary anti-pattern findings.
- **Verdict (trial result):** `impeccable detect` can run in the web Gate offline, with zero model calls and zero network requests, like ESLint or Biome. The trial did not wire it into the Gate; that is a follow-up in `BACKLOG.md`.

## 5. Pi installation

| Dimension | Upstream `impeccable@4.8.2` | Community `@herbertgao/pi-impeccable@0.3.0` |
|---|---|---|
| Source | upstream (Paul Bakaus, Apache-2.0) | third-party (Herbert Gao) |
| Version | 4.8.2, engine 0.1.21 | vendors Impeccable 4.8.2 (commit `035c8f3`) |
| Pi integration | built in: `install --providers=pi` writes `.pi/skills/impeccable` | standard `pi-package` with `pi.skills: ["./skills"]` |
| Engine delivery | checksum manifest, binary cached in `~/.impeccable/bin/0.1.21/` | vendored skill files, engine via upstream script |
| Offline Gate binary | `@impeccable/cli-<platform>` directly | needs a vendor tree or a global npm install |
| Axiom | passes: upstream dependency | breaks "never vendor … take the dependency" |

**Recommendation:** install upstream, pinned:

```bash
npx impeccable@4.8.2 install --providers=pi --project -y
```

This detects the Pi harness, writes `.pi/skills/impeccable/SKILL.md` and the command references, and installs the verified engine binary. The community package stays out unless a multi-agent `pi` manifest requires it, in which case it is a recorded deviation with its reason.

For the web Gate (not applied in this Job), the trial's pinned sketch is `impeccable@4.8.2`, `@impeccable/cli-darwin-arm64@0.1.21` and `@impeccable/cli-linux-x64@0.1.21` as devDependencies, with `lint:design` running `impeccable detect src/ui`.

## 6. Recommendations

1. **Gate:** adopt `impeccable detect` as an offline, zero-model step in the WebUI pre-merge Gate. Tracked in `BACKLOG.md`.
2. **Ignore policy:** commit `.impeccable/config.json` with `overused-font` suppressed for Roboto, per `web/src/ui/DESIGN.md` §2b.
3. **Agent workflows:** offer `/typeset`, `/layout` and `/audit` as opt-in tools for frontend authoring. Forbid automated multi-subagent `/critique` in unattended CI or worker loops.
4. **Pi:** install upstream `impeccable@4.8.2` for Pi, as in §5.

## Open items

- Not verified in this Job: the trial's timings and token counts were not re-measured here. They are the trial report's figures.
- Not verified: the keyboard-navigation and touch-target gaps in §2 were judged by critique, not by an assistive-technology or device test.
- Not verified: the trial report's 'Operate mode' contract and its DESIGN.md §10 citation (see §2).
