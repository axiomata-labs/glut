import unittest

from src.timeparse import parse_duration


class TimeParse(unittest.TestCase):
    def test_hours_and_minutes(self):
        self.assertEqual(parse_duration("2h30m"), 9000)

    def test_seconds(self):
        self.assertEqual(parse_duration("45s"), 45)
