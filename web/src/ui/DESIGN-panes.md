# Dango v3 + shadcn/ui: design contract (Plan View and Attention Inbox)

Builds on `web/src/ui/DESIGN.md` (called **DESIGN** below). Reused by reference, not restated: conventions (DESIGN 1: `data-part`/`data-action` hooks, tokens never hex in `.tsx`, the four text sizes `text-meta` 11px / `text-key` 12.5px / `text-body` 13px / `text-title` 15px, `bg-row-hover`), tokens (DESIGN 2b), glyph classes (2c), `web/src/ui/dango-status.ts` (2d: `jobDisplay`, `parsePhase`, `phaseLabel`, `phaseTone`, `inboxDisplay`, `urgencyDisplay`, `glyphClass`, `toneClasses`), `cn` (2e), `Button`/`Badge` (2f). Hex values below are the board's rendered values, written next to the token that carries them; `.tsx` files use the token (an arbitrary opacity such as `bg-status-active/[.14]` is allowed, a hex is not).

Files: `web/src/ui/cockpit/PlanView.tsx`, `web/src/ui/cockpit/Inbox.tsx`. Imports are relative (A1). Both stay wrapped in `memo()` by `App` (DESIGN 3e); their props change only by the optional additions named here, and `App` passes none of them. Phase 2 tests: `PlanView.test.tsx` (3 tests), `Inbox.test.tsx` (3 tests); no existing test changes (3d below).

Source for every field: `web/src/protocol/cockpit.ts` (`PlanViewModel`, `WorkflowView`, `JobView`, `AttemptView`, `InboxModel`, `InboxRow`, `InboxRowAction`, `CockpitAction`) and `web/src/server/cockpit/actions.ts` (`actionArgs`). Nothing in `web/src/protocol/**`, `state/**`, `server/**` changes (A6).

---

## 1. Plan View

### 1a. Props

```ts
import type { CockpitAction, PlanViewModel } from "../../protocol/cockpit.ts";

export function PlanView(props: {
  plan: PlanViewModel | null;                                   // KEPT
  onAction(action: CockpitAction): void;                         // KEPT
  /** Retry a failed job. NOT WIRED: cpd has no retry action (see 1h). The `Retry ▸` button renders only when this is passed AND job.status is "failed". `App` does not pass it. */
  onRetryJob?: (jobKey: string, workflowKey: string) => void;
}): ReactElement;
```

Module-private helpers (not exported): `Stepper`, `ExploreStrip`, `WorkflowCard`, `Dag`, `layoutDag` (pure, 1e).

### 1b. DOM outline

`plan === null`:

```
p[data-empty]   text exactly "No plan selected"     KEPT (PlanView.test 3); className grid h-full place-items-center text-muted-foreground
```
No `section` element is rendered in that case (KEPT: `querySelector("section")` is null).

`plan !== null`:

```
section[aria-label="Plan {planId}"][data-phase="{plan.phase}"]            KEPT label; flex flex-col gap-2.5 px-4 pb-3
├─ div[data-part="title-row"]            flex h-11 flex-wrap items-center gap-3.5 pr-16      (h-11 = 44px; pr-16 = 64px kept free for Shell's plan controls, DESIGN 3c)
│   ├─ h2                                {plan.title}      KEPT (test: h2 contains "Web UI"); font-mono text-[14px] font-semibold text-fg-strong
│   ├─ span[data-part="plan-key"]        {plan.planId}     font-mono text-meta text-fg-dim
│   ├─ ol[aria-label="Phase"][data-part="stepper"]     (1c)
│   ├─ span.flex-1
│   ├─ p[data-part="review"]             only when plan.review !== null; text exactly `Review {review.id}: {review.status}`   KEPT copy (PlanView.test 1: "Review r1: approved"); font-mono text-meta text-muted-foreground
│   └─ p[data-part="escalations"]        only when plan.openEscalations > 0; text exactly `Escalations: {n}`   KEPT copy; font-mono text-meta text-status-blocked
├─ section[aria-label="Explore jobs"][data-part="explore"]    only when explore jobs exist (job.workflow === null)   KEPT label   (1d)
├─ section[data-workflow="{wf.key}"][aria-label="Workflow {wf.key}"]  ×N   KEPT hooks   (1e to 1g)
└─ p[data-part="no-work"]                only when plan.workflows is empty AND there are no explore jobs; text exactly "No workflows yet"; px-1 text-muted-foreground
```
Order of children: title row, explore strip, workflow cards (in `plan.workflows` order), no-work note.

The old bare `<p>{plan.phase}</p>` is replaced by the stepper (no test pins it). The old standalone `h3`/`p` lines become the card header (1f).

### 1c. Plan key and phase stepper

