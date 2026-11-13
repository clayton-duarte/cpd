import importlib.util
from pathlib import Path
import time
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


if __name__ == "__main__":
    unittest.main()
