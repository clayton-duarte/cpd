# Dango v3 + shadcn/ui: design contract (Plan View and Attention Inbox)

Builds on `web/src/ui/DESIGN.md` (called **DESIGN** below). Reused by reference, not restated: conventions (DESIGN 1: `data-part`/`data-action` hooks, tokens never hex in `.tsx`, the four text sizes `text-meta` 11px / `text-key` 12.5px / `text-body` 13px / `text-title` 15px, `bg-row-hover`), tokens (DESIGN 2b), glyph classes (2c), `web/src/ui/dango-status.ts` (2d: `jobDisplay`, `parsePhase`, `phaseLabel`, `phaseTone`, `inboxDisplay`, `urgencyDisplay`, `glyphClass`, `toneClasses`), `cn` (2e), `Button`/`Badge` (2f). Hex values below are the board's rendered values, written next to the token that carries them; `.tsx` files use the token (an arbitrary opacity such as `bg-status-active/[.14]` is allowed, a hex is not).

Files: `web/src/ui/cockpit/PlanView.tsx`, `web/src/ui/cockpit/Inbox.tsx`, and `web/src/ui/cockpit/ManagerView.tsx` (1j). Imports are relative (A1). All three stay wrapped in `memo()` by `App` (DESIGN 3e); their props change only by the optional additions named here, and `App` passes only `postableWorkflows` to PlanView (1f). Phase 2 tests: `PlanView.test.tsx` (3 tests), `Inbox.test.tsx` (3 tests); no existing test changes (3d below).

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
  /** Workflow keys, PR urls and `#<number>`s of the Plan's Inbox rows that carry a `pr.post` action (1f). Absent: no postable check. */
  postableWorkflows?: ReadonlySet<string>;
}): ReactElement;
```

Module-private helpers (not exported): `Stepper`, `ExploreStrip`, `WorkflowCard`, `Dag`, `layoutDag` (pure, 1e).

### 1b. DOM outline

`plan === null`:

```
CpdFlowCanvas   (cockpit/FlowCanvas.tsx, pro-d68)   root div[data-part="flow-canvas"]   flex flex-col gap-2
├─ Button[data-part="expand-all"]   "Expand all" / "Collapse all" (all step cards open or closed)
└─ div   relative pr-28
    ├─ svg[data-part="dag-edges"][aria-hidden="true"]   the step edges
    └─ ul   one li[data-step] per Plan flowchart step (FLOW_STEPS), a collapsible card, closed at start
```
No `section` element is rendered in that case (KEPT: `querySelector("section")` is null). The Plan pane's content (this outline) is wrapped in a `ScrollArea` by Shell (DESIGN 3c, pro-d69); the outline's own elements do not scroll.

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
├─ section[data-part="review-pr"][aria-label="Review PR"]   only when plan.reviewedPr !== null and plan.workflows is empty (1i)
└─ p[data-part="no-work"]                only when plan.workflows is empty AND there are no explore jobs AND plan.reviewedPr is null; text exactly "No workflows yet"; px-1 text-muted-foreground
```
Order of children: title row, explore strip, workflow cards (in `plan.workflows` order), Review PR card (1i), no-work note.

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

Per `plan.workflows` entry `wf`. Card: `section[data-workflow="{wf.key}"][aria-label="Workflow {wf.key}"][data-status="{wf.status}"]` (KEPT hooks; `data-status` new), className `min-w-0 overflow-hidden rounded-[9px] border border-border bg-card` (pro-d77: the card never widens the Plan pane).

