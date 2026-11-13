# Attention inbox

The attention inbox is cpd's **single source of "needs you"**: one stored item per thing that needs the User, opened and closed by the engine from its cause. It is the second of the two queue-like concepts cpd keeps by design (`hs-d802` (a record of machine `hs`); verified in [architecture-verification.md §9](../research/architecture-verification.md)).

## Purpose

Keep the User on top of what matters, and nothing else. The Inbox is shown by `cpd inbox` and the cockpit's Inbox section; its colours are urgency names ([CONTEXT — Inbox](../CONTEXT.md)). A thing that needs the User becomes exactly one **Inbox item** — a dialog, a Plan review, an unread reply, a pull request of the User, a deploy awaiting approval, a curation proposal, the engine being down, and the other kinds of [contract §2.2](../contract.md) "Inbox item" — and stops being one when its condition ends.

## Producer, consumer, guarantee

- **Producer** — the engine, at **engine step `inbox`**, which opens an item when its condition starts and closes it when the condition ends; **nobody else does** ([contract §2.2](../contract.md) "Inbox item", [contract §3.7](../contract.md); [commands §21](../commands.md)). An item's identity is (`kind`, `subject`, `urgency`), so a colour change closes the row and opens a new one ([contract §2.2](../contract.md)). `engine-down` is the exception that is **not stored**: the engine cannot report its own absence, so `cpd inbox list` adds it first when no live Manager exists ([contract §2.2](../contract.md); [commands §21.1](../commands.md)).
- **Consumer** — the User, through `cpd inbox list` and the cockpit. The commands **read** the items and **hide** them: `inbox snooze` ([commands §21.3](../commands.md)) and `inbox dismiss` ([commands §21.4](../commands.md)) write `snoozed_until` and `dismissed_at`. The one command that closes an item is `cpd inbox approve`, and only a `deploy` item's run ([commands §21](../commands.md), [commands §21.13](../commands.md)).
- **Purpose** — the User never has to poll: the engine reconciles the Inbox every tick and the cockpit draws it.

The Inbox owns only what the User must act on. Escalations from Workers go to their Lead, a stuck Job and an engine step error go to the Context's Manager as a message, and an unanswered Context question raises nothing ([contract §2.2](../contract.md) "Inbox item"). Curation proposals from the knowledge plane reach the User here, as the `curate-retire` item ([[knowledge-plane]]); the documentation candidates that a `curate-docs` item once carried are gone (pro-d327).

## Guarantee

**Guarantee: one stored item per thing that needs the User, opened and closed by the engine from its cause — never by a command, except `inbox approve` closing a `deploy` item.** A snooze or a dismissal hides an item, it does not decide anything; a dismissed `curate-retire` item leaves the underlying proposals `pending` ([contract §2.2](../contract.md)). A closed item is kept forever ([contract §2.2](../contract.md)).

## The reviewed PR of a review Plan

A review Plan (a Plan of Context `codereview`, made by `inbox claim`) reviews one GitHub pull request, stored on the Plan as `reviewed_pr` (contract [§2.2](../contract.md) "Plan", decision pro-d412). Its Inbox items use it in the three ways below, and in no other:

- **The url rule.** A `dialog` item (contract [§2.2](../contract.md) "Inbox item") whose Session's Plan has a `reviewed_pr` carries `url` = `https://github.com/<owner>/<repo>/pull/<n>`, the Plan's `reviewed_pr.url`. `inbox` reads it from the Plan's stored columns when it opens the item. A `dialog` of a Session whose Plan has no `reviewed_pr` keeps `url` null, as before. Every other kind keeps its own url rule. The url is not part of the item's identity.
- **The primary click stays Go to Lead.** A `dialog` row is no link kind: no `Link` is drawn on its label (TUI `is_link`, web `linkKinds`), and a left click selects its Session and Plan as before. In the TUI that is `session open` of the item's Session, then `select-pane` on the chat slot ([commands §13.2](../commands.md), "Clicks on an Inbox item"). On the web it selects the row's Plan and Session ([DESIGN-panes §2.6](../../web/src/ui/DESIGN-panes.md)).
- **The menu gets two entries.** **Open Link** and **Copy Link** come first on the row, before Snooze and Dismiss, and show only when `url` is set. Open Link opens `url` in the default browser: the TUI through `link::open_url`, the web in a new tab (`noopener,noreferrer`). Copy Link copies `url`: the TUI with `tmux set-buffer` (`Opener::copy_link`), the web to the clipboard. The menus themselves are in [commands §13.2](../commands.md) ("The Inbox's right-click menu") and [DESIGN-panes §2.3](../../web/src/ui/DESIGN-panes.md); this section is the rule they follow.
- **The wake of the review Lead.** The Lead of a review Plan is woken, not only when the User re-claims the item (contract [§2.2](../contract.md) "The team review rule", commands [§21.7](../commands.md)), but also by the GitHub pass that sees the claimed pull request's head move. The pass appends one `session-message` to the Lead (`data.from` null, `wake` true) in the transaction that stores the new head, when the head differs from the head the item was claimed at (`inbox_items.head_oid`) and the pull request's checks are `passed`. Its `data.head_oid` is the new head, so one head wakes the Lead once and a second check run on the same head wakes nothing. The rule and its test are [swr-github.md](swr-github.md) §10 (b).

