# W2: pi ↔ herdr subagent packages: research report

Read at: 2026-10-12. Items reviewed: 17 (all clustered, shallow clones at the SHAs below, source read for the claims that matter) + 10 additional finds read to some depth (0xRichardH, mu, shepherdr, cowboy, itc-steve, agent-fleet, squad, pinata, asermax, zzjcool) + ~20 more only listed. Items not reviewed: none from the list. Nothing was installed or run; every "works" below is **unverified at runtime** (read from code/docs only).

Shared facts verified once in pi 1.1.0 (`~/.pi/agent/install/releases/1.1.0/node_modules/@earendil-works/pi-coding-agent`), used throughout:
- **N2 baseline.** `pi.sendMessage(...)` from an extension (`agent-session.js:1782-1820`) never touches the editor: while streaming it `steer()`/`followUp()`s, when idle with `triggerTurn:true` it starts a turn, `deliverAs:"nextTurn"` queues for the next user prompt. The interactive renderer only appends a chat component on a custom message (`interactive-mode.js:2825-2829`); `editor.setText("")` only occurs in submit paths. So **every package that delivers results via `pi.sendMessage` is draft-safe**. The hazard is any path that goes through `herdr agent prompt` / `pane run` (it types into the target TUI). Flagged per item.
- Legacy `@mariozechner/*` imports are aliased by pi 1.1.0's loader (`dist/core/extensions/virtual-modules.js:30-36`, `loader.js:70-72`), so packages still peering on `@mariozechner/*` should load; `@sinclair/typebox` root is also aliased (CHANGELOG). Unverified at runtime.
- `--tools read,grep,find,ls` replaces the whole tool selection (`docs/cli.md:119-128`), i.e. a real allowlist, but `bash` in an allowlist is **not** read-only.
- Pi session JSONL carries per-message `usage` incl. cost (`docs/session-format.md:45,91,113-121`). Any package that retains/reports the child session path therefore gives N13 turns/tokens/cost by reading that file; few expose it directly.
- herdr 0.9.3 CLI has `agent start <name> --kind KIND --pane ID [-- args]` (kinds include `pi`, `claude`, `codex`, `hermes`, `opencode`), `agent prompt/wait`, `plugin pane open … --target-pane --direction --cwd --env --no-focus`, `worktree create --branch --base` (branch created from `--base` or `HEAD` of the repo) (`cli-reference.mdx:182,343-359,518`).

## Summary

### Per-need best candidate

