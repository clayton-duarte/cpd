# Status-line junctions (`┸`) at the pane borders

Job `research-docs` of Plan `inputbox` (User, hs-d575 (b), 2026-10-22: "IDK if it's possible"). The ask: the tmux status line shows a `┸` at each column where a vertical pane border meets it, follows the border when it is dragged, and drops it where a pill or button sits on that column. This file changes no code.

## Answer

| | Question | Answer |
|---|---|---|
| Q1 | Can `status-format[0]` draw a `┸` at a border's column, with no shell command? | **Yes, with conditions.** One `#{P/i/:…}` loop over the window's panes, each bottom-row pane emitting its run of `─` and then the `┸`. Condition: the loop must be sorted by `i` (pane index), which tmux ≥ 3.6 has (§ Q1); tmux's default order is creation order and is not left to right. |
| Q2 | Does it follow a drag, a `resize-pane`, a client resize, a layout change? | **Yes, at once, with no hook and no `refresh-client`.** tmux redraws the status row on every one of them, per motion event during a drag. No hook is needed. |
| Q3 | Can the format drop the junction under a pill or button? | **Yes**, from the variables that size the fill today (`#{client_width}`, `@cpd_preview`, `@cpd_version`, `@cpd_workers_width`, `@cpd_workers_short_width`); no precomputed `@cpd_…` per border. A junction does not move any click range. |
| Q4 | Best design? | **A** (one format, no process). B (cpd computes the string from a hook) works but lags a motion event behind a drag and costs a process per event. |
| Q6 | Recommendation | **Do it, with design A**, size ≈ 40 lines in `core/src/host/tmux.rs`, a longer format (1.4 KB → 4.3 KB). Open decisions for the User at the end. |

What it looks like (tmux 3.8, the cockpit's real layout and format, a 140-column client; rows are the last two pane rows and the status row, captured with `capture-pane -p`; the tree is 34 wide, the widgets 33):

```
                                  ┃                                                                       ┃
                                  ┃                                                                       ┃
─󰨝─󰨁─󰧭──────────────────────┸───────────────────────────────────────────────────────────────────────┸───────────workers 1/4─0.9.1─
heavy [34, 106]  junction [34, 106]
```

The `┃` of the row above and the `┸` are at the same two columns (34 and 106). The buttons' caps (U+E0B6, U+E0B4) do not show in this text but are in the row (each is one cell: the row is exactly the client's width, 140).

