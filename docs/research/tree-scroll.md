# Tree scroll: why `tree/rows.rs::draw` renders into a 1x1 buffer, and how to drop it

Research Job `tree-scroll` (Plan `ui-widgets`), Attempt `tree-scroll-a1`. Base `c2953181` (`origin/main` already holds `Invert pill and selection text`, so it is the newest code). Crates read in `~/.cargo/registry/src/*`: `tui-tree-widget-0.24.1` (the version in `Cargo.lock`), `ratatui-0.30.2`. Nothing changed but this file; the two scratch experiments (section 4) were reverted before the commit.

## Summary

- The off-screen render exists for one reason: `TreeState::scroll_down` clamps to `last_biggest_index`, and the only writer of that field is `Tree::render`. Without the 1x1 render the clamp uses the previous frame's row count.
- **Dropping only that line is wrong**: one frame draws the wrong rows. Verified with a scratch test (section 4): green with the line, red without it. The 262 existing `tree::` tests all stay green without the line, so **no existing test guards it**.
- **Recommendation: option (a)**. Stop rendering `tui-tree-widget`'s `Tree`: `draw` paints the visible rows itself from its own flat list, and the View keeps `TreeState` only as the open/selected store. The widget's render also records what a mouse click resolves against (`rendered_at`), so the click geometry has to move into a value `draw` returns.
- There is no red test for the pure refactor. Stage it: a test PR first (green, shown red against the naive drop), then the refactor PR with those tests unchanged.

## 1. The reason

`tui/src/tree/rows.rs:502-505` (read at `c2953181`):

```rust
StatefulWidget::render(widget(), Rect::new(0, 0, 1, 1), &mut Buffer::empty(Rect::new(0, 0, 1, 1)), state); // 502
state.scroll_up(usize::MAX);                                                                              // 503
state.scroll_down(win.first);                                                                             // 504
f.render_stateful_widget(widget(), rows, state);                                                          // 505
```

The comment above it (500-501) says the widget "clamps to what the last render saw".

Verified against `tui-tree-widget` 0.24.1:

| Fact | Source |
|---|---|
| `TreeState.offset` is private (`pub(super)`); only `scroll_up` and `scroll_down` write it, besides `render`; there is no setter | `tree_state.rs:24`, `:312-316`, `:322-328`; `get_offset` (`:58`) only reads |
| `scroll_down` clamps the new offset with `.min(self.last_biggest_index)` | `tree_state.rs:322-328` |
| `last_biggest_index` is written in one place: `Tree::render`, from the flattened length, before any drawing | `lib.rs:189`; the other uses (`tree_state.rs:194,233,267`) only read it |
| `Tree::render` also sets `last_area` (`lib.rs:182`), clears and refills `last_rendered_identifiers` (`:183`, `:324`), sets `last_identifiers` (`:327`), clears `ensure_selected_in_view_on_next_render` (`:237`) and clamps `offset` to `last_biggest_index` (`:205`) | `lib.rs` |
| Every `select` sets `ensure_selected_in_view_on_next_render` (`tree_state.rs:99`); the next render then moves the offset so the selected row is in the area (`lib.rs:195-236`) | `tree_state.rs`, `lib.rs` |
| `TreeItem.text` is `pub(super)`: a flattened item's `Text` cannot be read from outside the crate | `tree_item.rs:37-40` |

So `draw` owns the scroll (`Scroll`, `view`, `window`, `scroll_to`, `max_scroll`, `rows.rs:431-473`) and has to push its offset into the widget. `scroll_up(usize::MAX)` zeroes the offset; `scroll_down(win.first)` sets it, but clamped to `last_biggest_index`. The 1x1 render refreshes `last_biggest_index` to this frame's length (1x1 is a non-empty area, so `lib.rs:184` does not return early) before the clamp. Side effects of that render, all overwritten by the real render on the next line: `last_area`, `last_rendered_identifiers`, `offset`. It also clears the ensure-selected flag, so the real render does not re-scroll (the flag is set by every `select`).

### Commit `1e12a94b` (PR #143)

