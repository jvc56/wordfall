//! PLAN.md § Unit tests → Backend → Search engine, against the fixture catalog.
//! Expected word lists were computed independently of this module.

use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::engine::{run, run_ids};
use super::validate::{validate, TargetInfo};
use super::wire::{parse_tree, PathError, QuizType};
use super::{SearchError, SearchSpec, Target};
use crate::catalog::fixtures::{load, Fixtures};
use crate::catalog::Snapshot;

fn snapshot(f: &Fixtures) -> Snapshot {
    let mut s = Snapshot::default();
    for d in [&f.english, &f.catalan] {
        s.distributions.insert(d.id, d.clone());
    }
    for l in [&f.en, &f.en_old, &f.ca] {
        s.lexicons.insert(l.id, l.clone());
    }
    for x in [&f.en_leaves, &f.ca_leaves] {
        s.leave_sets.insert(x.id, x.clone());
    }
    s
}

fn later() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

fn check(f: &Fixtures, q: QuizType, filters: Value) -> Result<SearchSpec, Vec<PathError>> {
    let snap = snapshot(f);
    let tree = parse_tree(&filters)?;
    let leaves = if q.is_leave() { Some(&f.en_leaves) } else { None };
    let info = TargetInfo { lexicon: &f.en, leaves, snapshot: &snap };
    validate(&tree, q, Some(&info)).map(|s| s.unwrap())
}

fn and(children: Vec<Value>) -> Value {
    json!({ "op": "and", "children": children })
}

fn or(children: Vec<Value>) -> Value {
    json!({ "op": "or", "children": children })
}

fn c(t: &str, extra: Value) -> Value {
    let mut v = json!({ "type": t, "negated": false });
    for (k, x) in extra.as_object().unwrap() {
        v[k] = x.clone();
    }
    v
}

fn not(mut v: Value) -> Value {
    v["negated"] = json!(true);
    v
}

/// The matching words of EN-FIX, alphabetical.
fn words_with(f: &Fixtures, filters: Value, shortcuts: bool) -> Vec<String> {
    let spec = check(f, QuizType::Definition, filters).unwrap();
    let ids = run_ids(Target::Words(&f.en), &spec, later(), shortcuts).unwrap();
    ids.iter().map(|&i| f.english.to_magpie(&f.en.words[i as usize].tiles)).collect()
}

fn words(f: &Fixtures, filters: Value) -> Vec<String> {
    let a = words_with(f, filters.clone(), true);
    let b = words_with(f, filters, false);
    assert_eq!(a, b, "the shortcut and the full scan agree");
    a
}

fn leaves(f: &Fixtures, filters: Value) -> Vec<String> {
    let spec = check(f, QuizType::LeaveValue, filters).unwrap();
    let keys = run(Target::Leaves(&f.en_leaves), QuizType::LeaveValue, &spec, later(), true).unwrap();
    keys.iter().map(|k| f.english.to_magpie(k)).collect()
}

