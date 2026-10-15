# t5: Structured questions to the User, and Worker → Lead Escalation

Date 2026-10-12, 18:42–19:00 EDT (about 18 min wall). Every agent was `github-copilot/claude-haiku-4.5:low`, run inside herdr 0.9.3 (session `cpd-trial`, workspace `t5`, closed at the end), pi 1.1.0.

| Thing | Version | Source |
|---|---|---|
| `@juicesharp/rpiv-ask-user-question` | 2.13.8 (`pi -e npm:@juicesharp/rpiv-ask-user-question@2.13.8`) | repo clone `juicesharp/rpiv-mono` @ `2d88b79c08ce19ffeecbe85d5ee1071046b708c0` |
| `pi-intercom` | 0.17.0 (`pi -e npm:pi-intercom@0.17.0`) | repo clone `nicobailon/pi-intercom` @ `14731a4873b500e19d000bf131142e430e4b7928` |
| cpd-native stub | written for this trial | `/tmp/cpd-trials/t5/{cpd-ask.sh,cpd-answer.sh,wake-ext.ts}` (full text in Recipes) |

## Verdict

- **Questions to the User: keep `rpiv-ask-user-question`.** It works over `send-keys` and returns structured answers in `details.answers` (JSONL excerpt below). Two caveats:
  - The Markdown preview is capped at 20 rows with no scrolling. A 40-line Plan shows "✂ 28 lines hidden". The approval dialog must carry a short summary (≤15 lines) and let plannotator hold the full Plan.
  - While the dialog is open, herdr reports the Lead as `working`, not `blocked`.
- **Escalation transport: recommend the `cpd`-native file transport (the stub, productised inside the `cpd` CLI).**
  - It works end to end, is draft-safe, and survives a Lead restart: the resumed Lead surfaced the pending question at `session_start` and answered it.
  - The Worker side is just a blocking CLI call, so Claude Code and Hermes Workers work unchanged.
  - It has no broker, no `tsx`/`esbuild` (about 10 MB), and no pi dependency on the Worker side.
  - The Lead-side pi extension is about 35 lines.
- **pi-intercom is a working alternative, not the pick.**
  - Round trip is fast (about 1.8 s, about 60 ms of it transport), and draft-safe.
  - `node cli.mjs ask` also works for non-pi senders (2.5 s).
  - Against it: a broker process that exits 5 s after the last session disconnects. A Lead restart or a broker kill loses the question, because the restarted Lead saw "No unresolved inbound asks". Killing the broker made the Worker's `ask` fail at once with "Disconnected while waiting".
  - It also injects an extra synthetic user message ("New intercom message above.") per delivery.
  - Its pending-ask files on disk (`pending-asks/*.json`) did not help recovery in any test I ran.
- **Both are draft-safe.** `DRAFT-intercom-123` and `DRAFT-stub-123` stayed in the Lead editor through the whole Lead turn, and `grep -c DRAFT` on the Lead session JSONL returned 0 in both runs. This holds because both go through `pi.sendMessage`, never terminal input.
- **Busy Lead:**
  - pi-intercom (`steer`) delivers at the next tool boundary and interleaves into the running turn without aborting it.
  - The stub (`deliverAs: "followUp"`) queues until the whole turn ends: 40 s sleep, surfaced 0.85 s after the turn ended.
  - Use `"steer"` in the stub if sooner delivery is wanted.
- **Environment deviation:** herdr panes were **120×20**, not 120×40. `pane layout` reports a 40-row area but `stty size` says `20 120`. I resized with `terminal session control --takeover --cols 120 --rows 40` (the resize persisted after the controller exited). Both sizes were tested.

## Acceptance checklist

