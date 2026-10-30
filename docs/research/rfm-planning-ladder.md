# Planning, plan lint and the model ladder: an audit of cpd against Section 8 of the retrieval-first memory proposal

## Question and scope

Section 8 of the "CPD Context: Retrieval-First Memory Proposal" ("Planning: parallelization, plan lint and the model ladder" and "Job log schema") proposes three things for cpd: declared Job footprints with engine-computed serialization (P1), draft-first incremental Plan lint keyed by a content hash (P2), and a model ladder driven by a per-attempt telemetry log (P3). This document asks what cpd already does today, so that the later sections can say what is worth building and what duplicates an existing mechanism.

This research is **read-only**: it writes no production code, no migration and no test. This file is built by three serial Jobs and each fills its own sections only:

- `research-docs` (this Job): Method, the audit (what cpd already built vs the proposal) and the findings on the existing code;
- `research-data`: "Data analysis" (live database and footprint numbers; none appear here);
- `research-design`: "Evaluation of the three sub-proposals" and "Roadmap".

## Method

**Proposal source.** `/tmp/rfm-proposal.md`, Section 8, lines 253-347 (the Spec of this Job reproduces it verbatim). The doc cites it as "Proposal, `<subsection>`" and quotes at most one line at a time.

**Code and docs read.** The worktree is the Job's branch cut from commit `77b80a89` (the `cpd` Rust workspace: `core/`, `cli/`, `engine/`, `tui/`, `pi/`). Every claim below cites a repo-relative path and a symbol or line, read with `rg`/`grep -n` and `sed -n`. Nothing was built or run, and no database was read: this Job has no access to `cpd.db`, so no database number is stated here (the 134 `gate_failed` Attempts with no `check_tail` are the Lead's figure, quoted as given and marked as such).

**Labels.** Anything not confirmed by reading the cited code is marked `unverified`, `guess:` or `I infer` in the same sentence. Where a doc and the code disagree, the code is taken as the truth and the disagreement is reported.

**The brief's names, corrected.** The brief used table names that do not all exist. The check below is against the contract DDL (`docs/contract.md`, section 4.4) and the migrations (`core/src/store/migrations/`):

