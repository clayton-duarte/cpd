# Planning UX: plannotator, ask_user_question and what a plan outputs

Synthesis for Plan `plan-ux` (Job `synthesis`). Evidence: [planning-ux-tools.md](planning-ux-tools.md) (cited **T §n**) and [planning-ux-usage.md](planning-ux-usage.md) (cited **U §n**, examples **U E<n>**), and Context records read with `cpd context show --record <id>`. "Inferred" marks a conclusion I drew and did not measure; "unverified" marks a claim nobody checked.

## 1. Summary

**Recommendation: option (b).** The User decides the plan's open choices in `ask_user_question` rounds; the plan itself reaches the User as a short summary (skeleton in §3.3) in the Lead's chat, approved in one question that `cpd_plan_review` opens; plannotator stays for long documents only. This needs a new User decision that supersedes hs-d468, a summary render, a chat surface for `cpd_plan_review`, and G1 dropped (G2 kept). It does not change what pnbox builds (§6).

| # | Question | Answer in two lines |
|---|---|---|
| 1 | What does plannotator give that ask_user_question + skills cannot? | A whole document of any size, rendered, with anchored annotations, a revision diff and a non-blocking, recoverable wait. In 82 plan reviews the User typed no annotation and no feedback, so only the non-blocking wait and the rendered page were used. |
| 2 | Does it make the plan better? | No measured effect: 76 of 82 approved, none with feedback the User typed; 51 of 323 approved draft Jobs were cancelled later. What improves plans is cpd's own checks (stub refusal, overlaps), not plannotator. |
| 3 | Verbose, hard to skim and to argue about | Confirmed, and the cause is `cpd plan render`, not plannotator: median 197,861 characters per review, almost all Worker Specs and Locked Jobs. The User has no list of decisions to argue with. |
| 4 | ask_user_question is directed, the User takes part | Confirmed: in 55% of 313 rounds the User picked something other than the first option, typed text or multi-selected; in 31% the User typed text. |
| 5 | Its limits; how many questions | 4 questions per call, 2–4 options each, 16-character header, 60-character label. In cpd the 1500-character total (G2) is the limit that binds. The tool blocks the agent, has no timeout, and a preview shows 16 rows at most. |
| 6 | What a plan defines and outputs | The deliverable, its PR shape, the decisions it needs, its assumptions, its order, how done is shown, the risks and what is out of scope. The User sees a summary of under 40 lines. Specs are for Workers. |

## 2. Answers to questions 1 to 5

### Q1. What does plannotator really give that ask_user_question plus skills could not?

| Capability only plannotator has (T §5) | Used in this Context? | Evidence |
|---|---|---|
| Shows a whole document of any size, rendered with tables and Mermaid | Yes, in every review. Whether anyone read it is doubtful: the 1,000,742-character `cross-repo` r5 was approved 1.1 min after it was requested (inferred: not read in full) | T §2.3, §3.2; U §2, E2 |
| Anchored annotations (comment, deletion), quick labels, reply threads, images | No: 0 of 82 plan reviews carry text the User typed; 1 of 93 plannotator decisions has User annotations, and that was an `annotate` review, not a plan review | U §0, §2 header, E8 |
| Diff against the previous revision, and a version list | Exists only when the first heading and the date match (inferred from code); whether the User ever opened it is unverified | T §2.3 "Diff base" |
| Non-blocking wait, recoverable after `/reload` via `review-status` | Yes: the tool returns at once and the Lead waits for a `cpd-plan-decision` message | T §2.4, §3.1 |
| Unsent annotations kept as a draft across reloads | Not observable in the data | T §2.3 |
| `:::question` blocks inside the plan | Never: cpd emits none | T §4 "Multiple choice" |
| Reachable from another machine's browser | Yes, through the review host and port. It failed once, on a `localhost` URL (hs-d496) | T §2.2; U §6.1 |

What is **not** plannotator's: the recorded approval (`cpd plan decide`, run by the adapter, T §3.4), the refusal of stub drafts (T §3.1 step 3), the Overlaps warning (docs/commands.md §4.5), and the "Deciding elsewhere" path that already accepts a decision from any caller (T §3.4). Any surface that ends in `plan decide` keeps those.

