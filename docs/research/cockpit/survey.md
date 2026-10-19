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

