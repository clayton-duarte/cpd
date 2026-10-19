# Attention audit: every `blocked` and red place against hs-d303

Base: `main` at `5b323eb` (Plan `inbox`, Job `attn-audit`). Read-only research: nothing here changed code.

**Rule (hs-d303):** `blocked` means it needs the human. Anywhere the cockpit or the store shows `blocked`, or the red that stands for it, the User has to act. What an agent handles shows as waiting or working, never blocked or red. hs-d302 (decided) applies it to a pull request a Plan owns (a `prs` row points to it): conflict, failed checks and `changes requested` reach its Lead as `pr-feedback`; none opens an Inbox item or turns a tree dot red.

**Verdicts:** `AH` = *clear: agent-handled* (must become waiting/working); `H` = *clear: human* (stays); `U` = *unclear* (ask the User, see the end).

**Method.** I grepped `blocked`, `Blocked`, `Red`, `NEEDS_YOU`, `Meaning::Failed`, `Urgency::Red`, `state_label` and `escalation` under `core/`, `tui/`, `pi/`, `cli/` and `docs/`, and read each hit. Not run: nothing here was executed, so every "trigger" is from the source. Where I did not trace something to the end it says "unverified".

**Two facts that shape the list.**

1. Only **one** Session state is called `blocked`, and the adapter and `cpd ask` use it for both human and agent reasons (`pi/lib/state.ts:2`, `core/src/ask/question.rs:138`). The Inbox then removes the agent reasons again by label (`core/src/inbox/rules.rs:32`, `:155`). So the store lies and the Inbox corrects it; every other reader of `state` sees `blocked` for an agent wait.
2. The Plan view has **no Worker mark** and the StatusBar has **no "waiting" count** on this base: `tui/src/plan/` reads no Worker state (`rg "state_label|\.state" tui/src/plan` finds only a test fixture insert, `tui/src/plan/mod.rs:876`), and `tui/src/statusbar/` draws only `inbox <n>` and `workers N/M` (`tui/src/statusbar/inbox.rs:35-45`, `workers.rs:30-41`). If the Plan `inbox` PR `attn` adds either, it must follow this table.

## 1. The table

### 1.1 Session `state` / `state_label` (store, `cpd session list|show`, events)

| # | Place (file:line) | Trigger | Who acts | Verdict |
|---|---|---|---|---|
| 1 | Worker Session `blocked`, label `cpd: question to the Lead`: `core/src/ask/question.rs:20` (label), `:124-139` (`block`), `:142-148` (`resume`) | The Worker runs `cpd ask`; the Escalation opens in the same transaction (`:67`). Back to `working` when answered or expired (`docs/commands.md:1105`, `:1107` "Answered… Session `working`"). | The **Lead** answers (`cpd answer`; `pi/skills/cpd-lead/SKILL.md:229`). The Worker waits for the Lead, not the User. | **AH** |
| 2 | Lead Session `blocked`, label `cpd: Worker question`: `pi/lib/adapter.ts:32` (label), `:317` (opened at start for each open Escalation), `:435` (opened on an `escalation` Event), `:438-439` (closed on `escalation-answered` / `-expired`); reported by `pi/lib/state.ts:44-46` | A Worker of the Plan has an open Escalation to this Lead. | The **Lead** answers it. | **AH** |
| 3 | Lead Session `blocked`, label `cpd: plan review`: `pi/lib/plan-review.ts:6` (label), `pi/lib/adapter.ts:323`, `:580` | The Plan's latest review is `pending`. | The **User** decides (plannotator, `cpd plan decide`). | **H** |
| 4 | Lead, fork or Manager Session `blocked`, label `dialog`: `pi/lib/state.ts:33-39` (`prompt`), `pi/lib/adapter.ts:768-771` (`ui_prompt_start` / `ui_prompt_end`) | pi shows any dialog (`ctx.ui.select`, `confirm`, `input`, `editor`, `custom`), including the Lead's `ask_user_question`. | The **User** answers it. | **H** |
| 5 | Worker Session `blocked`, label `dialog`: the same code as #4. `onPrompt` has no role check (`adapter.ts:765-771`); the "Plan Lead only" comment covers only the Escalation and review reasons (`adapter.ts:306`). The Inbox ignores it: `seats` excludes Workers (`core/src/inbox/rules.rs:101-107`, `:135-148`). | A pi dialog opens in a Worker's pi. Whether a Worker ever opens one is **unverified**. | Nobody is told. The row is `blocked` with no Inbox item and no dot. | **U** (Q6) |
| 6 | **Label precedence**: `pi/lib/state.ts:44-46` reports the label of the *most recently opened* reason. A Worker question opened while a `dialog` or `plan review` is open replaces its label; `dialogs` then drops the item (`rules.rs:155`) and the red/magenta dot goes out while the User still has to act. (From reading; not reproduced.) | A Worker escalates while the Lead has a dialog open. | Needs the **User**, shown as nothing. | **Defect against hs-d303**, fixed by the `waiting` split in Job J2 |
| 7 | The `sessions.state` domain: `core/src/store/migrations/0001-initial.sql:230,237`, `0010-manager.sql:22,31` (CHECK `state IN ('working','blocked','idle','stopped')`, and `blocked` ⇔ label set), `core/src/session/row.rs:41-57`, transition table `core/src/session/state.rs:52-58` | The enum itself: no state means "waiting for an agent". | n/a | The cause of #1, #2 and #6. Fixed in J1. |
| 8 | Contract and CONTEXT text: `docs/contract.md:348,396,572,671` ("an open Escalation to this Lead" is a `blocked` reason), `docs/commands.md:1105,1486,2273`, `CONTEXT.md:146` ("blocked, idle, …"), `pi/skills/cpd-lead/SKILL.md:229` ("The Worker is blocked until you answer") | Wording | n/a | **AH** (follows J1) |