fn errors(f: &Fixtures, q: QuizType, filters: Value) -> Vec<(Vec<usize>, String, String)> {
    check(f, q, filters).unwrap_err().into_iter().map(|e| (e.path, e.field, e.message)).collect()
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

// ---------------------------------------------------------------------------
// Zyzzyva's search help examples, translated from `?` to `.`
// ---------------------------------------------------------------------------

#[test]
fn zyzzyva_help_examples() {
    let f = load();
    let anagram = |p: &str| words(&f, and(vec![c("anagram_match", json!({ "pattern": p }))]));
    assert_eq!(anagram("E T X ."), s(&["EXIT", "NEXT", "SEXT", "TEXT", "VEXT"]));
    assert_eq!(anagram("P I . . Z"), s(&["PIEZO", "PIZZA", "PRIZE"]));
    assert_eq!(anagram("Z [A E I O U] [A E I O U]"), s(&["ZEA", "ZOA", "ZOO"]));
    assert_eq!(anagram("* J B X"), s(&["JUKEBOX"]));
    assert_eq!(
        anagram("A T . ."),
        s(&[
            "AUNT", "BATH", "BATS", "DATA", "LATE", "MATE", "MEAT", "OATS", "PART", "QATS", "RAPT", "STAB", "TABS",
            "TACO", "TALE", "TAME", "TARP", "TEAL", "TEAM", "TEAT", "TOAD", "TRAP", "TUNA", "ZETA"
        ])
    );
    let pattern = |p: &str| words(&f, and(vec![c("pattern_match", json!({ "pattern": p }))]));
    assert_eq!(pattern(". W * M . S"), s(&["SWAMIS", "SWAMPS", "TWASOMES"]));
    assert_eq!(pattern("T . P"), s(&["TAP", "TIP", "TOP", "TUP"]));
    let sub = |p: &str| words(&f, and(vec![c("subanagram_match", json!({ "pattern": p }))]));
    assert_eq!(sub("L X ."), s(&["A", "AL", "AX", "EL", "EX", "LA", "LAX", "LEX", "LOX", "LUX", "OX", "XU"]));
    assert_eq!(sub("L X [A U]"), s(&["A", "AL", "AX", "LA", "LAX", "LUX", "XU"]));
    assert_eq!(sub("*").len(), f.en.word_count(), "a `*` matches everything");
    // Includes Letters Q plus Not Includes U: Q without U.
    let q_not_u = words(
        &f,
        and(vec![c("includes_letters", json!({ "tiles": "Q" })), not(c("includes_letters", json!({ "tiles": "U" })))]),
    );
    assert_eq!(q_not_u, s(&["QAT", "QATS", "QI", "QIS", "QOPH", "TRANQ"]));
    // Not Includes AB matches words that lack an A or lack a B.
    let not_ab = words(&f, and(vec![not(c("includes_letters", json!({ "tiles": "AB" })))]));
    assert_eq!(not_ab.len(), 154 - 7);
    for w in ["AB", "BA", "BATH", "BATS", "BIZARRE", "STAB", "TABS"] {
        assert!(!not_ab.contains(&w.to_string()));
    }
    // Consists of AEIOU 70–100.
    let vowels = words(&f, and(vec![c("consists_of", json!({ "tiles": "AEIOU", "min": 70, "max": 100 }))]));
    assert_eq!(vowels, s(&["A", "AA", "AEON", "AIA", "AQUA", "AUDIO", "EAU", "OBOE"]));
}

#[test]
fn inner_hooks() {
    let f = load();
    let front = words(&f, and(vec![c("front_inner_hook", json!({}))]));
    assert!(front.contains(&"SPORT".to_string()));
    assert!(!front.contains(&"SPORTS".to_string()));
    assert!(!front.contains(&"A".to_string()), "a one-tile word never has one");
    let back = words(&f, and(vec![c("back_inner_hook", json!({}))]));
    assert!(back.contains(&"SPORTS".to_string()) && !back.contains(&"SPORT".to_string()));
    let not_front = words(&f, and(vec![not(c("front_inner_hook", json!({})))]));
    assert_eq!(front.len() + not_front.len(), 154);
    assert!(not_front.contains(&"A".to_string()) && not_front.contains(&"PORT".to_string()));
    let not_back = words(&f, and(vec![not(c("back_inner_hook", json!({})))]));
    assert!(not_back.contains(&"SPORT".to_string()));
}

// ---------------------------------------------------------------------------
// Groups
// ---------------------------------------------------------------------------

fn len(n: i64) -> Value {
    c("length", json!({ "min": n, "max": n }))
}

fn includes(t: &str) -> Value {
    c("includes_letters", json!({ "tiles": t }))
}

fn limit_prob(min: i64, max: i64, lax: bool) -> Value {
    c("limit_by_probability_order", json!({ "min": min, "max": max, "lax": lax }))
}

fn limit_play(min: i64, max: i64, lax: bool) -> Value {
    c("limit_by_playability_order", json!({ "min": min, "max": max, "lax": lax }))
}

/// Words ranked by (combinations desc, alphagram, word), for expectations.
fn by_probability(f: &Fixtures, ws: &[String]) -> Vec<String> {
    let mut v: Vec<_> = ws.iter().map(|w| f.en.find(&f.english.parse_magpie(w, false).unwrap()).unwrap()).collect();
    v.sort_by(|a, b| {
        b.combinations
            .cmp(&a.combinations)
            .then_with(|| f.en.alphagrams[a.alphagram as usize].cmp(&f.en.alphagrams[b.alphagram as usize]))
            .then_with(|| a.tiles.cmp(&b.tiles))
    });
    v.iter().map(|w| f.english.to_magpie(&w.tiles)).collect()
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort_by_key(|w| w.chars().collect::<Vec<_>>());
    v
}

#[test]
fn groups() {
    let f = load();
    // Length 7 AND (Includes Q OR Includes Z).
    let qz = words(&f, and(vec![len(7), or(vec![includes("Q"), includes("Z")])]));
    assert_eq!(qz, s(&["BIZARRE", "QUALITY", "QUICKEN", "QUIZZED", "SQUEEZE", "ZEALOTS", "ZIPPERS"]));

    // An OR of two AND groups.
    let a = words(&f, and(vec![len(3), includes("Z")]));
    let b = words(&f, and(vec![len(4), includes("Q")]));
    let both = words(&f, or(vec![and(vec![len(3), includes("Z")]), and(vec![len(4), includes("Q")])]));
    let mut expected = a.clone();
    expected.extend(b.clone());
    assert_eq!(sorted(both), sorted(expected));

    // A limit inside an AND group ranks that group's result within the
    // parent's predicate survivors: the 3 most probable 7s with a V.
    let sevens_v = words(&f, and(vec![len(7), includes("V")]));
    let top3: Vec<String> = by_probability(&f, &sevens_v).into_iter().take(3).collect();
    let nested = words(&f, and(vec![len(7), and(vec![includes("V"), limit_prob(1, 3, false)])]));
    assert_eq!(nested, sorted(top3));

    // A limit-only nested group under an AND parent equals the same limit
    // beside the parent's rows.
    let beside = words(&f, and(vec![len(7), limit_prob(1, 4, false)]));
    let inside = words(&f, and(vec![len(7), and(vec![limit_prob(1, 4, false)])]));
    assert_eq!(beside, inside);
    assert_eq!(beside.len(), 4);

    // A limit in a child of an OR group ranks within the OR group's candidates.
    let top_all: Vec<String> = by_probability(&f, &words(&f, and(vec![len(2)]))).into_iter().take(2).collect();
    let in_or = words(&f, and(vec![len(2), or(vec![and(vec![limit_prob(1, 2, false)]), includes("Q")])]));
    let mut exp = top_all.clone();
    exp.push("QI".into());
    exp.sort();
    exp.dedup();
    assert_eq!(in_or, sorted(exp));

    // A nested group with a limit under a top-level Length row: the same with
    // and without the candidate shortcut (checked inside `words`).
    words(&f, and(vec![len(4), or(vec![and(vec![includes("A"), limit_prob(1, 5, true)]), includes("Z")])]));
}

#[test]
fn both_limit_kinds_intersect() {
    let f = load();
    let sevens = words(&f, and(vec![len(7)]));
    let prob5: Vec<String> = by_probability(&f, &sevens).into_iter().take(5).collect();
    let mut play: Vec<_> =
        sevens.iter().map(|w| f.en.find(&f.english.parse_magpie(w, false).unwrap()).unwrap()).collect();
    play.sort_by(|a, b| b.playability.total_cmp(&a.playability));
    let play3: Vec<String> = play.iter().take(3).map(|w| f.english.to_magpie(&w.tiles)).collect();
    let expected: Vec<String> = sorted(prob5.iter().filter(|w| play3.contains(w)).cloned().collect());
    let one = words(&f, and(vec![len(7), limit_prob(1, 5, false), limit_play(1, 3, false)]));
    let two = words(&f, and(vec![len(7), limit_play(1, 3, false), limit_prob(1, 5, false)]));
    assert_eq!(one, expected, "the intersection of the two slices");
    assert_eq!(one, two, "row order never decides");
    // Neither "the 3 most playable of the 5 most probable" nor the reverse,
    // unless those happen to coincide with the intersection.
    assert!(one.len() <= 3);
}

#[test]
fn group_validation_errors() {
    let f = load();
    let q = QuizType::Definition;
    // An empty group.
    let e = errors(&f, q, and(vec![len(7), and(vec![])]));
    assert_eq!(e[0].0, vec![1]);
    assert_eq!(e[0].1, "children");
    // Nested more than 4 deep: the fifth level.
    let deep = and(vec![and(vec![and(vec![and(vec![and(vec![len(7)])])])])]);
    let e = errors(&f, q, deep);
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].0, vec![0, 0, 0, 0]);
    // More than 100 rows.
    let rows: Vec<Value> = (0..101).map(|_| len(7)).collect();
    let e = errors(&f, q, and(rows));
    assert_eq!((e[0].0.clone(), e[0].1.clone()), (vec![100], "type".to_string()));
    // 100 rows each wrapped in their own group is 101 groups: a field error.
    let wrapped: Vec<Value> = (0..100).map(|_| and(vec![len(7)])).collect();
    let e = errors(&f, q, and(wrapped));
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].0, vec![99]);
    assert!(e[0].2.contains("100 groups"));
}

