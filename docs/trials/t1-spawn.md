# t1: How should `cpd` start Workers and Reviewers?

Date: 2026-10-12 (18:27–18:52 EDT, ~25 min wall of the 60-min budget).
Versions: herdr 0.9.3 · pi 1.1.0 · node 22.22 · all test agents `github-copilot/claude-haiku-4.5` at `:off|:low|:medium`.
Third-party exercised:
- B `pi-herdr-agents` **3.2.1**, loaded with `pi -e npm:pi-herdr-agents@3.2.1`. The npm package has no `gitHead`; the clone at `/tmp/cpd-research/repos/giuseppecrj__pi-herdr-agents` is `9d4c956af1885af4e2e8ab20368d6445779cf96c`, used for code reading only.
- C `modem-dev/pi-herdr-subagents` **b6987324284b1fa22b2eb9f0effaf956ada27333** (package 0.1.0, bundled herdr plugin 0.2.0), copied to `/tmp/cpd-trials/t1/modem-dev`.

Scratch and logs: `/tmp/cpd-trials/t1/` (`spawn-{a,b,c}.log`, `wake-ext.ts`, `spawn-worker.sh`, `b-run.sh`, `c-run.sh`, event logs).

## Verdict

- **Use Option A (`cpd`-native) for v0.** In A, `cpd` drives `herdr pane split` + `herdr agent start/prompt` and wakes the Lead with a 39-line pi extension.
  - It met every must criterion: 14/14 launches, per-spawn model and thinking, correct cwd, read-only forked Reviewer, completion delivered as a `custom_message`, and a draft-safe wake.
  - It adds no third-party dependency, and the spawn path is deterministic code rather than an LLM tool call.
  - It keeps `cpd` as the control plane. B and C make the Lead's LLM the control plane, so a TypeScript `cpd` (cap, ledger, metrics, timeout) cannot own spawning.
- **Completion must be verified by an artefact, never by `agent prompt --wait` alone.**
  - After `kill -9` of the Worker, `agent prompt --wait` returned rc 0 with `agent_status: idle`, 273 ms later. That is indistinguishable from success.
  - Cheap discriminator: the last assistant entry in the Worker's session JSONL has `stopReason:"stop"` when finished and `"toolUse"` when killed. A completion file the Worker writes works too. Also watch for `released:true` / `agent_not_found`.
  - B and C report a killed child as `failed (exit code 137)` natively. This is their one clear advantage.
- **Draft safety is proven for all three.**
  - A (extension + `pi.sendMessage`), B and C all deliver through `pi.sendMessage` and left the draft intact, both when the Lead was idle and (A) when it was busy.
  - Negative control: `herdr agent prompt` into a Lead with a draft merged and submitted the draft (`"DRAFT-A-XYZ NOTIFY: …"`). Never use it to notify a Lead.
- **Two herdr/pi surprises `cpd` must handle:**
  1. A worktree with project-local `.pi/` resources makes pi show a **"Trust project folder?" dialog**. `agent start` still reported `interactive_ready:true`, so a typed prompt would land in the dialog. Pass `--approve`. This is a security decision, see Escalations.
  2. `ps -o args=` is useless for pi (it sets `process.title="pi"`). Take the argv from the `agent start` response (`.result.argv`) and verify with the session JSONL.
- **Speed.** A is slower to ready: `agent start` has a fixed 3 s settle delay in herdr (`AGENT_START_SETTLE_DELAY`), so ready is 3.0–3.6 s. B/C have the child's session file created 0.2–0.7 s after the tool call. This is irrelevant at Job granularity.
- **Why not B/C.** Both are pi-only, driven by an LLM tool call (~4 s Lead round trip per spawn, parameter-drift risk, one extra Lead turn per spawn). Both own worktrees, cap and lifecycle in ways that clash with `wt`, beads and `cpd`. C needs a global herdr plugin link. See "B/C coupling".

## Acceptance checklist

