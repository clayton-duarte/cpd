import datetime as dt
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("wake", Path(__file__).with_name("qa-wake.py"))
wake = importlib.util.module_from_spec(spec)
spec.loader.exec_module(wake)


class WakeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        prompt = root / "prompt.md"
        prompt.write_text("Synthetic QA prompt")
        self.config = dict(session_id="ses_synthetic", directory=str(root), opencode="/fake/opencode",
                           state_dir=str(root / "state"), prompt_file=str(prompt))
        self.calls = []
        self.session = dict(id="ses_synthetic", time={}, location=dict(directory=str(root)))
        self.active = {}
        self.inbox = []
        self.at = dt.datetime(2026, 11, 17, 8, tzinfo=dt.timezone.utc)

    def request(self, config, method, path, body=None):
        self.calls.append((method, path, body))
        if method == "post":
            return {"id": "synthetic-input"}
        if path.endswith("/inbox"):
            return self.inbox
        return self.active if path.endswith("/active") else self.session

    def test_delivers_same_session_once_and_retains_private_receipt(self):
        self.assertEqual(wake.wake(self.config, self.at, request=self.request)["state"], "admitted")
        self.assertEqual(wake.wake(self.config, self.at, request=self.request)["state"], "skipped")
        posts = [c for c in self.calls if c[0] == "post"]
        self.assertEqual(len(posts), 1)
        self.assertEqual(posts[0][1], "/api/session/ses_synthetic/prompt")
        self.assertTrue(posts[0][2]["resume"])
        self.assertEqual(posts[0][2]["delivery"], "queue")
        receipt = Path(self.config["state_dir"]) / "2026-11-17.json"
        self.assertEqual(receipt.stat().st_mode & 0o777, 0o600)

    def test_busy_skips_without_reserving_delivery(self):
        self.active = {"ses_synthetic": {"type": "running"}}
        self.assertEqual(wake.wake(self.config, self.at, request=self.request)["reason"], "target session is busy")
        self.assertFalse(any(c[0] == "post" for c in self.calls))
        self.assertFalse((Path(self.config["state_dir"]) / "2026-11-17.json").exists())

    def test_toronto_schedule_handles_both_dst_offsets_and_skips_other_hours(self):
        for at in [dt.datetime(2026, 6, 9, 7, tzinfo=dt.timezone.utc), self.at]:
            self.assertEqual(wake.wake(self.config, at, request=self.request)["state"], "admitted")
        self.calls.clear()
        at = dt.datetime(2026, 11, 17, 7, tzinfo=dt.timezone.utc)
        self.assertEqual(wake.wake(self.config, at, request=self.request)["state"], "skipped")
        self.assertEqual(self.calls, [])

    def test_check_never_posts_or_reserves(self):
        self.assertEqual(wake.wake(self.config, self.at, check=True, request=self.request)["state"], "checked")
        self.assertFalse(any(c[0] == "post" for c in self.calls))
        self.assertFalse(list(Path(self.config["state_dir"]).glob("*.json")))

    def test_pending_input_and_concurrent_wake_are_skipped(self):
        self.inbox = [{"id": "synthetic-pending"}]
        self.assertEqual(wake.wake(self.config, self.at, request=self.request)["reason"], "target has pending input")
        self.inbox = []
        import fcntl
        lock = Path(self.config["state_dir"]) / "wake.lock"
        with lock.open("w") as file:
            fcntl.flock(file, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assertEqual(wake.wake(self.config, self.at, request=self.request)["reason"], "another wake is running")
        self.assertFalse(any(c[0] == "post" for c in self.calls))

    def test_missing_moved_archived_or_unverifiable_session_fails_closed(self):
        for session in [None, {}, {**self.session, "time": {"archived": 1}},
                        {**self.session, "location": {"directory": "/elsewhere"}}]:
            self.session = session
            with self.assertRaises(ValueError):
                wake.wake(self.config, self.at, request=self.request)
        self.assertFalse(any(c[0] == "post" for c in self.calls))

    def test_ambiguous_post_failure_is_not_retried(self):
        def failed(config, method, path, body=None):
            if method == "post":
                raise TimeoutError("Synthetic ambiguous delivery")
            return self.request(config, method, path, body)
        with self.assertRaises(TimeoutError):
            wake.wake(self.config, self.at, request=failed)
        self.calls.clear()
        self.assertEqual(wake.wake(self.config, self.at, request=self.request)["state"], "skipped")
        self.assertFalse(any(c[0] == "post" for c in self.calls))

    def test_configuration_requires_recorded_standing_authority(self):
        root = Path(self.config["directory"])
        path = root / "wake.json"
        path.write_text(json.dumps(self.config))
        policy = root / "qa.config.json"
        policy.write_text(json.dumps({"weekly_allowance": {"reset_pacing": True}}))
        with self.assertRaises(ValueError):
            wake.load_config(path)
        policy.write_text(json.dumps({"standing_authorization": "Synthetic approval",
                                      "weekly_allowance": {"reset_pacing": True}}))
        self.assertEqual(wake.load_config(path), self.config)


if __name__ == "__main__":
    unittest.main()
