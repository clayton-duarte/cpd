#!/usr/bin/env python3
"""Keep an explicit device login in memory and publish sanitized weekly telemetry.

Requires Python >=3.9 and a Codex CLI supporting app-server's account APIs and
cli_auth_credentials_store="ephemeral". No inference, credit reset or auth-file reads.
"""
import argparse
import datetime as dt
import json
import math
import os
from pathlib import Path
import queue
import signal
import threading
import subprocess
import sys
import tempfile
import time


def normalize(result):
    bucket = (result.get("rateLimitsByLimitId") or {}).get("codex") or result.get("rateLimits") or {}
    windows = [w for w in (bucket.get("primary"), bucket.get("secondary"))
               if isinstance(w, dict) and w.get("windowDurationMins") == 10080]
    if bucket.get("limitId") != "codex" or len(windows) != 1:
        raise ValueError("No unambiguous Codex weekly window")
    window = windows[0]
    used = window.get("usedPercent")
    if isinstance(used, bool) or not isinstance(used, (int, float)) or not math.isfinite(used) or not 0 <= used <= 100:
        raise ValueError("Invalid weekly percentage")
    allowed, reached = result.get("ordinaryUsageAllowed"), bucket.get("spendControlReached")
    if not isinstance(allowed, bool) or not isinstance(reached, bool):
        raise ValueError("Unknown account availability")
    reset = window.get("resetsAt")
    if isinstance(reset, bool) or not isinstance(reset, (int, float)) or not math.isfinite(reset):
        raise ValueError("Invalid weekly reset")
    now = dt.datetime.now(dt.timezone.utc)
    reset_at = dt.datetime.fromtimestamp(reset, dt.timezone.utc)
    if not now < reset_at <= now + dt.timedelta(days=7):
        raise ValueError("Weekly reset outside the active window")
    return dict(version=1, source="codex-app-server", sampled_at=now.isoformat(),
                limit_id="codex", window_minutes=10080, remaining_percent=100-used,
                resets_at=reset_at.isoformat(), ordinary_usage_allowed=allowed, spend_control_reached=reached)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--min-remaining", required=True, type=float)
    args = parser.parse_args()
    if not 0 < args.min_remaining <= 100:
        parser.error("--min-remaining must be in (0,100]")
    def terminate(*_):
        raise KeyboardInterrupt()
    signal.signal(signal.SIGTERM, terminate)
    output = args.output.absolute()
    # Explicit output destination; existing snapshots never authorize a new reader.
    output.unlink(missing_ok=True)
    with tempfile.TemporaryDirectory(prefix="qa-usage-") as work:
        with open(Path(work) / "diagnostics.log", "w") as log:
            proc = subprocess.Popen(["codex", "app-server", "--stdio", "-c", 'sqlite_home='+json.dumps(work),
                                     "-c", 'cli_auth_credentials_store="ephemeral"'],
                                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, text=True)
            lines = queue.Queue()

            def receive():
                for line in proc.stdout:
                    lines.put(line)
                lines.put(None)

            threading.Thread(target=receive, daemon=True).start()

            def send(value):
                proc.stdin.write(json.dumps(value)+"\n")
                proc.stdin.flush()

            send(dict(id=1, method="initialize", params=dict(clientInfo=dict(name="repo-qa-usage", version="1"))))
            ready, pending = False, False
            deadline, next_poll = time.monotonic()+720, float("inf")
            try:
                while True:
                    current = time.monotonic()
                    if current > deadline:
                        raise RuntimeError("Reader timeout; stop QA and authenticate a new reader")
                    if ready and not pending and current >= next_poll:
                        output.unlink(missing_ok=True)
                        send(dict(id=3, method="account/rateLimits/read", params={}))
                        pending, deadline = True, current+15
                    try:
                        line = lines.get(timeout=1)
                    except queue.Empty:
                        if proc.poll() is not None:
                            raise RuntimeError("Codex app-server exited")
                        continue
                    if line is None:
                        raise RuntimeError("Codex app-server closed")
                    message = json.loads(line)
                    if "error" in message and message.get("id") in (1, 2, 3):
                        raise RuntimeError("Codex account API failed; stop QA and reauthenticate")
                    if message.get("id") == 1:
                        send(dict(method="initialized", params={}))
                        send(dict(id=2, method="account/login/start", params=dict(type="chatgptDeviceCode")))
                    elif message.get("id") == 2:
                        result = message.get("result") or {}
                        print(json.dumps(dict(verification_url=result.get("verificationUrl"), user_code=result.get("userCode"))), flush=True)
                    elif message.get("method") == "account/login/completed":
                        if not (message.get("params") or {}).get("success"):
                            raise RuntimeError("Device login failed")
                        ready, next_poll, deadline = True, 0, float("inf")
                    elif message.get("id") == 3:
                        sample = normalize(message.get("result") or {})
                        print(json.dumps(sample), flush=True)
                        if sample["remaining_percent"] < args.min_remaining or not sample["ordinary_usage_allowed"] or sample["spend_control_reached"]:
                            print('{"paused":true}', flush=True)
                            return
                        fd, name = tempfile.mkstemp(prefix=".qa-usage-", dir=output.parent)
                        try:
                            with os.fdopen(fd, "w") as file:
                                file.write(json.dumps(sample)+"\n")
                            os.replace(name, output)
                        finally:
                            Path(name).unlink(missing_ok=True)
                        pending, next_poll, deadline = False, time.monotonic()+30, float("inf")
            finally:
                output.unlink(missing_ok=True)
                proc.terminate()
                try:
                    proc.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()
                proc.stdin.close()
                proc.stdout.close()


if __name__ == "__main__":
    try:
        main()
    except (KeyboardInterrupt, Exception) as error:
        print(json.dumps(dict(error=str(error), creation_allowed=False)), file=sys.stderr)
        sys.exit(1)
