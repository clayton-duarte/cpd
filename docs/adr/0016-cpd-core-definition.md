---
status: accepted
refines: ADR-0002
---

# ADR 0016: cpd's canonical definition lives in README.md

**Accepted, 2026-11-07 (air-d85, air-d86).** cpd states what it is in several places (README.md's opening, CONTEXT.md's opening and **CPD** entry, contract §1.1 and the flow preamble) with no canonical one, and the glossary's Gate entry presents a Gate as only the repo's `pre-merge` hook. The canonical definition is decided and placed once, and the glossary points to it.

## Decision

cpd's canonical definition is exactly this sentence, placed verbatim in README.md's opening:

> cpd (Continuous Parallel Delivery) is a framework for how agents work: it enforces its protocols through tooling rather than prose, gates undesired behaviour, and logs every gate as a metric to improve on.

README.md's opening is the one source of truth. Every other statement of what cpd is points there and never restates the whole sentence. CONTEXT.md, the glossary, gains tight entries for **Gate**, **Guard**, **Enforcement** and **Refusal metric** that point to the README definition, and its **Gate** and **Quick check** entries are amended so a Gate is no longer presented as only the repo's `pre-merge` hook: the hooks are instances of the gating the definition names, and the entries say so while keeping their exact hook mechanics.

The definition's third clause is **design intent, not a built thing**: no refusal Events, refusal metrics or alerts exist today. The refusal-logging work belongs to Plan `agent-refusals`. Today's closest surfaces are the failed-Attempt causes in `cpd metrics` (`cpd_attempts_failed`, `cpd_attempt_retries`) and a `gate_failed` Attempt's four gate columns (contract §4.6, commands.md §14.7); nothing in this decision claims more than that.

**ADR-0002's Gate+Reviewer invariant is superseded in its Reviewer half.** ADR-0002 records that an Attempt reaches the Lead Branch only after it passes the Gate and a Reviewer approves it. The Reviewer was removed by R19 — the review is the pull request review on GitHub — so that invariant reads as live machinery that does not exist. It is superseded here: the Gate survives as one instance of the gating the canonical definition names, and the review gate is the pull request review. ADR-0002's text stays as history, unedited.

## Considered options

- **Put the definition in CONTEXT.md or contract.md instead.** Rejected: CONTEXT.md is a glossary of terms, not the product's self-description, and the contract is a read-only normative reference this decision does not amend; README.md is where a reader meets the project first.
- **Rewrite every page that states what cpd is.** Rejected: wholesale edits to contract.md, commands.md and the skills are out of scope and churn history; pointing is cheaper and keeps one source of truth.

## Consequences

- contract.md and commands.md stay read-only references; this decision edits only README.md, CONTEXT.md and this ADR.
- The wording of the definition is frozen by this ADR: a change to it is a new ADR that supersedes this one, not an edit in place.