**Answer:** what plannotator gives is a long document rendered in a browser and a wait that does not block the agent. Its review features (annotations, labels, diff) went unused for plan reviews. ask_user_question cannot show a document: its total is capped at 1500 characters (G2) and its preview at 16 rows (T §1.2, §3.3). The chat can show one (inferred).

### Q2. Does plannotator make the plan better in any way?

| Signal | Value | Evidence |
|---|---|---|
| Reviews approved / rejected by the User / undecided | 76 / 1 / 4 of 82 | U §4.1 |
| Reviews with any decision text | 4 of 82. All were written by agents: E1 (two texts, sent to plannotator with `curl` by the Manager), `cockpit-parity` r1 (the Manager) and `dev-speed` r3 (the Lead) | U §2 header, E1, §6.4 |
| The one rejection | Asked for 3 Tier changes, "everything else is approved"; written by an agent | U E1 |
| Approved draft Jobs later cancelled | 51 of 323 (16%), in 15 of 76 reviews | U §4.1 |
| Decision time, median | 3.5 min, for a median plan of 197,861 characters | U §4.1 |
| Time vs size | Plans at or below the median size: median 3.4 min. Plans above it: median 4.0 min | U §4.1 |

**Answer:** no measured improvement. The data cannot say whether the User read a plan (U §0). A larger plan took barely longer to approve, and no review text came from the User. The only change a review caused came from feedback an agent wrote (E1).

### Q3. Too verbose, hard to skim, hard to argue about

| Cause | Measure | Evidence |
|---|---|---|
| The review is the whole Plan, Locked Jobs included | 55 of 82 reviews show Locked Jobs, a median of 15.5 Locked Jobs for each draft. Median size is 342,169 characters with Locked Jobs and 28,357 without | U §4.1 |
| Worker Specs are almost all of it | In `bacon` r1 the PR section, mostly 3 Specs, is 18,675 of 19,020 bytes | T §3.2 |
| The Overlaps table can be enormous | `cross-repo` r5: 499,832 characters of Overlaps | T §3.2 |
| No list of decisions | The render has no Requirements, assumptions or decisions section (docs/commands.md §4.5). A choice hides inside a Spec's prose | T §3.2 |
| Feedback is one text blob | The anchors live only inside that text; cpd keeps `approved` and `feedback` | T §2.4 |
| Decided work was reviewed again | `cross-repo` r10–r14 were batches the User had just decided in chat (hs-d301) | hs-d301 |

**Answer:** the verbosity comes from `cpd plan render` (docs/commands.md §4.5), which writes for Workers. plannotator only displays it. It is hard to argue about because nothing in it is phrased as a choice. Any option that keeps the full render keeps this fault.

### Q4. ask_user_question is more directed: the User takes part

| Signal (all 313 rounds, 28 sessions) | Value | Evidence |
|---|---|---|
| Rounds with a non-first option, a multi-select or typed text | 173 of 313 (55%) | U §4.2 |
| Rounds where the User typed text instead of choosing | 96 (31%); typed answers median 93 characters, up to 1058 | U §4.2 |
| `(Recommended)` option picked / another option / typed or multi | 63% / 17% / 20% of 425 | U §4.2 |
| Answer latency, median | 4.9 min (Manager alone 2.8) | U §4.2 |
| Declined | 1 of 313 | U §6.5 |
| Steering examples | hs-d238 came from typed text (E5); a recommendation was corrected (E6); a bug was reported in an answer (E7 → hs-d496) | U E5–E7 |

**Answer:** confirmed. The User argues in ask_user_question: 37% of the questions that carried a recommendation got another option, typed text or a multi-select, and nearly a third of rounds got typed text. In plan reviews he never typed any. The difference is in the form. A question with options and a recommendation invites a reply; a long document invites a yes (inferred from the two rates above).

### Q5. ask_user_question's limits; how many questions?

