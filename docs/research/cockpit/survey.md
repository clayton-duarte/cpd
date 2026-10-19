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
- **Source:** lazygit [Config.md](https://github.com/jesseduffield/lazygit/blob/HEAD/docs/Config.md); the same idea in zellij's status bar (§3.4, I-08) and harlequin's keys panel (§3.11, I-20).
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
- **Driven by:** keys (`s` switch view, `/` search, `r`/`R` refresh, `v` approve, `m` merge, `w` watch checks, `C` checkout, `d` diff: [global](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/getting-started/keybindings/global.mdx), [selected PR](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/getting-started/keybindings/selected-pr.mdx)); every key is overridable and the README lists "custom actions to perform your specific workflow needs".
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
- **Source:** gh-dash [defaults](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/defaults.mdx) (`preview.position: auto`); yazi's preview column (§3.7, I-14).
- **cpd mapping:** the Plan view's selection (arrows, `docs/commands.md` §13.2) opens nothing until Enter; a one-row-high strip under the boxes would show the selected Job's last Worker line and its Attempt count, so he can follow progress without opening the Worker pane. Serves **P**.
- **Effort:** M. A new region in the Plan view and a read of the Worker file's tail (the Worker view already reads it).

### 3.3 k9s

- **What:** a terminal UI for a Kubernetes cluster ([README](https://github.com/derailed/k9s)).
- **Layout:** a header (inferred to hold cluster info and key hints, plus a logo; `ctrl-e` toggles it, `headless: true` hides it), a table of one resource kind, **breadcrumbs** at the bottom (`ctrl-g` toggles). `crumbsless`, `logoless`, `splashless` are config keys ([README config](https://github.com/derailed/k9s#k9s-configuration)).
- **Status and alerts:** colour skins (e.g. `skin: dracula`; that rows are coloured by state is inferred); a `pulses` view (`:pulses`) and an `xray` view; `popeye` for a cluster sanitizer; `readOnly: true` disables every modifying command.
- **Driven by:** command mode: `:pod` shows pods, `:pod ns-x`, `:pod /fred` (filtered), `:pod @ctx1` (switch context); `/filter` with regex, `/!` inverse, `/-l labels`, `/-f` fuzzy; `esc` goes back, `-` returns to the previous command (like `cd -`); `?` lists keys; **hotkeys** (`hotkeys.yaml`: a key bound to `xray deploy`) and **plugins** (`plugins.yaml`: a shortcut runs a command against the selected row) ([README table](https://github.com/derailed/k9s#key-bindings)). **Mouse is off by default** (`enableMouse: false`): the opposite of cpd's rule.
- **Standout:** one text box reaches anything; users add their own jumps and actions as a file.

**I-06. A `:` jump palette: type part of a Context, Plan or Job id, land there.**
- **Pattern:** command mode with aliases, filters and "go back".
- **Source:** k9s [README, key bindings](https://github.com/derailed/k9s#key-bindings); also posting's jump mode (§3.10, I-17) and harlequin's palette (§3.11, I-19).
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
- **Status and alerts:** graphs drawn in braille characters (U+2800–U+28FF; a `block` and a `tty` fallback) ([README, fonts](https://github.com/aristocratos/btop#prerequisites)); themes are files in a themes directory.
- **Driven by:** "full mouse support: all buttons with a highlighted key are clickable and mouse scrolling works in process list and menu boxes"; a game-inspired menu for every config option ([README features](https://github.com/aristocratos/btop#features)).
- **Standout:** *key hints are buttons*: the highlighted letter in a label is both the shortcut and the click target.

**I-10. Layout presets, one click each.**
- **Pattern:** named presets of which boxes show and in which form; one key cycles them.
- **Source:** btop [README, `presets`](https://github.com/aristocratos/btop#configurability).
- **cpd mapping:** the StatusBar's `⧉` runs one reset-layout (§13.2). A preset menu (right-click or click on `⧉`: `Default`, `Plan focus` (Deploy column hidden), `Chat focus` (Plan view collapsed to a strip), `Review` (Deploy column wide)) would let him change the shape to the task. tmux does it with `resize-pane` and `kill`-free hiding: sizes only, so no panes die (inferred). Serves **N**, **L**.
- **Effort:** S. Reset-layout already applies sizes; a preset is a different set of sizes.

**I-11. A braille sparkline for the host load in the Deploy column.**
- **Pattern:** a graph that scrolls in braille cells with a block fallback.
- **Source:** btop [README, graphs](https://github.com/aristocratos/btop#prerequisites); ratatui's `Sparkline` and `Chart` widgets with a braille marker (from ratatui docs; **unverified** here that the pinned ratatui version has them).
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

**I-13. Layout as a data file.**
- **Pattern:** rows and columns with ratios in config.
- **Source:** bottom [layout docs](https://github.com/ClementTsang/bottom/blob/HEAD/docs/content/configuration/config-file/layout.md).
- **cpd mapping:** the cockpit's layout is code (20 %/centre/20 %, `docs/commands.md` §13.1). A data file would let I-10's presets be written without a build. **Not recommended now**: convention over configuration; fold into I-10 if presets are built.
- **Effort:** M.

### 3.7 yazi

- **What:** an async terminal file manager in Rust ([README](https://github.com/sxyazi/yazi)).
- **Layout:** Miller columns: parent, current, preview, in the ratio `ratio = [1, 4, 3]` ([yazi-default.toml](https://github.com/sxyazi/yazi/blob/HEAD/yazi-config/preset/yazi-default.toml)); multi-tab support (README). A header, a status bar and a tasks overlay are **inferred** from the feature list.
- **Status and alerts:** "async task scheduling and management: real-time progress updates, task cancellation, and internal task priority"; a notify component; a client-server "data distribution service" with pub-sub across instances ([README features](https://github.com/sxyazi/yazi)).
- **Driven by:** vim keys, mouse, drag and drop, which-key style popups, a Lua plugin system with UI plugins.
- **Standout:** the preview follows the cursor, and long jobs live in a task list you can open.

**I-14. A task list popup: everything in flight, with progress and cancel.**
- **Pattern:** a task manager overlay with per-task progress and cancel.
- **Source:** yazi [README, features](https://github.com/sxyazi/yazi).
- **cpd mapping:** the StatusBar says `workers N/M`; a click would open a popup listing each Attempt holding a slot: Plan, Job, state, age, last tool line, and a cancel button. Today he finds a Worker by walking the flow chart. Serves **P**, **F**.
- **Effort:** M. A new popup view on the status bar's workers indicator; it reads Attempts (the statusbar already does) and runs a cancel (`job cancel`, `docs/commands.md` §5.3, cancels a Job; whether an Attempt-level stop exists is not checked here).

```
 ┌ workers 3/4 ─────────────────────────────────────────┐
 │ ⠹ schema/types    S1 a1  12m   edit src/gql.ts     ✕ │
 │ ⠼ schema/gql      S2 a1   3m   cargo test          ✕ │
 │ ⠴ web/login       S2 a2   9m   waiting on review   ✕ │
 │ ○ 2 queued                                           │
 └──────────────────────────────────────────────────────┘
```

### 3.8 superfile

- **What:** "a pretty fancy and modern terminal file manager", in Go and Bubble Tea; its stated reason to exist is that the author "was often disappointed by [other managers'] UI design" ([overview](https://github.com/yorukot/superfile/blob/HEAD/website/src/content/docs/overview.md)).
- **Layout:** file panel (focused by default), plus a sidebar, a processes panel, a metadata panel and a command bar ([tutorial, "Panel navigation"](https://github.com/yorukot/superfile/blob/HEAD/website/src/content/docs/getting-started/tutorial.mdx)).
- **Status and alerts:** a processes panel (inferred: running file operations) and a metadata panel for the selected file.
- **Driven by:** a key per panel (`s` sidebar, `p` processes, `m` metadata, `:` command bar; "to return focus back onto the file panel, press the same hotkey again"), `F` hides or shows all footer panels; themes by file ([theme wiki](https://superfile.dev/configure/custom-theme/)); hotkeys by file.
- **Standout:** the footer is a drawer that is one key away from gone, and the screen looks designed first.

**I-15. A collapsible side drawer, same control to open and close.**
- **Pattern:** one key hides or shows a whole band of secondary panels; the same key that focuses a panel also returns.
- **Source:** superfile [tutorial](https://github.com/yorukot/superfile/blob/HEAD/website/src/content/docs/getting-started/tutorial.mdx).
- **cpd mapping:** the Deploy column (20 % of 215 columns is about 43) is secondary to the chat while he writes to a Lead, and primary while he reviews. A click on its pane header collapses it to a one-column strip showing only a count badge (`3 PRs`, a ● when one needs him), a second click restores. Overlaps I-10 (presets) and I-03 (zoom); the difference is that this is a per-column toggle with a badge, so a collapsed column still *announces* a need. Serves **L**, **F**.
- **Effort:** S. `resize-pane -x` plus the view drawing a narrow form when its width is small.

### 3.9 taskwarrior-tui

- **What:** a terminal UI for Taskwarrior ([README](https://github.com/kdheepak/taskwarrior-tui)): vim-like navigation, live filter updates, multiple selection, tab completion, colours taken from Taskwarrior's own rules.
- **Layout:** a task report (table) with a task-details pane; separate project and context panes (source files `src/pane/report.rs`, `project.rs`, `context.rs`: inferred from the file names, [src/pane](https://github.com/kdheepak/taskwarrior-tui/tree/HEAD/src/pane)).
- **Status and alerts:** colours from Taskwarrior's `color.*` rules, so a user's "overdue" is red in the CLI and the TUI alike; the report columns include `urgency`.
- **Driven by:** keys (`/` filter, `c` context switcher menu, `V` toggle marks on all, `Ctrl-e`/`Ctrl-y` scroll the details), every key rebindable ([keybindings](https://github.com/kdheepak/taskwarrior-tui/blob/HEAD/docs/src/content/docs/keybindings.md)). No mouse mentioned in the docs read.
- **Standout:** the list narrows *as you type*, and a **context** is a saved scope you switch with one menu.

**I-16. Live filter and a "needs me" scope for the Context column.**
- **Pattern:** `/` narrows the list while typing; `c` opens a menu of saved contexts that change what every report shows.
- **Source:** taskwarrior-tui [keybindings](https://github.com/kdheepak/taskwarrior-tui/blob/HEAD/docs/src/content/docs/keybindings.md).
- **cpd mapping:** the Context column is a tree of Contexts, Plans and Leads with five counters. Two additions: a filter row at its bottom (type, rows narrow, Contexts with no match fold away), and a scope toggle button `needs me` that hides every row whose dot is not a needs-you dot (`Dot::needs_you()`, `docs/commands.md` §13.2). Overlaps the Inbox (the Inbox lists items; this reduces the tree to the Sessions behind them), so the Inbox owner should decide whether it is wanted. Serves **F**, **N**.
- **Effort:** S. View state only: no command, no write (as the archived-rows fold already is).

```
 ┌ Contexts ──────────────┐
 │ ▾ ● api            3⋅1 │
 │   ⏵ ● schema    1⋅2⋅0  │
 │   ●  auth       0⋅0⋅3  │
 │ ▸ ○ web                │
 │ ⌕ filter…    [needs me]│
 └────────────────────────┘
```

### 3.10 posting

- **What:** a terminal HTTP client ([README](https://github.com/darrenburns/posting)), in Python/Textual: requests live in YAML files, with environments, tabs, history and a command palette.
- **Layout:** the README names request tabs, a populated collection, header autocomplete and a response with a "collapsible JSON tree"; its mouse demo: "switch request tabs, resize the response panel, scroll through a response, and select text". The arrangement (collection left, request and response beside) is **inferred**.
- **Status and alerts:** request timing, a persistent history of responses, a live event log for streaming.
- **Driven by:** **jump mode** ("press a shortcut, type a label, and jump straight to any part of the interface"), a **command palette**, full **mouse** support (switch tabs, resize panels, select text), 37 themes plus user themes, `$EDITOR`/`$PAGER`; and `posting remote`, "so coding agents like Claude Code and Codex can send requests through the Posting you have open while you watch".
- **Standout:** two ways to get anywhere (palette by name, jump mode by label), and an agent that drives the *same* instance the human watches.

**I-17. Jump mode: labels over every pane and clickable region.**
- **Pattern:** a shortcut paints a letter over each target; typing the letter moves focus there.
- **Source:** posting [README, "Jump mode"](https://github.com/darrenburns/posting#jump-mode); tmux `display-panes` (man tmux 3.8: "displays an indicator for each visible pane") is the nearest built-in.
- **cpd mapping:** across the cockpit: panes and, inside the tree, rows. It is a keyboard accelerator for a mouse-first cockpit, so it is a "second wave" item next to I-06. tmux's `display-panes` already labels panes with numbers; labelling *rows* needs the view itself. Serves **N**.
- **Effort:** M for rows inside the tree; S for panes (a binding to `display-panes` and a `select-pane` template).

**I-18. Agent pointing: a Lead can highlight a thing in the cockpit.**
- **Pattern:** `posting remote` lets an agent act through the UI the human has open, so the human sees it.
- **Source:** posting [README, features list](https://github.com/darrenburns/posting) ("`posting remote`").
- **cpd mapping:** Leads already run `cpd` commands. A `cpd tui point <plan|pr|job>` that flashes the matching row (tree, box, Deploy row) for a few seconds would make "look at PR #412" in chat show *where*. Serves **F**, **N**. Whether the User wants an agent to move his cockpit is a question for him; listed because the source treats it as a headline feature.
- **Effort:** M. A view-side message channel (a file or tmux user option each view polls) and a highlight style.

### 3.11 harlequin

- **What:** "the SQL IDE for your terminal" ([README](https://github.com/tconbeer/harlequin)), Python/Textual.
- **Layout:** components named in source: a **data catalog** tree, a **code editor**, a **results viewer**, a **run-query bar**, a history screen, an export screen, and a **keys panel** ([components](https://github.com/tconbeer/harlequin/tree/HEAD/src/harlequin/components)). Layout of the whole screen is inferred from those names (catalog left, editor and results stacked).
- **Status and alerts:** a transaction mode with commit and rollback commands that exist only when the adapter supports them.
- **Driven by:** command palette on `ctrl+p`; each entry is a `PaletteCommand(title, help, action, over_modals, is_available)`: "the action's key so its bound key can be shown", `is_available` hides entries that do not apply (cancel only if the adapter implements it) ([palette.py](https://github.com/tconbeer/harlequin/blob/HEAD/src/harlequin/palette.py)); `KeysPanel`: "a panel listing the keys bound for the focused widget" ([keys_panel.py](https://github.com/tconbeer/harlequin/blob/HEAD/src/harlequin/components/keys_panel.py)); keymaps and themes in config.
- **Standout:** the palette teaches the shortcuts and never offers what cannot work *now*.

**I-19. A palette whose entries show their shortcut and hide when unavailable.**
- **Pattern:** palette entries carry title, help, bound key, and an availability predicate.
- **Source:** harlequin [palette.py](https://github.com/tconbeer/harlequin/blob/HEAD/src/harlequin/palette.py); crush's `ctrl+p` palette (§3.17, I-31).
- **cpd mapping:** one popup (shared with I-06's "go to") listing actions: New Plan, Archive, Reset layout, Open legend, Open logs, and per selection the right-click menu's items. Availability uses the same rule the tree uses to refuse (`in_chat_slot`, `archive_blocked`): hide the entry or show it dim with the reason instead of failing after the click. Serves **N**; **L** (it answers "what can I do here?").
- **Effort:** M. Needs a table of actions with predicates. The tree's menu items are already commands.

**I-20. A keys panel for the focused pane (replaces a static legend).**
- **Pattern:** a panel that lists only the bindings of what has focus.
- **Source:** harlequin [keys_panel.py](https://github.com/tconbeer/harlequin/blob/HEAD/src/harlequin/components/keys_panel.py).
- **cpd mapping:** the legend popup (`?`) says what colours and icons mean; a second page "keys and clicks here" per view complements it. Cheaper alternative: I-01. Serves **N**.
- **Effort:** S as a legend page; the data (§13.2's key table) is already written down.

### 3.12 lnav

- **What:** "the logfile navigator" ([README](https://github.com/tstack/lnav)): it decompresses, detects formats, merges files **by time into one view**, tails them, "builds an index of errors and warnings" and pretty-prints JSON lines.
- **Layout:** one main view of log lines and a histogram view (`i`); a command/SQL prompt (`:` and `;`). Any status line or chrome beyond that is **inferred**: the README text read does not describe it.
- **Status and alerts:** failed requests in red; identifiers (IPs, PIDs) "semantically highlighted" ([README, screenshot text](https://github.com/tstack/lnav#screenshot)); `e`/`E` jump to the next and previous error.
- **Driven by:** keys and a prompt (`/` regex search, `:` commands, `;` SQLite queries); filters by regex or SQL expression; `:highlight`.
- **Standout:** an *index of what matters* (errors) over a firehose, with one key to the next.

**I-21. Event log: jump to next problem, filter by Plan, histogram.**
- **Pattern:** `e`/`E` to next/previous error; `i` for a message-rate histogram; filters.
- **Source:** lnav [README](https://github.com/tstack/lnav#readme) (hotkeys linked there: [spatial navigation](https://docs.lnav.org/en/latest/hotkeys.html#spatial-navigation), [histogram](https://docs.lnav.org/en/latest/ui.html#hist)).
- **cpd mapping:** the Event log popup (`▤`) scrolls every Event, oldest first, `error` Events in red (§13.2 "logs"). Add `n`/`N` to jump between error and needs-you Events, `/` to filter by Plan id, and a top strip of activity per minute so a quiet stretch or a burst shows. The Pulse view in the Plan view already has per-Plan activity buckets (`docs/commands.md` §13.2, "Pulse"), so the histogram part is a reuse. Serves **P**, **F**.
- **Effort:** S for jump and filter (key handlers in one view); M with the strip.

**I-22. Semantic colour for ids.**
- **Pattern:** identifiers coloured the same wherever they appear.
- **Source:** lnav [README](https://github.com/tstack/lnav#screenshot) ("identifiers, like IP address and PIDs are semantically highlighted").
- **cpd mapping:** ids are the labels everywhere (hs-d101). A stable hash of the Plan id to one of ~8 muted hues, used for the Plan's id in the tree, its PR box title key, the Event log lines, and the Deploy column's owner chip, would let him follow one Plan across panes by colour. Conflicts with the rule that colour means status (palette `Meaning`): use hue only for the id text, never for dots or pills. Serves **P**, **N**, **L**.
- **Effort:** S. A function in the palette module; **unverified** that the User likes coloured ids (it competes with the status colours).

### 3.13 agent-deck

- **What:** "mission control for your AI coding agents": one terminal that shows every session "running, waiting, or done" ([README](https://github.com/asheshgoplani/agent-deck)). Go. Same job as the cockpit's Context column, for Claude Code, Codex, OpenCode and pi sessions.
- **Layout:** the README text read does not describe the screen. It names a session list with named groups ([Declarative groups](https://github.com/asheshgoplani/agent-deck#declarative-groups)) and a key table ([Key Shortcuts](https://github.com/asheshgoplani/agent-deck#key-shortcuts)); a list-plus-footer screen is **inferred**. It holds the terminal in mouse mode (its FAQ: drag-select needs `Shift+drag`).
- **Status and alerts:** `●` green running, `◐` yellow waiting ("needs your input"), `○` grey idle, `✕` red error ([Status Detection](https://github.com/asheshgoplani/agent-deck#status-detection)). **Waiting sessions appear in the tmux status bar**: `⚡ [1] frontend [2] api [3] backend`, and `Ctrl+b`, release, `1`–`6` jumps there ([Notification Bar](https://github.com/asheshgoplani/agent-deck#notification-bar)). A `notify-daemon` "watches status transitions and sends parent nudges when child sessions move `running -> waiting|error|idle`"; a **conductor** session answers routine questions and "escalates the interesting ones to your phone" over Telegram, Slack or ntfy; **watchers** take webhooks and GitHub events.
- **Driven by:** keys (`n` new, `f`/`F` fork, `/` search, `!`/`@`/`#`/`&` filters by running/waiting/idle/error, `$` cost, `M` move to group, `o` send a message to the selected session).
- **Standout:** the *needs-you strip in the status line with a number you press*.

**I-23. A needs-you strip in the StatusBar, one click or number to jump.**
- **Pattern:** waiting sessions listed in the tmux status line, each with a jump key.
- **Source:** agent-deck [Notification Bar](https://github.com/asheshgoplani/agent-deck#notification-bar); tmux `range=user|X` (see I-08).
- **cpd mapping:** the StatusBar (middle, currently empty). Take the first N Inbox items (Plan inbox's Inbox owns the item list, `docs/commands.md` §21) and render `● schema ● api ● web +2`; a click on one opens that Lead in the chat slot (`session open`) and focuses the chat; with the strip always visible, the User does not have to scan the tree to find who waits. The Inbox stays the source of truth: this is another *view* of it, not a second list. Serves **F** (the first pain point), **N**.
- **Effort:** S once the Inbox read model exists (the strip is a read plus a click mapping); **depends on** the Inbox Plan.

```
 ⧉ ? ▤  │ needs you: ● schema/review  ● api/conflict  ● web/q  +1 │ workers 3/4 · 2 queued
```

**I-24. Status filters as one-key toggles above the tree.**
- **Pattern:** `!` `@` `#` `&` filter the list to running, waiting, idle, error.
- **Source:** agent-deck [README, search](https://github.com/asheshgoplani/agent-deck#search).
- **cpd mapping:** four small clickable chips at the top of the Context column (`● needs` `⠋ working` `○ idle` `◌ stopped`) that filter the tree to Sessions in that dot state. This overlaps I-16's `needs me` scope; build one of them (chips contain it). Serves **F**.
- **Effort:** S. View state only.

**I-25. Push to the phone on a state change.**
- **Pattern:** a daemon on status transition sends a notification (ntfy, Telegram, Slack) so a session never "rots" while waiting.
- **Source:** agent-deck [README, conductor and `notify-daemon`](https://github.com/asheshgoplani/agent-deck#conductor).
- **cpd mapping:** outside the cockpit's panes, inside "finding what needs him": when an Inbox item opens and the User has not looked in N minutes, send it to ntfy. The engine already opens and closes items (Plan inbox). Serves **F**. It is a product decision, not a visual one.
- **Effort:** M. A step in the engine and a config key for the channel; **depends on** the Inbox Plan.

### 3.14 savras

- **What:** "a side panel that sees every Claude Code session you have running": lists each as `WAITING`, `WORKING`, `DONE` or `FAILED` with its pull request, context used and age; Rust ([README](https://github.com/Marijusaj/savras)). It reads Claude Code's own state files and never writes to them.
- **Layout:** a narrow panel (side set by `[`/`]`, width by `<`/`>` or by dragging the divider; "tmux draws the panes; Savras is the panel", the cockpit's own split of work) with a working shell beside it; a header counting `1 needs input · 2 working · 4 done`; rows under headings; a detail footer for the selected row; tabs, one per session you open (README "At a glance", panel sample and "Tabs").
- **Status and alerts:** the heading **Needs input** is first and the header says how many; a row shows `WAITING  answer: …` (the question), the context-used percentage and the age; `PR #357` / `MERGED #357` in the row; failed in its own heading.
- **Driven by:** keys (`↑↓`, `r`, `s` regroup, `a` start a parallel agent, `d d` delete, `x` close a tab); `ctrl-w`/`ctrl-s` step through the visible rows from anywhere, in the order shown; mouse: drag the divider.
- **Standout:** two groupings of the same rows and a rule that **rows never move under your cursor**.

**I-26. Two groupings, one toggle: by Context or by what needs me.**
- **Pattern:** `s` flips between grouping by repository (stable) and by status (Needs input first). A lead and its parallel agents are indented under the parent when grouped by repository, and not indented when grouped by status ("an arrow pointing off-screen would be a lie").
- **Source:** savras [README, grouping](https://github.com/Marijusaj/savras#readme) (the "Parallel agents" and `s` text).
- **cpd mapping:** the Context column today is only by Context. A button in its header `by status` re-sorts the same rows into `Needs you / Working / Idle / Stopped` and drops the guides (a row from two Contexts can sit under one heading). The Inbox, at the bottom of the column, is the by-status view of items; this is the by-status view of Sessions. Serves **F**, **P**.
- **Effort:** S. One enum of view state and a different row builder.

```
 by Context (default)      by status (toggle)
 ▾ ● api          3         ── needs you (2) ──
    ⏵ ● schema              ● schema · api
    ○ auth                  ● web
 ▾ ○ web                    ── working (1) ──
                            ⠋ auth · api
```

**I-27. "The order holds still."**
- **Pattern:** rows sort by *when they started*, never by recent activity or by status within a view, "so a row keeps its place for as long as it lives and the list only grows at the end". Freshest-first "made the panel unnavigable: a working session rewrites its timestamp every few seconds, so rows swapped places under your fingers".
- **Source:** savras [README, "The order holds still"](https://github.com/Marijusaj/savras#readme).
- **cpd mapping:** the tree already orders Plans by `created_at` (§13.2). Keep that rule written as a rule, and apply it to the Deploy column too (today oldest-first by PR age; fine) and to any future by-status view: **within** a group order by `created_at`, and do not move a row under the cursor while a click is in flight (freeze reorders for ~1 s after a pointer event; inferred need). Serves **N**.
- **Effort:** S. A constraint, not a feature.

### 3.15 bosun

- **What:** a "tmux-native orchestrator for AI agent sessions" in Rust and ratatui, "a from-scratch reimagining of agent-deck" ([README](https://github.com/yetidevworks/bosun)): the closest cousin of the cockpit's tech.
- **Layout:** a sidebar of sessions (collapsible named **sections**, each row a *container* of tmux sessions with a browser-style tab strip and a `+` button) and a large **preview** that is an embedded PTY terminal of the selected session; "single-window focus mode" opens a session inside the preview, with the sidebar still visible; a coloured focus border marks which pane has the keyboard.
- **Status and alerts:** `●` running, `◐` waiting, `○` idle, `✕` error, "refreshed across every managed session at the fast-preview cadence" (about 5 Hz); tab pills carry status glyphs so "background tabs surface Running / Waiting state without focus"; the sidebar row gets "a small accent dot when any background tab is busy". Status detection recognises the agent's prompt box rather than "scanning the whole capture for fragile substrings, which kills the 'stale Thinking… pegs the glyph to Running' failure mode".
- **Driven by:** "two-way mouse navigation": click a row to jump, click inside the embed to enter focus, drag the divider, wheel scrolls the sidebar; keys (`Shift+↑↓` cycle sessions in sidebar order, "stable, not MRU-shuffled"); `T` switches among 28 themes live.
- **Look:** "Opencode aesthetic. Borderless, subtly shaded panels, bold accents, modal dialogs with left accent bars and drop shadow."
- **Standout:** the visual language: shade instead of lines.

**I-28. Shaded panels and accent-bar dialogs inside the views.**
- **Pattern:** panels are told apart by a slightly different background, not by a border; dialogs get a coloured left bar.
- **Source:** bosun [README](https://github.com/yetidevworks/bosun) ("Opencode aesthetic").
- **cpd mapping:** the cockpit's rule is white tmux borders between slots and grey `─` section headers inside (hs-d124, `docs/commands.md` §13.2). Within a slot, the section blocks (PRs, machine) and the tree's inline prompt and right-click menu could use a one-step-lighter background and a left accent bar instead of `─` headers: a different answer to **L** that costs no cells. It conflicts with the User's design rule, so it is a proposal for him to look at, not for the Lead to apply. Needs truecolor in tmux (`terminal-features` RGB; **unverified** for his Ghostty-over-ssh path).
- **Effort:** M. Palette work plus a pass over each view; no behaviour change.

**I-29. Status on a *container* row: show the worst state of what is inside.**
- **Pattern:** background tabs' Waiting state surfaces as an accent dot on the collapsed row.
- **Source:** bosun [README, tabs](https://github.com/yetidevworks/bosun) ("the sidebar row picks up a small accent dot when any background tab is busy").
- **cpd mapping:** the tree's Context row already shows its Manager's dot (hs-d215) and the Plan count when collapsed; a collapsed Context should also show a needs-you dot when *any* of its Plans' Leads needs him, so folding the tree never hides a need. Serves **F**. Check what `docs/commands.md` §17 ("The tree's dot rule") already does before building: **unverified** that it does not.
- **Effort:** S.

### 3.16 Toad

- **What:** "a unified interface for AI in your terminal" (by Will McGugan; [README](https://github.com/batrachianai/toad)): a front-end to coding agents through the Agent Client Protocol, with a working shell.
- **Layout:** a chat/prompt view with a Markdown prompt editor, streaming Markdown, side-by-side or unified diffs; a fuzzy `@` file picker that switches to a tree on `Tab` (README feature table).
- **Status and alerts:** "hit `ctrl+s` to show the current state of all your agents in one screen"; session resume on `ctrl+r`.
- **Driven by:** keys, "full mouse support" in the prompt editor; a settings screen instead of a config file; the author recommends Ghostty.
- **Standout:** one key for "where is everything?" across agents.

**I-30. One overview screen of every agent's state (merge into I-14).**
- **Pattern:** `ctrl+s` shows all agents' states in one screen.
- **Source:** Toad [README](https://github.com/batrachianai/toad) ("Concurrent sessions").
- **cpd mapping:** the same popup as I-14, with Leads as well as Workers: grouped by Plan, state, last line. It replaces the habit of walking the tree to see who is doing what. Serves **P**, **F**. Treat as the second source for I-14 rather than a new idea.
- **Effort:** M, shared with I-14.

### 3.17 crush

- **What:** "the glamourous AI coding agent" from Charm ([README](https://github.com/charmbracelet/crush)): a Bubble Tea chat TUI with sessions, LSP and MCP.
- **Layout:** a chat view with a session picker; a compact mode (`tui.compact_mode`); a command palette on `ctrl+p`, model picker on `ctrl+l`.
- **Status and alerts:** permission prompts before tool calls (`--yolo` skips them); **desktop notifications** "when a tool call requires permission and when the agent finishes its turn", "only sent when the terminal window isn't focused *and* your terminal supports reporting the focus state"; modes `auto, native, osc, bell, disabled`; `auto` "uses native notifications locally and OSC notifications over SSH when supported" ([README, Desktop notifications](https://github.com/charmbracelet/crush#desktop-notifications)). Sessions expose `IsBusy` and `AttachedClients`, "the cue that a session is 'in progress' on another client" ([README, sharing a workspace](https://github.com/charmbracelet/crush#sharing-a-workspace-across-clients)).
- **Driven by:** keys and the palette.
- **Standout:** notifications that fire only when you are not looking.

**I-31. Notify the User when a need appears while the terminal is unfocused.**
- **Pattern:** notification only when the window is unfocused and focus reporting works; OSC over SSH.
- **Source:** crush [Desktop notifications](https://github.com/charmbracelet/crush#desktop-notifications).
- **cpd mapping:** the cockpit already turns on tmux `focus-events` (§13.2 "tree", Cursor) and the User runs it over ssh from a Mac terminal (§13.2 Copy id text names Ghostty). When an Inbox item opens and no client is focused, tmux can ring the bell (`monitor-bell`/`bell-action`) or a view can write an OSC 9/777 notification through tmux's passthrough (`allow-passthrough`; **unverified** that Ghostty shows it). Serves **F**.
- **Effort:** S for the bell (a tmux option), M for OSC (passthrough and a focus test per client); **depends on** the Inbox for the trigger.

**I-32. Theme palette with live preview.**
- **Pattern:** `ctrl+p` → Themes: "the UI previews each theme as you navigate", `enter` confirms, `esc` reverts; `ctrl+e` edits a theme with a live preview and saves to `themes/`.
- **Source:** crush [README, Themes](https://github.com/charmbracelet/crush#themes); posting's 37 themes ([README](https://github.com/darrenburns/posting)) and bosun's 28 live-switchable themes.
- **cpd mapping:** the palette is code (`palette::fg(Meaning::…)`). A theme chooser needs the palette to be data first. The User did not ask for choice, he asked for a *sleek* look; do this only after the powerline look lands. Serves **L**.
- **Effort:** L. Palette as data, tmux border and status colours regenerated, and every view re-reading it.

### 3.18 flowrs

- **What:** an Apache Airflow TUI in ratatui: "monitor, inspect and manage Airflow DAGs from the comforts of your terminal" ([README](https://github.com/jvanbuel/flowrs)). A DAG of tasks with runs and states: the nearest structure to a Plan of Jobs.
- **Layout:** drill-down views, one per level (from the model folders: config, dags, dagruns, logs; task instances; [src/app/model](https://github.com/jvanbuel/flowrs/tree/HEAD/src/app/model); inferred), each a **filterable table** with an autocompleting filter (`src/app/model/filter`), popups for clear, mark and trigger; a DAG **graph** (`src/airflow/graph.rs`) and a **gantt** model (`src/airflow/model/common/gantt.rs`).
- **Status and alerts:** state per run and per task instance; error and warning popups.
- **Driven by:** keys; **each view has a command popup** listing name, key and description: Clear `c`, "DAG Graph" `d` ("Show DAG task dependency graph"), Show `v`, Visual `V`, Mark `m`, Trigger `t` ([dagruns/commands.rs](https://github.com/jvanbuel/flowrs/blob/HEAD/src/app/model/dagruns/commands.rs)); visual selection for bulk actions.
- **Standout:** every view carries its own "what can I do here" popup.

**I-33. A per-selection command popup on Jobs (a right-click menu for pills).**
- **Pattern:** a popup of (name, key, description) for the selected item, plus a `Visual` mode for bulk.
- **Source:** flowrs [dagruns/commands.rs](https://github.com/jvanbuel/flowrs/blob/HEAD/src/app/model/dagruns/commands.rs).
- **cpd mapping:** the tree has a right-click menu (every item is a `cpd` command); the Plan view has keyboard selection and `Enter` to open a Worker, and (inferred from §13.2's table, which lists no menu) no right-click menu on a Job pill. Add one: Open Worker, Edit, Cancel (`job cancel`, §5.3), Copy key, with each item's command. Serves **N**, **P**.
- **Effort:** M. A menu widget exists in the tree; it has to be shared with the Plan view and the actions mapped to commands.

**I-34. A time view of Jobs (Gantt).**
- **Pattern:** a Gantt model for runs, next to the graph.
- **Source:** flowrs [gantt.rs](https://github.com/jvanbuel/flowrs/blob/HEAD/src/airflow/model/common/gantt.rs) (exists; how it renders is **unverified**: only the file name was read).
- **cpd mapping:** the flow chart shows *dependency*; a lane per Job over time shows *what ran when* and where a Plan waited. A toggle in the Plan view's header (`flow | time`). Serves **P**.
- **Effort:** L. A new renderer and time data (Attempt start/end are in the store; only whether the Plan view reads them is unchecked).

### 3.19 process-compose

- **What:** a process orchestrator with a TUI: processes with `depends_on`, health checks (liveness and readiness), restarts, namespaces, a dependency graph view ([README](https://github.com/F1bonacc1/process-compose)). Go with tview.
- **Layout:** from `createGrid` ([main-grid.go](https://github.com/F1bonacc1/process-compose/blob/HEAD/src/tui/main-grid.go)): a stat table on top, a command input (command mode), the **process table**, the **log** pane, and a help footer; three split states, `LogFull`, `ProcFull` and `LogProcHalf`; a full-screen mode that drops the stat table and footer; an interactive process shows a PTY view instead of the log.
- **Status and alerts:** the process table columns: icon (`●`, coloured), pid, name, namespace, status, age, health, mem, cpu, restarts, exit code ([proc-table.go](https://github.com/F1bonacc1/process-compose/blob/HEAD/src/tui/proc-table.go)).
- **Driven by:** keys with configurable shortcuts, a command palette (`command-palette.go`), a namespace selector, mouse (the terminal view has mouse tests).
- **Standout:** the *table with status columns* as the default for "what is every job doing".

**I-35. A Jobs table mode for the Plan view.**
- **Pattern:** one dense row per process with icon, status, age, health, cpu/mem, restarts; log below.
- **Source:** process-compose [proc-table.go](https://github.com/F1bonacc1/process-compose/blob/HEAD/src/tui/proc-table.go) and [main-grid.go](https://github.com/F1bonacc1/process-compose/blob/HEAD/src/tui/main-grid.go).
- **cpd mapping:** the flow chart is best for dependency and gets wide; a table of the Plan's Jobs (key, tier, status icon, Attempt number, age, last tool line) sorted by *needs attention first* scans faster when there are more than ~10 Jobs. A header toggle `flow | table` (with I-34's `time`). Serves **P**, **F**.
- **Effort:** M. A second renderer over the same Job data; selection and Enter reuse the chart's.

```
 key          tier  att  state       age   last
 ⠹ schema/gql  S2   a1   running     3m    cargo test
 ✗ web/login   S2   a2   failed      9m    exit 1
 ○ web/ui      S1   a1   queued      —
```

### 3.20 Reeve

- **What:** "a terminal cockpit for AI agents: watch a run live, score it, and step in when it goes sideways" ([README](https://github.com/Dancode-188/reeve)). Rust (listed in [awesome-ratatui](https://github.com/ratatui/awesome-ratatui)); takes OpenTelemetry spans.
- **Layout:** a live **trace tree** that builds as the agent works, with the streaming response and a blinking cursor; a health gauge; History on `3`.
- **Status and alerts:** a health score 0–100 that "changes color as it drops. Green, amber, red"; anomaly checks (loop detection, cost, latency, intent vs action) raise alerts; **policy rules** such as `health_score < 30` or `predicted_cost_at_completion > 10.0` fire automatically.
- **Driven by:** keys: `i` to intervene (pause, redirect with a new instruction, inject context, kill the trace); the agent "acknowledges every step back to the cockpit, so you watch the command land instead of hoping it did"; `R` replays a finished trace "like a DVR".
- **Standout:** an intervention is visibly *delivered*, and its result is measured.

**I-36. Show delivery state of a command to a Worker or Lead.**
- **Pattern:** a command goes through sent → acknowledged → applied, shown inline.
- **Source:** Reeve [README, "Intervene"](https://github.com/Dancode-188/reeve#what-it-does).
- **cpd mapping:** when he cancels or steers an Attempt (Job pill menu, I-33), the pill shows `⟳` until the Worker confirms, so "did my Cancel land?" is answered on screen. Serves **P**. Only worth building once there is a steer action to confirm.
- **Effort:** M. A state for the pending command in the engine and one glyph.

**I-37. Rules that raise alerts: stuck, expensive, looping.**
- **Pattern:** user-written conditions over live metrics; the alert carries its track record.
- **Source:** Reeve [README, "React"](https://github.com/Dancode-188/reeve#what-it-does).
- **cpd mapping:** the Pulse view already marks a **quiet** Plan (no activity for 30 min, a constant, `docs/commands.md` §13.2). A second kind of rule (an Attempt past its tier's usual time, a third Attempt of one Job) would open an Inbox item, which is the one place "needs you" lives. Serves **F**, **P**. Belongs to the Inbox owner; listed because Reeve's rules and the 30-minute constant are the same idea.
- **Effort:** L. Engine rules, config, and Inbox kinds; crosses Plan inbox.

## 4. All ideas in one list

Cockpit area: **Tree** (Context column), **Plan** (Plan view), **Chat**, **Deploy** (right column), **Bar** (StatusBar, moving to tmux's status line), **Pop** (popups), **tmux** (layout and options).

| Id | Idea | Area | Pain | Effort | Source | Depends on |
|---|---|---|---|---|---|---|
| I-01 | Context hint line | Bar | N | S | lazygit | |
| I-02 | Panel jump numbers in headers | tmux | N | S | lazygit | |
| I-03 | Zoom a pane (click header); narrow-window layout | tmux | N, L | S / M | lazygit | |
| I-04 | Saved-question sections for PRs | Deploy | F, P | S | gh-dash | |
| I-05 | Preview strip for the selected Job | Plan | P | M | gh-dash, yazi | |
| I-06 | `:` jump palette (go to any Context/Plan/PR/Job) | Pop | N | M | k9s | |
| I-07 | User-bound hotkeys file | Tree | N | M | k9s | not recommended now |
| I-08 | Clickable status-line ranges with verbs | Bar | N, L | S | zellij, tmux | status-line move |
| I-09 | Stacked Worker view (one expanded, rest titles) | Plan/Chat | P, N | M | zellij | |
| I-10 | Layout presets from `⧉` | tmux | N, L | S | btop | |
| I-11 | Braille sparkline for host load | Deploy | P, L | S | btop | CPU/memory widget |
| I-12 | Expand selected view | tmux | N | S | bottom | |
| I-13 | Layout as a data file | tmux | N | M | bottom | not recommended now |
| I-14 | Workers popup (everything in flight, cancel) | Pop | P, F | M | yazi, Toad | |
| I-15 | Collapsible Deploy column with a badge | tmux/Deploy | L, F | S | superfile | |
| I-16 | Live filter and `needs me` scope in the Tree | Tree | F, N | S | taskwarrior-tui | Inbox owner decides |
| I-17 | Jump mode (labels over panes and rows) | tmux/Tree | N | S / M | posting | |
| I-18 | Agent pointing: a Lead highlights a row | all | F, N | M | posting | User decision |
| I-19 | Palette with shortcuts and availability | Pop | N, L | M | harlequin, crush | shares I-06 |
| I-20 | Keys panel for the focused pane | Pop | N | S | harlequin | |
| I-21 | Event log: next problem, Plan filter, histogram | Pop | P, F | S / M | lnav | |
| I-22 | Semantic colour for ids | all | P, N, L | S | lnav | User taste |
| I-23 | Needs-you strip with click-to-jump | Bar | F, N | S | agent-deck | Inbox |
| I-24 | Status chips filtering the Tree | Tree | F | S | agent-deck | overlaps I-16 |
| I-25 | Push to phone on a new need | engine | F | M | agent-deck | Inbox; product decision |
| I-26 | Group by Context / by status toggle | Tree | F, P | S | savras | |
| I-27 | "The order holds still" rule | Tree/Deploy | N | S | savras | |
| I-28 | Shaded panels and accent-bar dialogs | all views | L | M | bosun | conflicts with hs-d124; truecolor |
| I-29 | Worst state on collapsed rows | Tree | F | S | bosun | check §17 dot rule |
| I-30 | One screen of every agent's state | Pop | P, F | M | Toad | merge into I-14 |
| I-31 | Notify when a need appears while unfocused | tmux/Bar | F | S / M | crush | Inbox |
| I-32 | Theme chooser with live preview | palette | L | L | crush, posting, bosun | after the powerline look |
| I-33 | Job right-click menu | Plan | N, P | M | flowrs | |
| I-34 | Time (Gantt) view of Jobs | Plan | P | L | flowrs | |
| I-35 | Jobs table mode | Plan | P, F | M | process-compose | |
| I-36 | Delivery state of a command | Plan | P | M | Reeve | a steer action |
| I-37 | Rules that open Inbox items (stuck, looping) | engine | F, P | L | Reeve | Inbox |

## 5. Where this points (for the Plan's next Job to weigh, not a decision)

- **Finding what needs him** is solved by *one place that is always visible*. The tools that do it best put it in the status line (agent-deck I-23), at the top of the list (savras I-26) or on a collapsed row (bosun I-29), never only in a tab. The Inbox owns the data; the visual claim here is that its first three items belong in the StatusBar, and that no fold of the tree may hide a need (I-29).
- **Following progress** is solved by *a table you can sort and a preview that follows the cursor* (process-compose I-35, yazi I-14, gh-dash I-05), not by more charts. The Plan view's flow chart is already the strongest "dependency" picture in the survey (only flowrs has a DAG view, and only as a popup); what it lacks is the dense "what is every Job doing now" view.
- **Getting around** is solved by *two doors to anywhere*: a typed one (k9s `:`, harlequin and crush palettes: I-06, I-19) and a labelled one (posting jump mode: I-17). Every mouse-first tool in the list (lazygit, btop, bosun, posting) *also* labels its keys on screen (I-01, I-20).
- **Looks and polish**: btop and bosun are the two looks worth copying and they disagree (btop: boxes, graphs, rounded borders; bosun: borderless, shaded). The powerline Job (hs-d266) decides this first; I-28 is the alternative to look at.
- **Small and independent** (S, no dependency): I-01, I-03 (zoom), I-04, I-10, I-11, I-12, I-15, I-21 (jump and filter), I-26, I-27. **Waiting on the Inbox**: I-23, I-25, I-31, I-37.

## 6. The 20 and the four pain points

● main fit, ○ secondary fit. Idea ids are from §4.

| # | Tool | F: finding what needs him | P: following progress | N: getting around | L: looks and polish | Ideas |
|---|---|:-:|:-:|:-:|:-:|---|
| 1 | lazygit | | ○ | ● | ○ | I-01, I-02, I-03 |
| 2 | gh-dash | ● | ● | ○ | | I-04, I-05 |
| 3 | k9s | ○ | | ● | | I-06, I-07 |
| 4 | zellij | | ○ | ● | ○ | I-08, I-09 |
| 5 | btop | | ○ | ○ | ● | I-10, I-11 |
| 6 | bottom | | | ● | ○ | I-12, I-13 |
| 7 | yazi | ○ | ● | | | I-14 |
| 8 | superfile | ○ | | ○ | ● | I-15 |
| 9 | taskwarrior-tui | ● | | ○ | | I-16 |
| 10 | posting | ○ | | ● | | I-17, I-18 |
| 11 | harlequin | | | ● | ○ | I-19, I-20 |
| 12 | lnav | ○ | ● | | ○ | I-21, I-22 |
| 13 | agent-deck | ● | ○ | ○ | | I-23, I-24, I-25 |
| 14 | savras | ● | ○ | ○ | | I-26, I-27 |
| 15 | bosun | ○ | | ○ | ● | I-28, I-29 |
| 16 | Toad | ○ | ● | | | I-30 |
| 17 | crush | ● | | | ○ | I-31, I-32 |
| 18 | flowrs | | ● | ○ | | I-33, I-34 |
| 19 | process-compose | ○ | ● | | | I-35 |
| 20 | Reeve | ○ | ● | | | I-36, I-37 |
