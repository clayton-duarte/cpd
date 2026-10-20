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

## 4. Clicks: where each Inbox kind's click lands (hs-d306 (2))

Same base and method as above (read from the source; **nothing was run**, so what the User "then sees" is inferred from the code and the docs, and marked *unverified* where it rests on tmux or pi behaviour I did not watch).

**The path of a plain click.** `View::click_inbox` (`tui/src/tree/view.rs:504-516`) → `View::jump` (`:519-525`) → `inbox::jump` (`tui/src/tree/inbox.rs:181-195`), which returns a `Jump`: `Run(Action)` or `Logs(event)`. Enter on an item does the same (`view.rs:273`). `tree/mod.rs:144-156` runs it: `Logs` opens the logs popup (`Runner::logs`, `tree/actions.rs:261-263`, `logs::open_at`, `tui/src/logs/mod.rs:61-68`); `Run` calls `Runner::run` (`actions.rs:229-247`), which runs the `cpd` command and, after `Action::Open` or `History`, `select-pane` on the chat slot (`:242-244`, `focus_chat` `:250-257`). `Action::CopyUrl` (`actions.rs:113`) is `tmux set-buffer -w` only, with the note `copied <url>: Cmd-click the row to open it` (`:202`). The OSC 8 link on the label (`inbox.rs:204-206` `is_link`, drawn `:245-248`) is the terminal's modifier-click; it is not part of a plain click.

**"Clear, actionable state"** (hs-d306): after the click the User is looking at something they can act on without a further search: a pending dialog in the chat slot, the review page, a pull request, a prompt that asks for the one thing needed.

### 4.1 The table

| Kind | Plain click today (file:line) | What the User then sees | Verdict |
|---|---|---|---|
| `dialog` | `Kind::Dialog` → `Action::Open(item.session)` or `History` if archived: `inbox.rs:183`, `open_or_history` `:168-176`. The seat is a live Lead, fork or Manager (`rules.rs:155`, `seats` `:101-107`). `session open` + `select-pane` (`actions.rs:242`). | The Session in the chat slot, focused, with its pi dialog still open (the Session is `blocked` *because* the dialog is open: `pi/lib/state.ts:33-39`, `adapter.ts:768-771`). Already in the slot: the same, plus the focus (*unverified*: `session open` of the Session already there). | **Actionable.** Keep. |
| `unread` | Same arm, `inbox.rs:183`. | The Lead's, fork's or Manager's chat with the reply. Landing in the slot closes the item (`session-moved` to `chat`, `rules.rs:172-181`). | **Actionable** (reading is the act). Keep. |
| `plan-review` | `inbox.rs:187`: `open_lead(plan)` → `Action::Open(plans.lead_session)`. The page URL is on the item (`rules.rs:203-231`, `plannotator_url` `:187-200`) but only as the label's OSC 8 link (`inbox.rs:204`, `:245-248`). | The Lead's chat. The Lead is `blocked` with label `cpd: plan review` (`pi/lib/plan-review.ts:6`): it waits on **plannotator's browser page**, not on a pi dialog, so the chat has nothing pending to answer. The Plan view beside it shows `Plan review r<n> pending 󰏌` (`tui/src/plan/mod.rs:82-104`), again only an OSC 8 link; the Plan view's mouse code reacts to chart pills only (`plan/mod.rs:437-455`). The page is reachable only by a modifier-click on a label (*unverified through tmux*, `docs/commands.md:1953`). | **Not actionable.** A plain click lands on a chat where nothing is asked. |
| `job-stuck` | `inbox.rs:184`: `open_lead(plan)` → `Action::Open(Lead)`. `item.job` (`rules.rs:254-268`, set) and `item.event` (the `job-failed` Event) are ignored. | The Lead's chat, at the end of its scrollback. The `job-failed` turn reached the Lead when the Job failed (`pi/lib/delivery.ts:11`), so it may be far up, or already answered. The Plan view beside it draws the Job pill red (`palette.rs:152`) but selects nothing: the selection is the view's own state, reset on every Lead switch (`plan/mod.rs:225-234`, `Selection::reset` `plan/flow.rs:416`), and no other process can set it. Nothing in the cockpit shows **why** the Job failed: `JobNode.failed_cause` is read (`tui/src/read/flow.rs:123,294`) and never drawn (`rg failed_cause tui/src` finds no other use). | **Not actionable.** The User lands on a chat, not on the failed Job. |
| `engine-error` | `inbox.rs:185`: `Jump::Logs(item.event)` → logs popup at the `error` Event (`logs/mod.rs:61-68`, `popup_args` `:44-52`). | The popup with the Event's line selected, red (`logs/view.rs:101-105`). The line is `The engine failed at <step> for <scope>: <message> It tries again at every tick. See: <command>` (`core/src/watch/report.rs:172-176`; `see` `:90` is a command for the Lead to **look** at it, not a fix). So the User learns what failed, and that it retries, not what to do. | **Actionable only if the User can fix that error.** An error the User cannot fix (a bug in cpd) is a place to read, not an action: see Q-C2. Fix for the rest: none in the click. |
| `engine-down` | `inbox.rs:185`: `Jump::Logs(None)` (no stored item, so no event: `inbox/cmd.rs:131-140`). | The logs popup at the bottom: the last Events from before the engine died. Nothing in it tells the User what to do, and a dead engine adds no line. The fix is a command the cockpit already knows: `cpd watch --ensure` (`docs/commands.md:1864`; `tui/src/cockpit.rs:74` runs it at start). | **Not actionable.** The click shows the past, the User must start the engine elsewhere. |
| `pr-failing` | Owned (`item.plan` set): `inbox.rs:188-190`: the Lead. Unowned: `:191-193`: `Action::CopyUrl(url)`. | Owned: the Lead's chat, which is already working on it (`pr-feedback`). Unowned: the footer note `copied <url>: Cmd-click…`; the page opens only by a modifier-click. | Owned: **gone** (hs-d302 removes the item; the arm is dead code afterwards). Unowned: **not actionable** until the User makes a second gesture (Q-C1). |
| `pr-changes` | Same arms. | Same. | Same. |
| `pr-ready` | Owned: `inbox.rs:188-190`: the Lead. Unowned: `:191-193`: `CopyUrl`. | Owned: the Lead's chat; the merge is a click on the merge icon in the Deploy column (`tui/src/widgets/column.rs:38-43`, `widgets/mod.rs:105-110`) that opens the confirmation popup (`tui/src/merge/mod.rs:1-7`, `open` `:208-214`). Unowned: the copy note, and the column lists only authored open PRs, so the icon may be there too. The act the User wants (merge) needs one more search-and-click. | **Partly.** Fix below: open the merge popup. |
| `review-requested` | `inbox.rs:191-193`: `CopyUrl`. | The copy note. The review itself happens on GitHub. | **Not actionable** until the modifier-click (Q-C1). |
| `question` | `inbox.rs:186`: `OpenManager(context)` → `context manager`, then `session open` (`actions.rs:83`, `:130`). | The Manager's chat with nothing pending (hs-q6 and hs-q7). | Dropped by hs-d306 (1): delete the arm, the kind and its test (`inbox.rs:411`, `:430`), not part of the Jobs below. |