- **Plan key.** `h2` shows the Plan's title (`plan.title`); `span[data-part="plan-key"]` shows `plan.planId`. (The board's `chat-research` is a key shown as the title; cpd carries both, and Phase 2 pins the title in `h2`.)
- **Phases** (the protocol's `PlanNode.phase`, DESIGN 2d `Phase`): `grounding`, `plan`, `dispatch`, `deploy`, `done`. **The board's `planning` is cpd's `plan`**: the `li` text is `phaseLabel[phase]` (`grounding`, `planning`, `dispatch`, `deploy`, `done`), the compared value is the cpd string.
- `parsePhase(plan.phase)` is the only boundary: `known` → that phase is current; `unknown` → no step is current and a trailing `span[data-part="phase-unknown"]` with the raw text (`font-mono text-meta text-fg-dim`, after the list) is rendered.

```
ol[aria-label="Phase"][data-part="stepper"][data-current="{plan.phase}"]   flex items-center gap-1 m-0 list-none p-0 font-mono text-meta
├─ li[data-phase="grounding"][data-state="past|current|upcoming"]  (aria-current="step" only when current)
├─ li[aria-hidden="true"][data-part="sep"]  "—" (U+2014)            text-line
├─ li[data-phase="plan"] ... "planning"
├─ sep, li[data-phase="dispatch"], sep, li[data-phase="deploy"], sep, li[data-phase="done"]
```
9 `li` in total (5 steps, 4 separators), the order above. `data-state`: steps before the current one `past`, the current `current`, after `upcoming`; all `upcoming` when the phase is unknown.

| `data-state` | className |
|---|---|
| `past` | `px-1.5 py-0.5 text-muted-foreground` |
| `upcoming` | `px-1.5 py-0.5 text-fg-dim` (the stepper sits on `bg-background`, so fg-dim is allowed; board `#87808c`) |
| `current` | `rounded-[4px] bg-status-active/[.14] px-2 py-0.5 font-semibold text-status-active-bright` (board: `#92d1ca` on `rgba(103,173,167,.14)`) |

**Resolution:** the board paints only the active step and greys the rest (`#87808c`); `past` is lightened to `#aba3af` for orientation only (no value in the board contradicts it); the current step is always mizu (`active`) as the board renders it, even for `plan`/`deploy`/`done` where `phaseTone` differs, because the board has one active style. `phaseTone` is therefore **not** used by the stepper.

### 1d. Explore chip strip

Explore jobs: `plan.jobs.filter(j => j.workflow === null)`, in `plan.jobs` order. Strip not rendered when there are none (KEPT: `section[aria-label="Explore jobs"]` contains `li[data-job="job-x"]`).

```
section[aria-label="Explore jobs"][data-part="explore"]     flex flex-wrap items-center gap-2
├─ span[data-part="explore-summary"]    font-mono text-[11.5px] text-muted-foreground mr-1
│     text: "explore" then, for each distinct job.kind in first-appearance order, ` · {count} {kind}`
│     e.g. kinds scout×2, research×3 → "explore · 2 scout · 3 research"; test fixture (one job, kind "explore") → "explore · 1 explore"
└─ ul[data-part="chips"]    contents; flex flex-wrap gap-2 (the ul is `display: contents`-equivalent: className `contents`)
    └─ li[data-job="{key}"][data-status="{status}"][data-kind="{kind}"]  ×N      KEPT data-job, data-status
        flex h-6 items-center gap-1.5 rounded-full border px-2 text-[11.5px]
        ├─ span[aria-hidden="true"]  className={glyphClass[jobDisplay(status).glyph]}
        ├─ span.font-mono[data-part="name"]   {job.key}
        ├─ span.font-mono[data-part="tier"]   {job.tier}      text-fg-dim
        └─ span.sr-only[data-part="status"]   `{jobDisplay(status).label}`
        + (needs / attempts sr-only text exactly as 1e, Job text contract)
```
Chip look by status (`jobDisplay(status).tone`):

| status / tone | border | name colour |
|---|---|---|
| `running` (active) | `border border-status-active/55` (board `rgba(103,173,167,.55)`) | `text-fg-strong` |
| `queued`, `draft`, `cancelled`, unknown (neutral, decision for draft) | `border border-dashed border-line` | `text-muted-foreground` |
| every other (merged, landing, checking, merging, failed) | `border border-border` (`#2e2a33`) | `text-foreground` (failed: `text-fg-strong`, border `border-status-blocked`) |

The chip is **not** interactive (no handler exists; the board's chips have none either): `li`, not `button`. Running time (`1m 48s`) is Not wired.

### 1e. Workflow card and the job DAG

Per `plan.workflows` entry `wf`. Card: `section[data-workflow="{wf.key}"][aria-label="Workflow {wf.key}"][data-status="{wf.status}"]` (KEPT hooks; `data-status` new), className `rounded-[9px] border border-border bg-card`.

```
section[data-workflow]
├─ header[data-part="wf-header"]   flex h-[34px] flex-wrap items-center gap-2.5 border-b border-border px-3      (34px)
│   ├─ h3                          {wf.key}       KEPT; font-mono text-key font-semibold text-fg-strong (12.5px)
│   ├─ span[data-part="wf-route"]  `{wf.repo} → {wf.base}`  (U+2192)   font-mono text-[11.5px] text-muted-foreground
│   ├─ span[data-part="wf-status"][data-tone]   {wf.status} verbatim   font-mono text-meta rounded-[4px] px-1.5 py-px
│   ├─ span[data-part="wf-failed"]   only when failed > 0; text `{n} failed`   font-mono text-meta text-status-blocked-bright (board #ffa796)
│   ├─ span.flex-1
│   └─ p[data-part="pr"]   only when wf.pr !== null  (1f)
└─ div[data-part="dag-body"]   overflow-x-auto p-3          (12px padding)
    └─ div[data-part="dag-canvas"]   relative   style: width {W}px; height {H}px   (1e-ii)
        ├─ svg[data-part="dag-edges"][aria-hidden="true"]   absolute left-0 top-0 pointer-events-none   width={W} height={H} viewBox="0 0 {W} {H}" fill="none"
        │   └─ path[data-edge="{from}>{to}"][data-state="done|pending"] ×(one per edge)   stroke-width 1.5
        └─ ul[data-part="dag"]   absolute inset-0 m-0 list-none p-0
            └─ li[data-job][data-status]  ×N   (1e-iii)   DOM order = `wf.jobKeys` order
```
Header value rules: `wf-status` colour from `wf.status`: `open` → tone `active` (`text-status-active-bright bg-status-active/[.14]`, board `#92d1ca` on `rgba(103,173,167,.14)`), `merged` → `done` (`text-status-done bg-status-done/10`), `waiting` → `decision` (`text-status-decision-bright bg-status-decision/10`), anything else (`cancelled`, unknown) → `neutral` (`text-muted-foreground bg-surface-pressed`). `data-tone` = that tone. `{n}` = count of the workflow's jobs whose `parseJobStatus(status)` is `failed`. **Resolution:** the board's phase badge (`dispatch`) has no source (a Workflow has a status, not a phase); the badge shows `wf.status`. The existing Phase 2 line `{status} · {repo} · {base}` is split into route + status; the test needs only `wf.textContent` to contain `main` (route carries `base`). The board's `PR opens at <job>`, duration and cost are Not wired.

**(i) Unknown job.** A key in `wf.jobKeys` not found in `plan.jobs` **throws** `workflow {wf.key} lists unknown job {key}` (KEPT Phase 2 behaviour).

**(ii) Layout rule `layoutDag(wf, jobs)`** (pure; geometry is in CSS px, exact):

Constants: `NODE_W = 148`, `NODE_H = 46`, `GAP_X = 28` (connector column), `GAP_Y = 8`, `COL_STEP = NODE_W + GAP_X = 176`, `ROW_STEP = NODE_H + GAP_Y = 54`.

1. Nodes = the workflow's jobs, in `wf.jobKeys` order. `needs` entries that are not in `wf.jobKeys` (other workflow, unknown, e.g. `job-0`) create **no** edge and no node.
2. `col(job)` = 0 when it has no in-workflow needs, else `1 + max(col(need))`. A dependency cycle **throws** `workflow {wf.key} has a dependency cycle at {key}`.
3. `row(job)` = the index of the job among the jobs of its column, in `wf.jobKeys` order (0-based).
4. `ncols = max(col) + 1`; `colSize(c)` = jobs in column c; `H = max_c (colSize(c)·46 + (colSize(c) − 1)·8)`; `W = ncols·148 + (ncols − 1)·28`. Empty workflow (`jobKeys` empty): `W = 0`, `H = 0`, and `div[data-part="dag-body"]` renders a `p[data-part="no-jobs"]` "No jobs" (`text-muted-foreground text-[11.5px]`) instead of the canvas.
5. Each column is vertically centred: `top(job) = (H − h_c)/2 + row·54` where `h_c = colSize(c)·46 + (colSize(c) − 1)·8`; `left(job) = col·176`. Applied as inline `style={{ left, top }}` on the `li`. `cy(job) = top + 23`.
   Check against the board: fork column of 2 → H = 100; the single node before it has top 27 (`cy = 50`); fork nodes `top` 0 and 54 (`cy` 23 and 77); matches the board's 28×100 connectors `M1 50 … 25 23` and `… 25 77`.
6. **Edges**: for every job `t` and every in-workflow need `s` (in `wf.jobKeys` order of `t`, then order of `t.needs`), one path `data-edge="{s}>{t}"`. Let `sx = left(s) + 148` (right side of the source), `tx = left(t)` (left side of the target), `ys = cy(s)`, `yt = cy(t)`. The gutter after the source column spans `sx … sx+28`.
   - `d` when `col(t) = col(s) + 1` and `ys = yt` (straight lane, with arrowhead; board `M1 23H25M21 19.5L25 23 21 26.5` shifted by `sx`):
     `M{sx+1} {ys}H{sx+25}M{sx+21} {ys−3.5}L{sx+25} {ys} {sx+21} {ys+3.5}`
   - `d` when `ys ≠ yt` (fan-out and fan-in curves, no arrowhead; board fork `M1 50C14 50 12 23 25 23` shifted by `sx`):
     `M{sx+1} {ys}C{sx+14} {ys} {sx+12} {yt} {sx+25} {yt}`
     This one rule yields **fan-out** (one source, several targets: each target gets its own curve from the same start point `(sx+1, ys)`) and **fan-in** (several sources, one target: each source gets its own curve ending at the shared point `(sx+25, yt)` of *its* gutter; the board's join `M1 23C14 23 12 50 25 50 M1 77C14 77 12 50 25 50`). Fan-in sources are in the same column, so they share `sx`; the two curves of the board are exactly two `path`s.
   - **Skip edge** (`col(t) > col(s) + 1`): the same `d` as above for the gutter after the source (straight with arrowhead when `ys = yt`, else the curve), then `H{tx−3}` appended when `ys = yt`, or `H{tx−3}` appended after the curve's end `(sx+25, yt)` when `ys ≠ yt`. It runs behind intermediate nodes (they have an opaque `bg-card` base, 1e-iii), so it is visible only in gutters. (Board has no skip edge; this is the only rule that needs none of the board's geometry; inferred.)
   - Stroke by **source** job status (`parseJobStatus(s.status)` = `merged` → `done`; anything else, including unknown → `pending`):
     `done`: `stroke="#8da96e"` as class `stroke-status-done`, `stroke-opacity=".6"`, solid. `pending`: class `stroke-line` (`#5a504a`), `stroke-dasharray="3 3"`. (The board colours a connector by the state of the job it leaves; verified for fork (source merged: solid) and join (sources checking/failed: dashed).)
7. A target with several needs and a source with several targets use the same path rule; edges are not merged or bundled.

**(iii) Node** `li[data-job="{key}"][data-status="{status}"]` (KEPT hooks; `data-status` is the raw `job.status`), plus new `data-tone`, `data-glyph`, `data-tier`. `parseJobStatus`/`jobDisplay` give tone, glyph, label. Box: `absolute box-border h-[46px] w-[148px] rounded-[6px] border px-[9px] py-[5px] flex flex-col justify-center gap-0.5 bg-card` (148×46 border-box, radius 6, padding `5px 9px`).

```
li[data-job][data-status][data-tone][data-glyph][data-tier]
├─ span[data-part="node-tint"][aria-hidden="true"]   absolute inset-0 rounded-[5px] pointer-events-none   (tint class, table below; omitted when "none")
├─ span[data-part="node-head"]   relative flex items-center gap-1.5
│   ├─ span[aria-hidden="true"]  className={glyphClass[glyph]}        (failed: the 9px-wide triangle of DESIGN 2c)
│   ├─ span[data-part="name"]    {job.key}                font-mono text-[12px] flex-1 truncate
│   └─ span[data-part="tier"]    {job.tier}               font-mono text-[10.5px] text-muted-foreground      (the Tier badge: plain text, no border; board)
├─ span[data-part="node-sub"]    relative flex items-center gap-1.5   font-mono text-meta
│   ├─ span[data-part="status"]  subtitle text (below)    flex-1 truncate
│   └─ button[data-action="retry"][type="button"]   only failed + onRetryJob passed (1h)
├─ span.sr-only[data-part="needs"]      only when job.needs.length > 0; text exactly ` needs: {needs.join(", ")}`      KEPT copy (PlanView.test 2: contains "needs: job-1, job-0"; row without needs contains no "needs:")
└─ span.sr-only[data-part="attempt"] ×(one per job.attempts item, key a.id)   text exactly ` #{a.number} {a.status}`   KEPT copy (PlanView.test 1: li[data-job="job-2"] contains "#1 running")
```
`job.title` (visible in Phase 2) is the node's `title` attribute (`title={job.title}`) and is not drawn (the board shows keys only).

Subtitle text (`[data-part="status"]`), exact:

| condition | text |
|---|---|
| `failed` and `n ≥ 2` (n = attempts whose `status === "failed"`; inferred, the board's `failed ×2`) | `failed ×{n}` (U+00D7) |
| `queued` and `u ≥ 1` (u = in-workflow-or-not needs whose job is missing from `plan.jobs` or whose status is not `merged`) | `blocked on {u}` |
| anything else | `jobDisplay(status).label` (`merged`, `running`, `checking`, `queued`, `failed`, raw string for unknown ...) |

**Resolution:** the board prints `queued` for `rpc-verify` although `rpc-impl` is unfinished and `blocked on 2` for `chat-impl`; the rule above is the single data-derived reading (`u ≥ 1`), the board's `queued` there is a hard-coded sample. Times (`merged · 1m 02s`) are Not wired: no timestamp is in `AttemptView`.

Node look by status (`jobDisplay(...).tone` and status), exact:

| status | border | tint (`node-tint`) | name colour | subtitle colour |
|---|---|---|---|---|
| `merged` | `border-input` (`#35313b`) | none | `text-foreground` (`#d8d2cc`) | `text-status-done` (`#8da96e`) |
| `running` | `border-status-active` | `bg-status-active/[.08]` | `text-fg-strong` | `text-status-active-bright` (`#92d1ca`) |
| `landing`, `checking`, `merging` | `border-status-landing` | `bg-status-landing/[.08]` | `text-fg-strong` | `text-status-landing-bright` (`#e4bb7f`) |
| `failed` | `border-status-blocked` | `bg-status-blocked/[.09]` | `text-fg-strong` | `text-status-blocked` (`#e17f6e`) |
| `queued`, `draft`, `cancelled`, unknown | `border-dashed border-line` (`#5a504a`) | none | `text-foreground` | `text-muted-foreground` (`#aba3af`) |

### 1f. PR link and Post PR

`p[data-part="pr"]` renders only when `wf.pr !== null` (KEPT behaviour), `flex items-center gap-2 font-mono text-[11.5px]`:
- `a[href="{pr.url}"]` text exactly `` `#${pr.number} ${pr.state ?? ""}`.trim() `` (KEPT; test: contains `#7 open`); `text-status-focus-bright underline-offset-2 hover:underline`.
- `button[type="button"][data-action="post-pr"]` text exactly `Post PR`; **KEPT hook**, KEPT copy. Click → `onAction({ kind: "pr.post", planId: plan.planId, workflow: wf.key })` (existing action; CLI `pr post --workflow <key> --plan <planId>`). `Button variant="outline" size="xs"` (h-6). Always enabled.

`checks` and `mergeable` of the PR are not drawn (Not wired).

### 1g. States

| State | Rendering |
|---|---|
| `plan === null` | 1b empty `p[data-empty]` |
| plan, no workflows, no explore jobs | title row + `p[data-part="no-work"]` "No workflows yet" |
| plan, workflows only | no explore strip |
| plan, explore only | strip, no cards, no `no-work` |
| workflow with zero jobs | header + `p[data-part="no-jobs"]` "No jobs" |
| job status not in the closed union | `jobDisplay` neutral idle ring, label = raw text, `data-status` = raw |
| long DAG | `dag-body` scrolls horizontally (`overflow-x-auto`); the canvas keeps its fixed `W`×`H` |

No keyboard behaviour: nothing in the Plan View except `Post PR` and `Retry ▸` is interactive; both are native buttons (Tab, Enter, Space).

### 1h. Retry ▸ and Not wired

`Retry ▸`: `button[data-action="retry"][type="button"]`, text exactly `Retry ▸` (U+25B8), `font-mono text-meta font-semibold text-status-blocked-bright` (board `#ffa796`), no border, `hover:underline`, `aria-label="Retry {job.key}"`. It exists **only** when `onRetryJob` is passed and the job is `failed`; click → `onRetryJob(job.key, wf.key)`. **No existing action fits**: `CockpitAction` has `inbox.*`, `pr.post`, `session.retier`, `plan.reject`; `actionArgs` has no `job retry` command (`session.retier` changes a Tier, it does not re-run a job). So Retry is **Not wired**: `App` does not pass `onRetryJob`, the button is not rendered, and nothing is dispatched to `onAction`.

**Not wired (omitted, never faked)**: `Retry ▸` (no action); job and attempt durations (`merged · 1m 02s`, `running · 2m 14s`, `checking · 38s`); the plan's `18m·42s active` timer; plan, workflow and job cost (`$2.140`); the explore chip run time (`1m 48s`); `PR opens at <job>` note; workflow duration; the workflow phase badge (shows `wf.status` instead); PR checks/mergeable; clicking a node or chip (no handler on the board); plan-level counts (`plan.counts`) are not drawn in the Plan View (the Context Tree shows them).

---

## 2. Attention Inbox

The web right column is the flat Inbox only. The TUI's machine block (hostname, CPU, GPU and memory) is not built (pro-d36). The Inbox is the TUI attention engine (`tui/src/widgets/inbox.rs`), so it follows that file: one flat list in `cpd inbox list` order, with no tier groups, no per-tier counts, no kind text, no inline buttons, no engine banner and no key footer. Secondary actions live in a right-click menu (pro-d40).

### 2a. Props

```ts
import type { CockpitAction, InboxModel } from "../../protocol/cockpit.ts";

export function Inbox(props: {
  inbox: InboxModel | null;                       // KEPT
  onAction(action: CockpitAction): void;          // KEPT
  onSelectSession(sessionId: string): void;       // KEPT
}): ReactElement;
```
No new props. The inbox panel is the Shell's (`aside[aria-label="Inbox column"]`, DESIGN 3c, 22% of the row at start, 14% to 45%); `Inbox` fills it: `aside[aria-label="Inbox"]` (KEPT; nested in the column aside) `flex h-full min-h-0 w-full flex-col overflow-hidden`. **Resolution:** the board's `aside.pane[aria-label="Attention inbox"]` label is **not** used; Phase 2's `Inbox` is pinned by `App.test` and `Inbox.test`.

**Row order.** The rows are shown in the order of `inbox.rows` (the `cpd inbox list` order). The component does not sort, group or rank.

**Glyph of a row** = `inboxDisplay(row.urgency).glyph` (DESIGN 2d), in its tone colour. The urgency colour mapping is the Phase 2 one, without the tier:

| urgency | glyph | tone colour (token, board hex) |
|---|---|---|
| `red` | blocked | `text-status-blocked` `#e17f6e` |
| `yellow` | landing | `text-status-landing` `#c29653` |
| `blue` | focus | `text-status-focus` `#7aa2ce` |
| `magenta` | decision | `text-status-decision` `#ca88a3` |
| `cyan` | active | `text-status-active` `#67ada7` |
| `white` | unread | `glyph-unread` `#d8d2cc` (the board's) |

`row.kind` never changes glyph or colour (DESIGN 2d).

### 2b. DOM outline

`inbox === null` (KEPT; unreachable from `App`, which gates by `live`):
```
aside[aria-label="Inbox"]
├─ div[data-part="inbox-header"]   flex h-9 shrink-0 items-center gap-2 px-[14px]
│   └─ h2   text exactly "Inbox"      text-meta font-semibold uppercase tracking-[.08em] text-muted-foreground
└─ p   text exactly "Loading inbox…"   (U+2026)   px-[14px] text-fg-dim
```

`inbox !== null`:
```
aside[aria-label="Inbox"]
├─ div[data-part="inbox-header"]   flex h-9 shrink-0 items-center gap-2 px-[14px]                    (36px)
│   ├─ h2     "Inbox" + span.sr-only " ({inbox.openCount})"        text-meta font-semibold uppercase tracking-[.08em] text-muted-foreground
│   │         textContent is exactly `Inbox (${inbox.openCount})`     KEPT (Inbox.test 3: "Inbox (2)")
│   └─ span[data-part="total"][aria-hidden="true"]   {rows.length}    font-mono text-meta text-fg-strong
├─ p[data-empty]      only when inbox.rows.length === 0; text exactly "up to date, drink some water"   KEPT hook; px-[14px] py-3 text-muted-foreground
└─ div[data-part="rows"]   flex-1 min-h-0 overflow-y-auto px-2 flex flex-col gap-0.5
    └─ li[data-kind][data-urgency][data-id]   ×rows, in `inbox.rows` order   (2c)
```
- The total is the visible count of the one list (`rows.length`). It is the single count of the Inbox; the per-tier count strip is removed. **Assumption:** the TUI header count (`───<Inbox>───n─`, `inbox.rs:577`) is kept as this total, and only the per-tier counts are removed.
- `div[data-part="rows"]` replaces the Phase 2 `div[data-part="groups"]` (there are no groups).
- The list scrolls (`overflow-y-auto`, `app.css.test` 3). The TUI's `▲ n more` / `▼ n more` cues are Not wired (2h).
- The Phase 2 engine banner, the per-tier groups and the footer are removed.

### 2c. Row

`li[data-kind="{row.kind}"][data-urgency="{row.urgency}"][data-id="{row.id}"]`. `data-kind` and `data-urgency` are KEPT. `data-id` is new. There is no `data-tier` and no `data-focused` (no tiers, no focus model). className: `flex gap-2.5 rounded-[7px] p-2` + `hover:bg-row-hover` (1.2 item 9). The row has no keyboard handler (2e).

```
li[data-kind][data-urgency][data-id]
├─ span[aria-hidden="true"][data-part="glyph"]   className={glyphClass[inboxDisplay(urgency).glyph]} (2a)     mt-1.5
└─ div[data-part="body"]   flex min-w-0 flex-1 flex-col gap-[3px]
    ├─ div[data-part="line1"]   flex w-full items-baseline gap-2
    │   ├─ (link kind with a url, see below) a[href="{row.url}"][data-part="link"]
    │   │     ├─ strong[data-part="subject"]  {first}   KEPT element; font-mono text-key font-normal text-fg-strong underline truncate
    │   │     └─ span[aria-hidden="true"]  link cue (lucide ExternalLinkIcon, size 12)   text-muted-foreground
    │   ├─ (otherwise) strong[data-part="subject"]  {first}   KEPT element; font-mono text-key font-normal text-fg-strong flex-1 min-w-0 truncate
    │   └─ span[data-part="age"]  {age}   font-mono text-meta text-muted-foreground shrink-0
    └─ span[data-part="subtitle"]   only when the label has a non-empty second line   text-muted-foreground text-[12px] truncate
          text: the second line `{rest[0]}`, aligned under the label (indented past the glyph column)
```

- `first` = `row.label.split("\n")[0] ?? ""`; the second line is `row.label.split("\n")[1]`, shown only when non-empty. Labels with more lines show the second line only (TUI).
- **Link kinds.** `row.url !== null` and `row.kind` is one of `plan-review`, `pr-ready`, `pr-closed`, `review-requested`, `deploy` (TUI `is_link`, `inbox.rs:492-494`): the label is the link. Clicking the label opens `row.url` (native anchor; TUI `mod.rs:591-601`). The link cue is the only extra mark. Other rows have no link.
- **Age** `{age}` from `row.ageSeconds` (DESIGN 2c Phase 2 rule, unchanged): `s < 60` → `{s}s`; `s < 3600` → `{floor(s/60)}m`; `s < 86400` → `{floor(s/3600)}h`; else `{floor(s/86400)}d`. The TUI shows `42s`, `5m`, `2d` (`inbox.rs:673-676`); its hour form is **unverified** (`read/age.rs` was not read), so the web keeps `h`.
- Row 1 is cut with an ellipsis before the age (`truncate`); row 2 is cut with an ellipsis as well. Nothing else is cut.

**Left click** (TUI `inbox.rs:289-327`). The click target is the row body (the subject and the subtitle). For a link kind the label is the link, and the rest of the row runs the kind's click:

| `row.kind` | left click |
|---|---|
| `dialog`, `unread`, `loop-guard` | select the Session: `onSelectSession(row.sessionId)` |
| `pr-ready`, `pr-closed`, `plan-review`, `drafts-parked` | select the Plan's Lead: `onSelectSession(row.sessionId)`, the chat target cpd sends for the row (`cockpit.ts` `InboxRow.sessionId`). **Assumption:** the Inbox props have no Plan lookup, so the Lead is the row's `sessionId`; a row without one does nothing (flagged). |
| `review-requested` | copy `row.url` to the clipboard (the TUI's `set-buffer -w`) |
| `engine-down` | start the engine: **Not wired** (2h) |
| any other kind | nothing (the TUI's question, curate-retire and models-changed dialogs are Not wired, 2h) |

A row with a left-click action has the click target as a native `button[type="button"]`, so keyboard Tab reaches it; it carries the KEPT hook `data-action="open-session"` when that action selects a Session. A row with no left-click action (or no `sessionId` for a Session-selecting kind) has a plain `div` body.

### 2d. Right-click menu

Right click on a row opens a shadcn/ui context menu (`context-menu.tsx`, 2f) on the `li`. There is no menu on `engine-down` (TUI `mod.rs:566`). Entries, in this order (TUI `inbox.rs:359-370`, `:420-450`); an entry shows only when its condition holds:

| Entry | Condition (TUI) | Action |
|---|---|---|
| `Go to Lead` | `pr-closed`, `pr-ready` with a Plan | the Plan's Lead, as the left click above (`onSelectSession(row.sessionId)`) |
| `Copy Link` | `row.url !== null` | copy `row.url` to the clipboard |
| `RePost` | `pr-ready`, Plan present, the row says `review overdue` | `pr.post` (`planId`, workflow key) |
| `Merge` | `pr-ready`, the row says `ready to merge` | **Not wired**: there is no merge action in `CockpitAction`; the entry is shown disabled |
| `Post` | `pr-ready`, the row says `ready for user review`, Plan present | `pr.post` (`planId`, workflow key) |
| `Approve` | `deploy`: first in the choices | `inbox.approve` with `row.id` |
| `Claim` | `review-requested`: first in the choices | `inbox.claim` with `row.id` |
| `Snooze 15 min` | every kind | `inbox.snooze`, `duration: "15m"` |
| `Snooze 1 h` | every kind | `inbox.snooze`, `duration: "1h"` |
| `Snooze until tomorrow 09:00` | every kind | `inbox.snooze`, `duration: "tomorrow"` (the 09:00 is the TUI's label; the server's meaning of `tomorrow` is **unverified**) |
| `Dismiss` | every kind | `inbox.dismiss` with `row.id` |

- Choices dispatch through `onAction` with the typed action (the Phase 2 shape). `Approve` and `Claim` show only when the row carries `inbox.approve` / `inbox.claim` in `row.actions`. The three snoozes and `Dismiss` show on every row.
- `Go to Lead`, `Copy Link` and the Merge/Post/RePost conditions are the TUI's. `Post` and `RePost` need the workflow key, which `InboxRow` does not carry (**unverified**): when the key is absent, the entry is shown disabled.
- Entries are plain items; a choice closes the menu.

### 2e. Keyboard and focus

- The Inbox has no keys of its own, as the TUI Inbox has none (`mod.rs:9`). There is no `onKeyDown`, no `j`/`k`, no `Enter` handler and no footer.
- Tab order: the row's native buttons and links in document order. The context menu opens by the platform context-menu key (Shift+F10), as the menu part provides (library behaviour, unverified).

### 2f. States

| State | Rendering |
|---|---|
| `inbox === null` | 2b first outline: `Inbox` heading and `Loading inbox…` |
| rows empty | header (`Inbox`, total `0`), `p[data-empty]` "up to date, drink some water"; no `rows` content |
| engine stopped | no banner. The read model's `engine-down` row, when it sends one, is an ordinary row |
| row with `url` of a link kind | the label is the link (2c) |
| row without `sessionId` | the click target is a `div`; its click does nothing |
| row without actions | the menu shows only the entries whose conditions hold (2d) |
| urgency known, kind unknown | renders by urgency (DESIGN 2d) |

### 2g. Phase 2 tests

| Test (file) | Pins | Status |
|---|---|---|
| `Inbox.test.tsx` "renders inbox rows by kind" | `li[data-kind]` order `["review-requested","deploy"]`; `data-urgency="red"`; `strong` text; `second line`; `a[href]`; `button[data-action="open-session"]` present/absent | **changed**: the kind text (`[data-part="kind"]`) is gone; the order is the `inbox.rows` order, no tier order; the row's second line is the subtitle |
| `Inbox.test.tsx` "row actions call onAction with the typed action" | exact-text buttons `Claim`, `Snooze 15m`; `onAction` called exactly twice | **changed**: there are no inline buttons; the same actions are reached from the right-click menu (`Claim`, `Snooze 15 min`) |
| `Inbox.test.tsx` "shows the open item count" | `aside[aria-label="Inbox"] h2` textContent `Inbox (2)` | KEPT unchanged |
| `PlanView.test.tsx` (all) | Plan view pins (DESIGN-panes 1) | KEPT unchanged |
| `App.test.tsx` 1 | `section[aria-label="Plan pane"] section[aria-label="Plan plan"]`; `aside[aria-label="Inbox"]` contains `up to date, drink some water` | **changed**: the empty copy (was `Nothing needs you`) |
| `AppMemo.test.tsx` | `PlanView`/`Inbox` mocked; render counts 1 | KEPT unchanged (props identity unchanged: `onRetryJob` is not passed) |

Not changed: `data-workflow`, `data-job`, `data-action="post-pr"`, `data-action="open-session"`, `li[data-kind]`, `aside[aria-label="Inbox"]`, `section[aria-label="Plan {id}"]`, `section[aria-label="Workflow {key}"]`, `section[aria-label="Explore jobs"]`, `p[data-empty]`.

### 2h. Not wired (omitted, never faked)

- Engine-down left click (`start the engine`): no `CockpitAction` starts the engine.
- `Merge` and the `Merging` spinner in the glyph cell: no merge action in `CockpitAction`.
- Left click for `question` (opens the Manager), `curate-retire` (checklist dialog) and `models-changed` (text dialog) of the TUI.
- The TUI's `▲ n more` / `▼ n more` scroll cues and the wheel step of three items.
- Key handling of the TUI's Plan and dialog views; the Inbox has no keys (2e).
- Per-row badges (`2 ✕ · 14 ✓ · conflict`, `18 ✓`); the board's subject/sub strings that cpd does not send; `Approve deploy` / `Claim → codereview` as fixed copy (only cpd's `a.label` is used, when a row carries it); `inbox.hidden` counts (claimed, dismissed, snoozed); jump to a row's context/plan (`contextId`, `planId`, `jobKey` unused); the unread tier's `replied` copy; the board's `Snooze S` / `Dismiss E` action bar; the `S`, `E`, `J`, `K` and `Enter` keys.

---

## 3. Test checklist for the writer (derived, no guessing)

Plan View: stepper has 9 `li`, current has `aria-current="step"` and `data-state="current"` (`phase: "dispatch"` → `li[data-phase="dispatch"]`; `phase: "plan"` → the text `planning`); node `li` has inline `left`/`top` and the 148×46 classes; DAG fixtures: (1) chain of 3 → `ncols=3`, `W=3·148+2·28=500`, `H=46`, two straight paths with `d` `M149 23H173M169 19.5L173 23 169 26.5` and `M325 23H349M345 19.5L349 23 345 26.5`; (2) one root with two dependents → `H=100`, root `top` 27, dependents `top` 0 and 54, paths `M149 50C162 50 160 23 173 23` and `M149 50C162 50 160 77 173 77`; (3) two roots into one target (fan-in) → `H=100`, roots `top` 0 and 54, target `top` 27, paths `M149 23C162 23 160 50 173 50` and `M149 77C162 77 160 50 173 50`; `data-state` `done` for a merged source (stroke opacity `.6`), `pending` otherwise (`stroke-dasharray="3 3"`); `Retry ▸` absent without `onRetryJob`, present and calling it with `(jobKey, wf.key)` with it, and never calls `onAction`; `Post PR` dispatches `{ kind: "pr.post", planId, workflow }`; subtitle `failed ×2`, `blocked on 2`; chip summary text; `No workflows yet`.

Inbox: rows are listed in `inbox.rows` order with no groups; total is `rows.length` (4 for the four-row fixture); no `data-tier`, no `data-focused`, no key handler, no footer text; `p[data-empty]` text exact `up to date, drink some water`; left click per kind (2c) calls `onSelectSession` once for a Session-selecting kind with `sessionId` set and nothing with `sessionId: null`; right-click menu entries and their order (2d); age formatting (`38`→`38s`, `240`→`4m`, `3600`→`1h`, `79200`→`22h`, `172800`→`2d`).