| # | Item | Result | Evidence |
|---|---|---|---|
| 1 | **must** `ask_user_question`, 2 questions (single with preview, multi), driven by send-keys, JSONL answer | **pass** | Finding 1: JSONL `details.answers`; rendering and keys documented |
| 2 | **must** plan approval, 40-line Plan preview: render, scroll, usable | **partial** | Renders and is usable for approve/revise/reject with notes. The preview is hard-capped at 20 rows and does not scroll. See Finding 2 |
| 3 | **must** pi-intercom Worker `ask` → Lead `reply` → Worker continues; latency | **pass** | Finding 3: both session files, 1.8 s round trip |
| 4 | **must** draft safety, both options | **pass** | Finding 4: draft visible in `pane read` after the turn; `grep -c DRAFT` = 0 on both Lead JSONLs |
| 5 | **must** stub end to end | **pass** | Finding 5 |
| 6 | **should** Lead busy | **pass** | Finding 6 (both options) |
| 7 | **should** Lead restarted with `ask` pending (both) | **pass** | Finding 7. Stub: survives and Worker unblocks. pi-intercom: Worker stays blocked, question not re-surfaced, rescued only by a manual reply with the message id. Broker kill also tested |
| 8 | **should** Timeouts | **pass** | Finding 8. pi-intercom at 30 s (set via env); stub via `CPD_ASK_TIMEOUT` (tested from the shell, not through an agent) |

## Findings

### 1. `ask_user_question`, two questions, driven over send-keys

**Setup:** Lead pane `w8:p1`: `herdr --session cpd-trial agent start lead --kind pi --pane w8:p1 -- --model github-copilot/claude-haiku-4.5:low --session-dir /tmp/cpd-trials/t5/sessions-lead -e npm:@juicesharp/rpiv-ask-user-question@2.13.8`. The Lead was told exactly which two questions to ask: Q1 single-select "Node 22" (with a preview) vs "Bun"; Q2 multi-select pi / Claude Code / Hermes.

**Rendering (20 rows × 120 cols, preview side by side because width ≥100):**
```
 ←  □ Runtime   □ Adapters   ✓ Submit  →

 Which runtime should CPD target?

❯ 1. Node 22                                     ┌──────────────────────────┐
     Current LTS                                 │   const runtime = "node22"; │
  2. Bun                                         └──────────────────────────┘
↓
 Enter to select · ↑/↓ to navigate · n to add notes · Tab to switch questions · Esc to cancel · Ctrl+] to collapse
```
- It opens in the editor area, below the transcript.
- At 20 rows only two options are visible; a `↓` marker shows hidden content. It is cramped but workable.
- Multi-select rows render as `1. [ ] pi`, with the description on the next line.

**Keys that worked** (`herdr --session cpd-trial pane send-keys w8:p1 …`):

| Step | Keys |
|---|---|
| Q1 single: choose option 1 and advance to the next tab | `enter` |
| Q2 multi: toggle pi, move, toggle Hermes | `space down down space` |
| Q2 multi: go to the commit row | `down down` (the row shown as "Submit"; the docs call it "Next"), then `enter` |
| Submit tab ("Ready to submit your answers? 1. Submit answers / 2. Cancel") | `enter` |
| Add a note to the focused question | `n`, then `pane send-text "…"`, then `enter` (saves the note, does not submit) |
| Cancel everything | `esc` |

`pagedown` is not a valid herdr key name (`unsupported key pagedown`).

**Result in the Lead's JSONL** (`sessions-lead/2026-10-12T22-42-33-966Z_*.jsonl`, `toolResult`):
```json
{"toolName":"ask_user_question","isError":false,
 "content":[{"type":"text","text":"User has answered your questions: \"Which runtime should CPD target?\"=\"Node 22\". selected preview: ```ts\nconst runtime = \"node22\";\n```. \"Which agent adapters should v0 ship?\"=\"pi, Hermes\". You can now continue with the user's answers in mind."}],
 "details":{"answers":[
   {"questionIndex":0,"question":"Which runtime should CPD target?","kind":"option","answer":"Node 22","preview":"```ts\nconst runtime = \"node22\";\n```"},
   {"questionIndex":1,"question":"Which agent adapters should v0 ship?","kind":"multi","answer":null,"selected":["pi","Hermes"]}],
  "cancelled":false}}
