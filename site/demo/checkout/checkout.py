"""What the customer pays at the checkout: the goods and the shipping."""

SHIPPING = 4.90


def to_pay(goods: float) -> float:
    """The goods' gross total plus shipping."""
    return round(goods + SHIPPING, 2)
