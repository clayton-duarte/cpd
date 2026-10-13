# Job `gate-failure-report`: a failed Gate tells the Worker which tests failed

Tier S2 · Needs: none (appended: found by the first cpd-dispatched Attempts) · Contract sections: §11 (`attempt submit`), §12 step 11, §2.

## Goal
`cpd attempt submit` refuses with `gate_failed` and puts "the last 60 lines of output" in `details`. For this repo's Gate (`node --test`, TAP output) the last 60 lines are always passing tests and the summary, so the Worker cannot see what failed and re-runs the whole suite (20 minutes under load) to find out. This happened to the first two cpd-dispatched Attempts (`ledger-hardening-a1`, `pi-adapter-a1`). Make every failed Gate actionable.

## Files you own
`src/dispatch/attempt-submit.ts`, `src/git/gate.ts` (only if the Gate output capture lives there), `test/dispatch/attempt-submit.test.ts`, `test/dispatch/gate-report.test.ts`.

## Requirements
- On `gate_failed`, write the **full** Gate output to `$CPD_HOME/workflows/<wf>/reviews/<attempt-id>.gate-failed-<k>.txt` (k = 1, 2, … per Attempt; never overwrite), and return in `details`:
  - `outputPath`: that file;
  - `failing`: up to 20 failing test entries extracted from TAP (`not ok <n> - <name>` at any indentation, with the `location:` line that follows when present);
  - `tail`: the last 60 lines, as today.
- If no `not ok` line is found (a non-TAP Gate, or a crash before TAP), `failing` is empty and the message says to read `outputPath`.
- On success, behaviour is unchanged (`reviews/<attempt-id>.gate.txt` as today).
- The message text tells the Worker exactly what to do: read `outputPath`, fix the listed tests, commit, submit again.

## Acceptance
1. A fixture Gate that prints TAP with two failing tests (one nested) and 100 passing lines after them: `details.failing` lists both with locations, `outputPath` holds the whole output, `tail` is the last 60 lines.
2. A second failure writes `…gate-failed-2.txt`, and the first file is unchanged.
3. A non-TAP failing Gate gives `failing: []` and the message points at `outputPath`.
4. `./verify.sh` passes.

## Out of scope
Changing the Gate itself, test timeouts, or the merge-time Gates.
