# t1: How should `cpd` start Workers and Reviewers?

Tier: S4 · Report: `~/projects/cpd/docs/trials/t1-spawn.md` · Budget: **60 minutes** (overrides the common 45) · Read `_common.md` first.

## Goal

Decide, with evidence, which of three ways `cpd` should use to start a Worker (fresh pi session for one Attempt) and a Reviewer (read-only fork of the Lead) in herdr panes, and how completion reaches the Lead without corrupting the User's input.

## Options to compare

- **A. `cpd`-native.** A shell script stands in for `cpd`. It starts agents with herdr directly and notifies the Lead through a tiny pi extension that you write. No spawner package.
  - Pane: `herdr --session cpd-trial pane split <pane> --direction right --cwd <worktree> --no-focus` (or a new tab). Read the new pane id from `.result.pane.pane_id`.
  - Start: `herdr --session cpd-trial agent start <name> --kind pi --pane <id> -- --model github-copilot/claude-haiku-4.5:low [--tools …]`. Per herdr source `src/app/agents.rs:197-199`, args after `--` are appended to the `pi` argv; confirm that by observation.
  - Task: `herdr --session cpd-trial agent prompt <name> "<task>" --wait --timeout 300000`. Typing into a Worker is fine because a Worker has no human draft.
  - Completion: `agent wait` and/or a completion file the Worker writes as the task's last step.
  - Wake the Lead: your script writes a JSON file into an inbox directory. A ~30-line pi extension loaded into the Lead with `-e`, which you write at `/tmp/cpd-trials/t1/wake-ext.ts`, watches that directory (start the watcher in `session_start`, stop it in `session_shutdown`) and calls `pi.sendMessage({customType: "cpd", content: …, display: true}, {triggerTurn: true, deliverAs: "followUp"})`. Model the shape on `~/.pi/agent/install/releases/1.1.0/node_modules/@earendil-works/pi-coding-agent/examples/extensions/send-user-message.ts` and `file-trigger.ts`.
  - Reviewer: `agent start <name> --kind pi --pane <id> -- --fork <lead-session.jsonl> --tools read,grep,find,ls --model …`. The Lead session path is `herdr --session cpd-trial pane get <lead-pane>` → `.result.pane.agent_session.value`.
- **B. giuseppecrj/pi-herdr-agents** (npm `pi-herdr-agents`, v3.2.1; clone at `/tmp/cpd-research/repos/giuseppecrj__pi-herdr-agents`). Load it into the Lead with `pi -e npm:pi-herdr-agents@3.2.1` (or `-e` the clone if npm fails). The Lead calls its `subagent` tool. Because the Lead is an LLM, give it exact instructions ("Call the subagent tool with exactly these parameters: …").
- **C. modem-dev/pi-herdr-subagents** (git only, clone at `/tmp/cpd-research/repos/modem-dev__pi-herdr-subagents`, SHA b698732). Copy it to `/tmp/cpd-trials/t1/modem-dev` and run `npm install` there if needed. It requires its bundled herdr plugin: `herdr plugin link /tmp/cpd-trials/t1/modem-dev/herdr-plugin` then `herdr plugin enable pi-herdr-subagents`. The plugin registry is **global** to the user (allowed for this trial only). You **must** `herdr plugin unlink pi-herdr-subagents` at the end and show `herdr plugin list` without it. Load the extension into the Lead with `-e /tmp/cpd-trials/t1/modem-dev`.

## Setup

- Scratch repo: `/tmp/cpd-trials/t1/repo` (git init, one commit). One plain git worktree per spawned Worker under `/tmp/cpd-trials/t1/wt/<n>` (`git worktree add`). Worktree tooling is t3's job; keep this simple.
- herdr: create your own workspace `herdr --session cpd-trial workspace create --cwd /tmp/cpd-trials/t1 --label t1 --no-focus`. Start each option's Lead as a pi agent in a pane of that workspace (`agent start lead-a|lead-b|lead-c --kind pi --pane … -- --model github-copilot/claude-haiku-4.5:low -e …`). Run options one at a time; the machine has 8 GB.
- All test agents use `github-copilot/claude-haiku-4.5:off` or `:low`. Exception: one check per option uses `:medium` to prove the per-spawn level is honoured.

## Acceptance criteria (per option A, B, C)

1. **must** Launch reliability: A ≥10 sequential Worker launches; B and C ≥5 each. Trivial task: "create file `hello-<n>.txt` containing `<n>` in the current directory, then stop". Count successes, failures, and time to ready.
2. **must** Per-spawn model + thinking honoured: show the child's session file entries (`model_change`, `thinking_level_change`, or assistant `model`/`thinkingLevel` fields) for `:low` and `:medium`, and show `ps -o args= -p <pid>` of the child pi process.
3. **must** Worker runs in the given worktree: the file appears in `/tmp/cpd-trials/t1/wt/<n>`, and the session header `cwd` matches.
4. **must** Read-only forked Reviewer: the fork contains the Lead's earlier conversation (ask the Reviewer to quote a token you put in the Lead's first prompt). Its tool set excludes `bash`, `write`, `edit` (evidence from the session's initial system/tool record and a refused write attempt).
5. **must** Completion reaches the Lead as a custom message (`"type":"custom_message"` or equivalent entry in the Lead's session JSONL). Measure the delay from Worker completion to Lead entry.
6. **must** Draft safety:
   - Put text in the Lead's editor without submitting: `herdr --session cpd-trial pane send-text <lead-pane> "DRAFT-<option>-123"`, with no Enter.
   - Then trigger a completion notification.
   - After the Lead's resulting turn, show `pane read <lead-pane> --source visible` still containing the draft in the editor.
   - Show that no user message containing it exists in the Lead's session JSONL.
   - Do this for A. For B and C, explain from their code whether delivery uses `sendMessage` (draft-safe) or terminal typing, and demonstrate if time allows.
7. **should** Killed child: `kill` a running Worker's pi process mid-task. Record how A (agent wait / pane state / `pane.exited`), B and C report it, and how long that takes.
8. **should** Escalation path:
   - A: a Worker runs a script that writes a question file and blocks polling for an answer file. The wake extension surfaces it to the Lead. Answer it by writing the answer file, and the Worker continues.
   - B and C: document their mechanism (`caller_ping` and resume) from code. Run it once if time allows.
9. **should** Cleanup: show no leftover panes, processes or plugin links from your trial.

## Questions the report must answer

- Which option should `cpd` use for v0, and why (reliability, determinism, draft safety, agent-agnostic fit with a TypeScript `cpd` CLI, dependency weight)?
- If A: the exact herdr command sequence and the minimal pi extension `cpd` needs (paste the working `wake-ext.ts`).
- If B or C: what `cpd` would still have to do itself (cap, timeout, worktree, metrics), and the coupling risk of an LLM-driven tool call as the spawn path.
- Any herdr behaviour that surprised you (readiness, trust dialogs, `agent_not_ready`, typing races).
