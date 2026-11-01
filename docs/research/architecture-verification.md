# Architecture verification: hs-d802 against cpd's current docs and code

Method. I read the decided model verbatim from the Context record (`cpd context
show --record hs-d802`) and mapped each of its claims to the document or code
section that owns it. The evidence is cpd's live docs: `CONTEXT.md`,
[contract.md](../contract.md), [commands.md](../commands.md),
[architecture.md](../architecture.md), [implementing.md](../implementing.md),
[versioning.md](../versioning.md), the ADRs under `docs/adr/`, and the research
notes [gate-transition.md](gate-transition.md), [durable-leads.md](durable-leads.md)
and [web-ui.md](web-ui.md); the supporting Context records `hs-d753`, `hs-d752`,
`hs-d795`, `hs-d798` (`cpd context show --record <id>`). I ran no code and opened
no file under `engine/src/`, `core/src/`, `cli/src/` or `pi/` for this attempt:
every verdict below rests on the documents and Context records named in its
`Sources:` line. Where the model's wording matches the docs it is `confirmed`;
where the docs say something else it is `partly confirmed` or `contradicted`, and
each mismatch has a `Disagreement:` line quoting both sides and linking both. A
claim the docs do not carry is `unverified`, with the reason.

The notes this file feeds are the architecture vault's [[sdk]], [[clocks]],
[[agent-runtime]], [[roles]], [[interfaces]], [[verification]], [[workspace]],
[[knowledge-plane]], [[message-board]], [[attention-inbox]],
[[edge-and-self-hosting]] and [[boundaries]]; each section names the note that
owns its claim.

## 1. "cpd SDK" = core + ledger + engine

**Claim.** `cpd SDK` names the whole core: core (all behaviour and logic), the
ledger (the one SQLite store — typed rows plus the append-only Event log) and the
engine (the tick loop); the term enters CONTEXT.md's glossary. (`hs-d802`, `cpd context show --record hs-d802`
item 1.)

**Sources:** [contract §1.1](../contract.md) (what cpd 2 is: the crates, the one
store, the one engine), [contract §3.7](../contract.md) (the engine), [contract
§4.1](../contract.md) (the database), [contract §5](../contract.md) (Events),
[CONTEXT — Engine](../CONTEXT.md), [CONTEXT — Ledger](../CONTEXT.md),
[architecture §1](../architecture.md) (the parts). Model: `hs-d802`
(`cpd context show --record hs-d802`).

**Verdict:** partly confirmed. The three things exist and are separable; the name
`SDK` and the crate mapping do not match the docs.

**Disagreement:** Model (`hs-d802`, `cpd context show --record hs-d802`): "`cpd SDK` names the entirety of the core: core (all
behavior), ledger (the one sqlite store - typed rows + the append-only Event log)
and engine (the tick loop)." Docs: [contract §1.1](../contract.md) names three
Rust crates, `core/` (`cpd-core`, the library), `cli/` (`cpd`, the binary) and
`tui/` (`cpd-tui`), and [architecture §3](../architecture.md) adds `cpd-engine` —
the *ledger* is not a crate but the `cpd.db` file ([contract §4.1](../contract.md)),
and the engine is the `cpd-engine` crate/the `cpd watch` loop ([contract
§3.7](../contract.md)), not one of the three named crates. No glossary entry and
no occurrence of the string `SDK` exists anywhere in `contract.md`, `commands.md`
or `CONTEXT.md` (grep, this worktree); the term is the model's, to be added by a
later Job.

## 2. Two clocks: the engine loop (`tick_seconds`) and the gh poll (`pr_poll_seconds`)

