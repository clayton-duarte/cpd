---
status: superseded by hs-d23 and hs-a1 (records in the database, Contexts machine-local; Plan manager B1)
---

# A Context's shared knowledge lives in the Context store

Decisions must never be lost (User: "that should be documented so we never loose those decisions"). Decisions about cpd's code go into the cpd repo as ADRs (`docs/adr/`); likewise decisions about a product's code go into that product's repo as ADRs. A Context's shared knowledge goes into the **Context store**: typed records (decisions, notes, questions with their answers, and the Context's Plans), written with `cpd context …` commands and rendered as markdown that Leads read when grounding.

Storage is append-only JSONL now. The Rust port (ADR-0010) chooses the database and imports the files; likely SQLite, which would revisit ADR-0004 (the Ledger is beads). Location is `~/.cpd/contexts/<slug>/`, a git repository with a private remote, for history, backup and sharing across the User's three machines. It is not keyed by repo, because a Context may span repos.

## Consequences

- Leads write the store only through `cpd context …`, as the Ledger is written only through `cpd` (ADR-0004).
- Markdown is a rendering of the records, not the source.
- The database choice and any change to ADR-0004 are deferred to the Rust port.
- The exact record schema and command set are not decided in the sources.
