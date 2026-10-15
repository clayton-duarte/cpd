# cpd-tui: ratatui, its ecosystem, a visual language and a component library

Research Job `research-components` (Workflow `cpd-ui`), 2026-10-15. Findings only; no product code changed.

**How claims are sourced.** `[src: …]` is a file in the crate source under
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/` (shortened to `$REG/`); `[cargo info]`, `[cargo info -v]` and
`[cargo search]` are commands run on 2026-10-15; `[gh api]` is `gh api repos/<owner>/<repo>` run the same day; `[tui/src/…]`
is this repo. Anything without a source is labelled **guess** or **unverified**.

---

## 1. Summary

**Recommendation: option (a), our own small `ui` module on top of plain ratatui, plus a `cpd-tui gallery` view. Adopt no
framework.** ratatui 0.30.2 already ships almost every widget we need, and we underuse it (no `Scrollbar`, `List`, `Clear`,
`Block::padding`, `Flex`, `Layout::spacing`, `TestBackend::assert_buffer_lines`). Our real gap is not widgets but **one
place that says what a component looks like**: today the same look is restated in several views (three copies of
`truncate`, hand-made `" "` insets, two kinds of section title, tab hit-ranges computed by hand, yellow meaning both
"review" and "note").

- ratcn (0.0.6) has the best ideas (paint widget + optional engine, three fills per control) but its themes are true-colour
  RGB and its engine owns focus, hover and layers inside one process; we have four processes in tmux panes, tmux owns
  focus and popups, and the User chose named terminal colours and a keyboard-only cursor. Borrow its *shape* (paint-only
  widgets that take a token set and some bools), not the crate.
- tui-realm (4.1.0) is an Elm/React-style framework that owns the event loop: too heavy for four ~250-line views.
- Adopt single-purpose crates only when a component needs them: `tui-input` (0.15.5, ratatui 0.30.2) if the inline prompt
  needs cursor editing; nothing else is needed now. No motion (tachyonfx, spinners): the cockpit is calm by design.

**The next Jobs** (detail in section 7):

1. **`ui-tokens`**: turn `palette.rs` into `ui/tokens.rs`: colour roles (status meanings + the few UI roles, fixing the
   yellow/green double use), type roles (bold, dim, reversed), glyphs with a width test, spacing constants. S.
2. **`ui-components`**: extract the existing pieces into `ui/` as ratatui `Widget`s (Pill, Dot, Counters, Separator,
   TabBar, KeyButton, Indicator, SectionTitle, EmptyState, PromptLine) with one `ellipsize`; views call them. M.
3. **`ui-gallery`**: `cpd-tui gallery` draws every component in every state from the `ui` module (the legend's idea,
   generalised), with one snapshot test of the whole gallery. M.
4. **`ui-ratatui-native`**: replace hand-made layout with ratatui: `Scrollbar` on the Worker view and the chart,
   `Table` for the Dispatch PR rows, `Layout::spacing` for tab hit areas, `Block::padding` for insets. M.
5. **`ui-feedback`** (after the User reviews the gallery): the missing components the views will need: a keys hint row
   shown only with focus, a toast for action outcomes (fork started, close refused), a loading/empty state. S–M.

---

## 2. What ratatui 0.30 can do

Version in use: `ratatui 0.30.2`, split into `ratatui-core 0.1.2`, `ratatui-widgets 0.3.2`, `ratatui-crossterm 0.1.2`
(crossterm 0.29.0), `ratatui-macros 0.7.2` [tui/Cargo.lock]. Default features: `all-widgets, crossterm, layout-cache,
macros, underline-color`; opt-in: `palette`, `serde`, `scrolling-regions`, `unstable-widget-ref`,
`unstable-rendered-line-info`, `unstable-backend-writer`, and the backends `termion`, `termwiz`, `termina` [cargo info
ratatui@0.30].

### 2.1 Built-in widgets

All in `$REG/ratatui-widgets-0.3.2/src/` [src: file listing]. "Use" is what `tui/src/*.rs` imports today
[grep of tui/src].

| Widget | What it gives | We use it? |
| --- | --- | --- |
| `Block` | borders (`Borders` flags), `BorderType` (Plain, Rounded, Double, Thick, four dashed kinds, QuadrantInside/Outside) [src: borders.rs], titles top/bottom with alignment, `padding(Padding)`, `merge_borders(MergeStrategy)` (Replace, Exact, Fuzzy: adjacent blocks share `┼ ├ ┤`) [src: block.rs:707, ratatui-core symbols/merge.rs], `shadow(Shadow)` with presets `overlay`, `block`, `light/medium/dark_shade` [src: block/shadow.rs], `inner(area)` | only as the separator (`Borders::TOP`) [tui/src/separator.rs]. No padding, titles, shadow or merge |
| `Padding` | `zero, horizontal, vertical, uniform, proportional, symmetric, left, right, top, bottom` [src: block/padding.rs] | no (insets are hand-typed `" "`) |
| `Paragraph` | `Text` with `alignment`, `wrap(Wrap { trim })`, `scroll((y, x))`, `line_count`, `line_width` [src: paragraph.rs] | yes, everywhere; Worker view scrolls it by hand |
| `List` + `ListState` | items, `highlight_style`, `highlight_symbol`, `highlight_spacing`, `repeat_highlight_symbol`, `scroll_padding`, `direction`; state: `select`, `select_next/previous/first/last`, `offset`, `scroll_up_by/down_by` [src: list.rs, list/state.rs] | no |
| `Table` + `TableState` | rows, header, footer, `widths`, `column_spacing`, `flex`, row/column/cell highlight styles; state: row, column and cell selection, `scroll_left_by/right_by` [src: table.rs, table/state.rs] | legend only [tui/src/legend.rs] |
| `Tabs` | titles, `select(Option)`, `divider`, `padding_left/right`, `highlight_style` [src: tabs.rs]. **No hit areas**: it returns nothing per tab | no: `tabs.rs` draws spans and computes `[from, to)` by hand for clicks |
| `Scrollbar` + `ScrollbarState` | `ScrollbarOrientation` VerticalRight/Left, HorizontalBottom/Top; thumb/track/begin/end symbols and styles; state `content_length`, `viewport_content_length`, `position`, `prev/next/first/last` [src: scrollbar.rs] | **no**: the Worker view and the chart scroll with no indicator |
| `Gauge`, `LineGauge` | ratio bar, block or line style, label [src: gauge.rs] | no (wave progress is an open idea, decision 18) |
| `Sparkline` | one-row bar series [src: sparkline.rs] | no (homeserver load could be one) |
| `BarChart` (`Bar`, `BarGroup`) | vertical/horizontal grouped bars [src: barchart.rs] | no |
| `Chart` | axes, datasets, markers (Dot, Block, Bar, Braille, HalfBlock, Quadrant, Sextant, Octant) [src: chart.rs; ratatui-core symbols/marker.rs] | no (our `flow::Chart` is our own) |
| `Canvas` | free drawing in braille/half-block: Line, Rectangle, Circle, Points, Map [src: canvas/] | no; considered for edges? A guess: braille lines would be too faint at 1 cell per row for the bus lines |
| `calendar::Monthly` | month calendar (feature `widget-calendar`, on by default) [src: calendar.rs] | no use for us |
| `Clear` | resets an area before drawing over it (popups) [src: clear.rs] | no: our popup is tmux's `display-popup` |
| `Fill` | paints an area with one symbol and style [src: fill.rs] | no |
| `RatatuiLogo`, `RatatuiMascot` | logos [src: logo.rs, mascot.rs] | no |
| `&str`, `String`, `Line`, `Span`, `Text` | all are widgets themselves [src: ratatui-core widgets/widget.rs:84] | yes (`Line::right_aligned().render(...)` in context.rs) |

### 2.2 Layout

`Layout::vertical/horizontal(constraints)`, `.margin`, `.horizontal_margin`, `.vertical_margin`, `.flex(Flex)`,
`.spacing(n)` (negative spacing overlaps, so borders can collapse), `.areas::<N>()`, `.try_areas`, `.spacers::<N>()`,
`.split_with_spacers` [src: ratatui-core layout/layout.rs:250–738]. `Constraint`: `Min, Max, Length, Percentage, Ratio,
Fill(weight)` [src: layout/constraint.rs]. `Flex`: `Legacy, Start, End, Center, SpaceBetween, SpaceEvenly, SpaceAround`
[src: layout/flex.rs]. `Rect::inner(Margin)`, `Rect::intersection`, `Offset`, `Position`, `Size` [src: layout/].
The `layout-cache` feature memoises splits [cargo info ratatui@0.30].

We use `Layout` with `Length`/`Min` and `areas` everywhere, `Margin` for the chart padding [tui/src/main.rs, flow.rs].
**Underused:** `Flex` (centring the legend, right-aligning without a `Min(0)` filler), `spacing` (tab gaps and the hit
areas of tabs: a horizontal `Layout` of `Length(label width)` with `spacing(1)` returns each tab's `Rect`, replacing the hand
arithmetic in `tabs.rs`), `Fill` (the PR title column), `spacers` (where separators go).

### 2.3 Text

`Span` (styled run), `Line` (spans + `alignment`: `left_aligned`, `centered`, `right_aligned`, `patch_style`,
`reset_style`, `width`, `styled_graphemes`), `Text` (lines) [src: ratatui-core text/line.rs]. Wrapping is a `Paragraph`
feature (`Wrap { trim }`). **Truncation:** a `Line` wider than its area is cut at the area edge, on the right for left
alignment and on the left for right alignment [src: text/line.rs:739–750], through `unicode-truncate` [src: text/line.rs:9;
ratatui-core Cargo.toml]. **There is no ellipsis** anywhere in ratatui-core or ratatui-widgets (no match for `ellipsis` or
`…` in either source tree). So our `…` truncation is legitimately ours, but we have three copies of it
(`context::truncate`, `flow::truncate`, `pr::truncate`), all counting `chars()` rather than display width
[tui/src/context.rs:97, flow.rs:63, pr.rs:117]. Width is measured by `unicode-width 0.2.2` in ratatui [tui/Cargo.lock].
`Masked` text exists for secrets [src: text/masked.rs]. `ratatui-macros` gives `line!`, `span!`, `text!`, `constraints!`
[cargo info ratatui@0.30: feature `macros`].

### 2.4 Styling

`Style { fg, bg, underline_color, add_modifier, sub_modifier }`; `Modifier`: `BOLD, DIM, ITALIC, UNDERLINED, SLOW_BLINK,
RAPID_BLINK, REVERSED, HIDDEN, CROSSED_OUT` [src: ratatui-core style.rs:105–113]; `underline_color` behind the default
`underline-color` feature [src: style.rs:185; cargo info]. `Color`: `Reset`, the 16 named ANSI colours, `Rgb(r,g,b)`,
`Indexed(u8)` [src: style/color.rs:72–126]; the `palette` feature adds conversions from the `palette` crate [cargo info].
The `Stylize` trait gives `.bold()`, `.dim()`, `.reversed()`, `.fg(...)` on strings, spans, lines and widgets
[src: style/stylize.rs].

We use named ANSI colours only, all in `palette.rs`, guarded by the test `no_colour_outside_the_palette`
[tui/src/palette.rs]. That is a strength: named colours follow the User's terminal theme, so the cockpit matches pi's chat
pane next to it. **Unused:** `underline_color`, `ITALIC`, `CROSSED_OUT`, `Rgb`/`Indexed`.

### 2.5 Stateful widgets and their states

`StatefulWidget { type State; fn render(self, area, buf, state) }` [src: ratatui-core widgets/stateful_widget.rs:124].
Built-in states: `ListState`, `TableState`, `ScrollbarState` [src: ratatui-widgets list/state.rs, table/state.rs,
scrollbar.rs]. Third-party: `tui_tree_widget::TreeState` (opened set, selection, offset, click hit-testing), which the
Context column uses [tui/src/context.rs]. Our own selection states (`flow::Selection`, the Worker view's `scroll`, the
chart offset) live as plain fields in `main.rs` loops, not as widget states.

### 2.6 Custom widgets

- `Widget { fn render(self, area, buf) }`; implement it for `&MyWidget` to render by reference [src: ratatui-core
  widgets/widget.rs:29–70]; ratatui itself does this for `&Paragraph` and `&Block` [src: paragraph.rs:408, block.rs:805].
- `WidgetRef` / `StatefulWidgetRef` (`render_ref(&self, …)`, boxable as `dyn WidgetRef`) are **unstable**, behind
  `unstable-widget-ref` [src: ratatui-0.30.2 src/widgets.rs:140–175]. We do not need them: `impl Widget for &T` covers it.
- We have one custom `Widget` use (rendering a `Line` into the buffer) and one hand-written blit
  (`flow::Chart::blit`, commented: "ratatui has no widget that copies a window of a `Buffer`") [tui/src/flow.rs].

### 2.7 Terminal features

| Feature | ratatui / crossterm | In cpd-tui now |
| --- | --- | --- |
| Mouse | `EnableMouseCapture` turns on modes 1000, 1002, **1003 (any motion)**, 1015, 1006 [src: crossterm-0.29.0 src/event.rs:327–333]; so `MouseEventKind::Moved` (hover) is already delivered | clicks and wheel only [tui/src/main.rs:459, 697–724, 946] |
| Focus | `EnableFocusChange` → `Event::FocusGained/FocusLost`; tmux forwards them with `focus-events on` | yes: the keyboard cursor shows only with focus [tui/src/main.rs:79, 413; scripts/tmux.conf:3] |
| Colour depth | `Rgb` is emitted as 24-bit SGR; the terminal or tmux maps it down | named ANSI only. `tmux.conf` sets `default-terminal "tmux-256color"` and no `RGB`/`Tc` terminal feature [tui/scripts/tmux.conf:4–6]; **guess:** RGB colours would be downsampled to 256 inside the cockpit. Another reason to keep named colours |
| Keyboard | crossterm key events; kitty "extended keys" through tmux `extkeys` [scripts/tmux.conf:6] | plain keys |
| Unicode width | `unicode-width 0.2.2`; cells hold graphemes [src: ratatui-core buffer/cell_width.rs] | see the glyph table in 4.6: several of our glyphs are East-Asian **Ambiguous** width |
| Init | `ratatui::init`, `try_init`, `init_with_options`, `run(|terminal| …)`, `restore` [src: ratatui-0.30.2 src/init.rs:318–524] | `init`/`restore` |

### 2.8 Testing

`TestBackend::new(w, h)`, `with_lines`, `assert_buffer`, **`assert_buffer_lines`**, `assert_cursor_position`,
`assert_scrollback_*` [src: ratatui-core backend/test.rs:71–233]; `assert_buffer_eq!` [src: buffer/assert.rs:8];
`Buffer::with_lines` builds an expected buffer, styles included. Snapshot crates: `insta 1.49.0`
[cargo info insta], 2 973 stars, pushed 2026-10-10 [gh api mitsuhiko/insta]; the ratatui docs recommend `insta` with
`TestBackend`'s `Display` (**unverified** in this Job).

We test with `TestBackend` everywhere, but each test module re-defines `fn text(buf, y)` to read a row
[tui/src/tabs.rs, main.rs tests], and the chart snapshot is a hand-kept fixture [tui/tests/fixtures/cpd2-chart.txt].
`assert_buffer_lines` (and `Buffer::with_lines(...).set_style(...)` for colours) replaces the helpers.

### 2.9 What we use and what we underuse

- **Used well:** `Layout`/`Constraint`, `Paragraph`, `Table` (legend), `Line` alignment, `TestBackend`, focus events,
  `tui-tree-widget`.
- **Underused, with a concrete place:**
  - `Scrollbar`: the Worker view (a long log with no position) and the chart (scrolls on both axes, no hint that more exists).
  - `List`/`ListState` or `Paragraph::scroll` + `ScrollbarState`: the Worker view clamps its own scroll [tui/src/main.rs:909–950].
  - `Table` with `Constraint::Fill`: the Dispatch PR row, which counts `used` and `room` by hand [tui/src/pr.rs:131–160].
  - `Layout::spacing` + `Length`: tab hit areas [tui/src/tabs.rs:22–45].
  - `Block::padding` / `Padding::horizontal(1)`: the `" homeserver"`, `" PRs"`, `" cpd ▸ "` insets.
  - `Flex::Center`: centring the legend and empty states.
  - `TestBackend::assert_buffer_lines`.
- **Not needed now:** `Canvas`, `Chart`, `BarChart`, `Calendar`, `Shadow` (it is the 3D look the User rejected, see 4.1).

---

## 3. The ecosystem

Versions and ratatui compatibility from `cargo info -v <crate>` (the line `+ratatui@…` or `+ratatui-core@…`; crates built
on `ratatui-core 0.1` / `ratatui-widgets 0.3` are the 0.30 generation). Stars and last push from `gh api repos/<repo>`
(`stargazers_count`, `pushed_at`), 2026-10-15. All sources are present in `$REG/` (fetched by `cargo info`).

| Crate | Version | ratatui | Repo, stars, last push | Fit for cpd |
| --- | --- | --- | --- | --- |
| **ratcn** | 0.0.6 | 0.30.2 (+ ratatui-textarea 0.9.3) | kristoferlund/ratcn, 167★, 2026-10-08 | Ideas yes, crate no (below) |
| **tuirealm** / tui-realm-stdlib | 4.1.0 / 4.1.0 | 0.30 | veeso/tui-realm, 1 005★, 2026-07-29 | No: a framework that owns the loop |
| **tui-widgets** (umbrella) | 0.7.12 | core 0.1 | ratatui/tui-widgets, 239★, 2026-10-14 | Pick single crates, not the umbrella |
| tui-popup | 0.7.7 | core 0.1 / widgets 0.3 | (tui-widgets) | Not now: tmux `display-popup` already floats the legend over the whole cockpit, which an in-pane popup cannot |
| tui-scrollview | 0.6.8 | core 0.1 / widgets 0.3 | (tui-widgets) | Maybe for the chart: it renders into an off-screen buffer and scrolls it, which is what `flow::Chart::blit` hand-writes. Worth a spike |
| tui-prompts | 0.6.8 | core 0.1 / widgets 0.3, crossterm 0.29 | (tui-widgets) | Overlaps our prompt; heavier than `tui-input` |
| tui-big-text | 0.8.10 | core 0.1 / widgets 0.3 | (tui-widgets) | No: big text costs rows |
| tui-cards | 0.3.6 | core 0.1 | (tui-widgets) | No: it draws **playing cards** [src: tui-cards-0.3.6/src/lib.rs], not UI cards |
| tui-scrollbar | 0.2.8 | core 0.1 | (tui-widgets) | No: ratatui's `Scrollbar` is enough |
| **tachyonfx** | 0.25.2 | core 0.1.2 | ratatui/tachyonfx, 1 308★, 2026-09-06 | No (motion is "none", 4.8). Effects: `fade_to/from`, `dissolve`, `coalesce`, `sweep_in/out`, `slide_in/out`, `hsl_shift`, `paint`, `lighten/darken`, `sequence`, `parallel`, `ping_pong`… [src: tachyonfx-0.25.2/src/fx/mod.rs] |
| throbber-widgets-tui | 0.11.1 | 0.30 | arkbig/throbber-widgets-tui, 54★, 2026-08-19 | No for now; sets BRAILLE_*, CLOCK, ARROW, QUADRANT_BLOCK… [src: src/symbols.rs]. Our running icon `◐` is static on purpose |
| tui-textarea | 0.7.0 | **0.29.0**, crossterm 0.28 | rhysd/tui-textarea, 513★, **2024-12-01** | No: not on 0.30, stale |
| ratatui-textarea | 0.9.3 | core 0.1.1 / widgets 0.3.1 | ratatui/ratatui-textarea, 97★, 2026-10-08 | The maintained successor; multi-line, more than we need |
| **tui-input** | 0.15.5 | 0.30.2, crossterm 0.29 | sayanarijit/tui-input, 207★, 2026-09-26 | **Adopt if** the inline prompt needs ←/→, Home/End, Ctrl-W |
| tui-tree-widget (used) | 0.24.1 | core 0.1.0 / widgets 0.3.0 | EdJoPaTo/tui-rs-tree-widget, 129★, 2026-09-01 | Keep (decision 15) |
| rataflow (evaluated) | 0.1.0 | 0.30 | furkankly/rataflow, 186★, 2026-10-14 | Rejected by the User: labels vanish when fitted (decision 18) |
| ascii-dag (evaluated) | 0.11.0 | none (plain text) | AshutoshMahala/ascii-dag, 25★, 2026-10-11 | Rejected: busy (decision 18) |
| ratatui-interact | 0.5.3 | 0.30, crossterm 0.29 | Brainwires/ratatui-interact, 39★, 2026-04-02 | No: focus manager + click regions + 25 components (toast_stack, breadcrumb, hotkey_dialog…) [src: src/components/]; small project |
| rat-widget (rat-salsa) | 3.2.1 | core 0.1 / widgets 0.3 | thscharler/rat-salsa, 64★, 2026-07-03 | No: its own event/focus model |
| ratatui-toaster | 0.1.4 | 0.30.0 | jayanaxhf/gitv (sub-crate) | Small toast engine; **unverified** quality. Our toast is ~40 lines; build |
| tui-overlay | 0.1.2 | core 0.1.0 / widgets 0.3.0 | jharsono/tui-overlay, 50★, 2026-04-07 | No: modal/drawer overlays, not needed under tmux |
| tui-logger | 0.18.3 | 0.30 | gin66/tui-logger, 319★, 2026-08-12 | No: for `log` records, not pi session files |
| tui-term | 0.3.4 | core 0.1.0 / widgets 0.3.0 | a-kenji/tui-term, 232★, 2026-10-02 | Not now: a pseudo-terminal widget; tmux does this for us |
| edtui | 0.11.7 | core 0.1 / widgets 0.3.0 | preiter93/edtui | No: a vim editor widget |
| ratatui-image | 12.0.0-rc.2 | 0.30.1 | ratatui/ratatui-image | No: images (kitty/sixel) under tmux 3.4 are an open point (vision, open points) |
| insta (testing) | 1.49.0 | n/a | mitsuhiko/insta, 2 973★, 2026-10-10 | Maybe for the gallery snapshot; a plain fixture file works too |

Also found by `cargo search ratatui --limit 30` (descriptions only, **not evaluated**): `ratatui-style 0.2.0` ("a CSS
cascade engine for ratatui … pseudo-states"), `ratatui-kit 0.12.0` (a framework), `tuika 0.13.0` (framework: flexbox,
overlays, focus, keymap), `ratatui-cheese 0.7.0` ("Bubbletea-inspired widgets"), `ratatui-comfy-tabs 0.5.12`,
`ratatui-comfy-toaster 0.6.2`, `ratatui-form 0.1.1`.

### 3.1 ratcn in detail

From its source [src: ratcn-0.0.6/src/lib.rs, theme.rs, runtime/engine.rs] and README [gh api repos/kristoferlund/ratcn/readme]:

- **Two halves per component.** Paint-only widgets (`ButtonWidget`, `ListWidget`, `TabsWidget`, …) are plain ratatui
  `Widget`s that "take a theme and some bools". Interactive components (`Button`, `Input`, `TextArea`, `List`,
  `ScrollArea`, `Select`, `Tabs`, `Dialog`, `Tooltip`, `Checkbox`, `Cycle`) add focus, keys, mouse and messages through the
  `Ratcn` engine, entered at `Ratcn::render` and `Ratcn::handle_event(event, &state) -> EventResult<Msg>`
  [src: runtime/engine.rs:1957, 2238].
- **Copy to own.** `cargo ratcn add dialog` copies a component into `src/components/` (shadcn's model).
- **Themes are RGB.** The default theme is `Color::Rgb` throughout (183 `Color::Rgb` in theme.rs), e.g.
  `background Rgb(10,10,10)`, `primary Rgb(229,229,229)` [src: theme.rs:214–221]; `Theme::adaptive` solves a palette from
  the terminal's background and foreground (feature `termina`). Each control has "three fills: at rest, focused, and
  hovered" [src: theme.rs:872].
- **Size and maturity:** 0.0.6, ~50 000 lines of Rust (`wc -l` over src), 167★.

**Fit:** its engine solves problems we do not have (several focusable controls in one screen, hover, in-process layers),
and conflicts with decisions we have (named terminal colours that follow the theme; keyboard-only cursor, no hover; tmux
owns focus between panes and floats the popup). Its paint-only half is the right *pattern* for our `ui` module.

### 3.2 tui-realm in detail

"A framework … re-usable components with properties and states, as you'd do in React … Messages and Events … `update`
routines as happens in Elm … components are organized inside the View, which manages mounting/umounting, focus and event
forwarding" [src: tuirealm-4.1.0/src/lib.rs]. Traits `Component`, `AppComponent<Msg, UserEvent>`, `Poll` listeners
[src: core/component.rs:30–68, listener/mod.rs:75]. The stdlib has label, input, list, table, select, radio, checkbox,
spinner, gauges, charts, textarea [src: tui-realm-stdlib-4.1.0/src/components/]. Adopting it means rewriting each
view's loop around its `Application`; our views are small loops over tmux state, so the cost buys little.

---

## 4. A visual language for the cockpit

### 4.1 Where we start: the User's decisions and the code

| Principle | Source | In the code |
| --- | --- | --- |
| **Minimalism.** The Context column shows a dot, a name and five numbers; the top row only the Context and tabs | decisions 14, 18 | `context.rs`, `tabs.rs` |
| **One colour per meaning, defined once**; grounding shares white with not started (the one exception) | decision 16 | `palette.rs` + `no_colour_outside_the_palette` |
| **Done is dimmed:** done is `Color::Gray`, "even more subtle than the ones still on todo" | decision 16 | `palette::color(Done)` |
| **One subtle separator:** a dark-grey `─` row, the only thing that separates areas | `separators-2` | `separator.rs` |
| **No default selection:** nothing is selected when a view opens; the first key selects | README, Plan view | `flow::Selection` |
| **Keyboard-only cursor:** drawn reversed only with focus and after an arrow; a click never shows it | decision 14 amendments, README | `cursor.rs` |
| **`⏵` marks the centre**, bold, in the dot column; tabs show the same by reversal | README | `context::ACTIVE_MARK` |
| **Flat, not 3D:** the 3D buttons cost three rows per control (`▄` face, label, `▀` shadow) and were dropped the same day | decision 13 | — |

Lesson from decision 13: **every token is paid for in rows and columns.** A pane is ~20 % of the window, so any
component that adds a row of chrome (borders, shadows, padding rows) must earn it. ratatui's `Block::shadow` is that same
3D look and stays unused.

### 4.2 Colour roles

Keep `Meaning` as the status axis; add a small, separate **UI role** axis, and give every role exactly one colour.

| Role | Colour | Used for |
| --- | --- | --- |
| `NotStarted` | White | queued Job, counter |
| `InProgress` | Green | running Job, phase dispatch, checks pending, live workers |
| `Review` | Yellow | in review / merging, open PR, phase pr |
| `Done` | Gray | merged Job, merged PR, phase done, checks passed |
| `Failed` | Red | failed Job, closed PR, checks failed |
| `Planning` | Magenta | phase plan |
| `Grounding` | White (shared, deliberate) | phase context |
| `Nothing` | DarkGray | zero, draft, separator, lines, no Workflow |
| `Accent` (new) | Cyan | the User's own voice: "you" lines in the Worker view, **and nothing else** |
| `Text` (new) | Reset (terminal default) | body text |

**Finding: two colours carry two meanings today.** `Ui::Note` and `Ui::Tool` are yellow (= review) and `Ui::Agent` is
green (= in progress) [tui/src/palette.rs:97–103]. Proposal (a User decision, so a question in the Job, not a silent
change): notes and tool calls become `Text` + DIM, agent lines `Text`, "you" lines `Accent` cyan, so yellow and green
only ever mean a status. Cyan is free since `running-green`.

Rules: colour is for **status**, never for decoration; a coloured background (black on colour) is **only** the Plan pill.
Stay on the 16 named colours: they follow the terminal theme (so the cockpit matches pi's pane), and the tmux server
advertises 256 colours, not RGB [scripts/tmux.conf:4].

### 4.3 Type roles (modifiers)

| Modifier | Means | Today | Proposal |
| --- | --- | --- | --- |
| **bold** | heading, or "this one" | Dispatch section titles, PR labels, ⏵, selected pill, active tab | `Heading` (section titles) and `Mark` (⏵) |
| **dim** | secondary, metadata, stale, disabled | Context name before tabs, timestamps, `(future)`, stale PR rows, `workers` label | `Meta` and `Disabled`: one role, DIM |
| **reversed** | "where you are": selection or active | keyboard cursor row, selected pill (with bold), active tab, the ` ? legend ` button | `Selected` (cursor, selected pill, active tab) and `Control` (a clickable label). **Finding:** the legend button is reversed like a selection; propose `Control` = bold, unreversed, so reversal means only "where you are" |
| **underlined** | heading in the legend (bold + underlined) | legend titles only | drop: one `Heading` role (bold) everywhere; underline stays free for a future "hotkey letter" if ever needed |
| **italic** | — | unused | keep unused (fonts vary; **guess:** many terminal fonts render it poorly) |
| crossed out, blink | — | unused | never |

### 4.4 Spacing and density

- **Inset:** 1 column on the left of text in the side columns (today a typed `" "`); one token `INSET =
  Padding::horizontal(1)` applied through `Block::padding`.
- **Chart padding:** 2 columns × 1 row (`Margin`), as today.
- **Rows:** one item per row; no blank rows inside a list; one blank row between chart cards; sections are split by the
  separator, never by blank rows.
- **Gaps:** 1 column between tabs and between inline items; the counters' `⋅` is the separator inside a number group.
- **Density:** compact by default (small panes). No "comfortable" mode (YAGNI).

### 4.5 Borders versus separators

- **tmux draws pane borders and the one pane header** (`pane-border-format`); views draw no outer `Block` border.
- **Inside a view, the only divider is the separator** (`separator.rs`).
- **Rounded corners `╭ ╮ ╰ ╯` only for groups** (the chart's bracketed cards).
- **Popups** are tmux popups (border by tmux, title ` legend `). If an in-pane overlay is ever needed: `Clear` + `Block`
  with `BorderType::Rounded`, no shadow.

### 4.6 Iconography and width safety

East-Asian width from Python `unicodedata.east_asian_width` (run 2026-10-15). "A" (Ambiguous) is one cell in
`unicode-width` and in Western terminals, but two cells in a terminal set to treat ambiguous as wide (a CJK setting).

| Glyph | Use | EAW |
| --- | --- | --- |
| `○` | queued; Context dot "does not need you" | A |
| `◐` | running | A |
| `◉` | in review | N |
| `✓` | merged; checks passed | N |
| `✗` | failed; checks failed | N |
| `◌` | draft | N |
| `●` | **only** "needs you" | A |
| `⏵` | the centre session | N |
| `⋅` | counter separator | N |
| `…` | truncation; checks pending | A |
| `–` | no checks | A |
| `⇥` | "merging" (decision 20, queued) | N |
| `▸` | between the Context name and its tabs | N |
| `─ │ ╭ ╮ ├ ┤ ┼ ┬ ┴` | separator and chart lines | A |

Rules: one glyph, one meaning (`…` is both "cut" and "pending" today: acceptable, different places, but note it);
no emoji (two cells, colour fonts); the gallery Job adds a test that every glyph token measures 1 with `unicode-width`.
Box-drawing being Ambiguous is universal in TUIs and not worth avoiding.

### 4.7 Interaction states

| State | How it shows | Rule |
| --- | --- | --- |
| Rest | plain | — |
| **Focus** (pane has keyboard focus) | nothing in the view; tmux's active border | the cursor may show only with focus |
| **Cursor / selected** | reversed full row (tree), bold + reversed (pill) | keyboard-only; never on open; Esc clears |
| **Active** ("this is the centre") | `⏵` in the tree, reversed tab | separate from selection |
| **Hover** | none | decided by the keyboard-only cursor; crossterm already reports `Moved` (mode 1003), so this is a choice, not a limit |
| **Pressed** | none | 3D rejected (decision 13) |
| **Disabled** | DIM | e.g. `(future)` sections, a refused action |
| **Stale** | DIM + age (`3m ago`) | PR rows when `gh` fails |
| **Needs you** | `●` | the only attention signal |

### 4.8 Motion

**None.** Views redraw on a 250 ms poll [tui/src/main.rs]; a spinner or tachyonfx effect would mean four processes
redrawing continuously for no information. Change is shown by state (icon, colour), not by animation. If the User ever
wants one: a single static-to-static change (e.g. `◐` → `✓`) is enough.

### 4.9 What good TUIs do that we could adopt

Stars and last push from `gh api` (2026-10-15). **The feature notes are from general knowledge of these tools, not
verified in this Job.**

| TUI | ★ / last push | What it does well | Adopt? |
| --- | --- | --- | --- |
| lazygit | 83 185 / 2026-10-14 | a one-row key-hints bar for the focused panel; `?` opens all keys for the context; the focused panel's border is highlighted | yes: our `?` legend could list keys per view; a hints row **only with focus** |
| k9s | 34 815 / 2026-10-14 | colour = status per row; a dense header of context facts; skins in one file | yes: we already do status colour; keep the one-file palette |
| btop | 34 988 / 2026-10-14 | titles sit **in** the border line; braille graphs; mouse-clickable labels | partly: a `Sparkline` for homeserver load; titles in the separator line (`Block::title` on the separator) |
| gitui | 22 574 / 2026-10-06 | a bottom command bar that greys out unavailable commands | yes: hints row with DIM for unavailable keys |
| helix | 46 591 / 2026-09-29 | minimal chrome; a which-key popup after a prefix key | yes: the legend popup is our which-key; keep chrome minimal |
| yazi | 42 821 / 2026-10-10 | Miller columns; consistent icons; which-key | the three columns already echo it |
| lipgloss (Charm) | 11 928 / 2026-10-11 | styles as values (padding, margin, border are style properties); adaptive light/dark colours | yes: our tokens as `const`/fns returning `Style`, `Padding` |
| bubbles (Charm) | 8 988 / 2026-10-04 | each component is a model with update/view; a `help` component built from the key bindings | yes: generate the keys hint row and the legend's keys from one key table |
| Textual | 37 463 / 2026-10-09 | design tokens (`$primary`, `$surface`…); snapshot testing of the whole screen | yes: tokens + a gallery snapshot test |

---

## 5. A component library for cpd-tui

### 5.1 Pattern

Every component is a **paint-only** ratatui widget: a small struct of props (data + state flags), `impl Widget for &T` (or
`StatefulWidget` when it scrolls or selects), styles only from `ui::tokens`. Behaviour (keys, clicks) stays in the views;
a component that is clickable returns or exposes its hit `Rect`s. This is ratcn's paint half, without its engine.

```rust
// ui/tokens.rs: the only file with Color:: (today's palette.rs, extended)
pub enum Meaning { NotStarted, InProgress, Review, Done, Failed, Planning, Grounding, Nothing }
pub enum Role { Text, Accent, Heading, Meta, Mark, Selected, Control }
pub fn style(r: Role) -> Style;               // e.g. Heading => bold, Meta => dim, Selected => reversed
pub const INSET: Padding = Padding::horizontal(1);
pub mod glyph { pub const NEEDS_YOU: &str = "●"; pub const CENTRE: &str = "⏵"; /* … */ }
pub fn ellipsize(text: &str, width: u16) -> Cow<str>;   // the one truncate, by display width
```

### 5.2 Inventory: what exists (from `tui/src`)

| Component | Where now | API sketch | Build / adopt |
| --- | --- | --- | --- |
| **Palette / tokens** | `palette.rs` (`Meaning`, `color`, `fg`, `pill`, `Ui`, `job_status`, `phase`, `pr_state`, `checks`) | `ui::tokens` as above; status mappings stay | build (move) |
| **Separator** | `separator.rs::draw(f, area)` | `struct Separator { title: Option<Line> }`, `impl Widget`; `HEIGHT = 1` | build (exists; make it a `Widget`, optional title in the line à la btop) |
| **Status pill** | `flow.rs` (`pill_text`, `pill_parts`) + `palette::pill` | `struct Pill<'a> { icon, key: &'a str, tier: &'a str, meaning, selected: bool }`, `impl Widget` fills its `Rect` (width = the column's) | build |
| **Card / group (brackets)** | `flow.rs` (`Card`, `layout`, `Mask`, `glyph`) | stays inside `flow::Chart`: edges and brackets have no ratatui widget (hand-drawn, already commented) | build (keep) |
| **Flow chart** | `flow.rs` (`Chart`, `layout`, `blit`, `step`, `scroll_to`) | `impl StatefulWidget for &Chart { type State = ChartState { selected: Option<usize>, offset: (u16,u16) } }`; draws a `Scrollbar` per axis when it overflows | build (wrap existing); spike `tui-scrollview` for the blit |
| **Tab bar** | `tabs.rs::draw` → `(row, ranges)` | `struct TabBar<'a> { context: Option<&'a str>, tabs: &'a [Tab] }`; `fn areas(&self, area) -> Vec<Rect>` from `Layout::horizontal(lengths).spacing(1)`; `impl Widget` | build (ratatui's `Tabs` has no hit areas) |
| **Tree row (session / Context)** | `context.rs` (`session_text`, `context_text`, `items`, `draw`) on `tui-tree-widget` | `fn session_item(row, width) -> TreeItem`; overlay layer `RowTrail { counters, active }` as a `Widget` per visible row | adopt (tui-tree-widget) + build the row text |
| **Dot** | `palette::dot` | `struct Dot { needs_you: bool, phase: Option<Phase> }` → `Span` | build (exists) |
| **Counters** | `context::counters`, `palette::count_style`, `palette::separator` | `struct Counters([usize;5], phase)` → `Line`, right-aligned by the caller | build (exists) |
| **Active mark `⏵`** | `context::ACTIVE_MARK` | `glyph::CENTRE` + `Role::Mark` | build (token) |
| **Keyboard cursor** | `cursor.rs::shown` (logic) + `buf.set_style(REVERSED)` in `context::draw` | logic stays; style = `Role::Selected` | build (exists) |
| **Legend** | `legend.rs` (`Section` → `Table`, `size`, `popup_args`) | becomes the gallery's "colours" page; `struct LegendSection { title, header, rows }`, `impl Widget` via `Table` | build (exists) |
| **Legend button** | `main.rs::draw_bottom` (` ? legend `, reversed) | `struct KeyButton<'a> { key: char, label: &'a str, enabled: bool }`, `impl Widget`, `fn width()` for its `Length` | build |
| **Worker indicator** | `workers.rs::Workers::line(width)` (long/short forms) | `struct Indicator<'a> { label, value: usize, limit: Option<u64> }`; picks the form that fits, like now | build (exists) |
| **Inline prompt** | `prompt.rs` (`Prompt::Name`, `Prompt::Close`, `key`, `text`) | `struct PromptLine<'a>(&'a Prompt)`, `impl Widget` (one row, `Role::Meta`, cursor `█`); text editing optionally by `tui_input::Input` | build; adopt `tui-input` only if editing grows |
| **Popup** | tmux `display-popup` running `cpd-tui legend` (`legend::popup_args`) | keep tmux; no in-pane popup component | keep |
| **Worker log view** | `main.rs::render_session` + `run_worker` (`Paragraph` + hand-clamped scroll) | `struct LogView<'a> { lines }`, `StatefulWidget<State = LogState { offset, follow: bool }>`, with `Scrollbar` | build |
| **PR row** | `pr.rs::render` (two `Line`s, hand width arithmetic) | a `Table` row: `[label] / [#n, state, checks, review, Fill(title)]`; stale = `Role::Meta` | build on `Table` |
| **Section title** | `main.rs::draw_dispatch` (`" homeserver".bold()`), legend (bold + underlined) | `struct SectionTitle(&str)` = `Role::Heading` + `INSET` | build |
| **Empty / placeholder** | "no Lead sessions in the cockpit", "no Lead session in the centre", "no Workflow yet", "no Plans in the cockpit", "(future)" | `struct EmptyState<'a>(&'a str)`, dim, inset, top-left (or `Flex::Center` in the chart area) | build |
| **Loading** | "loading… (first fetch)", PR " loading…" | `EmptyState` with the text `loading…`; no spinner | build |
| **Note** | the chart area's note of the last action (`Ui::Note`, yellow) | becomes a `Toast` (below) | build |

### 5.3 What we need next

| Component | Why | API sketch | Build / adopt |
| --- | --- | --- | --- |
| **Keys hint row** | lazygit/gitui; the keys are only in the README today | `struct KeyHints<'a>(&'a [(&'a str, &'a str, bool)])` (key, action, enabled); one row, dim, shown only with focus; cut from the right when narrow; the legend lists the same table | build |
| **Toast / notification** | fork started, close refused, `gh` failed: today a yellow note in the chart area | `struct Toast<'a> { text, meaning: Option<Meaning>, since: Instant }`; one row at the view's bottom, auto-hides after N s; one at a time | build (ratatui-toaster 0.1.4 / ratcn's `ToasterWidget` exist, more than we need) |
| **Badge** | a count or short status next to a label (e.g. PR checks, hidden Jobs) | `struct Badge { text, meaning }` → `Span` (fg colour, no background: the background is the pill's alone) | build |
| **Scroll area** | the Worker view, the chart, a long Context tree | `ScrollbarState` + `Scrollbar` (VerticalRight, track `│` in `Nothing`, thumb in `Text`), shown only on overflow | adopt ratatui `Scrollbar` |
| **Button** | `? legend` today; future Dispatch actions (decision 11: "actions by picking an option") | `KeyButton` above; enabled/disabled; returns its `Rect` | build |
| **Confirm** | `x` close | exists as `Prompt::Close`; becomes `PromptLine` | build (exists) |
| **Sparkline** | homeserver load as a trend | ratatui `Sparkline` | adopt ratatui |
| **Spinner** | — | not proposed (motion none) | — |

### 5.4 The gallery

`cpd-tui gallery` renders every component in every state, from the `ui` module only, like `cpd-tui legend` does for
colours (and opened the same way, in a tmux popup, or run in any terminal). Mock-up, 60 columns:

```
 Separator ─────────────────────────────────────────────────
 Pill        ○ a S1   ◐ b S1   ◉ c S1   ✓ d S1   ✗ e S1   ◌ f S1
 Pill sel.   [◐ b S1] (bold + reversed)
 Dot         ● needs you   ○ idle   (one per phase colour)
 Counters    3⋅1⋅0⋅0⋅12   0⋅0⋅0⋅0⋅0   (plan: drafts first)
 Tree row      ├ ○ v1                         36⋅0⋅0⋅1⋅5
             ⏵ ├ ● cpd rust lead               0⋅1⋅0⋅0⋅1
             [ └ ○ cpd-ui                      1⋅0⋅2⋅0⋅3 ] cursor
 Tabs        cpd ▸  v1  [cpd-ui]  rust
 Button      ? legend    (enabled)   ? legend (disabled, dim)
 Indicator   workers 2/4    2/4 (narrow)
 Prompt      Fork cpd-ui into a new Plan, name: new█ (Enter starts…)
 Toast       ✗ switch to another session first
 Empty       no Workflow yet          Loading   loading…
 Keys        ↑↓ move  ⏎ switch  n fork  x close  ? legend
 Scroll      a 10-line log in a 5-row box with its scrollbar
 Widths      ○ ◐ ◉ ✓ ✗ ◌ ● ⏵ ⋅ … – ⇥ ▸  (each measured 1 cell)
```

One snapshot test pins the whole gallery (`TestBackend` + a fixture, or `insta`); a change to any token shows in one
diff, and the User reviews the gallery instead of four panes.

---

## 6. Architecture options

Our setup: four separate processes (`context`, `plan`, `dispatch`, `worker`) plus `legend`, each a small loop in one tmux
pane; tmux owns pane focus, borders, headers and the popup; panes are small (20 % columns); one shared palette.

| | (a) own `ui` module | (b) ratcn engine | (c) tui-realm | (d) mix: (a) + single crates |
| --- | --- | --- | --- | --- |
| Fit with 4 processes in tmux | each process links the same module; nothing to coordinate | its focus/hover/layers are in-process; across panes tmux does it already | its View manages focus between in-process components; same mismatch | as (a) |
| Fit with named terminal colours | yes (today's palette) | themes are RGB / adaptive [src: ratcn theme.rs] | own props/colours | yes |
| Keyboard-only cursor, no hover | yes | hover is a core state ("three fills") | neutral | yes |
| Cost now | small: move and wrap existing code | rewrite loops around `Ratcn::render/handle_event`; 0.0.6 API churn risk (**guess**, pre-1.0) | rewrite every view around its `Application` | small |
| Dependencies | none new | ratcn + ratatui-textarea (+ termina for adaptive themes) | tuirealm + stdlib + derive | per need (`tui-input`) |
| Reversibility | high | low | low | high |
| Matches "ratatui as much as possible" | yes: plain widgets | wraps ratatui | wraps ratatui | yes |

**Recommendation: (a), growing into (d) only when a component needs a crate** (first candidate `tui-input` for the
prompt; spike `tui-scrollview` for the chart). Reasons: it changes nothing the User decided, adds no dependency, is cheap to
undo, and the gallery gives the User one screen to judge the visual language on. Take ratcn's lessons without the crate:
paint-only widgets driven by tokens and bools, and a showcase (`ssh ratcn.com`) ≈ our gallery.

---

## 7. Proposed Jobs, in order

Sizes: S ≈ one Worker hour, M ≈ two to three, L ≈ more (guesses). All touch only `tui/`.

1. **`ui-tokens`** (S–M)
   - **Goal:** `palette.rs` becomes `ui/tokens.rs`: `Meaning` + status mappings unchanged; new `Role` (Text, Accent,
     Heading, Meta, Mark, Selected, Control) with one style each; `glyph` constants; `INSET`; one `ellipsize` by display
     width replacing the three `truncate`s. Ask the User first about the two findings: notes/tool/agent colours (4.2) and
     the reversed legend button (4.3). The colour guard test moves with it.
   - **Files:** `tui/src/palette.rs` → `tui/src/ui/{mod,tokens}.rs`; imports in `context.rs`, `flow.rs`, `pr.rs`,
     `legend.rs`, `workers.rs`, `tabs.rs`, `separator.rs`, `main.rs`; `tui/README.md` (Palette section).
2. **`ui-components`** (M)
   - **Goal:** the existing pieces become `Widget`s in `ui/`: `Separator`, `Pill`, `Dot`, `Counters`, `TabBar` (hit areas
     from `Layout::spacing`), `KeyButton`, `Indicator`, `SectionTitle`, `EmptyState`, `PromptLine`. Views call them;
     rendering stays byte-identical (existing tests pass unchanged, except the shared `fn text` helpers replaced by
     `assert_buffer_lines`).
   - **Files:** `tui/src/ui/*.rs` (new), `separator.rs`, `tabs.rs`, `context.rs`, `flow.rs`, `workers.rs`, `prompt.rs`,
     `main.rs`, `tui/README.md` (Conventions).
3. **`ui-gallery`** (M)
   - **Goal:** `cpd-tui gallery` draws every component in every state (5.4), opened like the legend; a snapshot test of
     the whole gallery; a width test for every glyph token. The User reviews the visual language here.
   - **Files:** `tui/src/ui/gallery.rs` (new), `tui/src/main.rs` (subcommand), `tui/tests/fixtures/gallery.txt` (new),
     `tui/README.md`.
4. **`ui-ratatui-native`** (M)
   - **Goal:** use ratatui where we hand-make today: `Scrollbar` + state in the Worker view (`LogView`) and on the chart
     overflow; the Dispatch PR rows as a `Table` with `Constraint::Fill` for the title; `Block::padding(INSET)` for
     insets; `Flex::Center` for the legend and empty states. Optional spike: `tui-scrollview` instead of `Chart::blit`.
   - **Files:** `tui/src/main.rs` (worker, dispatch), `tui/src/pr.rs`, `tui/src/flow.rs`, `tui/src/legend.rs`,
     `tui/src/ui/*.rs`.
5. **`ui-feedback`** (S–M, after the User reviews the gallery)
   - **Goal:** the new components the views need: `KeyHints` (one dim row, only with focus; the legend lists the same key
     table), `Toast` (replaces the chart-area note for action outcomes), `Badge`; each added to the gallery first.
   - **Files:** `tui/src/ui/{hints,toast,badge}.rs` (new), `tui/src/ui/gallery.rs`, `tui/src/main.rs`,
     `tui/src/legend.rs`, `tui/README.md`.

---

## Open questions

- **For the User:** should notes, tool calls and agent lines stop using yellow and green (4.2)? Should the ` ? legend `
  button stop being reversed, so reversal means only "where you are" (4.3)? Is a keys hint row (only with focus) wanted, or
  should keys live only in the legend?
- **Unverified here:** whether tmux 3.4 with `tmux-256color` downsamples RGB (moot while we use named colours); the TUI
  comparisons in 4.9 (from general knowledge); ratcn's API stability (0.0.x).
- **Not done:** no rendered screenshots or scratch-server check (this Job changed no views); `tui-scrollview` as the chart's
  scroller needs a spike; the `cargo search` finds `ratatui-style`, `tuika`, `ratatui-cheese` were not evaluated.