// ---------------------------------------------------------------------------
// Limits and lax
// ---------------------------------------------------------------------------

/// The nine AEINRST anagrams tie on combinations in the 7s.
fn tie_block(f: &Fixtures) -> (u32, u32) {
    let w = f.en.find(&f.english.parse_magpie("RETAINS", false).unwrap()).unwrap();
    (w.min_probability_order, w.max_probability_order)
}

#[test]
fn lax_and_strict_order_filters() {
    let f = load();
    let (lo, hi) = tie_block(&f);
    assert_eq!(hi - lo, 8);
    let po = |min: u32, max: u32, lax: bool| {
        words(&f, and(vec![len(7), c("probability_order", json!({ "min": min, "max": max, "lax": lax }))]))
    };
    assert_eq!(po(lo + 2, lo + 2, false).len(), 1, "strict compares the unique rank");
    assert_eq!(po(lo + 2, lo + 2, true).len(), 9, "lax takes the whole tie group");
    assert_eq!(po(hi, hi, true).len(), 9);
    assert_eq!(po(hi, hi, false).len(), 1);
}

#[test]
fn limits() {
    let f = load();
    let (lo, _) = tie_block(&f);
    // A group holding only a limit row ranks every candidate.
    let top = words(&f, and(vec![limit_prob(1, 3, false)]));
    assert_eq!(top, sorted(by_probability(&f, &words(&f, or(vec![includes("A"), not(includes("A"))]))).into_iter().take(3).collect()));
    // A limit in an OR group ranks the union.
    let u = words(&f, or(vec![includes("Q"), includes("Z"), limit_prob(1, 2, false)]));
    let union: Vec<String> = sorted({
        let mut v = words(&f, and(vec![includes("Q")]));
        v.extend(words(&f, and(vec![includes("Z")])));
        v.sort();
        v.dedup();
        v
    });
    let expected: Vec<String> = sorted(by_probability(&f, &union).into_iter().take(2).collect());
    assert_eq!(u, expected);
    // Lax widens freely; strict does not widen; strict caps lax.
    let lim = |rows: Vec<Value>| {
        let mut v = vec![len(7)];
        v.extend(rows);
        words(&f, and(v))
    };
    let lo = lo as i64;
    assert_eq!(lim(vec![limit_prob(lo + 1, lo + 1, true)]).len(), 9);
    assert_eq!(lim(vec![limit_prob(lo + 1, lo + 1, false)]).len(), 1);
    assert_eq!(lim(vec![limit_prob(lo + 1, lo + 2, true), limit_prob(lo, lo + 4, false)]).len(), 5);
    // A min past the last survivor gives an empty result.
    assert!(lim(vec![limit_prob(100, 150, true)]).is_empty());
    // Limited words collapse to fewer alphagram questions than the range.
    let spec = check(&f, QuizType::Anagram, and(vec![len(7), limit_prob(lo, lo + 8, false)])).unwrap();
    let qs = run(Target::Words(&f.en), QuizType::Anagram, &spec, later(), true).unwrap();
    assert_eq!(qs.len(), 1);
    assert_eq!(f.english.to_magpie(&qs[0]), "AEINRST");
}

