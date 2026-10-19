# Cockpit research: finding what needs the User

Job `insp-attention`, Plan `tui-visuals`, PR `inspiration`. Research only: no code, nothing here is decided. It builds on [survey.md](survey.md) (ids `I-01`…`I-37`) and does not redo the Inbox (Plan inbox), the powerline look (hs-d266) or dynamic titles (hs-d257). Idea ids here are `A-01`…`A-14`, ranked by how much they would make the User more efficient at **F**inding what needs him (the first of his four pain points; the others are tagged **P** progress, **N** navigation, **L** looks, as in the survey). Effort is **S** (a day or less, one view), **M** (a few days, one view plus a command, a tmux hook or one engine field) or **L** (a week or more, or crossing the engine and several views).

## 0. The answer in six lines

1. The best tools solve "what needs me" with **one strip that is always on screen** (agent-deck, tmux-agent-tracker, weechat's hotlist), whose items are **numbered** so one click or one key lands on the cause (A-01, A-02).
2. They tell you **what it wants**, not only that something does: the question text in the row (savras), the latest notification text (cmux) (A-03).
3. They keep the **verb next to the problem**: approve, deny, merge, mark done, without opening anything (cmux Feed, gh-dash) (A-05), and a **notification centre** with a preview of the cause for the pile (gh-dash, ilmari, cmux) (A-04).
4. They separate **new from seen** (ilmari, tracker, cmux, savras): loud until you have looked, calm after (A-07).
5. They do not move rows under your pointer and do not announce what you already know (savras) (A-10, A-11).
6. cpd's in-flight Inbox section is a good base. Its slow spots are: three rows, no cause text, no click for the PR kinds, a counter that cannot be clicked, no marker on the thing the item is about (§2). Ideas A-01…A-06 fix those, and A-01, A-02, A-05 together are small (S/M) and need only the Inbox read model.

## 1. Method and sources

- Started from the survey (`docs/research/cockpit/survey.md` on this branch), its 20 tools and their I-ids. Kept only what is about *attention*; read nothing of it twice except to go deeper.
- Read through `gh api repos/<owner>/<repo>/readme` (raw) and `contents/<path>`: [awesome-tuis](https://github.com/rothgar/awesome-tuis) and [awesome-ratatui](https://github.com/ratatui/awesome-ratatui) again, this time for notification, inbox, triage, alert and toast tools. `gh search repos` returned nothing for "agent tmux waiting" queries in this session; two tmux plugins came from `gh api search/repositories`, the rest from the lists and from the survey.
- cpd side: `docs/contract.md` §2.2 "Inbox item", `docs/commands.md` §21 (`inbox`), §13.2 (`tui view`), §17 (dot rule) on this branch (`main`). **The Inbox section of the tree, the clickable-cause table and the `inbox <n>` counter are not on `main`**: they are in flight on `origin/cpd/inbox/cockpit` and were read there (`git show origin/cpd/inbox/cockpit:docs/commands.md`, §13.2 "Inbox section", §17). They may still change; every statement about them says "in flight".
- tmux facts are from `tmux(1)` of tmux 3.8 on this machine. The cockpit's minimum is tmux 3.4 (`docs/commands.md` §14.1), so each tmux feature below is **unverified on 3.4** unless said otherwise.
- Screenshots cannot be seen. Layouts are from READMEs, docs and source; **inferred** marks a layout or behaviour guessed from names or code paths.
- ASCII mockups: colour is lost. In every mockup `●` red, `◐` yellow, `◆` magenta, `○` white (the Inbox's four urgencies, contract §2.2) are a *shape stand-in for colour*, and the glyph after it is the proposed kind glyph (A-09). `·` is an idle dot and `⠹` a working spinner. Full-screen mockups are 215×62 cells with today's geometry (tree 43 columns, plan and chat 127, Deploy 43, one status row; `docs/commands.md` §13.1); the pane headers are drawn with `━`. Content (Plan, ids, numbers) is invented for the picture.

### Tools new to this theme, and what was read

| Tool | Kind | Read | URL |
|---|---|---|---|
| tmux-agent-tracker | tmux plugin, Lua | README: agent bar (a second status line), pane-border badges, jump keys, ordering, overflow | https://github.com/ulas96/tmux-agent-tracker |
| ilmari | tmux popup radar, Rust (ratatui) | README: popup list, sticky attention latch, status summary fragments, bell on transitions | https://github.com/bnomei/ilmari |
| tmux-agent-status | tmux plugin | README: fzf picker sorted needs-first, status-line summary, dismiss | https://github.com/gudoshnikovn/tmux-agent-status |
| cmux | macOS terminal app for agents (a GUI, **not a TUI**: used for patterns only) | README, `docs/notifications.md`, `docs/feed.md` | https://github.com/manaflow-ai/cmux |
| WeeChat | IRC client (curses) | user guide (notify levels, status-bar `hotlist` item), quickstart (`Alt+a`) | https://github.com/weechat/weechat/blob/HEAD/doc/en/weechat_user.en.adoc |
| Glances | system monitor | `docs/aoa/events.rst`, `index.rst`, `actions.rst` | https://github.com/nicolargo/glances/tree/HEAD/docs/aoa |
| Textual | Python TUI framework | `App.notify` in `src/textual/app.py` (toast model) | https://github.com/Textualize/textual/blob/HEAD/src/textual/app.py |

Gone deeper than the survey: gh-dash's notifications view ([sections](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/notification-section.mdx), [keys](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/getting-started/keybindings/selected-notification.mdx)), [savras](https://github.com/Marijusaj/savras) (rows, "Ping", "The order holds still"), [agent-deck](https://github.com/asheshgoplani/agent-deck#notification-bar) (Notification Bar). Toast crates listed in awesome-ratatui (`ratatui-toaster`, `ratatui-comfy-toaster`, `tui-overlay`) were **not read**: their crates.io pages did not load, and a toast is per process (A-11 says why that matters).

## 2. Where the in-flight Inbox is slow to act on

What exists, all **in flight** on `origin/cpd/inbox/cockpit` (`docs/commands.md` §13.2, §17) unless it says `main`:

- Items: 11 kinds, 4 urgencies, ordered red, yellow, magenta, white, oldest first, `engine-down` first (`main`: contract §2.2 "Order"). Each has `label`, `event` (the cause), `url`, links to context/plan/job/session/record. `snooze` (15 min, 1 h, tomorrow 09:00) and `dismiss` are the only commands that change one (`main`: §21.3, §21.4).
- The tree's bottom section ` Inbox <n> `: a header, **up to 3** rows `󰪥 <label>  <age>`, `+N more` to expand; `s` snooze, `d` dismiss, Enter jumps, right-click menu with the four hide entries. A row's click jumps to the cause: `dialog`, `unread`, `job-stuck` → `session open` + focus chat; `engine-*` → logs popup at the Event; `question` → Manager; **`plan-review`, `pr-failing`, `pr-changes`, `pr-ready`, `review-requested` → no click: the row text is an OSC 8 link (modifier-click in the terminal)**.
- The StatusBar's right side: `inbox <n>` in the most urgent colour; **"A click on the counter does nothing."**
- Dots: a Session row's `󰪥` comes from its own items; a Context row's only from its Manager's items (hs-d215, hs-d217): **the Context's other items do not colour it**.

The gaps, each tagged with the ideas that answer it:

| # | Gap | Why it costs the User time | Ideas |
|---|---|---|---|
| G1 | The section is 3 rows in a 43-column pane; labels are templates (`<who> asks you`, `<repo>#<n> checks failed`) cut with `…` | He cannot rank or decide from the row; he must open each | A-03, A-04 |
| G2 | The only always-visible signal is a number and a colour; it names nothing and is not clickable | He still scans the tree or the section to find *which* | A-01, A-02 |
| G3 | The five PR/review kinds need a modifier-click on a link; the Deploy column already uses "row click opens the Lead, `↗` opens the link" | Two conventions for the same object; a plain click does nothing | A-05 |
| G4 | The only verbs are snooze and dismiss | Retry, open Worker, open the page are separate trips | A-05 |
| G5 | Nothing marks the Job, PR box or Deploy row the item is about | After the jump he must find the red thing again | A-06, A-08 |
| G6 | One glyph for every kind; no new/seen; a new red item takes the top row and shifts the rest | Reading cost per row; mis-click risk on an irreversible `dismiss` | A-07, A-09, A-10 |
| G7 | A collapsed Context shows only its Manager's dot | Folding the tree hides the Plans' needs from the tree (the section still lists them, 3 at a time) | A-13 |

## 3. The ideas, ranked

Rank = expected time saved per day for the User, then cheapness. Every idea: **pattern → source → cpd mapping → effort**, with a mockup where layout matters. Ideas A-01…A-08 have mockups.

### A-01. A needs-you strip in the status line: numbered, urgency-coloured, click or key to jump

**Pain:** F, N. **Builds on:** survey I-23 (agent-deck's strip), deeper.

- **Pattern.** tmux-agent-tracker adds a **second status line, the "agent bar"**, one slot per agent: `✳¹ᵠ` is slot 1, waiting (yellow); `✳²ᶜ` finished and not looked at (orange); `✳³ᶜ` finished and seen (green); a spinner when busy. Names shrink to fit and "the remainder is counted as a `+3` on the end rather than quietly falling off the edge". Keys: `M-a`, then `1`–`9` jumps to that agent; `prefix + Q` the next one that is waiting ([README](https://github.com/ulas96/tmux-agent-tracker)). agent-deck shows waiting sessions in the tmux status bar as `⚡ [1] frontend [2] api [3] backend` and `Ctrl+b`, release, `1`–`6` jumps there ([Notification Bar](https://github.com/asheshgoplani/agent-deck#notification-bar)). WeeChat's status-bar `hotlist` item lists the buffers with activity, each with its number and its counts: `H: 3:#abc(2,5), 5`, and `Alt+a` jumps to the next one ([user guide, bar items](https://github.com/weechat/weechat/blob/HEAD/doc/en/weechat_user.en.adoc); [quickstart](https://github.com/weechat/weechat/blob/HEAD/doc/en/weechat_quickstart.en.adoc), "Alt+a: jump to buffer with activity (in hotlist)"). ilmari publishes the same as a tmux format fragment `@ilmari_status_summary` ([README](https://github.com/bnomei/ilmari#put-ilmari-ui-in-your-tmux-theme)).
- **cpd mapping.** The StatusBar's middle is empty today and the in-flight design puts only `inbox <n>` on its right, not clickable (§2, G2). Fill the middle with the **first items of the Inbox, in Inbox order** (red, yellow, magenta, white; oldest first; `engine-down` first), each `<urgency><n> <kind glyph> <short who/what>`. A click on one runs the same jump as clicking the Inbox row (the in-flight click table: `session open` + focus chat, the logs popup, or the Manager). White `unread` items go last and are the first to fold into the overflow `+N`; `+N` opens the triage popup (A-04). The Inbox stays the one source: the strip is a view of it, never a second list. With nothing open it reads `○ nothing needs you` (dim).
- **How, in tmux.** If the bar stays a ratatui pane (today's design) it can hover and spin the working dots; clicks are `MouseDown1Pane` forwarded with `send-keys -M` as now (contract §6.1). If it moves to tmux's status line (the task's direction), each item is a **user range**: `range=user|inb3` (at most 15 bytes) in `status-format[]`, a binding on `MouseDown1Status` reads `#{mouse_status_range}` and runs the jump; the text comes from a `#(…)` helper refreshed by `status-interval` and `refresh-client -S` (tmux(1) 3.8, "range=user|X", "status-format", "status-interval"; **unverified on 3.4**). tmux has no hover, so the verbs would be shown, not tooltipped (survey I-08).
- **Effort.** S as a ratatui row (the views already read the Inbox for the section); M in the tmux status line (ranges, helper, bindings). Depends on the Inbox read model.

```
 ⧉ ? ▤ │ ●1 ✗ core-docs  ●2 ✗ cpd#92  ◐3 ↗ infra#7  ◐4 ✓ cpd#91  ◆5 ☰ quality  ◆6 ? tui-visuals  +1 │                                                                                                      workers 3/4 
 ⧉ ? ▤ │ ●⚠ engine is down · click for the log  ●1 ✗ core-docs  ●2 ✗ cpd#92  ◐3 ↗ infra#7  +5 │                                                                                                            workers 3/4 
 ⧉ ? ▤ │ ○ nothing needs you │                                                                                                                                                                             workers 3/4 
```

### A-02. "Next": one move to the cause of the first item, and one move back

**Pain:** F, N.

- **Pattern.** WeeChat `Alt+a` ("jump to buffer with activity", [quickstart](https://github.com/weechat/weechat/blob/HEAD/doc/en/weechat_quickstart.en.adoc)). tmux-agent-tracker `prefix + Q` "next agent **waiting** on you", `prefix + Tab` "flip between the last two", and inside its `M-a` mode the keys that hunt keep the mode active while the ones that land you in a pane drop out of it, "so your next keystroke goes to the agent and not to a binding" ([README, Keys](https://github.com/ulas96/tmux-agent-tracker)). cmux `⌘⇧U` "jump to latest unread" and `⌃⌘U` "mark the current item as oldest unread and jump to the next latest unread" ([notifications.md, Navigation](https://github.com/manaflow-ai/cmux/blob/HEAD/docs/notifications.md)). savras `ctrl-w`/`ctrl-s` step through "the rows you can see, in the order you see them" and landing on one opens it ([README](https://github.com/Marijusaj/savras)).
- **cpd mapping.** A `▶` button at the strip's left edge and one key: it runs the cause jump of the **first visible item** (the same actions as an Inbox click, so no new decision logic: the cockpit "never decides"). After the User acts (a verb, A-05; or snooze) the item closes or hides and `▶` goes to the next. That turns the Inbox into a **triage loop with one control**: press, handle, press. A `↩` button next to it re-opens the Lead that held the chat slot before the jump (`session open <previous>`; the previous holder is in the `session-moved` Events with `place: chat` that the dot rule already reads, §17). The key must reach tmux through Ghostty and ssh: `M-…` needs Option-as-Meta in Ghostty (the tracker README gives `macos-option-as-alt = left`; **unverified** here for the User's setup), so a mouse button is the primary and the key a shortcut.
- **Effort.** S. One command or key handler over the Inbox read and the existing `session open` / logs popup actions; no engine change. Depends on the Inbox.

```
  ▶ next           (click, or key)                     ↩ back
  │                                                     │
  ▼                                                     ▼
  first visible item ── its cause ──▶ chat slot shows    chat slot shows the Lead that held it
  ● ✗ core-docs failed                its Lead (or logs, before the jump
  you act (A-05) → item closes        or the Manager)
  ▶ again → ●2 ✗ cpd#92 …
```

### A-03. Say what it wants: a one-line cause under every item

**Pain:** F, P.

- **Pattern.** savras's rows carry the actual question: `WAITING  answer: All five items are tier…` and `approve Bash: M=/User…`, under a heading `Needs input` and a header that counts `1 needs input · 2 working · 4 done` ([README](https://github.com/Marijusaj/savras)). cmux's author built the sidebar because the agent's notification body was always "Claude is waiting for your input" with no context; the sidebar shows "the latest notification text for each workspace" ([README, Why cmux?](https://github.com/manaflow-ai/cmux#why-cmux)). Glances' alert list adds the cause to the row: "top processes during the alert" and the duration ([events.rst](https://github.com/nicolargo/glances/blob/HEAD/docs/aoa/events.rst)).
- **cpd mapping.** The Inbox `label` is a template (contract §2.2: `<who> asks you`, `<repo>#<n> checks failed`), so two `dialog` items look the same and he must open each to rank them. Add a **detail**: one dim line (about 80 characters, cut with `…`) under the selected row, the first row, and in the popup (A-04): `dialog` the question's first line; `unread` the reply's first line; `job-stuck` the Job's `failed_cause` and the Worker's last line; `pr-failing` the names of the failing checks (`lint ✗ test ✗, 2 of 9`); `plan-review` the Plan title and Job count; `question` the record's text; `pr-changes` the reviewer's name. Most is already in the store (`jobs.failed_cause` is cleared by `job retry`, §5.4; `pull_requests` checks and `review_decision`, contract §2.2; Context records). For `dialog` and `unread` the text is in the Session's pi file, which the history and Worker views already render read-only; reading it on each render is **inferred** to be too slow, so E8 (the step that opens the item) would store the excerpt when it opens the item.
- **Effort.** M. S for the kinds whose data is already in rows (computed by the view); M for `dialog`/`unread` (a stored excerpt: a new item field, a migration, E8; `docs/contract.md` §2.2 has no such field today). Belongs to the Inbox owner; the visual part is a second line.

```
 ──[ Inbox 7 ]────────────────────────
 ▌● ✗ inbox/core-docs failed      21m
 ▌  build: cargo doc exit 1 (Attempt 2)
  ● ✗ cpd#92 checks failed         8m
    lint ✗  test ✗  (2 of 9 red)
  ◐ ↗ review infra#7               1h
    “bump the pinned runner image”
  ◆ ? tui-visuals asks you         3m
    “squash or rebase the merges?”
  +3 more
```

### A-04. A triage popup: the notification centre, with a preview of the cause

**Pain:** F, N.

- **Pattern.** gh-dash's Notifications view: tabs per reason (`reason:review-requested`, `reason:mention`, …), a preview pane, `D` mark done, `m` mark read, `Enter` "view notification (fetches content, marks as read)", and the PR keys (`v` approve, `m` merge, `w` watch checks) live inside the preview, with `?` showing only the ones that apply ([sections](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/notification-section.mdx), [keys](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/getting-started/keybindings/selected-notification.mdx)). ilmari is a tmux **popup** radar: j/k, `Enter` jumps to the pane and closes the popup, the most recent waiting item preselected, `b` toggles the bell ([README](https://github.com/bnomei/ilmari#popup-controls)). tmux-agent-status's `prefix + j` popup lists panes "sorted needs-your-input-first", `tab` multi-selects and `ctrl-x` dismisses ([README](https://github.com/gudoshnikovn/tmux-agent-status)). cmux has a Notification panel on `⌘I` ([notifications.md](https://github.com/manaflow-ai/cmux/blob/HEAD/docs/notifications.md)). Glances' alert list shows ongoing `●` and resolved `○` with counts in the title ([events.rst](https://github.com/nicolargo/glances/blob/HEAD/docs/aoa/events.rst)).
- **cpd mapping.** A `display-popup` (as the legend and the Event log are; 80 % of the client at most) opened by the strip's `+N`, by a click on the Inbox header, or by a key. Left: all open items grouped by urgency, with age; right: the selected item's **cause**, which cpd already stores: the `event` the item links to (the Event log's `--event` positioning is in flight for the same reason), the Worker's last lines (the Worker renderer), the Lead's last message, and what the item blocks (queued Jobs behind a failed one). Under it, the verbs of A-05. Below the list: `hidden` (the `inbox list` output already counts snoozed and dismissed, §21.1) and the recently resolved tail (A-12). Mouse: click selects, buttons act; keys are shortcuts. It solves G1: the column no longer has to hold the pile.
- **Effort.** M. One new view reading `inbox list` (§21.1) and `inbox show` (§21.2), reusing the Worker renderer and the logs view's `--event`; all actions are existing commands.

```
━[ Context ]━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╋━[ Plan ]━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╋━[ Deploy ]━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
   · · · · · · · · · · · · · · ·           ┃                                                                                                                               ┃
   · · · · · · · · · · · · · · ·           ┃                                                                                                                               ┃
   · · · · · · · · · · · · · · ·           ┃                                                                                                                               ┃
   · · · · · · · · · · · · · · ·           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                     ╭─ inbox · 7 open · 1 snoozed ──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
                     │                                                                    │                                                                                                      │
                     │ ── red ─────────────────────────────────────────────────────────── │ inbox/core-docs failed                         red · job-stuck · open 21m                            │
                     │ ▌● ✗ inbox/core-docs failed                                    21m │ Plan inbox · PR core (cpd#92) · Job core-docs · S2 · Attempt 2 of 3                                  │
                     │   ● ✗ cpd#92 checks failed                                      8m │                                                                                                      │
                     │ ── yellow ──────────────────────────────────────────────────────── │ Cause (Event 48311 job-failed, 21m ago)                                                              │
                     │   ◐ ↗ review infra#7                                            1h │   cargo doc --no-deps exited 1                                                                       │
                     │   ◐ ✓ cpd#91 ready to merge                                    40m │   unresolved link to `Item::snoozed_until` (core/src/inbox/model.rs:41)                              │
                     │ ── magenta ─────────────────────────────────────────────────────── │                                                                                                      │
                     │   ◆ ☰ quality plan review                                      14m │ Worker's last lines                                                                                  │
                     │   ◆ ? tui-visuals asks you                                      3m │   → cargo doc --no-deps                                                                              │
                     │ ── white ───────────────────────────────────────────────────────── │   ← error: unresolved link ... (1 error)                                                             │
                     │   ○ ✉ cross-repo replied                                        2m │   → attempt submit                                                                                   │
                     │                                                                    │   ← refused: check_failed                                                                            │
                     │ ── hidden ──────────────────────────────────────────────────────── │                                                                                                      │
                     │   z infra#7 snoozed until 14:00                                    │ Lead's last message (2m ago)                                                                         │
                     │ ── resolved in the last hour ───────────────────────────────────── │   I can retry at S2 or S3, or edit the Spec so the doc link                                          │
                     │   ○ cpd#90 checks failed       fixed 12m ago (open 6m)             │   may be missing. Which do you prefer?                                                               │
                     │   ○ dev-speed asks you        answered 31m ago (open 2m)           │                                                                                                      │
                     │   ○ cpd#89 ready to merge     merged 48m ago                       │ What else this touches                                                                               │
                     │                                                                    │   PR core waits on it · 2 Jobs blocked behind it (core-tests, docs/guide)                            │
                     │                                                                    │                                                                                                      │
                     │                                                                    │                                                                                                      │
                     │                                                                    │                                                                                                      │
                     │                                                                    │ [ Retry at S2 ] [ Retry at S3 ] [ Edit Job… ]                                                        │
                     │                                                                    │ [ Open Lead ] [ Open Worker ] [ Snooze ▾ ] [ Dismiss ]                                               │
                     ├────────────────────────────────────────────────────────────────────┴──────────────────────────────────────────────────────────────────────────────────────────────────────┤
                     │ ↑↓ move   ↵ do the first button   n next   s snooze   d dismiss   u keep unread   ⌫ close and return to where I was                                                       │
                     │                                                                                                                                                                           │
                     ╰───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯
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
 ⧉ ? ▤ │ ●1 ✗ core-docs  ●2 ✗ cpd#92  ◐3 ↗ infra#7  ◐4 ✓ cpd#91  ◆5 ☰ quality  ◆6 ? tui-visuals  +1 │                                                                                                      workers 3/4
```

### A-05. A verb on the row: the primary action per kind, in one click

**Pain:** F, N.

- **Pattern.** cmux **Feed**: the things that need a human (a permission request, a plan to approve, a question) appear in the sidebar with the answer buttons inline (Once / Always / All tools / Bypass / Deny) and in the macOS notification as inline actions; the waiting agent hook is released by the click; if he does not answer in 120 s it falls back to the agent's own prompt ("advisory, not blocking") ([feed.md](https://github.com/manaflow-ai/cmux/blob/HEAD/docs/feed.md)). gh-dash: from a notification the PR actions are one key (`m` merge, `v` approve, `w` watch checks, `C` checkout) and `?` shows only those that apply ([keys](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/getting-started/keybindings/selected-notification.mdx)).
- **cpd mapping.** Today a row's click is "jump to the cause" and, for the five PR/review kinds, nothing (§2, G3, G4). Two steps:
  1. **One convention.** Follow the Deploy column (`main`, §13.2): a plain click on a PR-kind item opens the owning Plan's Lead (`session open` + focus chat; the item links the Plan when a Plan's PR points to the pull request, contract §2.2) and a small `↗` at the row's end carries the OSC 8 link. For a `plan-review` the `↗` opens plannotator as the Plan view's pending row does. Effort S.
  2. **A primary verb chip per kind** on the selected row and in the popup (A-04); the rest sit in a `▾`:

| Kind | Primary verb | Command it runs | Also |
|---|---|---|---|
| `dialog`, `unread` | Open | `session open` + focus chat (as today) | Snooze ▾ |
| `job-stuck` | Retry ▾ (tier menu) | `job retry <key> --tier <T>` (§5.4: the tier is required) | Open Worker, Edit Job… (`job edit`, §5.2) |
| `pr-failing`, `pr-changes` | Open Lead | `session open <Lead>` (the Lead already gets a `pr-feedback` Event, §9.2) | `↗` the page |
| `pr-ready`, `review-requested` | `↗` the page | link | Open Lead |
| `plan-review` | `↗` review | link to plannotator | Open Lead |
| `question` | Open Manager | `context manager`, `session open` | |
| `engine-error`, `engine-down` | Open log | logs popup at the Event | |

  A **Merge** verb for `pr-ready` would be the largest single saving (cmux and gh-dash both put it on the row) but `main` has **no command that merges a pull request** (commands.md §9 lists `pr open|poll|rebase|add|edit|cancel|start|view|push`), so it is a product decision and an engine change, not a view change: L, not built here. Chips are one-cell-tall buttons like the StatusBar's glyphs; the selected row shows them, other rows do not (width: 43 columns).
- **Effort.** S for the click convention; M for the chips, the Retry tier menu and Open Worker; L for Merge.

```
 ▌● ✗ inbox/core-docs failed     21m
 ▌  build: cargo doc exit 1 (Attempt 2)
 ▌  [retry ▾] [Worker] [snooze ▾] [✕]
```

### A-06. Mark the cause where it lives: Plan view, PR box, Deploy row

**Pain:** F, P. Completes "jump-to-cause": after the jump, the red thing is *visibly* red.

- **Pattern.** cmux: the pane that needs you "gets a blue ring and the tab lights up in the sidebar", and the sidebar shows linked PR status and the latest notification text ([README](https://github.com/manaflow-ai/cmux#why-cmux)). Glances colours the *stat itself* in its own panel and lists the alert separately ([aoa/index.rst](https://github.com/nicolargo/glances/blob/HEAD/docs/aoa/index.rst): "Stats with a colored background are the most important ones for a given plugin"). gh-dash puts CI and review status in one-icon columns on the PR row ([survey §3.2](survey.md)).
- **cpd mapping.** Every Inbox item links what it is about: `plan`, `job` (`job:<plan>/<key>`), `pr:<owner>/<repo>#<n>`, `session` (contract §2.2). The views that draw those things take the open items and mark them:
  - **Plan view:** a `job-stuck` item rings its pill (urgency colour, a `▌ ▐` pair or reversed) and prints one line under it: `└─ failed 21m · [retry ▾] [open Worker]` (the verbs of A-05, same commands). A pending `plan-review` is already marked (` Plan review r<n> pending ↗`, §13.2).
  - **PR box title:** an urgency glyph before the `#<n> <state>` when a `pr-*` item links that PR (today only `conflict` turns red).
  - **Deploy column:** the urgency dot at the start of the row's first line and the kind text on the second line (`ready to merge`, `checks failed`), where today only ` conflict` is flagged (§13.2). A `review-requested` PR (not owned; today not listed there, §13.2) could appear dim with its `↗`.
  - **Tree:** unchanged; its dots already come from the items.
- **Effort.** M. Each view needs the Inbox snapshot (the tree's read exists; the Plan and `widgets` snapshots would add it) and a style per mark; no new command. Depends on the Inbox.

Full mockup (215×62, today's columns; strip from A-01, Inbox section with detail and chips from A-03/A-05, ring and verbs on the pill from this idea, pane badge from A-08):

```
━[ Context ]━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╋━[ Plan ]━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╋━[ Deploy ]━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
                                           ┃                                                                                                                               ┃
 ▾ · building-cpd                          ┃ ┌ core · cpd → main · #92 open ✗ ↗ ─────────────────────────────────────────────────────────────────────────────────────────┐ ┃ homeserver  load 1.2 0.9 0.8
 ⏵ ├─● inbox              0⋅1⋅0⋅1⋅4        ┃ │  ✓ core-types │S1 ──── ▌✗ core-docs │S2│a2▐ ──── ○ core-tests │S1                                                         │ ┃
   ├─◆ quality            0⋅0⋅0⋅0⋅6        ┃ │                           └─ failed 21m · [retry ▾] [open Worker]                                                         │ ┃ ──[ PRs ]────────────────────────────────
   ├─◆ tui-visuals        1⋅2⋅0⋅0⋅1        ┃ └───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘ ┃
   ├─○ cross-repo         0⋅0⋅0⋅0⋅9        ┃                                                                                                                               ┃ ● cpd#92                              ↗
   └─⠹ dev-speed          1⋅3⋅0⋅0⋅2        ┃ ┌ docs · cpd → main · waiting ───────────────────────┐                                                                        ┃   open ✗ no review  core: docs…
 ▸ · homelab               2 plans         ┃ │  ○ guide │S1                                       │                                                                        ┃
 ▸ · personal              1 plan          ┃ └────────────────────────────────────────────────────┘                                                                        ┃ ◐ cpd#91                              ↗
 …                                         ┃                                                                                                                               ┃   open ✓ approved  ready to merge
                                           ┃                                                                                                                               ┃
                                           ╋━━[ inbox ]  ● ✗ core-docs failed 21m · [retry ▾] [open Worker]━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━╋   cpd#88                              ↗
                                           ┃                                                                                                                               ┃   open ⠹ no review  Quality adapter
                                           ┃  ● you   core-docs failed again. What happened?                                                                               ┃
                                           ┃                                                                                                                               ┃   infra#7                             ↗
                                           ┃  ● agent Attempt 2 of core-docs ended `failed`: `cargo doc --no-deps` exits 1,                                                ┃   open ✓ approved  (review for you)
                                           ┃          unresolved link `Item::snoozed_until` in core/src/inbox/model.rs.                                                    ┃
                                           ┃          I can retry at S2 (same tier) or S3, or edit the Spec so the doc link is                                             ┃
                                           ┃          allowed to be missing. Which do you prefer?                                                                          ┃ ──[ git ]────────────────────────────────
                                           ┃                                                                                                                               ┃
                                           ┃    → job show core-docs                                                                                                       ┃ (future)
                                           ┃    ← status failed · cause build · attempts 2/3                                                                               ┃
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
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
                                           ┃                                                                                                                               ┃
 ──[ Inbox 7 ]──────────────────────────── ┃                                                                                                                               ┃
 ▌● ✗ inbox/core-docs failed     21m       ┃                                                                                                                               ┃
 ▌  S2 a2 · exit 1: unresolved link        ┃                                                                                                                               ┃
 ▌  [retry ▾] [open] [snooze ▾] [✕]        ┃                                                                                                                               ┃
  ● ✗ cpd#92 checks failed         8m      ┃                                                                                                                               ┃
  ◐ ↗ review infra#7               1h      ┃                                                                                                                               ┃
  +4 more                                  ┃                                                                                                                               ┃
                                           ┃ ╭───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮ ┃
                                           ┃ │ retry at S3 please▌                                                                                                       │ ┃
                                           ┃ ╰───────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯ ┃
                                           ┃  claude-opus · inbox · ctx 31%                                                                                                ┃
 ⧉ ? ▤ │ ●1 ✗ core-docs  ●2 ✗ cpd#92  ◐3 ↗ infra#7  ◐4 ✓ cpd#91  ◆5 ☰ quality  ◆6 ? tui-visuals  +1 │                                                                                                      workers 3/4
```

### A-07. New versus seen: loud until he has looked, calm after

**Pain:** F.

- **Pattern.** ilmari: attention is "sticky only after a qualifying transition occurs while that exact pane is not focused by any tmux client. Focusing the pane acknowledges it", and an unchanged later scan does not recreate it ([README](https://github.com/bnomei/ilmari#published-fragment-and-counter-reference)). tmux-agent-tracker: a finished agent "stays orange until you have actually been to its pane — the one you are sitting in is checked off" ([README](https://github.com/ulas96/tmux-agent-tracker)). cmux: "Focusing a pane marks that pane's notifications read", plus "mark as unread" ([notifications.md](https://github.com/manaflow-ai/cmux/blob/HEAD/docs/notifications.md)). savras: the bright `●` "stays until you go to the session: opening it, or moving the cursor onto it, clears it, and so does the session no longer needing you" ([README, Ping](https://github.com/Marijusaj/savras)).
- **cpd mapping.** The engine already has this rule for one kind: `unread` is open while the Session is not in the chat slot and its last turn is newer than its last `session-moved` to the chat (contract §2.2). Generalise it to **display** (not to the items): an open item is **new** from `opened_at` until the Session or Plan Lead it concerns has had a `session-moved` with `place: chat` newer than `opened_at`; else **seen**. New rows are bold with a leading `▌`; seen rows are normal weight; the strip's count says `3 new`. It needs no write: it is derived from Events the views already read (the dot rule's inputs, §17), so it respects "views write nothing". Caveat: the Lead being in the chat slot is not proof he read the item (the `unread` rule has the same limit); a key to mark an item as new again is the cmux `mark unread` verb.
- **Effort.** S for Session-linked kinds (`dialog`, `unread`, `plan-review`, `job-stuck`: via the Plan's Lead); M for PR and engine kinds if the User wants "seen" to mean something other than "I opened the Lead". Depends on the Inbox.

```
 ──[ Inbox 4 · 2 new ]────────────────
 ▌● ✗ inbox/core-docs failed      21m    <- new: bold, marker
 ▌● ✗ cpd#92 checks failed         8m    <- new
  ◐ ✓ cpd#91 ready to merge       40m    <- seen: normal weight
  ◆ ☰ quality plan review         14m    <- seen
```

### A-08. The pane that has the need says so in its header

**Pain:** F, N.

- **Pattern.** tmux-agent-tracker puts a numbered badge on each tracked pane's border (`✳²ᶜ luima`) so "you can see which agent is which" ([README](https://github.com/ulas96/tmux-agent-tracker)). ilmari publishes pane-local options `@ilmari_state`, `@ilmari_badge`, `@ilmari_attention` for "advanced custom formats" ([README](https://github.com/bnomei/ilmari#published-fragment-and-counter-reference)). cmux rings the pane.
- **cpd mapping.** A Lead pane's header is `#[reverse] #{@cpd_title} #[noreverse]` (contract §6.1, one inverted line per pane). Add a conditional segment from a pane option, say `#{?@cpd_need, #{@cpd_need} ,}`, set to the item text (`● ✗ core-docs failed 21m`) for the pane of the Session the item links, cleared when it closes. The chat header then shows *this* Lead's open need and what it is, so the User who is already in the chat sees it without looking left. The option is tmux layout state (like `@cpd_title`, set by cpd commands, never by a view), not Ledger state; the writer is the step that opens and closes items or the command that moves the Lead (inferred: where `@cpd_title` is set today). Clicks on a pane border are not ranges in tmux (`range=` is for the status line only, tmux(1) 3.8), so the badge is a label, not a button.
- **Effort.** S for the format; M for the writer (every Inbox change refreshes the option, plus a rename of the pane on `session open`). Depends on the Inbox and on the dynamic-title work (hs-d257), which touches the same format.

```
 ━[ inbox ]━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━   today
 ━[ inbox ]  ● ✗ core-docs failed 21m  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━   one need
 ━[ inbox ]  ● ✗ core-docs failed 21m · ◆ ☰ plan review  ━━━━━━━━━━━━━━━━━━━━━━━━━━━   two
```

### A-09. Shape as well as colour: one glyph per kind

**Pain:** F, L.

- **Pattern.** agent-deck gives each state its own shape: `●` running, `◐` waiting, `○` idle, `✕` error ([Status Detection](https://github.com/asheshgoplani/agent-deck#status-detection)). ilmari has one `[states.*]` table with an icon and a colour per state (`▶` running, `●` waiting or finished, `✖` terminated, `?` unknown) and a *separate* attention icon, shared by the popup, the window badges and the status summary ([README, configuration](https://github.com/bnomei/ilmari#configuration)). gh-dash separates CI and review into two one-icon columns ([survey §3.2](survey.md)).
- **cpd mapping.** Today every need is the same `󰪥` and four colours carry two meanings at once: who it is for and how urgent (§17: red/yellow/magenta/white). On the tree's Session rows that is right ("this row has a need"). In the Inbox rows, the strip, the popup and the Plan view marks, add a **kind glyph** after the urgency dot, so the User reads *what he must do* before reading the label: `✗` failed (job-stuck, pr-failing), `✎` changes requested, `✓` ready to merge, `↗` a review to give, `☰` a Plan review, `?` a question or dialog, `✉` a reply, `⚠` the engine. One function in the palette (`kind_glyph(Kind)`), one more table in the legend popup, which is generated from the palette so there is one source. The exact Nerd Font glyphs are to be chosen from the [cheat sheet](https://www.nerdfonts.com/cheat-sheet) once the powerline Job (hs-d266) fixes the icon style; the mockups use plain Unicode stand-ins.
- **Effort.** S. A palette function, the legend lines, and the row builders.

| Kind | Urgency | Glyph (stand-in) | Reads as |
|---|---|---|---|
| `job-stuck` | red | `● ✗` | a Job failed |
| `pr-failing` | red | `● ✗` | checks red or a conflict |
| `pr-changes` | red | `● ✎` | changes requested |
| `pr-ready` | yellow | `◐ ✓` | ready to merge |
| `review-requested` | yellow | `◐ ↗` | a review is asked of you |
| `plan-review` | magenta | `◆ ☰` | a Plan to approve |
| `dialog`, `question` | magenta (red in `dispatch`/`pr`) | `◆ ?` | a decision |
| `unread` | white | `○ ✉` | a finished turn |
| `engine-error`, `engine-down` | red | `● ⚠` | the engine |

### A-10. The list holds still under the pointer

**Pain:** N, F. A safety rule more than a feature.

- **Pattern.** savras: "The order holds still. Rows are sorted by when each session was *started*, oldest at the top, never by how recently one did something." Freshest-first "made the panel unnavigable"; and **status-first, the order the Inbox has, "moved a row at the worst moment — the instant a session asked or stopped asking, while you were reaching for it"** ([README](https://github.com/Marijusaj/savras)). Its answer is a fixed order plus a mark: the session that just asked carries a bright `●` and the header counts it (A-07, A-11). bosun cycles sessions "in sidebar order, stable, not MRU-shuffled" ([survey §3.15](survey.md)). The survey's I-27 states the rule for the tree and Deploy column; this applies it to the Inbox.
- **cpd mapping.** The Inbox order is urgency, then oldest first, with `engine-down` always first (contract §2.2 "Order", hs-d167): exactly the status-first order savras gave up. A new red item lands on top and shifts every row down **while the User may be aiming at `s` or `d` on row 2**; `dismiss` is immediate and its effect lasts "for the rest of its life" (§21.4). Keep the User's order (it is a decision) and make it safe: (1) a click acts on the item id captured at the button-down, not on the row index at the button-up; (2) while the pointer or the cursor has moved in the last ~1 s, new arrivals are queued and inserted after (a constant, not a config key; **inferred**, the interval is a guess); (3) an arrival is announced where nothing moves, the strip's toast (A-11) and the `new` marker (A-07). A stricter option, for him to decide: order by `opened_at` only and let colour, not position, carry urgency (savras's choice). An **undo** line for `dismiss` (`dismissed core-docs · u undo`) needs a command: §21 has no `inbox undismiss`, so it is not S.
- **Effort.** S for rules 1–2 (view state only); M for undo (a command and an Event).

### A-11. An arrival toast on the status row: rare, batched, never when you are looking

**Pain:** F.

- **Pattern.** savras's **Ping** is the most careful design read: it fires "on the *transition*, not on a poll tick"; opening beside four waiting sessions is silent; "a burst of sessions asking at once is one ping that counts them, and twenty quiet seconds follow"; a session that starts asking inside the quiet window is *held*, announced when it closes, and "re-checked before it is said: a session that went back to working, or that you have since opened, is dropped rather than announced twenty seconds stale"; the focused pane is spared "but only while the terminal has focus" (DEC mode 1004) ([README, Ping](https://github.com/Marijusaj/savras)). cmux limits each remote origin to "a burst of 5 notifications refilling at 1 per second (identical text within 5 seconds is collapsed)" ([notifications.md](https://github.com/manaflow-ai/cmux/blob/HEAD/docs/notifications.md)). Textual's toast is `notify(message, title, severity, timeout)` with three severities ([`App.notify`](https://github.com/Textualize/textual/blob/HEAD/src/textual/app.py)). The survey's I-31 already has the unfocused bell/OSC path; this is the *in-cockpit* half.
- **cpd mapping.** Each cockpit pane is its own process, so an in-view toast only shows in that pane; the one cross-pane surface is the status row. When `inbox-opened` reaches the StatusBar view for a **red or magenta** item (not white `unread`), draw in the strip's gap for ~6 s: `▶ new ◆ ? tui-visuals asks you  [go]` (a click is A-02's jump); a burst of arrivals reads `▶ 3 new`; the same item never toasts twice (item ids are unique and an item that reopens is a new item, §2.2). With tmux's status line instead of a pane, `display-message -d <ms>` shows a message for a delay (tmux(1) 3.8); whether it shows with `status off` and whether it can carry a click range is **unverified**.
- **Effort.** S as a ratatui StatusBar row (the middle exists); M with burst batching and the focus rule. Depends on the Inbox and on A-01.

```
 ⧉ ? ▤ │ ●1 ✗ core-docs  ●2 ✗ cpd#92  ◐3 ↗ infra#7  │ ▶ new: ◆ ? tui-visuals asks you · 2 more  [go] │            workers 3/4
```

### A-12. A "recently resolved" tail: watch the pile shrink

**Pain:** F, P.

- **Pattern.** Glances' alert block keeps ended alerts: `●` ongoing, `○` resolved, with start time and duration, ongoing first, and the title counts both ([events.rst](https://github.com/nicolargo/glances/blob/HEAD/docs/aoa/events.rst)). gh-dash keeps `is:done` as a filter ([notification-section](https://github.com/dlvhdr/gh-dash/blob/HEAD/docs/src/content/docs/configuration/notification-section.mdx)).
- **cpd mapping.** An Inbox item closes by itself when its cause ends, and a closed item is kept forever (contract §2.2, hs-d168) but never shown. The User who fixes a Job cannot tell "I fixed it" from "it went away" from "it flapped". In the triage popup (A-04) and as a fold `+3 resolved` under the tree section, list the closed items of the last hour, dim, with how long each was open: `○ cpd#90 checks failed · open 6m · closed 12m ago`. `inbox list --closed --limit <n>` already returns them (§21.1) with `opened_at` and `closed_at`. It does not say *why* an item closed (no field for the closing cause in §2.2): "how it ended" would need the closing Event.
- **Effort.** S. One read already in the CLI; one dim list.

### A-13. Counts on collapsed rows

**Pain:** F.

- **Pattern.** WeeChat's hotlist carries counts per buffer: `3:#abc(2,5)` is buffer 3 with 2 highlights and 5 unread messages ([user guide](https://github.com/weechat/weechat/blob/HEAD/doc/en/weechat_user.en.adoc), `hotlist` item). savras's header counts `1 needs input · 2 working · 4 done`. bosun puts "a small accent dot" on a collapsed row when anything inside is busy ([survey I-29](survey.md)).
- **cpd mapping.** In the in-flight dot rule a Context row's dot is the Manager's own state only, a decision of the User (hs-d215, hs-d217): the Context's other items "do not colour the Context row". So folding a Context hides its Plans' needs from the tree. Do not change the dot: while a Context is **collapsed**, show a count in the worst urgency colour where the Plan count is today (`▸ · homelab  ● 2`); expanded, show nothing extra. Its Plans' rows already carry their own dots. This does touch hs-d217's intent, so it is a question for him before a build.
- **Effort.** S. The tree already builds the collapsed row; the counts come from the same items.

### A-14. A hold time against flapping items

**Pain:** F. Engine side; the Inbox owner's call.

- **Pattern.** Glances' alert engine has a hold time "before a level change is committed and logged (avoid flapping alerts)", `min_duration_seconds=5.0`, and a lower level (`CAREFUL`) that colours the stat but "never creates an event" ([events.rst](https://github.com/nicolargo/glances/blob/HEAD/docs/aoa/events.rst), [index.rst](https://github.com/nicolargo/glances/blob/HEAD/docs/aoa/index.rst)).
- **cpd mapping.** E8 opens and closes items from the cause (contract §3.7). A `pr-failing` item follows `checks` (`failed` → `pending` on a rerun → `failed`), and `pr-ready` follows `passed`/`pending`; each flip opens or closes an item, appends an `inbox-*` Event and re-ranks the strip. A hold per kind (guess: 60 s for the `pr-*` kinds, none for `dialog` and `engine-*`), as a constant like Pulse's 30 minutes, would keep them steady. **That flapping happens is unverified**: no data was read; check the `inbox-opened`/`inbox-closed` Events of a real week before building.
- **Effort.** S to M. A condition in E8; needs the time the cause started.

## 4. All ideas in one table

Areas: **Bar** (StatusBar / tmux status line), **Tree** (Context column and its Inbox section), **Pop** (popups), **Plan**, **Deploy**, **tmux** (pane headers, options), **Engine**.

| Rank | Id | Idea | Area | Pain | Effort | Source | Depends on | Survey link |
|---|---|---|---|---|---|---|---|---|
| 1 | A-01 | Numbered needs-you strip, click or key | Bar | F, N | S (pane) / M (status line) | tmux-agent-tracker, agent-deck, WeeChat | Inbox | I-23 |
| 2 | A-02 | Next-needs-you button and Back | Bar | F, N | S | WeeChat, tmux-agent-tracker, cmux, savras | Inbox, A-01 | |
| 3 | A-03 | One-line cause on every item | Tree, Pop | F, P | S / M | savras, cmux, Glances | Inbox (new field for dialog/unread) | |
| 4 | A-04 | Triage popup with cause preview | Pop | F, N | M | gh-dash, ilmari, tmux-agent-status, cmux | Inbox | I-14, I-21 |
| 5 | A-05 | Verb on the row; one click convention for PR kinds | Tree, Pop | F, N | S / M / L (merge) | cmux Feed, gh-dash | Inbox; a merge command does not exist | I-33 |
| 6 | A-06 | Mark the cause in Plan view, PR box, Deploy | Plan, Deploy | F, P | M | cmux, Glances, gh-dash | Inbox | I-04 |
| 7 | A-07 | New vs seen | all | F | S / M | ilmari, tmux-agent-tracker, cmux, savras | Inbox | |
| 8 | A-08 | Need badge in the pane header | tmux | F, N | S / M | tmux-agent-tracker, ilmari, cmux | Inbox, hs-d257 | |
| 9 | A-09 | Kind glyph beside the urgency dot | all | F, L | S | agent-deck, ilmari, gh-dash | hs-d266 icon style | |
| 10 | A-10 | The Inbox holds still under the pointer | Tree | N, F | S / M (undo) | savras, bosun | Inbox; undo needs a command | I-27 |
| 11 | A-11 | Arrival toast on the status row | Bar | F | S / M | savras, cmux, Textual | A-01 | I-31 |
| 12 | A-12 | Recently resolved tail | Pop, Tree | F, P | S | Glances, gh-dash | Inbox | |
| 13 | A-13 | Counts on collapsed Contexts | Tree | F | S | WeeChat, savras, bosun | User on hs-d217 | I-29 |
| 14 | A-14 | Hold time against flapping | Engine | F | S / M | Glances | Inbox; unverified need | |

## 5. Where this points (for the Plan's next Job to weigh, not a decision)

- **The first build is A-01 + A-02 + the click convention of A-05.** Together: a numbered strip, a Next button and a plain click on the PR kinds. They are S (pane form) and need only the Inbox read model the views already take. They turn "scan the tree, find the dot, open it" into "click the red `1`", and "handle, then `▶`" into the loop. They also remove G2 and G3.
- **Second: A-03 and A-04.** The cause text makes ranking possible without opening; the popup holds the pile. A-03's `dialog`/`unread` excerpt is the only item that needs an engine field (M); the rest of A-03 and all of A-04 are views.
- **Third: A-06, A-07, A-08, A-09.** The look-and-where layer: mark the cause in place, new versus seen, the pane header, kind shapes. A-09 should wait for the powerline Job's icon choices; A-08 for the dynamic-title work on the same format.
- **Rules to write into the Inbox spec rather than build as features:** A-10 (the list holds still; actions use the id), A-12's "kept, not shown" decision, A-14's hold (after the data check).
- **Not covered here, already in the survey:** push to the phone on a new need (I-25), the OSC and bell path when unfocused (I-31), the filter chips and the `needs me` scope (I-16, I-24), the `:` palette (I-06).
- **Questions for the User, not assumptions:**
  1. Should a plain click on a PR-kind item open the Lead (like the Deploy column) and the `↗` carry the link? Today it needs a modifier-click.
  2. Is a Merge button on a ready PR wanted? It needs a new command (not in §9).
  3. May a collapsed Context show a count (A-13), given hs-d217?
  4. Status line or pane for the StatusBar? Hover and a spinner need the pane; click ranges and one less process need the status line (A-01).
  5. Does `M-<key>` reach tmux from Ghostty over ssh for him? Mouse is the primary either way.

## 6. What is not verified

- Every tmux feature (`range=user|X`, `status-format[]`, `MouseDown1Status`, `display-message -d`, `display-menu` colours) was read from the tmux 3.8 man page on this machine; the cockpit's minimum is 3.4. Not tried on either.
- Whether the Inbox section, the `inbox <n>` counter and the click table are final: they are on `origin/cpd/inbox/cockpit`, not `main`.
- Nothing was run in a terminal: no mockup was tried, no key was tested in Ghostty, no plugin was installed. All tool behaviour is from READMEs and docs, quoted where it matters; layouts are **inferred** from them.
- That flapping items exist (A-14), that `dialog`/`unread` excerpts are too slow to read per render (A-03), that the 1-second hold in A-10 is right, and that the User wants a Merge verb or kind glyphs: guesses, labelled where they are made.
- cmux is a macOS GUI app, not a TUI; its patterns (rings, panel, Feed) are used as design sources only, and the shortcuts quoted are its own.
