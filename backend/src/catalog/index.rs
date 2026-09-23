//! `LexiconIndex` and `LeaveSetIndex` (PLAN.md § Catalog Indexes → Derived
//! attributes). Every derived attribute is computed here when an index is
//! built, so there is one implementation of each and nothing stored can drift.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use super::pos;
use super::probability::{leave_combinations, word_combinations};
use super::tiles::{BLANK, Distribution, Tile};

pub const MAX_WORD_TILES: usize = 15;
pub const MAX_LEAVE_TILES: usize = 6;

/// One word and its derived attributes.
#[derive(Debug, Clone)]
pub struct WordEntry {
    pub tiles: Box<[Tile]>,
    /// The letter count vector, sized to the distribution.
    pub counts: Box<[u8]>,
    pub alphagram: u32,
    pub length: u8,
    pub num_vowels: u8,
    pub num_unique_letters: u8,
    pub point_value: u16,
    pub num_anagrams: u32,
    /// In tile order.
    pub front_hooks: Box<[Tile]>,
    pub back_hooks: Box<[Tile]>,
    pub has_front_inner_hook: bool,
    pub has_back_inner_hook: bool,
    pub combinations: u128,
    pub probability_order: u32,
    pub min_probability_order: u32,
    pub max_probability_order: u32,
    pub playability: f64,
    pub playability_order: u32,
    pub min_playability_order: u32,
    pub max_playability_order: u32,
    /// Parts of speech as a bitmask of `PartOfSpeech::bit`.
    pub pos: u16,
    pub definition: Box<str>,
}

/// A raw lexicon row, as stored.
pub struct RawWord {
    pub tiles: Vec<Tile>,
    pub playability: f64,
    pub definition: String,
}

#[derive(Debug)]
pub struct LexiconIndex {
    pub id: i16,
    pub name: String,
    pub distribution: Arc<Distribution>,
    /// Sorted in alphabetical (tile) order; a word's id is its index here.
    pub words: Vec<WordEntry>,
    pub word_lookup: HashMap<Box<[Tile]>, u32>,
    /// Distinct alphagrams, in tile order.
    pub alphagrams: Vec<Box<[Tile]>>,
    /// The words of each alphagram, in alphabetical order: the Anagram answers.
    pub alphagram_words: Vec<Vec<u32>>,
    pub alphagram_lookup: HashMap<Box<[Tile]>, u32>,
    /// Word ids per length (index = length), in alphabetical order.
    pub by_length: Vec<Vec<u32>>,
    pub max_num_anagrams: u32,
    /// The size of the largest length bucket: the highest per-length rank.
    pub max_order_rank: u32,
    pub build_ms: u64,
    pub approx_bytes: usize,
}

/// Ranks within one bucket by (value descending, alphagram, word): the unique
/// rank, and the lowest and highest rank shared by entries with the same value.
fn rank<V: Copy>(
    ids: &[u32],
    value: impl Fn(u32) -> V,
    cmp_value_desc: impl Fn(V, V) -> Ordering,
    tie_key: impl Fn(u32, u32) -> Ordering,
    mut set: impl FnMut(u32, u32, u32, u32),
) {
    let mut sorted = ids.to_vec();
    sorted.sort_by(|&a, &b| cmp_value_desc(value(a), value(b)).then_with(|| tie_key(a, b)));
    let mut i = 0;
    while i < sorted.len() {
        let mut j = i + 1;
        while j < sorted.len()
            && cmp_value_desc(value(sorted[i]), value(sorted[j])) == Ordering::Equal
        {
            j += 1;
        }
        for (k, &id) in sorted[i..j].iter().enumerate() {
            set(id, (i + k + 1) as u32, (i + 1) as u32, j as u32);
        }
        i = j;
    }
}