// ---------------------------------------------------------------------------
// Leave Value and leaves
// ---------------------------------------------------------------------------

fn lv(min: Option<f64>, max: Option<f64>) -> Value {
    c("leave_value", json!({ "min": min, "max": max }))
}

#[test]
fn leave_value_bounds() {
    let f = load();
    // Inclusive bounds, a value exactly equal to a bound.
    assert_eq!(leaves(&f, and(vec![lv(Some(34.1), Some(40.0))])), s(&["?AEINS", "?EIRS"]));
    // Open bounds.
    assert_eq!(leaves(&f, and(vec![lv(Some(35.0), None)])), s(&["??", "?AEINS"]));
    assert_eq!(leaves(&f, and(vec![lv(None, Some(-9.0))])), s(&["AQ", "VV"]));
    // Negative values.
    assert_eq!(leaves(&f, and(vec![lv(Some(-0.4), Some(-0.04))])), s(&["A", "EI", "T"]));
}

#[test]
fn leaves_rules() {
    let f = load();
    let pat = |t: &str, p: &str| leaves(&f, and(vec![c(t, json!({ "pattern": p }))]));
    // `.` in a leave pattern matches the blank; a literal `?` only the blank.
    assert_eq!(pat("pattern_match", ". S"), s(&["?S", "RS"]));
    assert_eq!(pat("pattern_match", "? ."), s(&["??", "?Q", "?S"]));
    assert_eq!(pat("anagram_match", "?"), s(&["?"]));
    // Number of Anagrams 0 for a leave holding a blank.
    let with_anagrams = leaves(&f, and(vec![c("num_anagrams", json!({ "min": 1, "max": 2 }))]));
    assert_eq!(with_anagrams, s(&["A", "AA", "AT", "EITX"]));
    // Number of Vowels does not count the blank.
    let two_vowels = leaves(&f, and(vec![len(5), c("num_vowels", json!({ "min": 2, "max": 2 }))]));
    assert_eq!(two_vowels, s(&["?EIRS", "EIRST"]));
    // Consists of with and without `?` in the set.
    let all_vowels = leaves(&f, and(vec![c("consists_of", json!({ "tiles": "AEIOU", "min": 100, "max": 100 }))]));
    assert_eq!(all_vowels, s(&["A", "AA", "AE", "AEIO", "E", "EE", "EI", "I", "O", "U"]));
    let blank_vowels = leaves(&f, and(vec![c("consists_of", json!({ "tiles": "?AEIOU", "min": 100, "max": 100 }))]));
    assert!(blank_vowels.contains(&"?".to_string()) && blank_vowels.contains(&"??".to_string()));
    assert_eq!(blank_vowels.len(), all_vowels.len() + 2);
    // In Word List entries in canonical leave order; invalid entries ignored.
    let listed = leaves(&f, and(vec![c("in_word_list", json!({ "entries": ["?EIRS", "AT", "ZZZ", "[NY]"] }))]));
    assert_eq!(listed, s(&["?EIRS", "AT"]));
    let e = errors(&f, QuizType::LeaveValue, and(vec![c("in_word_list", json!({ "entries": ["SEIR?"] }))]));
    assert_eq!(e[0].1, "entries");
}

