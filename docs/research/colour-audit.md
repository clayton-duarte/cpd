# Colour audit of the cockpit

Plan `ui-design`, Job `audit` (decisions hs-d501, hs-d504). Base: `main` at `71c77b06`. Research only: nothing here changed code. Every line number is a line of `main` at that commit; "unverified" marks what I did not run or read.

## How to read this

**Numbers.** The User's terminal colours: 0 black, 1 red, 2 green, 3 yellow, 4 blue, 5 magenta, 6 cyan, 7 grey, 8 bright black (dark grey), 9 bright red, 10 bright green, 11 bright yellow, 12 bright blue, 13 bright magenta, 14 bright cyan, 15 bright grey (white). ratatui: `Black` 0, `Red` 1, `Green` 2, `Yellow` 3, `Blue` 4, `Magenta` 5, `Cyan` 6, `Gray` 7, `DarkGray` 8, `LightRed` 9 … `LightCyan` 14, `White` 15. tmux: `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white` are 1 to 6 and **7** (tmux's `white` is 7, not 15), `colourN` is N. **default** = the terminal's own foreground (no colour set). Nothing in `tui/` uses `Color::Gray` (7) or any `Light*` colour (9-14): the cockpit today uses 0, 1, 2, 3, 4, 5, 6, 8 and 15 only (`tui/src/palette.rs:77-128`).

**Modifiers are not colours.** `DIM`, `REVERSED` and `BOLD` are written in parentheses after the number: `default (DIM)`. `REVERSED` swaps foreground and background, so a `REVERSED` 8 is a block of colour 8 with the text in the terminal's background colour (a "pill").

**One name per muted grey.** `Meaning::Nothing` is `Color::DarkGray`, colour **8** (`tui/src/palette.rs:86`). It is the "muted" of every view: `palette::fg(Meaning::Nothing)` in `tui/src/fold.rs:49`, `tui/src/section.rs:49`, `tui/src/plan/flow.rs:403`, `tui/src/plan/boxes.rs:202`. `Meaning::Done` is also 8 (`palette.rs:82`).

**Table columns.** `element | interactive | colour today | source | concept colour | differs`.

- *interactive*: yes/no and how (click, key, right-click, terminal modifier-click on an OSC 8 link). "Row" means the element sits in a row whose click or key does something; whether the element itself is interactive is ambiguity A9.
- *concept colour*: hs-d501 applied as the Job says: interactive and today base 1-6 gives its bright pair (n+8); non-interactive and today bright 9-14 gives n-8 (none exists today); non-interactive white 15 gives grey (`7 or 8, see A1`); non-interactive 8 stays grey 8 (`8 grey, A1` marks that A1 may move it to 7); focus blue stays 4; default foreground is `15 (A2)` when interactive and `grey (A1)` when it is a non-interactive label. `ambiguous` means the rule does not decide; the cell then names what the rule would give if interactivity decided, and the ambiguity.
- *differs*: `yes`, `no` or `ambiguous`. A `no` that depends on an open ambiguity names it in the concept cell.

## The four "already partly true" claims (hs-d501)

| # | Claim | Verdict | Evidence |
|---|---|---|---|
| 1 | Section and pane titles are grey; tree and Inbox items are bright | **partly** | *Titles: true.* In-pane titles are muted 8: `tui/src/section.rs:49,60-70`. Pane titles are colour 8 unfocused, blue focused: `core/src/host/tmux.rs:92,94`. *Items bright: only by accident of the terminal.* Tree and Inbox names and labels have no colour set (default): `tui/src/tree/rows.rs:299-301`, `tui/src/tree/inbox.rs:259`; whether default is bright is the terminal's (A2). Their coloured glyphs are mostly **base** colours: red 1, yellow 3, magenta 5, cyan 6 (`palette.rs:252-255,274-279`); only the white tier and white dot are bright (15, `palette.rs:255,277`). |
| 2 | Draft Plan-view cards are grey, every other state bright | **partly** | *Draft is grey: true* (8, `palette.rs:183,86`; pill `palette.rs:145-147`, drawn at `tui/src/plan/flow.rs:417-425`). *Others are not bright.* queued 15 (bright, `palette.rs:177,79`); running 2, merging 3, failed 6 (cyan, waiting), failed and stuck 1 (red) are base (`palette.rs:178-182,190`); merged is grey 8 like the draft (`palette.rs:180,82`). Only the queued pill is a bright colour; no pill uses 9-14. |
| 3 | Focus is blue 4 (not bright blue), non-interactive | **true, with exceptions** | `FOCUS = Color::Blue` = 4 (`palette.rs:132`), used by the selected tree name (`palette.rs:135-137`, `tui/src/tree/rows.rs:301`); tmux `pane-active-border-style fg=blue` and the focused title pill `#[fg=blue]` = 4 (`core/src/host/tmux.rs:92,94,154`); Pulse's escalation bar `Color::Blue` = 4 (`palette.rs:104`, hs-d352). Exceptions: five other focus-like marks are `REVERSED`, not blue (A8): the tree and Inbox cursor (`rows.rs:494`, `inbox.rs:293`), the selected Job pill (`plan/flow.rs:555`), the active PR row (`widgets/column.rs:199`), the menu's selected entry (`menu.rs:151`). |
| 4 | Listed PRs are grey/base when no Lead opened them, bright when a Lead did | **false for colours, partly true for dimming** | The state word, checks icon and conflict word get the **same base colours** owned or not: `open` yellow 3 always (`widgets/column.rs:168`); checks passed 8, pending 2, none 8 in both cases (`palette.rs:329-343`). Only two colours differ and neither is bright: failed checks and `conflict` are **cyan 6** when a Lead owns the PR and **red 1** when none does (`palette.rs:331`, `column.rs:162,180`). What separates the two is the **`DIM` modifier** on a PR no Lead owns, over both lines (`column.rs:200-201`), and its title is `DIM` whether owned or not unless the row is the active one (`column.rs:189`). No PR is drawn in 9-14. |

## 1. The Context tree (Context column, top part)

Drawn by `tui/src/tree/rows.rs` with `tui/src/fold.rs`; clicks in `tui/src/tree/view.rs:620-637`, keys `view.rs:392-428`.