| # | Item | A | B | C | Evidence |
|---|---|---|---|---|---|
| 1 | **must** Launch reliability (A ≥10, B/C ≥5) | pass 14/14 | pass 5/5 (+7 other spawns, see F1) | pass 5/5 (+5 other spawns) | `spawn-a.log`, `spawn-b.log`, `spawn-c.log`; F1 |
| 2 | **must** Per-spawn model + thinking | pass (`:low`×13, `:medium`) | pass (`low`, `medium`) | pass (`:low`, `:medium` via `model` suffix) | session `thinking_level_change` + assistant `thinkingLevel`; argv via ps poll (F2) |
| 3 | **must** Worker in given worktree | pass | pass | pass | files in `wt/<n>`; session header `cwd` (realpath `/private/tmp/...`) (F3) |
| 4 | **must** Read-only forked Reviewer | pass | pass | pass | token quoted; system-patch `toolsRemoved`; `Tool bash not found` (A) (F4) |
| 5 | **must** Completion → Lead `custom_message` | pass, 24–329 ms | pass, 80–109 ms after child's last entry | pass, 13–33 ms after `subagent_done` | Lead JSONL `custom_message` entries (F5) |
| 6 | **must** Draft safety | pass (idle and busy Lead) | pass (demonstrated + code) | pass (demonstrated + code) | `pane read` before/after, `grep -c DRAFT`=0 (F6) |
| 7 | should Killed child | pass | pass | pass | F7 |
| 8 | should Escalation path | pass (question file + answer file) | pass (`caller_ping` → `subagent_resume`) | pass (same) | F8 |
| 9 | should Cleanup | pass | pass | pass (plugin unlinked) | F9 |

## Findings

### F1. Launch reliability and time to ready

**A.** `spawn-worker.sh <n> <low|medium>` ran this sequence per Worker:
- `git worktree add`;
- `pane split`;
- `agent start worker-<n> --kind pi --pane <id> -- --model github-copilot/claude-haiku-4.5:low`;
- `agent prompt worker-<n> "Create a file hello-<n>.txt containing <n> in the current directory, then stop." --wait --timeout 300000`;
- `pane close`.

14 sequential launches (n=1..14, n=5 at `:medium`, n=11–14 with wake notification), 14/14 had `hello-<n>.txt` with the right content.

| metric (ms, n=14) | min | mean | max |
|---|---|---|---|
| `pane split` | 38 | 43 | 74 |
| `agent start` → `interactive_ready` | 3041 | 3380 | 3569 |
| typed `agent prompt --wait` (Haiku trivial task) | 3238 | 3421 | 3690 |

- No `agent_not_ready`, no typing race and no trust dialog occurred in the 14 runs. The only readiness error was the one I provoked (F10).
- The ~3 s ready time is herdr's fixed settle delay (`/tmp/herdr-src/src/app/agents.rs:10`, `AGENT_START_SETTLE_DELAY = 3s`), not pi's startup.
- Extra A launches, not counted: worker-k, worker-k2 and worker-e (F7, F8), two `@file` variants (below) and two trust-dialog runs (F10).

Variant tested twice, **task as a pi `@file` argument**: `agent start w --kind pi --pane P -- --model …:low @/path/task.md`.
- No typing happens at all.
- `agent start` then blocks until the first turn completes, returning `agent_status:"done"` at 3.96 s for a 1 s task and 17.6 s for a 12 s task.
- The cap is `--timeout` ≤ 300000 ms, so for long tasks keep the typed-prompt path or catch the timeout and fall back to `agent wait`.
- Reliability of this variant is only n=2, so **unverified at scale**.

**B** (Lead-B `subagent` tool, exact params given in the prompt). 5/5 intended launches (n=21..25, 23 at `medium`) produced a correct file and a `subagent_result` message. Other spawns: w27 (draft test), w28 (completed after my kill attempt matched nothing), w29 (kill), w30 (ping), resume, fork Reviewer. One failure, my fault: w26 had a nonexistent `cwd` and was reported cleanly as `failed (exit code 1)` within ~6 s.

| metric | B n=21..25 | C n=41..45 |
|---|---|---|
| Lead prompt → Lead's ack turn done (`agent prompt --wait`) | 3.9–4.7 s | 3.7–3.9 s |
| `subagent` tool call → child session file created | 624–705 ms | 186–201 ms |
| tool call → tool returned `started` | 46–97 ms | 19–25 ms |
| child file written → `subagent_result` in Lead JSONL | 2.6–3.3 s | 1.5–3.0 s |

The last row includes the child's final LLM turn and exit, so it is not comparable to A's notify delay (F5).

**C** (`-e /tmp/cpd-trials/t1/modem-dev`, plugin linked). 5/5 intended launches (n=41..45, 44 at `:medium`). Other spawns: w47 (draft), w48 (kill), w49 (ping), resume, fork Reviewer.

