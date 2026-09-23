//! Upload validation (PLAN.md § Admin → File formats). Every upload is
//! validated in full before anything is written; problems are listed with
//! line numbers, the first 1,000 plus a total count.

use std::collections::HashMap;

use serde::Serialize;

use super::index::{MAX_LEAVE_TILES, MAX_WORD_TILES};
use super::tiles::{
    BLANK, Distribution, MAX_LETTER_BYTES, MAX_TILES, NotationError, Tile, TileDef,
};

pub const MAX_REPORTED_ERRORS: usize = 1000;
/// Leave values are bounded so the shared rounding stays inside 64 bits.
pub const MAX_ABS_LEAVE_VALUE: f64 = 1_000_000.0;
pub const MAX_DEFINITION_CHARS: usize = 10_000;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LineError {
    /// 1-based line number; `None` for an error about the form rather than a line (PQ-007).
    pub line: Option<u32>,
    pub message: String,
}

#[derive(Debug, Default, Clone, Serialize, PartialEq)]
pub struct UploadErrors {
    pub errors: Vec<LineError>,
    pub total_errors: usize,
}

impl UploadErrors {
    pub fn push(&mut self, line: Option<u32>, message: impl Into<String>) {
        self.total_errors += 1;
        if self.errors.len() < MAX_REPORTED_ERRORS {
            self.errors.push(LineError {
                line,
                message: message.into(),
            });
        }
    }

    pub fn is_empty(&self) -> bool {
        self.total_errors == 0
    }

    pub fn single(line: Option<u32>, message: impl Into<String>) -> Self {
        let mut e = UploadErrors::default();
        e.push(line, message);
        e
    }
}

/// The records of a file: (1-based line number, trimmed fields). Handles the
/// BOM, LF or CRLF, blank lines, and invalid UTF-8 (reported per line).
fn records<'a>(
    bytes: &'a [u8],
    delim: char,
    errors: &mut UploadErrors,
) -> Vec<(u32, Vec<&'a str>)> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let mut out = Vec::new();
    for (i, raw) in bytes.split(|&b| b == b'\n').enumerate() {
        let line_no = (i + 1) as u32;
        let raw = raw.strip_suffix(b"\r").unwrap_or(raw);
        let Ok(line) = std::str::from_utf8(raw) else {
            errors.push(Some(line_no), "not valid UTF-8");
            continue;
        };
        if line.trim().is_empty() {
            continue;
        }
        out.push((line_no, line.split(delim).map(str::trim).collect()));
    }
    out
}

/// Plain decimals: an optional sign, digits, and an optional `.` and digits.
pub fn parse_decimal(s: &str) -> Option<f64> {
    let body = s.strip_prefix(['+', '-']).unwrap_or(s);
    let (int, frac) = match body.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };
    if int.is_empty() || !int.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if let Some(f) = frac {
        if f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    let v: f64 = s.parse().ok()?;
    v.is_finite().then_some(v)
}

fn parse_small_int(s: &str) -> Option<u16> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<u16>().ok().filter(|&v| v <= i16::MAX as u16)
}

fn bad_letter_char(c: char) -> bool {
    matches!(c, '[' | ']' | ',' | '?' | '*' | '.') || c.is_whitespace()
}

// ---------------------------------------------------------------------------
// Letter distribution
// ---------------------------------------------------------------------------

pub fn is_valid_distribution_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'_' | b'-'))
}

/// A parsed distribution file: one `TileDef` per line, plus fullwidth forms.
#[derive(Debug, Clone)]
pub struct ParsedDistribution {
    pub tiles: Vec<TileDef>,
    pub fullwidth: Vec<Option<(String, String)>>,
}

