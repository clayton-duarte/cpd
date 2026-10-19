# Cockpit survey: 20 TUIs worth learning from

Job `insp-survey`, Plan `tui-visuals`, PR `inspiration`. Research only: no code, nothing here is decided. The User's request (hs-d275): make the cpd cockpit beautiful, sleek, useful, organised, and above all make him more efficient. His four pain points, used as tags throughout:

| Tag | Pain point |
|---|---|
| **F** | **F**inding what needs him |
| **P** | Following **p**rogress |
| **N** | Getting around (**n**avigation) |
| **L** | **L**ooks and polish |

The cockpit stays **mouse-first** (clicks and menus; keys are shortcuts) and everything is open, tmux and the pane model included. Effort is **S** (a day or less, one view), **M** (a few days, one view plus a command or a tmux hook) or **L** (a week or more, or crossing the engine and several views); each has a one-line reason. The cockpit's current state is `docs/commands.md` §13 and §17 and `docs/contract.md` §6 on `main`; the Inbox (`docs/contract.md` §2.2, `docs/commands.md` §21), the powerline look (hs-d266) and dynamic titles (hs-d257) are in flight elsewhere and are not redone here.

## 1. Method and sources

- Read the three lists through `gh`: [awesome-tuis](https://github.com/rothgar/awesome-tuis) (about 860 lines, the Dashboards, Development, Docker/K8s, File Managers, Messaging and Productivity sections), [awesome-ratatui](https://github.com/ratatui/awesome-ratatui) (its "AI and Agents", "Source Control" and "Terminal Workflow" sections) and the Charm showcase [charm-in-the-wild](https://github.com/charm-and-friends/charm-in-the-wild) (its "AI" and "Development Tools" sections).
- For each pick: the repository README, then its docs folder or source (`gh api repos/<owner>/<repo>/contents/<path>`). Screenshots cannot be seen, so every layout is described from README, docs or source. Where a layout is **inferred** (from a config key, a file name or a code path), it says so.
- Every claim carries its URL. The three lists were enough; no web page was fetched except one attempt on harlequin.sh that returned nothing usable (not relied on).
- tmux facts were checked against the `tmux(1)` man page of tmux 3.8 on this machine; the cockpit's minimum tmux version for each is **unverified**.
- Not verified by this survey: that any idea fits the cockpit's real pane sizes (the cockpit is 215×62 at build, `docs/commands.md` §13.1), or that a terminal passes a given escape sequence. Each such idea says "unverified".

## 2. The 20 and why

The cockpit orchestrates agents (Leads, Workers), jobs (the Job flow) and pull requests (the Deploy column), from one tmux session. The picks cover those three things, plus the three layout problems: a many-pane screen, a status line that tells you what needs you, and a way to go anywhere fast.

| # | Tool | Kind | Why it is on the list |
|---|---|---|---|
| 1 | [lazygit](https://github.com/jesseduffield/lazygit) | git TUI | The reference multi-panel mouse-and-key TUI; its hint bar, panel jumps and screen modes solve "getting around" in a many-pane screen |
| 2 | [gh-dash](https://github.com/dlvhdr/gh-dash) | GitHub PR/issue dashboard | Same job as the Deploy column, done with saved-search sections, CI/review icons and a preview pane |
| 3 | [k9s](https://github.com/derailed/k9s) | Kubernetes dashboard | The model for "jump anywhere by typing a name", filters and hotkeys over a fleet of status-carrying objects |
| 4 | [zellij](https://github.com/zellij-org/zellij) | terminal workspace | The one multiplexer with a mode-aware, mouse-aware status bar and stacked/floating panes: a direct tmux alternative to compare against |
| 5 | [btop](https://github.com/aristocratos/btop) | system monitor | The look the User wants (rounded, graphs, themes), plus layout presets and clickable key buttons |
| 6 | [bottom](https://github.com/ClementTsang/bottom) | system monitor | Declarative row/column layout and "expand one widget" |
| 7 | [yazi](https://github.com/sxyazi/yazi) | file manager | Miller columns with a live preview, and an async task manager with progress |
| 8 | [superfile](https://github.com/yorukot/superfile) | file manager | Built "with a strong focus on user interface"; panel focus by one key; collapsible footer |
| 9 | [taskwarrior-tui](https://github.com/kdheepak/taskwarrior-tui) | task manager | Live filter, a context switcher and urgency colours over a to-do list that needs triage |
| 10 | [posting](https://github.com/darrenburns/posting) | API client | Jump mode, a command palette, 37 themes, and a remote control that lets agents drive the open UI |
| 11 | [harlequin](https://github.com/tconbeer/harlequin) | SQL IDE | A command palette whose entries know their shortcut and whether they are available; a keys panel for the focused widget |
| 12 | [lnav](https://github.com/tstack/lnav) | log viewer | Merges many sources by time, jumps to the next error, shows a histogram: the Event log's big brother |
| 13 | [agent-deck](https://github.com/asheshgoplani/agent-deck) | agent session dashboard | Status glyphs, a "waiting" strip in the tmux status line with number jump, status filters, push to phone |
| 14 | [savras](https://github.com/Marijusaj/savras) | agent session side panel | A panel built for "which sessions wait on me", with two groupings and a rule that rows never move under the cursor |
| 15 | [bosun](https://github.com/yetidevworks/bosun) | tmux-native agent orchestrator | A ratatui app on tmux like the cockpit: borderless shaded panels, accent-bar dialogs, click-into-preview |
| 16 | [Toad](https://github.com/batrachianai/toad) | agent chat UI | Multiple agents, one screen of states on `ctrl+s`, `@` file picker |
| 17 | [crush](https://github.com/charmbracelet/crush) | agent chat TUI (Charm) | Command palette with live theme preview; notifications only when unfocused, over SSH by OSC; session `IsBusy` signal |
| 18 | [flowrs](https://github.com/jvanbuel/flowrs) | Airflow TUI (ratatui) | A DAG-of-tasks UI: drill-down, per-view command popup, a graph and a Gantt model |
| 19 | [process-compose](https://github.com/F1bonacc1/process-compose) | process orchestrator | Jobs with dependencies and a status table (icon, age, health, cpu, restarts) over a log pane |
| 20 | [Reeve](https://github.com/Dancode-188/reeve) | agent cockpit | "Watch a run live, score it, step in": health gauge, rules, intervene with acknowledgment |

Left out on purpose, with the reason (all from the same three lists): `crmux` is in maintenance mode ([README](https://github.com/maedana/crmux#project-status)); `claudectl` is mostly a local-LLM auto-approver, outside a visual cockpit ([README](https://github.com/mercurialsolo/claudectl)); `kagan` (listed in awesome-tuis) returned 404 for `kagan-sh/kagan` on 2026-10-19, so it could not be read; `Backlog.md` is a Kanban over markdown files ([README](https://github.com/MrLesk/Backlog.md)), close to Plan view but with nothing a Job flow lacks; `gitui`, `serie`, `television` and `lazydocker` repeat patterns already taken from lazygit, a git DAG view and fuzzy channels.

## 3. The tools

Each tool: **What**, **Layout**, **Status and alerts**, **Driven by**, **Standout**, then its ideas as *pattern → source → cpd mapping → effort*. Idea ids (`I-01`…) are used by the table in §5.

### 3.1 lazygit

- **What:** the terminal UI for git that set the style for the `lazy*` family ([README](https://github.com/jesseduffield/lazygit)).
- **Layout:** a stack of panels on the left (default 1/3 of the width: `sidePanelWidth: 0.3333`) and one main view on the right; a command log; a bottom line of key hints. Config keys: `sidePanelWidth`, `expandFocusedSidePanel` ("increase the height of the focused side window; creating an accordion effect"), `screenMode` (`normal | half | full`, changed with `+` and `_`), `portraitMode` (`auto | always | never`, stacks panels when the window is narrow and tall), `border: rounded`, `showCommandLog`, `showBottomLine`, `showPanelJumps` ("show jump-to-window keybindings in window titles") ([Config.md](https://github.com/jesseduffield/lazygit/blob/HEAD/docs/Config.md)). The panel names (status, files, branches, commits, stash) are inferred from the README feature list and `statusPanelView`.
- **Status and alerts:** the branches panel shows a GitHub icon next to a branch with a pull request, "its color shows the state of the PR (open, merged, etc.)"; `gh` supplies it ([README, "Show GitHub pull requests"](https://github.com/jesseduffield/lazygit#show-github-pull-requests)).
- **Driven by:** keys first, mouse on by default (`mouseEvents: true`); menus for every compound action (`m` for the rebase menu, `shift+d` for reset options); user-defined [custom commands](https://github.com/jesseduffield/lazygit/blob/HEAD/docs/Custom_Command_Keybindings.md) with their own keys, prompts and menus.
- **Standout:** the screen always tells you *what keys work in the thing you are on* and how to jump to any other panel.

**I-01. A context hint line in the StatusBar.**
- **Pattern:** a bottom line that lists the keys and clickable buttons of the focused panel (`showBottomLine`).
- **Source:** lazygit [Config.md](https://github.com/jesseduffield/lazygit/blob/HEAD/docs/Config.md); the same idea in zellij's status bar (§3.4, I-08) and harlequin's keys panel (§3.11, I-22).
- **cpd mapping:** StatusBar (`docs/commands.md` §13.2 "statusbar"; today: three glyph buttons on the left, `workers N/M` on the right, the middle empty). Fill the empty middle with the hints of the pane that has focus, read from tmux's `#{pane_current_command}`/`@cpd_view` (inferred: the views already set `@cpd_view`). Serves **N** (and keys stay discoverable although the cockpit is mouse-first).
- **Effort:** S. The row exists; it needs a `focus` input and a table of hints per view. Tooltips are in `BACKLOG.md` already, so this is the same room.

```
 ⧉ ? ▤  │ tree: ↵ open  n new plan  x close  right-click menu                 workers 3/4 · 1 waiting
```

**I-02. Panel jump numbers in the pane headers.**
- **Pattern:** a number in each window title is the key that jumps there (`showPanelJumps`).
- **Source:** lazygit [Config.md](https://github.com/jesseduffield/lazygit/blob/HEAD/docs/Config.md) ("show jump-to-window keybindings in window titles").
- **cpd mapping:** the pane headers (tmux `pane-border-format`, `docs/contract.md` §6.1: one inverted line per pane). Add `1 tree`, `2 plan`, `3 chat`, `4 deploy` and a tmux `bind -n M-1…4 select-pane`. Serves **N**; weak for the mouse-first User (a click already focuses) so it is a shortcut only.
- **Effort:** S. A format string and four bindings; **unverified**: that `M-<n>` is not taken by the Mac terminal.

**I-03. Zoom and screen modes: focus one pane, then come back.**
- **Pattern:** `+`/`_` cycle the focused window between normal, half and full; `portraitMode: auto` re-stacks panels when the window is narrow.
- **Source:** lazygit [Config.md](https://github.com/jesseduffield/lazygit/blob/HEAD/docs/Config.md); bottom's `e` expand (§3.6, I-12).
- **cpd mapping:** the whole `main` window. tmux has `resize-pane -Z` (man tmux, "-Z zooms the pane"); a click on a pane header's title toggles zoom of that pane; reset layout (`⧉`) already restores sizes. A narrow-window rule (inferred: the User may attach from a smaller terminal) would swap the Deploy column below the Plan view. Serves **N**, **L**.
- **Effort:** S for zoom on header click (tmux does the work); M for the narrow-window re-layout (new layout code in reset-layout, `docs/commands.md` §13.2).

```
 lazygit                                        cpd (proposed zoom: chat)
 ┌[1]Status┐┌ main view ─────────────┐          ┌──────────── chat (zoomed) ───────────────┐
 ├[2]Files─┤│                        │          │                                          │
 ├[3]Branch┤│                        │          │  click the header again to restore       │
 ├[4]Commit┤└────────────────────────┘          └──────────────────────────────────────────┘
 └─────────┘ ┌ command log ───────────┐           ⧉ ? ▤ │ chat zoomed · click header to restore
 hint line:  space stage · c commit · ? help
```

### 3.2 gh-dash

- **What:** "a rich terminal UI for GitHub that doesn't break your flow": PRs, issues and notifications ([README](https://github.com/dlvhdr/gh-dash)). Built with Bubble Tea and Lip Gloss, so it is in the Charm showcase ([list entry](https://github.com/charm-and-friends/charm-in-the-wild#development-tools)).
- **Layout:** a tab row of **sections**, each a GitHub search filter and a title; a table below; a preview pane (default open, 45 % of the width to the right, or 40 % of the height below on a narrow terminal: [defaults](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/defaults.mdx)). Columns of the PR table: updatedAt, state, repo, title, author, labels, comments, reviewStatus, ci, lines ([PR layout](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/layout/pr.mdx)).
- **Status and alerts:** `ci` and `reviewStatus` are one-icon columns; notification sections filter by reason (`review-requested`, `mention`, `assign`, `participating`: [notification sections](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/notification-section.mdx)); sections refetch on an interval (`refetchIntervalMinutes: 30`).
- **Driven by:** keys (`s` switch view, `/` search, `r`/`R` refresh, `v` approve, `m` merge, `w` watch checks, `C` checkout, `d` diff: [global](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/getting-started/keybindings/global.mdx), [selected PR](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/getting-started/keybindings/selected-pr.mdx)); every key is overridable and user "custom actions" are shell commands.
- **Standout:** *the lists are saved questions* ("involves me, not by me, updated in 2 weeks"), each with a count, instead of one flat list.

**I-04. Saved-question sections in the Deploy column.**
- **Pattern:** named sections defined by a filter, each a tab with its own list.
- **Source:** gh-dash [PR sections](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/pr-section.mdx).
- **cpd mapping:** the Deploy column (`widgets` view; today one flat list of the User's open PRs, oldest first, `docs/commands.md` §13.2). Group the same rows under headers by what they need from him: `Needs you` (changes requested, a review to answer, conflict), `Failing` (checks red), `Waiting` (checks pending), `Ready` (approved and green). Header style is the existing section-header rule. Review requests stay Inbox items (`docs/commands.md` §13.2 says so), so this uses only owned PRs. Serves **F**, **P**.
- **Effort:** S. All data is already in `pull_requests` (checks, review state, `mergeable`); it is a sort-and-header change in one view.

```
 ──[ PRs ]──────────────
 ● needs you (2)
  #412 schema   conflict ↗
  #409 api      changes ↗
 ✗ failing (1)
  #415 web      2/9 red ↗
 ◌ waiting (1)
  #416 tui      checks ↗
```

**I-05. A preview strip for the selected PR or Job.**
- **Pattern:** a preview pane that follows the selection, placed right or below depending on width.
- **Source:** gh-dash [defaults](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/defaults.mdx) (`preview.position: auto`); yazi's preview column (§3.7, I-13).
- **cpd mapping:** the Plan view's selection (arrows, `docs/commands.md` §13.2) opens nothing until Enter; a one-row-high strip under the boxes would show the selected Job's last Worker line and its Attempt count, so he can follow progress without opening the Worker pane. Serves **P**.
- **Effort:** M. A new region in the Plan view and a read of the Worker file's tail (the Worker view already reads it).

### 3.3 k9s

- **What:** a terminal UI for a Kubernetes cluster ([README](https://github.com/derailed/k9s)).
- **Layout:** a header (context info, key hints, logo; `ctrl-e` toggles it, `headless: true` hides it), a table of one resource kind, **breadcrumbs** at the bottom (`ctrl-g` toggles). `crumbsless`, `logoless`, `splashless` are config keys ([README config](https://github.com/derailed/k9s#k9s-configuration)).
- **Status and alerts:** row colours by state (skins, e.g. `skin: dracula`); a `pulses` view (`:pulses`) and an `xray` view; `popeye` for a cluster sanitizer; `readOnly: true` disables every modifying command.
- **Driven by:** command mode: `:pod` shows pods, `:pod ns-x`, `:pod /fred` (filtered), `:pod @ctx1` (switch context); `/filter` with regex, `/!` inverse, `/-l labels`, `/-f` fuzzy; `esc` goes back, `-` returns to the previous command (like `cd -`); `?` lists keys; **hotkeys** (`hotkeys.yaml`: a key bound to `xray deploy`) and **plugins** (`plugins.yaml`: a shortcut runs a command against the selected row) ([README table](https://github.com/derailed/k9s#key-bindings)). **Mouse is off by default** (`enableMouse: false`): the opposite of cpd's rule.
- **Standout:** one text box reaches anything; users add their own jumps and actions as a file.

**I-06. A `:` jump palette: type part of a Context, Plan or Job id, land there.**
- **Pattern:** command mode with aliases, filters and "go back".
- **Source:** k9s [README, key bindings](https://github.com/derailed/k9s#key-bindings); also posting's jump mode (§3.10, I-19) and harlequin's palette (§3.11, I-21).
- **cpd mapping:** a popup (`display-popup`, like the legend and logs) opened by a StatusBar `⌕` button or a key; a typed fragment filters Contexts, Plans, Leads, forks and PRs; Enter runs `session open`/`context manager` exactly as the tree's click does (`docs/commands.md` §13.2, "Clicks"). With the ids as labels (hs-d101) a fuzzy match by id is the way to go to a Plan when the tree is long. Serves **N**.
- **Effort:** M. A new view (`cpd tui view jump`), reading the same snapshot as the tree; no new command.

```
 ┌ go to ──────────────────────────────┐
 │ ⌕ sch▌                              │
 │  ▸ plan   schema     (api)  ● needs │
 │    pr     schema/#31 open           │
 │    job    schema/types  running     │
 └─────────────────────────────────────┘
```

**I-07. User-bound hotkeys as a file.**
- **Pattern:** `hotkeys.yaml` maps a key to a command; `plugins.yaml` maps a shortcut to a command on the selected row.
- **Source:** k9s [README, HotKey support](https://github.com/derailed/k9s#hotkey-support) and [Plugins](https://github.com/derailed/k9s#plugins).
- **cpd mapping:** the tree's right-click menu is a fixed list of `cpd` commands (`docs/commands.md` §13.2). A config table would add the User's own items ("Open PR in browser", "Copy branch") without code. Serves **N**. Probably premature: the User has not asked for extension; listed only because k9s makes it the main way users make the tool theirs.
- **Effort:** M (config schema, validation at the edge, menu merge). Lowest priority of this survey.

### 3.4 zellij

- **What:** "a workspace aimed at developers": a multiplexer with layouts, floating and stacked panes, plugins and a web client ([README](https://github.com/zellij-org/zellij)).
- **Layout:** the default layout is a one-row borderless `tab-bar` plugin on top, the panes, and a one-row borderless `status-bar` plugin at the bottom ([default.kdl](https://github.com/zellij-org/zellij/blob/HEAD/zellij-utils/assets/layouts/default.kdl)); shipped layouts include `compact` (one bar), `strider` (a file tree on the side) and `disable-status-bar`.
- **Status and alerts:** the status bar is a plugin that knows the **input mode** (`InputMode` in [status_bar/mod.rs](https://github.com/zellij-org/zellij/blob/HEAD/default-plugins/bars/src/status_bar/mod.rs); inferred: it shows the keys of the current mode) and subscribes to **mouse** events (`EventType::Mouse`; files `hover.rs` and `click_actions.rs` next to it). A `compact_bar/tooltip.rs` draws a tooltip; a `session_indicator.rs` names the session.
- **Driven by:** modal keys (a mode per kind of action), mouse (click tabs, drag borders), a session manager plugin.
- **Standout:** the bars are *programs*, not text, so they react to the mouse and the mode.

**I-08. A hover and click status bar.**
- **Pattern:** the status bar highlights on hover, shows a tooltip, and its segments are clickable.
- **Source:** zellij [status_bar/hover.rs and click_actions.rs](https://github.com/zellij-org/zellij/tree/HEAD/default-plugins/bars/src); tmux `range=user|X` and `mouse_status_range` (tmux 3.8 man page, "range=user|X is a user-defined range; it triggers the 'Status' mouse key").
- **cpd mapping:** the StatusBar is moving to tmux's status line (task text). tmux can then make each segment a **user range** and bind `MouseDown1Status` on `#{mouse_status_range}` to run a `cpd` command. A tooltip cannot hover in tmux; instead show the verb inline (`⧉ layout`, `? legend`, `▤ log`) once the row is wide enough. Serves **N**, **L**. Hover feedback is **not available** in the tmux status line (inferred: tmux has no mouse-move range event); a drawn ratatui row keeps hover but the move to the status line gives it up.
- **Effort:** S for ranges and labels in tmux's status line; L to keep a ratatui bar with hover (rejected by the move already decided).

**I-09. Stacked panes for the Worker views.**
- **Pattern:** stacked panes: one expanded pane and the rest collapsed to a title row.
- **Source:** zellij [README](https://github.com/zellij-org/zellij) ("floating and stacked" panes).
- **cpd mapping:** the read-only Worker pane beside the chat. With three running Workers there is room for one; a stack of collapsed rows (name, status, last line) would let a click pick which expands. tmux has no stacked panes (inferred from the man page: only `select-layout` presets); it would be drawn by one ratatui view that shows all Workers of the Plan. Serves **P**, **N**.
- **Effort:** M. One view, reusing the Worker renderer (`tui/src/worker/render.rs`).

### 3.5 btop

- **What:** a resource monitor with graphs ([README](https://github.com/aristocratos/btop)).
- **Layout:** boxes (cpu, mem, net, proc, optional gpu 5/6/7/0 toggles) with rounded corners (`rounded_corners` is a config key); **presets**: `presets = "cpu:1:default,proc:0:default cpu:0:default,mem:0:default,net:0:default cpu:0:block,net:0:tty"`, up to nine, each a box list with a graph symbol; preset 0 is "all boxes" ([README config](https://github.com/aristocratos/btop#configurability)).
- **Status and alerts:** graphs drawn in braille characters (U+2800–U+28FF; a `block` and a `tty` fallback) ([README, fonts](https://github.com/aristocratos/btop#compilation-linux)); themes are files in a themes directory.
- **Driven by:** "full mouse support: all buttons with a highlighted key are clickable and mouse scrolling works in process list and menu boxes"; a game-inspired menu for every config option ([README features](https://github.com/aristocratos/btop#features)).
- **Standout:** *key hints are buttons*: the highlighted letter in a label is both the shortcut and the click target.

**I-10. Layout presets, one click each.**
- **Pattern:** named presets of which boxes show and in which form; one key cycles them.
- **Source:** btop [README, `presets`](https://github.com/aristocratos/btop#configurability).
- **cpd mapping:** the StatusBar's `⧉` runs one reset-layout (§13.2). A preset menu (right-click or click on `⧉`: `Default`, `Plan focus` (Deploy column hidden), `Chat focus` (Plan view collapsed to a strip), `Review` (Deploy column wide)) would let him change the shape to the task. tmux does it with `resize-pane` and `kill`-free hiding: sizes only, so no panes die (inferred). Serves **N**, **L**.
- **Effort:** S. Reset-layout already applies sizes; a preset is a different set of sizes.

**I-11. A braille sparkline for the host load in the Deploy column.**
- **Pattern:** a graph that scrolls in braille cells with a block fallback.
- **Source:** btop [README, graphs](https://github.com/aristocratos/btop#compilation-linux); ratatui's `Sparkline` and `Chart` widgets with a braille marker (from ratatui docs; **unverified** here that the pinned ratatui version has them).
- **cpd mapping:** the Deploy column's machine section (`load a b c`, soon CPU and memory %). Two lines of braille history under the numbers show a trend, not a number. Serves **P**, **L**.
- **Effort:** S. One widget and a ring buffer in the view; no new data beyond what is planned.

```
  mbp-dev
  cpu 38%  ⣀⣤⣶⣿⣷⣤⣀⣠⣴⣶     mem 61%  ⣀⣀⣠⣤⣤⣴⣴⣶⣶⣶
```

### 3.6 bottom

- **What:** a customizable graphical process and system monitor ([README](https://github.com/ClementTsang/bottom)).
- **Layout:** *declarative*: `[[row]]` blocks with `ratio` and `[[row.child]]` columns/widgets of types `cpu`, `mem`, `net`, `proc`, `temp`, `disk`, `battery`, `empty` ([layout docs](https://github.com/ClementTsang/bottom/blob/HEAD/docs/content/configuration/config-file/layout.md)). A basic (htop-like) mode exists.
- **Status and alerts:** a graph per widget; a process table with search and kill signals; themes.
- **Driven by:** click a widget to select it; `e` expands the selected widget to the full window, `esc` leaves ([general usage](https://github.com/ClementTsang/bottom/blob/HEAD/docs/content/usage/general-usage.md)); mouse bindings and `?` help.
- **Standout:** *select, then expand*, and the layout is data.

**I-12. Expand the selected view (a "maximize" toggle).**
- **Pattern:** `e` expands one widget to the whole window; the same key returns.
- **Source:** bottom [general usage](https://github.com/ClementTsang/bottom/blob/HEAD/docs/content/usage/general-usage.md), "Expansion".
- **cpd mapping:** same as I-03; listed apart because bottom's trigger is *selecting the widget*, which for cpd is a click on its header: `plan` full-screen to read a wide flow chart, the Deploy column full-screen to review all PRs. Serves **N**.
- **Effort:** S (tmux zoom).

**I-12b. Layout as a data file.**
- **Pattern:** rows and columns with ratios in config.
- **Source:** bottom [layout docs](https://github.com/ClementTsang/bottom/blob/HEAD/docs/content/configuration/config-file/layout.md).
- **cpd mapping:** the cockpit's layout is code (20 %/centre/20 %, `docs/commands.md` §13.1). A data file would let I-10's presets be written without a build. **Not recommended now**: convention over configuration; fold into I-10 if presets are built.
- **Effort:** M.

### 3.7 yazi

- **What:** an async terminal file manager in Rust ([README](https://github.com/sxyazi/yazi)).
- **Layout:** Miller columns: parent, current, preview, in the ratio `ratio = [1, 4, 3]` ([yazi-default.toml](https://github.com/sxyazi/yazi/blob/HEAD/yazi-config/preset/yazi-default.toml)); tabs; a header with the path and a status bar; a **tasks** overlay.
- **Status and alerts:** "async task scheduling and management: real-time progress updates, task cancellation, and internal task priority"; a notify component; a client-server "data distribution service" with pub-sub across instances ([README features](https://github.com/sxyazi/yazi#yazi---️-blazing-fast-terminal-file-manager)).
- **Driven by:** vim keys, mouse, drag and drop, which-key style popups, a Lua plugin system with UI plugins.
- **Standout:** the preview follows the cursor, and long jobs live in a task list you can open.

**I-13. A task list popup: everything in flight, with progress and cancel.**
- **Pattern:** a task manager overlay with per-task progress and cancel.
- **Source:** yazi [README, features](https://github.com/sxyazi/yazi#yazi---️-blazing-fast-terminal-file-manager).
- **cpd mapping:** the StatusBar says `workers N/M`; a click would open a popup listing each Attempt holding a slot: Plan, Job, state, age, last tool line, and a cancel button. Today he finds a Worker by walking the flow chart. Serves **P**, **F**.
- **Effort:** M. A new popup view on the status bar's workers indicator; it reads Attempts (the statusbar already does) and runs the existing cancel command (name to be checked in `docs/commands.md`).

```
 ┌ workers 3/4 ─────────────────────────────────────────┐
 │ ⠹ schema/types    S1 a1  12m   edit src/gql.ts     ✕ │
 │ ⠼ schema/gql      S2 a1   3m   cargo test          ✕ │
 │ ⠴ web/login       S2 a2   9m   waiting on review   ✕ │
 │ ○ 2 queued                                           │
 └──────────────────────────────────────────────────────┘
```

