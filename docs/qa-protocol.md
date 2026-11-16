# CPD QA Protocol profile

The QA Protocol was adopted through [#1116](https://github.com/clayton-duarte/cpd/pull/1116)
and its permanent 20/10 limits through [#1132](https://github.com/clayton-duarte/cpd/pull/1132).
The portable implementation was adopted through [#1149](https://github.com/clayton-duarte/cpd/pull/1149).
Canonical ownership moved to [`clayton-duarte/skills`](https://github.com/clayton-duarte/skills)
in [#1152](https://github.com/clayton-duarte/cpd/issues/1152); CPD retains a [pinned consumer](qa-source.md).
The User explicitly starts/resumes each run; copying the kit or merging its setup
never starts a loop. The User authorized the continuous weekly-gated run recorded in #1187.

Read [the reusable QA Protocol](../tools/repo-qa/references/PROTOCOL.md) for the
quality definition, exploration, prioritization, research, implementation and
handoff procedure. Its [entry point](../tools/repo-qa/SKILL.md) and
[adoption guide](../tools/repo-qa/README.md) work without CPD. This profile contains
CPD-specific facts, authority and commands; do not copy those into another repo.
[Decisions/research](qa-decisions.md) preserve User instructions. The
[Protocol registry](protocols.md) and [glossary](../CONTEXT.md#checks-and-gates)
retain CPD's vocabulary. [Today's record](qa-runs/2026-11-13.md) captures outcomes,
measured allowance, reproducible evidence and continuity lessons.

## Authority and existing Protocols

The reusable Protocol retains the agreed hierarchy: explicit documented/clarified
product intent > applicable community/upstream guidance > local contracts/docs >
code. Every decision is recorded. Unknown intent gets `needs-human-input` and an
exact question in its issue; independent authorized work may continue.

The Bug report Protocol supplies mitigation/root-fix/regression distinctions.
Standalone QA records those in GitHub, not duplicate CPD bug records, and cannot
apply a mitigation to a live host without the separately authorized scope.
The Manager's Merge Protocol does not override QA's explicit User-review-and-merge
policy. Relevant adopted refactor rules still apply to behavior-preserving work;
open [#1114](https://github.com/clayton-duarte/cpd/pull/1114) is a proposal until
merged. Do not silently apply another agent's proposal as adopted policy.

## Scope and priorities

Start with `clayton-duarte/cpd`; trace into `clayton-duarte/dotfiles` only when
CPD evidence places the cause or necessary fix there. File one finding in its
owning repo and link cross-repo dependencies. Scope spans docs, UI, installation,
architecture, storage, sync, dependencies, prompts/extensions, CI and tests.

Initial priorities remain:

1. Installation, upgrades and migration ordering, including interrupted execution,
   compatibility and recovery across branch histories.
2. External sync reaching the client: freshness, completeness, pagination,
   invalidation, retries, rate limits and observable failure.
3. Three independent full CPD instances, one per machine, each owning its state;
   one WebUI selects backends via a dropdown. No shared database or backend-to-
   backend synchronization is requested. Test stale responses, selections,
   subscriptions and pending actions across backend changes.

Machine definitions live in dotfiles' `home/private_dot_ssh/private_config`
(`homeserver`, `mac-pro`, `mac-air`) and `home/.chezmoidata/machines.yaml`. Link
those sources; do not duplicate private addresses. Read-only SSH diagnosis is
authorized in principle but access must be separately planned; it is not
configured by this workflow. Never install, migrate, restart, deploy or modify
live-machine state/config as part of local QA. Local tool installation and
isolated instances with synthetic data/test credentials are authorized. Use an
isolated `CPD_HOME`; never mutate the User's live store.

Simplicity, deletion and appropriate OSS are preferred to custom complexity;
check upstream/community replacements especially for pi extensions. The core is
Rust (`core`, `engine`, `cli`) with SQLite state; pi adapters are TypeScript;
WebUI is React/Vite/Electron. Re-ground these facts and callers on current base.
Historical `docs/research/quality.md` is a lead list, not current proof.

## Budget configuration and commands

[qa.config.json](../qa.config.json) is the canonical executable policy for this
shared CPD/dotfiles/skills group: **no cardinal caps; reserve
`100 * (official resets_at - now) / (10080 * 60 seconds)` percent
of the weekly allowance**, using the authenticated weekly window.
Stop at or below the reserve, or when fresh usage cannot be verified. Unused
allocation carries forward. Allocation releases continuously, not at midnight.
There is no assumed reset weekday or Sunday 8 p.m. release hold; DST does not
change elapsed-time pacing. A new window still requires explicit User run
authorization. This User decision (2026-11-16) supersedes calendar daily pacing,
the fixed 50% floor and former 20/10 count policy. Use full precision for decisions.
Configured PR base remains `main` in all three repos.
CPD remains the product priority; skills changes maintain the shared toolkit.
One serialized creator covers the group. Other agents must not overlap it under
a second config/scope. New agents' grouping/caps need explicit owner agreement;
the skills repo is counted as the explicitly authorized toolkit source. This
migration adds no fresh allowance and performs no budget reset.

Historical attribution `<!-- cpd-repo-qa:v1 -->` and batch markers are unchanged.
`scripts/qa.mjs` remains the CPD entry point and supports `CPD_QA_BATCH`; the
portable entry uses `QA_BATCH`. Prefer one variable and the same value on every
call; conflicting aliases fail before API access. Preserve markers through edits. The generic script also enforces an optional
run descriptor's lower caps, such as the measured run's 10/5, independently from
daily caps. Do not confuse an iteration, run, daily allowance and reset batch.

```sh
node scripts/qa.mjs status
node scripts/qa.mjs --run /path/to/qa-run.json status
node scripts/qa.mjs --run /path/to/qa-run.json issue clayton-duarte/cpd 'Concrete finding' /tmp/qa-issue.md
node scripts/qa.mjs --run /path/to/qa-run.json pr clayton-duarte/cpd qa/123-fix 'Concrete change' /tmp/qa-pr.md
```

Run descriptors contain authorization/start/limits, not counters. Before a new
bounded run, recover/create its exact JSON using the kit's template, record it
in an existing QA issue comment (setup anchor [#1115](https://github.com/clayton-duarte/cpd/issues/1115)
is available), and preserve its permalink in the handoff. Resume that descriptor
without changing ID/start or raising caps without User authorization. `/tmp` is
an expendable local copy; the canonical record must survive the session.
A User-authorized reset uses `QA_BATCH=YYYY-MM-DD-reset-N` (or legacy
`CPD_QA_BATCH`) matching Toronto today. Never autonomously reset or remove markers. The current run has null cardinal caps, fresh live usage (maximum60 seconds), and no reset batch.
Status reports full-day, active daily/batch and optional run counts from GitHub.
The [pinned canonical kit README](../tools/repo-qa/README.md) defines fields, exit codes and limits.

## Grounding and validation

Use Node >=22.19, authenticated `gh` and the project's pinned versions in
`package.json`/lockfiles and toolchain definitions. Do not retain another session's
absolute tool/worktree paths as prerequisites. Follow [scoped implementation](implementing.md#the-scoped-loop)
and `scripts/test-changed`. Run applicable lint/fmt before commit and tests for
changed behavior/callers. No JS/TS formatter/linter is currently configured for
QA tooling; syntax/whitespace are not semantic lint. CI runs the complete gate.

The local `tools/repo-qa` directory is a vendored consumer of that source, not
a second editable implementation. Its source commit and per-file hashes are in
`tools/repo-qa.upstream.json`; tests refuse drift. See [source ownership/update
rules](qa-source.md). Normal CPD checkout/CI needs no cross-repo fetch or credentials.

For this dependency-free kit:

```sh
node --check scripts/qa.mjs
node --check tools/repo-qa/scripts/qa.mjs
node --test scripts/qa.test.mjs
sh scripts/check-portable.sh
git diff --check
```

The cold-start test uses temporary files, child Node processes and fake GitHub,
with no real issue/PR or network service. If the sandbox blocks process creation,
run in an authorized environment and report the constraint; do not skip silently.
Unexpected broader failures must be compared on unchanged base. Today's full
local Node suite had five failures reproduced on main, while affected suites and
all five PR CI gates passed; see the run record rather than treating those failures
as caused by each fix.

## Fresh-session entry

Read the kit's SKILL.md, this profile/config, applicable repo instructions,
decisions and the latest canonical handoff. Recheck every open attributed QA PR for mergeability, CI and review feedback and the
same run's GitHub counts, verify current base and clean worktree ownership, then
continue only the User-started run. Issues retain research and self-contained
reproducers; PRs retain implementation/validation. User alone reviews and merges.
No CPD adoption, scheduler, live access or agent spawning is enabled.


## Recorded continuous run (2026-11-13; historical pacing)

The User explicitly lifted cardinal limits and authorized continuous QA until the
weekly allowance is under50%; exactly50% permitted continuation under that
historical rule, superseded by the 2026-11-16 daily pacing decision. Source research,
decisions and immutable run descriptor are recorded in
[#1187](https://github.com/clayton-duarte/cpd/issues/1187#issuecomment-6860759090).
Recover the recorded descriptor and `QA_USAGE_FILE` for an authorized resumption;
an expired descriptor cannot authorize the current weekly window.
Reuse the explicitly authorized private CLI credential cache and obtain a fresh
one-shot reading at each iteration/significant stage, before/after lengthy tests,
and before every write. Process loss alone must not require a new login.
Missing/stale/invalid telemetry pauses all further work. Never consume reset
credits, infer usage from issue counts, automatically restart after reset, or
claim an inactive cloud session can run unattended. The final model response and
other consumers can change the sampled percentage; this is no cost reservation.
The User still reviews and merges every PR. No live host writes are authorized.


## Reusable allowance authentication

The original memory-only reader lost its login when its process ended. The User
rejected repeated sign-ins. [Source PR3](https://github.com/clayton-duarte/skills/pull/3)
adds an explicit private credential directory outside Git and one-shot reads.
Use the kit README's persistent-auth commands with `--min-remaining 0` for
reset pacing; ordinary checks omit `--login`. The reader publishes telemetry
and the guard computes the changing floor.
The official CLI owns refresh. Never overwrite the cloud-managed login or commit
credentials. Initial device authorization is only needed when the private cache
has no usable login; revocation or loss of that volume can require authorization.
This authenticates allowance reads and does not start a new QA run or reset limits.
The live bootstrap and separate-process reuse both succeeded on2026-11-13. A fresh
reading was66% remaining; that historical number is not a gate for later work.
Source PR3 supplied persistent authentication; User-merged PR6 added proactive
PR upkeep. Skills #8 supplied the previous calendar policy. The current
reset-pacing pin and its satisfied source-review dependency are recorded in
[qa-source.md](qa-source.md); CPD adoption still requires User review and merge.


## Proactive PR upkeep

The User explicitly requires QA to address conflicts without being prompted
(2026-11-13). At iteration start and after observed base/head changes, inspect
all open QA-owned PRs, including earlier runs. Resolve conflicts on the existing
branch, preserve both intended changes, run scoped checks and push the result
before new discovery. Pending mergeability is unconfirmed and needs another
read. Recheck after pushing; a mergeable flag does not replace CI or review.
The User still reviews and merges every PR.

The portable implementation merged in
[skills #6](https://github.com/clayton-duarte/skills/pull/6), tracked by
[skills #5](https://github.com/clayton-duarte/skills/issues/5). The reviewed
source pin includes `pr-status`; use it for the mergeability gate and inspect
CI/review feedback separately. When the CLI connection is unavailable, native
GitHub API inspection supplies the same read-only inputs. The standing User
policy applies independently of a consumer update.

Weekly creation requires the same immutable `--run` descriptor. Its recorded start must belong to the current official allowance window; a fresh CLI process or reusable login cannot renew an old run after reset. Descriptor-free status is read-only inspection with `run_required:true` and creation disabled. User-merged skills#7 enforces this across sessions.


## 2026-11-16 pacing and interpretation

The formula is a cumulative remaining reserve, not a strict daily spending cap.
Halfway through the actual weekly window the reserve is 50%. The official
reset ends the old week's balance; no hardcoded weekday or release hour changes
that boundary. Current usage by other sessions reduces headroom too. Status
reports the computed `min_remaining_percent` and `spendable_percent`.

The static reader threshold must be zero for this mode. Missing or stale data,
account unavailability, spend controls, invalid/expired telemetry and expired
run authority still refuse work. No new loop, scheduler, login, deployment or
automatic weekly authorization is started by this policy update.

Previous calendar source: [skills #8](https://github.com/clayton-duarte/skills/pull/8).
