//! Deterministic shuffles and question hashes (PLAN.md § Cascades →
//! Deterministic shuffles). Rust and TypeScript must agree bit for bit.

/// The reset seed is the operation's seed XOR this constant.
pub const RESET_XOR: u64 = 0x9E37_79B9_7F4A_7C15;

/// Vigna's reference `splitmix64.c`: each call adds the golden gamma to the
/// state and mixes the result, so the first output follows the first increment.
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        SplitMix64 { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Shuffles question indexes: sorted ascending, then Fisher–Yates from the
/// last position down to 1, swapping `i` with `next_u64() mod (i + 1)`.
/// Returns the indexes in position order.
pub fn shuffle(idx: &[u32], seed: u64) -> Vec<u32> {
    let mut v = idx.to_vec();
    v.sort_unstable();
    let mut rng = SplitMix64::new(seed);
    for i in (1..v.len()).rev() {
        let j = (rng.next_u64() % (i as u64 + 1)) as usize;
        v.swap(i, j);
    }
    v
}

pub fn reset_seed(seed: u64) -> u64 {
    seed ^ RESET_XOR
}

/// FNV-1a 64 of the question indexes in ascending order, each fed as a
/// little-endian 32-bit integer.
pub fn questions_hash(idx: &[u32]) -> u64 {
    let mut v = idx.to_vec();
    v.sort_unstable();
    let mut h: u64 = 14_695_981_039_346_656_037;
    for i in v {
        for b in i.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(1_099_511_628_211);
        }
    }
    h
}

/// u64 values are stored as their two's-complement i64.
pub fn to_i64(v: u64) -> i64 {
    v as i64
}

pub fn from_i64(v: i64) -> u64 {
    v as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix_matches_the_reference() {
        // splitmix64.c seeded with 0 and 1234567: well-known first outputs.
        let mut r = SplitMix64::new(0);
        assert_eq!(r.next_u64(), 0xE220_A839_7B1D_CDAF);
        let mut r = SplitMix64::new(1_234_567);
        assert_eq!(r.next_u64(), 6_457_827_717_110_365_317);
        assert_eq!(r.next_u64(), 3_203_168_211_198_807_973);
    }

    #[test]
    fn shuffles_are_permutations_and_deterministic() {
        let idx: Vec<u32> = (0..1000).collect();
        let a = shuffle(&idx, 42);
        let mut sorted = a.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, idx);
        assert_eq!(a, shuffle(&idx, 42));
        assert_ne!(a, shuffle(&idx, 43));
        let mut rev = idx.clone();
        rev.reverse();
        assert_eq!(shuffle(&rev, 42), a, "input order does not matter");
    }

    #[test]
    fn the_hash_ignores_order_and_round_trips_through_i64() {
        assert_eq!(questions_hash(&[3, 1, 2]), questions_hash(&[1, 2, 3]));
        assert_ne!(questions_hash(&[1, 2, 3]), questions_hash(&[1, 2, 4]));
        let h = questions_hash(&[0]);
        assert_eq!(from_i64(to_i64(h)), h);
        assert_eq!(from_i64(-1), u64::MAX);
    }
}
