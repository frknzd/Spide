from dataclasses import dataclass, field

from shop.pricing import Priced, tax


@dataclass
class Line:
    qty: int
    unit_cents: int


class Order(Priced):
    def __init__(self, order_id: int) -> None:
        self.order_id = order_id
        self.lines: list[Line] = []

    def add(self, qty: int, unit_cents: int) -> None:
        if qty <= 0:
            return
        self.lines.append(Line(qty, unit_cents))

    def subtotal(self) -> int:
        total = 0
        for line in self.lines:
            if line.qty > 10:
                total += line.unit_cents * line.qty * 9 // 10
            else:
                total += line.unit_cents * line.qty
        return total

    def price(self) -> int:
        s = self.subtotal()
        return s + tax(s)