`subagent` has **no `thinking` parameter** in C (`/tmp/cpd-trials/t1/modem-dev/index.ts:401-437`). Per-spawn level works only through the `model` string suffix, `model="github-copilot/claude-haiku-4.5:medium"`. Agent-def frontmatter `thinking` would be appended as a second suffix (`src/launch.ts:315`, `:375`).

### F2. Per-spawn model and thinking honoured

Session-file evidence is identical in shape for all options. Example A worker n=5 (`:medium`) and n=1 (`:low`):
```
{"type":"model_change","provider":"github-copilot","modelId":"claude-haiku-4.5"}
{"type":"thinking_level_change","thinkingLevel":"medium"}     # n=1: "low"
assistant messages: model "claude-haiku-4.5", thinkingLevel "medium"
```
B n=23 and C n=44 show the same (`medium`); B n=21 and C n=41 show `low`.

**`ps -o args= -p <pid>` does not work for pi 1.1.0.** pi sets `process.title = "pi"` (`dist/cli/setup.js:4`, `dist/bundle/cli-runtime.js`).
```
$ ps -o args= -p 92260            # A worker n=1
pi
$ python3 procargs.py 92260       # KERN_PROCARGS2: argv is overwritten too
argc 4 ['/usr/local/bin/node', 'pi', <env strings…>]
```
Substitute evidence:
1. **herdr echoes the argv it typed.** `agent start` returns `.result.argv`:
   `["pi","--fork",".../<lead>.jsonl","--tools","read,grep,find,ls","--model","github-copilot/claude-haiku-4.5:low"]`.
2. **Poll `ps` at 20 ms during startup** (before pi renames itself). The pid matches `pane process-info`, which gave pid 95752 for A n=5.
   ```
   A n=5   95752 node …/.bin/pi --model github-copilot/claude-haiku-4.5:medium
   B n=23   6110 node …/.bin/pi --session <s.jsonl> -e <pkg>/maestro/adapters/pi/child/subagent-done.ts --model github-copilot/claude-haiku-4.5 --thinking medium @<ctx>/w23-….md
   C n=44  17869 node …/.bin/pi --session <s.jsonl> -e …/modem-dev/subagent-done.ts --model github-copilot/claude-haiku-4.5:medium @<ctx>/w44-….md
   ```
3. The C launch script is on disk, and `pane process-info` shows `bash <artifacts>/subagent-scripts/w48-a361aa6c.sh`. It `exec`s `pi --session … -e …/subagent-done.ts --model github-copilot/claude-haiku-4.5:low @ctx`.

### F3. Worker runs in the given worktree

For A n=1..10 the file is in `/tmp/cpd-trials/t1/wt/<n>/hello-<n>.txt`, the worktree shows `?? hello-<n>.txt` in `git status`, and the session header `cwd` is `/private/tmp/cpd-trials/t1/wt/<n>`. `/tmp` is a symlink to `/private/tmp` on macOS: herdr/pi record the **realpath**.

B and C pass the `cwd` string as given, so their session directories are named `--tmp-cpd-trials-t1-wt-<n>--`, while A's are `--private-tmp-cpd-trials-t1-wt-<n>--`. The header `cwd` is the realpath in all cases. **`cpd` must not derive the session directory from `cwd` by string-encoding**: take the path from `agent_session.value`.

### F4. Read-only forked Reviewer

**A.** The Lead's first prompt contained `ZEBRA-7731`.
```
herdr agent start reviewer-a --kind pi --pane $P -- --fork <lead.jsonl> --tools read,grep,find,ls --model github-copilot/claude-haiku-4.5:low
```
- Start took 3.2 s.
- The Reviewer answered `ZEBRA-7731` and reported it has only read/grep/find/ls. `rev-write.txt` was not created.
- Fork session `…22-29-42-895Z_…jsonl`, header has `parentSession` = the Lead's file. System-message record:
  ```
  system  tools: [read,bash,edit,write]                    (inherited from the Lead)
  user "Remember this token: ZEBRA-7731…" / assistant "OK" (inherited conversation)
  system  toolsAdded:[grep,find,ls]  toolsRemoved:[bash,edit,write]   <- --tools applied
  ```
- The model did not attempt a write on its own, so I forced a call by name. The tool result was an error:
  `{"role":"toolResult","isError":true,"content":"Tool bash not found"}`.

