# cpd φ

cpd's mark is **φ**, as pi's is **π**; the mark and its logo font are specified in [docs/mark.md](docs/mark.md).

cpd (Continuous Parallel Delivery) is a framework for how agents work: it enforces its protocols through tooling rather than prose, gates undesired behaviour, and logs every gate as a metric to improve on.

A Lead specifies and coordinates bounded Jobs; Workers execute them toward reviewed pull requests, in parallel when the work is independent.

The levels (Jira's words, as an analogy), the cpd objects and their roles are in [CONTEXT.md](CONTEXT.md).

## Docs

- [CPD essence after the live trial](docs/essence.md): living product direction; the verbatim prior upstream essay is preserved in [archive](docs/archive/essence-pre-live-trial.md); read alongside the accepted [delivery principles](docs/delivery-principles.md)
- [Manifesto draft](docs/manifesto.md): purpose and principles
- [docs/mark.md](docs/mark.md): the mark φ and its logo
- [CONTEXT.md](CONTEXT.md): the vocabulary
- [docs/contract.md](docs/contract.md): the normative contract
- [docs/commands.md](docs/commands.md): commands, installation and repair
- [docs/implementing.md](docs/implementing.md): where things go in the code
- [Architecture references](docs/architecture/INDEX.md)
- [docs/adr/](docs/adr/): decisions
- [docs/archive/cpd1/](docs/archive/cpd1/): cpd 1's docs, history only

## Gate

The continuous repository QA Protocol, its operating limits and creation
tools are documented in [QA Protocol](docs/qa-protocol.md), with the
[decisions and research](docs/qa-decisions.md) behind it. To adopt the same method
in another repo or agent session, use [`clayton-duarte/skills`](https://github.com/clayton-duarte/skills),
package `skills/repo-qa`. CPD keeps a [pinned consumer snapshot](docs/qa-source.md).

```sh
pnpm run verify
```

pnpm must be on `PATH`, at the version `package.json` pins (`packageManager`); cpd does not install it.
