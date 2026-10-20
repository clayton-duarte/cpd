# Records recency audit: building-cpd

Research for hs-d358 (recency wins) and hs-d359 (its mechanics). It lists the pairs of records in force where a later record contradicts an earlier one. It writes no record. The Manager applies the certain decision pairs with `cpd context decide --supersedes`.

## Method

- **Source.** One read-only `cpd context show --context building-cpd` on 2026-10-20. It returned 716 records: 364 decisions, 304 notes, 27 plans, 7 briefs, 7 questions and 7 answers. 32 records carry `supersedes`, naming 30 distinct older records. Those 30 are out of force and are not listed here.
- **Records read.** All 638 decisions and notes in force (334 decisions, 304 notes), read in full in `seq` order. Plans, briefs, questions and answers are out of scope: the Job asks about decisions and notes. Only decisions and notes can supersede or be superseded under rule A.
- **How I compared.** Each record was checked against the later records on the same subject, by topic (cockpit look, dots, Inbox, PR plumbing, conflicts, CI, migrations, installs, tmux, Manager and Lead roles, ownership). Records whose ids a later record names ("supersedes", "replaces", "refines", "correction to", "settles") were followed explicitly. I did not compare all 638² pairs mechanically. I considered roughly 250 candidate pairs in detail (an estimate, not a count) and listed 176: 107 certain (26 decision pairs, 81 involving a note) and 69 ambiguous.
- **What counts.**
  - **Certain:** the newer record says it replaces or corrects the older one, or it states something incompatible on the same subject. Retiring the older record loses nothing still in force, either because the newer record restates the rest or because the rest is history that the newer record ends.
  - **Ambiguous:** the newer record overrules only part of the older one, while the older one still holds something nothing else restates. Also listed here: cases where it is unclear whether the newer rule has taken effect ("until X ships", interim, one-night exceptions).
- **One row per pair.** An older record overruled by several newer ones gets one row naming the newer record that best replaces it. The others are cited in Why.
- **Not listed:**
  - plain refinements that keep the older statement true;
  - "no owner yet" followed by an owner being named, a completion rather than a contradiction;
  - coordination notes ("whoever lands second rebases") with no later record of the outcome;
  - name-only drift, such as `cpd2` in early records or the phase `pr` renamed to `deploy` by hs-d76/hs-d131.
- **Rule A.** A note can never retire a decision. Each decision→note row below therefore needs a decision restating the change before the older decision can leave force. Those rows are flagged "needs a decision".
- **Check.** Every id below was checked by script against the same snapshot: it exists, it is a decision or note in force, and Older's `seq` < Newer's `seq`. The record count was the same before and after (716).

## Certain

### Decision pairs (apply today with `cpd context decide --supersedes <older>`)

