# Axioms audit 1: Records, topic cockpit

## Coverage
- Read in full: /tmp/cpd-axioms/records/cockpit.md lines 1-982, all 237 in-force records of topic `cockpit` (Context building-cpd, snapshot 2026-10-24), in five line chunks (1-200, 201-400, 401-600, 601-800, 801-982). Read order was the records' order, oldest first: lines 1-200 (hs-n3 to hs-n176), 201-400 (hs-n176 to hs-d365), 401-600 (hs-d365 to hs-d476), 601-800 (hs-d476 to hs-d561), 801-982 (hs-d561 to hs-d621).
- Candidate-line pass: `rg -n -i` over the same file on the rule words axiom, hard rule, golden rule, never, always, must, no <thing>, rule, flake, leak, orphan, sleep, mock, external, review, draft, chat, guard, checkout, worktree, supersede, recency, YAGNI, DRY, KISS, idempotent, reversible, upstream, vendor, fork, one source of truth, source of truth, cause, symptom, delete first; each hit read in its record.
- Judge scope: every candidate was judged against the 29 axiom titles and texts given in the Spec. Records that apply an axiom to a concrete cpd case (a cause, a mechanic, a fix, a colour, a glyph, a routing) were not reported.
- Not read: nothing in this part is unread. Briefs and Plan records are out of scope and are not in this file (the Spec's records part holds only in-force records, no briefs). Records only referenced by an id (for example hs-d301, hs-d358, hs-d238, hs-d267, hs-d606, hs-d153) are not blocks in this file and were not judged as such, per the Spec.
- Not reported by the accepted assumptions: the real-tmux/real-git test violations of the mocks axiom (A6, audit 2 sizes the migration); no record here is that test code.

## Findings
| Where | Axiom | Kind | Quote | Proposed fix |
|---|---|---|---|---|
| [hs-d303] | Blocked means it needs the human | restate | "blocked means it needs the human. Anywhere the cockpit or the store shows 'blocked', or the red that stands for it" | retire: superseded by the axiom "Blocked means it needs the human" |
| [hs-d335] | No flakes, no leaks, no orphans, no sleep, no errors | restate | "'No flaky tests' is a hard rule (hs-d238, hs-d267): every known flake is fixed at its root first." | delete the record's last sentence, or replace with: see the axiom "No flakes, no leaks, no orphans, no sleep, no errors" |
| [hs-d498] | Convention over configuration | contradict | "pi-compact-tools' fold-toggle code moves inside the cpd repo as an extension cpd owns" | raise to the User: copies a package's code into cpd instead of taking the dependency, against the axiom. |
| [hs-d593] | Convention over configuration | contradict | "It replaces the rule that @juicesharp/rpiv-ask-user-question is a third-party package, never vendored or forked" | raise to the User: forks a third-party package instead of taking the dependency, against the axiom. |
| [hs-d618] | A primary checkout is a mirror of origin/main | borderline | "Repair is a guarded cpd command that acts on the cpd checkout only" | raise to the User: cannot tell whether "the cpd checkout" is the primary checkout the axiom keeps read-only. |

## Counts
restate 2, contradict 2, borderline 1
