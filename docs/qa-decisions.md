# QA Protocol decisions and research

Provenance: the User's QA Protocol design conversation, 2026-11-13. These are
explicit User decisions unless described as implementation choices below.
Setup research and acceptance criteria: [issue #1115](https://github.com/clayton-duarte/cpd/issues/1115).
The original Protocol was adopted in PR #1116 and the 20/10 limits in PR #1132.
Portable extraction was adopted in PR #1149; migration to its own source repo
is tracked in issue #1152. No new QA run starts during this
distillation. Each run still needs the User's explicit start/resume instruction.

## User decisions

| ID | Decision | Reason / consequence |
| --- | --- | --- |
| QA-01 | Scope includes everything: docs, UX/UI, tests, dependencies, prompts and core architecture. | Quality must cover complete product journeys and maintainability. |
| QA-02 | Start with installation/migration ordering, stale/incomplete external sync and rate limits, and three LAN instances. | These are the User's current pain points, not preconfirmed defects. |
| QA-03 | Three independent full instances; one WebUI switches backends through a dropdown. | No shared store or synchronization between instances is requested. Host definitions remain in dotfiles. |
| QA-04 | Every defensible finding becomes a GH issue; check duplicates first. | The initial goal includes backlog accumulation. No manufactured findings or preference-only complaints. |
| QA-05 | Issues hold reasons/research; PRs focus on implementation. | Keep durable reasoning discoverable without bloating implementation review. |
| QA-06 | Loop: find two issues, prioritize all open issues, implement one fix, open a PR, repeat. Discovery may end after 30 minutes with fewer findings. | Bound exploration without inventing defects. |
| QA-07 | At most 10 new issues and 5 new PRs per America/Toronto day, combined across CPD and dotfiles; closed and draft items count. Updates do not. | Original User limits; superseded by QA-22 on 2026-11-13. DST handling and independent accounting remain. |
| QA-08 | Prefer code to prose for deterministic rules; check at loop start and immediately before creation; fail closed on unverifiable counts; one active loop. | Executable limits with an explicit single-operator assumption. |
| QA-09 | CPD is the priority; trace bugs into dotfiles and fix there when the evidence warrants it. | Avoid unrelated dotfiles maintenance. |
| QA-10 | User reviews and merges every PR; agent may research and propose any change size. | Architecture/dependency/migration work is allowed, with evidence; no autonomous merge. |
| QA-11 | Intended product behavior > applicable upstream/community guidance > local contracts/docs > code. Intent must be in docs or clarified with the User. | Current behavior can be defective; disagreements require explicit documentation. |
| QA-12 | Use needs-human-input for unresolved intent/decisions. | Continue independent work while making the blocking question visible. |
| QA-13 | Simplicity and minimalism; deleting code can improve quality. Prefer suitable OSS over inferior custom solutions, particularly pi extensions. | Research the existing solution and its dependencies before removal or replacement. |
| QA-14 | Proportionate evidence and research; research industry documentation patterns too. | Routine fixes need direct evidence; consequential changes need alternatives and trade-offs. |
| QA-15 | All decisions, processes and adopted patterns must be documented. | Record lasting intent in repo docs; link issue research and implementation PRs. |
| QA-16 | Lint/format on commit, changed-area test subsets locally; CI runs automated checks. | Match local verification to changed behavior; do not substitute CI for focused evidence. |
| QA-17 | Local tool installs and disposable instances allowed. Remote access is read-only and needs a separate access plan, deferred to the next session. | This PR neither configures SSH nor modifies live hosts. |
| QA-18 | Use Git/GitHub directly first; consider CPD after proving the Protocol. | No premature dependency on the system being investigated. |
| QA-19 | Initial PR contains the Protocol, decisions and tested limit scripts only. Begin actual QA only after merge and explicit start. | No product fixes, automatic schedule or active investigation loop in this change. |
| QA-20 | Name this document the QA Protocol; CPD calls collections of procedures and methods Protocols. | User clarification, 2026-11-13; glossary and ADR-0018 record the vocabulary and standalone execution. |
| QA-21 | The User may explicitly reset today's allowance by starting a named dated batch while preserving all prior resources and aggregate totals. | User instruction, 2026-11-13 ([authorization record](https://github.com/clayton-duarte/cpd/issues/1115#issuecomment-6854012406)). Record authorization in GitHub; no autonomous resets or marker removal. |
| QA-22 | Permanent limits are now 20 new issues and 10 new PRs, shared across CPD/dotfiles, per Toronto day or explicitly reset batch. | User instruction, 2026-11-13; supersedes QA-07's numeric limits. The currently requested reset uses the new limits. |
| QA-23 | Make the method reproducible by a fresh agent in any repository using a self-contained kit, explicit project config/profile and durable GitHub handoffs. | User request after the measured pilot, 2026-11-13; implementation proposal and acceptance criteria in [#1148](https://github.com/clayton-duarte/cpd/issues/1148). No new loop, scheduler or live-host authority. |
| QA-24 | Distinguish iteration, User-started run, daily allowance and explicit reset batch; enforce optional lower run caps from GitHub run markers. | Distillation of the 10/5 measurement run versus permanent 20/10 limits; intent-only run descriptors are not a local creation ledger. Existing attribution is preserved. |
| QA-25 | Record the measured 10-issue/5-PR run as approximately a little over one weekly allowance percentage point, based on User dashboard readings. | User observed 80% before, 79% at start and 78% after; rounded readings are not exact telemetry or a guaranteed weekly throughput. [Run record](qa-runs/2026-11-13.md). |
| QA-26 | Adopt the open Agent Skills package layout with a manual Markdown entry, one generic implementation, configurable repo/base/timezone/scope/caps and a CPD compatibility entry point. | Researched implementation choice for User review; [portable pattern sources](../tools/repo-qa/references/RESEARCH.md). Discovery differs by agent; no universal auto-install claim. |
| QA-27 | The canonical reusable toolkit moves to `clayton-duarte/skills`, package `skills/repo-qa`; CPD consumes an immutable-revision, hash-verified vendored snapshot. | User explicitly requested creation/migration on 2026-11-13. Keeps CPD operational without secondary private-repo checkout credentials. [Source/consumer ownership](qa-source.md), tracking [#1152](https://github.com/clayton-duarte/cpd/issues/1152). |
| QA-28 | Count the explicitly authorized skills-source migration in the existing shared CPD/dotfiles allowance by including `clayton-duarte/skills` in the canonical allowlist. | Conservative implementation choice for the User-authorized related source, preserving 20/10 and all history rather than granting a fresh allowance. Unrelated consumers still require explicit grouping decisions; no automatic parallel writers. |

## Adopted patterns and evidence

Existing CPD conventions were reconciled after merging `origin/main` at
`2f4001fd`: [the Protocol registry](protocols.md) is the index, and QA's entry
states its standalone enforcement and lack of CPD breach events. The Bug report
Protocol supplies mitigation/root-fix/regression distinctions; QA retains the
User's explicit GitHub-first and User-merge decisions instead of adopting the
Manager's Merge Protocol implicitly. ADR-0018 records the vocabulary refinement.

Sources consulted 2026-11-13. Published guidance demonstrates established practice,
not universal consensus; the daily limits, ordering, authority hierarchy and
autonomy are User policy, not claims about industry consensus.

### Continuous code health and actionable findings

[Google Engineering Practices: review standard](https://google.github.io/eng-practices/review/reviewer/standard.html)
favors changes that improve code health without demanding perfection and puts
technical facts ahead of personal preferences. Its
[review criteria](https://google.github.io/eng-practices/review/reviewer/looking-for.html)
cover design, functionality, complexity, tests and documentation, including
over-engineering. Adopt measurable improvement and coherent changes rather than
an arbitrary code-size, coverage or complexity score.

[Mozilla bug-writing guidance](https://bugzilla.mozilla.org/page.cgi?id=bug-writing.html)
supports precise reproduction, expected/actual results, relevant environment,
duplicates and one issue per problem. Adopt these for runtime defects; adapt
the evidence to source-level contradictions and maintenance findings rather
than forcing fake reproduction steps on documentation issues.

### Proportionate research: ordinary fix, ADR, substantial design

The [Rust RFC process](https://rust-lang.github.io/rfcs) explicitly allows ordinary
bug fixes and documentation improvements through normal PRs, reserving design
discussion for substantial changes. Adopt proportionality; do not mandate a
separate RFC for every fix or copy Rust's governance process into CPD.

[Google Cloud's ADR guidance](https://docs.cloud.google.com/architecture/architecture-decision-records)
captures requirements, options and the selected decision near the code.
[AWS's ADR process](https://docs.aws.amazon.com/prescriptive-guidance/latest/architectural-decision-records/adr-process.html)
and [best practices](https://docs.aws.amazon.com/prescriptive-guidance/latest/architectural-decision-records/best-practices.html)
cover significant structural/dependency decisions, review, status and preserving
superseded history. [GDS guidance](https://gds-way.digital.cabinet-office.gov.uk/standards/architecture-decisions.html)
likewise keeps application decisions in their repository and distinguishes ADRs
from full architecture documentation.

Adopt: research in issues; current intended behavior in owning docs; significant
architectural decisions in the existing `docs/adr/` structure; links between
them. Record context, options, choice, consequences, evidence, status and
supersession. This Protocol's own decisions fit in this single table rather
than generating nineteen separate ADRs. No claim is made that these sources
mandate the exact issue/PR split chosen by the User.

### OSS evaluation and subtraction

The User's preference for suitable OSS and deletion drives this policy.
Google's review criteria support avoiding unnecessary complexity. OSS adoption
is evaluated against fit, supported versions, license, maintenance, security,
integration and migration/removal cost; upstream documentation/source and
maintained implementations are evidence. Neither community popularity nor fewer
local lines proves a replacement improves the product. No specific replacement
or pi extension is selected in this PR.

### Deterministic budget accounting

[GitHub REST issues documentation](https://docs.github.com/en/rest/issues/issues#list-repository-issues)
specifies repository listings, `state=all`, `since` (updated time), pagination,
and the `pull_request` discriminator. [GitHub CLI api documentation](https://cli.github.com/manual/gh_api)
documents structured JSON input and explicit methods. The script requests
numbered REST pages explicitly because the environment's older CLI lacks `--slurp`.
`scripts/qa.mjs` uses these interfaces, exact local Toronto creation-date filtering,
and an attribution marker in the creation payload. Node's built-in
[Intl.DateTimeFormat](https://nodejs.org/api/intl.html) supplies IANA time-zone
handling without a timezone dependency. Focused tests cover the boundaries.

Implementation choices: dependency-free Node fits this repo; REST listing avoids
search-index lag; a body marker is present at creation for both issues and PRs
without a later labeling window. Count all marked items regardless of author,
state or label. Labels alone were rejected because PR labeling is a separate
write and labels can be changed. A local ledger was rejected as the primary
counter because it can diverge from GitHub. Distributed reservation machinery
is unnecessary under the explicitly agreed single-loop constraint. Marker
deletion, bypass and concurrent writers remain documented limitations.

### Verification and iteration records

Use existing repo checks and focused behavioral regression tests, with evidence
of failure and success for defect fixes. The current CPD gate is `pnpm run verify`;
the [implementation guide](implementing.md#the-scoped-loop) describes scoped
verification. Local formatting/lint and relevant tests precede commit; CI
provides the complete configured gate. The User retains review and merge.

An iteration comment on the selected issue records work and next steps; GitHub
is the durable run record. No parallel Markdown backlog, local quota ledger or
schedule is introduced. Revisit Protocol design through a new documented
decision when actual execution exposes a limitation.

### Explicitly authorized batches and revised limits

The User requested a new batch after the earlier allowance was exhausted, then
permanently raised limits to 20 issues and 10 PRs (2026-11-13). The implementation
reuses existing GitHub attribution rather than deleting history or maintaining
a local creation ledger: a dated batch marker scopes the active allowance,
while every resource keeps its original marker and contributes to day totals.
The batch identifier must match today's Toronto date; invalid/stale identifiers
fail before API access. Starting a batch is an explicit User policy exception,
not an automatic response to exhaustion. This is a User operating decision,
not a claim about industry consensus. Existing REST/pagination and atomic body
marker evidence above supports the mechanism; tests pin scoping and boundaries.

### Distillation and cold-start reproducibility

The portable source is `tools/repo-qa/`; CPD policy is `qa.config.json` plus the
CPD profile. The existing `scripts/qa.mjs` entry and historical markers remain.
The generic Protocol owns common procedures once; project facts, decisions and
run retrospectives remain in CPD docs. Adopters configure their repository/base,
permissions and User-approved grouping rather than inheriting private CPD facts.
One serialized operator still owns a shared group. No distributed service or
provider-specific memory was introduced.

The cold-start experiment copies the whole kit into an unrelated temporary
location, uses another repo/base/timezone, invokes fresh Node processes from a
different working directory, and substitutes a fake GitHub CLI. It verifies
atomic literal markers, restart recovery and independent run/day refusals.
Malformed/unverifiable inputs fail closed. A two-day ordinary REST lookback
replaces the Toronto-specific 26-hour optimization; exact timezone day filtering
preserves counts, while a long run extends listings to its immutable start.

Run descriptors store authorization/ID/start/limits only. GitHub markers and
all-state REST listings are the ledger; copied counts in handoffs are snapshots.
Changing ID/start or scope to regain allowance is forbidden. A reset does not
reset a run cap. The script cannot authenticate prose permission references,
coordinate concurrent writers or prevent deliberate bypass; these limits are
explicit, rather than claims of automatic governance.

Today showed why reproducers must survive the workspace, why a fake boundary
alone does not prove the real caller permits a bug, why CI state must be current,
and why unrelated test failures need a base comparison. Those lessons now appear
in the generic procedure and handoff template. Earlier issue comments remain the
durable rationale; [the run record](qa-runs/2026-11-13.md) indexes the evidence.

### Canonical skills source and pinned consumers

The User requested `clayton-duarte/skills` after adopting the portable kit. Only
`tools/repo-qa` is extracted, with exact original commit/tree provenance; CPD's
private product Git history, machine definitions, profile and pilot records stay
here. The new repository matches CPD's private visibility; no public release or
new license is implied. The User created the empty repo after the integration's
create-repository endpoint returned 403. README initialization supplies a PR
base; actual migration/consumer feature changes remain User-reviewed PRs.

[The source migration record](https://github.com/clayton-duarte/skills/blob/9ea7f767ffbeddc3c0ff8adb0679a3d1b200f859/docs/repo-qa-migration.md)
compares a pinned vendored copy against submodule/private-token setup and package
publication. Git's upstream subtree documentation supports self-contained embedded
consumers/extraction: https://github.com/git/git/blob/master/contrib/subtree/git-subtree.adoc .
GitHub checkout documents private secondary-repo token needs:
https://github.com/actions/checkout#checkout-multiple-repos-private .
Sources consulted 2026-11-13. The dependency-free copy-based adoption method is
retained; no new updater service, registry or credential is required. The local
copy is deliberately a consumer, with upstream revision/file hashes verified in
CPD tests. Changes go upstream first, then consumers update through reviewed PRs.
