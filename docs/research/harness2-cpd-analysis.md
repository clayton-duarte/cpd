# Harness² (`arXiv:2610.14224v1`) × CPD Research & Plumbing Analysis

- **Date:** 2026-11-02 EST
- **Context:** `research` (records `pro-n1`, `pro-n2`, `pro-n4`, `pro-n5`)
- **Sources:**
  - Video: *Google's Harness²: Is This the End of Harness Engineering?* (`https://www.youtube.com/watch?v=yCgfyQmqhPM`, `@kaiexplainsYT`)
  - Paper: *Harness²: Recursive Agent Harnessing for an Open World* (Jiefeng Chen,ozan Sener, Sercan Ö. Arık, Jinsung Yoon, et al., Google Cloud AI Research, Oct 2026, `arXiv:2610.14224v1`)
  - Code: `https://github.com/google-research/harness2` (`harness2/core/{prompts,tree,spec}.py`, `harness2/substrate/{opencode,codex}.py`)
  - CPD Baseline: `~/projects/cpd` (`origin/main`) + live SQLite ledger `~/.cpd/cpd.db` (`2,809` Attempts across `158` Plans, `$856.92` total cost)

---

## Part 1 — Paper & Appendix H Summary

### 1.1 The `Propose → Probe → Compose` Meta-Loop
Harness² treats the agent harness itself as an editable directory tree $H$ over 8 component slots (`C1`–`C8`):
- **Always-on (`C1`–`C4`)** — capped at `24,000` chars on OpenCode (`8,000` chars on Codex `AGENTS.md`):
  - `C1`: `prompts/system.md` (role, loop shape, communication style)
  - `C2`: `prompts/guardrails.md` (stopping conditions, completion checklist, safety rails)
  - `C3`: `prompts/memory.md` (cross-task standing workspace rules)
  - `C4`: `prompts/tools.md` (tool selection and sequencing rules)
- **On-demand (`C5`–`C7`)** — loaded only when triggered:
  - `C5`: `agents/*.md` (isolated-context subagents)
  - `C6`: `skills/<name>/SKILL.md` (domain workflows; **must** have a `When <situation>, load skill <name>` trigger line in `C1`–`C4`)
  - `C7`: `scripts/*` (reusable executable helpers; **must** compile and be referenced in `C1`–`C6`)
- **Middleware (`C8`)**:
  - `plugin/*.ts` (OpenCode `tool.execute.before` / `tool.execute.after` hooks) or `.codex/hooks.json` (`PreToolUse`, `PostToolUse`, `Stop`, `UserPromptSubmit`).

Each round runs:
1. **Propose:** $K=4$ parallel editor sessions (`--Editor-1`..`4`) inspect prior task trajectories (`summary.json` + tool-call logs) and independently edit the harness tree.
2. **Probe:** Each candidate harness is validated deterministically (`tree.validate()`), then evaluated on the training tasks.
3. **Compose (`--Composer`):** If no single candidate dominates across all tasks, a Composer session inspects per-task trajectory divergences across all $K$ candidates and merges the winning mechanisms.
4. **Experience Reflector (`--Experience`):** Distills `<experience>` entries (`cue`, `lesson`, `evidence`, `confidence`) and `<confounded>` entries into `experience.json` to prevent repeating dead ends.

### 1.2 Component Ablation (Table 6, WorkBuddy)
Removing one component group at a time from the final discovered harness (`82.5` full score):
1. **Without On-Demand Skills (`C6`):** `77.2` (**`-5.3` pts** — largest single drop)
2. **Without Plugins/Hooks (`C8`):** `77.8` (**`-4.7` pts**)
3. **Without Subagents (`C5`):** `80.1` (**`-2.4` pts**)
4. **Without Scripts (`C7`):** `80.8` (**`-1.7` pts**)
5. **Without Always-On Prompts (`C1`–`C4`):** `81.0` (**`-1.5` pts** — smallest drop)

---

## Part 2 — Cross-Reference: Harness² vs. CPD Architecture

