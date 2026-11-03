# Architecture vault

This vault documents cpd's architecture as decided in `hs-d802` (a record of machine `hs`), and [architecture-verification.md](../research/architecture-verification.md) is the verification behind every note in it: that note maps each `hs-d802` claim to the section of [contract.md](../contract.md), [commands.md](../commands.md) or [CONTEXT.md](../CONTEXT.md) that owns it and records every disagreement. Vocabulary follows [CONTEXT — How to read this glossary](../CONTEXT.md): the object is a **Workflow**, and PR means only the **pull request** on GitHub — until the rename lands, the CLI still says `cpd pr add`, `--pr` and the `prs` table. Context records are machine-local ([contract §4.6](../contract.md), [CONTEXT — Context store](../CONTEXT.md)): `hs-*` ids belong to machine `hs` and are not readable with `cpd context show --record` elsewhere (that fails `record_not_found` with `details.record_machine`), so [architecture-verification.md](../research/architecture-verification.md) is the in-repo record of each `hs-d802` claim and the contract, commands and CONTEXT section that owns it.

The component map — the five subgraphs of the decided model, with the removed and deprecated parts dashed:

```mermaid
flowchart TD
  subgraph SDK["cpd SDK"]
    CORE["core — all behaviour"]
    LEDGER["ledger — cpd.db: typed rows + the Event log"]
    ENG["engine — the tick loop, cpd watch"]
    GATE["Gate (quick check at the hooks)"]
  end

  subgraph RUNTIME["agent runtime"]
    ADAPTER["pi adapter"]
    DURABLE["pi-durable runner"]
    TMUX["tmux host"]
  end

  subgraph INTERFACES["interfaces"]
    CLI["CLI — cpd"]
    TUI["TUI cockpit"]
    WEB["web UI (in flight)"]
  end

  subgraph EDGE["edge"]
    GH["GitHub — git/gh"]
    OP["1Password — secrets"]
    OR["OpenRouter — models"]
    LGTM["LGTM exporter (fail-open)"]
  end

  subgraph HOSTING["self-hosting"]
    INSTALL["install / host repair"]
    MIGR["store migrations"]
    PROTO["host-socket PROTOCOL v1"]
    DOT["dotfiles config chain"]
  end

  CI["CI checks"]

  CLI --> CORE
  ENG --> CORE
  ADAPTER -->|"subprocess cpd"| CORE
  DURABLE -->|"subprocess cpd"| CORE
  TUI --> CORE
  WEB --> CORE
  CLI -->|"git/gh"| GH
  CLI -->|"secrets"| OP
  CLI -->|"models"| OR
  CLI -->|"metrics, fail-open"| LGTM
  TMUX -.->|"(removal-marked)"| DURABLE
  GATE -.->|"(deprecated)"| CI
```

## Map

- **Foundations**: [[sdk]], [[job-lifecycle]]
- **Agents**: [[roles]], [[agent-runtime]]
- **Interfaces**: [[interfaces]], [[clocks]]
- **Operation**: [[verification]], [[workspace]], [[knowledge-plane]], [[message-board]], [[attention-inbox]]
- **Edges**: [[edge-and-self-hosting]], [[boundaries]]

## How to read

Start at [[sdk]] and [[job-lifecycle]], follow the map into the note that owns the question, and trust the citations: every behavioural claim links the contract, commands or CONTEXT section that owns it, and where the docs and `hs-d802` disagree the verification note is the record.

## Sources

- [contract.md](../contract.md)
- [commands.md](../commands.md)
- [CONTEXT.md](../CONTEXT.md)
- [architecture-verification.md](../research/architecture-verification.md)
