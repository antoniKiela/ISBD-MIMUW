// Prosty, deterministyczny generator liczb pseudolosowych (xorshift64).
// To samo ziarno daje zawsze ten sam ciąg liczb, więc wygenerowane dane są powtarzalne.
// Nie nadaje się do kryptografii – służy tylko do tworzenia danych testowych.

pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        // xorshift ze stanem 0 generowałby same zera.
        let state = if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed };
        Rng { state }
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// Liczba z zakresu 0..n (n > 0).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    /// Liczba z zakresu lo..=hi.
    pub fn range_i64(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo) as u64 + 1) as i64
    }
}
