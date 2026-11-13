---
status: proposed
supersedes: ADR-0017
---

# ADR 0018: Protocol names a collection of procedures and methods

The User clarified on 2026-11-13 that the repository QA document is the
**QA Protocol**, and that CPD adopts Protocol as the vocabulary for collections
of procedures and methods. The same design conversation explicitly chose
standalone Git/GitHub execution initially, with CPD adoption deferred until
the QA Protocol has proved useful.

## Decision

Update the **Protocol** entry in [CONTEXT.md](../../CONTEXT.md), which remains
the single home of the definition. This refines ADR-0017's definition: skill
delivery and CPD command/hook enforcement describe integration mechanisms,
not prerequisites for naming a collection a Protocol. Preserve ADR-0017's
glossary placement rule and its distinction from the pi adapter's wire protocol.
ADR-0017 remains the historical record of the earlier definition.

Name the operating document [QA Protocol](../qa-protocol.md). Preserve the
existing branch name and attribution marker because they identify work and
counted resources, not the product vocabulary. All other agreed operating
decisions remain in [QA decisions](../qa-decisions.md).

## Alternatives and consequences

- Keeping “QA process” would disregard the User's explicit vocabulary.
- Requiring a CPD skill now would contradict the explicit decision to prove
  the standalone Protocol first.
- Weakening the vocabulary guard is unnecessary; the new prose uses Protocol
  and the guard remains unchanged.

This is a User-defined product vocabulary decision, not an assertion of
industry consensus. The QA research and execution patterns remain sourced in
[issue #1115](https://github.com/clayton-duarte/cpd/issues/1115) and the decision
record. No new runtime behavior or CPD integration is introduced.
