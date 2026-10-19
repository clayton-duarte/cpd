# Cockpit proposal: what to build first

Job `insp-proposal` (Attempt `insp-proposal-a1`), Plan `tui-visuals`, PR `inspiration`. Research only: no code changed, no Job planned (the Lead turns the chosen phases into Jobs). This file is the synthesis of the five research files on this branch and adds nothing they do not source, except the facts checked in §0.2:

| File | What it holds | Idea ids |
|---|---|---|
| [survey.md](survey.md) | 20 TUIs, 37 ideas across the four pain points | `I-01`…`I-37` |
| [attention.md](attention.md) | finding what needs the User | `A-01`…`A-14` |
| [progress.md](progress.md) | following progress | `P-01`…`P-13` |
| [navigation.md](navigation.md) | getting around, mouse-first, and the pane model | `N-01`…`N-13` |
| [looks.md](looks.md) | looks and polish | `L-01`…`L-12` |

The User's four pain points are tagged as in those files: **F** finding what needs him, **P** following progress, **N** getting around, **L** looks and polish. Effort: **S** a day or less (one view, or a few tmux values), **M** a few days (several views, or a view plus a command, an engine step or a tmux hook), **L** a week or more (crossing the engine and several views). Every effort is a size estimate, not a measurement.

## 0. In short

### 0.1 The answer

1. **Build the "what needs me" layer first** (Phase 1): a numbered, coloured strip of the Inbox in the status line with `▶ next` and `↩ last`, a plain click on every Inbox row, gauges instead of the five counters, a stepper that shows each PR's next step, a quieter frame and a glyph per kind of need. All are S or a small M, and the data exists on `main` today.
2. **Then make progress visible without opening anything** (Phase 2): a Workers board in the Deploy column, finished PRs folded, a one-line cause under every need, changes that flash and calm.
3. **Then put everything one click away** (Phase 3): right-click menus on every object, a go-to picker, pane headers with zoom and a breadcrumb, the triage popup, hover feedback.
4. **Then the big picture** (Phase 4): an Overview of every Plan with a time view, and the optional shaded look.
5. **Keep tmux panes.** Every idea here fits inside tmux; owning the terminal would put the chat slot's verified keys and links at risk ([navigation.md §3](navigation.md#3-the-pane-model-tmux-panes-or-one-app)).
6. **Two facts unblock a lot** (§0.2): the live cockpit runs tmux **3.4**, while tmux **3.8** is already installed on the homeserver; and the status-line PR already has the engine feed the status line through a tmux option, which settles how the needs-you strip gets its data.

### 0.2 Facts checked by this Job (not in the theme files)

| Fact | How it was checked |
|---|---|
| The Inbox section of the tree (up to 3 rows, `+N more`) and the StatusBar's `inbox <n>` counter ("A click on the counter does nothing") are **on `main`**, not only in flight as attention.md says (it read them on `origin/cpd/inbox/cockpit`) | `git show main:docs/commands.md` §13.2 ("Inbox section", the `statusbar` paragraph); merged as #84 (`2551164`) |
| The live cockpit's tmux server is **3.4** (`/usr/bin/tmux`, Ubuntu 24.04) | `tmux -L cpd display -p '#{version}'` printed `3.4`; `readlink /proc/<pid>/exe` of the `-L cpd` processes is `/usr/bin/tmux` |
| A **tmux 3.8** is installed for the User at `~/.local/opt/tmux-3.8` and is first on his `PATH` (`~/.local/bin/tmux`) | `which tmux`, `ls -la ~/.local/bin/tmux`, `tmux -V` |
| The status-line PR (`cpd/tui-visuals/statusline`, in flight) makes the StatusBar tmux's status line and has **the engine write `@cpd_workers`** each tick (`watch.tick_seconds`, 5 s), "a deliberate deviation" from §17's "no tmux option carries state" | `git show origin/cpd/tui-visuals/statusline:docs/commands.md` (§12 "The `@cpd_workers` step", §13.2 "The StatusBar", §17) |
| `job retry` takes `--tier` and no note; there is no command that merges a pull request | `docs/commands.md` §5.4 (`cpd job retry <key> --tier <T> [--plan <id>]`), §9 |

## 1. The vision

The cockpit becomes **a calm screen that points**. The frame goes quiet and grey; colour and bold are spent only on things that need the User. The bottom line always says, by its colour, whether anything needs him, and lists what, numbered, one click each. Every Plan, PR and Worker shows **how far it is and what it is doing now** where it already sits: a gauge on the tree row, a stepper on the PR box, a two-line row per Worker. Anything named on screen is a click target with a right-click menu, and anything can be reached from one picker. It stays tmux panes with ratatui views and pi in the chat slot, mouse-first.

The mockup is 215×62, today's geometry (Context column 43, centre 127, Deploy column 43, the Plan view's 12 rows, the status line on row 62; `docs/commands.md` §13.1), with all four phases applied. Colour is lost in ASCII, so shapes stand in for it, as in the theme files: `●` red, `◐` yellow, `◆` magenta, `○` white or idle, `⠹ ⠼ ⠴ ⠋` working spinners, `◖text◗` a rounded-cap segment (hs-d266), `⌂ ⎇ ⑂` the Context, Plan Lead and fork icons. Grey `─ │` lines are the quiet frame; heavy `━ ┃ ┣ ┫` stand for the blue edge of the focused pane (the chat). The scenario is invented: Context `api` with Plans `schema` (in the chat slot), `auth`, `search`; Context `web` with Plan `login`.