**Claim.** Two clocks, not one: the engine loop clock (`tick_seconds`, purely
internal, runs engine steps E1–E21 except E5 and moves work from the Context
phase to Dispatch) and the gh polling clock (E5, `pr_poll_seconds`, watches
external GitHub state post-dispatch and drives cpd's response); the two are
deliberately different timings. (`hs-d802`, `cpd context show --record hs-d802` item 5.)

**Sources:** [contract §3.7](../contract.md) (the E-steps, E5's due time),
[contract §8.2](../contract.md) (`tick_seconds`, `pr_poll_seconds`),
[contract §7.6](../contract.md) (the poll), [commands §12](../commands.md)
(`watch`), [commands §9.2](../commands.md) (`pr poll`). Model: `hs-d802`.

**Verdict:** partly confirmed. Two decoupled clocks exist; the step range, the
"except E5" wording and the "different timings" claim do not match the docs.

**Disagreement:** Model (`hs-d802`, `cpd context show --record hs-d802`): "the engine loop (`tick_seconds` ... E1-E21 except E5)".
Docs: [contract §3.7](../contract.md) says the tick runs "the tick's **24 steps**
... (E1–E21; **E6 is removed**; E21 ... runs right after E20)", and E5 is *an
E-step too* — "The engine sleeps until the earlier of its next tick and E5's due
time, so E5 is not held back to a tick boundary." The model omits that E6 is
removed and misstates E5 as outside the step set.

**Disagreement:** Contradictory records on the step range. Model (`hs-d802`, `cpd context show --record hs-d802`): "E1-E21 except
E5" (from `hs-d802`). [CONTEXT — engine step](../CONTEXT.md) says "One numbered
part of the engine's loop, **E1 to E17**". The contract's E1–E21 is the newer
record and wins (recency); CONTEXT.md is stale and a later Job must fix it.

**Disagreement:** Model (`hs-d802`, `cpd context show --record hs-d802`): "different timings on purpose." Docs: [contract
§8.2](../contract.md) gives `tick_seconds = 5` and `pr_poll_seconds = 5`, the
same default, and [CONTEXT — PR poll](../CONTEXT.md) says "every 5 s". What
differs is the *clock* (E5 runs `pr_poll_seconds` after the end of the previous
poll, "never overlapping", not at the next tick — [contract §3.7](../contract.md)),
not the default interval; the "different timings" phrase is unsupported.

**Disagreement:** Model: the engine loop "moves work from the Context phase to
Dispatch". Docs: the engine "schedules every Plan's Jobs under one concurrency
limit, watches liveness and timeouts, polls pull requests, starts a Workflow once
the Workflows it needs are merged, and curates Context records" ([CONTEXT —
Engine](../CONTEXT.md)); the Context→Plan→Dispatch progression is the Manager's
and the Lead's work ([CONTEXT — Phase, stage and step](../CONTEXT.md)), not the
tick.

## 3. The agent runtime: the pi adapter, pi-dependent by design, pi-durable, tmux removal-marked

**Claim.** The pi adapter IS the agent runtime, and cpd is pi-dependent by design
— not agent-agnostic. pi-durable is the worker/lead runner. tmux is marked for
removal once the pi → pi-durable migration completes. (`hs-d802`, `cpd context show --record hs-d802` items
2 and 3.)

**Sources:** [contract §6.6](../contract.md) (how cpd starts pi), [contract
§6.12](../contract.md) (the durable Worker runner), [contract
§6.13](../contract.md) (the durable session host), [contract §6.14](../contract.md)
(the host socket protocol), [contract §1.1](../contract.md) (system deps),
[architecture §1.4](../architecture.md) (the pi adapter and the durable host),
[commands §15](../commands.md) (the pi adapter's protocol), [ADR-0005](../adr/0005-cpd-cli-control-plane.md)
(the control plane is the CLI). Model: `hs-d802`; related: `hs-d752`.

**Verdict:** partly confirmed. The adapter is the agent-side runtime and pi is a
hard dependency; pi-durable is an *alternative* runner, not *the* runner; the
"not agent-agnostic" stance and the tmux removal-mark each conflict with a live
doc.

**Disagreement (runtime):** Model (`hs-d802`, `cpd context show --record hs-d802`): "The pi adapter IS the agent runtime."
Docs: [architecture §1.4](../architecture.md) calls the pi adapter and the
durable host "the **agent side of the wire**" — code loaded into pi processes,
"[not] an agent itself"; the *runtime* the model names is pi, which the adapter
extends.

**Disagreement (pi-durable is the runner):** Model (`hs-d802`, `cpd context show --record hs-d802`): "pi-durable is the
worker/lead runner." Docs: [contract §6.12](../contract.md) says the durable
runner "is a second runner **beside pi, not a replacement: both runners are
kept** (pro-d165)", and [contract §8.2](../contract.md) sets
`worker_runner = "pi"` (the default); [contract §6.13](../contract.md) says the
durable host is "a second host beside pi, behind a per-role config key that
defaults to `pi`". pi-durable is the *future* runner, not today's.

**Disagreement (not agent-agnostic):** Model (`hs-d802`, `cpd context show --record hs-d802`): "cpd is pi-dependent by design -
not agent-agnostic." Docs: [ADR-0005](../adr/0005-cpd-cli-control-plane.md) says
the CLI control plane "keeps the path open to **Claude Code and Hermes** without
rewriting the core, at the cost of a small process boundary", i.e. the control
plane is deliberately agent-agnostic even though the runtime is pi. The model's
"not agent-agnostic" contradicts the ADR's stated design intent.

**Disagreement (tmux removal-mark):** Model (`hs-d802`, `cpd context show --record hs-d802`): "tmux is marked for removal once the
pi -> pi-durable migration completes." Docs: [contract
§1.1](../contract.md) still says "**tmux is the only host.** Leads run in the
cockpit; Workers and the engine run in a hidden session of the same tmux server."
The removal is a *future* Plan step, "dropping tmux (P3) is out of scope, its own
decision after durable Leads have run a while" (`hs-d752`,
`cpd context show --record hs-d752`; [durable-leads.md](durable-leads.md) "What
stays one-way: P3, deleting the tmux code, is out of scope and is its own
decision"). The removal-mark is not in the contract today, so the claim is
partly confirmed against a future decision, not today's docs.

