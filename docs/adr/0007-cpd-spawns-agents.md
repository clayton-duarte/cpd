---
status: accepted
---

# `cpd` starts Workers and Reviewers itself through herdr

`cpd` launches every Worker and Reviewer with `herdr agent start --kind <agent> -- <args>` (model and effort, tool allowlist, `--fork` for the Reviewer) instead of using a pi subagent package. Stage 1 trial t1 showed both trialled packages (`pi-herdr-agents`, `modem-dev/pi-herdr-subagents`) work, but they make the Lead's LLM the spawn path (a tool call per spawn, parameter drift, results landing only in the Lead's context) and are pi-only. Launching from `cpd` keeps scheduling deterministic, writes the Ledger directly and works for any agent kind herdr supports.

## Consequences

- `cpd` must detect Worker death itself (herdr `released:true` event, session stop reason), which the packages did natively.
- herdr's fixed ~3 s readiness delay per launch is accepted at Job granularity.
- Each Attempt needs a unique herdr agent name.
