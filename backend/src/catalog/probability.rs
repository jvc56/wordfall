//! Combinations (PLAN.md § Probability and probability order), following
//! Zyzzyva's `LetterBag::getNumCombinations` with two blanks: the number of
//! distinct draws from the bag that spell the word, allowing up to two of the
//! bag's blanks to stand in. Exact integers; Zyzzyva's doubles are exact for
//! every value a real bag produces.

use super::tiles::{BLANK, Distribution};

pub fn choose(n: u32, k: u32) -> u128 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    let mut r: u128 = 1;
    for i in 0..k {
        r = r * u128::from(n - i) / u128::from(i + 1);
    }
    r
}

/// The product over each distinct tile of `C(count in bag, count in word)`.
fn product(dist: &Distribution, counts: &[u8]) -> u128 {
    let mut p: u128 = 1;
    for (t, &n) in counts.iter().enumerate() {
        if n > 0 {
            p = p.saturating_mul(choose(u32::from(dist.tiles[t].count), u32::from(n)));
            if p == 0 {
                return 0;
            }
        }
    }
    p
}

/// A word's combinations with up to two blanks. `counts` is the word's
/// per-tile count vector; a word holds no blank.
pub fn word_combinations(dist: &Distribution, counts: &[u8]) -> u128 {
    let blanks = u32::from(dist.blank_count());
    let mut c = counts.to_vec();
    let mut total = product(dist, &c);
    let distinct: Vec<usize> = (0..c.len()).filter(|&t| c[t] > 0).collect();
    if blanks >= 1 {
        let one = choose(blanks, 1);
        for &t in &distinct {
            c[t] -= 1;
            total += one * product(dist, &c);
            c[t] += 1;
        }
    }
    if blanks >= 2 {
        let two = choose(blanks, 2);
        for (i, &t) in distinct.iter().enumerate() {
            // The same tile twice, when the word holds it at least twice.
            if c[t] >= 2 {
                c[t] -= 2;
                total += two * product(dist, &c);
                c[t] += 2;
            }
            for &u in &distinct[i + 1..] {
                c[t] -= 1;
                c[u] -= 1;
                total += two * product(dist, &c);
                c[t] += 1;
                c[u] += 1;
            }
        }
    }
    total
}

/// A leave's combinations: the blank is an ordinary tile at the bag's blank
/// count, with no blank substitution.
pub fn leave_combinations(dist: &Distribution, counts: &[u8]) -> u128 {
    debug_assert!(counts.len() == dist.len());
    let _ = BLANK;
    product(dist, counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::tiles::Tile;
    use crate::catalog::tiles::tests::tiledef;

    fn bag(blanks: u16) -> Distribution {
        Distribution::new(
            1,
            "tiny",
            vec![
                tiledef("?", "?", blanks, 0, false),
                tiledef("A", "a", 3, 1, true),
                tiledef("B", "b", 2, 3, false),
                tiledef("C", "c", 1, 3, false),
                tiledef("D", "d", 2, 2, false),
            ],
        )
    }

    /// Every subset of physical tiles of the word's length that spells it with
    /// at most two blanks.
    fn brute_force(dist: &Distribution, word: &[Tile]) -> u128 {
        let mut physical: Vec<Tile> = Vec::new();
        for (t, d) in dist.tiles.iter().enumerate() {
            for _ in 0..d.count {
                physical.push(t as Tile);
            }
        }
        let need = dist.counts(word);
        let n = physical.len();
        let k = word.len();
        let mut total = 0u128;
        for mask in 0u32..(1 << n) {
            if mask.count_ones() as usize != k {
                continue;
            }
            let mut have = vec![0u8; dist.len()];
            for (i, &t) in physical.iter().enumerate() {
                if mask & (1 << i) != 0 {
                    have[t as usize] += 1;
                }
            }
            let blanks = have[0];
            if blanks > 2 {
                continue;
            }
            if (1..dist.len()).all(|t| have[t] <= need[t]) {
                total += 1;
            }
        }
        total
    }

    #[test]
    fn combinations_match_brute_force_for_two_one_and_no_blanks() {
        for blanks in [2u16, 1, 0] {
            let d = bag(blanks);
            for w in [
                "A", "AB", "AA", "AAB", "ABCD", "BBD", "AAA", "ABBD", "CC", "AAAB", "DDBB",
            ] {
                let tiles = d.parse_magpie(w, false).unwrap();
                assert_eq!(
                    word_combinations(&d, &d.counts(&tiles)),
                    brute_force(&d, &tiles),
                    "{w} with {blanks} blanks"
                );
            }
        }
    }

    #[test]
    fn leave_combinations_count_the_blank_as_a_tile() {
        let d = bag(2);
        let t = d.parse_magpie("?AB", true).unwrap();
        assert_eq!(leave_combinations(&d, &d.counts(&t)), 2 * 3 * 2);
        let t = d.parse_magpie("??", true).unwrap();
        assert_eq!(leave_combinations(&d, &d.counts(&t)), 1);
    }
}