```

**Herdr state:** `agent get lead` stayed `working` the whole time the dialog was open, so `agent prompt --wait --until blocked --until idle` timed out. Do not use herdr state to detect that the Lead is waiting on the User.

**Other limits (from `docs/tool-schema.md`, not all exercised):**
- 1–4 questions, 2–4 options each, header ≤16 chars.
- Preview only on single-select questions.
- Labels `Other`, `Type something.`, `Next` are reserved.
- Cancel returns the string "User declined to answer questions".
- A `Type something.` row is always appended.

### 2. Plan approval (approve / revise / reject, ~40-line Plan as preview)

**Setup:** `/tmp/cpd-trials/t5/plan.md`, 39 lines (20 numbered steps plus headings). The Lead was told to pass it verbatim as the preview of "Approve", with the short previews "Revise" and "Reject".

- **At 120×20:** the preview box shows 1 line plus the border, then `✂ ── 42 lines hidden ──`.
- **At 120×40** (after resizing the pane): the box shows about 20 rows and `✂ ── 28 lines hidden ──`. The Markdown rendering collapses headings (`# Plan…` shown as `Plan…`), and blank lines are kept, so the line count is higher than the source.
- **No scrolling:** the cap is in source (`view/components/preview/markdown-content-cache.ts:8-10`: `MAX_PREVIEW_HEIGHT_SIDE_BY_SIDE = 20`, `MAX_PREVIEW_HEIGHT_STACKED = 15`). The docs mention no preview scroll key, and `PageUp`/`PageDown` scroll the transcript, not the preview.
- **Revise with a note:** `down`, `n`, send-text `needs tests first`, `enter` (note saved), `enter` (select). Lead JSONL:
  `"Approve this plan?"="Revise". selected preview: Revise. user notes: needs tests first.`
- **Usable only as a decision gate.** Approve / Revise (with notes) / Reject works well. It is not usable for reading a Plan.

**For CPD:** use a short (≤12-line) Markdown summary as the preview of "Approve": goal, Job count, risks, and the plannotator link. The full Plan goes through plannotator.

### 3. pi-intercom: Worker `ask` → Lead `reply` → Worker continues

**Setup:** both panes started with `--name lead|worker -e npm:pi-intercom@0.17.0`. The Lead was told: on an intercom question, call `intercom reply "The button must be BLUE"`. The Worker was told: `intercom ask` the Lead, then write the colour to a file.

**Timeline (session JSONLs, UTC):**
```
worker  22:47:49.261  assistant toolCall intercom{action:ask,to:lead}
lead    22:47:49.310  custom_message intercom_message  (broker delivered 49.308, injected 49.309)
lead    22:47:49.312  role=user "New intercom message above."   <- synthetic steer, not typed input
lead    22:47:51.054  assistant toolCall intercom{action:reply,"The button must be BLUE"}
worker  22:47:51.066  toolResult "**Reply from lead:**\nThe button must be BLUE"
worker  22:47:52.425  bash: echo "blue" > answer.txt     -> answer.txt contains "blue"
```
- Round trip is **1.8 s**. Transport (ask → Lead has it) is 48 ms, and the reply reaches the Worker 12 ms after the Lead's tool call. About 1.7 s is the Lead's model turn.
- Non-pi sender: `node …/pi-intercom/cli.mjs ask --to lead --text "Is the modal dismissible?" --timeout-ms 60000 --json` returned `{"ok":true,"from":"lead","text":"yes"}` in 2.47 s. `list --json` works as well. This path needs the pi-intercom package on disk (node plus its `tsx`).

**State written by pi-intercom (all under `~/.pi/agent/intercom/`, created by the trial and removed at the end):**
- `broker.pid`, `broker.sock` (mode 0600)
- `extension-state/`
- `pending-asks/<messageId>.json` (askId, asker, target, question, createdAt, expiresAt = +10 min)
- No `config.json` was created.
- `PI_CODING_AGENT_DIR` would relocate this directory, but I did not use it.

