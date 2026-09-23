//! Patterns (PLAN.md § Pattern syntax) and their matchers (§ Search Engine →
//! Pattern matching).
//!
//! The canonical text form is tokens separated by single spaces: `.` any
//! single tile, `*` any run, `?` the blank, a tile's letter, or a set
//! `[A NY]` whose tiles are separated by single spaces. Brackets in a pattern
//! always mean a set, never MAGPIE's multi-character notation.

use crate::catalog::tiles::{Distribution, Tile, BLANK};

/// A set of tiles as a 256-bit bitset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TileSet([u64; 4]);

impl TileSet {
    pub fn all() -> Self {
        TileSet([u64::MAX; 4])
    }

    pub fn insert(&mut self, t: Tile) {
        self.0[(t >> 6) as usize] |= 1 << (t & 63);
    }

    pub fn contains(&self, t: Tile) -> bool {
        self.0[(t >> 6) as usize] & (1 << (t & 63)) != 0
    }

    pub fn len(&self) -> u32 {
        self.0.iter().map(|w| w.count_ones()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn from_tiles(tiles: &[Tile]) -> Self {
        let mut s = TileSet::default();
        for &t in tiles {
            s.insert(t);
        }
        s
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    Tile(Tile),
    Any,
    Star,
    Set(TileSet, Vec<Tile>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    pub toks: Vec<Tok>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatternError {
    /// Not in canonical form (spacing, case, an empty token).
    NotCanonical,
    UnbalancedBracket,
    EmptyBracket,
    /// `?` where the quiz type has no blank.
    Blank,
    UnknownTile(String),
    Empty,
}

impl std::fmt::Display for PatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatternError::NotCanonical => write!(f, "the pattern is not in canonical form"),
            PatternError::UnbalancedBracket => write!(f, "unbalanced bracket"),
            PatternError::EmptyBracket => write!(f, "empty brackets"),
            PatternError::Blank => write!(f, "use `.` for any single tile"),
            PatternError::UnknownTile(t) => write!(f, "'{t}' is not a tile of this distribution"),
            PatternError::Empty => write!(f, "the pattern is empty"),
        }
    }
}

/// A token of canonical text before tiles are resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawTok<'a> {
    Any,
    Star,
    Blank,
    Letter(&'a str),
    Set(Vec<&'a str>),
}

/// Splits canonical pattern text into tokens, checking only the syntax, so
/// it runs without a distribution (a saved search carries none).
pub fn tokenize(text: &str) -> Result<Vec<RawTok<'_>>, PatternError> {
    if text.is_empty() {
        return Err(PatternError::Empty);
    }
    let mut out = Vec::new();
    let mut rest = text;
    loop {
        let (tok, after) = if let Some(inner_start) = rest.strip_prefix('[') {
            let Some(end) = inner_start.find(']') else {
                return Err(PatternError::UnbalancedBracket);
            };
            let inner = &inner_start[..end];
            if inner.contains('[') {
                return Err(PatternError::UnbalancedBracket);
            }
            if inner.is_empty() {
                return Err(PatternError::EmptyBracket);
            }
            let tiles: Vec<&str> = inner.split(' ').collect();
            if tiles.iter().any(|t| t.is_empty()) {
                return Err(PatternError::NotCanonical);
            }
            (RawTok::Set(tiles), &inner_start[end + 1..])
        } else {
            let end = rest.find(' ').unwrap_or(rest.len());
            let tok = &rest[..end];
            let raw = match tok {
                "" => return Err(PatternError::NotCanonical),
                "." => RawTok::Any,
                "*" => RawTok::Star,
                "?" => RawTok::Blank,
                t if t.contains(']') => return Err(PatternError::UnbalancedBracket),
                t if t.contains('[') => return Err(PatternError::UnbalancedBracket),
                t => RawTok::Letter(t),
            };
            (raw, &rest[end..])
        };
        if let RawTok::Letter(l) = &tok {
            if l.chars().any(|c| c.is_lowercase()) {
                return Err(PatternError::NotCanonical);
            }
        }
        if let RawTok::Set(tiles) = &tok {
            if tiles.iter().any(|l| l.chars().any(|c| c.is_lowercase())) {
                return Err(PatternError::NotCanonical);
            }
        }
        out.push(tok);
        if after.is_empty() {
            return Ok(out);
        }
        match after.strip_prefix(' ') {
            Some(r) if !r.is_empty() && !r.starts_with(' ') => rest = r,
            _ => return Err(PatternError::NotCanonical),
        }
    }
}

/// Whether the tokens hold the blank, which only a leave pattern may.
pub fn has_blank(toks: &[RawTok<'_>]) -> bool {
    toks.iter().any(|t| match t {
        RawTok::Blank => true,
        RawTok::Set(s) => s.contains(&"?"),
        _ => false,
    })
}

/// Resolves canonical pattern text against a distribution.
pub fn parse(text: &str, dist: &Distribution, allow_blank: bool) -> Result<Pattern, PatternError> {
    let raw = tokenize(text)?;
    if !allow_blank && has_blank(&raw) {
        return Err(PatternError::Blank);
    }
    let tile = |l: &str| -> Result<Tile, PatternError> {
        if l == "?" {
            return Ok(BLANK);
        }
        dist.tile_for_letter(l)
            .filter(|&t| t != BLANK)
            .ok_or_else(|| PatternError::UnknownTile(l.to_owned()))
    };
    let mut toks = Vec::with_capacity(raw.len());
    for r in raw {
        toks.push(match r {
            RawTok::Any => Tok::Any,
            RawTok::Star => Tok::Star,
            RawTok::Blank => Tok::Tile(BLANK),
            RawTok::Letter(l) => Tok::Tile(tile(l)?),
            RawTok::Set(ls) => {
                let tiles = ls.into_iter().map(tile).collect::<Result<Vec<_>, _>>()?;
                Tok::Set(TileSet::from_tiles(&tiles), tiles)
            }
        });
    }
    Ok(Pattern { toks })
}

/// Canonical text for a pattern (what `parse` accepts).
pub fn to_text(p: &Pattern, dist: &Distribution) -> String {
    let letter = |t: Tile| if t == BLANK { "?".to_owned() } else { dist.tile(t).letter.clone() };
    p.toks
        .iter()
        .map(|t| match t {
            Tok::Any => ".".to_owned(),
            Tok::Star => "*".to_owned(),
            Tok::Tile(t) => letter(*t),
            Tok::Set(_, tiles) => {
                format!("[{}]", tiles.iter().map(|&t| letter(t)).collect::<Vec<_>>().join(" "))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Anagram and subanagram patterns as literal counts plus wildcard slots.
#[derive(Debug, Clone)]
pub struct Bag {
    pub literals: Vec<u8>,
    pub literal_total: usize,
    /// One per `.` (all tiles) or set, most constrained first.
    pub slots: Vec<TileSet>,
    pub star: bool,
}

impl Bag {
    pub fn new(p: &Pattern, ntiles: usize) -> Self {
        let mut literals = vec![0u8; ntiles];
        let mut slots = Vec::new();
        let mut star = false;
        let mut total = 0;
        for t in &p.toks {
            match t {
                Tok::Tile(t) => {
                    literals[*t as usize] += 1;
                    total += 1;
                }
                Tok::Any => slots.push(TileSet::all()),
                Tok::Set(s, _) => slots.push(*s),
                Tok::Star => star = true,
            }
        }
        slots.sort_by_key(|s| s.len());
        Bag { literals, literal_total: total, slots, star }
    }

    pub fn only_literals(&self) -> bool {
        self.slots.is_empty() && !self.star
    }

    /// Anagram Match: the word uses exactly the pattern's tiles; `*` allows extras.
    pub fn anagram(&self, counts: &[u8], len: usize) -> bool {
        let need = self.literal_total + self.slots.len();
        if (self.star && len < need) || (!self.star && len != need) {
            return false;
        }
        let mut rem = [0u8; 256];
        for (t, (&c, &l)) in counts.iter().zip(&self.literals).enumerate() {
            if c < l {
                return false;
            }
            rem[t] = c - l;
        }
        if self.slots.is_empty() {
            return true;
        }
        // Without `*`, every remaining tile fills a slot; with it, the slots
        // take any of the remaining tiles and the rest are the extras.
        assign(&self.slots, 0, &mut rem, counts.len())
    }

    /// Subanagram Match: every tile of the word can be taken from the pattern.
    pub fn subanagram(&self, counts: &[u8], len: usize) -> bool {
        if self.star {
            return true;
        }
        if len > self.literal_total + self.slots.len() {
            return false;
        }
        let mut left: Vec<Tile> = Vec::new();
        for (t, (&c, &l)) in counts.iter().zip(&self.literals).enumerate() {
            for _ in l..c.max(l) {
                left.push(t as Tile);
            }
        }
        if left.is_empty() {
            return true;
        }
        if left.len() > self.slots.len() {
            return false;
        }
        let mut used = vec![false; self.slots.len()];
        place(&left, 0, &self.slots, &mut used)
    }
}

/// Fills every slot (from `i`) with a distinct remaining tile it accepts.
fn assign(slots: &[TileSet], i: usize, rem: &mut [u8; 256], ntiles: usize) -> bool {
    if i == slots.len() {
        return true;
    }
    for t in 0..ntiles {
        if rem[t] > 0 && slots[i].contains(t as Tile) {
            rem[t] -= 1;
            let ok = assign(slots, i + 1, rem, ntiles);
            rem[t] += 1;
            if ok {
                return true;
            }
        }
    }
    false
}

/// Places every leftover word tile (from `i`) into a distinct accepting slot.
fn place(left: &[Tile], i: usize, slots: &[TileSet], used: &mut [bool]) -> bool {
    if i == left.len() {
        return true;
    }
    for s in 0..slots.len() {
        if !used[s] && slots[s].contains(left[i]) {
            used[s] = true;
            if place(left, i + 1, slots, used) {
                return true;
            }
            used[s] = false;
        }
    }
    false
}

/// Pattern Match: an anchored match of the tokens against the tiles in order.
pub fn matches_in_order(p: &Pattern, tiles: &[Tile]) -> bool {
    // dp[j]: tokens [0, i) can match tiles [0, j).
    let n = tiles.len();
    let mut dp = vec![false; n + 1];
    dp[0] = true;
    for tok in &p.toks {
        let mut next = vec![false; n + 1];
        match tok {
            Tok::Star => {
                let mut any = false;
                for j in 0..=n {
                    any |= dp[j];
                    next[j] = any;
                }
            }
            _ => {
                for j in 0..n {
                    if dp[j] {
                        let ok = match tok {
                            Tok::Tile(t) => tiles[j] == *t,
                            Tok::Any => true,
                            Tok::Set(s, _) => s.contains(tiles[j]),
                            Tok::Star => unreachable!(),
                        };
                        if ok {
                            next[j + 1] = true;
                        }
                    }
                }
            }
        }
        dp = next;
    }
    dp[n]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::tiles::tests::catalan_like;

    #[test]
    fn canonical_form() {
        // `N Y`, `[A NY]` and `[ANY]` tokenise as the canonical form says.
        let d = catalan_like();
        let p = parse("N Y", &d, false).unwrap();
        assert_eq!(p.toks.len(), 2);
        let set = parse("[A NY]", &d, false).unwrap();
        assert!(matches!(&set.toks[0], Tok::Set(s, _) if s.len() == 2));
        // `[ANY]` is not canonical: the client sends `[A NY]`.
        assert!(matches!(parse("[ANY]", &d, false), Err(PatternError::UnknownTile(_))));
        for bad in ["N  Y", " N", "N ", "", "n", "[A  NY]", "[A NY", "A]", "[]", "[[A]]"] {
            assert!(parse(bad, &d, false).is_err(), "{bad:?} accepted");
        }
        assert_eq!(parse("? A", &d, false), Err(PatternError::Blank));
        assert!(parse("? A", &d, true).is_ok());
        let p = parse(". [A NY] * S", &d, false).unwrap();
        assert_eq!(to_text(&p, &d), ". [A NY] * S");
    }

    #[test]
    fn in_order_matching() {
        let d = catalan_like();
        let w = d.parse_magpie("CA[NY]A", false).unwrap();
        assert!(matches_in_order(&parse("C . NY A", &d, false).unwrap(), &w));
        assert!(matches_in_order(&parse("C *", &d, false).unwrap(), &w));
        assert!(matches_in_order(&parse("* A", &d, false).unwrap(), &w));
        assert!(matches_in_order(&parse("* [NY S] *", &d, false).unwrap(), &w));
        assert!(!matches_in_order(&parse("C .", &d, false).unwrap(), &w));
        assert!(!matches_in_order(&parse("C A N Y A", &d, false).unwrap(), &w));
    }

    #[test]
    fn bags() {
        let d = catalan_like();
        let casa = d.parse_magpie("CASA", false).unwrap();
        let c = d.counts(&casa);
        let bag = |s: &str| Bag::new(&parse(s, &d, false).unwrap(), d.len());
        assert!(bag("S A C A").anagram(&c, 4));
        assert!(bag("S A C .").anagram(&c, 4));
        assert!(bag("[A S] A C [A S]").anagram(&c, 4));
        assert!(!bag("[C E] A C [A S]").anagram(&c, 4));
        assert!(bag("C *").anagram(&c, 4));
        assert!(!bag("C . *").anagram(&d.counts(&casa[..1]), 1));
        assert!(bag("C A S A E").subanagram(&c, 4));
        assert!(bag("C S . . E").subanagram(&c, 4));
        assert!(!bag("C S . E").subanagram(&c, 4));
        assert!(bag("L *").subanagram(&c, 4));
    }
}
