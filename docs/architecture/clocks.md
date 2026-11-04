# The clocks

cpd runs two clocks, not one (`hs-d802` (a record of machine `hs`), item 5): the **engine loop clock** (`watch.tick_seconds`) and the **GitHub poll clock** (engine step E5, `watch.pr_poll_seconds`). They are decoupled on purpose: the engine sleeps until the earlier of its next tick and E5's due time, so the poll is never held to a tick boundary ([contract §3.7](../contract.md)). Both keys live in the `[watch]` domain of `$CPD_HOME/config.toml` ([contract §8.2](../contract.md)); the loop they belong to is the one `cpd watch` per machine ([CONTEXT — Engine](../CONTEXT.md), [commands §12](../commands.md)).

This note documents the model in hs-d802 against today's docs. Where the model and the docs disagree, the discrepancy is named in place and sourced to [architecture-verification.md](../research/architecture-verification.md), which verified `hs-d802` claim by claim.

## The engine loop clock: `tick_seconds`

The engine runs its loop every `tick_seconds` — a decimal, minimum `0.1`, default `5` ([contract §3.7](../contract.md), [contract §8.2](../contract.md)). Each tick runs the engine's numbered steps in order: **E1–E21**, with **E6 removed** and E21 (the closed-pull-request wake) right after E20 ([contract §3.7](../contract.md)); the command reference lists the same set ([commands §12](../commands.md)). A tick starts at the earliest of three things ([contract §3.7](../contract.md)):

- `tick_seconds` after the previous tick began (the longest wait);
- **E5's due time**, when the poll comes due;
- **500 ms after the previous tick began** when another process other than the engine committed to the store — the engine sees this by comparing SQLite's `PRAGMA data_version` at each 100 ms nap.

So ticks come at most twice a second, and the Inbox (E8) and the Workflow starts (E7) follow another process's commit within 500 ms plus the tick's own time; `tick_seconds` stays the longest wait ([contract §3.7](../contract.md)).

The engine clock is **purely internal**: it is the `cpd watch` tick loop ([contract §3.7](../contract.md), [CONTEXT — Engine](../CONTEXT.md)). The loop schedules every Plan's Jobs under one concurrency limit (E3), watches liveness and timeouts (E2), starts a Workflow once the Workflows it needs are merged (E7), reads and writes the Inbox (E8), curates Context records (E9) and titles Sessions (E10) ([CONTEXT — Engine](../CONTEXT.md), [contract §3.7](../contract.md)).

> **Model vs docs.** hs-d802 calls the loop "E1-E21 except E5" and says it "moves work from the Context phase to Dispatch." The docs put E5 *in* the step set (it is an E-step that runs at its own due time), remove E6, and describe the Context → Plan → Dispatch progression as the Manager's and the Lead's work, not the tick's ([architecture-verification.md](../research/architecture-verification.md), item 2). The step range is also stale in [CONTEXT — engine step](../CONTEXT.md), which says "E1 to E17" ([architecture-verification.md](../research/architecture-verification.md), item 2). This note follows the contract.

```mermaid
sequenceDiagram
    participant Clock as tick_seconds
    participant Eng as cpd watch (engine)
    participant Store as cpd.db (the ledger)
    Clock->>Eng: tick begins (every tick_seconds, min 0.1, default 5)
    Eng->>Store: E1–E21 in order (E6 removed; E21 after E20)
    Store-->>Eng: rows and Events committed
    Note over Eng,Store: E2 liveness · E3 schedule Jobs · E4 open Jobs<br/>E7 start Workflows · E8 Inbox · E9 curate · E10 titles
    Eng->>Eng: sleep until next tick or E5's due time
    Note over Eng: a tick starts at the earliest of the next tick,<br/>E5's due time, and 500 ms after another process's commit
```

## The GitHub poll clock: `pr_poll_seconds` (E5)

Engine step E5 runs `cpd pr poll` (with no `--plan`) as a **subprocess** — a process boundary — `pr_poll_seconds` after the **end** of the previous poll, **never overlapping**, and **at its due time, not at the next tick** ([contract §3.7](../contract.md), [contract §8.2](../contract.md), [commands §9.2](../contract.md)). `pr_poll_seconds` defaults to `5` ([contract §8.2](../contract.md)). A poll of 1.3 s with `pr_poll_seconds = 5` and `tick_seconds = 5` runs every 6.3 s, not every 10 s, whether or not any Plan is in `deploy` ([contract §3.7](../contract.md)).

One poll covers **every open pull request of the User**, any repo on github.com ([contract §7.6](../contract.md), [commands §9.2](../contract.md)); [CONTEXT — PR poll](../CONTEXT.md) calls it "the engine's poll, every 5 s, of the open pull requests of the User on GitHub, one search through `gh`." It watches the **external GitHub state after dispatch** and drives cpd's response ([contract §3.7](../contract.md) E5, [contract §7.6](../contract.md)):

- it stores each pull request in the `pull_requests` table and appends `pr-feedback` for the Workflow that owns it (checks failed, changes requested, closed, approved, comments) ([contract §7.6](../contract.md), [commands §9.2](../commands.md));
- when a pull request's `mergeable` becomes `conflicting` — or a merged base repoints it — it inserts an `update` Job for the Workflow ([contract §3.7](../contract.md) E5, [contract §7.6](../contract.md), [commands §9.2](../commands.md));
- it records a merge (`pr-merged`), unstacking and base follows, and the Plan's `done` ([contract §7.6](../contract.md), [commands §9.2](../commands.md));
- it reads the rate limit from the search: below 100 remaining, E5 skips polls until `rate_limit.reset_at` and reports one `error` Event with code `gh_rate_limited` ([contract §3.7](../contract.md) E5);
- a transient `gh` failure is tolerated for `watch.pr_poll_transient_minutes` (default `10`) before E5 raises an error ([contract §3.7](../contract.md) E5, [contract §8.2](../contract.md), [commands §9.2](../commands.md)).

