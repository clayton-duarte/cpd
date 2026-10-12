---
status: accepted
---

# The Lead is woken through a file queue and a thin pi extension

`cpd` reports to the Lead by atomically writing messages (completions, failures, Escalations) into a per-Lead queue directory. A small Lead-side pi extension watches it and delivers each message with `pi.sendMessage`, which never touches the User's half-typed input. Workers escalate by running `cpd ask`, which blocks until `cpd answer` writes the reply. Trial t5 chose this over pi-intercom because the queue survives a Lead restart (pending questions re-surface at session start) and the Worker side is agent-agnostic. pi-intercom lost pending questions when the Lead restarted and needs a broker process.

## Consequences

- Only the Lead adapter is pi-specific; Claude Code or Hermes Leads need their own equivalent of the extension.
- Questions from crashed Workers must expire, or they re-surface on every Lead restart.