| element | interactive | colour today | source | concept colour | differs |
|---|---|---|---|---|---|
| Fold lines `╭`, `├─`, `╰─`, `│` | no | 8 | `fold.rs:49,55,79`; `rows.rs:355-360` | 8 grey (A1) | no |
| Live Context title (its id), unselected | yes: click opens its Manager (`view.rs:627-629`) | default (BOLD) | `fold.rs:54-56`; `rows.rs:306-309` | 15 (A2) | no |
| Context title selected (its Manager is in the chat slot) | yes: same click | 4 | `rows.rs:299-309`; `palette.rs:132-137` | 4 (focus, hs-d351) | no |
| Archived Context row (dot and id) | no: click does nothing (`view.rs:627-629`) | 8 (BOLD) | `rows.rs:486-491`; `palette.rs:283` | 8 grey (A1) | no |
| Live Plan or fork row name, unselected | yes: click or Enter opens the Session (`view.rs:630-636,423`) | default | `rows.rs:299-301` | 15 (A2) | no |
| Plan or fork row name, selected (its Session is in the chat slot) | yes: same | 4 | `rows.rs:299-301`; `palette.rs:132-137` | 4 (focus) | no |
| Archived Plan or fork row (dot, name, counters) | yes: click opens its history (`view.rs:631-632`) | 8 | `rows.rs:486-491` | ambiguous: 8 grey, or bright because it is clickable (A3) | ambiguous |
| Keyboard cursor row (a live Session row, only while the pane has focus) | yes: ↑↓ j/k move, Enter opens | (REVERSED) of the row's colours | `rows.rs:493-495`; `view.rs:310-312` | ambiguous: blue 4 as focus, or reverse video kept (A8) | ambiguous |
| Dot: needs-you, red | no: part of the row (A9) | 1 | `palette.rs:274` | ambiguous: 9 if the row decides (A4) | ambiguous |
| Dot: needs-you, yellow | no: part of the row | 3 | `palette.rs:275` | ambiguous: 11 (A4) | ambiguous |
| Dot: needs-you, magenta | no: part of the row | 5 | `palette.rs:276` | ambiguous: 13 (A4) | ambiguous |
| Dot: needs-you, white | no: part of the row | 15 | `palette.rs:277` | ambiguous: 15 either way (A4) | ambiguous |
| Dot: waiting (half circle) | no: part of the row | 6 | `palette.rs:279,88` | ambiguous: 14 (A4, A7) | ambiguous |
| Dot: working (spinner, 9 frames) | no: part of the row | default | `palette.rs:278` | 15 (A2) | no |
| Dot: idle | no: part of the row | default | `palette.rs:280` | 15 (A2) | no |
| Dot: stopped | no: part of the row | default (DIM) | `palette.rs:281` | ambiguous: DIM or a grey number (A5) | ambiguous |
| Counter: queued (a draft count in phase `plan`: see next row) | no: part of the row | 15 | `rows.rs:241-248`; `palette.rs:292-294,177,79` | ambiguous: 15 either way (A4) | ambiguous |
| Counter: draft (phase `plan`), zero counts, merged | no: part of the row | 8 | `rows.rs:241-248`; `palette.rs:183,180,292-294` | ambiguous: 8 grey, or bright because the row is clickable (A3, A4) | ambiguous |
| Counter: running | no: part of the row | 2 | `palette.rs:178` | ambiguous: 10 (A4) | ambiguous |
| Counter: merging | no: part of the row | 3 | `palette.rs:179` | ambiguous: 11 (A4) | ambiguous |
| Counter: failed, a Lead handles it | no: part of the row | 6 | `palette.rs:182,298-300` | ambiguous: 14 (A4, A7) | ambiguous |
| Counter: failed, `job-stuck` open | no: part of the row | 1 | `palette.rs:190,298-300`; `rows.rs:245` | ambiguous: 9 (A4) | ambiguous |
| Counter separator `⋅` | no | 8 | `palette.rs:287-289`; `rows.rs:230` | 8 grey (A1) | no |
| Closing toggle `╰󰁋 expand` / `╰󰁣 collapse` | yes: click (`rows.rs:555-561`); no key | 8 | `fold.rs:61-64`; `rows.rs:359` | ambiguous: 8 grey, or bright because it is clickable (A3) | ambiguous |
| Closing `╰─` (nothing to fold) | no | 8 | `fold.rs:69` | 8 grey (A1) | no |
| ` no Contexts` | no | default (DIM) | `rows.rs:440` | ambiguous: DIM or a grey number (A5) | ambiguous |
| Scroll cues `▲ n more`, `▼ n more` | no (wheel scrolls) | default (DIM) | `dialog/scroll.rs:30`; `rows.rs:466,469` | ambiguous: DIM or a grey number (A5) | ambiguous |
| Footer: the snapshot's error | no | 3 | `view.rs:734`; `palette.rs:125,140` | 3 (non-interactive base) | no |

## 2. The Inbox (Context column, bottom part)

Drawn by `tui/src/tree/inbox.rs` with `tui/src/section.rs` and `tui/src/fold.rs`. Clicks `view.rs:642-654`, `inbox.rs:202-230` (`jump`); keys: Enter jumps, `s` snoozes, `d` dismisses (`view.rs:403-414,423-426`), right click opens a menu (`view.rs:468-472`).