### 4.2 Proposed fixes

| For | Fix (one line) | Files |
|---|---|---|
| `engine-down` | The click runs `cpd watch --ensure` (idempotent, `commands.md:1864`; new `Action::EnsureWatch`, argv `watch --ensure`, note `engine started` / `already running`), and no longer opens the logs (the `▤` button still does). | `tui/src/tree/inbox.rs:181-195`, `tui/src/tree/actions.rs` (`Action`, `argv`, `done`), `docs/commands.md` §13.2 click table, `cli/tests/tui_tree.rs` |
| `pr-ready` (owned and unowned) | The click opens the merge popup for the item's pull request, `cpd tui view merge <owner>/<repo>#<n>`, taken from the item's `subject` (`pr:<owner>/<repo>#<n>`, `rules.rs:357`); `pr merge` needs no Plan (`core/src/pr/merge.rs:1-3`), so it serves unowned ones too. New `Jump::Merge(String)` and `Effect::Merge`; reuse `merge::open` (`merge/mod.rs:208`). Better still: add a `pull` field to the item's JSON if the Lead prefers no subject parsing (then also `core/src/inbox/model.rs`, `cmd.rs`). | `tui/src/tree/inbox.rs`, `tui/src/tree/view.rs:519-525`, `tui/src/tree/mod.rs:144-156`, `tui/src/tree/actions.rs` (`Runner::merge`), `docs/commands.md` §13.2, §17, `cli/tests/tui_tree.rs` |
| `job-stuck` | The click opens the Lead **and** the read-only Worker pane of the failed Job's latest Attempt beside it (`tui view worker <attempt> --plan <plan>`, `plan/actions.rs:76-100`): the User lands on the chat and the log of what failed. The Attempt comes from `read::flow` (`read/flow.rs:192`, `JobNode::attempts`); `toggle_worker` moves from `plan/actions.rs` to `widgets` beside `open_lead` (one copy, as `widgets::open_lead` is). The Worker foot also says why the Job failed (`failed_cause`, now unused). | `tui/src/plan/actions.rs`, `tui/src/widgets/mod.rs`, `tui/src/tree/{inbox,view,actions,mod}.rs`, `tui/src/worker/mod.rs:134-142` (the foot), `docs/commands.md` §13.2, §17, `cli/tests/tui_tree.rs`, `cli/tests/tui_worker.rs` |
| `plan-review` | The click opens the Lead **and** puts the plannotator page where the User can use it: copies the item's `url` (the existing `CopyUrl`, so two steps: `Open`, then the copy, note `copied <url>: Cmd-click…`), and with no port file says so (`no review page yet`) instead of acting silently. Weaker than "opens the page": a browser on the cockpit's machine cannot be assumed (`review_host`, hs-d264, exists because the User is often remote). The same on the Plan view's `Plan review` row (a click on it copies the URL). Depends on Q-C1. | `tui/src/tree/inbox.rs:181-195` (a `Jump::Steps(Vec<Action>)` or two `Action`s in sequence), `tui/src/tree/actions.rs:229-247`, `tui/src/plan/mod.rs:437-455` and `:100-104` (the row), `docs/commands.md` §13.2, `cli/tests/tui_tree.rs`, `cli/tests/tui_plan.rs` |
| `engine-error` | No code change before Q-C2 is answered. | none |
| `pr-failing`, `pr-changes` (owned) | None: hs-d302 removes the items; delete the owned arm (`inbox.rs:188-190` keeps only `PrReady` after the `pr-ready` fix removes it too) in the Job that lands hs-d302 (J4 above). | `tui/src/tree/inbox.rs` |
| `question` | None here: the Job that drops the kind deletes `inbox.rs:186` and `actions.rs`'s `OpenManager` use stays (a Context row click). | (that Job) |

