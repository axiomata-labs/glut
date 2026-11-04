import unittest

from prices import discounted, total, with_vat


class Prices(unittest.TestCase):
    def test_a_discount_is_given_in_per_cent(self):
        self.assertEqual(discounted(100.0, 10), 90.0)

    def test_vat_is_added_to_the_net_price(self):
        self.assertEqual(with_vat(100.0), 119.0)

    def test_the_total_of_a_cart(self):
        self.assertEqual(total([(10.0, 2), (5.0, 1)], percent=20), 23.8)


if __name__ == "__main__":
    unittest.main()