```text
─◖ Context ◗──────────────◖ ●2 ◐1 ◆2 ◗─ ⋯ ─┬─◖ Plan ◗────────────────────────────────────────────────────────────────────────────◖ schema · 6/10 merged · dispatch ◗─ ⤢ ⋯ ─┬─◖ Deploy ◗──────────────◖ 1 ready ◗─ ⤢ ⋯ ─
                                           │                                                                                                                               │                                           
 ▾ ○ ⌂ api                                 │ ╭─ schema · api → main · dispatch ─────────────────────────╮                                   ╭─ schema/links · H2 · a2 ───╮ │ ⌂ homeserver                              
⏵  ├ ● ⎇ schema              ▰▰▰▱▱ 6/10 ✗1 │ │ ◖✓ types │S1◗──◖✓ gql │S2◗──◖⠹ ui │S2│a2◗                │                                   │ ✗ failed 21m · cause build │ │ cpu ▰▰▰▱▱▱ 38%  ⣀⣤⣶⣿⣷⣤⣀⣠⣴⣶⣤⣀⣠⣴            
   ├ ● ⎇ auth                    ▰▱▱▱▱ 1/5 │ ╰─ ▰▰▰▱▱ 2/3 ▸ ○ open ▸ ○ ci ▸ ○ review ▸ ○ merge ───┬──╯                                      │ a1 ✗ 48m ago · clippy      │ │ mem ▰▰▰▰▱▱ 61%  ⣀⣀⣠⣤⣤⣴⣴⣶⣶⣶⣶⣶⣶⣶            
   ├ ◆ ⎇ search                  ▱▱▱▱▱ 0/3 │                                                      ▼                                         │ → cargo clippy -p docs     │ │                                           
   └ ○ ⑂ schema·7f3a                       │ ╭─ docs · api → main · dispatch ─────╮  ╭─ web · web → main · waiting ───╮                     │ ← exit 1: unused import    │ │ ──◖ workers 3/4 ◗──────────────────────── 
 ▾ ○ ⌂ web                                 │ │ ◖✓ readme │H2◗──▌◖✗ links │H2│a2◗▐ │  │ ◖○ page │S1◗──◖○ list │S2◗     │                     │                            │ │ ✗ schema/links  H2 a2          failed 21m 
   └ ◆ ⎇ login                   ▰▰▱▱▱ 2/6 │ ╰─ ▰▰▱▱▱ 1/2 ✗1 ▸○open ▸○ci ▸○merge ─╯  ╰─ needs schema ▸○open ▸○ci ─╯                         │ [Worker] [Retry ▾] [Copy]  │ │   exit 1 · cargo clippy           ↻ retry 
 ▸ ○ ⌂ ops                         2 plans │                                                                                                ╰────────────────────────────╯ │ ⠹ schema/ui     S2 a2           7m ▰▰▰▰▱▱ 
 ▸ ○ ⌂ dotfiles                     1 plan │ ▸ ✓ core · api → main · ◖#28 merged◗ ↗   ▰▰▰▰▰ 3/3          (folded: a click opens it)                                        │   → edit src/ui/Login.tsx                 
 …                                         │                                                                                                                               │ ⠼ auth/session  S2 a1           3m ▰▰▱▱▱▱ 
                                           │ ── since you looked 14:52 · 6 changes ▸ ──                                                                      ◖ fold done ◗ │   → cargo test -p auth                    
                                           ┣━◖ api ▸ schema ▸ Lead · dispatch ◗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━◖ ● ✗ schema/links failed 21m ◗━ ⤢ ⋯ ✕ ━┫ ⠴ login/form    S2 a1        14m ▰▰▰▰▰▰ ! 
                                           ┃                                                                                                                               ┃   slow: typical S2 is 9m (est.)           
                                           ┃  you    schema/links failed twice. What is going on?                                                                          ┃ ○ 4 queued · 0 waiting                    
                                           ┃                                                                                                                               ┃                                           
                                           ┃  agent  → cpd job show links --plan schema                                                                                    ┃ ──◖ PRs ◗──────────────────────────────── 
                                           ┃         ← failed · attempt 2 · cause build · cargo clippy -p docs exit 1 (unused import in src/links.rs)                      ┃ ● api#34 auth                           ↗ 
                                           ┃         The Worker added the import in a1 and dropped its last use in a2. I can retry at H2 with a                            ┃   ✓ci ◖changes requested◗ ○merge          
                                           ┃         sharper Spec, or at S2 as it is. Which do you prefer?                                                                 ┃ ◐ api#30 hotfix                         ↗ 
                                           ┃                                                                                                                               ┃   ✓ci ✓review ○merge · ready              
                                           ┃                                                                                                                               ┃   web#12 login                          ↗ 
                                           ┃                                                                                                                               ┃   ◌ci ○review ○merge                      
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃ ──◖ git ◗──────────────────────────────── 
                                           ┃                                                                                                                               ┃  (future)                                 
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
 ──◖ Inbox 5 · 2 new ◗──────────────────── ┃                                                                                                                               ┃                                           
 ▌● ✗ schema/links failed              21m ┃                                                                                                                               ┃                                           
 ▌  a2 · clippy: unused import             ┃                                                                                                                               ┃                                           
 ▌  [retry ▾] [Worker] [snooze ▾] [✕]      ┃                                                                                                                               ┃                                           
 ▌● ✎ auth api#34 changes requested     8m ┃                                                                                                                               ┃                                           
 ▌  ana: "split the token refresh"         ┃                                                                                                                               ┃                                           
  ◐ ✓ api#30 ready to merge            40m ┃                                                                                                                               ┃                                           
    checks 9/9 · approved                  ┃                                                                                                                               ┃                                           
  ◆ ? login asks you                    3m ┃                                                                                                                               ┃                                           
    "which test account for e2e?"          ┃                                                                                                                               ┃                                           
  ◆ ☰ search plan review r1            14m ┃ ╭───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮ ┃                                           
    4 Jobs to approve                      ┃ │ H2, with a sharper Spec please▌                                                                                           │ ┃                                           
                                           ┃ ╰───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯ ┃                                           
  ○ 3 resolved in the last hour ▸          ┃  ~/projects/api · claude-opus · ctx 34%                                                                                       ┃                                           
                                           ┃                                                                                                                               ┃                                           
                                           ┃                                                                                                                               ┃                                           
 ⧉ ? ▤ ⌕ │ ◖● 5 need you◗ ▶ next ↩ last │ ●1 ✗ schema/links  ●2 ✎ api#34  ◐3 ✓ api#30  ◆4 ? login  +1 │                                                        ▰▰▰▱▱▱▱▱ 9/24 jobs ✗1 │ ●●●○ 3/4 · 1 waiting · 4 queued 
```

Where each idea shows (rank numbers of §2):

| Area | What you see | Ideas |
|---|---|---|
| Status line, left | `⌕` picker; `◖● 5 need you◗` filled with the worst urgency; `▶ next`, `↩ last`; the first four needs, numbered, each with a kind glyph; `+1` | #1, #7, #11 |
| Status line, right | all Jobs as one gauge with the failed count; one cell per Worker slot | #3 |
| Pane headers | a badge per pane (`●2 ◐1 ◆2`, `schema · 6/10 merged`, `1 ready`); `⤢` zoom, `⋯` menu, `✕` close; the chat header is a breadcrumb and carries this Lead's open need | #12 |
| Context column | a gauge per Plan instead of `queued⋅running⋅merging⋅failed⋅merged` | #3 |
| Inbox section | `2 new` marked `▌`; a dim cause line under each need; verb chips on the selected row; `3 resolved` tail | #2, #9, #13, #14 |
| Plan view | rounded PR boxes; each bottom border is the PR's stepper; the failed pill ringed `▌ ▐`; the selected Job's card at the right; the finished `core` PR folded to one row; `since you looked` strip | #4, #6, #8, #10, #14 |
| Deploy column | host gauges and sparklines; the Workers board with a slow-Job warning; PR rows with stepper and chips | #4, #5 |

## 2. Ranked ideas

Rank is the expected gain in the User's efficiency per unit of effort, with **F** first because it is the pain he named first. Each idea merges and deduplicates ideas from the theme files (column "Merges"); the theme file holds the full sources, quotes and alternatives, so each entry below cites the main sources only.

| # | Idea | Pain | Effort | Depends on | Phase | Merges |
|---|---|---|---|---|---|---|
| 1 | Needs-you strip: numbered, coloured by the worst need, `▶ next`, `↩ last` | F, N | M | status-line PR; Inbox (on `main`); D2 | 1 | A-01, A-02, N-01, N-04, L-02, I-23 |
| 2 | One click convention for every need, and its verb on the row | F, N | S / M | Inbox | 1 (S), 2 (M) | A-05, N-06 |
| 3 | Gauges instead of the five counters; slot strip | P, F | S | D7 | 1 | P-03, P-11 |
| 4 | PR lifecycle stepper and state chips | P, F, L | S | hs-d266 pill helper | 1 | P-02, L-05, I-04 |
| 5 | Workers board in the Deploy column | P, F | M | | 2 | P-01, P-10, I-14, I-30 |
| 6 | Quiet frame, loud signal; rounded inside | L, F | S | D4 (reverses hs-d124) | 1 | L-01, L-07, I-28 |
| 7 | Shape as well as colour: a glyph per kind; palette audit | L, F | S | hs-d266 icons; D5 | 1 | A-09, L-03 |
| 8 | Fold finished work; a fold never hides a need | P, F, L | S | D8 | 2 | P-05, I-29, A-13, I-15 |
| 9 | Say what it wants: a cause line under every need | F, P | S / M | Inbox (one new field) | 2 | A-03, N-08, P-12 |
| 10 | Right-click menus on every object, and a Job card | N, P | M | | 3 | N-03, I-33, P-06, I-05, I-19 |
| 11 | A go-to picker that needs no typing | N, F | S spike / M | | 3 | N-02, I-06, I-17, I-19 |
| 12 | Pane headers as toolbars: zoom, menu, breadcrumb, badge | N, F, L | S on tmux ≥ 3.7, M on 3.4 | D3; hs-d257 | 3 | N-05, L-06, A-08, I-02, I-03, I-12 |
| 13 | Triage popup with the cause, verbs and a resolved tail | F, N | M | Inbox | 3 | A-04, A-12 |
| 14 | Motion means change: flash, new versus seen, a slow pulse for waiting | P, F | M | | 2 | L-04, A-07, P-07, L-12 |
| 15 | Hover and selection tints | N, L | S / M | | 3 | L-08, N-09 |
| 16 | The Event log as a feed: clickable, newest first, next problem, floating | P, F, N | S / M | D3 for floating | 2 (S), 3 (M) | I-21, P-08, N-06, N-11 |
| 17 | Overview: Pulse at any time, with a time view | P, N | M | | 4 | P-04, P-09, I-34 |
| 18 | Empty and loading states that offer the next click | N, L | S | | 3 | L-09 |
| 19 | Layout presets, and `⧉` remembers a dragged layout | N, L | S | | 3 | I-10, N-12 |

### #1 · Needs-you strip: numbered, coloured by the worst need, with `▶ next` and `↩ last`

