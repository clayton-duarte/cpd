# t4: Can `cpd` drive plannotator for plan approval?

Tier: S3 · Report: `~/projects/cpd/docs/trials/t4-plan-approval.md` · Read `_common.md` first.

## Goal

Establish the exact, tested integration for this flow:
1. The Lead renders the Plan as Markdown.
2. The User reviews it in plannotator and approves or denies it, with annotations.
3. The decision comes back to the Lead.

The flow must not use plannotator's plan mode, and must not change the Lead's model, thinking level or tools.

## Background (from research, verify everything)

- Package `@plannotator/pi-extension` (repo `backnotprop/plannotator`, clone at `/tmp/cpd-research/repos/backnotprop__plannotator`; extension in `apps/pi-extension/`).
- Shared event API: channel `plannotator:request` with action `plan-review` and payload `{planContent}`. The decision arrives on `plannotator:review-result` (see `apps/pi-extension/plannotator-events.ts` around lines 83–85 and 443–451, and the README "Shared Plannotator event API"). Decisions are also delivered as a follow-up user message via `pi.sendUserMessage(…, {deliverAs: "followUp"})` (around `index.ts:2038`) and persisted in `~/.pi/plannotator-review-status.json`.
- Plan mode calls `pi.setModel`, `pi.setThinkingLevel` and `pi.setActiveTools` on phase changes (`index.ts` around lines 684, 696, 746–759). It must stay off.

## Constraints

- **Do not open a browser window on the User's screen.** Find in the source how plannotator decides to open a browser (e.g. `PLANNOTATOR_REMOTE`, a port setting, a no-open flag) and run it so it only serves the review page. Then drive the decision through the same local HTTP endpoints the browser UI uses (`curl`), as the browser would. If there is no way to avoid opening a browser, stop this part and record an Escalation.
- plannotator writes state under `~/.pi/` (outside `~/.pi/agent`). That is allowed for this trial only. **List every file it created or changed under `~/.pi/` and remove or restore them at the end.** Do not touch `~/.pi/agent`.
- Load packages per run only: Lead pi started in a `cpd-trial` pane with `-e npm:@plannotator/pi-extension@<pinned version>` plus your own test extension `-e /tmp/cpd-trials/t4/cpd-plan-ext.ts`. Record the version.
- Lead model: `github-copilot/claude-haiku-4.5:low`.

## Test extension you write (`/tmp/cpd-trials/t4/cpd-plan-ext.ts`)

A minimal pi extension standing in for `cpd`'s pi adapter:
- registers a command `/cpd-plan-review <path-to-plan.md>` that emits `plannotator:request` with action `plan-review` and the file content;
- listens on `plannotator:review-result` (and any other result channel you find) and appends every event payload to `/tmp/cpd-trials/t4/events.jsonl`.

Use `pi.events`. Model it on pi's examples (`examples/extensions/event-bus.ts`).

## Acceptance criteria

1. **must** Approve path: the request is emitted, the review page is served (URL captured), approval is simulated over HTTP, and the result event is received (payload in `events.jsonl`). Show whether a follow-up user message also entered the Lead's session (JSONL excerpt).
2. **must** Deny path with an annotation or feedback text: the same evidence, plus the feedback text reaching the Lead.
3. **must** No side effects: the Lead's model, thinking level and active tools are unchanged before and after (session JSONL entries, or a tool listing). Plan mode never engaged.
4. **should** Recovery: start a review, run `/reload` in the Lead before deciding, then decide. Show whether the result still arrives.
5. **should** Rendering: a 10-Job plan with a Dependency table and Mermaid graph. Note any Markdown features that don't render (inspect the served HTML/JSON if you can't view it).
6. **should** Timing: wall time from request to served page, and from decision to result event.
7. **should** Concurrency: what happens if a second plan-review is requested while one is open.

## Questions the report must answer

- The minimal integration surface `cpd`'s pi adapter needs (events, payload shapes, result shape).
- How `cpd` (outside pi) learns the decision: a file, a session entry, or an event forwarded by the adapter.
- Whether plannotator is safe to load alongside a Lead that must keep its Tier (any hook that changes model, tools or prompts even with plan mode off).
- Version pin recommendation and update risk.