`git log -S'Rect::new(0, 0, 1, 1), &mut Buffer::empty' -- tui/src/tree/rows.rs` gives only `1e12a94b`, "Inbox: waiting state, team review from Slack, errors to the Manager (#143)", 2026-10-20, a squash of many commits. Its message says nothing about the 1x1 render or the clamp; the scroll lines are "WIP: the tree and the Inbox share the column; the tree scrolls with cues (hs-d307)" and "TUI: the tree and the Inbox share the column, two thirds and one third, each scrolling with cues (hs-d307); docs §13.2, §17". In the same commit `rows.rs::draw` changed from "render the widget at `area`, then read `state.get_offset()`" to "own `Scroll`, cue rows, then push the offset into the widget". The only explanation is the code comment above.

Tests added in that commit that touch the scroll (names as in `tui/src/tree/view.rs` today):

- `a_tree_taller_than_its_rows_scrolls_to_keep_the_cursor_visible_between_cues` (`view.rs:2052`)
- `the_tree_scrolls_with_the_wheel_three_rows_a_notch_between_cues_and_clamps` (`:2704`)
- `the_wheel_scrolls_the_section_under_the_pointer_and_a_cue_row_is_no_hit` (`:2727`)
- `the_keyboard_cursor_scrolls_the_tree_to_stay_between_the_cues` (`:2742`)

None of them fails without the 1x1 render (section 4).

## 2. Every use of `TreeState` / `TreeWidget` in `tui/src`

Found with `rg -n 'TreeState|TreeWidget|tui_tree_widget|rendered_at|click_at' tui/src` plus the method names. "Needs render" = reads something only a `Tree::render` has recorded. Line numbers at `c2953181`.

| file:line | call | needs a render? |
|---|---|---|
| `rows.rs:50` | `use tui_tree_widget::{Tree as TreeWidget, TreeItem, TreeState}` | n/a |
| `rows.rs:375,411` | `TreeItem::new_leaf`, `TreeItem::new` (`context_items`, `items`, `spacer` `:405`) | no |
| `rows.rs:452,485,571` (and `view.rs:602`) | `state.flatten(&items)`: reads `opened` only | no |
| `rows.rs:452,484,562-563,571` (and `folds.rs:33,46,48`, `view.rs:601,602`) | `state.opened()`, `state.open(..)`, `state.close(..)` | no |
| `rows.rs:488,548,583,591`; `view.rs:324,387,469,472` | `state.selected()` | no |
| `rows.rs:586,604`; tests `rows.rs:648`, `counter_tests.rs:31`, `fixing_tests.rs:35`, `view.rs:941,1001,1158,1220,2055` | `state.select(..)`, `state.close` | no (sets the ensure flag, which only a render reads) |
| `rows.rs:613` | `state.toggle(path)` | no |
| `rows.rs:499` | `TreeWidget::new(&items).expect(..).node_*_symbol("")` | no |
| `rows.rs:502` | `StatefulWidget::render(widget(), 1x1, ..)` | **is** a render |
| `rows.rs:503` | `state.scroll_up(usize::MAX)` | no (`saturating_sub`) |
| `rows.rs:504` | `state.scroll_down(win.first)` | **yes**: clamps to `last_biggest_index` (`tree_state.rs:327`) |
| `rows.rs:505` | `f.render_stateful_widget(widget(), rows, state)` | **is** the render: records `last_area`, `last_rendered_identifiers`, paints the rows |
| `view.rs:702` | `self.state.rendered_at(at)` (`row_at`) | **yes**: `last_area` and `last_rendered_identifiers` of the real render at `:505` |
| `view.rs:705` | `self.state.rendered_at(Position::new(at.x, y - 1))` | **yes**, same |
| `view.rs:133,231` | the `state: TreeState<Id>` field and its `default()` | n/a |
| `rows.rs:642-648` | test helper `state_for` (`default`, `close`, `select`) | no |

Never used in `tui/src`: `click_at`, `key_up/down/left/right`, `scroll_selected_into_view`, `get_offset`, `select_first/last/relative`, `toggle_selected`. The keyboard cursor is `rows::move_cursor` over `visible()` (`rows.rs:569-605`), not the widget's.

### How a click on a row is resolved (`view.rs`)

`click` (`:633`) and `open_menu` (`:524-525`) call `row_at` (`:701-707`):

1. `state.rendered_at(at)` answers only if `at` is inside `last_area`, i.e. the `rows` rectangle of the **last real render** (the list area minus the cue rows; `rows.rs:497`), and returns the last rendered row whose `y <= at.y` (`tree_state.rs:274-283`: below the last row it answers the last row).
2. `rows::row(&tree, &path)?` drops a spacer row (an empty row between Contexts has no `row`).
3. `rendered_at(at.x, at.y - 1) != path` removes the "below the last row" answer of step 1.

