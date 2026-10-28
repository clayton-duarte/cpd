# ratatui chat: what to take instead of write (crates survey)

Plan `ratatui-chat-research`, Job `crates`, Attempt `crates-a1`. Written 2026-10-28 14:58 EDT to 15:30 EDT, on macOS (Darwin arm64, 14 cores), `cargo 1.99.1 (5f94df478 2026-08-27)` (**ran**: `cargo --version`).

This is one of six evidence files of `docs/research/ratatui-chat.md`. It answers "what can we take instead of write" for the chat's components. The component analysis is in `ratatui-chat-widgets.md` (another Job) and is not repeated.

Labels: **read** = a file and line I opened, **ran** = a command I ran (its output is quoted or summarised next to it), **unverified** = inferred, with why. Registry sources are the crates unpacked by `cargo` into `~/.cargo/registry/src/*/` (cited as `<crate>-<version>/path:line`).

## 1. Summary

- **Take, with an exception to the closed list:** `ratatui-textarea` 0.9.3 (editor), `tui-markdown` 0.3.12 (markdown, with its default `highlight-code`), `ansi-to-tui` 8.0.1 (ANSI). Exception texts are in section 8.
- **Do not add a crate for:** scrolling (ratatui has `Paragraph::scroll`, `Scrollbar`; the repo has `tui/src/scroll.rs`), images (`ratatui-image` costs 88 crates new to the lock and its default build fails here without `libchafa`; defer).
- **Three findings the synthesis must carry:**
  1. `Paragraph::line_count` is behind the unstable feature `unstable-rendered-line-info` (**read** `ratatui-widgets-0.3.2/src/paragraph.rs:328-332`). Enabling it changes the `ratatui` entry's features, which the closed list forbids without an exception; the alternative is the repo's own wrapper (`tui/src/scroll.rs:84`, **read**).
  2. `tui-markdown` has no streaming API and a fixed syntax set that lacks TOML, TypeScript, TSX, JSX and Dockerfile (**ran**, section 6.2). `ansi-to-tui` drops everything after an OSC 8 hyperlink that ends in `ESC \` (**ran**, section 6.4).
  3. Bracketed paste arrives as `Event::Paste(String)` only if `EnableBracketedPaste` is written; `tui/src` never writes it (**ran**: `git grep -n 'BracketedPaste' -- tui core cli | wc -l` printed `0`), and neither `ratatui-textarea` nor `tui-input` converts `Event::Paste` (section 6.1).
- Facts in the existing repo docs that I checked: `audit-shell.md` F2 (on `origin/main`) already recommends `ratatui-textarea` and says it adds "one crate to the lock"; this file's measurement agrees (**ran**, section 4: 1 name new to `Cargo.lock`). Its "build time not measured" is filled in below.

## 2. The rule and how an exception is justified

- `Cargo.toml:9-11` (**read**): "Every dependency of cpd 2, declared once here. Later Jobs may not add any: the list and the reasons are in docs/implementing.md."
- `docs/implementing.md:381` (**read**): "Later Jobs may not add, remove or change dependencies or features ... If you need something not listed, ask the Lead." The table of crates, feature and "Why" is at `docs/implementing.md:385-401` (**read**); `docs/implementing.md:403` records the two earlier exceptions: `sha1` removed, and `notify` "added after the TUI Job, by decision of the User in Plan sync".
- So an exception is: a decision of the User or the Lead (cited), one row in the table `docs/implementing.md:385-401` with crate, version, features and the reason, and the entry in `[workspace.dependencies]` (and `tui/Cargo.toml` as `x.workspace = true`). Section 8 gives the row text for each recommended crate. Enabling a feature on an existing entry (`ratatui`) is "change features" and needs the same.
- Axiom (`~/.pi/agent/AGENTS.md`, "Convention over configuration", **read**): take the dependency, pinned; never vendor or fork what upstream owns.
- `Cargo.lock` of the repo at this worktree: 272 `[[package]]` entries, 260 unique names (**ran**: `grep -c '^name = ' Cargo.lock`; `grep '^name = ' Cargo.lock | sort -u | wc -l`). It has no `syntect`, `pulldown-cmark`, `onig`, `ratatui-textarea` (**ran**: `grep -n 'name = "syntect"\|name = "pulldown-cmark"\|name = "onig"' Cargo.lock` printed nothing).

## 3. ratatui 0.30.2: what covers the need

Source: `cargo generate-lockfile` in a scratch crate with `ratatui = "0.30"` resolved to `ratatui 0.30.2`, `ratatui-core 0.1.2`, `ratatui-widgets 0.3.2`, `ratatui-crossterm 0.1.2`, `crossterm 0.29.0` (**ran**: `grep -A1 'name = "ratatui"' Cargo.lock`). The registry paths below are under `~/.cargo/registry/src/*/`.

| Need | ratatui item | Stable? | Source (**read**) | Notes |
|---|---|---|---|---|
| Scroll a `Paragraph` | `Paragraph::scroll((y, x))` | stable | `ratatui-widgets-0.3.2/src/paragraph.rs:233` | order is (y, x), documented `paragraph.rs:225-228`; the design RFC for scrollable widgets is linked at `paragraph.rs:230-231` |
| Wrap | `Paragraph::wrap(Wrap { trim })`, `Wrap` | stable | `paragraph.rs:122` | word wrapper in `reflow.rs:31` |
| Wrapped height of a paragraph | `Paragraph::line_count(width)` | **unstable**: `#[instability::unstable(feature = "rendered-line-info", issue = ".../issues/293")]` | `paragraph.rs:328-332` | feature `unstable-rendered-line-info` (`ratatui-0.30.2/Cargo.toml:137`, `ratatui-widgets-0.3.2/Cargo.toml:68-69`); also in the umbrella feature `unstable` (`ratatui-0.30.2/Cargo.toml:127-131`); `line_width` is gated the same (`paragraph.rs:385-388`) |
| Scrollbar | `Scrollbar`, `ScrollbarState::content_length/position` | stable | `ratatui-widgets-0.3.2/src/scrollbar.rs:83`, `:145`, `:445`, `:433`; `impl StatefulWidget` `:501` | the scrollbar has no scrolling state of its own beyond a position |
| Display width of text | `Line::width`, `Text::width` | stable | `ratatui-core-0.1.2/src/text/line.rs:441`, `text/text.rs:289` | the repo already uses it: `tui/src/cells.rs:10` (**read**) |
| Wide glyphs | the wrappers drop a grapheme wider than the line | stable | `reflow.rs:92-94`, `reflow.rs:296-298` | a CJK glyph in a 1-column area is silently skipped, not wrapped |
| Inline viewport | `Viewport::Inline(u16)` | stable | `ratatui-core-0.1.2/src/terminal/viewport.rs:62`, `:99` | draws in the main screen, below existing output |
| Insert lines above the viewport (native scrollback) | `Terminal::insert_before(height, draw_fn)` | stable, **Inline viewport only** (a no-op for other viewports, test `inline.rs:492`) | `ratatui-core-0.1.2/src/terminal/inline.rs:109`; two paths `:130` and `:228` | the `scrolling-regions` feature (not in the default set: `ratatui-0.30.2/Cargo.toml:74-80` lists `all-widgets, crossterm, layout-cache, macros, underline-color`) selects the flicker-free path; the cockpit's views use the alternate screen (`tui/src/view_ports.rs:131-136` leaves it), so `insert_before` is a design choice, not a given |
| Bracketed paste | `crossterm::event::Event::Paste(String)` | stable, needs `EnableBracketedPaste` | `crossterm-0.29.0/src/event.rs:562` (variant), `:421` (command); parse `src/event/sys/unix/parse.rs:821` | variant is `#[cfg(feature = "bracketed-paste")]`, a **default** feature of crossterm (`Cargo.toml:49-53`); **ran** `cargo tree -e features -i crossterm@0.29.0` shows `crossterm feature "bracketed-paste"` pulled by `ratatui-crossterm`'s default |
| Mouse capture and wheel | `Event::Mouse`, `MouseEventKind::ScrollUp/ScrollDown/Down/Drag/Up` | stable | `crossterm-0.29.0/src/event.rs:550`, `:800-812` | the cockpit already enables it: `Capture::Mouse`, `tui/src/view_ports.rs:131-134` (**read**) |
| Panic and exit restore | the cockpit's own `Restore` guard | n/a | `tui/src/view_ports.rs:117-140` | it turns off mouse and focus reporting and leaves the alternate screen, but has no paste case (**read**); adding `Capture::...Paste` is own code |

What this means: scrolling and the scrollbar need no crate. Wrapped heights need either the unstable feature (an exception to "change features") or the repo's own `wrap` (`tui/src/scroll.rs:84-130`, **read**; 187 lines in the file, `timeout 20 wc -l tui/src/scroll.rs`). Paste needs about one line in `Restore` and one in the view setup (unverified: my estimate; the `Capture` enum is `tui/src/runner.rs:20`).

## 4. The crates (facts)

How the facts were obtained (all **ran**):

- Version, publish date, downloads, licence: `timeout 30 curl -s -H 'User-Agent: cpd-research' https://crates.io/api/v1/crates/<name>` (`crate.max_stable_version`, the version's `created_at` and `license`, `crate.downloads`). Downloads are the all-time total at 2026-10-28 15:00 EDT.
- Last commit: `timeout 30 gh api repos/<owner>/<repo> --jq .pushed_at` (the last push to any branch; `two-face` lives on codeberg, not asked).
- ratatui it needs: `curl .../crates/<name>/<version>/dependencies`, entries named `ratatui*`, `tui`, `crossterm`.
- Tree and new crates: one scratch crate per candidate in `/tmp/cr1/s/<name>`, with `ratatui = "0.30"` plus the candidate; `timeout 100 cargo tree --prefix none 2>/dev/null | sed 's/ (\*)$//' | sort -u | wc -l` (the "tree lines" column, which counts name+version and includes the scratch crate itself; the ratatui-only baseline is **71**). "Delta" = lines not in the baseline (`comm -13`), "new to the lock" = names not in the repo's `Cargo.lock` (`comm -23` against its package names; a second version of a crate already in the lock is not counted, so it undercounts duplicates).
- Build time: `CARGO_PROFILE_DEV_DEBUG=0 timeout 600 cargo build --timings -q` from a clean `target/`, wall seconds, in the scratch crate (dev profile; dependencies at opt-level 0, not the repo's opt-level 1 of `Cargo.toml` `[profile.dev.package."*"]`; empty `main`). Single runs on a laptop: the same crate varied by up to 8 s between two runs (`syntect` 17 s and 9 s; `pulldown-cmark` 14 s and 9 s), and the ratatui-only baseline was 5-10 s. Read every time below as "small, inside the noise", and **unverified** as a ranking. CPU time and the repo's profile were not measured.

| Crate | Version (stable) | Published | Downloads | Last push | Licence | ratatui it needs | Delta crates | New to lock | Build wall (s) | Covers | Lacks |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `tui-textarea` | 0.7.0 | 2024-10-22 | 2,912,286 | 2024-12-01 | MIT | `ratatui ^0.29` (optional; `crossterm ^0.28`) | 16 (second `ratatui 0.29.0`, `crossterm 0.28.1`) | 3 | 6 | multi-line editor, undo, selection | maintained, ratatui 0.30, soft wrap (no wrap code: **ran** `grep -c 'WrapMode\|soft_wrap' src/*.rs` printed no match) |
| `ratatui-textarea` | 0.9.3 | 2026-10-05 | 972,530 | 2026-10-08 | MIT | `ratatui-core ^0.1`, `ratatui-widgets ^0.3` | 1 | 1 | 6 | editor, soft wrap (4 modes), undo/redo, selection, word motions, search (`search` feature), line numbers | `Event::Paste`, Shift+Enter policy, history recall, completion |
| `tui-textarea-2` | 0.13.2 | 2026-08-23 | 207,862 | 2026-10-26 | MIT | `ratatui-core ^0.1` (optional; dev `ratatui ^0.30`) | 2 (`portable-atomic`) | 1 | 8 | the same, soft wrap (`src/wrap.rs`) | as above; community fork (`srothgan/tui-textarea`) |
| `edtui` | 0.11.7 | 2026-08-16 | 363,279 | 2026-10-24 | MIT | `ratatui-core ^0.1`, `crossterm ^0.29` | 56 (default features `arboard`, `syntax-highlighting` = syntect) | 36 | 11 | vim modal editor (`EditorMode::Normal/Insert/Visual/Search`), handles `Event::Paste` | not a chat input model; 10,169 lines |
| `tui-input` | 0.15.5 | 2026-09-26 | 2,340,858 | 2026-09-26 | MIT | `ratatui ^0.30.2` (optional) | 1 | 1 | 7 | single-line input with word motions and a yank buffer (`src/input.rs:155-161`) | multi-line, undo (0 matches of `undo` in `src/input.rs`), paste |
| `rat-text` | 3.1.0 | 2026-03-08 | 80,511 | 2026-10-28 | MIT/Apache-2.0 | `ratatui-core ^0.1`, `ratatui-widgets ^0.3`, `ratatui-crossterm ^0.1` | 26 (`rat-event`, `rat-focus`, `rat-cursor`, `rat-reloc`, `rat-scrolled`, `ropey`, `chrono`, ...) | 19 | 8 | text area with `TextWrap` (`text_area.rs:61-62`), undo buffer (`undo_buffer.rs`), mask input, date/number input | a framework (the rat-salsa widgets share its event and focus traits); 18,711 lines |
| `tui-markdown` | 0.3.12 | 2026-10-19 | 585,428 | 2026-10-26 | MIT OR Apache-2.0 | `ratatui-core ^0.1` | 43 (default `highlight-code`: syntect with onig, plist, yaml-rust, bincode); 12 without default features | 23 (8 without) | 10-12 (10 without default features) | markdown to `Text`: tables with box drawing and width wrap, lists, quotes, headings, links, code fences, footnotes, definition lists, task lists | streaming, OSC 8 links, a syntax set choice (section 6.2) |
| `ratatui-markdown` | 0.3.6 | 2026-05-21 | 8,940 | 2026-10-26 | MIT OR Apache-2.0 | `ratatui ^0.29` (normal) | 56 (second `ratatui 0.29.0`, `crossterm 0.28.1`) | 19 | 11 | markdown widgets | does not match ratatui 0.30; 8,940 downloads |
| `pulldown-cmark` | 0.13.5 | 2026-10-21 | 189,188,932 | 2026-10-28 | MIT | none | 5 | 4 | 9-14 | CommonMark+GFM event parser (pull parser; offsets) | any rendering |
| `comrak` | 0.59.0 | 2026-10-26 | 9,633,266 | 2026-10-26 | BSD-2-Clause | none | 59 | 26 | 12 | CommonMark+GFM AST | rendering to `Text` |
| `termimad` | 0.35.5 | 2026-09-23 | 8,917,378 | 2026-09-28 | MIT | none (its own crossterm output) | 24 | 14 | 13 | markdown to terminal; `FmtText` wrapped to a width (`src/skin.rs:368`) | writes escape codes itself (`write_in_area`, `print_*`, `skin.rs:389-410`); no `Text` |
| `ratskin` | 0.3.1 | 2026-01-19 | 10,002 | 2026-10-18 (monorepo `mdfried`) | MIT | `ratatui ^0.30` | 25 (termimad) | 15 | 8 | termimad output as ratatui `Line`s (`src/lib.rs:15-36`) | 10,002 downloads; tied to termimad |
| `syntect` | 5.3.0 | 2025-09-27 | 36,249,821 | 2026-10-27 | MIT | none | 30 default (onig, C); 29 with `default-fancy` (pure Rust) | 13 (11 pure) | 9-17 | TextMate/Sublime grammars; stateful per-line `HighlightLines::highlight_line` (`src/easy.rs:74`) | grammars beyond its 75 defaults (section 6.3) |
| `two-face` | 0.5.2+bat-0.26.1 | 2026-08-07 | 13,726,659 | codeberg, not asked | MIT OR Apache-2.0 | none | 22 | 10 | 8-12 | bat's syntaxes (220) and themes for syntect; `syntect-fancy` or `syntect-onig` feature (`Cargo.toml:51-61`) | uses syntect; no widget |
| `tree-sitter-highlight` | 0.27.1 | 2026-10-08 | 4,532,759 | 2026-10-28 | MIT | none | 16 | 4 | 8 | incremental tree-sitter highlighter (`Highlighter` `src/highlight.rs:136`) | a grammar crate and a highlights query per language: `HighlightConfiguration::new(language, name, highlights_query, ...)` (`src/highlight.rs:352-356`) |
| `synoptic` | 2.2.9 | 2024-11-30 | 2,334,406 | 2024-11-30 | MIT | none | 9 | 4 | 7 | regex-rule highlighter | stale (no push for 11 months), grammars are hand-written rules |
| `tui-scrollview` | 0.6.8 | 2026-09-24 | 513,713 | 2026-10-28 | MIT OR Apache-2.0 | `ratatui-core ^0.1`, `ratatui-widgets ^0.3` | 1 | 1 | 6 | scroll a widget larger than the area, with scrollbars (`src/scroll_view.rs:63`) | stores the whole content in one `Buffer` (`scroll_view.rs:63-66`, `:90`) |
| `ansi-to-tui` | 8.0.1 | 2026-01-10 | 7,483,075 | 2026-10-10 | MIT | `ratatui-core ^0.1` | 4 (`nom`, `memchr`, `simdutf8`) | 2 | 7 | ANSI SGR (16, 256, true colour, bold, underline...) to `Text` (`into_text()`) | OSC 8 with `ESC \` (section 6.4) |
| `ratatui-image` | 11.1.0 stable (12.0.0-rc.3 pre-release, 2026-10-18) | 11.1.0 | 1,240,831 | 2026-10-18 | MIT | `ratatui ^0.30.1` | 111 with `default-features = false, features = ["crossterm", "image-defaults"]` | 88 | 14 | Kitty, Sixel, iTerm2, halfblocks (`ProtocolType` `src/picker.rs:72-77`) | default features fail to build without `libchafa` (below) |
| `md-tui` | 0.11.1 | 2026-10-25 | 48,479 | not asked | **AGPL-3.0-or-later** | n/a | not built | n/a | n/a | a markdown TUI application | licence rejects it at once |

Build-failure of the default `ratatui-image` (**ran**): `cargo build` in a scratch crate with `ratatui-image = "11"` (and with `=12.0.0-rc.3`) exits 101: "failed to run custom build command for `ratatui-image v11.1.0`", the build script probing `CHAFA_*`/`PKG_CONFIG`; the default features are `image-defaults, crossterm, chafa-dyn` (`ratatui-image-11.1.0/Cargo.toml:49-53`, **read**) and `chafa-dyn` links `libchafa` (README line 141-143, **read**). The row above is the build without it (exit 0, 14 s). The deltas of the crates that name two ratatui versions are **ran** (`grep -A1 'name = "ratatui"' Cargo.lock` in the `tui-textarea` and `ratatui-markdown` scratch crates gives `0.29.0` and `0.30.2`); the tree of the others has one `ratatui-core` (`grep -c 'name = "ratatui-core"' Cargo.lock` = 1 for all 21).

## 5. Existing own code to compare (**ran**: `timeout 20 wc -l`)

| File | Lines | What it is |
|---|---|---|
| `tui/src/fold.rs` | 130 | the fold widget |
| `tui/src/input_box.rs` | 103 | the labelled input box, wrapped single-text body (no multi-line, no undo, no word motion; `audit-shell.md` F2 on `origin/main` says the same of its `Field::key`) |
| `tui/src/scroll.rs` | 187 | `Scroller`, `wrap` (width in cells, per-char style), follow-tail |
| `tui/src/popup.rs` | 59 | popups |
| `tui/src/history/render.rs` | 201 | history rendering |
| `tui/src/chat.rs` | 75 | the chat slot's placeholder |
| `tui/src/cells.rs`, `tui/src/link.rs` | 38, 136 | cell widths; link rows |

Comparables for crate sizes (source lines of the crate, tests and docs included; **ran** `wc -l`): `ratatui-textarea` 5,707 over `textarea.rs`, `history.rs`, `wrap.rs`, `word.rs`, `cursor.rs`, `screen_map.rs`, `scroll.rs`, `highlight.rs`, `widget.rs`, `search.rs`; `tui-markdown` renderer 2,267 over `mod.rs`, `table.rs` (1,532 of them), `code.rs`; `ansi-to-tui` 1,247 over 5 files; `tui-scrollview` `scroll_view.rs` 1,109.

## 6. How each candidate handles the points that matter

### 6.1 Editor: `tui-textarea`, `ratatui-textarea`, `tui-textarea-2`, `tui-input`, `edtui`, `rat-text`

I ran a probe (`/tmp/cr1/probe`, `timeout 300 cargo run -q`) against `ratatui-textarea 0.9.3`: `set_wrap_mode(WrapMode::WordOrGlyph)`, `insert_str("a\r\n日本語b\r\nc")`, `undo()`, `redo()`, `move_cursor`.

| Point | `ratatui-textarea` 0.9.3 | Evidence |
|---|---|---|
| Multi-line | yes, `lines()`, `cursor()` | **ran**: after the insert `lines=["a", "日本語b", "c"] cursor=DataCursor(2, 1)` |
| Soft wrap | `WrapMode::{None, Word, Glyph, WordOrGlyph}`, grapheme wrapping through `unicode-segmentation` | **read** `src/wrap.rs:12-21`, `:5`; `set_wrap_mode` `src/textarea.rs:2382` |
| Wide characters | cursor columns by `unicode_width` per char; the cursor is on char indexes, not graphemes | **read** `src/textarea.rs:22`, `:1069-1074`, `src/screen_map.rs:5`; **ran**: on `日本語b` `End` gives column 4 (four chars) and the selection from `Head` is `((1, 0), (1, 4))`. Grapheme-cluster cursor (emoji with ZWJ) is **unverified** |
| Undo/redo | linear history, default 50 edits (`History::new(50)`, `set_max_histories`) | **read** `src/textarea.rs:225`, `:1890-1895`, `:1603`, `:1629`; **ran**: a multi-line `insert_str` is one undo step: after `undo()` the lines are `[""]` |
| Selection | `start_selection`, `cancel_selection`, `select_all`, `selection_range`, `copy`, `cut` | **read** `src/textarea.rs:1379`, `:1399`, `:1418`, `:2339`, `:1515`, `:1549` |
| Word motions | `WordForward`, `WordEnd`, `WordBack`; classes: whitespace, ASCII punctuation, other (`_` is a word char) | **read** `src/cursor.rs:121-167`, `src/word.rs:1-20`; `delete_word`, `delete_next_word` `src/textarea.rs:1265`, `:1295` |
| Paste | `insert_str` (removes the selection, strips `\r` before `\n`, one undo step); `paste()` pastes the **internal yank**, not the terminal's | **read** `src/textarea.rs:807-815`, `:1360-1366`; **ran** the CRLF insert above |
| Terminal paste event | **not converted**: `impl From<crossterm::Event> for Input` maps only `Key` and `Mouse`; `_ => Input::default()`; `Key::Paste` is termwiz only | **read** `src/input/crossterm.rs:6-16`, `src/input/mod.rs:55-56`. So the chat calls `insert_str(text)` on `Event::Paste` itself |
| Mouse wheel | `Key::MouseScrollUp/Down` scroll | **read** `src/textarea.rs:654-662` |
| Size | 5,707 lines | **ran** `wc -l` (section 5) |

The others, against the same points:

| Crate | Multi-line | Wrap | Undo | Paste event | Why not |
|---|---|---|---|---|---|
| `tui-textarea` 0.7.0 | yes | none (**ran**: no `WrapMode` or soft-wrap code in `src/`) | yes | not mapped (`src/input/mod.rs:55-56` termwiz only) | needs `ratatui ^0.29`: **ran** `cargo tree` shows `ratatui v0.29.0` and `v0.30.2` and `crossterm 0.28.1` and `0.29.0` together; not published since 2024-10-22 |
| `tui-textarea-2` 0.13.2 | yes | yes (`src/wrap.rs`, `src/layout.rs`) | yes (`History::new(50)` `src/textarea.rs:330`) | not mapped (`src/input/mod.rs:55-56`) | works with 0.30 (2 crates); a community fork while the org-owned `ratatui-textarea` exists: axiom "upstream owns" favours the org's |
| `tui-input` 0.15.5 | **no**: one `Value` | n/a | **none** | not handled in `src/backend/crossterm.rs` (**ran**: `grep -n 'Paste'` printed nothing) | single-line (`audit-shell.md` F2 reached the same) |
| `edtui` 0.11.7 | yes | yes (`src/view/line_wrapper.rs:6`) | yes (`src/state/undo.rs:57`, `:69`) | yes: `Event::Paste(text) => self.on_paste_event` (`src/events/mod.rs:60`) | vim modes (`src/state/mode.rs:3-9`: Normal, Insert, Visual, Search) are the wrong model for a chat box; default features add `arboard` and syntect: 56 crates |
| `rat-text` 3.1.0 | yes | `TextWrap` (`src/text_area.rs:61-62`) | yes (`src/undo_buffer.rs`) | handles paste through its own clipboard (`text_area.rs:855`) | 18,711 lines and the `rat-*` event, focus, reloc and scroll crates (26 crates); it is a framework to adopt whole |

Verdict for the editor: `ratatui-textarea`. Own estimate for the same features (selection, undo, word motions, soft wrap, wide characters, multi-line paste): about 1,200 lines (**unverified**: guess from the crate's core files `textarea.rs`, `history.rs`, `wrap.rs`, `cursor.rs`, `word.rs`, `screen_map.rs` = 4,498 lines with their docs and tests); the repo's `input_box.rs` does 103 lines of single-text wrapping.

### 6.2 Markdown: `tui-markdown` (and what it sits on)

Probe: `tui_markdown::from_str` / `from_str_with_options` in `/tmp/cr1/probe` (**ran**, `timeout 300 cargo run -q`).

| Point | `tui-markdown` 0.3.12 | Evidence |
|---|---|---|
| Parser | `pulldown-cmark` 0.13, with strikethrough, task lists, heading attributes, YAML metadata, superscript, subscript, math, footnotes, definition lists, GFM and tables enabled | **read** `src/renderer/mod.rs:14`, `:77-87` |
| Tables | box-drawing borders, column alignment, cell wrap to `Options::table_width` | **read** `src/lib.rs:12-14`, `src/renderer/table.rs` (1,532 lines); **ran**: `\| a \| 日本語 \|` renders as `┌───┬────────┐ / │ a │ 日本語 │ ...` with the wide header cells padded by display width; with `table_width(20)` the cells wrap over four rows |
| Fenced code | styled lines per fence; highlighted when the `highlight-code` feature (default) is on and the language token is found | **read** `src/renderer/code.rs:24`, `:139-150`; **ran**: the `rust` fence is coloured per token (`Rgb(180, 142, 173)` for `fn`) and keeps the ```` ```rust ```` and closing fence lines |
| Highlight engine | `syntect` `SyntaxSet::load_defaults_newlines()`, a **static** (`LazyLock`), `HighlightLines` per fence, then `as_24_bit_terminal_escaped` and `ansi_to_tui` back to spans | **read** `src/renderer/code.rs:10-14`, `:24`, `:120`; no option exists to choose a `SyntaxSet` (**ran**: `grep -n 'SyntaxSet' src/options.rs` printed nothing; the theme is the only choice, `options.rs:84`). Dependency is `syntect = "5"` with default features, so `onig` (C) comes with it (`Cargo.toml:78-80`; **ran** `cargo tree` lists `onig v6.5.3`, `onig_sys v69.9.3`) |
| Unknown language | falls back to `StyleSheet::code`, with a `warn!` | **read** `code.rs:139-144` |
| Links | rendered as `text (url)`, both underlined blue; **no OSC 8 hyperlink** | **read** `src/renderer/link.rs:18-30`; **ran**: `[l](http://x.y)` gives `l (http://x.y)` |
| Raw HTML | stays as literal text | **read** `src/lib.rs:15` |
| Streaming (incremental) input | **none**: `from_str(&str) -> Text` over a complete string; no state to append to. A growing message is re-rendered whole on each chunk. **ran** on prefixes: an unterminated fence (`"text\n\n```rust\nfn ma"`) renders as a closed code block (with an invented closing fence line), and a table cut after `\| 1` renders as a complete table with an empty second cell. Cost of re-rendering a long message on every token: **unverified** (not measured: no synthetic benchmarks) | **read** `src/renderer/mod.rs:53-55`, `:57-62`; **ran** prefixes above |
| Wide characters | measured by display width in tables | **ran** table above |
| Size | 915 lines in `lib.rs`, `options.rs`, `style_sheet.rs`, `code_theme.rs` (`wc -l src/*.rs`), 2,267 in the three core renderer files | **ran** |

The syntax set matters. **ran** (probe, `SyntaxSet::find_syntax_by_token`): syntect's default set has 75 syntaxes and finds `rust json bash sh python sql yaml diff`, but **not** `toml ts typescript tsx jsx dockerfile kotlin swift`; `two_face::syntax::extra_newlines()` has 220 syntaxes and finds all of them. `tui-markdown` cannot use the two-face set because the set is not an option (`code.rs:24`). For a coding agent's chat the missing TypeScript and TOML fences render as unhighlighted code.

Alternatives, against the same points:

| Crate | Verdict |
|---|---|
| `pulldown-cmark` alone | the parser under `tui-markdown`; +4 crates new to the lock. An own renderer on it controls streaming, OSC 8 links, the syntax set, fold boundaries per block. Own size: about 700-900 lines (**unverified**: guess; the tables alone are 1,532 lines in `tui-markdown`, and the guess assumes simple tables without width wrap) |
| `comrak` | AST of CommonMark+GFM, 26 crates new; renders HTML or CommonMark, not `Text`. Reject: bigger than `pulldown-cmark` for the same parse |
| `termimad` / `ratskin` | `termimad` writes terminal escape codes itself (`skin.rs:389-410`); `ratskin` bridges it to `Line`s but has 10,002 downloads and carries termimad's `crokey`, `coolor`, `crossbeam`, `lazy-regex` (`termimad-0.35.5/Cargo.toml:150-173`). Reject |
| `ratatui-markdown` 0.3.6 | needs `ratatui ^0.29`: a second ratatui in the tree (**ran**). Reject |
| `md-tui` | AGPL-3.0-or-later. Reject |

### 6.3 Syntax highlighting: `syntect`, `two-face`, `tree-sitter-highlight`, `synoptic`

| Point | `syntect` 5.3.0 | `two-face` | `tree-sitter-highlight` 0.27.1 | `synoptic` 2.2.9 |
|---|---|---|---|---|
| Incremental / per-line | stateful: `HighlightLines::highlight_line(line, &SyntaxSet)` keeps parse state across lines (**read** `src/easy.rs:43`, `:74`), so streaming a growing fence line by line works without re-highlighting finished lines (**unverified** as measured behaviour; the API shape is **read**) | uses syntect | incremental by tree edit; `Highlighter::highlight` (**read** `src/highlight.rs:136`, `:294`) | rules per line |
| Languages | 75 default syntaxes; no TOML, TypeScript, TSX, JSX, Dockerfile, Kotlin, Swift (**ran**) | 220 syntaxes (bat's), all of those found (**ran**) | none built in: a grammar crate (C, compiled by the crate's build script) and a highlights query per language (`HighlightConfiguration::new(language, name, highlights_query, ...)` **read** `src/highlight.rs:352-356`) | hand-written rules per language |
| Regex engine | `default-onig` (C, default) or `default-fancy` (pure Rust) (**read** `Cargo.toml:51-72`; **ran** builds of both exit 0) | `syntect-onig` default or `syntect-default-fancy` (**read** `two-face-0.5.2+bat-0.26.1/Cargo.toml:51-61`) | none | `regex` |
| Wide characters | styles ranges of `&str`; widths are the renderer's job (**read** `easy.rs:74`) | same | styles byte ranges | same |
| Output for ratatui | none: styles (RGB, bold...) to convert; `tui-markdown` converts through ANSI text (`code.rs:10-14`); about 40 own lines convert `Style` to `ratatui::style::Style` (**unverified**: estimate) | none | none | none |
| Staleness | published 2025-09-27, pushed 2026-10-27 | published 2026-08-07 | pushed 2026-10-28 | **no push since 2024-11-30** |

Verdict: when a markdown crate brings `syntect`, take that. If the chat renders fences itself (own markdown on `pulldown-cmark`), use `syntect` plus `two-face` with the `fancy` regex (29 delta crates for `syntect` `default-fancy`, 22 for `two-face`, **ran**; overlapping, so the sum is not additive: not measured together). Reject `tree-sitter-highlight` (a C grammar crate per language) and `synoptic` (stale).

### 6.4 ANSI: `ansi-to-tui`

Probe: `"...".into_text()` (**ran**, `/tmp/cr1/probe`).

| Input | Result | Evidence |
|---|---|---|
| `ESC[1;31m` bold red, `ESC[38;5;208m`, `ESC[38;2;1;2;3m`, `ESC[0m` | `Red` + `BOLD`, `Indexed(208)`, `Rgb(1, 2, 3)`; reset handled | **ran** |
| `ESC[2K`, `ESC[3A` (erase line, cursor up) | consumed, no text | **ran**: `"a\x1b[2Kb"` gives `ab`, `"a\x1b[3Ab"` gives `ab` |
| `ESC]0;title BEL` | consumed | **ran**: gives `ab` |
| OSC 8 hyperlink ended by `ESC \` (what `git`, `ls --hyperlink`, `gh` print): `"a \x1b]8;;http://x\x1b\\link\x1b]8;;\x1b\\ b"` | **loses everything after the first `ESC ]8;;url`**: result is `"a "`; the `link` text and ` b` are gone | **ran**; cause **read** `ansi-to-tui-8.0.1/src/parser.rs:273`: the OSC branch is `take_till(\|c\| c == b'\x07')`, terminated by BEL only (comment `parser.rs:262-266` says so) |
| `"abc\rX"` | two lines, `abc` and `X`: a bare CR is treated as a newline, not a carriage return | **ran** |
| `"a\tb"` | the tab stays in the span (`"a\tb"`) and is not expanded; the buffer cell for `\t` is the caller's problem | **ran** |
| Size and features | 1,247 lines in 5 files; default features `zero-copy`, `simd` (`simdutf8`) | **ran** `wc -l`; **read** `Cargo.toml:39-44` |

Verdict: take it, and **strip OSC sequences in our code first** (about 20 lines: remove `ESC ] ... (BEL | ESC \)`; **unverified** as an estimate). Own SGR parser instead: about 150 lines (**unverified**: guess) against 1,247 for the crate.

### 6.5 Scrolling: `tui-scrollview`, ratatui's own

- `tui-scrollview` 0.6.8 keeps the content in a `Buffer` of the content's full size: `pub struct ScrollView { buf: Buffer, size: Size, ... }` and `buf: Buffer::empty(area)` (**read** `src/scroll_view.rs:63-66`, `:90`). A chat's history is unbounded and its width changes on resize, so the whole `Buffer` is rebuilt and held (memory and CPU: **unverified**, not measured). It is +1 crate and 1,109 lines. Reject for the history; it suits a bounded popup.
- ratatui's `Paragraph::scroll` + `Scrollbar` (section 3) plus the repo's `Scroller` (`tui/src/scroll.rs:19-82`, **read**) already do follow-tail and wheel. What the chat needs beyond: variable-height blocks (fold) and their row count at a width. That is the repo's `wrap` (`scroll.rs:84`) or `Paragraph::line_count` (unstable, section 3). Own estimate: 200-300 lines beyond `scroll.rs` (**unverified**: guess).

### 6.6 Images: `ratatui-image`

- Protocols: `ProtocolType::{Halfblocks, Sixel, Kitty, Iterm2}` (**read** `src/picker.rs:72-77`). The `Picker` discovers the terminal by querying stdio (`Picker::from_query_stdio`, **read** `src/picker.rs:103`, with a timeout `:131`) and has tmux detection (`picker.rs:116-117`); it needs the terminal to answer the query, which a tmux pane may not forward: **unverified** (not run: needs a tty).
- Cost: **ran**: default features do not build here (needs `libchafa`); without them 111 delta crates (88 new to the lock) because `image-defaults` enables the `image` crate's codecs.
- Whether pi sends images to a chat the TUI must draw is not in this Job's evidence (**unverified**). Verdict: defer; do not add now. If needed later, halfblocks work with no terminal protocol, at the cost of the 88 crates.

## 7. Verdicts

| Crate | Verdict | Reason | What we would still write |
|---|---|---|---|
| `ratatui-textarea` 0.9.3 | **take** | org-owned fork of `tui-textarea`, ratatui 0.30 native (1 crate new to the lock), wrap + undo + selection + word motions; replaces the hand-made input (`audit-shell.md` F2) | `Event::Paste` to `insert_str` and `EnableBracketedPaste` in `Restore` (a few lines); Enter/Shift+Enter policy; history recall; slash-command completion; the box frame (`input_box.rs` 103 lines stays) |
| `tui-textarea` 0.7.0 | reject | second ratatui 0.29 and crossterm 0.28 in the tree; unpublished since 2024-10-22 | n/a |
| `tui-textarea-2` 0.13.2 | reject (fallback) | works, but a community fork next to the org-owned crate | n/a |
| `tui-input` 0.15.5 | reject | single line, no undo | n/a |
| `edtui` 0.11.7 | reject | vim modal; 56 delta crates by default | n/a |
| `rat-text` 3.1.0 | reject | framework with 26 crates and 18,711 lines | n/a |
| `tui-markdown` 0.3.12 | **take**, default features | tables with width wrap, lists, quotes, links, fences, highlight, in one dependency; replaces about 700-900 own lines (**unverified**) | re-render of the growing message per chunk (cost **unverified**); OSC 8 links (it prints `text (url)`); TypeScript/TOML highlighting (none in its set); the fold boundary per block |
| `pulldown-cmark` alone + own renderer | alternative if `tui-markdown`'s gaps bite | full control of streaming, links, syntax set | about 700-900 lines (**unverified**) |
| `comrak`, `termimad`, `ratskin`, `ratatui-markdown`, `md-tui` | reject | section 6.2 | n/a |
| `syntect` (through `tui-markdown`) | **take, indirectly** | the highlight engine behind `tui-markdown`'s default; 13 crates new to the lock with onig (C, builds in the scratch crate: exit 0) | nothing; a fuller set needs the own-renderer alternative with `two-face` |
| `two-face`, `tree-sitter-highlight`, `synoptic` | reject for now / reject / reject | `two-face` only helps the own-renderer alternative | n/a |
| `ansi-to-tui` 8.0.1 | **take** | SGR 16/256/RGB correct (**ran**); 2 crates new to the lock | strip OSC sequences first (about 20 lines); CR and tab policy |
| `tui-scrollview` 0.6.8 | reject | whole-content `Buffer` | n/a |
| ratatui `Paragraph::scroll`, `Scrollbar`, `Viewport`, `insert_before` | already a dependency | section 3 | the follow-tail and the block heights: `scroll.rs` exists (187 lines) |
| `ratatui-image` | reject for now | default build fails here; 88 new crates | n/a |

## 8. Summary table and the exception text

| Component | Take (crate) | Write (own estimate, lines) | Cost of taking: crates new to the lock / build wall | Cost of writing | Recommendation |
|---|---|---|---|---|---|
| Multi-line editor | `ratatui-textarea` 0.9.3 | glue and policy, 100-200 (**unverified**) | 1 / 6 s (`ratatui-textarea` row of section 4; baseline 5-10 s) | about 1,200 (**unverified**; crate core files are 4,498 with tests and docs) | take |
| Markdown to styled lines (tables, fences, links) | `tui-markdown` 0.3.12, default features | streaming re-render and link handling, 100-200 (**unverified**) | 23 (8 without highlight) / 10-12 s | 700-900 on `pulldown-cmark` (+4 crates), **unverified** | take; fall back to `pulldown-cmark` + own renderer when the missing OSC 8, streaming and TS/TOML bite |
| Syntax highlighting | `syntect` (via `tui-markdown`) | 0 | included above (13 of the 23) | 40 lines to convert `Style` plus `two-face` (10 crates) in the alternative | take, indirectly |
| Scrolling | none (ratatui `Paragraph::scroll`, `Scrollbar`) | 200-300 beyond `tui/src/scroll.rs` (**unverified**) | 0 | the same | write on ratatui; no crate. `line_count` needs `unstable-rendered-line-info` (an exception to the features rule) or the repo's `wrap` |
| ANSI to ratatui | `ansi-to-tui` 8.0.1 | OSC stripping, 20 (**unverified**) | 2 / 7 s | about 150 (**unverified**) | take |
| Images | none now (`ratatui-image` 11.1.0 later) | 0 | 88 / 14 s; default features fail to build here | n/a | defer |
| Paste, mouse, alternate screen | ratatui and crossterm (already in the tree) | `EnableBracketedPaste` in `Restore`, a few lines | 0 | a few lines | write |

### Exception text (one row each, for `docs/implementing.md:385-401`, and the User's or Lead's decision cited)

These are proposals for the synthesis to carry; I have not edited any `Cargo.toml` or `docs/implementing.md` (out of scope).

- **`ratatui-textarea` 0.9.3** (no feature changes; `crossterm` default): "the chat's multi-line input: cursor, selection, undo, word motions, soft wrap by grapheme and a multi-line `insert_str` for a terminal paste, in the ratatui organisation's own crate for ratatui 0.30. It adds one crate to the lock (ran: 1 name new to `Cargo.lock`). Writing it ourselves is about 1,200 lines (unverified) of what `ratatui/ratatui-textarea` already ships and maintains (pushed 2026-10-08, 972,530 downloads)."
- **`tui-markdown` 0.3.12** (default features, so `highlight-code`): "the chat's markdown to styled lines: tables with width wrap, lists, quotes, links and fenced code with syntax highlighting, as `ratatui_core::text::Text`. It adds 23 crates new to the lock (ran), including `syntect` with its `onig` C build, which compiled in a scratch crate on macOS arm64 (ran: exit 0). Writing the renderer on `pulldown-cmark` is about 700-900 lines (unverified), 1,532 of the crate's lines being tables alone. Known gaps, accepted: no streaming (re-render per chunk), links as `text (url)`, no TOML or TypeScript highlighting."
- **`ansi-to-tui` 8.0.1** (default features: `zero-copy`, `simd`): "tool output carries SGR colour codes; the crate converts 16, 256 and RGB colour and bold or underline to `Text` (ran). It adds 2 crates new to the lock (ran). Our own SGR parser would be about 150 lines (unverified). The crate drops the rest of an input after an OSC 8 link ended by `ESC \` (ran, `parser.rs:273`), so the chat strips OSC sequences before calling it."
- **`ratatui` feature `unstable-rendered-line-info`** (only if the chat takes `Paragraph::line_count` instead of `tui/src/scroll.rs:84`): "wrapped row count of a paragraph at a width, so a block's height is known before drawing; gated `#[instability::unstable]` (`paragraph.rs:328-332`), tracking issue ratatui#293, which can change in a minor release." Recommended against: the repo's `wrap` already counts rows in cells.

## 9. Cleanup (**ran**)

The scratch crates, their `target/` directories, the probe and the registry downloads' working notes live in `/tmp/cr1` and were deleted at the end (`rm -rf /tmp/cr1`; `ls /tmp/cr1` then fails). `~/.cargo/registry` is cargo's shared cache and was left. `git status --short` at submit shows only this file.
