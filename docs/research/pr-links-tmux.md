# PR links in the Deploy pane through tmux 3.8

VERDICT: `tmux correct`. In 6 runs of the script in the appendix (each: 484 frames per mode, anonymous and `id=`), no cue cell on the client's output was ever linked to a url other than the one the pane program last wrote there, and no other cell carried a link.

## Setup

- tmux 3.8 (`tmux -V`), private server `tmux -L cpd-pr-links-research -f /dev/null`; the default socket and the `cpd` socket were not touched.
- Pane `TERM` = `tmux-256color` (`default-terminal`). Outer (pty) `TERM` = `xterm-256color`; `xterm-ghostty` terminfo was not used. `terminal-features` = cpd's `,xterm*:extkeys,xterm-ghostty:extkeys,xterm-ghostty:hyperlinks` plus `,xterm-256color:hyperlinks` for the pty's type.
- Other server options as cpd's `SERVER_OPTIONS`: `mouse on`, `focus-events on`, `extended-keys on`, `escape-time 10`, `status off`.
- Evidence is the bytes the tmux client writes to its pty, replayed in a small terminal model (CUP/CUF/CUU.., EL/ED/ECH/DCH/ICH/REP, OSC 8 with either `ST` or `BEL`, split reads handled).
- The pane program writes cell diffs like ratatui: `ESC[<row>;<col>H` + `ESC]8;[id=..];<url> BEL` + U+F11AD + `ESC]8;; BEL` in column 30, text `owner/repo#N` at column 1, a frame counter in row 1 (the spinner; also the sync point, so no sleeps).

## What was tried (all correct)

Per mode (anonymous link, and `id=<per url>`):

1. initial 4 rows; a spinner-only frame.
2. reorder (cue url and text change), appear, disappear (to 2 rows, to 4 rows), each followed by a spinner-only frame.
3. cue-only url swap: the url of 4 cue cells changed while the text and glyph stayed (cue rewritten alone).
4. 40 churn frames of rotating PR sets (4 and 5 rows).
5. 150 seeded random frames (0-8 rows, random reorder, appear, disappear, urls changed with text kept), alone or in bursts of 3 or 6 frames written before the client output was read.
6. `tmux refresh-client`; a pty resize plus `resize-window -x 100 -y 30` plus refresh; `detach-client` then a fresh client attach (new model); `refresh-client` after the reattach. After each, the cue cells and a frame written afterwards were compared.

After every step: each cue cell's link url equals the last url written there, the glyph is U+F11AD, and no cell outside the cue cells has a link.

## What tmux does with the bytes

