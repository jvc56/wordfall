//! The filter tree as it travels (PLAN.md § API → Catalog and search): `filters`
//! is the top group, a group is `{ "op": "and" | "or", "children": [] }`, and a
//! child is a group or a condition, told apart by `op` versus `type`. Only the
//! parameters a type uses are accepted; any extra field is a `400`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::catalog::pos::PartOfSpeech;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "quiz_type", rename_all = "snake_case")]
pub enum QuizType {
    Anagram,
    Definition,
    LeaveValue,
}

impl QuizType {
    pub fn is_leave(self) -> bool {
        self == QuizType::LeaveValue
    }

    pub fn as_str(self) -> &'static str {
        match self {
            QuizType::Anagram => "anagram",
            QuizType::Definition => "definition",
            QuizType::LeaveValue => "leave_value",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "group_op", rename_all = "snake_case")]
pub enum GroupOp {
    And,
    Or,
}

/// The 23 condition types, in the order of Zyzzyva's dropdown and then the
/// three Wordfall-only filters (PLAN.md § Filter reference).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "condition_type", rename_all = "snake_case")]
pub enum ConditionType {
    AnagramMatch,
    PatternMatch,
    SubanagramMatch,
    Length,
    InLexicon,
    InWordList,
    NumVowels,
    IncludesLetters,
    ProbabilityOrder,
    LimitByProbabilityOrder,
    PlayabilityOrder,
    LimitByPlayabilityOrder,
    NumUniqueLetters,
    PointValue,
    TakesPrefix,
    TakesSuffix,
    PartOfSpeech,
    Definition,
    ConsistsOf,
    NumAnagrams,
    FrontInnerHook,
    BackInnerHook,
    LeaveValue,
}

impl ConditionType {
    pub const ALL: [ConditionType; 23] = [
        ConditionType::AnagramMatch,
        ConditionType::PatternMatch,
        ConditionType::SubanagramMatch,
        ConditionType::Length,
        ConditionType::InLexicon,
        ConditionType::InWordList,
        ConditionType::NumVowels,
        ConditionType::IncludesLetters,
        ConditionType::ProbabilityOrder,
        ConditionType::LimitByProbabilityOrder,
        ConditionType::PlayabilityOrder,
        ConditionType::LimitByPlayabilityOrder,
        ConditionType::NumUniqueLetters,
        ConditionType::PointValue,
        ConditionType::TakesPrefix,
        ConditionType::TakesSuffix,
        ConditionType::PartOfSpeech,
        ConditionType::Definition,
        ConditionType::ConsistsOf,
        ConditionType::NumAnagrams,
        ConditionType::FrontInnerHook,
        ConditionType::BackInnerHook,
        ConditionType::LeaveValue,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ConditionType::AnagramMatch => "anagram_match",
            ConditionType::PatternMatch => "pattern_match",
            ConditionType::SubanagramMatch => "subanagram_match",
            ConditionType::Length => "length",
            ConditionType::InLexicon => "in_lexicon",
            ConditionType::InWordList => "in_word_list",
            ConditionType::NumVowels => "num_vowels",
            ConditionType::IncludesLetters => "includes_letters",
            ConditionType::ProbabilityOrder => "probability_order",
            ConditionType::LimitByProbabilityOrder => "limit_by_probability_order",
            ConditionType::PlayabilityOrder => "playability_order",
            ConditionType::LimitByPlayabilityOrder => "limit_by_playability_order",
            ConditionType::NumUniqueLetters => "num_unique_letters",
            ConditionType::PointValue => "point_value",
            ConditionType::TakesPrefix => "takes_prefix",
            ConditionType::TakesSuffix => "takes_suffix",
            ConditionType::PartOfSpeech => "part_of_speech",
            ConditionType::Definition => "definition",
            ConditionType::ConsistsOf => "consists_of",
            ConditionType::NumAnagrams => "num_anagrams",
            ConditionType::FrontInnerHook => "front_inner_hook",
            ConditionType::BackInnerHook => "back_inner_hook",
            ConditionType::LeaveValue => "leave_value",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.as_str() == s)
    }

    /// Not is only allowed where Zyzzyva allows it.
    pub fn negatable(self) -> bool {
        use ConditionType::*;
        matches!(
            self,
            AnagramMatch
                | PatternMatch
                | SubanagramMatch
                | InLexicon
                | InWordList
                | IncludesLetters
                | TakesPrefix
                | TakesSuffix
                | PartOfSpeech
                | Definition
                | FrontInnerHook
                | BackInnerHook
        )
    }

    pub fn is_limit(self) -> bool {
        matches!(self, ConditionType::LimitByProbabilityOrder | ConditionType::LimitByPlayabilityOrder)
    }

    /// PLAN.md § Filter applicability by quiz type.
    pub fn applies_to(self, q: QuizType) -> bool {
        use ConditionType::*;
        match self {
            LeaveValue => q == QuizType::LeaveValue,
            InLexicon | PlayabilityOrder | LimitByPlayabilityOrder | TakesPrefix | TakesSuffix
            | FrontInnerHook | BackInnerHook | PartOfSpeech | Definition => q != QuizType::LeaveValue,
            _ => true,
        }
    }

    /// The wire parameter fields, in the order of the parameter table.
    pub fn fields(self) -> &'static [&'static str] {
        use ConditionType::*;
        match self {
            AnagramMatch | PatternMatch | SubanagramMatch => &["pattern"],
            Length | NumVowels | NumUniqueLetters | PointValue | NumAnagrams => &["min", "max"],
            ProbabilityOrder | LimitByProbabilityOrder | PlayabilityOrder | LimitByPlayabilityOrder => {
                &["min", "max", "lax"]
            }
            ConsistsOf => &["tiles", "min", "max"],
            IncludesLetters | TakesPrefix | TakesSuffix => &["tiles"],
            InLexicon => &["lexicon"],
            InWordList => &["entries"],
            PartOfSpeech => &["part_of_speech"],
            Definition => &["text"],
            FrontInnerHook | BackInnerHook => &[],
            LeaveValue => &["min", "max"],
        }
    }
}

