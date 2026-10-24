import unittest

from src.report import by_task, total


class Report(unittest.TestCase):
    def test_total(self):
        self.assertEqual(total(["a: 1h", "b: 30m"]), 5400)

    def test_by_task_joins_the_same_task(self):
        self.assertEqual(
            by_task(["write: 1h", " write : 30m", "read: 10m"]),
            {"write": 5400, "read": 600},
        )
