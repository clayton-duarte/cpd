# Audit: loop shell, dialogs, input box and tmux strings against ratatui 0.30.2, tmux 3.8 and the ecosystem

Job `audit-shell`, Plan `ui-widgets` (hs-d597 (5), hs-d598). Research only: no code, test, doc or dependency was changed, and every
finding goes to the User before anything is fixed. Code read at `origin/main` c2953181 (it holds `Invert pill and selection text`,
#291, so the worktree is the newest code). Upstream read in `~/.cargo/registry/src/*/`: ratatui-0.30.2, ratatui-core-0.1.2,
ratatui-widgets-0.3.2, ratatui-textarea-0.9.3, tui-input-0.15.5, tui-prompts-0.6.8; tmux is 3.8 on this machine (`tmux -V`),
`man tmux` read for `display-popup`, `display-menu`, `command-prompt`, `confirm-before`, formats and styles.

Area: `tui/src/runner.rs`, `cockpit.rs`, `lib.rs`, `chat.rs`, `input_box.rs`, `dialog/*`, and (the strings and layout only)
`core/src/host/tmux.rs`, `tmux/*.rs`, `layout.rs`. I also read `merge/mod.rs`, `widgets/legend.rs` (its popup) and
`engine/src/watch/workers.rs` where a finding led there; they are named in the finding.

Crate facts were fetched from crates.io on the day of the audit (`curl -A cpd-research .../api/v1/crates/<name>`, 2026-10-23) and the
crate's `Cargo.toml` read in the local registry. Everything not run or read is labelled `unverified`.

**Experiments** (all outside the tree or reverted with `git checkout`; the tree is clean):
- E1, two scratch tests (one in `input_box.rs`, one in `dialog/view.rs`), run and reverted: the width of what the input box and the
  dialog's `wrap` draw for wide characters (F1).
- E2, private tmux servers (`tmux -L cpdaudita1*`, killed after each run; an inner server attached to by a client running in an outer
  server's pane, whose `capture-pane` shows the inner status line and popups): `#{w:}` against `#{n:}` (F4); a status line whose fill
  is too long (kept, C6); the exit status of `display-popup -E` (kept, C4); `display-popup -e` (F6); a popup larger than the client (C5).
  No server was left running.

## Findings

Ranked by value (what the User sees first, then lines deleted, then risk removed).

### F1. The input box and the dialogs measure text in `char`s: a wide character breaks the box, the wrap and the popup size.

- **Where:** `tui/src/input_box.rs:7-20,33-60,73` (`text_width`, `clip`, `rows` of `len / tw + 1`: every cell is one `char`),
  `tui/src/dialog/view.rs:98-100,170-200,213-231,236-241` (`Field` keeps `Vec<char>` and a char cursor; `frame_top` cuts the title with
  `chars().take`; `wrap` splits at `rest.len() > width` chars; `cut`), `view.rs:318-325` (a button's width is `label.chars().count() + CAPS`),
  `dialog/mod.rs:275-296,330-336` (`popup_size`, `input_popup_size`, `widest`, `confirm`: `chars().count()` as columns).
- **What it does:** every row is laid out as if each character took one terminal column.
- **Why it exists:** unverified: no reason found. It is `str::chars()`. The cockpit already counts columns elsewhere (`tree/rows.rs`
  `width`/`truncate`, see audit-tree F1), so the dialog kit (#128, c8dcbff4, 2026-10-19; the box, #263) did not use it.
- **Evidence (E1, run):** `input_box::lines("Name", 20 × '計画' chars, cursor at end, focused, width 30)` returned three rows of widths
  **30, 50, 30**: the text row is 40 columns of text in a 30-column box (ratatui then clips it: the right wall and the end of the text are lost).
  `dialog::view::wrap("計画" × 20, 20)` returned **2 rows, each 40 columns wide**, for a 20-column body. Names, titles, Plan titles and a
  pasted value are free text (`Validator::Title` allows 200 characters), so this is reachable. The cursor moves by `char`, so a
  combining mark or an emoji sequence takes several presses and can be cut in two (unverified in a run; read from `Field::key`).
- **Upstream:** `Line::width()`, `Span::width()` and `Text::width()` count display columns (ratatui-core-0.1.2 `text/line.rs:441`,
  `text/span.rs:271`) and need no dependency; `Paragraph::wrap(Wrap { trim })` wraps by grapheme width
  (`ratatui-widgets-0.3.2/src/reflow.rs`: `WordWrapper`, `LineTruncator`); `Paragraph::line_count(width)` (`paragraph.rs:332`) gives the
  row count a popup or a scroll bound needs, but it is `#[instability::unstable(feature = "rendered-line-info")]`: it needs ratatui's
  Cargo feature `unstable-rendered-line-info` (`ratatui-0.30.2/Cargo.toml:137`; a feature change, which docs/implementing.md reserves to the Lead).
  `unicode-width` 0.2.2 is in `Cargo.lock` but is not a dependency of `cpd-tui` (a scratch test that named it did not compile).
- **Verdict:** `replace` the char arithmetic with display width: `Line::width` / `Span::width` for the box, the buttons and the popup
  size; a width-aware `wrap` (own, keeping the break rule, or `Paragraph::line_count` under the unstable feature). The text editing
  itself is F2.
- **Cost:** `input_box.rs`, `dialog/view.rs`, `dialog/mod.rs`; about 40 lines changed; tests move: `input_box_tests.rs` (168 lines, ASCII, would
  stay) and `dialog/view_tests.rs` get wide-character cases, `dialog/tests.rs` for the popup size. Risk low. (Same class as audit-tree F1, which
  covers `scroll.rs`, `dialog/scroll.rs`, `menu.rs`, `section.rs`, the Inbox item row: fix them together.)
- **User sees:** a Plan title or a name typed or pasted with CJK or emoji no longer runs out of the box or the popup; ASCII is unchanged.

### F2. The input box's editing is hand-made; `ratatui-textarea` 0.9.3 does it, with soft wrapping by grapheme.

- **Where:** `tui/src/dialog/view.rs:92-140` (`Field::key`: insert, backspace, delete, left, right, home, end), `tui/src/input_box.rs:30-76` (the
  text rows, the cursor cell, the wrap into `len / tw + 1` rows), `dialog/view.rs:356-376` (the window of rows the box shows when the text
  is taller than the popup, `start = (cursor / tw + 1).saturating_sub(avail)`).
- **What it does:** a one-line value that wraps into rows of `width - 6` characters, with a cursor drawn as a `selected` cell. No word
  movement, no Ctrl-W / Ctrl-U / Ctrl-K, no undo, no paste handling beyond dropping control characters (`Field::new` filters control
  characters of `initial` only; a key `Char` is inserted if it is not control).
- **Why it exists:** `5726bc78` (2026-10-22, "One labelled input box in the cockpit's dialog kit (hs-d575)") and `c8dcbff4` (the first `Field`). No
  record that a crate was weighed: unverified: no reason found. (`ratatui-textarea` is in `~/.cargo/registry` and in no `Cargo.toml` of the
  repo; see the note in audit-tree "Checked and kept".)
- **Upstream:** `ratatui-textarea` max_version 0.9.3, updated 2026-10-05, 918,436 downloads, repository ratatui/ratatui-textarea (the ratatui
  organisation, a fork of `tui-textarea`); depends on `ratatui-core` 0.1.1, `ratatui-widgets` 0.3.1, `unicode-segmentation`, `unicode-width`
  (+ `ratatui-crossterm` through its default `crossterm` feature); so it supports ratatui 0.30 (read `Cargo.toml`). Its README lists soft wrapping
  with character and word modes (`WrapMode::{None, Word, Glyph, WordOrGlyph}`, `src/wrap.rs:12`), `set_cursor_style`
  (`textarea.rs:2165`), `input(impl Into<Input>)` (`textarea.rs:283`; key mapping with Emacs-style bindings), undo/redo, 5,753 lines of source.
  Every dependency but itself is already in `Cargo.lock` (`unicode-segmentation` 1.13.3, `unicode-width`, `ratatui-core`, `-widgets`,
  `-crossterm`), so adopting it adds **one crate to the lock**, no new transitive crate, one pin (`ratatui-textarea = "0.9"`), and about
  5.7k lines to compile (build time not measured). `tui-input` 0.15.5 (updated 2026-09-26, 2,297,308 downloads, ratatui 0.30.2 optional):
  one-line, no soft wrap (`visual_scroll` scrolls horizontally): it does not give the growing box. `tui-prompts` 0.6.8 (2026-09-24,
  198,121 downloads; depends on `ratatui-macros`, `itertools`): a `StatefulWidget` text prompt, one line; not the growing box, and it
  brings the state into the widget (hs-d596). Not recommended.
- **What we would change:** keep the box frame and the label pill (Job box-widget's), render a `TextArea` (`WrapMode::Glyph`, cursor style = `palette::selected()`)
  into its inner `Rect`; `Field` becomes a wrapper of `TextArea` that refuses Enter and newlines; delete `Field::key`, `input_box::lines`' text
  rows and the row-window arithmetic. Open question, unverified: the popup height is computed before the popup opens from the value's length
  (`input_popup_size`: `max_len / tw + 1`); `TextArea` has no obvious "rows at width" API (not found in `textarea.rs`; `screen_map.rs` is private), so that count
  stays ours (with F1's width). It also keeps `TextArea` as state inside the View (hs-d595 allows state at view level; hs-d596 says cpd defines no `StatefulWidget`: `TextArea` is rendered as `&TextArea: Widget`, unverified).
- **Verdict:** `replace`, optional: the value is word and undo editing and correct wide-character editing; the cost is a first dependency of this kind and a rework of `input_box_tests.rs`.
- **Cost:** `tui/Cargo.toml`, root `Cargo.toml`, `Cargo.lock`, `dialog/view.rs` (-60 lines), `input_box.rs` (-50), `input_box_tests.rs` (rewritten, 168 lines),
  `dialog/input_tests.rs` (217 lines; key mapping tests move). A dependency decision for the User (docs/implementing.md: Jobs may not add dependencies). Risk medium (the key bindings change: Ctrl-keys now edit).
- **User sees:** Ctrl-W, Ctrl-U, Alt-B/F, undo in the input dialogs; correct cursor movement over wide characters; the box looks the same.

### F3. The `merge` popup is a second, hand-made confirm dialog.

- **Where:** `tui/src/merge/mod.rs:103-166` (`on_key`, `buttons`, `on_mouse`, `cut`, `draw`, `popup_size`), `:168-191` (`sh_quote`, `popup_args`),
  `:196-240` (`newest_named_client`, the store read of the title), `:242-262` (`run`).
- **What it does:** the same question as `dialog::confirm` (a title, two lines, `[Merge]` and `[Cancel]`; Enter/`y` confirms, Esc/`n`/`q` cancels,
  a click on a button), with its own keys, its own buttons, its own drawing, its own popup arguments and its own terminal set-up (F5). The
  difference is what happens after the confirm: it starts `cpd tui action merge …` with `run-shell -b` itself.
- **Why it exists:** the merge popup (hs-d273, hs-d473) came before the kit (#128); `dialog/view.rs`'s header says the kit is "in the `merge` popup's
  style". The buttons of the kit have since become pills (`pill`, `view.rs:205`, #272) and `merge` still draws `[Merge]` / `[Cancel]` as bracketed
  text (`merge/mod.rs:43-44,158-159`): the two have already drifted. unverified: whether that drift is wanted.
- **Upstream:** nothing outside cpd: this is cpd's own duplicate. tmux's own `confirm-before` (man tmux, line 3289) is a status-line prompt `(y/n)`: no
  pill, no wrap, no click; not a fit.
- **Verdict:** `replace` with `dialog::confirm`/`ask` on the client (`On::Client(name)`), the action started by the caller after `confirmed`.
- **Cost:** `merge/mod.rs` (about 150 lines deleted, 25 added: the caller of `popup_args` becomes a call of `confirm`), tests of `merge` for keys and mouse
  (`merge/action_d550.rs`, `marker*.rs` stay), `cli/tests` of the merge popup. Risk low-medium: the popup runs in the Deploy column's click path
  (`cpd tui action`), which waits on `display-popup` already.
- **User sees:** the merge popup looks like every other dialog (title pill, pill buttons, wrapped body); one fewer set of keys to learn.

### F4. The status line's widths: two options the engine computes in Rust, where tmux has `#{w:}`.

- **Where:** `engine/src/watch/workers.rs:21-26,66-76,86-107` (`@cpd_workers_width`, `@cpd_workers_short_width`; `Counts::width`/`short_width`, `chars().count()` of the text,
  `+ CAPS`, set in the same call "so a status line never draws a width that belongs to another text"), `core/src/host/tmux.rs:211-229` (`preview_width!`,
  `version_width!` with `#{n:@cpd_preview}` and `#{n:@cpd_version}`, `long_fits!`, `fill_count!`), and the tests that set the width options by hand:
  `core/src/host/tmux/junctions.rs:15-22`, `tmux/palette.rs`, `cli/tests/session/pill_lines.rs:48-62`, `cli/tests/session/host.rs:509-527`, `cli/tests/tui/tui.rs:138,1045`.
- **What it does:** the status line must know how wide the right pills are to draw the `─` fill (C6: a too-long fill erases the right pill). The engine counts the
  text's characters, stores the count as an option beside the text, and the format reads it.
- **Why it exists:** unverified: no reason found. The comment on `Counts::width` says "every character is one cell"; the workers text has `·` (one cell) and no wide
  character today.
- **Upstream (E2, run on tmux 3.8):** `man tmux`, FORMATS: "`n` expands to the length of the variable and `w` to its width when displayed". Run:
  `@w = "#[fg=colour8]\ue0b6#[reverse]workers 6/6#[noreverse]\ue0b4#[default]"` gives `#{w:@w}` = **13** (the display width, styles skipped; caps and 11 characters) and
  `#{n:@w}` = **62** (style bytes counted); `@c = "漢字ab"` gives `#{w:@c}` = **6** and `#{n:@c}` = **8**. So `#{w:@cpd_workers}` is the text's width, computed by tmux at
  expansion, always of the text it is about.
- **Verdict:** `replace` `@cpd_workers_width` and `@cpd_workers_short_width` with `#{w:@cpd_workers}` / `#{w:@cpd_workers_short}` in `long_fits!` and `fill_count!`, and `#{n:…}` with `#{w:…}`
  for the preview and version text. The constants 12, 14, 3 (the pills' caps and joins) stay: they are cpd's layout. unverified: that `w:` exists in tmux 3.6, the
  oldest `supported_version` (the man read is 3.8's).
- **Cost:** `engine/src/watch/workers.rs` (-20 lines: two options, the four-value state string becomes two), `tmux.rs` (4 macros), the five test files above (the
  width options are no longer set; the expansion is checked with a wide text). Risk low-medium: the format is the one pinned by `pill_lines.rs` (it is compared with a string:
  it moves). The engine step is Plan sync's tick (read by this audit only for its strings; not analysed).
- **User sees:** nothing today; a status line that stays right if a text ever holds a wide character, and an engine with no width to keep in step with its text.

### F5. Two views set the terminal up by hand, a third copy of what `run_view` owns; a panic there leaves mouse reporting on.

- **Where:** `tui/src/dialog/mod.rs:164-190` and `tui/src/merge/mod.rs:242-262` (`ratatui::init()`, `execute!(EnableMouseCapture)`, `show`, `DisableMouseCapture`, `ratatui::restore()`).
  The runner's own doc (`runner.rs:1-5`) says one place owns the set-up, the restore on every way out and the mapping of a terminal failure; `chat`, `legend`, `history`, `plan`, `worker`, `logs`,
  `tree`, `widgets` all go through `run_view`.
- **What it does:** the same steps, except no `Drop` guard: `ratatui::init()` installs the panic hook that restores raw mode and the alternate screen
  (ratatui-0.30.2 `init.rs:524-570`: `try_restore` = `disable_raw_mode` + `LeaveAlternateScreen`, nothing else), so a panic in `dialog`'s or `merge`'s loop leaves mouse reporting enabled in the popup's pty
  (the popup then closes: unverified that it matters in a popup). `ratatui::init()` also panics on a terminal error where `run_view` uses `try_init`.
- **Why it exists:** `dialog/mod.rs:164-168` has a reason for its order: "The arguments are parsed at the edge, before the terminal is asked: a bad items file is refused on any
  stdout"; `run_view` checks the terminal first and then runs `prepare`. `merge::run` has no such reason (it checks the terminal first, reads the store, then sets up): unverified: no reason found.
- **Upstream:** `ratatui::run` (`init.rs:318`) is `init` + closure + `restore`; no mouse, no focus, no `Drop` guard: not a replacement (see C1). The replacement is cpd's own `run_view`.
- **Verdict:** `replace` both with `run_view(name, Capture::Mouse, prepare, body)`. For `dialog`, `prepare` parses the kind and the order of the two refusals changes (a bad items file on a
  non-terminal gives `not_a_terminal` instead of `args_invalid`) unless `run_view` gets an "arguments first" variant: a choice for the User.
- **Cost:** `dialog/mod.rs` (-15), `merge/mod.rs` (-15); tests: `dialog/tests.rs` for the refusal order. Risk low.
- **User sees:** nothing; a terminal that is left clean in the two popups whatever happens.

### F6. Three popup builders, two `sh_quote`s and two newest-client parsers.

- **Where:** `dialog/mod.rs:238-264` (`sh_quote`, `view_command`, `popup_args`: `-E -B -w -h`), `merge/mod.rs:168-191` (`sh_quote`, `popup_args`: `-E -c -T -w -h --`), `widgets/legend.rs:297-310`
  (`popup_args`: `-E -T -w -h`), `cockpit.rs:396-399` (`on_client` splices `-c`), `widgets/mod.rs:427-436` (`newest_client`) and `merge/mod.rs:196-216` (`newest_named_client`, the same
  `list-clients` parse plus the name).
- **What it does:** each popup spells its own `display-popup` arguments; the shell-quoting of values into one command line is copied.
- **Why it exists:** each view grew its own popup before the kit (merge, legend: hs-d273; the kit #128). unverified: no reason found for the copies.
- **Upstream (E2, run):** `display-popup -e VAR=value` (`man tmux`, `display-popup`: "-e takes the form ‘VARIABLE=value’") sets an environment variable of the popup's command, verbatim: `-e A='x y"z'` arrived as `A=x y"z`. So the
  title, text and body of a dialog could travel as environment variables (clap reads `#[arg(env = …)]`) and no value would be quoted into a shell line. tmux has no argv form of the command
  (`man tmux`: `[shell-command]`, one string), so the quoting of the program's own words stays. `man tmux` gives `-T` (a title) and `-S`/`-s` (styles) to the popup's own border; the dialogs
  use `-B` and draw their own frame (C7).
- **Verdict:** `replace` the three builders and the two parsers with one `popup(client, size, title?, command)` in `cockpit.rs` (it already owns `on_client` and `client_size`); `-e` for
  free-text values is optional (removes `sh_quote` and the `--name=value` form, changes the internal command line, and `--body` of a long error stops hitting the argument limit: unverified).
- **Cost:** `dialog/mod.rs`, `merge/mod.rs` (see F3: gone with it), `widgets/legend.rs`, `cockpit.rs`, `widgets/mod.rs`; about -60 lines; tests `a_popup_opens_on_the_clicking_client`
  (`cockpit.rs:686`), `dialog/tests.rs`, `legend` popup tests move. Risk low.
- **User sees:** nothing.

### F7. The tmux-string tests run a real tmux server, and a hand-made parser reads back the colours.

- **Where:** `core/src/host/tmux/palette.rs:1-60,103-170` (`Server` starts `tmux new-session -d` on a private socket; `colour`, `style`, `cells` parse the `#[…]` styles of the expansion by hand),
  `tmux/junctions.rs` (the same `Server`, 182 lines), `STATUS_FORMAT.replace("#{client_width}", …)` (`palette.rs:`'s `status`, because a server with no client has no `client_width`).
- **What it does:** each test has a real tmux expand a format and reads cells back, so the tests pin what the User sees ("not how the format spells them", the file's own header).
- **Why it exists:** the header explains the intent. It is also what the pill and junction behaviour could be pinned by without a client.
- **Upstream:** the owner policy (`cpd policy`, `tests`): "No test uses the real tmux or any other real external dependency" (tmux is software we do not control). These tests use it
  (`tmux -L cpd-palette-test`, killed in `Drop`). unverified: whether the policy was meant to cover tests written before it . E2 shows the alternative the policy would not
  accept either: an inner/outer tmux whose `capture-pane` gives the true rendering, with a client.
- **Verdict:** `keep` as the only way to pin tmux's own expansion, but **flag to the User**: it contradicts the written policy; the options are (a) a documented exception for the format tests, (b) a
  mock of the expansion (a format interpreter: the arithmetic `#{e|…}`, `#{R:}`, `#{P/i/:}` would have to be re-implemented, i.e. the test would test the mock). Not a recommendation to change code.
- **Cost:** none until decided. **User sees:** nothing.

## Checked and kept

- **C1. `run_view`'s `Restore` guard** (`runner.rs:50-65,85-90`): ratatui's panic hook and `restore()` leave raw mode and the alternate screen only (`init.rs:524-570`, read); mouse and focus reporting need the guard. `ratatui::run`/`init`/`try_init`/`restore`/`DefaultTerminal` all in use otherwise. (The two views outside it: F5.)
- **C2. `backed_off`'s 3-second wait** (`lib.rs:192`): the `pane-died` hook respawns at once and `man tmux` has no restart backoff (`remain-on-exit`, `respawn-pane` read); keep.
- **C3. `runner::kept` and `one_row`**: cpd policy of its own (the last good snapshot stays); nothing upstream.
- **C4. Dialogs as popups that write an outcome file** (`dialog/mod.rs:218-236`): E2 shows `display-popup -E` **returns the popup command's exit status** (`-E 'exit 3'` returned 3; `-EE 'exit 3'` kept the popup open until
  the timeout) and `Tmux::run` fails on a non-zero status, so a `confirm` could answer by exit code. The `input` value and the `checklist` ticks need the file, and a file that is missing tells "the popup closed some other way, or failed; never a cancel" (`read_outcome`), which an exit
  status cannot (what status a popup closed from outside gives: unverified). Keep one mechanism.
- **C5. The popup size from the client size** (`dialog::popup_size`, `client_size`, `newest_client`): E2: `display-popup -w 500 -h 500` on a 60x10 client prints `height too large` and exits 1; tmux clamps nothing. `-w`/`-h` take a percentage but the dialogs fit their content. Keep (F6 is only the copies).
- **C6. The `─` fill's width arithmetic** (`tmux.rs:225-260`): E2: `#{R:─,300}#[align=right]RIGHTPILL` on a 60-column status line draws the fill and **no right pill**; `#{R:─,10}` draws both. tmux has no "fill with this
  character" style (`fill=colour` fills a background). So the format must subtract the right group; F4 only changes where the widths come from. The `#{P/i/:}` loop for the `┸` junctions has no tmux counterpart (man read).
- **C7. The dialog's own frame with `-B`** (`dialog/view.rs:170-203`, `popup_args`): `man tmux` gives popups a border (`-b`, `-s`, `-S`, `-T` title) but a pill title and the box's spacing (hs-d578) need the cell-level drawing: the frame is Job box-widget's (`see box-widget`).
- **C8. Hit-testing by the last frame's `Rect`s** (`Dialog::area`, `buttons`, `press_mouse`): the documented approach (handoff §5); not reported.
- **C9. `Scroll` and the checklist's `follow_cursor`** (`dialog/scroll.rs`, `view.rs:295-310`): `List`/`ListState` has `select_next`, `scroll_down_by` and keeps the selection visible (`ratatui-widgets-0.3.2/src/list/state.rs:177,248`,
  `rendering.rs:129`), but its offset is in items and its highlight covers the whole item (`rendering.rs:91-107`), where the kit highlights only the header and the logs and legend scroll by rows with the same `Scroll`. `ScrollbarState` has no keys or wheel. Not worth the look change; keep.
- **C10. `chat.rs`**: `Paragraph` + `Wrap` + `Block::padding`, already the upstream idiom; the loop redraws on any event, as the doc says.
- **C11. tmux server options and hooks** (`SERVER_OPTIONS`, `PANE_DIED_HOOK`, `WINDOW_UNLINKED_HOOK`, `STATUS_CLICK`): `range=user|X` + `MouseDown1Status` is tmux's own click mechanism (`man tmux` styles, `range=`); `-gq` on 3.8-only options is deliberate; `pane-died` + `remain-on-exit` has no counterpart. Keep.
- **C12. Splits and `resize-pane` percentages** (`cockpit.rs:195-320`): `select-layout` takes a layout string with a checksum (`man tmux`, `select-layout`); more machinery than a few `resize-pane -x 20%`. Keep.
- **C13. `list-panes -F` with tabs and `tmux -u`** (`tmux.rs:54,401`, hs-d463), `no_server`/`refused`/`dying` reading stderr, `version_key`/`supported_version`: tmux gives no machine-readable error and `#{version}` is already used; `title_case`: not in std. Keep.
- **C14. Input crates I read and do not recommend:** `tui-input` 0.15.5 (one line, horizontal scroll: no growing box), `tui-prompts` 0.6.8 (single-line `StatefulWidget`; `ratatui-macros`, `itertools`), `tui-popup` 0.7.7 and `tui-overlay` 0.1.2 (in-app popups; cpd's popups are tmux's), `tui-scrollview`/`tui-scrollbar` 0.6.8/0.2.8 (cpd draws cues, not bars: hs-d348). Facts: `tui-popup` 0.7.7, 2026-09-24, 1,054,073 downloads; `tui-overlay` 0.1.2, 2026-04-07, 3,353; `tui-scrollview` 0.6.8, 2026-09-24, 507,862; `tui-scrollbar` 0.2.8, 2026-09-24, 1,597,728; `tui-widgets` 0.7.12, 2026-09-24, 86,609. All depend on `ratatui-core`/`-widgets` 0.1/0.3 (ratatui 0.30). `tachyonfx` 0.25.2 (2026-09-06, 507,235) and `throbber-widgets-tui` 0.11.1 (2026-06-19, 1,031,135): spinner and effects are the tree's (audit-tree), not this area.
- **C15. tmux's own `command-prompt` / `confirm-before` / `display-menu`** (`man tmux`, `command-prompt`, `confirm-before`, `display-menu`): a status-line prompt with a `%%` template and a plain menu; neither draws the cpd palette, validates or wraps. Not a fit for dialogs (the menu is audit-tree's).

## Not checked

- `tui/src/cockpit.rs`: `bare`, `tmux_version`, `mark_if_older`, `respawn_old_views`, `server_tmux`, `exec_attach`, `last_chat_lead`, `open_lead` (lines ~410-650: the install-restart and attach flow) were skimmed by name, not read line by line; `build`/`add_missing_panes`/`reset_layout` and `client_size`/`on_client` were read.
- `core/src/host/tmux.rs`: `kill_server`, `Liveness`, `liveness`, `program_gone` (lines ~800-860), `create_session`/`ensure_session` and the `Spawn` arguments (~410-470, 599-636) and the tests from line 1085 were not read in detail; `deadline::output` (`host/deadline.rs`) not opened.
- `core/src/host/tmux/junctions.rs` beyond line 80 (the cases) and `palette.rs` lines 60-95 (`pill` helper): only the harness read.
- `tui/src/dialog/*_tests.rs` (`input_tests.rs`, `view_tests.rs`, `tests.rs`): sizes read, bodies not.
- `dialog/validate.rs`: read the top; the tests not read.
- No tmux popup was driven through a real cockpit client with a User's terminal (only the nested-server experiments of E2), so how a popup closed from outside reports (C4) is `unverified`.
- Plan sync's areas (the read path, `read/follow.rs`, the views' polling and redraw rate, the engine tick) and Jobs pill-spec, box-widget, tree-scroll: not analysed, as the Spec says; the pill caps and assembly in `view.rs:170-210` and `tmux.rs`'s `title_pill!`/`button!`/`pill!` are pill-spec's (`see pill-spec`), the frame and box spacing box-widget's (`see box-widget`).

## Summary for the User

Replace/remove verdicts, in value order (none is applied; each waits for you):
1. F1 (replace): the input box, the dialog wrap and the popup size count `char`s, so CJK text overflows (run: a 30-column box drew a 50-column row; the wrap gave 40-column rows for 20) - fix with `Line::width`, no dependency.
2. F2 (replace, optional): the input box's editing -> `ratatui-textarea` 0.9.3 (2026-10-05, 918k downloads, ratatui 0.30; adds one crate to the lock, its other dependencies are already there): word/undo editing, soft wrap by grapheme, about -110 lines.
3. F3 (replace): the `merge` popup is a second hand-made confirm; use `dialog::confirm` (about -125 lines, one look for every dialog).
4. F4 (replace): the engine's `@cpd_workers_width`/`_short_width` -> tmux's `#{w:@cpd_workers}` (run on 3.8: display width, styles skipped), -20 lines in the engine and nothing to keep in step.
5. F5 (replace): `dialog::run` and `merge::run` hand-roll `run_view`'s terminal set-up (no `Drop` guard: a panic leaves the mouse on); a choice about the order of two refusals.
6. F6 (replace): three popup builders, two `sh_quote`s and two newest-client parsers -> one helper (about -60 lines); `display-popup -e` (run) could drop the quoting.
7. F7 (flag, keep): the tmux-format tests start a real tmux, against the `tests` policy: decide an exception or not.
Kept and checked: C1-C15 (the runner's guard, popup-by-file, popup size, the status fill, hit-testing, tmux options, the list-panes parse). Not reached: see "Not checked".
