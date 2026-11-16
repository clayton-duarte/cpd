#!/usr/bin/env python3
"""Deliver one daily QA wake to an existing OpenCode session, never a new agent."""
import argparse
import datetime as dt
import fcntl
import json
import os
from pathlib import Path
import re
import subprocess
import sys
from zoneinfo import ZoneInfo


def load_config(path):
    value = json.loads(Path(path).read_text())
    keys = {"session_id", "directory", "opencode", "state_dir", "prompt_file"}
    if set(value) != keys or not re.fullmatch(r"ses_[A-Za-z0-9]+", value["session_id"]):
        raise ValueError("Invalid wake configuration")
    for key in keys - {"session_id"}:
        if not isinstance(value[key], str) or not Path(value[key]).is_absolute():
            raise ValueError(f"{key} must be an absolute path")
    policy = json.loads((Path(value["directory"]) / "qa.config.json").read_text())
    if not isinstance(policy.get("standing_authorization"), str) or not policy["standing_authorization"].strip():
        raise ValueError("Checkout has no recorded standing authorization")
    if not policy.get("weekly_allowance"):
        raise ValueError("Checkout has no live allowance policy")
    return value


def api(config, method, path, body=None):
    command = [config["opencode"], "api", method, path]
    if body is not None:
        command += ["--data", json.dumps(body)]
    # The CLI uses its own service discovery/auth; no raw tokens or public listener.
    result = subprocess.run(command, cwd=config["directory"], capture_output=True,
                            text=True, timeout=60, check=True)
    envelope = json.loads(result.stdout)
    if not isinstance(envelope, dict) or "data" not in envelope:
        raise ValueError("Invalid OpenCode API response")
    return envelope["data"]


def wake(config, now=None, check=False, request=api):
    now = now or dt.datetime.now(dt.timezone.utc)
    local = now.astimezone(ZoneInfo("America/Toronto"))
    if not check and local.hour != 3:
        return {"state": "skipped", "reason": "outside 3am Toronto"}
    state = Path(config["state_dir"])
    if state.is_symlink():
        raise ValueError("State directory must not be a symlink")
    state.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.chmod(state, 0o700)
    lock_path = state / "wake.lock"
    fd = os.open(lock_path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, "w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return {"state": "skipped", "reason": "another wake is running"}
        session_id = config["session_id"]
        session = request(config, "get", f"/api/session/{session_id}")
        if not isinstance(session, dict) or session.get("id") != session_id:
            raise ValueError("Target session is missing or mismatched")
        if session.get("time", {}).get("archived") is not None:
            raise ValueError("Target session is archived; do not silently revive it")
        if session.get("location", {}).get("directory") != config["directory"]:
            raise ValueError("Session moved; review its checkout before enabling wakes")
        active = request(config, "get", "/api/session/active")
        if not isinstance(active, dict):
            raise ValueError("Invalid active-session response")
        if session_id in active:
            return {"state": "skipped", "reason": "target session is busy"}
        inbox = request(config, "get", f"/api/session/{session_id}/inbox")
        if not isinstance(inbox, list):
            raise ValueError("Invalid session inbox response")
        if inbox:
            return {"state": "skipped", "reason": "target has pending input"}
        # Read prompt before reserving today's delivery, so missing input is retryable.
        prompt = Path(config["prompt_file"]).read_text()
        if not prompt.strip():
            raise ValueError("Wake prompt is empty")
        if check:
            return {"state": "checked", "session_id": session_id, "delivery": False}
        receipt = state / f"{local.date()}.json"
        try:
            fd = os.open(receipt, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        except FileExistsError:
            return {"state": "skipped", "reason": "today was already attempted; inspect receipt"}
        record = {"state": "attempted", "session_id": session_id, "at": now.isoformat()}
        with os.fdopen(fd, "w") as file:
            json.dump(record, file)
            file.flush()
            os.fsync(file.fileno())
        # Reserve before POST. Failure/timeout is ambiguous: never blindly resend.
        admitted = request(config, "post", f"/api/session/{session_id}/prompt", {
            "text": prompt, "delivery": "queue", "resume": True,
            "metadata": {"cpd_qa_wake_date": str(local.date())},
        })
        if not isinstance(admitted, dict) or not isinstance(admitted.get("id"), str):
            raise ValueError("Unverifiable prompt admission; inspect receipt before retrying")
        record.update(state="admitted", input_id=admitted["id"])
        receipt.write_text(json.dumps(record) + "\n")
        return {"state": "admitted", "session_id": session_id, "date": str(local.date())}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", required=True)
    parser.add_argument("--check", action="store_true", help="Validate target without sending a prompt")
    args = parser.parse_args()
    print(json.dumps(wake(load_config(args.config), check=args.check)), flush=True)


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        # Do not print API stderr, credentials, or session messages to cron logs.
        print(json.dumps({"state": "failed", "error": type(error).__name__,
                          "action": "inspect target and receipt before retrying"}), file=sys.stderr)
        sys.exit(1)
