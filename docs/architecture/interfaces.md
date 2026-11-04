# Interfaces

cpd has three interfaces: the **CLI**, the **TUI cockpit** and the **web UI**.
The CLI is first-class; the cockpit is a client of the core; the web UI is
decided but not yet built. The verification behind this note, and where the
decided model disagrees with the docs, is in
[the architecture verification note](../research/architecture-verification.md)
(item 5).

## The CLI is first-class

The CLI is the first-class interface, and it is first-class because it is the
agents' interface. `hs-d753` (a record of machine `hs`) states it
exactly: *"cpd is CLI-first: the CLI is the first-class interface, meant for
agents; the TUI (the cockpit, cpd chat) is just one client of the core, as a web
UI would be another. Moving Leads and Managers to pi-durable strengthens the core
as a headless engine that any interface attaches to; no capability may exist only
in a TUI."*

The contract makes the same rule structural — **"CLI first.** Every capability is
a command. Agents, the pi adapter, scripts and the TUI all use the same commands;
the TUI (`cpd tui`) is the human view onto them, never a second way to change
state." ([contract §1.1](../contract.md)) — and gives every command one output
shape: exactly one JSON value on stdout, so a command is equally usable by an
agent, a script and a view ([contract §1.2](../contract.md)).

Today's identifiers are still `cpd pr add`, `--pr` and the `prs` table; the
target word for the object is **Workflow**, and the rename has not landed
([CONTEXT — How to read this glossary](../../CONTEXT.md); [contract §2.5](../contract.md)).

## The TUI cockpit (humans)

The cockpit is the User's interface: one tmux window whose three columns are the
three stages, Context, Plan and Dispatch, with ratatui views
([CONTEXT — Cockpit](../../CONTEXT.md)). It is built and attached by `cpd tui`
([commands §13](../commands.md), `cpd tui [--attach] [--respawn]`); bare `cpd`
opens it ([commands §13](../commands.md) §13.3). It owns its terminal, so it
prints no JSON value on success ([contract §1.2](../contract.md)).

**It is tmux-hosted.** cpd owns one tmux server per machine
(`tmux -L <socket>`, default `cpd`) and the cockpit's panes live in it
([contract §6](../contract.md), "The tmux host"; [contract §6.1](../contract.md),
"One tmux server per machine"). tmux is the cockpit's medium and the pi Sessions'
medium ([architecture §1.6](../architecture.md)).

**It is a removal-marked dependency.** `hs-d802` marks tmux for removal once the pi → pi-durable migration completes.
The removal is a future Plan step, not a change to today's docs: the contract
still says "tmux is the only host", and the verification note records that the
removal mark rests on the future P3 decision `hs-d752`, not on a mark in the
contract ([verification note, items 3 and 11](../research/architecture-verification.md);
[contract §1.1](../contract.md)). The cockpit stays usable today because tmux
still hosts it.

## The web UI (in flight)

The web UI is a second human interface, decided but **not built**. The decided
stack is `cpd web` (a new `cpd-web` crate in the one binary), a synchronous
tiny_http-class server, a Vite + React + shadcn/ui front end, live views over a
WebSocket, and a unix socket behind `tailscale serve` (Context records
`hs-d768`–`hs-d779`). It is being built by the
Plan `web-client`, and it lives nowhere yet ([architecture §1.7](../architecture.md);
[architecture §6](../architecture.md); [verification note, item 5](../research/architecture-verification.md)).

The web is framed by `hs-d753` too: like the cockpit, it is **one client of the
core**, never a second interface of the engine, and the CLI-first rule applies to
it ([architecture §6](../architecture.md); `hs-d753`).

## Which interface is first-class, and why

The CLI is first-class ([contract §1.1](../contract.md); `hs-d753`). The reason is the direction of dependency:
the core is the programmable whole, the engine is headless, and both human
interfaces (cockpit and web) attach to the same commands. No capability may exist
only in a TUI, so a capability that the cockpit needs must be a command first
(`hs-d753`). The cockpit is therefore a
client of `cli`, not the other way round ([architecture §1.5](../architecture.md));
the web follows the same rule ([architecture §6](../architecture.md)).

## Related notes

The roles that use these interfaces are [[roles]]; the runtime the agents run
in is [[agent-runtime]]; the programmable whole the interfaces attach to is the
[[sdk]]; the engine loop behind them is [[clocks]].