### 2.1 Where CPD Already Beats or Matches Harness²

| Harness² Mechanism / Failure Mode | CPD Equivalent (`origin/main`) | Assessment |
|---|---|---|
| **Editor-written checker scripts confirm the editor’s wrong assumption** (`WB-CODE validator cache`: checker script reported `PASS` 3× while score dropped `75 → 64`) | **Separate `test` and `impl` Workers** (`core/src/attempt/kind.rs`, `core/src/attempt/submit.rs`): a dedicated `test` Worker writes red tests (`tests_not_red` enforced at submit); a separate `impl` Worker must pass them with `test_files` locked (`tests_tampered`). | **CPD is stronger.** Separating test authoring from implementation prevents self-confirming checks. |
| **Blind Selector regressions without a rubric** (`96 → 55` on Code, `54 → 26` on LAB) | **Deterministic `[test-commands]` + Gate (`hooks.pre-commit` / `hooks.pre-merge`)**: CPD gates merges on executable test/build exit codes (`core/src/attempt/submit.rs`, `scripts/gate-run.sh`), never an unrubriced LLM selector. | **CPD is stronger** for software engineering deliverables. |
| **Pre-rollout tree validator (`tree.validate()`)** rejects malformed frontmatter, missing scripts, or bad hooks before any rollout | **`core/src/job/spec.rs` + `core/src/plan/lint.rs` + `pi/skills/skill.test.ts`**: `spec.rs` rejects unknown fields and primary-checkout paths (`check_primary_roots`); `lint.rs` runs 10 rules (`parallel_write_overlap`, `tier_advice`, etc.); `skill.test.ts` verifies every CLI flag, error code, Event kind, and default number in `SKILL.md` against `docs/commands.md` & `docs/contract.md`. | **Parity** on structural validation. |
| **`C8` Tool-Loop Hooks (`plugin/*.ts` / `.codex/hooks.json`)**: *"Prose can be stopped reading; a hook cannot."* | **`pi/durable-worker/guards.ts` & `pi/lib/adapter.ts`**: `beforeTool` / `tool_call` hooks deterministically block `mainguard`, `secretguard`, `hostapprove`, `testguard`, `noblock`, `askguard`, `exploreGuard`, and `loop-guard`. | **Parity** on blocking guards; gap on non-blocking progress watchdogs. |

---

## Part 3 — "The Huge Score Jumps Came From Simple Plumbing Rules Rather Than Deep Reasoning"

### 3.1 What That Statement Actually Means

In the video, Kai highlights a striking pattern across all six case studies in the paper (Figures 9, 15–19):
> *"These aren't AI research insights so much as plumbing fixes, where you re-open what you wrote, run the tool instead of narrating the tool, and put the file where the grader looks for it. And the value of running four agent iterations isn't that the AI has a breakthrough. It's that it finds a specific dumb failure we would never have thought to look for, and then the fix is something a person could write in 10 seconds."*

Specifically, every huge score jump in Harness² (`+25` to `+36` points) came from fixing **hand-off and boundary friction** between the model and its environment, NOT from making the model reason harder:
1. ** WB-Research (`51.9 → 88.2`, `+36.3` pts):** The agent wrote answers in its chat response instead of writing the output file to the workspace root, or cited numbers that differed from the source table. Fix: 2 plumbing rules (re-check numbers against source; write the file to the workspace root).
2. **JobBench Producers (`43.1 → 77.8`, `+34.7` pts):** `python-docx` / `openpyxl` / `matplotlib` `write` calls exited `0`, so the agent assumed the document was fine while charts had overlapping labels or blank pages. Fix: reopen every produced file and rasterize pages to PNG before finishing.
3. **WB-Web (`64.0 → 100.0`, `+36.0` pts):** The agent edited HTML/JS and stopped without booting a headless browser, missing a 1-line runtime console error or a missing CSS property. Fix: boot headless Playwright, assert `0` console/page errors, and inspect computed styles before stopping.
4. **Terminal-Bench 2.0 (`60.2 → 77.7`, `+17.5` pts):** The agent fixed the file in `/tmp` or a build directory instead of the canonical path, or left an uncommitted/dirty working tree or a zombie background process. Fix: verify canonical path + clean tree before exit.
5. **Social-Science RAs (`72.0 → 100.0`, `+28.0` pts):** Stronger reasoning models (`Opus 5`) spent all 30+ turns reading data and never started writing the report until the turn budget expired. Fix: a 15-line `tool.execute.after` counter (`deliverable_watchdog.ts`) that injects a 1-line warning into the tool output after 25 tool calls with no deliverable written.