Other readers of `state = blocked` (not marks, but they must keep working when an agent wait stops being `blocked`): live-for-archive `core/src/session/archive.rs:47`, no reload while `blocked` `core/src/session/reload.rs:134,153` and `docs/commands.md:2273`, `unread` after a `blocked` → `idle` turn `core/src/inbox/rules.rs:72-79` (`LATEST_IDLE`), wake wording `docs/commands.md:1777`. A new `waiting` state must join each.

### 1.2 Inbox kinds and urgencies (`core/src/inbox/rules.rs`; contract §2.2, `docs/contract.md:376-410`)

| # | Place | Trigger | Who acts | Verdict |
|---|---|---|---|---|
| 9 | `dialog`, **red** in `dispatch` / `deploy`: `rules.rs:153-169` (`:160`) | A Lead, fork or Manager is `blocked` with a label other than the two in `NOT_A_DIALOG` (`rules.rs:32`). | The **User**. | **H** |
| 10 | `dialog`, **magenta** (other phases, Manager, loose fork): `rules.rs:161` | Same. | The **User**. | **H** |
| 11 | `plan-review`, magenta: `rules.rs:203-231` (`:218`) | Latest review `pending`. | The **User**. | **H** |
| 12 | `unread`, white: `rules.rs:172-181` | An idle Lead/fork/Manager answered the User and the chat has not been seen. Not blocked and not red; it is a "needs you" dot. | The **User** (reads). | **H** |
| 13 | `job-stuck`, **red**: `rules.rs:233-270` (`:254`) | A Job is `failed` and its latest `job-failed` Event is at least `inbox.stuck_job_minutes` old (`core/src/watch/inbox.rs:30`). The Lead got `job-failed` with a turn (`pi/lib/delivery.ts:11`) and its skill tells it to diagnose, retry or cancel (`SKILL.md:176`). Clears on retry or cancel. | The Lead first; the User only if the Lead did nothing. | **U** (Q1) |
| 14 | `engine-error`, **red**: `rules.rs:272-290` (`:277`) | The engine reports an `error` for one of its steps E1–E9 and has not cleared it. | The User (or whoever maintains cpd); no agent can fix the engine. | **U** (Q2) |
| 15 | `engine-down`, **red**: `core/src/inbox/cmd.rs:131-140` (never stored; `contract.md:402`) | `locks/watch.lock` is free: the engine is not running. | The **User** starts it (no agent runs `cpd watch`). | **H** |
| 16 | `question`, magenta: `rules.rs:296-325` (`:311`) | A Context `question` record has no `answer`. | The **User**. | **H** |
| 17 | `pr-failing`, **red**: `rules.rs:371-383` (`:377`). The filter is `r.authored = 1 AND (checks failed OR mergeable conflicting)`; it has **no** `prs` exclusion on this base (hs-d302 is only in the docs, `docs/contract.md:403-404,408` and `docs/commands.md:2503,2508`). | Failed checks or a conflict on an open pull request the User authored. | **Owned by a Plan:** its Lead (via `pr-feedback`, `SKILL.md:181`). **Not owned (no `prs` row):** the User. | Owned: **AH** (decided, hs-d302, not yet in code). Not owned: **H** (stays) |
| 18 | `pr-changes`, **red**: `rules.rs:385-394` (`:390`), same missing exclusion | `review_decision = changes_requested` on an authored open pull request. | Same split as #17. | Owned: **AH** (decided). Not owned: **H** |
| 19 | `pr-ready`, yellow: `rules.rs:396-406` | Authored, open, non-draft, checks passed, mergeable. | The **User** merges. | **H** (hs-d302: unchanged) |
| 20 | `review-requested`, yellow: `rules.rs:408-419` | The User's review is requested. | The **User**. | **H** (unchanged) |