| element | interactive | colour today | source | concept colour | differs |
|---|---|---|---|---|---|
| Title pill `Inbox` (caps and block) | no (never clickable, hs-d475) | 8 (block REVERSED) | `section.rs:60-64`; `inbox.rs:286` | 8 grey (A1) | no |
| Title lead `╭─` and the `─` line to the count | no | 8 | `section.rs:59,65`; `inbox.rs:286` | 8 grey (A1) | no |
| Count pill (`35`) | no | 8 (block REVERSED) | `section.rs:66-70` | 8 grey (A1) | no |
| Row branch `│` | no | 8 | `fold.rs:27,79`; `inbox.rs:255` | 8 grey (A1) | no |
| Tier glyph: red (blocker only the User can clear) | yes: the item's row (click jumps, except kinds with no jump; `s`, `d`, right click) | 1 | `palette.rs:252`; `inbox.rs:251` | ambiguous: 9 (A4, A9) | ambiguous |
| Tier glyph: yellow (PR work) | yes: same | 3 | `palette.rs:253` | ambiguous: 11 (A4, A9) | ambiguous |
| Tier glyph: magenta (a decision before the work runs) | yes: same | 5 | `palette.rs:254` | ambiguous: 13 (A4, A9) | ambiguous |
| Tier glyph: white (an unread reply) | yes: same | 15 | `palette.rs:255` | ambiguous: 15 either way (A4, A9) | ambiguous |
| Merge spinner in the glyph cell (while an item's PR merge runs) | no: a click on it does nothing (`view.rs:673`) | default | `inbox.rs:252-254` | ambiguous: a busy marker inside an interactive row (A4) | ambiguous |
| Item label | yes: the row | default | `inbox.rs:259` | 15 (A2) | no |
| Item label that is an OSC 8 link (`plan-review`, `review-waiting`, `pr-ready`, `review-requested`) | yes: terminal modifier-click opens the url; plain click jumps | default (the terminal underlines or not; cpd sets no colour) | `inbox.rs:295-298` | 15 (A2) | no |
| Age (`42s`, `5m`) | no: part of the row | 8 | `inbox.rs:261` | 8 grey (A1) | no |
| Keyboard cursor on an item row | yes: ↑↓ move, Enter jumps | (REVERSED) of the row's colours | `inbox.rs:292-294` | ambiguous: blue 4 as focus, or reverse video kept (A8) | ambiguous |
| Closing toggle `╰󰁋 expand` / `╰󰁣 collapse` | yes: click | 8 | `fold.rs:61-64`; `inbox.rs:301` | ambiguous: 8 grey, or bright because it is clickable (A3) | ambiguous |
| Closing `╰─` (3 items or fewer) and `╰󰁕 up to date, drink some water` (none) | no | 8 | `fold.rs:69,74`; `inbox.rs:310` | 8 grey (A1) | no |
| Scroll cues `▲ n more`, `▼ n more` | no (wheel scrolls) | default (DIM) | `inbox.rs:267-268,304,307`; `dialog/scroll.rs:30` | ambiguous: DIM or a grey number (A5) | ambiguous |

## 3. The Plan view (centre-top pane)

Drawn by `tui/src/plan/mod.rs`, `flow.rs` (Job pills), `boxes.rs` (PR boxes), `pulse.rs` (Pulse, shown when a Manager is in the chat slot); the legend popup is `tui/src/widgets/legend.rs`. Clicks and keys: `plan/mod.rs:513-559`. **"Cards by phase" has no element:** nothing in the views is coloured by a Plan's phase. `palette::phase` and `palette::attempt_status` (`palette.rs:203,213`) have no caller outside `palette.rs`'s tests, so `Meaning::Grounding` (15) and the phase colours are only ever drawn in the legend's Meanings table (`legend.rs:122`). The cards are the Job pills, by status.

A pill is black text (0) on a block of the status colour, with a cap on each side in that colour on the terminal's background (`palette.rs:145-147`; `flow.rs:417-425`); the "colour today" below is the block (and caps).

| element | interactive | colour today | source | concept colour | differs |
|---|---|---|---|---|---|
| Job pill: draft | yes: click or arrows select it (it has no Attempt, so it opens nothing: `plan/mod.rs:585-587`) | 8 | `palette.rs:183`; `flow.rs:47` | ambiguous: 8 grey, or bright because it selects (A3, A9) | ambiguous |
| Job pill: queued | yes: select only (no Attempt yet) | 15 | `palette.rs:177`; `flow.rs:48` | 15 | no |
| Job pill: running | yes: click opens or closes its Worker pane (`plan/mod.rs:585-595`) | 2 | `palette.rs:178`; `flow.rs:49` | 10 | yes |
| Job pill: merging | yes: same | 3 | `palette.rs:179`; `flow.rs:50` | 11 | yes |
| Job pill: merged | yes: same | 8 | `palette.rs:180`; `flow.rs:51` | ambiguous: 8 grey, or bright because it opens a Worker (A3) | ambiguous |
| Job pill: failed (its Lead handles it) | yes: same | 6 | `palette.rs:182`; `flow.rs:52` | 14 | yes |
| Job pill: failed and stuck (`job-stuck` open) | yes: same | 1 | `palette.rs:190`; `flow.rs:53` | 9 | yes |
| Job pill text (icon, key, tier, `aN`) | part of the pill | 0 (black) | `palette.rs:146`; `flow.rs:424-425` | 0 (unchanged) | no |
| Selected Job pill (body) | yes: it is the selection (`ui.selected`) | (BOLD, REVERSED) of the status colour | `flow.rs:555` | ambiguous: blue 4 as focus, or reverse video kept (A8) | ambiguous |
| Chart lines (`─`, `│`, junctions, arrows) between pills | no | 8 | `flow.rs:403-409` | 8 grey (A1) | no |
| PR box border (rounded) and the `┬` exit and `󱦳` arrow glyphs | no | 8 | `boxes.rs:202,205,227,232-233` | 8 grey (A1) | no |
| PR box title: key (`pr-links`) | no | default (BOLD) | `boxes.rs:104` | grey (A1): it is a label | yes |
| PR box title: ` · repo 󱦰 base · ` and `#12` | no (`#12` is inside the link, next row) | default | `boxes.rs:104,107` | grey (A1): label | yes |
| PR box title: state word `open` | yes: inside the OSC 8 link (modifier-click opens the PR) | 3 | `boxes.rs:94,109`; `palette.rs:305,81` | ambiguous: 11 (A4) | ambiguous |
| PR box title: `merged` | yes: same link | 8 | `boxes.rs:94,109`; `palette.rs:306,82` | ambiguous: 8 grey, or bright because linked (A3, A4) | ambiguous |
| PR box title: `closed` (Lead handles it) | yes: same link | 6 | `palette.rs:308` | ambiguous: 14 (A4, A7) | ambiguous |
| PR box title: `closed` (Lead asks the User) | yes: same link | 1 | `boxes.rs:93`; `palette.rs:314` | ambiguous: 9 (A4) | ambiguous |
| PR box title: `conflict` (box PRs are always owned) | yes: same link | 6 | `boxes.rs:92`; `palette.rs:325` | ambiguous: 14 (A4, A7) | ambiguous |
| PR box title: Lead-working dot | no | default | `boxes.rs:111`; `palette.rs:278` | 15 (A2), or grey (A1) | ambiguous |
| PR box title: link cue `󰏌` | yes: part of the link | default | `boxes.rs:113`; `link.rs:25` | 15 (A2) | no |
| PR box title: PR status before a pull request exists (`waiting`, `dispatch`) | no | default (DIM) | `boxes.rs:115` | ambiguous: DIM or a grey number (A5) | ambiguous |
| ` no Jobs ` in an empty box | no | default (DIM) | `boxes.rs:215` | ambiguous: DIM or a grey number (A5) | ambiguous |
| Plan review row: ` Plan review r2 ` | yes when the port is known: the link starts at `Plan` | default | `plan/mod.rs:105-114,117-126` | 15 (A2) | no |
| Plan review row: `pending` | yes: inside the same link | 3 | `plan/mod.rs:108`; `palette.rs:81` | ambiguous: 11 (A4) | ambiguous |
| Foot line: ` no Lead session in the centre`, `no PR yet: cpd pr add`, ` no Plans yet` | no | default (DIM) | `plan/mod.rs:197,198,258` | ambiguous: DIM or a grey number (A5) | ambiguous |
| Foot line: `no Plan yet` and the view's error | no | 3 | `plan/mod.rs:195,199,256`; `palette.rs:125,140` | 3 (non-interactive base) | no |
| Pulse row: Plan id | yes: click opens the Plan's Lead (`plan/mod.rs:517-519`) | default | `pulse.rs:79` | 15 (A2) | no |
| Pulse bar: error or Job failed | no: part of a clickable row (A9) | 1 | `pulse.rs:52`; `palette.rs:101` | ambiguous: 9 (A4) | ambiguous |
| Pulse bar: Plan or ask Event | no: part of the row | 5 | `palette.rs:102` | ambiguous: 13 (A4) | ambiguous |
| Pulse bar: PR opened | no: part of the row | 3 | `palette.rs:103` | ambiguous: 11 (A4) | ambiguous |
| Pulse bar: escalation | no: part of the row | 4 | `palette.rs:104` | 4 (a focus change, hs-d352) | no |
| Pulse bar: Job or PR merged | no: part of the row | 2 | `palette.rs:105` | ambiguous: 10 (A4) | ambiguous |
| Pulse bar: any other activity | no: part of the row | 15 | `palette.rs:106` | ambiguous: 15 either way (A4) | ambiguous |
| Pulse: empty bucket `▁` | no | 8 | `pulse.rs:46` | 8 grey (A1) | no |
| Pulse: age (`12m`) | no | default (DIM) | `pulse.rs:74` | ambiguous: DIM or a grey number (A5) | ambiguous |
| Pulse: `quiet 41m` | no: part of the row | 3 | `pulse.rs:72` | ambiguous: 11 (A4) | ambiguous |
| Legend popup: its title pill | no | 8 (block REVERSED) | `legend.rs:268`; `section.rs:60-64` | 8 grey (A1) | no |
| Legend popup: section titles (`Inbox`, `Pulse`, …) | no | default (BOLD, UNDERLINED) | `legend.rs:85` | grey (A1): label | yes |
| Legend popup: column header | no | default (DIM) | `legend.rs:94` | ambiguous: DIM or a grey number (A5) | ambiguous |
| Legend popup: scroll position `1-20/60` | no | 8 | `legend.rs:283` | 8 grey (A1) | no |
| Legend popup: Meanings swatches `██` (9 colours) | no | the 9 `Meaning` colours: 15, 2, 3, 8, 1, 5, 15, 8, 6 | `legend.rs:122`; `palette.rs:79-88` | follows what each Meaning is drawn as in the views (A13) | ambiguous |
| Legend popup: Job sample pills and counters | no | the pill and counter colours of the Job rows above | `legend.rs:131-144` | follows the pills (A13) | ambiguous |
| Legend popup: Session Dots, PR states, checks, Inbox tiers, Pulse samples | no | the dot, PR, checks, tier and Pulse colours above | `legend.rs:164-207`; `pulse.rs:131-144` | follows the views (A13) | ambiguous |
| Legend popup: all other words (meanings, intro line) | no | default | `legend.rs:100-102` | grey (A1): labels | yes |

## 4. The Deploy column (right column)

Drawn by `tui/src/widgets/column.rs`; clicks `tui/src/widgets/mod.rs:185-201`. The PR menu is `tui/src/menu.rs` driven by `widgets/mod.rs:100-120`.

A PR **owned** by a Lead (`pr.lead` is `Some`): left click on its two rows opens the Lead, except on `󰏌` (terminal link) and the merge icon (`column.rs:36-39`). A PR **no Lead owns** has no left click except on `󰏌` and the merge icon, but the right-click menu works on both (`widgets/mod.rs:185-188`, `column.rs:51-97`): see A11.

| element | interactive | colour today | source | concept colour | differs |
|---|---|---|---|---|---|
| Machine row: hostname | no | default (BOLD) | `column.rs:278`; `bold` `column.rs:125-127` | grey (A1): label | yes |
| Machine row: CPU, GPU, memory line | no | default | `column.rs:278`; `column.rs:117-123` | grey (A1): label | yes |
| Section title ` ──PRs───…──3 ` (lead, pill, line, count pill) | no | 8 (pill REVERSED) | `column.rs:208-210,277`; `section.rs:48-73` | 8 grey (A1) | no |
| PR first line: ` owner/repo#12` | owned: yes (click opens the Lead); not owned: right click only | default (BOLD) | `column.rs:196` | owned 15 (A2); not owned: ambiguous (A11) | ambiguous |
| PR first line: `󰏌` (last column) | yes: terminal modifier-click opens the PR | default | `column.rs:196,321-325`; `link.rs:25` | 15 (A2) | no |
| PR second line: state word `open` or `draft` (owned or not) | owned: yes; not owned: right click only | 3 | `column.rs:168`; `palette.rs:305,81` | ambiguous: 11 (A4, A11) | ambiguous |
| PR second line: Lead-working dot | no: part of the row | default | `column.rs:164,170-172`; `palette.rs:278` | 15 (A2) | no |
| PR second line: checks passed icon | no: part of the row | 8 | `column.rs:175`; `palette.rs:339` | ambiguous: 8 grey, or bright (A3, A4) | ambiguous |
| PR second line: checks pending icon | no: part of the row | 2 | `palette.rs:341` | ambiguous: 10 (A4) | ambiguous |
| PR second line: checks none icon | no: part of the row | 8 | `palette.rs:342` | ambiguous: 8 grey, or bright (A3, A4) | ambiguous |
| PR second line: checks failed icon, **owned** | no: part of the row | 6 | `palette.rs:331` | ambiguous: 14 (A4, A7) | ambiguous |
| PR second line: checks failed icon, **not owned** | no: part of the row | 1 | `palette.rs:340` | ambiguous: 9 (A4, A11) | ambiguous |
| PR second line: review text (`no review`, `approved`, `changes requested`) | no: part of the row | default | `column.rs:176-178` | 15 (A2) or grey (A1) | ambiguous |
| PR second line: ` conflict`, **owned** | no: part of the row | 6 | `column.rs:180-181`; `palette.rs:325` | ambiguous: 14 (A4, A7) | ambiguous |
| PR second line: ` conflict`, **not owned** | no: part of the row | 1 | `palette.rs:321` | ambiguous: 9 (A4, A11) | ambiguous |
| PR second line: title (not the active row) | no: part of the row | default (DIM) | `column.rs:189` | ambiguous: DIM or a grey number (A5) | ambiguous |
| PR second line: merge icon `󰘭` (a clean PR) | yes: click opens the merge popup (`widgets/mod.rs:191-192`) | default | `column.rs:194` | 15 (A2) | no |
| Merge spinner (replaces the merge icon while its merge runs, hs-d495) | no: a click on it does nothing (`column.rs:301-302`) | default | `column.rs:194`; `palette.rs:227-228` (`SPINNER`) | ambiguous: a busy marker (A4) | ambiguous |
| Active PR row (its Plan is in the chat slot): both lines | yes | (REVERSED, BOLD) of the row's colours | `column.rs:198-199` | ambiguous: blue 4 as focus, or reverse video kept (A8) | ambiguous |
| PR row no Lead owns: both lines | right click only (A11) | (DIM) over the row's colours | `column.rs:200-201` | ambiguous: DIM, a grey number, or bright (A5, A11) | ambiguous |
| ` no open PRs` | no | default (DIM) | `column.rs:310` | ambiguous: DIM or a grey number (A5) | ambiguous |
| Scroll cues `▲ n more`, `▼ n more` | no | default (DIM) | `column.rs:314,318`; `dialog/scroll.rs:30` | ambiguous: DIM or a grey number (A5) | ambiguous |
| The snapshot's error (last row) | no | 3 | `column.rs:335`; `palette.rs:125,140` | 3 (non-interactive base) | no |
| PR menu (right click): border `┌─┐` | no | default | `menu.rs:154` | ambiguous: grey like other borders, or default (A12) | ambiguous |
| PR menu: entries `Merge`, `Copy Link`, `Go to Lead` | yes: click, ↑↓, Enter | default | `menu.rs:139-150` | 15 (A2) | no |
| PR menu: selected entry | yes | (REVERSED) | `menu.rs:151` | ambiguous: blue 4 as focus, or reverse video kept (A8) | ambiguous |
| PR menu: busy `Merge ⠿` while the merge runs | no: click ignored (`menu.rs:112`) | default | `menu.rs:146` | ambiguous: busy marker (A4) | ambiguous |

## 5. The status line (tmux, bottom row; hs-d479)

One tmux format, `STATUS_FORMAT` (`core/src/host/tmux.rs:230-247`), fed by the engine (`engine/src/watch/workers.rs`). Only the three buttons have a click range (`range=user|reset|legend|logs`, `tmux.rs:232,234,236`).

| element | interactive | colour today | source | concept colour | differs |
|---|---|---|---|---|---|
| `─` line: leading, joins, fill, closing | no | 8 | `tmux.rs:204,238` | 8 grey (A1) | no |
| Button pills `󰨝 󰨁 󰧭` (reset layout, legend, logs): caps | yes: click (`STATUS_CLICK`, `tmux.rs:180`) | 8 | `tmux.rs:232,234,236` | ambiguous: bright 15, or 8 because they are pills (A3) | ambiguous |
| Button pills: glyph block | yes: same | 8 (REVERSED: glyph in the terminal background) | `tmux.rs:232,234,236` | ambiguous: bright 15, or 8 (A3) | ambiguous |
| Preview pill `preview: <path> <sha>` (only while `@cpd_preview` is set) | no | 3 (REVERSED) | `tmux.rs:237` | 3 (non-interactive base) | no |
| Preview pill's leading join `─` | no | 8 | `tmux.rs:237` | 8 grey (A1) | no |
| Inbox counter `inbox 8`, text, most urgent item red | no: a click does nothing (docs/commands.md:2362) | 1 on bg 236 | `engine/src/watch/workers.rs:65,79`; `tmux.rs:241` | 1 (non-interactive base) | no |
| Inbox counter, yellow | no | 3 on bg 236 | `workers.rs:65,80` | 3 | no |
| Inbox counter, magenta | no | 5 on bg 236 | `workers.rs:65,81` | 5 | no |
| Inbox counter, white (tmux `white`) | no | **7** on bg 236 (the tree's white tier is 15: A10) | `workers.rs:65,82`; `palette.rs:255` | 7 or 8, see A1, A10 | ambiguous |
| Inbox counter, no items (`inbox 0`) | no | default (DIM) on bg 236 | `workers.rs:66` | ambiguous: DIM or a grey number (A5) | ambiguous |
| Counter pills' caps and background | no | 236 (above 15: a 256-palette grey) | `tmux.rs:241,243,245` | ambiguous (A6) | ambiguous |
| Workers text: `workers `, `/M · `, `waiting`, `queued` | no | default (DIM) on bg 236 | `workers.rs:119,130` | ambiguous: DIM or a grey number (A5) | ambiguous |
| Workers: `N` running above 0 | no | 2 on bg 236 | `workers.rs:107-108` | 2 (non-interactive base) | no |
| Workers: `N` at 0 | no | 8 on bg 236 | `workers.rs:108` | 8 grey (A1, A6) | no |
| Workers: `W` waiting above 0 | no | 3 on bg 236 | `workers.rs:113` | 3 | no |
| Workers: `W` at 0, `Q` | no | default (DIM) on bg 236 | `workers.rs:113,119` | ambiguous: DIM or a grey number (A5) | ambiguous |

## 6. Pane borders and pane titles (tmux; hs-d475, hs-d489)

`core/src/host/tmux.rs`: `pane-border-lines heavy` (`:150`), `pane-border-style` (`:153`), `pane-active-border-style` (`:154`), `BORDER_FORMAT` (`:91-97`). Panes with a title: the three views (`Context`, `Plan`, `Deploy`), a Worker pane (`Worker · <job> (Read-Only)`), a Lead's pane when `@cpd_title` is set. The chat and history panes draw none (`tmux.rs:93-94`). A title is never clickable (hs-d475).

| element | interactive | colour today | source | concept colour | differs |
|---|---|---|---|---|---|
| Border lines, unfocused pane | no (tmux lets a border be dragged to resize: A12) | 8 | `tmux.rs:153` | 8 grey (A1) | no |
| Border lines, focused pane | no | 4 | `tmux.rs:154` | 4 (focus) | no |
| Pane title pill, unfocused: the `━` before it, the caps and the block | no | 8 (block REVERSED) | `tmux.rs:92,94` | 8 grey (A1) | no |
| Pane title pill, focused: the `━`, caps and block | no | 4 (block REVERSED) | `tmux.rs:92,94` | 4 (focus, hs-d351) | no |

## 7. Dialogs, popups and menus

Dialogs (`tui/src/dialog/`), the merge popup (`tui/src/merge/mod.rs`), the logs dialog (`tui/src/logs/`), the legend (section 3) all run in a tmux `display-popup`. Menus are cpd's own (`tui/src/menu.rs`; the tree's and the Deploy column's right-click menus), plus tmux's built-in pane menu.

| element | interactive | colour today | source | concept colour | differs |
|---|---|---|---|---|---|
| Popup frame (`┌─┐`, single line) and its ` title ` (`-T`) | no | default (popup-style and popup-border-style `default`; tmux 3.8 line style `single`) | `tmux.rs:157-158`; `dialog/mod.rs:219-229`; `legend.rs:293-297` | ambiguous: grey, blue (a modal in focus) or default (A12) | ambiguous |
| Dialog title (inside the popup) | no | 8 (pill REVERSED) | `dialog/view.rs:220`; `section.rs:48-73` | 8 grey (A1) | no |
| Dialog body text | no | default | `dialog/view.rs:227,236` | default (A2: content) | no |
| Input field text | yes: typing | default | `dialog/view.rs:98-109` | 15 (A2) | no |
| Input field cursor | yes | (REVERSED) | `dialog/view.rs:107` | ambiguous: blue 4 as focus, or reverse video kept (A8) | ambiguous |
| Validation refusal text | no | 3 | `dialog/view.rs:232`; `palette.rs:125,140` | 3 (non-interactive base) | no |
| Buttons: first (the default action: Yes, OK, Merge) | yes: click, Enter | (REVERSED) | `dialog/view.rs:245-246`; `merge/mod.rs:157` | ambiguous: blue 4 as focus, or bright (A8, A3) | ambiguous |
| Buttons: the others (No, Cancel) | yes: click | default | `dialog/view.rs:245`; `merge/mod.rs:158` | 15 (A2) | no |
| Dialog position `1-5/12` | no | default (DIM) | `dialog/view.rs:253` | ambiguous: DIM or a grey number (A5) | ambiguous |
| Dialog scroll cues | no | default (DIM) | `dialog/scroll.rs:30` | ambiguous: DIM or a grey number (A5) | ambiguous |
| Merge popup: `<owner>/<repo>#12` | no | default (BOLD) | `merge/mod.rs:151` | grey (A1): label | yes |
| Merge popup: title line and ` method: … ` | no | default | `merge/mod.rs:153-156` | default (A2: content) | no |
| Logs dialog: title pill `Logs` | no | 8 (pill REVERSED) | `logs/view.rs:174`; `section.rs:48-73` | 8 grey (A1) | no |
| Logs dialog: event time `12:03:41` | no | default (DIM) | `logs/view.rs:129-131,166` | ambiguous: DIM or a grey number (A5) | ambiguous |
| Logs dialog: event text | no (the wheel and keys only scroll) | default | `logs/view.rs:154-159,166` | default (A2: content) | no |
| Logs dialog: `error` Event addressed to a Lead | no | 6 | `logs/view.rs:156`; `palette.rs:88` | 6 (non-interactive base) | no |
| Logs dialog: `error` Event for the User | no | 1 | `logs/view.rs:157`; `palette.rs:83` | 1 | no |
| Logs dialog: the Event the dialog opened at | no | (REVERSED) across its line | `logs/view.rs:189-192` | ambiguous: blue 4 as focus, or reverse video kept (A8) | ambiguous |
| Logs dialog: foot error | no | 3 | `logs/view.rs:178`; `palette.rs:125,140` | 3 | no |
| Menu (tree, PR): selected entry | yes | (REVERSED) | `menu.rs:151` | ambiguous: blue 4 as focus, or reverse video kept (A8) | ambiguous |
| Menu (tree, PR): border and entries | border no; entries yes (click, ↑↓, Enter) | default | `menu.rs:143-155` | border: ambiguous (A12); entries 15 (A2) | ambiguous |
| tmux's own pane menu (right click on a pane that is not mouse-aware: the default `MouseDown3Pane` binding) | yes | `menu-style` default; `menu-border-style` and `menu-selected-style` are **not set**: tmux 3.8 gives dark grey (`themedarkgrey`) and `bg=themeyellow,fg=themeblack` (names resolve through tmux's theme: no number known) | `tmux.rs:159`; checked with tmux 3.8 on a private server (`show -g menu-border-style`) | ambiguous (A12) | ambiguous |

## 8. cpd's messages in the pi chat (`pi/lib/render.ts`; hs-d488, hs-d499)

pi's theme colours are **named**. The active theme, `~/.pi/agent/settings.json:3` → `~/.pi/agent/themes/ghostty-sync-789d1c84.json` (a machine-local file generated by `@ogulcancelik/pi-ghostty-theme-sync`, unverified; the Ghostty theme is `Vesper`, `~/.config/ghostty/config:10`), maps them to **hex, not to a 0-15 number**: `muted` → `#8f8f8f` (`:31` → `:13`), `customMessageBg` → `#211b1f` (`:38` → `:21`), `customMessageText` → empty = the terminal's default (`:39`). Only these two named colours and one SGR code are used by cpd's own messages (`render.ts:147,151,202`).

| element | interactive | colour today | source | concept colour | differs |
|---|---|---|---|---|---|
| Title row lead `╭─` and the left cap | no | theme: `muted` (`#8f8f8f`; number unknown) | `render.ts:149` | 8 grey (A1, A14) | ambiguous |
| Title pill text (`cpd Message from pi-pin`) | no: it never toggles by itself, but a click anywhere on the message toggles it (A15) | theme: `muted` made the block by SGR 7 reverse video (`\x1b[7m`) | `render.ts:151,159` | 8 grey (A1, A14) | ambiguous |
| Title pill right cap | no | theme: `muted` | `render.ts:159` | 8 grey (A1, A14) | ambiguous |
| Rail `│ ` before each content line | no | theme: `muted` | `render.ts:152` | 8 grey (A1, A14) | ambiguous |
| Content lines | no (a click toggles the message: A15) | theme: `customMessageText` = terminal default | `render.ts:161` | default (A2: content) | no |
| Toggle row `╰󰁋 expand` / `╰󰁣 collapse` | yes: click anywhere on the message, or Ctrl+O (`render.ts:208-212`) | theme: `muted` | `render.ts:162` | ambiguous: 8 grey, or bright because it toggles (A3, A15) | ambiguous |
| The row's background (every line, padded to the pane width) | no | theme: `customMessageBg` (`#211b1f`; no 0-15 number) | `render.ts:202` | not a foreground colour: unchanged (A14) | no |

Tool rows (read, bash, edit, thinking, …; hs-d499 (2)) are drawn by `pi/compact-tools/extensions/*.ts` with pi's theme colours (`dim`, `muted`, `success`, `error`, `accent`, `toolOutput`, …: for example `compact-tools-codex-rows.ts:232-293`); they are not cpd's own messages and are **out of scope of this audit** (the Job's Out of scope: pi theme colours other than those cpd's own messages use).

## 9. Other

| element | interactive | colour today | source | concept colour | differs |
|---|---|---|---|---|---|
| Worker pane: `you` label (bold) and the User's lines | no (keys and wheel scroll only) | 6 (label BOLD) | `worker/render.rs:34-36`; `palette.rs:126` | 6 (non-interactive base; A7) | no |
| Worker pane: `agent` label (bold) | no | 2 (BOLD) | `worker/render.rs:39-40`; `palette.rs:127` | 2 | no |
| Worker pane: agent text | no | default | `worker/render.rs:41` | default (A2: content) | no |
| Worker pane: `→ tool ` name | no | 3 | `worker/render.rs:48-49`; `palette.rs:125` | 3 | no |
| Worker pane: time `12:03:41` and tool result `󰘌 last` | no | default (DIM) | `worker/render.rs:28,53` | ambiguous: DIM or a grey number (A5) | ambiguous |
| Worker pane: foot status line | no | default (DIM) | `worker/view.rs:44` | ambiguous: DIM or a grey number (A5) | ambiguous |
| Worker pane: foot note when the session file is unreadable | no | 1 | `worker/view.rs:46`; `palette.rs:83` | 1 | no |
| Worker pane: placeholder `No session file at …` | no | default (DIM) | `worker/mod.rs:192` | ambiguous: DIM or a grey number (A5) | ambiguous |
| History pane (an archived Session's transcript): title line | no | default (BOLD) | `history/view.rs:43` | grey (A1): label | yes |
| Chat pane placeholder (no Lead in the slot) | no | default (DIM) | `chat.rs:35` | ambiguous: DIM or a grey number (A5) | ambiguous |
| tmux message line (`display-message`, a failed status-button click) | no | **not set by cpd**: tmux 3.8's `message-style` is `bg=themeyellow,fg=themeblack` (checked on a private server) | `docs/commands.md:2362` ("a failed click reports with `display-message -c <client>`"); no `message-style` in `SERVER_OPTIONS`, `core/src/host/tmux.rs:140-166` | ambiguous (A12) | ambiguous |
| tmux copy and view mode highlight | yes | **not set by cpd**: tmux 3.8's `mode-style` is `bg=themeyellow,fg=themeblack` (checked) | no `mode-style` in `SERVER_OPTIONS`, `core/src/host/tmux.rs:140-166` | ambiguous (A12) | ambiguous |
| OSC 8 links (the `󰏌` cells, link labels) | yes: terminal modifier-click | the cell's own colour; cpd adds none | `tui/src/link.rs:30-60` | (follows the cell it is on) | no |

## Ambiguities for the User

Each is a question the rule in hs-d501 does not decide. Nothing is decided here.

**A1. Which grey do titles, labels, lines and borders use: 7 or 8?** hs-d479 (status line) and hs-d489 (borders, titles) say colour **8**, and the code is 8 everywhere muted (`palette.rs:86`). The concept says titles and labels are "grey", which in the User's numbers is **7**, with 8 "counted as a grey". Options: (a) 8 for everything non-interactive (no code change); (b) 7 for titles and labels, 8 for lines and borders (two greys); (c) 7 for everything. Affects every row that says `8 grey (A1)`, and the labels that are default today (`grey (A1)`): the title pills, fold lines, ages, counter separators, chart and PR box lines, `─` line of the status line, pane borders and title pills (tmux `colour8`), the pi chat's title pill. Related: a bright-grey **15** cannot also be the white of "unread" (A10).

**A2. What is the terminal's default foreground in the concept?** Many interactive texts have no colour set (tree names, Inbox labels, menu entries, buttons, PR first line) and the concept calls interactive things bright. Options: (a) leave them default and treat the User's terminal default as bright grey 15 (unverified: it is the terminal's, not cpd's); (b) set `15` explicitly on every interactive text; (c) set grey on non-interactive labels that are default today (the machine row, PR box title, legend and dialog words) and leave content (dialog body, logs, chat message text) default. Affects every `15 (A2)` and `default (A2: content)` row.

**A3. Interactive elements drawn muted (8).** Toggles `╰󰁋 expand` / `╰󰁣 collapse` (tree, Inbox, chat message), archived Plan and fork rows (click opens history), merged and draft Job pills, merged PR state, status-line buttons (grey 8 pills), checks passed and none icons. The rule says interactive means bright; but grey also means "archived", "done" and "nothing". Options: (a) interactivity wins: bright grey 15 (a toggle, a button, a merged pill turn white); (b) the state wins: they stay 8 and interactivity is shown by position (a toggle on a closing row, a button in a pill); (c) split: toggles and buttons bright, state grey (archived rows, merged, draft) stays 8.

**A4. State colours that carry meaning and interactivity at once.** `Meaning` colours (`palette.rs:77-88`) are drawn on glyphs and words inside rows the User can click: tree dots and counters, Inbox tier glyphs, Plan-view PR state words (in a link), Pulse bars, PR state, checks, `conflict` in the Deploy column, `pending`, `quiet`, busy spinners. The concept gives these the bright pair if the row decides (the "if" values in the tables) but then one meaning has two colours (the status line's `inbox 8` stays base because it is not clickable: red 1 there, red 9 in the tree). Options: (a) interactivity wins: every state glyph in a clickable row goes bright; (b) meaning wins: state colours stay base, the interactive name or label carries bright; (c) bright only where the coloured element is itself the hit target (a Job pill, an Inbox tier glyph) and base where it is a read-only marker (a counter, a checks icon). Whole Job pills are decided by the rule today (yes: 9, 10, 11, 14) but depend on this answer.

**A5. `DIM` versus a grey number.** "Muted" is drawn as the `DIM` modifier on the terminal's default colour in: `no Contexts`, scroll cues (tree, Inbox, Deploy, dialogs), the Pulse age, `no Jobs`, the PR status before a pull request exists, `no PR yet`, the legend header, dialog position, the logs' times, the Worker pane's times, results, foot and placeholder, the chat placeholder, the Deploy column's `no open PRs`, a PR no Lead owns (both lines), a PR's title, a stopped dot, and the status line's `workers …`, `inbox 0` (tmux `dim`). It is not a number, so it cannot be 7 or 8. Options: (a) keep DIM for "inactive, secondary text" and use a number only for pill and line greys; (b) replace every DIM with the grey chosen in A1 (explicit colour, not terminal-dependent); (c) keep DIM only on a default foreground that is already grey. DIM's rendering also depends on the terminal (unverified).

**A6. `bg=colour236` pills.** The status line's Inbox counter and workers pills have a 256-palette background, **236**, outside the 0-15 concept, because `DIM` does not read on a colour 8 background (`tmux.rs:230-247`, docs/contract.md §6.1; unverified on the User's palette). The text on it carries state colours (A4) and DIM (A5). Options: (a) keep 236; (b) make them grey pills like the buttons (reverse of 8 or of 7) and drop DIM there (A5 (b)); (c) a plain `─` line with coloured text and no pill background.

**A7. Cyan is Waiting and the chat's `you` label (hs-d362).** Cyan 6 is `Meaning::Waiting` (waiting dot, a Lead-handled failed counter, owned failed checks, owned conflict, a Lead-handled failed Job pill, a Lead-handled closed PR, a Lead-addressed `error` in the logs) and `Ui::You` (the Worker pane's `you` label and the User's lines, `palette.rs:126`; the User kept both, hs-d362). In the concept, interactive Waiting things would turn 14 (bright cyan) while the Worker pane's non-interactive `you` stays 6. Options: keep one cyan family, or let the interactive ones differ.

**A8. Focus and selection drawn as reverse video, not blue.** hs-d351 and hs-d352 make blue 4 the one focus colour, and today only the tree's selected name, focused panes and Pulse's escalation bar use it. The keyboard cursor in the tree and the Inbox, the selected Job pill (BOLD + REVERSED), the active PR row (REVERSED + BOLD), the menu's selected entry, the first dialog button and the dialog input cursor, the Logs dialog's opened-at line are `REVERSED` of whatever colours they hold. Options: (a) keep reverse video as the "cursor / pressed" cue, blue only for "in focus"; (b) turn each into blue; (c) reverse video in blue.

**A9. Whole row or part of it.** A tree Plan row is clickable anywhere (dot, name, counters); an owned PR is clickable on both lines except `󰏌` and the merge icon; an Inbox row is clickable as a row and its label is also a link; a PR box title is clickable only on `#12 state 󰏌`; a Job pill is clickable whole; a Pulse row is clickable whole; every tree row, Inbox item and PR row also has a right-click menu (so by hs-d501's "act on it" every row would be interactive). Options: (a) the colour follows the hit target (the whole row of a Plan is bright, including dot and counters); (b) only the name or label is bright, markers keep meaning colours (A4); (c) right click does not count as interactive.

**A10. White is 15 in the views and 7 in tmux.** The Inbox's `white` tier and the white Session dot are `Color::White` = 15 (`palette.rs:255,277`); the status line's `inbox N` for a white urgency is tmux's `white` = **7** (`workers.rs:82`). Same urgency, two numbers. Options: tmux `colour15` (matches the views, is bright, and the counter is not clickable) or `colour7` everywhere; and whether white-tier items remain bright (interactive) while their non-interactive counterpart turns grey.

**A11. A PR no Lead opened.** The Deploy column draws such a PR `DIM` with red failed checks and conflict, and no left click, but it still has a right-click menu (Merge, Copy Link) and an `󰏌` link and a merge icon. The User's claim 4 treats it as non-interactive (grey/base). Options: (a) non-interactive for colour (left click is what counts): grey, base colours kept; (b) interactive (right click counts): bright like the owned ones, with DIM dropped; and whether red 1 (the User's to fix) versus cyan 6 (a Lead handles it) is kept as the owned/not-owned cue.

**A12. tmux's own chrome and popup frames.** cpd sets `popup-style`, `popup-border-style` and `menu-style` to `default` (hs-d353) but leaves `menu-border-style`, `menu-selected-style`, `message-style` and `mode-style` at tmux 3.8's theme colours (dark grey, yellow `themeyellow`; checked on a private server). Dialog and menu frames are therefore the terminal's default foreground, not grey or blue. A pane border can be dragged with the mouse (tmux `mouse on`, `tmux.rs:141`; the drag is tmux's own behaviour, unverified here), which is "interactive" by the concept's definition. Options: set grey 8 (or 7), blue for the modal in focus, or default for popup frames; set the four unset options; treat borders as non-interactive.

**A13. The legend must show the concept.** The legend (`legend.rs`) draws each Meaning once (`Meaning::ALL`), with one swatch per meaning and one sample per element. If the concept splits one meaning into a base colour (read-only) and a bright colour (interactive, A4), each meaning needs two samples and a line saying which is which. Options: two columns per meaning; one sample with the bright colour and a sentence; no change.

**A14. pi's `muted` is a hex colour of the theme, not 7 or 8.** The chat messages' title, rail and toggle use the theme's `muted` (`#8f8f8f` in this machine's theme, regenerated from the terminal theme by a sync package; unverified) and a background `#211b1f`. They cannot be set to terminal colour 7 or 8 through `theme.fg("muted")`. Options: accept the theme's grey (a deviation from A1), or emit SGR 90 (colour 8) or 37 (colour 7) directly in `render.ts` (a deviation from pi's theme: the user's other themes would not move it).