## 4. Exactly four roles (manager, lead, worker, auxiliary), the first three attached to the phases

**Claim.** The agent population has exactly four roles — manager, lead, worker,
auxiliary — and the first three are intrinsically attached to cpd's phases:
Context → Manager, Plan → Lead, Dispatch → Worker; the auxiliary one-shots
(distiller, retitle) sit outside that attachment. (`hs-d802`, `cpd context show --record hs-d802` item 4.)

**Sources:** [CONTEXT — Roles](../CONTEXT.md) (the roles section and the levels
table), [CONTEXT — Distilled Context](../CONTEXT.md) (auxiliary agents),
[contract §9](../contract.md) ("every auxiliary agent ... is `GX` only"),
[contract §3.1](../contract.md) (phases), [contract §3.6](../contract.md)
(session states), [contract §3.7](../contract.md) (E10 retitle, E9 distill),
[contract §3.9](../contract.md) (stages→phases). Model: `hs-d802`.

**Verdict:** partly confirmed. Manager/Lead/Worker are real and real roles; the
"fourth role is auxiliary" and the phase wording disagree with the docs, and the
docs' fourth role entry is the User.

**Disagreement (the fourth role):** Model: the four roles are "manager, lead,
worker, **auxiliary**". Docs: [CONTEXT — Roles](../CONTEXT.md) lists four entries,
"**User**, **Manager**, **Lead**, **Worker**"; `auxiliary` is not a role there.
`auxiliary` appears only as a *category of one-shot pi runs* — [CONTEXT —
Distilled Context](../CONTEXT.md) ("the pilot of Epic B, auxiliary agents") and
[contract §9](../contract.md) ("every auxiliary agent (a one-shot pi run that is
no Session and writes no code) is `GX` only"), which names the distiller and the
title writer, not a role row.

