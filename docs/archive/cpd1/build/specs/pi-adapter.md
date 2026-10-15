# Job `pi-adapter`: the Lead's pi extension (queue delivery, blocked state, plan review)

Tier S3 · Needs: `queue-ask`, `ledger-core`, `templates-render` · Read `_common.md` first. Contract section: §15 (normative), plus §10 message kinds. Trial evidence:
- `docs/trials/t1-spawn.md` (Recipes `wake-ext.ts`, F5, F6);
- `docs/trials/t5-questions-escalation.md` (Recipes Lead-side extension, Finding 6);
- `docs/trials/t4-plan-approval.md` (Recipes adapter, Findings 1–5, 7);
- `~/.pi/agent/extensions/herdr-agent-state.ts` (the `herdr:blocked` event).

## Goal
The only pi-specific code in v0. It delivers `cpd` messages to the Lead draft-safely, surfaces Worker questions as herdr "blocked", and lets the Lead request plan approval through plannotator.

## Files you own
`pi/package.json`, `pi/extension.ts`, `pi/lib/*.ts`, `test/pi/**`.

## Requirements
- `pi/package.json`: name `cpd-pi`, `"keywords": ["pi-package"]`, `"pi": {"extensions": ["./extension.ts"]}`, peerDependencies `@earendil-works/pi-coding-agent: "*"`. Import the message schema from `../src/queue/messages.ts` (relative import, same repo).
- **Activation:** `session_start` in TUI mode with `HERDR_ENV=1`; otherwise do nothing and register nothing.
- **Queue:** §15, exactly.
  - Find the session id the way pi exposes it to extensions (check pi 1.1.0 `dist/core/extensions/types.d.ts`; the session file header id must equal it; record how you got it).
  - Drain at start, then `fs.watch`, claim by rename (exactly-once).
  - Validate against the schema. An invalid file goes to `rejected/` and the Lead gets an error message.
  - Deliver per the §10 table (`triggerTurn` and `deliverAs: "followUp"`); `escalation-answered` produces no message.
  - Close the watcher on shutdown.
- **Blocked state:** emit `pi.events.emit("herdr:blocked", {active, label})`, counted per escalation id. Start counting from `cpd`-provided state: on start, count open questions in the attached Workflow (read via `cpd workflow show`, or via the `listOpenQuestions` helper of `queue-ask` if importable).
- **Tool `cpd_plan_review`** (`exposure: "model-only"`, no parameters), per §15 and the t4 Recipes:
  - the `plannotator:request` payload and `respond` handling;
  - `cpd plan decision` calls through `pi.exec` (`cpd` on `PATH`);
  - the `plannotator:review-result` listener;
  - `review-status` polling for pending reviews at `session_start`.
- Pure logic (claim, validate, kind→delivery mapping, blocked counter, review bookkeeping) lives in `pi/lib/*.ts` and is unit-tested without pi. The `extension.ts` glue stays thin.

## Acceptance
1. Unit tests for the pure modules:
   - claim exactly once under duplicate watch events;
   - an invalid message goes to `rejected/` plus an error delivery;
   - the kind→`{triggerTurn, deliver}` mapping equals the §10 table;
   - the blocked counter (two questions; answering one keeps blocked; answering both clears it);
   - plan-review bookkeeping (pending to approved and rejected; recovery from pending ids).
2. `CPD_IT=1` integration in `herdr --session cpd-test` (model `github-copilot/claude-haiku-4.5:low`):
   - start pi with `-e pi/extension.ts`;
   - type a draft with `herdr pane send-text` (no Enter);
   - write a `job-merged` queue message;
   - assert a `custom_message` with `customType "cpd"` appears in the Lead session JSONL, the draft is still visible (`pane read`), and no user message contains it (as in t1 F6).

   Record the transcript in the report if run.
3. `CPD_IT=1` plannotator integration: follow the t4 Recipe with `-e npm:@plannotator/pi-extension@0.28.17`, `PLANNOTATOR_BROWSER` set to a script that records the URL, and `PLANNOTATOR_DATA_DIR` under `/tmp/cpd-build/pi-adapter/`. Approve through HTTP; assert that `cpd plan decision --approved` was called (fake `cpd` on `PATH` recording argv) and that the decision message reached the Lead. Remove any `~/.pi/plannotator-review-status.json` the test created.

## Out of scope
Changes to `src/` (import only); installing packages into pi settings.