Broker lifecycle:
- It is auto-spawned by the first session (`node --import …/tsx/dist/loader.mjs …broker.ts`).
- It exits 5 s after the last session disconnects (`broker/broker.ts:441-447`, 5000 ms timer). I confirmed it was gone about 25 s after both pi's quit, and `broker.pid` and `broker.sock` were removed.
- The package cache that `-e npm:` populated, `~/.pi/agent/tmp/extensions/npm/f35b2129/` (rpiv-ask-user-question, pi-intercom, pi-herdr-agents, tsx, esbuild), already existed at 18:34. It is shared with other trials, so I left it.

### 4. Draft safety

**Run:** with the Lead idle, `pane send-text w8:p1 "DRAFT-<option>-123"` (no Enter). Then the Worker raised an Escalation. After the Lead's turn: `pane read w8:p1 --source visible` and `grep -c DRAFT <lead.jsonl>`.

| Option | Draft after the Lead turn | `grep -c DRAFT` on Lead JSONL |
|---|---|---|
| pi-intercom | `DRAFT-intercom-123` still on the editor line | 0. The Lead's user messages were only my prompts plus two `"New intercom message above."` |
| stub | `DRAFT-stub-123` still on the editor line | 0. The Lead saw `custom_message` `cpd-escalation` with no user-role message |

The pi-intercom run left the Lead in a turn that asked me what to answer (my instruction covered only one question), and the Worker stayed blocked. The draft was untouched throughout.

**Why:** both use `pi.sendMessage` inside the extension, which never touches terminal input. This confirms the hard rule from research.

### 5. cpd-native stub, end to end

**Setup:**
- Lead: `-e /tmp/cpd-trials/t5/wake-ext.ts`, told to answer `cpd-escalation` messages by running the named bash command (colour questions → "green").
- Worker: plain pi (no extension) that runs `/tmp/cpd-trials/t5/cpd-ask.sh "<question>"` and writes the stdout to a file.

**Timeline:**
```
worker   22:52:03  bash cpd-ask.sh "Which colour should the logo be…"  (blocks, polling a.json every 0.5 s)
         escalations/1791845524-22828.q.json   {"id","question","from":"worker","createdAt":1791845524}
lead     22:52:04.652  custom_message cpd-escalation "Worker escalation 1791845524-22828 from worker: … Answer by running: …cpd-answer.sh <id> "<answer>""
lead     22:52:06.746  bash cpd-answer.sh 1791845524-22828 "green"   -> escalations/…a.json {"answer":"green","answeredAt":…}
worker   22:52:07.195  cpd-ask.sh returns "green"; answer-stub.txt = green
```
- Wake latency is under 0.7 s (fs.watch), unblock latency is at most 0.5 s (poll interval), and total is about 3.2 s, dominated by the Lead's model turn.
- Unlike pi-intercom, no synthetic user message is added.

### 6. Lead busy

- **Stub** (`deliverAs: "followUp"`): the Lead ran `sleep 40` (22:52:22 → 22:53:02). The Worker asked at about 22:52:29. The `cpd-escalation` message was injected at 22:53:02.941, 0.85 s after the turn ended. It queued, did not interrupt, and was answered right away.
- **pi-intercom** (default `busyDelivery: "steer"`): the Lead was running four sequential `sleep 10` calls. The Worker asked at about 22:54:25, during call A (22:54:21–31). `intercom_message` was injected at 22:54:31.225, immediately after call A's toolResult and before the next model call. The Lead replied, then went on to call B. It was **not aborted**, but the reply was interleaved into the running turn, about 6 s after the ask (the next tool boundary).
- With a single long tool call (`sleep 100`) the steer waited for the call to end (22:56:56.751), but delivery state was already reported as `injected`.
- For the stub, `steer` is available and delivers at the next tool boundary, same as pi-intercom.

### 7. Lead restarted while an ask is pending