/// A condition's parameters as sent, before they are checked against a quiz
/// type or parsed into tiles. Integers are `i64` so a negative or oversized
/// bound reaches validation as a field error rather than a parse failure.
#[derive(Debug, Clone, PartialEq)]
pub enum Params {
    Pattern(String),
    Range { min: i64, max: i64 },
    Order { min: i64, max: i64, lax: bool },
    ConsistsOf { tiles: String, min: i64, max: i64 },
    Tiles(String),
    Lexicon(String),
    Entries(Vec<String>),
    PartOfSpeech(PartOfSpeech),
    Text(String),
    None,
    LeaveValue { min: Option<f64>, max: Option<f64> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct WireCondition {
    pub ctype: ConditionType,
    pub negated: bool,
    pub params: Params,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WireNode {
    Group(WireGroup),
    Condition(WireCondition),
}

#[derive(Debug, Clone, PartialEq)]
pub struct WireGroup {
    pub op: GroupOp,
    pub children: Vec<WireNode>,
}

/// An error keyed by the row's path: its child indexes from the top group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PathError {
    pub path: Vec<usize>,
    pub field: String,
    pub message: String,
}

impl PathError {
    pub fn new(path: &[usize], field: &str, message: impl Into<String>) -> Self {
        PathError { path: path.to_vec(), field: field.to_owned(), message: message.into() }
    }
}

pub fn parse_tree(v: &Value) -> Result<WireGroup, Vec<PathError>> {
    let mut errors = Vec::new();
    let g = parse_group(v, &mut Vec::new(), &mut errors);
    match g {
        Some(g) if errors.is_empty() => Ok(g),
        _ => Err(errors),
    }
}

fn parse_group(v: &Value, path: &mut Vec<usize>, errors: &mut Vec<PathError>) -> Option<WireGroup> {
    let Some(obj) = v.as_object() else {
        errors.push(PathError::new(path, "op", "a group must be an object"));
        return None;
    };
    for k in obj.keys() {
        if k != "op" && k != "children" {
            errors.push(PathError::new(path, k, format!("unexpected field {k:?}")));
        }
    }
    let op = match obj.get("op").and_then(Value::as_str) {
        Some("and") => Some(GroupOp::And),
        Some("or") => Some(GroupOp::Or),
        _ => {
            errors.push(PathError::new(path, "op", "op must be \"and\" or \"or\""));
            None
        }
    };
    let Some(children) = obj.get("children").and_then(Value::as_array) else {
        errors.push(PathError::new(path, "children", "children must be an array"));
        return None;
    };
    let mut out = Vec::with_capacity(children.len());
    for (i, c) in children.iter().enumerate() {
        path.push(i);
        let node = match c.as_object() {
            Some(o) if o.contains_key("op") && !o.contains_key("type") => {
                parse_group(c, path, errors).map(WireNode::Group)
            }
            Some(o) if o.contains_key("type") && !o.contains_key("op") => {
                parse_condition(o, path, errors).map(WireNode::Condition)
            }
            _ => {
                errors.push(PathError::new(path, "type", "a child is a group (op) or a condition (type)"));
                None
            }
        };
        path.pop();
        if let Some(n) = node {
            out.push(n);
        }
    }
    Some(WireGroup { op: op?, children: out })
}

fn parse_condition(o: &Map<String, Value>, path: &[usize], errors: &mut Vec<PathError>) -> Option<WireCondition> {
    let start = errors.len();
    let Some(ctype) = o.get("type").and_then(Value::as_str).and_then(ConditionType::parse) else {
        errors.push(PathError::new(path, "type", "unknown condition type"));
        return None;
    };
    let fields = ctype.fields();
    for k in o.keys() {
        if k != "type" && k != "negated" && !fields.contains(&k.as_str()) {
            errors.push(PathError::new(path, k, format!("unexpected field {k:?} for {}", ctype.as_str())));
        }
    }
    let negated = match o.get("negated") {
        Some(Value::Bool(b)) => *b,
        _ => {
            errors.push(PathError::new(path, "negated", "negated must be a boolean"));
            false
        }
    };
    let mut get = |field: &str| -> Option<&Value> {
        let v = o.get(field);
        if v.is_none() {
            errors.push(PathError::new(path, field, format!("{field} is required")));
        }
        v
    };
    let int = |v: Option<&Value>, field: &str, errors: &mut Vec<PathError>| -> i64 {
        match v.and_then(Value::as_i64) {
            Some(n) => n,
            None => {
                if v.is_some() {
                    errors.push(PathError::new(path, field, format!("{field} must be an integer")));
                }
                0
            }
        }
    };
    let string = |v: Option<&Value>, field: &str, errors: &mut Vec<PathError>| -> String {
        match v.and_then(Value::as_str) {
            Some(s) => s.to_owned(),
            None => {
                if v.is_some() {
                    errors.push(PathError::new(path, field, format!("{field} must be a string")));
                }
                String::new()
            }
        }
    };
    use ConditionType::*;
    let params = match ctype {
        AnagramMatch | PatternMatch | SubanagramMatch => {
            let v = get("pattern");
            Params::Pattern(string(v, "pattern", errors))
        }
        Length | NumVowels | NumUniqueLetters | PointValue | NumAnagrams => {
            let (a, b) = (get("min").cloned(), get("max").cloned());
            Params::Range { min: int(a.as_ref(), "min", errors), max: int(b.as_ref(), "max", errors) }
        }
        ProbabilityOrder | LimitByProbabilityOrder | PlayabilityOrder | LimitByPlayabilityOrder => {
            let (a, b, l) = (get("min").cloned(), get("max").cloned(), get("lax").cloned());
            let lax = match l {
                Some(Value::Bool(b)) => b,
                Some(_) => {
                    errors.push(PathError::new(path, "lax", "lax must be a boolean"));
                    true
                }
                None => true,
            };
            Params::Order { min: int(a.as_ref(), "min", errors), max: int(b.as_ref(), "max", errors), lax }
        }
        ConsistsOf => {
            let (t, a, b) = (get("tiles").cloned(), get("min").cloned(), get("max").cloned());
            Params::ConsistsOf {
                tiles: string(t.as_ref(), "tiles", errors),
                min: int(a.as_ref(), "min", errors),
                max: int(b.as_ref(), "max", errors),
            }
        }
        IncludesLetters | TakesPrefix | TakesSuffix => {
            let v = get("tiles").cloned();
            Params::Tiles(string(v.as_ref(), "tiles", errors))
        }
        InLexicon => {
            let v = get("lexicon").cloned();
            Params::Lexicon(string(v.as_ref(), "lexicon", errors))
        }
        InWordList => {
            let v = get("entries").cloned();
            match v {
                Some(Value::Array(items)) => {
                    let mut entries = Vec::with_capacity(items.len());
                    for it in items {
                        match it {
                            Value::String(s) => entries.push(s),
                            _ => {
                                errors.push(PathError::new(path, "entries", "every entry must be a string"));
                                break;
                            }
                        }
                    }
                    Params::Entries(entries)
                }
                Some(_) => {
                    errors.push(PathError::new(path, "entries", "entries must be an array"));
                    Params::Entries(Vec::new())
                }
                None => Params::Entries(Vec::new()),
            }
        }
        PartOfSpeech => {
            let v = get("part_of_speech").cloned();
            match v.map(serde_json::from_value::<crate::catalog::pos::PartOfSpeech>) {
                Some(Ok(p)) => Params::PartOfSpeech(p),
                Some(Err(_)) => {
                    errors.push(PathError::new(path, "part_of_speech", "unknown part of speech"));
                    Params::None
                }
                None => Params::None,
            }
        }
        Definition => {
            let v = get("text").cloned();
            Params::Text(string(v.as_ref(), "text", errors))
        }
        FrontInnerHook | BackInnerHook => Params::None,
        LeaveValue => {
            let (a, b) = (get("min").cloned(), get("max").cloned());
            let num = |v: Option<Value>, field: &str, errors: &mut Vec<PathError>| -> Option<f64> {
                match v {
                    Some(Value::Null) | None => None,
                    Some(Value::Number(n)) => n.as_f64(),
                    Some(_) => {
                        errors.push(PathError::new(path, field, format!("{field} must be a number or null")));
                        None
                    }
                }
            };
            Params::LeaveValue { min: num(a, "min", errors), max: num(b, "max", errors) }
        }
    };
    if errors.len() > start {
        return None;
    }
    Some(WireCondition { ctype, negated, params })
}

pub fn condition_to_json(c: &WireCondition) -> Value {
    let mut m = Map::new();
    m.insert("type".into(), json!(c.ctype.as_str()));
    m.insert("negated".into(), json!(c.negated));
    match &c.params {
        Params::Pattern(p) => {
            m.insert("pattern".into(), json!(p));
        }
        Params::Range { min, max } => {
            m.insert("min".into(), json!(min));
            m.insert("max".into(), json!(max));
        }
        Params::Order { min, max, lax } => {
            m.insert("min".into(), json!(min));
            m.insert("max".into(), json!(max));
            m.insert("lax".into(), json!(lax));
        }
        Params::ConsistsOf { tiles, min, max } => {
            m.insert("tiles".into(), json!(tiles));
            m.insert("min".into(), json!(min));
            m.insert("max".into(), json!(max));
        }
        Params::Tiles(t) => {
            m.insert("tiles".into(), json!(t));
        }
        Params::Lexicon(l) => {
            m.insert("lexicon".into(), json!(l));
        }
        Params::Entries(e) => {
            m.insert("entries".into(), json!(e));
        }
        Params::PartOfSpeech(p) => {
            m.insert("part_of_speech".into(), json!(p));
        }
        Params::Text(t) => {
            m.insert("text".into(), json!(t));
        }
        Params::None => {}
        Params::LeaveValue { min, max } => {
            m.insert("min".into(), json!(min));
            m.insert("max".into(), json!(max));
        }
    }
    Value::Object(m)
}

pub fn tree_to_json(g: &WireGroup) -> Value {
    let children: Vec<Value> = g
        .children
        .iter()
        .map(|c| match c {
            WireNode::Group(g) => tree_to_json(g),
            WireNode::Condition(c) => condition_to_json(c),
        })
        .collect();
    let mut m = Map::new();
    m.insert("op".into(), json!(g.op));
    m.insert("children".into(), Value::Array(children));
    Value::Object(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_round_trips() {
        let v = json!({ "op": "and", "children": [
            { "type": "length", "negated": false, "min": 7, "max": 7 },
            { "op": "or", "children": [
                { "type": "includes_letters", "negated": false, "tiles": "Q" },
                { "type": "includes_letters", "negated": false, "tiles": "Z" } ] },
            { "type": "probability_order", "negated": false, "min": 1, "max": 1000, "lax": true },
            { "type": "leave_value", "negated": false, "min": 10.0, "max": null },
            { "type": "front_inner_hook", "negated": true } ] });
        let t = parse_tree(&v).unwrap();
        assert_eq!(tree_to_json(&t), v);
    }

    #[test]
    fn extra_and_missing_fields_are_errors_keyed_by_path() {
        let v = json!({ "op": "and", "children": [
            { "type": "length", "negated": false, "min": 1, "max": 7, "lax": true },
            { "op": "or", "children": [ { "type": "definition", "text": "x" } ] },
            { "type": "front_inner_hook", "negated": false, "pattern": "A" } ] });
        let e = parse_tree(&v).unwrap_err();
        let keys: Vec<(Vec<usize>, String)> = e.iter().map(|e| (e.path.clone(), e.field.clone())).collect();
        assert_eq!(
            keys,
            vec![(vec![0], "lax".into()), (vec![1, 0], "negated".into()), (vec![2], "pattern".into())]
        );
    }
}