**Disagreement (User as a role):** The same [CONTEXT — Roles](../CONTEXT.md)
section lists **User** as a role ("The human who owns the ticket..."), so
CONTEXT.md presents *four* roles whose fourth is the human, while the model's
fourth is `auxiliary`; the docs and the model disagree on both the count and the
identity. The architecture-vault roles note must state that the User is the human
and not one of the agent roles (as the Plan's `arch-roles` Job requires).

**Disagreement (phase attachment):** Model (`hs-d802`, `cpd context show --record hs-d802`): "the first three are intrinsically
attached to cpd's phases: Context -> Manager, Plan -> Lead, Dispatch -> Worker."
Docs: the levels table of [CONTEXT — Work units](../CONTEXT.md) attaches
`Context → Manager`, `Plan → Lead`, **`Job → Worker`** (a Workflow is also
`Lead`), and [CONTEXT — Phase, stage and step](../CONTEXT.md) defines `Context`,
`Plan` and `Dispatch` as the three **stages**, not phases — the phases are
`grounding → plan → dispatch → deploy → done` ([contract §3.1](../contract.md)).
The model calls the stages "phases" and attaches the Worker to `Dispatch` where
the docs attach it to `Job`.

## 5. Interfaces: CLI primary (hs-d753), TUI cockpit, web UI in flight

**Claim.** The interfaces are the CLI (primary — the agents' interface, hs-d753),
the TUI cockpit (humans) and the web UI (humans, in flight). (`hs-d802`, `cpd context show --record hs-d802`
item 8's docs folder; the interfaces item of the Plan.)

**Sources:** [contract §1.1](../contract.md) ("CLI first"), [contract
§1.2](../contract.md) (one JSON value), [commands §13](../commands.md) (`tui`),
[CONTEXT — Cockpit](../CONTEXT.md), [architecture §6](../architecture.md) (the
web node, decided), [web-ui.md](web-ui.md). Records: `hs-d753`
(`cpd context show --record hs-d753`), `hs-d802`.

**Verdict:** confirmed. The CLI is first-class, the cockpit is its client, and the
web is decided but unbuilt.

**No disagreement.** `hs-d753` is exact: "cpd is CLI-first: the CLI is the
first-class interface, meant for agents; the TUI (the cockpit, cpd chat) is just
one client of the core, as a web UI would be another." The web is "**not built**"
and its stack is "decided" ([architecture §1.7 and §6](../architecture.md);
`hs-d768`–`hs-d779`). One documentation note, not a disagreement: the model's
target vocabulary ("Workflow") is not yet the CLI's — today's identifiers are
still `cpd pr add`, `--pr` and the `prs` table, per the transitional note in
[CONTEXT — How to read this glossary](../CONTEXT.md).

## 6. Verification and the Gate: PR gate → CI checks, merge gate → pre-commit/pre-merge hooks; deprecation path

**Claim.** The Gates are marked for deprecation: the PR gate is replaced by CI
checks, and the merge gate by pre-commit/pre-merge hooks. (`hs-d802`, `cpd context show --record hs-d802`
item 6.)

**Sources:** [contract §7.3](../contract.md) (the hooks: `pre-commit` = the quick
check, `pre-merge` = the Gate), [contract §7.4](../contract.md) (the merge and
the Gate), [contract §7.6](../contract.md) (the Gate on a snapshot at `pr open`,
`pr push`, `pr rebase`), [contract §7.7](../contract.md) (the checks at submit),
[contract §8.3](../contract.md) (`gate` domain, default `none`; deprecated
`[pr] gate`), [commands §9.2](../commands.md), [commands
§9.8](../commands.md), [CONTEXT — Gate / Quick check / CI](../CONTEXT.md),
[gate-transition.md](gate-transition.md) (hs-d282: moving the check to GitHub
Actions). Model: `hs-d802`.

**Verdict:** partly confirmed. The direction (CI as the check, hooks as the local
mechanism) matches; the model's naming of which hook is which gate is wrong, and
"marked for deprecation" is not a phrase the docs use.

**Disagreement (which hook):** Model (`hs-d802`, `cpd context show --record hs-d802`): "the merge gate by pre-commit/**pre-merge**
hooks." Docs: [contract §7.3](../contract.md) assigns one hook per gate —
`pre-commit` "**the quick check** for `gate.merge`" (the merge) and `pre-merge`
"**the Gate** for `gate.pr`" (the pre-PR, run "for `open` or `update` Jobs only").
So the *merge* gate is the **pre-commit** hook, and the **pre-merge** hook is the
pre-PR Gate; the model collapses them and reverses the pairing.

**Disagreement (deprecation is already the default):** Model (`hs-d802`, `cpd context show --record hs-d802`): "the Gates are
marked for deprecation." Docs: the deprecation is already the shipped default —
[contract §8.3](../contract.md) sets `gate.merge = "none"` and `gate.pr = "none"`
("cpd's default is `none`: core/src/policy/default.toml"), and [CONTEXT —
Gate](../CONTEXT.md): "Where `gate.pr` is `none`, CI is the check where it
exists." There is a *deprecated alias* (`[pr] gate`, [contract
§8.3](../contract.md)), but no "marked for deprecation" state for the gate
mechanism itself.

**Disagreement (which replaces which):** Model (`hs-d802`, `cpd context show --record hs-d802`): "the PR gate is replaced by CI
checks." Docs: [gate-transition.md](gate-transition.md) records that the check
moved to a GitHub status/`cpd/gate` on CI (hs-d282) and [contract
§7.6](../contract.md) says "When `gate.pr` is `none` ... the repo's CI is the
check", so the direction holds; but "CI checks" is `pr.feedback.checks` reads on
failing checks ([CONTEXT — CI](../CONTEXT.md)), which is a different mechanism
from the Gate — the docs keep `Gate`, `Quick check`, `Mechanical checks` and `CI`
as four distinct terms, so the one-line replacement the model states is an
oversimplification.

## 7. Workspace: per-attempt worktrees, the Lead Branch/Lead worktree per Workflow, prune/janitor

**Claim.** The workspace is per-attempt worktrees, with a Lead Branch and Lead
worktree per Workflow, plus prune/janitor clean-up. (`hs-d802`, `cpd context show --record hs-d802` item 7.)

**Sources:** [contract §7.1](../contract.md) (worktrees and branches), [contract
§7.2](../contract.md) (creating worktrees), [contract §7.4](../contract.md) (the
merge), [commands §29](../commands.md) (`janitor`), [commands
§23](../commands.md) (`prune`; note this "§23" is one of several sections numbered
23 in commands.md), `scripts/prune-worktrees.sh` (now only `exec cpd prune`,
[implementing.md](../implementing.md) "Covered" list), [CONTEXT — Lead
worktree](../CONTEXT.md), [CONTEXT — Attempt](../CONTEXT.md). Model: `hs-d802`.

**Verdict:** partly confirmed. The Lead Branch/Lead worktree and the
prune/janitor pair match; "per-attempt worktrees" is only true for parallel
Attempts; the object's name is still "PR" in the docs.

**Disagreement (per-attempt worktrees):** Model (`hs-d802`, `cpd context show --record hs-d802`): "per-attempt worktrees." Docs:
[contract §7.2](../contract.md) says "**Linear Attempt** ... reuses the Lead
worktree ... runs no `pre-start`", and only a **parallel** Attempt "creates its
own worktree"; the Attempt-worktree table in [contract §7.1](../contract.md) lists
"Parallel Attempt worktree" alone. So not every Attempt has its own worktree.

**Vocabulary note (the object's name):** Model (`hs-d802`, `cpd context show --record hs-d802`): "per Workflow". Docs: the
transitional note in [CONTEXT — How to read this glossary](../CONTEXT.md) says
"until the rename lands, the CLI (`cpd pr add`, `--pr`), the tables (`prs`,
`pr_needs`), the Events and the docs still call a Workflow 'PR'", and [contract
§7](../contract.md) writes "Everything in this section is per **PR** (R34)". The
model's target word is correct for the vault, but today's docs and identifiers
say `pr`/`prs`; the note must quote both.

