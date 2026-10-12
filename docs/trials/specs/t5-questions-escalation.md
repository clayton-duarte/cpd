# t5: Structured questions to the User, and Worker → Lead Escalation

Tier: S3 · Report: `~/projects/cpd/docs/trials/t5-questions-escalation.md` · Read `_common.md` first.

## Goal

Pick, with evidence, how CPD will:
1. let the Lead ask the **User** structured questions during Context (interviews) and simple approvals;
2. let a **Worker** ask the **Lead** a blocking question (Escalation) and continue once answered, without corrupting the User's half-typed input in the Lead's editor.

## Candidates

- **Questions to the User:** `@juicesharp/rpiv-ask-user-question` (npm, repo `juicesharp/rpiv-mono`, clone at `/tmp/cpd-research/repos/juicesharp__rpiv-mono`, tool `ask_user_question` in `packages/rpiv-ask-user-question/`): 1–4 questions × 2–4 options, optional Markdown preview, TUI overlay.
- **Escalation, option 1:** `pi-intercom` (npm 0.17.0, repo `nicobailon/pi-intercom`, clone at `/tmp/cpd-research/repos/nicobailon__pi-intercom`): local broker; tool `intercom` with `ask`/`reply`; wakes an idle session with `sendMessage` plus a steer. A broker process auto-spawns.
- **Escalation, option 2: a `cpd`-native stub** you write:
  - `/tmp/cpd-trials/t5/cpd-ask.sh "<question>"`: writes `escalations/<id>.q.json`, then blocks polling for `escalations/<id>.a.json` (timeout configurable); prints the answer.
  - A Lead-side pi extension `/tmp/cpd-trials/t5/wake-ext.ts`: watches `escalations/` and surfaces a new question with `pi.sendMessage({customType: "cpd-escalation", …}, {triggerTurn: true, deliverAs: "followUp"})`.
  - The Lead answers by running `/tmp/cpd-trials/t5/cpd-answer.sh <id> "<answer>"`.

## Setup

- herdr: workspace `t5` in `herdr --session cpd-trial`. Lead and Worker are pi agents in panes, started with `agent start … --kind pi -- --model github-copilot/claude-haiku-4.5:low -e <packages…>`. Load packages per run with `-e npm:<pkg>@<version>` only, and record the versions.
- Drive TUIs with `herdr --session cpd-trial pane send-keys <pane> <keys…>` / `pane send-text`, and read them with `pane read <pane> --source visible`. Prompt agents with `agent prompt <name> "…" --wait --timeout 300000`.
- pi-intercom keeps broker state somewhere under your home. Find out where, list it in the report, and remove what the trial created. Kill the broker at the end.

## Acceptance criteria

1. **must** `ask_user_question`, with the dialog driven over send-keys and the answer read back:
   - The Lead (instructed precisely) asks 2 questions, one single-select and one multi-select, one with a Markdown preview.
   - The structured answer appears in the Lead's session (JSONL excerpt).
   - Report how the dialog renders in a 120×40 headless pane, and the keys needed.
2. **must** `ask_user_question` as a plan approval: a single-select "approve / revise / reject" with the Plan as a Markdown preview (~40 lines). Does it render and scroll? Is it usable?
3. **must** pi-intercom: the Worker asks the Lead a question with `ask`, the Lead replies, and the Worker continues and uses the answer (evidence from both session files). Measure the latency.
4. **must** Draft safety for both Escalation options:
   - Put `DRAFT-<option>-123` in the Lead's editor with `pane send-text`, no Enter.
   - Trigger an Escalation while the Lead is idle.
   - After the Lead's turn, the draft is still in the editor (`pane read --source visible`) and was never submitted as a user message (session JSONL).
5. **must** The `cpd`-native stub works end to end: Worker blocked, Lead woken, answer delivered, Worker continues.
6. **should** Lead busy: an Escalation arrives while the Lead is mid-turn (`working`). When is it surfaced, and does it interrupt or queue?
7. **should** Lead restarted: kill and restart the Lead pi while an `ask` is pending (pi-intercom and stub). Does the question survive, and does the Worker unblock or time out?
8. **should** Timeouts: what happens to a Worker whose question is never answered (pi-intercom default 10 min; set it shorter for the test if configurable).

## Questions the report must answer

- Recommendation for questions to the User (keep `rpiv-ask-user-question`, or not) and the exact prompt or tool usage the `cpd-lead` skill should follow.
- Recommendation for Escalation transport: pi-intercom vs the `cpd`-native stub. Weigh agent-agnostic fit (a TypeScript `cpd` CLI must work later for Claude Code and Hermes Workers too), durability across restarts, draft safety, and dependency weight.
- The minimal Lead-side pi extension surface needed for whichever you recommend.
