---
status: accepted
amends: ADR-0007
---

# The cockpit replaces herdr as the User's interface

The User works in the cockpit: one tmux window (`tmux -L cpd`, session `cpd`) where tmux hosts the panes and ratatui draws the views. The window is split 20% / 60% / 20%. The left and right bars are global (the left is a tree of Contexts and Plans; the right is future work: PRs, git, Jira, view only or actions by picking an option). The centre is bound to one Plan: the plan view on top, the chat with the Plan's Lead below. The Lead is the only session the User types into. Clicking a Job opens its Worker's chat read-only beside the Lead's chat. The cockpit lists the Lead sessions actually running in it (forks included), labelled by their pi session names.

The User's verdict on the phase-0 spike: "it already has a better experience than herdr". tmux was chosen over zellij (WASM plugins give global bars natively, but need a Rust toolchain for plugins, and zellij's keybindings can clash with pi); zellij is the second option. herdr is dropped. The User's decision (2026-10-14): "we moved to the cpd-tui already, its commands should not be herdr dependent."

ADR-0007 says `cpd` starts Workers and Reviewers through herdr. This ADR amends it: that remains how it works until Workers move, but herdr is no longer the interface and `cpd`'s commands must stop depending on it.

## Consequences

- `cpd`'s commands do not depend on herdr. Lead attach works from tmux; Reviewers and the watcher no longer open off the Lead's herdr pane.
- Today the cockpit fakes herdr: it starts each Lead with `HERDR_ENV=1` only to switch on cpd's pi adapter, and keeps each Lead's old herdr pane open. Removing this is later work.
- Workers stay in herdr for now. They move later into a hidden tmux session; the read-only view then draws their session file. Until then the herdr path must keep working exactly as it does today (a Lead whose `workflow.json` has no `host` is a herdr Lead).
- Agent states come from a pi extension, which knows them exactly from pi's events (`agent_start`, `agent_settled`, blocked on `ask`), not from reading the screen. Today `tmux-agent-state.ts` writes `@agent_state` and `@agent_label` into the pane.
- Renaming a session is a CLI command (User: "cpd cli should provide an interface for that"): `cpd session rename`, applied by cpd's pi adapter through `pi.setSessionName`.
- `cpd` stays a CLI first; the cockpit is a view onto its commands. Live state needs a fast read path that takes no lock: `cpd workflow show` took 58 s under load in the spike. The mechanism is not decided (for example a snapshot written by `cpd watch`, an event stream, or `bd-serve`).
- pi under tmux 3.4 worked flawlessly for the User in the spike.
- Porting `src/herdr/*` to tmux is required for Workers and Reviewers.
- What happens to lead-fork's herdr-tab fork command is not decided; the sources say it is likely replaced by "a new Plan forks the Context's Lead" (ADR-0011).