**The core insight:**
Frontier models rarely fail because they lack intelligence to write the code. They fail (or burn 3× the tokens and human/Lead escalations) because of **dumb mechanical traps at the seams of the harness**:
- A check runs *after* submit instead of *during* submit.
- Approval A (`approve-tests`) unlocks Guard 1 (`tests_tampered`) but forgets Guard 2 (`touches_violation`).
- A batch check (`!(A && B && C)`) checks that *at least one* test failed instead of *every* test failing.
- A crash recovery path (`E2` / `abort`) kills the orphan process but forgets to `git reset --hard && git clean -fd` the shared linear worktree, poisoning the next Attempt with `setup_failed`.
- A policy feature (`[generated] paths`, `[protected] paths`) exists in the Rust engine, but the `.toml` files leave it `[]`.

---

### 3.2 Applying the Harness² Method to CPD’s Own Trajectories (`~/.cpd/cpd.db`)

Instead of guessing where CPD loses value, we can run the exact same trajectory-inspection pass over CPD’s live SQLite ledger (`~/.cpd/cpd.db`: **2,809 Attempts**, **491 Worker Escalations**, **534 failed Attempts**).

Here are the **7 concrete, high-value plumbing fixes** exposed by CPD’s own trajectory data — each one is a small, mechanical change ("something a person could write in minutes") that eliminates an entire class of wasted Attempts or Lead Escalations.

---

#### Plumbing Fix 1: `approve-tests` Triggers a Guaranteed `touches_violation` Trap on `impl` Jobs
- **Empirical evidence from `~/.cpd/cpd.db`:**
  - Out of **675 `impl` Jobs**, **151 (22.4%)** had `test_approved` set (`cpd job approve-tests`).
  - Since `touches_approved` (`20261103T0300-job-touches-approved.sql`) landed, **11 out of 13 `impl` Jobs** that received `test_approved` (`inbox-async-impl`, `direct-impl`, `direct-archive-impl`, `standing-pill-space-impl`, `standing-ui-impl`, `archive-dock-impl`, `settings-ui-impl`, `codemode-impl`, `dock-impl`, `perm-live-impl`, `lint-engine-cmd`) immediately hit **`touches_violation`** on `cpd attempt submit` for the **exact same test file** the Lead had just approved with `cpd job approve-tests`!
  - Why? Look at `core/src/attempt/submit.rs:281-303`:
    1. `run_checks()` runs `git::tamper::touched_outside()` **first**, passing `approved = crate::job::guard::approved_touches(job)` (which reads **only** `job.touches_approved`).
    2. An `impl` Job’s `touches` lists only production files, not the `test` Job’s `test_files`.
    3. So when the Lead runs `cpd job approve-tests <impl-job> --path <test-file>`, `kind::guard_tests()` (line 303) is satisfied, **but line 287 (`touched_outside`) rejects the commit with `TouchesViolation`!**
    4. The Worker is forced to pause a second time, run `cpd ask` a second time, and wait for the Lead to run `cpd job approve-touches <impl-job> --path <test-file>` on the exact same path.
- **The 10-second plumbing fix:**
  - In `core/src/attempt/submit.rs:281` (or `core/src/job/guard.rs:approved_touches`), union `job.test_approved` into the `Touches.approved` slice for `impl` Jobs. Approving a red-test path on an `impl` Job (`cpd job approve-tests`) must implicitly allow touching that path in `Touches`.