tmux does not forward the link parameters: it re-emits `ESC]8;id=tmux<N>;<url> ESC \` (its own id, one per distinct (id, url) it tracks) before the cue and `ESC]8;; ESC \` after it, and uses relative moves (`ESC[16X ESC[12C`) to the cue column. Example, first frame, anonymous mode (`\xf3\xb1\x86\xad` is U+F11AD):

```
\r\nowner/repo#11    \x1b[12C\x1b]8;id=tmux1;https://github.com/owner/repo/pull/11\x1b\\\xf3\xb1\x86\xad\r\n\x1b]8;;\x1b\\owner/repo#12 ...
```

An explicit `id=` of ours made no difference: tmux replaces it with its own, and the result was correct with and without.

## Limits

- Only tmux's output was tested, not Ghostty (out of scope) and not ratatui's bytes (diagnose-docs checks those).
- Outer type `xterm-256color` with `hyperlinks` forced, not `xterm-ghostty` (no terminfo here); the tmux code path (`tty_hyperlink`) is the same for any type with the feature.
- The tmux source was not read; "correct" is from behaviour only.
- Not tried: popups, panes side by side (the Deploy column in the real cockpit shares a window with other panes), `select-pane` switches, a pane alternate screen. A wrong link that only appears with those would not be seen here.
- Two harness defects were found and fixed along the way (a CSI with an empty body ignored, an OSC split across two reads); both made links look wrong when tmux was right. Hence the model's split-read handling.

## Reproduce

```
timeout 280 python3 r.py        # prints one line per mode and each mismatch; ends the server in a finally
tmux -L cpd-pr-links-research kill-server; pgrep -fl cpd-pr-links-research
```

## Appendix: the script (`r.py`, stdlib only)

```python
import os, pty, select, subprocess, sys, time, fcntl, termios, struct, re, tempfile, itertools
SOCK = "cpd-pr-links-research"
OUTER = "xterm-256color"
CUE = "\U000F11AD"
def tm(*a, check=True):
    return subprocess.run(["tmux", "-L", SOCK, "-f", "/dev/null", *a], capture_output=True, text=True, timeout=10, check=check)

PANE = r'''
import os, sys
fd = os.open(sys.argv[1], os.O_RDWR)
ack = os.open(sys.argv[2], os.O_RDWR)
out = sys.stdout.fileno()
while True:
    b = os.read(fd, 65536)
    if not b: continue
    os.write(out, b); os.write(ack, b'.')
'''

class Model:
    def __init__(s, rows, cols):
        s.rows, s.cols = rows, cols
        s.cells = {}; s.r = s.c = 0; s.link = None; s.last = " "
    def put(s, ch):
        if s.c >= s.cols: s.c = 0; s.r = min(s.r + 1, s.rows - 1)
        s.cells[(s.r, s.c)] = (ch, s.link); s.c += 1; s.last = ch
    def feed(s, data):
        import codecs
        if not hasattr(s, "dec"): s.dec = codecs.getincrementaldecoder("utf-8")("replace"); s.hold = ""
        t = s.hold + s.dec.decode(data); s.hold = ""
        k = t.rfind("\x1b")
        if k >= 0:
            tail = t[k:]
            done = (len(tail) > 1 and tail[1] == "]" and ("\x07" in tail or "\x1b\\" in tail)) or \
                   (len(tail) > 1 and tail[1] == "[" and any(0x40 <= ord(c) <= 0x7e for c in tail[2:])) or \
                   (len(tail) > 1 and tail[1] not in "[]()")
            if not done: s.hold, t = tail, t[:k]
        i = 0; n = len(t)
        while i < n:
            ch = t[i]
            if ch == "\x1b" and i + 1 < n:
                k = t[i + 1]
                if k == "]":
                    j = i + 2
                    while j < n and t[j] != "\x07" and not (t[j] == "\x1b" and j + 1 < n and t[j + 1] == "\\"): j += 1
                    body = t[i + 2:j]; i = j + (1 if t[j:j+1] == "\x07" else 2)
                    if body.startswith("8;"):
                        params, _, uri = body[2:].partition(";")
                        s.link = (params, uri) if uri else None
                elif k == "[":
                    j = i + 2
                    while j < n and not (0x40 <= ord(t[j]) <= 0x7e): j += 1
                    body, f = t[i + 2:j], t[j]; i = j + 1
                    nums = [int(x) if x.isdigit() else 0 for x in re.split("[;:]", body.lstrip("?>=")) ] if body else []
                    p = lambda k, d=1: (nums[k] if k < len(nums) and nums[k] else d)
                    if body and body[0] in "?>=": 
                        if f in "hl" and "1049" in body: s.cells = {}
                        continue
                    if f in "Hf": s.r, s.c = min(p(0)-1, s.rows-1), min(p(1)-1, s.cols-1)
                    elif f == "A": s.r = max(0, s.r - p(0))
                    elif f == "B": s.r = min(s.rows-1, s.r + p(0))
                    elif f == "C": s.c = min(s.cols-1, s.c + p(0))
                    elif f == "D": s.c = max(0, s.c - p(0))
                    elif f == "G": s.c = p(0) - 1
                    elif f == "d": s.r = p(0) - 1
                    elif f == "K":
                        m = nums[0] if nums else 0
                        for c in range(s.cols):
                            if (m == 0 and c >= s.c) or (m == 1 and c <= s.c) or m == 2: s.cells.pop((s.r, c), None)
                    elif f == "J":
                        m = nums[0] if nums else 0
                        for k2 in list(s.cells):
                            if m == 2 or (m == 0 and k2 >= (s.r, s.c)) or (m == 1 and k2 <= (s.r, s.c)): del s.cells[k2]
                    elif f == "X":
                        for c in range(s.c, s.c + p(0)): s.cells.pop((s.r, c), None)
                    elif f == "b":
                        for _ in range(p(0)): s.put(s.last)
                    elif f == "P":
                        for c in range(s.c, s.cols):
                            v = s.cells.pop((s.r, c + p(0)), None)
                            if v: s.cells[(s.r, c)] = v
                    elif f == "@":
                        for c in range(s.cols - 1, s.c + p(0) - 1, -1):
                            v = s.cells.pop((s.r, c - p(0)), None)
                            if v: s.cells[(s.r, c)] = v
                        for c in range(s.c, s.c + p(0)): s.cells.pop((s.r, c), None)
                    # m, others: ignored (SGR carries no link)
                elif k in "()": i += 3
                else: i += 2
            elif ch == "\r": s.c = 0; i += 1
            elif ch == "\n": s.r = min(s.r + 1, s.rows - 1); i += 1
            elif ch == "\b": s.c = max(0, s.c - 1); i += 1
            elif ch < " ": i += 1
            else: s.put(ch); i += 1

class Client:
    def __init__(s, rows=24, cols=80):
        s.rows, s.cols = rows, cols
        s.m = Model(rows, cols); s.raw = b""
        env = dict(os.environ, TERM=OUTER); env.pop("TMUX", None)
        pid, fd = pty.fork()
        if pid == 0:
            os.execvpe("tmux", ["tmux", "-L", SOCK, "-f", "/dev/null", "attach", "-t", "s"], env)
        s.pid, s.fd = pid, fd
        s.size(rows, cols)
    def size(s, rows, cols):
        fcntl.ioctl(s.fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
    def drain(s, quiet=0.25, limit=3.0):
        end = time.time() + limit; got = b""
        while time.time() < end:
            r, _, _ = select.select([s.fd], [], [], quiet)
            if not r: break
            try: d = os.read(s.fd, 65536)
            except OSError: break
            if not d: break
            got += d
        s.raw += got; s.m.feed(got); return got
    def sync(s, n, limit=5.0):
        want = "%06d" % n; end = time.time() + limit
        while time.time() < end:
            s.drain(quiet=0.05, limit=0.2)
            if "".join(s.m.cells.get((0, c), (" ",))[0] for c in range(6)) == want:
                s.drain(quiet=0.1, limit=0.5); return
        raise AssertionError("frame counter %s never reached the client" % want)
    def close(s):
        try: os.close(s.fd)
        except OSError: pass
        try: os.waitpid(s.pid, 0)
        except OSError: pass

COLC = 30
def frame_bytes(prev, rows, mode, spin, text_changes=True):
    """rows: list of (n, url) top to bottom; writes only changed cells like ratatui's diff."""
    out = ""
    for i, row in enumerate(rows):
        y = i + 2
        old = prev.get(y)
        if old != row:
            if old is None or old[0] != row[0]:
                out += f"\x1b[{y};1H{'owner/repo#%-6d' % row[0]}" if text_changes else ""
            if old is None or old[1] != row[1]:
                idp = f"id={abs(hash(row[1])) % 10**8}" if mode == "id" else ""
                out += f"\x1b[{y};{COLC}H\x1b]8;{idp};{row[1]}\x07{CUE}\x1b]8;;\x07"
        prev[y] = row
    for y in list(prev):
        if y - 2 >= len(rows): del prev[y]; out += f"\x1b[{y};1H\x1b[K"
    out += "\x1b[1;1H%06d" % spin
    return out.encode()

def url(n): return f"https://github.com/owner/repo/pull/{n}"

def check(cl, exp, label, bad):
    for (y0, x0), (ch, lk) in cl.m.cells.items():
        if lk and (x0 != COLC - 1 or (y0 + 1) not in exp): bad.append((label, "stray-link", y0, x0, lk[1]))
    for y, u in exp.items():
        cell = cl.m.cells.get((y - 1, COLC - 1))
        got = cell[1][1] if cell and cell[1] else None
        gc = cell[0] if cell else None
        if got != u or gc != CUE:
            bad.append((label, y, u, got, gc))

def run(mode, text_changes=True):
    tmp = tempfile.mkdtemp(prefix="cpdr-")
    fifo = os.path.join(tmp, "f"); os.mkfifo(fifo); ackf = os.path.join(tmp, "a"); os.mkfifo(ackf)
    afd = os.open(ackf, os.O_RDWR)
    def W(b):
        os.write(wfd, b)
        r_, _, _ = select.select([afd], [], [], 8)
        assert r_, "pane did not ack"
        os.read(afd, 100)
    open(os.path.join(tmp, "pane.py"), "w").write(PANE)
    wfd = os.open(fifo, os.O_RDWR)
    tm("kill-server", check=False)
    tm("new-session", "-d", "-x", "80", "-y", "24", "-s", "s", f"python3 {tmp}/pane.py {fifo} {ackf}")
    for o in (("mouse","on"),("focus-events","on"),("default-terminal","tmux-256color"),("extended-keys","on"),("escape-time","10"),("status","off")):
        tm("set","-g",*o)
    tm("set","-as","terminal-features",",xterm*:extkeys,xterm-ghostty:extkeys,xterm-ghostty:hyperlinks")
    tm("set","-as","terminal-features",f",{OUTER}:hyperlinks")
    cl = Client(); cl.drain()
    bad = []; prev = {}; exp = {}; spin = 0; frames = 0
    def send(rows, label):
        nonlocal spin, frames
        W(frame_bytes(prev, rows, mode, spin, text_changes)); spin += 1; frames += 1
        for i, r in enumerate(rows): exp[i + 2] = url(r[0]) if False else r[1]
        for y in list(exp):
            if y - 2 >= len(rows): del exp[y]
        cl.sync(spin - 1); check(cl, exp, label, bad)
    def tick():
        nonlocal spin
        W(b"\x1b[1;1H%06d" % spin); spin += 1; cl.sync(spin - 1)
    W(b"\x1b[2J\x1b[H"); tick()
    A = [(i, url(i)) for i in (11, 12, 13, 14)]
    send(A, "initial")
    send(A, "spinner-only")
    seqs = {
      "reorder": [(13,url(13)),(11,url(11)),(14,url(14)),(12,url(12))],
      "reorder2": [(12,url(12)),(13,url(13)),(11,url(11)),(14,url(14))],
      "appear": [(15,url(15)),(12,url(12)),(13,url(13)),(11,url(11)),(14,url(14))],
      "disappear": [(15,url(15)),(13,url(13)),(11,url(11)),(14,url(14))],
      "disappear2": [(13,url(13)),(11,url(11))],
      "reorder3": [(11,url(11)),(13,url(13))],
      "appear2": [(11,url(11)),(16,url(16)),(13,url(13)),(17,url(17))],
    }
    # cue url changes while the row text is NOT redrawn (worst case for cell diff)
    for name, rows in seqs.items():
        send(rows, name); send(rows, name + "+spin")
    # url swap only (same text, only link differs)
    base = [(11,url(11)),(16,url(16)),(13,url(13)),(17,url(17))]
    swapped = [(11,url(16)),(16,url(11)),(13,url(17)),(17,url(13))]
    for i, (n, u) in enumerate(swapped): pass
    # write cue only: text stays, url changes
    for y,(n,u) in enumerate(swapped, 2):
        idp = f"id={abs(hash(u)) % 10**8}" if mode == "id" else ""
        W(f"\x1b[{y};{COLC}H\x1b]8;{idp};{u}\x07{CUE}\x1b]8;;\x07".encode()); exp[y] = u; prev[y] = (prev[y][0], u)
    tick(); check(cl, exp, "cue-only-url-swap", bad)
    # many frames
    for k in range(40):
        rows = [(11+(k+j)%5, url(11+(k+j)%5)) for j in range(4 + k % 2)]
        send(rows, f"churn{k}")
    import random
    rnd = random.Random(7)
    for k in range(150):
        burst = rnd.choice([1, 1, 3, 6])
        for b in range(burst):
            nrows = rnd.randint(0, 8)
            rows = [(n, url(n if rnd.random() < .9 else rnd.randint(11, 30))) for n in rnd.sample(range(11, 30), nrows)] if rnd.random() < .7 else \
                   [(r_[0], url(rnd.randint(11, 30))) for r_ in rows]  # url changes, text kept
            W(frame_bytes(prev, rows, mode, spin, text_changes)); spin += 1; frames += 1
            exp.clear(); exp.update({i + 2: r_[1] for i, r_ in enumerate(rows)})
        cl.sync(spin - 1); check(cl, exp, f"fuzz{k}x{burst}", bad)
    # forced redraws
    tm("refresh-client", "-t", "s", check=False); tick(); check(cl, exp, "refresh-client", bad)
    cl.size(30, 100); cl.m = Model(30, 100); tm("resize-window","-t","s","-x","100","-y","30", check=False); tick()
    tm("refresh-client","-t","s",check=False); tick(); check(cl, exp, "resize", bad)
    send(rows, "post-resize")
    # detach / attach
    tm("detach-client", "-s", "s", check=False); cl.drain(); cl.close()
    cl = Client(30, 100); cl.drain(); tick(); check(cl, exp, "reattach", bad)
    send(rows, "post-reattach")
    tm("refresh-client","-t","s",check=False); tick(); check(cl, exp, "refresh-after-reattach", bad)
    cl.close()
    return frames, bad, tmp

if __name__ == "__main__":
    try:
        print("tmux", subprocess.run(["tmux","-V"],capture_output=True,text=True).stdout.strip(), "outer TERM", OUTER)
        for mode in ("anon", "id"):
            frames, bad, tmp = run(mode)
            print(f"mode={mode} frames={frames} mismatches={len(bad)}")
            for b in bad[:10]: print("  ", b)
    finally:
        tm("kill-server", check=False)
```
