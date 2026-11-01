# Dead durable Lead: root-caused report (hs-n658, hs-d818)

## Answer (first, for the Lead)

**The sole confirmed root cause of the dead durable Lead is hs-n658: the durable host had no first-turn trigger.** At startup the only input source was the follower (`pi/durable-host/run.ts:685`); a fresh non-by-user Lead has no mapped Events after its cursor, and `pi/durable-host/turn.ts::prepareTurn`'s first-message branch is gated on `seat.byUser` (line 64), which is false for a Plan Lead. So no first turn ever started, and `run/<plan>/lead-message.md` stayed unconsumed. **Fixed by Plan durable-first-turn** (`hs-p162`, PR first-turn, now merged): `pi/durable-host/run.ts` now reads `cpd session first-message` at startup for a fresh non-by-user Lead (run.ts:417-431) and submits it as the first turn's input. The live smoke `durable-first-turn-smoke.md` **PASSES** on the merged fix — that is the proof the dead-Leads question is closed. **Candidate 2 (hs-d815) is falsified** (hs-d818, User decision D1): see §4. No second confirmed defect exists; the smoke findings are observations, not defects.

## 1. Scratch home used

Both smokes ran on a scratch `CPD_HOME` with the real home untouched:

- Scratch path: `$CPD_HOME/run/durable-smoke2/explore/scratch/` (smoke-a) and `.../scratch-b/` (smoke-b). Both were removed at cleanup. **measured**
- Config: a copy of the real home's `config.toml` with `lead_runner = "durable"`, an isolated tmux socket (`smoke-a-sock` / `smoke-b-sock`), and dedicated tmux session names. **measured**
- Policy dir: `$CPD_HOME/policy/` copied to `$SCRATCH/policy/` (a fresh home has no Context otherwise; `plan new --context` refuses `context_not_found`). **measured**
- No `cpd install`: the host was run from the repo's `pi/durable-host/main.ts`. **measured**
- Real home and engine untouched: `~/.cpd` read only; its `lead_runner` stayed `pi`. **measured**

## 2. Root cause: hs-n658 (the sole confirmed root cause)

**Component:** the durable host's first-turn path (`pi/durable-host/run.ts` + `pi/durable-host/turn.ts`).

**Function:** `runHost` (run.ts) and `prepareTurn` (turn.ts).

**File:line, confirmed on the current tree at the report's branch:**

- `pi/durable-host/run.ts:685` — `follower?.start();` is the last statement of `start()`. Before it, nothing submits a first input. **read**
- `pi/durable-host/run.ts:260` — the `seat` is built from the session row; `byUser: session.byUser` is false for a Plan Lead created by `plan new` (no `--by-user`). **read**
- `pi/durable-host/turn.ts:64` — `if (seat.byUser && !state.firstDone)` gates the first-message branch. For a Plan Lead (`byUser: false`), this is false and the branch is skipped. **read**
- `pi/durable-host/deliver.ts:21` — `inputOf(event, session, info, sender?)` is the only path from a follower event to an input; `pi/lib/delivery.ts:20` (`leadRule`) maps a follower Event for a plan Lead only if it is a `LEAD_MESSAGES` kind (`job-merged`, `job-failed`, `pr-feedback`, `plan-done`, `error`, `escalation`) or `session-message`. A fresh durable Lead has none of these after its cursor (`registered_event`), so `inputOf` returns `undefined` and `submit` is never called. **read**
- `engine/src/watch/hosts.rs::launch` — the durable host's argv is exactly `["<node>", "<file>/durable-host/main.ts", "--session", "<id>", "--model", "<pattern>"]`. **No first message.** **read** (Compare `core/src/host/pi/launch.rs` for the pi path, which appends `-- @<lead-message.md>`.) **read**

**Mechanism (inferred from the read code, matching the scratch smokes):**

1. The engine starts the host with no first message. **read**
2. The host opens the conversation and computes `cursor = cursorFrom(session.registeredEvent, ...)`. The only Events after registration are `session-state` (its own idle report) and `session-archived` — none is a `LEAD_MESSAGES` kind, so `inputOf` returns `undefined` and `submit` is never called. **read**
3. With no `submit`, `prepareTurn` never runs, and the first message (`run/<plan>/lead-message.md`, which `plan new` DID write — `plan/new.rs:125-126`) is never consumed by the host. **read**
4. The host reports `idle` and waits forever. **read**

**Why the first message file is unconsumed:** `plan new` writes `run/<plan>/lead-message.md` at `plan/new.rs:125-126` (`if !args.by_user { write_message(...) }`), and the scratch `run/<plan>/` held only that file, unconsumed. **read**

## 3. The fix (Plan durable-first-turn, `hs-p162`)

**File:line, confirmed on the current tree:**

