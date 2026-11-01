# Message board

The message board is cpd's **agent-to-agent exchange**: a Session posts a short message to a target Session, the target reads it at its next turn, and the exchange leaves the Context registry clean because it writes no Context record and appends no Event. It is one of the two queue-like concepts cpd keeps by design (`hs-d802`, `cpd context show --record hs-d802`; verified in [architecture-verification.md §9](../research/architecture-verification.md)).

## Purpose

A Session that learns something another Session needs — "the pull request opened", "the Inbox item closed" — tells it through the board rather than by writing a Context record it does not warrant, or by waking it. **Important facts stay Context records** ([commands §10a](../commands.md)); the board carries the rest. The board is a SQLite table whose posts name who they are for, read by `cpd board list` and picked up at an agent's turn start by `cpd board take` ([commands §10a](../commands.md)).

## Producer, consumer, guarantee

- **Producer** — the sending Session. `cpd board post <text> --to <ref>` writes one `board_posts` row, addressed to `manager` (the live Manager of the sender's Context, else `no_manager`), a Plan id (its live Lead Session; a Plan in phase `done` or `archived` is refused `plan_inactive`) or a Session id ([commands §10a.1](../commands.md)). The row keeps the sender's Session (`from_session`, null when there is none), the target's Context, the text and a null `read_at`.
- **Consumer** — the target Session. `cpd board list` reads its unread posts, oldest first ([commands §10a.2](../commands.md)), and `cpd board take --session <id>` returns the unread posts and marks them read in the same transaction; it is internal, the pi adapter's message picker at turn start ([commands §10a.3](../commands.md); [commands §15.7](../commands.md)).
- **Purpose** — keep the Context registry clean: the post is a message, not a record ([CONTEXT — Board post](../CONTEXT.md)).

## Never starts a turn

`cpd board post` **appends no Event and never starts a turn** ([commands §10a.1](../commands.md); `hs-d514`, `hs-d802`). The target reads the post only when it is already taking a turn (`cpd board take`); the board cannot wake an idle Session. That is what makes it safe for routine agent-to-agent traffic.

## Guarantee

**Guarantee: a board post is delivered at the target's next turn and only then — it appends no Event and never starts a turn.** Reading marks the post read; a second `cpd board take` returns an empty list and changes nothing ([commands §10a.3](../commands.md)); `cpd board post` is not idempotent, each call is one post ([commands §10a.1](../commands.md)).

The board is distinct from the two other queue-like things cpd keeps: the truthful store of what the teams know is the Context records ([[knowledge-plane]]), and what needs the User is the Inbox ([[attention-inbox]]); the board is neither.

## Related notes

- [[knowledge-plane]] — the Context records the board does not write.
- [[attention-inbox]] — the other queue-like concept, which does reach the User.
- [[clocks]] — the engine loop the board deliberately does not enter.
- [[sdk]] — the store the `board_posts` table lives in.

## Sources

- [commands.md](../commands.md) — §10a (`board`), §15.7 (the adapter's turn start).
- [CONTEXT.md](../CONTEXT.md) — Board post.
- [architecture-verification.md](../research/architecture-verification.md) — §9 (the two queue-like concepts).
- Model: `hs-d802`, `hs-d514` (`cpd context show --record hs-d802`).