#[test]
fn in_lexicon_negated_against_a_second_lexicon() {
    let f = load();
    let new = words(&f, and(vec![not(c("in_lexicon", json!({ "lexicon": "EN-FIX-OLD" })))]));
    assert_eq!(new, s(&["JUKEBOX", "QUIZZED", "SPORTS", "TRANQ", "VEXT", "ZEPPELIN"]));
    let old = words(&f, and(vec![c("in_lexicon", json!({ "lexicon": "EN-FIX-OLD" }))]));
    assert_eq!(old.len() + new.len(), 154);
}

// ---------------------------------------------------------------------------
// Validation, one case per message
// ---------------------------------------------------------------------------

fn field(f: &Fixtures, q: QuizType, row: Value) -> Option<(String, String)> {
    match check(f, q, and(vec![row])) {
        Ok(_) => None,
        Err(e) => Some((e[0].field.clone(), e[0].message.clone())),
    }
}

#[test]
fn validation_messages() {
    let f = load();
    let w = QuizType::Definition;
    let range = |t: &str, min: i64, max: i64| c(t, json!({ "min": min, "max": max }));
    let order = |t: &str, min: i64, max: i64| c(t, json!({ "min": min, "max": max, "lax": true }));
    let n = f.en.max_order_rank as i64;
    let count = f.en.word_count() as i64;
    assert!(field(&f, w, range("length", 7, 5)).unwrap().1.contains("above the maximum"));
    // Ranges that narrow nothing, measured against each filter's own floor.
    assert!(field(&f, w, range("length", 1, 15)).unwrap().1.contains("narrows nothing"));
    assert!(field(&f, w, order("probability_order", 1, n)).unwrap().1.contains("narrows nothing"));
    assert!(field(&f, w, c("consists_of", json!({ "tiles": "AEIOU", "min": 0, "max": 100 }))).unwrap().1.contains("narrows nothing"));
    assert!(field(&f, w, range("length", 2, 15)).is_none());
    assert!(field(&f, w, range("length", 1, 14)).is_none());
    // N is the largest length bucket, not the word count.
    let (fld, msg) = field(&f, w, order("probability_order", 1, count)).unwrap();
    assert_eq!(fld, "max");
    assert!(msg.contains(&n.to_string()), "{msg}");
    assert!(field(&f, w, order("limit_by_probability_order", 1, count)).unwrap().1.contains("narrows nothing"));
    assert!(field(&f, w, order("limit_by_probability_order", 1, count - 1)).is_none());
    // Number of Anagrams against the target's largest count.
    let a = f.en.max_num_anagrams as i64;
    assert!(field(&f, w, range("num_anagrams", 0, a)).unwrap().1.contains("narrows nothing"));
    assert!(field(&f, w, range("num_anagrams", 0, a - 1)).is_none());
    assert!(field(&f, w, range("num_anagrams", 0, a + 1)).unwrap().1.contains(&a.to_string()));
    let la = f.en_leaves.max_num_anagrams as i64;
    assert!(la < a);
    let lvq = QuizType::LeaveValue;
    assert!(field(&f, lvq, range("num_anagrams", 0, la)).unwrap().1.contains("narrows nothing"));
    assert!(field(&f, lvq, range("num_anagrams", 0, a)).unwrap().1.contains(&la.to_string()));
    // The quiz type's own ceiling on a Leave Value cascade.
    assert!(field(&f, lvq, range("length", 1, 6)).unwrap().1.contains("narrows nothing"));
    assert!(field(&f, lvq, range("num_vowels", 0, 6)).unwrap().1.contains("narrows nothing"));
    let (fld, msg) = field(&f, lvq, range("length", 1, 15)).unwrap();
    assert_eq!((fld.as_str(), msg.contains('6')), ("max", true));
    assert!(field(&f, lvq, range("length", 1, 5)).is_none());
    let pv = 6 * i64::from(f.english.max_value());
    assert!(field(&f, lvq, range("point_value", 0, pv + 1)).unwrap().1.contains(&pv.to_string()));
    // `?` in a word pattern or a word cascade's tile list.
    assert!(field(&f, w, c("pattern_match", json!({ "pattern": "? A" }))).unwrap().1.contains("use `.`"));
    assert!(field(&f, w, c("includes_letters", json!({ "tiles": "A?" }))).unwrap().1.contains("use `.`"));
    // A Leave Value row with both bounds blank.
    assert_eq!(field(&f, lvq, lv(None, None)).unwrap().0, "min");
    // A filter that does not apply to the quiz type.
    assert_eq!(field(&f, lvq, c("definition", json!({ "text": "x" }))).unwrap().0, "type");
    assert_eq!(field(&f, w, lv(Some(1.0), None)).unwrap().0, "type");
    // In Lexicon on another distribution.
    assert!(field(&f, w, c("in_lexicon", json!({ "lexicon": "CA-FIX" }))).unwrap().1.contains("distribution"));
    // A pattern that is not in canonical form.
    assert!(field(&f, w, c("pattern_match", json!({ "pattern": "T.P" }))).is_some());
    assert!(field(&f, w, c("pattern_match", json!({ "pattern": "T  . P" }))).is_some());
    // Negation only where Zyzzyva allows it.
    assert_eq!(field(&f, w, not(range("length", 2, 3))).unwrap().0, "negated");
}

