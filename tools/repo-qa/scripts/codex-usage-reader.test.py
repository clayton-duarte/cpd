import importlib.util
from pathlib import Path
import time
import os
import subprocess
import sys
import tempfile
import json
import unittest

spec = importlib.util.spec_from_file_location("reader", Path(__file__).with_name("codex-usage-reader.py"))
reader = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reader)


class UsageTests(unittest.TestCase):
    def fixture(self):
        return {"ordinaryUsageAllowed": True, "accountId": "must not escape", "rateLimitsByLimitId": {"codex": {
            "limitId": "codex", "spendControlReached": False,
            "primary": {"windowDurationMins": 300, "usedPercent": 10},
            "secondary": {"windowDurationMins": 10080, "usedPercent": 29, "resetsAt": int(time.time()+86400)}}}}

    def test_selects_weekly_window_in_either_position_and_sanitizes(self):
        for swap in (False, True):
            data = self.fixture()
            bucket = data["rateLimitsByLimitId"]["codex"]
            if swap:
                bucket["primary"], bucket["secondary"] = bucket["secondary"], bucket["primary"]
            sample = reader.normalize(data)
            self.assertEqual(sample["remaining_percent"], 71)
            self.assertNotIn("accountId", sample)

    def test_invalid_or_ambiguous_account_data_fails_closed(self):
        for change in ({"usedPercent": -1}, {"usedPercent": True}, {"usedPercent": float("nan")}, {"resetsAt": 0}, {"resetsAt": True}):
            data = self.fixture()
            data["rateLimitsByLimitId"]["codex"]["secondary"].update(change)
            with self.assertRaises(ValueError):
                reader.normalize(data)
        data = self.fixture()
        data["rateLimitsByLimitId"]["codex"]["primary"]["windowDurationMins"] = 10080
        with self.assertRaises(ValueError):
            reader.normalize(data)


    def test_coalesced_app_server_replies_stop_at_threshold_and_remove_snapshot(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fake = root / "codex"
            fake.write_text("#!/usr/bin/env python3\n" + r'''
import json, sys, time
assert 'cli_auth_credentials_store="ephemeral"' in sys.argv
assert 'app-server' in sys.argv
def send(value):
    print(json.dumps(value), flush=True)
for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if method == "initialize":
        send({"id":1,"result":{}})
    elif method == "account/login/start":
        # Both messages may be buffered by a text reader in one OS read.
        sys.stdout.write(json.dumps({"id":2,"result":{"verificationUrl":"https://example.invalid/activate","userCode":"fake-code"}})+"\n"+json.dumps({"method":"account/login/completed","params":{"success":True}})+"\n")
        sys.stdout.flush()
    elif method == "account/rateLimits/read":
        for used in (30,51):
            send({"id":3,"result":{"accountId":"must-not-leak","ordinaryUsageAllowed":True,"rateLimitsByLimitId":{"codex":{"limitId":"codex","spendControlReached":False,"secondary":{"usedPercent":used,"windowDurationMins":10080,"resetsAt":int(time.time()+86400)}}}}})
''')
            fake.chmod(0o700)
            snapshot = root / "usage.json"
            result = subprocess.run([sys.executable, str(Path(__file__).with_name("codex-usage-reader.py")),
                                     "--output", str(snapshot), "--min-remaining", "50"],
                                    env={**os.environ, "PATH":str(root)+os.pathsep+os.environ["PATH"]},
                                    capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
            values = [json.loads(line) for line in result.stdout.splitlines()]
            self.assertEqual([v["remaining_percent"] for v in values if "remaining_percent" in v], [70,49])
            self.assertEqual(values[-1], {"paused":True})
            self.assertNotIn("must-not-leak", result.stdout+result.stderr)
            self.assertFalse(snapshot.exists())


    def test_saved_login_survives_reader_restart_without_another_device_flow(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fake = root / "codex"
            fake.write_text("#!/usr/bin/env python3\n" + r'''
import json, os, sys, time
from pathlib import Path
assert 'cli_auth_credentials_store="file"' in sys.argv
state = Path(os.environ["CODEX_HOME"]) / "auth.json"
def send(value):
    print(json.dumps(value), flush=True)
for line in sys.stdin:
    request = json.loads(line)
    method = request.get("method")
    if method == "initialize":
        send({"id":1,"result":{}})
    elif method == "account/read":
        send({"id":4,"result":{"account":{"type":"chatgpt","email":"must-not-leak"} if state.exists() else None}})
    elif method == "account/login/start":
        assert not state.exists(), "must reuse saved login"
        # Fake credentials only: verify lifecycle, never contact an account.
        state.write_text(json.dumps({"fake_auth":True}))
        send({"id":2,"result":{"verificationUrl":"https://example.invalid/activate","userCode":"fake-code"}})
        send({"method":"account/login/completed","params":{"success":True}})
    elif method == "account/rateLimits/read":
        send({"id":3,"result":{"ordinaryUsageAllowed":True,"rateLimitsByLimitId":{"codex":{"limitId":"codex","spendControlReached":False,"secondary":{"usedPercent":32,"windowDurationMins":10080,"resetsAt":int(time.time()+86400)}}}}})
''')
            fake.chmod(0o700)
            auth = root / "private-cli-state"
            output = root / "usage.json"
            original_selector = os.environ.get("CODEX_HOME")
            command = [sys.executable, str(Path(__file__).with_name("codex-usage-reader.py")),
                       "--output", str(output), "--min-remaining", "50", "--auth-dir", str(auth), "--once"]
            environment = {**os.environ, "PATH":str(root)+os.pathsep+os.environ["PATH"]}
            first = subprocess.run(command+["--login"], env=environment, capture_output=True, text=True, timeout=10)
            self.assertEqual(first.returncode, 0, first.stderr)
            self.assertIn("verification_url", first.stdout)
            second = subprocess.run(command, env=environment, capture_output=True, text=True, timeout=10)
            self.assertEqual(second.returncode, 0, second.stderr)
            self.assertNotIn("verification_url", second.stdout)
            self.assertNotIn("must-not-leak", first.stdout+second.stdout)
            self.assertEqual(json.loads(output.read_text())["remaining_percent"], 68)
            self.assertEqual(auth.stat().st_mode & 0o777, 0o700)
            self.assertEqual((auth / "auth.json").stat().st_mode & 0o777, 0o600)
            self.assertEqual(os.environ.get("CODEX_HOME"), original_selector)

    def test_persistent_mode_without_saved_login_fails_without_prompting(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fake = root / "codex"
            fake.write_text("#!/usr/bin/env python3\n" + r'''
import json,sys
for line in sys.stdin:
    request=json.loads(line)
    if request.get("method")=="initialize":print(json.dumps({"id":1,"result":{}}),flush=True)
    elif request.get("method")=="account/read":print(json.dumps({"id":4,"result":{"account":None}}),flush=True)
    elif request.get("method")=="account/login/start":raise AssertionError("Unexpected repeated sign-in")
''')
            fake.chmod(0o700)
            output = root / "usage.json"
            result = subprocess.run([sys.executable, str(Path(__file__).with_name("codex-usage-reader.py")),
                                     "--output", str(output), "--min-remaining", "50", "--auth-dir", str(root / "private"), "--once"],
                                    env={**os.environ, "PATH":str(root)+os.pathsep+os.environ["PATH"]},
                                    capture_output=True, text=True, timeout=10)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("No saved login", result.stderr)
            self.assertNotIn("verification_url", result.stdout)
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()
