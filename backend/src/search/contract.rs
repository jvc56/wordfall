//! PLAN.md § Contract fixtures → Contract test: the backend condition schema
//! checked against `contract-fixtures/filters/conditions.json`, which is
//! generated from PLAN.md's tables by `contract-fixtures/tools/gen_filters.py`.

use serde_json::{json, Value};

use super::wire::{condition_to_json, parse_tree, ConditionType, QuizType, WireNode};

fn fixture() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../contract-fixtures/filters/conditions.json");
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn the_condition_schema_matches_the_fixture() {
    let f = fixture();
    let types = f["types"].as_array().unwrap();
    assert_eq!(types.len(), ConditionType::ALL.len());
    for (t, ct) in types.iter().zip(ConditionType::ALL) {
        assert_eq!(t["type"], ct.as_str());
        assert_eq!(t["fields"], json!(ct.fields()), "{}", ct.as_str());
        assert_eq!(t["negatable"], ct.negatable(), "{}", ct.as_str());
        assert_eq!(t["limit"], ct.is_limit(), "{}", ct.as_str());
        let applies: Vec<&str> = [QuizType::Anagram, QuizType::Definition, QuizType::LeaveValue]
            .into_iter()
            .filter(|q| ct.applies_to(*q))
            .map(|q| q.as_str())
            .collect();
        assert_eq!(t["applies_to"], json!(applies), "{}", ct.as_str());
        assert_eq!(t["label"], super::validate::label(ct), "{}", ct.as_str());
    }
}

#[test]
fn every_valid_condition_serialises_byte_for_byte() {
    let f = fixture();
    for v in f["valid"].as_array().unwrap() {
        let tree = parse_tree(&json!({ "op": "and", "children": [v["condition"].clone()] })).unwrap();
        let WireNode::Condition(c) = &tree.children[0] else { panic!() };
        let text = serde_json::to_string(&condition_to_json(c)).unwrap();
        assert_eq!(text, v["text"].as_str().unwrap());
    }
}

#[test]
fn a_condition_with_an_extra_field_is_refused() {
    let f = fixture();
    let e = parse_tree(&json!({ "op": "and", "children": [f["extra_field"].clone()] })).unwrap_err();
    assert_eq!(e[0].path, vec![0]);
    assert_eq!(e[0].field, "lax");
}
