# Portable repository QA

Canonical source: [`clayton-duarte/skills`, `skills/repo-qa`](https://github.com/clayton-duarte/skills/tree/main/skills/repo-qa).
Improve the package there; adopters pin a source commit and keep their own project
profile/config. See the source repository's migration record for provenance.

This directory is a self-contained QA Protocol and budget guard. Copy the whole
`repo-qa` directory into a repository you control; configure its policy; give any
agent [SKILL.md](SKILL.md) as its entry point. It needs no CPD service, pi runtime,
provider-specific chat memory, npm install or background scheduler.

The package follows the open [Agent Skills specification](https://agentskills.io/specification).
An agent supporting that format can install it in its documented skill location.
Discovery paths differ: copying it into `tools/` alone does not guarantee automatic
discovery. For any other agent, explicitly ask it to read SKILL.md. Optionally add
one link to the repo's existing AGENTS.md or equivalent, scoped to QA tasks; do
not replace existing instructions or activate QA for unrelated work.

## Adopt in another repository

1. Start on an isolated branch. Read repo/contributor instructions and establish
   the User's intended product behavior, allowed repositories, base branches,
   tooling, constraints, daily limits and review/merge policy. Request only missing
   decisions. Installing this kit is not permission to access live systems.
2. Copy this directory, for example to `tools/repo-qa`. Copy
   `assets/config.example.json` to the repository root as `qa.config.json` and
   replace `OWNER/REPO`, base branch and scope. Add related repos only when
   authorized. The example's 20/10 and Toronto timezone are configurable policy,
   not universal defaults inferred from the agent's environment.
3. Use one canonical config for each budget group. The scope derives immutable
   body markers. Never change scope/allowlist to hide earlier creations. Preserve
   copied historical markers when migrating an existing group. Disjoint repo
   groups can run independently; overlapping groups require the same config and
   one serialized operator. This kit does not coordinate concurrent writers.
4. Start with `assets/profile.md` for the project QA profile: product intent/docs, priorities, repo and
   related-repo boundaries, commands/versions, test policy, live-host restrictions,
   human-input label, decisions source, run/handoff location. Validate facts
   against this repo rather than copying another project's assumptions.
5. Check tools/access with `node --version`, `git --version`, `gh auth status`
   and the status command below. Use the repository's pinned dependency versions.
   Do not inspect token files or log credentials. The configured API account needs
   read access to every counted repo and issue/PR creation access to the target.
6. Run the bundled tests below. Open the adoption issue/PR using the repo's
   approved path, record decisions, and have the User review/merge. Do not start
   a discovery loop until the User explicitly starts it.

GitHub.com issue/PR creation is the currently implemented provider. Other forges
need an adapter with equivalent pagination/counting tests; do not claim that the
existing `gh api` implementation supports them. Language/framework/build tools
are project profile facts, not dependencies of the generic guard.

## Start or resume an authorized run

An **iteration** finds up to two defensible issues (or explores for 30 minutes),
prioritizes the open backlog and opens one fix PR. A **run** is a User-started set
of iterations with creation ceilings, such as 10 issues/5 PRs. The **daily
allowance** is independent, such as 20/10. A User-authorized **reset batch** is a
recorded exception to today's daily allowance; it is not a routine iteration or
run. Numbers are ceilings, never a requirement to invent findings.

Copy `assets/run.example.json` to a tracked run descriptor or recover its exact
JSON from the canonical GitHub handoff. Set a unique ID, actual UTC start,
authorized caps and authorization reference. Record that descriptor in an existing
QA issue comment before creation; save its permalink in the project profile/run
record. The file stores intent only, never creation counts. Its ID and start are
immutable across sessions; lowering caps is allowed, raising them requires a new
recorded User decision. A new session must recover the same descriptor, not reset
it. Do not create an issue merely to store a handoff when no finding/setup issue
exists; use the existing authorized repo run record or chat handoff instead.

From the target repo root (or pass an absolute `--config` path):

```sh
node tools/repo-qa/scripts/qa.mjs --config qa.config.json --run qa-run.json status
node tools/repo-qa/scripts/qa.mjs --config qa.config.json --run qa-run.json issue OWNER/REPO 'Concrete finding' /tmp/qa-issue.md
node tools/repo-qa/scripts/qa.mjs --config qa.config.json --run qa-run.json pr OWNER/REPO qa/123-fix 'Concrete change' /tmp/qa-pr.md
```

Use `assets/issue.md`, `assets/pr.md` and `assets/handoff.md`. The body files are
literal input, not shell programs. The creator adds attribution/run/batch markers
in the same API payload. Preserve every marker on edits. Config controls the PR
base independently per repository; it need not be `main`.

For an explicitly authorized daily reset, record the instruction, then use the
same `QA_BATCH=YYYY-MM-DD-reset-N` on every command. The date uses the configured
timezone. A stale/empty/malformed batch fails before API calls. A reset does not
reset a run's own cap. Without `--run`, only daily/batch caps apply; therefore
always pass the descriptor for a bounded run.

Status exposes `counts` (today's active daily/reset allowance), `day_counts`
(all attributed creations today), `daily_remaining`, `run.counts/remaining` when
supplied, and effective `remaining`/`can_create_issue`/`can_create_pr` after both
caps. REST listings include closed/merged/draft items across all configured repos;
updates consume neither cap. Search and local counters are not the ledger. Long
runs extend the listing window to their recorded start; keep that start immutable.
The two-day ordinary lookback is conservatively wider than Toronto's original
26-hour window; exact local creation-day filtering still determines day counts.

Exit 0 means successful status/creation (status can report exhaustion); 3 means
creation refused at a cap; 1 means invalid input or unverifiable/failed API work.
On a failed/timed-out write, inspect the marker/title/head remotely before retrying.
A write can succeed without its response arriving. No automatic write retries.

This is a guard for one trusted, serialized operator. It cannot authenticate the
text of a User authorization reference, prevent a writer bypassing it or changing
config/markers, coordinate overlapping workers, or recover deleted resources.
Run/scope changes and quota resets are governance decisions, not ways around a
refusal. Weekly account usage is a different resource; optional enforcement is described below.

## Optional continuous weekly allowance mode

Only an explicit User instruction can replace cardinal limits. Set config
`"limits": null` and add
`"weekly_allowance": {"min_remaining_percent": 50, "max_age_seconds": 60}`
to allow continuous iterations while **at least** 50% remains. Run descriptors
may also use `"limits": null`; any finite daily or run cap still applies. Existing
finite configurations need no changes. Counts and markers remain auditable;
`null` remaining means no cardinal ceiling, never zero or a fabricated large cap.
Record the User decision and immutable run descriptor in the adopter's handoff.

This mode requires a live official Codex account reader, not a dashboard estimate.
The optional helper requires Python >=3.9 and a Codex CLI supporting the account
app-server API and ephemeral credential store (tested with 0.167.0-alpha.4):

```sh
python3 tools/repo-qa/scripts/codex-usage-reader.py --output /tmp/qa-weekly-usage.json --min-remaining 50
# In the QA session, after completing the displayed official device login:
export QA_USAGE_FILE=/tmp/qa-weekly-usage.json
node tools/repo-qa/scripts/qa.mjs --config qa.config.json --run qa-run.json status
```

Keep the reader process alive; it holds the expressly authorized login in memory,
polls every 30 seconds and publishes only sanitized usage via atomic mode-0600
files. It performs no inference, account reset or credit purchase, reads no auth
file, and does not persist a login. Termination/errors remove its snapshot. A lost
process/login needs a new explicit device sign-in; do not promise that a cloud
session survives disconnects or that a reader can keep an inactive agent running.
The helper is telemetry, not an autonomous agent scheduler. Use a private local
output directory; a writable forged snapshot is outside this trusted-operator guard.

The guard requires the Codex 10,080-minute window, a valid upcoming reset, a
sample no older than the configured 1..60 seconds, and explicit availability/
spend-control booleans. Missing, malformed, future, stale or reset telemetry fails
closed. Under-threshold/unavailable accounts refuse creation. Check status before
every iteration and significant stage, and before/after lengthy checks; creation
also checks again after GitHub pagination immediately before writing. Stop active
work when below threshold or unverifiable; preserve a handoff. Exactly 50% allows
continuation. Never automatically spend reset credits or restart after a reset.
Polling and rounded account percentages cannot reserve the cost of the final
response or exclude concurrent consumers; this is a sampled stop policy, not an
exact usage ceiling. The reader alone cannot interrupt a running model response.

## Verify the kit

No npm dependencies or repository services are needed:

```sh
node --check tools/repo-qa/scripts/qa.mjs
# Optional reader tests (Python >=3.9):
PYTHONDONTWRITEBYTECODE=1 python3 tools/repo-qa/scripts/codex-usage-reader.test.py
node --test tools/repo-qa/scripts/qa.test.mjs tools/repo-qa/scripts/portability.test.mjs
```

The cold-start test copies the whole package into a temporary unrelated directory,
uses a different repo/base/timezone/config, starts fresh CLI processes from another
working directory, and substitutes a fake `gh`. It tests literal payloads, resume,
independent run caps, malformed metadata and refusal without extra writes. It does
not create a remote repo, issue or PR. The environment must allow local child
processes and executable temporary files. If sandbox policy blocks those, report
the blocked test and run it in an authorized environment; do not silently skip it.

## Bootstrap prompt

> Read `<kit>/SKILL.md`, the target repo's QA profile/config, applicable agent
> instructions, and the canonical run handoff. Ground yourself in current code,
> issues and PR feedback. This User instruction starts/resumes run `<ID>` under
> the attached/referenced descriptor and authorized repository scope. Recheck
> live budgets, follow the QA Protocol, document decisions, and leave a durable
> handoff. Stop at the configured count/weekly limits, unavailable telemetry or an explicit stop. User reviews and merges.

Fill the placeholders and point to concrete records before starting. Unknown
scope, permissions or intended product behavior must not be guessed.
