# Audit: the tree view area against ratatui 0.30.2 and the ecosystem

Job `audit-tree`, Plan `ui-widgets` (hs-d597 (5), hs-d598). Research only: no code, test or dependency was changed, and
every finding goes to the User before anything is fixed. Code read at `origin/main` c2953181 (it holds the `Invert pill and
selection text` commit #291, so the newest tui code is on main). Ratatui source read in `~/.cargo/registry/src/*/`:
ratatui-0.30.2, ratatui-core-0.1.2, ratatui-widgets-0.3.2, tui-tree-widget-0.24.1.

Area: `tui/src/tree/*`, `fold.rs`, `section.rs`, `scroll.rs`, `menu.rs`, `link.rs`, `updating_label.rs`, `pace.rs`,
`palette.rs`, `palette/colours.rs`, `dialog/scroll.rs` (the scroll cue; it is the one `cue` every region draws with).

Crate facts below were fetched from crates.io on the day of the audit (`curl -A cpd-research .../api/v1/crates/<name>`) and the
crate's `Cargo.toml` / source read in the local registry.

**One experiment** (a scratch test appended to `tree/inbox.rs`, run, then reverted with `git checkout`; the tree is clean): it
showed F1 below. Nothing else was run.

## Findings

Ranked by value (what the User sees first, then lines deleted, then risk removed).

### F1. Cell widths are counted in `char`s in six places; the tree's own `truncate` counts columns. A label with wide characters breaks the row.

- **Where:** `tui/src/tree/inbox.rs:314-315` (`label.chars().count()`, `age.chars().count()`: the gap before the age and the
  cells of the OSC 8 link), `tui/src/section.rs:61-69` (`take(..)`/`chars().count()` on the lead, title and count),
  `tui/src/dialog/scroll.rs:27-28` (the cue's cut and padding), `tui/src/menu.rs:152` (a busy entry's padding),
  `tui/src/scroll.rs:90-95` (`wrap` splits at `width` *characters*: `cells.len() <= width`, `split_at(width)`).
- **What it does:** the row is laid out as if every character took one terminal column.
- **Why it exists:** unverified: no reason found. It is the default of `str::chars()`. The same crate already counts columns
  elsewhere: `tree/rows.rs:293-311` (`width`, `truncate`: "Counted in display columns, not chars (names are free text)",
  with a test on `計画計画`), so the author knew; the later copies did not use it.
- **Evidence (the scratch test, run):** `item_line` of an Inbox item with the label `計画計画計画計画計画計画`, row width 30,
  age `5m`: the label is cut by `truncate` to 12 wide chars (24 columns) but `label_width = label.chars().count()` is 12, so the
  gap is computed as if it took 12 columns; the drawn line is **41 columns in a 30-column row** (the age is pushed out of the
  pane; the link rect is half the label). The ASCII label gave exactly 30. `scroll::wrap` of ten `計` at width 10 returned **one
  row 20 columns wide** (ratatui clips it: the second half of the line is lost, not wrapped). Labels of Inbox items and the Worker
  and history views' lines are free text (Plan titles, Job names, agent output), so this is reachable.
- **Upstream:** `Line::width()` / `Span::width()` count display columns (ratatui-core-0.1.2 `text/line.rs:441`, `text/span.rs:271`;
  `unicode-width` 0.2.2 is already in `Cargo.lock`), and `Paragraph::wrap(Wrap { trim: false })` wraps by grapheme width
  (`ratatui-widgets-0.3.2/src/reflow.rs`: `WordWrapper`, `LineTruncator` work on `StyledGrapheme`). `scroll.rs::wrap` also has
  a hanging indent (`INDENT = 6`) that `Wrap` does not offer, so the wrap stays hand-made but must count columns; use the
  `rows::width` helper that already exists.
- **Verdict:** `replace` the `chars().count()` / `take(n)` cell arithmetic with the display width (`rows::width`, `Span::width`).
  Keep `scroll::wrap`'s own indent.
- **Cost:** 5 files (`inbox.rs`, `section.rs`, `dialog/scroll.rs`, `menu.rs`, `scroll.rs`), about 25 lines changed, no new
  dependency; new tests with a wide label for each (`inbox.rs` item row, `scroll.rs` wrap, the cue); the existing tests use ASCII
  and do not move. Risk low. (`section.rs` and the pill are also touched by Job pill-spec: see pill-spec for the title/pill
  assembly; the `take()` on the title text is the part reached here.)
- **User sees:** an Inbox item or a Worker line with Chinese/Japanese/emoji text no longer pushes the age out of the pane or
  runs past the edge; ASCII rows are unchanged.

### F2. The tree uses `tui-tree-widget` only for its open set and flatten, then undoes most of what its renderer draws.

- **Where:** `tui/src/tree/rows.rs:499-503` (`node_closed_symbol("")`, `node_open_symbol("")`, `node_no_children_symbol("")`; a render
  into a 1x1 buffer and `state.scroll_up(usize::MAX)` / `scroll_down` to set the offset: **the 1x1 render is Job tree-scroll's,
  not analysed here**), `rows.rs:518-523` (copies every cell of the row left by `depth * INDENT` columns), `rows.rs:549`
  (`buf.set_style(rect, palette::selected())` instead of the widget's `highlight_style`), `tree/rows.rs:395-423` (the fold's closing
  row is a *sibling* item "so that it stays under the live rows (the tree widget draws a child only under an open item)"),
  `tree/view.rs:700-708` (`row_at`: the widget's `rendered_at` answers "with the last row" below the tree, so a second call
  one row up is compared).
- **What it does:** a two-level, fixed-shape tree (Context -> Plan/fork) is built as `TreeItem`s only so the crate can flatten
  it and keep `opened` and `selected`; its drawing is then neutralised (no node symbols, indentation shifted back out, its offset
  forced, its highlight unused) and our own code draws the guides, counters, muted colours and the cursor on the buffer.
- **Why it exists:** the tree started on the crate (commit `2e201c14`, Tree look: ╭ ├─ ╰─ folds, #154) and the fold look
  (`├─` straight against the edge, no node symbol) was fitted over it afterwards. The shift's comment says why: "the widget
  indents a child by two columns per level; the fold's branch is straight against the edge".
- **Evidence:** `tui-tree-widget-0.24.1/src/lib.rs:290-296` draws `flattened.depth() * 2` blanks and then the node symbol before
  the text; `lib.rs:205-236` clamps `state.offset` to the rows `last_render` saw (the reason for the 1x1 render). The crate has no
  option for either (`block`, `style`, `highlight_style`, `highlight_symbol`, the three node symbols, `experimental_scrollbar` are
  all of `Tree`'s builder, lib.rs:110-151). cpd uses from the crate: `TreeState::{opened, open, selected, select, flatten,
  rendered_at}` and `TreeItem` (33 references in `folds.rs`, `rows.rs`, `view.rs`). tui-tree-widget 0.24.1: max_version 0.24.1,
  updated 2026-08-09, 1,983,630 downloads, supports ratatui 0.30 (`ratatui-core 0.1`, `ratatui-widgets 0.3`); it is maintained
  and nothing is wrong with the crate, cpd just uses a small part of it.
- **Upstream alternatives:** (a) own flat rows: the tree is two levels, `rows::items` already builds every row as a `Line`
  and `flatten` is a loop over Contexts; a `Vec<(path, Line)>` rendered with `Paragraph::new(lines).scroll((offset, 0))`
  (stateless, as hs-d595/hs-d596 want) needs no offset hack, no shift, no 1x1 render. State = the open set and the cursor path,
  which `folds.rs` already serialises. (b) ratatui's own `List` + `ListState` (`ratatui-widgets-0.3.2/src/list/state.rs`:
  `offset_mut`, `selected_mut` are public; `list/rendering.rs:135` clamps the offset only to `len - 1`, so no 1x1 render either;
  `highlight_style` draws the cursor row): it keeps a `StatefulWidget`, which hs-d596 avoids for cpd's own widgets (this one is
  ratatui's). Neither gives the cue rows (see Checked and kept).
- **Verdict:** `replace` tui-tree-widget's rendering with own `Vec<Line>` rows; the crate may then go (a dependency change, the
  User's call: `docs/implementing.md` "Dependencies" says Jobs may not change dependencies, and ratatui + tui-tree-widget are listed).
  Alternatively keep the crate for state only. The smaller step is the same code change without touching `Cargo.toml`.
- **Cost:** `tree/rows.rs` (`draw`, `items`, `context_items`, about 120 lines out, 60 in), `tree/folds.rs` and `tree/view.rs`
  (the state type: `HashSet<Vec<Id>>` + selected path, 8 call sites), optionally `Cargo.toml`/`Cargo.lock`. Tests that move:
  `rows.rs` (every test that builds a `TreeState`: ~25 of the 33 references), `folds.rs` (5), `view.rs` tests that call
  `view.state.select/opened` (a dozen); no drawn-text expectations should change. Risk medium (cursor, fold persistence, click
  mapping all run through the state); it is the change that also removes the off-screen render of Job tree-scroll, so do them
  together.
- **User sees:** nothing; less code that works against its own widget, and one less crate if the User chooses so.

### F3. After the tree widget draws, `draw` rewrites the buffer: cell shift, symbol-sniffing restyle, counters rendered on top.

- **Where:** `tui/src/tree/rows.rs:518-523` (shift), `rows.rs:525-532` (counters rendered with `Line::right_aligned()` straight
  into the buffer and their columns remembered as `counter_cols`), `rows.rs:533-547` (the muted/asleep restyle: for every cell
  of a muted row it tests `x < rect.x + 3 && "╭├╰│─".contains(buf[(x, y)].symbol())` to find "the branch", skips cells whose
  `fg` equals the selected name's, and re-styles the rest).
- **What it does:** the row's colours depend on which glyph happens to be in which cell: a branch is recognised by being one of
  five characters in the first three columns; "the selected name" by its foreground colour.
- **Why it exists:** the rows are built as `Line`s whose styles are the unmuted ones (`row_text`, `selectable_name`,
  `fold::content`), and archived (colour 8) / asleep (colour 7, branch and counters 8) rows are recoloured afterwards (introduced
  with the 16-colour palette, `93c817e5`, #229). `row_text` already takes `muted` and passes it to `dot_span`, so half the muting
  is done at construction.
- **Evidence:** `fold::content(branch, line)` (`fold.rs:99-103`) already produces the muted branch as its own `Span`; the
  name span is made by `selectable_name` (knows `selected`); counters are `counters(plan)` (knows its width, used to cut the name:
  `rows.rs:334`). All of it is known when the `Line` is built; the buffer is only needed to know what was drawn.
- **Upstream:** `Line::style`, `Span::patch_style`, `Line::right_aligned` (ratatui-core `text/line.rs:425`) on a `Line` built
  with its final styles; or one `Layout::horizontal([Fill(1), Length(counters_width)])` per Plan row. No crate needed.
- **Verdict:** `replace`: style the spans when the `Line` is built (muted flag -> `palette::archived_text()`/`asleep_text()` on
  the content spans, branch and counters left as they are) and draw the counters as part of the row (a `Line` with a padding span
  to the right edge, or the horizontal split). Deletes the `"╭├╰│─".contains` test and `counter_cols`.
- **Cost:** `tree/rows.rs` `draw`, `row_text`, `plan_row`, `fork_row`, `context_text` (about 40 lines out, 25 in); tests that move:
  the muted/asleep/selected tests in `rows.rs` (they read buffer (text, Style) cells and should pass unchanged: that is the
  proof). Risk low-medium: the colour rules hs-d534 (8 over 7, selected beats muted) are subtle, so the existing cell tests are the
  guard. Best done with F2 (same function).
- **User sees:** nothing, if the tests pass unchanged. A new glyph in the first three columns no longer gets recoloured by accident.

### F4. The scroll cue pads by hand; the right-aligned cue is built twice.

- **Where:** `tui/src/dialog/scroll.rs:22-30` (`cue`: `" ".repeat(width - text.chars().count())`, then the text), `tree/inbox.rs:333-335`
  (`cue_line`: the `│` branch plus the cue padded to `width - 1`), against `tree/rows.rs:530` which already uses
  `Line::right_aligned()` for the counters.
- **What it does:** a muted `▲ <n> more` / `▼ <n> more` right-aligned in a row, blank to its left.
- **Why it exists:** hs-d348 chose the text cue ("the one scroll indicator of cpd", commands.md §17 "Scroll cues"); the padded `Line`
  lets the Inbox put the fold's `│` in the same line. `git log -S'fn cue'` -> `96bcc735` (#164). No comment gives a reason for padding
  over alignment: unverified.
- **Upstream:** `Line::right_aligned()` / `Paragraph::right_aligned()` (ratatui-core-0.1.2 `text/line.rs:425`; a `Line`'s alignment is for the
  whole line, so the Inbox's `│` + cue needs a one-column split or the padding as now). Not `ratatui::widgets::Scrollbar`
  or `tui-scrollbar` 0.2.8 / `tui-scrollview` 0.6.8 (both updated 2026-09-24, 1.6M / 0.5M downloads, ratatui-core 0.1, ratatui 0.30
  compatible): they draw a bar in a column, not a count of hidden rows, which is the User's chosen cue.
- **Verdict:** `replace` (low value) the manual padding of `cue` with `Line::right_aligned` for the tree/Deploy rows; the Inbox
  composition stays (or splits the row). Mostly goes away with F1's width fix.
- **Cost:** `dialog/scroll.rs` (6 lines), `inbox.rs` (3), the tests `a_cue_is_right_aligned_in_its_row...` (3 in `dialog/scroll.rs`);
  risk low. Do it only with F1.
- **User sees:** nothing.

### F5. The right-click menu clamps its rectangle and hit-tests its inner area by hand.

- **Where:** `tui/src/menu.rs:92-93` (`x = at.x.min(pane.right().saturating_sub(width)).max(pane.x)`, same for `y`), `menu.rs:102`
  (`at.x > rect.x && at.x + 1 < rect.right() && at.y > rect.y && at.y + 1 < rect.bottom()`), `menu.rs:145-160` (the leading and
  trailing `" "` spans and `{:<width$}` padding so the selected row fills the box).
- **Why it exists:** the menu is a `Block` + `Clear` + `Paragraph` made when it was written (`59502932`, #150): the padding fills the
  blue selection block across the box (hs-n447 / hs-d534: the selected row is a block).
- **Upstream:** `Rect::clamp(self, other)` (ratatui-core-0.1.2 `layout/rect.rs:387-392`: `x.clamp(other.x, other.right().saturating_sub(width))`,
  the same arithmetic and it also shrinks to the pane); `Block::inner(rect)` + `Rect::contains` for the hit; `Block::padding(Padding::horizontal(1))`
  for the two blanks; `List` + `highlight_style` would fill the row without padding spans, but cpd's rows carry a busy-entry spinner span and the
  selection recolours every span (`menu.rs:156-158`), so the `List` gains little.
- **Verdict:** `replace` the clamp and the inner hit-test with `Rect::clamp` and `Block::inner`; `keep` the rest.
- **Cost:** `menu.rs` only, about 6 lines; tests `the_box_is_at_the_click_and_clamped_inside_the_pane` and the hit tests keep their
  assertions. Risk low.
- **User sees:** nothing.

### F6. The Worker/history footer height is counted with `div_ceil` while the text is drawn with word wrap.

- **Where:** `tui/src/tree/view.rs:727-729` (`rows::width(&footer).div_ceil(width)`, then `+ usize::from(wrapped > 1)`), drawn at `view.rs:~752` with
  `Paragraph::new(footer)...wrap(Wrap { trim: false })`.
- **What it does:** the snapshot-error footer reserves `ceil(columns / width)` rows (plus one when more than one).
- **Why it exists:** unverified: no reason found. `Paragraph::line_count(width)` is the exact count but is behind the unstable feature
  `unstable-rendered-line-info` (`ratatui-widgets-0.3.2/src/paragraph.rs:325-332`, `#[instability::unstable(feature = "rendered-line-info")]`),
  which cpd does not enable (and `docs/implementing.md` forbids changing features without the Lead).
- **Evidence:** `Wrap { trim: false }` breaks at word boundaries (`reflow.rs` `WordWrapper`), so a text can need more rows than
  `div_ceil` says; the extra `+1` for more than one row looks like a guess for that. I did not run it: the claim that the last row is
  cut is **unverified**; it follows from reading the wrapper.
- **Verdict:** `keep` unless the User wants the feature `unstable-rendered-line-info` (then `replace` with `Paragraph::line_count`); the alternative is to not wrap
  (the error is a one-line text) with `Wrap` removed. Low value.
- **Cost:** 5 lines, `view.rs`, tests `view.rs` (footer ones); risk low. Needs the User's decision on the feature.
- **User sees:** at worst, a long error footer shows all its lines.

## Checked and kept

- **Hit-testing by saved last-frame Rects** (`inbox_area`, `row_at`, `Shape::hit`, the menu's `rect`): the documented approach (handoff §5); not reported.
- **The right-click menu as a whole** (`menu.rs`, `tree/menu.rs`): `ratatui-interact` 0.5.3 (max 0.5.3, updated 2026-04-02, 79,104 downloads, ratatui 0.30) has a
  `ContextMenu` (`src/components/context_menu.rs`, 1,776 lines) that is also `Clear` + `Block` + `Paragraph`; it keys items by `String` id,
  colours with `Color`/`bg` highlight (against hs-n447's reverse video and hs-d534), has shortcuts/submenus we do not use, and its crate
  depends on `arboard`, `regex`, `termimad`, `serde`, `thiserror`. Nothing to gain. `ratatui-interact`, `ratatui-flow`, `ratatui-textarea`, `ratatui-toaster` and
  `ratatui-image` are in `~/.cargo/registry` but in no `Cargo.toml`/`Cargo.lock` of the repo; the only mention is `docs/research/cockpit/attention.md:35`
  ("toast crates ... were **not read**"). I found no record of why they were fetched (unverified: no reason found; likely a prior research run).
- **OSC 8 links** (`link.rs`): ratatui has no hyperlink API (`rg` over ratatui, -core, -widgets, -crossterm for `hyperlink`/`OSC 8` finds only
  tests in `ratatui-core-0.1.2/src/buffer/buffer.rs:1220-1245`, which use the same trick: `CellDiffOption::ForcedWidth` with the escape in the symbol). crates.io has
  `ratatui-link` 0.0.0 (2026-06-12, 20 downloads, "Reserved") and `tui-easy-styles` (22 downloads): nothing usable. Keep.
- **The spinner clock** (`palette::frame`, `SPINNER`, `pace::wait_for_next_frame`): `throbber-widgets-tui` 0.11.1 (updated 2026-06-19, 1,031,135 downloads, ratatui 0.30,
  deps `rand`) holds a `ThrobberState` that each widget steps by itself; cpd's clock is `(unix ms / 100) % 8`, a pure function so every spinner turns together
  (the glyphs are Nerd Font code points, which `throbber_set` can hold but adds nothing). Stateful, against hs-d595. Keep. `tachyonfx` 0.25.2 (2026-09-06, 507,235 downloads,
  ratatui-core 0.1.2; adds `bon`, `compact_str`, `web-time`) is for fades and sweeps; nothing in the area animates but the spinner. Keep.
- **Truncation with `…`** (`rows::truncate`): ratatui clips and has no ellipsis (rg `ellipsis` in ratatui-core/-widgets: no hit); `unicode-truncate` (v3.0.0, 2026-08-20, 63,983,740 downloads;
  2.0.1 is already in `Cargo.lock` through ratatui-core) cuts to a width and pads, and also has no ellipsis. Keep the 12 lines (they count columns; one caveat:
  it sums per-`char` widths, so a ZWJ emoji sequence would be over-counted: unverified, not run).
- **Right-aligned counters** (`rows.rs:526-530`): already `Line::right_aligned()`.
- **The scroll window and cue logic** (`rows.rs` `Scroll`/`view`/`scroll_to`, `widgets/column.rs::window/max_scroll`, `inbox::Shape`, `dialog/scroll.rs::Scroll`): four code paths, but each differs in a way
  upstream does not cover: ratatui's `ListState`/`ScrollbarState` hold an offset only and have no "▲ n more" row whose presence changes the number of rows left
  (`max_scroll` subtracts the cue row); `tui-scrollview` 0.6.8 renders a large buffer and clips it (no cues). Keep. (`rows::Scroll::wheel` and `dialog::scroll::Scroll::by` are alike, but
  `max` differs by the cue row; merging them is an internal refactor, not an upstream replacement.)
- **Right-aligned scroll cue as text rather than a Scrollbar:** a User decision (hs-d348); `tui-scrollbar` 0.2.8 / ratatui `Scrollbar` draw a bar. Keep.
- **`fold.rs` and `section.rs` building `Line`s** (pure functions, hs-d595); the fold has no upstream (`tui-widgets` 0.7.12 has no fold; `ratatui-interact` has an `accordion` with its own state and colours).
  The pill and box assembly in `section.rs::row` is Job pill-spec / box-widget: see pill-spec, see box-widget.
- **`tree/slug.rs`**: derives an id by the contract's own pattern (`^[a-z][a-z0-9-]{1,39}$`); a slug crate would not know the pattern or the `-2` suffix rule. Keep.
- **`tree/actions.rs`, `ask.rs`, `view.rs` effects**: Elm-shaped `Effect`s (hs-d595); the commands go through `Tmux`/the runner, not terminal drawing. Nothing upstream. Keep.
- **`Clear`** in `menu.rs:166`: the ratatui widget, used right.
- **`Layout::vertical` in `View::draw_at`** and `Margin`: used already; no manual column arithmetic found beyond the cases above.
- **`palette.rs`, `palette/colours.rs`**: indices by number (hs-d534), `Color::Indexed`; no upstream palette fits the rule "no hex". `dot_span` etc. keep. (`pill_*`: see pill-spec.)
- **`updating_label.rs`**: a test file only (red tests hs-d532 for the PR box/legend); no mechanism to audit.

## Not checked

- `tree/view.rs`: read the draw/click/footer paths (lines 560-770); the keyboard handling (lines 1-560) and the 2,400 lines of tests were not read line by line (searched for the workaround words only).
- `tree/actions.rs`, `tree/ask.rs`, `tree/mod.rs` (the loop, the reader thread: Plan sync's): searched for workaround words and read the signatures, not every function.
- `tree/look.rs`, `tree/margin_tests.rs`, `counter_tests.rs`, `fixing_tests.rs`, `inbox_indent.rs`, `actions_by_user_tests.rs`, `fixture.rs`: tests and fixtures; used as evidence, not audited.
- `tree/kinds.rs`, `tree/cursor.rs`: skimmed only.
- `palette.rs` (798 lines): read the spinner/dot/frame parts; the colour tables were not compared against the 16-colour rules (Job colour-audit exists: `docs/research/colour-audit.md`).
- `pace.rs` `Reads` (reader thread, newest-wins queue) and `wait_for_next_frame` (redraw rhythm): Plan sync (hs-p62) owns the read path and how often views redraw: see Plan sync.
- The off-screen 1x1 render in `rows.rs:502`: see Job tree-scroll. Pill caps/assembly and box spacing: see Job pill-spec, see Job box-widget.
- crates not read beyond name, version and dependency list: `tui-popup` 0.7.7 (a centred popup, not a click-placed menu), `tui-overlay` 0.1.2 (2026-04-07, 3,353 downloads; anchored overlays/toasts), `tui-widgets` 0.7.12, `ratatui-flow`, `ratatui-textarea`, `ratatui-toaster`, `ratatui-image`.

## Summary for the User

- F1 (replace): cell widths counted in `char`s in 6 places; a wide-character Inbox label made a 41-column row in a 30-column pane (run), `wrap` leaves a 20-column row in 10 (run). Fix: use `rows::width`. ~25 lines, no dependency. Highest value: the only user-visible bug.
- F2 (replace): the tree uses `tui-tree-widget` for state only and undoes its drawing (empty node symbols, cells shifted back, offset forced); own `Vec<Line>` + `Paragraph::scroll` removes ~60 net lines and, with Job tree-scroll, the 1x1 render. Medium risk; dropping the crate is the User's call.
- F3 (replace): the muting/asleep colours and the counters are applied to the buffer after drawing by sniffing `╭├╰│─` in the first 3 columns; style the `Line`s when built instead. Low-medium risk; do with F2.
- F4 (replace, low): the scroll cue pads by hand; `Line::right_aligned` exists (and is already used for the counters). Do with F1.
- F5 (replace, low): the menu clamp and inner hit-test equal `Rect::clamp` and `Block::inner`. 6 lines.
- F6 (keep, needs a decision): the error footer's height is `div_ceil`, the text word-wraps; the exact count (`Paragraph::line_count`) needs the unstable feature. Unverified that a row is cut.
- Kept with evidence: OSC 8 links (ratatui's own tests use the same trick), the spinner clock (`throbber-widgets-tui` is stateful), the text scroll cues and window code (no upstream cue rows), the context menu (`ratatui-interact`'s is the same shape plus `arboard`, `regex`, `termimad`), truncation with `…` (no upstream ellipsis).