---

#### Plumbing Fix 2: Dead-Driver (`E2`) and Aborted-Attempt Teardown Leave Linear Worktrees Dirty → `setup_failed` Cascade
- **Empirical evidence from `~/.cpd/cpd.db`:**
  - In the last 5 days (`2026-10-28` to `2026-11-02`), **7 Attempts** failed with `setup_failed: The workflow worktree ... is not clean` (`derive-tests-a2`, `lint-engine-verify-a4`, `shared-tests-a5`, `clippy-cli-b-a2`, `padroot-pin-a1`, `cleanup-docs-fix-2-a1`, `open-verify-a5`), plus 1 with `detached HEAD` (`fix-layout-context-fold-a1`).
  - Every `setup_failed` is **terminal** for the Job (`Job <key> failed (setup_failed)`) and wakes the Lead/Manager.
  - Why did the linear workflow worktree have dirty/untracked files when `-a2` started?
    - Look at `core/src/driver/run.rs:1412`: when a **live** driver ends a failed linear Attempt, `cleanup()` calls `reset_linear_lead_branch(&target.worktree, &target.lead_branch, &base_sha)`, which runs `git reset --hard <base_sha>` and `git clean -fd`.
    - **However**, when a driver **dies** (`crashed` / `infra`) and `E2` (`engine/src/watch/liveness.rs:146` `end_dead`) cleans it up, or when `teardown_aborted` (`core/src/driver/mod.rs:110`) or `merge::recover` `R3` (`core/src/merge/recover.rs:146`) cleans it up, they call `runner::kill_orphan(...)` **without calling `reset_linear_lead_branch(...)`**!
    - So whatever uncommitted edits or untracked files Attempt 1 wrote before its driver died sit in the linear Lead worktree, and when `E3` spawns Attempt 2 on the ladder, `verify_linear_worktree()` (`core/src/driver/run.rs:1553`) immediately fails Attempt 2 with `setup_failed: the Lead worktree ... is not clean`!
- **The 10-second plumbing fix:**
  - Call `reset_linear_lead_branch` (when `is_linear` and `base_sha` is recorded) inside `end_dead` (`engine/src/watch/liveness.rs`), `teardown_aborted` (`core/src/driver/mod.rs`), and `R3` (`core/src/merge/recover.rs`).

---

#### Plumbing Fix 3: `check_test` Uses Batch Exit Status (`!A || !B`) Instead of Per-Test Redness (`!A && !B`) & Accepts Deadlocks/Setup Panics as "Red"
- **Empirical evidence from `~/.cpd/cpd.db`:**
  - **42.8% of all Worker Escalations in CPD history (210 / 491)** happen on `impl` Jobs, and **208 of those 210** are about guarded tests written by the preceding `test` Job!
  - Why are `test` Jobs merging broken tests so often (151 / 675 `impl` Jobs needing `test_approved`)?
  - Look at `core/src/attempt/kind.rs:254-273`:
    1. **De Morgan batch bug:** `list` runs in a `for name in tests` loop (line 219) to verify each test exists individually, **but `run` (line 255) passes all `tests` in a single batch (`&[("tests", json!(tests))]`)** and only checks that the batch did not exit `0` (`("test", HookOutcome::Passed) => Err(TestsNotRed)`). If a `test` Job lists 6 tests and 1 fails while 5 **pass** on the unimplemented base branch, `cpd test run` exits `1` and `attempt submit` accepts all 6 as "red"!
    2. **Deadlocks pass as "red":** Multiple `test` Jobs merged tests that called `Fx::new()` twice in the same test (`dock-impl`, `lint-engine-cmd`, `direct-impl`), deadlocking on the global mutex until `nextest`'s 90s timeout killed them with exit code `!= 0` — which `check_test` accepted as a valid red test!
    3. **SQLite fixture syntax/constraint errors pass as "red":** Multiple `test` Jobs merged tests whose raw SQL setup crashed with `NOT NULL constraint failed`, `CHECK constraint failed`, `append_only`, or `near "*": syntax error` (`steer-impl`, `fp-schema-impl`, `advice-impl`, `lint-engine-cmd`, `provider-impl`, `dedup-impl`) before ever reaching the code under test.
    4. **Repo-wide SQL/lint invariants broken by `test_files`:** A `test` Job writes `SELECT *` in `core/src/store/schema/tier_rows_tests.rs` (`tier-migrate-v2-impl`), which breaks `core/tests/migrations.rs::no_code_selects_star_or_inserts_without_a_column_list`, but `check_test` only runs the `test` Job's named `tests`, so the violation isn't caught until the `impl` Job runs!
