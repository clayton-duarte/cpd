# pi-durable: kill mid-tool, resume in place

Verdict: all four facts held. Faux model (`fauxProvider` from pi-ai, no network), real pi-durable, real SQLite file, real `runWorker` and `openConversation` from this worktree.

## Setup

- Throwaway `phase.ts` in a `mktemp -d` directory outside the repo (not committed), with `node_modules` symlinked to the worktree's. It takes `1` or `2` and calls `runWorker` with `open` = `openConversation(models, o)`, a real SQLite store at `storePath`, `resolveModel` returning the faux model, a `cpd` fake (`session show`: `registered_event` 0, `loop_guard_turns` 20; `policy show`: empty policy; anything else: `{}`), a `follow` that does nothing, and the real report writer (temp file and rename).
- Phase 1 faux script: one `bash` call `echo ran >> <tmp>/ran.txt; sleep 8`, then `all done`. Phase 2 script: `all done` only.
- To read the entry kinds, `phase.ts` also dumped `conversation.entries()` to a file at each report write.

## Commands and exit codes

| # | Command | Result |
|---|---|---|
| 1 | `timeout 30 node phase.ts 1 &`, wait for `ran.txt` to be non-empty, `sleep 1.5`, `pkill -9 -f "phase.ts 1"` | killed with SIGKILL (pkill exit 0); the process had not exited by itself |
| 2 | `timeout 30 node phase.ts 2` (background, killed once the report existed) | report written; the exit code was not captured (see run 4) |
| 3 | `timeout 30 node phase.ts 2` again, report file removed first (third start) | report written; exit code not captured |
| 4 | `timeout 30 node phase.ts 2`, report file removed first, foreground | exit 13 |

Exit 13 is Node's "unsettled top-level await": after the report, the idle loop has nothing left to keep it alive, because the `follow` fake does nothing (the real `main.ts` follower would keep it alive). It is an artefact of the fake, not of resume. Runs 2 and 3 printed the same warning, so I infer they also ended by themselves before the `pkill`.

## The four facts

1. One `ran` line: **held**. `wc -l ran.txt` gave 1 after phase 1 and still 1 after every later start, so the tool did not run again.
2. Entry kinds after the resume: **held**. `pi.user, pi.system, pi.assistant, pi.tool-result, pi.assistant` (user, system, assistant, tool-result, assistant), the same at runs 2 and 3.
3. First message submitted once: **held**. The entries hold exactly one user message, `Read spec.md and follow it exactly. Your attempt id is durable-verify-a1.` (same timestamp at every start), though `runWorker` submits it at every start with the same `requestId`.
4. Same report on a third start: **held**. After phase 2 and after the third start the file was byte-identical (`cmp` equal):

   ```json
   {"stop_reason":"stop","error_message":null,"turns":2,"tokens_in":1555,"tokens_out":30,"cache_read":0,"cache_write":1555,"cost_usd":0,"model":"faux/faux-1"}
   ```

   The file did not exist before the first resume. In run 3 I removed it first, so the third start wrote it again with the same content.

## Cleanup

Every child was killed or exited, `pgrep -f phase.ts` printed nothing, and the temp directory was removed.
