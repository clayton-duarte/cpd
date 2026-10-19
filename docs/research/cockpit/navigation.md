# Cockpit research: getting around, mouse-first

Job `insp-navigation` (Attempt 1), Plan `tui-visuals`, PR `inspiration`. Research only: no code changed. It starts from [survey.md](survey.md) (ideas `I-01`…`I-37`) and goes deeper on one pain point, **N: getting around**, plus the pane-model question. Ideas here are `N-01`…`N-13`, ranked by how much they would make the User more efficient.

Pain-point letters, as in the survey: **F** finding what needs him, **P** following progress, **N** getting around, **L** looks and polish.

## 1. Summary

- **Verdict on the pane model: keep tmux panes.** Every project that draws the whole screen in one process pays for a terminal emulator and a server (herdr ships its own `ghostty-vt` crate and a server/client split; bosun 2.0 embeds `vt100` + `tui-term` and documents gaps; zellij is a whole multiplexer). Each of those costs lands on the one pane that matters most here, the chat slot (pi: OSC 8 links, images, extended keys, paste, selection). The interaction polish that a one-app design buys (clickable pane headers, floating panes, menus) is **mostly reachable inside tmux 3.7/3.8**. The one thing tmux cannot give is hover on its own status line. See §3.
- **First action, before building anything from §4:** find out which tmux the User's host runs. The contract names 3.4 (Ubuntu 24.04; `docs/contract.md` §6.1). Header controls and floating panes need **3.7** or **3.8**; everything else here works on 3.4.
- **Top five, by efficiency:** N-01 a needs-you jump strip with a `next ●` button; N-02 a go-to picker that works by mouse; N-03 right-click menus on every object; N-04 a "last Lead" bounce button; N-05 pane headers as toolbars with a breadcrumb.
- **One open decision this research found, not mine to settle:** the StatusBar is moving to tmux's status line, but `docs/commands.md` §17 says views carry no state in tmux options and take no subprocess; a *live* needs-you strip in the status line needs one of the two. See N-01 and §5.

## 2. Method and limits

- Read through `gh` (READMEs, docs folders, source files), the `tmux(1)` man page of tmux **3.8 on this machine**, tmux's `CHANGES`, and tmux source at tags (`3.4`, `3.5a`, `3.6`, `3.7`) and `master`. No web page was fetched. Screenshots cannot be seen: layouts come from text and source.
- **Verified here by running tmux 3.8 on a private socket (killed afterwards):** its default `pane-border-format` carries clickable `[z]`/`[x]` controls, and `list-keys` shows `MouseDown1Control7/8/9` bindings (§4, N-05).
- **Not verified:** that any idea fits the real pane sizes; the User's tmux version; whether Ghostty-over-ssh passes a given sequence; the behaviour of `choose-tree` with cpd's pane options (it needs an attached client). Each such claim says "unverified", "inferred" or "guess".
- Mockups are 215×62 cells, the cockpit's size at build (`docs/commands.md` §13.1), drawn with a script that checks every line is 215 cells wide and uses no double-width glyphs. They draw Nerd Font glyphs as plain ones (`●` `○` `⠹`). `▎`/`▌` marks a hover or selected row (the real views reverse the row).
- Facts about the cockpit come from `docs/commands.md` §13.2, §17 and `docs/contract.md` §6 on this branch.

## 3. The pane model: tmux panes, or one app?

### 3.1 The options

```
 A  today: tmux draws panes, one ratatui view per pane   B  one ratatui app, PTYs embedded     C  zellij
 ┌tree┐┌plan────────┐┌deploy┐                             ┌──────────────────────────────┐    server + client +
 │    ││            ││      │   chat = pi in its own pane │ one process draws tree, plan,│    WASM plugins; panes
 │    │├chat────────┤│      │   (swap-pane keeps its pid) │ deploy AND the chat, which is│    are its own terminals
 │    ││ pi         ││      │                             │ a vt100 grid it renders      │
 └────┘└────────────┘└──────┘                             └──────────────────────────────┘
```

| | A. tmux panes (today) | B. one app, embedded PTY | C. zellij | D. one app, no child terminals |
|---|---|---|---|---|
| Examples | savras, ilmari, the cockpit | herdr, bosun 2.0 | zellij | btop, bottom |
| Who owns the terminal emulation | tmux | the app | zellij | nobody (no child) |
| Mouse across views | each pane its own; tmux routes clicks | one model, hover everywhere | one model, hover on frames and plugins | one model |
| Chrome (headers, menus, floats) | tmux formats, menus, floats (3.7+) | drawn by the app | drawn by zellij, plugin API | drawn by the app |
| Fit for "chat slot = pi" | native | **emulated** | emulated, mature | not applicable |

### 3.2 What owning the terminal costs (all measured on the chat slot)