- **The plumbing fixes:**
  1. In `core/src/attempt/kind.rs:254`, for a `test` Job, verify that **every** named test in `tests` fails (not just $\ge 1$ of the batch), and reject any test whose failure output in `nextest` is `TIMEOUT` / `SIGABRT` rather than `FAIL`.
  2. Refuse `attempt submit` on a `test` Job if its `run` output contains SQLite fixture setup panics (`SQLite error`, `constraint failed`, `syntax error`) unless the test itself is in `store::schema`.

---

#### Plumbing Fix 4: `[generated] paths` and `[protected] paths` Are Built in Rust, But Empty (`[]`) in Every `~/.cpd/policy/**/*.toml` File
- **Empirical evidence from `~/.cpd/policy/` and `~/.cpd/cpd.db`:**
  - `grep -rn "generated\|protected" ~/.cpd/policy/` returns **0 matches** across all policy files (`clayton-duarte/cpd.toml`, `clayton-duarte/dotfiles.toml`, `missionlane/*.toml`).
  - Meanwhile, CPD already built `[generated] paths` and `[protected] paths` in `core/src/policy/`, `core/src/git/tamper.rs` (`Touches.generated`), and `core/src/plan/lint.rs` (`parallel_write_overlap` exemptions)!
  - Because `[generated] paths` is unset in `~/.cpd/policy/clayton-duarte/cpd.toml`, Workers that legitimately regenerate `docs/interface.md`, `pi/test/cpd-usage.json` (`tier-write-impl`), or `Cargo.toml`/`Cargo.lock` via `scripts/sync-version.sh` (`direct-submit-impl`) get blocked by `touches_violation` at submit and have to escalate to the Lead.
  - Conversely, `~/.cpd/policy/missionlane/owner.toml` lines 40–41 state **in prose**:
    > *"Never touch `.github/` or CI files, `src/gql/` (distributed from the backend schema), `playwright.config.ts`, `EftaV3.tsx` or `EftaV3.css`."*
    while `[protected] paths` in `missionlane/card-dashboard.toml` is empty!
- **The 10-second plumbing fix:**
  - Populate `[generated] paths = ["docs/interface.md", "pi/test/cpd-usage.json", "Cargo.lock", "pnpm-lock.yaml"]` in `~/.cpd/policy/clayton-duarte/cpd.toml`.
  - Populate `[protected] paths = [".github/", "src/gql/", "playwright.config.ts", "src/@core/components/Efta/EftaV3.tsx", "src/@core/components/Efta/EftaV3.css"]` in `~/.cpd/policy/missionlane/card-dashboard.toml` (turning prose into enforcement per Axiom 17).

---

