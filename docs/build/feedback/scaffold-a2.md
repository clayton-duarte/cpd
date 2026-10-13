# Job `scaffold`, Attempt 2: Reviewer feedback on Attempt 1 (rejected)

Attempt 1 (branch `cpd/v0--scaffold-a1`, commit `621efe3`, report `/tmp/cpd-build/reports/scaffold-a1.md`) is nearly correct. **Start from it:** `git cherry-pick 621efe3` (in your new worktree), then fix the following. Keep everything else as is.

1. **Blocking: command lookup is vulnerable to prototype keys.**
   - `COMMANDS` is a plain object indexed with user input, so `cpd constructor`, `cpd toString`, `cpd hasOwnProperty` and `cpd __proto__` resolve to `Object.prototype` members and end in exit 1 `unexpected` instead of exit 2 `unknown_command`.
   - A single argv word `"job add"` dispatches to the `job add` stub.
   - Fix: look up with a `Map` (or `Object.hasOwn` on a null-prototype object), and keep separate tables for single-word and grouped commands so a word containing a space never matches.
   - Extend `test/cli.test.ts` to cover `constructor`, `toString`, `__proto__` and `"job add"`, each expecting exit 2 `unknown_command`.
2. `tsconfig.json`: remove `types` and `skipLibCheck` unless they are required. If required, say why under Deviations.
3. `test/helpers/index.ts`: compute paths with `fileURLToPath`, not `new URL(...).pathname`.
4. `src/lib/exec.ts`: a signal-killed process currently becomes code 128 and the signal is lost. Return `signal` in the result too, and test it.
5. The command count: the contract lists **29** commands. Attempt 1 is right; the Spec's "30" is a Lead error. Keep 29.