| Limit | Value | Evidence |
|---|---|---|
| Questions per call | 1–4 (schema). There is no limit on rounds, but the tool's prompt says not to stack calls | T §1.1, §1.6 |
| Options per question | 2–4, plus the automatic `Type something.` row | T §1.1, §1.3 |
| Header / label | 16 / 60 characters. 5 of 324 calls were refused for a header longer than 16 | T §1.1; U §6.3 |
| Total text | No limit in the tool. cpd's G2 refuses over 1500 characters (any role). Before G2, 24 of 313 rounds were over 2000 | T §3.3; U §4.2 |
| Preview | Single-select only; 16 content rows side by side, 11 stacked; cut, no scrolling (inferred) | T §1.2 |
| Blocking, timeout | Blocks the tool call until the User answers; no timeout. Waits reached 1598.7 min. While it waits, nothing reaches that agent, not even a steered Worker escalation (inferred: steering lands after the current tool call, docs/commands.md:2561) | T §1.5; U §6.5 |
| Esc | Cancels the whole call; the model gets `User declined to answer questions` | T §1.5 |
| G1 | Refuses every ask from a Lead whose Plan has a draft Job. That covers the whole `plan` phase once a Job exists (every Job is a draft there) and any time a stub draft is registered (hs-d239) (inferred from `pi/lib/askguard.ts:40-44`, docs/commands.md:891) | T §3.3 |
| Approval | It has none. Approving a Plan is `cpd_plan_review` + `plan decide` | T §4 |
| Not seen | 3 calls have no result (session ended, unverified); no Inbox item exists beyond `dialog` | U §6.5; T §3.5 |

**Answer:** 4 questions per call, any number of calls. In practice the median is 2 questions and 882 characters per round, and 56 rounds used all 4 (U §4.2). The limits that matter are cpd's 1500 characters, the blocked agent and G1.

## 3. Question 6: what a plan must define and its expected output

### 3.1 The candidates

| Candidate | Must define? Why | User sees? | Form |
|---|---|---|---|
| Deliverable and PR shape (hs-d341, hs-d342) | Yes: each PR has a fixed cost (docs, red tests, open Job, Gate), so the cut is a decision (hs-d342) | Yes | One line per PR: what it delivers, and its template (`docs → tests → impl → open`; docs-only skips tests, hs-d341). A table row when there are 2 or more PRs |
| Requirements | Yes: the Specs inherit them (cpd-lead Clarifying) | Only the goal; the User wrote them | One line (goal). The full text goes in the Specs' `context` |
| Assumptions | Yes: a Worker sees only its Spec, so each accepted assumption is copied into it (cpd-lead Specifying 2) | Yes, every one | Numbered one-liners A1…, then one Accept/Change question (references/questions.md) |
| Decisions needed from the User | Yes. They are the only part the User engages with (Q4), and after hs-d301 they are what a review is for | Yes, first | Decision questions in `ask_user_question` rounds, each with a recommended option. The summary lists the answers as `D1 … → <answer>` |
| Risks | Yes, briefly: 16% of approved draft Jobs were cancelled later (U §4.1) | Top 1–3 | One line each, with what would show it |
| Areas and files (hs-d324) | Yes: the overlap map and stacking depend on `touches` | Area names and overlaps with other Plans only | One line: areas, overlap count. The rows appear only when there is something to settle. Files stay in the Specs and the table's Touches |
| Order and stacking (hs-d324) | Yes: `--needs` between PRs and the stack base decide what lands first | Yes | One line: `a → b; stacked on <PR>` |
| Acceptance | Yes, per Job (Spec) and per PR | PR level only | 1–3 "Done when" lines per PR. Job acceptance stays in the Specs |
| Out of scope | Yes: it stops scope creep in Workers (Spec field) | Only what the User might expect to be in | 1–3 lines. Job-level items stay in the Specs |
| *Tier per Job* (evidence: the only rejection was about Tiers, U E1) | Yes, cpd refuses a draft without one (T §3.1) | Yes | A column in the Jobs table |
| *Already decided* (hs-d301) | Yes, so decided work is not reviewed again | Record ids only | One line: `Decided: hs-…` |
| *Locked Jobs* (U §4.1: median 15.5 for each draft) | No, they are context, not under review | Count only | `N locked, not under review` |
| *Later work registered* (hs-d239) | Yes: every planned batch is a stub key | Keys only | One line: `Later: <pr>/<job keys>` |
| *Visual targets* (hs-n394: rounded caps never reach the agents) | Yes when the work is visual: pin exact rows in a test (hs-d341) | Yes | A file path to the mockup, never glyphs in chat |

