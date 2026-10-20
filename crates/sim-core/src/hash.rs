use std::hash::{Hash, Hasher};

/// FNV-1a: tiny, fast enough for small states, and identical on every platform we ship
/// (unlike `DefaultHasher`, whose algorithm is unspecified and may change between Rust releases).
pub struct StableHasher(u64);

impl Default for StableHasher {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Hasher for StableHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }

    // Lengths of `Vec`s and slices arrive here. Hash them as 64 bits on every platform, so a
    // 32-bit peer computes the same checksums (on 64-bit this is the bytes `usize` gives anyway).
    fn write_usize(&mut self, i: usize) {
        self.write_u64(i as u64);
    }

    fn write_isize(&mut self, i: isize) {
        self.write_i64(i as i64);
    }
}

/// Checksum of a simulation state, used for rollback desync detection and determinism tests.
pub fn checksum<T: Hash>(value: &T) -> u64 {
    let mut hasher = StableHasher::default();
    value.hash(&mut hasher);
    hasher.finish()
}

/// `f32` has no `Hash` impl; hash its exact bit pattern instead.
pub fn hash_f32<H: Hasher>(value: f32, state: &mut H) {
    state.write_u32(value.to_bits());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_hash_as_64_bits() {
        let mut by_usize = StableHasher::default();
        by_usize.write_usize(3);
        let mut by_u64 = StableHasher::default();
        by_u64.write_u64(3);
        assert_eq!(by_usize.finish(), by_u64.finish());
    }

    #[test]
    fn checksums_stay_the_same() {
        // FNV-1a over the length as 8 little-endian bytes, then the bytes: what 64-bit builds
        // always hashed, so checksums recorded before the `usize` fix still match.
        assert_eq!(checksum(&vec![1u8, 2, 3]), 0x01ef_76d4_29b1_1552);
    }
}