#[test]
fn word_list_totals() {
    let f = load();
    let rows = |sizes: [usize; 4]| {
        and(sizes
            .iter()
            .map(|&n| c("in_word_list", json!({ "entries": vec!["QI"; n] })))
            .collect())
    };
    assert!(check(&f, QuizType::Definition, rows([75_000, 75_000, 75_000, 75_000])).is_ok());
    let e = errors(&f, QuizType::Definition, rows([75_000, 75_000, 75_000, 75_001]));
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].0, vec![3], "the row that crosses the total");
    assert!(e[0].2.contains("300000") && e[0].2.contains("300001"), "{}", e[0].2);
}

#[test]
fn canonical_pattern_length_limit() {
    let f = load();
    let consonants: Vec<&str> = f.english.tiles[1..]
        .iter()
        .filter(|t| !t.is_vowel)
        .map(|t| t.letter.as_str())
        .collect();
    let set = format!("[{}]", consonants.join(" "));
    let long = vec![set.as_str(); 15].join(" ");
    assert!(long.chars().count() > 500);
    let (fld, msg) = field(&f, QuizType::Definition, c("pattern_match", json!({ "pattern": long }))).unwrap();
    assert_eq!(fld, "pattern");
    assert!(msg.contains("500"));
    // One just under the limit is accepted.
    let mut shorter = vec![set.as_str(); 15];
    let small = "[B C]";
    let mut count = long.chars().count();
    let mut i = 0;
    while count > 500 {
        shorter[i] = small;
        count = shorter.join(" ").chars().count();
        i += 1;
    }
    assert!(count <= 500);
    assert!(field(&f, QuizType::Definition, c("pattern_match", json!({ "pattern": shorter.join(" ") }))).is_none());
}