**pi-intercom:**
1. The Worker's `ask` ("serif or sans?") was pending; the Lead had received it but not answered it.
2. I killed the Lead pi (two quick `ctrl+c`; the first `ctrl+c` only clears the editor, which also discarded the draft).
3. The Worker **stayed blocked** (`working`) while the Lead was down, because the broker stayed alive for the Worker's own connection. `pending-asks/a883aeb7-….json` was still on disk.
4. I resumed the Lead with `pi --session <id> --name lead -e npm:pi-intercom@0.17.0`. It was **not** re-surfaced: no new `intercom_message` in the JSONL, and `intercom {action:"pending"}` returned "No unresolved inbound asks."
5. A manual `intercom {action:"send", to:"worker", replyTo:"a883aeb7-57f7-4e35-9e22-3ccba7b49c5e", message:"sans"}` unblocked the Worker ("The font should be sans."). It works, but only if someone knows the message id; the old id is in the Lead's session history.

**pi-intercom, broker killed** (`kill $(cat ~/.pi/agent/intercom/broker.pid)` with an ask pending): the Worker's tool failed immediately with `Failed: Disconnected while waiting for reply: Client disconnected (2cc99ca3)`. Sessions reconnected and a new broker (new pid) started. The Lead still replied later, because the message had already been injected before the kill. The stale `pending-asks` file stayed on disk until its 10-minute expiry.

**Stub:**
1. The Lead was killed.
2. The Worker then started its ask (`1791845601-4560.q.json` written, Worker blocked).
3. The Lead was resumed (`--session <id>`) with the extension. `scan()` at `session_start` surfaced the question; the Lead answered "green" and the Worker unblocked, with `answer-stub3.txt` = green.

### 8. Timeouts

- **pi-intercom:** `PI_INTERCOM_ASK_TIMEOUT_MS=30000` exported in the Worker pane. The Lead was busy in `sleep 100`. At exactly 30 s (22:55:22 → 22:55:52) the tool returned `isError:true`:
  `Failed: No reply from "01a13252-…" for message 26bbde90-… within 30000ms. Last known delivery state: injected. This waiter timeout is not cancellation; the delivered message may still be queued or actionable in the recipient session.`
  The Lead did answer when its sleep ended, and the late reply arrived at the idle Worker as an ordinary `intercom_message` that triggered a new Worker turn ("lead replied: yes. The footer is fixed."). The env var is read by both the client (`index.ts:658`) and the broker (`broker.ts:256`), so setting it only in the Worker pane is enough for the Worker-side timeout.
- **Stub:** `CPD_ASK_TIMEOUT=2 ./cpd-ask.sh x` printed `cpd-ask: timeout after 2s (id …)` and exited **124**. In my first version the `.q.json` stayed behind, so a Lead restart would have re-surfaced a question nobody was waiting for. I added `rm -f` of the question on timeout. A late `cpd-answer.sh` then fails loudly (`no such question`). This was tested from the shell only; I did not check what a Worker agent's own bash tool timeout does (**unverified**).

## Recipes

### Questions to the User (`cpd-lead` skill)

Load with `-e npm:@juicesharp/rpiv-ask-user-question@2.13.8` (pin it). Rules for the skill:
- One `ask_user_question` call per interview round. Maximum 4 questions, 2–4 options each, header ≤16 chars, a question ending in `?`.
- Every option needs a one-line `description` giving the trade-off. Never author the labels `Other`, `Type something.` or `Next`.
- Use `multiSelect: true` only when picks are independent. Put `preview` on single-select options only, and keep previews short.
- Treat `details.cancelled === true` or the text "User declined to answer questions" as "stop and ask in chat".
- Read per-question notes from `user notes:` and the global note from `global note:`. They arrive inside the result text.

Plan approval call shape:
```ts
ask_user_question({questions:[{
  header:"Plan", question:"Approve this plan?",
  options:[
    {label:"Approve", description:"Proceed to dispatch", preview:"<≤12-line Markdown summary: goal, N Jobs, risks, plannotator URL>"},
    {label:"Revise",  description:"Send back with notes (press n to add notes)", preview:"Revise"},
    {label:"Reject",  description:"Discard the plan", preview:"Reject"}]}]})
```
Do not rely on a non-empty preview for Revise and Reject; any short string works. In `cpd`, do not use herdr `blocked` to detect "Lead awaiting the User" (it reads `working`).

### Driving the dialog in tests