| Need | Best item | Coverage | Notes |
|---|---|---|---|
| N1 spawn + per-spawn model/thinking | `edxeth/pi-subagents` (`model`+`thinking` params, validated vs the model registry, `child-launch-plan.ts:105-157`); `giuseppecrj/pi-herdr-agents` (model+thinking+cwd+fork+`worktree{branch,base}` in one call); `AndrewJacop/pi-herdr` (same, plus `agent start` launch) | full (all three) | edxeth has **no per-spawn cwd** (`forcedCwd` is stripped from the model-callable schema, `types.ts:40-50`). modem-dev/HazAT/andrewfung729 take `model` only (use `provider/id:level` suffix). |
| N2 non-clobbering wake | All `sendMessage`-based packages for results. Best nuance: `fyang93/pi-herdr-swarm` (delivers only when idle / at a turn boundary), `AndrewJacop/pi-herdr` (`notifications: normal\|quiet\|none`), `howaboua/pi-shepherdr` (peer messages bypass the receiver's editor) | full (results) | **Upward escalation by terminal typing clobbers the Lead's draft**: AndrewJacop `herdr_message_agent`→`orchestrator` (`tools/orchestration.ts:233`), swarm `swarm_send` (`herdr agent prompt`). |
| N3 escalation (ask + wait) | `boadij/pi-herdsman` `ask_owner`/`reply_agent` (durable, correlated, same assignment resumes); `andrewfung729` `ask_question` (child stays open, reply arrives as next turn) | full | `caller_ping` (modem-dev, HazAT, edxeth, giuseppecrj) is **exit + `subagent_resume`** (new pane, same session): partial. giuseppecrj rejects resume for worktree children (`docs/tools.md:233-250`). |
| N4 Reviewer = read-only fork | modem-dev (`fork:true` param + `--tools` allowlist, `src/launch.ts:126-148,392`); andrewfung729 (`--no-extensions` + `--tools`, whitelist-only, fork via agent def); edxeth (`session-mode: fork` + `tools`/`deny-tools`) | full | Must write the reviewer def with `tools: read,grep,find,ls`; every bundled reviewer ships `read,bash` (not read-only). Fork copies the whole Lead transcript (edxeth adds a boundary marker; AndrewJacop truncates before the last user message). herdsman has **no fork**. |
| N6 concurrency cap | `AndrewJacop/pi-herdr` (`max_parallel_agents` default 3, over-cap = queued, `spawn.ts:273-276`); `pi-cowboy` (global 4 + per-model/provider/harness, queued) | full (per Lead session, in-process) | edxeth `spawn-width` rejects instead of queueing (`subagent-tools.ts:449`). None is a cross-process per-machine cap (in-memory per Lead; unverified for cowboy). |
| N7 timeout per Attempt | `edxeth` (`timeout`, `idle-timeout`, soft wrap-up, kills pane, resumable; README:187-194,275-390) | partial | Set in **agent definition**, not per spawn. HenryQW idle 10 min/hard 30 min only for direct/ephemeral children; itc-steve stall timeout; kirel `completionTimeoutMs` only stops watching. Everyone else: none. |
| N8 retry ladder | none implements it | n/a | Lead logic. Needs only N1 (per-spawn model/thinking) + a fresh session (default `standalone`/`lineage-only` everywhere). giuseppecrj model-fallback list retries on launch/provider errors only, never on a rejected result. |
| N9 worktree from Lead Branch HEAD + setup | `giuseppecrj` (`worktree:{branch,base}`, `base` resolved to an exact commit, default committed HEAD; `herdr worktree create`); `asermax/herdr-subagents` (`worktree/branch/base`); `pi-cowboy` (own git worktree, CoW-seeds ignored files such as `node_modules`/`.env`) | partial (setup only in cowboy; pinata detects `npm ci` but is not herdr-native) | AndrewJacop `isolated:true` has no `base` (`tools/worktrees.ts:99-113`; auto branch+path from project cwd). |
| N10 merge keep commits, serialised | `pi-cowboy` `merge_cowboy_branch` (plain `git merge` into target in main checkout; conflicts stay) ; `HenryQW/pi-subagent` (stage→validate→promote) | partial | No real merge queue anywhere. HenryQW is a whole parallel Dispatch system with `pnpm test` hard-wired (`docs/orchestration.md`). |
| N11 Gate | HenryQW (`checks`, `finalChecks`); AndrewJacop workflows (`gate` shell cmd) | partial | HenryQW requires a root `package.json` test script. |
| N12 PR feedback loop | `joelhooks/pi-bellwether` `herdr_watch` (`pane_output` watch on a `gh pr checks --watch` pane, wake the Lead with the matched line; `src/wake.ts:126-128`); `tobi/pi-herdr-workers` `/loop` timers | partial | Neither parses bot comments. tobi Monitors are pull-only (`MonitorList`), not a wake. |
| N13 metrics | HenryQW (result lines: model, thinking, tokens, elapsed), AndrewJacop workflows (`budget.spent()` from session JSONL), herdsman (`tokens` in `list_agents`) | partial | Others: derive from session JSONL path (shared fact above). No package records turns. |
| N14 | out of cluster | n/a | HenryQW `@henryqw/pi-ask-question` (2.5k dl/mo), `@alexshpunt/pi-ask`: not reviewed. |
| N15 | herdsman (role-named tabs, pane metadata), bellwether (herdr sidebar `$wait`/`$needs` tokens), giuseppecrj sidebar plugin, `mu` dashboard | partial | All have an above-editor widget. |
| N16 | none | n/a | andrewfung729 is the only whitelist-only (`--no-extensions` + `--tools`); HenryQW/edxeth/giuseppecrj say explicitly "not a sandbox". |
| N17 agent-agnostic | AndrewJacop (`kind`: pi/claude/codex/opencode via `agent start --kind`); `asermax/herdr-subagents` (one source → pi package + Claude Code plugin); `pi-cowboy` (`harness_type` pi/opencode/codex/claude); HazAT (`cli: claude`) | partial | herdr 0.9.3 itself lists kind `hermes`. Completion for non-pi kinds relies on herdr state detection. |
| N18 cheap aux (Haiku) | any package with a per-spawn model | full | `H0–H3` = `provider/claude-haiku-4.5:off..high`. |
| N19 building blocks | kirel (307 LOC reference), modem-dev (argv launcher + socket event watcher + test harness), bellwether (typed herdr socket client, wake coalescer), edxeth (`child-launch-plan.ts` thinking validation, `timeout-budget.ts`), giuseppecrj (`maestro/surfaces/herdr`, worktree ownership manifests) | n/a | See items. |

### Capability matrix (what each package actually does)

| Package | Launch (argv vs typed) | Completion detection | Delivery to parent | Escalation | Per-spawn model / thinking | cwd / worktree | Cap | Timeout |
|---|---|---|---|---|---|---|---|---|
| modem-dev | **argv** via herdr plugin pane (`dispatch.sh` execs generated script) | herdr socket `pane.exited/closed` + `.exit` sidecar | steer + triggerTurn, one per child | `caller_ping` (exit; `subagent_resume`) | `model` (use `:level`); no `thinking` param | `cwd` yes; no worktree | none | none |
| andrewfung729 | typed (`pane split` + `pane run` script) | poll `.exit`/activity sidecars, 60 s fatal-stall | **one wake per autonomous batch**; others display-only | `ask_question` (waits) | `model` (scope-validated); `thinking` from def | `cwd`; no worktree | none | 60 s startup stall |
| HazAT | typed, 4 muxes (no herdr; PR #57 open) | screen/sentinel poll | steer + triggerTurn | `caller_ping` | `model`; thinking from def | `cwd` | none | none |
| herdsman | **`herdr agent start --kind pi`** | herdr + mailbox | steer+trigger (`index.ts:847-854`), idle-gated health attention | `ask_owner` (waits) | definition only (no per-spawn) | owner's cwd; branch worktrees only for Manager→Lead | none | stale-attention only |
| edxeth | typed (`pane split` + `pane run`) | sidecar poll | steer+trigger (or nextTurn after launch batch) | `caller_ping` | `model`+`thinking` validated | agent-file `cwd` only | `spawn-width` (reject, ≤16) | `timeout`/`idle-timeout` (per def) |
| kirel | typed (`pane run`) | poll exit file 1 s | steer/followUp+trigger | none | `model` (`:level`) | `cwd` | none | notify-only |
| tobi | `herdr agent start --kind pi` | worker self-report (LLM calls `SendToAgent`) | per-pane inbox → steer/followUp | `SendToAgent` to orchestrator | `model`+`thinking` | orchestrator cwd | none | none |
| giacolees | typed (vendored 0xRichardH) | sidecar poll | steer+trigger | `caller_ping` | `model`/`thinking`, config routing | `cwd` | none | none |
| bellwether | socket `agent.start` (args) | socket `agent.wait` + 5 s probe | **held to `agent_end`, merged, followUp+trigger** | none | via `agentArgs` | pane-level | none | `timeoutSeconds` per watch |
| giuseppecrj | typed, waits for shell via `pane process-info` | `.exit` fs-watch + process identity | steer+trigger | `caller_ping` | `model`+`thinking`+fallback list | `cwd`, `worktree{branch,base}` | persistent only (3) | none (advisory) |
| AndrewJacop | **`herdr agent start --kind`** | sidecar → JSONL → gone grace | steer (`normal`) / nextTurn (`quiet`) | `ask_user`-blocked wakes; message via `agent prompt` | `model`+`thinking` validated | `cwd`, `isolated` (no base) | **`max_parallel_agents`=3, queued** | none |
| ribbons pi-herd | CLI/plugin, artifacts | poll verdict markers | none (CLI) | `blocked` verdict | per-role config | worktrees per role | – | `wait --timeout-ms` |
| fyang93 swarm | `herdr agent start --kind pi` | poll session binding 1 s | **idle or next turn boundary** | `swarm_send wait:true` | `model` (thinking via preset) | `cwd` | `maxAgents`=16 precheck | none |
| entwurf | herdr tab (pi/claude) | none (siblings) | control socket | `entwurf_v2` | `model` | `cwd` | none | none |
| HenryQW | `herdr worktree open` + `tab create` + `agent start/prompt` | `agent wait` + session proof | followUp / starts turn when idle | "needs attention" to Main | `modelClass` fast/balanced/frontier/fav | direct: Main cwd; isolated worktrees | `maxSubagents`=5 | idle 10 / hard 30 min (direct) |
| pi-harness-runtime | n/a (not herdr) | – | – | – | – | – | – | – |
| vishsant | argv (modem-dev fork) | as modem-dev | as modem-dev | `caller_ping` | + remote "Jev" router | `cwd` | none | none |

### Overlaps and conflicts (could two coexist?)

- **Same tool names → silent first-loaded-wins** (modem-dev states this, README "Transition"): `subagent`, `subagent_resume`, `subagent_interrupt`, `subagents_list`, `subagent_done`, `caller_ping` are registered by modem-dev, HazAT, 0xRichardH, giacolees (vendored), vishsant, giuseppecrj (+`subagent_cancel/send/stop`), edxeth (+`subagent_kill`, no `subagents_list`/`interrupt`), andrewfung729 (`subagent`, `subagent_message`, `subagents_list`, `ask_question`; no resume tool), asermax (single `subagent` tool with `command`), HenryQW (`subagent_resume`, `subagent_status`, …). **Pick exactly one of this family.** modem-dev only registers real tools inside herdr and warns when it loses the race (`index.ts:1179-1200`).
- Distinct namespaces that coexist with any of the above: herdsman (`delegate_agent`, `continue_agent`, `ask_owner`, …), swarm (`swarm_*`), cowboy (`cowboy_agent` …), kirel (`herdr_subagent`), tobi (`CreateAgentPanel`, `SendToAgent`, `Loop*`, `Monitor*`), bellwether (`herdr_layout/pane/agent/watch/ping_wait`), AndrewJacop (`herdr_spawn_agent`, …), itc-steve (`herd`, `herdr`), shepherdr (`agents`), mu (`mu_delegate`), `@ogulcancelik/pi-herdr` (`herdr`).
- bellwether and AndrewJacop both claim "replaces pi-herdr" generic control; bellwether `herdr_agent` vs AndrewJacop `herdr_*_agent` names differ but both push agent state to herdr (`pane report-agent` / sidebar tokens) next to herdr's own pi integration: possible double-reporting (unverified).
- giacolees vendors a copy of 0xRichardH's extension: loading it with another `subagent` package duplicates tools.
- edxeth's "orchestrator mode" removes every tool except `subagent*`: this is the closest thing to "Lead never writes code", but it also removes `bash` (the Lead could not run `bd`/`gh`/herdr CLI) → conflicts with a CPD Lead that must run those.

### Corrections to `docs/requirements.md` §5

1. `andrewfung729/pi-herdr-subagents` is **not a fork of modem-dev**. It descends from `amosblomqvist/pi-interactive-subagents` (tmux fork of HazAT) and launches by **typing** (`herdr pane split` then `herdr pane run`), polling sidecar files; no socket events, no argv launch (`pi-extension/subagents/herdr.ts:244-294`, `index.ts:1539`). "Argv launch, socket lifecycle events" apply to modem-dev only.
2. npm `pi-herdr-subagents` (702 dl/mo) is **0xRichardH's** package, not modem-dev's. modem-dev is git/local install only (no npm, no GitHub releases, no CI, **no LICENSE file**: package.json says MIT, GitHub API says none).
3. modem-dev has no `thinking` param on `subagent`; thinking comes from agent frontmatter. Per-spawn `model: "provider/id:level"` works, but if the agent def also sets `thinking` the code concatenates `model:thinking` (`launch.ts:375`) and a model string already carrying `:level` would double up (guess: invalid).
4. andrewfung729's "one wake per batch" is a **downside for CPD**: the parent turn is triggered only when no autonomous child is still running (`index.ts:710-717`), so with a continuously refilled worker pool the Lead is not woken per completion.

### Recommendation for CPD v0

1. **Do not write a spawner.** Choose from the `subagent` family or a distinct-namespace package; install only one.
2. **Most defensible stack:** `giuseppecrj/pi-herdr-agents` for spawn (+ worktree). One call gives per-spawn `model`+`thinking`, `cwd`, `fork`, `tools`, and `worktree:{branch, base}` via `herdr worktree create --base` (`maestro/surfaces/herdr/herdr.ts:238-257`); MIT, CI, 38 test files, releases, 5 contributors, pi 1.1.0 in devDeps, 5.8k dl/mo. Costs: typed launch (mitigated: waits for shell via `pane process-info`, `herdr.ts:1187-1216`), no cap, no timeout, `subagent_resume` rejects worktree children (escalation must be answered in the retained workspace), 63k LOC to own.
3. **Alternative if launch reliability matters more** (direnv/devenv repos): `modem-dev/pi-herdr-subagents` (argv plugin pane, socket-event lifecycle, honest crash steers, 18 test files, 11k LOC) + herdr's own `worktree create --base` done by the Lead, spawning into it with the `cwd` param. Costs: no worktree/cap/timeout, no LICENSE file, stale issues (#2, #3), git-only install; fork/pin it.
4. **Escalation (N3) is the weakest link everywhere** except herdsman `ask_owner` and andrewfung729 `ask_question`. Cheapest fix: borrow andrewfung729's `ask_question` pattern (sidecar `.ask` → `sendMessage`, child stays open, reply typed into the child pane, `subagent-done.ts:333-399`) as a small child extension; never route child→Lead messages through `herdr agent prompt` (clobbers the Lead's draft).
5. **Cap (N6), timeout (N7), retry ladder (N8), merge queue (N10), Gate (N11), metrics (N13) stay in the `cpd-lead` skill + a tiny script**; nothing reviewed provides them in a herdr-native, Job-oriented form. Borrow edxeth's thinking validation (`child-launch-plan.ts:105-157`) idea (or read pi's catalog directly) to reject invalid Tiers, and its `timeout`/`idle-timeout` design.
6. **Closest whole-system alternatives, not drop-ins:** `pi-herdsman` (Manager→Lead→Agent tree, durable mailbox, `ask_owner`, exactly pi 1.1.0 + herdr 0.9.3 tested; but no per-spawn model/thinking, no fork, no worktree param for Agents, own ownership model, 40k LOC) and `HenryQW/pi-subagent` (task graph with `dependsOn`, worktrees pinned to Main HEAD, checks, reviewer judgment, stage/promote; but 4 fixed model classes, `pnpm test` hard-wired, 8 tasks per request, own durable state). Adopting either means adopting their model of the world instead of CPD's Job/Attempt/Reviewer vocabulary.
7. **Reviewer (N4):** write a `reviewer` definition with `tools: read,grep,find,ls` + fork; verify with `pi --tools` semantics. Do not reuse any bundled reviewer (`read,bash`).
8. **N12:** try `bellwether` `herdr_watch` on a `gh pr checks --watch` pane for CI; bot-comment ingestion still needs a script.
9. **N17 later:** AndrewJacop/asermax/cowboy show the herdr `agent start --kind` path works for claude/codex/hermes; completion for non-pi kinds depends on herdr state detection.
10. **Skip** pi-herd (UNLICENSED, fixed roles, CLI), pi-harness-runtime (not herdr; usage tracker + LangChain loop; postinstall edits `~/.config/herdr/config.toml`), vishsant (sends task text to a third-party router), entwurf (sibling dispatch, no job semantics), giacolees (opinionated planner workflow).

