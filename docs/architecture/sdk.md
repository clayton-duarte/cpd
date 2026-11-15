# The SDK

The SDK comprises the **core**, **ledger** and **engine**
([CONTEXT — SDK](../../CONTEXT.md); `hs-d802`). It is a name for the
programmable whole, not a separate product to install. The CLI, cockpit and web
UI are its clients; see [[interfaces]].

## Core

Core is all of cpd's behaviour and logic: the objects (Context, Plan, Workflow
and PR, Job, Attempt, Session) and the rules that move them
([contract §2.2](../contract.md)), the store, the driver, and every area except
the cockpit and the engine loop. It is the `cpd-core` library, linked into every
cpd process — the `cpd` binary, the engine, and the Attempt drivers
([contract §1.1](../contract.md)). Nothing writes or decides outside the core:
a command, the engine and the pi adapter all call into it.

## Ledger

The ledger is cpd's one store: one SQLite database per machine at
`$CPD_HOME/cpd.db` ([contract §4.1](../contract.md); [CONTEXT — Ledger](../../CONTEXT.md)).
It holds two things at once:

- the **typed rows** — the stored state of every object (Plans, Jobs, PRs,
  Attempts, Sessions, the Inbox, Context records and so on)
  ([contract §2.2](../contract.md), [contract §4.1](../contract.md));
- the **append-only Event log**, with the `history` and `txns` tables: every
  state change appends an Event, and Events are the single channel to Leads, the
  cockpit and agent states ([contract §3.7](../contract.md), [contract §4.1](../contract.md),
  [contract §4.3](../contract.md)).

The ledger is written under a **single-writer discipline**:

- **one engine process** per machine writes the scheduled work
  ([contract §3.7](../contract.md); [CONTEXT — Engine](../../CONTEXT.md));
- **short-lived CLI transactions**: every write goes through cpd (a connection
  that is not a cpd process cannot write, because triggers call `cpd_txn()`),
  and one command is one write transaction for its state change, its `history`
  rows and its Events together ([contract §4.3](../contract.md));
- **re-read races resolved in the transaction**: a step reads, acts outside the
  transaction, then writes with a compare-and-swap on the state it read, and a
  `job edit --spec` re-reads the Job's status and Attempts, so the engine's
  Attempt and the edit win or lose as a whole — never both
  ([ADR-0014](../adr/0014-edit-a-queued-job.md); [contract §3.2](../contract.md)).
  A lost compare-and-swap is `cas_lost` ([contract §3.4](../contract.md)).

## Engine

The engine is the tick loop: the one `cpd watch` per machine
([contract §3.7](../contract.md); [CONTEXT — Engine](../../CONTEXT.md)). Every
`tick_seconds` it runs its ordered steps, `escalations` to `permanent-contexts` (`overdue-bump` is retired) — scheduling Jobs under one
concurrency limit, watching liveness and timeouts, polling pull requests,
reconciling the Inbox, curating Context records, and the rest (no Step sends a
Slack message by itself: `inbox` only labels an overdue post `review overdue`, pro-d388)
([contract §3.7](../contract.md)). One per machine is the rule (D5): the engine
is not a daemon per Plan or a scheduler per Workflow
([CONTEXT — Engine](../../CONTEXT.md)).

The SWR sources split across two steps ([swr.md §12](swr.md)): **`pr-poll` owns the
GitHub pass** — its poll and its outbox drain — and **`swr` runs every other SWR
source** (`slack`, `jira`; `model-discovery` runs `models`). Exactly one step polls GitHub per
tick, never two.

## The three parts and the store

```mermaid
flowchart TD
  ENGINE["engine — the cpd watch tick loop<br/>(one per machine)"]
  CORE["core — all behaviour and logic<br/>(cpd-core, linked into every cpd process)"]
  LEDGER["ledger — the one store:<br/>typed rows + append-only Event log"]
  DB[("cpd.db — $CPD_HOME/cpd.db")]

  ENGINE -- "drives" --> CORE
  CORE -- "reads and writes" --> LEDGER
  LEDGER --- DB
```

The engine drives the core; the core reads and writes the ledger; the ledger is
backed by the one `cpd.db`
([contract §3.7](../contract.md), [contract §4.1](../contract.md)).

## See also

- [[job-lifecycle]] — one Job's states and the Attempt that executes it.
- [[interfaces]] — the CLI, cockpit and web UI that are the SDK's clients.
