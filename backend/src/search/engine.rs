//! Evaluation (PLAN.md § Search Engine → How a search runs, § Groups, § Filter
//! reference → Lax).

use std::cmp::Ordering;
use std::time::Instant;

use super::pattern::matches_in_order;
use super::{Condition, ConditionKind, Group, GroupOp, Node, QuizType, Range, SearchError, SearchSpec, Target};
use crate::catalog::index::{LeaveEntry, LexiconIndex, WordEntry};
use crate::catalog::tiles::{Tile, BLANK};

/// The deadline is checked every this many candidates, and before each limit ranking.
pub const CHECK_EVERY: usize = 4096;

struct Ctx<'a> {
    target: Target<'a>,
    deadline: Instant,
    ticks: usize,
}

impl Ctx<'_> {
    fn tick(&mut self) -> Result<(), SearchError> {
        self.ticks += 1;
        if self.ticks % CHECK_EVERY == 0 && Instant::now() > self.deadline {
            return Err(SearchError::TooBroad);
        }
        Ok(())
    }

    fn check(&self) -> Result<(), SearchError> {
        if Instant::now() > self.deadline {
            Err(SearchError::TooBroad)
        } else {
            Ok(())
        }
    }
}

pub fn run(
    target: Target,
    quiz_type: QuizType,
    spec: &SearchSpec,
    deadline: Instant,
    shortcuts: bool,
) -> Result<Vec<Box<[Tile]>>, SearchError> {
    let ids = run_ids(target, spec, deadline, shortcuts)?;
    Ok(questions(target, quiz_type, &ids))
}

/// The matching candidate ids (words or leaves), ascending.
pub fn run_ids(target: Target, spec: &SearchSpec, deadline: Instant, shortcuts: bool) -> Result<Vec<u32>, SearchError> {
    let mut ctx = Ctx { target, deadline, ticks: 0 };
    let candidates = if shortcuts { shortcut_candidates(target, &spec.root) } else { None }
        .unwrap_or_else(|| all_candidates(target));
    eval_group(&mut ctx, &spec.root, &candidates)
}

fn all_candidates(target: Target) -> Vec<u32> {
    match target {
        Target::Words(l) => (0..l.words.len() as u32).collect(),
        Target::Leaves(s) => (0..s.leaves.len() as u32).collect(),
    }
}

/// Shortcuts only (PLAN.md § Search Engine, step 2): a top AND group's Length
/// rows pick per-length buckets, and a non-negated literal Anagram Match starts
/// from the alphagram map (words) or the canonical-leave lookup (leaves).
fn shortcut_candidates(target: Target, root: &Group) -> Option<Vec<u32>> {
    if root.op != GroupOp::And {
        return None;
    }
    let mut length: Option<Range> = None;
    let mut literal: Option<&Vec<Tile>> = None;
    for child in &root.children {
        if let Node::Condition(c) = child {
            match &c.kind {
                ConditionKind::Length(r) if !c.negated => {
                    length = Some(match length {
                        None => *r,
                        Some(l) => Range { min: l.min.max(r.min), max: l.max.min(r.max) },
                    });
                }
                ConditionKind::AnagramMatch { literal: Some(tiles), .. } if !c.negated => literal = Some(tiles),
                _ => {}
            }
        }
    }
    let from_literal: Option<Vec<u32>> = literal.map(|tiles| match target {
        Target::Words(l) => l
            .alphagram_lookup
            .get(tiles.as_slice())
            .map(|&a| l.alphagram_words[a as usize].clone())
            .unwrap_or_default(),
        Target::Leaves(s) => s.lookup.get(tiles.as_slice()).map(|&i| vec![i]).unwrap_or_default(),
    });
    let from_length: Option<Vec<u32>> = length.map(|r| {
        let buckets = match target {
            Target::Words(l) => &l.by_length,
            Target::Leaves(s) => &s.by_size,
        };
        let mut v: Vec<u32> = (r.min..=r.max.min(buckets.len() as u32 - 1))
            .flat_map(|len| buckets[len as usize].iter().copied())
            .collect();
        v.sort_unstable();
        v
    });
    match (from_literal, from_length) {
        (Some(a), Some(b)) => Some(intersect(&a, &b)),
        (a, b) => a.or(b),
    }
}

fn intersect(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            Ordering::Less => i += 1,
            Ordering::Greater => j += 1,
            Ordering::Equal => {
                out.push(a[i]);
                i += 1;
                j += 1;
            }
        }
    }
    out
}