## PR merge actions and Inbox item removal

When the User triggers a PR merge via the cockpit, the server queues the merge and responds with an `action.result` message carrying `ok: true` and `output.queued: true`. The client receives this action result and must wait for the Inbox item to be removed when the merge completes and the next Inbox snapshot reflects the merged state.

**Timer and armed flag.** When the client receives an action result with `ok: true` and no optimistic action exists, it starts a 10-second timer; if the Inbox item is not removed (the subject is not dropped from `inbox.rows`) within 10 seconds, the client reports an error to the User: "Inbox did not confirm the action within 10 seconds" ([b64](https://github.com/clayton-duarte/cpd/issues/1206)). However, when the server's action output carries `armed: true`, the client does **not** start this removal timer, because `armed: true` signals that the Inbox item will remain in updating state until the queued mutation completes and the Inbox is reconciled. Therefore, **a merge that the server accepted as queued never shows 'Inbox did not confirm the action within 10 seconds'**: the server returns `armed: true` in the action output (contract [§2.2](../contract.md) "action result") when the merge is queued and the Inbox item update is deferred, and the client respects this signal by omitting the removal timer.

## Approval dialogs

Four dialogs stop a session until the User answers: `cpd_secret_approve`, `cpd_external_approve`, `cpd_host_approve` and `ask_user_question` (decision pro-d580). Each one is an Inbox item while it waits, and each is answered in the Inbox or in the WebUI.

**Status: decided, not yet built** at base commit `b9b4aaec6`. Today the engine's `dialogs` rule (`core/src/inbox/rules.rs`) labels a waiting session `<who> asks you`, and a `dialog` row offers only Snooze and Dismiss. The rules below are the target.

- **The Inbox item.** While a session waits on one of the four dialogs, the engine opens one `dialog` item for it at engine step `inbox` (the rule above), labelled `<agent> waits for your approval: <what>`. `<agent>` is the waiting session and `<what>` is what the dialog asks the User to decide. The item closes when the session stops waiting on that dialog, by an answer or by the dialog ending.
- **Dual resolution (assumption A2).** An answer in the Inbox and an answer in the WebUI reach the same held dialog. The host holds one request per dialog (`pi/durable-host/held.ts`), and the first answer wins; the other surface then sees the dialog resolved and its item closes.
- **The WebUI composer (assumption A3).** While a session waits on a dialog, the WebUI replaces the message composer with the dialog card, and restores the composer when the dialog resolves. The swap is already in `web/src/ui/ChatPane.tsx`, which draws the card in place of the composer whenever a dialog is open.

## Related notes

- [[knowledge-plane]] — the curation this Inbox decides (`curate-retire`; `curate-docs` is history-only, pro-d327).
- [[message-board]] — the other queue-like concept, which never reaches the User.
- [[clocks]] — the engine loop whose `inbox` reconciles the Inbox.
- [[sdk]] — the store the `inbox_items` table lives in.

## Sources

- [commands.md](../commands.md) — §21 (`inbox`), §21.1 (`inbox list`), §21.3 (`inbox snooze`), §21.4 (`inbox dismiss`), §21.13 (`inbox approve`).
- [commands.md](../commands.md) — §23.7 (`secret approve`, the tool `cpd_secret_approve`).
- Decision pro-d580 (approval dialogs in the Inbox and WebUI, dual resolution); Plan `approvals-ui`, Job `approvals-docs`.
- [contract.md](../contract.md) — §2.2 "Inbox item", §3.7 (engine, `inbox`).
- [CONTEXT.md](../CONTEXT.md) — Inbox, Inbox item.
- [architecture-verification.md](../research/architecture-verification.md) — §9 (the two queue-like concepts).
- Model: `hs-d802` (a record of machine `hs`; see [INDEX](INDEX.md)).
