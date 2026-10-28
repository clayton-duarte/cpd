# The Inbox as its own tmux pane

Job `inbox-research` of Plan `inbox-flat` (User, pro-d156, 2026-10-28: "research first whether the Inbox can be its own tmux pane"; background pro-n141). The flat Inbox header (`flush_title`, commands.md §13.2) is drawn by the `widgets` pane from its column 0, so a half cell separates it from tmux's `┃` and it does not read as `┠`: tmux owns the border cells. The idea tested here: make the Inbox a pane of its own under the machine pane, so that its header is that pane's top border (`pane-border-format`) and tmux draws the junction. This file changes no code; every claim is from a run on a real tmux 3.8 unless it says *unverified* or *I infer*.

## Answer

| | Question | Answer | Proof |
|---|---|---|---|
| Q1 | Does tmux draw a junction where the Inbox pane's top border meets the vertical border? | **Yes.** `┣` (heavy) at the vertical border's column, on the Inbox header row, in the cockpit's real options. No glyph at the window's right edge (the row just runs to the last cell). At the bottom the status row's own format draws `┻` at the same column. | [Q1](#q1-the-junction) |
| Q2 | Can the header (title pill, count pill, fill) come from `pane-border-format`? | **Yes with conditions.** A branch `@cpd_view = inbox`; tmux draws **2** line cells after the junction, then the format, which reaches the last cell (pane width − 2 cells); the pill lands on the pane's column 3 like today's. The count can be a pane option (`set -p`, a **second exception** to §6.2's "options identify, never state") or the pane's **title** set by the view (OSC 2: no option). Heavy `━` fill is free; a light muted `─` fill needs a computed `#{R:─,n}` and keeps 2 heavy cells in tmux's colour. | [Q2](#q2-the-header-through-pane-border-format) |
| Q3 | Can the pane be as tall as its content, at the bottom, growing up, capped to leave the machine 3 rows? | **Yes with conditions.** The Inbox view calls `resize-pane -t $TMUX_PANE -y N` itself. tmux takes the rows from the pane above, never caps (the machine pane can go to 1 row): the view computes the cap. Conditions: a resize **kills a drag in progress**; **`resize-pane` un-zooms the window** (guard it in one `if -F`); a client resize **re-scales** the pane, so the view re-applies its height on every `SIGWINCH`; reset-layout leaves the Inbox's height alone. | [Q3](#q3-height) |
| Q4 | What does a resize cost the neighbours? | **One `SIGWINCH` to the machine pane and one to the Inbox pane per resize; none to the tree, plan or chat.** A same-size resize sends none. A drag sends one pair per motion. Cheap at the Inbox's rate (it changes on an engine tick at most: *I infer*). | [Q4](#q4-redraw-and-flicker) |
| Q5 | What changes if `widgets` becomes two views? | About 25 files, 900 to 1,300 lines touched (about 400 added, 300 moved or deleted, the rest test edits), 7 docs sections. The fakes need almost nothing: they do not model layout. | [Q5](#q5-what-changes) and [Cost](#cost) |
| Q6 | Wheel, clicks, menu, focus? | **Yes with one real regression: the right-click menu.** It is clamped to its pane and is 6 to 10 rows tall; the Inbox pane is 3 rows with one item. Wheel, clicks and focus work as today. A click on the header or the border selects the Inbox pane (tmux's `MouseDown1Border`); a right click on it does nothing. | [Q6](#q6-mouse-and-keyboard) |
| Q7 | Alternatives? | (a) keep one pane: works, the gap stays (docs must say so). (b) one pane, tmux draws the header: **No**, a pane border is only the pane's edge. (c) floating panes, popups, `pane-border-lines`, `pane-border-indicators`: **No**, none joins a line to a tiled border (probed). | [Q7](#q7-alternatives) |
| Q8 | Recommendation? | **Build it** if the User wants the join, in two PRs (docs, then implementation with its tests), after four decisions (menu, count carrier, header weight, drag/zoom rules). Otherwise accept the gap (a). The join is the only gain; the costs are Q5's size and Q3/Q6's hazards. | [Recommendation](#recommendation-and-open-decisions) |

## What it looks like

tmux 3.8, a 140 x 41 client (40 pane rows, status row 40), the cockpit's real options, layout built as `tui/src/cockpit.rs` `build` then the right column split in two. Text of `capture-pane -p` of a real client; columns 105 to 139 of the right column, rows as numbered; the caps (U+E0B6, U+E0B4) are private-use glyphs that a text file shows as nothing (each is one cell: every row below is exactly 140 cells). Light fill (the variant of Q2 that keeps today's `─` look), 0 items (Inbox pane 3 rows):

```
 0 ━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
 2     ┃ homeserver
 3     ┃ CPU 23%  MEM 41%
36     ┣━━─Inbox────────────────0─
38     ┃ up to date, drink some water
40 ━━━━┻━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

2 items (Inbox pane 5 rows), light fill (`─`, colour 8) and the heavy fill tmux draws by itself (`━`, the border's colour):

```
light fill                                heavy fill
 0 ━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━    0 ━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
 2     ┃ homeserver                        2     ┃ homeserver
 3     ┃ CPU 23%  MEM 41%                  3     ┃ CPU 23%  MEM 41%
34     ┣━━─Inbox────────────────2─        34     ┣━━━Inbox━━━━━━━━━━━━━━2━
36     ┃ ◆ cpd#1 merge conflict    1m      36     ┃ ◆ cpd#1 merge conflict    1m
37     ┃ ◆ cpd#2 merge conflict    2m      37     ┃ ◆ cpd#2 merge conflict    2m
40 ━━━━┻━━━━━━━━━━━━━━━━━━━━━━━━━━━━      40 ━━━━┻━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

Row 0's `┳` and the status row's `┻` are what the cockpit already draws at those columns. Row 34's `┣` is the new one, drawn by tmux: the Inbox header's first line cell is the one next to it. The panes' contents were scripted stand-ins (`inbox.sh`, `machine.sh`, below), not the real views.

## Method

- tmux 3.8 (`tmux -V` printed `tmux 3.8`). `man tmux` read for `new-pane`, `pane-border-*`, `display-popup` (text quoted where it matters). The tmux source was not at hand: what the source would say is marked *I infer*.
- **Private servers only**, as in `status-junctions.md` and `statusbar-no-row.md` § Method: the cockpit under test on `tmux -L cpd-research-34137 -f /dev/null` (`$T` below, every call under `timeout 20`) and a second server `tmux -L cpd-research-34137-outer` whose only window runs `tmux -L cpd-research-34137 attach -t cockpit` (`$O`): a real client, 140 x 41, whose screen `capture-pane -p -t o` shows (junction glyphs are drawn only on a client). Mouse bytes go to the outer window with `send-keys -H`:

      sgr(){ $O send-keys -t o -H $(printf "$1" | xxd -p | sed 's/../0x& /g'); sleep 0.35; }
      sgr '\033[<0;126;31M'    # left press at 0-based col 125, row 30
      sgr '\033[<32;126;28M'   # motion with the button held (a drag)
      sgr '\033[<0;126;28m'    # release

- **The real strings, not retyped.** `BORDER_FORMAT` and `STATUS_FORMAT` were produced by copying the lines of `core/src/host/tmux.rs` (consts `DANGO`, `MARK_*`, the macros `reset_glyph`…`fill`, `join`, `BORDER_FORMAT`, `STATUS_FORMAT`) and `core/src/host/tmux/pill.rs` with `sed` into a scratch file outside the repo, a stub of `pill::spec()` (the caps `\u{e0b6}` `\u{e0b4}` and the meanings of `pi/lib/pill-spec.json`), compiled with `rustc`, and printed (469 and 5267 bytes; origin/main `cd4bc290`).
- **Options** (the ones of `SERVER_OPTIONS` that touch layout): `mouse on`, `pane-border-status top`, `pane-border-lines heavy`, `pane-border-format` = `BORDER_FORMAT` (or a variant of Q2), `pane-border-style fg=colour7`, `pane-active-border-style fg=blue`, `remain-on-exit on`; session `status on`, `status-style default`, `status-format[0]` = `STATUS_FORMAT`.
- **Layout.** The steps of `build`: `new-session -x 140 -y 40` (tree, `%0`); `split-window -h -l 80%` (plan, `%1`); `split-window -h -l 25%` from the plan (machine, `%2`); `split-window -v -l 80%` from the plan (chat, `%3`); the `RESET_ORDER` resizes (`-x 20%` tree, `-x 20%` machine, `-y 20%` plan; the last probes used `-y 30%`, the file's `PLAN_HEIGHT`); `@cpd_view` set per pane. Then the Inbox: `split-window -d -t %2 -v -l 9` (`%4`, `@cpd_view inbox`). Panes run `sleep 3000`, a `SIGWINCH` logger (Q4) or a scripted stand-in:

      # inbox.sh N : the count as the pane title (OSC 2), then N rows
      printf '\033]2;%s\033\\' "$n"; printf '\033[H\033[2J'; echo; …rows…

- **Never touched: the socket `cpd` (the User's cockpit), `~/.cpd`, `cpd tui`, `cpd session run`.** Both servers killed and the scratch directory deleted at the end (last lines of this file).

## Q1. The junction

**Answer: Yes.** With the cockpit's real options and the Inbox pane directly under the machine pane, tmux draws `┣` (U+2523, heavy vertical and right) at the vertical border's column on the Inbox header row. Raw rows of the client (variant with the header branch of Q2; border column 111 because the tree is 28 wide and the plan 82):

```
row  0 (140 cells): ━━󰯲 ━󰰙 ━󰯵 ━━━━━━━━━━━━━━━━━━┳━━━━━━━━…━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━
row 33 (112 cells):                             ┃                                                                                  ┃
row 34 (140 cells):                             ┃                                                                                  ┣━━─Inbox──────────────2─
row 40 (140 cells): ━󰨝━󰨁━󰧭━━━━━━━━━━━━━━━━┻━━━━━━━━…━━━━━━┻━━━━━━━━━━━━━━━━━━━━━━━━━━
```

Read with a script over the captured row 34: `s[111]` is `┣`; `s[112]` and `s[113]` are `━` (the **two** line cells tmux draws before the format, in the border's colour); `s[114]` is the format's first cell (`━` in the heavy variant, `─` in the light one); the left cap is at `s[115]`, that is **column 3 of the pane** (the pane starts at column 112): the same place as today's flat header (`───` then the cap at column 3, commands.md §13.2). The row is **140 cells**: the format reaches the last cell, `s[139]`.

| Where | What tmux (or the status format) draws | Verified |
|---|---|---|
| Header row, border column (111) | `┣` | yes, row 34 |
| Header row, right edge (139) | no junction: the row's last cell is the format's last cell (`─` or `━`, `2` + cap in the count pill's case); there is no border to the right of a pane at the window's edge | yes |
| Top row (0) | `┳` at 111 (already so today) | yes |
| Status row (40) | `┻` at 111 and at 28 (cpd's `STATUS_FORMAT` loop, `#{P/i/:…}`, which finds the Inbox as the bottom-row pane of the right column: unchanged code) | yes |
| Machine pane's last row | nothing but its own cells: the Inbox header is the very next row (rows 33 and 34), so the **blank row above today's header** must be the machine view's own last row | yes (layout) |

Colours of the junction (SGR of `capture-pane -e`, the cases of the active pane: chat, machine, Inbox, plan, border colour 7 and blue as in `SERVER_OPTIONS`): with the **chat** or the **machine** pane active, the `┣` is `\e[34m` (blue) and the header's line cells are `\e[37m` (7); with the **Inbox** pane active, the `┣` and the line cells are blue and the pills stay 7; with the **plan** active, everything is 7 (the tree was not run). So the `┣` follows tmux's active-border rule, like every other junction, and the pills do not follow focus when set to a fixed `fg=colour7` (as the Inbox's pills are static grey today, §17).

## Q2. The header through `pane-border-format`

**Answer: Yes with conditions.** The branch is one more `#{?#{==:#{@cpd_view},inbox},…,…}` in the existing chain, between `widgets` and the fallback (`BORDER_FORMAT` has one branch per view; `widgets` is empty today). Two variants were run.

**Variant A, the heavy fill tmux draws by itself.** `━` + title pill + `#[align=right]` + count pill + `━`; the gap between the pills is filled by tmux with the border line, in the border's colour. Written readably (`\ue0b6`/`\ue0b4` the caps, as in `pill::pill`):

    #{?#{==:#{@cpd_view},inbox},
      ━#[fg=colour7]\ue0b6#[reverse]Inbox#[noreverse]\ue0b4
      #[align=right]
      #[fg=colour7]\ue0b6#[reverse]#{pane_title}#[noreverse]\ue0b4━ ,…}

The real row (140-cell client, pane 28 wide, 2 items):

```
row 34 (140 cells): …┃ ┣━━━Inbox━━━━━━━━━━━━━━2━        (the pane's cells: 112 to 139)
```

That is 2 tmux cells + `━` + 7 (the title pill: 5 letters + 2 caps) + 14 `━` + 3 (the count pill, `n` one digit) + 1 `━` = 28, exactly today's header's arithmetic (`3 + 7 + 16 + 3 + 1 = 30` at W = 30, §13.2). SGR of the row (Inbox not active): `\e[37m┣━━━\ue0b6\e[7mInbox\e[0m\e[37m\ue0b4━━━━━━━━━━━━━━\ue0b6\e[7m3\e[0m\e[37m\ue0b4━`.

**Variant B, the light muted fill of today's look.** `#[align=right]` fills with the border's style whatever `#[fg=…]` precedes it (run: a `#[fg=colour8]` before `#[align=right]` coloured only the cells after it: the trailing `━` came out `\e[90m`, the gap stayed `\e[37m`). So the fill is written explicitly with a computed run: `#{R:─,#{e|-:#{e|-:#{pane_width},13},#{n:@cpd_inbox_n}}}` (`13` = 1 for the first `─` + 7 title pill + 2 caps of the count pill + 1 for the last `─` + the 2 cells tmux draws; the count's own width is `#{n:…}`; checked by running it for the count `0`, `3`, `12`, `123`, always 140 cells):

```
row 34, count 3  : …┣━━─Inbox──────────────3─
row 34, count 12 : …┣━━─Inbox─────────────12─
row 34, count 123: …┣━━─Inbox────────────123─
```

SGR (Inbox not active): `\e[34m┣\e[37m━━\e[90m─\e[37m\ue0b6\e[7mInbox\e[0m\e[37m\ue0b4\e[90m────────────\e[37m\ue0b6\e[7m123\e[0m\e[37m\ue0b4\e[90m─`. So: the line cells are colour 8 as today, the pills are 7, **but the junction and the 2 cells next to it stay the border's heavy `━` in 7 or blue**: the header starts `┣━━─` instead of today's `───`. A first-attempt format that had two `─` at the start came out one cell too long (the last `─` was cut): the arithmetic above is the corrected one.

**How many cells tmux draws before the format: 2, and the format reaches the right edge.** Probe: `pane-border-format` = `#{R:X,200}` on the Inbox pane (28 wide):

```
row 34: …┃ ┣━━XXXXXXXXXXXXXXXXXXXXXXXXXX      (2 `━`, then 26 X: to column 139)
```

`#[align=right]` puts the last cell of its text on column 139 (`┣━━━━…━━R` with `R` at 139). This is the rule `BORDER_FORMAT`'s doc already states ("two border line characters before the format", `window_make_pane_status`), confirmed on the new junction.

**Heavy or light lines.** `pane-border-lines heavy` makes tmux's own cells and A's fill heavy (`━`). Every other pane header is heavy (`━━━Context━━━`, §6.1.1) so variant A matches them and differs from today's Inbox header, whose lines are light and muted; variant B keeps light and muted for the fill but cannot for the junction and the 2 next cells. The look changes in both: A: heavier and in the border's colour (blue when the Inbox is focused: the fill follows focus); B: a heavy stub `┣━━` then the light line.

**The count.** It changes with the store. Three carriers, all run:

| Carrier | Run | §6.2 / D4 |
|---|---|---|
| A pane option `@cpd_inbox_n`, `tmux set -p -t %4 @cpd_inbox_n 3` | the format read it, a change redrew the row at once (`3`, `12`, `123` above) | **A second exception** to "no tmux option carries state" (§6.1 and §6.2: "pane options identify panes, never state"; the one exception today is `@cpd_workers`/`@cpd_workers_short`, "a deliberate exception… display value only; nothing reads it back", reason: the status line cannot read the store). The justification would be the same: the border format cannot read the store, and the value is display only; but unlike `@cpd_workers` (the engine writes it) the **view** would write it, one `tmux set -p` per count change. A User decision. |
| The pane's **title**, set by the view with OSC 2 (`printf '\033]2;7\033\\'`), read as `#{pane_title}` | run: `display -p -t %4 '[#{pane_title}] [#{n:pane_title}]'` printed `[7] [1]`, the row showed `…Inbox──────────────7─`; `allow-set-title` is `on` (default) | **No option, no tmux command**: the pane's own title, a built-in property. It does not fit "identify, never state" either in the letter's spirit (tmux still holds a value), but there is no `@cpd_*` option: **I infer** it needs no exception. The view writes an escape sequence to its terminal outside its frame: `tui/src/terminal_writes.rs` forbids the print macros and handles on stdout, not a crossterm `SetTitle` (*unverified* how a view would write it through ratatui's backend). The title is also the `set-titles` source only if `set-titles-string` uses it: cpd's (`φ · #{@cpd_machine}`) does not. |
| No count in the border (count stays in the pane) | – | none: but then the header is a plain title pill and the count is drawn in the pane, which loses the header's look |

## Q3. Height

**Answer: Yes with conditions.** All probes on the layout of the Method (client 140 x 41, window 40 rows, so the column is 40 rows: the machine's header row, its rows, the Inbox's header row, its rows).

**Who resizes it.** Nothing in tmux grows a pane to its content. `resize-pane -t <pane> -y N` does it: it takes the rows from the neighbour above (verified below), so the pane at the bottom **grows upward** as asked. The caller is the Inbox view itself: its `Tmux` wrapper is already built in `widgets::show_with` (`Tmux::new(config.tmux.socket).with_runner(ctx.ports.runner)`); `reset_layout` and `cpd tui action` call tmux the same way (`tmux.run(&["resize-pane", …])`, `tui/src/cockpit.rs`). The view knows its pane id from `$TMUX_PANE` (*unverified* in a cpd view: the wrapper removes `TMUX_PANE` from the *child*'s environment, not the view's) and its needed height from its snapshot. The engine cannot: it does not know the client.

```
$ T resize-pane -t %4 -y 20 ; %2:1-18(18)  %4:20-39(20)      # 'id:top-bottom(height)'
$ T resize-pane -t %4 -y 9  ; %2:1-29(29)  %4:31-39(9)
$ T resize-pane -t %4 -y 5  ; %2:1-33(33)  %4:35-39(5)
$ T resize-pane -t %4 -y 1  ; %2:1-37(37)  %4:39-39(1)
$ T resize-pane -t %4 -y 0  ; rc=0, %4:39-39(1)               # silently clamped to 1
$ T resize-pane -t %4 -y 40 ; %2:1-1(1)    %4:3-39(37)        # the machine pane clamped to 1
```

**Minimum heights with a border-status row.** A pane is 1 row at least and its header row is *not* part of its height (`pane_top` is the row below the header). tmux lets the machine pane go to **1 row**: the column of 40 rows is 1 + 1 + 1 + 37. tmux has no notion of "3 rows for the machine", so **the cap is the view's**: the Inbox pane's rows ≤ `window_height − (1 + 3 + 1)` = 35 on this window (the machine pane's header, its 3 rows, the Inbox's header). The view reads `window_height` (and its own `pane_height`) in one `display -p`.

**Does the resize fight a drag?** Yes, and the drag loses. A left press on the Inbox header row (a `Border` location) followed by motions moves the border (`resize-pane -M`):

```
control, no resize between motions:        %4 height 9 → 12 → 13 → 14       (3 motions, 3 moves)
with `resize-pane -t %4 -y 5` after the first motion:
   motion to y=28   %2:1-26(26)  %4:28-39(12)
   resize -y 5      %2:1-33(33)  %4:35-39(5)
   motion to y=24   %4 unchanged (5)
   motion to y=20   %4 unchanged (5)
   release          %4 unchanged
```

After a resize made while the button is down, **the rest of that drag does nothing**: the border is no longer under the mouse's last position and tmux finds none (*I infer*: the drag looks the border up by the last position; the source was not read). The User must release and press again. A resize from the view is rare compared with a drag's length (an engine tick at most), but the interference is real. And whatever the User dragged, the view's next resize puts its height back: the view owns the height, a User drag lasts until the next count change (a decision, below).

**What reset-layout does with a third pane.** `reset_steps` resizes by view: tree `-x 20%`, widgets `-x 20%`, plan `-y 30%`. With the column split in two, `-x` on the machine pane sets the **column's** width (both panes), and nothing touches the Inbox's height:

```
before: tree 10, column 40:   %2 l=100 40x29   %4 l=100 40x9
reset:  -x 20% (tree), -x 20% (%2), -y 30% (plan)
after:  %0 28x39  %2 l=112 28x29  %4 l=112 28x9      # the column 28 wide, the Inbox still 9 rows
        %4 resize -x 30  →  %2 and %4 both 30 wide    # -x on either pane of the column is the same
```

So reset-layout needs no change for the Inbox's height (and the view re-applies it at its next `SIGWINCH`).

**A client that is short, or resized.** Outer window resized to 140 x 30, 20, 14, 12, 10, 8, 6, 41 (Inbox pane before: 9 rows):

```
outer height 30 → window 29 rows: %2 23 rows  %4 4 rows
outer height 20 → window 19:      %2 16       %4 1
outer height 14 → window 13:      %2 10       %4 1
 … 12, 10, 8, 6:                  %4 1 each, %2 shrinks to 2 at height 6
outer height 41 → window 40:      %2 20       %4 18      # it grew back, by proportion, not to 9
```

tmux **re-scales the column by proportion**: the Inbox pane shrank to 1 row on a short client (its content cut to the ▼ line) and grew to 18 on the way back. The view must therefore re-apply its height at every resize (it gets `SIGWINCH` anyway, Q4) and on a short client choose `min(needed, window_height − 5)`; at 20 rows or fewer a 5-row Inbox plus the machine's 3 rows and 2 headers fits (10 rows), so the cap, not tmux, decides. No error appears on a very short window (6 rows: the Inbox pane is 1 row).

**Zoom.** Zoom of another pane (`resize-pane -t %3 -Z`): the window shows `window_zoomed_flag` 1 and the chat 140 x 39. **`resize-pane -t %4 -y 12` while zoomed un-zooms the window** (flag 0, chat back to 80 x 19), even with the height it already had. That would undo the User's zoom each time the Inbox's count changes. A guard in **one** tmux command, so there is no race between a check and the resize, works:

```
$ T if -F '#{window_zoomed_flag}' '' 'resize-pane -t %4 -y 9'    # window zoomed: rc=0, flag stays 1, height stays 12
$ T if -F '#{window_zoomed_flag}' '' 'resize-pane -t %4 -y 9'    # not zoomed: flag 0, %4 height 9
```

(the fake tmux lists `if` among its commands, `core/src/fakes/tmux/mod.rs`). Zoom of the Inbox pane itself: it fills the window (140 x 39), the header spans 140 cells, `…Inbox──────…──7─`; the other panes keep their old reported size (no resize signals while zoomed: *I infer* from the sizes, `%0` stayed 28 x 39).

## Q4. Redraw and flicker

**Answer: one `SIGWINCH` to each of the two panes of the column, none to the rest.** Each pane (`%0` to `%4`) ran `winch.sh`: `trap 'echo "$id $(stty size)" >> log' WINCH; while :; do sleep 1000 & wait $!; done` (the trap runs at once because of `wait`), and the log was counted after each command.

```
A  resize-pane -t %4 -y 12 (9 → 12):  1 %2, 1 %4        sizes %2 26 30, %4 12 30
B  the same size again (12 → 12):     none
C  12 → 20:                           1 %2, 1 %4
D  20 → 9:                            1 %2, 1 %4
E  9 → 10:                            1 %2, 1 %4
F  the column's width 30 → 36:        1 %1, 1 %2, 1 %3, 1 %4    (the plan and chat shrink; the tree none)
G  a drag, 3 motions:                 3 %2, 3 %4    (%4 12 → 13 → 14 and %2 26 → 25 → 24: one pair per motion that moved the border)
```

The tree (`%0`), the plan (`%1`) and the chat (`%3`) saw **no** signal for a height change. The machine pane's view and the Inbox view redraw once each per resize (the ratatui views redraw on resize: `widgets/mod.rs` line 365, "a click, the wheel, a key or a resize"). Cost at the Inbox's rate: the count changes when an Inbox item opens or closes (the engine's E8 pass, every `tick_seconds`, default 5); a resize happens only when the **needed height** changes (the view skips a same-size call, and tmux sends nothing for one: B). I infer that this is a few redraws of two small panes per minute at most; unverified on the User's machine. No flicker was measured: a text capture cannot show it. Each `SIGWINCH` makes the view repaint its whole pane (a full frame, not a diff), which is the same as today when the column is resized.

## Q5. What changes

If `widgets` becomes two views (the machine, `widgets` kept for the machine so that old windows keep working, and a new `inbox`). By file and function (read on origin/main `cd4bc290`):

| File | Function / item | Change |
|---|---|---|
| `core/src/host/layout.rs` | `View` enum, `View::ALL` (5 → 6), `as_str`, `parse`; tests | add `Inbox` / `"inbox"` (about 10 lines + 20 of tests) |
| `core/src/host/tmux.rs` | `BORDER_FORMAT` and its doc comment | one more branch (Q2): about 15 lines; `core/src/host/tmux/pill_lines_tests.rs` (292 lines): the format's cases, about 40 new |
| `tui/src/lib.rs` | `View` (clap) and `run_with` dispatch | `View::Inbox => widgets::inbox_run(…)` (4 lines) |
| `tui/src/cockpit.rs` | module diagram (line 15), `LAYOUT: [View; 4]` → 5 | 5 lines |
| | `build` | `split_window(&widgets, Split::Below, …)` + `set_pane_view` (about 4 lines) |
| | `add_missing_panes` | the Inbox pane is a third case: missing Inbox with the machine present: `-v` below the machine; **missing machine with the Inbox present: not `-h -f`** (probed: with the Inbox there, `split-window -d -t %0 -h -f -l 20%` makes a **third column**, `%4` at l=89 and the new pane at l=112); it must be `-v -b` on the Inbox pane (probed: `split-window -d -t %4 -v -b -l 4` re-forms the column); neither: today's `-h -f` then the Inbox below: about 30 lines + 60 of tests |
| | `reset_layout`, `RESET_ORDER` | none for the Inbox's height (Q3: `-x` on the machine pane sizes the column); its tests (`tui_reset_layout.rs`, 46 lines) get a fifth pane |
| `tui/src/widgets/mod.rs` | `run`, `prepare`, `show_with`, `show` (lines 297 to 510, about 210), `menu_pane`, `Opener` | the one view splits: the machine view (host sampler, `column::draw` of the machine lines) and the Inbox view (store reader, snapshots, `Hit`, wheel, menu, `Opener`, plus the resize of Q3 and the count of Q2). About 150 lines move to a new module, about 60 are new (resize guard, height computation, count carrier) |
| `tui/src/widgets/column.rs` (154) | `draw` | no longer one column: the machine draws rows 0 to 2; the Inbox draws only its rows (▲ line, list, ▼ line); about 60 lines rewritten |
| `tui/src/widgets/inbox.rs` (741) | `Shape` (row 0 blank, row 1 header, row 2 ▲…), the hit table | the header and the blank row above it are gone from the pane (the header is the border, the blank row the machine view's last row): `Shape` loses 2 rows; `Hit::Header` is gone (a click on the header is tmux's, Q6): about 40 lines |
| `tui/src/section.rs`, `section_flush_tests.rs` | `flush_title` (36 to …), 85 lines of tests | **deleted** (`grep flush` finds one drawing caller, `inbox.rs:449`, and two colour tests, `pill_colours.rs:70` and `palette/colours.rs:104`, whose sample lines change): −120 lines, about 10 edited; §17 "Section titles" loses the flush form |
| `tui/src/widgets/{column_flat_tests,inbox_flat_tests,draw_tests,…}.rs` | the worked examples of §13.2 | rows 0 and 1 of every Inbox example change: about 100 lines edited |
| `cli/tests/tui/` | `tui_widgets*.rs` (3 files, 833 lines), `tui_inbox_models.rs` (148), `tui_idle.rs` (`VIEWS`, three `widgets` tests), `tui_read.rs`, `bare.rs`, `tui.rs`, `tui_reset_layout.rs` | the Inbox scenarios run `inbox` instead of `widgets`; the pane lists get a fifth pane: about 150 to 250 lines touched in 8 files |
| `cli/tests/session/host.rs`, `tooling/testkit_hosts.rs`, `tooling/preview_script.rs` | the pane builders and the `@cpd_view` lists | a fifth pane: about 20 lines |
| `core/src/fakes/tmux/`, `cli/src/bin/fake-tmux.rs` | `split-window` (flags `bdfhvPIZc:e:F:l:p:t:`: has `-v`, `-b`, `-f`), `resize-pane` (`DLRUZMTt:x:y:`), `if` | **almost nothing**: the fake does not model layout (`resize_pane` writes the pane's `width`/`height` columns, `select_layout` is "accepted, layouts are not modelled"). A test of the view's resize asserts the `Runner`'s calls (`if -F #{window_zoomed_flag} '' resize-pane …`), not a geometry: about 30 lines of new fake tests, no change to the fake. What cannot be tested with the fakes: the cap, the zoom guard and the drag interplay (all real-tmux behaviour, Q3), by the rule "no test runs a real tmux" they stay proved by this file only |
| docs | commands.md §13.1 (the cockpit's panes: four → five), §13.2 (the `widgets` row of the views table, line 3246; the "Inbox section" and "The Inbox's size and position" paragraphs; Reset-layout), §17 ("Section titles": the flush form), contract §6.1 (`BORDER_FORMAT`, the pane-title rules), §6.2 (pane table, the `@cpd_view` list, the exception for a count option), §6.1.1 | about 25 lines in 7 places |

**Estimate:** about 25 files; 900 to 1,300 lines touched: about 400 added, 300 moved or deleted, the rest edits of tests and docs. The riskiest part is not the code: it is the existing cockpits (below).

**Existing windows.** A cockpit window built before has no Inbox pane. `cpd tui` calls `add_missing_panes` on attach, which must add it (the case table above); a window whose `widgets` pane is the old combined view needs the old view process **respawned** as the machine-only view (`cpd tui --respawn`, `restart_views`, already restarts every pane with `@cpd_view`): until then the old pane would draw the Inbox too, under a new Inbox pane (two Inboxes). Order: respawn first, then split.

## Q6. Mouse and keyboard

**Wheel.** The wheel over the Inbox pane reaches the view as it does over the combined pane: the view uses mouse capture (`mouse_any_flag`, `alternate_on`), and tmux's default `WheelUpPane` binding passes the event to such an app (`if -F '#{||:#{alternate_on},#{pane_in_mode},#{mouse_any_flag}}' send-keys -M`, from `list-keys -T root`). *I infer* it is unchanged: not run with the real view.

**Left click on an item.** Unchanged: `MouseDown1Pane` → `select-pane -t = \; send-keys -M` (contract §6.1, tmux's own). The view gets the click with coordinates relative to its pane, which is now the Inbox pane: the code that today subtracts the machine rows (`Shape` offsets) loses that offset.

**Left click on the header or the border.** The header row is a `Border` location. Probe (a private root binding `MouseDown1Border` that logs, then the default `select-pane -t =`):

```
left click on the Inbox header, x=125:      active pane %3 → %4        (selected; the logging binding fired with pane=%4)
left click on the junction cell, x=111:     active pane → %4
right click on the header (MouseDown3Border): the binding fired (pane=%4); tmux's default has none: nothing happens
```

So a click on the header (or the `┣`) **focuses the Inbox pane** and a right click does nothing; today `Hit::Header` is "no action". Focusing the Inbox pane is the difference: the pill and the fill turn blue (Q1: the fill follows focus in variant A). `MouseDown1Border` is tmux's default (`select-pane -t =`); cpd binds only `MouseDown1Pane` and `MouseDown1Status` (`SERVER_OPTIONS`).

**The right-click menu — the real regression.** `menu_pane(pane)` is the pane less a blank column on each side; `Menu::rect` clamps the menu to it (`tui/src/menu.rs:92`, `.clamp(pane)`; the test at line 240 is named "cut to a pane smaller than the box"). The menu has 4 entries (`Snooze 15 min`, `Snooze 1 h`, `Snooze until tomorrow 09:00`, `Dismiss`), 5 with `Claim`, up to 8 for a `pr-ready` item (`Merge`/`Post`, `Go to Lead`, `Copy Link` + the four): **6 to 10 rows** with its frame. Today the pane is the whole column (about 40 rows). The Inbox pane is **3 rows** with one item and 5 with two. So the menu would be cut to the pane, and the entries below the cut cannot be clicked. Options (a User decision, none implemented): (1) a tmux `display-menu` (not clamped; each entry runs a command back into cpd, so the view's `Opener` actions — merge's spinner, the copy — need a `cpd tui action`-style entry point: new code); (2) a `display-popup` list (the view's other dialogs already are popups: `dialog::text`); (3) the view grows its own pane to the menu's height while the menu is open (a resize, with Q3's hazards, and a `SIGWINCH` to the machine pane per open and close); (4) accept a clipped menu above a minimum Inbox height. **I infer** (1) or (2) is right; not probed.

**Keyboard focus.** The Inbox view takes no keys today (§13.2, "no keys"); tmux's own pane navigation (the prefix and arrows) now has three panes to walk in the right column. The tree's keyboard cursor "goes on into the Inbox section's item rows" (§13.2, "Cursor"): that is the tree's own rows and is unchanged. The Inbox pane being focusable and blue when focused is new (above).

## Q7. Alternatives

**(a) Keep one pane and accept the half-cell gap.** Works today (merged). The docs would then say what is true: the header is drawn from the pane's column 0, one half cell right of the `┃` that tmux draws at the centre of its own border cell, and "reads as `┠`" goes (commands.md §13.2 "The `─` at column 0 touches tmux's `┃` border cell… so the join reads as `┠`"). No code, no new moving part. A glyph from column 0 cannot fill the half cell: no cell of the pane is left of it.

**(b) One pane, the machine lines and the Inbox header drawn by tmux in the `widgets` pane's own top border.** **Not possible mid-column.** `pane-border-format` draws only the status row of a pane's own edge: `pane-border-status` is `off`, `top`, `bottom`, `top-floating` or `bottom-floating` (`man tmux`, quoted in Method) and the row is the pane's first or last, not a row inside it. The Inbox header sits mid-column (under the machine rows, above the list), so a single pane has no border row there. With `bottom`, the format would be under the last row, the opposite of a header. No way found.

**(c) Other tmux 3.8 features.**

| Feature | Probe | Result |
|---|---|---|
| Floating pane `new-pane` (3.8) | `new-pane -d -x 29 -y 8 -X 111 -Y 31` over the right column's bottom | draws its own box, corners `┏ ┓ ┗ ┛`, inset: no `┣`, no join with the tiled `┃`; the float covers the pane below |
| Floating pane with `-B none` ("no border for floating panes", `pane-border-lines`) | `new-pane -B none` at the Inbox's rectangle | no border and no header drawn; it only hides the tiled Inbox's header row; the tiled junction stays where the tiled layout makes it. Nothing joins. |
| `pane-border-status top-floating` | read in `man tmux`: "status lines are shown only on floating panes" | it would turn the headers of all tiled panes off (the cockpit's titles): not run, ruled out by the man text |
| `display-popup` | `display-popup -c <client> -b heavy -x 109 -y 29 -w 31 -h 11` | draws its own frame (`┗━━…┛` on its bottom row) over the pane: no join |
| `pane-border-indicators` (`off`, `colour`, `arrows`, `both`) | read | colours half of the border or draws arrows, in windows with exactly two panes: not a join |
| `pane-border-lines` (`single`, `double`, `heavy`, `simple`, `number`, `spaces`, `none`) | read | changes the glyph set of tmux's own borders; none of them draws a line inside a pane |

**Ruled out**: no feature lets a pane's left border join a line drawn inside the pane. The only junction tmux draws is between two **tiled** panes' borders, hence the Inbox as a tiled pane.

## Cost

Q5's table is the cost: **about 25 files, 900 to 1,300 lines touched** (about 400 added, 300 moved or deleted, the rest edits), plus seven docs sections. New code is small (the resize call, the height rule, the count carrier, a format branch, an `add_missing_panes` case); most of the work is splitting the view and its tests and re-writing the §13.2 worked examples. New runtime: one more process (the Inbox view), two `SIGWINCH` per height change (Q4), one tmux call per height change (`if -F … resize-pane`). Deleted: `flush_title` and its tests. Fakes: unchanged; the view's resize is tested through the `Runner` (Q5). What no test can prove with the fakes: the cap, the zoom guard, the drag interplay: only this file's probes do.

## Risks

1. **The right-click menu is clipped** in a pane of 3 to 5 rows (Q6). Needs a design before building.
2. **`resize-pane` un-zooms** the window (Q3): one `if -F '#{window_zoomed_flag}'` guard in the same tmux command, and a test that asserts the guarded command.
3. **A resize kills a drag in progress** (Q3) and the view's next resize overrides what the User dragged: the height is the view's, not the User's (a decision).
4. **tmux re-scales the column on a client resize** (Q3): the view must re-apply its height on every resize; on a short client the cap, not tmux, decides.
5. **A second exception to §6.2** if the count is a pane option (Q2); the OSC-title carrier avoids the option but makes the view write an escape sequence outside its frame (the `terminal_writes` guard, Q2).
6. **Existing cockpits**: the old combined `widgets` pane must be respawned as the machine view before the Inbox pane is added, or two Inboxes show; `add_missing_panes`'s `-h -f` for a missing machine pane makes a third column when the Inbox exists (probed, Q5).
7. **The header look changes** (heavy and in the border's colour, or a heavy stub then light): a User decision (Q2).
8. **The fakes cannot prove the layout rules** (cap, zoom, drag): the risk is in what real tmux does, which only a probe like this one shows; "no test runs a real tmux".
9. **tmux version**: nothing here needs a version newer than the `#{P/i/:}` loop the StatusBar already needs (3.6); older tmux *unverified* (only 3.8 installed). `new-pane` and floats are 3.8-only and not used.
10. **Wide glyphs**: the caps are one cell each in these captures (every row is exactly 140 cells); a terminal font that draws them two cells wide would break the count arithmetic as it would for the pane titles (*unverified*: the User's terminal was not tried).

## Recommendation and open decisions

**Recommendation: build it, if the User wants the join** (Q1: tmux draws `┣` exactly where the header begins, in the cockpit's real options; Q7: nothing else can). It is the only way to get the join, every hazard has a verified, cheap mitigation except the menu (decision 1), and the cost (about 25 files) is mostly mechanical. **If the join is not worth about 1,000 lines and a resized pane, accept the gap (a)** and change the docs. Both are the User's to choose; the research does not decide.

Shape of the Plan if built, two PRs:

1. **Docs** (decided work after the answers below): commands.md §13.1/§13.2/§17, contract §6.1/§6.2/§6.1.1: the five-pane cockpit, the Inbox header as the pane's border, the height rules, the cap and the guard. One Job.
2. **Implementation**, one PR, three Jobs in dependency order: (i) `core`: `View::Inbox`, the `BORDER_FORMAT` branch and its tests; (ii) `tui`: the cockpit's `build` / `add_missing_panes` / respawn order, the view split (`inbox` view, the machine view), the resize with the zoom guard and the cap, deletion of `flush_title`; (iii) tests and fakes of the cli suites (they change with (ii), so (ii) and (iii) are one Job if the Gate needs them together).

**Open decisions for the User:**

1. **The right-click menu in a 3 to 5 row pane** (Q6): `display-menu`, a popup list, a pane that grows while the menu is open, or a clipped menu.
2. **The count's carrier** (Q2): a pane option `@cpd_inbox_n` (a second exception to §6.2's "no state in options", justified like `@cpd_workers`), the pane's title (OSC 2, no option, the view writes an escape), or no count in the border.
3. **The header's look** (Q2): variant A, heavy and in the border's colour (blue when focused), like every other pane header; or variant B, today's light muted `─` after a heavy `┣━━` stub.
4. **Who owns the height** (Q3): the view's always (a drag lasts until the next count change), or the view resizes only when the needed height changes, so a drag survives a refresh; and whether a resize while the window is zoomed is skipped (the guard) or never (zoom wins).
5. **The old windows** (Q5): whether `cpd tui` may respawn the old combined `widgets` pane on attach (it already does for `--respawn`) or the User restarts the cockpit once.
6. **Whether the join is worth it at all** (Q7 (a)): the half cell is cosmetic.

## Cleanup

The run's cleanup, and its output (below).
