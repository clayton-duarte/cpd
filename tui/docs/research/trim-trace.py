#!/usr/bin/env python3
"""Trim a raw pi-rpc-trace.py log into a readable trace: long strings cut, runs of deltas collapsed,
the system prompt and model endpoint details redacted. Usage: trim-trace.py <raw.jsonl> > pi-rpc-trace.jsonl"""
import json, sys

REDACT = {"baseUrl", "headers", "compat"}

def cut(v, n=100):
    if isinstance(v, str):
        return v if len(v) <= n else v[:n] + f"…(+{len(v) - n} chars)"
    if isinstance(v, list):
        return [cut(x, n) for x in v]
    if isinstance(v, dict):
        return {k: ("[redacted]" if k in REDACT else cut(x, n)) for k, x in v.items()}
    return v

def shape(rec):
    m = rec.get("message")
    if isinstance(m, dict) and m.get("role") == "system":
        rec = dict(rec, message={"role": "system", "sections": "[system prompt sections: " + ", ".join(m.get("sections", {})) + "]"})
    if rec.get("type") == "agent_end":
        rec = dict(rec, messages=f"[{len(rec['messages'])} messages]")
    if rec.get("type") == "turn_end":
        rec = dict(rec, message="[same as message_end]", toolResults=f"[{len(rec['toolResults'])} tool results]")
    if rec.get("type") == "message_start" and isinstance(m, dict) and m.get("role") == "assistant":
        rec = dict(rec, message={k: m[k] for k in ("role", "content", "provider", "model", "stopReason") if k in m})
    if rec.get("type") == "message_end" and isinstance(m, dict) and m.get("role") == "assistant":
        keep = {k: m[k] for k in ("role", "content", "stopReason", "errorMessage") if k in m}
        keep["usage"] = {k: m["usage"][k] for k in ("input", "output", "totalTokens")}
        rec = dict(rec, message=keep)
    if rec.get("type") == "message_update":
        rec = {k: v for k, v in rec.items() if k != "usage"}
    if rec.get("type") == "response" and rec.get("command") == "get_state":
        d = dict(rec["data"]); d["model"] = {k: d["model"][k] for k in ("id", "provider", "api", "reasoning", "contextWindow")}
        rec = dict(rec, data=d)
    return cut(rec)

run_kind, run_len = None, 0
for line in open(sys.argv[1]):
    r = json.loads(line)
    rec = r["rec"]
    kind = rec.get("assistantMessageEvent", {}).get("type") if rec.get("type") == "message_update" else None
    if kind in ("text_delta", "toolcall_delta") and kind == run_kind:
        run_len += 1
        if run_len > 3:
            continue
    else:
        if run_len > 3:
            print(json.dumps({"elided": f"{run_len - 3} more {run_kind} records"}))
        run_kind, run_len = kind, 1
    print(json.dumps({"t": r["t"], "dir": r["dir"], "rec": shape(rec)}, ensure_ascii=False))