### 4.3 Jobs (about 150 turns each, in order; both edit the same arms of `jump`, so they do not run in parallel)

- **K1 `click-engine-and-merge`** (rows `engine-down`, `pr-ready`). Tests first: `jump` of `EngineDown` is the `watch --ensure` action; `jump` of `PrReady` (owned and unowned) is the merge popup of the subject's pull request; a subject that is not `pr:<owner>/<repo>#<n>` is `None`. Files: those of the two rows above. About 100 turns, so it also takes the `plan-review` row if Q-C1 is answered "copy the URL".
- **K2 `click-job-stuck`** (row `job-stuck`, after K1). Tests first: the click runs `session open <lead>` then opens the Worker pane for the Job's latest Attempt; a Job with no Attempt opens the Lead only; moving `toggle_worker` leaves the Plan view's tests as they are. Files: the `job-stuck` row. About 150 turns: it moves code and needs a read of the Attempt.
- **K3 (only if Q-C1 asks for it)**: the `plan-review` row, and, if the User wants an open-in-cockpit view of a pull request, a popup for the unowned PR kinds (`gh pr view`), which is a new view and its own Job.

### 4.4 Unclear: ask the User

- **Q-C1 (`plan-review`, `review-requested`, unowned `pr-failing` / `pr-changes`).** For these the real action happens in a browser (plannotator's page, GitHub). A plain click can only copy the URL to the clipboard (OSC 52) and the User then modifier-clicks the row; running `xdg-open`/`open` on the cockpit's machine is no help when the cockpit runs over ssh (hs-d264), and I did not verify whether the cockpit ever runs locally. Is "copy the URL and say so" an actionable landing in hs-d306's sense, or should the cockpit open the browser (when it is local), or show the pull request inside the cockpit (a `gh pr view` popup, a new view)?
- **Q-C2 (`engine-error`).** Is the logs popup at the error's line enough, or should the click say what to do for each code? Which of the codes E1–E9 can the User fix themselves (a missing `gh` login, a full disk) and which are for the cpd maintainer? For the second kind, is a click that only shows the log acceptable, or should those not be Inbox items?
- **Q-C3 (`job-stuck`).** After K2 the click lands on the Lead plus the failed Attempt's log. Should it also select the Job in the Plan view (needs a way for the tree to tell the Plan view, for example a pane option such as the Worker pane's `@cpd_job`, `plan/actions.rs:19`), or is the Worker pane enough?
- **Q-C4 (`engine-down`).** Is a click that **starts** the engine acceptable (it changes state, and it is the only action there is), or should the cockpit only show the command to run?