impl LexiconIndex {
    pub fn build(
        id: i16,
        name: &str,
        distribution: Arc<Distribution>,
        mut raw: Vec<RawWord>,
    ) -> Self {
        let start = Instant::now();
        let d = &distribution;
        raw.sort_by(|a, b| a.tiles.cmp(&b.tiles));
        raw.dedup_by(|a, b| a.tiles == b.tiles);

        let word_lookup: HashMap<Box<[Tile]>, u32> = raw
            .iter()
            .enumerate()
            .map(|(i, w)| (w.tiles.clone().into_boxed_slice(), i as u32))
            .collect();

        // Alphagrams.
        let mut alpha_of: Vec<Box<[Tile]>> = raw
            .iter()
            .map(|w| {
                let mut a = w.tiles.clone();
                a.sort_unstable();
                a.into_boxed_slice()
            })
            .collect();
        let mut alphagrams: Vec<Box<[Tile]>> = alpha_of.clone();
        alphagrams.sort();
        alphagrams.dedup();
        let alphagram_lookup: HashMap<Box<[Tile]>, u32> = alphagrams
            .iter()
            .enumerate()
            .map(|(i, a)| (a.clone(), i as u32))
            .collect();
        let mut alphagram_words: Vec<Vec<u32>> = vec![Vec::new(); alphagrams.len()];
        let alpha_ids: Vec<u32> = alpha_of.drain(..).map(|a| alphagram_lookup[&a]).collect();
        for (w, &a) in alpha_ids.iter().enumerate() {
            alphagram_words[a as usize].push(w as u32);
        }

        // Hooks and inner hooks.
        let n = raw.len();
        let mut front_hooks: Vec<Vec<Tile>> = vec![Vec::new(); n];
        let mut back_hooks: Vec<Vec<Tile>> = vec![Vec::new(); n];
        let mut front_inner = vec![false; n];
        let mut back_inner = vec![false; n];
        for (i, w) in raw.iter().enumerate() {
            let t = &w.tiles;
            if t.len() >= 2 {
                if let Some(&j) = word_lookup.get(&t[1..]) {
                    front_hooks[j as usize].push(t[0]);
                    front_inner[i] = true;
                }
                if let Some(&j) = word_lookup.get(&t[..t.len() - 1]) {
                    back_hooks[j as usize].push(t[t.len() - 1]);
                    back_inner[i] = true;
                }
            }
        }

        let mut by_length: Vec<Vec<u32>> = vec![Vec::new(); MAX_WORD_TILES + 1];
        let mut words: Vec<WordEntry> = Vec::with_capacity(n);
        for (i, w) in raw.into_iter().enumerate() {
            let counts = d.counts(&w.tiles);
            let length = w.tiles.len() as u8;
            by_length[length as usize].push(i as u32);
            let mut fh = std::mem::take(&mut front_hooks[i]);
            fh.sort_unstable();
            let mut bh = std::mem::take(&mut back_hooks[i]);
            bh.sort_unstable();
            words.push(WordEntry {
                num_vowels: w.tiles.iter().filter(|&&t| d.is_vowel(t)).count() as u8,
                num_unique_letters: counts.iter().filter(|&&c| c > 0).count() as u8,
                point_value: w.tiles.iter().map(|&t| d.value(t)).sum(),
                combinations: word_combinations(d, &counts),
                counts: counts.into_boxed_slice(),
                alphagram: alpha_ids[i],
                length,
                num_anagrams: alphagram_words[alpha_ids[i] as usize].len() as u32,
                front_hooks: fh.into_boxed_slice(),
                back_hooks: bh.into_boxed_slice(),
                has_front_inner_hook: front_inner[i],
                has_back_inner_hook: back_inner[i],
                probability_order: 0,
                min_probability_order: 0,
                max_probability_order: 0,
                playability: w.playability,
                playability_order: 0,
                min_playability_order: 0,
                max_playability_order: 0,
                pos: pos::parse(&w.definition),
                definition: w.definition.into_boxed_str(),
                tiles: w.tiles.into_boxed_slice(),
            });
        }

        // Ranks within each length: value descending, then alphagram, then word.
        for bucket in &by_length {
            let tie = |a: u32, b: u32| {
                let (wa, wb) = (&words[a as usize], &words[b as usize]);
                alphagrams[wa.alphagram as usize]
                    .cmp(&alphagrams[wb.alphagram as usize])
                    .then_with(|| wa.tiles.cmp(&wb.tiles))
            };
            let mut prob = Vec::with_capacity(bucket.len());
            rank(
                bucket,
                |i| words[i as usize].combinations,
                |a, b| b.cmp(&a),
                tie,
                |id, r, lo, hi| prob.push((id, r, lo, hi)),
            );
            let mut play = Vec::with_capacity(bucket.len());
            rank(
                bucket,
                |i| words[i as usize].playability,
                |a: f64, b: f64| b.total_cmp(&a),
                tie,
                |id, r, lo, hi| play.push((id, r, lo, hi)),
            );
            for (id, r, lo, hi) in prob {
                let w = &mut words[id as usize];
                (
                    w.probability_order,
                    w.min_probability_order,
                    w.max_probability_order,
                ) = (r, lo, hi);
            }
            for (id, r, lo, hi) in play {
                let w = &mut words[id as usize];
                (
                    w.playability_order,
                    w.min_playability_order,
                    w.max_playability_order,
                ) = (r, lo, hi);
            }
        }

        let max_num_anagrams = words.iter().map(|w| w.num_anagrams).max().unwrap_or(0);
        let max_order_rank = by_length.iter().map(|b| b.len() as u32).max().unwrap_or(0);
        let approx_bytes = words
            .iter()
            .map(|w| {
                std::mem::size_of::<WordEntry>()
                    + w.tiles.len() * 2
                    + w.counts.len()
                    + w.front_hooks.len()
                    + w.back_hooks.len()
                    + w.definition.len()
                    + 48
            })
            .sum::<usize>()
            + alphagrams.iter().map(|a| a.len() * 2 + 64).sum::<usize>();
        let build_ms = start.elapsed().as_millis() as u64;
        tracing::info!(
            lexicon = name,
            words = words.len(),
            build_ms,
            approx_bytes,
            "lexicon index built"
        );
        LexiconIndex {
            id,
            name: name.to_owned(),
            distribution,
            words,
            word_lookup,
            alphagrams,
            alphagram_words,
            alphagram_lookup,
            by_length,
            max_num_anagrams,
            max_order_rank,
            build_ms,
            approx_bytes,
        }
    }

