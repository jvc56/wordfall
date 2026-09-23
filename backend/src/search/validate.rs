//! Validation (PLAN.md § Search Engine step 1, § Groups → Validation, § Filter
//! reference → Range defaults). Every error is returned at once, keyed by the
//! row's path.
//!
//! With a target (preview and creation) the tree is checked in full and parsed
//! into a `SearchSpec`. Without one (saving a search) everything that does not
//! need a lexicon is checked: group rules, parameters, negation, applicability,
//! ranges against the type's floor and type-fixed ceiling, canonical form and
//! length, and the In Word List total.

use std::collections::HashSet;
use std::sync::Arc;

use super::pattern::{self, Bag, PatternError, TileSet};
use super::wire::{ConditionType, Params, PathError, WireCondition, WireGroup, WireNode};
use super::{Condition, ConditionKind, Group, Node, QuizType, Range, SearchSpec};
use crate::catalog::index::{LeaveSetIndex, LexiconIndex};
use crate::catalog::tiles::{Distribution, NotationError, Tile, BLANK};
use crate::catalog::Snapshot;

pub const MAX_ROWS: usize = 100;
/// Matches the `search_groups.id` CHECK (0–99), counting the top group.
pub const MAX_GROUPS: usize = 100;
pub const MAX_DEPTH: usize = 4;
/// The limit for a whole filter tree, not for one row.
pub const MAX_WORD_LIST_ENTRIES: usize = 300_000;
/// `search_conditions.text_value` holds at most 500 characters.
pub const MAX_TEXT_CHARS: usize = 500;
/// `search_condition_words.entry` holds at most 300 characters.
pub const MAX_ENTRY_CHARS: usize = 300;
pub const USE_DOT: &str = "use `.` for any single tile";

pub struct TargetInfo<'a> {
    pub lexicon: &'a Arc<LexiconIndex>,
    /// The lexicon's leave values, required for a Leave Value quiz.
    pub leaves: Option<&'a Arc<LeaveSetIndex>>,
    pub snapshot: &'a Snapshot,
}

impl TargetInfo<'_> {
    fn dist(&self) -> &Distribution {
        &self.lexicon.distribution
    }
}

struct Validator<'a> {
    q: QuizType,
    target: Option<&'a TargetInfo<'a>>,
    errors: Vec<PathError>,
    rows: usize,
    groups: usize,
    entries: usize,
}

pub fn validate(tree: &WireGroup, q: QuizType, target: Option<&TargetInfo>) -> Result<Option<SearchSpec>, Vec<PathError>> {
    let mut v = Validator { q, target, errors: Vec::new(), rows: 0, groups: 0, entries: 0 };
    let root = v.group(tree, &mut Vec::new(), 1);
    if !v.errors.is_empty() {
        return Err(v.errors);
    }
    Ok(if target.is_some() { root.map(|root| SearchSpec { root }) } else { None })
}

/// A filter's display name, for messages.
pub fn label(t: ConditionType) -> &'static str {
    use ConditionType::*;
    match t {
        AnagramMatch => "Anagram Match",
        PatternMatch => "Pattern Match",
        SubanagramMatch => "Subanagram Match",
        Length => "Length",
        InLexicon => "In Lexicon",
        InWordList => "In Word List",
        NumVowels => "Number of Vowels",
        IncludesLetters => "Includes Letters",
        ProbabilityOrder => "Probability Order",
        LimitByProbabilityOrder => "Limit by Probability Order",
        PlayabilityOrder => "Playability Order",
        LimitByPlayabilityOrder => "Limit by Playability Order",
        NumUniqueLetters => "Number of Unique Letters",
        PointValue => "Point Value",
        TakesPrefix => "Takes Prefix",
        TakesSuffix => "Takes Suffix",
        PartOfSpeech => "Part of Speech",
        Definition => "Definition",
        ConsistsOf => "Consists of",
        NumAnagrams => "Number of Anagrams",
        FrontInnerHook => "Front Inner Hook",
        BackInnerHook => "Back Inner Hook",
        LeaveValue => "Leave Value",
    }
}

