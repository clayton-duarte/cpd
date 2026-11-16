# QA Protocol

Read the adopting repository's profile/config and applicable agent/contributor
instructions alongside this document. The User must explicitly start/resume a
run. Installation does not activate it. [README](../README.md) defines adoption,
commands, accounting and the bootstrap prompt; [research](RESEARCH.md) records
the patterns behind this Protocol. Repo policy and User decisions stay in the
project's own decision record. No chat history is required to execute it.

## Definition of quality

Quality means delivering documented intended behavior correctly, reliably,
securely and efficiently, with usable interfaces, trustworthy data, and code,
tests and documentation that are simple to understand and change. Scope can
include docs, installation/upgrades/migrations, external sync, UI/accessibility,
architecture, dependencies, prompts/extensions, CI and developer experience.

A finding demonstrates user impact, reduced maintenance cost, measured resource
savings, or correction of an explicit requirement/convention. Code smells,
duplication, dead code and missing tests are leads, not automatic defects.
Personal preference and issue counts are not quality measures. Deletion is a
valid fix: establish why code exists and what depends on it before removing it.
Prefer suitable maintained OSS when it reduces total complexity/cost, especially
for custom extensions; compare fit and maintenance rather than popularity.

Authority, in descending order:

1. Explicit intended product behavior documented in the repo or clarified with
   the User, then recorded.
2. Applicable community guidance and upstream documentation.
3. Local contracts and other local documentation.
4. Current code.

Observed code is not proof of intent. Research is evidence, not permission to
change product behavior. Record contradictions and counterevidence. If intent
remains unresolved, create/update an evidence-backed investigation issue,
apply `needs-human-input`, give the exact question/options, and continue an
independent issue. A durable policy change updates owning docs/contracts in
its PR; significant architectural choices use the repo's ADR/RFC convention.

## Run boundaries and ownership

An iteration is discovery (two defensible findings or 30 minutes), prioritization
of the whole backlog, and one researched fix PR. A run is an explicitly started
set of iterations with explicit creation caps or a User-authorized live weekly allowance threshold. Daily caps and explicitly
User-reset batches are separate. All are ceilings; never manufacture findings,
split one cause into extra issues, or weaken validation to fill them.

Use the configured creator and the same immutable run descriptor for every
creation/status call. GitHub is the count source. Optional weekly mode requires fresh official account telemetry and an immutable run descriptor for creation; its start must belong to the current weekly window. An old run never renews authorization at reset. Descriptor-free status is inspection only; see the README. Closed/merged/draft items still
count, and comments/updates do not. Setup work uses the allowance too. A daily
reset needs a recorded User instruction and preserves aggregate totals; changing
run IDs, scope or config to regain allowance is forbidden. Status failures mean
no further work until verification is restored. Failed writes need remote inspection before retry, not blind replay.

One serialized operator per budget group. Different agents may use disjoint
repository groups; overlapping repositories need one canonical config and
serialized creation. This is not a distributed scheduler or atomic reservation
service. Follow the authorized primary/related-repo relationship; trace a finding
to its owning repo and link cross-repo dependencies instead of duplicating it.

The agent researches, files issues, implements on branches and opens PRs within
that authorization. The User reviews and merges. No self-approval, auto-merge,
deployment, scheduler, agent delegation or live-host change is implied. Scope and
live-access rules must be explicit in the project profile; otherwise use local
isolated test instances and synthetic data. Do not send unrelated external
messages. Existing User authorization persists across sessions when documented.

## Exploration steps

At the start of a run and every iteration:

1. Read current User scope/decisions, this Protocol, repo profile/config,
   applicable agent/contributor instructions, run descriptor and last canonical
   handoff. Recover outstanding CI/review/conflict work before discovering more.
   Run `pr-status` for every open attributed QA PR, including older runs; resolve
   conflicts on its existing branch and run scoped checks. Pending mergeability
   is unconfirmed and must be rechecked. Repeat after observing base/head changes
   and after pushing a resolution; never merge the PR autonomously. Recheck
   GitHub budgets and configured live weekly allowance; copied counts in a handoff are historical observations.
