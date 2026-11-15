from typing import Protocol


class Priced(Protocol):
    def price(self) -> int: ...


def tax(cents: int) -> int:
    if cents == 0:
        return 0
    return cents // 10 if cents < 1000 else cents // 5


def parse_cents(s: str) -> int:
    try:
        return int(s)
    except:
        return 0
