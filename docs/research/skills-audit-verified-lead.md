# Skills audit: the ESCAPED rows, verified against main

Source of the rows: the Lead-side cross-check `scout-x-lead.md` (32 rows classed ESCAPED). Text checked: `origin/main` at 8aaed0d8f, which holds PR 794 (c4e7d70da). Line numbers are current lines of that commit. Prefixes: `L` is `pi/skills/cpd-lead/SKILL.md`, `SF` is `references/spec-files.md`, `PR` is `references/pr-description.md`, `Q` is `references/questions.md`, `M` is `pi/skills/cpd-manager/SKILL.md`. Pins: `WA` is `pi/test/skill-writing-audit.test.ts`, `ST` is `pi/skills/skill.test.ts`, `RT` is `pi/test/skill-retry.test.ts`, `LC` is `pi/test/skill-lateral-comms.test.ts`, `UC` is `pi/test/user-commands.test.ts`. Rules R1 to R17 are those of `docs/commands.md` section 15.25. Where two rows name one defect, each keeps its own row and the Fix cell says "same as".

Read by: every pin file and every rule was grepped for the quoted words. Where a row says NONE, no pin of the eight files (`WA`, `ST`, `RT`, `LC`, `UC`, `pi/test/skill-usage-grammar.test.ts`, `pi/test/mainguard.test.ts`) reads those words. A deletion in `L` also lowers the R13 negation count (WA:251), which only has a ceiling, so no deletion breaks it.

