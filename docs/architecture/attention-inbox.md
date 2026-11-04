# Attention inbox

The attention inbox is cpd's **single source of "needs you"**: one stored item per thing that needs the User, opened and closed by the engine from its cause. It is the second of the two queue-like concepts cpd keeps by design (`hs-d802` (a record of machine `hs`); verified in [architecture-verification.md §9](../research/architecture-verification.md)).

## Purpose

Keep the User on top of what matters, and nothing else. The Inbox is shown by `cpd inbox` and the cockpit's Inbox section; its colours are urgency names ([CONTEXT — Inbox](../CONTEXT.md)). A thing that needs the User becomes exactly one **Inbox item** — a dialog, a Plan review, an unread reply, a pull request of the User, a deploy awaiting approval, a curation proposal, the engine being down, and the other kinds of [contract §2.2](../contract.md) "Inbox item" — and stops being one when its condition ends.

## Producer, consumer, guarantee

- **Producer** — the engine, at **engine step E8**, which opens an item when its condition starts and closes it when the condition ends; **nobody else does** ([contract §2.2](../contract.md) "Inbox item", [contract §3.7](../contract.md); [commands §21](../commands.md)). An item's identity is (`kind`, `subject`, `urgency`), so a colour change closes the row and opens a new one ([contract §2.2](../contract.md)). `engine-down` is the exception that is **not stored**: the engine cannot report its own absence, so `cpd inbox list` adds it first when no live Manager exists ([contract §2.2](../contract.md); [commands §21.1](../commands.md)).
- **Consumer** — the User, through `cpd inbox list` and the cockpit. The commands **read** the items and **hide** them: `inbox snooze` ([commands §21.3](../commands.md)) and `inbox dismiss` ([commands §21.4](../commands.md)) write `snoozed_until` and `dismissed_at`. The one command that closes an item is `cpd inbox approve`, and only a `deploy` item's run ([commands §21](../commands.md), [commands §21.13](../commands.md)).
- **Purpose** — the User never has to poll: the engine reconciles the Inbox every tick and the cockpit draws it.

The Inbox owns only what the User must act on. Escalations from Workers go to their Lead, a stuck Job and an engine step error go to the Context's Manager as a message, and an unanswered Context question raises nothing ([contract §2.2](../contract.md) "Inbox item"). Curation proposals from the knowledge plane reach the User here, as the `curate-retire` item ([[knowledge-plane]]); the documentation candidates that a `curate-docs` item once carried are gone (pro-d327).

## Guarantee

**Guarantee: one stored item per thing that needs the User, opened and closed by the engine from its cause — never by a command, except `inbox approve` closing a `deploy` item.** A snooze or a dismissal hides an item, it does not decide anything; a dismissed `curate-retire` item leaves the underlying proposals `pending` ([contract §2.2](../contract.md)). A closed item is kept forever ([contract §2.2](../contract.md)).

## Related notes

- [[knowledge-plane]] — the curation this Inbox decides (`curate-retire`; `curate-docs` is history-only, pro-d327).
- [[message-board]] — the other queue-like concept, which never reaches the User.
- [[clocks]] — the engine loop whose E8 reconciles the Inbox.
- [[sdk]] — the store the `inbox_items` table lives in.

## Sources

- [commands.md](../commands.md) — §21 (`inbox`), §21.1 (`inbox list`), §21.3 (`inbox snooze`), §21.4 (`inbox dismiss`), §21.13 (`inbox approve`).
- [contract.md](../contract.md) — §2.2 "Inbox item", §3.7 (engine, E8).
- [CONTEXT.md](../CONTEXT.md) — Inbox, Inbox item.
- [architecture-verification.md](../research/architecture-verification.md) — §9 (the two queue-like concepts).
- Model: `hs-d802` (a record of machine `hs`; see [INDEX](INDEX.md)).