At **100 columns** (the cockpit's default test size) the second border (column 79) falls under the workers pill (cols 78 to 99), so only the first junction is drawn there: that is the occlusion rule at work, not a failure (§ Q3).

## Method

- tmux 3.8 (`tmux -V` → `tmux 3.8`). Its source, tag 3.8, read at `/tmp/cpd-pi-pin/tmux38/src/tmux-3.8/` (`format.c`, `sort.c`, `CHANGES`); `man tmux`.
- **Private servers only.** The cockpit under test on `tmux -L cpd-research-$$ -f /dev/null`; a second, `tmux -L cpd-research-$$-outer -f /dev/null`, whose only window runs `tmux -L cpd-research-$$ attach -t cockpit`: a real client, whose screen I read with `capture-pane -p` on the outer server and into which I send SGR mouse bytes with `send-keys -H` (the method of `statusbar-no-row.md` § Method):

      sgr(){ $O send-keys -t o -H $(printf "$1" | xxd -p | sed 's/../0x& /g'); }
      sgr '\033[<0;31;11M'    # left press at 0-based col 30, row 10
      sgr '\033[<32;36;11M'   # motion with the button held (a drag)
      sgr '\033[<0;36;11m'    # release

  A third private server (`…-outer2`) was used once, for the 80-column second client of Q5.
- **How the layout was reproduced.** The strings were not retyped: `STATUS_FORMAT` and `BORDER_FORMAT` were produced by copying the lines of `core/src/host/tmux.rs` (the macros `reset_glyph`…`join` and `title_pill`, then the two `pub const`s) with `sed` into a scratch file outside the repo, compiling that with `rustc` and printing the constants (`/tmp/rd/x.rs`, `/tmp/rd/b.rs`; the 1435-byte `STATUS_FORMAT` is what the cockpit sets). Server options: `mouse on`, `pane-border-status top`, `pane-border-lines heavy`, `pane-border-format` (`BORDER_FORMAT`), `pane-border-style fg=colour7`, `pane-active-border-style fg=blue` (the others of `SERVER_OPTIONS` do not touch layout). Panes by the steps of `tui/src/cockpit.rs` `build`: `new-session` (tree), `split-window -h -l 80%` (plan), `split-window -h -l 25%` from the plan (widgets), `split-window -v -l 80%` from the plan (chat), then the `RESET_ORDER` resizes (`-x 20%` tree, `-x 20%` widgets, `-y 20%` plan); `@cpd_view` set per pane; each pane runs `sleep`. Session `status on`, `status-style default`, `status-format[0]` = `STATUS_FORMAT`. The client's window is 100×30 unless a line says otherwise (status row = row 29, 29 pane rows).
- Standing values for the options the engine feeds (the engine's real widths are not known to me): `@cpd_workers` = `workers 1/4`, `@cpd_workers_width` 13 (pill: text + 2 caps), `@cpd_workers_short` = `1/4`, `@cpd_workers_short_width` 5, `@cpd_version` = `0.9.1`. The right group is then 22 cells wide in the long form (13 + join + 7 + closing `─`), 14 in the short.
- `status-interval 3600` for every tracking test, so that a redraw seen is not the interval's.
- **Never touched: the socket `cpd` (the User's cockpit), `~/.cpd`, `cpd tui`, `cpd session run`.** Every server above was killed with `kill-server` and the scratch directory and sockets deleted before I submitted (see § Cleanup).
- Everything below is shown by a run unless it says *unverified* or *I infer*.

### The format that worked (design A, generated for the experiment)

`/tmp/rd/gen3.py` (scratch, deleted) took the real `STATUS_FORMAT`, kept all of it and replaced the one `#{R:─,CNT}` fill by the loop below. Written readably (the real string has every name expanded inline, 4331 bytes; `CNT` is the existing fill count, copied from the format; `F0` = `12 + preview_width`; `F1` = `F0 + CNT`, the first column of the right group):

    #{?window_zoomed_flag,
        #{R:─,CNT},                                    # zoomed: today's fill, no junctions
        #{P/i/:                                        # every pane of the client's window, in index order
          #{?pane_at_bottom,                           # only panes whose bottom row is above the status row
            #{R:─,  max(0, min(pane_right, F1-1) - max(pane_left, F0) + 1) }   # the pane's columns that lie in the fill
            #{?#{&&: #{!:#{pane_at_right}} ,
                     #{&&: pane_right+1 >= F0 , pane_right+1 < F1 }},  ┸ ,},   # the border's column, if in the fill
          }}}

`max`/`min` are `#{?#{e|>:a,b},a,b}` (the format language has no max/min).

## Q1. Drawing

**Q1.1 `#{P:}` inside `status-format` sees the client's current window, and its variables resolve there — verified.**

    $ T display -p -t cockpit '#{P:[#{pane_id} l=#{pane_left} r=#{pane_right} atl=#{pane_at_left} atb=#{pane_at_bottom}]}'
    [%0 l=0 r=19 atl=1 atb=1][%1 l=21 r=78 atl=0 atb=0][%2 l=80 r=99 atl=0 atb=1][%3 l=21 r=78 atl=0 atb=1]

and in the status row itself (`status-format[0]` = `#{P:[#{pane_id} l=#{pane_left} r=#{pane_right}]}`, read from the outer client):

    [%0 l=0 r=19][%1 l=21 r=78][%2 l=80 r=99][%3 l=21 r=78]

(`#{window_width} #{client_width} #{window_panes}` → `100 100 4`.) The vertical border to the right of a pane that is not at the right edge is at column `pane_right + 1`: here 20 and 79, the `┃` columns (checked against the captured pane rows, below).

**Q1.2 The tools for a column — which work (all verified):**

