"""Prices for the shop's cart: discounts, VAT and the total."""

VAT_RATE = 0.19


def discounted(price: float, percent: float) -> float:
    """The price after a discount of `percent` per cent."""
    return price - price * percent / 100


def with_vat(net: float, rate: float = VAT_RATE) -> float:
    """The gross price of a net one, rounded to cents."""
    return round(net * (1 + rate), 2)


def total(items: list[tuple[float, int]], percent: float = 0) -> float:
    """The gross total of (price, quantity) pairs after a discount on each item."""
    return round(sum(with_vat(discounted(price, percent)) * quantity for price, quantity in items), 2)
