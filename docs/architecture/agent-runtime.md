# The agent runtime

The **pi adapter is the agent runtime** (`hs-d802` (a record of machine `hs`), item 3). It is cpd's own code — the TypeScript package `pi/`, loaded in every pi session cpd starts — that binds pi sessions to the SDK: first messages, dialog serialization, titles and state reporting ([commands §15](../commands.md), [commands §15.1](../commands.md)). The adapter is **the only pi-specific code**: it turns Events into messages, reports agent states and runs the Plan review in the Lead's chat, and it calls only `cpd` commands — it never reads the database and never types into a pane ([commands §15](../commands.md)).

This note documents the model in hs-d802 against today's docs. Where the model and the docs disagree, the discrepancy is named in place and sourced to [architecture-verification.md](../research/architecture-verification.md), which verified `hs-d802` claim by claim.

## cpd is pi-dependent by design

pi is a **system dependency** of cpd, named beside `git`, `tmux >= 3.6` and `gh` ([contract §1.1](../contract.md)). The adapter is the only pi-specific code ([commands §15](../commands.md)); cpd starts pi for every Lead, Manager and Worker, for the title writer and for the distiller ([contract §6.6](../contract.md)).

This is the design stance, not an accident (hs-d802, item 3): cpd does not abstract over agent runtimes. The contract writes the exact `pi` argv and environment per role ([contract §6.6](../contract.md)), and the runtime the adapter extends **is** pi — [architecture §1.4](../architecture.md) calls the adapter and the durable host "the agent side of the wire," code loaded into pi processes.

> **Model vs docs.** hs-d802 says cpd is "not agent-agnostic." The runtime is pi by design, but the *control plane* is deliberately agent-neutral: [ADR-0005](../adr/0005-cpd-cli-control-plane.md) keeps the path open to Claude Code and Hermes without rewriting the core, because every capability is a CLI command the adapter drives ([commands §15](../commands.md)). The accurate statement is: the **runtime** is pi-dependent by design; the **control plane** (the CLI) is not ([architecture-verification.md](../research/architecture-verification.md), item 3).

## What the adapter does (the runtime's four jobs)

- **First messages.** cpd builds each role's first message and passes it to pi as an `@file` after `--`, never as inline text, because Linux caps one argument at 131072 bytes ([contract §6.6](../contract.md)). A User-created Lead's first message is replayed by the adapter through `cpd session first-message` ([commands §15.7a](../commands.md)); the adapter stores it as a silent entry before the first turn ([contract §6.13](../contract.md), "The first message of a by-user Session").
- **Dialog serialization.** The adapter opens and closes pi's blocking dialogs and reports them: `ui_prompt_start` and `ui_prompt_end` open one `dialog` reason (or `verdict: <option>`), depth-counted so nested prompts give one pair ([commands §15.4](../commands.md)). The Plan review's own `select` and `editor` dialogs open no `dialog` reason; they are represented by `plan-review:<review>` ([commands §15.4](../commands.md), [commands §15.5](../commands.md)). A Worker's dialogs are not reported: it never talks to the User, it asks its Lead with `cpd ask` ([commands §15.4](../commands.md)).
- **Titles.** Engine step `session-titles` starts `cpd session retitle` for a finished Lead or Manager turn, and the title writer is itself a one-shot pi run whose prompt asks for one `TITLE:` line ([contract §3.7](../contract.md) `session-titles`, [contract §6.6](../contract.md), [commands §10.14](../commands.md)).
- **State reporting.** The adapter keeps `active`, its own reasons (`escalation:<id>`, `plan-review:<review>`, `loop-guard`) and blocking dialogs, and reports `working`, `waiting`, `blocked` or `idle` through `cpd session state` ([commands §15.4](../commands.md), [contract §3.6](../contract.md)). It also records whether a run is a **reply run** and reports it with the `idle` ([commands §15.4](../commands.md)).

## The worker runner: pi and pi-durable

The worker/lead runner is chosen by the `worker_runner` config key — exactly `"pi"` or `"durable"`, default `"pi"` — read once per Attempt and stored on it ([contract §8.2](../contract.md), [contract §8.1](../contract.md)); [CONTEXT — Worker runner](../CONTEXT.md) defines it as "what runs a Worker: `pi` (a pi session in a tmux window) or `durable` (a headless child process of the Attempt's driver that resumes in place)."

pi-durable is the headless runner built on `@earendil-works/pi-durable`: the **durable Worker runner** (`pi/durable-worker/main.ts`) and the **durable session host** (`pi/durable-host/main.ts`) ([contract §6.12](../contract.md), [contract §6.13](../contract.md), [contract §6.14](../contract.md)).

> **Model vs docs.** hs-d802 calls pi-durable "the worker/lead runner." The docs call it "a second runner **beside pi, not a replacement: both runners are kept**," with `worker_runner = "pi"` as the default and the durable host "a second host beside pi, behind a per-role config key that defaults to `pi`" ([contract §6.12](../contract.md), [contract §6.13](../contract.md), [contract §8.2](../contract.md)). pi-durable is the *alternative* runner today, and the migration's target; it is not yet *the* runner ([architecture-verification.md](../research/architecture-verification.md), item 3).

## tmux: still the host, marked for removal

**tmux is the only host today.** The engine runs in a hidden session, the cockpit's panes are tmux panes, and Leads run in the cockpit ([contract §1.1](../contract.md), [contract §6.1](../contract.md)); [CONTEXT — Cockpit](../CONTEXT.md) says "tmux hosts the panes, ratatui draws the views." pi sessions themselves run in a tmux window ([contract §6.6](../contract.md)).

hs-d802 (item 2) marks tmux **for removal once the pi → pi-durable migration completes** (hs-d802; see also the model's item 2). The durable runner and host already run with **no tmux pane** ([contract §6.12](../contract.md), [contract §6.13](../contract.md)).

> **Model vs docs.** The removal is a *future* Plan step, not a state in today's contract: [contract §1.1](../contract.md) still says "tmux is the only host," and dropping it ("P3") is out of scope, its own decision after durable Leads have run a while (`hs-d752`; [architecture-verification.md](../research/architecture-verification.md), item 3; [durable-leads.md](../research/durable-leads.md)). Treat tmux as a removal-marked dependency, not a removed one.

## Related notes

The runtime sits on the SDK ([[sdk]]); it starts the three agent roles and the auxiliaries ([[roles]]); and its products surface through the interfaces ([[interfaces]]). The clocks that start its processes and deliver its Events are [[clocks]].

## Sources

- [contract §6.6](../contract.md) — how cpd starts pi (argv, environment, first messages)
- [contract §6.12](../contract.md) — the durable Worker runner
- [contract §6.13](../contract.md) — the durable session host
- [contract §6.14](../contract.md) — the host socket protocol
- [contract §1.1](../contract.md) — system dependencies; tmux the only host
- [contract §3.6](../contract.md), [contract §3.7](../contract.md) — Session states; `session-titles` titles
- [commands §15](../commands.md) — the pi adapter's protocol ([§15.1](../commands.md), [§15.4](../commands.md), [§15.5](../commands.md))
- [CONTEXT — Worker runner](../CONTEXT.md), [CONTEXT — Session](../CONTEXT.md), [CONTEXT — Cockpit](../CONTEXT.md)
- [ADR-0005](../adr/0005-cpd-cli-control-plane.md) — the CLI control plane keeps the path open to other agents
- hs-d802, hs-d752 (records of machine `hs`; see [INDEX](INDEX.md)).
- [architecture-verification.md](../research/architecture-verification.md) — item 3 and its disagreements
