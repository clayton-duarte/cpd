# Axioms audit 2: cpd-specific axioms 22 to 27

## Method

I read, in this worktree: `docs/commands.md` §5.1, §5.2, §15.4, §15.8 and §15.11 to §15.17; `docs/contract.md` §2.2 ("Context record", recency wins), §4.6 ("Records in force", "Retirement", "The distiller curates"), §6.5 ("`PATH` and the `git` wrapper"); the adapters `pi/lib/adapter.ts`, `state.ts`, `agent-state.ts`, `loop-guard.ts`, `mainguard.ts`, `noblock.ts`, `testguard.ts`, `secretguard.ts`, `askguard.ts`, `pnguard.ts`, `reminder.ts`, `turns.ts`; `core/src/host/git_guard.rs`, `core/src/session/{row,state}.rs`, `core/src/inbox/rules.rs`, `core/src/context/{record,contexts,inforce,distill}.rs`, `core/src/job/{add,spec}.rs`; `pi/skills/cpd-lead/SKILL.md`, `pi/skills/cpd-manager/SKILL.md`, `pi/skills/skill.test.ts`; the tests `pi/test/{mainguard,loop-guard,state,reminder}.test.ts`, `core/src/context/inforce.rs`, `core/src/inbox/rules.rs`, `cli/tests/flow/context.rs`, `cli/tests/flow/context_distill.rs`. I read Plan `main-mirror`'s live state with `cpd plan show --plan main-mirror`. My own system prompt carries the text of `/home/clayton/.pi/agent/AGENTS.md`: the file today has **19** axioms (`grep -c '^## ' /home/clayton/.pi/agent/AGENTS.md` → 19), the prompt holds those 19 titles, and **none of axioms 22 to 27 is among them** (they are within the 10 that PR `agents` of this Plan adds). The Worker argv built in `core/src/host/pi/launch.rs` has no `--no-context-files` (the flag occurs only in `distiller`, line 195, the one-shot pi that is no Session); in the `lead`/`worker` builders (`core/src/host/pi/launch.rs:115` and `:163`) it is absent (`sed -n '115,192p' core/src/host/pi/launch.rs | rg -o 'no-context-files' | wc -l` → 0), so the prompt does get the context files. This confirms A9 for the 19 the file has, and it means a Worker is not yet told axioms 22 to 27. Every measurement was read-only and wrapped in `timeout`.

### 22. Guards only for proven bad behaviour