2. Verify working tree ownership/cleanliness, base branch/SHA, origin and tools.
   Use the project's pinned versions. Fetch current base safely; never discard
   others' edits or write fixes on the primary branch. Read current issues/PRs.
3. Ground the repo: product docs/intended journeys -> entry points -> services/
   storage/external boundaries -> protocols -> UI/state -> failure/recovery.
   Identify migration/ordering, cancellation, stale response, concurrency, rate
   limiting and cleanup risks appropriate to this repo. Record a compact map in
   the first handoff; do not inherit another project's architecture as fact.
4. Search open AND closed issues/PRs, previous audits, decisions and in-flight
   fixes before filing. Historical findings are leads to reproduce against the
   current base. Search indexing can lag: inspect exact records when necessary.
5. Explore until two new defensible findings are filed or 30 minutes elapse.
   Respect the remaining run/day issue cap; if it is exhausted, work the backlog.
   If the PR cap is exhausted but the issue cap remains, continue bounded
   discovery passes, then stop. Do not invent work to complete a target.
6. Trace actual callers and guards. A mock accepting an invalid call does not
   prove the real caller permits it. Distinguish reproduced behavior,
   source-confirmed contradiction, and unresolved investigation. Include base
   SHA, environment and exact expected/actual behavior. Quantitative efficiency
   claims need a reproducible workload and baseline.
7. Every defensible finding becomes a GitHub issue through the guard unless
   already tracked. Preserve one independently actionable cause per issue.
   Update/retract a claim when counterevidence warrants it. Incomplete suspicions
   stay exploration notes, not confirmed bugs. Label unresolved intent clearly.

Use [the issue template](../assets/issue.md). A runtime reproduction must be
replayable from a named base with a minimal fixture, exact command and observed
output. Save code in the issue/comment, tracked evidence or a stable artifact;
never leave the only fixture under a session's `/tmp` path. No real customer data,
credentials, running user database or unplanned live-host access.

## Prioritization

Review the entire open backlog, including issues not created by QA. Select one
actionable item with no active implementation PR. Explain the choice in that
issue: severity/user impact first, then exposure/frequency, evidence confidence,
dependencies and effort. Do not rank only today's findings or lines of code.

- Critical: data loss/corruption, serious security exposure, unusable install or
  upgrade with no reasonable recovery.
- High: broken core journey, unsafe migration ordering, persistent incomplete
  synchronization, materially incorrect user state.
- Medium: bounded functional defects, substantial maintenance/resource cost with
  evidence.
- Low: limited-impact clarity, consistency or usability improvements with a
  concrete benefit.

Severity, confidence and change size are distinct. A blocked critical issue stays
critical: label the decision/dependency and take the next independent item.
Large architecture/dependency/migration fixes are allowed when authorized, with
proportionate research and coherent implementation boundaries.

## Research steps

Before coding:

1. Establish intended behavior, real callers, guards, tests, historical decisions
   and upstream versions. Separate facts, hypotheses and unknowns, and preserve
   evidence that contradicts the initial theory. State acceptance criteria.
2. Read current official documentation/source for relevant APIs/standards. Consult
   mature prior art/upstream issues where useful. Cite exact URLs, versions and
   access date; source code evidence includes repo@SHA. One vendor/blog does not
   establish community consensus. Do not turn a policy preference into an
   unsupported industry-consensus claim.
3. Compare status quo, deletion/simplification, suitable maintained OSS, and a
   small local fix where applicable. Evaluate compatibility, licensing,
   maintenance, security, operational burden, migration and removal costs.
   Check upstream/community replacements before rebuilding custom extensions.
4. Scale the artifact: direct evidence is enough for a factual doc correction;
   significant structural/dependency changes need alternatives, tradeoffs,
   consequences, reversibility, and a focused experiment when uncertain.
   Benchmark efficiency claims before/after; do not invent speedup numbers.
5. Record rationale/research and proposed decisions in the issue before the PR.
   Put lasting intended behavior in owning docs/contracts and important
   architecture in the repo's ADR/RFC form. Link artifacts instead of duplicating
   a long analysis. Preserve superseded decisions and their replacements.

