//! A fixed, dependency-free pseudo-random generator.
//!
//! SplitMix64 is used on purpose: the algorithm is part of the seed contract. Swapping it would
//! change every reproduction, so it must never depend on a library version.

#[derive(Clone, Debug)]
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

    /// A value in `0..n`. The tiny modulo bias is irrelevant at the sizes used here and keeps
    /// the algorithm trivially portable.
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next_u64() % n
        }
    }

    /// A value in `lo..hi`.
    pub fn range(&mut self, lo: u64, hi: u64) -> u64 {
        if hi <= lo {
            lo
        } else {
            lo + self.below(hi - lo)
        }
    }

    /// True with probability `num / den`.
    pub fn chance(&mut self, num: u64, den: u64) -> bool {
        self.below(den) < num
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

/// Derive an independent stream for `(seed, a, b)`.
///
/// Used so that, for example, request 7's second provider call draws the same latency whether or
/// not other faults were removed by the shrinker. Stable streams are what make shrinking sound.
pub fn mix(seed: u64, a: u64, b: u64) -> u64 {
    let mut r = Rng::new(seed ^ a.rotate_left(21) ^ b.rotate_left(42));
    r.next_u64();
    r.next_u64()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_differ() {
        assert_ne!(Rng::new(1).next_u64(), Rng::new(2).next_u64());
    }

    #[test]
    fn splitmix64_reference_vector() {
        // First output of the reference SplitMix64 for seed 0.
        assert_eq!(Rng::new(0).next_u64(), 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn below_and_range_stay_in_bounds() {
        let mut r = Rng::new(7);
        for _ in 0..1000 {
            assert!(r.below(10) < 10);
            let v = r.range(5, 9);
            assert!((5..9).contains(&v));
        }
        assert_eq!(r.below(0), 0);
        assert_eq!(r.range(4, 4), 4);
    }

    #[test]
    fn mix_is_stable_and_separates_streams() {
        assert_eq!(mix(1, 2, 3), mix(1, 2, 3));
        assert_ne!(mix(1, 2, 3), mix(1, 2, 4));
        assert_ne!(mix(1, 2, 3), mix(1, 3, 3));
    }
}