    pub fn word_count(&self) -> usize {
        self.words.len()
    }

    pub fn find(&self, tiles: &[Tile]) -> Option<&WordEntry> {
        self.word_lookup
            .get(tiles)
            .map(|&i| &self.words[i as usize])
    }

    /// Anagram answers: every valid word with this alphagram, alphabetical.
    pub fn anagrams(&self, alphagram: &[Tile]) -> impl Iterator<Item = &WordEntry> {
        self.alphagram_lookup
            .get(alphagram)
            .into_iter()
            .flat_map(move |&a| {
                self.alphagram_words[a as usize]
                    .iter()
                    .map(move |&w| &self.words[w as usize])
            })
    }
}

/// One leave and its derived attributes.
#[derive(Debug, Clone)]
pub struct LeaveEntry {
    /// Canonical order: tile order, blank first.
    pub tiles: Box<[Tile]>,
    pub counts: Box<[u8]>,
    pub length: u8,
    pub num_vowels: u8,
    pub num_unique_letters: u8,
    pub point_value: u16,
    pub combinations: u128,
    pub probability_order: u32,
    pub min_probability_order: u32,
    pub max_probability_order: u32,
    /// Valid words in the lexicon using exactly these tiles; 0 with a blank.
    pub num_anagrams: u32,
    pub value: f64,
}

#[derive(Debug)]
pub struct LeaveSetIndex {
    pub id: i32,
    pub lexicon_id: i16,
    pub distribution: Arc<Distribution>,
    /// Sorted by canonical leave.
    pub leaves: Vec<LeaveEntry>,
    /// Canonical leave → entry: what a literal Anagram Match narrows by.
    pub lookup: HashMap<Box<[Tile]>, u32>,
    /// Leave ids per size (index = size).
    pub by_size: Vec<Vec<u32>>,
    pub max_num_anagrams: u32,
    pub max_order_rank: u32,
    pub build_ms: u64,
    pub approx_bytes: usize,
}

