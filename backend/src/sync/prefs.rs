//! `set_preferences` and `set_bindings` validation (PLAN.md § Operations,
//! § Preferences, § Controls). Every carried value is checked before any write,
//! so an out-of-range value is `invalid`, never a constraint violation.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::cascade::rules::Reason;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "input_action", rename_all = "snake_case")]
pub enum InputAction {
    ShowNext,
    ToggleGrade,
    Previous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "input_kind", rename_all = "snake_case")]
pub enum InputKind {
    MouseButton,
    Wheel,
    Key,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "anagram_answer_mode", rename_all = "snake_case")]
pub enum AnswerMode {
    Flashcard,
    Typed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub action: InputAction,
    pub kind: InputKind,
    pub code: String,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub meta: bool,
}

/// The preference fields a `set_preferences` may carry, typed and in range.
#[derive(Debug, Clone, Default)]
pub struct PreferenceChanges {
    pub default_clear_threshold: Option<i16>,
    pub leave_value_decimals: Option<i16>,
    pub anagram_show_definitions: Option<bool>,
    pub anagram_show_hooks: Option<bool>,
    pub anagram_answer_mode: Option<AnswerMode>,
    pub default_segment_size: Option<i32>,
    pub default_progression: Option<crate::cascade::rows::Progression>,
    pub default_require_alphabetical: Option<bool>,
}

pub const PREFERENCE_FIELDS: [&str; 8] = [
    "default_clear_threshold",
    "leave_value_decimals",
    "anagram_show_definitions",
    "anagram_show_hooks",
    "anagram_answer_mode",
    "default_segment_size",
    "default_progression",
    "default_require_alphabetical",
];

fn int_in(v: &Value, lo: i64, hi: i64) -> Result<i64, Reason> {
    v.as_i64().filter(|n| (lo..=hi).contains(n)).ok_or(Reason::Invalid)
}

fn boolean(v: &Value) -> Result<bool, Reason> {
    v.as_bool().ok_or(Reason::Invalid)
}

pub fn parse_preferences(fields: &Map<String, Value>, cap: u32) -> Result<PreferenceChanges, Reason> {
    let mut c = PreferenceChanges::default();
    for (k, v) in fields {
        match k.as_str() {
            "default_clear_threshold" => c.default_clear_threshold = Some(int_in(v, 1, 100)? as i16),
            "leave_value_decimals" => c.leave_value_decimals = Some(int_in(v, 0, 3)? as i16),
            "anagram_show_definitions" => c.anagram_show_definitions = Some(boolean(v)?),
            "anagram_show_hooks" => c.anagram_show_hooks = Some(boolean(v)?),
            "anagram_answer_mode" => {
                c.anagram_answer_mode = Some(serde_json::from_value(v.clone()).map_err(|_| Reason::Invalid)?)
            }
            "default_segment_size" => {
                let n = v.as_i64().ok_or(Reason::Invalid)?;
                c.default_segment_size = Some(crate::cascade::rules::check_segment_size(n, cap)? as i32);
            }
            "default_progression" => {
                c.default_progression = Some(serde_json::from_value(v.clone()).map_err(|_| Reason::Invalid)?)
            }
            "default_require_alphabetical" => c.default_require_alphabetical = Some(boolean(v)?),
            _ => return Err(Reason::Invalid),
        }
    }
    Ok(c)
}

/// The full list: every action has 1–3 bindings, no stroke is used twice, and
/// every binding is one the schema allows.
pub fn validate_bindings(v: &Value) -> Result<Vec<Binding>, Reason> {
    let list: Vec<Binding> = serde_json::from_value(v.clone()).map_err(|_| Reason::Invalid)?;
    let mut strokes = HashSet::new();
    for b in &list {
        let ok = match b.kind {
            InputKind::MouseButton => matches!(b.code.as_str(), "left" | "middle" | "right" | "back" | "forward"),
            InputKind::Wheel => matches!(b.code.as_str(), "up" | "down"),
            InputKind::Key => {
                (1..=32).contains(&b.code.len())
                    && b.code.bytes().all(|c| c.is_ascii_alphanumeric())
                    && b.code != "Escape"
            }
        };
        if !ok {
            return Err(Reason::Invalid);
        }
        if !strokes.insert((b.kind, b.code.clone(), b.ctrl, b.shift, b.alt, b.meta)) {
            return Err(Reason::Invalid);
        }
    }
    for action in [InputAction::ShowNext, InputAction::ToggleGrade, InputAction::Previous] {
        let n = list.iter().filter(|b| b.action == action).count();
        if !(1..=3).contains(&n) {
            return Err(Reason::Invalid);
        }
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn defaults() -> Value {
        json!([
            { "action": "show_next", "kind": "mouse_button", "code": "left" },
            { "action": "show_next", "kind": "key", "code": "Space" },
            { "action": "toggle_grade", "kind": "mouse_button", "code": "right" },
            { "action": "toggle_grade", "kind": "key", "code": "KeyX" },
            { "action": "previous", "kind": "mouse_button", "code": "middle" },
            { "action": "previous", "kind": "key", "code": "Backspace" }
        ])
    }

    #[test]
    fn bindings_rules() {
        assert!(validate_bindings(&defaults()).is_ok());
        let mut v = defaults();
        v[1]["code"] = json!("Escape");
        assert_eq!(validate_bindings(&v), Err(Reason::Invalid));
        let mut v = defaults();
        v[0]["code"] = json!("thumb");
        assert_eq!(validate_bindings(&v), Err(Reason::Invalid));
        let mut v = defaults();
        v.as_array_mut().unwrap().push(json!({ "action": "previous", "kind": "wheel", "code": "up" }));
        v.as_array_mut().unwrap().push(json!({ "action": "previous", "kind": "wheel", "code": "down" }));
        assert_eq!(validate_bindings(&v), Err(Reason::Invalid), "a fourth binding");
        let mut v = defaults();
        v[3]["code"] = json!("Space");
        v[3]["action"] = json!("toggle_grade");
        assert_eq!(validate_bindings(&v), Err(Reason::Invalid), "a stroke used twice");
        let v = json!([{ "action": "show_next", "kind": "key", "code": "Space" }]);
        assert_eq!(validate_bindings(&v), Err(Reason::Invalid), "every action keeps one");
    }

    #[test]
    fn preference_ranges() {
        let m = |v: Value| v.as_object().unwrap().clone();
        assert!(parse_preferences(&m(json!({ "default_clear_threshold": 0 })), 300_000).is_err());
        assert!(parse_preferences(&m(json!({ "leave_value_decimals": 4 })), 300_000).is_err());
        assert!(parse_preferences(&m(json!({ "default_segment_size": 4 })), 300_000).is_err());
        assert!(parse_preferences(&m(json!({ "anagram_answer_mode": "typed", "default_progression": "drill" })), 300_000).is_ok());
    }
}
