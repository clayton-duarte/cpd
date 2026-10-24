# Audit: the Plan view, the Deploy column and the small views against ratatui 0.30.2 and the ecosystem

Job `audit-plan`, Plan `ui-widgets` (hs-d597 (5), hs-d598). Research only: no code, test or dependency was changed, and every
finding goes to the User before anything is fixed. Code read at `origin/main` c2953181 (it holds `Invert pill and selection
text`, #291, so the newest tui code is on main) in a throwaway worktree, since removed. Ratatui source read in
`~/.cargo/registry/src/*/`: ratatui-0.30.2, ratatui-core-0.1.2, ratatui-widgets-0.3.2; also tui-scrollview-0.6.8,
ratatui-flow-0.1.1. Crate facts (version, updated, downloads) were fetched from crates.io on the day of the audit.

Area: `tui/src/plan/*`, `widgets/*`, `logs/*`, `history/*`, `worker/*`, `merge/*`. Paths below are relative to `tui/src/`.

**Two experiments** (scratch edits in the throwaway worktree, run with `scripts/cargo.sh test -p cpd-tui <filter>`, then
reverted with `git checkout`): E1 drew a PR whose title is wide characters (shows F1); E2 deleted one line of
`widgets/column.rs` and ran the `widgets::` tests (shows the line is needed, see "Checked and kept").

## Findings

Ranked by value (what the User sees first, then lines deleted, then risk removed). Every `replace` here is **in-crate**: no
finding needs a new dependency. Out of scope and owned elsewhere: the pill caps (`plan/flow.rs:141-142`, `widgets/legend.rs:25-37`,
a third copy of the caps) see Job pill-spec; box spacing and the box widget (`plan/boxes.rs`) see Job box-widget; the
off-screen 1x1 render in `tree/rows.rs` see Job tree-scroll; the read path, polling and redraw rate see Plan sync (hs-p62).

### F1. Seven helpers cut or wrap text by `char`s, not columns: a PR title, an Event or a header in wide characters overflows its row

- **Where:** `widgets/column.rs:134-143` (`truncate`) and `:156,:176,:198` (`pr_lines` counts `chars()`), `plan/flow.rs:147-153`
  (`truncate` of a pill's key), `merge/mod.rs:139-144` (`cut`), `logs/view.rs:160-164` (the Event's text), `worker/view.rs:62-90`
  (`wrap_header`), `scroll.rs:83-` (`wrap`, the Worker and history body), `widgets/legend.rs:65,75,252,286` (widths of fixed
  ASCII titles, harmless).
- **What it does:** the row is laid out as if every character took one terminal column.
- **Why it exists:** unverified: no reason found. It is `str::chars()`. The same crate has the right one: `tree/rows.rs:292-311`
  (`width`, `truncate`: "Counted in display columns, not chars (names are free text)"). The tree audit (`audit-tree.md` F1) found
  the same defect in the tree's own files; these are the sites in this area.
- **Evidence (E1, run):** `pr_lines` of an open PR with the title `計画計画計画計画計画計画計画計画`, column width 30 and 40:
  line 2 was **39 and 52 columns wide** (line 1 held at 30 and 40). The ASCII title of the same length gave exactly 30. Ratatui
  clips what is past the column, so the title's cut is wrong and, on a `clean` PR, the merge icon that `pr_lines` places in the
  last column is pushed off the column (a click on it opens no popup). PR titles, Event texts and Worker header reasons are free text.
- **Upstream:** `Line::width()` and `Span::width()` count display columns (ratatui-core-0.1.2 `text/line.rs:441`, `text/span.rs:271`;
  `unicode-width` 0.2.2 is in `Cargo.lock`). Upstream has no cut-with-`…` (the tree audit read the same; `unicode-truncate`, 3.0.0 on crates.io,
  updated 2026-08-20, 63,983,740 downloads, is already in `Cargo.lock` as 2.0.1 through ratatui: whether it has an ellipsis form is **unverified**).
  `Paragraph::wrap(Wrap { trim: false })` wraps by grapheme width, but cannot do `scroll.rs`'s hanging indent or `wrap_header`'s
  five-row cap with ` …`, so those two stay hand-made but must count columns.
- **Verdict:** `replace`: one `truncate`/`width` for the crate (the tree's, moved to a neutral module), used by the seven sites.
- **Cost:** files: `widgets/column.rs`, `plan/flow.rs`, `merge/mod.rs`, `logs/view.rs`, `worker/view.rs`, `scroll.rs` (+ the tree's module);
  about 40 lines deleted, 10 added; tests that move: the width asserts of `widgets::column::tests` (they count `chars()` at
  `column.rs:531-578`), `scroll`/`worker` wrap tests; new tests on wide input. Risk: low; the ASCII output is unchanged.
- **The User sees:** a wide-character title no longer pushes the second line out of the Deploy column, and the merge icon keeps its cell.

### F2. The popup's tmux command is built four times, and the Logs popup patches the legend's by array index

- **Where:** `logs/mod.rs:47-56` (`popup_args` calls `legend::popup_args` and overwrites `args[3]` and the last element), `widgets/legend.rs:297-312`,
  `merge/mod.rs:175-193`, and `dialog/mod.rs:251-` (outside this area, same shape).
- **What it does:** each builds `display-popup -E -T <title> -w <w> -h <h> <command>`; Logs reuses the legend's vector and replaces the
  title (index 3) and the command (last).
- **Why it exists:** unverified: no reason found. `logs/mod.rs:45` says "sized like the legend's": the copy was the shortest way. The
  index patching breaks silently if the legend's argument order changes (explicit over implicit); the command text is also written
  `"cpd tui view logs"` there while the other two use `PROGRAM`.
- **Upstream:** tmux 3.8 (`man tmux`, `display-popup`): `-w` and `-h` "may be a percentage"; so `-w 80%` would give Logs' `SHARE`. It does not replace
  the others, which want "at most the client's size" (`min`), nor Logs' "legend size, at most 80 %". No ratatui or crate is involved (tmux strings
  are for the host audit).
- **Verdict:** `replace`: one `popup(title, size, client, command)` builder; the three views call it.
- **Cost:** files: `logs/mod.rs`, `widgets/legend.rs`, `merge/mod.rs` (+ `dialog/mod.rs`); about 30 lines deleted, 15 added; tests that move:
  the argument-vector asserts (`legend.rs:513`, logs and merge popup tests). Risk: low.
- **The User sees:** nothing.

### F3. Four copies of "copy a window of cells from one Buffer into another" (and two more beside them)

- **Where:** `plan/flow.rs:533` (`with_margin`) and `:574` (`blit`), `plan/boxes.rs:263` (a chart into its box), `widgets/legend.rs:278-282`
  (the legend's scroll window); in the crate also `tree/rows.rs:665` and `tree/counter_tests.rs:38`.
- **What it does:** a double `for y, for x { dst[..] = src[..].clone() }` with its own clamping each time. `Chart::blit` says why:
  "(Hand-written: ratatui has no widget that copies a window of a `Buffer`.)" (`flow.rs:568`).
- **Why it exists:** the comment is right: ratatui has no blit. `Buffer::merge` (ratatui-core-0.1.2 `buffer/buffer.rs:435`) copies one buffer at its own
  area into another, but it re-indexes **all** of `self` on every call (`:441-451`) and has to be fed a clone whose `area` was moved first, so it is
  worse than the loop for `boxes.rs`, which calls it once per box.
- **Upstream:** `tui-scrollview` 0.6.8 (updated 2026-09-24, 507,862 downloads, ratatui-core 0.1 / ratatui-widgets 0.3 so ratatui 0.30; deps
  `indoc` (already in `Cargo.lock`), `ratatui-core`, `ratatui-widgets`) has the same loop (`src/scroll_view.rs:388-396`, with the author's own
  `// TODO: there's probably a more efficient way`) and the same offset clamp (`:257-262`), plus scrollbars and a stateful widget (`ScrollViewState`). It would
  replace `Chart::blit`'s copy and the legend's, but not the selection overlay (`flow.rs:578-593`), and it renders its own scrollbar unless turned off.
  A `StatefulWidget` of upstream's is allowed by hs-d596 (cpd defines none) but the gain is two 5-line loops: **not a real gain** under docs/implementing.md.
- **Verdict:** `replace` (in-crate): one `copy_cells(src, src_rect, dst, at)` helper that clamps once; the four callers use it. `tui-scrollview`: `keep` out.
- **Cost:** files: `plan/flow.rs`, `plan/boxes.rs`, `widgets/legend.rs` (+ `tree/rows.rs`, owned by tree-scroll); about 25 lines deleted, 12 added; tests that move:
  `flow::tests::the_window_scrolls_to_the_selection...`, `boxes` body tests (they pin the result, not the loop). Risk: low.
- **The User sees:** nothing.

### F4. Pulse re-implements the dialog kit's scroll arithmetic

- **Where:** `plan/pulse.rs:21,92-103` (`NOTCH`, `max_offset`, `clamp`, `scroll`) against `dialog/scroll.rs:10,56-61,70-73,93-99` (`WHEEL_ROWS`, `max_offset`,
  `by`, `mouse`); the Plan view keeps `pulse_offset: usize` (`plan/mod.rs:539-540`).
- **What it does:** an offset into `rows` seen through `height` rows, clamped to `rows - height`, three rows a wheel notch: the same maths as `Scroll`.
- **Why it exists:** Pulse (#73, `3b626704`) is older than the kit (`3e7e657e`, "one scroll"); the kit's own doc says "the Worker and Plan views' notch" (`dialog/scroll.rs:9`).
- **Upstream:** ratatui's `List` cannot be used instead: it clamps the offset only to `items.len() - 1` (ratatui-widgets-0.3.2 `list/rendering.rs:135`), so
  the last row may reach the top, and `ListState::scroll_down_by` moves the *selection*, not the offset (`list/state.rs:248-251`). That is why a clamp exists at all.
- **Verdict:** `replace` (in-crate): the view holds a `Scroll` (its state, passed in; hs-d596) and Pulse keeps only `row_at` and the drawing.
- **Cost:** files: `plan/pulse.rs`, `plan/mod.rs`; about 15 lines deleted; tests that move: `plan::pulse::tests::the_wheel_clamps_and_a_click_maps_to_the_row_under_it`. Risk: low.
- **The User sees:** nothing (the notch is 3 in both).

### F5. `merge/mod.rs` starts and ends the terminal by hand, as `run_view` does, and loses the mouse mode on a panic

- **Where:** `merge/mod.rs:247-262` (`ratatui::init()`, `execute!(EnableMouseCapture)`, `ratatui::restore()`); the same in `dialog/mod.rs:171-175` (outside this area).
- **What it does:** the same sequence as `runner::run_view` (`runner.rs:71-99`) but without its `Restore` guard.
- **Why it exists:** `merge` came first (#99, `1378cc6a`, 2026-10-19); `run_view` came with "Cockpit views: one runner" (`3e7e657e`) and `merge` was not moved onto it (unverified: no reason found).
- **Upstream:** `runner.rs:60-63` says why the guard matters: "ratatui's own panic hook restores only raw mode and the alternate screen, never the mouse and
  focus reporting". `ratatui::init()` also panics when the terminal cannot start, where `run_view` uses `try_init` and reports `not_a_terminal`. A panic inside `merge::show`
  leaves the popup's tmux client in mouse-reporting mode (unverified: not run; read from `runner.rs`).
- **Verdict:** `replace` with `run_view` (in-crate): the chosen `Action` leaves the body in a local.
- **Cost:** files: `merge/mod.rs` (+ `dialog/mod.rs`); about 14 lines deleted, 6 added; tests that move: none found (the merge tests draw `draw`, not `run`). Risk: low.
- **The User sees:** nothing, until a panic.

### F6. The "write a temporary file, rename it over the target" sequence is written six times; two are in this area

- **Where:** `plan/mod.rs:327-337` (`Offsets::write`) and `merge/marker.rs:50-56`; also `core/src/plan/mod.rs:257-266`, `core/src/session/port.rs:95-103`,
  `core/src/host/git_guard.rs:120`, `core/src/store/mod.rs:826` (found by `rg "fs::rename"`; the last two bodies not read, same call shape).
- **What it does:** `fs::write(tmp)` then `fs::rename(tmp, target)`, removing `tmp` on a failure; each names the temp file differently (`json.tmp<pid>`, `.<pid>.tmp`, ...).
- **Why it exists:** each file wanted an atomic replace (the marker's comment: "no reader takes it for a marker"); no shared helper existed.
- **Upstream:** none in std. `tempfile::NamedTempFile::persist` does it, but `tempfile` is a dev-dependency only (docs/implementing.md: later Jobs may not add dependencies).
- **Verdict:** `replace` (in-crate): one `write_atomic(path, bytes)` in `cpd-core`, used by the six.
- **Cost:** files: 6 (two in tui); about 30 lines deleted, 12 added; tests that move: the marker's and `Offsets`' write tests. Risk: low. Outside this Job's area for four of them.
- **The User sees:** nothing.

### F7. Nine test modules each define `row`/`rows`/`screen`/`text`, a Buffer-to-strings helper

- **Where:** `plan/flow.rs:610`, `plan/boxes.rs:387`, `plan/margin_tests.rs:10`, `widgets/column.rs:409`, `widgets/legend.rs:538`, `merge/mod.rs:382`, `history/view.rs:74`,
  `worker/view.rs:115`, `logs/view.rs:232`; the crate has six more (`tree/rows.rs:671,675`, `tree/look.rs:46`, `dialog/*`, `chat.rs:51`, `input_box_tests.rs:25,29`).
- **What it does:** turns a `Buffer` into `Vec<String>` (trimmed) so a test compares text.
- **Why it exists:** unverified: no reason found. Each test module wrote its own.
- **Upstream:** `Buffer::with_lines` (ratatui-core-0.1.2 `buffer/buffer.rs:92`) builds the *expected* buffer from strings and `assert_eq!(buffer, expected)` compares text and
  style with a readable diff; the layout test of hs-d598 reads `(text, Style)` cells, which this does too. It does not trim trailing blanks, so the expected lines are full width. insta is in the registry (1.49.0), and hs-d598 rules it out.
- **Verdict:** `replace` in part: one shared test helper for the "text of a row" reads (and `with_lines` where the whole buffer is pinned).
- **Cost:** files: the nine above; about 50 lines deleted; tests that move: every test that calls the helper (mechanical). Risk: none (tests only).
- **The User sees:** nothing.

### F8. A stale comment says ratatui has no way to merge box-drawing glyphs into junctions (the code stays)

- **Where:** `plan/flow.rs:7-10` ("ratatui has no widget for box-drawing edges that merge into junctions"), the code it justifies is `Mask` and `glyph` (`:231-292`, shared with `plan/boxes.rs`).
- **Why it exists:** `74960ea8` (the chart's first version), written before ratatui 0.30.
- **Upstream:** ratatui 0.30 has `Cell::merge_symbol(symbol, MergeStrategy)` (ratatui-core-0.1.2 `buffer/cell.rs:146`) and `Block::merge_borders`
  (`ratatui-widgets-0.3.2 block.rs:238`), and the docs page `recipes__layout__collapse-borders.md`. It merges `│` and `─` into `┼`. It
  **cannot** merge a rounded corner with anything: `symbols/merge.rs:31` ("Combining any rounded segments with any other segments (e.g. `╯` with `─`)") and `:362-364` replace rounded by plain. The chart's
  lines have rounded corners (`╭╮╰╯`, `flow.rs:240-243`) that turn plain where a third line joins, which `Mask` (a 4-bit set per cell: up, down, left, right, 11 glyphs) does in 25 lines. Using `merge_symbol` would need each line piece drawn as the exact glyph (corner or straight) first: more code, not less.
- **Verdict:** `keep` the code; **remove the stale claim** from the comment (say: `Cell::merge_symbol` exists but turns a rounded corner plain; `Mask` keeps `╭` for a pure corner).
- **Cost:** one comment, `plan/flow.rs`; no test. **The User sees:** nothing.

## Checked and kept

- **`plan::flow::starts` instead of `Layout`** (`flow.rs:307-318`): kept; its comment measures it: "its constraint solver took 4.6 s for a column of 300 cards in a debug build" (`cli/tests/tui_idle.rs`, commit `a853253b`). Not re-measured.
- **`Chart::blit`'s selection overlay** (`flow.rs:578-593`): kept; `set_style` over three rects (body, two caps) is ratatui's own API.
- **Pulse bars by hand, not `Sparkline`** (`pulse.rs:22-55`): `Sparkline` takes per-bar styles (`SparklineBar::style`, `ratatui-widgets-0.3.2 sparkline.rs:253-281`) and an absent-value symbol, but `scale_height` **floors**
  (`value * 8 / max`, `:434-441`), so a bucket with 1 event among a maximum of 100 draws a blank where Pulse draws the lowest bar (`pulse.rs:43` rounds up and clamps to 1). The legend and the row also reuse the same `bar`/`glyph` `Span`s. Using `Sparkline` would need pre-scaled levels with `.max(8)` and a three-cell split of every row: more code.
- **A `List` for the Pulse rows:** see F4 (the offset is not clamped to the last page).
- **Scrollbars** (`ratatui::widgets::Scrollbar`, `tui-scrollbar` 0.2.8, updated 2026-09-24, 1,597,728 downloads): the cockpit's cue rows `▲ n more` are the specified design (hs-d348, `dialog/scroll.rs:12-30`, the one `cue`); a scrollbar takes a column of every region. Not adopted, as the tree audit.
- **`window`/`max_scroll` of the PRs section** (`widgets/column.rs:226-266`): kept; whole two-row items under a cue row have no upstream (a `List` item can be partly cut and has no cues).
- **`Clear` before the error row** (`column.rs:304-307`): it is ratatui's own `Clear`, used as the docs describe.
- **The blue block's trailing blank span** (`column.rs:221`, `block()`): kept. **E2:** deleting the `line.spans.push(Span::styled(" ".repeat(...)))` line made
  `widgets::column::tests::conflict_is_cyan_on_an_owned_pr_..._in_the_active_block` fail (the cells past the text were not selected). Cause: `Paragraph` draws a line's graphemes cell by cell
  (`ratatui-widgets-0.3.2 paragraph.rs:471`), so `Line::style` is not applied past the text there, only when a `Line` is rendered as a widget (`ratatui-core-0.1.2 text/line.rs:730`).
- **`Table` with computed column widths in the legend** (`legend.rs:59-99`): `Table` has no content-fit width (`Constraint::Length` computed by hand); kept. The `Layout` stack of sections is `Layout`.
- **`Block::bordered().border_type(Rounded)` for the PR boxes** (`boxes.rs:245`): upstream's own widget, already used.
- **The chart's graph layout (`Mask`, `free_row`, buses) against `ratatui-flow` 0.1.1** (updated 2026-06-18, **65 downloads**, one repository, ratatui 0.30, deps `log`, `unicode-width`): it
  lays out boxes and arrows on a grid search; cpd's chart has stacked Job cards with shared needs, one bus per gap, 1-row pills and a tier column: not what it draws. Not adopted. (`ratatui-flow`, like `ratatui-interact`, `ratatui-textarea`, `ratatui-toaster`, `ratatui-image`, is in the registry but in no `Cargo.toml` or `Cargo.lock` of the repo.)
- **CPU, GPU and memory parsers in `widgets/host.rs`** against `sysinfo` 0.39.6 (updated 2026-07-09, 229,223,597 downloads; deps `libc` (in the lock), optional `memchr`, `rayon`, `serde`, `objc2-*` on macOS): it would
  replace `parse_proc_stat`, `parse_meminfo`, `parse_ps`, `parse_vm_stat` and the `sysctl` calls (about 70 lines and their tests), not the GPU (`nvidia-smi`, `ioreg`: it reads no GPU, unverified). Kept: the only reasons to move would be the macOS CPU
  (`ps -A -o %cpu=` summed is `ps`'s decayed average, not the share since the last sample: **unverified**) and fewer parsers; against it: a new dependency with macOS-only transitive crates (docs/implementing.md: take one only for a real gain), and `host.rs:62-66` records the view's I/O budget (`cli/tests/it/tui_idle.rs`), which I did not measure against `sysinfo`. User decision if the macOS number is judged wrong.
- **The streaming `nvidia-smi -lms` child** (`host.rs:57-117`): kept; its comment gives the reason (a spawn per sample counted against the idle budget, tui-F1/F2).
- **`pulse::row_at` and the other hand hit-tests by saved Rects:** the documented approach (handoff §5); nothing better found.
- **`Offsets`' debounce (`plan/mod.rs:315-324`) and the JSON file:** no upstream stores a view's scroll; its atomic write is F6.
- **`merge::buttons` fixed columns:** two buttons; ratatui has no button. `merge::sh_quote`: no shell-quote crate is in the lock; two lines.
- **Tmux popup percentages for Logs:** see F2: `-w 80%` cannot express "at most the client".
- **`scroll::Scroller`'s own wrap** (hanging indent), `worker::wrap_header` (five rows, ` …`): `Paragraph::wrap` has neither (see F1 for the column counting they need).
- **`throbber-widgets-tui` 0.11.1** (updated 2026-06-19, 1,031,135 downloads): stateful and ticks on its own; the spinner is a function of the shared clock (`palette::frame`) as hs-d595 asks. Kept, as the tree audit.
- **`tui-popup` 0.7.7 (1,054,073 downloads), `tui-overlay` 0.1.2 (3,353):** in-terminal popups; the cockpit's popups are tmux `display-popup` (they overlay the other panes). Not applicable.
- **`ratatui-textarea` 0.9.3, `tui-input` 0.15.5, `tachyonfx` 0.25.2, `tui-logger` 0.18.3:** the area has no text input (the input box is in tree/dialog), no animation and no log widget needing them: the Events log is a list read from the store. Not read beyond version and downloads.
- **`ratatui::init`/`restore` as such:** used (see F5 for the one place that bypasses `run_view`). `ratatui::run` is not used: `run_view` needs the pre-read and the capture guard.

## Not checked

- `plan/mod.rs`: the event loop's mixing of reads, polling and drawing (`show`, `:399-640`): Plan sync owns when it reads and draws; the Elm shape is hs-d595. Read, not judged.
- `plan/boxes.rs`: `levels`, `live_needs`, `state_word` and the box title (`:51-165`): read, no upstream equivalent expected (layout of this graph), not searched further.
- `plan/actions.rs` (tmux calls to open the Worker pane): the tmux strings belong to the host audit.
- `worker/render.rs` and `history/mod.rs`: how a pi session file turns into lines; read only their heads. `merge/action_d550.rs`, `merge/marker_d550.rs`: tests. All `*_tests.rs`, `fixtures/`: only grep.
- `widgets/mod.rs` below line 120 (the loop, `Opener`, `pick`): read for the loop shape only; `read/*` is Plan sync's.
- Whether `tui_idle`'s I/O budget would allow `sysinfo`: not measured.
- No crate was built or run except the two scratch tests above.

## Summary for the User

- F1 replace: seven text cutters count `char`s; a wide-character PR title makes line 2 of a Deploy-column row 39 columns in a 30-column pane (E1) and pushes the merge icon out; reuse the tree's column-counting `truncate`.
- F2 replace: `display-popup` arguments built four times, Logs patching the legend's by index (`args[3]`); one builder.
- F3 replace: the "copy a window of a Buffer" loop exists four times here (six in the crate); one helper (ratatui has none; `Buffer::merge` and `tui-scrollview` do not fit better).
- F4 replace: Pulse's scroll clamp and wheel notch duplicate `dialog::scroll::Scroll`; `List` cannot replace it (offset not clamped to the last page).
- F5 replace: `merge` and `dialog` start the terminal by hand, outside `run_view`, so a panic leaves mouse reporting on.
- F6 replace: the write-temp-then-rename sequence is written six times (two here); one `write_atomic`.
- F7 replace: nine test modules define the same Buffer-to-text helper; one helper, `Buffer::with_lines` for expected buffers.
- F8 keep: `Mask` stays (`Cell::merge_symbol` cannot merge rounded corners); only its comment is stale.
- No finding needs a dependency; `sysinfo`, `tui-scrollview`, `ratatui-flow`, `Sparkline`, `List`, `Scrollbar` were read and are not adopted (Checked and kept).