| Tool | Result |
|---|---|
| `#{R:─,n}` with `n` a nested format | works: `#{R:─,#{e|-:20,15}}` → `─────` |
| `#{p-N:…}` (pad right to N) with N nested | works **only with a leading `-`**: `#{p-#{e|-:20,1}:}\|` → 19 spaces then `\|`. |
| `#{p#{…}:…}`, `#{=#{…}:…}` (positive nested width) | **does not work**: `#{p#{e|-:20,1}:}\|` → `\|` (empty). tmux reads a punctuation character after the modifier letter as the *delimiter* of the argument (`format.c`, `format_build_modifiers`: `if (!ispunct(cp[1]) \|\| cp[1] == '-')` is the plain-argument path), so a width that starts with `#{` is taken as `s/…/…/`-style delimiters. |
| `#{=n:…}` truncation | works with a literal `n`; a nested `n` has the same `#` problem. Not needed. |
| `#[align=…]` | three zones (left, centre, right) and no column: it cannot put a glyph at column N. It is what the cockpit already uses for the right group. |
| `#{P:…}` | works, with the *order* problem below. |

So **`#{R:─,n}` with a computed `n` is the one tool for the fill, and it is enough**: the fill is written as pieces, each a run of `─` of the right length, with a `┸` between two runs. No shell command (`#(…)`), no `#{p…}`.

**Q1.3 The loop order — the one real condition.** `#{P:}` visits panes in *creation* order in 3.8, and that is not left to right in the cockpit (widgets, created before the chat, are at the right):

    $ T display -p -t cockpit '#{P:#{pane_id}#{?pane_at_bottom,b,-}#{?pane_at_right,r,-} }'
    %0b- %1-- %2br %3b-        # creation: tree, plan, widgets, chat
    $ … '#{P/i/:…}'
    %0b- %1-- %3b- %2br        # index order: tree, plan, chat, widgets = left to right

The `/i/` argument (sort by index) is a tmux 3.6 feature (`CHANGES` "FROM 3.5a TO 3.6": "Add sorting to W, P, L loop operators"; `format.c` `case 'P'`: `i` → `SORT_INDEX`, `z` → `SORT_Z`, `r` reverses). Whether `#{P/i/:…}` is *accepted* (ignored) by an older tmux is **unverified** (only 3.8 is installed); on 3.4/3.5 the loop has no sort option and I read its order as the pane list's, which is the index order: **unverified**. The cockpit's other options already assume newer tmux (`popup-style`, `extended-keys-format` use `-q`); whether 3.8 is the minimum is the User's to say (Q6).

**Is index order always left to right?** In every layout change I tried, tmux keeps the pane list in the layout's order, so the bottom-row panes come out sorted by `pane_left` — verified for: the build (above), `swap-pane -s %0 -t %4`, `rotate-window`, `select-layout even-horizontal`, a vertical `split-window` that adds a pane on the bottom row, `join-pane -d -b -f -h` (twice), killing the widgets pane and re-adding it with the flags of `add_missing_panes` (`split-window -d -t <first pane> -h -f -l 20%`): in each, `#{P/i/:#{?pane_at_bottom,#{pane_left} ,}}` printed ascending columns (e.g. `0 71 89 106`), and the junctions drawn were at `pane_right + 1` of every bottom pane (`[26, 111]`, `[35, 70, 105]`). I found no counter-example, but I did not prove there is none: **I infer** it from how tmux inserts a pane into the list at its layout position (source not read: unverified). A wrong order would show as a misplaced run of `─`, never as a crash. A guard exists if the User wants a proof: none in the format (it cannot sort by geometry: the sort orders are `creation`, `index`, `z`, `size`, `name`, `activity`, `time`: `sort.c`, `format.c:6235-6250`).

**Q1.4 A working format on the real layout — verified.** The generated format (above) on the 100-column cockpit, `@cpd_preview` unset:

    ─󰨝─󰨁─󰧭────────┸─────────────────────────────────────────────────────────workers 1/4─0.9.1─
    len 100   junctions at [20]

and at 140 columns, panes `%0 l=0 r=33`, `%1/%3 l=35 r=105`, `%2 l=107 r=139`, the row printed at the top of this file: `junctions at [34, 106]` and the pane rows above have `┃` at `[34, 106]`. The row is always exactly `client_width` cells wide (`len 100`, `len 140`, `len 60`, `len 34`), so the fill count stays exact: the sum of the runs plus the junctions equals `CNT`.

