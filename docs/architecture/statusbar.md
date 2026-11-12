# Status bar

The WebUI's Bottom Status Bar. Its segments are drawn in the browser from structured values. The browser never reads a display string of the engine, and no segment draws a private-use character. The full rules are in [web/CONTRACT.md](../../web/CONTRACT.md) §1.2 (pro-d18), §6.7 and §21; §20 is the serve status pane they read.

## Workers segment

The segment (`<span data-part="workers">`) is drawn from the four structured counts of the serve status pane (CONTRACT §20):

| Count | Pane field | Meaning |
|---|---|---|
| Q | `workers.queued` | every Job with status `queued` |
| W | `workers.waiting` | the Ready Jobs |
| N | `workers.running` | the tenant Jobs of pool `workers` |
| M | `workers.limit` | config `concurrency` |

The visible text is the label `workers `, then Q, the separator `·` (U+00B7 MIDDLE DOT), W, `·`, N, then `/` and M. For queued 5, waiting 0, running 1 and limit 5 it reads `workers 5·0·1/5`. The `·` is the separator of the TUI display string `workers Q·W·N/M` (CONTRACT §19.2, §20).

The pill carries a `title` tooltip that names each count in words, for the same example: `queued 5, waiting 0, running 1, limit 5`. Icons are the WebUI's to choose and are not fixed by this page.

Rules:

- The segment uses only ASCII characters and U+00B7. It never uses a private-use character (the Unicode Private Use Area, U+E000 to U+F8FF and the supplementary planes), because a browser without a Nerd Font draws such a character as a tofu box.
- No pluralisation, no padding, and no per-state colour. The pill keeps the `text-fg-dim` look.
- The web never reads the TUI's glyph-separated string nor the tmux option `@cpd_workers_short`, and it never draws the TUI's narrow-client short form.
- The counts reach the bar with the next serve snapshot, not on a timer (CONTRACT §21).

## Incident pill

The incident pill (`data-part="incident"`) reads `Incident Mode: <elapsed>`, where the elapsed time is the whole-minute time since the pane's `incident.started_at`. It is absent when the pane's `incident` is null.

## Clicks

Only two pills are clickable:

- **Workers segment:** opens the Settings dialog (Agent Tiers), exactly as the Settings button does (CONTRACT §21).
- **Incident pill:** when `incident.lead` is a session id, attaches that Lead in the chat slot. When `lead` is null the pill draws but a click does nothing.

Runner, preview, drift and version are not clickable.

## Why no private-use characters

Bug b44: the browser has no Nerd Font, so the TUI's separator U+F0798 in the engine's workers string drew as a tofu box. The WebUI therefore draws the segment from the four numbers above and never the engine's display string. The TUI is unchanged and keeps its own glyph.

## Implementation status

This page and CONTRACT §21 state the target. `web/src/ui/cockpit/StatusBar.tsx` still builds the segment with U+F0798 between the numbers and has no `title` tooltip; a follow-up code Job must change both. This Job changes no product code.
