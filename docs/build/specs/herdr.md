# Job `herdr`: herdr adapter, agent launch, `lead start`, `lead attach`

Tier S3 · Needs: `config-tiers` · Read `_common.md` first. Contract sections: §3 (agent names), §6 (`lead`), §11 (`lead start`, `lead attach`), §12 steps 4–7 and 9 (herdr parts). Trial evidence (normative): `docs/trials/t1-spawn.md` (F1–F10, Recipes "Spawn a Worker (A)", "Spawn a Reviewer (A)", "Dead Worker detection").

## Goal
A typed TypeScript library for every herdr operation `cpd` needs, plus the two Lead commands. It must handle the herdr behaviours trial t1 found (3 s ready delay, trust dialog, idle-after-kill, opaque pane ids, session path from herdr).

## Files you own
`src/herdr/herdr.ts`, `src/herdr/panes.ts`, `src/herdr/agents.ts`, `src/herdr/worktrees.ts`, `src/herdr/session.ts`, `src/commands/lead-start.ts`, `src/commands/lead-attach.ts`, `test/herdr/**`, `test/commands/lead-*.test.ts`.

## Requirements
- `herdr.ts`: runner adding `--session <config.herdrSession>` when set. Parses the JSON envelope (`result` or `error`) with zod. A herdr error becomes `herdr_<code>` with the message.
- `panes.ts`:
  - `splitPane(target, {direction, cwd, env?, noFocus: true})` returns the pane id;
  - `closePane(id)`, `paneGet(id)`;
  - `rootPaneOfWorkspace(workspaceId)` (`pane list --workspace`);
  - `closeWorkspace(id)` (close child workspaces first per t3 F9, or surface `workspace_group_close_required`).
- `worktrees.ts`: `openWorktree({repo, path, label})` returns the workspace id (idempotent, t3 F3), and `workspaceForWorktree(repo, path)`.
- `agents.ts`:
  - `startAgent({name, kind: "pi", pane, timeoutMs, args})` returns `{argv, sessionPath}`. Store `.result.argv` and `agent_session.value`; the session file may not exist yet (t1 F10).
  - `promptAgent(name, text, {wait: true, timeoutMs?})`.
  - `agentGet(name)` returns the agent, or `null` when `agent_not_found`.
  - `workerArgs(modelArg)` = `["--approve", "--model", modelArg]`.
  - `reviewerArgs(leadSessionPath, modelArg)` = `["--approve", "--fork", leadSessionPath, "--tools", "read,grep,find,ls", "--model", modelArg]`.
  - Validate names and refuse args containing control characters before calling herdr (t1 F10).
- `session.ts`: `readSessionHeader(path)` returns `{id, cwd, parentSession?}`; `lastAssistant(path)` returns `{stopReason, text}` (zod over JSONL lines, unknown entry types ignored, malformed lines fail loud).
- `cpd lead start`: per §11 (`exec` replaces the process; use `process.execve` if available in Node 22, otherwise spawn with `stdio: inherit` and exit with the child's code; document which). It must refuse outside herdr.
- `cpd lead attach --wf <id>`: per §11, writing `workflow.json.lead` and `leads/<sessionId>/workflow`. Use the `workflow.ts` read/write helpers from `ledger-core` if present; otherwise write `workflow.json` atomically yourself with the §6 shape. Check which exists in your base.

## Acceptance
1. Unit tests with a fake `herdr` binary (`test/helpers/fakeBin`) asserting the exact argv for: split, close, worktree open, agent start (Worker and Reviewer args), prompt `--wait`, agent get, and `--session` injection when configured.
2. Error mapping: a fake herdr returning `{"error":{"code":"agent_not_found",…}}` gives `agentGet` `null`; `server_not_running` gives `herdr_server_not_running` (exit 1).
3. `session.ts` against a fixture JSONL. Build it from the real shapes in t1 F2/F4/F7: a finished session gives `stopReason "stop"`; a killed one gives `"toolUse"`.
4. `lead attach` outside herdr gives `not_in_herdr`. With `HERDR_ENV=1`, a fake herdr pane get and a fixture session file, it writes the `lead` object and the `leads/<id>/workflow` file.
5. `CPD_IT=1` integration test against `herdr --session cpd-test`:
   - split a pane;
   - start a Worker with `github-copilot/claude-haiku-4.5:low`;
   - prompt it to create a file;
   - assert the session `thinking_level_change` is `low` and the file exists;
   - close the pane.

   Record the transcript in the report if you ran it.

## Out of scope
Deciding the lifecycle (Job `attempt-run`); the Lead queue; plannotator.
