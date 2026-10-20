/// PCG32 (XSH RR). Lives inside simulation state so rollback restores it along with everything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rng {
    state: u64,
}

const MULTIPLIER: u64 = 6_364_136_223_846_793_005;
const INCREMENT: u64 = 1_442_695_040_888_963_407;

/// Scrambles seeds so that related seeds (consecutive, or XOR-combined) give unrelated streams.
fn splitmix64(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut rng = Self { state: 0 };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(splitmix64(seed));
        rng.next_u32();
        rng
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(MULTIPLIER).wrapping_add(INCREMENT);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform-ish value in `lo..=hi` (tiny bias is irrelevant for gameplay, determinism is what matters).
    pub fn range_u32(&mut self, lo: u32, hi: u32) -> u32 {
        debug_assert!(lo <= hi);
        let span = (hi - lo) as u64 + 1;
        lo + ((self.next_u32() as u64 * span) >> 32) as u32
    }

    /// True with probability `percent`/100.
    pub fn chance(&mut self, percent: u32) -> bool {
        self.range_u32(0, 99) < percent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn range_stays_in_bounds() {
        let mut rng = Rng::new(7);
        for _ in 0..10_000 {
            let v = rng.range_u32(3, 9);
            assert!((3..=9).contains(&v));
        }
    }
}