So a click **does** depend on the widget's render having recorded the rows. It never needed the 1x1 render, only the real one. The real render's `last_area` is what makes the cue rows, the margins and row 0 "no hit" (`the_wheel_scrolls_the_section_under_the_pointer_and_a_cue_row_is_no_hit`, `row_0_is_blank_and_the_rows_follow_one_lower_and_clicks_land_on_the_row_under_the_pointer`, `a_click_on_the_first_drawn_cell_of_a_row_acts_on_that_row_and_the_margins_are_no_row`, `a_click_below_the_last_row_does_nothing`). The wheel uses `tree_area` (`view.rs:762`), not the widget.

## 3. Options

Context for all: the tree has only height-1 rows (`Line` texts: `rows.rs:393-406`); `draw` already **undoes** the widget's indentation: the widget indents a child by `depth * 2` columns and `draw` copies each row left by `shift` (`rows.rs:518-523`, `INDENT = 2`); the node symbols are set to `""`, there is no highlight symbol or style (the default is `""`/`Style::new()`, `lib.rs:102-103`), no block, no scrollbar. So the widget contributes: `flatten` (already called by `draw`), the offset window, the indent and `text.render`, and the click record.

### (a) Draw the rows ourselves; `TreeState` only stores `opened` and `selected` (recommended)