| Older | Newer | Kinds | Why (one line) |
|---|---|---|---|
| hs-d1 | hs-d179 | decision→decision | The StatusBar is no longer a 1-row pane in cpd:main: PR statusline moves it to tmux's status line (also hs-d149, hs-d177). |
| hs-d10 | hs-d20 | decision→decision | pi-v2/ beside pi/ held only until the switch-over; hs-d20 made pi-v2/ the pi/. |
| hs-d13 | hs-d104 | decision→decision | "cpd 2's cockpit matches cpd 1 exactly" was ended: tui-visuals deliberately leaves parity (cpd 1 retired). |
| hs-d14 | hs-d21 | decision→decision | Plans no longer run on the cpd2 tmux server: the cut-over dropped socket=cpd2 and resumed the Leads on -L cpd. |
| hs-d53 | hs-d339 | decision→decision | No '…' archived rows with dotted lines any more: archived rows show muted inside the expanded Context block (also hs-d337, hs-d344). |
| hs-d71 | hs-d283 | decision→decision | "CI only for cpd; dotfiles keeps lint.yml": both workflows are removed, and clayton-duarte repos run no Actions. |
| hs-d74 | hs-d283 | decision→decision | cpd's GitHub Actions CI is removed; the local Gate is the only check. |
| hs-d79 | hs-d209 | decision→decision | The turn-start Context push to Leads is stopped (hs-d209 says it supersedes hs-d79 for Leads). The Manager's digest is restated in hs-d209 (3). |
| hs-d93 | hs-d216 | decision→decision | The acting-Manager convention (2e66cebd) ended: the official Manager 97de2471 took over. The build order is history (hs-d112). |
| hs-d94 | hs-d111 | decision→decision | Dot-work owner: hs-d111 moves it from tree-menu to tui-visuals and says it supersedes hs-d94. |
| hs-d99 | hs-d100 | decision→decision | hs-d100 replaces hs-d73 and hs-d99 (its Context-row precedence is restated there). |
| hs-d148 | hs-d209 | decision→decision | session send to an idle Lead no longer starts a turn: --wake only (hs-d209 says it supersedes hs-d148). |
| hs-d164 | hs-d337 | decision→decision | No ▸/▾ fold glyph in the Context row's first cell: the '╰─ expand/collapse' line toggles. The Manager-opening click is kept by hs-d309. |
| hs-d176 | hs-d179 | decision→decision | Draft Jobs after the lock were built by tui-visuals' PR drafts, not pi-pin (also hs-d199, hs-d201). |
| hs-d220 | hs-d289 | decision→decision | hs-d289 replaces hs-d220's procedure (merge, no local tests, plain push) with rebase, then the Gate, then push. |
| hs-d226 | hs-d228 | decision→decision | Idle and stopped glyph U+F4AA → U+F0766 (hs-d228 "final"). |
| hs-d227 | hs-d228 | decision→decision | The U+F4AA-first spinner at 250 ms is replaced by the U+F0766 frames at 100 ms (hs-d228 names "the U+F4AA spinner record"). |
| hs-d242 | hs-d278 | decision→decision | Unread is narrowed from "any working/blocked→idle" to "a finished turn that answered the User". |
| hs-d269 | hs-d305 | decision→decision | hs-d305 restates hs-d269 under hs-d289: push through the Gate (cpd pr rebase / pr push), never a plain git push. |
| hs-d271 | hs-d276 | decision→decision | The 10-minute conflict re-wake is dropped and cwake cancelled (hs-d276 says it supersedes hs-d271). |
| hs-d282 | hs-d283 | decision→decision | hs-d283 supersedes hs-d282 (1)-(3) and restates (4) (every push through cpd pr push). |
| hs-d287 | hs-d293 | decision→decision | No cpd self-install: dotfiles installs on every machine (hs-d293 says it supersedes hs-d287). |
| hs-d290 | hs-d320 | decision→decision | GitHub review requests alone no longer open an Inbox review item: only Slack-posted NEEDS_REVIEW pull requests do. |
| hs-d311 | hs-d312 | decision→decision | "Correction to hs-d311": no per-PR refresh step; the cockpit runs the full poll refresh after a merge. |
| hs-d319 | hs-d329 | decision→decision | No draft pull requests at all: hs-d329 replaces hs-d319's drafts and restates its "ready only after the Gate on a head with its base". |
| hs-d347 | hs-d348 | decision→decision | Scroll cues are right-aligned, replacing "one column right of the line"; "never cut a line" is restated. |

### Pairs involving a note (wait for cpd to let notes be superseded, rule A)