pub fn parse_distribution(bytes: &[u8]) -> Result<ParsedDistribution, UploadErrors> {
    let mut errors = UploadErrors::default();
    let recs = records(bytes, ',', &mut errors);
    let mut tiles = Vec::new();
    let mut fullwidth = Vec::new();
    let mut letters: HashMap<String, u32> = HashMap::new();
    let mut blank_letters: HashMap<String, u32> = HashMap::new();
    for (idx, (line, f)) in recs.iter().enumerate() {
        let line = *line;
        if f.len() != 5 && f.len() != 7 {
            errors.push(
                Some(line),
                format!("expected 5 or 7 fields, found {}", f.len()),
            );
            continue;
        }
        let first = idx == 0;
        let (letter, blank_letter) = (f[0], f[1]);
        let count = parse_small_int(f[2]);
        let value = parse_small_int(f[3]);
        let is_vowel = match f[4] {
            "1" => Some(true),
            "0" => Some(false),
            _ => None,
        };
        let mut ok = true;
        if count.is_none() {
            errors.push(
                Some(line),
                format!("count {:?} is not a non-negative integer", f[2]),
            );
            ok = false;
        }
        if value.is_none() {
            errors.push(
                Some(line),
                format!("value {:?} is not a non-negative integer", f[3]),
            );
            ok = false;
        }
        if is_vowel.is_none() {
            errors.push(Some(line), format!("is_vowel {:?} must be 1 or 0", f[4]));
            ok = false;
        }
        let fw = if f.len() == 7 {
            if f[5].is_empty() || f[6].is_empty() {
                errors.push(Some(line), "give both fullwidth forms or neither");
                ok = false;
                None
            } else {
                Some((f[5].to_owned(), f[6].to_owned()))
            }
        } else {
            None
        };
        if first {
            // The first line is the blank: `?,?,<count>,0,0`.
            if letter != "?" || blank_letter != "?" || value != Some(0) || is_vowel != Some(false) {
                errors.push(
                    Some(line),
                    "the first line must be the blank: ?,?,<count>,0,0",
                );
                ok = false;
            }
        } else {
            for (field, s) in [("letter", letter), ("blank_letter", blank_letter)] {
                if s.is_empty() {
                    errors.push(Some(line), format!("{field} is empty"));
                    ok = false;
                } else if s.len() > MAX_LETTER_BYTES {
                    errors.push(
                        Some(line),
                        format!("{field} {s:?} is longer than {MAX_LETTER_BYTES} bytes"),
                    );
                    ok = false;
                } else if s.chars().any(bad_letter_char) {
                    errors.push(
                        Some(line),
                        format!("{field} {s:?} contains one of [ ] , ? * . or whitespace"),
                    );
                    ok = false;
                }
            }
        }
        if !first {
            if let Some(prev) = letters.get(letter) {
                errors.push(Some(line), format!("letter {letter:?} repeats line {prev}"));
                ok = false;
            }
            if let Some(prev) = blank_letters.get(blank_letter) {
                errors.push(
                    Some(line),
                    format!("blank_letter {blank_letter:?} repeats line {prev}"),
                );
                ok = false;
            }
        }
        letters.entry(letter.to_owned()).or_insert(line);
        if !first {
            blank_letters.entry(blank_letter.to_owned()).or_insert(line);
        }
        if ok {
            tiles.push(TileDef {
                letter: letter.to_owned(),
                blank_letter: blank_letter.to_owned(),
                count: count.unwrap_or(0),
                value: value.unwrap_or(0),
                is_vowel: is_vowel.unwrap_or(false),
            });
            fullwidth.push(fw);
        }
    }
    // No blank_letter may equal any tile's letter, so upper-cased typed text
    // maps to one tile.
    for (line, f) in recs.iter().skip(1) {
        if f.len() == 5 || f.len() == 7 {
            if let Some(other) = letters.get(f[1]) {
                errors.push(
                    Some(*line),
                    format!("blank_letter {:?} is the letter of line {other}", f[1]),
                );
            }
        }
    }
    if recs.is_empty() {
        errors.push(None, "the file has no lines");
    }
    if tiles.len() > MAX_TILES {
        errors.push(
            None,
            format!("a distribution holds at most {MAX_TILES} tiles"),
        );
    }
    if errors.is_empty() {
        Ok(ParsedDistribution { tiles, fullwidth })
    } else {
        Err(errors)
    }
}

// ---------------------------------------------------------------------------
// Lexicon
// ---------------------------------------------------------------------------

