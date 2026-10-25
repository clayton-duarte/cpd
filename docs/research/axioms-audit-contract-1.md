# Axioms audit 1: docs/contract.md lines 1-1027 (sections 1-3)

## Coverage
- Read in full: `docs/contract.md` lines 1-1032 (audit scope lines 1-1027: §1 Principles, §2 Model and identifiers, §3 State machines), with the read tool in six chunks: 1-170, 171-460, 461-705, 706-825, 826-902, 903-1032.
- Candidate-line pass: `rg -n -i` over lines 1-1027 on the rule words never, always, must, no thing, rule, hard rule, golden rule, hygiene, policy, prose is not enforcement, flake, leak, orphan, sleep, mock, external, review, draft, chat, guard, stash, checkout, worktree, supersede, recency, YAGNI, DRY, KISS, idempotent, reversible, upstream, vendor, fork, one source of truth, source of truth, cause, symptom, delete first, boundary, fail loud, illegal state, single, only way, red first; each hit was read in context.
- Judged every candidate against the 29 axiom titles and texts given in the Spec. A cpd mechanic that merely obeys an axiom (a command that is idempotent, a Gate that runs tests, derivation instead of a stored column) was not reported.
- Not read: `docs/contract.md` lines 1028-3791 (§4-§10), which are other audit parts.
- Accepted assumptions: A6 says audit 1 does not report the code's real-tmux and real-git test sites; that carve-out is about test code. The `against a real tmux` line reported below is contract prose, not a test site, so it is in audit 1's corpus.
- Citations to the record hs-d303 elsewhere (lines 523, 883) were not counted as separate restatements: they cite the record as the reason for a mechanic, per the Spec. The fresh statement of the rule is at line 826.

## Findings
| Where | Axiom | Kind | Quote | Proposed fix |
|---|---|---|---|---|
| docs/contract.md:95 | Validate at the boundary, fail loud | borderline | "is parsed into types once, at the edge. Unknown flags and unknown config keys are errors. Nothing falls back silently." | raise to the User: the bullet states the axiom almost verbatim but also names the cpd inputs, so it may be the concrete application, not a restatement. |
| docs/contract.md:229 | Recency wins | restate | "The User's rule is **recency wins**: when two records of a Context contradict each other, the later one stands and the earlier one is removed from force" | replace with: see the axiom "Recency wins" |
| docs/contract.md:308 | Decided work skips review | restate | "drafts and their review are for undecided design, and when unsure it is a draft." | replace with: see the axiom "Decided work skips review" |
| docs/contract.md:536 | Convention over configuration | borderline | "The rule is a port of hermes-config `plugins/cockpit/` (`slack_fetch.py`, `refresh.py`, `classify.py`), read-only; each part cites its function" | raise to the User: cannot tell whether porting hermes-config's classify logic is a fork of what upstream owns, or a cross-language rewrite. |
| docs/contract.md:826 | Blocked means it needs the human | restate | "`blocked` means the User has to act; `waiting` means another agent has to" | replace with: see the axiom "Blocked means it needs the human" |
| docs/contract.md:914 | Only authored code is tested | contradict | "the install, run under `LANG=C`, `LC_ALL=C` and an empty `LANG` against a real tmux, passes `watch --check` and restarts the engine" | raise to the User: the contract states an install test runs against a real tmux, against the axiom that every external tool is mocked. |

## Counts
restate 3, contradict 1, borderline 2