| Name in the brief | What cpd has | Evidence |
| --- | --- | --- |
| `job_needs` | The table `dependencies(plan, job, needs)` | `docs/contract.md:1604`; read by `core/src/plan/jobs.rs` (`load`, `SELECT job, needs FROM dependencies`) |
| `pr_needs` (Lead's note: "there is no such table") | **It does exist**: `pr_needs(plan, pr, needs)` holds the needs between PRs (Workflows) | `docs/contract.md:1530`; `core/src/plan/prs.rs:99` (`SELECT pr, needs FROM pr_needs`); `core/src/store/migrations/20261021T1300-local-needs.sql` |
| `job_resources` | The JSON column `jobs.resources` (a Job's declared pools), plus `job_pools` (the pools a Job holds once started) and the view `job_tenants` | `docs/contract.md:1585`; `core/src/store/migrations/20261101T0901-job-tenancy.sql` |
| a Plan-review table | `plan_reviews(plan, id, number, fingerprint, status, feedback, requested_at, decided_at, title)` | `docs/contract.md:1540` |
| "Plannotator fingerprint caching" | `plannotator_reviews(session, pn_id, kind, subject, port, opened_at, closed_at, ended)`, a log of UI sessions with no fingerprint (item 3) | `docs/contract.md:1796`; `core/src/review/mod.rs:1` |

The proposal also reuses the name `jobs` for a different table. Its `jobs` is a per-Job record (`content_hash`, `declared_write`, `kind`, `language`, ...) and its `job_attempts` is one row per Attempt, while cpd's `jobs` is the Locked, mutable Job row and cpd's `attempts` is already one row per Attempt. Item 6 maps each proposed column onto cpd's `jobs`, `attempts`, `dependencies` and `plan_reviews`.

**Vocabulary.** The glossary (`CONTEXT.md:44`) calls the part of a Plan that lives in one repository and ends in one pull request a **Workflow**, and says the CLI, tables and Events still call it a "PR" until a rename lands (`CONTEXT.md:16`). This audit writes "PR" (as the code does) and says "Workflow" where it compares with the proposal's word.

## Audit: what cpd already built vs the proposal

### 1. Workflow and Job DAG

**The hierarchy.** A Plan holds one or more PRs (Workflows); a PR holds Jobs. Job `needs` are rows of `dependencies` and are **same-PR only** (`needs_other_pr`, `core/src/job/mod.rs`, `check_needs_same_pr`). PR-to-PR order is `pr_needs` plus the implied need of a stacked PR whose `--base` is another PR's Lead Branch (`core/src/pr/start.rs:56-101`, macro `needs_unmerged!`; `core/src/pr/add.rs:83-86`, field `needs`; `base_prs` in `core/src/plan/review.rs:121`). A PR stays `waiting` until every need is `merged`; `plan lock` starts the PRs that need nothing (`core/src/plan/lock.rs:93-102`).

**PR shapes.** `pr add --shape` lays out stub drafts (`core/src/pr/add.rs:52-70`, `fn template`): `tdd` is `<pr>-docs` (docs), `<pr>-tests` (test, needs docs), `<pr>-impl` (impl, needs tests); `docs` is one docs Job; `config` is one `work` Job; every shape ends with the stub `open-<pr>`. `core/src/plan/shape.rs` enforces them at review and lock: `test_job_required`, `impl_job_required`, `shape_incomplete` (`shape.rs:39-95`, `fn check`).

**Job kinds.** The `jobs.kind` CHECK lists `work`, `docs`, `test`, `impl`, `verify`, `open`, `update`, `scout`, `research` (`docs/contract.md:1581`). `verify` was added by the rebuild `core/src/store/migrations/20261101T0900-job-verify-kind.sql` (header: "pro-d80: a new Job kind `verify`"): a check-adding Job that needs an `impl` Job of its PR and changes no product behaviour (`docs/contract.md:440`). `open` and `update` are cpd's own: `job add --kind open|update` is `kind_reserved` (`core/src/job/mod.rs`, `check_kind_not_reserved`); E4 adds `open`, the PR poll adds `update` on a merge conflict, both at `open_tier` (`docs/contract.md:451-452`). The red-test guard (`core/src/job/guard.rs`; `core/src/merge/mod.rs:207-221`) makes an `impl` diff that touches a `test` Job's `test_files` fail `tampered`.

**How Ready Jobs are chosen and started** (engine step E3, `engine/src/watch/schedule.rs`):

- `READY` (line 80) is one SQL query: Job `queued`, Plan in `dispatch` or `deploy`, PR `dispatch` or `open`, every `dependencies` need `merged`; `open` and `update` have a derived rule from their PR's other Jobs. The result is ordered by `urgent DESC, seq`.
- `pick` (line 152) builds the pools (`workers` = `concurrency`, plus each resource the Job names; item 2) and calls the pure `schedule()` (`core/src/driver/schedule.rs`): urgent first, then previously skipped Jobs, then the queue; a Job that misses a pool is skipped and reserves nothing.
- `allocations` (line 102) decides, per PR, whether an Attempt is **linear** or **parallel**: a PR with no live Attempt and exactly one Ready Job starts it `Linear` (the Attempt works directly in the PR's workflow worktree on the Lead Branch); a PR with two or more Ready Jobs, or with live parallel Attempts, starts every Job `Parallel` (own worktree and branch); a PR with a live linear Attempt starts nothing. The decision uses only counts of Ready and live Jobs: **no file or footprint information enters it**.
- The starter `driver::start_with_allocation` (`core/src/driver/start.rs`) inserts the Attempt and spawns the driver `attempt run` (`core/src/driver/run.rs`, steps A1-A9).

So the DAG is declared by the Lead (`job add --needs`), stored in `dependencies`, and read as a Ready set on every E3 tick. There is no stored schedule and no engine-built graph object beyond `jobs::cycle` (`core/src/plan/jobs.rs:276`), which looks only at `draft` Jobs.

**How an Attempt reaches the Lead Branch** (`core/src/merge/mod.rs`, driver step A8, "no lock", module header and `merge()`):

1. Tamper check: the Attempt diff may not touch the policy's protected paths or, for an `impl` Job, the guarded red-test files (`touched_protected_over`, `touched_guarded`, lines 203-221) or the Attempt fails `tampered`.
2. A parallel Attempt is rebased onto the Lead Branch tip (line 238); a conflict ends it `merge_conflict` (lines 235-240). A linear Attempt is already on the Lead Branch and never rebases.
3. The Gates run in the Attempt worktree at HEAD, after the rebase and **before** the fast-forward: `gate.merge` (the repo's `pre-commit` hook, the quick check) at every Job merge, `gate.pr` (`pre-merge`) only for `open` and `update` Jobs (`run_gate`, line 400; calls at lines 260-269).
4. Fast-forward by ref compare-and-swap: `git update-ref <lead_ref> <new> <old>`. If the ref moved, the loop goes back to step 2 (which re-runs the rebase **and the Gates**), at most `CAS_RETRIES = 3` times (line 38), then ends `cas_lost` (line 312).
5. Rule S: `git::sync_lead` brings the Lead worktree to the new tip (`core/src/git/lead.rs:49`).

**What is protected, precisely.** The term `stale-tree` appears in neither `docs/contract.md` nor `docs/research/quality.md` (exact-string grep returned nothing); the concept is **rule S** (contract section 7.4, `docs/contract.md:3857-3863`, rationale in `docs/research/no-repo-lock.md:107,129`):

| Protection | What it protects | Where |
| --- | --- | --- |
| Ref CAS at step 8 | Two merges of one PR cannot overwrite each other's fast-forward; the loser retries from step 2, then fails `cas_lost` | `core/src/merge/mod.rs:290-317` |
| Tamper check | Protected paths and red tests are not changed by an Attempt | `core/src/merge/mod.rs:203-221`; `core/src/git/tamper.rs:54` |
| Rule S | After the ref moved, the Lead worktree is rewritten only when its tree equals an earlier tip of the branch (a stale tree). A Lead's own staged or unstaged edit is never overwritten: rule S stops with an `error` Event `lead_worktree_stale` and the merge stands | `core/src/git/lead.rs:49`; `docs/contract.md:3857-3863` |
| Branch check (step 0) | The submitted commit is still on the branch (`tampered` otherwise) | `core/src/merge/mod.rs:136-161` |
| Linear allocation | A linear Attempt is the PR's only live Attempt, so nothing else moves the Lead Branch under it; a failed linear Attempt resets the branch to its `base_sha` | `core/src/driver/start.rs:322`; `core/src/driver/run.rs:1038-1056` |

`dirty_target` is **declared but never produced**: the `Cause` enum documents it as "Merge step 3: the Lead worktree is not clean" (`core/src/attempt/ledger.rs:177`), the merge code says step 3 was removed by hs-d243 (`core/src/merge/mod.rs`, comment "3. Removed by hs-d243"), and a grep of `Cause::DirtyTarget` / `Failure::DirtyTarget` outside tests finds only the declaration and the Spec sentence (`core/src/spec/mod.rs:81`). The producers of the other merge causes are `tampered` (`merge/mod.rs:148,160,207,221`), `merge_conflict` (235: an `open`/`update` Job whose Lead Branch moved; 240: the rebase conflicted; 255: `base_check` found a base conflict), `cas_lost` (312), `gate_failed` (413), `gate_error` (417, 423 and 83) and `crashed` (336, 344: a failed push or a `gh` failure of an `open` Job).

**What the proposal assumes that is true / false / absent here.**

| Proposal assumption | State in cpd |
| --- | --- |
| Jobs form a DAG the engine reads | True: `dependencies` and the `READY` query |
| The engine serializes Jobs whose write sets overlap | Absent: serialization is by `needs` and by the count-based linear/parallel allocation only (`schedule.rs:102`); a Job's `touches` never enters scheduling (item 2) |
| Jobs of one workflow are integrated through a queue | False as worded: there is no queue or lock; parallel Attempts race a ref CAS and retry (`merge/mod.rs:38`, `docs/contract.md:3835`: "The merge takes no lock") |
| A bad integration is caught at its source by a build after each merge | True in effect, in a different order: the quick check runs on the rebased HEAD before the fast-forward, on every retry of the CAS loop, when `gate.merge = local` (`merge/mod.rs:260-263`) |

### 2. Resource pools, `touches` and the validations that exist

**Resource pools.** A Job may list `resources` in its Spec (`jobs.resources`, JSON array). Each name must be declared by the policy of its PR's repo as `[resources.<name>]` with `slots` (an integer from 1 to 64, `core/src/policy/load.rs:73`) at the machine (`host.toml`), owner or repo scope; an undeclared name is `resource_undeclared` at `job add` (`core/src/job/add.rs:320`). The built-in pool `workers` has `concurrency` slots (config default 3, `core/src/config/file.rs:272`; counted over Jobs of every Plan on the machine, `core/src/resource/mod.rs:39-56`). E3 gives a non-urgent Job a slot of every pool it needs, all or none; a Job that misses one is **skipped**, takes nothing, reserves nothing, and gets `jobs.skipped_n` (the E3 skip rule, `engine/src/watch/schedule.rs:191-205`; `core/src/driver/schedule.rs:50-79`), so skipped Jobs are walked before the rest of the queue. The tenancy is recorded in `job_pools` and the view `job_tenants` (`docs/contract.md:1669`). The value `slots = 3` for `cargo` in `host.toml` is the Lead's statement; I did not read `host.toml` (unverified). `job show` reports `waiting_for` for a queued Job (`core/src/job/show.rs:68`); `driver::start`'s `require_ready` reports the same as `details.waiting_for` of `job_not_ready` (`core/src/driver/start.rs:392`).

**Every consumer of `jobs.touches`** (`rg touches core/src engine/src cli/src`, non-test, non-SQL-fixture hits):

| Consumer | Use | Enforces anything? |
| --- | --- | --- |
| `core/src/job/add.rs:420` (`check_touches`), called at `add.rs:224` and `core/src/job/edit.rs:210` | At `job add` / `job edit --spec` each entry must exist in the tree the Job starts from (a literal path or its parent directory, a glob with a match or an existing prefix); else `touches_missing` | Checks the **declaration against the tree**, not the Worker's diff |
| `core/src/plan/overlap.rs` | Cross-Plan overlap warning | Warning only |
| `core/src/plan/jobs.rs:112,140`, `core/src/plan/fingerprint.rs` | Part of the Locked fields hashed into both fingerprints (item 3) | Detects a changed declaration |
| `core/src/spec/mod.rs:274`, `core/src/spec/worker.md:139-142` | Rendered to the Worker as "Files you may touch" | Prose only |
| `core/src/plan/render.rs:260,332`, `core/src/context/pending.rs:104-123`, `docs/commands.md:557` | Shown in `plan render`, `plan review` and `pending` output | Display |

**Nothing enforces `touches` at submit or merge.** The only diff-vs-path checks in `attempt submit` and in the merge are the protected-path check and the red-test guard (`core/src/git/tamper.rs:54,95`; `core/src/attempt/submit.rs`, `run_checks`); neither reads `jobs.touches`. The Worker contract says the opposite of a hard limit: a Worker "may change a file outside its `touches` when a test named in its acceptance cannot pass without it" and lists it in the commit message (`docs/commands.md:4495`). A reusable primitive exists, `git::tamper::changed_paths` (`core/src/git/tamper.rs:87`, `git diff --name-only --no-renames`), but no consumer compares it with `touches`.

**`overlap.rs` semantics, exactly.**

- Path match: `overlaps(a, b)` is equality, or one path ends in `/` and is a prefix of the other (`overlap.rs:55`). No globs, no normalisation (`docs/commands.md:928`). A glob in `touches` is therefore compared as a literal string.
- Which Jobs: this Plan's Jobs not `cancelled`, in PRs not `merged` or `cancelled` (`overlap.rs:94,103-110`), against the Jobs of **other** Plans (`WHERE p.plan <> ?1`, line 68) that are not `done` or archived, in PRs not `merged` or `cancelled`, Jobs not `cancelled`, in the same repo realpath (`o.repo == pr.repo`, `overlap.rs:118`). Any status of the other Job except `cancelled` counts (`draft`, `queued`, `running`, `failed`, `merged`).
- Where shown: `plan render` (the `## Overlaps` section and the `overlaps` field of its output, `core/src/plan/review.rs:147-163`) and the Plan review (`snapshot`, `review.rs:100-118`). `job add` does not warn (`docs/commands.md:928`). `plan lock` does not use it (`core/src/plan/lock.rs` has no reference to `overlap`).
- **Two Jobs of the same PR, or of two PRs of the same Plan, are never compared.** The SQL excludes the Plan's own Jobs, so same-PR overlap is checked neither at `plan lock` nor at any other point. The one same-PR serialization is `needs`.

**Validations that exist today.** There is no lint pass. The list below is exhaustive for the commands named, found by reading each command's code:

| Command | Validations (error code) | Where |
| --- | --- | --- |
| `job add` | tier and spec required unless `--draft` (`usage`); phase (`illegal_phase`); key unique (`job_key_exists`); self-need (`dependency_cycle`); `needs` known (`needs_unknown`) and same PR (`needs_other_pr`) and not cancelled/failed/open/update (`needs_unavailable`); kind reserved (`kind_reserved`); kind allowed by the PR shape (`shape_violation`); `impl` needs a `test` Job (`test_job_required`), `verify` needs an `impl` Job (`impl_job_required`); Spec grammar, no primary-checkout paths, relative `test_files` (`spec_invalid`); tier range (`tier_invalid`, `tier_not_allowed`); resources declared (`resource_undeclared`); `touches` exist (`touches_missing`); PR live (`pr_ended`); explore-kind rules | `core/src/job/add.rs`, `core/src/job/mod.rs:285-475`, `core/src/job/spec.rs` |
| `job edit` | nothing to edit (`nothing_to_edit`); Locked (`job_locked`, `not_a_lead`); the same Spec, tier, `needs` and `touches` checks; `dependency_cycle` over the draft graph (`jobs::cycle`) | `core/src/job/edit.rs:52-246` |
| `plan render` | none refused; it computes the overlap warning | `core/src/plan/review.rs:147` |
| `plan review` | phase (`illegal_phase`); at least one draft (`no_draft_jobs`); every draft has tier and Spec (`draft_incomplete`); outline present and covers each PR with a draft (`outline_missing`, `outline_incomplete`); shape (`test_job_required`, `impl_job_required`, `shape_incomplete`); stale-state CAS (`cas_lost`) | `core/src/plan/review.rs:166-230,280,316`; `shape.rs:39` |
| `plan lock` | phase (`illegal_transition`); `no_draft_jobs`; `draft_incomplete`; the shape checks; **an approved review for the current fingerprint (`plan_not_approved`)**; every live PR has a draft Job (`pr_empty`); `dependency_cycle`; the PR start (`start::prepare`); CAS (`cas_lost`) | `core/src/plan/lock.rs:51-130` |

`tests_unconfigured` is refused when a `tdd` PR is added without `[test-commands]` in the repo policy (`core/src/pr/add.rs:444,711`) and again at a `test` or `impl` Job's submit (`core/src/attempt/kind.rs:59`). Of the proposal's six lint checks (Proposal, "Checks in the lint"): cycles are checked only among `draft` Jobs (`jobs::cycle`); orphan Jobs, undeclared overlap between parallel Jobs, footprint vs likely files, Job size vs worker context, self-containedness, and brief coverage are **absent**.

### 3. Plan review fingerprinting

**What is hashed.** `plan_fingerprint` (`core/src/plan/fingerprint.rs:19`) is SHA-256 of canonical JSON (`sha256_of`, `core/src/plan/jobs.rs`, sorted keys because `serde_json` is built without `preserve_order`). `of_with` (`fingerprint.rs:42`) hashes:

- every Job that `is_under_review` (a draft that is not cpd's `open` stub; `jobs.rs:83`), sorted by key, with `tier, goal, context, touches, acceptance, out_of_scope` (`locked_fields`, `jobs.rs:105`), `key`, `pr`, `title`, sorted `needs`, and `kind` (only if not `work`), `tests`, `test_files` (`kind_fields`);
- every PR not `cancelled`: `key, repo, base, sorted needs`, and `shape` if not `free`; PRs are included only while the Plan is in phase `grounding` or `plan` (`reviews_prs`, `fingerprint.rs:26`);
- the Plan outline when set.

**In no fingerprint:** `jobs.resources`, `urgent`, `test_approved`, `skipped_n`, `summary`, and the PR's `title` and `jira` (a PR contributes only `key, repo, base, needs, shape`). `resources` is documented as guarded by `jobs_locked` but "in no fingerprint" (`docs/contract.md:1585`). Of these, `resources` changes what a Job may run alongside, so a pool change after approval does not invalidate the review (I infer this from the field list; I did not test it).

**How an edit invalidates approval.** `plan review` stores the fingerprint it computed in `plan_reviews.fingerprint` (`review.rs:220-224`) and returns `already: true` when the latest review is pending for the same fingerprint (`review.rs:180-193`). `plan lock` recomputes the fingerprint and accepts only a latest review that is `approved` with an equal fingerprint (`latest_approved_for`, `review.rs:459`), else `plan_not_approved` (`lock.rs:62`). After the lock, `plan decide --approve` queues the drafts only if the review's fingerprint still equals the current one (`review.rs:374-385`); otherwise nothing is queued and the text says the drafts changed.

**Per-Job hash today.** Yes, a second, per-Job fingerprint exists: `jobs.fingerprint` (TEXT of length 64, `docs/contract.md:1557-1590`). It is `locked_fingerprint(job)` (`jobs.rs:254`): SHA-256 of `{goal, tier, context, touches, acceptance, out_of_scope, needs}` (`needs` sorted). It is `NULL` while the Job is a `draft` and `NOT NULL` once it is `queued`, `running`, `merged` or `failed` (CHECKs `status <> 'draft' OR fingerprint IS NULL` and `status IN ('draft','cancelled') OR fingerprint IS NOT NULL`). It is set at `plan lock` (`lock.rs:121-125`), at an approved review that queues drafts (`review.rs:374-385`), at append (`core/src/job/add.rs`), and recomputed on a Locked Job's `job edit --spec`. The engine and the starter recompute it and refuse a Job whose stored hash differs (`jobs::tampered`, `jobs.rs:264`; `core/src/driver/start.rs` check before the Attempt). It differs from the proposal's `content_hash` in three ways: it does not exist while a Job is a draft (the proposal wants it for draft lint), it excludes `kind`, `title`, `pr`, `tests`, `test_files`, `resources`, and it is a tamper seal, not a cache key (no table stores results per hash).

**"Plannotator fingerprint caching" is a misnomer.** `plannotator_reviews` is an append-only log of Plannotator UI sessions a Session has open: columns `session, pn_id, kind, subject, port, opened_at, closed_at, ended` (`docs/contract.md:1796`), written by `cpd review open|close` (`core/src/review/mod.rs:1`) and read by the inbox rule `review-waiting` (`core/src/inbox/rules.rs:354-359`). It has no fingerprint column and caches nothing. The nearest thing to caching is the idempotence of `plan review` (`already: true` for an unchanged pending review) and the fingerprint comparison of `plan lock`; the pi adapter reads `review.fingerprint` and `current` from `plan show` (`pi/test/plan-review.test.ts:10`) and I found no other adapter code that keeps a fingerprint cache (a grep of `pi/**/*.ts` outside `test/` found only unrelated renderer fingerprints in `pi/compact-tools/extensions/compact-tools-rendering.ts`).

**Is there a plan-revision number?** Only `plan_reviews.number`, per Plan, starting at 1 and incremented on each `plan review` that is not a no-op (`review.rs:197`; `UNIQUE (plan, number)`, `docs/contract.md:1540-1556`). It numbers review requests, not plan edits: ten edits between two reviews add one. It is not copied to `jobs` or `attempts`. Row-level history of Job edits exists in the append-only `history(txn, tbl, pk, op, before, after)` table (`docs/contract.md:1355`), written by triggers; whether its `before`/`after` payloads hold enough of a Job row to rebuild past revisions is unverified.

### 4. Worker tier ladder and Attempt telemetry

**Tier syntax and ladder.** A Tier is `<letter><effort>`: `H` Haiku, `S` Sonnet, `O` Opus (Leads only), `G` auxiliary (`core/src/config/tier.rs:16-23`); effort 0-5 maps to pi thinking levels `off, low, medium, high, xhigh, max` (`tier.rs:55`). A Worker may have exactly `H0-H3` and `S0-S3` (pro-d82; `tier.rs:67,121`). `Catalog::next` (`tier.rs:265`) steps the effort up on the same model, then leaves H for the lowest valid S effort at or above 1, ending at `S3`: **H0 > H1 > H2 > H3 > S1 > S2 > S3** (pro-d83, `docs/contract.md:4402-4428`; asserted in `core/src/config/tier_tests.rs:201-218`). `Catalog::step_up` (`tier.rs:289`) repairs a Tier the model catalog refuses.

**Where the starting Tier comes from.** `jobs.tier` is chosen by the Lead per Job (`job add --tier`, required unless `--draft`, `core/src/job/add.rs:142`), and Attempt 1 of the Job uses it (`core/src/driver/start.rs:291-293`, `job.require_tier()`). The `open`/`update` Jobs use `open_tier`: the code default is **`H3`** (`core/src/config/file.rs:289`, recommended by `docs/research/opener-tier.md`), whereas `docs/contract.md:4107,4161` still says `S1`: a stale doc. `retry_cap` defaults to 2 (`file.rs:273`).

**What climbs, and when.** `Cause::is_ladder` is true for exactly `crashed`, `no_submit` and `merge_conflict` (`core/src/attempt/ledger.rs:233`). For those, `next_rung` (`ledger.rs:551`) gives the next Tier while `automatic_attempts < 1 + retry_cap` (`ledger.rs:517`, `ledger.rs:572`), so by default a Job gets at most **3 automatic Attempts**, i.e. from H0 it reaches H2 and no further without the Lead. An Attempt whose driver never started is not counted and does not climb. `provider_error` is retried at the **same** Tier up to `PROVIDER_RETRIES = 3` times after a 30 s hold (`ledger.rs:239-242,553-557`; pro-d101/pro-d102, `docs/contract.md:979`). **Every other cause fails the Job and the Lead decides** (`job retry --tier`, `job cancel`): `gate_failed`, `gate_error`, `timed_out`, `setup_failed`, `tampered`, `cas_lost` do not promote (`docs/contract.md:979`: "`gate_failed` and `gate_error` are not ladder causes"). The proposal's rule "only reasoning failures promote" is therefore **not** what cpd does: cpd promotes on a dead Worker (`crashed`), an idle Worker (`no_submit`) and a rebase conflict, and does not promote on a failed Gate.

**`origin`.** `attempts.origin` is `ladder` or `retry` (`docs/contract.md:1620`). `retry` means the Attempt was created after `job retry --tier` (`core/src/driver/start.rs:289`); `ladder` means every other Attempt, including Attempt 1, a same-Tier requeue after a provider error or a never-started driver, and each automatic climb (`start.rs:290-305`). It is not the proposal's `start_reason`, but `start_reason` is derivable from existing columns: `initial` is `number = 1`, `promoted` is a Tier above the previous Attempt's, `retry_same_tier` is an equal Tier, and a manual retry is `origin = 'retry'`.

**Does a promoted Attempt inherit the failed diff? No, it starts clean, with text.**

- Parallel Attempt: a new branch and worktree are cut from the **current Lead Branch tip** (`core/src/git/worktree.rs:706`, `rev_parse(lead_worktree, refs/heads/<lead>)` then `git worktree add -b <attempt branch> <path> <base_sha>`; called at `core/src/driver/run.rs:412`). The previous Attempt's branch and worktree are kept "for debugging" when it did not merge (`run.rs:933-960`, doc comment of `cleanup`) but nothing reads them again.
- Linear Attempt: it works on the Lead Branch itself, and a failed or cancelled one is `git reset --hard <base_sha>` at A9 (`run.rs:943-956,1038-1056`), discarding what it left. `base_sha` is the Lead tip at A2 (`run.rs:403-427`).
- What carries over is text only: the next Spec lists "Previous Attempts" with outcome, cause and one explanation sentence, plus the conflicting files for `merge_conflict` and the Gate hook name and log path for `gate_failed`/`gate_error` (`core/src/spec/mod.rs:54-103`, `core/src/spec/worker.md:172-186`, `core/src/driver/worker.rs:45-70`).

So the proposal's `inherited = 'diff_and_summary'` does not exist in cpd, and `inherited = 'none'` would be ambiguous: cpd's behaviour is "clean tree plus a summary and a log pointer", which is neither of the proposal's two options exactly. No column records it, because it is one constant.

**`runner` and the neutral `EndReport`.** `attempts.runner` (`pi`|`durable`, `NOT NULL DEFAULT 'pi'`) and `worker_pid` were added by `core/src/store/migrations/20261102T1700-worker-runner.sql`. Both runners end with the same neutral report: `struct EndReport { stop_reason, error_message, turns, tokens_in, tokens_out, cache_read, cache_write, cost_usd, model }` (`core/src/driver/runner.rs:225-235`, contract section 6.12). `runner::metrics` (`runner.rs:269`) reads it for `durable` and parses the pi session file for `pi` (`core/src/metrics/mod.rs`).

**`metrics`.** `attempts.metrics` is a JSON object `{turns, tokens_in, tokens_out, cache_read, cache_write, cost_usd, duration_s, model, thinking_level, stop_reason}` (`core/src/metrics/mod.rs:29-56`), written by the same UPDATE that ends the Attempt (`ledger.rs`, `end_opened`, `SET ... metrics = ?7`). It is `NULL` when none could be read (`runner.rs:277-294`). The proposal's `input_tokens`, `output_tokens`, `cost_usd` and `wall_seconds` already exist here as JSON keys (`tokens_in`, `tokens_out`, `cost_usd`, `duration_s`); `started_at` and `ended_at` are columns.

**Failure causes.** The `attempts.cause` CHECK lists `merged, cancel_requested, setup_failed, timed_out, crashed, no_submit, merge_conflict, cas_lost, dirty_target, tampered, gate_failed, gate_error, provider_error` (`docs/contract.md:1620-1630`); the Rust enum is `Cause` (`ledger.rs:163`, macro `causes!`, tested for completeness in `ledger.rs` `every_cause_is_listed_and_reads_back`). Producers:

| Cause | Produced by |
| --- | --- |
| `setup_failed` | Driver A1-A2: worktree, `pre-start` or policy read failed (`core/src/driver/run.rs:385,400,408,416`) |
| `timed_out` | A5 after `stop_request = timed_out`, the Attempt's active-time limit (`run.rs:713`) |
| `crashed` | Worker or driver died, pi stopped badly (`run.rs:738-817`), E2 liveness (`engine/src/watch/liveness.rs:174,177`), merge recovery (`core/src/merge/recover.rs:101`), a failed push or `gh` call of an `open`/`update` Job (`merge/mod.rs:336,344`) |
| `no_submit` | Worker idle twice without submit (`run.rs:809`) |
| `provider_error` | pi stopped on a provider error (`run.rs:813`) |
| `merge_conflict`, `cas_lost`, `tampered`, `gate_failed`, `gate_error` | The merge, as listed in item 1 |
| `dirty_target` | Nothing (item 1) |

**Where `gate_failed` is produced, and what it covers.** Only in `core/src/merge/mod.rs:400-425` (`run_gate`), at merge step 6, when the hook exits non-zero. Two hooks can feed it: the repo's `pre-commit` (the quick check) at every Job merge when `gate.merge = local`, and `pre-merge` (the PR Gate) at an `open` or `update` Job's merge when `gate.pr = local` (`merge/mod.rs:260-269`). It is **not** produced at `attempt submit`: the move of both Gates to the merge is hs-d679 (`git show 84c274bf`, "Run both gates at the merge and drop test slots (#425)"; `core/src/attempt/submit.rs:3-4`). In this repo's policy `gate.merge = "local"` and `gate.pr = "none"` and the full `pnpm run verify` runs on GitHub as CI (the `[gate]` and `[hooks.pre-commit]` tables of the repo's policy file, which lives in the owner's policy directory and not in this repo; I read it for this audit), so a `gate_failed` here is a quick-check failure (the hooks `typecheck` and `cargo`); a failing CI check comes back as `pr-feedback`, not as an Attempt row. I infer, from that policy, that nothing in the `attempts` table ever sees a local full-suite failure of a `work` or `docs` Job (guess: not verified against the data).

**Is the failing command's output kept?** Yes, but not in `check_tail`. The 60-line tail (`core/src/git/mod.rs:32`, `TAIL_LINES`) and the log path go into the end `detail` (`gate_detail`, `merge/mod.rs:370-377`) which is stored in the `attempt-retry` or `job-failed` Event; the whole output is in `home.hook_log(plan, attempt, hook)`; `parse_gate_detail` reads hook and log path back out of the Event for the next Spec (`merge/mod.rs:381-395`, `core/src/driver/worker.rs:59-63`).

**Why `check_tail` is NULL for every `gate_failed` Attempt (134 of 134, given by the Lead; I did not measure it).** No non-test Rust code writes the column: a grep for `check_tail` outside migrations shows only reads (`core/src/attempt/row.rs:29,48,80`, `core/src/job/show.rs:119,135`) and a test fixture (`cli/tests/flow/job.rs:841`). The contract still documents it as "last 60 lines of the quick check at submit" (`docs/contract.md:489`, and section 7.3 at `:3831`), and that writer ran at `attempt submit`; hs-d679 moved the quick check to the merge, where the tail goes into the Event instead. So the NULL is not specific to open/update/docs roles: I infer from the missing writer that it holds for every role and cause, which the Lead's 0-of-134 is consistent with. The column is dead and the contract text is stale. The same stale text survives in the Worker Spec: `check_failed` and `check_error` are listed as submit refusals (`core/src/spec/worker.md:236-237`), and `ErrorCode::CheckFailed` is declared (`core/src/error.rs:218`) but has no producer (`rg 'ErrorCode::CheckFailed'` in non-test code is empty).

**Can a granular cause be derived today?**

| Proposed outcome | Derivable now? | Evidence and gap |
| --- | --- | --- |
| `fail_compile` | **Partly, not reliably** | A failing `build` of `[test-commands]` is a distinct submit refusal `tests_build_failed` (`core/src/attempt/kind.rs:150-160`), but a refusal is returned to the Worker and not written to `attempts` or an Event (I found no `append` in `kind.rs`, and `submit.rs`/`span.rs` append only `attempt_status` and the `checks` span events): new capture needed. At the merge, `HookOutcome::Failed { exit }` keeps only the first failing command's exit code (`core/src/git/hooks/mod.rs`, `HookOutcome`); in this repo both quick-check commands end in an `exit 64` fallback (a shell "or" clause), so the exit code cannot tell `typecheck` from `cargo build`. The command name is recoverable only from the log, where lines are prefixed `name │ ` when a block has more than one command (`core/src/git/hooks/exec.rs:108,125-128`): reading a log is not a deterministic field |
| `fail_test` | **No, for the merge; submit-only for test kinds** | `tests_failed` (impl) and `tests_not_red` (test) are submit refusals (`kind.rs:195-209`), not stored. The merge runs no test command when `gate.pr = none`. New capture needed |
| `fail_lint` | **No** | cpd has no lint concept: hooks are named commands, and this repo's quick check is `typecheck` plus `cargo build`, no linter (policy text). Needs a policy-level mapping from hook command name to class, plus the failing command's name |
| `fail_flake` | **No** | A Gate runs once (`run_gate`); nothing re-runs a failed check, so a flake is indistinguishable from a real failure. Detection needs new behaviour (a rerun), not capture |
| `fail_infra` | **Yes, mostly** | `gate_error` (killed, timeout, hook could not run, `merge/mod.rs:417-423`), `setup_failed`, `provider_error`, and `crashed` with `driver_pid` NULL are infra by construction |
| `timeout` | **Yes** | `timed_out` (`run.rs:713`); a Gate timeout is `gate_error`, so the two must be kept apart |
| `fail_reasoning` | **No single cause** | The closest are `no_submit` and a `gate_failed` of a work Job; `tampered` and `merge_conflict` are neither reasoning nor infra |

### 5. The proposal's claims about cpd, checked

| Claim (Proposal) | True of cpd today? | Evidence |
| --- | --- | --- |
| "Today every job starts at Haiku without reasoning and climbs to Sonnet on high" ("Model ladder", "Start tier from data") | **False as worded.** Haiku-without-reasoning is `H0` (`LEVELS[0]` is `off`, `tier.rs:55`) and the ladder does end in Sonnet (`S3`, the top Tier of the climb), but the starting Tier is whatever the Lead sets per Job (`jobs.tier`, `start.rs:291-293`), `open`/`update` start at `open_tier` (code default `H3`), the climb happens only for `crashed`/`no_submit`/`merge_conflict` and stops after `1 + retry_cap` (default 3) Attempts, so an unaided climb from H0 stops at H2. How the Lead chooses a Tier is not specified anywhere I found (a grep for guidance on picking a Tier in non-research docs and skills returned nothing; unverified beyond that grep) | `core/src/config/tier.rs`, `core/src/attempt/ledger.rs:233,551`, `core/src/config/file.rs:273,289` |
| "A workflow is one PR" ("Planning") | **True in the glossary's sense**: a Workflow ends in exactly one pull request (`CONTEXT.md:44`); the CLI, tables (`prs`, `pr_needs`) and Events still say "PR" | `CONTEXT.md:16,44` |
| "a job is one commit" | **False.** A Job is a unit of work that one merged Attempt delivers; the Attempt must have **at least one** commit ahead of its base (`rev-list --count base..HEAD`, refused only when it is 0 and the Job is not `open`/`update`, `core/src/attempt/submit.rs:73`), the Worker Spec tells it to commit a skeleton within 10 minutes and "at least every 20 minutes" after (`core/src/spec/worker.md:74`), and the merge fast-forwards all of its commits (`merge/mod.rs:290-300`). The one-commit property exists only on the base: the pull request is squash-merged (`docs/commands.md:2445`) | as cited |
| "jobs commit to the workflow branch through a queue, with a quick build/test after each merge" ("Merge gate") | **Half true.** No queue or lock exists: parallel Attempts race a ref CAS, retry up to 3 times, then fail `cas_lost`; a linear Attempt avoids the race by being the PR's only live Attempt. The quick check does run for every Job merge when `gate.merge = local`, on the rebased HEAD before the fast-forward, and again on each CAS retry | `merge/mod.rs:38,260-263,290-317`; `docs/contract.md:3835` |
| "the engine builds the DAG and computes the schedule" | **Mostly true, without footprints.** The Lead declares `needs`; E3 recomputes the Ready set every tick from `dependencies` and runs the pure `schedule()` over pools; there is no stored schedule and no write-set serialization | `engine/src/watch/schedule.rs:80,152`; `core/src/driver/schedule.rs` |
| "the lead decides the shape" | **True.** The Lead picks PR keys, shapes, Jobs, kinds, tiers, Specs and `needs`; `pr add --shape` only lays out stubs, and review and lock check them | `core/src/pr/add.rs:52`, `core/src/plan/shape.rs:39` |

**How parallelism is capped today.** By `concurrency` (default 3, machine-wide over all Plans, `core/src/config/file.rs:272`; one `cpd watch` engine per machine, `docs/contract.md:35`), by declared `resources` slots (item 2), by the per-PR linear-vs-parallel rule (`schedule.rs:102`), and by the `provider_error` hold of 30 s (`engine/src/watch/provider_hold.rs`). There is **no RAM or CPU budget and no model rate-limit cap**: a grep for `loadavg`, `available_parallelism`, `sysinfo`, `rate.limit` in `core/src`, `engine/src`, `cli/src` found only GitHub's rate limit in `core/src/pr/gh.rs` (`gh_rate_limited`). The shared build cache exists as a convention: `scripts/cargo.sh` starts an `sccache` server and exports `RUSTC_WRAPPER=sccache` when `sccache` is on `PATH`, and takes one slot of the policy resource `cargo` through `cpd resource run` (`scripts/cargo.sh:1-60`). `docs/research/build-cap.md` records a measurement of `build.jobs`; its numbers are the Data Job's to use, not this Job's.

**Hot files.** cpd has no notion of a hot-file list: a grep for `hot`, lockfile names and `Cargo.lock` in non-test code finds only the version-sync tool (`core/src/version_sync/mod.rs`), the test selector's root-input list (`core/src/testsel/mod.rs:227`, which decides which tests to run, not which Jobs to serialize) and install code. The migration-numbering hot spot was removed at its source instead: migrations are named by UTC minute (`scripts/new-migration.sh:4`; contract section 4.5 N1), so two Jobs adding migrations do not collide on a counter.

### 6. What the proposal's Job log schema would add that cpd lacks

Verdicts use: **exists as X**, **derivable from Y**, **absent**, **duplicates Z**.

| Proposed column / table | Verdict | Mapping and evidence |
| --- | --- | --- |
| `jobs` (new table) | **duplicates `jobs`** | cpd's `jobs` is the Job row; adding a second `jobs` would clash by name and role |
| `job_attempts` (new table) | **duplicates `attempts`** | `attempts` is already one row per Attempt (`UNIQUE (plan, job, number)`, `docs/contract.md:1615-1660`) |
| `id`, `job_id`, `attempt_no` | **exists as** `jobs.seq`/`(plan, key)`, `attempts.job`, `attempts.number` | |
| `workflow_id` | **exists as** `jobs.plan` + `jobs.pr` (the Workflow is the PR row `(plan, key)`) | `docs/contract.md:1557` |
| `plan_rev` | **absent** | No plan revision number. Nearest: `plan_reviews.number` (review requests, per Plan; item 3); `history` could order edits by `txn` (unverified payload) |
| `content_hash` | **derivable from** `jobs.fingerprint` (partly) | Exists only for Locked Jobs and excludes `kind`, `title`, `pr`, `tests`, `test_files`, `resources`; absent for drafts, which is where the proposal wants it |
| `kind` | **exists as** `jobs.kind` | cpd's values are work/docs/test/impl/verify/open/update/scout/research; the proposal's examples (`refactor`, `add-test`, `ui`, `config`) are a different taxonomy: `config` is a PR shape, the rest do not exist |
| `language` | **absent** | Derivable only from file extensions of `touches` or the diff (guess: not computed anywhere) |
| `declared_write` | **derivable from** `jobs.touches` | One list: cpd does not split read and write |
| `declared_read` | **absent** | |
| `instr_tokens` | **absent** | No token estimate of a Spec. The rendered Spec is a file (`attempts.spec_path`), so it is computable on demand |
| `depends_on` | **exists as** `dependencies` (and `pr_needs` for PRs) | `docs/contract.md:1604` |
| `tier` | **exists as** `attempts.tier` | Stored as `H0`..`S3`, not `haiku/none` |
| `start_reason` | **derivable from** `attempts.number`, `tier`, `origin` | item 4 |
| `inherited` | **absent, constant** | cpd never inherits a diff (item 4) |
| `started_at`, `ended_at` | **exists as** `attempts.started_at`, `ended_at` | |
| `outcome`: `pass` | **exists as** `attempts.outcome = 'merged'` | |
| `outcome`: `fail_reasoning`, `fail_flake`, `fail_lint`, `fail_compile`, `fail_test` | **absent** | item 4 table |
| `outcome`: `fail_infra`, `timeout` | **derivable from** `attempts.cause` | `setup_failed`, `provider_error`, `gate_error`, `timed_out` |
| `outcome`: `cancelled` | **exists as** `attempts.outcome = 'cancelled'` | The proposal has no such value |
| `actual_files` | **absent** (cheap to compute) | `git::tamper::changed_paths` (`core/src/git/tamper.rs:87`) is the primitive; it is called at the merge for the protected and guarded checks but its result is not stored |
| `footprint_match` | **absent** | Needs `actual_files` and `declared_write` |
| `loc_changed` | **absent** | |
| `input_tokens`, `output_tokens`, `cost_usd`, `wall_seconds` | **exists as** keys of `attempts.metrics` (`tokens_in`, `tokens_out`, `cost_usd`, `duration_s`) | `core/src/metrics/mod.rs:29-56`; `NULL` when no session was readable |
| `lint_results` | **absent** | No lint exists (item 2) |

## Findings on the existing code

Facts only, each with its evidence above; design belongs to the later Jobs.

1. **No consumer of `jobs.touches` compares it with a diff.** `touches` is checked against the start tree at `job add`/`job edit` (`touches_missing`), hashed into both fingerprints, rendered to the Worker and to reviewers, and compared across Plans in `overlap.rs`; nothing enforces it at submit or merge, and the Worker contract explicitly allows leaving it (`docs/commands.md:4495`).
2. **Same-PR and same-Plan `touches` overlap is checked nowhere.** `overlap.rs` excludes the Plan's own Jobs (`WHERE p.plan <> ?1`), is a warning, runs at `plan render` and `plan review`, and not at `plan lock`.
3. **cpd already has the primitives P1 would need, but not their wiring.** `changed_paths` yields the actual file set; `pr_has_live_linear` and `allocations` already serialize a PR's Attempts by a count rule; `resources` with `slots` already serializes Jobs on a named pool. No scheduler input is derived from file sets.
4. **No lint pass exists.** `plan review` and `plan lock` run the structural checks of item 2 (draft completeness, outline, shape, cycle among drafts, approved fingerprint) and nothing that reads Job text, footprints or the brief.
5. **Two fingerprints exist, neither is a lint cache.** The Plan fingerprint pins what the User approved (drafts, PRs while in plan phase, outline); `jobs.fingerprint` seals a Locked Job. No table stores a result per hash. "Plannotator fingerprint caching" names nothing that exists.
6. **The ladder promotes on causes the proposal calls non-reasoning and holds on one it would promote.** `crashed`, `no_submit` and `merge_conflict` climb; `gate_failed`, `timed_out`, `gate_error` fail the Job to the Lead. Automatic climbing is capped at `1 + retry_cap = 3` Attempts, and a promoted Attempt starts clean with a summary only.
7. **Granular failure causes are not captured.** The one stored datum on a failed Gate is `cause = gate_failed` plus an Event `detail` with the hook name, exit and 60-line tail; test and build refusals at submit are not stored; the failing command's name is not a field; no rerun exists to separate flakes.
8. **`attempts.check_tail` is dead and the docs around it are stale.** No code writes it since hs-d679, the contract still describes it, and the Worker Spec still lists `check_failed`/`check_error` submit refusals that nothing produces. `Cause::DirtyTarget` is also declared and never produced, and the contract's `open_tier` default (`S1`) disagrees with the code (`H3`).
9. **The telemetry the proposal wants mostly exists under other names.** Tokens, cost and wall time are in `attempts.metrics`; Tier, origin, cause, timestamps and runner are columns; `start_reason` is derivable. What is absent is `actual_files`, `footprint_match`, `loc_changed`, a plan revision, a content hash for drafts, and a failure class finer than `cause`.
10. **The cheapest P3 data source in the repo is the `attempts` table and the `attempt-retry`/`job-failed` Events, not a new `job_attempts` table**: they already hold one row per Attempt with Tier, cause and metrics (item 6).

## Data analysis

## Evaluation of the three sub-proposals

## Roadmap