The Lead can also run `cpd pr poll` by hand, and a Workflow's Lead does so right after `pr open`; the tolerance above is the **engine's**, not the command's — a hand poll fails at once ([commands §9.2](../commands.md)).

> **Model vs docs.** hs-d802 says the two clocks have "different timings on purpose." The docs give both keys the same default (`5`), so what differs is the *clock* (E5 runs `pr_poll_seconds` after the end of the previous poll, not at the next tick), not the default interval ([architecture-verification.md](../research/architecture-verification.md), item 2; [contract §8.2](../contract.md), [contract §3.7](../contract.md)).

```mermaid
sequenceDiagram
    participant Eng as cpd watch (engine E5)
    participant Poll as cpd pr poll (subprocess)
    participant GH as GitHub
    participant Store as cpd.db
    Eng->>Poll: spawn at E5's due time (pr_poll_seconds after the last poll ended)
    Poll->>GH: gh api graphql — every open pull request of the User
    GH-->>Poll: state, checks, reviews, mergeable, base
    Poll->>Store: pull_requests rows · pr-feedback · update Jobs · merges
    Poll-->>Eng: output (rate_limit.remaining, reset_at)
    Note over Eng,Poll: below 100 remaining → skip until reset_at (gh_rate_limited)<br/>transient failures tolerated for pr_poll_transient_minutes
    Note over Eng,Poll: never overlapping; a 1.3 s poll + 5 s/5 s runs every 6.3 s
```

## The repeat bump clock: 86 400 s (E23)

The repeat bump (pro-d382; [contract §4.4](../contract.md) "The repeat bump"; [contract §3.7](../contract.md) E23) was to bump an unreviewed posted pull request again once 24 hours had passed since its last post. Nothing does so today: E23 sends nothing and writes nothing (pro-d386). The later change that raises an Inbox item with a RePost action (pro-d386) keeps this clock. It is a fixed clock: `BUMP_INTERVAL_SECS` = 86 400 is a constant in the code, not a `[watch]` key, and no config changes it. A row is due when `COALESCE(last_posted_at, posted_at)` is at most 86 400 seconds before now, compared as text in the format `posted_at` has. No step writes `last_posted_at` today ([contract §3.7](../contract.md) E23).

## How the two clocks interleave

The clocks are independent by construction. The engine sleeps until the **earlier** of its next tick and E5's due time, so E5 is not held back to a tick boundary ([contract §3.7](../contract.md)). The two meet at two seams:

- **E7 runs after E5 in the same tick** ([commands §12](../commands.md), [contract §3.7](../contract.md)): a pull request starts in the tick after E5's poll result is applied, within `EARLY_GAP` (500 ms) of the poll's commit ([contract §3.7](../contract.md) E7).
- **A commit wakes a tick**: when another process (the poll child, a CLI command) commits to the store, the engine starts its next tick within 500 ms, so E8's Inbox and E7's Workflow starts react to a poll's writes promptly ([contract §3.7](../contract.md)).

The engine's time-only rules (a Job stuck, a snooze's end, liveness) still wait for `tick_seconds` ([contract §3.7](../contract.md)). This is the deliberate interleave: an internal loop pacing cpd's own work, and a separate poll pacing cpd's view of GitHub ([contract §3.7](../contract.md), hs-d802).

```mermaid
sequenceDiagram
    participant Eng as cpd watch (engine)
    participant Tick as tick_seconds
    participant Poll as E5 pr poll
    participant GH as GitHub
    Eng->>Eng: tick begins
    Eng->>Eng: E1–E4, E7, E8–E21 (E6 removed)
    Poll->>Eng: E5 comes due (not held to a tick boundary)
    Eng->>Poll: spawn cpd pr poll
    Poll->>GH: read open pull requests
    GH-->>Poll: external state
    Poll->>Eng: rows committed
    Note over Eng: the commit wakes a tick within 500 ms
    Eng->>Eng: E7 starts Workflows whose needs are merged (after E5, same tick)
    Eng->>Tick: sleep until earlier of next tick and E5's due time
    Poll->>Poll: next poll pr_poll_seconds after its end
```

## Related notes

The engine loop and the poll are served by the same core and the same store: [[sdk]]. Who acts on each clock is [[roles]]; what the poll drives on GitHub is [[verification]]; and the interfaces that read the store are [[interfaces]]. The agent processes these clocks start are [[agent-runtime]].

## Sources

- [contract §3.7](../contract.md) — the engine, `cpd watch`, the E-steps, E5's due time
- [contract §8.2](../contract.md) — `tick_seconds`, `pr_poll_seconds`
- [contract §7.6](../contract.md) — the PR poll
- [commands §12](../commands.md) — `watch`
- [commands §9.2](../commands.md) — `pr poll`
- [CONTEXT — Engine](../CONTEXT.md), [CONTEXT — PR poll](../CONTEXT.md)
- hs-d802 (records of machine `hs`; see [INDEX](INDEX.md)).
- [architecture-verification.md](../research/architecture-verification.md) — items 2 and the disagreements on the two clocks