### 1.3 Tree dots, the tree's Inbox section, the StatusBar

| # | Place | Trigger | Who acts | Verdict |
|---|---|---|---|---|
| 21 | `Dot::Red` and its colour: `tui/src/read/dot.rs:12-28`, `tui/src/palette.rs:215-221`; chosen by item urgency `dot.rs:55-63` | The most urgent visible item linked to the row is red: `dialog` (dispatch/deploy), `job-stuck`, `pr-failing`, `pr-changes` (`docs/commands.md:2503`). | By kind: see #9, #13, #17, #18. | Inherits the item verdict. No change of its own. |
| 22 | `PLAN_KINDS` puts `PrFailing` and `PrChanges` on a Plan Lead's dot: `dot.rs:67`, `:72-84` | An open `pr-failing` / `pr-changes` item whose `plan` is the Lead's Plan. | The Lead (owned PR). | **AH** (decided, hs-d302; dead once #17/#18 exclude owned PRs) |
| 23 | `Dot::glyph(Blocked)` → `Dot::Idle`: `dot.rs:36-41`, test `dot.rs:121` ("blocked is no dot of its own") | A Lead `blocked` without an item, which is exactly #2: **a Lead waiting for a Worker answer looks idle** (the plain hollow dot). Not red, not a violation of the red rule, but it hides "it is waiting for an agent". | The Lead. | **AH** (no `waiting` dot exists) |
| 24 | Context row dot = its Manager's dot only: `tui/src/read/tree.rs:155` (comment at `commands.md:2508`) | The Manager is `blocked` (`dialog`) or has an unread reply. | The **User**. | **H** |
| 25 | Inbox section rows: dot per item urgency `tui/src/tree/inbox.rs:216`; counts in the header | Any open visible item. | By kind. | Inherits |
| 26 | StatusBar `inbox <n>`, coloured by the most urgent item: `tui/src/statusbar/inbox.rs:35-45` (`:41`) | Any open visible item. | By kind. | Inherits (a red here comes only from #9, #13–#15, #17, #18). |
| 27 | StatusBar `workers N/M`: `tui/src/statusbar/workers.rs:30-41`; green when `N > 0` | Attempts holding a slot. Not an attention mark. | n/a | Not applicable. No `W waiting` count exists on this base (see "Two facts" 2). |

### 1.4 Plan view, Dispatch column, tree counters (red = `Meaning::Failed`, `tui/src/palette.rs:77`)

| # | Place | Trigger | Who acts | Verdict |
|---|---|---|---|---|
| 28 | Job pill `failed`: `palette.rs:146-152` (`:152`), drawn `tui/src/plan/flow.rs:48,393` | The Job's status is `failed`. | The Lead (retry/cancel, `SKILL.md:176`); the User only after #13. | **U** (Q3) |
| 29 | The tree's failed Job counter (4th number): `palette.rs:234-236` (`count_style`), `tui/src/tree/rows.rs:204-227`, counts `tui/src/read/shared.rs:118,130-133`; legend `tui/src/widgets/legend.rs:101-118` | Count of `failed` Jobs > 0. | Same as #28. | **U** (Q3) |
| 30 | Plan box title `#n conflict`, red: `tui/src/plan/boxes.rs:79-97` (`state_word`, `:82`), `palette.rs:252` (`PR_CONFLICT`) | An open owned pull request has `mergeable = conflicting`. A Plan's boxes are always owned. | The **Lead** (hs-d220, `SKILL.md:181`: resolves it itself; asks only if the two sides contradict). | **AH** |
| 31 | Dispatch column ` conflict`, red: `tui/src/widgets/column.rs:97-98` (comment `:74`) | An open authored pull request is `conflicting`. The column lists owned **and** unowned ones (`tui/src/read/widgets.rs:183-190`; unowned are drawn dim, `column.rs:75`). | Owned: the Lead. Unowned: the User. | Owned: **AH**. Unowned: **H** |
| 32 | PR checks `failed` icon, red: `palette.rs:255-258` (`checks`), `column.rs:86-93`, plan box `tui/src/plan/boxes.rs` (via `palette::checks`) | A pull request's `checks = failed`. | Owned: the Lead (fix Job, `SKILL.md:181`). Unowned: the User. | Owned: **AH**. Unowned: **H** |
| 33 | PR state `closed`, red: `palette.rs:239-244` (`:243`) | An open-then-closed (unmerged) pull request. The Lead gets `pr-feedback` `closed` and **asks the User** (`SKILL.md:181`), so it reaches the User through a `dialog`, but the red is immediate. | The User, via the Lead. | **U** (Q4) |
| 34 | `changes requested` in the Dispatch column: `column.rs:80-85` | `review_decision = changes_requested`. **Not red** (asserted `column.rs:435`: "the review is not red"). | Owned: the Lead. | Already fine. Keep as is. |
| 35 | Plan review row `Plan review r<n> pending`, yellow (`Meaning::Review`): `tui/src/plan/mod.rs:82-104` (`:104`) | Latest review pending. | The **User**. | **H** |

### 1.5 Legend

| # | Place | Trigger / text | Who acts | Verdict |
|---|---|---|---|---|
| 36 | `tui/src/widgets/legend.rs:140` "the Session needs you" | the filled dot | the User | **H** |
| 37 | `legend.rs:144` "red: blocked work (a dialog, an Escalation) in dispatch or pr" | Text. Two faults: an Escalation is no Inbox item (`contract.md:392`: "Escalations from Workers go to their Lead, not to the User, so they are not items"), so it is never red; and the phase is `deploy`, not `pr` (`rules.rs:160`). Violates hs-d303 as written. | n/a | **AH** (reword) |
| 38 | `legend.rs:145` "yellow: the Plan's Lead, its pull request is open with checks passed or failed" | Text. Stale: `pr-ready` is yellow only for passed checks (`rules.rs:396-406`); a failed run is `pr-failing` (red, to be removed for owned PRs). | n/a | **AH** (reword) |
| 39 | `legend.rs:146` "magenta: blocked while grounding, planning or done, or the Plan's review is pending" | Text. Correct for `dialog` and `plan-review`. | n/a | **H** (keep; reword "blocked" → "asks you" only if J3 does) |
| 40 | `legend.rs:101-118` and `palette.rs:294-298` legend rows `failed` (Job), `closed`, `conflict`, checks `failed` | Colour samples of #28–#33. | n/a | Follow the verdicts of #28–#33. |

### 1.6 Other red or "needs you" marks and the word `blocked` elsewhere

| # | Place | Trigger | Who acts | Verdict |
|---|---|---|---|---|
| 41 | Logs popup `error` Event lines, red: `tui/src/logs/view.rs:101-105` | Any Event of kind `error`. That covers the engine's step errors (#14, to the User) and the errors addressed to a Lead (`pr_start_failed`, `push_failed`, driver errors; `contract.md:392`, `pi/lib/delivery.ts:15` delivers `error` to the Lead with a turn). The view cannot tell them apart from the kind. | Mixed. | **U** (Q5) |
| 42 | Worker view foot: `blocked <label>` text: `tui/src/worker/mod.rs:134-142` | The Worker's Session is `blocked` (#1). Plain text, not red. | The Lead. | **AH** (becomes `waiting …` with J1) |
| 43 | Worker view foot note in red: `tui/src/worker/view.rs:88-91`, set at `tui/src/worker/mod.rs:106,160` | A refused command, an unreadable session file or a locked store: feedback to the User's own action. | The **User**. | **H** |
| 44 | Merge popup failure, `palette::note()` (yellow, not red): `tui/src/merge/mod.rs:168-171` | `cpd pr merge` was refused (for example GitHub `OPEN and BLOCKED`, `merge/mod.rs:331`). The User started the merge. | The **User**. | **H** |
| 45 | `merge_state = blocked` (GitHub's word for branch protection): `core/src/pr/status.rs:157-191`, `core/src/pr/gh.rs:707`, `docs/contract.md:235` | Stored from the poll; shown only through #44's message. Not a cpd state. | The User (merging). | **H** (name only) |
| 46 | Error code `archive_blocked`: `core/src/error.rs:171`, `core/src/session/archive.rs:81-90` | Archive/cancel refused because something is live (a Session `working` or `blocked`, a Job, an Escalation, a review). The word describes the refusal, not a Session. | The caller (User or Lead). | **H** (name only) |
| 47 | Adapter `ui.notify(…, "error")`: `pi/lib/adapter.ts:263` (cannot write the reload log), `:338` (adapter update not loaded) | An install/reload problem in the User's own pi. No agent sees it. | The **User** (whoever installs cpd). | **H** |
| 48 | "Dependents stay blocked" for a failed or cancelled Job: `docs/commands.md:897`, `pi/skills/cpd-lead/SKILL.md:216` | Wording for a Job that cannot run. No UI mark: the dependent is `queued`. | The Lead (cancel and re-append). | **AH** (wording: say "cannot start") |

## 2. Proposed changes for every AH place

Common design (one line, everywhere): a new Session **state `waiting`** = "the agent waits for another agent, not for the User"; `blocked` is kept only for the human reasons (`dialog`, `cpd: plan review`). The Worker's label `cpd: question to the Lead` and the Lead's label `cpd: Worker question` stay as the `state_label` of `waiting`. The name `waiting` is also a PR status (`tui/src/read/flow.rs:56`); I take that to be harmless (a different object), the Lead may prefer another word. `Meaning::Waiting` is a palette colour for the same idea, to be picked by the Lead (not red, not yellow-as-attention).

| For # | Proposed change (one line) | Files |
|---|---|---|
| 1, 7, 8 | `cpd ask` sets the Worker `waiting` (not `blocked`) with the same label; `waiting` joins the enum, the CHECK (`waiting` ⇔ label set, like `blocked`), `legal()` and the live/reload/unread readers. | migration (new, table rebuild like `0010-manager.sql`), `core/src/session/row.rs`, `state.rs`, `archive.rs:47`, `reload.rs:153`, `core/src/ask/question.rs`, `core/src/inbox/rules.rs:79`, `docs/contract.md` §3.6, `docs/commands.md` §8.1, §10.6 |
| 2, 6 | The adapter gives every reason a kind: human (`dialog`, `plan review`) or agent (`Worker question`). It reports `blocked` (label of the latest human reason) while any human reason is open, else `waiting` (Worker question) while one is open, else `working` / `idle`. A Worker question can no longer hide a dialog. | `pi/lib/state.ts:1-50`, `pi/lib/adapter.ts:317,435,438`, `pi/test/state.test.ts`, `pi/test/delivery.test.ts`, `docs/commands.md` §15.4 |
| 2 | `NOT_A_DIALOG` loses `cpd: Worker question` (a Lead `waiting` is never `blocked`); `cpd: plan review` stays (own kind). | `core/src/inbox/rules.rs:32,155` + its tests (`:599`) |
| 23, 42 | A `waiting` row shows its own dot (new `Dot::Waiting`, a non-red glyph, between `working` and `idle` in the order) and the Worker foot reads `waiting cpd: question to the Lead`. | `tui/src/read/dot.rs:12-41`, `tui/src/palette.rs:215`, `tui/src/worker/mod.rs:134`, `tui/src/tree/fixture.rs:36` (the test fixture's `Blocked ⇒ Red` guess), `docs/commands.md` §17 |
| 37, 38, 39 | Reword the legend: red = "a dialog in dispatch or deploy, a stuck Job, an engine error, your own failing or changed pull request"; yellow = "ready to merge, review requested"; add the `waiting` dot. | `tui/src/widgets/legend.rs:140-148` and its tests (`:297-320`) |
| 17, 18, 22 (decided, hs-d302) | Owned pull requests open no `pr-failing` / `pr-changes`: add `AND NOT EXISTS (SELECT 1 FROM prs WHERE prs.pull_request = r.id)` to both filters; drop `PrFailing`, `PrChanges` from `PLAN_KINDS`. | `core/src/inbox/rules.rs:371-394` + tests (`:865-905`, `:959-975`), `tui/src/read/dot.rs:67`, `cli/tests/inbox_rules.rs`, `cli/tests/tui_read.rs` |
| 30, 31 (owned), 32 (owned) | For an **owned** pull request show conflict and failed checks in `Meaning::Waiting`, not `Meaning::Failed`; unowned ones stay red. In the Plan box every PR is owned, so `state_word` and the checks icon never use red there. | `tui/src/palette.rs` (`Meaning::Waiting`, `checks`, `PR_CONFLICT`), `tui/src/plan/boxes.rs:79-97`, `tui/src/widgets/column.rs:86-98`, `tui/src/read/widgets.rs` (carry `owned` if not already, `:183-190`), their tests |
| 48 | Say "cannot start" for dependents of a failed or cancelled Job. | `docs/commands.md:897`, `pi/skills/cpd-lead/SKILL.md:216` |

### Jobs (about 150 turns each, in order)

- **J1 `waiting-state-core`** (rows #1, #7, #8 and the `rules.rs` part of #2). Add the `waiting` state to the store and to `cpd ask`; update the live, reload, unread and inbox readers; contract and `commands.md` §3.6, §8.1, §10.6. Tests first: `ask` leaves the Worker `waiting`; `session state waiting --label …` works; `archive` still counts it live; `reload_now` refuses it; a `waiting` → `idle` reply turn is `unread`. Files: the list in the first row above.
- **J2 `waiting-state-adapter`** (rows #2, #6, after J1). Reason kinds in `state.ts` and `adapter.ts`, the precedence rule, `stateArgs` for `waiting`; adapter tests incl. "a Worker question opened while a dialog is open keeps `blocked: dialog`"; `commands.md` §15.4; `SKILL.md:229`. Files: the second row above.
- **J3 `waiting-tui`** (rows #23, #37–#39, #42, after J1). `Dot::Waiting` and its glyph, the precedence order, the Worker foot, the legend rewording, `Meaning::Waiting` in the palette. Files: the `dot.rs`/`palette.rs`/`worker`/`legend.rs` rows above, plus `cli/tests/tui_read.rs`, `tui_tree.rs`, `tui_widgets.rs` as they assert `Blocked` and the legend.
- **J4 `owned-pr-not-red`** (rows #17, #18, #22 and #30–#32, after J3 for `Meaning::Waiting`). hs-d302 in code (this overlaps Plan `repo-policy`'s `policy-pr`, which also edits `rules.rs`; whoever lands second merges, as the Job context says), and the owned PR conflict / failed checks colour. Files: the `rules.rs`, `dot.rs`, `palette.rs`, `boxes.rs`, `column.rs`, `widgets.rs` rows above. Closes the hs-d303 audit for pull requests.
- **J5 (docs wording, optional)**: #48 and the remaining `blocked` prose, folded into J1 or J3 if the Lead prefers.

If the User decides any `U` below as "agent-handled", it adds one line to J3 (colour/label) and, for Q3 and Q4, to J4 (`palette.rs`, `flow.rs`, `rows.rs`).

## 3. Unclear: ask the User

Each is a place that shows red (or `blocked`) where I cannot tell from the code whether the User must act.

- **Q1 (#13 `job-stuck`).** A Job has been `failed` for `inbox.stuck_job_minutes` (default in `core/src/config/file.rs:190`) and its Lead has not retried or cancelled it. Is that an item for you at red, or should it stay with the Lead? And is the cut-off wrong: should a Lead that is actively `working` on the failed Job (its turn is running) postpone `job-stuck`?
- **Q2 (#14 `engine-error`).** An engine step (E1–E9) failed and stays active. Is every such error yours (red, as now), or may some codes (for example one the Lead can answer) go to the Lead as an `error` Event only? If only some, which codes?
- **Q3 (#28, #29 failed Job pill and counter).** The Job status `failed` is drawn red in the Plan view and in the tree counters from the moment it fails, while the Lead is still diagnosing it and `job-stuck` has not fired. Should `failed` stay red as a plain status colour, or show the waiting colour until it becomes `job-stuck`?
- **Q4 (#33 `closed` pull request).** A closed-unmerged pull request is red now, but its Lead asks you through a dialog (`SKILL.md:181`). Should `closed` stay red (a status), or follow the owned-PR rule (not red until the Lead's dialog)?
- **Q5 (#41 red `error` lines in the logs popup).** The popup colours every `error` Event red, whether it is the engine's (yours) or addressed to a Lead. Should it colour only the engine's errors red (a change in `logs/view.rs`), or keep every `error` red because it is a log, not an attention mark?
- **Q6 (#5 a Worker's `dialog`).** If a Worker's pi opens a dialog, the Session becomes `blocked` and nobody is told (no Inbox item for Workers). Do Workers ever have a human at their pane? If yes, should that be an Inbox item (a `dialog` for a Worker); if no, should the adapter refuse to report a `dialog` reason for a Worker, or report `waiting`?