- **Enforced by:**
  - `guard` `pi/lib/mainguard.ts:65` (`primaryCheckoutMove`, applied in the one handler at `pi/lib/adapter.ts:59`): refuses `checkout switch reset rebase merge pull stash` run in a primary checkout, for every role; evidence hs-n436 and hs-d248 at `docs/commands.md:3744`. Blind spot: only those seven git subcommands, only through the `bash` tool (Plan `main-mirror` PR `guard` extends it to `edit`, `write` and bash writes), and not judged through `sh -c`, `xargs` or `GIT_DIR`.
  - `guard` `pi/lib/secretguard.ts:5` (`secretBypass`, applied at `pi/lib/adapter.ts:61`): refuses `op` and `cpd secret approve`; its docs section `docs/commands.md:3772` cites **0** record ids. Blind spot: only the command word of a simple command; `sh -c`, an alias or a script passes.
  - `guard` `pi/lib/noblock.ts:70` (`blockingReason`, applied at `pi/lib/adapter.ts:75`): refuses a Manager's `bash` wait over 30 s; `docs/commands.md:3624` cites **0** record ids. Blind spot: literal durations only (`sleep $n` passes), Manager role only.
  - `guard` `pi/lib/testguard.ts:77` (`unfilteredTestRun`, applied at `pi/lib/adapter.ts:78`): refuses a Worker's unfiltered `cargo test`, `cargo nextest run`, `npm test` and `npm run verify`; `docs/commands.md:3656` cites **0** record ids. Blind spot: a filter heuristic; a run the heuristic does not read passes.
  - `guard` `core/src/host/git_guard.rs:30` (the `WRAPPER`, put first on the Session's `PATH` by `core/src/host/git_guard.rs:88`): refuses `git stash` in every Session; evidence hs-d310 at `core/src/host/git_guard.rs:19` and `docs/contract.md:2879`. Blind spot: `stash` only; `/usr/bin/git` by absolute path bypasses it.
  - `guard` `pi/lib/loop-guard.ts:47`, `pi/lib/askguard.ts:29` and `:86` (applied at `pi/lib/adapter.ts:66`), `pi/lib/pnguard.ts:23`: guards on adapter deliveries, on the `ask_user_question` tool and on the `plannotator` tool, not on a raw tool; evidence hs-d552/hs-d571 (`docs/commands.md:3730`) and hs-d502/hs-d548/hs-d553/hs-n425 (`docs/commands.md:3682`). Blind spot: they bound tools cpd adds, not git, gh, edit or the shell.
  - `none`: no mechanism requires a guard to be justified by evidence; the rule is not itself enforced.
- **Prose only:** `docs/commands.md:3746` (the adapter guard of §15.16 applies "hs-d248, a guard only for behaviour seen in practice") and `docs/research/repo-policy.md:288` (typed keys land "only with evidence (hs-d248)").
- **Gap:** no guard code names the record or Event that justified it (`rg -o 'hs-d[0-9]+' pi/lib | wc -l` → 0), and three raw-tool guards (on the shell) cite no record id in their docs section: `noblock` (0), `testguard` (0), `secretguard` (0), where `mainguard` cites 4 and the loop guard 2. A new guard on `git`, `gh`, `edit` or the shell can therefore be added today with prose and no evidence, and three were.
- **Proposal:** `check`: a test `pi/test/guards.test.ts` that reads every guard module's header comment and its docs section and fails when a shell/git/edit guard names no record id or Event code; before it can pass, `noblock` (§15.11), `testguard` (§15.12) and `secretguard` (§15.17) must record the evidence that justified them, or be removed (`no evidence: do not guard`). Size S.
- **Verified:**
  - `timeout 60 bash -c "rg -o 'hs-d[0-9]+' pi/lib | wc -l"` → 0; `ls pi/lib | grep -E 'guard|noblock' | wc -l` → 7.
  - `timeout 60 bash -c 'for r in "3624 3655 15.11" "3656 3681 15.12" "3682 3719 15.13" "3730 3743 15.15" "3744 3771 15.16" "3772 3823 15.17"; do set -- $r; sed -n "$1,$2p" docs/commands.md | rg -o "hs-[dn][0-9]+" | sort -u | wc -l; done'` → 0, 0, 8, 2, 4, 0.

### 23. Blocked means it needs the human

- **Enforced by:**
  - `type` `pi/lib/state.ts:7` (`Kind = "human" | "agent"`) with `pi/lib/state.ts:59` (`wanted()`): `blocked` is reported only for the latest open `human` reason, `waiting` for the latest open `agent` one; an agent reason can never yield `blocked`.
  - `test` `pi/test/state.test.ts` (16 tests): "a Worker question alone reports waiting with its label"; "a Worker question opened during a dialog does not hide it … still blocked `dialog`"; "the dialog closed with a Worker question still open reports waiting".
  - `check` `core/src/session/state.rs:54` (`legal`): the transition table; `core/src/session/state.rs:19` (`Label`): a `waiting`/`blocked` label is 1–80 characters, no controls.
  - `test` `core/src/inbox/rules.rs:703` asserts the Inbox treats any `blocked` as a dialog "whatever its label".
  - `none`: nothing in the core enforces *which* kind of work may be `blocked`; the distinction lives only in the adapter.
- **Prose only:** `docs/commands.md:3359` (§15.4), the comments `core/src/session/row.rs:38` ("`blocked` needs the User, `waiting` needs another agent") and `core/src/inbox/rules.rs:25` ("a Worker's question goes to its Lead, and the Lead that waits for the Worker is `waiting`, not `blocked`").
- **Gap:** `session state` accepts `blocked` with any label, including work another agent clears. Real Events: across all `session-state` Events (15 026), 905 transitions were to `blocked` and 200 of them carried the agent-work label `cpd: Worker question` (203 more `cpd: question to the Lead`), against 298 to `waiting` (`cpd: Worker question` 148). Those 403 `blocked` Events with agent labels are exactly the mislabels hs-d303 fixed in the adapter; the core still permits them, so the invariant can regress without any check failing.
- **Proposal:** `type`: give `cpd session state` an explicit `--kind human|agent`, store it, and refuse `blocked` with `agent` (and `waiting` with `human`) in `core/src/session/state.rs`, so the invariant is a refused state and not adapter discipline; the adapter already holds the kind (`pi/lib/state.ts:7`) and would pass it. Pinned by a `cli/tests/flow/session.rs` test and a `core/src/inbox/rules.rs` test; Size M.
- **Verified:**
  - `timeout 120 cpd events list --kind session-state --limit 5000`, then paged with `--since <next>` (5 pages): 15 026 Events; 905 to `blocked` (`dialog` 317, `cpd: question to the Lead` 203, `cpd: Worker question` 200, `cpd: plan review` 185); 298 to `waiting` (`cpd: question to the Lead` 150, `cpd: Worker question` 148).
  - `timeout 60 bash -c "rg -c '^test\(' pi/test/state.test.ts; rg -c '#\[test\]' core/src/inbox/rules.rs"` → 16; 58.

### 24. No planned work only in chat

- **Enforced by:**
  - `check` `pi/lib/reminder.ts:61` (`needsReminder`) with `pi/lib/turns.ts:71` and `:83`: at a Lead's run end, if its last assistant text matches the fixed phrase list and names no Job or PR key of its Plan, the next run carries one hidden (`display: false`) `cpd-reminder` message; 12 tests in `pi/test/reminder.test.ts`.
  - `check` `pi/lib/reminder.ts:11` (`PHRASES`): the whole definition is a fixed list of 19 phrases plus `after … merges`; extending it is a change to `docs/commands.md:3540`.
  - `test` `pi/skills/skill.test.ts:502` pins the skill prose ("Register planned work at once", "Check each Plan's registered work").
  - `check` `docs/commands.md:1263` and `docs/commands.md:1270`: a stub draft needs only `--title` (`cpd job add <k> --draft --title "…"`), so registration is one command at any time.
  - `none`: nothing blocks a Lead that plans in chat without registering.
- **Prose only:** `pi/skills/cpd-lead/SKILL.md:41` ("No planned work lives only in your chat"), `pi/skills/cpd-manager/SKILL.md` (the Manager checks registered work), `docs/commands.md:3542`.
- **Gap:** the reminder is a heuristic and invisible: it fires only for role `lead`, only after a `cpd plan show` that succeeds, only on the 19 fixed substrings, and a firing is not recorded (`rg -l 'reminder' ~/.cpd/run/adapter/*.log | wc -l` → 0 of 1327 logs; only a failed check is logged), so there is no measurement of how often planned work stays in chat, and any synonym outside the list escapes.
- **Proposal:** `check`: append one line to the Lead's adapter log when the reminder is due (for example `reminder: due`) and assert it in `pi/test/reminder.test.ts`, so the mechanism's rate is measurable; keep the phrase list. Size S.
- **Verified:**
  - `timeout 60 bash -c "awk '/export const PHRASES/,/\\];/' pi/lib/reminder.ts | grep -c '  \"'"` → 19.
  - `timeout 60 bash -c "rg -l 'reminder' ~/.cpd/run/adapter/*.log 2>/dev/null | wc -l; ls ~/.cpd/run/adapter/*.log | wc -l"` → 0; 1327.
  - `timeout 60 bash -c "rg -c '^test\(' pi/test/reminder.test.ts"` → 12.

### 25. Recency wins

- **Enforced by:**
  - `check` `core/src/context/contexts.rs:254` (`check_links`): `--supersedes` must name an existing record whose kind the writer may supersede (`supersedes_invalid`, `record_not_found`); `core/src/context/record.rs:276` (`Draft::from_parts`) applies the same pairing. Because every write appends the next `seq`, the target is always older, so the later record can never be removed by an earlier one.
  - `check` `core/src/context/distill.rs:334`: a `superseded` retirement is refused unless its target is in force, is not the record itself and has a **greater** `seq` ("a later record supersedes").
  - `check` `core/src/context/inforce.rs:31`: a record named by any later record's `supersedes` leaves force; `context show` labels it with its successor (`docs/contract.md:229`, `docs/commands.md:324`).
  - `gate` `core/src/store/migrations/20261018T0828-kind-tables.sql:106` (`context_records_immutable`, `BEFORE UPDATE`): a record is never edited, so the earlier record is "superseded, never edited".
  - `test` `core/src/context/inforce.rs` (9 `#[test]`s: a chain of supersessions leaves its last link; as of a `seq` a later supersede does not count) and `cli/tests/flow/context.rs` (4 `supersedes_invalid` assertions).
  - `none`: no check detects that two in-force records contradict when neither names the other.
- **Prose only:** `docs/contract.md:229` ("The User's rule is **recency wins**"), `docs/commands.md:320` (`--supersedes`, "recency wins").
- **Gap:** contradiction *detection* is gone by decision: hs-d549 and hs-d551 removed the `## Contradictions` section (`docs/contract.md:2029`, `docs/commands.md:521`) and `cli/tests/flow/context_distill.rs:442` now asserts the distiller prompt contains no "Contradictions"; the distiller only *proposes* a `superseded` retirement from its own model output, which the User approves. Two contradicting records in force therefore both stay active until someone supersedes them by hand. That is the accepted design, not a defect, but it is where recency is not enforced.
- **Proposal:** `none needed`: detecting a semantic contradiction needs a model, and the accepted replacement (the curating distiller proposing a User-approved `superseded` retirement) is already the smallest version of that; the write-time mechanisms above already make the direction of a supersession impossible to invert.
- **Verified:**
  - `timeout 60 bash -c "rg -c '#\[test\]' core/src/context/inforce.rs; rg -c 'supersedes_invalid' cli/tests/flow/context.rs"` → 9; 4.
  - `timeout 60 bash -c "rg -c 'Contradiction' docs/commands.md docs/contract.md cli/tests/flow/context_distill.rs"` → 1; 2; 1 (all saying it was removed and must not return).

### 26. Decided work skips review

- **Enforced by:**
  - `check` `core/src/job/add.rs:127` and `:158`: `--tier` and `--spec` are required unless `--draft`, and in phase `plan` every Job is a draft; `core/src/job/add.rs:167` stores a non-draft Job directly as `queued` with its fingerprint, so a decided Job runs without a Plan review.
  - `check` `core/src/job/add.rs:214` (`check_needs_not_draft`): a `queued` Job may not depend on a draft (`needs_unavailable`), so decided work and undecided design cannot be mixed silently.
  - `check` `docs/commands.md:945` and `:963` (`no_draft_jobs`, `draft_incomplete`): a Plan review covers exactly the drafts, so it can never be used to re-approve queued work.
  - `test` `pi/skills/skill.test.ts:574` (and `:581`): pins the skill text "added queued", "Spec that cites the decision", "When unsure, a draft".
  - `none`: nothing checks that a Job added without `--draft` actually carries out a recorded decision.
- **Prose only:** `pi/skills/cpd-lead/SKILL.md:42` and `:200` ("Work that only carries out a decision the User already recorded … is added queued … with a Spec that cites the decision"), `docs/commands.md:1270`.
- **Gap:** the `--draft` flag is the only switch: a Lead can add undecided design `queued` and it runs with no Plan review, and nothing ties a queued Job to a decision. Evidence: `core/src/job/spec.rs` has **0** references to `decision` or `hs-`; the preconditions of `job add` (`docs/commands.md:1280`–§5.1 table) contain no decision check; the only pin is the skill-prose test `pi/skills/skill.test.ts:574`.
- **Proposal:** `check`: `cpd job add` and `cpd job edit --spec` refuse a Job that is not a draft (and not a `--review` Job) whose Spec `context` cites no Context record id (`\bhs-[dn][0-9]+\b`) with `spec_invalid` and the JSON path `$.context`; a stub draft and a Job already stored are not re-checked. Pinned by a new `cli/tests/flow/job.rs` test; Size S.
- **Verified:**
  - `timeout 60 bash -c "rg -o 'decision|hs-' core/src/job/spec.rs | wc -l; rg -n 'decision' core/src/job/add.rs"` → 0; lines 85–86 only (the `decide(conn, …)` add/no-op helper, not a decision citation).
  - `timeout 60 bash -c "rg -n 'added queued|Spec that cites the decision' pi/skills/cpd-lead/SKILL.md | wc -l"` → 2.

### 27. A primary checkout is a mirror of origin/main

- **Enforced by:**
  - `guard` `pi/lib/mainguard.ts:65` (`primaryCheckoutMove`), applied for every role at `pi/lib/adapter.ts:59`: refuses `checkout switch reset rebase merge pull stash` run in the nearest `.git`-directory root, resolving `cd`, `~`, `$HOME` and `git -C`; evidence hs-n436/hs-d573/hs-d248 at `docs/commands.md:3744`; 21 tests in `pi/test/mainguard.test.ts` (including "refuses the incident command with a tilde and the home directory resolved").
  - `guard` `core/src/host/git_guard.rs:30`, `:88`: the `$CPD_HOME/bin/git` wrapper is first on every Session's `PATH` and refuses `git stash` everywhere; evidence hs-d310 (`core/src/host/git_guard.rs:19`).
  - `test` `pi/test/mainguard.test.ts:262` ("no skill file names a git command that moves HEAD") and `:258`/`:260` (both skills name only the allowed read forms); `docs/commands.md:3768` says the tests read the skill files.
  - `check` (in flight, Plan `main-mirror`, not to be duplicated): PR `guard` (dispatch; `guard-bash-tests` running, `guard-impl` queued) extends §15.16 to the `edit` and `write` tools and to the bash write forms; PR `spec` refuses a Job Spec that names a Plan repo's primary checkout (`cli/tests/flow/job.rs`); PR `install` makes a skipped `cpd-install.sh` fail loud.
  - `none`: the `edit` and `write` tools by absolute path are not guarded yet.
- **Prose only:** `pi/skills/cpd-lead/SKILL.md:30` and `pi/skills/cpd-manager/SKILL.md:31` ("Never move HEAD or change the files of a repo's primary checkout … the adapter refuses it"), `docs/commands.md:3744`.
- **Gap:** the guard reads only the `bash` tool (`rg -c 'toolName === "bash"' pi/lib/adapter.ts` → 1, the guard handler); `pi/lib/mainguard.ts` names no `edit`/`write` (0 matches), so a Worker that edits a primary checkout with the `edit`/`write` tool by absolute path is not refused. That is the 2026-10-24 incident (Plan `main-mirror`'s context: two Workers changed `docs/contract.md` and `docs/commands.md` of cpd's primary checkout with `edit`, records hs-d606/hs-d608/hs-d609), and PR `guard` of `main-mirror` owns the extension.
- **Proposal:** `none needed` here: Plan `main-mirror` PRs `guard`, `spec` and `install` are the smallest enforcement and are already in flight; duplicating them would be YAGNI.
- **Verified:**
  - `timeout 60 bash -c "rg -o 'edit|write' pi/lib/mainguard.ts | wc -l; rg -c 'toolName === \"bash\"' pi/lib/adapter.ts; rg -c '^test\(' pi/test/mainguard.test.ts"` → 0; 1; 21.
  - `timeout 60 cpd plan show --plan main-mirror` → phase `dispatch`; PR `guard` dispatch (`guard-bash-tests` running, `guard-impl` queued), PR `spec` dispatch, PR `install` dispatch.

## Not verified

- I did not open a live pi pane or read the raw argv of a running Worker; I confirmed only that my own system prompt carries the 19-axiom AGENTS.md and that the `lead`/`worker` argv builders in `core/src/host/pi/launch.rs` pass no `--no-context-files` (the flag is only in `distiller`, `core/src/host/pi/launch.rs:195`). That the running Worker argv equals that code is "should", not read.
- The 200 `blocked` `cpd: Worker question` and 203 `blocked` `cpd: question to the Lead` Events are counted without timestamps: I infer they predate hs-d303's switch to `waiting` (which now shows 148 and 150). I did not date them against hs-d303.
- I did not run any test, the full suite or the Gate; the test counts above are the number of `test(`/`#[test]` sites and `rg` occurrences, not passing runs.
- The proposal sizes (S/M) are my estimates of code change, not measured.
- `docs/commands.md` line numbers were read from this worktree's base commit; a later merge may move them.
