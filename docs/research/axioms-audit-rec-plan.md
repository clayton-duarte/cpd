# Axioms audit 1: Records, topics plan, planning, lifecycle, engine, schema, migration

## Coverage
- Read in full: /tmp/cpd-axioms/records/plan.md lines 1-688, all 170 in-force records of topics `plan` and `planning` (Context building-cpd, snapshot 2026-10-24, rendered oldest first), in line chunks 1-39, 40-299, 300-559, 560-688. Read order was the records' order: hs-d28 (line 3) to hs-n454 (line 686).
- Read in full: /tmp/cpd-axioms/records/engine.md lines 1-589, all 147 in-force records of topics `lifecycle`, `engine`, `schema` and `migration` (same Context and snapshot), in line chunks 1, 2-281, 282-461, 462-589. Read order was the records' order: hs-n5 (line 3) to hs-n440 (line 587).
- Candidate-line pass: `rg -n -i` over both files on the rule words axiom, hard rule, golden rule, never, always, must, no <thing>, rule, flake, leak, orphan, sleep, mock, external, review, draft, chat, guard, checkout, worktree, supersede, recency, YAGNI, DRY, KISS, idempotent, reversible, upstream, vendor, fork, one source of truth, source of truth, cause, symptom, delete first, fix the cause, make it work, existing package, existing tool, evidence, verify, dead code, retry, flaky; each hit was read in its record.
- Judge scope: every candidate was judged against the 29 axiom titles and texts given in the Spec. Records that apply an axiom to a concrete cpd case (a cause, a mechanic, a fix list, a routing, a supersession) were not reported, including the `Recency (hs-d358, ...)` records, which apply "Recency wins" to named supersessions, and the "restated under hs-d358" records, which restate cpd decisions rather than axioms.
- Not read: nothing in these two files is unread. Records only referenced by an id (for example hs-d153, hs-d238, hs-d267, hs-d303, hs-d358, hs-d606, hs-d233) are not blocks in these files and were not judged as such, per the Spec. Briefs and Plan records are out of scope and are absent from these files.
- Not reported by the accepted assumptions: the real-tmux/real-git test violations of the mocks axiom (A6, audit 2 sizes the migration); no record here is that test code.

## Findings
| Where | Axiom | Kind | Quote | Proposed fix |
|---|---|---|---|---|
| [hs-d239] | No planned work only in chat | restate | "every piece of work a Lead intends is registered in cpd the moment it is planned" | retire: superseded by the axiom "No planned work only in chat" |
| [hs-d301] | Decided work skips review | restate | "Decided work skips the Plan review (User, 2026-10-19; amends hs-d173, hs-d174 and hs-d179)." | retire: superseded by the axiom "Decided work skips review" |
| [hs-d400] | Decided work skips review | restate | "Work that only carries out a recorded User decision is added queued with no review (hs-d301)" | retire: superseded by the axiom "Decided work skips review" |
| [hs-d491] | No flakes, no leaks, no orphans, no sleep, no errors | restate | "A Gate failure fails the Job like any other; a flake is fixed at its root (hard rule)." | replace the clause with: see the axiom "No flakes, no leaks, no orphans, no sleep, no errors" |
| [hs-d573] | Fix the cause, not the symptom | restate | "we should fix the cause, not the symptom" | retire: superseded by the axiom "Fix the cause, not the symptom" |
| [hs-d299] | Convention over configuration | restate | "existing tools come first (the User's preference: search for an existing package before building)" | replace the clause with: see the axiom "Convention over configuration" |
| [hs-d250] | YAGNI | restate | "Everything is decided on evidence and not over-engineered (User)." | delete the sentence "Everything is decided on evidence and not over-engineered (User)" |
| [hs-n232] | Prose is not enforcement | contradict | "Freeze enforcement is prose only: the User declined a CI check (per dev-speed's Lead)." | raise to the User: a freeze rule enforced only by prose, which the axiom "Prose is not enforcement" forbids. |
| [hs-d196] | Validate at the boundary, fail loud | borderline | "A store write whose cwd cannot be read records cwd '(unreadable)' instead of failing." | raise to the User: cannot tell whether recording '(unreadable)' is the silent fallback the axiom forbids. |

## Counts
restate 7, contradict 1, borderline 1