### 3.2 What the User does not see

Job Specs (goal, context, touches, acceptance, out of scope), the Jobs' dependency graph, Locked Jobs and per-file overlaps belong to Workers and the Lead. The full render (`run/<plan>/plan-r<N>.md`) is still written and can be opened on demand, so the audit trail stays (T §3.1 step 2).

### 3.3 Expected output: a plan summary (example modelled on this Plan; the D and A lines are illustrative)

```markdown
# plan-ux · Research: planning UX · review r1
Goal: answer the User's six planning-UX questions with evidence (docs only)
Deliverable: PR `research` → docs/research/planning-ux*.md (docs-only: no test Job, hs-d341)

## Decisions (answered in 1 round)
D1 Include an option beyond (a)/(b)? → yes, at least one
D2 Evidence window → building-cpd, up to 2026-10-20T20:08Z

## Assumptions (accepted)
A1 No code, skill, guard or Inbox change in this Plan
A2 pnbox builds hs-d496 whatever this finds

## Jobs (3 under review, 0 locked)
| Job | Does | Tier | Needs |
|---|---|---|---|
| tools | limits of both tools, from source | S2 | – |
| usage | measures real reviews and asks | S3 | – |
| synthesis | answers, options, recommendation | O3 | tools, usage |

Order: tools and usage in parallel → synthesis → open. Stacked on: none.
Areas: docs/research. Overlaps: none.
Decided: hs-d468, hs-d472, hs-d496 (context, not reopened)
Later: none

## Done when
- planning-ux.md answers Q1–Q6, each claim cited; quick check passes

## Risks
- R1 One Context and one User: the rates may not generalise

## Out of scope
- Changing askguard, the skills, the Inbox or plannotator

Approve | Change (say what) | Open full plan (run/plan-ux/plan-r1.md)
```

That is 34 lines and 1,234 characters (`wc -l -m` of the block). For comparison, this Plan's real review was 18,372 characters (U §2).

## 4. Options

### (a) Keep plannotator for plans, as today (hs-d468)

1. The Lead interviews with `ask_user_question` before any draft exists, then drafts Jobs; from then on G1 refuses every ask.
2. `cpd_plan_review` renders the whole Plan and opens it in plannotator. The Lead's tool call returns at once and the Lead is `blocked`; a `plan-review` Inbox item appears (T §3.1, §3.5).
3. The User opens the URL, reads, and approves or requests changes; the adapter runs `plan decide` and messages the Lead (T §3.4).