fn quiz_label(q: QuizType) -> &'static str {
    match q {
        QuizType::Anagram => "Anagram",
        QuizType::Definition => "Definition",
        QuizType::LeaveValue => "Leave Value",
    }
}

/// MAGPIE notation syntax alone, for a save, which has no distribution.
fn magpie_syntax(s: &str, allow_blank: bool) -> Result<(), String> {
    if s.is_empty() {
        return Err("enter at least one tile".into());
    }
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '[' => {
                let mut n = 0;
                let mut closed = false;
                for d in chars.by_ref() {
                    match d {
                        ']' => {
                            closed = true;
                            break;
                        }
                        '[' => return Err(NotationError::NestedBracket.to_string()),
                        d if d.is_lowercase() => return Err("tiles are written in upper case".into()),
                        _ => n += 1,
                    }
                }
                if !closed {
                    return Err(NotationError::UnclosedBracket.to_string());
                }
                if n == 0 {
                    return Err(NotationError::EmptyBracket.to_string());
                }
                if n == 1 {
                    return Err("single-character tiles are written without brackets".into());
                }
            }
            ']' => return Err(NotationError::UnmatchedBracket.to_string()),
            '?' if !allow_blank => return Err(USE_DOT.into()),
            c if c.is_whitespace() => return Err("tiles are written without spaces".into()),
            c if c.is_lowercase() => return Err("tiles are written in upper case".into()),
            _ => {}
        }
    }
    Ok(())
}

fn notation_message(e: &NotationError) -> String {
    match e {
        NotationError::Blank => USE_DOT.into(),
        other => other.to_string(),
    }
}

