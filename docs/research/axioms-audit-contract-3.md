# Axioms audit 1: docs/contract.md lines 2091-3116 (section 5 Events to section 7)

## Coverage
- Read in full: `docs/contract.md` lines 2091-3116, the whole part (section 5 Events and its subsections 5.1-5.4, section 6 The tmux host with 6.1 through 6.10, the Dango palette 6.1.1, the input widget, chat balloons, and the opening paragraph of section 7), in four contiguous chunks: 2091-2290, 2291-2515, 2516-2735, 2736-3116. No line of the part is unread.
- Candidate-line pass: `rg -n -i` over the same range on the rule words never, always, must, no <thing>, rule, idempotent, reversible, retry, flake, leak, orphan, sleep, mock, guard, evidence, in practice, enforce, prose, source of truth, single source, one source, cause, symptom, vendor, upstream, copy of, fork, by hand, manual, and every `unverified`/`assumption` note; every hit was read in context.
- Judge scope: every candidate was judged against the 29 axiom titles and texts given in the Spec. cpd mechanics that merely obey an axiom (idempotent server setup at 2255, polling in 5.4, idempotent config writes, the Gate checks, the tmux guards) are not reported, nor are the many passages that state a cpd-specific detail an axiom does not have (colours, glyphs, formats, paths, record ids as reasons).
- The copy of `pi-compact-tools` is stated at 2970 and again in 6.1.1 and 6.9; the copy of `@juicesharp/rpiv-ask-user-question` is stated at 3105 and again at 2741; each rule is reported once, at its fullest statement.
- Not reported per accepted assumption A6: the test sites that use a real tmux or real git (for example the real-tmux defect test at 2233 and the tmux Evidence at 2345) are the mocks axiom's violations, sized by audit 2, not audit 1 findings.
- Not read: nothing in this part. Lines outside 2091-3116 belong to other parts and were not judged.

## Findings
| Where | Axiom | Kind | Quote | Proposed fix |
|---|---|---|---|---|
| docs/contract.md:2970 | Convention over configuration | contradict | "an extension cpd owns: a copy of `pi-compact-tools` (nedleeds, MIT) 0.16.6 with the fold language added, shipped inside the adapter" | raise to the User: cpd copies and ships upstream pi-compact-tools instead of taking the npm dependency, against the axiom. |
| docs/contract.md:3105 | Convention over configuration | contradict | "an extension cpd owns: a copy of `@juicesharp/rpiv-ask-user-question` (MIT) 2.13.8, shipped inside the adapter in `pi/ask-user-question/`" | raise to the User: cpd copies and ships a third-party pi package instead of taking the dependency, against the axiom. |
| docs/contract.md:2881 | Guards only for proven bad behaviour | borderline | "`git stash` is refused in cpd Sessions (hs-d310): refs/stash is shared by every worktree of a repo; commit on your branch instead." | raise to the User: a guard on git, a raw tool, cites shared-refs risk, not evidence of misbehaviour seen in practice. |

## Counts
restate 0, contradict 2, borderline 1