## 8. The knowledge plane: Context records, holographic search index, curation/distillation, separate from the Event ledger

**Claim.** The knowledge plane is Context records, the holographic search index
and curation/distillation, deliberately separate from the Event ledger (state vs
knowledge). (`hs-d802`, `cpd context show --record hs-d802` item 8's docs; the Plan's knowledge item.)

**Sources:** [contract §4.6](../contract.md) (the Context store, retirement and
the holographic engine rail), [contract §4.1](../contract.md) (the one database),
[contract §5](../contract.md) (Events), [contract §3.7](../contract.md) (E8/E9),
[commands §3.13](../commands.md) (`context distill`), [commands
§3.5](../commands.md) (`context decide`/`note`/`ask`), [commands
§3.6](../commands.md) (`context answer`), [CONTEXT — Context record / Context
store / Distilled Context / Event](../CONTEXT.md). Model: `hs-d802`.

**Verdict:** confirmed. Records, the derived holographic index and curation are
one plane; the Event log is another.

**Nuance (not a disagreement).** Model (`hs-d802`, `cpd context show --record hs-d802`): "separate from the Event ledger". Docs
agree on separation, but both live in the *same* SQLite database: [contract
§4.6](../contract.md) says "**cpd.db stays the source of truth**; the engine is a
derived per-Context index under `<context_engine_dir>/<context>.db` ... projected
lazily at search time, never on writes", and [contract §4.1](../contract.md) has
the `events` table beside `context_records`. "Separate plane" means separate
tables and a derived index, not a second store; the vault note must say so.

## 9. Two queue-like concepts: the message board and the attention inbox

**Claim.** Two queue-like concepts by design: the message board (agent-to-agent
exchange that keeps the Context registry clean) and the attention inbox (keeps
the User on top of what matters). (`hs-d802`, `cpd context show --record hs-d802` item 7.)

**Sources:** [commands §10a](../commands.md) (the message board; "It appends no
Event and never starts a turn (hs-d514)"), [commands §21](../commands.md) (the
Inbox; "the single source of 'needs you'"), [contract
§2.2](../contract.md) (Inbox item), [contract §3.7](../contract.md) (E8), [CONTEXT
— Board post / Inbox / Inbox item](../CONTEXT.md). Model: `hs-d802`.

**Verdict:** confirmed, and the "never starts a turn" guarantee is explicit in
the docs.

**No disagreement.** [commands §10a.1](../commands.md): a board post "appends no
Event and never starts a turn (hs-d514)"; posts are picked up at turn start by
`cpd board take`, and "Important facts stay Context records". [commands
§21](../commands.md): "The Inbox is the single source of 'needs you': one stored
item per thing that needs the User, opened and closed by the engine (E8) from its
cause, never by a command."

## 10. The edge and self-hosting