impl Validator<'_> {
    fn err(&mut self, path: &[usize], field: &str, message: impl Into<String>) {
        self.errors.push(PathError::new(path, field, message));
    }

    fn group(&mut self, g: &WireGroup, path: &mut Vec<usize>, depth: usize) -> Option<Group> {
        self.groups += 1;
        if self.groups == MAX_GROUPS + 1 {
            self.err(path, "op", format!("A search can hold at most {MAX_GROUPS} groups."));
        }
        if depth > MAX_DEPTH {
            self.err(path, "op", format!("Groups nest at most {MAX_DEPTH} deep."));
        }
        if g.children.is_empty() {
            self.err(path, "children", "A group must hold at least one row or group.");
        }
        let mut children = Vec::with_capacity(g.children.len());
        for (i, c) in g.children.iter().enumerate() {
            path.push(i);
            let node = match c {
                WireNode::Group(sub) => self.group(sub, path, depth + 1).map(Node::Group),
                WireNode::Condition(cond) => self.condition(cond, path).map(Node::Condition),
            };
            path.pop();
            if let Some(n) = node {
                children.push(n);
            }
        }
        Some(Group { op: g.op, children })
    }

    fn condition(&mut self, c: &WireCondition, path: &[usize]) -> Option<Condition> {
        self.rows += 1;
        let start = self.errors.len();
        if self.rows == MAX_ROWS + 1 {
            self.err(path, "type", format!("A search can hold at most {MAX_ROWS} rows."));
        }
        let t = c.ctype;
        if !t.applies_to(self.q) {
            self.err(path, "type", format!("{} does not apply to {} quizzes.", label(t), quiz_label(self.q)));
            return None;
        }
        if c.negated && !t.negatable() {
            self.err(path, "negated", format!("{} cannot be negated.", label(t)));
        }
        let kind = self.params(t, &c.params, path);
        if self.errors.len() > start {
            return None;
        }
        kind.map(|kind| Condition { kind, negated: c.negated })
    }

    /// The ceiling of an integer range, or `None` when it needs a target and
    /// there is none.
    fn ceiling(&self, t: ConditionType) -> Option<u64> {
        use ConditionType::*;
        let leave = self.q.is_leave();
        let tiles = if leave { 6 } else { 15 };
        match t {
            Length | NumVowels | NumUniqueLetters => Some(tiles),
            ConsistsOf => Some(100),
            PointValue => self.target.map(|x| tiles * u64::from(x.dist().max_value())),
            NumAnagrams => self.target.and_then(|x| {
                if leave {
                    x.leaves.map(|s| u64::from(s.max_num_anagrams))
                } else {
                    Some(u64::from(x.lexicon.max_num_anagrams))
                }
            }),
            ProbabilityOrder | PlayabilityOrder => self.target.and_then(|x| {
                if leave {
                    x.leaves.map(|s| u64::from(s.max_order_rank))
                } else {
                    Some(u64::from(x.lexicon.max_order_rank))
                }
            }),
            LimitByProbabilityOrder | LimitByPlayabilityOrder => self.target.and_then(|x| {
                if leave {
                    x.leaves.map(|s| s.leave_count() as u64)
                } else {
                    Some(x.lexicon.word_count() as u64)
                }
            }),
            _ => None,
        }
    }

    fn floor(t: ConditionType) -> i64 {
        use ConditionType::*;
        match t {
            Length | ProbabilityOrder | LimitByProbabilityOrder | PlayabilityOrder | LimitByPlayabilityOrder => 1,
            _ => 0,
        }
    }

    /// A valid range narrows something: min above the filter's own floor or
    /// max below its ceiling. A bound above the ceiling names the ceiling.
    fn range(&mut self, t: ConditionType, min: i64, max: i64, path: &[usize]) -> Option<Range> {
        let floor = Self::floor(t);
        let ceiling = self.ceiling(t);
        let before = self.errors.len();
        if min < floor {
            self.err(path, "min", format!("The minimum must be at least {floor}."));
        }
        if max < floor {
            self.err(path, "max", format!("The maximum must be at least {floor}."));
        }
        if let Some(c) = ceiling {
            if min >= 0 && min as u64 > c {
                self.err(path, "min", format!("The minimum can be at most {c}."));
            }
            if max >= 0 && max as u64 > c {
                self.err(path, "max", format!("The maximum can be at most {c}."));
            }
        }
        if min > max {
            self.err(path, "min", "The minimum is above the maximum.");
        }
        if self.errors.len() > before {
            return None;
        }
        if let Some(c) = ceiling {
            if min == floor && max as u64 == c {
                self.err(path, "min", format!("{floor}–{c} includes everything, so this row narrows nothing."));
                return None;
            }
        }
        if min > i64::from(u32::MAX) || max > i64::from(u32::MAX) {
            self.err(path, "max", "The bound is too large.");
            return None;
        }
        Some(Range { min: min as u32, max: max as u32 })
    }

    fn text_len(&mut self, s: &str, field: &str, path: &[usize]) -> bool {
        if s.chars().count() > MAX_TEXT_CHARS {
            self.err(path, field, format!("This is longer than the {MAX_TEXT_CHARS}-character limit."));
            return false;
        }
        true
    }

    /// Tiles in MAGPIE notation; `?` only in a Leave Value quiz.
    fn tiles(&mut self, s: &str, field: &str, path: &[usize]) -> Option<Vec<Tile>> {
        if !self.text_len(s, field, path) {
            return None;
        }
        let leave = self.q.is_leave();
        match self.target {
            None => match magpie_syntax(s, leave) {
                Ok(()) => Some(Vec::new()),
                Err(m) => {
                    self.err(path, field, m);
                    None
                }
            },
            Some(x) => match x.dist().parse_magpie(s, leave) {
                Ok(t) => Some(t),
                Err(e) => {
                    self.err(path, field, notation_message(&e));
                    None
                }
            },
        }
    }

    fn pattern(&mut self, t: ConditionType, s: &str, path: &[usize]) -> Option<ConditionKind> {
        if !self.text_len(s, "pattern", path) {
            return None;
        }
        let leave = self.q.is_leave();
        let raw = match pattern::tokenize(s) {
            Ok(r) => r,
            Err(e) => {
                self.err(path, "pattern", e.to_string());
                return None;
            }
        };
        if !leave && pattern::has_blank(&raw) {
            self.err(path, "pattern", USE_DOT);
            return None;
        }
        let x = self.target?;
        let p = match pattern::parse(s, x.dist(), leave) {
            Ok(p) => p,
            Err(e @ PatternError::UnknownTile(_)) | Err(e) => {
                self.err(path, "pattern", e.to_string());
                return None;
            }
        };
        let n = x.dist().len();
        Some(match t {
            ConditionType::AnagramMatch => {
                let bag = Bag::new(&p, n);
                let literal = bag.only_literals().then(|| {
                    let mut v: Vec<Tile> = p
                        .toks
                        .iter()
                        .map(|t| match t {
                            pattern::Tok::Tile(t) => *t,
                            _ => unreachable!(),
                        })
                        .collect();
                    v.sort_unstable();
                    v
                });
                ConditionKind::AnagramMatch { bag, literal }
            }
            ConditionType::SubanagramMatch => ConditionKind::SubanagramMatch(Bag::new(&p, n)),
            _ => ConditionKind::PatternMatch(p),
        })
    }

    fn entries(&mut self, entries: &[String], path: &[usize]) -> Option<ConditionKind> {
        let before = self.entries;
        self.entries += entries.len();
        if before <= MAX_WORD_LIST_ENTRIES && self.entries > MAX_WORD_LIST_ENTRIES {
            self.err(
                path,
                "entries",
                format!(
                    "In Word List rows can hold {MAX_WORD_LIST_ENTRIES} entries in all; this row takes the total to {}.",
                    self.entries
                ),
            );
            return None;
        }
        let leave = self.q.is_leave();
        let mut set: HashSet<Box<[Tile]>> = HashSet::with_capacity(entries.len());
        for e in entries {
            if e.chars().count() > MAX_ENTRY_CHARS {
                self.err(path, "entries", format!("An entry is longer than {MAX_ENTRY_CHARS} characters."));
                return None;
            }
            if !leave && e.contains('?') {
                self.err(path, "entries", USE_DOT);
                return None;
            }
            match self.target {
                None => {
                    if let Err(m) = magpie_syntax(e, leave) {
                        self.err(path, "entries", format!("{e:?}: {m}"));
                        return None;
                    }
                }
                Some(x) => {
                    // Entries that are not valid in the lexicon are ignored.
                    if let Ok(tiles) = x.dist().parse_magpie(e, leave) {
                        if leave && tiles.windows(2).any(|w| w[0] > w[1]) {
                            self.err(path, "entries", format!("{e:?} is not in canonical leave order."));
                            return None;
                        }
                        set.insert(tiles.into_boxed_slice());
                    }
                }
            }
        }
        Some(ConditionKind::InWordList(set))
    }

    fn params(&mut self, t: ConditionType, p: &Params, path: &[usize]) -> Option<ConditionKind> {
        use ConditionType as T;
        let leave = self.q.is_leave();
        match (t, p) {
            (T::AnagramMatch | T::PatternMatch | T::SubanagramMatch, Params::Pattern(s)) => self.pattern(t, s, path),
            (T::Length, Params::Range { min, max }) => self.range(t, *min, *max, path).map(ConditionKind::Length),
            (T::NumVowels, Params::Range { min, max }) => self.range(t, *min, *max, path).map(ConditionKind::NumVowels),
            (T::NumUniqueLetters, Params::Range { min, max }) => {
                self.range(t, *min, *max, path).map(ConditionKind::NumUniqueLetters)
            }
            (T::PointValue, Params::Range { min, max }) => self.range(t, *min, *max, path).map(ConditionKind::PointValue),
            (T::NumAnagrams, Params::Range { min, max }) => {
                if leave && self.target.is_some_and(|x| x.leaves.is_none()) {
                    self.err(path, "type", "This lexicon has no leave values.");
                    return None;
                }
                self.range(t, *min, *max, path).map(ConditionKind::NumAnagrams)
            }
            (T::ProbabilityOrder, Params::Order { min, max, lax }) => {
                self.range(t, *min, *max, path).map(|range| ConditionKind::ProbabilityOrder { range, lax: *lax })
            }
            (T::LimitByProbabilityOrder, Params::Order { min, max, lax }) => self
                .range(t, *min, *max, path)
                .map(|range| ConditionKind::LimitByProbabilityOrder { range, lax: *lax }),
            (T::PlayabilityOrder, Params::Order { min, max, lax }) => {
                self.range(t, *min, *max, path).map(|range| ConditionKind::PlayabilityOrder { range, lax: *lax })
            }
            (T::LimitByPlayabilityOrder, Params::Order { min, max, lax }) => self
                .range(t, *min, *max, path)
                .map(|range| ConditionKind::LimitByPlayabilityOrder { range, lax: *lax }),
            (T::ConsistsOf, Params::ConsistsOf { tiles, min, max }) => {
                let set = self.tiles(tiles, "tiles", path);
                let range = self.range(t, *min, *max, path);
                let (set, range) = (set?, range?);
                Some(ConditionKind::ConsistsOf {
                    tiles: TileSet::from_tiles(&set),
                    min_pct: range.min as u8,
                    max_pct: range.max as u8,
                })
            }
            (T::IncludesLetters, Params::Tiles(s)) => {
                let tiles = self.tiles(s, "tiles", path)?;
                let n = self.target.map(|x| x.dist().len()).unwrap_or(0);
                let mut need = vec![0u8; n];
                for t in tiles {
                    need[t as usize] = need[t as usize].saturating_add(1);
                }
                Some(ConditionKind::IncludesLetters(need))
            }
            (T::TakesPrefix, Params::Tiles(s)) => self.tiles(s, "tiles", path).map(ConditionKind::TakesPrefix),
            (T::TakesSuffix, Params::Tiles(s)) => self.tiles(s, "tiles", path).map(ConditionKind::TakesSuffix),
            (T::InLexicon, Params::Lexicon(name)) => {
                if name.is_empty() {
                    self.err(path, "lexicon", "Choose a lexicon.");
                    return None;
                }
                if !self.text_len(name, "lexicon", path) {
                    return None;
                }
                let x = self.target?;
                match x.snapshot.lexicon_by_name(name) {
                    None => {
                        self.err(path, "lexicon", format!("There is no lexicon named {name}."));
                        None
                    }
                    Some(other) if other.distribution.id != x.dist().id => {
                        self.err(
                            path,
                            "lexicon",
                            format!("{name} uses another letter distribution than {}.", x.lexicon.name),
                        );
                        None
                    }
                    Some(other) => Some(ConditionKind::InLexicon(other.clone())),
                }
            }
            (T::InWordList, Params::Entries(e)) => self.entries(e, path),
            (T::PartOfSpeech, Params::PartOfSpeech(pos)) => Some(ConditionKind::PartOfSpeech(*pos)),
            (T::Definition, Params::Text(s)) => {
                if s.is_empty() {
                    self.err(path, "text", "Enter some text.");
                    return None;
                }
                if !self.text_len(s, "text", path) {
                    return None;
                }
                Some(ConditionKind::Definition(s.to_lowercase()))
            }
            (T::FrontInnerHook, Params::None) => Some(ConditionKind::FrontInnerHook),
            (T::BackInnerHook, Params::None) => Some(ConditionKind::BackInnerHook),
            (T::LeaveValue, Params::LeaveValue { min, max }) => {
                if min.is_none() && max.is_none() {
                    self.err(path, "min", "Fill in at least one bound.");
                    return None;
                }
                if let (Some(a), Some(b)) = (min, max) {
                    if a > b {
                        self.err(path, "min", "The minimum is above the maximum.");
                        return None;
                    }
                }
                Some(ConditionKind::LeaveValue { min: *min, max: *max })
            }
            _ => {
                self.err(path, "type", "The parameters do not match the condition type.");
                None
            }
        }
    }
}

/// Checks that each blank-holding leave in a tile list is only for leaves.
pub fn holds_blank(tiles: &[Tile]) -> bool {
    tiles.contains(&BLANK)
}