**B** (`subagent` with `fork=true, tools="read,grep,find,ls"`, token `QUOKKA-5521`). The child quoted the token and listed read/grep/find/ls/`caller_ping`. System patch: `toolsAdded:[grep,find,ls,caller_ping] toolsRemoved:[bash,edit,write,worktree_*,subagent*…]`. No file was created. It completed by itself (auto-exit) in 5 s.

**C** (same call, `model="…:low"`). It quoted `QUOKKA-5521`. Tools: `grep,find,ls,caller_ping,subagent_done` plus `read`; `bash/edit/write/subagent*` removed. **Caveat:** in fork mode C does not append its "call `subagent_done` when finished" wrapper (`src/launch.ts:359`, `fullTask = params.task` when `inheritsConversationContext`). The child therefore stayed open and idle, the Lead never got a result (120 s), and I had to close the pane. The task text must say "call subagent_done" (or the agent def needs `auto-exit: true`).

### F5. Completion reaches the Lead, with delay

**A.** `spawn-worker.sh` runs `agent prompt --wait` to completion, then calls `notify-lead.sh`, which writes `{"kind","content","completedAtMs"}` to `inbox/<id>.json` atomically (tmp file + `mv`). `wake-ext.ts` (below) drains it via `pi.sendMessage`. The Lead's JSONL entry (n=11):
```
{"type":"custom_message","customType":"cpd","display":true,"timestamp":"2026-10-12T22:30:24.119Z",
 "content":"worker-11 finished: 11 (file_ok=yes)","details":{"completedAtMs":1791844223793,"deliveredAt":1791844224116,…}}
```
Delay completion → Lead entry, 4 samples: **323, 329, 24, 34 ms**. The first two include script overhead (`ps`, `jq` cold start), so call it 24–330 ms.
- 6 notifications produced exactly 6 `custom_message` entries (claim-by-rename makes delivery exactly-once even with duplicate `fs.watch` events).
- A file written *before* the Lead started was also delivered (`PRE-EXISTING: …` test with a fresh Lead): `drain()` at `session_start` works.
- When the Lead was **busy** (running `sleep 20`), the `followUp` waited for the turn to end: notified at 22:33:57, entry at 22:34:14.650, about 17 s later by design.

**B.** `subagent_result` entry in the Lead JSONL, 80–109 ms after the child's last session entry (n=21..23, autonomous child). Delivered with `sendMessage({customType:"subagent_result",…},{triggerTurn:true,deliverAs:"steer"})` (`pi-extension/subagents/index.ts:697-706`).

**C.** `subagent_result`, 13–33 ms after the child's `subagent_done` tool call (n=41..43). Delivered with `sendMessage(…,{triggerTurn:true,deliverAs:"steer"})` (`index.ts:378-391`).

### F6. Draft safety

**A, idle Lead.** `pane send-text w1:p1 "DRAFT-A-123"` (no Enter), then ran a Worker with notification:
```
visible before : line 37: DRAFT-A-123
visible after  : line 37: DRAFT-A-123   (below Lead's reply "Acknowledged.")
grep -c DRAFT <lead.jsonl>  → 0
user messages in JSONL      → only the original "Remember this token…" prompt
custom_message              → "worker-11 finished: 11 (file_ok=yes)"
```
**A, busy Lead.** The draft `DRAFT-A-BUSY-456` was typed while the Lead ran `sleep 20`, and the notification was dropped into the inbox mid-turn. After the turn the `custom_message` arrived, the draft was still at line 37, and `grep -c DRAFT` → 0.

**Negative control (why the hard rule exists).**
```
pane send-text lead-a "DRAFT-A-XYZ " ; agent prompt lead-a "NOTIFY: worker done. Reply with just ACK." --wait
user message in JSONL: "DRAFT-A-XYZ NOTIFY: worker done. Reply with just ACK."
```
`agent prompt` merges into the User's draft and submits it.

**B/C code reading plus demonstration.**
- B: `sendMessage` for results and pings. `sendUserMessage` only in the user-initiated `/subagent` and `/subagents-init` (`index.ts:3457`, `:3686`). `pane run` (typing) is used only to start the child's own pane (`maestro/surfaces/herdr/herdr.ts:1283`).
- C: `sendMessage` (`index.ts:378,384`). `sendUserMessage` only in the `/subagent` and `/iterate` commands (`:1120,1150`).
- Demonstrated for both: `DRAFT-B-123` / `DRAFT-C-123` typed while a 15–20 s child ran. `subagent_result` arrived (22:37:46 / 22:43:46), the draft stayed on the editor line, and `grep -c DRAFT` → 0 in both Lead JSONLs.

