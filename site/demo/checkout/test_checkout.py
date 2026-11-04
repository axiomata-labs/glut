import unittest

from checkout import to_pay


class Checkout(unittest.TestCase):
    def test_shipping_is_added(self):
        self.assertEqual(to_pay(20.0), 24.9)


if __name__ == "__main__":
    unittest.main()
