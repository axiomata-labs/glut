import os
import sys
import unittest

sys.path.append(os.getcwd())
from money import split  # noqa: E402


def cents(parts):
    return [round(part * 100) for part in parts]


class ArenaCheck(unittest.TestCase):
    def test_three_ways(self):
        self.assertEqual(cents(split(10.00, 3)), [334, 333, 333])

    def test_even(self):
        self.assertEqual(cents(split(9.00, 3)), [300, 300, 300])

    def test_two_cents_over(self):
        self.assertEqual(cents(split(0.05, 3)), [2, 2, 1])


if __name__ == "__main__":
    unittest.main()