**The junction glyph.** `┸` (U+2538, up heavy, horizontal light) is the one that matches: heavy `┃` above, light `─` below, so the heavy border runs into the light line. `┻` (U+253B, all heavy) would draw one heavy bar across the light line (the line looks thick for one cell on each side), and `┴` (U+2534, all light) would thin the heavy border's last half cell. The captures with both (the text is the same shape, the weight is the terminal's; I cannot see weight in a `capture-pane`):

    glyph ┻:   ┃ … / ─󰨝─󰨁─󰧭────…┻─────…workers 1/4─0.9.1─
    glyph ┴:   ┃ … / ─󰨝─󰨁─󰧭────…┴─────…workers 1/4─0.9.1─

That the weights read as described is **unverified by sight** (a text capture has no weight); it is the Unicode names' meaning. Colour: the status line is colour 8 and the border is colour 7 (or blue when the pane is focused): a `┸` in colour 8 meets a `┃` in 7 or blue. Matching the focused border's colour per junction is possible in the format (`#{?pane_active,#[fg=blue],#[fg=colour7]}` inside the loop) but the active pane is not always the one to the left of the border; a User decision (Q6).

## Q2. Tracking

**Q2.1 Redraw: the status row re-renders at once, with no hook and no `refresh-client`; `status-interval` was 3600 and no hook was set in these runs.**

    $ T resize-pane -t %0 -x 25      →  junctions at [25]      # immediately
    drag (SGR press at the border, motion to col 40), no hook, interval 3600:
                                      →  junctions at [40]
    $ T split-window -d -t %1 -h …   →  status row recomputed (bottom row panes unchanged: same junctions)

A mouse drag, motion by motion (`resize-pane -M` runs per motion; the junction followed each one):

    after motion to col 35: junctions at [35]
    after motion to col 40: junctions at [40]
    after motion to col 45: junctions at [45]     # 3 motions, 3 correct rows, none missing

A client resize (outer `resize-window -x 140`, the client is 140 wide, the window follows with `window-size latest`) re-expanded the row: `junctions at [34, 106]`, `len 140`. A second client of another width: Q5.

**Flicker / lag.** The captures after each motion, taken 0.3 s after the event, were always correct. I did not film the terminal: a text capture shows no flicker, so **flicker is unverified** — but there is nothing to flicker: tmux redraws the status row itself in the same pass as the panes, the status row is part of the same redraw as the layout change (unverified beyond the captures).

**Q2.2 Which hooks fire (one run each; each hook logged `echo <name> >> log` through `run-shell -b`):**

| Event | `after-resize-pane` | `client-resized` | `window-layout-changed` | `after-select-layout` | `window-resized` | others |
|---|---|---|---|---|---|---|
| `resize-pane -t %0 -x 30` | 1 | – | 2 | – | – | |
| mouse drag, per event | **1 per drag** (on the first motion) | – | **1 per motion** (3 motions: 3) | – | – | the press and the release: none |
| outer client `resize-window -x 120` | – | 1 | 1 | – | 1 | |
| `select-layout even-horizontal` | – | – | 2 | 1 | – | |
| `split-window -d` (a pane opens) | – | – | 1 | – | – | `after-split-window` 1 |
| `kill-pane` (a pane closes) | – | – | 1 | – | – | `after-kill-pane` 1 |
| inner `resize-window -x 110` | – | – | 1 | – | 1 | `after-resize-window` 1 |
| `select-pane` | – | – | – | – | – | (no layout change) |

- Of the User's three hooks: **`window-layout-changed` is the only one that fires for every geometry change** (resize-pane, drag, client resize, layout, open, close). `after-resize-pane` fires once per drag (not per motion) and not for a client resize or a layout change; `client-resized` only for the client.
- For design A none of them is needed. For design B, `window-layout-changed` is the hook, and it fires **per mouse-motion event** during a drag.

## Q3. Occlusion

