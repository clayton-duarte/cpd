# Unplanned work: decided in `building-cpd`, neither on `main` nor in a live Plan

This is Job `gap-audit` of Plan `cross-repo`, PR `gaps`. Per hs-d192, its method is the distiller's rule and its result is the distiller's test fixture. It changes no code.

## Method

- **Snapshot (2026-10-18, 05:53Z):**
  - **Records:** `cpd context show --context building-cpd` returned 357 records, up to hs-d198: 197 decisions, 126 notes, 23 Plan records, 5 questions, 4 answers.
  - **Plans:** `cpd plan list`, then `cpd plan show` for every Plan not `done`. I also read the done Plans whose records name a follow-up.
  - **cpd code:** `origin/main` at `d10a1ee` (#28). Line citations are at `d10a1ee`.
  - **dotfiles:** `origin/main` at `cac18c9` (#93).
  - **Records moved during the audit.** In the minutes the audit ran, #28, #31 and #32 merged, Plan quality appended its batch 1, and eight records were added. Everything above was read again at 05:53Z. Records written after that are not in this file.
- **Superseded records:** a record named in another's `supersedes` asks for nothing (17 pairs). The same holds for a record whose replacement says in its text that it replaces it (hs-d99 by hs-d100, for example). The evidence column names the replacement.
- **Classes:**
  - **BUILT** cites `file:line` on `origin/main` or the PR number. dotfiles PRs are written "dotfiles #n".
  - **OWNED** cites `<plan>/<job>` or `<plan>/<pr>` of a live Plan. Two cases count as OWNED and are marked as such:
    - a Job merged on the Lead Branch of a live Plan's PR that is not yet on `main` (for example manager's #30, or tui-dots `followups`): the work is held, although the Spec lists only queued, running and draft Jobs;
    - a record that names a live Plan as owner with no Job yet. The evidence says **record only, no Job**, and the section [Owned by a record only](#owned-by-a-record-only-no-job-yet) lists these.
  - **MISSING** means neither of the above. When I was unsure, I classed it MISSING and said "unsure".
  - **NO WORK** is a fact, a process rule, routing, an answered question, or a superseded record.
- Plan records (`hs-p…`) are classed by their Plan's state.

Counts: BUILT 140, OWNED 77 (10 of them wholly or partly on a record only: hs-n112, hs-n123, hs-n126, hs-n127, hs-d182, hs-d186, hs-d188, hs-d192, hs-d193, hs-d194), MISSING 18, NO WORK 122.

## MISSING

The groups are the Spec's areas. I added one group, **Epics**, for work that belongs to no single area. Tiers follow the skill. "After X" means the work touches files that Plan X has in flight.

### engine

**E1. An archived or cancelled Plan keeps its plannotator port, and no command cancels a Plan** (hs-d62, hs-d61; teardown G5/D1)
- **What hs-d62 asks:** a cancelled or archived Plan frees its port, and its teardown happens at archive.
  - The teardown is built: #9 closes the panes at archive, and #22 removes the worktrees at PR merge (hs-d142 replaced the timing).
  - The port is not freed. `core/src/session/port.rs:47` counts every Plan whose phase is not `done` as a holder, archived or not.
- **Effect today:** the whole range 19432–19441 is held by Plans that are not done, so the next `plan new` is refused with `no_port` (hs-d50). Two of the holders are Plans archived in grounding: `reopen-done` holds 19434 and `pr-view-sync` holds 19433. I read this from `~/.cpd/run/*/plannotator-port`.
- **What hs-d61 asks:** "Cancelling a Plan cancels its unfinished Jobs". No command does it:
  - `docs/commands.md` §4 has no `plan cancel`;
  - teardown.md's D1 (`plan abandon`) was never decided;
  - hs-d142 puts abandoned Plans out of scope, but only for the cleanup.
- Proposed Job: *An archived Plan frees its plannotator port*. Tier S3. Files: `core/src/session/port.rs`, `docs/contract.md` §6.5, `cli/tests/plan.rs`.
- Proposed Job: *Contract: cancelling a Plan (what it cancels, closes and frees)*. Tier O3. It needs a User decision first. Files: `docs/contract.md` §2.2 and §3, `docs/commands.md` §4.

### store

**S1. Named migrations and the schema compatibility window** (hs-a4, hs-d178)
The User approved `docs/research/migrations.md`, now on `main` through #28. hs-d178 says the code is "Built as PR migrations of Plan dev-speed after manager's PR merges". dev-speed is now `done`: #28 held only the design and the protocol check, and no PR `migrations` was added. hs-n100 also lists both items as not built. Plan dev-speed could take it as a new PR (hs-d87).
- Proposed Job: *Migrations applied by name; the legacy 0001–0010 recorded*. Tier O3, after manager. Files: `core/src/store/mod.rs`, `core/src/store/schema.rs`, `core/src/store/migrations/`, `cli/src/cli.rs` (`--version`), `scripts/install-cpd.sh`, `docs/contract.md` §4.5, `cli/tests/store.rs`.

**S2. A migration commits without checking its own schema hash** (hs-n121, quality store-W2, item 4b)
hs-n121 proposes this item for Plan manager, but manager's only queued Job is `gate-session`. hs-n127, the list of quality's batch 2, does not name item 4b either.
- Proposed Job: *Verify the schema hash before a migration commits (`migration_failed`)*. Tier S3, after manager. Files: `core/src/store/mod.rs:202-215`, `docs/contract.md` §4.5, `cli/tests/store.rs`.

### git/merge

**G1. The merged-PR cleanup can remove a Lead worktree under a running Gate** (hs-n124)
hs-n124 reports what happened: the User merged #31 while `cpd pr push --pr drafts` ran its Gate in that Lead worktree. #22's cleanup removed the worktree mid-Gate, and the push failed `gate_failed`. I did not reproduce it. The note's fix: the cleanup takes the per-repo lock that `pr push` and `pr open` hold, or skips a worktree whose Gate is running. No Plan holds it.
- Proposed Job: *The merged-PR cleanup takes the per-repo lock*. Tier S3. Files: `core/src/pr/poll.rs`, `docs/contract.md` §7.2, `cli/tests/pr_cleanup.rs`.

### PRs

None found. The tamper rule for base merges is OWNED (cross-repo/gaps), and so are PR conflicts (pr-conflicts) and the Inbox's PR kinds (inbox/github).

### adapter

None found. The Manager's digest reading the Inbox (hs-d186) and unknown Event kinds (pi-ci-W1) are owned by a record only (see [below](#owned-by-a-record-only-no-job-yet)).

### skill

**K1. Dialogs short, tables in chat** (hs-n48)
The legend half of hs-n48 is built (#25). The rule "keep dialogs short and put tables in chat" is in no skill text. `pi/skills/cpd-lead/references/questions.md:7` caps a preview at 20 rows, but it dates from 2026-10-13, before the incident, and it does not send tables to chat.
- Proposed Job: *cpd-lead: a dialog holds one short question; tables and long text go in chat*. Tier S1. Files: `pi/skills/cpd-lead/references/questions.md`, `pi/skills/cpd-lead/SKILL.md`, `pi/skills/skill.test.ts`.

**K2. Spec rules for research Jobs and for commands that never exit** (hs-n97; the second half of hs-n107)
The lessons:
- commit a skeleton within 10 min;
- wrap measurements in `timeout`;
- no synthetic benchmarks;
- ban by name the commands that never exit: `cpd events follow`, `cpd watch`, `cpd tui`, `cpd session run`.

Neither file has them. `references/spec-files.md:58-64` (Research Jobs) has none of the lessons. `core/src/spec/worker.md:33` says to stop processes, but does not forbid starting one that never ends. The enforcement is quality/stall (OWNED), and this rule is its prose half.
- Proposed Job: *Worker Spec and research-Job rules for bounded commands*. Tier S2. Files: `core/src/spec/worker.md`, `docs/commands.md` §16.1, `pi/skills/cpd-lead/references/spec-files.md`, `cli/tests/spec.rs`.

### cockpit

**C1. Views may stay stale after a PR merges** (hs-n86; unsure)
This is the User's bug report.
- Plan pr-view-sync was archived in grounding with no Job and no finding.
- hs-d159 says tui-visuals' pending work (#20) fixed it; I did not verify that.
- The DB was right at the time (hs-n86, hs-n90).

I class it MISSING only because nothing shows the fix.
- Proposed Job: *Research: reproduce a PR merge while the Plan view and the Deploy column are up*. Tier S2. Files: `docs/research/pr-view-sync.md`.

### docs

None found. Docs drift is quality's batch 2, item 13 (owned by a record only).

### ops

**O1. Drop dotfiles' global `PLANNOTATOR_PORT` range export** (hs-d40, hs-d48)
hs-d48 says the export "is dropped by a later dotfiles change once Plan hardening's per-Plan ports are installed on hs". The per-Plan ports have been on `main` since #8. dotfiles still exports `PLANNOTATOR_PORT=19432-19441` (`home/dot_config/zsh/modules/cpd.zsh:22`), and its test pins the line (`tests/test_cpd_install.py:71`). Unverified: whether anything without a Plan port, such as a hand-run pi, still relies on it. manager's `gate-manager-env` already keeps a Manager from inheriting it.
- Proposed Job (dotfiles): *Drop the PLANNOTATOR_PORT export and keep the Macs' forwards*. Tier S1. Files: `home/dot_config/zsh/modules/cpd.zsh`, `tests/test_cpd_install.py`.

**O2. sccache has not yet sped up CI** (hs-n122, quality pi-ci-F2, item 9b)
A warm run takes 339 s, against 307 s before sccache. The record calls it "dev-speed's acceptance", but dev-speed is now `done` without a Job for it, and neither of quality's batches names item 9b.
- Proposed Job: *Measure warm CI runs; restore the target cache or drop sccache on CI*. Tier S2. Files: `.github/workflows/ci.yml`, plus a research file.

**O3. Machine state nobody has checked** (hs-n93, hs-n25, hs-n114; unsure)
- pro's pi 1.3.0 and cpd rollout was never verified, because pro was unreachable.
- cpd 1 leftovers on air and pro were left untouched: `~/.cpd/config.json`, herdr servers, `cpd.cpd-v0*` worktrees. `prune-worktrees.sh` ran on hs only (hs-n98).
- Three Lead panes running the old adapter needed a `/reload` by the User.

None of these is clearly asked as work, and each needs ssh to the User's machines or the User's hand.
- Proposed step (not a Worker Job): *a Lead and the User run an ops checklist: pro and air at `main`, pi 1.3.0, and the cpd 1 leftovers listed for the User*.

### Epics

**X1. Epic B, auxiliary agents** (hs-n95, hs-d163)
It is second in hs-d163's order. No Plan exists. Its pilot, Context distillation, is owned by a record only (cross-repo PR `distill`, hs-d194).
- Proposed: *a Plan of its own, started with the User with an Epic discussion first*. It is not a Job.

**X2. Epic C, architecture review** (hs-n96, hs-d163)
It is third in the order, and no Plan exists. Some of it is already done or held elsewhere: hs-a4's guards (#21, #28) and quality's batch 2.
- Proposed: *a Plan of its own after Epic B*. It is not a Job.

## Owned by a record only (no Job yet)

These pass as OWNED under the Spec's rule ("a record explicitly names a live Plan as owner"), but no Job carries them yet. The named Lead has to append them, or they will drop out.

| Work | Records | Owner named | Proposed Job (tier; files) |
|---|---|---|---|
| Context distillation: digest files, engine rebuild, `cpd context distill`, `--digest`, `plan new`'s first message | hs-d182 (2), hs-d192, hs-d194 | cross-repo, PR `distill` after `gaps` | *Contract: the distilled Context* (O3; `docs/contract.md`, `docs/commands.md` §3, `CONTEXT.md`), then *distill and the engine rebuild* (O3, after manager; `core/src/context/`, `core/src/watch/`, `core/src/plan/new.rs`, `cli/tests/context.rs`) |
| The Manager's digest reads the Inbox | hs-d186 | manager, a follow-up once inbox/core lands | *Manager digest lists open Inbox items* (S3; `pi/lib/updates.ts`, `pi/lib/adapter.ts`, `docs/commands.md` §15, `pi/test/updates.test.ts`) |
| The Deploy row flags `mergeable = 'conflicting'` (pr-links `deploy-rows` covers only the ↗ link and the click) | hs-d188 (2), hs-d190 | pr-links | *Deploy row flags a conflicting PR* (S3, after pr-conflicts/store; `tui/src/read/widgets.rs`, `tui/src/widgets/column.rs`, `docs/commands.md` §13.2/§17, `cli/tests/tui_widgets.rs`) |
| quality batch 2: items 2, 3, 5–8, 10, 11, 13, including pi-ci-W1 (unknown Event kinds) and pi-ci-W3 (pi test flakes) | hs-d193, hs-n127, hs-n121, hs-n122, hs-n112 | quality, after pi-pin `ladder`, manager, tui-visuals and tui-dots | one PR per item, as `docs/research/quality.md` "Proposed work" lists them |
| The flaky tests `e2_kills_the_window_of_a_dead_parked_lead` (`cli/tests/session.rs:1076`, CI) and `open_and_park_keep_at_most_one_placeholder_only_in_the_slot` (`:535`, under load) | hs-n123, hs-n126, hs-n112 | pi-pin as "flaky-test owner" (hs-n126; hs-n112 says only "another Lead") | *Fix at its root …* (S3 each; `cli/tests/session.rs`, perhaps `core/src/session/moves.rs`); hs-n126 suggests tmux 3.4's lost SIGCHLD, which pi-pin/tmux may cure |

## Could not decide, and clashes found on the way

- **Two designs for draft Jobs.**
  - #31 (tui-visuals, hs-d179) is now on `main`: a Plan review approves drafts in any phase.
  - pi-pin's PR `drafts` (hs-d175, owner by hs-d176) is still live, with merged Jobs and a running `sync-main-drafts`. It builds `job add --draft` with `job promote` instead.

  Both are User decisions, six minutes apart (04:16Z and 04:22Z). Which one stands is the User's call. Until it is made, pi-pin's sync Job will fight #31's code.
- **Two Plans claim the same columns.** hs-d190 gives `pull_requests.mergeable` and `merge_state` to pr-conflicts, and says inbox's `gh-poll` "does not add mergeable". inbox `gh-poll`'s goal still reads "review requests and mergeability (migration 0012)", and its acceptance requires storing `mergeable`.
- **Shared ports.** cross-repo and manager both hold 19432, and tui-visuals and pr-view-sync both hold 19433, all four not done (`~/.cpd/run/*/plannotator-port`). I did not verify the cause: perhaps the files predate #8, or perhaps a Plan keeps an existing file with no holder check. It is outside this Job. It bears on E1.
- **hs-n86** (stale views) is MISSING (C1) only because nothing shows the fix. hs-d159 claims #20 fixed it.
- **hs-n65** (`plan phase` once returned `plan_not_found`) is classed BUILT on the guess that its cause is the inner join #17 fixed. The cause was never verified.
- **hs-d61 against hs-d67.** hs-d61 says that archiving a Lead Session cancels its Plan's Jobs that have not started. The later hs-d67 refuses with `archive_blocked` while Jobs are queued or running. I took hs-d67 as the rule in force, so only hs-d61's "cancel a Plan" is left (E1).
- **Not decided work:** the five User decisions hs-n127 lists as open (sql-F2, engine-W9, store-W7, pi-ci-W10, `-D warnings`). They are not listed as MISSING.
- **The skill and the configured timeout disagree.** `pi/skills/cpd-lead/SKILL.md:67` sizes Jobs at about 13 min under a "30 min" timeout, while hs-d11 and hs's config give 90 min. No record asks for this fix, so it is noted only here.
- **Fast-moving records:** hs-d192 to hs-d198 and hs-n121 to hs-n127 were written between 05:31Z and 05:52Z, while this audit ran. Their classes are the most likely to be out of date when this is read.

## Every record

The table has one line per record, in Context order. Superseded records are always NO WORK, which was checked when the table was built. The table covers each of the 357 records exactly once.

| record | kind | class | evidence |
|---|---|---|---|
| hs-p1 | plan | BUILT | Plan cockpit-statusbar done (PR #4 into cpd/cpd2, carried to main by #7) |
| hs-n1 | note | NO WORK | fact (pre-switch-over layout) |
| hs-d1 | decision | BUILT | tui/src/statusbar/ (moves to the status line: hs-d149, OWNED) |
| hs-d2 | decision | BUILT | tui/src/statusbar/layout.rs:1-2 |
| hs-n2 | note | NO WORK | items explicitly left out (assumptions) |
| hs-p2 | plan | BUILT | Plan cockpit-parity done (PR #5 into cpd/cpd2, carried by #7) |
| hs-n3 | note | NO WORK | fact |
| hs-n4 | note | NO WORK | process for cockpit-parity (done) |
| hs-n5 | note | BUILT | core/src/watch/report.rs:6, core/src/config/file.rs:234 (error_repeat_seconds 600) |
| hs-d3 | decision | NO WORK | architecture stands (confirmed by hs-a4) |
| hs-d4 | decision | NO WORK | process rule |
| hs-d5 | decision | BUILT | .config/wt.toml pre-commit / pre-merge |
| hs-d6 | decision | BUILT | pi/skills/cpd-lead/SKILL.md:22, :173-181 (review Job) |
| hs-d7 | decision | BUILT | pi/skills/cpd-lead/SKILL.md:34-45 |
| hs-d8 | decision | BUILT | cpd plan review / cpd_plan_review (commands.md §4.6, §15.5) |
| hs-d9 | decision | NO WORK | process rule |
| hs-d10 | decision | BUILT | #7 (pi/) |
| hs-d11 | decision | BUILT | config key timeouts.attempt_minutes (core/src/config/file.rs:222); 90 seeded by dotfiles home/dot_cpd/create_config.toml.tmpl:12 |
| hs-d12 | decision | NO WORK | superseded by hs-d28 |
| hs-d13 | decision | BUILT | #5 (into cpd/cpd2) via #7; parity later waived (hs-d104) |
| hs-d14 | decision | NO WORK | obsolete after the cut-over (hs-d21) |
| hs-n6 | note | NO WORK | handoff state |
| hs-n7 | note | NO WORK | handoff list; every item since built (#7, #11, #8) or overturned (store remote: hs-a1) |
| hs-n8 | note | NO WORK | procedures |
| hs-d15 | decision | NO WORK | superseded by hs-d16 |
| hs-d16 | decision | NO WORK | process (pre-switch-over sync) |
| hs-n9 | note | BUILT | #8: session run re-execs the installed cpd (hs-d55), core/src/session/run.rs |
| hs-d17 | decision | BUILT | #7 |
| hs-d18 | decision | BUILT | #7 (core/, cli/, tui/) |
| hs-d19 | decision | BUILT | #7 (scripts/install-cpd.sh) |
| hs-d20 | decision | BUILT | #7 |
| hs-d21 | decision | NO WORK | one-time ops, done (hs-n14, hs-n25) |
| hs-p3 | plan | BUILT | Plan switch-over: #7 |
| hs-d22 | decision | BUILT | #7 |
| hs-n10 | note | NO WORK | hazard, settled (hs-n15) |
| hs-p4 | plan | BUILT | Plan machine-sync done: dotfiles #87 |
| hs-d23 | decision | OWNED | manager ctx-contract (cpd/manager/cpd edits docs/adr/0012-context-store.md), PR #30 open |
| hs-q1 | question | NO WORK | answered by hs-a1 |
| hs-d24 | decision | NO WORK | fact |
| hs-d25 | decision | NO WORK | superseded by hs-d146 |
| hs-d26 | decision | BUILT | dotfiles #87: home/dot_cpd/create_config.toml.tmpl |
| hs-n11 | note | NO WORK | hazard, settled (hs-n15) |
| hs-n12 | note | BUILT | #8 'fake programs record each call atomically' (50/50 tui_logs runs) |
| hs-n13 | note | NO WORK | fact |
| hs-d27 | decision | OWNED | quality/nogh (queued): tests cannot reach the real gh (tests-build-W1) |
| hs-n14 | note | NO WORK | fact (cut-over done) |
| hs-n15 | note | NO WORK | fact |
| hs-n16 | note | BUILT | dotfiles #88: home/dot_config/systemd/user/dotfiles-sync.service Environment=PATH |
| hs-d28 | decision | BUILT | #11 |
| hs-d29 | decision | BUILT | #11 |
| hs-d30 | decision | BUILT | #11 |
| hs-q2 | question | NO WORK | answered by hs-a2 |
| hs-p5 | plan | BUILT | Plan cross-repo: #11, #27; PR gaps live (this audit, tamper-*) |
| hs-n17 | note | NO WORK | fact |
| hs-d31 | decision | NO WORK | policy; open flakes classed under hs-n112, hs-n123 |
| hs-d32 | decision | BUILT | dotfiles #88 (hs-n19, hs-n20, hs-n25) |
| hs-d33 | decision | BUILT | #8 (driver/session run re-exec), #21 (guards), #27 (follower, adapter reload) |
| hs-a1 | answer | OWNED | manager ctx-contract, ctx-db (migration 0007; context sync/E6 removed), PR #30 open |
| hs-d34 | decision | NO WORK | policy (teardown items classed under hs-d51, hs-n27) |
| hs-d35 | decision | BUILT | #8: core/src/session/port.rs |
| hs-a2 | answer | BUILT | #11 |
| hs-d36 | decision | BUILT | #11 |
| hs-d37 | decision | NO WORK | process (batching) |
| hs-d38 | decision | BUILT | #8; adapter reload #27 |
| hs-d39 | decision | NO WORK | policy |
| hs-d40 | decision | MISSING | cpd range BUILT #8 (port.rs), forward BUILT dotfiles #88; the global PLANNOTATOR_PORT export is still in dotfiles home/dot_config/zsh/modules/cpd.zsh:22 |
| hs-d41 | decision | NO WORK | routing |
| hs-p6 | plan | BUILT | Plan hardening: #8 |
| hs-p7 | plan | BUILT | Plan dotfiles-sync: dotfiles #88 |
| hs-n18 | note | NO WORK | fact |
| hs-n19 | note | BUILT | dotfiles #88 (hs-d47): scripts/dotfiles-sync.sh:235-255 plain git pull, tail -5 |
| hs-d42 | decision | BUILT | #11 |
| hs-d43 | decision | BUILT | #11 |
| hs-d44 | decision | BUILT | #11 (skill: pi/skills/cpd-lead/SKILL.md:117 closed unmerged) |
| hs-d45 | decision | BUILT | #11 |
| hs-n20 | note | NO WORK | fact |
| hs-d46 | decision | BUILT | dotfiles #88: dotfiles-sync.service Environment=PATH |
| hs-d47 | decision | BUILT | dotfiles #88: scripts/dotfiles-sync.sh:235-255 |
| hs-d48 | decision | MISSING | interim export BUILT dotfiles #88; the drop it promises is not done (cpd.zsh:22) though per-Plan ports are on hs (#8) |
| hs-d49 | decision | BUILT | #8 |
| hs-d50 | decision | BUILT | #8: core/src/error.rs:142, docs/commands.md:1424 |
| hs-d51 | decision | BUILT | #8: docs/research/teardown.md; fixes G1, G2, G3, G8, G10, G12 on main (events/mod.rs:208, liveness.rs:10, worker.md:33, exec.rs:140, common/mod.rs:30, ask.rs:82, driver/worker.rs:334) |
| hs-n21 | note | BUILT | #8: core/src/spec/worker.md:33 teardown rule (teardown G3) |
| hs-n22 | note | NO WORK | fact |
| hs-d52 | decision | BUILT | #9 |
| hs-d53 | decision | BUILT | #9 (dotted lines revised by hs-n38, #17) |
| hs-d54 | decision | BUILT | #9 (commands.md §4.10 archive_blocked) |
| hs-p8 | plan | BUILT | Plan tree-menu: #9, #17 |
| hs-n23 | note | NO WORK | ownership note |
| hs-d55 | decision | BUILT | #8 |
| hs-d56 | decision | NO WORK | superseded by hs-d58 |
| hs-d57 | decision | NO WORK | superseded by hs-d59 |
| hs-d58 | decision | NO WORK | withdrawal |
| hs-d59 | decision | NO WORK | withdrawal / routing |
| hs-n24 | note | BUILT | #25: pi/lib/adapter.ts:550 ui_prompt_start/end = blocked 'dialog' |
| hs-n25 | note | MISSING | ops: cpd 1 leftovers on air and pro (config.json, herdr servers, cpd.cpd-v0* worktrees) never cleaned; unsure it is wanted |
| hs-n26 | note | NO WORK | resolved by hs-n28 |
| hs-d60 | decision | BUILT | dialog part #25; question-ends-turn part replaced by hs-d73/hs-d100 |
| hs-d61 | decision | MISSING | no command cancels a Plan (commands.md §4 has no plan cancel); archive refuses live Jobs instead (hs-d67) |
| hs-d62 | decision | MISSING | archive closes panes (#9), worktrees at PR merge (#22, hs-d142); a cancelled or archived Plan still holds its port (core/src/session/port.rs:47 frees only at done) |
| hs-d63 | decision | BUILT | BACKLOG.md:5 |
| hs-n27 | note | OWNED | G4 BUILT by #22 (per-PR cleanup, best effort, retried); G9/G11/G13 merged on tui-dots/followups (feedback-file, teardown-rust-2), PR not yet open |
| hs-n28 | note | NO WORK | fact |
| hs-d64 | decision | NO WORK | superseded by hs-d71 |
| hs-d65 | decision | BUILT | #16 |
| hs-d66 | decision | BUILT | #9 (titles, rename commands); display rule superseded by hs-d101 |
| hs-d67 | decision | BUILT | #9 (commands.md §4.10, §10.10), revised by hs-d90 (#17) |
| hs-d68 | decision | NO WORK | process |
| hs-d69 | decision | BUILT | #9; tree now calls plan new without --repo (no --repo in tui/src/tree/actions.rs) |
| hs-n29 | note | NO WORK | coordination |
| hs-d70 | decision | BUILT | #11 (fork-siblings, review-job) |
| hs-n30 | note | NO WORK | fact; the unverified point verified by hs-n17 |
| hs-d71 | decision | BUILT | #10: .github/workflows/ci.yml |
| hs-d72 | decision | BUILT | #16 |
| hs-p9 | plan | BUILT | Plan pr-poller: #16 (reland of #13) |
| hs-p10 | plan | BUILT | Plan cpd-ci: #10 |
| hs-d73 | decision | NO WORK | superseded by hs-d100 |
| hs-n31 | note | NO WORK | draft, decided as hs-d75 |
| hs-d74 | decision | BUILT | #10 |
| hs-d75 | decision | OWNED | manager flow-docs, skills-flow (PR #30 open) |
| hs-d76 | decision | OWNED | column title BUILT #20; phase pr -> deploy = manager deploy-contract/deploy-code (PR #30 open) |
| hs-d77 | decision | NO WORK | superseded by hs-d171 |
| hs-d78 | decision | OWNED | manager adapter-updates (1) and overlap (2), PR #30 open |
| hs-n32 | note | BUILT | #25 (pi/lib/adapter.ts:550) |
| hs-d79 | decision | OWNED | manager b2-contract, adapter-updates (PR #30 open) |
| hs-d80 | decision | NO WORK | superseded by hs-d81 |
| hs-d81 | decision | NO WORK | routing |
| hs-d82 | decision | BUILT | #17: core/src/session/archive.rs:178, core/src/session/run.rs:138 |
| hs-n33 | note | NO WORK | resolved by hs-n43 |
| hs-d83 | decision | OWNED | manager b3-* (PR #30 open) |
| hs-d84 | decision | OWNED | manager (records in the DB, PR #30 open) |
| hs-d85 | decision | OWNED | manager (PR #30 open) |
| hs-d86 | decision | OWNED | manager adapter-manager digest (PR #30 open) |
| hs-n34 | note | NO WORK | fact |
| hs-d87 | decision | BUILT | #12 |
| hs-d88 | decision | NO WORK | superseded by hs-d117 |
| hs-d89 | decision | NO WORK | process (cross-repo integration) |
| hs-n35 | note | OWNED | same as hs-n27 |
| hs-n36 | note | BUILT | #25 (spinner), settled by hs-n40 |
| hs-n37 | note | BUILT | #25 dot read model (tui/src/read/dot.rs); own-state-first fix OWNED tui-dots/followups dot-fix (hs-d183) |
| hs-n38 | note | BUILT | #17 'archived rows at live depth, normal grey guides, no blank row' |
| hs-n39 | note | BUILT | #17 (same commit) |
| hs-d90 | decision | BUILT | #17 'Archive without y/N, chat-slot two-step archive, Rename… on archived rows, click … history' |
| hs-n40 | note | BUILT | #25 (tree and pills spin) |
| hs-n41 | note | BUILT | #25 (dialog = blocked; dot rule) |
| hs-d91 | decision | BUILT | #17 (session history, auto-archive on quit) |
| hs-n42 | note | BUILT | #25 |
| hs-n43 | note | NO WORK | fact |
| hs-d92 | decision | OWNED | manager (session send, Manager role, skill), PR #30 open |
| hs-d93 | decision | NO WORK | ordering |
| hs-d94 | decision | NO WORK | owners superseded by hs-d111 |
| hs-n44 | note | BUILT | replaced by hs-n47, built in #25; kinds in #11 adapter-kinds |
| hs-d95 | decision | NO WORK | superseded by hs-d99 |
| hs-d96 | decision | BUILT | #16 |
| hs-d97 | decision | BUILT | #16 |
| hs-d98 | decision | BUILT | #16 'the Dispatch PRs section lists the open pull requests, oldest first' |
| hs-n45 | note | NO WORK | coordination |
| hs-n46 | note | BUILT | #16: docs/research/gh-interface.md |
| hs-d99 | decision | NO WORK | superseded by hs-d100 (its text says so) |
| hs-n47 | note | BUILT | #25 |
| hs-n48 | note | MISSING | legend BUILT #25 (tui/src/widgets/legend.rs); the rule 'keep dialogs short, tables in chat' is in no skill text (references/questions.md:7 predates it, 2026-10-13) |
| hs-d100 | decision | BUILT | #25; refinements hs-d183/hs-d184 OWNED tui-dots/followups |
| hs-d101 | decision | BUILT | #26 tree-ids |
| hs-d102 | decision | BUILT | #20 (no tabs), #26 (Copy id: tui/src/tree/actions.rs:7; @cpd_title: core/src/host/tmux.rs:31) |
| hs-q3 | question | NO WORK | answered by hs-a3 |
| hs-n49 | note | NO WORK | backlog aggregate; each item classed under its own record |
| hs-p11 | plan | OWNED | Plan tui-visuals: #20, #26, #31 merged; herdr, statusline, scroll-memory live |
| hs-d103 | decision | BUILT | #26 |
| hs-n50 | note | BUILT | #26 'New Plan…, Fork… open the new Lead' |
| hs-d104 | decision | NO WORK | process |
| hs-d105 | decision | BUILT | #20 header-reverse |
| hs-d106 | decision | NO WORK | one-time ops |
| hs-n51 | note | NO WORK | fact |
| hs-d107 | decision | BUILT | #16 |
| hs-d108 | decision | BUILT | #11 skill-git-gh: pi/skills/cpd-lead/SKILL.md:13 |
| hs-n52 | note | BUILT | #27 (hs-d139): core/src/merge/mod.rs:140, core/src/pr/push.rs |
| hs-d109 | decision | BUILT | #27 (pi/lib/reload.ts) |
| hs-q4 | question | NO WORK | answered in practice by hs-d158 (no answer record) |
| hs-d110 | decision | BUILT | no send-keys into a pi pane (only MouseDown1Pane forwarding, core/src/host/tmux.rs:77) |
| hs-n53 | note | NO WORK | fact |
| hs-d111 | decision | NO WORK | routing |
| hs-d112 | decision | OWNED | manager (PR #30 open); auto-archive moved to tree-menu, BUILT #17 |
| hs-p12 | plan | OWNED | Plan manager: PR #30 open (27 Jobs merged on cpd/manager/cpd, gate-session queued) |
| hs-n54 | note | BUILT | #17: tui/src/read/shared.rs:382 LEFT JOIN prs |
| hs-n55 | note | NO WORK | fact |
| hs-p13 | plan | NO WORK | duplicate of hs-p14, archived (hs-d117, hs-n66) |
| hs-n56 | note | NO WORK | re-scope |
| hs-d113 | decision | NO WORK | process rule |
| hs-d114 | decision | NO WORK | superseded by hs-d132 |
| hs-n57 | note | NO WORK | fact |
| hs-n58 | note | NO WORK | ownership note |
| hs-d115 | decision | BUILT | #26 boxes-top-down |
| hs-n59 | note | NO WORK | coordination |
| hs-d116 | decision | NO WORK | process |
| hs-n60 | note | BUILT | #27: core/src/job/add.rs:212 clears all_merged_at |
| hs-p14 | plan | BUILT | Plan plan-reopen: #12 |
| hs-d117 | decision | BUILT | #12; adapter kinds #11 |
| hs-n61 | note | BUILT | #17: core/src/plan/row.rs:75 PLAN_WITH_PR LEFT JOIN |
| hs-n62 | note | NO WORK | backlog aggregate (items built in #20, #25, #26, #17) |
| hs-n63 | note | NO WORK | fact (ops done) |
| hs-d118 | decision | BUILT | #12 |
| hs-d119 | decision | BUILT | #12 |
| hs-d120 | decision | BUILT | #12 |
| hs-n64 | note | NO WORK | fact |
| hs-n65 | note | BUILT | guess: same cause as hs-n61, fixed by #17 (core/src/plan/row.rs:75); cause never verified |
| hs-d121 | decision | BUILT | #12 |
| hs-n66 | note | NO WORK | fact |
| hs-n67 | note | BUILT | #27 merge-step skip (core/src/merge/mod.rs:140) |
| hs-n68 | note | NO WORK | fact |
| hs-n69 | note | NO WORK | fact |
| hs-d122 | decision | BUILT | #17 |
| hs-d123 | decision | OWNED | tui-dots/followups pr-add-needs (merged on its branch; PR not open) |
| hs-d124 | decision | BUILT | #20 |
| hs-d125 | decision | BUILT | #20 |
| hs-n70 | note | BUILT | #20 'the Plan view has no top row and no tabs' |
| hs-d126 | decision | OWNED | manager (PR #30 open) |
| hs-d127 | decision | NO WORK | superseded by hs-d130 |
| hs-d128 | decision | OWNED | manager ctx-db (migration 0007), PR #30 open |
| hs-d129 | decision | OWNED | manager manager-store (migration 0010), PR #30 open |
| hs-d130 | decision | NO WORK | routing; auto-archive BUILT #17 |
| hs-d131 | decision | OWNED | manager deploy-code (0008), ctx-db, PR #30 open |
| hs-n71 | note | NO WORK | coordination |
| hs-n72 | note | NO WORK | coordination |
| hs-n73 | note | BUILT | #27 fx-follow: core/src/events/mod.rs:182 exe.replaced() |
| hs-n74 | note | BUILT | as hs-n73 |
| hs-d132 | decision | BUILT | #17 (read-no-pr) |
| hs-d133 | decision | OWNED | manager manager-store (manager_tier), PR #30 open |
| hs-d134 | decision | BUILT | #21 (PI_SKIP_VERSION_CHECK, pi-change restart, scripts/pi-check.sh); dotfiles #91 |
| hs-p15 | plan | OWNED | Plan pi-pin: #21, dotfiles #91/#92/#93 merged; ladder #23 open, drafts and tmux live |
| hs-d135 | decision | BUILT | #20 (tooltips later dropped, hs-d179) |
| hs-n75 | note | NO WORK | fact |
| hs-n76 | note | NO WORK | fact |
| hs-n77 | note | NO WORK | coordination |
| hs-d136 | decision | NO WORK | policy |
| hs-d137 | decision | BUILT | #18 (scripts/cargo.sh, CI sccache), dotfiles #90 |
| hs-d138 | decision | BUILT | #22, #24; one-time run done (hs-n98) |
| hs-q5 | question | NO WORK | answered by hs-a4 |
| hs-p16 | plan | BUILT | Plan dev-speed done: #18, #19, #22, #24, #28, dotfiles #90 |
| hs-d139 | decision | OWNED | pr push and merge-step skip BUILT #27; the known limit is replaced by hs-d189 (cross-repo/gaps) |
| hs-d140 | decision | BUILT | #27 |
| hs-a3 | answer | BUILT | #27: core/src/session/resolve.rs |
| hs-d141 | decision | NO WORK | superseded by hs-d158 |
| hs-d142 | decision | BUILT | #22 |
| hs-d143 | decision | BUILT | #22, #24 (scripts/prune-worktrees.sh) |
| hs-n78 | note | NO WORK | coordination |
| hs-d144 | decision | BUILT | dotfiles #91 |
| hs-d145 | decision | BUILT | dotfiles #91, scripts/pi-check.sh (#21) |
| hs-d146 | decision | BUILT | dotfiles #91 cpd-update |
| hs-d147 | decision | OWNED | manager adapter-updates, PR #30 open |
| hs-d148 | decision | OWNED | manager session-send, PR #30 open |
| hs-d149 | decision | OWNED | tui-visuals/statusline (sl-docs, sl-workers, sl-status merged on its branch; PR not open) |
| hs-n79 | note | BUILT | #21 install guard, reload guard, started_as (hs-n100) |
| hs-n80 | note | OWNED | install guard BUILT #21; 'ladder should not spend retries on start failures' = pi-pin/ladder (PR #23 open, ladder-code merged) |
| hs-n81 | note | BUILT | #21 (engine-exe, install-guard, reload-guard) |
| hs-n82 | note | NO WORK | coordination |
| hs-d150 | decision | BUILT | #25 (built by tui-dots instead) |
| hs-n83 | note | BUILT | #12 legacy-branch rename (verified in hs-n103) |
| hs-d151 | decision | BUILT | #21 |
| hs-p17 | plan | OWNED | Plan tui-dots: #25 merged; followups in dispatch |
| hs-n84 | note | NO WORK | coordination |
| hs-d152 | decision | BUILT | #21 |
| hs-d153 | decision | NO WORK | philosophy |
| hs-p18 | plan | OWNED | Plan quality: #29 merged; batch 1 PRs ci, nogh, quick, store, cmds, profile, suite, nextest-dot, stall live (hs-d193) |
| hs-n85 | note | BUILT | #27 fx-follow |
| hs-n86 | note | MISSING | unsure: never reproduced; pr-view-sync archived in grounding with no Job; hs-d159 says #20 fixed it (unverified) |
| hs-n87 | note | NO WORK | fact |
| hs-d154 | decision | OWNED | manager b3-*, brief-record, manager-open, adapter-manager (PR #30 open) |
| hs-n88 | note | NO WORK | coordination |
| hs-d155 | decision | NO WORK | superseded by hs-d183 (rest built in #25) |
| hs-d156 | decision | NO WORK | superseded by hs-d169 |
| hs-n89 | note | NO WORK | coordination |
| hs-d157 | decision | BUILT | #29 |
| hs-p19 | plan | NO WORK | Plan pr-view-sync archived in grounding, no Job (see hs-n86) |
| hs-d158 | decision | BUILT | #27 (pi/lib/reload.ts:11) |
| hs-n90 | note | NO WORK | fact (Plan pr-view-sync archived) |
| hs-d159 | decision | OWNED | manager pending-* (PR #30 open) |
| hs-n91 | note | OWNED | manager/pending-docs, pending-cmd, adapter-pending, skill-pending (merged on cpd/manager/cpd, PR #30 open) |
| hs-n92 | note | NO WORK | coordination |
| hs-n93 | note | MISSING | ops, unsure: pro's pi 1.3.0 and cpd rollout never verified (unreachable) |
| hs-d160 | decision | OWNED | ladder = pi-pin/ladder ladder-code (PR #23 open); prune BUILT dotfiles #92 |
| hs-d161 | decision | BUILT | #24 |
| hs-d162 | decision | NO WORK | superseded by hs-d163 |
| hs-n94 | note | OWNED | inbox (core, github, cockpit) |
| hs-n95 | note | MISSING | Epic B not planned; its pilot (Context distillation) is OWNED by record only (cross-repo PR distill, hs-d194) |
| hs-n96 | note | MISSING | Epic C not planned (third in hs-d163's order); partly fed by #19 and #29 |
| hs-d163 | decision | MISSING | Epic A OWNED (inbox); Epics B and C have no Plan |
| hs-n97 | note | MISSING | skill/Spec rules for research Jobs (skeleton in 10 min, timeouts, no synthetic benchmarks) not in pi/skills/cpd-lead/references/spec-files.md:58-64 |
| hs-d164 | decision | OWNED | manager fold-glyph (PR #30 open) |
| hs-n98 | note | NO WORK | fact (one-time cleanup done) |
| hs-n99 | note | OWNED | pi-pin/tmux (tmux-contract, tmux-require, tmux-restart queued); dotfiles #93 built the tmux 3.8 install |
| hs-d165 | decision | OWNED | inbox |
| hs-d166 | decision | OWNED | pi-pin/tmux (queued, waits on ladder); dotfiles #93 BUILT |
| hs-a4 | answer | MISSING | install guard BUILT #21; protocol check BUILT #28 (pi/test/protocol.test.ts); named migrations and the compatibility window MISSING (hs-d178) |
| hs-n100 | note | NO WORK | fact; the three hs-a4 items it lists as not built are classed under hs-a4/hs-d178 |
| hs-d167 | decision | OWNED | inbox ck-section, ck-actions (snooze, dismiss, OSC 8 clicks) |
| hs-d168 | decision | OWNED | inbox core-engine (E8), core-rules |
| hs-d169 | decision | OWNED | tui-visuals/herdr herdr-drop (merged on branch; session-reload-flake running); pi/lib/adapter.ts:202 still on main |
| hs-d170 | decision | BUILT | #27 (rl-prevalidate, rl-watchdog: core/src/session/run.rs:195 adapter_missing) |
| hs-d171 | decision | OWNED | manager flow-docs (PR #30 open) |
| hs-d172 | decision | OWNED | manager overlap (PR #30 open) |
| hs-d173 | decision | BUILT | #31 (job add --draft, Plan review approves drafts in any phase) |
| hs-d174 | decision | BUILT | #31 |
| hs-n101 | note | NO WORK | fact (since built, #27) |
| hs-n102 | note | OWNED | pi-pin/ladder ensure-flake (merged on the ladder branch, PR #23 open; touches core/src/host/tmux.rs) |
| hs-n103 | note | NO WORK | closed threads (facts) |
| hs-d175 | decision | OWNED | pi-pin/drafts (dispatch; sync-main-drafts running): a second design of #31's feature, see 'Could not decide' |
| hs-n104 | note | OWNED | pi-pin: flake fixes on ladder (hook-child-flake, etxtbsy, read-timing merged; watch-etxtbsy running); restart-tmux.sh = tmux-restart queued |
| hs-d176 | decision | NO WORK | ownership |
| hs-n105 | note | NO WORK | fact |
| hs-d177 | decision | OWNED | ids/all-merged/follow BUILT #27; pr add needs and G9/G11/G13 on tui-dots/followups; StatusBar moved to tui-visuals/statusline |
| hs-n106 | note | NO WORK | coordination |
| hs-d178 | decision | MISSING | named migrations: Plan dev-speed is done (#28 holds only the design, docs/research/migrations.md); no PR 'migrations' |
| hs-n107 | note | OWNED | quality/stall (stall-docs, stall-code queued; hs-d195); its Spec-rule half is under hs-n97 |
| hs-n108 | note | NO WORK | fact |
| hs-d179 | decision | OWNED | drafts BUILT #31; herdr and statusline live (tui-visuals) |
| hs-d180 | decision | OWNED | inbox |
| hs-p20 | plan | OWNED | Plan inbox: PRs core/github/cockpit, 12 Jobs queued or running |
| hs-n109 | note | NO WORK | correction |
| hs-n110 | note | NO WORK | coordination |
| hs-d181 | decision | OWNED | pr-conflicts/skill (queued) removes 'You cannot resolve code conflicts' (pi/skills/cpd-lead/SKILL.md:193) |
| hs-n111 | note | BUILT | #32 (core/src/host/pi.rs, core/src/session/moves.rs) |
| hs-n112 | note | OWNED | record only, no Job: session.rs:535 flake to pi-pin as the flaky-test owner (hs-n126); pi test flakes = pi-ci-W3, quality batch 2 (hs-n127) |
| hs-d182 | decision | OWNED | (1) BUILT #32; (2) record only, no Job: cross-repo PR distill (hs-d192, hs-d194) |
| hs-p21 | plan | BUILT | Plan argv-fix done: #32 |
| hs-n113 | note | NO WORK | workaround |
| hs-d183 | decision | OWNED | tui-dots/followups dot-docs merged, dot-fix running |
| hs-d184 | decision | OWNED | tui-dots/followups dot-fix running |
| hs-n114 | note | MISSING | ops, unsure: User step (/reload in three old Lead panes); not known whether done |
| hs-n115 | note | NO WORK | lesson (rule is hs-d159) |
| hs-n116 | note | NO WORK | coordination |
| hs-n117 | note | BUILT | #32 |
| hs-d185 | decision | OWNED | inbox github gh-rules |
| hs-d186 | decision | OWNED | record only, no Job: a follow-up of Plan manager once inbox/core lands |
| hs-n118 | note | NO WORK | process |
| hs-d187 | decision | OWNED | pr-links (plan-links, deploy-rows); Inbox links in inbox ck-section |
| hs-p22 | plan | OWNED | Plan pr-links: 4 Jobs queued |
| hs-d188 | decision | OWNED | (1) pr-conflicts (queued); (3) inbox gh-rules; (2) record only, no Job: pr-links draws the flag (hs-d190), deploy-rows' acceptance has no conflict |
| hs-p23 | plan | OWNED | Plan pr-conflicts: 4 Jobs queued |
| hs-d189 | decision | OWNED | cross-repo/gaps tamper-docs, tamper-base (queued) |
| hs-d190 | decision | OWNED | pr-conflicts docs/store/poll (queued); clashes with inbox gh-poll, see 'Could not decide' |
| hs-n119 | note | NO WORK | fact |
| hs-d191 | decision | OWNED | pr-links plan-links, deploy-rows |
| hs-n120 | note | NO WORK | coordination |
| hs-d192 | decision | OWNED | record only, no Job: cross-repo PR distill, after PR gaps |
| hs-n121 | note | MISSING | store-W2 is in no Job and in neither quality batch (hs-n127 omits item 4b); items 8, 10, 11, 13 OWNED by record (quality batch 2, hs-n127) |
| hs-n122 | note | MISSING | pi-ci-F2 (sccache not a CI win) has no owner (dev-speed is done); pi-ci-W1 OWNED by record (quality batch 2, item 6, hs-n127); engine-W2 and the red ladder runs OWNED pi-pin/ladder |
| hs-n123 | note | OWNED | record only, no Job: pi-pin as the flaky-test owner (hs-n126); quality/nogh does not fix it |
| hs-d193 | decision | OWNED | batch 1 = quality PRs ci, nogh, quick, store, cmds, profile, suite, nextest-dot, stall (queued); batch 2 by record (hs-n127) |
| hs-d194 | decision | OWNED | record only, no Job: cross-repo PR distill, after PR gaps |
| hs-d195 | decision | OWNED | quality/stall, suite (nextest), nextest-dot, ci (ci-pin) |
| hs-d196 | decision | OWNED | quality/cmds (cmds-titles, cmds-pr-session), store (store-guards) |
| hs-d197 | decision | OWNED | tui-visuals/scroll-memory plan-scroll-memory (queued) |
| hs-n124 | note | MISSING | no Job: per hs-n124 (not reproduced here) the merged-PR cleanup of #22 removed a Lead worktree while pr push ran its Gate there |
| hs-n125 | note | NO WORK | coordination |
| hs-n126 | note | OWNED | record only, no Job: pi-pin as the flaky-test owner |
| hs-n127 | note | OWNED | quality batch 2 by record; its five open User decisions are not decided work |
| hs-d198 | decision | OWNED | tui-visuals/statusline sl-counts (queued) |
