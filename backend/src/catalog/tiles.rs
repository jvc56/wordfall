//! Tiles (PLAN.md § Tiles).
//!
//! Words, alphagrams, leaves, patterns and question keys are sequences of
//! tiles. A tile is its index in the distribution, which is its line number in
//! the distribution file from 0, so index 0 is the blank and the natural order
//! of indexes is **tile order**. "Alphabetical order" everywhere in the plan is
//! the lexicographic order of tile-index slices, a prefix sorting first, which
//! is exactly `Ord` on `[Tile]`.

use std::collections::HashMap;
use std::fmt;

pub type Tile = u8;
pub const BLANK: Tile = 0;
/// A tile index is a `u8`, so a distribution holds at most 256 tiles (PQ-006).
pub const MAX_TILES: usize = 256;
/// `letter` and `blank_letter` are at most 8 bytes of UTF-8.
pub const MAX_LETTER_BYTES: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileDef {
    pub letter: String,
    pub blank_letter: String,
    pub count: u16,
    pub value: u16,
    pub is_vowel: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotationError {
    UnclosedBracket,
    UnmatchedBracket,
    EmptyBracket,
    NestedBracket,
    /// `[A]`: a single-character tile is never bracketed.
    BracketedSingle(String),
    /// A multi-character tile written without brackets cannot occur (it reads
    /// as its characters), so this is only for bracket contents.
    UnknownTile(String),
    /// A tile's `blank_letter`: a blank standing for a tile, which Wordfall never stores.
    BlankLetter(String),
    /// `?` where no blank is allowed.
    Blank,
    Empty,
}

impl fmt::Display for NotationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotationError::UnclosedBracket => write!(f, "a '[' is never closed"),
            NotationError::UnmatchedBracket => write!(f, "a ']' has no matching '['"),
            NotationError::EmptyBracket => write!(f, "empty brackets '[]'"),
            NotationError::NestedBracket => write!(f, "brackets cannot be nested"),
            NotationError::BracketedSingle(t) => {
                write!(
                    f,
                    "'[{t}]': single-character tiles are written without brackets"
                )
            }
            NotationError::UnknownTile(t) => write!(f, "'{t}' is not a tile of this distribution"),
            NotationError::BlankLetter(t) => write!(
                f,
                "'{t}' is a lower-case tile, which means a blank standing for a tile"
            ),
            NotationError::Blank => write!(f, "'?' (the blank) is not allowed here"),
            NotationError::Empty => write!(f, "no tiles"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Distribution {
    pub id: i16,
    pub name: String,
    pub tiles: Vec<TileDef>,
    by_letter: HashMap<String, Tile>,
    by_blank_letter: HashMap<String, Tile>,
    /// Letters longest first, for greedy matching of typed text.
    letters_by_len: Vec<(Vec<char>, Tile)>,
}

impl Distribution {
    /// `tiles[0]` must be the blank (`?`). Callers validate before building.
    pub fn new(id: i16, name: impl Into<String>, tiles: Vec<TileDef>) -> Self {
        assert!(tiles.len() <= MAX_TILES);
        let mut by_letter = HashMap::new();
        let mut by_blank_letter = HashMap::new();
        let mut letters_by_len = Vec::new();
        for (i, t) in tiles.iter().enumerate() {
            let i = i as Tile;
            by_letter.insert(t.letter.clone(), i);
            if i != BLANK {
                by_blank_letter.insert(t.blank_letter.clone(), i);
                letters_by_len.push((t.letter.chars().collect::<Vec<_>>(), i));
            }
        }
        letters_by_len.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.1.cmp(&b.1)));
        Distribution {
            id,
            name: name.into(),
            tiles,
            by_letter,
            by_blank_letter,
            letters_by_len,
        }
    }

    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    pub fn tile(&self, t: Tile) -> &TileDef {
        &self.tiles[t as usize]
    }

    pub fn tile_for_letter(&self, letter: &str) -> Option<Tile> {
        self.by_letter.get(letter).copied()
    }

    pub fn blank_count(&self) -> u16 {
        self.tiles.first().map(|t| t.count).unwrap_or(0)
    }

    pub fn max_value(&self) -> u16 {
        self.tiles.iter().map(|t| t.value).max().unwrap_or(0)
    }

    pub fn is_vowel(&self, t: Tile) -> bool {
        self.tiles[t as usize].is_vowel
    }

    pub fn value(&self, t: Tile) -> u16 {
        self.tiles[t as usize].value
    }

    /// A tile written in MAGPIE notation: its letter, bracketed when it is
    /// more than one character.
    pub fn write_tile(&self, t: Tile, out: &mut String) {
        let letter = &self.tiles[t as usize].letter;
        if letter.chars().count() > 1 {
            out.push('[');
            out.push_str(letter);
            out.push(']');
        } else {
            out.push_str(letter);
        }
    }

    pub fn to_magpie(&self, tiles: &[Tile]) -> String {
        let mut s = String::new();
        for &t in tiles {
            self.write_tile(t, &mut s);
        }
        s
    }

    /// Display form: each tile's letter without brackets.
    pub fn to_display(&self, tiles: &[Tile]) -> String {
        tiles
            .iter()
            .map(|&t| self.tiles[t as usize].letter.as_str())
            .collect()
    }

    /// Parses MAGPIE notation strictly, as MAGPIE's `ld_str_to_mls` does:
    /// single-character tiles as they are, multi-character tiles in brackets.
    /// Lower-case (blank) tiles are errors; `?` is the blank when allowed.
    pub fn parse_magpie(&self, s: &str, allow_blank: bool) -> Result<Vec<Tile>, NotationError> {
        let mut out = Vec::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            match c {
                '[' => {
                    let mut inner = String::new();
                    let mut closed = false;
                    for d in chars.by_ref() {
                        match d {
                            ']' => {
                                closed = true;
                                break;
                            }
                            '[' => return Err(NotationError::NestedBracket),
                            _ => inner.push(d),
                        }
                    }
                    if !closed {
                        return Err(NotationError::UnclosedBracket);
                    }
                    if inner.is_empty() {
                        return Err(NotationError::EmptyBracket);
                    }
                    if inner.chars().count() == 1 {
                        return Err(NotationError::BracketedSingle(inner));
                    }
                    out.push(self.lookup(&inner, allow_blank)?);
                }
                ']' => return Err(NotationError::UnmatchedBracket),
                _ => {
                    let s = c.to_string();
                    let t = self.lookup(&s, allow_blank)?;
                    if self.tiles[t as usize].letter.chars().count() != 1 {
                        return Err(NotationError::UnknownTile(s));
                    }
                    out.push(t);
                }
            }
        }
        if out.is_empty() {
            return Err(NotationError::Empty);
        }
        Ok(out)
    }

    fn lookup(&self, s: &str, allow_blank: bool) -> Result<Tile, NotationError> {
        if s == "?" {
            return if allow_blank {
                Ok(BLANK)
            } else {
                Err(NotationError::Blank)
            };
        }
        if let Some(&t) = self.by_letter.get(s) {
            return Ok(t);
        }
        if self.by_blank_letter.contains_key(s) {
            return Err(NotationError::BlankLetter(s.to_owned()));
        }
        Err(NotationError::UnknownTile(s.to_owned()))
    }

    /// Converts typed text to tiles (PLAN.md § Tiles → Typed text): upper-case
    /// it, treat a bracketed group as one multi-character tile, split the rest
    /// at spaces and match each piece greedily, longest tile first, with no
    /// backtracking. `?` is the blank when `allow_blank`.
    pub fn parse_typed(&self, s: &str, allow_blank: bool) -> Result<Vec<Tile>, NotationError> {
        let upper = s.to_uppercase();
        let mut out = Vec::new();
        let mut piece = String::new();
        let mut chars = upper.chars();
        while let Some(c) = chars.next() {
            match c {
                '[' => {
                    self.greedy(&piece, allow_blank, &mut out)?;
                    piece.clear();
                    let mut inner = String::new();
                    let mut closed = false;
                    for d in chars.by_ref() {
                        match d {
                            ']' => {
                                closed = true;
                                break;
                            }
                            '[' => return Err(NotationError::NestedBracket),
                            _ => inner.push(d),
                        }
                    }
                    if !closed {
                        return Err(NotationError::UnclosedBracket);
                    }
                    if inner.is_empty() {
                        return Err(NotationError::EmptyBracket);
                    }
                    out.push(self.lookup(&inner, allow_blank)?);
                }
                ']' => return Err(NotationError::UnmatchedBracket),
                c if c.is_whitespace() => {
                    self.greedy(&piece, allow_blank, &mut out)?;
                    piece.clear();
                }
                c => piece.push(c),
            }
        }
        self.greedy(&piece, allow_blank, &mut out)?;
        if out.is_empty() {
            return Err(NotationError::Empty);
        }
        Ok(out)
    }

    /// Greedy longest-first matching of one space-free piece.
    pub fn greedy(
        &self,
        piece: &str,
        allow_blank: bool,
        out: &mut Vec<Tile>,
    ) -> Result<(), NotationError> {
        let chars: Vec<char> = piece.chars().collect();
        let mut i = 0;
        'outer: while i < chars.len() {
            if chars[i] == '?' {
                if !allow_blank {
                    return Err(NotationError::Blank);
                }
                out.push(BLANK);
                i += 1;
                continue;
            }
            for (letter, t) in &self.letters_by_len {
                if chars[i..].starts_with(letter) {
                    out.push(*t);
                    i += letter.len();
                    continue 'outer;
                }
            }
            return Err(NotationError::UnknownTile(chars[i..].iter().collect()));
        }
        Ok(())
    }

    /// Canonical leave order: tile order, blank first.
    pub fn canonical_leave(tiles: &[Tile]) -> Vec<Tile> {
        let mut v = tiles.to_vec();
        v.sort_unstable();
        v
    }

    /// Per-tile counts, sized to the distribution.
    pub fn counts(&self, tiles: &[Tile]) -> Vec<u8> {
        let mut c = vec![0u8; self.tiles.len()];
        for &t in tiles {
            c[t as usize] = c[t as usize].saturating_add(1);
        }
        c
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn tiledef(letter: &str, blank: &str, count: u16, value: u16, vowel: bool) -> TileDef {
        TileDef {
            letter: letter.into(),
            blank_letter: blank.into(),
            count,
            value,
            is_vowel: vowel,
        }
    }

    /// A small Catalan-style distribution: `?`, A, C, Ç, E, L, L·L, N, NY, QU, S, Y.
    pub fn catalan_like() -> Distribution {
        Distribution::new(
            2,
            "cat",
            vec![
                tiledef("?", "?", 2, 0, false),
                tiledef("A", "a", 12, 1, true),
                tiledef("C", "c", 3, 2, false),
                tiledef("Ç", "ç", 1, 10, false),
                tiledef("E", "e", 13, 1, true),
                tiledef("L", "l", 4, 1, false),
                tiledef("L·L", "l·l", 1, 10, false),
                tiledef("N", "n", 6, 1, false),
                tiledef("NY", "ny", 1, 10, false),
                tiledef("QU", "qu", 1, 8, false),
                tiledef("S", "s", 8, 1, false),
                tiledef("Y", "y", 1, 4, false),
            ],
        )
    }

    #[test]
    fn bracketed_and_typed_forms_give_three_tiles() {
        // PLAN.md § Unit tests → Tile handling: `A[NY]S` and typed `ANYS`.
        let d = catalan_like();
        let a = d.parse_magpie("A[NY]S", false).unwrap();
        assert_eq!(a.len(), 3);
        assert_eq!(d.parse_typed("anys", false).unwrap(), a);
        assert_eq!(d.to_magpie(&a), "A[NY]S");
        assert_eq!(d.to_display(&a), "ANYS");
    }

    #[test]
    fn a_space_separates_tiles() {
        // Palette-inserted `N` + typed `Y` stay two tiles; `AN YS` is A N Y S.
        let d = catalan_like();
        assert_eq!(d.parse_typed(" N  Y ", false).unwrap().len(), 2);
        assert_eq!(d.parse_typed("AN YS", false).unwrap().len(), 4);
        assert_eq!(
            d.parse_typed("cel·la", false).unwrap().len(),
            4,
            "C E L·L A"
        );
    }

    #[test]
    fn greedy_matching_never_backtracks() {
        let d = catalan_like();
        // "QUE" is QU E; a lone "Q" is not a tile and greedy cannot recover.
        assert_eq!(d.parse_typed("QUE", false).unwrap().len(), 2);
        assert!(matches!(
            d.parse_typed("QE", false),
            Err(NotationError::UnknownTile(_))
        ));
    }

    #[test]
    fn malformed_notation_is_rejected_as_in_magpie() {
        let d = catalan_like();
        assert_eq!(
            d.parse_magpie("[", false),
            Err(NotationError::UnclosedBracket)
        );
        assert_eq!(
            d.parse_magpie("[]", false),
            Err(NotationError::EmptyBracket)
        );
        assert!(matches!(
            d.parse_magpie("[A]", false),
            Err(NotationError::BracketedSingle(_))
        ));
        assert_eq!(
            d.parse_magpie("[[NY]]", false),
            Err(NotationError::NestedBracket)
        );
        assert_eq!(
            d.parse_magpie("A]", false),
            Err(NotationError::UnmatchedBracket)
        );
        assert!(matches!(
            d.parse_magpie("a", false),
            Err(NotationError::BlankLetter(_))
        ));
        assert!(matches!(
            d.parse_magpie("[ny]", false),
            Err(NotationError::BlankLetter(_))
        ));
        assert_eq!(d.parse_magpie("A?", false), Err(NotationError::Blank));
        assert_eq!(d.parse_magpie("A?", true).unwrap(), vec![1, BLANK]);
        assert_eq!(d.parse_magpie("", false), Err(NotationError::Empty));
        // Unbracketed "NY" reads as N then Y.
        assert_eq!(d.parse_magpie("NY", false).unwrap().len(), 2);
    }

    #[test]
    fn sort_order_follows_the_distribution_with_the_blank_first() {
        let d = catalan_like();
        let leave = d.parse_magpie("S[L·L]?A", true).unwrap();
        let canon = Distribution::canonical_leave(&leave);
        assert_eq!(d.to_magpie(&canon), "?A[L·L]S");
        // L·L sorts after L, not between LA and LZ.
        let l = d.parse_magpie("LS", false).unwrap();
        let ll = d.parse_magpie("[L·L]", false).unwrap();
        let la = d.parse_magpie("LA", false).unwrap();
        assert!(la < ll && l < ll);
        // A prefix sorts first.
        assert!(d.parse_magpie("CA", false).unwrap() < d.parse_magpie("CAS", false).unwrap());
    }

    #[test]
    fn vowels_and_points_come_from_the_distribution() {
        let d = catalan_like();
        let w = d.parse_magpie("[QU]E[L·L]A", false).unwrap();
        assert_eq!(w.iter().filter(|&&t| d.is_vowel(t)).count(), 2);
        assert_eq!(w.iter().map(|&t| d.value(t)).sum::<u16>(), 8 + 1 + 10 + 1);
    }
}