**Q3.1 The format decides from the variables that size the fill: yes — verified, each edge.** The fill is the span `[F0, F1)`: `F0 = 12 + preview width` (left group + preview pill), `F1 = F0 + CNT` (`CNT` is the fill count the format computes today from `#{client_width}`, the preview, `@cpd_version`, and whichever of `@cpd_workers_width` / `@cpd_workers_short_width` the long-form test picks). A junction at column `c = pane_right + 1` is drawn iff `F0 <= c < F1` and the pane is not at the right edge. No new `@cpd_…` option.

Runs (tree width = the border's column; the table is the junction row's `junctions at`):

| Case | Setup | Result |
|---|---|---|
| border under the left group's buttons | tree width 3, 5, 6, 9, 10, 11 | no junction (columns 3 to 11 are the buttons) |
| first cell after the left group | tree width 12 → border col 12 | `─󰨝─󰨁─󰧭┸────…` drawn |
| after it | width 13, 14 | `┸` at 13, 14 |
| preview pill (`@cpd_preview` = `abc`, pill 15 cells, cols 12 to 26) | tree width 14, 15, 26 | none |
| | tree width 27 (the cell after the pill) | `─preview: abc┸───…` drawn |
| right group, long form (cols 78 to 99) | widgets width 23 → border col 76 | drawn `…┸─workers…` |
| | width 22 → col 77 (the last cell before the pill's cap) | drawn `…┸workers 1/4─0.9.1─` |
| | width 21 → col 78, width 20 → col 79 | none |
| right group, short form (34-column client: `1/4`, right group 14 cells, cols 20 to 33) | tree width 19 → col 19 | drawn `…┸1/4─0.9.1─` |
| | tree width 20 → col 20 | none |
| border at column 1 (tree width 1) | | none (it is under the first cap) |
| border at column 0 or the last column | **cannot exist**: a pane is at least 1 cell wide, so the first border is at col ≥ 1 and the last at ≤ width − 2 | n/a |

The short-form test is the format's own, so the junction followed the form that was drawn: at 34 columns the long form would have started the right group at col 12, and a border at col 19 was still drawn (correct, since the short form's group starts at col 20).

**Two borders closer than a pill is wide:** inside the right group the junctions that fall under it are all dropped (with the 22-cell group at cols 78–99 there were borders at 76, 77 both drawn and 78, 79 dropped). The fill's `R:` runs are computed per pane with `max(0, …)`, so a pane that lies wholly under a pill contributes 0 `─`, not a negative number. Two borders closer together than a pill is wide, in the fill: not run (unverified); each is drawn by its own pane's piece, so the arithmetic gives `┸─┸` for a 1-cell pane.

**Q3.2 Click ranges: a junction changes none — verified.** `bind -T root MouseDown1Status run-shell -b "echo x=#{mouse_x} range=#{mouse_status_range} >> log"`; one click per column, 0.55 s apart (a faster second click is taken as a double click and fired nothing on `MouseDown1Status` in my first run, which dropped some columns):

    tree w=40 (junction at 40, far away):  x=1 reset  x=2 reset  x=3 reset  x=5 legend  x=9 logs  x=10 logs  x=11 logs
    tree w=12 (junction at col 12):        x=1 reset  x=2 reset  x=3 reset  x=5 legend  x=9 logs  x=10 logs  x=11 logs
    tree w=13 (junction at col 13):        x=1 reset  x=2 reset  x=3 reset  x=5 legend  x=9 logs  x=10 logs  x=11 logs

Identical in the three. (The clicks at 0, 4, 8, 12, 13, 14 and 20 fire nothing in any of them: the joins and the fill are outside the ranges, and so is the `┸`.) The junction is plain text outside every `#[range]` because the loop is after `#[norange]` of the last button, and the cell after the logs button is not owned (`x=12` fired nothing).

## Q4. Alternatives

**(A) One `status-format[0]` that splits the fill at the junctions with `#{P/i/:}` and arithmetic — possible, verified (Q1.4, Q2, Q3).** Costs: the format is **4331 bytes against 1435** (the `F0`/`F1` expressions are repeated, because the format language has no variables), and it needs tmux ≥ 3.6 for the sort (Q1.3). Redraw cost, one-shot real runs: 200 expansions of the format with `display -p -t cockpit -F` took 0.90 s for the 1435-byte one, 1.24 s for the 4331-byte one, 0.58 s for a trivial `x` (the process start dominates): about 1.7 ms more per expansion than today's; **unverified** as a cost on the User's machine. No length limit was met at 4331 bytes (a limit at larger sizes: unverified).

