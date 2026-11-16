# Accepted architectural assessment

**Status:** documentation-based assessment of accepted direction, not a runtime
verification or a new architecture mandate. It records the accepted substance
from decision pro-d16. Upstream remains authoritative; the cited design pages
are evidence for reasoning, not proof that their proposals or described
behaviour are implemented.

## Assessment

A useful architecture preserves continuity: intent, decisions and ownership
should remain recoverable, and current guidance should present amendments
without requiring readers to reconstruct them from history. Authority should
be bounded. Delivery evidence should be meaningful and support end-to-end
outcomes, not just correctness of local state transitions. Where upstream
systems are involved, represent unknown, stale, pending and confirmed states
honestly; make dependency ownership real and recovery usable.

These are assessment criteria, not a prescribed implementation. Database,
daemon, scheduler, outbox, cache, particular state transitions, and module- or
Job-based boundaries are not presumed necessary. For each concrete need,
evaluate upstream alternatives and whole-system recovery, not merely local
transition correctness. Add no mandatory architecture or process without
that justification.

## Evidence and limits

The PR lifecycle scenario describes expected transitions and explicitly
labels source-reading versus tests not run; it is a design page, not a test
result ([PR lifecycle scenario](pr-lifecycle-scenario.md#the-nine-transitions)).
The SWR page describes an architecture for retaining confirmed upstream data,
serving local reads and settling writes; those details are design, not runtime
verification ([SWR](swr.md#1-the-rule)). The store-repair page labels its
proposal as not implemented; it must not be cited as established behaviour
([store repair](store-repair.md#status)). The implementation guide describes
where code belongs, not proof that an architecture is required
([implementing](../implementing.md)).

The contract is the normative documentation authority, including resource
tenancy and its boundaries ([contract §8.3, resources](../contract.md#the-resources-domain));
it does not turn design proposals into verified runtime claims. In
particular, conflicting failed-Job Worker tenancy text is not runtime proof.

This note relies only on the previously inspected documentation set at
`origin/main` commit `2438a512077267382f22be21f7a3200ba050e740`:
[implementing](../implementing.md), [PR lifecycle scenario](pr-lifecycle-scenario.md),
[store repair](store-repair.md), [SWR](swr.md), and [contract §8.3](../contract.md#the-resources-domain).
No further investigation or implementation claim is made here.
