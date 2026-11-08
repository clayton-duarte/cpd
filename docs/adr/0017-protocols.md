---
status: accepted
refines: ADR-0016
---

# ADR 0017: A protocol's definition lives in the glossary

**Accepted, 2026-11-08 (air-d101, air-d85).** The User has defined what a protocol is (air-d101), and cpd's canonical definition (air-d85) names protocols without defining them ("it enforces its protocols through tooling rather than prose", README.md line 5). The word has no home: nothing in the repo says what a protocol is, and the only enforcement of any protocol today is prose in agent skills, which air-d85 says cpd must not rely on. The definition of a protocol is stated once, in the glossary, and every other mention points there.

## Decision

A protocol is what air-d101 defines: a rule the User defines as a procedure that must be followed when it is invoked — called with prose, reaching the agent as a skill, and enforced by commands and hooks that exist as guardrails.

That definition is stated exactly once, in the **Protocol** entry of CONTEXT.md's `## Checks and gates` section. The entry points outward and never restates cpd's canonical definition: the sentence that names "its protocols" lives verbatim in README.md line 5 (air-d85) and stays there, per ADR-0016's placement rule (air-d86) — README owns cpd's self-description, the glossary owns terms, and a glossary entry points to the canonical sentence rather than repeating it. This ADR records the decision; the glossary entry cites it.

The word has a second sense that must stay apart: the pi adapter's wire protocol (commands.md §15). It keeps its own qualified name; the glossary's Avoid line on **Protocol** sends readers there. The senses are never merged.

As with the refusal metric in ADR-0016, part of this definition is design intent, not a built thing: no protocol is yet enforced by cpd's commands and hooks. The bug report protocol exists only as prose in the cpd-manager and cpd-lead skills. Plan `protocol-enforce` brings the protocols up to this definition, starting with the bug report protocol enforced by commands and hooks (air-b30); nothing in this decision claims more than that.

## Considered options

- **State the protocols definition in README.md, beside the canonical sentence.** Rejected: README.md line 5 is cpd's own definition, frozen by ADR-0016, and adding a second definition there would restate what the glossary must own and break air-d86's one-source rule.
- **Restate the canonical definition sentence in every mention of "protocol".** Rejected: each mention points to README.md line 5 instead; restating it in CONTEXT.md and here would create a second copy that drifts.
- **Define a protocol in the contract or commands.md.** Rejected: they are read-only normative references (ADR-0016's Consequences); the glossary is where terms are defined, and an entry points to them for how a thing works.

## Consequences

- README.md stays unchanged; this decision edits only CONTEXT.md and this ADR.
- The wording of the protocols definition is frozen by this ADR: a change to it is a new ADR that supersedes this one, not an edit in place.
- The enforcement the definition names — commands and hooks that refuse — is separate work in Plan `protocol-enforce`; this ADR decides only where the definition is stated and how other mentions point to it.