fn union(a: &[u32], b: &[u32]) -> Vec<u32> {
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::with_capacity(a.len() + b.len());
    while i < a.len() || j < b.len() {
        if j == b.len() || (i < a.len() && a[i] < b[j]) {
            out.push(a[i]);
            i += 1;
        } else if i == a.len() || b[j] < a[i] {
            out.push(b[j]);
            j += 1;
        } else {
            out.push(a[i]);
            i += 1;
            j += 1;
        }
    }
    out
}

/// Cheapest first: integer and leave value ranges, then tile counts, then
/// patterns and lookups, then definition substring scans.
fn cost(k: &ConditionKind) -> u8 {
    use ConditionKind::*;
    match k {
        Length(_) | NumVowels(_) | NumUniqueLetters(_) | PointValue(_) | NumAnagrams(_) | ProbabilityOrder { .. }
        | PlayabilityOrder { .. } | FrontInnerHook | BackInnerHook | LeaveValue { .. } | PartOfSpeech(_) => 0,
        IncludesLetters(_) | ConsistsOf { .. } | AnagramMatch { .. } | SubanagramMatch(_) => 1,
        PatternMatch(_) | InLexicon(_) | InWordList(_) | TakesPrefix(_) | TakesSuffix(_) => 2,
        Definition(_) => 3,
        LimitByProbabilityOrder { .. } | LimitByPlayabilityOrder { .. } => 4,
    }
}

fn eval_group(ctx: &mut Ctx, group: &Group, candidates: &[u32]) -> Result<Vec<u32>, SearchError> {
    let mut preds: Vec<&Condition> = Vec::new();
    let mut limits: Vec<&Condition> = Vec::new();
    let mut groups: Vec<&Group> = Vec::new();
    for child in &group.children {
        match child {
            Node::Condition(c) if c.kind.is_limit() => limits.push(c),
            Node::Condition(c) => preds.push(c),
            Node::Group(g) => groups.push(g),
        }
    }
    preds.sort_by_key(|c| cost(&c.kind));
    let result = match group.op {
        GroupOp::And => {
            let mut survivors = Vec::with_capacity(candidates.len());
            for &id in candidates {
                ctx.tick()?;
                if preds.iter().all(|p| test(ctx.target, p, id)) {
                    survivors.push(id);
                }
            }
            // Child groups get the candidates the parent's predicate rows
            // accept, never narrowed by sibling groups or the parent's limits.
            let mut result = survivors.clone();
            for g in groups {
                let r = eval_group(ctx, g, &survivors)?;
                result = intersect(&result, &r);
            }
            result
        }
        GroupOp::Or => {
            if preds.is_empty() && groups.is_empty() {
                // A group of limit rows only ranks every one of its candidates.
                candidates.to_vec()
            } else {
                let mut result = Vec::new();
                for p in preds {
                    let mut matched = Vec::new();
                    for &id in candidates {
                        ctx.tick()?;
                        if test(ctx.target, p, id) {
                            matched.push(id);
                        }
                    }
                    result = union(&result, &matched);
                }
                for g in groups {
                    let r = eval_group(ctx, g, candidates)?;
                    result = union(&result, &r);
                }
                result
            }
        }
    };
    apply_limits(ctx, &limits, result)
}

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// The value a limit ranks by: combinations, or playability.
#[derive(Clone, Copy, PartialEq)]
enum Value {
    Combos(u128),
    Play(f64),
}

impl Value {
    fn cmp_desc(self, other: Value) -> Ordering {
        match (self, other) {
            (Value::Combos(a), Value::Combos(b)) => b.cmp(&a),
            (Value::Play(a), Value::Play(b)) => b.total_cmp(&a),
            _ => Ordering::Equal,
        }
    }
}

fn value_of(target: Target, id: u32, probability: bool) -> Value {
    match target {
        Target::Words(l) => {
            let w = &l.words[id as usize];
            if probability {
                Value::Combos(w.combinations)
            } else {
                Value::Play(w.playability)
            }
        }
        Target::Leaves(s) => Value::Combos(s.leaves[id as usize].combinations),
    }
}

/// Ties are broken by alphagram, then by the word; for a leave both are the leave.
fn tie(target: Target, a: u32, b: u32) -> Ordering {
    match target {
        Target::Words(l) => {
            let (wa, wb) = (&l.words[a as usize], &l.words[b as usize]);
            l.alphagrams[wa.alphagram as usize]
                .cmp(&l.alphagrams[wb.alphagram as usize])
                .then_with(|| wa.tiles.cmp(&wb.tiles))
        }
        Target::Leaves(s) => s.leaves[a as usize].tiles.cmp(&s.leaves[b as usize].tiles),
    }
}

