# Durable Lead smoke on pi-durable 1.5.0 (host protocol 2)

Recorded smoke of fresh durable Plan Leads on a scratch `CPD_HOME`. It follows `docs/research/durable-first-turn-smoke.md`
(pi-durable 1.3.0; first turn, `session send`, `kill -9` only).

- Job `smoke-docs`, Attempt `smoke-docs-a1`, Plan `eval-durable-leads`, Tier S1. Run on 2026-11-05, 16:35-16:53 EST.
- Binary: the worktree's `target/debug/cpd` (`ls -t target/*/cpd | head -n 1`, built with `scripts/cargo.sh build -p cpd`): `{"protocol":22,"version":"0.22.112-dev"}`. It was first on `PATH` for every call, so the engine pane also ran it.
- Overall: steps 1-8 **PASS**, with four findings (none blocks).
- The real `~/.cpd` was not written (`~/.cpd/config.toml` mtime `1793888698` before and after). `cpd install` was not run. The scratch home, the tmux socket and every process of the smoke are gone (step 8).
- Models: Leads ran `lead_tier = "H3"` (`github-copilot/claude-haiku-4.5`, thinking `high`), per the Spec.

## Version and protocol

From the installed package:

```
$ grep '"version"' node_modules/@earendil-works/pi-durable/package.json
	"version": "1.5.0",
```

From the host's welcome frame, read over `run/host/<session>.sock` by the node client (hello `protocol 2, min_protocol 1, client "cpd-web"`):

```
{"type":"welcome","protocol":2,"host_version":"1","pi_durable":"1.5.0","session":"9d291e14-0bf4-4fd5-a37f-4ff6120a4979","role":"lead","epoch":"c3f1025c-abab-4bdc-9f25-9a7787f16f35","client_id":"c1","cwd":"/Users/Clayton.Duarte","limits":{"client_line":1048576}}
```

The package says 1.5.0, the welcome says `pi_durable` 1.5.0 and `protocol` 2. That pair is in `SUPPORTED_HOSTS` (`web/src/server/durable/frames.ts`: `{ protocol: 2, piDurable: "1.5.0" }`), so `assertSupportedHost` accepts it.

## Setup

```
S=$(mktemp -d /tmp/cds.XXXXXX)          # short path: a unix socket path is limited to 104 bytes on macOS
cp -R ~/.cpd/policy/clayton-duarte $S/policy/       # read-only copy
# $S/config.toml:
version = 1
machine = "smoke"
lead_runner = "durable"
pi_adapter = "<this worktree>/pi"
lead_tier = "H3"
worker_tier = "H2"        # needed: see finding F1
[tmux]
socket = "cdsmoke"        # private tmux socket
work_session = "cdsmoke-work"
export CPD_HOME=$S PATH=<worktree>/target/debug:$PATH; unset CPD_PLAN CPD_SESSION TMUX
cpd context new smoke --title Smoke --owner clayton-duarte
cpd watch --ensure       # {"ok":true,"pane":"%0","started":true}
```

The client script is `$S/c.mjs` (scratch dir, deleted at the end, not a repo file): a plain `node:net` client speaking the host's JSON-lines protocol as `web/src/server/durable/client.ts` does (`hello`, `attach {conversation:"root"}`, `history`, `abort`, `ui_response`).

## Steps

### 1. Fresh Lead with a brief runs its first turn at birth, no manual input — PASS

```
cpd plan new p1 --context smoke --title "Smoke 1" --brief-file $S/brief.md
# {"ok":true,"lead":{"name":"smoke:p1","pane":null,"session":"9d291e14-0bf4-4fd5-a37f-4ff6120a4979",...},"plan":{"id":"p1","phase":"grounding",...}}
cpd watch --ensure; sleep 10; cpd session show p1
# "by_user":false ... "runner":"durable","state":"working"   (then "state":"idle")
```

The host process came up (`node .../pi/durable-host/main.ts --session 9d29... --model github-copilot/claude-haiku-4.5:high`) and the engine reported `Session smoke:p1 is working` then `is idle` (events 14, 23). The attach replay of the ring (step 4) shows the whole turn: ops `["s",["docs","pi.live","run"],{"taskId":9,"inputs":[8],"turns":0,"first":7}]`, a `cpd-first-message` entry (id 7), `read` and `bash` tool calls, and the final text `Hello! Plan p1 "Smoke 1" is in grounding with no Jobs yet.` The Lead ran the turn with no input after `plan new`.

### 2. by_user=1 Plan starts without a turn; `session send` first prompt starts it — PASS

