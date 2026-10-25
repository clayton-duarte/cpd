# Axioms audit 1: docs/commands.md lines 1-2111

## Coverage

- Read in full: `docs/commands.md` lines 1-2111 (the audit's part: section 1 Conventions through section 9 `pr`, ending just before `## 10. session` at line 2112). Read with the read tool in nine chunks: 1-210, 211-450, 451-690, 691-950, 951-1210, 1211-1470, 1471-1730, 1731-1990, 1991-2111. Sub-parts read: §1 Conventions, §2 Command index, §3 `context`, §4 `plan`, §5 `job`, §6 `attempt`, §7 `review`, §8 `ask` and `answer`, §9 `pr`.
- Candidate-line pass: `rg -n -i` over lines 1-2111 on the rule words never, always, must, no thing, rule, hygiene, policy, prose is not enforcement, flake, leak, orphan, sleep, mock, external, review, draft, chat, guard, stash, checkout, worktree, supersede, recency, YAGNI, DRY, KISS, idempotent, reversible, upstream, vendor, fork, one source of truth, source of truth, cause, symptom, delete first, boundary, fail loud, illegal state, single, only way, red first, blocked means, undecided design, when unsure, the moment, registered, at once, enforce, prefer, by design; every hit was read in context.
- Judged every candidate against the 29 axiom titles and texts given in the Spec. Not reported: a cpd mechanic that merely obeys an axiom (a command that is idempotent, a Gate that runs tests, a derivation instead of a stored column), a cpd-specific rule that adds detail the axiom does not have (what the Gate runs, which commands a guard refuses), and a decision id cited as the reason for a mechanic (for example the many `hs-d...` citations).
- Per accepted assumption A6, the code's real-tmux and real-git test sites are not reported as findings; nothing in this part's prose states such a test, so A6 removed no candidate here.
- Not read: `docs/commands.md` lines 2112-4626 (§10 `session` to §24 `secret`), which are other audit parts.

## Findings

| Where | Axiom | Kind | Quote | Proposed fix |
|---|---|---|---|---|
| docs/commands.md:320 | Recency wins | restate | "the id of the earlier record this one replaces (recency wins, contract §2.2)" | replace with: "the id of the earlier record this one replaces (see the axiom \"Recency wins\", contract §2.2)" |
| docs/commands.md:1263 | No planned work only in chat | restate | "The Lead registers planned work with a stub the moment it is planned" | replace with: "Register planned work at once with a stub (see the axiom \"No planned work only in chat\")" |
| docs/commands.md:1270 | Decided work skips review | restate | "drafts are for design the User has not decided, and when unsure it is a draft" | replace with: "see the axiom \"Decided work skips review\"" |
| docs/commands.md:1843 | No planned work only in chat | restate | "A follow-up PR is registered at once, even when it cannot run yet" | replace with: "A follow-up PR is registered at once (see the axiom \"No planned work only in chat\")" |
| docs/commands.md:1614 | Blocked means it needs the human | borderline | "it waits for its Lead, not for the User" | raise to the User: the waiting/blocked rule is stated as a parenthetical reason, tied to a cpd Session-state mechanic and citing hs-d303. |
| docs/commands.md:867 | DRY | borderline | "It is derived and holds no information the Plan lacks: it carries no Spec, no Mermaid and no row for a Locked Job." | raise to the User: may be the Plan summary's design constraint, not a restatement of one source of truth. |

## Counts

restate 4, contradict 0, borderline 2
