# Durable-first-turn smoke (hs-n658, hs-d812)

Post-fix full-stack smoke of a fresh durable Plan Lead on a scratch `CPD_HOME`, with the first-turn fix
(`pi/durable-host/run.ts` reads `cpd session first-message` at startup for a fresh non-by-user Lead and
submits it as the first turn's input).

- Job `first-turn-verify`, Attempt `first-turn-verify-a1`, Plan `durable-first-turn`, Tier S1.
- Repo HEAD at smoke time: `fd8022c6537f22389577e7f867eebb5798ff85fe` (skeleton commit on top of the fix
  commit `cb775c413ee59addbaace9aabd7f851298bd7c3b`, "Run a fresh durable Lead's first turn at host
  startup").
- Overall: **PASS**. The first turn runs at birth with no manual input, one `kill -9` yields exactly one
  host restart, the mid-life `session send` is delivered, and the scratch home is gone with no process left.
- The real `~/.cpd` was **not written**. It was read only (`~/.cpd/config.toml` still has
  `lead_runner = "pi"`, mtime `2026-10-31 22:47:14 -0400`, before this smoke).

## Config used

Scratch `$S/config.toml` (all keys named in the Spec, plus a private tmux socket — see Finding A):

```toml
version = 1
machine = "smoke"
lead_runner = "durable"
pi_adapter = "/home/clayton/projects/cpd.durable-first-turn.first-turn/pi"
lead_tier = "H3"

[tmux]
socket = "cpdsmoke"
work_session = "cpdsmoke-work"
```

- `lead_runner = "durable"`, `lead_tier = "H3"`, `pi_adapter` = this worktree's `pi`.
- The scratch home's policy was a copy of `~/.cpd/policy/clayton-duarte/`, so
  `context new --owner clayton-duarte` resolves.
- The built binary is `target/debug/cpd` (`0.21.92-dev`, protocol 21), taken with the `scripts/tests.sh`
  pattern (`ls -t target/*/cpd | head -n 1`). Every `cpd` call below used that binary, resolved as `cpd`
  with `target/debug` first on `PATH`.

## Step log

### 1. Build — PASS

```
timeout 1800 scripts/cargo.sh build -p cpd        # Finished dev profile; exit 0
target/debug/cpd --version                        # {"protocol":21,"version":"0.21.92-dev"}
```

### 2. Scratch home + config — PASS

```
S=$(mktemp -d "${TMPDIR:-/tmp}/cpd-first-turn-smoke.XXXXXX")   # -> /tmp/cpd-first-turn-smoke.8RQrbw
cp -r ~/.cpd/policy/clayton-duarte "$S/policy/clayton-duarte"  # read-only copy
# $S/config.toml written as above
```

### 3. Context and Plan birth — PASS

```
cpd context new smoke --title Smoke --owner clayton-duarte
# {"ok":true,"context":{"id":"smoke","owner":"clayton-duarte",...}}
cpd plan new spl --context smoke --title "Smoke plan"
# {"ok":true,"lead":{"name":"spl","pane":null,"session":"1f09dc97-8217-494f-ac00-de7f0fc67dbc",...},
#  "plan":{"id":"spl","phase":"grounding","lead_session":"1f09dc97-...",...}}
```

`cpd session show spl` (before the engine ran): `runner=durable`, `by_user=false`, `role=lead`,
`tier=H3`, `model=github-copilot/claude-haiku-4.5`, `place=none`, `tmux_pane=null`, `state=stopped`.
The new Lead is durable with no pane.

### 4. Engine start — PASS

```
cpd watch --ensure
# {"ok":true,"pane":"%0","started":true}
```

Engine pid `3250058`, `/proc/3250058/exe -> .../target/debug/cpd` (the worktree binary; see Finding A).
Watch log: `cpd watch: started (pid 3250058), a tick every 5s` then `E16`/`E15`.

### 5. Host up, Session leaves stopped — PASS

Polled `cpd session show spl` and `$S/run/host/<lead>.pid`:

```
t=2s state=working pid=3250954
```

Host `3250954`: `node .../pi/durable-host/main.ts --session 1f09dc97-8217-494f-ac00-de7f0fc67dbc
--model github-copilot/claude-haiku-4.5:high` (the worktree's host).

### 6. First turn, no manual input — PASS

After `cpd plan new` alone, with no input of any kind, the Session went `stopped -> working -> idle`
(events `session-state` id 6 idle, 8 working, 13 idle), and the host's durable conversation received the
first message and ran the turn:

- host entry 7: kind `cpd-first-message` (the fix's own entry);
- host entries 12/17/…/57: `pi.assistant`;
- the first assistant output (entry 17): *"I'm reading the cpd-lead skill to follow the grounding
  phase. Let me get oriented:"*, then (entry 33) *"I'm grounding Phase: orienting to the Plan. …"*, then
  (entry 57) a grounding question.

`cpd session tail` (by the Session id) shows the transcript, whose first line is the submitted first
message and which includes the first assistant output:

```
user: You are the Lead of Plan spl "Smoke plan" in Context smoke. Phase grounding: follow the cpd-lead skill.
...
assistant: I'm reading the cpd-lead skill to follow the grounding phase. Let me get oriented:
```

Two command-shape caveats, both findings (below): `cpd session tail spl` fails (`session tail` wants the
Session id, not the Plan/name), and a tail taken immediately after birth renders the first message and
the tool/skill entries but not the assistant text; after the host restart (step 8) the assistant text
renders (`smoke2-tail5.txt` lines 298/310/385/391).

### 7. Mid-life `session send` — PASS

```
cpd session send spl "smoke: reply once" --unblocks smoke
# {"ok":true,"event":17,"from":null,"session":"1f09dc97-...","started":false,"unblocks":"smoke"}
```

- The `session-message` Event exists: `events` id 17,
  `Message for spl from the User (unblocks: smoke): smoke: reply once` (the sender is `null`, the User's side).
- The host delivered it: host entry 58 kind `cpd-event`,
  `{"id":17,"kind":"session-message","delivered_to":"1f09dc97-..."}`, model
  `user: Message from the User (23:47 EDT): smoke: reply once`; submission 59 `done`, requestId `event:17`;
  the Session went `idle -> working -> idle`.
- `cpd session tail <id>` shows `user: Message from the User (23:47 EDT): smoke: reply once`.

### 8. `kill -9` of the host — PASS, exactly one restart

```
old pid 3250954 (alive); kill -9 3250954        # 2026-10-31 23:49:23 EDT
t=4s NEW pid=3358053 (alive)
monitor 90s: pid stays 3358053, 0 further changes
```

The engine relaunched the host once. The first message was **not** replayed: after the restart the host
still holds exactly one `cpd-first-message` entry (id 7) and exactly one submission with requestId
`first-message:1f09dc97-...` (id 8, `done`). No restart loop.

### 9. Cleanup — PASS

```
kill -9 <host 3358053>
tmux -L cpdsmoke kill-server
rm -rf /tmp/cpd-first-turn-smoke.8RQrbw
```

No process of the smoke remains (checked `/proc/*/environ` for the scratch path: none), the scratch
directory is gone, and the private tmux server `cpdsmoke` is gone.

## Findings (observed, not fixed — out of scope)

- **A. `cpd watch --ensure` starts the engine from `PATH`, not from the invoking binary.** `--ensure`
  spawns `["cpd", "watch"]` in a tmux pane; the pane resolves `cpd` from the tmux server's `PATH`, so a
  run that used the worktree binary for the CLI still ran the *installed* `~/.local/bin/cpd` (0.22.93) as
  the engine. That installed engine migrated the scratch store past the worktree binary's schema and the
  worktree CLI then refused writes with `store_tampered` (`20261103T0400-job-addenda` unknown). Fixing
  this for the smoke: a private `[tmux] socket = "cpdsmoke"` in the scratch config, with `target/debug`
  first on `PATH`, so the engine pane also ran the worktree binary (verified via `/proc/<pid>/exe`). A
  smoke of a branch's engine should account for this.
- **B. `cpd session tail <name>` does not resolve a Plan or Session name.** `session tail` takes the raw
  `SessionId`; `cpd session tail spl` tries `run/host/spl.sock` and never connects. Other Session commands
  accept a `SessionRef` (a Plan id resolves to its Lead). Use the Session id with `session tail`.
- **C. Assistant text needs a populated snapshot.** A `cpd session tail` taken right after birth
  (empty snapshot, streamed message frames) rendered the first message and the tool/skill entries but no
  `assistant:` lines; the same tail after the host restart rendered them. The first turn itself had run
  (proved by the host's `pi.assistant` entries and the state machine), so this is a tail-rendering
  observation, not a missing turn.

## Reproduction notes

- Only `docs/research/durable-first-turn-smoke.md` was written in the worktree; no product code or tests.
- The real `~/.cpd` was read only (its policy copied, its config read); its `lead_runner` stayed `"pi"`.
- Evidence files used while running (config, host sqlite, tail captures) lived under `/tmp` and were
  removed with the scratch home.