- **Trade-offs:** fast to decide (median 3.5 min), but the User takes no part (0 typed texts in 82). Not skimmable (median 197,861 characters). Auditable: the review copy, the event and plannotator's history (75 of 77 agree, U §4.1). The agent gets `approved` plus one feedback blob, `answersOnly` dropped (T §2.4).
- **Failure modes:** a review nobody warned the User about (282 min, U §6.1); a port collision, so the review never opened (U §6.2); a superseded review keeps the Lead blocked (U E3); agents can approve with `curl` (U E1); G1 blocks every question during `plan` and whenever a stub draft exists (Q5).
- **Skills:** none. **G1/G2:** unchanged. **pnbox:** as planned, `review-waiting` for every plannotator session (docs/contract.md:451 on pnbox's branch).

### (b) ask_user_question rounds for decisions + a short summary; plannotator only for long documents

1. Clarifying and drafting: every open design choice becomes a question in an `ask_user_question` round (≤4 questions, ≤1500 characters), with a recommended option. Answers and accepted assumptions go into the Specs.
2. Approving: `cpd_plan_review` writes the full render as today, plus the summary (§3.3). The adapter posts the summary in the Lead's chat and opens one approval question: Approve / Change / Open full plan. "Open full plan" opens the full render in plannotator.
3. The answer goes through the existing path: the adapter runs `plan decide`; a Change note becomes the feedback (T §3.4). Drafts cannot run before that (docs/commands.md:891).
4. Long material that cannot be split into rounds (a design over 1500 characters) goes to plannotator and is tracked as an Inbox item by pnbox.

- **Trade-offs:** the User takes part where the evidence shows he does (Q4); a 37-line summary instead of ~200 K characters. Auditable: the review copy and the decision event stay; plannotator's store is used only for long documents. The agent gets structured answers (`details.answers`, notes) instead of a blob (T §1.4). Speed: one dialog in the Lead's pane, reached from the Inbox (Manager rounds median 2.8 min, U §4.2).
- **Failure modes:** the approval dialog blocks the Lead's tool call, so escalations wait (Q5; inferred). A pi `/reload` loses an open dialog, unlike `review-status` (T §1.5, §2.4), so the pending entry must be asked again. An approval in a dialog may become just as quick a yes (unverified). A summary can leave out what a Spec says, so the summary has to be generated by cpd from the Plan, never written by the Lead (DRY).
- **Skills:** cpd-lead Clarifying, Specifying, Approving and "Starting a later batch": decisions as rounds, then `cpd_plan_review`, which now shows the summary in chat; drop "show the batch to the User in chat" (SKILL.md:214). cpd-lead and cpd-manager Rules: plannotator only for long documents, `ask_user_question` allowed while drafts exist. references/questions.md: a "Decision round" call shape, and "never approve a Plan with this tool" kept.
- **G1:** removed. It guarded the surface, not the state: drafts never run without a recorded review (docs/commands.md:891), and the only record of one comes through `cpd_plan_review`. **G2:** kept at 1500; it still keeps documents out of dialogs.
- **pnbox:** builds as planned; plannotator sessions become rarer, not fewer in kind. The `plan-review` item's click already opens the Lead (T §3.5), which now lands on the approval dialog (hs-d306); the item's plannotator link is shown only when a page exists.

### (c) Summary-first plannotator with `:::question` blocks

1. `cpd_plan_review` renders the summary (§3.3) with each open decision as a `:::question` block (`- [ ]` options, `Recommended:` line; `question-block.ts:1-30`). The Specs go in a folded appendix or are left out.
2. plannotator opens it as today; the User answers the blocks inline and approves.
3. cpd keeps `answersOnly` and the answers (today dropped, T §2.4) and passes them to the Lead.

- **Trade-offs:** skimmable, non-blocking, recoverable and diffable, with one surface for plans. Participation is unknown: cpd has never emitted a question block, and the User's attention was in the terminal while a review sat unseen for 282 min (U §6.1). The answers come back inside the feedback text, not as structured data (T §2.4).
- **Failure modes:** all of (a)'s except the size. A decision cannot be asked while drafting (G1 stays), so decisions come in one batch at the end.
- **Skills:** cpd-lead Approving: write decisions as question blocks (a Spec field or a Plan field the render reads). questions.md unchanged. **G1/G2:** unchanged. **pnbox:** unchanged.

### (d) The cockpit's Plan view as the review surface

1. The Plan view shows the drafts' summary rows and adds approve and reject keys; the decision runs `plan decide` ("Deciding elsewhere", T §3.4 already handles it).
2. Decisions are asked as in (a) or (b).

- **Trade-offs:** where the User looks for status (the Plan view already shows a `Plan review r<n> pending` row, T §3.5); no browser. Typed feedback and annotations in a TUI are a large build; no evidence on how much the User uses the Plan view (unverified).
- **Skills:** the Approving wording changes to point at the Plan view. **G1/G2:** as (a) or (b). **pnbox:** unchanged; the `plan-review` item would target the Plan view instead of the URL.

### Comparison

| | (a) today | (b) rounds + summary | (c) summary in plannotator | (d) Plan view |
|---|---|---|---|---|
| User takes part | none measured (0/82 typed) | measured 55% / 31% (Q4) | unknown (never used) | none designed |
| Skimmable | no (median 197,861 chars) | yes (~40 lines) | yes | yes |
| Speed for the User | 3.5 min median, unseen reviews up to 282 min | ~3–5 min per round (U §4.2) | like (a) | unknown |
| What the agent receives | `approved` + blob | structured answers + approve | `approved` + blob with answers | `approved` + short text |
| Auditable | yes | yes (review copy, event) | yes, plus diff | yes |
| Blocks the Lead | no (waits) | yes during each dialog | no | no |
| Unseen-review risk | high (U §6.1) | low: the dialog sits in the Lead's pane, and its Inbox item opens that pane (T §3.5) | high | low |
| Agent can approve in the User's place | yes, `curl` (U E1) | no, the dialog is in the User's pane (inferred) | yes | depends |
| Build | none | summary render, chat review, skills, drop G1 | summary render, question blocks, parser | summary render, TUI review |
| Records to change | none | supersede hs-d468, hs-d472 (1) | amend hs-d468 | amend hs-d468 |

## 5. Recommendation

**Option (b).**

**Why:**
- It moves the User's part to decisions, the one form where he takes part (55% vs 0 of 82; Q2, Q4).
- It removes the cause of verbosity: the User sees a summary generated from the Plan, and the Specs stay with the Workers (Q3).
- It keeps the recorded approval path unchanged (`plan decide` by the adapter, T §3.4).
- It also removes two measured failures, the unseen browser review and the agent-made `curl` decisions (U §6.1, E1).
- In hs-d153's order: the approval surface the User does not miss comes first (work), then the shorter review (fast), then the wording (good).

**Evidence it rests on:** U §4.1 (82 reviews: decision time and size, 0 typed feedback, 16% cancelled after approval); U §4.2 (313 rounds: deviation, typed text, latency); U §6.1 and E1–E3 (failures); T §1–§3 (limits, wiring); hs-d301 (a review is for undecided design); hs-d341, hs-d342, hs-d324 (what a plan must define).

**Risks:**
- It reverses a 2026-10-20 User decision (hs-d468). The User must decide it first; this file records nothing.
- The approval dialog blocks the Lead's tool call, so Worker escalations wait behind it (inferred).
- A pi `/reload` loses the open dialog; the adapter must ask it again.
- Quick approvals may continue in the new form (unverified); only a measurement after the change can say.
- The evidence comes from one Context and one User.

**Follow-up Jobs** (named, not specified; ordered work → fast → good, hs-d153):

| Job | What |
|---|---|
| `plan-summary` | `cpd plan render` writes the summary of §3.3 (drafts only; Locked Jobs as a count; overlaps only when there are any; decisions, assumptions, done when, risks, out of scope); the full render is kept |
| `review-chat` | `cpd_plan_review` posts the summary in chat and opens the Approve / Change / Open full plan dialog; `plan decide` unchanged; ask again after `/reload` |
| `askguard-g1` | Remove G1 and its tests; keep G2 |
| `plan-fields` | Where the decisions, assumptions, risks and done-when lines live so cpd can render them (Plan or PR fields), not Lead prose |
| `skill-plan` | cpd-lead, cpd-manager and references/questions.md wording for (b) |
| `plan-ux-remeasure` | Rerun U §1's `gen.py` after ~20 reviews: typed feedback, deviation, cancellations after approval |

`plan-summary` also serves (a) and (c), so it is the cheap, reversible first step if the User wants to try the shorter review before changing the surface.

## 6. Impact on pnbox

None on its scope. pnbox (hs-d496) makes every plannotator session a cpd Session opens a `review-waiting` Inbox item with a reachable URL, and its guard refuses raw plannotator calls that skip tracking (docs/contract.md:451 on pnbox's branch; Jobs `pnbox-corei`, `pnbox-pi` queued). Under (b) plannotator is still used, for long documents and "Open full plan", so every such session still needs that item. Only the plan-review approval moves to a dialog, whose Inbox path already exists (the `dialog` and `plan-review` items open the Lead, T §3.5). Under (c) and (d) pnbox is likewise unchanged. pnbox should build as planned. One thing it should not do: hard-code that a `plan-review` item always carries a plannotator URL. Under (b) that link is optional.
