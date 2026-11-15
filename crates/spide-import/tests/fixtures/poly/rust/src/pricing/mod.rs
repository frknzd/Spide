pub trait Priced {
    fn price(&self) -> u64;
}

pub fn tax(cents: u64) -> u64 {
    match cents {
        0 => 0,
        c if c < 1000 => c / 10,
        c => c / 5,
    }
}

pub fn parse_cents(s: &str) -> u64 {
    s.parse().unwrap()
}