// ---------------------------------------------------------------------------
// The search deadline
// ---------------------------------------------------------------------------

#[test]
fn a_broad_search_past_its_deadline_is_too_broad() {
    use crate::catalog::index::{LexiconIndex, RawWord};
    let f = load();
    let d = f.english.clone();
    // Enough words that a scan with definition checks takes over 1 ms.
    let mut raw = Vec::new();
    let mut x: u64 = 1;
    for _ in 0..200_000 {
        let mut tiles = Vec::new();
        for _ in 0..8 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            tiles.push(1 + ((x >> 33) % 26) as u8);
        }
        raw.push(RawWord { tiles, playability: 1.0, definition: "a made-up word [n]".into() });
    }
    let big = LexiconIndex::build(9, "BIG", d, raw);
    let spec = {
        let snap = Snapshot::default();
        let lex = std::sync::Arc::new(big);
        let tree = parse_tree(&and(vec![c("definition", json!({ "text": "zzz" }))])).unwrap();
        let info = TargetInfo { lexicon: &lex, leaves: None, snapshot: &snap };
        let spec = validate(&tree, QuizType::Definition, Some(&info)).unwrap().unwrap();
        let deadline = Instant::now() + Duration::from_millis(1);
        let r = run(Target::Words(&lex), QuizType::Definition, &spec, deadline, true);
        assert_eq!(r, Err(SearchError::TooBroad));
        spec
    };
    let _ = spec;
}

// ---------------------------------------------------------------------------
// Property tests (proptest)
// ---------------------------------------------------------------------------

mod props {
    use super::*;
    use proptest::prelude::*;

    fn leaf() -> impl Strategy<Value = Value> {
        prop_oneof![
            (1i64..=8, 0i64..=4).prop_map(|(a, d)| c("length", json!({ "min": a, "max": (a + d).min(15) }))),
            (0i64..=3, 0i64..=2).prop_map(|(a, d)| c("num_vowels", json!({ "min": a, "max": a + d }))),
            prop::sample::select(vec!["A", "E", "S", "T", "Q", "Z", "AE", "ST"]).prop_map(|t| includes(t)),
            prop::sample::select(vec!["A", "E", "S", "T", "Q", "Z"]).prop_map(|t| not(includes(t))),
            prop::sample::select(vec!["A T", "E I N R S T A", "Q I", "T . P", ". . . .", "E T X ."])
                .prop_map(|p| c("anagram_match", json!({ "pattern": p }))),
            prop::sample::select(vec!["T *", "* S", ". A *", "S . . . ."])
                .prop_map(|p| c("pattern_match", json!({ "pattern": p }))),
            (1i64..=10, 0i64..=10, any::<bool>())
                .prop_map(|(a, d, lax)| c("probability_order", json!({ "min": a, "max": a + d, "lax": lax }))),
            (1i64..=20, 0i64..=20, any::<bool>()).prop_map(|(a, d, lax)| limit_prob(a, a + d, lax)),
            (1i64..=20, 0i64..=20, any::<bool>()).prop_map(|(a, d, lax)| limit_play(a, a + d, lax)),
        ]
    }

    fn tree() -> impl Strategy<Value = Value> {
        leaf().prop_recursive(3, 12, 4, |inner| {
            (any::<bool>(), prop::collection::vec(inner, 1..4)).prop_map(|(is_and, children)| {
                if is_and {
                    and(children)
                } else {
                    or(children)
                }
            })
        })
    }