**(B) cpd computes the string / an option from a hook through `run-shell` — possible, but worse.** A hook `window-layout-changed → run-shell -b hook.sh`, where `hook.sh` ran `tmux list-panes -F '…'` and `tmux set -g @cpd_j "<cols>"` (9 ms for the script, `time ./hook.sh` → `real 0m0.009s`), with a test format `opt=[#{@cpd_j}] live=[#{P/i/:…}]` showing both. Results:

    resize-pane -x 30 :   opt=[30  79  ] live=[30 79 ]      # settled after the hook
    drag, read at once :  opt=[30  79  ] live=[35 79 ]      # the option is one motion behind
                          opt=[35  79  ] live=[35 79 ]      # 0.3 s later it caught up

(`set -g @…` redraws the status row by itself; no `refresh-client` was needed.) So B lags a drag by the hook's run time, and a process starts **per motion event** (`window-layout-changed` fires once per motion: Q2.2). And B still needs the format to *use* the list: the format language cannot split a string `"20 79"` into numbers, so B must write the whole `status-format[0]` (or the pieces) from the script, i.e. `cpd` has to rebuild `STATUS_FORMAT` in Rust with the layout baked in at every hook, in a second place from the pills' arithmetic. A needs none of that.

**(C) Other ways, each run or read:**

- **`pane-border-format` of the bottom panes**: cannot reach the status row; it draws in the border row *above each pane* (`pane-border-status top`), i.e. at the top. Not possible.
- **`pane-border-status bottom`** (a window option): tmux then draws the junction itself, a `┻` in the bottom border row (`━━━Context━━━━…┻━━━…` above the status row, run), but it moves every pane's header to the bottom and costs a row, against the cockpit's design (`statusbar-no-row.md` Q1b: one window option for every pane) — not a way to put `┸` on the status row.
- **`pane-border-lines` variants** (`single`, `double`, `heavy`, `simple`, `number`): they change the glyphs tmux draws for the borders between panes; none touches the status line. Not a way.
- **A status line of two rows**: not a way to join them (the second row is another row of text); and it costs a row. **A one-row pane for the bar** is the pre-hs-d479 design: tmux draws the junction (`┻`) itself, at the price of the bar's second row, i.e. exactly the row the User gained by moving to the status line.

## Q5. Risks