```
herdr --session cpd-trial pane send-keys <pane> enter                       # single-select: select and advance
herdr --session cpd-trial pane send-keys <pane> space down down space       # multi-select toggles
herdr --session cpd-trial pane send-keys <pane> down down enter enter       # commit row, then Submit answers
herdr --session cpd-trial pane read <pane> --source visible                 # read the dialog
# get a real 120x40 pane (panes start at 120x20):
(printf '{"type":"terminal.resize","cols":120,"rows":40}\n'; sleep 3) | \
  herdr --session cpd-trial terminal session control <pane> --takeover --cols 120 --rows 40 >/dev/null
```

### Escalation: cpd-native transport (verified in this trial)

Files (queue directory `escalations/`, atomic publish with tmp+`mv`):
```bash
# cpd-ask.sh  (Worker side; agent-agnostic: any agent that can run bash)
#!/usr/bin/env bash
set -euo pipefail
DIR="${CPD_ESC_DIR:-/tmp/cpd-trials/t5/escalations}"; TIMEOUT="${CPD_ASK_TIMEOUT:-600}"
[ $# -eq 1 ] || { echo "usage: cpd-ask.sh <question>" >&2; exit 2; }
mkdir -p "$DIR"; ID="$(date +%s)-$RANDOM"
jq -n --arg id "$ID" --arg q "$1" --arg from "${CPD_FROM:-worker}" --argjson t "$(date +%s)" \
  '{id:$id,question:$q,from:$from,createdAt:$t}' > "$DIR/.$ID.q.tmp"
mv "$DIR/.$ID.q.tmp" "$DIR/$ID.q.json"
END=$(( $(date +%s) + TIMEOUT ))
while [ ! -f "$DIR/$ID.a.json" ]; do
  [ "$(date +%s)" -lt "$END" ] || { rm -f "$DIR/$ID.q.json"; echo "cpd-ask: timeout after ${TIMEOUT}s (id $ID)" >&2; exit 124; }
  sleep 0.5
done
jq -r .answer "$DIR/$ID.a.json"
```
```bash
# cpd-answer.sh <id> "<answer>"  (Lead runs it; errors if the question is missing or already answered)
#!/usr/bin/env bash
set -euo pipefail
DIR="${CPD_ESC_DIR:-/tmp/cpd-trials/t5/escalations}"
[ $# -eq 2 ] || { echo "usage: cpd-answer.sh <id> <answer>" >&2; exit 2; }
[ -f "$DIR/$1.q.json" ] || { echo "no such question: $1" >&2; exit 1; }
[ ! -f "$DIR/$1.a.json" ] || { echo "already answered: $1" >&2; exit 1; }
jq -n --arg id "$1" --arg a "$2" --argjson t "$(date +%s)" '{id:$id,answer:$a,answeredAt:$t}' > "$DIR/.$1.a.tmp"
mv "$DIR/.$1.a.tmp" "$DIR/$1.a.json"; echo "answered $1"
```
In the `cpd` build, replace these with `cpd ask` and `cpd answer` subcommands (TypeScript) over the same file protocol, and keep the Worker side free of pi-specific code.

Lead-side pi extension (`/tmp/cpd-trials/t5/wake-ext.ts`, the complete minimal surface):
```ts
import * as fs from "node:fs";
import * as path from "node:path";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

const DIR = process.env.CPD_ESC_DIR ?? "/tmp/cpd-trials/t5/escalations";
const ANSWER = process.env.CPD_ANSWER_CMD ?? "/tmp/cpd-trials/t5/cpd-answer.sh";

export default function (pi: ExtensionAPI) {
	const surfaced = new Set<string>();
	function scan() {
		for (const f of fs.readdirSync(DIR)) {
			const m = /^(.+)\.q\.json$/.exec(f);
			if (!m || f.startsWith(".")) continue;
			const id = m[1];
			if (surfaced.has(id) || fs.existsSync(path.join(DIR, `${id}.a.json`))) continue;
			surfaced.add(id);
			const q = JSON.parse(fs.readFileSync(path.join(DIR, f), "utf-8"));
			pi.sendMessage(
				{ customType: "cpd-escalation",
				  content: `Worker escalation ${id} from ${q.from}: ${q.question}\nAnswer by running: ${ANSWER} ${id} "<answer>"`,
				  display: true, details: { id } },
				{ triggerTurn: true, deliverAs: "followUp" },   // "steer" = deliver at next tool boundary
			);
		}
	}
	let w: fs.FSWatcher | undefined;
	pi.on("session_start", () => { fs.mkdirSync(DIR, { recursive: true }); scan(); w = fs.watch(DIR, () => { try { scan(); } catch {} }); });
	pi.on("session_shutdown", () => w?.close());
}
```
That is: `session_start` and `session_shutdown` handlers, `fs.watch` plus an initial scan for durability, and one `pi.sendMessage` call. Nothing else is required. Optional polish: `registerMessageRenderer` for a nicer transcript row. Replace the bare `JSON.parse` with schema validation at the boundary, per project rules.