    fn top() -> impl Strategy<Value = Value> {
        prop::collection::vec(tree(), 1..4).prop_map(|mut v| {
            // Give the shortcut something to use: a top-level Length row.
            v.insert(0, c("length", json!({ "min": 2, "max": 8 })));
            and(v)
        })
    }

    fn ids(f: &Fixtures, filters: &Value, shortcuts: bool) -> Option<Vec<u32>> {
        let spec = check(f, QuizType::Definition, filters.clone()).ok()?;
        Some(run_ids(Target::Words(&f.en), &spec, later(), shortcuts).unwrap())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        /// The shortcut candidate paths return the same results as a full scan.
        #[test]
        fn shortcuts_match_a_full_scan(filters in top()) {
            let f = load();
            if let Some(a) = ids(&f, &filters, true) {
                prop_assert_eq!(a, ids(&f, &filters, false).unwrap());
            }
        }

        /// Negating a predicate partitions the candidates.
        #[test]
        fn negation_partitions(t in prop::sample::select(vec!["A", "E", "S", "Q", "AE"])) {
            let f = load();
            let yes = ids(&f, &and(vec![includes(t)]), true).unwrap();
            let no = ids(&f, &and(vec![not(includes(t))]), true).unwrap();
            prop_assert_eq!(yes.len() + no.len(), f.en.word_count());
            prop_assert!(yes.iter().all(|i| !no.contains(i)));
        }

        /// OR is the union of its children, AND the intersection, and a limit
        /// inside a group is a subset of that group's unlimited result.
        #[test]
        fn and_or_and_limit_subsets(a in leaf(), b in leaf(), lim in (1i64..=10, 0i64..=10, any::<bool>())) {
            let f = load();
            let (Some(ra), Some(rb)) = (ids(&f, &and(vec![a.clone()]), true), ids(&f, &and(vec![b.clone()]), true)) else {
                return Ok(());
            };
            let is_limit = |v: &Value| v["type"].as_str().unwrap().starts_with("limit");
            if !is_limit(&a) && !is_limit(&b) {
                let union = ids(&f, &or(vec![a.clone(), b.clone()]), true).unwrap();
                let mut u = ra.clone();
                u.extend(&rb);
                u.sort_unstable();
                u.dedup();
                prop_assert_eq!(union, u);
                let inter = ids(&f, &and(vec![a.clone(), b.clone()]), true).unwrap();
                let i: Vec<u32> = ra.iter().copied().filter(|x| rb.contains(x)).collect();
                prop_assert_eq!(inter, i);
                let unlimited = ids(&f, &and(vec![a.clone()]), true).unwrap();
                let limited = ids(&f, &and(vec![a.clone(), limit_prob(lim.0, lim.0 + lim.1, lim.2)]), true).unwrap();
                prop_assert!(limited.iter().all(|x| unlimited.contains(x)));
            }
        }

        /// Anagram Match without wildcards returns exactly the alphagram map entry.
        #[test]
        fn literal_anagram_is_the_alphagram_entry(i in 0usize..154) {
            let f = load();
            let w = &f.en.words[i];
            let alpha = &f.en.alphagrams[w.alphagram as usize];
            let pat: Vec<String> = alpha.iter().map(|&t| f.english.tile(t).letter.clone()).collect();
            let got = ids(&f, &and(vec![c("anagram_match", json!({ "pattern": pat.join(" ") }))]), true).unwrap();
            prop_assert_eq!(got, f.en.alphagram_words[w.alphagram as usize].clone());
        }

        /// The leave target's shortcuts match a full scan too.
        #[test]
        fn leave_shortcuts_match(min in 1i64..=6, d in 0i64..=3, lit in prop::sample::select(vec!["A T", "? E I R S", "Q", "Z Z"])) {
            let f = load();
            let filters = and(vec![
                c("length", json!({ "min": min, "max": (min + d).min(6) })),
                c("anagram_match", json!({ "pattern": lit })),
            ]);
            if let Ok(spec) = check(&f, QuizType::LeaveValue, filters) {
                let a = run_ids(Target::Leaves(&f.en_leaves), &spec, later(), true).unwrap();
                let b = run_ids(Target::Leaves(&f.en_leaves), &spec, later(), false).unwrap();
                prop_assert_eq!(a, b);
            }
        }
    }
}
