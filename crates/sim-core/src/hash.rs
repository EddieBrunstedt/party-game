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