**A15. A message's toggle is the whole message.** pi gives the renderer one mouse region per message, so a click anywhere (title, rail, content) toggles it (`render.ts:13-14,208-212`), while hs-d480 says a title is "never clickable". Only the toggle row says it is interactive. Options: colour only the toggle row bright and keep the title grey, or treat the whole message block as interactive.


## Summary

### Elements per component

Counted from the tables above (one row is one coloured element or one group of identical elements). `yes` = the rule gives a different colour; `ambiguous` = the rule does not decide (see the A-number in the row); `no` = the concept colour equals today's. "A1" counts the `no` and `ambiguous` rows whose answer would change if A1 chose grey 7.

| component | rows | differ: yes | differ: ambiguous | same: no | rows that depend on A1 |
|---|---|---|---|---|---|
| 1. Context tree | 28 | 0 | 17 | 11 | 4 |
| 2. Inbox | 16 | 0 | 8 | 8 | 6 |
| 3. Plan view (pills, PR boxes, Pulse, legend) | 44 | 8 | 24 | 12 | 13 |
| 4. Deploy column | 27 | 2 | 19 | 6 | 10 |
| 5. Status line | 16 | 0 | 7 | 9 | 4 |
| 6. Pane borders and titles | 4 | 0 | 0 | 4 | 2 |
| 7. Dialogs, popups, menus | 22 | 1 | 10 | 11 | 6 |
| 8. Chat messages (pi) | 7 | 0 | 5 | 2 | 6 |
| 9. Other | 13 | 1 | 6 | 6 | 3 |
| **Total** | **177** | **12** | **96** | **69** | |

