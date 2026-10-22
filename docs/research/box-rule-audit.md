# Box rule audit (Job `dialog-audit`, Plan `dialog-look`)

The rule (User, hs-d578): every box has 1 column of margin (between its outermost edge, wall, corner or rail, and the edge of what contains it) and 1 column of padding (between wall or rail and content); 0 blank padding rows above its first and below its last content row (spacing rows between the box's elements are spacing, not padding); a box inside a box: the outer padding is the inner margin; a fold block has a left rail only: margin, rail, padding (` ╭─`, ` │ `, ` ╰`).

How measured. `file::test` means a test that already pins the rows (read off its assertion). `rendered` means rows I rendered with a throwaway test (deleted before the commit; nothing of it is in the tree). `·` is a blank cell. Rust rows are `Buffer` cells, pi rows are the components' `render(width)` with SGR stripped. No real tmux and no real pi pane was run.

Found with `rg -n '╭|╰|│|├|┌|└' tui/src pi --glob '!**/*test*' --glob '!pi/test/**'`; every file it lists is named in a `Where` below or under `Not boxes`.

## Table

| Box | Where (file, function) | Measured rows | Left margin / padding | Right padding / margin | Blank rows inside, top / bottom | Verdict |
|---|---|---|---|---|---|---|
| Input box (Rust) | `tui/src/input_box.rs` `lines`, `rule`; `tui/src/section.rs` `box_top` (its only user is the dialog kit's `dialog/view.rs`, whose frame is not measured) | `input_box_tests.rs::input_box_draws_the_label_the_borders_and_the_text_inside_the_margins`, width 44: `·╭─(Title)────…───╮·` / `·│·abc·····…····│·` / `·╰────…────╯·` | 1 / 1 | 1 / 1 | 0 / 0 | ok |
| Fold: Inbox title, item rows, scroll cues | `tui/src/tree/inbox.rs` `draw`, `item_line`, `cue_line`; `tui/src/fold.rs` `content`, `BRANCH`; `tui/src/section.rs` `collapsible_title` | `tree/view.rs::tests::three_items_are_three_rows_with_the_age_at_the_right_edge_and_the_terminator_close`, width 43: `·╭─(Inbox)───…──(3)·` / `·│·●·f-blocked: Worker question···…···42s` (the age ends in column 41 of 0..42) / `·╰󰁚`; `…::the_inbox_title_sits_inside_the_panels_padding_one_column_each_side`: cells 0, 1 and 41, 42 of the title are `·`, `╭` and the cap, `·`; `tree/inbox_indent.rs::an_item_row_is_the_rail_one_space_the_glyph_a_space_the_label_and_the_age`: `·│·{glyph}·{label}…5m`; cue: `tree/view.rs` `cue_row`: `·│` + right-aligned `▼ n more` | 1 / 1 (rail, one blank) | no right rail; the text ends 1 column before the edge: n/a / 1 | 0 / 0 (the empty rows above the title and below the closing row lie outside: rows[9] and rows[15] of that test) | ok |
| Fold closing row (toggle and terminator): Inbox, Context, pi cpd message, pi tool rows, pi Thinking | Rust: `tui/src/fold.rs` `toggle`, `plain`, `done`; pi: `pi/lib/render.ts` `rowsOf` (`toggle`, `terminator`), `compact-tools.ts` (`CHOOSE_ROW`, `TERMINATOR_ROW`, the closing `" ╰"` rows), `compact-tools-thinking.ts` `toggleRow` | `tree/view.rs` `EXPAND` = `·╰󰁋·expand`, `COLLAPSE` = `·╰󰁣·collapse`; `…::three_items…` rows[14] = `·╰󰁚`; pi `test/render.test.ts` `closing("expand")` = `·╰󰁋·expand`; rendered tool row, folded, width 40: `·╰󰁋·expand···Done·in·0.300s·(9·lines)` | 1 / 0: the glyph is straight against `╰` | n/a (text) | 0 / 0 | unclear: the rule writes the closing row as ` ╰` and says nothing of what follows; the design is recorded (Notes 1) |
| Fold: Context block (tree) | `tui/src/tree/rows.rs` `context_items` (about l. 379-409: `fold::title`, `fold::content`, `fold::toggle`, `fold::plain`), `tui/src/fold.rs` | `tree/view.rs::tests::a_context_with_nothing_to_fold_ends_with_its_last_rows_corner_branch_and_no_closing_row`, width 44: `·╭󰀨·alpha` / `·├─●·ship` / `·╰─󰀨·ship·f-bl` / `·` / `·╭󰀨·beta`; `tree/look.rs::an_expanded_context_shows_its_live_rows_then_its_archived_rows_and_closes_with_the_up_arrow`; `tree/view.rs::column_0_and_the_last_column_are_blank_on_every_row`; `…::the_counters_end_at_width_minus_2_and_the_cursor_row_is_inverted_between_the_margins` | 1 / 0: the glyph is against `╭` and `├─` (the branch rows are `Not boxes`, A1; the title row `╭<glyph>` has the same shape) | the row text and counters end in column width-2: n/a / 1 | 0 / 0 | unclear: recorded design, hs-d337, hs-d339 (Notes 2) |
| Section title (a rule with a pill, no wall): the Inbox (`collapsible_title`), the Deploy column's `PRs`, the legend and logs popups' titles | `tui/src/section.rs` `title`, `collapsible_title`; `tui/src/widgets/column.rs` `render_header`; `tui/src/widgets/legend.rs` `draw_scrolled`; `tui/src/logs/view.rs` `draw` | `section.rs` tests: `·╭─(Inbox)───────(35)·`, and the non-collapsible form starts `·──`; `logs/view.rs` (width 24): `·──(Logs)──────────────·`; rendered legend (137 wide): `·──Legend:·What·The·Cockpit's·Colours·And·Icons·Mean·(Any·Key·Closes)────…────·` | 1 / n/a (the lead `╭─` / `──` is the rail) | the last cell is blank: n/a / 1 | 0 / 0 | ok |
| Plan view: PR boxes | `tui/src/plan/boxes.rs` `layout` (`Block::bordered().border_type(Rounded)`, the chart blitted at `rect.inner(Margin 1,1)`); the canvas margin `tui/src/plan/mod.rs` `MARGIN` (l. 79) applied by `with_margin(MARGIN)` (l. 101, 129) | `plan/boxes.rs::tests::at_scroll_zero_the_first_box_starts_two_columns_and_one_row_inside_the_body` (rows[1]): `··╭·api·api-repo·󱦰·main·dispatch·╮··`; rendered, the box of a PR with two Jobs: `╭·api···api-repo·󱦰·main···dispatch·╮` / `│·󰝦·a│S1──…──󰝦·b│S1····│` / `╰──────…──────╯`; with a chart wider than the title: `│·(cap)󰝦·averylongjobkeynamehere1│S1──…──(cap)·│` (right end: pill cap, one blank, wall) | **2** (canvas margin) / 1 (wall, one blank, the pill's cap) | 1 / **2** (padding 1; margin 2 by `MARGIN`, pinned by the `body` test) | 0 / 0 (the top border is followed by the first Job row, the bottom border follows the last) | deviates (margin 2, rule wants 1) |
| Plan view: chart arrows and lines between boxes | `plan/boxes.rs` `layout` (the lines), `plan/flow.rs` | `plan/fixtures/cpd-chart.txt`; `boxes.rs::tests::a_pr_not_merged_that_needs_a_merged_one_sits_below_it_with_an_arrow` | see Not boxes | | | not a box |
| Right-click menu: the tree's | `tui/src/menu.rs` `draw_at`, `rect`; `tui/src/tree/menu.rs`; the view passes the inner area, `tree/view.rs` `self.pane` | `menu.rs::tests::the_menu_is_drawn_as_a_bordered_box_over_what_is_there`: `┌───────────┐` / `│·New·Plan…·│` / `│·Rename…···│` / `└───────────┘` (padding 1 and 1, no blank row); `tree/view.rs::tests::the_menu_is_clamped_inside_the_inner_area_and_a_click_on_a_margin_is_outside_it`: in 44×16 the box is x 27..43 (end exclusive), so column 43 stays blank, and a click at (0, 0) puts it at x = 1 | clamped: 1 / 1 | 1 / 1 | 0 / 0 | ok |
| Right-click menu: the Deploy column's | `tui/src/widgets/mod.rs` l. 170 `open.menu.draw_at(f, pane, frame)` and l. 184 `open.menu.mouse(pane, *m)`, `pane = f.area()` (l. 156) | rendered (`Menu::draw(f, f.area())`, 30×8, click at (29, 7)): `·················┌───────────┐` (17 blanks) / `·················│·New·Plan…·│` / … / `·················└───────────┘`, the last on the pane's last row | 0 when the click is in column 0 (`rect` is `at.x.min(pane.right() - width).max(pane.x)`, `pane.x` = 0; by the code, not rendered) / 1 | 1 / **0** when clamped at the right edge (the box's last column is the pane's last column) | 0 / 0 | deviates |
| Legend popup contents (tmux draws the border) | `tui/src/widgets/legend.rs` `body`, `draw_scrolled` (`Margin::new(1, 0)`) | rendered, 137×63: `·Meanings·→·Colour·(One·Source·For·Every·View)·…` / `·██··not·started` / `·██··in·progress`; `legend.rs::tests::the_title_is_a_section_title_under_an_empty_row_and_over_one` | 1 (no wall, no rail: text) | the body is laid out inside `Margin::new(1, 0)`: n/a / ≥ 1 | 0 / 0 | ok (no wall: only a margin applies) |
| Logs popup contents (tmux draws the border) | `tui/src/logs/view.rs` `line`, `draw` | `logs/view.rs::tests::the_title_is_a_section_title_between_two_empty_rows_and_the_events_start_under_it`: `·00:00:08·event·8`; rendered, 24×6, an Event of 60 characters: `·00:00:01·xxxxxxxxxxxxx…` (the `…` is in the last column) | 1 (no wall) | n/a / **0**: `room = width - 10` makes the row end in the last column; the error row too | 0 / 0 | unclear: no wall, rail or border, so the rule may not apply; the text reaches the edge (Notes 3) |
| Merge popup contents | `tui/src/merge/` (a dialog of the kit) | not measured | | | | rebuilt by this PR (dialog-impl) |
| Dialog kit frame | `tui/src/dialog/` | not measured | | | | rebuilt by this PR (dialog-impl) |
| pi: the cpd message fold | `pi/lib/render.ts` `rowsOf`, `Rendering.render` (`TOP`, `RAIL`, `CLOSE`, `MARGIN`) | `pi/test/render.test.ts` "an Event, expanded: …": `·╭─(cpd)·Job·api-1·merged.` / `·│·Job·api-1·merged.` / `·│·second·line` / `·╰󰁣·collapse`; rendered, width 24: `·╭─cpd·alpha·beta` / `·│·gamma·delta` / `·│·a·bit·longer·than·the` (exactly 24 cells: the last column holds text) / `·╰󰁣·collapse` | 1 / 1 | n/a / **0** (text reaches the last column) | 0 / 0 | unclear: left side ok; the right edge is not stated for a left-rail-only fold (Notes 4) |
| pi: tool rows and group heads (fold `toggle`) | `pi/compact-tools/extensions/compact-tools.ts` (heads `" ╭─"`, closing `" ╰"`), `compact-tools-layout.ts` `renderOutput`, `railComponent`, `renderArguments`, `continuationPrefix` (`" │ "`) | `pi/test/compact-tools/fold-rows.test.ts::head, body and closing rows of an expanded tool row share one column` (every row starts with one space, head `·╭─`, body `·│·`, closing `·╰`); rendered, width 40: `·╭─bash·cd·/project·&&·npm·test·--·--w` (exactly 40 cells) / `·│·atch=false` / `·│·out·1` / … / `·╰󰁣·collapse···Done·in·0.300s·(9·lines)` | 1 / 1 | n/a / **0** | 0 / 0 | unclear (as the cpd message fold, Notes 4) |
| pi: the Thinking fold | `pi/compact-tools/extensions/compact-tools-thinking.ts` `renderThinkingSections` (`connector`, `compactDetail`, `toggleRow`); its margin is the host's `Markdown` `paddingX = outputPad` (pi's `assistant-message.js`) | rendered through pi's real `AssistantMessageComponent` (outputPad 1, width 36), expanded: `·╭─Thinking…` / `·│` / `·│·Weighing·the·options.` / `·│` / `·│·Check·the·code` / … / `·╰󰁣·collapse`; folded: `·╭─Thinking…` / `·╰󰁋·expand`; long text: `·│·word·word·word·word·word·word····` (4 blank columns after the last word) | 1 / 1 | the longest row seen ends 4 columns before the edge; the exact limit is unmeasured (Unmeasured 2) | one blank rail row, `·│`, between the head and the first body row: 1 / 0 | unclear (Notes 5) |
| pi: tool rows of the non-toggle styles (`fold: status`, `style: claude`) | `compact-tools.ts` (`" └ "` rows), `compact-tools-layout.ts`, `compact-tools-codex-rows.ts` | `pi/test/compact-tools/compact-tools.test.ts` "collapsed Claude rows keep each line to one row and keep output indentation": `·└·······70·README.…` / `···xxxxxxxxxxxxxxxx…` (width 20); `prefixedLines("abcdefgh", " └ ", "   ", false).render(8)` = `·└·abcde` / `···fgh` | 1 / 1 | n/a / 0 | 0 / 0 | unclear (as the rows above, Notes 4) |
| pi: Codex-style rows (`style: codex`, not the default) | `compact-tools-codex.ts` (`CODEX_RESULT_PREFIX = "   └ "`), `compact-tools-codex-rows.ts` | `pi/test/compact-tools/codex-ui.test.ts` (l. 324): `·•·Called·web_search` / `···└·result·4` / `·····result·5` | the `└` is in column 3: 3 / 1 | n/a / text to the edge (unmeasured) | 0 / 0 | unclear: a copy of Codex's own chrome (the header comment of `compact-tools-codex.ts`: "Pi draws every row one column in, so each prefix keeps that geometry one column further right"); I found no record in docs/ |
| pi: the editor box | `pi/compact-tools/extensions/compact-tools-editor.ts` (l. 95-100) | `pi/test/compact-tools/editor-box.test.ts::the editor draws a rounded box with one margin column and one padding column`, width 40: `·╭────…────╮·` / `·│·fix·the·build···…···│·` / `·│·then·ship·it····…···│·` / `·╰────…────╯·`; contract §6.9 "The input widget" states the same geometry | 1 / 1 | 1 / 1 | 0 / 0 | ok |
| pi: chat balloons (the user's, the agent's) | `pi/compact-tools/extensions/compact-tools-balloon.ts` `drawBalloon` (re-exported by `pi/lib/balloon.ts`) | `pi/test/balloon-margins.test.ts::an agent balloon has one margin column on its left and at least three on its right`, width 40: `·╭─────────────╮` / `·│·agent·reply·│` / `·│╭────────────╯` / `·╰╯`; the user's: 24 blanks, then `╭─────────────╮·` / `│·user·prompt·│·` / `╰────────────╮│·` / `·············╰╯·`; `…::a balloon has one padding column on each side and none above or below` | agent 1 / 1; user ≥ 3 (far side) / 1 | agent ≥ 3 (far side) / 1; user 1 / 1 | 0 / 0 (the two tail rows are not blank rows: `│╭──╯` and `╰╯`) | unclear: the far-side gap of at least 3 is a recorded design (Notes 6) |
| pi: the `ask_user_question` tab bar, tool pills, the `cpd` pill | `pi/compact-tools/` | pills only; no wall, rail or border | | | | not a box (a pill is a label) |

## Fix: Plan view: PR boxes

Rows now (`plan/boxes.rs::tests::at_scroll_zero_the_first_box_starts_two_columns_and_one_row_inside_the_body`, one empty PR `api`, scroll (0, 0); row 0 is the 1 blank row of vertical margin):

```
(row 0)  ····································
(row 1)  ··╭·api·api-repo·󱦰·main·dispatch·╮··
```

The pinned string is `"  ╭ api · api-repo 󱦰 main · dispatch ╮  "`: 2 columns of margin on each side. Rows wanted (1 column of margin each side; the blank row above is not touched by the rule):

```
·╭·api·api-repo·󱦰·main·dispatch·╮·
·│·no·Jobs·······················│·
·╰────────────────────────────────╯·
```

That is `" ╭ api · api-repo 󱦰 main · dispatch ╮ "` for rows[1] of `body(…, (0, 0))`. The padding (wall, one blank, content) and the 0 blank rows inside already are as the rule wants.

Scrolled (`scrolled_the_box_is_drawn_up_to_every_edge_of_the_body`) the box is drawn up to every edge of the body, so those rows are unchanged; the offsets that reach the corners move by one column (the canvas is two columns narrower): `body(10, 3, (2, 1))` becomes `(1, 1)` and `body(10, 3, (28, 1))` becomes the largest offset of the new canvas (I did not run it; recompute it).

Files and functions to change:

- `tui/src/plan/mod.rs`: `const MARGIN: Margin = Margin { horizontal: 2, vertical: 1 }` (l. 79). It feeds the chart canvas (`with_margin(MARGIN)`, l. 101, 129 and the test helper l. 608), the chart's message row (`Ui::draw`, l. 228, ` no Lead session in the centre`, aligned with the chart's content) and Pulse (`draw_pulse`, l. 214 and 218; `pulse::buckets(width - 2 * MARGIN.horizontal, 0)`, l. 122). Pulse's rows are not boxes. To change only the boxes, add a `CHART_MARGIN` of `Margin { horizontal: 1, vertical: 1 }` for `with_margin` (l. 101, 129) and the message row (l. 228) and leave `MARGIN` for Pulse. Whether the message row and Pulse follow is the Lead's call; I did not decide it.
- `tui/src/plan/boxes.rs`: `Boxes::with_margin` (l. 301) is generic and stays. The tests' helper `body` (about l. 909) hard-codes `Margin { horizontal: 2, vertical: 1 }`.

Existing tests that pin the old rows and so change with the fix:

- `tui/src/plan/boxes.rs::tests::at_scroll_zero_the_first_box_starts_two_columns_and_one_row_inside_the_body` (its name says two columns too) and its helper `body`;
- `tui/src/plan/boxes.rs::tests::scrolled_the_box_is_drawn_up_to_every_edge_of_the_body` (the offsets, above);
- `tui/src/plan/mod.rs::tests` that use `scroll_centre` (l. 608) and compare scroll offsets that include the margin, for example `switching_plans_and_back_restores_the_offset_with_no_selection` (asserts scrolls like `(4, 9)`): re-run `plan::` after the change; I did not check which of them depend on the 2;
- `tui/src/plan/flow.rs::tests::a_margin_moves_the_buffer_and_the_pills_together` passes its own margin and does not change.

The docs paragraph that states the old form: `docs/commands.md` §13.2 `plan`, bullet **Chart** ("inside a blank margin of 2 columns and 1 row that belongs to the chart's canvas", l. 2969); and only if Pulse follows, the **Row layout** bullet (`MARGIN: 2 columns left/right, 1 row top`, l. 2988) and **Buckets** (`pane width - 2*2 (margin)`).

## Fix: Right-click menu: the Deploy column's

Rows now (rendered; a 30×8 pane, a right click at (29, 7); the box is clamped to the pane `f.area()`):

```
······························
······························
·················┌───────────┐
·················│·New·Plan…·│
·················│·Rename…···│
·················│·Copy·id···│
·················│·Archive···│
·················└───────────┘
```

The box ends in the pane's last column (right margin 0); a click in column 0 starts it in column 0 (left margin 0). Rows wanted for the same click: the box is clamped inside the pane less one blank column on each side, as the tree's menu is (`tree/view.rs` l. 724: `inner = f.area().inner(Margin::new(1, 0))`, and `self.pane` below its blank top row):

```
······························
······························
················┌───────────┐·
················│·New·Plan…·│·
················│·Rename…···│·
················│·Copy·id···│·
················│·Archive···│·
················└───────────┘·
```

The 13-column box then starts in column 16 and ends in column 28; column 29 is the margin. The rule says nothing of a vertical margin (the tree's box may end on the last row: `rect.bottom() == 16` of 16 in its pinned test), so the vertical clamp stays.

Files and functions to change: `tui/src/widgets/mod.rs`: the draw (l. 169-170, `open.menu.draw_at(f, pane, frame)`) and the input (l. 184, `open.menu.mouse(pane, *m)`) both take `pane = f.area()` (l. 156): give both `pane.inner(Margin::new(1, 0))`, so the drawn box and the hit test agree. `Menu::rect` and `Menu::hit` in `tui/src/menu.rs` stay as they are.

Existing tests that pin the old rows: none that I found. `tui/src/menu.rs::tests::the_box_is_at_the_click_and_clamped_inside_the_pane` and `…::the_menu_is_drawn_as_a_bordered_box_over_what_is_there` are generic over the pane they are given and do not change; `tui/src/widgets/mod.rs` has no test that draws the menu. The red test is a new one (draw the column's menu with a click at each edge, assert the blank edge columns).

The docs paragraph that states the old form: `docs/commands.md` §13.2 `widgets`, "The PR row's right-click menu" (l. 3037): "bordered, at the click, clamped inside the pane". The tree's text (l. 2896, 2946) already says the inner area, with its one blank column on each side.

## Notes on the unclear rows

1. **Closing rows** (` ╰󰁋 expand`, ` ╰󰁣 collapse`, ` ╰󰁚`): the glyph is straight against `╰`, in the Rust folds and in pi. The rule's example of the closing row is ` ╰` and says nothing of what follows; the body rows have a padding blank (` │ `), the closing row has none. Recorded: `docs/commands.md` §17 "Collapsible blocks" (l. 3767), `docs/contract.md` §6.9 "The terminator" (l. 2959) and "The fold's closing row" (hs-d484, l. 2209), and the header of `tui/src/fold.rs`. Rust and pi agree.
2. **Context fold title and branch rows**: ` ╭󰀨 alpha`, ` ├─●ship`, ` ╰─●rel`: the glyph is against the rail, not after a blank. Assumption A1 (Not boxes) covers the branch rows; the title row `╭<glyph>` is the same design. Recorded: `docs/commands.md` §13.2 `tree`, bullet **Rows** (User, hs-d337, hs-d339, l. 2897) and `tui/src/fold.rs`. The Inbox's own title ` ╭─Inbox` and rows ` │ ● label` follow the rule.
3. **Logs popup**: no wall, rail or border; the row text is cut at the last column (`room = width - 10`). By the rule's words the list is not a box; if the User counts a text list as one, its right margin is 0.
4. **pi rail folds** (cpd message, tool rows, ` └ ` rows): the left side follows the rule (1 margin, rail, 1 padding). A left-rail-only fold has no right wall and the rule states only "margin, rail, padding"; the rows wrap or are cut at the pane width, so text reaches the last column (right margin 0). The Rust folds end 1 column before the edge (the Inbox's age ends in column width-2) and the Thinking fold has the host's padding, so pi is inconsistent with itself and with the Rust side. Whether the rule asks for a right margin here is not mine to decide. If it does: `Rendering.render` in `pi/lib/render.ts` (it wraps with `draw(width)`; with `outputPad === 0` it uses `outputPadLines`) and the row components of `compact-tools-layout.ts` and `compact-tools.ts` would wrap at `width - 1`; I did not write a `Fix:` subsection for it.
5. **Thinking fold**: one blank rail row (`connector`, `compact-tools-thinking.ts` l. 165) between the head and the first body row; blank lines of the text are `│` rows too. The rule counts spacing between a box's elements as spacing, not padding; the head and the body are two elements, so it may be allowed; if the head is part of the box's content it is one blank row at the top. No record found: only the code; `docs/contract.md` §6.9 "The Thinking fold" (l. 2961) does not mention it.
6. **Chat balloons**: near-side margin 1 and padding 1 each side, no blank inner row: as the rule. The far side has a gap of at least 3 (the balloon is at most pane − 4 wide, and as wide as its text + 4). Recorded: `docs/contract.md` §6.9 "Chat balloons" (User, 2026-10-22, "Margins": 1 near, at least 3 far) and the header of `pi/test/balloon-margins.test.ts`. The rule hs-d578 does not mention the gap; not decided.

## Not boxes

- The tree's branch rows (`├─<glyph>`, `╰─<glyph>`; `tui/src/tree/rows.rs`, `tree/view.rs`, `fold::TREE_BRANCH`, `LAST_BRANCH`): Lead's assumption A1 (the glyph against the branch is the User's tree design; the User may correct it). The Context's title and closing rows are in the table.
- The tmux status line and the tmux pane and popup borders (tmux's own): A1. The popups' contents are in the table.
- Plan chart arrows, lines and junctions (`tui/src/plan/flow.rs` glyph table l. 241-247, `plan/boxes.rs` arrows, `plan/fixtures/cpd-chart.txt`): graph edges between boxes, not a wall, border or rail round or beside content.
- Pills (`tui/src/widgets/legend.rs` samples `key│S1`, `pill_colours.rs`, the `│` inside a Job pill): a pill is a label; the `│` is the tier separator.
- Plain text panes with no wall, border or rail: the history view (`tui/src/history/view.rs`), the Worker view (`tui/src/worker/`), the read views (`tui/src/read/`), the chat placeholder (`tui/src/chat.rs`, `Padding::left(1)`), the Deploy column's PR rows and machine widget (`tui/src/widgets/column.rs`, `host.rs`; the active PR is a blue block, no wall), the Plan view's message row and Pulse rows (`tui/src/plan/pulse.rs`). I searched these for `Block`, `Borders`, `╭`, `│`, `─` and found no drawing of one.
- Files the `rg` lists that only quote the rows above in comments or tests: `tui/src/tree/mod.rs`, `tree/look.rs`, `tree/inbox_indent.rs`, `pill_colours.rs`, `palette.rs` (comment on the toggles).
- `pi/compact-tools/schemas/compact-tools.schema.json`: a description string.

Rebuilt by this PR (`dialog-impl`), not measured: the dialog kit's frame, `tui/src/dialog/` (`dialog/view.rs` is in the `rg` list), with the input box inside it and the merge popup's contents.

## Unmeasured

1. Real tmux popups (legend, logs): their contents were rendered into a ratatui `Buffer` at a size of my choosing; the tmux border and the popup's own offset were not looked at.
2. The Thinking fold's right margin for a full row: the rows I rendered stop 4 columns before the edge, short of the wrap limit (`availableWidth - 2` in `compactDetail`, plus the host's `Markdown` `paddingX`); the number of blank columns after a full row was not read off a row.
3. The tool-row styles `claude`, `codex` and `fold: status`: only the rows existing tests pin (cited); no rendering of my own, and the `" └ "` closing rows of `fold: status` were not rendered.
4. The Plan view's PR boxes through the whole `Ui::draw` (with the message row): measured through `layout` and the pinned `body` helper (`Chart::blit`) only.
5. The merge popup's contents and every dialog: rebuilt by this PR (`dialog-impl`).
