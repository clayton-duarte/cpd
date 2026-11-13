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
node tools/repo-qa/scripts/qa.mjs --config qa.config.json --run qa-run.json pr-status
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
reset a run's own cap. Without `--run`, count-only mode applies daily/batch caps; weekly mode refuses creation until a descriptor is supplied. Therefore
always pass the descriptor for a bounded run.

Status exposes `counts` (today's active daily/reset allowance), `day_counts`
(all attributed creations today), `daily_remaining`, `run.counts/remaining` when
supplied, and effective `remaining`/`can_create_issue`/`can_create_pr` after both
caps. REST listings include closed/merged/draft items across all configured repos;
updates consume neither cap. Search and local counters are not the ledger. Long
runs extend the listing window to their recorded start; keep that start immutable.
The two-day ordinary lookback is conservatively wider than Toronto's original
26-hour window; exact local creation-day filtering still determines day counts.

`pr-status` is a read-only upkeep gate for all open PRs carrying the configured
scope marker, including previous runs and older PRs outside the counting lookback.
It paginates each configured repo, fetches current head/base and nullable GitHub
mergeability, and reports `mergeable`, `conflicted` or `pending`. Exit 4 means
conflicted/pending PRs need attention before new discovery; pending metadata needs
a later read. Exit 1 means metadata/API work could not be verified; exit 3 means
weekly allowance refused the check. Maintenance stays available when only creation
count caps are exhausted. No PR, review or branch is modified by this command.
Inspect CI/review feedback too: mergeability alone is not approval or a passed gate.
The reported revisions are a snapshot; recheck after base/head changes and after
pushing a conflict resolution. Resolve on the existing branch, preserve both
intended changes, run relevant checks, and leave the User to review/merge.

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

The command above selects disposable, memory-only authentication. It requires a
new device sign-in after losing its process and is unsuitable for unattended work
that must survive restarts. It remains available for Users who require that mode.

For explicitly authorized reusable authentication, choose a **private persistent
directory outside every Git repository**, then sign in once:

```sh
QA_USAGE_AUTH_DIR=/private/persistent/location/qa-codex-auth
python3 tools/repo-qa/scripts/codex-usage-reader.py --auth-dir "$QA_USAGE_AUTH_DIR" --login --once --output /tmp/qa-weekly-usage.json --min-remaining 50
# Subsequent checks, including after a reader/process restart, do not sign in:
python3 tools/repo-qa/scripts/codex-usage-reader.py --auth-dir "$QA_USAGE_AUTH_DIR" --once --output /tmp/qa-weekly-usage.json --min-remaining 50
```

This uses the official CLI's `file` credential store and documented child-process
home selector. It never changes the parent environment or managed Codex login;
choosing that managed directory, a Git directory, or a credential symlink is
refused. On Unix the directory is mode0700 and cached credentials mode0600.
No credential content is read or printed by this helper. Codex owns token refresh.
Treat the directory as account credentials: keep it out of commits, logs and
shared artifacts. A missing login fails without prompting unless `--login` was
explicitly supplied. `--once` leaves only a sanitized sample valid for the guard's
short freshness window; invoke it before each iteration/significant stage/write.
Without `--once` the reader polls every30 seconds. No model calls, quota resets,
credit purchases or agent scheduler are involved.

[Official authentication documentation](https://developers.openai.com/codex/auth)
confirms cached login and automatic token refresh. This avoids repeated login
on normal process restarts; revocation, a lost credential volume or account
policy can still require sign-in. The helper cannot keep an inactive agent
running or make a destroyed cloud workspace persistent. An already-lost
memory-only login cannot be converted into a saved one. The supported permanent
platform improvement would be a read-only native allowance tool; no such tool
is exposed to this agent. Cloud model access and a separate workspace CLI login
must not be assumed to share quota-read authorization.

The reader pins the first observed weekly reset. It stops and removes its snapshot
when that window expires or a subsequent response changes it. A new window needs
explicit User authorization; persistent authentication does not renew QA authority.
Live reads may differ by one second due to reset-time rounding; tolerate that
rounding while keeping the reader expiry at the earliest observed boundary.
The guard refreshes status after GitHub pagination and rejects a window change
within a status/creation operation. One-shot users must also preserve the initially
authorized window in their run handoff and stop before spending reset allowance.
Across fresh processes, the guard binds each supplied run's immutable `started_at`
to the current half-open weekly interval `[resets_at - window_minutes, resets_at)`.
An older run is rejected before GitHub access. Weekly creation requires `--run`;
standalone `status` without one remains inspection only, reports `run_required: true`
and sets both `can_create_*` flags false. It does not authorize continuation.
Current-window resumption is supported, including a run started exactly at the
window boundary. Count-only runs can still span weeks. After a weekly rollover,
record new explicit User authorization and a new descriptor; never rewrite an old
run's start or ID to bypass this check. The guard enforces the descriptor's dates,
not the authenticity of its prose authorization reference.

A continuous reader removes its sample on orderly exit/errors; an uncatchable
kill leaves a file that expires within60 seconds. Use a private local output
directory; a writable forged snapshot is outside this trusted-operator guard.

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
> issues and PR feedback, CI and mergeability. This User instruction starts/resumes run `<ID>` under
> the attached/referenced descriptor and authorized repository scope. Recheck
> live budgets, follow the QA Protocol, document decisions, and leave a durable
> handoff. Stop at the configured count/weekly limits, unavailable telemetry or an explicit stop. User reviews and merges.

Fill the placeholders and point to concrete records before starting. Unknown
scope, permissions or intended product behavior must not be guessed.