**Claim.** GitHub is the PR plane, 1Password the secrets edge, OpenRouter the
model plane, the LGTM exporter pending and fail-open, install/host repair, store
migrations, the PROTOCOL contract, and the dotfiles config chain chezmoidata →
config.toml with hs-d795 as the cautionary drift. (`hs-d802`, `cpd context show --record hs-d802` item 8's
docs; the Plan's edge item.)

**Sources:** [contract §7.6](../contract.md) (GitHub/pull requests), [contract
§8.4](../contract.md) (secrets/1Password), [contract §8.3](../contract.md)
(Policies, `[model]`), [contract §9](../contract.md) (models come from pi),
[contract §4.5](../contract.md) (store migrations), [contract
§6.14](../contract.md) (the host socket protocol v1), [contract
§5.2](../contract.md) (the events `PROTOCOL` number), [commands
§14.4](../commands.md) (`install`), [commands §9.11](../commands.md) (`host
repair`), [commands §22](../commands.md) (`policy`), [commands
§23](../commands.md) (`secret`), [commands §24](../commands.md) (`mcp`). Records:
`hs-d795`, `hs-d798` (`cpd context show --record <id>`); model: `hs-d802`.

**Verdict:** partly confirmed. GitHub, 1Password,
install/host repair, migrations and the socket protocol are documented; the
"OpenRouter = the model plane" and the dotfiles chain are not what the docs say;
the LGTM exporter is not in the docs at all.

**Disagreement (OpenRouter):** Model (`hs-d802`, `cpd context show --record hs-d802`): "OpenRouter the model plane." Docs:
[contract §3.7 E15](../contract.md) and [contract §9](../contract.md) say "cpd
has **no provider list and names no key variable**: the providers are found by
the helper, from pi" and "cpd does not know the name" of a custom provider;
`[model]` is a Policy domain ([contract §8.3](../contract.md)), and OpenRouter
appears only as one *example* provider in the preset refs of [contract
§8.2](../contract.md) (`openrouter/deepseek/deepseek-v4-pro`). The model plane
is pi + the `[model]` domain, not OpenRouter.

**Disagreement (LGTM exporter):** Model (`hs-d802`, `cpd context show --record hs-d802`): "the LGTM exporter pending/fail-open."
Docs: no occurrence of `LGTM`, `Loki`, `Grafana` or `Prometheus` in
`contract.md` or `commands.md` (grep, this worktree). It rests on `hs-d798`
(`cpd context show --record hs-d798`): "cpd observability ... a 'cpd metrics'
query with a Prometheus text output format. The LGTM stack ... is the sink, not a
dependency ... nothing in cpd may depend on the exporter being up (fail-open).
Plan: **metrics**." So the exporter is pending (an unmerged Plan) and fail-open
by decision, but it is **unverified** against the current cpd docs because it
does not exist there yet.

**Disagreement (dotfiles chain):** Model (`hs-d802`, `cpd context show --record hs-d802`): "the dotfiles config chain chezmoidata
-> config.toml with hs-d795 as the cautionary drift." Docs: [contract
§8.1](../contract.md) says only that `config.toml` "is managed by the User's
dotfiles (requirements §6, kept)"; no cpd doc carries the `chezmoidata ->
config.toml` chain (grep for `chezmoidata` in `contract.md`/`commands.md`/
`implementing.md` hits only font and nextest pins). The drift itself is real but
lives in the dotfiles and the record: `hs-d795` (`cpd context show --record
hs-d795`) — "machines.yaml hs still said 6, so a chezmoid re-render would have
silently reverted hs-d529's 5; dotfiles now carries 8". The chain is therefore
**unverified** against cpd's docs and sourced only to the record.

**Ambiguity (PROTOCOL):** Model (`hs-d802`, `cpd context show --record hs-d802`): "the PROTOCOL contract." Docs use `PROTOCOL`
for two different things: the **events protocol number** ([contract
§5.2](../contract.md), "Today it is 19", raised when an Event kind or an
adapter-read `data` field changes; [versioning.md](../versioning.md) says "Today
it is **13**" — a stale number in a second doc) and the **host socket protocol
v1** ([contract §6.14](../contract.md)). The note must disambiguate; the model's
one word does not.

## 11. What cpd deliberately is not (the boundaries)

**Claim.** cpd is deliberately not per-plan daemons, not a message-queue service
and not an orchestration server; it is one binary, one store, one engine process,
one (removal-marked) tmux server, with disposable agent sessions. (`hs-d802`, `cpd context show --record hs-d802`
item 8's docs; the Plan's boundaries item.)

**Sources:** [contract §1.1](../contract.md) (one binary, one engine per machine,
state in SQLite, events the single channel, tmux the only host, no `bd`/`wt`/`herdr`),
[contract §4.1](../contract.md) (one store), [contract §6.1](../contract.md) (one
tmux server), [CONTEXT — Engine](../CONTEXT.md) ("The one `cpd watch` per machine
... _Avoid_: daemon, scheduler, one watcher per Workflow"), [ADR-0001](../adr/0001-sqlite-workflow-state.md),
[ADR-0004](../adr/0004-beads-ledger.md), [ADR-0010](../adr/0010-one-rust-binary-in-steps.md).
Model: `hs-d802`.

**Verdict:** partly confirmed. "One binary", "one engine process", "no
per-plan daemons", "no orchestration server" and "disposable sessions" match;
"one store" rests on a doc whose ADR lineage contradicts it; "one tmux server
removal-marked" and "no message-queue service" need qualification.

**Disagreement (one store, ADR lineage):** Model (`hs-d802`, `cpd context show --record hs-d802`): "one ... store." Docs:
[contract §1.1](../contract.md) (R1) says "State in SQLite inside the binary:
one database per machine", but the ADRs disagree with each other —
[ADR-0001](../adr/0001-sqlite-workflow-state.md) is "**superseded by ADR-0004**",
and [ADR-0004](../adr/0004-beads-ledger.md) (accepted, supersedes ADR-0001) says
"Each Workflow's ... live in a **beads (`bd`) workspace** stored outside the
product repository", which cpd 2 no longer uses ([contract
§1.1](../contract.md): "No `bd`, no `wt` (worktrunk), no `herdr`"). The current
contract and [architecture §1.3](../architecture.md) say SQLite, so "one store"
holds today, but the model's citation trail (ADR-0004) states the opposite and
the boundary note must say which ADR is live.

**Disagreement (message-queue service):** Model (`hs-d802`, `cpd context show --record hs-d802`): "no message-queue service."
Docs: [contract §1.1](../contract.md) says "There are no **queue directories**, no
cache directories and no tmux user options carrying state", and Events are "the
single channel" — no external broker. But cpd does keep internal, SQLite-backed
queues: `slack_reactions` ([contract §4.4](../contract.md)), `jira_transitions`
([contract §4.4](../contract.md)) and the board posts ([commands
§10a](../commands.md)). "No message-queue service" is true of external services,
not of internal queue tables; the note must say so.

**Disagreement (tmux server, removal-marked):** Model (`hs-d802`, `cpd context show --record hs-d802`): "one tmux server
**removal-marked**." Docs: [contract §1.1](../contract.md) and [contract
§6.1](../contract.md) have one tmux server per machine, but "tmux is the only
host" — the removal is the future P3 decision (`hs-d752`), not a mark in the docs
(see item 3). Same disagreement as item 3's tmux line.

**Confirmed sub-claims (no disagreement):** "one binary" ([ADR-0010](../adr/0010-one-rust-binary-in-steps.md);
[architecture §3](../architecture.md)); "one engine process" ([CONTEXT —
Engine](../CONTEXT.md), [contract §3.7](../contract.md), D5); "no per-plan
daemons" ([CONTEXT — Engine](../CONTEXT.md) _Avoid_ line); "no orchestration
server" ([ADR-0005](../adr/0005-cpd-cli-control-plane.md), the CLI is the control
plane); "disposable agent sessions" ([contract §3.7](../contract.md) E12 archives
stopped Worker Sessions; [CONTEXT — Session](../CONTEXT.md)).

## Disagreements

1. **SDK** (item 1): model "SDK = core + ledger + engine"; [contract §1.1](../contract.md)
   and [architecture §3](../architecture.md) name the crates core/cli/tui/engine
   and the `cpd.db` store, and no doc contains the string `SDK`.
2. **Engine step range** (item 2): model "E1-E21 except E5" and `hs-d802`;
   [CONTEXT — engine step](../CONTEXT.md) says "E1 to E17";
   [contract §3.7](../contract.md) says E1–E21 with E6 removed and E5 in the set.
3. **"Different timings"** (item 2): model says the two clocks have different
   timings; [contract §8.2](../contract.md) gives both default 5 s, so only the
   clock is different, not the interval.
4. **Engine moves work Context→Dispatch** (item 2): model; [CONTEXT —
   Engine](../CONTEXT.md) describes scheduling/liveness/PR-poll/curation, and
   the stage progression is the Manager's and Lead's.
5. **pi-durable is *the* runner** (item 3): model; [contract §6.12](../contract.md)
   calls it "a second runner beside pi, not a replacement", default `pi`.
6. **Not agent-agnostic** (item 3): model; [ADR-0005](../adr/0005-cpd-cli-control-plane.md)
   keeps the path open to Claude Code and Hermes.
7. **tmux removal-mark** (items 3, 11): model; [contract §1.1](../contract.md)
   still says "tmux is the only host", and removal is the future P3 (`hs-d752`).
8. **Auxiliary is the fourth role** (item 4): model; [CONTEXT —
   Roles](../CONTEXT.md) lists User as the fourth entry and `auxiliary` only as a
   Tier category ([contract §9](../contract.md)).
9. **Context/Plan/Dispatch are phases** (item 4): model; [CONTEXT — Phase, stage
   and step](../CONTEXT.md) calls them *stages*, and [contract
   §3.1](../contract.md) has phases grounding→plan→dispatch→deploy→done.
10. **Worker attached to Dispatch** (item 4): model; [CONTEXT — Work
    units](../CONTEXT.md) attaches `Job → Worker`.
11. **Which hook is which gate** (item 6): model "merge gate by pre-commit/pre-merge
    hooks"; [contract §7.3](../contract.md) makes `pre-commit` the merge gate and
    `pre-merge` the pre-PR Gate.
12. **"Gates marked for deprecation"** (item 6): model; [contract
    §8.3](../contract.md) already defaults `gate.merge`/`gate.pr` to `none`, and
    only the `[pr] gate` alias is deprecated.
13. **Per-attempt worktrees** (item 7): model; [contract §7.2](../contract.md)
    reuses the Lead worktree for a linear Attempt.
14. **OpenRouter is the model plane** (item 10): model; [contract
    §3.7](../contract.md) and [contract §9](../contract.md) say cpd names no
    provider, and OpenRouter is one example in [contract §8.2](../contract.md).
15. **Dotfiles chain chezmoidata → config.toml** (item 10): model; [contract
    §8.1](../contract.md) says only "managed by the User's dotfiles", and no cpd
    doc carries the chain; sources are `hs-d795` and the dotfiles.
16. **LGTM exporter** (item 10): model says pending/fail-open; no cpd doc names
    it (`unverified` against the docs); source is the unmerged Plan `metrics` in
    `hs-d798`.
17. **PROTOCOL ambiguity** (item 10): model's one word covers both the events
    protocol number ([contract §5.2](../contract.md) = 19;
    [versioning.md](../versioning.md) says 13 — stale) and the host socket
    protocol v1 ([contract §6.14](../contract.md)).
18. **One store vs ADR-0004** (item 11): model "one store";
    [ADR-0004](../adr/0004-beads-ledger.md) (accepted, superseding
    [ADR-0001](../adr/0001-sqlite-workflow-state.md)) says beads, while [contract
    §1.1](../contract.md) and [architecture §1.3](../architecture.md) say SQLite.
19. **No message-queue service** (item 11): model; cpd keeps internal queue
    tables (`slack_reactions`, `jira_transitions`, board posts) beside the "no
    queue directories" rule of [contract §1.1](../contract.md).
20. **CONTEXT.md stale step range** (item 2): [CONTEXT — engine
    step](../CONTEXT.md) "E1 to E17" vs [contract §3.7](../contract.md) E1–E21.
21. **versioning.md stale PROTOCOL** (item 10): [versioning.md](../versioning.md)
    "Today it is 13" vs [contract §5.2](../contract.md) "Today it is 19".

## Sources

Files read for this verification (all relative to the repo root):

- `CONTEXT.md`
- `docs/contract.md`
- `docs/commands.md`
- `docs/architecture.md`
- `docs/implementing.md`
- `docs/versioning.md`
- `docs/adr/0001-sqlite-workflow-state.md`
- `docs/adr/0004-beads-ledger.md`
- `docs/adr/0005-cpd-cli-control-plane.md`
- `docs/adr/0010-one-rust-binary-in-steps.md`
- `docs/research/gate-transition.md`
- `docs/research/durable-leads.md`
- `docs/research/web-ui.md`

Context records read with `cpd context show --record <id>`:

- `hs-d802` (the model this file verifies)
- `hs-d752` (durable runners; dropping tmux is P3)
- `hs-d753` (CLI-first)
- `hs-d795` (dotfiles drift)
- `hs-d798` (LGTM exporter / Plan `metrics`)

Not opened for this attempt: `engine/src/`, `core/src/`, `cli/src/`, `pi/`, and
`scripts/prune-worktrees.sh` (its current content is one sentence, quoted by
[commands §23](../commands.md) and [implementing.md](../implementing.md)).