Identify mitigation, root fix and regression separately. Apply a mitigation only
inside authorized scope; otherwise describe it and the needed decision. A live
restart is not a root fix and is not authorized by a local QA run.

## Implementation

1. Refresh budgets and issue/PR state, branch from current configured base in an
   isolated worktree/checkout, and read applicable local instructions. Do not mix
   unrelated cleanup or reimplement an already active PR.
2. Reproduce the behavioral defect before the fix where feasible. Add meaningful
   regressions that fail against the original implementation and pass after the
   fix. Do not add tests merely for trivial prose or to mirror implementation.
   Existing behavior-preserving refactors must obey any adopted refactor Protocol.
3. Implement the smallest coherent root fix, removing superseded code or
   dependencies when justified. Preserve explicit contracts or change their
   documented intent deliberately in the same PR. No silent fallback that hides
   failure or speculative abstraction unrelated to the finding.
4. Run existing applicable lint/format checks BEFORE EACH COMMIT. Run scoped
   tests for changed behavior/callers plus relevant type/build checks. Use pinned
   repo tools; do not introduce a formatter/framework incidentally. Syntax and
   whitespace checks are not semantic linting. Report unconfigured/blocked/unrun
   checks honestly. Broaden tests only when new changes, failures or unresolved
   concerns warrant it. Compare unexpected failures against unchanged base in
   the same environment; report limits without asserting an unproven cause.
5. Commit/push and open a linked PR through the guard. Use [the PR template](../assets/pr.md)
   or the repo's equivalent: resulting behavior, implementation/tradeoffs,
   validation and material limitations. The issue holds research/rationale.
   User review/merge remains required. A PR is not a resolved issue.
6. Monitor mergeability, CI and review feedback; resolve conflicts and correct
   failures caused by this change on the
   same PR, preserving markers. Keep enough state to resume queued/running CI.
   Do not claim it passed because a scoped test passed. Do not modify another
   agent's unrelated PR merely because it is red.
7. Write [a durable handoff](../assets/handoff.md) in the selected issue comment
   or the canonical repo run record: authorization/config/descriptor, issue/PR
   links, base/head, evidence, decisions, checks/limitations, actual CI status,
   unresolved questions, next action and refreshed guard counts. If no issue
   exists, use the authorized repo/chat handoff; do not manufacture a tracking
   issue. An expiring workspace must not erase the only reproducer or run state.
8. Repeat only inside the User's configured count/weekly limits. Refresh weekly usage at every significant stage; stop active work below threshold or when unverifiable. On exhaustion or an explicit stop,
   save the handoff and pause. Starting a new day/run/reset requires the User's
   instruction; no automatic restart is installed. After User merge, verify
   closure and preserve any residual work as linked issues.

## Account usage and learning

GitHub creation budgets and model-account allowance are different resources.
When measuring, record source/time and remaining percentage before/after the
same bounded run, meter resolution, other concurrent activity and uncertainty.
Use exposed account telemetry or a User-provided dashboard reading. If telemetry
is unavailable, say so; do not infer exact tokens/cost from issue/PR counts.
One rounded observation supports an approximate planning estimate, not a weekly
throughput guarantee or a reason to manufacture work.

After a pilot, distill observed failures, useful checks, decisions and continuity
gaps into the Protocol/profile/tooling by normal reviewed changes. Keep the
reusable method separate from project facts, historical results and private
operational details. A fresh agent must be able to reproduce the workflow from
the package, profile, config and GitHub handoff without previous chat memory.


### Optional weekly calendar pacing

A User may replace a fixed weekly floor with the README's `daily_pacing` policy.
The remaining reserve is `d * 100 / 7`, Monday `d=6` through Sunday `0`; stop at
or below that reserve. Savings carry forward. An official Sunday reset starts
a new window, whose balance is held until the configured Sunday release hour,
then Monday's allocation is available through Monday. Use the configured IANA
timezone and live account balance; never manufacture a reset or assume a 100%
sample. The optional telemetry reader uses `--min-remaining 0`, while the guard
computes and enforces the moving floor. Freshness, account availability, spend
controls, immutable run authority and review/merge rules remain unchanged. A
changed reset weekday needs policy reconciliation. No scheduling is installed.
