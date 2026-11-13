# Adopted patterns and sources

Consulted 2026-11-13. These sources support established practices, not universal
consensus about every policy. Daily caps, authority ordering, User-only merge and
exact iteration structure are User operating decisions.

- [Agent Skills specification](https://agentskills.io/specification): a versioned
  directory with SKILL.md plus optional scripts/references/assets, self-contained
  scripts with explicit dependencies, and progressive disclosure. Adopt that
  package format without requiring a particular agent runtime or automatic
  installation. [Overview](https://agentskills.io/home) explains cross-product
  reuse; plain Markdown is the fallback.
- [AAIF: effective AGENTS.md](https://aaif.io/blog/writing-an-effective-agents-md):
  concise project facts, local instructions, no secrets, explicit User precedence,
  and differences in discovery between agent implementations. Use an optional
  QA-scoped link, not a second copy of the Protocol or a new global agent policy.
- [Google review standard](https://google.github.io/eng-practices/review/reviewer/standard.html)
  and [review criteria](https://google.github.io/eng-practices/review/reviewer/looking-for.html):
  code health, function/design/tests/docs, technical evidence ahead of preference,
  and avoidance of unnecessary complexity. Findings need a concrete benefit.
- [Mozilla bug-writing guidance](https://bugzilla.mozilla.org/page.cgi?id=bug-writing.html):
  duplicates, reproducible evidence, expected/actual behavior, environment and one
  problem per issue. Keep research in issues and implementation in linked PRs as
  the User's chosen split; do not pretend the source mandates that exact split.
- [Rust RFC process](https://rust-lang.github.io/rfcs): normal fixes need not be
  large RFCs. [AWS ADR guidance](https://docs.aws.amazon.com/prescriptive-guidance/latest/architectural-decision-records/adr-process.html)
  and [GDS](https://gds-way.digital.cabinet-office.gov.uk/standards/architecture-decisions.html):
  keep significant decisions near code, capture options/consequences and preserve
  supersession. Use the adopting repository's conventions.
- [GitHub repository issues REST API](https://docs.github.com/en/rest/issues/issues#list-repository-issues):
  `state=all`, updated-time `since`, pagination, and `pull_request` discriminator.
  [GitHub CLI api](https://cli.github.com/manual/gh_api): structured stdin JSON and
  explicit methods. Count exact creation dates and immutable body attribution,
  including merged/closed/draft work; avoid search lag and local counters.
- [Node Intl](https://nodejs.org/api/intl.html): IANA timezone formatting. The guard
  uses built-ins, `.mjs` files and a fake-CLI cold-start test rather than a new
  dependency/agent service. Repo tools remain separate project facts.

Implementation decisions: explicit validated config rather than guessed remote
or base branch; one implementation plus a compatibility entry point; intent-only
run descriptors plus GitHub run markers to enforce lower run caps across fresh
sessions; one serialized creator per shared budget group. This intentionally
avoids duplicate docs/engines, provider-specific memory, a local quota ledger,
automatic resets, distributed orchestration and token-cost estimation.

These mechanisms are reviewed software, not proof that an arbitrary agent will
follow every prose rule. Tests establish accounting/portability behavior; User
review and evidence-backed issues establish the judgment-dependent parts.


Weekly allowance mode (2026-11-13): the official
[Codex app-server](https://github.com/openai/codex/tree/main/codex-rs/app-server)
and [protocol](https://github.com/openai/codex/tree/main/codex-rs/app-server-protocol)
expose account login and rate-limit reads. Installed CLI 0.167.0-alpha.4 generated
schemas and an authorized device-login experiment confirmed the 10,080-minute
window may be primary or secondary, with usedPercent/reset and account/spend
availability. Use those semantics rather than guessing cost from GitHub counts.
An optional stdlib Python telemetry helper uses the official CLI with ephemeral
credentials, leaving the Node/GitHub core unchanged for other agents/providers.
No suitable generic account meter is assumed; unsupported CLI/provider data fails
closed. Null cardinal caps require an explicit weekly policy, with a maximum
60-second age, strict under-threshold stop, and a fresh check before writes.
These are User policy choices and conservative implementation decisions, not
claims of universal community consensus. Polling is not a cost reservation or a
scheduler and cannot guarantee an exact final percentage.


Reusable authentication correction (2026-11-13): official
[Codex authentication](https://developers.openai.com/codex/auth) and
[configuration reference](https://developers.openai.com/codex/config-reference)
document `file`, `keyring`, `auto` and `ephemeral` stores. Cached ChatGPT tokens
refresh automatically during use; `ephemeral` deliberately loses login when its
process ends. The original memory-only experiment therefore cannot satisfy an
unattended workflow across restarts. Add opt-in private reusable CLI state and
one-shot reads, keeping the managed cloud login separate and refusing credentials
inside Git. Prefer the OS credential store where the environment provides it;
this headless cloud environment exposes no user secret-service bus. File storage
is the official headless option, with private directory/file permissions. Tests
restart the fake app-server in separate reader processes and verify no second
device flow. They establish lifecycle behavior, not live token-refresh success.
The cloud subscription's native model connection supplies no exposed account-meter
tool; do not claim that being an OpenAI-hosted agent grants the workspace billing
credentials. This correction follows the User's explicit demand to avoid repeated
sign-ins; it supersedes treating ephemeral auth as the only supported reader mode.
