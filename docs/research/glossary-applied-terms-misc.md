# Glossary applied terms (terms-misc)

Attempt terms-misc-a1 of Job terms-misc, Plan glassary. Rows are the Spec's ROWS a to d, with one row per change and per use I saw. Line numbers are after the edit.

| Where | Row | Applied or not applied | The old quote and the new text | Reason when not applied |
|---|---|---|---|---|
| docs/implementing.md:413 | a | Applied | "CI's `rust` job runs the same command." → "Where CI exists, its `rust` job runs the same command." | |
| docs/implementing.md:416 | a | Applied | "the Gate and CI also run that command." → "the Gate runs that command as well (and CI too, where CI exists)." | |
| BACKLOG.md:6 (deleted) | a | Applied | "Move CI from `ubuntu-24.04` to `ubuntu-26.04` deliberately…" → bullet deleted | |
| docs/cockpit-parity.md → docs/archive/cockpit-parity.md | b | Applied | `git mv`, no content change; the file has no Markdown links to other files | |
| docs/commands.md:3175 | b | Applied (the Lead allowed it, question terms-misc-a1-q1) | "the checklist of every difference is [cockpit-parity.md](cockpit-parity.md)." → "…[cockpit-parity.md](archive/cockpit-parity.md)." | |
| docs/commands.md:4447 | b | Applied (the Lead allowed it, question terms-misc-a1-q1) | "the checklist behind them is [cockpit-parity.md](cockpit-parity.md)." → "…[cockpit-parity.md](archive/cockpit-parity.md)." | |
| docs/contract.md:4217 | b | Applied (the Lead allowed it, question terms-misc-a1-q1) | "checklist `cockpit-parity.md` \|" → "checklist `docs/archive/cockpit-parity.md` \|" | |
| BACKLOG.md:15 | b | Applied | "**Remote workflows: other machines' trees…**" → "**Remote trees: other machines' trees…**" | The title's word is the cpd 1 sense of Workflow (Plan); the new Workflow means one pull request's object, so it is a term use. Body unchanged |
| docs/implementing.md:127 | c | Not applied | The Spec says "name cpd 1 test areas with the word workflow": the word does not occur in the file | The claim is false: `rg -i workflow docs/implementing.md` finds nothing. The line names the test area `pr` (a code string) |
| docs/implementing.md:162 | c | Not applied | "this PR (`Store::open_with`)" → would be "this change" (audit) | Needs a decision: the work unit and its pull request are one phrase (audit: decide). Not a Workflow-to-Plan case |
| docs/implementing.md:231 | c | Not applied | "The tag is a PR key of Plan tests-mock" | Not a Plan: the tag names a work unit. Object PR → Workflow is the rename's job (pro-d148 rule 1) |
| docs/ui-preview.md | d | Not applied | `rg -n -i 'workflow\|Deploy\|Reviewing'` finds only line 1 ("Previewing", which contains the letters "reviewing") | No term the decisions change is in the file |
| docs/ui-preview.md:9, :11 | d | Not applied | "The Lead adds a throwaway PR to the Plan…", "From the PR's Lead worktree, run:" | Object PR → Workflow is forbidden outside CONTEXT.md in this workflow (pro-d148 rule 1); deferred to the rename |
| docs/versioning.md | d | Not applied | `rg -n -i 'workflow\|Deploy\|Reviewing'` finds nothing | No term the decisions change is in the file |
| docs/versioning.md:5-6 | other | Not applied | "Today it is **13**" and "Today it is **45**" | A count, not a term; the audit lists it as an adjacent error. Out of scope |
| BACKLOG.md:15 | other | Not applied | "Estimated 7 PRs:" | Mixed: 7 Workflows or 7 pull requests; the text cannot tell (audit: decide) |
| BACKLOG.md:15 | other | Not applied | "Plan and Deploy views read remote" | Cockpit label: the cockpit still says Deploy today (rule 4); deferred |

## Deferred to the rename

- docs/implementing.md:127 — test area `pr` and its file glob `pr*` (a code string; `cli/tests/pr/` stays by pro-d148).
- docs/implementing.md:120 — `cli/tests/pr/git_guard.rs` (a path).
- docs/implementing.md:168 — "PR rebase guard" (the guard of the command `pr rebase`).
- docs/implementing.md:162 and :170 — "this PR (`Store::open_with`)", "this PR has not created yet" (a decision: work unit or pull request).
- docs/implementing.md:231 — tags `m-pr-a`, `m-pr-b` (identifiers in files) and "The tag is a PR key of Plan tests-mock" (object PR → Workflow).
- docs/ui-preview.md:9 — "The PR is never opened; it is cancelled after the preview." (mixed: needs a decision).
- docs/ui-preview.md:9, :11 — PR as the object (rename).
- BACKLOG.md:15 — "Plan and Deploy views read remote" (cockpit label Deploy; the third column becomes Dispatch in the rename).
- BACKLOG.md:16 — "Estimated 7 PRs" (a decision).
- Migration names `0002-cockpit-parity` in core/src/store/schema.rs:50, cli/tests/store/store.rs:1201 and docs/contract.md:1262, :2074, :2117 — applied migrations and code strings; they match the acceptance `rg` but do not link to the file.
- docs/archive/cockpit-parity.md — archived history; its Workflow and Dispatch words stay (docs/archive is never rewritten).
