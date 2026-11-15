import { tax, Priced } from "./pricing";

export interface Line {
  qty: number;
  unitCents: number;
}

export class Order implements Priced {
  private lines: Line[] = [];
  constructor(public readonly id: number) {}

  add(qty: number, unitCents: number): void {
    if (qty <= 0) {
      return;
    }
    this.lines.push({ qty, unitCents });
  }

  subtotal(): number {
    let sum = 0;
    for (const l of this.lines) {
      sum += l.qty > 10 ? (l.unitCents * l.qty * 9) / 10 : l.unitCents * l.qty;
    }
    return sum;
  }

  price(): number {
    const s = this.subtotal();
    return s + tax(s);
  }
}

export type Status = "open" | "paid";
