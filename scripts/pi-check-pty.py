#!/usr/bin/env python3
"""The pty runner of scripts/pi-check.sh (docs/implementing.md, "Checking a pi version").

  pi-check-pty.py run --seconds <n> --follow <fifo> --cwd <dir> --stderr <file> --out <file> -- <command...>
  pi-check-pty.py pause <seconds>

`run` starts <command> on a new pty of 120x30, as a session leader that owns the pty as its controlling terminal, so
pi's interactive mode starts (pi needs a tty with a size, not a multiplexer). It drains the pty, so pi never blocks on
a full buffer, and waits on three things with one select: the pty, the fifo <follow> (the fake cpd writes to it when
the adapter runs `cpd events follow`) and SIGCHLD (pi's end). Exit 0 when the fifo was written; exit 1 when pi ended
first or <seconds> passed first, with the reason on the first line of <out> and the tail of what pi drew after it;
exit 130 on SIGTERM or SIGINT. On every one of those paths the whole process group of the command is killed (SIGKILL)
and waited for before this returns, so nothing of pi's is left. `pause` is a select with no descriptor: the
script's one way to wait a given time without `sleep` (the cleanup's retries).
"""

import fcntl
import os
import re
import select
import signal
import struct
import subprocess
import sys
import termios
import time

COLUMNS, ROWS = 120, 30
TAIL_BYTES = 4000
GROUP_POLLS = 400  # of 5 ms: the bound on waiting for the group's members to be gone after SIGKILL


class Interrupted(Exception):
    pass


def interrupt(signum, frame):
    raise Interrupted(signum)


def pause(seconds):
    select.select([], [], [], seconds)


def group_gone(pgid):
    try:
        os.killpg(pgid, 0)
    except (ProcessLookupError, PermissionError):  # macOS answers EPERM for a group of zombies only
        return True
    return False


def stop(proc):
    """Kills the command's process group, reaps the command and waits until no member of the group is left."""
    pgid = proc.pid  # start_new_session: the command is its group's leader
    try:
        os.killpg(pgid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        pass
    proc.wait()
    for _ in range(GROUP_POLLS):
        if group_gone(pgid):
            return
        pause(0.005)


def readable(fd):
    return bool(select.select([fd], [], [], 0)[0])


def screen(tail):
    text = tail.decode("utf-8", "replace")
    text = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b.", "", text)
    return "".join(c for c in text.replace("\r\n", "\n").replace("\r", "\n") if c == "\n" or c >= " ")


def describe(code):
    return "pi was killed by signal %d" % -code if code < 0 else "pi exited with status %d" % code


def run(opts, command):
    master, slave = os.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLUMNS, 0, 0))
    follow = os.open(opts["--follow"], os.O_RDWR | os.O_NONBLOCK)  # O_RDWR: no EOF while no writer, on Linux and macOS alike
    wake_r, wake_w = os.pipe()
    for fd in (master, wake_r, wake_w):
        os.set_blocking(fd, False)
    errors = open(opts["--stderr"], "ab")
    tail = b""
    reason = None
    proc = None
    try:
        proc = subprocess.Popen(
            command, stdin=slave, stdout=slave, stderr=errors, cwd=opts["--cwd"], start_new_session=True, close_fds=True,
            preexec_fn=lambda: fcntl.ioctl(0, termios.TIOCSCTTY, 0),
        )
        os.close(slave)
        signal.set_wakeup_fd(wake_w)
        signal.signal(signal.SIGCHLD, lambda *_: None)  # a handler must exist for the wake-up descriptor to be written
        signal.signal(signal.SIGTERM, interrupt)
        signal.signal(signal.SIGINT, interrupt)
        end = time.monotonic() + float(opts["--seconds"])
        watched = [master, follow, wake_r]
        while reason is None:
            ready, _, _ = select.select(watched, [], [], max(0.0, end - time.monotonic()))
            if follow in ready:
                return 0
            if master in ready:
                try:
                    data = os.read(master, 65536)
                except BlockingIOError:
                    data = None
                except OSError:  # EIO on Linux: every holder of the slave is gone
                    data = b""
                if data == b"":
                    watched.remove(master)
                elif data:
                    tail = (tail + data)[-TAIL_BYTES:]
            if wake_r in ready:
                try:
                    os.read(wake_r, 4096)
                except BlockingIOError:
                    pass
                code = proc.poll()
                if code is not None:
                    if readable(follow):
                        return 0
                    while master in watched and readable(master):
                        try:
                            data = os.read(master, 65536)
                        except OSError:
                            data = b""
                        if not data:
                            watched.remove(master)
                        tail = (tail + data)[-TAIL_BYTES:]
                    reason = describe(code)
            if reason is None and time.monotonic() >= end:
                reason = "no `cpd events follow` within %s s" % opts["--seconds"]
        with open(opts["--out"], "w") as out:
            out.write(reason + "\n" + screen(tail))
        return 1
    except Interrupted:
        return 130
    finally:
        for sig in (signal.SIGTERM, signal.SIGINT):
            signal.signal(sig, signal.SIG_IGN)
        if proc is not None:
            stop(proc)
        signal.set_wakeup_fd(-1)
        for fd in (master, follow, wake_r, wake_w):
            os.close(fd)
        errors.close()


def main(argv):
    if len(argv) == 3 and argv[1] == "pause":
        pause(float(argv[2]))
        return 0
    if len(argv) > 2 and argv[1] == "run" and "--" in argv:
        split = argv.index("--")
        flags = argv[2:split]
        opts = dict(zip(flags[::2], flags[1::2]))
        if sorted(opts) == ["--cwd", "--follow", "--out", "--seconds", "--stderr"] and len(flags) == 10 and argv[split + 1:]:
            return run(opts, argv[split + 1:])
    print("usage: pi-check-pty.py run --seconds N --follow FIFO --cwd DIR --stderr FILE --out FILE -- COMMAND... | pause SECONDS", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