## Items

### modem-dev/pi-herdr-subagents (npm: none; npm `pi-herdr-subagents` is another repo)
- Meta: ★39 · pushed 2026-09-01 · created 2026-07-07 · license none on GitHub / MIT in package.json, **no LICENSE file** · no releases/tags · 0 dl (not on npm) · SHA b698732 · 1 contributor (Justin Giancola) · 5 open issues.
- What: herdr-native rewrite of HazAT's subagents: argv-launched pi children, socket-event lifecycle, async steer results.
- Mechanism:
  - Launch: `herdr plugin pane open --plugin pi-herdr-subagents --entrypoint subagent --placement split --target-pane $HERDR_PANE_ID --direction right --cwd … --env PI_HERDR_LAUNCH_SCRIPT=… --no-focus` (`src/herdr/client.ts:179-200`); manifest `herdr-plugin/herdr-plugin.toml` (`[[panes]] subagent/argv`, `min_herdr_version 0.8.2`); `dispatch.sh` `exec bash $script`. Generated script holds PATH, `direnv exec` wrap, `pi --session <file> -e subagent-done.ts --model … --tools … @task.md` (`src/launch.ts:312-392`). Needs `herdr plugin link … --enabled` (flags exist in 0.9.3 docs).
  - Completion: `events.subscribe` (`pane.exited`/`pane.closed`) + `.exit`/`.exitcode` sidecars (`src/watcher.ts`, `src/herdr/events.ts`); every child ends in exactly one classified state (README "Lifecycle" table).
  - Delivery: `pi.sendMessage(msg,{triggerTurn:true,deliverAs:"steer"})` per child, no batching (`index.ts:378`). Draft-safe (shared fact).
  - Escalation: `caller_ping` (`subagent-done.ts:251`) writes `{"type":"ping"}`, child exits; parent resumes with `subagent_resume`.
  - Model/thinking: `model` param only; thinking from agent frontmatter (`launch.ts:312-315`).
  - Sessions/tools: `fork:true` param or `session-mode` (`launch.ts:46-51`, `src/session.ts`); `--tools` allowlist always adds `caller_ping,subagent_done` (`launch.ts:126-148`); `deny-tools`/`spawning:false` via `PI_DENY_TOOLS` (`index.ts:1160`).
  - cwd: `cwd` param; children split beside the Lead pane (single workspace, no worktree). Cap/timeouts: none (grep). Metrics: `<session>.context-usage` (tokens, window, percent) only (`src/context-usage.ts`); session path in every steer.
  - Versions: herdr ≥0.8.2, node ≥22, pi peers `*` (devDeps ^0.65.0, `@mariozechner`).
- Maturity: 18 test files (unit + real-pi integration harness with private herdr/tmux), no CI, no releases, last push 6 weeks ago, issues #2 (old herdr CLI shape, predates current code) and #3 (use `@earendil-works` deps) open.
- CPD fit: N1 partial (model+`:level` suffix), N2 full, N3 partial (`caller_ping`), N4 full (fork + allowlist), N17 no (pi only), N18 full. N6/N7/N9/N13 no.
- Gaps/risks: no license file; unreleased; `@mariozechner` imports rely on pi's alias; plugin must be linked into herdr; no worktree/cap/timeout.
- Verdict: **BORROW/FORK**: best launch + lifecycle mechanism of the family; pin or fork it, add worktree/cap/timeout in the Lead skill.

### andrewfung729/pi-herdr-subagents (npm: none)
- Meta: ★0 · pushed 2026-10-01 · created 2026-08-29 · MIT · no releases · SHA 6ce6321 · 1 commit, 1 author · 0 issues.
- What: herdr-only descendant of the tmux-only `amosblomqvist/pi-interactive-subagents`, with batch wake, `ask_question`, and whitelist-only sandboxed children. Not a modem-dev fork (see Summary).
- Mechanism:
  - Launch: `herdr pane split --current|--pane … --direction right|down --cwd --no-focus` then `herdr pane run <pane> <cmd>` / script (`herdr.ts:244-294`, `index.ts:700,1539`); delay `PI_SUBAGENT_SHELL_READY_DELAY_MS`=500 (typed, open-loop).
  - Completion: polls `.exit`, activity snapshot, pane screen; 60 s fatal-stall escalation reads the pane error (README "Fatal startup stalls"); `pollForExit` (`index.ts:1754`).
  - Delivery: `triggerTurn: launchingSubagentSurfaces.size===0 && !autonomousStillRunning` (`index.ts:710-717`) i.e. **wake once per autonomous batch**; each result is still appended to the transcript. `ask_question` always `triggerTurn:true` (`index.ts:1600-1614`).
  - Escalation: `ask_question` (`subagent-done.ts:333-399`) writes `<session>.ask`, session stays open parked `waiting`; parent answers `subagent_message({name,message})` (typed into the pane); reply is the child's next message. Full ask-and-wait.
  - Model/thinking: `model` param validated against `enabledModels` scope, else silently falls back with a warning (`models.ts:81-115`); `thinking` from frontmatter, or `:level` suffix when the def pins none.
  - Sessions/tools: `session-mode: standalone|lineage-only|fork` in agent def only (no `fork` param); **whitelist-only**: every child gets `--no-extensions --tools <allowlist>`, only backing extensions + installed pi packages reloaded (`index.ts:909-1000`); spawn targets restricted via `PI_SUBAGENT_ALLOWED`; loadout snapshot `.loadout.json` replayed on resume.
  - cwd: `cwd` param (relative to parent cwd). Cap: none. Timeout: only the 60 s startup stall. Metrics: activity snapshots (state, tool), no tokens.
  - Versions: herdr ≥0.8.2 (tested 0.9.0, warns ≥1.0), pi ≥0.99 (`@earendil-works`), node 22+.