```
cpd plan new p2 --context smoke --title "Smoke 2" --by-user      # "by_user":true, state idle
node c.mjs run/host/0a000bdf-....sock '[]'
# +2ms attached ... head_seq":2 ; ops seq=2 [["p",["entries"],0,0,[{"kind":"cpd-skills",...
sqlite3 run/host/0a000bdf-....sqlite "select count(*) from entries"      # 1  (only the cpd-skills entry; no run)
cpd session send p2 "Reply with exactly: PONG2" --unblocks smoke-step2
# {"ok":true,"event":28,"from":null,"session":"0a000bdf-...","started":false,"unblocks":"smoke-step2"}
```

Before the send: one entry (`cpd-skills`), no `pi.live.run`, state `idle`. After it, `entries` hold `9 cpd-first-message` (the Lead's header), `11 cpd-event` (`Message from the User (16:36 EST): Reply with exactly: PONG2`), `15 pi.assistant` `PONG2`; submissions `8` (`first-message:0a00...`) and `10`, `12` (`event:28`) all `done`; events 29/30 `working`/`idle`. The first prompt started the first turn. The reply says `"started":false` although a turn began (F2).

### 3. `session send` mid-life while idle is delivered; a send while a turn runs is queued as followUp — PASS

```
cpd session send p2 "Mid-life idle send: reply with exactly: PONG3" --unblocks smoke-step3a
# {"ok":true,"event":32,...,"started":false,...}
# entries: 16 cpd-event "Message from the User (16:37 EST): Mid-life idle send..." ; 19 pi.assistant "PONG3" ; submission 17 "event:32" done
```

Busy case, on p1 with a client watching the ops stream:

```
cpd session send p1 "Use the bash tool to run exactly: sleep 20; echo LONG-DONE. Then reply LONG-DONE." --unblocks step3b   # event 37
# (bash tool status "running")
cpd session send p1 "Second message while busy: also say SECOND-SEEN at the end." --unblocks step3c                  # event 39
# +9150ms ops seq=143 ["p",["docs","pi.inbox","items"],0,0,[{"id":29,"mode":"followUp","content":"Messag...
# entries: 26 cpd-event(first) 30 pi.assistant 32 pi.tool-result "LONG-DONE\n" 34 pi.assistant "LONG-DONE"
#          35 cpd-event(second) 37 pi.assistant "SECOND-SEEN"
```

The second send was **queued as `followUp`** (inbox item `mode: "followUp"`). It did not steer: the running turn finished first (`LONG-DONE`), then the queued message ran as its own turn (`SECOND-SEEN`, entry 35 after 34). The inbox item was removed (`["p",["docs","pi.inbox","items"],0,1,[]]`) when it was taken.

### 4. A host client reads ring/history and a transcript frame over `run/host/<session>.sock` — PASS

```
node c.mjs run/host/9d29...sock '[{"type":"history","req":"h1","limit":5}]'
+1ms {"type":"welcome","protocol":2,"host_version":"1","pi_durable":"1.5.0",...}
+2ms {"type":"attached","req":"a1","mode":"view","epoch":"c3f1025c-...","from_seq":1,"head_seq":95,"client_id":"c1"}
+2ms {"type":"view","epoch":"c3f1025c-...","seq":1,"view":{"conversation":{"id":1},"entries":[],"docs":{"pi.agent":{"model":{"provider":"github-copilot","modelId":"claude-haiku-4.5"},"thinkingLevel":"high",...
+2ms {"type":"ops","epoch":"c3f1025c-...","seq":2,"ops":[["s",["docs","pi.live","run"],{"taskId":9,...
 ... ops seq 3..95 (ring replay of the whole first turn: entries, live generation deltas, tool slots) ...
+6ms {"type":"host_state","state":"idle","name":"smoke:p1","clients":1,"held_ui":0}
+6ms {"type":"result","req":"h1","data":{"entries":[{"model":[{"role":"assistant","content":[{"type":"thinking",...
```

The client's `hello` used protocol 2, min 1, name `cpd-web`, the same as `client.ts` (`PROTOCOL = 2`, `MIN_PROTOCOL = 1`, `CLIENT_NAME = "cpd-web"`). The host answered `welcome`, `attached` (mode `view`, `head_seq` 95), a `view` frame at seq 1, 94 `ops` frames (the ring) in order, a `host_state`, and the `history` result with entries. The `view` and `ops` frames carry the transcript (`entries`, `docs.pi.live`, `docs.pi.inbox`, `docs.pi.agent`), the shapes `frames.ts` (`createViewDialect`) folds. This smoke did not run `frames.ts` itself over them: the node script only checked frame order and shape.

### 5. `kill -9` of the host: exactly one restart, state from the live run, transcript continues — PASS

```
old pid 40957 (idle, 37 max entry id, 16 entries)
kill -9 40957      # 16:38:41 EST
t=1..4  pidfile 40957 (dead), state "idle"
t=5     pidfile 73217 (alive), state "idle"
15 s later: pidfile 73217; `ps` shows one durable-host process for the session
```

Exactly one restart (pid 73217); the pid then stayed stable. The session state was `idle` before, during and after, reported from the live host (not stuck `working`). After the restart `cpd session send p1 "post-kill: reply exactly PONG5"` (event 44) was delivered and answered: `max(entries.id)` went 37 to 42, `count` 16 to 19, submission `event:44` `done`, and `pi.assistant` `PONG5` once. `run/host/<session>.sqlite` continued, with exactly one `cpd-first-message` entry (no replay of the first turn). Events: `idle`(40) `working`(45) `idle`(47).

### 6. `ask_user_question` and `cpd_plan_review` over the host: ui_request, then ui_response unblocks — PASS (with one model-side note)

`ask_user_question` (p2, Tier H3 model, trivial prompt):

```
cpd session send p2 "Call the ask_user_question tool once with one question (header Smoke, question 'Pick one?', options Alpha and Beta...)" --unblocks step6a
+939ms  host_state working
UI_REQUEST_FULL {"questions":[{"header":"Smoke","question":"Pick one?","options":[{"label":"Alpha",...},{"label":"Beta",...}]}],"type":"ui_request","id":"bb729f16-...","method":"questionnaire","purpose":"ask_user_question","created_at":...}
+2997ms host_state blocked  held_ui 1
SENT ui_response {"answer":{"answers":[{"questionIndex":0,"question":"Pick one?","kind":"option","answer":"Alpha"}],"cancelled":false}}
+4498ms host_state working  held_ui 0
+4500ms {"type":"ui_resolved","id":"bb729f16-...","outcome":"answered","by":"c4"}
+5219ms host_state idle
# entries: 25 pi.tool-result "User has answered your questions: "Pick one?"="Alpha"..." ; 27 pi.assistant "Alpha"
```

`method` is `questionnaire`, which `dialogFromRequest` (`frames.ts`) maps to `ask_user_question`. The state went `blocked` while the ui was held and back to `working` on the response.

`cpd_plan_review` (p2 first moved to phase `plan` with a docs Workflow `w1` on a scratch repo, one Job with a Spec, an outline):

```
+8931ms {"title":"Approve the plan review","options":["Approve","Change"],"type":"ui_request","id":"f4aac79c-...","method":"select","purpose":"plan_review","created_at":...}
+8931ms host_state blocked  held_ui 1
SENT ui_response {"value":"Approve"}
+10433ms host_state working  held_ui 0
+10434ms {"type":"ui_resolved","id":"f4aac79c-...","outcome":"answered","by":"c6"}
+11426ms host_state idle
+11426ms OPS ... ["p",["entries"],36,0,[{"kind":"cpd-plan-decision","data":{"plan":"p2","review":"r1","approved":true,"current":true},...
```

`method:"select"`, `purpose:"plan_review"`, options `["Approve","Change"]` is exactly the shape `dialogFromRequest` maps to `cpd_plan_review`. `cpd plan show --plan p2` then reported review `r1` `status:"approved"`. No provider failure occurred in any call. A first `cpd_plan_review` attempt (before the Plan had its outline) raised no ui_request: the tool refused with the CPD error (`outline_missing`/`draft_incomplete`), as it should. The model then called the tool a second time after the decision (task 71, visible in the ops); the loop is the model's, not the host's.

### 7. Abort of a running turn through the host protocol; in-flight bash tool call — PASS (resolves the unverified abort question)

First attempt: `sleep 117` was not run; the model refused it (the Lead adapter's bash guard allows waits of 30 s or less, so the model wrote "The adapter refuses bash calls that wait longer than 30 seconds"). Second attempt, `sleep 28`:

```
cpd session send p1 "Use the bash tool to run exactly: sleep 28. Then reply ABORT-NOT-SEEN." --unblocks step7     # event 99
t=3s  ps: 12443 (ppid 73217 = the host) /bin/bash -c sleep 28; echo ABORT-NOT-SEEN
      12444 (ppid 12443) sleep 28
16:42:55  node c.mjs ... '[{"type":"abort","req":"ab1","background":false},{"wait":3500}]'
+9ms  ops seq=38 tools[0].status "running" ; host_state working
+9ms  ops seq=39 tools[0].status "done", tools[0].entry 52, entry 23 pi.tool-result
+9ms  ops seq=40 ["s",["docs","pi.live","runFailure"],{"taskId":49,"inputs":[48],"reason":"aborted","first":47}], ["d",["docs","pi.live","run"]], ["d",[...,"tools"]]
+9ms  host_state idle ; result ab1 {}
16:43:01  ps for "sleep 28": none (bash and sleep gone)
entries: 50 pi.assistant (thinking only) ; 52 pi.tool-result "<harness>\n[error] Tool bash was aborted\n</harness>"  isError 1
```

An abort over the host protocol ends the turn at once: the host went `working` to `idle`, `runFailure.reason` is `"aborted"`, and **no assistant text followed** (`ABORT-NOT-SEEN` never came). The in-flight bash call was recorded as a tool result with `isError: 1` and the text `Tool bash was aborted`, and the **bash process and its `sleep` child were killed** (neither was alive at the check, 6 s after the abort and well before the 28 s would have elapsed). The queued submission was not retried.

### 8. `plan archive` stops the host and removes its pid (PR #871); nothing left; scratch home deleted — PASS

`cpd plan archive` is refused to an agent (`agent_forbidden`, "for the User and the engine, not an agent"; `CPD_ATTEMPT` is set in the Worker's shell). The Lead answered `cpd ask smoke-docs-a1` (escalation `smoke-docs-a1-q1`) to run it against the scratch home with the agent variables unset, as the User's CLI would: `env -u CPD_ATTEMPT -u CPD_SESSION -u CPD_ROLE -u CPD_PLAN -u CPD_CONTEXT cpd plan archive --plan <id>` with `CPD_HOME` = the scratch home.

```
cpd plan archive --plan p1
# {"ok":true,"archived":[{"id":"p1","kind":"plan"},{"id":"9d291e14-...","kind":"session"}],"closed":[]}
t=1s  host process gone, p1.sock gone, p1.pid still listed
+6s   p1.pid gone ; cpd session show p1: "archived_at":"2026-11-05T21:52:39.241Z","state":"stopped"

cpd plan archive --plan p2     # first: archive_blocked ("Workflow w1 of Plan p2 is waiting"), so: cpd workflow cancel w1; then
# {"ok":true,"archived":[{"id":"p2","kind":"plan"},{"id":"0a000bdf-...","kind":"session"}],"closed":[]}
t=1 host (pid 48540) dead ; t=2 p2.pid gone
```

Archive stopped each host and removed its pid file (the file was gone within 2 s for p2 and was still present at 1 s, gone by 7 s, for p1). The `.sock` of p2 and the `.sqlite` stay (not asked). Cleanup:

```
pkill -f "c.mjs run/host"; tmux -L cdsmoke kill-server; rm -rf $S
ps -axo pid,command | grep -E "<scratch path>|cdsmoke|<session ids>"     # empty
ls -d /tmp/cds.*                                                         # No such file or directory
tmux -L cdsmoke ls                                                       # no server running
stat -f '%m' ~/.cpd/config.toml                                          # 1793888698 (unchanged)
```

Only the real engine (`cpd watch` pid 23606 and tmux server `cpd`) remained, and those were not touched. The smoke's engine (`cpd watch`, pid 40484) exited with its tmux server.

## Findings

| finding | severity | path:line |
|---|---|---|
| F1. `lead_tier = "H3"` is refused (`config_invalid`: it must be above `worker_tier`, default H3). A scratch config copied from the Spec's H3 needs `worker_tier = "H2"` too. The Spec and the earlier smoke omit it. | low (docs/spec) | `core/src/config/file.rs:338` |
| F2. `cpd session send` reports `"started":false` for a durable Lead whose first turn the send starts (and for every durable send): `started` is `pane.is_some()` and a durable Lead has no pane. | low | `core/src/session/send.rs:272` |
| F3. `cpd job add --draft` help says a draft "Needs no --tier and no --spec: a stub draft", but the command refuses it with `usage` ("--tier and --spec required for every Job, a --draft too"). | low | `core/src/job/mod.rs:103-106` vs `core/src/job/add.rs:211` |
| F4. `cpd plan archive` leaves the host's `.pid` for up to several seconds after the host process is gone (p1: still listed at 1 s, gone by 7 s; p2: gone by 2 s). PR #871's removal holds, but is not synchronous with the command's reply. | low | `engine/src/watch/hosts.rs:482` (the engine removes the file when it stops the host, on its own tick, not in the `archive` reply; the cause is inferred, unverified) |

No defect was found in the host protocol, ring replay, queueing, restart, ui dialogs or abort.

## Not exercised

- `frames.ts` and `ops.ts` themselves were not run over the frames; only the frame shapes and order were checked against them by reading the source (step 4).
- Manager Sessions, protocol 1 / pi-durable 1.3.0 hosts, `history` paging beyond the first page, `compact`, `configure`, `fork`, `reload`, and the WebUI browser (out of scope).
- `session tail <id>` was not used: it streams and does not exit.

## Reproduction notes

- Only `docs/research/durable-lead-smoke-1-5-0.md` was written in the worktree; no product code or tests.
- Waits used at most 30 s per shell call except the build (run in the background and polled). No sleep loop was added to a repo file.
- The scratch `repo` (a bare `git init` with `docs/x.md`) existed only for the Workflow that `cpd_plan_review` needs. The Plan was never locked, so no Worker started.
