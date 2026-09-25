//! Deterministic, serializable random number generator (SplitMix64).
//!
//! Battles store their RNG in the save file so a reloaded battle continues
//! with exactly the same random sequence, and simulations are reproducible.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform integer in `0..n`. Returns 0 when `n == 0`.
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        // Lemire's multiply-shift reduction; bias is negligible for game use.
        ((self.next_u64() >> 32).wrapping_mul(n as u64) >> 32) as u32
    }

    /// Uniform integer in `lo..=hi` (arguments may be given in any order).
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        let span = (hi as i64 - lo as i64 + 1) as u64;
        lo + (self.below(span.min(u32::MAX as u64) as u32) as i32)
    }

    /// True with the given probability in percent (clamped to 0..=100).
    pub fn chance(&mut self, percent: i32) -> bool {
        if percent <= 0 {
            return false;
        }
        if percent >= 100 {
            return true;
        }
        (self.below(100) as i32) < percent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn bounds() {
        let mut r = Rng::new(7);
        for _ in 0..10_000 {
            let v = r.range(-3, 5);
            assert!((-3..=5).contains(&v));
            assert!(r.below(10) < 10);
        }
        assert_eq!(r.below(0), 0);
        assert!(!r.chance(0));
        assert!(r.chance(100));
    }

    #[test]
    fn chance_is_roughly_fair() {
        let mut r = Rng::new(1);
        let hits = (0..10_000).filter(|_| r.chance(30)).count();
        assert!((2_700..3_300).contains(&hits), "{hits}");
    }
}