| Older | Newer | Kinds | Why (one line) |
|---|---|---|---|
| hs-n1 | hs-d20 | note→decision | The cpd 2 code is no longer only on cpd/cpd2 with tui/ on main: the switch-over moved it to main and removed tui/. |
| hs-n2 | hs-d102 | note→decision | The cockpit-statusbar assumptions are overruled: the Plan view's tabs row is gone (hs-d102) and so is the StatusBar pane (hs-d179). |
| hs-n6 | hs-d17 | note→decision | Handoff state (cpd2 binary, cpd 1 still running other Workflows) ended with the switch-over and cpd 1's retirement. |
| hs-n7 | hs-d17 | note→decision | Pending-work order (R34/R35 before the switch-over; a git store remote) is overruled: switch-over first (hs-d17); the DB store replaces ADR-0012 (hs-d23). |
| hs-n8 | hs-d21 | note→decision | Procedures (cpd/cpd2 sync, install-cpd2.sh, tmux -L cpd2) are replaced by the cut-over and the rename (hs-d19). |
| hs-n10 | hs-n15 | note→note | The cut-over hazard was settled (dotfiles PR #87 merged). |
| hs-n11 | hs-n15 | note→note | "NOT settled until machine-sync merges" became settled. |
| hs-n13 | hs-n14 | note→note | "PR #7 opened; the cut-over waits" became "PR #7 merged, cut-over script written". |
| hs-n16 | hs-n25 | note→note | dotfiles-sync now has ~/.local/bin on PATH and applies. |
| hs-n18 | hs-n25 | note→note | The PATH gap it verified is fixed. |
| hs-n19 | hs-n25 | note→note | pro's cpd 1 symlink was removed and cpd installed. |
| hs-n20 | hs-n25 | note→note | air's cpd 1 symlink was removed and cpd installed. |
| hs-n22 | hs-n25 | note→note | "dotfiles-sync.service sets no Environment" no longer holds. |
| hs-n26 | hs-n28 | note→note | pro's stuck pi-config rebase was resolved (origin's version taken). |
| hs-n31 | hs-d245 | note→decision | hs-d245 replaces hs-n31's "CPD = Context, Plan, Deploy"; the draft flow was decided by hs-d75. |
| hs-n33 | hs-n43 | note→note | The interim e2e fixture was removed ("hs-n33 resolved"). |
| hs-n36 | hs-n40 | note→note | "Unverified what they expect, need asking" was settled: animate the running state. |
| hs-n44 | hs-n47 | note→note | The rpiv:ask-user:blocked idea is replaced by counting ui_prompt_start..end. |
| hs-n45 | hs-n59 | note→note | pr-poller was replanned on R34 (migration 5, prs.pull_request), replacing the pre-R34 plan. |
| hs-n49 | hs-n56 | note→note | tree-menu's follow-up backlog was re-scoped (hs-d111): most items moved to tui-visuals. |
| hs-n50 | hs-n56 | note→note | Item 9 (open the new Lead after create) moved to tui-visuals. |
| hs-n52 | hs-n103 | note→note | The pr rebase merge gap is built (cross-repo plumbing: fx-merge-ff, fx-pr-push). |
| hs-n56 | hs-d117 | note→decision | The adapter's missing Event kinds were already fixed in #11. Reopening is built by plan-reopen, not reopen-done. |
| hs-n57 | hs-n68 | note→note | "tree-menu cannot append yet" no longer holds: plan-reopen #12 merged. |
| hs-n59 | hs-n69 | note→note | pr-poller's migration is 0006, not 5 (renumbered after #12). |
| hs-n60 | hs-n115 | note→note | The all-merged-again gap is fixed (fx-all-merged merged as #27). |
| hs-n67 | hs-n103 | note→note | The merge-step rebase gap is built (fx-merge-ff). |
| hs-n73 | hs-n74 | note→note | "Correction to hs-n73": install times and schema corrected. |
| hs-n74 | hs-n85 | note→note | "Inferred, not verified" was confirmed, and the bug is fixed for good by fx-follow. |
| hs-n79 | hs-n100 | note→note | The three guards it proposed are on main (#21). |
| hs-n80 | hs-n103 | note→note | Its gaps are closed: the install guard is on main, and the ladder fix is in pi-pin's ladder PR. |
| hs-n83 | hs-n103 | note→note | The legacy-branch bug is fixed by plan-reopen's rename (verified). |
| hs-n99 | hs-n284 | note→note | "The product gap stays open" (tmux 3.4) no longer holds: hs's cpd server now runs tmux 3.8. |
| hs-n101 | hs-n115 | note→note | "The hs-d139 skip is not on main" no longer holds: #27 merged it. |
| hs-n104 | hs-d294 | note→decision | Its pending steps are overruled: the User's restart-tmux.sh run became hold-drain-restart, and PR drafts was dropped (hs-d199). |
| hs-n124 | hs-n170 | note→note | The merged-PR clean-up now takes the per-repo lock on main (#48). |
| hs-n131 | hs-n140 | note→note | "main is red" became "main is green again" (#30). |
| hs-n132 | hs-n140 | note→note | Same red main, fixed by #30. |
| hs-n133 | hs-n140 | note→note | Same red main, fixed by #30. |
| hs-n134 | hs-n140 | note→note | The main-red fix landed through manager #30, not pi-pin's PR drafts. |
| hs-n135 | hs-n140 | note→note | "Until #30 merges, other PRs fail": #30 merged. |
| hs-n136 | hs-n140 | note→note | "Until pi-test-timing lands on main": it landed with #30. |
| hs-n141 | hs-n148 | note→note | "Cause unknown" was found: the wrappers exited on store_too_new. |
| hs-n142 | hs-n224 | note→note | "hs has no tmux 3.8" no longer holds: it was installed at 00:55Z. Its "User step" was also superseded by hs-d208. |
| hs-n146 | hs-n152 | note→note | The conflict flag waited for pr-conflicts: #42 landed. |
| hs-n150 | hs-d218 | note→decision | "Distillation is NOT planned" was settled: PR distill stays planned as Epic B's pilot. |
| hs-n156 | hs-n163 | note→note | "The overlap is gone." |
| hs-n159 | hs-n160 | note→note | "Cause unverified" became a verified cause (clean-up under a running Gate). |
| hs-n160 | hs-n161 | note→note | "Owner: dev-speed" no longer holds: already fixed in cross-repo PR fixes. |
| hs-n161 | hs-n170 | note→note | tui-visuals' cleanup-lock landed first, so cross-repo drops its fx2-cleanup-lock copy. |
| hs-n162 | hs-n170 | note→note | The duplicate was resolved: the first to land (#48) stays. |
| hs-n164 | hs-n171 | note→note | "Correction": the adapter reload does work; the cause was a missed idle gap. |
| hs-n165 | hs-n171 | note→note | Same correction. |
| hs-n178 | hs-n199 | note→note | The E5 test failures on main were fixed (#62, now on main). |
| hs-n179 | hs-n199 | note→note | main's missing Text import was fixed (#62). |
| hs-n180 | hs-n199 | note→note | Same build break, fixed. |
| hs-n181 | hs-n199 | note→note | Same E5 failures, fixed on main. |
| hs-n184 | hs-n199 | note→note | Same build break, fixed (and the owner was tui-dots, hs-n197). |
| hs-n185 | hs-n199 | note→note | Same build break, fixed. |
| hs-n191 | hs-n243 | note→note | Legacy is 0001-0014, not 0001-0013 (#71 added 0014). |
| hs-n192 | hs-n197 | note→note | "Correction to hs-n192": tui-dots owns the E5 fix; pr-conflicts cancelled its Job. |
| hs-n193 | hs-n197 | note→note | Same correction (the Text import fix is tui-dots'). |
| hs-n224 | hs-n284 | note→note | The 3.8-client/3.4-server attach failure is gone: the server is now 3.8. |
| hs-n240 | hs-n241 | note→note | "Corrects hs-n240": the Lead did not ignore the notice; delivery delayed it. |
| hs-n247 | hs-d283 | note→decision | "No PR can be CLEAN until billing is fixed" is moot: there is no Actions CI any more, and only mergeability matters. |
| hs-n249 | hs-d283 | note→decision | Same billing block, moot. |
| hs-n253 | hs-d283 | note→decision | Same billing block, moot. |
| hs-n254 | hs-d283 | note→decision | Same billing block, moot. |
| hs-n255 | hs-d283 | note→decision | Same billing block, moot. |
| hs-n256 | hs-d283 | note→decision | Same billing block, moot. |
| hs-n272 | hs-n273 | note→note | "Bug, urgent" was refined to "a one-time upgrade gap, not a current bug; no fix Job". |
| hs-n274 | hs-n290 | note→note | The merge-order advice was replaced by the stack. |
| hs-n278 | hs-n281 | note→note | "Correction to hs-n278": 'cpd config' does not open the store. |
| hs-n282 | hs-n284 | note→note | The pending tmux restart ran (server now 3.8). |
| hs-n285 | hs-n290 | note→note | hs-n290 says it replaces hs-n285 (the landing plan). |
| hs-n289 | hs-n290 | note→note | hs-n290 says it replaces hs-n289 (the line); see also hs-d334. |
| hs-n290 | hs-n296 | note→note | The stack was re-made as "THE LINE" after #136, #144, #154 and #149 merged. |
| hs-n303 | hs-n304 | note→note | "Pending: retire the timer" became "hs-n303 done". |
| hs-d131 | hs-n72 | decision→note | The phase rename is migration 0008, not 0007 (0007 imports the records). Needs a decision. |
| hs-d233 | hs-n243 | decision→note | The numbered-migration freeze is over (#75 merged; named migrations live on every machine). Needs a decision. |
| hs-d281 | hs-n257 | decision→note | The one-night overnight merge watcher ended at 12:56Z. Needs a decision (hs-d287/hs-d293 cover installs). |

## Ambiguous

| Older | Newer | Kinds | Why (one line) | Question for the User |
|---|---|---|---|---|
| hs-d2 | hs-d177 | decision→decision | reset-layout "is not a CLI command" vs the internal `cpd tui click reset`; the StatusBar's 1-row pane size is gone too. | Retire hs-d2, keeping only its layout sizes in a new decision? |
| hs-d4 | hs-d20 | decision→decision | docs/v2 on cpd/cpd2 is gone (moved to docs/), but "amend the contract before the code" still holds. | Restate the docs-first rule with today's paths and retire hs-d4? |
| hs-d5 | hs-d338 | decision→decision | "Rust-only Jobs verify with test --workspace" is overruled (targeted tests; hs-d341's red/green checks at submit); the Gate rules still hold. | Retire hs-d5 and restate its Gate/quick-check rule? |
| hs-d6 | hs-d70 | decision→decision | Review by forking a side Session became review Jobs (B3), and no local review is required (hs-d171). | Is "Lead never reviews code in its own session" still a rule worth keeping alone? |
| hs-d8 | hs-d301 | decision→decision | "Every batch is reviewed by the User before it starts" vs decided work queued with no review. | Retire hs-d8 now that hs-d301 and hs-d239 define when a review is needed? |
| hs-d30 | hs-d258 | decision→decision | "The merge step pushes inside the per-repo lock" vs no per-repo lock; the push-after-merge rule stays. | Retire hs-d30 (hs-d42 restates its push rules)? |
| hs-d38 | hs-d210 | decision→decision | "Each Lead's pi restarts at next idle on install" vs the wrapper re-exec at once, the adapter reload (hs-d158) and a pi restart only for a new pi. | Retire hs-d38, keeping hs-d33 as the install rule? |
| hs-d40 | hs-d264 | decision→decision | "dotfiles forwards the whole port range from the Macs" vs links using the host IP and the forwards dropped; the port range itself stays. | Retire hs-d40 and restate the range? |
| hs-d42 | hs-d258 | decision→decision | Creation "pushed -u" and the push "inside the per-repo lock" vs --no-track and no repo lock; reuse/branch_exists stay. | Restate hs-d42 without -u and the lock? |
| hs-d48 | hs-d264 | decision→decision | Interim PLANNOTATOR_PORT range export and Mac forwards vs host-IP links (and hs-d204 O1 drops the export). | Has the interim ended; retire hs-d48? |
| hs-d60 | hs-d100 | decision→decision | hs-d100's ● cases drop "an idle Lead whose last turn ended with a question"; the owner (hardening) changed (hs-d81, hs-d111); questions go through dialogs (hs-d306). | Retire hs-d60? |
| hs-d62 | hs-d142 | decision→decision | Worktree removal moved from archive to PR merge ("replaces hs-d62's worktree timing"); pane, pi and port teardown at archive remain. | Restate the remaining archive teardown and retire hs-d62? |
| hs-d66 | hs-d101 | decision→decision | "The tree shows titles" was superseded by "the tree shows ids"; the id/title model and the rename commands remain. | Retire hs-d66 after restating ids vs titles? |
| hs-d67 | hs-d90 | decision→decision | hs-d90 "supersedes hs-d67 where they differ" (y/N, chat-slot archive, renaming archived rows); the archive commands and their refusals remain. | Merge hs-d67 and hs-d90 into one current archive decision? |
| hs-d75 | hs-d171 | decision→decision | Deploy no longer starts with Reviewing before the Gate; the Context and Plan steps stand. | Restate the flow and retire hs-d75? |
| hs-d81 | hs-d94 | decision→decision | Routing of the adapter changes after cross-repo: the dot fix left that Plan; Context updates were built by manager. | Retire as history? |
| hs-d82 | hs-d122 | decision→decision | Auto-archive on quit: the owner moved to tree-menu, and the quit kinds were settled by hs-d91. | Retire hs-d82, keeping hs-d91 as the rule? |
| hs-d86 | hs-d238 | decision→decision | "The Manager acts only when the User talks to it" vs Leads waking it with --wake (hs-d238, hs-n295) and engine errors and stuck Jobs sent to it (hs-d314, hs-d321). | Is hs-d86 retired, or does it still bound the Manager? |
| hs-d90 | hs-d297 | decision→decision | "Archive needs no y/N" is superseded by the popup confirmation; the other parts stand. | Restate the archive rules in one decision? |
| hs-d100 | hs-d228 | decision→decision | Glyphs replaced (hs-d228), the Context row now shows its Manager (hs-d215) and white narrowed (hs-d278); the case colours stand. | Restate the current dot scheme and retire hs-d100? |
| hs-d102 | hs-d111 | decision→decision | Owner tree-menu → tui-visuals; the copy action, no tabs and the chat title stand. | Keep hs-d102 for its content? |
| hs-d103 | hs-d111 | decision→decision | Owner changed; the rule (open the new Lead in the chat) stands. | Keep hs-d103 for its rule? |
| hs-d108 | hs-d305 | decision→decision | "Never editing product code" vs a Lead committing the code a conflict resolution needs; Leads also no longer open PRs (hs-d340). | Restate the Lead's tool rules? |
| hs-d112 | hs-d130 | decision→decision | Plan manager's scope: auto-archive moved to tree-menu, and "review before the Gate" was dropped (hs-d171). | Retire hs-d112 as history (Plan manager merged)? |
| hs-d124 | hs-d349 | decision→decision | In-slot headers are now pills with muted ─ and the count on the right (hs-d337, hs-d349); "first section has no header" is unclear. | Which parts of hs-d124 still hold? |
| hs-d125 | hs-d257 | decision→decision | Batch 3 gave the git and Jira sections headers; hs-d257 removes them. | Retire hs-d125 as a finished batch? |
| hs-d126 | hs-d171 | decision→decision | Batch B4 "Reviewing before the Gate" was dropped and B2 changed (hs-d130). | Retire hs-d126 as a finished plan? |
| hs-d134 | hs-d145 | decision→decision | The pi pin check runs at apply on each machine, not as PR checks ("refines hs-d134"). | Retire hs-d134, keeping hs-d144/hs-d145? |
| hs-d135 | hs-d322 | decision→decision | StatusBar buttons as grey reversed blocks and a "no-row StatusBar to BACKLOG" vs powerline pills on tmux's status line; tooltips dropped (hs-d179). | Retire hs-d135's StatusBar parts? |
| hs-d139 | hs-d258 | decision→decision | pr push "under the per-repo lock" and the tamper "known limit" (hs-d189) are replaced; the merge-step fast-forward stays. | Restate hs-d139 without the lock and the limit? |
| hs-d143 | hs-d161 | decision→decision | "Everything else is listed, never deleted" vs deleting clean cpd 1 leftovers not on main; the cleanup ran (hs-n98). | Retire both as a finished one-time cleanup? |
| hs-d150 | hs-d228 | decision→decision | The dots went to Plan tui-dots (hs-n92 says followups changes no dots); 250 ms frames → 100 ms. | Retire hs-d150? |
| hs-d165 | hs-d318 | decision→decision | Inbox scope: stuck work and engine errors go to the Manager, and question items are dropped (hs-d306, hs-d321). | Restate the Inbox kinds and retire hs-d165? |
| hs-d166 | hs-d294 | decision→decision | "The server moves when the User runs restart-tmux.sh" → hold-drain-restart by cpd (hs-d259, hs-d294); the tmux ≥ 3.6 requirement and the pinned 3.8 stand. | Restate the tmux requirement and retire hs-d166? |
| hs-d167 | hs-d317 | decision→decision | Inbox clicks changed (copy URL; the engine-error click superseded by hs-d318); order and snooze stand. | Which parts of hs-d167 still hold? |
| hs-d168 | hs-d321 | decision→decision | The stuck-Job and engine-error items left the Inbox; '+N more' inline is replaced by hs-d307/hs-d309; the reconcile step stands. | Restate hs-d168's reconcile rule alone? |
| hs-d177 | hs-d179 | decision→decision | The StatusBar move went to tui-visuals' PR statusline (hs-n116); the rest of hs-d177 was built by tui-dots. | Retire hs-d177 as a finished batch? |
| hs-d183 | hs-d215 | decision→decision | The Context row shows its Manager's state, not the most urgent Session; the Session-row rule stands. | Retire only after restating the Session rule? |
| hs-d185 | hs-d302 | decision→decision | Owned PRs' failing/changes no longer open items; review requests changed (hs-d320). | Restate the PR item rules? |
| hs-d187 | hs-d317 | decision→decision | "No copy fallback" vs Inbox rows that copy the URL on click. | Retire hs-d187's "no copy fallback" only? |
| hs-d188 | hs-d302 | decision→decision | (3) "a conflicting PR counts as failing in the Inbox" is overruled for owned PRs. | Restate hs-d188 without (3)? |
| hs-d191 | hs-d223 | decision→decision | "The link is a ↗ glyph" vs the whole text as the link (hs-d219, hs-d223) and the cue glyph md-open_in_new (hs-d235). | Retire hs-d191, keeping its URL rule? |
| hs-d211 | hs-n243 | decision→note | The numbered-migration race rules are moot once named migrations are live. Needs a decision. | Retire hs-d211 with a decision? |
| hs-d232 | hs-d327 | decision→decision | Pulse v2 drops the dot, ✓N and the ✓ icon and colours the bars; the window, quiet rule and clicks stand. | Restate the current Pulse design? |
| hs-d235 | hs-d337 | decision→decision | The tree's fold and archived-toggle glyphs are gone in the new look; the other glyphs stand. | Retire only those glyph lines? |
| hs-d252 | hs-d268 | decision→decision | hs-d268 "supersedes hs-d252 (1)"; items (2)-(5) stand. | Restate (2)-(5) and retire hs-d252? |
| hs-d257 | hs-d333 | decision→decision | The memory icon U+F035B → U+EFC5, and GPU added; titles and scroll offsets stand. | Keep hs-d257 for (2) and (3)? |
| hs-d259 | hs-d294 | decision→decision | "Attach with the server's binary and retry at the next bare cpd" vs a hold and drain before the restart. | Restate bare cpd's update follow-ups? |
| hs-d270 | hs-d289 | decision→decision | `pr sync` merges origin/<base> and pushes plainly vs rebase and push through the Gate; (1) was superseded by hs-d276. | Is cpd pr sync still wanted, and with merge or rebase? |
| hs-d276 | hs-d280 | decision→decision | followUpMode 'all' for Leads was dropped (steer only); dropping cwake stands. | Retire hs-d276, keeping hs-d280? |
| hs-d286 | hs-d339 | decision→decision | '⦁ cpd …' / '└ Click to expand' vs '╭⦁ title', '│', '╰─ expand/collapse' (hs-d309, hs-d339). | Retire hs-d286's look, keeping the renderer mechanism? |
| hs-d305 | hs-d357 | decision→decision | Update Jobs resolve conflicts, but "until update Jobs ship, Leads keep resolving inline"; hs-d181/hs-d289 are the same interim. | Retire hs-d305 now or when update Jobs ship? |
| hs-d308 | hs-d349 | decision→decision | "One empty row above and below each section title" vs none below (hs-d337, hs-d349). | Which spacing holds for non-fold sections? |
| hs-d309 | hs-d337 | decision→decision | hs-d337 replaces hs-d309's ┌/└ details; hs-d349 adds the arrow toggles; "only the toggle line toggles" stands. | Restate the fold language and retire hs-d309? |
| hs-d316 | hs-d318 | decision→decision | Q2 was superseded by hs-d318 and Q1 (job-stuck red) by hs-d321; Q3-Q6 stand. | Restate Q3-Q6 and retire hs-d316? |
| hs-d317 | hs-d318 | decision→decision | The engine-error click was superseded and the job-stuck click is moot (hs-d321); the other clicks stand. | Restate the click table? |
| hs-d322 | hs-d361 | decision→decision | StatusBar "pills joined by a grey ─ line" vs hs-d361's restated "agnoster-style segments" (from the older hs-d266). | Which StatusBar look holds: pills on a line, or agnoster segments? |
| hs-d324 | hs-d334 | decision→decision | "One PR in flight per area, stacked" vs open at once and stack only where two would conflict; the Manager's planning duty stands. | Retire hs-d324 (2)? |
| hs-d326 | hs-d329 | decision→decision | "A conflict sends it back to draft" is replaced; stacking replacing drafts for ordering stands. | Restate the stacking rule alone? |
| hs-d328 | hs-d364 | decision→decision | The interim timer is retired (hs-n304), and the tree's engine-restart net is dropped; the reliability rule stands. | Restate the reliability rule and retire hs-d328? |
| hs-d329 | hs-d335 | decision→decision | "Closed and reopened if it can't be ready soon" vs close never used; no drafts and the CI-repo open question stand. | Is hs-d329's open question (pr.gate ci needs an open PR) still open? |
| hs-d334 | hs-d340 | decision→decision | The Lead-opens step is superseded by an open Job cpd adds, only once that ships (#144 stays until then). | Retire hs-d334 now or when open Jobs ship? |
| hs-d336 | hs-n302 | decision→note | The interim flock wrappers are dropped (cpd's built-in slots since #153); the 2-Gate limit and the order stand. Needs a decision. | Restate the Gate-slot rule without the wrappers? |
| hs-d337 | hs-d349 | decision→decision | The Inbox title and '╰─ expand' were replaced (hs-d349); the fold covers only archived rows (hs-d344); the tree row shapes stand. | Restate the tree look in one decision? |
| hs-d343 | hs-n295 | decision→note | The Manager's overnight merge delegation: the User was back at 12:53Z and merged; it says "the User can take it back". | Has hs-d343's delegation ended? |
| hs-d346 | hs-d349 | decision→decision | Inverted text on a heavy ━ line vs a pill with a muted ─ line (an assumption for every section). | Do all in-pane titles take hs-d349's pill? |
| hs-n30 | hs-d285 | note→decision | The PR title '<plan title> (<key>)' is overruled; the rest of the R34 contract facts stand. | Retire hs-n30 as history? |
| hs-n102 | hs-n284 | note→note | "Do not run restart-tmux.sh before ensure-flake lands": the server now runs 3.8. | Did the Pane.pid-optional fix land? |
| hs-n248 | hs-d283 | note→decision | The billing block is moot, but its dotfiles local-check facts are cited by hs-d283. | Retire hs-n248? |