impl LeaveSetIndex {
    pub fn build(id: i32, lexicon: &LexiconIndex, mut raw: Vec<(Vec<Tile>, f64)>) -> Self {
        let start = Instant::now();
        let d = lexicon.distribution.clone();
        for (t, _) in raw.iter_mut() {
            t.sort_unstable();
        }
        raw.sort_by(|a, b| a.0.cmp(&b.0));
        raw.dedup_by(|a, b| a.0 == b.0);
        let mut by_size: Vec<Vec<u32>> = vec![Vec::new(); MAX_LEAVE_TILES + 1];
        let mut leaves: Vec<LeaveEntry> = Vec::with_capacity(raw.len());
        for (i, (tiles, value)) in raw.into_iter().enumerate() {
            let counts = d.counts(&tiles);
            by_size[tiles.len().min(MAX_LEAVE_TILES)].push(i as u32);
            let num_anagrams = if tiles.contains(&BLANK) {
                0
            } else {
                lexicon
                    .alphagram_lookup
                    .get(tiles.as_slice())
                    .map(|&a| lexicon.alphagram_words[a as usize].len() as u32)
                    .unwrap_or(0)
            };
            leaves.push(LeaveEntry {
                length: tiles.len() as u8,
                num_vowels: tiles
                    .iter()
                    .filter(|&&t| t != BLANK && d.is_vowel(t))
                    .count() as u8,
                num_unique_letters: counts.iter().filter(|&&c| c > 0).count() as u8,
                point_value: tiles.iter().map(|&t| d.value(t)).sum(),
                combinations: leave_combinations(&d, &counts),
                counts: counts.into_boxed_slice(),
                probability_order: 0,
                min_probability_order: 0,
                max_probability_order: 0,
                num_anagrams,
                value,
                tiles: tiles.into_boxed_slice(),
            });
        }
        for bucket in &by_size {
            let mut prob = Vec::with_capacity(bucket.len());
            // For a leave the alphagram and the "word" are the leave itself.
            rank(
                bucket,
                |i| leaves[i as usize].combinations,
                |a, b| b.cmp(&a),
                |a, b| leaves[a as usize].tiles.cmp(&leaves[b as usize].tiles),
                |id, r, lo, hi| prob.push((id, r, lo, hi)),
            );
            for (id, r, lo, hi) in prob {
                let l = &mut leaves[id as usize];
                (
                    l.probability_order,
                    l.min_probability_order,
                    l.max_probability_order,
                ) = (r, lo, hi);
            }
        }
        let lookup = leaves
            .iter()
            .enumerate()
            .map(|(i, l)| (l.tiles.clone(), i as u32))
            .collect();
        let max_num_anagrams = leaves.iter().map(|l| l.num_anagrams).max().unwrap_or(0);
        let max_order_rank = by_size.iter().map(|b| b.len() as u32).max().unwrap_or(0);
        let approx_bytes = leaves
            .iter()
            .map(|l| std::mem::size_of::<LeaveEntry>() + l.tiles.len() * 2 + l.counts.len() + 48)
            .sum();
        let build_ms = start.elapsed().as_millis() as u64;
        tracing::info!(
            leave_set = id,
            leaves = leaves.len(),
            build_ms,
            approx_bytes,
            "leave set index built"
        );
        LeaveSetIndex {
            id,
            lexicon_id: lexicon.id,
            distribution: d,
            leaves,
            lookup,
            by_size,
            max_num_anagrams,
            max_order_rank,
            build_ms,
            approx_bytes,
        }
    }

    pub fn leave_count(&self) -> usize {
        self.leaves.len()
    }

    pub fn find(&self, canonical: &[Tile]) -> Option<&LeaveEntry> {
        self.lookup
            .get(canonical)
            .map(|&i| &self.leaves[i as usize])
    }
}