- **Wide / private-use glyphs.** The caps (U+E0B6, U+E0B4) and the Material glyphs (U+F0A1D, U+F0A01, U+F09ED) are one cell each here: every captured status row had exactly `client_width` codepoints and the junctions were at the same columns as the `┃` above (34 and 106; 20 and 40), with the buttons and pills to their left. A terminal font that draws those glyphs two cells wide would shift the junctions (unverified; the User's terminal was not tried): the fill arithmetic has the same exposure today.
- **Several clients with different widths.** `#{client_width}` is per client, the pane columns are per window. Run: a 120-column and an 80-column client on one session (the window was 110 wide, set by an earlier manual `resize-window`, so it did not follow either):

      client 1 (120 wide): junctions at [22, 44, 67]
      client 2 (80 wide) : junctions at [22, 44]          # col 67 is under the right group of the 80-wide row: dropped

  The row each client draws is its own width and the junctions follow its own `F1`; the borders are at their window columns in both (22, 44 are the same columns in both clients). A client wider than the window shows the row longer than the panes (the same as the fill and the pills today); a client narrower than the window sees a clipped window and the format drops what it cannot show (col 67 above). With `window-size latest` the window follows the most recently active client, so the mismatch is a transient state. No inconsistency in the junction/border columns was seen.
- **Zoom.** Verified problem: a zoomed window still reports the unzoomed geometry for the hidden panes and the full width for the zoomed one (`%0 l=0 r=19`, `%3 l=0 r=99`, `%2 l=80 r=99`, `window_zoomed_flag=1`), so without a guard the loop would draw a false `┸` at col 20 where no border is on screen, and the runs would overlap. With the guard `#{?window_zoomed_flag,#{R:─,CNT},…}` (included in the format above) the zoomed row is the plain line (`junctions at []`) and the unzoomed row returns to `[40]`. Run, both ways.
- **`pane-border-status top` and a matching `┰`.** tmux already draws it: the top row of the capture is `━━━Context━━━━…┳━━━Plan━━━…` with `┳` at col 34, the same column as the `┸` below; nothing to add (and it is not ours to draw).
- **Colour.** Grey `┸` under a grey (7) or blue (focused) `┃`: cosmetic (Q1.4).
- **Cost of a longer format.** 4331 bytes against 1435; expansion about 1.7 ms more per status redraw in a one-shot measure; the status row is redrawn on every layout change and, during a drag, per motion (Q2). No size limit hit. The unit test that finds the version pill in `STATUS_FORMAT` (`core/src/host/tmux.rs`, `STATUS_FORMAT.find(pill)`) is a string search and is not affected: **unverified** until it is run with the new format.
- **Index order** (Q1.3) is the one assumption not proven.

## Q6. Recommendation

**Do it, with design A: one `STATUS_FORMAT` that splits the fill with a `#{P/i/:}` loop.** It is feasible, it follows a drag live with no hook and no process, and it drops the junction under every pill and button from the variables the fill already uses. It is clean in the sense of the Job's test: a single format string, no state outside it, no new option for `cpd` to feed.

- **Files that would change:** `core/src/host/tmux.rs` only: the `STATUS_FORMAT` constant and its doc comment (the loop, replacing the one `#{R:\u{2500},…}`), plus two macros next to `preview_width!`/`version_width!` (the fill count, currently inline in the format, and the fill's start `F0`) so the count is written once and the loop uses it (`F1` = `F0` + count). Contract text in `docs/commands.md` §6.1 that describes the StatusBar (one line on the junctions).
- **Size:** about 40 lines of Rust (macros, `max`/`min` helpers, the loop, the zoom guard, the doc comment); the format string grows 1.4 KB → 4.3 KB (generated; nothing is written by hand).
- **Tests that would pin it** (contract: the columns, not the string): an integration test on a private tmux server (the repo's existing tmux tests in `core/src/host/tmux.rs` run one) that builds the cockpit's panes in a window of a given width, reads the status row with a client (or `display -p` of `STATUS_FORMAT` with `-t`/`-c`), and asserts, as the runs above did: (1) a `┸` at every `pane_right + 1` of a bottom-row pane that is not at the right edge, and at no other column; (2) none where the left group (cols 0 to 11), the preview pill, or the right group (long and short form) covers it; (3) the row is `client_width` cells wide; (4) a zoomed window has no `┸`; (5) after `resize-pane -x` the columns move. The existing format-width tests keep passing if the new format keeps the fill arithmetic: unverified until run.
- **Decisions that are the User's:**
  1. **Minimum tmux 3.6** for `#{P/i/:}` (the cockpit is tested on 3.8; Q1.3). If an older tmux must work, the junctions cannot be drawn without the sort, and the format would show none (design: probe the version and keep today's format for it).
  2. **Junction colour**: grey 8 like the line (simple, mismatched with a focused blue border) or the border's colour (more format).
  3. **Index-order assumption**: accepted as unproven; the failure is cosmetic (a wrong run of `─`), shown to hold in 8 layout changes (Q1.3).
  4. **Keep `┸`** (heavy-up / light line) or choose `┻` / `┴` (Q1.4).

If the User does not accept tmux 3.6 as the floor, the answer is **do not do it**: B works on any tmux but lags a drag by a process, rebuilds the whole format in Rust and runs a process per mouse-motion event, which is not clean.

## Cleanup

Both private servers (`cpd-research-<pid>`, `-outer`) and the third (`-outer2`) were killed with `tmux -L <name> kill-server`; the scratch files in `/tmp/rd/` (the extracted formats, the generator, the hook script, the logs) were deleted. Nothing was run on the socket `cpd` or in `~/.cpd`; no file of the repo but this one was changed.
