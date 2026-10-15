#!/usr/bin/env python3
"""Convert a Stage 2 build Spec (docs/build/specs/<job>.md) into a `cpd job add --spec` JSON file.
Usage: spec2json.py <spec.md> <out.json>  -> prints "key<TAB>tier<TAB>title"."""
import sys, re, json
src, out = sys.argv[1], sys.argv[2]
t = open(src).read()
m = re.match(r"# Job `([^`]+)`: (.+)\n", t)
key, title = m.group(1), m.group(2).strip()
tier = re.search(r"^Tier ([OSH][0-5])", t, re.M).group(1)
parts = re.split(r"^## ", t, flags=re.M)
head, secs = parts[0], {}
order = []
for p in parts[1:]:
    name, _, body = p.partition("\n")
    secs[name.strip()] = body.strip()
    order.append(name.strip())
def take(prefix):
    for n in order:
        if n.lower().startswith(prefix):
            return n, secs[n]
    return None, ""
gname, goal = take("goal")
fname, files = take("files you own")
aname, acc = take("acceptance")
oname, oos = take("out of scope")
def expand(s):
    m = re.search(r"\{([^{}]+)\}", s)
    if not m: return [s]
    return [x for alt in m.group(1).split(",") for x in expand(s[:m.start()] + alt + s[m.end():])]
touches = []
for tok in re.findall(r"`([^`]+)`", files):
    if "/" in tok or "." in tok:
        touches += expand(tok)
items, cur = [], None
for line in acc.splitlines():
    mm = re.match(r"^(\d+)\.\s+(.*)", line)
    if mm:
        if cur is not None: items.append(cur.strip())
        cur = mm.group(2)
    elif cur is not None:
        cur += "\n" + line
    elif line.strip():
        cur = line.strip()
if cur is not None: items.append(cur.strip())
if not items and acc: items = [acc]
outscope = [x.strip().rstrip(".") for x in re.split(r";\s+", oos) if x.strip()] if oos else []
rest = [f"## {n}\n\n{secs[n]}" for n in order if n not in (gname, fname, aname, oname)]
context = (
    "This Job builds `cpd` itself (Workflow `v0`, dispatched by `cpd`). Before anything else, read the common rules in "
    "`docs/build/specs/_common.md` in your worktree and follow them, with three changes:\n"
    "- escalate with `cpd ask <attempt-id> \"<question>\" --wf v0` (`cpd` is on PATH);\n"
    "- still write the report file `_common.md` describes, but finish by running `cpd attempt submit <attempt-id> --wf v0` "
    "as the Finish section of this Worker Spec says (not by replying with the report path);\n"
    "- the Files section below is authoritative for what you may touch.\n\n"
    f"Spec header: {head.split(chr(10),1)[1].strip()}\n\n" + "\n\n".join(rest)
)
spec = {"goal": goal, "context": context, "touches": touches, "acceptance": items, "outOfScope": outscope}
for k in ("goal",):
    assert spec[k], (src, k)
assert touches and items, (src, touches, items)
json.dump(spec, open(out, "w"), indent=2)
print(f"{key}\t{tier}\t{title}")