- **What changes.** `items()` stops returning `TreeItem`s and returns the flat visible rows `Vec<FlatRow { path: Vec<Id>, line: Line<'static> }>` (the same open rule as `tui-tree-widget`'s `flatten`: a Context's children show iff the Context's path is in `opened`; the closing row and the spacer are top-level siblings; `flatten.rs:26-60`). `draw` takes `flat[win.first..win.first + shown]`, renders each `line` at `Rect::new(area.x, y, area.width, 1)` (no widget indent, so the shift/copy loop at `:518-523` goes), and **returns** a `Hits { rows: Rect, paths: Vec<Vec<Id>> }` (the paths drawn, one per row of `rows`). `View` stores it (as it already stores `tree_area`, `pane`, `inbox_area`) and `row_at` becomes `rows.contains(at).then(|| paths[at.y - rows.y])` filtered through `rows::row`: the step-3 "above" check goes, because a row below the last is simply outside `paths`. `row_count`, `visible` and `view::free_for` read the same flat list instead of `state.flatten(&items(..))`. `TreeWidget`, `StatefulWidget`, `Buffer` imports, `scroll_up/down` and both renders go.
- **What breaks / must be kept.** (i) `TreeItem.text` is private, so the Lines have to come from cpd's own functions (`context_text`, `plan_row`, `fork_row`, `fold::*`: they already produce `Line`s before they are wrapped in `TreeItem`). (ii) Row text is fitted to the width with `depth * INDENT` already counted as used (`rows.rs:334`, in `row_text`). Keep that line unchanged in the refactor so the frames stay identical; dropping it is a later, visible change (2 columns more room), not part of this one. (iii) The widget clipped a row to `width - indent`; `draw` rendering at `width` could show up to 2 more columns of a too-long Line. `row_text` truncates to `width - RIGHT_PAD`, so this should not happen: **unverified**; the frame-equality test (section 5, test 3) checks it. (iv) The ensure-selected flag is no longer consumed by any render; nothing else reads it. (v) The `last_*` fields stay stale: nothing reads them.
- **Risk to views' behaviour.** Low to medium: the frames must be identical (test 3) and the click geometry moves from the widget to the View (tests 1, 2, 4). The dependency stays (`TreeState`, `TreeItem` is no longer needed). It matches hs-d596: no `StatefulWidget` render, the state (`Scroll`, hits) lives in the View, `draw` is a function of `(tree, state, scroll, area)`.

### (b) Feed the widget only a window of rows

- **What changes.** Build `TreeItem`s only for rows `first..first+shown` and render them with offset 0.
- **What breaks.** The widget's tree is hierarchical (children only under an open parent, depth by nesting). A window that starts inside a Context has no parent item to nest under, so the depth, and with it the indent that `draw` removes, has to be faked; the open set would need rewriting per frame. `last_biggest_index` would be the window length, so `scroll_down` is moot, but `last_rendered_identifiers` would be right. It still renders through a `StatefulWidget` and still writes state during render, against hs-d596.
- **Risk.** High, with no gain over (a). Rejected.

### (c) Another `tui-tree-widget` version or another crate

- `tui-tree-widget` on crates.io (`curl -s -A cpd-research https://crates.io/api/v1/crates/tui-tree-widget`, 2026-10-23): max version 0.24.1 (2026-08-09, the one cpd has), then 0.24.0 (2026-01-09), 0.23.1 (2025-05-29), 0.23.0, 0.22.0, 0.21.0; about 1.98 M downloads; repo `EdJoPaTo/tui-rs-tree-widget`. **No newer version exists**, and 0.24.1 has no offset setter (read in `tree_state.rs`: `offset` is only written by `scroll_up`, `scroll_down`, `render`). I did not read the sources of 0.24.0 or older (unverified whether an older one exposes a setter; it would be a downgrade anyway). There is no changelog file in the 0.24.1 package (`ls` shows `README.md`, `src`, `examples`, `benches`).
- Alternatives: `crates.io` searches `ratatui tree`, `ratatui tree view`, `ratatui treeview widget` (2026-10-23) show `ratatui-tree`, `ratatui-directory-tree`, `ratatui-collections` (version 0.0.0, 20 downloads, "Reserved for ..." placeholders: do not use), and `ratatui-interact` 0.5.3 (2026-04-02, 79 k downloads, "Interactive TUI components for ratatui with focus management and mouse"), `ratkit` 0.2.18. **I did not read the source of any of these**: whether `ratatui-interact` or `ratkit` has a tree with an external offset is **unverified**, and a component library with its own focus and mouse model would contradict hs-d595/hs-d596 (no Component interface, state in the View). I name none as a verified alternative. A patch to `tui-tree-widget` upstream (an `offset` setter) would fit but is a new dependency state (a fork or a release we wait for); not proposed.

### (d) Anything else

- **d1. Keep the widget, replace the 1x1 render with an empty real render**: still two renders per frame (the first one clamps and paints, the second repaints); same state write during render. No gain.
- **d2. Keep the widget, never `scroll_down`; let the widget find the offset itself** (`scroll_selected_into_view` plus its own offset): the View's scroll (cue rows, wheel, `followed` key) is independent of the selection by design (`Scroll` doc, `rows.rs:431-441`), and the widget's offset is private, so the cues would disagree with the rows. Rejected.
- **d3. Compute the offset against the current length ourselves and bypass the clamp**: impossible; the clamp is inside `scroll_down` and `offset` is private (section 1).
- **d4. Own state struct instead of `TreeState` as well** (drop the crate completely): possible after (a), since only `opened`/`selected`/`open`/`close`/`toggle`/`select` are used. It is a larger change (`folds.rs`, every `view.rs` test that touches `view.state`, `Cargo.toml`, `docs/implementing.md:145,161`) and is not asked for; see open question 1.

### Recommendation: (a)

Exact functions to change (all `tui/src/tree/`):

- `rows.rs`: `items` and `context_items` and `spacer` (return flat rows with their `Line`s instead of `TreeItem`s); `row_count`, `visible`, `draw` (no widget; return `Hits`); remove the `TreeWidget`, `StatefulWidget`, `Buffer`, `Cell` (if unused) imports and `INDENT` use in the shift loop (keep `INDENT` for `row_text` at `:334`). Keep `Scroll`, `view`, `window`, `scroll_to`, `Scroll::wheel`, `click_toggle`, `open_new`, `reselect`, `selected_index`, `move_cursor` unchanged.
- `view.rs`: `View` gets a `hits: rows::Hits` field next to `tree_area`; `draw_at` (`:760`) stores what `rows::draw` returns; `row_at` (`:701`); `free_for` (`:601-602`) uses the flat list.
- No change: `folds.rs`, `mod.rs`, `Cargo.toml` (`TreeState` stays).

## 4. What I ran (scratch, reverted)

1. In `rows.rs`, only line 502 removed (`scroll_up`/`scroll_down` kept): `timeout 900 scripts/cargo.sh nextest run -p cpd-tui tree::` gave **262 passed, 0 failed**. No existing `tree::` test covers the clamp.
2. A scratch test in `view.rs` `mod tests`, then deleted (`unverified` only in the sense that it is not committed; it ran as described):

   ```rust
   fn plans_of(ctx: &str, n: usize) -> ContextNode { /* n Plans `<ctx>00..`, each with an idle Lead (fixture::plan, session) */ }

   #[test]
   fn scratch_rows_grow_above_a_cursor_that_did_not_move() {
       let mut view = View::new(tree(vec![plans_of("bb", 25)]), true);
       for _ in 0..24 { press(&mut view, KeyCode::Down); }          // cursor on bb24, the last row of 27
       let _ = draw_now(&mut view, 43, 24);
       view.refresh(tree(vec![plans_of("aa", 40), plans_of("bb", 25)])); // 42 rows now above the cursor
       let grown = text_rows(&draw_now(&mut view, 43, 24));
       let again = text_rows(&draw_now(&mut view, 43, 24));
       assert_eq!(grown, again, "the frame the rows grew in is the next one");
   }
   ```

   Result: **passes** with the 1x1 render; **fails** without it. The failing frame's rows are `aa24 … aa39` and two Context rows (the widget's offset clamped to the old `last_biggest_index` 26, so it drew the wrong rows) under a cue that says `▲ 50 more` (our offset): the cue and the rows disagree, and the overlay loop (`rows.rs:513-`) styled and indented the wrong rows. The next frame is right. (A first version of the scratch test assigned `view.tree = ..` instead of `refresh`: the new Contexts were not open, so it proved nothing; `refresh` is the real path, `view.rs:277`.)

   Why the existing tests miss it: the cursor tests all `select` first, and `select` sets the ensure-selected flag, which makes the widget find the row by itself; the wheel tests never grow the rows between two frames. The failing case needs the rows to grow **while the selection stays put** (a snapshot adds rows above the cursor).