### F7. Killed child

All with `kill -9` of the child pi pid (from `pane process-info`).

| | A | B | C |
|---|---|---|---|
| What the Lead/`cpd` sees | `agent prompt --wait` returns **rc 0**, `agent_status:"idle"`, 273 ms after kill. **Looks like success.** `agent get` then stays `idle` ~0.5 s, and `agent_not_found` at ~1.1 s. Event stream: `pane_agent_detected {released:true,final_status:"idle"}` at +306 ms, then `agent_status_changed unknown` at +523 ms. `pane.exited` does NOT fire (pi runs under the pane's shell, which stays). | Lead entry `Sub-agent "w29" failed (exit code 137).` +session path, **5.2 s** after kill. Pane auto-closed ~5 s later. | Lead entry `Sub-agent "w48" failed (exit code 137).` + session path + last 20 pane lines, **341 ms** after kill. Pane **held open** ("subagent crashed (exit 137) — press Enter to close") with a stale herdr status `working`. I had to close it. |
| Discriminator `cpd` must use | `done.txt` absent; last assistant entry in the Worker's session has `stopReason:"toolUse"` (finished Workers end with `"stop"`); `released:true` event. | native | native |

`agent wait <name>` called right after a non-waiting `agent prompt` returned in 31 ms with `idle` (the turn hadn't registered as `working` yet). Always use `agent prompt --wait` (it requires an observed `working`, else `agent_prompt_stalled` after 5 s) and never a bare `agent wait` after a non-waiting prompt.

### F8. Escalation path

**A (question file → Lead → answer file → Worker continues).**
- `ask.sh <worker> "<q>"` writes a `question` message to the inbox via `notify-lead.sh`, then polls `answers/<id>.txt` once a second.
- Worker `worker-e` ran it from its bash tool.
- Lead entry at 22:32:55.397: `QUESTION from worker-e (id=q-1791844375-3658): Which color should answer.txt contain? -- answer by writing the answer text to …/answers/q-….txt`. The Lead's Haiku began investigating on its own turn.
- I wrote the answer at 1791844391516. The Worker finished 3.3 s later and wrote `answer.txt` = `teal`.
- Works because the Worker's bash tool blocks without a timeout. A Worker started with a restrictive tool timeout would need a different poll approach (**unverified**).

**B.** `caller_ping` tool (child-only). Child `w30` called it → the Lead received `custom_message` `subagent_ping` "Sub-agent "w30" needs help (2s): Which colour…?" + session path → I had the Lead call `subagent_resume(sessionPath, message:"…teal…")` → a new pane resumed the same session and wrote `answer.txt`=`teal`; the ping→resumed result roundtrip was 7.2 s. An ordinary child exits on ping. Persistent specialists stay alive and are answered via `subagent_send` (docs/tools.md:94-130; unverified, not run).

**C.** Same: `subagent_ping` entry (also includes a context-usage line), `subagent_resume` → `teal` in `wt/49/answer.txt`, roundtrip 6.3 s.

### F9. Cleanup

- `herdr plugin unlink pi-herdr-subagents` → `{"removed":true}`.
  - `herdr plugin list` → `No plugins installed.` (both plain and `--session cpd-trial`).
  - Linking created an empty `~/.config/herdr/plugins/config/pi-herdr-subagents`; I `rmdir`'d it and its two empty parents (they were created at 18:41 by my link).
- All workspaces I created (`t1`, `t1-trust`, `t1-drain`) closed. `workspace list` now shows only the other trials' w7, w8. `cpd-trial` server left running.
- No pi process with a `t1` cwd remains (checked via `lsof -d cwd`). Remaining pi processes belong to the User and other trials (cwds `~/projects/cpd`, `/tmp/cpd-trials/t4`, `t5`).
- Nothing written outside `/tmp/cpd-trials/t1`, pi's session store, pi's npm extension cache (`~/.pi/agent/tmp/extensions/npm/f35b2129`, created by `pi -e npm:`) and the herdr plugin registry (restored). `find ~/.pi/agent -newer` (excluding sessions/tmp) found nothing after the `--approve` run, so `--approve` persists no trust record.

### F10. herdr surprises (A)

- **Trust dialog.** A worktree containing `.pi/settings.json`, `.pi/extensions/noop.ts` and `AGENTS.md` made the new pi show "Trust project folder?" while `agent start` returned `{"agent_status":"idle","interactive_ready":true}` after 4.0 s.
  - Pane read: `Trust project folder? … → Trust / Trust parent folder / Trust (this session only) / Do not trust …`.
  - The next `agent prompt` would type into that menu.
  - With `-- --approve --model …` (pi: "Trust project-local files for this run"): no dialog, `AGENTS.md` and `noop.ts` loaded, the task ran. No trust record persisted (see F9).
  - The 14 A launches did not hit it because the scratch repo has no `.pi/`. Real repos with project `.pi/` or extensions will. `--no-approve` is the opposite choice; I did not verify what it does to `AGENTS.md` (**unverified**).
- **`agent_not_ready`.** `agent prompt` while `agent start` is still in flight → `{"error":{"code":"agent_not_ready","message":"agent worker-r is not an active named agent"}}`. Sequential `start` then `prompt` never hit it.
- **Names must be unique** (`DuplicateName`, `agents.rs:167`). `cpd` needs a unique agent name per Attempt.
- **`agent start` rejects argv containing control characters** (`agents.rs:~157-161`, newlines too). Long tasks go in a file via `@path`, or are typed with `agent prompt`.
- The `agent_session.value` is returned by `agent start` before the session file exists (it appears after the first message). Reading it early gives ENOENT.
- herdr pane ids are opaque, non-numeric (`w1:pA`, `w1:p1B`). `pane split` returns `.result.pane.pane_id`.
- B opens children in a separate herdr tab (`w1:tA`); C splits beside the Lead (`w1:t1`). Both auto-close the pane on clean exit. A's panes are wherever `cpd` splits them (here a right split of the Lead).
- macOS `date +%s%N` is unsupported (prints a literal `N`). Use `python3`/node for ms timestamps; the `cpd` CLI will be TypeScript, so not an issue.

## Recipes

### Spawn a Worker (A)

```bash
H="herdr --session $CPD_HERDR_SESSION"
git worktree add -b "$BRANCH" "$WT"                  # (t3: use wt)
PANE=$($H pane split "$LEAD_PANE" --direction right --cwd "$WT" --no-focus | jq -r .result.pane.pane_id)
$H agent start "worker-$JOB-$ATTEMPT" --kind pi --pane "$PANE" --timeout 60000 \
   -- --approve --model "github-copilot/claude-haiku-4.5:low"     # tier → model:effort; add --tools … if restricting
# argv herdr typed: .result.argv ; session path: .result.agent.agent_session.value
$H agent prompt "worker-$JOB-$ATTEMPT" "$TASK" --wait --timeout 300000   # blocks until idle/done/blocked
# completion check (NOT just rc 0): last assistant entry of the session has stopReason "stop"
jq -s -e '[.[]|select(.type=="message" and .message.role=="assistant")]|last|.message.stopReason=="stop"' "$SESSION"
$H pane close "$PANE"
```

Alternative with no typing: put the task in a file and pass it as an argument. `agent start` then blocks until the first turn ends (≤300000 ms `--timeout`):
```bash
$H agent start "$NAME" --kind pi --pane "$PANE" --timeout 300000 -- --approve --model "$MODEL" "@$TASKFILE"
```

### Spawn a Reviewer (A)

```bash
LEAD_SESSION=$($H pane get "$LEAD_PANE" | jq -r .result.pane.agent_session.value)
PANE=$($H pane split "$LEAD_PANE" --direction right --cwd "$LEAD_CWD" --no-focus | jq -r .result.pane.pane_id)
$H agent start "reviewer-$N" --kind pi --pane "$PANE" -- --fork "$LEAD_SESSION" --tools read,grep,find,ls --model "$MODEL"
$H agent prompt "reviewer-$N" "$REVIEW_BRIEF" --wait --timeout 300000
```
Verify the tool set from the fork's system patch: `toolsRemoved:[bash,edit,write]`. The `cwd` must be the Lead's, so the fork is in the same project.

### Dead Worker detection (A)

Subscribe over the herdr socket, `events.subscribe` with `pane.agent_status_changed` and `pane.agent_detected` for the pane; `released:true` in `pane_agent_detected` means the process died. Raw socket: `~/.config/herdr/sessions/<session>/herdr.sock`, request `{"id":"s","method":"events.subscribe","params":{"subscriptions":[{"type":"pane.agent_detected","pane_id":"w1:p9"}]}}`; each pushed line is one event. Do the read in a `select`/async reader, not a socket with timeouts. Always cross-check the artefact (stopReason / completion file).

### `wake-ext.ts` (working, loaded into the Lead with `-e`; 39 lines; `/tmp/cpd-trials/t1/wake-ext.ts`)

```ts
import * as fs from "node:fs";
import * as path from "node:path";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

// Watches $CPD_INBOX for *.json files written (atomically, via rename) by `cpd`,
// and delivers each as a custom message to the Lead. Never types into the editor.
const INBOX = process.env.CPD_INBOX ?? "/tmp/cpd-trials/t1/inbox";

export default function (pi: ExtensionAPI) {
	let watcher: fs.FSWatcher | undefined;

	const drain = () => {
		for (const f of fs.readdirSync(INBOX).filter((n) => n.endsWith(".json"))) {
			const src = path.join(INBOX, f);
			const claimed = `${src}.claimed`;
			try {
				fs.renameSync(src, claimed); // claim: exactly-once even with duplicate fs events
				const body = JSON.parse(fs.readFileSync(claimed, "utf-8"));
				pi.sendMessage(
					{ customType: "cpd", content: body.content ?? JSON.stringify(body), display: true, details: { ...body, deliveredAt: Date.now() } },
					{ triggerTurn: true, deliverAs: "followUp" },
				);
				fs.unlinkSync(claimed);
			} catch {
				/* raced with another event or a partial file: next event retries */
			}
		}
	};

	pi.on("session_start", () => {
		fs.mkdirSync(INBOX, { recursive: true });
		watcher = fs.watch(INBOX, drain);
		drain(); // pick up anything written before we started
	});
	pi.on("session_shutdown", () => {
		watcher?.close();
		watcher = undefined;
	});
}
```
Writer side (must be atomic, and dot-prefixed temp files are ignored by the `*.json` filter):
```bash
jq -n --arg kind "$1" --arg content "$2" --argjson at "$3" '{kind:$kind,content:$content,completedAtMs:$at}' > "$INBOX/.$id.tmp" && mv "$INBOX/.$id.tmp" "$INBOX/$id.json"
```
Gaps for production (not run): `session_shutdown` closing the watcher is untested (the pane was killed); the `catch {}` swallows malformed JSON silently and leaves a `.claimed` file behind, which violates "fail loud", so `cpd` should validate against a schema and report; per-Lead inbox directory (not shared) is needed if several Leads exist.

### Question/answer pair (A)

`ask.sh` (Worker side): writes a `question` message into the Lead inbox (same atomic write), then `while [ ! -s answers/$id.txt ]; do sleep 1; done; cat answers/$id.txt`. `cpd answer <id> <text>` just writes `answers/<id>.txt`.

### B/C launch call (for reference)

B: `subagent({name, model:"github-copilot/claude-haiku-4.5", thinking:"low", cwd:"<wt>", task:"…"})`, load `pi -e npm:pi-herdr-agents@3.2.1`.
C: `subagent({name, model:"github-copilot/claude-haiku-4.5:low", cwd:"<wt>", task:"…"})` (no `thinking`), `herdr plugin link …/herdr-plugin --enabled` (global) and `pi -e …/modem-dev`.

## Risks and unknowns

- **Trust dialog / `--approve` is a security decision** (Escalation 1).
- **Concurrency not tested.** A was sequential per the Spec; `cpd` will run several Workers in parallel on an 8 GB machine. Each pi was ~50–250 MB RSS (`ps` sample). Parallel `pane split` + `agent start` is unverified.
- **Reviewer fork cost.** `--fork` replays the Lead's whole conversation (a ~3 s start in this tiny session). Cost and latency with a long Lead session are unverified.
- **Long tasks.** `agent start --timeout` is capped at 300000 ms (`agents.rs:9`), so the `@file` variant cannot wait on tasks longer than 5 min. I did not test whether `agent prompt --timeout` has a cap (**unverified**). Fall back to a loop on `agent wait` (and mind its idle-before-working race, F7).
- **Completion proof.** The `stopReason:"stop"` check is a heuristic: a Worker that ends a turn politely without finishing the task also has `"stop"`. `cpd` still needs its own definition of done (a completion file or a verifiable artefact). Not tested.
- **herdr agent state can be stale.** After a kill in C, the held-open pane showed `working` forever. In A the agent is released after ~1 s, but the shell pane remains and must be closed by `cpd`.
- **pi version coupling.** `wake-ext.ts` uses `pi.sendMessage` with `deliverAs:"followUp"` (docs and types for pi 1.1.0). The session-JSONL shape (`custom_message`, `stopReason`, system `toolsRemoved`) is pi 1.1.0-specific.
- **B/C package freshness.** C's `package.json` still depends on `@mariozechner/pi-*` 0.65.x (deprecated names). It loaded and ran on pi 1.1.0 with and without `npm install` (no error loading without `node_modules`: **unverified that its tools registered**, since I did not call it in that run). The 166 MB `node_modules` comes from devDependencies.
- The Lead-LLM compliance with "call exactly this tool with these params" was 100% in my runs with Haiku, but the sample is small (~15 calls) and the prompts were deliberately mechanical.

## B/C coupling (what `cpd` would still have to do)

- **LLM-driven spawn path.** `cpd` cannot call `subagent` itself: the Lead's LLM must emit the tool call. That costs a Lead turn (~4 s, tokens) per spawn, and param drift, refusal or a wrong `cwd` becomes a spawn failure. The only structured record of the spawn is the Lead's tool call, not a `cpd` ledger row.
- **Result goes to the Lead, not to `cpd`.** B/C deliver to the Lead's LLM context. Nothing writes the ledger (beads) or enforces a state machine unless the Lead then calls `cpd`. `cpd` would have to scrape the Lead's session or the child session sidecars (`.exit`, `.exitcode` in C; `subagent_result` details in B).
- **Cap, timeout, metrics: all `cpd`'s job.**
  - C has no cap, no timeout and no stall detection (README "No stall detection (yet)").
  - B has `persistent.maxAgents` (default 3) only for persistent specialists and a stall watchdog for status. It has no per-Attempt hard timeout and no cap on ordinary children (I did not find one: **unverified**).
  - Token/cost metrics would be read from the child session JSONL either way.
- **Worktrees conflict.** B has its own Herdr-managed worktree handling (`worktree:{branch,base}`), and `subagent_resume` rejects managed-worktree sessions. C has none, which is fine with `wt`. `cpd` + `wt` already own worktrees and merges.
- **Reviewer.** Both support fork + `tools`, but B/C always add `caller_ping` (and C `subagent_done`) to the child's tools, so the Reviewer is not strictly "read/grep/find/ls". A gives exactly the list asked for.
- **Agent-agnostic fit.** A uses `herdr agent start --kind <any of 24 kinds>`. Only the wake extension is pi-specific. B and C are pi-only by design (C states non-goal for other CLIs).
- **Dependency weight.**
  - A: none (one 39-line file plus shell/TS calling the `herdr` CLI).
  - B: a ~1.2 MB npm package with a 3.9k-line `index.ts` (about 63k lines of repo including tests).
  - C: ~3.7k lines TS, git-only, needs a **global** herdr plugin link, and 166 MB of dev `node_modules` if installed.
- **Where B/C beat A.**
  - A dead child is reported as failure natively and in hundreds of ms (C) or ~5 s (B).
  - Launch is deterministic with no typing (C's argv plugin pane; B's `pane run` of a script).
  - Ready is ~0.2–0.7 s instead of ~3.4 s.
  - A can reproduce the first two cheaply: `@taskfile` argument gives no typing, and `released:true` plus the session `stopReason` gives death detection.

## Escalations

1. **Should Workers run with `pi --approve` (trust project-local `.pi/` and extensions without the dialog)?**
   - Why it blocks: it is a security posture choice, and the sub-test only needed to show the dialog exists and that `--approve` avoids it (done).
   - If **yes**: add `--approve` to the `agent start` args (as in Recipes). Workers will load whatever `.pi/` extensions and `AGENTS.md` the project's worktree has, i.e. the same code the User already trusts in that repo.
   - If **no**: choose `--no-approve` (ignore project-local files, including possibly `AGENTS.md`: I did not verify) or seed trust some other way (I did not find a non-interactive mechanism; `pi config` and `~/.pi` edits are out of scope here). In that case `cpd` must detect the dialog (`pane read` for `Trust project folder?`) before typing, because `agent start` does not flag it.
