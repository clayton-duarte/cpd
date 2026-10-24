# Thinking block in a real pi: click, reload, italics

Before (origin/main `09b72595`): fails. With `hideThinkingBlock` true the block never starts folded and no click does anything (all 3 cells); with `hideThinkingBlock` false it is fine in a fresh pi, but after one or two /reload a click does nothing (rows stay folded or become pi's own italic `Thinking…` label) and the block carries `ESC[3m`.
After (Lead Branch `7e6e40ba`): passes. All 6 cells fold at start, expand on one click, fold on a second click, and no row of the block carries `ESC[3m`.
Failed: only the 'before' cells listed below; no 'after' cell failed, and nothing was left 'not run' except what is under "Not verified".

## Matrix

Cell columns: (1) first view folded (` ╭─` pill row `Thinking…` + ` ╰` expand row); (2) after one click, all four thinking lines are shown on `│` rails; (3) after a second click folded again; (4) any row of the block with `ESC[3m` in any of the three captures. Result is `pass` only when (1)–(3) hold and (4) is none.

| Run | hideThinkingBlock | Pi state | (1) folded | (2) expands on click | (3) folds on 2nd click | (4) italic rows | Result |
|---|---|---|---|---|---|---|---|
| before | false | fresh | yes | yes | yes | none | pass |
| before | false | after 1 /reload | yes | **no** | yes (nothing had expanded) | **3 views** | **fail** |
| before | false | after 2 /reload | yes | **no** | yes (nothing had expanded) | **3 views** | **fail** |
| before | true | fresh | **no** | **no** | **no** | none | **fail** |
| before | true | after 1 /reload | **no** | **no** | **no** | **3 views** | **fail** |
| before | true | after 2 /reload | **no** | **no** | **no** | **3 views** | **fail** |
| after | false | fresh | yes | yes | yes | none | pass |
| after | false | after 1 /reload | yes | yes | yes | none | pass |
| after | false | after 2 /reload | yes | yes | yes | none | pass |
| after | true | fresh | yes | yes | yes | none | pass |
| after | true | after 1 /reload | yes | yes | yes | none | pass |
| after | true | after 2 /reload | yes | yes | yes | none | pass |

The italic detector parses every `ESC[...m` sequence of the block rows (from the pill row to the row above the `Short reply.` box), skipping the arguments of `38;5;n` / `38;2;r;g;b`, and flags a bare `3`. A first, naive match on `;3m` also hit the amber `38;5;3` of pi's warning rows; that was a detector bug and was fixed before the results above. The detector does fire: it flags the `before` rows below.

## Versions

- `pi --version`: 1.3.0
- `tmux -V`: tmux 3.8
- 'after' extension: worktree of this Job, commit `7e6e40ba` (`thinking: reload-proof updateContent hook, start folded`), file `pi/compact-tools/extensions/compact-tools.ts`
- 'before' extension: `origin/main` at `09b72595` (`Make the local Gate pass on macOS (#278)`), same file in a throwaway worktree

## Commands

Setup (scratch only; nothing under `~/.pi/agent`, nothing in the User's tmux):

```sh
git fetch origin
git worktree add --detach /tmp/tl/before origin/main     # removed at the end
mkdir -p /tmp/tl/cwd
```

`/tmp/tl/sess.jsonl`: a version-3 session (`docs/session-format.md`): a header, a user message `Hello`, and an assistant message with a `thinking` block of four lines and a `text` block `Short reply.`:

```json
{"type":"session","version":3,"id":"11111111-1111-4111-8111-111111111111","timestamp":"2025-01-01T00:00:00.000Z","cwd":"/tmp/tl/cwd"}
{"type":"message","id":"a1b2c3d4","parentId":null,"timestamp":"2025-01-01T00:00:01.000Z","message":{"role":"user","content":"Hello","timestamp":1735689601000}}
{"type":"message","id":"b2c3d4e5","parentId":"a1b2c3d4","timestamp":"2025-01-01T00:00:02.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"First line of thought with *emphasis* here.\nSecond line with _underscore emphasis_ here.\nThird line plain.\nFourth line plain."},{"type":"text","text":"Short reply."}],"api":"anthropic-messages","provider":"anthropic","model":"claude-sonnet-4-5","usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"totalTokens":2,"cost":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"total":0}},"stopReason":"stop","timestamp":1735689602000}}
```

One run of one cell group (`<ROOT>` is the worktree for 'after', `/tmp/tl/before` for 'before'; `<HIDE>` is `false` or `true`):

```sh
A=$(mktemp -d /tmp/tl/agent.XXXX)
cat > $A/settings.json <<S
{"hideThinkingBlock": <HIDE>, "quietStartup": true, "tuiMode": "fullscreen", "theme": "dango", "themes": ["<worktree>/pi/themes"]}
S
mkdir -p $A/sessions/--tmp-tl-cwd--; cp /tmp/tl/sess.jsonl $A/sessions/--tmp-tl-cwd--/s.jsonl
tmux -u -L thinking-live -f /dev/null new-session -d -x 100 -y 32 -s t -c /tmp/tl/cwd \
  "PI_CODING_AGENT_DIR=$A pi --offline --session $A/sessions/--tmp-tl-cwd--/s.jsonl -e <ROOT>/pi/compact-tools/extensions/compact-tools.ts"
```

Per view (fresh, then after each /reload): rows with `tmux -L thinking-live capture-pane -p -t t`, SGR with `... capture-pane -p -e -t t`. The pill row is found in the plain capture (row containing `Thinking…`); with `ROW` its 1-based row and column 5:

```sh
tmux -L thinking-live send-keys -t t -l $'\e[<0;5;'$ROW'M\e[<0;5;'$ROW'm'    # click 1, wait 1 s, capture
# find the pill row again in that capture, same send-keys with the new ROW  # click 2, wait 1 s, capture
tmux -L thinking-live send-keys -t t '/reload' Enter                          # then poll capture-pane (0.3 s steps, max 20 s) for 'Reloaded', wait 1.5 s
tmux -L thinking-live kill-server                                             # end of each run
```

The sequence ran in one pi per (run, hideThinkingBlock): fresh views and clicks, `/reload`, views and clicks, `/reload`, views and clicks. The driver was a Python script in `/tmp/tl` (`drive.py`, not committed) that wrapped exactly these commands, each under a 20 s `timeout`, and the whole run under `timeout 120`. A tuiMode of `regular` was only used while debugging (see below); every result is `fullscreen`.

Cleanup:

```sh
tmux -L thinking-live kill-server
git worktree remove --force /tmp/tl/before
rm -rf /tmp/tl
```

## Captured rows of the failing cells

Top of the screen (the user bubble `Hello`, blank rows and the footer trimmed to the block). Glyph in the expand row is the Nerd Font icon pi's theme draws.

### before, hideThinkingBlock false, after 1 /reload (after 2 /reload is identical)

First view (folded, but with italic SGR on both block rows):

```text
 ╭─Thinking…
 ╰󰬦 expand
```

After one click: the block is replaced by pi's own `Thinking…` label (italic), no rails, no thinking text:

```text
 Thinking…
```

After a second click:

```text
 ╭─Thinking…
 ╰󰬦 expand
```

Italic rows (`ESC[3m`): v1 ` ╭─Thinking…` and ` ╰ expand`; v2 ` Thinking…`; v3 ` ╭─Thinking…` and ` ╰ expand`.

### before, hideThinkingBlock true, fresh (identical after 1 and 2 /reload)

All three views (first, after click 1, after click 2) show only pi's hidden label, never the pill row, the expand row or the text:

```text
 Thinking…
```

Italic rows: none in the fresh pi's three views; after 1 and 2 /reload all three views carry `ESC[3m` on the ` Thinking…` row.

## Not verified

- tuiMode `regular`: every cell ran in `fullscreen` (the User's mode). Not matrixed in `regular`.
- A real model reply, streaming, and the adapter's own reload after an install: not run (offline, no model; the session is replayed). `/reload` was the only reload path tested.
- Real mouse hardware or a terminal emulator other than tmux 3.8: clicks were SGR mouse bytes typed into the pane.
- The first attempt with `--session <file>` pointing outside pi's session directory showed an empty chat (pi moved the file into `$A/sessions/--tmp-tl-cwd--/` and displayed nothing); I put the file there before starting, which is the form in the commands above. Cause unverified.
- Only one Thinking block per session, four lines, 100x32 pane.