- Maturity: 3 test files, biome+tsgo+husky, 3-tier test scripts; single commit; no releases; no issues (no users).
- CPD fit: N2 partial (batch wake wrong for per-completion Dispatch), N3 full, N4 full (strongest read-only via whitelist), N16 partial (best sandbox of the set, still full user account). N1 partial (no thinking param).
- Gaps/risks: typed launch race (shell-ready delay, direnv); batch-wake semantics; `--no-extensions` children lose any pi package not in settings `packages`; scope-validation silently swaps the model if the Lead names one outside `enabledModels` (would silently break Tier selection).
- Verdict: **BORROW**: `ask_question` + whitelist-only launch args; do not adopt wholesale.

### HazAT/pi-interactive-subagents (npm: none; install `git:github.com/HazAT/pi-interactive-subagents`)
- Meta: ★715 · pushed 2026-05-12 · created 2026-03-16 · MIT · v3.7.2 (29 releases) · SHA c100577 · 13 contributors · 23 open issues · peers `@mariozechner/*`.
- What: the ancestor: typed-command subagents over cmux/tmux/zellij/WezTerm (+Claude Code children); **no herdr** (PR #57 "add herdr terminal multiplexer support" still open; #32 closed unmerged).
- Mechanism: creates a pane, types `pi … ` after `PI_SUBAGENT_SHELL_READY_DELAY_MS` (500 ms) with verify/retry, polls the screen for a sentinel/`.exit`; steer + `triggerTurn` (`index.ts:882,1481…`); `caller_ping` exit+`subagent_resume` (`subagent-done.ts:269`); `subagent_done` (`:304`); per-spawn `model`, `tools`, `skills`, `cwd`, `fork`, `interactive`, `resumeSessionId` (`index.ts:88-124`); `thinking` only in frontmatter (`:945`); `cli: claude` path (`:1010`). Stall state machine, activity files, status widget; plugin hooks for Claude (`plugin/hooks`).
- Maturity: 3 test files, no CI, active until May then quiet; 23 open issues.
- CPD fit: N17 partial (Claude children) but not on herdr; otherwise superseded by its descendants.
- Gaps/risks: not herdr. Tool names are the family's canonical ones (see conflicts).
- Verdict: **SKIP** for v0 (reference only; revisit for Claude Code Workers if the herdr PR lands).

### boadij/pi-herdsman (npm: pi-herdsman)
- Meta: ★178 · pushed 2026-10-12 · created 2026-09-07 · Apache-2.0 · v0.26.0 (30 releases, release-please) · ~9.2k dl/mo · SHA 8978100 · contributors boadij, kozer, primetimetank21 (+bot) · 15 open issues.
- What: durable Chief→Manager→Lead→Agent orchestration where Pi owns conversations and herdr owns panes/worktrees; async delegation with mailbox-based results.
- Mechanism:
  - Launch: split pane (`herdr pane split --pane … --ratio --cwd --env … --no-focus`, `extension/herdr.ts:1405-1425`) then `herdr agent start <alias> --kind pi --pane … -- --extension … <agentArgs>` (`:1520-1545`): herdr-native, readiness-checked. Placement `tab|subtree|split` (`docs/reference/configuration.md`).
  - Completion/delivery: mailbox + herdr state; result via `pi.sendMessage(...,{triggerTurn:true,deliverAs:"steer"})` (`extension/index.ts:847-854`); asks via `triggerTurn:true` (`:858-874`); health attention only while owner `isIdle()` (`agent-controller.ts:6792…`). Draft-safe.
  - Escalation: `ask_owner` (sole, final tool call of the turn; one outstanding; no timeout; state `blocked`); owner answers `reply_agent` correlated by ask/request/run/pane/session IDs; same assignment resumes (`docs/reference/ask-owner.md`). Not forwarded up the chain automatically: each level asks its own owner. Strongest N3 of the set.
  - Model/thinking: **definition frontmatter only** (`model`, `thinking` incl. `xhigh/max`, omitted = inherit spawning session); `delegate_agent` takes `definition, task, label, files` (`docs/reference/agent.md`, `agent-definition-schema.md`). Per-Tier definitions (15) would be required.
  - Sessions/tools: always a fresh session; **no fork**; closed `tools` allowlist, `excludeTools`, `noTools`, `extensions`, `skills`; `permission` is opaque.
  - cwd: Agents run in the controller's cwd (no param); Manager `delegate_project` creates branch worktrees for Leads (`herdr worktree`).
  - Cap/timeout: none; stale (10 min) health attention only; `contextRetirement` ends agents near context limit. Metrics: `tokens` in `list_agents`, `/agents` Session stats.
  - Versions: `piHerdsman.runtime` pi 1.1.0, herdr 0.9.3 (exactly the CPD stack), node ≥22.19; requires `herdr integration install pi`.
- Maturity: 24 test files, validate/preview/release workflows, 20 ADRs, 30 releases in 5 weeks.
- CPD fit: N1 partial (definition-only), N3 full, N2 full, N5 partial (open issue "declare dependencies, fan-out, and joins"), N15 partial. N4 no (no fork), N6/N7 no, N9 partial (Manager level), N13 partial.
- Gaps/risks: opinionated worldview (Leads own Agents, branch=work handle, one result per assignment), 40k LOC, `delegate_agent` runs in controller cwd so no per-Attempt worktree for Agents, `herdr integration install pi` mutates herdr setup, version churn (0.x, weekly).
- Verdict: **BORROW**: `ask_owner`/`reply_agent` protocol and mailbox ideas; adopt wholesale only if CPD drops its own Job/Attempt model.

### edxeth/pi-subagents (npm: none; npm `pi-subagents` is nicobailon's)
- Meta: ★130 · pushed 2026-09-18 · created 2026-04-16 · MIT · v2.9.2 (30 releases) · git install only · SHA cf6dbf4 · 5 contributors · 2 open issues.
- What: power-user fork of HazAT's with orchestrator mode, background workers, fork, timeouts, spawn limits, best-of-N verified fan-out in worktrees.
- Mechanism:
  - Launch: interactive children typed via `herdr pane split` + `pane run` (`src/mux/herdr.ts:426-438`), placement policy auto/right-stack/tab with min cols/rows (README "Herdr placement"); `mode: background` = headless `pi -p` child.
  - Completion: sidecar/`PollResult` polling (`runtime/interactive-watch.ts`); delivery `pi.sendMessage(...,{triggerTurn:true,deliverAs: steer|nextTurn})` (`runtime/result-router.ts:76-101`); after an async launch the parent's turn is stopped (coordinator-only turn) so results arrive cleanly.
  - Escalation: `caller_ping` (exit; `subagent_resume`) only.
  - Model/thinking: **`model` and `thinking` per launch** (`tools/subagent-tools.ts:67-68,85-86`), gated by `allow-model-override`/`allowed-models`; **thinking validated against the model registry's supported levels, explicit unsupported ⇒ error** (`launch/child-launch-plan.ts:105-157`), matching CPD's "reject invalid Tiers". Tool text tells the model not to pick a model unless the user named one (would need CPD-specific tool prompt/skill override).
  - Sessions/tools: `session-mode: standalone|lineage-only|fork` (fork carries a `<subagent-boundary>` marker), `tools` allowlist, `deny-tools`, `extensions`, `deny-env`, `spawning`, `spawn-depth`, `visible-to`; **orchestrator mode** strips all tools but `subagent*`.
  - cwd: per-agent-file `cwd` only; `forcedCwd` is privileged and stripped from the tool schema (`types.ts:40-50`), used by `llm-as-a-verifier` to put N candidates in isolated git worktrees (`src/vf/worktrees.ts`).
  - Cap: `spawn-width` per session, ceiling 16, **rejects** over-cap (`runtime/spawn-width.ts`, `subagent-tools.ts:449`). Timeout: `timeout`, `idle-timeout`, `timeout-warn-threshold` soft wrap-up, `on-timeout: report|block-resume`; closes the pane; resume keeps same limits (README:187-194,275-390). Metrics: final context use line, output tokens in `PollResult`.
  - Versions: pi ≥0.85, node 22+, herdr min version not stated (unverified).
- Maturity: 101 test files + live herdr e2e scripts; no CI workflow; very large (57k LOC); single main author; install via git only.
- CPD fit: N1 full, N4 full, N7 partial, N6 partial, N2 full, N17 partial (muxes, not agents), N8 no. N9 no (no cwd per spawn). N3 partial.
- Gaps/risks: typed launch; per-spawn cwd missing (a Lead would have to write a temp agent file per Job to set `cwd`); orchestrator mode removes `bash`; tool names collide with the whole `subagent` family; no CI; frequent churn.
- Verdict: **BORROW** (thinking validation, timeout/wrap-up, spawn-width); **FORK** candidate only if per-spawn `cwd` is added.

### kirel/herdr-subagents (npm: herdr-subagents, unpublished)
- Meta: ★3 · pushed 2026-06-19 · created 2026-06-10 · no license · no releases · SHA ed936d8 · 352 LOC · 0 tests.
- What: smallest possible spawn-and-callback: `herdr_subagent` + child `herdr_subagent_done`.
- Mechanism: `pane split|tab create|workspace create` per `placement` then `pane run` typed `pi --session … -e child.ts @task.md --model <model>` (`index.ts:153-170,282`); child writes an exit file `{done, summary}` (`child.ts:28-37`); parent polls every 1 s (`index.ts:176-187`) then `pi.sendMessage(…,{triggerTurn:true,deliverAs: steer|followUp})` (`:204-217`). Params: `task, name, cwd, placement, direction, sessionId, model(:level), piCommand, completionTimeoutMs, deliverAs` (`:228-249`). `completionTimeoutMs`=30 min only stops the watcher and sends "did not report" (does not kill). Child pane stays open.
- Maturity: none (no tests/CI/releases).
- CPD fit: N1 partial (model with `:level`, cwd), N2 full; nothing else. `piCommand` could append `--tools`.
- Gaps/risks: no licence, no resume, no fork, no failure detection except timeout.
- Verdict: **BORROW** as the 300-line reference for "exit-file + steer" (N19).

### tobi/pi-herdr-workers (npm: none)
- Meta: ★13 · pushed 2026-09-01 · created 2026-09-01 · MIT · v0.2.0 · SHA e540d09 · 2 commits (Tobi Lütke) · 1 open issue.
- What: persistent `/team` orchestrator↔worker chat, `/split-handoff|fork`, `/loop` re-wakes, Monitor panes.
- Mechanism: `CreateAgentPanel{name,direction,type,purpose,model,thinking(off..max),initial_prompt}` → `pane split` + `herdr agent start --kind pi --pane … -- --orchestrated-by … --model m:level` (`extensions/herdr-worker.ts:706-740,667`); messages through a per-pane file inbox, delivered via `pi.sendMessage(…,{triggerTurn:true,deliverAs: priority?"steer":"followUp"})` (`:405-406`), non-pi targets via `herdr agent prompt` (`:467`). **No completion detection**: the worker is told by system prompt to `SendToAgent` its report (`:880-884`); a worker that stops silently is never reported. Same cwd as orchestrator; no cap, no timeout, no fork (`/split-fork` is a human command).
- `/loop` (cron/event/idle re-wake, 25-loop cap, read-only wake gate) and Monitors (`MonitorCreate/List/Stop`, panes that stay open; status refreshed on `MonitorList`, not pushed; 25 tracked, `loop/runtime/herdr-monitor.ts:67`).
- Maturity: no tests, no CI, 2 commits.
- CPD fit: N12 partial (timed `/loop` to poll `gh pr checks`), N3 partial (worker→orchestrator `SendToAgent`, safe delivery), N1 full params. Not Job-oriented.
- Verdict: **BORROW** (durable mailbox ack-after-persist, `/loop` for N12); skip as spawner.

### giacolees/plan-herdr-subagents (npm: pi-herdr-subagents-planning)
- Meta: ★1 · pushed 2026-09-07 · created 2026-08-25 · NOASSERTION (LICENSE text MIT) · v0.2.1 · 140 dl/mo · SHA 0bf1b94 · 1 author · no tests.
- What: a `/plan` grill-and-plan→sequential workers→reviewer workflow bundling a **vendored fork of 0xRichardH/pi-herdr-subagents@0.1.6** + todo overlay + pi-lens.
- Mechanism: same typed `pane run` launch (`extensions/herdr-subagents/herdr.ts:249`), `subagent`/`subagent_resume`/`subagent_interrupt`/`subagents_list` + `caller_ping`/`subagent_done`; model routing via `config.json` `models.agents.<role>` (tool-arg → frontmatter → per-agent config → default → parent). Sequential workers (one todo each), pointer-contract plans, `.agent/` cached knowledge. Uses the grilling/domain-modeling skills the user already has.
- CPD fit: none of N1–N12 beyond the vendored base; planning prompts (grill, ISC evidence, P0–P3 triage) are borrowable for `cpd-lead`.
- Verdict: **SKIP** (BORROW prompts only).

### joelhooks/pi-bellwether (npm: @joelhooks/pi-bellwether, not on npm registry)
- Meta: ★83 · pushed 2026-10-08 · created 2026-06-26 · MIT · v1.5.0 (package.json) · SHA b40d24a · bot-authored commits · 1 open issue.
- What: Effect + XState herdr control: structured tools and **watch lifecycles with batched wakes** (replaces the `pi-herdr` fork).
- Mechanism: direct socket client with Schema-decoded envelopes (`src/herdr-client.ts`); tools `herdr_layout`, `herdr_pane`, `herdr_agent` (`agent.start` with `args`, `prompt` with a 30 s "working" proof handshake, `extensions/pi-bellwether.ts:1856-1925`), `herdr_watch` (`agent_state`, `pane_output`; one wait socket each + 5 s `agent.get` liveness probe; `src/watch.ts:165`), `herdr_ping_wait` (degraded fallback). **Wakes**: held until `agent_end`, merged into one `bellwether-wakes` follow-up ~250 ms later, ≤5 min delay (`src/wake.ts:126-128` `{deliverAs:"followUp",triggerTurn:true}`); a worker's own intercom report supersedes its state watch (they measured 124/217 no-op wakes). Sidebar tokens `$wait/$agents/$needs` with a 3-min lease (`src/sidebar.ts`). Suspension entries survive `/reload`.
- Maturity: 11 test files, `npm run check/smoke/pack:check`, but no CI workflow, no releases/tags.
- CPD fit: N2 full (best wake design), N12 partial (`pane_output` watch on `gh pr checks --watch`), N15 partial (sidebar), N19 high. Not a Job/Attempt spawner: spawn = `herdr_agent start` with raw `agentArgs`, no completion semantics ("a matched state does not prove completion").
- Gaps/risks: depends on Effect 4 beta and XState; pi-until optional integration; not published to npm under that name.
- Verdict: **BORROW** (wake coalescing, socket client, sidebar tokens); possible companion for N12.

### giuseppecrj/pi-herdr-agents (npm: pi-herdr-agents)
- Meta: ★49 · pushed 2026-10-12 · created 2026-08-05 · MIT · v3.2.1 (30 releases) · 5.8k dl/mo · SHA 9d4c956 · 5 contributors · 3 open issues.
- What: async herdr-only subagents (lineage of HazAT/0xRichardH) rebuilt as `maestro/{core,adapters/pi,surfaces/herdr}`, with managed worktrees and persistent specialists.
- Mechanism:
  - Launch: tab/pane via `herdr tab create|pane split … --no-focus`; waits for the shell via `pane process-info` (`waitForHerdrShellReady`, `maestro/surfaces/herdr/herdr.ts:1187-1216`) then typed `pane run` (`:1283`). Worktree: `herdr worktree create --cwd --branch --base --label --no-focus` (`:238-257`) with an ownership manifest under `artifacts/<session>/worktree-runs/` written first.
  - Completion: `.exit` sidecar via `fs.watch` + polling fallback (`maestro/core/wake.ts`), child process identity (`adapters/pi/process-identity.ts`), activity files; stall advisories only (`supervision.hangWarningMinutes` 15).
  - Delivery: `pi.sendMessage(…,{triggerTurn:true,deliverAs:"steer"})` per child, bounded 16k chars (`pi-extension/subagents/index.ts:697-704`). Draft-safe.
  - Escalation: `caller_ping`; ordinary child exits and Lead `subagent_resume`s it; **resume is rejected for worktree children** (`docs/tools.md:233-250`); persistent specialists stay alive and are answered with `subagent_send`.
  - Model/thinking: `model` (exact `provider/id`, ordered fallback list, or `task:<category>`), `thinking`; resolution tool arg → frontmatter → `models.agents` → `models.default` → parent (`docs/tools.md:121-145`); fallback only on launch/provider error, never after a completed negative result; worktrees use first authenticated candidate only.
  - Sessions/tools/cwd: `fork:true|false`, `tools`, `skills`, `cwd`, `worktree:{branch,base}` (`base` resolved to an exact commit before creation; default committed HEAD; uncommitted parent changes are not copied), `persistent`. `read,bash` is explicitly "not read-only".
  - Cap: `persistent.maxAgents`=3 only. Timeout: none (advisory). Metrics: none beyond widget; worktree completion reports path, branch, base/head SHA, commits ahead, changed files, clean/dirty (`docs/tools.md:155-176`).
  - Never pushes/merges/removes worktrees automatically; `/worktree` and `worktree_remove` are explicit.
  - Versions: pi ^1.1 devDeps; herdr min 0.9.2 for sidebar plugin (main extension minimum not stated; unverified). Optional Linux sidebar plugin (`herdr plugin install …/plugins/sidebar`).
- Maturity: 38 test files + deterministic integration suite with real pi/herdr and a local provider, CI + release workflows, ADRs, 5 contributors; a removed "workflow subsystem" (ADR-0009) shows scope churn.
- CPD fit: N1 full, N9 full (base SHA) minus setup, N4 full (fork+tools), N2 full, N3 partial, N17 partial (adapter seam, only pi shipped), N6/N7/N10/N11/N13 no.
- Gaps/risks: typed launch; resume vs worktree; "role packs", `/subagents-init` model-preference drafting and sidebar plugin are extra surface; tool-name collision; 63k LOC to own; `casualjim/pi-mimir` forks it (`@casualjim/pi-herdr-agents` 2.0.11).
- Verdict: **REUSE** (primary spawn+worktree candidate) with the Lead skill supplying cap/timeout/retry; **FORK** only if the resume-vs-worktree gap blocks escalation.

### AndrewJacop/pi-herdr (npm: @andrewjacop/pi-herdr)
- Meta: ★33 · pushed 2026-09-27 · created 2026-07-12 · MIT · v0.6.0 (no GitHub releases) · 2.8k dl/mo · SHA e8e6f82 · 1 author · 10 open issues.
- What: orchestrator over a fleet of herdr agent panes (pi, claude, codex, opencode) with sessions, push results, parallel cap, isolated worktrees and scripted workflows.
- Mechanism:
  - Launch: split pane then `agent start <name> --kind <kind> --pane <id> [-- argv]` (one path on every OS; `README` "Platform notes"), version floor herdr ≥0.9.0 (`src/version.ts`). Task submitted via `agent prompt <pane> <text> --wait`; >2000 chars ride `<session>.task.md`.
  - Completion: child extension writes typed sidecar; fallback to JSONL last message; then "gone" after 10 s grace (`delivery.ts`); pi self-reports `agent_start→working`, `agent_settled→idle` to herdr via `pane report-agent` (`selfreport.ts`).
  - Delivery: `push.ts:39-40` `{triggerTurn:true,deliverAs:"steer"}` when `notifications:"normal"`, else `{triggerTurn:false,deliverAs:"nextTurn"}` (`quiet`); a blocked child always wakes. Draft-safe. Takeover/idle re-arm logic for human-driven panes.
  - Escalation: a blocked child (pi-ask-user overlay via `herdr:blocked`) wakes the parent; answer with `herdr_message_agent` (freeform) or `herdr_send_keys` (option lists). `herdr_message_agent` to the reserved `orchestrator` role is **typed with `agent prompt`** (`tools/orchestration.ts:233`): clobbers a Lead's draft and "delivered ≠ consumed". No dedicated ask tool.
  - Model/thinking: `model`+`thinking` per spawn, 5-level chain, exact `provider/model-id`, thinking must be a valid pi level, **enforce-or-error naming the level** (`launchplan.ts:94-190`).
  - Sessions/tools/cwd: `fork:true` (copy conversation truncated before the last user message, noise filtered), `lineage-only`, `standalone`; `tools`, `exclude_tools`, `agent_args`; `cwd`; `isolated:true` = herdr worktree with auto branch+path, **no `base`** (`spawn.ts:870`, `tools/worktrees.ts:99-113`), mutually exclusive with `cwd`.
  - Cap: `max_parallel_agents`=3 with queue (`spawn.ts:273-276`), `max_spawn_depth`=2, `agents_kill_switch`. Timeout: none (watchdog flags `stalled`). Metrics: `budget.spent()` output tokens from session JSONL for workflows (`workflow/host.ts:139-165`); widget shows tokens/state.
  - `herdr_run_workflow`: sandboxed JS fan-out (`agent()`, `parallel`, `pipeline`, `gate` shell command, `resume`, journal replay) ported from tintinweb/pi-subagents.
- Maturity: ~30 offline test scripts + live ones, no CI workflow; README still says "v0.6 in flight" for features that exist in the code (fork, push, routing); macOS gotcha: herdr started via `brew services` has a minimal PATH and pi dies silently (relevant on the user's Mac).
- CPD fit: N1 full, N6 full, N2 full (results), N17 partial (claude/codex), N9 partial (no base), N11 partial (workflow `gate`), N4 full (fork+allowlist), N13 partial. N3 partial/hazardous, N7 no.
- Gaps/risks: terminal-typed messaging; undocumented base for `isolated`; docs lag code; single author; pi 0.x-era README.
- Verdict: **BORROW** (queue/cap, thinking enforce-or-error, `notifications` modes, `kind` passthrough); candidate REUSE if `isolated` gets a `base` (small patch) and `herdr_message_agent` upward path is avoided.

### ribbons-digital/pi-herd (npm: pi-herd, private)
- Meta: ★22 · pushed 2026-07-06 · created 2026-07-01 · **UNLICENSED (package.json) / none (GitHub)** · v0.1.0 · SHA 3211872 · CI + 20 tests.
- What: CLI + herdr plugin + `/herd` slash commands orchestrating a run with fixed roles (planner, implementer, reviewer, tester) in worktrees; artifacts under `.pi-herd/runs/`.
- Mechanism: `pi-herd start|send|wait|collect|merge-plan|cleanup`; completion via artifact freshness / `pi-herd-verdict: done pass=N` marker, `wait` polls (exit 0/2/3); per-role model/thinking via harness profile (no validation); implementer owns `pi-herd/<run>/impl`; herdr ≥0.7.1; plugin actions `doctor`, run-board pane, `start-wizard`.
- CPD fit: none for spawn/dispatch (human-driven CLI, no push to the Lead). Concepts (verdict marker, run artifacts) only.
- Verdict: **SKIP** (no license).

### fyang93/pi-herdr-swarm (npm: not published; npm `pi-herdr-swarm` is another repo)
- Meta: ★1 · pushed 2026-10-07 · created 2026-10-01 · MIT · v0.1.0 · SHA 5095a08 · 30 commits, 1 author · 8 test files · 1.1k LOC.
- What: named peers + messages + sessions; `swarm_spawn`, `swarm_send`, `swarm_list`.
- Mechanism: `pane split` (roomiest pane) + `herdr agent start <name> --kind pi --pane … -- <args>` (`src/herdr.ts`, `run.ts:235-239`); result read from the child's session on exit and delivered as one `swarm_result` custom message: only when `ctx.isIdle()`, entered at turn boundary via the pi `agent_before_settle` hook (`src/run.ts:224-231,247-250`), so it never lands mid-turn and "clearing the input queue does not discard results". Child auto-exit refuses if `context.ui.getEditorText()` (`run.ts:195`). Escalation: `swarm_send({to,message,wait:true})` stops the sender's turn until a reply; works either direction. **Messages go through `herdr agent prompt`** (terminal typing; README "Delivery": clearing input queue can discard): upward escalation would type into the Lead's editor. Params: `task, agent(preset), name, model, cwd` (thinking via preset only); `can-spawn` policy; `maxAgents` 16 precheck. Needs pi ≥0.99.2, herdr ≥0.9.0, `herdr integration install pi`.
- Maturity: tiny, 9 days old, tests present, no CI/releases.
- CPD fit: N2 best-in-class design, N3 partial (hazard above), N6 partial, N1 partial.
- Verdict: **BORROW** (idle/turn-boundary delivery; session-derived wait state).

### junghan0611/entwurf (npm: @junghanacs/entwurf)
- Meta: ★27 · pushed 2026-10-12 · created 2026-04-16 · MIT · v2026.10.12-herdr.0.35.0 (30 releases) · 4k dl/mo · SHA 3d6cee6 · 5 open issues · CI + 45 tests · 127k LOC.
- What: "garden-citizen dispatch substrate": record-addressed sibling sessions (pi, Claude Code, Copilot, OMP, Codex, Antigravity) messaging each other over a control socket / mailbox; not workers.
- Mechanism: `entwurf_v2`, `entwurf_fresh_call {backend, model, task, cwd}` opens a new unfocused herdr tab (pi/Claude Code only), `entwurf_resume_call`; pi receive via a record-addressed Unix control socket (explicitly not pane-typing); needs pi `>=1.1.0 <1.2`, Node ≥24, MCP bridge, optional pi-durable carrier. Receipts mean launch/enqueue, never completion; no cap/timeout/worktrees/results.
- CPD fit: N17 (Claude Code bridge) idea only; contradicts Job/Attempt (siblings, "not a disposable worker").
- Verdict: **SKIP** (BORROW only for a future Claude bridge).

### HenryQW/pi-harness (npm: @henryqw/pi-subagent, @henryqw/pi-task-models, @henryqw/pi-herdr, @henryqw/pi-herdr-tools)
- Meta: ★16 · pushed 2026-10-12 · created 2026-08-07 · MIT · no GitHub releases (npm `@henryqw/pi-subagent` 27.1.1: 24.6k dl/mo; `pi-task-models` 18.2k) · SHA 6842d93 · 4 contributors · CI + publish · 95 test files · monorepo of 19 extensions (`install.sh` "highly opinionated Pi harness").
- What: `@henryqw/pi-subagent`: `delegate_task` direct (Herdr tabs, shared checkout) or **isolated checked task graphs** in Herdr worktrees with per-task checks, reviewer judgment, staging, integration and promotion to Main.
- Mechanism: `herdr worktree open`+`tab create`+`agent start`+`agent prompt … --wait`+`agent wait` (`src/herdr-runtime.ts:623,882,921,983,1128`); coordinator is extension code in Main's process with durable state v5 under `config/pi-subagent/state/` and a productive lease; results/attention as Pi follow-ups ("if Main is idle the result starts a turn; if busy it queues after the current turn"). Tasks: `{id, kind: text|changeset, role, modelClass, requirements, deliverable, dependsOn, contextFrom, checks, judgment}`, 1–8 per request; workers branch from Main's pinned committed branch/HEAD/tree; `subagent_stage` merges candidates into an owned integration checkout (committed merges), `subagent_integrate validate|correct|promote|refresh|release`, `subagent_status`, `subagent_abort`, `/subagent recover|prune`; reviewer role has `git_read`. Children run `--no-extensions`.
  - Model: **only four classes** `fast|balanced|frontier|fav`, each a model+thinking route configured with `/task-models` (primary + fallback); no per-spawn arbitrary model.
  - Cap/timeouts: `maxSubagents`=5 (direct, FIFO pool), `maxTurns`=50, optional `maxTokens`, `timeout.idleMinutes`=10 / `maxMinutes`=30 for direct and ephemeral children; **isolated productive requests have no wall-clock deadline**; `maxCorrections`=1.
  - Metrics: result lines show model, thinking, tokens, elapsed (`✓ … · provider/model · thinking high · 1.5k tok · 42s`).
  - Escalation: none upward from children (`ask_question` excluded); "needs attention" to Main; Main sends follow-ups.
  - Versions: **pi ≥1.1.0 <2, herdr ≥0.9.3, API protocol ≥22** (exactly the CPD stack).
- CPD fit: overlaps N5 (dependsOn), N9 (pinned HEAD worktrees), N10 (stage/promote, commits kept as merges), N11 (checks), N4 (judgment), N6/N7 (direct only), N13. But: reviewer is a fresh child with an exact patch, not a fork of the Lead (N4 differs); `pnpm test` root script **required** and prepended to final checks (not generic; iOS repos fail); graph is submitted per request (Locked/append-only not modelled the CPD way); `delegate_task` encourages Main to write in shared checkout.
- Gaps/risks: very strong opinions and a large state machine (11k LOC in the package) owning Dispatch; fixed 4 tiers instead of 15; npm-first distribution.
- Verdict: **BORROW** (design reference for pinned-HEAD worktrees, exact-identity staging, recovery; read `docs/orchestration.md` before specifying CPD's merge step). Adopting it would replace the `cpd-lead` Dispatch loop.

### ManotLuijiu/pi-harness-runtime (npm: pi-harness-runtime)
- Meta: ★1 · pushed 2026-10-12 · created 2026-06-26 · MIT · v1.1.249 (30 releases) · 31.9k dl/mo · SHA b79fe95 · branch `develop` · 1 author · 154 test files · README says "[BETA] not production ready".
- What: `/usage` token tracker (the download driver) plus a LangChain "autonomous coding loop" and clipboard-bridge/LINE/Telegram bits.
- Mechanism: "herdr" in the code is a **file-based JSONL event bus** (`packages/event-bus/src/herdr-bus.ts`), not the herdr CLI/socket; no pane/agent start. `postinstall` copies `config.toml.example` to `~/.config/herdr/config.toml` if absent and copies plugins (`package.json` scripts).
- CPD fit: none (a todo↔`bd` sync feature exists but is unrelated to herdr).
- Verdict: **SKIP** (postinstall touches herdr config; not a subagent system).

### vishsant/pi-herdr-smart-subagents (npm: none; package name `pi-herdr-subagents`)
- Meta: ★0 · pushed 2026-10-02 · created 2026-10-02 · NOASSERTION (MIT in package.json, LICENSE file present) · default branch `feat/jev-routing` · SHA 39e3201 · 22 tests.
- What: modem-dev fork adding opt-in "Jev" model routing (sends task text + candidate descriptions to `api.typesafe.ai`), single-level nesting by default, autonomous-completion defaults, child-widget shortcut. Same package name as modem-dev ("do not load alongside").
- Mechanism: identical to modem-dev plus `src/routing.ts`, `src/nesting.ts`.
- CPD fit: negative (third-party router receives task text; auto model choice conflicts with Tiers).
- Verdict: **SKIP**.

## Additional finds

Searched `gh search repos` (6 queries) and the npm keyword query (≈90 `herdr` pi packages). Traction filter: ≥~1k downloads/mo or ≥~25★ or directly CPD-relevant. Not reviewed: the many tab-naming, status, "btw/fork/aside" and single-purpose packages (`pi-herdr-btw`, `@henryqw/pi-herdr-tools`, `pi-forkme`, `herdr-aside`, `@tifan/pi-rename`, `pi-herdr-task-label`, prompt-state/blocked-reporting packages, `@muzi3/herdr`, etc.).

### 0xRichardH/pi-herdr-subagents (npm: pi-herdr-subagents)
- Meta: ★69 · pushed 2026-10-08 · created 2026-06-25 · MIT · v0.2.1 · 702 dl/mo · SHA 76a5975 · 18 contributors (HazAT lineage) · 17 open issues · CI workflows.
- What/mechanism: HazAT's design ported to herdr with typed `pane run` (`pi-extension/subagents/herdr.ts:257`), `subagent` with **`model` and `thinking` per spawn** (`index.ts:112-149`), projected states (`starting/active/waiting/interrupted/stalled`), steer+trigger (`index.ts:1063…`), `caller_ping`. The ancestor of giuseppecrj and the base vendored by giacolees.
- Verdict: **SKIP** (giuseppecrj supersedes it); useful as the npm-published sibling of the family.

### mu-crew/mu (npm: @mu-crew/mu)
- Meta: ★4 · pushed 2026-10-11 · created 2026-05-07 · MIT (package.json; no LICENSE file) · v3.12.0 · 4.7k dl/mo · SHA d53d5e3 · 1 author (martintrojer) · tests present.
- What: control plane for a crew of agents in tmux **or herdr** panes. Work is a **task DAG in SQLite** (`--blocked-by`, claims, owners, notes, workstreams) with an **append-only ops log and `mu undo`/`mu rebuild`**, per-agent VCS workspaces (jj/sl/git worktrees), adversarial-review and fan-out recipes; pi extension serves a control socket per agent (send/state/wait/abort) and `mu_delegate` delivers results as `followUp` + `triggerTurn` (`extension/delegate.ts:563-570,788,897`). "Not a model router": model/tier is passed in the spawn command (`--cli pi_big`).
- CPD fit: overlaps N5 (DAG/ready/history; competes with beads), N15 (dashboard), reviewer gates as tasks, N9 (workspaces). Not read deeply: this is a whole alternative architecture.
- Verdict: **BORROW** (job-graph/ops-log ideas; compare with beads in the N5 cluster).

### howaboua/pi-shepherdr (npm: @howaboua/pi-shepherdr; monorepo `IgorWarzocha/howaboua-pi-stuff`)
- Meta: ★401 (monorepo) · pushed 2026-10-12 · MIT · v0.2.19 · 3.9k dl/mo · SHA 76f2077 · 4 contributors · no tests found in the package.
- What/mechanism: one routed `agents` tool (`spawn|assign|send|watch|reply|read|attach|…`), herdr socket `events.subscribe` (`src/herdr-client.ts:278`), blocking or async calls, completion/blocked pushed as steer; profiles (`profiles/*/profile.json` with pinned `model`, `thinking`, `pi_args`, **no per-spawn model**), remote machines through herdr profiles. Notable for N2: messages sent through `agents` **bypass the receiving Pi editor, preserving unsent drafts**, whereas raw `herdr agent prompt` types into the terminal (README "Agent calls"). Requires pi 1.1.0+, herdr 0.9+, `herdr integration install pi`.
- Verdict: **BORROW** (extension-channel delivery that preserves drafts, `reply` protocol).

### electric-dragon-80000v/pi-cowboy (npm: pi-cowboy)
- Meta: ★14 · pushed 2026-10-12 · created 2026-10-06 (6 days old) · MIT · v0.8.2 · 1.7k dl/mo · SHA db6c019 · 1 author · extensive tests (73k LOC incl. tests) · pnpm 12 / node ≥24.20.
- What: orchestrator where **each agent = own pi process in its own herdr workspace + CoW git worktree**; queue; merge and cleanup tools; orchestrator templates (e.g. `with code review`).
- Mechanism: `cowboy_agent{agents:[{prompt, task_name, agent_type, model, harness_type}]}` returns IDs immediately; results via `pi.sendMessage` (`src/spawn/spawn-coordinator.ts:246,292`); `steer_cowboy_agent` (revives a settled agent), `stop_cowboy_agent`, **`merge_cowboy_branch`** (plain `git merge`, FF or merge commit, into a target in the main working tree which must be on that branch and clean; conflicts stay; `src/infrastructure/git/git-merger.ts`), `cleanup_cowboy_agent` (removes pane, tab, worktree, branch only if merged, keeps dirty worktrees). Worktrees under `~/.pi/agent/pi-cowboy/worktrees/`, copy-on-write (APFS `clonefile` via python3 on macOS; falls back to `checkout` on ext4), seeding ignored state such as `node_modules`/`.env` (`git-materializer.ts`), "dirty" vs HEAD state. **Concurrency**: global 4 + per-provider/model/harness limits, queued, first waiting agent whose levels all have room starts. `harness_type`: `pi|pig|pi-bolt|opencode|codex|claude`. Thinking comes from config/template, not a per-call param. No escalation tool, no timeouts, no reviewer.
- CPD fit: N6 full, N9 full incl. setup via CoW seeding (needs verification on the user's APFS/Mac), N10 partial, N17 partial, N1 partial (model per call). Brand new; engine/node floors high.
- Verdict: **BORROW** (worktree CoW seeding + queue + merge/cleanup contract); watch for maturity; strongest fit for N6/N9/N10 among herdr-native packages.

### itc-steve/pi-herdr (npm: @itc-steve/pi-herdr)
- Meta: ★0 · pushed 2026-10-07 · created 2026-07-23 · MIT (LICENSE text; GitHub NOASSERTION) · v2.0.1 · 895 dl/mo · SHA e0a9d70 · 1 author · tests with a fake herdr client.
- What/mechanism: `herd` tool: named workers in `~/.pi/agent/herd.json` (model + thinking + `maxConcurrent`), `spawn worker=|model= thinking= owns= after= output=`, queue, **`owns=` write lanes** (overlapping writers serialise), **`after=` runs only after the Lead `accept`s prerequisites** ("Completed does not mean approved"), `steer`, `abort`, worker↔worker mailbox notes, private/local-only routing with redaction; completion messages `triggerTurnOnResult` default true, `resultDelivery` pointer|full; `timeoutMs` default 10 min is a **stall timeout**; each job in a background herdr tab (`src/herdr/client.ts` tab create + `agent wait`); one shared checkout (no worktrees).
- CPD fit: concept-level match to Dependencies + "Merged" gate (`accept`), per-spawn model/thinking, per-worker cap; no worktrees, no fork.
- Verdict: **BORROW** (accept-gated dependencies; file-ownership lanes).

### Others skimmed (one line each)
- `chankov/agent-fleet` (npm `@chankov/agent-fleet`, ★19, 14.5k dl/mo, MIT): pi-first "thin dispatcher" + Verification Contract + herdr tiled peer panes + Hermes Desktop plugin + Telegram relay for questions; heavy framework/kit; **BORROW ideas** for N14 (phone relay) and Hermes (N17); not a drop-in.
- `jillesme/pi-herdr-squad` (★26, MIT, 118 dl/mo): `/herdr-squad` read-only investigation squads (1–12) and `/herdr-worker-squad` coding in one shared checkout with exclusive writable-root ownership; prompt-template driven, no worktrees; **SKIP**.
- `funsaized/pinata` (npm `pi-pinata`, ★1, MIT, 1.5k dl/mo, 8 days old): in-process agents by default, herdr panes optional; builder in git worktree + setup command detection + checks + **independent reviewer must approve before the change is applied (uncommitted, never staged/committed)**; wrong merge semantics for CPD (no commits kept) but a good reference for N9 setup and review gating; **BORROW** docs `how-to/dependencies.md`.
- `asermax/herdr-subagents` (npm `@asermax/pi-herdr-subagents`, ★1, no license, v0.11.0, 1.8k dl/mo, SHA 94b8ac1, tests+ADRs): one source builds a pi package and a Claude Code plugin; `subagent` tool `command: spawn|prompt|collect|list|close|read|unblock|wait`, options `kind, agent, label, model, worktree, branch, base`; completion via herdr `events.subscribe pane.agent_status_changed` (`src/helper/watch.ts`), wake is payload-free (parent then `collect`s), `pi.sendMessage` with `triggerTurn` (`src/extension/parent-role.ts:36`); child question = `<subagent-ask>` tag; worktree child = a tab in a herdr worktree workspace (ADR-0009). **BORROW** for N17 (works for Claude via herdr state) and event-driven wake.
- `@zzjcool/pi-herdr-subagents` 0.14.3 (2.9k dl/mo): renamed to `pi-legion` (`zzjcool/pi-herdr-subagents`, ★1, SQLite, headless pi RPC, "herdr pane execution archived to legacy"): **SKIP** (herdr dropped; note `pi-herdr-swarm` on npm is this repo's earlier name, unrelated to fyang93's).
- `@maxiaochao/pi-herdr-subagents` (2k dl/mo, in personal `pi-toolkit` monorepo, no license), `@asermax`, `@qcts33/pi-herdr-subagents` (Forgejo-hosted), `@yofriadi/pi-subagent-herdr`, `@maxedapps/pi-subagents-herdr`, `@luminascale/pi-shepherd` (1.4k dl/mo), `pi-pier`, `@sting8k/pi-peer` (1k dl/mo, peer messaging), `@ogulcancelik/pi-herdr` (generic herdr control tool, 1.2k dl/mo, ★559 monorepo), `@narumitw/pi-herdr`, `@casualjim/pi-herdr-agents` (fork of giuseppecrj), `supersubs`, `pi-herdr-flock`, `pi-herdr-managed-agents` (pi+Claude+Codex, 0 dl), `@mcuste/pi-herdr-worktree`, `@evoclock/pi-agentic-driver` (3.5k dl/mo, "advisory review, bounded Herdr communication"): **not reviewed** beyond listing; check `@luminascale/pi-shepherd`, `pi-herdr-managed-agents`, `@evoclock/pi-agentic-driver` first if the main candidates disappoint.
- N14 candidates seen but not reviewed: `@henryqw/pi-ask-question` (2.5k dl/mo, in HenryQW monorepo), `@alexshpunt/pi-ask` (herdr blocked-state aware, 972 dl/mo), `pi-ask-herdr`.