/// The intersection of one kind's rows: (highest min, lowest max), 1…∞ if none.
fn reduce(rows: &[(Range, bool)], lax: bool) -> Option<(u64, u64)> {
    let rs: Vec<&Range> = rows.iter().filter(|(_, l)| *l == lax).map(|(r, _)| r).collect();
    if rs.is_empty() {
        return None;
    }
    Some((
        rs.iter().map(|r| u64::from(r.min)).max().unwrap(),
        rs.iter().map(|r| u64::from(r.max)).min().unwrap(),
    ))
}

fn apply_limits(ctx: &mut Ctx, limits: &[&Condition], result: Vec<u32>) -> Result<Vec<u32>, SearchError> {
    if limits.is_empty() {
        return Ok(result);
    }
    let mut kept: Option<Vec<u32>> = None;
    for probability in [true, false] {
        let rows: Vec<(Range, bool)> = limits
            .iter()
            .filter_map(|c| match &c.kind {
                ConditionKind::LimitByProbabilityOrder { range, lax } if probability => Some((*range, *lax)),
                ConditionKind::LimitByPlayabilityOrder { range, lax } if !probability => Some((*range, *lax)),
                _ => None,
            })
            .collect();
        if rows.is_empty() {
            continue;
        }
        ctx.check()?;
        // Each kind ranks the same result, never the other kind's output.
        let target = ctx.target;
        let mut ranked = result.clone();
        ranked.sort_by(|&a, &b| {
            value_of(target, a, probability).cmp_desc(value_of(target, b, probability)).then_with(|| tie(target, a, b))
        });
        let strict = reduce(&rows, false).unwrap_or((1, u64::MAX));
        let lax = reduce(&rows, true);
        let any_lax = lax.is_some();
        let lax = lax.unwrap_or((1, u64::MAX));
        let start = strict.0.max(lax.0);
        let end = strict.1.min(lax.1);
        let n = ranked.len() as u64;
        let mut slice: Vec<u32> = Vec::new();
        if start <= end && start <= n && start >= 1 {
            let mut s = (start - 1) as usize;
            let mut e = (end.min(n) - 1) as usize;
            if any_lax {
                // Widen to neighbours of equal value, never past the strict bounds.
                let v = |i: usize| value_of(target, ranked[i], probability);
                while s > 0 && (s as u64) >= strict.0 && v(s - 1) == v(s) {
                    s -= 1;
                }
                while e + 1 < ranked.len() && ((e + 2) as u64) <= strict.1 && v(e + 1) == v(e) {
                    e += 1;
                }
            }
            slice = ranked[s..=e].to_vec();
        }
        slice.sort_unstable();
        kept = Some(match kept {
            None => slice,
            Some(k) => intersect(&k, &slice),
        });
    }
    Ok(kept.unwrap_or(result))
}

// ---------------------------------------------------------------------------
// Predicates
// ---------------------------------------------------------------------------

fn test(target: Target, c: &Condition, id: u32) -> bool {
    let hit = match target {
        Target::Words(l) => word_test(l, &c.kind, &l.words[id as usize]),
        Target::Leaves(s) => leave_test(&c.kind, &s.leaves[id as usize]),
    };
    hit != c.negated
}

fn order(range: &Range, lax: bool, rank: u32, lo: u32, hi: u32) -> bool {
    if lax {
        hi >= range.min && lo <= range.max
    } else {
        range.contains(rank)
    }
}

fn includes(need: &[u8], counts: &[u8]) -> bool {
    need.iter().zip(counts).all(|(&n, &c)| c >= n)
}

fn consists(tiles: &super::pattern::TileSet, word: &[Tile], min: u8, max: u8) -> bool {
    let inside = word.iter().filter(|&&t| tiles.contains(t)).count();
    let pct = (100 * inside / word.len().max(1)) as u32;
    u32::from(min) <= pct && pct <= u32::from(max)
}