pub fn is_valid_lexicon_name(name: &str) -> bool {
    (1..=32).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

#[derive(Debug, Clone)]
pub struct ParsedWord {
    pub tiles: Vec<Tile>,
    /// Canonical MAGPIE notation, as stored.
    pub word: String,
    pub playability: f64,
    pub definition: String,
}

fn notation_message(e: &NotationError) -> String {
    e.to_string()
}

pub fn parse_lexicon(bytes: &[u8], dist: &Distribution) -> Result<Vec<ParsedWord>, UploadErrors> {
    let mut errors = UploadErrors::default();
    let recs = records(bytes, '\t', &mut errors);
    let mut seen: HashMap<Vec<Tile>, u32> = HashMap::new();
    let mut words = Vec::with_capacity(recs.len());
    for (line, f) in recs {
        if f.len() != 3 {
            errors.push(
                Some(line),
                format!("expected 3 tab-separated fields, found {}", f.len()),
            );
            continue;
        }
        let mut ok = true;
        let tiles = match dist.parse_magpie(f[0], false) {
            Ok(t) if t.len() > MAX_WORD_TILES => {
                errors.push(
                    Some(line),
                    format!("{:?} has {} tiles; at most {MAX_WORD_TILES}", f[0], t.len()),
                );
                ok = false;
                t
            }
            Ok(t) => t,
            Err(NotationError::Blank) => {
                errors.push(
                    Some(line),
                    format!("{:?}: a word cannot hold a blank", f[0]),
                );
                ok = false;
                Vec::new()
            }
            Err(e) => {
                errors.push(Some(line), format!("{:?}: {}", f[0], notation_message(&e)));
                ok = false;
                Vec::new()
            }
        };
        let playability = parse_decimal(f[1]);
        if playability.is_none() {
            errors.push(
                Some(line),
                format!("playability {:?} is not a plain decimal number", f[1]),
            );
            ok = false;
        }
        let def_chars = f[2].chars().count();
        if def_chars == 0 {
            errors.push(Some(line), "the definition is required");
            ok = false;
        } else if def_chars > MAX_DEFINITION_CHARS {
            errors.push(
                Some(line),
                format!("the definition is longer than {MAX_DEFINITION_CHARS} characters"),
            );
            ok = false;
        }
        if ok {
            if let Some(prev) = seen.get(&tiles) {
                errors.push(Some(line), format!("{:?} repeats line {prev}", f[0]));
                continue;
            }
            seen.insert(tiles.clone(), line);
            words.push(ParsedWord {
                word: dist.to_magpie(&tiles),
                tiles,
                playability: playability.unwrap_or(0.0),
                definition: f[2].to_owned(),
            });
        }
    }
    if errors.is_empty() && words.is_empty() {
        errors.push(None, "the file has no words");
    }
    if errors.is_empty() {
        Ok(words)
    } else {
        Err(errors)
    }
}

// ---------------------------------------------------------------------------
// Leave values
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ParsedLeave {
    /// Canonical order: tile order, blank first.
    pub tiles: Vec<Tile>,
    pub leave: String,
    pub value: f64,
}

pub fn parse_leaves(bytes: &[u8], dist: &Distribution) -> Result<Vec<ParsedLeave>, UploadErrors> {
    let mut errors = UploadErrors::default();
    let recs = records(bytes, ',', &mut errors);
    let mut seen: HashMap<Vec<Tile>, u32> = HashMap::new();
    let mut leaves = Vec::with_capacity(recs.len());
    for (line, f) in recs {
        if f.len() != 2 {
            errors.push(
                Some(line),
                format!("expected 2 comma-separated fields, found {}", f.len()),
            );
            continue;
        }
        let mut ok = true;
        let tiles = match dist.parse_magpie(f[0], true) {
            Ok(t) if t.len() > MAX_LEAVE_TILES => {
                errors.push(
                    Some(line),
                    format!(
                        "{:?} has {} tiles; at most {MAX_LEAVE_TILES}",
                        f[0],
                        t.len()
                    ),
                );
                ok = false;
                t
            }
            Ok(t) => Distribution::canonical_leave(&t),
            Err(e) => {
                errors.push(Some(line), format!("{:?}: {}", f[0], notation_message(&e)));
                ok = false;
                Vec::new()
            }
        };
        if ok {
            let counts = dist.counts(&tiles);
            for (t, &n) in counts.iter().enumerate() {
                let bag = dist.tiles[t].count;
                if u16::from(n) > bag {
                    let what = if t as Tile == BLANK {
                        "blanks".to_owned()
                    } else {
                        format!("{:?}", dist.tiles[t].letter)
                    };
                    errors.push(
                        Some(line),
                        format!("{:?} holds {n} {what}; the bag has {bag}", f[0]),
                    );
                    ok = false;
                }
            }
        }
        let value = parse_decimal(f[1]);
        match value {
            None => {
                errors.push(
                    Some(line),
                    format!("value {:?} is not a plain decimal number", f[1]),
                );
                ok = false;
            }
            Some(v) if v.abs() > MAX_ABS_LEAVE_VALUE => {
                errors.push(
                    Some(line),
                    format!("value {:?} is outside ±1,000,000", f[1]),
                );
                ok = false;
            }
            _ => {}
        }
        if ok {
            if let Some(prev) = seen.get(&tiles) {
                errors.push(
                    Some(line),
                    format!("{:?} is the same leave as line {prev}", f[0]),
                );
                continue;
            }
            seen.insert(tiles.clone(), line);
            leaves.push(ParsedLeave {
                leave: dist.to_magpie(&tiles),
                tiles,
                value: value.unwrap_or(0.0),
            });
        }
    }
    if errors.is_empty() && leaves.is_empty() {
        errors.push(None, "the file has no leaves");
    }
    if errors.is_empty() {
        Ok(leaves)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::tiles::tests::catalan_like;

    fn lines(e: &UploadErrors) -> Vec<Option<u32>> {
        e.errors.iter().map(|x| x.line).collect()
    }

    #[test]
    fn decimals_are_plain() {
        for ok in ["542388", "28.292000", "-0.378", "+3", "0"] {
            assert!(parse_decimal(ok).is_some(), "{ok}");
        }
        for bad in [
            "1e5", "1,000", "NaN", "Infinity", ".5", "5.", "", "+", "--1", "0x1",
        ] {
            assert!(parse_decimal(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn distribution_rules() {
        let good = "\u{FEFF}?,?,2,0,0\r\nA,a,9,1,1\n\n  \nL·L,l·l,1,10,0,Ｌ,ｌ\n";
        let p = parse_distribution(good.as_bytes()).unwrap();
        assert_eq!(p.tiles.len(), 3);
        assert_eq!(p.tiles[2].letter, "L·L");
        let bad = "A,a,1,1,1\nB,b,1\nC,c,x,1,0\nD,d,1,-1,0\nE,e,1,1,2\nF,A,1,1,0\nAAAAAAAAA,x,1,1,0\nG.,g,1,1,0\nH,h,1,1,0,Ｈ,\nH,i,1,1,0\n";
        let e = parse_distribution(bad.as_bytes()).unwrap_err();
        assert_eq!(
            lines(&e),
            vec![
                Some(1),
                Some(2),
                Some(3),
                Some(4),
                Some(5),
                Some(7),
                Some(8),
                Some(9),
                Some(10),
                Some(6)
            ]
        );
    }

    #[test]
    fn errors_cap_at_one_thousand_with_a_total() {
        let mut s = String::from("?,?,2,0,0\n");
        for _ in 0..1500 {
            s.push_str("bad\n");
        }
        let e = parse_distribution(s.as_bytes()).unwrap_err();
        assert_eq!(e.errors.len(), 1000);
        assert!(e.total_errors >= 1500);
        assert_eq!(e.errors[0].line, Some(2));
    }

    #[test]
    fn lexicon_rules() {
        let d = catalan_like();
        let good = "A[NY]S\t3000\tyears [n]\nCASA\t2.5\ta house\n";
        let w = parse_lexicon(good.as_bytes(), &d).unwrap();
        assert_eq!(w[0].tiles.len(), 3);
        let bad = "anys\t1\tx\nA?\t1\tx\nCASA\tNaN\tx\nCASA\t1\t\nA[NY]S\t1\tx\t\nNASA\t1\tx\nNASA\t2\ty\nAAAAAAAAAAAAAAAA\t1\tx\n";
        let e = parse_lexicon(bad.as_bytes(), &d).unwrap_err();
        assert_eq!(
            lines(&e),
            vec![
                Some(1),
                Some(2),
                Some(3),
                Some(4),
                Some(5),
                Some(7),
                Some(8)
            ]
        );
    }

    #[test]
    fn leave_rules() {
        let d = catalan_like();
        let good = "S[L·L]?A,12.0\n?,-1000000\n";
        let l = parse_leaves(good.as_bytes(), &d).unwrap();
        assert_eq!(l[0].leave, "?A[L·L]S");
        let bad = "AS,1\nSA,2\n???,1\n[L·L][L·L],1\nAAAAAAA,1\nA,1000000.5\nC,abc\nc,1\n";
        let e = parse_leaves(bad.as_bytes(), &d).unwrap_err();
        assert_eq!(
            lines(&e),
            vec![
                Some(2),
                Some(3),
                Some(4),
                Some(5),
                Some(6),
                Some(7),
                Some(8)
            ]
        );
    }
}
