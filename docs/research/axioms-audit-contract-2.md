# Axioms audit 1: docs/contract.md lines 1028-2090 (§4 Storage, §4.1 to §4.6)

## Coverage
- Read in full: `docs/contract.md` lines 1028-2090 (the whole part: `## 4. Storage` at 1028 through the end of §4.6, before `## 5. Events` at 2091), in line chunks 1028-1247, 1248-1467, 1468-1687, 1688-1907, 1908-2065, 2066-2090. Lines 2091-2095 (the opening of §5.1) were read only to confirm the part boundary and were not audited.
- Candidate-line pass: `rg -n -i` over lines 1028-2090 on the rule words axiom, never, always, must, rule, hygiene, prose, flake, leak, orphan, sleep, mock, review, draft, chat, guard, stash, checkout, supersede, recency, YAGNI, DRY, KISS, idempotent, reversible, upstream, vendor, source of truth, only store, cause, symptom, delete first, evidence, verify, retry, flaky, enforce, no test, unrepresentable, explicit, implicit, at the boundary, fail loud, fallback, silent, root cause, until needed, existing package, existing tool, one source; every hit was read in place.
- Judge scope: every candidate was judged against the 29 axiom titles and texts given in the Spec. Passages that apply an axiom to a concrete cpd mechanic (the schema check and its hash, the generated history triggers and the reconcile, the migration runner, the conversion, the delete tombstones, the distill run, the digest) were not reported.
- Judged and not reported (cpd mechanic or scope decision, not a restatement): the "idempotent" statements at 1822, 1985 and 2027 (mechanics that obey Idempotence; the Spec excludes an idempotent command); "the records stay the source of truth" at 2023 (states which artefact is authoritative, not the DRY rule); "Dropping a table listed in `history_tables` is not supported until needed" at 1792 (a cpd scope decision); "to block the root cause of hs-n427" at 2086 (a fix rationale for one incident); the enforcement statements at 1078, 2011 and 2020 (each names a real check, not prose).
- The axiom "Recency wins" is stated again at 2033, 2048 and 2055; it is reported once, at its first line 2013.
- Not reported: the real-tmux/real-git test violations of the mocks axiom (accepted assumption A6: audit 2 sizes that migration); this part holds no such test code.
- Not read: every other line of `docs/contract.md` and every other file; this Job audits only lines 1028-2090.

## Findings
| Where | Axiom | Kind | Quote | Proposed fix |
|---|---|---|---|---|
| docs/contract.md:2013 | Recency wins | restate | "Recency wins: a record superseded by a later one is out of force, and it leaves no trace in a digest." | replace the clause "Recency wins: a record superseded by a later one is out of force" with: see the axiom "Recency wins" |
| docs/contract.md:1791 | Prose is not enforcement | contradict | "No test can enforce that; the author declares it, and a review checks it." | raise to the User: a migration rule that a build test can check is enforced only by declaration and review. |
| docs/contract.md:1055 | Validate at the boundary, fail loud | borderline | "when it cannot be read (the directory was deleted under the process) it is the text `(unreadable)` and the write goes on (hs-d196)" | raise to the User: cannot tell whether recording (unreadable) is the silent fallback the axiom forbids. |

## Counts
restate 1, contradict 1, borderline 1
