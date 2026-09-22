//! Deterministic pseudo-random number generator (no extra crates).
//!
//! A small splitmix64-based generator with Box-Muller gaussian sampling,
//! used for the AI score noise (rules.md section 13.8). The sequence is
//! fully deterministic for a given seed (specification_rust.md).

/// Deterministic PRNG: splitmix64 state plus a cached gaussian sample.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
    spare: Option<f64>,
}

impl Rng {
    /// Create a generator seeded with `seed`.
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed.wrapping_add(0x9E3779B97F4A7C15),
            spare: None,
        }
    }
    /// Next unbiased `u64` of the splitmix64 stream.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
    /// Next uniform `f64` in the half-open interval [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        const DIV: f64 = 18446744073709551616.0;
        (self.next_u64() as f64) / DIV
    }
    /// Next gaussian sample with mean `mean` and std-dev `sigma`
    /// (Box-Muller, cached second sample).
    pub fn gauss(&mut self, mean: f64, sigma: f64) -> f64 {
        if sigma <= 0.0 {
            return mean;
        }
        if let Some(v) = self.spare.take() {
            return mean + sigma * v;
        }
        let mut u1 = self.next_f64();
        if u1 <= 0.0 {
            u1 = f64::MIN_POSITIVE;
        }
        let u2 = self.next_f64();
        let r = (-2.0 * u1.ln()).sqrt();
        let t = 2.0 * std::f64::consts::PI * u2;
        self.spare = Some(r * t.sin());
        mean + sigma * r * t.cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deterministic() {
        let mut a = Rng::new(12345);
        let mut b = Rng::new(12345);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
        let mut a = Rng::new(999);
        let mut b = Rng::new(999);
        for _ in 0..50 {
            let x = a.gauss(0.0, 1.0);
            let y = b.gauss(0.0, 1.0);
            assert!((x - y).abs() < 1e-12);
        }
    }
}
