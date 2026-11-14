#!/usr/bin/env python3
"""Read sanitized weekly telemetry using explicit ephemeral or reusable CLI auth.

Requires Python >=3.9 and a Codex CLI supporting app-server's account APIs and
credential stores. No inference, credit reset or credential-content reads.
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
    parser.add_argument("--auth-dir", type=Path, help="Private persistent Codex credential directory outside Git")
    parser.add_argument("--login", action="store_true", help="Explicitly allow initial device sign-in with --auth-dir")
    parser.add_argument("--once", action="store_true", help="Publish one reading and exit; saved CLI auth survives")
    args = parser.parse_args()
    if args.login and args.auth_dir is None:
        parser.error("--login requires --auth-dir")
    child_environment = os.environ.copy()
    if args.auth_dir is not None:
        auth_dir = args.auth_dir.absolute()
        if auth_dir.is_symlink():
            parser.error("Credential directory must not be a symlink")
        parent_codex_state = Path(os.environ.get("CODEX_HOME", str(Path.home() / ".codex"))).resolve()
        if auth_dir.resolve() == parent_codex_state:
            parser.error("Use a separate credential directory; managed Codex auth must remain untouched")
        if (auth_dir / "auth.json").is_symlink():
            parser.error("Credential file must not be a symlink")
        auth_dir.mkdir(mode=0o700, parents=True, exist_ok=True)
        auth_dir = auth_dir.resolve()
        inside_git = subprocess.run(["git", "-C", str(auth_dir), "rev-parse", "--show-toplevel"],
                                    capture_output=True).returncode == 0
        if inside_git:
            parser.error("Credential directory must be outside Git repositories")
        os.chmod(auth_dir, 0o700)
        # Documented CLI home selector, scoped only to this child process.
        # The parent/managed Codex environment and its credentials are untouched.
        child_environment["CODEX_HOME"] = str(auth_dir)
    else:
        auth_dir = None
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
                                     "-c", 'cli_auth_credentials_store='+json.dumps("file" if auth_dir else "ephemeral")],
                                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, text=True,
                                    env=child_environment)
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
            ready, pending, keep_snapshot, window_reset = False, False, False, None
            deadline, next_poll = time.monotonic()+720, float("inf")
            try:
                while True:
                    current = time.monotonic()
                    if window_reset is not None and dt.datetime.now(dt.timezone.utc) >= window_reset:
                        raise RuntimeError("Authorized weekly window expired; stop QA and obtain new authorization")
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
                    if "error" in message and message.get("id") in (1, 2, 3, 4):
                        raise RuntimeError("Codex account API failed; stop QA and reauthenticate")
                    if message.get("id") == 1:
                        send(dict(method="initialized", params={}))
                        if auth_dir is None:
                            send(dict(id=2, method="account/login/start", params=dict(type="chatgptDeviceCode")))
                        else:
                            send(dict(id=4, method="account/read", params={}))
                    elif message.get("id") == 4:
                        account = (message.get("result") or {}).get("account")
                        if account is not None:
                            if account.get("type") != "chatgpt":
                                raise RuntimeError("A saved ChatGPT subscription login is required")
                            ready, next_poll, deadline = True, 0, float("inf")
                        elif args.login:
                            send(dict(id=2, method="account/login/start", params=dict(type="chatgptDeviceCode")))
                        else:
                            raise RuntimeError("No saved login; run once with --auth-dir and --login")
                    elif message.get("id") == 2:
                        result = message.get("result") or {}
                        print(json.dumps(dict(verification_url=result.get("verificationUrl"), user_code=result.get("userCode"))), flush=True)
                    elif message.get("method") == "account/login/completed":
                        if not (message.get("params") or {}).get("success"):
                            raise RuntimeError("Device login failed")
                        if auth_dir is not None:
                            credential_file = auth_dir / "auth.json"
                            if not credential_file.is_file():
                                raise RuntimeError("Login completed without saving reusable CLI credentials")
                            os.chmod(credential_file, 0o600)
                        ready, next_poll, deadline = True, 0, float("inf")
                    elif message.get("id") == 3:
                        sample = normalize(message.get("result") or {})
                        reset = dt.datetime.fromisoformat(sample["resets_at"])
                        if window_reset is not None and abs((reset - window_reset).total_seconds()) > 1:
                            raise RuntimeError("Authorized weekly window changed; stop QA and obtain new authorization")
                        window_reset = min(reset, window_reset) if window_reset is not None else reset
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
                        if auth_dir is not None and (auth_dir / "auth.json").is_file():
                            os.chmod(auth_dir / "auth.json", 0o600)
                        if args.once:
                            keep_snapshot = True
                            return
                        pending, next_poll, deadline = False, time.monotonic()+30, float("inf")
            finally:
                if not keep_snapshot:
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