Launch the Lead with it: `pi --name lead -e <path>/wake-ext.ts`.

### pi-intercom (if chosen instead)

`-e npm:pi-intercom@0.17.0` on both sides, `--name lead|worker`.
- Worker: `intercom({action:"ask", to:"lead", message:"…"})`. Env `PI_INTERCOM_ASK_TIMEOUT_MS` (default 600000).
- Lead: `intercom({action:"reply", message:"…"})`.
- Non-pi sender: `node <pkg>/cli.mjs ask --to lead --text "…" --timeout-ms 60000 --json`.

## Risks and unknowns

- **Pane size:** the trial server's panes were 120×20, not 120×40 (see Verdict). Anything that sizes the `ask_user_question` dialog (preview rows, visible options) depends on this. `cpd` should set the headless size itself (`headless_cols/headless_rows` in config, or `terminal session control`) rather than trust the default. I did not test the `headless_rows` config key.
- **Preview cap is a hard 20 rows, 15 when stacked** (width <100). No scrolling means Plans cannot be reviewed inside this dialog.
- **Herdr status** stays `working` while the dialog is open. The `cpd` UI cannot use herdr to tell that the Lead is waiting on the User.
- **Haiku followed the structured-question prompts precisely.** I did not test less-explicit prompts, so the skill wording above is untested with a lower-instruction prompt.
- **Stub gaps, not built or tested:**
  - No routing between multiple Leads sharing one queue directory (all would surface the same question). `cpd` should key the directory per Lead Branch or Lead session.
  - A Worker that crashes leaves `.q.json` behind, which gets re-surfaced on every Lead restart. It needs a TTL or an owner-pid check.
  - There is no pending-asks view for the Lead beyond the session history.
  - Poll interval 0.5 s on the Worker side.
  - It only works for same-machine panes and a shared filesystem.
  - Whether a Worker agent's own bash tool kills `cpd-ask.sh` after its default timeout is **unverified**.
- **pi-intercom gaps:**
  - It is the heavier dependency: `tsx` (about 480 KB) plus `esbuild` (about 10 MB) and 5.2 k lines in `index.ts` and `broker.ts`.
  - `pending-asks/*.json` is written, but nothing in my tests read it for recovery. I did not read the broker code that consumes it, so I cannot say what it is for.
  - Only one pending `ask` per session (README).
  - The broker is shared by every pi session on the machine under `~/.pi/agent/intercom` (outside `/tmp`, hence the cleanup).
- The Lead's Haiku-level instruction-following was inconsistent in one run: it declined to answer a second question because my standing instruction covered only one. That is a prompt issue, not a transport issue.

## Escalations

None. No install, credentials or out-of-scope action was required.

Notes on isolation:
- Only `~/.pi/agent/intercom/` was created by the trial (by pi-intercom), and I removed it. It did not exist before.
- No `~/.config/rpiv-ask-user-question` was created.
- Pi session files went to `/tmp/cpd-trials/t5/sessions-*` via `--session-dir`.
- I closed only my workspace `w8`. At the end `herdr workspace list` returned no workspaces; the earlier `t1` and `t4` were already gone before I looked again, and I did not close them.
- All pi's, the broker and watchers were stopped. The `cpd-trial` server was left running.