- `pi/durable-host/run.ts:417-431` — the startup first-message block: if the Session is not by-user and the conversation holds no `cpd-first-message` entry for it, the host reads `cpd session first-message --session <id>` (run.ts:420) and submits it as an input whose `requestId` is `first-message:<id>` (run.ts:429) and whose entry kind is `cpd-first-message` (run.ts:431). **read**
- `pi/durable-host/turn.ts:64-76` — the `prepareTurn` first-message branch is unchanged but now finds the entry already stored (line 65) or, on a restart before the fix's entry was written, writes it itself. The `seat.byUser` gate (line 64) stays: a by-user Session's first message is stored by `prepareTurn`; a non-by-user Lead's is stored by `runHost` at startup. **read**

## 4. Candidate 2: hs-d815 — FALSIFIED (hs-d818, User decision D1)

**Status: falsified and superseded.** `delivery2.md` proposed hs-d815 as a submit state-leak in `pi/durable-host/run.ts` (~line 357). That code is **not in this repository's history at any commit near the report's date**; `git log -S` on `if (--pending === 0)` finds only #687 and #683. **read** (See `/tmp/cpd-durable-first-turn/hs-d815-falsified.md` for the full falsification record.)

What the source actually does (`pi/durable-host/run.ts`, same bytes on `origin/main` and the branch):