| Cost | Evidence |
|---|---|
| **A terminal-emulation core.** herdr has its own `crates/ghostty-vt` and a vendored, patched `portable-pty` ([Cargo.toml](https://github.com/herdrdev/herdr/blob/HEAD/Cargo.toml)). bosun uses `portable-pty` + `vt100` + `tui-term` and writes in its manifest that if focus mode "hits vt100's mouse / HVP gaps" the fallback is `alacritty_terminal` ([Cargo.toml](https://github.com/yetidevworks/bosun/blob/HEAD/Cargo.toml)). | pi needs more than a screen grid |
| **OSC 8 links are lost by the usual embed.** In `vt100` 0.18 (HEAD) `osc_dispatch` handles OSC 0, 1, 2 and 52 and sends everything else to `unhandled_osc` ([perform.rs](https://github.com/doy/vt100-rust/blob/HEAD/src/perform.rs)); a search of `cell.rs`, `grid.rs` and `screen.rs` found no hyperlink field (guess: not stored). The cockpit lists `xterm-ghostty:hyperlinks` in `terminal-features` so pi's links survive (`docs/contract.md` §6.1; verified by the User, `docs/commands.md` §13.2 "Links"). | a vt100 embed would need hyperlink tracking added |
| **Titles and images stop at the emulator.** herdr: "Herdr emulates the terminals in its panes, so an `OSC 0`/`OSC 2` title written inside a pane stops at Herdr"; Kitty images are "cropped around" popups and menus ([configuration](https://github.com/herdrdev/herdr/blob/HEAD/docs/next/website/src/content/docs/configuration.mdx)). pi emits images when the terminal allows (`docs/contract.md` §6.1, pi-tui `terminal-image.js`, cited there). | each protocol becomes the app's job |
| **Keyboard protocols.** herdr documents Kitty modes, "Super, Hyper, and Meta chords are dropped when the pane cannot represent them", and layout-dependent Ctrl shortcuts ([keyboard](https://github.com/herdrdev/herdr/blob/HEAD/docs/next/website/src/content/docs/keyboard.mdx)). bosun re-encodes mouse to SGR 1006, handles DECCKM, bracketed paste and modifyOtherKeys, and needs a `Ctrl+V`-then-key chord to send a shortcut through to the agent ([README](https://github.com/yetidevworks/bosun)). The cockpit's pi keys are `extended-keys on` under tmux, "verified by the User" (`docs/contract.md` §6.1). | the User's verified key path is at risk |
| **Selection, copy mode, scrollback.** herdr re-implements copy mode and keeps drag selections "active during output and redraws" (same keyboard doc). savras keeps tmux *because* "drag to select … a URL that wraps comes out as one line … links, scrolling, bracketed paste and focus all work as they do in any tmux, because tmux is doing them" ([README](https://github.com/Marijusaj/savras)). | a second copy mode to get right |
| **Process supervision and several clients.** Leads live in parked tmux windows and are swapped into the chat slot with `swap-pane`, "both processes keep their pids" (`docs/contract.md` §6.3); several clients may attach (§6.2); `cpd tui --respawn` restarts views without touching a Lead (`docs/commands.md` §13.1). herdr needed a background server so "the server owns panes and process state" and the client can detach ([concepts](https://github.com/herdrdev/herdr/blob/HEAD/docs/next/website/src/content/docs/concepts.mdx)). | one-app also means building that server |
| **A class of ssh input bugs.** zellij's changelog: "fragmented STDIN causing 'garbage' input on slow SSH connections" ([CHANGELOG](https://github.com/zellij-org/zellij/blob/HEAD/CHANGELOG.md), unreleased list, #5320). The User attaches over ssh. | bugs tmux already fixed |

btop and bottom are the "no child terminal" case: they show what one app can do for mouse and look, and nothing about the chat slot ([btop README](https://github.com/aristocratos/btop)).

### 3.3 What a one-app design buys, and where tmux already gets there

| Gain | In tmux? |
|---|---|
| Clickable controls in pane headers | **Yes, tmux ≥ 3.7**: `#[range=control\|N]` inside `pane-border-format` and `status-format`, bound as `MouseDown1ControlN`. `style.c` has `STYLE_RANGE_CONTROL` at tag `3.7`, not at `3.6` ([3.7](https://github.com/tmux/tmux/blob/3.7/style.c), [3.6](https://github.com/tmux/tmux/blob/3.6/style.c)); tmux's own default format ships `[z]` zoom, `[x]` kill, `[t]` float ([options-table.c](https://github.com/tmux/tmux/blob/master/options-table.c); default bindings in [key-bindings.c](https://github.com/tmux/tmux/blob/master/key-bindings.c)). Not listed in `CHANGES`. |
| Floating, draggable panes | **Yes, 3.7**; 3.8 adds `move-pane`/`resize-pane` for them, `new-pane -O` modal panes (`-C` closes on a click outside), `new-pane -A` above a zoomed pane ([CHANGES](https://github.com/tmux/tmux/blob/master/CHANGES)). |
| Right-click menus | **Yes**: `display-menu -x M -y M`, disabled items dim when the name starts with `-` (`tmux(1)`; `-M` since 3.5). |
| Mouse picker with preview | **Yes**: `choose-tree` handles clicks, double-clicks, right-click and the wheel ([mode-tree.c](https://github.com/tmux/tmux/blob/master/mode-tree.c)), previews, filters and templates (`tmux(1)`). |
| Draggable scrollbar | **Yes, 3.6**; auto-hide on hover in 3.8 (`CHANGES`). |
| Hover on a **ratatui view** | **Probably yes now** (guess): crossterm's `EnableMouseCapture` writes `?1000h ?1002h ?1003h ?1015h ?1006h` ([commands.rs](https://github.com/crossterm-rs/crossterm/blob/HEAD/src/event/commands.rs)); tmux asks the outer terminal for all-motion when any pane requests it, and forwards moves to the pane (server-client.c, "Set mouse mode if requested": [source](https://github.com/tmux/tmux/blob/master/server-client.c); forwarding to the right pane is inferred, not tested). |
| Hover on the **tmux status line** or a border | **No.** `CHANGES`: "Mouse move keys are not useful as key bindings because we do not turn them on unless the application requests them." |
| Cross-view effects (hover a PR, highlight its box) | **No**, short of an IPC channel between views. |
| One theme and one hit-test layer | No; each view keeps its own. |

### 3.4 Verdict

Stay with A. B/C trade the User's verified key and link path for hover and cross-view effects, which N-09 and N-06 mostly reach without them. Revisit B only if the User asks for effects across views (hover one thing, see it light up in another), and then as bosun does it: one ratatui app for the *sidebar* columns, chat still a tmux pane. That shape is hard here, because the chat slot sits in the middle with chrome on three sides (tmux panes are rectangles), so it would force a layout change. C (zellij) is a one-way door: every tmux command of `docs/contract.md` §6.3, pane options (`@cpd_view`, `@cpd_session`) without a known zellij equivalent (guess), and the engine's death detection. Its web client and its hover are real attractions; they do not outweigh that today.

| Feature | First tmux version | Basis |
|---|---|---|
| status `range=user\|X`, `#{mouse_status_range}` | 3.4 | `CHANGES` 3.3a→3.4 |
| `display-menu -M` | 3.5 | `CHANGES` 3.4→3.5 |
| `pane-scrollbars` | 3.6 (auto-hide 3.8) | `CHANGES` |
| floating panes, `new-pane` | 3.7 (mouse move/resize); 3.8 for `resize-pane`, modal, `-A` | `CHANGES` |
| `range=control\|N` (pane headers and status line) | 3.7 | `style.c` at tags 3.6 and 3.7 |
| `#{A/count:frames}` status animation, `set-hook -B` monitors | 3.8 | `CHANGES` |
| the cockpit's documented baseline | 3.4 | `docs/contract.md` §6.1 |

## 4. Ideas, ranked

Each: **pattern** → **source** → **cpd mapping** → **effort**, then why it ranks where it does. Survey ids (`I-xx`) say what this builds on. "tmux ≥" is the minimum version the idea needs (§3.4).

### N-01 · A needs-you strip with a `next ●` button, in the StatusBar

- **Pattern:** the sessions that wait on you are listed in the status line, each one click (or one number) away; one control walks to the next.
- **Source:**
  - agent-deck: "Waiting sessions appear right in your tmux status bar. Press `Ctrl+b`, release, then press `1`–`6` to jump directly to them" ([README](https://github.com/asheshgoplani/agent-deck#notification-bar)).
  - savras: `ctrl-w`/`ctrl-s` "walk the panel's list, back and forward, from either side of the divider", through "the rows you can see, in the order you see them"; "every row is one press away rather than three" ([README](https://github.com/Marijusaj/savras)).
  - ilmari "publishes optional tmux badge/status fragments" ([README](https://github.com/bnomei/ilmari)).
  - herdr rolls agent states up into the workspace row ("its sidebar state rolls up from the agents inside it") ([concepts](https://github.com/herdrdev/herdr/blob/HEAD/docs/next/website/src/content/docs/concepts.mdx)).
  - tmux: a `range=user|X` segment fires `MouseDown1Status` with `#{mouse_status_range}`; for user ranges the location is `Status`, so `WheelUpStatus`/`WheelDownStatus` fire too ([server-client.c](https://github.com/tmux/tmux/blob/master/server-client.c), `case STYLE_RANGE_USER`); the default wheel binding there is `previous-window`/`next-window` ([key-bindings.c](https://github.com/tmux/tmux/blob/master/key-bindings.c)), which the cockpit does not use (one window shown).
  - Extends survey **I-23** (strip) with the *walk* control and the wheel.
- **cpd mapping (Bar; F, N):** the StatusBar's empty middle (`docs/commands.md` §13.2 "statusbar"). Left to right: `‹ next ● 3 ›`, then one pill per Session whose dot needs the User (`Dot::needs_you()`: red, yellow, magenta, white; §17's rule), each pill `<dot> <plan id>/<why>` in the tree's colours.
  - A click on a pill does what the tree click does: `session open`, then `select-pane` on the chat. `next ●` opens the first needing-you Lead after the current one, **in tree order** (stable, as savras and bosun hold their order; survey **I-27**); `‹`/`›` step; the wheel over the strip steps.
  - The data exists today: the tree's dot rule needs no Inbox. When the Inbox lands (`docs/commands.md` §21), the pills read Inbox items instead; the strip is another view of it, not a second list.
  - A shortcut key is optional (mouse-first). A global key costs the program under it: savras says "Ctrl-W costs you something … it is delete-previous-word". Candidate `M-n`; **unverified** that Ghostty on the Mac sends Option as Meta.
- **Open decision (for the Lead/User):** a *live* strip in tmux's status line needs a writer. `status-interval` is in seconds (default 15; `tmux(1)`), so the line only changes on a redraw. Options: (a) `cpd` sets a tmux user option and runs `refresh-client -S` on each Event, which `docs/commands.md` §17 forbids ("no tmux option carrying state"); (b) `status-format` calls `#(cpd tui status-line)`, a subprocess per redraw, which §17 also avoids for views; (c) keep the StatusBar a ratatui row (a view that follows Events, as today) and handle clicks itself, which keeps hover (N-09) but is not the move to the status line. This research does not choose.
- **Effort:** S for the click map and the walk logic; the data path (above) decides whether it is S or M. **tmux ≥ 3.4** for user ranges; none for option (c).
- **Rank 1:** it attacks the first pain point head-on, and it is the only idea that removes a *scan* (the tree) instead of making it nicer.

### N-02 · A go-to picker that works with no typing

- **Pattern:** one popup reaches anything: a search box, a result list you can click, and filters that are buttons. An empty query is already useful.
- **Source:**
  - herdr's Goto picker: "lists each agent or terminal on its own row, grouped by workspace … Click a row or press Enter to jump directly to it. Press `/` to search … Outside search, `b/w/i/d` filter blocked, working, idle, or done agents … The selected destination's location and path appear below the list" ([keyboard](https://github.com/herdrdev/herdr/blob/HEAD/docs/next/website/src/content/docs/keyboard.mdx)).
  - Textual's palette (fuzzy: typing `th` finds **t**heme) has a header icon `⭘` that opens it on a click, with the tooltip "Open the command palette" ([_header.py](https://github.com/Textualize/textual/blob/HEAD/src/textual/widgets/_header.py), [guide](https://github.com/Textualize/textual/blob/HEAD/docs/guide/command_palette.md)).
  - fzf: mouse is on unless `--no-mouse`; `click-header` and `click-footer` events; `--popup` ([man page](https://github.com/junegunn/fzf/blob/HEAD/man/man1/fzf.1)).
  - ilmari: a tmux popup that "groups panes by workspace, shows each pane's state and recent output, and jumps you back to the selected pane" ([README](https://github.com/bnomei/ilmari)).
  - tmux itself: `choose-tree` takes clicks, right-click, double-click and the wheel ([mode-tree.c](https://github.com/tmux/tmux/blob/master/mode-tree.c)), shows a preview, and runs a `template` on the choice (`tmux(1)`).
  - Extends survey **I-06** (k9s `:`), **I-19** (harlequin palette) and **I-17** (jump mode), which are keyboard-first.
- **cpd mapping (Pop; N, F):** a `⌕` button in the StatusBar (M1) and a `⌕ go to…` row at the tree's foot opens a popup (`cpd tui view jump`, like legend and logs: `display-popup`, §13.2). Two stages:
  - **Spike, S, no new view (unverified):** `choose-tree -w` over the `cpd` session lists one row per parked Lead window (`park-<label>`, `docs/contract.md` §6.2), formatted with `-F` and opened by a `template` that runs `cpd session open` on the window's `@cpd_session`. Not tested: `choose-tree` needs an attached client, and whether `-f` sees pane options at window level is unknown. It lists Leads only, not Plans without a Lead, PRs or Jobs. Use it to learn whether the User reaches for a picker at all.
  - **The view, M:** rows from the same read model as the tree: Contexts, Plans, Leads, forks, PRs, Jobs. **Empty query shows `needs you` first, then `recent`** (from `session-moved` Events), so a click-only User never types. Chips (`● needs you` `⠹ working` `○ idle` `◌ stopped` `PRs`) are buttons. A result click = a tree click (`session open`, focus chat); the destination's Context, phase and PR sit on a footer row, as herdr's path does. Items unavailable now show dim with the reason (survey I-19).
  - A click outside the popup closes it only with **tmux ≥ 3.8** (`new-pane -O -C`, `CHANGES`); on 3.4 the popup needs its own `✕` and Esc.
- **Effort:** S (spike) / M (view). **tmux ≥ 3.4** (`display-popup` is older than 3.4).
- **Rank 2:** every Lead and PR becomes one click away from anywhere, and a long tree stops being a cost. Mockup: M3.

### N-03 · Right-click menus on every object, from one shared widget

- **Pattern:** right-click an object, get a menu of what you can do to it, with unavailable items dim.
- **Source:**
  - zellij: `context_menu` per target, `pane`, `tab` and `bar`, with per-item add, replace and remove in config ([default.kdl](https://github.com/zellij-org/zellij/blob/HEAD/zellij-utils/assets/config/default.kdl)); the feature is in the unreleased list ([CHANGELOG](https://github.com/zellij-org/zellij/blob/HEAD/CHANGELOG.md), #5685).
  - tmux `display-menu`: items are `name key command`; "if the name begins with a hyphen (-), then the item is disabled (shown dim)"; `-x M -y M` opens it at the mouse (`tmux(1)`); tmux's own defaults bind right-click on a pane, on the status line and on an empty area to menus ([key-bindings.c](https://github.com/tmux/tmux/blob/master/key-bindings.c)).
  - lazygit: a click on the repo name in the status panel opens the "Recent repositories" menu; a click on the working-tree state opens the rebase menu ([test](https://github.com/jesseduffield/lazygit/blob/HEAD/pkg/integration/tests/status/click_repo_name_to_open_repos_menu.go)).
  - Builds on survey **I-33** (a Job menu) and **I-19** (availability).
- **cpd mapping (Tree, Plan, Deploy, Pop; N, P):** the tree already has a right-click menu whose every item is a `cpd` command (`docs/commands.md` §13.2). Give the other objects the same, **drawn by one shared widget** (extract it from `tui/src/tree`, so a menu looks and clamps the same everywhere):

  | Object | Items (command it runs) |
  |---|---|
  | Job pill | Open Worker, Copy key, Edit… (`job edit`), Cancel (`job cancel`), Retry (`job retry`; dim unless failed) |
  | PR box title | Open PR ↗, Rebase (`pr rebase`), Open Lead, Copy branch |
  | Deploy PR row | Open Lead, Open PR ↗, Copy URL; dim for a PR no Plan owns |
  | Event log line | Open Plan's Lead, Copy id, Filter to this Plan |
  | Pane header (`⋯`, N-05) | Zoom, Reset layout, Close (tmux `display-menu`) |
  | StatusBar segments | each pill: Open, Open in Worker view, Dismiss |

  Disabled items show *why* in the footer ("Retry: Job is running"), instead of failing after the click (`archive_blocked` and `in_chat_slot` already name the reason, §13.2).
- **Effort:** M. One widget to extract and an items table per object; no engine change. **tmux ≥ 3.4**; header menus are native tmux menus.
- **Rank 3:** it makes every command discoverable by mouse without keys, and it is the one place where cpd's "views only run commands" rule already gives a clean item list.

### N-04 · A `↩ last` button: bounce between the two most recent Leads

- **Pattern:** one control returns to the previous place; pressing it again comes back.
- **Source:** sesh lists "Last session switching - seamlessly bounce between your two most recent sessions" ([README](https://github.com/joshmedeski/sesh)); tmux has the same idea for panes and sessions (`last-pane`, `switch-client -l`, `tmux(1)`); k9s's `-` returns to the previous view (survey, [README](https://github.com/derailed/k9s#key-bindings)).
- **cpd mapping (Bar; N):** a `↩ last` pill next to `‹ next ●` (M1). The previous chat Lead is the second-newest `session-moved` Event with `place: "chat"` (the Plan view already reads it, §17), so it needs **no new state**. Click = `session open <previous>` + focus the chat. Guess: the User flips between two Leads (a Plan and its fork, or two Plans) many times a day; that frequency is inferred, not measured.
- **Effort:** S. **tmux ≥ 3.4** if it lives in the status line (same data-path decision as N-01).
- **Rank 4:** small, constant use. Savras and bosun both chose stable order over most-recently-used for *cycling*; `last` is the one MRU step worth having.

### N-05 · Pane headers as toolbars, with a breadcrumb

- **Pattern:** the title bar of a pane carries its own small controls (zoom, menu, close) and says where you are.
- **Source:**
  - tmux 3.7+: `#[range=control|N]` in `pane-border-format`, bound as `MouseDown1ControlN`. The default format draws `[z]`, `[x]`, `[t]`; the default bindings `MouseDown1Control8` = `resize-pane -Z`, `…9` = a "Kill pane?" `display-menu`, `…7` = float/tile ([options-table.c](https://github.com/tmux/tmux/blob/master/options-table.c), [key-bindings.c](https://github.com/tmux/tmux/blob/master/key-bindings.c)). **Verified here** on tmux 3.8: `show -gw pane-border-format` and `list-keys -T root` print exactly this. When the click lands in a range, tmux reports `Control<n>` instead of `Border` ([server-client.c](https://github.com/tmux/tmux/blob/master/server-client.c)).
  - zellij draws a title frame per pane and lets a right click on a pane or tab open a menu ([default.kdl](https://github.com/zellij-org/zellij/blob/HEAD/zellij-utils/assets/config/default.kdl)).
  - k9s: breadcrumbs at the bottom (`ctrl-g`) (survey §3.3). herdr shows a `ZOOM` pill in the tab row when zoomed ([configuration](https://github.com/herdrdev/herdr/blob/HEAD/docs/next/website/src/content/docs/configuration.mdx)).
  - Builds on survey **I-03** (zoom on a header click) and **I-02** (jump numbers).
- **cpd mapping (tmux; N, L):** `pane-border-format` (`docs/contract.md` §6.1 "pane border format") keeps its reversed title block; add on its right `⤢` (zoom, `resize-pane -Z`), `⋯` (menu) and, on the chat slot, `✕` (the Lead's close, with its y/N as a `display-menu`). The chat header becomes a **breadcrumb** `api ▸ schema ▸ Lead · deploy`: Context, Plan, role, phase. The point is **zoom** (M2): with the chat zoomed the tree is hidden, so the header must say which Lead it is; a click on `api` runs the Context-row click (`context manager`, `session open`, §13.2); on `schema`, the Plan's Lead.
  - The title text is `@cpd_title`, which hs-d257 (dynamic titles) is changing; the controls should be designed together with it and with the powerline look (hs-d266).
  - On tmux 3.4 (no `range=control`), the same click can be built from `MouseDown1Border` plus `#{mouse_x}` arithmetic against `#{pane_left}` (inferred, fragile).
- **Effort:** S on tmux ≥ 3.7 (a format string and three bindings, one per control); M on 3.4. **tmux ≥ 3.7.**
- **Rank 5:** zoom and the pane menu are click-only in a mouse-first cockpit, and tmux already ships the mechanism. Mockups: M1, M2.

### N-06 · Close the gaps: make every named thing clickable

- **Pattern:** anything that names an object is a click target for that object.
- **Source:** lazygit's click tests: the repo name, the working-tree state, a file in the diff stat, a collapse arrow ([integration tests](https://github.com/jesseduffield/lazygit/tree/HEAD/pkg/integration/tests)); herdr: "You can click panes, tabs, workspaces, and agents. You can drag split borders, select text, and use right-click menus. Everything below is also reachable by mouse" ([concepts](https://github.com/herdrdev/herdr/blob/HEAD/docs/next/website/src/content/docs/concepts.mdx)); btop: "all buttons with a highlighted key are clickable" (survey §3.5).
- **cpd mapping (Pop, Bar, Tree; N):** what is clickable today, from `docs/commands.md` §13.2: tree rows, Pulse rows, Deploy rows and `↗`, Job pills, the three StatusBar glyphs. The gaps:

  | Where | Today | Click should |
  |---|---|---|
  | Event log (`▤` popup) | scroll and `q` only | open that Event's Plan Lead (`events.plan` → `plans.lead_session`) and close the popup; right-click → N-03 |
  | `workers 3/4` | text | open the workers popup (survey **I-14**) |
  | Inbox rows (planned, §21) | not built | open the Lead the item belongs to |
  | chat header | text | breadcrumb, N-05 |
  | Context counters `1⋅2⋅0⋅0⋅3` | text | guess: open the Plan's Lead and select its first failed Job; **unverified** the User wants it |

- **Effort:** S per row: each reuses `Actions::open_lead`. **tmux ≥ 3.4.**
- **Rank 6:** each gap is small, but each one is a place the User now has to find the object somewhere else.

### N-07 · Pinned Leads as tabs

- **Pattern:** a strip of tabs for what you are working on now, each with its state glyph; the tree keeps everything.
- **Source:** bosun's browser-style tab strip with status on each pill, "background tabs surface Running / Waiting state without focus" ([README](https://github.com/yetidevworks/bosun)); herdr's tab row ([concepts](https://github.com/herdrdev/herdr/blob/HEAD/docs/next/website/src/content/docs/concepts.mdx)); zellij's tab bar, and tab reordering by mouse in its unreleased list ([CHANGELOG](https://github.com/zellij-org/zellij/blob/HEAD/CHANGELOG.md), #5697).
- **cpd mapping (Bar; N, F):** the User removed the Plan view's tab row on purpose ("No top row, no tabs", hs-d102, `docs/commands.md` §13.2), so tabs do **not** go there. In the StatusBar strip of N-01, after the needs-you pills, show the Leads he has **pinned** (`ⓟ` or a right-click item `Pin`), each with a static state glyph. Needs-you pills are automatic; pins are his working set.
  - Pins must be stored. Views write nothing themselves and change state only through `cpd` commands (§13.2), so a pin needs a command and a place (a new `session pin`, schema included): that is why this is M and not S. Not recommended before N-01 shows the strip is used.
  - tmux cannot animate a spinner at 100 ms in the status line (`status-interval` is in seconds), so pills use static glyphs; 3.8's `#{A/count:frames}` animates frames per redraw (`CHANGES`; rate unverified).
- **Effort:** M (storage and a command). **tmux ≥ 3.4.**
- **Rank 7:** useful once the tree is long; the tree plus N-01 and N-04 cover most of it first.

### N-08 · Peek: see a Lead's question without opening it

- **Pattern:** a preview of the selected item, without changing where you are.
- **Source:** ilmari (state "and recent output" for each pane, [README](https://github.com/bnomei/ilmari)); tmux-sessionx ("fuzzy Tmux session manager with preview capabilities", [README](https://github.com/omerxx/tmux-sessionx)); herdr's Goto footer shows "the selected destination's location and path"; `choose-tree` has `v` to toggle a preview (`tmux(1)`); survey **I-05** (gh-dash preview).
- **cpd mapping (Tree, Pop; F, P):** hover a Lead row for ~400 ms, or right-click → Peek: a card under the row with state, age, and for a needs-you Lead the *question it asked* (the same text savras shows in the row, `WAITING answer: …`). Click = open; the picker (N-02) shows the same card for the highlighted result. Drawn by the tree as an overlay clamped to its 41 inner columns (as its right-click menu is), so one fixed-width card.
  - Data: the Lead's last message needs the Session file read (the Worker view reads one; the tree does not today), or the Inbox item's text once it exists. **Unverified** which is cheaper.
- **Effort:** M. **tmux ≥ 3.4.**
- **Rank 8:** it turns "open each to see which needs me" into one glance, but N-01's pills already name the reason.

### N-09 · Hover feedback and a one-line tip

- **Pattern:** the item under the pointer lights up and a tip names the click.
- **Source:** zellij `mouse_hover_effects` ("frame highlight and help text") and `mouse_hover_tips`, with `Mouse::Hover` delivered to plugins; its session manager and its status-bar plugin handle hover ([default.kdl](https://github.com/zellij-org/zellij/blob/HEAD/zellij-utils/assets/config/default.kdl), [data.rs](https://github.com/zellij-org/zellij/blob/HEAD/zellij-utils/src/data.rs), [session-manager](https://github.com/zellij-org/zellij/blob/HEAD/default-plugins/session-manager/src/main.rs)); Textual widgets style `:hover` and take tooltips ([_header.py](https://github.com/Textualize/textual/blob/HEAD/src/textual/widgets/_header.py)).
- **cpd mapping (all views; L, N):** the ratatui views already capture the mouse with crossterm, which requests all-motion (§3.3), so a `Moved` event probably arrives (unverified in this cockpit): underline the hovered row and show its click in the StatusBar middle (`click: open Lead · right-click: menu`, M1). Hover cannot exist in tmux's status line or on a border (`CHANGES`), so the tip is drawn only if the StatusBar stays a ratatui row (the open decision in N-01). Tooltips are already in `BACKLOG.md` (`docs/commands.md` §13.2).
  - Cost: all-motion mode makes the terminal send an event per cell moved, over ssh; **unverified** that it is noticeable.
- **Effort:** S for row highlight in each view; M for the shared tip line (needs an IPC between a view and the bar, e.g. a tmux user option per pane; §17 allows none for state, but a tip is not state: unresolved).
- **Rank 9:** polish. It makes the cockpit feel alive and teaches the clicks, but it does not shorten a path.

### N-10 · Make the hint line clickable

- **Pattern:** each key hint at the foot is a button.
- **Source:** Textual's `Footer`: each `FooterKey` calls `simulate_key` on `on_mouse_down`, with a tooltip, and a disabled key rings the bell ([_footer.py](https://github.com/Textualize/textual/blob/HEAD/src/textual/widgets/_footer.py)); btop's clickable highlighted letters (survey §3.5); survey **I-01** (lazygit's hint line) is text only.
- **cpd mapping (Bar; N):** survey I-01 fills the StatusBar middle with the focused pane's hints. Make each hint a click target: `n new plan`, `x close`, `↵ open`. A hint runs the view's own key action; cross-pane that needs `send-keys` to the focused pane (guess: fine for the views, wrong for pi, so no hints while the chat has focus).
- **Effort:** S on top of I-01. **tmux ≥ 3.4** (user ranges).
- **Rank 10:** it only helps a User who forgets a key; the mouse already does it.

### N-11 · Floating, non-modal panes for logs, Worker peek and the picker

- **Pattern:** a panel that floats over the layout, can be dragged and resized, and does not block the panes below.
- **Source:** tmux 3.7 floating panes, "unlike popups … not modal and behave like panes"; 3.8 adds mouse drag and resize, `new-pane -T/-B` title and border, modal panes with `-O` and click-outside close with `-C` ([CHANGES](https://github.com/tmux/tmux/blob/master/CHANGES)); fzf's `--popup` switches to a floating pane on tmux ≥ 3.7 ("not modal; you can switch to other panes … move and resize the pane with the mouse") and on Zellij ([man page](https://github.com/junegunn/fzf/blob/HEAD/man/man1/fzf.1)); zellij floating and pinned panes (survey §3.4).
- **cpd mapping (Pop; P, N):** legend and Event log are `display-popup`s, modal in effect: opening `▤` stops him working in the chat. Run `logs` as a floating pane he can leave open while he types (M3b); the same for a Worker peek from a Job pill. Do not move the legend (short-lived, modal is right).
  - Risk: 3.7 floats are "an early release … relatively limited" (`CHANGES` 3.7); 3.8 fixed redraw bugs for them. The contract's tmux baseline is 3.4 (unverified for the User's host).
- **Effort:** M (the `logs` view must size itself to a pane it does not own; a `new-pane` call, a hook to clean up). **tmux ≥ 3.8** (3.7 for the basics).
- **Rank 11:** a clear win for **P** (follow progress while working), but gated by the tmux version.

### N-12 · Drag borders freely, and let `⧉` remember

- **Pattern:** drag a divider to resize; the app keeps where you put it.
- **Source:** tmux's default `MouseDrag1Border` = `resize-pane -M` ([key-bindings.c](https://github.com/tmux/tmux/blob/master/key-bindings.c)); bosun: "Drag the divider → resize the list / preview split. The position is persisted to `config.toml` as `divider_x`" ([README](https://github.com/yetidevworks/bosun)); savras drags its divider and keeps a minimum width ([README](https://github.com/Marijusaj/savras)).
- **cpd mapping (tmux; N, L):** the cockpit runs with `-f /dev/null` (`docs/contract.md` §6.1), so tmux's default drag-resize already works (inferred; not tried). `⧉` resets to 20 %/20 % (§13.2), so a dragged size is lost. Make `⧉` a two-item menu, `Reset to default` and `Use this layout`; the latter stores the current column widths in a tmux server option (layout is "the view's own layout, not Ledger state", §17 "Actions") and reset-layout reads it. Combine with survey **I-10** (presets).
- **Effort:** S. **tmux ≥ 3.4.**
- **Rank 12:** drag already works; this only stops the reset from undoing it.

### N-13 · A scrollbar on the chat and Worker panes

- **Pattern:** a slim scrollbar you can click and drag.
- **Source:** tmux `pane-scrollbars` (3.6) with default bindings `MouseDown1ScrollbarUp/Down` and `MouseDrag1ScrollbarSlider` ([key-bindings.c](https://github.com/tmux/tmux/blob/master/key-bindings.c)); 3.8 auto-hides it and shows it "while scrolling or while hovered" ([CHANGES](https://github.com/tmux/tmux/blob/master/CHANGES)).
- **cpd mapping (Chat, Worker; N):** a long pi conversation is scrolled by wheel today. A draggable slider jumps to "the beginning of this turn" by mouse. **Unverified** whether pi draws in the alternate screen (then there is no tmux history to scroll) and whether the ratatui Worker view, which scrolls itself (§13.2), would show tmux's bar at all.
- **Effort:** S (one option). **tmux ≥ 3.6.**
- **Rank 13:** speculative until the first unverified point is checked.

## 5. Mockups

All 215×62. Rows are numbered from 0 in the notes. Glyphs are plain stand-ins for the Nerd Font ones. `▎`/`▌` marks a hovered or selected row.

### M1 · Today's columns with the navigation layer (N-01, N-02, N-03, N-04, N-05, N-06, N-07, N-09)

Notes, by place:

| Where | What | Idea |
|---|---|---|
| row 0 and row 13, right end of each header | `⤢` zoom, `⋯` menu, `✕` close (chat only); breadcrumb in the chat header `api ▸ schema ▸ Lead · deploy` | N-05 |
| tree, row 4 | `▎` the row under the pointer; a card would open after a pause | N-09, N-08 |
| tree, row 59 | `⌕ go to…` opens the picker (M3) | N-02 |
| Plan view, over the `gql` pill | right-click menu; `Retry (running)` is dim because the Job is running; `Open PR ↗` | N-03 |
| row 61, left | `⧉ ? ▤` as today, plus `⌕` | N-02 |
| row 61 | `‹ next ● 3 ›` walks the needs-you Sessions; `↩ last` bounces between the last two Leads | N-01, N-04 |
| row 61 | needs-you pills (`●`, in the tree's colours), then after `┆` the pinned Leads (`◐` working, `○` idle) | N-01, N-07 |
| row 61, middle | the tip for the hovered object | N-09 |

```
━[ Context ]━━━━━━━━━━━━━━━━━━━━━━━━━━━⋯ ━━┳━[ Plan ]━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━⤢ ⋯ ━━┳━[ Deploy ]━━━━━━━━━━━━━━━━━━━━━━━━━━⤢ ⋯ ━━
                                           ┃                                                                                                                               ┃
 ▾ ● api                                   ┃  ┌─ schema · api → main · #31 open ↗ ─────────────────────────┐    ┌─ web · web → main · #44 conflict ↗ ──────────────────┐   ┃  mbp-dev  load 1.2 0.9 0.8
   ▶ ⠹ schema                   1⋅2⋅0⋅0⋅3  ┃  │ ✓ types │S1 ── ⠹ gql │S2 ── ○ tests │S2                    │    │ ✗ login │S2│a2 ── ○ ui │S1                           │   ┃
▎    ● auth                     0⋅0⋅0⋅0⋅4  ┃  │           ┌──────────────────────┐                         │    │                                                      │   ┃ ──[ PRs ]───────────────────────────────
     ○ schema·7f3a                         ┃  └───────────│ Open Worker          │─────────────────────────┘    └──────────────────────────────────────────────────────┘   ┃
 ▾ ○ web                                   ┃              │ Copy key             │                                                                                         ┃ ▌api/api#31                            ↗
     ● web-ui                   2⋅0⋅0⋅1⋅0  ┃              │ Edit…                │                                                                                         ┃ ▌ open ✓ approved   schema
     ○ docs                     0⋅0⋅0⋅0⋅2  ┃  ┌─ docs · ap├──────────────────────┤─────────────────────────┐                                                               ┃  api/api#27                            ↗
 ▸ ○ infra  (2 plans)                      ┃  │ ○ readme ││ Cancel Job           │                         │                                                               ┃   open ✗ changes    auth
 ▸ ○ tools  (1 plan)                       ┃  │           │ -Retry (running)     │                         │                                                               ┃  web/web#44                            ↗
 …                                         ┃  └───────────│ Open PR ↗            │─────────────────────────┘                                                               ┃   open ✓ no review conflict
                                           ┃              └──────────────────────┘                                                                                         ┃  web/web#40                            ↗
                                           ┣━[ api ▸ schema ▸ Lead · deploy ]━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━⤢ ⋯ ✕ ━━┫   draft ◌ no review docs
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃ ──[ git ]───────────────────────────────
                                           ┃   you    what is blocking schema?                                                                                             ┃  (future)
                                           ┃                                                                                                                               ┃
                                           ┃   agent  Plan review r2 is approved and the Plan is in deploy. Two Jobs are left:                                             ┃ ──[ Jira ]──────────────────────────────
                                           ┃          schema/gql (running, S2, 3m) and schema/tests (queued behind it).                                                    ┃  (future)
                                           ┃          PR #31 has checks passing; it waits for the last Job before merge.                                                   ┃
                                           ┃                                                                                                                               ┃
                                           ┃   → cpd job list --plan schema                                                                                                ┃
                                           ┃   ← 3 jobs: types merged · gql running · tests queued                                                                         ┃
                                           ┃                                                                                                                               ┃
                                           ┃   you    ok, and the conflict on web?                                                                                         ┃
                                           ┃                                                                                                                               ┃
                                           ┃   agent  web-ui has a merge conflict with main on src/login.ts. I can rebase the PR                                           ┃
                                           ┃          branch (cpd pr rebase web) or hand it to you. Rebasing now.                                                          ┃
                                           ┃                                                                                                                               ┃
                                           ┃   → cpd pr rebase web                                                                                                         ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
 ──[ Inbox 3 ]──────────────────────────   ┃                                                                                                                               ┃
 ● schema   review r2 ready                ┃                                                                                                                               ┃
 ● auth     question: which scope?         ┃                                                                                                                               ┃
 ● web-ui   PR #44 conflict                ┃  ┌─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐  ┃
                                           ┃  │ >                                                                                                                       │  ┃
                                           ┃  │                                                                                                                         │  ┃
                                           ┃  └─────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘  ┃
 ⌕ go to…                                  ┃                                                                                                                               ┃
                                           ┃  ~/projects/cpd · claude-opus · 41% ctx                                                                                       ┃
 ⧉ ? ▤ ⌕ │ ‹ next ● 3 › ↩ last │ ▌● schema/review  ● auth/ask  ● web-ui/conflict ┆ ◐ api·schema  ○ web │ click: open Lead · right-click: menu · wheel: next │                                   workers 3/4 · 2 queued
```

### M2 · The chat zoomed: the header keeps the context (N-05)

Zoom (`⤢`, `resize-pane -Z`) hides the tree, the Plan view and the Deploy column. The breadcrumb says which Lead this is, and the strip stays in reach: a click on any pill replaces the zoomed Lead without un-zooming first (guess: `session open` swaps the pane, and tmux keeps the zoom; **unverified**).

```
━[ api ▸ schema ▸ Lead · deploy  ZOOM ]━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━⤢ ⋯ ✕ ━━

   you    what is blocking schema?

   agent  Plan review r2 is approved and the Plan is in deploy. Two Jobs are left:
          schema/gql (running, S2, 3m) and schema/tests (queued behind it).
          PR #31 has checks passing; it waits for the last Job before merge.

   → cpd job list --plan schema
   ← 3 jobs: types merged · gql running · tests queued

   you    ok, and the conflict on web?

   agent  web-ui has a merge conflict with main on src/login.ts. I can rebase the PR
          branch (cpd pr rebase web) or hand it to you. Rebasing now.

   → cpd pr rebase web







































 ┌───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
 │ >                                                                                                                                                                                                                 │
 │                                                                                                                                                                                                                   │
 └───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
  ~/projects/cpd · claude-opus · 41% ctx
 ⧉ ? ▤ ⌕ │ ‹ next ● 3 › ↩ last │ ● schema/review  ● auth/ask  ● web-ui/conflict ┆ ◐ api·schema  ○ web │ chat zoomed · click ⤢ in the header to restore │                                        workers 3/4 · 2 queued
```

### M3 · The go-to picker, empty query (N-02)

A popup over the centre, 84×19. Nothing typed: what needs him first, then recent. Chips are buttons; a row click is a tree click; the footer shows where the highlighted row leads.

```
 ┌ go to ──────────────────────────────────────────────────────────────────────── ✕ ┐
 │ ⌕ type a Context, Plan, PR or Job…                                               │
 │ [● needs you 3] [◐ working 2] [○ idle 4] [◌ stopped 1] [PRs 4]                  │
 │ ── needs you ────────────────────────────────────────────────────────────────── │
 │ ▌● auth        api   asked 4m ago: which OAuth scope?                            │
 │  ● schema      api   Plan review r2 ready                                        │
 │  ● web-ui      web   PR #44 conflict                                             │
 │ ── recent ───────────────────────────────────────────────────────────────────── │
 │  ◐ schema      api   Lead working · 1⋅2⋅0⋅0⋅3                                    │
 │  ○ docs        web   Lead idle · 0⋅0⋅0⋅0⋅2                                       │
 │  ○ schema·7f3a api   fork, idle                                                  │
 │ ──────────────────────────────────────────────────────────────────────────────── │
 │ auth · Context api · Plan phase dispatch · 4 of 7 Jobs merged                    │
 │ ↵ open   → peek   esc close                                                      │
 └──────────────────────────────────────────────────────────────────────────────────┘
```

### M3b · The Event log as a floating pane (N-11)

Draggable and resizable (tmux 3.8), non-modal: he keeps typing in the chat behind it. Each line is a click target (N-06).

```
                                    ┌[ logs ]────────────────────────────────────── ⤡ ✕ ┐
                                    │ 14:02:11  schema   job-merged types              │
                                    │ 14:02:40  schema   attempt-started gql           │
                                    │ 14:03:05  auth     escalation: which scope?      │
                                    │ 14:05:12  web-ui   pr-feedback conflict          │
                                    │▌14:06:30  web-ui   attempt-started rebase        │
                                    └──────────────────────────────────────────────────┘
```

### M4 · Peek (N-08)

Under the hovered tree row, clamped to the pane's 41 inner columns. For a needs-you Lead it quotes the question.

```
 ▎    ● auth                 0⋅0⋅0⋅0⋅4
 ┌ auth · Lead · asking 4m ───────────┐
 │ which OAuth scope should the token │
 │ request: read or read:org?         │
 │ phase dispatch · 4 of 7 Jobs merged│
 │ click: open   right-click: menu    │
 └────────────────────────────────────┘
```

## 6. Considered and not recommended

| Idea | Source | Why not |
|---|---|---|
| Rewrite onto one ratatui app with an embedded PTY for the chat | herdr, bosun 2.0 | §3.2: emulation core, OSC 8, keys and selection all become cpd's work, in the one pane that works today |
| Move to zellij | zellij README and plugin API | one-way door (§3.4); no known equivalent of pane options `@cpd_view`; it fixes hover, which N-09 reaches inside tmux for ratatui views |
| Focus follows mouse | zellij `focus_follows_mouse`; tmux `focus-follows-mouse` needs all-motion mode ([server-client.c](https://github.com/tmux/tmux/blob/master/server-client.c)) | brushing the pointer across the tree would steal the keyboard from the chat mid-sentence (inferred). The StatusBar already refuses focus on click (`docs/contract.md` §6.1) |
| Tabs in the Plan view | gh-dash, posting, herdr | the User removed them (hs-d102); tabs live in the StatusBar strip (N-07) |
| Drag to reorder Plans or Jobs | zellij tab reorder (#5697) | the tree's order is `created_at` on purpose (survey **I-27**); a *pin* (N-07) is the reorder the User needs |
| A user-editable menu file | zellij `context_menu` config, k9s plugins | no request; menus are `cpd` commands, fixed (survey **I-07** says the same) |
| A mouse-aware status line with hover | zellij status bar | tmux cannot (`CHANGES`); see the N-01 decision |

## 7. All ideas in one table

Areas as in the survey: **Tree**, **Plan**, **Chat**, **Deploy**, **Bar** (StatusBar), **Pop** (popups), **tmux**.

| Rank | Id | Idea | Area | Pain | Effort | tmux ≥ | Depends on | Sources |
|---|---|---|---|---|---|---|---|---|
| 1 | N-01 | Needs-you strip, `next ●`, wheel | Bar | F, N | S (+ data path) | 3.4 | decision in N-01 | agent-deck, savras, ilmari, herdr, tmux ranges |
| 2 | N-02 | Go-to picker, empty query useful, chips | Pop | N, F | S spike / M | 3.4 (3.8 click-outside) | | herdr, Textual, fzf, ilmari, `choose-tree` |
| 3 | N-03 | Right-click menus on every object, one widget | Tree, Plan, Deploy, Pop | N, P | M | 3.4 | | zellij, tmux, lazygit |
| 4 | N-04 | `↩ last` bounce between Leads | Bar | N | S | 3.4 | N-01's data path | sesh, tmux, k9s |
| 5 | N-05 | Pane headers as toolbars, breadcrumb | tmux | N, L | S (3.7) / M (3.4) | 3.7 | hs-d257, hs-d266 | tmux, zellij, k9s, herdr |
| 6 | N-06 | Click gaps: Event log, workers, Inbox | Pop, Bar, Tree | N | S each | 3.4 | Inbox for its row | lazygit, herdr, btop |
| 7 | N-07 | Pinned Leads as tabs in the strip | Bar | N, F | M | 3.4 | a `pin` command; N-01 | bosun, herdr, zellij |
| 8 | N-08 | Peek card for a Lead | Tree, Pop | F, P | M | 3.4 | Inbox text or a session read | ilmari, sessionx, herdr |
| 9 | N-09 | Hover highlight and tip | all | L, N | S / M | 3.4 | StatusBar stays a ratatui row (tip) | zellij, Textual |
| 10 | N-10 | Clickable key hints | Bar | N | S | 3.4 | survey I-01 | Textual, btop |
| 11 | N-11 | Floating non-modal logs and Worker peek | Pop | P, N | M | 3.8 | the User's tmux | tmux, fzf, zellij |
| 12 | N-12 | Drag borders, `⧉` remembers | tmux | N, L | S | 3.4 | | tmux, bosun, savras |
| 13 | N-13 | Scrollbar on long panes | Chat, Worker | N | S | 3.6 | pi's screen mode | tmux |

## 8. Where this points

- **Do first, any tmux:** N-01 (after deciding its data path), N-04, N-03 for Job pills, N-06 for the Event log. They share one thing, **a click target that runs an existing `cpd` command**, so they add no engine surface.
- **Needs one fact:** the User's tmux version. ≥ 3.7 unlocks N-05 cheaply; ≥ 3.8 N-11. Ask for `tmux -V` on his host.
- **Needs one decision:** where the StatusBar lives (N-01): a ratatui row keeps hover and the strip's data path; the status line gives native ranges and wheel but needs a writer §17 forbids.
- **Cheap experiment before the picker view:** try `choose-tree` as the `⌕` target (N-02 spike). If he never opens it, the view is not worth building.
- **Not this Job's:** the powerline look (hs-d266) decides how the header controls and pills are drawn; the Inbox owner decides what the strip shows.