| Row | Class | Where | Quote | Fix | Read by | Breaks |
|---|---|---|---|---|---|---|
| LW-7 | CONFIRMED | L, Addressing item 3, L:432 | "written with `cpd workflow describe <key> --file <body.md>`, and opens only when you do." (L:45: "cpd opens the pull requests, you do not.") | delete the clause "and opens only when you do" | NONE | NO |
| LW-8 | CONFIRMED | SF, A feedback fix Job, SF:70 | "written with `cpd workflow describe <key> --file <body.md>`, and opens only when you do." | delete the clause; same as LW-7 | NONE | NO |
| LW-9 | CONFIRMED | L, A command the User asks for, L:116 | "Check the log once, after a few seconds (`sleep 10; tail -n 20 /tmp/cpd-<plan>/<name>.log`)" (L:57: a sleep "runs in the background") | delete `sleep 10;` | NONE (UC:108 reads only a `sleep 60` command sample, not this text) | NO |
| LW-14 | CONFIRMED | L, Addressing items 7 to 9, L:436-438 | "7. A Workflow can be added after the lock, in any phase, `done` included" | merge items 7 and 8 into the `plan-done` row (L:308); delete item 9 | WA:77-81 (R4, numbering 1 upward: a shorter list still passes); WA:62-75 (R3, step references: item 9 holds the only "step 7" there) | NO |
| LW-15 | CONFIRMED | L, Addressing item 8, L:437 | "8. The Plan is done when every Workflow is merged (`plan-done`)." | delete; L:308 already says it | NONE | NO |
| LW-16 | CONFIRMED | L, Addressing item 9, L:438 | "9. Register every further fix, batch or follow-up Workflow you intend as soon as you plan it" | delete; L:58 states it | NONE | NO |
| LW-21 | CONFIRMED | L, Failed Jobs, L:348 | "A `gate_failed` Attempt was already retried once at the same Tier (contract §3.4 "A Gate retry")" | delete the bullet; the table row at L:341 says it | NONE (WA:83-95 reads the nine cause rows, not this bullet) | NO |
| LW-24 | CONFIRMED | L, Rules, L:55-56 | "**Ask the User directly.** You talk to the User yourself" and "**Resolve first, ask second.**" | merge L:55 into L:56 | WA:233-248 (R12, bullet start) and WA:251 (R13); ST:207-210 reads Escalations, which stays | NO |
| LW-26 | CONFIRMED | L, Exploring, L:152 | "**Promoting a finding** into the repo (a `docs/research/*.md` file) is an ordinary docs Job you add." | delete; SF:105 states it | WA:165-177 (R10, Exploring word ceiling only) | NO |
| LW-27 | CONFIRMED | SF, Test Jobs, SF:95 | "a `tdd` Workflow that needs `[test-commands]` is refused `tests_unconfigured`" | delete the sentence; L:208 states it | ST:759 reads `tests_unconfigured` in L Scoping, not SF | NO |
| LW-32 | CONFIRMED | L, Scoping, L:206 | "**A Workflow's shape** (`--shape`, stored and enforced: it cannot be changed, so cancel the Workflow and add another)" | move to references/workflow-shape.md behind one pointer in Scoping | ST:753-759 (Scoping must hold `--shape docs`, `--shape config`, the `<key>-docs` template, "stored and enforced", `shape_violation`, `shape_incomplete`); ST:292-305 (Splitting "Workflow shape" bullet, not this paragraph) | YES |
| LW-36 | CONFIRMED | L, Events table, `pr-feedback` row, L:307 | "A Workflow's pull request needs attention" (cell of six inline branches) | replace the branches with links to #pull-request-review and #addressing | WA:262-275 (R14, the row keeps its link); ST:696-709 (row must keep "A conflict with the base is no reason and sends you nothing", the `cpd job add` text, "nothing else is queued", "cpd queues an update Job for it") | YES |
| LW-37 | CONFIRMED | L, Events table, `job-merged` row, L:304 | "A Job is Merged. Note it. An explore" (cell of all branches inline) | split the branches into the sections the cell links | ST:788-791 (cell must keep `data.number` and `data.url`, "Do not open a pull request yourself", "A fix Job of an open Workflow is done when its `job-merged` arrives"); ST:227 (table stays under the first Dispatch step) | YES |
| LW-39 | CONFIRMED | L, Orchestrating step 6, L:280 | "`cpd workflow describe <key> --file <body.md>` where `<body.md` is a file outside the repo" | close the placeholder: `<body.md>` | NONE (the fact check ST:430-470 reads code spans for codes and commands, not this one) | NO |
| LA-5 | CONFIRMED | PR, Good example, PR:97 | "Cache invalidation for batch updates is not implemented; registered in backlog." (PR:41 asks for registered Job and Workflow keys) | name a Workflow key in the example, such as "see Workflow cache-invalidation" | NONE | NO |
| LA-7 | CONFIRMED | L, Addressing items 8 and 9, L:437-438 | "8. The Plan is done when every Workflow is merged (`plan-done`)." | delete both; same as LW-15 and LW-16 | NONE | NO |
| LA-10 | CONFIRMED | L, Failed Jobs, L:348 | "so a `job-failed` of `gate_failed` is the second failure." | delete L:348; same as LW-21 | NONE | NO |
| LA-11 | CONFIRMED | L, Events table L:305, Failed Jobs L:351, Mechanical checks L:395 | "never cancel (a cancel stands: commands.md §5.3)" (L:351); L:305 and L:395 repeat it | keep L:351; link L:305 and L:395 to #failed-jobs | RT:34-37 (each of the three lines must hold `cpd job retry <k> --tier <T>` and "never cancel") | YES |
| LA-12 | CONFIRMED | L, Grilling item 1, L:158 | "Group independent questions into one round; a question that depends on an open answer waits for the next round." | replace the sentence with a link to references/questions.md (Q:9) | WA:110-134 (R7, Grilling item 1 keeps its five phrases: this sentence holds none) | NO |
| LA-14 | CONFIRMED | L, Splitting, L:225 and L:228; SF:28, SF:38, SF:48 | "never a full unfiltered run (the whole workspace, `npm test` of everything)" (L:228); "about 150 turns" (L:225, SF:38) | keep SF; link from L:225 and L:228 | ST:286-287 (L and SF must each state "targeted tests" and "full suite is the Gate's"); ST:276-280 (SF rules) | YES |
| LA-17 | CONFIRMED | L, A command the User asks for, L:107 | "tell the User what was refused and what to do instead; never work around it." | replace with a pointer to the axiom "Fix the cause, not the symptom" | NONE (UC:49 reads "What stays." only) | NO |
| LA-18 | CONFIRMED | L, Rules, L:56 | "Ask the User only for decisions you cannot derive." | delete the sentence | NONE | NO |
| LA-19 | CONFIRMED | L, Rules, L:58 | "No planned work lives only in your chat: register every batch, follow-up Workflow or fix when you plan it." | point to the axiom "No planned work only in chat" | WA:136-146 (R8, the bullet keeps its tokens and its 180-word ceiling; this sentence holds none) | NO |
| LA-20 | CONFIRMED | L, Rules, L:60 | "with a Spec that cites the decision; it needs no Plan review." | replace "it needs no Plan review" with a pointer to the axiom "Decided work skips review" | WA:136-146 (R8 keeps "Spec that cites the decision" and "When unsure, a draft", both stay); ST:672-677 (Lead keeps "added queued"); ST:682 reads the Manager skill, not L | NO |
| LA-21 | CONFIRMED | L, Splitting, L:229 | "a fresh Worker with only this Spec must finish without asking." | delete the clause; keep "Complete and guess-free" | NONE | NO |
| LA-25 | CONFIRMED | PR, What and why rule, PR:35 | "A reviewer should understand the problem and the change in these two questions answered" | rewrite as "A reviewer should be able to answer two questions from it:" | NONE | NO |
| DL-1 | CONFIRMED | L, Rules, L:44 | "`cpd session archive` and `cpd session unarchive` (refused `agent_forbidden`: the User archives" | move the parenthesis to the six archive commands | WA:233-248 (R17, command tokens stay); docs/commands.md section 3.9 lists six archive commands | NO |
| DL-5 | CONFIRMED | L, Addressing items 7 to 9, L:436-438 | "7. A Workflow can be added after the lock, in any phase" | same as LW-14 | WA:77-81 (R4) | NO |
| DL-6 | CONFIRMED | L, Splitting Size, L:225; SF:38 | "Size a Job to fit well inside it (about 150 turns)" | drop the number (no docs/commands.md or docs/contract.md hit for "150 turns") | NONE (ST:838 reads "1500-character", another text) | NO |
| DL-7 | CONFIRMED | L, Orchestrating step 6, L:280 | "where `<body.md` is a file outside the repo" | close the placeholder; same as LW-39 | NONE | NO |
| XS-1 | CONFIRMED | L, Agent keys, L:47 | "a Plan id (that Plan's Lead Session) or `<plan>.<prefix>` (a Session of that Plan by UUID prefix) as well as a full UUID." | cut the bullet to the three agent keys | LC:75-94 (no `<manager-id>`, no `session rename`: the fix keeps both out); docs/commands.md section 15.24 "Addressing" | NO |
| XS-3 | CONFIRMED | L, Splitting, Within the Plan, L:241 | "ask the Manager to split the work or move it: tell the Manager with `cpd session send <context>" ... "and it handles the sequencing." | align with M:122: the Manager "moves the work or re-routes it, never sequences across Leads" | LC:96-110 (the `session send ... --unblocks` form stays) | NO |

## Counts

- CONFIRMED: 32
- FALSE: 0
- STALE: 0

## CONFIRMED rows with Breaks YES

- LW-32: ST:753-759 reads the Scoping paragraph.
- LW-36: ST:696-709 and WA:262-275 read the `pr-feedback` row.
- LW-37: ST:788-791 reads the `job-merged` row.
- LA-11: RT:34-37 needs "never cancel" in all three lines.
- LA-14: ST:286-287 needs "targeted tests" and "full suite is the Gate's" in L.

The 27 other CONFIRMED rows break no pin.
