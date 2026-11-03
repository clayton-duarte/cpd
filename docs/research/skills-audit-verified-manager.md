# Skills audit: the ESCAPED rows of the Manager side, verified against main

Source of the rows: the Manager-side cross-check `scout-x-mgr.md` (13 rows classed ESCAPED). Skipped by the User's decision: MG-2, MG-3, MG-4, MG-5, MG-6, MG-7 and MG-15. Six rows remain; none is marked `=` with another. Text checked: `origin/main` at 8aaed0d8f, which holds PR 794. Line numbers are current lines of that commit, found again by their quoted words with grep. Prefixes: `M` is `pi/skills/cpd-manager/SKILL.md`, `L` is `pi/skills/cpd-lead/SKILL.md`, `ST` is `pi/skills/skill.test.ts`, `WA` is `pi/test/skill-writing-audit.test.ts`.

Read by: the eight pin files (`WA`, `ST`, `pi/test/skill-usage-grammar.test.ts`, `pi/test/skill-lateral-comms.test.ts`, `pi/test/user-commands.test.ts`, `pi/test/mainguard.test.ts`, `pi/test/skill-retry.test.ts`) and section 15.25 (R1 to R17) were grepped for the quoted words. A deletion lowers the R13 negation count (WA:251-257), which only has a ceiling, so it breaks nothing.

| Row | Class | Where | Quote | Fix | Read by | Breaks |
|---|---|---|---|---|---|---|
| MG-10 | CONFIRMED | M, Areas and overlaps, M:126 (and M:127) | "the next Workflow targets the previous Workflow's branch (`gh pr edit <n> --base <branch>` right after its pull request opens, or `cpd workflow add <key> --repo <path> --title \"<title>\" --base <branch>`" | replace the commands with a line that tells the Lead the landing order and the branch to build on (M:47 leaves `cpd workflow add` to the Lead) | ST:574-581 reads `gh pr edit <n> --base` (only at M:126) and `cpd pr push --workflow <key>` (only at M:127) in M; ST:624-627 reads M:122, not M:126; R17 (WA:240-247) reads M:47 | YES |
| MG-11 | CONFIRMED | M, 2. Plans, step 4, M:104 | "4. **Check pending work first.** Before you start a Plan or route an issue to a Lead, run `cpd context pending --context <c>`" | fold the check into step 1 Intake (keep the step numbers 1 to 6), or renumber so it comes first | WA:125-126 (R7) reads the text of items 1 Intake and 2 Interview; ST:567-571 reads `cpd context pending --context <c>` anywhere in M | NO (as long as Intake keeps "every deliverable", "affected repo", "out-of-scope" and `cpd context pending --context <c>` stays in M; moving it to a new step 1 would break the `^1\. \*\*Intake` match at WA:125) |
| MG-12 | FALSE | M, Rules M:45; Context records M:85 and M:93 | M:45 "Never use `cpd context ask`: it records an open question in the Context and asks nobody." M:85 "cpd context ask \"<question nobody can answer yet>\" --context <c>   (a record, not a question to the User)" | none: M:45 bans `context ask` as a way to ask the User; M:85 records an open question and says so; the two do not conflict | ST:685-692 reads both (the ban at M:45, and the line `cpd context ask ` with "(a record, not a question to the User)"); ST:157 reads `^cpd context ask ` in M | NO (no fix; dropping either text would break ST:688 or ST:690-692) |
| XS-2 | CONFIRMED | M, Oversight, Check each Plan's registered work, M:134 | "a Job that `--needs` a `queued` or `running` Job" | add `failed`: "a `queued`, `running` or `failed` Job" (as L Rules and `docs/commands.md` say) | ST:561-564 reads M:134 only for "Check each Plan's registered work", `cpd plan show --plan <p>` and `cpd session send <context>:<plan>[^\n]*register`; R8 (WA:140) pins the Lead bullet, not this one | NO |
| XS-3 | CONFIRMED | L, Within the Plan, L:241; M, Landing order, M:122 | L:241 "ask the Manager to split the work or move it: ... and it handles the sequencing." M:122 "which moves the work or re-routes it, never sequences across Leads." | in L:241 use the Manager's wording: "and it moves the work or re-routes it" | NONE (grep of the eight pin files for "sequenc", "handles", "moves the work" and "re-routes": only a comment at ST:605 and an unrelated hit at `mcp-attach.test.ts`, not a pin file) | NO |
| XS-10 | FALSE | M, Urgent Jobs, M:136 | "For a queued Job run `cpd job urgent <k> --plan <p>` instead of `cpd job dispatch`" | none: the clause names `cpd job dispatch` only as the command not to use, and M:48 leaves it to cpd, so the text agrees with the Rules | ST:630-638 reads `cpd job urgent <k>`, `--urgent` and the explicit-request rule at M:136; R17 (WA:240) reads M:48 | NO (no fix) |

## Counts

| Class | Count | Rows |
|---|---:|---|
| CONFIRMED | 4 | MG-10, MG-11, XS-2, XS-3 |
| FALSE | 2 | MG-12, XS-10 |
| STALE | 0 | none |

CONFIRMED rows with Breaks YES:

- MG-10: ST:574-581 needs `gh pr edit <n> --base` and `cpd pr push --workflow <key>` in M; the fix must keep those two strings in M, or ST:574-581 changes in the same commit.
