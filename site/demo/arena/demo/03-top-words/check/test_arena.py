import os
import sys
import unittest

sys.path.append(os.getcwd())
from words import top_words  # noqa: E402


class ArenaCheck(unittest.TestCase):
    def test_the_example(self):
        with open("reviews.txt") as f:
            text = f.read()
        self.assertEqual(top_words(text, 2), [("the", 4), ("mug", 2)])

    def test_a_tie_keeps_the_first_seen(self):
        self.assertEqual(top_words("b a b a c", 3), [("b", 2), ("a", 2), ("c", 1)])

    def test_punctuation_parts_words(self):
        self.assertEqual(top_words("it's IT", 1), [("it", 2)])


if __name__ == "__main__":
    unittest.main()
