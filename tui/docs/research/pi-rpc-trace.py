#!/usr/bin/env python3
"""Drive `pi --mode rpc` on a scratch, in-memory session and record every JSONL record both ways.

Usage: pi-rpc-trace.py <out.jsonl>
Needs /tmp/x (a few files) and /tmp/pi-rpc-demo/ext.ts (a copy of pi-rpc-demo-ext.ts) (see pi-headless-chat.md, section 3).
Sequence: get_state -> prompt (one tool call) -> steer while the tool runs -> settle
          -> /demo (extension dialogs, answered) -> prompt -> abort after a few deltas -> close stdin.
"""
import json, subprocess, sys, threading, time, queue

CMD = ["pi", "--mode", "rpc", "--no-session", "-ne", "-e", "/tmp/pi-rpc-demo/ext.ts",
       "--model", "github-copilot/claude-haiku-4.5", "--thinking", "off"]
out = open(sys.argv[1], "w")
t0 = time.monotonic()
p = subprocess.Popen(CMD, cwd="/tmp/x", stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
events: "queue.Queue[dict]" = queue.Queue()

def log(direction, rec):
    out.write(json.dumps({"t": round(time.monotonic() - t0, 3), "dir": direction, "rec": rec}) + "\n")
    out.flush()

def reader():
    # Split on LF only (rpc.md "Framing"): readline() on a binary pipe does that.
    for line in p.stdout:
        rec = json.loads(line)
        log("out", rec)
        events.put(rec)
    events.put({"type": "__eof__"})

threading.Thread(target=reader, daemon=True).start()

def send(rec):
    log("in", rec)
    p.stdin.write(json.dumps(rec).encode() + b"\n")
    p.stdin.flush()

def until(pred, timeout=120):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        try:
            r = events.get(timeout=1)
        except queue.Empty:
            continue
        if r.get("type") == "__eof__":
            raise SystemExit("pi exited")
        if r.get("type") == "extension_ui_request" and r.get("method") in ("confirm", "select"):
            if r["method"] == "confirm":
                send({"type": "extension_ui_response", "id": r["id"], "confirmed": True})
            else:
                send({"type": "extension_ui_response", "id": r["id"], "value": r["options"][0]})
        if pred(r):
            return r
    raise SystemExit("timeout")

send({"id": "s1", "type": "get_state"})
until(lambda r: r.get("id") == "s1")
send({"id": "p1", "type": "prompt", "message": "List the files in /tmp/x with one bash call, then answer in one short line."})
until(lambda r: r.get("type") == "tool_execution_start")
send({"id": "st1", "type": "steer", "message": "Also say how many files there are."})
until(lambda r: r.get("type") == "agent_settled")
send({"id": "d1", "type": "prompt", "message": "/demo"})
until(lambda r: r.get("id") == "d1")
time.sleep(1)
send({"id": "p2", "type": "prompt", "message": "Write a 400-word poem about terminals."})
n = 0
def deltas(r):
    global n
    if r.get("type") == "message_update" and r["assistantMessageEvent"]["type"] == "text_delta":
        n += 1
    return n >= 3
until(deltas)
send({"id": "a1", "type": "abort"})
until(lambda r: r.get("type") == "agent_settled")
send({"id": "s2", "type": "get_session_stats"})
until(lambda r: r.get("id") == "s2")
p.stdin.close()
p.wait(timeout=30)
log("exit", {"code": p.returncode, "stderr": p.stderr.read().decode()[-2000:]})
