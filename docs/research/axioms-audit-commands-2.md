# Axioms audit 1: docs/commands.md lines 2112-3189 (section 10 `session` to section 14)

## Coverage
- Read in full: `docs/commands.md` lines 2112-3189 (the whole part: the `## 10. session` heading through §13.3 bare `cpd`, before `## 14`), with the `read` tool in chunks 2112-2140 (with `sed`), 2140-2340, 2340-2540, 2540-2740, 2740-2940, 2940-2985, 2985-3030, 3030-3055, 3055-3080, 3080-3105, 3105-3130, 3130-3150 and 3150-3189; lines 2100-2111 were read only to confirm the part boundary and were not audited.
- Candidate-line pass: `rg -n -i` over lines 2112-3189 on the rule words never, always, must, rule, hygiene, policy, prose, flake, leak, orphan, sleep, mock, external, review, draft, chat, guard, stash, checkout, supersede, recency, YAGNI, DRY, KISS, idempotent, reversible, upstream, vendor, fork, one source, source of truth, single source, retry, duplicate, copy, shared, reused, test, explicit, implicit, fallback, silent, boundary, fail loud, cause, symptom; each hit was read in context.
- Judged every candidate against the 29 axiom titles and texts given in the Spec. Not reported: the many idempotence statements (commands that obey the axiom, which the Spec excludes); the "single source of needs you", "one marker is the source every view reads", "one helper ... not a copy" and "reused, not copied" passages (they name a concrete cpd mechanic or artefact, not the DRY rule); "decided work, hs-d301" and the supersedes citations (decisions cited as the reason for a mechanic); "The tests use the harness's fake pi" (obeys Only authored code is tested; A6 leaves test sites to audit 2); the unverified labels (they obey Inference is not evidence).
- The view-state fallback rule of line 2892 is repeated at line 3027 and is reported once, at 2892.
- Not read: `docs/commands.md` lines 1-2111 and 3190-4626, which are other audit parts, and every other file.

## Findings
| Where | Axiom | Kind | Quote | Proposed fix |
|---|---|---|---|---|
| docs/commands.md:2243 | Blocked means it needs the human | restate | "`waiting` (an agent waits for another agent) and `blocked` (the User has to act) say why" | replace with: "`waiting` and `blocked` say why (see the axiom Blocked means it needs the human)" |
| docs/commands.md:2892 | Validate at the boundary, fail loud | borderline | "is not an error: the view starts with an empty map, logs `scroll positions reset: <reason>`" | raise to the User: a corrupt view-state file resets to defaults with a log line, maybe the silent fallback the axiom forbids |

## Counts
restate 1, contradict 0, borderline 1