#### Plumbing Fix 5: `hooks.pre-commit` Runs at Merge Instead of `cpd attempt submit`
- **Empirical evidence from `~/.cpd/cpd.db`:**
  - Recent Attempts like `status-config-tiers | tier-model-config-tests-a1` (`test`, `S1`, `2026-11-02`), `explore-view | drawn-docs-a1` & `drawn-docs-a2` (`docs`, `H3`, `2026-11-01`), `review-head | head-guard-docs-a1`, and `stack-review-ready | peer-docs-a1` passed `cpd attempt submit`, shut down the Worker, entered `merging`, and then died with **`gate_failed` on `hooks.pre-commit`** (`cargo clippy --workspace --all-targets -- -D warnings` or `cargo build --workspace`).
  - Why? `core/src/attempt/submit.rs:254` (`run_checks`) runs `scripts/tests.sh build` (`nextest --no-run` + `pnpm run typecheck`) ONLY for `test` and `impl` Jobs (never `clippy` or `fmt`), and runs **nothing** for `work` or `docs` Jobs. Then `hooks.pre-commit` (`typecheck`, `fmt`, `cargo build`, `clippy -D warnings`) runs at merge (`§7.4 step 6`), when the Worker is already dead!
  - In Harness² terms: this is the exact difference between checking before stopping vs. failing the grader after stopping.
- **The plumbing fix:**
  - Run `hooks.pre-commit` inside `cpd attempt submit` (`run_checks`), so a `fmt`, `clippy`, `typecheck`, or `cargo build` error refuses `cpd attempt submit` in-session while the Worker is alive and can fix it in 30 seconds, instead of killing the Attempt at merge as `gate_failed`.

---

#### Plumbing Fix 6: Eliminate Duplicate Always-On Prompt Prose & Enforce Triggered Progressive Disclosure (`C1`–`C4` vs `C6`)
- **Empirical evidence from `~/projects/cpd/pi/skills/`:**
  - `pi/skills/cpd-lead/SKILL.md` is **83,371 bytes** (113.1 KB with references) loaded into every Lead session on turn 1.
  - `pi/skills/cpd-lead/references/spec-files.md:83-90` tells the Lead:
    > *"Put these in the `context` of every Spec (they are a Worker's only rules): Commit a skeleton within 10 minutes... Run only the tests for your change..."*
    even though `core/src/spec/worker.md:94-101` **already compiles those exact 4 rules into every Worker Spec automatically**! Every Lead is spending output tokens typing those 4 rules into every `spec.json`, and every Worker reads them twice.
- **The plumbing fix:**
  1. Delete the duplicate "Put these in the `context` of every Spec" instruction from `pi/skills/cpd-lead/references/spec-files.md:83-90`.
  2. Split phase-specific sections of `pi/skills/cpd-lead/SKILL.md` (`Deploying`, `Stacked Workflows`, `Addressing`, `Failed Jobs`) into `pi/skills/cpd-lead/references/*.md` with explicit `When <phase/event>, read references/<file>.md` trigger lines in `SKILL.md`, and enforce both a `SKILL.md` byte cap and a reference-trigger check in `pi/skills/skill.test.ts` (just like `tree.py`'s `always_on_max` and `_check_skills`).

---

#### Plumbing Fix 7: Non-Blocking Progress Watchdog Hook (`deliverable_watchdog.ts`) + Retry Trajectory Footprint
- **Empirical evidence from `~/.cpd/cpd.db`:**
  - On non-`open` Jobs, `no_submit` and `timed_out` cluster on multi-file `work`/`test`/`scout` Jobs (`convert-snaps-a1/a5/a6`, `runner-tests-a1`, `final-sleep-a3`, `fp-merges-a1`), and when Attempt 1 fails with `no_submit`, `timed_out`, or `crashed`, `core/src/spec/mod.rs:34-43` passes `Evidence::None` to Attempt 2 — giving the retry Worker only a static 1-line string (*"The Worker ran out of time before it submitted"*).
- **The plumbing fixes:**
  1. Add a non-blocking `afterTool` counter in `pi/durable-worker/extension.ts` & `pi/lib/adapter.ts` (mirroring Harness²’s `deliverable_watchdog.ts`): after 10 minutes or 20 tool calls with 0 commits (or 0 writes to the explore artefact), append a 1-line `[cpd watchdog]` reminder to the tool result.
  2. In `core/src/spec/mod.rs`, populate `Evidence` for `TimedOut`, `NoSubmit`, and `Crashed` with the previous Attempt’s changed/uncommitted file paths (`actual_files` or `git status --porcelain` before reset) so the retry Worker sees where Attempt 1 was working.
