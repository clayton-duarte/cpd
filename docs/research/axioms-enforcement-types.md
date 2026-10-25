# Axioms audit 2: types and boundary axioms

## Method

Read in this worktree: the axiom texts (this Job's Spec and the `AGENTS.md` project instructions pi loaded), the core's types and boundaries (`core/src/ids.rs`, `core/src/error.rs`, `core/src/context/record.rs`, `core/src/plan/jobs.rs`, `core/src/attempt/{kind,submit,ledger}.rs`, `core/src/ask/*`, `core/src/policy/*`, `core/src/config/*`, `core/src/store/{mod,schema}.rs`), the 45 store migrations, the pi adapter guards (`pi/lib/{askguard,mainguard,testguard,noblock,loop-guard}.ts` and the one handler in `pi/lib/adapter.ts`), the Worker launch argv (`core/src/host/pi/launch.rs`), the test that pins the submit checks (`cli/tests/flow/attempt.rs`), the Gate (`package.json`, `scripts/gate-run.sh`, `.config/nextest.toml`), and the repo policy (`$CPD_HOME/policy/repos/clayton-duarte/cpd.toml`).

Ran, each read-only and under `timeout`: `rg`, `grep`, `sed`, `ls | wc -l`, and `cpd events list --kind <k> --limit 5000` (the largest `--limit` the command accepts; every kind counted is below it, so each count is complete).

Delivery of the axioms (A9): my own system prompt carries the `AGENTS.md` project instructions. `/home/clayton/.pi/agent/AGENTS.md` has 19 axioms (`grep -c '^## '` = 19), which includes every axiom of this part, 8-13. The Worker argv (`core/src/host/pi/launch.rs:166`) passes no `--no-context-files` (the distiller at `:195` does), so pi loads the agent directory's `AGENTS.md` into the Worker prompt. (The 29-axiom file is the `agents` worktree `/home/clayton/.pi/agent.axioms.agents/AGENTS.md`, not the one the prompt loads.)

### 8. Make illegal states unrepresentable
- **Enforced by:**
  - `type` `core/src/ids.rs:89` (`simple_id!`) and `core/src/ids.rs:97` (`parse`): each id is a newtype whose only constructor validates contract §2.1; `Deserialize` at `core/src/ids.rs:82` runs `parse`, so a malformed id cannot enter from JSON or the CLI. Blind spot: it covers ids only, not other stored strings.
  - `type` `core/src/context/record.rs:36` (`enum Draft`): each record kind carries exactly the links it may have (`:33-34`), so a decision cannot name a question. Blind spot: the record text is free.
  - `type` `core/src/error.rs:41` (`enum ErrorCode`) with the macro at `core/src/error.rs:36`: each code maps to exactly one `Exit`, so a code cannot be printed with the wrong exit (`core/src/error.rs:4`). Blind spot: the message text is free.
  - `type` `core/src/ids.rs:181` (`AttemptId` over `NonZeroU32`): an Attempt number of zero is unrepresentable.
  - `check` `core/src/store/migrations/*.sql`: 349 `CHECK` clauses across 45 files constrain statuses, shapes and nullability; `core/src/store/migrations/0001-initial.sql:302` (`jobs_status`) allows only the declared status transitions.
- **Prose only:** none (the invariant lives in the types and the CHECK constraints; `docs/contract.md` §2.1 restates the grammar the code enforces).
- **Gap:** `jobs.kind` and `attempts.status` are plain `String` inside the core (`core/src/plan/jobs.rs:47`, `core/src/attempt/ledger.rs:30`), so a kind a later migration adds to the CHECK but no `matches!` is silently treated as "neither": `core/src/attempt/submit.rs:62` (`open|update`) and `core/src/attempt/kind.rs:21` (`test|impl`). No evidence of a wrong value reaching such a match (the CHECK guards writes).
- **Proposal:** parse `jobs.kind` and `attempts.status` into enums at load (`core/src/plan/jobs.rs`, `core/src/attempt/ledger.rs`), as the ids are parsed, so the matches are exhaustive and an unknown stored value is `bad_row`; pin it with a unit test that loads an unknown kind. Size S.
- **Verified:** `rg -c '^simple_id!' core/src/ids.rs` -> 7; `grep -h CHECK core/src/store/migrations/*.sql | grep -c CHECK` -> 349; `ls core/src/store/migrations/*.sql | wc -l` -> 45; `rg -n 'pub kind: String|pub status: String' core/src/plan/jobs.rs core/src/attempt/ledger.rs` -> 4; `sed -n '36p' core/src/context/record.rs` -> `pub enum Draft {`; `rg -c 'draft>queued' core/src/store/migrations/*.sql | awk -F: '{s+=$2} END{print s}'` -> 3.

### 9. Validate at the boundary, fail loud
- **Enforced by:**
  - `check` `core/src/ids.rs:82`: serde `Deserialize` for every id calls `parse`; bad input is `invalid_*_id` (exit 2), never a default.
  - `type` `core/src/error.rs:41`: every failure is a coded `CpdError`; the process exit is fixed by the code (`core/src/error.rs:58`), so no failure can be swallowed into exit 0.
  - `check` `core/src/config/file.rs:1-10`: `config.toml` is parsed key by key and every unknown key is refused with its dotted path (contract §8.1); a bad leaf (`PortRange::parse` at `core/src/config/file.rs:81`) is an error, not a fallback.
  - `check` `core/src/policy/load.rs:29` (`parse_scope`): a scope file is parsed once into `Scope`; a bad key is `policy_invalid` with the key (`core/src/policy/load.rs:22`); `/test-commands` is validated (`core/src/policy/domain.rs:161`).
  - `check` `serde(deny_unknown_fields)` on 34 structs (`core/src/policy/domain.rs`, `core/src/context/record.rs`, `core/src/config/repo.rs`, `tui/`): an unknown field is refused.
  - `check` `core/src/error.rs:71-73`: a store too new, a changed migration or a wrong migration order refuses (`store_too_new`, `migration_changed`, `migration_order`, contract §4.5).
- **Prose only:** none.
- **Gap:** values read back from the Ledger are not re-validated: `core/src/plan/jobs.rs:168` reads `kind` as a bare `String` and `core/src/attempt/ledger.rs:40` reads `kind`/`status` the same, so "inside, the types are already true" does not hold for a hand-edited or older database; the CHECK constraints guard writes, not reads. No evidence of a bad row today.
- **Proposal:** the same parse-at-load as entry 8 (one enum per `kind`/`status`, `bad_row` on an unknown value); one change closes both gaps. Size S. A test in `core/src/plan/jobs.rs` (or `cli/tests/store`) pins that an unknown `kind` is refused.
- **Verified:** `rg -c 'deny_unknown_fields' -g '*.rs' --glob '!node_modules' | awk -F: '{s+=$2} END{print s}'` -> 34; `rg -c '= "[a-z_]+" => (Failure|Usage|Refused|NotFound|Timeout),' core/src/error.rs` -> 190; `rg -n 'pub fn parse_scope' core/src/policy/load.rs` -> 1 line; `sed -n '168p' core/src/plan/jobs.rs` -> the tuple read.

### 10. Tests are the proof
- **Enforced by:**
  - `check` `core/src/attempt/kind.rs:200` (`("test", HookOutcome::Passed) => TestsNotRed`): a `test` Job's named tests must FAIL at submit. Blind spot: only the exit status is read, so a compile error or a wrong panic counts as red.
  - `check` `core/src/attempt/kind.rs:206` (`("impl", HookOutcome::Failed { .. }) => TestsFailed`): an `impl` Job's named tests must pass.
  - `check` `core/src/attempt/kind.rs:34` (`guard_tests`) with `core/src/git/tamper.rs:95` (`touched_guarded`): an `impl` Job's diff may not change the `test_files` of the `test` Jobs it needs unless the Lead approved the path (`tests_tampered`).
  - `check` `core/src/attempt/kind.rs:83` (`TestFilesMissing`): a `test` Job must change a path its `test_files` names.
  - `test` `cli/tests/flow/attempt.rs:559` (not-red), `:598` (files-missing), `:694` (failed), `:715` (tampered): the contract of the checks is pinned.
  - `gate` `package.json:14` (`scripts/gate-run.sh npm run verify:steps`) with `.config/nextest.toml:19` (`fail-fast = false`): the full suite runs once per PR and lists every failure.
- **Prose only:** `core/src/spec/worker.md:36` ("A test that is wrong is a question, not an edit"); the Spec asks the red test to fail for the right reason, which no check can see.
- **Gap:** "for the right reason" is unenforced: a compile error or a bare `panic!()` is accepted as a red test because the check only compares the exit (`core/src/attempt/kind.rs:200-206`), and `[test-commands]` `run` has no way to assert the failure's shape. No evidence of a wrong-reason red (no Event, no metric).
- **Proposal:** none needed now (no evidence). If a wrong-reason red is ever seen, S: the test Job's Spec names an expected failure marker and `[test-commands] run` must surface it; do not build it speculatively.
- **Verified:** `rg -c 'tests_not_red|tests_failed|tests_tampered|test_files_missing' cli/tests/flow/attempt.rs` -> 9; `rg -n 'TestsNotRed|TestsFailed|TestsTampered' core/src/error.rs` -> 3 codes; `sed -n '19p' .config/nextest.toml` -> `fail-fast = false`; `sed -n '200p' core/src/attempt/kind.rs` -> the not-red arm.

### 11. Ask, don't assume
- **Enforced by:**
  - `script` `core/src/ask/question.rs:39` (`ask`): a Worker's `cpd ask` opens an Escalation and blocks in 500 ms polls (`core/src/ask/question.rs:23`) until an answer or the timeout (`ask_timeout`, exit 5); only a `running` Attempt may ask (`core/src/ask/question.rs:84`).
  - `check` `core/src/ask/question.rs:124` (`wait`): the Worker Session goes `waiting` under the label `cpd: question to the Lead`; a `stopped` Session is `illegal_transition`.
  - `check` `core/src/ask/question.rs:68` (`open`): the Escalation row and its `escalation` Event are written in the transaction that starts the wait.
  - `check` `core/src/ask/reply.rs:14` (`answer`): only `open -> answered`; a second answer is `escalation_answered`, an expired one is `escalation_expired`.
  - `type` `core/src/host/pi/launch.rs:14` (`WORKER_EXCLUDED_TOOLS`, applied at `core/src/host/pi/launch.rs:166`): the Worker pi is launched with `--exclude-tools ask_user_question,...`, so its only question channel is `cpd ask`.
- **Prose only:** `core/src/spec/worker.md:54` ("If continuing would require guessing, ask the Lead and wait"); `pi/skills/cpd-lead/SKILL.md:39` ("Resolve first, ask second").
- **Gap:** nothing forces a Worker to ask: `attempt submit` checks the tests and the diff, not that a question was asked, so a Worker can guess and submit. Measured: 358 `escalation` Events, 347 `escalation-answered`, 11 `escalation-expired`; the 11 expiries are Workers that followed the rule and got no answer in `ask_minutes`. No Event counts a Worker that guessed.
- **Proposal:** none needed for the channel (it exists and blocks until answered). The 11 expiries are the one signal; the engine already emits `escalation-expired` and `core/src/inbox/rules.rs` is the surface that should make them seen, a follow-up for the Manager, not a new guard.
- **Verified:** `timeout 60 cpd events list --kind escalation --limit 5000` -> 358; `--kind escalation-answered` -> 347; `--kind escalation-expired` -> 11; `rg -c 'WORKER_EXCLUDED_TOOLS' core/src/host/pi/launch.rs` -> 2.

### 12. A question is not an instruction
- **Enforced by:**
  - `check` `core/src/plan/review.rs:379-384`: a Plan's draft Jobs become `queued` (work) only when `cpd_plan_review` is approved; presenting the plan starts nothing. The transition is pinned by the `jobs_status` trigger (`core/src/store/migrations/0001-initial.sql:302`, only `draft>queued`) and by `is_draft`/`is_under_review` (`core/src/plan/jobs.rs:68`, `core/src/plan/jobs.rs:73`). Blind spot: it gates Plan drafts only.
  - `check` `core/src/store/migrations/20261018T0828-kind-tables.sql:90`: a `question` record carries no links and only an `answer` names its `question`, so a question cannot be stored as a decision.
  - `check` `core/src/context/mod.rs:285` (`context ask`): records a question that asks nobody; `cpd context answer` is a separate act.
  - `check` `pi/lib/adapter.ts:64` with `pi/lib/askguard.ts:4` (1500-character limit) and `pi/lib/askguard.ts:86` (a cited label must appear in the visible chat): an `ask_user_question` that is not a short, grounded question is refused. Blind spot: it shapes the ask, it does not stop work started elsewhere.
- **Prose only:** `pi/skills/cpd-lead/SKILL.md:174` ("While a review is pending, don't ask the User in chat..."); `pi/skills/cpd-lead/SKILL.md:29` ("Never use `cpd context ask`: it records an open question ... and asks nobody").
- **Gap:** nothing separates a User's interrogative chat message from an instruction to an agent: a Lead or Worker can act on "should we X?" and no check refuses it. No evidence of the misbehaviour (no Event, no record); the draft gate covers the Plan, not chat.
- **Proposal:** none needed now: no evidence, and a guard on how an agent reads chat is not a guard on a raw tool (A2/hs-d248 do not force one). If evidence appears, the smallest useful step is S: make the Lead skill's "resolve first, ask second" a check only where cpd already sees the ask (the Plan review).
- **Verified:** `timeout 60 cpd events list --kind plan-review --limit 5000` -> 349 (178 pending, 168 approved, 3 rejected); `rg -n 'a decision cannot name a question' core/src/context/record.rs` -> 1; `sed -n '90p' core/src/store/migrations/20261018T0828-kind-tables.sql` -> the question CHECK; `rg -c 'draft>queued' core/src/store/migrations/*.sql | awk -F: '{s+=$2} END{print s}'` -> 3.

### 13. Reversibility
- **Enforced by:**
  - `check` `core/src/driver/run.rs:342` with `core/src/git/worktree.rs:578` (`create_attempt_worktree_with`): every Attempt runs in a worktree cut from the Lead Branch (`cpd/<plan>--<attempt>`), so a Job's work is discarded by removing the worktree, and the merge rebases/fast-forwards.
  - `check` `scripts/check-migrations-frozen.sh:1` (diff filter at `scripts/check-migrations-frozen.sh:20`, `MDR`): an applied migration is never edited, renamed or deleted. The Gate runs it first (`package.json:15`) and `scripts/install-cpd.sh:42-62` refuses an install whose migrations are not on `origin/main`, so the schema one-way door is fenced.
  - `check` `core/src/store/schema.rs:551` (generated `_no_delete` triggers) with `core/src/store/schema/tests.rs:1581` (`events_and_context_records_stay_append_only`): `txns`, `history` and `events` are append-only and `context_records` refuses UPDATE and DELETE, so a decision is superseded, never rewritten (contract §4.4).
  - `check` `core/src/git/tamper.rs:120` (`check_over`, called at `core/src/attempt/submit.rs:163`): a protected path in an Attempt's diff is `gate_tampered`, so a merge cannot silently rewrite a guarded file.
  - `check` `core/src/plan/review.rs:379`: an approval queues only the drafts it reviewed (fingerprint `current`), so a post-review edit is not silently applied.
- **Prose only:** the `AGENTS.md` axiom 13; `docs/contract.md` §4.5 "N5" states the migration rule the script enforces.
- **Gap:** nothing records the reversibility cost of a decision or names a one-way door outside the migrations (the design docs do it by hand, for example `docs/research/migrations.md:272`). No evidence of a loss from it.
- **Proposal:** none needed (YAGNI): the one class of one-way door cpd owns, the store schema, is already fenced by the frozen check and the install refusal.
- **Verified:** `sed -n '1p' scripts/check-migrations-frozen.sh` -> the usage comment; `rg -c 'MDR' scripts/check-migrations-frozen.sh` -> 1; `ls core/src/store/migrations/*.sql | wc -l` -> 45; `sed -n '1581p' core/src/store/schema/tests.rs` -> the append-only test; `rg -n 'CREATE TRIGGER \{table\}_no_delete' core/src/store/schema.rs` -> 1 (the DELETE trigger is generated, not written in a migration).

## Not verified

- The instruction "the Worker's prompt gets `AGENTS.md` by its argv" (A9): I read the Worker argv (`core/src/host/pi/launch.rs:166`, no `--no-context-files`) and the live file (`/home/clayton/.pi/agent/AGENTS.md`, 19 axioms including 8-13); I did not dump my own prompt to a file, and I infer from `PI_SESSION_FILE` (`.pi/agent/sessions/...`) that the loaded file is the 19-axiom one rather than the 29-axiom `agents` worktree.
- The store counts are the live machine's `$CPD_HOME` database at this time; they are point-in-time, not a rate.
- Whether any production Worker has actually guessed instead of asking: no command measures it; the "no evidence" labels stand until one does.
- The emit of a wrong-reason red test: `rg` found no marker mechanism; I did not run the Gate or the full suite (out of scope for a Worker).
