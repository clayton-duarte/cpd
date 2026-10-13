# Job `job-add-in-pr`: allow appended Jobs in phase `pr`

Tier S1 · Needs: none (appended: contract gap escalated by `lead-skill-a1`) · Contract sections: §11 (`cpd job add`, amended 2026-10-13: phase `dispatch` or `pr`).

## Goal
PR feedback must become appended Jobs (requirements), but `addJob` refuses every phase except `plan` and `dispatch` with `illegal_phase`, so after `cpd pr open` the Lead cannot append anything. Allow phase `pr` with exactly the `dispatch` semantics.

## Files you own
`src/ledger/jobs.ts` (the phase check and the appended path only), `test/ledger/job-add-pr.test.ts`.

## Requirements
- `addJob` in phase `pr` behaves exactly like phase `dispatch`: status `queued` at once, fingerprint recorded, dependency checks, the resumable flip from `ledger-hardening`, and the log event.
- Phases `context` and `done` still refuse `illegal_phase`.

## Acceptance
1. Real-bd test: a Workflow moved to phase `pr` accepts `job add`; the Job is `queued` with a fingerprint and is returned by `readyJobs`.
2. Phase `done` still refuses with `illegal_phase` and writes nothing.
3. `./verify.sh` passes.

## Out of scope
Any other phase rule; `cpd pr rebase` (it already pushes the Lead Branch).