```
section[data-workflow]
├─ header[data-part="wf-header"]   flex h-[34px] flex-wrap items-center gap-2.5 border-b border-border px-3      (34px)
│   ├─ h3                          {wf.key}       KEPT; font-mono text-key font-semibold text-fg-strong (12.5px)
│   ├─ span[data-part="wf-route"]  `{wf.repo} → {wf.base}`  (U+2192)   font-mono text-[11.5px] text-muted-foreground
│   ├─ span[data-part="wf-status"][data-tone]   {wf.status} verbatim   font-mono text-meta rounded-[4px] px-1.5 py-px
│   ├─ span[data-part="wf-failed"]   only when failed > 0; text `{n} failed`   font-mono text-meta text-status-blocked-bright (board #ffa796)
│   ├─ span.flex-1
│   └─ p[data-part="pr"]   only when wf.pr !== null  (1f)
└─ ScrollArea[data-part="dag-body"]   horizontal ScrollBar (the ScrollArea default, 3c); its content is `div.w-fit.p-3` (12px padding) around the canvas   (pro-d69, pro-d77)
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
4. `ncols = max(col) + 1`; `colSize(c)` = jobs in column c; `H = max_c (colSize(c)·46 + (colSize(c) − 1)·8)`; `W = ncols·148 + (ncols − 1)·28`. Empty workflow (`jobKeys` empty): `W = 0`, `H = 0`, and `dag-body` (the ScrollArea) renders a `p[data-part="no-jobs"]` "No jobs" (`text-muted-foreground text-[11.5px]`) instead of the canvas.
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
- `a[href="{pr.url}"][target="_blank"][rel="noopener noreferrer"]`, `inline-flex items-center gap-1 text-status-focus-bright underline-offset-2 hover:underline`: a `span` with the text exactly `` `#${pr.number} ${pr.state ?? ""}`.trim() `` (KEPT; test: contains `#7 open`), then an `ExternalLinkIcon` (`size-3 shrink-0`, `aria-hidden="true"`) (pro-d77).
- `button[type="button"][data-action="post-pr"]` text exactly `Post PR`; **KEPT hook**, KEPT copy. Rendered only when the PR can be posted (pro-d77, `canPost`): `wf.pr.posted !== true` (`plan show`'s `pr.posted`: no `pr_posts` row for the Workflow), `wf.pr.state !== "merged"`, `wf.status !== "merged"`, and, when `postableWorkflows` is passed, the Workflow's key, PR `url` or `#<number>` is in it. `App` passes `postableWorkflows` (the Plan's Inbox rows that carry a `pr.post` action) when the Inbox is loaded and passes nothing otherwise, so with no Inbox the guard is `posted` and merged alone. Click → `onAction({ kind: "pr.post", planId: plan.planId, workflow: wf.key })` (existing action; CLI `pr post --workflow <key> --plan <planId>`). `Button variant="outline" size="xs"` (h-6). Always enabled when rendered.

`checks` and `mergeable` of the PR are not drawn (Not wired).

### 1g. States

| State | Rendering |
|---|---|
| `plan === null` | 1b `CpdFlowCanvas` flowchart (pro-d68). `App` renders PlanView only for a loaded Plan; with no Plan it renders ManagerView (1j), which shows this flowchart only when no Context is selected |
| plan, no workflows, no explore jobs, no `reviewedPr` | title row + `p[data-part="no-work"]` "No workflows yet" |
| plan, `reviewedPr` set, no workflows | title row + the Review PR card (1i); no `no-work` note, whatever the explore jobs |
| plan, workflows only | no explore strip |
| plan, explore only | strip, no cards, no `no-work` |
| workflow with zero jobs | header + `p[data-part="no-jobs"]` "No jobs" |
| job status not in the closed union | `jobDisplay` neutral idle ring, label = raw text, `data-status` = raw |
| long DAG | `dag-body` scrolls horizontally through `ScrollArea` with a horizontal `ScrollBar` (pro-d69); the canvas keeps its fixed `W`×`H` |

No keyboard behaviour: nothing in the Plan View except `Post PR` and `Retry ▸` is interactive; both are native buttons (Tab, Enter, Space).

### 1h. Retry ▸ and Not wired

`Retry ▸`: `button[data-action="retry"][type="button"]`, text exactly `Retry ▸` (U+25B8), `font-mono text-meta font-semibold text-status-blocked-bright` (board `#ffa796`), no border, `hover:underline`, `aria-label="Retry {job.key}"`. It exists **only** when `onRetryJob` is passed and the job is `failed`; click → `onRetryJob(job.key, wf.key)`. **No existing action fits**: `CockpitAction` has `inbox.*`, `pr.post`, `session.retier`, `plan.reject`; `actionArgs` has no `job retry` command (`session.retier` changes a Tier, it does not re-run a job). So Retry is **Not wired**: `App` does not pass `onRetryJob`, the button is not rendered, and nothing is dispatched to `onAction`.

**Not wired (omitted, never faked)**: `Retry ▸` (no action); job and attempt durations (`merged · 1m 02s`, `running · 2m 14s`, `checking · 38s`); the plan's `18m·42s active` timer; plan, workflow and job cost (`$2.140`); the explore chip run time (`1m 48s`); `PR opens at <job>` note; workflow duration; the workflow phase badge (shows `wf.status` instead); PR checks/mergeable; clicking a node or chip (no handler on the board); plan-level counts (`plan.counts`) are not drawn in the Plan View (the Context Tree shows them).

### 1i. Review PR card (pro-d412, assumption A5)

A Plan with `plan.reviewedPr` (the read model's camelCase of the Plan's `reviewed_pr`, contract §2.2 Plan: `{repo, number, url, title, state}`, `state` `open`, `merged`, `closed` or null) and no Workflow (`plan.workflows` empty) renders one card, in the place of the Workflow cards, after the explore strip. It is a read-only card: no button and no handler of its own. The Workflow card's look is reused (its border, its header height and its title text classes, 1e).

```
section[data-part="review-pr"][aria-label="Review PR"]   rounded-[9px] border border-border bg-card
├─ header[data-part="rpr-header"]   flex h-[34px] flex-wrap items-center gap-2.5 border-b border-border px-3   (the Workflow header's look, 1e)
│   ├─ h3                           `Review {name}`, name = the part of repo after "/"   font-mono text-key font-semibold text-fg-strong
│   └─ span[data-part="wf-route"]   {repo}   font-mono text-[11.5px] text-muted-foreground
└─ div[data-part="rpr-body"]        flex flex-col gap-1.5 p-3
    ├─ p[data-part="rpr-title"]     {title}, also its title attribute   font-mono text-[11.5px] text-foreground truncate
    └─ p[data-part="rpr-link"]      flex items-center gap-2 font-mono text-[11.5px]
        └─ a[href="{url}"][target="_blank"][rel="noopener noreferrer"]   text: `#{number}`, then ` {state}` when state is not null, then an `ExternalLinkIcon` (`size-3`, `aria-hidden`)   inline-flex items-center gap-1 text-status-focus-bright underline-offset-2 hover:underline
```

The anchor's text is `#912 open` for state `open` and `#7` for state null, followed by the `ExternalLinkIcon` (pro-d77), which replaces the `󱆭` glyph here; the TUI box in `docs/contract.md` keeps its glyph. Its class is the Workflow card's PR anchor's (`web/src/ui/cockpit/PlanView.tsx`, `WorkflowCard`). `rel` is added: the anchor opens in a new tab, and `noopener noreferrer` is the rule of §2.6 for the Inbox's new-tab links. The TUI box's rows of the same Plan are in `docs/contract.md` §2.6 ("The Review PR box"), the same data and the same cut title.

### 1j. ManagerView (a Manager or Context session selected; pro-d70, pro-d75, pro-d76)

File `web/src/ui/cockpit/ManagerView.tsx`. The Plan pane shows it instead of PlanView when no Plan is loaded for the selection (`visiblePlan` null, DESIGN 5.3). `App` passes the Context that holds the selected session (its Manager, a Lead or a fork) and renders `CpdFlowCanvas` (1b) when there is none. So a selected Manager shows ManagerView, not the flowchart. `App` wraps it in `ErrorBoundary name="ManagerView"` and in `memo()` (DESIGN 3e).

```ts
export interface ManagerViewProps {
  readonly context: ContextNode | null;               // null: <CpdFlowCanvas />, no props
  readonly onSelectPlan: (planId: string) => void;    // a Plan row's click, Clicks below
  readonly onSelectSession: (sessionId: string) => void;
}
```

DOM:

```
section[aria-label="Manager overview {context.id}"][data-manager-view="{context.id}"]   flex min-w-0 flex-col overflow-x-hidden
├─ header[data-part="manager-header"]   sticky top-0 z-10 flex h-11 shrink-0 items-center gap-2.5 border-b border-border bg-background px-4 pr-16
│   ├─ h2[data-part="manager-key"][title={context.title}]   {context.id}   font-mono text-[14px] font-semibold text-fg-strong
│   ├─ span[data-part="manager-counters"]   `{n} active · {n} done · {n} records`   min-w-0 truncate font-mono text-meta text-muted-foreground
│   └─ span.flex-1                                          (no Flowchart button, pro-d76)
└─ div[data-part="manager-body"]   flex min-w-0 flex-col gap-2 px-4 py-2.5
    ├─ Empty[data-empty]   "No plans in this context yet — chat with the Manager below to shape a Plan"   (no unarchived Plan)
    ├─ ol[data-part="active-plans"]   one li[data-manager-plan="{id}"][data-phase] per unarchived Plan that is not `done`; `p` "All current plans are done." when there are none
    └─ Collapsible   only when a `done` Plan exists; closed at start; trigger `Done plans ({n})`; content ol[data-part="done-plans"] (same rows)
```

**Order.** The Plans are sorted by `compareOrderKey` (`state/archiveNav.ts`) on the Lead's dot and `created_at`, the key the tree's Plan rows use (3d).

**Row** (pro-d76; the order is the one of pro-d76, which supersedes pro-d75):

```
div[role="button"][tabIndex="0"]   flex w-full min-w-0 items-center gap-2 overflow-hidden rounded-[6px] border border-border bg-card px-2.5 py-1.5
├─ Dot   the Lead's dot (DESIGN 3d, exported for this view, pro-d76); `stopped` with no Lead
├─ span[data-part="plan-id"]        {plan.id}   shrink-0 font-mono text-[12px] font-semibold text-fg-strong
├─ span[data-part="plan-title"][title={plan.title}]   min-w-0 flex-1 truncate text-[12px] text-muted-foreground   (the one flexible item)
├─ span[data-part="plan-workflows"]   the PR chips (below); absent with no Workflow that is not `cancelled`
├─ Badge[data-part="plan-phase"]     {plan.phase}   h-4 shrink-0 font-mono text-[10px]
├─ Badge[data-part="plan-actor"]     `worker` when a Job is running; else `manager` when the Plan has no Lead, is `done` or is `shaping`; else `lead`
└─ span[data-part="plan-counters"]  planned (draft + queued) · active (running + merging) · failed · merged   h-4 shrink-0 font-mono text-[10px]
```

No horizontal scroll: the row and the list clip (`min-w-0`, `overflow-hidden`), and the Plan pane's ScrollArea constrains the content width (3c, vertical-only pane).

**PR chips** (pro-d75):
- Up to two Workflows: one chip each. With a PR, `a[data-part="wf-pr-chip"][target="_blank"][rel="noopener noreferrer"]` with the text `#{number} {state}`, plus ` · {checks}` when the PR is not merged and checks is neither null nor `none`; `state` is the pull request's state, else the Workflow's status. Without a PR, `span[data-part="wf-chip"]` with `{key} ({status})`. A chip click does not select the Plan.
- More than two Workflows: one `button[data-part="wf-pr-summary"]` with the text `{n} PRs` and a chevron, the trigger of a HoverCard (`data-part="wf-pr-hover-list"`) that lists every chip.
- Tone: `anko` when a check failed or a PR is `conflicting`; `matcha` when all are merged; `mizu` otherwise.

**Collapse** (pro-d76; `app.css`): when `section[aria-label="Plan pane"][data-state="collapsed"]` (the 44px title row only), `[data-part="manager-body"]` and every `[data-slot="scroll-area-scrollbar"]` of the Plan pane are hidden, and the header's bottom border is transparent.

**Clicks** (pro-d70). A Plan row (click, Enter or Space) calls `onSelectPlan(plan.id)`, then `onSelectSession(lead.ref.id)` when the Plan has a Lead (`leadSessionId` not null). The Plan and its Lead are selected; PlanView shows once the Plan is loaded. No back control is built: the Context's Manager row in the tree selects the Manager again, which shows ManagerView.

---

## 2. Attention Inbox

The web right column is the Inbox only. Its rows follow the TUI attention engine (`tui/src/widgets/inbox.rs`) in content and order: one list in `inbox.rows` order (the `cpd inbox list` order), with no tier groups and no engine banner. Since pro-d48..pro-d61 the Inbox is shaped by the approved preview: a tier-filter ToggleGroup in the header (2.4), shadcn `Item` rows with a media column and an actions column (2.5), links, selection and the curate dialog (2.6), and async action buttons (2.7). The right-click menu (2.3) stays.

### 2a. Props

```ts
import type { CockpitAction, InboxModel } from "../../protocol/cockpit.ts";

export function Inbox(props: {
  inbox: InboxModel | null;                       // KEPT
  onAction(action: CockpitAction): string | void; // CHANGED: returns the actionId from connection.action (2.7)
  onSelectSession(sessionId: string): void;       // KEPT
  onSelectPlan?(planId: string): void;            // KEPT when passed: a row with a planId selects the Plan too (2.6)
  pendingActionIds?: readonly string[];           // NEW: store.pendingActionIds (2.7)
  completedActionIds?: readonly string[];         // NEW: store.completedActionIds (2.7)
}): ReactElement;
```

`App` passes `onAction={connection.action}` (which returns the id), `onSelectSession`, `onSelectPlan` and the two id arrays from the store. The inbox panel is the Shell's (`aside[aria-label="Inbox column"]`, DESIGN 3c: 30% at start, 14% to 45%, double-click on `split-right` resets it to 30%). `Inbox` fills it: `aside[aria-label="Inbox"]` `relative flex h-full min-h-0 w-full flex-col overflow-hidden`.

**Urgency colour.** Each row's urgency maps to one text token, used by the row icon and the title:

| urgency | title and icon class | tier-filter text (2.4) |
|---|---|---|
| `red` | `text-status-blocked` | `blocker`: `text-status-blocked-bright` |
| `yellow` | `text-status-landing` | `delivery`: `text-status-landing-bright` |
| `magenta` | `text-status-decision` | `decision`: `text-status-decision-bright` |
| `blue` | `text-status-focus` | `support`: `text-status-focus-bright` |
| `cyan` | `text-status-active` | `notice`: `text-status-active-bright` |
| `white` | `text-muted-foreground` | `unread`: `text-fg-strong` |

### 2b. DOM outline

`inbox === null` (before the first snapshot; `App` gates the panes by `live`, so this is seen only in isolation):

```
aside[aria-label="Inbox"]
├─ div[data-part="inbox-header"]   flex h-8 shrink-0 items-center justify-between gap-8 px-2.5
│   └─ h2   text exactly "Inbox"    text-meta font-semibold uppercase tracking-[.08em] text-muted-foreground
└─ p   text exactly "Loading inbox…"   px-[14px] text-fg-dim
```

`inbox !== null`:

```
aside[aria-label="Inbox"]
├─ div[aria-hidden="true"]   the measuring copy (2.4.2): invisible absolute -z-10 flex h-8 w-max items-center gap-8 px-2.5
├─ div[data-part="inbox-header"]   flex h-8 shrink-0 items-center justify-between gap-8 px-2.5
│   ├─ h2   "Inbox"        shrink-0; text-meta font-semibold uppercase tracking-[.08em] text-muted-foreground
│   └─ the tier filter (2.4): ToggleGroup[data-part="inbox-filters"] (ml-auto shrink-0), or the collapsed trigger (2.4.2)
├─ p[data-empty]     only when the filtered list is empty; text exactly "up to date, drink some water"; px-[14px] py-3 text-muted-foreground
└─ ScrollArea   min-h-0 flex-1 (pro-d69; the list scrolls here, the aside clips)     (only when filtered rows exist)
    └─ ItemGroup   gap-2 px-2.5 py-1.5
        └─ InboxItemRow ×filtered rows, in `inbox.rows` order (2.5)
Dialog (2.6): the curate checklist, closed unless a curate-retire row was clicked.
```

- The Phase 2 `div[data-part="rows"]`, the `total` span and the `Inbox (n)` textContent are gone. The header carries no count beside the title; the counts are on the filter items (2.4).
- The list scrolls through `ScrollArea` (pro-d69); the aside clips (`overflow: hidden`). The TUI's `▲ n more` / `▼ n more` cues are not built (2.9).

### 2.3. Right-click menu

Every row except `engine-down` is wrapped in a shadcn `ContextMenu` (`context-menu.tsx`) with `ContextMenuTrigger asChild` around the `Item`. Entries, in this order; an entry shows only when its condition holds. Choices dispatch through `onAction`, and a choice closes the menu.

| Entry | Condition | Action |
|---|---|---|
| the primary action label (`Merge`, `Post`, `RePost`) | `pr-ready` with a primary action (2.5) | that action |
| `Go to Lead` | `pr-ready`, `pr-closed` with `sessionId` | select the row's Plan and Session (2.6) |
| `Copy Link` | `pr-ready`, `pr-closed` with `url` | copy `row.url` to the clipboard |
| (separator) | when any entry above is shown | |
| `Approve` | `deploy` | `inbox.approve` with `row.id` |
| `Claim` | `review-requested` | `inbox.claim` with `row.id` |
| `Open Link`, `Copy Link` | `dialog` with `url` (a review Plan's reviewed PR; `docs/architecture/attention-inbox.md`) | `Open Link` opens `row.url` in a new tab (`noopener,noreferrer`); `Copy Link` copies `row.url` to the clipboard, as the `pr-ready` and `pr-closed` entry above does. A `dialog` row's click stays the Go to Lead of 2.6 |
| `Snooze 15 min`, `Snooze 1 h`, `Snooze until tomorrow 09:00` | every kind | `inbox.snooze` with `duration` `15m`, `1h`, `tomorrow` |
| `Dismiss` | every kind | `inbox.dismiss` with `row.id` |

Snoozes and `Dismiss` go straight through `onAction` (they are not tracked in `pendingActionIds`; 2.7 covers the row buttons only).

### 2.4. Tier filter (pro-d49, pro-d50)

**Header control.** `ToggleGroup[type="multiple"][variant="outline"][size="sm"][data-part="inbox-filters"][aria-label="Filter inbox by tier"]`, right-aligned (`ml-auto shrink-0`), in the header row after the `Inbox` h2. The header is `justify-between` with `gap-8`, so the title and the filters are at least `gap-8` (32px) apart. Seven `ToggleGroupItem`s in this order, each `h-6 gap-1.5 px-1.5 text-xs`:

| Order | `value` | `data-tier-filter` | label | count | text class |
|---|---|---|---|---|---|
| 1 | `all` | `all` | `all` | `rows.length` | `text-fg-strong` |
| 2 | `red` | `blocker` | `blocker` | rows with `urgency === "red"` | `text-status-blocked-bright` |
| 3 | `yellow` | `delivery` | `delivery` | `yellow` | `text-status-landing-bright` |
| 4 | `magenta` | `decision` | `decision` | `magenta` | `text-status-decision-bright` |
| 5 | `blue` | `support` | `support` | `blue` | `text-status-focus-bright` |
| 6 | `cyan` | `notice` | `notice` | `cyan` | `text-status-active-bright` |
| 7 | `white` | `unread` | `unread` | `white` | `text-fg-strong` |

- Each item reads `<label> <count>`: the label in a `span`, the count in a `span.font-mono.text-[10px].opacity-80`. Counts are over **all** rows, not the filtered ones. No item is ever `disabled`, including at 0.
- The toggle's `value` list is `["all", "red", …, "white"]` while `all` is selected, else the selected urgencies.
- **Selection rules** (state: `"all"` or a set of urgencies; initial `"all"`):
  - click `all` → `"all"`;
  - click a tier while `"all"` → the set holds only that tier (**isolate**);
  - click a tier while a set is active → toggle that tier; **additive**: later clicks add or remove tiers;
  - a set that becomes empty (last tier cleared) or holds all six tiers → `"all"`.
- The list shows the rows whose urgency is in the set (every row when `"all"`).

**Collapse into a Popover (pro-d50).** The header measures itself with a `ResizeObserver` on the header and on the measuring copy (`measureRef`, the same items in a `w-max` row with `gap-8`). When the header's `clientWidth` is less than the copy's `scrollWidth`, the filters collapse:

- The `ToggleGroup` is replaced by `Popover` > `PopoverTrigger asChild` > `Button[type="button"][variant="outline"][size="sm"][data-part="inbox-filters-trigger"][aria-label="Filter inbox by tier"]` (`ml-auto h-6 shrink-0`, `ListFilterIcon` size 3.5, the label `all` or `N tier` / `N tiers`, and the count `filteredRows.length`).
- The `PopoverContent` (`align="end"`, `w-auto min-w-[8.5rem] p-1.5`) holds a `ToggleGroup` with the same `type`, `variant`, `size`, `value`, `aria-label`, `data-part="inbox-filters"` and the same seven items, with `orientation="vertical"` and `w-full`.
- The measure runs on mount and whenever the tier counts or the row count change. The measuring copy shows the same labels and counts, in plain spans.

### 2.5. Row (`Item`)

Each row is a shadcn `Item` (`components/item.tsx`) with `variant="outline"` `size="sm"`, `role="button"`, `tabIndex={0}`, `data-kind`, `data-urgency`, `data-id`, `className="cursor-pointer hover:bg-accent/50"`, inside an `ItemGroup`. Three columns, full height:

```
Item[role=button][variant=outline][size=sm]   (the preview renders a role=button, not an <a>; see the note below)
├─ ItemMedia  flex-col items-center justify-between gap-1 self-stretch          (column 1: media)
│   ├─ div.flex.h-[19.25px].items-center.justify-center → RowIcon   lucide, size-4 shrink-0, urgency text colour
│   │     deploy Rocket · pr-ready, pr-closed GitPullRequest · review-requested GitPullRequestArrow · curate-retire Sparkles
│   │     else by urgency: red CircleAlert · yellow CircleDot · magenta CircleHelp · blue Eye · cyan Bell · white MessageSquare
│   └─ span.mt-auto.block.text-sm.leading-normal → span[data-part="age"]   font-mono text-[10px] text-muted-foreground
├─ ItemContent  justify-between self-stretch                                    (column 2: text)
│   ├─ ItemTitle  className = urgency text colour
│   │     first label line; when the row is linked (2.6): a[href=row.url][target=_blank][rel="noopener noreferrer"] hover:underline
│   └─ ItemDescription   only when the rest of the label is non-empty (the rest joined by a space)
└─ ItemActions  flex-col items-end justify-between gap-1.5 self-stretch         (column 3: actions)
    ├─ linked row: a[data-part="item-link"][aria-label="Open external link"][href=row.url]  ExternalLinkIcon size-4 mt-[1.5px]
    │   else: ChevronRightIcon size-4 mt-[1.5px] text-muted-foreground
    └─ primary action or fallback (2.7): Button[type="button"][variant="outline"][size="sm"][data-part="item-action"] h-6 gap-1 px-2 text-xs
```

- **`Item` root.** The preview renders `role="button"` on the `Item` root, with the click, `Enter` and `Space` handled on the root. pro-d51 names `asChild <a>` for the root; the approved preview does not do that, so this contract follows the preview (flagged in the consolidate report).
- The age is `age(row.ageSeconds)`: `s < 60` → `{s}s`; `s < 3600` → `{floor(s/60)}m`; `s < 86400` → `{floor(s/3600)}h`; else `{floor(s/86400)}d`.
- Media and actions are full-height vertical columns (pro-d52): the icon sits at the top of column 1 and the age at the bottom, on the last text line's baseline (`mt-auto` + `leading-normal`); the external link or chevron sits at the top of column 3 and the primary button at the bottom.
- `first` = `row.label.split("\n")[0]`; the description is the rest, joined by a space, trimmed, shown only when non-empty.
- The row's `Item` is wrapped in the context menu (2.3), except `engine-down`, which has no menu.

**Primary action** (`primaryActionOf`): the first `row.actions` entry whose kind is `pr.merge`, `pr.post`, `pr.repost`, `inbox.approve`, `inbox.claim` or `engine.start`. Without one, the UI derives it, in this order: `engine-down` → `Start` (`engine.start`); `deploy` → `Approve` (`inbox.approve`, `itemId: row.id`); `review-requested` → `Claim` (`inbox.claim`); `pr-ready` whose label's status reads `ready to merge` → `Merge` (`pr.merge` with `planId`, `workflow`, `pull`), `ready for user review` → `Post` (`pr.post`), `review overdue` → `RePost` (`pr.repost`), each when the Plan or pull is known. When there is no primary action: a `curate-retire` row with a `contextId` shows `Curate` (opens 2.6); any other row with `id >= 0` shows `Dismiss` (`inbox.dismiss`); otherwise no button.

### 2.6. Selection, links and the curate dialog (pro-d53)

**Clicking the row** (`jump`), by kind:

| `row.kind` | click |
|---|---|
| `engine-down` | `engine.start` (the engine starts; the row's `Start` button does the same) |
| `curate-retire` | select the row's Plan and Session (below), then open the curate dialog (below) |
| `review-requested` | copy `row.url` to the clipboard and open it in a new tab (`noopener,noreferrer`); nothing when there is no url |
| `deploy` | open `row.url` in a new tab; nothing when there is no url |
| `dialog`, `unread`, `loop-guard`, `question`, `plan-review`, `drafts-parked`, `pr-ready`, `pr-closed`, with a `sessionId` | select the row's Plan and Session (below) |
| any other kind with a `url` | open `row.url` in a new tab |

- **Select both.** `selectRowSession`: `onSelectPlan(row.planId)` when `planId !== null`, then `onSelectSession(row.sessionId)` when `sessionId !== null`. The read model resolves a missing `sessionId` (`readmodel.ts`, `resolveSessionId`): the row's own session; else the Plan's `lead_session`; else the live Lead of the Plan (`plan_lead`, not archived); else, for `question` and `curate-retire` rows, the live Manager of the Context (role `manager`, no Plan). Null when none of these exists, and the click then selects nothing.
- **Links.** A row is linked when `row.url !== null` and its kind is one of `plan-review`, `pr-ready`, `pr-closed`, `review-requested`, `deploy`. The linked title `a` and the item-link icon `a` open `row.url` in a new tab and stop the click from reaching the row (`stopPropagation`), so they do not also select.
- **Curate dialog.** `Dialog` (`dialog.tsx`), title `Retire records of <contextId>` (or `Retire records` without a contextId), description `Checked records will be retired; unchecked records will be kept.` Opening it checks every record. Each record is a `label` with a checkbox, the record id and `— <detail>`, and its `text`. With no records: `No pending records to retire.` Footer: `Cancel` (closes) and `Confirm`. The checklist scrolls through `ScrollArea` (pro-d69); the dialog's own content scrolls through `ScrollArea` too (`components/dialog.tsx`, pro-d72).
- **Confirm**: with a `contextId`, `approve` = the checked record ids joined by `,`, `reject` = the unchecked ids joined by `,`; when either is non-empty it dispatches `context.retire {contextId, approve, reject}`; when both are empty and `row.id >= 0` it dispatches `inbox.dismiss`. Then it closes. Confirm is not tracked in `pendingActionIds`.
- **Records.** `InboxRow.curation` holds the records of `cpd context curation --context=<id> --limit=10` (CONTRACT 5.3): `{ id, text, detail }`, `detail` = `reason` and `target` joined by a space.

### 2.7. Async action buttons (pro-d53)

The row buttons are `Approve`, `Claim`, `Merge`, `Post`, `RePost`, `Start` (the primary action) and `Dismiss` (the fallback). Each one is tracked:

1. `connection.action(action)` mints `actionId`, dispatches `actionStart` (adds the id to `store.pendingActionIds`) and returns it. The row stores it as `activeActionId`.
2. While `activeActionId` is in `pendingActionIds`, the button is `disabled` and its label is preceded by `LoaderCircleIcon` (`size-3 animate-spin`). A click while pending does nothing.
3. `action.result` with `ok: true` removes the id from `pendingActionIds` and adds it to `completedActionIds`. While `activeActionId` is in `completedActionIds` the button is **not rendered** (it disappears).
4. `action.result` with `ok: false` removes the id from `pendingActionIds` and sets `store.lastError` to the wire error. The button is enabled again, and the error shows as a dismissible destructive `Alert` (4.8).

The ids live in the store (CONTRACT 6.2): `pendingActionIds: readonly string[]` and `completedActionIds: readonly string[]`, both `[]` at start.

`Merge` is also optimistic: its Inbox row is removed at the click and restored on `ok: false`, unless the Inbox has changed since (DESIGN.md, "After an action").

### 2.8. Empty and loading

| State | Rendering |
|---|---|
| `inbox === null` | the 2b first outline: `Inbox` heading and `Loading inbox…` |
| no rows in the snapshot | the header (the filters at `all 0`), `p[data-empty]` `up to date, drink some water`; no list |
| rows exist, the filter selects none | the same `p[data-empty]` copy; the header keeps its filters |
| engine stopped | no banner. The read model's `engine-down` row, when it sends one, is an ordinary row with `Start` |

### 2.9. Not built (Inbox)

- The TUI's `▲ n more` / `▼ n more` scroll cues and the wheel step of three items.
- Keyboard shortcuts of the TUI Inbox. The Inbox has no keys beyond `Enter` / `Space` on a focused row.
- The `inbox.hidden` counts, per-row badges (`2 ✕ · 14 ✓ · conflict`) and the TUI's kind text.
- Jump to a row's Context (`contextId`) and job (`jobKey`).
- Engine-down's button: built (2.5 `Start`). `Merging` spinner in the glyph cell: not built (`merging` is a Plan count, DESIGN-panes 4.x).

### 2.10. Phase 2 and pro-d48..pro-d61 Inbox tests

| Test (file) | Pins | Status |
|---|---|---|
| `Inbox.test.tsx` rows in `inbox.rows` order | `[data-slot="item"][data-kind]` in the order of the fixture; no `data-tier`; `[data-part="kind"]` absent | **changed** (the Phase 2 `li[data-kind]` is now the `Item` root) |
| `Inbox.test.tsx` filter items | seven `[data-tier-filter]` in the order of 2.4, labels `all blocker delivery decision support notice unread`, `<label> <count>` text, counts over all rows, none `disabled` | **new** |
| `Inbox.test.tsx` selection rules | the 2.4 rules, with the `value` list checked after each click | **new** |
| `Inbox.test.tsx` collapse | with `ResizeObserver` and `clientWidth`/`scrollWidth` mocked (external), the trigger `[data-part="inbox-filters-trigger"]` appears and its popover holds a vertical ToggleGroup | **new** |
| `Inbox.test.tsx` item layout | `ItemMedia` holds the icon and `[data-part="age"]`; `ItemActions` holds the link or chevron and `[data-part="item-action"]` | **new** |
| `Inbox.test.tsx` selection | a click on a row with a `planId` and a `sessionId` calls `onSelectPlan` then `onSelectSession`; a `curate-retire` click opens `Retire records of <contextId>` | **new** |
| `Inbox.test.tsx` async actions | the button is disabled with `animate-spin` while pending, disappears when completed, comes back enabled on `ok:false` | **new** |
| `Inbox.test.tsx` "row actions call onAction with the typed action" | Phase 2 inline buttons | **changed**: the actions are the row's primary button and the right-click menu |
| `Inbox.test.tsx` "shows the open item count" (`Inbox (2)`) | Phase 2 | **removed** (no count beside the title) |
| `App.test.tsx` 1 | `aside[aria-label="Inbox"]` contains `up to date, drink some water` | KEPT |
| `AppMemo.test.tsx` | `Inbox` mocked; render counts 1 | KEPT |

Not changed: `data-kind`, `data-urgency`, `data-id` on the item root; `aside[aria-label="Inbox"]`; `p[data-empty]`.

---

## 3. Test checklist for the writer (derived, no guessing)

Plan View: stepper has 9 `li`, current has `aria-current="step"` and `data-state="current"` (`phase: "dispatch"` → `li[data-phase="dispatch"]`; `phase: "plan"` → the text `planning`); node `li` has inline `left`/`top` and the 148×46 classes; DAG fixtures: (1) chain of 3 → `ncols=3`, `W=3·148+2·28=500`, `H=46`, two straight paths with `d` `M149 23H173M169 19.5L173 23 169 26.5` and `M325 23H349M345 19.5L349 23 345 26.5`; (2) one root with two dependents → `H=100`, root `top` 27, dependents `top` 0 and 54, paths `M149 50C162 50 160 23 173 23` and `M149 50C162 50 160 77 173 77`; (3) two roots into one target (fan-in) → `H=100`, roots `top` 0 and 54, target `top` 27, paths `M149 23C162 23 160 50 173 50` and `M149 77C162 77 160 50 173 50`; `data-state` `done` for a merged source (stroke opacity `.6`), `pending` otherwise (`stroke-dasharray="3 3"`); `Retry ▸` absent without `onRetryJob`, present and calling it with `(jobKey, wf.key)` with it, and never calls `onAction`; `Post PR` dispatches `{ kind: "pr.post", planId, workflow }`; subtitle `failed ×2`, `blocked on 2`; chip summary text; `No workflows yet`.

Inbox: rows are listed in `inbox.rows` order with no groups; the `all` filter count is `rows.length` (4 for the four-row fixture) and each tier count is over all rows; the seven filter items in the 2.4 order with `<label> <count>` text and none disabled; the 2.4 selection rules (isolate, additive, reset at empty or all six); the popover collapse appears when `clientWidth < scrollWidth` (mocked); no key handler and no footer text; `p[data-empty]` text exact `up to date, drink some water`; the click per kind (2.6) calls `onSelectPlan` then `onSelectSession` for a row with both ids, `onSelectSession` only when `planId` is null, and nothing for a row with `sessionId: null`; the async button states (2.7); right-click menu entries and their order (2.3); age formatting (`38`→`38s`, `240`→`4m`, `3600`→`1h`, `79200`→`22h`, `172800`→`2d`).

## 4. Cockpit amendments pro-d48..pro-d61 (consolidate-docs)

The contract of each item below is stated in full in the section it names. This section is the index for the cockpit decisions and the one place the Header's breadcrumb is specified.

| Decision | Contract | Section |
|---|---|---|
| pro-d48 fonts | Roboto and Roboto Mono bundled; `--font-sans`, `--font-mono`, `--font-display` and `--font-heading` | DESIGN.md 2b (already on main) |
| pro-d49 tier filter | seven-item ToggleGroup, right-aligned, selection rules | 2.4 |
| pro-d50 layout | `gap-8` title to filters; popover collapse; defaults 20 / 50 / 30; double-click resets | 2.4, 4.2 |
| pro-d51 Inbox rows | `Item` rows in an `ItemGroup`, icon / text / actions | 2.5 |
| pro-d52 columns | full-height media and actions columns; age and primary button at the bottom | 2.5 |
| pro-d53 links, selection, actions | link opens `row.url`; click selects Plan and Session; curate dialog; engine start; async action buttons; error Alert | 2.6, 2.7, 4.8 |
| pro-d60 breadcrumb | Header `Breadcrumb` crumbs `<host>/<group>/<manager>/<lead>/<worker>` | 4.1 |

### 4.1 Header breadcrumb (pro-d60)

`App.tsx` builds the crumbs (`useMemo`) and passes them to `Header` as `crumbs` (DESIGN.md 3a). Each crumb is `{ part, label, onClick? }`, `part` one of `host`, `group`, `manager`, `lead`, `worker`. The crumbs are a shadcn `Breadcrumb` (2f) in the header, `aria-label="Location"`, `data-part="breadcrumb"`, `font-mono text-xs`, separated by `/`.

**Crumbs and their rules**, in order, for the active session (`session.sessionId`, else `selectedSessionId`) found in the tree:

| Crumb | Shown when | `label` | `data-part` | Click |
|---|---|---|---|---|
| `host` | the tree has a `hostname`, or an active Context exists | `tree.hostname` (the machine hostname, `.local` removed), else `host` | `breadcrumb-host` | selects the host Manager: the non-archived `manager` session of Context `host`, else its first session (absent: no click) |
| `group` | an active Context exists | `standing` for Contexts `cpd`, `host`, `codereview`; `archive` when the Context, the Plan or the session is archived; else `contexts` | `breadcrumb-group` | none |
| `manager` | an active Context exists | the Context id | `breadcrumb-context` | selects the Context's non-archived `manager` session (else any `manager`) |
| `lead` | the active session belongs to a Plan | the Plan id | `breadcrumb-plan` | selects the Plan and its Lead session (`onSelectPlan`, then `onSelectSession(leadSessionId)` when set) |
| `worker` | the active session is a Worker of the Plan | the Worker's `jobKey`, else its key | `breadcrumb-worker` | selects the Worker's session |
| `lead` (fork) | the active session is a fork of the Plan (not its Lead) | `<planId>·<first 4 characters of the session id>` | `breadcrumb-plan` | selects the fork |
| `lead` (no Plan) | the active session has role `lead` and no Plan | its key | `breadcrumb-plan` | selects the session |

- **Before the first tree snapshot**, with only `cockpit.plan` known: two crumbs, `manager` = `plan.contextId` and `lead` = `plan.planId`, no host and no group.
- **Worker crumb:** the crumb exists and selects the Worker's session, but there is no Worker view of its own yet (the Worker's chat is the view). Stated here because pro-d60 says "prepared but no worker view yet".
- The last crumb is the current page: `BreadcrumbPage` with `aria-current="page"`, `font-medium text-fg-strong`. Clickable crumbs are `BreadcrumbLink render={<button type="button" />}`; a crumb with no click is a `span`. Both carry the `data-part` above.
- `Header` with no `crumbs` and a `location` (DESIGN 3a) renders the Phase 2 two-crumb form through the same parts.
- The ChatHeader carries no breadcrumb (DESIGN-chat 1).

### 4.2 Shell columns and double-click resets (pro-d50)

The defaults (tree 20%, centre 50%, Inbox 30%), the double-click on `split-left` (`treeRef` → 20%) and on `split-right` (`inboxRef` → 30%), and the `overflow-hidden` tree aside are in DESIGN.md 3c. `Reset layout` (the Status Bar button, DESIGN.md 3g) restores the same three values and the plan 40% / chat 60% split.

### 4.3 Sidebar ContextTree

pro-d48..pro-d61 name no tree change. The tree is DESIGN.md 3d and 3g as they stand, with one later change, pro-d78 (transient, not persisted): when a Manager session is selected, the tree auto-collapses that Context's live Plans and forks (the Context row's `Collapsible`; the Archive is not affected). The User may toggle it by hand until the selection changes. Selecting any other agent expands it again.

### 4.4 Fonts and tokens (pro-d48)

Roboto and Roboto Mono, with `--font-sans`, `--font-mono`, `--font-display` and `--font-heading` as in DESIGN.md 2b. The φ mark, the Header's φ and the fold markers use `font-display`, which is the sans family.

### 4.5 Inbox (pro-d49 to pro-d53)

Section 2 of this file, in full (props 2a, tier filter 2.4, rows 2.5, links and curate 2.6, async actions 2.7).

### 4.8 Cockpit error Alert (pro-d53)

When an action fails (`action.result` `ok: false`), the store's `lastError` is set (CONTRACT 6.2) and the app shows one error toast:

```
div[data-part="cockpit-error-toast"]   fixed right-4 bottom-8 z-50 w-full max-w-md shadow-lg
└─ Alert[variant="destructive"][class pr-9]
    ├─ CircleAlertIcon size-4
    ├─ AlertTitle         = the error's code
    ├─ AlertDescription   = the error's message
    └─ Button[type="button"][variant="ghost"][size="icon-sm"][aria-label="Dismiss error"]   absolute top-2 right-2; XIcon size-3.5
```

The toast shows `lastError ?? cockpit.error`. Its Dismiss button dispatches `dismissError`, which clears both. The toast is dismissible and nothing else closes it.

---
