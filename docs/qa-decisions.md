# QA Protocol decisions and research

Provenance: the User's QA Protocol design conversation, 2026-11-13. These are
explicit User decisions unless described as implementation choices below.
Setup research and acceptance criteria: [issue #1115](https://github.com/clayton-duarte/cpd/issues/1115).
The Protocol and scripts are proposed for User review through this PR; the
QA loop remains inactive until merge and an explicit start instruction.

## User decisions

| ID | Decision | Reason / consequence |
| --- | --- | --- |
| QA-01 | Scope includes everything: docs, UX/UI, tests, dependencies, prompts and core architecture. | Quality must cover complete product journeys and maintainability. |
| QA-02 | Start with installation/migration ordering, stale/incomplete external sync and rate limits, and three LAN instances. | These are the User's current pain points, not preconfirmed defects. |
| QA-03 | Three independent full instances; one WebUI switches backends through a dropdown. | No shared store or synchronization between instances is requested. Host definitions remain in dotfiles. |
| QA-04 | Every defensible finding becomes a GH issue; check duplicates first. | The initial goal includes backlog accumulation. No manufactured findings or preference-only complaints. |
| QA-05 | Issues hold reasons/research; PRs focus on implementation. | Keep durable reasoning discoverable without bloating implementation review. |
| QA-06 | Loop: find two issues, prioritize all open issues, implement one fix, open a PR, repeat. Discovery may end after 30 minutes with fewer findings. | Bound exploration without inventing defects. |
| QA-07 | At most 10 new issues and 5 new PRs per America/Toronto day, combined across CPD and dotfiles; closed and draft items count. Updates do not. | User-specified autonomy limits; account for DST, not fixed EST. Each cap is independent. |
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
