# CPD QA Protocol profile

The QA Protocol was adopted through [#1116](https://github.com/clayton-duarte/cpd/pull/1116)
and its permanent 20/10 limits through [#1132](https://github.com/clayton-duarte/cpd/pull/1132).
The portable implementation is proposed in [#1148](https://github.com/clayton-duarte/cpd/issues/1148).
The User explicitly starts/resumes each run; copying the kit or merging its setup
never starts a loop. The measured pilot is paused for distillation.

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
shared CPD/dotfiles group: **20 new issues and 10 new PRs per America/Toronto day**,
or explicitly User-reset batch. Configured PR base is `main` in both repos.
One serialized creator covers the group. Other agents must not overlap it under
a second config/scope. New agents' grouping/caps need explicit owner agreement;
this extraction does not expand CPD's scope or reset its budget.

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
`CPD_QA_BATCH`) matching Toronto today. Never autonomously reset or remove markers.
Status reports full-day, active daily/batch and optional run counts from GitHub.
The [kit README](../tools/repo-qa/README.md) defines fields, exit codes and limits.

## Grounding and validation

Use Node >=22.19, authenticated `gh` and the project's pinned versions in
`package.json`/lockfiles and toolchain definitions. Do not retain another session's
absolute tool/worktree paths as prerequisites. Follow [scoped implementation](implementing.md#the-scoped-loop)
and `scripts/test-changed`. Run applicable lint/fmt before commit and tests for
changed behavior/callers. No JS/TS formatter/linter is currently configured for
QA tooling; syntax/whitespace are not semantic lint. CI runs the complete gate.

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
decisions and the latest canonical handoff. Recheck open PR feedback/CI and the
same run's GitHub counts, verify current base and clean worktree ownership, then
continue only the User-started run. Issues retain research and self-contained
reproducers; PRs retain implementation/validation. User alone reviews and merges.
No CPD adoption, scheduler, new loop, live access or agent spawning is enabled.
