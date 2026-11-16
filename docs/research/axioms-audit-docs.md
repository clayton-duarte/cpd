# Axioms audit 1: CONTEXT.md and the smaller docs

## Coverage
- Read in full, with the `read` tool, every file of this part: `CONTEXT.md` 1-282; `README.md` 1-51; `BACKLOG.md` 1-8; `docs/implementing.md` 1-442 (chunks 1-223, 224-429, 430-442); `docs/interface.md` 1-137; `docs/cockpit-parity.md` 1-227; `docs/versioning.md` 1-45; `docs/contract.md#37b-the-serve-process-runtime-and-port-supervision` 1-33; `ideas/dango-spinner.sh` 1-15.
- Candidate-line pass: `rg -n -i` over all nine files on the rule words never, always, must, no thing, rule, hygiene, policy, prose, flake, leak, orphan, sleep, mock, external, review, draft, chat, guard, stash, checkout, supersede, recency, YAGNI, DRY, KISS, idempotent, reversible, upstream, vendor, fork, copy of, source of truth, single source, one source, cause, symptom, evidence, unverified, infer, fallback, silent, default, retry, rerun, skip, weaken, by hand, manual, primary checkout, main checkout, speculative, later; every hit was read in place.
- Judged every candidate against the 29 axiom titles and texts given in the Spec. cpd mechanics that merely obey an axiom (the one-area rule, the tripwire and portability checks, the leak check, the idempotent install, "safe to rerun", the area table as the one source of truth, "the records stay the truth", the `answers.ts` one source of truth) were not reported, nor were the passages that state a cpd-specific detail an axiom does not have (paths, formats, commands, record ids cited as the reason for a mechanic).
- `docs/implementing.md:132` and `docs/implementing.md:217` both restate Tests are the proof; reported once, at 132.
- The two copies of upstream pi packages are stated in two separate sections of `docs/implementing.md` (the compact-tools copy and the ask-user-question copy); each is reported once, at its fullest statement.
- Not read and not judged: `docs/contract.md`, `docs/commands.md`, `docs/research/`, `docs/archive/`, `docs/adr/` (other audit parts or snapshots), and every other file of the repo. This part holds no Context records, so there is no record group to report.
- Accepted assumptions: A4 (scope) and A6 (the code's real-tmux and real-git test sites are sized by audit 2). The real-tmux lines reported below are `docs/implementing.md` prose about tests, not test code; A6's carve-out is about the test sites themselves, so this prose is in audit 1's corpus (as the contract audit read the same distinction).

## Findings
| Where | Axiom | Kind | Quote | Proposed fix |
|---|---|---|---|---|
| CONTEXT.md:158 | Blocked means it needs the human | restate | "waiting (an agent waits for another agent: a Worker for its Lead's answer), blocked (the User has to act: a dialog, a pending Plan review)" | replace the parenthetical with: see the axiom "Blocked means it needs the human" |
| docs/implementing.md:132 | Tests are the proof | restate | "A test must fail first, for the right reason." | delete line 132 or replace with: see the axiom "Tests are the proof" (line 217 states the same rule) |
| docs/implementing.md:138 | No flakes, no leaks, no orphans, no sleep, no errors | borderline | "never a fixed sleep to get a result; waits poll a positive signal." | raise to the User: the sentence restates the no-sleep axiom inside a speed rule; decide if it should point to the axiom instead |
| docs/implementing.md:240 | Only authored code is tested | contradict | "with the real tmux, the install against a real cockpit passes and every tmux line the fake logs begins `tmux -u`." | raise to the User: an install test runs against the real tmux, against the axiom that every external tool is mocked |
| docs/implementing.md:352 | No flakes, no leaks, no orphans, no sleep, no errors | contradict | "failed one test at load 34-50 (the test's name was not kept); two reruns at load 40-42 passed (unverified: a load flake)." | raise to the User: a flaky test is rerun and left as an unverified load flake, against the axiom's zero tolerance |
| docs/implementing.md:368 | Convention over configuration | contradict | "**1. The copy.** `pi/compact-tools/extensions/` ← the fork's `extensions/*.ts` but `compact-tools-release.ts`" | raise to the User: cpd copies and ships upstream pi-compact-tools instead of taking the npm dependency, against the axiom |
| docs/implementing.md:390 | Convention over configuration | contradict | "**1. The copy.** `pi/ask-user-question/` ← the 43 `.ts` files of the package under the same relative names" | raise to the User: cpd copies and ships a third-party pi package instead of taking the dependency, against the axiom |

## Counts
restate 2, contradict 4, borderline 1
