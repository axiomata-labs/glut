"""Money for the shop: a bill split between people."""


def split(total: float, people: int) -> list[float]:
    """The bill `total` in `people` parts."""
    part = round(total / people, 2)
    return [part] * people