## 5. Test plan

Existing tests that cover the behaviour (all in `tui/src/tree/`, green at the base):

- scroll with cues and the cursor following: `view::tests::a_tree_taller_than_its_rows_scrolls_to_keep_the_cursor_visible_between_cues` (`:2052`), `the_keyboard_cursor_scrolls_the_tree_to_stay_between_the_cues` (`:2742`), `a_long_tree_and_a_long_inbox_share_thirty_rows_twenty_and_ten`, `a_long_tree_and_a_short_expanded_inbox_give_the_rest_to_the_tree`, `a_short_tree_and_a_long_inbox_give_the_rest_to_the_inbox`;
- the wheel: `the_tree_scrolls_with_the_wheel_three_rows_a_notch_between_cues_and_clamps` (`:2704`), `the_wheel_scrolls_the_section_under_the_pointer_and_a_cue_row_is_no_hit` (`:2727`);
- clicks and hit rows: `row_0_is_blank_and_the_rows_follow_one_lower_and_clicks_land_on_the_row_under_the_pointer` (`:1056`), `a_click_on_the_first_drawn_cell_of_a_row_acts_on_that_row_and_the_margins_are_no_row` (`:1123`), `a_click_below_the_last_row_does_nothing` (`:1264`), `a_click_folds_a_context_and_opens_a_session_and_hides_the_cursor` (`:998`), `a_click_on_a_context_title_opens_its_manager_and_only_its_closing_row_folds`, `a_right_click_opens_the_menu_of_the_row_or_the_empty_area`, `the_menu_is_clamped_inside_the_inner_area_and_a_click_on_a_margin_is_outside_it`;
- the drawn rows (glyphs, guides, counters, muting, selection): the `rows::tests` module (`render`/`render_at` draw into a pane 40 rows taller than asked, so no cue row appears: `rows.rs:652-664`), `margin_tests.rs`, `counter_tests.rs`, `fixing_tests.rs`.

What is missing:

1. Rows that grow while the selection stays put (the case above).
2. A click that names **which** row a scrolled tree answers: `the_wheel_scrolls_..._cue_row_is_no_hit` only matches `Effect::Run(Action::Open(_))`, so a click landing on the wrong row (an off-by-offset) is not caught.
3. A frame-for-frame comparison of a scrolled tree with folds, an archived Context and a spacer, styles included (the `rows::tests` never scroll; the `view` scroll tests compare text only).
4. The `Hits` after a wheel: a click on the row under a cue, on the last row of a window, and just below a short tree.

The red test. **No test can be red on the refactor itself**: `draw` is a pure refactor and today's behaviour is already correct. The closest: `tree::scroll_tests::rows_that_grow_above_a_cursor_that_did_not_move_draw_the_same_frame_twice`, the scratch test of section 4. It is green today and red under the naive drop of line 502 (verified). It pins the contract ("the frame in which the rows change is the right frame"), not the implementation. The staging therefore is:

- **PR 1 (tests)**: tests 1-4 above, all green on `c2953181`. The PR description shows test 1 red with line 502 removed (the experiment of section 4, repeated) so it is known to guard.
- **PR 2 (refactor)**: option (a); the PR 1 tests must stay green and unchanged; no test is edited to make it pass (the existing `view::tests` and `rows::tests` already pin the frames and clicks).

Names (all new, in a new `tui/src/tree/scroll_tests.rs`, registered like `margin_tests`):

1. `rows_that_grow_above_a_cursor_that_did_not_move_draw_the_same_frame_twice`: the test of section 4 (asserts the frame drawn after `refresh` equals the next one, and that the cursor's row `bb24` is on screen between the cues).
2. `a_click_on_a_scrolled_tree_opens_the_session_drawn_under_the_pointer`: `long_tree(60)`-like fixture, wheel down twice, draw, click each row of the window and compare with `Action::Open(sid(<the Lead drawn on that row>))`; assert the first row under the ▲ cue and the last above the ▼ cue.
3. `a_scrolled_tree_with_folds_and_spacers_draws_the_same_cells_as_the_widget_did`: draw a tree (live Contexts, an archived Context folded and unfolded, spacers) at several heights and wheel offsets and compare the buffer cells (`symbol`, `fg`, `bg`, modifier), reading the expected cells from a **frozen** reference: before the refactor the test records them as a plain-text-plus-style table in the test file (the repo's rule is no `insta`; the table is a literal). This is the one test whose expected value is generated from the old code; it is generated once, in PR 1, and reviewed.
4. `the_click_geometry_follows_the_scroll_and_the_cues_and_the_tail_are_no_row`: cue rows, row 0, margins, spacers and the rows below a short tree answer `Effect::None`; the wheel moves what the same coordinates mean.

## 6. Job split (each one Attempt, about 150 turns)

1. **`tree-scroll-tests`** (tests, PR 1). `touches`: `tui/src/tree/scroll_tests.rs` (new), `tui/src/tree/mod.rs` (one `mod scroll_tests;` line, with `#[cfg(test)]` like its neighbours). Acceptance: tests 1-4 pass on the base; test 1 is shown red by deleting `rows.rs:502` in a scratch (not committed). Size: small (about 150 lines of test; fixtures exist in `tree/fixture.rs` and `view.rs` helpers `long_tree`, `press`, `click`, `wheel`; these are in `view.rs` `mod tests`, so the new file may need its own copies, or `pub(super)` on the helpers: that is a `view.rs` change, to list in `touches` if chosen).
2. **`tree-scroll-draw`** (implementation, PR 2, after 1 is merged). `touches`: `tui/src/tree/rows.rs`, `tui/src/tree/view.rs`. Acceptance: tests of Job 1 and all existing `tree::` tests green; `rg 'TreeWidget|StatefulWidget|Rect::new\(0, 0, 1, 1\)|rendered_at' tui/src` is empty. Size: medium (the flat list and `Hits` are about 120 changed lines in `rows.rs`, 20 in `view.rs`).
3. **`tree-scroll-docs`** (docs, with or after 2). `touches`: `docs/implementing.md` (line 161 says the dependency is used for "the `tree` view's collapsible ... tree"; it would say it is only the open/selected store), `docs/research/tree-scroll.md` (note the outcome). Nothing in `docs/commands.md` or `CONTEXT.md` mentions the widget (`rg -i 'tui-tree-widget|TreeState|1x1' docs/*.md CONTEXT.md` found only `implementing.md:145,161`). Size: tiny; may be folded into Job 2.

## 7. Open questions only the User can answer

1. After (a) the crate is used only for `TreeState` (`opened`, `selected`, `open`, `close`, `toggle`, `select`). Keep it as that store (recommended: nothing else to change, `Cargo.toml` untouched) or replace it with a small struct of our own and drop the dependency (follow-up Job; `docs/implementing.md` forbids changing dependencies in later Jobs, so it needs a go-ahead)?
2. `row_text` counts `depth * INDENT` as used width (`rows.rs:334`) because the widget used to indent; after (a) rows could use those 2 columns. Keep the frames identical in this Plan (recommended) or take the 2 columns in the same PR (a visible change to the user's tree)?
