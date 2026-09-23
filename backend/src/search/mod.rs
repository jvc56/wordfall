//! The search engine (PLAN.md § Search Engine): a pure module with no I/O.
//!
//! `wire` is the filter tree as sent; `validate` checks it against a quiz type
//! and target and parses it into the engine's `SearchSpec`; `engine` runs it.

pub mod engine;
pub mod pattern;
pub mod routes;
#[cfg(test)]
mod tests;
pub mod validate;
pub mod wire;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use crate::catalog::index::{LeaveSetIndex, LexiconIndex};
use crate::catalog::pos::PartOfSpeech;
use crate::catalog::tiles::Tile;
pub use wire::{ConditionType, GroupOp, QuizType};

use pattern::{Bag, Pattern, TileSet};

#[derive(Debug, Clone)]
pub struct SearchSpec {
    pub root: Group,
}

#[derive(Debug, Clone)]
pub struct Group {
    pub op: GroupOp,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub enum Node {
    Condition(Condition),
    Group(Group),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub min: u32,
    pub max: u32,
}

impl Range {
    pub fn contains(&self, v: u32) -> bool {
        self.min <= v && v <= self.max
    }
}

#[derive(Debug, Clone)]
pub enum ConditionKind {
    /// `literal` is the sorted tiles when the pattern holds only literal tiles.
    AnagramMatch { bag: Bag, literal: Option<Vec<Tile>> },
    PatternMatch(Pattern),
    SubanagramMatch(Bag),
    Length(Range),
    InLexicon(Arc<LexiconIndex>),
    InWordList(HashSet<Box<[Tile]>>),
    NumVowels(Range),
    /// Per-tile counts the word must hold at least.
    IncludesLetters(Vec<u8>),
    ProbabilityOrder { range: Range, lax: bool },
    LimitByProbabilityOrder { range: Range, lax: bool },
    PlayabilityOrder { range: Range, lax: bool },
    LimitByPlayabilityOrder { range: Range, lax: bool },
    NumUniqueLetters(Range),
    PointValue(Range),
    TakesPrefix(Vec<Tile>),
    TakesSuffix(Vec<Tile>),
    PartOfSpeech(PartOfSpeech),
    /// Lower-cased, matched as a case-insensitive substring.
    Definition(String),
    ConsistsOf { tiles: TileSet, min_pct: u8, max_pct: u8 },
    NumAnagrams(Range),
    FrontInnerHook,
    BackInnerHook,
    LeaveValue { min: Option<f64>, max: Option<f64> },
}

impl ConditionKind {
    pub fn is_limit(&self) -> bool {
        matches!(self, ConditionKind::LimitByProbabilityOrder { .. } | ConditionKind::LimitByPlayabilityOrder { .. })
    }
}

#[derive(Debug, Clone)]
pub struct Condition {
    pub kind: ConditionKind,
    pub negated: bool,
}

#[derive(Clone, Copy)]
pub enum Target<'a> {
    Words(&'a LexiconIndex),
    Leaves(&'a LeaveSetIndex),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    /// The cooperative deadline passed: "search too broad".
    TooBroad,
}

/// The search's questions: alphagrams, words or canonical leaves, in
/// alphabetical order of the key.
pub fn search(target: Target, quiz_type: QuizType, spec: &SearchSpec, deadline: Instant) -> Result<Vec<Box<[Tile]>>, SearchError> {
    engine::run(target, quiz_type, spec, deadline, true)
}