1. **The sync path already decrements.** `git blame` shows the `catch` decrement (`if (--pending === 0) { reply = false; changed("idle"); }`) came in commit `d55c00251` ("Serve the host socket...", #687, 2026-10-31 14:11 EDT), long before `delivery2.md` was written and present on the branch's base. `conversation.submit` throwing cannot leak `pending`. **read**
2. **`starts` gates only `prepareTurn`, not `conversation.submit`.** `conversation.submit(input)` runs for every input regardless; a turn still starts even if `starts` is false. Only the context update is skipped. The reported symptom ("prepareTurn is skipped and NO live turn runs") does not follow. **read**
3. **The only un-decremented path exits the host.** `wait()`'s rejection calls `end(1)`, which resolves the `ended` promise; `runHost` tears down and returns 1 (`pi/durable-host/main.ts` does `process.exit(await runHost(...))`). The host is not "live but stuck"; the process is gone, and the engine restarts it. **read**

**Consequence:** the prescribed fix (decrement `pending` in the catch block and in `wait()`'s rejection handler) is a no-op on this tree. **read** `hs-d815` must not appear as a live defect; it is recorded only as candidate 2, falsified. **record hs-d818.**

## 5. Smoke findings (scratch home, pre-fix)

### Slices

| Slice | What it tested | Verdict | Evidence |
|---|---|---|---|
| S1 | Lead birth | **PASS** | `cpd plan new scratch-a …` created Lead `7123c0df-…` with `runner=durable`, `role=lead`, `state=idle`. **measured** |
| S2 | Engine-started host | **PASS** | Host files appeared in `$SCRATCH/run/host/` within a few ticks; `ps` shows `node …/pi/durable-host/main.ts --session <id> --model …`. **measured** |
| S3 | First turn with no manual input | **FAIL (defect reproduced)** | `cpd session show` stayed `idle` for >15 s after creation; `cpd session tail` empty; host log only the Node experimental warning. **measured** |
| S4 | `session send` while live | **FAIL** | `session send` returned `{"ok":true,"event":17,"started":false}`; state stayed `idle`; the message rendered in the transcript (`user: Message from …`) but the host never processed it. **measured** |
| S5 | `session tail` | **PASS** | The transcript printed correctly (`user: Message from …`). **measured** |
| S6 | host kill and restart bound (§6.19) | **PASS** | One `kill -9` of pid 2803918 → new pid 2805497 within ~4 s (one engine tick); exactly one restart, no further; `state_label: null`, no `host crash loop`. A second kill (2805497 → 2874407) also stayed under the bound. **measured** |
| S7 | p1-7..p1-11 host operations (§6.14–§6.20) | **mostly PASS** | See the table below. **measured** |

### S7 operations table

| Op (contract) | Verdict | Evidence |
|---|---|---|
| not attached (§6.14/§6.17 r1) | **pass** | `error hello_required` before `hello`; `error not_attached` after `hello`, no `attach`. **measured** |
| ping (§6.14) | **pass** | `pong req=p1`. **measured** |
| configure (§6.17) | **pass** | `result {agent: {model: …, thinking_level: high}}`; `bad_args` when both `model` and `thinking_level` are given. **measured** |
| list_models (§6.17) | **pass** | `result {models: [430 entries]}` with the prescribed fields. **measured** |
| fork (§6.17) | **pass** | `bad_args` on a bad `at`; `refused` (`durable_session`) for a durable source. **measured** |
| history (§6.17) | **pass** | `result {entries: [1 entry: cpd-skills baseline, id 7]}`; `bad_args` on `limit:0`. **measured** |
| submit + images (§6.17) | **pass** | `result {submission_id: 10, status: "placed"}`; `host_state` → `working` → `idle`; `assistant: ok`. **measured** |
| compact (§6.17) | **pass** | `result {task_id: 14}`; `compaction_start` / `compaction_end` frames; compacted history after. **measured** |
| reload (§6.18) | **pass** | `result {reloaded: true}`. **measured** |
| skills baseline (§6.18) | **pass** | `cpd-skills` entry with digest data written at host start. **measured** |
| retier (§6.18) | **FAIL** | A live `session-retiered` (event 27) was never applied: no `configure`, no `cpd session switched`/`switch-failed`, minutes idle. **measured** |
| retier, replayed | **fail (same root)** | The restarted host replayed event 17 from cursor 5; no `switched`/`switch-failed` either, while the `session send` in the same replay **was** processed. **measured** |
| socket tail (§6.20) | **pass** | Plain transcript lines with role prefixes. **measured** |
| tail reconnect (§6.20 r4) | **partial** | Reconnect after a host kill+restart works (6 stderr lines then `snapshot … seq 1: view replaced`, streaming continued). **measured** The no-repeat rule **FAIL** (below). |

**6.20 tail no-repeat rule failure (§6.20 r4):** after a new-epoch snapshot the transcript was printed again in full on stdout. **measured** The contract says the stream must never repeat lines it printed. **read**

### Live-follower-delivery observation (not a second root cause)

**Observation:** a live `session send` (event 20, `"started": false`) sat unprocessed for minutes, and a live `session-retiered` (event 27) was never applied. **measured** After a host kill+restart, the new host replayed the follower backlog from its cursor and **did** process the pending Events: the queued `session send` got its turn (`session-state` events 21–23, `assistant: Received — S7 user message probe acknowledged.` in the tail). **measured**

**Classification:** this is the **then-broken-trigger symptom** of the same defect hs-n658 describes — delivery through the follower was dead while a host was live, and the startup-replay path worked. It is not a second confirmed defect, and it is superseded by the merged fix (the live smoke's step 7 shows a live `session send` being delivered post-fix). **inferred**

## 6. Live-turn proof (Plan durable-first-turn's `first-turn-verify` smoke)

**Merged, result in `docs/research/durable-first-turn-smoke.md` on `origin/main` (read via `git show origin/main:docs/research/durable-first-turn-smoke.md`).** Overall: **PASS**. **read**

The smoke proves on a merged scratch `CPD_HOME`:

- The first turn runs at birth with no manual input. **read** (`durable-first-turn-smoke.md` step 6: after `cpd plan new` alone, the Session went `stopped -> working -> idle`; the host's durable conversation received the first message (`cpd-first-message` entry id 7) and ran the turn: `pi.assistant` entries 12/17/…/57, the first output *"I'm reading the cpd-lead skill to follow the grounding phase. Let me get oriented:"*.)
- One `kill -9` yields exactly one host restart, no loop. **read** (step 8: old pid 3250954 killed, new pid 3358053 at t=4 s, 0 further changes over 90 s; the first message was **not** replayed — exactly one `cpd-first-message` entry and one submission with requestId `first-message:<id>`.)
- A mid-life `cpd session send` is delivered. **read** (step 7: event 17, `Message from the User …: smoke: reply once`, host entry 58 `cpd-event`, submission 59 `done`, Session went `idle -> working -> idle`.)

This is the proof that the dead-Leads question is closed.

## 7. Reproduction recipe

```bash
# 1. Scratch home
S=/home/clayton/.cpd/run/durable-smoke2/explore/scratch
mkdir -p "$S"
cp -r "$CPD_HOME/policy/" "$S/policy/"
# 2. Config: lead_runner=durable, isolated tmux socket, own tmux sessions
cat > "$S/config.toml" <<'EOF'
version = 1
machine = "smokea"
lead_runner = "durable"
[tmux]
socket = "smoke-a-sock"
cockpit_session = "smoke-a-cock"
work_session = "smoke-a-work"
EOF
# 3. A store-creating command first (a fresh home has no cpd.db; watch --ensure refuses otherwise)
env CPD_HOME="$S" cpd context new durable-smoke2 --owner clayton-duarte
# 4. Engine detached
env CPD_HOME="$S" cpd watch --ensure
# 5. Plan birth (real home and engine untouched; no cpd install)
env CPD_HOME="$S" cpd plan new scratch-a --context durable-smoke2 --title "…" --tier O3
# 6. Observe: the Lead stays idle forever (the dead durable Lead, pre-fix)
env CPD_HOME="$S" cpd session show <lead-id>
# 7. Cleanup
tmux -L smoke-a-sock kill-server
rm -rf "$S"
```

Every claim above is labelled **read**, **measured**, **inferred** or **unverified** inline. No second confirmed defect is claimed: hs-n658 alone is the root cause; the other observations are smoke findings, not defects.