fn word_test(l: &LexiconIndex, k: &ConditionKind, w: &WordEntry) -> bool {
    use ConditionKind::*;
    let len = w.tiles.len();
    match k {
        AnagramMatch { bag, .. } => bag.anagram(&w.counts, len),
        PatternMatch(p) => matches_in_order(p, &w.tiles),
        SubanagramMatch(bag) => bag.subanagram(&w.counts, len),
        Length(r) => r.contains(u32::from(w.length)),
        InLexicon(other) => other.word_lookup.contains_key(&w.tiles),
        InWordList(set) => set.contains(&w.tiles),
        NumVowels(r) => r.contains(u32::from(w.num_vowels)),
        IncludesLetters(need) => includes(need, &w.counts),
        ProbabilityOrder { range, lax } => {
            order(range, *lax, w.probability_order, w.min_probability_order, w.max_probability_order)
        }
        PlayabilityOrder { range, lax } => {
            order(range, *lax, w.playability_order, w.min_playability_order, w.max_playability_order)
        }
        NumUniqueLetters(r) => r.contains(u32::from(w.num_unique_letters)),
        PointValue(r) => r.contains(u32::from(w.point_value)),
        TakesPrefix(prefix) => {
            let mut t = prefix.clone();
            t.extend_from_slice(&w.tiles);
            l.word_lookup.contains_key(t.as_slice())
        }
        TakesSuffix(suffix) => {
            let mut t = w.tiles.to_vec();
            t.extend_from_slice(suffix);
            l.word_lookup.contains_key(t.as_slice())
        }
        PartOfSpeech(p) => w.pos & p.bit() != 0,
        Definition(needle) => w.definition.to_lowercase().contains(needle.as_str()),
        ConsistsOf { tiles, min_pct, max_pct } => consists(tiles, &w.tiles, *min_pct, *max_pct),
        NumAnagrams(r) => r.contains(w.num_anagrams),
        FrontInnerHook => w.has_front_inner_hook,
        BackInnerHook => w.has_back_inner_hook,
        // Validation keeps these off word targets.
        LeaveValue { .. } | LimitByProbabilityOrder { .. } | LimitByPlayabilityOrder { .. } => true,
    }
}

fn leave_test(k: &ConditionKind, l: &LeaveEntry) -> bool {
    use ConditionKind::*;
    let len = l.tiles.len();
    match k {
        AnagramMatch { bag, .. } => bag.anagram(&l.counts, len),
        // Pattern Match matches against the alphabetized leave.
        PatternMatch(p) => matches_in_order(p, &l.tiles),
        SubanagramMatch(bag) => bag.subanagram(&l.counts, len),
        Length(r) => r.contains(u32::from(l.length)),
        InWordList(set) => set.contains(&l.tiles),
        NumVowels(r) => r.contains(u32::from(l.num_vowels)),
        IncludesLetters(need) => includes(need, &l.counts),
        ProbabilityOrder { range, lax } => {
            order(range, *lax, l.probability_order, l.min_probability_order, l.max_probability_order)
        }
        NumUniqueLetters(r) => r.contains(u32::from(l.num_unique_letters)),
        PointValue(r) => r.contains(u32::from(l.point_value)),
        ConsistsOf { tiles, min_pct, max_pct } => consists(tiles, &l.tiles, *min_pct, *max_pct),
        NumAnagrams(r) => r.contains(l.num_anagrams),
        LeaveValue { min, max } => min.is_none_or(|m| l.value >= m) && max.is_none_or(|m| l.value <= m),
        // Validation keeps word-only filters off leave targets.
        InLexicon(_) | PlayabilityOrder { .. } | TakesPrefix(_) | TakesSuffix(_) | PartOfSpeech(_)
        | Definition(_) | FrontInnerHook | BackInnerHook | LimitByProbabilityOrder { .. }
        | LimitByPlayabilityOrder { .. } => true,
    }
}

// ---------------------------------------------------------------------------
// Questions
// ---------------------------------------------------------------------------

/// Anagram: distinct alphagrams. Definition: words. Leave Value: canonical
/// leaves. Ids are already in alphabetical order of their keys (the index
/// stores words, alphagrams and leaves sorted), so the result is too.
pub fn questions(target: Target, quiz_type: QuizType, ids: &[u32]) -> Vec<Box<[Tile]>> {
    match target {
        Target::Words(l) => match quiz_type {
            QuizType::Anagram => {
                let mut alphas: Vec<u32> = ids.iter().map(|&i| l.words[i as usize].alphagram).collect();
                alphas.sort_unstable();
                alphas.dedup();
                alphas.into_iter().map(|a| l.alphagrams[a as usize].clone()).collect()
            }
            _ => ids.iter().map(|&i| l.words[i as usize].tiles.clone()).collect(),
        },
        Target::Leaves(s) => ids.iter().map(|&i| s.leaves[i as usize].tiles.clone()).collect(),
    }
}

/// Whether a leave holds the blank.
pub fn has_blank(tiles: &[Tile]) -> bool {
    tiles.contains(&BLANK)
}
