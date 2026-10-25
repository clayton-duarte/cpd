# Axioms audit 1: Records, topics process, testing, hygiene, context, architecture, config, dotfiles, machines

## Coverage
- Read in full: /tmp/cpd-axioms/records/process.md lines 1-854, all 212 in-force records of topics process, testing, hygiene and context (Context building-cpd, snapshot 2026-10-24). Read in three line chunks: 1-400 (hs-d7 to hs-n226), 401-813 (hs-n226 to hs-d551), 813-854 (hs-d551 to hs-d613).
- Read in full: /tmp/cpd-axioms/records/misc.md lines 1-382, all 83 in-force records of topics architecture, config, dotfiles, machines plus the 4 not-yet-curated records (hs-d607, hs-d610, hs-d612, hs-d613). Read in one pass.
- Candidate-line pass: `rg -n -i` over both files on the rule words never, always, must, hard rule, rule, no thing, flake, leak, orphan, sleep, mock, external, review, draft, chat, guard, stash, checkout, worktree, supersede, recency, YAGNI, DRY, KISS, idempotent, reversible, upstream, vendor, fork, source of truth, cause, symptom, delete first, axiom, convention over, only authored code, make it work, primary checkout; each hit was read in its record. 223 candidate lines were judged.
- Verified record texts outside the files to judge a hit: `cpd context show --record hs-d488` and `cpd context show --record hs-d498` (to place hs-n396's fork). Read-only, no edits.
- Judge scope: every candidate was judged against the 29 axiom titles and texts in the Spec. Records that apply an axiom to a concrete cpd case (a cause, a mechanic, a fix list, a retirement) were not reported, including the ~40 "Recency (hs-d358, ...)" retirement records and the "restated under hs-d358" records, which apply recency to specific records.
- Not read: nothing in this part is unread. Briefs and Plan records are not in these files. Records only referenced by id (hs-d239, hs-d301, hs-d303, hs-d488, hs-d498, hs-d573, hs-d606) are not blocks in these two files and were not judged as such, per the Spec.
- Not reported per accepted assumption A6: the real-tmux and real-git test violations of the mocks axiom (audit 2 sizes the migration); no record in this part is that test code.

## Findings
| Where | Axiom | Kind | Quote | Proposed fix |
|---|---|---|---|---|
| [hs-d27] | Only authored code is tested | restate | "Tests mock every external tool and test only code we wrote (User, machine-sync PR #87)" | retire: superseded by the axiom "Only authored code is tested" |
| [hs-d34] | No flakes, no leaks, no orphans, no sleep, no errors | restate | "no memory leaks and no dangling or orphaned processes, from cpd or from anything it spawns" | retire: superseded by the axiom "No flakes, no leaks, no orphans, no sleep, no errors" |
| [hs-d153] | Make it work, make it fast, make it good | restate | "make it work, make it fast, make it good, in that order: correctness first, then measured performance, then code quality" | retire: superseded by the axiom "Make it work, make it fast, make it good" |
| [hs-d238] | No flakes, no leaks, no orphans, no sleep, no errors | restate | "flaky tests are never acceptable, ever. A test that fails intermittently is a defect, fixed at once at its root as the top priority" | retire: superseded by the axiom "No flakes, no leaks, no orphans, no sleep, no errors" |
| [hs-d248] | Guards only for proven bad behaviour | restate | "A lock or guard is added only to prevent an incorrect behaviour seen in practice, on evidence" | retire: superseded by the axiom "Guards only for proven bad behaviour" |
| [hs-d358] | Recency wins | restate | "A newer decision overrules an older one, every time. When two records of a Context contradict each other, the later one stands" | retire: superseded by the axiom "Recency wins" |
| [hs-d606] | A primary checkout is a mirror of origin/main | restate | "cpd and its agents never touch a repo's main checkout; when code must change, at any level, they fetch fresh and work on a branch in a worktree" | retire: superseded by the axiom "A primary checkout is a mirror of origin/main" |
| [hs-n242] | Convention over configuration | restate | "before building anything new, search for an existing package that already does it (for pi: npm packages with the keyword pi-package)" | retire: superseded by the axiom "Convention over configuration" |
| [hs-n396] | Convention over configuration | contradict | "The pi-compact-tools fork (clayton-duarte/pi-compact-tools, hs-d488) keeps upstream's .github/workflows (ci.yml, release.yml) untouched" | raise to the User: keeps a fork of an upstream package instead of taking the dependency, against the axiom |
| [hs-d267] | No flakes, no leaks, no orphans, no sleep, no errors | borderline | "When the User wants to ship anyway, a Lead pushes and opens the pull request by hand at the User's request" | raise to the User: permits opening a PR past a failing test by hand; cannot tell if the User's order overrides the no-bypass rule |
| [hs-d607] | DRY | borderline | "The set, as the User confirmed: the 19 of AGENTS.md plus make it work/fast/good in that order (hs-d153)" | raise to the User: lists all 29 axioms; cannot tell if the concept decision may enumerate them or must point to AGENTS.md |

## Counts
restate 8, contradict 1, borderline 2
