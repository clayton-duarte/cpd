## CPD’s essence

**CPD makes delegated agent work dependable—from the User’s intent to a verified, reviewed result—without making the User supervise every execution step.**

Parallelism serves that purpose. It is not the purpose itself.

That is my distillation of the documentation, not a proposed replacement definition or a new rule.

### 1. Preserve what was decided

Decisions and lasting context must survive a conversation or an agent being replaced.

The essential capability is **recoverable understanding**: another agent can determine what was agreed, why, and what remains to be delivered.

Context records and briefs provide that capability. Embeddings, automated curation, and elaborate retrieval are mechanisms—not the purpose.

### 2. Separate deciding, coordinating, and executing

The documented responsibilities are:

| Responsibility | Owner |
|---|---|
| Settle product intent with the User | Manager |
| Turn settled intent into bounded, coordinated work | Lead |
| Execute specified work and produce evidence | Worker |

The reason for this separation is practical: execution can continue while the agent discussing the product remains available to the User.

### 3. Delegate bounded work

A Worker needs enough information to act without reconstructing its parent’s conversation:

- The intended outcome.
- Relevant context.
- The permitted scope.
- Acceptance criteria.
- What is explicitly excluded.

**A delegation is a contract for an outcome, not merely a prompt to stay busy.**

### 4. Execute according to dependencies

Independent work may run in parallel. Dependent work waits.

The essential capability is **correct coordination**, not maximum concurrency or maximum numbers of agents.

### 5. Establish completion through evidence

Agent activity is not delivery.

A stopped agent, a passing test, a commit, a merged pull request, and a production deployment establish different facts. CPD must not substitute one for another.

The documented default for behavior changes is:

**Expected behavior → independently authored failing tests → implementation → verification → reviewed delivery.**

The docs also recognize distinct paths for documentation, configuration, coverage, refactoring, and exploration. The default sequence is not a universal ritual.

### 6. Keep authority with the User

Agents work from settled decisions and surface uncertainty rather than inventing intent.

Tooling enforces agreed boundaries. Failures and refusals provide evidence for improvement; their existence does not, by itself, justify another rule.

---

## The minimum conceptual model

The terminology reduces to this:

```text
Context: durable understanding shared across work
  Plan: one deliverable
    Workflow: one repository’s contribution, toward one pull request
      Job: one bounded assignment
        Attempt: one execution of that assignment
```

Exploration is an explicit exception: it can produce findings without a Workflow or pull request.

The corresponding operating loop is:

> **Understand → decide → specify → execute → verify → deliver.**
>
> Preserve decisions throughout; involve the User when their judgment or authority is needed.

Everything else should be understood in relation to that loop—not mistaken for CPD’s reason to exist.

## What is mechanism rather than essence

Rust, SQLite, pi, tmux, WebSockets, Electron, model selection, Jira, Slack, caches, and particular command names are implementation or integration choices.

They may be necessary to the current implementation. **That does not make them essential to the product’s purpose, nor establish that they should be deleted.**

## The documentation already shows the problem

Three concrete examples from the material read:

- **Obsolete claims remain presented as current facts.** `CONTEXT.md` says refusal Events, metrics, and alerts do not exist; `docs/contract.md` describes them as required machinery.
- **Old rules and amendments coexist inside the live contract.** Its draft-Job section says drafts are “never a waiting state,” then introduces an amendment allowing named waits and acknowledges that part of the resulting counting behavior is unspecified.
- **Architecture generations overlap.** Contract §1.1 still describes a `tui/` crate and cockpit architecture, while the approved `pi-native-core` design specifies their removal and a WebUI-first architecture.

These are documentation inconsistencies, **not verified diagnoses of runtime bugs**. They show that a reader must reconstruct the effective rules from accumulated changes.

## Grounding boundary

This synthesis is grounded in fresh `origin/main` at `f0bab4a`: the README, manifesto, glossary, core contract sections, relevant ADRs, approved architecture design, and Manager instructions. It is **not an exhaustive audit of every Markdown file or of the code**. The manifesto is explicitly a draft; the README holds the canonical definition, and the contract specifies intended behavior.

**The essence is small: preserve intent, delegate bounded work, coordinate dependencies, require evidence, and retain human authority. The accumulated machinery is not the essence.**
