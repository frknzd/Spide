use crate::pricing::{tax, Priced};

/// An order with lines.
pub struct Order {
    pub id: u64,
    lines: Vec<Line>,
}

pub struct Line {
    pub qty: u32,
    pub unit_cents: u64,
}

pub enum Status {
    Open,
    Paid { at: u64 },
}

impl Order {
    pub fn new(id: u64) -> Self {
        Order { id, lines: Vec::new() }
    }

    pub fn add(&mut self, qty: u32, unit_cents: u64) {
        if qty == 0 {
            return;
        }
        self.lines.push(Line { qty, unit_cents });
    }

    pub fn subtotal(&self) -> u64 {
        let mut sum = 0;
        for l in &self.lines {
            if l.qty > 10 {
                sum += l.unit_cents * l.qty as u64 * 9 / 10;
            } else {
                sum += l.unit_cents * l.qty as u64;
            }
        }
        sum
    }
}

impl Priced for Order {
    fn price(&self) -> u64 {
        let s = self.subtotal();
        s + tax(s)
    }
}
