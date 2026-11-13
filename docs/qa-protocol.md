# QA Protocol

Status: proposed until this PR is merged. The loop starts only after the User
explicitly starts it after merge. No scheduler, live-host access or CPD adoption
is enabled by this document. Decisions and research: [QA decisions](qa-decisions.md).
This Protocol is registered in [Protocols](protocols.md); vocabulary lives in
the [Protocol glossary entry](../CONTEXT.md#checks-and-gates).

## Relationship to existing Protocols

The registry's **Bug report** Protocol distinguishes the immediate mitigation,
the root-cause fix and the regression test. Preserve those distinctions in a
QA bug issue: document a safe mitigation or why none is available, then research
the root fix and its regression evidence. Standalone QA records these in GitHub;
it does not require CPD bug commands or introduce duplicate CPD bug records.
The live-host restrictions below still apply: a mitigation is not authorization
to change the User's running instance. If live mitigation is needed, state the
action and request User input.

The registry's **Merge** Protocol describes a CPD Manager's integration path;
the QA operator uses the explicitly agreed User-review-and-merge policy instead.
Other Protocols apply according to their documented actors, triggers and the
User's authorization. This document adds no secret access or external messaging
authority. Refactors must preserve intended behavior and use relevant existing
checks; changing intended behavior requires an explicit documented decision.

## Definition of quality

Quality means the software delivers its documented intended behavior correctly,
reliably, securely and efficiently, with a usable interface, trustworthy data,
and code, tests and documentation that are simple to understand and change.
Assess installation, upgrades, migration safety, external synchronization,
failure recovery, UI/accessibility, architecture, dependencies, agent prompts
and pi extensions, CI, developer experience and documentation.

An improvement must demonstrate user impact, reduced maintenance cost, measured
resource savings, or correction of an explicit requirement or convention.
Personal preference alone is not a finding. Complexity, duplication, dead code,
contradictions and missing tests are candidates, not automatically defects.
Deletion is a valid fix: establish why the code exists and what depends on it
before removing it. Prefer appropriate maintained OSS to custom implementations,
especially pi extensions; compare suitability and total costs, not popularity alone.

Intended product behavior must be explicitly documented in the repository or
clarified with the User and then documented. Authority, in descending order:

1. Explicit intended product behavior, documented or confirmed by the User.
2. Applicable community guidance and upstream documentation.
3. Local contracts and other local documentation.
4. Current code.

Observed behavior does not establish intended behavior. External guidance is
evidence, not permission to change the product. Record contradictions rather
than silently treating one document as obsolete. A change that departs from a
contract updates it in the same PR. If intent or competing guidance cannot be
resolved, apply `needs-human-input`, state the exact question and alternatives,
and continue with an independent issue. Every decision is documented; proposals
remain proposals until the User reviews and merges them.

## Scope and first exploration priorities

Start with `clayton-duarte/cpd`. Trace into `clayton-duarte/dotfiles` only when
evidence places a CPD issue's cause or necessary fix there. File the issue in
the owning repo, and link dependent cross-repo issues/PRs without duplicating
the same finding. Both repos share one daily budget.

Initial priorities, not yet confirmed defect reports:

1. Installation and upgrade reliability, particularly out-of-order database
   migrations: fresh installs, existing-store upgrades, branch histories,
   interrupted execution, compatibility and recovery.
2. External connections through to the client: stale data, incomplete sync,
   invalidation, pagination, rate-limit handling, retries and observable errors.
3. Three independent CPD instances, one per machine, with a WebUI backend
   dropdown. Each machine runs a full instance and owns its database/state;
   the UI connects to and switches between backends. No shared database or
   backend-to-backend state synchronization is implied. Verify connection
   lifecycle and that data, subscriptions and pending actions cannot cross hosts.

`dotfiles/home/private_dot_ssh/private_config` declares `homeserver`, `mac-pro`
and `mac-air`; `home/.chezmoidata/machines.yaml` owns machine configuration.
Reference these sources instead of copying addresses into QA documentation.
Read-only SSH diagnosis is authorized in principle but access must be separately
planned; it is not configured here. Never install, migrate, restart, deploy or
modify configuration/state on the live machines as part of this loop. Local
tool installation and disposable instances are authorized. Use synthetic data,
isolated `CPD_HOME` and test credentials; never mutate the User's live store.

## Limits and deterministic tools

One active QA loop across both repos. Per `America/Toronto` calendar day,
including daylight-saving transitions: **at most 10 new issues and 5 new PRs**.
The purpose initially includes accumulating a useful backlog: issue creation
can continue after the PR allowance is exhausted. These are ceilings, not quotas.
Updates/comments on existing items consume neither allowance. Closed issues,
closed/merged PRs and drafts still count on their creation day. Reopening does
not reset creation time. The setup issue and PR use the same accounting.

Use Node (the repo already requires Node >=22.19) and authenticated `gh` with
read access to both repos and write access to the target. From the CPD checkout:

```sh
node scripts/qa.mjs status
node scripts/qa.mjs issue clayton-duarte/cpd 'Concrete problem' /tmp/qa-issue.md
node scripts/qa.mjs pr clayton-duarte/cpd qa/123-short-name 'Concrete change' /tmp/qa-pr.md
```

The creator reads a literal body file, rechecks both repos immediately before
creation and injects `<!-- cpd-repo-qa:v1 -->` into the body atomically. Preserve
that marker through edits; do not delete or transfer counted items. Attribution
uses the marker, not the author's identity or a mutable process label. Creation
always targets `main` for PRs. Set any needed labels through GitHub afterward.
Use `needs-human-input` for unresolved decisions; create that label once if absent.

Status prints JSON with the Toronto day, counts, limits, remaining allowances
and independent creation permissions. It uses paginated REST listings of all
states and distinguishes PRs using `pull_request`. It reads a 26-hour updated
window (Toronto days are at most 25 hours), then filters by creation day locally.
No GitHub search index or local counter is the source of truth.

Exit codes: `0` successful status/creation; `3` creation refused at its cap;
`1` invalid input or failed/unverifiable API operation. Status exits `0` even
when exhausted: inspect its permissions, or use the creator which enforces them.
If counting fails, no creation occurs. On write timeout/failure, inspect GitHub
for the marker, title and branch before retrying: a write can succeed remotely
without returning a response. Never blindly retry a creation.

This is deterministic enforcement for a **single serialized operator using the
creator**. It cannot prevent another writer bypassing it, simultaneous operators,
marker removal, deleted items, or GitHub's response delay. Do not launch another
loop or bypass the creator. Adding distributed workers requires a new decision
and stronger coordination, not an unsupported claim of atomic quota enforcement.

## Exploration steps

At the beginning of every iteration:

1. Read this Protocol, its decisions and applicable `AGENTS.md` instructions.
   Run the status script. Read the previous iteration's issue comment, if any.
   Verify clean worktrees, current base SHAs and the existing issues/open PRs.
2. Search open **and closed** issues, PRs, previous audits and ADRs for duplicates,
   rejected proposals and in-flight fixes. Historical audits such as
   [quality.md](research/quality.md) are leads, not current evidence. Reproduce
   against current `main` before reviving an old finding.
3. Explore until two new defensible findings are filed, or 30 minutes elapse.
   If the issue allowance is exhausted, skip new discovery and work the backlog.
   If only one slot remains, seek at most one new issue. Never manufacture a
   finding to hit a count. When the PR allowance is exhausted, continue discovery
   passes up to the issue allowance, then stop.
4. Follow complete paths: intent/docs -> entry point -> core/store -> runtime or
   external boundary -> protocol -> UI state and rendering -> failure/recovery.
   Inspect boundaries, concurrency, cancellation, partial failures, ordering,
   stale responses and resource cleanup. Exercise user journeys locally where
   useful; static analysis alone does not establish runtime behavior.
5. Record repo@SHA, affected paths, expected/observed behavior, commands and
   outputs, environment, and confidence. Runtime claims require reproduction
   when feasible; a source-level contradiction can be established by paired
   citations. Performance claims require a reproducible baseline and workload.
6. Every defensible finding becomes a GitHub issue through the creator, unless
   already tracked: update/link that issue instead. A credible unresolved
   hypothesis becomes an explicitly marked investigation issue with evidence
   and a bounded experiment, never a confirmed bug. Incomplete suspicions stay
   exploration notes until they meet this threshold.

## Prioritization

After discovery, review the **whole open backlog**, including issues not created
by QA. Choose one actionable issue with no active implementation PR. Do not
prioritize only today's discoveries. Order by severity and user impact, then
frequency/exposure, evidence confidence, dependencies and effort. Explain the
selection briefly in the selected issue. Use these severity meanings in its body:

| Severity | Meaning |
| --- | --- |
| Critical | Data loss/corruption, serious security exposure, unusable install/upgrade with no reasonable recovery. |
| High | Broken core journey, unsafe migration ordering, persistent incomplete synchronization, materially wrong UI state. |
| Medium | Bounded functional defects, significant maintenance or performance costs with evidence. |
| Low | Limited-impact clarity, consistency, usability or cleanup improvements with a concrete benefit. |

Severity is distinct from confidence and change size. A blocked critical issue
still stays critical. Add `needs-human-input` to blocked decisions and take the
next independent actionable issue. Large architectural fixes are allowed, with
proportionate research; split them into coherent linked changes when appropriate.
Never select work by number of lines deleted, issues filed or tests added.

## Research steps

Before implementation, expand the selected issue into an actionable investigation:

1. Establish intended behavior and inspect actual callers, implementation,
   tests, historical decisions and upstream versions. Separate verified facts,
   hypotheses and unknowns. Preserve evidence that contradicts the initial idea.
2. Consult current official documentation/source for relevant APIs, libraries
   and standards. Look for adopted community approaches in mature projects and
   relevant upstream issues. Cite exact URLs, versions and access dates; record
   repo@SHA for code evidence. Do not assert consensus from one blog or vendor.
3. Compare the current approach, deletion/simplification, an existing OSS solution
   and a small local fix where applicable. Evaluate compatibility, maintenance,
   licensing, security, operational burden, migration and removal costs. For pi
   extensions specifically, check upstream and community replacements first.
4. Scale the artifact to the decision. A factual doc correction needs direct
   evidence, not an RFC. An architectural/dependency change needs alternatives,
   trade-offs, consequences, reversibility and a focused experiment/prototype
   when uncertainty warrants it. Benchmark efficiency claims before and after.
5. Write the rationale and recommendation in the issue. Document lasting intent
   in the owning repo docs in the implementation PR, with an ADR for significant
   architectural choices. Link instead of duplicating long research. Keep old
   decisions and mark superseded decisions with their replacement.

## Implementation

1. Refresh the budget and selected issue's current state. Work from updated
   `origin/main` in an isolated branch/worktree such as `qa/123-short-name`.
   Read the target repo's instructions. Do not write fixes on `main` or mix
   unrelated cleanup. If evidence locates the root cause in dotfiles, move that
   work there with linked issues, while CPD remains the priority.
2. State checkable acceptance criteria in the issue before coding. For a
   behavioral defect, demonstrate the failing behavior and add a meaningful
   regression check where appropriate, then verify the fix. Do not add tests
   that merely mirror implementation or artificial tests for trivial prose.
3. Implement the smallest coherent root-cause fix. Remove superseded code and
   obsolete dependencies when justified; avoid parallel custom implementations,
   speculative abstractions, silent fallbacks and unrelated scope expansion.
   Update affected docs/contracts and document every new decision.
4. Run the existing applicable lint/format checks **before each commit**. Use
   the repo's tools and conventions, not a new formatter introduced incidentally.
   Run the relevant test subset based on changed behavior and affected callers;
   include build/type checks where needed. Follow CPD's
   [scoped-loop guidance](implementing.md#the-scoped-loop) and `scripts/test-changed`.
   For this dependency-free QA tooling: `node --check scripts/qa.mjs`,
   `node --check scripts/qa.test.mjs`, `node --test scripts/qa.test.mjs`, and
   `sh scripts/check-portable.sh`, and `git diff --check`.
   Syntax and whitespace checks are not a claim of semantic
   linting; no JS lint/formatter is configured for these files. Add broader
   checks when risk or failures warrant them. CI runs the full configured gate.
   Record blocked checks and their reasons; never report unrun checks as passing.
5. Commit with one coherent intent, push and open a linked PR through the creator.
   The issue is the rationale/research record. The PR focuses on changed behavior,
   implementation, trade-offs, validation and remaining limitations, and links
   to the issue and any ADR. A small amount of problem context makes it readable.
   User review and merge are mandatory: never self-approve, merge, enable
   auto-merge or deploy. Fix CI failures and review feedback on that same PR.
6. Leave a concise iteration comment on the selected issue: base/head SHAs,
   areas explored, findings/duplicates, chosen priority, decision/research links,
   validation, PR link, unresolved questions and next area. If no selected issue
   exists, put the handoff in the chat; do not create a tracking issue just to
   store it. End with a refreshed budget. Repeat, respecting independent caps.
   When both are exhausted, stop until the next explicitly started session;
   no scheduler is installed. Check existing PR feedback in subsequent sessions.
7. A PR being opened is not a resolved issue. Let the User's merge close it via
   the linked closing reference when applicable; verify closure after merge and
   retain residual work as linked issues. CPD adoption and process automation
   can be proposed after this Protocol has proved useful.

## Issue format

Preserve the injected attribution marker when editing. Use one independently
actionable finding per issue, grouping multiple symptoms only for a shared cause.

```markdown
## Problem and intended behavior
User impact; expected versus actual; intent source or question for the User.

## Evidence
Repo@SHA, paths/permalinks, environment, reproduction and observed outputs.
Confidence: reproduced / source-confirmed / investigation (state unknowns).
Severity: critical / high / medium / low, with impact-based justification.

## Research and decision
Sources with versions and access dates; prior art; alternatives (including
deletion and OSS where applicable); trade-offs; recommendation and status.
Link lasting decision docs. For needs-human-input: the exact question/options.

## Acceptance criteria
Observable outcomes and the focused checks that establish them.

## Scope and links
Boundaries, dependencies, duplicates, related issues/PRs and follow-ups.
```

## PR format

Follow the repo's existing template, or CPD's
[PR description guidance](../pi/skills/cpd-lead/references/pr-description.md).
Include: implementation and resulting behavior; linked issue (`Fixes #N` or
`Fixes owner/repo#N`); validation results; significant risks/limitations and
decision-doc links. Research stays in the issue; durable decisions stay in docs.