- **Efficiency gain:** "scan the tree for a coloured dot, find it, click it" becomes one click on the bottom line, and `▶ next` turns the Inbox into a loop: press, handle, press. The colour of one segment answers "does anything need me?" from the corner of the eye. `↩ last` bounces back to the Lead he was talking to.
- **Look:** one capped segment at the bottom left, filled with the worst urgency (`◖● 5 need you◗` red; `◖○ all quiet◗` grey), then numbered pills with a kind glyph.
- **Pattern:** the waiting items sit in the status line, each with a number; one control walks to the next; the previous place is one press away.
- **Sources:** agent-deck's Notification Bar, "Waiting sessions appear right in your tmux status bar" with `1`–`6` to jump ([README](https://github.com/asheshgoplani/agent-deck#notification-bar)); tmux-agent-tracker's agent bar with `+3` overflow and "next agent waiting on you" ([README](https://github.com/ulas96/tmux-agent-tracker)); WeeChat's `hotlist` and `Alt+a` ([quickstart](https://github.com/weechat/weechat/blob/HEAD/doc/en/weechat_quickstart.en.adoc)); savras walking "the rows you can see, in the order you see them" ([README](https://github.com/Marijusaj/savras)); sesh's "bounce between your two most recent sessions" ([README](https://github.com/joshmedeski/sesh)); Helix's statusline coloured by mode ([editor.md](https://github.com/helix-editor/helix/blob/HEAD/book/src/editor.md)). Details: attention.md A-01, A-02; navigation.md N-01, N-04; looks.md L-02.
- **cpd mapping (status line):** the StatusBar's `inbox <n>` counter, which a click ignores today (`docs/commands.md` §13.2 on `main`), becomes the strip. Its items are the first open, visible Inbox items in Inbox order (`cpd inbox list`, §21.1: red, yellow, magenta, white; oldest first; `engine-down` first), each `<urgency><n> <kind glyph> <who>`. A click on an item runs the same jump as a click on that item's tree row; `+N` opens the tree's Inbox expanded (Phase 1), later the triage popup (#13). `▶ next` runs the first item's jump. `↩ last` opens the Lead of the second-newest `session-moved` Event with `place: "chat"`, which the views already read (§17), so it needs no new state. The Inbox stays the one source of "needs you": the strip is a view of it, never a second list.
- **How the strip gets its data (D2):** the way the status-line PR feeds `@cpd_workers` (§0.2): the engine's tick writes the styled strip into a tmux option, at most one tick (5 s) late. Each item is a `range=user|…` click range (tmux(1); the cockpit already uses ranges for `⧉ ? ▤` on the statusline branch). A click must act on the **item id** captured in the range, never on a position, so a new red item arriving cannot redirect the click (attention.md A-10). Unverified: that an item id fits a range name (at most 15 bytes, tmux(1) 3.8).
- **Effort:** M. A new engine step and option (the `@cpd_workers` step is the template), a `cpd tui action` kind per range, and the format; the jumps exist.
- **Depends on:** the status-line PR; the Inbox (on `main`); D2.

```
 ⧉ ? ▤ ⌕ │ ◖● 5 need you◗ ▶ next ↩ last │ ●1 ✗ schema/links  ●2 ✎ api#34  ◐3 ✓ api#30  ◆4 ? login  +1 │      ▰▰▰▱▱▱▱▱ 9/24 jobs ✗1 │ ●●●○ 3/4 · 0 waiting · 4 queued
 ⧉ ? ▤ ⌕ │ ◖◆ 1 needs you◗ ▶ next ↩ last │ ◆1 ☰ search plan review │                                                ▰▰▰▱▱▱▱▱ 9/24 jobs │ ●●○○ 2/4 · 0 waiting · 4 queued
 ⧉ ? ▤ ⌕ │ ◖○ all quiet◗ ↩ last │                                                                                     ▰▰▰▰▰▰▱▱ 18/24 jobs │ ●○○○ 1/4 · 0 waiting · 2 queued
```

### #2 · One click convention for every need, and its verb on the row

- **Efficiency gain:** the five PR and review kinds stop needing a modifier-click on a link, and the commonest answer (retry a failed Job, open its Worker) happens from the row, without first opening the Lead and asking.
- **Look:** small capped chips on the selected row only, so the column stays calm.
- **Pattern:** the action buttons sit on the notification itself.
- **Sources:** cmux's Feed, where permission requests and questions carry their answer buttons inline ([feed.md](https://github.com/manaflow-ai/cmux/blob/HEAD/docs/feed.md); cmux is a macOS GUI, used for the pattern only); gh-dash's notification keys (`m` merge, `v` approve, `w` watch checks) with `?` showing only those that apply ([keys](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/getting-started/keybindings/selected-notification.mdx)); lazygit's click targets on every named thing ([integration tests](https://github.com/jesseduffield/lazygit/tree/HEAD/pkg/integration/tests)). Details: attention.md A-05; navigation.md N-06.
- **cpd mapping (Inbox section, Event log, status line):**
  - **S, Phase 1:** a plain click on a `plan-review`, `pr-failing`, `pr-changes`, `pr-ready` or `review-requested` row opens the owning Plan's Lead and focuses the chat, as a Deploy row does today; a `↗` at the row's end carries the OSC 8 link (`docs/commands.md` §13.2 "Links"). Close the other click gaps of navigation.md N-06: an Event log line opens its Plan's Lead; `workers N/M` opens the Workers board (#5).
  - **M, Phase 2:** verb chips on the selected row: `retry ▾` (a Tier menu, because `job retry` requires `--tier`, §5.4), `Worker`, `snooze ▾`, `✕` (dismiss, §21.4). The table of verbs per kind is attention.md A-05's.
- **Effort:** S for the convention and the gaps (each reuses the open-Lead action); M for the chips and the Tier menu.
- **Depends on:** the Inbox. A **Merge** verb would be the largest saving for `pr-ready`, but no command merges a pull request (§0.2): D6.

```
 ▌● ✗ schema/links failed              21m
 ▌  a2 · clippy: unused import
 ▌  [retry ▾] [Worker] [snooze ▾] [✕]
  ● ✎ auth api#34 changes requested  ↗  8m      a plain click opens auth's Lead; ↗ opens the PR
```

### #3 · Gauges instead of the five counters; a slot strip

- **Efficiency gain:** "how far along is it?" is one glance instead of adding `queued⋅running⋅merging⋅failed⋅merged` in the head, and a failure shows as `✗1` where he already looks.
- **Look:** thin `▰▱` bars that line up down the column; nothing drawn when there is nothing to count.
- **Pattern:** a gauge with a count in a fixed corner, red when anything failed, hidden at zero; a summary that shows only non-zero counts.
- **Sources:** yazi's status-bar progress, drawn in an error style when `summary.failed` is not 0 and not at all when `summary.total == 0` ([progress.lua](https://github.com/sxyazi/yazi/blob/HEAD/yazi-plugin/preset/components/progress.lua)); ilmari's status summary that "shows only nonzero counts" ([README](https://github.com/bnomei/ilmari#add-the-global-summary-to-the-status-line)); ratatui's `Gauge` and `LineGauge`, present in the locked `ratatui-widgets` 0.3.2 (progress.md §1, checked there). Details: progress.md P-03, P-11.
- **cpd mapping (tree, Plan view, status line):** a Plan Lead row shows `▰▰▰▱▱ 6/10 ✗1` (merged ÷ not cancelled); a collapsed Context the sum of its Plans; a PR box its own gauge; the status line `▰▰▰▱▱▱▱▱ 9/24 jobs ✗1` and one cell per slot, `●●●○ 3/4`. The counters move to the legend and the row's right-click detail (D7).
- **Effort:** S. The counters are already in the read model (§13.2 "Rows"); the widgets ship with ratatui.
- **Depends on:** D7 (counters out or in; drafts in the denominator or not).

```
 today                                         proposed
 ⏵  ├ ● schema         2⋅1⋅0⋅1⋅6               ⏵  ├ ● ⎇ schema      ▰▰▰▱▱ 6/10 ✗1
    ├ ● auth           4⋅1⋅0⋅0⋅1                  ├ ● ⎇ auth        ▰▱▱▱▱ 1/5
    ├ ○ search         3⋅0⋅0⋅0⋅0                  ├ ◆ ⎇ search      ▱▱▱▱▱ 0/3
```

### #4 · PR lifecycle stepper and state chips

- **Efficiency gain:** each PR says which step it is on and which step is red, so "what do I do next for this PR?" needs no reading of words; the long phase before a PR opens (Jobs running) becomes visible too.
- **Look:** the box's empty bottom border carries the stepper; state words that change what he does next become filled chips; the rest stay calm words.
- **Pattern:** an item's whole path as a row of stage glyphs; each question its own column, colour carrying the state.
- **Sources:** drydock, "Release state is a separate axis from working state", colour so "the rows worth acting on stand out without reading a word" ([README](https://github.com/yetidevworks/drydock)); gh-dash's one-icon `ci` and `reviewStatus` columns ([PR layout](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/layout/pr.mdx)); Catppuccin tmux's capped modules ([status_module.conf](https://github.com/catppuccin/tmux/blob/HEAD/utils/status_module.conf)). Details: progress.md P-02; looks.md L-05.
- **cpd mapping (Plan view, Deploy column):** the PR box's bottom border becomes `▰▰▰▱▱ 2/3 ▸ ○ open ▸ ○ ci ▸ ○ review ▸ ○ merge` (glyphs only below about 46 cells); the Deploy row's second line `open ✓ approved conflict` becomes `✓ci ✓review ○merge` with `◖conflict◗`, `◖changes requested◗`, `◖failed◗`, `◖draft◗` as chips. All five facts are already read (`prs`, `pull_requests.checks`, `review`, `mergeable`; §17).
- **Effort:** S (S–M with the chip helper across three views and their snapshot tests).
- **Depends on:** hs-d266's pill helper, so chips and pills share one grammar.

```
 ╭─ schema · api → main · dispatch ────────────────────────╮      ● api#34 auth                          ↗
 │ ◖✓ types │S1◗──◖✓ gql │S2◗──◖⠹ ui │S2│a2◗               │        ✓ci ◖changes requested◗ ○merge
 ╰─ ▰▰▰▱▱ 2/3 ▸ ○ open ▸ ○ ci ▸ ○ review ▸ ○ merge ───┬──╯      ◐ api#30 hotfix                        ↗
                                                     ▼            ✓ci ✓review ○merge · ready
```

### #5 · Workers board in the Deploy column

- **Efficiency gain:** "what is each Worker doing right now, and is any stuck?" is answered without walking the flow chart or opening a Worker pane; the one that failed sits on top with its retry.
- **Look:** two rows per Attempt, a spinner, an age bar, the last tool line dim.
- **Pattern:** a live list of everything in flight with state, age and the latest output line, in a stable order that survives refreshes; a bar of elapsed time against the usual, never a made-up percentage.
- **Sources:** ilmari lists every agent with its state and "recent output" ([README](https://github.com/bnomei/ilmari#what-the-radar-shows)); sqwatch's job table where "your selection and cursor position survive each refresh" ([README](https://github.com/fedonman/sqwatch#features)); yazi's task list ([tasks.lua](https://github.com/sxyazi/yazi/blob/HEAD/yazi-plugin/preset/components/tasks.lua)); process-compose's status table ([proc-table.go](https://github.com/F1bonacc1/process-compose/blob/HEAD/src/tui/proc-table.go)); gentooplz, "Real progress, not guesses" ([README](https://github.com/JustRoccat/gentooplz)). Details: progress.md P-01, P-10; survey.md I-14, I-30.
- **cpd mapping (Deploy column):** a `workers N/M` section above the PRs. Order: failed in the last few minutes first (with `↻ retry`), then running Attempts by start time, so rows never move under the pointer (survey.md I-27). A click opens the Worker pane beside the chat (the existing action). Second step: on a running row, a bar of elapsed ÷ the median of finished Attempts of the same Tier, labelled `(est.)`, amber with `!` past 1.5×, nothing before three finished Attempts of that Tier (progress.md P-10; the query over `attempts` was not run).
- **Effort:** M. A new section in the `widgets` view and a tail read of each running Worker file with the existing renderer (`tui/src/worker/render.rs`); M more for the estimate.

```
 ──◖ workers 3/4 ◗────────────────────────
 ✗ schema/links  H2 a2          failed 21m
   exit 1 · cargo clippy           ↻ retry
 ⠹ schema/ui     S2 a2           7m ▰▰▰▰▱▱
   → edit src/ui/Login.tsx
 ⠴ login/form    S2 a1        14m ▰▰▰▰▰▰ !
   slow: typical S2 is 9m (est.)
 ○ 4 queued · 0 waiting
```

### #6 · Quiet frame, loud signal; rounded inside

- **Efficiency gain:** the brightest things on screen become the needs and failures, not the frame, so the eye lands on what matters; the focused pane is found by its one accent header.
- **Look:** grey single lines between panes, a blue edge and header on the focused pane; rounded PR boxes, menus and popups; no mixed corner styles.
- **Pattern:** a "faint" token for inactive borders and hints, one accent for focus; rounded corners as a deliberate, consistent choice.
- **Sources:** lazygit's `activeBorderColor` against `inactiveBorderColor` and `border: rounded` ([Config.md](https://github.com/jesseduffield/lazygit/blob/HEAD/docs/Config.md)); gh-dash's required `border.faint` and `text.faint` ([theme.mdx](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/theme.mdx)); btop's `rounded_corners` and `inactive_fg` ([README](https://github.com/aristocratos/btop#configurability)); ratatui `BorderType::Rounded`; tmux `popup-border-lines rounded` (tmux(1) 3.8; tmux cannot round pane borders). Details: looks.md L-01, L-07.
- **cpd mapping (tmux options, Plan view, tree menu, popups):** `pane-border-style fg=colour8`, keep `pane-active-border-style fg=blue`, consider `pane-border-lines single`; PR boxes and the tree menu `BorderType::Rounded` (today `┌┐` boxes beside `╭╮` arrows, §13.2); `display-popup -b rounded` for the legend and the log (minimum tmux version unverified).
- **Effort:** S. Two server options (`core/src/host/tmux.rs`), a `BorderType` in two views, and their tests.
- **Depends on:** D4. It reverses the User's rule hs-d124 (white top-level separators): his call, not the Lead's.

### #7 · Shape as well as colour: a glyph per kind of need; palette audit

- **Efficiency gain:** he reads *what he must do* (a failure, a review, a question) before reading the label, and he can tell red from yellow even where his theme makes them close.
- **Look:** a second glyph after the urgency dot: `✗` failed, `✎` changes requested, `✓` ready to merge, `↗` review to give, `☰` Plan review, `?` question, `✉` reply, `⚠` engine.
- **Pattern:** each state has its own shape as well as its colour, shared by every surface; apps use roles over the terminal's 16 colours and check legibility.
- **Sources:** agent-deck's `●` `◐` `○` `✕` ([Status Detection](https://github.com/asheshgoplani/agent-deck#status-detection)); ilmari's one table of icon and colour per state ([configuration](https://github.com/bnomei/ilmari#configuration)); Catppuccin's "Legibility always comes first" ([style guide](https://github.com/catppuccin/catppuccin/blob/HEAD/docs/style-guide.md)); opencode's `system` theme on ANSI 0–15 ([themes.mdx](https://github.com/sst/opencode/blob/HEAD/packages/web/src/content/docs/themes.mdx)). Details: attention.md A-09; looks.md L-03, whose §2.2 computed from the User's two Ghostty themes that red and yellow are ΔE 18 apart in Vesper and that colour 8 is 2.6 : 1 on the CPD theme (calculations, not seen on a screen).
- **cpd mapping (palette, every surface that shows a need):** `palette::kind_glyph(Kind)` used by the Inbox rows, the strip, the popup and the Plan view marks; one legend table generated from the palette; `failed` and `conflict` bold. A throwaway audit script that prints contrast and ΔE for a Ghostty theme file; fix what it finds in the palette, not in a theme engine.
- **Effort:** S. A palette function, the legend lines, the row builders; the audit is about fifty lines and not runtime code.
- **Depends on:** hs-d266's Nerd Font icon choices (the glyphs above are stand-ins); D5.

### #8 · Fold finished work; a fold never hides a need

- **Efficiency gain:** the Plan view has about 12 rows; a merged PR's box gives its rows back to what is moving, and folding the tree never hides a Plan that needs him.
- **Look:** one-row folded boxes with the worst glyph inside kept visible.
- **Pattern:** an accordion: the focused thing gets the room, the rest collapse to one row that still shows the worst state inside.
- **Sources:** lazygit's `expandFocusedSidePanel`, "an accordion effect" ([Config.md](https://github.com/jesseduffield/lazygit/blob/HEAD/docs/Config.md)); bosun's collapsed row that "picks up a small accent dot when any background tab is busy" ([README](https://github.com/yetidevworks/bosun)); WeeChat's hotlist counts per buffer ([user guide](https://github.com/weechat/weechat/blob/HEAD/doc/en/weechat_user.en.adoc)). Details: progress.md P-05; survey.md I-29; attention.md A-13.
- **cpd mapping (Plan view, tree):** a PR box whose Jobs are all merged folds to `▸ ✓ core · api → main · ◖#28 merged◗ ↗ ▰▰▰▰▰ 3/3`; a click expands it; `◖ fold done ◗` folds all; view state only, like the archived-rows fold (§13.2). A **collapsed** Context row shows a count of its Plans' needs in the worst colour (`▸ ○ ⌂ ops  ● 2`); expanded, nothing extra.
- **Effort:** S. A layout rule in the box layout and a click target; the counts come from the items the tree already reads.
- **Depends on:** D8 for the Context count (it touches hs-d215 and hs-d217, which keep a Context row's dot to its Manager).

### #9 · Say what it wants: a cause line under every need

- **Efficiency gain:** he can rank and decide from the row; today two `dialog` items read the same template (`<who> asks you`) and each must be opened.
- **Look:** one dim line under the item, cut with `…`.
- **Pattern:** the row carries the actual question or failure, not "something is waiting".
- **Sources:** savras's rows `WAITING answer: …` under a `Needs input` heading ([README](https://github.com/Marijusaj/savras)); cmux's sidebar of "the latest notification text for each workspace", built because the agent's notification said only "Claude is waiting for your input" ([README](https://github.com/manaflow-ai/cmux#why-cmux)); Glances' alert rows with their cause ([events.rst](https://github.com/nicolargo/glances/blob/HEAD/docs/aoa/events.rst)). Details: attention.md A-03; navigation.md N-08 (a hover peek card, a later form of the same text); progress.md P-12.
- **cpd mapping (Inbox section, triage popup):** `job-stuck` the Job's `failed_cause` and the Worker's last line; `pr-changes` the reviewer; `pr-ready` checks and review; `plan-review` the Job count; `question` the record's text. `dialog` and `unread` need the Lead's last message, which no item stores today (`docs/contract.md` §2.2), so the engine step that opens the item would store an excerpt. `pr-failing` would name the red checks only once the poll keeps their names (today `pull_requests.checks` is a four-value summary, progress.md P-12).
- **Effort:** S for the kinds whose data is in rows; M for the `dialog`/`unread` excerpt (a field, a migration, the opening step) and for check names.
- **Depends on:** the Inbox owner (Plan inbox) for the new field.

```
 ──◖ Inbox 5 · 2 new ◗──────────────────
 ▌● ✗ schema/links failed              21m
 ▌  a2 · clippy: unused import
  ◆ ? login asks you                    3m
    "which test account for e2e?"
  ◆ ☰ search plan review r1            14m
    4 Jobs to approve
```

### #10 · Right-click menus on every object, and a Job card

- **Efficiency gain:** every command becomes reachable by mouse from the thing it acts on, and a command that cannot run now shows dim with the reason instead of failing after the click.
- **Look:** one menu widget, the same everywhere; a card that follows the selected Job in the Plan view's empty right side.
- **Pattern:** right-click an object, get what you can do to it, unavailable items dim; a preview that follows the selection.
- **Sources:** tmux `display-menu`, where "if the name begins with a hyphen (-), then the item is disabled (shown dim)" (tmux(1)); zellij's `context_menu` per pane, tab and bar ([default.kdl](https://github.com/zellij-org/zellij/blob/HEAD/zellij-utils/assets/config/default.kdl)); flowrs' per-view command popup ([commands.rs](https://github.com/jvanbuel/flowrs/blob/HEAD/src/app/model/dagruns/commands.rs)); harlequin's palette entries with `is_available` ([palette.py](https://github.com/tconbeer/harlequin/blob/HEAD/src/harlequin/palette.py)); gh-dash's preview pane ([defaults](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/defaults.mdx)). Details: navigation.md N-03; progress.md P-06; survey.md I-05, I-33.
- **cpd mapping (Plan view, Deploy column, Event log, pane headers):** extract the tree's menu into a shared widget; items per object are navigation.md N-03's table (Job pill: Open Worker, Copy key, Edit…, Cancel, Retry dim unless failed; PR box: Open PR ↗, Rebase, Open Lead; Deploy row; Event line). The card: status, Tier, one line per Attempt, the Worker's last `→`/`←`, and `[Worker] [Retry ▾] [Copy]` (vision mockup, Plan view right).
- **Effort:** M. One widget to extract and an item table per object; a card region and a tail read; no engine change.

```
 ◖✓ types │S1◗──◖⠹ gql │S2◗──◖○ tests │S2◗
                ╭──────────────────────╮
                │ Open Worker          │
                │ Copy key             │
                │ Edit…                │
                │ Cancel Job           │
                │ Retry (running)      │   dim: the Job is running
                │ Open PR ↗            │
                ╰──────────────────────╯
```

### #11 · A go-to picker that needs no typing

- **Efficiency gain:** any Lead, Plan, PR or Job is one click from anywhere, and a long tree stops costing scrolling. With nothing typed it already shows what needs him, then what he used recently.
- **Look:** a popup with a search line, filter chips that are buttons, grouped results, and a footer saying where the highlighted row leads.
- **Pattern:** one popup reaches anything; an empty query is useful; filters are buttons.
- **Sources:** herdr's Goto picker, "Click a row or press Enter to jump directly to it … `b/w/i/d` filter blocked, working, idle, or done agents" ([keyboard](https://github.com/herdrdev/herdr/blob/HEAD/docs/next/website/src/content/docs/keyboard.mdx)); Textual's command palette opened from a header icon ([guide](https://github.com/Textualize/textual/blob/HEAD/docs/guide/command_palette.md)); k9s's `:` command mode ([README](https://github.com/derailed/k9s#key-bindings)); tmux `choose-tree` with clicks and preview ([mode-tree.c](https://github.com/tmux/tmux/blob/master/mode-tree.c)). Details: navigation.md N-02 (with its mockup M3); survey.md I-06, I-17, I-19.
- **cpd mapping (popup):** `⌕` in the status line opens a `display-popup` view (like legend and logs) over the same read model as the tree. Spike first: `choose-tree` over the parked Lead windows, to learn whether he reaches for a picker at all (navigation.md N-02; untested).
- **Effort:** S for the spike; M for the view.

```
 ╭ go to ─────────────────────────────────────────────────────── ✕ ╮
 │ ⌕ type a Context, Plan, PR or Job…                              │
 │ [● needs you 5] [⠹ working 3] [○ idle 4] [PRs 3]                │
 ├─ needs you ─────────────────────────────────────────────────────┤
 │ ▌● schema      api   schema/links failed 21m                    │
 │  ● auth        api   api#34 changes requested                   │
 │  ◆ login       web   asks: which test account for e2e?          │
 ├─ recent ────────────────────────────────────────────────────────┤
 │  ⠹ auth        api   Lead working · ▰▱▱▱▱ 1/5                    │
 ├─────────────────────────────────────────────────────────────────┤
 │ schema · Context api · phase dispatch · 6 of 10 Jobs merged     │
 ╰─────────────────────────────────────────────────────────────────╯
```

### #12 · Pane headers as toolbars: zoom, menu, breadcrumb, badge

- **Efficiency gain:** zoom any pane with one click (read a wide flow chart, or the chat alone) and come back with one click; a zoomed chat still says which Lead it is; a need of the Lead he is talking to shows in the header he is looking at.
- **Look:** each header is a small status line: the title on the left, a capped badge and `⤢ ⋯ ✕` on the right.
- **Pattern:** the title bar carries its own controls and says where you are.
- **Sources:** tmux ≥ 3.7's `#[range=control|N]` in `pane-border-format`, whose default format ships `[z]` zoom and `[x]` kill; navigation.md verified it on tmux 3.8 ([options-table.c](https://github.com/tmux/tmux/blob/master/options-table.c), [key-bindings.c](https://github.com/tmux/tmux/blob/master/key-bindings.c)); k9s breadcrumbs ([README](https://github.com/derailed/k9s#k9s-configuration)); gh-dash's section counts ([theme.mdx](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/theme.mdx)); tmux-agent-tracker's numbered pane-border badges ([README](https://github.com/ulas96/tmux-agent-tracker)). Details: navigation.md N-05; looks.md L-06; attention.md A-08; survey.md I-03, I-12.
- **cpd mapping (tmux `pane-border-format`):** `⤢` = `resize-pane -Z`, `⋯` = a `display-menu` (zoom, reset layout, close), `✕` on the chat = close with its y/N. The chat header becomes `api ▸ schema ▸ Lead · dispatch`, each part a click (Context row click, Plan's Lead). The badge text comes from a pane option written next to `@cpd_title`.
- **Effort:** S on tmux ≥ 3.7 (a format and three bindings); M on 3.4 (`MouseDown1Border` plus `#{mouse_x}` arithmetic, inferred fragile).
- **Depends on:** D3 (the cockpit runs 3.4 today, §0.2); hs-d257 (dynamic titles touch the same format); hs-d266 (caps).

```
─◖ Context ◗──────────────◖ ●2 ◐1 ◆2 ◗─ ⋯ ─┬─◖ Plan ◗──────────────────────◖ schema · 6/10 merged · dispatch ◗─ ⤢ ⋯ ─┬─◖ Deploy ◗─────◖ 1 ready ◗─ ⤢ ⋯ ─
                                           ┣━◖ api ▸ schema ▸ Lead · dispatch ◗━━━━━━━━━━━━◖ ● ✗ schema/links failed 21m ◗━ ⤢ ⋯ ✕ ━┫
```

### #13 · Triage popup with the cause, verbs and a resolved tail

- **Efficiency gain:** when several things wait, he handles the whole pile in one place: list on the left, the cause on the right (the failing Event, the Worker's last lines, the Lead's last message, what is blocked behind it), the verbs under it; and he sees the pile shrink.
- **Look:** a large popup grouped by urgency; a dim "resolved in the last hour" tail.
- **Pattern:** a notification centre with a preview of the cause, and recently ended alerts kept visible with their duration.
- **Sources:** gh-dash's notifications view with preview and per-kind keys ([sections](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/notification-section.mdx)); ilmari's tmux popup radar ([README](https://github.com/bnomei/ilmari#popup-controls)); tmux-agent-status's needs-first picker ([README](https://github.com/gudoshnikovn/tmux-agent-status)); Glances' ongoing `●` and resolved `○` alerts ([events.rst](https://github.com/nicolargo/glances/blob/HEAD/docs/aoa/events.rst)). Details and full mockup: attention.md A-04, A-12.
- **cpd mapping (popup):** opened by the strip's `+N`, a click on the Inbox header, or a key; reads `inbox list` (with `--closed` for the tail) and `inbox show` (§21.1, §21.2), reuses the Worker renderer and the log view; every button is an existing command.
- **Effort:** M. One new view; no engine change.
- **Depends on:** the Inbox.

### #14 · Motion means change: flash, new versus seen, a slow pulse for waiting

- **Efficiency gain:** after ten minutes in the chat, the one pill or row that moved is findable among twenty, and a need he has not looked at stays loud while one he has seen goes calm. Working and waiting-on-someone-else become distinguishable by rhythm alone.
- **Look:** a changed cell reversed for 1.5 s, bold 3 s more, then normal; new needs bold with `▌`; a fast spin for working, a slow two-glyph pulse for waiting on a review or CI; a `since you looked 14:52 · 6 changes ▸` strip.
- **Pattern:** a highlight that expires; attention that latches until you have been there; spinners whose rhythm means something.
- **Sources:** htop's `--highlight-changes[=DELAY]`, 5 s by default ([htop.1.in](https://github.com/htop-dev/htop/blob/HEAD/htop.1.in)); ilmari's sticky attention cleared by focusing the pane ([README](https://github.com/bnomei/ilmari#published-fragment-and-counter-reference)); tmux-agent-tracker's agent that "stays orange until you have actually been to its pane" ([README](https://github.com/ulas96/tmux-agent-tracker)); savras's Ping ([README](https://github.com/Marijusaj/savras)); cli-spinners' `{interval, frames}` ([repo](https://github.com/sindresorhus/cli-spinners)); hwatch's history of differences ([README](https://github.com/blacknon/hwatch#readme)). Details: looks.md L-04, L-12; attention.md A-07; progress.md P-07.
- **cpd mapping (tree, Plan view, Deploy, Inbox):** each view keeps its last snapshot and a `changed_at` per row; "new" is derived from `session-moved` Events newer than `opened_at`, so views still write nothing (attention.md A-07). Where "since you looked" is stored is open: a view's memory is lost on respawn and §17 forbids state in tmux options (progress.md P-07).
- **Effort:** M. Snapshot diffing in three views and one `flash(age)` palette helper; S for the slow pulse alone.

### #15 · Hover and selection tints

- **Efficiency gain:** in a mouse-first cockpit, the pointer shows what a click will act on before he clicks; the keyboard cursor stops shouting.
- **Look:** a faint tint under the pointer, a slightly stronger tint with a `▌` bar for the selected row, instead of full-row reverse video.
- **Pattern:** the selected line is a background tint; the item under the pointer lights up.
- **Sources:** lazygit's `selectedLineBgColor` ([Config.md](https://github.com/jesseduffield/lazygit/blob/HEAD/docs/Config.md)); zellij's `mouse_hover_effects` ([default.kdl](https://github.com/zellij-org/zellij/blob/HEAD/zellij-utils/assets/config/default.kdl)); crossterm's `EnableMouseCapture` already asks for any-motion (`?1003h`; [commands.rs](https://github.com/crossterm-rs/crossterm/blob/HEAD/src/event/commands.rs)). Details: looks.md L-08; navigation.md N-09.
- **cpd mapping (tree, Plan view, Deploy column):** a hover field per view, redrawn on `Moved` events and coalesced so ssh does not carry a redraw per cell; one `tint(level)` in the palette. Not possible on the tmux status line or borders (tmux turns mouse-move keys on only for applications that ask; navigation.md §3.3). Unverified: that tmux forwards motion to the pane in this cockpit.
- **Effort:** S for the selection tint; M for hover.

### #16 · The Event log as a feed: clickable, newest first, next problem, floating

- **Efficiency gain:** he can follow the stream while he types in the chat, and jump straight to the next failure or need instead of scrolling.
- **Look:** newest on top, bursts grouped (`3 Jobs merged in auth`), an activity strip over it; on tmux ≥ 3.8 a floating pane he can move and leave open.
- **Pattern:** one time-ordered feed with an index of what matters and one key to the next; a non-modal floating panel.
- **Sources:** lnav, which "builds an index of errors and warnings", jumps with `e`/`E` and has a histogram ([README](https://github.com/tstack/lnav#readme), [hist](https://docs.lnav.org/en/latest/ui.html#hist)); tmux 3.7 floating panes, "not modal and behave like panes", and 3.8's mouse move and resize ([CHANGES](https://github.com/tmux/tmux/blob/master/CHANGES)); fzf's `--popup` as a floating pane ([man page](https://github.com/junegunn/fzf/blob/HEAD/man/man1/fzf.1)). Details: survey.md I-21; progress.md P-08; navigation.md N-06, N-11.
- **cpd mapping (`logs` view):** S: newest first, a click on a line opens its Plan's Lead, `n`/`N` to the next `error` or needs-you Event, `/` to filter by Plan. M: grouping, the activity strip from Pulse's buckets, and the floating pane.
- **Effort:** S / M.
- **Depends on:** D3 for the floating pane (3.8).

### #17 · Overview: Pulse at any time, with a time view

- **Efficiency gain:** every Plan at once without giving the chat slot to the Manager (today Pulse shows only while a Manager is in the chat slot, §13.2 "Pulse"), and a time view that shows where a Plan's time went: which Attempt ran long, which failed twice, what waited on what.
- **Look:** the Plan pane zoomed into a dense table (gauge, PR steps, activity sparkline, age, last line), and below it one lane per Job on a shared time axis.
- **Pattern:** select a widget and expand it to the whole screen; a Gantt column of tries, coloured by state.
- **Sources:** bottom's `e` expand and graph time zoom ([general usage](https://github.com/ClementTsang/bottom/blob/HEAD/docs/content/usage/general-usage.md)); drydock's one screen across every repo ([README](https://github.com/yetidevworks/drydock)); flowrs' Gantt of task tries ([gantt.rs](https://github.com/jvanbuel/flowrs/blob/HEAD/src/ui/gantt.rs)). Details and full mockup: progress.md P-04, P-09 (Mockup B).
- **cpd mapping (Plan view):** a `Plan | Overview` toggle in its header and the header's `⤢` (#12); `tui/src/plan/pulse.rs` gains columns, a window `‹ 1h [3h] 12h ›` and a `flow | time` toggle. The time view reads `attempts.started_at`/`ended_at`; a Job's `queued` time is derivable from `history` (progress.md P-09; not run).
- **Effort:** M.

### #18 · Empty and loading states that offer the next click

- **Efficiency gain:** an empty pane says what to do and offers the button, instead of a dim line at the bottom.
- **Look:** a small centred block: a dim headline, one line of what to do, a chip.
- **Pattern:** every list has three drawn states: loading, empty, data.
- **Sources:** gh-dash's table with `EmptyState`, `loadingMessage` and a spinner ([table.go](https://github.com/dlvhdr/gh-dash/blob/HEAD/internal/tui/components/table/table.go)). Details: looks.md L-09.
- **cpd mapping (every view):** `+ New Plan` runs the tree's `n` action; `+ New Context` the empty-area menu item (§13.2); messages that need a path stay text.
- **Effort:** S.

```
                     no Lead in the centre
           pick a Plan in the Context column, or
                       ◖ + New Plan ◗
```

### #19 · Layout presets, and `⧉` remembers a dragged layout

- **Efficiency gain:** the shape follows the task (chat focus, review focus) in one click, and a border he dragged is not lost at the next reset.
- **Look:** `⧉` opens a small menu.
- **Pattern:** named layout presets; the divider position is kept.
- **Sources:** btop's `presets` ([README](https://github.com/aristocratos/btop#configurability)); bosun's divider "persisted to `config.toml` as `divider_x`" ([README](https://github.com/yetidevworks/bosun)); tmux's default `MouseDrag1Border` = `resize-pane -M` ([key-bindings.c](https://github.com/tmux/tmux/blob/master/key-bindings.c)). Details: survey.md I-10, I-15; navigation.md N-12.
- **cpd mapping (status line `⧉`, reset-layout):** menu `Default`, `Chat focus` (Plan view to a strip), `Review` (Deploy column wide), `Use this layout` (stores the column widths in a tmux server option that reset-layout reads; layout is view state, not Ledger state, §17).
- **Effort:** S. Reset-layout already applies sizes.

## 3. Phases

Four batches, in order. Each is one visible improvement the User can review as a whole on his cockpit, and each leaves the cockpit consistent if the next never comes.

| Phase | Name | Ideas | Pain | Size | Needs first |
|---|---|---|---|---|---|
| 1 | **One glance says what needs me and how far things are** | #1, #2 (S), #3, #4, #6, #7 | F, P, L | about 5 S + 1 M | the status-line PR merged; D2, D4, D5, D7 |
| 2 | **Watch work without opening anything** | #5, #8, #9, #14, #2 (M), #16 (S) | P, F | about 3 S + 4 M | D8; Inbox owner agrees to the excerpt field (#9) |
| 3 | **Everything one click away** | #10, #11, #12, #13, #15, #18, #19, #16 (M) | N, F | about 5 S + 5 M | D3 (tmux 3.8) |
| 4 | **The big picture** | #17, #5's estimate; optional: the shaded look (D4 c), arrival toast and bell (D9) | P, L | 2–3 M | Phases 1–3 used for a while |

### Phase 1 · One glance says what needs me and how far things are

What changes on the screen, against today:

- **Status line:** the dead `inbox <n>` becomes the strip (#1), with the worst-colour segment, `▶ next`, `↩ last`; the right side gains the Job gauge and slot cells (#3).
- **Context column:** gauges replace the counters (#3); the Inbox rows get a kind glyph (#7) and a plain click for every kind (#2).
- **Plan view and Deploy column:** the stepper on each PR box and PR row, chips for `conflict`, `changes requested`, `failed`, `draft` (#4); rounded boxes (#6).
- **Frame:** grey lines, blue focus (#6), if D4 says so.

```
 today (main, with the status-line PR's right side)
 ⧉ ? ▤                                                                                                       inbox 5    workers 3/4 · 0 waiting · 4 queued
 after Phase 1
 ⧉ ? ▤ │ ◖● 5 need you◗ ▶ next ↩ last │ ●1 ✗ schema/links  ●2 ✎ api#34  ◐3 ✓ api#30  ◆4 ? login  +1 │  ▰▰▰▱▱▱▱▱ 9/24 jobs ✗1 │ ●●●○ 3/4 · 0 waiting · 4 queued

 today                                          after Phase 1
 ┌ schema · api → main · dispatch ─────┐        ╭─ schema · api → main · dispatch ─────────────────╮
 │  ✓ types │S1 ── ✓ gql │S2 ── ⠹ ui   │        │ ◖✓ types │S1◗──◖✓ gql │S2◗──◖⠹ ui │S2│a2◗        │
 └─────────────────────────────────────┘        ╰─ ▰▰▰▱▱ 2/3 ▸ ○ open ▸ ○ ci ▸ ○ review ▸ ○ merge ─╯

  api/api#34                          ↗          ● api#34 auth                          ↗
   open ✓ changes requested  auth                  ✓ci ◖changes requested◗ ○merge
```

### Phase 2 · Watch work without opening anything

- **Deploy column:** the Workers board above the PRs (#5); the host line shrinks to two rows.
- **Plan view:** finished PRs fold (#8); a changed pill flashes then calms (#14); the `since you looked` strip.
- **Context column:** the Inbox rows get their cause line and verb chips (#9, #2 M); new needs are bold with `▌` (#14); a collapsed Context shows its needs (#8).
- **Event log:** newest first, clickable lines, `n`/`N` to the next problem (#16 S).

```
 before (12-row Plan view, 3 boxes, scrolls)       after Phase 2 (core folded, nothing scrolls)
 ┌ core · api → main · #28 merged ┐                ▸ ✓ core · api → main · ◖#28 merged◗ ↗   ▰▰▰▰▰ 3/3
 │ ✓ a │S1 ── ✓ b │S1 ── ✓ c │S2   │                ╭─ schema · api → main · dispatch ─────────────╮
 └────────────────────────────────┘                │ ◖✓ types │S1◗──◖✓ gql │S2◗──◖⠹ ui │S2│a2◗    │
 ┌ schema · api → main ───────────┐                ╰─ ▰▰▰▱▱ 2/3 ▸ ○ open ▸ ○ ci ▸ ○ review ─────╯
 │ ✓ types ── ✓ gql ── ⠹ ui       │
 └────────────────────────────────┘                ── since you looked 14:52 · 6 changes ▸ ──
```

### Phase 3 · Everything one click away

- **Every object:** a right-click menu from one shared widget; a Job card in the Plan view (#10); hover and selection tints (#15); empty states with a button (#18).
- **Status line:** `⌕` opens the go-to picker (#11); `+N` opens the triage popup (#13); `⧉` opens the layout menu (#19).
- **Pane headers:** `⤢ ⋯ ✕`, the chat breadcrumb and the badges (#12).
- **Event log:** a floating, non-modal pane on tmux 3.8 (#16 M).

The vision mockup (§1) is the cockpit at the end of Phase 3, plus Phase 4's `since you looked` strip and slow-Job estimate.

### Phase 4 · The big picture

- **Plan view:** the Overview toggle and its time view (#17), zoomed by the header's `⤢`; the Workers board's `(est.)` bars (#5).
- **Optional, by decision:** the shaded-panel look (D4 option c; looks.md L-11 and its §6 mockup); the arrival toast and the bell or desktop notification when the terminal is unfocused (D9).

```
 Overview · zoomed (click ⤢ to restore)      [ by Context | by status ]      window ‹ 1h [3h] 12h ›
    PLAN          JOBS             PRs                                       ACTIVITY, last 3 h               AGE   LAST
  ▾ ○ api
    ⏵ ● schema    ▰▰▰▱▱ 6/10 ✗1   ⠹ schema · ⠹ docs · ○ web · ✓ core#28      ▁▂▁▃▄▃▁▁▅█▄▃▁▂▄▃▂▁▁▃▅▄▂▁▃▃▂▁▂▃   0m    ✗ links failed a2
      ● auth      ▰▱▱▱▱ 1/5       ◖changes requested◗ #34                     ▁ ▁▁▃▃▂ ▁▁▂▄▅▃▂▁▁▁▂▃▂▂▃▄▃▂▁▁▁▂▃   0m    ⠼ session: cargo test
 ── schema · time view ──────────────────────────────────────────────────────────────────────────────────────────────
 links H2 a1     ··▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃✗
 links H2 a2                                 ····▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃▃✗                                      ┊ now
```

## 4. Decisions for the User

Each decision is his; the recommendation is one line with its reason. The ones Phase 1 needs are D2, D4, D5 and D7.

### D1 · Keep tmux panes, or one app?

| Option | What it means | For | Against |
|---|---|---|---|
| **a. Keep tmux panes and ratatui views (today)** | tmux draws the layout; each view is a ratatui program; pi runs in the chat slot | the chat's verified keys (`extended-keys`), OSC 8 links, images, copy and paste stay tmux's; all 19 ideas fit | no hover on the status line or borders; no cross-view effects |
| b. One ratatui app for the side columns, chat still a tmux pane | bosun's shape | one hit-test layer and hover across the side views | the chat slot sits in the middle with chrome on three sides, and tmux panes are rectangles: a layout change |
| c. One app that embeds the chat as a PTY | herdr, bosun 2.0 | full control of every pixel | a terminal emulator becomes cpd's job: herdr ships its own `ghostty-vt` crate; `vt100` drops OSC 8 links; keys and selection must be redone ([navigation.md §3.2](navigation.md#32-what-owning-the-terminal-costs-all-measured-on-the-chat-slot)) |
| d. Move to zellij | a multiplexer with hover-aware bars and floating panes | hover, a web client | a one-way door: every tmux command of the contract §6.3 and the pane options `@cpd_view`, `@cpd_session` |

**Recommendation: a.** Every idea in §2 fits; tmux 3.7 and 3.8 give the header controls and floating panes that were the best argument for b–d.

### D2 · Where the "needs you" list lives

| Option | What it means |
|---|---|
| a. The tree's Inbox section only (`main` today) | three rows at the bottom of a 43-column pane; the status line shows a count he cannot click |
| **b. The tree's section plus the status-line strip** | the strip names the first four and is always in view; the section holds the rest with causes and verbs; both read the same Inbox |
| c. The strip plus the triage popup; the tree's section removed | gives the tree its rows back; the causes and verbs move to the popup |
| d. A pane of its own | the most room; the least room for everything else |

**Recommendation: b**, with the strip fed the way the status-line PR feeds `@cpd_workers`: the engine's tick writes it into a tmux option, at most one tick late. That PR has already made the deviation from §17 for one option; this would be the second, and §17 should name both. The alternatives for the data path are worse: a `#(cpd …)` subprocess on each status redraw, or keeping a ratatui StatusBar row (which would undo hs-d149).

### D3 · Which tmux the cockpit runs

| Option | What it unlocks | Cost |
|---|---|---|
| a. Stay on the system's 3.4 (live today, §0.2) | nothing new; #12 costs M with `#{mouse_x}` arithmetic; no floating log | none |
| **b. Run the cockpit on tmux 3.8** (installed at `~/.local/opt/tmux-3.8`, §0.2) | clickable header controls (`range=control`, 3.7), floating and modal panes with click-outside close (3.8), scrollbars (3.6), `display-menu -M` (3.5) | the contract's baseline (§6.1, verified on 3.4) moves; a `doctor` check of the version; the Mac's tmux is not involved (the User attaches over ssh) |
| c. Require 3.7 | header controls only | same as b, less gained |

**Recommendation: b.** It is reversible (a binary path), the binary is already on the machine, and it turns #12 from M to S and makes #16's floating log possible. Unverified: that the cockpit's own tests pass under 3.8, and how cpd chooses the binary today.

### D4 · The frame

| Option | Look | Note |
|---|---|---|
| a. Keep white heavy separators (hs-d124) | the frame is the brightest thing on screen | today's rule |
| **b. Quiet grey frame, blue focus (#6)** | needs and failures become the brightest things | reverses hs-d124 |
| c. Shaded panels, no lines (looks.md L-11) | the most "designed": panels told apart by background tint | needs RGB through tmux (unverified on the Mac's path), changes how headers look, and pi's own background over a tinted pane is unverified |

**Recommendation: b now**, and look at c in Phase 4 with a mockup on his own screen. b is two tmux values and undone as easily.

### D5 · Colours and theme

| Option | What it means |
|---|---|
| **a. Follow the terminal's ANSI theme (today) and fix the palette for both his themes** | his Ghostty `theme =` line restyles the cockpit; #7's glyphs and the audit make red and yellow distinguishable in Vesper and grey legible in the CPD theme |
| b. cpd ships its own palette and a theme chooser | a second theme system next to the terminal's (survey.md I-32) |
| c. A truecolour palette | smooth fades and gradients; leaves his theme; needs RGB through tmux |

**Recommendation: a.** The terminal owns the theme; cpd only owes legibility on the themes he uses. Which Ghostty theme he runs is his taste; the audit says Vesper needs shapes for red/yellow and the CPD theme needs a lighter grey (looks.md §2.2, calculations).

### D6 · A Merge button on a ready PR

| Option | What it means |
|---|---|
| **a. No; the `↗` opens the PR page, where he merges** | no engine change |
| b. Add a `pr merge` command and a `Merge` verb on `pr-ready` rows and PR menus | the largest single saving for the `pr-ready` kind (attention.md A-05); a new command, its policy (who may merge, which method) and its tests |

**Recommendation: a for Phases 1–3**; revisit once the strip shows how many `pr-ready` items he handles a day.

### D7 · Counters or gauges on tree rows

| Option | What it means |
|---|---|
| **a. Gauges replace the five counters; the counters go to the legend and the right-click detail** | one glance for "how far"; `✗N` for failures |
| b. Both, side by side | 14 more cells in a 43-column row; names get cut |
| c. Keep the counters | no change |

**Recommendation: a**, and count drafts in the denominator (they are planned work; progress.md P-03 leaves this open).

### D8 · May a collapsed Context show its Plans' needs?

hs-d215 and hs-d217 keep a Context row's dot to its Manager's own items. Options: **a. yes, a count in the worst colour only while collapsed (the dot unchanged)**; b. no, the Inbox section and the strip are enough. **Recommendation: a**: folding the tree should never hide a need from the tree, and the dot rule stays as decided.

### D9 · Telling him when he is not looking

| Option | What it means |
|---|---|
| a. Nothing outside the cockpit (today) | he finds needs when he looks |
| **b. The terminal bell when a red or magenta item opens and no client has focus** | a tmux option; Ghostty decides how to show a bell |
| c. A desktop notification by OSC 9/777 through tmux passthrough | richer text; unverified that it reaches the Mac through ssh and tmux |
| d. A push to his phone (ntfy, Telegram) from the engine | works away from the desk; a new engine step and a channel to configure (survey.md I-25) |

**Recommendation: b, in Phase 4**, then c if the bell is too vague. Sources: crush's notifications "only sent when the terminal window isn't focused" ([README](https://github.com/charmbracelet/crush#desktop-notifications)); agent-deck's conductor and `notify-daemon` ([README](https://github.com/asheshgoplani/agent-deck#conductor)).

## 5. What not to do, and why

| Do not | Why | Source |
|---|---|---|
| Rewrite onto one app with an embedded terminal, or move to zellij | D1: the chat slot's verified keys, links and copy would become cpd's to rebuild; zellij is a one-way door | navigation.md §3 |
| Compute "needs you" anywhere but the Inbox | the strip, badges, chips, popup and tree are views of one list (hs-d165); a second rule would disagree with it | `docs/commands.md` §13.2 "Inbox section" |
| A theme engine or theme chooser inside cpd | the terminal already owns the theme, and the cockpit follows it by design (D5) | looks.md §8; survey.md I-32 |
| A truecolour-only palette, smooth fades, 30 fps transitions | takes the cockpit off his theme, needs RGB on an unverified path, and redraws over ssh; steps in ANSI colours give most of the value | looks.md L-04, L-12, §8 |
| Tabs in the Plan view | the User removed them (hs-d102) | navigation.md §6 |
| Focus follows mouse; drag to reorder Plans | brushing the pointer across the tree would steal the keyboard from the chat; the tree's `created_at` order is deliberate, and rows must not move under the pointer | navigation.md §6; survey.md I-27 |
| Hover tooltips on the status line or pane borders | tmux turns mouse-move on only for applications that ask; show the verb in the text instead | navigation.md §3.3; survey.md I-08 |
| User-editable menus, hotkeys or layout files | not asked for; menus are `cpd` commands, layouts are code (YAGNI, convention over configuration) | survey.md I-07, I-13; navigation.md §6 |
| Colour each Plan id by a hash | competes with the status colours, which are the signal | survey.md I-22 |
| Replay or time-scrubbing of a Plan | L (as-of reads behind every view); `since you looked` and the time view give most of it | progress.md P-13 |
| A Kanban board or a Jobs table mode | the flow chart already is the board; the Workers board answers "what is each doing" | progress.md §6; survey.md I-35 |
| Pinned Leads as tabs | needs storage and a command; wait until the strip and `↩ last` show they are not enough | navigation.md N-07 |
| An agent that moves or highlights things in his cockpit | not asked for; his cockpit should move only when he does | survey.md I-18 |
| Alert rules (stuck, looping) and hold times against flapping, built now | they belong to the Inbox owner, and no data yet shows flapping | survey.md I-37; attention.md A-14 |

## 6. What is not verified

- Nothing here was seen on a screen: every look is from READMEs, docs, config and source, as the theme files say; no mockup was tried in a terminal.
- The vision mockup was drawn with a script that checks every line is 215 cells and has no double-width glyph; Nerd Font glyphs are drawn as plain stand-ins.
- tmux features beyond 3.4 were read from the 3.8 man page and source by the theme files; the cockpit's tests were not run on 3.8 (D3).
- That an Inbox item id fits a tmux range name (#1); that tmux forwards mouse motion to the views (#15); that `display-popup -b rounded` exists on the tmux the cockpit runs (#6).
- That red and yellow are hard to tell apart in Vesper: inferred from a computed ΔE of 18 (looks.md §2.2).
- The `history` and `attempts` queries behind #5's estimate and #17's time view were not run.
- Effort sizes are estimates, not measurements.
