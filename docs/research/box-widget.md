# The box widget and its spacing layout test (Job `box-widget`, Plan `ui-widgets`)

Research only. Read on `origin/main` c2953181 (it holds `Invert pill and selection text`, #291, so no branch was read). Every `file:line` is that commit. Ratatui facts come from `ratatui-widgets-0.3.2` and `ratatui-core-0.1.2` source or from `/tmp/cpd-building-cpd/ratatui/`. Anything else is labelled `unverified`.

Scratch experiments: I added throwaway `scratch_*` tests that print the rows of each box at several widths, ran them (`scripts/cargo.sh test -p cpd-tui --lib scratch`), and reverted them before the commit; nothing of them is in the tree. Rows below are cells printed with `·` for a blank. The two pill caps U+E0B6/U+E0B4 are invisible in my terminal, so a pill's text sits against its `─`.

## Summary

1. All three deviations `docs/research/box-rule-audit.md` found in Rust are **closed** on main: the PR boxes' margin (`CHART_MARGIN`, plan/mod.rs:79, #288), the Deploy column's menu (widgets/mod.rs:170 and :184 pass `pane.inner(Margin::new(1, 0))`, #272) and the logs popup's right margin (logs/view.rs:175, #272). Each has a test (table 1).
2. What is **still open** is narrow widths (measured, section 5) and that the spacing is still applied by hand at every site. Prose became tests per site; no test or type stops a ninth box being drawn wrong.
3. Design: one `BoxWidget` struct (`impl Widget`) over `ratatui::widgets::Block` for full-wall boxes, with a rail variant for the Inbox; one small `SectionTitle` widget; one `spacing` module that holds every number. The widget is given its **wall rect** (the box's outer edge); the horizontal margin is the container's, so the widget cannot guarantee it: the layout test pins it per container.
4. `plan::MARGIN` does **not** break the rule (section 4).
5. The layout test is one table over every box in `tui/src/layout_tests/`, reaching each box through a `#[cfg(test)] pub(crate)` sample function in the box's module. It is red today for narrow widths only.

## 1. Inventory

Spacing is "measured" when a pinned test names the rows, or "scratch" when my deleted scratch test printed them. `m/p` = margin/padding in columns; `v` = blank rows inside the box at top/bottom. Rule: hs-d578 (m 1, p 1, v 0), hs-d476 (a blank row above and below only a fold and a section title; a section's body none).

| # | Box (site) | What it draws | Inputs today | Spacing, measured | Verdict |
|---|---|---|---|---|---|
| 1 | Input box: `input_box.rs` `lines` :43, `rule` :37, `section::box_top` :51 | full box, rounded, label pill on the top border | `label, text: &[char], cursor, focused, width` (a `Vec<Line>`, no widget); drawn by `dialog/view.rs:365` | m 1 (`SIDES` = 6, input_box.rs:9, the margin is drawn *inside* the width), p 1, v 0. Pinned: `input_box_tests.rs::input_box_draws_the_label_the_borders_and_the_text_inside_the_margins`. Scratch width 7: `·╭───╮·`, `·│·a·│·`; width 6: `·│·a·│` (right margin gone) | ok from 7 up; **open below 7** |
| 2 | Dialog frame: `dialog/view.rs` `layout` :158, `frame_top` :169, `frame_bottom` :193, `frame_wall` :199, `Dialog::draw` :341; popup size `dialog/mod.rs` `popup_size` :36 | full box, rounded, title pill on the border, blank row, body, blank row, pill buttons | `Dialog { title, text, mode, scroll, area }` (state; `area` = last frame's rect, view.rs:251, for hit-testing) | popup = the frame exactly, 1 column margin from the client by the clamp `client.0 - 2` (mod.rs:40); p 1 (`body = sub(2, 2, width - 4, …)`, view.rs:163); rows: top, blank, body, blank, buttons, bottom. Pinned: `view_tests.rs::the_rows_are_the_title_border_a_blank_row_the_body_a_blank_row_the_buttons_and_the_closing_border`, `…::the_content_has_one_column_of_padding_inside_each_wall`. Scratch 44×10 input: `│·╭─Name───╮·│` / `│·│·abc···│·│` / `│·╰───╯·│` | ok at 44; **open at small widths** (section 5) |
| 3 | The dialog's input box (nested in 2) | as 1, inside 2's `inside` rect (view.rs:361) | as 1; windowed by the dialog (view.rs:362-371) | the dialog's padding column is the input's margin (outer padding = inner margin) | ok |
| 4 | Inbox: `tree/inbox.rs` `draw` :342, `item_line` :311, `cue_line` :333; `fold::content`, `fold::toggle` :58, `fold::plain` :65, `fold::done` :71; `section::collapsible_title` :45 | rail-only fold: `╭─(Inbox)───(n)`, rows `│ glyph label age`, scroll cues, closing row `╰󰬦 expand`; blank row above and below | `items, Shape{count, expanded, cap, scroll}, cursor, now, spinning` (the Shape is the View's window) | m 1 (`full.inner(Margin::new(1, 0))`, inbox.rs:347), rail, p 1, content ends W-4 (inbox.rs:348 `- 2`), v 0, blank row above/below (Shape's first and last row). Pinned: `tree/view.rs::tests::three_items_close_with_the_terminator_and_an_empty_row_below`, `…::five_items_fold_between_an_empty_row_above_and_one_below_and_the_tree_above_shares_the_row`, `tree/margin_tests.rs::the_inbox_row_ends_three_columns_before_the_panes_edge` (+ 2 more there). Scratch 40×8: `·╭─Inbox──…──4·` / `·│·󰍢·acme/api#12…·····5m···` / `·╰󰬦·expand` | ok; the closing row's glyph is against `╰` (recorded design, audit Notes 1) |
| 5 | Context fold (tree): `tree/rows.rs` `context_items` :375 (`fold::title` :396, `fold::content`, closing :400), `draw` :478, `RIGHT_PAD` :74 | rail-only fold inside a `tui-tree-widget` list: `╭glyph name`, `├─glyph row`, `╰─glyph last`/`╰󰬪` | `Tree` (the View's `TreeState` opened set, scroll) | m 1, no p on the branch rows (glyph against `├─`, recorded hs-d337/d339), counters end W-4 (`RIGHT_PAD` 2), blank row between Contexts. Pinned: `tree/view.rs::tests::column_0_and_the_last_column_are_blank_on_every_row`, `tree/margin_tests.rs::a_context_block_row_is_cut_three_columns_before_the_panes_edge`. Scratch 43×20: `·╭󰽢·alpha` / `·├─󰽤·ship····2⋅1⋅1⋅0⋅5` / `·╰󰬪` | ok at 43; **open at width 12** (section 5) |
| 6 | Section title, non-collapsible: `section::title` :37 (Deploy column `PRs` via `column.rs:225` `render_header`, legend `legend.rs:275`, logs `logs/view.rs:173`) | a rule with a pill, no wall: ` ──(Title)────(n) ` | `title, count, width` (a `Line`) | m/p 1 cell each side (`PAD`, section.rs:32); blank row above and below made by each caller's layout: `column.rs` `Layout::vertical([Length(1), Length(2), Length(1), Length(1), Length(1), Min(1)])`, `logs/view.rs:17` `HEADER_ROWS = 3`, `legend.rs:47` `HEADER_ROWS = 2`. Pinned: `column.rs::tests::the_prs_title_has_one_empty_row_above_and_one_below_and_is_no_click_target`, `logs/view.rs::tests::the_title_is_a_section_title_between_two_empty_rows_and_the_events_start_under_it`, `legend.rs::tests::the_title_is_a_section_title_under_an_empty_row_and_over_one`. Scratch logs 40×8: `·──Logs──…──·` under and over a blank row | ok |
| 7 | Plan view PR boxes: `plan/boxes.rs` `layout`, `Block::bordered().border_type(Rounded).title(...)` :249, chart blitted at `rect.inner(Margin 1,1)` :257; the chart's own padding `plan/flow.rs:347-348` (`starts(1, …)`, `width = inner_w + 2`) | full box, rounded; title is text, not a pill: ` key · repo 󱦰 base · #12 open  ` | `PrNode`s, `JobNode`s, `NeedsUser`, `frame` (a `Boxes` canvas built when a snapshot arrives; pills' rects are saved on it for hit-testing) | wall rect at canvas x; canvas margin `CHART_MARGIN { 1, 1 }` (plan/mod.rs:79, applied :161 and :684): m 1; p 1 is **inside the chart buffer**, not drawn by the box; v 0. Pinned: `plan/margin_tests.rs::the_chart_boxes_are_drawn_one_column_from_the_edge_of_the_plan_view` (`·╭ api · api-repo 󱦰 main · dispatch ╮`), `plan/boxes.rs::tests::at_scroll_zero_the_first_box_starts_…`. Scratch 40×8, one empty PR: row 0 blank, `·╭·api···api-repo·󱦰·main···dispatch·╮···` / `·│·no·Jobs····│···` / `·╰───╯···` | ok (the audit's margin 2 is **closed**) |
| 8 | Tree right-click menu: `menu.rs` `rect` :88, `hit` :97, `draw_at` :163-167 (`Block::new().borders(Borders::ALL)`, plain corners); tree passes `self.pane`, tree/view.rs:763 | full box, **plain** corners `┌`, no title | `Menu { items, at, selected }` + `pane` | m 1 by the clamp (tree/view.rs:724 `inner`), p 1, v 0. Pinned: `menu.rs::tests::the_menu_is_drawn_as_a_bordered_box_over_what_is_there`, `tree/view.rs::tests::the_menu_is_clamped_inside_the_inner_area_and_a_click_on_a_margin_is_outside_it` | ok |
| 9 | Deploy column's menu: `widgets/mod.rs:170` `draw_at(f, pane.inner(Margin::new(1, 0)), frame)`, :184 `mouse(pane.inner(…), *m)` | as 8 | as 8 | scratch 30×8, click (29, 7): `················┌───────────┐·` (right margin 1; before the fix 0); click (0, 0): `·┌───────────┐` (left margin 1). **No test draws it** (it is inside the event loop, widgets/mod.rs:159-172) | ok by reading; **unpinned** |
| 10 | Legend body: `widgets/legend.rs` `body` :257 (`Margin::new(1, 0)` :258), `draw_scrolled` :272 | text, no wall | — | m 1, no p (no wall) | ok, not a box |
| 11 | Logs rows: `logs/view.rs` `draw` :168, `line` | text, no wall | `Log` | m 1 both sides (:175 `inner(Margin::new(1, 0))`), pinned `logs/margin_tests.rs::the_logs_popup_rows_keep_one_blank_column_on_each_side`. Scratch 12×6: `·00:00:01·…·` | ok, not a box |
| 12 | Plan view message row and Pulse | text rows, no wall | `plan/mod.rs:253, :257, :267` (`MARGIN.horizontal` 2) | text at column 2; see section 4 | not boxes (docs/commands.md §13.2 `plan` says so too) |
| 13 | Merge popup | a dialog of the kit (merge/mod.rs:336 `dialog::show_error_on`) | — | row 2 | ok |
| 14 | Chat placeholder: `chat.rs:35` `Block::new().padding(Padding::left(1))` | text | — | p 1 left; no wall | not a box |

Grep: the only ratatui `Block` uses in `tui/src` outside tests are plan/boxes.rs:249, menu.rs:167 and chat.rs:35. No `impl Widget for` / `impl StatefulWidget for` exists in `tui/src` (grep of `impl.*Widget for`: no match); `tui-tree-widget` is the only `StatefulWidget` in use (tree/rows.rs:478+).

### Deviations still open, from the audit

| Audit row | Status now |
|---|---|
| PR boxes margin 2 | **closed**: `CHART_MARGIN` 1 (plan/mod.rs:79); `plan/margin_tests.rs` |
| Deploy column menu margin 0 | **closed** at the call site (widgets/mod.rs:170, :184); **unpinned** (no test reaches the event loop) |
| Logs popup right margin 0 | **closed**: logs/view.rs:175; `logs/margin_tests.rs` |
| Closing row glyph against `╰`; tree `├─glyph`, `╭glyph` | open by recorded design (audit Notes 1, 2: hs-d337, hs-d339, hs-d484); the User's call, section 7 Q2 |
| pi rows | out of scope (pi has its own tests) |
| Merge popup, dialog kit | **rebuilt** by #272, rows in 1 and 2 |

New, found by me (section 5): the box rows at widths narrower than the box needs.

## 2. The widget API

Module `tui/src/box_widget.rs`, `mod box_widget;` in lib.rs. Three parts:

```rust
/// The only place the spacing numbers live (replaces section::PAD, rows::RIGHT_PAD, plan::CHART_MARGIN, dialog's `- 4`, input_box::SIDES).
pub(crate) mod spacing {
    pub const MARGIN_X: u16 = 1;   // hs-d578: outside the wall or rail, the container's
    pub const PAD_X: u16 = 1;      // hs-d578: between wall or rail and content
    pub const FOLD_MARGIN_Y: u16 = 1; // hs-d476: blank rows above and below a fold or a section title
    pub const MIN_WIDTH: u16 = 2 + 2 * PAD_X + 1; // wall, pad, one cell, pad, wall
    /// What a container lays boxes in: `area` less the margin.
    pub fn contain(area: Rect) -> Rect { area.inner(Margin::new(MARGIN_X, 0)) }
}

pub(crate) enum Walls<'a> {
    Full(BorderType),                  // Rounded for every box; Plain for the menu (Q1)
    Rail { closing: Line<'a> },        // a fold: left rail only, a closing row, blank row above and below
}

pub(crate) struct BoxWidget<'a, B: Widget> {
    walls: Walls<'a>,
    border: Style,                     // palette::muted_text() or palette::input_border(focused)
    title: Option<Line<'a>>,           // after the corner and one `─`: `pill(...)` spans for a pill, plain spans for the PR box
    body: B,                           // drawn into `inner`; a Paragraph, the chart blit, the input's text rows…
    highlight: Option<Range<u16>>,     // body rows drawn as the blue block, the padding cells included
}

pub(crate) struct Geometry { pub wall: Rect, pub inner: Rect, pub title_row: u16, pub closing_row: Option<u16> }
pub(crate) fn geometry(walls: &Walls, area: Rect) -> Geometry;   // pure; the widget and the views both call it

impl<B: Widget> Widget for BoxWidget<'_, B> { fn render(self, area: Rect, buf: &mut Buffer) { … } }

/// A section title outside a fold (hs-d476): blank row, the rule with the pill, blank row. Not a box: no walls, no body.
pub(crate) struct SectionTitle<'a> { title: &'a str, count: Option<usize> }
impl SectionTitle<'_> { pub const ROWS: u16 = 3; /* + impl Widget; `body(area)` = the rows under it */ }
```

How it is built, from the ratatui source:

- `Full`: `Block::bordered().border_type(t).border_style(border).title(title).padding(Padding::horizontal(PAD_X))` rendered into `area`; the body into `block.inner(area)`. This is what plan/boxes.rs:249-257 and menu.rs:167 do by hand today. A title is placed from `area.left() + 1` (block.rs:1097-1112 `titles_area`), so the `─` before a pill is the first span of the title line.
- `Rail`: drawn by the widget with a loop (see section 3 for why not `Block`): `╭` + the title row, `│` on each body row, the closing row `╰` + `closing`; the blank rows above and below are inside `area`; `inner` leaves the right `PAD_X` and the unwritten wall column, so content ends at W-4 when the container passed the area less its margin (replaces `RIGHT_PAD`, tree/rows.rs:74, and inbox.rs:348's `- 2`).
- Below `spacing::MIN_WIDTH` columns (or one row) the widget writes nothing and `geometry` returns `Rect::ZERO` for `inner`, which is how `Rect::inner` itself behaves when its margin is larger than the Rect (rect.rs:232-239, read). So nothing is clipped half-way (Q3).

### One struct with a variant enum, or several structs

Decision: **one struct, `Walls` enum**, plus the separate `SectionTitle`.

- Full and Rail have the same horizontal geometry (margin, wall or rail column, pad, content, pad, a wall column that is blank for a rail) and differ in three drawn lines. One `geometry` function is the single source of that arithmetic; several structs would copy it, which is the drift this Plan is meant to end.
- `closing` lives inside `Walls::Rail` and a `BorderType` inside `Walls::Full`, so a full box with a closing row, or a rail with rounded corners, cannot be written.
- A section title has no body and no walls; folding it into the same struct would make "a title with a body" representable. It shares only `spacing` with the box.

### Which spacing cannot be written wrongly by construction

| Spacing | By construction? |
|---|---|
| p 1 column each side (hs-d578), p 0 rows | **yes**: `PAD_X` is applied inside `geometry`; there is no padding field |
| wall/rail column, corners, title position (corner, `─`, pill), cap colour | **yes** |
| a nested box's margin = its parent's padding | **yes**: the child's wall rect is the parent's `inner` (the input box in the dialog: dialog `body` rect, view.rs:163, is exactly the input's wall rect, so the input's 6 `SIDES` become 4) |
| a fold's blank row above and below (hs-d476); a section title's | **yes** (`Rail`, `SectionTitle::ROWS` draw them) |
| a rail's content width (W-4) | **yes** |
| the horizontal margin of a box | **no**: it is the container's (`spacing::contain`). The dialog's popup (sized by `popup_size`, mod.rs:36-41), the menu's clamp (menu.rs:88) and the chart canvas (`with_margin`, flow.rs:527) place the wall rect themselves, so the widget cannot add the margin without moving the dialog and the canvas. The layout test pins it per container. A newtype `Placed(Rect)` only `contain` could build would force it, but the canvas and the popup must build one too: I judge it not worth it (YAGNI); unverified that no cheaper option exists |
| the dialog's two spacing rows between title, body and buttons | **no**: a vertical `Layout` of the dialog (`Layout::vertical([…]).spacing(1)`, layout.rs:546, is the ratatui tool for gaps between elements; that it can lead with a blank row is `unverified`); pinned by the test |

### Fit, box by box

- **Dialog frame**: fits. `BoxWidget<Full(Rounded)>`, `title: Some(Line::from(["─", pill(title)…]))`, body = a small widget that lays out `[blank, body, blank, buttons]` and draws the kind's content. The 4 kinds are one body widget, not 4 box variants. The buttons stay pills (atom). `Dialog.area` stays and is set from `geometry(...).wall`.
- **Input boxes** (all in the dialog): fit. `BoxWidget<Full(Rounded)>` with `border: palette::input_border(focused)`, title the label pill, body `TextRows { text, cursor, first_row }` where `first_row` is the View's scroll (today the dialog slices the rows itself, view.rs:362-371); `input_box::lines` and `section::box_top` go away. Height = shown rows + 2.
- **PR boxes**: fit, with one change outside the widget: the chart's own padding (flow.rs:347-348) must go, so that the widget's `PAD_X` is the only padding (else the box pads twice, or the widget needs `Padding::ZERO`, which is the illegal field I want to avoid). Title = `plan::boxes::title(…)` as plain spans; body = the chart blit (a `Widget` over `&Buffer`). The box draws into the off-screen canvas `Buffer` (boxes.rs:249 already renders a `Block` into one), so a `Widget` taking a `Buffer` is right. The title link rect (boxes.rs:255) uses `rect.x + title width - len`, which stays valid because the title still starts at `rect.x + 1`. Pills' rects (saved for hit-testing) are derived from `geometry(...).inner` instead of `+ 1`.
- **Right-click menus**: fit with two changes. (1) Corners: the menu is plain `┌` (menu.rs:167), every other box rounded (Q1). (2) Selection: the blue block covers the padding cells today (menu.rs:159-161 gives every span, padding spaces included, the selected style), but the widget draws the padding outside the body; hence the `highlight` field, which paints the rows' cells between the walls. The Inbox cursor row (inbox.rs:359 `set_style(row, selected)`) needs the same. Neither is a `StatefulWidget`: the selected row is a field the View passes in.
- **Inbox**: fits as `Rail`. The View keeps `Shape` (window, cues, hit rows) and passes the visible slice as the body and `closing` as `fold::toggle`/`plain`/`done`. `Fold::drawn`'s margin rows (fold.rs:112, :118) move into the widget.
- **Context fold (tree)**: does **not** fit. Its rows are items of a third-party `StatefulWidget` (`tui-tree-widget`) that owns the Rect and draws each item's line; the `╭`, `├─`, `╰─` are part of those lines (`fold::title`, `fold::content`), then counters and the cursor are painted over the buffer afterwards (rows.rs:512-555). It stays on the `fold::*` line builders (atoms/lines), with `RIGHT_PAD` replaced by `spacing`'s. The layout test covers it from outside. (Dropping the 1×1 off-screen render at rows.rs:502 is another Job.)
- **Section titles**: `SectionTitle` for the Deploy column, legend and logs; the three callers' hand-counted blank rows (column.rs Layout, `HEADER_ROWS` 2 and 3) become `SectionTitle::ROWS`.

### Hit-testing

A widget's `render` returns nothing, so it cannot hand back Rects. The documented approach (handoff §5: save last frame's Rects) stays: the View calls `geometry(&walls, area)` — the same pure function the widget calls — and saves `wall`, `inner`, `title_row` and `closing_row` for the next input (what `Dialog.area`, `inbox_area`, `Menu::rect` do by hand now). The widget keeps no state and the View keeps no widget. The toggle's hit row is `geometry.closing_row` (replaces `Drawn::toggle`, fold.rs:96).

## 3. What ratatui covers, and what the widget adds

From `ratatui-widgets-0.3.2/src/block.rs` and `block/padding.rs`, `ratatui-core-0.1.2/src/layout/`:

| Need | Ratatui | Source |
|---|---|---|
| walls, rounded or plain corners, border style, a title line | `Block::bordered`, `border_type`, `border_style`, `title` | block.rs:293, :606, :520, :366 |
| title placement | after the left border column, width less both borders; clipped to that | block.rs:1097-1112 |
| padding | `Block::padding(Padding)` only shrinks `Block::inner` (block.rs:762-790: it adds `padding.left`/`top` to `inner.x`/`y` and subtracts the sums). It draws nothing, and it is not a margin | block.rs:660, :762 |
| margin | none in `Block`. `Rect::inner(Margin)` is symmetric and returns `Rect::ZERO` when the margin is larger than the Rect | rect.rs:232-239 |
| gaps between stacked elements | `Layout::spacing(u16)`: between elements only, so not before the first or after the last | layout.rs:546; concepts__layout.md:301 |
| render guard | `Block::render` intersects with the buffer and returns on an empty area | block.rs:805-812 |

What `Block` does not do, and so what the widget adds:

1. **Horizontal margin and the nested-box rule**: `Rect::inner` exists, but nothing says "margin = parent's padding"; `geometry` does.
2. **Rail-only box with `╭`/`╰`**: `Borders::LEFT` draws `│` on **every** row of the area, first and last included, and draws no corner (corners need both adjacent borders, block.rs:885-917). `LEFT | TOP | BOTTOM` would also draw a top and bottom line. Only a custom `border::Set` with blank horizontal symbols would draw `╭ │ ╰` and nothing else, plus `title_top`/`title_bottom` for the title and closing rows; I did not try it (`unverified`), and it reads as a trick, so the rail is a short loop in the widget.
3. **The selected row's block over the padding**: `Block` has no per-row style.
4. **The minimum width**: `Block` draws a corner and a side even at width 1 or 2, the cells overlapping; it has no "nothing" below a size. The widget does.
5. **Vertical rows of a fold/section title**: no equivalent.
6. **The pill title's cap colour and the cut to the room** (`section::row`, section.rs:57-87 cuts the label and drops the pill before the line): `Block` clips the title at its area edge with no such logic. The cut stays in `section`/`pill`, which the parallel pill Job owns.

## 4. A3: `plan::MARGIN { horizontal: 2, vertical: 1 }`

Read: plan/mod.rs:77 `const MARGIN`, :78-79 (`CHART_MARGIN { 1, 1 }`, "Pulse and the message row keep `MARGIN`"). Every use of `MARGIN` (grep `MARGIN` in plan/): 

- :153 `pulse::buckets(width - 2 * MARGIN.horizontal, 0)`: the Pulse bar chart's width;
- :253 and :267: the Pulse message row and the chart's message row (`message_area.inner(Margin { horizontal: MARGIN.horizontal, vertical: 0 })`): one line of text;
- :257 `self.body = window.inner(MARGIN)`: the Pulse rows' area (`pulse::draw`, pulse.rs:115, writes bar rows into it).

The chart canvas and the PR boxes use `CHART_MARGIN` (:161, :684), whose vertical 1 is the blank row above the first box and below the last (the canvas), as the User reads it.

**Verdict: it does not break the rule.** Nothing that is a box takes `MARGIN` any more; Pulse rows and the two message rows are text rows with no wall or rail, and docs/commands.md §13.2 `plan` already says so (the **Chart** bullet: "Pulse's `MARGIN` … is unchanged, as are the chart's message row's: Pulse rows and that row are not boxes"). Its vertical 1 stays (Pulse's canvas).

Two observations, neither a rule break:

- the chart's message row is drawn at column 2 (scratch 40×8: `··no·Plan·yet`), while the box wall is at column 1 and its content at column 3 (`·│·no·Jobs`), so it matches neither; its comment at plan/mod.rs:265 ("indented like the chart's content at scroll (0, 0)") was true when both used `MARGIN` and is stale now. Whether it should align (Q4) is the User's call; the code is unchanged by this Plan unless they say so;
- the doc comment on `MARGIN` (:75-76) says "the blank space around the chart's content"; it now only describes Pulse.

In the implementation Jobs `CHART_MARGIN` becomes `Margin::new(spacing::MARGIN_X, 1)` and `MARGIN` stays.

## 5. The layout test

### Where it lives and how it reaches each box

`tui/src/layout_tests/` (`mod.rs` = the table type and the assertions; one file per area for its rows), `#[cfg(test)] mod layout_tests;` in lib.rs. Not an `insta` snapshot (A4): assertions on `(symbol, Style)` cells, like `failed_pill_tests.rs` (handoff §5).

Reach: the boxes live in private modules (`tree::inbox`, `plan`, `dialog::view`, `widgets::column` are not `pub(crate)` from the crate root: lib.rs:22-59, tree/mod.rs:29-48), so each module gets one `#[cfg(test)] pub(crate) fn layout_sample(width: u16, height: u16, case: Case) -> Sample`, the production draw path with a fixture: the view's `draw`/`draw_at` into a `TestBackend`. A `Sample` is `{ buf: Buffer, boxes: Vec<Placed> }`, where each `Placed { name, kind, rect }` is built from the **Rects the view saved for hit-testing** (`Dialog.area`, `View.inbox_area`, the menu's `rect`, the chart's pills' parents…), so the test also proves the saved Rect and the drawn box are the same thing. This is a `pub(crate)` constructor per box, not the widget's public surface, because most containers do not use the widget yet and the margin is the container's (section 2). After migration the table also gets rows that build `BoxWidget` directly with a fixture body (the geometry, no container), per variant.

### The table

`Subject { name, kind, widths: &[u16], sample: fn(u16, u16, Case) -> Sample }`, `kind` ∈ `Full`, `Rail`, `Rule`, `Text`.

| Subject (module) | kind | cases | widths |
|---|---|---|---|
| input box (dialog/view.rs) | Full | text empty, 1 row, wrapped 3 rows | 44, 20, 8, 7, 6, 4 |
| dialog frame ×4 kinds (dialog/view.rs) | Full | confirm, input, text, checklist | 44, 20, 14, 8, 6 |
| Inbox (tree/inbox.rs) | Rail | 0, 3 items, 5 folded, 5 expanded, scroll cues | 43, 24, 12, 6, 3 |
| Context fold (tree/rows.rs via `View`) | Rail (exempt rows, below) | one Context, archived Context | 43, 24, 12, 6, 3 |
| PR box (plan/mod.rs) | Full | empty PR, PR with Jobs, scrolled 0 and mid | 40, 20, 8, 4 |
| tree menu (tree/view.rs) | Full (Plain) | click at (0,0), at the last cell, in the middle | 30×8, 8×3 |
| Deploy column menu (widgets/mod.rs, needs `menu_pane`) | Full (Plain) | the same clicks | 30×8, 8×3 |
| section titles: Deploy `PRs` (column.rs), legend, logs | Rule | with and without a count | 30, 12, 4 |
| logs rows, legend body | Text | long and short lines | 40, 12, 4 |

Widths: wide, typical, "just holds the box" (7 with margins), one below, and a very small one.

### Assertions per row (`Full`, `Rail`, `Rule`, `Text`)

For every `Placed` box, row by row of its rect (cells read from `buf`):

1. **margin columns**: the `MARGIN_X` columns left and right of the rect are blank and unstyled (`Style::default()`, bg `Reset`, no modifier), on **every** row of the pane, boxes or not. (Generalises `tree/view.rs::assert_margins_blank`.)
2. **wall or rail column**: `Full`: first row `╭`/`┌`, last `╰`/`└`, body rows `│` at the rect's first and last column; `Rail`: `╭`/`│`/`├`/`╰` at the first column, nothing in the last.
3. **padding column**: the cell after the wall (and before the right wall) is blank on body rows. Exempt, by a `Case` flag that names its record: the tree's `├─glyph`/`╭glyph` rows and the closing rows `╰glyph` (hs-d337, hs-d339, hs-d484, audit Notes 1-2; Q2).
4. **content**: ends at or before `rect.right() - 1 - PAD_X - 1` (W-4 for a rail).
5. **vertical padding 0**: the first row under the top border and the row above the bottom border are not blank, unless the table names them as spacing rows (the dialog's two).
6. **hs-d476**: for `Rail` and `Rule`: the row above and below are blank **inside the pane** (unless the pane edge); for `Full` there is no such blank row between the box and what adjoins it that the box owns. No other kind has a blank row it owns.
7. **narrow**: for width < `MIN_WIDTH + 2 MARGIN_X` nothing may panic, assertion 1 still holds, and nothing is written outside the pane.
8. **title**: the top border starts `╭─` then the pill's left cap (when the title fits).

### Which fail today (verified by scratch tests, deleted)

I rendered every row of section 1 at the table's widths. At the typical and wide widths **everything passes today** (the audit's three deviations are closed). What fails, from the printed rows and cell dumps:

| Assertion | Subject / width | Evidence |
|---|---|---|
| 1 (right margin) | input box ≤ 6 | `input_box::lines(…, 6)`: `·│·a·│` — the 7-cell row is cut at 6, the right margin cell is gone (`SIDES` 6 + `max(1)`, input_box.rs:9-14) |
| 1, 4 | dialog input 8×8 | `│·│···││`: the input's right margin is gone, its wall touches the dialog's |
| 7 | dialog 14×10 / confirm 10×9 | buttons row `│·OCancel·│` / `│·Cance·│`: the `Cancel` pill (placed from the right, view.rs:317) overlaps and cuts the `OK` / `Archive` pill; no minimum width |
| 2, 3 | Context fold, Plan row, 12×12 | cell dump of row 2: `[" ", "⋅", "1", "⋅", "1", "⋅", "0", "⋅", "5", …]`: the right-aligned counters (9 cells, rows.rs:526-531) start in column 1, so the branch `├─` and the glyph are overwritten and the rail column holds a counter |

Why those, and why only those: today there is no minimum-width rule at all (a clip at the pane edge), so below the box's width the code writes a clipped box. The migrated widget fixes this by drawing nothing (Q3), and the tree row needs the counters dropped when they do not fit (an implementation detail of Job I4). Not measured: widths between those printed (between 6 and 12 for the tree; the first width that fails is `unverified`), the PR box scrolled with margin at the edges (the canvas rule is "scrolled content reaches the edges", docs/commands.md §13.2), and the Deploy column's menu through the real event loop (no test reaches it; `unverified`, read from widgets/mod.rs:170/184 and scratch-rendered through `Menu::draw_at` with `pane.inner(Margin::new(1, 0))`).

### Existing tests that change or go

- Change with the widget: `input_box_tests.rs` (every test: the width no longer includes the margin; `body()` helper, `input_box_draws_the_label_the_borders_and_the_text_inside_the_margins`), `dialog/view_tests.rs` (`the_content_has_one_column_of_padding_inside_each_wall`, `a_dialog_in_a_tiny_area_draws_without_panicking` and the `kinds()` drawings if a row changes), `plan/flow.rs` tests that pin pill x (chart padding removed: e.g. `a_margin_moves_the_buffer_and_the_pills_together`; I did not read each one), `plan/boxes.rs::tests` `body()` helper (:870, :893 build `Margin { horizontal: 2, vertical: 1 }` by hand) and `at_scroll_zero_the_first_box_starts_two_columns_and_one_row_inside_the_body` (its name says two columns; the pinned string is already 1).
- Go (subsumed by the table, delete in the last Job): `tree/margin_tests.rs` (4 tests), `plan/margin_tests.rs::the_chart_boxes_are_drawn_one_column_from_the_edge_of_the_plan_view`, `logs/margin_tests.rs::the_logs_popup_rows_keep_one_blank_column_on_each_side`, `tree/view.rs::tests::column_0_and_the_last_column_are_blank_on_every_row`.
- Stay: the string-pinning look tests (`fold.rs`, `section.rs`, `tree/view.rs` row tests): they pin text and colour, not the spacing contract.

## 6. Job split

Each Job is one Attempt (about 150 turns at most). Dependencies: A before I1-I5; B0 before B1-B3; the docs Job and B0/A are independent. `touches` are files; Jobs that share a file are marked.

| Job | Goal | touches | Depends |
|---|---|---|---|
| **D** `box-docs` | docs/commands.md §17 and `docs/contract.md` §6.1.1 "Boxes": the numbers become "see `spacing`"; the dialog stack, the nested rule, the widget API in one paragraph; no prose copy of the test | `docs/commands.md`, `docs/contract.md` | none (can run first) |
| **A** `box-widget-core` | `box_widget.rs`: `spacing`, `Walls`, `BoxWidget`, `geometry`, `SectionTitle`; unit tests of the geometry per variant and width first (red), then green; no site moves | `tui/src/box_widget.rs`, `tui/src/box_widget_tests.rs`, `tui/src/lib.rs` | none. The pill atom (`pill(...)`) is only the title input; a `Line` is enough to start |
| **B0** `layout-test-harness` | `layout_tests/mod.rs` (the table type, `Sample`, `Placed`, the 8 assertions), empty area files `dialog.rs`, `tree.rs`, `plan.rs`, `popups.rs` | `tui/src/layout_tests/` (new), `tui/src/lib.rs` | none; **parallel with A** (it needs no widget) |
| **B1** `layout-rows-dialog` (red) | rows for the input box, dialog ×4, the tree menu and the Deploy menu, with their `layout_sample`s; extracts `widgets/mod.rs` `menu_pane(pane)` so the call site is testable | `tui/src/layout_tests/dialog.rs`, `tui/src/layout_tests/popups.rs`, `tui/src/dialog/view.rs` (cfg(test) fn), `tui/src/menu.rs` (cfg(test) fn), `tui/src/widgets/mod.rs` (`menu_pane`) | B0 |
| **B2** `layout-rows-tree` (red) | Inbox, Context fold, the Deploy column's `PRs` title | `tui/src/layout_tests/tree.rs`, `tui/src/tree/inbox.rs` and `tui/src/tree/view.rs` (cfg(test) fns), `tui/src/widgets/column.rs` (cfg(test) fn) | B0 |
| **B3** `layout-rows-plan` (red) | PR boxes, legend, logs | `tui/src/layout_tests/plan.rs`, `tui/src/plan/mod.rs`, `tui/src/widgets/legend.rs`, `tui/src/logs/view.rs` (cfg(test) fns) | B0 |
| **I1** `box-dialog` | dialog frame and input box on `BoxWidget`; `popup_size` uses `spacing`; delete `input_box::lines`, `section::box_top`; the dialog's button row gets its minimum width. Turns the dialog/input rows green | `tui/src/dialog/view.rs`, `tui/src/dialog/mod.rs`, `tui/src/input_box.rs`, `tui/src/input_box_tests.rs`, `tui/src/dialog/view_tests.rs`, `tui/src/section.rs` | A, B1 |
| **I2** `box-menu` | `menu.rs` on `BoxWidget` (Plain, `highlight`); both menus go through `menu_pane` | `tui/src/menu.rs`, `tui/src/widgets/mod.rs`, `tui/src/tree/view.rs` (:763 only) | A, B1 |
| **I3** `box-pr` | PR boxes on `BoxWidget`; chart padding out of `flow.rs`; `CHART_MARGIN` from `spacing` | `tui/src/plan/boxes.rs`, `tui/src/plan/flow.rs`, `tui/src/plan/mod.rs` | A, B3 |
| **I4** `box-fold` | Inbox on `Rail`; `Fold::drawn`'s margin rows move; `RIGHT_PAD` from `spacing`; the Context rows' counters drop when they do not fit (the width-12 failure) | `tui/src/tree/inbox.rs`, `tui/src/fold.rs`, `tui/src/tree/rows.rs`, `tui/src/tree/view.rs` | A, B2 |
| **I5** `box-section` | `SectionTitle` for `PRs`, legend, logs | `tui/src/widgets/column.rs`, `tui/src/widgets/legend.rs`, `tui/src/logs/view.rs`, `tui/src/section.rs` | A, B2, B3 |
| **Z** `box-cleanup` | delete the subsumed tests (section 5), the stale comment at plan/mod.rs:265 and `MARGIN`'s doc | the test files named in section 5 | I1-I5 |

Parallel: D, A, B0 at once; then B1, B2, B3 at once; then I1, I2, I3, I4 at once (I2 and I4 both touch `tree/view.rs`, a few lines each: serialise them if the Lead prefers no merge); I5 after I4's `section.rs` is settled (I1 also edits `section.rs`: delete `box_top`; small); Z last. The pill spec Job (another Job) touches `section.rs` and the cap constants too: I1 and I5 should start after it lands or rebase on it.

## 7. Open questions only the User can answer

- **Q1. The menus' corners.** Menus are plain `┌┐└┘` (menu.rs:167), every other box rounded `╭╮╰╯` (hs-d474 for the chart, hs-d578 for the dialog). Options: (a) round the menus (one `BorderType` everywhere; recommended: one look, and the widget needs no plain variant); (b) keep plain as the popup menu's own look.
- **Q2. Rows with the glyph straight against the rail.** The rule (hs-d578) says 1 column of padding between rail and content; the Context fold's `├─glyph`, `╭glyph` and every closing row `╰󰬦 expand` have none by recorded design (hs-d337, hs-d339, hs-d484). Options: (a) the layout test exempts those rows by name and the design stays (recommended: it is recorded and Rust and pi agree); (b) the test asserts the padding and the rows change to ` ├─ glyph`, ` ╰ 󰬦 expand`.
- **Q3. A box narrower than it can be drawn.** Options: (a) the widget draws nothing below `MIN_WIDTH` (5 columns, 7 with margins; recommended: no half boxes, the layout test can assert it); (b) keep today's clip at the pane edge.
- **Q4. The Plan view's message row** (`no Plan yet`, `no PR yet: cpd pr add`) is at column 2, the box wall at column 1 and its content at column 3. Options: (a) leave it (it is not a box; the docs already say so); (b) align it with the box content (column 3) or the wall (column 1).
