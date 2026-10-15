# Job `queue-ask`: Lead queue, Escalation files, `cpd ask`, `cpd answer`

Tier S2 · Needs: `config-tiers` · Read `_common.md` first. Contract sections: §3 (Escalation ids), §4, §10, §11 (`ask`, `answer`). Trial evidence: `docs/trials/t5-questions-escalation.md` (Finding 5, 7 and 8, Recipes "cpd-native transport") and `docs/trials/t1-spawn.md` (wake-ext writer side).

## Goal
The file protocol between `cpd` and the Lead: an atomic, schema-checked Lead queue, plus blocking Worker questions that survive a Lead restart. No pi code here; the pi adapter is another Job.

## Files you own
`src/queue/messages.ts`, `src/queue/lead-queue.ts`, `src/queue/escalation-files.ts`, `src/commands/ask.ts`, `src/commands/answer.ts`, `test/queue/**`, `test/commands/{ask,answer}.test.ts`.

## Requirements
- `messages.ts`: zod schema for the §10 message and its `kind` union. **Export it**, because the pi adapter Job imports it.
- `lead-queue.ts`: `notifyLead(wf, {kind, text, data?, job?, attempt?})` writes `<epoch-ms>-<id>.json` atomically (`.tmp` then rename) into `workflow.json.lead.queueDir`. If the Workflow has no attached Lead, fail `lead_not_attached` (do not drop messages silently). Read `workflow.json` with zod; the shape is in §6.
- `escalation-files.ts`:
  - `writeQuestion`, `readQuestion`, `writeAnswer`, `readAnswer` and `listOpenQuestions(wf)`, all atomic and zod-checked;
  - `nextEscalationId(wf, attemptId)` gives `<attempt-id>-q<k>`;
  - `expireOrphans(wf)` removes `.q.json` files whose `askerPid` is dead and that have no answer, returning their ids.
- `cpd ask <attempt-id> <question> [--timeout-minutes N] --wf <wf>`: per §11. Write the question, then a Lead message `escalation` whose `text` ends with the exact `cpd answer <esc-id> "<answer>" --wf <wf>` command. Poll every 0.5 s. Print `{ok, answer}`. On timeout, remove the question and exit 5 `ask_timeout`.
- `cpd answer <esc-id> <answer> --wf <wf>`: per §11, including the `escalation-answered` message.

## Acceptance
1. `cpd ask` started as a child process blocks; `cpd answer` from another process unblocks it; the asker prints the answer. Assert the order of files and queue messages (`escalation`, then `escalation-answered`).
2. Lead-restart durability: a question written while no reader exists is listed by `listOpenQuestions`, and answering it later unblocks the asker.
3. A timeout of 3 s (via `--timeout-minutes 0.05`) exits 5 and leaves no `.q.json`. A later `answer` gives `escalation_missing`. A second `answer` gives `escalation_answered`.
4. `notifyLead` writes only complete files (never a partial `.json` visible to a reader). Prove it with a reader racing 200 writes. Every file validates against the schema.
5. `expireOrphans` removes a question whose `askerPid` is dead.

## Out of scope
Delivering messages to pi (Job `pi-adapter`); recording Escalations in the Ledger (Job `watch`).
