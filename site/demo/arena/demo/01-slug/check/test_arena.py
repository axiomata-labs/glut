import os
import sys
import unittest

sys.path.append(os.getcwd())
from slug import slug  # noqa: E402


class ArenaCheck(unittest.TestCase):
    def test_the_example(self):
        self.assertEqual(slug("Big Red Mug!"), "big-red-mug")

    def test_runs_become_one_dash(self):
        self.assertEqual(slug("  a -- b__c  "), "a-b-c")

    def test_digits_stay(self):
        self.assertEqual(slug("Mug 2 (XL)"), "mug-2-xl")


if __name__ == "__main__":
    unittest.main()