What the counts say. (1) The cockpit uses no bright colour 9-14 at all and no grey 7; it uses 0, 1, 2, 3, 4, 5, 6, 8 and 15. (2) The rule decides only 12 rows: four Job pills (running, merging, failed, stuck: 10, 11, 14, 9), and eight default-foreground labels that the concept makes grey (the PR box title's key and repo, the legend's section titles and words, the Deploy column's machine row, the merge popup's PR name, the history pane's title). (3) 96 rows are ambiguous, almost all for four reasons: a **state colour on a marker inside an interactive row** (A4, 38 rows), **a muted element that is also interactive** (A3, 13 rows), **`DIM` is not a number** (A5, 23 rows) and **reverse video as focus** (A8, 9 rows); a row may name more than one. Deciding A4, A5 and A8 resolves most of the rest. (4) 69 rows already match, 43 of them only while A1 keeps 8 for the muted grey and A2 leaves the default foreground alone.

### Proposed order of the six decision rounds

The order puts what every later round reuses first: the grey (A1) is in every round, the bright rule's reach (A3, A4) is first met in the tree.

1. **Titles, borders, status line** (sections 5, 6 and the title pills of 1-4, 7): decides A1 (7 or 8), A5 for the status line's `dim`, A6 (`bg=colour236`), A10 (tmux `white` is 7) and whether the status-line buttons are bright (A3).
2. **Tree and Inbox** (sections 1, 2): A2 (default foreground), A3 (toggles, archived rows), A4 (dots, counters, tier glyphs), A8 (cursor), A9 (whole row).
3. **Plan view** (section 3): Job pills (the only rows the rule decides), A4 for PR state words and Pulse, A8 for the selected pill, A13 for the legend.
4. **Deploy column** (section 4): A11 (a PR no Lead owns), A5 (the DIM title and row), A8 (the active PR row), A7 for cyan.
5. **Dialogs, popups, menus** (section 7): A12 (popup and menu frames, tmux's unset `menu-*`, `message-style`, `mode-style`), A8 (buttons, input cursor, selected entry), A5.
6. **Chat messages** (sections 8, 9): A14 (pi's `muted` is a hex), A15 (the toggle is the whole message), A7 (the Worker pane's `you` cyan).

### What a throwaway prototype changes, per round

A prototype is a Worker Job on a throwaway PR (hs-d504): it is previewed with `sh scripts/preview-tui.sh on <worktree>` and never opened. That script swaps only the cockpit's **view panes** (its header, `scripts/preview-tui.sh:1-9`): a change to tmux options, to the engine's status-line texts or to pi's `render.ts` is **not** covered by it (unverified how a prototype would load them; the tmux ones can be tried on the live server with `tmux -L <socket> set -g <option> <value>`, reversed with `set -gu`, also unverified here). The test `no_colour_outside_the_palette` (`tui/src/palette.rs:562-585`) forbids `Color::` outside `palette.rs`, so every TUI prototype change below is a new function in `palette.rs` that the named call site uses; the "bright" helper (the pair of a base colour, n+8) is one new function next to `color` (`palette.rs:77`).

**Round 1: titles, borders, status line.**
- `tui/src/palette.rs:86` `Meaning::Nothing => Color::DarkGray` → `Color::Gray` for A1 option (b) or (c): one line moves every muted element at once (`section.rs:49`, `fold.rs:49`, `flow.rs:403`, `boxes.rs:202`, `inbox.rs:261`, `palette.rs:288,293`, `pulse.rs:46`) and leaves `Meaning::Done` (`palette.rs:82`) at 8.
- `core/src/host/tmux.rs:92,94`: the two `#[fg=colour8]` of `BORDER_FORMAT` (the unfocused title pill); `:153` `pane-border-style` `fg=colour8`; `:204` the `join!` macro, `:238` the fill: all `colour8` → `colour7` for A1 (b) or (c). `:154` and the `#[fg=blue]` of `:92,94` stay.
- `core/src/host/tmux.rs:232,234,236`: the three buttons' `#[fg=colour8]` → `#[fg=colour15]` (A3 bright option); `:241,243,245` `colour236` → `colour8` or `colour7` (A6).
- `engine/src/watch/workers.rs:82` `"white"` → `"colour15"` (A10); `:66,119,130` `#[dim]` → `#[fg=colour8]` (A5); `:108` `"colour8"` follows A1.

**Round 2: tree and Inbox.**
- `tui/src/palette.rs:274-277` (`dot_span`): `Color::Red`, `Yellow`, `Magenta` → `LightRed`, `LightYellow`, `LightMagenta` (`White` stays 15); `:279` the waiting dot through `Meaning::Waiting` (`:88`) → `LightCyan` (A4, A7).
- `tui/src/palette.rs:250-256` (`tier_span`) and `:292-300` (`count_style`, `failed_count_style`): the `Meaning`s `Failed`, `Review`, `Planning`, `NotStarted`, `InProgress`, `Waiting` through the bright helper (A4); the separator `:288` stays.
- `tui/src/fold.rs:61-64` (`toggle`, the `╰󰁋 expand` row) and `tui/src/tree/rows.rs:485-491` (the archived rows' mute): an interactive-muted style, `Color::White`, from a new `palette` function (A3).
- `tui/src/tree/rows.rs:494` and `tui/src/tree/inbox.rs:293`: `Modifier::REVERSED` → `palette::selected()` as a background (A8); `tui/src/tree/rows.rs:440` and `tui/src/dialog/scroll.rs:30` `Modifier::DIM` → the grey of round 1 (A5).

**Round 3: Plan view.**
- `tui/src/palette.rs:145-147` (`pill`): `.bg(color(m))` → `.bg(bright(color(m)))`, the Job pill and its caps (`plan/flow.rs:417-425`) and the legend's sample pills (`widgets/legend.rs:131,143`) follow; `Meaning::Nothing` (draft) and `Meaning::Done` (merged) stay 8 until A3 is decided.
- `tui/src/plan/flow.rs:555`: `Modifier::BOLD | Modifier::REVERSED` → a `palette::selected()` background (A8).
- `tui/src/plan/boxes.rs:104,107` (the key, repo and `#12` of the title) → a `palette` label style (grey, A1) and `:109` the state word through the bright helper (A4); `:202` stays.
- `tui/src/palette.rs:99-107` (`pulse_kind`): `Color::Red`, `Magenta`, `Yellow`, `Green` → the `Light*` pairs, `Blue` (escalation) and `White` stay (A4); `tui/src/plan/pulse.rs:72,74` (`quiet`, the age).
- `tui/src/widgets/legend.rs:122` (`Meaning::ALL` swatches): two swatches per meaning if A13 asks for it.

**Round 4: Deploy column.**
- `tui/src/widgets/column.rs:198-203`: delete the `Modifier::DIM` arm for `pr.lead.is_none()` and give a PR no Lead owns the style A11 chooses (grey, or the owned style); the `active` arm (`REVERSED | BOLD`) → `palette::selected()` (A8).
- `tui/src/widgets/column.rs:168` (the `open` word), `:175` (checks icon, `palette::checks_of`, `palette.rs:329-343`), `:180-181` (`conflict`, `palette.rs:321,325`): through the bright helper (A4), the owned/not-owned colours kept apart or not (A7, A11).
- `tui/src/widgets/column.rs:189` (title `dim`, A5), `:196` (the PR name `bold`, default), `:278` (machine rows: `bold` and default → a `palette` label style, A1); `tui/src/menu.rs:151,154` (selected entry, border).

**Round 5: dialogs, popups, menus.**
- `core/src/host/tmux.rs:157-159`: `popup-border-style` `default` → `fg=colour8` (or blue); add `["set", "-gq", "menu-border-style", …]`, `menu-selected-style`, `message-style` and `mode-style` after `:159` (A12).
- `tui/src/dialog/view.rs:107` (input cursor) and `:245` (first button) `Modifier::REVERSED` → `palette::selected()` (A8); `:232` `palette::note()` (`palette.rs:125,140`, the refusal, stays 3); `:253` `Modifier::DIM` (A5).
- `tui/src/merge/mod.rs:151,157`; `tui/src/logs/view.rs:129-131` (`dim`), `:156-157` (the `error` colours), `:189-192` (the opened-at line) and `tui/src/menu.rs:151`.

**Round 6: chat messages.**
- `pi/lib/render.ts:147` (`muted`, one function: every title, rail and toggle goes through it, `:149,152,159,162`) → `\x1b[90m` or `\x1b[37m` instead of `theme.fg("muted", …)` for A14; `:151` the pill's `\x1b[7m` stays; `:162` the toggle row in `\x1b[97m` (colour 15) for A15; `:202` the background stays.
- `tui/src/palette.rs:126` (`Ui::You => Color::Cyan`, A7) and `tui/src/worker/render.rs:34-48`; `tui/src/history/view.rs:43` and `tui/src/chat.rs:35` (default and DIM, A1, A5).
- pi must reload to show a `render.ts` change (`docs/research/pi-reload.md`; unverified here).
